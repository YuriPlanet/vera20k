//! The retail shell substrate: dialog geometry and the owner-draw widgets'
//! render-agnostic behaviour, ported from gamemd's shell (addresses cited per
//! item). The host paints; these give it rects, frames and timing.

pub mod button;
pub mod combo_scroll;
pub mod combobox;
pub mod descriptor;
pub mod geom;
pub mod layout;
pub mod list;
pub mod new_edit;
pub mod paging;
pub mod static_reveal;
pub mod trackbar;
pub mod warning_monitor;
