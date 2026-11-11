//! `veraui.ini`: the match interface as a layout a mod defines.
//!
//! A game-folder file, loose or inside a mod MIX, names a layout for each
//! side. A layout starts as that side's original interface, and each part
//! section of the file changes the numbers it writes.
//! `docs/interface-layout.md` is the modder's reference and the promise this
//! reader keeps: a key it documents keeps its meaning in later versions.
//!
//! The file is the player's own display. It never reaches the simulation and
//! is not one of the sources online players' rules are compared by.
//!
//! A bad file never stops the game: every refusal is a [`LayoutProblem`] and
//! the original value stays. A VERA file, not a gamemd one, so section and
//! key names match without regard to capitals and a missing last line break
//! loses nothing; values still go through the native INI readers.
//!
//! ## Dependency rules
//! - Part of ui/ — reads the rules/ INI store; no app/ or render/.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::ui::added_parts::{AddedKind, AddedPart, Holder, TextValue};
use crate::ui::field_layout::{FieldLayout, FieldPart, Hold, PartBox};
use crate::ui::sidebar::{Edge, SidebarChromeLayoutSpec, SidebarPart};

/// The file's name in the game folder or a mod MIX.
pub const FILE_NAME: &str = "VERAUI.INI";

/// The newest `Version=` this build reads.
const FORMAT_VERSION: i32 = 1;

/// `[Layouts]`: one `Side=Layout` line per `[Sides]` name, and `Default=`
/// for every side without a line.
const LAYOUTS_SECTION: &str = "Layouts";
const DEFAULT_SIDE: &str = "Default";

/// ReadString capacities: one name, and a list of part names.
const NAME_CAPACITY: usize = 0x80;
const LIST_CAPACITY: usize = 0x800;

/// One number of the sidebar's layout table a part key writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Number {
    SideWidth,
    SideHeight,
    CreditsX,
    CreditsY,
    MoneyX,
    MoneyY,
    TopX,
    TopY,
    DiplomacyX,
    DiplomacyY,
    OptionsX,
    OptionsY,
    RadarX,
    RadarY,
    RadarMapX,
    RadarMapY,
    RadarMapWidth,
    RadarMapHeight,
    MiddleX,
    MiddleY,
    RepairX,
    RepairY,
    SellX,
    SellY,
    TabsX,
    TabsY,
    TabsStep,
    CameosX,
    CameosY,
    CameosColumns,
    CameosCellX,
    CameosCellY,
    CameosColumnWidth,
    CameosRowHeight,
    CameosFooter,
    CameosScrollX,
    CameosScrollY,
    CameosScrollStep,
    PowerX,
    PowerY,
}

impl Number {
    fn slot(self, spec: &mut SidebarChromeLayoutSpec) -> &mut f32 {
        match self {
            Self::SideWidth => &mut spec.sidebar_width,
            Self::SideHeight => &mut spec.side_height,
            Self::CreditsX => &mut spec.credits_x,
            Self::CreditsY => &mut spec.credits_y,
            Self::MoneyX => &mut spec.money_x,
            Self::MoneyY => &mut spec.money_y,
            Self::TopX => &mut spec.top_x,
            Self::TopY => &mut spec.top_y,
            Self::DiplomacyX => &mut spec.top_buttons[0][0],
            Self::DiplomacyY => &mut spec.top_buttons[0][1],
            Self::OptionsX => &mut spec.top_buttons[1][0],
            Self::OptionsY => &mut spec.top_buttons[1][1],
            Self::RadarX => &mut spec.radar_housing_x,
            Self::RadarY => &mut spec.radar_housing_y,
            Self::RadarMapX => &mut spec.radar_x,
            Self::RadarMapY => &mut spec.radar_y,
            Self::RadarMapWidth => &mut spec.radar_width,
            Self::RadarMapHeight => &mut spec.radar_height,
            Self::MiddleX => &mut spec.side1_x,
            Self::MiddleY => &mut spec.side1_y,
            Self::RepairX => &mut spec.repair_x,
            Self::RepairY => &mut spec.repair_y,
            Self::SellX => &mut spec.sell_x,
            Self::SellY => &mut spec.sell_y,
            Self::TabsX => &mut spec.tab_x,
            Self::TabsY => &mut spec.tab_y,
            Self::TabsStep => &mut spec.tab_pitch,
            Self::CameosX => &mut spec.strip_x,
            Self::CameosY => &mut spec.strip_y,
            Self::CameosColumns => &mut spec.cameo_columns,
            Self::CameosCellX => &mut spec.cameo_inset_x,
            Self::CameosCellY => &mut spec.cameo_inset_y,
            Self::CameosColumnWidth => &mut spec.cameo_column_pitch,
            Self::CameosRowHeight => &mut spec.cameo_row_height,
            Self::CameosFooter => &mut spec.strip_footer,
            Self::CameosScrollX => &mut spec.scroll_x,
            Self::CameosScrollY => &mut spec.scroll_y,
            Self::CameosScrollStep => &mut spec.scroll_pitch,
            Self::PowerX => &mut spec.power_bar_x,
            Self::PowerY => &mut spec.power_bar_y,
        }
    }
}

/// A key that sets one number, and the range it is clamped to.
struct NumberKey {
    key: &'static str,
    number: Number,
    min: i32,
    max: i32,
}

/// A place or a step, in sidebar pixels: further than any screen reaches.
const fn place(key: &'static str, number: Number) -> NumberKey {
    NumberKey {
        key,
        number,
        min: -8192,
        max: 8192,
    }
}

/// A size that something is divided by or drawn into: never below `min`.
const fn size(key: &'static str, number: Number, min: i32) -> NumberKey {
    NumberKey {
        key,
        number,
        min,
        max: 8192,
    }
}

/// One part of the original interface: what the file may say about it now
/// (`numbers`), and what it has to leave as it is until a later version
/// (`fixed`, each key with the only value accepted; `None` accepts no value).
struct StockPart {
    name: &'static str,
    numbers: &'static [NumberKey],
    fixed: &'static [(&'static str, Option<&'static str>)],
}

/// The original screen's top-level parts, in `Parts=` order.
const SCREEN_PARTS: &str = "Field,Side,Bar";

/// Keys every part has. `Art=` has no accepted value yet: the pictures a
/// part draws still come from the side's own files.
const ART_KEY: (&str, Option<&str>) = ("Art", None);

/// A part whose place the engine cannot move yet.
const UNMOVED: [(&str, Option<&str>); 2] = [("X", Some("0")), ("Y", Some("0"))];

const STOCK_PARTS: &[StockPart] = &[
    StockPart {
        name: "Field",
        numbers: &[],
        fixed: &[
            ("Type", Some("Battlefield")),
            ("Hold", Some("TopLeft")),
            ("Width", Some("Rest")),
            ("Height", Some("Rest")),
            ("Parts", None),
            UNMOVED[0],
            UNMOVED[1],
            ART_KEY,
        ],
    },
    StockPart {
        name: "Side",
        // The battlefield and the bar take what the sidebar leaves. Its
        // width counts on the left or the right edge, its height on the top
        // or the bottom; `Rest` is the other one, the screen's own.
        numbers: &[
            size("Width", Number::SideWidth, 16),
            size("Height", Number::SideHeight, 16),
        ],
        fixed: &[
            ("Type", Some("Panel")),
            ("Width", Some("Rest")),
            ("Height", Some("Rest")),
            UNMOVED[0],
            UNMOVED[1],
            ART_KEY,
        ],
    },
    StockPart {
        name: "Bar",
        numbers: &[],
        fixed: &[
            ("Type", Some("Panel")),
            ("Width", Some("Rest")),
            ("Height", Some("32")),
            ("Parts", None),
            UNMOVED[0],
            UNMOVED[1],
            ART_KEY,
        ],
    },
    StockPart {
        name: "Credits",
        numbers: &[place("X", Number::CreditsX), place("Y", Number::CreditsY)],
        fixed: &[
            ("Type", Some("Panel")),
            ("Hold", Some("TopLeft")),
            ("Width", None),
            ("Height", None),
            ART_KEY,
        ],
    },
    StockPart {
        name: "Money",
        numbers: &[place("X", Number::MoneyX), place("Y", Number::MoneyY)],
        fixed: &[
            ("Type", Some("Text")),
            ("Hold", Some("Top")),
            ("Width", None),
            ("Height", None),
            ("Parts", None),
            // The value it shows; names come with the step that adds text parts.
            ("Value", None),
            ART_KEY,
        ],
    },
    StockPart {
        name: "Top",
        numbers: &[place("X", Number::TopX), place("Y", Number::TopY)],
        fixed: &[
            ("Type", Some("Panel")),
            ("Hold", Some("TopLeft")),
            ("Width", None),
            ("Height", None),
            ART_KEY,
        ],
    },
    StockPart {
        name: "Diplomacy",
        numbers: &[
            place("X", Number::DiplomacyX),
            place("Y", Number::DiplomacyY),
        ],
        fixed: &BUTTON_FIXED,
    },
    StockPart {
        name: "Options",
        numbers: &[place("X", Number::OptionsX), place("Y", Number::OptionsY)],
        fixed: &BUTTON_FIXED,
    },
    StockPart {
        name: "Radar",
        numbers: &[
            place("X", Number::RadarX),
            place("Y", Number::RadarY),
            place("MapX", Number::RadarMapX),
            place("MapY", Number::RadarMapY),
            size("MapWidth", Number::RadarMapWidth, 1),
            size("MapHeight", Number::RadarMapHeight, 1),
        ],
        fixed: &[
            ("Type", Some("Radar")),
            ("Hold", Some("TopLeft")),
            ("Width", None),
            ("Height", None),
            ("Parts", None),
            ART_KEY,
        ],
    },
    StockPart {
        name: "Middle",
        numbers: &[place("X", Number::MiddleX), place("Y", Number::MiddleY)],
        fixed: &[
            ("Type", Some("Panel")),
            ("Hold", Some("TopLeft")),
            ("Width", None),
            ("Height", None),
            ART_KEY,
        ],
    },
    StockPart {
        name: "Repair",
        numbers: &[place("X", Number::RepairX), place("Y", Number::RepairY)],
        fixed: &BUTTON_FIXED,
    },
    StockPart {
        name: "Sell",
        numbers: &[place("X", Number::SellX), place("Y", Number::SellY)],
        fixed: &BUTTON_FIXED,
    },
    StockPart {
        name: "Tabs",
        numbers: &[
            place("X", Number::TabsX),
            place("Y", Number::TabsY),
            place("Step", Number::TabsStep),
        ],
        fixed: &[
            ("Type", Some("Tabs")),
            ("Hold", Some("TopLeft")),
            ("Width", None),
            ("Height", None),
            ("Parts", None),
            ART_KEY,
        ],
    },
    StockPart {
        name: "Cameos",
        numbers: &[
            place("X", Number::CameosX),
            place("Y", Number::CameosY),
            // Each visible cameo is a gadget, so a row stays a row.
            NumberKey {
                key: "Columns",
                number: Number::CameosColumns,
                min: 1,
                max: 8,
            },
            place("CellX", Number::CameosCellX),
            place("CellY", Number::CameosCellY),
            place("ColumnWidth", Number::CameosColumnWidth),
            // The row height divides the strip into rows.
            size("RowHeight", Number::CameosRowHeight, 16),
            size("Footer", Number::CameosFooter, 0),
            place("ScrollX", Number::CameosScrollX),
            place("ScrollY", Number::CameosScrollY),
            place("ScrollStep", Number::CameosScrollStep),
        ],
        fixed: &[
            ("Type", Some("CameoGrid")),
            ("Hold", Some("TopLeft")),
            ("Width", None),
            ("Height", Some("Rest")),
            ("Parts", None),
            ("CellWidth", Some("60")),
            ("CellHeight", Some("48")),
            ART_KEY,
        ],
    },
    StockPart {
        name: "Power",
        numbers: &[place("X", Number::PowerX), place("Y", Number::PowerY)],
        fixed: &[
            ("Type", Some("PowerBar")),
            ("Hold", Some("TopLeft")),
            ("Width", None),
            ("Height", None),
            ("Parts", None),
            ART_KEY,
        ],
    },
];

