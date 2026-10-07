//! Actual SHAD Unit741970 -> Foot4D94B0 -> Jumpjet54B1C0, followed by
//! Process54AEC0 through State1's occupied-slot callback and next Translate.
//!
//! The first53 commands of the composed arrival control exercise the Cell
//! order and 52 Process visits used by the causal regression. Expected values
//! come from the current native corpus, including corrected Object CRT startup;
//! the earlier r6 capture is not a frozen byte expectation. This compares the
//! represented motion, navigation, timers, facing and all three RNG streams;
//! it does not compare opaque object bytes or execute a complete object turn.
//! Full ordinary arrival/Stop and retained-handler controls below use the
//! same supplied-world owner and native-derived compact observations. The
//! snapshot control saves a Rust replay already compared against native, then
//! compares the native Scenario Seed(0) handoff and resumed Process calls. It
//! covers Rust snapshot persistence of this supplied world, not a whole native
//! file load or object COM Load sequence.

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::playfield::PlayfieldBounds;
use crate::map::retail_trig::required_math_tables;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::retail_ini_fixture::retail_battle_rules_for_map;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::facing_class::FacingClass;
use crate::sim::movement::ground_pose::{position_world_coord, put_location};
use crate::sim::movement::jumpjet_movement::jumpjet_flight;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::OnceLock;

fn corpus() -> &'static Value {
    static CORPUS: OnceLock<Value> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/jumpjet_states.json",
        ))
        .expect("native Jumpjet corpus");
        assert_eq!(corpus["schema_version"], 2);
        corpus
    })
}

fn control_named(name: &str) -> &'static Value {
    corpus()["composed_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("native composed control {name}"))
}

fn signed(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed integer")).unwrap()
}

fn coordinate(value: &Value) -> DriveCoord {
    DriveCoord {
        x: signed(&value[0]),
        y: signed(&value[1]),
        z: signed(&value[2]),
    }
}

fn cell(value: &Value) -> (u16, u16) {
    (signed(&value[0]) as u16, signed(&value[1]) as u16)
}

fn native_timer(value: &Value) -> CdTimer {
    CdTimer::from_raw(signed(&value[0]), signed(&value[1]))
}

fn timer_fields(timer: CdTimer) -> Value {
    json!([timer.start_frame(), timer.duration()])
}

fn facing_fields(facing: FacingClass) -> Value {
    json!({
        "destination_word": facing.destination(),
        "current_word": facing.start_word(),
        "timer": [
            facing.timer_start_frame().map_or(-1, |frame| frame as i32),
            facing.timer_duration(),
        ],
        "rate_word": facing.rot_per_frame(),
    })
}

fn import_rng(sim: &mut Simulation, native: &Value) {
    sim.main_rng = SimRng::from_native_state_hex_for_test(native["main"].as_str().unwrap());
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(native["scenario"].as_str().unwrap());
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(native["mapgen"].as_str().unwrap());
}

fn assert_rng(sim: &Simulation, native: &Value, label: &str) {
    for (stream, actual) in [
        ("main", &sim.main_rng),
        ("scenario", &sim.scenario_rng),
        ("mapgen", &sim.mapgen_rng),
    ] {
        assert_eq!(
            actual.native_state_hex(),
            native[stream].as_str().unwrap(),
            "{label}: complete {stream} Random2Class",
        );
    }
}

fn assert_stock_type(rules: &RuleSet, native: &Value) {
    let object = rules.object("SHAD").expect("retail SHAD");
    assert_eq!(object.locomotor, LocomotorKind::Jumpjet);
    let params = &object.jumpjet_params;
    let actual = json!({
        "balloon_hover": object.balloon_hover,
        "is_simple_deployer": object.is_simple_deployer,
        "deploy_to_land": object.deploy_to_land,
        "jumpjet": object.jumpjet,
        "hover_attack": object.hover_attack,
        "crashable": object.crashable,
        "tilt_crash_jumpjet": object.tilt_crash_jumpjet,
        "unit_rot": object.turret_rot,
        "speed_type": object.speed_type as i32,
        "movement_zone": object.movement_zone as i32,
        "turn_rate": params.turn_rate,
        "speed": params.speed.to_num::<i32>(),
        "height": params.height,
        "deviation": params.deviation,
        "no_wobbles": params.no_wobbles,
        "climb_bits": format!("0x{:08x}", params.climb.to_bits()),
        "crash_bits": format!("0x{:08x}", params.crash.to_bits()),
        "accel_bits": format!("0x{:08x}", params.accel.to_bits()),
        "wobbles_bits": format!("0x{:08x}", params.wobbles.to_bits()),
    });
    for (field, value) in actual.as_object().unwrap() {
        assert_eq!(value, &native[field], "original SHAD reader field {field}");
    }
}

