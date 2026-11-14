//! What each unit has already been told, and when it may be told again.
//!
//! Every other layer here answers "what should this unit do"; this one answers
//! "does it need telling". Those are different questions, and the second one
//! is where this brain was wrong: it re-decided for every unit every frame and
//! sent the answer again, so a command was issued once per frame for as long
//! as the decision stood.
//!
//! That is invisible for a command that sets a destination, and it is a bug
//! for the two kinds that are not: a **toggle** (the D key flips the pose, so a
//! second press undoes the first) and one whose **state lags the command** (a
//! deploying soldier still reads as standing until the animation finishes, so a
//! step that re-reads the state toggles it straight back up). Both were being
//! patched one at a time — a refresh timer here, a cooldown there — and the
//! garrison, which has neither, was re-issued every frame.
//!
//! One owner now holds the answer: an intent is recorded when it is issued,
//! and a layer that asks for the same one is refused until one of three things
//! is true —
//!
//! - **it expired** (each kind gets its own term: a deploy animation is short,
//!   a walk across the map is long);
//! - **the intent changed** (a different kind, or a different target);
//! - **the target moved** past [`REISSUE_CELLS`], which is the case an expiry
//!   alone cannot see: a unit walking at a target that has itself walked away
//!   is still holding a valid-looking intent.
//!
//! The third is why this is a record of *what was said* rather than a timer:
//! the world moves under the order, and only the order's own target knows when
//! it has moved far enough to be worth repeating.
//!
//! Determinism: `BTreeMap` keyed by the unit id, integer frame and integer
//! squared-distance comparisons only.

use std::collections::BTreeMap;

use crate::sim::control::Goal;

/// What a unit has been told to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TrackedKind {
    /// Walk to a cell.
    Move,
    /// Attack one object.
    Attack,
    /// What is in reach *this second*, decided by the micro layer.
    ///
    /// Separate from [`Self::Attack`] because it has to be able to override a
    /// march and be overridden by one: its term is one micro cycle, so the
    /// fight is re-decided every cycle and a unit that has finished fighting
    /// is free again within one.
    Engage,
    /// Attack-move to a cell: engage what is on the way, keep going.
    AttackMove,
    /// Hold the ground it stands on and answer what comes.
    Guard,
    /// Enter a building: a garrison, or the hut whose span is down.
    Enter,
    /// Capture a building: the engineer's and the spy's click.
    Capture,
    /// The D key: take, or leave, the deployed pose.
    Deploy,
    /// Step out of the landing point of a shot already in the air.
    ///
    /// Its own kind because its term is its own: a shell lands in a handful of
    /// frames, and the whole point is to be free to step again the moment the
    /// next one is fired — a march's half-minute term would make a unit dodge
    /// once and stand still through everything after it.
    Dodge,
    /// Step out of a shorter-ranged enemy's reach while reloading.
    ///
    /// One micro cycle: a step is a cell, and the cycle re-reads the reload, so
    /// the next decision comes when the gun is about to be ready again.
    Kite,
    /// Walk around a fight the unit would lose on the way, decided by the
    /// avoidance layer.
    ///
    /// Its own kind because it re-aims a march: a unit half a leg into a detour
    /// must not be re-aimed every micro cycle by the next reading of the same
    /// block, and a march that owns it again has to be a different intent. Two
    /// seconds is one leg of the walk.
    Avoid,
    /// Shoot the wall standing where the walk would go, decided by the
    /// avoidance layer.
    ///
    /// Its own kind because it is not a march and not a target: there is no
    /// object to hold, only a cell, and sharing [`Self::Attack`] would make
    /// every pass that asks "is this unit pushing" believe it is. A wall takes
    /// a few shots to fall, so the unit is left to finish them rather than
    /// being re-aimed at the same cell every frame.
    Breach,
    /// Put an idle harvester back on the ore.
    ///
    /// A harness: the simulation's miner state machine does the work once the
    /// object is on `Harvest`, but a harvester that rolled out of a factory on
    /// its birth posture never starts it. Its own kind so the click is not
    /// repeated every frame while the machine is already running.
    Harvest,
}

