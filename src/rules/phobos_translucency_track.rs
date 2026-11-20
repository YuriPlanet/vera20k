//! Phobos `Animatable<TranslucencyLevel>` for `Translucency=` and
//! `Translucency.Cloaked=` (`Ext/AnimType/Body.cpp:113-114`,
//! `Utilities/Template.h:560-594`).
//!
//! Both keys are read as a keyframe track, not as one integer:
//! `Translucency.KeyframeN.Percentage`, `Translucency.KeyframeN.Absolute`,
//! `Translucency.KeyframeN.Value` and `Translucency.Interpolation` (the
//! cloaked key prefixes the same suffixes with `Translucency.Cloaked`). The
//! animation queries the track at `currentFrame / End`
//! (`AnimClass_DrawIt_Translucency`, hook `0x423183`,
//! `Ext/Anim/Hooks.cpp:573-615`).
//!
//! Native reader: `Animatable<T>::Read` at `Utilities/TemplateDef.h:2446-2524`,
//! the entry at `:2330-2368`, the vector walk at `:2275-2314`, and
//! `Get(percentage)` at `:2390-2441`. The interpolated cast is
//! `interpolate<TranslucencyLevel>` (`Utilities/Interpolation.h:57-64`) and the
//! `0/25/50/75`-only level map is `Utilities/Constructs.h:464-501`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use std::hash::{Hash, Hasher};

use crate::rules::ini_parser::IniSection;

/// `InterpolationMode` (`Utilities/Enum.h:440-444`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InterpolationMode {
    /// The enum's first value; an absent or unknown `Interpolation` key keeps
    /// it (`Utilities/TemplateDef.h:1354-1376`).
    #[default]
    None,
    Linear,
}

/// One `Animatable<T>::KeyframeDataEntry` (`Utilities/Template.h:566-577`).
#[derive(Debug, Clone, Copy)]
pub struct TranslucencyKeyframe {
    /// `KeyframeN.Percentage`, or `KeyframeN.Absolute / End` when the absolute
    /// length is positive (`Utilities/TemplateDef.h:2336-2363`).
    pub percentage: f64,
    /// Native `TranslucencyLevel.GetIntValue()`: `0`, `25`, `50` or `75`
    /// (`Utilities/Constructs.h:513-533`). Every other authored integer maps
    /// to `0`, which is `BlitterFlags::None` (`:481-501`).
    pub level: u8,
}

/// Config identity compares the percentage bit-exactly, like the
/// `NativeF64Bits` fields of the same config; a re-read that changes a
/// keyframe must invalidate any cache keyed by the config.
impl PartialEq for TranslucencyKeyframe {
    fn eq(&self, other: &Self) -> bool {
        self.percentage.to_bits() == other.percentage.to_bits() && self.level == other.level
    }
}

impl Eq for TranslucencyKeyframe {}

impl Hash for TranslucencyKeyframe {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.percentage.to_bits().hash(state);
        self.level.hash(state);
    }
}

/// `Animatable<TranslucencyLevel>`, sorted for [`Self::get`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TranslucencyTrack {
    /// Ascending by percentage, as `SortedKeyFrames` is after
    /// `Utilities/TemplateDef.h:2510-2524`.
    keyframes: Vec<TranslucencyKeyframe>,
    interpolation: InterpolationMode,
}

impl TranslucencyTrack {
    /// `HasValues()` (`Utilities/TemplateDef.h:2384-2388`).
    pub fn has_values(&self) -> bool {
        !self.keyframes.is_empty()
    }

    pub fn keyframes(&self) -> &[TranslucencyKeyframe] {
        &self.keyframes
    }

