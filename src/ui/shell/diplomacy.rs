//! In-battle Diplomacy dialog, RT_DIALOG 0x73 (in-scenario state 8):
//! `Show_Diplomacy_Menu` 0x006586D0, proc 0x00657DA0.
//!
//! Geometry: `ResizeShellChildControl` 0x0060C0C0 sends title 0x694 to the
//! right-panel static (0x00608D4B), Return 0x423 to the lower button
//! (0x006097AB -> 0x00609A81) and status line 0x695 to 0x0060B550; 0x73 is in
//! the 0x0060C3E1 list that sends every other child to 0x0060B7A0. The
//! template, those placements and the 0x006040B0 help keys are pinned by
//! `tools/storage_oracle/diplomacy_shell_layout.py`.
//!
//! The row contents ([`DiplomacyView`]) are resolved by the app from the
//! simulation; this module owns only control identity, geometry and the
//! pointer's help target.

use super::button::{CheckboxFace, ShellButtonInteraction};
use super::geom::{self, RectPx};
use super::in_game_shell::{self, InGameShellLayout};

/// Rows the dialog has: the local player's row 0 and seven others.
pub const DIPLOMACY_ROWS: usize = 8;

/// Template rects, in dialog units, of each row's controls (0x8391FC,
/// 0x83921C, 0x83923C, 0x83925C id tables; insignia 0x004E3320 -> 0x6DA+row).
const NAME_DLU: [RectPx; DIPLOMACY_ROWS] = [
    RectPx::new(111, 68, 110, 10),
    RectPx::new(111, 88, 110, 10),
    RectPx::new(111, 108, 110, 9),
    RectPx::new(111, 128, 110, 8),
    RectPx::new(111, 148, 110, 8),
    RectPx::new(111, 168, 110, 8),
    RectPx::new(111, 188, 110, 8),
    RectPx::new(111, 208, 110, 9),
];
const KILLS_DLU: [RectPx; DIPLOMACY_ROWS] = [
    RectPx::new(316, 68, 41, 10),
    RectPx::new(316, 88, 41, 10),
    RectPx::new(316, 108, 41, 9),
    RectPx::new(316, 128, 41, 8),
    RectPx::new(316, 148, 41, 8),
    RectPx::new(316, 168, 41, 8),
    RectPx::new(316, 188, 41, 8),
    RectPx::new(316, 208, 41, 9),
];
const INSIGNIA_DLU: [RectPx; DIPLOMACY_ROWS] = [
    RectPx::new(69, 66, 32, 12),
    RectPx::new(69, 86, 32, 12),
    RectPx::new(69, 106, 32, 12),
    RectPx::new(69, 126, 32, 12),
    RectPx::new(69, 146, 32, 12),
    RectPx::new(69, 166, 32, 12),
    RectPx::new(69, 186, 32, 12),
    RectPx::new(69, 206, 32, 12),
];
/// Rows 1..=7; row 0 (the local player) has no Allies or Chat box.
const ALLIES_DLU: [RectPx; DIPLOMACY_ROWS - 1] = [
    RectPx::new(226, 88, 10, 10),
    RectPx::new(226, 108, 10, 9),
    RectPx::new(226, 128, 10, 8),
    RectPx::new(226, 148, 10, 10),
    RectPx::new(226, 168, 10, 8),
    RectPx::new(226, 188, 10, 8),
    RectPx::new(226, 208, 10, 9),
];
const CHAT_DLU: [RectPx; DIPLOMACY_ROWS - 1] = [
    RectPx::new(271, 88, 10, 10),
    RectPx::new(271, 108, 10, 10),
    RectPx::new(271, 128, 10, 10),
    RectPx::new(271, 148, 10, 10),
    RectPx::new(271, 168, 10, 10),
    RectPx::new(271, 188, 10, 8),
    RectPx::new(271, 208, 10, 10),
];
const NAME_IDS: [u16; DIPLOMACY_ROWS] = [0x411, 0x412, 0x413, 0x414, 0x415, 0x416, 0x417, 0x418];
const KILLS_IDS: [u16; DIPLOMACY_ROWS] = [0x419, 0x41A, 0x41B, 0x41C, 0x41D, 0x41E, 0x41F, 0x420];
const ALLIES_IDS: [u16; DIPLOMACY_ROWS - 1] = [0x6CF, 0x6D0, 0x6D1, 0x6D2, 0x6D3, 0x6D4, 0x6D5];
const CHAT_IDS: [u16; DIPLOMACY_ROWS - 1] = [0x6D6, 0x6D7, 0x6D8, 0x6D9, 0x6E2, 0x6E3, 0x6E4];

