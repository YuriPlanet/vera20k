//! iOS-only faction chrome painted into the existing sidebar surface.
//!
//! The sidebar projection still owns every hit rectangle, the original cameo
//! and Factory layers still own production feedback, and RadarAnimState still
//! decides whether the real minimap is online. Only their surrounding artwork
//! is replaced here. SidebarFrame applies the same final scale to this surface
//! and to the input rectangles.

use crate::app::AppState;
use crate::app::presentation::sidebar_render::{
    active_minimap_screen_rect, current_sidebar_theme, current_sidebar_view,
};
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::ui::sidebar::{Rect, SidebarTheme, SidebarView};

const ALLIED: &[u8] = include_bytes!("../../art/mobile/sidebar-allied.png");
const SOVIET: &[u8] = include_bytes!("../../art/mobile/sidebar-soviet.png");
const YURI: &[u8] = include_bytes!("../../art/mobile/sidebar-yuri.png");
const GLYPH_CELL: u32 = 64;
const GLYPH_SIDE: u32 = 4;

pub(crate) struct IosSidebarArt {
    themes: [BatchTexture; 3],
    glyphs: BatchTexture,
}

impl IosSidebarArt {
    pub(crate) fn new(gpu: &GpuContext, batch: &BatchRenderer) -> Self {
        let themes = [ALLIED, SOVIET, YURI].map(|bytes| {
            let rgba = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
                .expect("built-in iOS sidebar atlas is valid PNG")
                .into_rgba8();
            batch.create_texture(gpu, rgba.as_raw(), rgba.width(), rgba.height())
        });
        let pixels = glyph_atlas();
        let glyphs = batch.create_texture(
            gpu,
            &pixels,
            GLYPH_SIDE * GLYPH_CELL,
            GLYPH_SIDE * GLYPH_CELL,
        );
        Self { themes, glyphs }
    }

    pub(crate) fn texture(&self, theme: SidebarTheme) -> &BatchTexture {
        &self.themes[theme_index(theme)]
    }

    pub(crate) fn glyphs(&self) -> &BatchTexture {
        &self.glyphs
    }
}

/// Upload at the start of sidebar submission, before its immutable draw borrows.
pub(crate) fn prepare(state: &mut AppState) {
    let Some(view) = current_sidebar_view(state) else {
        return;
    };
    let theme = current_sidebar_theme(state);
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let radar = active_minimap_screen_rect(state);
    let online = state
        .match_state
        .match_presentation
        .radar_anim
        .as_ref()
        .is_none_or(|animation| animation.is_minimap_visible());
    let background = background_instances(view, radar, online, theme, camera);
    let controls = control_instances(view, theme, camera);
    let renderer = &mut state.renderer;
    renderer
        .ios_sidebar_art
        .get_or_insert_with(|| IosSidebarArt::new(&renderer.gpu, &renderer.batch_renderer));
    renderer
        .instance_pool
        .upload(&renderer.gpu, "ios_sidebar_background", &background);
    renderer
        .instance_pool
        .upload(&renderer.gpu, "ios_sidebar_controls", &controls);
}

fn theme_index(theme: SidebarTheme) -> usize {
    match theme {
        SidebarTheme::Allied => 0,
        SidebarTheme::Soviet => 1,
        SidebarTheme::Yuri => 2,
    }
}

fn accent(theme: SidebarTheme) -> [f32; 3] {
    match theme {
        SidebarTheme::Allied => [0.46, 0.84, 1.0],
        SidebarTheme::Soviet => [1.0, 0.47, 0.24],
        SidebarTheme::Yuri => [0.83, 0.56, 1.0],
    }
}

