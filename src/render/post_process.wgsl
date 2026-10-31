// The presentation hook's frame pass.
//
// `textureLoad`, never a sampler: with no effects configured this pass must
// reproduce the 1:1 texture copy it replaces bit for bit, so it reads the
// texel the fragment centre lands in and applies no filtering, no half-texel
// offset and no colour conversion the copy would not also apply.
//
// The optional arms:
//   - ColorGrade runs in linear RGB on the decoded texel;
//   - FilmicPass, Colourfulness and Sepia are ports of the ReShade community
//     shaders of those names, which are authored in gamma space. They run after
//     an explicit linear-to-sRGB encode and the result is decoded back for the
//     sRGB target.
// With every arm off the loaded texel is returned untouched.

struct Params {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    // 0 = no colour grade, 1 = colour grade.
    color_grade: f32,

    // (x, y, width, height) of the tactical viewport in frame pixels. Texels
    // outside it (sidebar column, bottom strip) return unchanged.
    viewport: vec4f,

    filmic_strength: f32,
    filmic_fade: f32,
    filmic_contrast: f32,
    filmic_linearization: f32,

    filmic_bleach: f32,
    filmic_saturation: f32,
    // 0 = no FilmicPass, 1 = FilmicPass.
    filmic_enabled: f32,
    pad0: f32,

    colour_amount: f32,
    colour_lim_luma: f32,
    // 0 = no Colourfulness, 1 = Colourfulness.
    colour_enabled: f32,
    // 0 = no Sepia, 1 = Sepia.
    sepia_enabled: f32,

