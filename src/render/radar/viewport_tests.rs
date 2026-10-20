use std::collections::BTreeSet;

use super::*;

#[test]
fn clickable_radar_points_roundtrip_at_native_and_scaled_apertures() {
    let surface = NativeRadarSurfaceGeometry::from_raw_rect(30, 20, 300, 180).unwrap();
    for rect in [
        [100.0, 50.0, 140.0, 108.0],
        [100.4, 50.6, 210.0, 162.0],
        [100.0, 50.0, 70.0, 54.0],
    ] {
        let screen = NativeRadarScreenGeometry::new(surface, rect);
        let size = surface.generated_size();
        let mut represented = 0;
        for y in 0..size.1 {
            for x in 0..size.0 {
                if let Some(point) = screen.surface_pixel_screen_point((x, y)) {
                    assert_eq!(screen.screen_to_surface_pixel(point), Some((x, y)));
                    represented += 1;
                }
            }
        }
        assert!(represented > 0);
        if rect[2] >= 140.0 {
            assert_eq!(represented, size.0 * size.1);
        } else {
            assert!(represented < size.0 * size.1);
        }
        assert_eq!(screen.surface_pixel_screen_point((-1, 0)), None);
        assert_eq!(screen.surface_pixel_screen_point((size.0, 0)), None);
    }
}

#[test]
fn native_radar_screen_geometry_rejects_letterbox_and_maps_half_open_edges() {
    let wide = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    let screen = NativeRadarScreenGeometry::new(wide, [100.0, 50.0, 280.0, 216.0]);
    assert_eq!(screen.content_rect(), [100.0, 74.0, 280.0, 166.0]);
    assert_eq!(screen.screen_to_surface_pixel((100.0, 74.0)), Some((0, 0)));
    assert_eq!(
        screen.screen_to_surface_pixel((379.0, 239.0)),
        Some((139, 82))
    );
    assert_eq!(screen.screen_to_surface_pixel((100.0, 73.0)), None);
    assert_eq!(screen.screen_to_surface_pixel((380.0, 239.0)), None);
    assert_eq!(screen.screen_to_surface_pixel((379.0, 240.0)), None);

    let tall = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 180, 300).unwrap();
    let screen = NativeRadarScreenGeometry::new(tall, [100.0, 50.0, 280.0, 216.0]);
    assert_eq!(screen.content_rect(), [176.0, 50.0, 128.0, 216.0]);
    assert_eq!(screen.screen_to_surface_pixel((176.0, 50.0)), Some((0, 0)));
    assert_eq!(
        screen.screen_to_surface_pixel((303.0, 265.0)),
        Some((63, 107))
    );
    assert_eq!(screen.screen_to_surface_pixel((175.0, 50.0)), None);
}

#[test]
fn native_viewport_rect_matches_wide_and_tall_binary_arithmetic() {
    let wide = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    assert_eq!(
        native_viewport_rect(wide, (80, 50), 632, 570),
        NativeRadarRect {
            x: 9,
            y: 52,
            w: 10,
            h: 17
        }
    );
    assert_eq!(
        native_viewport_rect(wide, (0, 0), 632, 570),
        NativeRadarRect {
            x: 0,
            y: 0,
            w: 10,
            h: 17
        }
    );

    let tall = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 180, 300).unwrap();
    assert_eq!(
        native_viewport_rect(tall, (80, 60), 632, 570),
        NativeRadarRect {
            x: 4,
            y: 44,
            w: 8,
            h: 13
        }
    );
    assert_eq!(
        native_viewport_rect(tall, (179, 0), 632, 570),
        NativeRadarRect {
            x: 55,
            y: 58,
            w: 8,
            h: 13
        }
    );
}

