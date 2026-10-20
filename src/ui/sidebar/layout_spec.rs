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
    /// VERA's touch chrome fills the column with larger control targets.
    /// Retail desktop layout and native comparisons leave this disabled.
    pub touch_controls: bool,
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
    // VERA-local development controls do not reserve native chrome rows.
    pub control_button_gap: f32,
    pub control_block_top_pad: f32,
}

impl SidebarChromeLayoutSpec {
    pub const fn stock() -> Self {
        Self::for_theme(SidebarTheme::Allied)
    }

    pub const fn for_theme(theme: SidebarTheme) -> Self {
        let allied = matches!(theme, SidebarTheme::Allied);
        Self {
            theme,
            touch_controls: false,
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
            control_button_gap: 2.0,
            control_block_top_pad: 2.0,
        }
    }
}

impl Default for SidebarChromeLayoutSpec {
    fn default() -> Self {
        Self::stock()
    }
}
