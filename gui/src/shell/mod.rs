//! The owner-draw widgets' render-agnostic behaviour, ported from gamemd's
//! shell (addresses cited per item), and the scaled pixel geometry they lay
//! out on. The host paints; these give it rects, frames and timing.

pub mod bar;
pub mod button;
pub mod combobox;
pub mod geom;
pub mod list;
pub mod new_edit;
pub mod trackbar;
