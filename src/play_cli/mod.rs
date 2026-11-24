//! JSON Lines control of one GPU-free skirmish. A tool module beside the
//! engine: it reads the simulation's own owners and issues its commands
//! through them.

mod command;
mod observe;
pub mod protocol;
mod session;

pub use command::Request;
pub use observe::Event;
pub use protocol::Outcome;
pub use session::{Session, SessionOptions};

#[cfg(all(test, tests_play_cli))]
#[path = "tests.rs"]
mod tests;