/// One micro cycle, in frames: a second at play cadence. The layer that
/// decides what a unit shoots *now* re-decides on this term, so it can both
/// override a march and hand the unit back to it quickly.
pub(super) const MICRO_FRAMES: u64 = 15;

/// How long a refused command is left alone before it is asked again, in
/// frames.
///
/// A command the world refuses leaves no mission behind, so the layer that
/// decided it sees the old state again on the next frame and asks again — for
/// as long as the refusal stands. Two seconds is long enough to stop that
/// storm and short enough that a refusal caused by a passing state (a bay that
/// just filled, a cell a unit has not stepped out of yet) is retried while the
/// unit still matters.
const REFUSED_FRAMES: u64 = 30;

/// How strongly a kind owns the unit's next command. Lower wins.
///
/// The passes each decide one unit's next order, and before this every pass
/// carried its own list of kinds to defer to — so a rule added to one pass was
/// a hole in the next, and fixing one unit's walk broke another's. One ladder,
/// asked by [`Tracker::admits`], is the whole answer: a live intent of a
/// stronger kind is the unit's owner until its term runs, whatever pass would
/// like to say otherwise.
///
/// - A step out from under a shell, a kite step, and a click into a building
///   outrank everything: they are one frame's worth of a decision that cannot
///   wait.
/// - The fight in reach outranks the route: a unit shoots what it can see
///   before it walks anywhere.
/// - A breach outranks a detour: the wall it is shooting is the way through.
/// - The march is the weakest order a unit takes, and a guard weaker still.
fn rank(kind: TrackedKind) -> u8 {
    match kind {
        TrackedKind::Dodge => 0,
        TrackedKind::Kite => 1,
        TrackedKind::Enter | TrackedKind::Capture | TrackedKind::Deploy => 2,
        TrackedKind::Engage => 3,
        TrackedKind::Breach => 4,
        TrackedKind::Avoid => 5,
        // A harvester's cycle is a walk and a stop on ore: it is the same
        // family as the march, not below it, or the walk to the field would
        // hold the harvest off for the walk's whole term.
        TrackedKind::Move
        | TrackedKind::Attack
        | TrackedKind::AttackMove
        | TrackedKind::Harvest => 6,
        TrackedKind::Guard => 7,
    }
}

impl TrackedKind {
    /// How long the intent stands before it may be issued again, in frames.
    ///
    /// Each is how long the object needs for the command to be *done* rather
    /// than merely issued: a pose takes its animation, walking into a building
    /// takes the walk. Something that holds ground has no natural end, so it
    /// gets the same term the brain used to refresh an order on.
    fn term(self) -> u64 {
        match self {
            Self::Deploy => 45,
            Self::Engage => MICRO_FRAMES,
            // A step is one cell's worth of walk, and a fast unit crosses it
            // in a handful of frames. Three frames let every new shell pick a
            // new cell, which is the back-and-forth a dog shows under fire;
            // twelve frames is one step committed, not a dance between two
            // cells.
            Self::Dodge => 12,
            Self::Kite => MICRO_FRAMES,
            // One leg of a detour: long enough to walk the few cells to the
            // corner, short enough that a tower that died — or a road that
            // opened — is read again while the unit is still on the way.
            Self::Avoid => 30,
            // A few shots at a wall segment, then read the ground again: the
            // cell may already be open, or the way around it may have.
            Self::Breach => 90,
            Self::Enter | Self::Capture => 300,
            Self::Move | Self::Attack | Self::AttackMove | Self::Guard | Self::Harvest => 450,
        }
    }
}