fn background_instances(
    view: &SidebarView,
    radar: Rect,
    online: bool,
    theme: SidebarTheme,
    camera: [f32; 2],
) -> Vec<SpriteInstance> {
    // Generated region boundaries vary slightly between the original images.
    let regions = regions(theme);
    let mut instances = vec![quad(
        view.panel_rect,
        [regions[0][0], regions[0][1]],
        [regions[0][2], regions[0][3]],
        [1.0; 3],
        1.0,
        0.00049,
        camera,
    )];
    // The original sidebar is a stack of continuous housings. Keep that
    // structure: each control row shares one metal strip instead of floating
    // individual button plates above the background.
    for row in control_rows(view) {
        if let Some(bounds) = row_bounds(&row, view.panel_rect) {
            append_metal_row(&mut instances, bounds, regions[1], camera);
        }
    }
    if !online {
        let image = regions[2];
        let scale = (radar.w / image[2]).min(radar.h / image[3]);
        let width = image[2] * scale;
        let height = image[3] * scale;
        instances.push(quad(
            Rect {
                x: radar.x + (radar.w - width) * 0.5,
                y: radar.y + (radar.h - height) * 0.5,
                w: width,
                h: height,
            },
            [image[0], image[1]],
            [image[2], image[3]],
            [1.0; 3],
            1.0,
            0.00047,
            camera,
        ));
    }
    instances
}

/// Normalized [x, y, width, height] of sidebar, toolbar and offline display.
/// The 1254-square generated originals remain unmodified.
fn regions(theme: SidebarTheme) -> [[f32; 4]; 3] {
    let pixels: [[f32; 4]; 3] = match theme {
        SidebarTheme::Allied => [
            [0.0, 0.0, 314.0, 1254.0],
            [315.0, 0.0, 939.0, 158.0],
            [315.0, 159.0, 710.0, 475.0],
        ],
        SidebarTheme::Soviet => [
            [0.0, 0.0, 284.0, 1254.0],
            [285.0, 0.0, 969.0, 148.0],
            [286.0, 149.0, 738.0, 447.0],
        ],
        SidebarTheme::Yuri => [
            [0.0, 0.0, 303.0, 1254.0],
            [304.0, 0.0, 950.0, 142.0],
            [304.0, 143.0, 777.0, 430.0],
        ],
    };
    pixels.map(|rect| rect.map(|value| value / 1254.0))
}

pub(super) fn bottom_region(theme: SidebarTheme) -> [f32; 4] {
    regions(theme)[1]
}

fn control_rows(view: &SidebarView) -> [Vec<Rect>; 4] {
    [
        view.top_buttons.iter().map(|button| button.rect).collect(),
        vec![view.repair_button.rect, view.sell_button.rect],
        view.tabs.iter().map(|button| button.rect).collect(),
        vec![view.scroll_down_button.rect, view.scroll_up_button.rect],
    ]
}

/// A small skirt joins the housing to the column, but never enters the radar
/// or cameo rows: the shared iOS layout leaves at least three pixels there.
fn row_bounds(buttons: &[Rect], panel: Rect) -> Option<Rect> {
    let first = buttons.iter().find(|rect| rect.w > 0.0 && rect.h > 0.0)?;
    let [mut left, mut top, mut right, mut bottom] =
        [first.x, first.y, first.x + first.w, first.y + first.h];
    for button in buttons.iter().filter(|rect| rect.w > 0.0 && rect.h > 0.0) {
        left = left.min(button.x);
        top = top.min(button.y);
        right = right.max(button.x + button.w);
        bottom = bottom.max(button.y + button.h);
    }
    left = (left - 2.0).max(panel.x);
    top = (top - 1.0).max(panel.y);
    right = (right + 2.0).min(panel.x + panel.w);
    bottom = (bottom + 1.0).min(panel.y + panel.h);
    (right > left && bottom > top).then_some(Rect {
        x: left,
        y: top,
        w: right - left,
        h: bottom - top,
    })
}