/// The composed native owner supplies already-live Unit/map/House storage.
/// This is not an Unlimbo, full House constructor or map-load comparison.
fn supplied_world(
    control: &Value,
    rules: &RuleSet,
    terrain_rules: &TerrainRules,
    registry: &OverlayTypeRegistry,
) -> (Simulation, u64, Option<u64>) {
    let input = &control["input"];
    let output = &control["output"];
    let world = &output["input_world"];
    assert_eq!(world["registered_tile_count"], 0);
    assert_eq!(world["land_type_override_source"], "cached_cell_ec");
    let initial = &output["boundaries"][output["steps"][0]["before"].as_u64().unwrap() as usize];
    let state = &initial["state"];
    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap());
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session.binary_frame = initial["frame"].as_u64().unwrap() as u32;
    let allocation = cell(&world["cell_allocation"]);
    super::super::lifecycle_tests::install_common_raw_terrain(
        &mut sim,
        allocation.0,
        allocation.1,
        signed(&world["default_level"]) as u8,
        None,
    );
    // Native supplies no IsoTile registrations or TMPs. Mark's Recalc47D2B0
    // therefore takes the Clear fallback through the selected retail rules.
    crate::map::resolved_terrain::install_no_registered_tiles_test_catalog(
        sim.resolved_terrain.as_mut().unwrap(),
        terrain_rules,
    );
    sim.overlay_grid = Some(OverlayGrid::new(allocation.0, allocation.1));
    let land = signed(&world["default_land_type"]) as u8;
    let semantics = terrain_rules
        .semantics_for_land_type(land)
        .expect("native fixture land has selected retail semantics");
    for y in 0..allocation.1 {
        for x in 0..allocation.0 {
            let terrain = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(x, y)
                .unwrap();
            terrain.slope_type = signed(&world["default_slope"]) as u8;
            terrain.land_type = land;
            terrain.yr_cell_land_type = land;
            terrain.base_land_type = land;
            terrain.base_yr_cell_land_type = land;
            terrain.terrain_class = semantics.terrain_class;
            terrain.base_terrain_class = semantics.terrain_class;
            terrain.speed_costs = semantics.speed_costs;
            terrain.base_speed_costs = semantics.speed_costs;
        }
    }
    for override_row in world["land_type_overrides"].as_array().unwrap() {
        let at = cell(override_row);
        let land = signed(&override_row[2]) as u8;
        let semantics = terrain_rules
            .semantics_for_land_type(land)
            .expect("native override land has selected retail semantics");
        let terrain = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(at.0, at.1)
            .unwrap();
        terrain.land_type = land;
        terrain.yr_cell_land_type = land;
        terrain.terrain_class = semantics.terrain_class;
        terrain.speed_costs = semantics.speed_costs;
        // The native input writes cached Cell+EC only. Its empty tile array
        // still supplies Clear on the next actual Mark/Recalc; this is not a
        // Water TMP or a base-terrain mutation.
    }
    let map = &world["map_f4"];
    sim.playfield_bounds = Some(PlayfieldBounds {
        base: signed(&map[0]),
        off_fc: signed(&map[2]),
        off_100: signed(&map[3]),
        off_104: signed(&map[4]),
        off_108: signed(&map[5]),
    });
    sim.playfield_size_height = Some(signed(&map[1]));
    // The oracle's tracker allocation is an independent supplied extent;
    // Map+F4/+F8 above remains the In_Bounds authority.
    let tracker_extent = cell(&world["tracker_extent"]);
    sim.session.map_width = tracker_extent.0;
    sim.session.map_height = tracker_extent.1;
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, true, 0, 10);
    house.player_control = true;
    sim.houses.insert(owner, house);
    sim.session.house_order = vec![owner];
    let id = sim
        .construct_object_limbo_at_height("SHAD", "Americans", 10, 10, 0, 0, rules)
        .expect("stock SHAD constructor");
    {
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        actor.lifecycle.in_limbo = false;
        actor.health.current = signed(&state["health"]);
        actor.on_bridge = state["on_bridge"].as_bool().unwrap();
        // make_fixture supplies zeroed Unit storage, retaining Foot+6B6=0.
        // All initial raw boundaries in jumpjet_states.raw.json.gz confirm
        // this byte; these supplied-world controls do not execute Unit's
        // constructor. Preserve it before Mark links any ground footprint.
        actor.foot_occupation_enabled = false;
        put_location(&mut actor.position, coordinate(&state["position"]));
        actor.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(signed(&state["mission"])),
            suspended: MissionId::NONE,
            queued: MissionId::from_raw(signed(&state["queued_mission"])),
            movement_bypass_latch: 0,
            handler_state: signed(&state["mission_status"]) as u32,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::from_raw(0, 0),
        });
        actor.navigation.nav_com_aux = state["aux_cell"].as_array().map(|_| {
            let at = cell(&state["aux_cell"]);
            NavTargetRef::cell(at.0, at.1)
        });
        actor.navigation.path_replay.directions = state["path"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| {
                let word = signed(word);
                assert!((-1..=8).contains(&word));
                word as u8
            })
            .collect();
        let reference = cell(&state["reference_cell"]);
        actor.navigation.path_replay.reference_cell =
            Some((reference.0 as i16, reference.1 as i16));
        actor.navigation.path_runtime.movement_timer = native_timer(&state["movement_timer"]);
        actor.navigation.path_runtime.blocked_timer = native_timer(&state["blocked_timer"]);
        actor.navigation.path_runtime.path_blocked = state["blocked"].as_bool().unwrap();
        actor.navigation.path_runtime.retries_left = state["retries"].as_u64().unwrap() as u32;
        actor.body_facing.snap(
            signed(&state["body_facing"]["destination_word"]) as u16,
            sim.session.binary_frame,
        );
        let runtime = actor
            .locomotor
            .as_mut()
            .unwrap()
            .jumpjet_runtime_mut()
            .unwrap();
        let mut flight = runtime.flight();
        flight.facing.snap(
            signed(&state["loco_facing"]["destination_word"]) as u16,
            sim.session.binary_frame,
        );
        *runtime = runtime.clone().with_flight_for_test(flight);
    }
    assert!(sim.foot_mark_put(id, Some(rules), Some(registry)));
    if state["marked"] == 0 {
        assert!(sim.foot_mark_remove(id, Some(rules), Some(registry)));
    } else {
        assert_eq!(state["marked"], 1);
    }
    assert_eq!(state["alive"], 1);
    let other = input["taken"].as_bool().unwrap().then(|| {
        let other = sim
            .construct_object_limbo_at_height("SHAD", "Americans", 12, 10, 0, 0, rules)
            .expect("foreign supplied SHAD identity");
        {
            let foreign = sim.substrate.entities.get_mut(other).unwrap();
            foreign.lifecycle.in_limbo = false;
            foreign.health.current = signed(&state["other"]["health"]);
            foreign.on_bridge = state["other"]["on_bridge"].as_bool().unwrap();
            put_location(
                &mut foreign.position,
                coordinate(&state["other"]["position"]),
            );
        }
        let foreign_slot = cell(&input["foreign_slot"]);
        assert!(
            sim.set_cell_air_slot((foreign_slot.0 as i16, foreign_slot.1 as i16), Some(other),)
        );
        if input["foreign_air_registered"].as_bool().unwrap_or(true) {
            sim.aircraft_tracker_add(other);
        }
        other
    });
    // Native seeds its three streams after supplying this world, before the
    // measured order. Fixture construction is outside the comparison.
    import_rng(&mut sim, &state["rng"]);
    (sim, id, other)
}

