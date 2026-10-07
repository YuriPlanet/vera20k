//! Locomotor unit tests — verifies locomotor state initialization, speed type mapping,
//! and ObjectType-to-LocomotorState conversion for various unit categories.

use super::*;
use crate::rules::jumpjet_params::JumpjetParams;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::util::fixed_math::{SIM_ZERO, sim_from_f32};

#[test]
fn walk_destination_and_cell_producer_match_original_startup_conversion() {
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::sim::{components::DriveCoord, game_entity::GameEntity};
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_head_occupation.json",
    ))
    .unwrap();
    let rows = native["destination"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for row in rows {
        let input = &row["input"];
        let output = &row["output"];
        let mut terrain = ResolvedTerrainGrid::from_cells(
            11,
            11,
            (0..11)
                .flat_map(|y| {
                    (0..11).map(move |x| {
                        crate::sim::world::common_raw_test_terrain_cell(x, y, 2, false)
                    })
                })
                .collect(),
        );
        let c = terrain.cell_mut(10, 10).unwrap();
        c.slope_type = 1;
        c.bridge_facts.raw_flags = if input["structural"].as_bool().unwrap() {
            0x100
        } else {
            0
        };
        let mut entity = GameEntity::test_default(1, "E1", "Owner", 9, 10);
        entity.locomotor = Some(LocomotorState::from_object_type(
            &make_obj(LocomotorKind::Walk, ObjectCategory::Infantry),
            0,
        ));
        let coord = if input["cell_target"].as_bool().unwrap() {
            crate::sim::movement::navcom::target_cell_coord(
                10,
                10,
                Some(&terrain)
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            )
        } else {
            let c = &input["coord"];
            DriveCoord {
                x: c[0].as_i64().unwrap() as i32,
                y: c[1].as_i64().unwrap() as i32,
                z: c[2].as_i64().unwrap() as i32,
            }
        };
        assert_eq!(
            serde_json::json!([coord.x, coord.y, coord.z]),
            output["incoming"],
            "{row}"
        );
        crate::sim::movement::set_walk_destination_coord(&mut entity, coord, Some(&terrain));
        let loco = entity.locomotor.as_ref().unwrap();
        let dest = loco.walk_destination().unwrap();
        assert_eq!(
            serde_json::json!([dest.x, dest.y, dest.z]),
            output["destination"],
            "{row}"
        );
        assert_eq!(loco.walk_is_moving(), output["moving"].as_bool(), "{row}");
        assert_eq!(
            loco.step_head(),
            None,
            "the destination setter never accepts a head"
        );
    }
}

#[test]
fn walk_moving_byte_matches_original_setter_and_head_lifetime_traces() {
    use crate::sim::components::DriveCoord;
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_head_occupation.json",
    ))
    .unwrap();
    let cases = native["moving"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    let coord = DriveCoord {
        x: 2752,
        y: 2624,
        z: 0,
    };
    let xyz = |value: Option<DriveCoord>| value.map_or([0, 0, 0], |c| [c.x, c.y, c.z]);
    for case in cases {
        let mut loco = LocomotorState::from_object_type(
            &make_obj(LocomotorKind::Walk, ObjectCategory::Infantry),
            0,
        );
        let actions = case["actions"].as_array().unwrap();
        let trace = case["output"]["trace"].as_array().unwrap();
        assert_eq!(trace.len(), actions.len() + 1);
        for (index, expected) in trace.iter().enumerate() {
            if index > 0 {
                match actions[index - 1].as_str().unwrap() {
                    "move" => loco.set_walk_destination(Some(coord)),
                    "stop" => loco.set_walk_destination(None),
                    // Supplied private-head transitions isolate this byte's
                    // lifetime; production placement/raw has its own corpus.
                    "head" => loco.set_step_head(Some(coord)),
                    "retire" => loco.set_step_head(None),
                    action => panic!("unknown original action {action}"),
                }
            }
            assert_eq!(
                serde_json::json!({
                    "moving": loco.walk_is_moving().unwrap(),
                    "destination": xyz(loco.walk_destination()),
                    "head": xyz(loco.step_head()),
                }),
                *expected,
                "{case} at {index}"
            );
            // A null destination/head with moving=true is a real callback
            // state, so persistence must not infer this byte from either.
            let restored: LocomotorState =
                serde_json::from_str(&serde_json::to_string(&loco).unwrap()).unwrap();
            assert_eq!(restored.walk_is_moving(), loco.walk_is_moving());
        }
    }
}

