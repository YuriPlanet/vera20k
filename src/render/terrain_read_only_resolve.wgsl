// The WebGL2 fallback's read-only final words (terrain_read_only.wgsl).
@group(0) @binding(0) var old_words: texture_2d<u32>;
@group(1) @binding(0) var read_only_bodies: texture_2d<u32>;
@group(1) @binding(1) var read_only_shadows: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4f {
    let p = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    return vec4f(p[vertex], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let p = vec2i(position.xy);
    let body = textureLoad(read_only_bodies, p, 0).r;
    let shadows = u32(round(textureLoad(read_only_shadows, p, 0).r * 255.0));
    // Preserve untouched attachment bytes, including colors outside RGB565.
    if body == 0u && shadows == 0u { discard; }
    var word = select(textureLoad(old_words, p, 0).r, body & 65535u, body != 0u);
    // Every native RGB565 word reaches zero after six original half operations.
    // The native exhaustive transition corpus proves this bound for all words.
    for (var i = 0u; i < min(shadows, 6u); i += 1u) { word = native_half_word(word); }
    return vec4f(retail_linear(word), 1.0);
}
