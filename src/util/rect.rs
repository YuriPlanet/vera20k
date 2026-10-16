//! Signed rectangle intersection shared by map normalization and surface draws.

/// ClipRect421B60 with null optional source-offset pointers. `candidate` is
/// the stack rectangle and `bounds` the EDX rectangle; their roles matter on
/// signed overflow. Empty results are all zero, not a retained empty origin.
/// Native execution: tools/spatial_oracle/map_queries.py and
/// tools/procedural_drawing_oracle/action_lines.py.
pub fn clip_rect(
    [mut left, mut top, mut width, mut height]: [i32; 4],
    [clip_left, clip_top, clip_width, clip_height]: [i32; 4],
) -> [i32; 4] {
    if clip_width <= 0 || clip_height <= 0 || width <= 0 || height <= 0 {
        return [0; 4];
    }
    if left < clip_left {
        width = width.wrapping_add(left.wrapping_sub(clip_left));
        left = clip_left;
    }
    if width <= 0 {
        return [0; 4];
    }
    if top < clip_top {
        height = height.wrapping_add(top.wrapping_sub(clip_top));
        top = clip_top;
    }
    if height <= 0 {
        return [0; 4];
    }
    if clip_left.wrapping_add(clip_width) < left.wrapping_add(width) {
        width = clip_left.wrapping_sub(left).wrapping_add(clip_width);
    }
    if width <= 0 {
        return [0; 4];
    }
    let clip_bottom = clip_top.wrapping_add(clip_height);
    if clip_bottom < top.wrapping_add(height) {
        height = clip_bottom.wrapping_sub(top);
    }
    if height <= 0 {
        return [0; 4];
    }
    [left, top, width, height]
}
