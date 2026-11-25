//! VERA's Create Game choices for its own rooms, which the online and LAN
//! Create Game boxes both offer: the most people a room holds, players and
//! spectators together, on a Max players trackbar like Generate Map's, and
//! whether later arrivals may watch. The map decides the player seats.

use crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT;
use crate::ui::shell::geom::RectPx;
use vera20k_gui::shell::trackbar::TrackbarRange;
use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

/// What the host of VERA's room chose at Create Game: the most people it
/// holds, and whether later arrivals may watch once its seats are taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoomLimits {
    pub(crate) players: u8,
    pub(crate) spectators: bool,
}

impl Default for RoomLimits {
    fn default() -> Self {
        Self {
            players: NETWORK_PLAYER_SLOT_COUNT as u8,
            spectators: true,
        }
    }
}

/// The Max players trackbar: two up to every row a room has.
pub(crate) const PLAYERS: TrackbarRange =
    TrackbarRange::new(2, NETWORK_PLAYER_SLOT_COUNT as i32, 1);

/// Where a Create Game box puts the two choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomLimitsLayout {
    /// The Players caption, right aligned.
    pub players_label: RectPx,
    /// The Max players trackbar, including its value plaque.
    pub players: RectPx,
    /// The Allow spectators checkbox.
    pub spectators: RectPx,
}

vera20k_gui::scaled_rects!(RoomLimitsLayout {
    players_label,
    players,
    spectators
});

/// The two choices as a box's widgets, with control ids `players` and
/// `spectators`: the Max players trackbar takes the pointer as every shell
/// trackbar does, and the Allow spectators box toggles on a press on its
/// icon (`0x006163A0`).
pub(crate) fn widgets(limits: RoomLimits, players: u16, spectators: u16) -> [NodeSpec; 2] {
    [
        NodeSpec::new(NodeKind::Trackbar)
            .id(players.into())
            .with(Prop::Min, Value::Int(PLAYERS.min))
            .with(Prop::Max, Value::Int(PLAYERS.max))
            .with(Prop::Pos, Value::Int(limits.players.into())),
        NodeSpec::new(NodeKind::Checkbox)
            .id(spectators.into())
            .with(Prop::Checked, Value::Bool(limits.spectators))
            .label("VERA:AllowSpectators"),
    ]
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::page;
    use vera20k_gui::{DialogKind, Gui, InputEvent, MouseButton, NodeId};

    const LAYOUT: RoomLimitsLayout = RoomLimitsLayout {
        players_label: RectPx::new(0, 0, 100, 20),
        players: RectPx::new(200, 100, 129, 22),
        spectators: RectPx::new(200, 130, 210, 24),
    };

    /// The choices' widgets in a box at [`LAYOUT`], as Create Game shows them.
    fn picker() -> (Gui, NodeId) {
        let mut gui = Gui::new(8);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        gui.push_dialog(root, DialogKind::Modal).unwrap();
        page::ensure_built(&mut gui, root, || {
            NodeSpec::new(NodeKind::Dialog).children(widgets(RoomLimits::default(), 1, 2))
        });
        page::sync_rects(&mut gui, root, |id| match id {
            1 => Some(LAYOUT.players),
            2 => Some(LAYOUT.spectators),
            _ => None,
        });
        (gui, root)
    }

    fn press(gui: &mut Gui, x: i32, y: i32) {
        gui.input(InputEvent::PointerMove { x, y }, 0);
        gui.input(
            InputEvent::PointerDown {
                x,
                y,
                button: MouseButton::Left,
            },
            0,
        );
    }

    fn drag(gui: &mut Gui, x: i32) {
        gui.input(InputEvent::PointerMove { x, y: 110 }, 0);
    }

    fn release(gui: &mut Gui, x: i32, y: i32) {
        gui.input(
            InputEvent::PointerUp {
                x,
                y,
                button: MouseButton::Left,
            },
            0,
        );
    }

    fn limits(gui: &Gui, root: NodeId) -> RoomLimits {
        RoomLimits {
            players: page::int(gui, root, 1, Prop::Pos).unwrap() as u8,
            spectators: page::checked(gui, root, 2),
        }
    }

    #[test]
    fn the_players_trackbar_jumps_drags_and_keeps_two_to_thirty() {
        let (mut gui, root) = picker();
        // A press left of the thumb jumps to the left end, without a drag.
        press(&mut gui, 201, 110);
        assert_eq!(limits(&gui, root).players, 2);
        drag(&mut gui, 320);
        assert_eq!(limits(&gui, root).players, 2);
        release(&mut gui, 320, 110);
        // The thumb drags as far as thirty.
        let thumb = LAYOUT.players.x
            + PLAYERS.thumb_left_in(2, LAYOUT.players, crate::ui::shell::geom::RETAIL_SCALE);
        press(&mut gui, thumb + 6, 110);
        drag(&mut gui, 400);
        assert_eq!(limits(&gui, root).players, 30);
        // Released, the thumb stays.
        release(&mut gui, 400, 110);
        drag(&mut gui, 210);
        assert_eq!(limits(&gui, root).players, 30);
    }

    #[test]
    fn allow_spectators_toggles_on_its_icon_only() {
        let (mut gui, root) = picker();
        press(&mut gui, 205, 135);
        release(&mut gui, 205, 135);
        assert!(!limits(&gui, root).spectators);
        press(&mut gui, 300, 135);
        // A missed press holds nothing for a drag.
        drag(&mut gui, 320);
        release(&mut gui, 320, 135);
        assert_eq!(
            limits(&gui, root),
            RoomLimits {
                players: 30,
                spectators: false
            }
        );
    }
}
