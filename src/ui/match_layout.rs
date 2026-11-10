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
        // The battlefield and the bottom bar take what the sidebar leaves.
        numbers: &[size("Width", Number::SideWidth, 16)],
        fixed: &[
            ("Type", Some("Panel")),
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
}

impl MatchLayout {
    /// Write this layout's numbers and part lists over the side's original
    /// sidebar table.
    pub fn apply_to_sidebar(&self, spec: &mut SidebarChromeLayoutSpec) {
        for &(number, value) in &self.numbers {
            *number.slot(spec) = value;
        }
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

    /// The layout `[Layouts]` gives this `[Sides]` name: its own line, else
    /// `Default=`. `None` keeps the original interface.
    pub fn for_side(&self, side: Option<&str>) -> Option<&MatchLayout> {
        let line = |name: &str| {
            self.sides
                .iter()
                .find(|(listed, _)| listed.eq_ignore_ascii_case(name))
        };
        let (_, layout) = side.and_then(line).or_else(|| line(DEFAULT_SIDE))?;
        self.layouts.get((*layout)?)
    }

    /// Whether every side keeps the original interface and nothing was
    /// refused: no file, or one that names no layout.
    pub fn is_original(&self) -> bool {
        self.sides.is_empty() && self.problems.is_empty()
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
        let Some(stock) = STOCK_PARTS
            .iter()
            .find(|stock| stock.name.eq_ignore_ascii_case(part))
        else {
            problem(name, "", LayoutProblemKind::UnknownPart);
            continue;
        };
        let panel = panel_of(part);
        for key in section.keys() {
            if let Some(read_place) = read_place(&mut read, section, part, key) {
                if let Err(kind) = read_place {
                    problem(name, key, kind);
                }
            } else if let (Some(panel), true) = (panel, key.eq_ignore_ascii_case("Parts")) {
                let (listed, unknown, elsewhere) = part_list(section, key, panel);
                if unknown {
                    problem(name, key, LayoutProblemKind::UnknownPart);
                }
                if elsewhere {
                    problem(name, key, LayoutProblemKind::NotYet);
                }
                read.parts.push((panel, listed));
            } else if let Some(number) = stock
                .numbers
                .iter()
                .find(|number| number.key.eq_ignore_ascii_case(key))
            {
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
    Some(read)
}

/// The keys that say where the sidebar and the bar sit: `[Side] Hold=` (`Left`
/// or `Right`) and `Float=`, and `[Bar] Hold=` (`Top` or `Bottom`). `None` for
/// any other key; an edge the panel cannot sit on yet is refused as not yet.
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
        Some(edge @ (Edge::Left | Edge::Right)) if side => {
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

/// A panel's `Parts=`: the parts it names that the original holds in that
/// panel, in the order written and each once, then whether it named something
/// that is no part at all, and a part that belongs to another panel (moving a
/// part between panels comes with a later version).
fn part_list(section: &IniSection, key: &str, panel: Panel) -> (Vec<SidebarPart>, bool, bool) {
    let mut listed = Vec::new();
    let (mut unknown, mut elsewhere) = (false, false);
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
            None => unknown = true,
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
              [Over]\nParts=Field,Side\n[Over.Side]\nFloat=yes\nHold=Top\n\
              [Over.Bar]\nHold=Left\n[Over.Radar]\nFloat=maybe\n",
        );
        // A sidebar along the top and a bar on the left are not there yet,
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
    fn the_sample_layout_reads_cleanly_and_does_what_it_says() {
        let file = MatchLayoutFile::from_bytes(
            crate::test_fixture::text("docs/examples/veraui.ini").as_bytes(),
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        for (side, theme) in [("GDI", SidebarTheme::Allied), ("Nod", SidebarTheme::Soviet)] {
            let (_, stock) = sidebar(theme, None);
            let (_, view) = sidebar(theme, file.for_side(Some(side)));
            assert_eq!(
                view.repair_button.rect.x, stock.sell_button.rect.x,
                "{side}"
            );
            assert_eq!(
                view.sell_button.rect.x, stock.repair_button.rect.x,
                "{side}"
            );
            // The tabs run right to left over the same four places.
            let places = |view: &SidebarView| -> Vec<f32> {
                view.tabs.iter().map(|tab| tab.rect.x).collect()
            };
            let mut reversed = places(&stock);
            reversed.reverse();
            assert_eq!(places(&view), reversed, "{side}");
        }
        // Yuri's is wider, three cameos to a row, with an empty top strip.
        let (spec, view) = sidebar_with(SidebarTheme::Yuri, file.for_side(Some("ThirdSide")), 4);
        assert_eq!((view.panel_rect.w, spec.columns()), (232.0, 3));
        assert_eq!(view.items[3].rect.x, view.items[0].rect.x);
        assert!(spec.shows(SidebarPart::Top) && !spec.shows(SidebarPart::Options));
    }
}
