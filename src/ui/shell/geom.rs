//! The front-end shell's dialog geometry: DLU->pixel, right-panel chrome,
//! button snap and the text alignment flags. Builds on the GUI crate's
//! generic pixel geometry, re-exported here so the shells have one geometry
//! path.
//!
//! Render-agnostic; depends on plain integers only. The three shells (dialogs
//! 0xE2 / 0x100 / 0x102) import these instead of each keeping a private copy.
//! Two distinct owner-draw snap algorithms are preserved (the main menu rounds
//! half-up to the nearest button row; single-player and skirmish bias-truncate a
//! tile index) because they are not proven equivalent at the half-row boundary.

pub use vera20k_gui::shell::geom::{RectPx, ScaledRects, ShellScale, layout_on};

/// The logical shell is authored at 800x600; the fill-window pass scales that
/// rectangle into the live window, keeping its 4:3 shape.
pub const SHELL_BASE_W: i32 = 800;
pub const SHELL_BASE_H: i32 = 600;

/// The retail mapping: the 800x600 dialog is placed in the window by the
/// host's dialog origin, never scaled. Every art fit keyed on this scale is
/// then a no-op, so the fill-window path is the only source of drift.
pub const RETAIL_SCALE: ShellScale = ShellScale::unscaled(SHELL_BASE_W, SHELL_BASE_H);

/// The 800x600 shell scaled to fit a `screen_w`x`screen_h` window at its own
/// shape, centred.
pub const fn shell_scale(screen_w: i32, screen_h: i32) -> ShellScale {
    ShellScale::fit(SHELL_BASE_W, SHELL_BASE_H, screen_w, screen_h)
}

/// The mapping the front-end shell takes in a window. Only the two sizes
/// gamemd's shell runs at keep the retail layout: 800x600, and 640x480 with
/// its own 640-wide art (`AllowModeToggle`). Any other window gets the
/// 800x600 dialog scaled to fit at its own shape, centred.
pub fn shell_scale_for_window(screen_w: i32, screen_h: i32) -> ShellScale {
    if matches!((screen_w, screen_h), (800, 600) | (640, 480)) {
        RETAIL_SCALE
    } else {
        shell_scale(screen_w, screen_h)
    }
}

/// The window-filling variant of a retail layout: `base` laid out on the
/// verified 800x600 dialog, then scaled to fit the `screen_w`x`screen_h`
/// window, keeping its shape. Identity at the base size.
pub fn fit_window<L: ScaledRects>(
    screen_w: u32,
    screen_h: u32,
    base: impl FnOnce(u32, u32) -> L,
) -> L {
    vera20k_gui::shell::geom::fit_window(SHELL_BASE_W, SHELL_BASE_H, screen_w, screen_h, base)
}

/// The room chat edit's rect beside VERA's send key, sized as the online
/// room's: the log's width less the send tile and its gap (`26 + 6` authored
/// px). Callers that do not show the key keep the chat edit's own rect.
pub(crate) fn room_chat_text_rect(chat: RectPx, log: RectPx, scale: ShellScale) -> RectPx {
    let tile = (26.0 * scale.factor()).round() as i32;
    let gap = (6.0 * scale.factor()).round() as i32;
    RectPx::new(chat.x, chat.y, (log.w - tile - gap).max(1), chat.h)
}

/// The room's send tile, at the online room's emote slot: just right of the
/// box, one pixel above it and `26 x 23` authored px. A platform without an
/// Enter key shows it; a press sends the line.
pub(crate) fn room_chat_send_rect(chat: RectPx, log: RectPx, scale: ShellScale) -> RectPx {
    let text = room_chat_text_rect(chat, log, scale);
    let tile_w = (26.0 * scale.factor()).round() as i32;
    let tile_h = (23.0 * scale.factor()).round() as i32;
    let gap = (6.0 * scale.factor()).round() as i32;
    RectPx::new(
        text.x + text.w + gap,
        chat.y - (1.0 * scale.factor()).round() as i32,
        tile_w,
        tile_h,
    )
}

