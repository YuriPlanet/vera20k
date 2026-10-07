use super::*;
use crate::map::resolved_terrain::ResolvedTerrainCell;
use crate::rules::ini_parser::IniFile;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::rng::SimRng;

fn constructor_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n1=SLAV\n\n\
         [VehicleTypes]\n0=MTNK\n1=CARRIER\n2=SMIN\n\n\
         [AircraftTypes]\n0=ORCA\n1=HORN\n\n\
         [BuildingTypes]\n0=BASE\n1=UP1\n2=UP2\n3=YAREFN\n\n\
         [E1]\nStrength=100\nSpeed=4\n\n\
         [SLAV]\nStrength=125\nSpeed=4\nStorage=4\n\n\
         [MTNK]\nStrength=300\nSpeed=6\n\n\
         [CARRIER]\nStrength=800\nSpeed=4\nSpawns=HORN\nSpawnsNumber=3\nSpawnRegenRate=600\nSpawnReloadRate=25\n\n\
         [SMIN]\nStrength=2000\nSpeed=3\nEnslaves=SLAV\nSlavesNumber=2\nSlaveRegenRate=500\nSlaveReloadRate=25\n\n\
         [ORCA]\nStrength=200\nSpeed=8\n\n\
         [HORN]\nStrength=75\nSpeed=14\nAmmo=1\n\n\
         [BASE]\nStrength=500\nFoundation=2x2\n\n\
         [UP1]\nStrength=100\nFoundation=1x1\n\n\
         [UP2]\nStrength=100\nFoundation=1x1\n\n\
         [YAREFN]\nStrength=2000\nFoundation=2x2\nEnslaves=SLAV\nSlavesNumber=2\nSlaveRegenRate=500\nSlaveReloadRate=25\n",
    ))
    .expect("constructor fixture rules parse")
}

/// Eight original51DFF0 floor/Place controls, including full Scenario buffers.
/// The inherited native harness stops full lifecycle at Foot4D7170; these rows
/// therefore compare this shared class placement owner, not complete Reveal.
#[test]
fn infantry_unlimbo_placement_matches_original_coordinate_and_rng_controls() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "src/sim/world/world_spawn/fixtures/infantry_unlimbo_placement_native.json",
    ))
    .unwrap();
    for row in corpus["rows"].as_array().unwrap() {
        let (mut sim, rules, _) = crate::sim::world::entry_test_fixture::fixture();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_class_before"].as_str().unwrap());
        let input = &row["input"];
        let raw_z = input["raw_z"].as_i64().unwrap() as i32;
        let bits = input["bits"].as_u64().unwrap() as u8;
        if bits != 0 {
            sim.substrate.raw_cell_occupation.mark_ground(15, 15, bits);
        }
        let native_requested = &row["class_requested_coordinate"];
        let x = native_requested[0].as_i64().unwrap() as i32;
        let y = native_requested[1].as_i64().unwrap() as i32;
        let requested = RevealPosition {
            // The104 control proves the coarse input's raw-Z comparison too.
            exact_z_leptons: (raw_z != 104).then_some(raw_z),
            rx: (x / 256) as u16,
            ry: (y / 256) as u16,
            z: raw_z.div_euclid(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS) as u8,
            sub_x: crate::util::fixed_math::SimFixed::from_num(x % 256),
            sub_y: crate::util::fixed_math::SimFixed::from_num(y % 256),
        };
        let result = if input["priority"].as_u64() == Some(0) {
            sim.infantry_unlimbo_position(requested, Some(&rules))
        } else {
            sim.with_object_placement_scope(|sim| {
                sim.infantry_unlimbo_position(requested, Some(&rules))
            })
        };
        let foot = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event[0].as_str() == Some("foot_unlimbo"));
        assert_eq!(
            result.is_some(),
            row["result"].as_u64() == Some(1),
            "{input}"
        );
        if let (Some(position), Some(foot)) = (result, foot) {
            assert_eq!(
                serde_json::json!([
                    i32::from(position.rx) * 256 + position.sub_x.to_num::<i32>(),
                    i32::from(position.ry) * 256 + position.sub_y.to_num::<i32>(),
                    raw_z,
                ]),
                foot[2],
                "{input}"
            );
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "{input}"
        );
        assert!(
            !sim.object_placement_scope_active(),
            "{input}: priority restored"
        );
    }
}

/// Original51DFF0/481180/578460/4525F0 frontend controls. The native fixture
/// supplies an admitted ground building and stable Door/Mission state; Foot
/// success is an observation seam. Compare its incoming FootXYZ and three RNG
/// buffers, not the later class Mark or complete Gate/Infantry lifecycle.
#[test]
fn infantry_unlimbo_matches_native_gate_incoming_membership_and_signed_xy() {
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};
    use crate::sim::movement::ground_pose;
    use crate::sim::occupancy::CellListInsertion;

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "src/sim/world/world_spawn/fixtures/infantry_unlimbo_gate_native.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(
        corpus["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let mut compared = 0;
    for row in corpus["rows"].as_array().unwrap() {
        // The retained -123264 control exceeds the existing Position owner's
        // documented -32768 lower bound. It remains native-only range evidence.
        if row["representable_frontend_input"] == false {
            assert!(row["requested_xyz"][0].as_i64().unwrap() < -32768);
            continue;
        }
        let name = row["name"].as_str().unwrap();
        let input = &row["input"];
        let cell = &row["cell_before"];
        let cx = cell["coordinate"][0].as_u64().unwrap() as u16;
        let cy = cell["coordinate"][1].as_u64().unwrap() as u16;
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[BuildingTypes]\n0=GAGATE_A\n[GAGATE_A]\nGate={}\nStrength=1000\nFoundation=1x1\n",
            if row["gate_prior"]["gate_byte"] == 1 {
                "yes"
            } else {
                "no"
            }
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap());
        let mut terrain = ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| {
                        crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
                    })
                })
                .collect(),
        );
        let selected_cell = terrain.cell_mut(cx, cy).unwrap();
        selected_cell.level = cell["level_slope"][0].as_u64().unwrap() as u8;
        selected_cell.slope_type = cell["level_slope"][1].as_u64().unwrap() as u8;
        sim.install_resolved_terrain_for_new_map(terrain);
        let fields = row["map_fields"].as_array().unwrap();
        sim.playfield_bounds = Some(
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
                fields[0].as_i64().unwrap() as i32,
                fields[1].as_i64().unwrap() as i32,
                fields[2].as_i64().unwrap() as i32,
                fields[3].as_i64().unwrap() as i32,
                fields[4].as_i64().unwrap() as i32,
            ),
        );
        sim.substrate.raw_cell_occupation.mark_ground(
            cx,
            cy,
            cell["ground_bits"].as_u64().unwrap() as u8,
        );
        if input["gate_present"] == true {
            let owner = sim.interner.intern("Americans");
            let ty = sim.interner.intern("GAGATE_A");
            let mut gate = GameEntity::new_at_frame_for_test(
                1,
                cx,
                cy,
                0,
                0,
                owner,
                Health { current: 1000 },
                ty,
                EntityCategory::Structure,
                0,
                0,
                false,
                0,
            );
            if row["gate_prior"]["door_state"] == serde_json::json!([0, 1]) {
                gate.open_door(0, 0);
                gate.advance_door(0);
            }
            gate.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(
                    row["gate_prior"]["current_mission"].as_i64().unwrap() as i32,
                ),
                suspended: MissionId::NONE,
                queued: MissionId::from_raw(
                    row["gate_prior"]["queued_mission"].as_i64().unwrap() as i32
                ),
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::from_raw(0, 0),
            });
            gate.lifecycle.in_limbo = false;
            gate.lifecycle.cell_marked = true;
            gate.in_playfield = true;
            sim.substrate.entities.insert(gate);
            sim.substrate.occupancy.add(
                cx,
                cy,
                1,
                MovementLayer::Ground,
                None,
                CellListInsertion::AppendBuilding,
            );
        }
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap());
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        let xyz = &row["requested_xyz"];
        let mut requested = RevealPosition {
            exact_z_leptons: Some(xyz[2].as_i64().unwrap() as i32),
            rx: 0,
            ry: 0,
            z: 0,
            sub_x: SimFixed::ZERO,
            sub_y: SimFixed::ZERO,
        };
        ground_pose::set_position_world_xy(
            &mut requested,
            [
                xyz[0].as_i64().unwrap() as i32,
                xyz[1].as_i64().unwrap() as i32,
            ],
        );
        let positioned = if input["priority"] == 0 {
            sim.infantry_unlimbo_position(requested, Some(&rules))
        } else {
            sim.with_object_placement_scope(|sim| {
                sim.infantry_unlimbo_position(requested, Some(&rules))
            })
        };
        assert_eq!(positioned.is_some(), row["result"] == 1, "{name}");
        if let Some(position) = positioned {
            let coord = ground_pose::position_world_coord(&position);
            assert_eq!(
                serde_json::json!([coord.x, coord.y, coord.z]),
                row["foot_handoff"][0]["coordinate"],
                "{name}"
            );
        }
        for (stream, rng) in [
            ("main", &sim.main_rng),
            ("scenario", &sim.scenario_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert_eq!(
                rng.native_state_hex(),
                row["rng_after"][stream].as_str().unwrap(),
                "{name}: {stream}"
            );
        }
        assert!(
            !sim.object_placement_scope_active(),
            "{name}: scope restored"
        );
        compared += 1;
    }
    assert_eq!(compared, 17);
}

