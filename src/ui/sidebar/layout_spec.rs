//! Ordinary YR sidebar geometry, in unscaled sidebar-surface pixels.
//!
//! `6A5090` installs the side-specific constants; `6A5130` recomputes screen
//! anchors and strip capacity. `6ABD30` applies them to the actual gadgets.
//! The previous implicit RON override fitted an unrelated scaled stack and is
//! absent from the production layout path.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTheme {
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
    /// The radar housing and its minimap.
    Radar,
    /// SIDE1.
    Middle,
    /// Repair, Sell and the four tabs, in [`Self::Middle`].
    Repair,
    Sell,
    Tabs,
    /// The cameo strip: its SIDE2 rows, cameos, SIDE3, ADDON and scroll
    /// buttons.
    Cameos,
    Power,
}

impl SidebarPart {
    /// Every part, in the original's draw order: a panel, then the parts in
    /// it.
    pub const STOCK_ORDER: [Self; 12] = [
        Self::Credits,
        Self::Money,
        Self::Top,
        Self::Diplomacy,
        Self::Options,
        Self::Radar,
        Self::Middle,
        Self::Repair,
        Self::Sell,
        Self::Tabs,
        Self::Cameos,
        Self::Power,
    ];

    /// The panel a part sits in; `None` for a part held by the sidebar
    /// itself.
    pub const fn panel(self) -> Option<Self> {
        match self {
            Self::Money => Some(Self::Credits),
            Self::Diplomacy | Self::Options => Some(Self::Top),
            Self::Repair | Self::Sell | Self::Tabs => Some(Self::Middle),
            Self::Credits | Self::Top | Self::Radar | Self::Middle | Self::Cameos | Self::Power => {
                None
            }
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
            Self::Middle => "Middle",
            Self::Repair => "Repair",
            Self::Sell => "Sell",
            Self::Tabs => "Tabs",
            Self::Cameos => "Cameos",
            Self::Power => "Power",
        }
    }
}

/// Every number the sidebar is laid out from, in sidebar pixels. Each part's
/// place is measured from the part that holds it, as `veraui.ini` writes it
/// (`ui::match_layout`); [`super::compute_layout_with_spec`] and the view
/// builder turn them into screen positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarChromeLayoutSpec {
    pub theme: SidebarTheme,
    /// The parts the sidebar has, in draw order: a panel before the parts in
    /// it, a later part over an earlier one. A part that is not listed is not
    /// drawn and takes no clicks.
    pub parts: [Option<SidebarPart>; 12],
    /// The edges the sidebar and the command bar sit on, whether the sidebar
    /// floats over the battlefield, and whether the layout has the bar.
    pub places: super::PanelPlaces,
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
    /// CREDITS.SHP plate, from the sidebar's top-left corner
    /// (`DrawCreditsSHPBackground @ 0x006D0E60`).
    pub credits_x: f32,
    pub credits_y: f32,
    /// The credits number, from the middle of the plate's top edge. gamemd
    /// centres it on the sidebar surface and passes the literal row 2.
    pub money_x: f32,
    pub money_y: f32,
    /// TOP.SHP strip, from the sidebar's top-left corner
    /// (`RadarClass::Draw @ 0x00653100`).
    pub top_x: f32,
    pub top_y: f32,
    /// DIPLOBTN then OPTBTN, from the strip's top-left corner.
    /// `RadarClass::Init_For_House @ 0x00652E90`: shape X + 0xB and + 0x53,
    /// top shape Y + 4 for GDI; + 0xE, + 0x56 and + 5 for NOD.
    pub top_buttons: [[f32; 2]; 2],
    /// Radar housing (RADAR.SHP), from the sidebar's top-left corner.
    pub radar_housing_x: f32,
    pub radar_housing_y: f32,
    /// Radar aperture, from the housing's top-left corner, and its size.
    /// `RadarClass::One_Time @ 0x00652CF0` sets MapAreaY 49 (housing at
    /// RadarShapeY 48) and the 140x108 area; `Init_For_House @ 0x00652E90`
    /// writes MapAreaX 16 in both its GDI and NOD arms (`0x00652ECF`,
    /// `0x00652F13`), which differ only in the two top buttons.
    pub radar_x: f32,
    pub radar_y: f32,
    pub radar_width: f32,
    pub radar_height: f32,
    /// SIDE1 and the sidebar body rectangle, from the sidebar's top-left
    /// corner (`0x006A5130`: the rectangle's top is 0x9E).
    pub side1_x: f32,
    pub side1_y: f32,
    /// Repair, Sell and the first tab, from SIDE1's top-left corner
    /// (`0x006A5090`: body + 8 or 7, body + 0x27).
    pub repair_x: f32,
    pub repair_y: f32,
    pub sell_x: f32,
    pub sell_y: f32,
    pub tab_x: f32,
    pub tab_y: f32,
    pub tab_pitch: f32,
    /// The cameo strip (its SIDE2 rows), from the sidebar's top-left corner
    /// (`0x006A5090`: body + 0x45).
    pub strip_x: f32,
    pub strip_y: f32,
    /// The first cameo, from the strip's top-left corner (`0x006A5130`:
    /// sidebar + 0x16).
    pub cameo_inset_x: f32,
    pub cameo_inset_y: f32,
    /// How many cameos a row holds: the original's two (GetVisibleSlotCount
    /// `0x006AC430` counts 2 * rows).
    pub cameo_columns: f32,
    /// From one cameo column to the next (`0x006A5090`: 0x3F or 0x40).
    pub cameo_column_pitch: f32,
    pub cameo_width: f32,
    pub cameo_height: f32,
    /// From one cameo row to the next, and the height that decides how many
    /// rows fit (`0x006A5130`: 0x32).
    pub cameo_row_height: f32,
    /// Height kept free under the last row (`0x006A5130`: 0x1A, or 0x12 when
    /// Scenario +0x34B8 is set, plus 7).
    pub strip_footer: f32,
    /// R-DN, from the strip's left edge and the last row's bottom, and the
    /// step to R-UP (`0x006A5130`: sidebar + 0x27, strip bottom + 7, 0x2E or
    /// 0x2D).
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub scroll_pitch: f32,
    /// Power bar, from the sidebar's top-left corner.
    pub power_bar_x: f32,
    pub power_bar_y: f32,
    pub power_bar_tile_height: f32,
}

