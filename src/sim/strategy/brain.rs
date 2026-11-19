//! The brain contract, as the decision layer sees it.
//!
//! [`Brain`] and [`Outbox`] are declared by the control layer
//! ([`crate::sim::control::brain`]) and re-exported here. That direction is
//! deliberate: L2 must be able to drive *any* brain without naming this
//! module, so the trait cannot live on this side of the boundary. A brain
//! implementation imports it from here; the compiler does not care, and the
//! layering test does.
//!
//! Implementations live beside this file — [`super::assist`] is the assist one.
//! An implementation must be a pure function of the projection plus its own
//! memory: no clock, no `f32` in a decision, no unordered iteration, and any
//! random draw comes from a generator it seeds and owns.

pub use crate::sim::control::brain::{Brain, Outbox};
