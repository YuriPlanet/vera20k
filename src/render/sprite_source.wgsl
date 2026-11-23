// The source page every SHP-family blitter reads. An RGBA page binds its
// colour at binding 0 and, when it has one, its palette-index plane at
// binding 3 (a 1x1 placeholder otherwise). A palette-indexed SHP atlas page
// stores only the index plane: binding 0 then holds the atlas's palette rows,
// and the instance's `source_palette` names the row, plus one, that its
// indices resolve through. RGBA instances carry `source_palette` 0.
@group(1) @binding(0) var t_sprite: texture_2d<f32>;
@group(1) @binding(1) var s_sprite: sampler;
@group(1) @binding(3) var source_indices: texture_2d<u32>;

// The index-plane texel `uv` addresses.
fn source_texel(uv: vec2f) -> vec2i {
    let dims = vec2f(textureDimensions(source_indices));
    return vec2i(clamp(uv * dims, vec2f(0.0), dims - 1.0));
}

// An indexed page's encoded colour for `index`: its palette row's entry, with
// index 0 transparent because the SHP blitters skip it (`ShpFile::frame_to_rgba`).
fn indexed_color(index: u32, source_palette: u32) -> vec4f {
    let color = textureLoad(t_sprite, vec2i(i32(index), i32(source_palette) - 1), 0);
    return vec4f(color.rgb, select(color.a, 0.0, index == 0u));
}

// The encoded colour of the source texel at `texel`, whose palette index is
// `index`, for blitters that load texels instead of sampling them.
fn source_color(texel: vec2i, index: u32, source_palette: u32) -> vec4f {
    if source_palette != 0u { return indexed_color(index, source_palette); }
    let color = textureLoad(t_sprite, texel, 0);
    return vec4f(srgb_encode(color.rgb), color.a);
}

// `source_color` for blitters that sample `uv`. Every page has one mip and one
// filter, so level 0 is the texel an ordinary sample reads.
fn sampled_source_color(uv: vec2f, index: u32, source_palette: u32) -> vec4f {
    if source_palette != 0u { return indexed_color(index, source_palette); }
    let color = textureSampleLevel(t_sprite, s_sprite, uv, 0.0);
    return vec4f(srgb_encode(color.rgb), color.a);
}