fn assert_motion_boundary(sim: &Simulation, id: u64, control: &Value, boundary: &Value) {
    let label = format!(
        "{}: {}",
        control["name"].as_str().unwrap(),
        boundary["label"].as_str().unwrap()
    );
    let state = &boundary["state"];
    let actor = sim.substrate.entities.get(id).unwrap();
    let runtime = actor.locomotor.as_ref().unwrap().jumpjet_runtime().unwrap();
    // Keep the class result first: the original State1 callback publishes
    // destination/NavCom/timers before this Process returns, not next frame.
    let destination = runtime.destination();
    assert_eq!(
        json!([destination.x, destination.y, destination.z]),
        state["destination"],
        "{label}: retained class destination",
    );
    let nav_cell = match actor.navigation.nav_com {
        Some(NavTargetRef::Cell { rx, ry }) => json!([rx as i16, ry as i16]),
        None => Value::Null,
        unexpected => panic!("{label}: unexpected NavCom {unexpected:?}"),
    };
    assert_eq!(nav_cell, state["nav_cell"], "{label}: live NavCom");
    assert_eq!(
        timer_fields(actor.navigation.path_runtime.movement_timer),
        state["movement_timer"],
        "{label}: accepted movement timer",
    );
    assert_eq!(
        timer_fields(actor.navigation.path_runtime.blocked_timer),
        state["blocked_timer"],
        "{label}: accepted blocked timer",
    );
    let position = position_world_coord(&actor.position);
    assert_eq!(
        json!([position.x, position.y, position.z]),
        state["position"],
        "{label}: XYZ"
    );
    assert_eq!(runtime.phase(), signed(&state["phase"]), "{label}: phase");
    assert_eq!(
        runtime.moving(),
        signed(&state["moving"]) != 0,
        "{label}: moving"
    );
    assert_eq!(
        runtime.landing_latched(),
        state["landing_latched"].as_bool().unwrap(),
        "{label}: landing latch"
    );
    assert_eq!(
        actor.lifecycle.cell_marked,
        signed(&state["marked"]) != 0,
        "{label}: marked"
    );
    assert_eq!(
        actor.lifecycle.object_alive,
        signed(&state["alive"]) != 0,
        "{label}: alive"
    );
    assert_eq!(
        actor.health.current,
        signed(&state["health"]),
        "{label}: health"
    );
    assert_eq!(
        actor.on_bridge,
        state["on_bridge"].as_bool().unwrap(),
        "{label}: OnBridge"
    );
    assert_eq!(
        actor.mission.current().raw(),
        signed(&state["mission"]),
        "{label}: Mission"
    );
    assert_eq!(
        actor.mission.queued().raw(),
        signed(&state["queued_mission"]),
        "{label}: queued Mission"
    );
    assert_eq!(
        actor.mission.handler_state(),
        signed(&state["mission_status"]) as u32,
        "{label}: Mission status"
    );
    let path = &actor.navigation.path_replay;
    let backing_words: Vec<i32> = path
        .directions
        .iter()
        .skip(usize::from(path.cursor))
        .take(4)
        .map(|&word| i32::from(word as i8))
        .collect();
    assert_eq!(
        json!(backing_words),
        state["path"],
        "{label}: retained path backing"
    );
    let reference = path.reference_cell.unwrap_or((0, 0));
    assert_eq!(
        json!([reference.0, reference.1]),
        state["reference_cell"],
        "{label}: reference Cell"
    );
    assert_eq!(
        actor.navigation.path_runtime.path_blocked,
        state["blocked"].as_bool().unwrap(),
        "{label}: blocked"
    );
    assert_eq!(
        actor.navigation.path_runtime.retries_left,
        state["retries"].as_u64().unwrap() as u32,
        "{label}: retries"
    );
    let aux_cell = match actor.navigation.nav_com_aux {
        Some(NavTargetRef::Cell { rx, ry }) => json!([rx as i16, ry as i16]),
        None => Value::Null,
        unknown => panic!("{label}: unexpected Aux {unknown:?}"),
    };
    assert_eq!(aux_cell, state["aux_cell"], "{label}: Aux Cell identity");
    assert!(
        state["nav_queue"].as_array().unwrap().is_empty(),
        "{label}: selected native prefix has no NavQueue"
    );
    assert!(
        actor.navigation.nav_queue.is_empty(),
        "{label}: no invented NavQueue"
    );
    let flight = runtime.flight();
    assert_eq!(
        format!("{:016x}", flight.current_speed_bits),
        state["current_speed_bits"].as_str().unwrap(),
        "{label}: current speed"
    );
    assert_eq!(
        format!("{:016x}", flight.target_speed_bits),
        state["target_speed_bits"].as_str().unwrap(),
        "{label}: target speed"
    );
    assert_eq!(
        flight.target_height,
        signed(&state["target_height"]),
        "{label}: target height"
    );
    assert_eq!(
        format!("{:016x}", flight.bob_phase_bits),
        state["bob_phase_bits"].as_str().unwrap(),
        "{label}: bob phase"
    );
    assert_eq!(
        facing_fields(actor.body_facing),
        state["body_facing"],
        "{label}: body facing"
    );
    assert_eq!(
        facing_fields(flight.facing),
        state["loco_facing"],
        "{label}: locomotor facing"
    );
    assert_rng(sim, &state["rng"], &label);
}

