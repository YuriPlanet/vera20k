// Bloom composite: the graded frame plus the accumulated bloom level.
//
// `t_scene` is the intermediate sRGB frame; `textureLoad` decodes it to linear
// exactly as the presentation copy would. `t_bloom` is the linear RGBA16Float
// level 0, sampled with the linear filter so its half-resolution texels spread
// smoothly.

struct BloomParams {
    threshold: f32,
    intensity: f32,
    radius: f32,
    pad0: f32,
    dst_width: f32,
    dst_height: f32,
    pad1: f32,
    pad2: f32,
    // Tactical viewport in frame pixels; UI texels keep the copy path.
    viewport: vec4f,
};

@group(0) @binding(0) var t_scene: texture_2d<f32>;
@group(0) @binding(1) var t_bloom: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: BloomParams;
@group(0) @binding(3) var s_linear: sampler;

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_composite(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let scene = textureLoad(t_scene, vec2i(position.xy), 0);
    let v = params.viewport;
    if (position.x < v.x || position.y < v.y
        || position.x >= v.x + v.z || position.y >= v.y + v.w) {
        return scene;
    }
    let dims = vec2f(textureDimensions(t_scene));
    let bloom = textureSampleLevel(t_bloom, s_linear, position.xy / dims, 0.0).rgb;
    let rgb = clamp(scene.rgb + bloom * params.intensity, vec3f(0.0), vec3f(1.0));
    return vec4f(rgb, scene.a);
}
