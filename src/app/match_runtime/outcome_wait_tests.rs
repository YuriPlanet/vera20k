//! App's shared Vox wait clock gates the production House continuation.

use super::super::scenario_exit::ScenarioOutcomeVoiceWait;
use crate::sim::house_state::{HouseOutcomeKind, HouseState};
use crate::sim::intern::InternedId;
use crate::sim::runtime::SimRuntime;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{FrameEffects, OutcomeWaitBoundary, Simulation, TickLane};
use std::cell::{Cell, RefCell};

struct PausedOutcome<'a, 'b> {
    client: &'a super::FrameClientBoundary<'b>,
    elapsed: &'a RefCell<&'b mut super::super::frame_pacer::ScenarioElapsedClock>,
    vox: &'a ServicedVox,
    cue_seen: &'a Cell<bool>,
}

impl crate::sim::world::SoundBoundary for PausedOutcome<'_, '_> {
    fn consume(
        &self,
        sim: &Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        events: &[crate::sim::world::SimSoundEvent],
    ) {
        use crate::sim::world::SimSoundEvent;
        if events[self.client.delivered()..]
            .iter()
            .any(|event| matches!(event, SimSoundEvent::MatchOutcome { .. }))
        {
            // Sampling a future stop detects a still-armed elapsed source.
            // The pause must already precede the very first outcome cue.
            assert_eq!(self.elapsed.borrow_mut().stop_buckets(120 << 4), 0);
            self.cue_seen.set(true);
        }
        self.client.consume(sim, rules, events);
    }

    fn delivered(&self) -> usize {
        crate::sim::world::SoundBoundary::delivered(self.client)
    }
}

impl OutcomeWaitBoundary for PausedOutcome<'_, '_> {
    fn apply_prefix_effect(&self, effect: &crate::sim::trigger_runtime::TriggerEffect) -> bool {
        self.client
            .apply_prefix_effect_at(effect, self.vox.now_ms.get())
    }

    fn wait_complete(&self, owner: InternedId, kind: HouseOutcomeKind) -> bool {
        assert!(self.cue_seen.get());
        self.vox.wait_complete(owner, kind)
    }
}

struct ServicedVox {
    now_ms: Cell<u64>,
    pending: RefCell<Option<ScenarioOutcomeVoiceWait>>,
}

impl OutcomeWaitBoundary for ServicedVox {
    fn wait_complete(&self, _owner: InternedId, kind: HouseOutcomeKind) -> bool {
        ScenarioOutcomeVoiceWait::poll_pending(
            &mut self.pending.borrow_mut(),
            kind,
            self.now_ms.get(),
            true,
        )
    }
}

#[test]
fn shared_outcome_clock_resumes_the_same_house_at_120_buckets_once() {
    let mut sim = Simulation::with_seed(0x004F_860B);
    let owner = sim.intern("Human");
    let mut house = HouseState::new(owner, 0, None, true, 0, 10);
    assert!(house.accept_win_result(0, 0, false));
    sim.houses_mut_for_test().register(house);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let mut runtime = SimRuntime::from_simulation(sim);
    let vox = ServicedVox {
        now_ms: Cell::new(15),
        pending: RefCell::new(None),
    };
    let first = runtime
        .advance_frame(
            &[],
            67,
            TickLane::Ordinary,
            FrameEffects::empty().with_outcome_wait(&vox),
        )
        .unwrap();
    assert!(first.outcome_waiting);
    assert!(first.lifecycle_outputs.is_empty());
    assert!(runtime.simulation.houses()[&owner].has_won());
    assert!(runtime.simulation.ready_outcome_for_owner(owner).is_none());
    assert_eq!(runtime.simulation.session().tick(), 0);

    // The event loop may service audio/network repeatedly. It must preserve
    // the first absolute anchor while the same native House is suspended.
    vox.now_ms.set((120 << 4) - 1);
    assert!(
        runtime
            .resume_outcome_wait(FrameEffects::empty().with_outcome_wait(&vox))
            .unwrap()
            .outcome_waiting
    );
    vox.now_ms.set(120 << 4);
    let completed = runtime
        .resume_outcome_wait(FrameEffects::empty().with_outcome_wait(&vox))
        .unwrap();
    assert!(!completed.outcome_waiting);
    assert!(vox.pending.borrow().is_none());
    assert!(!runtime.simulation.houses()[&owner].has_won());
    assert_eq!(
        runtime
            .simulation
            .ready_outcome_for_owner(owner)
            .map(|outcome| outcome.kind),
        Some(HouseOutcomeKind::Victory)
    );
    assert!(!completed.tick.frame_committed);
    assert_eq!(runtime.simulation.session().tick(), 0);
    assert!(
        runtime
            .resume_outcome_wait(FrameEffects::empty().with_outcome_wait(&vox))
            .is_err()
    );
}