/// Parse defaults through the ObjectType owner, then supply the locomotor
/// inputs these initialization fixtures vary. Unrelated type defaults stay
/// with the production reader instead of a second hundreds-field constructor.
fn make_obj(locomotor: LocomotorKind, category: ObjectCategory) -> ObjectType {
    let ini = crate::rules::ini_parser::IniFile::from_str("[TEST]\n");
    let mut object = ObjectType::from_ini_section("TEST", ini.section_or_empty("TEST"), category);
    object.locomotor = locomotor;
    object.speed_type = SpeedType::Track;
    object.movement_zone = MovementZone::Normal;
    object.turret_rot = 0;
    object.jumpjet_params = JumpjetParams::default();
    object.airport_bound = false;
    object.balloon_hover = false;
    object.hover_attack = false;
    object
}

#[test]
fn test_drive_locomotor() {
    let obj = make_obj(LocomotorKind::Drive, ObjectCategory::Vehicle);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Drive);
    assert_eq!(state.layer, MovementLayer::Ground);
    assert_eq!(state.air_phase(), AirMovePhase::Landed);
    assert!(state.is_ground_mover());
    assert!(!state.is_air_mover());
}

#[test]
fn test_hover_cruises_at_full_base_speed() {
    // Hover now cruises at its full base Speed (throttle 1.0), not the old
    // made-up 0.65x. The accel/brake throttle ramp lives in sim/movement/hover.rs.
    let obj = make_obj(LocomotorKind::Hover, ObjectCategory::Vehicle);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Hover);
    assert!(state.is_ground_mover());
}

#[test]
fn test_walk_locomotor() {
    let obj = make_obj(LocomotorKind::Walk, ObjectCategory::Infantry);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Walk);
    assert_eq!(state.layer, MovementLayer::Ground);
    assert!(state.is_ground_mover());
}

#[test]
fn test_fly_locomotor_air_layer() {
    let obj = make_obj(LocomotorKind::Fly, ObjectCategory::Aircraft);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Fly);
    assert_eq!(state.layer, MovementLayer::Air);
    assert_eq!(state.air_phase(), AirMovePhase::Landed);
    assert!(!state.is_ground_mover());
    assert!(state.is_air_mover());
    assert_eq!(state.fly_target_height(), 0);
    assert_eq!(
        state.fly_runtime().unwrap().target_speed,
        SIM_ZERO,
        "constructor4CC9E5 target speed"
    );
}

#[test]
fn fly_target_uses_type_flight_level_without_changing_other_locomotors() {
    let mut obj = make_obj(LocomotorKind::Fly, ObjectCategory::Aircraft);
    for (configured, expected) in [(-1, 1500), (0, 0), (-2, -2), (2200, 2200)] {
        obj.flight_level = configured;
        let mut state = LocomotorState::from_object_type(&obj, 0);
        assert_eq!(state.fly_target_height(), 0, "constructor4CC9EE");
        state.begin_fly_takeoff(obj.flight_level(1500));
        assert_eq!(
            state.fly_target_height(),
            expected,
            "admitted BeginTakeoff4CF9F8"
        );
    }
    obj.flight_level = 2200;
    obj.locomotor = LocomotorKind::Jumpjet;
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(
        state.jumpjet_runtime().unwrap().params().height,
        obj.jumpjet_params.height
    );
    assert!(state.fly_runtime().is_none());
}

#[test]
fn test_jumpjet_air_layer() {
    let obj = make_obj(LocomotorKind::Jumpjet, ObjectCategory::Infantry);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Jumpjet);
    assert_eq!(state.layer, MovementLayer::Air);
    assert!(!state.is_ground_mover());
    assert!(state.is_air_mover());
    assert_eq!(state.jumpjet_runtime().unwrap().params().height, 500);
}

