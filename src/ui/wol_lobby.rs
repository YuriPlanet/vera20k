//! Westwood Online custom lobby `0x113` (`Dialog0x113__DlgProc`
//! `0x0078AC10`): its executed layout and help, its list contents and
//! interaction state, and the two popups it runs, Create Game `0xC3`
//! (`0x0078DB70`) and the channel password `0xE1` (`0x0078E030`).
//!
//! Channel and user records, the lobby runner (`0x00787770`), service
//! requests and timers belong to the app's lobby owner; this module holds
//! what the dialog's children show and how they take input. The chat edit,
//! transcript and lists are the shared LAN owners ([`LanChatState`],
//! [`LanListState`]).

mod channels;
mod popups;

pub(crate) use channels::*;
pub(crate) use popups::*;

use crate::ui::lan_shell::{LanChatState, LanEditKey, LanListState, apply_chat_length_limit};
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::descriptor::{AnchorRule, DialogId};
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale};
use crate::ui::shell::layout::{anchor_rect, status_line_rect};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::skirmish_shell::SavedSeedDescriptionEdit;

pub(crate) const DIALOG: DialogId = DialogId(0x0113);

pub(crate) const CREATE: u16 = 0x62B;
pub(crate) const JOIN: u16 = 0x62D;
pub(crate) const BUDDY: u16 = 0x689;
pub(crate) const CLAN: u16 = 0x688;
pub(crate) const TOURNAMENT: u16 = 0x7A3;
pub(crate) const NEWS: u16 = 0x7A6;
pub(crate) const OPTIONS: u16 = 0x61F;
pub(crate) const PAGE_BUTTON: u16 = 0x5C2;
pub(crate) const BACK: u16 = 0x686;
pub(crate) const REFRESH: u16 = 0x55D;
pub(crate) const EMOTE: u16 = 0x3ED;
pub(crate) const LOBBIES: u16 = 0x53D;
pub(crate) const GAMES: u16 = 0x6EE;
pub(crate) const PLAYERS: u16 = 0x53E;
pub(crate) const CHAT_LOG: u16 = 0x53F;
pub(crate) const CHAT: u16 = 0x540;
pub(crate) const OBSERVER_IMAGE: u16 = 0x75F;
pub(crate) const OBSERVER_TOGGLE: u16 = 0x760;
pub(crate) const SORT_ALPHA: u16 = 0x765;
pub(crate) const SORT_PING: u16 = 0x766;
pub(crate) const SORT_RESLOCK: u16 = 0x767;
pub(crate) const SORT_PASSWORD: u16 = 0x768;
pub(crate) const SORT_GAMETYPE: u16 = 0x769;
pub(crate) const PLAYER_SORT_ALPHA: u16 = 0x76A;
pub(crate) const PLAYER_SORT_RANK: u16 = 0x76B;
pub(crate) const PLAYER_SORT_CLAN: u16 = 0x76C;

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

/// The right column: eight top-list buttons (`0x0060B000`) and Back
/// (`0x0060B350`). Join is created `WS_DISABLED`.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: "GUI:CustomMatch",
    stacked: &[
        button(CREATE, 111, "GUI:CreateGame", "STT:LobbyButtonNew"),
        button(JOIN, 138, "GUI:JoinGame", "STT:LobbyButtonJoin"),
        button(BUDDY, 166, "GUI:PlayBuddy", "STT:WOLBuddy"),
        button(CLAN, 193, "GUI:Clan", "STT:WOLClan"),
        button(TOURNAMENT, 222, "GUI:Tournament", "STT:WOLTournament"),
        button(NEWS, 250, "GUI:News", "STT:WOLNews"),
        button(OPTIONS, 278, "GUI:WOLOptions", "STT:WOLLobbyOptions"),
        button(PAGE_BUTTON, 305, "GUI:PageButton", "STT:WOLPageButton"),
    ],
    back: button(BACK, 332, "GUI:Back", "STT:WOLLobbyBack"),
};

