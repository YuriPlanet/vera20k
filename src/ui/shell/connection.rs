//! The connection dialog, RT_DIALOG 0xEA: `Wait_For_Players` 0x00648710
//! creates it over the active game (0x00648C40) when a peer's frames stop
//! arriving; proc 0x0064AE50.
//!
//! Geometry: `ResizeShellChildControl` 0x0060C0C0 sends Quit Game 2 to the
//! bottom button (0x0060B350, matched at 0x00609979) and status line 0x695 to
//! 0x0060B550; every other child goes to 0x0060B7A0 (0x0060C3EE). The
//! template, those placements and the 0x006040B0 help keys are pinned by
//! `tools/storage_oracle/connection_shell_layout.py`.
//!
//! The app's wait model owns the rows, the list and the countdown; this
//! module owns control identity, geometry, the list's line rules and the
//! per-player bar paint.

use super::button::ShellButtonInteraction;
use super::geom::{self, RectPx};
use super::in_game_shell::{self, InGameShellLayout};
use crate::ui::lan_shell::{LanChatLine, LanListState};

/// Rows the dialog has, one per connection node (local player first).
pub const CONNECTION_ROWS: usize = 8;

/// Row button ids by row (table 0x00837300). The template swaps 0x641 and
/// 0x642, so the rows still run top to bottom.
const PLAYER_IDS: [u16; CONNECTION_ROWS] = [0x640, 0x642, 0x641, 0x643, 0x644, 0x645, 0x646, 0x647];
/// Progress bar ids by row (table 0x00837320): hidden group boxes 0x40000007.
const BAR_IDS: [u16; CONNECTION_ROWS] = [0x648, 0x649, 0x64A, 0x64B, 0x64C, 0x64D, 0x64E, 0x64F];

const fn player_dlu(row: usize) -> RectPx {
    RectPx::new(74, 49 + 20 * row as i32, 83, 15)
}

const fn bar_dlu(row: usize) -> RectPx {
    RectPx::new(197, 49 + 20 * row as i32, 155, 14)
}

const COUNTDOWN_DLU: RectPx = RectPx::new(74, 219, 278, 10);
const LIST_DLU: RectPx = RectPx::new(74, 234, 278, 70);

pub const QUIT_LABEL: (&str, &str) = ("GUI:LeaveGame", "Quit Game");

/// The dialog's push buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionButton {
    /// Quit Game 2: the result cell becomes 2 (Leave).
    Quit,
    /// Row button `0x640..0x647`: propose removing that player
    /// (0x0064AAE0).
    Player(usize),
}

/// A child the pointer can rest on, for the status-line help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionControl {
    Quit,
    Player(usize),
    Bar(usize),
    Countdown,
    List,
}

impl ConnectionControl {
    /// The status-line help key: the 0x006040B0 table for dialog 0xEA.
    pub fn help_key(self) -> &'static str {
        match self {
            Self::Quit => "STT:ReconnectButtonLeaveGame",
            Self::Player(_) => "STT:ReconnectButtonPlayer",
            Self::Bar(_) => "STT:ReconnectProgressPlayer",
            Self::Countdown => "STT:ReconnectLabelTime",
            Self::List => "STT:ReconnectEditInfo",
        }
    }

    /// Native control id.
    pub fn id(self) -> u16 {
        match self {
            Self::Quit => 0x2,
            Self::Player(row) => PLAYER_IDS[row],
            Self::Bar(row) => BAR_IDS[row],
            Self::Countdown => 0x650,
            Self::List => 0x651,
        }
    }

    fn button(self) -> Option<ConnectionButton> {
        match self {
            Self::Quit => Some(ConnectionButton::Quit),
            Self::Player(row) => Some(ConnectionButton::Player(row)),
            Self::Bar(_) | Self::Countdown | Self::List => None,
        }
    }
}

/// Physical rectangles of every control in the active-game shell's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionLayout {
    pub quit: RectPx,
    pub status_line: RectPx,
    pub players: [RectPx; CONNECTION_ROWS],
    pub bars: [RectPx; CONNECTION_ROWS],
    pub countdown: RectPx,
    pub list: RectPx,
}

