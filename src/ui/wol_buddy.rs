//! The Buddy List `0x116` (proc `0x0079E520`, runner `0x0079F140`) and its
//! Add Buddy popup `0x115` (proc `0x0079DFE0`). `0x116` is a family page
//! (`0x0060C540` accepts it): the owner-draw buddy list `0x689`, Refresh
//! `0x704`, the observer toggle, the selected buddy's ladder statistics and
//! the command column.
//!
//! The model owns the children and the proc's control logic; the buddy
//! mirror, the ladder table and the runner belong to the app's WOL owners,
//! which run each returned [`WolBuddyAction`].

use crate::ui::lan_shell::{LanEditKey, LanListState};
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{
    RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, child_window, dlu_rect,
};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::skirmish_shell::SavedSeedDescriptionEdit;
use crate::ui::wol_lobby::{AdmissionContext, LOBBY_PREFIX, WolChannel, WolLobbyRow, admit};

pub(crate) const DIALOG: DialogId = DialogId(0x0116);
pub(crate) const ADD_DIALOG: DialogId = DialogId(0x0115);
pub(crate) const CREATE: u16 = 0x062B;
pub(crate) const JOIN: u16 = 0x062D;
pub(crate) const PAGE_BUTTON: u16 = 0x05C2;
pub(crate) const ADD: u16 = 0x0702;
pub(crate) const REMOVE: u16 = 0x0703;
pub(crate) const BACK: u16 = 0x0686;
pub(crate) const LIST: u16 = 0x0689;
pub(crate) const REFRESH: u16 = 0x0704;
pub(crate) const HEADING: u16 = 0x06EF;
pub(crate) const WINS: u16 = 0x06F0;
pub(crate) const LOSSES: u16 = 0x06F1;
pub(crate) const DISCONNECTS: u16 = 0x0701;
pub(crate) const RANK: u16 = 0x06F4;
pub(crate) const POINTS: u16 = 0x06F5;
pub(crate) const OBSERVER_IMAGE: u16 = 0x075F;
pub(crate) const OBSERVER_TOGGLE: u16 = 0x0760;
/// `0x115`'s name edit.
pub(crate) const NAME: u16 = 0x0540;

/// The value statics in template order.
pub(crate) const STATS: [u16; 5] = [WINS, LOSSES, DISCONNECTS, RANK, POINTS];

/// The list's column stops (`0x4A6` at `0x497`, x from `0x0084A1C4`): name,
/// game type, password, limit/observer, status and ping.
pub(crate) const NAME_X: i32 = 4;
pub(crate) const NAME_W: i32 = 202;
pub(crate) const TYPE_X: i32 = 210;
pub(crate) const PASSWORD_X: i32 = 227;
pub(crate) const LIMIT_X: i32 = 240;
pub(crate) const STATUS_X: i32 = 257;
pub(crate) const STATUS_W: i32 = 155;
pub(crate) const PING_X: i32 = 416;

/// Refresh's pictures (`0x49C`/`0x4AA`), the lobby's pair.
pub(crate) const REFRESH_FACES: (&str, &str) = ("wouref.pcx", "wodref.pcx");

/// Row colours (`COLORREF`, `00BBGGRR`).
pub(crate) const GREY: u32 = 0x0080_8080;
pub(crate) const ONLINE_COLOR: u32 = 0x0000_F0F0;
pub(crate) const CHAT_COLOR: u32 = 0x00FF_4000;
pub(crate) const GAME_COLOR: u32 = 0x00FF_FF00;
pub(crate) const PLAYING_COLOR: u32 = 0x0030_50FF;
pub(crate) const QUICK_MATCH_COLOR: u32 = 0x0080_FF00;

const fn button(
    id: u16,
    dlu_top: i32,
    csf_key: &'static str,
    tooltip_key: &'static str,
) -> MenuPageButtonSpec {
    MenuPageButtonSpec {
        id,
        dlu_top,
        csf_key,
        tooltip_key,
        result: None,
    }
}

