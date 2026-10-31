//! VERA's Create Game choices for its own rooms, which the online and LAN
//! Create Game boxes both offer: the most players a room seats, on a Players
//! trackbar like Generate Map's, and whether later arrivals may watch.

use crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::trackbar::{TrackbarHold, TrackbarPress, TrackbarRange};

/// What the host of VERA's room chose at Create Game: the most players it
/// seats, and whether later arrivals may watch once those seats are taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoomLimits {
    pub(crate) players: u8,
    pub(crate) spectators: bool,
}

impl Default for RoomLimits {
    fn default() -> Self {
        Self {
            players: 8,
            spectators: true,
        }
    }
}

/// The Players trackbar: two up to every row a room has.
pub(crate) const PLAYERS: TrackbarRange =
    TrackbarRange::new(2, NETWORK_PLAYER_SLOT_COUNT as i32, 1);

/// Where a Create Game box puts the two choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomLimitsLayout {
    /// The Players caption, right aligned.
    pub players_label: RectPx,
    /// A plaque trackbar of the Skirmish Credits window's 129x22.
    pub players: RectPx,
    /// The Allow spectators checkbox.
    pub spectators: RectPx,
}

/// What a press on the choices took, for its sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoomLimitsPress {
    Missed,
    Players,
    Spectators,
}

/// The choices while a Create Game box is up, with the Players trackbar's
/// hold on the mouse.
#[derive(Debug, Clone, Default)]
pub(crate) struct RoomLimitsPicker {
    pub(crate) limits: RoomLimits,
    hold: Option<TrackbarHold>,
}

impl RoomLimitsPicker {
    pub(crate) fn new(limits: RoomLimits) -> Self {
        Self { limits, hold: None }
    }

    /// A left press at `(x, y)`: the trackbar takes it as every shell
    /// trackbar does, and the checkbox only on its icon.
    pub(crate) fn press(
        &mut self,
        layout: &RoomLimitsLayout,
        scale: ShellScale,
        x: i32,
        y: i32,
    ) -> RoomLimitsPress {
        let press = PLAYERS.press_in(i32::from(self.limits.players), x, y, layout.players, scale);
        if let Some(hold) = press.hold(()) {
            if let TrackbarPress::Jump(value) = press {
                self.limits.players = value as u8;
            }
            self.hold = Some(hold);
            return RoomLimitsPress::Players;
        }
        if crate::ui::skirmish_shell::checkbox_icon_rect(layout.spectators, scale).contains(x, y) {
            self.limits.spectators = !self.limits.spectators;
            return RoomLimitsPress::Spectators;
        }
        RoomLimitsPress::Missed
    }

    /// The pointer moved to `x`: a thumb held since the press follows it.
    pub(crate) fn drag(&mut self, layout: &RoomLimitsLayout, scale: ShellScale, x: i32) {
        if self.hold.is_some_and(|hold| hold.dragging) {
            self.limits.players = PLAYERS.value_at_in(x, layout.players, scale) as u8;
        }
    }

    /// The trackbar keeps the mouse from its press to the release.
    pub(crate) fn release(&mut self) {
        self.hold = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_players_trackbar_jumps_drags_and_keeps_two_to_thirty() {
        let layout = RoomLimitsLayout {
            players_label: RectPx::new(0, 0, 100, 20),
            players: RectPx::new(200, 100, 129, 22),
            spectators: RectPx::new(200, 130, 210, 24),
        };
        let scale = ShellScale::RETAIL;
        let mut picker = RoomLimitsPicker::default();
        // A press left of the thumb jumps to the left end, without a drag.
        let press = picker.press(&layout, scale, 201, 110);
        assert_eq!(
            (press, picker.limits.players),
            (RoomLimitsPress::Players, 2)
        );
        picker.drag(&layout, scale, 320);
        assert_eq!(picker.limits.players, 2);
        picker.release();
        // The thumb drags as far as thirty.
        let thumb = layout.players.x + PLAYERS.thumb_left_in(2, layout.players, scale);
        picker.press(&layout, scale, thumb + 6, 110);
        picker.drag(&layout, scale, 400);
        assert_eq!(picker.limits.players, 30);
        // Released, the thumb stays.
        picker.release();
        picker.drag(&layout, scale, 210);
        assert_eq!(picker.limits.players, 30);
    }

    #[test]
    fn allow_spectators_toggles_on_its_icon_only() {
        let layout = RoomLimitsLayout {
            players_label: RectPx::new(0, 0, 100, 20),
            players: RectPx::new(200, 100, 129, 22),
            spectators: RectPx::new(200, 130, 210, 24),
        };
        let mut picker = RoomLimitsPicker::default();
        let press = picker.press(&layout, ShellScale::RETAIL, 205, 135);
        assert_eq!(press, RoomLimitsPress::Spectators);
        assert!(!picker.limits.spectators);
        let press = picker.press(&layout, ShellScale::RETAIL, 300, 135);
        assert_eq!(press, RoomLimitsPress::Missed);
        // A missed press holds nothing for a drag.
        picker.drag(&layout, ShellScale::RETAIL, 320);
        assert_eq!(
            picker.limits,
            RoomLimits {
                players: 8,
                spectators: false
            }
        );
    }
}
