//! Multiplayer via deterministic lockstep.
//!
//! net/ carries LAN and online datagrams (the socket pair and, online, the
//! client relay route), their reliable and global delivery, the event-packet
//! codec and command exchange, each match's live command session and lockstep
//! timing, and the LAN lobby and setup message codecs.
//!
//! Deterministic lockstep works because:
//! 1. All players run the same simulation code
//! 2. All sim math uses fixed-point (no float divergence across machines)
//! 3. Only input commands are transmitted, not game state
//! 4. Each tick processes the same commands in the same order
//!
//! ## Dependency rules
//! - net/ depends on sim/, util/, `skirmish_launch` (the network slot count)
//!   and the `vera20k_relay_protocol` crate.
//! - net/ does not depend on assets/, rules/, map/, render/, ui/ or audio/.

pub mod command_exchange;
pub mod event_packet;
pub mod global;
pub mod live_commands;
pub mod lockstep;
pub mod reliable;
pub mod udp;

pub mod lan_messages;

#[cfg(any(test, feature = "test-support"))]
pub mod fault_proxy;
