//! Shared tactical ABuffer — native per-pixel palette-row input.
//!
//! The original engine writes SHROUD.SHP values into a 16-bit circular buffer;
//! each tile/sprite blitter uses A to select a palette row. Our GPU source:
//!
//! 1. CPU-side: blit SHROUD.SHP raw brightness pixels into a screen-res R8 buffer
//! 2. Upload to an R8Unorm GPU texture
//! 3. Every world blitter reads this same texture before storing its source color
//!
//! Original493DF0/494B60/4990E0 select a palette row with A. Native surface
//! lines store their packed color directly, and shadow497390 halves the existing
//! destination without reading A. There is no later framebuffer curtain.
//!
//! The lattice is FLAT: the native per-cell blit
//! (`Tactical_layer_shroud_edges @ 0x006D3660`) zeroes the cell coord's Z
//! before projecting, so terrain height never shifts a shroud frame and the
//! diamonds tile the screen without gaps at height discontinuities.
//!
//! ## Dependency rules
//! - Part of render/ — reads FogState (no mutation), uses GpuContext.

use crate::map::terrain::{iso_to_screen, screen_to_iso};
use crate::render::gpu::GpuContext;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;
use wgpu::util::DeviceExt;

/// Tactical clear value written by6D3F9F. A selects a palette row; it is not
/// itself a universal RGB brightness multiplier.
const NEUTRAL: u8 = 0x7F;

/// SHP transparent pixel marker — skip (don't overwrite buffer).
const TRANSPARENT: u8 = 0xFE;
const SHROUD_CELL_PAD: i32 = 8;

/// What the shroud brightness buffer writes for one cell.
///
/// Split out of the rebuild loop so the decision can be tested without a GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellFill {
    /// Leave the neutral value — nothing to darken here.
    None,
    /// Blit this SHROUD.SHP frame.
    Frame(usize),
}

/// Decide what one cell contributes to the shroud brightness buffer.
///
/// Native6FB170 changes Cell+12C shroud knowledge only without sustained sight;
/// native578100 consumes pending departure conceal at the120-frame Logic boundary.
/// FogState publishes that selected-cell result here. This CPU decision comparison
/// does not certify the complete native edge cache or GPU pixel composition.
pub(crate) fn cell_fill(
    fog: &FogState,
    owner: InternedId,
    rx: u16,
    ry: u16,
    lut: &[u8; 256],
) -> CellFill {
    // Original6D8700 ->4801F0 ->47EFE0 selects the actual SHROUD frame15.
    // Stock frame15 stores A2, not zero: this also decides native rally pass
    // admission. The archive bytes belong to the existing frame-data owner.
    if !fog.is_cell_revealed(owner, rx, ry) || fog.is_cell_gap_covered(owner, rx, ry) {
        return CellFill::Frame(15);
    }
    let bitmask = fog.shroud_edge_mask_8bit(owner, rx, ry);
    if bitmask == 0 {
        return CellFill::None; // Fully revealed — already bright.
    }
    match lut[bitmask as usize] {
        0xFF => CellFill::None,
        0xFE => CellFill::Frame(15),
        idx => CellFill::Frame(idx as usize),
    }
}

pub use crate::sim::vision::SHROUD_EDGE_LUT;

/// Immutable decoded SHROUD bytes and their derived non-FE row spans. Both are
/// created together when a ShroudBuffer takes a source frame set; viewport,
/// camera and fog changes reuse them. Replacing the source creates a new set,
/// so no separately invalidated copy of the source bytes exists.
struct BrightnessFrame {
    pixels: Vec<u8>,
    spans: Vec<BrightnessSpan>,
}

/// Compact source coordinates and byte offsets, visited in source row order.
struct BrightnessSpan {
    row: u32,
    left: u32,
    source: std::ops::Range<u32>,
}

