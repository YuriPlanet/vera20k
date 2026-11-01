// `Pirate_Bloom.fx` (Pirate / Daodan): a thresholded bright pass followed by
// two horizontal/vertical 12-tap box pairs at a quarter of the frame, then a
// composite with the effect's five blend modes.
//
// The first box pass subtracts `BLOOM_THRESHOLD` from every tap, so it writes
// `(1 - threshold) * (2 * taps - 1)` for a fully white scene; the later passes
// only divide by `2 * taps - 1`. `filter.x` carries the threshold (0 after the
// first pass) and `filter.y` the normalizer, so one blur entry covers all four
// passes.

struct Params {
    // Per-tap offset in source uv; only the pass's blur axis is non-zero.
    step: vec2f,
    // The pass target's size in pixels, so fragment positions become uv.
    dst_size: vec2f,
    // threshold, normalizer, unused, unused.
    filt: vec4f,
    // strength, saturation, blend mode, unused.
    blend: vec4f,
    // (x, y, width, height) of the tactical viewport in frame pixels. Texels
    // outside it (sidebar column, bottom strip) keep the frame untouched.
    viewport: vec4f,
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var s_linear: sampler;
@group(0) @binding(3) var t_bloom: texture_2d<f32>;

const TAPS: i32 = 12;
// `ReShade.fxh` luma used by Pirate_Global.
const BLOOM_LUMA = vec3f(0.2126, 0.7152, 0.0722);

fn blur(uv: vec2f) -> vec3f {
    let threshold = vec3f(params.filt.x);
    var sum = max(textureSampleLevel(t_source, s_linear, uv, 0.0).rgb - threshold, vec3f(0.0));
    for (var i = 1; i < TAPS; i += 1) {
        let off = params.step * f32(i);
        sum += max(textureSampleLevel(t_source, s_linear, uv + off, 0.0).rgb - threshold, vec3f(0.0));
        sum += max(textureSampleLevel(t_source, s_linear, uv - off, 0.0).rgb - threshold, vec3f(0.0));
    }
    return sum * params.filt.y;
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_blur(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst_size;
    return vec4f(blur(uv), 1.0);
}

@fragment
fn fs_combine(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let source = textureLoad(t_source, vec2i(position.xy), 0);
    let p = position.xy;
    let v = params.viewport;
    if (p.x < v.x || p.y < v.y || p.x >= v.x + v.z || p.y >= v.y + v.w) {
        return source;
    }
    let uv = p / vec2f(textureDimensions(t_source));
    let sampled = textureSampleLevel(t_bloom, s_linear, uv, 0.0).rgb;
    let bloom = mix(vec3f(dot(sampled, BLOOM_LUMA)), sampled, params.blend.y) * params.blend.x;
    let frame = source.rgb;
    var out = frame + bloom * clamp(vec3f(1.0) - frame, vec3f(0.0), vec3f(1.0));
    let mode = params.blend.z;
    if (mode < 0.5) {
        out = frame + bloom;
    } else if (mode < 1.5) {
        out = frame + bloom * clamp(vec3f(1.0) - frame, vec3f(0.0), vec3f(1.0));
    } else if (mode < 2.5) {
        out = vec3f(1.0) - (vec3f(1.0) - frame) * (vec3f(1.0) - bloom);
    } else if (mode < 3.5) {
        out = (vec3f(1.0) - bloom * 2.0) * frame * frame + bloom * 2.0 * frame;
    } else {
        out = frame / max(vec3f(1.0) - bloom, vec3f(1e-4));
    }
    return vec4f(out, source.a);
}