#[test]
fn native_viewport_rect_exhausts_lower_right_equality_and_all_four_corners() {
    // Generated 140x83, w=10/h=17, horizontal half=4, vertical half=8.
    // `0x006570AD..0x0065712E` uses a strict lower-edge comparison and a
    // `size - 1 - rect_size` clamp, leaving the final generated row/column
    // outside the current camera-window outline.
    let wide = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    assert_eq!(wide.generated_size(), (140, 83));
    let fixture = |center| native_viewport_rect_from_center(wide, center, 632, 570);

    assert_eq!(
        fixture((3, 7)),
        NativeRadarRect {
            x: 0,
            y: 0,
            w: 10,
            h: 17
        }
    );
    assert_eq!(
        fixture((4, 8)),
        NativeRadarRect {
            x: 0,
            y: 0,
            w: 10,
            h: 17
        }
    );
    assert_eq!(
        fixture((134, 7)),
        NativeRadarRect {
            x: 129,
            y: 0,
            w: 10,
            h: 17
        }
    );
    assert_eq!(
        fixture((3, 74)),
        NativeRadarRect {
            x: 0,
            y: 65,
            w: 10,
            h: 17
        }
    );
    assert_eq!(
        fixture((134, 74)),
        NativeRadarRect {
            x: 129,
            y: 65,
            w: 10,
            h: 17
        }
    );

    // Just inside, equality, and just outside all settle on the native
    // lower clamp point; equality enters the clamp branch (`JL` is false).
    for center_x in [133, 134, 135] {
        assert_eq!(fixture((center_x, 40)).x, 129, "center_x={center_x}");
    }
    for center_y in [73, 74, 75] {
        assert_eq!(fixture((70, center_y)).y, 65, "center_y={center_y}");
    }

    // Repeat the boundary table through the height-constrained 64x108 branch.
    let tall = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 180, 300).unwrap();
    assert_eq!(tall.generated_size(), (64, 108));
    let fixture = |center| native_viewport_rect_from_center(tall, center, 632, 570);
    assert_eq!(
        fixture((2, 5)),
        NativeRadarRect {
            x: 0,
            y: 0,
            w: 8,
            h: 13
        }
    );
    assert_eq!(
        fixture((59, 5)),
        NativeRadarRect {
            x: 55,
            y: 0,
            w: 8,
            h: 13
        }
    );
    assert_eq!(
        fixture((2, 101)),
        NativeRadarRect {
            x: 0,
            y: 94,
            w: 8,
            h: 13
        }
    );
    assert_eq!(
        fixture((59, 101)),
        NativeRadarRect {
            x: 55,
            y: 94,
            w: 8,
            h: 13
        }
    );
}

#[test]
fn native_viewport_rect_stores_vertical_divisor_and_preserves_oversize_branch_order() {
    let tiny = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 19, 10).unwrap();
    assert_eq!(tiny.generated_size(), (140, 73));

    // At `0x00657050`, native stores 30/zoom to f32. With the old extended
    // Rust quotient this fixture produced h=280 and half-height=140.
    assert_eq!(
        native_viewport_rect_from_center(tiny, (0, 0), 632, 570),
        NativeRadarRect {
            x: 0,
            y: 0,
            w: 156,
            h: 279
        },
    );

    // The clamp is an ordered if/else-if on each axis. It deliberately does
    // not revisit the lower bound after an oversize right/bottom clamp, so an
    // oversize window approached from inside can acquire a negative origin.
    assert_eq!(
        native_viewport_rect_from_center(tiny, (300, 300), 632, 570),
        NativeRadarRect {
            x: -17,
            y: -207,
            w: 156,
            h: 279
        },
    );
}

