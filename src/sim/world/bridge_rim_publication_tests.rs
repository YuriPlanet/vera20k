//! Actual damage-event caller against original body+perpendicular+rim output.
//! Stock scalar initialization is shared with Unicorn, not a loader comparison.

use super::*;
use crate::map::bridge_facts::BridgeCellFacts;
use crate::map::bridge_rim_tiles::HighBridgeRimTiles;
use serde_json::{Value, json};

fn stock_world(rules: &RuleSet, input: &Value) -> Simulation {
    let rows = input["cells"].as_array().unwrap();
    let height = rows
        .iter()
        .map(|r| r[1].as_u64().unwrap() as u16)
        .max()
        .unwrap()
        + 1;
    let cells = (0..height)
        .flat_map(|y| {
            (0..512).map(move |x| {
                crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(512, height, cells);
    let mut allocated = Vec::new();
    let mut grid = OverlayGrid::new(512, height);
    type RimRow = (
        u16,
        u16,
        i32,
        u8,
        u32,
        Option<u8>,
        u8,
        Option<(u16, u16)>,
        u8,
        u8,
    );
    for row in rows {
        let (x, y, tile, subtile, flags, overlay, state, anchor, level, land): RimRow =
            serde_json::from_value(row.clone()).unwrap();
        let index = usize::from(y) * 512 + usize::from(x);
        allocated.push((x, y));
        let cell = &mut terrain.cells_mut_for_test()[index];
        cell.final_tile_index = tile;
        cell.final_sub_tile = subtile;
        cell.level = level;
        cell.land_type = land;
        cell.bridge_facts = BridgeCellFacts {
            state_byte: state,
            overlay_id: overlay,
            family: if flags & 0x1f80 != 0 {
                BridgeStampFamily::Nesw
            } else {
                BridgeStampFamily::None
            },
            direction: (flags & 0x1f80 != 0).then_some(6),
            native_anchor: anchor
                .filter(|_| flags & 0x80 == 0)
                .map(|(x, y)| Cell::Real(usize::from(y) * 512 + usize::from(x))),
            ..Default::default()
        };
        terrain.write_native_cell_flags(Cell::Real(index), flags);
        if let Some(overlay) = overlay {
            grid.place_overlay(x, y, overlay, state);
        }
        grid.write_literal_bridge_state(&mut terrain, x, y, state);
    }
    terrain.test_set_native_allocated_cells(&allocated);
    let mut ini = String::from("[General]\n");
    for (key, value) in input["rim_keys"].as_object().unwrap() {
        ini.push_str(&format!("{key}={value}\n"));
    }
    terrain.test_set_high_bridge_rim_tiles(HighBridgeRimTiles::from_ini(
        input["bridge_base"].as_i64().unwrap() as i32,
        ini.as_bytes(),
    ));
    let size: (i32, i32) = serde_json::from_value(input["size"].clone()).unwrap();
    let state = BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 1, size);
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    // Map load installs the same Size, with its LocalSize, beside the bridges.
    let local: [u32; 4] = serde_json::from_value(input["local_size"].clone()).unwrap();
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: input["theater"].as_str().unwrap().to_owned(),
        fill: "Clear".to_owned(),
        level: 0,
        width: size.0 as u32,
        height: size.1 as u32,
        local_left: local[0],
        local_top: local[1],
        local_width: local[2],
        local_height: local[3],
    });
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.bridge_state = Some(state);
    sim.overlay_grid = Some(grid);
    sim
}

fn snapshot(sim: &Simulation, coord: (u16, u16)) -> Value {
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let cell = terrain.cell(coord.0, coord.1).unwrap();
    json!({"coord": coord, "flags": cell.bridge_facts.raw_flags,
        "state": cell.bridge_facts.state_byte,
        "overlay": cell.bridge_facts.overlay_id.map_or(-1, i32::from),
        "anchor": cell.bridge_facts.native_anchor.map(|a| terrain.native_cell_coord(a))})
}

#[test]
fn bridge_rim_stock_damage_events_match_original_body_perpendicular_and_cleanup() {
    let input: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_stock_inputs.json",
    ))
    .unwrap();
    let original: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_body.json",
    ))
    .unwrap();
    let coords: Vec<(u16, u16)> = input["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r[0].as_u64().unwrap() as u16, r[1].as_u64().unwrap() as u16))
        .collect();
    let rules = rules();
    for case in original["cases"].as_array().unwrap() {
        let mut sim = stock_world(&rules, &input);
        let mut expected: std::collections::BTreeMap<_, _> =
            coords.iter().map(|&c| (c, snapshot(&sim, c))).collect();
        for hit in case["hits"].as_array().unwrap() {
            let coord = serde_json::from_value(hit["input"].clone()).unwrap();
            let mut damage = event(&mut sim, coord);
            damage.impact_z_leptons = 416
                + 104
                    * i32::from(
                        sim.resolved_terrain
                            .as_ref()
                            .unwrap()
                            .cell(coord.0, coord.1)
                            .unwrap()
                            .level,
                    );
            sim.radar_terrain_dirty_cells.clear();
            let collapsed = apply_bridge_damage_events_with_overlay_registry(
                &mut sim,
                &rules,
                &[damage],
                None,
                crate::sim::world::FrameEffects::default(),
            );
            let calls = hit["calls"].as_array().unwrap();
            assert_eq!(
                collapsed,
                calls.iter().any(|c| c["kind"] == "fallout_sink"),
                "{} at{coord:?}",
                case["name"]
            );
            for change in hit["changes"].as_array().unwrap() {
                let coord = serde_json::from_value(change["before"]["coord"].clone()).unwrap();
                assert_eq!(expected[&coord], change["before"]);
                expected.insert(coord, change["after"].clone());
            }
            for (&coord, wanted) in &expected {
                assert_eq!(
                    &snapshot(&sim, coord),
                    wanted,
                    "{} hit{} at{coord:?}",
                    case["name"],
                    hit["input"]
                );
            }
            let mut radar: Vec<(u16, u16)> = Vec::new();
            for call in calls.iter().filter(|c| c["kind"] == "radar_sink") {
                let coord = serde_json::from_value(call["coord"].clone()).unwrap();
                if !radar.contains(&coord) {
                    radar.push(coord);
                }
            }
            assert_eq!(
                sim.radar_terrain_dirty_cells, radar,
                "{} hit{} radar",
                case["name"], hit["input"]
            );
            for change in hit["changes"].as_array().unwrap() {
                let coord: (u16, u16) =
                    serde_json::from_value(change["after"]["coord"].clone()).unwrap();
                let terrain = sim
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(coord.0, coord.1)
                    .unwrap();
                assert_eq!(
                    *sim.dynamic_terrain_cells.get(&coord).unwrap(),
                    DynamicTerrainCellState::capture(terrain)
                );
                if change["before"]["flags"].as_u64().unwrap() & 0x100 != 0
                    && change["after"]["flags"].as_u64().unwrap() & 0x100 == 0
                {
                    assert!(!terrain.has_bridge_deck && !terrain.bridge_walkable);
                    assert!(
                        !sim.path_grid()
                            .unwrap()
                            .cell(coord.0, coord.1)
                            .unwrap()
                            .bridge_walkable
                    );
                    assert!(
                        !sim.resolved_terrain
                            .as_ref()
                            .unwrap()
                            .cell(coord.0, coord.1)
                            .unwrap()
                            .bridge_facts
                            .has_structural_bridge()
                    );
                }
            }
        }
    }
}

