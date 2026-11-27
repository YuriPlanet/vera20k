//! `veraui.ini`: the match interface as a tree of parts a mod lays out.
//!
//! A game-folder file, loose or inside a mod MIX, names a layout for each
//! side. Each `[Layout.Part]` section is one part of the interface and its
//! keys the part's properties; the original interface is the built-in
//! `veraui_default.ini`, which the mod's file and the player's own write over
//! (`ui::match_layout_tree`). `docs/interface-layout.md` is the modder's
//! reference.
//!
//! The file is the player's own display. It never reaches the simulation and
//! is not one of the sources online players' rules are compared by.
//!
//! A bad file never stops the game: every refusal is a [`LayoutProblem`] and
//! the value under it stays. A VERA file, not a gamemd one, so section and
//! key names match without regard to capitals and a missing last line break
//! loses nothing; values still go through the native INI readers.
//!
//! ## Dependency rules
//! - Part of ui/ — reads the rules/ INI store; no app/ or render/.

use std::sync::{Arc, LazyLock};

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::ui::added_parts::{AddedKind, AddedPart, Holder, TextShows, TextValue};
use crate::ui::field_layout::{FieldLayout, FieldPart, Hold, PartBox, TooltipPlace};
use crate::ui::match_layout_tree::{TABLES, Tree, TreeProblem, VALUE_CAPACITY, exists, part_of};
use crate::ui::sidebar::{Edge, PanelPlaces, SidebarChromeLayoutSpec, SidebarPart, SidebarTheme};

/// The file's name in the game folder or a mod MIX.
pub const FILE_NAME: &str = "VERAUI.INI";

/// `[Layouts]`: one `Side=Layout` line per `[Sides]` name, and `Default=`
/// for every side without a line.
const LAYOUTS_SECTION: &str = "Layouts";
const DEFAULT_SIDE: &str = "Default";

/// ReadString capacity of a layout's name.
const NAME_CAPACITY: usize = 0x80;

/// The built-in file, the bottom layer.
static BUILT_IN: LazyLock<IniFile> = LazyLock::new(|| {
    parse(include_bytes!("veraui_default.ini")).expect("veraui_default.ini is an INI file")
});

/// The two position tables as they are built in, by [`TABLES`] order.
static ORIGINAL: LazyLock<[MatchLayout; 2]> = LazyLock::new(|| {
    TABLES.map(|table| {
        let (layout, problems) = read_layout(&[&BUILT_IN], table, table);
        debug_assert!(problems.is_empty(), "{problems:?}");
        layout
    })
});

/// `LoadFromStraw` drops a last line that has no line break.
fn parse(bytes: &[u8]) -> Option<IniFile> {
    let mut complete = Vec::with_capacity(bytes.len() + 1);
    complete.extend_from_slice(bytes);
    complete.push(b'\n');
    IniFile::from_bytes(&complete).ok()
}

/// A position table's index: the allied one (`GDI`) or the other.
fn table(gdi_positions: bool) -> usize {
    usize::from(!gdi_positions)
}

/// How far a part may be placed: further than any screen reaches.
const FAR: i32 = 8192;

/// One number of the sidebar's table, the part key that writes it and the
/// range it is clamped to.
struct NumberKey {
    node: &'static str,
    key: &'static str,
    min: i32,
    max: i32,
    slot: fn(&mut SidebarChromeLayoutSpec) -> &mut f32,
}

/// A place or a step: any whole number within [`FAR`].
const fn place(
    node: &'static str,
    key: &'static str,
    slot: fn(&mut SidebarChromeLayoutSpec) -> &mut f32,
) -> NumberKey {
    NumberKey {
        node,
        key,
        min: -FAR,
        max: FAR,
        slot,
    }
}

/// A size something is divided by or drawn into: never below `min`.
const fn size(
    node: &'static str,
    key: &'static str,
    min: i32,
    slot: fn(&mut SidebarChromeLayoutSpec) -> &mut f32,
) -> NumberKey {
    NumberKey {
        node,
        key,
        min,
        max: FAR,
        slot,
    }
}

const NUMBERS: [NumberKey; 13] = [
    // The battlefield and the bar take what the sidebar leaves. Its width
    // counts on the left or the right edge, its height on the top or the
    // bottom; `Rest` is the other one, the screen's own.
    size("Side", "Width", 16, |spec| &mut spec.sidebar_width),
    size("Side", "Height", 16, |spec| &mut spec.side_height),
    size("Minimap", "Width", 1, |spec| &mut spec.radar_width),
    size("Minimap", "Height", 1, |spec| &mut spec.radar_height),
    // Each visible cameo is a gadget, so a row stays a row.
    NumberKey {
        node: "Cameos",
        key: "Columns",
        min: 1,
        max: 8,
        slot: |spec| &mut spec.cameo_columns,
    },
    size("Cameos", "CellWidth", 1, |spec| &mut spec.cameo_width),
    size("Cameos", "CellHeight", 1, |spec| &mut spec.cameo_height),
    place("Cameos", "CellX", |spec| &mut spec.cameo_inset_x),
    place("Cameos", "CellY", |spec| &mut spec.cameo_inset_y),
    place("Cameos", "ColumnWidth", |spec| &mut spec.cameo_column_pitch),
    // The row height divides the strip into rows.
    size("Cameos", "RowHeight", 16, |spec| &mut spec.cameo_row_height),
    size("Cameos", "Footer", 0, |spec| &mut spec.strip_footer),
    // The rows the bar's tiles take, one picture each.
    size("Power", "Step", 1, |spec| &mut spec.power_bar_tile_height),
];

/// The four tabs' names for a cameo grid's `Tab=`, in the tabs' order.
const GRID_TABS: [&str; 4] = ["Building", "Defense", "Infantry", "Vehicle"];

/// The word a `Parts=` list uses for "no parts"; the INI store drops a key
/// with no value.
const NO_PARTS: &str = "None";

/// The keys every part may have, then the keys of each type.
const COMMON_KEYS: [&str; 12] = [
    "Type", "Base", "Parts", "Hold", "X", "Y", "Width", "Height", "Art", "Frames", "Palette",
    "Color",
];
const TYPE_KEYS: &[(&str, &[&str])] = &[
    ("Screen", &[]),
    ("Battlefield", &[]),
    ("Box", &[]),
    ("Tooltip", &[]),
    ("Panel", &["Float"]),
    ("Image", &[]),
    ("Text", &["Value", "Label", "Align", "Font"]),
    ("Button", &["Command", "Label"]),
    ("Tab", &["Tab"]),
    ("Radar", &[]),
    ("Minimap", &[]),
    (
        "CameoGrid",
        &[
            "Tab",
            "Columns",
            "Rows",
            "CellWidth",
            "CellHeight",
            "CellX",
            "CellY",
            "ColumnWidth",
            "RowHeight",
            "Footer",
            "RowArt",
            "EmptyRowArt",
            "EndArt",
            "FootArt",
            "Clock",
            "Darken",
            "LabelColor",
            "LabelStrip",
        ],
    ),
    ("PowerBar", &["Step"]),
    ("CommandBar", &["Rail", "LeftCap", "RightCap", "Slot"]),
    ("Row", &["Gap"]),
    ("Column", &["Gap"]),
    // Added for client modules (RFC 0001 section 10): a module fills them.
    ("Bar", &[]),
    ("List", &[]),
];

/// The types a layout can add a part of.
const ADDED_TYPES: [&str; 8] = [
    "Button",
    "Text",
    "Row",
    "Column",
    "CameoGrid",
    "Image",
    "Bar",
    "List",
];

/// Why a line of the file was not used as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutProblemKind {
    /// `[Layouts]` or a `Base=` names a layout the file has no section for,
    /// or a `Base=` a part the layout does not have.
    MissingLayout,
    /// `Base=` lines that come back to where they started.
    Loop,
    /// A section for no part the interface has or a layout can add, or a
    /// `Parts=` list naming one.
    UnknownPart,
    UnknownKey,
    NotANumber,
    /// The number was outside the key's range and became `used`.
    Clamped {
        used: i32,
    },
    /// A yes-or-no key with another word.
    NotYesOrNo,
    /// A key of the part's type this version cannot change yet.
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
                "{place}: names nothing the file has; the original is used"
            ),
            LayoutProblemKind::Loop => {
                write!(f, "{place}: the Base= lines go round in a loop")
            }
            LayoutProblemKind::UnknownPart => {
                write!(f, "{place}: not a part of the interface; it is ignored")
            }
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
                "{place}: not a command the game has; an added button is left out, an original one keeps its own"
            ),
            LayoutProblemKind::UnknownValue => write!(
                f,
                "{place}: not a value the game shows; an added text is left out, an original one keeps its own"
            ),
            LayoutProblemKind::NotListed => {
                write!(f, "{place}: the part is in no Parts= list; it is not shown")
            }
        }
    }
}

/// One layout of the file, its bases written in: the sidebar's numbers and
/// parts, where the panels sit, the battlefield's pieces and what it adds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchLayout {
    /// Each read number of [`NUMBERS`], by its index there.
    numbers: Vec<(usize, f32)>,
    /// Each part's `X` and `Y`, by `SidebarPart as usize`.
    part_places: [[f32; 2]; 19],
    parts: [Option<SidebarPart>; 19],
    places: PanelPlaces,
    field: FieldLayout,
    /// The buttons and texts the layout adds, in the order their holders
    /// list them.
    added: Arc<[AddedPart]>,
    /// The layout's tree, its bases written in, for the keys of the built-in
    /// parts that their own code reads once the file is loaded.
    tree: Arc<Tree>,
    /// `[Colors]`: the built-in file's, then each file's over it, the later
    /// over the earlier. Each line's name, the side it holds for and its text.
    colors: Arc<[(String, Option<usize>, String)]>,
    /// The side's text colour for each side's pictures (by `SidebarTheme as
    /// usize`): the money's `Color=`, resolved once the layout is read.
    side_text: [[u8; 3]; 3],
    /// The cameo labels' `LabelColor=`, resolved the same way.
    label_text: [[u8; 3]; 3],
    /// Each built-in part's `Command=` for each side's pictures, by
    /// `SidebarTheme as usize` and `SidebarPart as usize`.
    commands: [[Option<String>; 19]; 3],
    /// The money's `Value=` and `Align=` for each side's pictures.
    money: [MoneyText; 3],
}

impl MatchLayout {
    /// The buttons and texts this layout adds.
    pub fn added(&self) -> &Arc<[AddedPart]> {
        &self.added
    }

    /// The text `key` of built-in part `part` holds for the side whose
    /// pictures are `theme`: `Key@Side`, else `Key`. For what the parts'
    /// own code reads once a file is loaded (`STOCK_KEYS`), not every frame.
    pub fn text(&self, part: SidebarPart, key: &str, theme: SidebarTheme) -> Option<&str> {
        self.node_text(part.name(), key, theme)
    }

    /// [`Self::text`] of the part named `node`, for a part that is no
    /// [`SidebarPart`], the command bar's `Bar`.
    pub fn node_text(&self, node: &str, key: &str, theme: SidebarTheme) -> Option<&str> {
        self.tree.node(node)?.text_for(key, theme as usize)
    }

    /// The colour a `Color=` value names for the side whose pictures are
    /// `theme`: a `[Colors]` name (`Name@Side`, else `Name`), or `r,g,b` /
    /// `r,g,b,a` itself.
    pub fn color(&self, value: &str, theme: SidebarTheme) -> Option<[u8; 4]> {
        let named = |side: Option<usize>| {
            self.colors
                .iter()
                .rev()
                .find(|(name, of, _)| *of == side && name.eq_ignore_ascii_case(value))
        };
        match named(Some(theme as usize)).or_else(|| named(None)) {
            Some((.., text)) => channels(text),
            None => channels(value),
        }
    }