/// Horizontal three-slice preserves the authored metal end caps while its
/// middle section spans the complete row. All regions share a square atlas,
/// so the cap's UV width is directly comparable to its UV height.
fn append_metal_row(
    instances: &mut Vec<SpriteInstance>,
    row: Rect,
    region: [f32; 4],
    camera: [f32; 2],
) {
    let uv_cap = (region[3] * 0.5).min(region[2] / 3.0);
    let cap = (row.h * 0.5).min(row.w / 3.0);
    for (x, width, uv_x, uv_width) in [
        (row.x, cap, region[0], uv_cap),
        (
            row.x + cap,
            row.w - cap * 2.0,
            region[0] + uv_cap,
            region[2] - uv_cap * 2.0,
        ),
        (
            row.x + row.w - cap,
            cap,
            region[0] + region[2] - uv_cap,
            uv_cap,
        ),
    ] {
        instances.push(quad(
            Rect {
                x,
                y: row.y,
                w: width,
                h: row.h,
            },
            [uv_x, region[1]],
            [uv_width, region[3]],
            [0.78; 3],
            1.0,
            0.000475,
            camera,
        ));
    }
}

fn control_instances(
    view: &SidebarView,
    theme: SidebarTheme,
    camera: [f32; 2],
) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();
    let accent = accent(theme);
    for mut row in control_rows(view) {
        row.retain(|button| button.w > 0.0 && button.h > 0.0);
        row.sort_by(|left, right| left.x.total_cmp(&right.x));
        for pair in row.windows(2) {
            let x = (pair[0].x + pair[0].w + pair[1].x) * 0.5;
            let top = pair[0].y.max(pair[1].y) + 3.0;
            let bottom = (pair[0].y + pair[0].h).min(pair[1].y + pair[1].h) - 3.0;
            if bottom > top {
                instances.push(glyph(
                    12,
                    Rect {
                        x,
                        y: top,
                        w: 1.0,
                        h: bottom - top,
                    },
                    [0.005, 0.01, 0.015],
                    0.8,
                    0.00046,
                    camera,
                ));
                instances.push(glyph(
                    12,
                    Rect {
                        x: x + 1.0,
                        y: top,
                        w: 1.0,
                        h: bottom - top,
                    },
                    accent,
                    0.16,
                    0.00045,
                    camera,
                ));
            }
        }
    }
    for tab in &view.tabs {
        append_button(
            &mut instances,
            tab.rect,
            tab.tab.tab_index(),
            tab.active,
            tab.disabled,
            accent,
            camera,
        );
    }
    for (index, button) in view.top_buttons.iter().enumerate() {
        append_button(
            &mut instances,
            button.rect,
            index + 4,
            button.frame_index == 1,
            button.disabled,
            accent,
            camera,
        );
    }
    for (button, icon) in [(&view.repair_button, 6), (&view.sell_button, 7)] {
        append_button(
            &mut instances,
            button.rect,
            icon,
            button.active,
            button.disabled,
            accent,
            camera,
        );
    }
    for (button, icon) in [(&view.scroll_down_button, 8), (&view.scroll_up_button, 9)] {
        append_button(
            &mut instances,
            button.rect,
            icon,
            button.frame_index == 1,
            button.disabled,
            accent,
            camera,
        );
    }
    instances
}

fn append_button(
    instances: &mut Vec<SpriteInstance>,
    rect: Rect,
    icon: usize,
    selected: bool,
    disabled: bool,
    accent: [f32; 3],
    camera: [f32; 2],
) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let opacity = if disabled { 0.32 } else { 1.0 };
    if selected {
        // A pressed-in dark well and one narrow light on its inner lower
        // edge read as part of the row housing, rather than a separate frame.
        instances.push(glyph(
            12,
            Rect {
                x: rect.x + 2.0,
                y: rect.y + 2.0,
                w: (rect.w - 4.0).max(0.0),
                h: (rect.h - 4.0).max(0.0),
            },
            [0.015, 0.025, 0.035],
            0.82,
            0.00046,
            camera,
        ));
        instances.push(glyph(
            12,
            Rect {
                x: rect.x + 4.0,
                y: rect.y + rect.h - 3.0,
                w: (rect.w - 8.0).max(0.0),
                h: 1.0,
            },
            accent,
            0.7,
            0.00045,
            camera,
        ));
    }
    let size = rect.w.min(rect.h) * 0.82;
    let icon_rect = Rect {
        x: rect.x + (rect.w - size) * 0.5,
        y: rect.y + (rect.h - size) * 0.5,
        w: size,
        h: size,
    };
    instances.push(glyph(
        icon,
        icon_rect,
        if selected {
            [1.0, 0.91, 0.65]
        } else {
            [0.82, 0.93, 0.96]
        },
        opacity,
        0.00044,
        camera,
    ));
}

