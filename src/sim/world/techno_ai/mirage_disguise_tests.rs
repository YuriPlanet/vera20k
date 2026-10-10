//! Standing Mirage lifecycle through the shared post-Foot disguise owner.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{DriveCoord, MovementTarget};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;

fn mirage(order: bool, destination: Option<DriveCoord>) -> (Simulation, RuleSet) {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MGTK\n[MGTK]\nStrength=400\nSpeed=6\nCanDisguise=yes\nDisguiseWhenStill=yes\n",
    ))
    .expect("rules");
    rules.general.default_mirage_disguises = vec!["TREE01".to_string()];
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_flat_ground_grid(16));
    let mut entity = GameEntity::test_default(1, "MGTK", "Americans", 5, 5);
    entity.category = EntityCategory::Unit;
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                crate::sim::movement::DriveLocomotionRuntime::default()
                    .with_destination_for_test(destination)
            ))
    );
    entity.movement_target = order.then(MovementTarget::default);
    sim.substrate.entities.insert(entity);
    sim.interner = crate::sim::intern::test_interner();
    (sim, rules)
}

fn disguised(sim: &Simulation) -> bool {
    sim.substrate
        .entities
        .get(1)
        .and_then(|entity| entity.disguise.as_ref())
        .is_some_and(|disguise| disguise.is_disguised())
}

/// Original7468CD tests IsDisguised before setting the acquisition flag;
/// a retained idle disguise therefore cannot spend another Scenario pick.
/// Use the retail layered list, not the singleton authored motion fixture.
#[test]
fn an_idle_retail_mirage_retains_its_disguise_without_another_scenario_draw() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert!(retail.rules.general.default_mirage_disguises.len() > 1);
    let (mut sim, _) = mirage(false, None);
    sim.session.binary_frame = 100;
    sim.update_unit_disguise(1, &retail.rules).unwrap();
    assert!(disguised(&sim));
    let initial = sim.substrate.entities.get(1).unwrap().disguise.clone();
    let scenario = sim.scenario_rng.logical_state();
    sim.session.binary_frame = 101;
    sim.update_unit_disguise(1, &retail.rules).unwrap();
    assert_eq!(
        sim.scenario_rng.logical_state(),
        scenario,
        "an already-disguised idle visit must not choose another tree"
    );
    assert_eq!(sim.substrate.entities.get(1).unwrap().disguise, initial);
}

/// Original746A72/746A78 acquire a terrain type and NULL disguise house;
/// Unit746750 accepts that NULL house for an ordinary enemy observer.
#[test]
fn an_idle_retail_mirage_hides_from_an_ordinary_enemy_observer() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (mut sim, _) = mirage(false, None);
    let enemy = sim.interner.intern("Russians");
    sim.update_unit_disguise(1, &retail.rules).unwrap();
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(crate::sim::cloak_disguise::object_disguised_to(
        entity,
        enemy,
        None,
        None,
        &sim.interner,
    ));
}

/// `UnitClass::UpdateDisguise @ 0x007468C0` asks the locomotor's Is_Moving
/// (ILocomotion+0x10 at `0x007468F4`), which reads the Drive's own destination
/// and head, not the owner's order. A Mirage holding an order its Drive has
/// not taken up is still, so it takes a disguise. No production path is known
/// to leave a Mirage in that state; this pins the owner's answer.
#[test]
fn a_mirage_whose_drive_has_not_started_its_order_disguises() {
    let (mut sim, rules) = mirage(true, None);
    sim.update_unit_disguise(1, &rules).unwrap();
    assert!(disguised(&sim));
}

/// A Mirage whose Drive has a destination is moving, so UpdateDisguise takes
/// its clear arm (Is_Moving at `0x0074693D`, then ClearDisguise at
/// `0x00746AFF`) even without an order.
#[test]
fn a_disguised_mirage_whose_drive_has_a_destination_drops_its_disguise() {
    let (mut sim, rules) = mirage(false, Some(DriveCoord::cell(9, 5, 0)));
    let tree = sim.interner.intern("TREE01");
    let entity = sim.substrate.entities.get_mut(1).expect("mirage");
    entity
        .disguise
        .get_or_insert_with(Default::default)
        .acquire(0, Some(tree), None);
    assert!(disguised(&sim));
    sim.update_unit_disguise(1, &rules).unwrap();
    assert!(!disguised(&sim));
}

// The shared Foot fixture owns the inherited four-actor world, original
// registration transport and physical Cell receipts. This adapter adds the
// independently observed Mirage and limbo Infantry constructors only. The
// two calls below visit MGTK alone; this is not a whole-Logic-frame replay.
fn native_rng_values(bytes: &serde_json::Value) -> serde_json::Value {
    let mut value = serde_json::json!({});
    for stream in ["scenario", "main", "mapgen"] {
        value[stream] =
            serde_json::to_value(crate::sim::rng::SimRng::from_native_state_hex_for_test(
                bytes[stream].as_str().unwrap(),
            ))
            .unwrap();
    }
    value
}