/// A button's keys. `Command=` is the command it gives; its names come with
/// the step that lets a layout add buttons.
const BUTTON_FIXED: [(&str, Option<&str>); 7] = [
    ("Type", Some("Button")),
    ("Hold", Some("TopLeft")),
    ("Width", None),
    ("Height", None),
    ("Parts", None),
    ("Command", None),
    ART_KEY,
];

/// Why a line of the file was not used as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutProblemKind {
    /// `[Layouts]` names a layout the file has no section for.
    MissingLayout,
    /// The layout's `Version=` is newer than this build reads.
    NewerVersion,
    /// A section for a part the original interface does not have.
    UnknownPart,
    UnknownKey,
    NotANumber,
    /// The number was outside the key's range and became `used`.
    Clamped {
        used: i32,
    },
    /// A yes-or-no key with another word.
    NotYesOrNo,
    /// A documented key this version cannot change yet.
    NotYet,
    /// A button's `Command=` is no command the game has, or is missing.
    UnknownCommand,
    /// A text's `Value=` is no value the game shows, or is missing.
    UnknownValue,
    /// An added part's section that no `Parts=` list names.
    NotListed,
}

/// One refused line: its section, its key (empty for a whole section) and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutProblem {
    pub section: String,
    pub key: String,
    pub kind: LayoutProblemKind,
}

impl LayoutProblem {
    /// Where in the file, as the player is told: `[Section] Key`.
    pub fn place(&self) -> String {
        if self.key.is_empty() {
            format!("[{}]", self.section)
        } else {
            format!("[{}] {}", self.section, self.key)
        }
    }
}

impl std::fmt::Display for LayoutProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let place = self.place();
        match &self.kind {
            LayoutProblemKind::MissingLayout => write!(
                f,
                "{place}: the file has no section for this layout; the original interface is used"
            ),
            LayoutProblemKind::NewerVersion => write!(
                f,
                "{place}: made for a newer VERA20k (this one reads Version={FORMAT_VERSION}); the original interface is used"
            ),
            LayoutProblemKind::UnknownPart => write!(
                f,
                "{place}: not a part of the interface; the section is ignored"
            ),
            LayoutProblemKind::UnknownKey => write!(f, "{place}: unknown key, ignored"),
            LayoutProblemKind::NotANumber => {
                write!(f, "{place}: not a whole number; the original value is kept")
            }
            LayoutProblemKind::Clamped { used } => {
                write!(f, "{place}: out of range, {used} is used")
            }
            LayoutProblemKind::NotYesOrNo => {
                write!(f, "{place}: not yes or no; the original value is kept")
            }
            LayoutProblemKind::NotYet => write!(
                f,
                "{place}: cannot be changed yet; the original value is kept"
            ),
            LayoutProblemKind::UnknownCommand => write!(
                f,
                "{place}: not a command the game has; the button is left out"
            ),
            LayoutProblemKind::UnknownValue => write!(
                f,
                "{place}: not a value the game shows; the text is left out"
            ),
            LayoutProblemKind::NotListed => {
                write!(f, "{place}: the part is in no Parts= list; it is not shown")
            }
        }
    }
}

/// The word a `Parts=` list uses for "no parts"; the INI store drops a key
/// with no value.
const NO_PARTS: &str = "None";

/// One of the sidebar's panels: the sidebar itself (`None`) or a part that
/// holds parts.
type Panel = Option<SidebarPart>;

/// A panel's section name: `Side` for the sidebar itself.
fn panel_of(section_part: &str) -> Option<Panel> {
    if section_part.eq_ignore_ascii_case("Side") {
        return Some(None);
    }
    SidebarPart::STOCK_ORDER
        .into_iter()
        .find(|part| part.name().eq_ignore_ascii_case(section_part))
        .filter(|&part| stock_children(Some(part)).next().is_some())
        .map(Some)
}

/// The parts a panel holds in the original interface, in draw order.
fn stock_children(panel: Panel) -> impl Iterator<Item = SidebarPart> {
    SidebarPart::STOCK_ORDER
        .into_iter()
        .filter(move |part| part.panel() == panel)
}

/// One layout of the file: the numbers and part lists it changes on the
/// side's original.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchLayout {
    numbers: Vec<(Number, f32)>,
    /// Each panel whose `Parts=` the layout writes, with its parts in order.
    parts: Vec<(Panel, Vec<SidebarPart>)>,
    /// `[Side] Hold=` and `Float=`, `[Bar] Hold=`, and whether the screen's
    /// `Parts=` has the bar; `None` keeps the original's.
    side_edge: Option<Edge>,
    side_floats: Option<bool>,
    bar_edge: Option<Edge>,
    has_bar: Option<bool>,
    /// The pieces over the battlefield, the original's where the layout
    /// says nothing.
    field: FieldLayout,
    /// The buttons and texts the layout adds, in the order their holders
    /// list them.
    added: std::sync::Arc<[AddedPart]>,
}

impl MatchLayout {
    /// The buttons and texts this layout adds.
    pub fn added(&self) -> &std::sync::Arc<[AddedPart]> {
        &self.added
    }

    /// Write this layout's numbers and part lists over the side's original
    /// sidebar table.
    pub fn apply_to_sidebar(&self, spec: &mut SidebarChromeLayoutSpec) {
        for &(number, value) in &self.numbers {
            *number.slot(spec) = value;
        }
        spec.field = self.field;
        let places = &mut spec.places;
        places.side = self.side_edge.unwrap_or(places.side);
        places.side_floats = self.side_floats.unwrap_or(places.side_floats);
        places.bar = match self.has_bar {
            Some(false) => None,
            _ => self.bar_edge.or(places.bar).or(Some(Edge::Bottom)),
        };
        if self.parts.is_empty() {
            return;
        }
        // A panel's own list, else the original's; a part goes with its panel.
        let children = |panel: Panel| -> Vec<SidebarPart> {
            self.parts
                .iter()
                .rev()
                .find(|(listed, _)| *listed == panel)
                .map_or_else(
                    || stock_children(panel).collect(),
                    |(_, parts)| parts.clone(),
                )
        };
        let mut order = children(None)
            .into_iter()
            .flat_map(|part| std::iter::once(part).chain(children(Some(part))));
        spec.parts = std::array::from_fn(|_| order.next());
    }
}

/// A read `veraui.ini`: which layout each side uses, and what was refused.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchLayoutFile {
    /// `[Layouts]` lines in file order: the side and its layout, `None` when
    /// that layout was refused and the side keeps the original.
    sides: Vec<(String, Option<usize>)>,
    layouts: Vec<MatchLayout>,
    /// Each layout's name, as `[Layouts]` writes it.
    names: Vec<String>,
    problems: Vec<LayoutProblem>,
}

