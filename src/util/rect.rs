//! Signed rectangle operations shared by map normalization and surface draws.

/// Ordered rectangle update from cached VXL blit 0x70755D..0x707678 and
/// BuildingType image bounds 0x465570.
/// Rectangles are x/y/width/height in one shared pixel coordinate frame.
/// Expanding a maximum edge adds one; expanding only a minimum edge does
/// not. An empty accumulator is replaced. This is not a normal AABB union.
pub fn union_native_draw_bounds(accumulated: &mut Option<[i32; 4]>, next: [i32; 4]) {
    let Some(mut current) = *accumulated else {
        *accumulated = Some(next);
        return;
    };
    if current[2] <= 0 || current[3] <= 0 {
        *accumulated = Some(next);
        return;
    }
    if next[2] <= 0 || next[3] <= 0 {
        return;
    }
    for axis in 0..2 {
        if next[axis] < current[axis] {
            current[axis + 2] += current[axis] - next[axis];
            current[axis] = next[axis];
        }
        let next_end = next[axis] + next[axis + 2];
        if next_end > current[axis] + current[axis + 2] {
            current[axis + 2] = next_end - current[axis] + 1;
        }
    }
    *accumulated = Some(current);
}

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

#[cfg(all(test, tests_util))]
mod tests {
    use super::{clip_blit_rects, union_native_draw_bounds};

    #[test]
    fn native_draw_union_matches_executed_cached_blit_block() {
        // Execute original 70755D..707678 with old rectangle at B1CFC0 and
        // signed cached x/y, unsigned w/h. These are native outputs, not a
        // mathematical AABB-union oracle. The +1 maximum edge is deliberate.
        let vectors = [
            ([5, 6, 10, 12], [6, 7, 2, 3], [5, 6, 10, 12]),
            ([5, 6, 10, 12], [5, 6, 10, 12], [5, 6, 10, 12]),
            ([5, 6, 10, 12], [1, 2, 8, 8], [1, 2, 14, 16]),
            ([5, 6, 10, 12], [12, 16, 10, 10], [5, 6, 18, 21]),
            ([5, 6, 10, 12], [1, 2, 30, 40], [1, 2, 31, 41]),
            ([5, 6, 10, 12], [15, 18, 0, 0], [5, 6, 10, 12]),
            ([5, 6, 0, 12], [15, 18, 0, 0], [15, 18, 0, 0]),
            ([-20, -10, 8, 6], [-12, -4, 4, 4], [-20, -10, 13, 11]),
            ([0, 0, 0, 0], [124, 123, 20, 30], [124, 123, 20, 30]),
            ([124, 123, 20, 30], [124, 123, 20, 30], [124, 123, 20, 30]),
        ];
        for (initial, next, expected) in vectors {
            let mut accumulated = Some(initial);
            union_native_draw_bounds(&mut accumulated, next);
            assert_eq!(accumulated, Some(expected), "{initial:?} then {next:?}");
        }
    }

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
