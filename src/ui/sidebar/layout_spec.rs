//! Ordinary YR sidebar geometry, in unscaled sidebar-surface pixels.
//!
//! `6A5090` installs the side-specific constants; `6A5130` recomputes screen
//! anchors and strip capacity. `6ABD30` applies them to the actual gadgets.
//! The previous implicit RON override fitted an unrelated scaled stack and is
//! absent from the production layout path.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarTheme {
    #[default]
    Allied,
    Soviet,
    Yuri,
}

/// A part of the original sidebar, under the name `veraui.ini` gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarPart {
    /// CREDITS.SHP plate.
    Credits,
    /// The credits number, in [`Self::Credits`].
    Money,
    /// TOP.SHP strip.
    Top,
    /// DIPLOBTN and OPTBTN, in [`Self::Top`].
    Diplomacy,
    Options,
    /// The radar housing.
    Radar,
    /// The map picture, in [`Self::Radar`].
    Minimap,
    /// SIDE1.
    Middle,
    /// Repair, Sell and the four tabs, in [`Self::Middle`].
    Repair,
    Sell,
    Tabs,
    /// The four tab buttons, in [`Self::Tabs`].
    Buildings,
    Defenses,
    Infantry,
    Vehicles,
    /// The cameo strip: its SIDE2 rows, cameos, SIDE3 and ADDON.
    Cameos,
    /// The strip's scroll buttons, in [`Self::Cameos`] and placed from the
    /// bottom of its last row.
    ScrollDown,
    ScrollUp,
    Power,
}

/// A picture key of a part: the file of each one draws a piece of it. Every
/// part's own picture is `Art`; a cameo grid has its rows' and its end's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtKey {
    Art,
    RowArt,
    EmptyRowArt,
    EndArt,
    FootArt,
}

impl ArtKey {
    pub const ALL: [Self; 5] = [
        Self::Art,
        Self::RowArt,
        Self::EmptyRowArt,
        Self::EndArt,
        Self::FootArt,
    ];

    /// The key's name in `veraui.ini`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Art => "Art",
            Self::RowArt => "RowArt",
            Self::EmptyRowArt => "EmptyRowArt",
            Self::EndArt => "EndArt",
            Self::FootArt => "FootArt",
        }
    }
}

/// What kind of part a [`SidebarPart`] is: its `veraui.ini` `Type=`, which
/// decides how it is drawn and what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    /// A picture's first frame.
    Image,
    /// A line of text.
    Text,
    /// A picture whose frame shows the button's state.
    Button,
    /// A tab button: a [`Self::Button`] that picks a strip.
    Tab,
    /// The radar's housing and its opening and closing.
    Radar,
    /// The map picture.
    Minimap,
    /// Draws nothing; holds parts.
    Panel,
    /// The build pictures and the rows behind them.
    CameoGrid,
    PowerBar,
}

impl SidebarPart {
    /// Every part, in the original's draw order: a panel, then the parts in
    /// it.
    pub const STOCK_ORDER: [Self; 19] = [
        Self::Credits,
        Self::Money,
        Self::Top,
        Self::Diplomacy,
        Self::Options,
        Self::Radar,
        Self::Minimap,
        Self::Middle,
        Self::Repair,
        Self::Sell,
        Self::Tabs,
        Self::Buildings,
        Self::Defenses,
        Self::Infantry,
        Self::Vehicles,
        Self::Cameos,
        Self::ScrollDown,
        Self::ScrollUp,
        Self::Power,
    ];

    /// The four tab buttons, in the tabs' order.
    pub const TABS: [Self; 4] = [Self::Buildings, Self::Defenses, Self::Infantry, Self::Vehicles];

    /// The panel a part sits in; `None` for a part held by the sidebar
    /// itself.
    pub const fn panel(self) -> Option<Self> {
        match self {
            Self::Money => Some(Self::Credits),
            Self::Diplomacy | Self::Options => Some(Self::Top),
            Self::Minimap => Some(Self::Radar),
            Self::Repair | Self::Sell | Self::Tabs => Some(Self::Middle),
            Self::Buildings | Self::Defenses | Self::Infantry | Self::Vehicles => Some(Self::Tabs),
            Self::ScrollDown | Self::ScrollUp => Some(Self::Cameos),
            Self::Credits | Self::Top | Self::Radar | Self::Middle | Self::Cameos | Self::Power => {
                None
            }
        }
    }