/// One unit's standing intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct UnitRuntime {
    pub(super) kind: TrackedKind,
    /// The object the intent names, when it names one.
    pub(super) target: Option<u64>,
    /// The cell the intent names, when it names one: the thing that can move
    /// out from under the order.
    pub(super) cell: Option<(u16, u16)>,
    /// The frame before which it is not issued again — the commit window.
    pub(super) until: u64,
    /// The frame the order was issued on. Kept apart from `until` so a commit
    /// window can be read as a duration rather than recomputed from the term,
    /// and so a diagnostic can say how long a unit has been on one order.
    pub(super) issued_at: u64,
    /// How many times the world refused this unit's last order. A refusal is
    /// the one fact the decision layer needs and the simulation does not keep:
    /// without it a unit whose every order is dropped looks like a unit that
    /// chose to stand still.
    pub(super) refusals: u8,
}

/// How far a target may move before an otherwise-valid intent is worth
/// repeating, in cells. Squared-distance comparisons below; three cells is a
/// target that has visibly walked away, not one that shuffled. At two, a fast
/// unit's next recon sample was far enough to re-issue every time and the
/// walk alternated between two samples.
const REISSUE_CELLS: i32 = 3;

/// Every unit's standing intent, and the units a routine is busy with.
#[derive(Debug, Default)]
pub(super) struct Tracker {
    intents: BTreeMap<u64, UnitRuntime>,
    /// Units a managed routine owns, and the frame it hands them back on. A
    /// routine is its object's only issuer, so the layers above ask here before
    /// they tell one anything.
    held: BTreeMap<u64, u64>,
    /// Commands the world refused, and the frame before which they are not
    /// asked again. A refused command leaves no mission behind, so without a
    /// term the layer below re-asks it every frame — the command storm.
    refused: BTreeMap<(u64, TrackedKind), u64>,
}

impl Tracker {
    pub(super) fn new() -> Self {
        Self {
            intents: BTreeMap::new(),
            held: BTreeMap::new(),
            refused: BTreeMap::new(),
        }
    }

    /// Whether `unit` may be told to do `kind` at `target` / `cell` now.
    ///
    /// True when it has not been told anything, when what it was told has
    /// expired, when the intent itself changed, or when its target has moved
    /// far enough that the order is stale.
    pub(super) fn admits(
        &self,
        now: u64,
        unit: u64,
        kind: TrackedKind,
        target: Option<u64>,
        cell: Option<(u16, u16)>,
    ) -> bool {
        if self.held.get(&unit).is_some_and(|until| now < *until) {
            return false;
        }
        if self
            .refused
            .get(&(unit, kind))
            .is_some_and(|until| now < *until)
        {
            return false;
        }
        let Some(tracked) = self.intents.get(&unit) else {
            return true;
        };
        // Whoever owns the unit outranks the asker: a pass may not replace a
        // stronger intent that has not finished. This is the one place the
        // ladder is asked, so every pass inherits the same rule and no pass
        // can learn it differently.
        if tracked.kind != kind && now < tracked.until && rank(tracked.kind) < rank(kind) {
            return false;
        }
        if tracked.kind != kind || tracked.target != target {
            return true;
        }
        if now >= tracked.until {
            return true;
        }
        moved(tracked.cell, cell)
    }

    /// Whether anyone at all still has a live `kind` intent on `target`.
    ///
    /// The reverse of `admits`, for the orders that are about a *place* rather
    /// than about a unit: one building takes one engineer, and a second
    /// engineer sent at the same one walks to a building that is already being
    /// taken, arrives after it is taken, and stands there. Whoever asks is
    /// holding the answer, so the claim is recorded like any other intent and
    /// released by the same expiry.
    pub(super) fn claimed_by_other(
        &self,
        now: u64,
        kind: TrackedKind,
        target: u64,
        asker: u64,
    ) -> bool {
        self.intents.iter().any(|(owner, tracked)| {
            *owner != asker
                && tracked.kind == kind
                && tracked.target == Some(target)
                && now < tracked.until
        })
    }

