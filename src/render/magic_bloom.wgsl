// MagicBloom (luluco250, MIT): eight Gaussian levels, each a 9x9 blur with the
// effect's precalculated kernel, thresholded at the first level only. The
// blend sums the levels, tone-maps them with Uncharted 2's curve and
// screen-blends them over the frame. An optional adaptation exposure scales
// the bloom by the image's average luminance, smoothed across frames.
//
// Lens dirt is not ported: it needs a texture the game does not ship.
//
// UI texels outside the tactical viewport keep the graded frame untouched.

struct Params {
    dst: vec2f,      // render target size in pixels
    step: vec2f,     // blur step in uv (full-frame pixel * scale / frame)
    frame: vec2f,    // graded frame size in pixels (viewport test)
    _pad: vec2f,
    // threshold, intensity, first-blur flag, unused
    scalar: vec4f,
    // adaptation sensitivity, speed, limit min, limit max
    adapt: vec4f,
    viewport: vec4f,
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var s_linear: sampler;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var t_aux: texture_2d<f32>;
@group(0) @binding(4) var t_level0: texture_2d<f32>;
@group(0) @binding(5) var t_level1: texture_2d<f32>;
@group(0) @binding(6) var t_level2: texture_2d<f32>;
@group(0) @binding(7) var t_level3: texture_2d<f32>;
@group(0) @binding(8) var t_level4: texture_2d<f32>;
@group(0) @binding(9) var t_level5: texture_2d<f32>;
@group(0) @binding(10) var t_level6: texture_2d<f32>;
@group(0) @binding(11) var t_level7: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn outside_viewport(position: vec2f) -> bool {
    let sample_position = position * params.frame;
    let v = params.viewport;
    return sample_position.x < v.x
        || sample_position.y < v.y
        || sample_position.x >= v.x + v.z
        || sample_position.y >= v.y + v.w;
}

fn luminance(color: vec3f) -> f32 {
    return dot(color, vec3f(0.2126, 0.7152, 0.0722));
}

/// The effect's precalculated 9-tap Gaussian kernel and its 1/(2*pi*sigma^2)
/// normalization constant.
const KERNEL: array<f32, 9> = array<f32, 9>(
    0.0269955, 0.0647588, 0.120985, 0.176033, 0.199471, 0.176033, 0.120985, 0.0647588, 0.0269955,
);
const KERNEL_ACCUM: f32 = 1.02352;

/// Uncharted 2's tonemapper (Hable/Naughty Dog), as the effect ports it.
fn tonemap(color: vec3f, exposure: f32) -> vec3f {
    let a = 0.15;
    let b = 0.50;
    let c = 0.10;
    let d = 0.20;
    let e = 0.02;
    let f = 0.30;
    let w = 11.2;
    var col = color * exposure;
    col = ((col * (a * col + c * b) + d * e) / (col * (a * col + b) + d * f)) - e / f;
    let white_input = w * (a * w + c * b) + d * e;
    let white_denominator = w * (a * w + b) + d * f;
    let white = 1.0 / ((white_input / white_denominator) - e / f);
    return col * white;
}

fn blend_screen(a: vec3f, b: vec3f) -> vec3f {
    return 1.0 - (1.0 - a) * (1.0 - b);
}

/// Luminance of the frame, sampled down to the adaptation chain's base size.
@fragment
fn fs_luma(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    let color = textureSampleLevel(t_source, s_linear, uv, 0.0).rgb;
    return vec4f(luminance(color), 0.0, 0.0, 1.0);
}

/// 2x2 box: four bilinear taps a quarter texel off centre average exactly the
/// four source texels.
@fragment
fn fs_luma_down(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    let texel = 0.25 / vec2f(textureDimensions(t_source));
    let sum = textureSampleLevel(t_source, s_linear, uv + vec2f(-texel.x, -texel.y), 0.0).x
        + textureSampleLevel(t_source, s_linear, uv + vec2f(texel.x, -texel.y), 0.0).x
        + textureSampleLevel(t_source, s_linear, uv + vec2f(-texel.x, texel.y), 0.0).x
        + textureSampleLevel(t_source, s_linear, uv + vec2f(texel.x, texel.y), 0.0).x;
    return vec4f(sum * 0.25, 0.0, 0.0, 1.0);
}

/// One adaptation step: sensitivity, clamp, then the effect's per-frame lerp
/// toward the previous value (no frame-time scaling, as the original).
@fragment
fn fs_adapt(@builtin(position) position: vec4f) -> @location(0) vec4f {
    // The chosen level is already a box-downsampled average; sampling its
    // centre (uv 0.5, as the effect does) keeps the read scale independent of
    // that level's size.
    let uv = vec2f(0.5);
    var current = textureSampleLevel(t_source, s_linear, uv, 0.0).x;
    current = current * params.adapt.x;
    current = clamp(current, params.adapt.z, params.adapt.w);
    let last = textureLoad(t_aux, vec2i(0, 0), 0).x;
    return vec4f(mix(last, current, params.adapt.y), 0.0, 0.0, 1.0);
}

/// The effect's single-pass 9x9 Gaussian. The threshold and intensity apply
/// only to the first level, as the original does.
@fragment
fn fs_blur(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    var color = vec3f(0.0);
    for (var x = -4; x <= 4; x += 1) {
        for (var y = -4; y <= 4; y += 1) {
            let weight = KERNEL[x + 4] * KERNEL[y + 4];
            let tap = uv + params.step * vec2f(f32(x), f32(y));
            color += textureSampleLevel(t_source, s_linear, tap, 0.0).rgb * weight;
        }
    }
    color = color * KERNEL_ACCUM;
    if (params.scalar.z > 0.5) {
        color = pow(abs(color), vec3f(params.scalar.x)) * params.scalar.y;
    }
    return vec4f(color, 1.0);
}

/// Sum the eight levels, tone-map with the adapted exposure, then screen-blend
/// over the graded frame.
@fragment
fn fs_blend(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let scene = textureLoad(t_source, vec2i(position.xy), 0).rgb;
    if (outside_viewport(position.xy / vec2f(textureDimensions(t_source)))) {
        return vec4f(scene, 1.0);
    }
    let uv = position.xy / params.dst;
    var bloom = textureSampleLevel(t_level0, s_linear, uv, 0.0).rgb
        + textureSampleLevel(t_level1, s_linear, uv, 0.0).rgb
        + textureSampleLevel(t_level2, s_linear, uv, 0.0).rgb
        + textureSampleLevel(t_level3, s_linear, uv, 0.0).rgb
        + textureSampleLevel(t_level4, s_linear, uv, 0.0).rgb
        + textureSampleLevel(t_level5, s_linear, uv, 0.0).rgb
        + textureSampleLevel(t_level6, s_linear, uv, 0.0).rgb
        + textureSampleLevel(t_level7, s_linear, uv, 0.0).rgb;
    bloom = bloom * 0.125;
    let adapt_value = max(textureLoad(t_aux, vec2i(0, 0), 0).x, 0.00001);
    let exposure = params._pad.x / adapt_value;
    bloom = tonemap(bloom, exposure);
    let out = blend_screen(scene, bloom);
    return vec4f(clamp(out, vec3f(0.0), vec3f(1.0)), 1.0);
}