    /// The `Command=` of built-in part `part` for the side whose pictures
    /// are `theme`, read with the layout.
    pub fn command(&self, part: SidebarPart, theme: SidebarTheme) -> Option<&str> {
        self.commands[theme as usize][part as usize].as_deref()
    }

    /// What the money counter shows and where, for the side whose pictures
    /// are `theme`; a `Value=` or `Align=` the game has not keeps the
    /// original's (`MatchLayoutFile::check_commands` names it).
    pub fn money(&self, theme: SidebarTheme) -> MoneyText {
        self.money[theme as usize]
    }

    /// A position table as it is built in: the allied one (`GDI`) or the
    /// one of the Soviets and Yuri (`Nod`).
    pub fn original(gdi_positions: bool) -> &'static Self {
        &ORIGINAL[table(gdi_positions)]
    }

    /// Write this layout over the sidebar's table.
    pub fn apply_to_sidebar(&self, spec: &mut SidebarChromeLayoutSpec) {
        for &(number, value) in &self.numbers {
            *(NUMBERS[number].slot)(spec) = value;
        }
        spec.part_places = self.part_places;
        spec.parts = self.parts;
        spec.places = self.places;
        spec.field = self.field;
        spec.side_text = self.side_text[spec.theme as usize];
        spec.label_text = self.label_text[spec.theme as usize];
    }

    /// The colour `key` of `part` names for each side's pictures, by
    /// `SidebarTheme as usize`: a `[Colors]` name or `r,g,b`, else the
    /// side's `SideText` when the value is none.
    fn side_colors(&self, part: SidebarPart, key: &str) -> [[u8; 3]; 3] {
        [
            SidebarTheme::Allied,
            SidebarTheme::Soviet,
            SidebarTheme::Yuri,
        ]
        .map(|theme| {
            let [r, g, b, _] = self
                .text(part, key, theme)
                .and_then(|value| self.color(value, theme))
                .or_else(|| self.color("SideText", theme))
                .unwrap_or([255; 4]);
            [r, g, b]
        })
    }
}

/// Where a built-in text sits in its part's width: `Align=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl TextAlign {
    /// The alignment a layout file names, without regard to capitals.
    pub fn named(name: &str) -> Option<Self> {
        [
            ("Left", Self::Left),
            ("Center", Self::Center),
            ("Right", Self::Right),
        ]
        .into_iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, align)| align)
    }
}

/// What the money counter shows and where: `[Layout.Money] Value=` and
/// `Align=`. The original's is the credits, centred on the plate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoneyText {
    pub value: TextValue,
    pub align: TextAlign,
}

impl Default for MoneyText {
    fn default() -> Self {
        Self {
            value: TextValue::Credits,
            align: TextAlign::Center,
        }
    }
}

/// A read `veraui.ini`: which layout each side uses, and what was refused.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchLayoutFile {
    /// `[Layouts]` lines in file order: the side and its layout, `None` when
    /// that layout was refused and the side keeps the original.
    sides: Vec<(String, Option<usize>)>,
    /// Each layout read over either position table, in [`TABLES`] order.
    layouts: Vec<[MatchLayout; 2]>,
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
        parse(bytes).map_or_else(Self::none, |ini| Self::from_ini(&ini))
    }

    pub fn from_ini(ini: &IniFile) -> Self {
        let mut file = Self::default();
        let Some(layouts) = find_section(ini, LAYOUTS_SECTION) else {
            return file;
        };
        let files = [&*BUILT_IN, ini];
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
                    let index = if exists(&files, layout) {
                        file.read(&files, layout);
                        Some(file.layouts.len() - 1)
                    } else {
                        file.problem(LAYOUTS_SECTION, side, LayoutProblemKind::MissingLayout);
                        None
                    };
                    read.push((layout, index));
                    index
                }
            };
            file.sides.push((side.to_owned(), index));
        }
        file
    }

    /// Read `layout` over both position tables, with what was refused.
    fn read(&mut self, files: &[&IniFile], layout: &str) {
        // The layout's own section has nothing but its base.
        for ini in &files[1..] {
            for section in ini.section_names() {
                if section.eq_ignore_ascii_case(layout)
                    && let Some(own) = ini.section(section)
                {
                    for key in own.keys().filter(|key| !key.eq_ignore_ascii_case("Base")) {
                        self.problem(section, key, LayoutProblemKind::UnknownKey);
                    }
                }
            }
        }
        let read = TABLES.map(|table| read_layout(files, layout, table));
        for problem in read.iter().flat_map(|(_, problems)| problems) {
            if !self.problems.contains(problem) {
                self.problems.push(problem.clone());
            }
        }
        self.layouts.push(read.map(|(layout, _)| layout));
        self.names.push(layout.to_owned());
    }

    fn problem(&mut self, section: &str, key: &str, kind: LayoutProblemKind) {
        self.problems.push(LayoutProblem {
            section: section.to_owned(),
            key: key.to_owned(),
            kind,
        });
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

    /// The layout `[Layouts]` gives this `[Sides]` name (its own line, else
    /// `Default=`), over the position table the side uses. `None` keeps the
    /// original interface.
    pub fn for_side(&self, side: Option<&str>, gdi_positions: bool) -> Option<&MatchLayout> {
        self.layouts
            .get(self.layout_index(side)?)
            .map(|layouts| &layouts[table(gdi_positions)])
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

    fn all_layouts_mut(&mut self) -> impl Iterator<Item = &mut MatchLayout> {
        self.layouts.iter_mut().flatten()
    }

    /// The picture files the layouts' added buttons and pictures name
    /// (`Art=`), each once.
    pub fn art_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for part in self
            .layouts
            .iter()
            .flatten()
            .flat_map(|layout| layout.added.iter())
        {
            let art = match &part.kind {
                AddedKind::Button { art: Some(art), .. } | AddedKind::Image { art } => art,
                _ => continue,
            };
            if !names.iter().any(|name| name.eq_ignore_ascii_case(art)) {
                names.push(art.clone());
            }
        }
        names
    }

    /// Leave out every added button whose `Command=` is not `known`, with a
    /// line for each. The command lists belong to the input code, so the
    /// loader asks them once the file is read. A built-in button's
    /// `Command=` that is not `known`, and a built-in text's `Value=` or
    /// `Align=` the game has not, get a line too and keep the built-in one.
    pub fn check_commands(&mut self, known: impl Fn(&str) -> bool) {
        let mut refused: Vec<LayoutProblem> = Vec::new();
        for layout in self.all_layouts_mut() {
            if let Some(kept) = known_commands(&layout.added, &known, &mut refused) {
                layout.added = kept.into();
            }
        }
        // A built-in part's `Command=` other than its own, and a built-in
        // text's `Value=` and `Align=`: the parts' own code reads them once
        // the layout is loaded (`MatchLayout::text`), and one it cannot use
        // keeps the built-in action or text. A key the part does not have
        // in the built-in file is not checked here.
        for pair in &self.layouts {
            for (table, layout) in pair.iter().enumerate() {
                let original = &ORIGINAL[table];
                for part in SidebarPart::STOCK_ORDER {
                    let (Some(node), Some(built_in)) = (
                        layout.tree.node(part.name()),
                        original.tree.node(part.name()),
                    ) else {
                        continue;
                    };
                    for (key, kind) in [
                        ("Command", LayoutProblemKind::UnknownCommand),
                        ("Value", LayoutProblemKind::UnknownValue),
                        ("Align", LayoutProblemKind::UnknownValue),
                    ] {
                        let values = node.values(key);
                        for (at, value) in values.iter().enumerate() {
                            // A file's own value, where it holds for its side.
                            if !value.own
                                || values[at + 1..]
                                    .iter()
                                    .any(|later| later.theme == value.theme)
                            {
                                continue;
                            }
                            let own = match value.theme {
                                Some(theme) => built_in.text_for(key, theme),
                                None => built_in.text(key),
                            };
                            let Some(own) = own else {
                                continue;
                            };
                            let text = value.text.as_str();
                            let usable = own.eq_ignore_ascii_case(text)
                                || match key {
                                    "Command" => known(text),
                                    "Value" => TextValue::named(text).is_some(),
                                    _ => TextAlign::named(text).is_some(),
                                };
                            let problem = LayoutProblem {
                                section: value.section.clone(),
                                key: key.to_owned(),
                                kind: kind.clone(),
                            };
                            if !usable && !refused.contains(&problem) {
                                refused.push(problem);
                            }
                        }
                    }
                }
            }
        }
        self.problems.extend(refused);
    }

    /// Every refused line.
    pub fn problems(&self) -> &[LayoutProblem] {
        &self.problems
    }

    /// A file's text without the layout `layout`: its own section, its
    /// parts' sections and every `[Layouts]` line that names it, and
    /// `[Layouts]` itself once it has no line left. Every other byte stays.
    /// The edit mode's Reset takes the player's changes out this way.
    pub fn without_layout(text: &[u8], layout: &str) -> Vec<u8> {
        use crate::util::ini_writer::{remove_ini_keys, remove_ini_sections};
        let text = remove_ini_sections(text, |name| part_of(name, layout).is_some());
        let sides: Vec<String> = parse(&text)
            .and_then(|ini| {
                let layouts = find_section(&ini, LAYOUTS_SECTION)?;
                Some(
                    layouts
                        .keys()
                        .filter(|side| {
                            layouts
                                .read_name(side, NAME_CAPACITY)
                                .is_some_and(|name| name.eq_ignore_ascii_case(layout))
                        })
                        .map(str::to_owned)
                        .collect(),
                )
            })
            .unwrap_or_default();
        if sides.is_empty() {
            return text;
        }
        let sides: Vec<&str> = sides.iter().map(String::as_str).collect();
        let text = remove_ini_keys(&text, LAYOUTS_SECTION, &sides);
        let emptied = parse(&text).is_none_or(|ini| {
            find_section(&ini, LAYOUTS_SECTION)
                .is_none_or(|layouts| layouts.keys().next().is_none())
        });
        if emptied {
            remove_ini_sections(&text, |name| name.eq_ignore_ascii_case(LAYOUTS_SECTION))
        } else {
            text
        }
    }
}

/// `added` without the buttons whose `Command=` is not `known`, with a line
/// for each in `refused`; `None` when every part stays. A button without a
/// `Command=` stays: it only tells the client modules of its clicks.
fn known_commands(
    added: &[AddedPart],
    known: &impl Fn(&str) -> bool,
    refused: &mut Vec<LayoutProblem>,
) -> Option<Vec<AddedPart>> {
    let keep: Vec<bool> = added
        .iter()
        .map(|part| match &part.kind {
            AddedKind::Button { command, .. } => command.as_deref().is_none_or(known),
            AddedKind::Text { .. }
            | AddedKind::Image { .. }
            | AddedKind::Bar
            | AddedKind::List
            | AddedKind::Group { .. }
            | AddedKind::CameoGrid { .. } => true,
        })
        .collect();
    if keep.iter().all(|&keep| keep) {
        return None;
    }
    // A group's parts name it by its place in the list, which moves up by
    // the parts left out before it.
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
    for (index, part) in added.iter().enumerate() {
        if keep[index] {
            let mut part = part.clone();
            if let Holder::Group(group) = part.holder {
                part.holder = Holder::Group(moved[group]);
            }
            kept.push(part);
        } else {
            let problem = LayoutProblem {
                section: part.section.clone(),
                key: "Command".to_owned(),
                kind: LayoutProblemKind::UnknownCommand,
            };
            if !refused.contains(&problem) {
                refused.push(problem);
            }
        }
    }
    Some(kept)
}

/// The section a map's own parts are named under: `[VeraUI]` (its
/// `Parts=` lists the top-level parts) and `[VeraUI.<part>]`.
pub const MAP_SECTION: &str = "VeraUI";

