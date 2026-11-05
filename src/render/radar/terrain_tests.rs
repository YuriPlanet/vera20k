use super::*;

#[test]
fn fill_terrain_colors_preserves_two_halves_and_clipped_edges() {
    let mut raw = vec![[0; 3]; 5];
    fill_raw_cell(&mut raw, (5, 1), (-1, 0), [1, 2, 3], [11, 12, 13]);
    fill_raw_cell(&mut raw, (5, 1), (1, 0), [21, 22, 23], [31, 32, 33]);
    fill_raw_cell(&mut raw, (5, 1), (4, 0), [41, 42, 43], [51, 52, 53]);
    assert_eq!(
        raw,
        vec![
            [11, 12, 13],
            [21, 22, 23],
            [31, 32, 33],
            [0, 0, 0],
            [41, 42, 43],
        ]
    );
}

#[test]
fn width_constrained_surface_matches_native_weighted_sampler_every_pixel() {
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 173, 113).unwrap();
    assert_eq!(geometry.generated_size(), (140, 91));
    assert_surface_matches_native((173, 113), geometry.generated_size());
}

#[test]
fn height_constrained_surface_matches_native_weighted_sampler_every_pixel() {
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 137, 149).unwrap();
    assert_eq!(geometry.generated_size(), (99, 108));
    assert_surface_matches_native((137, 149), geometry.generated_size());
}

#[test]
fn overlay_color_replaces_both_halves_before_weighted_generation() {
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 2, 1).unwrap();
    let surface = NativeRadarTerrainSurface::new(
        geometry,
        vec![NativeRadarCellColors {
            cell: (0, 0),
            left: [10, 20, 30],
            right: [40, 50, 60],
        }],
        BTreeMap::from([((0, 0), [77, 88, 99])]),
        1.0,
    );
    assert_eq!(surface.raw_rgb, vec![[77, 88, 99], [77, 88, 99]]);
    assert!(
        surface
            .generated_rgb565()
            .iter()
            .all(|&pixel| pixel == RGB565.pack_rgb8([77, 88, 99]))
    );
}

/// Full original6547C0 stores, rather than a maintained second sampler.
fn assert_surface_matches_native(raw_size: (i32, i32), generated_size: (i32, i32)) {
    let packet = native_packet();
    let row = packet["radar"]["weighted_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["raw_size"] == serde_json::json!(raw_size))
        .unwrap();
    assert_eq!(row["input"]["pattern"], "terrain_tests_coordinate_v1");
    assert_eq!(
        row["output"]["generated_size"],
        serde_json::json!(generated_size)
    );
    let actual = generate_rgb565(&fixture(raw_size), raw_size, generated_size);
    assert_eq!(packed_hash(&actual), row["output"]["rgb565_sha256"]);
}

fn native_packet() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/spatial_oracle/ore_render.json")
}

