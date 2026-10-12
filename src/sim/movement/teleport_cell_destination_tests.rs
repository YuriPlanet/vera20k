//! Original already-live CLEG Cell requests: Infantry51AA40 -> Foot4D94B0 ->
//! Teleport718100/718B70 -> Cell481180, raw5217C0/521850, Ready521B60,
//! Commence5B3570, StopDriver51DAF0 and DoAction51D6F0.
//!
//! The native fixture supplies the prior actor/cell/House state. Production
//! ordered retail readers supply CLEG's type and fixed ART records. Every
//! command boundary compares retained state and all three complete RNGs;
//! there is no supplied gameplay return or second selection implementation.
//! Fourteen supplied Unit-object destinations, two warp-in refusal tails and
//! four TimerCheck boundaries extend the comparison below. Full Infantry/House
//! construction, warp Process, native radio/piggyback/save-load execution are
//! outside this corpus; separate ports own those prerequisites. Persistence
//! and production command tests are Rust integration regressions.

use crate::map::entities::EntityCategory;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::retail_ini_fixture::retail_battle_rules_for_map;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::RawCellKey;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::stage::StageClass;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;
use serde_json::{Value, json};

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_teleport_destination.json",
    ))
    .unwrap()
}

fn signed(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: signed(&value[0]),
        y: signed(&value[1]),
        z: signed(&value[2]),
    }
}

fn nullable_coord(value: &Value) -> Option<DriveCoord> {
    let point = coord(value);
    (point != DriveCoord { x: 0, y: 0, z: 0 }).then_some(point)
}

fn pair(value: &Value) -> (u16, u16) {
    (signed(&value[0]) as u16, signed(&value[1]) as u16)
}

fn timer(value: &Value) -> CdTimer {
    CdTimer::from_raw(signed(&value[0]), signed(&value[1]))
}

fn row_named<'a>(data: &'a Value, name: &str) -> &'a Value {
    data["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == name)
        .unwrap()
}