/// A map's own interface parts (RFC 0001 section 10, "Static layouts"):
/// `[VeraUI] Parts=` lists the top-level parts and each `[VeraUI.<part>]`
/// is one, with the keys of a `veraui.ini` added part. The same for every
/// side; it only adds parts, never a built-in one. A top-level part's
/// holder is [`Holder::Field`], which for a map's part means the screen's
/// overlay above the match interface (screen pixels); a row's or a
/// column's parts are in their group. Buttons whose `Command=` is not
/// `known` are left out, each with a line.
pub fn map_parts(
    map: &IniFile,
    known: impl Fn(&str) -> bool,
) -> (Vec<AddedPart>, Vec<LayoutProblem>) {
    // The map's sections are its own: over an empty bottom layer, so none
    // reads as a built-in part.
    let empty = IniFile::empty();
    let files = [&empty, map];
    let mut tree_problems = Vec::new();
    // Over the first position table, which a map's file has no sections of:
    // the chain of bases ends there.
    let tree = Tree::read(&files, MAP_SECTION, TABLES[0], &mut tree_problems);
    let mut reader = Reader {
        tree: &tree,
        read: Vec::new(),
        problems: Vec::new(),
    };
    for problem in tree_problems {
        if let TreeProblem::UnknownSide { section, key } = problem {
            reader.problem(&section, &key, LayoutProblemKind::UnknownKey);
        }
    }
    let mut listed: Vec<(ListedIn, String, String)> = Vec::new();
    if let Some(section) = find_section(map, MAP_SECTION) {
        let section_name = MAP_SECTION.to_owned();
        let parts = section
            .keys()
            .find(|key| key.eq_ignore_ascii_case("Parts"))
            .and_then(|key| section.read_name(key, VALUE_CAPACITY))
            .map(|text| names(&text))
            .unwrap_or_default();
        listed.extend(
            parts
                .into_iter()
                .map(|name| (ListedIn::Field, name, section_name.clone())),
        );
    }
    let added = read_added(&mut reader, listed);
    read_leftovers(&mut reader);
    let mut problems = reader.problems;
    let added = known_commands(&added, &known, &mut problems).unwrap_or(added);
    (added, problems)
}

fn find_section<'a>(ini: &'a IniFile, name: &str) -> Option<&'a IniSection> {
    ini.section_names()
        .into_iter()
        .find(|section| section.eq_ignore_ascii_case(name))
        .and_then(|section| ini.section(section))
}

/// One picture a layout gives a built-in sidebar part: the part and its
/// picture key, the file, the palette it is drawn with when not the
/// sidebar's own, and how many frames the part uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarArt {
    pub part: SidebarPart,
    pub key: crate::ui::sidebar::ArtKey,
    pub file: String,
    pub palette: Option<String>,
    pub frames: usize,
    /// Whether the layout names a picture other than the built-in one's:
    /// any SHP the game can find.
    pub own: bool,
    /// The built-in file's picture for this part and key, drawn when the
    /// layout's own is not found; `None` when it has none.
    pub built_in: Option<String>,
    /// Whether the layout names a palette other than the built-in one's.
    pub own_palette: bool,
}

/// The pictures `layout` gives the built-in sidebar for the side whose
/// pictures are `theme`, part by part: its own `Art=`, `RowArt=`, ...,
/// `Frames=` and `Palette=` over the built-in file's
/// ([`MatchLayout::original`] for the original sidebar). The command bar's
/// load apart. Read when the sidebar's pictures are loaded, not every frame.
pub fn sidebar_art(layout: &MatchLayout, theme: SidebarTheme) -> Vec<SidebarArt> {
    let original = MatchLayout::original(true);
    let mut art = Vec::new();
    for part in SidebarPart::STOCK_ORDER {
        let Some(node) = layout.tree.node(part.name()) else {
            continue;
        };
        for key in crate::ui::sidebar::ArtKey::ALL {
            let Some(file) = node.text_for(key.name(), theme as usize) else {
                continue;
            };
            let built_in = original.text(part, key.name(), theme);
            let own = built_in.is_none_or(|built_in| !built_in.eq_ignore_ascii_case(file));
            let palette = node.text_for("Palette", theme as usize);
            let own_palette = palette.is_some_and(|palette| {
                original
                    .text(part, "Palette", theme)
                    .is_none_or(|built_in| !built_in.eq_ignore_ascii_case(palette))
            });
            art.push(SidebarArt {
                part,
                key,
                file: file.to_owned(),
                palette: palette.map(str::to_owned),
                frames: node
                    .text("Frames")
                    .map_or(1, |frames| names(frames).len().max(1)),
                own,
                built_in: built_in.map(str::to_owned),
                own_palette,
            });
        }
    }
    art
}

/// What reading one value gives: the value, with a problem to report when it
/// had to be changed; or the problem that keeps the value under it, `None`
/// for a value that silently keeps it.
type Parsed<T> = Result<(T, Option<LayoutProblemKind>), Option<LayoutProblemKind>>;

/// The properties of one layout's tree as they are read, which keys were,
/// and what was refused.
struct Reader<'a> {
    tree: &'a Tree,
    /// Each read key: the node's index and the key.
    read: Vec<(usize, &'static str)>,
    problems: Vec<LayoutProblem>,
}

impl<'a> Reader<'a> {
    fn problem(&mut self, section: &str, key: &str, kind: LayoutProblemKind) {
        let problem = LayoutProblem {
            section: section.to_owned(),
            key: key.to_owned(),
            kind,
        };
        if !self.problems.contains(&problem) {
            self.problems.push(problem);
        }
    }

    /// The value of `key` on part `node` that `parse` takes: the last one
    /// written that it can, each later one a problem when a file over the
    /// built-in one wrote it. With it, the section of a file's own value.
    fn pick<T>(
        &mut self,
        node: &str,
        key: &'static str,
        parse: impl Fn(&str) -> Parsed<T>,
    ) -> Option<(T, Option<String>)> {
        let tree = self.tree;
        let (index, node) = tree
            .nodes
            .iter()
            .enumerate()
            .find(|(_, known)| known.name.eq_ignore_ascii_case(node))?;
        self.read.push((index, key));
        for value in node
            .values(key)
            .iter()
            .rev()
            .filter(|value| value.theme.is_none())
        {
            match parse(&value.text) {
                Ok((parsed, note)) => {
                    if let (Some(kind), true) = (note, value.own) {
                        self.problem(&value.section, key, kind);
                    }
                    return Some((parsed, value.own.then(|| value.section.clone())));
                }
                Err(Some(kind)) if value.own => self.problem(&value.section, key, kind),
                Err(_) => {}
            }
        }
        None
    }

    fn value<T>(
        &mut self,
        node: &str,
        key: &'static str,
        parse: impl Fn(&str) -> Parsed<T>,
    ) -> Option<T> {
        self.pick(node, key, parse).map(|(value, _)| value)
    }

    /// A whole number in `min..=FAR`.
    fn number(&mut self, node: &str, key: &'static str, min: i32) -> Option<i32> {
        self.value(node, key, |text| number_in(text, min, FAR))
    }
}

/// A key's value as a whole number: ReadInt's result, for text that is
/// nothing but an optional minus sign and up to nine digits. ReadInt alone
/// reads other text as zero, which would move a part to the corner without a
/// word.
fn whole_number(text: &str) -> Option<i32> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// A whole number kept inside `min..=max`.
fn number_in(text: &str, min: i32, max: i32) -> Parsed<i32> {
    let value = whole_number(text).ok_or(Some(LayoutProblemKind::NotANumber))?;
    let used = value.clamp(min, max);
    Ok((
        used,
        (used != value).then_some(LayoutProblemKind::Clamped { used }),
    ))
}

/// A size: a whole number of at least one, or `Rest`, what is left.
fn size_or_rest(text: &str) -> Parsed<Option<i32>> {
    if text.eq_ignore_ascii_case("Rest") {
        return Ok((None, None));
    }
    number_in(text, 1, FAR).map(|(value, note)| (Some(value), note))
}

fn yes_no(text: &str) -> Parsed<bool> {
    match text.bytes().next().map(|byte| byte.to_ascii_lowercase()) {
        Some(b'y' | b't' | b'1') => Ok((true, None)),
        Some(b'n' | b'f' | b'0') => Ok((false, None)),
        _ => Err(Some(LayoutProblemKind::NotYesOrNo)),
    }
}

fn hold(text: &str) -> Parsed<Hold> {
    Hold::named(text)
        .map(|hold| (hold, None))
        .ok_or(Some(LayoutProblemKind::NotYet))
}

/// A word of `words`, without regard to capitals; another is refused as
/// `kind`.
fn one_of<'a, T: Copy>(
    words: &'a [(&'a str, T)],
    kind: LayoutProblemKind,
) -> impl Fn(&str) -> Parsed<T> + 'a {
    move |text| {
        words
            .iter()
            .find(|(word, _)| word.eq_ignore_ascii_case(text))
            .map(|&(_, value)| (value, None))
            .ok_or(Some(kind.clone()))
    }
}

/// A comma list of names, without empty names and the word for none.
fn names(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.eq_ignore_ascii_case(NO_PARTS))
        .map(str::to_owned)
        .collect()
}

fn list(text: &str) -> Parsed<Vec<String>> {
    Ok((names(text), None))
}

/// The list that names an added part: the sidebar's, the battlefield's, or
/// the `Parts=` of a row or a column the layout adds, by that group's name.
enum ListedIn {
    Side,
    Field,
    Group(String),
}

/// Read layout `layout` of `files` over position table `table`.
fn read_layout(files: &[&IniFile], layout: &str, table: &str) -> (MatchLayout, Vec<LayoutProblem>) {
    let mut tree_problems = Vec::new();
    let tree = Tree::read(files, layout, table, &mut tree_problems);
    let mut reader = Reader {
        tree: &tree,
        read: Vec::new(),
        problems: Vec::new(),
    };
    for problem in tree_problems {
        match problem {
            TreeProblem::MissingBase { section } => {
                reader.problem(&section, "Base", LayoutProblemKind::MissingLayout)
            }
            TreeProblem::Loop { section } => {
                reader.problem(&section, "Base", LayoutProblemKind::Loop)
            }
            TreeProblem::UnknownSide { section, key } => {
                reader.problem(&section, &key, LayoutProblemKind::UnknownKey)
            }
        }
    }
    let mut read = MatchLayout::default();
    for (index, number) in NUMBERS.iter().enumerate() {
        // A size may say `Rest` instead of a number: the screen's own.
        let value = reader.value(number.node, number.key, |text| {
            if text.eq_ignore_ascii_case("Rest") {
                return Err(None);
            }
            number_in(text, number.min, number.max)
        });
        if let Some(value) = value {
            read.numbers.push((index, value as f32));
        }
    }
    read.part_places = SidebarPart::STOCK_ORDER.map(|part| {
        ["X", "Y"].map(|key| reader.number(part.name(), key, -FAR).unwrap_or_default() as f32)
    });
    read_places(&mut reader, &mut read);
    read_field(&mut reader, &mut read.field);
    let mut listed: Vec<(ListedIn, String, String)> = Vec::new();
    read_parts(&mut reader, &mut read, &mut listed);
    read.added = read_added(&mut reader, listed).into();
    read_leftovers(&mut reader);
    read.colors = read_colors(files, &mut reader).into();
    let problems = reader.problems;
    read.tree = Arc::new(tree);
    read.side_text = read.side_colors(SidebarPart::Money, "Color");
    read.label_text = read.side_colors(SidebarPart::Cameos, "LabelColor");
    let themes = [
        SidebarTheme::Allied,
        SidebarTheme::Soviet,
        SidebarTheme::Yuri,
    ];
    read.commands = themes.map(|theme| {
        SidebarPart::STOCK_ORDER.map(|part| read.text(part, "Command", theme).map(str::to_owned))
    });
    read.money = themes.map(|theme| MoneyText {
        value: read
            .text(SidebarPart::Money, "Value", theme)
            .and_then(TextValue::named)
            .unwrap_or(TextValue::Credits),
        align: read
            .text(SidebarPart::Money, "Align", theme)
            .and_then(TextAlign::named)
            .unwrap_or_default(),
    });
    (read, problems)
}

