use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::overlay_grid::OverlayGrid;
use serde_json::{Value, json};

fn fixture() -> (
    Simulation,
    RuleSet,
    crate::map::overlay_types::OverlayTypeRegistry,
) {
    fixture_with_rules("")
}

fn fixture_with_rules(
    extra: &str,
) -> (
    Simulation,
    RuleSet,
    crate::map::overlay_types::OverlayTypeRegistry,
) {
    let mut text = String::from("[Clear]\nWheel=100%\n[Road]\nWheel=100%\n[OverlayTypes]\n");
    for id in 0..=238 {
        text.push_str(&format!("{id}=O{id}\n"));
    }
    for id in 0..=238 {
        text.push_str(&format!("[O{id}]\nNoUseTileLandType=no\n"));
        if id == 7 {
            text.push_str("Overrides=yes\n");
        }
    }
    let mut ini = IniFile::from_str(&text);
    ini.merge(&IniFile::from_str(extra));
    let rules = RuleSet::from_ini(&ini).unwrap();
    let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let terrain = crate::map::resolved_terrain::bridge_constructor_terrain();
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.overlay_grid = Some(OverlayGrid::new(33, 33));
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1000));
    (sim, rules, registry)
}

fn cells(sim: &Simulation) -> Value {
    let grid = sim.resolved_terrain.as_ref().unwrap();
    Value::Array((12..=20).flat_map(|y| (12..=20).filter_map(move |x| {
        let cell = grid.cell(x, y).unwrap();
        let flags = cell.bridge_facts.raw_flags;
        let overlay = cell.bridge_facts.overlay_id.map_or(-1, i32::from);
        let state = cell.bridge_facts.state_byte;
        let anchor = cell.bridge_facts.native_anchor.map(|id| grid.native_cell_coord(id));
        (flags != 0 || overlay != -1 || state != 0 || anchor.is_some()).then(||
            json!({"coord":[x,y], "flags":flags, "overlay":overlay, "state":state, "anchor":anchor}))
    })).collect())
}

fn dummy(sim: &Simulation) -> Value {
    let grid = sim.resolved_terrain.as_ref().unwrap();
    let fallback = grid.shared_cell_dummy();
    let (overlay, state) = fallback.overlay_identity_state();
    json!({"coord":grid.native_cell_coord(Cell::Dummy), "flags":fallback.raw_flags(),
        "overlay":overlay,"state":state,"anchor_is_self":fallback.native_anchor()==Some(Cell::Dummy)})
}

