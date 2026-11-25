//! L3, the decision layer: what the house builds, raises, sends and takes.
//!
//! **Hard boundary.** This module uses [`crate::sim::control`]'s public types
//! and nothing else above it; it never names [`crate::sim::behavior`] directly
//! and never writes simulation state. It reads one projection
//! ([`control::Observation`](crate::sim::control::Observation)) and answers into
//! one [`control::Outbox`](crate::sim::control::Outbox). A decision that needs
//! state to persist across frames keeps it in the brain ([`assist`] holds its
//! plan, its generator and its outstanding orders), because a brain is the only
//! owner that knows what it already asked for.
//!
//! The layer is replaceable end to end: [`assist`] is one brain, a script is
//! another, and [`crate::sim::control::external`] fixes the contract an
//! out-of-process controller would speak. Swapping the brain changes what a
//! house decides and nothing else — not admission, not budgets, not the
//! command stream.
//!
//! Anything the native game decides and this layer does not yet drive is a
//! recorded gap, listed in `docs/ai-layers.md`, not a stub that pretends to.

pub mod brain;
pub mod assist;

pub use brain::{Brain, Outbox};