    /// The part's kind, as the built-in `veraui_default.ini` gives it.
    pub const fn kind(self) -> PartKind {
        match self {
            Self::Credits | Self::Top | Self::Middle => PartKind::Image,
            Self::Money => PartKind::Text,
            Self::Diplomacy
            | Self::Options
            | Self::Repair
            | Self::Sell
            | Self::ScrollDown
            | Self::ScrollUp => PartKind::Button,
            Self::Buildings | Self::Defenses | Self::Infantry | Self::Vehicles => PartKind::Tab,
            Self::Radar => PartKind::Radar,
            Self::Minimap => PartKind::Minimap,
            Self::Tabs => PartKind::Panel,
            Self::Cameos => PartKind::CameoGrid,
            Self::Power => PartKind::PowerBar,
        }
    }

    /// The name of the part's `[Layout.Part]` section.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Credits => "Credits",
            Self::Money => "Money",
            Self::Top => "Top",
            Self::Diplomacy => "Diplomacy",
            Self::Options => "Options",
            Self::Radar => "Radar",
            Self::Minimap => "Minimap",
            Self::Middle => "Middle",
            Self::Repair => "Repair",
            Self::Sell => "Sell",
            Self::Tabs => "Tabs",
            Self::Buildings => "Buildings",
            Self::Defenses => "Defenses",
            Self::Infantry => "Infantry",
            Self::Vehicles => "Vehicles",
            Self::Cameos => "Cameos",
            Self::ScrollDown => "ScrollDown",
            Self::ScrollUp => "ScrollUp",
            Self::Power => "Power",
        }
    }
}

/// Every number the sidebar is laid out from, in sidebar pixels. Each part's
/// place is measured from the part that holds it, as `veraui.ini` writes it
/// (`ui::match_layout`); [`super::compute_layout_with_spec`] and the view
/// builder turn them into screen positions.
/// [`Default`] is every number zero: [`Self::stock`] is the original.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SidebarChromeLayoutSpec {
    pub theme: SidebarTheme,
    /// The side's sidebar text colour (`SetSidebarTextColor @ 0x0072F440`):
    /// the money's `veraui.ini` `Color=`, by default `[Colors] SideText`
    /// of the built-in file. Read with the layout, not every frame.
    pub side_text: [u8; 3],
    /// The cameo labels' colour: `[Cameos] LabelColor=`, by default the
    /// same `SideText`.
    pub label_text: [u8; 3],
    /// The parts the sidebar has, in draw order: a panel before the parts in
    /// it, a later part over an earlier one. A part that is not listed is not
    /// drawn and takes no clicks.
    pub parts: [Option<SidebarPart>; 19],
    /// The edges the sidebar and the command bar sit on, whether the sidebar
    /// floats over the battlefield, and whether the layout has the bar.
    pub places: super::PanelPlaces,
    /// The column's width, `[Side] Width=`: the frame takes it
    /// ([`super::SidebarFrame::for_layout`]), and the layout reads the
    /// sidebar's place and size from the frame.
    pub sidebar_width: f32,
    /// The sidebar's height when its layout puts it on the top or the
    /// bottom edge; on the left or the right it is as tall as the screen.
    pub side_height: f32,
    /// Where the pieces over the battlefield sit: messages, timers, the
    /// banner, the tooltip and Phobos' superweapon column.
    pub field: crate::ui::field_layout::FieldLayout,
    /// SIDE2 and SIDE3 canvas heights, taken from the loaded art.
    pub side2_height: f32,
    pub side3_height: f32,
    /// Every part's `X` and `Y` in the part that holds it, by
    /// `SidebarPart as usize`: see [`Self::place`]. The original's come from
    /// `0x006A5090`, `0x006A5130`, `RadarClass::Init_For_House @ 0x00652E90`
    /// and `DrawCreditsSHPBackground @ 0x006D0E60`, as `veraui_default.ini`
    /// writes them.
    pub part_places: [[f32; 2]; 19],
    /// The radar aperture's size. `RadarClass::One_Time @ 0x00652CF0` sets
    /// MapAreaY 49 (housing at RadarShapeY 48) and the 140x108 area;
    /// `Init_For_House @ 0x00652E90` writes MapAreaX 16 in both its GDI and
    /// NOD arms (`0x00652ECF`, `0x00652F13`), which differ only in the two
    /// top buttons.
    pub radar_width: f32,
    pub radar_height: f32,
    /// The first cameo, from the strip's top-left corner (`0x006A5130`:
    /// sidebar + 0x16).
    pub cameo_inset_x: f32,
    pub cameo_inset_y: f32,
    /// How many cameos a row holds: the original's two (GetVisibleSlotCount
    /// `0x006AC430` counts 2 * rows).
    pub cameo_columns: f32,
    /// From one cameo column to the next (`0x006A5090`: 0x3F or 0x40).
    pub cameo_column_pitch: f32,
    /// One cell of SIDE2's own art, in its pixels: the button frame plus the
    /// gap that follows it (0x3F, or 0x40 on the Soviet table). SIDE2 paints
    /// two of these side by side, so a strip of more columns repeats this one
    /// cut-out per cell instead of stretching the painted pair. It never
    /// changes once the table is built: it is a source rectangle, not a
    /// layout span.
    pub cameo_cell_source_width: f32,
    pub cameo_width: f32,
    pub cameo_height: f32,
    /// From one cameo row to the next, and the height that decides how many
    /// rows fit (`0x006A5130`: 0x32).
    pub cameo_row_height: f32,
    /// Height kept free under the last row (`0x006A5130`: 0x1A, or 0x12 when
    /// Scenario +0x34B8 is set, plus 7).
    pub strip_footer: f32,
    pub power_bar_tile_height: f32,
}

