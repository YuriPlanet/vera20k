//! Native radar screen mapping and tactical camera-window overlay.
//!
//! This presentation-only authority composes the generated primary surface
//! established by `RebuildRadarSurfaces @ 0x00654650` with the ordinary
//! sidebar aperture. It owns both the click inverse (`0x006539D0/0x00656750`)
//! and the viewport outline (`RadarClass::Update @ 0x00656EC0`) so those paths
//! cannot drift into independent 200x200 transforms.

use crate::render::batch::SpriteInstance;
use crate::util::native_x87::{NativeF32Bits, X87Chop53, X87Value};

use super::geometry::{NativeRadarSurfaceGeometry, RADAR_APERTURE_HEIGHT, RADAR_APERTURE_WIDTH};
use super::helpers::MINIMAP_DEPTH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NativeRadarRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Generated-primary placement inside one screen-space 140x108 aperture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct NativeRadarScreenGeometry {
    surface: NativeRadarSurfaceGeometry,
    aperture: [f32; 4],
}

impl NativeRadarScreenGeometry {
    pub const fn new(surface: NativeRadarSurfaceGeometry, aperture: [f32; 4]) -> Self {
        Self { surface, aperture }
    }

    pub fn content_rect(self) -> [f32; 4] {
        let scale = self.surface_scale();
        let offset = self.surface.aperture_offset();
        let size = self.surface.generated_size();
        [
            self.aperture[0] + offset.0 as f32 * scale.0,
            self.aperture[1] + offset.1 as f32 * scale.1,
            size.0 as f32 * scale.0,
            size.1 as f32 * scale.1,
        ]
    }

    /// Native screen coordinates are integral and the live surface bounds are
    /// lower-inclusive/upper-exclusive. VERA's optional UI scale is an outer
    /// adapter; rounded destination pixels are distributed monotonically over
    /// the unchanged native generated coordinate range.
    pub fn screen_to_surface_pixel(self, screen: (f32, f32)) -> Option<(i32, i32)> {
        let rect = self.content_rect();
        let left = rect[0].round() as i32;
        let top = rect[1].round() as i32;
        let width = rect[2].round() as i32;
        let height = rect[3].round() as i32;
        let x = screen.0.round() as i32;
        let y = screen.1.round() as i32;
        let dx = x.wrapping_sub(left);
        let dy = y.wrapping_sub(top);
        if (dx as u32) >= width as u32 || (dy as u32) >= height as u32 {
            return None;
        }
        let size = self.surface.generated_size();
        Some((
            dx.wrapping_mul(size.0) / width.max(1),
            dy.wrapping_mul(size.1) / height.max(1),
        ))
    }

    /// VERA's HD radar: the pixel of the surface generated at `scale` times
    /// the size under an exact layout `point`, clamped onto that surface. The
    /// native pick above decides whether the point is on the radar at all.
    pub fn fine_surface_pixel(self, point: (f32, f32), scale: u32) -> (i32, i32) {
        let rect = self.content_rect();
        let size = self.surface.subdivided(scale).generated_size();
        let pick = |point: f32, start: f32, extent: f32, count: i32| {
            let count = count.max(1);
            let offset = (point - start.round()) * count as f32 / extent.round().max(1.0);
            (offset.floor() as i32).clamp(0, count - 1)
        };
        (
            pick(point.0, rect[0], rect[2], size.0),
            pick(point.1, rect[1], rect[3], size.1),
        )
    }

    /// Resolve an actual destination pixel which the existing inverse maps
    /// back to this generated pixel. A downscaled, unrepresented pixel has no
    /// clickable point; do not silently select its neighbor.
    pub fn surface_pixel_screen_point(self, pixel: (i32, i32)) -> Option<(f32, f32)> {
        let rect = self.content_rect();
        let size = self.surface.generated_size();
        if pixel.0 < 0 || pixel.1 < 0 || pixel.0 >= size.0 || pixel.1 >= size.1 {
            return None;
        }
        let extent = (rect[2].round() as i32, rect[3].round() as i32);
        let point = (
            rect[0].round() + ((pixel.0 * extent.0 + size.0 - 1) / size.0) as f32,
            rect[1].round() + ((pixel.1 * extent.1 + size.1 - 1) / size.1) as f32,
        );
        (self.screen_to_surface_pixel(point) == Some(pixel)).then_some(point)
    }