impl BrightnessFrame {
    fn new(pixels: Vec<u8>, canvas: [u32; 2]) -> Self {
        assert_eq!(
            pixels.len() as u64,
            u64::from(canvas[0]) * u64::from(canvas[1])
        );
        assert!(u32::try_from(pixels.len()).is_ok());
        let mut spans = Vec::new();
        if canvas[0] != 0 {
            for (row_index, row) in pixels.chunks_exact(canvas[0] as usize).enumerate() {
                let mut x = 0;
                while x < row.len() {
                    if row[x] == TRANSPARENT {
                        x += 1;
                        continue;
                    }
                    let left = x;
                    while x < row.len() && row[x] != TRANSPARENT {
                        x += 1;
                    }
                    let base = row_index * canvas[0] as usize;
                    spans.push(BrightnessSpan {
                        row: row_index as u32,
                        left: left as u32,
                        source: (base + left) as u32..(base + x) as u32,
                    });
                }
            }
        }
        Self { pixels, spans }
    }
}

/// Authoritative CPU A bytes and their single GPU texture. Derived bindings
/// cache the retained view identity; a resize replaces it and invalidates them.
pub struct ShroudBuffer {
    /// CPU-side brightness buffer (one byte per screen pixel).
    /// Native clear is 0x7F; stock unrevealed SHROUD stores 0x02. These are
    /// palette-row inputs, stored with padded GPU-upload row stride.
    pixels: Vec<u8>,
    /// Actual screen width.
    width: u32,
    /// Actual screen height.
    height: u32,
    /// Padded bytes-per-row (aligned to 256 for wgpu).
    row_stride: u32,
    /// GPU R8 texture.
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// World-pixel origin of the CPU/GPU A lattice, updated with the pixels.
    source_uniform: wgpu::Buffer,
    /// Original SHROUD bytes plus immutable derived visible row spans.
    /// Recreated only with the source frame set, not on viewport/fog changes.
    frames: Vec<BrightnessFrame>,
    /// SHP canvas width (typically 60).
    canvas_w: u32,
    /// SHP canvas height (typically 30).
    canvas_h: u32,
    /// Cached camera X for change detection.
    last_cam_x: f32,
    /// Cached camera Y for change detection.
    last_cam_y: f32,
    /// Cached fog generation for change detection.
    last_fog_gen: u64,
    /// The same fog generation can be sampled for a different local viewer.
    last_owner: Option<InternedId>,
    /// Cached screen width for resize detection.
    last_screen_w: u32,
    /// Cached screen height for resize detection.
    last_screen_h: u32,
    /// Cached zoom level for change detection.
    last_zoom: f32,
    /// Map dimensions in cells.
    map_width: u16,
    map_height: u16,
    /// 256-byte LUT mapping neighbor bitmask to SHROUD.SHP frame index.
    lut: [u8; 256],
}

/// Align `n` up to the next multiple of `align`.
fn align_up(n: u32, align: u32) -> u32 {
    n.div_ceil(align) * align
}

/// Same virtual-pixel coverage as the camera projection; no arbitrary4096
/// truncation. Device limits belong to the zoom/viewport admission owner.
fn virtual_dimensions(screen: [u32; 2], zoom: f32) -> [u32; 2] {
    assert!(zoom.is_finite() && zoom > 0.0);
    screen.map(|value| ((value as f32 / zoom).ceil() as u32).max(1))
}

fn clamp_cell_range(min_v: i32, max_v: i32, limit: u16) -> Option<(u16, u16)> {
    if limit == 0 {
        return None;
    }
    let upper = i32::from(limit) - 1;
    let start = min_v.clamp(0, upper);
    let end = max_v.clamp(0, upper);
    if start > end {
        return None;
    }
    Some((start as u16, end as u16))
}
impl ShroudBuffer {
    fn visible_cell_bounds(
        &self,
        cam_x: f32,
        cam_y: f32,
        vp_w: i32,
        vp_h: i32,
    ) -> Option<((u16, u16), (u16, u16))> {
        let pad_x = self.canvas_w as f32;
        let pad_y = self.canvas_h as f32;
        let left = cam_x - pad_x;
        let right = cam_x + vp_w as f32 + pad_x;
        let top = cam_y - pad_y;
        let bottom = cam_y + vp_h as f32 + pad_y;
        let corners = [
            screen_to_iso(left, top),
            screen_to_iso(right, top),
            screen_to_iso(left, bottom),
            screen_to_iso(right, bottom),
        ];

        let mut min_rx = f32::INFINITY;
        let mut max_rx = f32::NEG_INFINITY;
        let mut min_ry = f32::INFINITY;
        let mut max_ry = f32::NEG_INFINITY;
        for (rx, ry) in corners {
            min_rx = min_rx.min(rx);
            max_rx = max_rx.max(rx);
            min_ry = min_ry.min(ry);
            max_ry = max_ry.max(ry);
        }

        let rx_range = clamp_cell_range(
            min_rx.floor() as i32 - SHROUD_CELL_PAD,
            max_rx.ceil() as i32 + SHROUD_CELL_PAD,
            self.map_width,
        )?;
        let ry_range = clamp_cell_range(
            min_ry.floor() as i32 - SHROUD_CELL_PAD,
            max_ry.ceil() as i32 + SHROUD_CELL_PAD,
            self.map_height,
        )?;
        Some((rx_range, ry_range))
    }

