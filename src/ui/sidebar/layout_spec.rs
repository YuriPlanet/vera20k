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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarChromeLayoutSpec {
    pub theme: SidebarTheme,
    pub sidebar_width: f32,
    pub top_inset: f32,
    pub side2_height: f32,
    pub side3_height: f32,
    pub footer_allowance: i32,
    pub repair_x: f32,
    pub repair_y: f32,
    pub sell_x: f32,
    pub sell_y: f32,
    pub top_button_x: f32,
    pub top_button_y: f32,
    pub tab_x: f32,
    pub tab_pitch: f32,
    pub cameo_inset_x: f32,
    pub cameo_inset_y: f32,
    pub cameo_gap_x: f32,
    pub cameo_width: f32,
    pub cameo_height: f32,
    pub cameo_row_height: f32,
    pub scroll_x: f32,
    pub scroll_pitch: f32,
    pub power_bar_x: f32,
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
        Self {
            theme,
            sidebar_width: super::SIDEBAR_WIDTH_PX as f32,
            top_inset: 48.0,
            side2_height: 50.0,
            side3_height: 26.0,
            footer_allowance: if allied { 26 } else { 18 },
            repair_x: if allied { 20.0 } else { 33.0 },
            repair_y: if allied { 8.0 } else { 7.0 },
            sell_x: if allied { 84.0 } else { 85.0 },
            sell_y: if allied { 8.0 } else { 7.0 },
            top_button_x: if allied { 11.0 } else { 14.0 },
            top_button_y: if allied { 20.0 } else { 21.0 },
            tab_x: if allied { 26.0 } else { 20.0 },
            tab_pitch: if allied { 29.0 } else { 32.0 },
            cameo_inset_x: 22.0,
            cameo_inset_y: 1.0,
            cameo_gap_x: if allied { 3.0 } else { 4.0 },
            cameo_width: 60.0,
            cameo_height: 48.0,
            cameo_row_height: 50.0,
            scroll_x: 39.0,
            scroll_pitch: if allied { 46.0 } else { 45.0 },
            power_bar_x: if allied { 5.0 } else { 0.0 },
            power_bar_tile_height: 3.0,
        }
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
        assert_eq!(soviet_gdi.cameo_gap_x, 3.0);
        assert_eq!(soviet_gdi.power_bar_x, 5.0);

        let allied_nod =
            SidebarChromeLayoutSpec::for_theme_and_positions(SidebarTheme::Allied, false);
        assert_eq!(allied_nod.theme, SidebarTheme::Allied);
        assert_eq!(allied_nod.tab_x, 20.0);
        assert_eq!(allied_nod.tab_pitch, 32.0);
        assert_eq!(allied_nod.cameo_gap_x, 4.0);
        assert_eq!(allied_nod.power_bar_x, 0.0);
    }
}