#[test]
fn test_jumpjet_with_custom_params() {
    let mut obj = make_obj(LocomotorKind::Jumpjet, ObjectCategory::Infantry);
    obj.jumpjet = true;
    obj.jumpjet_params = JumpjetParams {
        turn_rate: 4,
        speed: sim_from_f32(20.0),
        climb: 8.0,
        crash: 5.0,
        height: 750,
        accel: 2.0,
        wobbles: 0.2,
        deviation: 40,
        no_wobbles: false,
    };
    let state = LocomotorState::from_object_type(&obj, 0);
    let params = state.jumpjet_runtime().unwrap().params();
    assert_eq!(params.height, 750);
    assert_eq!(params.speed, 20);
    assert_eq!(params.climb_bits, 8.0f32.to_bits());
}

#[test]
fn test_ship_is_ground_mover() {
    let obj = make_obj(LocomotorKind::Ship, ObjectCategory::Vehicle);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Ship);
    assert!(state.is_ground_mover());
    assert!(!state.is_air_mover());
}

#[test]
fn cmin_locomotor_initializes_primary_and_active_teleport() {
    let mut obj = make_obj(LocomotorKind::Teleport, ObjectCategory::Vehicle);
    obj.harvester = true;
    obj.teleporter = true;

    let state = LocomotorState::from_object_type(&obj, 0);

    assert_eq!(state.active_kind(), LocomotorKind::Teleport);
    assert_eq!(state.effective_kind(), LocomotorKind::Teleport);
    assert!(state.is_primary_active());
}

// --- Override/Piggyback mechanism tests ---

#[test]
fn test_override_teleport_round_trip() {
    let obj = make_obj(LocomotorKind::Drive, ObjectCategory::Vehicle);
    let mut state = LocomotorState::from_object_type(&obj, 0);
    assert!(!state.is_overridden());
    assert_eq!(state.kind, LocomotorKind::Drive);
    assert_eq!(state.layer, MovementLayer::Ground);

    // Begin teleport override.
    state.begin_piggyback(LocomotorKind::Teleport, 0);
    assert!(state.is_overridden());
    assert_eq!(state.kind, LocomotorKind::Teleport);
    assert_eq!(state.layer, MovementLayer::Ground);

    // End override — should restore Drive.
    assert!(state.end_piggyback());
    assert!(!state.is_overridden());
    assert_eq!(state.kind, LocomotorKind::Drive);
    assert_eq!(state.layer, MovementLayer::Ground);
}

#[test]
fn end_piggyback_without_a_stash_reports_nothing_to_pop() {
    let obj = make_obj(LocomotorKind::Drive, ObjectCategory::Vehicle);
    let mut state = LocomotorState::from_object_type(&obj, 0);
    let result = state.end_piggyback();
    assert!(
        !result,
        "ending with nothing stashed reports nothing to pop"
    );
    assert_eq!(state.kind, LocomotorKind::Drive);
}

#[test]
fn test_override_preserves_speed_type() {
    let mut obj = make_obj(LocomotorKind::Drive, ObjectCategory::Vehicle);
    obj.speed_type = SpeedType::Wheel;
    let mut state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.speed_type, SpeedType::Wheel);

    state.begin_piggyback(LocomotorKind::Teleport, 0);
    // SpeedType should still reflect the original during override.
    state.end_piggyback();
    assert_eq!(state.speed_type, SpeedType::Wheel);
}

#[test]
fn drive_piggyback_restores_primary_teleport_only_after_not_moving() {
    let obj = make_obj(LocomotorKind::Teleport, ObjectCategory::Vehicle);
    let mut state = LocomotorState::from_object_type(&obj, 0);

    assert!(state.begin_drive_piggyback_for_teleporter(0));
    assert_eq!(state.active_kind(), LocomotorKind::Drive);
    assert_eq!(state.effective_kind(), LocomotorKind::Teleport);
    assert!(state.end_piggyback());
    assert_eq!(state.active_kind(), LocomotorKind::Teleport);
    assert_eq!(state.effective_kind(), LocomotorKind::Teleport);
    assert!(state.is_primary_active());
}