    /// Create a new shroud buffer for the given screen and map dimensions.
    ///
    /// `frame_pixels` is the raw SHROUD.SHP brightness data per frame,
    /// extracted by the caller from the SHP file.
    pub fn new(
        gpu: &GpuContext,
        screen_w: u32,
        screen_h: u32,
        map_width: u16,
        map_height: u16,
        frame_pixels: Vec<Vec<u8>>,
        canvas_w: u32,
        canvas_h: u32,
        lut: [u8; 256],
    ) -> Self {
        Self::new_on_device(
            &gpu.device,
            &gpu.queue,
            [screen_w, screen_h],
            [map_width, map_height],
            frame_pixels,
            [canvas_w, canvas_h],
            lut,
        )
    }

    fn new_on_device(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_size: [u32; 2],
        map_size: [u16; 2],
        frame_pixels: Vec<Vec<u8>>,
        canvas_size: [u32; 2],
        lut: [u8; 256],
    ) -> Self {
        let [screen_w, screen_h] = screen_size;
        let [map_width, map_height] = map_size;
        let [canvas_w, canvas_h] = canvas_size;
        let row_stride = align_up(screen_w, 256);
        let pixels = vec![NEUTRAL; (row_stride * screen_h) as usize];
        let texture = create_r8_texture(device, screen_w, screen_h);
        let view = texture.create_view(&Default::default());
        let source_uniform = source_uniform(device, [0.0; 2], true);
        let result = Self {
            pixels,
            width: screen_w,
            height: screen_h,
            row_stride,
            texture,
            view,
            source_uniform,
            frames: frame_pixels
                .into_iter()
                .map(|pixels| BrightnessFrame::new(pixels, [canvas_w, canvas_h]))
                .collect(),
            canvas_w,
            canvas_h,
            last_cam_x: f32::NAN,
            last_cam_y: f32::NAN,
            last_fog_gen: u64::MAX,
            last_owner: None,
            last_screen_w: screen_w,
            last_screen_h: screen_h,
            last_zoom: 1.0,
            map_width,
            map_height,
            lut,
        };
        result.upload(queue);
        result
    }

    /// Invalidate the fog dirty-gate so the next frame rebuilds regardless of
    /// generation (F10): the view-cache generation restarts from zero after a
    /// load, so an equal counter value no longer proves an unchanged view.
    pub fn mark_stale(&mut self) {
        self.last_fog_gen = u64::MAX;
    }

