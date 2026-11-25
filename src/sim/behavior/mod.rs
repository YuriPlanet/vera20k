//! L1, the execution layer: what one object does, given an order.
//!
//! **Hard boundary.** Nothing in this module names
//! [`crate::sim::control`](crate::sim::control) or
//! [`crate::sim::strategy`](crate::sim::strategy). An execution behaviour
//! receives a [`goal::Goal`], reads its own state and the threats around it,
//! and answers with one command through [`order::Funnel`]. It makes no
//! decision about the house — not what to build, not what to attack, not which
//! unit goes where. `architecture_guards.rs` fails the build's tests if a reference
//! to the upper layers appears here.
//!
//! Two modes, one vocabulary:
//!
//! - **Direct** — the brain decides, the control layer translates its order and
//!   the native mission machine runs it to the end. No per-object memory.
//! - **Managed** ([`managed`]) — an object is attached to a small phase
//!   machine that issues its own goals frame by frame, and hands it back on a
//!   timeout, a fallback or completion. The phase lives in the control layer;
//!   routines are pure.
//!
//! What this layer does *not* re-implement: attack, move, guard, hunt, capture,
//! enter, harvest, deploy and infiltrate are already ported, each with one
//! owner (`mission`, `combat`, `aircraft`, `movement`, `production`). A goal
//! becomes the same [`crate::sim::command::Command`] the input layer produces
//! for the equivalent click, so the existing receivers, admission and retask
//! funnel stay the only path. [`order`] states the mapping and its evidence.

pub mod goal;
pub mod managed;
pub mod order;
pub mod view;

pub use goal::Goal;
pub use managed::{FallbackReason, MicroKind, MicroParams, MicroRoutine, MicroStep, Phase, Stage};
pub use order::{ApplyOutcome, Funnel, Refusal};
pub use view::{ObjectView, Threat, ThreatView};
