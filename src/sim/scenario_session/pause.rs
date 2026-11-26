//! ScenarioPause684060/684180. Scenario62C/630 and B054CC's retained prior
//! input lock have one owner. The app owns only620's system-clock timer;
//! it reports expiry without passing wall time into the simulation.
//! Executed controls: tools/scenario_pause.json, identity in its sidecar.
use crate::sim::trigger_runtime::TriggerEffect;
use crate::sim::world::Simulation;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(super) struct ScenarioPause {
    depth: u32,
    timed: bool,
    prior_input_locked: bool,
}

/// What one ScenarioPause684060/684180 call leaves the app to apply, in that
/// call's order. `Vec::from(change)` is the same as the general effect list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScenarioPauseChange {
    /// A release that leaves an outer credit held, or nothing to release.
    #[default]
    NoEffect,
    /// 684060's pause, then the input lock its outermost entry took.
    Pause {
        duration: i32,
        outermost: bool,
        lock_input: bool,
    },
    /// 684180's last release: the unlock of the lock the outermost entry
    /// took (`Some(was_locked)`), then the resume.
    Resume { unlock_input: Option<bool> },
}

impl From<ScenarioPauseChange> for Vec<TriggerEffect> {
    fn from(change: ScenarioPauseChange) -> Self {
        match change {
            ScenarioPauseChange::NoEffect => Vec::new(),
            ScenarioPauseChange::Pause {
                duration,
                outermost,
                lock_input,
            } => {
                let mut effects = vec![TriggerEffect::ScenarioPause {
                    duration,
                    outermost,
                }];
                if lock_input {
                    effects.push(TriggerEffect::InputLockChanged {
                        locked: true,
                        was_locked: false,
                    });
                }
                effects
            }
            ScenarioPauseChange::Resume { unlock_input } => {
                let mut effects = Vec::new();
                if let Some(was_locked) = unlock_input {
                    effects.push(TriggerEffect::InputLockChanged {
                        locked: false,
                        was_locked,
                    });
                }
                effects.push(TriggerEffect::ScenarioResume);
                effects
            }
        }
    }
}

impl super::ScenarioSession {
    pub const fn scenario_paused(&self) -> bool {
        self.scenario_pause.depth != 0
    }

    pub const fn scenario_pause_timed(&self) -> bool {
        self.scenario_pause.timed
    }
}

impl super::ScenarioSession {
    /// Original684060. A repeated nonzero request still rearms the app
    /// timer, but630 prevents acquiring a second timed hold. The outermost
    /// entry synchronously locks35A2 before the next authored action runs.
    pub(crate) fn pause_scenario(&mut self, duration: i32) -> ScenarioPauseChange {
        let pause = &mut self.scenario_pause;
        if duration != 0 {
            if pause.timed {
                return ScenarioPauseChange::Pause {
                    duration,
                    outermost: false,
                    lock_input: false,
                };
            }
            pause.timed = true;
        }
        let outermost = pause.depth == 0;
        pause.depth = pause.depth.wrapping_add(1);
        let mut lock_input = false;
        if outermost {
            pause.prior_input_locked = self.input_locked;
            if !pause.prior_input_locked {
                self.input_locked = true;
                lock_input = true;
            }
        }
        ScenarioPauseChange::Pause {
            duration,
            outermost,
            lock_input,
        }
    }

    /// Original684180. Only the last release clears a lock this pause acquired. An
    /// explicit46/47 during the hold does not replace native B054CC.
    pub(crate) fn resume_scenario(&mut self) -> ScenarioPauseChange {
        let pause = &mut self.scenario_pause;
        if pause.depth == 0 {
            return ScenarioPauseChange::NoEffect;
        }
        pause.depth = pause.depth.wrapping_sub(1);
        if pause.depth != 0 {
            return ScenarioPauseChange::NoEffect;
        }
        let mut unlock_input = None;
        if !pause.prior_input_locked {
            let was_locked = self.input_locked;
            self.input_locked = false;
            unlock_input = Some(was_locked);
        }
        ScenarioPauseChange::Resume { unlock_input }
    }

    /// Main_Tick55D815 clears630 before calling the shared684180 owner.
    /// The app has already observed620's expiry in offline modes0/5.
    pub(crate) fn expire_scenario_pause(&mut self) -> ScenarioPauseChange {
        if !self.scenario_pause.timed {
            return ScenarioPauseChange::NoEffect;
        }
        self.scenario_pause.timed = false;
        self.resume_scenario()
    }
}

/// The app's entry point; the session owns the pause.
impl Simulation {
    /// [`ScenarioSession::resume_scenario`](super::ScenarioSession::resume_scenario).
    pub fn resume_scenario(&mut self) -> ScenarioPauseChange {
        self.session.resume_scenario()
    }
}