#[test]
fn native_viewport_state_dirties_previous_then_rebuild_suppresses_baseline_replay() {
    let first = NativeRadarRect {
        x: 4,
        y: 5,
        w: 8,
        h: 6,
    };
    let second = NativeRadarRect {
        x: 9,
        y: 7,
        w: 8,
        h: 6,
    };
    let mut state = NativeRadarViewportState::default();
    let initial = state.update(first);
    assert_eq!(initial.previous, first);
    assert!(initial.dirty_previous_border.is_empty());

    let moved = state.update(second);
    assert_eq!(moved.previous, first);
    assert_eq!(moved.current, second);
    assert_eq!(moved.dirty_previous_border.len(), 28);
    assert_eq!(
        &moved.dirty_previous_border[..4],
        &[(4, 5), (11, 5), (4, 6), (11, 6)]
    );

    state.reset_for_rebuild();
    let rebuilt = state.update(second);
    assert_eq!(rebuilt.previous, second);
    assert!(rebuilt.dirty_previous_border.is_empty());
    state.reset_for_rebuild();
    assert!(state.update(second).dirty_previous_border.is_empty());
}

#[test]
fn native_previous_border_visits_keep_duplicates_and_native_edge_order() {
    let rect = NativeRadarRect {
        x: 4,
        y: 5,
        w: 3,
        h: 2,
    };
    assert_eq!(
        native_previous_border_visits(rect),
        vec![
            (4, 5),
            (6, 5),
            (4, 6),
            (6, 6),
            (4, 5),
            (4, 6),
            (5, 5),
            (5, 6),
            (6, 5),
            (6, 6),
        ],
    );
    assert_eq!(
        native_previous_border_visits(NativeRadarRect {
            x: 4,
            y: 5,
            w: 1,
            h: 1
        }),
        vec![(4, 5), (4, 5), (4, 5), (4, 5)],
        "MarkCellDirty receives all four corner visits even for one pixel",
    );
    assert_eq!(
        native_previous_border_visits(NativeRadarRect {
            x: 4,
            y: 5,
            w: 0,
            h: 2
        }),
        vec![(4, 5), (3, 5), (4, 6), (3, 6)],
        "zero width skips only the horizontal loop",
    );
    assert_eq!(
        native_previous_border_visits(NativeRadarRect {
            x: 4,
            y: 5,
            w: 2,
            h: 0
        }),
        vec![(4, 5), (4, 4), (5, 5), (5, 4)],
        "zero height skips only the vertical loop",
    );
    assert!(
        native_previous_border_visits(NativeRadarRect {
            x: 4,
            y: 5,
            w: 0,
            h: 0
        })
        .is_empty()
    );
}

#[test]
fn native_viewport_outline_is_one_surface_pixel_and_uses_injected_sidebar_color() {
    let surface = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    let screen = NativeRadarScreenGeometry::new(surface, [100.0, 50.0, 280.0, 216.0]);
    let rect = NativeRadarRect {
        x: 10,
        y: 12,
        w: 8,
        h: 6,
    };
    let color = [164.0 / 255.0, 210.0 / 255.0, 1.0];
    let lines = native_viewport_outline_instances(
        (3.0, 4.0),
        screen,
        rect,
        [84.0, 0.0, 336.0, 600.0],
        color,
    );
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0].position, [123.0, 102.0]);
    assert_eq!(lines[0].size, [16.0, 2.0]);
    assert_eq!(lines[1].position, [137.0, 102.0]);
    assert_eq!(lines[1].size, [2.0, 12.0]);
    assert_eq!(lines[2].position, [123.0, 112.0]);
    assert_eq!(lines[3].position, [123.0, 102.0]);
    assert_eq!(lines[3].size, [2.0, 12.0]);
    assert!(lines.iter().all(|line| line.tint == color));
}

