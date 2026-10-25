//! Elevated retail targets after a trigger reveal, without a local sight source.
//! This checks the production reveal and mouse-target owners; it is not a GPU
//! or original whole-mission comparison.

use super::*;

#[test]
fn native_click_shroud_controls_match_cursor_and_both_object_pickers() {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::sim::{combat::Health, game_entity::GameEntity, world::Simulation};
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/click_shroud.json",
    ))
    .unwrap();
    let rows = corpus["controls"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for category in [EntityCategory::Structure, EntityCategory::Infantry] {
        for row in rows {
            let input = &row["input"];
            let mut sim = Simulation::new();
            let player = sim.intern("Player");
            let enemy = sim.intern("Enemy");
            let typ = sim.intern("GAPILL");
            let level = input["level"].as_u64().unwrap() as u8;
            let flags = input["flags"].as_u64().unwrap() as u32;
            let on_bridge = flags & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0;
            let mut terrain_cells = Vec::new();
            for y in 0..32 {
                for x in 0..32 {
                    let mut cell = ResolvedTerrainCell::clear_for_test(x, y);
                    cell.level = level;
                    // The native suffix supplies an already-resolved Cell.
                    // Its bridge controls therefore need a coherent upstream
                    // span and a deck-height object, rather than disabling
                    // bridge picking to force that Cell at a ground pixel.
                    if (x, y) == (20, 20) || on_bridge && y == 20 && (18..=21).contains(&x) {
                        cell.bridge_facts.raw_flags = flags;
                    }
                    terrain_cells.push(cell);
                }
            }
            sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
                32,
                32,
                terrain_cells,
            ));
            sim.fog_mut_for_test().set_width_for_test(32);
            sim.fog_mut_for_test().set_height_for_test(32);
            let mapped: Vec<_> = input["mapped_cells"]
                .as_array()
                .unwrap()
                .iter()
                .map(|cell| {
                    (
                        cell[0].as_u64().unwrap() as u16,
                        cell[1].as_u64().unwrap() as u16,
                    )
                })
                .collect();
            sim.fog_mut_for_test()
                .reveal_cells_for_owner(player, mapped);
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                1,
                20,
                20,
                if on_bridge { level + 4 } else { level },
                0,
                enemy,
                Health::new(100),
                typ,
                category,
                0,
                5,
                false,
            );
            entity.set_on_bridge(on_bridge);
            entity.lifecycle.set_object_alive_for_test(true);
            entity.lifecycle.set_in_limbo_for_test(false);
            assert!(!entity.discovery.discovered_by_current_house());
            let (sx, sy) =
                crate::render::locomotor_visual::ground_screen_position(&entity.position);
            sim.entities_mut_for_test().insert(entity);
            sim.resubmit_entity_display_for_test(1);
            // Use the same retained Display owner as the production Click arm.
            // The bounded ScreenSelection list wrongly removes projected-only
            // foreign mobiles before the actual pointer picker can see them.
            let encounter_order =
                crate::app::presentation::instances::tactical_entity_encounter_order(&sim);
            assert_eq!(encounter_order, [1]);
            let terrain = sim.resolved_terrain().unwrap();

            let expected_cell = if on_bridge {
                let native =
                    crate::test_fixture::json("tools/bridge_click_state_oracle/vectors.json");
                let upstream = native["shroud_upstream_rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|case| {
                        case["base_height"] == level
                            && case["world_input"][0].as_f64() == Some(f64::from(sx))
                            && case["world_input"][1].as_f64() == Some(f64::from(sy))
                    })
                    .unwrap();
                (
                    upstream["expected_cell"][0].as_u64().unwrap() as u16,
                    upstream["expected_cell"][1].as_u64().unwrap() as u16,
                )
            } else {
                (20, 20)
            };
            assert_eq!(
                crate::app::match_runtime::sim_tick::world_point_to_cell(sx, sy, Some(terrain)),
                expected_cell
            );
            let expected = row["picked_target"].as_bool().unwrap().then_some(1);
            let visible = crate::app::presentation::instances::cell_visibility_for_local_owner(
                Some(player),
                Some(sim.fog()),
                20,
                20,
                false,
                Some(terrain),
            );
            assert_eq!(
                visible == crate::app::presentation::instances::CellVisibilityState::Visible,
                !row["shrouded"].as_bool().unwrap(),
                "{}",
                row["name"]
            );
            assert_eq!(
                hover_target_at_point(&sim, sx, sy, "Player", false)
                    .filter(|hover| {
                        matches!(
                            hover.kind,
                            HoverTargetKind::EnemyStructure | HoverTargetKind::EnemyUnit
                        )
                    })
                    .map(|hover| hover.stable_id),
                expected,
                "{}: context attack target",
                row["name"]
            );
            assert_eq!(
                pick_entity_at_point(
                    sim.entities(),
                    &encounter_order,
                    Some(sim.fog()),
                    Some("Player"),
                    sx,
                    sy,
                    Some(sim.interner()),
                    Some(terrain),
                ),
                expected,
                "{}: direct selection target {category:?}",
                row["name"]
            );
            for additive in [false, true] {
                let mutation = compute_click_selection_snapshot_with_playfield(
                    sim.entities(),
                    &encounter_order,
                    &[],
                    Some(sim.fog()),
                    Some("Player"),
                    sx,
                    sy,
                    additive,
                    None,
                    Some(sim.interner()),
                    false,
                    Some(terrain),
                );
                assert_eq!(
                    mutation.map(|change| change.select),
                    expected.map(|id| vec![id]),
                    "{}: production selection mutation {category:?}, shift={additive}",
                    row["name"]
                );
            }
        }
    }
}

