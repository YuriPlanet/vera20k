//! Production House rung -> local input -> post-Logic command transport.
//! The authored objects use the layered retail reader. Native637A10's narrow
//! executable controls are separate from this runtime integration evidence.

use super::FrameClientBoundary;
use crate::app::input::planning_mode::PlanningInputState;
use crate::audio::events::SoundEventQueue;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::selection::SelectionState;
use crate::sim::world::{
    FrameEffects, SimSoundEvent, Simulation, SoundBoundary, TickLane, UninitContext,
};
use std::cell::RefCell;

/// Observe the existing queue before the production dispatcher appends the
/// outcome EVA. Delivery is delegated unchanged; no device is constructed.
struct OutcomeCueBoundary<'a, 'b> {
    client: &'a FrameClientBoundary<'b>,
    outcomes: RefCell<Vec<(bool, Vec<String>)>>,
}

impl SoundBoundary for OutcomeCueBoundary<'_, '_> {
    fn consume(
        &self,
        sim: &Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        events: &[SimSoundEvent],
    ) {
        if events[self.client.delivered()..].iter().any(|event| {
            matches!(event, SimSoundEvent::MatchOutcome { owner, .. } if Some(*owner) == sim.session.current_house())
        }) {
            let active = self.client.planning.as_ref().unwrap().borrow().0.active();
            let queued = self.client.output.borrow().iter().filter_map(|event| match event {
                crate::audio::events::GameSoundEvent::UiSound { sound_id } => Some(sound_id.clone()),
                _ => None,
            }).collect();
            self.outcomes.borrow_mut().push((active, queued));
        }
        self.client.consume(sim, rules, events);
    }

    fn delivered(&self) -> usize {
        self.client.delivered()
    }
}

fn retail_runtime() -> Option<(SimRuntime, [u64; 2])> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")?;
    let rules = &retail.rules;
    let mut sim = Simulation::with_seed(0x004F_C1EE);
    // ReplayRunner returns a hash on every frame; record the same cadence.
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().short_game = true;
    for (country, human) in [("Americans", true), ("British", false)] {
        let owner = sim.intern(country);
        let side = rules.country_side_index(country).unwrap().0;
        let mut house = HouseState::new(owner, side, Some(owner), human, 50_000, 10);
        house.project_country_mults(rules, &sim.interner);
        sim.houses.register(house);
        if human {
            sim.session.set_current_house_for_test(Some(owner));
        }
    }
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    let yards = [("Americans", 6, 6), ("British", 20, 20)].map(|(owner, x, y)| {
        sim.spawn_object("GACNST", owner, x, y, 0, rules)
            .expect("stock last construction yard")
    });
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let mut resources = SimResources::empty();
    resources.rules = retail.rules;
    resources.overlay_registry = overlays;
    Some((
        SimRuntime {
            simulation: sim,
            resources,
        },
        yards,
    ))
}