/// Column headers 0x421/0x597/0x425/0x422.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiplomacyHeader {
    Player,
    Allies,
    Chat,
    Kills,
}

impl DiplomacyHeader {
    pub const ALL: [Self; 4] = [Self::Player, Self::Allies, Self::Chat, Self::Kills];

    pub fn label(self) -> (&'static str, &'static str) {
        match self {
            Self::Player => ("GUI:Player", "Player"),
            Self::Allies => ("GUI:Allies", "Allies"),
            Self::Chat => ("GUI:Chat", "Chat"),
            Self::Kills => ("GUI:Kills", "Kills"),
        }
    }

    const fn dlu(self) -> RectPx {
        match self {
            Self::Player => RectPx::new(111, 53, 110, 10),
            Self::Allies => RectPx::new(226, 53, 35, 10),
            Self::Chat => RectPx::new(271, 53, 35, 10),
            Self::Kills => RectPx::new(316, 53, 60, 10),
        }
    }
}

pub const TITLE_LABEL: (&str, &str) = ("GUI:DiplomacyMenu", "Diplomacy");
pub const RETURN_LABEL: (&str, &str) = ("GUI:Return", "Return");
const TAUNTS_DLU: RectPx = RectPx::new(69, 232, 227, 15);
const MODE_NAME_DLU: RectPx = RectPx::new(69, 247, 263, 10);
const MAP_NAME_DLU: RectPx = RectPx::new(69, 262, 263, 10);

/// The dialog's only owner-draw push button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiplomacyButton {
    /// 0x423: the proc kills its timer and ends the modal (0x006581AD).
    Return,
}

/// Pointer state while 0x73 is open: Return's capture and the child whose
/// help the status line shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiplomacyInteraction {
    pub button: ShellButtonInteraction<DiplomacyButton>,
    pub hovered: Option<DiplomacyControl>,
}

/// A child the pointer can rest on, for the status-line help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiplomacyControl {
    Return,
    Header(DiplomacyHeader),
    Insignia(usize),
    Name(usize),
    Kills(usize),
    /// Rows 1..=7.
    Allies(usize),
    /// Rows 1..=7.
    Chat(usize),
    Taunts,
    ModeName,
    MapName,
}

impl DiplomacyControl {
    /// The status-line help key: the 0x006040B0 table for dialog 0x73
    /// (`diplomacy_shell_layout.json`). The 0x73 proc answers no 0x4E9.
    pub fn help_key(self) -> Option<&'static str> {
        match self {
            Self::Return => Some("STT:DiplomacyButtonReturn"),
            Self::Name(_) => Some("STT:DiplomacyLabelPlayer"),
            Self::Kills(_) => Some("STT:DiplomacyCBoxKills"),
            Self::Allies(_) => Some("STT:DiplomacyCBoxAllies"),
            Self::Chat(_) => Some("STT:DiplomacyCBoxListeners"),
            Self::Taunts => Some("STT:TauntsOn"),
            Self::Header(_) | Self::Insignia(_) | Self::ModeName | Self::MapName => None,
        }
    }

    /// Native control id.
    pub fn id(self) -> u16 {
        match self {
            Self::Return => 0x423,
            Self::Header(DiplomacyHeader::Player) => 0x421,
            Self::Header(DiplomacyHeader::Allies) => 0x597,
            Self::Header(DiplomacyHeader::Chat) => 0x425,
            Self::Header(DiplomacyHeader::Kills) => 0x422,
            Self::Insignia(row) => 0x6DA + row as u16,
            Self::Name(row) => NAME_IDS[row],
            Self::Kills(row) => KILLS_IDS[row],
            Self::Allies(row) => ALLIES_IDS[row - 1],
            Self::Chat(row) => CHAT_IDS[row - 1],
            Self::Taunts => 0x730,
            Self::ModeName => 0x426,
            Self::MapName => 0x71B,
        }
    }
}