fn glyph(
    index: usize,
    rect: Rect,
    tint: [f32; 3],
    alpha: f32,
    depth: f32,
    camera: [f32; 2],
) -> SpriteInstance {
    quad(
        rect,
        [
            (index % GLYPH_SIDE as usize) as f32 / GLYPH_SIDE as f32,
            (index / GLYPH_SIDE as usize) as f32 / GLYPH_SIDE as f32,
        ],
        [1.0 / GLYPH_SIDE as f32; 2],
        tint,
        alpha,
        depth,
        camera,
    )
}

fn quad(
    rect: Rect,
    uv_origin: [f32; 2],
    uv_size: [f32; 2],
    tint: [f32; 3],
    alpha: f32,
    depth: f32,
    camera: [f32; 2],
) -> SpriteInstance {
    SpriteInstance {
        position: [rect.x + camera[0], rect.y + camera[1]],
        size: [rect.w, rect.h],
        uv_origin,
        uv_size,
        tint,
        alpha,
        depth,
        ..Default::default()
    }
}

/// Text-free control silhouettes. Four sub-pixel coverage samples retain
/// legible diagonals when SidebarFrame scales these native-surface pixels.
fn glyph_atlas() -> Vec<u8> {
    let width = GLYPH_SIDE * GLYPH_CELL;
    let mut rgba = vec![0; (width * width * 4) as usize];
    for index in 0..GLYPH_SIDE * GLYPH_SIDE {
        for y in 0..GLYPH_CELL {
            for x in 0..GLYPH_CELL {
                let mut count = 0_u16;
                for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    count += u16::from(glyph_contains(index, x as f32 + dx, y as f32 + dy));
                }
                let pixel = (((index / GLYPH_SIDE * GLYPH_CELL + y) * width
                    + index % GLYPH_SIDE * GLYPH_CELL
                    + x)
                    * 4) as usize;
                rgba[pixel..pixel + 4].copy_from_slice(&[255, 255, 255, (count * 255 / 4) as u8]);
            }
        }
    }
    rgba
}