#[test]
fn action110_stops_elapsed_before_action67_cue_and_house_wait() {
    use crate::app::match_runtime::frame_pacer::ScenarioElapsedClock;
    use crate::app::match_runtime::scenario_pause::ScenarioPauseClock;
    use crate::audio::events::SoundEventQueue;
    use crate::map::map_file::MapFile;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Fight.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    let mut sim = Simulation::with_seed(110);
    let owner = sim.intern("Player");
    let country = sim.intern("Russians");
    sim.houses_mut_for_test().register(HouseState::new(
        owner,
        rules.country_side_index_raw("Russians").unwrap(),
        Some(country),
        true,
        0,
        10,
    ));
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let map = MapFile::from_bytes(b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n[Triggers]\nT=Russians,<none>,Pause then Win,0,1,1,1,0\n[Events]\nT=1,13,0,0\n[Actions]\nT=2,110,0,30,0,0,0,0,A,67,0,0,0,0,0,0,A\n[Tags]\nTAG=0,Pause then Win,T\n").unwrap();
    sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
    sim.classify_map_triggers(&rules);
    sim.publish_scenario_started();
    let mut runtime = SimRuntime::from_simulation(sim);
    runtime.resources.rules = rules;
    let mut pause = ScenarioPauseClock::default();
    let mut elapsed_owner = ScenarioElapsedClock::new();
    elapsed_owner.start(0);
    let mut sounds = SoundEventQueue::new();
    let mut radar = |_, _, _: &crate::rules::ruleset::RuleSet| false;
    let vox = ServicedVox {
        now_ms: Cell::new(15),
        pending: RefCell::new(None),
    };
    let cue_seen = Cell::new(false);
    let first = {
        let elapsed = RefCell::new(&mut elapsed_owner);
        let client = super::FrameClientBoundary::new(None, Some("Player"), &mut sounds, &mut radar)
            .with_scenario_pause(&mut pause, &elapsed, ScenarioGameMode::Skirmish, false);
        let observed = PausedOutcome {
            client: &client,
            elapsed: &elapsed,
            vox: &vox,
            cue_seen: &cue_seen,
        };
        runtime
            .advance_frame(
                &[],
                67,
                TickLane::Ordinary,
                FrameEffects::for_sound(&observed).with_outcome_wait(&observed),
            )
            .unwrap()
    };
    assert!(first.outcome_waiting);
    assert!(runtime.simulation.session().scenario_paused());
    assert!(runtime.simulation.houses()[&owner].has_won());
    assert!(runtime.simulation.ready_outcome_for_owner(owner).is_none());
    vox.now_ms.set(120 << 4);
    let finished = {
        let elapsed = RefCell::new(&mut elapsed_owner);
        let client = super::FrameClientBoundary::new(None, Some("Player"), &mut sounds, &mut radar)
            .with_scenario_pause(&mut pause, &elapsed, ScenarioGameMode::Skirmish, false);
        let observed = PausedOutcome {
            client: &client,
            elapsed: &elapsed,
            vox: &vox,
            cue_seen: &cue_seen,
        };
        runtime
            .resume_outcome_wait(FrameEffects::for_sound(&observed).with_outcome_wait(&observed))
            .unwrap()
    };
    assert!(!finished.outcome_waiting);
    assert_eq!(elapsed_owner.stop_buckets(120 << 4), 0);
    assert!(runtime.simulation.ready_outcome_for_owner(owner).is_some());
    assert!(
        !finished.trigger_effects.iter().any(|effect| matches!(
            effect,
            crate::sim::trigger_runtime::TriggerEffect::ScenarioPause { .. }
        )),
        "handled pause cannot re-arm after wait"
    );
}