    /// Rebuild the shroud buffer if camera moved, fog changed, or screen resized.
    ///
    /// Blits SHROUD.SHP bytes into the CPU buffer, then uploads it once.
    /// Disjoint stock frame masks establish equivalence to native traversal.
    pub fn rebuild_if_needed(
        &mut self,
        gpu: &GpuContext,
        fog: &FogState,
        owner: crate::sim::intern::InternedId,
        cam_x: f32,
        cam_y: f32,
        screen_w: u32,
        screen_h: u32,
        zoom: f32,
    ) {
        // Render shroud at virtual resolution (screen / zoom) so diamond blits
        // stay at fixed world-pixel sizes. Blitters address it from the uploaded
        // camera/zoom and this source's origin, without normalized UV stretching.
        // The camera owner bounds zoom against the requested device limit.
        // Truncating this lattice would silently expose unsampled shroud as
        // neutral A; keep the full virtual extent and reject that broken input.
        let [virt_w, virt_h] = virtual_dimensions([screen_w, screen_h], zoom);
        let limit = gpu.device.limits().max_texture_dimension_2d;
        assert!(
            virt_w <= limit && virt_h <= limit,
            "tactical A extent exceeds requested GPU limit"
        );

        // Resize GPU texture if virtual dimensions changed.
        if virt_w != self.width
            || virt_h != self.height
            || screen_w != self.last_screen_w
            || screen_h != self.last_screen_h
        {
            self.width = virt_w;
            self.height = virt_h;
            self.row_stride = align_up(virt_w, 256);
            self.pixels
                .resize((self.row_stride * virt_h) as usize, NEUTRAL);
            self.texture = create_r8_texture(&gpu.device, virt_w, virt_h);
            self.view = self.texture.create_view(&Default::default());
            self.last_screen_w = screen_w;
            self.last_screen_h = screen_h;
            // Force rebuild after resize.
            self.last_fog_gen = u64::MAX;
        }

        // Skip if nothing changed (camera rounded to pixel + fog generation + zoom).
        let cam_x_r = cam_x.floor();
        let cam_y_r = cam_y.floor();
        if fog.view_generation() == self.last_fog_gen
            && self.last_owner == Some(owner)
            && cam_x_r == self.last_cam_x
            && cam_y_r == self.last_cam_y
            && (zoom - self.last_zoom).abs() < 1e-6
        {
            return;
        }
        self.last_fog_gen = fog.view_generation();
        self.last_owner = Some(owner);
        self.last_cam_x = cam_x_r;
        self.last_cam_y = cam_y_r;
        self.last_zoom = zoom;

        self.rasterize(fog, owner);

        // Debug: dump the CPU brightness buffer as a grayscale PNG when
        // RA2_DUMP_SHROUD names a path. Diagnostic only — no gamemd counterpart.
        if let Ok(path) = std::env::var("RA2_DUMP_SHROUD") {
            let w = self.width as usize;
            let mut img = vec![0u8; w * self.height as usize];
            for y in 0..self.height as usize {
                let src = y * self.row_stride as usize;
                img[y * w..(y + 1) * w].copy_from_slice(&self.pixels[src..src + w]);
            }
            if let Err(e) =
                image::save_buffer(&path, &img, self.width, self.height, image::ColorType::L8)
            {
                log::warn!("shroud dump failed: {e}");
            }
        }

        self.upload(&gpu.queue);
    }