// --- DLU base metrics (MS Sans Serif 8pt) ---
pub const DLU_BASE_X: i32 = 6;
pub const DLU_BASE_Y: i32 = 13;

// --- Right-panel chrome (SDTP top / SDBTNBKGD tile / SDBTM bottom) ---
pub const RIGHT_PANEL_WIDTH: i32 = 168;
pub const RIGHT_PANEL_TOP_H: i32 = 199;
pub const RIGHT_PANEL_TILE_H: i32 = 42;
pub const RIGHT_PANEL_TILE_COUNT_CAP: i32 = 9;
pub const SDBTNANM_CELL_H: i32 = 42;
pub const LOWER_STRIP_H: i32 = 32;

/// Native SDBTNANM.SHP button-cell width. The active 0xE2, 0x100, and 0x102
/// right-panel controls all use the 156-wide canvas flush at the panel's right
/// edge; the surrounding chrome remains 168 pixels wide.
pub const SDBTNANM_CELL_W_NARROW: i32 = 156;

/// Legacy fit for shell controls that still use `PANEL_FACE`.
pub const RIGHT_PANEL_FACE_W: u32 = 110;

/// The family heading and full-width right-panel buttons share the SDTP
/// window's 116 native pixels less one pixel on either side. Other shell
/// controls keep their fit.
pub(crate) const FAMILY_CAPTION_W: i32 = 116 - 2;

/// The shell's text alignment flags: what a GUI text node carries in
/// `Prop::Align` and `render::shell_text::draw_in_rect_scaled` draws by.
/// 0x01 = h-center, 0x02 = h-right, 0x04 = v-center, 0x08 = panel face,
/// 0x10 = single line.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellAlign(pub u8);

impl ShellAlign {
    pub const NONE: ShellAlign = ShellAlign(0);
    pub const H_CENTER: ShellAlign = ShellAlign(0x01);
    pub const H_RIGHT: ShellAlign = ShellAlign(0x02);
    pub const V_CENTER: ShellAlign = ShellAlign(0x04);
    /// VERA's own: the rect is wider than the right-panel art it labels, an
    /// SDBTNANM button's red face or the title window atop the column, so the
    /// text must also fit [`RIGHT_PANEL_FACE_W`] on one line to stay on it.
    pub const PANEL_FACE: ShellAlign = ShellAlign(0x08);
    /// An SDBTNANM button's caption, centred on its face.
    pub const BUTTON_CAPTION: ShellAlign = ShellAlign(0x01 | 0x04 | 0x08);
    /// The right panel's title, centred in its window.
    pub const PANEL_TITLE: ShellAlign = ShellAlign(0x01 | 0x08);
    /// Centred text whose rect already bounds the painted art.
    pub const ART_CAPTION: ShellAlign = ShellAlign(0x01 | 0x10);
    pub(crate) const SINGLE_LINE: ShellAlign = ShellAlign(0x10);

    pub fn contains(self, flag: ShellAlign) -> bool {
        (self.0 & flag.0) != 0
    }
}