    /// Drop an intent the world does not show.
    ///
    /// An order that was refused, that had no path, or that a later order
    /// cancelled leaves no trace in the simulation, and the intent table used
    /// to go on believing its own record for the intent's whole term: a Move
    /// that never took held a scout in the yard for 450 frames. That is the
    /// heart of the "it asks once and then assumes" behaviour — the brain had
    /// no feedback at all.
    ///
    /// Callers ask for this when the object's own state contradicts the intent,
    /// before [`Self::admits`], so a refused order is asked again next frame
    /// instead of on the term.
    pub(super) fn void(&mut self, unit: u64, kind: TrackedKind) {
        if self
            .intents
            .get(&unit)
            .is_some_and(|tracked| tracked.kind == kind)
        {
            self.intents.remove(&unit);
        }
    }

    /// Leave `kind` alone for `unit` until the refusal cools off.
    ///
    /// Called when the world refused the command: the order left no mission
    /// behind, so the intent table would otherwise let the layer below ask
    /// again on the very next frame. Keyed by kind because a refused walk must
    /// not also silence the dodge that would save the same unit.
    pub(super) fn refuse(&mut self, now: u64, unit: u64, kind: TrackedKind) {
        self.refused
            .insert((unit, kind), now.saturating_add(REFUSED_FRAMES));
        // The refusal belongs to the unit's runtime as well, not only to the
        // cool-off table: "this unit has been refused five times" is what
        // separates a unit that cannot act from one that chose not to.
        if let Some(runtime) = self.intents.get_mut(&unit) {
            runtime.refusals = runtime.refusals.saturating_add(1);
        }
    }

    /// How many units are holding an order the world refused at least once:
    /// the reading that says whether a pass is asking for the impossible.
    pub(super) fn refused_units(&self) -> usize {
        self.intents
            .values()
            .filter(|runtime| runtime.refusals > 0)
            .count()
    }

    /// Record that `unit` has been told to do `kind` at `frame`.
    pub(super) fn record(
        &mut self,
        now: u64,
        unit: u64,
        kind: TrackedKind,
        target: Option<u64>,
        cell: Option<(u16, u16)>,
    ) {
        // A command that was taken is no longer refused: clear the term so a
        // later refusal starts a fresh one.
        self.refused.remove(&(unit, kind));
        self.intents.insert(
            unit,
            UnitRuntime {
                kind,
                target,
                cell,
                until: now.saturating_add(kind.term()),
                issued_at: now,
                refusals: 0,
            },
        );
    }

    /// Drop the units the house no longer has, so the table stays the size of
    /// the army and a recycled id cannot inherit another unit's intent.
    pub(super) fn retain(&mut self, live: &std::collections::BTreeSet<u64>) {
        self.intents.retain(|id, _| live.contains(id));
        self.held.retain(|id, _| live.contains(id));
        self.refused.retain(|(id, _), _| live.contains(id));
    }

    /// Whether `unit` is still holding an intent of one of `kinds`.
    ///
    /// A layer that wants to know whether another layer already owns the unit
    /// asks this rather than reading the record itself: "the advance step is
    /// pushing it" is a question about the intents above, not about this
    /// unit's mission.
    pub(super) fn stands(&self, now: u64, unit: u64, kinds: &[TrackedKind]) -> bool {
        self.intents
            .get(&unit)
            .is_some_and(|tracked| kinds.contains(&tracked.kind) && now < tracked.until)
    }

    /// The live intent's kind, by name, for the log that asks why a claimed
    /// unit is standing still.
    pub(super) fn live_kind(&self, now: u64, unit: u64) -> Option<TrackedKind> {
        let tracked = self.intents.get(&unit)?;
        (now < tracked.until).then_some(tracked.kind)
    }

    /// Whether any layer has claimed `unit` inside the current term: a live
    /// intent, or a routine holding it.
    ///
    /// The question "does anything speak for this unit" rather than "is it
    // moving right now" — the census's idle count answers the second and also
    /// catches an order that is in flight.
    pub(super) fn is_claimed(&self, now: u64, unit: u64) -> bool {
        self.intents
            .get(&unit)
            .is_some_and(|tracked| now < tracked.until)
            || self.held.get(&unit).is_some_and(|until| now < *until)
    }

