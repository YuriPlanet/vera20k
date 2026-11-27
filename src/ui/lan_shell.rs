//! LAN lobby BB: original5DDBD0, list projections5DB9B0/5DAFE0 and chat
//! NewEdit614B30 ->5EDD00 ->5DE360. Discovery and displayed rows are views;
//! admitted participants, setup, timers and service requests belong to the app.

use std::collections::VecDeque;

use vera20k_gui::{Gui, InputEvent, Key, NodeId, NodeKind, NodeSpec, Prop, Value};

use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, ShellAlign, ShellScale};
use crate::ui::shell::layout::RectTable;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};

pub(crate) const CREATE: u16 = 0x6CC;
pub(crate) const JOIN: u16 = 0x6CD;
/// The dedicated Observe button is hidden in the original BB resource.
pub(crate) const OBSERVE: u16 = 0x6CE;
pub(crate) const BACK: u16 = 0x5C0;
pub(crate) const NAME: u16 = 0x541;
/// The name edit's limit: 16 units, each a code-page byte in gamemd.
pub(crate) const NAME_LIMIT: usize = 16;
pub(crate) const CHAT: u16 = 0x540;
pub(crate) const GAMES: u16 = 0x549;
pub(crate) const PLAYERS: u16 = 0x53E;
pub(crate) const CHAT_LOG: u16 = 0x53F;
pub(crate) const OBSERVER_IMAGE: u16 = 0x75F;
pub(crate) const OBSERVER_TOGGLE: u16 = 0x760;
/// The heading `0x694` and the status line `0x695`: kind-1 statics whose
/// text and reveal the page gives while they show.
pub(crate) const TITLE: u16 = 0x694;
pub(crate) const STATUS: u16 = 0x695;

pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogResourceId(0xBB),
    title_key: "GUI:Lobby",
    stacked: &[
        MenuPageButtonSpec::new(CREATE, 122, "GUI:New", "STT:LobbyButtonNew"),
        MenuPageButtonSpec::new(JOIN, 149, "GUI:Join", "STT:LobbyButtonJoin"),
    ],
    back: MenuPageButtonSpec::new(BACK, 346, "GUI:MainMenu", "STT:LobbyButtonBack"),
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

