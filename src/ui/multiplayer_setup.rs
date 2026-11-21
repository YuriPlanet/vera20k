//! Network setup presentation: LAN host BC (005DE520) and guest BD
//! (005E1CA0), WOL host C2 (0078F7F0) and guest C9 (00791B10). The WOL pages
//! are supersets of the LAN ones with the same control ids (resources C2/C9).
//! MatchSession owns admitted participants, options, readiness and map status.
//! This leaf keeps only control interaction and emits commands for that owner.

use crate::util::time::Instant;

use crate::ui::lan_shell::{LanChatState, LanListState};
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::combobox::{ComboPopup, ComboPopupInteraction};
use crate::ui::shell::descriptor::{DialogId, LOW_HEADING_ANCHOR, MAP_TEXT_ANCHOR};
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, child_window};
use crate::ui::shell::layout::anchor_rect;
use crate::ui::shell::list::{
    ListMetrics, ListScrollInteraction, ListScrollPart, ShellListGeometry,
};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::trackbar::{TrackbarHold, TrackbarRange};
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
/// host's Quick Team Setup and Bulk Seat Actions lists
/// ([`crate::ui::team_tools`]).
pub(crate) const TEAM_COUNT: u16 = 0x7F0D;
pub(crate) const TEAM_MENU: u16 = 0x7F0E;
pub(crate) const SEAT_MENU: u16 = 0x7F0F;
/// VERA-only, the host's, in the team bar: Room size, the most people the
/// room holds, spectators too.
pub(crate) const ROOM_SIZE: u16 = 0x7F10;
/// VERA-only, in an online room: Lobby, to the public lobby while the room
/// goes on hidden (docs/lobby-protocol.md "Visiting the lobby").
pub(crate) const LOBBY: u16 = 0x7F16;
pub(crate) const CAMPAIGN_SERIES: u16 = 0x7F17;
pub(crate) const CAMPAIGN_STAGE: u16 = 0x7F18;
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
/// On a window wide enough that each half of the roster's width holds
/// entries this wide (a 16:9 one, with the chat at its least), its groups
/// stand two abreast, their boxes [`ROSTER_GROUP_GAP`] apart.
const ROSTER_PAIR_MIN_W: i32 = 260;
const ROSTER_PAIR_GAP: i32 = 2 * ROSTER_BOX_MARGIN.0 + ROSTER_GROUP_GAP;
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
/// by a taller row for the host's two lists. Its trackbars are authored at
/// 78% of the shell's size ([`MultiplayerSetupLayout::control_scale`]).
const TEAM_BAR_ROW_H: i32 = 25;
const TEAM_BAR_MENU_ROW_H: i32 = 30;
const TEAM_BAR_INSET: i32 = 8;
const TEAM_BAR_GAP: i32 = 12;
const TEAM_BAR_LABEL: RectPx = RectPx::new(0, 3, 90, 18);
const TEAM_BAR_TRACKBAR: RectPx = RectPx::new(92, 4, 151, 17);
/// The team bar's trackbar window: the room's 129x21 half again as wide,
/// its plaque the same, so its rail is longer. It has its own frame
/// (`render::skirmish_shell_chrome::TRACKBAR_FRAMES`).
const TEAM_BAR_TRACKBAR_WINDOW: (i32, i32) = (194, 21);
/// The host's lists at the shell's own size, on a face the shell has art
/// for (`combo_face_150`).
const TEAM_BAR_MENU: RectPx = RectPx::new(0, 3, 150, 24);
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
        Self::across(column.x + ROSTER_INSET, column.w - ROSTER_SPARE_W)
    }
    /// Entries `w` wide from `x`.
    fn across(x: i32, w: i32) -> Self {
        let narrow = |length: i32| length * w.clamp(0, ROSTER_W) / ROSTER_W;
        let first = ROSTER_CELLS[0].0;
        let at = |dx: i32| first + narrow(dx - first);
        Self {
            x,
            w,
            cells: ROSTER_CELLS.map(|(dx, width)| (at(dx), narrow(width))),
            caption_w: narrow(ROSTER_CAPTION_W),
            add_ai: (at(ROSTER_ADD_AI.0), narrow(ROSTER_ADD_AI.1)),
            join_w: narrow(ROSTER_JOIN_W),
        }
    }
    fn half_w(self) -> i32 {
        (self.w - ROSTER_PAIR_GAP) / 2
    }
    /// Whether the roster's groups stand two abreast here.
    fn pairs(self) -> bool {
        self.half_w() >= ROSTER_PAIR_MIN_W
    }
    /// The entries of `span`: the whole width, or one half of it.
    fn span(self, span: RosterSpan) -> Self {
        let half = self.half_w();
        match span {
            RosterSpan::Full => self,
            RosterSpan::Left => Self::across(self.x, half),
            RosterSpan::Right => Self::across(self.x + self.w - half, half),
        }
    }
    /// The column labels over the cells. The narrow cells' (Colour, Start
    /// and Team) centre on their cell over the step from one to the next,
    /// so their words still read where the cells are narrower than them.
    fn labels(self, scale: ShellScale) -> impl Iterator<Item = (&'static str, RectPx)> {
        let step = self.cells[3].0 - self.cells[2].0;
        [
            "GUI:Players",
            "GUI:Side",
            "GUI:Color",
            "GUI:StartPosition",
            "GUI:Team",
        ]
        .into_iter()
        .zip(self.cells)
        .enumerate()
        .map(move |(index, (key, (dx, w)))| {
            let wide = if index < 2 { w } else { step.max(w) };
            let x = self.x + dx - (wide - w) / 2;
            let label = RectPx::new(x, ROSTER_LABEL_Y, wide, ROSTER_LABEL_H);
            (key, scale.scale_rect(label))
        })
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

/// Which part of VERA's roster's width an entry takes: all of it, or where
/// its groups stand two abreast ([`RosterFrame::pairs`]), one half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RosterSpan {
    Full,
    Left,
    Right,
}

/// Where an entry of VERA's roster stands: which entry, its top at 800x600,
/// and its part of the roster's width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RosterPlace {
    pub(crate) item: usize,
    pub(crate) y: i32,
    pub(crate) header: bool,
    pub(crate) span: RosterSpan,
}

/// Where VERA's roster's entries fit: above `bottom` (800x600 pixels: over
/// the team bar in a room with one), and whether the window is wide enough
/// for its groups to stand two abreast
/// ([`MultiplayerSetupLayout::roster_frame`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RosterFrame {
    pub(crate) bottom: i32,
    pub(crate) pairs: bool,
}

/// A line of VERA's roster: one entry across it, or, where its groups stand
/// two abreast, a group's entry on each half, either missing beside a
/// longer group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RosterLine {
    entries: [Option<usize>; 2],
    paired: bool,
    header: bool,
}
impl RosterLine {
    fn height(self) -> i32 {
        if self.header {
            ROSTER_HEADER_H
        } else {
            ROSTER_ROW_H
        }
    }
    fn first(self) -> Option<usize> {
        self.entries.into_iter().flatten().next()
    }
    fn places(self, y: i32) -> impl Iterator<Item = RosterPlace> {
        let spans = if self.paired {
            [RosterSpan::Left, RosterSpan::Right]
        } else {
            [RosterSpan::Full; 2]
        };
        self.entries
            .into_iter()
            .zip(spans)
            .filter_map(move |(item, span)| {
                Some(RosterPlace {
                    item: item?,
                    y,
                    header: self.header,
                    span,
                })
            })
    }
}

