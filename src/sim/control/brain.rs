//! The decision port: what a brain is, and the box it answers into.
//!
//! The contract is declared here, on L2's side, so the dependency arrow points
//! one way only: a brain is something L2 drives, and L2 never names a concrete
//! brain. [`crate::sim::strategy`] implements it; an external controller can
//! implement it too without touching anything else (see
//! [`super::external`]).
//!
//! A brain is **read-only over the simulation**. It sees
//! [`Observation`](super::observation::Observation), returns
//! [`Order`](super::intent::Order)s and [`Intent`](super::intent::Intent)s, and
//! writes nothing: the control layer applies what it asks for through the
//! owning modules, in one place, with one admission rule. That is what lets a
//! second brain — a script, a replay, a network — drop in without a second
//! implementation of "may I issue this".

use std::collections::BTreeMap;

use crate::rules::ruleset::RuleSet;
use crate::sim::behavior::goal::Goal;
use crate::sim::behavior::managed::{MicroKind, MicroParams};
use crate::sim::command::Command;
use crate::sim::control::intent::{Intent, Order};
use crate::sim::control::observation::Observation;

/// A request to hand one object to a managed routine.
///
/// A brain decides *what* an object should be doing; a routine in
/// [`crate::sim::behavior::managed`] owns *how* it does it across frames (backing
/// off a reload, waiting out a patrol). The table of attached objects is L2's,
/// so a brain cannot attach one itself and asks through this box instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attachment {
    /// The object the routine takes over.
    pub actor: u64,
    pub kind: MicroKind,
    pub params: MicroParams,
    /// A cell the routine works from or towards, when one applies.
    pub post: Option<(u16, u16)>,
}

/// Everything a brain asks for in one frame.
#[derive(Debug, Default)]
pub struct Outbox {
    /// Object orders. Applied through the L1 funnel, in order.
    pub orders: Vec<Order>,
    /// House-scoped requests: production, placement, teams. Applied by the
    /// owning native module, in order.
    pub intents: Vec<Intent>,
    /// Commands named directly, for a source that is not deciding *what to do*
    /// but replaying a command stream: a script, a recorded click sequence, a
    /// test. Admission is unchanged — the domain switch, the budget and the
    /// order gate apply exactly as they do to an order — so this is a shorter
    /// path to the same door, not a second one.
    pub commands: Vec<Command>,
    /// Objects handed to a managed routine this frame. Consumed before the
    /// orders below, so an object a routine just took owns no second issuer.
    pub attachments: Vec<Attachment>,
    /// Destinations the map refused this frame, as `(unit, cell, frame)`.
    ///
    /// A walk the projection will not take is a fact the order gate holds and
    /// the passes cannot see: the unit is left with no mission, and every pass
    /// reads missions. The gate records it here; the obstacle pass answers it
    /// in the same frame.
    pub blocked: Vec<(u64, (u16, u16), u64)>,
    /// The strongest rank each object was asked for this frame, and where its
    /// order sits in `orders` — the arbiter's own record.
    ///
    /// Passes run in a fixed order, and before this the *last* pass to speak
    /// owned the object: a dodge followed by a march was a march, and fixing
    /// one pass's precedence left the next one's hole open. A rank is a fact
    /// about the order, not about who asked first, so the strongest ask wins
    /// wherever it appears in the frame.
    wants: BTreeMap<u64, (u8, usize)>,
}

/// The rank of an order nobody ranked: the march's own. A source that does
/// not name a kind is asking for the ordinary fleet order, never for a dodge.
pub const ORDINARY_RANK: u8 = 6;

impl Outbox {
    /// Ask for one object order.
    pub fn order(&mut self, actor: u64, goal: Goal) {
        self.want(actor, ORDINARY_RANK, goal);
    }

    /// Ask for one object order, saying how strong the ask is: the smaller the
    /// rank, the more the order owns the object this frame.
    ///
    /// A weaker ask for an object already owned by a stronger one is dropped,
    /// and a stronger ask replaces the one already in `orders` — resolved
    /// before the frame's orders leave, so the engine still sees one order per
    /// object and the winner is the same whichever pass asked first.
    pub fn want(&mut self, actor: u64, rank: u8, goal: Goal) -> bool {
        if self
            .wants
            .get(&actor)
            .is_some_and(|(held, _)| *held <= rank)
        {
            // The object already has an ask at least this strong: this one is
            // not sent, and `false` is what tells the caller not to remember
            // an order the object never received.
            return false;
        }
        self.orders.push(Order { actor, goal });
        let index = self.orders.len() - 1;
        self.wants.insert(actor, (rank, index));
        true
    }

    /// Whether this frame already holds an order for `actor`, at any rank.
    pub fn ordered_this_frame(&self, actor: u64) -> bool {
        self.wants.contains_key(&actor)
    }

