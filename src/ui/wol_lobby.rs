//! Westwood Online custom lobby `0x113` (`Dialog0x113__DlgProc`
//! `0x0078AC10`): its executed layout and help, its list contents and
//! interaction state, and the two popups it runs, Create Game `0xC3`
//! (`0x0078DB70`) and the channel password `0xE1` (`0x0078E030`).
//!
//! Channel and user records, the lobby runner (`0x00787770`), service
//! requests and timers belong to the app's lobby owner; this module holds
//! what the dialog's children show and how they take input. The children
//! are `vera20k-gui` widgets in the lobby's own tree, which owns the lists'
//! selection and scroll, the edits' text and the boxes' values; the chat
//! transcript is the shared LAN owner ([`LanChatState`]).

mod channels;
mod player_card;
mod popups;

pub(crate) use channels::*;
pub(crate) use player_card::*;
pub(crate) use popups::*;

use vera20k_gui::{
    DialogId as GuiDialog, DialogKind, Gui, InputEvent, NodeId, NodeKind, NodeSpec, Prop, Value,
};

use crate::ui::lan_shell::{
    LanChatState, PlayerNameMatch, apply_chat_length_limit, saved_player_name_matches,
};
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::layout::RectTable;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::page::{self, PageCue, PageEvent};
use crate::util::time::Instant;
use vera20k_gui::shell::list::ListMetrics;
use vera20k_gui::shell::new_edit::NewEdit;

pub(crate) const DIALOG: DialogResourceId = DialogResourceId(0x0113);

pub(crate) const CREATE: u16 = 0x62B;
pub(crate) const JOIN: u16 = 0x62D;
pub(crate) const BUDDY: u16 = 0x689;
pub(crate) const CLAN: u16 = 0x688;
pub(crate) const TOURNAMENT: u16 = 0x7A3;
pub(crate) const NEWS: u16 = 0x7A6;
pub(crate) const OPTIONS: u16 = 0x61F;
pub(crate) const PAGE_BUTTON: u16 = 0x5C2;
/// The online page's Quick Match: the welcome's control.
pub(crate) const QUICK_MATCH: u16 = crate::ui::wol_shell::QUICK_MATCH;
pub(crate) const BACK: u16 = 0x686;
/// VERA: Back to Room, while this player visits the lobby from a game room.
pub(crate) const BACK_TO_ROOM: u16 = 0x7E30;
/// VERA: the online page's button between its games and its chat with the
/// player list ([`LobbyView`]).
pub(crate) const VIEW: u16 = 0x7E3A;
/// VERA: the online page's Join Any Game ([`WolLobbyAction::JoinAny`]).
pub(crate) const JOIN_ANY: u16 = 0x7E3B;
/// VERA: the games view's filter row under the list: the room search and
/// the Hide Full and Hide Locked checkboxes ([`RoomFilter`]).
pub(crate) const SEARCH: u16 = 0x7E3C;
pub(crate) const HIDE_FULL: u16 = 0x7E3D;
pub(crate) const HIDE_LOCKED: u16 = 0x7E3E;
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

/// The modern lobby lists names and ping, and with its one chat lobby it
/// shows no lobby list; legacy service-only columns and the list stay
/// available to native diagnostics through the same layout and state owners.
const LEGACY_CONTROLS: [u16; 6] = [
    SORT_GAMETYPE,
    SORT_PASSWORD,
    SORT_RESLOCK,
    PLAYER_SORT_CLAN,
    PLAYER_SORT_RANK,
    LOBBIES,
];

const fn button(
    id: u16,
    dlu_top: i32,
    csf_key: &'static str,
    tooltip_key: &'static str,
) -> MenuPageButtonSpec {
    MenuPageButtonSpec::new(id, dlu_top, csf_key, tooltip_key)
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

/// The normal VERA lobby keeps core play and Options. Its third row is
/// VERA's Join Any Game. It has no welcome `0x10E`, so its fourth row is that
/// page's Quick Match (`0x6E0`, its caption and tooltip) and Options moves to
/// the fifth. The sixth swaps the games for the chat and the player list; its
/// caption and help are the view's ([`WolLobbyState::view_caption`]). The
/// retail page above remains the owner for legacy service diagnostics.
const ONLINE_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        button(CREATE, 111, "GUI:CreateGame", "STT:LobbyButtonNew"),
        button(JOIN, 138, "GUI:JoinGame", "STT:LobbyButtonJoin"),
        button(JOIN_ANY, 166, "VERA:JoinAnyGame", "VERA:JoinAnyGameHelp"),
        button(
            QUICK_MATCH,
            193,
            "GUI:QuickMatch",
            "STT:WOLWelcomeQuickMatch",
        ),
        button(OPTIONS, 222, "GUI:WOLOptions", "STT:WOLLobbyOptions"),
        button(VIEW, 250, "VERA:LobbyChat", "VERA:LobbyChatHelp"),
    ],
    ..PAGE
};

/// The online page while this player visits the lobby from a game room
/// (docs/lobby-protocol.md "Visiting the lobby"): Join Game, which leaves
/// that room, Options, Back to Room and the view button. Its Back goes back
/// to the room too, so it says nothing of the online menu.
const ONLINE_VISIT_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        button(JOIN, 111, "GUI:JoinGame", "STT:LobbyButtonJoin"),
        button(OPTIONS, 138, "GUI:WOLOptions", "STT:WOLLobbyOptions"),
        button(BACK_TO_ROOM, 166, "VERA:BackToRoom", ""),
        button(VIEW, 193, "VERA:LobbyChat", "VERA:LobbyChatHelp"),
    ],
    back: button(BACK, 332, "GUI:Back", ""),
    ..ONLINE_PAGE
};

/// Record `+0xD5`: the retail page's top panel holds the lobby list. The
/// online page shows no list, so it keeps the family's warning monitor
/// there, as the other online pages do.
pub(crate) fn keeps_top_panel(page: &MenuPageSpec) -> bool {
    *page != ONLINE_PAGE && *page != ONLINE_VISIT_PAGE
}

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

/// The rank digits `number<d>.pcx` (`0x00783A90`).
const DIGITS: [&str; 10] = [
    "number0.pcx",
    "number1.pcx",
    "number2.pcx",
    "number3.pcx",
    "number4.pcx",
    "number5.pcx",
    "number6.pcx",
    "number7.pcx",
    "number8.pcx",
    "number9.pcx",
];

/// Every PCX the lobby's children and list columns draw: the image buttons'
/// faces, the list icons, the rank digits, co-op and the badges.
pub(crate) fn pcx_files() -> Vec<&'static str> {
    let faces = IMAGE_BUTTONS.iter().flat_map(|(_, up, down)| [*up, *down]);
    let icons = [
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
    faces
        .chain(icons)
        .chain(DIGITS)
        .chain(["cooperat.pcx"])
        .chain(badge_files())
        .collect()
}

/// List column stops (`0x4A6` at `0x497`): the game list's name column
/// x=0x35 (0x93 wide), type 2, password 0x13, ping 200 and limit 0x24; the
/// player list's name x=0x4E (0xA2 wide), op/voice 2 and clan tag 0xAA.
pub(crate) const GAME_NAME_X: i32 = 0x35;
pub(crate) const GAME_NAME_W: i32 = 0x93;
pub(crate) const GAME_TYPE_X: i32 = 2;
pub(crate) const GAME_PASSWORD_X: i32 = 0x13;
pub(crate) const GAME_LIMIT_X: i32 = 0x24;
pub(crate) const GAME_PING_X: i32 = 200;
pub(crate) const PLAYER_NAME_X: i32 = 0x4E;
pub(crate) const PLAYER_NAME_W: i32 = 0xA2;
pub(crate) const PLAYER_ICON_X: i32 = 2;
pub(crate) const PLAYER_CLAN_X: i32 = 0xAA;
/// The player list's high-score cells: the rank's hundreds, tens and ones
/// digits, the co-op image and the badge; and the ladder rank's text.
pub(crate) const PLAYER_DIGIT_X: [i32; 3] = [0x12, 0x1A, 0x22];
pub(crate) const PLAYER_COOP_X: i32 = 0x2C;
pub(crate) const PLAYER_BADGE_X: i32 = 0x3D;
pub(crate) const PLAYER_RANK_X: i32 = 0xF0;

/// Executed `0x6040B0` help of `0x113`. The lists answer through `0x4E9`
/// instead (`0x00780E30`, `0x00781090`); the app adapter builds those.
pub(crate) fn tooltip_key(control: u16, page: &MenuPageSpec) -> Option<&'static str> {
    if let Some(spec) = page.button(control) {
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
        SEARCH => "VERA:LobbySearchHelp",
        HIDE_FULL => "VERA:HideFullHelp",
        HIDE_LOCKED => "VERA:HideLockedHelp",
        _ => return None,
    })
}

/// The list headings `GUI:OpenGames` and `GUI:Players` (template statics
/// `0xFFFF`), and the modern lobby's right-aligned `GUI:Observe` before the
/// checkbox; the retail dialog has no caption there.
pub(crate) const LABELS: [(u16, &str); 2] = [(0xFF00, "GUI:OpenGames"), (0xFF01, "GUI:Players")];
pub(crate) const OBSERVER_CAPTION: u16 = 0xFF02;

