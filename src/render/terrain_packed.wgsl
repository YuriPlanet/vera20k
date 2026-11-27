// Original leaves: 004950C0/00495250/00494F20 and warp 004953F0/00495590/00495730.
// Goldens: tools/procedural_drawing_oracle/translucent_blitter_a.json.
struct Params {
    rect: vec4u,
    selector: u32,
    // 0 when the leaf reads its own word. Otherwise 4, plus the parity of the
    // tactical top (bit 1) and of the clipped left edge from the tactical
    // left (bit 0): see `warp_words`.
    warp: u32,
    reads_depth: u32,
    // The camera's zoom. At zoom != 1 (`docs/zoom-one-pass.md`) a word is a
    // world pixel, a `zoom`-sized block of target pixels: the warp's rows and
    // offsets are world pixels (see `snapshot_word`).
    zoom: f32,
};
@group(0) @binding(0) var old_words: texture_2d<u32>;
@group(0) @binding(1) var old_depth: texture_2d<f32>;
@group(0) @binding(2) var source_words: texture_2d<u32>;
@group(0) @binding(3) var candidates: texture_2d<f32>;
@group(0) @binding(4) var<uniform> params: Params;

fn admitted(p: vec2u) -> bool {
    return (textureLoad(source_words, vec2i(p), 0).r & 0x10000u) != 0u
        && (params.reads_depth == 0u || i32(textureLoad(candidates, vec2i(p), 0).r) < decoded_native_z(textureLoad(old_depth, vec2i(p), 0).r));
}
fn blend(source: u32, destination: u32) -> u32 {
    let selector = params.selector & 6u;
    // Convert490E50 tests translucency before bit1. The no-Z compressed
    // shadow496250 ignores source colour and halves the old destination.
    if selector == 0u {
        if (params.selector & 1u) != 0u { return native_half_word(destination); }
        return source; // Original opaque copy494B60.
    }
    if selector == 4u {
        return (native_half_word(source) + native_half_word(destination)) & 0xffffu;
    }
    let s = (source >> 2u) & 0x39e7u;
    let d = (destination >> 2u) & 0x39e7u;
    return select(s + d * 3u, s * 3u + d, selector == 2u) & 0xffffu;
}
// The warp leaves blend with the word `warp` to the right (MOV AX,[EDI+EAX*2]
// at 0049549D) and store to their own. Bit_Blit 004373B0 seeds the warp from
// the first row y, relative to the A-buffer's top (the tactical top), and the
// clipped left x: ((y & 1) << 1) | ((y ^ x) & 1) (004374A3..004374B7), then
// XORs it with 3 every row (004378F9). RLE_Blit 00437A10 does the same
// (00437D09..00437D20, 00437E45), so row y's warp is the seed formula at y.
fn warp_words(y: u32) -> u32 {
    if params.warp == 0u { return 0u; }
    let row = (y ^ (params.warp >> 1u)) & 1u;
    return (row << 1u) | (row ^ (params.warp & 1u));
}
// A warp of 0..3 reads a word this span has not stored yet: the snapshot.
// RESIDUAL: within three words of the surface's right edge the native read
// runs on into the next row; this one stops at the row's last word. Only the
// ripple of a cloaking voxel at the right screen edge can differ, by 1-3 px.
fn snapshot_word(p: vec2u) -> u32 {
    var x = p.x + warp_words(p.y);
    if params.warp != 0u && params.zoom != 1.0 {
        // The target pixel at the same place in the block of the world pixel
        // `warp` to the right, with the row parity of its world row.
        let centre = vec2f(p) + vec2f(0.5);
        let row = u32(floor(centre.y / params.zoom));
        x = u32(floor(centre.x + f32(warp_words(row)) * params.zoom));
    }
    x = min(x, textureDimensions(old_words).x - 1u);
    let destination = textureLoad(old_words, vec2u(x, p.y), 0).r;
    return blend(textureLoad(source_words, vec2i(p), 0).r & 0xffffu, destination);
}
@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4f {
    let p = array<vec2f, 3>(vec2f(-1.0,-1.0),vec2f(3.0,-1.0),vec2f(-1.0,3.0));
    return vec4f(p[vertex],0.0,1.0);
}
struct Resolve { @location(0) color: vec4f, @builtin(frag_depth) depth: f32 };
@fragment
fn fs_resolve(@builtin(position) position: vec4f) -> Resolve {
    let p = vec2u(position.xy);
    if !admitted(p) { discard; }
    return Resolve(vec4f(retail_linear(snapshot_word(p)),1.0), stored_native_depth(i32(textureLoad(candidates,vec2i(p),0).r)));
}
struct Clear { @location(0) word: u32, @location(1) candidate: f32 };
@fragment
fn fs_clear() -> Clear { return Clear(0u,0.0); }
