//! Auto play: an agent that issues a house's commands.
//!
//! The agent replaces the player's hands, not the game. It observes through a
//! read-only projection scoped to one house
//! ([`crate::sim::control::Observation`]) and answers with
//! [`crate::sim::command::Command`]s.
//! [`host::AutoPlayHost`] stamps each answer with the house and the current tick
//! and hands it to `Simulation::queue_commands`, the same append-only ingress
//! the player's clicks use (`sim/world/command_schedule.rs`). Nothing downstream
//! changes and no layer writes simulation state directly, so an agent-driven
//! match records and replays like any other: the agent is just another command
//! producer.
//!
//! # This module is the facade
//!
//! The agent itself lives in three layers, described in `docs/ai-layers.md`:
//!
//! - [`crate::sim::behavior`] — **L1, execute.** One object, one order: the
//!   goal vocabulary, the single order funnel with its readiness gate, the
//!   per-object view, and the managed micro routines.
//! - [`crate::sim::control`] — **L2, control.** Who decides when, what is
//!   admitted, what the budget is, where a managed phase lives, and the
//!   projection a decision sees. It decides nothing itself.
//! - [`crate::sim::strategy`] — **L3, decide.** What to build, raise, send and
//!   take. Replaceable end to end, and the only layer an external controller
//!   would replace.
//!
//! Dependencies run one way, L3 → L2 → L1, and
//! `architecture_guards.rs::sim_ai_layers_depend_one_way` fails the tests if that
//! stops being true. The client depends on
//! this file only: a hotkey installs a host, the frame steps it, the HUD reads
//! its name and switch.
//!
//! # Scope: actuation only
//!
//! This module does not decide what to build or attack; that is the installed
//! brain. It also does not reproduce the input layer's `What_Action` and sidebar
//! admission trees; strict equivalence with a human click is a non-goal, and the
//! gate is only the minimal self-harm check `docs/autoplay.md` specifies.
//!
//! # Timing
//!
//! The host runs just before the frame drains its due commands, so a command it
//! issues is stamped with the current tick, is drained by that same frame
//! (`take_due_commands`, due at `execute_tick <= tick + 1`), and takes effect in
//! that frame's tail. Nothing in the simulation remembers what the agent asked
//! for — the brain keeps that — and
//! `a_host_issued_command_is_drained_by_the_frame_that_follows_its_step` pins
//! this, because it is easy to state one frame out.
//!
//! Dependencies: `sim` only, so this stays inside the core crate and is usable
//! from `headless_scenario` with no window, renderer or UI.

pub mod host;
pub mod scripted_policy;

pub use crate::sim::behavior::managed::{MicroKind, MicroParams};
pub use crate::sim::control::board::AutoPlayDomains;
pub use crate::sim::control::brain::Brain;
pub use host::AutoPlayHost;
pub use scripted_policy::{ScriptStep, ScriptedPolicy};

#[cfg(all(test, tests_sim_a_b))]
mod autoplay_tests;