#[test]
fn live_bridge_recalc_publishes_retained_attributes_before_connectivity() {
    use crate::sim::pathfinding::{PathGrid, zone_map::ZoneGrid};
    let (mut sim, rules, registry) = fixture();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let path = PathGrid::from_resolved_terrain(terrain);
    let mut prior_terrain = terrain.clone();
    prior_terrain.cell_mut(16, 16).unwrap().overlay_blocks = true;
    prior_terrain.cell_mut(17, 16).unwrap().overlay_blocks = true;
    sim.terrain_costs =
        crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(&prior_terrain);
    let prior_path = std::sync::Arc::new(PathGrid::from_resolved_terrain(&prior_terrain));
    sim.path_grid = Some(prior_path.clone());
    sim.zone_grid = Some(ZoneGrid::build_with_terrain(&path, terrain, &[], 33, 33));
    let base = sim.zone_grid.as_mut().unwrap().base_topology_mut();
    base.levels.fill(200);
    base.movement_classes.fill(6);
    let ids_before = base.zone_ids.clone();
    let rows_before = base.raw_zone_ids_by_row.clone();
    let mut host = LivePublication {
        sim: &mut sim,
        rules: &rules,
        registry: Some(&registry),
        collapsed: false,
    };
    let cell = host.lookup((16, 16));
    host.recalc_cell(cell, 4).unwrap();
    let terrain_cell = sim.resolved_terrain.as_ref().unwrap().cell(16, 16).unwrap();
    assert_eq!(terrain_cell.level, 4);
    let mut expected_classes = vec![6; 33 * 33];
    let mut expected_levels = vec![200; 33 * 33];
    expected_classes[16 * 33 + 16] = terrain_cell.zone_type;
    expected_levels[16 * 33 + 16] = terrain_cell.level;
    let base = sim.zone_grid.as_mut().unwrap().base_topology_mut();
    assert_eq!(base.movement_classes, expected_classes);
    assert_eq!(base.levels, expected_levels);
    assert_eq!(base.zone_ids, ids_before);
    assert_eq!(base.raw_zone_ids_by_row, rows_before);
    let current = sim.path_grid().unwrap();
    assert_eq!(current.cell(16, 16).unwrap().ground_level, 4);
    assert!(current.is_walkable(16, 16));
    assert_eq!(current.cell(17, 16), prior_path.cell(17, 16));
    assert_eq!(prior_path.cell(16, 16).unwrap().ground_level, 6);
    for speed in crate::rules::locomotor_type::SpeedType::ALL_WITH_COSTS {
        let costs = &sim.terrain_costs[speed];
        assert_eq!(costs.cost_at(17, 16), 0, "unrelated cached cost changed");
        assert_eq!(
            costs.cost_at(16, 16),
            crate::sim::pathfinding::terrain_cost::TerrainCostGrid::from_resolved_terrain(
                sim.resolved_terrain.as_ref().unwrap(),
                *speed
            )
            .cost_at(16, 16)
        );
    }
}

#[test]
fn live_bridge_constructor_matches_original_and_drains_at_admitted_tick() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_constructor.json",
    ))
    .unwrap();
    let original_cases = corpus["cases"].as_array().unwrap();
    assert_eq!(original_cases.len(), 36);
    for original in original_cases {
        let (mut sim, rules, registry) = fixture();
        match original["kind"].as_str().unwrap() {
            "terrain" => {
                sim.production.terrain_object_cells.insert((16, 16), 919);
            }
            "slope" => {
                sim.resolved_terrain
                    .as_mut()
                    .unwrap()
                    .cell_mut(16, 16)
                    .unwrap()
                    .slope_type = 5;
            }
            "overrides" => {
                sim.overlay_grid
                    .as_mut()
                    .unwrap()
                    .place_overlay(16, 16, 7, 0);
                sim.resolved_terrain.as_mut().unwrap().cells[16 * 33 + 16]
                    .bridge_facts
                    .overlay_id = Some(7);
            }
            "success" => (),
            other => panic!("unexpected native case {other}"),
        }
        let requested = (
            original["requested"][0].as_i64().unwrap() as i16,
            original["requested"][1].as_i64().unwrap() as i16,
        );
        let mut host = LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        };
        let handle = host
            .construct_bridge_overlay(
                requested,
                original["overlay_id"].as_u64().unwrap() as u8,
                -1,
            )
            .unwrap();
        let snapshot = sim.load_objects.snapshot(handle).unwrap();
        assert_eq!(
            json!(snapshot.world),
            original["object_world"],
            "{original}"
        );
        for (key, value) in [
            ("alive", snapshot.alive),
            ("limbo", snapshot.limbo),
            ("on_map", snapshot.on_map),
            ("redraw", snapshot.redraw),
        ] {
            assert_eq!(
                u64::from(value),
                original[key].as_u64().unwrap(),
                "{key}: {original}"
            );
        }
        assert_eq!(
            json!(sim.load_objects.registry_counts()),
            original["registry_counts"]
        );
        assert_eq!(
            json!(sim.load_objects.queue_count()),
            original["queue_count"]
        );
        assert_eq!(json!(snapshot.native_id.unwrap()), original["native_id"]);
        assert_eq!(cells(&sim), original["cells"], "{original}");
        assert_eq!(dummy(&sim), original["dummy"], "{original}");
        assert_eq!(sim.native_unique_ids.as_ref().unwrap().current_raw(), 1001);
        // This is the production frame admission/drain, not a direct test-only
        // destruction call. Original corpus separately executes725C70's body.
        sim.advance_tick(&[], None, None, None, 67);
        assert_eq!(
            json!(sim.load_objects.registry_counts()),
            original["after_drain"]["registry_counts"]
        );
        assert_eq!(
            json!(sim.load_objects.queue_count()),
            original["after_drain"]["queue_count"]
        );
        assert_eq!(cells(&sim), original["after_drain"]["cells"]);
        assert_eq!(dummy(&sim), original["after_drain"]["dummy"]);
        assert_eq!(sim.native_unique_ids.as_ref().unwrap().current_raw(), 1001);
    }
}

