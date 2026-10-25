//! Screen-fixed landscape controls. The generated RGBA art is sampled as a
//! single atlas; neither the tactical camera nor Options zoom scales its hit
//! targets. Editing and gesture ownership remain in `ios_shortcuts` and
//! `ios_battle_touch`; this module only paints their current presentation.

use super::ios_battle_touch::TouchRect;
use super::ios_shortcuts::{LandscapeLayout, Shortcut};
use super::state::AppState;
use crate::render::batch::{BatchRenderer, BatchTexture, CameraUniform, SpriteInstance};
use crate::render::gpu::GpuContext;

const ART: &[u8] = include_bytes!("../../art/mobile/landscape-controls.png");
const GLYPH_CELL: u32 = 64;
const GLYPH_COLUMNS: u32 = 8;
const GLYPH_ROWS: u32 = 4;
const ROUNDED_FILL: usize = 16;
const ROUNDED_LINE: usize = 17;
const CIRCLE_FILL: usize = 18;
const CIRCLE_LINE: usize = 19;
const SOLID: usize = 20;

pub(super) struct IosHudArt {
    atlas: BatchTexture,
    glyphs: BatchTexture,
    camera_buffer: wgpu::Buffer,
    camera: wgpu::BindGroup,
}

impl IosHudArt {
    fn new(gpu: &GpuContext, batch: &BatchRenderer) -> Self {
        let atlas = image::load_from_memory_with_format(ART, image::ImageFormat::Png)
            .expect("built-in landscape control atlas is a valid PNG")
            .into_rgba8();
        let glyphs = glyph_atlas();
        let (camera_buffer, camera) = batch.create_ui_camera(&gpu.device);
        Self {
            atlas: batch.create_texture(gpu, atlas.as_raw(), atlas.width(), atlas.height()),
            glyphs: batch.create_texture(
                gpu,
                &glyphs,
                GLYPH_COLUMNS * GLYPH_CELL,
                GLYPH_ROWS * GLYPH_CELL,
            ),
            camera_buffer,
            camera,
        }
    }
}

/// Paint after the match has been copied/upscaled into the physical window.
pub(super) fn render(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
) {
    if !super::ios_controls::enabled(state) {
        return;
    }
    let layout = super::ios_controls::layout(state);
    let controls = &state.match_state.input.ios_controls;
    let theme = crate::app::presentation::sidebar_render::current_sidebar_theme(state);
    let size = [
        state.renderer.gpu.config.width as f32,
        state.renderer.gpu.config.height as f32,
    ];
    let sprites = prepare(&layout, size, &controls.touch, controls.save_failed, theme);
    let tags = super::ios_unit_tags::build(state);
    let renderer = &mut state.renderer;
    let faction = renderer.ios_sidebar_art.get_or_insert_with(|| {
        super::ios_sidebar_art::IosSidebarArt::new(&renderer.gpu, &renderer.batch_renderer)
    });
    let art = renderer
        .ios_hud
        .get_or_insert_with(|| IosHudArt::new(&renderer.gpu, &renderer.batch_renderer));
    let camera = CameraUniform {
        screen_size: size,
        camera_pos: [0.0; 2],
        zoom: 1.0,
        world_origin_y: 0.0,
        world_height: 1.0,
        _pad: 0.0,
        native_z_origin_y: 0.0,
        _native_z_pad: 0.0,
    };
    renderer
        .gpu
        .queue
        .write_buffer(&art.camera_buffer, 0, bytemuck::bytes_of(&camera));
    for (key, instances) in [
        ("ios_touch_tags", &tags),
        ("ios_touch_background", &sprites.background),
        ("ios_touch_strip", &sprites.strip),
        ("ios_touch_art", &sprites.art),
        ("ios_touch_foreground", &sprites.foreground),
    ] {
        renderer.instance_pool.upload(&renderer.gpu, key, instances);
    }
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("iOS landscape touch controls"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &renderer.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    });
    for (key, texture) in [
        ("ios_touch_tags", renderer.bit_font.atlas()),
        ("ios_touch_background", &art.glyphs),
        ("ios_touch_strip", faction.texture(theme)),
        ("ios_touch_art", &art.atlas),
        ("ios_touch_foreground", &art.glyphs),
    ] {
        if let Some((buffer, count)) = renderer.instance_pool.get(key) {
            renderer.batch_renderer.draw_passthrough_range_bound(
                &mut pass,
                texture,
                buffer,
                &art.camera,
                0,
                count,
            );
        }
    }
}

#[derive(Default)]
struct Sprites {
    background: Vec<SpriteInstance>,
    strip: Vec<SpriteInstance>,
    art: Vec<SpriteInstance>,
    foreground: Vec<SpriteInstance>,
}