fn whole_unit_fixture(
    native: &serde_json::Value,
    first: &serde_json::Value,
    rules: &RuleSet,
) -> super::mission_handlers::foot_mission_oracle_tests::SuppliedFootFixture {
    use super::mission_handlers::foot_mission_oracle_tests::{
        SuppliedFootFixture, install_recorded_cell_coordinates, oracle, signed, xyz,
    };
    use crate::sim::components::Health;
    use crate::sim::game_entity::InfantryRuntime;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionTimer};
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::CellListInsertion;
    use crate::sim::world::display_layers::DisplayLayer;
    use crate::util::fixed_math::SimFixed;
    use serde_json::json;
    use std::collections::BTreeMap;

    let inherited = oracle();
    let initial = &native["initialization"];
    let actors = &initial["actors"];
    let roles = &initial["actor_roles"];
    let mut supplied = inherited["rows"][0].clone();
    supplied["input"]["name"] = first["name"].clone();
    supplied["before"] = actors["source"]["foot"].clone();
    // The original harness Unlimbo'd all four inherited actors, then called
    // Unit Limbo for the enemy candidate before the first MGTK visit.
    supplied["input"]["candidate_live"] = json!(true);
    supplied["rng_before"] = native_rng_values(&first["rng_before"]);
    let mut fixture = SuppliedFootFixture::new(&supplied, rules);
    install_recorded_cell_coordinates(&mut fixture);

    let mut pointers = BTreeMap::new();
    let friendly = fixture
        .sim
        .substrate
        .entities
        .get(fixture.actor)
        .unwrap()
        .owner();
    let enemy_id = fixture.id(&inherited["setup"]["candidate"]).unwrap();
    let enemy = fixture
        .sim
        .substrate
        .entities
        .get(enemy_id)
        .unwrap()
        .owner();
    for role in ["source", "e1", "victim", "candidate"] {
        let id = fixture.id(&inherited["setup"][role]).unwrap();
        assert_eq!(actors[role]["pointer"], roles[role]);
        pointers.insert(roles[role].as_str().unwrap().to_owned(), id);
        let entity = fixture.sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            fixture.sim.interner.resolve(entity.type_ref()),
            actors[role]["type_name"]
        );
        let position = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        assert_eq!(
            json!([position.x, position.y, position.z]),
            actors[role]["xyz"]
        );
        assert_eq!(json!(entity.health.current), actors[role]["health"]);
        if actors[role]["limbo"] != 0 {
            assert_eq!(
                role, "candidate",
                "only the inherited enemy was put in Limbo"
            );
            fixture.sim.techno_limbo(id);
        }
    }

    // The shared Foot corpus does not query this additional Cell. Transport
    // its independent pre-history readback; do not infer its level from the
    // actor's resulting Z or substitute a synthetic flat-map height.
    let cell = &initial["mirage_cell"];
    let [rx, ry] = std::array::from_fn(|i| u16::try_from(signed(&cell["cell"][i])).unwrap());
    assert_eq!(
        cell["slope"], 0,
        "represented original standing Cell is flat"
    );
    let level = u8::try_from(signed(&cell["level"])).unwrap();
    let mut terrain = fixture.sim.resolved_terrain.take().unwrap();
    let mut represented =
        crate::sim::world::lifecycle_tests::common_raw_terrain_cell(rx, ry, level, false);
    represented.final_tile_index = signed(&cell["tile_index"]);
    represented.yr_cell_land_type = u8::try_from(signed(&cell["land"])).unwrap();
    represented.land_type = represented.yr_cell_land_type;
    *terrain.cell_mut(rx, ry).unwrap() = represented;
    let identity = terrain.native_cell_identity((rx as i16, ry as i16));
    terrain.write_native_cell_flags(identity, cell["flags"].as_u64().unwrap() as u32);
    fixture.sim.install_resolved_terrain_for_new_map(terrain);

    let frame = initial["constructor_prior"]["frame"].as_u64().unwrap() as u32;
    for (role, category) in [
        ("mirage", EntityCategory::Unit),
        ("enemy_e1", EntityCategory::Infantry),
    ] {
        let before = &actors[role];
        assert_eq!(before["pointer"], roles[role]);
        let id = fixture.sim.allocate_stable_id();
        pointers.insert(roles[role].as_str().unwrap().to_owned(), id);
        let object = rules.object(before["type_name"].as_str().unwrap()).unwrap();
        let [x, y, z] = xyz(&before["xyz"]);
        let owner = if before["house"] == actors["source"]["house"] {
            friendly
        } else {
            assert_eq!(before["house"], actors["candidate"]["house"]);
            enemy
        };
        let type_ref = fixture.sim.interner.intern(&object.id);
        let mut entity = GameEntity::new_at_frame_for_test(
            id,
            (x / 256) as u16,
            (y / 256) as u16,
            if role == "mirage" { level } else { 0 },
            if role == "mirage" {
                (signed(&first["foot_before"]["body_facing_words"][0]) >> 8) as u8
            } else {
                0
            },
            owner,
            Health {
                current: signed(&before["health"]),
            },
            type_ref,
            category,
            0,
            object.sight as u16,
            category == EntityCategory::Unit,
            frame,
        );
        entity.position.sub_x = SimFixed::from_num(x % 256);
        entity.position.sub_y = SimFixed::from_num(y % 256);
        entity.position.exact_z_leptons = Some(z);
        entity.lifecycle.in_limbo = before["limbo"] != 0;
        entity.lifecycle.object_alive = before["alive"] != 0;
        entity.lifecycle.cell_marked = before["limbo"] == 0;
        entity.in_playfield = before["limbo"] == 0;
        entity.locomotor = Some(LocomotorState::from_object_type(object, frame));
        entity.set_body_facing_rot(object.turret_rot);
        if role == "mirage" {
            fixture.actor = id;
            let foot = &first["foot_before"];
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(signed(&foot["mission"])),
                suspended: MissionId::NONE,
                queued: MissionId::from_raw(signed(&foot["queued"])),
                movement_bypass_latch: 0,
                handler_state: signed(&foot["status"]) as u32,
                mission_start_frame: frame,
                ai_counter: signed(&foot["visit"]) as u32,
                dispatch_timer: MissionDispatchTimer::from_raw(
                    signed(&foot["dispatch"][0]),
                    signed(&foot["dispatch"][1]),
                ),
            });
            entity.passive_scan_timer = MissionTimer::armed(
                signed(&foot["targeting_timer"][0]) as u32,
                signed(&foot["targeting_timer"][2]) as u32,
            );
            entity.on_bridge = foot["on_bridge"] != 0;
            entity.body_facing.snap(
                signed(&foot["body_facing_words"][0]) as u16,
                signed(&foot["body_facing_words"][2]) as u32,
            );
            entity.disguise = Some(crate::sim::cloak_disguise::mirage_tests::state_from_native(
                &mut fixture.sim,
                &first["before"],
            ));
            for key in ["archive", "target", "nav", "aux"] {
                assert_eq!(
                    foot[key], "0x0",
                    "the supplied standing Mirage has no {key}"
                );
            }
            assert_eq!(foot["nav_queue"]["count"], 0);
        } else {
            assert_eq!(before["limbo"], 1);
            entity.infantry = Some(InfantryRuntime::new());
        }
        fixture.sim.substrate.entities.insert(entity);
        if before["limbo"] == 0 {
            fixture.sim.substrate.occupancy.add(
                rx,
                ry,
                id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
            fixture
                .sim
                .submit_object_display(id, DisplayLayer::GROUND, Some(rules));
        }
    }
    let native_order = |name: &str| {
        initial["registration"][name]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pointer| pointers[pointer.as_str().unwrap()])
            .collect::<Vec<_>>()
    };
    assert_eq!(
        fixture
            .sim
            .substrate
            .entities
            .values()
            .map(GameEntity::stable_id)
            .collect::<Vec<_>>(),
        native_order("techno"),
        "constructor ordinal transport"
    );
    assert_eq!(
        fixture.sim.display_layers().members(DisplayLayer::GROUND),
        native_order("ground_display"),
        "independent Display order"
    );
    fixture.sim.set_logic_order_for_test(native_order("logic"));
    for (role, before) in actors.as_object().unwrap() {
        let id = pointers[roles[role].as_str().unwrap()];
        let entity = fixture.sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            entity.lifecycle.object_alive,
            before["alive"] != 0,
            "{role}: alive"
        );
        assert_eq!(
            entity.lifecycle.in_limbo,
            before["limbo"] != 0,
            "{role}: Limbo"
        );
        assert_eq!(
            entity.in_logic_vector,
            before["logic_registered"] != 0,
            "{role}: Logic registration"
        );
    }
    fixture.sim.session.binary_frame = first["frame"].as_u64().unwrap() as u32;
    // Setup is a supplied-state boundary. Restore the recorded complete streams
    // after constructor/registration transport; none of their draws is modeled.
    fixture.sim.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
        first["rng_before"]["scenario"].as_str().unwrap(),
    );
    fixture.sim.main_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
        first["rng_before"]["main"].as_str().unwrap(),
    );
    fixture.sim.mapgen_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
        first["rng_before"]["mapgen"].as_str().unwrap(),
    );
    fixture
}

