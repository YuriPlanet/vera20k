//! Network setup presentation: LAN host BC (005DE520) and guest BD
//! (005E1CA0), WOL host C2 (0078F7F0) and guest C9 (00791B10). The WOL pages
//! are supersets of the LAN ones with the same control ids (resources C2/C9).
//! MatchSession owns admitted participants, options, readiness and map status.
//! This leaf keeps only control interaction and emits commands for that owner.

use crate::util::time::Instant;
use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use crate::ui::lan_shell::{LanChatState, LanListState};
use crate::ui::shell::combobox::ComboPopup;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::{AnchorRule, DialogId, LOW_HEADING_ANCHOR, MAP_TEXT_ANCHOR};
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, child_window};
use crate::ui::shell::layout::Placer;
use crate::ui::shell::list::{ListMetrics, ListScrollPart, ShellListGeometry};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::page::{self as page_nodes, PageCue, PageEvent};
use crate::ui::shell::trackbar::{TrackbarPress, TrackbarRange};
use crate::ui::skirmish_shell::{SkirmishStatics, SkirmishValidationModalState};

pub(crate) const VISIBLE_SETUP_ROWS: usize = 8;
/// The most rows and headers VERA's roster ([`SetupDialog::modern`]) shows
/// at once ([`roster_page`]), and the controls it numbers by them.
pub(crate) const MODERN_SETUP_ROWS: usize = 48;
// VERA-only roster navigation; original dialog/control IDs retain their role.
pub(crate) const PREVIOUS_ROWS: u16 = 0x7F00;
pub(crate) const NEXT_ROWS: u16 = 0x7F01;
/// VERA-only: the room's Game Options button and its popup's OK.
pub(crate) const GAME_OPTIONS: u16 = 0x7F02;
pub(crate) const GAME_OPTIONS_OK: u16 = 0x7F03;
/// VERA-only: the missing-map offer's Download and Leave.
pub(crate) const MAP_OFFER_DOWNLOAD: u16 = 0x7F04;
pub(crate) const MAP_OFFER_LEAVE: u16 = 0x7F05;
/// VERA-only, in the team bar under the roster: the Teams count and the
/// host's Team Starts and Team Colors.
pub(crate) const TEAM_COUNT: u16 = 0x7F0D;
pub(crate) const TEAM_STARTS: u16 = 0x7F0E;
pub(crate) const TEAM_COLORS: u16 = 0x7F11;
/// VERA-only, online: the team bar's Balance.
pub(crate) const BALANCE: u16 = 0x7F0F;
/// VERA-only, the host's, in the team bar: Room size, the most people the
/// room holds, spectators too.
pub(crate) const ROOM_SIZE: u16 = 0x7F10;
/// VERA-only, in an online room: Lobby, to the public lobby while the room
/// goes on hidden (docs/lobby-protocol.md "Visiting the lobby").
pub(crate) const LOBBY: u16 = 0x7F16;
/// VERA-only combos of the rows past the resource's eight, one block of
/// 0x40 ids per column.
const ROOM_ROW_COMBOS: u16 = 0x7000;
/// VERA's Player list items past gamemd's 0..7: the host moves a seated
/// guest to the spectators, or a spectator into a free seat.
pub(crate) const MOVE_TO_SPECTATORS: i32 = 8;
pub(crate) const GIVE_SEAT: i32 = 9;
/// VERA-only header rows ([`SetupHeader`]): each one's Join (or Watch)
/// button and the host's Add AI list, by the row on the page.
const TEAM_JOIN: u16 = 0x7140;
const TEAM_ADD_AI: u16 = 0x7180;

/// VERA's room ([`SetupDialog::modern`]): the roster's and the chat's
/// columns side by side, in 800x600 pixels, from past the background's frame
/// at the window's left edge to the shell's right panel
/// ([`ShellScale::anchored_right`]), with this gap between them and after
/// the frame, and this margin before the panel. The chat takes this share
/// of their width, within these limits.
const ROOM_MARGIN: i32 = 20;
const ROOM_GAP: i32 = 8;
/// The room background's frame down its left edge, in its 800-wide art: the
/// room keeps it at its own width where it spreads the background.
pub(crate) const ROOM_FRAME_W: i32 = 9;
const ROOM_RIGHT_PANEL_W: i32 = 168;
const CHAT_SHARE: (i32, i32) = (3, 10);
const CHAT_W: (i32, i32) = (190, 380);
/// VERA's roster: the top of its first entry and the bottom of its last,
/// the heights of a player's row and of a header, and its column labels'
/// top and height.
const ROSTER_TOP: i32 = 40;
const ROSTER_BOTTOM: i32 = 551;
const ROSTER_ROW_H: i32 = 20;
const ROSTER_HEADER_H: i32 = 23;
const ROSTER_LABEL_Y: i32 = 14;
const ROSTER_LABEL_H: i32 = 18;
/// The space between one group's box and the next one's.
const ROSTER_GROUP_GAP: i32 = 14;
/// A group's box reaches this far past its entries' left and right edges,
/// and above and below them.
const ROSTER_BOX_MARGIN: (i32, i32) = (5, 3);
/// The roster's entries start this far into its column, which is this much
/// wider than they are: the groups' boxes and, at its right end, a scrollbar
/// when they do not all fit.
const ROSTER_INSET: i32 = 5;
const ROSTER_SPARE_W: i32 = 34;
/// The roster's entries at their full width, from their left edge: a row's
/// Player, Side, Colour, Start and Team cells, as left edge and width; a
/// header's caption, the host's Add AI and, at the row's right end, its Join:
/// text like the rows', no button art. A narrower column narrows them all
/// ([`RosterColumns`]).
const ROSTER_W: i32 = 751;
const ROSTER_CELLS: [(i32, i32); 5] = [(18, 260), (286, 156), (450, 57), (515, 57), (580, 57)];
const ROSTER_CAPTION_W: i32 = 260;
const ROSTER_ADD_AI: (i32, i32) = (286, 91);
const ROSTER_JOIN_W: i32 = 91;
/// A list opened from VERA's roster is at least as wide as its widest text,
/// with this margin at each side, and as a colour swatch this wide.
const ROSTER_LIST_MARGIN: i32 = 3;
const ROSTER_SWATCH_LIST_W: i32 = 40;
/// VERA's team bar gives Teams and Room size one row each, followed
/// by one row for the available buttons. The controls are authored at
/// 78% of the shell's size ([`MultiplayerSetupLayout::control_scale`]).
const TEAM_BAR_ROW_H: i32 = 25;
const TEAM_BAR_INSET: i32 = 8;
const TEAM_BAR_GAP: i32 = 12;
const TEAM_BAR_LABEL: RectPx = RectPx::new(0, 3, 90, 18);
const TEAM_BAR_TRACKBAR: RectPx = RectPx::new(92, 4, 151, 17);
/// The team bar's trackbar window: the room's 129x21 half again as wide,
/// its plaque the same, so its rail is longer. It has its own frame
/// (`render::skirmish_shell_chrome::TRACKBAR_FRAMES`).
const TEAM_BAR_TRACKBAR_WINDOW: (i32, i32) = (194, 21);
const TEAM_BAR_BUTTON: RectPx = RectPx::new(0, 3, 97, 19);
/// The chat's log ends here, its edit line under it.
const CHAT_LOG_BOTTOM: i32 = 529;

/// Where VERA's roster's entries stand across its column, in 800x600
/// pixels: [`ROSTER_CELLS`] and a header's parts, narrowed with a column
/// too narrow for [`ROSTER_W`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RosterColumns {
    x: i32,
    w: i32,
    cells: [(i32, i32); 5],
    caption_w: i32,
    add_ai: (i32, i32),
    join_w: i32,
}
impl RosterColumns {
    fn new(column: RectPx) -> Self {
        let w = column.w - ROSTER_SPARE_W;
        let narrow = |length: i32| length * w.clamp(0, ROSTER_W) / ROSTER_W;
        let first = ROSTER_CELLS[0].0;
        let at = |dx: i32| first + narrow(dx - first);
        Self {
            x: column.x + ROSTER_INSET,
            w,
            cells: ROSTER_CELLS.map(|(dx, width)| (at(dx), narrow(width))),
            caption_w: narrow(ROSTER_CAPTION_W),
            add_ai: (at(ROSTER_ADD_AI.0), narrow(ROSTER_ADD_AI.1)),
            join_w: narrow(ROSTER_JOIN_W),
        }
    }
}
/// The roster's text, as a share of the shell's own size.
pub(crate) const ROSTER_TEXT_SCALE: f32 = 0.975;

/// Whether a press on an enabled cell of VERA's roster opens its list: it
/// has something else to choose.
pub(crate) fn roster_cell_opens(combo: &SetupCombo) -> bool {
    combo.choices.len() > 1
}

/// One entry of VERA's roster: a player's row, or a header over a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RosterItem {
    Row,
    Header(SetupGroup),
}
impl RosterItem {
    const fn height(self) -> i32 {
        match self {
            Self::Row => ROSTER_ROW_H,
            Self::Header(_) => ROSTER_HEADER_H,
        }
    }
}

/// Where an entry of VERA's roster stands: which entry, and its top at
/// 800x600.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RosterPlace {
    pub(crate) item: usize,
    pub(crate) y: i32,
    pub(crate) header: bool,
}

/// The entries VERA's roster shows from entry `top` down its one column,
/// until one no longer fits above `bottom` (800x600 pixels: over the team
/// bar in a room with one, [`MultiplayerSetupLayout::roster_bottom`]). When `top` is a player under
/// a header above it, that header stays pinned first, so the group keeps
/// its caption and Join. A header under another group leaves
/// [`ROSTER_GROUP_GAP`] between their boxes, and none stands at the foot
/// without its first row.
pub(crate) fn roster_page(items: &[RosterItem], top: usize, bottom: i32) -> Vec<RosterPlace> {
    let pinned = items
        .get(top)
        .filter(|item| **item == RosterItem::Row)
        .and_then(|_| {
            items[..top]
                .iter()
                .rposition(|item| matches!(item, RosterItem::Header(_)))
        });
    let mut places = Vec::new();
    let mut y = ROSTER_TOP;
    for index in pinned
        .into_iter()
        .chain(top..items.len())
        .take(MODERN_SETUP_ROWS)
    {
        let item = items[index];
        let header = matches!(item, RosterItem::Header(_));
        if header && y > ROSTER_TOP {
            y += ROSTER_GROUP_GAP;
        }
        let first_row = if header && items.get(index + 1) == Some(&RosterItem::Row) {
            ROSTER_ROW_H
        } else {
            0
        };
        if y + item.height() + first_row > bottom {
            break;
        }
        places.push(RosterPlace {
            item: index,
            y,
            header,
        });
        y += item.height();
    }
    places
}

/// Where VERA's roster scrolls: the entries it can show first ([`roster_page`]'s
/// `top`), and the first of them from which every entry to the last shows.
/// Each player's row is one, its header pinned over it; a header only when
/// no row is under it, as it would show the same as its first row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RosterScroll {
    pub(crate) stops: Vec<usize>,
    pub(crate) max_top: usize,
}
pub(crate) fn roster_scroll(items: &[RosterItem], bottom: i32) -> RosterScroll {
    let stops: Vec<usize> = (0..items.len())
        .filter(|&index| {
            items[index] == RosterItem::Row || items.get(index + 1) != Some(&RosterItem::Row)
        })
        .collect();
    let max_top = stops
        .iter()
        .position(|&top| {
            roster_page(items, top, bottom)
                .last()
                .is_some_and(|place| place.item + 1 == items.len())
        })
        .unwrap_or(0);
    RosterScroll { stops, max_top }
}

/// The Join button of the header on page row `row`.
pub(crate) fn team_join(row: usize) -> u16 {
    TEAM_JOIN + row as u16
}
/// The host's Add AI list on the header on page row `row`.
pub(crate) fn team_add_ai(row: usize) -> u16 {
    TEAM_ADD_AI + row as u16
}
fn page_row(id: u16, first: u16) -> Option<usize> {
    id.checked_sub(first)
        .map(usize::from)
        .filter(|row| *row < MODERN_SETUP_ROWS)
}
/// The page row of a header's Join button.
pub(crate) fn team_join_row(id: u16) -> Option<usize> {
    page_row(id, TEAM_JOIN)
}
/// The page row of a header's Add AI list.
pub(crate) fn team_add_ai_row(id: u16) -> Option<usize> {
    page_row(id, TEAM_ADD_AI)
}
pub(crate) const START: u16 = 0x588;
pub(crate) const ACCEPT: u16 = 0x59F;
pub(crate) const BACK: u16 = 2;
pub(crate) const CHOOSE_MAP: u16 = 0x5AA;
pub(crate) const CHAT: u16 = 0x540;
pub(crate) const CHAT_LOG: u16 = 0x53F;
pub(crate) const SPEED: u16 = 0x529;
pub(crate) const CREDITS: u16 = 0x402;
pub(crate) const UNITS: u16 = 0x54F;
pub(crate) const DIFFICULTY: u16 = 0x732;
pub(crate) const CHECKBOXES: [u16; 5] = [0x54E, 0x3E8, 0x54D, 0x69A, 0x69D];
/// VERA-only checkboxes its Game Options adds under the native five.
pub(crate) const BRIDGES: u16 = 0x7F06;
pub(crate) const ALLYING: u16 = 0x7F07;
pub(crate) const NO_YURI: u16 = 0x7F08;
pub(crate) const NO_FRANCE: u16 = 0x7F09;
pub(crate) const NO_SPY: u16 = 0x7F0A;
pub(crate) const NO_STOLEN_TECH: u16 = 0x7F0B;
/// The room's Terrain Deformation ([`HostRules`](crate::skirmish_launch::HostRules)).
pub(crate) const TERRAIN_DEFORMATION: u16 = 0x7F12;
/// The room's Superweapon Limit, a trackbar in VERA's Game Options only
/// ([`HostRules`](crate::skirmish_launch::HostRules)).
pub(crate) const SUPERWEAPON_LIMIT: u16 = 0x7F14;
pub(crate) const ROOM_CHECKBOXES: [u16; 7] = [
    BRIDGES,
    ALLYING,
    NO_YURI,
    NO_FRANCE,
    NO_SPY,
    NO_STOLEN_TECH,
    TERRAIN_DEFORMATION,
];
/// Their labels in VERA's language files; the retail string table has none
/// for them.
pub(crate) const ROOM_CHECKBOX_LABELS: [&str; 7] = [
    "VERA:DestroyableBridges",
    "VERA:InGameAllying",
    "VERA:NoYuri",
    "VERA:NoFrance",
    "VERA:NoSpy",
    "VERA:NoStolenTech",
    "VERA:TerrainDeformation",
];
/// The trackbars VERA's room moves into Game Options; Difficulty stays.
pub(crate) const OPTION_TRACKBARS: [u16; 3] = [SPEED, CREDITS, UNITS];
pub(crate) const COUNTRIES: [u16; 8] = [0x6A1, 0x510, 0x513, 0x51E, 0x514, 0x51F, 0x520, 0x521];
pub(crate) const COLORS: [u16; 8] = [0x6A2, 0x522, 0x523, 0x524, 0x525, 0x526, 0x527, 0x528];
pub(crate) const STARTS: [u16; 8] = [0x6A3, 0x6A4, 0x6A5, 0x6A6, 0x6A7, 0x6A8, 0x6AA, 0x6AB];
/// WOL-only C2/C9 controls: Page (modal 0xCF) and the emote button.
pub(crate) const PAGE: u16 = 0x5C2;
pub(crate) const EMOTE: u16 = 0x3ED;
/// The map thumbnail.
pub(crate) const MAP_PREVIEW: u16 = 0x468;