/// Physical rectangles of every control in the active-game shell's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiplomacyLayout {
    pub title: RectPx,
    pub return_button: RectPx,
    pub status_line: RectPx,
    pub headers: [RectPx; 4],
    pub insignia: [RectPx; DIPLOMACY_ROWS],
    pub names: [RectPx; DIPLOMACY_ROWS],
    pub kills: [RectPx; DIPLOMACY_ROWS],
    /// Index 0 is unused: row 0 has no box.
    pub allies: [RectPx; DIPLOMACY_ROWS],
    pub chat: [RectPx; DIPLOMACY_ROWS],
    pub taunts: RectPx,
    pub mode_name: RectPx,
    pub map_name: RectPx,
}

pub fn diplomacy_layout(
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button_size: [i32; 2],
) -> DiplomacyLayout {
    let ordinary = |dlu: RectPx| {
        in_game_shell::ordinary_child_rect(
            geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h),
            width,
            height,
        )
    };
    let boxes = |table: &[RectPx; DIPLOMACY_ROWS - 1]| {
        std::array::from_fn(|row| {
            if row == 0 {
                RectPx::new(0, 0, 0, 0)
            } else {
                ordinary(table[row - 1])
            }
        })
    };
    DiplomacyLayout {
        title: in_game_shell::right_panel_title_rect(width),
        return_button: shell.lower_button_rect(width, button_size),
        status_line: in_game_shell::status_line_rect(height),
        headers: DiplomacyHeader::ALL.map(|header| ordinary(header.dlu())),
        insignia: INSIGNIA_DLU.map(ordinary),
        names: NAME_DLU.map(ordinary),
        kills: KILLS_DLU.map(ordinary),
        allies: boxes(&ALLIES_DLU),
        chat: boxes(&CHAT_DLU),
        taunts: ordinary(TAUNTS_DLU),
        mode_name: ordinary(MODE_NAME_DLU),
        map_name: ordinary(MAP_NAME_DLU),
    }
}

impl DiplomacyLayout {
    pub fn control_rect(&self, control: DiplomacyControl) -> RectPx {
        match control {
            DiplomacyControl::Return => self.return_button,
            DiplomacyControl::Header(header) => self.headers[header as usize],
            DiplomacyControl::Insignia(row) => self.insignia[row],
            DiplomacyControl::Name(row) => self.names[row],
            DiplomacyControl::Kills(row) => self.kills[row],
            DiplomacyControl::Allies(row) => self.allies[row],
            DiplomacyControl::Chat(row) => self.chat[row],
            DiplomacyControl::Taunts => self.taunts,
            DiplomacyControl::ModeName => self.mode_name,
            DiplomacyControl::MapName => self.map_name,
        }
    }
}

/// One Allies box: the dual-state checkbox 0x006163A0 after 0x4E5(1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiplomacyAlliesBox {
    /// BM_SETCHECK: the local player allies this house.
    pub checked: bool,
    /// 0x4E6: this house allies the local player.
    pub partner: bool,
    /// Enabled only for another human while the local player is active.
    pub enabled: bool,
}

