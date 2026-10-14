//! Slice 8 — `MissionCom` is folded into `state_hash`. These pin that each
//! component field is hash-relevant (the inverse of the Slice-2 shadow tests).

use super::Simulation;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionCom, MissionDispatchTimer, MissionId, MissionType};

fn fixture_from(state: &MissionCom) -> MissionTestFixture {
    MissionTestFixture {
        current: state.current(),
        suspended: state.suspended(),
        queued: state.queued(),
        movement_bypass_latch: state.movement_bypass_latch(),
        handler_state: state.handler_state(),
        mission_start_frame: state.mission_start_frame(),
        ai_counter: state.ai_counter(),
        dispatch_timer: state.dispatch_timer(),
    }
}

fn edit_mission(sim: &mut Simulation, edit: impl FnOnce(&mut MissionTestFixture)) {
    let entity = sim.substrate.entities.get_mut(1).expect("test entity");
    let mut fixture = fixture_from(&entity.mission);
    edit(&mut fixture);
    entity.mission.apply_test_fixture(fixture);
}

fn two_sims() -> (Simulation, Simulation) {
    let mut a = Simulation::new();
    let mut b = Simulation::new();
    a.substrate
        .entities
        .insert(GameEntity::test_default(1, "MTNK", "Americans", 10, 10));
    b.substrate
        .entities
        .insert(GameEntity::test_default(1, "MTNK", "Americans", 10, 10));
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "baseline sims must hash equal"
    );
    (a, b)
}

#[test]
fn mission_current_changes_state_hash() {
    let (a, mut b) = two_sims();
    edit_mission(&mut b, |fixture| {
        fixture.current = MissionId::from_known(MissionType::Attack);
    });
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "mission.current must contribute to the state hash"
    );
}

#[test]
fn mission_timer_and_substate_change_state_hash() {
    let (a, mut b) = two_sims();
    // The old reduced substate projects into the final full-width handler state.
    edit_mission(&mut b, |fixture| fixture.handler_state = 7);
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "mission.handler_state must affect hash"
    );
    // Reset handler state -> back to equal -> then perturb the dispatch timer.
    edit_mission(&mut b, |fixture| fixture.handler_state = 0);
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "handler-state reset restores equality"
    );
    edit_mission(&mut b, |fixture| {
        fixture.dispatch_timer = MissionDispatchTimer::from_raw(5, 30);
    });
    let dispatch_timer = b
        .substrate
        .entities
        .get(1)
        .expect("test entity")
        .mission
        .dispatch_timer();
    assert_eq!(dispatch_timer.start_frame(), 5);
    assert_eq!(dispatch_timer.delay(), 30);
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "mission.dispatch_timer must affect hash"
    );
}

#[test]
fn mission_queued_and_suspended_change_state_hash() {
    let (a, mut b) = two_sims();
    edit_mission(&mut b, |fixture| {
        fixture.queued = MissionId::from_known(MissionType::Guard);
    });
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "mission.queued must affect hash"
    );
    edit_mission(&mut b, |fixture| fixture.queued = MissionId::NONE);
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "queued reset restores equality"
    );
    edit_mission(&mut b, |fixture| {
        fixture.suspended = MissionId::from_known(MissionType::Move);
    });
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "mission.suspended must affect hash"
    );
}