/// The game list's header buttons and the player list's, with captions.
pub(crate) const SORT_BUTTONS: [(u16, &str); 8] = [
    (SORT_GAMETYPE, "GUI:GamesortGametype"),
    (SORT_PASSWORD, "GUI:GamesortPassword"),
    (SORT_RESLOCK, "GUI:GamesortReslock"),
    (SORT_ALPHA, "GUI:GamesortAlpha"),
    (SORT_PING, "GUI:GamesortPing"),
    (PLAYER_SORT_ALPHA, "GUI:PlayersortAlpha"),
    (PLAYER_SORT_CLAN, "GUI:PlayersortClan"),
    (PLAYER_SORT_RANK, "GUI:PlayersortRank"),
];

/// The image buttons' PCX faces set at `0x497`: released (`0x49C`), then
/// pressed (`0x4AA`).
pub(crate) const IMAGE_BUTTONS: [(u16, &str, &str); 5] = [
    (EMOTE, "wodact.pcx", "wouact.pcx"),
    (REFRESH, "wouref.pcx", "wodref.pcx"),
    (SORT_GAMETYPE, "woluclan.pcx", "woldclan.pcx"),
    (SORT_PASSWORD, "wolupriv.pcx", "woldpriv.pcx"),
    (SORT_RESLOCK, "wolurslkob.pcx", "woldrslkob.pcx"),
];

/// Every PCX the lobby's children and list columns draw.
pub(crate) const PCX_FILES: [&str; 19] = [
    "wodact.pcx",
    "wouact.pcx",
    "wouref.pcx",
    "wodref.pcx",
    "woluclan.pcx",
    "woldclan.pcx",
    "wolupriv.pcx",
    "woldpriv.pcx",
    "wolurslkob.pcx",
    "woldrslkob.pcx",
    "gt18.pcx",
    "wolclan.pcx",
    "woltrny.pcx",
    "wolpriv.pcx",
    "wolreslk.pcx",
    "wolrslkob.pcx",
    "wolob.pcx",
    "woloper.pcx",
    "wolvoice.pcx",
];

/// List column stops (`0x4A6` at `0x497`): the game list's name column
/// x=0x35 (0x93 wide), type 2, password 0x13, ping 200 and limit 0x24; the
/// player list's name x=0x4E (0xA2 wide), op/voice 2 and clan tag 0xAA.
pub(crate) const GAME_NAME_X: i32 = 0x35;
pub(crate) const GAME_NAME_W: i32 = 0x93;
pub(crate) const GAME_TYPE_X: i32 = 2;
pub(crate) const GAME_PASSWORD_X: i32 = 0x13;
pub(crate) const GAME_LIMIT_X: i32 = 0x24;
pub(crate) const GAME_PING_X: i32 = 200;
pub(crate) const LOBBY_TEXT_X: i32 = 2;
pub(crate) const PLAYER_NAME_X: i32 = 0x4E;
pub(crate) const PLAYER_NAME_W: i32 = 0xA2;
pub(crate) const PLAYER_ICON_X: i32 = 2;
pub(crate) const PLAYER_CLAN_X: i32 = 0xAA;

/// Executed `0x6040B0` help of `0x113`. The lists answer through `0x4E9`
/// instead (`0x00780E30`, `0x00781090`); the app adapter builds those.
pub(crate) fn tooltip_key(control: u16) -> Option<&'static str> {
    if let Some(spec) = PAGE.button(control) {
        return Some(spec.tooltip_key);
    }
    Some(match control {
        EMOTE => "STT:EmoteButton",
        LOBBIES => "STT:WOLLobbyChannelList",
        CHAT_LOG => "STT:LobbyEditOutput",
        CHAT => "STT:LobbyEditInput",
        REFRESH => "STT:WOLLobbyRefreshChannels",
        OBSERVER_TOGGLE => "STT:ObserverToggle",
        SORT_ALPHA => "STT:WOLGamesortAlpha",
        SORT_PING => "STT:WOLGamesortPing",
        SORT_RESLOCK => "STT:WOLGamesortReslock",
        SORT_PASSWORD => "STT:WOLGamesortPassword",
        SORT_GAMETYPE => "STT:WOLGamesortGametype",
        PLAYER_SORT_ALPHA => "STT:WOLPlayersortAlpha",
        PLAYER_SORT_RANK => "STT:WOLPlayersortRank",
        PLAYER_SORT_CLAN => "STT:WOLPlayersortClan",
        _ => return None,
    })
}

