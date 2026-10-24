// The same authoritative A bytes used by CPU surface admission. World origin
// belongs to ShroudBuffer; camera rounding belongs to BatchRenderer. Sampling
// texel centers through floor preserves native pixels at fractional zoom/scroll.
// `fog_strength` is VERA's client-only translucent-fog policy: 1000 keeps the
// native A, 500 lifts it halfway to the neutral 127, matching the CPU sampler.
struct TacticalASource { origin: vec2f, enabled: u32, fog_strength: u32 };
@group(0) @binding(1) var tactical_a: texture_2d<f32>;
@group(0) @binding(2) var<uniform> tactical_a_source: TacticalASource;
fn fog_lifted_a(a: u32, strength: u32) -> u32 {
    let delta = i32(a) - 127;
    return u32(clamp(127 + delta * i32(strength) / 1000, 0, 255));
}
fn tactical_a_at(position: vec2f) -> u32 {
    // Fragment @builtin(position) is the rasterization point in framebuffer
    // pixels, even when its user-defined field is named clip_position.
    // https://www.w3.org/TR/WGSL/#builtin-values-position
    if tactical_a_source.enabled == 0u { return 127u; }
    let pixel = vec2i(floor(position / camera.zoom + camera.camera_pos - tactical_a_source.origin));
    if any(pixel < vec2i(0)) || any(pixel >= vec2i(textureDimensions(tactical_a))) { return 127u; }
    let a = u32(round(textureLoad(tactical_a, pixel, 0).r * 255.0));
    return fog_lifted_a(a, tactical_a_source.fog_strength);
}
