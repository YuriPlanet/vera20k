//! VERA's Create Game choice for its own rooms, which the online and LAN
//! Create Game boxes both offer: whether later arrivals may watch. The map
//! decides how many players a room seats.

use crate::ui::shell::geom::{RectPx, ShellScale};

/// What the host of VERA's room chose at Create Game: whether later
/// arrivals may watch once the map's seats are taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoomLimits {
    pub(crate) spectators: bool,
}

impl Default for RoomLimits {
    fn default() -> Self {
        Self { spectators: true }
    }
}

/// Where a Create Game box puts the choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomLimitsLayout {
    /// The Allow spectators checkbox.
    pub spectators: RectPx,
}

/// What a press on the choice took, for its sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoomLimitsPress {
    Missed,
    Spectators,
}

/// The choice while a Create Game box is up.
#[derive(Debug, Clone, Default)]
pub(crate) struct RoomLimitsPicker {
    pub(crate) limits: RoomLimits,
}

impl RoomLimitsPicker {
    pub(crate) fn new(limits: RoomLimits) -> Self {
        Self { limits }
    }

    /// A left press at `(x, y)`: the checkbox takes it only on its icon.
    pub(crate) fn press(
        &mut self,
        layout: &RoomLimitsLayout,
        scale: ShellScale,
        x: i32,
        y: i32,
    ) -> RoomLimitsPress {
        if crate::ui::skirmish_shell::checkbox_icon_rect(layout.spectators, scale).contains(x, y) {
            self.limits.spectators = !self.limits.spectators;
            return RoomLimitsPress::Spectators;
        }
        RoomLimitsPress::Missed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_spectators_toggles_on_its_icon_only() {
        let layout = RoomLimitsLayout {
            spectators: RectPx::new(200, 130, 210, 24),
        };
        let mut picker = RoomLimitsPicker::default();
        let press = picker.press(&layout, ShellScale::RETAIL, 205, 135);
        assert_eq!(press, RoomLimitsPress::Spectators);
        assert!(!picker.limits.spectators);
        let press = picker.press(&layout, ShellScale::RETAIL, 300, 135);
        assert_eq!(press, RoomLimitsPress::Missed);
        assert_eq!(picker.limits, RoomLimits { spectators: false });
    }
}
