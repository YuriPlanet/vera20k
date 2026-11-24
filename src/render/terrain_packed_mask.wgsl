// StripDraw6A9B46 flags401 -> Convert490F04 -> compressed leaf496250.
// Only the original nonzero source index admits a store. Palette, A and Z
// are not inputs to this no-Z stencil, including for black palette entries.
struct PackedSource { @location(0) word: u32, @location(1) candidate: f32 };
@fragment
fn fs_packed(input: VertexOutput) -> PackedSource {
    if textureLoad(source_indices, source_texel(input.uv), 0).r == 0u { discard; }
    return PackedSource(0x10000u, 0.0);
}
