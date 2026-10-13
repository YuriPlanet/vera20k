//! Shared front-end shell geometry: DLU->pixel, right-panel chrome, button snap.
//!
//! Render-agnostic; depends on plain integers only. The three shells (dialogs
//! 0xE2 / 0x100 / 0x102) import these instead of each keeping a private copy.
//! Two distinct owner-draw snap algorithms are preserved (the main menu rounds
//! half-up to the nearest button row; single-player and skirmish bias-truncate a
//! tile index) because they are not proven equivalent at the half-row boundary.

/// Pixel rect in window space. Fields/semantics identical to the three prior
/// per-shell copies; `translate` is hoisted from the skirmish copy so all shells
/// share one type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl RectPx {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn translate(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            w: self.w,
            h: self.h,
        }
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RightPanelRects {
    pub top: RectPx,
    pub tile: RectPx,
    pub tile_count: i32,
    pub bottom: RectPx,
}

impl RightPanelRects {
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

/// The logical shell is authored at 800x600; the fill-window pass scales that
/// rectangle into the live window, keeping its 4:3 shape.
pub const SHELL_BASE_W: i32 = 800;
pub const SHELL_BASE_H: i32 = 600;

/// Corner-rounded uniform mapping from the 800x600 logical shell space into a
/// live window: one factor on both axes, the largest that fits the window, with
/// the scaled shell centred and the spare width or height left to the window's
/// black clear. Identity at the base size, so a fill-window layout equals the
/// retail layout there.
///
/// Holds the window it maps into, not the derived factor and origin, so a
/// layout carrying one stays `Eq` for the shell structs that compare it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellScale {
    screen_w: i32,
    screen_h: i32,
}

impl ShellScale {
    /// The retail mapping: the 800x600 dialog is placed in the window by
    /// [`dialog_origin`], never scaled. Every art fit keyed on this scale is
    /// then a no-op, so the fill-window path is the only source of drift.
    pub const RETAIL: Self = Self {
        screen_w: SHELL_BASE_W,
        screen_h: SHELL_BASE_H,
    };

    pub fn from_screen(screen_w: i32, screen_h: i32) -> Self {
        Self { screen_w, screen_h }
    }

    /// The one factor both axes scale by: the largest at which the 800x600
    /// shell still fits the window. Glyphs, the software cursor and every
    /// rect use it, so the shell keeps its authored proportions.
    pub fn factor(self) -> f32 {
        let fit_w = self.screen_w as f32 / SHELL_BASE_W as f32;
        let fit_h = self.screen_h as f32 / SHELL_BASE_H as f32;
        fit_w.min(fit_h)
    }

    /// Where the scaled 800x600 shell sits in the window: centred, the spare
    /// width or height split between the two sides.
    pub fn dialog_rect(self) -> RectPx {
        let factor = self.factor();
        let w = (SHELL_BASE_W as f32 * factor).round() as i32;
        let h = (SHELL_BASE_H as f32 * factor).round() as i32;
        RectPx::new((self.screen_w - w) / 2, (self.screen_h - h) / 2, w, h)
    }

    /// The whole window this scale maps into. A layout's `screen` is this on
    /// both paths: prompts, the eject box and the input side centre in it.
    pub fn window(self) -> RectPx {
        RectPx::new(0, 0, self.screen_w, self.screen_h)
    }

    /// Where art drawn from the dialog's top-left corner sits in the window:
    /// `retail`, wherever the retail page puts it, on the retail mapping, and
    /// the scaled dialog's corner otherwise.
    pub fn dialog_origin_or(self, retail: (i32, i32)) -> (i32, i32) {
        if self.is_retail() {
            retail
        } else {
            let dialog = self.dialog_rect();
            (dialog.x, dialog.y)
        }
    }

    /// Is this the retail mapping — place the 800x600 shell in the window, never
    /// scale it? Callers that keep a native-size draw for the retail layout
    /// test this instead of inferring it from a rect.
    pub fn is_retail(self) -> bool {
        self.screen_w == SHELL_BASE_W && self.screen_h == SHELL_BASE_H
    }