#[test]
fn native_viewport_lower_right_outline_is_inclusive_and_leaves_native_final_pixel_gap() {
    let surface = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    let screen = NativeRadarScreenGeometry::new(surface, [0.0, 0.0, 140.0, 108.0]);
    let rect = NativeRadarRect {
        x: 129,
        y: 65,
        w: 10,
        h: 17,
    };
    let lines = native_viewport_outline_instances(
        (0.0, 0.0),
        screen,
        rect,
        [-16.0, 0.0, 168.0, 600.0],
        [1.0; 3],
    );
    // Generated 140x83 is centered at sidebar-aperture y=12. Inclusive axis
    // endpoints are x=129..138/y=77..93; generated x=139/y=94 remain clear.
    assert_eq!(lines[0].position, [129.0, 77.0]);
    assert_eq!(lines[0].size, [10.0, 1.0]);
    assert_eq!(lines[1].position, [138.0, 77.0]);
    assert_eq!(lines[1].size, [1.0, 17.0]);
    assert_eq!(lines[2].position, [129.0, 93.0]);
    assert_eq!(lines[2].size, [10.0, 1.0]);
    assert_eq!(lines[3].position, [129.0, 77.0]);
    assert_eq!(lines[3].size, [1.0, 17.0]);
}

#[test]
fn native_viewport_outline_clips_all_sidebar_edges_with_inclusive_equality() {
    let surface = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 140, 108).unwrap();
    let screen = NativeRadarScreenGeometry::new(surface, [216.0, 49.0, 140.0, 108.0]);
    let sidebar = [200.0, 0.0, 168.0, 300.0];

    let horizontal = native_viewport_outline_instances(
        (0.0, 0.0),
        screen,
        NativeRadarRect {
            x: -20,
            y: -49,
            w: 200,
            h: 10,
        },
        sidebar,
        [1.0; 3],
    );
    assert_eq!(horizontal.len(), 2);
    assert_eq!(horizontal[0].position, [200.0, 0.0]);
    assert_eq!(horizontal[0].size, [168.0, 1.0]);
    assert_eq!(horizontal[1].position, [200.0, 9.0]);
    assert_eq!(horizontal[1].size, [168.0, 1.0]);

    let vertical = native_viewport_outline_instances(
        (0.0, 0.0),
        screen,
        NativeRadarRect {
            x: -16,
            y: -60,
            w: 168,
            h: 400,
        },
        sidebar,
        [1.0; 3],
    );
    assert_eq!(vertical.len(), 2);
    assert_eq!(vertical[0].position, [367.0, 0.0]);
    assert_eq!(vertical[0].size, [1.0, 300.0]);
    assert_eq!(vertical[1].position, [200.0, 0.0]);
    assert_eq!(vertical[1].size, [1.0, 300.0]);

    // x=151 relative to the generated surface lands on the sidebar's final
    // inclusive column (367). x=152 lands at the exclusive edge and vanishes.
    let last_column = native_viewport_outline_instances(
        (0.0, 0.0),
        screen,
        NativeRadarRect {
            x: 151,
            y: 0,
            w: 1,
            h: 1,
        },
        sidebar,
        [1.0; 3],
    );
    assert_eq!(last_column.len(), 4);
    assert!(
        last_column
            .iter()
            .all(|line| line.position == [367.0, 49.0])
    );
    assert!(last_column.iter().all(|line| line.size == [1.0, 1.0]));
    assert!(
        native_viewport_outline_instances(
            (0.0, 0.0),
            screen,
            NativeRadarRect {
                x: 152,
                y: 0,
                w: 1,
                h: 1
            },
            sidebar,
            [1.0; 3],
        )
        .is_empty()
    );

    let last_row = native_viewport_outline_instances(
        (0.0, 0.0),
        screen,
        NativeRadarRect {
            x: 0,
            y: 250,
            w: 1,
            h: 1,
        },
        sidebar,
        [1.0; 3],
    );
    assert_eq!(last_row.len(), 4);
    assert!(last_row.iter().all(|line| line.position == [216.0, 299.0]));
    assert!(
        native_viewport_outline_instances(
            (0.0, 0.0),
            screen,
            NativeRadarRect {
                x: 0,
                y: 251,
                w: 1,
                h: 1
            },
            sidebar,
            [1.0; 3],
        )
        .is_empty()
    );
}

