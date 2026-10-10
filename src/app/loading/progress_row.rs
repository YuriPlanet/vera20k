//! CPU-side loading progress-row identity and geometry.
//!
//! Depends only on immutable launch data and integer shell geometry. GPU asset
//! decoding remains in `render::loading_screen_chrome`; draw submission remains
//! in `app::loading`.

use crate::app::loading::composition::{NARROW_LOADING_SCREEN_WIDTH, loading_base_origin};
use crate::skirmish_launch::SkirmishLaunchSession;
use crate::ui::shell::geom::RectPx;

/// Row offsets from the loading-screen base origin. gamemd picks the narrow pair
/// only at exactly 640 screen pixels wide.
const NARROW_ROW_OFFSET_X: i32 = 0x0C;
const NARROW_ROW_OFFSET_Y: i32 = 0x100;
const NARROW_ROW_WIDTH: i32 = 0x146;
const WIDE_ROW_OFFSET_X: i32 = 0x10;
const WIDE_ROW_OFFSET_Y: i32 = 0x141;
const WIDE_ROW_WIDTH: i32 = 0x196;
const BAR_X_HELPER_INSET: i32 = 5 + 3;
const BAR_Y_INSET: i32 = 3;
const BAR_HEIGHT_BAND: i32 = 6;
const ROW_PADDING: i32 = 4;
const SIDE_ICON_GAP: i32 = 0x15;
const LABEL_GAP_AFTER_ICON: i32 = 10;
const LABEL_RIGHT_INSET: i32 = 3;

/// `0x00642E80/0x00642EF0` measure fifteen copies of the original literal
/// `W` at `0x008258C0`, with the initialized GAME.FNT. This width is used only
/// when native leaves the explicit row-width override at -1 (campaign).
pub(crate) const PROGRESS_ROW_MEASURE_TEXT: &str = "WWWWWWWWWWWWWWW";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LoadingProgressRowSnapshot {
    pub label: String,
}

impl LoadingProgressRowSnapshot {
    pub fn from_launch_session(session: &SkirmishLaunchSession) -> Self {
        Self {
            label: session.player_name.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoadingProgressRowLayout {
    pub bar_origin: [i32; 2],
    pub icon_origin: Option<[i32; 2]>,
    pub label_rect: RectPx,
}

/// Original meter placement and row-width policy. The campaign point comes
/// from the already executed `0x00552BE0` layout; it has no width override.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoadingProgressRowPlacement {
    StandardSkirmish([u32; 2]),
    Campaign([i32; 2]),
}

/// One `DrawPlayerProgressRow @ 0x00643720` geometry implementation shared by
/// campaign and standard offline callers. `0x00642EF0/0x00642E00` supply the
/// frame's six-pixel band and four-pixel row padding; `DrawFill @ 0x00643400`
/// adds three after DrawPlayerProgressRow's five-pixel X inset.
///
/// `font_size` is the initialized font's fifteen-W width and cell height.
/// Side-icon dimensions are absent when the native caller disables the icon.
pub(crate) fn layout_loading_progress_row(
    placement: LoadingProgressRowPlacement,
    bar_size: [i32; 2],
    side_icon_size: Option<[i32; 2]>,
    font_size: [i32; 2],
) -> LoadingProgressRowLayout {
    let ([base_x, base_y], row_width_override) = match placement {
        LoadingProgressRowPlacement::StandardSkirmish(render_size) => {
            let [origin_x, origin_y] = loading_base_origin(render_size);
            let [offset_x, offset_y, width] = if render_size[0] == NARROW_LOADING_SCREEN_WIDTH {
                [NARROW_ROW_OFFSET_X, NARROW_ROW_OFFSET_Y, NARROW_ROW_WIDTH]
            } else {
                [WIDE_ROW_OFFSET_X, WIDE_ROW_OFFSET_Y, WIDE_ROW_WIDTH]
            };
            ([origin_x + offset_x, origin_y + offset_y], Some(width))
        }
        LoadingProgressRowPlacement::Campaign(point) => (point, None),
    };
    let bar_width = bar_size[0].max(0);
    let bar_height = bar_size[1].max(0);
    let font_width = font_size[0].max(0);
    let font_height = font_size[1].max(0);
    let side_icon_size = side_icon_size.filter(|size| size[0] > 0 && size[1] > 0);
    let icon_height = side_icon_size.map_or(0, |size| size[1]);
    let row_width = row_width_override
        .unwrap_or_else(|| bar_width + side_icon_size.map_or(0, |size| size[0]) + font_width + 36);
    let row_height = icon_height
        .max(bar_height + BAR_HEIGHT_BAND)
        .max(font_height)
        + ROW_PADDING;

    let bar_origin = [
        base_x + BAR_X_HELPER_INSET,
        base_y + (row_height - (bar_height + BAR_HEIGHT_BAND)) / 2 + BAR_Y_INSET,
    ];
    let icon_x = base_x + bar_width + SIDE_ICON_GAP;
    let icon_origin = side_icon_size.map(|size| [icon_x, base_y + (row_height - size[1]) / 2]);
    let label_x = side_icon_size.map_or(icon_x, |size| icon_x + size[0] + LABEL_GAP_AFTER_ICON);
    let label_y = base_y + (row_height - font_height) / 2;
    let label_right = base_x + row_width - LABEL_RIGHT_INSET;
    let label_rect = RectPx::new(
        label_x,
        label_y,
        (label_right - label_x).max(0),
        font_height,
    );

    LoadingProgressRowLayout {
        bar_origin,
        icon_origin,
        label_rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_row_frame_origin_matches_original_execution() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start.json",
        ))
        .expect("original campaign-start corpus");
        assert_eq!(
            fixture["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        let rows = fixture["progress_rows"]["rows"].as_array().unwrap();
        assert!(!rows.is_empty(), "executed campaign row cases");
        // This is the corpus's supplied font prior, not a retail raster
        // claim: its original GetTextWidth433ED0 measures fifteen W glyphs,
        // each width8 with spacing1, and the declared cell height17.
        let mut font = crate::render::bit_font::tests::make_test_font(&[(b'W' as u16, 8)], 4);
        font.cell_height = 17;
        for row in rows {
            let pair = |name: &str| {
                [
                    row[name][0].as_i64().unwrap() as i32,
                    row[name][1].as_i64().unwrap() as i32,
                ]
            };
            assert_eq!(
                [
                    font.text_width(PROGRESS_ROW_MEASURE_TEXT) as i32,
                    font.cell_height() as i32,
                ],
                pair("font_size"),
                "original row font measure"
            );
            let layout = layout_loading_progress_row(
                LoadingProgressRowPlacement::Campaign(pair("point")),
                pair("bar_size"),
                None,
                pair("font_size"),
            );
            assert_eq!(
                layout.bar_origin,
                [
                    row["clip_rect"][0].as_i64().unwrap() as i32,
                    row["clip_rect"][1].as_i64().unwrap() as i32,
                ],
                "DrawFill's final absolute clip origin: {row}"
            );
            assert_eq!(layout.icon_origin, None);
        }
    }

    #[test]
    fn loading_progress_row_layout_matches_native_640_fixture() {
        let layout = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([640, 480]),
            [80, 5],
            Some([47, 23]),
            [0, 12],
        );

        assert_eq!(layout.bar_origin, [20, 267]);
        assert_eq!(layout.icon_origin, Some([113, 258]));
        assert_eq!(layout.label_rect, RectPx::new(170, 263, 165, 12));
    }

    #[test]
    fn loading_progress_row_layout_matches_native_800_fixture() {
        let layout = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([800, 600]),
            [80, 5],
            Some([47, 23]),
            [0, 12],
        );

        assert_eq!(layout.bar_origin, [24, 332]);
        assert_eq!(layout.icon_origin, Some([117, 323]));
        assert_eq!(layout.label_rect, RectPx::new(174, 328, 245, 12));
    }

