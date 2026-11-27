// The source page every SHP-family blitter reads. An RGBA page binds its
// colour at binding 0 and, when it has one, its palette-index plane at
// binding 3 (a 1x1 placeholder otherwise). A palette-indexed SHP atlas page
// stores only the index plane: binding 0 then holds the atlas's palette rows,
// and the instance's `source_palette` names the row, plus one, that its
// indices resolve through. RGBA instances carry `source_palette` 0, or a
// picture's house colour (`picture_house.wgsl`).
@group(1) @binding(0) var t_sprite: texture_2d<f32>;
@group(1) @binding(1) var s_sprite: sampler;
@group(1) @binding(3) var source_indices: texture_2d<u32>;
// VERA-only, experimental (docs/zoom-one-pass.md): an RGBA HD page's
// binding (a picture page, `picture_zoom::PictureHdPage`, the only binding
// it has; or the overlay atlas's HD art twin, `hd_terrain::RgbaTwin`) binds
// premultiplied encoded colour with mipmaps at 0 and a linear sampler at 1,
// its level-0 index plane at 3, its house layer at 4 and the house ramps
// (`picture_house.wgsl`) at 5. An indexed SHP page's HD page
// (`hd_art::ShpHd`) keeps the palette rows at 0 and its 1x indices at 3,
// and binds its 2x twin at 4 (`shp_hd_color`) with a linear sampler at 1.
// Every other page, the model page among them, binds 1x1 placeholders at 4
// and 5.
@group(1) @binding(4) var source_house: texture_2d<f32>;
@group(1) @binding(5) var source_ramps: texture_2d<f32>;

// Whether the page bound is an RGBA HD page: only such a binding binds
// house ramps bigger than the 1x1 placeholder.
fn source_hd() -> bool {
    return textureDimensions(source_ramps).x > 1u;
}

// Whether the page bound is an indexed SHP page with VERA's HD art twin at
// binding 4: the ramps placeholder beside a twin bigger than 1x1.
fn source_shp_hd() -> bool {
    return textureDimensions(source_ramps).x <= 1u && textureDimensions(source_house).x > 1u;
}

// An SHP HD twin texel `twin` (premultiplied by its coverage) at a pixel
// whose 1x index is `index`, in `source_palette`'s row, as `indexed_color`
// returns a 1x pixel: encoded colour. The twin holds the HD art as it
// differs from the pack's reference palette at each texel's 1x index
// (`hd_art::compose_shp_twin`): for house colours (16..31) the ratio of its
// brightest channel to the reference's, over 2.5; otherwise the difference,
// halved around 128/255. So the row's own colours (theater, house) come
// through, and a texel with no HD art is the 1x colour exactly. A pixel less
// than half covered is a hole.
fn shp_hd_color(twin: vec4f, index: u32, source_palette: u32) -> vec4f {
    if twin.a < 0.5 || index == 0u {
        return vec4f(0.0);
    }
    let own = indexed_color(index, source_palette);
    let stored = twin.rgb / twin.a;
    var rgb: vec3f;
    if index >= 16u && index < 32u {
        rgb = own.rgb * (stored.r * 2.5);
    } else {
        rgb = own.rgb + (stored - 128.0 / 255.0) * 2.0;
    }
    return vec4f(clamp(rgb, vec3f(0.0), vec3f(1.0)), own.a);
}

// The level of an HD page of `size` texels to sample for UV derivatives
// `uv_dx`, `uv_dy`: log2 of the page texels a target pixel spans, which for
// an unscaled quad is the page's scale over the camera zoom (a picture page
// at zoom 1: its one-texel-per-game-pixel level; zoomed out: a zoom-out
// level). The sampler clamps it to the page's last mip; textureNumLevels has
// no GLSL ES form.
fn twin_lod(size: vec2u, uv_dx: vec2f, uv_dy: vec2f) -> f32 {
    let dims = vec2f(size);
    let span = max(length(uv_dx * dims), length(uv_dy * dims));
    return max(log2(max(span, 1e-6)), 0.0);
}