#[test]
#[ignore = "requires retail SOV01 assets"]
fn sov01_trigger_revealed_hill_bunkers_can_be_targeted_without_tank_sight() {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let mut scene = crate::headless_scenario::load(&retail, "SOV01UMD.MAP", 339173046)
        .expect("retail SOV01 scenario");
    let runtime = &mut scene.runtime;
    let sim = &mut runtime.simulation;
    let rules = &runtime.resources.rules;
    // The one-player headless loader keeps Neutral objects, but omits campaign
    // country houses. Install the six authored bunker placements as foreign
    // Neutral targets; ownership is a controlled input, not campaign parity.
    let bunkers: Vec<_> = scene
        .map
        .entities
        .iter()
        .filter(|e| e.type_id == "GAPILL")
        .collect();
    assert_eq!(bunkers.len(), 6);
    let mut targets = Vec::new();
    for bunker in bunkers {
        targets.push(
            sim.spawn_object(
                "GAPILL",
                "Neutral",
                bunker.cell_x,
                bunker.cell_y,
                bunker.facing,
                rules,
            )
            .expect("authored bunker cell"),
        );
    }
    let viewer = sim.session().current_house().unwrap();
    let viewer_name = sim.resolve(viewer).to_owned();
    // SP4.4b_Reveal Allied, 0885994C: the actual eight Action17 waypoints.
    for waypoint in [63, 66, 71, 99, 100, 114, 437, 647] {
        assert!(sim.trigger_reveal_at_waypoint(
            waypoint,
            crate::sim::world::UninitContext::with_rules(rules),
        ));
    }
    let terrain = sim.resolved_terrain().unwrap();

    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    let mut mismatches = Vec::new();
    let mut checked = 0;
    for id in targets {
        let target = sim.entities().get(id).unwrap();
        let point = crate::sim::movement::ground_pose::position_world_coord(&target.position);
        let shrouded = crate::sim::vision::point_is_shrouded(sim.fog(), &cells, viewer, point);
        let (sx, sy) = crate::render::locomotor_visual::ground_screen_position(&target.position);
        let hover = hover_target_at_point(&*sim, sx, sy, &viewer_name, false);
        println!(
            "target={} cell=({}, {}) z={} raw_mapped={} native_shrouded={shrouded} hover={hover:?}",
            target.stable_id(),
            target.position.rx(),
            target.position.ry(),
            point.z,
            sim.fog()
                .is_cell_revealed(viewer, target.position.rx(), target.position.ry())
        );
        if !shrouded {
            checked += 1;
            if hover
                != Some(HoverTargetKindWithId {
                    kind: HoverTargetKind::EnemyStructure,
                    stable_id: target.stable_id(),
                })
            {
                mismatches.push(target.stable_id());
            }
        }
    }
    assert!(checked > 0, "retail trigger must reveal a bunker");
    assert!(
        mismatches.is_empty(),
        "revealed targets rejected by input: {mismatches:?}"
    );
}