/// VERA's roster's lines: with `pairs` and two groups or more, each group
/// beside the next, the first on the left half, in pairs down the roster
/// (Teams A and B, then C and D); otherwise one entry a line.
fn roster_lines(items: &[RosterItem], pairs: bool) -> Vec<RosterLine> {
    let line = |index: usize| RosterLine {
        entries: [Some(index), None],
        paired: false,
        header: matches!(items[index], RosterItem::Header(_)),
    };
    let starts: Vec<usize> = (0..items.len())
        .filter(|&index| matches!(items[index], RosterItem::Header(_)))
        .collect();
    if !pairs || starts.len() < 2 {
        return (0..items.len()).map(line).collect();
    }
    // Rows before the first header stand alone; each group runs from its
    // header to the next one.
    let groups: Vec<std::ops::Range<usize>> = starts
        .iter()
        .zip(starts[1..].iter().chain([&items.len()]))
        .map(|(&start, &end)| start..end)
        .collect();
    let mut lines: Vec<RosterLine> = (0..starts[0]).map(line).collect();
    for pair in groups.chunks(2) {
        let (left, right) = (&pair[0], pair.get(1));
        let len = right.map_or(0, |group| group.len()).max(left.len());
        lines.extend((0..len).map(|k| {
            let at = |group: &std::ops::Range<usize>| {
                Some(group.start + k).filter(|i| group.contains(i))
            };
            RosterLine {
                entries: [at(left), right.and_then(at)],
                paired: true,
                header: k == 0,
            }
        }));
    }
    lines
}

/// The entries VERA's roster shows from entry `top` down, until a line no
/// longer fits above the frame's bottom. When `top` is a player under a
/// header above it, that header stays pinned first, so the group keeps its
/// caption and Join. A header under another group leaves
/// [`ROSTER_GROUP_GAP`] between their boxes, and none stands at the foot
/// without its first row.
pub(crate) fn roster_page(
    items: &[RosterItem],
    top: usize,
    frame: RosterFrame,
) -> Vec<RosterPlace> {
    let lines = roster_lines(items, frame.pairs);
    let top = lines
        .iter()
        .position(|line| line.entries.contains(&Some(top)))
        .unwrap_or(lines.len());
    lines_page(&lines, top, frame.bottom)
}
fn lines_page(lines: &[RosterLine], top: usize, bottom: i32) -> Vec<RosterPlace> {
    let pinned = lines
        .get(top)
        .filter(|line| !line.header)
        .and_then(|_| lines[..top].iter().rposition(|line| line.header));
    let mut places = Vec::new();
    let mut y = ROSTER_TOP;
    for index in pinned.into_iter().chain(top..lines.len()) {
        let line = lines[index];
        if line.header && y > ROSTER_TOP {
            y += ROSTER_GROUP_GAP;
        }
        let first_row = if line.header && lines.get(index + 1).is_some_and(|next| !next.header) {
            ROSTER_ROW_H
        } else {
            0
        };
        let count = line.entries.iter().flatten().count();
        if y + line.height() + first_row > bottom || places.len() + count > MODERN_SETUP_ROWS {
            break;
        }
        places.extend(line.places(y));
        y += line.height();
    }
    places
}

/// Where VERA's roster scrolls: the entries it can show first ([`roster_page`]'s
/// `top`), and the first of them from which every line to the last shows.
/// Each line of players' rows is one, its headers pinned over it; a header
/// only when no row is under it, as it would show the same as its first
/// row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RosterScroll {
    pub(crate) stops: Vec<usize>,
    pub(crate) max_top: usize,
}
pub(crate) fn roster_scroll(items: &[RosterItem], frame: RosterFrame) -> RosterScroll {
    let lines = roster_lines(items, frame.pairs);
    let tops: Vec<usize> = (0..lines.len())
        .filter(|&index| {
            !lines[index].header || lines.get(index + 1).is_none_or(|next| next.header)
        })
        .collect();
    let last = lines.last().copied();
    let max_top = tops
        .iter()
        .position(|&top| {
            lines_page(&lines, top, frame.bottom)
                .last()
                .is_some_and(|place| {
                    last.is_some_and(|line| line.entries.contains(&Some(place.item)))
                })
        })
        .unwrap_or(0);
    let stops = tops.iter().filter_map(|&top| lines[top].first()).collect();
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
/// The room's Building Production Queue ([`HostRules`](crate::skirmish_launch::HostRules)).
pub(crate) const BUILDING_QUEUE: u16 = 0x7F13;
/// The room's Superweapon Limit, a trackbar in VERA's Game Options only
/// ([`HostRules`](crate::skirmish_launch::HostRules)).
pub(crate) const SUPERWEAPON_LIMIT: u16 = 0x7F14;
/// The room's Tech Level, a trackbar in VERA's Game Options only; C2's proc
/// reads no tech level from its dialog.
pub(crate) const TECH_LEVEL: u16 = 0x7F15;
/// Game Options' button to its other page.
pub(crate) const GAME_OPTIONS_PAGE: u16 = 0x7F19;
/// Four session checkboxes C2's proc (`Dialog0xC2__DlgProc`, 0x0078F7F0)
/// handles though YR's room shows none of them: Bases (`0x0078FA51`), Fog of
/// War (`0x0078FC07`), Multi-Engineer (`0x0078FB2B`) and Harvester Truce
/// (`0x0078FBBB`). VERA's Game Options shows them beside the native five.
pub(crate) const BASES: u16 = 0x54B;
pub(crate) const FOG_OF_WAR: u16 = 0x599;
pub(crate) const MULTI_ENGINEER: u16 = 0x551;
pub(crate) const HARVESTER_TRUCE: u16 = 0x596;
pub(crate) const SESSION_CHECKBOXES: [u16; 4] =
    [BASES, FOG_OF_WAR, MULTI_ENGINEER, HARVESTER_TRUCE];
/// Their labels in VERA's language files, as Skirmish's Game Options has them.
pub(crate) const SESSION_CHECKBOX_LABELS: [&str; 4] = {
    use crate::ui::skirmish_shell::game_options as labels;
    [
        labels::BASES_LABEL,
        labels::FOG_OF_WAR_LABEL,
        labels::MULTI_ENGINEER_LABEL,
        labels::HARVESTER_TRUCE_LABEL,
    ]
};
pub(crate) const ROOM_CHECKBOXES: [u16; 8] = [
    BRIDGES,
    ALLYING,
    NO_YURI,
    NO_FRANCE,
    NO_SPY,
    NO_STOLEN_TECH,
    TERRAIN_DEFORMATION,
    BUILDING_QUEUE,
];
/// Their labels in VERA's language files; the retail string table has none
/// for them.
pub(crate) const ROOM_CHECKBOX_LABELS: [&str; 8] = [
    "VERA:DestroyableBridges",
    "VERA:InGameAllying",
    "VERA:NoYuri",
    "VERA:NoFrance",
    "VERA:NoSpy",
    "VERA:NoStolenTech",
    "VERA:TerrainDeformation",
    "VERA:BuildingProductionQueue",
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
        MenuPageButtonSpec {
            id: START,
            dlu_top: 147,
            csf_key: "GUI:StartGame",
            tooltip_key: "STT:HostButtonGo",
            result: None,
        },
        MenuPageButtonSpec {
            id: CHOOSE_MAP,
            dlu_top: 176,
            csf_key: "GUI:ChooseMap",
            tooltip_key: "STT:HostButtonChooseMap",
            result: None,
        },
    ],
    back: MenuPageButtonSpec {
        id: BACK,
        dlu_top: 346,
        csf_key: "GUI:Back",
        tooltip_key: "STT:HostButtonBack",
        result: None,
    },
};
pub(crate) const GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xBD),
    title_key: "GUI:JoinScreen",
    stacked: &[MenuPageButtonSpec {
        id: ACCEPT,
        dlu_top: 149,
        csf_key: "GUI:Accept",
        tooltip_key: "STT:GuestButtonAccept",
        result: None,
    }],
    back: MenuPageButtonSpec {
        id: BACK,
        dlu_top: 346,
        csf_key: "GUI:Back",
        tooltip_key: "STT:GuestButtonBack",
        result: None,
    },
};

