// Bloom downsample and upsample passes.
//
// `t_input` is either the composed frame (first threshold pass) or the previous
// bloom level. All passes keep linear RGB in an RGBA16Float target; only the
// threshold reads the display-luma of the sRGB-encoded frame, matching
// ReShade's LDR threshold tuning.

struct BloomParams {
    threshold: f32,
    intensity: f32,
    radius: f32,
    pad0: f32,
    // Size of the pass's render target, in pixels.
    dst_width: f32,
    dst_height: f32,
    pad1: f32,
    pad2: f32,
    // Tactical viewport in frame pixels; the bright pass ignores UI texels.
    viewport: vec4f,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: BloomParams;
@group(0) @binding(2) var s_linear: sampler;

fn linear_to_srgb(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

fn box_sample(uv: vec2f) -> vec3f {
    let dims = vec2f(textureDimensions(t_input));
    let rcp = vec2f(0.5 / dims.x, 0.5 / dims.y);
    return (textureSampleLevel(t_input, s_linear, uv + vec2f(-rcp.x, -rcp.y), 0.0)
        + textureSampleLevel(t_input, s_linear, uv + vec2f(rcp.x, -rcp.y), 0.0)
        + textureSampleLevel(t_input, s_linear, uv + vec2f(-rcp.x, rcp.y), 0.0)
        + textureSampleLevel(t_input, s_linear, uv + vec2f(rcp.x, rcp.y), 0.0)).rgb * 0.25;
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

// Bright pass: keep only the fraction of a texel above the display-luma floor.
@fragment
fn fs_down_threshold(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let dst = vec2f(params.dst_width, params.dst_height);
    let uv = position.xy / dst;
    let src = uv * vec2f(textureDimensions(t_input));
    let v = params.viewport;
    if (src.x < v.x || src.y < v.y || src.x >= v.x + v.z || src.y >= v.y + v.w) {
        return vec4f(0.0);
    }
    let color = box_sample(uv);
    let gamma = linear_to_srgb(max(color, vec3f(0.0)));
    let luma = max(max(gamma.r, gamma.g), gamma.b);
    let contribution = max(luma - params.threshold, 0.0) / max(luma, 1e-4);
    return vec4f(color * contribution, 1.0);
}

@fragment
fn fs_down(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let dst = vec2f(params.dst_width, params.dst_height);
    return vec4f(box_sample(position.xy / dst), 1.0);
}

// Upsample into the next larger level; the pipeline adds it to the stored
// downsample content of that level.
@fragment
fn fs_up(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let dst = vec2f(params.dst_width, params.dst_height);
    let uv = position.xy / dst;
    let dims = vec2f(textureDimensions(t_input));
    let rcp = vec2f(1.0 / dims.x, 1.0 / dims.y) * params.radius;
    let color = (textureSampleLevel(t_input, s_linear, uv + vec2f(-rcp.x, -rcp.y), 0.0).rgb
        + textureSampleLevel(t_input, s_linear, uv + vec2f(rcp.x, -rcp.y), 0.0).rgb
        + textureSampleLevel(t_input, s_linear, uv + vec2f(-rcp.x, rcp.y), 0.0).rgb
        + textureSampleLevel(t_input, s_linear, uv + vec2f(rcp.x, rcp.y), 0.0).rgb) * 0.25;
    return vec4f(color, 1.0);
}