/// The service whose setup dialog this is. Every shared helper keeps one
/// code path and branches on GameMode 3/4 like the native helpers do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SetupFlavor {
    Lan,
    Wol,
}

/// One of the four setup dialogs: BC/BD (LAN) or C2/C9 (WOL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SetupDialog {
    pub(crate) host: bool,
    pub(crate) flavor: SetupFlavor,
    /// VERA's game room: a guest's game accepts the host's settings by
    /// itself, so BD and C9 have no Accept button, and C2 and C9 have no
    /// Page (chat's `TXT_PAGE_COMMAND` still pages). Speed, credits, units
    /// and the checkboxes move into a Game Options popup, and the roster
    /// lists its players as small plain rows in two columns
    /// ([`roster_page`]), each cell opening its list when pressed. Native
    /// diagnostics keep the original dialogs.
    pub(crate) modern: bool,
}
impl SetupDialog {
    pub(crate) const fn page(self) -> &'static MenuPageSpec {
        match (self.flavor, self.host) {
            (SetupFlavor::Lan, true) if self.modern => &MODERN_HOST_PAGE,
            (SetupFlavor::Lan, true) => &HOST_PAGE,
            (SetupFlavor::Lan, false) if self.modern => &MODERN_GUEST_PAGE,
            (SetupFlavor::Lan, false) => &GUEST_PAGE,
            (SetupFlavor::Wol, true) if self.modern => &MODERN_WOL_HOST_PAGE,
            (SetupFlavor::Wol, true) => &WOL_HOST_PAGE,
            (SetupFlavor::Wol, false) if self.modern => &MODERN_WOL_GUEST_PAGE,
            (SetupFlavor::Wol, false) => &WOL_GUEST_PAGE,
        }
    }
    pub(crate) const fn wol(self) -> bool {
        matches!(self.flavor, SetupFlavor::Wol)
    }
    /// VERA's room captions BC's map button "Choose Map": retail's
    /// `GUI:ChooseMap` text, "Customize Battle", reads as the game settings
    /// the room keeps under Game Options. The caption's label in VERA's
    /// language files; `None` keeps the CSF caption.
    pub(crate) fn button_label(self, id: u16) -> Option<&'static str> {
        match id {
            CHOOSE_MAP if self.modern => Some("VERA:ChooseMap"),
            _ => None,
        }
    }
    /// Rows one roster page shows; at most, in VERA's room.
    pub(crate) const fn rows_per_page(self) -> usize {
        if self.modern {
            MODERN_SETUP_ROWS
        } else {
            VISIBLE_SETUP_ROWS
        }
    }
}

pub(crate) const HOST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xBC),
    title_key: "GUI:HostScreen",
    stacked: &[
        MenuPageButtonSpec::new(START, 147, "GUI:StartGame", "STT:HostButtonGo"),
        MenuPageButtonSpec::new(CHOOSE_MAP, 176, "GUI:ChooseMap", "STT:HostButtonChooseMap"),
    ],
    back: MenuPageButtonSpec::new(BACK, 346, "GUI:Back", "STT:HostButtonBack"),
};
pub(crate) const GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xBD),
    title_key: "GUI:JoinScreen",
    stacked: &[MenuPageButtonSpec::new(
        ACCEPT,
        149,
        "GUI:Accept",
        "STT:GuestButtonAccept",
    )],
    back: MenuPageButtonSpec::new(BACK, 346, "GUI:Back", "STT:GuestButtonBack"),
};

const WOL_BACK: MenuPageButtonSpec =
    MenuPageButtonSpec::new(BACK, 257, "GUI:Back", "STT:WOLNewGameCancel");
pub(crate) const WOL_HOST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xC2),
    title_key: "GUI:HostScreen",
    stacked: &[
        MenuPageButtonSpec::new(START, 156, "GUI:StartGame", "STT:HostButtonGo"),
        MenuPageButtonSpec::new(CHOOSE_MAP, 183, "GUI:ChooseMap", "STT:HostButtonChooseMap"),
        MenuPageButtonSpec::new(PAGE, 210, "GUI:PageButton", "STT:WOLPageButton"),
    ],
    back: WOL_BACK,
};
pub(crate) const WOL_GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xC9),
    title_key: "GUI:JoinScreen",
    stacked: &[
        MenuPageButtonSpec::new(ACCEPT, 156, "GUI:Accept", "STT:GuestButtonAccept"),
        MenuPageButtonSpec::new(PAGE, 183, "GUI:PageButton", "STT:WOLPageButton"),
    ],
    back: WOL_BACK,
};
/// VERA's game room ([`SetupDialog::modern`]): Game Options takes a guest's
/// Accept place and a host's Choose Map place, and Choose Map moves down a
/// place, to C2's Page place on WOL.
const GAME_OPTIONS_BUTTON: MenuPageButtonSpec =
    MenuPageButtonSpec::new(GAME_OPTIONS, 0, "GUI:GameOptions", "");
const MODERN_HOST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        HOST_PAGE.stacked[0],
        GAME_OPTIONS_BUTTON.at(HOST_PAGE.stacked[1].dlu_top),
        HOST_PAGE.stacked[1].at(2 * HOST_PAGE.stacked[1].dlu_top - HOST_PAGE.stacked[0].dlu_top),
    ],
    ..HOST_PAGE
};
const MODERN_GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[GAME_OPTIONS_BUTTON.at(GUEST_PAGE.stacked[0].dlu_top)],
    ..GUEST_PAGE
};
const LOBBY_BUTTON: MenuPageButtonSpec = MenuPageButtonSpec::new(LOBBY, 0, "VERA:Lobby", "");
/// VERA's online room adds Lobby under its other buttons.
const MODERN_WOL_GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        GAME_OPTIONS_BUTTON.at(WOL_GUEST_PAGE.stacked[0].dlu_top),
        LOBBY_BUTTON.at(WOL_GUEST_PAGE.stacked[1].dlu_top),
    ],
    ..WOL_GUEST_PAGE
};
const MODERN_WOL_HOST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        WOL_HOST_PAGE.stacked[0],
        GAME_OPTIONS_BUTTON.at(WOL_HOST_PAGE.stacked[1].dlu_top),
        WOL_HOST_PAGE.stacked[1].at(WOL_HOST_PAGE.stacked[2].dlu_top),
        LOBBY_BUTTON.at(2 * WOL_HOST_PAGE.stacked[2].dlu_top - WOL_HOST_PAGE.stacked[1].dlu_top),
    ],
    ..WOL_HOST_PAGE
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetupColumn {
    Player,
    Country,
    Color,
    Start,
    Team,
}
impl SetupColumn {
    pub(crate) const ALL: [Self; 5] = [
        Self::Player,
        Self::Country,
        Self::Color,
        Self::Start,
        Self::Team,
    ];
    pub(crate) fn control(self, row: usize) -> u16 {
        if row >= VISIBLE_SETUP_ROWS {
            return ROOM_ROW_COMBOS + self as u16 * 0x40 + row as u16;
        }
        match self {
            Self::Player => 0x411 + row as u16,
            Self::Country => COUNTRIES[row],
            Self::Color => COLORS[row],
            Self::Start => STARTS[row],
            Self::Team => 0x76D + row as u16,
        }
    }
    pub(crate) fn decode(control: u16) -> Option<(Self, usize)> {
        Self::ALL.into_iter().find_map(|column| {
            (0..MODERN_SETUP_ROWS)
                .find(|row| column.control(*row) == control)
                .map(|row| (column, row))
        })
    }
    /// Explicit 4DE overrides from shared setup initialization4E37D0 etc.
    pub(crate) fn visible_rows(self) -> usize {
        match self {
            Self::Country => 7,
            Self::Color | Self::Start | Self::Team => 9,
            Self::Player => 3,
        }
    }
}

/// Read-only control projection, rebuilt from the session; native item-data is
/// passed back unchanged so the session owner can validate a requested mutation.
#[derive(Debug, Clone)]
pub(crate) struct SetupChoice {
    pub(crate) value: i32,
    pub(crate) text: String,
    pub(crate) swatch: Option<usize>,
}
#[derive(Debug, Clone)]
pub(crate) struct SetupCombo {
    pub(crate) id: u16,
    /// Session row displayed by this resource control, independent of its page.
    pub(crate) row: u8,
    pub(crate) enabled: bool,
    pub(crate) visible: bool,
    pub(crate) selected: Option<usize>,
    pub(crate) choices: Vec<SetupChoice>,
    /// In VERA's roster, the least width its list opens at, in 800x600
    /// pixels ([`roster_list_width`]).
    pub(crate) list_w: i32,
}
/// The least width, in 800x600 pixels, VERA's roster opens a list of
/// `choices` at: its widest text at the roster's size, which `width`
/// measures in the shell font, or its colour swatches.
pub(crate) fn roster_list_width(choices: &[SetupChoice], width: impl Fn(&str) -> u32) -> i32 {
    choices
        .iter()
        .map(|choice| match choice.swatch {
            Some(_) => ROSTER_SWATCH_LIST_W,
            None => {
                (width(&choice.text) as f32 * ROSTER_TEXT_SCALE).ceil() as i32
                    + 2 * ROSTER_LIST_MARGIN
            }
        })
        .max()
        .unwrap_or(0)
}
#[derive(Debug, Clone)]
pub(crate) struct SetupTrackbar {
    pub(crate) id: u16,
    pub(crate) value: i32,
    pub(crate) range: TrackbarRange,
    pub(crate) enabled: bool,
    pub(crate) visible: bool,
}
impl SetupTrackbar {
    /// Its range as its window holds it: Co-op5C21D0 sends7324AC(0), so
    /// Difficulty runs with no value-plaque reserve; ordinary options retain
    /// the49-pixel plaque.
    pub(crate) fn window_range(&self) -> TrackbarRange {
        self.range.with_plaque(self.id != DIFFICULTY)
    }
    pub(crate) fn thumb_left(&self, rect: RectPx, scale: ShellScale) -> i32 {
        self.window_range().thumb_left_in(self.value, rect, scale)
    }
    pub(crate) fn press(&self, x: i32, y: i32, rect: RectPx, scale: ShellScale) -> TrackbarPress {
        self.window_range().press_in(self.value, x, y, rect, scale)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SetupRowIcon {
    pub(crate) country: Option<String>,
    pub(crate) host: bool,
    pub(crate) accepted: bool,
    /// VERA's online room: the player's badge
    /// ([`crate::ui::wol_lobby::badge`]), drawn at the right of their name.
    pub(crate) badge: Option<&'static str>,
    /// The local player's own row, which VERA's roster marks.
    pub(crate) local: bool,
}

/// The rows a header row of VERA's room starts, in a mode with teams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetupGroup {
    /// The players on one team, by its number (0 is Team A).
    Team(u8),
    /// Every seated player of a room with Teams Off, in one box without a
    /// caption.
    Players,
    /// The players on no team, in an older host's room with Teams set.
    NoTeam,
    /// The host's closed seats, over the Add slot row that opens one again.
    ClosedSeats,
    Spectators,
}

/// A header row of VERA's room: its caption in the Player column, then a
/// Join button (Watch on the spectators' header) and, for the host, an Add
/// AI list among the combos.
#[derive(Debug, Clone)]
pub(crate) struct SetupHeader {
    pub(crate) group: SetupGroup,
    /// Chosen swatches of every player on this team, including rows offscreen.
    pub(crate) member_colors: Vec<u8>,
    pub(crate) caption: String,
    /// The button's caption; `None` shows no button.
    pub(crate) join: Option<String>,
    pub(crate) join_enabled: bool,
}

/// A roster row the host of VERA's room may drag to another box: a
/// person's or an AI's, and the box it stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DragRow {
    pub(crate) row: u8,
    pub(crate) group: SetupGroup,
    /// Whether it may go to the spectators: a guest's, not the host's own or
    /// an AI's.
    pub(crate) may_watch: bool,
}

/// The host's drag of a roster row: pressed on `row`, carried once the
/// pointer leaves the press by [`ROW_DRAG_START`], over the box whose header
/// is page row `target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowDrag {
    pub(crate) row: u8,
    from: (i32, i32),
    pub(crate) at: (i32, i32),
    pub(crate) carrying: bool,
    pub(crate) target: Option<usize>,
}
/// How far, in window pixels, the pointer moves before a pressed row is
/// carried; a shorter press stays a click on its cell.
const ROW_DRAG_START: i32 = 6;

/// A box VERA's room opens over itself; it takes the room's input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoomBox {
    GameOptions,
}