/// The original5247D0 controls preserve positive raw Z and clamp-73 to0.
/// Body snap must occur after successful Mark and before the idle callback.
#[test]
fn constructed_infantry_commits_raw_z_and_facing_before_idle() {
    use crate::sim::world::lifecycle::{LifecycleTestEvent, UninitContext};
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "src/sim/world/world_spawn/fixtures/infantry_unlimbo_placement_native.json",
    ))
    .unwrap();
    for row in corpus["rows"].as_array().unwrap() {
        let raw_z = row["input"]["raw_z"].as_i64().unwrap() as i32;
        let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
        let id = sim
            .construct_object_limbo_at_height("ENGINEER", "Americans", 15, 15, 0, 0, &rules)
            .unwrap();
        sim.clear_lifecycle_test_events_for_test();
        let request = RevealRequest {
            position: RevealPosition {
                exact_z_leptons: Some(raw_z),
                rx: 15,
                ry: 15,
                z: 0,
                sub_x: crate::util::lepton::CELL_CENTER_LEPTON,
                sub_y: crate::util::lepton::CELL_CENTER_LEPTON,
            },
            placement: PlacementEvidence::MarkSucceeded,
            logic_eligible: true,
        };
        let outcome = sim.try_reveal_entity_with_context(
            id,
            request,
            UninitContext::with_rules(&rules)
                .with_registry(Some(&registry))
                .with_unlimbo_facing(Some(128)),
        );
        assert!(matches!(outcome, RevealOutcome::Revealed { .. }));
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            entity.position.exact_z_leptons,
            Some(row["type_clamped_coordinate"][2].as_i64().unwrap() as i32)
        );
        assert_eq!(entity.body_facing.current(sim.session.binary_frame), 0x8000);
        let events = sim.lifecycle_test_events_for_test();
        let mark = events
            .iter()
            .position(|event| *event == LifecycleTestEvent::MarkPut)
            .unwrap();
        let facing = events
            .iter()
            .position(|event| *event == LifecycleTestEvent::UnlimboBodyFacingSnapped)
            .unwrap();
        let idle = events
            .iter()
            .position(|event| *event == LifecycleTestEvent::UnlimboIdleMode)
            .unwrap();
        assert!(mark < facing && facing < idle);

        let failed = sim
            .construct_object_limbo_at_height("ENGINEER", "Americans", 16, 15, 0, 0, &rules)
            .unwrap();
        sim.clear_lifecycle_test_events_for_test();
        let mut refused = request;
        refused.placement = PlacementEvidence::MarkFailed;
        assert!(matches!(
            sim.try_reveal_entity_with_context(
                failed,
                refused,
                UninitContext::with_rules(&rules).with_unlimbo_facing(Some(128))
            ),
            RevealOutcome::Failed(_)
        ));
        assert_eq!(
            sim.substrate
                .entities
                .get(failed)
                .unwrap()
                .body_facing
                .current(sim.session.binary_frame),
            0
        );
        assert!(
            !sim.lifecycle_test_events_for_test()
                .contains(&LifecycleTestEvent::UnlimboBodyFacingSnapped)
        );
        assert!(
            !sim.lifecycle_test_events_for_test()
                .contains(&LifecycleTestEvent::UnlimboIdleMode)
        );
    }
}

fn map_entity(type_id: &str, category: EntityCategory, cell: (u16, u16)) -> MapEntity {
    MapEntity {
        owner: "Americans".to_string(),
        type_id: type_id.to_string(),
        health: 256,
        cell_x: cell.0,
        cell_y: cell.1,
        facing: 0,
        category,
        sub_cell: 0,
        veterancy: 0,
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        structure_upgrades: [None, None, None],
        structure_ai_sellable: false,
        structure_ai_repairable: false,
    }
}

fn install_american_house(sim: &mut Simulation) {
    let owner = sim.interner.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
    );
}

/// Radio65A750 and Building43BCBD..43BCD5 execute unchanged in the retained
/// refinery packet. Both production construction routes must install those
/// actual slots before any HELLO; the component prefix consumes no RNG.
#[test]
fn building_contact_constructor_matches_original_slots_before_hello() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/refinery_dock.json",
    ))
    .unwrap();
    let rows = native["building_contact_constructor_controls"]
        .as_array()
        .unwrap();
    assert_eq!(rows.len(), 4);
    for row in rows {
        let count = row["input"]["type_number_of_docks"].as_i64().unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[BuildingTypes]\n0=BASE\n[BASE]\nStrength=500\nFoundation=2x2\nNumberOfDocks={count}\n\
             [AircraftTypes]\n0=ORCA\n[ORCA]\nStrength=200\nAmmo=6\n"
        )))
        .unwrap();
        let expected = &row["after"]["contacts"];
        let check = |entity: &GameEntity| {
            let contacts = &entity.radio_contacts;
            assert_eq!(
                serde_json::json!(contacts.capacity()),
                expected["count"],
                "NumberOfDocks={count}: constructed count"
            );
            let slots: Vec<_> = (0..contacts.capacity()).map(|i| contacts.slot(i)).collect();
            assert_eq!(
                serde_json::json!(slots),
                expected["slots"],
                "NumberOfDocks={count}: initial NULL slots"
            );
        };
        let mut sim = Simulation::with_seed(row["input"]["seed"].as_u64().unwrap());
        install_constructor_test_playfield(&mut sim);
        install_constructor_flat_terrain(&mut sim);
        install_american_house(&mut sim);
        let owner = sim.interner.get("Americans").unwrap();
        let type_id = sim.interner.intern("BASE");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            6,
            5,
            0,
            0,
            owner,
            Health { current: 500 },
            type_id,
            EntityCategory::Structure,
            0,
            8,
            false,
        );
        let rng = || {
            let view = sim.scenario_rng.logical_view();
            serde_json::json!({"disabled":view.disabled,"index_a":view.index_a,
                "index_b":view.index_b,"state":view.words})
        };
        assert_eq!(rng(), row["after_radio_constructor"]["scenario_rng"]);
        sim.install_techno_components(
            &mut entity,
            rules.object("BASE"),
            Some(&rules),
            construction::ComponentOrigin::Runtime,
        );
        check(&entity);
        let view = sim.scenario_rng.logical_view();
        assert_eq!(
            serde_json::json!({"disabled":view.disabled,"index_a":view.index_a,
                "index_b":view.index_b,"state":view.words}),
            row["after"]["scenario_rng"],
            "contact allocation leaves all 250 Scenario words unchanged"
        );
        let held = sim
            .construct_object_limbo_at_height("BASE", "Americans", 0, 0, 0, 0, &rules)
            .unwrap();
        check(sim.substrate.entities.get(held).unwrap());
        let mut authored = Simulation::with_seed(1);
        install_constructor_test_playfield(&mut authored);
        install_constructor_flat_terrain(&mut authored);
        install_american_house(&mut authored);
        assert_eq!(
            authored.spawn_from_map(
                &[map_entity("BASE", EntityCategory::Structure, (6, 5))],
                Some(&rules)
            ),
            1
        );
        check(authored.substrate.entities.values().next().unwrap());
        // Required airfield consumer: reservation publishes HELLO against
        // the already-constructed slots, rather than sizing them on demand.
        let base_id = authored
            .substrate
            .entities
            .values()
            .next()
            .unwrap()
            .stable_id();
        let pads = expected["count"].as_u64().unwrap() as u32;
        for pad in 0..pads {
            let aircraft = authored
                .construct_object_limbo_at_height("ORCA", "Americans", 0, 0, 0, 0, &rules)
                .unwrap();
            let rng = authored.scenario_rng.logical_state();
            assert_eq!(
                authored.reserve_airfield_pad(base_id, aircraft, pads),
                Some(pad)
            );
            let base = authored.substrate.entities.get(base_id).unwrap();
            assert_eq!(base.radio_contacts.capacity(), pads as usize);
            assert_eq!(base.radio_contacts.slot(pad as usize), Some(aircraft));
            assert_eq!(
                authored
                    .substrate
                    .entities
                    .get(aircraft)
                    .unwrap()
                    .radio_contacts
                    .slot(0),
                Some(base_id)
            );
            assert_eq!(authored.scenario_rng.logical_state(), rng);
        }
    }
}

#[test]
fn signed_rot_reaches_spawn_combat_turn_and_snapshot_restore() {
    use crate::sim::combat::UnitFacingUpdate;
    use crate::sim::snapshot::GameSnapshot;

    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/facing_class.json",
    ))
    .unwrap();
    for row in rows.as_array().unwrap().iter().filter(|row| {
        row["input"]["rate_constructor"] == false
            && row["input"]["start"] == 100
            && row["input"]["operations"].as_array().unwrap().len() == 9
    }) {
        let rot = row["input"]["rot"].as_i64().unwrap() as i32;
        let expected = &row["observations"][0];
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nSpeed=6\nTurret=yes\nROT={rot}\n"
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(0);
        install_constructor_test_playfield(&mut sim);
        install_constructor_flat_terrain(&mut sim);
        install_american_house(&mut sim);
        let id = sim
            .spawn_object_at_height("MTNK", "Americans", 6, 5, 64, 0, &rules)
            .unwrap();
        let barrel = sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .barrel_facing
            .unwrap();
        assert_eq!(
            serde_json::json!(barrel.rot_per_frame()),
            expected["rate"],
            "ROT={rot}"
        );

        crate::sim::world::unit_post::apply_unit_facing(
            &mut sim.substrate.entities,
            &[UnitFacingUpdate {
                entity_id: id,
                turret_destination: Some(0xC000),
                hull_destination: Some(0xC000),
                turret_destination_is_idle_return: false,
            }],
            &rules,
            &sim.interner,
            100,
        );
        let entity = sim.substrate.entities.get(id).unwrap();
        for facing in [entity.body_facing, entity.barrel_facing.unwrap()] {
            assert_eq!(
                serde_json::json!(facing.rot_per_frame()),
                expected["rate"],
                "ROT={rot}"
            );
            assert_eq!(
                serde_json::json!(facing.current(100)),
                expected["animated"],
                "ROT={rot}"
            );
            assert_eq!(
                serde_json::json!(facing.is_rotating(100)),
                expected["rotating"],
                "ROT={rot}"
            );
        }
        assert_eq!(
            serde_json::json!(entity.turret_rotation_latch),
            expected["rotating"]
        );
        // Match the load-time RNG initialization before comparing whole state.
        sim.scenario_rng = SimRng::new(0);
        let hash = sim.state_hash();
        let saved = GameSnapshot::save(&sim, 0, 0, "signed facing ROT", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), hash, "ROT={rot}");
        for _ in 0..3 {
            sim.advance_tick(&[], Some(&rules), None, None, 67);
            restored.advance_tick(&[], Some(&rules), None, None, 67);
            assert_eq!(sim.state_hash(), restored.state_hash(), "ROT={rot}");
        }
    }
}