fn assert_whole_unit_state(
    fixture: &super::mission_handlers::foot_mission_oracle_tests::SuppliedFootFixture,
    row: &serde_json::Value,
    boundary: &str,
) {
    use super::mission_handlers::foot_mission_oracle_tests::{assert_foot_projection, signed};
    use serde_json::json;
    let name = row["name"].as_str().unwrap();
    let foot = &row[format!("foot_{boundary}")];
    assert_foot_projection(
        fixture,
        &json!({
            "input": {"name": name},
            "after": foot,
            "rng_after": native_rng_values(&row[format!("rng_{boundary}")]),
        }),
    );
    crate::sim::cloak_disguise::mirage_tests::assert_entity_state(
        &fixture.sim,
        fixture.actor,
        &row[boundary],
        name,
    );
    let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
    assert_eq!(
        json!(actor.health.current),
        row[boundary]["health"],
        "{name}: HP"
    );
    assert_eq!(
        actor.lifecycle.object_alive,
        row[boundary]["alive"] != 0,
        "{name}: alive"
    );
    assert_eq!(
        actor.lifecycle.in_limbo,
        row[boundary]["limbo"] != 0,
        "{name}: Limbo"
    );
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(actor),
        Some(foot["locomotor"]["is_moving_al"] != 0),
        "{name}: Drive IsMoving"
    );
    let loco = actor.locomotor.as_ref().unwrap();
    for (actual, field) in [
        (
            loco.track_destination(crate::sim::movement::track_process::TrackFamily::Drive),
            "destination",
        ),
        (
            loco.track_head(crate::sim::movement::track_process::TrackFamily::Drive),
            "head",
        ),
    ] {
        assert_eq!(
            json!(actual.map_or([0, 0, 0], |coord| [coord.x, coord.y, coord.z])),
            foot["locomotor"][field],
            "{name}: retained Drive {field}"
        );
    }
    assert_eq!(
        actor.body_facing_current(fixture.sim.session.binary_frame),
        signed(&foot["body_facing_words"][0]) as u16,
        "{name}: standing facing"
    );
    assert_eq!(
        json!([
            actor.body_facing.destination(),
            actor.body_facing.start_word(),
            actor.body_facing.timer_duration(),
            actor.body_facing.rot_per_frame(),
        ]),
        json!([
            foot["body_facing_words"][0],
            foot["body_facing_words"][1],
            foot["body_facing_words"][4],
            foot["body_facing_words"][5],
        ]),
        "{name}: represented FacingClass words"
    );
}