/// Where the sidebar and the bar sit: `[Side] Hold=` (any edge) and
/// `Float=`, and `[Bar] Hold=` (`Top` or `Bottom`).
fn read_places(reader: &mut Reader, read: &mut MatchLayout) {
    use Edge::{Bottom, Left, Right, Top};
    let edges = [
        ("Left", Left),
        ("Right", Right),
        ("Top", Top),
        ("Bottom", Bottom),
    ];
    let places = &mut read.places;
    places.side = reader
        .value("Side", "Hold", one_of(&edges, LayoutProblemKind::NotYet))
        .unwrap_or(Right);
    places.side_floats = reader.value("Side", "Float", yes_no).unwrap_or(false);
    places.bar = reader.value(
        "Bar",
        "Hold",
        one_of(&edges[2..], LayoutProblemKind::NotYet),
    );
}

/// The pieces over the battlefield: `Messages`, `Timers` and `Banner` (a
/// box: `Hold`, `X`, `Y`, `Width`, `Height`), `Powers` (`X`, `Y`) and
/// `Tooltip` (`Hold`: `Cursor` or a corner, and `X`, `Y`).
fn read_field(reader: &mut Reader, field: &mut FieldLayout) {
    for (part, part_box) in [
        ("Messages", &mut field.messages),
        ("Timers", &mut field.timers),
        ("Banner", &mut field.banner),
    ] {
        *part_box = PartBox {
            hold: reader.value(part, "Hold", hold).unwrap_or_default(),
            x: reader.number(part, "X", -FAR).unwrap_or_default(),
            y: reader.number(part, "Y", -FAR).unwrap_or_default(),
            width: reader.value(part, "Width", size_or_rest).flatten(),
            height: reader.value(part, "Height", size_or_rest).flatten(),
        };
    }
    field.powers = ["X", "Y"].map(|key| reader.number("Powers", key, -FAR).unwrap_or_default());
    let cursor_or_hold = |text: &str| -> Parsed<Option<Hold>> {
        if text.eq_ignore_ascii_case("Cursor") {
            return Ok((None, None));
        }
        hold(text).map(|(hold, note)| (Some(hold), note))
    };
    field.tooltip = TooltipPlace {
        hold: reader.value("Tooltip", "Hold", cursor_or_hold).flatten(),
        x: reader.number("Tooltip", "X", -FAR).unwrap_or_default(),
        y: reader.number("Tooltip", "Y", -FAR).unwrap_or_default(),
    };
}

/// The `Parts=` lists: the screen's (the battlefield and the sidebar stay;
/// the bar may go), the sidebar's and its panels' (the parts the sidebar
/// draws, in order), the battlefield's (the pieces it shows). Each name
/// that is no part of the original goes to `listed`, with the list's
/// section.
fn read_parts(
    reader: &mut Reader,
    read: &mut MatchLayout,
    listed: &mut Vec<(ListedIn, String, String)>,
) {
    let screen = reader.pick("Screen", "Parts", |text| {
        let names = names(text);
        let has = |part: &str| names.iter().any(|name| name.eq_ignore_ascii_case(part));
        if !(has("Field") && has("Side")) {
            return Err(Some(LayoutProblemKind::NotYet));
        }
        let unknown = names.iter().any(|name| {
            !["Field", "Side", "Bar"]
                .iter()
                .any(|part| name.eq_ignore_ascii_case(part))
        });
        Ok((
            has("Bar"),
            unknown.then_some(LayoutProblemKind::UnknownPart),
        ))
    });
    if let Some((false, _)) = screen {
        read.places.bar = None;
    }

    // The sidebar's parts, each panel followed by the parts it holds.
    let mut order: Vec<SidebarPart> = Vec::new();
    let named = |name: &str| {
        SidebarPart::STOCK_ORDER
            .into_iter()
            .find(|part| part.name().eq_ignore_ascii_case(name))
    };
    // Each panel's own parts, in its list's order.
    let mut held: Vec<(SidebarPart, Vec<SidebarPart>)> = Vec::new();
    for panel in SidebarPart::STOCK_ORDER {
        if !SidebarPart::STOCK_ORDER
            .iter()
            .any(|part| part.panel() == Some(panel))
        {
            continue;
        }
        let Some((children, section)) = reader.pick(panel.name(), "Parts", list) else {
            continue;
        };
        let (mut parts, mut unknown, mut moved) = (Vec::new(), false, false);
        for child in children {
            match named(&child) {
                Some(child) if child.panel() == Some(panel) => {
                    if !parts.contains(&child) {
                        parts.push(child);
                    }
                }
                Some(_) => moved = true,
                None => unknown |= reader.tree.node(&child).is_none_or(|node| !node.built_in()),
            }
        }
        if let Some(section) = section {
            if unknown {
                reader.problem(&section, "Parts", LayoutProblemKind::UnknownPart);
            }
            if moved {
                reader.problem(&section, "Parts", LayoutProblemKind::NotYet);
            }
        }
        held.push((panel, parts));
    }
    let (side, section) = reader.pick("Side", "Parts", list).unwrap_or_default();
    let mut elsewhere = false;
    for name in side {
        match named(&name) {
            Some(part) if part.panel().is_none() => {
                if order.contains(&part) {
                    continue;
                }
                // The panel, then the parts it holds, each followed by its own.
                let mut next = vec![part];
                while let Some(part) = next.pop() {
                    order.push(part);
                    if let Some((_, parts)) = held.iter().find(|(panel, _)| *panel == part) {
                        next.extend(parts.iter().rev().filter(|child| !order.contains(child)));
                    }
                }
            }
            Some(_) => elsewhere = true,
            None => {
                if let Some(section) = &section {
                    listed.push((ListedIn::Side, name, section.clone()));
                }
            }
        }
    }
    if let (true, Some(section)) = (elsewhere, &section) {
        reader.problem(section, "Parts", LayoutProblemKind::NotYet);
    }
    let mut order = order.into_iter();
    read.parts = std::array::from_fn(|_| order.next());

    // The pieces the battlefield holds; one left out is not shown.
    let (field, section) = reader.pick("Field", "Parts", list).unwrap_or_default();
    read.field.shown = [false; 4];
    let mut tooltip = false;
    for name in field {
        match FieldPart::named(&name) {
            Some(part) => read.field.shown[part as usize] = true,
            None if name.eq_ignore_ascii_case("Tooltip") => tooltip = true,
            None => {
                if let Some(section) = &section {
                    listed.push((ListedIn::Field, name, section.clone()));
                }
            }
        }
    }
    if let (false, Some(section)) = (tooltip, &section) {
        reader.problem(section, "Parts", LayoutProblemKind::NotYet);
    }
}

/// The parts the layout adds: each node of a type a layout can add that no
/// built-in layout has, once a list names it, where its first list puts
/// it. The sidebar's and the battlefield's lists come first, so a group has
/// its place in the list before the parts it holds.
fn read_added(reader: &mut Reader, mut listed: Vec<(ListedIn, String, String)>) -> Vec<AddedPart> {
    let tree = reader.tree;
    let mut defined: Vec<(String, AddedPart)> = Vec::new();
    for node in tree.nodes.iter().filter(|node| !node.built_in()) {
        let section = node.section().unwrap_or_default().to_owned();
        let Some(kind) = node.text("Type") else {
            reader.problem(&section, "", LayoutProblemKind::UnknownPart);
            continue;
        };
        if !ADDED_TYPES
            .iter()
            .any(|added| added.eq_ignore_ascii_case(kind))
        {
            let known = TYPE_KEYS
                .iter()
                .any(|(known, _)| known.eq_ignore_ascii_case(kind));
            if known {
                reader.problem(&section, "Type", LayoutProblemKind::NotYet);
            } else {
                reader.problem(&section, "", LayoutProblemKind::UnknownPart);
            }
            continue;
        }
        if let Some(part) = read_added_part(reader, &node.name, kind, &section, &mut listed) {
            defined.push((node.name.clone(), part));
        }
    }
    let mut added: Vec<AddedPart> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for in_groups in [false, true] {
        for (list, word, section) in &listed {
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
                // A built-in part in a list of its own is no added part.
                if tree.node(word).is_none_or(|node| !node.built_in()) {
                    reader.problem(section, "Parts", LayoutProblemKind::UnknownPart);
                }
                continue;
            };
            // A group holds buttons and texts, not other groups, and build
            // pictures draw only in the sidebar.
            let kind = &defined[index].1.kind;
            let grid = matches!(kind, AddedKind::CameoGrid { .. });
            if (in_groups && (grid || matches!(kind, AddedKind::Group { .. })))
                || (grid && holder != Holder::Side)
            {
                reader.problem(section, "Parts", LayoutProblemKind::NotYet);
                continue;
            }
            let (name, part) = defined.remove(index);
            names.push(name);
            added.push(AddedPart { holder, ..part });
        }
    }
    for (_, part) in defined {
        reader.problem(&part.section, "", LayoutProblemKind::NotListed);
    }
    added
}

/// A part of type `kind` (one of [`ADDED_TYPES`]) the layout adds, in the
/// battlefield until a `Parts=` list says where it goes; a row's or a
/// column's own list goes to `listed`. `None` when it cannot be built (its
/// problem is reported).
fn read_added_part(
    reader: &mut Reader,
    node: &str,
    kind: &str,
    section: &str,
    listed: &mut Vec<(ListedIn, String, String)>,
) -> Option<AddedPart> {
    let is = |name: &str| kind.eq_ignore_ascii_case(name);
    let word = |text: &str| -> Parsed<String> { Ok((text.to_owned(), None)) };
    reader.read.push((
        reader
            .tree
            .nodes
            .iter()
            .position(|known| known.name == node)?,
        "Type",
    ));
    let place = PartBox {
        hold: reader.value(node, "Hold", hold).unwrap_or_default(),
        x: reader.number(node, "X", -FAR).unwrap_or_default(),
        y: reader.number(node, "Y", -FAR).unwrap_or_default(),
        width: reader.number(node, "Width", 1),
        height: reader.number(node, "Height", 1),
    };
    let kind = if is("Row") || is("Column") {
        if let Some((children, Some(section))) = reader.pick(node, "Parts", list) {
            listed.extend(
                children
                    .into_iter()
                    .map(|child| (ListedIn::Group(node.to_owned()), child, section.clone())),
            );
        }
        AddedKind::Group {
            column: is("Column"),
            gap: reader
                .value(node, "Gap", |text| number_in(text, 0, FAR))
                .unwrap_or(0),
        }
    } else if is("CameoGrid") {
        // The four tabs, by the names of the tab commands' strips.
        let tabs: Vec<(&str, crate::ui::sidebar::SidebarTab)> = GRID_TABS
            .into_iter()
            .zip(crate::ui::sidebar::SidebarTab::all())
            .collect();
        let tab = reader.value(node, "Tab", one_of(&tabs, LayoutProblemKind::UnknownValue));
        let columns = reader.value(node, "Columns", |text| number_in(text, 1, 8));
        let rows = reader.value(node, "Rows", |text| number_in(text, 1, 60));
        // A grid shows one tab; without a tab it has nothing to show.
        let Some(tab) = tab else {
            reader.problem(section, "Tab", LayoutProblemKind::UnknownValue);
            return None;
        };
        AddedKind::CameoGrid {
            tab,
            columns: columns.map(|columns| columns as usize),
            rows: rows.map_or(2, |rows| rows as usize),
        }
    } else if is("Button") {
        let command = reader.value(node, "Command", word);
        let label = reader.value(node, "Label", word);
        let art = reader.value(node, "Art", word);
        // A button with a label or a picture and no command only tells the
        // client modules of its clicks (RFC 0001 section 10).
        if command.is_none() && label.is_none() && art.is_none() {
            reader.problem(section, "Command", LayoutProblemKind::UnknownCommand);
            return None;
        }
        AddedKind::Button {
            label: label.or_else(|| command.clone()).unwrap_or_default(),
            art,
            command,
        }
    } else if is("Image") {
        let Some(art) = reader.value(node, "Art", word) else {
            reader.problem(section, "Art", LayoutProblemKind::UnknownValue);
            return None;
        };
        AddedKind::Image { art }
    } else if is("Bar") {
        AddedKind::Bar
    } else if is("List") {
        AddedKind::List
    } else {
        let values: Vec<(&str, TextValue)> = TextValue::ALL
            .into_iter()
            .map(|value| (value.name(), value))
            .collect();
        let value = reader.value(
            node,
            "Value",
            one_of(&values, LayoutProblemKind::UnknownValue),
        );
        // `Label=` in place of `Value=`: a text a client module may change.
        let label = reader.value(node, "Label", word);
        let shows = match (value, label) {
            (Some(value), _) => TextShows::Value(value),
            (None, Some(label)) => TextShows::Label(label),
            (None, None) => {
                reader.problem(section, "Value", LayoutProblemKind::UnknownValue);
                return None;
            }
        };
        AddedKind::Text { shows }
    };
    Some(AddedPart {
        section: section.to_owned(),
        holder: Holder::Field,
        kind,
        place,
    })
}