#[test]
fn discovery_owner_entry_and_lifetime_match_original_history_blocks() {
    use crate::sim::snapshot::GameSnapshot;
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/jumpjet_entry_discovery.json",
    ))
    .unwrap();
    let foreign = native
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["input"]["foreign_constructor"] == true && row["input"]["repeat_entry"] == true
        })
        .unwrap();
    let bytes = |history: crate::sim::game_entity::TechnoDiscoveryHistory| {
        [
            u8::from(history.owned_by_current_house),
            u8::from(history.discovered_by_current_house),
            u8::from(history.discovered_by_other_house),
        ]
    };
    for sight in [8, 0] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nSpeed=4\nSight={sight}\n"
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(17);
        install_constructor_test_playfield(&mut sim);
        install_constructor_flat_terrain(&mut sim);
        install_american_house(&mut sim);
        let owner = sim.interner.get("Americans").unwrap();
        let other = sim.interner.intern("Other");
        sim.houses.insert(
            other,
            crate::sim::house_state::HouseState::new(other, 0, None, false, 0, 10),
        );
        sim.session.house_order = vec![owner, other];
        sim.session.current_house = Some(owner);
        sim.session.game_mode_nonzero = true;
        let entity = sim
            .construct_runtime_techno(
                "E1",
                "Americans",
                6,
                5,
                0,
                0,
                &rules,
                TechnoConstructorInit::FreshScenario,
            )
            .unwrap()
            .unwrap();
        let row = if sight == 0 { &native[2] } else { &native[1] };
        assert_eq!(
            serde_json::json!(bytes(entity.discovery)),
            row["output"]["constructor"]["object"]
        );
        let (id, outcome) = sim.unlimbo_after_constructor_managers(entity, Some(&rules), None);
        assert!(matches!(outcome, RevealOutcome::Revealed { .. }));
        assert!(sim.substrate.occupancy.contains_entity(6, 5, id));
        assert_eq!(
            serde_json::json!(bytes(sim.substrate.entities.get(id).unwrap().discovery)),
            row["output"]["sight"]["object"]
        );
        if sight == 0 {
            continue;
        }

        sim.object_conceal(id);
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house,
            "human Conceal retains the actual observation"
        );
        assert!(
            sim.reveal_constructed_object_at_height(
                id,
                6,
                5,
                0,
                0,
                PlacementEvidence::EvaluateMark,
                &rules
            )
            .is_some()
        );
        sim.change_owner(id, other);
        let transferred = sim.substrate.entities.get(id).unwrap().discovery;
        assert_eq!(bytes(transferred), [0, 1, 0]);
        // Snapshot deserialization intentionally executes native Scenario Seed0.
        // Compare this state-only roundtrip on that same admitted RNG cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let saved = GameSnapshot::save(&sim, 0, 0, "discovery", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            restored.substrate.entities.get(id).unwrap().discovery,
            transferred
        );
        assert_eq!(restored.state_hash(), hash);
        restored.object_conceal(id);
        assert_eq!(
            bytes(restored.substrate.entities.get(id).unwrap().discovery),
            [0, 0, 0]
        );
        assert!(
            restored
                .reveal_constructed_object_at_height(
                    id,
                    6,
                    5,
                    0,
                    0,
                    PlacementEvidence::MarkSucceeded,
                    &rules
                )
                .is_some()
        );
        assert_eq!(
            serde_json::json!(bytes(
                restored.substrate.entities.get(id).unwrap().discovery
            )),
            foreign["output"]["put"]["object"],
            "full reentry includes CellPUT47E8A0 -> Discovered6F4960(PlayerPtr); the original entry/sight boundary alone has [0,0,1]"
        );
        assert_eq!(
            serde_json::json!(restored.houses[&other].discovered_by_current_house()),
            foreign["output"]["put"]["houses"][1]["byte_1f4"],
            "first current-viewer discovery notifies the object's noncurrent owner"
        );
    }
}

#[test]
fn outside_reentry_clears_current_discovery_only_after_successful_alive_mark() {
    for (placement, alive, expected_b, expected_c) in [
        (PlacementEvidence::MarkFailed, true, true, false),
        (PlacementEvidence::MarkSucceeded, true, false, true),
        (PlacementEvidence::MarkSucceeded, false, true, true),
    ] {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nSpeed=4\nSight=8\n",
        ))
        .unwrap();
        let mut sim = Simulation::with_seed(0);
        install_constructor_test_playfield(&mut sim);
        install_constructor_flat_terrain(&mut sim);
        install_american_house(&mut sim);
        let owner = sim.interner.get("Americans").unwrap();
        let other = sim.interner.intern("OtherHuman");
        sim.houses.insert(
            other,
            crate::sim::house_state::HouseState::new(other, 0, None, true, 0, 10),
        );
        sim.session.house_order = vec![owner, other];
        sim.session.current_house = Some(owner);
        sim.session.game_mode_nonzero = true;
        let id = sim
            .spawn_object_at_height("E1", "Americans", 6, 5, 0, 0, &rules)
            .unwrap();
        assert!(sim.substrate.entities.get(id).unwrap().in_playfield);
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house
        );
        sim.change_owner(id, other);
        sim.object_conceal(id);
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house,
            "other human Conceal retains historical B"
        );
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .lifecycle
            .object_alive = alive;
        // Caller-supplied successful Mark admits an outside cell, as existing
        // release callers can. Techno6F6CB8 returns after failed Object Mark
        // before the membership writer6F6CFE, retaining the prior true byte.
        // Only successful Mark reaches that writer and the mode-one query.
        let result = sim.reveal_constructed_object_at_height(id, 5, 5, 0, 0, placement, &rules);
        assert_eq!(result.is_some(), placement != PlacementEvidence::MarkFailed);
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            entity.in_playfield,
            placement == PlacementEvidence::MarkFailed
        );
        assert!(!entity.discovery.owned_by_current_house);
        assert_eq!(entity.discovery.discovered_by_current_house, expected_b);
        assert_eq!(entity.discovery.discovered_by_other_house, expected_c);
        assert_eq!(
            sim.substrate.occupancy.contains_entity(5, 5, id),
            placement != PlacementEvidence::MarkFailed
        );
    }
}

#[test]
fn first_nonhuman_owner_entry_queues_hunt_from_ambush_but_repeat_does_not() {
    use crate::sim::mission::{MissionId, MissionType};
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=E1\n[E1]\nStrength=100\nSpeed=4\nSight=8\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    install_constructor_test_playfield(&mut sim);
    install_constructor_flat_terrain(&mut sim);
    install_american_house(&mut sim);
    let current = sim.interner.get("Americans").unwrap();
    let other = sim.interner.intern("Computer1");
    sim.houses.insert(
        other,
        crate::sim::house_state::HouseState::new(other, 0, None, false, 0, 10),
    );
    sim.session.house_order = vec![current, other];
    sim.session.current_house = Some(current);
    sim.session.game_mode_nonzero = true;
    let id = sim
        .construct_object_limbo_at_height("E1", "Computer1", 6, 5, 0, 0, &rules)
        .unwrap();
    let ambush = MissionId::from_known(MissionType::Ambush);
    let hunt = MissionId::from_known(MissionType::Hunt);
    let guard = MissionId::from_known(MissionType::Guard);
    // TechnoClass::Unlimbo runs Enter_Idle_Mode and Commence (`0x006F6E2A`)
    // before FootClass::Unlimbo's owner discovery (`0x004D722F`), so a
    // queued Ambush is already replaced by Guard when discovery looks.
    crate::sim::mission::authority::queue_entity_mission_deferred(
        sim.substrate.entities.get_mut(id).unwrap(),
        ambush,
    );
    assert!(
        sim.reveal_constructed_object_at_height(
            id,
            6,
            5,
            0,
            0,
            PlacementEvidence::MarkSucceeded,
            &rules
        )
        .is_some()
    );
    let entity = sim.substrate.entities.get(id).unwrap();
    assert!(entity.discovery.discovered_by_other_house);
    assert_eq!(entity.mission.current(), guard);
    assert_eq!(entity.mission.queued(), MissionId::NONE);
    // The discovery arm itself (`TechnoClass 0x006F4960`, reached again when
    // another house first sees the object): an Ambush queue on a computer
    // house's object turns to Hunt on the first entry only.
    for first in [true, false] {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.discovery.discovered_by_other_house = !first;
        entity
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: MissionId::NONE,
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::timer::MissionDispatchTimer::at_frame(0),
            });
        crate::sim::mission::authority::queue_entity_mission_deferred(entity, ambush);
        assert_eq!(entity.mission.effective(), ambush);
        sim.record_techno_discovery(id, other);
        let entity = sim.substrate.entities.get(id).unwrap();
        assert!(entity.discovery.discovered_by_other_house);
        assert_eq!(entity.mission.queued(), if first { hunt } else { ambush });
        assert_eq!(
            entity.mission.current(),
            MissionId::NONE,
            "Queue(false) does not commence"
        );
    }
}

#[test]
fn exact_sight_zero_survives_save_and_rules_less_reentry() {
    use crate::sim::snapshot::GameSnapshot;
    for sight in [0, -1, 65536] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nSpeed=4\nSight={sight}\n"
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(19);
        install_constructor_test_playfield(&mut sim);
        install_constructor_flat_terrain(&mut sim);
        install_american_house(&mut sim);
        let owner = sim.interner.get("Americans").unwrap();
        sim.session.house_order = vec![owner];
        sim.session.current_house = Some(owner);
        sim.session.game_mode_nonzero = true;
        let entity = sim
            .construct_runtime_techno(
                "E1",
                "Americans",
                6,
                5,
                0,
                0,
                &rules,
                TechnoConstructorInit::FreshScenario,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            entity.vision_range, 0,
            "all three inputs lose information in fog range"
        );
        assert_eq!(entity.sight_is_zero, sight == 0);
        let (id, outcome) = sim.unlimbo_after_constructor_managers(entity, Some(&rules), None);
        assert!(matches!(outcome, RevealOutcome::Revealed { .. }));
        sim.object_conceal(id);
        let saved = GameSnapshot::save(&sim, 0, 0, "exact Sight predicate", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            restored.substrate.entities.get(id).unwrap().sight_is_zero,
            sight == 0
        );
        let hash = restored.state_hash();
        let mut changed_type_predicate = GameSnapshot::load(&saved).unwrap().sim;
        changed_type_predicate
            .restore_after_snapshot_load()
            .unwrap();
        changed_type_predicate
            .substrate
            .entities
            .get_mut(id)
            .unwrap()
            .sight_is_zero = sight != 0;
        assert_ne!(
            changed_type_predicate.state_hash(),
            hash,
            "type predicate is shared input"
        );
        // The same rules-less transaction used by admitted passenger/re-entry
        // callers must run +198(owner) and then the exact raw-Type zero gate.
        assert!(matches!(
            restored.reveal(id),
            RevealOutcome::Revealed { .. }
        ));
        assert_eq!(
            restored
                .substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house,
            sight != 0,
            "raw Sight={sight}"
        );
    }
}

