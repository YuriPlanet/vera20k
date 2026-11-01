// ArcaneBloom's composite: the graded frame, tone-mapped the way the effect
// does it, plus the five Gaussian levels and the adaptation exposure.
//
// Ported from luluco250's `ArcaneBloom.fx`. UI texels outside the tactical
// viewport keep the graded frame untouched, so chrome neither glows nor
// changes exposure.

struct Params {
    dst: vec2f,
    step: vec2f,
    frame: vec2f,
    _pad: vec2f,
    // intensity, max brightness, exposure, adaptation intensity
    scalar: vec4f,
    // sensitivity, adaptation seconds, frame seconds, luminance mip
    adapt: vec4f,
    limits: vec4f,
    viewport: vec4f,
};

@group(0) @binding(0) var t_scene: texture_2d<f32>;
@group(0) @binding(1) var t_l0: texture_2d<f32>;
@group(0) @binding(2) var t_l1: texture_2d<f32>;
@group(0) @binding(3) var t_l2: texture_2d<f32>;
@group(0) @binding(4) var t_l3: texture_2d<f32>;
@group(0) @binding(5) var t_l4: texture_2d<f32>;
@group(0) @binding(6) var t_adapt: texture_2d<f32>;
@group(0) @binding(7) var<uniform> params: Params;
@group(0) @binding(8) var s_linear: sampler;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn inv_reinhard(color: vec3f, inv_max: f32) -> vec3f {
    return color / max(1.0 - color, vec3f(inv_max));
}

fn reinhard(color: vec3f) -> vec3f {
    return color / (1.0 + color);
}

@fragment
fn fs_blend(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let scene = textureLoad(t_scene, vec2i(position.xy), 0).rgb;
    let v = params.viewport;
    if (position.x < v.x
        || position.y < v.y
        || position.x >= v.x + v.z
        || position.y >= v.y + v.w) {
        return vec4f(scene, 1.0);
    }
    let dims = vec2f(textureDimensions(t_scene));
    let uv = position.xy / dims;
    let max_brightness = max(params.scalar.y, 1.0);
    var color = inv_reinhard(scene, 1.0 / max_brightness);
    // The effect's level weights: the widest halo carries the most, the two
    // tightest levels the least.
    let weights = array<f32, 5>(1.0, 6.0 / 9.0, 3.0 / 9.0, 2.0 / 9.0, 6.0 / 9.0);
    var bloom = textureSampleLevel(t_l0, s_linear, uv, 0.0).rgb * weights[0]
        + textureSampleLevel(t_l1, s_linear, uv, 0.0).rgb * weights[1]
        + textureSampleLevel(t_l2, s_linear, uv, 0.0).rgb * weights[2]
        + textureSampleLevel(t_l3, s_linear, uv, 0.0).rgb * weights[3]
        + textureSampleLevel(t_l4, s_linear, uv, 0.0).rgb * weights[4];
    bloom = bloom * (params.scalar.x / max_brightness);
    color = color + bloom;
    let adapt = max(textureLoad(t_adapt, vec2i(0, 0), 0).x, 0.001);
    let exposure = params.scalar.z / adapt;
    color = color * mix(vec3f(1.0), vec3f(exposure), params.scalar.w);
    color = reinhard(color);
    return vec4f(clamp(color, vec3f(0.0), vec3f(1.0)), 1.0);
}
