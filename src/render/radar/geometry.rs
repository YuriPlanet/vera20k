//! Native generated primary-radar surface geometry.
//!
//! This is client presentation authority. It is rebuilt from the current
//! normalized playfield and never enters simulation snapshots or world hashes.

use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, X87Value,
};

pub(super) const RADAR_APERTURE_WIDTH: u32 = 140;
pub(super) const RADAR_APERTURE_HEIGHT: u32 = 108;
const MAX_RADAR_WIDTH: i32 = RADAR_APERTURE_WIDTH as i32;
const MAX_RADAR_HEIGHT: i32 = RADAR_APERTURE_HEIGHT as i32;
const MAX_RADAR_WIDTH_BITS: u32 = 0x430c_0000;
const MAX_RADAR_HEIGHT_BITS: u32 = 0x42d8_0000;
const ONE_OVER_CELL_BITS: u32 = 0x3b80_0000;

/// The raw isometric transform plus the generated primary-surface dimensions.
///
/// Active YR establishes these values through `ComputeRadarMapBounds @
/// 0x00654490`, `GenerateTerrainSurface @ 0x006547C0`, and
/// `RebuildRadarSurfaces @ 0x00654650`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::render) struct NativeRadarSurfaceGeometry {
    raw_offset_x: i32,
    raw_offset_y: i32,
    raw_width: i32,
    raw_height: i32,
    zoom: f32,
    generated_width: i32,
    generated_height: i32,
}

/// `InitRadarEvent @ 0x0065FC32..0x0065FC5D`: maximum signed distance from
/// the event center to an edge of the generated primary surface.
pub(super) fn native_event_initial_radius(
    pixel: (i32, i32),
    surface_size: (i32, i32),
) -> i32 {
    pixel
        .0
        .max(pixel.1)
        .max(surface_size.0.wrapping_sub(pixel.0))
        .max(surface_size.1.wrapping_sub(pixel.1))
}

impl NativeRadarSurfaceGeometry {
    /// Rebuild the generated surface from exact mode-one-valid cells.
    pub fn from_playfield(
        terrain: &ResolvedTerrainGrid,
        bounds: PlayfieldBounds,
    ) -> Option<Self> {
        // `0x006544D4..0x00654533` narrows the LocalSize inputs to signed
        // words before establishing RadarClass+0x1490. Preserve those wraps.
        let left = bounds.off_fc as i16;
        let top = bounds.off_100 as i16;
        let upper_left_sum = left.wrapping_add(1).wrapping_add(top);
        let lower_left_sum = (bounds.base as i16)
            .wrapping_sub(left)
            .wrapping_add(top);
        let raw_offset_x = i32::from(lower_left_sum).wrapping_sub(i32::from(upper_left_sum));

        // `ComputeRadarMapBounds @ 0x0065455B..0x00654572` passes literal
        // mode 1 to `MapClass::IsCellInPlayfield @ 0x00578460`. This is
        // deliberately distinct from the mode-zero terrain/overlay surface.
        let valid_cells: Vec<(i32, i32)> = terrain
            .iter()
            .filter(|cell| {
                bounds.contains_height_aware_packed(
                    i32::from(cell.rx),
                    i32::from(cell.ry),
                    cell.level as i8,
                    cell.slope_type,
                )
            })
            .map(|cell| (i32::from(cell.rx as i16), i32::from(cell.ry as i16)))
            .collect();
        let raw_offset_y = valid_cells
            .iter()
            .map(|&(x, y)| x.wrapping_add(y))
            .min()?;

        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        let right_edge = bounds.off_104.wrapping_mul(2).wrapping_sub(1);
        for (x, y) in valid_cells {
            let mut raw_x = raw_offset_x.wrapping_sub(y).wrapping_add(x);
            let mut cell_width = 2;
            if raw_x == -1 {
                raw_x = 0;
                cell_width = 1;
            } else if raw_x == right_edge {
                cell_width = 1;
            }
            let raw_y = y.wrapping_sub(raw_offset_y).wrapping_add(x);
            min_x = min_x.min(raw_x);
            min_y = min_y.min(raw_y);
            max_x = max_x.max(raw_x.wrapping_add(cell_width));
            max_y = max_y.max(raw_y.wrapping_add(1));
        }

        // MapClass's active normalized diamond makes both raw origins zero.
        // Keep this explicit so a sparse/non-authoritative grid cannot silently
        // shift the native CellToRadarPixel frame.
        if min_x != 0 || min_y != 0 {
            return None;
        }
        Self::from_raw_rect(
            raw_offset_x,
            raw_offset_y,
            max_x.wrapping_sub(min_x),
            max_y.wrapping_sub(min_y),
        )
    }