impl SidebarChromeLayoutSpec {
    pub const fn stock() -> Self {
        Self::for_theme(SidebarTheme::Allied)
    }

    pub const fn for_theme(theme: SidebarTheme) -> Self {
        Self::for_theme_and_positions(theme, matches!(theme, SidebarTheme::Allied))
    }

    /// Phobos `[SOMESIDE] Sidebar.GDIPositions=`: the artwork theme and the
    /// hardcoded button coords are independent. `theme` still selects the
    /// SIDEBAR MIX and chrome heights; `gdi_positions` selects the GDI
    /// (allied) position table.
    pub const fn for_theme_and_positions(theme: SidebarTheme, gdi_positions: bool) -> Self {
        let allied = gdi_positions;
        let stock = SidebarPart::STOCK_ORDER;
        let mut parts = [None; 12];
        let mut index = 0;
        while index < stock.len() {
            parts[index] = Some(stock[index]);
            index += 1;
        }
        Self {
            theme,
            parts,
            places: super::PanelPlaces::ORIGINAL,
            sidebar_width: super::SIDEBAR_WIDTH_PX as f32,
            side_height: super::SIDEBAR_WIDTH_PX as f32,
            field: crate::ui::field_layout::FieldLayout::ORIGINAL,
            side2_height: 50.0,
            side3_height: 26.0,
            credits_x: 0.0,
            credits_y: 0.0,
            money_x: 0.0,
            money_y: 2.0,
            top_x: 0.0,
            top_y: 16.0,
            top_buttons: if allied {
                [[11.0, 4.0], [83.0, 4.0]]
            } else {
                [[14.0, 5.0], [86.0, 5.0]]
            },
            radar_housing_x: 0.0,
            radar_housing_y: 48.0,
            radar_x: 16.0,
            radar_y: 1.0,
            radar_width: 140.0,
            radar_height: 108.0,
            side1_x: 0.0,
            side1_y: 158.0,
            repair_x: if allied { 20.0 } else { 33.0 },
            repair_y: if allied { 8.0 } else { 7.0 },
            sell_x: if allied { 84.0 } else { 85.0 },
            sell_y: if allied { 8.0 } else { 7.0 },
            tab_x: if allied { 26.0 } else { 20.0 },
            tab_y: 39.0,
            tab_pitch: if allied { 29.0 } else { 32.0 },
            strip_x: 0.0,
            strip_y: 227.0,
            cameo_inset_x: 22.0,
            cameo_inset_y: 1.0,
            cameo_columns: 2.0,
            cameo_column_pitch: if allied { 63.0 } else { 64.0 },
            cameo_width: 60.0,
            cameo_height: 48.0,
            cameo_row_height: 50.0,
            strip_footer: if allied { 33.0 } else { 25.0 },
            scroll_x: 39.0,
            scroll_y: 7.0,
            scroll_pitch: if allied { 46.0 } else { 45.0 },
            power_bar_x: if allied { 5.0 } else { 0.0 },
            power_bar_y: 227.0,
            power_bar_tile_height: 3.0,
        }
    }
}