fn prepare(
    layout: &LandscapeLayout,
    size: [f32; 2],
    touch: &super::ios_battle_touch::BattleTouch,
    save_failed: bool,
    theme: crate::ui::sidebar::SidebarTheme,
) -> Sprites {
    let mut result = Sprites::default();
    let scale = layout.touch.scale_factor;
    if let Some(panel) = layout.editor_panel {
        result.background.push(glyph(
            SOLID,
            rect(0.0, 0.0, size[0], size[1]),
            [0.0; 3],
            0.72,
        ));
        result.background.push(glyph(
            SOLID,
            expanded(panel, 10.0 * scale),
            [0.035, 0.065, 0.085],
            0.99,
        ));
        let frame = expanded(panel, 10.0 * scale);
        for edge in [
            rect(frame.x, frame.y, frame.width, scale),
            rect(frame.x, frame.y + frame.height - scale, frame.width, scale),
            rect(frame.x, frame.y, scale, frame.height),
            rect(frame.x + frame.width - scale, frame.y, scale, frame.height),
        ] {
            result
                .background
                .push(glyph(SOLID, edge, [0.24, 0.47, 0.55], 0.8));
        }
        // A pair of subtle separators keeps the selected slots, edit actions
        // and available catalogue visually distinct without text labels.
        for kind in [0, 1] {
            let y = layout
                .buttons
                .iter()
                .filter(|button| {
                    if kind == 0 {
                        button.command == Shortcut::MoveLeft
                    } else {
                        matches!(button.command, Shortcut::Catalog(_))
                    }
                })
                .map(|button| button.rect.y)
                .reduce(f32::min);
            if let Some(y) = y {
                result.background.push(glyph(
                    SOLID,
                    rect(panel.x, y - 4.0 * scale, panel.width, scale),
                    [0.22, 0.40, 0.46],
                    0.65,
                ));
            }
        }
    }
    let bar_panel = layout.bar_background();
    let bar_top = bar_panel.y;
    // Preserve the outer end cap, but let the center plate meet the sidebar's
    // vertical rail directly. An extra end cap there makes a detached joint
    // and also sits underneath the fixed shortcut editor button.
    let width = bar_panel.width;
    let height = bar_panel.height;
    let cap = (24.0 * scale).min(width * 0.25);
    let image = super::ios_sidebar_art::bottom_region(theme);
    for (x, part_width, u, uv_width) in [(0.0, cap, 0.0, 0.06), (cap, width - cap, 0.06, 0.88)] {
        result.strip.push(quad(
            rect(x, bar_top, part_width, height),
            [image[0] + u * image[2], image[1]],
            [uv_width * image[2], image[3]],
            [1.0; 3],
            1.0,
        ));
    }
    if let Some(joystick) = layout.touch.joystick {
        let radius = joystick.radius;
        result
            .art
            .push(atlas(0, centered(joystick.center, radius * 2.0), 0.88));
        let offset = touch.knob_offset();
        result.art.push(atlas(
            1,
            centered(
                [
                    joystick.center[0] + offset[0],
                    joystick.center[1] + offset[1],
                ],
                radius * 1.12,
            ),
            0.98,
        ));
    }
    for button in &layout.buttons {
        let pressed = touch.is_pressed(button.command);
        let selected = button.selected || pressed;
        let color = match button.icon {
            Shortcut::Save if save_failed => [1.0, 0.25, 0.18],
            Shortcut::Save => [0.44, 0.94, 0.73],
            Shortcut::Discard | Shortcut::Remove => [1.0, 0.64, 0.39],
            _ => [0.77, 0.93, 0.96],
        };
        let opacity = if button.enabled { 1.0 } else { 0.33 };
        if button.icon == Shortcut::Save && save_failed {
            result.foreground.push(glyph(
                ROUNDED_LINE,
                expanded(button.rect, 3.0 * scale),
                [1.0, 0.25, 0.18],
                1.0,
            ));
        }
        let index = button.icon.icon_index();
        let inset_in_bar =
            layout.editor_panel.is_none() && (button.rect.y - layout.bar_button_y()).abs() < 1.0;
        if index < 16 {
            result.art.push(atlas(index, button.rect, opacity));
        } else {
            if !inset_in_bar || selected {
                result.foreground.push(glyph(
                    ROUNDED_FILL,
                    button.rect,
                    if selected {
                        [0.10, 0.26, 0.30]
                    } else {
                        [0.07, 0.13, 0.17]
                    },
                    0.96,
                ));
            }
            if !inset_in_bar {
                result.foreground.push(glyph(
                    ROUNDED_LINE,
                    button.rect,
                    [0.28, 0.46, 0.52],
                    opacity,
                ));
            }
            result
                .foreground
                .push(glyph(index - 16, inset(button.rect, 0.18), color, opacity));
        }
        if selected {
            result.foreground.push(glyph(
                if index < 16 {
                    CIRCLE_LINE
                } else {
                    ROUNDED_LINE
                },
                expanded(button.rect, 2.0 * scale),
                if pressed {
                    [1.0, 0.71, 0.27]
                } else {
                    [0.33, 0.91, 0.97]
                },
                0.95,
            ));
        }
        let team = match button.icon {
            Shortcut::Team1 => 1,
            Shortcut::Team2 => 2,
            Shortcut::Team3 => 3,
            _ => 0,
        };
        let dot = button.rect.width * 0.085;
        for index in 0..team {
            let x = button.rect.x
                + button.rect.width * 0.5
                + (index as f32 - (team - 1) as f32 * 0.5) * dot * 1.65;
            result.foreground.push(glyph(
                CIRCLE_FILL,
                centered([x, button.rect.y + button.rect.height * 0.83], dot),
                [1.0, 0.82, 0.38],
                opacity,
            ));
        }
        if matches!(button.command, Shortcut::Catalog(_)) && button.enabled {
            let badge = centered(
                [
                    button.rect.x + button.rect.width * 0.85,
                    button.rect.y + button.rect.height * 0.16,
                ],
                button.rect.width * 0.25,
            );
            result
                .foreground
                .push(glyph(CIRCLE_FILL, badge, [0.08, 0.37, 0.33], 1.0));
            // An add badge uses two simple strokes, not a textual plus glyph.
            let center = badge.center();
            let length = badge.width * 0.5;
            let stroke = (badge.width * 0.1).max(1.0);
            for (width, height) in [(length, stroke), (stroke, length)] {
                result.foreground.push(glyph(
                    SOLID,
                    rect(
                        center[0] - width / 2.0,
                        center[1] - height / 2.0,
                        width,
                        height,
                    ),
                    [0.75, 1.0, 0.91],
                    1.0,
                ));
            }
        }
    }
    result
}

