//! The Anime4K plans: convolution weights and pass wiring read from TC2's
//! `A4K_*.hlsl` effect files (`src/render/anime4k/`).
//!
//! The effect pack is machine-written in one shape. Every pass reads one RGBA
//! layer (or several), convolves a 3x3 (or 1x1) neighbourhood with
//! `mul(<tap>, float{3,4}x4(<weights>))`, adds a bias, and splits each input
//! into its positive and negative halves (`max(x, 0)` / `max(-x, 0)`), so a
//! leaky ReLU is two separate linear maps. The last pass turns its channels
//! into the 2x2 output block (depth to space). This module reduces all of that
//! to flat weight tables the shader reads, keeping the shipped `.hlsl` files as
//! the single source of truth for every tier.

use crate::rules::post_process::Anime4kTier;

/// One tier's complete network.
pub(crate) struct Network {
    pub(crate) passes: Vec<Pass>,
    /// Intermediate RGBA layers, `INPUT` included (it is layer 0).
    pub(crate) layers: usize,
}

/// One convolution pass.
pub(crate) struct Pass {
    /// Layer ids read by the convolution, in group order.
    pub(crate) inputs: Vec<usize>,
    /// Layer ids written by the pass; empty on the final upsample.
    pub(crate) outputs: Vec<usize>,
    /// 9 for a 3x3 kernel, 1 for 1x1.
    pub(crate) taps: u32,
    /// The first pass is linear: no activation, so negatives are kept.
    pub(crate) linear: bool,
    /// `[out][in][tap][sign][4 out][4 in]` — 32 lanes per channel pair.
    pub(crate) weights: Vec<f32>,
    /// One RGBA bias per output group, stored after the matrices.
    pub(crate) bias: Vec<f32>,
    /// Output channel groups on the final pass (one RGBA value per 2x2 corner).
    pub(crate) targets: u32,
    /// The pass that writes the 2x frame.
    pub(crate) upsample: bool,
}

/// The effect source of a tier.
fn source(tier: Anime4kTier) -> &'static str {
    match tier {
        Anime4kTier::S => include_str!("a4k_s.hlsl"),
        Anime4kTier::L => include_str!("a4k_l.hlsl"),
        Anime4kTier::Vl => include_str!("a4k_vl.hlsl"),
        Anime4kTier::Ul => include_str!("a4k_ul.hlsl"),
        Anime4kTier::Off => "",
    }
}

/// One `mul()` term of the effect code.
struct Term {
    target: usize,
    texture: String,
    offset: (i32, i32),
    negative: bool,
    rows: usize,
    matrix: [f32; 16],
}

/// Parse a tier's weights and wiring.
pub(crate) fn parse(tier: Anime4kTier) -> Result<Network, String> {
    let text = source(tier);
    let sections: Vec<&str> = text.split("//!PASS ").skip(1).collect();
    if sections.len() < 2 {
        return Err(format!("{}: no passes", tier.label()));
    }
    let mut names = vec!["INPUT".to_string()];
    let mut passes = Vec::new();
    for section in sections {
        passes.push(parse_pass(section, &mut names)?);
    }
    Ok(Network {
        passes,
        layers: names.len(),
    })
}

fn parse_pass(section: &str, names: &mut Vec<String>) -> Result<Pass, String> {
    let inputs_line = list(section, "//!IN ");
    let outputs_line = list(section, "//!OUT ");
    let upsample = outputs_line.iter().any(|name| name == "OUTPUT");

    let mut defs: Vec<(String, String, (i32, i32))> = Vec::new();
    for line in section.lines() {
        if let Some(definition) = definition(line) {
            defs.push(definition);
        }
    }

    let mut terms = Vec::new();
    let mut target_count = 0;
    let mut activated = false;
    for line in section.lines() {
        let Some(target) = target_index(line) else {
            continue;
        };
        target_count = target_count.max(target + 1);
        if let Some((var, matrix)) = term(line) {
            activated |= var.trim_start().starts_with("max(");
            let (inner, negative) = split_sign(&var);
            let (texture, offset) = resolve(inner, &defs, &inputs_line)
                .ok_or_else(|| format!("unresolved tap `{var}`"))?;
            let rows = matrix.rows;
            terms.push(Term {
                target,
                texture,
                offset,
                negative,
                rows,
                matrix: matrix.values,
            });
        }
    }

    if let Some(term) = terms
        .iter()
        .find(|term| term.offset.0.abs() > 1 || term.offset.1.abs() > 1)
    {
        return Err(format!(
            "tap offset {:?} outside 3x3: {section:.40}",
            term.offset
        ));
    }
    let taps = if terms.iter().any(|term| term.offset != (0, 0)) {
        9
    } else {
        1
    };

    // Group order is the order the effect code samples textures in, which is
    // also the order `//!IN` lists them.
    let mut inputs: Vec<String> = Vec::new();
    for term in &terms {
        if !inputs.iter().any(|name| name == &term.texture) {
            inputs.push(term.texture.clone());
        }
    }
    let group_of = |texture: &str| inputs.iter().position(|name| name == texture).unwrap_or(0);

    let outputs: Vec<String> = outputs_line
        .iter()
        .filter(|name| name != &"OUTPUT")
        .cloned()
        .collect();
    let groups = if upsample {
        target_count
    } else {
        outputs.len()
    };
    if groups == 0 {
        return Err(format!("pass without outputs: {section:.40}"));
    }

    let mut weights = vec![0.0f32; groups * inputs.len() * taps as usize * 32];
    let mut bias = vec![0.0f32; groups * 4];
    for term in &terms {
        let tap = if taps == 1 {
            0
        } else {
            ((term.offset.1 + 1) * 3 + (term.offset.0 + 1)) as usize
        };
        let base = (((term.target * inputs.len() + group_of(&term.texture)) * taps as usize + tap)
            * 32)
            + if term.negative { 16 } else { 0 };
        // `mul(v, float{3,4}x4)` is a row vector times a row-major matrix:
        // output lane `c` takes column `c`, i.e. literal `i * 4 + c`.
        for lane in 0..4 {
            for channel in 0..term.rows {
                weights[base + lane * 4 + channel] = term.matrix[channel * 4 + lane];
            }
        }
    }
    if let Some(collected) = biases(section, groups) {
        bias = collected;
    }

    let mut input_ids = Vec::new();
    for name in &inputs {
        input_ids.push(id_of(name, names));
    }
    let mut output_ids = Vec::new();
    for name in &outputs {
        output_ids.push(id_of(name, names));
    }

    Ok(Pass {
        inputs: input_ids,
        outputs: output_ids,
        taps,
        linear: !activated,
        weights,
        bias,
        targets: groups as u32,
        upsample,
    })
}

