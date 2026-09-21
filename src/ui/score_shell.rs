//! End-of-match score screen (dialog `0x108`) — layout, model, and state.
//!
//! Render-agnostic: plain geometry plus the already-resolved row values. Part of
//! `ui/`, so it depends on nothing from `render/`, `sim/`, or `assets/` — the
//! caller reads the simulation and hands the finished [`ScoreScreenModel`] over.
//!
//! ## What this screen is
//!
//! A stock skirmish ends by running one modal dialog whose resource template is
//! the SAME `533x369` frame the main menu and single-player shells use: the
//! right-panel chrome, the centred parent background, one owner-draw button in
//! the panel, a right-panel heading, and a bottom-left status-help line. There is
//! no bespoke score background art and no movie panel — the table sits directly on
//! the shell background.
//!
//! The template's control geometry below is transcribed verbatim from the retail
//! `RT_DIALOG` resource (dialog units, `MS Sans Serif` 8pt, base `6x13`), so the
//! DLU numbers are the retail numbers and the pixel rects fall out of the shared
//! `shell::geom` conversion the other three shells already use.
//!
//! ## Verified: the ten band statics are inert
//!
//! The template declares ten full-width band statics behind the rows —
//! `WS_CHILD|WS_VISIBLE|SS_GRAYRECT`, `x=63 cx=293 cy=22`, on the same 22-DLU
//! pitch as the rows (`y=72, 94, 116, ...`). They are intentionally not emitted.
//!
//! Retail's shared static subclass routes them through
//! `OwnerDraw_Static_006153E0`. For unassigned kind `0`, that procedure consumes
//! `WM_PAINT`, ignores the original `SS_GRAYRECT` style, validates the control,
//! and returns without a text or fill draw. `ScoreDialog__WndProc @ 0x005C9B10`
//! does not send the custom `0x4B1` fill message that would change that result.
//! Falling through to USER32 or painting ten synthetic bands would therefore be
//! a visible divergence.

use crate::ui::shell::geom::{
    RIGHT_PANEL_WIDTH, RectPx, RightPanelRects, SDBTNANM_CELL_W_NARROW, center_offset, dlu_rect,
    lower_strip_rect, right_panel_rects, snap_button_biased_truncate,
};

const SHELL_BASE_W: i32 = 800;
const SHELL_BASE_H: i32 = 600;
/// Status-help strip metrics, shared with the single-player shell (`0x100`),
/// whose template declares the same bottom-left static in the same place.
const STATUS_HELP_W: i32 = 456;
const STATUS_HELP_H: i32 = 21;
const STATUS_HELP_BOTTOM_INSET: i32 = 1;

/// Rows the template declares. The dialog has exactly eight player slots; a
/// roster with fewer contenders leaves the remaining slots blank.
pub const SCORE_ROW_SLOTS: usize = 8;

/// Vertical spacing between table rows, in dialog units (row tops 120, 142, ...).
const ROW_PITCH_DLU: i32 = 22;
/// Dialog-unit top of the first player row.
const FIRST_ROW_TOP_DLU: i32 = 120;
/// Dialog-unit top of the column-header row.
const HEADER_TOP_DLU: i32 = 98;
/// Dialog-unit top of the Game / Time summary row.
const SUMMARY_TOP_DLU: i32 = 76;
/// Every table cell is ten dialog units tall.
const CELL_H_DLU: i32 = 10;

/// Column x/width in dialog units, in template order.
const COL_NAME_DLU: (i32, i32) = (66, 75);
const COL_KILLS_DLU: (i32, i32) = (147, 45);
const COL_LOSSES_DLU: (i32, i32) = (202, 45);
const COL_BUILT_DLU: (i32, i32) = (257, 45);
const COL_SCORE_DLU: (i32, i32) = (308, 45);
/// `Game: n` summary label, left-aligned at the head of the table.
const GAME_LABEL_DLU: (i32, i32) = (66, 115);
/// `Time: h:mm:ss` summary label, right-aligned above the Score column.
const TIME_LABEL_DLU: (i32, i32) = (248, 105);
/// Right-panel heading. One dialog unit left of the other shells' heading.
const TITLE_DLU: RectPx = RectPx::new(424, 1, 108, 10);
/// Continue owner-draw button (bottom of the right panel).
const CONTINUE_DLU: RectPx = RectPx::new(425, 326, 108, 23);

/// The five columns of one table row, plus the row's text colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreRowRects {
    pub name: RectPx,
    pub kills: RectPx,
    pub losses: RectPx,
    pub built: RectPx,
    pub score: RectPx,
}

