//! CPU-side loading progress-row identity and geometry.
//!
//! Depends only on immutable launch data and integer shell geometry. GPU asset
//! decoding remains in `render::loading_screen_chrome`; draw submission remains
//! in `app::loading`.

use crate::app::loading::composition::{
    NARROW_LOADING_SCREEN_WIDTH, loading_authored_size, loading_base_origin,
};
use crate::skirmish_launch::SkirmishLaunchSession;
use crate::ui::shell::geom::{RectPx, ShellScale};

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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LoadingProgressParticipant {
    pub label: String,
    pub variant: Option<crate::render::loading_screen_chrome::LoadingArtVariant>,
    pub color_index: crate::rules::house_colors::HouseColorIndex,
    pub backing_rgb: [f32; 3],
    pub ramp: [crate::assets::pal_file::Color; 16],
    /// Local progress belongs to the synchronous loading cursor. Only remote
    /// slots have an independently service-updated progress value.
    pub remote_progress: Option<super::pump::LoadingProgressState>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LoadingProgressRowSnapshot {
    pub participants: Vec<LoadingProgressParticipant>,
    pub local_player_index: usize,
}

impl LoadingProgressRowSnapshot {
    /// Redraw643C0B..36 iterates source nodes, not color-sorted Houses.
    pub fn from_launch_session(session: &SkirmishLaunchSession) -> Self {
        Self {
            participants: session
                .humans
                .iter()
                .enumerate()
                .map(|(index, human)| LoadingProgressParticipant {
                    label: human.name.clone(),
                    variant: if human.is_observer() {
                        Some(crate::render::loading_screen_chrome::LoadingArtVariant::Observer)
                    } else {
                        super::pump::loading_art_variant_from_launch_country(human.slot.country)
                    },
                    color_index: crate::rules::house_colors::HouseColorIndex(
                        crate::rules::color_scheme::scheme_entry_for_priority(i32::from(
                            human.slot.color_index,
                        )) as u8,
                    ),
                    backing_rgb: super::pump::FALLBACK_BACKING_RGB,
                    ramp: super::pump::FALLBACK_PROGRESS_RAMP,
                    remote_progress: (index != session.local_player_index)
                        .then(super::pump::LoadingProgressState::standard_skirmish),
                })
                .collect(),
            local_player_index: session.local_player_index,
        }
    }

    #[cfg(test)]
    pub fn local_label(&self) -> &str {
        self.participants
            .get(self.local_player_index)
            .map_or("", |row| row.label.as_str())
    }

    pub fn update_remote_progress(&mut self, source_index: usize, percent: i32) -> bool {
        self.participants
            .get_mut(source_index)
            .and_then(|row| row.remote_progress.as_mut())
            .is_some_and(|progress| progress.set_percent(f64::from(percent)))
    }
    /// Original6433D0 averages every source row, including observers.
    pub(super) fn total_fraction(&self, local: &super::pump::LoadingProgressState) -> f64 {
        self.participants
            .iter()
            .map(|row| {
                row.remote_progress
                    .as_ref()
                    .unwrap_or(local)
                    .current_value()
            })
            .sum::<f64>()
            / self.participants.len() as f64
            / 100.0
    }

    pub(super) fn incomplete_remote_indices(&self) -> Vec<usize> {
        self.participants
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                row.remote_progress
                    .as_ref()
                    .filter(|progress| progress.current_percent() / 100.0 < 0.999)
                    .map(|_| index)
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoadingProgressRowLayout {
    pub bar_origin: [i32; 2],
    pub icon_origin: Option<[i32; 2]>,
    pub label_rect: RectPx,
}

impl LoadingProgressRowLayout {
    /// The row a stretched page draws: the authored meter origin, icon anchor and
    /// label box grown by the same scale as the art around them. Identity on the
    /// retail mapping, which is why the centered-origin cases stay exact.
    pub(crate) fn scaled(self, scale: ShellScale) -> Self {
        Self {
            bar_origin: scale.scale_point(self.bar_origin),
            icon_origin: self.icon_origin.map(|origin| scale.scale_point(origin)),
            label_rect: scale.scale_rect(self.label_rect),
        }
    }
}

/// Original meter placement and row-width policy. The campaign point comes
/// from the already executed `0x00552BE0` layout; it has no width override.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoadingProgressRowPlacement {
    StandardSkirmish([u32; 2]),
    Campaign([i32; 2]),
    ///642C80 auto-centres in72AD20's tactical-sized surface extent.
    AutoCentered([u32; 2]),
}

/// One `DrawPlayerProgressRow @ 0x00643720` geometry implementation shared by
/// campaign and standard offline callers. `0x00642EF0/0x00642E00` supply the
/// frame's six-pixel band and four-pixel row padding; `DrawFill @ 0x00643400`
/// adds three after DrawPlayerProgressRow's five-pixel X inset.
///
/// `font_size` is the initialized font's fifteen-W width and cell height.
/// Side-icon dimensions are absent when the native caller disables the icon.
/// `scale` is the loading screen's mapping: the standard row is measured in the
/// authored viewport and grown by it, while the campaign point arrives already in
/// window pixels from that layout's own owner.
pub(crate) fn layout_loading_progress_row(
    placement: LoadingProgressRowPlacement,
    bar_size: [i32; 2],
    side_icon_size: Option<[i32; 2]>,
    font_size: [i32; 2],
    scale: ShellScale,
    row_index: usize,
) -> LoadingProgressRowLayout {
    let ([mut base_x, mut base_y], row_width_override) = match placement {
        LoadingProgressRowPlacement::StandardSkirmish(render_size) => {
            let authored = loading_authored_size(render_size, scale);
            let [origin_x, origin_y] = loading_base_origin(authored);
            let [offset_x, offset_y, width] = if authored[0] == NARROW_LOADING_SCREEN_WIDTH {
                [NARROW_ROW_OFFSET_X, NARROW_ROW_OFFSET_Y, NARROW_ROW_WIDTH]
            } else {
                [WIDE_ROW_OFFSET_X, WIDE_ROW_OFFSET_Y, WIDE_ROW_WIDTH]
            };
            ([origin_x + offset_x, origin_y + offset_y], Some(width))
        }
        LoadingProgressRowPlacement::Campaign(point) => (point, None),
        LoadingProgressRowPlacement::AutoCentered(_) => ([0, 0], None),
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

    if let LoadingProgressRowPlacement::AutoCentered([width, height]) = placement {
        //642D9B..642DBD ignores72AD20's origin, using width-168,height-32.
        base_x = (width as i32 - 168 - row_width) / 2;
        base_y = (height as i32 - 32 - row_height) / 2;
    }

    // DrawPlayerProgressRow64377D multiplies this measured height by
    // source-player index before the per-element vertical centering.
    let base_y = base_y + row_height * row_index as i32;
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

    let layout = LoadingProgressRowLayout {
        bar_origin,
        icon_origin,
        label_rect,
    };
    match placement {
        LoadingProgressRowPlacement::StandardSkirmish(_) => layout.scaled(scale),
        LoadingProgressRowPlacement::Campaign(_) | LoadingProgressRowPlacement::AutoCentered(_) => {
            layout
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_order_rows_preserve_guest_local_cursor_and_accept_only_peer_progress() {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        let mut guest = launch.local_player().clone();
        guest.raw_slot_index = Some(3);
        guest.name = "Guest".into();
        guest.slot.color_index = 0;
        launch.humans[0].slot.color_index = 6;
        launch.humans.push(guest);
        launch.local_player_index = 1;
        let mut rows = LoadingProgressRowSnapshot::from_launch_session(&launch);
        assert_eq!(
            rows.participants
                .iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Player", "Guest"]
        );
        assert_eq!(rows.local_label(), "Guest");
        assert!(rows.update_remote_progress(0, 45));
        // Packet0F calls643C50 directly, so a peer may report a lower value.
        assert!(rows.update_remote_progress(0, 30));
        assert!(!rows.update_remote_progress(1, 100));
        assert!(!rows.update_remote_progress(2, 100));
        assert_eq!(
            rows.participants[0]
                .remote_progress
                .as_ref()
                .unwrap()
                .current_percent(),
            30.0
        );
        assert!(rows.participants[1].remote_progress.is_none());
    }

    #[test]
    fn native_source_index_advances_by_measured_row_height_before_scaling() {
        let first = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([800, 600]),
            [100, 8],
            Some([18, 20]),
            [135, 17],
            ShellScale::RETAIL,
            0,
        );
        let third = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([800, 600]),
            [100, 8],
            Some([18, 20]),
            [135, 17],
            ShellScale::RETAIL,
            2,
        );
        // 64377D: 2 * (max(insignia20,bar8+6,font17)+padding4).
        assert_eq!(third.bar_origin[1] - first.bar_origin[1], 48);
        assert_eq!(third.label_rect.y - first.label_rect.y, 48);
        assert_eq!(
            third.icon_origin.unwrap()[1] - first.icon_origin.unwrap()[1],
            48
        );
    }

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
                ShellScale::RETAIL,
                0,
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
            ShellScale::RETAIL,
            0,
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
            ShellScale::RETAIL,
            0,
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
            ShellScale::RETAIL,
            0,
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
            ShellScale::RETAIL,
            0,
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
            ShellScale::RETAIL,
            0,
        );
        let maximized = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([1024, 768]),
            [80, 5],
            Some([47, 23]),
            [0, 12],
            ShellScale::RETAIL,
            0,
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
            ShellScale::RETAIL,
            0,
        );
        let just_above = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([641, 480]),
            [80, 5],
            None,
            [0, 12],
            ShellScale::RETAIL,
            0,
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
