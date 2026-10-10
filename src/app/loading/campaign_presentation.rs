//! Pure campaign MISSION loading presentation for the active mode-0 route.
//!
//! `SetupLoadingRects @ 0x00552B10` and `GetProgressMeterPosition @ 0x00552BE0`
//! use the existing loading viewport origin with separate title/body/bar
//! rectangles. Executed integer cases live in
//! `tools/input_oracle/campaign_start.{json,meta.json}`. GPU decoding and text
//! rasterization remain with the existing loading render owners.

use super::composition::{loading_art_viewport_size, loading_base_origin};
use crate::assets::csf_file::CsfFile;
use crate::render::loading_screen_chrome::LoadingScreenWidth;
use crate::rules::campaign_loading::CampaignLoadingMetadata;
use crate::ui::shell::geom::RectPx;

/// The three original campaign rectangles and the meter's base point, in
/// loading-destination pixels. Progress-row frame insets belong to its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CampaignLoadingLayout {
    pub title: RectPx,
    pub body: RectPx,
    pub bar: RectPx,
    pub progress_point: [i32; 2],
}

impl CampaignLoadingLayout {
    pub(crate) const fn for_render_size(render_size: [u32; 2]) -> Self {
        let [x, y] = loading_base_origin(render_size);
        let [art_width, art_height] = loading_art_viewport_size(render_size[0]);
        let title = RectPx::new(x, y, art_width, 40);
        let body = RectPx::new(x, y + 40, art_width, art_height - 80);
        let bar = RectPx::new(x, body.y + body.h, art_width, 40);
        let meter_offset_x = if render_size[0] == super::composition::NARROW_LOADING_SCREEN_WIDTH {
            84
        } else {
            164
        };
        Self {
            title,
            body,
            bar,
            progress_point: [bar.x + meter_offset_x, bar.y + 7],
        }
    }
}

/// Immutable projection of the retained MISSION fields and initialized CSF
/// table for one render size. Rebuild when its metadata, CSF or size changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CampaignLoadingPresentation {
    pub width: LoadingScreenWidth,
    pub layout: CampaignLoadingLayout,
    pub background_name: String,
    pub background_palette_name: String,
    pub title: Option<String>,
    pub briefing: Option<String>,
    pub title_origin: [i32; 2],
    pub briefing_origin: [i32; 2],
}

impl CampaignLoadingPresentation {
    /// `DrawLoading @ 0x00552D60` resolves nonempty labels with the existing
    /// initialized CSF lookup. An empty selected background exits before any
    /// campaign chrome or copy is drawn, including these localized strings.
    pub(crate) fn new(
        metadata: &CampaignLoadingMetadata,
        csf: &CsfFile,
        render_size: [u32; 2],
    ) -> Self {
        let width = LoadingScreenWidth::for_render_width(render_size[0]);
        let (background_name, brief_location) = match width {
            LoadingScreenWidth::W640 => (
                metadata.background_name_640(),
                metadata.brief_location_640(),
            ),
            LoadingScreenWidth::W800 => (
                metadata.background_name_800(),
                metadata.brief_location_800(),
            ),
        };
        let layout = CampaignLoadingLayout::for_render_size(render_size);
        let localize = |key: &str| {
            (!background_name.is_empty() && !key.is_empty()).then(|| csf.text(key).into_owned())
        };
        Self {
            width,
            layout,
            background_name: background_name.to_owned(),
            background_palette_name: metadata.background_palette_name().to_owned(),
            title: localize(metadata.load_message_key()),
            briefing: localize(metadata.load_briefing_key()),
            title_origin: [layout.title.x + 10, layout.title.y + 10],
            briefing_origin: [
                layout.body.x.wrapping_add(brief_location[0]),
                layout.body.y.wrapping_add(brief_location[1]),
            ],
        }
    }

    /// BitFontMeasureText's native width cap is 400 at both resolutions.
    pub(crate) const BRIEFING_WRAP_WIDTH: i32 = 400;

    /// `DrawLoading` expands measured briefing bounds through
    /// `RectAdjust @ 0x0072A9E0` with -4 before the black alpha-159 backing.
    pub(crate) const BRIEFING_BACKING_PADDING: i32 = 4;
}

#[cfg(test)]
mod tests {
    use super::CampaignLoadingLayout;
    use crate::ui::shell::geom::RectPx;
    use serde_json::Value;

    #[test]
    fn campaign_rects_and_progress_origin_match_original_execution() {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start.json",
        ))
        .expect("original campaign-start corpus");
        assert_eq!(
            fixture["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        for row in fixture["loading_geometry"].as_array().unwrap() {
            if row["mode"].as_u64() != Some(0) {
                continue;
            }
            let size = [
                row["width"].as_u64().unwrap() as u32,
                row["height"].as_u64().unwrap() as u32,
            ];
            let layout = CampaignLoadingLayout::for_render_size(size);
            let expected_rect = |name: &str| {
                let rect = row["rects"][name].as_array().unwrap();
                RectPx::new(
                    rect[0].as_i64().unwrap() as i32,
                    rect[1].as_i64().unwrap() as i32,
                    rect[2].as_i64().unwrap() as i32,
                    rect[3].as_i64().unwrap() as i32,
                )
            };
            assert_eq!(layout.title, expected_rect("title"), "{size:?}");
            assert_eq!(layout.body, expected_rect("body"), "{size:?}");
            assert_eq!(layout.bar, expected_rect("bar"), "{size:?}");
            assert_eq!(
                layout.progress_point,
                [
                    row["progress_point"][0].as_i64().unwrap() as i32,
                    row["progress_point"][1].as_i64().unwrap() as i32,
                ],
                "{size:?}"
            );
        }
    }
}