    /// Hand `unit` to a routine until `until`: the routine is its only issuer
    /// for those frames.
    pub(super) fn hold(&mut self, unit: u64, until: u64) {
        self.held.insert(unit, until);
    }

    /// The rank of the intent that owns `unit` right now, when one does.
    ///
    /// The same ladder [`rank`] gives `admits`, asked by the frame's arbiter:
    /// an order's rank is the rank of the intent its caller recorded.
    pub(super) fn rank_of(&self, now: u64, unit: u64) -> Option<u8> {
        self.intents
            .get(&unit)
            .filter(|tracked| now < tracked.until)
            .map(|tracked| rank(tracked.kind))
    }

    /// Whether a routine owns `unit` right now.
    pub(super) fn held(&self, now: u64, unit: u64) -> bool {
        self.held.get(&unit).is_some_and(|until| now < *until)
    }
}

/// The intents a goal may have been recorded under.
///
/// One goal can be two kinds, because the same click is issued by two steps for
/// two reasons: `Goal::Attack` is `Engage` from the micro layer — what this
/// unit shoots *now* — and `Attack` from the defensive one, what its squad is
/// aimed at. A refusal names only the goal, so the caller tries the set and
/// [`Tracker::void`] drops the one that actually matches; a set is not a second
/// rule, it is the ambiguity the vocabulary already has.
pub(super) fn kinds_for(goal: Goal) -> &'static [TrackedKind] {
    match goal {
        Goal::Attack { .. } => &[TrackedKind::Engage, TrackedKind::Attack],
        Goal::AttackMove { .. } => &[TrackedKind::AttackMove],
        Goal::Move { .. } | Goal::MoveToObject { .. } => &[
            TrackedKind::Move,
            TrackedKind::Dodge,
            TrackedKind::Kite,
            TrackedKind::Avoid,
        ],
        Goal::Guard | Goal::GuardAt { .. } => &[TrackedKind::Guard],
        Goal::Capture { .. } => &[TrackedKind::Capture],
        Goal::Enter { .. } => &[TrackedKind::Enter],
        Goal::Deploy => &[TrackedKind::Deploy],
        Goal::AttackCell { .. } => &[TrackedKind::Breach],
        Goal::Harvest { .. } => &[TrackedKind::Harvest],
        Goal::Stop | Goal::Scatter | Goal::Unload => &[],
    }
}

