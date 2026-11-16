//! VERA watching: a watcher's world follows another player
//! (`Simulation::follow_house`, docs/Networking.md "Watching").

use super::*;
use crate::sim::combat::Health;
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;

/// Ann, Bob and a computer, with Bob's tank that only Bob has seen; the
/// world follows Ann.
fn watched_world(mode: ScenarioGameMode) -> (Simulation, InternedId, InternedId, InternedId) {
    let mut sim = Simulation::new();
    sim.session.set_game_mode(mode);
    let ann = sim.interner.intern("Ann");
    let bob = sim.interner.intern("Bob");
    let computer = sim.interner.intern("Computer");
    for (owner, human) in [(ann, true), (bob, true), (computer, false)] {
        sim.houses
            .register(HouseState::new(owner, 0, None, human, 0, 10));
    }
    sim.session.install_current_house(Some(ann));
    let type_ref = sim.interner.intern("MTNK");
    let mut tank = GameEntity::new_at_frame_zero_for_test(
        1,
        5,
        5,
        0,
        0,
        bob,
        Health::new(100),
        type_ref,
        EntityCategory::Unit,
        0,
        0,
        true,
    );
    // Bob is the second human: bit 1.
    tank.discovery.network_human_discovery = 0b10;
    sim.substrate.entities.insert(tank);
    (sim, ann, bob, computer)
}

#[test]
fn following_another_player_moves_only_this_games_view() {
    let (mut sim, ann, bob, computer) = watched_world(ScenarioGameMode::Wol);
    let shared = sim.state_hash();
    assert!(
        !sim.follow_house(computer),
        "a computer has no view to follow"
    );
    assert!(!sim.follow_house(ann), "Ann is already followed");
    assert!(sim.follow_house(bob));
    assert_eq!(sim.session.current_house(), Some(bob));
    let tank = sim.substrate.entities.get(1).unwrap().discovery;
    assert!(tank.owned_by_current_house());
    assert!(tank.discovered_by_current_house(), "Bob saw his own tank");
    assert!(!tank.discovered_by_other_house, "nobody else has seen it");
    assert_eq!(
        sim.state_hash(),
        shared,
        "every player's world is unchanged"
    );
    assert!(sim.follow_house(ann));
    let tank = sim.substrate.entities.get(1).unwrap().discovery;
    assert!(!tank.owned_by_current_house() && !tank.discovered_by_current_house());
    assert!(
        tank.discovered_by_other_house,
        "Bob, now another receiver, saw it"
    );
}

#[test]
fn an_offline_world_has_one_view() {
    let (mut sim, ann, bob, _) = watched_world(ScenarioGameMode::from_legacy_nonzero(true));
    assert!(!sim.follow_house(bob));
    assert_eq!(sim.session.current_house(), Some(ann));
}
