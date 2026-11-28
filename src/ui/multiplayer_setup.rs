//! Network setup presentation: LAN host BC (005DE520) and guest BD
//! (005E1CA0), WOL host C2 (0078F7F0) and guest C9 (00791B10). The WOL pages
//! are supersets of the LAN ones with the same control ids (resources C2/C9).
//! MatchSession owns admitted participants, options, readiness and map status.
//! This leaf keeps only control interaction and emits commands for that owner.

use crate::util::time::Instant;
use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use crate::ui::lan_shell::LanChatState;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{
    RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellAlign, ShellScale, child_window,
};
use crate::ui::shell::layout::Placer;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::page::{self as page_nodes, PageEvent};
use crate::ui::skirmish_shell::SkirmishStatics;
use vera20k_gui::shell::combobox::ComboPopup;
use vera20k_gui::shell::list::{ListMetrics, ListScrollPart, ShellListGeometry};
use vera20k_gui::shell::trackbar::TrackbarRange;

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
/// The offer's text, a static.
pub(crate) const MAP_OFFER_BODY: u16 = 0x7F34;
/// The offer's download bar under its text, while the map comes.
pub(crate) const MAP_OFFER_PROGRESS: u16 = 0x7F36;
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
/// The statics captioning [`OPTION_TRACKBARS`], [`SUPERWEAPON_LIMIT`] and
/// [`TECH_LEVEL`].
pub(crate) const OPTION_CAPTIONS: [u16; 3] = [0x7F30, 0x7F31, 0x7F32];
pub(crate) const SUPERWEAPON_LIMIT_CAPTION: u16 = 0x7F33;
pub(crate) const TECH_LEVEL_CAPTION: u16 = 0x7F35;
/// [`CHECKBOXES`]' captions.
const CHECKBOX_LABELS: [&str; 5] = [
    "GUI:ShortGame",
    "GUI:MCVRepacks",
    "GUI:CratesAppear",
    "GUI:SuperWeaponsAllowed",
    "GUI:BuildOffAlly",
];
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
    dialog: DialogResourceId(0xBC),
    title_key: "GUI:HostScreen",
    stacked: &[
        MenuPageButtonSpec::new(START, 147, "GUI:StartGame", "STT:HostButtonGo"),
        MenuPageButtonSpec::new(CHOOSE_MAP, 176, "GUI:ChooseMap", "STT:HostButtonChooseMap"),
    ],
    back: MenuPageButtonSpec::new(BACK, 346, "GUI:Back", "STT:HostButtonBack"),
};
pub(crate) const GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogResourceId(0xBD),
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
    dialog: DialogResourceId(0xC2),
    title_key: "GUI:HostScreen",
    stacked: &[
        MenuPageButtonSpec::new(START, 156, "GUI:StartGame", "STT:HostButtonGo"),
        MenuPageButtonSpec::new(CHOOSE_MAP, 183, "GUI:ChooseMap", "STT:HostButtonChooseMap"),
        MenuPageButtonSpec::new(PAGE, 210, "GUI:PageButton", "STT:WOLPageButton"),
    ],
    back: WOL_BACK,
};
pub(crate) const WOL_GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogResourceId(0xC9),
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
    /// Its range as its window holds it: Co-op5C21D0 sends7324AC(0), so
    /// Difficulty runs with no value-plaque reserve; ordinary options retain
    /// the49-pixel plaque.
    pub(crate) fn window_range(&self) -> TrackbarRange {
        self.range.with_plaque(self.id != DIFFICULTY)
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

/// The host's drag of a roster row, as the room's widgets hold it
/// ([`row_drag`]): pressed on `row`, carried once the pointer leaves the
/// press by [`ROW_DRAG_START`], over the box whose header is page row
/// `target` when that box takes the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowDrag {
    pub(crate) row: u8,
    pub(crate) at: (i32, i32),
    pub(crate) carrying: bool,
    pub(crate) target: Option<usize>,
}
/// How far, in window pixels, the pointer moves before a pressed row is
/// carried; a shorter press stays a click on its cell (the cells'
/// `DragThreshold`).
const ROW_DRAG_START: i32 = 6;
/// VERA's roster's group boxes, a row's drop targets: the box whose header
/// is page row `row` is `GROUP_BOX + row`.
pub(crate) const GROUP_BOX: u16 = 0x7F40;

/// The room's own texts, boxes and pictures, statics that take no pointer
/// ([`page_spec`]), each painted from its node; the per-row ones are
/// `first + row` of page row `row`. The column labels over the cells
/// (`COLUMN_LABEL + index` of [`MultiplayerSetupLayout::labels`]).
pub(crate) const COLUMN_LABEL: u16 = 0x7F80;
/// "Players 1-8 of 12" between Previous and Next.
pub(crate) const ROSTER_PAGE_LINE: u16 = 0x7F8A;
/// VERA's team bar's box and its trackbars' captions.
pub(crate) const TEAM_BAR_BOX: u16 = 0x7F8B;
pub(crate) const TEAMS_CAPTION: u16 = 0x7F8C;
pub(crate) const ROOM_SIZE_CAPTION: u16 = 0x7F8D;
/// A cooperative map's description, and the Difficulty trackbar's three
/// words (`DIFFICULTY_LABEL + 0..3`).
pub(crate) const COOPERATIVE_TEXT: u16 = 0x7F8E;
pub(crate) const DIFFICULTY_LABEL: u16 = 0x7F90;
/// The panels that hold them, painted whole: the column labels, the
/// roster's headers (caption and Join per row), the local player's band,
/// the rows' pictures, and the cooperative texts.
pub(crate) const ROOM_LABELS: u16 = 0x7F93;
pub(crate) const ROOM_HEADERS: u16 = 0x7F94;
pub(crate) const ROOM_BANDS: u16 = 0x7F95;
pub(crate) const ROOM_ICONS: u16 = 0x7F96;
pub(crate) const ROOM_NOTES: u16 = 0x7F97;
pub(crate) const ROOM_BOXES: u16 = 0x7F98;
pub(crate) const ROOM_TEAM_BAR: u16 = 0x7F99;
/// A roster header's caption, over its Player cell.
pub(crate) const HEADER_CAPTION: u16 = 0x7200;
/// A roster header's Join (or Watch) caption, at its band's right end.
pub(crate) const JOIN_CAPTION: u16 = 0x7240;
/// The dark band behind the local player's roster row.
pub(crate) const OWN_ROW: u16 = 0x7280;
/// A player's badge in front of their name (VERA's online room).
pub(crate) const BADGE: u16 = 0x72C0;
/// gamemd's rooms: a row's country flag and its host or accepted icon.
pub(crate) const ROW_FLAG: u16 = 0x7300;
pub(crate) const ROW_STATUS: u16 = 0x7340;
/// The Difficulty trackbar's words and where they stand (800x600).
const DIFFICULTY_WORDS: [(&str, i32); 3] =
    [("TXT_Easy", 170), ("TXT_Normal", 295), ("TXT_Hard", 430)];

/// The page row of the per-row painted node `id` in the family `first`.
pub(crate) fn painted_row(id: u16, first: u16) -> Option<usize> {
    page_row(id, first)
}