/// RT_DIALOG `0x116`: Create Game, Join, Page, Add and Remove stacked by
/// their template tops (Join, Page and Remove created disabled), Back on
/// the bottom row. Help keys from the executed `0x006040B0`.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: "GUI:BuddyTitle",
    stacked: &[
        button(CREATE, 116, "GUI:CreateGame", "STT:WOLBuddyCreateGame"),
        button(JOIN, 144, "GUI:Join", "STT:WOLBuddyJoin"),
        button(PAGE_BUTTON, 172, "GUI:Page", "STT:WOLBuddyPage"),
        button(ADD, 200, "GUI:AddBuddy", "STT:WOLBuddyAdd"),
        button(REMOVE, 228, "GUI:RemoveBuddy", "STT:WOLBuddyRemove"),
    ],
    back: button(BACK, 256, "GUI:Back", "STT:WOLBuddyBack"),
};

/// The captions: the two column headers, then the statistics' labels.
pub(crate) const LABELS: [(&str, [i32; 4]); 7] = [
    ("GUI:BuddyName", [27, 9, 135, 11]),
    ("GUI:Status", [177, 9, 100, 11]),
    ("GUI:LadderWins", [89, 237, 115, 13]),
    ("GUI:LadderLosses", [89, 255, 115, 13]),
    ("GUI:Disconnects", [89, 273, 115, 13]),
    ("GUI:LadderRank", [89, 290, 115, 13]),
    ("GUI:LadderPoints", [89, 309, 115, 13]),
];

/// The statistics' template caption, kept until the first fill.
pub(crate) const UNKNOWN_STATS_KEY: &str = "GUI:UnknownStats";

/// Status help of `0x116`'s children (executed `0x006040B0`).
pub(crate) fn tooltip_key(control: u16) -> Option<&'static str> {
    if let Some(spec) = PAGE.button(control) {
        return Some(spec.tooltip_key);
    }
    Some(match control {
        LIST => "STT:WOLBuddyBuddyList",
        REFRESH => "STT:WOLBuddyRefresh",
        WINS => "STT:WOLBuddyWins",
        LOSSES => "STT:WOLBuddyLosses",
        DISCONNECTS => "STT:WOLBuddyDisconnects",
        RANK => "STT:WOLBuddyRank",
        POINTS => "STT:WOLBuddyPoints",
        OBSERVER_TOGGLE => "STT:ObserverToggle",
        _ => return None,
    })
}

/// The status texts `0x0079AF10` loads, resolved by the caller.
#[derive(Debug, Clone, Default)]
pub(crate) struct BuddyTexts {
    pub(crate) find_disabled: String,
    pub(crate) offline: String,
    pub(crate) online: String,
    pub(crate) playing: String,
    pub(crate) quick_matching: String,
    pub(crate) unknown: String,
    /// `TXT_LOB_1..8`.
    pub(crate) lobby_names: [String; 8],
}

impl BuddyTexts {
    /// The CSF keys in field order.
    pub(crate) const KEYS: [&str; 6] = [
        "WOL:BuddyDisabledFind",
        "WOL:BuddyOffline",
        "WOL:BuddyOnline",
        "WOL:BuddyPlayingGame",
        "WOL:BuddyQuickMatching",
        "WOL:BuddyUnknownStatus",
    ];
}

/// The type column of a buddy's listed game: `woltrny` for a tournament,
/// `wolclan` for a clan game, else `gt18.bmp`, which the PCX cache does
/// not hold, so nothing shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuddyGameType {
    Plain,
    Clan,
    Tournament,
}

impl BuddyGameType {
    pub(crate) fn file(self) -> Option<&'static str> {
        match self {
            Self::Plain => None,
            Self::Clan => Some("wolclan.pcx"),
            Self::Tournament => Some("woltrny.pcx"),
        }
    }
}

/// The cells of a buddy in a listed game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuddyGameCells {
    pub(crate) kind: BuddyGameType,
    pub(crate) keyed: bool,
    pub(crate) limit_icon: Option<&'static str>,
    /// The integer cell (type 3) at x=416.
    pub(crate) ping: i32,
}

