//! Complete Drive storage at CMIN constructor/Move/BEGIN/END boundaries.
//! Native receipts: tools/spatial_oracle/cmin_dock.{py,json,md}, Drive4AF540,
//! Unit741970/7425F8, BEGIN4AF8E0, Stop4AFE00, gate4AF970 and END4AF930.
//! The existing CMIN scene supplies the same production setter and pad route.
//! Only represented fields are compared: constructor padding, opaque words,
//! COM counts and no-free allocation bytes remain native evidence. Fractions
//! are compared after the existing SimFixed conversion, including Stop's 0.3
//! precision difference. These are not native save/load or Ship witnesses.

use super::{Scene, cmin_scene, coord, corpus};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, FootPathQueue, NavTargetRef, TrackProgress};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::slope_transition::SlopeTransitionState;
use crate::sim::movement::track_process::TrackFamily;
use crate::sim::movement::{self, DriveLocomotionRuntime, ground_pose, locomotor_owner};
use crate::sim::timer::CdTimer;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

fn integer(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed integer")).unwrap()
}

fn byte(value: &Value) -> u8 {
    u8::try_from(value.as_u64().expect("native byte")).unwrap()
}

fn pointer(value: &Value) -> u32 {
    u32::from_str_radix(value.as_str().unwrap().strip_prefix("0x").unwrap(), 16).unwrap()
}

fn fraction(bits: &Value) -> SimFixed {
    let bits = u64::from_str_radix(bits.as_str().unwrap(), 16).unwrap();
    let native = f64::from_bits(bits);
    assert!(native.is_finite());
    SimFixed::from_num(native)
}

fn nonnull_coord(value: &Value) -> Option<DriveCoord> {
    let point = coord(value);
    (point.x != 0 || point.y != 0 || point.z != 0).then_some(point)
}

fn control<'a>(data: &'a Value, name: &str) -> &'a Value {
    data["instance_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == name)
        .expect("executed instance history")
}

fn boundary<'a>(row: &'a Value, step: &Value, phase: &str) -> &'a Value {
    &row["boundaries"][usize::try_from(step[phase].as_u64().unwrap()).unwrap()]
}

fn step_calls<'a>(row: &'a Value, step: &Value) -> &'a [Value] {
    let start = usize::try_from(step["call_start"].as_u64().unwrap()).unwrap();
    let end = usize::try_from(step["call_end"].as_u64().unwrap()).unwrap();
    &row["native_calls"].as_array().unwrap()[start..end]
}

/// Decode the existing fixture's Cell pointer, not a second coordinate port.
fn native_nav(row: &Value, value: &Value) -> Option<NavTargetRef> {
    let address = pointer(value);
    if address == 0 {
        return None;
    }
    let base = pointer(&row["cell_baseline"]["address"]);
    let offset = address.checked_sub(base).expect("mapped native Cell");
    assert_eq!(offset % 0x200, 0, "native Cell allocation stride");
    let index = offset / 0x200;
    assert!(index < 32 * 32, "native fixture has 32x32 live Cells");
    Some(NavTargetRef::cell(
        u16::try_from(index % 32).unwrap(),
        u16::try_from(index / 32).unwrap(),
    ))
}

fn path(value: &Value) -> Vec<u8> {
    value
        .as_array()
        .unwrap()
        .iter()
        .take_while(|word| integer(word) != -1)
        .map(byte)
        .collect()
}

fn raw_word(native: &Value, offset: usize) -> i32 {
    let raw = native["raw_hex"].as_str().unwrap();
    i32::from_le_bytes(std::array::from_fn(|index| {
        let start = (offset + index) * 2;
        u8::from_str_radix(&raw[start..start + 2], 16).unwrap()
    }))
}

fn slope(native: &Value) -> SlopeTransitionState {
    // Constructor4AF54D/4AF550 and Process4B0523..4B0533 access full
    // dwords: current at complete-object+1C, previous at +20.
    let previous = raw_word(native, 0x20);
    let current = raw_word(native, 0x1C);
    assert_eq!(native["slopes"], serde_json::json!([previous, current]));
    // +28 is unused. The owner represents the equal +2C/+30 duration/total.
    assert_eq!(native["timer_words"][2], native["interpolation_total"]);
    SlopeTransitionState::from_fields_for_test(
        u8::try_from(previous).expect("representable native previous slope"),
        u8::try_from(current).expect("representable native current slope"),
        integer(&native["timer_words"][0]),
        byte(&native["interpolation_total"]),
    )
}