    pub(in crate::render) fn from_raw_rect(
        raw_offset_x: i32,
        raw_offset_y: i32,
        raw_width: i32,
        raw_height: i32,
    ) -> Option<Self> {
        if raw_width <= 0 || raw_height <= 0 {
            return None;
        }
        let (zoom, generated_width, generated_height) = native_aspect_fit(raw_width, raw_height);
        Some(Self {
            raw_offset_x,
            raw_offset_y,
            raw_width,
            raw_height,
            zoom,
            generated_width,
            generated_height,
        })
    }

    /// `CellToRadarPixel @ 0x006550C0`, relative to the primary surface.
    pub fn cell_to_surface_pixel(self, cell: (u16, u16)) -> (i32, i32) {
        let x = i32::from(cell.0 as i16);
        let y = i32::from(cell.1 as i16);
        let raw_x = self.raw_offset_x.wrapping_sub(y).wrapping_add(x);
        let raw_y = y.wrapping_sub(self.raw_offset_y).wrapping_add(x);
        let mut pixel_x = native_ftol(X87Chop53::mul(
            X87Chop53::load_i32(raw_x),
            load_f32(self.zoom),
        ));
        let pixel_y = native_ftol(X87Chop53::mul(
            X87Chop53::load_i32(raw_y),
            load_f32(self.zoom),
        ));
        // Native applies this after adding the integral surface origin. In the
        // relative frame the same correction is exactly `-1 -> 0`.
        if pixel_x == -1 {
            pixel_x = 0;
        }
        (pixel_x, pixel_y)
    }

    /// Raw `FillTerrainColors @ 0x00654EA0` footprint origin before the
    /// function clips its native two-by-one cell rectangle.
    pub const fn cell_to_raw_pixel(self, cell: (u16, u16)) -> (i32, i32) {
        let x = cell.0 as i16 as i32;
        let y = cell.1 as i16 as i32;
        (
            self.raw_offset_x.wrapping_sub(y).wrapping_add(x),
            y.wrapping_sub(self.raw_offset_y).wrapping_add(x),
        )
    }

    /// `RenderCellPixel @ 0x00655CB8..0x00655D03` inverse used for the
    /// shroud/fog query belonging to one generated-primary pixel.
    pub fn surface_pixel_to_visibility_cell(self, pixel: (i32, i32)) -> (u16, u16) {
        let raw_x = X87Chop53::sub(
            X87Chop53::div(X87Chop53::load_i32(pixel.0), load_f32(self.zoom))
                .expect("positive radar zoom"),
            X87Chop53::load_i32(self.raw_offset_x),
        );
        let raw_y = X87Chop53::add(
            X87Chop53::div(X87Chop53::load_i32(pixel.1), load_f32(self.zoom))
                .expect("positive radar zoom"),
            X87Chop53::load_i32(self.raw_offset_y),
        );
        let half = X87Chop53::load_f32(NativeF32Bits::from_bits(0x3f00_0000))
            .expect("0.5f is finite");
        let bias = X87Chop53::load_f64(NativeF64Bits::HALF).expect("0.5 is finite");
        let cell_x = native_ftol(X87Chop53::add(
            X87Chop53::mul(X87Chop53::add(raw_y, raw_x), half),
            bias,
        )) as i16;
        let cell_y = native_ftol(X87Chop53::add(
            X87Chop53::mul(X87Chop53::sub(raw_y, raw_x), half),
            bias,
        )) as i16;
        (cell_x as u16, cell_y as u16)
    }

    /// Signed-word result consumed by the radar input handler at
    /// `0x00653ABC..0x00653E66`. Do not reinterpret these words as unsigned
    /// before that handler applies its map/tactical-view clamp.
    pub fn surface_pixel_to_signed_cell(self, pixel: (i32, i32)) -> (i16, i16) {
        let packed = self.surface_pixel_to_visibility_cell(pixel);
        (packed.0 as i16, packed.1 as i16)
    }

    /// `FUN_006557F0 @ 0x006557F0`, relative to the generated primary
    /// surface. `TechnoClass+0x4A0 @ 0x0070D990` uses this exact current-world
    /// projection for its cached tracker coordinate.
    pub fn world_to_surface_pixel(
        self,
        world_x_leptons: i32,
        world_y_leptons: i32,
        clamp: bool,
    ) -> (i32, i32) {
        let raw_x = self
            .raw_offset_x
            .wrapping_mul(256)
            .wrapping_sub(world_y_leptons)
            .wrapping_add(world_x_leptons);
        let raw_y = world_y_leptons
            .wrapping_sub(self.raw_offset_y.wrapping_mul(256))
            .wrapping_add(world_x_leptons);
        let project = |raw| {
            native_ftol(X87Chop53::mul(
                X87Chop53::mul(X87Chop53::load_i32(raw), load_f32(self.zoom)),
                load_constant(ONE_OVER_CELL_BITS),
            ))
        };
        let mut pixel = (
            project(raw_x),
            project(raw_y),
        );
        if clamp {
            pixel.0 = pixel.0.clamp(0, self.generated_width.wrapping_sub(1));
            pixel.1 = pixel.1.clamp(0, self.generated_height.wrapping_sub(1));
        }
        pixel
    }