/// One buddy row (`0x0079AF10`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolBuddyRow {
    pub(crate) name: String,
    /// The x=257 cell.
    pub(crate) status: String,
    /// The name's and the status's colour.
    pub(crate) color: u32,
    pub(crate) game: Option<BuddyGameCells>,
}

/// `0x0079AF10`'s cells for one buddy: `status` from the status map,
/// `channel` its record in the channel map and `games` the game list.
pub(crate) fn buddy_row(
    name: &str,
    status: Option<i32>,
    channel: Option<&WolChannel>,
    games: &[WolChannel],
    texts: &BuddyTexts,
) -> WolBuddyRow {
    let row = |status: &str, color| WolBuddyRow {
        name: name.to_owned(),
        status: status.to_owned(),
        color,
        game: None,
    };
    match status.unwrap_or(0) {
        1 => row(&texts.find_disabled, GREY),
        2 => row(&texts.offline, GREY),
        3 => row(&texts.online, ONLINE_COLOR),
        4 => match channel {
            // A chat lobby shows as TXT_JOINED_S does.
            Some(channel) if channel.kind == 0 && channel.name.starts_with(LOBBY_PREFIX) => row(
                &WolLobbyRow::joined_name(channel.lobby_number().unwrap_or(0), &texts.lobby_names),
                CHAT_COLOR,
            ),
            Some(channel) => row(&channel.name, CHAT_COLOR),
            None => row(&texts.online, ONLINE_COLOR),
        },
        5 => {
            let listed = channel.and_then(|channel| {
                games.iter().find(|game| {
                    crate::util::native_string::crt_strcmpi(&game.name, &channel.name).is_eq()
                })
            });
            let Some(game) = listed else {
                return row(&texts.online, ONLINE_COLOR);
            };
            let (shown, capacity) = game.shown_capacity();
            WolBuddyRow {
                game: Some(BuddyGameCells {
                    kind: if game.tournament != 0 {
                        BuddyGameType::Tournament
                    } else if game.game_flags & crate::ui::wol_lobby::GAME_CLAN != 0 {
                        BuddyGameType::Clan
                    } else {
                        BuddyGameType::Plain
                    },
                    keyed: game.keyed(),
                    limit_icon: game.limit_icon(),
                    ping: game.ping,
                }),
                ..row(&format!("{} {shown}/{capacity}", game.name), GAME_COLOR)
            }
        }
        6 => row(&texts.playing, PLAYING_COLOR),
        7 | 8 => row(&texts.quick_matching, QUICK_MATCH_COLOR),
        _ => row(&texts.unknown, GREY),
    }
}

/// `0x0079BC80`: Join may run for the selected buddy: in a chat channel,
/// or in a listed game the admission `0x0079BA60` takes silently.
pub(crate) fn join_enabled(
    selected: &str,
    status: Option<i32>,
    channel: Option<&WolChannel>,
    games: &[WolChannel],
    observer: bool,
) -> bool {
    if selected.is_empty() {
        return false;
    }
    match status {
        Some(4) => true,
        Some(5) => channel
            .and_then(|channel| {
                games.iter().find(|game| {
                    crate::util::native_string::crt_strcmpi(&game.name, &channel.name).is_eq()
                })
            })
            .is_some_and(|game| {
                admit(
                    game,
                    observer,
                    AdmissionContext {
                        quick_match: false,
                        clan_id: 0,
                        local_hash: None,
                    },
                )
                .is_ok()
            }),
        _ => false,
    }
}

/// Page follows a selected buddy whose status is known and outside 0..2.
pub(crate) fn page_enabled(selected: &str, status: Option<i32>) -> bool {
    !selected.is_empty() && status.is_some_and(|status| !(0..=2).contains(&status))
}

