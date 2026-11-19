//! One auto-play session: the host, and the presentation its orders owe the
//! player.
//!
//! Auto play is the player's other hand, and the rule is that the interface
//! must not be able to tell the two apart. A click writes more than a command:
//! a PRODUCE press sets the cameo's `request1` and its predicted clock, and a
//! refused order says so. Auto play therefore may not simply queue commands —
//! it must queue them *and* write the same presentation, and that pairing
//! belongs in one place.
//!
//! This is that place. It is the only code that steps the host, and it mirrors
//! every admitted command before handing it back to the frame. A command with a
//! visible half that is missing from [`cameo_mirror`] shows nothing, and
//! [`cameo_mirror`] is small enough to read in one sitting.

use std::collections::BTreeSet;

use crate::rules::ruleset::RuleSet;
use crate::sim::autoplay::AutoPlayHost;
use crate::sim::command::Command;
use crate::sim::control::board::AutoPlayDomains;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

use crate::app::sidebar_projection::SidebarProjectionState;

/// A host plus the presentation duty that comes with it.
pub(crate) struct AutoplaySession {
    host: AutoPlayHost,
}

impl AutoplaySession {
    pub(crate) fn new(host: AutoPlayHost) -> Self {
        Self { host }
    }

    /// Which halves of the work the session lets through now.
    pub(crate) fn domains(&self) -> AutoPlayDomains {
        self.host.domains()
    }

    pub(crate) fn set_domains(&mut self, domains: AutoPlayDomains) {
        self.host.set_domains(domains);
    }

    /// The brain driving the house, for the HUD line.
    pub(crate) fn policy_name(&self) -> &'static str {
        self.host.policy_name()
    }

    /// The house every command is stamped with.
    pub(crate) fn owner(&self) -> InternedId {
        self.host.owner()
    }

    /// The objects the player has taken back.
    pub(crate) fn take_back(&mut self, taken: BTreeSet<u64>) {
        self.host.take_back(taken);
    }

    /// One frame of auto play: what the host asks for, mirrored into the strip.
    ///
    /// Returns the commands the gate admitted, in issue order, for the caller's
    /// diagnostics. The simulation is never written here; every answer leaves as
    /// a command through the same ingress a player's click uses.
    pub(crate) fn step(
        &mut self,
        sim: &mut Simulation,
        rules: &RuleSet,
        projection: &mut SidebarProjectionState,
    ) -> Vec<Command> {
        // The player's own units: a unit in one of the player's control groups
        // stays under the player's hand. Read from the sim, where an assignment
        // lands, so a group assigned in this frame's commands counts before
        // auto play steps.
        let player_owned: BTreeSet<u64> = sim
            .projected_control_groups()
            .into_iter()
            .flatten()
            .collect();
        self.host.take_back(player_owned);

        let issued = self.host.step(sim, rules);
        let owner = self.host.owner();
        for command in &issued {
            projection.mirror_auto_play(sim, rules, owner, command);
        }
        issued
    }
}