/// BB's widgets, each with its status help: the right-panel buttons, the
/// two NewEdits (the name's limits as [`LanLobbyState::name_edited`] says),
/// the lists (their rows painted by BB, hit inside their frame) and the
/// observer box, which
/// toggles on the press (`0x5DDBD0`'s BN_CLICKED off the icon), with the
/// observer image `0x75F` beside it (a box the page paints as its image,
/// toggled anywhere on it).
pub(crate) fn spec() -> NodeSpec {
    let node = |kind: NodeKind, id: u16| {
        NodeSpec::new(kind)
            .id(i32::from(id))
            .tooltip(tooltip_key(id).unwrap_or_default())
    };
    // Each row's text 2 px in from both ends; the chat log shows no
    // selection.
    let list = |id: u16| {
        node(NodeKind::List, id)
            .with(Prop::RowCount, Value::Int(0))
            .with(Prop::TextInset, Value::Rect(RectPx::new(2, 0, 2, 0)))
            .with(Prop::NoSelectionFill, Value::Bool(id == CHAT_LOG))
    };
    let text = |id: u16, align: ShellAlign| {
        NodeSpec::new(NodeKind::Text)
            .id(i32::from(id))
            .with(Prop::Align, Value::Int(align.0.into()))
    };
    // The captions, top-left.
    let labels = LABELS.map(|(id, key)| text(id, ShellAlign::NONE).label(key));
    NodeSpec::new(NodeKind::Dialog)
        .children(labels)
        .children(crate::ui::shell::page::menu_buttons(&PAGE))
        .child(
            node(NodeKind::Edit, NAME)
                .with(Prop::ByteOnly, Value::Bool(true))
                .with(Prop::MaxBytes, Value::Int(NAME_LIMIT as i32))
                .with(Prop::Rejected, Value::Text(",".into())),
        )
        .child(node(NodeKind::Edit, CHAT))
        .children([GAMES, PLAYERS, CHAT_LOG].map(list))
        .children([OBSERVER_TOGGLE, OBSERVER_IMAGE].map(|id| node(NodeKind::Checkbox, id)))
        .child(text(TITLE, ShellAlign::PANEL_TITLE))
        .child(text(STATUS, ShellAlign::NONE))
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

#[derive(Debug, Clone, Copy)]
pub(crate) enum PlayerNameMatch {
    Exact,
    IgnoreAsciiCase,
}

/// RestorePlayerList `0x005E6230` shares name matching across WOL and LAN:
/// its multi-select arm compares `WideWstring` exactly through `wcscmp`
/// (`0x007B67C0` -> `0x007CA5D3`), while LAN's single-select `0x4CA` lookup
/// uses `_wcsicmp`. Keep that policy explicit at each lobby caller.
pub(crate) fn saved_player_name_matches(
    saved_name: &str,
    current_name: &str,
    matching: PlayerNameMatch,
) -> bool {
    match matching {
        PlayerNameMatch::Exact => saved_name == current_name,
        PlayerNameMatch::IgnoreAsciiCase => saved_name.eq_ignore_ascii_case(current_name),
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
    /// Every selection notification refreshes displayed users; `change`
    /// decides what the service broadcasts.
    SelectGame {
        game_id: Option<u64>,
        change: LanSelectionChange,
    },
}

/// How a game selection came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LanSelectionChange {
    /// A click on the row already selected.
    Unchanged,
    /// A click on another row.
    Changed,
    /// Discovery dropped rows and clamped the selection to the last one.
    Clamped,
}

/// The gui inputs a key press or typed text makes in a focused NewEdit
/// (`0x00614B30`), one per WM_CHAR: Escape (the edit consumes it) and Tab
/// (the dialog's) make none.
fn edit_inputs(code: Option<winit::keyboard::KeyCode>, text: Option<&str>) -> Vec<InputEvent> {
    let inputs = crate::ui::shell::controller::gui_inputs(code, false, text);
    inputs
        .into_iter()
        .filter(|e| !matches!(e, InputEvent::Key(Key::Escape | Key::Tab | Key::BackTab)))
        .collect()
}

/// The chat edit `0x540`'s EN_CHANGE: `0x005EDD00`'s length cap
/// ([`apply_chat_length_limit`]) on its text. The hosts run it after every
/// input the edit takes.
pub(crate) fn cap_chat_edit(gui: &mut Gui, node: NodeId) {
    let mut edit = gui.edit_state(node).unwrap_or_default();
    let before = edit.units.len();
    apply_chat_length_limit(&mut edit);
    if edit.units.len() != before {
        let _ = gui.set_edit_state(node, &edit);
    }
}

/// Enter in the chat edit `0x540`: `0x614DEB` appends CRLF before the
/// parent's `0x501` handler `0x005EDD00` caps the text, reads at most 110
/// units and clears the edit. The line read, unless it holds no more than
/// the line end.
pub(crate) fn submit_chat_edit(gui: &mut Gui, node: NodeId) -> Option<Vec<u16>> {
    let mut edit = gui.edit_state(node).unwrap_or_default();
    edit.append_enter_line_end();
    apply_chat_length_limit(&mut edit);
    let sent = edit.wide_text(111);
    let _ = gui.set_edit_state(node, &Default::default());
    (sent.len() > 2).then_some(sent)
}

/// A key or typed text for `dialog`'s chat edit `0x540` while it holds the
/// focus (BC/BD's, BC6's): it takes every key but Escape and Tab, capping
/// its text after each; the line Enter sends.
pub(crate) fn chat_key(
    controller: &mut DialogController,
    dialog: DialogResourceId,
    code: Option<winit::keyboard::KeyCode>,
    text: Option<&str>,
) -> Option<Vec<u16>> {
    let node = controller.control(dialog, CHAT)?;
    if controller.focused_control(dialog) != Some(CHAT) {
        return None;
    }
    let mut sent = None;
    for input in edit_inputs(code, text) {
        controller.on_input(input);
        let submitted = controller.drain_events().into_iter().any(
            |event| matches!(event, vera20k_gui::UiEvent::Submitted { node: n, .. } if n == node),
        );
        if submitted {
            sent = submit_chat_edit(controller.gui_mut(), node);
        } else {
            cap_chat_edit(controller.gui_mut(), node);
        }
    }
    sent
}

/// One chat transcript owner for LAN BB/BC/BD, the WOL lobby and BC6:
/// `0x005E2820` -> `0x005E29A0`'s rows. The log's list widget owns the
/// scroll ([`Gui::log_rows`]); the edit is a gui `Edit`
/// ([`edit_inputs`], [`cap_chat_edit`], [`submit_chat_edit`]).
#[derive(Debug, Clone, Default)]
pub(crate) struct LanChatState {
    pub(crate) lines: Vec<LanChatLine>,
    /// The messages `lines` were wrapped from, newest last, and the width
    /// they were wrapped to: VERA's room re-wraps them when its chat column
    /// changes width with the window ([`Self::rewrap`]).
    messages: VecDeque<LanChatLine>,
    width: i32,
}

impl LanChatState {
    /// Adds `message`, wrapped to `width`; `log` hears each change of the
    /// rows (their count, and how many left the front) for the log's list
    /// ([`Gui::log_rows`]).
    pub(crate) fn append_message(
        &mut self,
        message: LanChatLine,
        width: i32,
        measure: impl Fn(&[u16]) -> i32,
        mut log: impl FnMut(usize, usize),
    ) {
        if self.rewrap(width, &measure) {
            log(self.lines.len(), 0);
        }
        // Each message adds a row at least, so these cover the rows kept.
        if self.messages.len() >= CHAT_ROWS {
            self.messages.pop_front();
        }
        self.messages.push_back(message.clone());
        let dropped = append_wrapped(&mut self.lines, message, width, measure);
        log(self.lines.len(), dropped);
    }

    /// Wrap the messages again when the log is now `width` wide (VERA's own:
    /// gamemd's logs never change width); whether it did. The log's list
    /// keeps a view at the bottom there, a scrolled one at its top row's
    /// index.
    pub(crate) fn rewrap(&mut self, width: i32, measure: impl Fn(&[u16]) -> i32) -> bool {
        if width == self.width {
            return false;
        }
        self.width = width;
        self.lines = self
            .messages
            .iter()
            .flat_map(|message| {
                split_chat_lines(&message.text, width, &measure)
                    .into_iter()
                    .map(|text| LanChatLine {
                        text,
                        color: message.color,
                    })
            })
            .collect();
        let dropped = self.lines.len().saturating_sub(CHAT_ROWS);
        self.lines.drain(..dropped);
        true
    }
}

/// `list`'s rows changed as `log` reports them (count, dropped from the
/// front): the transcript list follows them ([`Gui::log_rows`]).
pub(crate) fn log_to(gui: &mut Gui, list: Option<NodeId>) -> impl FnMut(usize, usize) + '_ {
    move |rows, dropped| {
        if let Some(list) = list {
            let _ = gui.log_rows(list, rows, dropped);
        }
    }
}

