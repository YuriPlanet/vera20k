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

impl super::ScenarioSession {
    pub const fn scenario_paused(&self) -> bool {
        self.scenario_pause.depth != 0
    }

    pub const fn scenario_pause_timed(&self) -> bool {
        self.scenario_pause.timed
    }
}

impl Simulation {
    /// Original684060. A repeated nonzero request still rearms the app
    /// timer, but630 prevents acquiring a second timed hold. The outermost
    /// entry synchronously locks35A2 before the next authored action runs.
    pub fn pause_scenario(&mut self, duration: i32) -> Vec<TriggerEffect> {
        let pause = &mut self.session.scenario_pause;
        if duration != 0 {
            if pause.timed {
                return vec![TriggerEffect::ScenarioPause {
                    duration,
                    outermost: false,
                }];
            }
            pause.timed = true;
        }
        let outermost = pause.depth == 0;
        pause.depth = pause.depth.wrapping_add(1);
        let mut effects = vec![TriggerEffect::ScenarioPause {
            duration,
            outermost,
        }];
        if outermost {
            pause.prior_input_locked = self.session.input_locked;
            if !pause.prior_input_locked {
                self.session.input_locked = true;
                effects.push(TriggerEffect::InputLockChanged {
                    locked: true,
                    was_locked: false,
                });
            }
        }
        effects
    }

    /// Original684180. Only the last release clears a lock this pause acquired. An
    /// explicit46/47 during the hold does not replace native B054CC.
    pub fn resume_scenario(&mut self) -> Vec<TriggerEffect> {
        let pause = &mut self.session.scenario_pause;
        if pause.depth == 0 {
            return Vec::new();
        }
        pause.depth = pause.depth.wrapping_sub(1);
        if pause.depth != 0 {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if !pause.prior_input_locked {
            let was_locked = self.session.input_locked;
            self.session.input_locked = false;
            effects.push(TriggerEffect::InputLockChanged {
                locked: false,
                was_locked,
            });
        }
        effects.push(TriggerEffect::ScenarioResume);
        effects
    }

    /// Main_Tick55D815 clears630 before calling the shared684180 owner.
    /// The app has already observed620's expiry in offline modes0/5.
    pub fn expire_scenario_pause(&mut self) -> Vec<TriggerEffect> {
        if !self.session.scenario_pause.timed {
            return Vec::new();
        }
        self.session.scenario_pause.timed = false;
        self.resume_scenario()
    }
}