/// The keys of a built-in sidebar part that its own code reads from the layout once
/// the file is loaded ([`MatchLayout::text`]), rather than this reader.
const STOCK_KEYS: [&str; 12] = [
    "Art",
    "RowArt",
    "EmptyRowArt",
    "EndArt",
    "FootArt",
    "Palette",
    "Frames",
    "Command",
    "Value",
    "Align",
    "Color",
    "LabelColor",
];

/// `[Colors]` of `files`, the bottom layer first: each line's name, the side
/// it holds for (`Name@Side`) and its colour. A line that is no colour is a
/// problem and keeps the one under it.
fn read_colors(files: &[&IniFile], reader: &mut Reader) -> Vec<(String, Option<usize>, String)> {
    const SIDES: [&str; 3] = ["Allied", "Soviet", "Yuri"];
    let mut colors = Vec::new();
    for (layer, ini) in files.iter().enumerate() {
        let Some(section) = find_section(ini, "Colors") else {
            continue;
        };
        for key in section.keys() {
            let text = section
                .read_name(key, NAME_CAPACITY)
                .unwrap_or_default()
                .trim();
            let (name, side) = match key.split_once('@') {
                None => (key, None),
                Some((name, side)) => {
                    match SIDES
                        .iter()
                        .position(|known| known.eq_ignore_ascii_case(side))
                    {
                        Some(side) => (name, Some(side)),
                        None => {
                            reader.problem("Colors", key, LayoutProblemKind::UnknownKey);
                            continue;
                        }
                    }
                }
            };
            if channels(text).is_none() {
                if layer > 0 {
                    reader.problem("Colors", key, LayoutProblemKind::NotANumber);
                }
                continue;
            }
            colors.push((name.trim().to_owned(), side, text.to_owned()));
        }
    }
    colors
}

/// `r,g,b` or `r,g,b,a`, each a whole number to 255.
fn channels(text: &str) -> Option<[u8; 4]> {
    let channels: Vec<u8> = text
        .split(',')
        .map(|channel| channel.trim().parse().ok())
        .collect::<Option<_>>()?;
    match channels[..] {
        [r, g, b] => Some([r, g, b, 255]),
        [r, g, b, a] => Some([r, g, b, a]),
        _ => None,
    }
}