/// Whether the target has moved far enough for a standing order to be stale.
fn moved(was: Option<(u16, u16)>, now: Option<(u16, u16)>) -> bool {
    let (Some(was), Some(now)) = (was, now) else {
        return was != now;
    };
    let dx = i32::from(was.0) - i32::from(now.0);
    let dy = i32::from(was.1) - i32::from(now.1);
    dx * dx + dy * dy > REISSUE_CELLS * REISSUE_CELLS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toggle issued twice is the bug this layer exists for: the second
    /// press undoes the first, so the answer has to be no until the pose has
    /// had time to take.
    #[test]
    fn a_toggle_is_not_issued_twice() {
        let mut tracker = Tracker::new();
        assert!(tracker.admits(0, 7, TrackedKind::Deploy, None, None));
        tracker.record(0, 7, TrackedKind::Deploy, None, None);
        assert!(
            !tracker.admits(10, 7, TrackedKind::Deploy, None, None),
            "the pose is still being taken"
        );
        assert!(
            tracker.admits(60, 7, TrackedKind::Deploy, None, None),
            "and may be asked again once it has"
        );
    }

    /// An order is stale when its target walks away, even though nothing about
    /// the order itself has changed and it has not expired.
    #[test]
    fn an_order_is_repeated_when_its_target_moves() {
        let mut tracker = Tracker::new();
        let target = 9;
        tracker.record(0, 7, TrackedKind::Attack, Some(target), Some((20, 20)));
        assert!(
            !tracker.admits(10, 7, TrackedKind::Attack, Some(target), Some((21, 20))),
            "one cell is a shuffle, not a move"
        );
        assert!(
            tracker.admits(10, 7, TrackedKind::Attack, Some(target), Some((25, 20))),
            "five cells is the target walking away"
        );
    }

    /// A different intent is a new decision, not a repeat of the old one.
    #[test]
    fn a_different_intent_is_admitted_at_once() {
        let mut tracker = Tracker::new();
        tracker.record(0, 7, TrackedKind::Guard, None, None);
        assert!(tracker.admits(1, 7, TrackedKind::Attack, Some(9), None));
    }

    /// A unit a routine owns is not told anything by the layers above it.
    #[test]
    fn a_held_unit_is_left_to_its_routine() {
        let mut tracker = Tracker::new();
        tracker.hold(7, 100);
        assert!(!tracker.admits(10, 7, TrackedKind::Guard, None, None));
        assert!(tracker.admits(101, 7, TrackedKind::Guard, None, None));
    }

    /// A refused command is not asked again on the next frame: the world left
    /// no mission behind, so the refusal term is the only thing holding the
    /// layer back. Without it the same order is re-decided and re-sent every
    /// frame — the command storm.
    #[test]
    fn a_refused_command_cools_off() {
        let mut tracker = Tracker::new();
        tracker.refuse(100, 7, TrackedKind::Move);
        assert!(
            !tracker.admits(101, 7, TrackedKind::Move, None, Some((10, 10))),
            "the refusal still stands"
        );
        assert!(
            tracker.admits(131, 7, TrackedKind::Move, None, Some((10, 10))),
            "and is retried once it has cooled"
        );
    }

    /// A refusal of one kind does not silence another: a walk the world refused
    /// must not also stop the dodge that would keep the unit alive.
    #[test]
    fn a_refusal_is_per_kind() {
        let mut tracker = Tracker::new();
        tracker.refuse(0, 7, TrackedKind::Move);
        assert!(!tracker.admits(10, 7, TrackedKind::Move, None, Some((0, 0))));
        assert!(tracker.admits(10, 7, TrackedKind::Dodge, None, Some((1, 0))));
    }

    /// One ladder for the whole brain: whoever owns the unit outranks the
    /// asker, so no pass has to carry its own list of kinds to defer to.
    /// The lists that used to live in `advance`, `avoidance` and `micro` were
    /// three copies of this rule, and a rule added to one was a hole in the
    /// next — which is why fixing one unit's walk kept breaking another's.
    #[test]
    fn a_stronger_intent_owns_the_unit_until_its_term_runs() {
        let mut tracker = Tracker::new();
        // A kite step outranks the fight it is stepping out of.
        tracker.record(0, 7, TrackedKind::Kite, None, Some((10, 10)));
        assert!(
            !tracker.admits(1, 7, TrackedKind::Engage, Some(9), None),
            "an attack does not interrupt a kite step"
        );
        assert!(
            !tracker.admits(1, 7, TrackedKind::AttackMove, None, Some((20, 20))),
            "nor does a march"
        );
        // A step out from under a shell outranks the kite.
        assert!(tracker.admits(1, 7, TrackedKind::Dodge, None, Some((11, 10))));
        // A guard is the weakest order a unit takes.
        let mut guard = Tracker::new();
        guard.record(0, 7, TrackedKind::Guard, None, None);
        assert!(
            guard.admits(1, 7, TrackedKind::AttackMove, None, Some((20, 20))),
            "a guard does not hold a unit against the march"
        );
        // A breach outranks the detour around the wall it is shooting.
        let mut breach = Tracker::new();
        breach.record(0, 7, TrackedKind::Breach, None, Some((10, 10)));
        assert!(!breach.admits(1, 7, TrackedKind::Avoid, None, Some((11, 10))));
        // And it expires on its own term.
        assert!(breach.admits(90, 7, TrackedKind::Avoid, None, Some((11, 10))));
    }
}