    #[test]
    fn missing_icon_uses_would_be_icon_anchor_for_label() {
        let layout = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([640, 480]),
            [80, 5],
            None,
            [0, 12],
        );

        assert_eq!(layout.bar_origin, [20, 261]);
        assert_eq!(layout.icon_origin, None);
        assert_eq!(layout.label_rect, RectPx::new(113, 258, 222, 12));
    }

    #[test]
    fn actual_font_height_can_dominate_the_row() {
        let layout = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([640, 480]),
            [80, 5],
            Some([20, 10]),
            [0, 30],
        );

        assert_eq!(layout.label_rect.y, 258);
        assert_eq!(layout.label_rect.h, 30);
    }

    #[test]
    fn oversized_window_moves_the_row_with_the_centered_art() {
        let base = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([800, 600]),
            [80, 5],
            Some([47, 23]),
            [0, 12],
        );
        let maximized = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([1024, 768]),
            [80, 5],
            Some([47, 23]),
            [0, 12],
        );

        // (1024-800)/2, (768-600)/2 — the same base origin the art and text use.
        assert_eq!(
            maximized.bar_origin,
            [base.bar_origin[0] + 112, base.bar_origin[1] + 84]
        );
        assert_eq!(
            maximized.icon_origin,
            base.icon_origin
                .map(|origin| [origin[0] + 112, origin[1] + 84])
        );
        assert_eq!(maximized.label_rect, base.label_rect.translate(112, 84));
    }

    #[test]
    fn only_exactly_640_selects_the_narrow_row_offsets() {
        let narrow = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([640, 480]),
            [80, 5],
            None,
            [0, 12],
        );
        let just_above = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([641, 480]),
            [80, 5],
            None,
            [0, 12],
        );

        // 641 takes the wide offsets and the 800x600 art viewport, so its base
        // origin is negative on both axes rather than falling back to narrow.
        assert_eq!(narrow.bar_origin, [20, 261]);
        assert_eq!(
            just_above.bar_origin,
            [(641 - 800) / 2 + 24, (480 - 600) / 2 + 326]
        );
    }
}
