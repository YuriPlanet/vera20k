//! L2, the control layer: it stores, schedules and delivers, and decides
//! nothing.
//!
//! **Hard boundary.** This module names [`crate::sim::behavior`] (L1) and never
//! [`crate::sim::strategy`] (L3): the port a brain implements is declared here
//! ([`brain`]), so the dependency arrow runs L3 → L2 → L1 and no cycle is even
//! representable. `architecture_guards.rs` fails the tests if a reference to the
//! strategy module appears here.
//!
//! What it owns:
//!
//! - the frame's scheduling ([`board`]): state requests, decisions, managed
//!   stepping, admission, and one queue at the end;
//! - the domain switches and the per-frame budget ([`budget`]) — the two
//!   player-visible throttles and the one silent-drop defence;
//! - the managed table ([`micro`]): which objects run a routine, and its phase.
//!   **Not simulation state**: never in a snapshot, never in the checksum;
//! - the read-only projection a decision sees ([`observation`]);
//! - the native placement answer ([`placement`]);
//! - the reserved external seam ([`external`]).
//!
//! What it does not own: any gameplay rule. Production, placement, the team
//! block, missions and combat stay with their native owners; the board calls
//! them and applies what they answer.

pub mod board;
pub mod brain;
pub mod budget;
pub mod external;
pub mod intent;
pub mod micro;
pub mod observation;
pub mod perception;
pub mod placement;

pub use board::{AutoPlayDomains, ControlBoard};
pub use brain::{Attachment, Brain, Outbox};
pub use budget::Budget;
pub use intent::{Intent, Order};
pub use micro::{Managed, MicroTable};
pub use observation::Observation;

/// The order vocabulary, re-exported so a decision layer speaks to L2 without
/// naming L1. A brain asks for a goal; the control layer turns it into a
/// command.
pub use crate::sim::behavior::goal::Goal;

/// The managed routine vocabulary, re-exported for the same reason: a brain
/// asks L2 to attach an object, and L2 holds the table and steps the routine.
/// Naming the routines from L3 directly would point the dependency arrow
/// backwards, which `architecture_guards.rs` fails on.
pub use crate::sim::behavior::managed::{MicroKind, MicroParams};