#[test]
fn live_bridge_constructor_side_cells_restore_deck_without_overlay_sprites() {
    use crate::map::entities::EntityCategory;
    use crate::sim::world::{PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest};
    use crate::util::fixed_math::SimFixed;

    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_constructor.json",
    ))
    .unwrap();
    // Original5FC380 success controls for24/25/237/238. Execute the actual
    // constructor on the fixture's CellClass cells.
    // The field goldens pin publication; occupation below exercises the live
    // reader and is not a claim of full native Engineer traversal coverage.
    for original in &corpus["cases"].as_array().unwrap()[8..12] {
        assert_eq!(original["kind"], "success");
        let (mut sim, rules, registry) = fixture();
        LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        }
        .construct_bridge_overlay((16, 16), original["overlay_id"].as_u64().unwrap() as u8, -1)
        .unwrap();
        assert_eq!(cells(&sim), original["cells"]);
        let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain_with_bridges(
            sim.resolved_terrain.as_ref().unwrap(),
        );
        let mut id = 100;
        for row in original["cells"].as_array().unwrap() {
            if row["flags"].as_u64().unwrap() & 0x100 == 0 || row["overlay"] != -1 {
                continue;
            }
            let x = row["coord"][0].as_u64().unwrap() as u16;
            let y = row["coord"][1].as_u64().unwrap() as u16;
            let terrain = sim.resolved_terrain.as_ref().unwrap().cell(x, y).unwrap();
            assert!(terrain.bridge_facts.has_structural_bridge());
            assert_eq!(terrain.bridge_facts.overlay_id, None);
            assert!(crate::sim::bridge_state::cell_render_state(terrain.bridge_facts).is_none());
            assert!(path.cell(x, y).unwrap().bridge_walkable, "{row}");

            for (category, mask) in [
                (EntityCategory::Unit, 0x20),
                // Original5217C0 center-subcell raw[94]/[98] in
                // walk_head_occupation.json: deck8 -> deck9 (slot0).
                (EntityCategory::Infantry, 0x01),
            ] {
                if category == EntityCategory::Infantry {
                    crate::sim::world::lifecycle_tests::insert_walker(&mut sim, id);
                } else {
                    crate::sim::world::lifecycle_tests::insert_entity(&mut sim, id, category);
                }
                assert!(matches!(
                    sim.try_reveal_entity(
                        id,
                        RevealRequest {
                            position: RevealPosition {
                                exact_z_leptons: None,
                                rx: x,
                                ry: y,
                                z: 10,
                                sub_x: SimFixed::from_num(128),
                                sub_y: SimFixed::from_num(128),
                            },
                            placement: PlacementEvidence::MarkSucceeded,
                            logic_eligible: true,
                        }
                    ),
                    RevealOutcome::Revealed { .. }
                ));
                assert_eq!(
                    sim.substrate.raw_cell_occupation.ground_bits(x, y),
                    0,
                    "{row}"
                );
                assert_eq!(
                    sim.substrate.raw_cell_occupation.deck_bits(x, y),
                    mask,
                    "{row}"
                );
                // Mark(UP) clears the Unit's 0x20. The infantryman's bit
                // leaves with his Limbo: `FootClass::Limbo` (`0x004DB260`)
                // has Walk release it through Infantry vt+0xF4.
                let _ = sim.techno_limbo(id);
                assert_eq!(
                    sim.substrate.raw_cell_occupation.deck_bits(x, y),
                    0,
                    "{row}"
                );
                id += 1;
            }
        }
    }
}