fn retained(native: &Value) -> DriveLocomotionRuntime {
    DriveLocomotionRuntime::default()
        .with_destination_for_test(nonnull_coord(&native["destination"]))
        .with_head_to_for_test(nonnull_coord(&native["head"]))
        .with_track_for_test(TrackProgress::for_test(
            integer(&native["selector"]),
            integer(&native["cursor"]),
            byte(&native["flags_60_67"][0]) != 0,
            integer(&native["residual"]),
        ))
        .with_track_valid_for_test(byte(&native["flags_60_67"][3]) != 0)
        .with_turn_latched_for_test(byte(&native["flags_60_67"][2]) != 0)
        .with_end_permitted_for_test(byte(&native["flags_60_67"][5]) != 0)
        .with_target_speed_fraction_for_test(fraction(&native["target_speed_bits"]))
}

fn supply_retained(entity: &mut GameEntity, native: &Value) {
    let loco = entity.locomotor.as_mut().unwrap();
    *loco.active_slope_transition_mut().unwrap() = slope(native);
    assert!(loco.install_drive_state_for_test(Some(retained(native))));
}

fn assert_drive(loco: &LocomotorState, native: &Value, context: &str) {
    let complete = loco
        .selected_drive_runtime()
        .expect("complete active Drive");
    assert_eq!(
        complete.slope().hash_fields(),
        slope(native).hash_fields(),
        "{context}: slopes and constructor/retained frame"
    );
    // A lazy None is a constructor-default retained payload, not an absent class.
    assert_eq!(
        complete.retained().cloned().unwrap_or_default(),
        retained(native),
        "{context}: every represented retained field"
    );
    assert_eq!(
        loco.is_powered(),
        byte(&native["power"][0]) != 0,
        "{context}: base power"
    );
}

fn prepare(row: &Value) -> Scene {
    let first = boundary(row, &row["steps"][0], "before");
    let native = &first["state"]["foot"];
    let mut scene = cmin_scene(&row["input"]);
    // The inherited track_destination::make_destination_fixture supplies
    // Rules+1768 = 22, not the constructor default 60. Feed that declared
    // input through the same [AI] reader; no after-state sets a rule value.
    let supplied_rules =
        RuleSet::from_ini(&IniFile::from_str("[AI]\nBlockagePathDelay=22\n")).unwrap();
    scene.rules.general.blockage_path_delay_ticks =
        supplied_rules.general.blockage_path_delay_ticks;
    scene
        .sim
        .session
        .set_binary_frame_for_test(u32::try_from(first["frame"].as_u64().unwrap()).unwrap());
    let entity = scene.sim.substrate.entities.get_mut(scene.miner).unwrap();
    // dress_cmin already owns list placement and XY. Publish the measured Z too.
    entity.position.put_location(coord(&native["physical"]));
    entity.set_on_bridge(byte(&native["on_bridge"]) != 0);
    entity
        .foot_speed
        .set_speed_fraction(fraction(&native["applied_speed_bits"]));
    entity.navigation.path_replay = FootPathQueue::for_test(
        path(&native["path"]),
        0,
        Some((
            i16::try_from(integer(&native["reference_cell"][0])).unwrap(),
            i16::try_from(integer(&native["reference_cell"][1])).unwrap(),
        )),
    );
    let runtime = &mut entity.navigation.path_runtime;
    runtime.set_movement_timer_for_test(CdTimer::from_raw(
        integer(&native["movement_timer"][0]),
        integer(&native["movement_timer"][2]),
    ));
    runtime.set_blocked_timer_for_test(CdTimer::from_raw(
        integer(&native["blocked_timer"][0]),
        integer(&native["blocked_timer"][2]),
    ));
    runtime.set_path_blocked(byte(&native["path_blocked"]) != 0);
    // Foot+64C is supplied independently as 7 by the same fixture. The
    // accepted setter4D96C2..4D9707 preserves it; FindPath owns its reset.
    runtime.set_retries_left(u32::try_from(integer(&native["retries"])).unwrap());
    entity
        .navigation
        .set_nav_com_for_test(native_nav(row, &native["nav"]));
    entity
        .navigation
        .set_nav_com_aux_for_test(native_nav(row, &native["aux"]));
    entity
        .setter_force_reassign
        .set_for_test(byte(&native["force_reassign"]) != 0);
    entity
        .lift
        .set_swap_active_for_test(byte(&native["swap_6ad"]) != 0);
    // Preserve unrelated Rust scheduler fields while supplying the recorded words.
    let mission = &mut entity.mission;
    mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(integer(&native["mission"])),
        suspended: mission.suspended(),
        queued: MissionId::from_raw(integer(&native["queued"])),
        movement_bypass_latch: mission.movement_bypass_latch(),
        handler_state: u32::try_from(integer(&native["status"])).unwrap(),
        mission_start_frame: mission.mission_start_frame(),
        ai_counter: mission.ai_counter(),
        dispatch_timer: MissionDispatchTimer::from_raw(
            integer(&native["dispatch_timer"][0]),
            integer(&native["dispatch_timer"][2]),
        ),
    });
    if native["active"] != "teleport" {
        supply_retained(entity, &first["state"]["drives"]["drive"]);
    }
    scene
}