/// A chat edit and its log while their dialog is torn down (VERA's room
/// hidden while its player visits the lobby): the draft, and the log's view
/// as its list kept it, still following the lines that arrive meanwhile.
#[derive(Debug, Clone, Default)]
pub(crate) struct ParkedChat {
    pub(crate) edit: vera20k_gui::shell::new_edit::NewEdit,
    pub(crate) top: usize,
    pub(crate) rows: usize,
}

impl ParkedChat {
    /// The log's rows changed while parked, `visible` of them showing
    /// ([`vera20k_gui::shell::list::log_top`]).
    pub(crate) fn log(&mut self, rows: usize, dropped: usize, visible: usize) {
        let top =
            vera20k_gui::shell::list::log_top(self.rows, Some(self.top), visible, rows, dropped);
        (self.top, self.rows) = (top, rows);
    }
}

/// The width a chat log's rows wrap to ([`LanChatState::append_message`]):
/// the log's width in the shell's 800x600 pixels, counted in its text's own
/// pixels, so smaller text fits more of it.
pub(crate) fn chat_wrap_width(rect: RectPx, scale: ShellScale, text: f32) -> i32 {
    (scale.base_rect(rect).w as f32 / text) as i32
}

/// `0x005E29A0` keeps at most 501 rows.
const CHAT_ROWS: usize = 501;

/// The shell list line adder `0x005E2820` -> `0x005E29A0`: wrap the text to
/// the list's width and append each row, keeping at most 501 (delete only
/// when the old count is over 500). An empty text adds nothing. The rows
/// dropped from the front, for the list's view
/// ([`vera20k_gui::shell::list::log_top`]).
pub(crate) fn append_wrapped(
    lines: &mut Vec<LanChatLine>,
    message: LanChatLine,
    width: i32,
    measure: impl Fn(&[u16]) -> i32,
) -> usize {
    let mut dropped = 0;
    for text in split_chat_lines(&message.text, width, measure) {
        if lines.len() >= CHAT_ROWS {
            lines.remove(0);
            dropped += 1;
        }
        lines.push(LanChatLine {
            text,
            color: message.color,
        });
    }
    dropped
}