/// `0x113`'s executed relayout (`0x60AAB0 -> 0x60C540 -> 0x60C4A0 ->
/// 0x60C0C0`, `tools/storage_oracle/wol_shell.json`): the body keeps its
/// template windows at every size; heading, lobby list and status line
/// anchor.
#[derive(Debug, Clone)]
pub(crate) struct WolLobbyLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) rects: RectTable,
    /// VERA: what the online page's games view hides of its chat, the log
    /// and the edit, on their chat-view places ([`show_view`]): they keep
    /// their scroll and text across the swap but take no pointer.
    pub(crate) kept: RectTable,
}

impl WolLobbyLayout {
    pub(crate) fn new(width: i32, height: i32, scale: ShellScale, spec: &MenuPageSpec) -> Self {
        let (page, rects) = layout_data::menu_page(spec, "0x113", scale, width, height);
        Self {
            page,
            rects,
            kept: RectTable::default(),
        }
    }

    /// The child's window; a child the page hides has none.
    pub(crate) fn control_rect(&self, id: u16) -> Option<RectPx> {
        self.page.button_rect(id).or_else(|| self.rects.get(id))
    }

    /// The header buttons the page shows, with their windows.
    #[cfg(test)]
    pub(crate) fn sort_buttons(&self) -> impl Iterator<Item = (u16, RectPx)> + '_ {
        SORT_BUTTONS
            .iter()
            .filter_map(|(id, _)| Some((*id, self.rects.get(*id)?)))
    }
}

/// VERA: the online page's body in `view`, `[0x113.Games]` or
/// `[0x113.Chat]` over `[0x113.Modern]`. The games fill it down to the
/// filter row on the chat edit's line; or the chat takes its left and the
/// player list its right, both from under their headings to above the chat
/// edit, and ending together. The lists hold whole rows to the retail chat
/// log's foot, so the pointer never meets a row that is not drawn. What the
/// other view shows has no window; the games view keeps the chat's log and
/// edit on their chat-view places in `kept`.
fn show_view(layout: &mut WolLobbyLayout, view: LobbyView, width: i32, height: i32) {
    let scale = layout.page.scale;
    let row_h = ListMetrics::scaled(scale).row_h;
    let bottom = layout.rects.get(CHAT_LOG).map_or(0, |log| log.y + log.h);
    let whole_rows = |rect: RectPx| RectPx {
        h: 2 + ((bottom - rect.y - 2) / row_h).max(1) * row_h,
        ..rect
    };
    let mut chat = layout.rects.clone();
    layout_data::overlay(&mut chat, "0x113.Chat", scale, width, height);
    if let Some(players) = chat.get(PLAYERS).map(whole_rows) {
        chat.insert(PLAYERS, players);
        if let Some(log) = chat.get(CHAT_LOG) {
            chat.insert(
                CHAT_LOG,
                RectPx {
                    h: players.h,
                    ..log
                },
            );
        }
    }
    match view {
        LobbyView::Games => {
            layout_data::overlay(&mut layout.rects, "0x113.Games", scale, width, height);
            if let Some(games) = layout.rects.get(GAMES) {
                layout.rects.insert(GAMES, whole_rows(games));
            }
            layout.kept = [CHAT_LOG, CHAT]
                .into_iter()
                .filter_map(|id| Some((id, chat.get(id)?)))
                .collect();
        }
        LobbyView::Chat => layout.rects = chat,
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
    /// `"%d"` of the ladder rank, when the ladder table holds the user.
    pub(crate) rank: Option<String>,
    /// The high-score record's images and their x, when the high-score
    /// table holds the user.
    pub(crate) score_icons: Vec<(&'static str, i32)>,
}

/// A high-score record's fields the player list reads (`+0x0C` points,
/// `+0x10` rank, `+0x14` co-op campaigns), compared unsigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlayerScore {
    pub(crate) points: u32,
    pub(crate) rank: u32,
    pub(crate) coop: u32,
}

/// `0x00783B80`'s badges, highest first: the high-score points each starts
/// at, its PCX and VERA's name for it, as the game names none.
const BADGES: [(u32, &str, &str); 10] = [
    (2001, "comchief.pcx", "VERA:LobbyBadgeCommanderInChief"),
    (1501, "stargen.pcx", "VERA:LobbyBadgeFiveStarGeneral"),
    (1001, "general.pcx", "VERA:LobbyBadgeGeneral"),
    (501, "briggenr.pcx", "VERA:LobbyBadgeBrigadierGeneral"),
    (251, "colonel.pcx", "VERA:LobbyBadgeColonel"),
    (151, "major.pcx", "VERA:LobbyBadgeMajor"),
    (81, "lieutena.pcx", "VERA:LobbyBadgeLieutenant"),
    (61, "sergeant.pcx", "VERA:LobbyBadgeSergeant"),
    (32, "corporal.pcx", "VERA:LobbyBadgeCorporal"),
    (0, "private.pcx", "VERA:LobbyBadgePrivate"),
];

/// `0x00783B80`: the badge of the high-score points.
pub(crate) fn badge(points: u32) -> &'static str {
    BADGES
        .iter()
        .find(|(from, ..)| points >= *from)
        .map_or("private.pcx", |(_, file, _)| file)
}

/// Every badge's PCX, highest first.
pub(crate) fn badge_files() -> impl Iterator<Item = &'static str> {
    BADGES.iter().map(|(_, file, _)| *file)
}

/// The text key of the badge drawn from `file`.
pub(crate) fn badge_name_key(file: &str) -> Option<&'static str> {
    BADGES
        .iter()
        .find(|(_, pcx, _)| *pcx == file)
        .map(|(.., key)| *key)
}

/// `0x00783C80`'s high-score cells (`0x007845A8..0x007847F0`): the rank's
/// digits (`0x00783A90`, `number<d>.pcx`) for a rank below 1000, hundreds
/// from 100 and tens from 10; `cooperat.pcx` once the co-op bits hold every
/// one of the `campaigns` CoopCampMD campaigns (`0x0049E4F0`); the badge.
pub(crate) fn score_icons(score: PlayerScore, campaigns: u32) -> Vec<(&'static str, i32)> {
    let rank = score.rank;
    let mut icons = Vec::with_capacity(5);
    if rank < 1000 {
        for (x, (from, digit)) in PLAYER_DIGIT_X.into_iter().zip([
            (100, rank / 100),
            (10, rank / 10 % 10),
            (1, rank % 10),
        ]) {
            if rank >= from {
                icons.push((DIGITS[digit as usize], x));
            }
        }
    }
    let mask = 1u32.wrapping_shl(campaigns).wrapping_sub(1);
    if campaigns > 0 && score.coop & mask == mask {
        icons.push(("cooperat.pcx", PLAYER_COOP_X));
    }
    icons.push((badge(score.points), PLAYER_BADGE_X));
    icons
}

impl WolPlayerRow {
    /// `rank` is the ladder record's rank; `score_icons` come from
    /// [`score_icons`].
    pub(crate) fn from_member(
        member: &WolMember,
        rank: Option<i32>,
        score_icons: Vec<(&'static str, i32)>,
    ) -> Self {
        Self {
            rank: rank.map(|rank| rank.to_string()),
            score_icons,
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
    /// `0x61F`: WOL Options `0x2BC`.
    Options,
    /// `0x5C2`: the page dialog `0xCF` (`0x0078BA3E`).
    Page,
    /// The online page's Quick Match: the Quick Match runner `0x0079FC40`.
    QuickMatch,
    /// VERA: a lobby visitor back to their game room.
    BackToRoom,
    /// VERA: the view button swapped the online page's view.
    View,
    /// VERA: Join Any Game, for the lobby owner to join the first room the
    /// list shows that admits this player.
    JoinAny,
    /// VERA: Hide Full or Hide Locked was clicked; the lobby owner keeps the
    /// choice ([`RoomFilter::toggle`]) and refills the games.
    FilterToggled(u16),
    /// VERA: the room search changed; the games are refilled.
    SearchChanged,
    /// Buddy List, Clan, Tournament and News run dialogs or web pages that
    /// are later chains.
    Unported(u16),
    Popup(WolPopupAction),
}

/// What one input asks of the app, in order: a control's cue comes before
/// what it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolLobbyEffect {
    Cue(PageCue),
    Action(WolLobbyAction),
    /// VERA's player card opened on the player.
    Card(String),
    /// A card button released.
    CardButton(u16),
    /// A row of your card's country list picked.
    CardCountry(usize),
    /// A press outside the card, which closes it.
    CardDismissed,
}

/// VERA: what the online page's body shows. The games fill it, and the view
/// button ([`VIEW`]) swaps them for the chat with the player list. The
/// retail page shows everything at once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum LobbyView {
    #[default]
    Games,
    Chat,
}

/// VERA: the rooms the online page's game list hides, as the player ticked
/// them. The app's lobby owner keeps them for the run; the search is the
/// page's own edit ([`WolLobbyState::search_text`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RoomFilter {
    pub(crate) hide_full: bool,
    pub(crate) hide_locked: bool,
}

impl RoomFilter {
    pub(crate) fn checked(&self, control: u16) -> bool {
        match control {
            HIDE_FULL => self.hide_full,
            HIDE_LOCKED => self.hide_locked,
            _ => false,
        }
    }

    pub(crate) fn toggle(&mut self, control: u16) {
        match control {
            HIDE_FULL => self.hide_full = !self.hide_full,
            HIDE_LOCKED => self.hide_locked = !self.hide_locked,
            _ => {}
        }
    }

