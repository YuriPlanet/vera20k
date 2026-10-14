use super::*;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::sim::occupancy::CellListInsertion;
use crate::util::fixed_math::SimFixed;

#[test]
fn unit_entry_preserves_original_numeric_results_and_repair_projection() {
    compare_rows(
        crate::test_fixture::text("tools/spatial_oracle/unit_entry.json"),
        150,
        true,
    );
}

#[test]
fn unit_entry_matches_original_height_bridge_and_tube_traversal() {
    compare_rows(
        crate::test_fixture::text("tools/spatial_oracle/unit_entry_traversal.json"),
        328,
        false,
    );
}

#[test]
fn unit_entry_matches_original_playfield_boundary_permissions() {
    compare_rows(
        crate::test_fixture::text("tools/spatial_oracle/unit_entry_boundary.json"),
        100,
        true,
    );
}

#[test]
fn unit_entry_reads_actual_blocker_movement_state() {
    compare_rows(
        crate::test_fixture::text("tools/spatial_oracle/unit_entry_motion.json"),
        56,
        true,
    );
}

#[test]
fn unit_entry_reads_fly_pitch_and_jumpjet_request_independently_of_phase() {
    compare_rows(
        crate::test_fixture::text("tools/spatial_oracle/unit_entry_air_motion.json"),
        288,
        true,
    );
}

/// A closed gate's House asks about the mover (`IsAlliedWith @ 0x004F9A50`,
/// `0x0073F6F5..0x0073F702`). A gate whose House allies the mover one way
/// makes it wait (3, as for an own gate); the mover's one-way alliance alone
/// leaves the gate hostile, which refuses an unarmed mover (7). The codes are
/// the oracle's own-gate and enemy-gate rows in `unit_entry.json`.
#[test]
fn a_closed_gate_asks_its_own_house_about_the_mover() {
    use crate::map::houses::HouseAllianceMap;
    use crate::sim::house_diplomacy::HouseAllianceAdmission;

    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[VehicleTypes]\n0=MOVER\n[BuildingTypes]\n1=GATE\n\
         [MOVER]\nSpeedType=Track\n[GATE]\nFoundation=1x1\nGate=yes\n",
    );
    sim.session.binary_frame = 100;
    let mut mover = GameEntity::test_default(90, "MOVER", "Americans", 10, 10);
    mover.set_owner_for_test(sim.intern("Americans"));
    mover.set_type_ref_for_test(sim.intern("MOVER"));
    mover.set_category_for_test(EntityCategory::Unit);
    mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    sim.substrate.entities.insert(mover);
    sim.mission_assign_exact(90, crate::sim::mission::MissionId::from_raw(5), 100)
        .unwrap();
    let mut gate = GameEntity::test_default(91, "GATE", "Russians", 11, 10);
    gate.set_owner_for_test(sim.intern("Russians"));
    gate.set_type_ref_for_test(sim.intern("GATE"));
    gate.set_category_for_test(EntityCategory::Structure);
    gate.building_gate = Some(Default::default());
    sim.substrate.entities.insert(gate);
    sim.substrate.occupancy.add(
        11,
        10,
        91,
        MovementLayer::Ground,
        None,
        CellListInsertion::AppendBuilding,
    );
    let cell = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_identity((11, 10));
    let classify = |sim: &Simulation| {
        let live = EntryReadContext {
            sim,
            rules: &rules,
            registry: Some(&registry),
        };
        let args = InfantryEntryArgs {
            direction: -1,
            height: -1,
            previous_cell: None,
        };
        classify_entry(&live, CellObjectMember::Entity(90), cell, args)
    };
    let one_way = |asker: &str, other: &str| {
        HouseAllianceAdmission::Admitted(HouseAllianceMap::from([(
            asker.to_ascii_uppercase(),
            [other.to_ascii_uppercase()].into(),
        )]))
    };
    sim.install_house_alliances(one_way("Russians", "Americans"), &rules);
    assert_eq!(classify(&sim), Ok(3));
    sim.install_house_alliances(one_way("Americans", "Russians"), &rules);
    assert_eq!(classify(&sim), Ok(7));
}