fn assert_boundary(scene: &Scene, row: &Value, expected: &Value, context: &str) {
    let entity = scene.sim.substrate.entities.get(scene.miner).unwrap();
    let foot = &expected["state"]["foot"];
    let loco = entity.locomotor.as_ref().unwrap();
    assert_eq!(
        ground_pose::position_world_coord(&entity.position),
        coord(&foot["physical"]),
        "{context}: exact Foot XYZ"
    );
    assert_eq!(
        entity.foot_speed.applied_fraction(),
        fraction(&foot["applied_speed_bits"]),
        "{context}: independent Foot applied speed"
    );
    assert_eq!(
        entity.on_bridge(),
        byte(&foot["on_bridge"]) != 0,
        "{context}: OnBridge"
    );
    assert_eq!(
        entity.navigation.nav_com(),
        native_nav(row, &foot["nav"]),
        "{context}: NavCom"
    );
    assert_eq!(
        entity.navigation.nav_com_aux(),
        native_nav(row, &foot["aux"]),
        "{context}: aux"
    );
    assert_eq!(
        entity.navigation.path_replay.remaining_directions(),
        path(&foot["path"]),
        "{context}: Foot queue"
    );
    assert_eq!(
        entity.navigation.path_replay.reference_cell(),
        Some((
            i16::try_from(integer(&foot["reference_cell"][0])).unwrap(),
            i16::try_from(integer(&foot["reference_cell"][1])).unwrap(),
        )),
        "{context}: Foot reference cell"
    );
    let runtime = entity.navigation.path_runtime;
    assert_eq!(
        runtime.movement_timer(),
        CdTimer::from_raw(
            integer(&foot["movement_timer"][0]),
            integer(&foot["movement_timer"][2])
        ),
        "{context}: movement timer"
    );
    assert_eq!(
        runtime.blocked_timer(),
        CdTimer::from_raw(
            integer(&foot["blocked_timer"][0]),
            integer(&foot["blocked_timer"][2])
        ),
        "{context}: blocked timer"
    );
    assert_eq!(
        runtime.retries_left(),
        u32::try_from(integer(&foot["retries"])).unwrap(),
        "{context}: retries"
    );
    assert_eq!(
        runtime.path_blocked(),
        byte(&foot["path_blocked"]) != 0,
        "{context}: path latch"
    );
    assert_eq!(
        entity.setter_force_reassign.is_raised(),
        byte(&foot["force_reassign"]) != 0,
        "{context}: force"
    );
    assert_eq!(
        entity.lift.swap_active(),
        byte(&foot["swap_6ad"]) != 0,
        "{context}: Foot swap"
    );
    assert_eq!(
        entity.mission.current().raw(),
        integer(&foot["mission"]),
        "{context}: mission"
    );
    assert_eq!(
        entity.mission.queued().raw(),
        integer(&foot["queued"]),
        "{context}: queued mission"
    );
    assert_eq!(
        entity.mission.handler_state(),
        u32::try_from(integer(&foot["status"])).unwrap(),
        "{context}: mission cursor"
    );
    assert_eq!(
        entity.navigation.nav_queue().len(),
        usize::try_from(foot["nav_queue"]["count"].as_u64().unwrap()).unwrap(),
        "{context}: NavQueue count"
    );
    assert_eq!(
        entity.mission.dispatch_timer().start_frame(),
        integer(&foot["dispatch_timer"][0]),
        "{context}: dispatch frame"
    );
    assert_eq!(
        entity.mission.dispatch_timer().delay(),
        integer(&foot["dispatch_timer"][2]),
        "{context}: dispatch delay"
    );
    if foot["active"] == "teleport" {
        assert_eq!(
            loco.active_kind(),
            LocomotorKind::Teleport,
            "{context}: restored class"
        );
        assert!(loco.piggyback().is_none(), "{context}: transferred stash");
        assert!(
            loco.selected_drive_runtime().is_none(),
            "{context}: retired complete Drive"
        );
        assert!(
            !loco.has_track_state(TrackFamily::Drive),
            "{context}: no retired retained slot"
        );
    } else {
        assert_eq!(
            loco.active_kind(),
            LocomotorKind::Drive,
            "{context}: active class"
        );
        assert_drive(
            loco,
            &expected["state"]["drives"][foot["active"].as_str().unwrap()],
            context,
        );
        assert_eq!(
            loco.piggyback().as_deref().unwrap().active_kind(),
            LocomotorKind::Teleport,
            "{context}: complete suspended primary"
        );
    }
    assert_eq!(
        loco.effective_kind(),
        LocomotorKind::Teleport,
        "{context}: constructed family"
    );
    let primary = loco.piggyback().as_deref().unwrap_or(loco);
    let teleport = primary.teleport_runtime().unwrap();
    let native_teleport = &expected["state"]["teleport"];
    assert_eq!(
        teleport.is_moving(),
        byte(&native_teleport["moving"]) != 0,
        "{context}: restored/suspended Teleport request"
    );
    assert_eq!(
        teleport.warp().and_then(|warp| warp.destination()),
        nonnull_coord(&native_teleport["destination"]),
        "{context}: Teleport armed XYZ"
    );
    assert_eq!(
        teleport.resolved_destination(),
        nonnull_coord(&native_teleport["resolved"]),
        "{context}: Teleport resolver XYZ"
    );
}