#[test]
fn retail_local_defeat_leaves_planning_in_the_same_house_frame() {
    let Some((mut rt, yards)) = retail_runtime() else {
        return;
    };
    let mut recorded = ReplayLog::new(ReplayHeader {
        version: 1,
        tick_hz: 15,
        seed: rt.simulation.session.seed(),
        map_name: "retail-local-defeat".into(),
        rules_hash: rt.resources.rules.simulation_config_hash(),
        pixel_conversion_bounds: rt.simulation.session.pixel_conversion_bounds(),
    });
    let mut planning = PlanningInputState::default();
    let mut selection = SelectionState::new();
    let mut sounds = SoundEventQueue::new();
    assert!(
        planning
            .set_active(
                true,
                Some(&rt.simulation),
                Some(&rt.resources.rules),
                &mut selection,
                &mut sounds,
            )
            .is_none()
    );
    let _ = sounds.drain();
    planning.node_deleted(0, Some(73));
    selection.begin_drag(10., 20.);
    selection.update_drag(100., 120.);

    // UnInit is a real lifecycle call, not a forged defeat/count flag. The
    // pending destructor drain and the next House gate decide the loss.
    rt.simulation.uninit_with_context(
        yards[0],
        UninitContext::new(
            Some(&rt.resources.rules),
            Some(&rt.resources.overlay_registry),
        ),
    );
    let mut commits = Vec::new();
    let mut defeat_tick = None;
    let mut outcome_cues = Vec::new();
    for _ in 0..5 {
        let tick = rt.simulation.session.tick();
        let was_defeated = rt.simulation.local_player_defeated();
        let mut admit_radar = |_, _, _: &crate::rules::ruleset::RuleSet| false;
        let mut output = {
            let boundary =
                FrameClientBoundary::new(None, Some("Americans"), &mut sounds, &mut admit_radar)
                    .with_planning(&mut planning, &mut selection);
            let observed = OutcomeCueBoundary {
                client: &boundary,
                outcomes: RefCell::new(Vec::new()),
            };
            let output = rt
                .advance_frame(
                    &[],
                    67,
                    TickLane::Ordinary,
                    FrameEffects::for_sound(&observed).with_local_defeat(&boundary),
                )
                .expect("ordinary retail defeat frame");
            outcome_cues.extend(observed.outcomes.into_inner());
            output
        };
        if !was_defeated && rt.simulation.local_player_defeated() {
            assert!(
                !planning.active(),
                "House4FC1EE must leave the local planner before returning"
            );
            assert_eq!(planning.highlighted_node(), None);
            assert!(!selection.is_dragging());
            assert!(
                rt.simulation.houses[&rt.simulation.session.current_house().unwrap()].has_lost()
            );
            assert!(
                !rt.simulation.houses[&rt.simulation.session.current_house().unwrap()]
                    .outcome()
                    .unwrap()
                    .exit_ready
            );
            defeat_tick = Some(tick);
        }
        let admitted = output.take_admitted_commands();
        recorded.record_tick(tick, admitted.clone(), output.tick.state_hash);
        for command in admitted {
            if matches!(command.payload, Command::PlanningRecord { .. }) {
                commits.push((tick, command));
            }
        }
    }
    let tick = defeat_tick.expect("last yard destruction must reach the real House defeat gate");
    assert_eq!(commits.len(), 1, "later House visits must not commit again");
    let (dispatch_tick, commit) = &commits[0];
    assert_eq!(
        *dispatch_tick, tick,
        "the commit joins the same frame's post-Logic dispatch"
    );
    assert_eq!(commit.execute_tick, tick);
    assert_eq!(Some(commit.owner), rt.simulation.session.current_house());
    let Command::PlanningRecord { record } = &commit.payload else {
        unreachable!()
    };
    assert_eq!(record.opcode(), 0x2B);
    assert_eq!(
        outcome_cues,
        [(false, vec!["PlanningModeEnd".into()])],
        "the real loss boundary must see End queued before outcome EVA"
    );
    assert!(rt.simulation.pending_command_snapshot().is_empty());

    // Press/toggle cannot re-enter during SavourDelay. A release is harmless
    // once637A10 cleared active, and must not enqueue a duplicate commit/cue.
    for active in [true, false, true, false] {
        assert!(
            planning
                .set_active(
                    active,
                    Some(&rt.simulation),
                    Some(&rt.resources.rules),
                    &mut selection,
                    &mut sounds,
                )
                .is_none()
        );
        assert!(!planning.active());
    }
    assert!(sounds.is_empty());

    let (mut replay, replay_yards) = retail_runtime().unwrap();
    assert_eq!(replay_yards, yards);
    replay.simulation.uninit_with_context(
        replay_yards[0],
        UninitContext::new(
            Some(&replay.resources.rules),
            Some(&replay.resources.overlay_registry),
        ),
    );
    let hashes = ReplayRunner::run_runtime(&mut replay, &recorded, 67).unwrap();
    assert_eq!(
        hashes,
        recorded
            .ticks
            .iter()
            .map(|frame| frame.state_hash)
            .collect::<Vec<_>>(),
        "headless playback consumes the recorded commit without client input"
    );
    assert_eq!(
        replay.simulation.pending_command_snapshot(),
        rt.simulation.pending_command_snapshot()
    );
}

#[test]
fn retail_inactive_and_nonlocal_defeat_do_not_leave_the_local_planner() {
    for (losing_house, active) in [(0, false), (1, false), (1, true)] {
        let Some((mut rt, yards)) = retail_runtime() else {
            return;
        };
        let mut planning = PlanningInputState::default();
        planning.restore_active(active);
        planning.node_deleted(0, Some(73));
        let mut selection = SelectionState::new();
        let mut sounds = SoundEventQueue::new();
        rt.simulation.uninit_with_context(
            yards[losing_house],
            UninitContext::new(
                Some(&rt.resources.rules),
                Some(&rt.resources.overlay_registry),
            ),
        );
        let mut admitted = Vec::new();
        for _ in 0..5 {
            let mut admit_radar = |_, _, _: &crate::rules::ruleset::RuleSet| false;
            let mut output = {
                let boundary = FrameClientBoundary::new(
                    None,
                    Some("Americans"),
                    &mut sounds,
                    &mut admit_radar,
                )
                .with_planning(&mut planning, &mut selection);
                rt.advance_frame(
                    &[],
                    67,
                    TickLane::Ordinary,
                    FrameEffects::for_sound(&boundary).with_local_defeat(&boundary),
                )
                .unwrap()
            };
            admitted.extend(output.take_admitted_commands());
        }
        let loser = rt.simulation.houses.order()[losing_house];
        assert!(rt.simulation.houses[&loser].is_defeated);
        assert_eq!(planning.active(), active);
        assert_eq!(
            planning.highlighted_node(),
            Some(73),
            "inactive637A10 and a remote4FC0B0 must preserve local references"
        );
        assert!(
            !admitted
                .iter()
                .any(|command| matches!(command.payload, Command::PlanningRecord { .. }))
        );
        assert!(sounds.is_empty());
    }
}