/// What one `0x116` or `0x115` interaction asks the app's owner to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolBuddyAction {
    /// Create (`0x62B`), Join (`0x62D`) or Back (`0x686`): the dialog ends
    /// with that id and the runner acts on it.
    End(u16),
    /// LBN_SELCHANGE: `[0xB76E24]` becomes the row's name (empty for none).
    Select(String),
    /// Page `0x5C2`: the modal `0x11E`.
    Page,
    /// Add `0x702`: the modal `0x115`.
    OpenAdd,
    /// Remove `0x703` on the selected row (no confirmation).
    Remove(String),
    /// Refresh `0x704`.
    Refresh,
    /// `0x75F`/`0x760`: `[0xAC10C8]`.
    Observer(bool),
    /// `0x115`'s Add with the edit's text; the dialog always ends.
    Add(String),
    /// `0x115`'s Back.
    CloseAdd,
}

/// The Add Buddy popup `0x115` while it runs.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolAddBuddy {
    pub(crate) edit: SavedSeedDescriptionEdit,
    /// `0x702`, created disabled; `0x0079C520` of the text.
    pub(crate) add_enabled: bool,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) focus_at: Option<std::time::Instant>,
}

/// `EM_LIMITTEXT 9` and `0x4D1(1)` (byte characters only).
const NAME_LIMIT: usize = 9;

impl WolAddBuddy {
    /// `0x497`: an empty edit with the focus, Add disabled.
    pub(crate) fn new() -> Self {
        let mut popup = Self {
            focus_at: Some(std::time::Instant::now()),
            ..Self::default()
        };
        popup.edit.focused = true;
        popup
    }

    pub(crate) fn text(&self) -> String {
        self.edit.narrow_text(usize::MAX)
    }

    /// Every edit notification re-reads the text: Add follows `valid`.
    pub(crate) fn insert_text(&mut self, text: &str, valid: &dyn Fn(&str) -> bool) {
        for unit in text.encode_utf16() {
            if self.edit.insert_unit_filtered(unit, NAME_LIMIT, true, &[]) {
                self.add_enabled = valid(&self.text());
            }
        }
    }

    /// Enter (`0x4DA`) posts `BM_CLICK` to Add, which a disabled button
    /// ignores.
    pub(crate) fn key(
        &mut self,
        key: LanEditKey,
        valid: &dyn Fn(&str) -> bool,
    ) -> Option<WolBuddyAction> {
        let before = self.edit.units.clone();
        match key {
            LanEditKey::Enter => return self.activate(ADD),
            LanEditKey::Backspace => self.edit.backspace(),
            LanEditKey::Delete => self.edit.delete(),
            LanEditKey::Left => self.edit.left(),
            LanEditKey::Right => self.edit.right(),
            LanEditKey::Home => self.edit.home(),
            LanEditKey::End => self.edit.end(),
            LanEditKey::Tab => {}
        }
        if self.edit.units != before {
            self.add_enabled = valid(&self.text());
        }
        None
    }

    /// `0x702` reads and clears the edit and always ends the dialog; Back
    /// ends it.
    pub(crate) fn activate(&mut self, control: u16) -> Option<WolBuddyAction> {
        match control {
            ADD if self.add_enabled => {
                let text = self.text();
                self.edit.set_units_limited([], 0);
                Some(WolBuddyAction::Add(text))
            }
            BACK => Some(WolBuddyAction::CloseAdd),
            _ => None,
        }
    }
}

/// `0x116` while it runs.
#[derive(Debug, Clone)]
pub(crate) struct WolBuddyPage {
    pub(crate) rows: Vec<WolBuddyRow>,
    pub(crate) list: LanListState,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    disabled: Vec<u16>,
    /// `[0xAC10C8]`'s view; the multiplayer preference owner keeps it.
    pub(crate) observer: bool,
    /// The heading and statistics `0x0079C040` wrote; `None` keeps the
    /// template captions.
    pub(crate) texts: Option<Vec<(u16, String)>>,
    /// The modal Add Buddy `0x115`.
    pub(crate) add: Option<WolAddBuddy>,
}