    pub fn scale_rect(self, rect: RectPx) -> RectPx {
        let factor = self.factor();
        let origin = self.dialog_rect();
        let edge = |authored: i32| (authored as f32 * factor).round() as i32;
        let x0 = origin.x + edge(rect.x);
        let y0 = origin.y + edge(rect.y);
        let x1 = origin.x + edge(rect.x + rect.w);
        let y1 = origin.y + edge(rect.y + rect.h);
        RectPx::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    /// The window-space point an authored point came to sit on, for layers placed
    /// by origin rather than by rectangle — the loading screen's meter and its
    /// country icon. Identity on the retail mapping.
    pub fn scale_point(self, point: [i32; 2]) -> [i32; 2] {
        let rect = self.scale_rect(RectPx::new(point[0], point[1], 0, 0));
        [rect.x, rect.y]
    }

    pub fn scale_panel(self, panel: RightPanelRects) -> RightPanelRects {
        RightPanelRects {
            top: self.scale_rect(panel.top),
            tile: self.scale_rect(panel.tile),
            tile_count: panel.tile_count,
            bottom: self.scale_rect(panel.bottom),
        }
    }

    /// Chrome art authored at `(w, h)` pixels in the 800x600 dialog, in window
    /// space. Identity on the retail mapping.
    pub fn art_size(self, w: f32, h: f32) -> [f32; 2] {
        let factor = self.factor();
        [w * factor, h * factor]
    }

    /// The 800x600-space point a scaled device point came from: the point
    /// form of [`Self::base_rect`]. A cursor mapped through it lets pointer
    /// arithmetic run on the authored pixels the paint path used.
    pub fn base_point(self, x: i32, y: i32) -> (i32, i32) {
        let origin = self.dialog_rect();
        let factor = self.factor();
        let back = |offset: i32| (offset as f32 / factor).round() as i32;
        (back(x - origin.x), back(y - origin.y))
    }

    /// The 800x600-space rect a scaled device rect came from: the inverse of
    /// [`Self::scale_rect`]. Control geometry derived from a device rect maps
    /// through this first, so the insets and row counts run on the authored
    /// numbers the paint and input paths agree on.
    ///
    /// Each edge is mapped back on its own, as `scale_rect` rounded it: a device
    /// edge is within half a pixel of the authored edge times the factor, so
    /// from 1x up every edge, and therefore every authored size an exact-size
    /// art lookup keys on, comes back exactly. Dividing the rounded device
    /// width instead can miss by one below 2x.
    pub fn base_rect(self, rect: RectPx) -> RectPx {
        let (x0, y0) = self.base_point(rect.x, rect.y);
        let (x1, y1) = self.base_point(rect.x + rect.w, rect.y + rect.h);
        RectPx::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Derive a control's sub-rect from its device rect: map back to the
    /// authored numbers, apply `derive` there, then scale the result. Paint
    /// and hit-testing both go through this, so a scaled control's drawn art
    /// and its clickable area are the same rect. The retail mapping hands the
    /// rect to `derive` untouched, keeping every unscaled page byte-identical.
    pub fn derive_rect(self, rect: RectPx, derive: impl FnOnce(RectPx) -> RectPx) -> RectPx {
        if self.is_retail() {
            derive(rect)
        } else {
            self.scale_rect(derive(self.base_rect(rect)))
        }
    }
}

/// Every rect a layout owns, mapped onto the live window in one pass.
///
/// Each impl is an exhaustive struct literal, so a rect field added to a layout
/// is a compile error here until it is stretched too: a page cannot half-scale.
pub trait ScaledRects {
    fn scaled_rects(&self, scale: ShellScale) -> Self;
}

impl ScaledRects for RectPx {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        scale.scale_rect(*self)
    }
}

impl<T: ScaledRects, const N: usize> ScaledRects for [T; N] {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        std::array::from_fn(|index| self[index].scaled_rects(scale))
    }
}