pub fn connection_layout(
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button_size: [i32; 2],
) -> ConnectionLayout {
    let ordinary = |dlu: RectPx| {
        in_game_shell::ordinary_child_rect(
            geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h),
            width,
            height,
        )
    };
    ConnectionLayout {
        quit: shell.lower_button_rect(width, button_size),
        status_line: in_game_shell::status_line_rect(height),
        players: std::array::from_fn(|row| ordinary(player_dlu(row))),
        bars: std::array::from_fn(|row| ordinary(bar_dlu(row))),
        countdown: ordinary(COUNTDOWN_DLU),
        list: list_rect(width, height),
    }
}

/// List 0x651 alone, for the wait's line wrapping.
pub fn list_rect(width: i32, height: i32) -> RectPx {
    in_game_shell::ordinary_child_rect(
        geom::dlu_rect(LIST_DLU.x, LIST_DLU.y, LIST_DLU.w, LIST_DLU.h),
        width,
        height,
    )
}

impl ConnectionLayout {
    pub fn control_rect(&self, control: ConnectionControl) -> RectPx {
        match control {
            ConnectionControl::Quit => self.quit,
            ConnectionControl::Player(row) => self.players[row],
            ConnectionControl::Bar(row) => self.bars[row],
            ConnectionControl::Countdown => self.countdown,
            ConnectionControl::List => self.list,
        }
    }
}

/// The outline colour `[0x00A8EAE4]`, set once by the static initializer
/// 0x004E8180.
pub const BAR_OUTLINE_RGB: [u8; 3] = [255, 0, 0];

/// One row's bar as 0x0064A7B0 paints it on the dialog surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarPaint {
    pub outline: RectPx,
    pub fill: RectPx,
    pub fill_rgb: [u8; 3],
}

/// 0x0064A7B0 for one row. `lag` is the record's age in 16 ms buckets
/// (`[0x00AFA418]` less the record's last-received bucket; row 0 is 0) and
/// `age_ticks` the record's millisecond age ×60/1000. Green up to 240,
/// yellow up to 480, then red, or blue (with the age as the lag) while the
/// age is under 420. The fill keeps `100 − lag·100/1200` percent of the
/// width, at least 6 pixels, inset by 2.
pub fn bar_paint(rect: RectPx, lag: u32, age_ticks: u32) -> BarPaint {
    let mut lag = lag;
    let mut rgb = [0, 200, 0];
    if lag > 240 {
        rgb = [200, 200, 0];
        if lag > 480 {
            rgb = [200, 0, 0];
            if age_ticks < 420 {
                rgb = [0, 0, 200];
                lag = age_ticks;
            }
        }
    }
    // Unsigned arithmetic, then the signed `< 1` clamp to zero.
    let percent = 100u32.wrapping_sub(lag.wrapping_mul(100) / 1200) as i32;
    let width = rect.w * percent.max(0) / 100;
    let width = if width > 5 { width } else { 6 };
    BarPaint {
        outline: rect,
        fill: RectPx::new(rect.x + 2, rect.y + 2, width - 4, rect.h - 4),
        fill_rgb: rgb,
    }
}

/// Pointer state and the list while 0xEA is open. The row names are fixed
/// at the init arm (0x497): row `i < nodeCount` shows node `i`'s name, the
/// rest are destroyed.
#[derive(Debug, Clone, Default)]
pub(crate) struct ConnectionPage {
    pub(crate) rows: Vec<String>,
    /// Static 0x650 after its first `TXT_TIME_ALLOWED` write.
    pub(crate) countdown: Option<String>,
    /// List 0x651.
    pub(crate) lines: Vec<LanChatLine>,
    pub(crate) list: LanListState,
    /// The wait's elapsed buckets at the last full repaint: the bars are
    /// drawn by WM_PAINT, which the wait posts on even countdown seconds.
    pub(crate) painted_elapsed: u64,
    pub(crate) buttons: ShellButtonInteraction<ConnectionButton>,
    pub(crate) hovered: Option<ConnectionControl>,
}

