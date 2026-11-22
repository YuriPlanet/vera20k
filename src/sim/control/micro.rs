//! The managed table: which objects run a routine, and its phase.
//!
//! **This state is not simulation state.** It never enters a snapshot, a
//! checksum or a replay: a replay reproduces it from the same command stream
//! and the same rules, because every routine is a pure function of
//! `(object, threats, phase)` and the phase is advanced only by stepping. What
//! the table decides reaches the world exclusively as commands. The header of
//! [`crate::sim::behavior::managed`] states the same invariant from the other
//! side; a test replays an identical sequence into two tables and compares them
//! step for step.
//!
//! Lifecycle is L2's: an entry is dropped when its object dies, leaves the map,
//! is captured, boards a transport or is mind-controlled, and when a team script
//! takes it over — a team's members already have an issuer, and two issuers
//! would overwrite each other's orders (`TeamClass::Recruit` clears the
//! member's target).

use std::collections::BTreeMap;

use crate::rules::ruleset::RuleSet;
use crate::sim::behavior::managed::{FallbackReason, MicroKind, MicroParams, MicroStep, Phase};
use crate::sim::behavior::order::ApplyOutcome;
use crate::sim::behavior::view::{ObjectView, ThreatView};
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// One attached object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Managed {
    pub kind: MicroKind,
    pub params: MicroParams,
    pub phase: Phase,
    /// Consecutive goals the gate dropped (another house's object, a switched
    /// off domain, no budget). Past [`Self::MAX_REFUSALS`] the entry falls back
    /// to the brain: an order this table cannot deliver is not worth holding an
    /// object for.
    pub refusals: u32,
    /// Consecutive frames the funnel answered [`ApplyOutcome::NotReady`].
    ///
    /// **This is not a refusal.** `Ready_To_Commence` answers false while the
    /// object is in the middle of a mission — for the whole walk a `Move` order
    /// is executing, not for one awkward frame. Counting it as a failure made a
    /// retreating unit flap: the routine asked for the walk it was already
    /// walking, three refusals dropped the entry, the brain re-attached it the
    /// same frame its condition still held, and the cycle ran for the rest of
    /// the match (26 809 log lines in one 36 000-tick run). So it has its own
    /// bound, long enough to outlast any order the routine itself issued.
    pub waiting: u32,
}

impl Managed {
    /// How many orders the gate may drop in a row before the routine gives the
    /// object back.
    pub const MAX_REFUSALS: u32 = 3;

    /// How long an object may stay mid-mission before the routine gives up on
    /// it, in frames: a minute at the native 15 a second, the same window a
    /// plan node gets before the plan gives up on *it*. Long enough to cover
    /// the longest walk a routine issues (a retreat crosses a base, not a map)
    /// and short enough that an object stuck in a mission the routine cannot
    /// interrupt surfaces instead of being held silently.
    pub const MAX_WAITING: u32 = 900;
}

/// Every managed object of one house.
#[derive(Debug, Default)]
pub struct MicroTable {
    entries: BTreeMap<u64, Managed>,
}

impl MicroTable {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_managed(&self, entity: u64) -> bool {
        self.entries.contains_key(&entity)
    }

    pub fn get(&self, entity: u64) -> Option<&Managed> {
        self.entries.get(&entity)
    }

    /// Attach `entity` to a routine. `false` when it is already managed, so a
    /// caller cannot silently reset a phase.
    pub fn attach(
        &mut self,
        entity: u64,
        kind: MicroKind,
        params: MicroParams,
        post: Option<(u16, u16)>,
    ) -> bool {
        if self.entries.contains_key(&entity) {
            return false;
        }
        self.entries.insert(
            entity,
            Managed {
                kind,
                params,
                phase: Phase {
                    post,
                    ..Phase::default()
                },
                refusals: 0,
                waiting: 0,
            },
        );
        true
    }

    pub fn detach(&mut self, entity: u64) -> Option<Managed> {
        self.entries.remove(&entity)
    }