fn owned_state(entity: &GameEntity) -> Value {
    serde_json::to_value((
        &entity.locomotor,
        &entity.navigation,
        &entity.foot_speed,
        ground_pose::position_world_coord(&entity.position),
        &entity.mission,
        entity.lift.swap_active(),
    ))
    .unwrap()
}

fn replay(row: &Value) {
    let mut scene = prepare(row);
    let original_primary = scene
        .sim
        .substrate
        .entities
        .get(scene.miner)
        .unwrap()
        .locomotor
        .clone();
    assert_eq!(row["rng_calls"].as_array().unwrap().len(), 0);
    for step in row["steps"].as_array().unwrap() {
        let label = step["label"].as_str().unwrap();
        let before = boundary(row, step, "before");
        let after = boundary(row, step, "after");
        scene
            .sim
            .session
            .set_binary_frame_for_test(u32::try_from(before["frame"].as_u64().unwrap()).unwrap());
        let entity = scene.sim.substrate.entities.get_mut(scene.miner).unwrap();
        // Only the recipe's declared inputs are supplied between operations.
        // No after-state is fed back into the production controller.
        match label {
            "reuse_move" => supply_retained(entity, &before["state"]["drives"]["drive1"]),
            "disable_end" => {
                assert!(entity.locomotor.as_mut().unwrap().store_track_head(
                    TrackFamily::Drive,
                    nonnull_coord(&before["state"]["drives"]["drive1"]["head"]),
                ));
            }
            "swap_end_refusal" | "successful_end" => {
                entity
                    .lift
                    .set_swap_active_for_test(byte(&before["state"]["foot"]["swap_6ad"]) != 0);
            }
            _ => {}
        }
        let saved = owned_state(entity);
        assert_boundary(&scene, row, before, &format!("{label}: before"));
        let rng_before = (
            scene.sim.scenario_rng.logical_state(),
            scene.sim.main_rng.logical_state(),
            scene.sim.mapgen_rng.logical_state(),
        );
        match label {
            "fresh_move" | "reuse_move" | "repeat_fresh_move" | "pad_setter" => {
                let call = step_calls(row, step)
                    .iter()
                    .find(|call| call["name"] == "unit_set_destination")
                    .unwrap();
                // The actual call argument is the requested native Cell identity.
                let pointer = Value::String(format!("0x{:x}", call["args"][0].as_u64().unwrap()));
                let target = native_nav(row, &pointer).unwrap();
                assert!(scene.sim.set_unit_destination(
                    scene.miner,
                    target,
                    &scene.rules,
                    call["args"][1].as_u64().unwrap() != 0,
                    crate::sim::world::FrameEffects::default()
                ));
            }
            "original_stop" => {
                assert!(movement::track_stop_moving(
                    scene.sim.substrate.entities.get_mut(scene.miner).unwrap()
                ));
            }
            "disable_end" | "enable_end" => {
                let call = step_calls(row, step)
                    .iter()
                    .find(|call| {
                        matches!(
                            call["name"].as_str(),
                            Some("drive_disable_end" | "drive_enable_end")
                        )
                    })
                    .unwrap();
                let permitted = call["name"] == "drive_enable_end";
                assert!(
                    scene
                        .sim
                        .substrate
                        .entities
                        .get_mut(scene.miner)
                        .unwrap()
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .store_drive_end_permission(permitted)
                );
            }
            "moving_end_refusal"
            | "permission_end_refusal"
            | "swap_end_refusal"
            | "successful_end"
            | "post_stop_end" => {
                let gate = step_calls(row, step)
                    .iter()
                    .find(|call| call["name"] == "drive_end_gate")
                    .unwrap();
                let admitted = gate["return_eax"].as_u64().unwrap() & 0xFF != 0;
                assert_eq!(
                    scene.sim.try_end_piggyback(scene.miner, None),
                    admitted,
                    "{label}: native END gate"
                );
                if !admitted {
                    assert_eq!(
                        owned_state(scene.sim.substrate.entities.get(scene.miner).unwrap()),
                        saved,
                        "{label}: refused complete state"
                    );
                } else if row["input"]["history"] == "far" {
                    assert_eq!(
                        scene
                            .sim
                            .substrate
                            .entities
                            .get(scene.miner)
                            .unwrap()
                            .locomotor,
                        original_primary,
                        "{label}: same complete primary restored"
                    );
                }
            }
            other => panic!("unreplayed native instance step {other}"),
        }
        assert_eq!(
            (
                scene.sim.scenario_rng.logical_state(),
                scene.sim.main_rng.logical_state(),
                scene.sim.mapgen_rng.logical_state(),
            ),
            rng_before,
            "{label}: no Scenario, Main or MapGen draw"
        );
        assert_boundary(&scene, row, after, label);
    }
}

