// Shadow halves after a depth-tested read-write interval (terrain_depth.wgsl).
// Bodies already stored their colour and every admitted piece its Z; per
// pixel, the shadows admitted after its last admitted body remain counted.
// Shadow496250 halves the packed word once per shadow, in sequence, so the
// result is that many halvings of the word the interval left.
@group(0) @binding(0) var old_words: texture_2d<u32>;
@group(1) @binding(0) var shadow_counts: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4f {
    let p = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    return vec4f(p[vertex], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let p = vec2i(position.xy);
    let shadows = u32(round(textureLoad(shadow_counts, p, 0).r * 255.0));
    // Preserve untouched attachment bytes, including colors outside RGB565.
    if shadows == 0u { discard; }
    var word = textureLoad(old_words, p, 0).r;
    // Every native RGB565 word reaches zero after six original half operations.
    for (var i = 0u; i < min(shadows, 6u); i += 1u) { word = native_half_word(word); }
    return vec4f(retail_linear(word), 1.0);
}
