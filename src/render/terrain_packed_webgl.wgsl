// WebGL2 fallback for cs_main/resolve_pixel (no compute or storage). Each
// admitted pixel resolves its own residue chain: it walks back through the
// earlier admitted pixels whose stores its destination read sees, then
// applies the same blends forward from the snapshot word. The words equal
// the ordered compute results; the fragment writes them to an R32Uint target.
@group(1) @binding(0) var fallback_results: texture_2d<u32>;

fn fallback_linear(p: vec2u) -> i32 {
    return i32(p.y * params.surface_width + p.x) + params.offset_words;
}
fn fallback_neighbor(p: vec2u) -> vec2i {
    let linear = fallback_linear(p);
    return vec2i(linear % i32(params.surface_width), linear / i32(params.surface_width));
}
// resolve_pixel's feedback condition: the neighbor is an earlier store here.
fn fallback_feeds(p: vec2u) -> bool {
    let neighbor = fallback_neighbor(p);
    return params.offset_words < 0 && fallback_linear(p) >= 0
        && inside(vec2u(neighbor)) && admitted(vec2u(neighbor));
}
fn fallback_source(p: vec2u) -> u32 {
    return textureLoad(source_words, vec2i(p), 0).r & 0xffffu;
}
@fragment
fn fs_feedback(@builtin(position) position: vec4f) -> @location(0) u32 {
    let p = vec2u(position.xy);
    if !admitted(p) { discard; }
    var first = p;
    var links = 0u;
    while fallback_feeds(first) {
        first = vec2u(fallback_neighbor(first));
        links += 1u;
    }
    var word = blend(fallback_source(first), textureLoad(old_words, fallback_neighbor(first), 0).r);
    var linear = first.y * params.surface_width + first.x;
    for (var link = 0u; link < links; link += 1u) {
        linear += u32(-params.offset_words);
        let q = vec2u(linear % params.surface_width, linear / params.surface_width);
        word = blend(fallback_source(q), word);
    }
    return word;
}