impl MatchLayoutFile {
    /// No file: every side keeps the original interface.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        // `LoadFromStraw` drops a last line that has no line break.
        let mut complete = Vec::with_capacity(bytes.len() + 1);
        complete.extend_from_slice(bytes);
        complete.push(b'\n');
        match IniFile::from_bytes(&complete) {
            Ok(ini) => Self::from_ini(&ini),
            Err(_) => Self::none(),
        }
    }

    pub fn from_ini(ini: &IniFile) -> Self {
        let mut file = Self::default();
        let Some(layouts) = find_section(ini, LAYOUTS_SECTION) else {
            return file;
        };
        // Each layout is read once, however many sides use it.
        let mut read: Vec<(&str, Option<usize>)> = Vec::new();
        for side in layouts.keys() {
            let Some(layout) = layouts.read_name(side, NAME_CAPACITY) else {
                continue;
            };
            let known = read
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(layout));
            let index = match known {
                Some(&(_, index)) => index,
                None => {
                    let index = read_layout(ini, layout, side, &mut file.problems).map(|read| {
                        file.layouts.push(read);
                        file.names.push(layout.to_owned());
                        file.layouts.len() - 1
                    });
                    read.push((layout, index));
                    index
                }
            };
            file.sides.push((side.to_owned(), index));
        }
        file
    }

    /// The mod's file with the player's own laid over it: the player's keys
    /// change exactly the keys they write, section by section, and add the
    /// sections the mod's file lacks. A side the mod gives a layout keeps
    /// that layout: the player's file changes its parts, not which layout
    /// the side uses. Either file may be missing.
    pub fn from_layers(mod_file: Option<&IniFile>, player_file: Option<&IniFile>) -> Self {
        let Some(player_file) = player_file else {
            return mod_file.map_or_else(Self::none, Self::from_ini);
        };
        let mut merged = mod_file.cloned().unwrap_or_else(IniFile::empty);
        merged.merge(player_file);
        if let Some(layouts) = mod_file.and_then(|ini| find_section(ini, LAYOUTS_SECTION)) {
            merged.merge(&IniFile::from_sections([layouts.clone()]));
        }
        Self::from_ini(&merged)
    }

    /// The layout `[Layouts]` gives this `[Sides]` name: its own line, else
    /// `Default=`. `None` keeps the original interface.
    pub fn for_side(&self, side: Option<&str>) -> Option<&MatchLayout> {
        self.layouts.get(self.layout_index(side)?)
    }

    /// The name of the layout [`Self::for_side`] gives, as the file writes
    /// it: the section prefix a change to that layout goes under.
    pub fn layout_name_for_side(&self, side: Option<&str>) -> Option<&str> {
        self.names.get(self.layout_index(side)?).map(String::as_str)
    }

    fn layout_index(&self, side: Option<&str>) -> Option<usize> {
        let line = |name: &str| {
            self.sides
                .iter()
                .find(|(listed, _)| listed.eq_ignore_ascii_case(name))
        };
        let (_, layout) = side.and_then(line).or_else(|| line(DEFAULT_SIDE))?;
        *layout
    }

    /// Whether every side keeps the original interface and nothing was
    /// refused: no file, or one that names no layout.
    pub fn is_original(&self) -> bool {
        self.sides.is_empty() && self.problems.is_empty()
    }

    /// The picture files the layouts' added buttons name (`Art=`), each once.
    pub fn art_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for part in self.layouts.iter().flat_map(|layout| layout.added.iter()) {
            if let AddedKind::Button { art: Some(art), .. } = &part.kind
                && !names.iter().any(|name| name.eq_ignore_ascii_case(art))
            {
                names.push(art.clone());
            }
        }
        names
    }

    /// Leave out every added button whose `Command=` is not `known`, with a
    /// line for each. The command lists belong to the input code, so the
    /// loader asks them once the file is read.
    pub fn check_commands(&mut self, known: impl Fn(&str) -> bool) {
        for layout in &mut self.layouts {
            let keep: Vec<bool> = layout
                .added
                .iter()
                .map(|part| match &part.kind {
                    AddedKind::Button { command, .. } => known(command),
                    AddedKind::Text { .. }
                    | AddedKind::Group { .. }
                    | AddedKind::CameoGrid { .. } => true,
                })
                .collect();
            if keep.iter().all(|&keep| keep) {
                continue;
            }
            // A group's parts name it by its place in the list, which moves
            // up by the parts left out before it.
            let mut next = 0;
            let moved: Vec<usize> = keep
                .iter()
                .map(|&keep| {
                    let place = next;
                    next += usize::from(keep);
                    place
                })
                .collect();
            let mut kept = Vec::with_capacity(next);
            for (index, part) in layout.added.iter().enumerate() {
                if keep[index] {
                    let mut part = part.clone();
                    if let Holder::Group(group) = part.holder {
                        part.holder = Holder::Group(moved[group]);
                    }
                    kept.push(part);
                } else {
                    self.problems.push(LayoutProblem {
                        section: part.section.clone(),
                        key: "Command".to_owned(),
                        kind: LayoutProblemKind::UnknownCommand,
                    });
                }
            }
            layout.added = kept.into();
        }
    }

    /// Every refused line, in file order.
    pub fn problems(&self) -> &[LayoutProblem] {
        &self.problems
    }
}

fn find_section<'a>(ini: &'a IniFile, name: &str) -> Option<&'a IniSection> {
    ini.section_names()
        .into_iter()
        .find(|section| section.eq_ignore_ascii_case(name))
        .and_then(|section| ini.section(section))
}

/// Split `Layout.Part` section names of `layout`; the layout's own section
/// gives an empty part.
fn part_of<'a>(section: &'a str, layout: &str) -> Option<&'a str> {
    let head = section.get(..layout.len())?;
    if !head.eq_ignore_ascii_case(layout) {
        return None;
    }
    match &section[layout.len()..] {
        "" => Some(""),
        rest => rest.strip_prefix('.'),
    }
}

fn layout_exists(ini: &IniFile, layout: &str) -> bool {
    ini.section_names()
        .into_iter()
        .any(|section| part_of(section, layout).is_some())
}

/// Read one layout's sections. `None` when the whole layout is refused.
fn read_layout(
    ini: &IniFile,
    layout: &str,
    side: &str,
    problems: &mut Vec<LayoutProblem>,
) -> Option<MatchLayout> {
    let mut problem = |section: &str, key: &str, kind| {
        problems.push(LayoutProblem {
            section: section.to_owned(),
            key: key.to_owned(),
            kind,
        });
    };
    if !layout_exists(ini, layout) {
        problem(LAYOUTS_SECTION, side, LayoutProblemKind::MissingLayout);
        return None;
    }
    let mut read = MatchLayout::default();
    // Names a holder's `Parts=` lists that the original does not have, each
    // with the line that lists it, and the sections that define such parts.
    let mut listed: Vec<(ListedIn, String, String, String)> = Vec::new();
    let mut defined: Vec<(String, AddedPart)> = Vec::new();
    let names = ini.section_names();
    for (position, &name) in names.iter().enumerate() {
        let Some(part) = part_of(name, layout) else {
            continue;
        };
        // A repeated section name reads as its first copy, once.
        if names[..position].contains(&name) {
            continue;
        }
        let Some(section) = ini.section(name) else {
            continue;
        };
        if part.is_empty() {
            for key in section.keys() {
                if key.eq_ignore_ascii_case("Version") {
                    match whole_number(section, key) {
                        Some(version) if version > FORMAT_VERSION => {
                            problem(name, key, LayoutProblemKind::NewerVersion);
                            return None;
                        }
                        Some(_) => {}
                        None => problem(name, key, LayoutProblemKind::NotANumber),
                    }
                } else if key.eq_ignore_ascii_case("Parts") {
                    // The battlefield and the sidebar stay; the bar may go.
                    let names: Vec<_> = section
                        .read_list(key, LIST_CAPACITY)
                        .unwrap_or_default()
                        .into_iter()
                        .map(str::trim)
                        .collect();
                    let has = |part: &str| names.iter().any(|name| name.eq_ignore_ascii_case(part));
                    if names.iter().any(|name| {
                        !SCREEN_PARTS
                            .split(',')
                            .any(|part| name.eq_ignore_ascii_case(part))
                    }) {
                        problem(name, key, LayoutProblemKind::UnknownPart);
                    }
                    if has("Field") && has("Side") {
                        read.has_bar = Some(has("Bar"));
                    } else {
                        problem(name, key, LayoutProblemKind::NotYet);
                    }
                } else {
                    problem(name, key, LayoutProblemKind::UnknownKey);
                }
            }
            continue;
        }
        if read_field_part(&mut read.field, section, part, |key, kind| {
            problem(name, key, kind)
        }) {
            continue;
        }
        let Some(stock) = STOCK_PARTS
            .iter()
            .find(|stock| stock.name.eq_ignore_ascii_case(part))
        else {
            // A section with a `Type=` this version can add is a new part.
            let mut children = Vec::new();
            match read_added_part(section, name, &mut children, |key, kind| {
                problem(name, key, kind)
            }) {
                Some(Some(added)) => {
                    listed.extend(children.into_iter().map(|child| {
                        (
                            ListedIn::Group(part.to_owned()),
                            child,
                            name.to_owned(),
                            "Parts".to_owned(),
                        )
                    }));
                    defined.push((part.to_owned(), added));
                }
                Some(None) => {}
                None => problem(name, "", LayoutProblemKind::UnknownPart),
            }
            continue;
        };
        let panel = panel_of(part);
        for key in section.keys() {
            if let Some(read_place) = read_place(&mut read, section, part, key) {
                if let Err(kind) = read_place {
                    problem(name, key, kind);
                }
            } else if part.eq_ignore_ascii_case("Field") && key.eq_ignore_ascii_case("Parts") {
                // The pieces the battlefield holds; one left out is not shown.
                let words = section.read_list(key, LIST_CAPACITY).unwrap_or_default();
                let mut shown = [false; 4];
                for word in words.into_iter().map(str::trim) {
                    match FieldPart::named(word) {
                        Some(part) => shown[part as usize] = true,
                        None if word.eq_ignore_ascii_case(NO_PARTS) || word.is_empty() => {}
                        None => listed.push((
                            ListedIn::Field,
                            word.to_owned(),
                            name.to_owned(),
                            key.to_owned(),
                        )),
                    }
                }
                read.field.shown = shown;
            } else if let (Some(panel), true) = (panel, key.eq_ignore_ascii_case("Parts")) {
                let (parts, unknown, elsewhere) = part_list(section, key, panel);
                if panel.is_none() {
                    // The sidebar itself can hold parts a layout adds.
                    listed.extend(
                        unknown
                            .into_iter()
                            .map(|word| (ListedIn::Side, word, name.to_owned(), key.to_owned())),
                    );
                } else if !unknown.is_empty() {
                    problem(name, key, LayoutProblemKind::UnknownPart);
                }
                if elsewhere {
                    problem(name, key, LayoutProblemKind::NotYet);
                }
                read.parts.push((panel, parts));
            } else if let Some(number) = stock
                .numbers
                .iter()
                .find(|number| number.key.eq_ignore_ascii_case(key))
            {
                // A size may say its word instead of a number (`Rest`).
                let word = stock.fixed.iter().any(|(fixed, accepted)| {
                    fixed.eq_ignore_ascii_case(key)
                        && accepted.is_some_and(|accepted| same_list(section, key, accepted))
                });
                if word {
                    continue;
                }
                let Some(value) = whole_number(section, key) else {
                    problem(name, key, LayoutProblemKind::NotANumber);
                    continue;
                };
                let used = value.clamp(number.min, number.max);
                if used != value {
                    problem(name, key, LayoutProblemKind::Clamped { used });
                }
                read.numbers.push((number.number, used as f32));
            } else if let Some((_, accepted)) = stock
                .fixed
                .iter()
                .find(|(fixed, _)| fixed.eq_ignore_ascii_case(key))
            {
                if !accepted.is_some_and(|accepted| same_list(section, key, accepted)) {
                    problem(name, key, LayoutProblemKind::NotYet);
                }
            } else {
                problem(name, key, LayoutProblemKind::UnknownKey);
            }
        }
    }
    // A listed name is a part once a section defines it; each part is added
    // once, where its first list puts it. The sidebar's and the
    // battlefield's lists come first, so a group has its place in the list
    // before the parts it holds.
    let mut added: Vec<AddedPart> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for in_groups in [false, true] {
        for (list, word, section, key) in &listed {
            let holder = match list {
                ListedIn::Side if !in_groups => Holder::Side,
                ListedIn::Field if !in_groups => Holder::Field,
                ListedIn::Group(group) if in_groups => {
                    // A group that no list names shows none of its parts.
                    match names
                        .iter()
                        .position(|name| name.eq_ignore_ascii_case(group))
                    {
                        Some(index) => Holder::Group(index),
                        None => continue,
                    }
                }
                _ => continue,
            };
            let Some(index) = defined
                .iter()
                .position(|(part, _)| part.eq_ignore_ascii_case(word))
            else {
                problem(section, key, LayoutProblemKind::UnknownPart);
                continue;
            };
            // A group holds buttons and texts, not other groups, and build
            // pictures draw only in the sidebar.
            let kind = &defined[index].1.kind;
            let grid = matches!(kind, AddedKind::CameoGrid { .. });
            if (in_groups && (grid || matches!(kind, AddedKind::Group { .. })))
                || (grid && holder != Holder::Side)
            {
                problem(section, key, LayoutProblemKind::NotYet);
                continue;
            }
            let (name, part) = defined.remove(index);
            names.push(name);
            added.push(AddedPart { holder, ..part });
        }
    }
    for (_, part) in defined {
        problem(&part.section, "", LayoutProblemKind::NotListed);
    }
    read.added = added.into();
    Some(read)
}

