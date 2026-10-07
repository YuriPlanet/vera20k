//! Stock pavement through the actual live publisher and save/restore boundary.
//! This proves the perpendicular entry and scalar core; engineer fallback and
//! GPU composition remain separate acceptance requirements.
use super::*;
use crate::sim::snapshot::GameSnapshot;

#[test]
#[ignore = "requires retail assets and VERA20K_XMP34U4_MAP"]
fn retail_plain_pavement_native_entry_draw_selection_and_snapshot() {
    let retail = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            crate::util::config::GameConfig::load()
                .unwrap()
                .paths
                .ra2_dir
        });
    let map = std::env::var("VERA20K_XMP34U4_MAP").unwrap();
    let mut scenario = crate::headless_scenario::load(&retail, &map, 0x0B21_D6E5).unwrap();
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_pavement.json",
    ))
    .unwrap();
    let original = &corpus["stock"]["perpendicular"]["result"];
    assert_eq!(original["trace"], corpus["stock"]["steps"][0]["trace"]);
    let initial = scenario.sim().resolved_terrain.as_ref().unwrap().clone();
    // A plain cell carries no bridge deck or bridge walkability at map load.
    let plain_at_load = |p: (u16, u16)| {
        initial.cell(p.0, p.1).is_some_and(|cell| {
            !cell.bridge_facts.has_structural_bridge()
                && !cell.has_bridge_deck
                && !cell.bridge_walkable
        })
    };
    let runtime = &mut scenario.runtime;
    runtime.simulation.radar_terrain_dirty_cells.clear();
    let before_hash = runtime.simulation.state_hash();
    let mut live = LivePublication {
        sim: &mut runtime.simulation,
        rules: &runtime.resources.rules,
        registry: Some(&runtime.resources.overlay_registry),
        collapsed: false,
    };
    assert!(plain_at_load((66, 102)));
    live.perpendicular((67, 102), Axis::NS, Phase::DamageB, 6, Family::High);
    let expected_radar: Vec<(u16, u16)> = original["trace"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "radar")
        .map(|event| {
            (
                event["coord"][0].as_u64().unwrap() as u16,
                event["coord"][1].as_u64().unwrap() as u16,
            )
        })
        .collect();
    assert_eq!(expected_radar.len(), 15);
    assert_eq!(live.sim.radar_terrain_dirty_cells, expected_radar);
    assert_ne!(
        live.sim.state_hash(),
        before_hash,
        "plain pavement contributes to deterministic state"
    );
    let mut plain = 0;
    for row in original["final"].as_array().unwrap() {
        let p = (
            row[0].as_u64().unwrap() as u16,
            row[1].as_u64().unwrap() as u16,
        );
        let cell = live.terrain().cell(p.0, p.1).unwrap();
        assert_eq!(cell.final_tile_index, row[2].as_i64().unwrap() as i32);
        assert_eq!(cell.bridge_facts.raw_flags, row[3].as_u64().unwrap() as u32);
        if expected_radar.contains(&p) {
            assert_eq!(
                live.sim.dynamic_terrain_cells.get(&p),
                Some(&DynamicTerrainCellState::capture(cell))
            );
            assert_eq!(live.terrain().pavement_draw_variant(p.0, p.1), Some(1));
            plain += usize::from(plain_at_load(p));
        }
    }
    assert_eq!(plain, 11);
    let bytes = GameSnapshot::save_validated(live.sim, 1, 2, "Pavement", 3);
    let mut restored = GameSnapshot::load_validated(&bytes, 1, 2, &live.sim.session.map_name)
        .unwrap()
        .sim;
    assert_eq!(
        restored.dynamic_terrain_cells,
        live.sim.dynamic_terrain_cells
    );
    restored.resolved_terrain = Some(initial.clone());
    restored.restore_after_snapshot_load().unwrap();
    restored
        .restore_map_authority_after_snapshot_load(live.rules, live.registry.unwrap())
        .unwrap();
    for &p in &expected_radar {
        let terrain = restored.resolved_terrain.as_ref().unwrap();
        assert!(terrain.pavement_damaged_at(p.0, p.1));
        assert_eq!(terrain.pavement_draw_variant(p.0, p.1), Some(1));
        assert_eq!(
            terrain.current_tile_radar_metadata(p.0, p.1),
            live.terrain().current_tile_radar_metadata(p.0, p.1)
        );
    }
    // Core repetition and explicit clear use the original56E990 corpus. This
    // is not an assertion that the missing engineer fallback already works.
    for step in corpus["stock"]["steps"].as_array().unwrap().iter().skip(1) {
        live.sim.radar_terrain_dirty_cells.clear();
        live.pavement_at((66, 102), step["state"] == 1);
        for row in step["final"].as_array().unwrap() {
            let cell = live
                .terrain()
                .cell(
                    row[0].as_u64().unwrap() as u16,
                    row[1].as_u64().unwrap() as u16,
                )
                .unwrap();
            assert_eq!(cell.bridge_facts.raw_flags, row[3].as_u64().unwrap() as u32);
        }
    }
    println!(
        "15 native pavement cells, including11 plain: original entry/order, retained flags, draw selection, and validated snapshot restoration passed"
    );
}

#[test]
fn retained_pavement_restore_preserves_damaged_radar_colors_and_validity() {
    use crate::map::resolved_terrain::RadarColorMetadata;
    for valid in [false, true] {
        let mut cell = crate::sim::world::lifecycle_tests::common_raw_terrain_cell(0, 0, 0, false);
        cell.final_tile_index = 42;
        cell.has_damaged_data = true;
        cell.radar_left = [1, 2, 3];
        cell.radar_right = [4, 5, 6];
        let mut initial = ResolvedTerrainGrid::from_cells(1, 1, vec![cell]);
        initial.test_set_radar_color_valid(0, 0, false);
        let damaged = RadarColorMetadata {
            left: [100, 110, 120],
            right: [130, 140, 150],
            valid,
        };
        initial.test_set_damaged_radar_metadata(0, 0, damaged);
        let mut live = initial.clone();
        let native = live.native_cell_identity((0, 0));
        live.write_pavement_flags(
            native,
            live.native_cell_flags(native) | crate::map::bridge_pavement::DAMAGED_PAVEMENT,
        );
        let saved = DynamicTerrainCellState::capture(live.cell(0, 0).unwrap());
        assert!(initial.apply_dynamic_cell_state(0, 0, &saved));
        assert_eq!(initial.current_tile_radar_metadata(0, 0), Some(damaged));
        assert!(
            !initial.radar_color_valid(0, 0),
            "pristine sparse validity must also survive scalar restore"
        );
    }
}
