//! VERA watching (docs/Networking.md "Watching"): a watcher's world plays the
//! match in the followed player's seat, sends nothing, and can follow another
//! player or show the observer's sidebar (`Simulation::install_watcher`,
//! `Simulation::follow_house`, `Simulation::observe`).

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::command::{Command, CommandEnvelope, CommandRecord};
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{
    FrameEffects, LifecycleOutput, NetworkCommandBoundary, NetworkCommandError,
    NetworkCommandQueues, NetworkQueueContext, NetworkQueuePass, PlayerOutcomeRequest, TickLane,
};

/// Ann, Bob and a computer; the world follows Ann, as a watcher's when
/// `watcher`.
fn players(
    mode: ScenarioGameMode,
    watcher: bool,
) -> (Simulation, InternedId, InternedId, InternedId) {
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
    if watcher {
        sim.install_watcher();
    }
    (sim, ann, bob, computer)
}

/// The players, with Bob's tank that only Bob has seen.
fn watched_world(
    mode: ScenarioGameMode,
    watcher: bool,
) -> (Simulation, InternedId, InternedId, InternedId) {
    let (mut sim, ann, bob, computer) = players(mode, watcher);
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
    tank.seldom.discovery.network_human_discovery = 0b10;
    sim.substrate.entities.insert(tank);
    (sim, ann, bob, computer)
}

#[test]
fn following_another_player_moves_only_this_games_view() {
    let (mut sim, ann, bob, computer) = watched_world(ScenarioGameMode::Wol, true);
    let shared = sim.state_hash();
    assert!(
        !sim.follow_house(computer),
        "a computer has no view to follow"
    );
    assert!(!sim.follow_house(ann), "Ann is already followed");
    assert!(sim.follow_house(bob));
    assert_eq!(sim.session.current_house(), Some(bob));
    assert!(
        sim.houses.get(&bob).unwrap().player_control(),
        "Bob's EVA and score lines"
    );
    assert!(!sim.houses.get(&ann).unwrap().player_control());
    let tank = sim.substrate.entities.get(1).unwrap().seldom.discovery;
    assert!(tank.owned_by_current_house());
    assert!(tank.discovered_by_current_house(), "Bob saw his own tank");
    assert!(!tank.discovered_by_other_house, "nobody else has seen it");
    assert_eq!(
        sim.state_hash(),
        shared,
        "every player's world is unchanged"
    );
    assert!(sim.follow_house(ann));
    let tank = sim.substrate.entities.get(1).unwrap().seldom.discovery;
    assert!(!tank.owned_by_current_house() && !tank.discovered_by_current_house());
    assert!(
        tank.discovered_by_other_house,
        "Bob, now another receiver, saw it"
    );
}

/// The observer's sidebar keeps the followed House, so Tab can come back to
/// it, and changes nothing every player shares.
#[test]
fn the_observers_sidebar_keeps_the_followed_house() {
    let (mut sim, ann, _, _) = watched_world(ScenarioGameMode::Wol, true);
    let shared = sim.state_hash();
    assert!(sim.observe());
    assert!(!sim.observe(), "it shows it already");
    assert_eq!(sim.session.current_house(), Some(ann));
    assert!(sim.session.watcher_observes() && !sim.session.is_observer_house(ann));
    assert_eq!(sim.state_hash(), shared);
    assert!(sim.follow_house(ann), "back to Ann's sidebar");
    assert!(!sim.session.watcher_observes());
    assert!(!sim.follow_house(ann), "Ann is already followed");
    let (mut player, ..) = players(ScenarioGameMode::Wol, false);
    assert!(!player.observe(), "a player keeps their own sidebar");
}

/// A stream with nothing more yet: every frame waits on it.
struct NothingYet;

impl NetworkCommandBoundary for NothingYet {
    fn exchange_commands(
        &mut self,
        _: NetworkQueueContext,
        _: &mut NetworkCommandQueues<'_>,
    ) -> Result<Option<NetworkQueuePass>, NetworkCommandError> {
        Ok(Some(NetworkQueuePass::Waiting))
    }