impl SidebarChromeLayoutSpec {
    pub fn stock() -> Self {
        Self::for_theme(SidebarTheme::Allied)
    }

    pub fn for_theme(theme: SidebarTheme) -> Self {
        Self::for_theme_and_positions(theme, matches!(theme, SidebarTheme::Allied))
    }

    /// Phobos `[SOMESIDE] Sidebar.GDIPositions=`: the artwork theme and the
    /// hardcoded button coords are independent. `theme` still selects the
    /// SIDEBAR MIX and chrome heights; `gdi_positions` selects the GDI
    /// (allied) position table.
    ///
    /// The positions are the built-in `veraui_default.ini`, the bottom layer
    /// under a mod's `veraui.ini` and the player's own; what a layout file has
    /// no key for stays here.
    pub fn for_theme_and_positions(theme: SidebarTheme, gdi_positions: bool) -> Self {
        let mut spec = Self {
            theme,
            side2_height: 50.0,
            side3_height: 26.0,
            ..Self::default()
        };
        crate::ui::match_layout::MatchLayout::original(gdi_positions).apply_to_sidebar(&mut spec);
        // SIDE2 paints two cells of the table's own column width.
        spec.cameo_cell_source_width = spec.cameo_column_pitch;
        spec
    }
}

impl SidebarChromeLayoutSpec {
    /// Where `part` sits in the part that holds it (the sidebar itself for a
    /// part with no panel; the bottom of the last row for the scroll
    /// buttons), as its `veraui.ini` `X` and `Y` say.
    pub fn place(&self, part: SidebarPart) -> [f32; 2] {
        self.part_places[part as usize]
    }

    /// Where a radar movie's top-left corner sits in the radar housing:
    /// the enqueue `0x005BF390` places the clip at (MapAreaX - 3,
    /// MapAreaY - 1) on the sidebar (`0x005BF3D0..0x005BF3E5`), the housing's
    /// own corner being (ShapeX, RadarShapeY).
    pub fn radar_movie_origin(&self) -> [f32; 2] {
        let [x, y] = self.place(SidebarPart::Minimap);
        [x - 3.0, y - 1.0]
    }

    /// The size of a grid of `columns` by `rows` cameo places: its
    /// pictures' pitches.
    pub fn grid_size(&self, columns: usize, rows: usize) -> [f32; 2] {
        [
            columns as f32 * self.cameo_column_pitch,
            rows as f32 * self.cameo_row_height,
        ]
    }

    /// The cameos in one strip row, at least one.
    pub fn columns(&self) -> usize {
        (self.cameo_columns as usize).max(1)
    }