#[test]
fn fresh_begin_owns_native_slope_frame_without_eager_retained_allocation() {
    let data = corpus();
    let row = control(&data, "far_fresh_reuse_end_repeat");
    for native in row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["label"] == "allocator_original_constructor_return")
    {
        let mut scene = prepare(row);
        let frame = u32::try_from(native["frame"].as_u64().unwrap()).unwrap();
        let entity = scene.sim.substrate.entities.get_mut(scene.miner).unwrap();
        assert!(locomotor_owner::begin_drive_for_teleporter(entity, frame));
        let loco = entity.locomotor.as_ref().unwrap();
        assert!(loco.selected_drive_runtime().unwrap().retained().is_none());
        assert_drive(
            loco,
            &native["state"]["drives"][native["drive"].as_str().unwrap()],
            "original constructor through BEGIN",
        );
    }
}

/// Far-move reuse, refusals, end and repeat; a stopped pad setter releasing
/// the complete Drive before the teleport move; a moving pad setter stopping,
/// then Foot's end releasing the same complete Drive.
#[test]
fn drive_instance_controls_match_complete_native_instances() {
    let data = corpus();
    for name in [
        "far_fresh_reuse_end_repeat",
        "pad_stopped_end",
        "pad_moving_refused_stop_end",
    ] {
        replay(control(&data, name));
    }
}
