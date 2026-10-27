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

use crate::ui::lan_shell::{
    LanChatState, LanEditKey, LanListState, PlayerNameMatch, apply_chat_length_limit,
    saved_player_name_matches,
};
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

/// The modern lobby lists names and ping; legacy service-only columns stay
/// available to native diagnostics through the same layout and state owners.
const LEGACY_SORT_CONTROLS: [u16; 5] = [
    SORT_GAMETYPE,
    SORT_PASSWORD,
    SORT_RESLOCK,
    PLAYER_SORT_CLAN,
    PLAYER_SORT_RANK,
];

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

/// The normal VERA lobby keeps core play and Options, with Options in the third
/// row. The retail page above remains the owner for legacy service diagnostics.
const ONLINE_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        button(CREATE, 111, "GUI:CreateGame", "STT:LobbyButtonNew"),
        button(JOIN, 138, "GUI:JoinGame", "STT:LobbyButtonJoin"),
        button(OPTIONS, 166, "GUI:WOLOptions", "STT:WOLLobbyOptions"),
    ],
    ..PAGE
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
pub(crate) const PCX_FILES: [&str; 40] = [
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
    "cooperat.pcx",
    "comchief.pcx",
    "stargen.pcx",
    "general.pcx",
    "briggenr.pcx",
    "colonel.pcx",
    "major.pcx",
    "lieutena.pcx",
    "sergeant.pcx",
    "corporal.pcx",
    "private.pcx",
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
    pub(crate) sort_buttons: Vec<(u16, RectPx)>,
    pub(crate) observer_image: RectPx,
    pub(crate) observer_toggle: RectPx,
    /// The modern lobby's right-aligned `GUI:Observe` before the checkbox;
    /// the retail dialog has no caption there.
    pub(crate) observer_caption: Option<RectPx>,
    pub(crate) labels: [(&'static str, RectPx); 2],
}

impl WolLobbyLayout {
    pub(crate) fn new(width: i32, height: i32, page: &MenuPageSpec) -> Self {
        Self::build(width, height, ShellScale::RETAIL, page)
    }

    pub(crate) fn responsive(width: i32, height: i32, page: &MenuPageSpec) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height), page)
    }

    fn build(width: i32, height: i32, scale: ShellScale, spec: &MenuPageSpec) -> Self {
        let mut page = if scale.is_retail() {
            menu_page::compute_layout(spec, width as u32, height as u32)
        } else {
            menu_page::compute_responsive_layout(spec, width as u32, height as u32)
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
            ]
            .into(),
            observer_image: px(594, 10, 28, 21),
            observer_toggle: px(576, 13, 16, 17),
            observer_caption: None,
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

/// `0x00783B80`: the badge of the high-score points.
fn badge(points: u32) -> &'static str {
    match points {
        2001.. => "comchief.pcx",
        1501.. => "stargen.pcx",
        1001.. => "general.pcx",
        501.. => "briggenr.pcx",
        251.. => "colonel.pcx",
        151.. => "major.pcx",
        81.. => "lieutena.pcx",
        61.. => "sergeant.pcx",
        32.. => "corporal.pcx",
        _ => "private.pcx",
    }
}

/// `0x00783C80`'s high-score cells (`0x007845A8..0x007847F0`): the rank's
/// digits (`0x00783A90`, `number<d>.pcx`) for a rank below 1000, hundreds
/// from 100 and tens from 10; `cooperat.pcx` once the co-op bits hold every
/// one of the `campaigns` CoopCampMD campaigns (`0x0049E4F0`); the badge.
pub(crate) fn score_icons(score: PlayerScore, campaigns: u32) -> Vec<(&'static str, i32)> {
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
    /// Buddy List, Clan, Tournament and News run dialogs or web pages that
    /// are later chains.
    Unported(u16),
    Popup(WolPopupAction),
}

/// The dialog's children: list contents, selection, enable state, chat.
#[derive(Debug, Clone)]
pub(crate) struct WolLobbyState {
    modern: bool,
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
    focus_started_at: crate::util::time::Instant,
}

impl WolLobbyState {
    pub(crate) fn new(observer: bool, modern: bool) -> Self {
        Self {
            modern,
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
            focus_started_at: crate::util::time::Instant::now(),
        }
    }

    pub(crate) fn enabled(&self, control: u16) -> bool {
        !self.disabled.contains(&control)
            && (!self.modern || !LEGACY_SORT_CONTROLS.contains(&control))
            && (PAGE.button(control).is_none() || self.page_spec().button(control).is_some())
    }

