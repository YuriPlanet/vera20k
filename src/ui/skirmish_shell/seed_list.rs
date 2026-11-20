//! Saved-browser adapter for the shared618D40/61C690 list geometry.
use super::layout::SavedSeedControl;
use crate::ui::shell::list::ListScrollPart;
pub use crate::ui::shell::list::{ROW_HEIGHT, ShellListGeometry as SeedListGeometry, thumb_height};

/// The browser control a scrollbar part of `geometry` stands for.
pub fn scroll_control_at(geometry: &SeedListGeometry, x: i32, y: i32) -> Option<SavedSeedControl> {
    geometry.scroll_part_at(x, y).map(|part| match part {
        ListScrollPart::Up => SavedSeedControl::ScrollUp,
        ListScrollPart::Down => SavedSeedControl::ScrollDown,
        ListScrollPart::Thumb => SavedSeedControl::ScrollThumb,
        ListScrollPart::Track => SavedSeedControl::ScrollTrack,
    })
}