    sepia_strength: f32,
    sepia_tint_r: f32,
    sepia_tint_g: f32,
    sepia_tint_b: f32,
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;

// ReShade `FilmicPass.fx` luminance coefficients.
const FILMIC_LUMA = vec3f(0.212656, 0.715158, 0.072186);
// ReShade `Colourfulness.fx` mean of Rec. 709 & 601 luma coefficients.
const COLOUR_LUMA = vec3f(0.2558, 0.6511, 0.0931);
// FilmicPass curve/gamma constants left at their authored defaults.
const FILMIC_CURVE = 1.0;
const FILMIC_BASE_CURVE = 1.5;
const FILMIC_EFFECT_GAMMA = 0.65;

fn sigmoid(x: f32) -> f32 {
    return 1.0 / (1.0 + exp(-x));
}

fn curve_norm(curve: f32) -> f32 {
    return 1.0 / (1.0 + exp(curve / 2.0));
}

fn linear_to_srgb(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

fn srgb_to_linear(c: vec3f) -> vec3f {
    let x = clamp(c, vec3f(0.0), vec3f(1.0));
    let lo = x / 12.92;
    let hi = pow((x + 0.055) / 1.055, vec3f(2.4));
    return select(hi, lo, x <= vec3f(0.04045));
}

// The quadratic/sqrt toe used by FilmicPass for a single channel.
fn filmic_tone(x: f32, b: f32) -> f32 {
    if (x < 0.5) {
        return (2.0 * x - 1.0) * (b - b * b) + b;
    }
    return (2.0 * x - 1.0) * (sqrt(b) - b) + b;
}

// Port of ReShade `FilmicPass.fx`; curve/gamma constants stay at defaults.
fn filmic_pass(b0: vec3f) -> vec3f {
    let h = vec3f(0.01);
    var b = clamp(b0, vec3f(0.0), vec3f(1.0));
    b = pow(b, vec3f(params.filmic_linearization));
    b = h + (b - h) * params.filmic_contrast;

    // BaseGamma is 1.0, so the base keeps its values.
    let a_luma = dot(b, FILMIC_LUMA);
    var d = vec3f(a_luma);
    let y = curve_norm(FILMIC_CURVE);
    d.x = (sigmoid(FILMIC_CURVE * (d.x - 0.5)) - y) / (1.0 - 2.0 * y);
    d.y = (sigmoid(FILMIC_CURVE * (d.y - 0.5)) - y) / (1.0 - 2.0 * y);
    d.z = (sigmoid(FILMIC_CURVE * (d.z - 0.5)) - y) / (1.0 - 2.0 * y);

    d = pow(abs(d), vec3f(1.0 / FILMIC_EFFECT_GAMMA));
    d = mix(d, vec3f(1.0) - d, params.filmic_bleach);

    var c = vec3f(
        filmic_tone(d.x, b.x),
        filmic_tone(d.y, b.y),
        filmic_tone(d.z, b.z),
    );
    var f = mix(b, c, params.filmic_strength);

    let v = curve_norm(FILMIC_BASE_CURVE);
    f = vec3f(
        (sigmoid(FILMIC_BASE_CURVE * (f.x - 0.5)) - v) / (1.0 - 2.0 * v),
        (sigmoid(FILMIC_BASE_CURVE * (f.y - 0.5)) - v) / (1.0 - 2.0 * v),
        (sigmoid(FILMIC_BASE_CURVE * (f.z - 0.5)) - v) / (1.0 - 2.0 * v),
    );

    let sat = params.filmic_saturation;
    let fade = params.filmic_fade;
    let r2r = 1.0 - sat;
    let g2r = sat;
    let b2r = sat;
    let r2g = sat;
    let g2g = (1.0 - fade) - sat;
    let b2g = fade + sat;
    let r2b = sat;
    let g2b = fade + sat;
    let b2b = (1.0 - fade) - sat;
    let i_f = f;
    f = vec3f(
        i_f.x * r2r + i_f.y * g2r + i_f.z * b2r,
        i_f.x * r2g + i_f.y * g2g + i_f.z * b2g,
        i_f.x * r2b + i_f.y * g2b + i_f.z * b2b,
    );

    let n = dot(f, FILMIC_LUMA);
    c = vec3f(
        filmic_tone(n, f.x),
        filmic_tone(n, f.y),
        filmic_tone(n, f.z),
    );
    c = pow(max(c, vec3f(0.0)), vec3f(1.0 / params.filmic_linearization));
    return mix(b, c, params.filmic_strength);
}

// Port of ReShade `Colourfulness.fx` (dither off).
fn colourfulness_pass(c_in: vec3f) -> vec3f {
    let c0 = clamp(c_in, vec3f(0.0), vec3f(1.0));
    let luma = sqrt(dot(c0 * abs(c0), COLOUR_LUMA));
    let diff = c0 - vec3f(luma);
    var c_diff = diff * (params.colour_amount + 1.0) - diff;

    if (params.colour_amount > 0.0) {
        let rlc = clamp(c_diff * 1.2 + c0, vec3f(-0.0001), vec3f(1.0001)) - c0;
        let max_diff = max(max(diff.x, diff.y), diff.z);
        let min_diff = min(min(diff.x, diff.y), diff.z);
        let pos_lim = (1.0002 - luma) / (abs(max_diff) + 0.0001);
        let neg_lim = (luma + 0.0002) / (abs(min_diff) + 0.0001);
        let lim_factor = min(min(pos_lim, neg_lim), 32.0);
        let diff_max = diff * lim_factor - diff;

        let w = clamp(params.colour_lim_luma, 0.0, 1.0);
        let mean = pow(abs(w) * sqrt(abs(diff_max)) + abs(1.0 - w) * sqrt(abs(rlc)), vec3f(2.0));
        let soft = max(mean, vec3f(1e-7));
        c_diff = (c_diff * soft) / sqrt(soft * soft + c_diff * c_diff);
    }
    return clamp(c0 + c_diff, vec3f(0.0), vec3f(1.0));
}

// Port of ReShade `Sepia.fx` (`technique Tint`).
fn sepia_pass(c: vec3f) -> vec3f {
    let tint = vec3f(params.sepia_tint_r, params.sepia_tint_g, params.sepia_tint_b);
    return mix(c, c * tint * 2.55, params.sepia_strength);
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
    // One triangle that covers the whole target.
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let source = textureLoad(t_source, vec2i(position.xy), 0);

    if (params.color_grade == 0.0
        && params.filmic_enabled == 0.0
        && params.colour_enabled == 0.0
        && params.sepia_enabled == 0.0) {
        return source;
    }

    let p = position.xy;
    let v = params.viewport;
    if (p.x < v.x || p.y < v.y || p.x >= v.x + v.z || p.y >= v.y + v.w) {
        return source;
    }

    var color = source;
    if (params.color_grade != 0.0) {
        var rgb = color.rgb;
        let luma = dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
        rgb = mix(vec3f(luma), rgb, params.saturation);
        rgb = (rgb - vec3f(0.5)) * params.contrast + vec3f(0.5);
        rgb = rgb + vec3f(params.brightness);
        color = vec4f(clamp(rgb, vec3f(0.0), vec3f(1.0)), color.a);
    }

    if (params.filmic_enabled != 0.0
        || params.colour_enabled != 0.0
        || params.sepia_enabled != 0.0) {
        var gamma = linear_to_srgb(color.rgb);
        if (params.filmic_enabled != 0.0) {
            gamma = filmic_pass(gamma);
        }
        if (params.colour_enabled != 0.0) {
            gamma = colourfulness_pass(gamma);
        }
        if (params.sepia_enabled != 0.0) {
            gamma = sepia_pass(gamma);
        }
        color = vec4f(srgb_to_linear(gamma), color.a);
    }
    return color;
}