/// A supplied already-live command fixture, not a native Infantry Unlimbo or
/// whole-map load comparison. The native owner allocates all32x32 cells with
/// empty lists, level0/slope0/land0, and the exact playfield fields below.
fn fixture(row: &Value, rules: &RuleSet) -> (Simulation, u64, InternedId) {
    let before = &row["before"];
    let input = &row["input"];
    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap());
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session.binary_frame = input["frame"].as_u64().unwrap() as u32;
    sim.session.game_options.game_speed = signed(&input["game_speed"]);
    sim.session.game_mode_nonzero = row["human_input"]["game_mode"].as_u64().unwrap() != 0;
    sim.session.map_width = 32;
    sim.session.map_height = 32;
    sim.session.map_name = "CLEG supplied command cells".into();
    sim.playfield_bounds = Some(PlayfieldBounds {
        base: 16,
        off_fc: -16,
        off_100: -16,
        off_104: 64,
        off_108: 64,
    });
    // Executed FNPC56DC20 reads Map+F4=16 and retained Map+F8=0,
    // giving radius cap16. The allocated32x32 cell table is independent.
    sim.playfield_size_height = Some(0);
    let mut cells: Vec<_> = (0..32)
        .flat_map(|y| {
            (0..32).map(move |x| {
                let mut cell = crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false);
                // Original fixture installs nine land0 speed entries of1.0 at
                //89EA40; this is supplied terrain, independently of retail INIs.
                cell.speed_costs = crate::rules::terrain_rules::SpeedCostProfile {
                    foot: Some(100),
                    track: Some(100),
                    wheel: Some(100),
                    float: Some(100),
                    amphibious: Some(100),
                    float_beach: Some(100),
                    hover: Some(100),
                };
                cell.base_speed_costs = cell.speed_costs;
                cell
            })
        })
        .collect();
    // Overlays as the map loader leaves them: the overlay cell and the bridge facts.
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(32, 32);
    for native in before["cells"].as_array().unwrap() {
        let (x, y) = pair(&native["coord"]);
        let cell = &mut cells[usize::from(y) * 32 + usize::from(x)];
        cell.level = native["level"].as_u64().unwrap() as u8;
        cell.slope_type = native["slope"].as_u64().unwrap() as u8;
        if let Some(overlay) = native.get("overlay_id") {
            cell.bridge_facts.overlay_id = overlay.as_i64().and_then(|id| u8::try_from(id).ok());
            overlays.cell_mut(x, y).overlay_id = cell.bridge_facts.overlay_id;
        }
        cell.bridge_facts.raw_flags = native["structural_flags"].as_u64().unwrap() as u32;
        assert_eq!(native["land"], 0, "fixture has only native land0");
    }
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(32, 32, cells));
    sim.overlay_grid = Some(overlays);
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, input["human"].as_bool().unwrap(), 0, 10);
    house.player_control = row["human_input"]["player_control_byte"].as_u64().unwrap() != 0;
    sim.houses.insert(owner, house);
    sim.session.house_order = vec![owner];
    assert_eq!(row["human_input"]["house_index"], 0);
    assert_eq!(row["human_input"]["player_control_byte"], 0);
    assert_eq!(row["human_input"]["original_predicate_al"], 1);
    let id = sim
        .construct_object_limbo_at_height("CLEG", "Americans", 10, 10, 0, 0, rules)
        .unwrap();
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    assert_eq!(actor.category, EntityCategory::Infantry);
    assert_eq!(
        actor.locomotor.as_ref().unwrap().active_kind(),
        LocomotorKind::Teleport
    );
    actor.lifecycle.in_limbo = false;
    actor.lifecycle.cell_marked = true;
    super::super::ground_pose::put_location(&mut actor.position, coord(&before["actor"]["coords"]));
    actor.on_bridge = signed(&before["actor"]["on_bridge"]) != 0;
    actor.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(signed(&before["actor"]["mission"])),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(signed(&before["actor"]["queued_mission"])),
        movement_bypass_latch: 0,
        handler_state: before["actor"]["mission_status"].as_u64().unwrap() as u32,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::from_raw(
            signed(&before["actor"]["mission_timer"][0]),
            signed(&before["actor"]["mission_timer"][1]),
        ),
    });
    actor
        .mission_leaf
        .set_infantry_doing_verified(signed(&before["actor"]["doing"]))
        .unwrap();
    let infantry = actor.infantry.as_mut().unwrap();
    infantry.is_prone = signed(&before["actor"]["prone"]) != 0;
    infantry.cell_entry_blocked = signed(&before["actor"]["entry_blocked"]) != 0;
    let stage = &before["actor"]["stage"];
    actor.install_native_stage_fixture(StageClass::from_native_fixture(
        signed(&stage[0]),
        signed(&stage[1]) as u8,
        CdTimer::from_raw(signed(&stage[2]), signed(&stage[3])),
        signed(&stage[4]),
        signed(&stage[5]),
    ));
    //123 is the corpus's opaque retained pointer token, never dereferenced by
    //the selected caller. Preserve identity through early-return controls.
    let aux = before["actor"]["aux"].as_u64().unwrap();
    actor.navigation.nav_com_aux = (aux != 0).then_some(NavTargetRef::Object { id: aux });
    actor.navigation.nav_com = (!before["actor"]["navcell"].is_null()).then(|| {
        let (rx, ry) = pair(&before["actor"]["navcell"]);
        NavTargetRef::cell(rx, ry)
    });
    actor.navigation.path_replay.directions = before["actor"]["path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| signed(word) as u8)
        .collect();
    actor.navigation.path_replay.cursor = 0;
    actor.navigation.path_replay.reference_cell = Some((
        signed(&before["actor"]["reference"][0]) as i16,
        signed(&before["actor"]["reference"][1]) as i16,
    ));
    actor.navigation.path_runtime.movement_timer = timer(&before["actor"]["movement_timer"]);
    actor.navigation.path_runtime.blocked_timer = timer(&before["actor"]["blocked_timer"]);
    actor.navigation.path_runtime.retries_left =
        before["actor"]["retries"].as_u64().unwrap() as u32;
    //Raw prestate follows the actual original5217C0 initial mark. Native Cell
    //object lists are deliberately empty, independently of these raw bytes.
    for native in before["cells"].as_array().unwrap() {
        let (x, y) = pair(&native["coord"]);
        for (plane, layer) in [(0, MovementLayer::Ground), (1, MovementLayer::Bridge)] {
            let raw = native["raw"][plane].as_u64().unwrap() as u8;
            let index = native["owners"][plane].as_u64().unwrap();
            assert!(index == 0 || index == u64::from(u32::MAX));
            assert!(raw != 0 || index == u64::from(u32::MAX));
            sim.substrate.raw_cell_occupation.write_occupant(
                RawCellKey::Real(x, y),
                layer,
                raw,
                (index == 0).then_some(owner),
                true,
            );
        }
    }
    if let Some(transport) = input.get("transport") {
        let target = sim
            .construct_object_limbo_at_height("FV", "Americans", 12, 10, 0, 0, rules)
            .unwrap();
        let supplied = coord(&transport["coords"]);
        let vehicle = sim.substrate.entities.get_mut(target).unwrap();
        vehicle.lifecycle.in_limbo = false;
        vehicle.lifecycle.cell_marked = true;
        super::super::ground_pose::put_location(&mut vehicle.position, supplied);
        vehicle.health.current = signed(&transport["health"]);
        vehicle.on_bridge = transport
            .get("on_bridge")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let layer = if vehicle.on_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let cell = (vehicle.position.rx, vehicle.position.ry);
        if transport["cell_content"].as_bool().unwrap() {
            sim.substrate.occupancy.add(
                cell.0,
                cell.1,
                target,
                layer,
                None,
                crate::sim::occupancy::CellListInsertion::from_category(EntityCategory::Unit),
            );
        }
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .nav_com = Some(NavTargetRef::object(target));
        if input.get("chrono_timer").and_then(Value::as_bool) == Some(true) {
            // The native TimerCheck fixture supplies a nonnull TarCom+2B4,
            // deliberately excluding passive-acquisition/idle callbacks.
            assert_ne!(before["actor"]["target"], "0x0");
            sim.substrate.entities.get_mut(id).unwrap().attack_target =
                Some(crate::sim::combat::AttackTarget::new(target));
        }
        assert_eq!((cell.0, cell.1), pair(&row["transport_input"]["mapcoords"]));
        assert_eq!(
            sim.foot_navigation_coordinate(target).unwrap(),
            coord(&row["transport_input"]["navcoords"])
        );
        assert_eq!(
            row["transport_input"]["rtti"], 1,
            "original Unit target receiver"
        );
    }
    import_rng(&mut sim, &row["rng_before"]);
    (sim, id, owner)
}