fn owner_name(id: u64, actor: u64, foreign: Option<u64>) -> &'static str {
    if id == actor {
        "actor"
    } else if Some(id) == foreign {
        "foreign"
    } else {
        panic!("object {id} is outside the supplied native world")
    }
}

fn assert_world_boundary(
    sim: &Simulation,
    actor: u64,
    foreign: Option<u64>,
    control: &Value,
    boundary: &Value,
) {
    let state = &boundary["state"];
    let label = format!(
        "{}: {}",
        control["name"].as_str().unwrap(),
        boundary["label"].as_str().unwrap()
    );
    assert!(
        state.get("tarcom_owner").is_some(),
        "{label}: native TarCom observation"
    );
    for (id, tracker_key, slot_key) in [
        (Some(actor), "tracker_cell", "slot_cell"),
        (foreign, "other_tracker_cell", "other_slot_cell"),
    ] {
        if let Some(id) = id {
            let entity = sim.substrate.entities.get(id).unwrap();
            let tracker = entity.air_tracker_cell();
            let slot = entity.air_slot_cell();
            assert_eq!(
                json!([tracker.0, tracker.1]),
                state[tracker_key],
                "{label}: {tracker_key}"
            );
            assert_eq!(
                json!([slot.0, slot.1]),
                state[slot_key],
                "{label}: {slot_key}"
            );
        }
    }
    if let Some(foreign) = foreign {
        let entity = sim.substrate.entities.get(foreign).unwrap();
        let native = &state["other"];
        let position = position_world_coord(&entity.position);
        assert_eq!(
            json!([position.x, position.y, position.z]),
            native["position"],
            "{label}: foreign XYZ"
        );
        assert_eq!(
            entity.health.current,
            signed(&native["health"]),
            "{label}: foreign health"
        );
        assert_eq!(
            entity.lifecycle.cell_marked,
            signed(&native["marked"]) != 0,
            "{label}: foreign marked"
        );
        assert_eq!(
            entity.lifecycle.object_alive,
            signed(&native["alive"]) != 0,
            "{label}: foreign alive"
        );
        assert_eq!(
            entity.on_bridge,
            native["on_bridge"].as_bool().unwrap(),
            "{label}: foreign OnBridge"
        );
    }
    let actual_target = sim
        .substrate
        .entities
        .get(actor)
        .unwrap()
        .attack_target
        .as_ref()
        .map_or(Value::Null, |target| match target.target {
            TargetKind::Entity(id) => json!(owner_name(id, actor, foreign)),
            unknown => panic!("{label}: unexpected target {unknown:?}"),
        });
    assert_eq!(
        actual_target, state["tarcom_owner"],
        "{label}: TarCom identity"
    );
    // These fields are the shared tracker's retained membership and insertion
    // order; the test does not infer membership from pose or altitude.
    let mut buckets: BTreeMap<u16, Vec<(u64, &str)>> = BTreeMap::new();
    for (id, entity) in sim.substrate.entities.iter_sorted() {
        if let Some(bucket) = entity.air_spatial_bucket() {
            buckets.entry(bucket).or_default().push((
                entity.air_spatial_enter_order(),
                owner_name(id, actor, foreign),
            ));
        }
    }
    let actual_air: Vec<Value> = buckets
        .into_iter()
        .map(|(index, mut members)| {
            members.sort_by_key(|member| member.0);
            let owners: Vec<&str> = members.into_iter().map(|member| member.1).collect();
            json!({"index": index, "owners": owners})
        })
        .collect();
    let expected_air: Vec<Value> = state["air"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bucket| json!({"index": bucket["index"], "owners": bucket["owners"]}))
        .collect();
    assert_eq!(actual_air, expected_air, "{label}: ordered tracker vectors");

    let allocation = cell(&control["output"]["input_world"]["cell_allocation"]);
    let native_cells: BTreeMap<(u16, u16), &Value> = state["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|observation| (cell(&observation["cell"]), observation))
        .collect();
    for y in 0..allocation.1 {
        for x in 0..allocation.0 {
            let native = native_cells.get(&(x, y));
            let held = sim.substrate.air_slots.holder(x, y);
            let actual_slot = held.map_or(Value::Null, |id| json!(owner_name(id, actor, foreign)));
            let expected_slot = native.map_or(Value::Null, |native| native["slot_owner"].clone());
            assert_eq!(actual_slot, expected_slot, "{label}: Cell{x},{y} slot");
            for (layer, raw_key, members_key, actual_raw) in [
                (
                    MovementLayer::Ground,
                    "ground_raw",
                    "ground_owners",
                    sim.substrate.raw_cell_occupation.ground_bits(x, y),
                ),
                (
                    MovementLayer::Bridge,
                    "bridge_raw",
                    "bridge_owners",
                    sim.substrate.raw_cell_occupation.deck_bits(x, y),
                ),
            ] {
                let expected_raw = native.map_or(0, |native| native[raw_key].as_u64().unwrap());
                // All represented bits of these supplied Cell+124/+128
                // dwords fit the existing authoritative raw-byte owner.
                assert_eq!(
                    u64::from(actual_raw),
                    expected_raw,
                    "{label}: Cell{x},{y} {layer:?} raw occupation"
                );
                let actual_members: Vec<&str> = sim
                    .substrate
                    .occupancy
                    .get(x, y)
                    .map_or_else(Vec::new, |members| members.snapshot_layer(layer))
                    .into_iter()
                    .map(|id| owner_name(id, actor, foreign))
                    .collect();
                let expected_members =
                    native.map_or_else(|| json!([]), |native| native[members_key].clone());
                assert_eq!(
                    json!(actual_members),
                    expected_members,
                    "{label}: Cell{x},{y} {layer:?} list order"
                );
            }
        }
    }
}

/// Replay the caller selected by the native fixture. Supplemental writes are
/// declared inputs between calls, never inferred from the following state.
fn execute_step(
    sim: &mut Simulation,
    actor: u64,
    foreign: Option<u64>,
    control: &Value,
    step: &Value,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
) {
    let label = step["label"].as_str().unwrap();
    match label {
        "cell_order" => {
            let at = cell(&control["output"]["input_world"]["initial_cell_order"]);
            let _ = sim.set_unit_destination(actor, NavTargetRef::cell(at.0, at.1), rules, true);
        }
        "unit_stop" => {
            let _ = sim.set_unit_null_destination(actor, Some(rules), Some(registry));
        }
        "scenario_load_seed0" => {
            // Scenario Load689470 -> PostRead683560 reseeds Scenario+218.
            // The composed native control invokes original Seed65C6D0(0)
            // at that declared handoff; it does not load the whole game.
            sim.scenario_rng = SimRng::new(0);
        }
        "retained_request_move_to" => {
            let args = &step["args"];
            let request = DriveCoord {
                x: signed(&args[1]),
                y: signed(&args[2]),
                z: signed(&args[3]),
            };
            sim.jumpjet_move_to(actor, request, Some(rules)).unwrap();
        }
        "retained_handler_inputs" => {
            let inputs = &step["supplied_inputs"];
            let entity = sim.substrate.entities.get_mut(actor).unwrap();
            entity.navigation.nav_com = inputs["nav_cell"].as_array().map(|_| {
                let at = cell(&inputs["nav_cell"]);
                NavTargetRef::cell(at.0, at.1)
            });
            entity.attack_target = match inputs["tarcom_owner"].as_str() {
                None => None,
                Some("foreign") => Some(AttackTarget {
                    target: TargetKind::Entity(foreign.unwrap()),
                }),
                unknown => panic!("undeclared target input {unknown:?}"),
            };
            let runtime = entity
                .locomotor
                .as_mut()
                .unwrap()
                .jumpjet_runtime_mut()
                .unwrap();
            let mut flight = runtime.flight();
            flight.target_height = signed(&inputs["target_height"]);
            *runtime = runtime
                .clone()
                .with_phase_for_test(signed(&inputs["phase"]))
                .with_flight_for_test(flight);
        }
        "register_air" => sim.aircraft_tracker_add(actor),
        "state1_decision" | "state3_decision" | "state4_decision" => {
            let frame = sim.session.binary_frame;
            let (trig, _) = required_math_tables();
            sim.with_jumpjet_process(actor, |runtime, sim| {
                let mut host = super::CruiseHost {
                    sim,
                    frame,
                    trig,
                    rules: Some(rules),
                    registry: Some(registry),
                    stable_id: actor,
                    touched_down: false,
                    impact: false,
                    crash_latched: false,
                };
                let phase = match label {
                    "state1_decision" => jumpjet_flight::state1_ascend(runtime, &mut host),
                    "state3_decision" => jumpjet_flight::state3_translate(runtime, &mut host),
                    "state4_decision" => jumpjet_flight::state4_descend(runtime, &mut host),
                    _ => unreachable!(),
                };
                *runtime = runtime.clone().with_phase_for_test(phase);
            })
            .unwrap();
        }
        process if process.starts_with("process_") => {
            sim.tick_jumpjet_cruise_one(actor, Some(rules), Some(registry))
                .unwrap();
        }
        unknown => panic!("unrepresented native command {unknown}"),
    }
}

fn replay_steps(
    sim: &mut Simulation,
    actor: u64,
    foreign: Option<u64>,
    control: &Value,
    steps: &[Value],
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
) {
    let boundaries = control["output"]["boundaries"].as_array().unwrap();
    for step in steps {
        let before = &boundaries[step["before"].as_u64().unwrap() as usize];
        let after = &boundaries[step["after"].as_u64().unwrap() as usize];
        sim.session.binary_frame = before["frame"].as_u64().unwrap() as u32;
        assert_motion_boundary(sim, actor, control, before);
        assert_world_boundary(sim, actor, foreign, control, before);
        execute_step(sim, actor, foreign, control, step, rules, registry);
        assert_motion_boundary(sim, actor, control, after);
        assert_world_boundary(sim, actor, foreign, control, after);
    }
}

#[test]
fn stock_shad_state1_scatter_publishes_before_following_translate() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let (trig, _) = required_math_tables();
    assert!(trig.matches_retail());
    let control = control_named("stock_taken_state1_arrival");
    let output = &control["output"];
    assert_stock_type(&retail.rules, &output["input_final"]);
    let terrain_rules = TerrainRules::from_ini(&retail.processed_rules);
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let (mut sim, id, _) = supplied_world(control, &retail.rules, &terrain_rules, &registry);
    let steps = output["steps"].as_array().unwrap();
    let boundaries = output["boundaries"].as_array().unwrap();
    assert!(
        steps.len() >= 53,
        "the Cell order and 52 Process visits are recorded"
    );
    let initial_cell = cell(&output["input_world"]["initial_cell_order"]);
    let mut callback = None;
    for (index, step) in steps.iter().take(53).enumerate() {
        let before = &boundaries[step["before"].as_u64().unwrap() as usize];
        let after = &boundaries[step["after"].as_u64().unwrap() as usize];
        sim.session.binary_frame = before["frame"].as_u64().unwrap() as u32;
        assert_motion_boundary(&sim, id, control, before);
        if index == 0 {
            assert_eq!(step["label"], "cell_order");
            // Native Unit setter EAX is incidental; compare its state effects,
            // not a made-up class-setter boolean ABI.
            let _ = sim.set_unit_destination(
                id,
                NavTargetRef::cell(initial_cell.0, initial_cell.1),
                &retail.rules,
                true,
            );
        } else {
            assert!(
                sim.tick_jumpjet_cruise_one(id, Some(&retail.rules), Some(&registry))
                    .is_some()
            );
        }
        assert_motion_boundary(&sim, id, control, after);
        if step["label"] == "process_50" {
            callback = Some(after);
        }
    }
    let callback = callback.expect("native State1 occupied-slot callback boundary");
    assert_eq!(callback["frame"], 151);
    assert_eq!(steps[52]["label"], "process_51");
    assert_eq!(
        steps[52]["draw_end"].as_u64().unwrap() - steps[0]["draw_start"].as_u64().unwrap(),
        1
    );
    let actor = sim.substrate.entities.get(id).unwrap();
    // These independent native cached cells are checked after the motion
    // prefix, so a delayed class callback fails before cache-owner migration.
    let final_state = &boundaries[steps[52]["after"].as_u64().unwrap() as usize]["state"];
    let tracker = actor.air_tracker_cell();
    let slot = actor.air_slot_cell();
    assert_eq!(json!([tracker.0, tracker.1]), final_state["tracker_cell"]);
    assert_eq!(json!([slot.0, slot.1]), final_state["slot_cell"]);
    assert_ne!(final_state["tracker_cell"], final_state["slot_cell"]);
}