    fn upload(&self, queue: &wgpu::Queue) {
        let origin =
            [self.last_cam_x, self.last_cam_y].map(|v| if v.is_finite() { v } else { 0.0 });
        queue.write_buffer(
            &self.source_uniform,
            0,
            bytemuck::bytes_of(&SourceUniform::new(origin, true)),
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.row_stride),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Shared full-rebuild CPU owner, separated from upload for direct native
    /// scene comparisons. Empty bounds still retain the authoritative reset.
    fn rasterize(&mut self, fog: &FogState, owner: InternedId) {
        // Fill bright, then darken unrevealed cells and blit edge transitions.
        // Blitting in world-pixel coordinates at virtual resolution
        // means diamond tiles match the world grid exactly at every zoom.
        self.pixels.fill(NEUTRAL);

        let vp_w = self.width as i32;
        let vp_h = self.height as i32;
        let cam_x_r = self.last_cam_x;
        let cam_y_r = self.last_cam_y;
        let cam_xi = cam_x_r as i32;
        let cam_yi = cam_y_r as i32;

        let Some(((rx_start, rx_end), (ry_start, ry_end))) =
            self.visible_cell_bounds(cam_x_r, cam_y_r, vp_w, vp_h)
        else {
            return;
        };

        // Native6D71E0 visits these cells on screen diagonals. All47 stock
        // LUT-selected SHROUD frames share a disjoint 900-pixel lattice mask;
        // therefore this bounded row-major traversal produces identical stores.
        // The executable/retail proof is tools/procedural_drawing_oracle/shroud.
        // Alpha/fog overlays and nonstock overlapping masks are outside it.
        for ry in ry_start..=ry_end {
            for rx in rx_start..=rx_end {
                // The shroud lattice is FLAT: gamemd's per-cell edge blit
                // (`Tactical_layer_shroud_edges @ 0x006D3660`) takes
                // `CellClass::Get_Center_Coords`, forces the coord's Z to 0,
                // projects, then centers the 60x30 frame there. Terrain height
                // never shifts a shroud blit, so the diamonds tile the screen
                // with no gaps at cliffs, ramps, or bridge ends. Blitting at
                // cell height instead left an undarkened tear at every height
                // discontinuity.
                let (sx, sy) = iso_to_screen(rx, ry, 0);
                let vx = sx as i32 - cam_xi;
                let vy = sy as i32 - cam_yi;

                if vx + self.canvas_w as i32 <= 0
                    || vx >= vp_w
                    || vy + self.canvas_h as i32 <= 0
                    || vy >= vp_h
                {
                    continue;
                }

                let fill = cell_fill(fog, owner, rx, ry, &self.lut);
                match fill {
                    CellFill::None => continue,
                    CellFill::Frame(idx) => self.blit_frame(idx, vx, vy, vp_w, vp_h),
                }
            }
        }
    }

    /// Blit one SHROUD.SHP frame's raw brightness pixels into the CPU buffer.
    ///
    /// Coordinates are in viewport space (0,0 = top-left of screen).
    /// Handles clipping and transparent pixel skip (0xFE).
    fn blit_frame(&mut self, frame_idx: usize, vx: i32, vy: i32, vp_w: i32, vp_h: i32) {
        let Some(frame) = self.frames.get(frame_idx) else {
            return;
        };
        blit_frame_pixels(
            &mut self.pixels,
            self.row_stride as usize,
            [vp_w as u32, vp_h as u32],
            frame,
            [vx, vy],
        );
    }

    /// Read-only GPU source. The view stays stable until a resize; the uniform
    /// and CPU samples share the floored world origin from the last rebuild.
    pub(crate) fn gpu_source(&self) -> (&wgpu::TextureView, &wgpu::Buffer) {
        (&self.view, &self.source_uniform)
    }

    /// Synthetic native A input for GPU leaf tests, through the production
    /// source allocation/upload owner. Pixels are row-major, without padding.
    #[cfg(test)]
    pub(crate) fn fixture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        origin: [f32; 2],
        pixels: &[u8],
    ) -> Self {
        assert_eq!(pixels.len(), (size[0] * size[1]) as usize);
        let mut result = Self::new_on_device(
            device,
            queue,
            size,
            [0, 0],
            Vec::new(),
            [60, 30],
            SHROUD_EDGE_LUT,
        );
        result.last_cam_x = origin[0].floor();
        result.last_cam_y = origin[1].floor();
        for (row, source) in pixels.chunks_exact(size[0] as usize).enumerate() {
            let start = row * result.row_stride as usize;
            result.pixels[start..start + source.len()].copy_from_slice(source);
        }
        result.upload(queue);
        result
    }

    /// Sample the CPU-side ABuffer at a world-space screen pixel.
    ///
    /// The shroud buffer is rebuilt in virtual world pixels (`screen / zoom`),
    /// with camera scroll subtracted before blitting. Callers that build
    /// world-space surface operations use this same source as the GPU blitters.
    pub fn sample_world(&self, world_x: f32, world_y: f32, cam_x: f32, cam_y: f32) -> Option<u8> {
        let vx = (world_x - cam_x.floor()).floor() as i32;
        let vy = (world_y - cam_y.floor()).floor() as i32;
        if vx < 0 || vy < 0 || vx >= self.width as i32 || vy >= self.height as i32 {
            return None;
        }
        let idx = vy as usize * self.row_stride as usize + vx as usize;
        self.pixels.get(idx).copied()
    }
}

