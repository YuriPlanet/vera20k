//! The scripted brain: a fixed list of commands released by frame number.
//!
//! It is a schedule, not a plan — it never reads the observation. It exists so
//! the actuation path can be built and tested against something, and it is the
//! simplest thing that can occupy the brain seam. Because its steps are
//! commands rather than goals, it uses the outbox's command channel: the gate
//! (domain switch, budget, ownership, order admission) applies to it exactly as
//! it does to an assist brain's orders, which is the point of keeping it.

use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::control::brain::{Brain, Outbox};
use crate::sim::control::observation::Observation;

/// One command, released at one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptStep {
    /// The command is released on the first frame whose tick reaches this.
    pub frame: u64,
    pub command: Command,
}

/// A brain that plays a fixed script.
#[derive(Debug, Clone, Default)]
pub struct ScriptedPolicy {
    steps: Vec<ScriptStep>,
    next: usize,
}

impl ScriptedPolicy {
    /// A script in any order; it is sorted by frame with a stable sort, so the
    /// release order of same-frame steps is the order they were given.
    pub fn new(steps: Vec<ScriptStep>) -> Self {
        let mut steps = steps;
        steps.sort_by_key(|step| step.frame);
        Self { steps, next: 0 }
    }

    pub fn from_pairs(pairs: impl IntoIterator<Item = (u64, Command)>) -> Self {
        Self::new(
            pairs
                .into_iter()
                .map(|(frame, command)| ScriptStep { frame, command })
                .collect(),
        )
    }

    /// Steps not yet released.
    pub fn pending(&self) -> &[ScriptStep] {
        &self.steps[self.next..]
    }
}

impl Brain for ScriptedPolicy {
    fn name(&self) -> &'static str {
        "scripted"
    }

    fn think(&mut self, _rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox) {
        let tick = obs.tick();
        while let Some(step) = self.steps.get(self.next) {
            if step.frame > tick {
                break;
            }
            out.command(step.command.clone());
            self.next += 1;
        }
    }
}
