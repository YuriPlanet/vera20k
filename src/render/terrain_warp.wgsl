// Terrain-only refraction. Samples the terrain colour target in screen space
// and shifts it by the engine water field; everything drawn after the terrain
// pass is untouched.

struct Warp {
    amplitude: f32,
    width: f32,
    height: f32,
    _pad: f32,
};

@group(0) @binding(0) var t_terrain: texture_2d<f32>;
@group(0) @binding(1) var s_linear: sampler;
@group(0) @binding(2) var t_field: texture_2d<f32>;
@group(0) @binding(3) var<uniform> warp: Warp;

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((idx << 1u) & 2u), f32(idx & 2u));
    return vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let dims = vec2f(warp.width, warp.height);
    let uv = position.xy / dims;
    // Neutral (0.5, 0.5) on land and under fog, so only water moves.
    let field = textureSampleLevel(t_field, s_linear, uv, 0.0).rg;
    let offset = (field - vec2f(0.5)) * 2.0 * warp.amplitude;
    let warped = clamp(uv + offset / dims, vec2f(0.0), vec2f(1.0));
    return textureSampleLevel(t_terrain, s_linear, warped, 0.0);
}