const WOL_BACK: MenuPageButtonSpec = MenuPageButtonSpec {
    id: BACK,
    dlu_top: 257,
    csf_key: "GUI:Back",
    tooltip_key: "STT:WOLNewGameCancel",
    result: None,
};
pub(crate) const WOL_HOST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xC2),
    title_key: "GUI:HostScreen",
    stacked: &[
        MenuPageButtonSpec {
            id: START,
            dlu_top: 156,
            csf_key: "GUI:StartGame",
            tooltip_key: "STT:HostButtonGo",
            result: None,
        },
        MenuPageButtonSpec {
            id: CHOOSE_MAP,
            dlu_top: 183,
            csf_key: "GUI:ChooseMap",
            tooltip_key: "STT:HostButtonChooseMap",
            result: None,
        },
        MenuPageButtonSpec {
            id: PAGE,
            dlu_top: 210,
            csf_key: "GUI:PageButton",
            tooltip_key: "STT:WOLPageButton",
            result: None,
        },
    ],
    back: WOL_BACK,
};
pub(crate) const WOL_GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xC9),
    title_key: "GUI:JoinScreen",
    stacked: &[
        MenuPageButtonSpec {
            id: ACCEPT,
            dlu_top: 156,
            csf_key: "GUI:Accept",
            tooltip_key: "STT:GuestButtonAccept",
            result: None,
        },
        MenuPageButtonSpec {
            id: PAGE,
            dlu_top: 183,
            csf_key: "GUI:PageButton",
            tooltip_key: "STT:WOLPageButton",
            result: None,
        },
    ],
    back: WOL_BACK,
};
/// VERA's game room ([`SetupDialog::modern`]): Game Options takes a guest's
/// Accept place and a host's Choose Map place, and Choose Map moves down a
/// place, to C2's Page place on WOL.
const GAME_OPTIONS_BUTTON: MenuPageButtonSpec = MenuPageButtonSpec {
    id: GAME_OPTIONS,
    dlu_top: 0,
    csf_key: "GUI:GameOptions",
    tooltip_key: "",
    result: None,
};
const MODERN_HOST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        HOST_PAGE.stacked[0],
        MenuPageButtonSpec {
            dlu_top: HOST_PAGE.stacked[1].dlu_top,
            ..GAME_OPTIONS_BUTTON
        },
        MenuPageButtonSpec {
            dlu_top: 2 * HOST_PAGE.stacked[1].dlu_top - HOST_PAGE.stacked[0].dlu_top,
            ..HOST_PAGE.stacked[1]
        },
    ],
    ..HOST_PAGE
};
const MODERN_GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[MenuPageButtonSpec {
        dlu_top: GUEST_PAGE.stacked[0].dlu_top,
        ..GAME_OPTIONS_BUTTON
    }],
    ..GUEST_PAGE
};
const LOBBY_BUTTON: MenuPageButtonSpec = MenuPageButtonSpec {
    id: LOBBY,
    dlu_top: 0,
    csf_key: "VERA:Lobby",
    tooltip_key: "",
    result: None,
};
/// VERA's online room adds Lobby under its other buttons.
const MODERN_WOL_GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        MenuPageButtonSpec {
            dlu_top: WOL_GUEST_PAGE.stacked[0].dlu_top,
            ..GAME_OPTIONS_BUTTON
        },
        MenuPageButtonSpec {
            dlu_top: WOL_GUEST_PAGE.stacked[1].dlu_top,
            ..LOBBY_BUTTON
        },
    ],
    ..WOL_GUEST_PAGE
};
const MODERN_WOL_HOST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        WOL_HOST_PAGE.stacked[0],
        MenuPageButtonSpec {
            dlu_top: WOL_HOST_PAGE.stacked[1].dlu_top,
            ..GAME_OPTIONS_BUTTON
        },
        MenuPageButtonSpec {
            dlu_top: WOL_HOST_PAGE.stacked[2].dlu_top,
            ..WOL_HOST_PAGE.stacked[1]
        },
        MenuPageButtonSpec {
            dlu_top: 2 * WOL_HOST_PAGE.stacked[2].dlu_top - WOL_HOST_PAGE.stacked[1].dlu_top,
            ..LOBBY_BUTTON
        },
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
    /// Listed grey, and picking it does nothing: a team bar menu's item
    /// that cannot be had now.
    pub(crate) greyed: bool,
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
    // Co-op5C21D0 sends7324AC(0), so the shared native trackbar runs with no
    // value-plaque reserve; ordinary options retain the49-pixel plaque.
    fn reserve(&self) -> i32 {
        if self.id == DIFFICULTY {
            0
        } else {
            crate::ui::shell::trackbar::PLAQUE_RESERVE
        }
    }
    pub(crate) fn thumb_left(&self, rect: RectPx, scale: ShellScale) -> i32 {
        crate::ui::shell::trackbar::thumb_left_at(
            self.value - self.range.min,
            rect,
            self.reserve(),
            self.range.max - self.range.min,
            scale,
        )
    }
    pub(crate) fn pointer_value(&self, x: i32, rect: RectPx, scale: ShellScale) -> i32 {
        let track = crate::ui::shell::trackbar::authored_track(rect, (x, rect.y), scale);
        self.range.stepped(
            self.range.min
                + crate::ui::shell::trackbar::trackbar_position_from_x(
                    track.local_x,
                    track.width,
                    self.reserve(),
                    self.range.max - self.range.min,
                ),
        )
    }
    pub(crate) fn press(
        &self,
        x: i32,
        y: i32,
        rect: RectPx,
        scale: ShellScale,
    ) -> crate::ui::shell::trackbar::TrackbarPress {
        use crate::ui::shell::trackbar::{TrackbarPress, authored_track, trackbar_press};
        let track = authored_track(rect, (x, y), scale);
        match trackbar_press(
            self.value - self.range.min,
            track.local_x,
            track.local_y,
            track.width,
            track.height,
            self.reserve(),
            self.range.max - self.range.min,
        ) {
            TrackbarPress::Jump(value) => {
                TrackbarPress::Jump(self.range.stepped(self.range.min + value))
            }
            other => other,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SetupRowIcon {
    pub(crate) country: Option<String>,
    pub(crate) host: bool,
    pub(crate) accepted: bool,
    /// VERA's online room: the player's badge
    /// ([`crate::ui::wol_lobby::badge`]), drawn in front of their name
    /// ([`MultiplayerSetupLayout::badge_rect`]).
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
    /// Game Options: the match options gamemd's room keeps.
    GameOptions,
    /// Its second page: VERA's own rules and bans.
    MoreOptions,
}

impl RoomBox {
    /// The Game Options page [`GAME_OPTIONS_PAGE`] turns to.
    pub(crate) fn other_page(self) -> Self {
        match self {
            Self::GameOptions => Self::MoreOptions,
            Self::MoreOptions => Self::GameOptions,
        }
    }
}

/// The options a Game Options page holds, besides its page button and OK.
pub(crate) fn game_options_controls(page: RoomBox) -> Vec<u16> {
    match page {
        RoomBox::GameOptions => CHECKBOXES
            .into_iter()
            .chain(SESSION_CHECKBOXES)
            .chain(OPTION_TRACKBARS)
            .chain([TECH_LEVEL])
            .collect(),
        RoomBox::MoreOptions => ROOM_CHECKBOXES
            .into_iter()
            .chain([SUPERWEAPON_LIMIT])
            .collect(),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SetupView {
    pub(crate) campaign: bool,
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
    /// C2's Short Game and Bases are held on in a tournament or clan room.
    pub(crate) short_game_locked: bool,
    /// The team header `0x794`; hidden in a clan room.
    pub(crate) team_header: bool,
    pub(crate) accept_enabled: bool,
    pub(crate) start_requested: bool,
    pub(crate) combos: Vec<SetupCombo>,
    pub(crate) trackbars: Vec<SetupTrackbar>,
    pub(crate) checks: [bool; 5],
    /// [`SESSION_CHECKBOXES`]' states.
    pub(crate) session_checks: [bool; 4],
    /// [`ROOM_CHECKBOXES`]' states.
    pub(crate) room_checks: [bool; 8],
    /// Indexed by the row on the page, up to [`SetupDialog::rows_per_page`].
    pub(crate) row_icons: [Option<SetupRowIcon>; MODERN_SETUP_ROWS],
    /// VERA's header rows, by the row on the page.
    pub(crate) headers: [Option<SetupHeader>; MODERN_SETUP_ROWS],
    /// The box open over VERA's room.
    pub(crate) open_box: Option<RoomBox>,
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
            // A campaign room's Game Options holds only its speed.
            || (id == GAME_OPTIONS_PAGE && !self.campaign)
        {
            return true;
        }
        if self.locked {
            return false;
        }
        match id {
            BACK | CHAT | CHAT_LOG | GAME_OPTIONS => true,
            LOBBY => self.lobby_enabled,
            START => self.host,
            CHOOSE_MAP => self.host && !self.campaign,
            PAGE => self.dialog.wol(),
            PREVIOUS_ROWS => self.first_row > 0,
            NEXT_ROWS => self.first_row + self.page_len < self.row_count,
            ACCEPT => !self.host && self.accept_enabled,
            0x54E | BASES if self.short_game_locked => false,
            id if CHECKBOXES.contains(&id)
                || SESSION_CHECKBOXES.contains(&id)
                || ROOM_CHECKBOXES.contains(&id) =>
            {
                self.host && !self.campaign
            }
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
    /// The primary button offers Download or Retry.
    pub(crate) button_key: &'static str,
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
    /// VERA's roster's scrollbar.
    roster_scrollbar: ListScrollInteraction,
    pub(crate) chat: LanChatState,
    pub(crate) notice: Option<SkirmishValidationModalState>,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) popup: ComboPopupInteraction<u16>,
    /// The row the open list belongs to. Joins, leaves and moves shift VERA's
    /// roster under it, and a list whose row moved away hides, so a pick
    /// never changes whoever took its place.
    popup_row: Option<u8>,
    pub(crate) trackbar_hold: Option<TrackbarHold<u16>>,
    /// The host's drag of a roster row, while the button is down.
    pub(crate) row_drag: Option<RowDrag>,
    pub(crate) statics: SkirmishStatics,
    pub(crate) shown: bool,
    pub(crate) focused_control: u16,
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
            roster_scrollbar: ListScrollInteraction::default(),
            chat: {
                let mut chat = LanChatState::default();
                chat.edit.focused = true;
                chat
            },
            notice: None,
            buttons: ShellButtonInteraction::default(),
            popup: ComboPopupInteraction::default(),
            popup_row: None,
            trackbar_hold: None,
            row_drag: None,
            statics: SkirmishStatics::default(),
            shown: false,
            focused_control: CHAT,
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
        self.popup.close();
        self.cancel_pointer();
    }
    /// Opens `control`'s list, on `row`.
    pub(crate) fn open_list(&mut self, control: u16, row: u8, selected: Option<usize>) {
        self.popup.open(control, selected);
        self.popup_row = Some(row);
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
    /// row is carried, its list closing, over the box `target` gives.
    pub(crate) fn drag_row(&mut self, x: i32, y: i32, target: impl FnOnce(u8) -> Option<usize>) {
        let Some(drag) = self.row_drag.as_mut() else {
            return;
        };
        drag.at = (x, y);
        if !drag.carrying && (x - drag.from.0).abs().max((y - drag.from.1).abs()) >= ROW_DRAG_START
        {
            drag.carrying = true;
            self.popup.close();
            self.popup_row = None;
        }
        drag.target = if drag.carrying {
            target(drag.row)
        } else {
            None
        };
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
    /// Works VERA's roster's scrollbar with `work`. Its rows' controls move
    /// under the pointer, so a list open on one closes when it scrolls.
    fn scroll_roster(
        &mut self,
        geometry: ShellListGeometry,
        work: impl FnOnce(&mut ListScrollInteraction, &mut usize),
    ) {
        let before = self.roster_top(geometry.max_top);
        let mut top = before;
        work(&mut self.roster_scrollbar, &mut top);
        self.first_visible_row = top;
        if top != before {
            self.popup.close();
        }
    }
    /// A press on the roster's scrollbar; `false` when it is not on it.
    pub(crate) fn roster_scroll_press(
        &mut self,
        geometry: ShellListGeometry,
        x: i32,
        y: i32,
        now: Instant,
    ) -> bool {
        let Some(part) = geometry.scroll_part_at(x, y) else {
            return false;
        };
        self.scroll_roster(geometry, |bar, top| bar.press(part, geometry, top, y, now));
        true
    }
    pub(crate) fn roster_scroll_moved(&mut self, geometry: ShellListGeometry, x: i32, y: i32) {
        self.scroll_roster(geometry, |bar, top| bar.pointer_moved(geometry, top, x, y));
    }
    /// The scrollbar's arrow repeat; returns whether the roster or its
    /// pressed arrow changed.
    pub(crate) fn roster_scroll_poll(
        &mut self,
        geometry: ShellListGeometry,
        x: i32,
        y: i32,
        now: Instant,
    ) -> bool {
        let mut changed = false;
        self.scroll_roster(geometry, |bar, top| {
            changed = bar.poll(geometry, top, x, y, now);
        });
        changed
    }
    pub(crate) fn roster_scroll_repeat_at(&self) -> Option<Instant> {
        self.roster_scrollbar.repeat_at()
    }
    pub(crate) fn roster_scroll_pressed(&self) -> Option<ListScrollPart> {
        self.roster_scrollbar.pressed_part()
    }
    /// An open roster list owns the wheel even at its ends. Once it closes,
    /// the same gesture over the roster scrolls the teams again.
    pub(crate) fn wheel(
        &mut self,
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
        let wheel_top = |top: usize, max: usize| top.saturating_add_signed(-step as isize).min(max);
        if let Some(mut popup) = self.popup_geometry(view, layout) {
            if popup.rect.contains(x, y) {
                popup.first = wheel_top(popup.first, popup.total - popup.visible_rows);
                self.popup.top_index = popup.first;
                popup.track_hot_row(&mut self.popup.hot_row, x, y);
            }
            return true;
        }
        if layout.roster_area.contains(x, y) {
            self.scroll_roster(roster, |_, top| *top = wheel_top(*top, roster.max_top));
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
        self.popup.close();
        self.cancel_pointer();
    }
    pub(crate) fn accept_highlight(&mut self, enabled: bool, now: Instant) -> bool {
        if !enabled {
            self.accept_flash_started = None;
            return false;
        }
        crate::ui::shell::button::flash_phase(*self.accept_flash_started.get_or_insert(now), now)
    }
    pub(crate) fn popup_geometry(
        &self,
        view: &SetupView,
        layout: &MultiplayerSetupLayout,
    ) -> Option<ComboPopup> {
        if view.locked {
            return None;
        }
        let id = self.popup.control?;
        let combo = view.combo(id).filter(|combo| {
            combo.visible && combo.enabled && self.popup_row.is_none_or(|row| row == combo.row)
        })?;
        // VERA's room shows its Player, Add AI and team bar menus whole,
        // with no scrollbar.
        let team_bar = matches!(id, TEAM_MENU | SEAT_MENU);
        let cap = match SetupColumn::decode(id) {
            Some((SetupColumn::Player, _)) if view.dialog.modern => 0,
            Some((column, _)) => column.visible_rows(),
            None if matches!(id, CAMPAIGN_SERIES | CAMPAIGN_STAGE) => {
                SetupColumn::Country.visible_rows()
            }
            None if team_bar => 0,
            None => {
                team_add_ai_row(id)?;
                0
            }
        };
        let scale = layout.page.scale;
        let face = layout.control_rect(id)?;
        let mut popup =
            ComboPopup::new(face, scale, combo.choices.len(), cap, self.popup.top_index);
        // VERA's roster opens a list on rows as tall as its own, as wide as
        // its cell or, if wider, its widest text and any scrollbar.
        if layout.roster {
            popup = popup.with_row_height(scale.scale_rect(RectPx::new(0, 0, 0, ROSTER_ROW_H)).h);
            let text_w = scale.scale_rect(RectPx::new(0, 0, combo.list_w, 0)).w;
            let bar_w = popup.scrollbar().map_or(0, |bar| bar.w);
            popup.rect.w = popup.rect.w.max(text_w + bar_w);
        }
        // The team bar's lists would run past the status line under it, so
        // they open upward.
        if team_bar && popup.rect.y + popup.rect.h > layout.page.status_help.y {
            popup.rect.y = face.y - 1 - popup.rect.h;
        }
        Some(popup)
    }
    pub(crate) fn caret_visible(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.focus_started).as_secs() % 2 == 0
    }
    pub(crate) fn focus_chat(&mut self) {
        if self.focused_control != CHAT {
            self.focus_started = Instant::now();
        }
        self.focused_control = CHAT;
        self.chat.edit.focused = true;
    }
    pub(crate) fn release_scrollbars(&mut self) {
        self.popup.cancel_pointer();
        self.chat.list.scroll.cancel();
        self.roster_scrollbar.cancel();
    }
    pub(crate) fn cancel_pointer(&mut self) {
        self.buttons = ShellButtonInteraction::default();
        self.trackbar_hold = None;
        self.row_drag = None;
        self.release_scrollbars();
    }
}

/// VERA's Game Options popup over the room ([`SetupDialog::modern`]): Red
/// Alert 2's 451x326 box, as the online lobby's popups use, with the room's
/// own option controls in its right half, clear of the art's Desolator.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GameOptionsLayout {
    pub(crate) dialog: RectPx,
    pub(crate) ok: RectPx,
    /// [`GAME_OPTIONS_PAGE`], left of OK.
    pub(crate) page: RectPx,
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
    /// [`SESSION_CHECKBOXES`], on Game Options' first page.
    pub(crate) session_checkboxes: [RectPx; 4],
    /// [`ROOM_CHECKBOXES`], on Game Options' second page.
    pub(crate) room_checkboxes: [RectPx; 8],
    /// [`TECH_LEVEL`]'s label, on Game Options' first page.
    pub(crate) tech_level_label: RectPx,
    /// [`SUPERWEAPON_LIMIT`]'s label, on Game Options' second page.
    pub(crate) superweapon_limit_label: RectPx,
    /// VERA's team bar under the roster, with the Teams count's label, the
    /// host's Room size's, and the host's Quick Team Setup, in a room with
    /// teams, and Bulk Seat Actions.
    pub(crate) team_bar: Option<RectPx>,
    pub(crate) teams_label: RectPx,
    pub(crate) room_size_label: RectPx,
    pub(crate) team_menu: Option<RectPx>,
    pub(crate) seat_menu: Option<RectPx>,
    pub(crate) trackbars: [(u16, RectPx); 8],
    pub(crate) labels: Vec<(&'static str, RectPx)>,
    pub(crate) option_labels: [(&'static str, RectPx); 3],
    pub(crate) game_options: Option<GameOptionsLayout>,
    /// VERA's missing-map offer, a screen-centred two-button box.
    pub(crate) map_offer: crate::ui::shell::modal::QuitConfirmLayout,
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
        let rect = |x, y, w, h| scale.scale_rect(RectPx::new(x, y, w, h));
        let (aw, ah) = if responsive {
            (800, 600)
        } else {
            (width, height)
        };
        // Resource templates of the anchored statics (BC/BD vs C2/C9).
        let (title, preview_x, label_x, map_label_y) = match (dialog.flavor, host) {
            (SetupFlavor::Lan, _) => (RectPx::new(425, 1, 108, 10), 429, 431, 116),
            (SetupFlavor::Wol, true) => (RectPx::new(424, 4, 108, 11), 432, 434, 118),
            (SetupFlavor::Wol, false) => (RectPx::new(424, 4, 108, 11), 433, 435, 118),
        };
        page.title = scale.scale_rect(anchor_rect(LOW_HEADING_ANCHOR, title, aw, ah));
        // Original BC/BD relayout60C4A0→60C0C0. Body controls stay at
        // resource coordinates; only the sidebar is anchored to screen size.
        page.status_help = scale.scale_rect(crate::ui::shell::layout::status_line_rect(
            RectPx::new(2, 355, 410, 12),
            aw,
            ah,
        ));
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
            session_checkboxes: [RectPx::new(0, 0, 0, 0); 4],
            room_checkboxes: [RectPx::new(0, 0, 0, 0); 8],
            tech_level_label: RectPx::new(0, 0, 0, 0),
            superweapon_limit_label: RectPx::new(0, 0, 0, 0),
            team_bar: None,
            teams_label: RectPx::new(0, 0, 0, 0),
            room_size_label: RectPx::new(0, 0, 0, 0),
            team_menu: None,
            seat_menu: None,
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
                (TECH_LEVEL, RectPx::new(0, 0, 0, 0)),
                (ROOM_SIZE, RectPx::new(0, 0, 0, 0)),
            ],
            labels,
            option_labels,
            game_options: None,
            map_offer: crate::ui::shell::modal::quit_confirm_layout(width, height),
            cooperative_description: rect(60, 98, 540, 143),
            map_preview: scale.scale_rect(anchor_rect(
                crate::ui::shell::descriptor::AnchorRule::RightAnchor,
                RectPx::new(preview_x, 23, 96, 69),
                aw,
                ah,
            )),
            mode_label: scale.scale_rect(anchor_rect(
                MAP_TEXT_ANCHOR,
                RectPx::new(label_x, 103, 90, 10),
                aw,
                ah,
            )),
            map_label: scale.scale_rect(anchor_rect(
                MAP_TEXT_ANCHOR,
                RectPx::new(label_x, map_label_y, 90, 20),
                aw,
                ah,
            )),
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
        // Where the roster's groups can stand two abreast with the chat at
        // its least, the chat takes no more.
        let narrow_chat = width - ROOM_GAP - CHAT_W.0;
        let chat_w = if RosterColumns::new(RectPx::new(left, 0, narrow_chat, 0)).pairs() {
            CHAT_W.0
        } else {
            (width * CHAT_SHARE.0 / CHAT_SHARE.1).clamp(CHAT_W.0, CHAT_W.1)
        };
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
        // The first page: Tech Level under Unit Count; the second: the
        // Superweapon Limit at the top.
        let tech_level = form_row(OPTION_TRACKBARS.len());
        for (id, bar) in &mut self.trackbars {
            if let Some(index) = OPTION_TRACKBARS.iter().position(|option| option == id) {
                *bar = scale.scale_rect(form_row(index).1);
            }
            match *id {
                TECH_LEVEL => *bar = scale.scale_rect(tech_level.1),
                SUPERWEAPON_LIMIT => *bar = scale.scale_rect(form_row(0).1),
                _ => {}
            }
        }
        self.tech_level_label = scale.scale_rect(tech_level.0);
        self.superweapon_limit_label = scale.scale_rect(form_row(0).0);
        // Two columns of checkboxes on each page, the left one over the art.
        // The first page has the native five, then beside them the four
        // C2 keeps hidden, Bases next to Short Game, which it ties to it.
        let column = |left: bool, top: i32, index: i32| {
            let y = top + 22 * index;
            if left {
                popup(24, y, 185, 17)
            } else {
                popup(215, y, 226, 17)
            }
        };
        for (index, checkbox) in self.checkboxes.iter_mut().enumerate() {
            *checkbox = column(true, 154, index as i32);
        }
        for (index, checkbox) in self.session_checkboxes.iter_mut().enumerate() {
            *checkbox = column(false, 154, index as i32);
        }
        // The second page: Destroyable Bridges, In-game Allying, Terrain
        // Deformation and Building Production Queue, then the four bans.
        let more = |left, index| column(left, 70, index);
        self.room_checkboxes = [
            more(true, 0),
            more(true, 1),
            more(false, 0),
            more(false, 1),
            more(false, 2),
            more(false, 3),
            more(true, 2),
            more(true, 3),
        ];
        self.game_options = Some(GameOptionsLayout {
            dialog: popup(0, 0, options.w, options.h),
            ok: popup(276, 292, 124, 24),
            page: popup(51, 292, 124, 24),
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
        self.labels = columns.labels(scale).collect();
        self.arrange_team_bar(true, host, host);
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
    fn arrange_team_bar(&mut self, teams: bool, host: bool, room_size: bool) {
        let scale = self.page.scale;
        let (mx, my) = ROSTER_BOX_MARGIN;
        let slider_rows = i32::from(teams) + i32::from(room_size);
        let menus = host && slider_rows > 0;
        let rows = slider_rows + i32::from(menus);
        let bar_h = slider_rows * TEAM_BAR_ROW_H + i32::from(menus) * TEAM_BAR_MENU_ROW_H;
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
        let menu = |column| {
            let part = TEAM_BAR_MENU.translate(column * (TEAM_BAR_MENU.w + TEAM_BAR_GAP), 0);
            at(part, slider_rows)
        };
        self.team_menu = (menus && teams).then(|| menu(0));
        self.seat_menu = menus.then(|| menu(i32::from(teams)));
    }
    /// Where VERA's roster's entries fit ([`roster_page`]).
    pub(crate) fn roster_frame(&self) -> RosterFrame {
        RosterFrame {
            bottom: self.roster_bottom,
            pairs: self.roster && self.roster_columns.pairs(),
        }
    }
    /// VERA's team bar for the room: with or without teams, and with or
    /// without the host's Room size ([`Self::arrange_team_bar`]).
    pub(crate) fn fit_team_bar(&mut self, dialog: SetupDialog, teams: bool, room_size: bool) {
        if self.roster {
            self.arrange_team_bar(teams, dialog.host, room_size);
        }
    }
    /// VERA's roster: each entry showing where [`roster_page`] puts it, a
    /// header's caption, Add AI and Join along its band, and a row's cells
    /// along its line. Groups standing two abreast have the column labels
    /// over each half.
    pub(crate) fn arrange_roster(&mut self, places: &[RosterPlace]) {
        let scale = self.page.scale;
        let rect = |x, y, w, h| scale.scale_rect(RectPx::new(x, y, w, h));
        if places.iter().any(|place| place.span != RosterSpan::Full) {
            let halves = [RosterSpan::Left, RosterSpan::Right];
            self.labels = halves
                .into_iter()
                .flat_map(|span| self.roster_columns.span(span).labels(scale))
                .collect();
        }
        for (position, place) in places.iter().enumerate().take(MODERN_SETUP_ROWS) {
            let columns = self.roster_columns.span(place.span);
            let (x, y) = (columns.x, place.y);
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
        // Each header's group: it and the rows under it, the entries up to
        // the next header's.
        for (position, place) in places.iter().enumerate().take(MODERN_SETUP_ROWS) {
            if !place.header {
                continue;
            }
            let end = places
                .iter()
                .filter(|next| next.header && next.item > place.item)
                .map(|next| next.item)
                .min()
                .unwrap_or(usize::MAX);
            let last = places
                .iter()
                .filter(|row| (place.item + 1..end).contains(&row.item))
                .map(|row| row.y + ROSTER_ROW_H)
                .max()
                .unwrap_or(place.y + ROSTER_HEADER_H);
            let columns = self.roster_columns.span(place.span);
            let (mx, my) = ROSTER_BOX_MARGIN;
            self.group_boxes[position] = Some(rect(
                columns.x - mx,
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
    /// VERA's roster: where the badge ([`SetupRowIcon::badge`]) of the
    /// player in Player cell `control` stands, in front of the name: the
    /// row's margin left of the cell, so names line up with or without one
    /// and keep their whole cell. `None` for a row without a badge.
    pub(crate) fn badge_rect(&self, view: &SetupView, control: u16) -> Option<RectPx> {
        let (SetupColumn::Player, row) = SetupColumn::decode(control)? else {
            return None;
        };
        view.row_icons.get(row)?.as_ref()?.badge?;
        let name = self.control_rect(control)?;
        let band = self.roster_rows[row];
        (self.roster && name.x > band.x)
            .then(|| RectPx::new(band.x, name.y, name.x - band.x, name.h))
    }
    /// VERA's roster: the Player cell whose badge stands at `x`, `y`.
    pub(crate) fn badge_at(&self, view: &SetupView, x: i32, y: i32) -> Option<u16> {
        (0..MODERN_SETUP_ROWS)
            .map(|row| SetupColumn::Player.control(row))
            .find(|&id| {
                self.badge_rect(view, id)
                    .is_some_and(|rect| rect.contains(x, y))
            })
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
        if let Some(index) = SESSION_CHECKBOXES
            .iter()
            .position(|candidate| *candidate == id)
        {
            return Some(self.session_checkboxes[index]);
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
            GAME_OPTIONS_PAGE => self.game_options.map(|popup| popup.page),
            CAMPAIGN_SERIES => Some(self.mode_label),
            CAMPAIGN_STAGE => Some(self.map_label),
            TEAM_MENU => self.team_menu,
            SEAT_MENU => self.seat_menu,
            MAP_OFFER_DOWNLOAD => Some(self.map_offer.ok),
            MAP_OFFER_LEAVE => Some(self.map_offer.cancel),
            _ => self.page.button_rect(id),
        }
    }
    /// An open Game Options popup takes every press; the room's own options
    /// are only in the popup when it exists.
    pub(crate) fn control_at(&self, view: &SetupView, x: i32, y: i32) -> Option<u16> {
        let hit = |id: &u16| {
            self.control_rect(*id)
                .is_some_and(|rect| rect.contains(x, y))
        };
        if view.map_offer_open {
            return [MAP_OFFER_DOWNLOAD, MAP_OFFER_LEAVE].into_iter().find(hit);
        }
        if let Some(page) = view.open_box {
            return game_options_controls(page)
                .into_iter()
                .chain([GAME_OPTIONS_PAGE, GAME_OPTIONS_OK])
                .find(hit);
        }
        let in_room = |id: &u16| {
            !matches!(*id, SUPERWEAPON_LIMIT | TECH_LEVEL)
                && (self.game_options.is_none() || !OPTION_TRACKBARS.contains(id))
        };
        let paged = view.pages.len() > 1;
        view.combos
            .iter()
            .filter(|combo| combo.visible)
            .map(|combo| combo.id)
            .chain(
                view.trackbars
                    .iter()
                    .filter(|bar| bar.visible)
                    .map(|bar| bar.id)
                    .filter(in_room),
            )
            .chain(
                CHECKBOXES
                    .into_iter()
                    .filter(|_| self.game_options.is_none()),
            )
            .chain(
                (0..MODERN_SETUP_ROWS)
                    .map(team_join)
                    .filter(|id| view.header_button(*id).is_some()),
            )
            .chain([CHAT, CHAT_LOG])
            .chain(
                [PREVIOUS_ROWS, NEXT_ROWS]
                    .into_iter()
                    .filter(|id| paged && view.enabled(*id)),
            )
            .chain(self.emote.map(|_| EMOTE))
            .chain(self.page.buttons.iter().map(|button| button.id))
            .find(hit)
    }
    /// What a press at `x`, `y` works: the enabled control there. In VERA's
    /// roster a cell that opens no list lets the press through to its
    /// group's box, which joins the group.
    pub(crate) fn press_target(&self, view: &SetupView, x: i32, y: i32) -> Option<u16> {
        let over = self
            .control_at(view, x, y)
            .filter(|id| view.enabled(*id))
            .filter(|id| !self.roster || view.combo(*id).is_none_or(roster_cell_opens));
        if !self.roster || over.is_some() {
            return over;
        }
        (0..MODERN_SETUP_ROWS).map(team_join).find(|id| {
            view.enabled(*id)
                && self.group_boxes[team_join_row(*id).unwrap_or(0)]
                    .is_some_and(|rect| rect.contains(x, y))
        })
    }
    /// The scale `id`'s art draws and takes the pointer at: the team bar's
    /// trackbars at 78% of the shell's.
    pub(crate) fn control_scale(&self, id: u16) -> ShellScale {
        let scale = self.page.scale;
        if self.team_bar.is_some() && matches!(id, TEAM_COUNT | ROOM_SIZE) {
            let window = scale.window();
            ShellScale::from_screen(window.w * 78 / 100, window.h * 78 / 100)
        } else {
            scale
        }
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
        ui.popup.open(SetupColumn::Country.control(5), Some(0));
        assert_eq!(ui.first_row(&pages), 24);
        ui.page_rows(&pages, false);
        assert_eq!(ui.first_row(&pages), 16);
        assert!(ui.popup.control.is_none());
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
            campaign: false,
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
            session_checks: [false; 4],
            room_checks: [false; 8],
            row_icons: std::array::from_fn(|_| None),
            headers: std::array::from_fn(|_| None),
            open_box: None,
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
            [TEAM_COUNT, ROOM_SIZE, TEAM_MENU, SEAT_MENU]
                .map(|id| layout.control_rect(id).unwrap().translate(-bar.x, -bar.y))
        };
        let [teams, size, team_menu, seat_menu] = controls(&regular);
        assert!(teams.y + teams.h <= size.y && size.y + size.h <= team_menu.y);
        assert!(team_menu.y == seat_menu.y && team_menu.x + team_menu.w < seat_menu.x);
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
        let frame = layout.roster_frame();
        assert!(!frame.pairs);
        let team_bar = layout.team_bar.unwrap();
        let area = layout.roster_area;
        assert!(area.y + area.h < team_bar.y && team_bar.x == area.x);
        let scroll = roster_scroll(&full, frame);
        // Each player's row and the empty spectators' header.
        assert_eq!(scroll.stops.len(), 46);
        let top = roster_page(&full, scroll.stops[0], frame);
        assert_eq!(
            top[..2],
            [
                RosterPlace {
                    item: 0,
                    y: ROSTER_TOP,
                    header: true,
                    span: RosterSpan::Full,
                },
                RosterPlace {
                    item: 1,
                    y: ROSTER_TOP + ROSTER_HEADER_H,
                    header: false,
                    span: RosterSpan::Full,
                }
            ]
        );
        assert!(top.last().unwrap().item < full.len() - 1);
        let last = roster_page(&full, scroll.stops[scroll.max_top], frame);
        assert_eq!(last.last().unwrap().item, full.len() - 1);
        let before = roster_page(&full, scroll.stops[scroll.max_top - 1], frame);
        assert!(before.last().unwrap().item < full.len() - 1);
        // Scrolled into Team B, its header stays on top.
        let into_b = roster_page(&full, 20, frame);
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
        ui.wheel(&view, &layout, area.x, area.y, -1.0);
        assert_eq!(ui.roster_top(scroll.max_top), 3);
        ui.wheel(&view, &layout, area.x, area.y, -100.0);
        assert_eq!(ui.roster_top(scroll.max_top), scroll.max_top);
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
            greyed: false,
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
        let press = |row: usize| {
            let (x, y) = center(layout.combos[row][SetupColumn::Country as usize]);
            layout.press_target(&view, x, y)
        };
        assert_eq!(press(1), Some(team_join(0)));
        assert_eq!(press(2), Some(team_join(0)));
        assert_eq!(press(3), Some(SetupColumn::Country.control(3)));
        // Its lists open on rows as tall as its own.
        ui.open_list(SetupColumn::Country.control(3), 3, Some(0));
        let popup = ui.popup_geometry(&view, &layout).unwrap();
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
        let popup = ui.popup_geometry(&view, &layout).unwrap();
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
        assert!(ui.popup_geometry(&view, &layout).is_none());

        // Fifteen teams of two: from every place, the column ends above
        // its bottom with no header left without its first row.
        let many: Vec<_> = (0..15).flat_map(|index| team(index, 2)).collect();
        let scroll = roster_scroll(&many, frame);
        assert_eq!(scroll.stops.len(), 30);
        for top in scroll.stops {
            let places = roster_page(&many, top, frame);
            let foot = places.last().unwrap();
            assert!(!foot.header);
            assert!(foot.y + ROSTER_ROW_H <= frame.bottom);
        }
    }

    /// On a 16:9 window VERA's roster stands its groups two abreast, Team A
    /// beside Team B and the next two under them, with the column labels
    /// over each half and both headers pinned over a line of their rows; a
    /// 16:10 window keeps one group under another.
    #[test]
    fn vera_roster_pairs_its_groups_on_a_wide_window() {
        use RosterItem::{Header, Row};
        use RosterSpan::{Left, Right};
        let room =
            |w, h| MultiplayerSetupLayout::new(dialog(true, SetupFlavor::Wol, true), w, h, true);
        assert!(!room(1920, 1200).roster_frame().pairs);
        let mut layout = room(1920, 1080);
        let frame = layout.roster_frame();
        assert!(frame.pairs);
        let team = |team: u8, rows: usize| {
            std::iter::once(Header(SetupGroup::Team(team))).chain(std::iter::repeat_n(Row, rows))
        };
        let items: Vec<_> = team(0, 5)
            .chain(team(1, 5))
            .chain(team(2, 6))
            .chain([Header(SetupGroup::Spectators)])
            .collect();
        let places = roster_page(&items, 0, frame);
        assert_eq!(places.len(), items.len());
        let at = |item| places.iter().find(|place| place.item == item).unwrap();
        assert_eq!((at(0).span, at(6).span), (Left, Right));
        assert_eq!((at(12).span, at(19).span), (Left, Right));
        assert_eq!((at(6).y, at(19).y), (at(0).y, at(12).y));
        assert_eq!(at(12).y, at(5).y + ROSTER_ROW_H + ROSTER_GROUP_GAP);
        // Team C's sixth row stands beside no other.
        assert_eq!(at(18).y, at(17).y + ROSTER_ROW_H);
        assert_eq!(roster_scroll(&items, frame).stops.len(), 11);

        layout.arrange_roster(&places);
        let group_box = |item| {
            let position = places.iter().position(|place| place.item == item);
            layout.group_boxes[position.unwrap()].unwrap()
        };
        let (a, b) = (group_box(0), group_box(6));
        assert_eq!((a.y, a.h), (b.y, b.h));
        assert!((a.w - b.w).abs() <= 1, "{a:?} {b:?}");
        let area = layout.roster_area;
        assert!(a.x >= area.x && a.x + a.w < b.x && b.x + b.w <= area.x + area.w);
        let players: Vec<i32> = layout
            .labels
            .iter()
            .filter(|(key, _)| *key == "GUI:Players")
            .map(|(_, rect)| rect.x)
            .collect();
        assert_eq!(players.len(), 2);
        assert!(players[0] < b.x && players[1] > b.x);
        // Scrolled to a line of Team A's and B's rows, both headers stay on
        // top.
        let into_b = roster_page(&items, 9, frame);
        let firsts: Vec<_> = into_b[..4]
            .iter()
            .map(|place| (place.item, place.header))
            .collect();
        assert_eq!(firsts, [(0, true), (6, true), (3, false), (9, false)]);
    }

    /// VERA's online room puts a player's badge in front of the name, in
    /// the row's margin left of the Player cell, where the pointer finds
    /// it; a row without a badge has none.
    #[test]
    fn a_room_players_badge_stands_in_front_of_the_name() {
        let mut layout =
            MultiplayerSetupLayout::new(dialog(true, SetupFlavor::Wol, true), 800, 600, false);
        let place = |item, y, header| RosterPlace {
            item,
            y,
            header,
            span: RosterSpan::Full,
        };
        layout.arrange_roster(&[
            place(0, ROSTER_TOP, true),
            place(1, ROSTER_TOP + ROSTER_HEADER_H, false),
            place(2, ROSTER_TOP + ROSTER_HEADER_H + ROSTER_ROW_H, false),
        ]);
        let mut view = vera_room_view();
        let icon = |badge| {
            Some(SetupRowIcon {
                country: None,
                host: false,
                accepted: false,
                badge,
                local: false,
            })
        };
        view.row_icons[1] = icon(Some("major.pcx"));
        view.row_icons[2] = icon(None);
        let name = SetupColumn::Player.control(1);
        let cell = layout.control_rect(name).unwrap();
        let badge = layout.badge_rect(&view, name).unwrap();
        assert!(
            badge.w > 0 && badge.x + badge.w <= cell.x,
            "{badge:?} {cell:?}"
        );
        assert_eq!((badge.y, badge.h), (cell.y, cell.h));
        let (x, y) = (badge.x + badge.w / 2, badge.y + badge.h / 2);
        assert_eq!(layout.badge_at(&view, x, y), Some(name));
        assert_eq!(
            layout.badge_rect(&view, SetupColumn::Player.control(2)),
            None
        );
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

        let mut ui = MultiplayerSetupState::default();
        ui.open_list(SetupColumn::Player.control(1), 1, Some(0));
        ui.press_row(1, 100, 100);
        ui.drag_row(103, 105, |_| Some(2));
        assert!(ui.popup.control.is_some());
        assert_eq!(ui.row_drag.and_then(|drag| drag.target), None);
        ui.drag_row(100, 106, |_| Some(2));
        assert!(ui.popup.control.is_none());
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
        // The checkboxes may cover the art; the trackbars and OK stay right
        // of it, and no page's controls overlap each other or its buttons.
        let overlap = |a: RectPx, b: RectPx| {
            a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
        };
        for page in [RoomBox::GameOptions, RoomBox::MoreOptions] {
            let rects: Vec<(u16, RectPx)> = game_options_controls(page)
                .into_iter()
                .chain([GAME_OPTIONS_PAGE, GAME_OPTIONS_OK])
                .map(|id| (id, popup_layout.control_rect(id).unwrap()))
                .collect();
            for (index, &(id, rect)) in rects.iter().enumerate() {
                let bar = OPTION_TRACKBARS.contains(&id)
                    || matches!(id, TECH_LEVEL | SUPERWEAPON_LIMIT | GAME_OPTIONS_OK);
                let left = if bar { 215 } else { 0 };
                assert!(
                    rect.x >= popup.dialog.x + left
                        && rect.x + rect.w <= popup.dialog.x + popup.dialog.w
                        && rect.y + rect.h <= popup.dialog.y + popup.dialog.h,
                    "{id:#x} at {rect:?}"
                );
                for &(other, beside) in &rects[index + 1..] {
                    assert!(!overlap(rect, beside), "{id:#x} overlaps {other:#x}");
                }
            }
        }
        let mut view = vera_room_view();
        view.open_box = Some(RoomBox::GameOptions);
        let center = |rect: RectPx| (rect.x + rect.w / 2, rect.y + rect.h / 2);
        let (x, y) = center(popup_layout.checkboxes[0]);
        assert_eq!(popup_layout.control_at(&view, x, y), Some(CHECKBOXES[0]));
        let (x, y) = center(popup_layout.chat);
        assert_eq!(popup_layout.control_at(&view, x, y), None);
        // Each page takes presses on its own options only.
        let (x, y) = center(popup_layout.room_checkboxes[0]);
        assert_eq!(popup_layout.control_at(&view, x, y), None);
        view.open_box = Some(RoomBox::MoreOptions);
        assert_eq!(
            popup_layout.control_at(&view, x, y),
            Some(ROOM_CHECKBOXES[0])
        );
        let (x, y) = center(popup_layout.checkboxes[0]);
        assert_ne!(popup_layout.control_at(&view, x, y), Some(CHECKBOXES[0]));
        // The team bar under the roster holds the Teams count, and the
        // host's Room size, Quick Team Setup and Bulk Seat Actions; a guest
        // has neither list.
        view.open_box = None;
        for id in [TEAM_MENU, SEAT_MENU] {
            view.combos.push(SetupCombo {
                id,
                row: u8::MAX,
                enabled: true,
                visible: true,
                selected: Some(0),
                choices: Vec::new(),
                list_w: 0,
            });
        }
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
        for id in [TEAM_COUNT, ROOM_SIZE, TEAM_MENU, SEAT_MENU] {
            let rect = popup_layout.control_rect(id).unwrap();
            assert!(
                rect.x >= bar.x
                    && rect.x + rect.w <= bar.x + bar.w
                    && rect.y >= bar.y
                    && rect.y + rect.h <= bar.y + bar.h,
                "{id:#x} at {rect:?}"
            );
            let (x, y) = center(rect);
            assert_eq!(popup_layout.control_at(&view, x, y), Some(id));
        }
        // At 800x600 Room size stacks under the Teams count, the two
        // trackbars in one column, and the lists stand side by side.
        let [teams, room, team_menu, seat_menu] = [TEAM_COUNT, ROOM_SIZE, TEAM_MENU, SEAT_MENU]
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
        assert!(team_menu.y == seat_menu.y && team_menu.x + team_menu.w < seat_menu.x);
        // Its art draws at 78% of the shell's size.
        assert_eq!(popup_layout.control_scale(TEAM_COUNT).factor(), 0.78);
        assert_eq!(popup_layout.control_scale(SPEED).factor(), 1.0);
        let mut guest =
            MultiplayerSetupLayout::new(dialog(false, SetupFlavor::Wol, true), 800, 600, false);
        assert_eq!(guest.control_rect(TEAM_MENU), None);
        assert_eq!(guest.control_rect(SEAT_MENU), None);
        // Without teams the host's bar keeps only Room size, on one line;
        // a guest's roster then reaches the column's foot.
        let mut no_teams = popup_layout.clone();
        no_teams.fit_team_bar(dialog(true, SetupFlavor::Wol, true), false, true);
        let room = no_teams.control_rect(ROOM_SIZE).unwrap();
        assert!(room.w > 0 && no_teams.team_bar.unwrap().y < room.y);
        assert_eq!(no_teams.control_rect(TEAM_COUNT).unwrap().w, 0);
        assert_eq!(no_teams.control_rect(TEAM_MENU), None);
        assert!(no_teams.control_rect(SEAT_MENU).is_some());
        assert!(no_teams.team_bar.unwrap().h < popup_layout.team_bar.unwrap().h);
        guest.fit_team_bar(dialog(false, SetupFlavor::Wol, true), false, false);
        assert_eq!(guest.team_bar, None);
        assert_eq!(guest.roster_bottom, ROSTER_BOTTOM);
        view.open_box = None;
        // VERA's missing-map offer takes every press until it is answered.
        view.map_offer_open = true;
        let (x, y) = center(popup_layout.chat);
        assert_eq!(popup_layout.control_at(&view, x, y), None);
        let (x, y) = center(popup_layout.map_offer.ok);
        assert_eq!(
            popup_layout.control_at(&view, x, y),
            Some(MAP_OFFER_DOWNLOAD)
        );
        view.map_offer_open = false;
        let (x, y) = center(popup_layout.checkboxes[0]);
        assert_ne!(popup_layout.control_at(&view, x, y), Some(CHECKBOXES[0]));
        view.dialog = dialog(true, SetupFlavor::Wol, false);
        let native = MultiplayerSetupLayout::new(view.dialog, 800, 600, false);
        assert!(native.game_options.is_none());
        let (x, y) = center(native.checkboxes[0]);
        assert_eq!(native.control_at(&view, x, y), Some(CHECKBOXES[0]));
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