/// Heading `0x694` and the chat-lobby list `0x53D`: `0x0060B1D0`'s
/// right-panel anchor of the window one pixel wider and taller than its
/// conversion. The executed heading sits at y 7 (800x600), not the +1 of
/// [`crate::ui::shell::descriptor::LOW_HEADING_ANCHOR`].
const RIGHT_PANEL_CHILD_ANCHOR: AnchorRule = AnchorRule::RightAnchorRuntimeAdjust {
    resource_dw: 1,
    resource_dh: 1,
    inset: None,
    dy: 0,
    dh: 0,
};

#[derive(Debug, Clone)]
pub(crate) struct WolLobbyLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) lobbies: RectPx,
    pub(crate) games: RectPx,
    pub(crate) players: RectPx,
    pub(crate) chat_log: RectPx,
    /// The chat NewEdit after its init inset (`0x00614CCF`).
    pub(crate) chat: RectPx,
    pub(crate) emote: RectPx,
    pub(crate) refresh: RectPx,
    pub(crate) sort_buttons: [(u16, RectPx); 8],
    pub(crate) observer_image: RectPx,
    pub(crate) observer_toggle: RectPx,
    pub(crate) labels: [(&'static str, RectPx); 2],
}

impl WolLobbyLayout {
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::RETAIL)
    }

    pub(crate) fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height))
    }

    fn build(width: i32, height: i32, scale: ShellScale) -> Self {
        let mut page = if scale.is_retail() {
            menu_page::compute_layout(&PAGE, width as u32, height as u32)
        } else {
            menu_page::compute_responsive_layout(&PAGE, width as u32, height as u32)
        };
        // Executed relayout 0x60AAB0 -> 0x60C540 -> 0x60C4A0 -> 0x60C0C0
        // (tools/storage_oracle/wol_shell.json): the body keeps its template
        // windows at every size; heading, lobby list and status line anchor.
        let (aw, ah) = if scale.is_retail() {
            (width, height)
        } else {
            (SHELL_BASE_W, SHELL_BASE_H)
        };
        page.title = scale.scale_rect(anchor_rect(
            RIGHT_PANEL_CHILD_ANCHOR,
            RectPx::new(423, 4, 108, 11),
            aw,
            ah,
        ));
        page.status_help = scale.scale_rect(status_line_rect(RectPx::new(1, 351, 316, 12), aw, ah));
        // The body's executed windows (identical at every size); the chat
        // NewEdit is inset +1/-2 by 0x00614CCF.
        let px = |x, y, w, h| scale.scale_rect(RectPx::new(x, y, w, h));
        Self {
            page,
            lobbies: scale.scale_rect(anchor_rect(
                RIGHT_PANEL_CHILD_ANCHOR,
                RectPx::new(428, 22, 103, 73),
                aw,
                ah,
            )),
            games: px(11, 57, 301, 307),
            players: px(321, 57, 301, 307),
            chat_log: px(11, 369, 611, 160),
            chat: px(12, 534, 578, 22),
            emote: px(596, 533, 27, 24),
            refresh: px(12, 10, 27, 24),
            sort_buttons: [
                (SORT_GAMETYPE, px(12, 34, 15, 21)),
                (SORT_PASSWORD, px(28, 34, 18, 21)),
                (SORT_RESLOCK, px(45, 34, 18, 21)),
                (SORT_ALPHA, px(63, 37, 148, 17)),
                (SORT_PING, px(210, 37, 100, 17)),
                (PLAYER_SORT_ALPHA, px(398, 37, 93, 17)),
                (PLAYER_SORT_CLAN, px(491, 37, 69, 17)),
                (PLAYER_SORT_RANK, px(560, 37, 61, 17)),
            ],
            observer_image: px(594, 10, 28, 21),
            observer_toggle: px(576, 13, 16, 17),
            labels: [
                ("GUI:OpenGames", px(50, 10, 201, 17)),
                ("GUI:Players", px(324, 10, 157, 17)),
            ],
        }
    }

    /// The child under `(x, y)`: buttons first, then the template's order.
    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        if let Some(button) = self.page.buttons.iter().find(|b| b.rect.contains(x, y)) {
            return Some(button.id);
        }
        if let Some((id, _)) = self.sort_buttons.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(*id);
        }
        [
            (REFRESH, self.refresh),
            (EMOTE, self.emote),
            (OBSERVER_TOGGLE, self.observer_toggle),
            (OBSERVER_IMAGE, self.observer_image),
            (LOBBIES, self.lobbies),
            (GAMES, self.games),
            (PLAYERS, self.players),
            (CHAT_LOG, self.chat_log),
            (CHAT, self.chat),
        ]
        .into_iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(id, _)| id)
    }

    pub(crate) fn control_rect(&self, id: u16) -> Option<RectPx> {
        if let Some(rect) = self.page.button_rect(id) {
            return Some(rect);
        }
        if let Some((_, rect)) = self.sort_buttons.iter().find(|(known, _)| *known == id) {
            return Some(*rect);
        }
        Some(match id {
            REFRESH => self.refresh,
            EMOTE => self.emote,
            OBSERVER_TOGGLE => self.observer_toggle,
            OBSERVER_IMAGE => self.observer_image,
            LOBBIES => self.lobbies,
            GAMES => self.games,
            PLAYERS => self.players,
            CHAT_LOG => self.chat_log,
            CHAT => self.chat,
            _ => return None,
        })
    }
}

