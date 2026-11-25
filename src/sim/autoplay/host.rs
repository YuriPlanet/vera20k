//! The auto play facade: the one type the client holds.
//!
//! Behind it are three layers — [`crate::sim::behavior`] (execute),
//! [`crate::sim::control`] (store, schedule, deliver) and
//! [`crate::sim::strategy`] (decide) — and this file exists so the client never
//! has to know that. The app installs a host, steps it once a frame before the
//! frame drains its commands, and reads a name, a switch and a rejection count
//! for the HUD. Nothing in the client names a layer, a brain or a routine.
//!
//! Keeping the name and the shape is deliberate: a hotkey, a match-state field
//! and an initialiser already speak `AutoPlayHost`, `AutoPlayDomains` and
//! `step`, and the layers under them may be re-arranged without touching any of
//! those three.

use crate::rules::ruleset::RuleSet;
use crate::sim::behavior::managed::{MicroKind, MicroParams};
use crate::sim::command::Command;
use crate::sim::control::board::{AutoPlayDomains, ControlBoard};
use crate::sim::control::brain::Brain;
use crate::sim::control::micro::MicroTable;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// Drives one brain for one house.
pub struct AutoPlayHost {
    board: ControlBoard,
}

impl AutoPlayHost {
    pub fn new(brain: Box<dyn Brain>, owner: InternedId, max_per_frame: usize) -> Self {
        Self {
            board: ControlBoard::new(brain, owner, max_per_frame),
        }
    }

    /// Which halves of the work the host lets through now.
    pub fn domains(&self) -> AutoPlayDomains {
        self.board.domains()
    }

    /// Flip the switches. A kind turned off from here on issues nothing more of
    /// its own; what is already queued in the simulation keeps running.
    pub fn set_domains(&mut self, domains: AutoPlayDomains) {
        self.board.set_domains(domains);
    }

    /// The player's control-group members, by stable id. The fifth key decides
    /// whether these members or the ungrouped units are automated.
    pub fn excluded(&self) -> &std::collections::BTreeSet<u64> {
        self.board.excluded()
    }

    /// Update membership of the player's numbered control groups. The board
    /// alone selects the eligible half according to its fifth-key scope.
    pub fn take_back(&mut self, groups: std::collections::BTreeSet<u64>) {
        self.board.take_back(groups);
    }

    /// The house every issued command is stamped with. Not necessarily the
    /// local player's: a computer house can be driven the same way.
    pub fn owner(&self) -> InternedId {
        self.board.owner()
    }

    /// The brain driving the house, for diagnostics and the HUD line.
    pub fn policy_name(&self) -> &'static str {
        self.board.brain_name()
    }

    /// Commands the gate has dropped since the host was built. Diagnostics
    /// only: the queue is the authority, not this count.
    pub fn rejected(&self) -> usize {
        self.board.rejected()
    }

    /// Orders the funnel called not-ready: the object was mid-transition and
    /// nothing was written. Retried next frame by the issuer.
    pub fn not_ready(&self) -> usize {
        self.board.not_ready()
    }

    /// One frame of auto play. Call it before the frame advances.
    ///
    /// Returns what the frame queued, in issue order. The client uses it to
    /// mirror the presentation writes a player's press makes for the same
    /// order (the cameo's `request1` and predicted clock).
    pub fn step(&mut self, sim: &mut Simulation, rules: &RuleSet) -> Vec<Command> {
        self.board.step(sim, rules)
    }

    /// Put an object under a managed routine (non-parity micro, see
    /// `sim::behavior::managed`). `false` when it is already managed.
    pub fn attach(
        &mut self,
        entity: u64,
        kind: MicroKind,
        params: MicroParams,
        post: Option<(u16, u16)>,
    ) -> bool {
        self.board.attach(entity, kind, params, post)
    }

    /// Return an object to the brain's direct orders.
    pub fn detach(&mut self, entity: u64) -> bool {
        self.board.detach(entity)
    }

    /// The managed table, for diagnostics and tests.
    pub fn managed(&self) -> &MicroTable {
        self.board.managed()
    }
}