fn atlas(index: usize, rect: TouchRect, alpha: f32) -> SpriteInstance {
    quad(
        rect,
        [(index % 4) as f32 * 0.25, (index / 4) as f32 * 0.25],
        [0.25; 2],
        [1.0; 3],
        alpha,
    )
}

fn glyph(index: usize, rect: TouchRect, tint: [f32; 3], alpha: f32) -> SpriteInstance {
    quad(
        rect,
        [
            (index % GLYPH_COLUMNS as usize) as f32 / GLYPH_COLUMNS as f32,
            (index / GLYPH_COLUMNS as usize) as f32 / GLYPH_ROWS as f32,
        ],
        [1.0 / GLYPH_COLUMNS as f32, 1.0 / GLYPH_ROWS as f32],
        tint,
        alpha,
    )
}

fn quad(
    rect: TouchRect,
    uv_origin: [f32; 2],
    uv_size: [f32; 2],
    tint: [f32; 3],
    alpha: f32,
) -> SpriteInstance {
    SpriteInstance {
        position: [rect.x, rect.y],
        size: [rect.width, rect.height],
        uv_origin,
        uv_size,
        tint,
        alpha,
        ..Default::default()
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> TouchRect {
    TouchRect {
        x,
        y,
        width,
        height,
    }
}

fn centered(center: [f32; 2], size: f32) -> TouchRect {
    rect(center[0] - size / 2.0, center[1] - size / 2.0, size, size)
}

fn expanded(value: TouchRect, amount: f32) -> TouchRect {
    rect(
        value.x - amount,
        value.y - amount,
        value.width + amount * 2.0,
        value.height + amount * 2.0,
    )
}

fn inset(value: TouchRect, ratio: f32) -> TouchRect {
    rect(
        value.x + value.width * ratio,
        value.y + value.height * ratio,
        value.width * (1.0 - ratio * 2.0),
        value.height * (1.0 - ratio * 2.0),
    )
}

/// A tiny generated mask atlas for editor affordances and panel primitives.
/// White coverage is tinted by the batch instance. Four sub-pixel samples keep
/// diagonals legible at 44pt without introducing fonts or a second art source.
fn glyph_atlas() -> Vec<u8> {
    let width = GLYPH_COLUMNS * GLYPH_CELL;
    let mut rgba = vec![0; (width * GLYPH_ROWS * GLYPH_CELL * 4) as usize];
    for index in 0..GLYPH_COLUMNS * GLYPH_ROWS {
        for y in 0..GLYPH_CELL {
            for x in 0..GLYPH_CELL {
                let mut covered = 0;
                for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    covered +=
                        u8::from(glyph_contains(index as usize, x as f32 + dx, y as f32 + dy));
                }
                let pixel = (((index / GLYPH_COLUMNS * GLYPH_CELL + y) * width
                    + index % GLYPH_COLUMNS * GLYPH_CELL
                    + x)
                    * 4) as usize;
                rgba[pixel..pixel + 4].copy_from_slice(&[
                    255,
                    255,
                    255,
                    (u16::from(covered) * 255 / 4) as u8,
                ]);
            }
        }
    }
    rgba
}

fn glyph_contains(index: usize, x: f32, y: f32) -> bool {
    let point = [x, y];
    let line = |a, b| segment_distance(point, a, b) <= 2.6;
    let radius = ((x - 32.0).powi(2) + (y - 32.0).powi(2)).sqrt();
    let arrow = |right: bool, offset: f32| {
        let center = 32.0 + offset;
        let (tip, tail) = if right {
            (center + 10.0, center - 7.0)
        } else {
            (center - 10.0, center + 7.0)
        };
        line([tail, 16.0], [tip, 32.0]) || line([tip, 32.0], [tail, 48.0])
    };
    match index {
        // Planning: linked waypoints along a route.
        0 => {
            line([12.0, 47.0], [27.0, 18.0])
                || line([27.0, 18.0], [49.0, 39.0])
                || [[12.0, 47.0], [27.0, 18.0], [49.0, 39.0]]
                    .iter()
                    .any(|center| (x - center[0]).hypot(y - center[1]) <= 5.0)
        }
        // Repair: open-jaw wrench and a handle.
        1 => {
            line([16.0, 49.0], [37.0, 28.0])
                || ((x - 43.0).hypot(y - 21.0) >= 9.0
                    && (x - 43.0).hypot(y - 21.0) <= 14.0
                    && !(x > 43.0 && y < 22.0))
                || (x - 16.0).hypot(y - 49.0) <= 5.0
        }
        // Sell: coin returning through an outward arrow.
        2 => {
            ((radius - 20.0).abs() < 2.5 && !(x > 39.0 && y > 33.0))
                || ((radius - 11.0).abs() < 2.3 && x < 40.0)
                || line([33.0, 42.0], [55.0, 42.0])
                || line([47.0, 34.0], [55.0, 42.0])
                || line([55.0, 42.0], [47.0, 50.0])
        }
        3 => {
            (18.0..27.0).contains(&x) && (14.0..50.0).contains(&y)
                || (37.0..46.0).contains(&x) && (14.0..50.0).contains(&y)
        }
        // Settings gear: eight teeth, a ring, and a transparent centre.
        4 => {
            let angle = (y - 32.0).atan2(x - 32.0);
            radius > 8.5 && (radius < 18.0 || (radius < 24.0 && (angle * 4.0).cos().abs() > 0.70))
        }
        5 => line([13.0, 33.0], [26.0, 46.0]) || line([26.0, 46.0], [52.0, 17.0]),
        6 => line([17.0, 17.0], [47.0, 47.0]) || line([47.0, 17.0], [17.0, 47.0]),
        7 => {
            ((radius - 20.0).abs() < 2.7 && !(x < 23.0 && y < 29.0))
                || line([13.0, 12.0], [13.0, 29.0])
                || line([13.0, 29.0], [30.0, 29.0])
        }
        8 => {
            line([17.0, 18.0], [47.0, 18.0])
                || line([26.0, 11.0], [38.0, 11.0])
                || line([22.0, 24.0], [24.0, 51.0])
                || line([24.0, 51.0], [40.0, 51.0])
                || line([40.0, 51.0], [42.0, 24.0])
                || line([29.0, 27.0], [29.0, 44.0])
                || line([35.0, 27.0], [35.0, 44.0])
        }
        9 => arrow(false, 0.0),
        10 => arrow(true, 0.0),
        11 => arrow(false, -8.0) || arrow(false, 9.0),
        12 => arrow(true, -9.0) || arrow(true, 8.0),
        ROUNDED_FILL => rounded_distance(x, y) <= 0.0,
        ROUNDED_LINE => rounded_distance(x, y).abs() <= 1.1,
        CIRCLE_FILL => radius <= 29.0,
        CIRCLE_LINE => (radius - 29.0).abs() <= 1.1,
        SOLID => true,
        _ => false,
    }
}

fn rounded_distance(x: f32, y: f32) -> f32 {
    let q = [(x - 32.0).abs() - 21.0, (y - 32.0).abs() - 21.0];
    q[0].max(0.0).hypot(q[1].max(0.0)) + q[0].max(q[1]).min(0.0) - 8.0
}

fn segment_distance(point: [f32; 2], from: [f32; 2], to: [f32; 2]) -> f32 {
    let vector = [to[0] - from[0], to[1] - from[1]];
    let length = vector[0] * vector[0] + vector[1] * vector[1];
    let t = (((point[0] - from[0]) * vector[0] + (point[1] - from[1]) * vector[1])
        / length.max(1.0))
    .clamp(0.0, 1.0);
    (point[0] - from[0] - t * vector[0]).hypot(point[1] - from[1] - t * vector[1])
}
