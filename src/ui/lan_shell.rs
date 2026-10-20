//! LAN lobby BB: original5DDBD0, list projections5DB9B0/5DAFE0 and chat
//! NewEdit614B30 ->5EDD00 ->5DE360. Discovery and displayed rows are views;
//! admitted participants, setup, timers and service requests belong to the app.

use std::time::Instant;

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::{ListMetrics, ListScrollInteraction, ShellListGeometry};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::skirmish_shell::SavedSeedDescriptionEdit;

pub(crate) const CREATE: u16 = 0x6CC;
pub(crate) const JOIN: u16 = 0x6CD;
/// The dedicated Observe button is hidden in the original BB resource.
pub(crate) const OBSERVE: u16 = 0x6CE;
pub(crate) const BACK: u16 = 0x5C0;
pub(crate) const NAME: u16 = 0x541;
pub(crate) const CHAT: u16 = 0x540;
pub(crate) const GAMES: u16 = 0x549;
pub(crate) const PLAYERS: u16 = 0x53E;
pub(crate) const CHAT_LOG: u16 = 0x53F;
pub(crate) const OBSERVER_IMAGE: u16 = 0x75F;
pub(crate) const OBSERVER_TOGGLE: u16 = 0x760;

pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xBB),
    title_key: "GUI:Lobby",
    stacked: &[
        MenuPageButtonSpec {
            id: CREATE,
            dlu_top: 122,
            csf_key: "GUI:New",
            tooltip_key: "STT:LobbyButtonNew",
            result: None,
        },
        MenuPageButtonSpec {
            id: JOIN,
            dlu_top: 149,
            csf_key: "GUI:Join",
            tooltip_key: "STT:LobbyButtonJoin",
            result: None,
        },
    ],
    back: MenuPageButtonSpec {
        id: BACK,
        dlu_top: 346,
        csf_key: "GUI:MainMenu",
        tooltip_key: "STT:LobbyButtonBack",
        result: None,
    },
};

/// Executed6040B0 against BB's original resource control IDs.
pub(crate) fn tooltip_key(control: u16) -> Option<&'static str> {
    Some(match control {
        CREATE => "STT:LobbyButtonNew",
        JOIN => "STT:LobbyButtonJoin",
        OBSERVE => "STT:LobbyButtonObserve",
        BACK => "STT:LobbyButtonBack",
        CHAT => "STT:LobbyEditInput",
        NAME => "STT:LobbyEditYourName",
        GAMES => "STT:LobbyListGames",
        PLAYERS => "STT:LobbyListUsers",
        CHAT_LOG => "STT:LobbyEditOutput",
        OBSERVER_TOGGLE => "STT:ObserverToggle",
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanGameRow {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) open: bool,
}

impl LanGameRow {
    ///5DB9B0 passes the name to this localized format; closed rows stay visible.
    pub(crate) fn label_key(&self) -> &'static str {
        if self.open {
            "TXT_THATGUYS_GAME"
        } else {
            "TXT_THATGUYS_GAME_BRACKET"
        }
    }
}