#[test]
fn infantry_owner_discovery_leaves_building_power_radar_and_spysat_inputs_unchanged() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[BuildingTypes]\n0=OBS\n\
         [E1]\nStrength=100\nSpeed=4\nSight=8\n\
         [OBS]\nStrength=500\nFoundation=1x1\nPower=100\nRadar=yes\nSpySat=yes\nSight=8\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(18);
    install_constructor_test_playfield(&mut sim);
    install_constructor_flat_terrain(&mut sim);
    install_american_house(&mut sim);
    let owner = sim.interner.get("Americans").unwrap();
    sim.session.house_order = vec![owner];
    sim.session.current_house = Some(owner);
    sim.session.game_mode_nonzero = true;
    assert_eq!(
        sim.spawn_from_map(
            &[map_entity("OBS", EntityCategory::Structure, (6, 5))],
            Some(&rules),
        ),
        1
    );
    sim.advance_tick(&[], Some(&rules), None, None, 66);
    let before = (
        sim.power_states[&owner].total_output,
        sim.power_states[&owner].total_drain,
        crate::sim::radar::has_radar_for_owner(&sim, &rules, "Americans"),
        sim.houses[&owner].spy_sat_active,
    );
    let id = sim
        .spawn_object("E1", "Americans", 7, 5, 0, &rules)
        .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .discovery
            .discovered_by_current_house
    );
    sim.advance_tick(&[], Some(&rules), None, None, 66);
    let after = (
        sim.power_states[&owner].total_output,
        sim.power_states[&owner].total_drain,
        crate::sim::radar::has_radar_for_owner(&sim, &rules, "Americans"),
        sim.houses[&owner].spy_sat_active,
    );
    assert_eq!(before, (100, 0, true, true));
    assert_eq!(after, before);
}

#[test]
fn building_light_allocates_only_after_authored_or_held_placement_succeeds() {
    // Retail Building440DFD/446767 allocate a nullable614 only after successful
    // placement. Exercise both actual Unlimbo paths, including failed marks.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=GALITE\n[GALITE]\nStrength=100\n\
         LightVisibility=5000\nLightIntensity=.2\n",
    ))
    .unwrap();
    let mut authored = Simulation::with_seed(0x1a41);
    install_constructor_test_playfield(&mut authored);
    assert_eq!(
        authored.spawn_from_map(
            &[map_entity("GALITE", EntityCategory::Structure, (6, 5))],
            Some(&rules),
        ),
        1
    );
    let (&authored_id, source) = authored
        .lighting_sources
        .buildings
        .first_key_value()
        .unwrap();
    assert!(source.active);
    assert_eq!((source.rx, source.ry), (6, 5));
    assert_eq!(authored.lighting_sources.pending.len(), 1);
    authored.discard_lighting_events();
    authored.set_building_light_active(authored_id, false);
    authored.allocate_building_light(authored_id, &rules);
    assert!(
        !authored.lighting_sources.buildings[&authored_id].active,
        "a repeated construction callback must not activate an existing614"
    );
    assert_eq!(authored.lighting_sources.pending.len(), 1);

    let mut held = Simulation::with_seed(0x1a42);
    install_constructor_test_playfield(&mut held);
    let held_id = held
        .construct_object_limbo_at_height("GALITE", "Americans", 0, 0, 0, 0, &rules)
        .expect("construct held lamp");
    assert!(held.lighting_sources.buildings.is_empty());
    assert!(held.lighting_sources.pending.is_empty());
    assert!(
        held.reveal_constructed_object_at_height(
            held_id,
            6,
            5,
            0,
            0,
            PlacementEvidence::MarkFailed,
            &rules
        )
        .is_none()
    );
    assert!(held.lighting_sources.buildings.is_empty());
    assert!(held.lighting_sources.pending.is_empty());
    let rng_after_constructor = held.scenario_rng.logical_state();
    assert_eq!(
        held.reveal_constructed_object_at_height(
            held_id,
            6,
            5,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            &rules
        ),
        Some(held_id)
    );
    assert_eq!(held.scenario_rng.logical_state(), rng_after_constructor);
    assert!(held.lighting_sources.buildings[&held_id].active);
    assert_eq!(held.lighting_sources.pending.len(), 1);

    let mut failed_new = Simulation::with_seed(0x1a43);
    install_constructor_test_playfield(&mut failed_new);
    assert!(
        failed_new
            .spawn_object_at_height("GALITE", "Americans", 1, 1, 0, 0, &rules)
            .is_none()
    );
    assert!(failed_new.lighting_sources.buildings.is_empty());
    assert!(failed_new.lighting_sources.pending.is_empty());
}

fn install_constructor_test_playfield(sim: &mut Simulation) {
    sim.session.map_width = 10;
    sim.session.map_height = 10;
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 10,
        off_fc: 0,
        off_100: 0,
        off_104: 10,
        off_108: 10,
    });
}

fn install_constructor_flat_terrain(sim: &mut Simulation) {
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
        10,
        10,
        |rx, ry| ResolvedTerrainCell {
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_clear_cell(rx, ry)
        },
    ));
}

fn assert_generated_projection_rejects_before_mutation(
    seed: u64,
    entities: &[MapEntity],
    table: &GeneratedTechnoInitTable,
    expected_error: GeneratedTechnoInitError,
) {
    let rules = constructor_rules();
    let mut sim = Simulation::with_seed(seed);
    install_american_house(&mut sim);
    let scenario_before = sim.scenario_rng.logical_state();
    let stable_id_before = sim.substrate.next_stable_object_id;
    let enter_order_before = sim.substrate.next_air_tracker_order.current();
    let occupancy_generation_before = sim.substrate.occupancy.generation();
    let raw_occupation_entries_before = sim.substrate.raw_cell_occupation.entry_count();

    assert_eq!(
        sim.spawn_generated_from_map_with_resolved(entities, &rules, None, table,),
        Err(expected_error)
    );
    assert_eq!(sim.scenario_rng.logical_state(), scenario_before);
    assert_eq!(sim.substrate.next_stable_object_id, stable_id_before);
    assert_eq!(
        sim.substrate.next_air_tracker_order.current(),
        enter_order_before
    );
    assert_eq!(
        sim.substrate.occupancy.generation(),
        occupancy_generation_before
    );
    assert_eq!(sim.substrate.occupancy.occupied_cell_count(), 0);
    assert_eq!(sim.substrate.logic.len(), 0);
    assert!(sim.substrate.entities.is_empty());
    assert_eq!(
        sim.substrate.raw_cell_occupation.entry_count(),
        raw_occupation_entries_before
    );
    for entity in entities {
        assert!(sim.substrate.occupancy.is_empty_on_layer(
            entity.cell_x,
            entity.cell_y,
            MovementLayer::Ground,
        ));
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(entity.cell_x, entity.cell_y),
            0
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .deck_bits(entity.cell_x, entity.cell_y),
            0
        );
    }
}

fn constructor_overlay_registry() -> crate::map::overlay_types::OverlayTypeRegistry {
    crate::map::overlay_types::OverlayTypeRegistry::from_ini(
        &IniFile::from_str(
            "[OverlayTypes]\n0=TESTORE\n1=TESTWALL\n\
             [TESTORE]\nTiberium=yes\nLand=Tiberium\n\
             [TESTWALL]\nWall=yes\nCrushable=yes\nLand=Wall\n",
        ),
        None,
    )
}

