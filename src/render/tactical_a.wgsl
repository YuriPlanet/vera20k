// The authoritative A plane every world blitter, surface line and A-gated
// store reads. World origin belongs to ShroudBuffer; camera rounding belongs
// to BatchRenderer. Sampling texel centers through floor preserves native
// pixels at fractional zoom/scroll.
@group(0) @binding(1) var tactical_a: texture_2d<f32>;

// VERA-only, experimental (docs/zoom-one-pass.md): at zoom != 1 a world
// pixel is the block from `n * zoom` plus this shift, so no block edge falls
// on a target pixel's centre (1.25 puts every fifth one there) and each
// target pixel samples one world pixel's texel, Z row and A, without a
// rounding coin toss.
const ZOOM_BLOCK_SHIFT: f32 = 0.015625;

// A world blitter's corner at `world_rel` world pixels from the camera:
// snapped to whole world pixels, then a zoom-sized block (shifted at zoom
// != 1); at zoom 1 the plain rounding to target pixels.
fn world_block_corner(world_rel: vec2f) -> vec2f {
    let snapped = floor(world_rel + vec2f(0.5, 0.5)) * camera.zoom;
    return select(snapped + vec2f(ZOOM_BLOCK_SHIFT), snapped, camera.zoom == 1.0);
}

// The world pixels from the target's top left to target point `position`,
// the inverse of `world_block_corner`.
fn block_world(position: vec2f) -> vec2f {
    return select((position - vec2f(ZOOM_BLOCK_SHIFT)) / camera.zoom, position, camera.zoom == 1.0);
}
@group(0) @binding(2) var<uniform> tactical_a_source: TacticalASource;
fn tactical_a_at(position: vec2f) -> u32 {
    // Fragment @builtin(position) is the rasterization point in framebuffer
    // pixels, even when its user-defined field is named clip_position.
    // https://www.w3.org/TR/WGSL/#builtin-values-position
    if tactical_a_source.enabled == 0u { return 127u; }
    let pixel = vec2i(floor(block_world(position) + camera.camera_pos - tactical_a_source.origin));
    if any(pixel < vec2i(0)) || any(pixel >= vec2i(textureDimensions(tactical_a))) { return 127u; }
    let a = u32(round(textureLoad(tactical_a, pixel, 0).r * 255.0));
    return fog_lifted_a(a, tactical_a_source.fog_strength);
}