impl<T: ScaledRects> ScaledRects for Option<T> {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        self.as_ref().map(|value| value.scaled_rects(scale))
    }
}

impl ScaledRects for RightPanelRects {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        scale.scale_panel(*self)
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn mul_div_round_matches_round_half_up_muldiv_all_odd_dlu() {
        // Round-half-up MulDiv reference (i64 to avoid overflow), both signs.
        fn reference(n: i64, numer: i64, denom: i64) -> i64 {
            let v = n * numer;
            if v >= 0 {
                (v + denom / 2) / denom
            } else {
                (v - denom / 2) / denom
            }
        }
        for dlu in (-1024..=1024).filter(|d| d % 2 != 0) {
            assert_eq!(
                mul_div_round(dlu, DLU_BASE_X, 4) as i64,
                reference(dlu as i64, DLU_BASE_X as i64, 4)
            );
            assert_eq!(
                mul_div_round(dlu, DLU_BASE_Y, 8) as i64,
                reference(dlu as i64, DLU_BASE_Y as i64, 8)
            );
        }
    }

    #[test]
    fn center_offset_equals_if_guard_form_at_boundaries() {
        for (s, b) in [(800, 800), (801, 800), (1024, 800), (799, 800), (797, 800)] {
            let if_form = if s > b { (s - b) / 2 } else { 0 };
            assert_eq!(center_offset(s, b), if_form, "s={s} b={b}");
        }
    }

    #[test]
    fn right_panel_rects_byte_equal_to_pre_refactor_literals() {
        // Values asserted by the three shells' existing suites pre-refactor.
        let a = right_panel_rects(800, 600);
        assert_eq!(a.top, RectPx::new(632, 0, 168, 199));
        assert_eq!(a.tile, RectPx::new(632, 199, 168, 42));
        assert_eq!(a.tile_count, 9);
        assert_eq!(a.bottom, RectPx::new(632, 577, 168, 23));

        let b = right_panel_rects(1024, 768);
        assert_eq!(b.top, RectPx::new(744, 84, 168, 199));
        assert_eq!(b.tile, RectPx::new(744, 283, 168, 42));
        assert_eq!(b.tile_count, 9);
        assert_eq!(b.bottom, RectPx::new(744, 661, 168, 23));

        let c = right_panel_rects(640, 480);
        assert_eq!(c.top, RectPx::new(472, 0, 168, 199));
        assert_eq!(c.tile, RectPx::new(472, 199, 168, 42));
        assert_eq!(c.tile_count, 6);
        assert_eq!(c.bottom, RectPx::new(472, 451, 168, 29));
    }