impl SidebarChromeLayoutSpec {
    /// Where a radar movie's top-left corner sits in the radar housing:
    /// the enqueue `0x005BF390` places the clip at (MapAreaX - 3,
    /// MapAreaY - 1) on the sidebar (`0x005BF3D0..0x005BF3E5`), the housing's
    /// own corner being (ShapeX, RadarShapeY).
    pub fn radar_movie_origin(&self) -> [f32; 2] {
        [self.radar_x - 3.0, self.radar_y - 1.0]
    }

    /// Layout-space x of the sidebar's left edge on a layout screen
    /// `screen_w` wide: flush with the edge the layout puts it on.
    pub fn sidebar_x(&self, screen_w: f32) -> f32 {
        if self.places.side == super::Edge::Right {
            screen_w - self.sidebar_width
        } else {
            0.0
        }
    }

    /// The sidebar's width on a layout screen `screen_w` wide: its own on
    /// the left or the right edge, the screen's on the top or the bottom.
    pub fn side_width(&self, screen_w: f32) -> f32 {
        match self.places.side {
            super::Edge::Left | super::Edge::Right => self.sidebar_width,
            super::Edge::Top | super::Edge::Bottom => screen_w,
        }
    }

    /// The cameos in one strip row, at least one.
    pub fn columns(&self) -> usize {
        (self.cameo_columns as usize).max(1)
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

impl Default for SidebarChromeLayoutSpec {
    fn default() -> Self {
        Self::stock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gdi_positions_are_independent_of_the_art_theme() {
        let allied = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        let soviet = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Soviet);
        assert_eq!(allied.tab_x, 26.0);
        assert_eq!(soviet.tab_x, 20.0);

        let soviet_gdi =
            SidebarChromeLayoutSpec::for_theme_and_positions(SidebarTheme::Soviet, true);
        assert_eq!(soviet_gdi.theme, SidebarTheme::Soviet);
        assert_eq!(soviet_gdi.tab_x, 26.0);
        assert_eq!(soviet_gdi.tab_pitch, 29.0);
        assert_eq!(soviet_gdi.cameo_column_pitch, 63.0);
        assert_eq!(soviet_gdi.power_bar_x, 5.0);
        assert_eq!((soviet_gdi.radar_x, soviet_gdi.radar_y), (16.0, 1.0));

        let allied_nod =
            SidebarChromeLayoutSpec::for_theme_and_positions(SidebarTheme::Allied, false);
        assert_eq!(allied_nod.theme, SidebarTheme::Allied);
        assert_eq!(allied_nod.tab_x, 20.0);
        assert_eq!(allied_nod.tab_pitch, 32.0);
        assert_eq!(allied_nod.cameo_column_pitch, 64.0);
        assert_eq!(allied_nod.power_bar_x, 0.0);
        // Both arms of Init_For_House write MapAreaX 16 (`0x00652F13`).
        assert_eq!((allied_nod.radar_x, allied_nod.radar_y), (16.0, 1.0));
        // The 140-wide clip starts 13 px into the 168-wide housing, not
        // centred at 14.
        assert_eq!(allied_nod.radar_movie_origin(), [13.0, 0.0]);
    }
}
