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
#[ignore = "slow; run when changing what it covers"]
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
#[ignore = "slow; run when changing what it covers"]
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
    sim.session.set_binary_frame_for_test(100);
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
            facts: None,
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

/// The walk's continuation (`0x0073FC24..0x0073FD43`) on the native controls
/// in `cell_entry_crush_tail.json`. Each enters at `0x0073FC24` with a crush
/// latch, a running code and the selected plane's raw byte, over GROUND and
/// deck lists that vary `Crushable=`, `OmniCrushResistant=`, `OmniCrusher=`,
/// the Iron Curtain timer (none, running, just expired) and alliance either
/// way.
#[test]
fn unit_post_walk_tail_matches_original_crush_controls() {
    use crate::map::houses::HouseAllianceMap;
    use crate::sim::house_diplomacy::HouseAllianceAdmission;
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/cell_entry_crush_tail.json",
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 68);
    let mut mismatches = Vec::new();
    for case in cases {
        let input = &case["input"];
        let (mut sim, rules, registry) =
            crate::sim::world::entry_test_fixture::fixture_with_rules(&format!(
                "[VehicleTypes]\n0=MOVER\n1=VICTIM\n[MOVER]\nSpeedType=Track\nCrusher=yes\n\
                 OmniCrusher={}\n",
                if input["omni"].as_bool().unwrap() {
                    "yes"
                } else {
                    "no"
                }
            ));
        sim.session.set_binary_frame_for_test(100);
        let nodes = |layer: &str| input[layer].as_array().unwrap().clone();
        // The oracle's houses: the mover's allies the third, and a
        // `reverse_allied` target's House allies the mover's one way.
        let mut alliances =
            HouseAllianceMap::from([("AMERICANS".to_string(), ["BRITISH".to_string()].into())]);
        if nodes("ground")
            .iter()
            .chain(&nodes("deck"))
            .any(|node| node["reverse_allied"] == true)
        {
            alliances.insert("RUSSIANS".to_string(), ["AMERICANS".to_string()].into());
        }
        sim.install_house_alliances(HouseAllianceAdmission::Admitted(alliances), &rules);
        let mut mover = GameEntity::test_default(90, "MOVER", "Americans", 10, 10);
        mover.set_owner_for_test(sim.intern("Americans"));
        mover.set_type_ref_for_test(sim.intern("MOVER"));
        mover.set_category_for_test(EntityCategory::Unit);
        sim.substrate.entities.insert(mover);
        let mut next_id = 91;
        for (key, layer) in [
            ("ground", MovementLayer::Ground),
            ("deck", MovementLayer::Bridge),
        ] {
            for node in nodes(key) {
                let id = if node["self"] == true {
                    90
                } else {
                    let id = next_id;
                    next_id += 1;
                    let (name, category) = if node["category"] == "infantry" {
                        ("ENGINEER", EntityCategory::Infantry)
                    } else {
                        ("VICTIM", EntityCategory::Unit)
                    };
                    let house = if node["allied"] == true {
                        "British"
                    } else {
                        "Russians"
                    };
                    let mut target = GameEntity::test_default(id, name, house, 11, 10);
                    target.set_owner_for_test(sim.intern(house));
                    target.set_type_ref_for_test(sim.intern(name));
                    target.set_category_for_test(category);
                    target
                        .crushable_facts
                        .set_crushable_for_test(node["crushable"] == true);
                    target
                        .mover_facts
                        .set_omni_crush_resistant_for_test(node["resistant"] == true);
                    // Timer start 90, read at frame 100.
                    let duration = match node["timer"].as_str().unwrap_or("none") {
                        "none" => 0,
                        "expired" => 10,
                        "active" => 20,
                        other => panic!("unknown timer {other}"),
                    };
                    target.invulnerability = Some(InvulnerabilityState::new(
                        crate::sim::timer::CdTimer::started(90, duration),
                        InvulnKind::IronCurtain,
                    ));
                    sim.substrate.entities.insert(target);
                    id
                };
                sim.substrate.occupancy.add(
                    11,
                    10,
                    id,
                    layer,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            }
        }
        let raw = input[if input["bits_layer"] == "ground" {
            "raw_ground"
        } else {
            "raw_deck"
        }]
        .as_u64()
        .unwrap() as u8;
        let live = EntryReadContext {
            sim: &sim,
            rules: &rules,
            registry: Some(&registry),
            facts: None,
        };
        let mover = EntryMover::new(
            &live,
            90,
            sim.substrate.entities.get(90).unwrap(),
            rules.object("MOVER").unwrap(),
        );
        let actual = unit_post_walk_tail(
            &live,
            &mover,
            (11, 10),
            UnitWalk {
                running_code: input["running_code"].as_u64().unwrap() as u8,
                crush_latch: true,
                bits: raw,
                vehicle_occupied: raw & 0x20 != 0,
                owner: None,
            },
        );
        let expected = case["result"].as_u64().unwrap() as u8;
        if actual != expected {
            mismatches.push(format!(
                "{}: expected {expected}, actual {actual}",
                input["name"]
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// An unarmed `IsTrain=` Unit answers 5 where an unarmed Unit answers 7, both
/// over an enemy infantryman in the cell's list, which the walk refuses only
/// for a non-train (`0x0073FB71..0x0073FB90`) and otherwise raises to 5
/// (`0x0073FBE3`), and over an enemy's raw infantry bit with no object, which
/// the tail answers after its Crusher test (`0x0073FC91`). No retail type sets
/// `IsTrain=`.
#[test]
fn an_unarmed_train_answers_five_where_an_unarmed_unit_answers_seven() {
    for (train, expected) in [("no", 7), ("yes", 5)] {
        for listed in [false, true] {
            let (mut sim, rules, registry) =
                crate::sim::world::entry_test_fixture::fixture_with_rules(&format!(
                    "[VehicleTypes]\n0=MOVER\n[MOVER]\nSpeedType=Track\nIsTrain={train}\n"
                ));
            sim.session.set_binary_frame_for_test(100);
            let mut mover = GameEntity::test_default(90, "MOVER", "Americans", 10, 10);
            mover.set_owner_for_test(sim.intern("Americans"));
            mover.set_type_ref_for_test(sim.intern("MOVER"));
            mover.set_category_for_test(EntityCategory::Unit);
            mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
            sim.substrate.entities.insert(mover);
            sim.mission_assign_exact(90, crate::sim::mission::MissionId::from_raw(5), 100)
                .unwrap();
            let enemy = sim.intern("Russians");
            if listed {
                let mut infantry = GameEntity::test_default(91, "ENGINEER", "Russians", 11, 10);
                infantry.set_owner_for_test(enemy);
                infantry.set_type_ref_for_test(sim.intern("ENGINEER"));
                infantry.set_category_for_test(EntityCategory::Infantry);
                sim.substrate.entities.insert(infantry);
                sim.substrate.occupancy.add(
                    11,
                    10,
                    91,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            } else {
                sim.substrate
                    .raw_cell_occupation
                    .mark_ground_infantry(11, 10, 0x04, enemy);
            }
            let cell = sim
                .resolved_terrain
                .as_ref()
                .unwrap()
                .native_cell_identity((11, 10));
            let live = EntryReadContext {
                sim: &sim,
                rules: &rules,
                registry: Some(&registry),
                facts: None,
            };
            let args = InfantryEntryArgs {
                direction: -1,
                height: -1,
                previous_cell: None,
            };
            assert_eq!(
                classify_entry(&live, CellObjectMember::Entity(90), cell, args),
                Ok(expected),
                "IsTrain={train}, listed={listed}"
            );
        }
    }
}

/// A Unit's walk meets a terrain object through the weapon it would fire at
/// it (`0x0073FBAC`, `+0x2E4`): 5 for a `Wood=` warhead, else 7. A crusher is
/// no exception, however the terrain's type spells `Crushable=`.
#[test]
fn a_crusher_meets_a_terrain_object_with_its_weapon() {
    for (mover, expected) in [
        ("Crusher=yes\n", 7),
        ("OmniCrusher=yes\n", 7),
        ("Crusher=yes\nPrimary=SAW\n", 5),
    ] {
        let (mut sim, rules, registry) =
            crate::sim::world::entry_test_fixture::fixture_with_rules(&format!(
                "[VehicleTypes]\n0=MOVER\n[MOVER]\nSpeedType=Track\n{mover}\
                 [TerrainTypes]\n0=TREE01\n[TREE01]\nCrushable=yes\n\
                 [SAW]\nDamage=10\nROF=20\nRange=5\nProjectile=SAWBULLET\nWarhead=CHOP\n\
                 [SAWBULLET]\nAG=yes\n[CHOP]\nWood=yes\n"
            ));
        sim.session.set_binary_frame_for_test(100);
        let mut unit = GameEntity::test_default(90, "MOVER", "Americans", 10, 10);
        unit.set_owner_for_test(sim.intern("Americans"));
        unit.set_type_ref_for_test(sim.intern("MOVER"));
        unit.set_category_for_test(EntityCategory::Unit);
        unit.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        sim.substrate.entities.insert(unit);
        sim.mission_assign_exact(90, crate::sim::mission::MissionId::from_raw(5), 100)
            .unwrap();
        sim.production.terrain_object_cells.insert((11, 10), 900);
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((11, 10));
        let live = EntryReadContext {
            sim: &sim,
            rules: &rules,
            registry: Some(&registry),
            facts: None,
        };
        let args = InfantryEntryArgs {
            direction: -1,
            height: -1,
            previous_cell: None,
        };
        assert_eq!(
            classify_entry(&live, CellObjectMember::Entity(90), cell, args),
            Ok(expected),
            "{mover}"
        );
    }
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
            cell.land_type = input["land"].as_u64().unwrap_or(0) as u8;
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
        sim.session.set_binary_frame_for_test(100);
        // The legacy corpus name seeds A8E7AC, not A8B238.
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(flag(
                "actual_game_mode",
            )),
        );
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
        mover.set_in_playfield(flag("in_playfield"));
        if flag("mission_only") {
            mover.mission_only.mark();
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
                blocker
                    .crushable_facts
                    .set_crushable_for_test(node["crushable"].as_bool().unwrap_or(false));
                blocker.mover_facts.set_omni_crush_resistant_for_test(
                    node["resistant"].as_bool().unwrap_or(false),
                );
                blocker.set_foot_occupation_enabled(node["occupation"].as_bool().unwrap_or(true));
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
                    blocker.position.set_sub_x_for_test(SimFixed::from_num(128));
                    blocker.position.set_sub_y_for_test(SimFixed::from_num(128));
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
                            let LocomotorRuntimePayload::Walk(walk) = loco.runtime_payload_mut()
                            else {
                                unreachable!()
                            };
                            walk.set_moving(state["moving"].as_bool().unwrap());
                            walk.set_head(head);
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
                    blocker
                        .navigation
                        .set_nav_com_for_test(Some(NavTargetRef::cell(11, 10)));
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
                .set_nav_com_for_test(Some(NavTargetRef::Object {
                    id: ids[index as usize],
                }));
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
        // Each row is asked from the world and again through a path batch's
        // kept facts, read from this row's world.
        let query = |sim: &mut Simulation| {
            let sim: &Simulation = sim;
            let kept = EntryFacts::fitted(sim);
            [None, Some(&kept)].map(|facts| {
                let live = EntryReadContext {
                    sim,
                    rules: &rules,
                    registry: Some(&registry),
                    facts,
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
            })
        };
        let answers = if flag("game_mode_nonzero") {
            sim.with_object_placement_scope(query)
        } else {
            query(&mut sim)
        };
        for ((actual, repair), reads) in answers.into_iter().zip(["world", "kept"]) {
            if actual != Ok(expected) || repair.is_some_and(|answer| answer != Ok(expected == 7)) {
                mismatches.push(format!(
                    "{input} ({reads}): expected {expected}, actual {actual:?}"
                ));
            }
        }
    }
    assert_eq!(rows.as_array().unwrap().len(), expected_count);
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

fn row_entry_input(
    branch: VehicleBuildingEntryBranch,
    candidate_x: u16,
) -> LiveVehicleBuildingEntry {
    LiveVehicleBuildingEntry {
        branch,
        checked_building_id: 100,
        candidate_building_id: Some(100),
        candidate_x,
        building_origin_x: 10,
        number_impassable_rows: 1,
        is_unit_repair: false,
        is_bunker: false,
        bunker_occupied: false,
    }
}

#[test]
fn contacted_vehicle_row_skip_opens_east_columns_but_keeps_west() {
    let contacted = VehicleBuildingEntryBranch::RadioContact {
        mover_has_contact: true,
    };
    assert_eq!(
        decide_live_vehicle_building_entry(row_entry_input(contacted, 10,)),
        BuildingOccupantEntryDecision::KeepBlocker
    );
    assert_eq!(
        decide_live_vehicle_building_entry(row_entry_input(contacted, 11,)),
        BuildingOccupantEntryDecision::SkipBlocker
    );
    assert_eq!(
        decide_live_vehicle_building_entry(row_entry_input(
            VehicleBuildingEntryBranch::RadioContact {
                mover_has_contact: false,
            },
            11,
        )),
        BuildingOccupantEntryDecision::KeepBlocker
    );
}

#[test]
fn empty_vs_occupied_bunker_uses_explicit_runtime_occupant_arg() {
    let mut empty = row_entry_input(VehicleBuildingEntryBranch::UnitRepairOrBunker, 10);
    empty.number_impassable_rows = 0;
    empty.is_bunker = true;

    assert_eq!(
        decide_live_vehicle_building_entry(empty),
        BuildingOccupantEntryDecision::SkipBlocker
    );

    let occupied = LiveVehicleBuildingEntry {
        bunker_occupied: true,
        ..empty
    };
    assert_eq!(
        decide_live_vehicle_building_entry(occupied),
        BuildingOccupantEntryDecision::KeepBlocker
    );
}

#[test]
fn row_helper_requires_same_candidate_building_and_rows_value() {
    let mut other_building = row_entry_input(VehicleBuildingEntryBranch::UnitRepairOrBunker, 11);
    other_building.is_unit_repair = true;
    other_building.candidate_building_id = Some(200);
    assert_eq!(
        decide_live_vehicle_building_entry(other_building),
        BuildingOccupantEntryDecision::KeepBlocker
    );

    assert_eq!(
        decide_live_vehicle_building_entry(LiveVehicleBuildingEntry {
            branch: VehicleBuildingEntryBranch::RadioContact {
                mover_has_contact: true
            },
            ..other_building
        }),
        BuildingOccupantEntryDecision::SkipBlocker,
        "73F5A2 consumes false458A00 as skip even for a different first Building"
    );
    let no_rows = LiveVehicleBuildingEntry {
        candidate_building_id: Some(100),
        number_impassable_rows: -1,
        ..other_building
    };
    assert_eq!(
        decide_live_vehicle_building_entry(no_rows),
        BuildingOccupantEntryDecision::KeepBlocker
    );
}