/// The `//!KEY` name list of a section.
fn list(section: &str, key: &str) -> Vec<String> {
    for line in section.lines() {
        if let Some(rest) = line.trim().strip_prefix(key) {
            return rest
                .split(',')
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// `float4 <name> = <texture>.SampleLevel(sam[, pos + float2(dx, dy)], 0);`
fn definition(line: &str) -> Option<(String, String, (i32, i32))> {
    let rest = line.trim().strip_prefix("float4 ")?;
    let (left, right) = rest.split_once('=')?;
    let name = left.trim().to_string();
    if !right.contains("SampleLevel") {
        return None;
    }
    let right = right.trim();
    let texture = right.split('.').next()?.trim().to_string();
    let offset = match right.find("float2(") {
        Some(at) => offset_of(&right[at + "float2(".len()..])?,
        None => (0, 0),
    };
    Some((name, texture, offset))
}

/// The `float2(<x>, <y>)` tap offset, where the effect code writes either `0`
/// or `±inputPt.<axis>`.
fn offset_of(rest: &str) -> Option<(i32, i32)> {
    let inner = &rest[..rest.find(')')?];
    let mut parts = inner.split(',');
    let x = axis(parts.next()?)?;
    let y = axis(parts.next()?)?;
    Some((x, y))
}

fn axis(token: &str) -> Option<i32> {
    let token = token.trim();
    if token == "0" {
        return Some(0);
    }
    if token.contains("inputPt.") {
        return Some(if token.starts_with('-') { -1 } else { 1 });
    }
    None
}

/// The `target<n>` (or the effect code's `result`) a line accumulates into.
fn target_index(line: &str) -> Option<usize> {
    let mut tokens = line.split_whitespace();
    let first = tokens.next()?;
    let name = if first == "float4" {
        let name = tokens.next()?;
        if tokens.next()? != "=" {
            return None;
        }
        name
    } else {
        first
    };
    if name == "result" {
        return Some(0);
    }
    name.strip_prefix("target")?.parse::<usize>().ok()?.checked_sub(1)
}

/// The tap variable and its matrix literal of a `mul()` line.
fn term(line: &str) -> Option<(String, Matrix)> {
    let mul_at = line.find("mul(")?;
    let float_at = line[mul_at..].find("float")? + mul_at;
    let var = line[mul_at + "mul(".len()..float_at]
        .trim()
        .trim_end_matches(',')
        .trim()
        .to_string();
    let open = float_at + line[float_at..].find('(')? + 1;
    let close = open + line[open..].find(')')?;
    let rows = line[float_at..float_at + 6]
        .chars()
        .nth(5)
        .and_then(|digit| digit.to_digit(10))
        .filter(|rows| (3..=4).contains(rows))? as usize;
    let mut matrix = [0.0f32; 16];
    let mut values = line[open..close].split(',');
    for index in 0..rows * 4 {
        matrix[index] = values.next()?.trim().parse().ok()?;
    }
    Some((
        var,
        Matrix {
            rows,
            values: matrix,
        },
    ))
}

struct Matrix {
    rows: usize,
    values: [f32; 16],
}

/// Strip the `max(<expression>, 0)` wrapper and its sign.
fn split_sign(var: &str) -> (&str, bool) {
    let Some(rest) = var.strip_prefix("max(") else {
        return (var, false);
    };
    let rest = rest
        .trim()
        .trim_end_matches(')')
        .trim_end_matches(", 0")
        .trim();
    match rest.strip_prefix('-') {
        Some(positive) => (positive.trim(), true),
        None => (rest, false),
    }
}

/// The texture and texel offset a tap variable names. Gather-style code taps
/// the pass's own input as `src[i + dx][j + dy]`; element-style code names a
/// sample defined earlier in the pass.
fn resolve(
    var: &str,
    defs: &[(String, String, (i32, i32))],
    inputs: &[String],
) -> Option<(String, (i32, i32))> {
    if var.starts_with("src[") {
        let texture = inputs.first()?.clone();
        return Some((texture, src_offset(var)?));
    }
    if let Some((_, texture, offset)) = defs.iter().find(|(name, ..)| name == var) {
        return Some((texture.clone(), *offset));
    }
    let positive = var.strip_prefix('n')?;
    let (_, texture, offset) = defs.iter().find(|(name, ..)| name == positive)?;
    Some((texture.clone(), *offset))
}

/// `src[i + dx][j + dy]`: the third index is x, the fourth y (see the
/// `// w z / x y` comment in the effect files), so the tap sits at
/// `(dx, dy)` from the destination texel.
fn src_offset(var: &str) -> Option<(i32, i32)> {
    let rest = var.strip_prefix("src[")?;
    let (first, rest) = rest.split_once("][")?;
    let second = rest.trim_end_matches(']');
    Some((index_delta(first)?, index_delta(second)?))
}

fn index_delta(expression: &str) -> Option<i32> {
    let expression = expression.trim();
    if expression == "i" || expression == "j" {
        return Some(0);
    }
    let (_, delta) = expression.split_once(' ')?;
    Some(if delta.trim().starts_with('-') { -1 } else { 1 })
}

/// The per-target biases (`target<n> += float4(...);`).
fn biases(section: &str, groups: usize) -> Option<Vec<f32>> {
    let mut bias = vec![0.0f32; groups * 4];
    let mut found = false;
    for line in section.lines() {
        if line.contains("mul(") {
            continue;
        }
        let Some(target) = target_index(line) else {
            continue;
        };
        if target >= groups {
            continue;
        }
        let Some(at) = line.find("float4(") else {
            continue;
        };
        let open = at + "float4(".len();
        let close = open + line[open..].find(')')?;
        let mut values = line[open..close].split(',');
        for lane in 0..4 {
            bias[target * 4 + lane] = values.next()?.trim().parse().ok()?;
        }
        found = true;
    }
    found.then_some(bias)
}

/// The id of a layer name, registering it on first sight.
fn id_of(name: &str, names: &mut Vec<String>) -> usize {
    if let Some(index) = names.iter().position(|known| known == name) {
        return index;
    }
    names.push(name.to_string());
    names.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passes_of(tier: Anime4kTier) -> Network {
        parse(tier).expect("the tier parses")
    }

    #[test]
    fn every_tier_parses_its_documented_shape() {
        for (tier, passes, layers) in [
            (Anime4kTier::S, 4, 3),
            (Anime4kTier::L, 4, 5),
            (Anime4kTier::Vl, 8, 15),
            (Anime4kTier::Ul, 8, 22),
        ] {
            let network = passes_of(tier);
            assert_eq!(network.passes.len(), passes, "{} passes", tier.label());
            assert_eq!(network.layers, layers, "{} layers", tier.label());
            assert!(
                network.passes.last().is_some_and(|pass| pass.upsample),
                "{} upsamples last",
                tier.label()
            );
            assert!(
                network.passes[..passes - 1].iter().all(|pass| !pass.upsample),
                "{} upsamples once",
                tier.label()
            );
        }
    }

    #[test]
    fn the_first_tap_keeps_the_effect_files_weights() {
        let network = passes_of(Anime4kTier::S);
        let first = &network.passes[0];
        // `mul(src[i - 1][j - 1], float3x4(-0.0057322932, ...))`, output lane 0.
        assert!((first.weights[0] + 0.0057322932).abs() < 1e-9);
        // The bias follows the matrices.
        assert!((first.bias[0] + 0.01050964).abs() < 1e-9);
        // A float3x4 leaves the alpha row empty.
        assert_eq!(first.weights[3], 0.0);
    }

    #[test]
    fn the_final_pass_reports_its_channel_groups() {
        let small = passes_of(Anime4kTier::S);
        let last = small.passes.last().expect("last pass");
        assert_eq!(last.targets, 1);
        assert_eq!(last.taps, 9);
        assert_eq!(last.inputs.len(), 1);
        let ultra = passes_of(Anime4kTier::Ul);
        let last = ultra.passes.last().expect("last pass");
        assert_eq!(last.targets, 3);
        assert_eq!(last.taps, 1);
        assert_eq!(last.inputs.len(), 15);
    }
}