impl WolBuddyPage {
    /// `0x497`: an empty list; Join, Page and Remove disabled.
    pub(crate) fn new(observer: bool) -> Self {
        Self {
            rows: Vec::new(),
            list: LanListState::default(),
            buttons: ShellButtonInteraction::default(),
            disabled: vec![JOIN, PAGE_BUTTON, REMOVE],
            observer,
            texts: None,
            add: None,
        }
    }

    pub(crate) fn enabled(&self, control: u16) -> bool {
        !self.disabled.contains(&control)
    }

    pub(crate) fn set_enabled(&mut self, control: u16, enabled: bool) {
        self.disabled.retain(|known| *known != control);
        if !enabled {
            self.disabled.push(control);
            if self.buttons.pressed == Some(control) {
                self.buttons.pressed = None;
            }
        }
    }

    pub(crate) fn cancel_pointer(&mut self) {
        self.buttons = ShellButtonInteraction::default();
        self.list.scroll.cancel();
        self.list.last_press = None;
        if let Some(add) = self.add.as_mut() {
            add.buttons = ShellButtonInteraction::default();
        }
    }

    /// `0x0079AF10`'s loop: each buddy's row found by name (`0x4C5`) is
    /// updated in place, else appended (`0x4C8`); rows leave only through
    /// Remove. Then `selected`'s row is selected again (none for an empty
    /// name or an unlisted one).
    pub(crate) fn update_rows(&mut self, rows: Vec<WolBuddyRow>, selected: &str) {
        for row in rows {
            match self
                .rows
                .iter_mut()
                .find(|known| known.name.eq_ignore_ascii_case(&row.name))
            {
                Some(known) => *known = row,
                None => self.rows.push(row),
            }
        }
        self.list.selected = (!selected.is_empty())
            .then(|| {
                self.rows
                    .iter()
                    .position(|row| row.name.eq_ignore_ascii_case(selected))
            })
            .flatten();
        self.list.hot_row = None;
    }

    /// The enables `0x0079AF10` and LBN_SELCHANGE end with.
    pub(crate) fn set_enables(&mut self, remove: bool, join: bool, page: bool) {
        self.set_enabled(REMOVE, remove);
        self.set_enabled(JOIN, join);
        self.set_enabled(PAGE_BUTTON, page);
    }

    pub(crate) fn selected_name(&self) -> Option<&str> {
        self.list
            .selected
            .and_then(|row| self.rows.get(row))
            .map(|row| row.name.as_str())
    }

    /// A press on a row (`LBN_SELCHANGE`).
    pub(crate) fn select(&mut self, row: usize) -> Option<WolBuddyAction> {
        let name = self.rows.get(row)?.name.clone();
        self.list.selected = Some(row);
        Some(WolBuddyAction::Select(name))
    }

    /// `LBN_DBLCLK` (`0x0079EB52`): `BM_CLICK` on Join when it may run,
    /// else on Page for a known status outside 0..2. The clicks reach only
    /// an enabled button.
    pub(crate) fn double_click(&mut self, join: bool, page: bool) -> Option<WolBuddyAction> {
        if join {
            self.activate(JOIN)
        } else if page {
            self.activate(PAGE_BUTTON)
        } else {
            None
        }
    }

    /// Remove's `LB_DELETESTRING` and the cleared selection.
    pub(crate) fn delete_row(&mut self, name: &str) {
        if let Some(index) = self.rows.iter().position(|row| row.name == name) {
            self.rows.remove(index);
        }
        self.list.selected = None;
    }

    /// A released button or picture.
    pub(crate) fn activate(&mut self, control: u16) -> Option<WolBuddyAction> {
        if !self.enabled(control) {
            return None;
        }
        Some(match control {
            CREATE | JOIN | BACK => WolBuddyAction::End(control),
            PAGE_BUTTON => WolBuddyAction::Page,
            ADD => WolBuddyAction::OpenAdd,
            REMOVE => WolBuddyAction::Remove(self.selected_name()?.to_owned()),
            REFRESH => {
                // 0x0079EE87: disabled until its 3000 ms timer or the list.
                self.set_enabled(REFRESH, false);
                WolBuddyAction::Refresh
            }
            OBSERVER_IMAGE | OBSERVER_TOGGLE => {
                self.observer = !self.observer;
                WolBuddyAction::Observer(self.observer)
            }
            _ => return None,
        })
    }
}