fn glyph_contains(index: u32, x: f32, y: f32) -> bool {
    let line = |from: [f32; 2], to: [f32; 2]| segment_distance([x, y], from, to) <= 2.4;
    let box_in = |left: f32, top: f32, right: f32, bottom: f32| {
        x >= left && x <= right && y >= top && y <= bottom
    };
    let radius = (x - 32.0).hypot(y - 32.0);
    match index {
        // Building: factory roof and chimney, with a doorway cutout.
        0 => {
            (box_in(12.0, 28.0, 51.0, 52.0) && !box_in(28.0, 39.0, 37.0, 53.0))
                || box_in(43.0, 13.0, 50.0, 32.0)
                || line([12.0, 28.0], [27.0, 18.0])
                || line([27.0, 18.0], [27.0, 30.0])
        }
        // Defence: a shield and targeting cross.
        1 => {
            line([16.0, 16.0], [32.0, 10.0])
                || line([32.0, 10.0], [48.0, 16.0])
                || line([16.0, 16.0], [19.0, 39.0])
                || line([48.0, 16.0], [45.0, 39.0])
                || line([19.0, 39.0], [32.0, 53.0])
                || line([32.0, 53.0], [45.0, 39.0])
                || line([24.0, 29.0], [40.0, 29.0])
                || line([32.0, 21.0], [32.0, 37.0])
        }
        // Infantry: helmet above shoulders.
        2 => {
            ((x - 32.0).hypot(y - 25.0) <= 12.0 && y >= 18.0)
                || ((x - 32.0).hypot(y - 24.0) <= 15.0 && y <= 25.0)
                || line([15.0, 26.0], [49.0, 26.0])
                || (box_in(15.0, 41.0, 49.0, 53.0) && (x - 32.0).abs() + (44.0 - y).max(0.0) < 20.0)
        }
        // Vehicle: tracked hull and turret.
        3 => {
            box_in(13.0, 34.0, 51.0, 45.0)
                || box_in(25.0, 25.0, 41.0, 35.0)
                || line([34.0, 27.0], [54.0, 19.0])
                || line([17.0, 50.0], [47.0, 50.0])
                || (x - 16.0).hypot(y - 45.0) < 6.0
                || (x - 48.0).hypot(y - 45.0) < 6.0
        }
        // Diplomacy: two linked hands, separated wrist marks.
        4 => {
            line([10.0, 36.0], [27.0, 20.0])
                || line([27.0, 20.0], [43.0, 36.0])
                || line([21.0, 36.0], [36.0, 21.0])
                || line([36.0, 21.0], [54.0, 38.0])
                || line([16.0, 35.0], [29.0, 48.0])
                || line([29.0, 48.0], [47.0, 32.0])
                || line([8.0, 28.0], [18.0, 18.0])
                || line([47.0, 20.0], [57.0, 30.0])
        }
        5 => {
            let angle = (y - 32.0).atan2(x - 32.0);
            radius > 8.0 && (radius < 18.0 || (radius < 24.0 && (angle * 4.0).cos().abs() > 0.7))
        }
        6 => {
            line([16.0, 49.0], [37.0, 28.0])
                || ((x - 43.0).hypot(y - 21.0) >= 9.0
                    && (x - 43.0).hypot(y - 21.0) <= 14.0
                    && !(x > 43.0 && y < 22.0))
                || (x - 16.0).hypot(y - 49.0) <= 5.0
        }
        7 => {
            ((radius - 20.0).abs() < 2.5 && !(x > 39.0 && y > 33.0))
                || ((radius - 11.0).abs() < 2.3 && x < 40.0)
                || line([33.0, 42.0], [55.0, 42.0])
                || line([47.0, 34.0], [55.0, 42.0])
                || line([55.0, 42.0], [47.0, 50.0])
        }
        8 => line([17.0, 24.0], [32.0, 40.0]) || line([32.0, 40.0], [47.0, 24.0]),
        9 => line([17.0, 40.0], [32.0, 24.0]) || line([32.0, 24.0], [47.0, 40.0]),
        10 => rounded_distance(x, y) <= 0.0,
        11 => rounded_distance(x, y).abs() <= 1.2,
        12 => true,
        _ => false,
    }
}

fn rounded_distance(x: f32, y: f32) -> f32 {
    let q = [(x - 32.0).abs() - 23.0, (y - 32.0).abs() - 23.0];
    q[0].max(0.0).hypot(q[1].max(0.0)) + q[0].max(q[1]).min(0.0) - 7.0
}

fn segment_distance(point: [f32; 2], from: [f32; 2], to: [f32; 2]) -> f32 {
    let vector = [to[0] - from[0], to[1] - from[1]];
    let length = vector[0] * vector[0] + vector[1] * vector[1];
    let t = (((point[0] - from[0]) * vector[0] + (point[1] - from[1]) * vector[1])
        / length.max(1.0))
    .clamp(0.0, 1.0);
    (point[0] - from[0] - t * vector[0]).hypot(point[1] - from[1] - t * vector[1])
}
