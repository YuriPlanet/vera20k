// Native Convert palette/A precedes 004984D0/004986D0/004988C0.
struct PackedSource { @location(0) word: u32, @location(1) candidate: f32 };
@fragment
fn fs_packed(input: VertexOutput) -> PackedSource {
    let dims = vec2f(textureDimensions(source_indices));
    let texel = vec2i(clamp(input.uv * dims, vec2f(0.0), dims - 1.0));
    let index = textureLoad(source_indices, texel, 0).r;
    if index == 0u { discard; }
    let candidate = native_candidate(input);
    let rgb = vec3u(round(srgb_encode(textureLoad(t_sprite, texel, 0).rgb) * 255.0));
    let word = native_palette_word(rgb, index, input.palette_light, tactical_a_at(input.position.xy));
    return PackedSource(word | 0x10000u, f32(candidate));
}
