// Unit 0073B140 resolves the entire indexed cache once, after layer composition.
// Translucent tinted slots forward to plain copies; keep the colour-word OR
// with the ordinary resolver, as BlitPolicy::ors_colour_word requires (reading).
struct PackedSource { @location(0) word: u32, @location(1) candidate: f32 };
@fragment
fn fs_packed(input: VertexOutput) -> PackedSource {
    if ((input.fx_flags & 128u) != 0u && input.world_pos.y >= input.sinking_row) { discard; }
    let texel = vec2i(input.atlas_uv * vec2f(textureDimensions(atlas)));
    let index = textureLoad(atlas, texel, 0).r;
    if index == 0u { discard; }
    let rgb = voxel_source_rgb(index, input.remap_row, input.source_palette);
    var word = native_palette_word(vec3u(round(srgb_encode(rgb) * 255.0)), index,
        input.palette_light, tactical_a_at(input.clip_position.xy));
    if (u32(input.fx_params.y) & 6u) == 0u {
        word = native_colour_word(word, index, input.palette_light);
    }
    return PackedSource(word | 0x10000u, f32(native_candidate(input)));
}
