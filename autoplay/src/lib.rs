//! The auto-play policy, outside the engine.
//!
//! Core gives it two things and nothing else: a read-only projection to ask
//! questions of, and a command ingress to answer into. Everything in this crate
//! is a decision, and every decision is registered in the registry module so the
//! list of what the policy can do is one table, not a search.

pub mod job;
pub mod manager;
pub mod policy;
pub mod profile;
pub mod registry;

#[cfg(test)]
mod board_move_tests;
#[cfg(test)]
mod fog_tests;
#[cfg(test)]
mod guard_move_tests;
#[cfg(test)]
mod move_tests;
#[cfg(test)]
mod never_wrong_tests;
#[cfg(test)]
mod transport_tests;