/// Original Unit7360C0 -> Foot4DA530 spends the Guard cadence draw at
/// 4D5334 before UpdateDisguise7468C0 chooses its tree at746A50. Compare the
/// native mission timer, selected identity and all three complete RNG streams
/// through the real object-turn host; never pre-consume a jitter in the test.
#[test]
fn native_whole_unit_ai_preserves_foot_then_mirage_rng_order_and_retention() {
    use super::mission_handlers::foot_mission_oracle_tests::{oracle, retail_rules};
    use serde_json::{Value, json};
    let Some(rules) = retail_rules() else {
        return;
    };
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/mirage_disguise.json",
    ))
    .unwrap();
    assert_eq!(native["native_sha256"], oracle()["native_sha256"]);
    assert_eq!(
        native["initialization"]["inherited_inputs"],
        oracle()["inputs"]
    );
    let histories = native["histories"].as_array().unwrap();
    let find = |name| histories.iter().find(|row| row["name"] == name).unwrap();
    let first = find("first_whole_unit_ai");
    let retained = find("retained_whole_unit_ai");
    let range_calls = first["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["name"] == "rng_range")
        .collect::<Vec<_>>();
    assert_eq!(
        range_calls
            .iter()
            .map(|call| call["caller"].clone())
            .collect::<Vec<_>>(),
        vec![json!("0x4d5334"), json!("0x746a50")]
    );
    for call in range_calls {
        assert_eq!(
            call["ecx"],
            native["initialization"]["rng_pointers"]["scenario"]
        );
    }
    assert!(retained["raw_draws"].as_array().unwrap().is_empty());
    assert_eq!(first["after"], retained["before"]);
    assert_eq!(first["foot_after"], retained["foot_before"]);
    let mut fixture = whole_unit_fixture(&native, first, &rules);
    for row in [first, retained] {
        fixture.sim.session.binary_frame = row["frame"].as_u64().unwrap() as u32;
        assert_whole_unit_state(&fixture, row, "before");
        fixture
            .sim
            .advance_live_object_turn(fixture.actor, Some(&rules), ObjectAiCtx::default())
            .unwrap();
        assert_whole_unit_state(&fixture, row, "after");
    }
}
