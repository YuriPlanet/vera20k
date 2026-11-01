// ArcaneBloom's preparation passes, ported from luluco250's `ArcaneBloom.fx`.
//
// `fs_hdr` turns the graded frame into the shader's HDR-ish buffer (an inverse
// Reinhard lift to `MaxBrightness`), `fs_luma*` build the luminance mip chain
// the adaptation reads, `fs_adapt` smooths that average over time, and
// `fs_box`/`fs_blur` build the five Gaussian levels. `arcane_bloom_blend.wgsl`
// then mixes them back into the frame.
//
// Outside the tactical viewport the HDR buffer stays black, so UI chrome never
// contributes to the glow; the blend leaves those texels untouched.

struct Params {
    dst: vec2f,      // render target size in pixels
    step: vec2f,     // blur or box offset in uv
    frame: vec2f,    // graded frame size in pixels (for the viewport test)
    _pad: vec2f,
    // intensity, max brightness, exposure, adaptation intensity
    scalar: vec4f,
    // sensitivity, adaptation seconds, frame seconds, luminance mip
    adapt: vec4f,
    // limit min, limit max, use limits, unused
    limits: vec4f,
    viewport: vec4f, // tactical viewport in frame pixels
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var s_linear: sampler;
@group(0) @binding(3) var t_aux: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn luminance(color: vec3f) -> f32 {
    return dot(color, vec3f(0.2126, 0.7152, 0.0722));
}

// The shader's inverse Reinhard: a lift that maps 1.0 to `1 / inv_max`, so the
// bloom pyramid works in an HDR range instead of clipping at the display white.
fn inv_reinhard_lum(color: vec3f, inv_max: f32) -> vec3f {
    let lum = max(color.r, max(color.g, color.b));
    return color * (lum / max(1.0 - lum, inv_max));
}

fn outside_viewport(position: vec2f, frame: vec2f, viewport: vec4f) -> bool {
    let sample_position = position * frame;
    return sample_position.x < viewport.x
        || sample_position.y < viewport.y
        || sample_position.x >= viewport.x + viewport.z
        || sample_position.y >= viewport.y + viewport.w;
}

@fragment
fn fs_hdr(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    if (outside_viewport(uv, params.frame, params.viewport)) {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    let color = textureSampleLevel(t_source, s_linear, uv, 0.0).rgb;
    return vec4f(inv_reinhard_lum(color, 1.0 / max(params.scalar.y, 1.0)), 1.0);
}

@fragment
fn fs_luma(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    let color = textureSampleLevel(t_source, s_linear, uv, 0.0).rgb;
    return vec4f(luminance(color), 0.0, 0.0, 1.0);
}

// A 2x2 box: four bilinear taps a quarter texel off centre average exactly the
// four source texels.
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

@fragment
fn fs_adapt(@builtin(position) position: vec4f) -> @location(0) vec4f {
    // The effect reads the average at its first texel; the mip level is the
    // precision knob, where 0 is the fully averaged 1x1 level.
    let uv = vec2f(0.5) / vec2f(textureDimensions(t_source));
    var adapt = textureSampleLevel(t_source, s_linear, uv, params.adapt.w).x;
    adapt = adapt * params.adapt.x;
    if (params.limits.z > 0.0) {
        adapt = clamp(adapt, params.limits.x, params.limits.y);
    }
    let last = textureLoad(t_aux, vec2i(0, 0), 0).x;
    let mix_amount = clamp(params.adapt.z / max(params.adapt.y, 0.0001), 0.0, 1.0);
    return vec4f(mix(last, adapt, mix_amount), 0.0, 0.0, 1.0);
}

// The four-tap box the effect applies before each Gaussian level.
@fragment
fn fs_box(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    let offset = params.step * 0.5;
    let color = textureSampleLevel(t_source, s_linear, uv + vec2f(-offset.x, -offset.y), 0.0).rgb
        + textureSampleLevel(t_source, s_linear, uv + vec2f(offset.x, -offset.y), 0.0).rgb
        + textureSampleLevel(t_source, s_linear, uv + vec2f(-offset.x, offset.y), 0.0).rgb
        + textureSampleLevel(t_source, s_linear, uv + vec2f(offset.x, offset.y), 0.0).rgb;
    return vec4f(color * 0.25, 1.0);
}

// The effect's fixed 13-tap Gaussian, stepped along `params.step`.
@fragment
fn fs_blur(@builtin(position) position: vec4f) -> @location(0) vec4f {
    var weights = array<f32, 13>(
        0.017997,
        0.033159,
        0.054670,
        0.080657,
        0.106483,
        0.125794,
        0.132981,
        0.125794,
        0.106483,
        0.080657,
        0.054670,
        0.033159,
        0.017997,
    );
    var uv = position.xy / params.dst - params.step * 6.0;
    var color = vec3f(0.0);
    for (var tap = 0; tap < 13; tap += 1) {
        color += textureSampleLevel(t_source, s_linear, uv, 0.0).rgb * weights[tap];
        uv += params.step;
    }
    return vec4f(color, 1.0);
}
