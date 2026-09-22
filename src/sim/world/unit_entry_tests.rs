use super::*;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::occupancy::CellListInsertion;

#[test]
fn unit_entry_preserves_original_numeric_results_and_repair_projection() {
    compare_rows(
        include_str!("../../../tools/spatial_oracle/unit_entry.json"),
        150,
        true,
    );
}

#[test]
fn unit_entry_matches_original_height_bridge_and_tube_traversal() {
    compare_rows(
        include_str!("../../../tools/spatial_oracle/unit_entry_traversal.json"),
        328,
        false,
    );
}

#[test]
fn unit_entry_matches_original_playfield_boundary_permissions() {
    compare_rows(
        include_str!("../../../tools/spatial_oracle/unit_entry_boundary.json"),
        100,
        true,
    );
}

#[test]
fn unit_entry_reads_actual_blocker_movement_state() {
    compare_rows(
        include_str!("../../../tools/spatial_oracle/unit_entry_motion.json"),
        56,
        true,
    );
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
        let (mut sim, rules, registry) = super::super::super::tests::fixture_with_rules(&extra);
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
        sim.session.game_mode_nonzero = flag("game_mode_nonzero");
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
        mover.owner = ours;
        mover.type_ref = sim.intern("MOVER");
        mover.category = EntityCategory::Unit;
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
                .create_team(ours, script_id, vec![90], None, 100);
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
                blocker.type_ref = sim.intern(name);
                blocker.owner = if node["enemy"].as_bool().unwrap_or(false) {
                    enemy
                } else {
                    ours
                };
                blocker.category = if building {
                    EntityCategory::Structure
                } else {
                    EntityCategory::Unit
                };
                blocker.crushable = node["crushable"].as_bool().unwrap_or(false);
                blocker.omni_crush_resistant = node["resistant"].as_bool().unwrap_or(false);
                blocker.foot_occupation_enabled = node["occupation"].as_bool().unwrap_or(true);
                if !building {
                    blocker.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
                }
                if let Some(state) = node.get("motion") {
                    use crate::sim::components::{
                        DriveCoord, DriveLocomotionRuntime, ShipLocomotionRuntime,
                    };
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
                    let head = coordinate(&state["head"]);
                    match state["family"].as_str().unwrap() {
                        "drive" => {
                            blocker.drive_locomotion = Some(DriveLocomotionRuntime {
                                destination: coordinate(&state["destination"]),
                                head_to: head,
                                ..Default::default()
                            })
                        }
                        "ship" => {
                            blocker.locomotor =
                                Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
                            blocker.ship_locomotion = Some(ShipLocomotionRuntime {
                                destination: coordinate(&state["destination"]),
                                head_to: head,
                                ..Default::default()
                            });
                        }
                        "walk" => {
                            blocker.category = EntityCategory::Infantry;
                            let mut loco = LocomotorState::for_test_kind(LocomotorKind::Walk);
                            let LocomotorRuntimePayload::Walk(walk) = &mut loco.runtime_payload
                            else {
                                unreachable!()
                            };
                            walk.moving = state["moving"].as_bool().unwrap();
                            walk.head = head;
                            blocker.locomotor = Some(loco);
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
                    blocker.body_facing = Some(facing);
                }
                if node["gate"].as_bool().unwrap_or(false) {
                    let opened = node["open"].as_bool().unwrap_or(false);
                    blocker.building_gate = Some(crate::sim::game_entity::BuildingGateRuntime {
                        mission_18_active: opened,
                        phase: if opened {
                            crate::sim::game_entity::BuildingGatePhase::OpenStable
                        } else {
                            crate::sim::game_entity::BuildingGatePhase::ClosedStable
                        },
                        ..Default::default()
                    });
                }
                if node["cloaked"].as_bool().unwrap_or(false) {
                    let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(100, 9);
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
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(11, 10)
                .unwrap()
                .bridge_facts
                .overlay_id = Some(0);
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
        let mut live = LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        };
        let expected = row["result"].as_u64().unwrap() as u8;
        let actual = foot_entry(&mut live, CellObjectMember::Entity(90), cell, args);
        let repair =
            repair_projection.then(|| impassable(&mut live, CellObjectMember::Entity(90), cell));
        if actual != Ok(expected) || repair.is_some_and(|answer| answer != Ok(expected == 7)) {
            mismatches.push(format!("{input}: expected {expected}, actual {actual:?}"));
        }
    }
    assert_eq!(rows.as_array().unwrap().len(), expected_count);
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
