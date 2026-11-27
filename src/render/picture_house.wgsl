// VERA-only, experimental (docs/picture-model-art.md): a picture page entry
// whose house-colour pixels take their owner's colour as they draw
// (`picture_atlas::PICTURE_HOUSE`). Its `source_palette` is that bit and the
// house colour index, a house-colour pixel's index is 16 plus its ramp step,
// and the page holds every house colour's ramp at its top left
// (`picture_atlas::RAMP_STRIP`). One frame on a page serves every owner.
const PICTURE_HOUSE: u32 = 0x80000000u;

fn picture_house(source_palette: u32) -> bool {
    return (source_palette & PICTURE_HOUSE) != 0u;
}

// The page texel of `source_palette`'s house ramp at `step`.
fn picture_ramp_texel(source_palette: u32, step: u32) -> vec2i {
    let house = source_palette & 0xFFu;
    return vec2i(i32((house % 16u) * 16u + min(step, 15u)), i32(house / 16u));
}

// Whether a pixel of index `index` takes such an entry's house colour.
fn picture_house_pixel(index: u32, source_palette: u32) -> bool {
    return picture_house(source_palette) && index >= 16u && index < 32u;
}
