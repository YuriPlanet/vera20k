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
use crate::ui::sidebar::SidebarChromeLayoutSpec;

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
        numbers: &[],
        fixed: &[
            ("Type", Some("Panel")),
            ("Hold", Some("Right")),
            ("Width", Some("168")),
            ("Height", Some("Rest")),
            ("Parts", Some("Credits,Top,Radar,Middle,Cameos,Power")),
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
            ("Hold", Some("Bottom")),
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
            ("Parts", Some("Money")),
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
            ("Parts", Some("Diplomacy,Options")),
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
            ("Parts", Some("Repair,Sell,Tabs")),
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
            ("Columns", Some("2")),
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
            LayoutProblemKind::NotYet => write!(
                f,
                "{place}: cannot be changed yet; the original value is kept"
            ),
        }
    }
}

/// One layout of the file: the numbers it changes on the side's original.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchLayout {
    numbers: Vec<(Number, f32)>,
}

impl MatchLayout {
    /// Write this layout's numbers over the side's original sidebar table.
    pub fn apply_to_sidebar(&self, spec: &mut SidebarChromeLayoutSpec) {
        for &(number, value) in &self.numbers {
            *number.slot(spec) = value;
        }
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
                    if !same_list(section, key, SCREEN_PARTS) {
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
        for key in section.keys() {
            if let Some(number) = stock
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
    use crate::ui::sidebar::cameo_order::CameoStrips;
    use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
    use crate::ui::sidebar::{
        SidebarTab, SidebarTheme, SidebarView, build_sidebar_view_with_spec,
        radar_minimap_rect_with_spec,
    };

    /// The side's sidebar at 800x600 with `layout` over its original table.
    fn sidebar(
        theme: SidebarTheme,
        layout: Option<&MatchLayout>,
    ) -> (SidebarChromeLayoutSpec, SidebarView) {
        let mut spec = SidebarChromeLayoutSpec::for_theme(theme);
        if let Some(layout) = layout {
            layout.apply_to_sidebar(&mut spec);
        }
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
            &[],
            &[],
            None,
            None,
            &[],
            &SidebarGadgetState::new(),
            Some([52.0, 32.0]),
            Some([52.0, 32.0]),
            None,
            None,
            [None; 2],
            &CameoStrips::default(),
        );
        (spec, view)
    }

    #[test]
    fn a_side_layout_changes_only_the_numbers_it_writes() {
        // Keys that repeat the original (`Parts`, `type`, `Columns`) are
        // accepted, capitals do not count, and the last line has no break.
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nNod=Steel\nDefault=Plain\n\
              [Steel]\nVersion=1\nParts=Field,Side,Bar\n\
              [Steel.Middle]\nY=170\n\
              [Steel.Tabs]\nStep=40\n\
              [Steel.Radar]\ntype=radar\nMapX=10\nMapWidth=120\n\
              [Steel.Cameos]\nColumns=2\nRowHeight=60\n\
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
    }

    #[test]
    fn a_broken_file_keeps_the_original_and_names_each_line() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Broken\nNod=Missing\nThirdSide=Future\n\
              [Broken]\nParts=Side,Field\nColour=red\n\
              [Broken.Radar]\nMapWidth=wide\nMapHeight=0\nHold=Left\nSpeed=3\n\
              [Broken.Cameos]\nRowHeight=1\nColumns=3\n\
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
            ("[Broken] Parts", NotYet),
            ("[Broken] Colour", UnknownKey),
            ("[Broken.Radar] MapWidth", NotANumber),
            ("[Broken.Radar] MapHeight", Clamped { used: 1 }),
            ("[Broken.Radar] Hold", NotYet),
            ("[Broken.Radar] Speed", UnknownKey),
            ("[Broken.Cameos] RowHeight", Clamped { used: 16 }),
            ("[Broken.Cameos] Columns", NotYet),
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
                ..SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied)
            }
        );
        // A missing layout and one from a newer version leave the side alone.
        assert!(file.for_side(Some("Nod")).is_none());
        assert!(file.for_side(Some("ThirdSide")).is_none());
    }

    #[test]
    fn the_sample_layout_reads_cleanly_and_swaps_the_buttons() {
        let file = MatchLayoutFile::from_bytes(
            crate::test_fixture::text("docs/examples/veraui.ini").as_bytes(),
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        for (side, theme) in [
            ("GDI", SidebarTheme::Allied),
            ("Nod", SidebarTheme::Soviet),
            ("ThirdSide", SidebarTheme::Yuri),
        ] {
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
    }
}