/// Original47EFE0 copies raw A bytes and skips FE; palette or RGB conversion
/// does not participate. Native frame headers are expanded by the SHP owner.
fn blit_frame_pixels(
    pixels: &mut [u8],
    stride: usize,
    size: [u32; 2],
    frame: &BrightnessFrame,
    origin: [i32; 2],
) {
    let [vx, vy] = origin;
    let [vp_w, vp_h] = size.map(|v| v as i32);
    // The spans retain every native store (including 00 and FF), omitting
    // only FE. Copying each clipped span preserves source order without a
    // transparency branch for every pixel on every rebuild.
    for span in &frame.spans {
        let y = vy + span.row as i32;
        if y < 0 {
            continue;
        }
        if y >= vp_h {
            break;
        }
        let left = vx + span.left as i32;
        let right = (left + (span.source.end - span.source.start) as i32).min(vp_w);
        let x = left.max(0);
        if x >= right {
            continue;
        }
        let count = (right - x) as usize;
        let source = span.source.start as usize + (x - left) as usize;
        let destination = y as usize * stride + x as usize;
        pixels[destination..destination + count]
            .copy_from_slice(&frame.pixels[source..source + count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::intern;
    use crate::sim::vision::apply_gap_generators;

    #[test]
    fn virtual_source_covers_the_supported_zoomed_out_view() {
        assert_eq!(virtual_dimensions([1280, 720], 0.25), [5120, 2880]);
        assert_eq!(virtual_dimensions([3840, 2160], 0.25), [15360, 8640]);
        assert_eq!(virtual_dimensions([19, 11], 0.75), [26, 15]);
    }

    #[test]
    fn gap_operational_native_order_reaches_shroud_fill_after_restore() {
        for (fog, viewer, shrouded) in
            crate::sim::world::gap_generator_tests::gap_operational_power_loss_views()
        {
            assert_eq!(
                matches!(
                    cell_fill(&fog, viewer, 12, 12, &SHROUD_EDGE_LUT),
                    CellFill::Frame(15)
                ),
                shrouded,
                "native gap/source update order must reach the visible shroud consumer"
            );
        }
    }

    /// A 5x5 explored block so the centre cell has all eight neighbours
    /// explored and therefore needs no edge frame of its own.
    fn explored_block(fog: &mut FogState, owner: InternedId) {
        for ry in 4..=8u16 {
            for rx in 4..=8u16 {
                fog.mark_visible_for_owner(owner, rx, ry);
            }
        }
    }

    fn fog_16() -> FogState {
        FogState::new(16, 16)
    }

    /// The generator's own ground stays at full brightness.
    ///
    /// VERA painted a flat half-bright field over it — a constant with no
    /// counterpart anywhere in the retail shroud path, which dimmed the
    /// owner's own base by roughly half over a radius-11 circle for the rest
    /// of the match.
    #[test]
    fn a_friendly_gap_generator_does_not_darken_its_owners_ground() {
        let owner = intern::test_intern("Americans");
        let interner = intern::test_interner();
        let mut fog = fog_16();
        explored_block(&mut fog, owner);
        apply_gap_generators(&mut fog, &[(owner, 6, 6, 2)], &interner);

        assert!(
            fog.is_cell_gap_fog(owner, 6, 6),
            "the sim still records friendly gap coverage for other consumers"
        );
        assert_eq!(
            cell_fill(&fog, owner, 6, 6, &SHROUD_EDGE_LUT),
            CellFill::None,
            "the shroud buffer must leave the generator's own ground alone"
        );
    }

    /// Past mapping without a sustained sight contribution is concealed.
    #[test]
    fn a_hostile_gap_generator_still_blacks_the_cell_out() {
        let victim = intern::test_intern("Soviet");
        let gapper = intern::test_intern("Americans");
        let interner = intern::test_interner();
        let mut fog = fog_16();
        explored_block(&mut fog, victim);
        apply_gap_generators(&mut fog, &[(gapper, 6, 6, 2)], &interner);

        assert_eq!(
            cell_fill(&fog, victim, 6, 6, &SHROUD_EDGE_LUT),
            CellFill::Frame(15)
        );
    }

    /// RevealEntireMap (a local defeat or Shroud=no) lifts the full shroud from
    /// unseen ground for the current house alone.
    #[test]
    fn a_whole_map_reveal_lifts_the_full_shroud_for_the_current_house() {
        let mut sim = crate::sim::world::Simulation::with_seed(0x577F30);
        sim.fog.width = 8;
        sim.fog.height = 6;
        let local = sim.interner.intern("Player");
        let other = sim.interner.intern("Computer");
        sim.session.current_house = Some(local);
        assert_eq!(
            cell_fill(&sim.fog, local, 3, 2, &SHROUD_EDGE_LUT),
            CellFill::Frame(15)
        );

        sim.reveal_entire_map(
            Some(local),
            crate::sim::world::UninitContext::new(None, None),
        );

        assert_eq!(
            cell_fill(&sim.fog, local, 3, 2, &SHROUD_EDGE_LUT),
            CellFill::None
        );
        assert_eq!(
            cell_fill(&sim.fog, other, 3, 2, &SHROUD_EDGE_LUT),
            CellFill::Frame(15)
        );
    }

    #[test]
    fn shroud_current_sight_keeps_tactical_cell_bright_until_pending_boundary() {
        use crate::map::entities::EntityCategory;
        use crate::sim::{components::Health, game_entity::GameEntity, vision};
        let owner = intern::test_intern("Americans");
        let gapper = intern::test_intern("Soviet");
        let interner = intern::test_interner();
        let mut fog = fog_16();
        let entity = GameEntity::new_at_frame_zero_for_test(
            1,
            6,
            6,
            0,
            0,
            owner,
            Health { current: 100 },
            intern::test_intern("E1"),
            EntityCategory::Infantry,
            0,
            3,
            false,
        );
        vision::reveal_entity_vision(
            &mut fog,
            &entity,
            &vision::VisionConfig::default(),
            None,
            None,
            false,
            &interner,
        );
        apply_gap_generators(&mut fog, &[(gapper, 6, 6, 2)], &interner);
        assert_eq!(
            cell_fill(&fog, owner, 6, 6, &SHROUD_EDGE_LUT),
            CellFill::None
        );
        vision::recompute_owner_visibility_in_place(
            &mut fog,
            &Default::default(),
            None,
            &Default::default(),
            &vision::VisionConfig::default(),
            None,
            None,
            &interner,
            None,
        );
        apply_gap_generators(&mut fog, &[(gapper, 6, 6, 2)], &interner);
        fog.flush_pending_gap_conceal(119);
        assert_eq!(
            cell_fill(&fog, owner, 6, 6, &SHROUD_EDGE_LUT),
            CellFill::None
        );
        fog.flush_pending_gap_conceal(120);
        assert_eq!(
            cell_fill(&fog, owner, 6, 6, &SHROUD_EDGE_LUT),
            CellFill::Frame(15)
        );
    }

    #[test]
    fn shroud_current_sight_nontransitive_allied_view_stays_dark() {
        use crate::map::entities::EntityCategory;
        use crate::sim::{components::Health, game_entity::GameEntity, vision};
        let a = intern::test_intern("Americans");
        let b = intern::test_intern("Alliance");
        let c = intern::test_intern("Soviet");
        let gapper = intern::test_intern("Neutral");
        let mut fog = fog_16();
        for (left, right) in [
            ("AMERICANS", "ALLIANCE"),
            ("ALLIANCE", "AMERICANS"),
            ("ALLIANCE", "SOVIET"),
            ("SOVIET", "ALLIANCE"),
        ] {
            fog.alliances
                .entry(left.into())
                .or_default()
                .insert(right.into());
        }
        for owner in [b, c] {
            fog.mark_visible_for_owner(owner, 0, 0);
        }
        let entity = GameEntity::new_at_frame_zero_for_test(
            1,
            6,
            6,
            0,
            0,
            a,
            Health { current: 100 },
            intern::test_intern("E1"),
            EntityCategory::Infantry,
            0,
            3,
            false,
        );
        let interner = intern::test_interner();
        vision::reveal_entity_vision(
            &mut fog,
            &entity,
            &vision::VisionConfig::default(),
            None,
            None,
            false,
            &interner,
        );
        for _ in 0..3 {
            apply_gap_generators(&mut fog, &[(gapper, 6, 6, 2)], &interner);
            assert_eq!(cell_fill(&fog, b, 6, 6, &SHROUD_EDGE_LUT), CellFill::None);
            assert_eq!(
                cell_fill(&fog, c, 6, 6, &SHROUD_EDGE_LUT),
                CellFill::Frame(15)
            );
            assert!(!fog.is_cell_visible(c, 6, 6));
        }
        apply_gap_generators(&mut fog, &[], &interner);
        assert_eq!(
            cell_fill(&fog, c, 6, 6, &SHROUD_EDGE_LUT),
            CellFill::Frame(15)
        );
    }

    #[test]
    fn unexplored_ground_is_dark_and_a_shroud_boundary_picks_a_frame() {
        let owner = intern::test_intern("Americans");
        let mut fog = fog_16();
        explored_block(&mut fog, owner);

        assert_eq!(
            cell_fill(&fog, owner, 12, 12, &SHROUD_EDGE_LUT),
            CellFill::Frame(15),
            "never-explored ground is fully shrouded"
        );
        // A corner of the explored block borders shroud on five sides, so it
        // takes an edge frame rather than the full diamond.
        match cell_fill(&fog, owner, 4, 4, &SHROUD_EDGE_LUT) {
            CellFill::Frame(idx) => assert!(idx < 48, "frame {idx} is outside SHROUD.SHP"),
            other => panic!("a shroud boundary cell must take a frame, got {other:?}"),
        }
    }
}

/// Extract raw brightness pixel data from a loaded SHROUD.SHP file.
///
/// Returns `(frame_pixels, canvas_w, canvas_h)` where each entry in
/// `frame_pixels` is a `canvas_w × canvas_h` buffer of raw SHP pixel values.
pub fn extract_shp_brightness(shp: &crate::assets::shp_file::ShpFile) -> (Vec<Vec<u8>>, u32, u32) {
    let canvas_w = shp.width as u32;
    let canvas_h = shp.height as u32;
    let canvas_size = (canvas_w * canvas_h) as usize;
    let mut all_frames: Vec<Vec<u8>> = Vec::with_capacity(shp.frames.len());

    for frame in &shp.frames {
        // Start with TRANSPARENT so pixels outside the actual SHP subframe
        // are skipped by blit_frame(). Only real SHP pixels affect the buffer.
        let mut buf = vec![TRANSPARENT; canvas_size];
        let fw = frame.frame_width as u32;
        let fh = frame.frame_height as u32;
        let fx = frame.frame_x as u32;
        let fy = frame.frame_y as u32;

        for row in 0..fh {
            for col in 0..fw {
                let src = (row * fw + col) as usize;
                let pixel = frame.pixels[src];
                let dx = fx + col;
                let dy = fy + row;
                if dx < canvas_w && dy < canvas_h {
                    buf[(dy * canvas_w + dx) as usize] = pixel;
                }
            }
        }
        all_frames.push(buf);
    }

    (all_frames, canvas_w, canvas_h)
}

// ---------------------------------------------------------------------------
// GPU resource helpers
// ---------------------------------------------------------------------------

fn create_r8_texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Shroud ABuffer Texture"),
        size: wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SourceUniform {
    origin: [f32; 2],
    enabled: u32,
    _pad: u32,
}
impl SourceUniform {
    fn new(origin: [f32; 2], enabled: bool) -> Self {
        Self {
            origin,
            enabled: u32::from(enabled),
            _pad: 0,
        }
    }
}
fn source_uniform(device: &wgpu::Device, origin: [f32; 2], enabled: bool) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Tactical A source origin"),
        contents: bytemuck::bytes_of(&SourceUniform::new(origin, enabled)),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

/// A disabled source for operation classes that do not read native A (surface
/// stores and screen UI), and for an absent/sandbox world. This constant is not
/// a second live A plane. Keeping it with the source owner also shares its ABI.
pub(crate) fn neutral_gpu_source(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Buffer) {
    let texture = create_r8_texture(device, 1, 1);
    crate::render::atlas_growth::write_texels(queue, &texture, [0, 0], [1, 1], 1, &[NEUTRAL]);
    (
        texture.create_view(&Default::default()),
        source_uniform(device, [0.0; 2], false),
    )
}

#[cfg(test)]
#[path = "shroud_native_tests.rs"]
mod native_tests;