fn import_rng(sim: &mut Simulation, native: &Value) {
    sim.main_rng = SimRng::from_native_state_hex_for_test(native["main"].as_str().unwrap()).into();
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(native["scenario"].as_str().unwrap());
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(native["mapgen"].as_str().unwrap());
}

fn assert_rng(sim: &Simulation, native: &Value, context: &str) {
    for (name, rng) in [
        ("main", sim.main_rng.native_state_hex()),
        ("scenario", sim.scenario_rng.native_state_hex()),
        ("mapgen", sim.mapgen_rng.native_state_hex()),
    ] {
        assert_eq!(
            rng,
            native[name].as_str().unwrap(),
            "{context}: full {name} RNG"
        );
    }
}

fn assert_runtime(sim: &Simulation, id: u64, native: &Value, context: &str) {
    let loco = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap();
    let runtime = loco.teleport_runtime().unwrap();
    assert_eq!(
        runtime.warp.as_ref().and_then(|warp| warp.destination),
        nullable_coord(&native["armed_xyz"]),
        "{context}: armed+1C"
    );
    assert_eq!(
        runtime.resolved,
        nullable_coord(&native["resolved_xyz"]),
        "{context}: resolved+28"
    );
    assert_eq!(
        runtime.requested,
        signed(&native["request_byte"]) != 0,
        "{context}: request+34"
    );
    assert_eq!(
        loco.is_powered(),
        signed(&native["powered"]) != 0,
        "{context}: powered"
    );
    //Neither independent COM reference bookkeeping nor the dormant secondary
    //byte has a Rust field. These invariant native receipts are not mirrored.
    assert_eq!(native["reference_count"], 1);
    assert_eq!(native["secondary_byte"], 0);
}

fn assert_state(sim: &Simulation, id: u64, owner: InternedId, native: &Value, context: &str) {
    let actor = sim.substrate.entities.get(id).unwrap();
    let expected = &native["actor"];
    assert_eq!(
        super::super::ground_pose::position_world_coord(&actor.position),
        coord(&expected["coords"]),
        "{context}: physical Location"
    );
    assert_eq!(
        actor.on_bridge,
        signed(&expected["on_bridge"]) != 0,
        "{context}: OnBridge"
    );
    assert_eq!(
        actor.mission.current().raw(),
        signed(&expected["mission"]),
        "{context}: current mission"
    );
    assert_eq!(
        actor.mission.queued().raw(),
        signed(&expected["queued_mission"]),
        "{context}: queued mission"
    );
    assert_eq!(
        actor.mission.handler_state(),
        expected["mission_status"].as_u64().unwrap() as u32,
        "{context}: handler state"
    );
    let dispatch = actor.mission.dispatch_timer();
    assert_eq!(
        json!([dispatch.start_frame(), dispatch.delay()]),
        expected["mission_timer"],
        "{context}: Mission timer"
    );
    assert_eq!(
        actor.mission_leaf.as_infantry().unwrap().doing(),
        signed(&expected["doing"]),
        "{context}: Doing"
    );
    assert_eq!(
        actor.infantry.unwrap().is_prone,
        signed(&expected["prone"]) != 0,
        "{context}: prone"
    );
    assert_eq!(
        actor.infantry.unwrap().cell_entry_blocked,
        signed(&expected["entry_blocked"]) != 0,
        "{context}: entry blocked"
    );
    let stage = actor.native_stage();
    let serialized = serde_json::to_value(stage).unwrap();
    assert_eq!(
        json!([
            stage.value(),
            serialized["changed"],
            stage.timer().start_frame(),
            stage.timer().duration(),
            stage.rate(),
            serialized["increment"]
        ]),
        expected["stage"],
        "{context}: retained Stage"
    );
    let expected_nav = (!expected["navcell"].is_null()).then(|| {
        let (rx, ry) = pair(&expected["navcell"]);
        NavTargetRef::cell(rx, ry)
    });
    assert_eq!(
        actor.navigation.nav_com, expected_nav,
        "{context}: NavCom receiver identity"
    );
    let aux = expected["aux"].as_u64().unwrap();
    assert_eq!(
        actor.navigation.nav_com_aux,
        (aux != 0).then_some(NavTargetRef::Object { id: aux }),
        "{context}: retained auxiliary identity"
    );
    assert_eq!(
        actor.navigation.path_replay.cursor, 0,
        "{context}: no Process consumes this queue"
    );
    let path: Vec<_> = actor
        .navigation
        .path_replay
        .directions
        .iter()
        .map(|&word| if word == u8::MAX { -1 } else { i32::from(word) })
        .collect();
    assert_eq!(
        json!(path),
        expected["path"],
        "{context}: retained path head/suffix"
    );
    let reference = actor.navigation.path_replay.reference_cell.unwrap();
    assert_eq!(
        json!([reference.0, reference.1]),
        expected["reference"],
        "{context}: Foot reference cell"
    );
    let path = actor.navigation.path_runtime;
    assert_eq!(
        json!([
            path.movement_timer.start_frame(),
            path.movement_timer.duration()
        ]),
        expected["movement_timer"],
        "{context}: movement timer"
    );
    assert_eq!(
        json!([
            path.blocked_timer.start_frame(),
            path.blocked_timer.duration()
        ]),
        expected["blocked_timer"],
        "{context}: blocked timer"
    );
    assert_eq!(
        u64::from(path.retries_left),
        expected["retries"].as_u64().unwrap(),
        "{context}: retained retries"
    );
    assert_runtime(sim, id, &native["locomotor"], context);
    for cell in native["cells"].as_array().unwrap() {
        let (x, y) = pair(&cell["coord"]);
        for (plane, layer) in [(0, MovementLayer::Ground), (1, MovementLayer::Bridge)] {
            let key = RawCellKey::Real(x, y);
            assert_eq!(
                u64::from(sim.substrate.raw_cell_occupation.bits_at(key, layer)),
                cell["raw"][plane].as_u64().unwrap(),
                "{context}: raw {x},{y} {layer:?}"
            );
            let native_owner = cell["owners"][plane].as_u64().unwrap();
            assert!(native_owner == 0 || native_owner == u64::from(u32::MAX));
            assert_eq!(
                sim.substrate.raw_cell_occupation.owner_at(key, layer),
                (native_owner == 0).then_some(owner),
                "{context}: raw owner {x},{y} {layer:?}"
            );
        }
    }
}

