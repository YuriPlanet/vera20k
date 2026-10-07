//! Original Object/Bullet render geometry consumed by the production builder.
use super::*;
use serde_json::Value;

fn flag(input: &Value, key: &str, default: bool) -> bool {
    input[key]
        .as_bool()
        .or_else(|| input[key].as_i64().map(|n| n != 0))
        .unwrap_or(default)
}

fn grid(input: &Value) -> ResolvedTerrainGrid {
    let mut grid = crate::map::resolved_terrain::test_flat_ground_grid(32);
    grid.test_set_native_allocated_cells(&[(10, 20)]);
    let cell = grid.cell_mut(10, 20).unwrap();
    cell.level = input["level"].as_i64().unwrap_or(0) as u8;
    cell.slope_type = input["slope"].as_u64().unwrap_or(0) as u8;
    cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap_or(0) as u32;
    grid
}

fn check_row(input: &Value, row: &Value, terrain: &ResolvedTerrainGrid) {
    let xyz = &row["xyz"];
    let coord = ProjectileCoord::new(
        xyz[0].as_i64().unwrap() as i32,
        xyz[1].as_i64().unwrap() as i32,
        xyz[2].as_i64().unwrap() as i32,
    );
    let (ground, structural) = ground_probe(terrain, coord);
    let actual = geometry(
        coord,
        ground,
        structural,
        flag(input, "on_bridge", false),
        flag(input, "shadow", true),
    );
    let camera = [
        input["camera"][0].as_i64().unwrap_or(-600) as f32,
        input["camera"][1].as_i64().unwrap_or(0) as f32 + 15.0,
    ];
    let viewport = [
        input["viewport"][2].as_i64().unwrap_or(800) as f32,
        input["viewport"][3].as_i64().unwrap_or(600) as f32,
    ];
    assert_eq!(
        [
            actual.body.point[0] - camera[0],
            actual.body.point[1] - camera[1]
        ],
        [
            row["projection"][0].as_i64().unwrap() as f32,
            row["projection"][1].as_i64().unwrap() as f32
        ],
        "{input}"
    );
    assert_eq!(
        projection_admitted(actual.body.point, camera, viewport),
        row["projection_visible"] == 1,
        "{input}: padded viewport"
    );
    let draws = row["draws"].as_array().unwrap();
    if draws.is_empty() {
        return;
    }
    let actual_pieces: Vec<_> = actual
        .shadow
        .into_iter()
        .chain(std::iter::once(actual.body))
        .collect();
    assert_eq!(actual_pieces.len(), draws.len(), "{input}");
    let vp = [
        input["viewport"][0].as_i64().unwrap_or(0),
        input["viewport"][1].as_i64().unwrap_or(0),
    ];
    let dirty = [
        input["dirty"][0].as_i64().unwrap_or(vp[0]),
        input["dirty"][1].as_i64().unwrap_or(vp[1]),
    ];
    for (piece, native) in actual_pieces.iter().zip(draws) {
        assert_eq!(
            [
                piece.point[0] - camera[0] + (vp[0] - dirty[0]) as f32,
                piece.point[1] - camera[1] + (vp[1] - dirty[1]) as f32
            ],
            [
                native["point"][0].as_i64().unwrap() as f32,
                native["point"][1].as_i64().unwrap() as f32
            ],
            "{input}"
        );
        assert_eq!(
            i64::from(piece.z_adjust),
            native["z_adjust"].as_i64().unwrap(),
            "{input}"
        );
    }
}

#[test]
fn retained_bullet_geometry_and_read_only_live_bridge_probe_match_native() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/bridge_render.json",
    ))
    .unwrap();
    for row in corpus["rows"].as_array().unwrap() {
        let mut sim = crate::sim::world::Simulation::with_seed(31);
        sim.install_resolved_terrain_for_new_map(grid(&row["input"]));
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        terrain.shared_cell_dummy().stamp_coord(123, -44);
        let before = sim.state_hash();
        let dummy_before = terrain.shared_cell_dummy().snapshot();
        check_row(&row["input"], row, terrain);
        assert_eq!(sim.state_hash(), before, "presentation changed simulation");
        assert_eq!(terrain.shared_cell_dummy().snapshot(), dummy_before);
    }
    for sequence in corpus["live_structural_sequences"].as_array().unwrap() {
        let mut terrain = grid(&sequence["input"]);
        for row in sequence["rows"].as_array().unwrap() {
            terrain.cell_mut(10, 20).unwrap().bridge_facts.raw_flags =
                row["flags"].as_u64().unwrap() as u32;
            check_row(&sequence["input"], row, &terrain);
        }
    }
}
