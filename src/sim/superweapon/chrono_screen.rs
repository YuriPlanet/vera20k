//! ChronoScreenEffect53B460/53B560/53BAD0, reached by TAction127.
//! State belongs to ScenarioSession; the existing53A747 pre-object rung is
//! its only clock. Commands carry presentation/audio boundaries, not a second
//! scheduler. Original executed controls: tools/chrono_screen_effect.json.

use crate::sim::trigger_runtime::TriggerEffect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChronoScreenCommand {
    Start,
    Draw { step: i32, duration: i32 },
    White,
    Pause,
    CaptureDestination,
    Finish,
}

/// Native A9FAB0/FA98/FAA8/FAD1. The surface and VocHandle are app resources.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct ChronoScreenState {
    phase: u8,
    remaining: i32,
    duration: i32,
    prior_input_locked: bool,
}

impl ChronoScreenState {
    pub(crate) const fn active(&self) -> bool {
        self.phase != 0
    }

    pub(crate) fn start(&mut self, duration: i32, prior_input_locked: bool) -> bool {
        if duration <= 0 {
            return false;
        }
        *self = Self {
            phase: 1,
            remaining: duration,
            duration,
            prior_input_locked,
        };
        true
    }

    pub(crate) fn step(&mut self) -> Vec<ChronoScreenCommand> {
        use ChronoScreenCommand::*;
        let mut commands = Vec::new();
        match self.phase {
            1 | 3 => {
                let step = if self.phase == 1 {
                    self.duration.wrapping_sub(self.remaining)
                } else {
                    self.remaining
                };
                commands.push(Draw {
                    step,
                    duration: self.duration,
                });
                self.remaining = self.remaining.wrapping_sub(1);
                if self.remaining == 0 {
                    if self.phase == 1 {
                        self.phase = 2;
                        self.remaining = 45; //53B597 literal, executed controls.
                    } else {
                        self.phase = 0;
                        commands.push(Finish);
                    }
                }
            }
            2 => {
                commands.push(White);
                self.remaining = self.remaining.wrapping_sub(1);
                if self.remaining == 1 {
                    commands.push(Pause);
                }
                if self.remaining == 0 {
                    self.remaining = self.duration;
                    self.phase = 3;
                    commands.push(CaptureDestination);
                }
            }
            _ => {}
        }
        commands
    }

    pub(crate) const fn prior_input_locked(&self) -> bool {
        self.prior_input_locked
    }
}

impl crate::sim::scenario_session::ScenarioSession {
    /// LightningStorm::Process53A747, after Dominator and before storm work.
    /// Original53B560 also runs from Main_Tick55D83A while ordinary Logic
    /// is paused. Return ordered effects to the calling frame/service owner.
    pub(crate) fn process_chrono_screen_effect(&mut self) -> Vec<TriggerEffect> {
        let mut effects = Vec::new();
        let was_locked = self.input_locked();
        let commands = self.step_chrono_screen_state();
        for command in commands {
            match command {
                ChronoScreenCommand::Pause => effects.extend(self.pause_scenario(0)),
                ChronoScreenCommand::Finish => effects.extend(self.resume_scenario()),
                _ => {}
            }
            effects.push(TriggerEffect::ChronoScreen { command });
            if command == ChronoScreenCommand::Finish && !self.chrono_screen().prior_input_locked()
            {
                effects.push(TriggerEffect::InputLockChanged {
                    locked: false,
                    was_locked,
                });
            }
        }
        effects
    }
}

impl crate::sim::scenario_session::ScenarioSession {
    ///53B460 owns admission; Execute127 itself succeeds even for a no-op.
    pub(crate) fn start_chrono_screen_effect(
        &mut self,
        duration: i32,
        effects: &mut Vec<TriggerEffect>,
    ) {
        let was_locked = self.input_locked();
        if !self.start_chrono_screen_state(duration) {
            return;
        }
        effects.push(TriggerEffect::ChronoScreen {
            command: ChronoScreenCommand::Start,
        });
        effects.push(TriggerEffect::InputLockChanged {
            locked: true,
            was_locked,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn assert_state(state: ChronoScreenState, native: &Value) {
        assert_eq!(i64::from(state.phase), native["phase"].as_i64().unwrap());
        assert_eq!(
            i64::from(state.remaining),
            native["remaining"].as_i64().unwrap()
        );
        assert_eq!(
            i64::from(state.duration),
            native["duration"].as_i64().unwrap()
        );
        assert_eq!(state.active(), native["active"].as_bool().unwrap());
    }

    #[test]
    fn chrono_screen_lifecycle_matches_original_process_controls() {
        let oracle: Value =
            serde_json::from_str(crate::test_fixture::text("tools/chrono_screen_effect.json"))
                .unwrap();
        for row in oracle["lifecycle_controls"].as_array().unwrap() {
            let mut state = ChronoScreenState::default();
            let prior = row["scenario_pause_prior"].as_bool().unwrap_or(false);
            let duration = row["duration_input"].as_i64().unwrap() as i32;
            state.start(duration, prior);
            assert_state(state, &row["after_start"]);
            for frame in row["frames"].as_array().unwrap() {
                assert_state(state, &frame["before"]);
                let commands = state.step();
                assert_state(state, &frame["after"]);
                let native_events = frame["events"].as_array().unwrap();
                for (command, name) in [
                    (ChronoScreenCommand::Pause, "midpoint_sink"),
                    (
                        ChronoScreenCommand::CaptureDestination,
                        "snapshot_redraw_sink",
                    ),
                ] {
                    assert_eq!(
                        commands.contains(&command),
                        native_events.iter().any(|event| event["name"] == name)
                    );
                }
                assert_eq!(
                    commands
                        .iter()
                        .any(|command| matches!(command, ChronoScreenCommand::Draw { .. })),
                    native_events
                        .iter()
                        .any(|event| event["name"] == "draw_sink")
                );
            }
            assert_state(state, &row["final"]);
        }
    }
}