fn execute(
    sim: &mut Simulation,
    id: u64,
    command: &Value,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
) {
    match command["kind"].as_str().unwrap() {
        "set_destination" => {
            assert_eq!(command["flag"], 1);
            let (rx, ry) = pair(&command["cell"]);
            //Original class EAX is register residue (e.g.100), not a Bool
            //contract. Compare the owned state/query/fullRNG below instead.
            let _ = sim
                .set_infantry_destination(
                    id,
                    NavTargetRef::cell(rx, ry),
                    rules,
                    registry,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
        }
        "stop_moving" => sim
            .locomotor_stop_moving(
                id,
                Some(rules),
                registry,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap(),
        "queue_mission" => {
            sim.mission_queue_exact(
                id,
                MissionId::from_raw(signed(&command["mission"])),
                signed(&command["start"]),
                sim.session.binary_frame,
                &EntityReadyInputProvider,
            )
            .unwrap();
        }
        other => panic!("unsupported native command {other}"),
    }
}

/// The native live command fixture supplies Rules+1768=22 separately from
/// its real CLEG type/ART reads. Keep that input explicit; retail uses60.
fn command_fixture_rules() -> Option<RuleSet> {
    let mut rules = retail_battle_rules_for_map("XMP03T4.MAP")?.rules;
    rules.general.blockage_path_delay_ticks = 22;
    Some(rules)
}

/// Executed original718B70 transport controls; excludes passenger radio,
/// attachment and Process. MoveTo's timer/arming writes are not resolver goldens.
#[test]
fn object_enter_resolution_matches_original_transport_controls() {
    let Some(rules) = command_fixture_rules() else {
        return;
    };
    let data = corpus();
    let rows = data["transport_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 14);
    for row in rows {
        let name = row["input"]["name"].as_str().unwrap();
        let (mut sim, id, _) = fixture(row, &rules);
        let input = coord(&row["input"]["commands"][0]["coords"]);
        let expected = nullable_coord(&row["after"]["locomotor"]["resolved_xyz"]);
        let accepted = sim
            .teleport_move_to(
                id,
                input,
                &rules,
                false,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        assert_eq!(
            accepted,
            row["boundaries"][0]["eax"] == 1,
            "{name}: resolver acceptance"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .teleport_runtime()
                .unwrap()
                .resolved_destination(),
            expected,
            "{name}: resolved XYZ"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .mission
                .current()
                .raw(),
            signed(&row["after"]["actor"]["mission"]),
            "{name}: Commence before placement"
        );
        assert_rng(&sim, &row["rng_after"], name);
    }
}

/// Compose the original refusal tail with the separately executed Unit0F
/// receiver. Native rows begin after a declared refusal, not at whole PerCell.
/// Compare RNG, navigation and path timers; absolute warp-timer lifetime is
/// outside these two same-frame controls.
#[test]
fn full_transport_arrival_replays_original_warp_in_refusal_tail() {
    let Some(rules) = command_fixture_rules() else {
        return;
    };
    let data = corpus();
    let rows = data["refused_enter_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let name = row["input"]["name"].as_str().unwrap();
        let (mut sim, id, _) = fixture(row, &rules);
        let transport = match sim.substrate.entities.get(id).unwrap().navigation.nav_com {
            Some(NavTargetRef::Object { id }) => id,
            other => panic!("{name}: missing transport {other:?}"),
        };
        let occupant = sim
            .construct_object_limbo_at_height("E1", "Americans", 12, 10, 0, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(transport)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .unwrap()
            .board_forced(occupant, rules.object("E1").unwrap().size);
        crate::sim::passenger::begin_boarding(
            sim.substrate.entities.get_mut(id).unwrap(),
            transport,
        );
        let runtime = sim
            .substrate
            .entities
            .get_mut(id)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .teleport_runtime_mut()
            .unwrap();
        runtime.warp = Some(super::TeleportState {
            phase: super::TeleportPhase::ChronoDelay,
            destination: nullable_coord(&row["before"]["locomotor"]["armed_xyz"]),
            delay_timer: crate::sim::timer::CdTimer::started(
                signed(&row["input"]["teleport_timer"][0]),
                signed(&row["input"]["teleport_timer"][2]),
            ),
        });
        // Fixture construction is not part of the executed native tail.
        import_rng(&mut sim, &row["rng_before"]);
        sim.per_cell_process(
            id,
            super::super::PerCellReason::Arrival,
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        assert_state(
            &sim,
            id,
            sim.interner.get("Americans").unwrap(),
            &row["after"],
            name,
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .mission
                .queued()
                .raw(),
            signed(&row["after"]["actor"]["queued_mission"]),
            "{name}: queued Guard"
        );
        assert!(
            sim.substrate.entities.get(id).unwrap().is_warping_in(),
            "{name}: warp-in retained"
        );
        assert!(
            matches!(
                sim.substrate.entities.get(id).unwrap().passenger_role,
                crate::sim::passenger::PassengerRole::None
            ),
            "{name}: rejected passenger stays outside"
        );
        assert_rng(&sim, &row["rng_after"], name);
    }
}

/// Actual719BF0 controls compare before/at/after expiry and frame wrap. The
/// native fixture retains a target, excluding passive-acquisition callbacks.
#[test]
fn chrono_epoch_expiry_matches_original_timer_check() {
    let Some(rules) = command_fixture_rules() else {
        return;
    };
    let data = corpus();
    let rows = data["chrono_timer_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    for row in rows {
        let name = row["input"]["name"].as_str().unwrap();
        let (mut sim, id, _) = fixture(row, &rules);
        let timer = &row["before"]["locomotor"]["timer"];
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .teleport_runtime_mut()
            .unwrap()
            .warp = Some(super::TeleportState {
            phase: super::TeleportPhase::ChronoDelay,
            destination: None,
            delay_timer: crate::sim::timer::CdTimer::from_raw(signed(&timer[0]), signed(&timer[2])),
        });
        sim.process_teleport(id, Some(&rules), crate::sim::world::ObjectAiCtx::default());
        assert_eq!(
            sim.substrate.entities.get(id).unwrap().is_warping_in(),
            row["after"]["actor"]["warping_in"].as_bool().unwrap(),
            "{name}: warp-in flag"
        );
        assert_rng(&sim, &row["rng_after"], name);
    }
}

#[test]
fn retail_cleg_type_and_fixed_art_match_the_original_readers() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let data = corpus();
    let cleg = retail.rules.object("CLEG").unwrap();
    let native = &data["type_inputs"]["fields"];
    assert_eq!(cleg.locomotor, LocomotorKind::Teleport);
    assert_eq!(cleg.jumpjet, signed(&native["jumpjet"]) != 0);
    assert_eq!(
        i32::from(cleg.movement_zone as i8),
        signed(&native["movement_zone"])
    );
    assert_eq!(cleg.speed_type as i32, signed(&native["speed_type"]));
    assert_eq!(
        i64::from(cleg.strength),
        native["strength"].as_i64().unwrap()
    );
    assert_eq!(cleg.image, native["image"].as_str().unwrap());
    assert_eq!(cleg.crawls, signed(&native["crawls"]) != 0);
    assert_eq!(cleg.fraidycat, signed(&native["fraidycat"]) != 0);
    assert_eq!(cleg.deployer, signed(&native["deployer"]) != 0);
    assert_eq!(
        json!([
            cleg.fire_up_frame,
            cleg.fire_prone_frame,
            cleg.secondary_fire_frame,
            cleg.secondary_prone_frame
        ]),
        native["fire_frames"]
    );
    let bank = retail.rules.animation_sequence("CLEG").unwrap();
    let records = data["records"].as_array().unwrap();
    assert_eq!(records.len(), 42);
    for (action, native) in records.iter().enumerate() {
        let actual = bank.infantry_action(action as i32).unwrap();
        assert_eq!(
            json!([
                actual.start_frame,
                actual.frames_per_facing,
                actual.facings,
                actual.facing_hint.map_or(-1, |hint| hint as i32)
            ]),
            json!([native[0], native[1], native[2], native[3]]),
            "original523D00 action{action}"
        );
        assert!(
            native.as_array().unwrap()[4..].iter().all(|word| word == 0),
            "unmodeled native sequence auxiliary/sound records remain zero"
        );
    }
}

#[test]
fn cleg_cell_requests_match_every_original_command_boundary() {
    let Some(rules) = command_fixture_rules() else {
        return;
    };
    let data = corpus();
    assert_eq!(
        data["native_identity"]["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let rows = data["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 22);
    let mut compared = 0;
    for row in rows {
        assert_eq!(row["code_unchanged"], true);
        let (mut sim, id, owner) = fixture(row, &rules);
        let name = row["input"]["name"].as_str().unwrap();
        assert_state(&sim, id, owner, &row["before"], name);
        assert_rng(&sim, &row["rng_before"], name);
        for (index, boundary) in row["boundaries"].as_array().unwrap().iter().enumerate() {
            let context = format!("{name} command{index} {}", boundary["input"]);
            assert_state(&sim, id, owner, &boundary["before"], &context);
            assert_rng(&sim, &boundary["rng_before"], &context);
            execute(&mut sim, id, &boundary["input"], &rules, None);
            assert_state(&sim, id, owner, &boundary["after"], &context);
            assert_rng(&sim, &boundary["rng_after"], &context);
            assert_eq!(
                super::super::motion_query::is_moving(sim.substrate.entities.get(id).unwrap()),
                Some(boundary["query_al"].as_u64().unwrap() != 0),
                "{context}: original718080"
            );
            compared += 1;
        }
        assert_state(&sim, id, owner, &row["after"], name);
        assert_rng(&sim, &row["rng_after"], name);
    }
    assert_eq!(compared, 46);
}

/// Rust instance lifecycle regression using the actual native-produced first
/// Move runtime. The original command corpus does not execute BEGIN/END.
#[test]
fn stop_does_not_touch_a_stashed_native_cleg_request() {
    let Some(rules) = command_fixture_rules() else {
        return;
    };
    let data = corpus();
    let row = row_named(&data, "guard_move_stop_move");
    let (mut sim, id, owner) = fixture(row, &rules);
    let boundaries = row["boundaries"].as_array().unwrap();
    execute(&mut sim, id, &boundaries[0]["input"], &rules, None);
    assert_state(
        &sim,
        id,
        owner,
        &boundaries[0]["after"],
        "accepted native runtime",
    );
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    let loco = entity.locomotor.as_mut().unwrap();
    let retained = loco.teleport_runtime().unwrap().clone();
    assert!(loco.begin_piggyback(LocomotorKind::Walk, sim.session.binary_frame));
    assert_eq!(
        super::super::motion_query::is_moving(sim.substrate.entities.get(id).unwrap()),
        Some(false)
    );
    let raw = bincode::serialize(&sim.substrate.raw_cell_occupation).unwrap();
    execute(&mut sim, id, &boundaries[1]["input"], &rules, None);
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    assert_eq!(
        *entity
            .locomotor
            .as_mut()
            .unwrap()
            .teleport_instance_for_test_mut()
            .unwrap(),
        retained
    );
    assert!(super::super::locomotor_owner::end_admitted_piggyback(
        entity
    ));
    assert_eq!(
        entity
            .locomotor
            .as_ref()
            .unwrap()
            .teleport_runtime()
            .unwrap(),
        &retained
    );
    assert_eq!(super::super::motion_query::is_moving(entity), Some(true));
    execute(&mut sim, id, &boundaries[1]["input"], &rules, None);
    assert_runtime(
        &sim,
        id,
        &boundaries[1]["after"]["locomotor"],
        "restored active Stop",
    );
    assert_eq!(
        bincode::serialize(&sim.substrate.raw_cell_occupation).unwrap(),
        raw,
        "neither Stop releases the retained reservation"
    );
    assert_rng(&sim, &boundaries[1]["rng_after"], "active/stashed Stop");
}

/// Rust snapshot regression. Actual native-produced retained+28 survives
/// save/load active or stashed. Scenario load's established Seed0 policy is
/// applied on both sides; this does not claim native save/load parity.
#[test]
fn native_cleg_reservation_survives_snapshot_and_is_hashed() {
    let Some(rules) = command_fixture_rules() else {
        return;
    };
    let data = corpus();
    let row = row_named(&data, "guard_move_stop_move");
    let boundaries = row["boundaries"].as_array().unwrap();
    for stashed in [false, true] {
        let (mut sim, id, owner) = fixture(row, &rules);
        execute(&mut sim, id, &boundaries[0]["input"], &rules, None);
        execute(&mut sim, id, &boundaries[1]["input"], &rules, None);
        assert_state(
            &sim,
            id,
            owner,
            &boundaries[1]["after"],
            "native snapshot input",
        );
        if stashed {
            assert!(
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .begin_piggyback(LocomotorKind::Walk, sim.session.binary_frame)
            );
        }
        sim.scenario_rng = SimRng::new(0);
        let hash = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "CLEG supplied command cells", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(
            sim.resolved_terrain.as_ref().unwrap().clone(),
            sim.terrain_speed_config.clone(),
            &rules,
        );
        assert_eq!(
            restored.state_hash(),
            hash,
            "active={}: snapshot state/hash",
            !stashed
        );
        assert_eq!(
            bincode::serialize(&restored.substrate.raw_cell_occupation).unwrap(),
            bincode::serialize(&sim.substrate.raw_cell_occupation).unwrap()
        );
        let expected = sim
            .substrate
            .entities
            .get_mut(id)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .teleport_instance_for_test_mut()
            .unwrap()
            .clone();
        let actual = restored
            .substrate
            .entities
            .get_mut(id)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .teleport_instance_for_test_mut()
            .unwrap();
        assert_eq!(*actual, expected);
        //A legal stopped instance retains its resolver reservation. Holding
        //armed/request/physical/RNG fixed while changing +28 must change hash.
        actual.resolved = None;
        assert_ne!(
            restored.state_hash(),
            hash,
            "retained resolver is authoritative even when stashed"
        );
        restored
            .substrate
            .entities
            .get_mut(id)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .teleport_instance_for_test_mut()
            .unwrap()
            .resolved = expected.resolved;
        assert_eq!(restored.state_hash(), hash, "exact resolver restoration");
        for world in [&mut sim, &mut restored] {
            if stashed {
                assert!(super::super::locomotor_owner::end_admitted_piggyback(
                    world.substrate.entities.get_mut(id).unwrap()
                ));
            }
            execute(world, id, &boundaries[2]["input"], &rules, None);
        }
        assert_eq!(
            sim.state_hash(),
            restored.state_hash(),
            "same second request after restore"
        );
        assert_eq!(
            sim.rng_state(),
            restored.rng_state(),
            "full three-stream continuation"
        );
    }
}

/// Ordinary command integration with the same retail type and supplied cells.
/// This is a Rust dispatch check, not a native EventClass command comparison.
#[test]
fn two_ordinary_cleg_moves_publish_class_destinations_before_process() {
    let Some(rules) = command_fixture_rules() else {
        return;
    };
    let data = corpus();
    let row = row_named(&data, "guard_two_distinct_cells");
    let (mut sim, id, _) = fixture(row, &rules);
    let physical = super::super::ground_pose::position_world_coord(
        &sim.substrate.entities.get(id).unwrap().position,
    );
    for target_rx in [12, 13] {
        assert!(sim.apply_command_with_overlays(
            "Americans",
            &Command::Move {
                entity_id: id,
                target_rx,
                target_ry: 10,
                queue: false
            },
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default()
        ));
        let entity = sim.substrate.entities.get(id).unwrap();
        let runtime = entity
            .locomotor
            .as_ref()
            .unwrap()
            .teleport_runtime()
            .unwrap();
        assert!(runtime.requested);
        assert_eq!(runtime.warp.as_ref().unwrap().destination, runtime.resolved);
        let selected = runtime.resolved.unwrap();
        assert_eq!(
            (selected.x / 256, selected.y / 256),
            (i32::from(target_rx), 10)
        );
        assert_eq!(super::super::motion_query::is_moving(entity), Some(true));
        assert_eq!(
            super::super::ground_pose::position_world_coord(&entity.position),
            physical
        );
    }
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(12, 10) & 0x1c,
        0,
        "second command releases first reservation synchronously"
    );
    assert_ne!(
        sim.substrate.raw_cell_occupation.ground_bits(13, 10) & 0x1c,
        0
    );
}

/// An absent caller dependency is a Rust transaction error, not native
///51BF90 admission. Preflight must refuse before Foot/Teleport writes.
#[test]
fn overlay_destination_without_registered_inputs_is_atomic() {
    use crate::rules::ini_parser::IniFile;

    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let ore = registry.id_for_name("GEM01").unwrap();
    assert!(registry.flags(ore).unwrap().tiberium);
    let mut rules = retail.rules;
    rules.general.blockage_path_delay_ticks = 22;
    let empty = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);
    let data = corpus();
    let row = row_named(&data, "guard_two_distinct_cells");
    for missing in [None, Some(&empty)] {
        let (mut sim, id, _) = fixture(row, &rules);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(12, 10, ore, 0);
        let before = (
            bincode::serialize(sim.substrate.entities.get(id).unwrap()).unwrap(),
            bincode::serialize(&sim.substrate.raw_cell_occupation).unwrap(),
            sim.rng_state(),
        );
        let target = NavTargetRef::cell(12, 10);
        assert!(
            !sim.infantry_destination_inputs_available(id, target, &rules, missing),
            "Teleport Cell admission requires registered overlay inputs before mutation"
        );
        assert!(
            sim.set_infantry_destination(
                id,
                target,
                &rules,
                missing,
                crate::sim::world::FrameEffects::default()
            )
            .is_err()
        );
        assert_eq!(
            (
                bincode::serialize(sim.substrate.entities.get(id).unwrap()).unwrap(),
                bincode::serialize(&sim.substrate.raw_cell_occupation).unwrap(),
                sim.rng_state(),
            ),
            before,
            "missing or incomplete registry leaves NavCom, timers, reservations and RNG intact"
        );
    }
}

/// A broken retained terrain input is a Rust context error. Once resolution
/// has released raw occupation, it must take the existing refusal cleanup
/// and Foot timer tail instead of leaking a half-published reservation.
#[test]
fn teleport_admission_input_error_restores_physical_occupation() {
    let Some(rules) = command_fixture_rules() else {
        return;
    };
    let data = corpus();
    let row = row_named(&data, "guard_two_distinct_cells");
    let (mut sim, id, _) = fixture(row, &rules);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(12, 10)
        .unwrap()
        .speed_costs = crate::rules::terrain_rules::SpeedCostProfile::default();
    let raw = bincode::serialize(&sim.substrate.raw_cell_occupation).unwrap();
    assert!(
        !sim.set_infantry_destination(
            id,
            NavTargetRef::cell(12, 10),
            &rules,
            None,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    let actor = sim.substrate.entities.get(id).unwrap();
    let runtime = actor
        .locomotor
        .as_ref()
        .unwrap()
        .teleport_runtime()
        .unwrap();
    assert_eq!(runtime.resolved_destination(), None);
    assert_eq!(super::super::motion_query::is_moving(actor), Some(false));
    assert_eq!(actor.navigation.nav_com, None);
    assert_eq!(
        bincode::serialize(&sim.substrate.raw_cell_occupation).unwrap(),
        raw
    );
    let after = &row["boundaries"][0]["after"]["actor"];
    assert_eq!(
        actor.navigation.path_runtime.movement_timer,
        timer(&after["movement_timer"])
    );
    assert_eq!(
        actor.navigation.path_runtime.blocked_timer,
        timer(&after["blocked_timer"])
    );
    assert_rng(
        &sim,
        &row["boundaries"][0]["rng_after"],
        "selection precedes context error",
    );
}

/// Original overlays execute through the same class owner and the ordinary
/// command caller. Whole-reader inputs and every native boundary are pinned;
/// this still excludes warp Process and other overlay families.
#[test]
fn cleg_overlay_cell_requests_match_every_original_command_boundary() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let mut rules = retail.rules;
    rules.general.blockage_path_delay_ticks = 22;
    let data = corpus();
    for (name, native) in data["overlay_inputs"]["fields"].as_object().unwrap() {
        let id = registry.id_for_name(name).unwrap();
        let flags = registry.flags(id).unwrap();
        assert_eq!(u64::from(id), native["index"].as_u64().unwrap());
        for (key, actual) in [
            ("wall", flags.wall),
            ("tiberium", flags.tiberium),
            ("crate", flags.crate_type),
            ("crushable", flags.crushable),
            ("no_use_tile_land", flags.no_use_tile_land_type),
        ] {
            assert_eq!(
                actual,
                signed(&native[key]) != 0,
                "{name}: original {key} reader"
            );
        }
        assert_eq!(
            u64::from(flags.damage_levels),
            native["damage_levels"].as_u64().unwrap()
        );
        assert_eq!(i32::from(flags.land.as_index()), signed(&native["land"]));
    }
    let native_weapon = &data["overlay_inputs"]["weapon"];
    let weapon = rules
        .weapon(rules.object("CLEG").unwrap().primary().unwrap())
        .unwrap();
    assert_eq!(weapon.id, native_weapon["name"].as_str().unwrap());
    assert_eq!(weapon.damage, signed(&native_weapon["damage"]));
    assert_eq!(
        weapon.ambient_damage,
        signed(&native_weapon["ambient_damage"])
    );
    assert_eq!(weapon.warhead.as_deref(), native_weapon["warhead"].as_str());
    assert_eq!(
        rules
            .warhead(weapon.warhead.as_deref().unwrap())
            .unwrap()
            .wall,
        signed(&native_weapon["warhead_wall"]) != 0,
    );
    let rows = data["overlay_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let mut direct_compared = 0;
    let mut command_compared = 0;
    for row in rows {
        assert_eq!(row["code_unchanged"], true);
        // Event4C73B9 queues Move with flag0 before calling the class setter;
        // Ready/Commence inside718B70 promotes it. The ordinary comparison
        // starts from that recorded queued-Move context. Requeuing the same
        // selector keeps it, and after promotion the second request's Queue
        // is a no-op (5B3601..5B3612). Guard controls retain direct coverage.
        let callers: &[bool] = match signed(&row["before"]["actor"]["queued_mission"]) {
            2 => &[false, true],
            -1 => &[false],
            other => panic!("unrepresented overlay caller queued mission {other}"),
        };
        for &ordinary_command in callers {
            let (mut sim, id, owner) = fixture(row, &rules);
            let name = row["input"]["name"].as_str().unwrap();
            assert_state(&sim, id, owner, &row["before"], name);
            assert_rng(&sim, &row["rng_before"], name);
            for (index, boundary) in row["boundaries"].as_array().unwrap().iter().enumerate() {
                let context = format!("{name} command{index} ordinary={ordinary_command}");
                assert_state(&sim, id, owner, &boundary["before"], &context);
                assert_rng(&sim, &boundary["rng_before"], &context);
                assert_eq!(boundary["input"]["kind"], "set_destination");
                if ordinary_command {
                    let (rx, ry) = pair(&boundary["input"]["cell"]);
                    assert_eq!(
                        sim.apply_command_with_overlays(
                            "Americans",
                            &Command::Move {
                                entity_id: id,
                                target_rx: rx,
                                target_ry: ry,
                                queue: false
                            },
                            Some(&rules),
                            Some(&registry),
                            crate::sim::world::FrameEffects::default(),
                        ),
                        boundary["query_al"].as_u64().unwrap() != 0,
                        "{context}: request acceptance"
                    );
                } else {
                    execute(&mut sim, id, &boundary["input"], &rules, Some(&registry));
                }
                assert_state(&sim, id, owner, &boundary["after"], &context);
                assert_rng(&sim, &boundary["rng_after"], &context);
                assert_eq!(
                    super::super::motion_query::is_moving(sim.substrate.entities.get(id).unwrap()),
                    Some(boundary["query_al"].as_u64().unwrap() != 0),
                    "{context}: original718080",
                );
                if ordinary_command {
                    command_compared += 1;
                } else {
                    direct_compared += 1;
                }
            }
        }
    }
    assert_eq!(
        (direct_compared, command_compared),
        (8, 4),
        "all eight class boundaries and four matching ordinary-Move boundaries"
    );
}
