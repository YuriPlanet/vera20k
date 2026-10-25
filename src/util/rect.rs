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

/// Coupled source/destination clipping (`7BBE20`, called by SHP walker437AAB).
/// Equal-size rectangles retain matching pixels as each edge is clipped. Its
/// fast path uses the surfaces' width/height, ignoring their bounds origins;
/// differently sized rectangles use ClipRect421B60 independently instead.
/// Destination comes first, matching the active437AAB caller.
pub fn clip_blit_rects(
    mut destination: [i32; 4],
    destination_bounds: [i32; 4],
    mut source: [i32; 4],
    source_bounds: [i32; 4],
) -> Option<([i32; 4], [i32; 4])> {
    if destination[2..] == source[2..] {
        for destination_side in [true, false] {
            let (rect, bounds, other) = if destination_side {
                (&mut destination, destination_bounds, &mut source)
            } else {
                (&mut source, source_bounds, &mut destination)
            };
            for axis in 0..2 {
                if rect[axis] < 0 {
                    other[axis] = other[axis].wrapping_sub(rect[axis]);
                    other[axis + 2] = other[axis + 2].wrapping_add(rect[axis]);
                    rect[axis + 2] = rect[axis + 2].wrapping_add(rect[axis]);
                    rect[axis] = 0;
                }
            }
            for axis in 0..2 {
                let excess = rect[axis]
                    .wrapping_sub(bounds[axis + 2])
                    .wrapping_add(rect[axis + 2]);
                if excess > 0 {
                    other[axis + 2] = other[axis + 2].wrapping_sub(excess);
                    rect[axis + 2] = rect[axis + 2].wrapping_sub(excess);
                }
            }
        }
    } else {
        // 7BBF66..7BBFB7 supplies each surface bounds as ClipRect's stack
        // candidate. Reversing the roles changes signed-overflow results.
        destination = clip_rect(destination_bounds, destination);
        source = clip_rect(source_bounds, source);
    }
    (source[2] > 0 && source[3] > 0 && destination[2] > 0 && destination[3] > 0)
        .then_some((destination, source))
}

#[cfg(test)]
mod tests {
    use super::clip_blit_rects;

    #[test]
    fn coupled_blit_rectangles_match_original_execution() {
        let native = crate::test_fixture::json("tools/spatial_oracle/ore_shape_clip.json");
        for row in native["rows"].as_array().expect("native controls") {
            let rect = |field: &str| -> [i32; 4] {
                row[field]
                    .as_array()
                    .expect("rectangle")
                    .iter()
                    .map(|value| value.as_i64().expect("signed native component") as i32)
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("four rectangle components")
            };
            let expected = row["accepted"]
                .as_bool()
                .expect("native admission")
                .then(|| (rect("clipped_destination"), rect("clipped_source")));
            assert_eq!(
                clip_blit_rects(
                    rect("destination"),
                    rect("destination_bounds"),
                    rect("source"),
                    rect("source_bounds"),
                ),
                expected,
                "{}",
                row["name"]
            );
        }
    }
}