/// `0x116` at the current screen: the family right panel and status line,
/// the left children at their template places (`0x0060B950`).
#[derive(Debug, Clone)]
pub(crate) struct WolBuddyLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) status_help: RectPx,
    /// [`LABELS`] order.
    pub(crate) labels: [RectPx; 7],
    pub(crate) list: RectPx,
    pub(crate) refresh: RectPx,
    pub(crate) heading: RectPx,
    pub(crate) group_box: RectPx,
    /// [`STATS`] order.
    pub(crate) stats: [RectPx; 5],
    pub(crate) observer_image: RectPx,
    pub(crate) observer_toggle: RectPx,
}

impl WolBuddyLayout {
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::RETAIL)
    }

    pub(crate) fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height))
    }

    fn build(width: i32, height: i32, scale: ShellScale) -> Self {
        let status_template = RectPx::new(2, 355, 316, 12);
        let (mut page, status_help) = if scale.is_retail() {
            (
                menu_page::compute_layout(&PAGE, width as u32, height as u32),
                status_line_rect(status_template, width, height),
            )
        } else {
            (
                menu_page::compute_responsive_layout(&PAGE, width as u32, height as u32),
                scale.scale_rect(status_line_rect(
                    status_template,
                    SHELL_BASE_W,
                    SHELL_BASE_H,
                )),
            )
        };
        // Heading 0x694 is 108x11 DLU, one row taller than the family's.
        let mut title =
            menu_page::compute_layout(&PAGE, SHELL_BASE_W as u32, SHELL_BASE_H as u32).title;
        title.h = dlu_rect(426, 4, 108, 11).h + 1;
        if scale.is_retail() {
            page.title.h = title.h;
        } else {
            page.title = scale.scale_rect(title);
        }
        let px = |[x, y, w, h]: [i32; 4]| scale.scale_rect(child_window(x, y, w, h));
        Self {
            page,
            status_help,
            labels: LABELS.map(|(_, rect)| px(rect)),
            list: px([6, 22, 408, 182]),
            refresh: px([5, 6, 17, 14]),
            heading: px([80, 213, 255, 15]),
            group_box: px([80, 227, 255, 97]),
            stats: [237, 255, 273, 290, 309].map(|top| px([216, top, 110, 13])),
            observer_image: px([395, 8, 18, 12]),
            observer_toggle: px([383, 9, 10, 10]),
        }
    }

    /// The child under `(x, y)`: buttons first, then the template's other
    /// children with input or help.
    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        if let Some(button) = self.page.buttons.iter().find(|b| b.rect.contains(x, y)) {
            return Some(button.id);
        }
        [
            (REFRESH, self.refresh),
            (OBSERVER_TOGGLE, self.observer_toggle),
            (OBSERVER_IMAGE, self.observer_image),
            (LIST, self.list),
        ]
        .into_iter()
        .chain(STATS.iter().copied().zip(self.stats))
        .find_map(|(id, rect)| rect.contains(x, y).then_some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::wol_lobby::{CHANNEL_KEYED, EX_INFO_VERSION, GAME_CHANNEL, GAME_LIMIT_800};

    fn rect(value: &serde_json::Value) -> RectPx {
        let a: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
        RectPx::new(a[0], a[1], a[2], a[3])
    }

    fn corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap()
    }

    #[test]
    fn layout_help_and_slide_follow_the_executed_tables() {
        let corpus = corpus();
        let mut checked = 0;
        for (size, case) in corpus["layouts"]["0x116"]["sizes"].as_object().unwrap() {
            let (width, height) = size.split_once('x').unwrap();
            let layout = WolBuddyLayout::new(width.parse().unwrap(), height.parse().unwrap());
            for (control, child) in case["controls"].as_object().unwrap() {
                let actual = match control.as_str() {
                    "0x71C" => layout.page.warning_monitor,
                    "0x694" => layout.page.title,
                    "0x695" => layout.status_help,
                    "0x6EF" => layout.heading,
                    "0x689" => layout.list,
                    "0x704" => layout.refresh,
                    "0x75F" => layout.observer_image,
                    "0x760" => layout.observer_toggle,
                    "0xFFFF#2" => layout.group_box,
                    key if key.starts_with("0xFFFF") => {
                        let index = key
                            .strip_prefix("0xFFFF#")
                            .map_or(0, |n| n.parse::<usize>().unwrap());
                        let index = if index > 2 { index - 1 } else { index };
                        assert_eq!(
                            child["caption"].as_str(),
                            Some(LABELS[index].0),
                            "{size} {key}"
                        );
                        layout.labels[index]
                    }
                    key => {
                        let id = u16::from_str_radix(&key[2..], 16).unwrap();
                        match STATS.iter().position(|s| *s == id) {
                            Some(index) => layout.stats[index],
                            None => layout
                                .page
                                .button_rect(id)
                                .unwrap_or_else(|| panic!("{size} {key}")),
                        }
                    }
                };
                assert_eq!(actual, rect(&child["rect"]), "{size} {control}");
                checked += 1;
            }
        }
        assert_eq!(checked, 3 * 27);
        for (control, help) in corpus["help"]["0x116"].as_object().unwrap() {
            let id = u16::from_str_radix(&control[2..], 16).unwrap();
            // The heading has no help; the observer image answers nothing.
            assert_eq!(tooltip_key(id), help.as_str(), "{control}");
        }
        let record = &corpus["slides"]["0x116"];
        let slide = crate::ui::shell::slide::slide_spec_for(DIALOG).unwrap();
        assert_eq!(
            slide.top_buttons as usize,
            record["top"].as_array().unwrap().len()
        );
        assert_eq!(
            slide.bottom_button,
            !record["bottom"].as_array().unwrap().is_empty()
        );
        assert_eq!(
            (slide.top_panel, slide.map_button),
            (record["d5"] != 0, record["d6"] != 0)
        );
    }

    fn texts() -> BuddyTexts {
        BuddyTexts {
            find_disabled: "find off".into(),
            offline: "offline".into(),
            online: "online".into(),
            playing: "playing".into(),
            quick_matching: "qm".into(),
            unknown: "unknown".into(),
            lobby_names: std::array::from_fn(|index| format!("L{index}")),
        }
    }

    fn game(name: &str) -> WolChannel {
        WolChannel {
            kind: GAME_CHANNEL,
            max_users: 7,
            users: 2,
            ping: 180,
            name: name.into(),
            ex_info: format!("{EX_INFO_VERSION},0,0,0,1,"),
            ..Default::default()
        }
    }

    #[test]
    fn rows_follow_the_status_switch() {
        let texts = texts();
        let lobby = WolChannel {
            name: "Lob_41_9".into(),
            ..Default::default()
        };
        let other = WolChannel {
            name: "Custom".into(),
            ..Default::default()
        };
        let mut listed = game("Boris's_game");
        listed.flags = CHANNEL_KEYED;
        listed.game_flags = GAME_LIMIT_800;
        listed.tournament = 1;
        let games = [listed.clone()];
        let row = |status, channel| buddy_row("B", status, channel, &games, &texts);
        let cell = |row: WolBuddyRow| (row.status, row.color);
        assert_eq!(cell(row(Some(1), None)), ("find off".into(), GREY));
        assert_eq!(cell(row(Some(2), None)), ("offline".into(), GREY));
        assert_eq!(cell(row(Some(3), None)), ("online".into(), ONLINE_COLOR));
        assert_eq!(
            cell(row(Some(4), Some(&lobby))),
            ("L1 2".into(), CHAT_COLOR)
        );
        assert_eq!(
            cell(row(Some(4), Some(&other))),
            ("Custom".into(), CHAT_COLOR)
        );
        assert_eq!(cell(row(Some(4), None)), ("online".into(), ONLINE_COLOR));
        let gone = game("Gone's_game");
        assert_eq!(
            cell(row(Some(5), Some(&gone))),
            ("online".into(), ONLINE_COLOR)
        );
        let named = game("boris's_GAME");
        let in_game = row(Some(5), Some(&named));
        assert_eq!(
            (in_game.status.as_str(), in_game.color),
            ("Boris's_game 2/6", GAME_COLOR)
        );
        assert_eq!(
            in_game.game,
            Some(BuddyGameCells {
                kind: BuddyGameType::Tournament,
                keyed: true,
                limit_icon: Some("wolrslkob.pcx"),
                ping: 180,
            })
        );
        assert_eq!(cell(row(Some(6), None)), ("playing".into(), PLAYING_COLOR));
        assert_eq!(cell(row(Some(8), None)), ("qm".into(), QUICK_MATCH_COLOR));
        assert_eq!(cell(row(None, None)), ("unknown".into(), GREY));
    }

    #[test]
    fn join_and_page_enables_follow_status_and_admission() {
        let open = game("Open's_game");
        let mut full = game("Full's_game");
        full.users = 6;
        let games = [open.clone(), full.clone()];
        assert!(join_enabled("B", Some(4), None, &games, false));
        assert!(join_enabled("B", Some(5), Some(&open), &games, false));
        assert!(!join_enabled("B", Some(5), Some(&full), &games, false));
        assert!(!join_enabled("B", Some(5), Some(&game("x")), &games, false));
        assert!(!join_enabled("", Some(4), None, &games, false));
        assert!(!join_enabled("B", Some(3), None, &games, false));
        assert!(page_enabled("B", Some(3)) && page_enabled("B", Some(-1)));
        assert!(!page_enabled("B", Some(2)) && !page_enabled("B", None));
    }

    #[test]
    fn rows_update_in_place_and_reselect_by_name() {
        let texts = texts();
        let mut page = WolBuddyPage::new(false);
        assert!(!page.enabled(JOIN) && !page.enabled(REMOVE) && page.enabled(ADD));
        let row = |name: &str, status| buddy_row(name, status, None, &[], &texts);
        page.update_rows(vec![row("A", None), row("B", None)], "b");
        assert_eq!(page.list.selected, Some(1));
        page.update_rows(vec![row("B", Some(2)), row("C", Some(3))], "");
        let names: Vec<_> = page.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["A", "B", "C"]);
        assert_eq!(page.rows[1].status, "offline");
        assert_eq!(page.list.selected, None);
        assert_eq!(page.select(2), Some(WolBuddyAction::Select("C".into())));
        page.set_enables(true, false, true);
        assert_eq!(
            page.activate(REMOVE),
            Some(WolBuddyAction::Remove("C".into()))
        );
        page.delete_row("C");
        assert_eq!(page.rows.len(), 2);
        assert_eq!(page.double_click(false, true), Some(WolBuddyAction::Page));
        assert_eq!(page.activate(REFRESH), Some(WolBuddyAction::Refresh));
        assert_eq!(page.activate(REFRESH), None);
        assert_eq!(page.activate(BACK), Some(WolBuddyAction::End(BACK)));
    }

    #[test]
    fn add_buddy_enables_by_validity_and_always_closes() {
        let valid = |name: &str| name != "Tester" && !name.contains(',');
        let mut add = WolAddBuddy::new();
        assert_eq!(add.key(LanEditKey::Enter, &valid), None);
        add.insert_text("Tester", &valid);
        assert!(!add.add_enabled);
        add.key(LanEditKey::Backspace, &valid);
        assert!(add.add_enabled);
        add.insert_text("ABCDEFGH", &valid);
        assert_eq!(add.text(), "TesteABCD");
        assert_eq!(
            add.key(LanEditKey::Enter, &valid),
            Some(WolBuddyAction::Add("TesteABCD".into()))
        );
        assert_eq!(add.text(), "");
        assert_eq!(add.activate(BACK), Some(WolBuddyAction::CloseAdd));
    }
}