/// [`append_wrapped`] into a list whose view the host keeps itself at
/// `top`, `visible` rows showing (the connection dialog's `0x651`).
pub(crate) fn append_list_message(
    lines: &mut Vec<LanChatLine>,
    top: &mut usize,
    message: LanChatLine,
    width: i32,
    visible: usize,
    measure: impl Fn(&[u16]) -> i32,
) {
    let old = lines.len();
    let dropped = append_wrapped(lines, message, width, measure);
    *top = vera20k_gui::shell::list::log_top(old, Some(*top), visible, lines.len(), dropped);
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

/// BB's lobby: its discovery rows, transcript and admission gates. The
/// edits (their text, caret and focus), the lists' scroll, the buttons and
/// the observer box are widgets in the shell tree ([`spec`]); the
/// selections are the lobby's (the join target, the roster's by-name rule),
/// which the tree shows.
#[derive(Debug, Clone)]
pub(crate) struct LanLobbyState {
    pub(crate) chat: LanChatState,
    /// Excludes the always-present TXT_LOBBY sentinel at displayed index0.
    pub(crate) games: Vec<LanGameRow>,
    pub(crate) players: Vec<LanPlayerRow>,
    /// The selected games row; `0x549` always has one (index 0 the Lobby).
    pub(crate) selected_game: usize,
    pub(crate) selected_player: Option<usize>,
    pub(crate) enabled: bool,
    /// Projected native membership gate<=0; no membership authority lives here.
    pub(crate) selection_enabled: bool,
    pending_command: bool,
    /// Last emitted edit value, not a second live Session name owner; the
    /// name edit starts with it.
    pub(crate) notified_name: String,
}

impl LanLobbyState {
    pub(crate) fn new(name: &str) -> Self {
        let units: Vec<u16> = name.encode_utf16().take(NAME_LIMIT).collect();
        Self {
            chat: LanChatState::default(),
            games: Vec::new(),
            players: Vec::new(),
            selected_game: 0,
            selected_player: None,
            enabled: true,
            selection_enabled: true,
            pending_command: false,
            notified_name: String::from_utf16_lossy(&units),
        }
    }

    pub(crate) fn game_count(&self) -> usize {
        self.games.len() + 1
    }

    pub(crate) fn selected_game(&self) -> Option<&LanGameRow> {
        self.games.get(self.selected_game.checked_sub(1)?)
    }

    pub(crate) fn replace_games(&mut self, games: Vec<LanGameRow>) -> Option<LanLobbyAction> {
        let before = self.selected_game;
        self.games = games;
        //5DB9B0 clamps the numeric selection, preserving top index; it does
        // not reselect by name when discovery rows are reordered.
        self.selected_game = before.min(self.games.len());
        (self.selected_game != before).then(|| LanLobbyAction::SelectGame {
            game_id: self.selected_game().map(|row| row.id),
            change: LanSelectionChange::Clamped,
        })
    }

    /// The roster rebuild `0x005DAFE0`. Before `LB_RESETCONTENT` it saves the
    /// selected row's name (`0x005E5F10`); after re-adding the rows it selects
    /// the first row with that name, ignoring case (`0x005E6230`: list message
    /// `0x4CA`, `_wcsicmp`), or none.
    pub(crate) fn replace_players(&mut self, players: Vec<LanPlayerRow>) {
        let selected = self
            .selected_player
            .and_then(|index| self.players.get(index))
            .map(|row| row.name.clone());
        self.players = players;
        self.selected_player = selected.and_then(|name| {
            self.players.iter().position(|row| {
                saved_player_name_matches(&name, &row.name, PlayerNameMatch::IgnoreAsciiCase)
            })
        });
    }

    /// A press on a player row (`0x53E`, style `0x50000151`: single-select,
    /// no `LBS_NOSEL`). `OwnerDraw_ListBox` selects it (`LB_SETCURSEL`,
    /// `0x0061AA3E..0x0061AA69`); the caller plays its GenericClick.
    pub(crate) fn select_player(&mut self, index: usize) -> bool {
        if index >= self.players.len() {
            return false;
        }
        self.selected_player = Some(index);
        true
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled {
            self.pending_command = false;
        }
    }

    /// The edits take keys and presses: the page is on and no command is
    /// pending.
    pub(crate) fn takes_keys(&self) -> bool {
        self.enabled && !self.pending_command
    }

    pub(crate) fn select_game(&mut self, index: usize) -> Option<LanLobbyAction> {
        if !self.enabled
            || self.pending_command
            || !self.selection_enabled
            || index >= self.game_count()
        {
            return None;
        }
        let change = if self.selected_game == index {
            LanSelectionChange::Unchanged
        } else {
            LanSelectionChange::Changed
        };
        self.selected_game = index;
        // The roster follows through `replace_players` (the 0x549 LBN_SELCHANGE
        // handler's `0x005DAFE0`), which keeps the selected player by name.
        Some(LanLobbyAction::SelectGame {
            game_id: self.selected_game().map(|row| row.id),
            change,
        })
    }

    /// Control `control` activated, with the player's observer choice
    /// `observer` (the preference owner's).
    pub(crate) fn activate(&mut self, control: u16, observer: bool) -> Option<LanLobbyAction> {
        if !self.enabled || self.pending_command {
            return None;
        }
        let action = match control {
            CREATE => LanLobbyAction::Create { observe: observer },
            JOIN | OBSERVE => LanLobbyAction::Join {
                game_id: self.selected_game().map(|row| row.id),
                observe: control == OBSERVE || observer,
            },
            BACK => LanLobbyAction::Back,
            OBSERVER_IMAGE | OBSERVER_TOGGLE => {
                return Some(LanLobbyAction::ObserverChanged(!observer));
            }
            _ => return None,
        };
        self.pending_command = true;
        if matches!(control, JOIN | OBSERVE) {
            self.set_enabled(false);
        }
        Some(action)
    }

    /// The name edit's EN_CHANGE with its text `text` (each typed
    /// character's: the host types it in one at a time). `0x5DDD90..
    /// 0x5DDE42` collapses a name of ASCII spaces only, but keeps every
    /// leading/trailing space in one with another character; the text the
    /// edit then holds, if it changed, and the change to announce.
    ///
    /// BB's name edit takes 16 bytes: BB switches on its byte-only filter
    /// (`0x4D1` at `0x5DE0E3`), which drops units from 0x100 up
    /// (`0x61525E..0x61526A`). Typed keys reach gamemd, an ANSI program, as
    /// code-page bytes and get in, but input-method characters arrive widened
    /// (`0x6151F6..0x61520C`) and stay out. VERA gets keys as UTF-16 text
    /// and its edit applies the same filter to it (`ByteOnly`): `é` gets in,
    /// while an input method's Chinese or an emoji stays out instead of
    /// turning into `?`. A name set rather than typed keeps what it holds, as
    /// in gamemd. The edit admits a character while the name's code-page bytes
    /// fit in 16 (`MaxBytes`); commas stay out (`Rejected`). BB narrows its
    /// edit's units by their low byte (`0x5DDE07`), which gives back the
    /// code-page bytes its ANSI edit was typed in; the app saves the text
    /// through the code page instead. RESIDUAL: gamemd's units are code-page
    /// bytes, so it also takes a code page's letters beyond Latin-1 (`€` in
    /// 1252, Cyrillic in 1251); VERA's are Unicode, so those stay out, and the
    /// wire's low-byte name (`net::lan_messages`) could not carry them.
    pub(crate) fn name_edited(&mut self, text: &str) -> (Option<String>, Option<LanLobbyAction>) {
        let collapse = !text.is_empty() && text.chars().all(|c| c == ' ');
        let value = if collapse { "" } else { text };
        let kept = collapse.then(String::new);
        if value == self.notified_name {
            return (kept, None);
        }
        self.notified_name = value.to_owned();
        (kept, Some(LanLobbyAction::NameChanged(value.to_owned())))
    }
}

/// A key or typed text for BB while the lobby takes keys: Tab trades the
/// focus between the edits (the chat's at first), the rest goes to the
/// focused one. The name edit announces its changes, the chat edit caps
/// its text and sends its line on Enter (`0x5DE360`).
pub(crate) fn key(
    controller: &mut DialogController,
    lobby: &mut LanLobbyState,
    code: Option<winit::keyboard::KeyCode>,
    text: Option<&str>,
) -> Vec<LanLobbyAction> {
    let mut actions = Vec::new();
    if !lobby.takes_keys() {
        return actions;
    }
    let focus = controller.focused_control(PAGE.dialog);
    let key = code.and_then(|code| crate::ui::shell::controller::gui_key(code, false));
    if let Some(key @ (Key::Tab | Key::BackTab)) = key {
        controller.on_input(InputEvent::Key(key));
        controller.drain_events();
        return actions;
    }
    if focus == Some(CHAT) {
        let sent = chat_key(controller, PAGE.dialog, code, text);
        return sent.map(LanLobbyAction::SendChat).into_iter().collect();
    }
    if focus != Some(NAME) {
        return actions;
    }
    // Each typed character's EN_CHANGE announces the name (`0x5DDD90`).
    for input in edit_inputs(code, text) {
        controller.on_input(input);
        for event in controller.drain_events() {
            if let vera20k_gui::UiEvent::TextChanged { text, .. } = event {
                let (kept, action) = lobby.name_edited(&text);
                if let Some(kept) = kept {
                    controller.set_text(PAGE.dialog, NAME, &kept);
                }
                actions.extend(action);
            }
        }
    }
    actions
}

/// One port of the shared5EDD00 chat-length prepass (BB/BC/BD and WOL).
/// IsLatin1 tests>FF, but the following scan tests>100. DEC ESI wraps when
/// that scan stops at zero, leaving no cap; keep this unsigned native trap.
/// The helper's trimmed return is discarded by these LAN dialog callers.
pub(crate) fn apply_chat_length_limit(edit: &mut vera20k_gui::shell::new_edit::NewEdit) {
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

/// BB's captions (statics `0xFFFF`), the lobby paints them.
pub(crate) const LABELS: [(u16, &str); 3] = [
    (0xFF00, "GUI:Games"),
    (0xFF01, "GUI:Players"),
    (0xFF02, "GUI:YourName"),
];

/// BB at the current screen (`[0xBB]`): ordinary controls keep their
/// template places at every size, right-panel controls take the shared
/// anchors.
#[derive(Debug, Clone)]
pub(crate) struct LanLobbyLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) rects: RectTable,
    /// Whether VERA's send key sits beside the chat edit, and the edit and
    /// its row move to make room. Off keeps the native BB layout.
    pub(crate) touch_send: bool,
}