/// Display projection only. The app supplies its own row first in the lobby,
/// or the selected game's ordered roster (native5DAFE0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanPlayerRow {
    pub(crate) name: String,
    pub(crate) color: Option<u32>,
    pub(crate) side_icon: Option<String>,
    pub(crate) host: bool,
    pub(crate) accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanChatLine {
    pub(crate) text: Vec<u16>,
    pub(crate) color: u32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LanListState {
    pub(crate) selected: Option<usize>,
    pub(crate) top_index: usize,
    pub(crate) hot_row: Option<usize>,
    pub(crate) scroll: ListScrollInteraction,
    /// Passed to the shared list::is_double_click owner by the app adapter.
    pub(crate) last_press: Option<(Instant, i32, i32)>,
}

impl LanListState {
    pub(crate) fn geometry(
        &self,
        rect: RectPx,
        count: usize,
        scale: ShellScale,
    ) -> ShellListGeometry {
        ShellListGeometry::new(rect, count, self.top_index, ListMetrics::scaled(scale))
    }

    fn reconcile(&mut self, count: usize) {
        self.selected = self.selected.filter(|index| *index < count);
        self.hot_row = self.hot_row.filter(|index| *index < count);
        self.top_index = self.top_index.min(count.saturating_sub(1));
    }

    /// Reapply the shared scrollbar's range after discovery or a resized
    /// viewport, before painting/hit-testing its retained top index.
    pub(crate) fn reconcile_geometry(&mut self, rect: RectPx, count: usize, scale: ShellScale) {
        self.reconcile(count);
        self.top_index = self
            .top_index
            .min(self.geometry(rect, count, scale).max_top);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LanLobbyAction {
    Create {
        observe: bool,
    },
    Join {
        game_id: Option<u64>,
        observe: bool,
    },
    Back,
    NameChanged(String),
    SendChat(Vec<u16>),
    ObserverChanged(bool),
    /// Every selection notification refreshes displayed users; only a change
    /// broadcasts selection in5DDCFD..5DDD28.
    SelectGame {
        game_id: Option<u64>,
        changed: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LanEditKey {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Tab,
    Enter,
}

/// One chat editor/transcript owner for LAN BB/BC/BD. Parent dialogs own
/// focus/admission; the shared NewEdit owns editing and this owner implements
/// original5EDD00 ->5DE360 send and5E2820 ->5E29A0 display behavior.
#[derive(Debug, Clone, Default)]
pub(crate) struct LanChatState {
    pub(crate) edit: SavedSeedDescriptionEdit,
    pub(crate) lines: Vec<LanChatLine>,
    pub(crate) list: LanListState,
}

impl LanChatState {
    pub(crate) fn insert_text(&mut self, text: &str) {
        for unit in text.encode_utf16() {
            self.insert_unit(unit);
        }
    }

    pub(crate) fn insert_unit(&mut self, unit: u16) {
        if self.edit.insert_unit_filtered(unit, 0, false, &[]) {
            apply_chat_length_limit(&mut self.edit);
        }
    }

    pub(crate) fn key(&mut self, key: LanEditKey) -> Option<Vec<u16>> {
        if key == LanEditKey::Enter {
            //614DEB appends before the parent's501 handler5EDD00/5DE360.
            self.edit.append_enter_line_end();
            apply_chat_length_limit(&mut self.edit);
            let sent = self.edit.wide_text(111);
            self.edit.set_units_limited([], 0);
            return (sent.len() > 2).then_some(sent);
        }
        let before = self.edit.units.len();
        match key {
            LanEditKey::Backspace => self.edit.backspace(),
            LanEditKey::Delete => self.edit.delete(),
            LanEditKey::Left => self.edit.left(),
            LanEditKey::Right => self.edit.right(),
            LanEditKey::Home => self.edit.home(),
            LanEditKey::End => self.edit.end(),
            LanEditKey::Tab | LanEditKey::Enter => {}
        }
        if self.edit.units.len() != before {
            apply_chat_length_limit(&mut self.edit);
        }
        None
    }

    pub(crate) fn append_message(
        &mut self,
        message: LanChatLine,
        width: i32,
        visible_rows: usize,
        measure: impl Fn(&[u16]) -> i32,
    ) {
        append_list_message(
            &mut self.lines,
            &mut self.list,
            message,
            width,
            visible_rows,
            measure,
        );
    }
}

/// The shell list line adder `0x005E2820` -> `0x005E29A0`: wrap the text to
/// the list's width and append each row. An empty text adds nothing.
pub(crate) fn append_list_message(
    lines: &mut Vec<LanChatLine>,
    list: &mut LanListState,
    message: LanChatLine,
    width: i32,
    visible_rows: usize,
    measure: impl Fn(&[u16]) -> i32,
) {
    for text in split_chat_lines(&message.text, width, measure) {
        append_row(
            lines,
            list,
            LanChatLine {
                text,
                color: message.color,
            },
            visible_rows,
        );
    }
}

///5E29A0: keep at most501rows (delete only when oldcount>500),
/// and follow appended text only when the old view was at the bottom.
fn append_row(
    lines: &mut Vec<LanChatLine>,
    list: &mut LanListState,
    line: LanChatLine,
    visible_rows: usize,
) {
    let follow = lines.len().saturating_sub(list.top_index) <= visible_rows;
    if lines.len() > 500 {
        lines.remove(0);
        list.top_index = list.top_index.saturating_sub(1);
    }
    lines.push(line);
    if follow {
        list.top_index = lines.len().saturating_sub(visible_rows);
    }
}

///5E2820 first tries through the next LF, then walks backward across graphic
/// characters, measuring each candidate with the shared GAME.FNT owner.
/// Unlike paragraph wrapping, only ASCII spaces are skipped after emission.
fn split_chat_lines(units: &[u16], width: i32, measure: impl Fn(&[u16]) -> i32) -> Vec<Vec<u16>> {
    let units = &units[..units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len())];
    let mut remaining = units;
    let mut count = remaining.len();
    let mut result = Vec::new();
    // BB list53F has ordinary scrollbar mode (message4A0 returns0), reserve18.
    let available = width - 18;
    while count > 0 {
        if let Some(newline) = remaining.iter().position(|unit| *unit == 10) {
            count = count.min(newline + 1);
        }
        let measured = measure(&remaining[..count]);
        if measured < available - 4 {
            let row = &remaining[..count];
            let end = row
                .iter()
                .position(|unit| *unit == 13)
                .or_else(|| row.iter().position(|unit| *unit == 10))
                .unwrap_or(row.len());
            result.push(row[..end].to_vec());
            remaining = &remaining[count..];
            while remaining.first() == Some(&32) {
                remaining = &remaining[1..];
            }
            count = remaining.len();
            continue;
        }
        let step = if measured / 2 > available {
            count = count.saturating_sub(10);
            10
        } else {
            1
        };
        let mut boundary = count.saturating_sub(1);
        while boundary > 0 && chat_graphic_unit(remaining[boundary]) {
            boundary -= 1;
        }
        count = if boundary > 0 {
            boundary
        } else {
            count.saturating_sub(step)
        };
    }
    result
}

/// CRT7CB8E6 ->7D3920, mask117. Original initial C-locale table87C59A
/// classifies only21..7E as graphical below100; Latin1 is not Unicode-classed.
/// AboveFF native asks GetStringTypeW. The portable text classification below
/// covers letters/symbols and rejects whitespace/control and surrogate units;
/// Windows-version-specific unassigned/format classes remain a text residual.
fn chat_graphic_unit(unit: u16) -> bool {
    if unit < 0x100 {
        return (0x21..=0x7E).contains(&unit);
    }
    unit != 0xFFFF
        && char::from_u32(u32::from(unit)).is_some_and(|c| !c.is_whitespace() && !c.is_control())
}

#[derive(Debug, Clone)]
pub(crate) struct LanLobbyState {
    pub(crate) name: SavedSeedDescriptionEdit,
    pub(crate) chat: LanChatState,
    /// Excludes the always-present TXT_LOBBY sentinel at displayed index0.
    pub(crate) games: Vec<LanGameRow>,
    pub(crate) players: Vec<LanPlayerRow>,
    pub(crate) game_list: LanListState,
    pub(crate) player_list: LanListState,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) focused_control: u16,
    pub(crate) observer: bool,
    pub(crate) enabled: bool,
    /// Projected native membership gate<=0; no membership authority lives here.
    pub(crate) selection_enabled: bool,
    pending_command: bool,
    /// Last emitted edit value, not a second live Session name owner.
    notified_name: String,
    focus_started_at: Instant,
}

impl LanLobbyState {
    pub(crate) fn new(name: &str, observer: bool) -> Self {
        let mut name_edit = SavedSeedDescriptionEdit::default();
        name_edit.set_units_limited(name.encode_utf16(), 16);
        let notified_name = name_edit.narrow_text(20);
        Self {
            name: name_edit,
            chat: LanChatState {
                edit: SavedSeedDescriptionEdit {
                    focused: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            games: Vec::new(),
            players: Vec::new(),
            game_list: LanListState {
                selected: Some(0),
                ..Default::default()
            },
            player_list: LanListState::default(),
            buttons: ShellButtonInteraction::default(),
            focused_control: CHAT,
            observer,
            enabled: true,
            selection_enabled: true,
            pending_command: false,
            notified_name,
            focus_started_at: Instant::now(),
        }
    }

    pub(crate) fn game_count(&self) -> usize {
        self.games.len() + 1
    }

    pub(crate) fn selected_game(&self) -> Option<&LanGameRow> {
        self.game_list
            .selected
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| self.games.get(index))
    }

    pub(crate) fn replace_games(&mut self, games: Vec<LanGameRow>) -> Option<LanLobbyAction> {
        let before = self.game_list.selected.unwrap_or(0);
        self.games = games;
        //5DB9B0 clamps the numeric selection, preserving top index; it does
        // not reselect by name when discovery rows are reordered.
        let selected = before.min(self.games.len());
        self.game_list.reconcile(self.game_count());
        self.game_list.selected = Some(selected);
        (selected != before).then(|| LanLobbyAction::SelectGame {
            game_id: self.selected_game().map(|row| row.id),
            changed: true,
        })
    }

    pub(crate) fn replace_players(&mut self, players: Vec<LanPlayerRow>) {
        self.players = players;
        self.player_list.reconcile(self.players.len());
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled {
            self.pending_command = false;
        }
        if !enabled {
            self.cancel_pointer();
        }
    }

    pub(crate) fn cancel_pointer(&mut self) {
        self.buttons = ShellButtonInteraction::default();
        self.game_list.scroll.cancel();
        self.player_list.scroll.cancel();
        self.chat.list.scroll.cancel();
        self.game_list.last_press = None;
    }

    pub(crate) fn focus(&mut self, control: u16) {
        // Ordinary owner-draw buttons restore the preceding focus610CA0.
        if !matches!(control, NAME | CHAT) || !self.enabled {
            return;
        }
        if self.focused_control != control {
            self.focus_started_at = Instant::now();
        }
        self.focused_control = control;
        self.name.focused = control == NAME;
        self.chat.edit.focused = control == CHAT;
    }

    pub(crate) fn caret_visible(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.focus_started_at)
            .as_secs()
            % 2
            == 0
    }

    pub(crate) fn select_game(&mut self, index: usize) -> Option<LanLobbyAction> {
        if !self.enabled
            || self.pending_command
            || !self.selection_enabled
            || index >= self.game_count()
        {
            return None;
        }
        let changed = self.game_list.selected != Some(index);
        self.game_list.selected = Some(index);
        self.players.clear();
        self.player_list.reconcile(0);
        Some(LanLobbyAction::SelectGame {
            game_id: self.selected_game().map(|row| row.id),
            changed,
        })
    }

    pub(crate) fn activate(&mut self, control: u16) -> Option<LanLobbyAction> {
        if !self.enabled || self.pending_command {
            return None;
        }
        let action = match control {
            CREATE => LanLobbyAction::Create {
                observe: self.observer,
            },
            JOIN | OBSERVE => LanLobbyAction::Join {
                game_id: self.selected_game().map(|row| row.id),
                observe: control == OBSERVE || self.observer,
            },
            BACK => LanLobbyAction::Back,
            OBSERVER_IMAGE | OBSERVER_TOGGLE => {
                self.observer = !self.observer;
                return Some(LanLobbyAction::ObserverChanged(self.observer));
            }
            _ => return None,
        };
        self.pending_command = true;
        if matches!(control, JOIN | OBSERVE) {
            self.set_enabled(false);
        }
        Some(action)
    }

    fn name_changed(&mut self) -> Option<LanLobbyAction> {
        //5DDD90..5DDE42: collapse all ASCII spaces, but preserve every
        // leading/trailing space in a name that contains another character.
        if !self.name.units.is_empty() && self.name.units.iter().all(|unit| *unit == 32) {
            self.name.set_units_limited([], 16);
        }
        let value = self.name.narrow_text(20);
        if value == self.notified_name {
            return None;
        }
        self.notified_name = value.clone();
        Some(LanLobbyAction::NameChanged(value))
    }

    pub(crate) fn insert_text(&mut self, text: &str) -> Vec<LanLobbyAction> {
        let mut actions = Vec::new();
        if !self.enabled || self.pending_command {
            return actions;
        }
        for unit in text.encode_utf16() {
            match self.focused_control {
                NAME if self.name.insert_unit_filtered(unit, 16, true, &[44]) => {
                    if let Some(action) = self.name_changed() {
                        actions.push(action);
                    }
                }
                CHAT => self.chat.insert_unit(unit),
                _ => {}
            }
        }
        actions
    }

    pub(crate) fn edit_key(&mut self, key: LanEditKey) -> Option<LanLobbyAction> {
        if !self.enabled || self.pending_command {
            return None;
        }
        if key == LanEditKey::Tab {
            self.focus(if self.focused_control == CHAT {
                NAME
            } else {
                CHAT
            });
            return None;
        }
        if self.focused_control == CHAT {
            return self.chat.key(key).map(LanLobbyAction::SendChat);
        }
        if self.focused_control != NAME || key == LanEditKey::Enter {
            return None;
        }
        let before = self.name.units.len();
        match key {
            LanEditKey::Backspace => self.name.backspace(),
            LanEditKey::Delete => self.name.delete(),
            LanEditKey::Left => self.name.left(),
            LanEditKey::Right => self.name.right(),
            LanEditKey::Home => self.name.home(),
            LanEditKey::End => self.name.end(),
            LanEditKey::Tab | LanEditKey::Enter => unreachable!(),
        }
        if self.name.units.len() != before {
            self.name_changed()
        } else {
            None
        }
    }
}

/// One port of the shared5EDD00 chat-length prepass (BB/BC/BD and WOL).
/// IsLatin1 tests>FF, but the following scan tests>100. DEC ESI wraps when
/// that scan stops at zero, leaving no cap; keep this unsigned native trap.
/// The helper's trimmed return is discarded by these LAN dialog callers.
pub(crate) fn apply_chat_length_limit(edit: &mut SavedSeedDescriptionEdit) {
    let cap = if edit.units.iter().all(|unit| *unit <= 0xff) {
        120
    } else {
        let first_wide = edit
            .units
            .iter()
            .position(|unit| *unit > 0x100)
            .unwrap_or(edit.units.len()) as u32;
        first_wide.wrapping_sub(1).max(40) as usize
    };
    if edit.units.len() > cap {
        edit.set_units_limited(edit.units.clone(), cap);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LanLobbyLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) name: RectPx,
    pub(crate) chat: RectPx,
    pub(crate) games: RectPx,
    pub(crate) players: RectPx,
    pub(crate) chat_log: RectPx,
    pub(crate) observer_image: RectPx,
    pub(crate) observer_toggle: RectPx,
    pub(crate) labels: [(&'static str, RectPx); 3],
}

impl LanLobbyLayout {
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::RETAIL)
    }
    pub(crate) fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height))
    }

    fn build(width: i32, height: i32, scale: ShellScale) -> Self {
        let page = if scale.is_retail() {
            menu_page::compute_layout(&PAGE, width as u32, height as u32)
        } else {
            menu_page::compute_responsive_layout(&PAGE, width as u32, height as u32)
        };
        let rect = |x, y, w, h| scale.scale_rect(RectPx::new(x, y, w, h));
        // Executed original BB child setup60AAB0/include60C540/relayout60C4A0
        // ->60C0C0; corpus lan_lobby.json. Ordinary controls retain template
        // positions even at1024x768; right-panel controls use shared anchors.
        // NewEdit497 at614CCF..614CF3 adds1 to x/y and removes2 from w/h.
        Self {
            page,
            name: rect(136, 17, 134, 19),
            chat: rect(12, 524, 609, 19),
            games: rect(323, 73, 300, 292),
            players: rect(11, 73, 301, 292),
            chat_log: rect(11, 369, 612, 154),
            observer_image: rect(594, 13, 28, 21),
            observer_toggle: rect(576, 16, 16, 17),
            labels: [
                ("GUI:Games", rect(326, 49, 223, 17)),
                ("GUI:Players", rect(15, 49, 171, 17)),
                ("GUI:YourName", rect(15, 16, 112, 17)),
            ],
        }
    }

    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        for button in &self.page.buttons {
            if button.rect.contains(x, y) {
                return Some(button.id);
            }
        }
        for (id, rect) in [
            (NAME, self.name),
            (CHAT, self.chat),
            (GAMES, self.games),
            (PLAYERS, self.players),
            (CHAT_LOG, self.chat_log),
            (OBSERVER_IMAGE, self.observer_image),
            (OBSERVER_TOGGLE, self.observer_toggle),
        ] {
            if rect.contains(x, y) {
                return Some(id);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native() -> serde_json::Value {
        crate::test_fixture::json("tools/storage_oracle/lan_lobby.json").clone()
    }

    #[test]
    fn chat_display_strips_line_ends_and_preserves_manual_newlines() {
        let units: Vec<u16> = "[Tester] hello\r\n  second\nthird".encode_utf16().collect();
        let rows = split_chat_lines(&units, 612, |units| units.len() as i32 * 8);
        let rows: Vec<_> = rows
            .iter()
            .map(|row| String::from_utf16_lossy(row))
            .collect();
        assert_eq!(rows, ["[Tester] hello", "second", "third"]);
    }

    #[test]
    fn chat_retains_501_rows_and_does_not_jump_a_scrolled_view() {
        let mut lobby = LanLobbyState::new("Tester", false);
        for value in 0..501 {
            let chat = &mut lobby.chat;
            append_row(
                &mut chat.lines,
                &mut chat.list,
                LanChatLine {
                    text: vec![value],
                    color: u32::MAX,
                },
                8,
            );
        }
        assert_eq!(lobby.chat.lines.len(), 501);
        assert_eq!(lobby.chat.list.top_index, 493);
        lobby.chat.list.top_index = 10;
        let chat = &mut lobby.chat;
        append_row(
            &mut chat.lines,
            &mut chat.list,
            LanChatLine {
                text: vec![501],
                color: u32::MAX,
            },
            8,
        );
        assert_eq!(lobby.chat.lines.len(), 501);
        assert_eq!(lobby.chat.lines.first().unwrap().text, [1]);
        assert_eq!(lobby.chat.list.top_index, 9);
    }

    #[test]
    fn chat_enter_matches_original_newedit_parent_getter_and_clear_chain() {
        for row in native()["chat"].as_array().unwrap() {
            let units: Vec<u16> = serde_json::from_value(row["input"].clone()).unwrap();
            let mut lobby = LanLobbyState::new("Tester", false);
            lobby.chat.edit.set_units_limited(units, 0);
            let sent = match lobby.edit_key(LanEditKey::Enter) {
                Some(LanLobbyAction::SendChat(units)) => vec![units],
                None => Vec::new(),
                other => panic!("{other:?}"),
            };
            assert_eq!(
                serde_json::to_value(sent).unwrap(),
                row["sent"],
                "{}",
                row["name"]
            );
            assert_eq!(
                serde_json::to_value(lobby.chat.edit.units).unwrap(),
                row["edit_after"]
            );
        }
    }

    #[test]
    fn layout_and_help_follow_original_bb_execution() {
        let layout = LanLobbyLayout::new(800, 600);
        for row in native()["layout_800"].as_array().unwrap() {
            let id = row["id"].as_u64().unwrap() as u32;
            let expected: Vec<i32> = serde_json::from_value(row["rect"].clone()).unwrap();
            let rect = match id as u16 {
                NAME => Some(RectPx::new(
                    layout.name.x - 1,
                    layout.name.y - 1,
                    layout.name.w + 2,
                    layout.name.h + 2,
                )),
                CHAT => Some(RectPx::new(
                    layout.chat.x - 1,
                    layout.chat.y - 1,
                    layout.chat.w + 2,
                    layout.chat.h + 2,
                )),
                GAMES => Some(layout.games),
                PLAYERS => Some(layout.players),
                CHAT_LOG => Some(layout.chat_log),
                OBSERVER_IMAGE => Some(layout.observer_image),
                OBSERVER_TOGGLE => Some(layout.observer_toggle),
                _ => layout
                    .page
                    .buttons
                    .iter()
                    .find(|button| u32::from(button.id) == id)
                    .map(|button| button.rect),
            };
            if let Some(rect) = rect {
                assert_eq!(
                    [rect.x, rect.y, rect.w, rect.h].as_slice(),
                    expected,
                    "{id:#x}"
                );
            }
        }
        for row in native()["help"].as_array().unwrap() {
            assert_eq!(
                tooltip_key(row["control"].as_u64().unwrap() as u16),
                row["key"].as_str()
            );
        }
    }

    #[test]
    fn name_input_uses_byte_filter_comma_rejection_and_literal_space_rule() {
        let mut lobby = LanLobbyState::new("", false);
        assert_eq!(lobby.focused_control, CHAT);
        lobby.focus(NAME);
        lobby.insert_text("  A,éĀB");
        assert_eq!(lobby.name.display_text(), "AéB");
        lobby.insert_text("12345678901234567890");
        assert_eq!(lobby.name.units.len(), 16);
        assert_eq!(lobby.edit_key(LanEditKey::Enter), None);
        let retained = LanLobbyState::new(" A ", false);
        assert_eq!(retained.name.display_text(), " A ");
    }

    #[test]
    fn discovery_clamps_selection_and_join_remains_an_intent() {
        let mut lobby = LanLobbyState::new("Tester", false);
        let rows = (0..3)
            .map(|id| LanGameRow {
                id,
                name: format!("Player{id}"),
                open: true,
            })
            .collect();
        lobby.replace_games(rows);
        assert_eq!(
            lobby.select_game(3),
            Some(LanLobbyAction::SelectGame {
                game_id: Some(2),
                changed: true
            })
        );
        assert_eq!(
            lobby.replace_games(Vec::new()),
            Some(LanLobbyAction::SelectGame {
                game_id: None,
                changed: true
            })
        );
        assert_eq!(
            lobby.activate(JOIN),
            Some(LanLobbyAction::Join {
                game_id: None,
                observe: false
            })
        );
        assert!(!lobby.enabled);
        assert_eq!(lobby.activate(CREATE), None);
        lobby.set_enabled(true);
        assert_eq!(
            lobby.activate(OBSERVER_IMAGE),
            Some(LanLobbyAction::ObserverChanged(true))
        );
        assert_eq!(
            lobby.activate(CREATE),
            Some(LanLobbyAction::Create { observe: true })
        );
    }
}