#[test]
fn bridge_rim_middle_section_fallout_and_restored_navigation() {
    let input: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_stock_inputs.json",
    ))
    .unwrap();
    let rules = rules();
    let mut sim = stock_world(&rules, &input);
    let pristine = sim.resolved_terrain.as_ref().unwrap().clone();
    let victim = sim
        .construct_object_limbo_at_height(
            "MTNK",
            "Americans",
            111,
            142,
            0,
            0,
            &rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    // Supply the intended ground victim before the existing Reveal clamps Z.
    sim.substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .position
        .put_location(crate::sim::components::DriveCoord {
            x: 111 * 256 + 128,
            y: 142 * 256 + 128,
            z: 0,
        });
    sim.reveal(victim);
    for (index, coord) in [(112, 140), (112, 140), (112, 144), (112, 144)]
        .into_iter()
        .enumerate()
    {
        let mut damage = event(&mut sim, coord);
        damage.impact_z_leptons = 416
            + 104
                * i32::from(
                    sim.resolved_terrain
                        .as_ref()
                        .unwrap()
                        .cell(coord.0, coord.1)
                        .unwrap()
                        .level,
                );
        apply_bridge_damage_events_with_overlay_registry(
            &mut sim,
            &rules,
            &[damage],
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let alive = sim
            .substrate
            .entities
            .get(victim)
            .is_some_and(|unit| unit.health.current() > 0);
        assert_eq!(
            alive,
            index < 3,
            "only the final rim group clear reaches this ground occupant"
        );
    }
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "stock-rim", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(pristine, Default::default(), &rules);
    restored
        .restore_map_authority_after_snapshot_load(
            &rules,
            &crate::rules::overlay_types::OverlayTypeRegistry::empty(),
            None,
        )
        .unwrap();
    for y in 140..=144 {
        for x in 110..=113 {
            assert_eq!(snapshot(&sim, (x, y)), snapshot(&restored, (x, y)));
            assert!(
                !restored
                    .path_grid()
                    .unwrap()
                    .cell(x, y)
                    .unwrap()
                    .bridge_walkable
            );
            assert!(
                !restored
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(x, y)
                    .unwrap()
                    .bridge_facts
                    .has_structural_bridge()
            );
        }
    }
    assert_eq!(sim.dynamic_terrain_cells, restored.dynamic_terrain_cells);
    assert!(
        restored
            .resolved_terrain
            .as_ref()
            .unwrap()
            .high_bridge_rim_tiles()
            .is_some()
    );
}

/// Install one Event31 trigger per `(x, y, local, repeat)` on the stock rim
/// world: its Tag is a CellTag (NewINIFormat4 packs y*1000+x) and the action
/// sets `local`. The minimal map header leaves Cell storage the stock
/// world's, so CellTags resolve against its real cells.
fn install_event31_cell_tags(sim: &mut Simulation, rules: &RuleSet, tags: &[(u16, u16, u32, u8)]) {
    let mut sections: [String; 5] = Default::default();
    for (index, &(x, y, local, repeat)) in tags.iter().enumerate() {
        sections[0].push_str(&format!(
            "T{index}=Neutral,<none>,Bridge{index},0,1,1,1,0\n"
        ));
        sections[1].push_str(&format!("T{index}=1,31,0,0\n"));
        sections[2].push_str(&format!("T{index}=1,56,0,{local},0,0,0,0,A\n"));
        sections[3].push_str(&format!("G{index}={repeat},Bridge{index},T{index}\n"));
        sections[4].push_str(&format!(
            "{}=G{index}\n",
            u32::from(y) * 1000 + u32::from(x)
        ));
    }
    let mut body = String::new();
    for (name, rows) in ["Triggers", "Events", "Actions", "Tags", "CellTags"]
        .into_iter()
        .zip(sections)
    {
        body.push_str(&format!("[{name}]\n{rows}"));
    }
    let map = crate::map::map_file::MapFile::from_bytes(
        format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
             [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
             [Basic]\nNewINIFormat=4\n{body}"
        )
        .as_bytes(),
    )
    .unwrap();
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.install_authored_cell_trigger_tags(&map);
    sim.classify_map_triggers(Some(rules));
}

/// Apply one native rim hit and return the event31 Tag indices raised.
fn rim_hit_raises(sim: &mut Simulation, rules: &RuleSet, coord: (u16, u16)) -> Vec<usize> {
    let mut damage = event(sim, coord);
    damage.impact_z_leptons = 416
        + 104
            * i32::from(
                sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(coord.0, coord.1)
                    .unwrap()
                    .level,
            );
    sim.clear_tag_dispatch_trace_for_test();
    apply_bridge_damage_events_with_overlay_registry(
        sim,
        rules,
        &[damage],
        None,
        crate::sim::world::FrameEffects::default(),
    );
    sim.tag_dispatch_trace_for_test()
        .iter()
        .filter(|(_, kind)| *kind == 31)
        .map(|&(tag, _)| tag)
        .collect()
}

fn rim_case<'a>(original: &'a Value, name: &str) -> &'a Value {
    original["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap()
}