    fn record_frame_checksum(&mut self, _: u32, _: u32) {}

    fn compare_frame_info(&self, _: &CommandRecord, _: i32) -> Result<bool, NetworkCommandError> {
        Ok(true)
    }
}

/// A live watcher's frames often wait on the stream: Tab follows then too.
#[test]
fn a_watcher_follows_while_its_frame_waits_on_the_stream() {
    let (mut sim, _, bob, _) = players(ScenarioGameMode::Lan, true);
    let output = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            33,
            TickLane::Ordinary,
            &mut FrameEffects::empty().with_network_commands(&mut NothingYet),
        )
        .unwrap();
    assert!(output.network_waiting && sim.pending_network_wait());
    assert!(sim.follow_house(bob));
    assert_eq!(sim.session.current_house(), Some(bob));
}

#[test]
fn only_a_watchers_online_world_follows() {
    for (mode, watcher) in [
        (ScenarioGameMode::from_legacy_nonzero(true), true),
        (ScenarioGameMode::Wol, false),
    ] {
        let (mut sim, ann, bob, _) = watched_world(mode, watcher);
        assert!(!sim.follow_house(bob));
        assert_eq!(sim.session.current_house(), Some(ann));
    }
}

/// A watcher stands in the followed player's seat but sends nothing: that
/// player's OPTIONS opens nothing here, and their EXIT is another game's
/// (`0x0064C9B1`), whose WOL tournament tail kills their House as on the
/// players' games, so the watch goes on in step with them.
#[test]
fn the_followed_players_own_records_are_another_games() {
    let (mut sim, ann, _, _) = players(ScenarioGameMode::Wol, true);
    sim.install_wol_start(1, Default::default());
    let records = [
        CommandEnvelope::new(ann, 1, Command::OpenOptions),
        CommandEnvelope::new(ann, 1, Command::ExitMatch),
    ];
    let output = sim
        .advance_app_frame(
            &records,
            RuleSet::empty_for_test(),
            33,
            TickLane::Ordinary,
            &mut FrameEffects::default(),
        )
        .unwrap();
    assert!(
        !output
            .lifecycle_outputs
            .contains(&LifecycleOutput::OptionsRequested)
    );
    assert!(!*sim.quit_requested, "the watch goes on");
    let house = sim.houses.get(&ann).unwrap();
    assert!(
        house.is_to_die() || house.has_lost(),
        "the tournament tail ran"
    );
    assert_eq!(
        sim.take_departed_senders(),
        [sim.houses.index_of(ann).unwrap() as i8]
    );
}

#[test]
fn the_followed_house_can_lose_before_the_watch_ends() {
    use crate::sim::house_state::HouseOutcomeKind;
    use crate::sim::timer::CdTimer;

    let (mut sim, ann, bob, computer) = players(ScenarioGameMode::Wol, true);
    for house in sim.houses.values_mut() {
        house
            .tracking_mut_for_test()
            .set_for_test(1, &[], (0, 0, 0), &[]);
    }
    sim.houses.get_mut(&ann).unwrap().mark_defeated();
    let first = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            33,
            TickLane::Ordinary,
            &mut FrameEffects::empty(),
        )
        .unwrap();
    assert!(first.tick.frame_committed);
    assert_eq!(sim.session.current_house(), Some(ann));
    assert!(sim.ready_outcome_for_owner(ann).is_none());
    assert!(sim.network_match_winners().is_none());
    assert_eq!(sim.houses.len(), 3);
    assert_eq!(crate::sim::crates::human_player_count(&sim), 2);

    sim.houses.get_mut(&computer).unwrap().mark_defeated();
    sim.houses
        .get_mut(&bob)
        .unwrap()
        .supply_result_flags_for_test(true, false, CdTimer::default());
    let last = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            33,
            TickLane::Ordinary,
            &mut FrameEffects::empty(),
        )
        .unwrap();
    assert!(!last.tick.frame_committed);
    assert_eq!(sim.network_match_winners(), Some(vec![bob]));
    assert_eq!(
        sim.ready_outcome_for_owner(ann).unwrap().kind,
        HouseOutcomeKind::Defeat
    );
}

