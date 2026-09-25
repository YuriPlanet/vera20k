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

/// `(screen - base) / 2` clamped to >= 0. Canonical form; algebraically equal to
/// the single-player `if screen > base` guard and the main-menu inline guard for
/// every i32.
pub fn center_offset(screen: i32, base: i32) -> i32 {
    ((screen - base) / 2).max(0)
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
}