#[test]
fn native_viewport_outline_preserves_thin_zero_and_fully_outside_contracts() {
    let surface = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 140, 108).unwrap();
    let screen = NativeRadarScreenGeometry::new(surface, [16.0, 49.0, 140.0, 108.0]);
    let sidebar = [0.0, 0.0, 168.0, 300.0];

    let one_pixel = native_viewport_outline_instances(
        (0.0, 0.0),
        screen,
        NativeRadarRect {
            x: 10,
            y: 10,
            w: 1,
            h: 1,
        },
        sidebar,
        [1.0; 3],
    );
    assert_eq!(one_pixel.len(), 4, "native calls all four line slots");
    assert!(one_pixel.iter().all(|line| line.position == [26.0, 59.0]));
    assert!(one_pixel.iter().all(|line| line.size == [1.0, 1.0]));

    let vertical = native_viewport_outline_instances(
        (0.0, 0.0),
        screen,
        NativeRadarRect {
            x: 10,
            y: 10,
            w: 1,
            h: 3,
        },
        sidebar,
        [1.0; 3],
    );
    assert_eq!(vertical[0].size, [1.0, 1.0]);
    assert_eq!(vertical[1].size, [1.0, 3.0]);
    assert_eq!(vertical[2].position, [26.0, 61.0]);
    assert_eq!(vertical[3].size, [1.0, 3.0]);

    // The rectangle worker performs `right=x+w-1` / `bottom=y+h-1`
    // without rejecting zero extents, yielding the four sides of a reversed
    // 2x2 inclusive box.
    let zero = native_viewport_outline_instances(
        (0.0, 0.0),
        screen,
        NativeRadarRect {
            x: 10,
            y: 10,
            w: 0,
            h: 0,
        },
        sidebar,
        [1.0; 3],
    );
    assert_eq!(zero.len(), 4);
    assert_eq!(zero[0].position, [25.0, 59.0]);
    assert_eq!(zero[0].size, [2.0, 1.0]);
    assert_eq!(zero[1].position, [25.0, 58.0]);
    assert_eq!(zero[1].size, [1.0, 2.0]);
    assert_eq!(zero[2].position, [25.0, 58.0]);
    assert_eq!(zero[2].size, [2.0, 1.0]);
    assert_eq!(zero[3].position, [26.0, 58.0]);
    assert_eq!(zero[3].size, [1.0, 2.0]);

    assert!(
        native_viewport_outline_instances(
            (0.0, 0.0),
            screen,
            NativeRadarRect {
                x: -500,
                y: -500,
                w: 4,
                h: 4
            },
            sidebar,
            [1.0; 3],
        )
        .is_empty()
    );
}

#[test]
fn native_viewport_outline_scales_then_clips_without_sidebar_bleed() {
    let surface = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 140, 108).unwrap();
    let screen = NativeRadarScreenGeometry::new(surface, [208.0, 24.5, 70.0, 54.0]);
    let sidebar = [200.0, 0.0, 84.0, 150.0];
    let unsampled_phase = native_viewport_outline_instances(
        (13.0, 17.0),
        screen,
        NativeRadarRect {
            x: -20,
            y: -50,
            w: 200,
            h: 200,
        },
        sidebar,
        [1.0; 3],
    );
    assert!(
        unsampled_phase.is_empty(),
        "0.5x nearest sampling selects odd source rows; even row 198 vanishes",
    );

    let lines = native_viewport_outline_instances(
        (13.0, 17.0),
        screen,
        NativeRadarRect {
            x: -20,
            y: -50,
            w: 200,
            h: 201,
        },
        sidebar,
        [1.0; 3],
    );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].position, [213.0, 116.0]);
    assert_eq!(lines[0].size, [84.0, 1.0]);

    for line in lines {
        let left = line.position[0] - 13.0;
        let top = line.position[1] - 17.0;
        assert!(left >= sidebar[0] && top >= sidebar[1]);
        assert!(left + line.size[0] <= sidebar[0] + sidebar[2]);
        assert!(top + line.size[1] <= sidebar[1] + sidebar[3]);
    }
}