fn native_notifies(hit: &Value) -> Vec<&Value> {
    hit["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["kind"] == "notify")
        .collect()
}

#[test]
fn bridge_rim_collapse_raises_event31_on_each_tagged_span_cell() {
    // Original575EE0 raises Tag6E53A0(31) for every nonnull CellClass+3C on
    // the endpoint-exclusive span recorded by the native notify call, in
    // traversal order (row 140 before row 150); the endpoint itself and the
    // earlier non-collapsing hit raise nothing.
    let input: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_stock_inputs.json",
    ))
    .unwrap();
    let original: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_body.json",
    ))
    .unwrap();
    let case = rim_case(&original, "single_section");
    let notifies = native_notifies(&case["hits"][1]);
    assert_eq!(notifies.len(), 1);
    assert_eq!(notifies[0]["endpoints"], json!([[112, 136], [112, 152]]));
    let rules = rules();
    let mut sim = stock_world(&rules, &input);
    // One-shot Tags on the span centre (112,140), the fourth row cell
    // (110,150) and the exclusive endpoint (112,152).
    install_event31_cell_tags(
        &mut sim,
        &rules,
        &[(112, 140, 9, 0), (110, 150, 11, 0), (112, 152, 10, 0)],
    );
    let tag_at = |sim: &Simulation, cell| {
        let identity = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity(cell);
        sim.cell_trigger_tag_for_test(identity).unwrap()
    };
    let expected = [tag_at(&sim, (112, 140)), tag_at(&sim, (110, 150))];
    for (hit, raised, locals) in [(0, &[][..], &[][..]), (1, &expected[..], &[9, 11][..])] {
        let coord = serde_json::from_value(case["hits"][hit]["input"].clone()).unwrap();
        assert_eq!(rim_hit_raises(&mut sim, &rules, coord), raised, "hit{hit}");
        for local in [9, 10, 11] {
            assert_eq!(
                sim.trigger_local_is_set_for_test(local),
                locals.contains(&local),
                "hit{hit} local{local}"
            );
        }
    }
}

#[test]
fn bridge_rim_repeating_cell_tag_is_raised_once_per_native_notify() {
    // Retail Event31 Tags repeat (type 2). Native notifies the same span four
    // times in two_breaks_remove_middle's final hit; each notify raises it.
    let input: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_stock_inputs.json",
    ))
    .unwrap();
    let original: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_body.json",
    ))
    .unwrap();
    let case = rim_case(&original, "two_breaks_remove_middle");
    let rules = rules();
    let mut sim = stock_world(&rules, &input);
    install_event31_cell_tags(&mut sim, &rules, &[(112, 140, 9, 2)]);
    for (index, hit) in case["hits"].as_array().unwrap().iter().enumerate() {
        let notifies = native_notifies(hit);
        assert!(
            notifies
                .iter()
                .all(|call| call["endpoints"] == json!([[112, 136], [112, 152]]))
        );
        let coord = serde_json::from_value(hit["input"].clone()).unwrap();
        assert_eq!(
            rim_hit_raises(&mut sim, &rules, coord).len(),
            notifies.len(),
            "hit{index}"
        );
    }
}

#[path = "bridge_ifv_impact_tests.rs"]
mod ifv_impact;