impl DiplomacyAlliesBox {
    pub fn face(self) -> CheckboxFace {
        CheckboxFace::dual_state(self.checked, self.partner)
    }
}

/// One shown row (init 0x497 arm, refresh 0x00658330).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiplomacyRow {
    /// The 0x004E3560 insignia PCX, when the house type has one.
    pub insignia: Option<&'static str>,
    /// House UI name (House+0x1602A).
    pub name: String,
    /// Name and kills text colour (0x498).
    pub rgb: [u8; 3],
    /// Kills, formatted `%d`; `None` leaves the static blank.
    pub kills: Option<i32>,
    /// `None` when hidden; row 0 never has one.
    pub allies: Option<DiplomacyAlliesBox>,
    /// `None` when hidden. Shown only for human rows.
    pub chat: Option<DiplomacyChatBox>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiplomacyChatBox {
    pub checked: bool,
    pub enabled: bool,
}

/// The dialog's resolved contents.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiplomacyView {
    pub rows: [Option<DiplomacyRow>; DIPLOMACY_ROWS],
    /// 0x597, hidden when no row showed an Allies box at init.
    pub allies_header: bool,
    /// 0x425, hidden when no row is human-controlled.
    pub chat_header: bool,
    /// 0x426 MPGameMode title, set only in LAN/WOL; otherwise `GUI:Blank`.
    pub mode_name: Option<String>,
    /// 0x71B: the selected scenario's description (Session+0xEA).
    pub map_name: String,
}

impl DiplomacyView {
    /// The visible child under a pointer, in template (Z) order, as the
    /// common handler's `ChildWindowFromPointEx(.., CWP_SKIPINVISIBLE)`
    /// (0x00622D24) finds it. Disabled children still answer.
    pub fn control_at(&self, layout: &DiplomacyLayout, x: i32, y: i32) -> Option<DiplomacyControl> {
        self.visible_controls()
            .find(|control| layout.control_rect(*control).contains(x, y))
    }

    fn visible_controls(&self) -> impl Iterator<Item = DiplomacyControl> + '_ {
        let rows = (0..DIPLOMACY_ROWS).filter(|row| self.rows[*row].is_some());
        let row_controls = rows.flat_map(move |row| {
            let shown = self.rows[row].as_ref().expect("filtered");
            [
                Some(DiplomacyControl::Insignia(row)),
                Some(DiplomacyControl::Name(row)),
                Some(DiplomacyControl::Kills(row)),
                shown.allies.map(|_| DiplomacyControl::Allies(row)),
                shown.chat.map(|_| DiplomacyControl::Chat(row)),
            ]
            .into_iter()
            .flatten()
        });
        let headers = DiplomacyHeader::ALL
            .into_iter()
            .filter(|header| match header {
                DiplomacyHeader::Allies => self.allies_header,
                DiplomacyHeader::Chat => self.chat_header,
                DiplomacyHeader::Player | DiplomacyHeader::Kills => true,
            });
        std::iter::once(DiplomacyControl::Return)
            .chain(row_controls)
            .chain(headers.map(DiplomacyControl::Header))
            .chain([DiplomacyControl::ModeName, DiplomacyControl::MapName])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::in_game_shell::InGameShellSizes;