#[test]
fn techno_constructor_live_overlay_context_admits_ore_and_structural_bridge() {
    let seed = 0xC701_0017;
    let rules = constructor_rules();
    let registry = constructor_overlay_registry();
    let mut sim = Simulation::with_seed(seed);
    install_constructor_test_playfield(&mut sim);
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    install_constructor_flat_terrain(&mut sim);

    let ore_authored = (6, 5);
    let ore_runtime = (7, 5);
    let bridge_cell = (8, 5);
    {
        let bridge = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(bridge_cell.0, bridge_cell.1)
            .unwrap();
        bridge.level = 3;
        bridge.bridge_deck_level = 7;
        bridge.has_bridge_deck = true;
        bridge.bridge_walkable = true;
        bridge.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        bridge.bridge_facts.overlay_id = Some(0);
    }
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain(
            sim.resolved_terrain.as_ref().unwrap(),
            true,
            300,
        ),
    );
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(10, 10);
    for cell in [ore_authored, ore_runtime, bridge_cell] {
        overlays.place_overlay(cell.0, cell.1, 0, 0);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .bridge_facts
            .overlay_id = Some(0);
    }
    sim.overlay_grid = Some(overlays);

    assert_eq!(
        sim.spawn_from_map_with_resolved_and_overlay_registry(
            &[
                map_entity("MTNK", EntityCategory::Unit, ore_authored),
                MapEntity {
                    high: true,
                    ..map_entity("MTNK", EntityCategory::Unit, bridge_cell)
                },
            ],
            Some(&rules),
            None,
            Some(&registry),
        ),
        2,
    );
    let authored = sim.substrate.entities.get(1).unwrap();
    assert_eq!((authored.position.rx, authored.position.ry), ore_authored);
    assert!(!authored.on_bridge);
    let bridge = sim.substrate.entities.get(2).unwrap();
    assert_eq!((bridge.position.rx, bridge.position.ry), bridge_cell);
    assert_eq!(bridge.position.z, 7);
    assert!(bridge.on_bridge);
    assert_ne!(
        sim.substrate
            .raw_cell_occupation
            .deck_bits(bridge_cell.0, bridge_cell.1)
            & crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        0,
    );

    let runtime = sim
        .spawn_object_with_overlay_registry(
            "MTNK",
            "Americans",
            ore_runtime.0,
            ore_runtime.1,
            0,
            &rules,
            &registry,
        )
        .expect("non-wall overlay must not veto runtime Unit Unlimbo");
    assert_eq!(
        (
            sim.substrate.entities.get(runtime).unwrap().position.rx,
            sim.substrate.entities.get(runtime).unwrap().position.ry
        ),
        ore_runtime
    );

    let mut expected = SimRng::new(seed);
    for _ in 0..3 {
        let _ = expected.next_u32();
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn techno_constructor_wall_rejection_precedes_mutation_and_keeps_graph_draws_spent() {
    let seed = 0xC701_0018;
    let rules = constructor_rules();
    let registry = constructor_overlay_registry();
    let mut sim = Simulation::with_seed(seed);
    install_constructor_test_playfield(&mut sim);
    install_constructor_flat_terrain(&mut sim);
    let wall_cell = (6, 5);
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(10, 10);
    overlays.place_overlay(wall_cell.0, wall_cell.1, 1, 0);
    sim.overlay_grid = Some(overlays);
    // Unit73F0A0 reads the native Cell's overlay identity. OverlayGrid's
    // presentation record alone does not supply that class-admission input.
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(wall_cell.0, wall_cell.1)
        .unwrap()
        .bridge_facts
        .overlay_id = Some(1);

    let parent_id = sim
        .construct_object_limbo_at_height("CARRIER", "Americans", 2, 2, 9, 0, &rules)
        .expect("constructor-complete carrier graph");
    let child_ids = sim
        .substrate
        .entities
        .get(parent_id)
        .unwrap()
        .spawn_manager
        .as_ref()
        .unwrap()
        .slots
        .iter()
        .filter_map(|slot| slot.spawn)
        .collect::<Vec<_>>();
    assert_eq!(child_ids.len(), 3);
    assert!(
        sim.reveal_constructed_object_at_height_with_overlay_context(
            parent_id,
            wall_cell.0,
            wall_cell.1,
            0x80,
            7,
            PlacementEvidence::EvaluateMark,
            &rules,
            Some(&registry),
        )
        .is_none()
    );
    let rejected = sim.substrate.entities.get(parent_id).unwrap();
    assert_eq!(
        (
            rejected.position.rx,
            rejected.position.ry,
            rejected.position.z
        ),
        (0, 0, 0)
    );
    assert_eq!(rejected.body_facing.destination(), 9 << 8);
    assert!(rejected.lifecycle.in_limbo && !rejected.lifecycle.cell_marked);
    assert!(
        child_ids
            .iter()
            .all(|id| sim.substrate.entities.contains(*id))
    );

    let mut expected = SimRng::new(seed);
    for _ in 0..4 {
        let _ = expected.next_u32();
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert!(sim.discard_constructed_limbo(parent_id, Some(&rules)));
    assert!(sim.substrate.entities.is_empty());
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn techno_constructor_routes_preserve_components_and_authored_overrides() {
    // Rust regression: preserve existing route differences, including map
    // omissions. This fixture does not establish native constructor parity.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=CREW\n[VehicleTypes]\n0=SHIP\n\
         [AircraftTypes]\n0=PLANE\n[BuildingTypes]\n0=GATE\n\
         [CREW]\nStrength=200\nSpeed=4\nDontScore=yes\nOccupier=yes\n\
         ImmuneToRadiation=yes\nCrushable=yes\n\
         [SHIP]\nStrength=200\nSpeed=0\nDontScore=yes\nTurret=yes\n\
         Passengers=3\nSizeLimit=2\nLocomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
         [PLANE]\nStrength=200\nSpeed=8\nDontScore=yes\nAmmo=5\n\
         Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
         [GATE]\nStrength=200\nDontScore=yes\nGate=yes\nBunker=yes\n\
         CanBeOccupied=yes\nMaxNumberOccupants=4\nFoundation=1x1\n",
    ))
    .unwrap();
    for (type_id, category) in [
        ("CREW", EntityCategory::Infantry),
        ("SHIP", EntityCategory::Unit),
        ("PLANE", EntityCategory::Aircraft),
        ("GATE", EntityCategory::Structure),
    ] {
        for route in 0..3 {
            let mut sim = Simulation::with_seed(0xC701_0021);
            sim.debug_event_logging = true;
            install_american_house(&mut sim);
            if type_id == "CREW" && route != 0 {
                sim.spawn_object_at_height(type_id, "Americans", 4, 4, 64, 0, &rules)
                    .expect("resident occupies the first subcell at the requested cell");
            }
            let id = match route {
                0 => {
                    let mut authored = map_entity(type_id, category, (4, 4));
                    authored.health = 128;
                    authored.veterancy = 100;
                    authored.facing = 64;
                    authored.sub_cell = 3;
                    authored.recruitable_a = false;
                    authored.recruitable_b = false;
                    assert_eq!(sim.spawn_from_map(&[authored], Some(&rules)), 1);
                    sim.substrate.entities.values().next().unwrap().stable_id
                }
                1 => sim
                    .spawn_object_at_height(type_id, "Americans", 4, 4, 64, 0, &rules)
                    .unwrap(),
                _ => sim
                    .spawn_object_limbo_at_height(type_id, "Americans", 4, 4, 64, 0, &rules)
                    .unwrap(),
            };
            let entity = sim.substrate.entities.get(id).unwrap();
            assert!(entity.debug_log.is_some(), "{type_id} route {route}");
            assert!(entity.dont_score);
            assert_eq!(entity.health.current, if route == 0 { 100 } else { 200 });
            assert_eq!(entity.veterancy(), if route == 0 { 100 } else { 0 });
            assert_eq!(entity.lifecycle.in_limbo, route == 2);
            if route == 0 {
                assert!(!entity.base_defense_response.recruitable_a);
                assert!(!entity.base_defense_response.recruitable_b);
            }
            match type_id {
                "CREW" => {
                    assert!(entity.animation.is_none());
                    assert!(entity.infantry.is_some());
                    assert!(entity.infantry_sprite_pose().is_some());
                    assert!(entity.crushable && entity.occupier && entity.immune_to_radiation);
                    if route == 2 {
                        assert_eq!(entity.sub_cell, None, "a held constructor reserves no spot");
                        let coord = crate::sim::movement::ground_pose::position_world_coord(
                            &entity.position,
                        );
                        assert_eq!((coord.x, coord.y, coord.z), (0, 0, 0));
                    } else {
                        let sub_cell = entity.sub_cell.unwrap();
                        if route == 0 {
                            assert_eq!(sub_cell, 3);
                        } else {
                            assert!(
                                crate::sim::movement::bump_crush::FUNCTIONAL_SUB_CELLS
                                    .contains(&sub_cell)
                            );
                            let resident = sim
                                .substrate
                                .entities
                                .values()
                                .find(|resident| resident.stable_id() != id)
                                .unwrap();
                            assert_ne!(Some(sub_cell), resident.sub_cell);
                        }
                        let (x, y) = crate::util::lepton::subcell_lepton_offset(Some(sub_cell));
                        assert_eq!((entity.position.sub_x, entity.position.sub_y), (x, y));
                    }
                }
                "SHIP" => {
                    assert!(entity.barrel_facing.is_some());
                    assert_eq!(
                        entity.locomotor.as_ref().unwrap().kind,
                        crate::rules::locomotor_type::LocomotorKind::Ship
                    );
                    assert_eq!(
                        entity
                            .locomotor
                            .as_ref()
                            .and_then(|l| l.selected_ship_runtime())
                            .and_then(|r| r.retained())
                            .is_some(),
                        route != 0
                    );
                    let cargo = entity.passenger_role.cargo().unwrap();
                    assert_eq!((cargo.capacity, cargo.size_limit), (3, 2));
                }
                "PLANE" => {
                    assert_eq!(entity.aircraft_mission.is_some(), route != 0);
                    if route != 0 {
                        assert!(matches!(
                            entity.aircraft_mission,
                            Some(crate::sim::aircraft::AircraftMission::Idle)
                        ));
                    }
                    assert_eq!(
                        entity
                            .aircraft_ammo
                            .as_ref()
                            .map(|ammo| (ammo.current, ammo.max)),
                        Some((5, 5))
                    );
                }
                "GATE" => {
                    assert!(entity.building_gate.is_some() && entity.bunker_runtime.is_some());
                    assert_eq!(entity.foundation, "1x1");
                    let cargo = entity.passenger_role.cargo().unwrap();
                    assert_eq!((cargo.capacity, cargo.size_limit), (4, 1));
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn techno_constructor_raw_entity_constructor_is_world_spawn_only() {
    fn collect_rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read source directory") {
            let path = entry.expect("read source entry").path();
            if path.is_dir() {
                collect_rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rust_files(&root.join("src"), &mut files);
    let needle = ["GameEntity::", "new_at_frame_from_constructor_word("].concat();
    let mut owners = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(&path).expect("read Rust source");
        let count = source.matches(&needle).count();
        if count != 0 {
            let relative = path
                .strip_prefix(root)
                .expect("source under manifest root")
                .to_string_lossy()
                .replace('\\', "/");
            owners.push(relative);
        }
    }
    owners.sort();
    assert_eq!(
        owners,
        vec![
            "src/sim/world/world_spawn.rs".to_string(),
            "src/sim/world/world_spawn/construction.rs".to_string(),
        ]
    );

    let production_zero_helper = ["new_at_frame_", "zero_for_diagnostics"].concat();
    let game_entity_source = std::fs::read_to_string(root.join("src/sim/game_entity.rs"))
        .expect("read GameEntity source");
    assert!(
        !game_entity_source.contains(&production_zero_helper),
        "production diagnostics must not synthesize a Techno constructor word"
    );
}

#[test]
fn techno_constructor_diagnostic_path_is_simulation_owned_and_draws_once() {
    let seed = 0xC701_0006;
    let mut sim = Simulation::with_seed(seed);
    let mut expected = SimRng::new(seed);
    let expected_word = (expected.next_u32() & 0xFFFF) as u16;
    let owner = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern("GACNST");

    let stable_id = sim.insert_synthetic_techno_for_diagnostics(
        10,
        12,
        0,
        0,
        owner,
        Health { current: 1000 },
        type_ref,
        EntityCategory::Structure,
        0,
        6,
        false,
    );

    assert_eq!(stable_id, 1);
    assert_eq!(
        sim.substrate
            .entities
            .get(stable_id)
            .expect("diagnostic Techno stored")
            .techno_ctor_random_word,
        expected_word
    );
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn techno_constructor_runtime_fresh_paths_draw_once_after_type_resolution() {
    let seed = 0xC701_0001;
    let rules = constructor_rules();
    let mut sim = Simulation::with_seed(seed);
    install_constructor_test_playfield(&mut sim);
    let mut expected = SimRng::new(seed);

    let before_invalid = sim.scenario_rng.logical_state();
    assert!(
        sim.spawn_object("MISSING", "Americans", 1, 1, 0, &rules)
            .is_none()
    );
    assert_eq!(sim.scenario_rng.logical_state(), before_invalid);

    let placed_word = (expected.next_u32() & 0xFFFF) as u16;
    let placed = sim
        .spawn_object("MTNK", "Americans", 6, 5, 0, &rules)
        .expect("placed runtime Techno");
    assert_eq!(
        sim.substrate
            .entities
            .get(placed)
            .unwrap()
            .techno_ctor_random_word,
        placed_word
    );

    let _failed_word = (expected.next_u32() & 0xFFFF) as u16;
    assert!(
        sim.spawn_object("BASE", "Americans", 1, 1, 0, &rules)
            .is_none()
    );
    assert!(sim.substrate.entities.get(2).is_none());

    let limbo_word = (expected.next_u32() & 0xFFFF) as u16;
    let limbo = sim
        .spawn_object_limbo_at_height("E1", "Americans", 6, 6, 0, 0, &rules)
        .expect("limbo runtime Techno");
    let limbo_entity = sim.substrate.entities.get(limbo).unwrap();
    assert_eq!(limbo_entity.techno_ctor_random_word, limbo_word);
    assert!(limbo_entity.lifecycle.in_limbo);
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn techno_constructor_spawn_manager_pool_draws_parent_then_children_and_cancels_as_one_graph() {
    let seed = 0xC701_0010;
    let rules = constructor_rules();
    let mut sim = Simulation::with_seed(seed);
    install_american_house(&mut sim);
    let mut expected = SimRng::new(seed);
    let words = (0..4)
        .map(|_| (expected.next_u32() & 0xFFFF) as u16)
        .collect::<Vec<_>>();

    let parent_id = sim
        .construct_object_limbo_at_height("CARRIER", "Americans", 0, 0, 0, 0, &rules)
        .expect("factory-held carrier constructor");
    let parent = sim
        .substrate
        .entities
        .get(parent_id)
        .expect("carrier parent");
    assert_eq!(parent.techno_ctor_random_word, words[0]);
    let child_ids = parent
        .spawn_manager
        .as_ref()
        .expect("constructor spawn manager")
        .slots
        .iter()
        .map(|slot| slot.spawn.expect("constructor-filled spawn slot"))
        .collect::<Vec<_>>();
    assert_eq!(child_ids, vec![2, 3, 4]);
    for (index, child_id) in child_ids.iter().copied().enumerate() {
        let child = sim.substrate.entities.get(child_id).expect("spawn child");
        assert_eq!(child.techno_ctor_random_word, words[index + 1]);
        assert_eq!(child.spawn_owner_id, Some(parent_id));
        assert!(child.lifecycle.in_limbo && !child.lifecycle.cell_marked);
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());

    let after_constructor = sim.scenario_rng.logical_state();
    assert!(sim.discard_constructed_limbo(parent_id, Some(&rules)));
    assert!(sim.substrate.entities.is_empty());
    assert_eq!(sim.scenario_rng.logical_state(), after_constructor);
}

#[test]
fn techno_constructor_slave_manager_pool_draws_parent_then_children_and_cancels_as_one_graph() {
    let seed = 0xC701_0011;
    let rules = constructor_rules();
    let mut sim = Simulation::with_seed(seed);
    install_american_house(&mut sim);
    let mut expected = SimRng::new(seed);
    let words = (0..3)
        .map(|_| (expected.next_u32() & 0xFFFF) as u16)
        .collect::<Vec<_>>();

    let parent_id = sim
        .construct_object_limbo_at_height("SMIN", "Americans", 0, 0, 0, 0, &rules)
        .expect("factory-held slave miner constructor");
    let slave_ids = sim
        .substrate
        .entities
        .get(parent_id)
        .and_then(|parent| parent.slave_manager.as_ref())
        .expect("constructor slave manager")
        .slaves()
        .collect::<Vec<_>>();
    assert_eq!(slave_ids, vec![2, 3]);
    assert_eq!(
        sim.substrate
            .entities
            .get(parent_id)
            .unwrap()
            .techno_ctor_random_word,
        words[0]
    );
    for (index, slave_id) in slave_ids.iter().copied().enumerate() {
        let slave = sim.substrate.entities.get(slave_id).expect("slave child");
        assert_eq!(slave.techno_ctor_random_word, words[index + 1]);
        assert_eq!(slave.slave.owner(), Some(parent_id));
        assert!(slave.lifecycle.in_limbo && !slave.lifecycle.cell_marked);
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());

    let after_constructor = sim.scenario_rng.logical_state();
    assert!(sim.discard_constructed_limbo(parent_id, Some(&rules)));
    assert!(sim.substrate.entities.is_empty());
    assert_eq!(sim.scenario_rng.logical_state(), after_constructor);
}

#[test]
fn techno_constructor_manager_pools_survive_delivery_without_reconstruction() {
    let rules = constructor_rules();
    for (seed, parent_type, expected_child_count) in [
        (0xC701_0012, "CARRIER", 3usize),
        (0xC701_0013, "SMIN", 2usize),
    ] {
        let mut sim = Simulation::with_seed(seed);
        install_constructor_test_playfield(&mut sim);
        let parent_id = sim
            .construct_object_limbo_at_height(parent_type, "Americans", 0, 0, 0, 0, &rules)
            .expect("held manager parent");
        let child_ids = if parent_type == "CARRIER" {
            sim.substrate
                .entities
                .get(parent_id)
                .unwrap()
                .spawn_manager
                .as_ref()
                .unwrap()
                .slots
                .iter()
                .filter_map(|slot| slot.spawn)
                .collect::<Vec<_>>()
        } else {
            sim.substrate
                .entities
                .get(parent_id)
                .unwrap()
                .slave_manager
                .as_ref()
                .unwrap()
                .slaves()
                .collect::<Vec<_>>()
        };
        assert_eq!(child_ids.len(), expected_child_count);
        let after_constructor = sim.scenario_rng.logical_state();

        assert_eq!(
            sim.reveal_constructed_object_at_height(
                parent_id,
                6,
                5,
                0,
                0,
                PlacementEvidence::EvaluateMark,
                &rules,
            ),
            Some(parent_id)
        );
        assert_eq!(sim.scenario_rng.logical_state(), after_constructor);
        let retained_ids = if parent_type == "CARRIER" {
            sim.substrate
                .entities
                .get(parent_id)
                .unwrap()
                .spawn_manager
                .as_ref()
                .unwrap()
                .slots
                .iter()
                .filter_map(|slot| slot.spawn)
                .collect::<Vec<_>>()
        } else {
            sim.substrate
                .entities
                .get(parent_id)
                .unwrap()
                .slave_manager
                .as_ref()
                .unwrap()
                .slaves()
                .collect::<Vec<_>>()
        };
        assert_eq!(retained_ids, child_ids);
    }
}

#[test]
fn techno_constructor_failed_parent_placement_discards_both_manager_pool_kinds_without_rewind() {
    let rules = constructor_rules();
    for (seed, parent_type, draw_count) in [
        (0xC701_0014, "CARRIER", 4usize),
        (0xC701_0015, "SMIN", 3usize),
    ] {
        let mut sim = Simulation::with_seed(seed);
        install_constructor_test_playfield(&mut sim);
        let mut expected = SimRng::new(seed);
        for _ in 0..draw_count {
            let _ = expected.next_u32();
        }

        assert!(
            sim.spawn_object_at_height(parent_type, "Americans", 1, 1, 0, 0, &rules)
                .is_none()
        );
        assert!(sim.substrate.entities.is_empty());
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    }
}

#[test]
fn techno_constructor_unit_can_enter_rejection_discards_eager_pool_without_refunding_draws() {
    let seed = 0xC701_0016;
    let rules = constructor_rules();
    let mut sim = Simulation::with_seed(seed);
    install_constructor_test_playfield(&mut sim);
    install_constructor_flat_terrain(&mut sim);
    let mut expected = SimRng::new(seed);

    let blocker_word = (expected.next_u32() & 0xFFFF) as u16;
    assert_eq!(
        sim.spawn_from_map(
            &[map_entity("MTNK", EntityCategory::Unit, (6, 5))],
            Some(&rules),
        ),
        1
    );
    // Authored placement retains nonzero A8E7AC and skips Object5F4F1B's
    // class gate (saved unit_unlimbo.json authored_rows). Runtime placement
    // outside that scope still asks UnitClass::Can_Enter_Cell. Its parent and
    // three SpawnManager children are constructed before that refusal.
    assert!(!sim.object_placement_scope_active());
    for _ in 0..4 {
        let _ = expected.next_u32();
    }
    assert!(
        sim.spawn_object("CARRIER", "Americans", 6, 5, 0, &rules)
            .is_none()
    );
    let blocker_id = 1;
    assert_eq!(
        sim.substrate
            .entities
            .get(blocker_id)
            .unwrap()
            .techno_ctor_random_word,
        blocker_word
    );

    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert_eq!(sim.substrate.next_stable_object_id, 6);
    assert_eq!(sim.substrate.entities.len(), 1);
    let blocker = sim.substrate.entities.get(blocker_id).unwrap();
    assert!(blocker.lifecycle.cell_marked && !blocker.lifecycle.in_limbo);
    assert!(
        sim.substrate
            .entities
            .values()
            .all(|entity| { !matches!(sim.interner.resolve(entity.type_ref), "CARRIER" | "HORN") })
    );
}

#[test]
fn techno_constructor_fixed_map_uses_native_category_order_after_prior_mark_draw() {
    let seed = 0xC701_0002;
    let rules = constructor_rules();
    let mut sim = Simulation::with_seed(seed);
    install_constructor_test_playfield(&mut sim);
    install_american_house(&mut sim);
    let mut expected = SimRng::new(seed);
    assert_eq!(sim.scenario_rng.next_u32(), expected.next_u32());
    let mut invalid_owner = map_entity("MTNK", EntityCategory::Unit, (3, 3));
    invalid_owner.owner = "UnresolvableHouse".to_string();
    let entities = vec![
        map_entity("MISSING", EntityCategory::Unit, (2, 2)),
        invalid_owner,
        map_entity("MTNK", EntityCategory::Unit, (6, 5)),
        map_entity("ORCA", EntityCategory::Aircraft, (7, 5)),
        map_entity("E1", EntityCategory::Infantry, (8, 5)),
        map_entity("BASE", EntityCategory::Structure, (9, 5)),
        map_entity("BASE", EntityCategory::Structure, (1, 1)),
    ];
    let expected_words: Vec<u16> = (0..5)
        .map(|_| (expected.next_u32() & 0xFFFF) as u16)
        .collect();

    // Authored scope skips ordinary admission at Object5F4F1B, so the fifth
    // outside BASE remains a live marked object. Techno6F6CFE still publishes
    // its actual outside membership; scope does not make that byte true.
    assert_eq!(sim.spawn_from_map(&entities, Some(&rules)), 5);
    let actual_words: Vec<u16> = sim
        .substrate
        .entities
        .values()
        .map(|entity| entity.techno_ctor_random_word)
        .collect();
    assert_eq!(actual_words, expected_words);
    let outside = sim.substrate.entities.get(5).unwrap();
    assert_eq!((outside.position.rx, outside.position.ry), (1, 1));
    assert!(!outside.in_playfield);
    assert!(outside.lifecycle.object_alive);
    assert!(outside.lifecycle.cell_marked && !outside.lifecycle.in_limbo);
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn techno_constructor_generated_projection_installs_without_a_second_draw() {
    let rules = constructor_rules();
    let entity = map_entity("MTNK", EntityCategory::Unit, (7, 9));
    let table = GeneratedTechnoInitTable::try_new([GeneratedTechnoInit {
        entity_index: 0,
        techno_type: "MTNK".to_string(),
        cell: (7, 9),
        techno_ctor_random_word: 0xA55A,
        native_unique_id: -17,
    }])
    .unwrap();
    let mut sim = Simulation::with_seed(0xC701_0003);
    install_american_house(&mut sim);
    let before = sim.scenario_rng.logical_state();
    let native_before = sim.native_unique_ids.as_ref().unwrap().current_raw();

    assert_eq!(
        sim.spawn_generated_from_map_with_resolved(&[entity], &rules, None, &table,)
            .unwrap(),
        1
    );
    assert_eq!(sim.scenario_rng.logical_state(), before);
    assert_eq!(
        sim.native_unique_ids.as_ref().unwrap().current_raw(),
        native_before
    );
    assert_eq!(sim.substrate.entities.get(1).unwrap().native_unique_id, -17);
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .techno_ctor_random_word,
        0xA55A
    );

    assert!(matches!(
        GeneratedTechnoInitTable::try_new([
            GeneratedTechnoInit {
                entity_index: 0,
                techno_type: "MTNK".to_string(),
                cell: (7, 9),
                techno_ctor_random_word: 1,
                native_unique_id: 0,
            },
            GeneratedTechnoInit {
                entity_index: 0,
                techno_type: "MTNK".to_string(),
                cell: (7, 9),
                techno_ctor_random_word: 2,
                native_unique_id: 0,
            },
        ]),
        Err(GeneratedTechnoInitError::DuplicateEntityIndex(0))
    ));
}

#[test]
fn generated_projection_validates_the_whole_table_before_any_mutation() {
    // Every missing/mismatched slot is deliberately after a valid index 0.
    // An inline validator would therefore construct or mark the first map
    // entity before discovering the fault; the shared postconditions below
    // prove the complete table remains a preflight transaction.
    let entities = [
        map_entity("MTNK", EntityCategory::Unit, (7, 9)),
        map_entity("MTNK", EntityCategory::Unit, (8, 9)),
    ];
    let valid_first = || GeneratedTechnoInit {
        entity_index: 0,
        techno_type: "MTNK".to_string(),
        cell: (7, 9),
        techno_ctor_random_word: 0x1111,
        native_unique_id: 0,
    };

    let missing = GeneratedTechnoInitTable::try_new([valid_first()]).unwrap();
    assert_generated_projection_rejects_before_mutation(
        0xC701_0004,
        &entities,
        &missing,
        GeneratedTechnoInitError::MissingEntityIndex(1),
    );

    let unexpected = GeneratedTechnoInitTable::try_new([
        valid_first(),
        GeneratedTechnoInit {
            entity_index: 2,
            techno_type: "MTNK".to_string(),
            cell: (9, 9),
            techno_ctor_random_word: 0x2222,
            native_unique_id: 0,
        },
    ])
    .unwrap();
    assert_generated_projection_rejects_before_mutation(
        0xC701_0005,
        &entities,
        &unexpected,
        GeneratedTechnoInitError::UnexpectedEntityIndex(2),
    );

    let type_mismatch = GeneratedTechnoInitTable::try_new([
        valid_first(),
        GeneratedTechnoInit {
            entity_index: 1,
            techno_type: "ORCA".to_string(),
            cell: (8, 9),
            techno_ctor_random_word: 0x3333,
            native_unique_id: 0,
        },
    ])
    .unwrap();
    assert_generated_projection_rejects_before_mutation(
        0xC701_0006,
        &entities,
        &type_mismatch,
        GeneratedTechnoInitError::IdentityMismatch {
            entity_index: 1,
            expected_type: "ORCA".to_string(),
            found_type: "MTNK".to_string(),
            expected_cell: (8, 9),
            found_cell: (8, 9),
        },
    );

    let cell_mismatch = GeneratedTechnoInitTable::try_new([
        valid_first(),
        GeneratedTechnoInit {
            entity_index: 1,
            techno_type: "MTNK".to_string(),
            cell: (9, 9),
            techno_ctor_random_word: 0x4444,
            native_unique_id: 0,
        },
    ])
    .unwrap();
    assert_generated_projection_rejects_before_mutation(
        0xC701_0007,
        &entities,
        &cell_mismatch,
        GeneratedTechnoInitError::IdentityMismatch {
            entity_index: 1,
            expected_type: "MTNK".to_string(),
            found_type: "MTNK".to_string(),
            expected_cell: (9, 9),
            found_cell: (8, 9),
        },
    );
}

#[test]
fn techno_constructor_authored_upgrades_are_distinct_attached_live_entities_with_native_ids() {
    let seed = 0xC701_0005;
    let rules = constructor_rules();
    let mut sim = Simulation::with_seed(seed);
    sim.native_unique_ids = Some(
        crate::sim::native_identity::build_noncampaign_fresh_id_prefix(0, 0, 0, 0, 0, 0, 1, 1)
            .into_cursor(),
    );
    let native_before = sim
        .native_unique_ids
        .as_ref()
        .expect("native cursor")
        .current_raw();
    let mut expected = SimRng::new(seed);
    let mut base = map_entity("BASE", EntityCategory::Structure, (10, 12));
    base.structure_upgrades = [Some("UP1".to_string()), Some("UP2".to_string()), None];
    let words = [
        (expected.next_u32() & 0xFFFF) as u16,
        (expected.next_u32() & 0xFFFF) as u16,
        (expected.next_u32() & 0xFFFF) as u16,
    ];

    assert_eq!(sim.spawn_from_map(&[base], Some(&rules)), 3);
    let parent = sim.substrate.entities.get(1).unwrap();
    assert_eq!(parent.techno_ctor_random_word, words[0]);
    assert_eq!(
        parent.native_unique_id,
        native_before.wrapping_add(1) as i32
    );
    assert!(parent.lifecycle.cell_marked);
    for (stable_id, slot) in [(2, 0), (3, 1)] {
        let upgrade = sim.substrate.entities.get(stable_id).unwrap();
        assert_eq!(upgrade.techno_ctor_random_word, words[slot + 1]);
        assert_eq!(
            upgrade.native_unique_id,
            native_before.wrapping_add(slot as u32 + 2) as i32
        );
        assert_eq!(
            upgrade.structure_upgrade_link,
            Some(StructureUpgradeLink {
                parent_stable_id: 1,
                slot: slot as u8,
            })
        );
        assert!(!upgrade.lifecycle.in_limbo);
        assert!(!upgrade.lifecycle.cell_marked);
        assert!(upgrade.in_logic_vector);
        assert_eq!((upgrade.position.rx, upgrade.position.ry), (10, 12));
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert_eq!(
        sim.native_unique_ids.as_ref().unwrap().current_raw(),
        native_before.wrapping_add(3),
        "the parent and both accepted upgrade constructors consume one shared native ID each"
    );
}

#[test]
fn techno_constructor_failed_reveal_keeps_one_draw_and_reuses_identity() {
    let seed = 0xC701_0006;
    let rules = constructor_rules();
    let mut sim = Simulation::with_seed(seed);
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(1000));
    let mut expected = SimRng::new(seed);
    let word = (expected.next_u32() & 0xFFFF) as u16;
    let stable_id = sim
        .construct_object_limbo_at_height("MTNK", "Americans", 3, 3, 0, 0, &rules)
        .unwrap();

    assert!(
        sim.reveal_constructed_object_at_height(
            stable_id,
            3,
            3,
            0,
            0,
            PlacementEvidence::MarkFailed,
            &rules,
        )
        .is_none()
    );
    let held = sim.substrate.entities.get(stable_id).unwrap();
    assert!(held.lifecycle.in_limbo);
    assert_eq!(held.techno_ctor_random_word, word);
    assert_eq!(held.native_unique_id, 1001);
    assert_eq!(sim.native_unique_ids.as_ref().unwrap().current_raw(), 1001);
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert!(sim.discard_constructed_limbo(stable_id, Some(&rules)));
    assert!(sim.substrate.entities.get(stable_id).is_none());
}

fn signed_health_rules(strength: i32) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=UNIT\n[AircraftTypes]\n0=AIR\n[InfantryTypes]\n0=INF\n[BuildingTypes]\n0=BLD\n\
         [UNIT]\nStrength={strength}\nSpeed=4\n[AIR]\nStrength={strength}\nSpeed=4\n\
         [INF]\nStrength={strength}\nSpeed=4\n[BLD]\nStrength={strength}\nFoundation=1x1\n"
    ))).unwrap()
}

fn native_health_class(kind: &str) -> (EntityCategory, &'static str) {
    match kind {
        "unit" => (EntityCategory::Unit, "UNIT"),
        "aircraft" => (EntityCategory::Aircraft, "AIR"),
        "infantry" => (EntityCategory::Infantry, "INF"),
        "building" => (EntityCategory::Structure, "BLD"),
        other => panic!("unknown native class {other}"),
    }
}

#[test]
fn signed_constructor_and_authored_health_consume_original_corpus() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/object_health.json",
    ))
    .unwrap();
    assert_eq!(corpus["constructors"].as_array().unwrap().len(), 32);
    assert_eq!(corpus["map_health"].as_array().unwrap().len(), 416);
    for row in corpus["constructors"].as_array().unwrap() {
        let input = &row["input"];
        let (category, name) = native_health_class(input["kind"].as_str().unwrap());
        let strength = input["strength"].as_i64().unwrap() as i32;
        let rules = signed_health_rules(strength);
        let mut sim = Simulation::with_seed(7);
        let entity = sim
            .construct_runtime_techno(
                name,
                "Americans",
                6,
                5,
                0,
                0,
                &rules,
                TechnoConstructorInit::FreshScenario,
            )
            .unwrap()
            .unwrap();
        assert_eq!(entity.category, category);
        assert_eq!(
            entity.health.current,
            row["output"]["actual"].as_i64().unwrap() as i32,
            "{row}"
        );
        assert_eq!(
            entity.estimated_health.get(),
            row["output"]["estimated"].as_i64().unwrap() as i32,
            "{row}"
        );
    }
    for row in corpus["map_health"].as_array().unwrap() {
        let input = &row["input"];
        let (category, _) = native_health_class(input["kind"].as_str().unwrap());
        assert_eq!(
            authored_health::authored_health(
                category,
                input["authored"].as_i64().unwrap() as i32,
                input["strength"].as_i64().unwrap() as i32
            ),
            row["output"]["actual"].as_i64().unwrap() as i32,
            "{row}"
        );
    }
}

#[test]
fn aircraft_spawn_initializes_both_facings_without_a_turret_flag() {
    // Native rate observations plus the traced Unlimbo snaps at6F6DAA/414417.
    // Exercise both authored and runtime consumers of component construction.
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/facing_class.json",
    ))
    .unwrap();
    for row in rows.as_array().unwrap().iter().filter(|row| {
        row["input"]["rate_constructor"] == false
            && row["input"]["start"] == 100
            && row["input"]["operations"].as_array().unwrap().len() == 9
    }) {
        let rot = row["input"]["rot"].as_i64().unwrap();
        let rate = &row["observations"][0]["rate"];
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[AircraftTypes]\n0=AIR\n[AIR]\nStrength=100\nTurret=no\nROT={rot}\n"
        )))
        .unwrap();
        for direction in [0, 64, 255] {
            let mut sim = Simulation::with_seed(7);
            sim.session.binary_frame = 100;
            let runtime = sim
                .construct_runtime_techno(
                    "AIR",
                    "Americans",
                    6,
                    5,
                    direction,
                    0,
                    &rules,
                    TechnoConstructorInit::FreshScenario,
                )
                .unwrap()
                .unwrap();
            let mut placement = map_entity("AIR", EntityCategory::Aircraft, (6, 5));
            placement.facing = direction;
            assert_eq!(sim.spawn_from_map(&[placement], Some(&rules)), 1);
            let authored = sim.substrate.entities.values().next().unwrap();
            for entity in [&runtime, authored] {
                for facing in [entity.body_facing, entity.barrel_facing.unwrap()] {
                    assert_eq!(
                        serde_json::json!(facing.rot_per_frame()),
                        *rate,
                        "ROT={rot}"
                    );
                    assert_eq!(facing.destination(), u16::from(direction) << 8);
                    assert_eq!(facing.current(100), u16::from(direction) << 8);
                    assert_eq!(facing.timer_start_frame(), Some(100));
                    assert!(!facing.is_rotating(100));
                }
            }
        }
    }
}