    #[test]
    fn snap_round_half_up_reproduces_main_menu_0xe2_stacked_cells() {
        // 0xE2 at 800x600: cell_w=156, flush-right x=644, rows from y=199.
        let panel = right_panel_rects(800, 600);
        let dlu_y = [125, 152, 179, 206, 233];
        let expected_y = [199, 241, 283, 325, 367];
        for (dy, ey) in dlu_y.iter().zip(expected_y) {
            assert_eq!(
                snap_button_round_half_up(*dy, panel, SDBTNANM_CELL_W_NARROW),
                RectPx::new(644, ey, 156, 42)
            );
        }
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

    #[test]
    fn snap_biased_truncate_reproduces_skirmish_0x102_narrow_cells() {
        // 0x102 at 800x600: cell_w=156, flush-right x=644, start/choose 241/283.
        let panel = right_panel_rects(800, 600);
        assert_eq!(
            snap_button_biased_truncate(
                800,
                600,
                dlu_rect(425, 149, 108, 23),
                panel,
                SDBTNANM_CELL_W_NARROW
            ),
            RectPx::new(644, 241, 156, 42)
        );
        assert_eq!(
            snap_button_biased_truncate(
                800,
                600,
                dlu_rect(425, 176, 108, 23),
                panel,
                SDBTNANM_CELL_W_NARROW
            ),
            RectPx::new(644, 283, 156, 42)
        );
    }

    #[test]
    fn lower_strip_matches_pre_refactor_values() {
        assert_eq!(lower_strip_rect(800, 600), RectPx::new(0, 568, 632, 32));
        assert_eq!(lower_strip_rect(1024, 768), RectPx::new(112, 652, 632, 32));
        assert_eq!(lower_strip_rect(640, 480), RectPx::new(0, 448, 472, 32));
    }

    #[test]
    fn fill_window_scale_keeps_the_shell_shape_and_centres_it() {
        // 21:9: the height limits the factor to 2.4, and the 1920-wide shell is
        // centred with 760 px of window on either side.
        let wide = ShellScale::from_screen(3440, 1440);
        assert_eq!(wide.factor(), 2.4);
        assert_eq!(wide.dialog_rect(), RectPx::new(760, 0, 1920, 1440));
        assert_eq!(
            wide.scale_rect(RectPx::new(644, 199, 156, 42)),
            RectPx::new(2306, 478, 374, 100)
        );
        assert_eq!(wide.base_point(2306, 478), (644, 199));

        // A window taller than 4:3 centres the shell vertically instead.
        let tall = ShellScale::from_screen(1200, 1200);
        assert_eq!(tall.factor(), 1.5);
        assert_eq!(tall.dialog_rect(), RectPx::new(0, 150, 1200, 900));

        // A 4:3 window is filled exactly, and 800x600 is the identity.
        assert_eq!(
            ShellScale::from_screen(1600, 1200).dialog_rect(),
            RectPx::new(0, 0, 1600, 1200)
        );
        assert_eq!(
            ShellScale::RETAIL.dialog_rect(),
            RectPx::new(0, 0, SHELL_BASE_W, SHELL_BASE_H)
        );
    }

    #[test]
    fn base_rect_recovers_every_authored_rect_from_1x_up() {
        // Exact-size art (the combo faces, the trackbar frames) is looked up by
        // the authored size read back from a device rect. Dividing the rounded
        // device width missed by one below 2x (480 of these at 1024x768).
        let windows = [
            (1024, 768),
            (1280, 720),
            (1366, 768),
            (1600, 600),
            (1920, 1080),
            (3440, 1440),
        ];
        for (w, h) in windows {
            let scale = ShellScale::from_screen(w, h);
            for x in 0..SHELL_BASE_W {
                for width in [38, 44, 117, 150, 180, 207] {
                    let authored = RectPx::new(x, x % SHELL_BASE_H, width, 23);
                    assert_eq!(
                        scale.base_rect(scale.scale_rect(authored)),
                        authored,
                        "{w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn derive_rect_runs_the_derivation_in_authored_pixels() {
        // A control sub-rect inset by 2 px on each axis, the shape every
        // paint/hit-test derivation in the shells uses.
        fn inset(rect: RectPx) -> RectPx {
            RectPx::new(rect.x + 2, rect.y + 2, rect.w - 4, rect.h - 4)
        }

        // The retail mapping hands the rect to `derive` untouched, so the
        // default path keeps its exact bytes.
        let authored = RectPx::new(100, 200, 207, 23);
        assert_eq!(
            ShellScale::RETAIL.derive_rect(authored, inset),
            inset(authored)
        );

        // A stretched device rect round-trips to the authored numbers, so the 2 px
        // inset is 2 authored pixels (4 device) rather than 2 device pixels: the
        // paint and input paths can never disagree by a scale factor.
        let scale = ShellScale::from_screen(1600, 1200);
        let device = scale.scale_rect(authored);
        assert_eq!(scale.base_rect(device), authored);
        assert_eq!(
            scale.derive_rect(device, inset),
            scale.scale_rect(inset(authored))
        );
        assert_eq!(
            scale.derive_rect(device, inset),
            RectPx::new(204, 404, 406, 38)
        );
    }
}