#[test]
fn constructor_side_admission_matches_original_foot_receiver() {
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::infantry_entry::InfantryEntryArgs;
    use crate::util::fixed_math::SimFixed;

    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_side_admission.json",
    ))
    .unwrap();
    let rows = corpus["admission"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    for row in rows {
        let input = &row["input"];
        let (mut sim, rules, registry) = fixture_with_rules(
            "[InfantryTypes]\n0=ENGINEER\n[ENGINEER]\nEngineer=yes\nSpeedType=Foot\n",
        );
        LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        }
        .construct_bridge_overlay((16, 16), 25, -1)
        .unwrap();
        sim.playfield_bounds = Some(
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(16, 0, 0, 16, 16),
        );
        sim.playfield_size_height = Some(16);
        sim.session.binary_frame = input["frame"].as_u64().unwrap() as u32;
        // These native controls retain constructor25 topology, then supply
        // raw occupation / the candidate raw100-clear discriminator. The
        // original and Rust both execute the complete Infantry +1AC body.
        for native in input["cells"].as_array().unwrap() {
            let x = native["coord"][0].as_u64().unwrap() as u16;
            let y = native["coord"][1].as_u64().unwrap() as u16;
            let cell = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(x, y)
                .unwrap();
            let flags = native["bridge_flags"].as_u64().unwrap() as u32;
            assert_eq!(cell.bridge_facts.raw_flags & !0x100, flags & !0x100);
            assert_eq!(cell.level, native["level"].as_u64().unwrap() as u8);
            assert_eq!(
                cell.bridge_facts.overlay_id.map_or(-1, i32::from),
                native["overlay"]
            );
            assert_eq!(
                cell.bridge_facts.state_byte,
                native["state"].as_u64().unwrap() as u8
            );
            cell.bridge_facts.raw_flags = flags;
            // Original674000 reads Foot=1.0 for the Clear0/Road1 cells in
            // this corpus; the fixture catalog only recalculates its body.
            cell.speed_costs.foot = Some(100);
            sim.substrate.raw_cell_occupation.mark_ground(
                x,
                y,
                native["occupation"][0].as_u64().unwrap() as u8,
            );
            sim.substrate.raw_cell_occupation.mark_deck(
                x,
                y,
                native["occupation"][1].as_u64().unwrap() as u8,
            );
        }
        let native = &input["actor"];
        let id = native["id"].as_u64().unwrap();
        let x = native["coord"][0].as_i64().unwrap() as i32;
        let y = native["coord"][1].as_i64().unwrap() as i32;
        let z = native["coord"][2].as_i64().unwrap() as i32;
        let mut actor = GameEntity::test_default(
            id,
            "ENGINEER",
            "Americans",
            (x / 256) as u16,
            (y / 256) as u16,
        );
        actor.owner = sim.intern("Americans");
        actor.type_ref = sim.intern("ENGINEER");
        actor.category = EntityCategory::Infantry;
        actor.position.sub_x = SimFixed::from_num(x % 256);
        actor.position.sub_y = SimFixed::from_num(y % 256);
        actor.position.z = (z / 104) as u8;
        actor.position.exact_z_leptons = Some(z);
        actor.on_bridge = native["on_bridge"].as_bool().unwrap();
        actor.in_playfield = native["in_playfield"].as_bool().unwrap();
        sim.substrate.entities.insert(actor);
        sim.mission_assign_exact(
            id,
            crate::sim::mission::MissionId::from_raw(
                native["current_mission"].as_i64().unwrap() as i32
            ),
            sim.session.binary_frame,
        )
        .unwrap();
        assert_eq!(native["queued_mission"], -1);
        assert_eq!(native["speed_type"], 0);
        let query = &row["native_entry_arguments"][0];
        assert_eq!(query["previous_null"], true);
        assert_eq!(query["flag"], 1);
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((
                query["candidate"][0].as_i64().unwrap() as i16,
                query["candidate"][1].as_i64().unwrap() as i16,
            ));
        let result = sim
            .foot_can_enter(
                id,
                cell,
                InfantryEntryArgs {
                    direction: query["direction"].as_i64().unwrap() as i32,
                    height: query["height"].as_i64().unwrap() as i32,
                    previous_cell: None,
                },
                &rules,
                Some(&registry),
            )
            .unwrap();
        assert_eq!(
            u64::from(result),
            row["can_enter_class"].as_u64().unwrap(),
            "{}",
            input["name"]
        );
    }
}