#[test]
fn aircraft_ammo_initialization_matches_native_for_authored_and_runtime_objects() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_attack_release.json",
    ))
    .unwrap();
    let rows = corpus["initialization"].as_array().unwrap();
    assert_eq!(rows.len(), 21);
    for row in rows {
        let maximum = row["input"]["maximum"].as_i64().unwrap() as i32;
        let initial = row["input"]["initial"].as_i64().unwrap() as i32;
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[AircraftTypes]\n0=AIR\n[AIR]\nStrength=150\nAmmo={maximum}\nInitialAmmo={initial}\n"
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(7);
        let runtime = sim
            .construct_runtime_techno(
                "AIR",
                "Americans",
                6,
                5,
                0,
                0,
                &rules,
                TechnoConstructorInit::FreshScenario,
            )
            .unwrap()
            .unwrap();
        let placement = map_entity("AIR", EntityCategory::Aircraft, (6, 5));
        assert_eq!(sim.spawn_from_map(&[placement], Some(&rules)), 1);
        let authored = sim.substrate.entities.values().next().unwrap();
        for entity in [&runtime, authored] {
            let ammo = entity.aircraft_ammo.as_ref().unwrap();
            assert_eq!(ammo.current, row["ammo"].as_i64().unwrap() as i32, "{row}");
            assert_eq!(ammo.max, maximum);
            assert!(!ammo.release_pending());
        }
    }
}