impl LanLobbyLayout {
    pub(crate) fn new(width: i32, height: i32, scale: ShellScale) -> Self {
        let (page, rects) = layout_data::menu_page(&PAGE, "0xBB", scale, width, height);
        Self {
            page,
            rects,
            touch_send: true,
        }
    }

    /// The layout with VERA's touch send key, for a platform without Enter.
    pub(crate) fn with_touch_send(mut self, on: bool) -> Self {
        self.touch_send = on;
        self
    }

    /// Control `id`'s window; none for a control BB does not show.
    pub(crate) fn rect(&self, id: u16) -> Option<RectPx> {
        self.rects.get(id).or_else(|| self.page.button_rect(id))
    }

    /// Where widget `id` of [`spec`] sits: [`Self::rect`], the heading on
    /// the panel's title and the status line's text in its window.
    pub(crate) fn widget_rect(&self, id: u16) -> Option<RectPx> {
        match id {
            TITLE => Some(self.page.title),
            STATUS => Some(crate::ui::shell::layout::status_text_rect(
                self.page.status_help,
            )),
            _ => self.rect(id),
        }
    }

    /// The chat row: the native place, or for a touch platform the room's
    /// spacing beside VERA's send key, with the log's bottom edge to the input
    /// row's top edge at 5 authored px.
    fn chat_row(&self) -> Option<RectPx> {
        let chat = self.rect(CHAT)?;
        if !self.touch_send {
            return Some(chat);
        }
        let log = self.rect(CHAT_LOG)?;
        let scale = self.page.scale;
        let y = log.y + log.h + (5.0 * scale.factor()).round() as i32;
        Some(RectPx::new(chat.x, y, chat.w, chat.h))
    }