#[derive(Debug, Clone)]
pub(crate) struct SetupView {
    pub(crate) first_row: usize,
    pub(crate) row_count: usize,
    /// The first row of each page.
    pub(crate) pages: Vec<usize>,
    /// The rows (headers too) on the page showing.
    pub(crate) page_len: usize,
    /// VERA's roster: how many places it scrolls through ([`RosterScroll`])
    /// and the last; [`Self::first_row`] is the place it shows.
    pub(crate) roster_scroll: (usize, usize),
    pub(crate) dialog: SetupDialog,
    pub(crate) host: bool,
    pub(crate) locked: bool,
    /// C2's Short Game is held on in a tournament or clan room.
    pub(crate) short_game_locked: bool,
    /// The team header `0x794`; hidden in a clan room.
    pub(crate) team_header: bool,
    pub(crate) accept_enabled: bool,
    pub(crate) start_requested: bool,
    pub(crate) combos: Vec<SetupCombo>,
    pub(crate) trackbars: Vec<SetupTrackbar>,
    pub(crate) checks: [bool; 5],
    /// [`ROOM_CHECKBOXES`]' states.
    pub(crate) room_checks: [bool; 7],
    /// Indexed by the row on the page, up to [`SetupDialog::rows_per_page`].
    pub(crate) row_icons: [Option<SetupRowIcon>; MODERN_SETUP_ROWS],
    /// VERA's header rows, by the row on the page.
    pub(crate) headers: [Option<SetupHeader>; MODERN_SETUP_ROWS],
    /// The box open over VERA's room.
    pub(crate) open_box: Option<RoomBox>,
    /// Team Starts: Teams is set and the map has starts to give.
    pub(crate) team_starts_enabled: bool,
    /// Team Colors: Teams is set.
    pub(crate) team_colors_enabled: bool,
    /// Online, the host's Balance: the room's mode seats players on teams.
    pub(crate) balance_enabled: bool,
    /// Online, Lobby: the lobby offers visits to the public lobby.
    pub(crate) lobby_enabled: bool,
    /// The rows the host may drag to another box, in a room with teams.
    pub(crate) drag_rows: Vec<DragRow>,
    /// VERA's missing-map offer is open over the room.
    pub(crate) map_offer_open: bool,
    pub(crate) mode_label: String,
    pub(crate) map_label: String,
    pub(crate) selected_map_index: Option<usize>,
    pub(crate) cooperative: bool,
    pub(crate) cooperative_description: Option<String>,
}
impl SetupView {
    /// Whether VERA's team bar shows: in a room with teams, and for the
    /// host's Room size.
    pub(crate) fn team_bar(&self) -> bool {
        self.trackbars
            .iter()
            .any(|bar| matches!(bar.id, TEAM_COUNT | ROOM_SIZE) && bar.visible)
    }
    pub(crate) fn page(&self) -> &'static MenuPageSpec {
        self.dialog.page()
    }
    pub(crate) fn combo(&self, id: u16) -> Option<&SetupCombo> {
        self.combos.iter().find(|combo| combo.id == id)
    }
    /// The box a dragged `row` lands in when dropped on the box whose
    /// header is page row `header`: a team or the one box of a room with
    /// Teams Off, the spectators for a guest; not the box it is in.
    pub(crate) fn drop_group(&self, row: u8, header: usize) -> Option<SetupGroup> {
        let drag = self.drag_rows.iter().find(|drag| drag.row == row)?;
        let group = self.headers.get(header)?.as_ref()?.group;
        let fits = match group {
            SetupGroup::Team(_) | SetupGroup::Players => true,
            SetupGroup::Spectators => drag.may_watch,
            SetupGroup::NoTeam | SetupGroup::ClosedSeats => false,
        };
        (fits && group != drag.group && !self.locked).then_some(group)
    }
    /// The header whose Join button is `id`, while it shows one.
    pub(crate) fn header_button(&self, id: u16) -> Option<&SetupHeader> {
        self.headers
            .get(team_join_row(id)?)?
            .as_ref()
            .filter(|header| header.join.is_some())
    }
    pub(crate) fn enabled(&self, id: u16) -> bool {
        if matches!(
            id,
            CHAT | CHAT_LOG | GAME_OPTIONS_OK | MAP_OFFER_DOWNLOAD | MAP_OFFER_LEAVE
        ) || (id == EMOTE && self.dialog.wol())
        {
            return true;
        }
        if self.locked {
            return false;
        }
        match id {
            BACK | CHAT | CHAT_LOG | GAME_OPTIONS => true,
            LOBBY => self.lobby_enabled,
            START | CHOOSE_MAP => self.host,
            TEAM_STARTS => self.team_starts_enabled,
            TEAM_COLORS => self.team_colors_enabled,
            BALANCE => self.balance_enabled,
            PAGE => self.dialog.wol(),
            PREVIOUS_ROWS => self.first_row > 0,
            NEXT_ROWS => self.first_row + self.page_len < self.row_count,
            ACCEPT => !self.host && self.accept_enabled,
            0x54E if self.short_game_locked => false,
            id if CHECKBOXES.contains(&id) || ROOM_CHECKBOXES.contains(&id) => self.host,
            id if team_join_row(id).is_some() => self
                .header_button(id)
                .is_some_and(|header| header.join_enabled),
            id => {
                self.combo(id)
                    .is_some_and(|combo| combo.visible && combo.enabled)
                    || self
                        .trackbars
                        .iter()
                        .any(|bar| bar.id == id && bar.visible && bar.enabled)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetupAction {
    Start,
    Accept,
    Back,
    ChooseMap,
    Select {
        control: u16,
        value: i32,
    },
    Toggle {
        control: u16,
    },
    Trackbar {
        control: u16,
        value: i32,
    },
    /// A header's Join, or the spectators' Watch, in VERA's room.
    Join {
        control: u16,
    },
    Chat(Vec<u16>),
    /// C2/C9 0x3ED sends the chat edit's text as an action message.
    Emote,
    Page,
    RosterPage {
        forward: bool,
    },
    /// A box over VERA's room opens, or the open one closes.
    OpenBox(Option<RoomBox>),
    /// The host's Team Starts in the Teams box.
    TeamStarts,
    /// The host's Team Colors in the Teams box.
    TeamColors,
    /// The host's Balance in the team bar, online.
    Balance,
    /// Lobby in an online room: the public lobby, the room keeping this
    /// player's place.
    Lobby,
    /// The missing-map offer's answer: download the room's map, or leave.
    MapOffer {
        download: bool,
    },
    /// The host drops a dragged roster row on another box.
    DropRow {
        row: u8,
        group: SetupGroup,
    },
}

/// VERA asks a guest before downloading a room map it does not have, then
/// shows the download's progress until the map arrives.
#[derive(Debug, Clone)]
pub(crate) struct MapOffer {
    pub(crate) name: String,
    pub(crate) message: String,
    /// Download was pressed; the map transfer takes it from here.
    pub(crate) downloading: bool,
    /// When it opened: keys typed for the chat just before do not answer it.
    pub(crate) opened: Instant,
}

impl MapOffer {
    /// How long after opening the offer ignores Enter and Escape.
    pub(crate) const KEY_DELAY: crate::util::time::Duration =
        crate::util::time::Duration::from_millis(750);

    /// Whether a key now answers it.
    pub(crate) fn takes_keys(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.opened) >= Self::KEY_DELAY
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MultiplayerSetupState {
    /// The first row of the page showing; in VERA's roster, the place it
    /// is scrolled to ([`RosterScroll`]).
    first_visible_row: usize,
    pub(crate) chat: LanChatState,
    pub(crate) notice: Option<SkirmishValidationModalState>,
    /// The row the open list belongs to. Joins, leaves, moves and paging
    /// put another row under its control, and a list whose row moved away
    /// closes, so a pick never changes whoever took its place.
    pub(crate) popup_row: Option<u8>,
    /// The host's drag of a roster row, while the button is down.
    pub(crate) row_drag: Option<RowDrag>,
    /// The control under the pointer as its last move, press or release
    /// left it: the one a press works in VERA's roster, which lights it;
    /// a button shows pushed only under it.
    pub(crate) hovered: Option<u16>,
    pub(crate) statics: SkirmishStatics,
    pub(crate) shown: bool,
    focus_started: Instant,
    accept_flash_started: Option<Instant>,
    /// The box open over VERA's room; it takes the room's input.
    pub(crate) open_box: Option<RoomBox>,
    /// VERA's missing-map offer; open until its map arrives, it takes the
    /// room's input.
    pub(crate) map_offer: Option<MapOffer>,
    /// The maps whose briefing VERA's room has looked for, by file name, so
    /// it points each one out once while the room stays open.
    briefings_checked: Vec<String>,
}
impl Default for MultiplayerSetupState {
    fn default() -> Self {
        Self {
            first_visible_row: 0,
            chat: {
                let mut chat = LanChatState::default();
                chat.edit.focused = true;
                chat
            },
            notice: None,
            popup_row: None,
            row_drag: None,
            hovered: None,
            statics: SkirmishStatics::default(),
            shown: false,
            focus_started: Instant::now(),
            accept_flash_started: None,
            open_box: None,
            map_offer: None,
            briefings_checked: Vec::new(),
        }
    }
}
impl MultiplayerSetupState {
    /// The page showing, of the pages starting at rows `pages`.
    fn page(&self, pages: &[usize]) -> usize {
        pages
            .iter()
            .rposition(|start| *start <= self.first_visible_row)
            .unwrap_or(0)
    }
    /// The first row of the page showing, of the pages starting at `pages`.
    pub(crate) fn first_row(&self, pages: &[usize]) -> usize {
        pages.get(self.page(pages)).copied().unwrap_or(0)
    }
    /// Shows the page of `row`, of pages `rows` long.
    pub(crate) fn show_row(&mut self, row: usize, rows: usize) {
        self.first_visible_row = row / rows * rows;
    }
    pub(crate) fn page_rows(&mut self, pages: &[usize], forward: bool) {
        let page = self.page(pages);
        let page = if forward {
            (page + 1).min(pages.len().saturating_sub(1))
        } else {
            page.saturating_sub(1)
        };
        self.first_visible_row = pages.get(page).copied().unwrap_or(0);
        self.cancel_pointer();
    }
    /// The host presses roster row `row` at `x`, `y`: it may drag it.
    pub(crate) fn press_row(&mut self, row: u8, x: i32, y: i32) {
        self.row_drag = Some(RowDrag {
            row,
            from: (x, y),
            at: (x, y),
            carrying: false,
            target: None,
        });
    }
    /// The pointer moves with a row pressed: once it has left the press the
    /// row is carried, over the box `target` gives. True when the carry
    /// starts now, which closes the list the press opened.
    pub(crate) fn drag_row(
        &mut self,
        x: i32,
        y: i32,
        target: impl FnOnce(u8) -> Option<usize>,
    ) -> bool {
        let Some(drag) = self.row_drag.as_mut() else {
            return false;
        };
        drag.at = (x, y);
        let starts = !drag.carrying
            && (x - drag.from.0).abs().max((y - drag.from.1).abs()) >= ROW_DRAG_START;
        drag.carrying |= starts;
        if starts {
            self.popup_row = None;
        }
        drag.target = if drag.carrying {
            target(drag.row)
        } else {
            None
        };
        starts
    }
    /// The button comes up: the row and the box it was carried to.
    pub(crate) fn drop_row(&mut self) -> Option<(u8, usize)> {
        let drag = self.row_drag.take()?;
        Some((drag.row, drag.target?))
    }
    /// The place VERA's roster shows, of the last `max_top`.
    pub(crate) fn roster_top(&self, max_top: usize) -> usize {
        self.first_visible_row.min(max_top)
    }
    /// VERA's roster scrolled to place `top` (its scrollbar).
    pub(crate) fn scroll_roster_to(&mut self, top: usize) {
        self.first_visible_row = top;
    }
    /// An open roster list owns the wheel even at its ends. Once it closes,
    /// the same gesture over the roster scrolls the teams again.
    pub(crate) fn wheel(
        &mut self,
        gui: &mut Gui,
        view: &SetupView,
        layout: &MultiplayerSetupLayout,
        x: i32,
        y: i32,
        lines: f32,
    ) -> bool {
        let Some(roster) = layout.roster_scroll(view) else {
            return false;
        };
        if view.open_box.is_some() || view.map_offer_open || lines == 0.0 {
            return false;
        }
        let step = ((lines * 3.0).round() as i32).clamp(-30, 30);
        let step = if step == 0 {
            lines.signum() as i32
        } else {
            step
        };
        if gui.wheel_popup(-step, x, y) {
            return true;
        }
        if layout.roster_area.contains(x, y) {
            let top = self.roster_top(roster.max_top);
            self.first_visible_row = top
                .saturating_add_signed(-step as isize)
                .min(roster.max_top);
            // Its rows' controls move under the pointer: a hidden list closes.
            if self.first_visible_row != top {
                gui.close_popup();
            }
            return true;
        }
        false
    }
    /// Whether the room looks for `file`'s briefing now: only the first time
    /// it shows that map.
    pub(crate) fn first_briefing_check(&mut self, file: &str) -> bool {
        if self
            .briefings_checked
            .iter()
            .any(|checked| checked.eq_ignore_ascii_case(file))
        {
            return false;
        }
        self.briefings_checked.push(file.to_owned());
        true
    }
    pub(crate) fn show_box(&mut self, open: Option<RoomBox>) {
        self.open_box = open;
        self.cancel_pointer();
    }
    pub(crate) fn accept_highlight(&mut self, enabled: bool, now: Instant) -> bool {
        if !enabled {
            self.accept_flash_started = None;
            return false;
        }
        crate::ui::shell::button::flash_phase(*self.accept_flash_started.get_or_insert(now), now)
    }
    pub(crate) fn caret_visible(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.focus_started).as_secs() % 2 == 0
    }
    pub(crate) fn focus_chat(&mut self) {
        self.chat.edit.focused = true;
    }
    pub(crate) fn cancel_pointer(&mut self) {
        self.row_drag = None;
        self.hovered = None;
    }
}

/// VERA's Game Options popup over the room ([`SetupDialog::modern`]): Red
/// Alert 2's 451x326 box, as the online lobby's popups use, with the room's
/// own option controls in its right half, clear of the art's Desolator.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GameOptionsLayout {
    pub(crate) dialog: RectPx,
    pub(crate) ok: RectPx,
}

/// Previous and Next under the roster: the size of the MNBTTN art the
/// shell's ordinary buttons draw, as Game Options' OK.
const ROSTER_BUTTON_W: i32 = 124;
const ROSTER_BUTTON_H: i32 = 24;

#[derive(Debug, Clone)]
pub(crate) struct MultiplayerSetupLayout {
    pub(crate) page: MenuPageLayout,
    /// VERA's roster ([`SetupDialog::modern`]): plain rows in one column,
    /// placed by [`Self::arrange_roster`].
    pub(crate) roster: bool,
    /// Rows past [`SetupDialog::rows_per_page`] are never shown. On a
    /// header row of VERA's roster, the first is its caption.
    pub(crate) combos: [[RectPx; 5]; MODERN_SETUP_ROWS],
    /// A header row's Join button and the host's Add AI list.
    pub(crate) header_controls: [(RectPx, RectPx); MODERN_SETUP_ROWS],
    /// In VERA's roster, each row's whole width in its column.
    pub(crate) roster_rows: [RectPx; MODERN_SETUP_ROWS],
    /// In VERA's roster, the box around each header's group, by the
    /// header's row: a press in it joins the group.
    pub(crate) group_boxes: [Option<RectPx>; MODERN_SETUP_ROWS],
    pub(crate) roster_navigation: [RectPx; 2],
    pub(crate) roster_page_label: RectPx,
    /// In VERA's roster, the area it scrolls in, its scrollbar at the right.
    pub(crate) roster_area: RectPx,
    /// In VERA's roster, where its entries' parts stand across it.
    roster_columns: RosterColumns,
    /// In VERA's room, the chat's column at 800x600: its left edge and
    /// width.
    /// Where VERA's roster's entries end, in 800x600 pixels: over the team
    /// bar in a room with one.
    pub(crate) roster_bottom: i32,
    pub(crate) flags: [RectPx; MODERN_SETUP_ROWS],
    pub(crate) acceptance: [RectPx; MODERN_SETUP_ROWS],
    /// C2/C9 emote button; `None` on BC/BD.
    pub(crate) emote: Option<RectPx>,
    pub(crate) chat: RectPx,
    pub(crate) chat_log: RectPx,
    /// In the room, or in [`Self::game_options`] when it is `Some`, with
    /// the trackbars of [`OPTION_TRACKBARS`] and [`Self::option_labels`].
    pub(crate) checkboxes: [RectPx; 5],
    /// [`ROOM_CHECKBOXES`], placed only in [`Self::game_options`].
    pub(crate) room_checkboxes: [RectPx; 7],
    /// [`SUPERWEAPON_LIMIT`]'s label, placed only in [`Self::game_options`].
    pub(crate) superweapon_limit_label: RectPx,
    /// VERA's team bar under the roster, with the Teams count's label, the
    /// host's Room size's, Team Starts and Team Colors and, online, Balance.
    pub(crate) team_bar: Option<RectPx>,
    pub(crate) teams_label: RectPx,
    pub(crate) room_size_label: RectPx,
    pub(crate) team_starts: Option<RectPx>,
    pub(crate) team_colors: Option<RectPx>,
    pub(crate) balance: Option<RectPx>,
    pub(crate) trackbars: [(u16, RectPx); 7],
    pub(crate) labels: Vec<(&'static str, RectPx)>,
    pub(crate) option_labels: [(&'static str, RectPx); 3],
    pub(crate) game_options: Option<GameOptionsLayout>,
    /// VERA's missing-map offer, a screen-centred two-button box.
    pub(crate) map_offer: crate::ui::shell::modal::QuitConfirmLayout,
    /// The OK of the notice 0xCE ([`MultiplayerSetupState::notice`]).
    pub(crate) notice_ok: RectPx,
    pub(crate) map_preview: RectPx,
    pub(crate) cooperative_description: RectPx,
    pub(crate) mode_label: RectPx,
    pub(crate) map_label: RectPx,
}
impl MultiplayerSetupLayout {
    pub(crate) fn new(dialog: SetupDialog, width: i32, height: i32, responsive: bool) -> Self {
        let host = dialog.host;
        let spec = dialog.page();
        let mut page = if responsive {
            // VERA's room fills the window: its shell stands at the right
            // edge, and its roster and chat spread over the rest.
            let scale = ShellScale::from_screen(width, height);
            menu_page::compute_layout_on(
                spec,
                if dialog.modern {
                    scale.anchored_right()
                } else {
                    scale
                },
            )
        } else {
            menu_page::compute_layout(spec, width as u32, height as u32)
        };
        let scale = page.scale;
        let place = Placer::on(scale, width, height);
        let rect = |x, y, w, h| place.px(x, y, w, h);
        // Resource templates of the anchored statics (BC/BD vs C2/C9).
        let (title, preview_x, label_x, map_label_y) = match (dialog.flavor, host) {
            (SetupFlavor::Lan, _) => (RectPx::new(425, 1, 108, 10), 429, 431, 116),
            (SetupFlavor::Wol, true) => (RectPx::new(424, 4, 108, 11), 432, 434, 118),
            (SetupFlavor::Wol, false) => (RectPx::new(424, 4, 108, 11), 433, 435, 118),
        };
        page.title = place.at(LOW_HEADING_ANCHOR, title.x, title.y, title.w, title.h);
        // Original BC/BD relayout60C4A0→60C0C0. Body controls stay at
        // resource coordinates; only the sidebar is anchored to screen size.
        page.status_help = place.at(AnchorRule::StatusLine, 2, 355, 410, 12);
        let combos = std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                let (x, w) = [(49, 100), (201, 78), (294, 27), (334, 25), (374, 25)][column];
                let window = child_window(x, 22 + row as i32 * 16, w, 74);
                scale.scale_rect(RectPx::new(window.x, window.y, window.w, 24))
            })
        });
        // C9's option block sits 1-2 px lower than C2/BC/BD in its resource.
        let wol_guest = dialog.wol() && !host;
        let option_y: [i32; 4] = if wol_guest {
            [254, 281, 309, 336]
        } else {
            [252, 280, 307, 335]
        };
        // BC/BD and C2 differ by one pixel in the credits/unit rows.
        let (credit_y, unit_y) = match (dialog.flavor, host) {
            (SetupFlavor::Lan, _) => (279, 306),
            (SetupFlavor::Wol, _) => (option_y[1], option_y[2]),
        };
        let (label_speed_y, label_credit_y, label_unit_y) = if wol_guest {
            (254, 281, 309)
        } else {
            (252, 280, 307)
        };
        let labels = [
            ("GUI:Players", 74, 11, 147, 17),
            ("GUI:Side", 302, 11, 111, 17),
            ("GUI:Color", 440, 11, 64, 17),
            ("GUI:StartPosition", 503, 11, 52, 17),
            ("GUI:Team", 560, 11, 52, 17),
        ]
        .into_iter()
        .map(|(s, x, y, w, h)| (s, rect(x, y, w, h)))
        .collect();
        let option_labels = [
            ("GUI:GameSpeed", rect(302, label_speed_y, 93, 17)),
            ("GUI:Credits", rect(302, label_credit_y, 91, 17)),
            ("GUI:UnitCount", rect(302, label_unit_y, 91, 17)),
        ];
        // C2's sixth row icons sit at 164 (template 101), C9's at 166.
        let icon_y = |row: usize| {
            if dialog.wol() && host && row == 5 {
                164
            } else {
                36 + row as i32 * 26
            }
        };
        let checkbox_x = if dialog.wol() { 72 } else { 71 };
        let build_off_ally_width = match (dialog.flavor, host) {
            (SetupFlavor::Lan, true) => 225,
            (SetupFlavor::Lan, false) => 234,
            (SetupFlavor::Wol, true) => 256,
            (SetupFlavor::Wol, false) => 261,
        };
        let (chat_y, chat_w, chat_log_w) = match (dialog.flavor, host) {
            (SetupFlavor::Lan, _) => (533, 609, 610),
            (SetupFlavor::Wol, true) => (533, 580, 612),
            (SetupFlavor::Wol, false) => (535, 580, 612),
        };
        // A header's Join over the Side column, the size of the MNBTTN art
        // as Previous and Next under the roster; the host's Add AI over the
        // Colour, Start and Team columns.
        let header_controls = std::array::from_fn(|row| {
            let [_, side, color, _, team]: [RectPx; 5] = combos[row];
            (
                scale.derive_rect(side, |mut rect| {
                    rect.w = ROSTER_BUTTON_W;
                    rect.h = ROSTER_BUTTON_H;
                    rect
                }),
                RectPx::new(color.x, color.y, team.x + team.w - color.x, color.h),
            )
        });
        let mut layout = Self {
            page,
            roster: false,
            combos,
            header_controls,
            roster_rows: [RectPx::new(0, 0, 0, 0); MODERN_SETUP_ROWS],
            group_boxes: [None; MODERN_SETUP_ROWS],
            // The native dialogs' rows leave 18 px above their options.
            roster_navigation: [
                rect(74, 230, ROSTER_BUTTON_W, 18),
                rect(611 - ROSTER_BUTTON_W, 230, ROSTER_BUTTON_W, 18),
            ],
            roster_page_label: rect(208, 230, 269, 18),
            roster_area: RectPx::new(0, 0, 0, 0),
            roster_columns: RosterColumns::new(RectPx::new(0, 0, ROSTER_W + ROSTER_SPARE_W, 0)),
            roster_bottom: ROSTER_BOTTOM,
            flags: std::array::from_fn(|row| rect(240, 36 + row as i32 * 26, 49, 21)),
            acceptance: std::array::from_fn(|row| {
                if dialog.wol() {
                    rect(50, icon_y(row), 16, 21)
                } else {
                    rect(42, icon_y(row), 28, 21)
                }
            }),
            emote: dialog.wol().then(|| rect(596, 533, 27, 24)),
            // The NewEdit paints inside its one-pixel resource frame.
            chat: rect(12, chat_y + 1, chat_w - 2, 19),
            chat_log: rect(11, 369, chat_log_w, 160),
            checkboxes: [
                rect(checkbox_x, option_y[0], 151, 17),
                rect(checkbox_x, option_y[1], 151, 17),
                rect(checkbox_x, option_y[2], 151, 17),
                rect(checkbox_x, option_y[3], 151, 17),
                rect(302, option_y[3], build_off_ally_width, 17),
            ],
            room_checkboxes: [RectPx::new(0, 0, 0, 0); 7],
            superweapon_limit_label: RectPx::new(0, 0, 0, 0),
            team_bar: None,
            teams_label: RectPx::new(0, 0, 0, 0),
            room_size_label: RectPx::new(0, 0, 0, 0),
            team_starts: None,
            team_colors: None,
            balance: None,
            trackbars: [
                (SPEED, rect(404, option_y[0], 129, 21)),
                (CREDITS, rect(404, credit_y, 129, 21)),
                (UNITS, rect(404, unit_y, 129, 21)),
                (
                    DIFFICULTY,
                    rect(180, 215, 266, if dialog.wol() && host { 21 } else { 22 }),
                ),
                (TEAM_COUNT, RectPx::new(0, 0, 0, 0)),
                (SUPERWEAPON_LIMIT, RectPx::new(0, 0, 0, 0)),
                (ROOM_SIZE, RectPx::new(0, 0, 0, 0)),
            ],
            labels,
            option_labels,
            game_options: None,
            map_offer: crate::ui::shell::modal::quit_confirm_layout(width, height),
            notice_ok: crate::ui::shell::modal::body_ok_layout(width, height).ok,
            cooperative_description: rect(60, 98, 540, 143),
            map_preview: place.at(AnchorRule::RightAnchor, preview_x, 23, 96, 69),
            mode_label: place.at(MAP_TEXT_ANCHOR, label_x, 103, 90, 10),
            map_label: place.at(MAP_TEXT_ANCHOR, label_x, map_label_y, 90, 20),
        };
        if dialog.modern {
            layout.move_options_into_popup(dialog.wol(), host);
        }
        layout
    }
    /// VERA's room: the roster's column and the chat's side by side across
    /// the window, the team bar under the roster, and the options in Game
    /// Options, centred over the columns.
    fn move_options_into_popup(&mut self, online: bool, host: bool) {
        let scale = self.page.scale;
        let base = |x, y, w, h| scale.scale_rect(RectPx::new(x, y, w, h));
        // The window in the shell's 800x600 pixels: on a wider one, the
        // shell stands at its right edge and the spare width is at the left.
        let window = scale.base_rect(scale.window());
        let left = window.x + ROOM_FRAME_W + ROOM_GAP;
        let right = window.x + window.w - ROOM_RIGHT_PANEL_W - ROOM_MARGIN;
        let width = right - left;
        let chat_w = (width * CHAT_SHARE.0 / CHAT_SHARE.1).clamp(CHAT_W.0, CHAT_W.1);
        let roster_w = width - ROOM_GAP - chat_w;
        let chat_x = right - chat_w;
        // Where the shell's menu boxes sit on the 800x600 page, centred
        // left of its right panel: as far again left as the window reaches.
        let options = crate::ui::shell::modal::menu_box(SHELL_BASE_W, SHELL_BASE_H)
            .translate(window.x / 2, 0);
        let popup = |x, y, w, h| base(options.x + x, options.y + y, w, h);
        let form_row = |index| crate::ui::shell::modal::game_options_form_row(options, index);
        // The same two columns as the checkboxes below: each trackbar's
        // label in the left one, the trackbar (its art's own width) starting
        // the right one.
        for (index, (_, label)) in self.option_labels.iter_mut().enumerate() {
            *label = scale.scale_rect(form_row(index).0);
        }
        for (id, bar) in &mut self.trackbars {
            if let Some(index) = OPTION_TRACKBARS.iter().position(|option| option == id) {
                *bar = scale.scale_rect(form_row(index).1);
            }
            // Superweapon Limit under Unit Count.
            if *id == SUPERWEAPON_LIMIT {
                *bar = scale.scale_rect(form_row(OPTION_TRACKBARS.len()).1);
            }
        }
        self.superweapon_limit_label = scale.scale_rect(form_row(OPTION_TRACKBARS.len()).0);
        // Two columns of six under the trackbars and Superweapon Limit, the
        // left one over the art: the native five and Destroyable Bridges,
        // then In-game Allying, the four bans and Terrain Deformation.
        let row = |index: i32| 154 + 22 * index;
        let left_option = |index| popup(24, row(index), 185, 17);
        let right_option = |index| popup(215, row(index), 226, 17);
        for (index, checkbox) in self.checkboxes.iter_mut().enumerate() {
            *checkbox = left_option(index as i32);
        }
        self.room_checkboxes = [
            left_option(5),
            right_option(0),
            right_option(1),
            right_option(2),
            right_option(3),
            right_option(4),
            right_option(5),
        ];
        self.game_options = Some(GameOptionsLayout {
            dialog: popup(0, 0, options.w, options.h),
            ok: popup(276, 292, 124, 24),
        });
        // The roster's column: the labels over its cells, the area it
        // scrolls in and, at its foot, the team bar.
        self.roster = true;
        let my = ROSTER_BOX_MARGIN.1;
        let columns = RosterColumns::new(RectPx::new(left, 0, roster_w, 0));
        let area = RectPx::new(
            left,
            ROSTER_TOP - my,
            roster_w,
            ROSTER_BOTTOM - ROSTER_TOP + 2 * my,
        );
        self.roster_area = scale.scale_rect(area);
        self.roster_columns = columns;
        self.labels = [
            "GUI:Players",
            "GUI:Side",
            "GUI:Color",
            "GUI:StartPosition",
            "GUI:Team",
        ]
        .into_iter()
        .zip(columns.cells)
        .map(|(key, (dx, w))| (key, base(columns.x + dx, ROSTER_LABEL_Y, w, ROSTER_LABEL_H)))
        .collect();
        self.arrange_team_bar(true, online, host, host);
        // The chat's column: the log from as high as the roster's area,
        // then the edit line, and online the emote button at its right end.
        self.chat_log = base(chat_x + 1, area.y, chat_w, CHAT_LOG_BOTTOM - area.y);
        let chat = scale.base_rect(self.chat);
        let emote_w = if online { 34 } else { 5 };
        self.chat = base(chat_x + 2, chat.y, chat_w - emote_w, chat.h);
        if let Some(emote) = &mut self.emote {
            *emote = scale.derive_rect(*emote, |emote| {
                RectPx::new(chat_x + chat_w - 26, emote.y, emote.w, emote.h)
            });
        }
        // The status line along the bottom from the window's left edge.
        self.page.status_help = scale.derive_rect(self.page.status_help, |line| {
            RectPx::new(window.x + line.x, line.y, line.w - window.x, line.h)
        });
    }
    /// The frame follows the roster's width. Only visible rows reserve
    /// height at its foot; widening the window keeps the controls in place.
    fn arrange_team_bar(&mut self, teams: bool, online: bool, host: bool, room_size: bool) {
        let scale = self.page.scale;
        let (mx, my) = ROSTER_BOX_MARGIN;
        let slider_rows = i32::from(teams) + i32::from(room_size);
        let rows = slider_rows + i32::from(teams && (host || online));
        let bar_h = rows * TEAM_BAR_ROW_H;
        let bar = RectPx::new(
            self.roster_columns.x - mx,
            ROSTER_BOTTOM + my - bar_h,
            self.roster_columns.w + 2 * mx,
            bar_h,
        );
        self.team_bar = (rows > 0).then(|| scale.scale_rect(bar));
        self.roster_bottom = if rows == 0 {
            ROSTER_BOTTOM
        } else {
            bar.y - ROSTER_GROUP_GAP - my
        };
        self.roster_area = scale.derive_rect(self.roster_area, |area| {
            RectPx::new(
                area.x,
                area.y,
                area.w,
                self.roster_bottom - ROSTER_TOP + 2 * my,
            )
        });

        let at = |part: RectPx, row| {
            scale.scale_rect(part.translate(bar.x + TEAM_BAR_INSET, bar.y + row * TEAM_BAR_ROW_H))
        };
        let small = self.control_scale(TEAM_COUNT);
        let zero = RectPx::new(0, 0, 0, 0);
        for (id, window) in &mut self.trackbars {
            let (visible, row, label) = match *id {
                TEAM_COUNT => (teams, 0, &mut self.teams_label),
                ROOM_SIZE => (room_size, i32::from(teams), &mut self.room_size_label),
                _ => continue,
            };
            if visible {
                *label = at(TEAM_BAR_LABEL, row);
                let corner = small.base_rect(at(TEAM_BAR_TRACKBAR, row));
                let (w, h) = TEAM_BAR_TRACKBAR_WINDOW;
                *window = small.scale_rect(RectPx::new(corner.x, corner.y, w, h));
            } else {
                (*label, *window) = (zero, zero);
            }
        }
        let button = |column| {
            let part = TEAM_BAR_BUTTON.translate(column * (TEAM_BAR_BUTTON.w + TEAM_BAR_GAP), 0);
            at(part, slider_rows)
        };
        self.team_starts = (teams && host).then(|| button(0));
        self.team_colors = (teams && host).then(|| button(1));
        self.balance = (teams && online).then(|| button(if host { 2 } else { 0 }));
    }
    /// VERA's team bar for the room: with or without teams, and with or
    /// without the host's Room size ([`Self::arrange_team_bar`]).
    pub(crate) fn fit_team_bar(&mut self, dialog: SetupDialog, teams: bool, room_size: bool) {
        if self.roster {
            self.arrange_team_bar(teams, dialog.wol(), dialog.host, room_size);
        }
    }
    /// VERA's roster: each entry showing where [`roster_page`] puts it, a
    /// header's caption, Add AI and Join along its band, and a row's cells
    /// along its line.
    pub(crate) fn arrange_roster(&mut self, places: &[RosterPlace]) {
        let scale = self.page.scale;
        let rect = |x, y, w, h| scale.scale_rect(RectPx::new(x, y, w, h));
        let columns = self.roster_columns;
        let x = columns.x;
        for (position, place) in places.iter().enumerate().take(MODERN_SETUP_ROWS) {
            let y = place.y;
            if place.header {
                self.combos[position][0] = rect(x, y, columns.caption_w, ROSTER_HEADER_H);
                let (dx, w) = columns.add_ai;
                self.header_controls[position] = (
                    rect(
                        x + columns.w - columns.join_w,
                        y,
                        columns.join_w,
                        ROSTER_HEADER_H,
                    ),
                    rect(x + dx, y, w, ROSTER_HEADER_H),
                );
                self.roster_rows[position] = rect(x, y, columns.w, ROSTER_HEADER_H);
            } else {
                self.combos[position] = columns
                    .cells
                    .map(|(dx, w)| rect(x + dx, y, w, ROSTER_ROW_H));
                self.roster_rows[position] = rect(x, y, columns.w, ROSTER_ROW_H);
            }
        }
        // Each header's group: it and the rows under it.
        for (position, place) in places.iter().enumerate().take(MODERN_SETUP_ROWS) {
            if !place.header {
                continue;
            }
            let last = places[position + 1..]
                .iter()
                .take_while(|next| !next.header)
                .last()
                .map_or(place.y + ROSTER_HEADER_H, |row| row.y + ROSTER_ROW_H);
            let (mx, my) = ROSTER_BOX_MARGIN;
            self.group_boxes[position] = Some(rect(
                x - mx,
                place.y - my,
                columns.w + 2 * mx,
                last - place.y + 2 * my,
            ));
        }
    }
    /// The page row of the header whose box in VERA's roster holds `x`, `y`.
    pub(crate) fn group_box_at(&self, x: i32, y: i32) -> Option<usize> {
        self.group_boxes
            .iter()
            .position(|rect| rect.is_some_and(|rect| rect.contains(x, y)))
    }
    /// Make room for navigation only on an extended roster. Resource placement
    /// of all ordinary <=8-player dialogs remains unchanged.
    pub(crate) fn arrange_extended_rows(&mut self) {
        let scale = self.page.scale;
        for row in 0..MODERN_SETUP_ROWS {
            let shift = row as i32 * 2;
            let move_up = |rect| {
                scale.derive_rect(rect, |mut rect| {
                    rect.y -= shift;
                    rect
                })
            };
            for rect in &mut self.combos[row] {
                *rect = move_up(*rect);
            }
            let (join, add_ai) = self.header_controls[row];
            self.header_controls[row] = (move_up(join), move_up(add_ai));
            self.flags[row] = move_up(self.flags[row]);
            self.acceptance[row] = move_up(self.acceptance[row]);
        }
    }
    pub(crate) fn control_rect(&self, id: u16) -> Option<RectPx> {
        if let Some((column, row)) = SetupColumn::decode(id) {
            return Some(self.combos[row][column as usize]);
        }
        // In VERA's roster, a group's Join is its whole box.
        if let Some(row) = team_join_row(id) {
            return Some(self.group_boxes[row].unwrap_or(self.header_controls[row].0));
        }
        if let Some(row) = team_add_ai_row(id) {
            return Some(self.header_controls[row].1);
        }
        if let Some(index) = CHECKBOXES.iter().position(|candidate| *candidate == id) {
            return Some(self.checkboxes[index]);
        }
        if let Some(index) = ROOM_CHECKBOXES
            .iter()
            .position(|candidate| *candidate == id)
        {
            return Some(self.room_checkboxes[index]);
        }
        if let Some((_, rect)) = self
            .trackbars
            .iter()
            .find(|(candidate, _)| *candidate == id)
        {
            return Some(*rect);
        }
        match id {
            PREVIOUS_ROWS => Some(self.roster_navigation[0]),
            NEXT_ROWS => Some(self.roster_navigation[1]),
            CHAT => Some(self.chat),
            CHAT_LOG => Some(self.chat_log),
            EMOTE => self.emote,
            GAME_OPTIONS_OK => self.game_options.map(|popup| popup.ok),
            TEAM_STARTS => self.team_starts,
            TEAM_COLORS => self.team_colors,
            BALANCE => self.balance,
            MAP_OFFER_DOWNLOAD => Some(self.map_offer.ok),
            crate::ui::shell::modal::control::OK => Some(self.notice_ok),
            MAP_OFFER_LEAVE => Some(self.map_offer.cancel),
            _ => self.page.button_rect(id),
        }
    }
    /// The screen VERA's team bar's controls draw and take the pointer at:
    /// 78% of the shell's ([`Self::control_scale`]).
    pub(crate) fn team_bar_screen(&self) -> Option<(i32, i32)> {
        let window = self.page.scale.window();
        self.team_bar
            .map(|_| (window.w * 78 / 100, window.h * 78 / 100))
    }
    /// The scale `id`'s art draws and takes the pointer at: the team bar's
    /// controls at 78% of the shell's.
    pub(crate) fn control_scale(&self, id: u16) -> ShellScale {
        match self.team_bar_screen() {
            Some((w, h))
                if matches!(
                    id,
                    TEAM_COUNT | ROOM_SIZE | TEAM_STARTS | TEAM_COLORS | BALANCE
                ) =>
            {
                ShellScale::from_screen(w, h)
            }
            _ => self.page.scale,
        }
    }
    /// The list cell `id` of VERA's roster opens: on rows as tall as its
    /// own, as wide as its cell or, if wider, its widest text and any
    /// scrollbar.
    fn roster_list(&self, id: u16, combo: &SetupCombo, cap: usize) -> Option<ComboPopup> {
        let scale = self.page.scale;
        let face = self.control_rect(id)?;
        let row_h = scale.scale_rect(RectPx::new(0, 0, 0, ROSTER_ROW_H)).h;
        let mut popup =
            ComboPopup::new(face, scale, combo.choices.len(), cap, 0).with_row_height(row_h);
        let text_w = scale.scale_rect(RectPx::new(0, 0, combo.list_w, 0)).w;
        let bar_w = popup.scrollbar().map_or(0, |bar| bar.w);
        popup.rect.w = popup.rect.w.max(text_w + bar_w);
        Some(popup)
    }
    /// The chat's text, as a share of the shell's own size: the roster's
    /// in VERA's room.
    pub(crate) fn chat_text_scale(&self) -> f32 {
        if self.roster { ROSTER_TEXT_SCALE } else { 1.0 }
    }
    /// The chat log's list, its rows at the chat's text size.
    pub(crate) fn chat_list(&self, list: &LanListState, count: usize) -> ShellListGeometry {
        list.geometry_for_text(
            self.chat_log,
            count,
            self.page.scale,
            self.chat_text_scale(),
        )
    }
    /// VERA's roster's scroll geometry: its scrollbar shows when its
    /// entries do not all fit.
    pub(crate) fn roster_scroll(&self, view: &SetupView) -> Option<ShellListGeometry> {
        let (count, max_top) = view.roster_scroll;
        self.roster.then(|| {
            ShellListGeometry::with_visible_rows(
                self.roster_area,
                count,
                count - max_top,
                view.first_row,
                ListMetrics::scaled(self.page.scale),
            )
        })
    }
}

/// VERA's roster's scrollbar: a list whose rows are the places it scrolls
/// to ([`RosterScroll`]), hit only on its bar.
pub(crate) const ROSTER_SCROLL: u16 = 0x7F17;
/// The boxes over VERA's room in the shell controller, which take the
/// room's input while they are open.
pub(crate) const GAME_OPTIONS_BOX: DialogId = DialogId(GAME_OPTIONS);
pub(crate) const MAP_OFFER_BOX: DialogId = DialogId(MAP_OFFER_DOWNLOAD);
/// The shared message box (`0x005D3490`) over the room, which owns all
/// input while it runs.
pub(crate) const NOTICE_BOX: DialogId = DialogId(0xCE);

fn node(kind: NodeKind, id: u16) -> NodeSpec {
    NodeSpec::new(kind).id(i32::from(id))
}

/// The room's widgets in the order a press finds them (no two overlap but a
/// roster cell and its group's box, the cell first): the rows' combos, the
/// options, the buttons, the chat and the roster's scrollbar. Each owner-draw
/// checkbox toggles on the press (`0x006163A0`).
fn page_spec(dialog: SetupDialog) -> NodeSpec {
    let rows = 0..dialog.rows_per_page();
    let combos = rows.clone().flat_map(move |row| {
        let add_ai = dialog.modern.then(|| team_add_ai(row));
        SetupColumn::ALL
            .map(|column| column.control(row))
            .into_iter()
            .chain(add_ai)
    });
    let buttons = rows
        .map(team_join)
        .chain([
            TEAM_STARTS,
            TEAM_COLORS,
            BALANCE,
            PREVIOUS_ROWS,
            NEXT_ROWS,
            EMOTE,
        ])
        .chain(dialog.page().buttons().map(|button| button.id));
    let list = |id| {
        node(NodeKind::List, id)
            .with(Prop::ListWindow, Value::Bool(true))
            .with(Prop::RowCount, Value::Int(0))
    };
    let help = tooltip_key(dialog, CHAT).unwrap_or_default();
    // VERA's room keeps its options in Game Options; Difficulty stays.
    let modern = dialog.modern;
    let native = move |ids: &'static [u16]| ids.iter().copied().filter(move |_| !modern);
    let trackbars = native(&OPTION_TRACKBARS).chain([DIFFICULTY, TEAM_COUNT, ROOM_SIZE]);
    NodeSpec::new(NodeKind::Dialog)
        .children(combos.map(|id| {
            node(NodeKind::Combo, id).with(Prop::OpenAnywhere, Value::Bool(dialog.modern))
        }))
        .children(option_spec(native(&CHECKBOXES), trackbars))
        .children(buttons.map(|id| node(NodeKind::Button, id)))
        .child(node(NodeKind::Text, CHAT).tooltip(&help))
        .child(list(CHAT_LOG))
        .child(list(ROSTER_SCROLL))
}

/// Checkboxes and trackbars, Difficulty's without its value plaque.
fn option_spec(
    checkboxes: impl IntoIterator<Item = u16>,
    trackbars: impl IntoIterator<Item = u16>,
) -> impl Iterator<Item = NodeSpec> {
    let checkboxes = checkboxes
        .into_iter()
        .map(|id| node(NodeKind::Checkbox, id).with(Prop::ActivateOnPress, Value::Bool(true)));
    let trackbars = trackbars.into_iter().map(|id| {
        node(NodeKind::Trackbar, id).with(Prop::ShowValue, Value::Bool(id != DIFFICULTY))
    });
    checkboxes.chain(trackbars)
}

/// VERA's Game Options box: the room's options and its OK.
fn game_options_spec() -> NodeSpec {
    let checkboxes = CHECKBOXES.into_iter().chain(ROOM_CHECKBOXES);
    let trackbars = OPTION_TRACKBARS.into_iter().chain([SUPERWEAPON_LIMIT]);
    NodeSpec::new(NodeKind::Dialog)
        .children(option_spec(checkboxes, trackbars))
        .child(node(NodeKind::Button, GAME_OPTIONS_OK))
}

fn buttons(ids: &'static [u16]) -> impl Fn() -> NodeSpec {
    move || {
        NodeSpec::new(NodeKind::Dialog).children(ids.iter().map(|id| node(NodeKind::Button, *id)))
    }
}

/// Control `id`'s rect while it shows: a combo for a shown row, a trackbar
/// the view shows, a header's Join while it has one, the team bar's buttons
/// with the bar, Previous and Next on a roster of pages while they lead to
/// one, and VERA's roster's scrollbar.
fn control_rect(view: &SetupView, layout: &MultiplayerSetupLayout, id: u16) -> Option<RectPx> {
    let shows = match id {
        ROSTER_SCROLL => return layout.roster.then_some(layout.roster_area),
        TEAM_STARTS | TEAM_COLORS | BALANCE => view.team_bar(),
        PREVIOUS_ROWS | NEXT_ROWS => view.pages.len() > 1 && view.enabled(id),
        id if team_join_row(id).is_some() => view.header_button(id).is_some(),
        id if view.combo(id).is_some() => view.combo(id).is_some_and(|combo| combo.visible),
        id if layout.trackbars.iter().any(|(bar, _)| *bar == id) => {
            view.trackbars.iter().any(|bar| bar.id == id && bar.visible)
        }
        id => SetupColumn::decode(id).is_none() && team_add_ai_row(id).is_none(),
    };
    layout.control_rect(id).filter(|_| shows)
}

/// Builds the room's widgets under the controller's page dialog (made the
/// active dialog first with `activate`) and the boxes open over it, puts
/// them on `layout` and shows the session; closes a list whose row moved
/// away. The page's root.
pub(crate) fn sync(
    controller: &mut DialogController,
    ui: &MultiplayerSetupState,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    activate: bool,
) -> Option<NodeId> {
    let page = view.page().dialog;
    if controller.root_in(page).is_none() {
        if !activate {
            return None;
        }
        controller.ensure_active(page, false);
    }
    for (dialog, open) in [
        (GAME_OPTIONS_BOX, view.open_box.is_some()),
        (MAP_OFFER_BOX, view.map_offer_open),
        (NOTICE_BOX, ui.notice.is_some()),
    ] {
        match (open, controller.root_in(dialog)) {
            (true, None) => controller.push_modal(dialog, false),
            (false, Some(_)) => controller.close(dialog),
            _ => {}
        }
    }
    let scale = layout.page.scale;
    let room = view.dialog;
    let spec_page = move || page_spec(room);
    let roots: [(DialogId, &dyn Fn() -> NodeSpec); 4] = [
        (page, &spec_page),
        (GAME_OPTIONS_BOX, &game_options_spec),
        (
            MAP_OFFER_BOX,
            &buttons(&[MAP_OFFER_DOWNLOAD, MAP_OFFER_LEAVE]),
        ),
        (
            NOTICE_BOX,
            &buttons(&[crate::ui::shell::modal::control::OK]),
        ),
    ];
    for (dialog, spec) in roots {
        let Some(root) = controller.root_in(dialog) else {
            continue;
        };
        controller.set_scale_in(dialog, scale);
        let gui = controller.gui_mut();
        page_nodes::ensure_built(gui, root, spec);
        page_nodes::sync_rects(gui, root, |id| control_rect(view, layout, id));
        let nodes: Vec<(NodeId, u16)> = gui
            .tree()
            .children(root)
            .map(|children| children.to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|node| Some((node, u16::try_from(gui.id_of(node)?).ok()?)))
            .collect();
        for (node, id) in nodes {
            for (prop, value) in props(ui, view, layout, id) {
                let _ = gui.set(node, prop, value);
            }
        }
    }
    let root = controller.root_in(page)?;
    let on_top = controller.top_id() == Some(page);
    let gui = controller.gui_mut();
    // A list hides while its combo does not take it, and shows again as it
    // was once it does.
    if let Some(node) = gui.popup().filter(|_| on_top) {
        let id = gui.id_of(node).and_then(|id| u16::try_from(id).ok());
        let shows = id.and_then(|id| view.combo(id)).is_some_and(|combo| {
            !view.locked
                && combo.visible
                && combo.enabled
                && ui.popup_row.is_none_or(|row| row == combo.row)
        });
        let _ = gui.set(node, Prop::PopupHidden, Value::Bool(!shows));
    }
    Some(root)
}

/// What control `id` shows of the session.
fn props(
    ui: &MultiplayerSetupState,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    id: u16,
) -> Vec<(Prop, Value)> {
    // VERA's roster scrolls under a lock too.
    let enabled = id == ROSTER_SCROLL || view.enabled(id);
    let mut props = vec![(Prop::Enabled, Value::Bool(enabled))];
    let scale = layout.page.scale;
    if let Some(combo) = view.combo(id) {
        // The rows its list shows before it scrolls; VERA's room shows its
        // Player and Add AI menus whole.
        let cap = match SetupColumn::decode(id) {
            Some((SetupColumn::Player, _)) if view.dialog.modern => 0,
            Some((column, _)) => column.visible_rows(),
            None => 0,
        };
        props.extend([
            (Prop::RowCount, Value::Int(combo.choices.len() as i32)),
            (
                Prop::Selected,
                Value::Int(combo.selected.map_or(-1, |row| row as i32)),
            ),
            (Prop::VisibleRows, Value::Int(cap as i32)),
            (Prop::PopupHidden, Value::Bool(false)),
        ]);
        // VERA's roster draws no faces: a press anywhere on a cell opens its
        // list while it has something else to choose; any other press goes
        // through to its group's box.
        if layout.roster {
            let opens = view.enabled(id) && roster_cell_opens(combo);
            props.push((Prop::PressThrough, Value::Bool(!opens)));
            if let Some(list) = layout.roster_list(id, combo, cap) {
                props.extend([
                    (Prop::PopupRowHeight, Value::Int(list.row_height)),
                    (Prop::PopupRect, Value::Rect(list.rect)),
                ]);
            }
        }
    } else if let Some(index) = CHECKBOXES.iter().position(|check| *check == id) {
        props.push((Prop::Checked, Value::Bool(view.checks[index])));
    } else if let Some(index) = ROOM_CHECKBOXES.iter().position(|check| *check == id) {
        props.push((Prop::Checked, Value::Bool(view.room_checks[index])));
    } else if let Some(bar) = view.trackbars.iter().find(|bar| bar.id == id) {
        props.extend([
            (Prop::Min, Value::Int(bar.range.min)),
            (Prop::Max, Value::Int(bar.range.max)),
            (Prop::Step, Value::Int(bar.range.step)),
            (Prop::Pos, Value::Int(bar.value)),
        ]);
        if let Some((w, h)) = layout
            .team_bar_screen()
            .filter(|_| layout.control_scale(id) != scale)
        {
            props.push((Prop::ScaleScreen, Value::Rect(RectPx::new(0, 0, w, h))));
        }
    } else if id == CHAT_LOG {
        let row_h = ListMetrics::for_text(scale, layout.chat_text_scale()).row_h;
        props.extend([
            (Prop::RowCount, Value::Int(ui.chat.lines.len() as i32)),
            (Prop::Top, Value::Int(ui.chat.list.top_index as i32)),
            (Prop::Selected, Value::Int(-1)),
            (Prop::RowHeight, Value::Int(row_h)),
        ]);
    } else if id == ROSTER_SCROLL {
        let (count, max_top) = view.roster_scroll;
        let bar = layout
            .roster_scroll(view)
            .and_then(|roster| roster.scrollbar);
        props.extend([
            (Prop::RowCount, Value::Int(count as i32)),
            (Prop::VisibleRows, Value::Int((count - max_top) as i32)),
            (Prop::Top, Value::Int(view.first_row as i32)),
            (Prop::HitRect, Value::Rect(bar.unwrap_or_default())),
        ]);
    }
    props
}

/// A left press at `x`, `y` on the room's top dialog: the widgets take it.
/// A press on the chat focuses it; a list the press opens belongs to its
/// row, and on VERA's roster the host may carry the row away from it.
pub(crate) fn press(
    controller: &mut DialogController,
    ui: &mut MultiplayerSetupState,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    x: i32,
    y: i32,
) {
    let page = view.page().dialog;
    let gui = controller.gui();
    let before = open_list(gui);
    let target = controller
        .root_in(page)
        .filter(|_| before.is_none() && controller.top_id() == Some(page))
        .and_then(|root| gui.hit_at(root, x, y, true));
    let chat = target.and_then(|node| gui.id_of(node)) == Some(CHAT.into());
    // A press on the combo of a hidden list opens it afresh.
    let reopens = target.is_some() && gui.popup() == target;
    controller.on_pointer_down(x, y, &[]);
    if chat {
        ui.focus_chat();
    }
    if reopens && let Some(node) = target {
        let _ = controller
            .gui_mut()
            .set(node, Prop::PopupHidden, Value::Bool(false));
    }
    if before.is_none()
        && let Some((id, _)) = open_list(controller.gui())
        && let Some(combo) = view.combo(id)
        && layout.roster
    {
        ui.popup_row = Some(combo.row);
        if view.drag_rows.iter().any(|drag| drag.row == combo.row) {
            ui.press_row(combo.row, x, y);
        }
    }
}

/// The control of the top dialog `root` under the pointer: the one whose
/// status help shows (a disabled one too, and under an open list), or with
/// `press`, the one a press works.
pub(crate) fn control_at(gui: &Gui, root: NodeId, x: i32, y: i32, press: bool) -> Option<u16> {
    u16::try_from(gui.id_of(gui.hit_at(root, x, y, press)?)?).ok()
}

/// The combo whose list is open over the room, and the list as the pointer
/// hits it.
pub(crate) fn open_list(gui: &Gui) -> Option<(u16, vera20k_gui::ComboDropDown)> {
    let node = gui.popup()?;
    Some((
        u16::try_from(gui.id_of(node)?).ok()?,
        gui.combo_drop_down(node)?,
    ))
}

/// What the room paints of its widgets: the buttons shown pushed, the
/// control a press at the pointer works (VERA's roster lights it), the open
/// list, and the scrollbar arrows held.
#[derive(Default)]
pub(crate) struct RoomLook {
    pressed: Vec<u16>,
    pub(crate) target: Option<u16>,
    pub(crate) open: Option<(u16, vera20k_gui::ComboDropDown)>,
    pub(crate) roster_scroll: Option<ListScrollPart>,
    pub(crate) chat_scroll: Option<ListScrollPart>,
    /// The notice's OK holds the pointer.
    pub(crate) notice_ok: bool,
}
impl RoomLook {
    /// Whether button `id` shows pushed: captured, the pointer still on it.
    pub(crate) fn pressed(&self, id: u16) -> bool {
        self.pressed.contains(&id)
    }
}

pub(crate) fn look(
    controller: &DialogController,
    ui: &MultiplayerSetupState,
    view: &SetupView,
) -> RoomLook {
    let gui = controller.gui();
    let page = view.page().dialog;
    let mut look = RoomLook {
        notice_ok: controller.pressed_node_in(NOTICE_BOX).is_some(),
        target: ui.hovered,
        ..RoomLook::default()
    };
    for dialog in [page, GAME_OPTIONS_BOX, MAP_OFFER_BOX] {
        let pressed = controller
            .pressed_node_in(dialog)
            .and_then(|node| gui.id_of(node));
        look.pressed.extend(
            pressed
                .and_then(|id| u16::try_from(id).ok())
                .filter(|id| ui.hovered == Some(*id)),
        );
    }
    let Some(root) = controller.root_in(page) else {
        return look;
    };
    look.open = (controller.top_id() == Some(page))
        .then(|| open_list(gui))
        .flatten();
    let scroll = |id: u16| {
        let node = gui.find_id(root, i32::from(id))?;
        gui.list_view(node)?.pressed
    };
    look.roster_scroll = scroll(ROSTER_SCROLL);
    look.chat_scroll = scroll(CHAT_LOG);
    look
}

/// One thing the room's widgets did, in order: a cue, or what the session
/// is asked to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RoomEvent {
    Cue(PageCue),
    Action(SetupAction),
    /// The notice's OK.
    Acknowledge,
}