/// Resolved pixel geometry for dialog `0x108` at one screen size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreShellLayout {
    pub screen: RectPx,
    pub title: RectPx,
    pub game_label: RectPx,
    pub time_label: RectPx,
    pub header: ScoreRowRects,
    pub rows: [ScoreRowRects; SCORE_ROW_SLOTS],
    pub continue_button: RectPx,
    pub status_help: RectPx,
    pub right_panel: RightPanelRects,
    pub lower_strip: RectPx,
}

/// One player's line on the score screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreRow {
    pub name: String,
    /// House colour scheme as linear-ready sRGB bytes; the row's text colour.
    pub rgb: [u8; 3],
    pub kills: u32,
    pub losses: u32,
    pub built: u32,
    pub score: i32,
}

/// Everything the score screen displays, resolved once when the match ends.
///
/// Built before the simulation is torn down, because every value it carries comes
/// off the houses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScoreScreenModel {
    /// CSF key for the heading: skirmish and networked games use different ones.
    pub title_key: &'static str,
    /// Sequence number of the finished game within this session (`Game: n`).
    pub game_number: u32,
    /// Match length in whole seconds, already clamped to the native ceiling.
    pub elapsed_seconds: u32,
    /// Rows in display order (score descending), at most [`SCORE_ROW_SLOTS`].
    pub rows: Vec<ScoreRow>,
}

/// Native ceiling on the displayed time: `99:59:59`. gamemd clamps the raw
/// second count to this before splitting it, so a runaway timer cannot widen the
/// field.
pub const MAX_DISPLAY_SECONDS: u32 = 359_999;

impl ScoreScreenModel {
    /// Split the elapsed time into the `h, m, s` triple the time format string
    /// consumes, applying the native clamp first.
    pub fn elapsed_hms(&self) -> (u32, u32, u32) {
        let total = self.elapsed_seconds.min(MAX_DISPLAY_SECONDS);
        (total / 3600, (total % 3600) / 60, total % 60)
    }
}

/// Transient interaction state for the screen's single button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScoreShellState {
    pub continue_pressed: bool,
    pub continue_hovered: bool,
}

fn cell(col: (i32, i32), top_dlu: i32, dx: i32, dy: i32) -> RectPx {
    dlu_rect(col.0, top_dlu, col.1, CELL_H_DLU).translate(dx, dy)
}

fn row_rects(top_dlu: i32, dx: i32, dy: i32) -> ScoreRowRects {
    ScoreRowRects {
        name: cell(COL_NAME_DLU, top_dlu, dx, dy),
        kills: cell(COL_KILLS_DLU, top_dlu, dx, dy),
        losses: cell(COL_LOSSES_DLU, top_dlu, dx, dy),
        built: cell(COL_BUILT_DLU, top_dlu, dx, dy),
        score: cell(COL_SCORE_DLU, top_dlu, dx, dy),
    }
}

/// Right-panel heading anchor. Same convention as the single-player shell: the
/// converted resource rect is sidebar-inset inside the 168px panel and flush to
/// its right edge, with the resource top carried through the centring offset.
fn right_anchor(screen_w: i32, screen_h: i32, original: RectPx) -> RectPx {
    let offset_x = center_offset(screen_w, SHELL_BASE_W);
    let offset_y = center_offset(screen_h, SHELL_BASE_H);
    let inset = (RIGHT_PANEL_WIDTH - original.w) / 2;
    RectPx::new(
        screen_w - offset_x - original.w - inset,
        original.y + offset_y,
        original.w,
        original.h,
    )
}

fn status_help_rect(screen_w: i32, screen_h: i32) -> RectPx {
    let offset_x = center_offset(screen_w, SHELL_BASE_W);
    let offset_y = center_offset(screen_h, SHELL_BASE_H);
    RectPx::new(
        offset_x + 10,
        screen_h - offset_y - STATUS_HELP_H - STATUS_HELP_BOTTOM_INSET,
        STATUS_HELP_W,
        STATUS_HELP_H,
    )
}

