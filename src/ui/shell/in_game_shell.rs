//! Active-scenario dialog geometry, from original gamemd.exe 0x0072FC60.
//!
//! This consumes SHP canvas dimensions, without depending on the renderer or
//! assuming all themes have the same art. The complete native procedure is
//! exercised by tools/storage_oracle/in_game_shell_geometry.py; its nine stock
//! cases cover Allied, Soviet and Yuri at 640x480, 800x600 and 1024x768.

use super::geom::RectPx;

/// Canvas dimensions supplied by the active side's loaded art. SIDE2, rather
/// than the separately painted SIDE2B, controls column repetition (0x0072FD96).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InGameShellSizes {
    pub background_small: [i32; 2],
    pub background_medium: [i32; 2],
    pub background_large: [i32; 2],
    pub credits: [i32; 2],
    pub top: [i32; 2],
    pub radar: [i32; 2],
    pub side1: [i32; 2],
    pub side2: [i32; 2],
    pub side3: [i32; 2],
    pub addon: [i32; 2],
    pub bottom_spacer: [i32; 2],
    pub left_cap: [i32; 2],
    pub button_background: [i32; 2],
    pub right_cap: [i32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InGameShellLayout {
    pub background: RectPx,        // B0FC30
    pub credits: RectPx,           // B0FC34
    pub top: RectPx,               // B0FC38
    pub radar: RectPx,             // B0FC3C
    pub top_and_radar: RectPx,     // B0FC40
    pub side1: RectPx,             // B0FC44
    pub side2: RectPx,             // B0FC48, first repeated tile
    pub side3: RectPx,             // B0FC4C; its y is the Back button's lower anchor
    pub addon: RectPx,             // B0FC50
    pub side_column: RectPx,       // B0FC54
    pub bottom_clip: RectPx,       // B0FC58
    pub bottom_spacer: RectPx,     // B0FC5C, LSPACER
    pub left_cap: RectPx,          // B0FC60, expanded command strip
    pub closed_left_cap: RectPx,   // B0FC64, ordinary modal strip
    pub button_background: RectPx, // B0FC68, BTTNBKGD first repeat
    pub right_cap: RectPx,         // B0FC6C
    pub side2_count: i32,
    pub bottom_repeat_count: i32,
    pub bottom_left_x: i32,
    pub bottom_closed_x: i32,
    pub bottom_repeat_span: i32,
}

impl InGameShellLayout {
    /// Native layout for valid loaded art; absent divisor dimensions cannot
    /// produce a usable layout. Stock positive dimensions are oracle-covered.
    pub fn new(width: i32, height: i32, sizes: InGameShellSizes) -> Option<Self> {
        if sizes.side2[1] <= 0 || sizes.button_background[0] <= 0 {
            return None;
        }
        let background_size = match width {
            640 => sizes.background_small,
            800 => sizes.background_medium,
            _ => sizes.background_large,
        };
        let background = RectPx::new(0, 0, background_size[0], background_size[1]);
        // 0x0072FCC2..0x0072FE33: all column rectangles use CREDITS width.
        let column_x = width - sizes.credits[0];
        let column = |y, h| RectPx::new(column_x, y, sizes.credits[0], h);
        let credits = column(0, sizes.credits[1]);
        let top = column(credits.h, sizes.top[1]);
        let radar = column(top.y + top.h, sizes.radar[1]);
        let side1 = column(radar.y + radar.h, sizes.side1[1]);
        let side2 = column(side1.y + side1.h, sizes.side2[1]);
        // 0x0072FE33..0x0072FE9B: signed integer division, then bottom anchors.
        let side2_count = (height - sizes.side3[1] - side2.y) / side2.h;
        let side3 = column(side2.y + side2_count * side2.h, sizes.side3[1]);
        let addon = column(side3.y + side3.h, sizes.addon[1]);
        let bottom_y = height - 32;
        let bottom_clip = RectPx::new(0, bottom_y, width - 168, 32);
        let bottom = |x, size: [i32; 2]| RectPx::new(x, bottom_y, size[0], size[1]);
        let bottom_spacer = bottom(0, sizes.bottom_spacer);
        // 0x0072FFBF..0x00730058 positions lower art from right to left.
        let bottom_repeat_count = (width - sizes.left_cap[0] - credits.w - sizes.right_cap[0])
            / sizes.button_background[0];
        let right_cap = bottom(credits.x - sizes.right_cap[0], sizes.right_cap);
        let bottom_repeat_span = bottom_repeat_count * sizes.button_background[0];
        let button_background = bottom(right_cap.x - bottom_repeat_span, sizes.button_background);
        let left_cap = bottom(button_background.x - sizes.left_cap[0], sizes.left_cap);
        let closed_left_cap = bottom(right_cap.x - sizes.left_cap[0], sizes.left_cap);
        Some(Self {
            background,
            credits,
            top,
            radar,
            top_and_radar: column(top.y, top.h + radar.h),
            side1,
            side2,
            side3,
            addon,
            side_column: column(side1.y, side1.h + side2_count * side2.h + side3.h),
            bottom_clip,
            bottom_spacer,
            left_cap,
            closed_left_cap,
            button_background,
            right_cap,
            side2_count,
            bottom_repeat_count,
            bottom_left_x: left_cap.x,
            bottom_closed_x: closed_left_cap.x,
            bottom_repeat_span,
        })
    }

    /// Active-game branch of original 0x0060B000, 0x0060B124..0x0060B1C1.
    /// `raw` is the original control rectangle in parent-client physical pixels;
    /// the native midpoint and row divisions both truncate toward zero.
    pub fn button_rect(&self, raw: RectPx, button_size: [i32; 2]) -> RectPx {
        let row = (raw.y + raw.h / 2 - 198) / 44;
        RectPx::new(
            self.credits.x + self.credits.w - 147,
            self.side2.y + row * button_size[1],
            button_size[0],
            button_size[1],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::BTreeMap;

    #[derive(Deserialize)]
    struct Asset {
        width: i32,
        height: i32,
    }

    #[derive(Deserialize)]
    struct Case {
        theme: String,
        width: i32,
        height: i32,
        rects: BTreeMap<String, [i32; 4]>,
        side2_count: i32,
        bottom_repeat_count: i32,
        bottom_left_x: i32,
        bottom_closed_x: i32,
        bottom_repeat_span: i32,
    }

    #[derive(Deserialize)]
    struct Fixture {
        assets: BTreeMap<String, BTreeMap<String, Asset>>,
        cases: Vec<Case>,
    }

    #[test]
    fn all_native_stock_geometry_rectangles_and_counts_match() {
        let fixture: Fixture = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/in_game_shell_geometry.json",
        ))
        .unwrap();
        assert_eq!(fixture.cases.len(), 9);
        for case in fixture.cases {
            let assets = &fixture.assets[&case.theme];
            let size = |name: &str| {
                let asset = &assets[name];
                [asset.width, asset.height]
            };
            let layout = InGameShellLayout::new(
                case.width,
                case.height,
                InGameShellSizes {
                    background_small: size("00b0fad4"),
                    background_medium: size("00b0fac8"),
                    background_large: size("00b0fa50"),
                    credits: size("00b0fb08"),
                    top: size("00b0f9e0"),
                    radar: size("00b0fa68"),
                    side1: size("00b0fa70"),
                    side2: size("00b0fafc"),
                    side3: size("00b0fa8c"),
                    addon: size("00b0fa48"),
                    bottom_spacer: size("00b0fa3c"),
                    left_cap: size("00b0fa90"),
                    button_background: size("00b0faa8"),
                    right_cap: size("00b0fabc"),
                },
            )
            .unwrap();
            let actual_rects = [
                layout.background,
                layout.credits,
                layout.top,
                layout.radar,
                layout.top_and_radar,
                layout.side1,
                layout.side2,
                layout.side3,
                layout.addon,
                layout.side_column,
                layout.bottom_clip,
                layout.bottom_spacer,
                layout.left_cap,
                layout.closed_left_cap,
                layout.button_background,
                layout.right_cap,
            ];
            assert_eq!(case.rects.len(), actual_rects.len());
            for (index, rect) in actual_rects.into_iter().enumerate() {
                let address = format!("{:08x}", 0xb0fc30 + index * 4);
                assert_eq!(
                    [rect.x, rect.y, rect.w, rect.h],
                    case.rects[&address],
                    "{} {}x{} {address}",
                    case.theme,
                    case.width,
                    case.height,
                );
            }
            assert_eq!(layout.side2_count, case.side2_count);
            assert_eq!(layout.bottom_repeat_count, case.bottom_repeat_count);
            assert_eq!(layout.bottom_left_x, case.bottom_left_x);
            assert_eq!(layout.bottom_closed_x, case.bottom_closed_x);
            assert_eq!(layout.bottom_repeat_span, case.bottom_repeat_span);
        }
    }
}