#[test]
fn a_watcher_admits_scripted_win_and_loss_while_hostile_houses_remain() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::rule_policy::RulePolicy;
    use crate::sim::house_state::HouseOutcomeKind;
    use crate::skirmish_modes::MpModeClass;

    let mode = ScenarioGameMode::Wol;
    let cooperative = Some(MpModeClass::Cooperative);
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[Americans]\nSide=Allies\n",
    ))
    .unwrap();
    for (class, policy) in [
        (cooperative, mode.stock_rule_policy(cooperative)),
        (None, RulePolicy::stock_campaign()),
    ] {
        for kind in [HouseOutcomeKind::Victory, HouseOutcomeKind::Defeat] {
            let (mut sim, followed, partner, _) = players(mode, true);
            sim.install_session_admission(mode, class, policy);
            sim.session.install_network_timing(10);
            let country = sim.interner.intern("Americans");
            for owner in [followed, partner] {
                sim.houses
                    .get_mut(&owner)
                    .unwrap()
                    .set_country_for_test(Some(country));
            }
            let outcome = if class.is_some() {
                PlayerOutcomeRequest::Country {
                    country_index: 0,
                    win_on_match: kind == HouseOutcomeKind::Victory,
                }
            } else {
                PlayerOutcomeRequest::Flag {
                    kind,
                    skip_timer: true,
                }
            };
            assert!(sim.request_player_outcome(outcome, &rules, &mut FrameEffects::empty(),));
            assert!(sim.network_match_winners().is_none());
            let timer = sim.houses[&followed].outcome_timer();
            sim.session.set_binary_frame_for_test(
                timer.start_frame().wrapping_add(timer.duration()) as u32,
            );
            sim.house_rung(&rules, None, false, &mut FrameEffects::empty(), None)
                .unwrap();
            assert_eq!(
                sim.ready_outcome_for_owner(followed)
                    .map(|result| result.kind),
                Some(kind)
            );
        }
    }
}

#[test]
fn an_eliminated_cooperative_viewpoint_shares_the_survivors_victory() {
    use crate::sim::house_state::HouseOutcomeKind;
    use crate::skirmish_modes::MpModeClass;

    let mode = ScenarioGameMode::Wol;
    let class = Some(MpModeClass::Cooperative);
    let (mut sim, ann, bob, computer) = players(mode, true);
    sim.install_session_admission(mode, class, mode.stock_rule_policy(class));
    sim.session.install_network_timing(10);
    sim.session.set_binary_frame_for_test(1);
    for (owner, has_unit) in [(ann, 0), (bob, 1), (computer, 1)] {
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .tracking_mut_for_test()
            .set_for_test(has_unit, &[], (0, 0, 0), &[]);
    }
    sim.check_defeat(None, None);
    assert!(sim.houses[&ann].is_defeated());
    assert!(sim.ready_outcome_for_owner(ann).is_none());

    sim.houses
        .get_mut(&computer)
        .unwrap()
        .tracking_mut_for_test()
        .set_for_test(0, &[], (0, 0, 0), &[]);
    sim.check_defeat(None, None);
    assert_eq!(sim.network_match_winners(), Some(vec![ann, bob]));
    let timer = sim.houses[&ann].outcome_timer();
    sim.session
        .set_binary_frame_for_test(timer.start_frame().wrapping_add(timer.duration()) as u32);
    sim.check_defeat(None, None);
    assert_eq!(
        sim.ready_outcome_for_owner(ann).map(|result| result.kind),
        Some(HouseOutcomeKind::Victory)
    );
}
