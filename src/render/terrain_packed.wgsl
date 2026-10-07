// Original leaves: 004950C0/00495250/00494F20 and displaced 00495590/00495730.
// Goldens: tools/procedural_drawing_oracle/translucent_blitter_a.json.
struct Params {
    rect: vec4u,
    surface_width: u32,
    selector: u32,
    offset_words: i32,
    reads_depth: u32,
};
@group(0) @binding(0) var old_words: texture_2d<u32>;
@group(0) @binding(1) var old_depth: texture_2d<f32>;
@group(0) @binding(2) var source_words: texture_2d<u32>;
@group(0) @binding(3) var candidates: texture_2d<f32>;
@group(0) @binding(4) var<storage, read_write> results: array<u32>;
@group(0) @binding(5) var<uniform> params: Params;

fn inside(p: vec2u) -> bool {
    return all(p >= params.rect.xy) && all(p < params.rect.xy + params.rect.zw);
}
fn local_index(p: vec2u) -> u32 {
    let q = p - params.rect.xy;
    return q.y * params.rect.z + q.x;
}
fn admitted(p: vec2u) -> bool {
    return (textureLoad(source_words, vec2i(p), 0).r & 0x10000u) != 0u
        && (params.reads_depth == 0u || i32(textureLoad(candidates, vec2i(p), 0).r) < decoded_native_z(textureLoad(old_depth, vec2i(p), 0).r));
}
fn blend(source: u32, destination: u32) -> u32 {
    let selector = params.selector & 6u;
    if selector == 4u {
        return (((source >> 1u) & 0x7befu) + ((destination >> 1u) & 0x7befu)) & 0xffffu;
    }
    let s = (source >> 2u) & 0x39e7u;
    let d = (destination >> 2u) & 0x39e7u;
    return select(s + d * 3u, s * 3u + d, selector == 2u) & 0xffffu;
}
fn resolve_pixel(p: vec2u, feedback: bool) {
    if !admitted(p) { return; }
    let linear = i32(p.y * params.surface_width + p.x) + params.offset_words;
    let neighbor = vec2i(linear % i32(params.surface_width), linear / i32(params.surface_width));
    var destination = textureLoad(old_words, neighbor, 0).r;
    // Earlier stores in this exact native span are visible to a negative offset.
    if feedback && linear >= 0 && inside(vec2u(neighbor)) && admitted(vec2u(neighbor)) {
        destination = results[local_index(vec2u(neighbor))];
    }
    results[local_index(p)] = blend(textureLoad(source_words, vec2i(p), 0).r & 0xffffu, destination);
}
@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) invocation: vec3u,
    @builtin(num_workgroups) groups: vec3u) {
    // WGSL num_workgroups gives the actual dispatch grid. Local size is64x1x1,
    // so this visits each linear pixel/residue once even across multiple rows.
    // https://www.w3.org/TR/WGSL/#builtin-values
    let index = invocation.x + invocation.y * (groups.x * 64u);
    let offset = params.offset_words;
    if offset >= 0 {
        if index >= params.rect.z * params.rect.w { return; }
        resolve_pixel(params.rect.xy + vec2u(index % params.rect.z, index / params.rect.z), false);
        return;
    }
    // Independent residue chains preserve native left-to-right, top-to-bottom
    // word order without imposing a serial whole-frame GPU dispatch.
    let stride = u32(-offset);
    if index >= stride { return; }
    let start = params.rect.y * params.surface_width + params.rect.x;
    let end = (params.rect.y + params.rect.w - 1u) * params.surface_width + params.rect.x + params.rect.z;
    var linear = start + ((index + stride - (start % stride)) % stride);
    while linear < end {
        let p = vec2u(linear % params.surface_width, linear / params.surface_width);
        if inside(p) { resolve_pixel(p, true); }
        linear += stride;
    }
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
    let word = results[local_index(p)];
    let encoded = vec3f(f32(RETAIL_FIVE[(word >> 11u) & 31u]),
        f32(RETAIL_SIX[(word >> 5u) & 63u]), f32(RETAIL_FIVE[word & 31u])) / 255.0;
    return Resolve(vec4f(srgb_decode(encoded),1.0), stored_native_depth(i32(textureLoad(candidates,vec2i(p),0).r)));
}
struct Clear { @location(0) word: u32, @location(1) candidate: f32 };
@fragment
fn fs_clear() -> Clear { return Clear(0u,0.0); }
