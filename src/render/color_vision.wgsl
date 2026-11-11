// Whole-viewport daltonization, VERA-only: no gamemd counterpart.
//
// `srgb_encode` / `srgb_decode` are prepended by color_vision.rs from
// palette_light.wgsl. The source and target are ordinary sRGB views, so the
// hardware decodes on load and encodes on store. The reference matrices were
// calibrated on gamma-encoded sRGB channel bytes (SVG feColorMatrix with
// color-interpolation-filters="sRGB"), so convert back to encoded bytes before
// applying them and hand linear values back for the store. Native and WebGL2
// take the same path; no reinterpret view, storage buffer or compute is used.

@group(0) @binding(0) var source: texture_2d<f32>;

struct Correction {
    r: vec4f,
    g: vec4f,
    b: vec4f,
};
@group(0) @binding(1) var<uniform> correction: Correction;

// One oversized triangle covers the whole attachment; the fragment's own
// framebuffer position is the texel to read, so no UV interpolation is needed.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let corner = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let texel = textureLoad(source, vec2i(position.xy), 0);
    let encoded = vec4f(srgb_encode(texel.rgb), texel.a);
    let rgb = vec3f(
        dot(correction.r.xyz, encoded.rgb),
        dot(correction.g.xyz, encoded.rgb),
        dot(correction.b.xyz, encoded.rgb),
    );
    let corrected = vec4f(clamp(rgb, vec3f(0.0), vec3f(1.0)), encoded.a);
    return vec4f(srgb_decode(corrected.rgb), corrected.a);
}