#[test]
fn native_content_boundary_is_generated_size_plus_two_for_wide_and_tall() {
    let color = [160.0 / 255.0, 208.0 / 255.0, 248.0 / 255.0];
    let sidebar = [0.0, 0.0, 168.0, 300.0];

    let wide = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    assert_eq!(wide.generated_size(), (140, 83));
    let wide_lines = native_content_boundary_outline_instances(
        (0.0, 0.0),
        NativeRadarScreenGeometry::new(wide, [16.0, 49.0, 140.0, 108.0]),
        wide,
        sidebar,
        color,
    );
    assert_eq!(wide_lines.len(), 4);
    assert_eq!(wide_lines[0].position, [15.0, 60.0]);
    assert_eq!(wide_lines[0].size, [142.0, 1.0]);
    assert_eq!(wide_lines[1].position, [156.0, 60.0]);
    assert_eq!(wide_lines[1].size, [1.0, 85.0]);
    assert_eq!(wide_lines[2].position, [15.0, 144.0]);
    assert_eq!(wide_lines[3].position, [15.0, 60.0]);
    assert!(wide_lines.iter().all(|line| line.tint == color));

    let tall = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 180, 300).unwrap();
    assert_eq!(tall.generated_size(), (64, 108));
    let tall_lines = native_content_boundary_outline_instances(
        (0.0, 0.0),
        NativeRadarScreenGeometry::new(tall, [16.0, 49.0, 140.0, 108.0]),
        tall,
        sidebar,
        color,
    );
    assert_eq!(tall_lines.len(), 4);
    assert_eq!(tall_lines[0].position, [53.0, 48.0]);
    assert_eq!(tall_lines[0].size, [66.0, 1.0]);
    assert_eq!(tall_lines[1].position, [118.0, 48.0]);
    assert_eq!(tall_lines[1].size, [1.0, 110.0]);
    assert_eq!(tall_lines[2].position, [53.0, 157.0]);
    assert_eq!(tall_lines[3].position, [53.0, 48.0]);
}

#[test]
fn native_content_boundary_clips_to_sidebar_edges_not_generated_content() {
    let surface = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 140, 108).unwrap();
    let lines = native_content_boundary_outline_instances(
        (7.0, 11.0),
        NativeRadarScreenGeometry::new(surface, [0.0, 0.0, 140.0, 108.0]),
        surface,
        [0.0, 0.0, 141.0, 108.0],
        [1.0; 3],
    );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].position, [147.0, 11.0]);
    assert_eq!(lines[0].size, [1.0, 108.0]);
    assert_eq!(lines[0].position[0] - 7.0, 140.0);
}

fn outline_source_pixels(
    rect: NativeRadarRect,
    source_content: (i32, i32),
) -> BTreeSet<(i32, i32)> {
    let left = source_content.0.wrapping_add(rect.x);
    let top = source_content.1.wrapping_add(rect.y);
    let right = left.wrapping_add(rect.w).wrapping_sub(1);
    let bottom = top.wrapping_add(rect.h).wrapping_sub(1);
    let mut pixels = BTreeSet::new();
    for x in left.min(right)..=left.max(right) {
        pixels.insert((x, top));
        pixels.insert((x, bottom));
    }
    for y in top.min(bottom)..=top.max(bottom) {
        pixels.insert((left, y));
        pixels.insert((right, y));
    }
    pixels
}

