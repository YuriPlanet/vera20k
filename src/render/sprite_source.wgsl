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
// VERA-only, experimental (docs/zoom-one-pass.md): a picture page's HD
// binding (`picture_house.wgsl`) binds the HD twin at 0 (premultiplied
// encoded colour with mipmaps, a linear sampler at 1), the twin's level-0
// index plane at 3, its house layer at 4 and the 1x page at 5. Every other
// page binds 1x1 placeholders at 4 and 5.
@group(1) @binding(4) var source_house: texture_2d<f32>;
@group(1) @binding(5) var source_low_page: texture_2d<f32>;

// Whether the page bound is a picture page's HD twin: only an HD binding
// binds a 1x page bigger than the 1x1 placeholder.
fn source_hd() -> bool {
    return textureDimensions(source_low_page).x > 1u;
}

// An HD twin's `premultiplied` colour and `house` layer at a pixel whose
// nearest level-0 index is `index`, as `source_color` returns a 1x pixel:
// encoded colour, opaque. A pixel less than half covered, or whose nearest
// twin texel is a hole, is a hole (`picture_zoom.wgsl` draws the same
// pixels). House-colour texels are black on the twin; their owner's ramp
// colour, from the 1x page's strip, comes in by their coverage at the step
// their filtered layer gives.
fn hd_source_color(premultiplied: vec4f, house: vec4f, index: u32, source_palette: u32) -> vec4f {
    if premultiplied.a < 0.5 || index == 0u {
        return vec4f(0.0);
    }
    var colour = premultiplied.rgb;
    if picture_house(source_palette) && house.g > 0.0 {
        let step = u32(round(house.r / house.g * 15.0));
        let ramp = textureLoad(source_low_page, picture_ramp_texel(source_palette, step), 0).rgb;
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

// The encoded colour of the source texel at `texel`, whose palette index is
// `index`, for blitters that load texels instead of sampling them.
fn source_color(texel: vec2i, index: u32, source_palette: u32) -> vec4f {
    if source_palette != 0u && !picture_house(source_palette) {
        return indexed_color(index, source_palette);
    }
    if source_hd() {
        let house_texel = min(texel, vec2i(textureDimensions(source_house)) - 1);
        return hd_source_color(
            textureLoad(t_sprite, texel, 0),
            textureLoad(source_house, house_texel, 0),
            index,
            source_palette,
        );
    }
    return rgba_source_color(textureLoad(t_sprite, texel, 0), index, source_palette);
}

// An RGBA page's `color` at a pixel of index `index`, encoded: a picture
// house-colour pixel's from its owner's ramp on the page.
fn rgba_source_color(color: vec4f, index: u32, source_palette: u32) -> vec4f {
    var rgb = color.rgb;
    if picture_house_pixel(index, source_palette) {
        rgb = textureLoad(t_sprite, picture_ramp_texel(source_palette, index - 16u), 0).rgb;
    }
    return vec4f(srgb_encode(rgb), color.a);
}

// `source_color` for blitters that sample `uv`. Every page has one mip and one
// filter, so level 0 is the texel an ordinary sample reads. An HD twin
// (`source_hd`) is sampled at the level whose texels match the target's
// pixels: log2 of the twin texels a target pixel spans, which in the one-pass
// zoomed draw is the twin's scale over the camera zoom, within its levels.
// The UV's derivatives are taken first, in uniform control flow; callers call
// this at the top of their fragment entry points.
fn sampled_source_color(uv: vec2f, index: u32, source_palette: u32) -> vec4f {
    let uv_dx = dpdx(uv);
    let uv_dy = dpdy(uv);
    if source_palette != 0u && !picture_house(source_palette) {
        return indexed_color(index, source_palette);
    }
    if source_hd() {
        let dims = vec2f(textureDimensions(t_sprite));
        let span = max(length(uv_dx * dims), length(uv_dy * dims));
        // The sampler clamps the level to the twin's last mip (one texel per
        // game pixel); textureNumLevels has no GLSL ES form.
        let lod = max(log2(max(span, 1e-6)), 0.0);
        return hd_source_color(
            textureSampleLevel(t_sprite, s_sprite, uv, lod),
            textureSampleLevel(source_house, s_sprite, uv, lod),
            index,
            source_palette,
        );
    }
    let color = textureSampleLevel(t_sprite, s_sprite, uv, 0.0);
    return rgba_source_color(color, index, source_palette);
}