    pub fn surface_scale(self) -> (f32, f32) {
        (
            self.aperture[2] / RADAR_APERTURE_WIDTH as f32,
            self.aperture[3] / RADAR_APERTURE_HEIGHT as f32,
        )
    }
}

/// `RadarClass::Update @ 0x00656F5E..0x00657117` camera-window rectangle in
/// generated-primary coordinates. The x origin-minus-one fix lives in
/// `cell_to_surface_pixel`; sizes preserve the native x87 operation order.
pub(super) fn native_viewport_rect(
    surface: NativeRadarSurfaceGeometry,
    center_cell: (u16, u16),
    tactical_width: i32,
    tactical_height: i32,
) -> NativeRadarRect {
    let center = surface.cell_to_surface_pixel(center_cell);
    native_viewport_rect_from_center(surface, center, tactical_width, tactical_height)
}

fn native_viewport_rect_from_center(
    surface: NativeRadarSurfaceGeometry,
    center: (i32, i32),
    tactical_width: i32,
    tactical_height: i32,
) -> NativeRadarRect {
    let zoom = load_f32(surface.zoom());
    let denom_x = div(load_constant(60.0), zoom);
    // `0x00657031..0x00657054` executes FSTP dword before either vertical
    // division. Keeping the quotient in extended precision changes boundary
    // results (for example raw 19x10, tactical height 570: 280 instead of
    // native 279).
    let denom_y = load_f32(store_f32(div(load_constant(30.0), zoom)));
    let width_twice = add(load_i32(tactical_width), load_i32(tactical_width));
    let height_twice = add(load_i32(tactical_height), load_i32(tactical_height));
    let w = native_ftol(add(div(width_twice, denom_x), load_constant(1.0)));
    let h_float = div(height_twice, denom_y);
    let h = native_ftol(h_float);
    let mut x = center
        .0
        .wrapping_sub(native_ftol(div(load_i32(tactical_width), denom_x)));
    let mut y = center
        .1
        .wrapping_sub(native_ftol(mul(h_float, load_constant(0.5))));
    let size = surface.generated_size();

    if x < 0 {
        x = 0;
    } else if size.0 <= x.wrapping_add(w) {
        x = size.0.wrapping_sub(1).wrapping_sub(w);
    }
    if y < 0 {
        y = 0;
    } else if size.1 <= y.wrapping_add(h) {
        y = size.1.wrapping_sub(1).wrapping_sub(h);
    }
    NativeRadarRect { x, y, w, h }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeRadarViewportTransition {
    pub previous: NativeRadarRect,
    pub current: NativeRadarRect,
    pub dirty_previous_border: Vec<(i32, i32)>,
}

/// Client-local current/previous `RadarClass+0x14DC..0x14F8` lifecycle.
///
/// VERA clears and redraws the GPU target every frame, so old border pixels are
/// already output-equivalently restored. The exact native dirty visit list is
/// nevertheless retained here to pin chronology and avoid losing it if the
/// sidebar becomes a retained software surface later.
#[derive(Debug, Clone, Default)]
pub(super) struct NativeRadarViewportState {
    current: Option<NativeRadarRect>,
    previous: Option<NativeRadarRect>,
    force_redraw: bool,
}

impl NativeRadarViewportState {
    pub fn reset_for_rebuild(&mut self) {
        self.current = None;
        self.previous = None;
        self.force_redraw = true;
    }

    pub fn update(&mut self, current: NativeRadarRect) -> NativeRadarViewportTransition {
        let previous = if self.force_redraw {
            current
        } else {
            self.previous.unwrap_or(current)
        };
        let dirty_previous_border = if previous == current {
            Vec::new()
        } else {
            native_previous_border_visits(previous)
        };
        self.current = Some(current);
        // Update copies +0x14DC..+0x14E8 over +0x14EC..+0x14F8 after draw.
        self.previous = Some(current);
        self.force_redraw = false;
        NativeRadarViewportTransition {
            previous,
            current,
            dirty_previous_border,
        }
    }
}

fn native_previous_border_visits(rect: NativeRadarRect) -> Vec<(i32, i32)> {
    let mut visits =
        Vec::with_capacity((rect.w.wrapping_add(rect.h).wrapping_mul(2)).max(0) as usize);
    for y in 0..rect.h.max(0) {
        visits.push((rect.x, rect.y.wrapping_add(y)));
        visits.push((
            rect.x.wrapping_add(rect.w).wrapping_sub(1),
            rect.y.wrapping_add(y),
        ));
    }
    for x in 0..rect.w.max(0) {
        visits.push((rect.x.wrapping_add(x), rect.y));
        visits.push((
            rect.x.wrapping_add(x),
            rect.y.wrapping_add(rect.h).wrapping_sub(1),
        ));
    }
    visits
}

pub(super) fn native_viewport_outline_instances(
    camera: (f32, f32),
    screen: NativeRadarScreenGeometry,
    rect: NativeRadarRect,
    sidebar_surface: [f32; 4],
    tint: [f32; 3],
) -> Vec<SpriteInstance> {
    native_radar_outline_instances(camera, screen, rect, sidebar_surface, tint)
}

/// `RadarClass::Update @ 0x00657669..0x006576A2` final retained-radar write.
///
/// Native submits `(origin_x - 1, origin_y - 1, generated_w + 2,
/// generated_h + 2)` after the camera-window outline. Keeping this stateless
/// means an ordinary redraw, a force-reset, and an action-40 surface rebuild
/// all consume the currently installed generated-primary dimensions.
pub(super) fn native_content_boundary_outline_instances(
    camera: (f32, f32),
    screen: NativeRadarScreenGeometry,
    surface: NativeRadarSurfaceGeometry,
    sidebar_surface: [f32; 4],
    tint: [f32; 3],
) -> Vec<SpriteInstance> {
    let size = surface.generated_size();
    native_radar_outline_instances(
        camera,
        screen,
        NativeRadarRect {
            x: -1,
            y: -1,
            w: size.0.wrapping_add(2),
            h: size.1.wrapping_add(2),
        },
        sidebar_surface,
        tint,
    )
}

fn native_radar_outline_instances(
    camera: (f32, f32),
    screen: NativeRadarScreenGeometry,
    rect: NativeRadarRect,
    sidebar_surface: [f32; 4],
    tint: [f32; 3],
) -> Vec<SpriteInstance> {
    let left = rect.x;
    let top = rect.y;
    let right = left.wrapping_add(rect.w).wrapping_sub(1);
    let bottom = top.wrapping_add(rect.h).wrapping_sub(1);

    // Rectangle worker 0x007BADC0 emits top, right, bottom, left in that
    // order. XSurface line slot 0x007BA610 clips each inclusive segment to
    // g_SidebarSurface before SidebarClass copies/scales that retained raster.
    // Reproduce that architecture by selecting the integer destination pixels
    // whose centres nearest-sample the source line. Directly submitting a
    // scaled-width float quad is not equivalent: batch_shader rounds its two
    // vertices independently and a 0.5-source-pixel quad can collapse.
    let edges = [
        ([left, top], [right, top]),
        ([right, top], [right, bottom]),
        ([right, bottom], [left, bottom]),
        ([left, bottom], [left, top]),
    ];
    let scale = screen.surface_scale();
    if scale.0 <= 0.0 || scale.1 <= 0.0 {
        return Vec::new();
    }
    let source_aperture = (
        ((screen.aperture[0] - sidebar_surface[0]) / scale.0).round() as i32,
        ((screen.aperture[1] - sidebar_surface[1]) / scale.1).round() as i32,
    );
    let aperture_offset = screen.surface.aperture_offset();
    let source_content = (
        source_aperture.0.wrapping_add(aperture_offset.0),
        source_aperture.1.wrapping_add(aperture_offset.1),
    );
    let clip_left = shader_pixel_round(sidebar_surface[0]);
    let clip_top = shader_pixel_round(sidebar_surface[1]);
    let clip_right = shader_pixel_round(sidebar_surface[0] + sidebar_surface[2]);
    let clip_bottom = shader_pixel_round(sidebar_surface[1] + sidebar_surface[3]);
    let mut instances = Vec::with_capacity(4);
    for (from, to) in edges {
        crate::render::surface_line::solid_line(from, to, |[left, top, width, height]| {
            let source_x = source_content.0.wrapping_add(left);
            let source_y = source_content.1.wrapping_add(top);
            let Some((x, w)) = nearest_scaled_interval(
                source_x,
                width,
                sidebar_surface[0],
                scale.0,
                clip_left,
                clip_right,
            ) else {
                return;
            };
            let Some((y, h)) = nearest_scaled_interval(
                source_y,
                height,
                sidebar_surface[1],
                scale.1,
                clip_top,
                clip_bottom,
            ) else {
                return;
            };
            instances.push(SpriteInstance {
                position: [camera.0 + x as f32, camera.1 + y as f32],
                size: [w as f32, h as f32],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: MINIMAP_DEPTH,
                tint,
                alpha: 1.0,
                ..Default::default()
            })
        });
    }
    instances
}

/// `batch_shader.wgsl` native-zoom vertex snap: `floor(value + 0.5)`.
fn shader_pixel_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

/// Integer destination span whose pixel centres nearest-sample one half-open
/// source interval. Scanning the already-clipped destination surface makes the
/// 0.5x parity phase explicit and prevents adjacent source pixels from both
/// claiming the same output pixel.
fn nearest_scaled_interval(
    source_start: i32,
    source_len: i32,
    destination_origin: f32,
    scale: f32,
    clip_start: i32,
    clip_end: i32,
) -> Option<(i32, i32)> {
    if source_len <= 0 || clip_start >= clip_end {
        return None;
    }
    let source_end = source_start.wrapping_add(source_len);
    let mut first = None;
    let mut end = clip_start;
    for destination in clip_start..clip_end {
        let source = (((destination as f32 + 0.5) - destination_origin) / scale).floor() as i32;
        if source >= source_start && source < source_end {
            first.get_or_insert(destination);
            end = destination.wrapping_add(1);
        }
    }
    first.map(|start| (start, end.wrapping_sub(start)))
}

fn load_i32(value: i32) -> X87Value {
    X87Chop53::load_i32(value)
}

fn load_constant(value: f32) -> X87Value {
    load_f32(value)
}

fn load_f32(value: f32) -> X87Value {
    X87Chop53::load_f32(NativeF32Bits::from_bits(value.to_bits())).expect("finite radar value")
}

fn store_f32(value: X87Value) -> f32 {
    f32::from_bits(
        X87Chop53::store_f32(value)
            .expect("native radar viewport value remains finite")
            .bits(),
    )
}

fn add(lhs: X87Value, rhs: X87Value) -> X87Value {
    X87Chop53::add(lhs, rhs)
}

fn mul(lhs: X87Value, rhs: X87Value) -> X87Value {
    X87Chop53::mul(lhs, rhs)
}

fn div(lhs: X87Value, rhs: X87Value) -> X87Value {
    X87Chop53::div(lhs, rhs).expect("positive radar divisor")
}

fn native_ftol(value: X87Value) -> i32 {
    X87Chop53::ftol_i64(value).expect("radar viewport value fits i32") as i32
}

#[cfg(test)]
#[path = "viewport_tests.rs"]
mod tests;