/// The keys that say where the sidebar and the bar sit: `[Side] Hold=` (any
/// edge) and `Float=`, and `[Bar] Hold=` (`Top` or `Bottom`). `None` for any
/// other key; an edge the panel cannot sit on yet is refused as not yet.
fn read_place(
    read: &mut MatchLayout,
    section: &IniSection,
    part: &str,
    key: &str,
) -> Option<Result<(), LayoutProblemKind>> {
    let side = part.eq_ignore_ascii_case("Side");
    let bar = part.eq_ignore_ascii_case("Bar");
    if side && key.eq_ignore_ascii_case("Float") {
        return Some(match section.read_bool_value(key) {
            Some(floats) => {
                read.side_floats = Some(floats);
                Ok(())
            }
            None => Err(LayoutProblemKind::NotYesOrNo),
        });
    }
    if !(side || bar) || !key.eq_ignore_ascii_case("Hold") {
        return None;
    }
    let word = section.read_name(key, NAME_CAPACITY).unwrap_or_default();
    let edge = [
        ("Left", Edge::Left),
        ("Right", Edge::Right),
        ("Top", Edge::Top),
        ("Bottom", Edge::Bottom),
    ]
    .into_iter()
    .find(|(name, _)| name.eq_ignore_ascii_case(word))
    .map(|(_, edge)| edge);
    Some(match edge {
        Some(edge) if side => {
            read.side_edge = Some(edge);
            Ok(())
        }
        Some(edge @ (Edge::Top | Edge::Bottom)) if bar => {
            read.bar_edge = Some(edge);
            Ok(())
        }
        _ => Err(LayoutProblemKind::NotYet),
    })
}

/// A key's whole number kept inside `min..=max`, with the problem to report
/// when it had to be moved there.
fn number_in(
    section: &IniSection,
    key: &str,
    min: i32,
    max: i32,
) -> Result<(i32, Option<LayoutProblemKind>), LayoutProblemKind> {
    let value = whole_number(section, key).ok_or(LayoutProblemKind::NotANumber)?;
    let used = value.clamp(min, max);
    Ok((
        used,
        (used != value).then_some(LayoutProblemKind::Clamped { used }),
    ))
}

/// The sections of the pieces over the battlefield: `[Messages]`, `[Timers]`
/// and `[Banner]` (a box: `Hold`, `X`, `Y`, `Width`, `Height`), `[Powers]`
/// (`X`, `Y`) and `[Tooltip]` (`Hold`: `Cursor` or a corner, and `X`, `Y`).
/// `false` for any other part.
fn read_field_part(
    field: &mut FieldLayout,
    section: &IniSection,
    part: &str,
    mut problem: impl FnMut(&str, LayoutProblemKind),
) -> bool {
    let tooltip = part.eq_ignore_ascii_case("Tooltip");
    let field_part = FieldPart::named(part);
    if !tooltip && field_part.is_none() {
        return false;
    }
    let is = |key: &str, name: &str| key.eq_ignore_ascii_case(name);
    for key in section.keys() {
        // A place: how far in from the edge the part holds.
        let place =
            |slot: &mut i32, problem: &mut dyn FnMut(&str, LayoutProblemKind)| match number_in(
                section, key, -8192, 8192,
            ) {
                Ok((value, clamped)) => {
                    *slot = value;
                    if let Some(kind) = clamped {
                        problem(key, kind);
                    }
                }
                Err(kind) => problem(key, kind),
            };
        let hold = || Hold::named(section.read_name(key, NAME_CAPACITY).unwrap_or_default());
        let boxed: Option<&mut PartBox> = match field_part {
            Some(FieldPart::Messages) => Some(&mut field.messages),
            Some(FieldPart::Timers) => Some(&mut field.timers),
            Some(FieldPart::Banner) => Some(&mut field.banner),
            Some(FieldPart::Powers) | None => None,
        };
        if let Some(part_box) = boxed {
            if is(key, "X") {
                place(&mut part_box.x, &mut problem);
            } else if is(key, "Y") {
                place(&mut part_box.y, &mut problem);
            } else if is(key, "Hold") {
                match hold() {
                    Some(hold) => part_box.hold = hold,
                    None => problem(key, LayoutProblemKind::NotYet),
                }
            } else if is(key, "Width") || is(key, "Height") {
                // `Rest` is what is left of the battlefield.
                let size = if same_list(section, key, "Rest") {
                    None
                } else {
                    match number_in(section, key, 1, 8192) {
                        Ok((value, clamped)) => {
                            if let Some(kind) = clamped {
                                problem(key, kind);
                            }
                            Some(value)
                        }
                        Err(kind) => {
                            problem(key, kind);
                            continue;
                        }
                    }
                };
                if is(key, "Width") {
                    part_box.width = size;
                } else {
                    part_box.height = size;
                }
            } else if FIELD_PART_LATER.iter().any(|later| is(key, later)) {
                problem(key, LayoutProblemKind::NotYet);
            } else {
                problem(key, LayoutProblemKind::UnknownKey);
            }
        } else if is(key, "X") {
            let slot = if tooltip {
                &mut field.tooltip.x
            } else {
                &mut field.powers[0]
            };
            place(slot, &mut problem);
        } else if is(key, "Y") {
            let slot = if tooltip {
                &mut field.tooltip.y
            } else {
                &mut field.powers[1]
            };
            place(slot, &mut problem);
        } else if tooltip && is(key, "Hold") {
            if same_list(section, key, "Cursor") {
                field.tooltip.hold = None;
            } else {
                match hold() {
                    Some(hold) => field.tooltip.hold = Some(hold),
                    None => problem(key, LayoutProblemKind::NotYet),
                }
            }
        } else if is(key, "Hold")
            || is(key, "Width")
            || is(key, "Height")
            || FIELD_PART_LATER.iter().any(|later| is(key, later))
        {
            problem(key, LayoutProblemKind::NotYet);
        } else {
            problem(key, LayoutProblemKind::UnknownKey);
        }
    }
    true
}