    /// Whether the list shows `row`. A full room or match is the greyed row,
    /// a room with a password the keyed one. `search`, already lowercase, is
    /// part of the row's heading or tag.
    pub(crate) fn shows(&self, row: &WolGameRow, search: &str) -> bool {
        let found = |text: &str| text.to_lowercase().contains(search);
        !(self.hide_full && row.greyed)
            && !(self.hide_locked && row.keyed)
            && (search.is_empty() || found(&row.text) || row.tag.as_deref().is_some_and(found))
    }
}

/// VERA: the room search's length limit, in UTF-16 units.
const SEARCH_LIMIT: usize = 32;

/// The dialog's children: list contents, chat transcript, popups and the
/// lobby's widget tree. The tree is the lobby's own: the shell stack resets
/// around the lobby's boxes, pages and Options, and the dialog keeps its
/// lists' selection and scroll and its chat text across them.
pub(crate) struct WolLobbyState {
    modern: bool,
    /// VERA: this player visits the lobby from a game room
    /// ([`Self::set_visiting`]).
    visiting: bool,
    /// VERA: the online page's view; a new lobby runner opens on its games.
    view: LobbyView,
    /// VERA: chat messages that arrived while the games showed. The view
    /// button counts them until the chat shows.
    unread: u32,
    /// The transcript `0x53F` shows; the widget tree owns the chat edit and
    /// VERA's room search.
    pub(crate) chat: LanChatState,
    pub(crate) lobby_rows: Vec<WolLobbyRow>,
    pub(crate) game_rows: Vec<WolGameRow>,
    pub(crate) player_rows: Vec<WolPlayerRow>,
    /// `[0xB77E78]`: the game name the last selection admitted.
    pub(crate) admitted_name: Option<String>,
    /// `[0x84A23C]`: the row index of the last Join (initially -1).
    pub(crate) join_row: Option<usize>,
    pub(crate) popup: Option<WolLobbyPopup>,
    /// VERA's player card over the modern lobby, and its modal dialog.
    pub(crate) card: Option<WolPlayerCard>,
    card_dialog: Option<(GuiDialog, NodeId)>,
    gui: Gui,
    page: GuiDialog,
    scale: ShellScale,
    epoch: Instant,
}

impl std::fmt::Debug for WolLobbyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WolLobbyState")
            .field("modern", &self.modern)
            .finish_non_exhaustive()
    }
}

/// `0x113`'s children in hit order (the template's): the column, the sort
/// and image buttons, the observer checkbox and image (toggled on the press,
/// `OwnerDraw_CheckBox 0x006163A0`), the four owner-draw lists the page
/// paints (the game list's second press is LBN_DBLCLK) and the chat NewEdit;
/// then VERA's filter row, the room search and the Hide Full and Hide Locked
/// checkboxes (their plates toggle, as on every shell checkbox).
fn page_spec() -> NodeSpec {
    let button = |id: u16| NodeSpec::new(NodeKind::Button).id(id.into());
    let checkbox = |id: u16| NodeSpec::new(NodeKind::Checkbox).id(id.into());
    let align = |a: ShellAlign| Value::Int(i32::from(a.0));
    // The transcript and the multi-select user list paint no selected row.
    let list = |id: u16| {
        NodeSpec::new(NodeKind::List)
            .id(id.into())
            .with(Prop::RowCount, Value::Int(0))
            .with(
                Prop::NoSelectionFill,
                Value::Bool(matches!(id, PLAYERS | CHAT_LOG)),
            )
    };
    NodeSpec::new(NodeKind::Dialog)
        .children(
            PAGE.buttons()
                .map(|b| b.id)
                .chain([QUICK_MATCH, BACK_TO_ROOM, VIEW, JOIN_ANY])
                .map(button),
        )
        .children(SORT_BUTTONS.iter().map(|(id, key)| {
            let centre = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
            button(*id).label(key).with(Prop::Align, align(centre))
        }))
        .children([REFRESH, EMOTE].map(button))
        .children([OBSERVER_TOGGLE, OBSERVER_IMAGE].map(checkbox))
        .children([LOBBIES, GAMES, PLAYERS, CHAT_LOG].map(list))
        .child(NodeSpec::new(NodeKind::Edit).id(CHAT.into()))
        .child(
            NodeSpec::new(NodeKind::Edit)
                .id(SEARCH.into())
                .with(Prop::MaxLen, Value::Int(SEARCH_LIMIT as i32)),
        )
        .children(
            [
                (HIDE_FULL, "VERA:HideFull"),
                (HIDE_LOCKED, "VERA:HideLocked"),
            ]
            .map(|(id, key)| checkbox(id).label(key)),
        )
        .children(
            (LABELS.iter().map(|(id, key)| (*id, *key, ShellAlign::NONE)))
                .chain([(OBSERVER_CAPTION, "GUI:Observe", ShellAlign::H_RIGHT)])
                .map(|(id, key, a)| {
                    let text = NodeSpec::new(NodeKind::Text).id(id.into()).label(key);
                    text.with(Prop::Align, align(a))
                }),
        )
}

impl WolLobbyState {
    pub(crate) fn new(observer: bool, modern: bool) -> Self {
        let mut gui = Gui::new(160);
        let (time, width, height) = crate::ui::shell::controller::host_double_click_limits();
        gui.set_double_click(time.as_millis() as u64, width, height);
        let root = gui.create(NodeKind::Dialog, None).expect("an empty tree");
        let page = gui
            .push_dialog(root, DialogKind::Page)
            .expect("a Dialog node");
        page::ensure_built(&mut gui, root, page_spec);
        let mut lobby = Self {
            modern,
            visiting: false,
            view: LobbyView::Games,
            unread: 0,
            chat: LanChatState::default(),
            lobby_rows: Vec::new(),
            game_rows: Vec::new(),
            player_rows: Vec::new(),
            admitted_name: None,
            join_row: None,
            popup: None,
            card: None,
            card_dialog: None,
            gui,
            page,
            scale: crate::ui::shell::geom::RETAIL_SCALE,
            epoch: Instant::now(),
        };
        // Join is created WS_DISABLED; the chat edit holds the focus.
        lobby.set_enabled(JOIN, false);
        // The classic user list is `LBS_MULTIPLESEL`: its selected rows are
        // the private chat targets. VERA's opens a card instead.
        lobby.set(PLAYERS, Prop::MultiSelect, Value::Bool(!modern));
        lobby.set_observer(observer);
        lobby.refocus_chat();
        lobby
    }

    fn now(&self) -> u64 {
        Instant::now()
            .saturating_duration_since(self.epoch)
            .as_millis() as u64
    }

    fn page_root(&self) -> NodeId {
        self.gui
            .dialog_root(self.page)
            .expect("the page dialog stays")
    }

