// Unit 0073B140 resolves the entire indexed cache once, after layer composition.
struct PackedSource { @location(0) word: u32, @location(1) candidate: f32 };
@fragment
fn fs_packed(input: VertexOutput) -> PackedSource {
    if ((input.fx_flags & 128u) != 0u && input.world_pos.y >= input.sinking_row) { discard; }
    let texel = vec2i(input.atlas_uv * vec2f(textureDimensions(atlas)));
    let index = textureLoad(atlas, texel, 0).r;
    if index == 0u { discard; }
    var rgb: vec3f;
    if index >= 16u && index < 32u {
        rgb = textureLoad(house_ramp, vec2i(i32(index - 16u), i32(input.remap_row)), 0).rgb;
    } else {
        rgb = textureLoad(palette, vec2i(i32(index), 0), 0).rgb;
    }
    let word = native_palette_word(vec3u(round(srgb_encode(rgb) * 255.0)), index,
        input.palette_light, tactical_a_at(input.clip_position.xy));
    return PackedSource(word | 0x10000u, f32(native_candidate(input)));
}