/// A section for a part the original does not have: with `Type=Button`,
/// `Type=Text`, `Type=Row`, `Type=Column` or `Type=CameoGrid` it is a part
/// the layout adds. A row's or a column's `Parts=` names go to `children`. `None` when it has no `Type=`
/// this version knows, `Some(None)` when it has one but cannot be built
/// (its problem is reported), else the part, in the battlefield until a
/// `Parts=` list says where it goes.
fn read_added_part(
    section: &IniSection,
    name: &str,
    children: &mut Vec<String>,
    mut problem: impl FnMut(&str, LayoutProblemKind),
) -> Option<Option<AddedPart>> {
    let word = |key: &str| section.read_name(key, NAME_CAPACITY).map(str::trim);
    let type_key = section
        .keys()
        .find(|key| key.eq_ignore_ascii_case("Type"))?;
    let kind = word(type_key)?;
    let (button, text) = (
        kind.eq_ignore_ascii_case("Button"),
        kind.eq_ignore_ascii_case("Text"),
    );
    // A row or a column: `Some(column)`.
    let group = if kind.eq_ignore_ascii_case("Row") {
        Some(false)
    } else if kind.eq_ignore_ascii_case("Column") {
        Some(true)
    } else {
        None
    };
    let grid = kind.eq_ignore_ascii_case("CameoGrid");
    if !(button || text || grid || group.is_some()) {
        return None;
    }
    let is = |key: &str, name: &str| key.eq_ignore_ascii_case(name);
    let mut place = PartBox::WHOLE;
    let (mut command, mut label, mut value, mut art) = (None, None, None, None);
    let mut gap = 0;
    let (mut tab, mut columns, mut rows) = (None, None, 2);
    for key in section.keys() {
        if is(key, "Type") {
        } else if grid && is(key, "Tab") {
            // The four tabs, by the names of the tab commands' strips.
            tab = word(key).and_then(|name| {
                crate::ui::sidebar::SidebarTab::all()
                    .into_iter()
                    .zip(GRID_TABS)
                    .find(|(_, known)| known.eq_ignore_ascii_case(name))
                    .map(|(tab, _)| tab)
            });
        } else if grid && (is(key, "Columns") || is(key, "Rows")) {
            let most = if is(key, "Columns") { 8 } else { 60 };
            match number_in(section, key, 1, most) {
                Ok((number, clamped)) => {
                    if is(key, "Columns") {
                        columns = Some(number as usize);
                    } else {
                        rows = number as usize;
                    }
                    if let Some(kind) = clamped {
                        problem(key, kind);
                    }
                }
                Err(kind) => problem(key, kind),
            }
        } else if group.is_some() && is(key, "Gap") {
            match number_in(section, key, 0, 8192) {
                Ok((number, clamped)) => {
                    gap = number;
                    if let Some(kind) = clamped {
                        problem(key, kind);
                    }
                }
                Err(kind) => problem(key, kind),
            }
        } else if group.is_some() && is(key, "Parts") {
            // The buttons and texts the group holds, in the order it places
            // them.
            children.extend(
                section
                    .read_list(key, LIST_CAPACITY)
                    .unwrap_or_default()
                    .into_iter()
                    .map(str::trim)
                    .filter(|child| !child.is_empty() && !child.eq_ignore_ascii_case(NO_PARTS))
                    .map(str::to_owned),
            );
        } else if is(key, "X") || is(key, "Y") {
            match number_in(section, key, -8192, 8192) {
                Ok((number, clamped)) => {
                    if is(key, "X") {
                        place.x = number;
                    } else {
                        place.y = number;
                    }
                    if let Some(kind) = clamped {
                        problem(key, kind);
                    }
                }
                Err(kind) => problem(key, kind),
            }
        } else if is(key, "Width") || is(key, "Height") {
            match number_in(section, key, 1, 8192) {
                Ok((number, clamped)) => {
                    if is(key, "Width") {
                        place.width = Some(number);
                    } else {
                        place.height = Some(number);
                    }
                    if let Some(kind) = clamped {
                        problem(key, kind);
                    }
                }
                Err(kind) => problem(key, kind),
            }
        } else if is(key, "Hold") {
            match Hold::named(word(key).unwrap_or_default()) {
                Some(hold) => place.hold = hold,
                None => problem(key, LayoutProblemKind::NotYet),
            }
        } else if button && is(key, "Command") {
            command = word(key).filter(|name| !name.is_empty());
        } else if button && is(key, "Label") {
            label = word(key);
        } else if text && is(key, "Value") {
            value = word(key).and_then(TextValue::named);
        } else if button && is(key, "Art") {
            art = word(key).filter(|name| !name.is_empty());
        } else if is(key, "Art") || is(key, "Parts") {
            problem(key, LayoutProblemKind::NotYet);
        } else {
            problem(key, LayoutProblemKind::UnknownKey);
        }
    }
    let kind = if let Some(column) = group {
        AddedKind::Group { column, gap }
    } else if grid {
        // A grid shows one tab; without a tab it has nothing to show.
        let Some(tab) = tab else {
            problem("Tab", LayoutProblemKind::UnknownValue);
            return Some(None);
        };
        AddedKind::CameoGrid { tab, columns, rows }
    } else if button {
        let Some(command) = command else {
            problem("Command", LayoutProblemKind::UnknownCommand);
            return Some(None);
        };
        AddedKind::Button {
            command: command.to_owned(),
            label: label.unwrap_or(command).to_owned(),
            art: art.map(str::to_owned),
        }
    } else {
        let Some(value) = value else {
            problem("Value", LayoutProblemKind::UnknownValue);
            return Some(None);
        };
        AddedKind::Text { value }
    };
    Some(Some(AddedPart {
        section: name.to_owned(),
        holder: Holder::Field,
        kind,
        place,
    }))
}

/// The four tabs' names for a cameo grid's `Tab=`, in the tabs' order.
const GRID_TABS: [&str; 4] = ["Building", "Defense", "Infantry", "Vehicle"];

/// The list that names an added part: the sidebar's, the battlefield's, or
/// the `Parts=` of a row or a column the layout adds, by that group's name.
enum ListedIn {
    Side,
    Field,
    Group(String),
}

/// Keys of the format a piece over the battlefield cannot take yet.
const FIELD_PART_LATER: [&str; 5] = ["Type", "Art", "Parts", "Command", "Value"];

/// A panel's `Parts=`: the parts it names that the original holds in that
/// panel, in the order written and each once, then whether it named something
/// that is no part at all, and a part that belongs to another panel (moving a
/// part between panels comes with a later version).
fn part_list(
    section: &IniSection,
    key: &str,
    panel: Panel,
) -> (Vec<SidebarPart>, Vec<String>, bool) {
    let mut listed = Vec::new();
    let (mut unknown, mut elsewhere) = (Vec::new(), false);
    let names = section.read_list(key, LIST_CAPACITY).unwrap_or_default();
    for name in names.into_iter().map(str::trim) {
        if name.is_empty() || name.eq_ignore_ascii_case(NO_PARTS) {
            continue;
        }
        let part = SidebarPart::STOCK_ORDER
            .into_iter()
            .find(|part| part.name().eq_ignore_ascii_case(name));
        match part {
            Some(part) if part.panel() == panel => {
                if !listed.contains(&part) {
                    listed.push(part);
                }
            }
            Some(_) => elsewhere = true,
            None => unknown.push(name.to_owned()),
        }
    }
    (listed, unknown, elsewhere)
}

/// A key's value as a whole number: ReadInt's result, for text that is
/// nothing but an optional minus sign and up to nine digits. ReadInt alone
/// reads other text as zero, which would move a part to the corner without a
/// word.
fn whole_number(section: &IniSection, key: &str) -> Option<i32> {
    let text = section.read_name(key, NAME_CAPACITY)?;
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    section.read_int_value(key)
}

