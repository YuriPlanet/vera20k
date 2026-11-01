// Copies the reduced world into Anime4K's `INPUT` layer. The source may be an
// sRGB surface, so a typed load is what decodes it to the linear values the
// network expects.

@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let at = vec2i(floor(position.xy));
    let limit = vec2i(textureDimensions(source)) - vec2i(1);
    return textureLoad(source, clamp(at, vec2i(0), limit), 0);
}