/// One player-list row (`0x00783C80`, chat lobby).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolPlayerRow {
    pub(crate) name: String,
    /// `woloper.pcx` with `TXT_OPER`, else `wolvoice.pcx` with `TXT_VOICE`.
    pub(crate) icon: Option<(&'static str, &'static str)>,
    /// `"[%hs]"` of the tag, with the clan name as its tooltip.
    pub(crate) clan: Option<(String, String)>,
}

impl WolPlayerRow {
    pub(crate) fn from_member(member: &WolMember) -> Self {
        Self {
            name: member.name.clone(),
            icon: if member.flags & MEMBER_OPERATOR != 0 {
                Some(("woloper.pcx", "TXT_OPER"))
            } else if member.flags & MEMBER_VOICE != 0 {
                Some(("wolvoice.pcx", "TXT_VOICE"))
            } else {
                None
            },
            clan: (!member.clan_tag.is_empty())
                .then(|| (format!("[{}]", member.clan_tag), member.clan_name.clone())),
        }
    }
}

/// What one lobby interaction asks the app's lobby owner to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolLobbyAction {
    Create,
    Join,
    Back,
    Refresh,
    GameSort(u16),
    PlayerSort(u16),
    /// `0x6EE` LBN_SELCHANGE (1) or LBN_DBLCLK (2) on a row.
    SelectGame {
        row: usize,
        double: bool,
    },
    /// `0x53D` LBN_SELCHANGE.
    SelectLobby(usize),
    /// `0x540` Enter after `0x005EDD00`.
    Chat(Vec<u16>),
    /// `0x3ED` with the capped edit text.
    Emote(Vec<u16>),
    ObserverChanged(bool),
    /// Buddy List, Clan, Tournament, News, Options and Page run dialogs or
    /// web pages that are later chains.
    Unported(u16),
    Popup(WolPopupAction),
}

/// The controls `0x0078AC10` disables around Create and Join.
const COMMAND_BUTTONS: [u16; 9] = [
    CREATE,
    JOIN,
    BUDDY,
    CLAN,
    TOURNAMENT,
    NEWS,
    OPTIONS,
    PAGE_BUTTON,
    BACK,
];
/// The runner's lobby switch (`0x00787ED8` path) also disables both lists.
const SWITCH_CONTROLS: [u16; 11] = [
    LOBBIES,
    GAMES,
    JOIN,
    CREATE,
    BUDDY,
    CLAN,
    TOURNAMENT,
    NEWS,
    OPTIONS,
    PAGE_BUTTON,
    BACK,
];

/// The dialog's children: list contents, selection, enable state, chat.
#[derive(Debug, Clone)]
pub(crate) struct WolLobbyState {
    pub(crate) chat: LanChatState,
    pub(crate) lobby_rows: Vec<WolLobbyRow>,
    pub(crate) game_rows: Vec<WolGameRow>,
    pub(crate) player_rows: Vec<WolPlayerRow>,
    pub(crate) lobby_list: LanListState,
    pub(crate) game_list: LanListState,
    /// `LBS_MULTIPLESEL`: the selected rows are the private chat targets;
    /// every refill (`LB_RESETCONTENT`) clears them.
    pub(crate) player_list: LanListState,
    pub(crate) selected_players: Vec<usize>,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    disabled: Vec<u16>,
    /// `[0xAC10C8]`'s view; the multiplayer preference owner keeps it.
    pub(crate) observer: bool,
    /// `[0xB77E78]`: the game name the last selection admitted.
    pub(crate) admitted_name: Option<String>,
    /// `[0x84A23C]`: the row index of the last Join (initially -1).
    pub(crate) join_row: Option<usize>,
    pub(crate) popup: Option<WolLobbyPopup>,
    focus_started_at: std::time::Instant,
}