    /// The open dialogs' roots and dialogs, top first: popup, card, page.
    fn dialogs(&self) -> impl Iterator<Item = (GuiDialog, NodeId)> + '_ {
        let popup = self.popup.as_ref().map(|popup| (popup.dialog, popup.root));
        popup
            .into_iter()
            .chain(self.card_dialog)
            .chain([(self.page, self.page_root())])
    }

    /// Control `id`'s node and dialog, top dialog first.
    fn find(&self, id: u16) -> Option<(GuiDialog, NodeId)> {
        self.dialogs()
            .find_map(|(dialog, root)| Some((dialog, self.gui.find_id(root, id.into())?)))
    }

    /// The lobby's widget tree: the page, the card and the popup.
    pub(crate) fn gui(&self) -> &Gui {
        &self.gui
    }

    pub(crate) fn node(&self, id: u16) -> Option<NodeId> {
        self.find(id).map(|(_, node)| node)
    }

    fn set(&mut self, id: u16, prop: Prop, value: Value) {
        if let Some(node) = self.node(id) {
            let _ = self.gui.set(node, prop, value);
        }
    }

    fn int(&self, id: u16, prop: Prop) -> Option<i32> {
        match self.gui.get(self.node(id)?, prop).ok().flatten()? {
            Value::Int(value) => Some(*value),
            _ => None,
        }
    }

    fn flag(&self, id: u16, prop: Prop) -> Option<bool> {
        match self.gui.get(self.node(id)?, prop).ok().flatten()? {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Edit `id` as its NewEdit (caret in UTF-16 units).
    pub(crate) fn edit(&self, id: u16) -> NewEdit {
        self.node(id).map_or_else(Default::default, |node| {
            self.gui.edit_state(node).unwrap_or_default()
        })
    }

    /// Writes `edit`'s text and caret back to edit `id`.
    fn store_edit(&mut self, id: u16, edit: &NewEdit) {
        if let Some(node) = self.node(id) {
            let _ = &mut self.gui.set_edit_state(node, edit);
        }
    }

    pub(crate) fn enabled(&self, control: u16) -> bool {
        self.flag(control, Prop::Enabled) != Some(false)
            && (!self.modern || !LEGACY_CONTROLS.contains(&control))
            && (PAGE.button(control).is_none() || self.page_spec().button(control).is_some())
    }

    pub(crate) fn page_spec(&self) -> &'static MenuPageSpec {
        match (self.modern, self.visiting) {
            (true, true) => &ONLINE_VISIT_PAGE,
            (true, false) => &ONLINE_PAGE,
            (false, _) => &PAGE,
        }
    }

    /// This player visits the lobby from a game room: the page offers Back
    /// to Room.
    pub(crate) fn set_visiting(&mut self, visiting: bool) {
        self.visiting = visiting;
    }

    pub(crate) fn is_modern(&self) -> bool {
        self.modern
    }

    /// VERA: the chat and its edit show: always on the retail page, on the
    /// online page in its chat view.
    pub(crate) fn shows_chat(&self) -> bool {
        !self.modern || self.view == LobbyView::Chat
    }

    /// The page edit that takes the keys: the chat edit, or VERA's room
    /// search while the online page's games show.
    pub(crate) fn view_edit(&self) -> u16 {
        if self.shows_chat() { CHAT } else { SEARCH }
    }

    /// VERA: the room search in lowercase, for [`RoomFilter::shows`].
    pub(crate) fn search_text(&self) -> String {
        String::from_utf16_lossy(&self.edit(SEARCH).units).to_lowercase()
    }

    /// VERA: Hide Full and Hide Locked show the lobby owner's filter.
    pub(crate) fn set_filter(&mut self, filter: RoomFilter) {
        for id in [HIDE_FULL, HIDE_LOCKED] {
            self.set(id, Prop::Checked, Value::Bool(filter.checked(id)));
        }
    }

    /// VERA: a chat message arrived; the view button counts it while the
    /// games show.
    pub(crate) fn chat_arrived(&mut self) {
        if !self.shows_chat() {
            self.unread = self.unread.saturating_add(1);
        }
    }

    /// VERA: the view button's caption names the view it shows next, with
    /// the messages that arrived while the games showed.
    pub(crate) fn view_caption(&self) -> String {
        match self.view {
            LobbyView::Chat => crate::i18n::tr("VERA:LobbyGames"),
            LobbyView::Games if self.unread > 0 => {
                crate::i18n::tr_fill("VERA:LobbyChatUnread", &[("count", &self.unread)])
            }
            LobbyView::Games => crate::i18n::tr("VERA:LobbyChat"),
        }
    }

    /// VERA: the view button's status help.
    pub(crate) fn view_help_key(&self) -> &'static str {
        match self.view {
            LobbyView::Games => "VERA:LobbyChatHelp",
            LobbyView::Chat => "VERA:LobbyGamesHelp",
        }
    }

    /// VERA: the view button shows the other view, whose edit takes the
    /// keys; the chat's count clears once the chat shows.
    fn toggle_view(&mut self) {
        self.view = match self.view {
            LobbyView::Games => LobbyView::Chat,
            LobbyView::Chat => LobbyView::Games,
        };
        if self.view == LobbyView::Chat {
            self.unread = 0;
        }
        // The swap keeps the blink's phase: both edits started it together
        // ([`Self::refocus_chat`]).
        let edit = self.gui.find_id(self.page_root(), self.view_edit().into());
        if self.popup.is_none() && self.card_dialog.is_none() {
            let _ = self.gui.set_focus(edit);
        }
    }

    pub(crate) fn layout(&self, width: i32, height: i32, scale: ShellScale) -> WolLobbyLayout {
        self.layout_of(self.view, width, height, scale)
    }

    /// VERA: the page as its chat view lays it out, whichever view shows:
    /// the chat's messages wrap to that log's width.
    pub(crate) fn chat_layout(&self, width: i32, height: i32, scale: ShellScale) -> WolLobbyLayout {
        self.layout_of(LobbyView::Chat, width, height, scale)
    }

    fn layout_of(
        &self,
        view: LobbyView,
        width: i32,
        height: i32,
        scale: ShellScale,
    ) -> WolLobbyLayout {
        let mut layout = WolLobbyLayout::new(width, height, scale, self.page_spec());
        if self.modern {
            // `[0x113.Modern]`: the legacy columns and the lobby list go, the
            // Observe checkbox ends the headings' row; then the view's own.
            layout_data::overlay(&mut layout.rects, "0x113.Modern", scale, width, height);
            show_view(&mut layout, view, width, height);
        }
        layout
    }

    /// Puts the page's children on `layout` with their row counts; the
    /// lists keep their selection and top within range (`0x4C5`-style
    /// reconcile), and the transcript shows from its own top row.
    pub(crate) fn sync(&mut self, layout: &WolLobbyLayout) {
        self.scale = layout.page.scale;
        self.gui.set_dialog_scale(self.page, self.scale);
        let root = self.page_root();
        page::sync_rects(&mut self.gui, root, |id| layout.control_rect(id));
        if let Some(rect) = layout.control_rect(OBSERVER_IMAGE) {
            self.set(OBSERVER_IMAGE, Prop::HitRect, Value::Rect(rect));
        }
        // VERA: the games view keeps its hidden chat on its own places.
        for (id, rect) in layout.kept.iter() {
            self.set(id, Prop::Rect, Value::Rect(rect));
        }
        // VERA: the chat view's first heading names the chat.
        let heading = match self.modern && self.view == LobbyView::Chat {
            true => "VERA:LobbyChat",
            false => LABELS[0].1,
        };
        self.set(LABELS[0].0, Prop::Label, Value::Text(heading.to_owned()));
        // The transcript's list keeps its view, following new lines.
        if let Some(log) = self.node(CHAT_LOG) {
            let _ = self.gui.log_rows(log, self.chat.lines.len(), 0);
        }
        let counts = [
            (LOBBIES, self.lobby_rows.len()),
            (GAMES, self.game_rows.len()),
            (PLAYERS, self.player_rows.len()),
        ];
        for (id, rows) in counts {
            self.set(id, Prop::RowCount, Value::Int(rows as i32));
            if self
                .int(id, Prop::Selected)
                .is_some_and(|row| row >= rows as i32)
            {
                self.set(id, Prop::Selected, Value::Int(-1));
            }
            let max_top = self.placed_list(id).map_or(0, |geometry| geometry.max_top);
            let top = self
                .int(id, Prop::Top)
                .unwrap_or(0)
                .min(rows.saturating_sub(1) as i32);
            self.set(id, Prop::Top, Value::Int(top.min(max_top as i32).max(0)));
        }
        if let Some(popup) = &self.popup {
            self.gui.set_dialog_scale(popup.dialog, self.scale);
        }
    }

    /// List `id` as it shows and hits, the gui's.
    fn placed_list(&self, id: u16) -> Option<vera20k_gui::shell::list::ShellListGeometry> {
        self.gui
            .list_geometry(self.gui.find_id(self.page_root(), id.into())?)
    }

    /// The row of list `id` under `(x, y)`.
    pub(crate) fn row_at(&self, id: u16, x: i32, y: i32) -> Option<usize> {
        self.gui
            .list_row_at(self.gui.find_id(self.page_root(), id.into())?, x, y)
    }

    /// NewEdit keeps each edit's caret in view (`measure`: text widths in
    /// the authored pixels).
    pub(crate) fn scroll_edits(&mut self, measure: &dyn Fn(&str) -> u32) {
        for id in [CHAT, SEARCH, POPUP_PASSWORD, CREATE_NAME] {
            if let Some(node) = self.node(id) {
                let _ = self.gui.keep_caret_in_view(node, measure);
            }
        }
    }

    /// `SetFocus` on the chat edit `0x540` (VERA: the room search while the
    /// online page's games show): its caret blink restarts. VERA's two edits
    /// share one blink, so the view button's swap keeps its phase.
    pub(crate) fn refocus_chat(&mut self) {
        let now = self.now();
        let root = self.page_root();
        let other = match self.view_edit() {
            CHAT => SEARCH,
            _ => CHAT,
        };
        let [other, edit] = [other, self.view_edit()].map(|id| self.gui.find_id(root, id.into()));
        if self.popup.is_none() && self.card_dialog.is_none() {
            let _ = self.gui.set_focus(None);
            if self.modern {
                let _ = self.gui.focus_edit(other, now);
            }
            let _ = self.gui.focus_edit(edit, now);
        }
    }

    pub(crate) fn set_enabled(&mut self, control: u16, enabled: bool) {
        self.set(control, Prop::Enabled, Value::Bool(enabled));
    }

    /// Every command button on or off, Join on as `join` admits; `lists`
    /// takes the lobby and game lists along. Create/Join turn the commands
    /// off; a chat lobby switch turns the lists off too, and `0x007A5E40`
    /// back on once the lobby joined.
    pub(crate) fn set_commands(&mut self, on: bool, join: bool, lists: bool) {
        let lists = [LOBBIES, GAMES].into_iter().filter(|_| lists);
        for control in lists
            .chain(self.page_spec().buttons().map(|b| b.id))
            .collect::<Vec<_>>()
        {
            self.set_enabled(control, on && (control != JOIN || join));
        }
    }

    /// The chat edit's blink is in its shown half (`0x540` holds the focus
    /// throughout; its phase restarts only on `refocus_chat`).
    fn chat_blink(&self) -> bool {
        let chat = self.gui.find_id(self.page_root(), self.view_edit().into());
        chat.is_some_and(|chat| self.gui.caret_visible_in(self.page, chat, self.now()))
    }

    /// The chat caret shows: no popup, and the blink's shown half.
    pub(crate) fn caret_visible(&self) -> bool {
        self.popup.is_none() && self.chat_blink()
    }

    /// The popup edit `id`'s caret shows: it has the popup's focus, on the
    /// chat edit's blink phase (the popups never restart it).
    pub(crate) fn popup_caret_visible(&self, id: u16) -> bool {
        self.node(id)
            .is_some_and(|node| self.gui.focus() == Some(node))
            && self.chat_blink()
    }

    /// Button `id` paints pushed: captured and under the pointer in its
    /// own dialog (612B70).
    pub(crate) fn pressed(&self, id: u16) -> bool {
        self.find(id).is_some_and(|(dialog, node)| {
            self.gui.pressed_in(dialog) == Some(node) && self.gui.hovered_in(dialog) == Some(node)
        })
    }

    /// The control under the pointer in the top dialog.
    pub(crate) fn hovered(&self) -> Option<u16> {
        self.gui
            .id_of(self.gui.hovered()?)
            .and_then(|id| u16::try_from(id).ok())
    }

    /// The pointer left the window, a box took it or the page slides out:
    /// every capture and hover ends (a held scroll arrow lets go).
    pub(crate) fn release_buttons(&mut self) {
        let up = InputEvent::PointerUp {
            x: i32::MIN / 2,
            y: i32::MIN / 2,
            button: vera20k_gui::MouseButton::Left,
        };
        self.gui.input(up, self.now());
        self.gui.cancel_pointer();
        self.gui.drain_events();
    }

    /// `[0xAC10C8]`'s view; the multiplayer preference owner keeps it.
    pub(crate) fn observer(&self) -> bool {
        self.flag(OBSERVER_TOGGLE, Prop::Checked) == Some(true)
    }

    pub(crate) fn set_observer(&mut self, observer: bool) {
        for id in [OBSERVER_TOGGLE, OBSERVER_IMAGE] {
            self.set(id, Prop::Checked, Value::Bool(observer));
        }
    }

    /// List `id`'s first shown row (the transcript's follows new lines).
    pub(crate) fn list_top(&self, id: u16) -> usize {
        self.top(id)
    }

    /// Adds a line to the transcript `0x53F`, wrapped to `width`; its list
    /// keeps the view ([`vera20k_gui::Gui::log_rows`]).
    pub(crate) fn append_chat(
        &mut self,
        line: crate::ui::lan_shell::LanChatLine,
        width: i32,
        measure: impl Fn(&[u16]) -> i32,
    ) {
        let log = self.node(CHAT_LOG);
        let log = crate::ui::lan_shell::log_to(&mut self.gui, log);
        self.chat.append_message(line, width, measure, log);
    }

    /// List `id` in `rect` with `count` rows, as it shows.
    pub(crate) fn shown_geometry(
        &self,
        id: u16,
        rect: RectPx,
        count: usize,
        scale: ShellScale,
    ) -> vera20k_gui::shell::list::ShellListGeometry {
        vera20k_gui::shell::list::ShellListGeometry::new(
            rect,
            count,
            self.list_top(id),
            ListMetrics::scaled(scale),
        )
    }

    pub(crate) fn selected_row(&self, list: u16) -> Option<usize> {
        usize::try_from(self.int(list, Prop::Selected)?).ok()
    }

    pub(crate) fn select_row(&mut self, list: u16, row: Option<usize>) {
        self.set(
            list,
            Prop::Selected,
            Value::Int(row.map_or(-1, |row| row as i32)),
        );
    }

    fn top(&self, list: u16) -> usize {
        self.int(list, Prop::Top).unwrap_or(0).max(0) as usize
    }

    pub(crate) fn selected_game(&self) -> Option<&WolGameRow> {
        self.selected_row(GAMES)
            .and_then(|row| self.game_rows.get(row))
    }

    /// `0x00781480`: the rows replaced, the selection and the top row found
    /// again by name (`0x4C5`), or cleared.
    pub(crate) fn replace_games(&mut self, rows: Vec<WolGameRow>) {
        let selected = self.selected_game().map(|row| row.name.clone());
        let top = self
            .game_rows
            .get(self.top(GAMES))
            .map(|row| row.name.clone());
        self.game_rows = rows;
        let find = |name: Option<String>| {
            name.and_then(|name| self.game_rows.iter().position(|row| row.name == name))
        };
        let (selected, top) = (find(selected), find(top));
        self.select_row(GAMES, selected);
        self.set(
            GAMES,
            Prop::RowCount,
            Value::Int(self.game_rows.len() as i32),
        );
        if let Some(top) = top {
            self.set(GAMES, Prop::Top, Value::Int(top as i32));
        }
    }

    /// `0x007806C0`: the current channel's row is selected.
    pub(crate) fn replace_lobbies(&mut self, rows: Vec<WolLobbyRow>, current: Option<&str>) {
        let top = self
            .lobby_rows
            .get(self.top(LOBBIES))
            .map(|row| row.name.clone());
        self.lobby_rows = rows;
        let selected =
            current.and_then(|current| self.lobby_rows.iter().position(|row| row.name == current));
        self.select_row(LOBBIES, selected);
        self.set(
            LOBBIES,
            Prop::RowCount,
            Value::Int(self.lobby_rows.len() as i32),
        );
        if let Some(top) =
            top.and_then(|top| self.lobby_rows.iter().position(|row| row.name == top))
        {
            self.set(LOBBIES, Prop::Top, Value::Int(top as i32));
        }
    }

    /// WOL `0x00783C80` saves selected names before `LB_RESETCONTENT` and
    /// restores every matching new row afterward (`0x005E5F10` / `0x005E6230`).
    pub(crate) fn replace_players(&mut self, rows: Vec<WolPlayerRow>) {
        let mut selected_rows = self.selected_players();
        selected_rows.sort_unstable();
        selected_rows.dedup();
        let selected_names = selected_rows
            .into_iter()
            .filter_map(|index| self.player_rows.get(index))
            .map(|row| row.name.clone())
            .collect::<Vec<_>>();
        self.player_rows = rows;
        let selected: Vec<usize> = self
            .player_rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                selected_names
                    .iter()
                    .any(|name| saved_player_name_matches(name, &row.name, PlayerNameMatch::Exact))
                    .then_some(index)
            })
            .collect();
        if let Some(list) = self.node(PLAYERS) {
            let _ = self.gui.set_selected_rows(list, &selected);
        }
        // A player card closes once its player has left the list.
        if self
            .card
            .as_ref()
            .is_some_and(|card| !self.player_rows.iter().any(|row| row.name == card.name))
        {
            self.close_card();
        }
    }

    /// The selected players' names (`LB_GETSELITEMS`, at most 32).
    pub(crate) fn private_targets(&self) -> Vec<String> {
        let mut rows = self.selected_players();
        rows.sort_unstable();
        rows.into_iter()
            .filter_map(|row| self.player_rows.get(row))
            .take(32)
            .map(|row| row.name.clone())
            .collect()
    }

    /// The classic user list's selected rows (`LBS_MULTIPLESEL`), in the
    /// order they were selected.
    pub(crate) fn selected_players(&self) -> Vec<usize> {
        self.node(PLAYERS)
            .map_or_else(Vec::new, |list| self.gui.selected_rows(list))
    }

    /// A press on a player row: the classic page's list toggled its
    /// selection itself (`LBS_MULTIPLESEL`); VERA's lobby opens the player's
    /// card instead and returns the name.
    pub(crate) fn press_player(&mut self, row: usize) -> Option<String> {
        if !self.modern {
            return None;
        }
        let name = self.player_rows.get(row)?.name.clone();
        self.close_card();
        if let Ok(root) = self.gui.create_tree(&player_card::card_spec(), None)
            && let Ok(dialog) = self.gui.push_dialog(root, DialogKind::Modal)
        {
            self.gui.set_dialog_scale(dialog, self.scale);
            self.card_dialog = Some((dialog, root));
        }
        self.card = Some(WolPlayerCard {
            name: name.clone(),
            countries: Vec::new(),
        });
        Some(name)
    }

    pub(crate) fn close_card(&mut self) {
        self.card = None;
        if let Some((dialog, _)) = self.card_dialog.take() {
            let _ = self.gui.pop_dialog(dialog);
        }
    }

    /// A released button or picture.
    pub(crate) fn activate(&mut self, control: u16) -> Option<WolLobbyAction> {
        if !self.enabled(control) {
            return None;
        }
        Some(match control {
            CREATE => WolLobbyAction::Create,
            JOIN => WolLobbyAction::Join,
            BACK_TO_ROOM => WolLobbyAction::BackToRoom,
            // VERA: Back steps back one page, so on the online page's chat
            // it returns to the games; only from there does it leave.
            BACK if self.modern && self.view == LobbyView::Chat => {
                self.toggle_view();
                WolLobbyAction::View
            }
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
                let mut edit = self.edit(CHAT);
                apply_chat_length_limit(&mut edit);
                let text = crate::ui::wol_chat::trim_line_end(&edit.wide_text(111)).to_vec();
                self.store_edit(CHAT, &NewEdit::default());
                WolLobbyAction::Emote(text)
            }
            OPTIONS => WolLobbyAction::Options,
            PAGE_BUTTON => WolLobbyAction::Page,
            QUICK_MATCH if self.modern => WolLobbyAction::QuickMatch,
            VIEW if self.modern => {
                self.toggle_view();
                WolLobbyAction::View
            }
            JOIN_ANY if self.page_spec().button(JOIN_ANY).is_some() => WolLobbyAction::JoinAny,
            HIDE_FULL | HIDE_LOCKED if self.modern => WolLobbyAction::FilterToggled(control),
            BUDDY | CLAN | TOURNAMENT | NEWS => WolLobbyAction::Unported(control),
            _ => return None,
        })
    }

    /// The pointer at `(x, y)` in shell pixels: moved there, pressed or
    /// released. Down and up move the hover first, as the window system
    /// sends a move before them.
    pub(crate) fn pointer(&mut self, x: i32, y: i32, press: Option<bool>) -> Vec<WolLobbyEffect> {
        use vera20k_gui::MouseButton::Left;
        let now = self.now();
        self.gui.input(InputEvent::PointerMove { x, y }, now);
        let mut out = Vec::new();
        let over = self.hovered();
        match press {
            None => {}
            Some(true) => {
                // A row of a disabled list still clicks (0x0078AC10 plays
                // GenericClick before the selection refuses it).
                if let Some(list @ (LOBBIES | GAMES)) =
                    over.filter(|_| self.popup.is_none() && self.card.is_none())
                    && !self.enabled(list)
                    && self.row_at(list, x, y).is_some()
                {
                    out.push(WolLobbyEffect::Cue(PageCue::ListClick));
                }
                // A press on an online setting's title gives its edit the
                // keys: the title's `FocusTarget` ([`popups`]).
                self.gui
                    .input(InputEvent::PointerDown { x, y, button: Left }, now);
            }
            Some(false) => {
                self.gui
                    .input(InputEvent::PointerUp { x, y, button: Left }, now);
            }
        }
        self.drain(&mut out);
        out
    }

    /// A key (`Escape` excluded: the NewEdits consume it and `0x113` has no
    /// IDCANCEL case) or typed text: the popup's focused edit takes it, else
    /// the chat NewEdit, which holds the focus while it shows; VERA's room
    /// search holds it while the online page's games show.
    pub(crate) fn key(
        &mut self,
        key: Option<vera20k_gui::Key>,
        text: Option<&str>,
    ) -> Vec<WolLobbyEffect> {
        use vera20k_gui::Key;
        let mut out = Vec::new();
        let popup = self.popup.is_some();
        let event = match (key, text) {
            // The player card's widgets take Escape.
            (Some(Key::Escape), _) if popup || self.card_dialog.is_none() => return out,
            // The chat edit ignores Tab; in VERA's popup it moves between
            // the Room name and the password.
            (Some(Key::Tab | Key::BackTab), _) if !popup => return out,
            (Some(key), _) => InputEvent::Key(key),
            (None, Some(text)) if !popup => {
                // Each typed character's EN_CHANGE caps the chat edit
                // (`TextChanged`, below).
                for input in crate::ui::shell::controller::gui_inputs(None, false, Some(text)) {
                    let now = self.now();
                    self.gui.input(input, now);
                    self.drain(&mut out);
                }
                return out;
            }
            (None, Some(text)) => InputEvent::Text(text.to_owned()),
            (None, None) => return out,
        };
        let now = self.now();
        self.gui.input(event, now);
        self.drain(&mut out);
        out
    }

    /// The widgets' timers (a held scroll arrow's repeat) while `0x113` is
    /// the current window; the next deadline.
    pub(crate) fn poll(&mut self) -> Option<Instant> {
        let next = self.gui.poll(self.now());
        let mut out = Vec::new();
        self.drain(&mut out);
        next.map(|ms| self.epoch + std::time::Duration::from_millis(ms))
    }

    /// The widgets' events as effects, in order.
    fn drain(&mut self, out: &mut Vec<WolLobbyEffect>) {
        let raw = self.gui.drain_events();
        let card = self.card_dialog.map(|(dialog, _)| dialog);
        if raw.iter().any(|event| {
            matches!(event, vera20k_gui::UiEvent::PressedOutside { dialog } if Some(*dialog) == card)
        }) {
            out.push(WolLobbyEffect::CardDismissed);
        }
        for event in page::page_events(&self.gui, raw) {
            let popup = self.popup.is_some();
            match event {
                // The lists' GenericClick is the page's: the transcript has none.
                PageEvent::Cue(PageCue::ListClick) => {}
                PageEvent::Cue(cue) => out.push(WolLobbyEffect::Cue(cue)),
                PageEvent::Activated(id) if popup => out.extend(
                    self.popup_activate(id)
                        .map(|a| WolLobbyEffect::Action(WolLobbyAction::Popup(a))),
                ),
                PageEvent::Activated(id) if self.card_dialog.is_some() => {
                    out.push(WolLobbyEffect::CardButton(id))
                }
                PageEvent::Picked(CARD_COUNTRIES, row) => {
                    out.extend(usize::try_from(row).ok().map(WolLobbyEffect::CardCountry))
                }
                PageEvent::Activated(id) => {
                    out.extend(self.activate(id).map(WolLobbyEffect::Action))
                }
                PageEvent::Toggled(OBSERVER_TOGGLE | OBSERVER_IMAGE, on) if !popup => {
                    self.set_observer(on);
                    out.push(WolLobbyEffect::Action(WolLobbyAction::ObserverChanged(on)));
                }
                PageEvent::Toggled(CREATE_WATCHING, _) => self.step_watching(),
                // VERA: the lobby owner keeps the filter ([`Self::set_filter`]).
                PageEvent::Toggled(id @ (HIDE_FULL | HIDE_LOCKED), _) if !popup => {
                    out.extend(self.activate(id).map(WolLobbyEffect::Action))
                }
                PageEvent::TextChanged(SEARCH, _) if !popup => {
                    out.push(WolLobbyEffect::Action(WolLobbyAction::SearchChanged))
                }
                PageEvent::Selected(list @ (GAMES | LOBBIES), row)
                | PageEvent::DoubleClicked(list @ GAMES, row) => {
                    let row = row as usize;
                    out.push(WolLobbyEffect::Cue(PageCue::ListClick));
                    let rows = match list {
                        GAMES => self.game_rows.len(),
                        _ => self.lobby_rows.len(),
                    };
                    if self.enabled(list) && row < rows {
                        out.push(WolLobbyEffect::Action(match list {
                            GAMES => WolLobbyAction::SelectGame {
                                row,
                                double: matches!(event, PageEvent::DoubleClicked(..)),
                            },
                            _ => WolLobbyAction::SelectLobby(row),
                        }));
                    }
                }
                PageEvent::Selected(PLAYERS, row) => {
                    out.push(WolLobbyEffect::Cue(PageCue::ListClick));
                    out.extend(self.press_player(row as usize).map(WolLobbyEffect::Card));
                }
                // 0x540's EN_CHANGE caps its text (0x005EDD00's limit).
                PageEvent::TextChanged(CHAT, _) => {
                    if let Some(chat) = self.node(CHAT) {
                        crate::ui::lan_shell::cap_chat_edit(&mut self.gui, chat);
                    }
                }
                PageEvent::Submitted(CHAT, _) => {
                    let sent = self.node(CHAT).and_then(|chat| {
                        crate::ui::lan_shell::submit_chat_edit(&mut self.gui, chat)
                    });
                    let text = sent.map(|sent| crate::ui::wol_chat::trim_line_end(&sent).to_vec());
                    if let Some(text) = text.filter(|text| !text.is_empty()) {
                        out.push(WolLobbyEffect::Action(WolLobbyAction::Chat(text)));
                    }
                }
                _ => {}
            }
        }
        self.gate_popup_ok();
    }
}