fn packed_hash(pixels: &[u16]) -> String {
    crate::util::sha256::sha256_hex(
        &pixels
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

fn fixture(size: (i32, i32)) -> Vec<[u8; 3]> {
    (0..size.1)
        .flat_map(|y| {
            (0..size.0).map(move |x| {
                [
                    x.wrapping_mul(17).wrapping_add(y.wrapping_mul(3)) as u8,
                    x.wrapping_mul(5).wrapping_add(y.wrapping_mul(29)) as u8,
                    x.wrapping_mul(41).wrapping_add(y.wrapping_mul(7)) as u8,
                ]
            })
        })
        .collect()
}

/// Original655250 consumes the dirty Cell, queries47C060 and invokes6547C0.
/// This component transports the packet's declared geometry/background; normal
/// paid-map harvesting/upload/ack is covered by minimap_gpu_tests separately.
/// Native incremental vs full regeneration differs at five pixels in one3x2
/// patch/fringe row; this retained visual residual is not exact incremental proof.
#[test]
fn ore_depletion_current_pair_and_rebuilt_surface_match_original() {
    let packet = native_packet();
    let native = &packet["radar"]["depletion"];
    let input = &native["input"];
    assert_eq!(input["background_rgb"], serde_json::json!([0, 0, 0]));
    let number = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(
        number(&input["column_base"]),
        number(&input["row_base"]),
        number(&input["raw_size"][0]),
        number(&input["raw_size"][1]),
    )
    .unwrap();
    let coords = &native["terrain"]["coords"];
    let cell = (number(&coords[0]) as u16, number(&coords[1]) as u16);
    let rows = native["rows"].as_array().unwrap();
    let empty = rows.iter().find(|row| row["name"] == "empty").unwrap();
    let rgb = |value: &serde_json::Value| std::array::from_fn(|i| number(&value[i]) as u8);
    let base = NativeRadarCellColors {
        cell,
        left: rgb(&empty["color_pair"][0]),
        right: rgb(&empty["color_pair"][1]),
    };
    let mut surface = NativeRadarTerrainSurface::new(geometry, vec![base], BTreeMap::new(), 1.0);
    for row in rows {
        let override_color =
            (row["state"]["overlay_id"].as_i64().unwrap() >= 0).then(|| rgb(&row["color_pair"][0]));
        surface.set_cell_states([NativeRadarCellUpdate {
            base,
            override_color,
        }]);
        let raw: Vec<u8> = surface.raw_rgb().iter().flatten().copied().collect();
        assert_eq!(
            crate::util::sha256::sha256_hex(&raw),
            row["raw_rgb_sha256"],
            "raw {}",
            row["name"]
        );
        assert_eq!(
            packed_hash(surface.generated_rgb565()),
            row["rebuild_output"]["rgb565_sha256"],
            "generated {}",
            row["name"]
        );
        for run in row["rebuild_output"]["runs"].as_array().unwrap() {
            let start = number(&run[1]) * geometry.generated_size().0 + number(&run[0]);
            let count = number(&run[2]);
            let word = number(&run[3]) as u16;
            assert!(
                surface.generated_rgb565()[start as usize..(start + count) as usize]
                    .iter()
                    .all(|&actual| actual == word)
            );
        }
    }
}

#[test]
fn hd_terrain_resamples_changed_cells_like_full_generation() {
    // Native zoom ~0.68 and the HD zoom above 1: spans of one and two pixels.
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(80, 0, 161, 159).unwrap();
    let color = |x: u16, y: u16, half: u16| [(x * 3) as u8, (y * 3) as u8, (half * 200) as u8];
    let cells = (0..80u16)
        .flat_map(|x| (0..80u16).map(move |y| (x, y)))
        .map(|(x, y)| NativeRadarCellColors {
            cell: (x, y),
            left: color(x, y, 0),
            right: color(x, y, 1),
        })
        .collect();
    let mut surface = NativeRadarTerrainSurface::new(geometry, cells, BTreeMap::new(), 1.0);
    surface.set_hd_scale(Some(3));
    let (_, revision) = surface.hd_rgb565().unwrap();
    // An inner cell's colours, and an overlay on the raw surface's corner cell.
    let inner = NativeRadarCellColors {
        cell: (40, 41),
        left: [250, 1, 2],
        right: [3, 250, 4],
    };
    let corner = NativeRadarCellColors {
        cell: (79, 79),
        left: color(79, 79, 0),
        right: color(79, 79, 1),
    };
    assert!(surface.set_cell_states([
        NativeRadarCellUpdate {
            base: inner,
            override_color: None,
        },
        NativeRadarCellUpdate {
            base: corner,
            override_color: Some([9, 9, 250]),
        },
    ]));
    let (resampled, changed) = surface.hd_rgb565().unwrap();
    assert_ne!(changed, revision);
    let full = HdTerrain::new(surface.raw_rgb(), geometry, 3);
    assert_eq!(resampled, full.rgb565.as_slice());
}