#[test]
fn map_admission_uses_class_health_and_rejects_unresolved_types() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/object_health.json",
    ))
    .unwrap();
    for row in corpus["map_health"].as_array().unwrap() {
        let input = &row["input"];
        let strength = input["strength"].as_i64().unwrap() as i32;
        if ![100, 65536].contains(&strength) {
            continue;
        }
        let rules = signed_health_rules(strength);
        let (category, name) = native_health_class(input["kind"].as_str().unwrap());
        let mut sim = Simulation::with_seed(9);
        let mut placement = map_entity(name, category, (6, 5));
        placement.health = input["authored"].as_i64().unwrap() as i32;
        assert_eq!(sim.spawn_from_map(&[placement], Some(&rules)), 1, "{row}");
        let entity = sim.substrate.entities.values().next().unwrap();
        assert_eq!(
            entity.health.current,
            row["output"]["actual"].as_i64().unwrap() as i32,
            "{row}"
        );
        assert_eq!(
            entity.estimated_health.get(),
            row["output"]["estimated"].as_i64().unwrap() as i32,
            "{row}"
        );
    }
    let rules = signed_health_rules(100);
    for kind in ["unit", "aircraft", "infantry", "building"] {
        let (category, _) = native_health_class(kind);
        let mut sim = Simulation::with_seed(9);
        let before = sim.scenario_rng.logical_state();
        let missing = map_entity("MISSING", category, (6, 5));
        let wrong_class = map_entity(
            if category == EntityCategory::Unit {
                "BLD"
            } else {
                "UNIT"
            },
            category,
            (6, 5),
        );
        assert_eq!(sim.spawn_from_map(&[missing, wrong_class], Some(&rules)), 0);
        assert_eq!(sim.scenario_rng.logical_state(), before);
        assert!(sim.substrate.entities.is_empty());
    }
}