impl WolLobbyState {
    pub(crate) fn new(observer: bool) -> Self {
        Self {
            chat: LanChatState {
                edit: SavedSeedDescriptionEdit {
                    focused: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            lobby_rows: Vec::new(),
            game_rows: Vec::new(),
            player_rows: Vec::new(),
            lobby_list: LanListState::default(),
            game_list: LanListState::default(),
            player_list: LanListState::default(),
            selected_players: Vec::new(),
            buttons: ShellButtonInteraction::default(),
            disabled: vec![JOIN],
            observer,
            admitted_name: None,
            join_row: None,
            popup: None,
            focus_started_at: std::time::Instant::now(),
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

    /// Create/Join's disable of every command button (and the game list
    /// selection for Create).
    pub(crate) fn disable_commands(&mut self) {
        for control in COMMAND_BUTTONS {
            self.set_enabled(control, false);
        }
    }

    /// Re-enable them; Join takes `join`.
    pub(crate) fn enable_commands(&mut self, join: bool) {
        for control in COMMAND_BUTTONS {
            self.set_enabled(control, control != JOIN || join);
        }
    }

    pub(crate) fn begin_lobby_switch(&mut self) {
        for control in SWITCH_CONTROLS {
            self.set_enabled(control, false);
        }
    }

    /// `0x007A5E40` after a chat lobby joined: everything back, Join as the
    /// still-selected game admits.
    pub(crate) fn lobby_switched(&mut self, join: bool) {
        for control in SWITCH_CONTROLS {
            self.set_enabled(control, control != JOIN || join);
        }
    }

    pub(crate) fn caret_visible(&self, now: std::time::Instant) -> bool {
        now.saturating_duration_since(self.focus_started_at)
            .as_secs()
            % 2
            == 0
    }

    pub(crate) fn cancel_pointer(&mut self) {
        self.buttons = ShellButtonInteraction::default();
        for list in [
            &mut self.lobby_list,
            &mut self.game_list,
            &mut self.player_list,
            &mut self.chat.list,
        ] {
            list.scroll.cancel();
            list.last_press = None;
        }
        if let Some(popup) = self.popup.as_mut() {
            popup.cancel_pointer();
        }
    }

    pub(crate) fn selected_game(&self) -> Option<&WolGameRow> {
        self.game_list
            .selected
            .and_then(|row| self.game_rows.get(row))
    }

    /// `0x00781480`: the rows replaced, the selection and the top row found
    /// again by name (`0x4C5`), or cleared.
    pub(crate) fn replace_games(&mut self, rows: Vec<WolGameRow>) {
        let selected = self.selected_game().map(|row| row.name.clone());
        let top = self
            .game_rows
            .get(self.game_list.top_index)
            .map(|row| row.name.clone());
        self.game_rows = rows;
        let find = |name: Option<String>| {
            name.and_then(|name| self.game_rows.iter().position(|row| row.name == name))
        };
        self.game_list.selected = find(selected);
        if let Some(top) = find(top) {
            self.game_list.top_index = top;
        }
        self.game_list.hot_row = None;
    }

    /// `0x007806C0`: the current channel's row is selected.
    pub(crate) fn replace_lobbies(&mut self, rows: Vec<WolLobbyRow>, current: Option<&str>) {
        let top = self
            .lobby_rows
            .get(self.lobby_list.top_index)
            .map(|row| row.name.clone());
        self.lobby_rows = rows;
        self.lobby_list.selected =
            current.and_then(|current| self.lobby_rows.iter().position(|row| row.name == current));
        if let Some(top) =
            top.and_then(|top| self.lobby_rows.iter().position(|row| row.name == top))
        {
            self.lobby_list.top_index = top;
        }
        self.lobby_list.hot_row = None;
    }

    /// `0x00783C80`: a reset list keeps only its top index.
    pub(crate) fn replace_players(&mut self, rows: Vec<WolPlayerRow>) {
        self.player_rows = rows;
        self.selected_players.clear();
        self.player_list.hot_row = None;
    }

    /// The selected players' names (`LB_GETSELITEMS`, at most 32).
    pub(crate) fn private_targets(&self) -> Vec<String> {
        let mut rows = self.selected_players.clone();
        rows.sort_unstable();
        rows.into_iter()
            .filter_map(|row| self.player_rows.get(row))
            .take(32)
            .map(|row| row.name.clone())
            .collect()
    }

    pub(crate) fn toggle_player(&mut self, row: usize) {
        if row >= self.player_rows.len() {
            return;
        }
        if let Some(at) = self.selected_players.iter().position(|known| *known == row) {
            self.selected_players.remove(at);
        } else {
            self.selected_players.push(row);
        }
    }

    pub(crate) fn select_game(&mut self, row: usize, double: bool) -> Option<WolLobbyAction> {
        if !self.enabled(GAMES) || row >= self.game_rows.len() {
            return None;
        }
        self.game_list.selected = Some(row);
        Some(WolLobbyAction::SelectGame { row, double })
    }

    pub(crate) fn select_lobby(&mut self, row: usize) -> Option<WolLobbyAction> {
        if !self.enabled(LOBBIES) || row >= self.lobby_rows.len() {
            return None;
        }
        self.lobby_list.selected = Some(row);
        Some(WolLobbyAction::SelectLobby(row))
    }

    /// A released button or picture.
    pub(crate) fn activate(&mut self, control: u16) -> Option<WolLobbyAction> {
        if !self.enabled(control) {
            return None;
        }
        Some(match control {
            CREATE => WolLobbyAction::Create,
            JOIN => WolLobbyAction::Join,
            BACK => WolLobbyAction::Back,
            REFRESH => WolLobbyAction::Refresh,
            SORT_ALPHA | SORT_PING | SORT_RESLOCK | SORT_PASSWORD | SORT_GAMETYPE => {
                WolLobbyAction::GameSort(control)
            }
            PLAYER_SORT_ALPHA | PLAYER_SORT_RANK | PLAYER_SORT_CLAN => {
                WolLobbyAction::PlayerSort(control)
            }
            EMOTE => {
                // 0x3ED: 0x005EDD00 caps and reads the edit, which clears.
                apply_chat_length_limit(&mut self.chat.edit);
                let text =
                    crate::ui::wol_chat::trim_line_end(&self.chat.edit.wide_text(111)).to_vec();
                self.chat.edit.set_units_limited([], 0);
                WolLobbyAction::Emote(text)
            }
            OBSERVER_IMAGE | OBSERVER_TOGGLE => {
                // 0x75F/0x760 write [0xAC10C8] from the checkbox, as BB.
                self.observer = !self.observer;
                WolLobbyAction::ObserverChanged(self.observer)
            }
            BUDDY | CLAN | TOURNAMENT | NEWS | OPTIONS | PAGE_BUTTON => {
                WolLobbyAction::Unported(control)
            }
            _ => return None,
        })
    }

    /// Keys reach the popup's edit, else the chat NewEdit (always focused).
    pub(crate) fn edit_key(&mut self, key: LanEditKey) -> Option<WolLobbyAction> {
        if let Some(popup) = self.popup.as_mut() {
            return popup.edit_key(key).map(WolLobbyAction::Popup);
        }
        if key == LanEditKey::Tab {
            return None;
        }
        let sent = self.chat.key(key)?;
        let text = crate::ui::wol_chat::trim_line_end(&sent).to_vec();
        (!text.is_empty()).then_some(WolLobbyAction::Chat(text))
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        match self.popup.as_mut() {
            Some(popup) => popup.insert_text(text),
            None => self.chat.insert_text(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap()
    }

    fn rect(value: &serde_json::Value) -> RectPx {
        let a: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
        RectPx::new(a[0], a[1], a[2], a[3])
    }

    #[test]
    fn layout_and_help_follow_the_executed_0x113_table() {
        let corpus = corpus();
        let mut checked = 0;
        for (size, case) in corpus["layouts"]["0x113"]["sizes"].as_object().unwrap() {
            let (width, height) = size.split_once('x').unwrap();
            let layout = WolLobbyLayout::new(width.parse().unwrap(), height.parse().unwrap());
            for (key, child) in case["controls"].as_object().unwrap() {
                let mut native = rect(&child["rect"]);
                let actual = match key.as_str() {
                    "0xFFFF" => layout.labels[0].1,
                    "0xFFFF#1" => layout.labels[1].1,
                    "0x694" => layout.page.title,
                    "0x695" => layout.page.status_help,
                    _ => {
                        let id = u16::from_str_radix(&key[2..], 16).unwrap();
                        if id == CHAT {
                            native =
                                RectPx::new(native.x + 1, native.y + 1, native.w - 2, native.h - 2);
                        }
                        layout.control_rect(id).unwrap_or_else(|| panic!("{key}"))
                    }
                };
                assert_eq!(actual, native, "{size} {key}");
                checked += 1;
            }
        }
        assert_eq!(checked, 90);
        for (key, help) in corpus["help"]["0x113"].as_object().unwrap() {
            let id = u16::from_str_radix(&key[2..], 16).unwrap();
            // The lists' null help is answered through 0x4E9 instead.
            assert_eq!(tooltip_key(id), help.as_str(), "{key}");
        }
    }

    #[test]
    fn create_and_join_disable_the_command_column_and_join_starts_disabled() {
        let mut lobby = WolLobbyState::new(false);
        assert!(!lobby.enabled(JOIN) && lobby.enabled(CREATE));
        assert_eq!(lobby.activate(JOIN), None);
        lobby.disable_commands();
        assert_eq!(lobby.activate(BACK), None);
        lobby.enable_commands(false);
        assert_eq!(lobby.activate(BACK), Some(WolLobbyAction::Back));
        assert!(!lobby.enabled(JOIN));
        lobby.begin_lobby_switch();
        assert!(!lobby.enabled(GAMES) && !lobby.enabled(LOBBIES));
        lobby.lobby_switched(true);
        assert!(lobby.enabled(JOIN) && lobby.enabled(GAMES));
    }

    #[test]
    fn refills_find_the_selection_by_name_and_reset_player_selection() {
        let row = |name: &str| WolGameRow {
            record: 0,
            name: name.into(),
            text: name.into(),
            greyed: false,
            kind: GameTypeIcon::Plain,
            keyed: false,
            limit_icon: None,
            ping: -1,
        };
        let mut lobby = WolLobbyState::new(false);
        lobby.replace_games(vec![row("a"), row("b")]);
        lobby.select_game(1, false);
        lobby.replace_games(vec![row("c"), row("a"), row("b")]);
        assert_eq!(lobby.game_list.selected, Some(2));
        lobby.replace_games(vec![row("a")]);
        assert_eq!(lobby.game_list.selected, None);

        let player = |name: &str| WolPlayerRow {
            name: name.into(),
            icon: None,
            clan: None,
        };
        lobby.replace_players(vec![player("x"), player("y"), player("z")]);
        lobby.toggle_player(2);
        lobby.toggle_player(0);
        assert_eq!(lobby.private_targets(), ["x", "z"]);
        lobby.toggle_player(0);
        assert_eq!(lobby.private_targets(), ["z"]);
        lobby.replace_players(vec![player("x")]);
        assert!(lobby.private_targets().is_empty());
    }

    #[test]
    fn chat_enter_and_emote_read_and_clear_the_edit() {
        let mut lobby = WolLobbyState::new(false);
        lobby.insert_text("hello");
        assert_eq!(
            lobby.edit_key(LanEditKey::Enter),
            Some(WolLobbyAction::Chat("hello".encode_utf16().collect()))
        );
        assert!(lobby.chat.edit.units.is_empty());
        assert_eq!(lobby.edit_key(LanEditKey::Enter), None);
        lobby.insert_text("waves");
        assert_eq!(
            lobby.activate(EMOTE),
            Some(WolLobbyAction::Emote("waves".encode_utf16().collect()))
        );
        assert_eq!(
            lobby.activate(EMOTE),
            Some(WolLobbyAction::Emote(Vec::new()))
        );
        assert_eq!(
            lobby.activate(OBSERVER_IMAGE),
            Some(WolLobbyAction::ObserverChanged(true))
        );
    }
}