    pub(crate) fn page_spec(&self) -> &'static MenuPageSpec {
        if self.modern { &ONLINE_PAGE } else { &PAGE }
    }

    pub(crate) fn is_modern(&self) -> bool {
        self.modern
    }

    pub(crate) fn layout(&self, width: i32, height: i32, responsive: bool) -> WolLobbyLayout {
        let mut layout = if responsive {
            WolLobbyLayout::responsive(width, height, self.page_spec())
        } else {
            WolLobbyLayout::new(width, height, self.page_spec())
        };
        if self.modern {
            layout
                .sort_buttons
                .retain(|(id, _)| !LEGACY_SORT_CONTROLS.contains(id));
            let scale = layout.page.scale;
            for (id, rect) in &mut layout.sort_buttons {
                match *id {
                    SORT_ALPHA => *rect = scale.scale_rect(RectPx::new(12, 37, 198, 17)),
                    PLAYER_SORT_ALPHA => *rect = scale.scale_rect(RectPx::new(322, 37, 299, 17)),
                    _ => {}
                }
            }
            // The retail checkbox and eye image sit on two centre lines below
            // the Players heading, with no caption. Here the checkbox trails
            // that heading: its 18x18 plate is the control (the plate alone
            // toggles, as on every shell checkbox), flush with the list's
            // right edge and centred on the heading's capitals (rows 14..22),
            // and a caption on the heading's baseline names it. It shows the
            // state, so the eye image is gone.
            layout.observer_toggle = scale.scale_rect(RectPx::new(604, 10, 18, 18));
            layout.observer_caption = Some(scale.scale_rect(RectPx::new(484, 10, 114, 17)));
            layout.observer_image = RectPx::new(0, 0, 0, 0);
        }
        layout
    }

    /// `SetFocus` on the chat edit `0x540`: its caret blink restarts.
    pub(crate) fn refocus_chat(&mut self) {
        self.focus_started_at = crate::util::time::Instant::now();
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
        for button in self.page_spec().buttons() {
            self.set_enabled(button.id, false);
        }
    }

    /// Re-enable them; Join takes `join`.
    pub(crate) fn enable_commands(&mut self, join: bool) {
        for button in self.page_spec().buttons() {
            self.set_enabled(button.id, button.id != JOIN || join);
        }
    }

    pub(crate) fn begin_lobby_switch(&mut self) {
        for control in [LOBBIES, GAMES]
            .into_iter()
            .chain(self.page_spec().buttons().map(|b| b.id))
        {
            self.set_enabled(control, false);
        }
    }

    /// `0x007A5E40` after a chat lobby joined: everything back, Join as the
    /// still-selected game admits.
    pub(crate) fn lobby_switched(&mut self, join: bool) {
        for control in [LOBBIES, GAMES]
            .into_iter()
            .chain(self.page_spec().buttons().map(|b| b.id))
        {
            self.set_enabled(control, control != JOIN || join);
        }
    }

    pub(crate) fn caret_visible(&self, now: crate::util::time::Instant) -> bool {
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

    /// WOL `0x00783C80` saves selected names before `LB_RESETCONTENT` and
    /// restores every matching new row afterward (`0x005E5F10` / `0x005E6230`).
    pub(crate) fn replace_players(&mut self, rows: Vec<WolPlayerRow>) {
        let mut selected_rows = self.selected_players.clone();
        selected_rows.sort_unstable();
        selected_rows.dedup();
        let selected_names = selected_rows
            .into_iter()
            .filter_map(|index| self.player_rows.get(index))
            .map(|row| row.name.clone())
            .collect::<Vec<_>>();
        self.player_rows = rows;
        self.selected_players = self
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
            OPTIONS => WolLobbyAction::Options,
            PAGE_BUTTON => WolLobbyAction::Page,
            BUDDY | CLAN | TOURNAMENT | NEWS => WolLobbyAction::Unported(control),
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
            let layout =
                WolLobbyLayout::new(width.parse().unwrap(), height.parse().unwrap(), &PAGE);
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
            assert_eq!(tooltip_key(id, &PAGE), help.as_str(), "{key}");
        }
    }

    #[test]
    fn create_and_join_disable_the_command_column_and_join_starts_disabled() {
        let mut lobby = WolLobbyState::new(false, false);
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
    fn online_sidebar_keeps_core_actions_and_never_reenables_removed_controls() {
        let mut lobby = WolLobbyState::new(false, true);
        let layout = WolLobbyLayout::new(800, 600, lobby.page_spec());
        let controls: Vec<_> = layout.page.buttons.iter().map(|button| button.id).collect();
        assert_eq!(controls, [CREATE, JOIN, OPTIONS, BACK]);
        let retail = WolLobbyLayout::new(800, 600, &PAGE);
        assert_eq!(layout.control_rect(OPTIONS), retail.control_rect(BUDDY));
        lobby.disable_commands();
        lobby.enable_commands(true);
        lobby.begin_lobby_switch();
        lobby.lobby_switched(true);
        for control in [BUDDY, CLAN, TOURNAMENT, NEWS, PAGE_BUTTON] {
            assert!(!lobby.enabled(control));
            assert_eq!(lobby.activate(control), None);
            assert_eq!(layout.control_rect(control), None);
            assert_eq!(tooltip_key(control, lobby.page_spec()), None);
        }
        assert_eq!(lobby.activate(OPTIONS), Some(WolLobbyAction::Options));
    }

    #[test]
    fn modern_body_keeps_useful_controls_and_cannot_activate_legacy_headers() {
        let mut lobby = WolLobbyState::new(false, true);
        let layout = lobby.layout(800, 600, false);
        let legacy = WolLobbyState::new(false, false).layout(800, 600, false);
        let headers: Vec<_> = layout.sort_buttons.iter().map(|(id, _)| *id).collect();
        assert_eq!(headers, [SORT_ALPHA, SORT_PING, PLAYER_SORT_ALPHA]);
        for id in LEGACY_SORT_CONTROLS {
            let old = legacy.control_rect(id).expect("legacy header");
            assert_eq!(layout.control_rect(id), None);
            assert_ne!(
                layout.control_at(old.x + old.w / 2, old.y + old.h / 2),
                Some(id)
            );
            lobby.set_enabled(id, true);
            assert!(!lobby.enabled(id));
            assert_eq!(lobby.activate(id), None);
        }
        for id in [
            SORT_ALPHA,
            SORT_PING,
            PLAYER_SORT_ALPHA,
            REFRESH,
            EMOTE,
            GAMES,
            PLAYERS,
            LOBBIES,
            CHAT_LOG,
            CHAT,
            OBSERVER_TOGGLE,
        ] {
            let rect = layout.control_rect(id).expect("useful control");
            assert_eq!(
                layout.control_at(rect.x + rect.w / 2, rect.y + rect.h / 2),
                Some(id)
            );
        }
    }

    /// The modern Observe checkbox trails the Players heading on its row,
    /// flush with the player list, with its caption between the two; the
    /// retail eye image's place left of it takes no clicks.
    #[test]
    fn modern_observe_checkbox_trails_the_players_heading() {
        let layout = WolLobbyState::new(false, true).layout(800, 600, false);
        let legacy = WolLobbyState::new(false, false).layout(800, 600, false);
        let (_, heading) = layout.labels[1];
        let plate = crate::ui::skirmish_shell::checkbox_icon_rect(
            layout.observer_toggle,
            layout.page.scale,
        );
        assert_eq!(plate, layout.observer_toggle);
        assert_eq!(plate.y, heading.y);
        assert_eq!(plate.x + plate.w, layout.players.x + layout.players.w);
        let caption = layout.observer_caption.expect("caption");
        assert_eq!(caption.y, heading.y);
        assert!(heading.x + heading.w <= caption.x && caption.x + caption.w < plate.x);
        let eye = legacy.observer_image;
        assert_eq!(layout.control_at(eye.x + 2, eye.y + eye.h / 2), None);
        assert_eq!(legacy.observer_caption, None);
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
        };
        let mut lobby = WolLobbyState::new(false, false);
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
            rank: None,
            score_icons: Vec::new(),
        };
        lobby.replace_players(vec![player("x"), player("y"), player("z")]);
        lobby.toggle_player(2);
        lobby.toggle_player(0);
        assert_eq!(lobby.private_targets(), ["x", "z"]);
        lobby.replace_players(vec![
            player("z"),
            player("x"),
            player("x"),
            player("X"),
            player("y"),
        ]);
        assert_eq!(lobby.selected_players, [0, 1, 2]);
        assert_eq!(lobby.private_targets(), ["z", "x", "x"]);

        lobby.replace_players(vec![player("x"), player("y")]);
        assert_eq!(lobby.selected_players, [0]);
        assert_eq!(lobby.private_targets(), ["x"]);

        lobby.replace_players(vec![player("y")]);
        assert!(lobby.private_targets().is_empty());
    }

    #[test]
    fn chat_enter_and_emote_read_and_clear_the_edit() {
        let mut lobby = WolLobbyState::new(false, false);
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
}