    /// `Animatable<T>::Read` (`Utilities/TemplateDef.h:2446-2524`).
    ///
    /// `base` is the full key prefix (`Translucency` or `Translucency.Cloaked`)
    /// and `absolute_length` the animation type's `End` at read time.
    ///
    /// ART is read once into a fresh config, so the vector starts empty and
    /// `MultiflagVectorHelpers::ShouldResetValues` (`:2264-2272`) — the
    /// `<base>.Keyframe.ResetValues` token that would clear a retained vector —
    /// has nothing to clear here.
    pub(crate) fn read(
        section: &IniSection,
        base: &str,
        absolute_length: i32,
        require_parsed_fallback: bool,
    ) -> Self {
        let keyframe_base = format!("{base}.Keyframe");
        let mut keyframes = Vec::new();
        let mut index = 0usize;
        // `ReadVectorBase` (`:2275-2300`) stops at the first index whose entry
        // read fails.
        while let Some(keyframe) = read_keyframe(section, &keyframe_base, index, absolute_length) {
            keyframes.push(keyframe);
            index += 1;
        }
        let interpolation = read_interpolation(section, base);
        if keyframes.is_empty() {
            // `:2463-2478`: with no keyframes, the plain key's level becomes a
            // single entry at percentage `0.0`. The cloaked key passes
            // `requireParsedFallback=true`, so it stays empty (and the draw
            // keeps using the plain track) when its own plain key did not
            // parse.
            let plain = section.read_int_value(base);
            if !require_parsed_fallback || plain.is_some() {
                keyframes.push(TranslucencyKeyframe {
                    percentage: 0.0,
                    level: plain.map_or(0, level_from_int),
                });
            }
        } else {
            keyframes = validated(keyframes, base);
        }
        Self {
            keyframes,
            interpolation,
        }
    }

    /// `Get(percentage)` (`Utilities/TemplateDef.h:2390-2441`).
    ///
    /// `None` is native `BlitterFlags::None`, which the draw arm treats as
    /// "no track answer" and falls back to the stock ladder.
    ///
    /// VERA evaluates on every call instead of filling native's
    /// `KeyframeValueCache` (`:2402-2406`, `:2437-2438`): the cache only
    /// repeats the same answer for the same percentage, so dropping it cannot
    /// change a draw and keeps the evaluation a pure function of the track and
    /// the percentage.
    pub fn get(&self, percentage: f64) -> Option<u8> {
        if self.keyframes.is_empty() {
            return None;
        }
        // `:2398-2400`: the fallback entry answers at every percentage.
        if self.keyframes.len() == 1 && self.keyframes[0].percentage == 0.0 {
            return level_answer(self.keyframes[0].level);
        }
        // `std::lower_bound` (`:2408-2417`): the first entry whose percentage
        // is not below the query, then one step back. A query below the first
        // entry — and a NaN query, which fails every comparison — leaves the
        // iterator at `begin()` and the native match at its default value.
        let index = self
            .keyframes
            .partition_point(|entry| entry.percentage < percentage);
        if index == 0 {
            return None;
        }
        let start = self.keyframes[index - 1];
        let mut level = start.level;
        if self.interpolation == InterpolationMode::Linear {
            if let Some(next) = self.keyframes.get(index) {
                let progress =
                    (percentage - start.percentage) / (next.percentage - start.percentage);
                // `interpolate<TranslucencyLevel>` (`Utilities/Interpolation.h:57-64`):
                // the `double` linear form, cast to `int` (truncation toward
                // zero), then `TranslucencyLevel(value, true)`.
                let value =
                    f64::from(level) + (f64::from(next.level) - f64::from(level)) * progress;
                level = clamp_level(value as i32);
            }
        }
        level_answer(level)
    }
}

