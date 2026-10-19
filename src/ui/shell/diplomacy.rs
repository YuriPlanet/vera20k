//! Original active Diplomacy dialog73, callback657DA0 and refresh658640.
//! Resource geometry comes from gamemd's RT_DIALOG73; ordinary child placement
//! shares60B7A0 with Game Controls. App owns live House queries and admission.

use super::button::ShellButtonInteraction;
use super::geom::{self, RectPx};
use super::in_game_shell::InGameShellLayout;
use super::layout::in_game_ordinary_child_rect;

pub const VISIBLE_ROWS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiplomacyRow {
    pub name: String,
    pub rgb: [u8; 3],
    /// Original658586 sums Unit/Building kill counters with signed32 display.
    pub kills: i32,
    pub allied: bool,
    pub alliance_visible: bool,
    pub flag_country: Option<i32>,
}

/// View-only state; House statistics and alliances remain in the simulation.
#[derive(Debug, Default)]
pub struct DiplomacyState {
    pub buttons: ShellButtonInteraction<()>,
    pub top: usize,
}

impl DiplomacyState {
    /// VERA's larger House table can exceed the original eight visible rows.
    /// Scrolling changes the viewport, never the native encounter order.
    pub fn scroll(&mut self, rows: usize, delta: i32) {
        self.top = self
            .top
            .saturating_add_signed(delta as isize)
            .min(rows.saturating_sub(VISIBLE_ROWS));
    }
}

pub struct DiplomacyLayout {
    pub back: RectPx,
    pub title: RectPx,
    pub headers: [RectPx; 3],
    pub players: [RectPx; VISIBLE_ROWS],
    pub kills: [RectPx; VISIBLE_ROWS],
    pub allies: [RectPx; VISIBLE_ROWS],
    pub flags: [RectPx; VISIBLE_ROWS],
}

pub fn diplomacy_layout(
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button: [i32; 2],
) -> DiplomacyLayout {
    let ordinary =
        |x, y, w, h| in_game_ordinary_child_rect(geom::child_window(x, y, w, h), width, height);
    // Exact per-row resource heights; the native template is not eight copies
    // of a fitted pixel rectangle. The row window then takes60B7A0 centering.
    let rows = [
        (68, 10),
        (88, 10),
        (108, 9),
        (128, 8),
        (148, 8),
        (168, 8),
        (188, 8),
        (208, 9),
    ];
    DiplomacyLayout {
        // Return423 follows60B350 just like B5 Resume686, on SIDE3's top.
        back: RectPx::new(width - 147, shell.side3.y - button[1], button[0], button[1]),
        title: RectPx::new(width - 165, 2, 163, 17),
        headers: [
            ordinary(111, 53, 110, 10),
            ordinary(226, 53, 35, 10),
            ordinary(316, 53, 60, 10),
        ],
        players: rows.map(|(y, h)| ordinary(111, y, 110, h)),
        kills: rows.map(|(y, h)| ordinary(316, y, 41, h)),
        allies: std::array::from_fn(|i| {
            ordinary(226, rows[i].0, 10, if i == 4 { 10 } else { rows[i].1 })
        }),
        flags: std::array::from_fn(|i| ordinary(69, 66 + i as i32 * 20, 32, 12)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_diplomacy_controls_match_executed_native_placement() {
        // Steam gamemd SHA256 3e81a617..d308600. Full72FC60 then original
        // RT_DIALOG73 relayout60AAB0/60C540/60C4A0/60C0C0, active Session
        // +30D8=1, physical Allied headers and SIDEBTTN125x25. Win32 window
        // transport supplies6x13 template conversion; all placement executes.
        // Existing72FC60 corpus supplies its exact retail art dimensions.
        let source = crate::test_fixture::json("tools/storage_oracle/in_game_shell_geometry.json");
        let assets = &source["assets"]["allied"];
        let size = |key: &str| {
            [
                assets[key]["width"].as_i64().unwrap() as i32,
                assets[key]["height"].as_i64().unwrap() as i32,
            ]
        };
        let sizes = super::super::in_game_shell::InGameShellSizes {
            background_small: size("00b0fad4"),
            background_medium: size("00b0fac8"),
            background_large: size("00b0fa50"),
            credits: size("00b0fb08"),
            top: size("00b0f9e0"),
            radar: size("00b0fa68"),
            side1: size("00b0fa70"),
            side2: size("00b0fafc"),
            side3: size("00b0fa8c"),
            addon: size("00b0fa48"),
            bottom_spacer: size("00b0fa3c"),
            left_cap: size("00b0fa90"),
            button_background: size("00b0faa8"),
            right_cap: size("00b0fabc"),
        };
        for (width, height, back, player, last_kill, flag) in [
            (
                640,
                480,
                [493, 402, 125, 25],
                [87, 51, 166, 17],
                [394, 278, 63, 16],
                [24, 47, 49, 21],
            ),
            (
                800,
                600,
                [653, 502, 125, 25],
                [167, 111, 166, 17],
                [474, 338, 63, 16],
                [104, 107, 49, 21],
            ),
            (
                1024,
                768,
                [877, 702, 125, 25],
                [279, 195, 166, 17],
                [586, 422, 63, 16],
                [216, 191, 49, 21],
            ),
        ] {
            let shell = InGameShellLayout::new(width, height, sizes).unwrap();
            let layout = diplomacy_layout(width, height, shell, [125, 25]);
            let rect = |r: RectPx| [r.x, r.y, r.w, r.h];
            assert_eq!(rect(layout.back), back);
            assert_eq!(rect(layout.players[0]), player);
            assert_eq!(rect(layout.kills[7]), last_kill);
            assert_eq!(rect(layout.flags[0]), flag);
        }
    }

    #[test]
    fn enlarged_house_view_scrolls_without_losing_the_final_players() {
        let mut view = DiplomacyState::default();
        view.scroll(30, 100);
        assert_eq!(view.top, 22);
        view.scroll(30, -3);
        assert_eq!(view.top, 19);
        view.scroll(30, -100);
        assert_eq!(view.top, 0);
        view.scroll(2, 1);
        assert_eq!(view.top, 0);
    }
}