#[test]
fn live_bridge_constructor_queue_respects_terminal_admission_and_other_objects() {
    let (mut sim, rules, registry) = fixture();
    for (index, requested) in [(16, 16), (17, 16)].into_iter().enumerate() {
        let mut host = LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        };
        host.construct_bridge_overlay(requested, 24, -1).unwrap();
        let id = 100 + index as u64;
        let mut object =
            crate::sim::game_entity::GameEntity::test_default(id, "E1", "Americans", 16, 16);
        object.type_ref = sim.interner.intern("E1");
        object.owner = sim.interner.intern("Americans");
        sim.substrate.entities.insert(object);
        sim.reveal(id);
        if index == 0 {
            sim.uninit(id);
        } else {
            sim.substrate.pending_delete.push(id);
        }
    }
    let retained_cells = cells(&sim);
    let retained_dummy = dummy(&sim);
    sim.quit_requested = true;
    sim.advance_tick(&[], None, None, None, 67);
    assert_eq!(sim.load_objects.queue_count(), 2);
    assert_eq!(sim.load_objects.registry_counts(), [2; 5]);
    assert!(sim.substrate.entities.get(100).is_some());
    assert!(sim.substrate.pending_delete.contains(&100));
    assert!(sim.substrate.pending_delete.contains(&101));
    sim.quit_requested = false;
    sim.advance_tick(&[], None, None, None, 67);
    assert_eq!(sim.load_objects.queue_count(), 0);
    assert_eq!(sim.load_objects.registry_counts(), [0; 5]);
    assert!(sim.substrate.entities.get(100).is_none());
    assert!(sim.substrate.entities.get(101).is_some());
    assert_eq!(sim.substrate.pending_delete, vec![101]);
    assert_eq!(cells(&sim), retained_cells);
    assert_eq!(dummy(&sim), retained_dummy);
    assert_eq!(sim.native_unique_ids.as_ref().unwrap().current_raw(), 1002);
}

#[test]
fn live_bridge_constructor_restamp_updates_the_state_byte_axis() {
    let (mut sim, rules, registry) = fixture();
    let render = |sim: &Simulation| {
        crate::sim::bridge_state::cell_render_state(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(16, 16)
                .unwrap()
                .bridge_facts,
        )
    };
    {
        let mut host = LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        };
        host.construct_bridge_overlay((16, 16), 25, -1).unwrap();
    }
    assert_eq!(render(&sim).map(|(_, axis)| axis), Some(Axis::EW));
    let mut host = LivePublication {
        sim: &mut sim,
        rules: &rules,
        registry: Some(&registry),
        collapsed: false,
    };
    host.construct_bridge_overlay((16, 16), 24, -1).unwrap();
    let selected = host.lookup((16, 16));
    assert_eq!(host.state(selected), 0);
    assert_eq!(render(&sim).map(|(_, axis)| axis), Some(Axis::NS));
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(16, 16)
            .unwrap()
            .bridge_facts
            .overlay_id,
        Some(24)
    );
}