/// The widgets' events since the last drain as cues and session actions;
/// the scrollbars' new tops go to the roster and the chat log.
pub(crate) fn room_events(
    controller: &mut DialogController,
    ui: &mut MultiplayerSetupState,
    view: &SetupView,
) -> Vec<RoomEvent> {
    let raw = controller.drain_events();
    let gui = controller.gui();
    let root = controller.root_in(view.page().dialog);
    let top = |id: u16| root.and_then(|root| page_nodes::int(gui, root, id, Prop::Top));
    let mut out = Vec::new();
    let mut scrolled = false;
    for event in page_nodes::page_events(gui, raw) {
        let action = match event {
            // The chat log's rows take no selection.
            PageEvent::Cue(PageCue::ListClick) => None,
            PageEvent::Cue(cue) => {
                out.push(RoomEvent::Cue(cue));
                None
            }
            PageEvent::Activated(crate::ui::shell::modal::control::OK) => {
                out.push(RoomEvent::Acknowledge);
                None
            }
            PageEvent::Activated(control) => match control {
                START => Some(SetupAction::Start),
                ACCEPT => Some(SetupAction::Accept),
                BACK => Some(SetupAction::Back),
                CHOOSE_MAP => Some(SetupAction::ChooseMap),
                EMOTE => Some(SetupAction::Emote),
                PAGE => Some(SetupAction::Page),
                PREVIOUS_ROWS => Some(SetupAction::RosterPage { forward: false }),
                NEXT_ROWS => Some(SetupAction::RosterPage { forward: true }),
                GAME_OPTIONS => Some(SetupAction::OpenBox(Some(RoomBox::GameOptions))),
                GAME_OPTIONS_OK => Some(SetupAction::OpenBox(None)),
                TEAM_STARTS => Some(SetupAction::TeamStarts),
                TEAM_COLORS => Some(SetupAction::TeamColors),
                BALANCE => Some(SetupAction::Balance),
                LOBBY => Some(SetupAction::Lobby),
                MAP_OFFER_DOWNLOAD => Some(SetupAction::MapOffer { download: true }),
                MAP_OFFER_LEAVE => Some(SetupAction::MapOffer { download: false }),
                control if team_join_row(control).is_some() => Some(SetupAction::Join { control }),
                _ => None,
            },
            PageEvent::Toggled(control, _) => Some(SetupAction::Toggle { control }),
            PageEvent::Picked(control, row) => view
                .combo(control)
                .and_then(|combo| combo.choices.get(usize::try_from(row).ok()?))
                .map(|choice| SetupAction::Select {
                    control,
                    value: choice.value,
                }),
            PageEvent::Slid {
                id,
                value,
                final_: false,
            } => Some(SetupAction::Trackbar { control: id, value }),
            // Its rows' controls move under the pointer, so an open list
            // closes when it scrolls.
            PageEvent::Scrolled(ROSTER_SCROLL) => {
                ui.scroll_roster_to(top(ROSTER_SCROLL).unwrap_or(0).max(0) as usize);
                scrolled = true;
                None
            }
            PageEvent::Scrolled(CHAT_LOG) => {
                ui.chat.list.top_index = top(CHAT_LOG).unwrap_or(0).max(0) as usize;
                None
            }
            _ => None,
        };
        out.extend(action.map(RoomEvent::Action));
    }
    if scrolled {
        controller.gui_mut().close_popup();
    }
    out
}

