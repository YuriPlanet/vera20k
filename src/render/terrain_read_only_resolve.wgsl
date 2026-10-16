@group(0) @binding(0) var old_words: texture_2d<u32>;
struct ReadOnlyPixel { body: atomic<u32>, shadows: atomic<u32> };
@group(1) @binding(0) var<storage, read_write> read_only_pixels: array<ReadOnlyPixel>;

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4f {
    let p = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    return vec4f(p[vertex], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let p = vec2u(position.xy);
    let width = textureDimensions(old_words).x;
    let rows = arrayLength(&read_only_pixels) / width;
    let address = (p.y % rows) * width + p.x;
    let body = atomicLoad(&read_only_pixels[address].body);
    let shadows = atomicLoad(&read_only_pixels[address].shadows);
    // Preserve untouched attachment bytes, including colors outside RGB565.
    if body == 0u && shadows == 0u { discard; }
    var word = select(textureLoad(old_words, vec2i(p), 0).r, body & 65535u, body != 0u);
    // Every native RGB565 word reaches zero after six original half operations.
    // The native exhaustive transition corpus proves this bound for all words.
    for (var i = 0u; i < min(shadows, 6u); i += 1u) { word = native_half_word(word); }
    let encoded = vec3f(f32(RETAIL_FIVE[(word >> 11u) & 31u]),
        f32(RETAIL_SIX[(word >> 5u) & 63u]), f32(RETAIL_FIVE[word & 31u])) / 255.0;
    return vec4f(srgb_decode(encoded), 1.0);
}