#[test]
fn stock_shad_arrival_free_slot_and_stop_match_complete_native_histories() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let terrain_rules = TerrainRules::from_ini(&retail.processed_rules);
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    for name in [
        "stock_taken_state1_arrival",
        "stock_free_state1_translate",
        "stock_taken_state1_stop",
    ] {
        let control = control_named(name);
        assert_stock_type(&retail.rules, &control["output"]["input_final"]);
        let (mut sim, actor, foreign) =
            supplied_world(control, &retail.rules, &terrain_rules, &registry);
        let steps = control["output"]["steps"].as_array().unwrap();
        replay_steps(
            &mut sim,
            actor,
            foreign,
            control,
            steps,
            &retail.rules,
            &registry,
        );
    }
}

/// These histories supply retained state inputs and invoke the actual leaf
/// handler before two complete Process calls. They do not demonstrate that a
/// full scenario/object turn produces each retained prestate.
#[test]
fn retained_shad_state1_state3_and_water_landing_match_native_callbacks() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let terrain_rules = TerrainRules::from_ini(&retail.processed_rules);
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    for name in [
        "state1_taken_unmarked",
        "state1_free_unmarked",
        "state1_taken_same_nav",
        "state3_live_new_water",
        "state3_live_new_clear",
        "state4_water_taken",
        "state4_water_free",
    ] {
        let control = control_named(name);
        assert_stock_type(&retail.rules, &control["output"]["input_final"]);
        let (mut sim, actor, foreign) =
            supplied_world(control, &retail.rules, &terrain_rules, &registry);
        let steps = control["output"]["steps"].as_array().unwrap();
        replay_steps(
            &mut sim,
            actor,
            foreign,
            control,
            steps,
            &retail.rules,
            &registry,
        );
    }
}