    /// Drop the orders a stronger ask replaced, leaving one per object.
    ///
    /// Called once, after every pass has had its say and before the frame's
    /// orders are applied. The survivors keep the order they were asked in.
    /// Returns the orders a stronger ask replaced: those callers recorded an
    /// intent for an order that never left, and the brain has to hear about it
    /// or the unit waits out a term for an order the engine never saw.
    pub fn resolve(&mut self) -> Vec<(u64, Goal)> {
        if self.wants.is_empty() {
            return Vec::new();
        }
        let keep: std::collections::BTreeSet<usize> =
            self.wants.values().map(|(_, index)| *index).collect();
        let mut index = 0usize;
        let mut dropped = Vec::new();
        self.orders.retain(|order| {
            let held = keep.contains(&index);
            index += 1;
            if !held {
                dropped.push((order.actor, order.goal));
            }
            held
        });
        self.wants.clear();
        dropped
    }

    /// Ask L2 to hand `actor` to a managed routine. The routine becomes that
    /// object's only issuer: the brain's own orders for it are dropped before
    /// admission from this frame on, and its later hand-back is logged by the
    /// control layer.
    pub fn attach(
        &mut self,
        actor: u64,
        kind: MicroKind,
        params: MicroParams,
        post: Option<(u16, u16)>,
    ) {
        self.attachments.push(Attachment {
            actor,
            kind,
            params,
            post,
        });
    }

    /// Ask for one house-scoped request.
    pub fn intent(&mut self, intent: Intent) {
        self.intents.push(intent);
    }

    /// Name one command directly, for a command-level source.
    pub fn command(&mut self, command: Command) {
        self.commands.push(command);
    }

    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
            && self.intents.is_empty()
            && self.commands.is_empty()
            && self.attachments.is_empty()
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::*;

    /// The frame's arbiter: the strongest ask owns the object, whoever made it
    /// and in whatever order. Two passes that disagree are a fact about their
    /// ranks, not about the order the brain happens to run them in.
    #[test]
    fn the_strongest_ask_owns_the_object() {
        let mut out = Outbox::default();
        assert!(out.want(7, 6, Goal::Move { cell: (10, 10) }));
        assert!(
            !out.want(7, 6, Goal::Move { cell: (20, 20) }),
            "an equal ask does not replace the one already sent"
        );
        assert!(
            !out.want(7, 7, Goal::Guard),
            "and a weaker one does not either"
        );
        assert!(
            out.want(7, 3, Goal::Attack { target: 9 }),
            "the fight replaces the march"
        );
        out.resolve();
        assert_eq!(out.orders.len(), 1, "one order per object");
        assert!(
            matches!(out.orders[0].goal, Goal::Attack { target: 9 }),
            "and it is the strongest ask: {:?}",
            out.orders[0].goal
        );
    }

    /// The ask a stronger one replaced is a refusal: the caller recorded an
    /// intent for an order that never left, and without hearing about it the
    /// unit waits out a term for nothing.
    #[test]
    fn a_replaced_ask_comes_back_as_a_refusal() {
        let mut out = Outbox::default();
        assert!(out.want(7, 6, Goal::Move { cell: (10, 10) }));
        assert!(out.want(7, 3, Goal::Attack { target: 9 }));
        let replaced = out.resolve();
        assert_eq!(replaced.len(), 1, "one order was replaced");
        assert!(matches!(replaced[0].1, Goal::Move { .. }));
        assert_eq!(out.orders.len(), 1);
        assert!(matches!(out.orders[0].goal, Goal::Attack { target: 9 }));
    }

    /// A weaker ask that arrives after a stronger one is refused, so the pass
    /// that made it does not record an order the object never received.
    #[test]
    fn a_refused_ask_is_not_sent() {
        let mut out = Outbox::default();
        assert!(out.want(7, 0, Goal::Move { cell: (11, 10) }));
        assert!(!out.want(7, 6, Goal::Move { cell: (30, 30) }));
        out.resolve();
        assert_eq!(out.orders.len(), 1);
        assert!(matches!(out.orders[0].goal, Goal::Move { cell: (11, 10) }));
    }
}

/// A decision source for one house.
pub trait Brain {
    /// A short name, for diagnostics and for the HUD line that names what is
    /// driving the house.
    fn name(&self) -> &'static str;

    /// Called once per frame, before the frame advances. Everything returned in
    /// `out` is considered for that frame's tail.
    ///
    /// Determinism: no clock, no file, no socket, no `f32` in a decision and no
    /// unordered container. Randomness comes from a generator the brain seeds
    /// and owns, never from a simulation stream.
    fn think(&mut self, rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox);

    /// The object orders the gate would not admit, called once after
    /// [`Self::think`] with the orders that never left.
    ///
    /// A brain that remembers what it has told each object needs this: the
    /// frame's budget admits a bounded number of commands, and an order dropped
    /// by it leaves no trace in the simulation. A brain that assumes its order
    /// went out then waits out its own re-issue term — 450 frames for a march —
    /// while the object stands still. The default does nothing, because a brain
    /// that keeps no memory has nothing to forget.
    fn orders_refused(&mut self, _refused: &[(u64, Goal)]) {}
}