    /// Drop the entries whose object may no longer be driven by a routine.
    ///
    /// Called once per frame, before stepping. Returns the ids it dropped, so
    /// the caller can log them.
    pub fn retain_live(
        &mut self,
        sim: &Simulation,
        owner: InternedId,
    ) -> Vec<(u64, FallbackReason)> {
        let mut dropped = Vec::new();
        self.entries.retain(|&entity, _| {
            let Some(entry) = sim.entities().get(entity) else {
                dropped.push((entity, FallbackReason::Gone));
                return false;
            };
            let reason =
                if entry.owner() != owner {
                    Some(FallbackReason::Gone)
                } else if !entry.is_object_alive()
                    || entry.lifecycle.in_limbo()
                    || entry.health.current() <= 0
                {
                    Some(FallbackReason::Gone)
                } else if entry.passenger_role().inside_transport().is_some()
                    || entry.seldom.mind_control.controller().is_some()
                {
                    Some(FallbackReason::Gone)
                } else if sim.team_script_vm().team_for_member(entity).is_some_and(
                    |(team_id, _)| {
                        sim.team_script_vm()
                            .team(team_id)
                            .is_some_and(|team| team.owner() == owner)
                    },
                ) {
                    // A team's script owns its members.
                    Some(FallbackReason::NothingToDo)
                } else {
                    None
                };
            match reason {
                Some(reason) => {
                    dropped.push((entity, reason));
                    false
                }
                None => true,
            }
        });
        dropped
    }