// An HD page's `premultiplied` colour and `house` layer at a pixel whose
// nearest level-0 index is `index`, as `source_color` returns a 1x pixel:
// encoded colour, opaque. A pixel less than half covered at the level
// sampled, or whose nearest level-0 texel is a hole, is a hole.
// House-colour texels are black on the page; their owner's ramp colour,
// from the house ramps, comes in by their coverage at the step their
// filtered layer gives.
fn hd_source_color(premultiplied: vec4f, house: vec4f, index: u32, source_palette: u32) -> vec4f {
    if premultiplied.a < 0.5 || index == 0u {
        return vec4f(0.0);
    }
    var colour = premultiplied.rgb;
    if picture_house(source_palette) && house.g > 0.0 {
        let step = u32(round(house.r / house.g * 15.0));
        let ramp = textureLoad(source_ramps, picture_ramp_texel(source_palette, step), 0).rgb;
        colour = colour + srgb_encode(ramp) * house.g;
    }
    return vec4f(min(colour / premultiplied.a, vec3f(1.0)), 1.0);
}

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

// The encoded colour of the source texel at `texel` (of the index plane),
// whose palette index is `index`, for blitters that load texels instead of
// sampling them. An HD twin larger than the index plane gives the twin texel
// at the index texel's top-left; `source_color_at` takes the one under a UV.
fn source_color(texel: vec2i, index: u32, source_palette: u32) -> vec4f {
    let planes = vec2i(textureDimensions(source_indices));
    var twin = vec2i(textureDimensions(t_sprite));
    if source_shp_hd() {
        twin = vec2i(textureDimensions(source_house));
    }
    return loaded_source_color(texel, texel * twin / planes, index, source_palette);
}

// `source_color` for the texels under `uv`: the index plane's and, on an HD
// twin, the twin's own.
fn source_color_at(uv: vec2f, index: u32, source_palette: u32) -> vec4f {
    var twin = vec2f(textureDimensions(t_sprite));
    if source_shp_hd() {
        twin = vec2f(textureDimensions(source_house));
    }
    let twin_texel = vec2i(clamp(uv * twin, vec2f(0.0), twin - 1.0));
    return loaded_source_color(source_texel(uv), twin_texel, index, source_palette);
}

// `source_color` with the index plane's `texel` and an HD twin's `twin_texel`.
fn loaded_source_color(texel: vec2i, twin_texel: vec2i, index: u32, source_palette: u32) -> vec4f {
    if source_palette != 0u && !picture_house(source_palette) {
        if source_shp_hd() {
            return shp_hd_color(textureLoad(source_house, twin_texel, 0), index, source_palette);
        }
        return indexed_color(index, source_palette);
    }
    if source_hd() {
        let house_texel = min(twin_texel, vec2i(textureDimensions(source_house)) - 1);
        return hd_source_color(
            textureLoad(t_sprite, twin_texel, 0),
            textureLoad(source_house, house_texel, 0),
            index,
            source_palette,
        );
    }
    return rgba_source_color(textureLoad(t_sprite, texel, 0));
}

// A plain RGBA page's `color`, encoded. Only HD pages hold picture house
// colour.
fn rgba_source_color(color: vec4f) -> vec4f {
    return vec4f(srgb_encode(color.rgb), color.a);
}

// `source_color` for blitters that sample `uv`. A plain page has one mip and
// one filter, so level 0 is the texel an ordinary sample reads. An HD page
// (`source_hd`, `source_shp_hd`) is sampled at the level whose texels match
// the target's pixels (`twin_lod`). The UV's derivatives are taken first, in
// uniform control flow; callers call this at the top of their fragment entry
// points.
fn sampled_source_color(uv: vec2f, index: u32, source_palette: u32) -> vec4f {
    let uv_dx = dpdx(uv);
    let uv_dy = dpdy(uv);
    if source_palette != 0u && !picture_house(source_palette) {
        if source_shp_hd() {
            let lod = twin_lod(textureDimensions(source_house), uv_dx, uv_dy);
            return shp_hd_color(textureSampleLevel(source_house, s_sprite, uv, lod), index, source_palette);
        }
        return indexed_color(index, source_palette);
    }
    if source_hd() {
        let lod = twin_lod(textureDimensions(t_sprite), uv_dx, uv_dy);
        return hd_source_color(
            textureSampleLevel(t_sprite, s_sprite, uv, lod),
            textureSampleLevel(source_house, s_sprite, uv, lod),
            index,
            source_palette,
        );
    }
    return rgba_source_color(textureSampleLevel(t_sprite, s_sprite, uv, 0.0));
}
