// IonBlast 0053D580: a ripple frame moves destination pixels where native Z
// lies behind the blast's row threshold. Reads the blast's own snapshot.
@group(0) @binding(0) var old_words: texture_2d<u32>;
@group(0) @binding(1) var old_depth: texture_2d<f32>;
@group(1) @binding(0) var frames: texture_2d_array<u32>;
// Each ripple byte's move in pixels (x, rows); see ion_blast_ripple.rs.
struct Moves { by_byte: array<vec4<i32>, 128> };
@group(1) @binding(1) var<uniform> moves: Moves;

struct Output {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) origin: vec2i,
    @location(1) @interpolate(flat) frame_seed: vec2u,
    @location(2) @interpolate(flat) zoom: f32,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @location(0) quad: vec4f,
           @location(1) origin: vec2i, @location(2) frame_seed: vec2u,
           @location(3) zoom: f32) -> Output {
    let corners = array<vec2f, 6>(vec2f(0, 0), vec2f(1, 0), vec2f(0, 1),
        vec2f(0, 1), vec2f(1, 0), vec2f(1, 1));
    var out: Output;
    out.position = vec4f(quad.xy + corners[vertex] * quad.zw, 0, 1);
    out.origin = origin;
    out.frame_seed = frame_seed;
    out.zoom = zoom;
    return out;
}

@fragment
fn fs_main(input: Output) -> @location(0) vec4f {
    let p = vec2i(input.position.xy);
    let logical = vec2i(floor(input.position.xy / input.zoom));
    let byte = textureLoad(frames, logical - input.origin, input.frame_seed.x, 0).r;
    // 53D722..53D72D: a signed byte above zero; -1 and 0 leave the pixel.
    if byte == 0u || byte > 127u { discard; }
    // 53D6A2..53D6B9, 53D765: the row's threshold low16(seed - y - 3),
    // 53D72F: unsigned, the pixel moves only where Z lies above it.
    let threshold = (input.frame_seed.y - u32(logical.y) - 3u) & 65535u;
    if threshold >= u32(decoded_native_z(textureLoad(old_depth, p, 0).r)) { discard; }
    let size = vec2i(textureDimensions(old_words));
    let read = clamp(p + vec2i(round(vec2f(moves.by_byte[byte].xy) * input.zoom)),
        vec2i(0), size - 1);
    let word = textureLoad(old_words, read, 0).r;
    return vec4f(retail_linear(word), 1);
}