    /// The ids to step this frame: `limit` of them, starting at `from` and
    /// wrapping, in ascending id order so the sequence is reproducible.
    pub fn rotation(&self, from: u64, limit: usize) -> Vec<u64> {
        if self.entries.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut ids: Vec<u64> = self
            .entries
            .range(from..)
            .map(|(&id, _)| id)
            .chain(self.entries.range(..from).map(|(&id, _)| id))
            .take(limit)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Step one attached object. `None` when it is still waiting out its timer
    /// and was not asked this frame.
    pub fn step_one(
        &mut self,
        entity: u64,
        sim: &Simulation,
        rules: &RuleSet,
        threats: &ThreatView<'_>,
    ) -> Option<MicroStep> {
        let Some(entry) = self.entries.get_mut(&entity) else {
            return None;
        };
        if entry.phase.wait > 0 {
            entry.phase.wait -= 1;
            return None;
        }
        let Some(me) = ObjectView::new(sim, rules, entity) else {
            return Some(MicroStep::Fallback(FallbackReason::Gone));
        };
        let kind = entry.kind;
        let params = entry.params;
        let step = kind.routine().step(&me, threats, &mut entry.phase, &params);
        if let MicroStep::Wait { frames } = step {
            entry.phase.wait = frames;
        }
        Some(step)
    }

    /// Record the outcome of issuing a routine's goal.
    ///
    /// The three answers mean three different things and are counted
    /// separately, because a routine's object being busy is not the routine
    /// failing:
    ///
    /// - [`ApplyOutcome::Queued`] — the order left. Both counters reset.
    /// - [`ApplyOutcome::NotReady`] — nothing was written and the object is
    ///   mid-mission, which is what walking home *is*. It keeps the entry, and
    ///   only [`Managed::MAX_WAITING`] frames without a single order taking
    ///   drops it.
    /// - [`ApplyOutcome::Rejected`] — the gate dropped the order. Past
    ///   [`Managed::MAX_REFUSALS`] in a row the entry is dropped so the brain
    ///   takes the object back: the silent-stall defence.
    ///
    /// Either drop is reported so the caller can log it.
    pub fn record_outcome(&mut self, entity: u64, outcome: ApplyOutcome) -> Option<FallbackReason> {
        let entry = self.entries.get_mut(&entity)?;
        match outcome {
            ApplyOutcome::Queued => {
                entry.refusals = 0;
                entry.waiting = 0;
                None
            }
            ApplyOutcome::NotReady => {
                entry.refusals = 0;
                entry.waiting += 1;
                if entry.waiting < Managed::MAX_WAITING {
                    return None;
                }
                self.entries.remove(&entity);
                Some(FallbackReason::RepeatedNotReady)
            }
            ApplyOutcome::Rejected => {
                entry.waiting = 0;
                entry.refusals += 1;
                if entry.refusals < Managed::MAX_REFUSALS {
                    return None;
                }
                self.entries.remove(&entity);
                Some(FallbackReason::Refused)
            }
        }
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::*;

    fn attached(entity: u64) -> MicroTable {
        let mut table = MicroTable::default();
        assert!(
            table.attach(entity, MicroKind::Retreat, MicroParams::default(), None),
            "the object starts unattached"
        );
        table
    }

    /// A unit walking home answers `NotReady` for the whole walk — that is what
    /// `Ready_To_Commence` means while a mission is executing — and the routine
    /// it belongs to must keep it. Counting those answers as failures made the
    /// retreat flap: three frames, hand back, the brain re-attaches the same
    /// frame its condition still holds, and the pair ran for the rest of the
    /// match (26 809 log lines in one 36 000-tick run).
    #[test]
    fn a_walking_object_keeps_its_routine() {
        let mut table = attached(7);
        for frame in 0..30u32 {
            assert_eq!(
                table.record_outcome(7, ApplyOutcome::NotReady),
                None,
                "frame {frame} of a walk is not a failure"
            );
        }
        let entry = table.get(7).expect("still managed");
        assert_eq!(entry.waiting, 30, "the walk is counted, not failed");
        assert_eq!(entry.refusals, 0, "the gate dropped nothing");
    }

    /// The other half: an object that never takes an order must surface rather
    /// than be held silently. A minute of continuous `NotReady` is the bound.
    #[test]
    fn an_object_mid_mission_for_a_minute_is_handed_back() {
        let mut table = attached(7);
        for _ in 1..Managed::MAX_WAITING {
            assert_eq!(table.record_outcome(7, ApplyOutcome::NotReady), None);
        }
        assert_eq!(
            table.record_outcome(7, ApplyOutcome::NotReady),
            Some(FallbackReason::RepeatedNotReady)
        );
        assert!(table.get(7).is_none(), "the brain takes it back");
    }

    /// An order the object takes resets the wait: the walk it was waiting on
    /// ended, and the next stretch is a new one.
    #[test]
    fn a_queued_goal_resets_the_wait() {
        let mut table = attached(7);
        for _ in 0..100 {
            table.record_outcome(7, ApplyOutcome::NotReady);
        }
        assert_eq!(table.record_outcome(7, ApplyOutcome::Queued), None);
        assert_eq!(table.get(7).expect("still managed").waiting, 0);
    }

    /// A gate rejection is a failure and keeps its own, much shorter bound: the
    /// table cannot deliver this object's orders at all, so holding it only
    /// costs the frame.
    #[test]
    fn three_gate_rejections_hand_the_object_back() {
        let mut table = attached(7);
        assert_eq!(table.record_outcome(7, ApplyOutcome::Rejected), None);
        assert_eq!(table.record_outcome(7, ApplyOutcome::Rejected), None);
        assert_eq!(
            table.record_outcome(7, ApplyOutcome::Rejected),
            Some(FallbackReason::Refused)
        );
        assert!(table.get(7).is_none());
    }

    /// The two counters do not stand in for each other: a rejected order in the
    /// middle of a long wait is one failure, not a hand-back.
    #[test]
    fn a_rejection_between_waits_does_not_add_up_to_a_hand_back() {
        let mut table = attached(7);
        for _ in 0..10 {
            table.record_outcome(7, ApplyOutcome::Rejected);
            table.record_outcome(7, ApplyOutcome::NotReady);
        }
        assert!(
            table.get(7).is_some(),
            "the wait keeps resetting the streak"
        );
    }
}
