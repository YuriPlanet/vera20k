//! Multiplayer via deterministic lockstep.
//!
//! net/ is a pure command transport - it collects player input commands and feeds
//! them into sim/. The simulation doesn't know if it's single-player or multiplayer.
//! net/ doesn't know game rules - it just moves command packets around.
//!
//! Deterministic lockstep works because:
//! 1. All players run the same simulation code
//! 2. All sim math uses fixed-point (no float divergence across machines)
//! 3. Only input commands are transmitted, not game state
//! 4. Each tick processes the same commands in the same order
//!
//! ## Dependency rules
//! - net/ depends on: sim/ (feeds commands into simulation, nothing else)
//! - net/ does NOT depend on: assets/, rules/, map/, render/, ui/, audio/

// Future modules - uncomment as implemented:
// pub mod protocol;

pub mod command_exchange;
pub mod event_packet;
pub mod global;
pub mod live_commands;
pub mod lockstep;
#[cfg(test)]
mod lockstep_sim_convergence_tests;
pub mod reliable;
pub mod udp;

pub mod lan_messages;

#[cfg(any(test, feature = "test-support"))]
pub mod fault_proxy;
