// StripDraw6A9BC0/6A9E97 flags404 -> compressed no-Z leaf4963F0.
// The source was converted once through ordinary Convert N=1 when uploaded.
// Recover its packed word without tactical A, tint or depth dependencies.
struct PackedSource { @location(0) word: u32, @location(1) candidate: f32 };
@fragment
fn fs_packed(input: VertexOutput) -> PackedSource {
    let texel = source_texel(input.uv);
    let index = textureLoad(source_indices, texel, 0).r;
    if index == 0u { discard; }
    let color = source_color(texel, index, input.source_palette);
    let rgb = vec3u(round(srgb_encode(color.rgb) * 255.0));
    return PackedSource(native_rgb565_word(rgb) | 0x10000u, 0.0);
}