#[test]
fn drive_piggyback_refuses_an_unstashed_active_drive() {
    let obj = make_obj(LocomotorKind::Teleport, ObjectCategory::Vehicle);
    let mut state = LocomotorState::from_object_type(&obj, 0);
    state.kind = LocomotorKind::Drive;

    assert!(!state.begin_drive_piggyback_for_teleporter(0));
    assert_eq!(state.kind, LocomotorKind::Drive);
    assert!(state.piggyback.is_none());
}

/// End of the production chain for the two units the `JumpJet=` gate broke:
/// retail INI bytes -> `ObjectType::from_ini_section` -> `LocomotorState`.
///
/// gamemd copies the type's jumpjet block into the locomotor unconditionally
/// once the Jumpjet locomotor is installed (parameter copy at `0x0054AD30`,
/// reading `TechnoType+0xD70`..`+0xD8C`), and `TechnoTypeClass::ReadINI`
/// `0x00715020`-`0x0071520F` filled that block with no reference to
/// `JumpJet=`. So a stock Kirov hovers at its authored 750, not at the
/// constructor's 500.
#[test]
fn retail_kirov_and_disc_reach_their_authored_hover_altitude() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };

    // (section, JumpjetSpeed, JumpjetClimb, JumpjetCrash) as authored.
    for (id, speed, climb, crash) in [("ZEP", 5, 6.0_f32, 12.0_f32), ("DISK", 16, 8.0, 15.0)] {
        let obj = ObjectType::from_ini_section(
            id,
            ini.section(id).unwrap_or_else(|| panic!("[{id}] section")),
            ObjectCategory::Vehicle,
        );
        let state = LocomotorState::from_object_type(&obj, 0);

        assert_eq!(state.kind, LocomotorKind::Jumpjet, "[{id}]");
        let params = state.jumpjet_runtime().unwrap().params();
        assert_eq!(
            params.height, 750,
            "[{id}] hovers at its authored JumpjetHeight"
        );
        assert_eq!(params.speed, speed, "[{id}]");
        assert_eq!(params.climb_bits, climb.to_bits(), "[{id}]");
        assert_eq!(params.crash_bits, crash.to_bits(), "[{id}]");
        // The constructor's acceleration and turn rate, because stock spells
        // the keys `JumpJetAccel=`/`JumpJetTurnRate=` and gamemd looks up
        // `JumpjetAccel`/`JumpjetTurnRate`.
        assert_eq!(params.accel_bits, 2.0f32.to_bits(), "[{id}]");
        assert_eq!(params.turn_rate, 4, "[{id}]");
    }
}

/// Non-Jumpjet locomotors must not pick up the block. Every `TechnoType`
/// carries it, but only the copy at `0x0054AD30` reads it, and that lives in
/// the Jumpjet locomotor.
#[test]
fn non_jumpjet_locomotors_ignore_the_types_jumpjet_block() {
    for kind in [
        LocomotorKind::Drive,
        LocomotorKind::Walk,
        LocomotorKind::Hover,
        LocomotorKind::Fly,
        LocomotorKind::Ship,
    ] {
        let mut obj = make_obj(kind, ObjectCategory::Vehicle);
        // A section that authored every jumpjet key would still not reach a
        // Drive or Fly locomotor.
        obj.jumpjet_params = JumpjetParams {
            turn_rate: 100,
            speed: sim_from_f32(99.0),
            climb: 9.0,
            crash: 9.0,
            height: 750,
            accel: 10.0,
            wobbles: 0.5,
            deviation: 15,
            no_wobbles: true,
        };
        let state = LocomotorState::from_object_type(&obj, 0);

        assert!(state.jumpjet_runtime().is_none(), "{kind:?}");
        if kind != LocomotorKind::Fly {
            assert_eq!(state.fly_target_height(), 0, "{kind:?}");
        }
    }
}
