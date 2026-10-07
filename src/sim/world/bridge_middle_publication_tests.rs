//! Native high-perpendicular sequences through the production live host.
//! Retail loading supplies the inputs; the native corpus excludes its loader
//! and object-fallout callbacks. Whole damage dispatch/display remain separate.

use super::*;

#[test]
#[ignore = "requires retail assets and extracted c3y03md.map via VERA20K_C3Y03MD_MAP"]
fn retail_middle_perpendicular_live_tiles_match_original_sequences() {
    let retail = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            crate::util::config::GameConfig::load()
                .unwrap()
                .paths
                .ra2_dir
        });
    let map = std::env::var("VERA20K_C3Y03MD_MAP").unwrap_or_else(|_| "c3y03md.map".into());
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_middle_tiles.json",
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    let coord = |value: &serde_json::Value| {
        (
            value[0].as_i64().unwrap() as u16,
            value[1].as_i64().unwrap() as u16,
        )
    };
    let mut calls = 0;
    for case in cases {
        let mut scenario = crate::headless_scenario::load(&retail, &map, 0x0B21_D6E5).unwrap();
        let axis = if case["axis"] == "NS" {
            Axis::NS
        } else {
            Axis::EW
        };
        let input = coord(&case["input"]);
        let initial = scenario.sim().resolved_terrain.as_ref().unwrap().clone();
        let runtime = &mut scenario.runtime;
        for step in case["steps"].as_array().unwrap() {
            let phase = match step["phase"].as_str().unwrap() {
                "DamageA" => Phase::DamageA,
                "DamageB" => Phase::DamageB,
                "CollapseA" => Phase::CollapseA,
                "CollapseB" => Phase::CollapseB,
                _ => unreachable!(),
            };
            let mut live = LivePublication {
                sim: &mut runtime.simulation,
                rules: &runtime.resources.rules,
                registry: Some(&runtime.resources.overlay_registry),
                collapsed: false,
            };
            live.perpendicular(
                (input.0 as i16, input.1 as i16),
                axis,
                phase,
                case["direction"].as_u64().unwrap() as u8,
                Family::High,
            );
            calls += 1;
            let recalculated: std::collections::BTreeSet<_> = step["recalc"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| coord(&row["cell"]["coord"]))
                .collect();
            for change in step["changes"].as_array().unwrap() {
                let after = &change["after"];
                let p = coord(&after["coord"]);
                let cell = live.terrain().cell(p.0, p.1).unwrap();
                let context = format!("{axis:?}/{phase:?}/{p:?}");
                assert_eq!(
                    cell.final_tile_index,
                    after["tile"].as_i64().unwrap() as i32,
                    "{context}"
                );
                assert_eq!(
                    cell.bridge_facts.raw_flags,
                    after["flags"].as_u64().unwrap() as u32,
                    "{context}"
                );
                assert_eq!(
                    cell.bridge_facts.state_byte,
                    after["state"].as_u64().unwrap() as u8,
                    "{context}"
                );
                assert_eq!(
                    cell.bridge_facts.overlay_id.map_or(-1, i32::from),
                    after["overlay"].as_i64().unwrap() as i32,
                    "{context}"
                );
                if recalculated.contains(&p) {
                    let attrs = &after["attributes"];
                    assert_eq!(
                        cell.final_sub_tile,
                        attrs[0].as_u64().unwrap() as u8,
                        "{context}"
                    );
                    assert_eq!(cell.level, attrs[1].as_u64().unwrap() as u8, "{context}");
                    assert_eq!(
                        cell.slope_type,
                        attrs[2].as_u64().unwrap() as u8,
                        "{context}"
                    );
                    assert_eq!(
                        cell.height_in_pixels,
                        attrs[3].as_u64().unwrap() as u8 as i8,
                        "{context}"
                    );
                    assert_eq!(
                        cell.yr_cell_land_type,
                        after["land"].as_u64().unwrap() as u8,
                        "{context}"
                    );
                    assert_eq!(
                        cell.zone_type,
                        after["zone"].as_u64().unwrap() as u8,
                        "{context}"
                    );
                }
                assert_eq!(
                    live.sim.dynamic_terrain_cells.get(&p),
                    Some(&DynamicTerrainCellState::capture(cell)),
                    "{context}: persist each completed scalar/metadata write"
                );
            }
            let mut restored = initial.clone();
            for (&p, state) in &live.sim.dynamic_terrain_cells {
                assert!(restored.apply_dynamic_cell_state(p.0, p.1, state));
                assert_eq!(
                    DynamicTerrainCellState::capture(restored.cell(p.0, p.1).unwrap()),
                    DynamicTerrainCellState::capture(live.terrain().cell(p.0, p.1).unwrap())
                );
                assert_eq!(
                    restored.current_tile_radar_metadata(p.0, p.1),
                    live.terrain().current_tile_radar_metadata(p.0, p.1)
                );
            }
        }
    }
    println!(
        "12 retail sequences, {calls} native helper calls: live tile/flags/Recalc fields and retained restore passed"
    );
}
