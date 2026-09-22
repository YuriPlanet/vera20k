use super::*;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::deploy::DeployPhase;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::{DestinationTiming, FacingClass, locomotor::LocomotorState};

#[test]
fn unconditional_unit_scatter_refusals_match_native_and_leave_orders_and_rng_untouched() {
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/unit_scatter_state.json"
    ))
    .unwrap();
    let interner = crate::sim::intern::test_interner();
    let mut checked = 0;
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap_or(false);
        let mission =
            |key: &str, default| MissionId::from_raw(input[key].as_i64().unwrap_or(default) as i32);
        let frame = input["frame"].as_u64().unwrap_or(100) as u32;
        let now = frame.wrapping_add(input["elapsed"].as_u64().unwrap_or(0) as u32);
        let mut actor = GameEntity::test_default(1, "MTNK", "Allies", 5, 5);
        actor.category = EntityCategory::Unit;
        actor
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: mission("mission", 5),
                queued: mission("queued", -1),
                suspended: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });
        actor.locomotor = Some(LocomotorState::for_test_kind(if flag("teleport") {
            LocomotorKind::Teleport
        } else {
            LocomotorKind::Drive
        }));
        actor.locomotor.as_mut().unwrap().powered = !flag("power_off");
        actor.deploy_state = if flag("deployed") {
            Some(DeployPhase::Deployed)
        } else if flag("deploying") {
            Some(DeployPhase::Deploying { ticks_remaining: 5 })
        } else if flag("undeploying") {
            Some(DeployPhase::Undeploying { ticks_remaining: 5 })
        } else {
            None
        };
        if flag("nav") {
            actor.navigation.nav_com = Some(NavTargetRef::cell(8, 8));
        }
        let mut facing = FacingClass::new(0, input["rot"].as_i64().unwrap_or(5) as i32);
        if flag("turn") {
            facing.set(0x4000, frame);
        }
        actor.body_facing = Some(facing);
        let admitted = unit_scatter_state_allows(&actor, now);
        assert_eq!(admitted, row["admitted"].as_bool().unwrap(), "{input}");
        if !admitted {
            // Call the actual shared production blocker receiver, not only the
            // predicate. Rejection precedes selection RNG, PowerOn, mission,
            // destination, timers and all movement writes.
            let before = serde_json::to_value(&actor).unwrap();
            let mut entities = EntityStore::new();
            entities.insert(actor);
            let mut rng = SimRng::new(42);
            let rng_before = rng.state();
            assert!(
                !scatter_blocker(
                    &mut entities,
                    1,
                    Some(&PathGrid::new(12, 12)),
                    None,
                    &OccupancyGrid::new(),
                    MovementLayer::Ground,
                    &mut rng,
                    None,
                    &interner,
                    DestinationTiming::new(now, 60),
                ),
                "{input}"
            );
            assert_eq!(
                serde_json::to_value(entities.get(1).unwrap()).unwrap(),
                before,
                "{input}"
            );
            assert_eq!(rng.state(), rng_before, "{input}");
        }
        checked += 1;
    }
    assert_eq!(checked, 69);
}

#[test]
fn unit_scatter_checks_active_locomotor_and_body_rotation_not_stashed_slot_or_turret() {
    let mut actor = GameEntity::test_default(1, "CMIN", "Allies", 5, 5);
    actor.category = EntityCategory::Unit;
    actor
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: MissionId::from_known(MissionType::Move),
            queued: MissionId::NONE,
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
    actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
    assert!(!unit_scatter_state_allows(&actor, 100));
    assert!(crate::sim::movement::locomotor_owner::begin_drive_for_teleporter(&mut actor, 100));
    actor.turret_rotation_latch = true;
    actor.navigation.nav_com = Some(NavTargetRef::cell(8, 8));
    actor.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
        head_to: Some(DriveCoord::cell(6, 5, 0)),
        ..Default::default()
    });
    assert!(
        unit_scatter_state_allows(&actor, 100),
        "prefix has no moving, NavCom or turret gate"
    );
}