/// `%s.Percentage` / `%s.Absolute` / `%s.Value` of one entry
/// (`Utilities/TemplateDef.h:2330-2368`).
fn read_keyframe(
    section: &IniSection,
    keyframe_base: &str,
    index: usize,
    absolute_length: i32,
) -> Option<TranslucencyKeyframe> {
    let key = format!("{keyframe_base}{index}");
    // `Nullable<double>` (`Utilities/TemplateDef.h:1834-1849`): `<default>`,
    // `none`/`<none>` and a failed conversion leave it unset.
    let percentage = section.read_phobos_nullable_double(&format!("{key}.Percentage"), None);
    if absolute_length > 0 {
        if let Some(absolute) = section.read_phobos_nullable_int(&format!("{key}.Absolute"), None) {
            // `:2350-2354`: an absolute entry replaces the percentage and
            // skips the non-absolute test.
            return Some(TranslucencyKeyframe {
                percentage: f64::from(absolute) / f64::from(absolute_length),
                level: read_level(section, &key),
            });
        }
    }
    // `:2357-2363`: without a usable percentage the entry fails. A reused
    // entry would first fall back to its stored percentage (`:2342-2343`);
    // this read always builds fresh entries, whose stored percentage is the
    // `-1.0` default.
    let percentage = percentage?;
    if percentage < 0.0 {
        return None;
    }
    Some(TranslucencyKeyframe {
        percentage,
        level: read_level(section, &key),
    })
}

/// `%s.Value` as `Valueable<TranslucencyLevel>` (`Utilities/Constructs.cpp:232-242`):
/// the integer grammar reads `0/25/50/75` exactly and maps every other integer
/// to `BlitterFlags::None`; a failed read keeps the value, which is the
/// default `None` here.
fn read_level(section: &IniSection, key: &str) -> u8 {
    section
        .read_int_value(&format!("{key}.Value"))
        .map_or(0, level_from_int)
}

/// `%s.Interpolation` (`Utilities/TemplateDef.h:1354-1376`): a
/// case-insensitive `none`/`linear` token. An absent key or any other token
/// leaves the enum unchanged, i.e. `None`.
fn read_interpolation(section: &IniSection, base: &str) -> InterpolationMode {
    let raw = section.read_string(&format!("{base}.Interpolation"), "", 2048);
    if raw.eq_ignore_ascii_case("linear") {
        InterpolationMode::Linear
    } else {
        InterpolationMode::None
    }
}

/// `TranslucencyLevel::operator=(int)` (`Utilities/Constructs.h:481-501`).
fn level_from_int(value: i32) -> u8 {
    match value {
        25 => 25,
        50 => 50,
        75 => 75,
        _ => 0,
    }
}

/// `TranslucencyLevel(value, true)` (`Utilities/Constructs.h:464-478`): the
/// interpolated integer is bucketed up to the next supported level.
fn clamp_level(value: i32) -> u8 {
    if value >= 75 {
        75
    } else if value >= 50 {
        50
    } else if value >= 25 {
        25
    } else {
        0
    }
}

/// `0` is `BlitterFlags::None`, which is native's "no level" answer; only the
/// three translucent stages survive.
fn level_answer(level: u8) -> Option<u8> {
    matches!(level, 25 | 50 | 75).then_some(level)
}

/// `Utilities/TemplateDef.h:2480-2524`: native logs a developer warning for a
/// duplicated or negative percentage and then ends the process. VERA keeps the
/// first occurrence, drops the offending entry and continues, so one authored
/// mistake cannot take the game down; this is the package's one intentional
/// divergence from the native fatal.
fn validated(keyframes: Vec<TranslucencyKeyframe>, base: &str) -> Vec<TranslucencyKeyframe> {
    let mut seen: Vec<u64> = Vec::with_capacity(keyframes.len());
    let mut kept = Vec::with_capacity(keyframes.len());
    for keyframe in keyframes {
        let bits = keyframe.percentage.to_bits();
        if keyframe.percentage < 0.0 || seen.contains(&bits) {
            #[cfg(debug_assertions)]
            eprintln!(
                "[VERA] {base} keyframe {} is duplicated or negative; the entry is dropped",
                keyframe.percentage
            );
            continue;
        }
        seen.push(bits);
        kept.push(keyframe);
    }
    kept.sort_by(|left, right| left.percentage.total_cmp(&right.percentage));
    kept
}