impl ConnectionPage {
    /// The visible child under a pointer, in template order (Quit, list,
    /// row buttons, bars, countdown), as `ChildWindowFromPointEx` finds it.
    pub fn control_at(
        &self,
        layout: &ConnectionLayout,
        x: i32,
        y: i32,
    ) -> Option<ConnectionControl> {
        let rows = self.rows.len().min(CONNECTION_ROWS);
        [ConnectionControl::Quit, ConnectionControl::List]
            .into_iter()
            .chain(TEMPLATE_PLAYER_ORDER.iter().filter_map(|id| {
                PLAYER_IDS
                    .iter()
                    .position(|row_id| row_id == id)
                    .filter(|row| *row < rows)
                    .map(ConnectionControl::Player)
            }))
            .chain((0..rows).map(ConnectionControl::Bar))
            .chain([ConnectionControl::Countdown])
            .find(|control| layout.control_rect(*control).contains(x, y))
    }

    /// The button a control is, for capture.
    pub fn button_at(&self, layout: &ConnectionLayout, x: i32, y: i32) -> Option<ConnectionButton> {
        self.control_at(layout, x, y)
            .and_then(ConnectionControl::button)
    }

    /// The tail of 0x0064AAE0 / 0x0064AD10 after their line: keep at most
    /// 50 lines (delete line 0) and show the last one.
    pub fn trim_and_show_last(&mut self, visible_rows: usize) {
        if self.lines.len() > 50 {
            self.lines.remove(0);
        }
        let last = self.lines.len().saturating_sub(1);
        self.list.top_index = last.min(self.lines.len().saturating_sub(visible_rows));
    }
}