impl std::ops::BitOr for ShellAlign {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        ShellAlign(self.0 | rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RightPanelRects {
    pub top: RectPx,
    /// The first tile row. Later rows come from [`Self::tile_row`], not from
    /// stepping this rect's height.
    pub tile: RectPx,
    pub tile_count: i32,
    pub bottom: RectPx,
    /// The mapping these rects went through and the tile it mapped, so a row
    /// or a cell in it scales like every other authored rect.
    scale: ShellScale,
    authored_tile: RectPx,
}

impl RightPanelRects {
    /// Text viewport in the family's SDTP heading window. At 800×600 the
    /// black inset is [658, 774); leave one pixel of margin on either side.
    /// Keep the heading static's native y and height (0x0060BD17).
    pub(crate) fn family_title_caption(self, title: RectPx) -> RectPx {
        let art = self.scale.scale_rect(RectPx::new(
            self.authored_tile.x + (RIGHT_PANEL_WIDTH - FAMILY_CAPTION_W) / 2,
            0,
            FAMILY_CAPTION_W,
            0,
        ));
        RectPx::new(art.x, title.y, art.w, title.h)
    }

    /// Tile row `row` of the column. A scaled panel rounds each row's edges
    /// like any other authored rect, so row `k` lands where a button authored
    /// on row `k` does; stepping `tile.y` by the rounded height drifts a pixel
    /// every few rows. Identity on the unscaled panel.
    pub fn tile_row(&self, row: i32) -> RectPx {
        self.row_cell(row, self.authored_tile.w, self.authored_tile.h)
    }

    /// A `w`x`h` shape authored right-aligned at the top of tile row `row`, in
    /// window space: where `0x006071E0` draws each slide frame (the executed
    /// positions in `tools/storage_oracle/shell_slide_engine.json`) and where
    /// the SDBTNANM buttons sit.
    pub fn row_cell(&self, row: i32, w: i32, h: i32) -> RectPx {
        let tile = self.authored_tile;
        self.scale.scale_rect(RectPx::new(
            tile.x + tile.w - w,
            tile.y + row * tile.h,
            w,
            h,
        ))
    }

    /// The panel moved `(dx, dy)` window pixels, as a fixed 800x600 layout is
    /// centred in a larger window. Only an unscaled panel moves; its authored
    /// tile is its window tile.
    pub fn translate(self, dx: i32, dy: i32) -> Self {
        debug_assert!(
            self.scale.is_unscaled(),
            "a scaled panel already fills its window"
        );
        Self {
            top: self.top.translate(dx, dy),
            tile: self.tile.translate(dx, dy),
            bottom: self.bottom.translate(dx, dy),
            authored_tile: self.authored_tile.translate(dx, dy),
            ..self
        }
    }

    /// The panel on `scale`'s mapping, scaled once from its 800x600 rects.
    pub fn scaled(self, scale: ShellScale) -> Self {
        debug_assert!(self.scale.is_unscaled(), "panels scale once, from 800x600");
        Self {
            top: scale.scale_rect(self.top),
            tile: scale.scale_rect(self.tile),
            tile_count: self.tile_count,
            bottom: scale.scale_rect(self.bottom),
            scale,
            authored_tile: self.authored_tile,
        }
    }

    /// How far these rects sit from the 168x199 SDTP canvas the panel art is
    /// authored against: `(1.0, 1.0)` on the retail layout at every screen size,
    /// the shell's scale factor once the 800x600 dialog is scaled into the
    /// window. Both axes clamp at one, so an unscaled panel never shrinks its art.
    pub fn stretch(&self) -> (f32, f32) {
        (
            (self.tile.w as f32 / RIGHT_PANEL_WIDTH as f32).max(1.0),
            (self.top.h as f32 / RIGHT_PANEL_TOP_H as f32).max(1.0),
        )
    }
}

/// Round-half-up MulDiv (sign-correct). Byte-identical to the three prior copies.
pub fn mul_div_round(n: i32, numer: i32, denom: i32) -> i32 {
    let value = n * numer;
    if value >= 0 {
        (value + denom / 2) / denom
    } else {
        (value - denom / 2) / denom
    }
}

/// DLU rect -> pixel rect (MS Sans Serif 8pt). Byte-identical to the three copies.
pub fn dlu_rect(x: i32, y: i32, w: i32, h: i32) -> RectPx {
    RectPx::new(
        mul_div_round(x, DLU_BASE_X, 4),
        mul_div_round(y, DLU_BASE_Y, 8),
        mul_div_round(w, DLU_BASE_X, 4),
        mul_div_round(h, DLU_BASE_Y, 8),
    )
}

/// Common resized template window: the 6x13 conversion one pixel wider and
/// taller. Original 775BC0 executes this extent correction for F0's modal and
/// children (tools/storage_oracle/saved_game_layout.json); the `0x10E` and `0x6B`
/// children also measure it in retail captures (`0x0060C42E` keeps the place).
/// The `0x129` movie list measured at the plain conversion.
pub fn child_window(x: i32, y: i32, w: i32, h: i32) -> RectPx {
    let rect = dlu_rect(x, y, w, h);
    RectPx::new(rect.x, rect.y, rect.w + 1, rect.h + 1)
}

/// The dialog origin `[0xB0FC1C]` (`0x0072EC70`): half the screen beyond
/// 800x600, from 1024x768 up only. A dialog's own background is drawn there
/// (`Background_Overlay` `0x0072E730`).
pub fn dialog_origin(screen_w: i32, screen_h: i32) -> (i32, i32) {
    (
        if screen_w > 1023 {
            (screen_w - 800) / 2
        } else {
            0
        },
        if screen_h > 767 {
            (screen_h - 600) / 2
        } else {
            0
        },
    )
}

/// `(screen - base) / 2` clamped to >= 0. Canonical form; algebraically equal to
/// the single-player `if screen > base` guard and the main-menu inline guard for
/// every i32.
pub fn center_offset(screen: i32, base: i32) -> i32 {
    ((screen - base) / 2).max(0)
}

impl ScaledRects for RightPanelRects {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        self.scaled(scale)
    }
}

/// Where an image static paints a `w`x`h` image in its window: centred only
/// along an axis where the window is larger, else at the window edge (kind 2
/// `0x00615831..0x006158F3`, kind 4 `0x0061595E..0x0061597E`).
pub fn centred_in_window(window: RectPx, w: i32, h: i32) -> (i32, i32) {
    let dx = if window.w > w { (window.w - w) / 2 } else { 0 };
    let dy = if window.h > h { (window.h - h) / 2 } else { 0 };
    (window.x + dx, window.y + dy)
}

/// Right-panel layout (SDTP top cap / SDBTNBKGD tile column / SDBTM bottom cap).
/// Reproduces all three shells' output exactly, including the `bottom_h.max(0)`
/// clamp. Same for 0xE2 / 0x100 / 0x102.
pub fn right_panel_rects(screen_w: i32, screen_h: i32) -> RightPanelRects {
    let left_margin = if screen_w > 1023 {
        (screen_w - 800) / 2
    } else {
        0
    };
    let top_margin = if screen_h > 767 {
        (screen_h - 600) / 2
    } else {
        0
    };
    let effective_right = screen_w - left_margin;
    let top = RectPx::new(
        effective_right - RIGHT_PANEL_WIDTH,
        top_margin,
        RIGHT_PANEL_WIDTH,
        RIGHT_PANEL_TOP_H,
    );
    let tile = RectPx::new(top.x, top.y + top.h, RIGHT_PANEL_WIDTH, RIGHT_PANEL_TILE_H);
    let effective_h = if screen_h > 767 {
        screen_h - top_margin * 2
    } else {
        screen_h
    };
    let remaining = (effective_h - top.h).max(0);
    let tile_count = (remaining / RIGHT_PANEL_TILE_H).min(RIGHT_PANEL_TILE_COUNT_CAP);
    let bottom_y = tile.y + tile_count * RIGHT_PANEL_TILE_H;
    let bottom_h = (screen_h - top_margin - bottom_y).max(0);
    RightPanelRects {
        top,
        tile,
        tile_count,
        bottom: RectPx::new(top.x, bottom_y, RIGHT_PANEL_WIDTH, bottom_h),
        scale: RETAIL_SCALE,
        authored_tile: tile,
    }
}

/// Lower strip (LWSCRN cap) flush against the screen/shell bottom. Used by the
/// main menu (0xE2) and single player (0x100); skirmish (0x102) has no lower strip.
pub fn lower_strip_rect(screen_w: i32, screen_h: i32) -> RectPx {
    let left_margin = if screen_w > 1023 {
        (screen_w - 800) / 2
    } else {
        0
    };
    let top_margin = if screen_h > 767 {
        (screen_h - 600) / 2
    } else {
        0
    };
    let shell_h = if screen_h > 767 { 600 } else { screen_h };
    // LWSCRNS at 640w is 472 wide; LWSCRNL at >=800w is 632 wide.
    let w = if screen_w == 640 { 472 } else { 632 };
    RectPx::new(
        left_margin,
        top_margin + shell_h - LOWER_STRIP_H,
        w,
        LOWER_STRIP_H,
    )
}

/// Owner-draw button snap, round-half-up variant (main menu 0xE2 stacked buttons).
/// `dlu_y` is the resource DLU top; the rect is right-anchored flush to the panel
/// right edge at `cell_w` wide and the DLU top is snapped to the nearest 42-px
/// SDBTNANM row anchored at the button-column top (the SDBTNBKGD tile origin).
pub fn snap_button_round_half_up(dlu_y: i32, panel: RightPanelRects, cell_w: i32) -> RectPx {
    let dlu_top = mul_div_round(dlu_y, DLU_BASE_Y, 8) + panel.top.y;
    let panel_y = panel.tile.y;
    let row_h = RIGHT_PANEL_TILE_H;
    let delta = (dlu_top - panel_y).max(0);
    let q = delta / row_h;
    let rem = delta % row_h;
    let q = if row_h - rem <= rem { q + 1 } else { q };
    let y = q * row_h + panel_y;
    let x = panel.top.x + (RIGHT_PANEL_WIDTH - cell_w);
    RectPx::new(x, y, cell_w, SDBTNANM_CELL_H)
}

/// Owner-draw button snap, biased-truncate variant (single player 0x100 and
/// skirmish 0x102). `source` is the DLU-derived resource rect; the rect is
/// flush-left at `screen_w - center_offset - cell_w`, `cell_w` wide, snapped to a
/// 42-px tile index from the SDBTNBKGD column top via `+tile_h/2` truncation.
pub fn snap_button_biased_truncate(
    screen_w: i32,
    screen_h: i32,
    source: RectPx,
    panel: RightPanelRects,
    cell_w: i32,
) -> RectPx {
    let offset_x = center_offset(screen_w, 800);
    let source_y = source.y + center_offset(screen_h, 600);
    let tile_h = panel.tile.h.max(1);
    let tile_index = ((source_y - panel.tile.y + tile_h / 2) / tile_h).max(0);
    RectPx::new(
        screen_w - offset_x - cell_w,
        panel.tile.y + tile_index * tile_h,
        cell_w,
        SDBTNANM_CELL_H,
    )
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    #[test]
    fn family_caption_fits_the_black_heading_at_native_and_scaled_sizes() {
        let layout = crate::ui::main_menu_shell::compute_layout(800, 600);
        let panel = layout.right_panel;
        let title = layout.title;
        let actual = panel.family_title_caption(title);
        assert_eq!(actual, RectPx::new(659, title.y, 114, title.h));
        for size in [(1600, 1200), (2048, 1532)] {
            let scale = shell_scale(size.0, size.1);
            assert_eq!(
                panel.scaled(scale).family_title_caption(scale.scale_rect(title)),
                scale.scale_rect(actual),
            );
        }
    }

    #[test]
    fn dialog_origin_letterboxes_only_from_1024x768() {
        assert_eq!(dialog_origin(640, 480), (0, 0));
        assert_eq!(dialog_origin(800, 600), (0, 0));
        assert_eq!(dialog_origin(1023, 767), (0, 0));
        // Executed 0x0072EC70 (research origin.out.txt).
        assert_eq!(dialog_origin(1024, 768), (112, 84));
        assert_eq!(dialog_origin(1280, 1024), (240, 212));
    }

    #[test]
    fn snap_biased_truncate_reproduces_single_player_0x100_native_cells() {
        // 0x100 at 800x600: the 156-wide SDBTNANM cell is flush-right at
        // x=644, with rows 199/241/283.
        let panel = right_panel_rects(800, 600);
        let dlu = [
            dlu_rect(425, 122, 108, 23),
            dlu_rect(425, 149, 108, 23),
            dlu_rect(425, 176, 108, 23),
        ];
        let expected_y = [199, 241, 283];
        for (src, ey) in dlu.iter().zip(expected_y) {
            assert_eq!(
                snap_button_biased_truncate(800, 600, *src, panel, SDBTNANM_CELL_W_NARROW),
                RectPx::new(644, ey, 156, 42)
            );
        }
    }
}
