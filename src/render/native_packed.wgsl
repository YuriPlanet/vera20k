// Original RGB565 mask initializer004BAA73..004BAAC1 and shadow496250.
// Executed for every input word by tools/terrain_draw_oracle/leaf.py.
fn native_half_word(word: u32) -> u32 {
    return (word >> 1u) & 0x7befu;
}

fn native_rgb565_word(rgb: vec3u) -> u32 {
    return ((rgb.r >> 3u) << 11u) | ((rgb.g >> 2u) << 5u) | (rgb.b >> 3u);
}
