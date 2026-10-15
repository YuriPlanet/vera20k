//! Scenario countdown: original TAction23..27/103 and Event14. Steam gamemd
//! SHA3e81a617..08600, Execute6DE418..6DE6DA, HasOccurred71EBE9 and shared
//! CdTimer426630/46B640. Captions retain the authored key, never localized
//! app text, so language selection cannot affect synchronized state.

use super::ScenarioSession;
use crate::sim::timer::{CdTimer, PAUSED_START_FRAME};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ScenarioTimerAction {
    Start,
    Stop,
    Set(i32),
    Extend(i32),
    Shorten(i32),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct ScenarioCountdown {
    timer: CdTimer,
    caption: String,
}

impl ScenarioCountdown {
    pub(super) fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl ScenarioSession {
    /// Reset683610 leaves the shared timer stopped. Set/extend/shorten use
    /// the original constructor's running timer, even when previously paused.
    /// Native seconds are signed wrapping *15; shortening compares signed
    /// remaining to that delta before choosing zero versus subtraction.
    pub(crate) fn change_scenario_timer(&mut self, action: ScenarioTimerAction) {
        let now = self.binary_frame as i32;
        let state = &mut self.scenario_countdown;
        match action {
            ScenarioTimerAction::Start => state.timer.resume(now),
            ScenarioTimerAction::Stop => state.timer.pause(now),
            ScenarioTimerAction::Set(seconds) => {
                state.timer.start(now, seconds.wrapping_mul(15));
                state.caption.clear();
            }
            ScenarioTimerAction::Extend(seconds) => {
                let frames = state
                    .timer
                    .remaining(now)
                    .wrapping_add(seconds.wrapping_mul(15));
                state.timer.start(now, frames);
            }
            ScenarioTimerAction::Shorten(seconds) => {
                let left = state.timer.remaining(now);
                let delta = seconds.wrapping_mul(15);
                let frames = if left <= delta {
                    0
                } else {
                    left.wrapping_sub(delta)
                };
                state.timer.start(now, frames);
            }
        }
    }

    pub(crate) fn set_scenario_timer_caption(&mut self, caption: &str) {
        self.scenario_countdown.caption.clear();
        self.scenario_countdown.caption.push_str(caption);
    }

    /// Event14 needs a running timer as well as exact-zero remaining time.
    pub(crate) fn scenario_timer_elapsed(&self) -> bool {
        self.scenario_countdown.timer.start_frame() != PAUSED_START_FRAME
            && self
                .scenario_countdown
                .timer
                .expired(self.binary_frame as i32)
    }

    /// Tactical6D4941 draws this line whenever the timer runs, including0.
    pub(crate) fn scenario_timer_view(&self) -> Option<(i32, &str)> {
        let state = &self.scenario_countdown;
        (state.timer.start_frame() != PAUSED_START_FRAME).then(|| {
            (
                state.timer.remaining(self.binary_frame as i32),
                state.caption.as_str(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::Simulation;

    #[test]
    fn scenario_timer_matches_original_executed_histories() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/scenario_timer.json",
        ))
        .unwrap();
        for history in corpus["histories"].as_array().unwrap() {
            let mut sim = Simulation::new();
            let rng = sim.rng_state();
            assert_eq!(history["initial"]["start"], -1);
            assert_eq!(history["initial"]["duration"], 0);
            for step in history["steps"].as_array().unwrap() {
                sim.session.binary_frame = step["frame"].as_u64().unwrap() as u32;
                if let Some(read) = step.get("read") {
                    let seconds = read["value"].as_i64().unwrap() as i32;
                    let action = match read["kind"].as_u64().unwrap() {
                        23 => Some(ScenarioTimerAction::Start),
                        24 => Some(ScenarioTimerAction::Stop),
                        25 => Some(ScenarioTimerAction::Extend(seconds)),
                        26 => Some(ScenarioTimerAction::Shorten(seconds)),
                        27 => Some(ScenarioTimerAction::Set(seconds)),
                        103 => {
                            sim.session
                                .set_scenario_timer_caption(step["label_read"].as_str().unwrap());
                            None
                        }
                        kind => panic!("unhandled native timer action {kind}"),
                    };
                    if let Some(action) = action {
                        sim.session.change_scenario_timer(action);
                    }
                }
                let state = &sim.session.scenario_countdown;
                let expected = &step["state"];
                let context = (&history["name"], &step["name"]);
                assert_eq!(
                    i64::from(state.timer.start_frame()),
                    expected["start"].as_i64().unwrap(),
                    "{context:?}"
                );
                assert_eq!(
                    i64::from(state.timer.duration()),
                    expected["duration"].as_i64().unwrap(),
                    "{context:?}"
                );
                assert_eq!(
                    i64::from(state.timer.remaining(sim.session.binary_frame as i32)),
                    expected["remaining"].as_i64().unwrap(),
                    "{context:?}"
                );
                assert_eq!(
                    state.caption,
                    expected["key"].as_str().unwrap(),
                    "{context:?}"
                );
                assert_eq!(
                    sim.session.scenario_timer_elapsed(),
                    expected["event14"] == 1,
                    "{context:?}"
                );
                assert_eq!(sim.rng_state(), rng, "timer actions pay no RNG draws");
            }
        }
    }

    #[test]
    fn scenario_timer_snapshot_keeps_paused_caption_and_remaining_time() {
        let mut sim = Simulation::new();
        let initial_hash = sim.state_hash();
        sim.session.binary_frame = 123;
        sim.session
            .change_scenario_timer(ScenarioTimerAction::Set(120));
        sim.session.set_scenario_timer_caption("DESC:SOV1");
        sim.session.binary_frame = 423;
        sim.session.change_scenario_timer(ScenarioTimerAction::Stop);
        assert_ne!(sim.state_hash(), initial_hash);
        let bytes = GameSnapshot::save(&sim, 0, 0, "scenario_timer", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            restored.session.scenario_countdown,
            sim.session.scenario_countdown
        );
        restored.session.binary_frame = 5000;
        assert_eq!(restored.session.scenario_timer_view(), None);
        assert!(!restored.session.scenario_timer_elapsed());
        restored
            .session
            .change_scenario_timer(ScenarioTimerAction::Start);
        assert_eq!(
            restored.session.scenario_timer_view(),
            Some((1500, "DESC:SOV1"))
        );
        restored.session.binary_frame = 6500;
        assert!(restored.session.scenario_timer_elapsed());
        restored.session.set_scenario_timer_caption("");
        assert_eq!(restored.session.scenario_timer_view(), Some((0, "")));
    }
}