/// A page of VERA's Game Options box over the room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RoomBox {
    /// Game Options: the match options gamemd's room keeps.
    #[default]
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
    /// [`GAME_OPTIONS_PAGE`]'s caption: the page it turns to.
    pub(crate) fn page_button_label(self) -> &'static str {
        match self {
            Self::GameOptions => "VERA:MoreOptions",
            Self::MoreOptions => "VERA:MainOptions",
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

/// Whether control `id` of the Game Options box shows on `page`: the
/// page's options with their captions, the page button and OK. A campaign
/// room's box holds only its speed (no page button).
fn on_game_options_page(view: &SetupView, page: RoomBox, id: u16) -> bool {
    if view.campaign {
        return matches!(id, SPEED | GAME_OPTIONS_OK) || id == OPTION_CAPTIONS[0];
    }
    if let Some(index) = OPTION_CAPTIONS.iter().position(|caption| *caption == id) {
        return page == RoomBox::GameOptions && view.trackbar_shows(OPTION_TRACKBARS[index]);
    }
    match id {
        GAME_OPTIONS_OK | GAME_OPTIONS_PAGE => true,
        TECH_LEVEL_CAPTION => page == RoomBox::GameOptions && view.trackbar_shows(TECH_LEVEL),
        SUPERWEAPON_LIMIT_CAPTION => {
            page == RoomBox::MoreOptions && view.trackbar_shows(SUPERWEAPON_LIMIT)
        }
        id => game_options_controls(page).contains(&id),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SetupView {
    pub(crate) campaign: bool,
    pub(crate) first_row: usize,
    pub(crate) row_count: usize,
    /// The first row of each page.
    pub(crate) pages: Vec<usize>,
    /// In VERA's roster, where the entries showing stand ([`roster_page`]).
    pub(crate) places: Vec<RosterPlace>,
    /// The rows (headers too) on the page showing.
    pub(crate) page_len: usize,
    /// VERA's roster: how many places it scrolls through ([`RosterScroll`])
    /// and the last; [`Self::first_row`] is the place it shows.
    pub(crate) roster_scroll: (usize, usize),
    pub(crate) dialog: SetupDialog,
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
    /// Online, Lobby: the lobby offers visits to the public lobby.
    pub(crate) lobby_enabled: bool,
    /// The rows the host may drag to another box, in a room with teams.
    pub(crate) drag_rows: Vec<DragRow>,
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
    /// Whether trackbar `id` shows.
    pub(crate) fn trackbar_shows(&self, id: u16) -> bool {
        self.trackbars.iter().any(|bar| bar.id == id && bar.visible)
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
            START => self.dialog.host,
            CHOOSE_MAP => self.dialog.host && !self.campaign,
            PAGE => self.dialog.wol(),
            PREVIOUS_ROWS => self.first_row > 0,
            NEXT_ROWS => self.first_row + self.page_len < self.row_count,
            ACCEPT => !self.dialog.host && self.accept_enabled,
            0x54E | BASES if self.short_game_locked => false,
            id if CHECKBOXES.contains(&id)
                || SESSION_CHECKBOXES.contains(&id)
                || ROOM_CHECKBOXES.contains(&id) =>
            {
                self.dialog.host && !self.campaign
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
    /// VERA's Game Options box over the room opens on, or turns to, a page,
    /// or closes (`None`).
    GameOptions(Option<RoomBox>),
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
    /// The first row of a native roster's page showing (Previous / Next).
    /// VERA's roster keeps the place it is scrolled to on its scrollbar
    /// node [`ROSTER_SCROLL`] instead.
    first_visible_row: usize,
    /// VERA's roster's place while the room hides behind the lobby (its
    /// dialog and scrollbar are torn down), for when it shows again.
    pub(crate) parked_roster_top: Option<usize>,
    pub(crate) chat: LanChatState,
    /// The chat's draft and view while the room hides behind the lobby
    /// (its dialog is torn down), for when it shows again.
    pub(crate) parked_chat: Option<crate::ui::lan_shell::ParkedChat>,
    pub(crate) notice: Option<crate::ui::shell::modal::ShellBox>,
    /// The row the open list belongs to. Joins, leaves, moves and paging
    /// put another row under its control, and a list whose row moved away
    /// closes, so a pick never changes whoever took its place.
    pub(crate) popup_row: Option<u8>,
    pub(crate) statics: SkirmishStatics,
    pub(crate) shown: bool,
    accept_flash_started: Option<Instant>,
    /// VERA's Game Options box is open over the room; it takes the room's
    /// input.
    pub(crate) game_options: bool,
    /// The page the Game Options box shows.
    pub(crate) game_options_page: RoomBox,
    /// VERA's missing-map offer; open until its map arrives, it takes the
    /// room's input.
    pub(crate) map_offer: Option<MapOffer>,
    /// The maps whose briefing VERA's room has looked for, by file name, so
    /// it points each one out once while the room stays open.
    briefings_checked: Vec<String>,
    /// The room's painted nodes, found when its page was built.
    pub(crate) room_nodes: std::cell::Cell<RoomNodes>,
}
impl Default for MultiplayerSetupState {
    fn default() -> Self {
        Self {
            first_visible_row: 0,
            parked_roster_top: None,
            chat: LanChatState::default(),
            parked_chat: None,
            notice: None,
            popup_row: None,
            statics: SkirmishStatics::default(),
            shown: false,
            accept_flash_started: None,
            game_options: false,
            game_options_page: RoomBox::GameOptions,
            map_offer: None,
            briefings_checked: Vec::new(),
            room_nodes: std::cell::Cell::default(),
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
    }
    /// An open roster list owns the wheel even at its ends. Once it closes,
    /// the same gesture over the roster (its area node `area`) scrolls the
    /// teams again: its scrollbar node `scroll` ([`ROSTER_SCROLL`], one
    /// place a step) moves and reports `Scrolled`, on which the host closes
    /// the open list.
    pub(crate) fn wheel(
        &self,
        gui: &mut Gui,
        area: Option<NodeId>,
        scroll: Option<NodeId>,
        view: &SetupView,
        layout: &MultiplayerSetupLayout,
        x: i32,
        y: i32,
        lines: f32,
        now: u64,
    ) -> bool {
        if layout.roster_scroll(view).is_none() {
            return false;
        }
        if self.game_options || self.map_offer.is_some() || lines == 0.0 {
            return false;
        }
        // Three places a notch of fractional lines, at least one, at most 30.
        let step = ((lines * 3.0).round() as i32).clamp(-30, 30);
        let step = if step == 0 {
            lines.signum() as i32
        } else {
            step
        };
        if gui.wheel_popup(-step, x, y, now) {
            return true;
        }
        // Over the roster, its cells or the gaps between them.
        if area.is_some_and(|area| gui.hit_at(area, x, y, false).is_some()) {
            if let Some(scroll) = scroll {
                gui.wheel_list(scroll, step, now);
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
    /// Opens the Game Options box on `page`, turns it to `page`, or closes
    /// it (`None`).
    pub(crate) fn show_game_options(&mut self, page: Option<RoomBox>) {
        self.game_options = page.is_some();
        self.game_options_page = page.unwrap_or_default();
    }
    pub(crate) fn accept_highlight(&mut self, enabled: bool, now: Instant) -> bool {
        if !enabled {
            self.accept_flash_started = None;
            return false;
        }
        // 612B70's 4DC timer highlight, from the first frame it is armed.
        let started = *self.accept_flash_started.get_or_insert(now);
        let elapsed = now.saturating_duration_since(started).as_millis() as u64;
        vera20k_gui::time::blink_phase(0, elapsed)
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

/// The roster's column captions (ids `0xFF00..`) and the option trackbars'
/// captions.
const LABELS: [&str; 5] = [
    "GUI:Players",
    "GUI:Side",
    "GUI:Color",
    "GUI:StartPosition",
    "GUI:Team",
];
const OPTION_LABELS: [(u16, &str); 3] = [
    (OPTION_CAPTIONS[0], "GUI:GameSpeed"),
    (OPTION_CAPTIONS[1], "GUI:Credits"),
    (OPTION_CAPTIONS[2], "GUI:UnitCount"),
];

#[derive(Debug, Clone)]
pub(crate) struct MultiplayerSetupLayout {
    pub(crate) page: MenuPageLayout,
    /// Whether VERA's send key sits beside the room's chat edit, narrowing
    /// that edit. Off keeps the native room layout.
    touch_send: bool,
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
    pub(crate) map_offer: crate::ui::shell::modal::BoxParts,
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
            let scale = crate::ui::shell::geom::shell_scale(width, height);
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
        // `shell_layout.ini`'s BC/BD/C2/C9: the original relayout
        // 0x60C4A0 -> 0x60C0C0 keeps the body at its resource places and
        // anchors the heading, status line, preview and map texts.
        let section = match (dialog.flavor, host) {
            (SetupFlavor::Lan, true) => "0xBC",
            (SetupFlavor::Lan, false) => "0xBD",
            (SetupFlavor::Wol, true) => "0xC2",
            (SetupFlavor::Wol, false) => "0xC9",
        };
        let rects = layout_data::place(section, scale, width, height);
        let at = |id| rects.get(id).unwrap_or_default();
        page.title = at(0x694);
        page.status_help = at(0x695);
        let combos = std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                let (x, w) = [(49, 100), (201, 78), (294, 27), (334, 25), (374, 25)][column];
                let window = child_window(x, 22 + row as i32 * 16, w, 74);
                scale.scale_rect(RectPx::new(window.x, window.y, window.w, 24))
            })
        });
        // C2's sixth row icons sit 2 px higher (template 101); VERA keeps
        // C9's at 166.
        let icon_y = |row: usize| 36 + row as i32 * 26;
        let mut layout = Self {
            page,
            touch_send: true,
            roster: false,
            combos,
            header_controls: [Default::default(); MODERN_SETUP_ROWS],
            roster_rows: [RectPx::new(0, 0, 0, 0); MODERN_SETUP_ROWS],
            group_boxes: [None; MODERN_SETUP_ROWS],
            roster_navigation: [at(PREVIOUS_ROWS), at(NEXT_ROWS)],
            roster_page_label: at(0xFF05),
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
            emote: rects.get(EMOTE),
            chat: at(CHAT),
            chat_log: at(CHAT_LOG),
            checkboxes: CHECKBOXES.map(at),
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
                (SPEED, at(SPEED)),
                (CREDITS, at(CREDITS)),
                (UNITS, at(UNITS)),
                (DIFFICULTY, at(DIFFICULTY)),
                (TEAM_COUNT, RectPx::new(0, 0, 0, 0)),
                (SUPERWEAPON_LIMIT, RectPx::new(0, 0, 0, 0)),
                (TECH_LEVEL, RectPx::new(0, 0, 0, 0)),
                (ROOM_SIZE, RectPx::new(0, 0, 0, 0)),
            ],
            labels: LABELS
                .iter()
                .zip(0xFF00..)
                .map(|(key, id)| (*key, at(id)))
                .collect(),
            option_labels: OPTION_LABELS.map(|(id, key)| (key, at(id))),
            game_options: None,
            map_offer: crate::ui::shell::modal::menu_message_box(2, width, height),
            notice_ok: crate::ui::shell::modal::menu_message_box(1, width, height).button(0),
            cooperative_description: at(0xFF06),
            map_preview: at(MAP_PREVIEW),
            mode_label: at(0x6EC),
            map_label: at(0x5A8),
        };
        if dialog.modern {
            layout.move_options_into_popup(dialog.wol(), host);
        }
        layout
    }

    /// The layout with VERA's send key beside the room's chat edit.
    pub(crate) fn with_touch_send(mut self, on: bool) -> Self {
        self.touch_send = on;
        self
    }

    /// The online room already has an emote button beside its chat edit.
    fn shows_chat_send(&self, dialog: SetupDialog) -> bool {
        self.touch_send && !dialog.wol()
    }

    /// The chat edit leaves room for the LAN send button when it is shown.
    pub(crate) fn chat_text(&self, dialog: SetupDialog) -> RectPx {
        if self.shows_chat_send(dialog) {
            crate::ui::shell::geom::room_chat_text_rect(self.chat, self.chat_log, self.page.scale)
        } else {
            self.chat
        }
    }

    /// VERA's send key beside the chat edit, when this platform shows it.
    pub(crate) fn chat_send(&self, dialog: SetupDialog) -> Option<RectPx> {
        self.shows_chat_send(dialog).then(|| {
            crate::ui::shell::geom::room_chat_send_rect(self.chat, self.chat_log, self.page.scale)
        })
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
            self.flags[row] = move_up(self.flags[row]);
            self.acceptance[row] = move_up(self.acceptance[row]);
        }
    }
    pub(crate) fn control_rect(&self, id: u16) -> Option<RectPx> {
        if let Some((column, row)) = SetupColumn::decode(id) {
            return Some(self.combos[row][column as usize]);
        }
        // Only VERA's roster has headers; a group's Join is its whole box.
        if let Some(row) = team_join_row(id) {
            return self.group_boxes[row];
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
        if let Some(index) = OPTION_CAPTIONS
            .iter()
            .position(|candidate| *candidate == id)
        {
            return Some(self.option_labels[index].1);
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
            SUPERWEAPON_LIMIT_CAPTION => self.game_options.map(|_| self.superweapon_limit_label),
            TECH_LEVEL_CAPTION => self.game_options.map(|_| self.tech_level_label),
            CAMPAIGN_SERIES => Some(self.mode_label),
            CAMPAIGN_STAGE => Some(self.map_label),
            TEAM_MENU => self.team_menu,
            SEAT_MENU => self.seat_menu,
            MAP_OFFER_DOWNLOAD => Some(self.map_offer.button(0)),
            MAP_OFFER_BODY => Some(self.map_offer.body),
            MAP_OFFER_PROGRESS => Some(self.map_offer.progress_track()),
            crate::ui::shell::modal::control::OK => Some(self.notice_ok),
            MAP_OFFER_LEAVE => Some(self.map_offer.button(1)),
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
    /// trackbars at 78% of the shell's.
    pub(crate) fn control_scale(&self, id: u16) -> ShellScale {
        match self.team_bar_screen() {
            Some((w, h)) if matches!(id, TEAM_COUNT | ROOM_SIZE) => {
                crate::ui::shell::geom::shell_scale(w, h)
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
        // The team bar's lists would run past the status line under it, so
        // they open upward.
        if matches!(id, TEAM_MENU | SEAT_MENU)
            && popup.rect.y + popup.rect.h > self.page.status_help.y
        {
            popup.rect.y = face.y - 1 - popup.rect.h;
        }
        Some(popup)
    }
    /// The chat's text, as a share of the shell's own size: the roster's
    /// in VERA's room.
    pub(crate) fn chat_text_scale(&self) -> f32 {
        if self.roster { ROSTER_TEXT_SCALE } else { 1.0 }
    }
    /// The chat log's rows showing at once, at the chat's text size.
    pub(crate) fn chat_visible_rows(&self) -> usize {
        let metrics = ListMetrics::for_text(self.page.scale, self.chat_text_scale());
        ShellListGeometry::new(self.chat_log, 0, 0, metrics).visible_rows
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
/// to ([`RosterScroll`]), hit only on its bar. Its `Top` is the place the
/// roster shows: the bar, the wheel and parking move it, the view reads it.
pub(crate) const ROSTER_SCROLL: u16 = 0x7F1A;
/// VERA's roster's whole area, a static that only says the pointer is over
/// the roster (the wheel scrolls it there), behind its cells.
pub(crate) const ROSTER_AREA: u16 = 0x7F1B;
/// The boxes over VERA's room in the shell controller, which take the
/// room's input while they are open.
pub(crate) const GAME_OPTIONS_BOX: DialogResourceId = DialogResourceId(GAME_OPTIONS);
pub(crate) const MAP_OFFER_BOX: DialogResourceId = DialogResourceId(MAP_OFFER_DOWNLOAD);
/// The shared message box (`0x005D3490`) over the room, which owns all
/// input while it runs.
pub(crate) const NOTICE_BOX: DialogResourceId = DialogResourceId(0xCE);

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
    // VERA's room: the host's team bar lists and a campaign room's series
    // and stage lists ([`SetupView::combos`]).
    let lists = [TEAM_MENU, SEAT_MENU, CAMPAIGN_SERIES, CAMPAIGN_STAGE]
        .into_iter()
        .filter(move |_| dialog.modern);
    let buttons = rows
        .map(team_join)
        .chain([PREVIOUS_ROWS, NEXT_ROWS, EMOTE])
        .chain(dialog.page().buttons().map(|button| button.id));
    let list = |id| {
        node(NodeKind::List, id)
            .with(Prop::RowCount, Value::Int(0))
            .with(Prop::TextInset, Value::Rect(RectPx::new(2, 0, 2, 0)))
    };
    let button = |id| match id {
        PREVIOUS_ROWS => node(NodeKind::Button, id).label("VERA:Previous"),
        NEXT_ROWS => node(NodeKind::Button, id).label("VERA:Next"),
        id => node(NodeKind::Button, id),
    };
    let help = tooltip_key(dialog, CHAT).unwrap_or_default();
    // VERA's room keeps its options in Game Options; Difficulty stays.
    let modern = dialog.modern;
    let native = move |ids: &'static [u16]| ids.iter().copied().filter(move |_| !modern);
    let trackbars = native(&OPTION_TRACKBARS).chain([DIFFICULTY, TEAM_COUNT, ROOM_SIZE]);
    NodeSpec::new(NodeKind::Dialog)
        // A badge's status help names its player, over Join's box too; a
        // press goes through it.
        .child(icons_spec(dialog))
        .children(combos.chain(lists).map(|id| {
            let combo =
                node(NodeKind::Combo, id).with(Prop::OpenAnywhere, Value::Bool(dialog.modern));
            // VERA's roster's Start and Team cells centre their value.
            match SetupColumn::decode(id) {
                Some((SetupColumn::Start | SetupColumn::Team, _)) => {
                    combo.with(Prop::Align, centred())
                }
                _ => combo,
            }
        }))
        .children(option_spec(native(&CHECKBOXES), trackbars))
        .children(buttons.map(button))
        .child(node(NodeKind::Edit, CHAT).tooltip(&help))
        .child(list(CHAT_LOG))
        .child(list(ROSTER_SCROLL))
        .children(option_captions(native(&OPTION_CAPTIONS)))
        // The map preview `0x468`, a static that takes no press: VERA's
        // enlarged preview opens from a press on it (`preview_zoom`). It
        // shows only while it holds a picture.
        .child(crate::ui::shell::menu_page::hover_target(MAP_PREVIEW, ""))
        .child(node(NodeKind::Text, ROSTER_AREA).tooltip(""))
        // Each group's box takes a carried roster row (no press).
        .child(
            node(NodeKind::Panel, ROOM_BOXES).children((0..MODERN_SETUP_ROWS as u16).map(|row| {
                node(NodeKind::Text, GROUP_BOX + row).with(Prop::DropTarget, Value::Bool(true))
            })),
        )
        .child(node(NodeKind::Text, ROSTER_PAGE_LINE).with(Prop::Align, centred()))
        .children(painted_spec(dialog))
}

/// The rows' pictures: VERA's roster's badges, which hover with their
/// player's help, or gamemd's flags and status icons.
fn icons_spec(dialog: SetupDialog) -> NodeSpec {
    let rows = 0..dialog.rows_per_page() as u16;
    let icons = rows.flat_map(move |row| match dialog.modern {
        true => vec![
            node(NodeKind::Image, BADGE + row)
                .tooltip("")
                .with(Prop::PressThrough, Value::Bool(true)),
        ],
        false => vec![
            node(NodeKind::Image, ROW_FLAG + row),
            node(NodeKind::Image, ROW_STATUS + row),
        ],
    });
    node(NodeKind::Panel, ROOM_ICONS).children(icons)
}

fn centred() -> Value {
    Value::Int(i32::from((ShellAlign::H_CENTER | ShellAlign::V_CENTER).0))
}

/// The room's painted statics ([`COLUMN_LABEL`]..): Text and Image nodes
/// that take no pointer, in panels in the order they paint, VERA's
/// roster's per-row ones for its rows and gamemd's flags for its own.
fn painted_spec(dialog: SetupDialog) -> impl Iterator<Item = NodeSpec> {
    let modern = dialog.modern;
    let rows = 0..dialog.rows_per_page() as u16;
    let align = |align: ShellAlign| Value::Int(i32::from(align.0));
    // Over VERA's roster, the narrow cells' labels centre on them.
    let labels = (0..2 * LABELS.len() as u16).map(move |index| {
        let narrow = modern && usize::from(index) % LABELS.len() >= 2;
        let at = if narrow {
            ShellAlign::H_CENTER
        } else {
            ShellAlign::NONE
        };
        node(NodeKind::Text, COLUMN_LABEL + index).with(Prop::Align, align(at))
    });
    let headers = rows.clone().filter(move |_| modern).flat_map(move |row| {
        let join = ShellAlign::H_RIGHT | ShellAlign::V_CENTER;
        [
            node(NodeKind::Text, HEADER_CAPTION + row),
            node(NodeKind::Text, JOIN_CAPTION + row).with(Prop::Align, align(join)),
        ]
    });
    let bands = rows
        .filter(move |_| modern)
        .map(|row| node(NodeKind::Text, OWN_ROW + row));
    let notes =
        std::iter::once(node(NodeKind::Text, COOPERATIVE_TEXT).with(Prop::Align, Value::Int(0)))
            .chain((0..DIFFICULTY_WORDS.len() as u16).map(move |index| {
                let (key, _) = DIFFICULTY_WORDS[usize::from(index)];
                node(NodeKind::Text, DIFFICULTY_LABEL + index)
                    .label(key)
                    .with(Prop::Align, align(ShellAlign::H_CENTER))
            }));
    [
        node(NodeKind::Panel, ROOM_LABELS).children(labels),
        node(NodeKind::Panel, ROOM_HEADERS).children(headers),
        node(NodeKind::Panel, ROOM_BANDS).children(bands),
        node(NodeKind::Panel, ROOM_NOTES).children(notes),
        node(NodeKind::Panel, ROOM_TEAM_BAR).children([
            node(NodeKind::Text, TEAM_BAR_BOX),
            node(NodeKind::Text, TEAMS_CAPTION),
            node(NodeKind::Text, ROOM_SIZE_CAPTION),
        ]),
    ]
    .into_iter()
}

/// The room's painted nodes and the roster's boxes, found once when its
/// page is built ([`sync`]), so a frame paints them without a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RoomNodes {
    pub(crate) root: Option<NodeId>,
    pub(crate) labels: Option<NodeId>,
    pub(crate) headers: Option<NodeId>,
    pub(crate) boxes: Option<NodeId>,
    pub(crate) bands: Option<NodeId>,
    pub(crate) icons: Option<NodeId>,
    pub(crate) notes: Option<NodeId>,
    pub(crate) team_bar: Option<NodeId>,
    pub(crate) page_line: Option<NodeId>,
    pub(crate) scroll: Option<NodeId>,
    pub(crate) chat: Option<NodeId>,
    pub(crate) emote: Option<NodeId>,
}

impl RoomNodes {
    /// `root`'s children by id, in one pass.
    fn find(gui: &Gui, root: NodeId) -> Self {
        let mut nodes = RoomNodes {
            root: Some(root),
            ..RoomNodes::default()
        };
        for &child in gui.tree().children(root).unwrap_or_default() {
            let Some(id) = gui.id_of(child).and_then(|id| u16::try_from(id).ok()) else {
                continue;
            };
            let slot = match id {
                ROOM_LABELS => &mut nodes.labels,
                ROOM_HEADERS => &mut nodes.headers,
                ROOM_BOXES => &mut nodes.boxes,
                ROOM_BANDS => &mut nodes.bands,
                ROOM_ICONS => &mut nodes.icons,
                ROOM_NOTES => &mut nodes.notes,
                ROOM_TEAM_BAR => &mut nodes.team_bar,
                ROSTER_PAGE_LINE => &mut nodes.page_line,
                ROSTER_SCROLL => &mut nodes.scroll,
                CHAT => &mut nodes.chat,
                EMOTE => &mut nodes.emote,
                _ => continue,
            };
            *slot = Some(child);
        }
        nodes
    }
}

/// The room's painted statics and their panels, which take no session
/// state ([`props`]).
fn painted(id: u16) -> bool {
    (COLUMN_LABEL..=ROOM_TEAM_BAR).contains(&id)
        || (HEADER_CAPTION..ROW_STATUS + MODERN_SETUP_ROWS as u16).contains(&id)
}

/// The column label `index` of `layout`'s, as the room words it: its key
/// (Side is a campaign's role), `None` where the room shows none (Team
/// without teams, a campaign's Colour, Start and Team).
pub(crate) fn column_label(
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    index: usize,
) -> Option<(&'static str, RectPx)> {
    let (key, rect) = *layout.labels.get(index)?;
    if !view.team_header && key == "GUI:Team" {
        return None;
    }
    let key = match (view.campaign, key) {
        (true, "GUI:Side") => "VERA:CampaignRole",
        (true, "GUI:Color" | "GUI:StartPosition" | "GUI:Team") => return None,
        _ => key,
    };
    Some((key, rect))
}

/// Where the room's painted static `id` stands while it shows (the outer
/// `None` for any other control).
fn painted_rect(
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    id: u16,
) -> Option<Option<RectPx>> {
    if matches!(
        id,
        ROOM_LABELS
            | ROOM_HEADERS
            | ROOM_BANDS
            | ROOM_ICONS
            | ROOM_NOTES
            | ROOM_BOXES
            | ROOM_TEAM_BAR
    ) {
        return Some(Some(RectPx::default()));
    }
    let bar = |caption: u16| {
        let shown = view
            .trackbars
            .iter()
            .any(|bar| bar.id == caption && bar.visible);
        layout.team_bar.filter(|_| view.team_bar() && shown)
    };
    let header = |row: usize| view.headers[row].as_ref().filter(|_| layout.roster);
    let icon = |row: usize| view.row_icons[row].as_ref();
    let rect = match id {
        ROSTER_PAGE_LINE => (view.pages.len() > 1).then_some(layout.roster_page_label),
        TEAM_BAR_BOX => layout.team_bar.filter(|_| view.team_bar()),
        TEAMS_CAPTION => bar(TEAM_COUNT).map(|_| layout.teams_label),
        ROOM_SIZE_CAPTION => bar(ROOM_SIZE).map(|_| layout.room_size_label),
        COOPERATIVE_TEXT => view
            .cooperative_description
            .as_ref()
            .map(|_| layout.cooperative_description),
        id if (COLUMN_LABEL..COLUMN_LABEL + 2 * LABELS.len() as u16).contains(&id) => {
            column_label(view, layout, usize::from(id - COLUMN_LABEL)).map(|(_, rect)| rect)
        }
        id if (DIFFICULTY_LABEL..DIFFICULTY_LABEL + DIFFICULTY_WORDS.len() as u16)
            .contains(&id) =>
        {
            let (_, x) = DIFFICULTY_WORDS[usize::from(id - DIFFICULTY_LABEL)];
            (view.cooperative || view.campaign)
                .then(|| layout.page.scale.scale_rect(RectPx::new(x, 190, 75, 20)))
        }
        id => {
            if let Some(row) = page_row(id, HEADER_CAPTION) {
                header(row).map(|_| layout.combos[row][0])
            } else if let Some(row) = page_row(id, JOIN_CAPTION) {
                header(row)
                    .filter(|header| header.join.is_some())
                    .map(|_| layout.header_controls[row].0)
            } else if let Some(row) = page_row(id, OWN_ROW) {
                (layout.roster && icon(row).is_some_and(|icon| icon.local))
                    .then_some(layout.roster_rows[row])
            } else if let Some(row) = page_row(id, BADGE) {
                layout.badge_rect(view, SetupColumn::Player.control(row))
            } else if let Some(row) = page_row(id, ROW_FLAG) {
                (!layout.roster && icon(row).is_some_and(|icon| icon.country.is_some()))
                    .then_some(layout.flags[row])
            } else if let Some(row) = page_row(id, ROW_STATUS) {
                (!layout.roster && icon(row).is_some()).then_some(layout.acceptance[row])
            } else {
                return None;
            }
        }
    };
    Some(rect)
}

/// The option trackbars' captions, statics that take no pointer.
fn option_captions(ids: impl Iterator<Item = u16>) -> impl Iterator<Item = NodeSpec> {
    ids.zip(OPTION_LABELS.map(|(_, key)| key)).map(|(id, key)| {
        node(NodeKind::Text, id)
            .label(key)
            .with(Prop::Align, Value::Int(0))
    })
}

/// Checkboxes and trackbars, Difficulty's without its value plaque.
fn option_spec(
    checkboxes: impl IntoIterator<Item = u16>,
    trackbars: impl IntoIterator<Item = u16>,
) -> impl Iterator<Item = NodeSpec> {
    let label = |id| {
        let native = CHECKBOXES.iter().zip(CHECKBOX_LABELS);
        let session = SESSION_CHECKBOXES.iter().zip(SESSION_CHECKBOX_LABELS);
        let room = ROOM_CHECKBOXES.iter().zip(ROOM_CHECKBOX_LABELS);
        native
            .chain(session)
            .chain(room)
            .find(|(known, _)| **known == id)
            .map_or("", |(_, key)| key)
    };
    let checkboxes = checkboxes
        .into_iter()
        .map(move |id| node(NodeKind::Checkbox, id).label(label(id)));
    let trackbars = trackbars.into_iter().map(|id| {
        node(NodeKind::Trackbar, id).with(Prop::ShowValue, Value::Bool(id != DIFFICULTY))
    });
    checkboxes.chain(trackbars)
}

/// VERA's Game Options box: both pages' options, its page button and its
/// OK; [`on_game_options_page`] shows one page's.
fn game_options_spec() -> NodeSpec {
    use crate::ui::skirmish_shell::game_options as labels;
    let checkboxes = CHECKBOXES
        .into_iter()
        .chain(SESSION_CHECKBOXES)
        .chain(ROOM_CHECKBOXES);
    let trackbars = OPTION_TRACKBARS
        .into_iter()
        .chain([TECH_LEVEL, SUPERWEAPON_LIMIT]);
    let caption = |id, key| {
        node(NodeKind::Text, id)
            .label(key)
            .with(Prop::Align, Value::Int(0))
    };
    NodeSpec::new(NodeKind::Dialog)
        .children(option_spec(checkboxes, trackbars))
        // Its caption names the page it turns to (`props`).
        .child(node(NodeKind::Button, GAME_OPTIONS_PAGE))
        // VERA's box closes on Enter or Escape as on OK.
        .child(
            node(NodeKind::Button, GAME_OPTIONS_OK)
                .label("GUI:OK")
                .with(Prop::Default, Value::Bool(true))
                .with(Prop::Cancel, Value::Bool(true)),
        )
        .children(option_captions(OPTION_CAPTIONS.into_iter()))
        .child(caption(TECH_LEVEL_CAPTION, labels::TECH_LEVEL_LABEL))
        .child(caption(
            SUPERWEAPON_LIMIT_CAPTION,
            labels::SUPERWEAPON_LIMIT_LABEL,
        ))
}

/// VERA's missing-map offer, in paint order: its text, Download (greyed
/// in [0xAC1CB4] while the map comes), Leave and the download bar.
fn map_offer_spec() -> NodeSpec {
    let grey = Value::Rgba(vera20k_gui::Rgba(0x9F, 0, 0, 0xFF));
    NodeSpec::new(NodeKind::Dialog)
        .child(node(NodeKind::Text, MAP_OFFER_BODY).with(Prop::Align, Value::Int(0)))
        .child(node(NodeKind::Button, MAP_OFFER_DOWNLOAD).with(Prop::DisabledColor, grey))
        .child(node(NodeKind::Button, MAP_OFFER_LEAVE))
        .child(node(NodeKind::Text, MAP_OFFER_PROGRESS))
}

/// Control `id`'s rect while it shows: a combo for a shown row (the team
/// bar's lists too), a trackbar the view shows, a header's Join while it has
/// one, Previous and Next on a roster of pages while they lead to
/// one, and VERA's roster's scrollbar.
fn control_rect(view: &SetupView, layout: &MultiplayerSetupLayout, id: u16) -> Option<RectPx> {
    if let Some(rect) = painted_rect(view, layout, id) {
        return rect;
    }
    let shows = match id {
        ROSTER_SCROLL | ROSTER_AREA => return layout.roster.then_some(layout.roster_area),
        id if (GROUP_BOX..GROUP_BOX + MODERN_SETUP_ROWS as u16).contains(&id) => {
            return layout.group_boxes[usize::from(id - GROUP_BOX)];
        }
        MAP_PREVIEW => return Some(layout.map_preview),
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
        controller.ensure_active(page);
    }
    for (dialog, open) in [
        (GAME_OPTIONS_BOX, ui.game_options),
        (MAP_OFFER_BOX, ui.map_offer.is_some()),
        (NOTICE_BOX, ui.notice.is_some()),
    ] {
        match (open, controller.root_in(dialog)) {
            (true, None) => controller.push_modal(dialog),
            (false, Some(_)) => controller.close(dialog),
            _ => {}
        }
    }
    let scale = layout.page.scale;
    let room = view.dialog;
    let spec_page = move || page_spec(room);
    let notice = || {
        // The native 5D3490 box's OK takes Enter and Escape.
        let ok = crate::ui::shell::modal::control::OK;
        NodeSpec::new(NodeKind::Dialog).child(
            node(NodeKind::Button, ok)
                .with(Prop::Default, Value::Bool(true))
                .with(Prop::Cancel, Value::Bool(true)),
        )
    };
    let roots: [(DialogResourceId, &dyn Fn() -> NodeSpec); 4] = [
        (page, &spec_page),
        (GAME_OPTIONS_BOX, &game_options_spec),
        (MAP_OFFER_BOX, &map_offer_spec),
        (NOTICE_BOX, &notice),
    ];
    for (dialog, spec) in roots {
        let Some(root) = controller.root_in(dialog) else {
            continue;
        };
        controller.set_scale_in(dialog, scale);
        let gui = controller.gui_mut();
        let built = page_nodes::ensure_built(gui, root, spec);
        // The Game Options box shows its page's controls.
        let box_page = (dialog == GAME_OPTIONS_BOX).then_some(ui.game_options_page);
        page_nodes::sync_rects(gui, root, |id| {
            let rect = control_rect(view, layout, id)
                .filter(|_| box_page.is_none_or(|page| on_game_options_page(view, page, id)))?;
            // VERA's chat edit answers with its own rect: narrowed beside a
            // touch platform's send key, the native place otherwise.
            if dialog == page && id == CHAT {
                return Some(layout.chat_text(view.dialog));
            }
            Some(rect)
        });
        let nodes: Vec<(NodeId, u16)> = gui
            .tree()
            .children(root)
            .map(|children| children.to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|node| Some((node, u16::try_from(gui.id_of(node)?).ok()?)))
            .collect();
        // The painted statics and their panels take no session state.
        for (node, id) in nodes.into_iter().filter(|(_, id)| !painted(*id)) {
            for (prop, value) in props(ui, view, layout, id) {
                let _ = gui.set(node, prop, value);
            }
            // The transcript's rows; its list keeps the view, following
            // new lines at the bottom.
            if dialog == page && id == CHAT_LOG {
                let _ = gui.log_rows(node, ui.chat.lines.len(), 0);
            }
        }
        if dialog == page {
            if built || ui.room_nodes.get().root != Some(root) {
                ui.room_nodes.set(RoomNodes::find(gui, root));
            }
            // gamemd's rooms show a row's flag by its country's PCX, each
            // row's flag first of its two pictures.
            if let Some(icons) = ui.room_nodes.get().icons.filter(|_| !room.modern) {
                let rows = view.row_icons.iter().take(room.rows_per_page());
                for (row, icon) in rows.enumerate() {
                    let flag = gui
                        .tree()
                        .children(icons)
                        .ok()
                        .and_then(|c| c.get(2 * row).copied());
                    if let (Some(flag), Some(country)) =
                        (flag, icon.as_ref().and_then(|icon| icon.country.as_deref()))
                    {
                        page_nodes::set_node_text(gui, flag, Prop::Art, country);
                    }
                }
            }
        }
        // The chat edit holds the room's focus from the start, its caret
        // blinking on from then.
        if built && dialog == page {
            controller.focus_control(page, Some(CHAT));
        }
    }
    let root = controller.root_in(page)?;
    // A list closes once its combo no longer takes it, or its row moved
    // away, so a pick never changes whoever took its place.
    if controller.top_id() == Some(page)
        && let Some((id, _)) = open_list(controller.gui())
        && !view.combo(id).is_some_and(|combo| {
            !view.locked
                && combo.visible
                && combo.enabled
                && ui.popup_row.is_none_or(|row| row == combo.row)
        })
    {
        controller.gui_mut().close_popup();
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
            None if matches!(id, CAMPAIGN_SERIES | CAMPAIGN_STAGE) => {
                SetupColumn::Country.visible_rows()
            }
            None => 0,
        };
        props.extend([
            (Prop::RowCount, Value::Int(combo.choices.len() as i32)),
            (
                Prop::Selected,
                Value::Int(combo.selected.map_or(-1, |row| row as i32)),
            ),
            (Prop::VisibleRows, Value::Int(cap as i32)),
        ]);
        // A colour row's swatch, by its colour index.
        let swatches = combo.choices.iter().map(|choice| {
            choice
                .swatch
                .map_or_else(String::new, |index| index.to_string())
        });
        props.push((Prop::Swatches, Value::Items(swatches.collect())));
        // VERA's roster draws no faces: a press anywhere on a cell opens its
        // list while it has something else to choose; any other press goes
        // through to its group's box. The team bar's and a campaign's lists
        // have faces.
        let face = matches!(id, TEAM_MENU | SEAT_MENU | CAMPAIGN_SERIES | CAMPAIGN_STAGE);
        if layout.roster && !face {
            let opens = view.enabled(id) && roster_cell_opens(combo);
            props.push((Prop::PressThrough, Value::Bool(!opens)));
            // The host carries a draggable row from a press that opens a
            // cell's list, once it leaves the press.
            let drags = opens && view.drag_rows.iter().any(|drag| drag.row == combo.row);
            props.extend([
                (Prop::Draggable, Value::Bool(drags)),
                (Prop::DragThreshold, Value::Int(ROW_DRAG_START)),
            ]);
        }
        if layout.roster
            && let Some(list) = layout.roster_list(id, combo, cap)
        {
            props.extend([
                (Prop::RowHeight, Value::Int(list.row_height)),
                (Prop::PopupRect, Value::Rect(list.rect)),
            ]);
        }
    } else if let Some(index) = CHECKBOXES.iter().position(|check| *check == id) {
        props.push((Prop::Checked, Value::Bool(view.checks[index])));
    } else if let Some(index) = SESSION_CHECKBOXES.iter().position(|check| *check == id) {
        props.push((Prop::Checked, Value::Bool(view.session_checks[index])));
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
        // The rows and the top: `sync`'s log of the transcript.
        props.extend([
            (Prop::Selected, Value::Int(-1)),
            (Prop::RowHeight, Value::Int(row_h)),
        ]);
    } else if id == GAME_OPTIONS_PAGE {
        let label = ui.game_options_page.page_button_label();
        props.push((Prop::Label, Value::Text(label.into())));
    } else if id == ROSTER_SCROLL {
        let (count, max_top) = view.roster_scroll;
        let bar = layout
            .roster_scroll(view)
            .and_then(|roster| roster.scrollbar);
        // The node owns the place; `first_row` is its top read back within
        // the range the roster has now (`view`), written back so the bar
        // and the wheel work from it.
        props.extend([
            (Prop::RowCount, Value::Int(count as i32)),
            (Prop::VisibleRows, Value::Int((count - max_top) as i32)),
            (Prop::Top, Value::Int(view.first_row as i32)),
            (Prop::WheelStep, Value::Int(1)),
            (Prop::HitRect, Value::Rect(bar.unwrap_or_default())),
        ]);
    }
    // The map offer: Enter downloads unless it already does, Escape leaves;
    // keys meant for the chat just before it opened answer neither.
    if let Some(offer) = ui.map_offer.as_ref() {
        let keys = offer.takes_keys(Instant::now());
        match id {
            MAP_OFFER_DOWNLOAD => {
                props.push((Prop::Default, Value::Bool(keys && !offer.downloading)))
            }
            MAP_OFFER_LEAVE => props.push((Prop::Cancel, Value::Bool(keys))),
            _ => {}
        }
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
    let before = open_list(controller.gui());
    // A press on the chat edit keeps its focus, its blink running on.
    controller.on_pointer_down(x, y);
    if before.is_none()
        && let Some((id, _)) = open_list(controller.gui())
        && let Some(combo) = view.combo(id)
        && layout.roster
    {
        ui.popup_row = Some(combo.row);
    }
}

/// The host's drag of a roster row on the room's page `page`, as its
/// widgets hold it: the row whose cell was pressed, the pointer, and while
/// it is carried the box under it that takes the row ([`SetupView::drop_group`]).
pub(crate) fn row_drag(controller: &DialogController, view: &SetupView) -> Option<RowDrag> {
    let page = view.page().dialog;
    if controller.top_id() != Some(page) {
        return None;
    }
    let gui = controller.gui();
    let drag = gui.drag()?;
    let row = view.combo(u16::try_from(gui.id_of(drag.node)?).ok()?)?.row;
    let target = drag
        .target
        .filter(|_| drag.carrying)
        .and_then(|node| gui.id_of(node))
        .and_then(|id| usize::try_from(id - i32::from(GROUP_BOX)).ok())
        .filter(|header| view.drop_group(row, *header).is_some());
    Some(RowDrag {
        row,
        at: drag.at,
        carrying: drag.carrying,
        target,
    })
}

/// The control of the top dialog `root` under the pointer: the one whose
/// status help shows (a disabled one too, and under an open list), or with
/// `press`, the one a press works.
/// The map preview and the roster's area are no controls: they only tell
/// the host where the pointer is.
pub(crate) fn control_at(gui: &Gui, root: NodeId, x: i32, y: i32, press: bool) -> Option<u16> {
    u16::try_from(gui.id_of(gui.hit_at(root, x, y, press)?)?)
        .ok()
        .filter(|id| !matches!(*id, MAP_PREVIEW | ROSTER_AREA))
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

/// `target`: the control at the pointer a press works on the top dialog,
/// none while the host carries a row.
pub(crate) fn look(
    controller: &DialogController,
    view: &SetupView,
    target: Option<u16>,
) -> RoomLook {
    let gui = controller.gui();
    let page = view.page().dialog;
    let mut look = RoomLook {
        notice_ok: controller.pressed_node_in(NOTICE_BOX).is_some(),
        target,
        ..RoomLook::default()
    };
    for dialog in [page, GAME_OPTIONS_BOX, MAP_OFFER_BOX] {
        let held = controller
            .pressed_node_in(dialog)
            .filter(|node| controller.hovered_node_in(dialog) == Some(*node));
        look.pressed.extend(
            held.and_then(|node| gui.id_of(node))
                .and_then(|id| u16::try_from(id).ok()),
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

/// What the session is asked to do for the room's widget event `event`,
/// with the Game Options box on `page`.
pub(crate) fn action(view: &SetupView, page: RoomBox, event: PageEvent) -> Option<SetupAction> {
    Some(match event {
        PageEvent::Activated(control) => match control {
            START => SetupAction::Start,
            ACCEPT => SetupAction::Accept,
            BACK => SetupAction::Back,
            CHOOSE_MAP => SetupAction::ChooseMap,
            EMOTE => SetupAction::Emote,
            PAGE => SetupAction::Page,
            PREVIOUS_ROWS => SetupAction::RosterPage { forward: false },
            NEXT_ROWS => SetupAction::RosterPage { forward: true },
            GAME_OPTIONS => SetupAction::GameOptions(Some(RoomBox::GameOptions)),
            GAME_OPTIONS_OK => SetupAction::GameOptions(None),
            GAME_OPTIONS_PAGE => SetupAction::GameOptions(Some(page.other_page())),
            LOBBY => SetupAction::Lobby,
            MAP_OFFER_DOWNLOAD => SetupAction::MapOffer { download: true },
            MAP_OFFER_LEAVE => SetupAction::MapOffer { download: false },
            control if team_join_row(control).is_some() => SetupAction::Join { control },
            _ => return None,
        },
        PageEvent::Toggled(control, _) => SetupAction::Toggle { control },
        PageEvent::Picked(control, row) => SetupAction::Select {
            control,
            value: view
                .combo(control)?
                .choices
                .get(usize::try_from(row).ok()?)?
                .value,
        },
        PageEvent::Slid {
            id,
            value,
            final_: false,
        } => SetupAction::Trackbar { control: id, value },
        _ => return None,
    })
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

    /// A touch room takes the send key's width out of its chat edit; the
    /// native room keeps the edit and shows no key.
    #[test]
    fn touch_send_narrows_the_room_chat_edit_for_its_key() {
        let lan = dialog(true, SetupFlavor::Lan, false);
        let native = MultiplayerSetupLayout::new(lan, 800, 600, false).with_touch_send(false);
        let touch = native.clone().with_touch_send(true);
        let native_chat = native.chat_text(lan);
        let touch_chat = touch.chat_text(lan);
        let send = touch.chat_send(lan).unwrap();
        assert!(native.chat_send(lan).is_none());
        assert_eq!(native_chat, native.chat);
        assert!(touch_chat.w < native_chat.w, "the edit leaves the key room");
        assert!(
            send.x >= touch_chat.x + touch_chat.w,
            "the key follows the edit"
        );
    }

    #[test]
    fn online_room_keeps_the_emote_button_beside_its_chat_edit() {
        let online = dialog(false, SetupFlavor::Wol, true);
        let layout = MultiplayerSetupLayout::new(online, 800, 600, false).with_touch_send(true);
        assert!(layout.emote.is_some());
        assert!(layout.chat_send(online).is_none());
        assert_eq!(layout.chat_text(online), layout.chat);
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
            campaign: false,
            first_row: 0,
            row_count: 8,
            pages: vec![0],
            places: Vec::new(),
            page_len: 8,
            roster_scroll: (0, 0),
            dialog: dialog(true, SetupFlavor::Wol, true),
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
            lobby_enabled: false,
            drag_rows: Vec::new(),
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
        let ui = MultiplayerSetupState::default();
        let mut gui = Gui::new(8);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        gui.push_dialog(root, vera20k_gui::DialogKind::Page)
            .unwrap();
        let roster = gui.create(NodeKind::Text, Some(root)).unwrap();
        gui.set(roster, Prop::Rect, Value::Rect(area)).unwrap();
        gui.set(roster, Prop::Tooltip, Value::Text(String::new()))
            .unwrap();
        let bar_node = gui.create(NodeKind::List, Some(root)).unwrap();
        gui.set(bar_node, Prop::Rect, Value::Rect(area)).unwrap();
        for (prop, value) in props(&ui, &view, &layout, ROSTER_SCROLL) {
            gui.set(bar_node, prop, value).unwrap();
        }
        let (roster, scroll_node) = (Some(roster), Some(bar_node));
        let top = |gui: &Gui| gui.list_view(bar_node).unwrap().top;
        ui.wheel(
            &mut gui,
            roster,
            scroll_node,
            &view,
            &layout,
            area.x,
            area.y,
            -1.0,
            0,
        );
        assert_eq!(top(&gui), 3);
        ui.wheel(
            &mut gui,
            roster,
            scroll_node,
            &view,
            &layout,
            area.x,
            area.y,
            -100.0,
            0,
        );
        assert_eq!(top(&gui), scroll.max_top);
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
        // closes it, so a pick cannot change them.
        let open = SetupColumn::Country.control(3);
        let combo = view
            .combos
            .iter_mut()
            .find(|combo| combo.id == open)
            .unwrap();
        combo.row = 4;
        sync(&mut controller, &ui, &view, &layout, true);
        assert!(open_list(controller.gui()).is_none());

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
        let mut ui = MultiplayerSetupState {
            game_options: true,
            ..MultiplayerSetupState::default()
        };
        let mut controller = widgets(&ui, &view, &popup_layout);
        let center = |rect: RectPx| (rect.x + rect.w / 2, rect.y + rect.h / 2);
        assert_eq!(
            hit(&controller, center(popup_layout.checkboxes[0]), false),
            Some(CHECKBOXES[0])
        );
        assert_eq!(hit(&controller, center(popup_layout.chat), false), None);
        // Each page takes presses on its own options only.
        let more = center(popup_layout.room_checkboxes[0]);
        assert_eq!(hit(&controller, more, false), None);
        ui.show_game_options(Some(RoomBox::MoreOptions));
        sync(&mut controller, &ui, &view, &popup_layout, true);
        assert_eq!(hit(&controller, more, false), Some(ROOM_CHECKBOXES[0]));
        assert_ne!(
            hit(&controller, center(popup_layout.checkboxes[0]), false),
            Some(CHECKBOXES[0])
        );
        // The team bar under the roster holds the Teams count, and the
        // host's Room size, Quick Team Setup and Bulk Seat Actions; a guest
        // has neither list.
        ui.show_game_options(None);
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
            sync(&mut controller, &ui, &view, &popup_layout, true);
            assert_eq!(hit(&controller, center(rect), false), Some(id));
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
        // VERA's missing-map offer takes every press until it is answered.
        ui.map_offer = Some(MapOffer {
            name: String::new(),
            message: String::new(),
            button_key: "VERA:Download",
            downloading: false,
            opened: Instant::now(),
        });
        sync(&mut controller, &ui, &view, &popup_layout, true);
        assert_eq!(hit(&controller, center(popup_layout.chat), false), None);
        assert_eq!(
            hit(&controller, center(popup_layout.map_offer.button(0)), false),
            Some(MAP_OFFER_DOWNLOAD)
        );
        ui.map_offer = None;
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