/// Every value a file wrote that nothing read: a key the part's type does
/// not have is unknown; one it has, that this version cannot change yet,
/// unless it says what the built-in file says.
fn read_leftovers(reader: &mut Reader) {
    let tree = reader.tree;
    for (index, node) in tree.nodes.iter().enumerate() {
        let Some(keys) = node
            .text("Type")
            .and_then(|kind| {
                TYPE_KEYS
                    .iter()
                    .find(|(known, _)| known.eq_ignore_ascii_case(kind))
            })
            .map(|(_, keys)| *keys)
        else {
            continue;
        };
        for (key, values) in &node.keys {
            let known = COMMON_KEYS
                .iter()
                .chain(keys)
                .any(|known| known.eq_ignore_ascii_case(key));
            let read = reader
                .read
                .iter()
                .any(|&(read, name)| read == index && name.eq_ignore_ascii_case(key));
            for (at, value) in values.iter().enumerate().filter(|(_, value)| value.own) {
                let stock = node.built_in()
                    && SidebarPart::STOCK_ORDER
                        .iter()
                        .any(|part| part.name().eq_ignore_ascii_case(&node.name))
                    && STOCK_KEYS
                        .iter()
                        .any(|stock| stock.eq_ignore_ascii_case(key))
                    // A key the built-in file gives the part, not one it lacks.
                    && values.iter().any(|under| !under.own);
                if key.eq_ignore_ascii_case("Base") || stock || (read && value.theme.is_none()) {
                    continue;
                }
                if !known {
                    reader.problem(&value.section, key, LayoutProblemKind::UnknownKey);
                    continue;
                }
                let built_in = values[..at]
                    .iter()
                    .rev()
                    .find(|under| !under.own && under.theme == value.theme);
                if built_in.is_none_or(|under| !under.text.eq_ignore_ascii_case(&value.text)) {
                    reader.problem(&value.section, key, LayoutProblemKind::NotYet);
                }
            }
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::rules::object_type::ObjectCategory;
    use crate::sim::intern::StringInterner;
    use crate::sim::production::{BuildOption, ProductionCategory};
    use crate::ui::sidebar::cameo_order::{CameoStrips, StripColumns};
    use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
    use crate::ui::sidebar::{
        SidebarFrame, SidebarTab, SidebarTheme, SidebarView, build_sidebar_view_with_spec,
        power_bar_rect,
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

    /// Each problem as `([Section] Key, kind)`, sorted: the reader's order
    /// is not part of what it promises.
    fn problems(file: &MatchLayoutFile) -> Vec<(String, LayoutProblemKind)> {
        let mut problems: Vec<_> = file
            .problems()
            .iter()
            .map(|problem| (problem.place(), problem.kind.clone()))
            .collect();
        problems.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        problems
    }

    fn expect(expected: &[(&str, LayoutProblemKind)]) -> Vec<(String, LayoutProblemKind)> {
        let mut expected: Vec<_> = expected
            .iter()
            .map(|(place, kind)| ((*place).to_owned(), kind.clone()))
            .collect();
        expected.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        expected
    }

    /// `spec` with each of `places` written over its part's place.
    fn placed(
        mut spec: SidebarChromeLayoutSpec,
        places: &[(SidebarPart, [f32; 2])],
    ) -> SidebarChromeLayoutSpec {
        for &(part, place) in places {
            spec.part_places[part as usize] = place;
        }
        spec
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
            SidebarFrame::for_layout(800, 600, 100, &spec),
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
        // Keys that repeat the original (`Parts`, `type`, `Art`) are
        // accepted, capitals do not count, and the last line has no break.
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nNod=Steel\nDefault=Plain\n\
              [Steel.Screen]\nParts=Field,Side,Bar\n\
              [Steel.Middle]\nY=170\n\
              [Steel.Defenses]\nX=40\n[Steel.Infantry]\nX=80\n[Steel.Vehicles]\nX=120\n\
              [Steel.Radar]\ntype=radar\nArt=radar.shp\n\
              [Steel.Minimap]\nX=10\nWidth=120\n\
              [Steel.Cameos]\nCellWidth=60\nRowHeight=60\n\
              [Plain.Power]\nX=9",
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());

        let (spec, view) = sidebar(SidebarTheme::Soviet, file.for_side(Some("nod"), false));
        // SIDE1 moved from 158 to 170 and took Repair and the tabs with it.
        assert_eq!(view.repair_button.rect.y, 177.0);
        assert_eq!((view.tabs[0].rect.x, view.tabs[0].rect.y), (652.0, 209.0));
        assert_eq!(view.tabs[1].rect.x, 692.0);
        let radar = view.layout.minimap_rect(&spec);
        // MapX and MapWidth are the file's; y keeps every side's MapAreaY 49
        // (`RadarClass::One_Time @ 0x00652CF0`).
        assert_eq!(
            (radar.x, radar.y, radar.w, radar.h),
            (642.0, 49.0, 120.0, 108.0)
        );
        // (600 - 227 - 25) / 60 rows of 60.
        assert_eq!(view.layout.side2_tile_count, 5);
        assert_eq!(view.layout.cameo_grid_bottom, 527.0);

        // A side without a line takes `Default=`; no file changes nothing.
        let (plain, _) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI"), true));
        let stock = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        assert_eq!(plain, placed(stock, &[(SidebarPart::Power, [9.0, 227.0])]));
        assert!(
            MatchLayoutFile::none()
                .for_side(Some("GDI"), true)
                .is_none()
        );
        assert!(MatchLayoutFile::none().is_original() && !file.is_original());
    }

    #[test]
    fn a_broken_file_keeps_the_original_and_names_each_line() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Broken\nNod=Missing\nThirdSide=Looped\n\
              [Broken]\nColour=red\nVersion=1\n\
              [Broken.Screen]\nParts=Side,Clock,Field,Bar\n\
              [Broken.Minimap]\nWidth=wide\nHeight=0\n\
              [Broken.Radar]\nHold=Left\nSpeed=3\nArt@Martian=x.shp\nX@Yuri=4\n\
              [Broken.Cameos]\nRowHeight=1\nCellHeight=x\nColumns=0\n\
              [Broken.Clock]\nX=4\n\
              [Broken.Infantry]\nX=50\n\
              [Broken.Sell]\nBase=Nothing\n\
              [Looped]\nBase=Round\n[Round]\nBase=Looped\n[Round.Radar]\nY=10\n",
        );
        use LayoutProblemKind::*;
        assert_eq!(
            problems(&file),
            expect(&[
                ("[Broken] Colour", UnknownKey),
                ("[Broken] Version", UnknownKey),
                ("[Broken.Screen] Parts", UnknownPart),
                ("[Broken.Minimap] Width", NotANumber),
                ("[Broken.Minimap] Height", Clamped { used: 1 }),
                ("[Broken.Radar] Hold", NotYet),
                ("[Broken.Radar] Speed", UnknownKey),
                ("[Broken.Radar] Art", UnknownKey),
                // A side's own place is not there yet.
                ("[Broken.Radar] X", NotYet),
                ("[Broken.Cameos] RowHeight", Clamped { used: 16 }),
                ("[Broken.Cameos] CellHeight", NotANumber),
                ("[Broken.Cameos] Columns", Clamped { used: 1 }),
                ("[Broken.Clock]", UnknownPart),
                ("[Broken.Sell] Base", MissingLayout),
                ("[Layouts] Nod", MissingLayout),
                ("[Round] Base", Loop),
            ])
        );

        // The lines that could be read still apply; the rest stay original.
        let (spec, _) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI"), true));
        let read = SidebarChromeLayoutSpec {
            radar_height: 1.0,
            cameo_row_height: 16.0,
            cameo_columns: 1.0,
            ..SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied)
        };
        assert_eq!(spec, placed(read, &[(SidebarPart::Infantry, [50.0, 0.0])]));
        // A missing layout leaves the side alone; a loop of bases still
        // starts from the table.
        assert!(file.for_side(Some("Nod"), false).is_none());
        let (spec, _) = sidebar(SidebarTheme::Yuri, file.for_side(Some("ThirdSide"), false));
        assert_eq!(
            spec,
            placed(
                SidebarChromeLayoutSpec::for_theme(SidebarTheme::Yuri),
                &[(SidebarPart::Radar, [0.0, 10.0])]
            )
        );
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
        assert_eq!(
            problems(&file),
            expect(&[
                ("[Bare.Top] Parts", LayoutProblemKind::UnknownPart),
                ("[Bare.Top] Parts", LayoutProblemKind::NotYet),
            ])
        );

        use SidebarPart::*;
        let (stock_spec, stock) = sidebar(SidebarTheme::Allied, None);
        let (spec, view) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI"), true));
        // The strip's part Top is not in Side's list, so its Options goes too.
        assert_eq!(
            spec.parts_in_order().collect::<Vec<_>>(),
            [
                Radar, Minimap, Cameos, ScrollDown, ScrollUp, Middle, Tabs, Buildings, Defenses,
                Infantry, Vehicles
            ]
        );
        // A part that stays keeps its place; one left out takes no click.
        assert_eq!(view.tabs, stock.tabs);
        assert_eq!(view.scroll_down_button.rect, stock.scroll_down_button.rect);
        assert_eq!(
            view.layout.minimap_rect(&spec),
            stock.layout.minimap_rect(&stock_spec)
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
        let (spec, _) = sidebar(SidebarTheme::Soviet, file.for_side(Some("Nod"), false));
        assert!(spec.shows(Top) && !spec.shows(Diplomacy) && !spec.shows(Options));
        assert!(spec.shows(Credits) && spec.shows(Money) && spec.shows(Power));
    }

    #[test]
    fn a_layout_sets_the_cameo_columns_for_rows_and_scrolling() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Wide\n[Wide.Cameos]\nColumns=3\nColumnWidth=40\n",
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        let (spec, view) = sidebar_with(SidebarTheme::Allied, file.for_side(Some("GDI"), true), 7);
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
        let (spec, view) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI"), true));
        assert_eq!((view.panel_rect.x, view.panel_rect.w), (600.0, 200.0));
        // The frame gives the battlefield and the bottom bar what is left.
        let frame = SidebarFrame::for_layout(800, 600, 100, &spec);
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
              [Over.Screen]\nParts=Field,Side\n[Over.Side]\nFloat=yes\nHold=Middle\n\
              [Over.Bar]\nHold=Left\n[Over.Radar]\nFloat=maybe\n",
        );
        // A sidebar in the middle and a bar on the left are not there yet,
        // and only the sidebar can float.
        assert_eq!(
            problems(&file),
            expect(&[
                ("[Over.Side] Hold", LayoutProblemKind::NotYet),
                ("[Over.Bar] Hold", LayoutProblemKind::NotYet),
                ("[Over.Radar] Float", LayoutProblemKind::UnknownKey),
            ])
        );

        let (spec, view) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI"), true));
        let frame = SidebarFrame::for_layout(800, 600, 100, &spec);
        // The sidebar's parts follow it to the left edge, and the battlefield
        // starts beside it and under the bar.
        assert_eq!((view.panel_rect.x, view.tabs[0].rect.x), (0.0, 26.0));
        assert_eq!(view.layout.minimap_rect(&spec).x, 16.0);
        assert_eq!(frame.tactical_rect(), (168, 32, 632, 568));

        // A floating sidebar and no bar leave the battlefield the whole frame.
        let (spec, _) = sidebar(SidebarTheme::Soviet, file.for_side(Some("Nod"), false));
        let frame = SidebarFrame::for_layout(800, 600, 100, &spec);
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
        file.for_side(Some("GDI"), true)
            .unwrap()
            .apply_to_sidebar(&mut spec);
        let frame = SidebarFrame::for_layout(800, 600, 100, &spec);
        assert_eq!(frame.column(), (0, 440, 800, 160));
        assert_eq!(frame.tactical_rect(), (0, 0, 800, 408));
        // The parts lay out in the band: it starts at the screen's left
        // edge, and its rows are the ones that fit in its own height.
        let layout = crate::ui::sidebar::compute_layout_with_spec(spec, frame);
        assert_eq!(layout.sidebar_x, 0.0);
        assert_eq!(layout.origin(&spec, SidebarPart::Cameos)[0], 336.0);
        assert_eq!(layout.side2_tile_count, (160 - 33) / 50);
        assert_eq!(frame.surface_size().0, 800);
    }

    #[test]
    fn a_layout_places_the_pieces_over_the_battlefield() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Hud\n\
              [Hud.Field]\nParts=Messages,Timers,Powers,Tooltip\n\
              [Hud.Messages]\nX=200\nWidth=300\n\
              [Hud.Timers]\nhold=topright\nY=8\nHeight=120\n\
              [Hud.Tooltip]\nHold=BottomLeft\nX=8\nY=4\n\
              [Hud.Powers]\nX=6\nSpeed=2\n\
              [Hud.Banner]\nHold=Sideways\nWidth=wide\nHeight=Rest\nArt=x.shp\n",
        );
        assert_eq!(
            problems(&file),
            expect(&[
                ("[Hud.Powers] Speed", LayoutProblemKind::UnknownKey),
                ("[Hud.Banner] Hold", LayoutProblemKind::NotYet),
                ("[Hud.Banner] Width", LayoutProblemKind::NotANumber),
                ("[Hud.Banner] Art", LayoutProblemKind::NotYet),
            ])
        );
        let mut spec = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        assert_eq!(spec.field, FieldLayout::ORIGINAL);
        file.for_side(Some("GDI"), true)
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

    /// The parts a client module drives: a button with no command, a text
    /// with a label, a picture, a bar and a list.
    #[test]
    fn a_layout_adds_the_parts_client_modules_drive() {
        let mut file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Hud\n\
              [Hud.Field]\nParts=Messages,Timers,Banner,Powers,Tooltip,Buy,Note,Logo,Charge,Log,Mute,Empty\n\
              [Hud.Buy]\nType=Button\nLabel=Buy\n\
              [Hud.Logo]\nType=Image\nArt=logo.shp\nX=4\n\
              [Hud.Note]\nType=Text\nLabel=GUI:Hello\n\
              [Hud.Charge]\nType=Bar\nWidth=80\nHeight=6\n\
              [Hud.Log]\nType=List\nWidth=200\nHeight=60\n\
              [Hud.Mute]\nType=Button\nArt=mute.shp\n\
              [Hud.Empty]\nType=Button\n",
        );
        file.check_commands(|_| false);
        assert_eq!(
            problems(&file),
            expect(&[
                // A button with no command, label or picture is refused, and
                // its list names a part that is not there.
                ("[Hud.Empty] Command", LayoutProblemKind::UnknownCommand),
                ("[Hud.Field] Parts", LayoutProblemKind::UnknownPart),
            ])
        );
        assert_eq!(file.art_names(), ["logo.shp", "mute.shp"]);
        let kinds: Vec<AddedKind> = file
            .for_side(Some("GDI"), true)
            .unwrap()
            .added()
            .iter()
            .map(|part| part.kind.clone())
            .collect();
        assert_eq!(
            kinds,
            [
                AddedKind::Button {
                    command: None,
                    label: "Buy".to_owned(),
                    art: None,
                },
                AddedKind::Text {
                    shows: TextShows::Label("GUI:Hello".to_owned()),
                },
                AddedKind::Image {
                    art: "logo.shp".to_owned(),
                },
                AddedKind::Bar,
                AddedKind::List,
                AddedKind::Button {
                    command: None,
                    label: String::new(),
                    art: Some("mute.shp".to_owned()),
                },
            ]
        );
    }

    /// A map's `[VeraUI]` parts: the top-level list, a row's own, the keys
    /// of added parts, and the commands the game has.
    #[test]
    fn a_map_adds_its_own_parts() {
        let map = IniFile::from_str(
            "[Basic]\nName=Test\n\
             [VeraUI]\nParts=Shop,Score\n\
             [VeraUI.Shop]\nType=Row\nHold=Bottom\nGap=2\nParts=Buy,Stop\n\
             [VeraUI.Buy]\nType=Button\nLabel=Buy\n\
             [VeraUI.Stop]\nType=Button\nCommand=StopObject\n\
             [VeraUI.Fly]\nType=Button\nCommand=Levitate\n\
             [VeraUI.Score]\nType=Text\nValue=Kills\nHold=TopRight\nSpeed=2\n",
        );
        let (parts, problems) = map_parts(&map, |name| name.eq_ignore_ascii_case("StopObject"));
        let places: Vec<String> = problems.iter().map(LayoutProblem::place).collect();
        assert_eq!(places, ["[VeraUI.Fly]", "[VeraUI.Score] Speed"]);
        let [shop, score, buy, stop] = &parts[..] else {
            panic!("four parts: {parts:?}");
        };
        assert_eq!(shop.holder, Holder::Field);
        assert_eq!(
            shop.kind,
            AddedKind::Group {
                column: false,
                gap: 2
            }
        );
        assert_eq!(
            score.kind,
            AddedKind::Text {
                shows: TextShows::Value(TextValue::Kills)
            }
        );
        assert_eq!(
            (buy.holder, stop.holder),
            (Holder::Group(0), Holder::Group(0))
        );
        assert_eq!(buy.section, "VeraUI.Buy");
        assert_eq!(
            stop.kind,
            AddedKind::Button {
                command: Some("StopObject".to_owned()),
                label: "StopObject".to_owned(),
                art: None,
            }
        );
        // A map without the sections adds nothing.
        let (none, problems) = map_parts(&IniFile::from_str("[Basic]\n"), |_| true);
        assert!(none.is_empty() && problems.is_empty());
    }

    #[test]
    fn a_layout_adds_buttons_and_texts_to_the_sidebar_and_the_battlefield() {
        let mut file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Hud\n\
              [Hud.Side]\nParts=Credits,Top,Radar,Middle,Cameos,Power,Halt,Missing\n\
              [Hud.Field]\nParts=Messages,Timers,Banner,Powers,Tooltip,Clock,Fly\n\
              [Hud.Halt]\nType=Button\nCommand=StopObject\nLabel=Stop\nArt=button08.shp\nX=10\nY=300\n\
              [Hud.Clock]\ntype=text\nValue=time\nHold=TopRight\nX=6\nY=6\n\
              [Hud.Fly]\nType=Button\nCommand=Levitate\n\
              [Hud.Loose]\nType=Text\nValue=Credits\n\
              [Hud.Gold]\nType=Text\nValue=Gold\n\
              [Hud.Dish]\nType=Radar\n",
        );
        // The input code owns the command names; the loader asks it.
        file.check_commands(|name| name.eq_ignore_ascii_case("StopObject"));
        assert_eq!(
            problems(&file),
            expect(&[
                ("[Hud.Gold] Value", LayoutProblemKind::UnknownValue),
                // A radar of its own is a type a layout cannot add yet.
                ("[Hud.Dish] Type", LayoutProblemKind::NotYet),
                ("[Hud.Side] Parts", LayoutProblemKind::UnknownPart),
                ("[Hud.Loose]", LayoutProblemKind::NotListed),
                ("[Hud.Fly] Command", LayoutProblemKind::UnknownCommand),
            ])
        );
        assert_eq!(file.art_names(), ["button08.shp"]);
        let layout = file.for_side(Some("GDI"), true).unwrap();
        let [halt, clock] = &layout.added()[..] else {
            panic!("two parts are added: {:?}", layout.added());
        };
        assert_eq!(halt.holder, Holder::Side);
        assert_eq!(
            halt.kind,
            AddedKind::Button {
                command: Some("StopObject".to_owned()),
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
                shows: TextShows::Value(TextValue::Time)
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
              [Hud.Field]\nParts=Messages,Timers,Banner,Powers,Tooltip,Bad,Orders\n\
              [Hud.Bad]\nType=Button\nCommand=Levitate\n\
              [Hud.Orders]\nType=row\nHold=Bottom\nY=4\nGap=2\nParts=Halt,Fly,Guard,Orders,Ghost\n\
              [Hud.Halt]\nType=Button\nCommand=Stop\n\
              [Hud.Fly]\nType=Button\nCommand=Levitate\n\
              [Hud.Guard]\nType=Button\nCommand=Guard\n\
              [Hud.Stray]\nType=Column\nParts=Lone\n\
              [Hud.Lone]\nType=Text\nValue=Time\n",
        );
        file.check_commands(|name| !name.eq_ignore_ascii_case("Levitate"));
        assert_eq!(
            problems(&file),
            expect(&[
                // A row can't hold itself, and Ghost has no section.
                ("[Hud.Orders] Parts", LayoutProblemKind::UnknownPart),
                // A column no list names shows nothing, nor does its text.
                ("[Hud.Stray]", LayoutProblemKind::NotListed),
                ("[Hud.Lone]", LayoutProblemKind::NotListed),
                ("[Hud.Bad] Command", LayoutProblemKind::UnknownCommand),
                ("[Hud.Fly] Command", LayoutProblemKind::UnknownCommand),
            ])
        );
        // The row and its two real buttons are left, and the buttons still
        // name the row after the refused button before it is gone.
        let added = file.for_side(Some("GDI"), true).unwrap().added();
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
              [Wide.Field]\nParts=Messages,Timers,Banner,Powers,Tooltip,Stray\n\
              [Wide.More]\nType=CameoGrid\nTab=building\nColumns=3\nRows=1\nX=0\nY=500\n\
              [Wide.Lost]\nType=CameoGrid\nRows=2\n\
              [Wide.Stray]\nType=CameoGrid\nTab=Vehicle\n",
        );
        assert_eq!(
            problems(&file),
            expect(&[
                // A grid needs a tab, and draws only in the sidebar.
                ("[Wide.Lost] Tab", LayoutProblemKind::UnknownValue),
                ("[Wide.Side] Parts", LayoutProblemKind::UnknownPart),
                ("[Wide.Field] Parts", LayoutProblemKind::NotYet),
                ("[Wide.Stray]", LayoutProblemKind::NotListed),
            ])
        );
        let added = file.for_side(Some("GDI"), true).unwrap().added();
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
            SidebarFrame::native(800, 600),
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
            "[Layouts]\nNod=Low\n[Low.Radar]\nX=4\nY=60\n\
             [Low.Defenses]\nX=40\n[Low.Infantry]\nX=80\n[Low.Vehicles]\nX=120\n\
             [Low.Field]\nParts=Messages,Timers,Banner,Powers,Tooltip,Halt\n\
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
        file.for_side(Some("Nod"), false)
            .unwrap()
            .apply_to_sidebar(&mut soviet);
        // The radar keeps the mod's X and takes the player's Y; the tabs
        // keep the mod's places.
        assert_eq!(soviet.place(SidebarPart::Radar), [4.0, 70.0]);
        assert_eq!(
            SidebarPart::TABS.map(|tab| soviet.place(tab)[0]),
            [0.0, 40.0, 80.0, 120.0]
        );
        let halt = &file.for_side(Some("Nod"), false).unwrap().added()[0];
        assert_eq!((halt.place.x, halt.place.y), (300, 20));
        // The Allies get the player's own layout.
        assert_eq!(file.layout_name_for_side(Some("GDI")), Some("OwnGDI"));
        let mut allied = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        file.for_side(Some("GDI"), true)
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
        let (spec, view) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI"), true));
        let frame = SidebarFrame::for_layout(800, 600, 100, &spec);
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
        file.for_side(Some("Nod"), false)
            .unwrap()
            .apply_to_sidebar(&mut spec);
        let frame = SidebarFrame::for_layout(800, 600, 100, &spec);
        assert_eq!(frame.tactical_rect(), (0, 0, 800, 408));
        let layout = crate::ui::sidebar::compute_layout_with_spec(spec, frame);
        assert_eq!(
            [SidebarPart::Radar, SidebarPart::Middle, SidebarPart::Cameos]
                .map(|part| layout.origin(&spec, part)[0]),
            [0.0, 168.0, 336.0]
        );
        assert_eq!(layout.side2_tile_count, 2);
        // Two buttons and the match clock sit in the band's empty half.
        let added = file.for_side(Some("Nod"), false).unwrap().added();
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
                shows: TextShows::Value(TextValue::Time)
            }
        );
        assert!(
            added[3..]
                .iter()
                .all(|part| part.holder == Holder::Group(0))
        );
        // Yuri's is wider, three cameos to a row, with an empty top strip.
        let (spec, view) = sidebar_with(
            SidebarTheme::Yuri,
            file.for_side(Some("ThirdSide"), false),
            4,
        );
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

    #[test]
    fn a_reset_takes_the_sides_layout_and_the_lines_naming_it_out_of_the_file() {
        // The player's own Soviet layout, also named by `Default=`, and an
        // Allied one that stays.
        let text = b"; mine\r\n[Layouts]\r\nNod=OwnNod\r\nDefault=ownnod\r\nGDI=OwnGDI\r\n\
                     [OwnNod]\r\nBase=Nod\r\n[OwnNod.Radar]\r\nX=-3\r\nY=35\r\n\
                     [OwnNodTwo.Radar]\r\nX=1\r\n[OwnGDI.Radar]\r\nY=52";
        let reset = MatchLayoutFile::without_layout(text, "OwnNod");
        assert_eq!(
            String::from_utf8(reset.clone()).unwrap(),
            "; mine\r\n[Layouts]\r\nGDI=OwnGDI\r\n[OwnNodTwo.Radar]\r\nX=1\r\n[OwnGDI.Radar]\r\nY=52"
        );
        let file = MatchLayoutFile::from_bytes(&reset);
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        assert_eq!(file.layout_name_for_side(Some("Nod")), None);
        assert_eq!(file.layout_name_for_side(Some("GDI")), Some("OwnGDI"));
        // Once no side names a layout, `[Layouts]` goes too: nothing is left.
        let text = b"[Layouts]\nNod=OwnNod\n[OwnNod.Radar]\nX=-3\n[OwnNod.Power]\nY=194\n";
        assert_eq!(MatchLayoutFile::without_layout(text, "OwnNod"), b"");
    }

    /// The built-in file is a layout file like any other: read as a mod's,
    /// every line of it is one this version reads, and it gives the
    /// original interface.
    #[test]
    fn the_built_in_file_reads_cleanly_as_a_mods() {
        let text = format!(
            "{}\n[Layouts]\nGDI=GDI\nNod=Nod\n",
            String::from_utf8_lossy(include_bytes!("veraui_default.ini"))
        );
        let file = MatchLayoutFile::from_bytes(text.as_bytes());
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        for (side, gdi, theme) in [
            ("GDI", true, SidebarTheme::Allied),
            ("Nod", false, SidebarTheme::Soviet),
        ] {
            let (spec, _) = sidebar(theme, file.for_side(Some(side), gdi));
            assert_eq!(spec, SidebarChromeLayoutSpec::for_theme(theme));
        }
    }

    /// The two tables hold the numbers the original's code has
    /// (0x006A5090, 0x006A5130, 0x00652E50), as the format before the tree
    /// wrote them.
    #[test]
    fn the_position_tables_keep_the_originals_numbers() {
        let numbers = |spec: SidebarChromeLayoutSpec| {
            [
                spec.sidebar_width,
                spec.side_height,
                spec.radar_width,
                spec.radar_height,
                spec.cameo_columns,
                spec.cameo_width,
                spec.cameo_height,
                spec.cameo_inset_x,
                spec.cameo_inset_y,
                spec.cameo_column_pitch,
                spec.cameo_row_height,
                spec.strip_footer,
                spec.power_bar_tile_height,
                spec.cameo_cell_source_width,
            ]
        };
        let gdi = [
            168., 168., 140., 108., 2., 60., 48., 22., 1., 63., 50., 33., 3., 63.,
        ];
        let nod = [
            168., 168., 140., 108., 2., 60., 48., 22., 1., 64., 50., 25., 3., 64.,
        ];
        // Each part's place, in `SidebarPart::STOCK_ORDER`.
        let gdi_places = [
            [0., 0.],
            [0., 2.],
            [0., 16.],
            [11., 4.],
            [83., 4.],
            [0., 48.],
            [16., 1.],
            [0., 158.],
            [20., 8.],
            [84., 8.],
            [26., 39.],
            [0., 0.],
            [29., 0.],
            [58., 0.],
            [87., 0.],
            [0., 227.],
            [39., 7.],
            [85., 7.],
            [5., 227.],
        ];
        let nod_places = [
            [0., 0.],
            [0., 2.],
            [0., 16.],
            [14., 5.],
            [86., 5.],
            [0., 48.],
            [16., 1.],
            [0., 158.],
            [33., 7.],
            [85., 7.],
            [20., 39.],
            [0., 0.],
            [32., 0.],
            [64., 0.],
            [96., 0.],
            [0., 227.],
            [39., 7.],
            [84., 7.],
            [0., 227.],
        ];
        for theme in [
            SidebarTheme::Allied,
            SidebarTheme::Soviet,
            SidebarTheme::Yuri,
        ] {
            for (gdi_positions, expected, places) in
                [(true, gdi, gdi_places), (false, nod, nod_places)]
            {
                let spec = SidebarChromeLayoutSpec::for_theme_and_positions(theme, gdi_positions);
                assert_eq!(numbers(spec), expected, "{theme:?} {gdi_positions}");
                assert_eq!(
                    SidebarPart::STOCK_ORDER.map(|part| spec.place(part)),
                    places,
                    "{theme:?} {gdi_positions}"
                );
                assert_eq!(spec.parts, SidebarPart::STOCK_ORDER.map(Some));
                assert_eq!(spec.places, PanelPlaces::ORIGINAL);
                assert_eq!(spec.field, FieldLayout::ORIGINAL);
            }
        }
    }

    /// A layout starts from its `Base=` layout, and a part from its `Base=`
    /// part: what it does not write comes from there.
    #[test]
    fn a_layout_and_a_part_start_from_their_bases() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Child\n\
              [Child]\nBase=Parent\n\
              [Parent.Repair]\nX=1\nY=12\n[Parent.Radar]\nY=60\n\
              [Child.Radar]\nY=70\n\
              [Child.Clock]\nType=Text\nValue=Time\nX=4\nY=5\n\
              [Child.Kills]\nBase=Clock\nValue=Kills\nY=20\n\
              [Child.Field]\nParts=Messages,Timers,Banner,Powers,Tooltip,Clock,Kills\n",
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        let (spec, _) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI"), true));
        // The child's own Y over the parent's, and the parent's Repair. Sell,
        // whose base is Repair, has its own place.
        assert_eq!(
            [SidebarPart::Radar, SidebarPart::Repair, SidebarPart::Sell]
                .map(|part| spec.place(part)),
            [[0.0, 70.0], [1.0, 12.0], [84.0, 8.0]]
        );
        let added = file.for_side(Some("GDI"), true).unwrap().added();
        assert_eq!(
            added
                .iter()
                .map(|part| (part.kind.clone(), part.place.x, part.place.y))
                .collect::<Vec<_>>(),
            [
                (
                    AddedKind::Text {
                        shows: TextShows::Value(TextValue::Time)
                    },
                    4,
                    5
                ),
                (
                    AddedKind::Text {
                        shows: TextShows::Value(TextValue::Kills)
                    },
                    4,
                    20
                ),
            ]
        );
    }

    #[test]
    fn a_deep_layout_chain_keeps_the_side_table_and_reports_the_cutoff() {
        use std::fmt::Write;

        let mut ini = String::from("[Layouts]\nGDI=Level0\n[Level0.Radar]\nY=70\n");
        for level in 0..18 {
            writeln!(ini, "[Level{level}]\nBase=Level{}", level + 1).unwrap();
        }
        let file = MatchLayoutFile::from_bytes(ini.as_bytes());
        assert_eq!(
            problems(&file),
            expect(&[("[Level16] Base", LayoutProblemKind::Loop)])
        );

        for (gdi, diplomacy_x) in [(true, "11"), (false, "14")] {
            let layout = file.for_side(Some("GDI"), gdi).unwrap();
            let text = |part, key| layout.text(part, key, SidebarTheme::Allied);
            assert_eq!(text(SidebarPart::Radar, "Y"), Some("70"));
            assert_eq!(text(SidebarPart::Diplomacy, "X"), Some(diplomacy_x));
            assert_eq!(text(SidebarPart::Diplomacy, "Art"), Some("diplobtn.shp"));
        }
    }

    #[test]
    fn a_modded_nod_base_keeps_the_stock_sidebar_and_side_positions() {
        for (base, problem, radar_y) in [
            ("Nod", Some(LayoutProblemKind::Loop), 48.0),
            ("Absent", Some(LayoutProblemKind::MissingLayout), 48.0),
            ("Custom", None, 63.0),
        ] {
            let ini = format!(
                "[Layouts]\nNod=Nod\n[Nod]\nBase={base}\n\
                 [Nod.Radar]\nX=4\n[Custom.Radar]\nY=63\n"
            );
            let file = MatchLayoutFile::from_bytes(ini.as_bytes());
            let expected = problem.map_or_else(Vec::new, |kind| expect(&[("[Nod] Base", kind)]));
            assert_eq!(problems(&file), expected, "Base={base}");
            let (spec, _) = sidebar(SidebarTheme::Soviet, file.for_side(Some("Nod"), false));
            assert_eq!(spec.parts, SidebarPart::STOCK_ORDER.map(Some));
            assert_eq!(spec.field, FieldLayout::ORIGINAL);
            assert_eq!(spec.place(SidebarPart::Diplomacy), [14.0, 5.0]);
            assert_eq!(spec.place(SidebarPart::Radar), [4.0, radar_y]);
        }
    }

    #[test]
    fn a_mod_button_can_inherit_the_stock_diplomacy_button() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Hud\n[Hud.Side]\nParts=Top,Halt\n\
              [Hud.Halt]\nBase=Diplomacy\nCommand=StopObject\n",
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        let [halt] = file.for_side(Some("GDI"), true).unwrap().added().as_ref() else {
            panic!("the sidebar should contain one added button");
        };
        assert_eq!(halt.holder, Holder::Side);
        assert_eq!(
            halt.kind,
            AddedKind::Button {
                command: Some("StopObject".to_owned()),
                label: "StopObject".to_owned(),
                art: Some("diplobtn.shp".to_owned()),
            }
        );
    }

    /// The sidebar's pictures come from the built-in file: each part's
    /// file, Yuri's own radar and palette, and the frames each part uses.
    #[test]
    fn the_built_in_sidebar_names_its_pictures() {
        let art = |theme| {
            sidebar_art(MatchLayout::original(true), theme)
                .into_iter()
                .map(|art| {
                    (
                        art.part.name().to_owned(),
                        art.key.name(),
                        art.file,
                        art.palette,
                        art.frames,
                    )
                })
                .collect::<Vec<_>>()
        };
        let piece = |node: &str, key: &'static str, file: &str, palette: Option<&str>, frames| {
            (
                node.to_owned(),
                key,
                file.to_owned(),
                palette.map(str::to_owned),
                frames,
            )
        };
        let allied = art(SidebarTheme::Allied);
        for expected in [
            piece("Credits", "Art", "credits.shp", None, 1),
            piece("Diplomacy", "Art", "diplobtn.shp", None, 2),
            piece("Options", "Art", "optbtn.shp", None, 2),
            piece("Radar", "Art", "radar.shp", Some("sidebar.pal"), 1),
            piece("Repair", "Art", "repair.shp", None, 5),
            piece("Sell", "Art", "sell.shp", None, 5),
            piece("Vehicles", "Art", "tab03.shp", None, 5),
            piece("Cameos", "RowArt", "side2.shp", None, 1),
            piece("Cameos", "FootArt", "addon.shp", None, 1),
            piece("ScrollUp", "Art", "r-up.shp", None, 3),
            piece("Power", "Art", "powerp.shp", None, 5),
        ] {
            assert!(allied.contains(&expected), "{expected:?} in {allied:?}");
        }
        assert!(art(SidebarTheme::Yuri).contains(&piece(
            "Radar",
            "Art",
            "radary.shp",
            Some("radaryuri.pal"),
            1
        )));
    }

    /// A built-in button's `Command=` and the money's `Value=` and `Align=`
    /// are read by the parts' own code; one the game has not is refused
    /// and the built-in one kept. A tab's `Tab=` cannot change yet.
    #[test]
    fn a_layout_gives_the_built_in_parts_commands_and_texts() {
        let mut file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nGDI=Hud\nNod=Bad\n\
              [Hud.Repair]\nCommand=StopObject\n\
              [Hud.Sell]\nCommand=Levitate\n\
              [Hud.Options]\nCommand=options\n\
              [Hud.Buildings]\nTab=Defense\n\
              [Hud.Money]\nValue=Time\nAlign=Left\n\
              [Bad.Money]\nValue=Gold\nAlign=Middle\n",
        );
        file.check_commands(|name| name.eq_ignore_ascii_case("StopObject"));
        assert_eq!(
            problems(&file),
            expect(&[
                ("[Hud.Sell] Command", LayoutProblemKind::UnknownCommand),
                ("[Hud.Buildings] Tab", LayoutProblemKind::NotYet),
                ("[Bad.Money] Value", LayoutProblemKind::UnknownValue),
                ("[Bad.Money] Align", LayoutProblemKind::UnknownValue),
            ])
        );
        for gdi_positions in [true, false] {
            let layout = file.for_side(Some("GDI"), gdi_positions).unwrap();
            let text = |part, key| layout.text(part, key, SidebarTheme::Allied);
            assert_eq!(text(SidebarPart::Repair, "Command"), Some("StopObject"));
            assert_eq!(text(SidebarPart::Sell, "Command"), Some("Levitate"));
            assert_eq!(text(SidebarPart::Diplomacy, "Command"), Some("Diplomacy"));
            assert_eq!(text(SidebarPart::Money, "Value"), Some("Time"));
            assert_eq!(text(SidebarPart::Money, "Align"), Some("Left"));
        }
    }

    #[test]
    fn a_layout_changes_the_pictures_of_the_built_in_parts() {
        use crate::ui::sidebar::ArtKey;
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nDefault=Mine\n\
              [Mine.Repair]\nArt=myrepair.shp\nFrames=Idle,On\n\
              [Mine.Radar]\nArt@Soviet=myradar.shp\nPalette@Soviet=mine.pal\n",
        );
        assert_eq!(problems(&file), expect(&[]));
        let layout = file.for_side(Some("GDI"), true).unwrap();
        let find = |theme: SidebarTheme, part: SidebarPart| {
            sidebar_art(layout, theme)
                .into_iter()
                .find(|art| art.part == part && art.key == ArtKey::Art)
                .unwrap()
        };
        // The layout's own picture, with the built-in one to fall back on.
        let repair = find(SidebarTheme::Allied, SidebarPart::Repair);
        assert_eq!(repair.file, "myrepair.shp");
        assert!(repair.own);
        assert_eq!(repair.built_in.as_deref(), Some("repair.shp"));
        assert_eq!(repair.frames, 2);
        assert_eq!((repair.palette, repair.own_palette), (None, false));
        // What it does not write stays the built-in file's.
        let sell = find(SidebarTheme::Soviet, SidebarPart::Sell);
        assert_eq!((sell.file.as_str(), sell.own), ("sell.shp", false));
        // A key of one side's pictures holds for that side alone.
        let radar = find(SidebarTheme::Allied, SidebarPart::Radar);
        assert_eq!((radar.file.as_str(), radar.own), ("radar.shp", false));
        let radar = find(SidebarTheme::Soviet, SidebarPart::Radar);
        assert_eq!(radar.file, "myradar.shp");
        assert!(radar.own);
        assert_eq!(radar.built_in.as_deref(), Some("radar.shp"));
        assert_eq!(radar.palette.as_deref(), Some("mine.pal"));
        assert!(radar.own_palette);
        // The built-in Yuri radar is not the layout's own.
        let radar = find(SidebarTheme::Yuri, SidebarPart::Radar);
        assert_eq!((radar.file.as_str(), radar.own), ("radary.shp", false));
        assert_eq!(radar.palette.as_deref(), Some("radaryuri.pal"));
        assert!(!radar.own_palette);
        // The original layout names no picture of its own.
        for theme in [
            SidebarTheme::Allied,
            SidebarTheme::Soviet,
            SidebarTheme::Yuri,
        ] {
            assert!(
                sidebar_art(MatchLayout::original(false), theme)
                    .iter()
                    .all(|art| !art.own && !art.own_palette)
            );
        }
    }

    #[test]
    fn a_layouts_colors_give_the_side_text() {
        // The original: SideText of the built-in [Colors].
        for (theme, color) in [
            (SidebarTheme::Allied, [164, 210, 255]),
            (SidebarTheme::Soviet, [255, 255, 0]),
            (SidebarTheme::Yuri, [255, 255, 0]),
        ] {
            let spec = SidebarChromeLayoutSpec::for_theme(theme);
            assert_eq!((spec.side_text, spec.label_text), (color, color));
        }
        // A [Colors] line of the mod's file changes the colour that names it,
        // for its side alone; a part may name its own colour.
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nDefault=Mine\n\
              [Colors]\nSideText@Soviet=200,0,0\nGold=250,200,50\n\
              [Mine.Cameos]\nLabelColor=Gold\n\
              [Mine.Money]\nColor@Yuri=1,2,3\n",
        );
        assert_eq!(problems(&file), expect(&[]));
        let (allied, _) = sidebar(SidebarTheme::Allied, file.for_side(Some("GDI"), true));
        assert_eq!(allied.side_text, [164, 210, 255]);
        assert_eq!(allied.label_text, [250, 200, 50]);
        let (soviet, _) = sidebar(SidebarTheme::Soviet, file.for_side(Some("Nod"), false));
        assert_eq!(soviet.side_text, [200, 0, 0]);
        assert_eq!(soviet.label_text, [250, 200, 50]);
        let (yuri, _) = sidebar(SidebarTheme::Yuri, file.for_side(Some("ThirdSide"), false));
        assert_eq!(yuri.side_text, [1, 2, 3]);
    }

    /// A mod writes a built-in part's picture and the money's place on its
    /// plate: both keys are its to change.
    #[test]
    fn a_mods_picture_and_money_keys_are_read_without_a_problem() {
        let file = MatchLayoutFile::from_bytes(
            b"[Layouts]\nDefault=Probe\n\
              [Colors]\nSideText@Soviet=255,0,0\n\
              [Probe.Repair]\nArt=sell.shp\n\
              [Probe.Money]\nAlign=Left\n",
        );
        assert!(file.problems().is_empty(), "{:?}", file.problems());
    }
}