fn nearest_scaled_reference(
    source_pixels: &BTreeSet<(i32, i32)>,
    sidebar_surface: [f32; 4],
    scale: (f32, f32),
) -> BTreeSet<(i32, i32)> {
    let left = shader_pixel_round(sidebar_surface[0]);
    let top = shader_pixel_round(sidebar_surface[1]);
    let right = shader_pixel_round(sidebar_surface[0] + sidebar_surface[2]);
    let bottom = shader_pixel_round(sidebar_surface[1] + sidebar_surface[3]);
    let mut pixels = BTreeSet::new();
    for y in top..bottom {
        let source_y = (((y as f32 + 0.5) - sidebar_surface[1]) / scale.1).floor() as i32;
        for x in left..right {
            let source_x = (((x as f32 + 0.5) - sidebar_surface[0]) / scale.0).floor() as i32;
            if source_pixels.contains(&(source_x, source_y)) {
                pixels.insert((x, y));
            }
        }
    }
    pixels
}

fn shader_raster_pixels(instances: &[SpriteInstance], camera: (f32, f32)) -> BTreeSet<(i32, i32)> {
    let mut pixels = BTreeSet::new();
    for instance in instances {
        let raw_left = instance.position[0] - camera.0;
        let raw_top = instance.position[1] - camera.1;
        let raw_right = raw_left + instance.size[0];
        let raw_bottom = raw_top + instance.size[1];
        let left = shader_pixel_round(raw_left);
        let top = shader_pixel_round(raw_top);
        let right = shader_pixel_round(raw_right);
        let bottom = shader_pixel_round(raw_bottom);
        assert_eq!(
            raw_left, left as f32,
            "adapter must submit an integer left edge"
        );
        assert_eq!(
            raw_top, top as f32,
            "adapter must submit an integer top edge"
        );
        assert_eq!(
            raw_right, right as f32,
            "adapter must submit an integer right edge"
        );
        assert_eq!(
            raw_bottom, bottom as f32,
            "adapter must submit an integer bottom edge"
        );
        for y in top..bottom {
            for x in left..right {
                pixels.insert((x, y));
            }
        }
    }
    pixels
}

#[test]
fn native_outlines_match_retained_surface_nearest_raster_at_half_one_and_one_half() {
    let surface = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    assert_eq!(surface.generated_size(), (140, 83));
    let camera = (7.25, -2.5);
    let viewport = NativeRadarRect {
        x: 10,
        y: 12,
        w: 8,
        h: 6,
    };
    let boundary = NativeRadarRect {
        x: -1,
        y: -1,
        w: 142,
        h: 85,
    };
    // Native aperture starts at source (16,49); the height-constrained raw
    // surface is centred another 12 source rows down.
    let source_content = (16, 61);

    for scale in [0.5_f32, 1.0, 1.5] {
        let sidebar = [101.0, 3.0, 168.0 * scale, 220.0 * scale];
        let aperture = [
            sidebar[0] + 16.0 * scale,
            sidebar[1] + 49.0 * scale,
            140.0 * scale,
            108.0 * scale,
        ];
        let screen = NativeRadarScreenGeometry::new(surface, aperture);

        let viewport_instances =
            native_viewport_outline_instances(camera, screen, viewport, sidebar, [1.0; 3]);
        let viewport_actual = shader_raster_pixels(&viewport_instances, camera);
        let viewport_expected = nearest_scaled_reference(
            &outline_source_pixels(viewport, source_content),
            sidebar,
            (scale, scale),
        );
        assert_eq!(
            viewport_actual, viewport_expected,
            "camera outline scale={scale}"
        );

        let boundary_instances =
            native_content_boundary_outline_instances(camera, screen, surface, sidebar, [1.0; 3]);
        let boundary_actual = shader_raster_pixels(&boundary_instances, camera);
        let boundary_expected = nearest_scaled_reference(
            &outline_source_pixels(boundary, source_content),
            sidebar,
            (scale, scale),
        );
        assert_eq!(
            boundary_actual, boundary_expected,
            "content boundary scale={scale}"
        );
        assert!(
            !viewport_actual.is_empty(),
            "fixture must cover camera pixels at scale={scale}"
        );
        assert!(
            !boundary_actual.is_empty(),
            "fixture must cover boundary pixels at scale={scale}"
        );
    }
}