    /// VERA's own: the player's cameo columns, kept inside the sidebar's own
    /// width. The two original columns keep the span they take (2 *
    /// `cameo_column_pitch`, 126 in the allied table); each further column
    /// takes an equal share of it, and the cameo and its row shrink by the
    /// same factor, so a strip of 3 to 5 columns neither overlaps nor draws
    /// past the column. 2 leaves the original's numbers alone.
    ///
    /// Call it once, on the numbers the side's layout starts from: a second
    /// call would measure the span from the already shrunk pitch.
    pub fn apply_player_columns(&mut self, columns: usize) {
        let columns = columns.clamp(2, 5);
        if columns == 2 {
            return;
        }
        let span = 2.0 * self.cameo_column_pitch;
        let gap = (self.cameo_column_pitch - self.cameo_width).max(0.0);
        let pitch = span / columns as f32;
        let width = (pitch - gap).max(1.0);
        let scale = width / self.cameo_width.max(1.0);
        self.cameo_columns = columns as f32;
        self.cameo_column_pitch = pitch;
        self.cameo_width = width;
        self.cameo_height = (self.cameo_height * scale).max(1.0);
        self.cameo_row_height = (self.cameo_row_height * scale).max(1.0);
    }

    /// Whether the layout has this part.
    pub fn shows(&self, part: SidebarPart) -> bool {
        self.parts.contains(&Some(part))
    }

    /// The parts in draw order.
    pub fn parts_in_order(&self) -> impl Iterator<Item = SidebarPart> + '_ {
        self.parts.iter().flatten().copied()
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    #[test]
    fn gdi_positions_are_independent_of_the_art_theme() {
        use SidebarPart::{Defenses, Minimap, Power, Tabs};
        let allied = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        let soviet = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Soviet);
        assert_eq!(allied.place(Tabs), [26.0, 39.0]);
        assert_eq!(soviet.place(Tabs), [20.0, 39.0]);

        let soviet_gdi =
            SidebarChromeLayoutSpec::for_theme_and_positions(SidebarTheme::Soviet, true);
        assert_eq!(soviet_gdi.theme, SidebarTheme::Soviet);
        assert_eq!(soviet_gdi.place(Tabs), [26.0, 39.0]);
        assert_eq!(soviet_gdi.place(Defenses), [29.0, 0.0]);
        assert_eq!(soviet_gdi.cameo_column_pitch, 63.0);
        assert_eq!(soviet_gdi.place(Power), [5.0, 227.0]);
        assert_eq!(soviet_gdi.place(Minimap), [16.0, 1.0]);

        let allied_nod =
            SidebarChromeLayoutSpec::for_theme_and_positions(SidebarTheme::Allied, false);
        assert_eq!(allied_nod.theme, SidebarTheme::Allied);
        assert_eq!(allied_nod.place(Tabs), [20.0, 39.0]);
        assert_eq!(allied_nod.place(Defenses), [32.0, 0.0]);
        assert_eq!(allied_nod.cameo_column_pitch, 64.0);
        assert_eq!(allied_nod.place(Power), [0.0, 227.0]);
        // Both arms of Init_For_House write MapAreaX 16 (`0x00652F13`).
        assert_eq!(allied_nod.place(Minimap), [16.0, 1.0]);
        // The 140-wide clip starts 13 px into the 168-wide housing, not
        // centred at 14.
        assert_eq!(allied_nod.radar_movie_origin(), [13.0, 0.0]);
    }

    #[test]
    fn player_columns_share_the_original_span_without_overlapping() {
        let stock = SidebarChromeLayoutSpec::stock();
        let span = 2.0 * stock.cameo_column_pitch;
        for columns in 3..=5 {
            let mut spec = stock;
            spec.apply_player_columns(columns);
            assert_eq!(spec.columns(), columns);
            // The sidebar's own width never changes: only the cells inside it.
            assert_eq!(spec.sidebar_width, stock.sidebar_width);
            // The strip still ends where the original's two columns ended.
            let right = spec.cameo_inset_x + columns as f32 * spec.cameo_column_pitch;
            assert!((right - (stock.cameo_inset_x + span)).abs() < 0.01);
            // A cameo fits its pitch, so neighbours never overlap.
            assert!(spec.cameo_width < spec.cameo_column_pitch);
            assert!(spec.cameo_height < spec.cameo_row_height);
            // SIDE2 paints one cell per column from this source cut, and that
            // cut is the art's own, so it must not follow the pitch.
            assert_eq!(spec.cameo_cell_source_width, stock.cameo_cell_source_width);
            // The picture keeps its shape.
            let ratio = spec.cameo_height / spec.cameo_width;
            assert!((ratio - 48.0 / 60.0).abs() < 0.001);
        }
    }

    #[test]
    fn two_columns_leave_the_original_numbers_alone() {
        let stock = SidebarChromeLayoutSpec::stock();
        let mut spec = stock;
        spec.apply_player_columns(2);
        assert_eq!(spec, stock);
    }
}