/// The template's row-button order (DLGTEMPLATEEX), which sets Z order.
const TEMPLATE_PLAYER_ORDER: [u16; CONNECTION_ROWS] =
    [0x640, 0x641, 0x642, 0x643, 0x644, 0x645, 0x646, 0x647];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::in_game_shell::InGameShellSizes;

    fn corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/connection_shell_layout.json",
        ))
        .unwrap()
    }

    fn all_controls() -> Vec<ConnectionControl> {
        let mut out = vec![ConnectionControl::Quit, ConnectionControl::List];
        out.extend((0..CONNECTION_ROWS).map(ConnectionControl::Player));
        out.extend((0..CONNECTION_ROWS).map(ConnectionControl::Bar));
        out.push(ConnectionControl::Countdown);
        out
    }

    fn shell(width: i32, height: i32) -> InGameShellLayout {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/in_game_shell_geometry.json",
        ))
        .unwrap();
        let assets = &fixture["assets"]["allied"];
        let size = |address: &str| {
            [
                assets[address]["width"].as_i64().unwrap() as i32,
                assets[address]["height"].as_i64().unwrap() as i32,
            ]
        };
        InGameShellLayout::new(
            width,
            height,
            InGameShellSizes {
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
            },
        )
        .unwrap()
    }

    /// Every ordinary child lands where original 0x0060B7A0 put it at
    /// 640x480, 800x600 and 1024x768, row buttons through table 0x00837300.
    #[test]
    fn ordinary_children_match_the_executed_native_placement() {
        let corpus = corpus();
        let cases = corpus["cases"].as_array().unwrap();
        let mut checked = 0;
        for control in all_controls() {
            if control == ConnectionControl::Quit {
                continue;
            }
            let id = u64::from(control.id());
            for case in cases.iter().filter(|case| case["control_id"] == id) {
                let (width, height) = (
                    case["width"].as_i64().unwrap() as i32,
                    case["height"].as_i64().unwrap() as i32,
                );
                let layout = connection_layout(width, height, shell(width, height), [125, 25]);
                let rect = layout.control_rect(control);
                assert_eq!(
                    serde_json::json!([rect.x, rect.y, rect.w, rect.h]),
                    case["rect"],
                    "{control:?} {id:#x} at {width}x{height}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, (1 + 8 + 8 + 1) * 3);
    }

    /// Quit Game 2 and the status line take B5's Resume and footer places,
    /// like Diplomacy's Return (0x0060B350, 0x0060B550).
    #[test]
    fn quit_and_status_line_share_the_pause_menu_placements() {
        use crate::ui::shell::pause_menu::{PauseMenuButton, PauseMenuTemplate, pause_menu_layout};
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            let shell = shell(width, height);
            let layout = connection_layout(width, height, shell, [125, 25]);
            let pause =
                pause_menu_layout(width, height, shell, [125, 25], PauseMenuTemplate::Offline);
            assert_eq!(layout.quit, pause.button(PauseMenuButton::Resume).unwrap());
            assert_eq!(layout.status_line, pause.footer);
        }
    }

    /// The help keys are the executed 0x006040B0 answers for dialog 0xEA.
    #[test]
    fn help_keys_match_the_native_help_table() {
        let corpus = corpus();
        for control in all_controls() {
            let native = corpus["controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["control_id"] == u64::from(control.id()))
                .unwrap_or_else(|| panic!("{control:?} is in the template"));
            assert_eq!(
                native["help"].as_str(),
                Some(control.help_key()),
                "{control:?}"
            );
        }
    }

    #[test]
    fn row_buttons_follow_the_swapped_id_table_and_absent_rows_take_no_pointer() {
        let layout = connection_layout(800, 600, shell(800, 600), [125, 25]);
        let page = ConnectionPage {
            rows: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        let row1 = layout.players[1];
        assert_eq!(
            page.control_at(&layout, row1.x + 1, row1.y + 1),
            Some(ConnectionControl::Player(1))
        );
        assert_eq!(ConnectionControl::Player(1).id(), 0x642);
        let row2 = layout.players[2];
        assert_eq!(page.control_at(&layout, row2.x + 1, row2.y + 1), None);
        let bar2 = layout.bars[2];
        assert_eq!(page.control_at(&layout, bar2.x + 1, bar2.y + 1), None);
    }

    /// 0x0064A7B0's colour bands, the 1200-bucket empty point and the
    /// 6-pixel floor.
    #[test]
    fn bars_shrink_with_lag_and_change_colour_at_the_native_bands() {
        let rect = RectPx::new(296, 80, 233, 23);
        let full = bar_paint(rect, 0, 0);
        assert_eq!(full.fill, RectPx::new(298, 82, 229, 19));
        assert_eq!(full.fill_rgb, [0, 200, 0]);
        assert_eq!(full.outline, rect);
        assert_eq!(bar_paint(rect, 240, 1000).fill_rgb, [0, 200, 0]);
        let yellow = bar_paint(rect, 420, 1000);
        assert_eq!(yellow.fill_rgb, [200, 200, 0]);
        assert_eq!(yellow.fill.w, 233 * 65 / 100 - 4);
        assert_eq!(bar_paint(rect, 481, 1000).fill_rgb, [200, 0, 0]);
        let blue = bar_paint(rect, 600, 120);
        assert_eq!(blue.fill_rgb, [0, 0, 200]);
        assert_eq!(blue.fill.w, 233 * 90 / 100 - 4);
        assert_eq!(bar_paint(rect, 1200, 1000).fill.w, 2);
        assert_eq!(bar_paint(rect, 5000, 1000).fill.w, 2);
    }

    #[test]
    fn appended_lines_keep_fifty_and_show_the_last() {
        let mut page = ConnectionPage::default();
        for n in 0..51 {
            page.lines.push(LanChatLine {
                text: format!("{n}").encode_utf16().collect(),
                color: u32::MAX,
            });
            page.trim_and_show_last(7);
        }
        assert_eq!(page.lines.len(), 50);
        assert_eq!(String::from_utf16_lossy(&page.lines[0].text), "1");
        assert_eq!(page.list.top_index, 43);
    }
}
