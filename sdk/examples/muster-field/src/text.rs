//! The shared line builder, and how the simulation prints a line.

pub use muster_field_data::Line;
use vera20k_sdk::prelude::*;

/// A line in the players' message list.
pub trait Say {
    fn log(&self);
}

impl Say for Line {
    fn log(&self) {
        log(self.as_str());
    }
}