#[test]
fn authored_unlimbo_preserves_scenario_scope_and_applies_native_class_health() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/object_health.json",
    ))
    .unwrap();
    let rules = signed_health_rules(65_536);
    for kind in ["unit", "aircraft", "infantry", "building"] {
        let (category, name) = native_health_class(kind);
        let mut sim = Simulation::with_seed(9);
        install_constructor_test_playfield(&mut sim);
        let entity = sim
            .construct_runtime_techno(
                name,
                "Americans",
                1,
                1,
                0,
                0,
                &rules,
                TechnoConstructorInit::FreshScenario,
            )
            .unwrap()
            .unwrap();
        assert_eq!(entity.category, category);
        assert_eq!(entity.health.current, 65_536);
        // ScenarioFullInit686B4F keeps A8E7AC raised through all map-object
        // readers. This terrain-less diagnostic coordinate is outside the
        // ordinary playfield gate; that gate cannot supply a false authored
        // Unlimbo result. Original object_health/map_health supplies the
        // arithmetic golden independently of this Rust admission regression.
        assert!(!sim.object_placement_scope_active());
        let (id, outcome) = sim.unlimbo_authored_techno(entity, 128, Some(&rules), None);
        assert!(matches!(outcome, RevealOutcome::Revealed { .. }), "{kind}");
        assert!(!sim.object_placement_scope_active());
        let native = corpus["map_health"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["input"]["kind"] == kind
                    && row["input"]["strength"] == 65_536
                    && row["input"]["authored"] == 128
            })
            .expect("original authored-health arithmetic control");
        let admitted = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            admitted.health.current,
            native["output"]["actual"].as_i64().unwrap() as i32,
            "{kind}"
        );
        assert_eq!(
            admitted.estimated_health.get(),
            native["output"]["estimated"].as_i64().unwrap() as i32,
            "{kind}"
        );
        assert!(!admitted.lifecycle.in_limbo);
    }
}

/// The AI sale byte (`BuildingClass+0x6DC`): the constructor sets it and
/// Init_Managers clears it for a type without a Buildup (`0x00442CBC`); a
/// map-placed building takes its line's AI Sellable field instead
/// (`BuildingClass::ReadFromINI` `0x0044FB5B`).
#[test]
fn a_building_s_ai_sale_byte_follows_its_buildup_or_its_map_line() {
    let mut rules = constructor_rules();
    rules.set_buildup_control_for_test("BASE", [0, 25, 2]);
    let byte = |sim: &Simulation, id: u64| sim.substrate.entities.get(id).unwrap().ai_sellable;

    let mut built = Simulation::with_seed(0x6DC);
    install_constructor_test_playfield(&mut built);
    let base = built
        .spawn_object("BASE", "Americans", 9, 5, 0, &rules)
        .expect("BASE");
    let bare = built
        .spawn_object("UP1", "Americans", 6, 5, 0, &rules)
        .expect("UP1");
    assert!(byte(&built, base), "a Buildup keeps the byte");
    assert!(!byte(&built, bare), "no Buildup clears it");

    let mut authored = Simulation::with_seed(0x6DD);
    install_constructor_test_playfield(&mut authored);
    let mut sellable = map_entity("UP1", EntityCategory::Structure, (6, 5));
    sellable.structure_ai_sellable = true;
    let unsellable = map_entity("BASE", EntityCategory::Structure, (9, 5));
    assert_eq!(
        authored.spawn_from_map(&[sellable, unsellable], Some(&rules)),
        2
    );
    let bytes: Vec<bool> = authored
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .map(|id| byte(&authored, id))
        .collect();
    assert_eq!(bytes, [true, false], "the map line decides");
}

/// The AI repair byte (`BuildingClass+0x6CB`): a map line's AI Repairable
/// field (`BuildingClass::ReadFromINI` `0x0044FB70`), which Unlimbo sets
/// outside a campaign for a house no human controls whose type is not
/// `MultiplayPassive=` (`0x00440B4F..0x00440B7A`), and never clears.
#[test]
fn a_building_s_ai_repair_byte_follows_its_map_line_or_its_computer_owner() {
    let rules = constructor_rules();
    for (game_mode_nonzero, owner, field, expected) in [
        (true, "Computer", false, true),
        (true, "Human", false, false),
        (true, "Passive", false, false),
        (true, "Human", true, true),
        (false, "Computer", false, false),
        (false, "Human", false, false),
        (false, "Computer", true, true),
    ] {
        let mut sim = Simulation::with_seed(0x6CB);
        install_constructor_test_playfield(&mut sim);
        sim.session.game_mode_nonzero = game_mode_nonzero;
        for (name, human, passive) in [
            ("Computer", false, false),
            ("Human", true, false),
            ("Passive", false, true),
        ] {
            let id = sim.interner.intern(name);
            let mut house = crate::sim::house_state::HouseState::new(id, 0, None, human, 0, 10);
            house.player_control = human;
            house.multiplay_passive = passive;
            sim.houses.insert(id, house);
        }
        let mut line = map_entity("UP1", EntityCategory::Structure, (6, 5));
        line.owner = owner.to_string();
        line.structure_ai_repairable = field;
        assert_eq!(sim.spawn_from_map(&[line], Some(&rules)), 1);
        let building = sim.substrate.entities.values().next().unwrap();
        assert_eq!(
            building.ai_repairable, expected,
            "game mode {game_mode_nonzero}, {owner}, field {field}"
        );
    }
}