    /// VERA's send key beside the chat box, at the LAN room's slot.
    pub(crate) fn chat_send(&self) -> Option<RectPx> {
        if !self.touch_send {
            return None;
        }
        let chat = self.chat_row()?;
        let log = self.rect(CHAT_LOG)?;
        Some(crate::ui::shell::geom::room_chat_send_rect(
            chat,
            log,
            self.page.scale,
        ))
    }

    /// The chat edit's rect: the native one, or the room's width and row
    /// position beside the send key.
    pub(crate) fn chat_text(&self) -> Option<RectPx> {
        let chat = self.rect(CHAT)?;
        if !self.touch_send {
            return Some(chat);
        }
        let row = self.chat_row()?;
        let log = self.rect(CHAT_LOG)?;
        Some(crate::ui::shell::geom::room_chat_text_rect(
            row,
            log,
            self.page.scale,
        ))
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    fn native() -> serde_json::Value {
        crate::test_fixture::json("tools/storage_oracle/lan_lobby.json").clone()
    }

    /// A touch platform moves the chat row beside VERA's send key and takes
    /// the key's width out of the edit; the native layout keeps both places.
    #[test]
    fn touch_send_moves_the_lobby_chat_row_and_narrows_its_edit() {
        let native = LanLobbyLayout::new(800, 600, crate::ui::shell::geom::RETAIL_SCALE)
            .with_touch_send(false);
        let touch = native.clone().with_touch_send(true);
        let native_chat = native.chat_text().unwrap();
        let touch_chat = touch.chat_text().unwrap();
        let send = touch.chat_send().unwrap();
        assert!(native.chat_send().is_none());
        assert!(touch_chat.w < native_chat.w, "the edit leaves the key room");
        assert!(touch_chat.y > native_chat.y, "the row takes the room's spacing");
        assert!(send.x >= touch_chat.x + touch_chat.w, "the key follows the edit");
    }

    /// A press selects a player row (`0x0061AA3E`); a roster rebuild
    /// (`0x005DAFE0`) moves the selection to the same name, or clears it.
    #[test]
    fn a_player_press_selection_follows_its_name_across_rebuilds() {
        let player = |name: &str| LanPlayerRow {
            name: name.to_owned(),
            color: None,
            side_icon: None,
            host: false,
            accepted: false,
        };
        let mut lobby = LanLobbyState::new("Tester");
        lobby.replace_players(vec![player("A"), player("B")]);
        assert!(lobby.select_player(1));
        assert!(!lobby.select_player(2));
        assert_eq!(lobby.selected_player, Some(1));
        lobby.replace_players(vec![player("b"), player("A")]);
        assert_eq!(lobby.selected_player, Some(0));
        lobby.replace_players(vec![player("A")]);
        assert_eq!(lobby.selected_player, None);
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

    /// A controller with BB's page built, as the lobby shows it.
    fn bb() -> (DialogController, LanLobbyState) {
        let mut controller = DialogController::default();
        controller.push(PAGE.dialog);
        let root = controller.root_in(PAGE.dialog).unwrap();
        crate::ui::shell::page::ensure_built(controller.gui_mut(), root, spec);
        controller.focus_control(PAGE.dialog, Some(CHAT));
        (controller, LanLobbyState::new("Tester"))
    }

    #[test]
    fn chat_retains_501_rows_and_does_not_jump_a_scrolled_view() {
        let line = |value: u16| LanChatLine {
            text: vec![value],
            color: u32::MAX,
        };
        let (mut lines, mut top) = (Vec::new(), 0);
        for value in 1..=501 {
            append_list_message(&mut lines, &mut top, line(value), 600, 8, |_| 8);
        }
        assert_eq!((lines.len(), top), (501, 493));
        top = 10;
        append_list_message(&mut lines, &mut top, line(502), 600, 8, |_| 8);
        assert_eq!(lines.len(), 501);
        assert_eq!(lines.first().unwrap().text, [2]);
        assert_eq!(top, 9);
    }

    /// VERA's room re-wraps its chat when the column changes width: a
    /// message wrapped wide splits to fit a narrow log, and a later message
    /// at the old width joins it again; the log hears each change.
    #[test]
    fn chat_rewraps_its_messages_to_a_new_width() {
        let measure = |units: &[u16]| units.len() as i32 * 8;
        let message = |text: &str| LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        };
        let rows = |chat: &LanChatState| -> Vec<String> {
            chat.lines
                .iter()
                .map(|line| String::from_utf16_lossy(&line.text))
                .collect()
        };
        let hint = "Players join ready. Press Start Game when everyone is here.";
        let mut chat = LanChatState::default();
        let mut heard = Vec::new();
        chat.append_message(message(hint), 600, measure, |r, d| heard.push((r, d)));
        assert_eq!(rows(&chat), [hint]);
        assert!(chat.rewrap(200, measure));
        assert_eq!(
            rows(&chat),
            [
                "Players join ready.",
                "Press Start Game when",
                "everyone is here."
            ]
        );
        chat.append_message(message("ok"), 600, measure, |r, d| heard.push((r, d)));
        assert_eq!(rows(&chat), [hint, "ok"]);
        assert_eq!(heard, [(0, 0), (1, 0), (1, 0), (2, 0)]);
    }

    #[test]
    fn chat_enter_matches_original_newedit_parent_getter_and_clear_chain() {
        for row in native()["chat"].as_array().unwrap() {
            let units: Vec<u16> = serde_json::from_value(row["input"].clone()).unwrap();
            let (mut controller, mut lobby) = bb();
            controller.set_text(PAGE.dialog, CHAT, &String::from_utf16(&units).unwrap());
            let sent = match key(
                &mut controller,
                &mut lobby,
                Some(winit::keyboard::KeyCode::Enter),
                None,
            )
            .as_slice()
            {
                [LanLobbyAction::SendChat(units)] => vec![units.clone()],
                [] => Vec::new(),
                other => panic!("{other:?}"),
            };
            assert_eq!(
                serde_json::to_value(sent).unwrap(),
                row["sent"],
                "{}",
                row["name"]
            );
            assert_eq!(controller.text(PAGE.dialog, CHAT), "");
        }
    }

    #[test]
    fn layout_and_help_follow_original_bb_execution() {
        let layout = LanLobbyLayout::new(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        for row in native()["layout_800"].as_array().unwrap() {
            let id = row["id"].as_u64().unwrap() as u32;
            let expected: Vec<i32> = serde_json::from_value(row["rect"].clone()).unwrap();
            let rect = match id as u16 {
                // The NewEdits' windows before 0x00614CCF's inset.
                NAME | CHAT => layout
                    .rect(id as u16)
                    .map(|r| RectPx::new(r.x - 1, r.y - 1, r.w + 2, r.h + 2)),
                id => layout.rect(id),
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

    /// Types `text` into the focused edit; the actions it made.
    fn type_text(
        controller: &mut DialogController,
        lobby: &mut LanLobbyState,
        text: &str,
    ) -> Vec<LanLobbyAction> {
        key(controller, lobby, None, Some(text))
    }

    #[test]
    fn name_edit_drops_units_from_0x100_takes_16_bytes_rejects_commas_and_keeps_spaces() {
        let (mut controller, _) = bb();
        let mut lobby = LanLobbyState::new("");
        controller.set_text(PAGE.dialog, NAME, "");
        key(
            &mut controller,
            &mut lobby,
            Some(winit::keyboard::KeyCode::Tab),
            None,
        );
        assert_eq!(controller.focused_control(PAGE.dialog), Some(NAME));
        // Latin-1 gets in; Chinese, an emoji and `€` (U+20AC) stay out, no `?`.
        type_text(&mut controller, &mut lobby, "  A,é€玩😀B");
        assert_eq!(controller.text(PAGE.dialog, NAME), "AéB");
        type_text(&mut controller, &mut lobby, "12345678901234567890");
        let saved = crate::util::native_string::acp_encode(&controller.text(PAGE.dialog, NAME));
        assert_eq!(saved.len(), 16);
        let enter = Some(winit::keyboard::KeyCode::Enter);
        assert_eq!(key(&mut controller, &mut lobby, enter, None), []);
        assert_eq!(LanLobbyState::new(" A ").notified_name, " A ");
    }

    /// Typing into a name beyond Latin-1 and deleting it again reports the
    /// name as shown, not its characters' low bytes (`玩家` as `©¶`).
    #[test]
    fn name_edit_reports_the_name_it_shows() {
        let (mut controller, _) = bb();
        let mut lobby = LanLobbyState::new("玩家");
        controller.set_text(PAGE.dialog, NAME, &lobby.notified_name.clone());
        controller.focus_control(PAGE.dialog, Some(NAME));
        assert_eq!(
            type_text(&mut controller, &mut lobby, "x"),
            vec![LanLobbyAction::NameChanged("玩家x".into())]
        );
        assert_eq!(
            key(
                &mut controller,
                &mut lobby,
                Some(winit::keyboard::KeyCode::Backspace),
                None
            ),
            vec![LanLobbyAction::NameChanged("玩家".into())]
        );
    }

    #[test]
    fn discovery_clamps_selection_and_join_remains_an_intent() {
        let mut lobby = LanLobbyState::new("Tester");
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
                change: LanSelectionChange::Changed
            })
        );
        assert_eq!(
            lobby.replace_games(Vec::new()),
            Some(LanLobbyAction::SelectGame {
                game_id: None,
                change: LanSelectionChange::Clamped
            })
        );
        assert_eq!(
            lobby.activate(JOIN, false),
            Some(LanLobbyAction::Join {
                game_id: None,
                observe: false
            })
        );
        assert!(!lobby.enabled);
        assert_eq!(lobby.activate(CREATE, false), None);
        lobby.set_enabled(true);
        assert_eq!(
            lobby.activate(OBSERVER_IMAGE, false),
            Some(LanLobbyAction::ObserverChanged(true))
        );
        assert_eq!(
            lobby.activate(CREATE, true),
            Some(LanLobbyAction::Create { observe: true })
        );
    }
}