    fn corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/diplomacy_shell_layout.json",
        ))
        .unwrap()
    }

    fn all_controls() -> Vec<DiplomacyControl> {
        let mut out = vec![DiplomacyControl::Return];
        out.extend(DiplomacyHeader::ALL.map(DiplomacyControl::Header));
        for row in 0..DIPLOMACY_ROWS {
            out.extend([
                DiplomacyControl::Insignia(row),
                DiplomacyControl::Name(row),
                DiplomacyControl::Kills(row),
            ]);
            if row > 0 {
                out.extend([DiplomacyControl::Allies(row), DiplomacyControl::Chat(row)]);
            }
        }
        out.extend([
            DiplomacyControl::Taunts,
            DiplomacyControl::ModeName,
            DiplomacyControl::MapName,
        ]);
        out
    }

    /// The Allied shell at a stock resolution, from the executed geometry
    /// fixture's loaded canvas sizes.
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

    /// Every ordinary child lands where original 0x0060B7A0 put it at 640x480,
    /// 800x600 and 1024x768, from the pinned template's own rects.
    #[test]
    fn ordinary_children_match_the_executed_native_placement() {
        let corpus = corpus();
        let template: std::collections::HashMap<u64, Vec<i64>> = corpus["controls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let rect = c["dlu_rect"].as_array().unwrap();
                (
                    c["control_id"].as_u64().unwrap(),
                    rect.iter().map(|v| v.as_i64().unwrap()).collect(),
                )
            })
            .collect();
        let cases = corpus["cases"].as_array().unwrap();
        let controls = all_controls();
        let mut checked = 0;
        for control in &controls {
            if *control == DiplomacyControl::Return {
                continue;
            }
            let id = u64::from(control.id());
            for case in cases.iter().filter(|case| case["control_id"] == id) {
                let (width, height) = (
                    case["width"].as_i64().unwrap() as i32,
                    case["height"].as_i64().unwrap() as i32,
                );
                let layout = diplomacy_layout(width, height, shell(width, height), [125, 25]);
                let rect = layout.control_rect(*control);
                assert_eq!(
                    serde_json::json!([rect.x, rect.y, rect.w, rect.h]),
                    case["rect"],
                    "control {id:#x} at {width}x{height} (template {:?})",
                    template[&id]
                );
                checked += 1;
            }
        }
        // 4 headers, 8x3 row statics, 7x2 boxes, taunts, mode and map names.
        assert_eq!(checked, (4 + 24 + 14 + 3) * 3);
    }

    /// Title 0x694, Return 0x423 and status line 0x695 take B5's title,
    /// Resume and footer places (0x00608D4B, 0x006097AB, 0x00601360).
    #[test]
    fn title_return_and_status_line_share_the_pause_menu_placements() {
        use crate::ui::shell::pause_menu::{PauseMenuButton, pause_menu_layout};
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            let shell = shell(width, height);
            let layout = diplomacy_layout(width, height, shell, [125, 25]);
            let pause = pause_menu_layout(width, height, shell, [125, 25]);
            assert_eq!(layout.title, pause.title);
            assert_eq!(
                layout.return_button,
                pause.buttons[PauseMenuButton::Resume as usize]
            );
            assert_eq!(layout.status_line, pause.footer);
        }
    }

    /// The help keys are the executed 0x006040B0 answers for dialog 0x73.
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
                control.help_key().map(str::to_owned),
                native["help"].as_str().map(str::to_owned),
                "{control:?}"
            );
        }
    }

    #[test]
    fn hidden_children_take_no_pointer_and_disabled_ones_do() {
        let layout = diplomacy_layout(800, 600, shell(800, 600), [125, 25]);
        let row = |allies| DiplomacyRow {
            insignia: None,
            name: String::new(),
            rgb: [0; 3],
            kills: None,
            allies,
            chat: None,
        };
        let disabled = DiplomacyAlliesBox {
            checked: false,
            partner: false,
            enabled: false,
        };
        let mut view = DiplomacyView {
            rows: [
                Some(row(None)),
                Some(row(Some(disabled))),
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            ..DiplomacyView::default()
        };
        let allies = layout.allies[1];
        assert_eq!(
            view.control_at(&layout, allies.x, allies.y),
            Some(DiplomacyControl::Allies(1))
        );
        let chat = layout.chat[1];
        assert_eq!(view.control_at(&layout, chat.x, chat.y), None);
        let name2 = layout.names[2];
        assert_eq!(view.control_at(&layout, name2.x, name2.y), None);
        view.rows[1] = Some(row(None));
        assert_eq!(view.control_at(&layout, allies.x, allies.y), None);
    }
}
