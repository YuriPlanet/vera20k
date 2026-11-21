//! JSON Lines control of one GPU-free skirmish. Compiled only into `vera20k-play`.

mod command;
mod observe;
mod session;

pub use command::Request;
pub use observe::Event;
pub use session::{Session, SessionOptions};