    /// `RebuildRadarSurfaces @ 0x00654650` centers the generated primary
    /// surface in the fixed 140x108 aperture using signed integer division.
    pub const fn aperture_offset(self) -> (i32, i32) {
        (
            (MAX_RADAR_WIDTH - self.generated_width) / 2,
            (MAX_RADAR_HEIGHT - self.generated_height) / 2,
        )
    }

    pub const fn surface_to_aperture_pixel(self, pixel: (i32, i32)) -> (i32, i32) {
        let offset = self.aperture_offset();
        (pixel.0 + offset.0, pixel.1 + offset.1)
    }

    pub const fn zoom(self) -> f32 {
        self.zoom
    }

    /// VERA's HD radar: the same raw frame generated at `scale` times the
    /// size, so HD pixel (x, y) is drawn inside native pixel
    /// (x / scale, y / scale). Its pixel and cell mappings work in HD pixels;
    /// its aperture placement is the native one times `scale`, never
    /// [`Self::aperture_offset`], which assumes the native size.
    pub fn subdivided(self, scale: u32) -> Self {
        let scale = scale.max(1) as i32;
        Self {
            zoom: self.zoom * scale as f32,
            generated_width: self.generated_width.wrapping_mul(scale),
            generated_height: self.generated_height.wrapping_mul(scale),
            ..self
        }
    }

    /// `RadarClass::GetObjectAtRadarPixel @ 0x00656750` fallback inverse.
    ///
    /// The binary uses the same x87 divide/half/+0.5/ftol sequence as
    /// `RenderCellPixel`; returning the packed words preserves native wrapping
    /// for an edge pixel instead of imposing a rectangular Rust admission.
    pub fn surface_pixel_to_cell(self, pixel: (i32, i32)) -> Option<(u16, u16)> {
        let signed = self.surface_pixel_to_signed_cell(pixel);
        Some((signed.0 as u16, signed.1 as u16))
    }

    pub const fn generated_size(self) -> (i32, i32) {
        (self.generated_width, self.generated_height)
    }

    pub const fn raw_size(self) -> (i32, i32) {
        (self.raw_width, self.raw_height)
    }
}

fn native_aspect_fit(raw_width: i32, raw_height: i32) -> (f32, i32, i32) {
    // `0x006548B3..0x00654939`: the candidate is stored to f32 before use.
    let width_zoom = store_f32(
        X87Chop53::div(load_constant(MAX_RADAR_WIDTH_BITS), X87Chop53::load_i32(raw_width))
            .expect("positive raw radar width"),
    );
    let scaled_height = X87Chop53::mul(X87Chop53::load_i32(raw_height), load_f32(width_zoom));
    if X87Chop53::compare(scaled_height, load_constant(MAX_RADAR_HEIGHT_BITS))
        == X87Ordering::Less
    {
        // FST stores the product to f32 before the width-branch ftol call.
        let stored_height = store_f32(scaled_height);
        (width_zoom, MAX_RADAR_WIDTH, native_ftol(load_f32(stored_height)))
    } else {
        let height_zoom = store_f32(
            X87Chop53::div(
                load_constant(MAX_RADAR_HEIGHT_BITS),
                X87Chop53::load_i32(raw_height),
            )
            .expect("positive raw radar height"),
        );
        let generated_width = native_ftol(X87Chop53::mul(
            X87Chop53::load_i32(raw_width),
            load_f32(height_zoom),
        ));
        (height_zoom, generated_width, MAX_RADAR_HEIGHT)
    }
}

fn load_constant(bits: u32) -> X87Value {
    X87Chop53::load_f32(NativeF32Bits::from_bits(bits)).expect("native radar constant is finite")
}

fn load_f32(value: f32) -> X87Value {
    X87Chop53::load_f32(NativeF32Bits::from_bits(value.to_bits()))
        .expect("native radar geometry value is finite")
}

fn store_f32(value: X87Value) -> f32 {
    f32::from_bits(
        X87Chop53::store_f32(value)
            .expect("native radar geometry remains finite")
            .bits(),
    )
}

fn native_ftol(value: X87Value) -> i32 {
    X87Chop53::ftol_i64(value).expect("native radar geometry fits i32") as i32
}

#[cfg(test)]
#[path = "geometry_tests.rs"]
mod tests;