#[test]
fn native_scatter_result_survives_snapshot_and_continues_through_touchdown() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let terrain_rules = TerrainRules::from_ini(&retail.processed_rules);
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let control = control_named("stock_taken_state1_arrival_scenario_seed0");
    assert_stock_type(&retail.rules, &control["output"]["input_final"]);
    let (mut sim, actor, foreign) =
        supplied_world(control, &retail.rules, &terrain_rules, &registry);
    let steps = control["output"]["steps"].as_array().unwrap();
    let checkpoint = steps
        .iter()
        .position(|step| step["label"] == "scenario_load_seed0")
        .unwrap();
    assert_eq!(steps[checkpoint - 1]["label"], "process_50");
    assert_eq!(steps.len() - checkpoint - 1, 59);
    replay_steps(
        &mut sim,
        actor,
        foreign,
        control,
        &steps[..checkpoint],
        &retail.rules,
        &registry,
    );
    // Save the reached seed31/draw1 state before the native load reset. No
    // native after-state is installed as a snapshot fixture input.
    let saved_rng = sim.rng_state();
    let saved_hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "SHAD native supplied world", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.retain_in_scenario_process_state_from(&sim);
    let native_seed0_hex =
        crate::test_fixture::text("tests/fixtures/rng/mapgen_seed0_native_0x3f4.hex")
            .split_whitespace()
            .collect::<String>()
            .to_ascii_lowercase();
    let seed0 = SimRng::from_native_state_hex_for_test(&native_seed0_hex);
    let loaded_rng = restored.rng_state();
    assert_ne!(saved_rng.scenario, seed0.logical_state());
    assert_eq!(
        loaded_rng.scenario,
        seed0.logical_state(),
        "native Scenario Load discards the saved seed31/draw1 cursor"
    );
    assert_eq!(
        loaded_rng.main, saved_rng.main,
        "in-scenario load retains the live Main object"
    );
    assert_eq!(
        loaded_rng.mapgen, saved_rng.mapgen,
        "in-scenario load retains the live MapGen object"
    );
    let reset = &steps[checkpoint];
    let before = &control["output"]["boundaries"][reset["before"].as_u64().unwrap() as usize];
    let at = &control["output"]["boundaries"][reset["after"].as_u64().unwrap() as usize];
    assert_motion_boundary(&sim, actor, control, before);
    assert_world_boundary(&sim, actor, foreign, control, before);
    execute_step(
        &mut sim,
        actor,
        foreign,
        control,
        reset,
        &retail.rules,
        &registry,
    );
    assert_motion_boundary(&sim, actor, control, at);
    assert_world_boundary(&sim, actor, foreign, control, at);
    assert_rng(&restored, &at["state"]["rng"], "native loaded RNG handoff");
    // Only the reference Scenario RNG changes here. Equal complete hashes
    // after restoration therefore also protect every other hashed owner.
    let hash = sim.state_hash();
    assert_ne!(saved_hash, hash, "Scenario reset changes the world hash");
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        sim.terrain_speed_config.clone(),
        &retail.rules,
    );
    assert_eq!(
        restored.state_hash(),
        hash,
        "native-reached snapshot state/hash after Scenario Seed(0)"
    );
    assert_motion_boundary(&restored, actor, control, at);
    assert_world_boundary(&restored, actor, foreign, control, at);
    replay_steps(
        &mut sim,
        actor,
        foreign,
        control,
        &steps[checkpoint + 1..],
        &retail.rules,
        &registry,
    );
    replay_steps(
        &mut restored,
        actor,
        foreign,
        control,
        &steps[checkpoint + 1..],
        &retail.rules,
        &registry,
    );
    assert_eq!(
        restored.state_hash(),
        sim.state_hash(),
        "snapshot continuation hash"
    );
}
