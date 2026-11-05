// One tactical A source as its readers bind it: the plane's world origin, an
// enable flag (0 reads the neutral 127 everywhere) and VERA's client-only
// translucent-fog strength. 1000 keeps the native A; 500 lifts it halfway to
// the neutral 127, as ShroudBuffer's readback lifts it.
struct TacticalASource { origin: vec2f, enabled: u32, fog_strength: u32 };
fn fog_lifted_a(a: u32, strength: u32) -> u32 {
    let delta = i32(a) - 127;
    return u32(clamp(127 + delta * i32(strength) / 1000, 0, 255));
}