#[cfg(all(test, tests_ui))]
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

    /// The child the pointer at `(x, y)` is over, on `layout`.
    fn control_at(
        lobby: &mut WolLobbyState,
        layout: &WolLobbyLayout,
        x: i32,
        y: i32,
    ) -> Option<u16> {
        lobby.sync(layout);
        lobby.pointer(x, y, None);
        lobby.hovered()
    }

    #[test]
    fn layout_and_help_follow_the_executed_0x113_table() {
        let corpus = corpus();
        let mut checked = 0;
        for (size, case) in corpus["layouts"]["0x113"]["sizes"].as_object().unwrap() {
            let (width, height) = size.split_once('x').unwrap();
            let layout = WolLobbyLayout::new(
                width.parse().unwrap(),
                height.parse().unwrap(),
                crate::ui::shell::geom::RETAIL_SCALE,
                &PAGE,
            );
            for (key, child) in case["controls"].as_object().unwrap() {
                let mut native = rect(&child["rect"]);
                let actual = match key.as_str() {
                    "0xFFFF" => layout.control_rect(LABELS[0].0).unwrap(),
                    "0xFFFF#1" => layout.control_rect(LABELS[1].0).unwrap(),
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
            assert_eq!(tooltip_key(id, &PAGE), help.as_str(), "{key}");
        }
    }

    #[test]
    fn create_and_join_disable_the_command_column_and_join_starts_disabled() {
        let mut lobby = WolLobbyState::new(false, false);
        assert!(!lobby.enabled(JOIN) && lobby.enabled(CREATE));
        assert_eq!(lobby.activate(JOIN), None);
        lobby.set_commands(false, false, false);
        assert_eq!(lobby.activate(BACK), None);
        lobby.set_commands(true, false, false);
        assert_eq!(lobby.activate(BACK), Some(WolLobbyAction::Back));
        assert!(!lobby.enabled(JOIN));
        lobby.set_commands(false, false, true);
        assert!(!lobby.enabled(GAMES) && !lobby.enabled(LOBBIES));
        lobby.set_commands(true, true, true);
        assert!(lobby.enabled(JOIN) && lobby.enabled(GAMES));
    }

    #[test]
    fn online_sidebar_keeps_core_actions_and_never_reenables_removed_controls() {
        let mut lobby = WolLobbyState::new(false, true);
        let layout = WolLobbyLayout::new(
            800,
            600,
            crate::ui::shell::geom::RETAIL_SCALE,
            lobby.page_spec(),
        );
        let controls: Vec<_> = layout.page.buttons.iter().map(|button| button.id).collect();
        assert_eq!(
            controls,
            [CREATE, JOIN, JOIN_ANY, QUICK_MATCH, OPTIONS, VIEW, BACK]
        );
        let retail = WolLobbyLayout::new(800, 600, crate::ui::shell::geom::RETAIL_SCALE, &PAGE);
        assert_eq!(layout.control_rect(JOIN_ANY), retail.control_rect(BUDDY));
        assert_eq!(layout.control_rect(QUICK_MATCH), retail.control_rect(CLAN));
        assert_eq!(
            layout.control_rect(OPTIONS),
            retail.control_rect(TOURNAMENT)
        );
        assert_eq!(layout.control_rect(VIEW), retail.control_rect(NEWS));
        assert_eq!(
            tooltip_key(QUICK_MATCH, lobby.page_spec()),
            Some("STT:WOLWelcomeQuickMatch")
        );
        // Only the online page offers them; the retail page has its welcome.
        assert_eq!(WolLobbyState::new(false, false).activate(QUICK_MATCH), None);
        assert_eq!(WolLobbyState::new(false, false).activate(JOIN_ANY), None);
        let mut visitor = WolLobbyState::new(false, true);
        visitor.set_visiting(true);
        assert_eq!(visitor.activate(JOIN_ANY), None);
        assert_eq!(retail.control_rect(QUICK_MATCH), None);
        lobby.set_commands(false, false, false);
        lobby.set_commands(true, true, false);
        lobby.set_commands(false, false, true);
        lobby.set_commands(true, true, true);
        for control in [BUDDY, CLAN, TOURNAMENT, NEWS, PAGE_BUTTON] {
            assert!(!lobby.enabled(control));
            assert_eq!(lobby.activate(control), None);
            assert_eq!(layout.control_rect(control), None);
            assert_eq!(tooltip_key(control, lobby.page_spec()), None);
        }
        assert_eq!(lobby.activate(OPTIONS), Some(WolLobbyAction::Options));
        assert_eq!(
            lobby.activate(QUICK_MATCH),
            Some(WolLobbyAction::QuickMatch)
        );
    }

    #[test]
    fn modern_body_keeps_useful_controls_and_hides_legacy_ones() {
        let mut lobby = WolLobbyState::new(false, true);
        let layout = lobby.layout(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        let legacy =
            WolLobbyState::new(false, false).layout(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        let headers: Vec<_> = layout.sort_buttons().map(|(id, _)| id).collect();
        assert_eq!(headers, [SORT_ALPHA, SORT_PING]);
        for id in [
            SORT_GAMETYPE,
            SORT_PASSWORD,
            SORT_RESLOCK,
            PLAYER_SORT_CLAN,
            PLAYER_SORT_RANK,
            LOBBIES,
        ] {
            let old = legacy.control_rect(id).expect("legacy control");
            assert_eq!(layout.control_rect(id), None);
            assert_ne!(
                control_at(&mut lobby, &layout, old.x + old.w / 2, old.y + old.h / 2),
                Some(id)
            );
            lobby.set_enabled(id, true);
            assert!(!lobby.enabled(id));
            assert_eq!(lobby.activate(id), None);
        }
        lobby.activate(VIEW);
        let chat = lobby.layout(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        for (layout, ids) in [
            (
                &layout,
                &[SORT_ALPHA, SORT_PING, REFRESH, GAMES, OBSERVER_TOGGLE][..],
            ),
            (&chat, &[EMOTE, PLAYERS, CHAT_LOG, CHAT][..]),
        ] {
            for &id in ids {
                let rect = layout.control_rect(id).expect("useful control");
                assert_eq!(
                    control_at(&mut lobby, layout, rect.x + rect.w / 2, rect.y + rect.h / 2),
                    Some(id)
                );
            }
        }
    }

    /// The online page's games run on where its chat was; the view button
    /// swaps them for the chat and the player list, and back.
    #[test]
    fn the_view_button_swaps_the_full_games_list_for_the_chat_and_players() {
        let scale = crate::ui::shell::geom::RETAIL_SCALE;
        let mut ui = WolLobbyState::new(false, true);
        let channels = (1..=40)
            .map(|n| WolChannel {
                name: format!("Game {n}"),
                max_users: 8,
                ..Default::default()
            })
            .collect::<Vec<_>>();
        ui.replace_games(game_rows(&channels, 0, false, true));
        let classic = WolLobbyState::new(false, false).layout(800, 600, scale);
        let log = classic.control_rect(CHAT_LOG).unwrap();
        let (x, y) = (log.x + 20, log.y + log.h / 2);
        let games = ui.layout(800, 600, scale);
        assert_eq!(control_at(&mut ui, &games, x, y), Some(GAMES));
        assert!(ui.row_at(GAMES, x, y).is_some());
        for id in [PLAYERS, PLAYER_SORT_ALPHA, CHAT_LOG, CHAT, EMOTE] {
            assert_eq!(games.control_rect(id), None);
        }
        assert_eq!(ui.activate(VIEW), Some(WolLobbyAction::View));
        let chat = ui.layout(800, 600, scale);
        assert_eq!(control_at(&mut ui, &chat, x, y), Some(CHAT_LOG));
        // Neither box has a header: both start on the games' header row and
        // end together.
        let span = |rect: RectPx| (rect.y, rect.y + rect.h);
        let (log, players) = (
            chat.control_rect(CHAT_LOG).unwrap(),
            chat.control_rect(PLAYERS).unwrap(),
        );
        assert_eq!(span(log), span(players));
        let header = games.control_rect(SORT_ALPHA).expect("games header");
        assert_eq!(log.y, header.y);
        // The games view kept the chat's log where the chat view puts it.
        assert_eq!(games.kept.get(CHAT_LOG), Some(log));
        for id in [
            GAMES,
            SORT_ALPHA,
            SORT_PING,
            PLAYER_SORT_ALPHA,
            REFRESH,
            OBSERVER_TOGGLE,
        ] {
            assert_eq!(chat.control_rect(id), None);
        }
        assert_eq!(ui.activate(VIEW), Some(WolLobbyAction::View));
        let games = ui.layout(800, 600, scale);
        assert_eq!(control_at(&mut ui, &games, x, y), Some(GAMES));
    }

    /// VERA's filter row: the games end above it, and while they show its
    /// search takes the keys. Hide Full and Hide Locked hide the greyed and
    /// the keyed rows; the search keeps the rows whose heading or tag has it.
    #[test]
    fn the_filter_row_hides_full_locked_and_unmatched_rooms() {
        use vera20k_gui::Key;
        let mut ui = WolLobbyState::new(false, true);
        let games = ui.layout(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        let search = games.control_rect(SEARCH).expect("search");
        let list = games.control_rect(GAMES).expect("games");
        assert!(list.y + list.h <= search.y);
        for id in [SEARCH, HIDE_FULL, HIDE_LOCKED] {
            let rect = games.control_rect(id).expect("filter control");
            assert_eq!(
                control_at(&mut ui, &games, rect.x + 4, rect.y + rect.h / 2),
                Some(id)
            );
        }
        assert_eq!(
            ui.key(None, Some("2V2")),
            vec![WolLobbyEffect::Action(WolLobbyAction::SearchChanged); 3]
        );
        assert_eq!(ui.search_text(), "2v2");
        assert!(ui.edit(CHAT).units.is_empty());
        assert!(ui.key(Some(Key::Enter), None).is_empty());
        assert_eq!(
            ui.activate(HIDE_FULL),
            Some(WolLobbyAction::FilterToggled(HIDE_FULL))
        );

        let row = |text: &str, greyed, keyed, tag: Option<&str>| WolGameRow {
            record: 0,
            name: text.to_owned(),
            text: text.to_owned(),
            greyed,
            kind: GameTypeIcon::Plain,
            keyed,
            limit_icon: None,
            ping: 0,
            tag: tag.map(str::to_owned),
            watch: false,
        };
        let full = row("Full - 2v2", true, false, None);
        let locked = row("Keyed - 2v2", false, true, None);
        let open = row("Open - FFA", false, false, Some("Co-op"));
        let mut filter = RoomFilter::default();
        assert!(
            [&full, &locked, &open]
                .iter()
                .all(|row| filter.shows(row, ""))
        );
        filter.toggle(HIDE_FULL);
        filter.toggle(HIDE_LOCKED);
        assert!(!filter.shows(&full, "") && !filter.shows(&locked, ""));
        assert!(filter.shows(&open, "co-op") && filter.shows(&open, "ffa"));
        assert!(!filter.shows(&open, "2v2"));

        // The chat view has no filter row, and its edit takes the keys.
        ui.activate(VIEW);
        let chat = ui.layout(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        assert_eq!(chat.control_rect(SEARCH), None);
        ui.sync(&chat);
        assert!(ui.key(None, Some("hi")).is_empty());
        assert_eq!(ui.search_text(), "2v2");
        assert_eq!(String::from_utf16_lossy(&ui.edit(CHAT).units), "hi");
    }

    /// The online page's chat takes keys only while it shows, and the view
    /// button counts the messages that arrive while the games show.
    #[test]
    fn a_hidden_chat_takes_no_keys_and_the_view_button_counts_its_messages() {
        use vera20k_gui::Key;
        let mut lobby = WolLobbyState::new(false, true);
        lobby.key(None, Some("hello"));
        assert!(lobby.key(Some(Key::Enter), None).is_empty());
        assert!(lobby.edit(CHAT).units.is_empty());
        lobby.chat_arrived();
        lobby.chat_arrived();
        assert_eq!(
            lobby.view_caption(),
            crate::i18n::tr_fill("VERA:LobbyChatUnread", &[("count", &2)])
        );
        lobby.activate(VIEW);
        assert_eq!(lobby.view_caption(), crate::i18n::tr("VERA:LobbyGames"));
        lobby.chat_arrived();
        lobby.key(None, Some("hello"));
        assert_eq!(
            lobby.key(Some(Key::Enter), None),
            [WolLobbyEffect::Action(WolLobbyAction::Chat(
                "hello".encode_utf16().collect()
            ))]
        );
        lobby.activate(VIEW);
        assert_eq!(lobby.view_caption(), crate::i18n::tr("VERA:LobbyChat"));
    }

    /// Back steps back one page: on the online page's chat it returns to
    /// the games, and only from there does it leave the lobby.
    #[test]
    fn back_on_the_chat_page_returns_to_the_games() {
        let mut lobby = WolLobbyState::new(false, true);
        lobby.activate(VIEW);
        assert_eq!(lobby.activate(BACK), Some(WolLobbyAction::View));
        assert!(!lobby.shows_chat());
        assert_eq!(lobby.activate(BACK), Some(WolLobbyAction::Back));
    }

    /// The modern Observe checkbox ends the games' heading row, flush with
    /// the games list, with its caption between the two; the retail eye
    /// image's place left of it takes no clicks.
    #[test]
    fn modern_observe_checkbox_ends_the_games_heading_row() {
        let mut lobby = WolLobbyState::new(false, true);
        let layout = lobby.layout(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        let legacy =
            WolLobbyState::new(false, false).layout(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        let rect = |id| layout.control_rect(id).unwrap();
        let (heading, toggle, games) = (rect(LABELS[0].0), rect(OBSERVER_TOGGLE), rect(GAMES));
        let plate = crate::ui::skirmish_shell::checkbox_icon_rect(toggle, layout.page.scale);
        assert_eq!(plate, toggle);
        assert_eq!(plate.y, heading.y);
        assert_eq!(plate.x + plate.w, games.x + games.w);
        let caption = rect(OBSERVER_CAPTION);
        assert_eq!(caption.y, heading.y);
        assert!(heading.x + heading.w <= caption.x && caption.x + caption.w < plate.x);
        let eye = legacy.control_rect(OBSERVER_IMAGE).unwrap();
        assert_eq!(
            control_at(&mut lobby, &layout, eye.x + 2, eye.y + eye.h / 2),
            None
        );
        assert_eq!(legacy.control_rect(OBSERVER_CAPTION), None);
    }

    #[test]
    fn refills_find_game_and_private_player_selections_by_name() {
        let row = |name: &str| WolGameRow {
            record: 0,
            name: name.into(),
            text: name.into(),
            greyed: false,
            kind: GameTypeIcon::Plain,
            keyed: false,
            limit_icon: None,
            ping: -1,
            tag: None,
            watch: false,
        };
        let mut lobby = WolLobbyState::new(false, false);
        lobby.replace_games(vec![row("a"), row("b")]);
        lobby.select_row(GAMES, Some(1));
        lobby.replace_games(vec![row("c"), row("a"), row("b")]);
        assert_eq!(lobby.selected_row(GAMES), Some(2));
        lobby.replace_games(vec![row("a")]);
        assert_eq!(lobby.selected_row(GAMES), None);

        // A pointer resting on the second row finds it across a refill.
        let layout = lobby.layout(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        let games = layout.control_rect(GAMES).unwrap();
        let list = lobby.shown_geometry(GAMES, games, 2, layout.page.scale);
        let (x, y) = (list.content.x + 1, list.content.y + list.metrics.row_h + 1);
        lobby.replace_games(vec![row("a"), row("b")]);
        lobby.sync(&layout);
        assert_eq!(lobby.row_at(GAMES, x, y), Some(1));

        let player = |name: &str| WolPlayerRow {
            name: name.into(),
            icon: None,
            clan: None,
            rank: None,
            score_icons: Vec::new(),
        };
        lobby.replace_players(vec![player("x"), player("y"), player("z")]);
        // Presses on the third row, then the first.
        let list = lobby.node(PLAYERS).unwrap();
        lobby.gui.set_selected_rows(list, &[2, 0]).unwrap();
        assert_eq!(lobby.press_player(0), None);
        assert_eq!(lobby.private_targets(), ["x", "z"]);
        lobby.replace_players(vec![
            player("z"),
            player("x"),
            player("x"),
            player("X"),
            player("y"),
        ]);
        assert_eq!(lobby.selected_players(), [0, 1, 2]);
        assert_eq!(lobby.private_targets(), ["z", "x", "x"]);

        lobby.replace_players(vec![player("x"), player("y")]);
        assert_eq!(lobby.selected_players(), [0]);
        assert_eq!(lobby.private_targets(), ["x"]);

        lobby.replace_players(vec![player("y")]);
        assert!(lobby.private_targets().is_empty());
    }

    /// VERA's lobby opens a player's card where the classic page selects the
    /// name for private chat; the card closes once its player has left.
    #[test]
    fn a_modern_press_opens_the_players_card_until_they_leave() {
        let player = |name: &str| WolPlayerRow {
            name: name.into(),
            icon: None,
            clan: None,
            rank: None,
            score_icons: Vec::new(),
        };
        let mut lobby = WolLobbyState::new(false, true);
        lobby.replace_players(vec![player("Tanya"), player("Boris")]);
        assert_eq!(lobby.press_player(1).as_deref(), Some("Boris"));
        assert!(lobby.private_targets().is_empty());
        lobby.replace_players(vec![player("Boris"), player("Natasha")]);
        assert_eq!(
            lobby.card.as_ref().map(|card| card.name.as_str()),
            Some("Boris")
        );
        lobby.replace_players(vec![player("Natasha")]);
        assert!(lobby.card.is_none());
    }

    #[test]
    fn chat_enter_and_emote_read_and_clear_the_edit() {
        use vera20k_gui::Key;
        let mut lobby = WolLobbyState::new(false, false);
        lobby.key(None, Some("hello"));
        assert_eq!(
            lobby.key(Some(Key::Enter), None),
            [WolLobbyEffect::Action(WolLobbyAction::Chat(
                "hello".encode_utf16().collect()
            ))]
        );
        assert!(lobby.edit(CHAT).units.is_empty());
        assert!(lobby.key(Some(Key::Enter), None).is_empty());
        lobby.key(None, Some("waves"));
        assert_eq!(
            lobby.activate(EMOTE),
            Some(WolLobbyAction::Emote("waves".encode_utf16().collect()))
        );
        assert_eq!(
            lobby.activate(EMOTE),
            Some(WolLobbyAction::Emote(Vec::new()))
        );
        // The observer image is a box: its toggle, not an activation,
        // changes the preference.
        assert_eq!(lobby.activate(OBSERVER_IMAGE), None);
        let layout = lobby.layout(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        lobby.sync(&layout);
        let rect = layout.control_rect(OBSERVER_IMAGE).unwrap();
        let (x, y) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
        let out = lobby.pointer(x, y, Some(true));
        assert!(
            out.contains(&WolLobbyEffect::Action(WolLobbyAction::ObserverChanged(
                true
            )))
        );
    }

    #[test]
    fn score_icons_show_rank_digits_co_op_and_badge() {
        let score = |points, rank, coop| PlayerScore { points, rank, coop };
        assert_eq!(
            score_icons(score(1520, 7, 0), 4),
            [("number7.pcx", 0x22), ("stargen.pcx", 0x3D)]
        );
        assert_eq!(
            score_icons(score(90, 123, 0xF), 4),
            [
                ("number1.pcx", 0x12),
                ("number2.pcx", 0x1A),
                ("number3.pcx", 0x22),
                ("cooperat.pcx", 0x2C),
                ("lieutena.pcx", 0x3D),
            ]
        );
        // Rank 40 has no hundreds digit; three of four campaigns is no co-op.
        assert_eq!(
            score_icons(score(31, 40, 0x7), 4),
            [
                ("number4.pcx", 0x1A),
                ("number0.pcx", 0x22),
                ("private.pcx", 0x3D),
            ]
        );
        // Rank 0 or 1000 shows no digits; points compare unsigned.
        assert_eq!(
            score_icons(score(u32::MAX, 0, 0), 0),
            [("comchief.pcx", 0x3D)]
        );
        assert_eq!(score_icons(score(32, 1000, 0), 4), [("corporal.pcx", 0x3D)]);
    }

    /// The hover help names every badge the list draws, in each language.
    #[test]
    fn every_badge_the_list_draws_has_a_name() {
        let mut csf = crate::assets::csf_file::test_csf(&[("GUI:Ok", "OK")]);
        crate::i18n::apply(&mut csf);
        for points in [0, 31, 32, 60, 61, 1500, 1501, 2000, 2001] {
            let key = badge_name_key(badge(points)).expect("a named badge");
            assert!(!csf.text(key).starts_with("MISSING:"), "{key}");
        }
        assert_eq!(
            badge_name_key(badge(2001)),
            Some("VERA:LobbyBadgeCommanderInChief")
        );
        assert_eq!(badge_name_key("number1.pcx"), None);
    }
}