fn compare_rows(json: &str, expected_count: usize, repair_projection: bool) {
    let rows: serde_json::Value = serde_json::from_str(json).unwrap();
    let mut mismatches = Vec::new();
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap_or(false);
        let restriction = input["restricted_land"]
            .as_u64()
            .map(|land| {
                format!(
                    "MovementRestrictedTo={}\n",
                    crate::rules::terrain_rules::LandType::from_index(land as u8)
                        .unwrap()
                        .section_name()
                )
            })
            .unwrap_or_default();
        let mut extra = format!(
            "[VehicleTypes]\n0=MOVER\n1=BLOCKER\n[BuildingTypes]\n1=BUILDING\n[MOVER]\nSpeedType=Track\nCrusher={}\nOmniCrusher={}\nMovementZone={}\n{restriction}{}[BLOCKER]\nSpeedType=Track\n[BUILDING]\nFoundation=1x1\nGate={}\n[TESTGUN]\nDamage=10\nROF=20\nRange=5\nProjectile=TESTPROJECTILE\nWarhead=SA\n[TESTPROJECTILE]\nAG={}\n[SA]\nWall={}\n",
            flag("crusher"),
            flag("omni"),
            if flag("crusher_all") {
                "CrusherAll"
            } else {
                "Normal"
            },
            if flag("armed") {
                "Primary=TESTGUN\n"
            } else {
                ""
            },
            input["objects"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|node| node["gate"].as_bool().unwrap_or(false)),
            input["ag"].as_bool().unwrap_or(true),
            flag("wall_weapon"),
        );
        if input.get("wall").is_some() {
            extra.push_str(&format!("[O0]\nWall=yes\nCrushable={}\n", flag("wall")));
        }
        let (mut sim, rules, registry) =
            crate::sim::world::entry_test_fixture::fixture_with_rules(&extra);
        let coord = |value: &serde_json::Value| {
            (
                value[0].as_u64().unwrap() as u16,
                value[1].as_u64().unwrap() as u16,
            )
        };
        if let Some(tubes) = input["tubes"].as_array() {
            use crate::map::tube_facts::{TubeFact, TubeId, TubeSource};
            let old = sim.resolved_terrain.take().unwrap();
            let mut cells = old.cells().to_vec();
            let tubes = tubes
                .iter()
                .enumerate()
                .map(|(index, tube)| {
                    let at = coord(&tube["cell"]);
                    cells
                        .iter_mut()
                        .find(|cell| (cell.rx, cell.ry) == at)
                        .unwrap()
                        .tube_index = Some(TubeId(index as u16));
                    TubeFact {
                        entry: tube.get("entry").map_or(at, coord),
                        exit: tube.get("exit").map_or((20, 20), coord),
                        direction: tube["direction"].as_i64().unwrap() as i32,
                        path_steps: Vec::new(),
                        source: TubeSource::ExplicitMap,
                    }
                })
                .collect();
            sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells_with_tubes(
                33, 33, cells, tubes,
            ));
        }
        for input_cell in input["cells"].as_array().into_iter().flatten() {
            let (x, y) = coord(input_cell);
            let cell = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(x, y)
                .unwrap();
            cell.level = input_cell[2].as_u64().unwrap() as u8;
            cell.bridge_facts.raw_flags = input_cell[3].as_u64().unwrap() as u32;
        }
        for slope in input["slopes"].as_array().into_iter().flatten() {
            let (x, y) = coord(slope);
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(x, y)
                .unwrap()
                .slope_type = slope[2].as_u64().unwrap() as u8;
        }
        if input.get("restricted_land").is_some() {
            let cell = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(11, 10)
                .unwrap();
            cell.yr_cell_land_type = input["land"].as_u64().unwrap_or(0) as u8;
            cell.final_tile_index = 0;
            cell.final_sub_tile = input["subtile"].as_u64().unwrap_or(0) as u8;
        }
        if let Some(overlay) = input["overlay"].as_u64() {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(11, 10)
                .unwrap()
                .bridge_facts
                .overlay_id = Some(overlay as u8);
        }
        sim.session.binary_frame = 100;
        // The legacy corpus name seeds A8E7AC, not A8B238.
        sim.session.game_mode_nonzero = flag("actual_game_mode");
        if let Some(bounds) = input["bounds"].as_array() {
            sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
                base: bounds[0].as_i64().unwrap() as i32,
                off_fc: bounds[1].as_i64().unwrap() as i32,
                off_100: bounds[2].as_i64().unwrap() as i32,
                off_104: bounds[3].as_i64().unwrap() as i32,
                off_108: bounds[4].as_i64().unwrap() as i32,
            });
        }
        let ours = sim.intern("Americans");
        let enemy = sim.intern("Russians");
        let mut mover = GameEntity::test_default(90, "MOVER", "Americans", 10, 10);
        mover.set_owner_for_test(ours);
        mover.set_type_ref_for_test(sim.intern("MOVER"));
        mover.set_category_for_test(EntityCategory::Unit);
        mover.in_playfield = flag("in_playfield");
        if flag("mission_only") {
            mover.mark_mission_only();
        }
        mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        sim.substrate.entities.insert(mover);
        sim.mission_assign_exact(
            90,
            crate::sim::mission::MissionId::from_raw(input["mission"].as_i64().unwrap_or(5) as i32),
            100,
        )
        .unwrap();
        if let Some(queued) = input["queued"].as_i64() {
            use crate::sim::mission::{MissionDispatchTimer, MissionId, state::MissionTestFixture};
            sim.substrate
                .entities
                .get_mut(90)
                .unwrap()
                .mission
                .apply_test_fixture(MissionTestFixture {
                    current: MissionId::from_raw(input["mission"].as_i64().unwrap_or(5) as i32),
                    queued: MissionId::from_raw(queued as i32),
                    suspended: MissionId::NONE,
                    movement_bypass_latch: 0,
                    handler_state: 0,
                    mission_start_frame: 100,
                    ai_counter: 0,
                    dispatch_timer: MissionDispatchTimer::at_frame(100),
                });
        }
        if let Some(team) = input.get("team") {
            let script_id = sim.intern("ENTRY_TEAM_SCRIPT");
            sim.team_script_vm
                .register_script(crate::sim::team_script_vm::TeamScriptDefinition {
                    id: script_id,
                    actions: vec![crate::sim::team_script_vm::TeamScriptAction {
                        action_id: team["action"].as_i64().unwrap() as i32,
                        argument: 4,
                    }],
                    source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                });
            let id = sim
                .team_script_vm
                .create_team(ours, script_id, vec![90], 100);
            // Supplied retained Script cursor, independent of the unported
            // Team activation lifecycle. Non-action3 is false for either7F.
            let mut state = serde_json::to_value(&sim.team_script_vm).unwrap();
            state["teams"][id.to_string()]["cursor"] = team["cursor"].clone();
            sim.team_script_vm = serde_json::from_value(state).unwrap();
        }
        let mut ids = Vec::new();
        for (index, node) in input["objects"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let id = if node["self"].as_bool().unwrap_or(false) {
                90
            } else {
                let id = 91 + index as u64;
                let building = node["building"].as_bool().unwrap_or(false);
                let name = if building { "BUILDING" } else { "BLOCKER" };
                let mut blocker = GameEntity::test_default(id, name, "Americans", 11, 10);
                blocker.set_type_ref_for_test(sim.intern(name));
                blocker.set_owner_for_test(if node["enemy"].as_bool().unwrap_or(false) {
                    enemy
                } else {
                    ours
                });
                blocker.set_category_for_test(if building {
                    EntityCategory::Structure
                } else {
                    EntityCategory::Unit
                });
                blocker.crushable = node["crushable"].as_bool().unwrap_or(false);
                blocker.omni_crush_resistant = node["resistant"].as_bool().unwrap_or(false);
                blocker.foot_occupation_enabled = node["occupation"].as_bool().unwrap_or(true);
                if !building {
                    blocker.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
                }
                if let Some(state) = node.get("motion") {
                    use crate::sim::components::DriveCoord;
                    use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
                    let coordinate = |v: &serde_json::Value| {
                        let c = DriveCoord {
                            x: v[0].as_i64().unwrap() as i32,
                            y: v[1].as_i64().unwrap() as i32,
                            z: v[2].as_i64().unwrap() as i32,
                        };
                        (c.x != 0 || c.y != 0 || c.z != 0).then_some(c)
                    };
                    blocker.position.sub_x = SimFixed::from_num(128);
                    blocker.position.sub_y = SimFixed::from_num(128);
                    let head = state.get("head").and_then(coordinate);
                    match state["family"].as_str().unwrap() {
                        "drive" => {
                            assert!(
                                blocker
                                    .locomotor
                                    .as_mut()
                                    .unwrap()
                                    .install_drive_state_for_test(Some(
                                        DriveLocomotionRuntime::default()
                                            .with_destination_for_test(coordinate(
                                                &state["destination"]
                                            ))
                                            .with_head_to_for_test(head)
                                    ))
                            )
                        }
                        "ship" => {
                            blocker.locomotor =
                                Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
                            assert!(
                                blocker
                                    .locomotor
                                    .as_mut()
                                    .unwrap()
                                    .install_ship_state_for_test(Some(
                                        ShipLocomotionRuntime::default()
                                            .with_destination_for_test(coordinate(
                                                &state["destination"]
                                            ))
                                            .with_head_to_for_test(head)
                                    ))
                            );
                        }
                        "walk" => {
                            blocker.set_category_for_test(EntityCategory::Infantry);
                            let mut loco = LocomotorState::for_test_kind(LocomotorKind::Walk);
                            let LocomotorRuntimePayload::Walk(walk) = &mut loco.runtime_payload
                            else {
                                unreachable!()
                            };
                            walk.moving = state["moving"].as_bool().unwrap();
                            walk.head = head;
                            blocker.locomotor = Some(loco);
                        }
                        "fly" | "jumpjet" => {
                            if state["family"] == "fly" {
                                blocker.set_category_for_test(EntityCategory::Aircraft);
                            } else if node["infantry"] == true {
                                blocker.set_category_for_test(EntityCategory::Infantry);
                            }
                            crate::sim::movement::motion_query::tests::apply_air_state(
                                &mut blocker,
                                state,
                            );
                        }
                        _ => unreachable!(),
                    }
                }
                if node["nav"].as_bool().unwrap_or(false) {
                    blocker.navigation.nav_com = Some(NavTargetRef::cell(11, 10));
                }
                if node["turn"].as_bool().unwrap_or(false) {
                    let mut facing = crate::sim::movement::FacingClass::new(0, 5);
                    facing.set(0x4000, 100);
                    blocker.body_facing = facing;
                }
                if node["gate"].as_bool().unwrap_or(false) {
                    let opened = node["open"].as_bool().unwrap_or(false);
                    blocker.building_gate = Some(Default::default());
                    if opened {
                        crate::sim::mission::authority::queue_entity_mission_deferred(
                            &mut blocker,
                            crate::sim::mission::MissionId::from_known(
                                crate::sim::mission::MissionType::Open,
                            ),
                        );
                        crate::sim::mission::authority::commence_entity_mission(&mut blocker, 100);
                        blocker.open_door(0, 100);
                        blocker.advance_door(100);
                    }
                }
                if node["cloaked"].as_bool().unwrap_or(false) {
                    let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(100);
                    cloak.establish_unlimbo_fully_cloaked();
                    blocker.cloak = Some(cloak);
                }
                sim.substrate.entities.insert(blocker);
                id
            };
            ids.push(id);
        }
        if let Some(index) = input["nav_target"].as_u64() {
            sim.substrate
                .entities
                .get_mut(90)
                .unwrap()
                .navigation
                .nav_com = Some(NavTargetRef::Entity {
                id: ids[index as usize],
            });
        }
        for id in ids {
            sim.substrate.occupancy.add(
                11,
                10,
                id,
                if flag("deck") {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                },
                None,
                // Supplied native list order, independent of Unlimbo's
                // prepend/append lifecycle producers (outside this corpus).
                CellListInsertion::AppendBuilding,
            );
        }
        if flag("deck") {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(11, 10)
                .unwrap()
                .bridge_facts
                .raw_flags = 0x100;
        }
        if input.get("wall").is_some() {
            sim.overlay_grid
                .as_mut()
                .unwrap()
                .place_overlay(11, 10, 0, 0);
            if let Some(owner) = match input["wall_owner"].as_i64().unwrap_or(-1) {
                0 => Some(ours),
                1 => Some(enemy),
                _ => None,
            } {
                sim.overlay_grid
                    .as_mut()
                    .unwrap()
                    .set_wall_owner(11, 10, owner);
            }
        }
        for (bits_key, owner_key, layer) in [
            ("bits", "owner", MovementLayer::Ground),
            ("deck_bits", "deck_owner", MovementLayer::Bridge),
        ] {
            let bits = input[bits_key].as_u64().unwrap_or(0) as u8;
            let owner = match input[owner_key].as_i64().unwrap_or(-1) {
                -1 => None,
                0 => Some(ours),
                1 => Some(enemy),
                _ => unreachable!(),
            };
            let raw = &mut sim.substrate.raw_cell_occupation;
            match (layer, owner) {
                (MovementLayer::Ground, None) => raw.mark_ground(11, 10, bits),
                (MovementLayer::Ground, Some(owner)) => {
                    raw.mark_ground_infantry(11, 10, bits, owner)
                }
                (MovementLayer::Bridge, None) => raw.mark_deck(11, 10, bits),
                (MovementLayer::Bridge, Some(owner)) => raw.mark_deck_infantry(11, 10, bits, owner),
                _ => unreachable!(),
            }
        }
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((11, 10));
        let args = InfantryEntryArgs {
            direction: input["direction"].as_i64().unwrap_or(-1) as i32,
            height: input["height"].as_i64().unwrap_or(-1) as i32,
            previous_cell: input.get("previous").map(|p| {
                let (x, y) = coord(p);
                sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .native_cell_identity((x as i16, y as i16))
            }),
        };
        let expected = row["result"].as_u64().unwrap() as u8;
        let query = |sim: &mut Simulation| {
            let live = EntryReadContext {
                sim,
                rules: &rules,
                registry: Some(&registry),
            };
            let actual = classify_entry(&live, CellObjectMember::Entity(90), cell, args);
            let repair = repair_projection.then(|| {
                classify_entry(
                    &live,
                    CellObjectMember::Entity(90),
                    cell,
                    crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
                )
                .map(|code| code == 7)
            });
            (actual, repair)
        };
        let (actual, repair) = if flag("game_mode_nonzero") {
            sim.with_object_placement_scope(query)
        } else {
            query(&mut sim)
        };
        if actual != Ok(expected) || repair.is_some_and(|answer| answer != Ok(expected == 7)) {
            mismatches.push(format!("{input}: expected {expected}, actual {actual:?}"));
        }
    }
    assert_eq!(rows.as_array().unwrap().len(), expected_count);
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
