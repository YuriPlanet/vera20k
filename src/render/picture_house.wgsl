// VERA-only, experimental (docs/picture-model-art.md): a picture page entry
// whose house-colour pixels take their owner's colour as they draw
// (`picture_atlas::PICTURE_HOUSE`). Its `source_palette` is that bit and the
// house colour index, a house-colour texel's index is 16 plus its ramp step
// (its colour black, its step on the page's house layer), and the house
// ramps texture its page binds holds every house colour's ramp
// (`picture_atlas::RAMP_STRIP`, `BatchRenderer::write_picture_ramps`). One
// frame on a page serves every owner.
const PICTURE_HOUSE: u32 = 0x80000000u;

fn picture_house(source_palette: u32) -> bool {
    return (source_palette & PICTURE_HOUSE) != 0u;
}

// The house ramps texel of `source_palette`'s house ramp at `step`.
fn picture_ramp_texel(source_palette: u32, step: u32) -> vec2i {
    let house = source_palette & 0xFFu;
    return vec2i(i32((house % 16u) * 16u + min(step, 15u)), i32(house / 16u));
}