/// Whether a key's comma list names the same things as `accepted`, in the
/// same order, without regard to capitals or spaces.
fn same_list(section: &IniSection, key: &str, accepted: &str) -> bool {
    let Some(given) = section.read_list(key, LIST_CAPACITY) else {
        return false;
    };
    let mut given = given.into_iter().map(str::trim);
    let mut accepted = accepted.split(',');
    loop {
        match (given.next(), accepted.next()) {
            (None, None) => return true,
            (Some(given), Some(accepted)) if given.eq_ignore_ascii_case(accepted) => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::object_type::ObjectCategory;
    use crate::sim::intern::StringInterner;
    use crate::sim::production::{BuildOption, ProductionCategory};
    use crate::ui::sidebar::cameo_order::{CameoStrips, StripColumns};
    use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
    use crate::ui::sidebar::{
        SidebarFrame, SidebarTab, SidebarTheme, SidebarView, build_sidebar_view_with_spec,
        power_bar_rect, radar_minimap_rect_with_spec,
    };

    /// `count` buildings to offer, each a cameo of the Building strip.
    fn options(count: usize) -> (Vec<BuildOption>, StringInterner) {
        let mut interner = StringInterner::new();
        let options = (0..count)
            .map(|i| BuildOption {
                type_id: interner.intern(&format!("T{i}")),
                cost: 1,
                object_category: ObjectCategory::Building,
                queue_category: ProductionCategory::Building,
                enabled: true,
                at_build_limit: false,
                reason: None,
            })
            .collect();
        (options, interner)
    }

    /// The side's sidebar at 800x600 with `layout` over its original table.
    fn sidebar(
        theme: SidebarTheme,
        layout: Option<&MatchLayout>,
    ) -> (SidebarChromeLayoutSpec, SidebarView) {
        sidebar_with(theme, layout, 0)
    }

    /// [`sidebar`] with `count` cameos on its Building strip.
    fn sidebar_with(
        theme: SidebarTheme,
        layout: Option<&MatchLayout>,
        count: usize,
    ) -> (SidebarChromeLayoutSpec, SidebarView) {
        let mut spec = SidebarChromeLayoutSpec::for_theme(theme);
        if let Some(layout) = layout {
            layout.apply_to_sidebar(&mut spec);
        }
        let (options, interner) = options(count);
        let view = build_sidebar_view_with_spec(
            spec,
            800.0,
            600.0,
            SidebarTab::Building,
            0,
            0,
            0,
            Some([32.0, 28.0]),
            &[],
            &options,
            &[],
            None,
            Some(&interner),
            &[],
            &SidebarGadgetState::new(),
            Some([52.0, 32.0]),
            Some([52.0, 32.0]),
            Some([46.0, 25.0]),
            Some([46.0, 25.0]),
            [Some([72.0, 18.0]); 2],
            &CameoStrips::layout_fixture(&options, &[], &[]),
        );
        (spec, view)
    }

    #[test]
    fn a_side_layout_changes_only_the_numbers_it_writes() {
        // Keys that repeat the original (`Parts`, `type`, `CellWidth`) are
        // accepted, capitals do not count, and the last line has no break.
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nNod=Steel\nDefault=Plain\n\
              [Steel]\nVersion=1\nParts=Field,Side,Bar\n\
              [Steel.Middle]\nY=170\n\
              [Steel.Tabs]\nStep=40\n\
              [Steel.Radar]\ntype=radar\nMapX=10\nMapWidth=120\n\
              [Steel.Cameos]\nCellWidth=60\nRowHeight=60\n\
              [Plain.Power]\nX=9",
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());

        let (spec, view) = sidebar(SidebarTheme::Soviet, file.for_side(Some("nod")));
        // SIDE1 moved from 158 to 170 and took Repair and the tabs with it.
        assert_eq!(view.repair_button.rect.y, 177.0);
        assert_eq!((view.tabs[0].rect.x, view.tabs[0].rect.y), (652.0, 209.0));
        assert_eq!(view.tabs[1].rect.x, 692.0);
        let radar = radar_minimap_rect_with_spec(800.0, spec);
        assert_eq!(
            (radar.x, radar.y, radar.w, radar.h),
            (642.0, 50.0, 120.0, 108.0)
        );
        // (600 - 227 - 25) / 60 rows of 60.
        assert_eq!(view.layout.side2_tile_count, 5);
        assert_eq!(view.layout.cameo_grid_bottom, 527.0);

        // A side without a line takes `Default=`; no file changes nothing.
        let (plain, _) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI")));
        let stock = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        assert_eq!(
            plain,
            SidebarChromeLayoutSpec {
                power_bar_x: 9.0,
                ..stock
            }
        );
        assert!(MatchLayoutFile::none().for_side(Some("GDI")).is_none());
        assert!(MatchLayoutFile::none().is_original() && !file.is_original());
    }

    #[test]
    fn a_broken_file_keeps_the_original_and_names_each_line() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Broken\nNod=Missing\nThirdSide=Future\n\
              [Broken]\nParts=Side,Clock,Field,Bar\nColour=red\n\
              [Broken.Radar]\nMapWidth=wide\nMapHeight=0\nHold=Left\nSpeed=3\n\
              [Broken.Cameos]\nRowHeight=1\nCellHeight=30\nColumns=0\n\
              [Broken.Clock]\nX=4\n\
              [Future]\nVersion=2\n\
              [Future.Radar]\nY=10\n",
        );
        use LayoutProblemKind::*;
        let problems: Vec<_> = file
            .problems()
            .iter()
            .map(|problem| (problem.place(), problem.kind.clone()))
            .collect();
        let expected = [
            ("[Broken] Parts", UnknownPart),
            ("[Broken] Colour", UnknownKey),
            ("[Broken.Radar] MapWidth", NotANumber),
            ("[Broken.Radar] MapHeight", Clamped { used: 1 }),
            ("[Broken.Radar] Hold", NotYet),
            ("[Broken.Radar] Speed", UnknownKey),
            ("[Broken.Cameos] RowHeight", Clamped { used: 16 }),
            ("[Broken.Cameos] CellHeight", NotYet),
            ("[Broken.Cameos] Columns", Clamped { used: 1 }),
            ("[Broken.Clock]", UnknownPart),
            ("[Layouts] Nod", MissingLayout),
            ("[Future] Version", NewerVersion),
        ];
        assert_eq!(
            problems,
            expected.map(|(place, kind)| (place.to_owned(), kind))
        );

        // The lines that could be read still apply; the rest stay original.
        let (spec, _) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI")));
        assert_eq!(
            spec,
            SidebarChromeLayoutSpec {
                radar_height: 1.0,
                cameo_row_height: 16.0,
                cameo_columns: 1.0,
                ..SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied)
            }
        );
        // A missing layout and one from a newer version leave the side alone.
        assert!(file.for_side(Some("Nod")).is_none());
        assert!(file.for_side(Some("ThirdSide")).is_none());
    }

    #[test]
    fn a_layout_leaves_parts_out_and_keeps_the_rest_in_its_order() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Bare\nNod=NoButtons\n\
              [Bare.Side]\nParts=Radar,Cameos,Middle\n\
              [Bare.Middle]\nParts=tabs\n\
              [Bare.Top]\nParts=Options,Sell,Clock\n\
              [NoButtons.Top]\nParts=None\n",
        );
        // Clock is no part, and Sell belongs to another panel.
        let problems: Vec<_> = file
            .problems()
            .iter()
            .map(|problem| (problem.place(), problem.kind.clone()))
            .collect();
        assert_eq!(
            problems,
            [
                (
                    "[Bare.Top] Parts".to_owned(),
                    LayoutProblemKind::UnknownPart
                ),
                ("[Bare.Top] Parts".to_owned(), LayoutProblemKind::NotYet),
            ]
        );

        use SidebarPart::*;
        let (stock_spec, stock) = sidebar(SidebarTheme::Allied, None);
        let (spec, view) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI")));
        // The strip's part Top is not in Side's list, so its Options goes too.
        assert_eq!(
            spec.parts_in_order().collect::<Vec<_>>(),
            [Radar, Cameos, Middle, Tabs]
        );
        // A part that stays keeps its place; one left out takes no click.
        assert_eq!(view.tabs, stock.tabs);
        assert_eq!(view.scroll_down_button.rect, stock.scroll_down_button.rect);
        assert_eq!(
            radar_minimap_rect_with_spec(800.0, spec),
            radar_minimap_rect_with_spec(800.0, stock_spec)
        );
        for left_out in [
            view.repair_button.rect,
            view.sell_button.rect,
            view.top_buttons[0].rect,
            view.top_buttons[1].rect,
            power_bar_rect(&view.layout, spec),
        ] {
            assert_eq!((left_out.w, left_out.h), (0.0, 0.0));
        }
        assert!(stock.repair_button.rect.w > 0.0 && stock.top_buttons[1].rect.w > 0.0);

        // `None` empties a panel that stays.
        let (spec, _) = sidebar(SidebarTheme::Soviet, file.for_side(Some("Nod")));
        assert!(spec.shows(Top) && !spec.shows(Diplomacy) && !spec.shows(Options));
        assert!(spec.shows(Credits) && spec.shows(Money) && spec.shows(Power));
    }

    #[test]
    fn a_layout_sets_the_cameo_columns_for_rows_and_scrolling() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Wide\n[Wide.Cameos]\nColumns=3\nColumnWidth=40\n",
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        let (spec, view) = sidebar_with(SidebarTheme::Allied, file.for_side(Some("GDI")), 7);
        // Three to a row and 40 apart: the fourth cameo starts the next row.
        let x: Vec<f32> = view.items.iter().map(|item| item.rect.x).collect();
        assert_eq!(x[..4], [654.0, 694.0, 734.0, 654.0]);
        assert_eq!(view.items[3].rect.y, view.items[0].rect.y + 50.0);

        // The strip scrolls in the same rows: with two rows of three showing,
        // seven cameos leave one more row, where rows of two would leave two.
        let mut strips = CameoStrips::layout_fixture(&options(7).0, &[], &[]);
        strips.set_columns(StripColumns::new(spec.columns()));
        assert!(strips.queue_scroll(SidebarTab::Building, true, 2, 1));
        strips.step_pending_scroll(2);
        assert_eq!(strips.scroll_row(SidebarTab::Building), 1);
        assert!(!strips.queue_scroll(SidebarTab::Building, true, 2, 1));
    }

    #[test]
    fn a_layout_sets_the_sidebar_width_the_frame_takes() {
        let file = MatchLayoutFile::from_bytes(b"[Layouts]\nGDI=Wide\n[Wide.Side]\nWidth=200\n");
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        let (spec, view) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI")));
        assert_eq!((view.panel_rect.x, view.panel_rect.w), (600.0, 200.0));
        // The frame gives the battlefield and the bottom bar what is left.
        let frame = SidebarFrame::native(800, 600).with_column_width(spec.sidebar_width as u32);
        assert_eq!(frame.tactical_size(), (600, 568));
        assert_eq!(frame.surface_size(), (200, 600));
        assert_eq!(frame.bar_surface_size(), (600, 32));
        assert_eq!(frame.layout_left(), view.panel_rect.x);
    }

    #[test]
    fn a_layout_puts_the_sidebar_and_the_bar_on_other_edges() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Lefty\nNod=Over\n\
              [Lefty.Side]\nHold=left\n[Lefty.Bar]\nHold=Top\n\
              [Over]\nParts=Field,Side\n[Over.Side]\nFloat=yes\nHold=Middle\n\
              [Over.Bar]\nHold=Left\n[Over.Radar]\nFloat=maybe\n",
        );
        // A sidebar in the middle and a bar on the left are not there yet,
        // and only the sidebar can float.
        let problems: Vec<_> = file
            .problems()
            .iter()
            .map(|problem| (problem.place(), problem.kind.clone()))
            .collect();
        assert_eq!(
            problems,
            [
                ("[Over.Side] Hold".to_owned(), LayoutProblemKind::NotYet),
                ("[Over.Bar] Hold".to_owned(), LayoutProblemKind::NotYet),
                (
                    "[Over.Radar] Float".to_owned(),
                    LayoutProblemKind::UnknownKey
                ),
            ]
        );

        let (spec, view) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI")));
        let frame = SidebarFrame::native(800, 600).with_places(spec.places);
        // The sidebar's parts follow it to the left edge, and the battlefield
        // starts beside it and under the bar.
        assert_eq!((view.panel_rect.x, view.tabs[0].rect.x), (0.0, 26.0));
        assert_eq!(radar_minimap_rect_with_spec(800.0, spec).x, 16.0);
        assert_eq!(frame.tactical_rect(), (168, 32, 632, 568));

        // A floating sidebar and no bar leave the battlefield the whole frame.
        let (spec, _) = sidebar(SidebarTheme::Soviet, file.for_side(Some("Nod")));
        let frame = SidebarFrame::native(800, 600).with_places(spec.places);
        assert_eq!(frame.tactical_rect(), (0, 0, 800, 600));
        assert_eq!(frame.column(), (632, 0, 168, 600));
        assert!(!frame.has_bar());
    }

    #[test]
    fn a_layout_lays_the_sidebar_along_the_bottom_as_a_band() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Low\n[Low.Side]\nHold=Bottom\nWidth=Rest\nHeight=160\n\
              [Low.Cameos]\nX=336\nY=0\nColumns=6\n[Low.Power]\nX=320\nY=0\n",
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        let mut spec = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        file.for_side(Some("GDI"))
            .unwrap()
            .apply_to_sidebar(&mut spec);
        let frame = SidebarFrame::native(800, 600)
            .with_band_height(spec.side_height as u32)
            .with_places(spec.places);
        assert_eq!(frame.column(), (0, 440, 800, 160));
        assert_eq!(frame.tactical_rect(), (0, 0, 800, 408));
        // The parts lay out in the band: it starts at the screen's left
        // edge, and its rows are the ones that fit in its own height.
        let (width, height) = frame.layout_screen();
        let layout = crate::ui::sidebar::compute_layout_with_spec(spec, width, height, 0);
        assert_eq!((layout.sidebar_x, layout.strip_x), (0.0, 336.0));
        assert_eq!(layout.side2_tile_count, (160 - 33) / 50);
        assert_eq!(spec.side_width(width), 800.0);
    }

    #[test]
    fn a_layout_places_the_pieces_over_the_battlefield() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Hud\n\
              [Hud.Field]\nParts=Messages,Timers,Powers\n\
              [Hud.Messages]\nX=200\nWidth=300\n\
              [Hud.Timers]\nhold=topright\nY=8\nHeight=120\n\
              [Hud.Tooltip]\nHold=BottomLeft\nX=8\nY=4\n\
              [Hud.Powers]\nX=6\nSpeed=2\n\
              [Hud.Banner]\nHold=Sideways\nWidth=wide\nHeight=Rest\nArt=x.shp\n",
        );
        let problems: Vec<_> = file
            .problems()
            .iter()
            .map(|problem| (problem.place(), problem.kind.clone()))
            .collect();
        assert_eq!(
            problems,
            [
                (
                    "[Hud.Powers] Speed".to_owned(),
                    LayoutProblemKind::UnknownKey
                ),
                ("[Hud.Banner] Hold".to_owned(), LayoutProblemKind::NotYet),
                (
                    "[Hud.Banner] Width".to_owned(),
                    LayoutProblemKind::NotANumber
                ),
                ("[Hud.Banner] Art".to_owned(), LayoutProblemKind::NotYet),
            ]
        );
        let mut spec = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        assert_eq!(spec.field, FieldLayout::ORIGINAL);
        file.for_side(Some("GDI"))
            .unwrap()
            .apply_to_sidebar(&mut spec);
        let battlefield = [0, 0, 632, 568];
        // Messages start 200 in and wrap 300 on; the timers' box is 120 tall
        // at the top, so they stack up from 128 down at the right edge.
        assert_eq!(
            spec.field.part_rect(FieldPart::Messages, battlefield),
            [200, 0, 300, 568]
        );
        assert_eq!(
            spec.field.part_rect(FieldPart::Timers, battlefield),
            [0, 8, 632, 120]
        );
        assert_eq!(spec.field.part_rect(FieldPart::Powers, battlefield)[0], 6);
        // The banner is left out, and the broken lines changed nothing of it.
        assert!(!spec.field.shows(FieldPart::Banner) && spec.field.shows(FieldPart::Timers));
        assert_eq!(spec.field.banner, PartBox::WHOLE);
        assert_eq!(
            spec.field
                .tooltip
                .fixed_in([0.0, 0.0, 800.0, 600.0], [86.0, 38.0]),
            Some([8.0, 558.0])
        );
    }

    #[test]
    fn a_layout_adds_buttons_and_texts_to_the_sidebar_and_the_battlefield() {
        let mut file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Hud\n\
              [Hud.Side]\nParts=Credits,Top,Radar,Middle,Cameos,Power,Halt,Missing\n\
              [Hud.Field]\nParts=Messages,Timers,Banner,Powers,Clock,Fly\n\
              [Hud.Halt]\nType=Button\nCommand=StopObject\nLabel=Stop\nArt=button08.shp\nX=10\nY=300\n\
              [Hud.Clock]\ntype=text\nValue=time\nHold=TopRight\nX=6\nY=6\n\
              [Hud.Fly]\nType=Button\nCommand=Levitate\n\
              [Hud.Loose]\nType=Text\nValue=Credits\n\
              [Hud.Gold]\nType=Text\nValue=Gold\n\
              [Hud.Dish]\nType=Radar\n",
        );
        // The input code owns the command names; the loader asks it.
        file.check_commands(|name| name.eq_ignore_ascii_case("StopObject"));
        let problems: Vec<_> = file
            .problems()
            .iter()
            .map(|problem| (problem.place(), problem.kind.clone()))
            .collect();
        assert_eq!(
            problems,
            [
                (
                    "[Hud.Gold] Value".to_owned(),
                    LayoutProblemKind::UnknownValue
                ),
                ("[Hud.Dish]".to_owned(), LayoutProblemKind::UnknownPart),
                (
                    "[Hud.Side] Parts".to_owned(),
                    LayoutProblemKind::UnknownPart
                ),
                ("[Hud.Loose]".to_owned(), LayoutProblemKind::NotListed),
                (
                    "[Hud.Fly] Command".to_owned(),
                    LayoutProblemKind::UnknownCommand
                ),
            ]
        );
        assert_eq!(file.art_names(), ["button08.shp"]);
        let layout = file.for_side(Some("GDI")).unwrap();
        let [halt, clock] = &layout.added()[..] else {
            panic!("two parts are added: {:?}", layout.added());
        };
        assert_eq!(halt.holder, Holder::Side);
        assert_eq!(
            halt.kind,
            AddedKind::Button {
                command: "StopObject".to_owned(),
                label: "Stop".to_owned(),
                art: Some("button08.shp".to_owned()),
            }
        );
        assert_eq!(
            halt.rect_in([632, 0, 168, 600], [40, 17]),
            [642, 300, 40, 17]
        );
        assert_eq!(clock.holder, Holder::Field);
        assert_eq!(
            clock.kind,
            AddedKind::Text {
                value: TextValue::Time
            }
        );
        assert_eq!(clock.rect_in([0, 0, 632, 568], [40, 13]), [586, 6, 40, 13]);
        // The sidebar's own parts are as the list has them.
        let mut spec = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        layout.apply_to_sidebar(&mut spec);
        assert!(spec.shows(SidebarPart::Power) && spec.shows(SidebarPart::Tabs));
    }

    #[test]
    fn a_layout_lines_buttons_up_in_a_row() {
        let mut file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Hud\n\
              [Hud.Field]\nParts=Messages,Timers,Banner,Powers,Bad,Orders\n\
              [Hud.Bad]\nType=Button\nCommand=Levitate\n\
              [Hud.Orders]\nType=row\nHold=Bottom\nY=4\nGap=2\nParts=Halt,Fly,Guard,Orders,Ghost\n\
              [Hud.Halt]\nType=Button\nCommand=Stop\n\
              [Hud.Fly]\nType=Button\nCommand=Levitate\n\
              [Hud.Guard]\nType=Button\nCommand=Guard\n\
              [Hud.Stray]\nType=Column\nParts=Lone\n\
              [Hud.Lone]\nType=Text\nValue=Time\n",
        );
        file.check_commands(|name| !name.eq_ignore_ascii_case("Levitate"));
        let problems: Vec<_> = file
            .problems()
            .iter()
            .map(|problem| (problem.place(), problem.kind.clone()))
            .collect();
        assert_eq!(
            problems,
            [
                // A row can't hold itself, and Ghost has no section.
                (
                    "[Hud.Orders] Parts".to_owned(),
                    LayoutProblemKind::UnknownPart
                ),
                (
                    "[Hud.Orders] Parts".to_owned(),
                    LayoutProblemKind::UnknownPart
                ),
                // A column no list names shows nothing, nor does its text.
                ("[Hud.Stray]".to_owned(), LayoutProblemKind::NotListed),
                ("[Hud.Lone]".to_owned(), LayoutProblemKind::NotListed),
                (
                    "[Hud.Bad] Command".to_owned(),
                    LayoutProblemKind::UnknownCommand
                ),
                (
                    "[Hud.Fly] Command".to_owned(),
                    LayoutProblemKind::UnknownCommand
                ),
            ]
        );
        // The row and its two real buttons are left, and the buttons still
        // name the row after the refused button before it is gone.
        let added = file.for_side(Some("GDI")).unwrap().added();
        assert_eq!(added.len(), 3);
        assert_eq!(
            added[0].kind,
            AddedKind::Group {
                column: false,
                gap: 2
            }
        );
        assert_eq!(added[0].holder, Holder::Field);
        assert!(
            added[1..]
                .iter()
                .all(|part| part.holder == Holder::Group(0))
        );
        // Three buttons 52 wide would sit centred, 4 up from the bottom.
        let (places, whole) = crate::ui::added_parts::flow(&[[52, 32]; 3], false, 2, None);
        assert_eq!(
            added[0].rect_in([0, 0, 632, 568], whole),
            [236, 532, 160, 32]
        );
        assert_eq!(places[2], [108, 0]);
    }

    #[test]
    fn a_layout_adds_a_grid_of_build_pictures_for_a_tab() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Wide\n\
              [Wide.Side]\nParts=Credits,Top,Radar,Middle,Cameos,Power,More,Lost\n\
              [Wide.Field]\nParts=Messages,Timers,Banner,Powers,Stray\n\
              [Wide.More]\nType=CameoGrid\nTab=building\nColumns=3\nRows=1\nX=0\nY=500\n\
              [Wide.Lost]\nType=CameoGrid\nRows=2\n\
              [Wide.Stray]\nType=CameoGrid\nTab=Vehicle\n",
        );
        let problems: Vec<_> = file
            .problems()
            .iter()
            .map(|problem| (problem.place(), problem.kind.clone()))
            .collect();
        assert_eq!(
            problems,
            [
                // A grid needs a tab, and draws only in the sidebar.
                (
                    "[Wide.Lost] Tab".to_owned(),
                    LayoutProblemKind::UnknownValue
                ),
                (
                    "[Wide.Side] Parts".to_owned(),
                    LayoutProblemKind::UnknownPart
                ),
                ("[Wide.Field] Parts".to_owned(), LayoutProblemKind::NotYet),
                ("[Wide.Stray]".to_owned(), LayoutProblemKind::NotListed),
            ]
        );
        let added = file.for_side(Some("GDI")).unwrap().added();
        assert_eq!(added.len(), 1);
        assert_eq!(
            added[0].kind,
            AddedKind::CameoGrid {
                tab: SidebarTab::Building,
                columns: Some(3),
                rows: 1
            }
        );

        // Five buildings: the sidebar's own grid shows them all, and the
        // added grid, scrolled past its end, shows its last row of three.
        let spec = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        let (options, interner) = options(5);
        let strips = CameoStrips::layout_fixture(&options, &[], &[]);
        let mut view = build_sidebar_view_with_spec(
            spec,
            800.0,
            600.0,
            SidebarTab::Building,
            0,
            0,
            0,
            Some([32.0, 28.0]),
            &[],
            &options,
            &[],
            None,
            Some(&interner),
            &[],
            &SidebarGadgetState::new(),
            Some([52.0, 32.0]),
            Some([52.0, 32.0]),
            Some([46.0, 25.0]),
            Some([46.0, 25.0]),
            [Some([72.0, 18.0]); 2],
            &strips,
        );
        let own = view.items.clone();
        assert_eq!(own.len(), 5);
        let [x, y, _, _] = added[0].rect_in([632, 0, 168, 600], [189, 50]);
        crate::ui::sidebar::append_added_grids(
            &mut view,
            spec,
            &[crate::ui::sidebar::AddedGrid {
                part: 0,
                tab: SidebarTab::Building,
                origin: [x as f32, y as f32],
                columns: 3,
                rows: 1,
                scroll_rows: 9,
            }],
            &[],
            &options,
            &[],
            None,
            Some(&interner),
            &[],
            &strips,
        );
        // The sidebar's own cameos are as they were, and the grid's follow.
        assert_eq!(view.items[..5], own[..]);
        let grid = view.added_grids[0];
        assert_eq!(
            (grid.first, grid.len, grid.scroll_rows, grid.max_scroll_rows),
            (5, 2, 1, 1)
        );
        let places: Vec<_> = view.items[5..]
            .iter()
            .map(|item| (item.rect.x, item.rect.y, item.type_id.clone()))
            .collect();
        assert_eq!(
            places,
            [
                (632.0, 500.0, own[3].type_id.clone()),
                (695.0, 500.0, own[4].type_id.clone()),
            ]
        );
    }

    #[test]
    fn a_players_file_changes_only_what_it_writes_over_the_mods() {
        let mod_file = IniFile::from_str(
            "[Layouts]\nNod=Low\n[Low.Radar]\nX=4\nY=60\n[Low.Tabs]\nStep=40\n\
             [Low.Field]\nParts=Messages,Timers,Banner,Powers,Halt\n\
             [Low.Halt]\nType=Button\nCommand=Stop\nX=10\nY=20\n",
        );
        // The player moved the radar and the mod's button, tried to give the
        // Soviets another layout, and laid out the Allies, whom the mod left
        // as they were.
        let player_file = IniFile::from_str(
            "[Layouts]\nNod=Own\nGDI=OwnGDI\n[Low.Radar]\nY=70\n[Low.Halt]\nX=300\n\
             [Own.Radar]\nY=1\n[OwnGDI.Timers]\nHold=TopRight\nHeight=100\n",
        );
        let file = MatchLayoutFile::from_layers(Some(&mod_file), Some(&player_file));
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        assert_eq!(file.layout_name_for_side(Some("Nod")), Some("Low"));
        let mut soviet = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Soviet);
        file.for_side(Some("Nod"))
            .unwrap()
            .apply_to_sidebar(&mut soviet);
        // The radar keeps the mod's X and takes the player's Y; the tabs
        // keep the mod's step.
        assert_eq!(
            (
                soviet.radar_housing_x,
                soviet.radar_housing_y,
                soviet.tab_pitch
            ),
            (4.0, 70.0, 40.0)
        );
        let halt = &file.for_side(Some("Nod")).unwrap().added()[0];
        assert_eq!((halt.place.x, halt.place.y), (300, 20));
        // The Allies get the player's own layout.
        assert_eq!(file.layout_name_for_side(Some("GDI")), Some("OwnGDI"));
        let mut allied = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        file.for_side(Some("GDI"))
            .unwrap()
            .apply_to_sidebar(&mut allied);
        assert_eq!(
            allied.field.part_rect(FieldPart::Timers, [0, 0, 632, 568]),
            [0, 0, 632, 100]
        );
        // With no player's file the mod's stands as it is, and with neither
        // every side keeps the original.
        assert_eq!(
            MatchLayoutFile::from_layers(Some(&mod_file), None),
            MatchLayoutFile::from_ini(&mod_file)
        );
        assert!(MatchLayoutFile::from_layers(None, None).is_original());
    }

    #[test]
    fn the_sample_layout_reads_cleanly_and_does_what_it_says() {
        let file = MatchLayoutFile::from_bytes(
            crate::test_fixture::text("docs/examples/veraui.ini").as_bytes(),
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        // The Allies' is mirrored: the sidebar on the left, the bar on top,
        // Repair and Sell swapped, and the tabs right to left over the same
        // four places.
        let (_, stock) = sidebar(SidebarTheme::Allied, None);
        let (spec, view) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI")));
        let frame = SidebarFrame::native(800, 600).with_places(spec.places);
        assert_eq!(frame.tactical_rect(), (168, 32, 632, 568));
        let from_side = |view: &SidebarView, x: f32| x - view.panel_rect.x;
        assert_eq!(
            from_side(&view, view.repair_button.rect.x),
            from_side(&stock, stock.sell_button.rect.x)
        );
        assert_eq!(
            from_side(&view, view.sell_button.rect.x),
            from_side(&stock, stock.repair_button.rect.x)
        );
        let places = |view: &SidebarView| -> Vec<f32> {
            view.tabs
                .iter()
                .map(|tab| from_side(view, tab.rect.x))
                .collect()
        };
        let mut reversed = places(&stock);
        reversed.reverse();
        assert_eq!(places(&view), reversed);
        // The Soviets' lies along the bottom, two rows of cameos beside the
        // middle panel beside the radar.
        let mut spec = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Soviet);
        file.for_side(Some("Nod"))
            .unwrap()
            .apply_to_sidebar(&mut spec);
        let frame = SidebarFrame::native(800, 600)
            .with_band_height(spec.side_height as u32)
            .with_places(spec.places);
        assert_eq!(frame.tactical_rect(), (0, 0, 800, 408));
        let (width, height) = frame.layout_screen();
        let layout = crate::ui::sidebar::compute_layout_with_spec(spec, width, height, 0);
        assert_eq!(
            (layout.radar_x, layout.side1_x, layout.strip_x),
            (0.0, 168.0, 336.0)
        );
        assert_eq!(layout.side2_tile_count, 2);
        // Two buttons and the match clock sit in the band's empty half.
        let added = file.for_side(Some("Nod")).unwrap().added();
        assert_eq!(added.len(), 5);
        // A grid for the infantry tab, three rows of three.
        assert_eq!(
            added[2].kind,
            AddedKind::CameoGrid {
                tab: SidebarTab::Infantry,
                columns: Some(3),
                rows: 3
            }
        );
        // A row of the two buttons, then the clock, both in the sidebar.
        assert_eq!(
            (added[0].holder, added[1].holder),
            (Holder::Side, Holder::Side)
        );
        assert_eq!(
            added[0].kind,
            AddedKind::Group {
                column: false,
                gap: 4
            }
        );
        assert_eq!(
            added[1].kind,
            AddedKind::Text {
                value: TextValue::Time
            }
        );
        assert!(
            added[3..]
                .iter()
                .all(|part| part.holder == Holder::Group(0))
        );
        // Yuri's is wider, three cameos to a row, with an empty top strip.
        let (spec, view) = sidebar_with(SidebarTheme::Yuri, file.for_side(Some("ThirdSide")), 4);
        assert_eq!((view.panel_rect.w, spec.columns()), (232.0, 3));
        assert_eq!(view.items[3].rect.x, view.items[0].rect.x);
        assert!(spec.shows(SidebarPart::Top) && !spec.shows(SidebarPart::Options));
        // Its timers stack up in a box at the top of the battlefield, and
        // its tooltips stay in the screen's bottom-left corner.
        assert_eq!(
            spec.field.part_rect(FieldPart::Timers, [0, 0, 568, 568]),
            [0, 8, 568, 140]
        );
        assert_eq!(
            spec.field
                .tooltip
                .fixed_in([0.0, 0.0, 800.0, 600.0], [86.0, 38.0]),
            Some([8.0, 554.0])
        );
    }
}
