//! Test-support counters for the hot paths a performance change touches.
//!
//! These count work, not state: a foot's body-frame predicate asked twice
//! per frame, and an attack-cell search that walks the whole entity store
//! for every candidate cell, are the costs a change has to keep out. The
//! counters live in test builds only, so a release build carries none of
//! them.
//!
//! They are per-thread: the simulation's parallel work runs on other
//! threads, and a test that counts its own thread's work stays independent
//! of the tests running beside it.

use std::cell::Cell;

thread_local! {
    static FOOT_BODY_FRAME_DUE_CALLS: Cell<u64> = const { Cell::new(0) };
    static AIRCRAFT_RESERVATION_VISITS: Cell<u64> = const { Cell::new(0) };
}

/// Note one [`crate::sim::animation::foot_body_frame_due`] evaluation.
pub(crate) fn note_foot_body_frame_due() {
    FOOT_BODY_FRAME_DUE_CALLS.with(|calls| calls.set(calls.get() + 1));
}

/// How many times [`crate::sim::animation::foot_body_frame_due`] was asked
/// on this thread.
pub(crate) fn foot_body_frame_due_calls() -> u64 {
    FOOT_BODY_FRAME_DUE_CALLS.with(Cell::get)
}

/// Note one entity the landing-zone reservation scan visited.
pub(crate) fn note_aircraft_reservation_visit() {
    AIRCRAFT_RESERVATION_VISITS.with(|visits| visits.set(visits.get() + 1));
}

/// How many entities the landing-zone reservation scan visited on this
/// thread. A search that answers from a set it read once visits none.
pub(crate) fn aircraft_reservation_visits() -> u64 {
    AIRCRAFT_RESERVATION_VISITS.with(Cell::get)
}

/// Clear both counters.
pub(crate) fn reset() {
    FOOT_BODY_FRAME_DUE_CALLS.with(|calls| calls.set(0));
    AIRCRAFT_RESERVATION_VISITS.with(|visits| visits.set(0));
}
