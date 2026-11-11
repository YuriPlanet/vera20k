//! Exact common Mission transitions over private [`MissionCom`] state.
//!
//! These functions model only the verified base Assign, Queue, Commence,
//! Override, and Restore field semantics. Category wrappers own readiness,
//! Aircraft policy, Target, and NavCom ordering. No base transition reads a
//! clock, consumes RNG, allocates, or exposes a production-facing verb.

use super::{MissionCom, MissionId};

const GUARD: MissionId = MissionId::from_raw(5);
const SELLING: MissionId = MissionId::from_raw(19);
const DELIBERATE: MissionId = MissionId::from_raw(28);

/// Whether Queue passed its whole-function guard.
///
/// `Continue` does not imply that Queue's mutation predicate wrote anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueueContinuation {
    OuterGuardBlocked,
    Continue,
}

/// Apply the common Assign transition.
///
/// Native Assign has only the Deliberate-to-Guard guard; Selling is allowed.
pub(super) fn assign_base(state: &mut MissionCom, requested: MissionId, now: u32) {
    if state.current() == DELIBERATE && requested == GUARD {
        return;
    }
    state.assign_transition(requested, now);
}

/// Apply Queue's guard and conditional queue write.
///
/// Readiness and optional synchronous Commence belong to the category-aware
/// caller and run only after this function returns `Continue`.
pub(super) fn queue_base(state: &mut MissionCom, requested: MissionId) -> QueueContinuation {
    let current = state.current();
    if (current == DELIBERATE && requested == GUARD) || current == SELLING {
        return QueueContinuation::OuterGuardBlocked;
    }
    if requested != MissionId::NONE
        && !(current == requested
            && (state.queued() == requested || state.queued() == MissionId::NONE))
    {
        state.write_queue_and_clear_b8(requested);
    }
    QueueContinuation::Continue
}

/// Promote a queued selector and apply the exact Commence reset set.
pub(super) fn commence_base(state: &mut MissionCom, now: u32) -> bool {
    if !commences(state) {
        return false;
    }
    state.promote_queue(now);
    true
}

/// Whether [`commence_base`] promotes: something is queued.
pub(super) fn commences(state: &MissionCom) -> bool {
    state.queued() != MissionId::NONE
}

/// Apply the common Override transition.
///
/// The queued selector, when present, is copied to suspended and remains queued.
pub(super) fn override_base(state: &mut MissionCom, requested: MissionId) {
    let current = state.current();
    if (current == DELIBERATE && requested == GUARD) || current == SELLING {
        return;
    }
    state.override_transition(requested);
}

/// Restore a suspended selector without resetting handler or timing state.
pub(super) fn restore_base(state: &mut MissionCom) -> bool {
    if state.suspended() == MissionId::NONE {
        return false;
    }
    state.restore_transition();
    true
}