/// OwnerDraw__GetTooltipStringLabel6040B0 BC/BD and C2/C9 branches. Guest
/// controls retain their own help even while disabled; no invented fallback.
pub(crate) fn tooltip_key(dialog: SetupDialog, control: u16) -> Option<String> {
    let host = dialog.host;
    if dialog.wol() {
        match control {
            BACK => return Some("STT:WOLNewGameCancel".into()),
            PAGE => return Some("STT:WOLPageButton".into()),
            EMOTE => return Some("STT:EmoteButton".into()),
            _ => {}
        }
    }
    let suffix = if let Some((column, _)) = SetupColumn::decode(control) {
        match column {
            SetupColumn::Player => "ComboPlayer",
            SetupColumn::Country => "ComboCountry",
            SetupColumn::Color => "ComboColor",
            SetupColumn::Start => "ComboStart",
            SetupColumn::Team => "ComboTeam",
        }
    } else {
        match control {
            SPEED => "SliderSpeed",
            CREDITS => "SliderCredits",
            UNITS => "SliderUnit",
            0x3E8 => "CBoxRedeploys",
            0x54E => "CBoxShortGame",
            0x69A => "CBoxSWAllowed",
            0x69D => "CBoxBuildOffAlly",
            0x54D => "CBoxCrates",
            CHAT => "EditInput",
            CHAT_LOG => "EditOutput",
            START => "ButtonGo",
            ACCEPT => "ButtonAccept",
            CHOOSE_MAP => "ButtonChooseMap",
            BACK => "ButtonBack",
            MAP_PREVIEW => "MapThumbnail",
            0x5A8 => "LabelScenario",
            0x6D2..=0x6D9 => "PictureAcceptance",
            0x6DA..=0x6E1 => "PictureFlag",
            _ => return None,
        }
    };
    Some(format!(
        "STT:{}{}",
        if host { "Host" } else { "Guest" },
        suffix
    ))
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    fn dialog(host: bool, flavor: SetupFlavor, modern: bool) -> SetupDialog {
        SetupDialog {
            host,
            flavor,
            modern,
        }
    }

    /// The room's widgets in a fresh shell controller.
    fn widgets(
        ui: &MultiplayerSetupState,
        view: &SetupView,
        layout: &MultiplayerSetupLayout,
    ) -> DialogController {
        let mut controller = DialogController::default();
        sync(&mut controller, ui, view, layout, true);
        controller
    }

    /// The control of the top dialog at `x`, `y`: with `press`, the one a
    /// press works, else the one whose help shows.
    fn hit(controller: &DialogController, (x, y): (i32, i32), press: bool) -> Option<u16> {
        let root = controller.root_in(controller.top_id()?)?;
        control_at(controller.gui(), root, x, y, press)
    }

    #[test]
    fn the_room_looks_for_each_maps_briefing_once() {
        let mut room = MultiplayerSetupState::default();
        assert!(room.first_briefing_check("normandy.yrm"));
        assert!(!room.first_briefing_check("NORMANDY.YRM"), "the same map");
        assert!(room.first_briefing_check("tour.yrm"));
        assert!(
            !room.first_briefing_check("normandy.yrm"),
            "back to the first"
        );
    }

    /// Native pages hold eight rows, whose last still clears the navigation
    /// under it.
    #[test]
    fn roster_paging_reaches_last_player_and_clears_stale_popup() {
        let mut ui = MultiplayerSetupState::default();
        let pages: Vec<usize> = (0..30).step_by(VISIBLE_SETUP_ROWS).collect();
        ui.show_row(29, VISIBLE_SETUP_ROWS);
        assert_eq!(ui.first_row(&pages), 24);
        ui.page_rows(&pages, false);
        assert_eq!(ui.first_row(&pages), 16);
        ui.page_rows(&pages, true);
        ui.page_rows(&pages, true);
        assert_eq!(ui.first_row(&pages), 24);
        assert_eq!(ui.first_row(&[0]), 0, "small maps return to the first page");
        let mut layout =
            MultiplayerSetupLayout::new(dialog(true, SetupFlavor::Lan, false), 800, 600, false);
        layout.arrange_extended_rows();
        let last = layout.combos[VISIBLE_SETUP_ROWS - 1][0];
        assert!(last.y + last.h <= layout.roster_navigation[0].y);
        // The rows past the resource's eight decode back to their own row,
        // clear of the headers' Join and Add AI.
        for row in VISIBLE_SETUP_ROWS..MODERN_SETUP_ROWS {
            for column in SetupColumn::ALL {
                let id = column.control(row);
                assert_eq!(SetupColumn::decode(id), Some((column, row)));
                assert!(team_join_row(id).is_none() && team_add_ai_row(id).is_none());
            }
        }
    }

    /// VERA's online room for its host, with nothing in it.
    fn vera_room_view() -> SetupView {
        SetupView {
            first_row: 0,
            row_count: 8,
            pages: vec![0],
            page_len: 8,
            roster_scroll: (0, 0),
            dialog: dialog(true, SetupFlavor::Wol, true),
            host: true,
            locked: false,
            short_game_locked: false,
            team_header: true,
            accept_enabled: false,
            start_requested: false,
            combos: Vec::new(),
            trackbars: Vec::new(),
            checks: [false; 5],
            room_checks: [false; 7],
            row_icons: std::array::from_fn(|_| None),
            headers: std::array::from_fn(|_| None),
            open_box: None,
            team_starts_enabled: true,
            team_colors_enabled: true,
            balance_enabled: true,
            lobby_enabled: false,
            drag_rows: Vec::new(),
            map_offer_open: false,
            mode_label: String::new(),
            map_label: String::new(),
            selected_map_index: None,
            cooperative: false,
            cooperative_description: None,
        }
    }

    /// Lobby stays off in a room whose lobby does not offer visits, as one
    /// from before them does not.
    #[test]
    fn lobby_needs_a_lobby_that_offers_visits() {
        let mut view = vera_room_view();
        assert!(!view.enabled(LOBBY));
        view.lobby_enabled = true;
        assert!(view.enabled(LOBBY));
    }

    #[test]
    fn team_bar_keeps_its_rows_when_the_window_widens() {
        let host = dialog(true, SetupFlavor::Wol, true);
        let regular = MultiplayerSetupLayout::new(host, 800, 600, true);
        let wide = MultiplayerSetupLayout::new(host, 1600, 600, true);
        let controls = |layout: &MultiplayerSetupLayout| {
            let bar = layout.team_bar.unwrap();
            [TEAM_COUNT, ROOM_SIZE, TEAM_STARTS, TEAM_COLORS, BALANCE]
                .map(|id| layout.control_rect(id).unwrap().translate(-bar.x, -bar.y))
        };
        let [teams, size, starts, colors, balance] = controls(&regular);
        assert!(teams.y + teams.h <= size.y && size.y + size.h <= starts.y);
        assert!(starts.y == colors.y && colors.y == balance.y);
        assert_eq!(controls(&wide), controls(&regular));
        let (bar, wide_bar) = (regular.team_bar.unwrap(), wide.team_bar.unwrap());
        assert!(wide_bar.w > bar.w);
        assert_eq!(wide_bar.h, bar.h);
        assert_eq!(wide.roster_bottom, regular.roster_bottom);
    }

    /// VERA's roster stacks its teams in one column, over the team bar. A
    /// full room of three teams scrolls: its scrollbar stands right of the
    /// groups' boxes and left of the chat, a team scrolled into keeps its
    /// header pinned on top, and the last place shows the last entry. No
    /// header is left at the foot without its first row.
    #[test]
    fn vera_roster_stacks_teams_and_scrolls() {
        use RosterItem::{Header, Row};
        let team = |team: u8, rows: usize| {
            std::iter::once(Header(SetupGroup::Team(team))).chain(std::iter::repeat_n(Row, rows))
        };
        let full: Vec<_> = team(0, 15)
            .chain(team(1, 15))
            .chain(team(2, 15))
            .chain([Header(SetupGroup::Spectators)])
            .collect();
        let mut layout =
            MultiplayerSetupLayout::new(dialog(true, SetupFlavor::Wol, true), 800, 600, false);
        let bottom = layout.roster_bottom;
        let team_bar = layout.team_bar.unwrap();
        let area = layout.roster_area;
        assert!(area.y + area.h < team_bar.y && team_bar.x == area.x);
        let scroll = roster_scroll(&full, bottom);
        // Each player's row and the empty spectators' header.
        assert_eq!(scroll.stops.len(), 46);
        let top = roster_page(&full, scroll.stops[0], bottom);
        assert_eq!(
            top[..2],
            [
                RosterPlace {
                    item: 0,
                    y: ROSTER_TOP,
                    header: true
                },
                RosterPlace {
                    item: 1,
                    y: ROSTER_TOP + ROSTER_HEADER_H,
                    header: false
                }
            ]
        );
        assert!(top.last().unwrap().item < full.len() - 1);
        let last = roster_page(&full, scroll.stops[scroll.max_top], bottom);
        assert_eq!(last.last().unwrap().item, full.len() - 1);
        let before = roster_page(&full, scroll.stops[scroll.max_top - 1], bottom);
        assert!(before.last().unwrap().item < full.len() - 1);
        // Scrolled into Team B, its header stays on top.
        let into_b = roster_page(&full, 20, bottom);
        assert_eq!((into_b[0].item, into_b[0].header), (16, true));
        assert_eq!(into_b[1].item, 20);

        layout.arrange_roster(&into_b);
        let mut view = vera_room_view();
        view.roster_scroll = (scroll.stops.len(), scroll.max_top);
        // The roster's column stands left of the chat's.
        let bar = layout.roster_scroll(&view).unwrap().scrollbar.unwrap();
        assert!(bar.x + bar.w <= layout.chat_log.x);
        let area = layout.roster_area;
        let boxes: Vec<_> = layout.group_boxes.iter().flatten().collect();
        assert!(!boxes.is_empty());
        for rect in &boxes {
            assert!(rect.x + rect.w < bar.x && rect.y + rect.h <= area.y + area.h);
        }
        for (position, place) in into_b.iter().enumerate() {
            let row = layout.roster_rows[position];
            assert_eq!(row.x, layout.roster_rows[0].x);
            if !place.header {
                let team = layout.combos[position][SetupColumn::Team as usize];
                assert!(team.x + team.w <= row.x + row.w, "{position}");
            }
        }
        // The wheel scrolls three places a notch, no further than the last.
        let mut ui = MultiplayerSetupState::default();
        let mut gui = Gui::new(8);
        ui.wheel(&mut gui, &view, &layout, area.x, area.y, -1.0);
        assert_eq!(ui.roster_top(scroll.max_top), 3);
        ui.wheel(&mut gui, &view, &layout, area.x, area.y, -100.0);
        assert_eq!(ui.roster_top(scroll.max_top), scroll.max_top);
        let mut ui = MultiplayerSetupState::default();
        // A room that fits shows no scrollbar.
        view.roster_scroll = (3, 0);
        assert!(layout.roster_scroll(&view).unwrap().scrollbar.is_none());

        // A press on Team B's box joins it, on a cell whose list does not
        // open too; a cell whose list opens takes the press for its list.
        view.headers[0] = Some(SetupHeader {
            group: SetupGroup::Team(1),
            member_colors: vec![],
            caption: String::new(),
            join: Some(String::new()),
            join_enabled: true,
        });
        let choice = |value| SetupChoice {
            value,
            text: String::new(),
            swatch: None,
        };
        for (row, enabled, choices) in [(1, false, 1), (2, true, 1), (3, true, 2)] {
            view.combos.push(SetupCombo {
                id: SetupColumn::Country.control(row),
                row: row as u8,
                enabled,
                visible: true,
                selected: Some(0),
                choices: (0..choices).map(choice).collect(),
                list_w: 0,
            });
        }
        let center = |rect: RectPx| (rect.x + rect.w / 2, rect.y + rect.h / 2);
        let cell = |row: usize| center(layout.combos[row][SetupColumn::Country as usize]);
        let mut controller = widgets(&ui, &view, &layout);
        let press = |controller: &DialogController, row: usize| hit(controller, cell(row), true);
        assert_eq!(press(&controller, 1), Some(team_join(0)));
        assert_eq!(press(&controller, 2), Some(team_join(0)));
        assert_eq!(press(&controller, 3), Some(SetupColumn::Country.control(3)));
        // The status line names the cell under the pointer all the same.
        assert_eq!(
            hit(&controller, cell(1), false),
            Some(SetupColumn::Country.control(1))
        );
        // Its lists open on rows as tall as its own.
        let (x, y) = cell(3);
        super::press(&mut controller, &mut ui, &view, &layout, x, y);
        let popup = open_list(controller.gui()).unwrap().1.popup;
        assert_eq!(popup.row_height, ROSTER_ROW_H);
        assert_eq!(popup.rect.h, 2 * ROSTER_ROW_H);
        assert_eq!(
            popup.row_at(popup.rect.x + 1, popup.rect.y + ROSTER_ROW_H),
            Some(1)
        );
        // They are as wide as their cell, or as their widest text if wider.
        let open = SetupColumn::Country.control(3);
        let cell = layout.control_rect(open).unwrap();
        assert_eq!(popup.rect.w, cell.w);
        let combo = view.combos.iter_mut().find(|combo| combo.id == open);
        combo.unwrap().list_w = cell.w + 40;
        sync(&mut controller, &ui, &view, &layout, true);
        let popup = open_list(controller.gui()).unwrap().1.popup;
        assert_eq!(popup.rect.w, cell.w + 40);
        // A join or move that puts another player where the list opened
        // hides it, so a pick cannot change them.
        let open = SetupColumn::Country.control(3);
        let combo = view
            .combos
            .iter_mut()
            .find(|combo| combo.id == open)
            .unwrap();
        combo.row = 4;
        sync(&mut controller, &ui, &view, &layout, true);
        assert!(open_list(controller.gui()).is_none());
        // It hides, and shows again as it was once its row is back.
        let combo = view.combos.iter_mut().find(|combo| combo.id == open);
        combo.unwrap().row = 3;
        sync(&mut controller, &ui, &view, &layout, true);
        assert!(open_list(controller.gui()).is_some_and(|(id, _)| id == open));

        // Fifteen teams of two: from every place, the column ends above
        // its bottom with no header left without its first row.
        let many: Vec<_> = (0..15).flat_map(|index| team(index, 2)).collect();
        let scroll = roster_scroll(&many, bottom);
        assert_eq!(scroll.stops.len(), 30);
        for top in scroll.stops {
            let places = roster_page(&many, top, bottom);
            let foot = places.last().unwrap();
            assert!(!foot.header);
            assert!(foot.y + ROSTER_ROW_H <= bottom);
        }
    }

    /// On a window wider than the shell, VERA's room stands the shell at its
    /// right edge and spreads the roster and the chat from its left edge;
    /// Game Options stays centred over them. A native dialog keeps the
    /// centred shell.
    #[test]
    fn vera_room_spreads_over_a_wide_window() {
        let room = |modern| {
            MultiplayerSetupLayout::new(dialog(true, SetupFlavor::Wol, modern), 1920, 1080, true)
        };
        let layout = room(true);
        let shell = layout.page.scale.dialog_rect();
        assert_eq!((shell.x + shell.w, shell.w), (1920, 1440));
        let back = layout.control_rect(BACK).unwrap();
        let panel_x = 1920 - 168 * 1440 / 800;
        assert!(back.x >= panel_x, "{back:?}");
        let (roster, chat_log) = (layout.roster_area, layout.chat_log);
        // The roster starts a gap past the background's frame, which keeps
        // its own width (1.8 times the art's), and the chat's log runs as
        // high as the roster.
        let frame = (ROOM_FRAME_W as f32 * 1.8).ceil() as i32;
        assert!(roster.x > frame + 8 && roster.x < 40, "{roster:?}");
        assert!(roster.x + roster.w < chat_log.x && chat_log.y == roster.y);
        assert!(chat_log.x + chat_log.w < panel_x);
        let popup = layout.game_options.unwrap().dialog;
        assert!(
            (popup.x + popup.w / 2 - panel_x / 2).abs() <= 2,
            "{popup:?}"
        );
        assert_eq!(room(false).page.scale.dialog_rect().x, 240);
    }

    #[test]
    fn accept_prompt_flash_starts_off_stops_and_restarts_at_its_own_phase() {
        let mut ui = MultiplayerSetupState::default();
        let now = Instant::now();
        assert!(!ui.accept_highlight(true, now));
        assert!(ui.accept_highlight(true, now + std::time::Duration::from_millis(1000)));
        assert!(!ui.accept_highlight(false, now + std::time::Duration::from_millis(1500)));
        assert!(!ui.accept_highlight(true, now + std::time::Duration::from_millis(1600)));
        assert!(ui.accept_highlight(true, now + std::time::Duration::from_millis(2600)));
    }

    /// The host's press on a row carries it only once the pointer has left
    /// the press, closing the list the press opened. It drops on another
    /// team, and a guest's on the spectators too; never on its own box or the
    /// closed seats, and a row the host may not drag drops nowhere.
    #[test]
    fn a_carried_row_drops_only_on_another_box() {
        let mut view = vera_room_view();
        let header = |group| {
            Some(SetupHeader {
                group,
                member_colors: vec![],
                caption: String::new(),
                join: None,
                join_enabled: false,
            })
        };
        view.headers[0] = header(SetupGroup::Team(0));
        view.headers[2] = header(SetupGroup::Team(1));
        view.headers[4] = header(SetupGroup::Spectators);
        view.headers[5] = header(SetupGroup::ClosedSeats);
        let drag = |row, group, may_watch| DragRow {
            row,
            group,
            may_watch,
        };
        view.drag_rows = vec![
            drag(1, SetupGroup::Team(0), true),
            drag(3, SetupGroup::Team(1), false),
        ];
        assert_eq!(view.drop_group(1, 2), Some(SetupGroup::Team(1)));
        assert_eq!(view.drop_group(1, 0), None);
        assert_eq!(view.drop_group(1, 4), Some(SetupGroup::Spectators));
        assert_eq!(view.drop_group(3, 4), None);
        assert_eq!(view.drop_group(3, 5), None);
        assert_eq!(view.drop_group(3, 0), Some(SetupGroup::Team(0)));
        assert_eq!(view.drop_group(2, 2), None);

        // The list the press opened stays open until the row is carried.
        let mut ui = MultiplayerSetupState::default();
        ui.press_row(1, 100, 100);
        assert!(!ui.drag_row(103, 105, |_| Some(2)));
        assert_eq!(ui.row_drag.and_then(|drag| drag.target), None);
        assert!(ui.drag_row(100, 106, |_| Some(2)));
        assert_eq!(ui.drop_row(), Some((1, 2)));
        assert_eq!(ui.row_drag, None);
    }

    /// VERA's game room trades a guest's Accept for Game Options in its
    /// place; a host's Game Options takes Choose Map's place over Choose Map,
    /// which moves a place down, into C2's Page place. The other buttons keep
    /// their places, an online room has its Lobby a place under them, and
    /// native diagnostics keep all.
    #[test]
    fn game_room_puts_game_options_in_accept_or_choose_map_place() {
        let buttons = |host, flavor, modern| {
            MultiplayerSetupLayout::new(dialog(host, flavor, modern), 800, 600, false)
                .page
                .buttons
                .into_iter()
                .map(|button| (button.id, button.rect))
                .collect::<std::collections::HashMap<_, _>>()
        };
        for flavor in [SetupFlavor::Lan, SetupFlavor::Wol] {
            let mut native = buttons(false, flavor, false);
            let accept = native.remove(&ACCEPT).unwrap();
            native.remove(&PAGE);
            native.insert(GAME_OPTIONS, accept);
            if flavor == SetupFlavor::Wol {
                native.insert(LOBBY, accept.translate(0, accept.h));
            }
            assert_eq!(buttons(false, flavor, true), native, "{flavor:?} guest");

            let mut native = buttons(true, flavor, false);
            let choose_map = native[&CHOOSE_MAP];
            let step = choose_map.y - native[&START].y;
            let below = native
                .remove(&PAGE)
                .unwrap_or(choose_map.translate(0, step));
            native.insert(GAME_OPTIONS, choose_map);
            native.insert(CHOOSE_MAP, below);
            if flavor == SetupFlavor::Wol {
                native.insert(LOBBY, below.translate(0, step));
            }
            assert_eq!(buttons(true, flavor, true), native, "{flavor:?} host");
        }
    }

    #[test]
    fn only_the_vera_room_relabels_customize_battle() {
        for flavor in [SetupFlavor::Lan, SetupFlavor::Wol] {
            assert_eq!(
                dialog(true, flavor, true).button_label(CHOOSE_MAP),
                Some("VERA:ChooseMap")
            );
            assert_eq!(dialog(true, flavor, false).button_label(CHOOSE_MAP), None);
        }
        assert_eq!(
            dialog(true, SetupFlavor::Wol, true).button_label(START),
            None
        );
    }

    /// The open popup takes every press, over the room's rows too; closed,
    /// VERA's room has no options of its own, while the native room keeps
    /// them in place.
    #[test]
    fn game_options_popup_holds_the_room_options() {
        let popup_layout =
            MultiplayerSetupLayout::new(dialog(true, SetupFlavor::Wol, true), 800, 600, false);
        let popup = popup_layout.game_options.unwrap();
        // The checkboxes may cover the art; the trackbars and OK stay right of it.
        for id in CHECKBOXES
            .into_iter()
            .chain(ROOM_CHECKBOXES)
            .chain(OPTION_TRACKBARS)
            .chain([GAME_OPTIONS_OK])
        {
            let rect = popup_layout.control_rect(id).unwrap();
            let left = if OPTION_TRACKBARS.contains(&id) || id == GAME_OPTIONS_OK {
                215
            } else {
                0
            };
            assert!(
                rect.x >= popup.dialog.x + left
                    && rect.x + rect.w <= popup.dialog.x + popup.dialog.w
                    && rect.y + rect.h <= popup.dialog.y + popup.dialog.h,
                "{id:#x} at {rect:?}"
            );
        }
        let mut view = vera_room_view();
        view.open_box = Some(RoomBox::GameOptions);
        let ui = MultiplayerSetupState::default();
        let mut controller = widgets(&ui, &view, &popup_layout);
        let center = |rect: RectPx| (rect.x + rect.w / 2, rect.y + rect.h / 2);
        assert_eq!(
            hit(&controller, center(popup_layout.checkboxes[0]), false),
            Some(CHECKBOXES[0])
        );
        assert_eq!(hit(&controller, center(popup_layout.chat), false), None);
        // The team bar under the roster holds the Teams count, the host's
        // Room size, Team Starts and Team Colors and, online, Balance; a LAN
        // room has no Balance, a guest no Team Starts.
        view.open_box = None;
        for (id, range) in [
            (TEAM_COUNT, TrackbarRange::new(1, 15, 1)),
            (ROOM_SIZE, TrackbarRange::new(2, 30, 1)),
        ] {
            view.trackbars.push(SetupTrackbar {
                id,
                value: 2,
                range,
                enabled: true,
                visible: true,
            });
        }
        let bar = popup_layout.team_bar.unwrap();
        let roster = popup_layout.roster_area;
        let chat_log = popup_layout.chat_log;
        assert!(
            bar.x == roster.x
                && roster.y + roster.h < bar.y
                && bar.x + bar.w < chat_log.x
                && chat_log.y < bar.y
        );
        for id in [TEAM_COUNT, ROOM_SIZE, TEAM_STARTS, TEAM_COLORS, BALANCE] {
            let rect = popup_layout.control_rect(id).unwrap();
            assert!(
                rect.x >= bar.x
                    && rect.x + rect.w <= bar.x + bar.w
                    && rect.y >= bar.y
                    && rect.y + rect.h <= bar.y + bar.h,
                "{id:#x} at {rect:?}"
            );
            sync(&mut controller, &ui, &view, &popup_layout, true);
            assert_eq!(hit(&controller, center(rect), false), Some(id));
        }
        // At 800x600 Room size stacks under the Teams count, the two
        // trackbars in one column, and the buttons stand evenly apart.
        let [teams, room, starts, colors, balance] =
            [TEAM_COUNT, ROOM_SIZE, TEAM_STARTS, TEAM_COLORS, BALANCE]
                .map(|id| popup_layout.control_rect(id).unwrap());
        assert!(teams.y < room.y && teams.x == room.x);
        // Their windows are half again as wide as the room's, with a frame.
        let small = popup_layout.control_scale(TEAM_COUNT);
        let base = small.base_rect(teams);
        assert!(
            base.w > 190
                && crate::render::skirmish_shell_chrome::drawn_trackbar_frame_slot(
                    true, base.w, base.h, small
                )
                .is_some()
        );
        assert_eq!(
            colors.x - (starts.x + starts.w),
            balance.x - (colors.x + colors.w)
        );
        // Its art draws at 78% of the shell's size.
        assert_eq!(popup_layout.control_scale(TEAM_COUNT).factor(), 0.78);
        assert_eq!(popup_layout.control_scale(SPEED).factor(), 1.0);
        let lan =
            MultiplayerSetupLayout::new(dialog(true, SetupFlavor::Lan, true), 800, 600, false);
        assert_eq!(lan.control_rect(BALANCE), None);
        let mut guest =
            MultiplayerSetupLayout::new(dialog(false, SetupFlavor::Wol, true), 800, 600, false);
        assert_eq!(guest.control_rect(TEAM_STARTS), None);
        // Without teams the host's bar keeps only Room size, on one line;
        // a guest's roster then reaches the column's foot.
        let mut no_teams = popup_layout.clone();
        no_teams.fit_team_bar(dialog(true, SetupFlavor::Wol, true), false, true);
        let room = no_teams.control_rect(ROOM_SIZE).unwrap();
        assert!(room.w > 0 && no_teams.team_bar.unwrap().y < room.y);
        assert_eq!(no_teams.control_rect(TEAM_COUNT).unwrap().w, 0);
        assert_eq!(no_teams.control_rect(TEAM_STARTS), None);
        assert!(no_teams.team_bar.unwrap().h < popup_layout.team_bar.unwrap().h);
        guest.fit_team_bar(dialog(false, SetupFlavor::Wol, true), false, false);
        assert_eq!(guest.team_bar, None);
        assert_eq!(guest.roster_bottom, ROSTER_BOTTOM);
        view.open_box = None;
        // VERA's missing-map offer takes every press until it is answered.
        view.map_offer_open = true;
        sync(&mut controller, &ui, &view, &popup_layout, true);
        assert_eq!(hit(&controller, center(popup_layout.chat), false), None);
        assert_eq!(
            hit(&controller, center(popup_layout.map_offer.ok), false),
            Some(MAP_OFFER_DOWNLOAD)
        );
        view.map_offer_open = false;
        sync(&mut controller, &ui, &view, &popup_layout, true);
        assert_ne!(
            hit(&controller, center(popup_layout.checkboxes[0]), false),
            Some(CHECKBOXES[0])
        );
        view.dialog = dialog(true, SetupFlavor::Wol, false);
        let native = MultiplayerSetupLayout::new(view.dialog, 800, 600, false);
        assert!(native.game_options.is_none());
        let controller = widgets(&ui, &view, &native);
        assert_eq!(
            hit(&controller, center(native.checkboxes[0]), false),
            Some(CHECKBOXES[0])
        );
    }

    #[test]
    fn setup_controls_follow_original_resources_and_executed_relayout() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        for case in corpus["setup_layouts"].as_array().unwrap() {
            // BC6 shares the fixture; ui::coop_score_shell owns its layout.
            if case["dialog"] == 0xBC6 {
                continue;
            }
            let dialog = SetupDialog {
                host: case["dialog"] == 0xBC,
                flavor: SetupFlavor::Lan,
                modern: false,
            };
            let layout = MultiplayerSetupLayout::new(
                dialog,
                case["width"].as_i64().unwrap() as i32,
                case["height"].as_i64().unwrap() as i32,
                false,
            );
            for child in case["children"].as_array().unwrap() {
                let id = child["id"].as_u64().unwrap() as u16;
                let Some(actual) = layout.control_rect(id) else {
                    continue;
                };
                // Co-op controls are repositioned by their active mode later;
                // this corpus is the common resource relayout only.
                if id == DIFFICULTY {
                    continue;
                }
                let a = child["rect"].as_array().unwrap();
                let mut native = RectPx::new(
                    a[0].as_i64().unwrap() as i32,
                    a[1].as_i64().unwrap() as i32,
                    a[2].as_i64().unwrap() as i32,
                    a[3].as_i64().unwrap() as i32,
                );
                if SetupColumn::decode(id).is_some() {
                    native.h = 24;
                }
                if id == CHAT {
                    native = RectPx::new(native.x + 1, native.y + 1, native.w - 2, native.h - 2);
                }
                assert_eq!(actual, native, "dialog{} control{id:#x}", case["dialog"]);
            }
        }
    }

    #[test]
    fn setup_help_uses_original_host_and_guest_control_tables() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        for case in corpus["setup_help"].as_array().unwrap() {
            if case["dialog"] == 0xBC6 {
                continue;
            }
            assert_eq!(
                tooltip_key(
                    SetupDialog {
                        host: case["dialog"] == 0xBC,
                        flavor: SetupFlavor::Lan,
                        modern: false,
                    },
                    case["control"].as_u64().unwrap() as u16
                )
                .as_deref(),
                case["key"].as_str(),
                "{case}"
            );
        }
    }

    /// C2/C9 executed relayout and help tables
    /// (`tools/storage_oracle/wol_shell.json`).
    #[test]
    fn wol_setup_controls_and_help_follow_the_executed_c2_c9_tables() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let parse_id = |key: &str| u16::from_str_radix(key.trim_start_matches("0x"), 16).ok();
        for (key, host) in [("0xC2", true), ("0xC9", false)] {
            let dialog = SetupDialog {
                host,
                flavor: SetupFlavor::Wol,
                modern: false,
            };
            let mut checked = 0;
            for (size, case) in corpus["layouts"][key]["sizes"].as_object().unwrap() {
                let (width, height) = size.split_once('x').unwrap();
                let layout = MultiplayerSetupLayout::new(
                    dialog,
                    width.parse().unwrap(),
                    height.parse().unwrap(),
                    false,
                );
                for (id, child) in case["controls"].as_object().unwrap() {
                    let Some(id) = parse_id(id) else {
                        continue;
                    };
                    let Some(actual) = layout.control_rect(id) else {
                        continue;
                    };
                    if id == DIFFICULTY {
                        continue;
                    }
                    let a = child["rect"].as_array().unwrap();
                    let mut native = RectPx::new(
                        a[0].as_i64().unwrap() as i32,
                        a[1].as_i64().unwrap() as i32,
                        a[2].as_i64().unwrap() as i32,
                        a[3].as_i64().unwrap() as i32,
                    );
                    if SetupColumn::decode(id).is_some() {
                        native.h = 24;
                    }
                    if id == CHAT {
                        native =
                            RectPx::new(native.x + 1, native.y + 1, native.w - 2, native.h - 2);
                    }
                    assert_eq!(actual, native, "{key} {size} control {id:#x}");
                    checked += 1;
                }
            }
            assert!(checked > 100, "{key}: {checked} controls compared");
            for (id, help) in corpus["help"][key].as_object().unwrap() {
                let Some(id) = parse_id(id) else {
                    continue;
                };
                assert_eq!(
                    tooltip_key(dialog, id).as_deref(),
                    help.as_str(),
                    "{key} control {id:#x}"
                );
            }
        }
    }

    #[test]
    fn setup_retains_native_row_identity_across_columns() {
        for row in 0..8 {
            for column in SetupColumn::ALL {
                assert_eq!(
                    SetupColumn::decode(column.control(row)),
                    Some((column, row))
                );
            }
        }
        assert_eq!(SetupColumn::decode(0x6A9), None);
    }
}