pub fn compute_layout(screen_w: u32, screen_h: u32) -> ScoreShellLayout {
    let screen_w = screen_w as i32;
    let screen_h = screen_h as i32;
    // Ordinary (non right-panel) children keep their converted resource rect,
    // shifted by the centring origin — the right-panel anchoring policy moves
    // only the panel's own controls.
    let dx = center_offset(screen_w, SHELL_BASE_W);
    let dy = center_offset(screen_h, SHELL_BASE_H);
    let panel = right_panel_rects(screen_w, screen_h);
    let mut rows = [row_rects(FIRST_ROW_TOP_DLU, dx, dy); SCORE_ROW_SLOTS];
    for (slot, row) in rows.iter_mut().enumerate() {
        *row = row_rects(FIRST_ROW_TOP_DLU + ROW_PITCH_DLU * slot as i32, dx, dy);
    }
    ScoreShellLayout {
        screen: RectPx::new(0, 0, screen_w, screen_h),
        title: right_anchor(screen_w, screen_h, dlu_rect_of(TITLE_DLU)),
        game_label: cell(GAME_LABEL_DLU, SUMMARY_TOP_DLU, dx, dy),
        time_label: cell(TIME_LABEL_DLU, SUMMARY_TOP_DLU, dx, dy),
        header: row_rects(HEADER_TOP_DLU, dx, dy),
        rows,
        continue_button: snap_button_biased_truncate(
            screen_w,
            screen_h,
            dlu_rect_of(CONTINUE_DLU),
            panel,
            SDBTNANM_CELL_W_NARROW,
        ),
        status_help: status_help_rect(screen_w, screen_h),
        right_panel: panel,
        lower_strip: lower_strip_rect(screen_w, screen_h),
    }
}

fn dlu_rect_of(r: RectPx) -> RectPx {
    dlu_rect(r.x, r.y, r.w, r.h)
}

impl ScoreShellLayout {
    /// Hit test for the one interactive control. The button cell is the native
    /// SDBTNANM cell, not the resource rect.
    pub fn hit_continue(&self, x: i32, y: i32) -> bool {
        self.continue_button.contains(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_columns_match_dialog_0x108_template_at_800x600() {
        let layout = compute_layout(800, 600);
        // Column x/width from the retail DLU rects through the shared MulDiv.
        assert_eq!(layout.header.name, RectPx::new(99, 159, 113, 16));
        assert_eq!(layout.header.kills, RectPx::new(221, 159, 68, 16));
        assert_eq!(layout.header.losses, RectPx::new(303, 159, 68, 16));
        assert_eq!(layout.header.built, RectPx::new(386, 159, 68, 16));
        assert_eq!(layout.header.score, RectPx::new(462, 159, 68, 16));
    }

    #[test]
    fn eight_row_slots_step_by_the_template_row_pitch() {
        let layout = compute_layout(800, 600);
        let tops: Vec<i32> = layout.rows.iter().map(|r| r.name.y).collect();
        assert_eq!(tops, vec![195, 231, 267, 302, 338, 374, 410, 445]);
        // Columns are identical on every row.
        for row in &layout.rows {
            assert_eq!(row.name.x, 99);
            assert_eq!(row.score.x, 462);
        }
    }

    #[test]
    fn continue_button_lands_on_the_native_bottom_panel_row() {
        // The Continue resource top (326 DLU) snaps to the same SDBTNANM row the
        // single-player Back button occupies, flush to the panel's right edge.
        assert_eq!(
            compute_layout(800, 600).continue_button,
            RectPx::new(644, 535, 156, 42)
        );
        assert_eq!(
            compute_layout(1024, 768).continue_button,
            RectPx::new(756, 619, 156, 42)
        );
    }

    #[test]
    fn summary_row_sits_above_the_column_headers() {
        let layout = compute_layout(800, 600);
        assert_eq!(layout.game_label, RectPx::new(99, 124, 173, 16));
        assert_eq!(layout.time_label, RectPx::new(372, 124, 158, 16));
        assert!(layout.game_label.y < layout.header.name.y);
    }

    #[test]
    fn large_screen_centers_the_table_and_keeps_native_extents() {
        let base = compute_layout(800, 600);
        let large = compute_layout(1024, 768);
        assert_eq!(large.rows[0].name.x, base.rows[0].name.x + 112);
        assert_eq!(large.rows[0].name.y, base.rows[0].name.y + 84);
        assert_eq!(large.rows[0].name.w, base.rows[0].name.w);
        assert_eq!(large.status_help, RectPx::new(122, 662, 456, 21));
    }

    #[test]
    fn elapsed_clamps_to_the_native_ceiling() {
        let mut model = ScoreScreenModel::default();
        model.elapsed_seconds = 3661;
        assert_eq!(model.elapsed_hms(), (1, 1, 1));
        model.elapsed_seconds = 10_000_000;
        assert_eq!(model.elapsed_hms(), (99, 59, 59));
    }

    #[test]
    fn continue_hit_test_covers_the_button_cell_only() {
        let layout = compute_layout(800, 600);
        assert!(layout.hit_continue(700, 550));
        assert!(!layout.hit_continue(700, 534));
        assert!(!layout.hit_continue(643, 550));
    }
}
