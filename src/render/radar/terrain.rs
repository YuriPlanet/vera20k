//! Active-YR raw RGB and generated packed radar terrain surfaces.
//!
//! This presentation-local cache mirrors `FillTerrainColors @ 0x00654EA0`
//! and the full-surface path through `GenerateTerrainSurface @ 0x006547C0`.
//! VERA's optional HD radar keeps a larger copy beside it ([`super::hd`]).

use std::collections::BTreeMap;
use std::ops::Range;

use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53, X87Value};

use super::geometry::NativeRadarSurfaceGeometry;
use super::helpers::RadarCellIndex;
use crate::render::native_surface_format::RGB565;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NativeRadarCellColors {
    pub cell: (u16, u16),
    pub left: [u8; 3],
    pub right: [u8; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NativeRadarCellUpdate {
    pub base: NativeRadarCellColors,
    pub override_color: Option<[u8; 3]>,
}

#[derive(Debug, Clone)]
pub(super) struct NativeRadarTerrainSurface {
    geometry: NativeRadarSurfaceGeometry,
    terrain_brightness: f32,
    base_cells: Vec<NativeRadarCellColors>,
    /// Built once: dirty updates rewrite colors but never cell identities.
    base_index: RadarCellIndex,
    overrides: BTreeMap<(u16, u16), [u8; 3]>,
    raw_rgb: Vec<[u8; 3]>,
    generated_rgb565: Vec<u16>,
    /// The generated surface's sampler, built once with the geometry.
    sampler: RadarSampler,
    /// The cell each generated pixel shows the shroud and fog of
    /// ([`NativeRadarSurfaceGeometry::visibility_cells`]), built once: the
    /// geometry never changes under a surface.
    pixel_cells: Vec<u32>,
    /// Bumped whenever `generated_rgb565` is regenerated.
    revision: u64,
    /// VERA's optional HD terrain, bumping `hd_revision` whenever it changes.
    hd: Option<HdTerrain>,
    hd_revision: u64,
}

impl NativeRadarTerrainSurface {
    pub fn new(
        geometry: NativeRadarSurfaceGeometry,
        base_cells: Vec<NativeRadarCellColors>,
        overrides: BTreeMap<(u16, u16), [u8; 3]>,
        terrain_brightness: f32,
    ) -> Self {
        let raw_size = geometry.raw_size();
        let generated_size = geometry.generated_size();
        let base_index = RadarCellIndex::new(base_cells.iter().map(|base| base.cell));
        let mut surface = Self {
            geometry,
            terrain_brightness,
            base_cells,
            base_index,
            overrides,
            raw_rgb: vec![[0; 3]; area(raw_size)],
            generated_rgb565: vec![0; area(generated_size)],
            sampler: RadarSampler::new(raw_size, generated_size),
            pixel_cells: geometry.visibility_cells(),
            revision: 0,
            hd: None,
            hd_revision: 0,
        };
        surface.rebuild();
        surface
    }

    pub const fn geometry(&self) -> NativeRadarSurfaceGeometry {
        self.geometry
    }

    pub const fn terrain_brightness(&self) -> f32 {
        self.terrain_brightness
    }

    pub fn generated_rgb565(&self) -> &[u16] {
        &self.generated_rgb565
    }

    /// Changes whenever [`Self::generated_rgb565`] does.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Each generated pixel's packed cell, row-major.
    pub fn pixel_cells(&self) -> &[u32] {
        &self.pixel_cells
    }

    /// [`NativeRadarSurfaceGeometry::surface_pixel_to_visibility_cell`],
    /// from the table on the surface; a pixel off it, such as an edge
    /// building's brush, is inverted directly.
    pub fn visibility_cell(&self, (x, y): (i32, i32)) -> (u16, u16) {
        let (width, height) = self.geometry.generated_size();
        if (0..width).contains(&x) && (0..height).contains(&y) {
            let packed = self.pixel_cells[(y * width + x) as usize];
            (packed as u16, (packed >> 16) as u16)
        } else {
            self.geometry.surface_pixel_to_visibility_cell((x, y))
        }
    }

    #[cfg(test)]
    pub(super) fn raw_rgb(&self) -> &[[u8; 3]] {
        &self.raw_rgb
    }

    /// Keep HD terrain at `scale` times the generated size, or drop it.
    pub fn set_hd_scale(&mut self, scale: Option<u32>) {
        if self.hd.as_ref().map(|hd| hd.scale) == scale {
            return;
        }
        self.hd = scale.map(|scale| HdTerrain::new(&self.raw_rgb, self.geometry, scale));
        self.hd_revision = self.hd_revision.wrapping_add(1);
    }

    /// The HD terrain, row-major, and a revision that changes with its pixels.
    pub fn hd_rgb565(&self) -> Option<(&[u16], u64)> {
        self.hd
            .as_ref()
            .map(|hd| (hd.rgb565.as_slice(), self.hd_revision))
    }

    // Original655250 regenerates the dirty union rectangle through6547C0.
    // This cache keeps full generation's output instead: the consumed
    // ordinary-ore control matches original full generation, and native's
    // rectangle walk differs at five pixels in one3x2 patch/fringe row
    // (ore_render.json radar). A generated pixel depends only on its own
    // column and row spans, so rewriting the changed cells' raw pixels and
    // resampling the pixels whose spans reach them gives exactly what full
    // generation would, without redrawing every cell on each update.
    pub fn set_cell_states(
        &mut self,
        changes: impl IntoIterator<Item = NativeRadarCellUpdate>,
    ) -> bool {
        let mut changed_cells = Vec::new();
        for update in changes {
            let (rx, ry) = update.base.cell;
            let mut changed = false;
            if let Some(slot) = self.base_index.slot(rx, ry) {
                let base = &mut self.base_cells[slot];
                changed |= *base != update.base;
                *base = update.base;
            }
            match update.override_color {
                Some(color) => {
                    changed |= self.overrides.insert(update.base.cell, color) != Some(color);
                }
                None => {
                    changed |= self.overrides.remove(&update.base.cell).is_some();
                }
            }
            if changed {
                changed_cells.push(update.base.cell);
            }
        }
        if changed_cells.is_empty() {
            return false;
        }
        // A cell writes only its own two raw pixels (`fill_raw_cell`), and no
        // two cells share one: cells in a raw row differ by two in x.
        let raw_size = self.geometry.raw_size();
        for &cell in &changed_cells {
            if let Some(slot) = self.base_index.slot(cell.0, cell.1) {
                let base = self.base_cells[slot];
                let (left, right) = self
                    .overrides
                    .get(&cell)
                    .map_or((base.left, base.right), |&color| (color, color));
                fill_raw_cell(
                    &mut self.raw_rgb,
                    raw_size,
                    self.geometry.cell_to_raw_pixel(cell),
                    left,
                    right,
                );
            }
        }
        let width = raw_size.0;
        for &cell in &changed_cells {
            let (x, y) = self.geometry.cell_to_raw_pixel(cell);
            let columns = x.max(0)..x.saturating_add(2).min(width);
            let rows = y..y.saturating_add(1);
            self.sampler.resample(
                &self.raw_rgb,
                &mut self.generated_rgb565,
                columns.clone(),
                rows.clone(),
            );
            if let Some(hd) = &mut self.hd {
                hd.sampler
                    .resample(&self.raw_rgb, &mut hd.rgb565, columns, rows);
            }
        }
        self.revision = self.revision.wrapping_add(1);
        if self.hd.is_some() {
            self.hd_revision = self.hd_revision.wrapping_add(1);
        }
        true
    }

    fn rebuild(&mut self) {
        self.raw_rgb.fill([0; 3]);
        let raw_size = self.geometry.raw_size();
        for cell in &self.base_cells {
            let (left, right) = self
                .overrides
                .get(&cell.cell)
                .map_or((cell.left, cell.right), |&color| (color, color));
            fill_raw_cell(
                &mut self.raw_rgb,
                raw_size,
                self.geometry.cell_to_raw_pixel(cell.cell),
                left,
                right,
            );
        }
        self.generated_rgb565 = self.sampler.generate(&self.raw_rgb);
        self.revision = self.revision.wrapping_add(1);
    }
}

fn area(size: (i32, i32)) -> usize {
    usize::try_from(size.0)
        .expect("positive radar width")
        .saturating_mul(usize::try_from(size.1).expect("positive radar height"))
}

/// `FillTerrainColors @ 0x00654F9E..0x00655023`: the clipped left edge
/// retains only the right half, the clipped right edge only the left half.
fn fill_raw_cell(
    raw: &mut [[u8; 3]],
    size: (i32, i32),
    origin: (i32, i32),
    left: [u8; 3],
    right: [u8; 3],
) {
    let (width, height) = size;
    if origin.1 < 0 || origin.1 >= height {
        return;
    }
    let row = origin.1 as usize * width as usize;
    if origin.0 == -1 {
        raw[row] = right;
    } else if origin.0 == width.wrapping_sub(1) {
        raw[row + origin.0 as usize] = left;
    } else if origin.0 >= 0 && origin.0.wrapping_add(1) < width {
        raw[row + origin.0 as usize] = left;
        raw[row + origin.0 as usize + 1] = right;
    }
}

/// The weighted-area sampler of `GenerateTerrainSurface @ 0x006547C0`, its raw
/// spans laid out once per column and row. Every f32 store below maps to the
/// explicit `FST[P] float` sites at `0x006549D3..0x006549EC` and the advancing
/// edge stores inside `0x00654AF5..0x00654DD7`. Native walks the same column
/// chain on every row, so a pixel depends only on its own column and row; the
/// HD radar resamples single pixels with it.
#[derive(Debug, Clone)]
struct RadarSampler {
    raw_width: i32,
    x_step: f32,
    y_step: f32,
    normalization: f32,
    columns: Vec<SampleSpan>,
    rows: Vec<SampleSpan>,
}

/// The raw pixels one generated column or row covers.
#[derive(Debug, Clone, Copy)]
struct SampleSpan {
    start: f32,
    /// What the last raw pixel's weight subtracts from: a column reloads its
    /// stored f32 end, a row keeps the unstored x87 sum.
    end: X87Value,
    first: i32,
    last: i32,
}

impl SampleSpan {
    fn weight(self, source: i32, step: f32) -> X87Value {
        if self.last.wrapping_sub(self.first) <= 1 {
            load_f32(step)
        } else if source == self.first {
            X87Chop53::sub(
                X87Chop53::load_i32(source.wrapping_add(1)),
                load_f32(self.start),
            )
        } else if source == self.last.wrapping_sub(1) {
            X87Chop53::sub(self.end, X87Chop53::load_i32(source))
        } else {
            load_f32(1.0)
        }
    }
}

impl RadarSampler {
    fn new(raw_size: (i32, i32), generated_size: (i32, i32)) -> Self {
        let (raw_width, raw_height) = raw_size;
        let (generated_width, generated_height) = generated_size;
        let x_step = store_f32(
            X87Chop53::div(
                X87Chop53::load_i32(raw_width),
                X87Chop53::load_i32(generated_width),
            )
            .expect("positive generated radar width"),
        );
        let y_step_extended = X87Chop53::div(
            X87Chop53::load_i32(raw_height),
            X87Chop53::load_i32(generated_height),
        )
        .expect("positive generated radar height");
        let y_step = store_f32(y_step_extended);
        let normalization = store_f32(
            X87Chop53::div(
                load_f64(NativeF64Bits::ONE),
                // `0x006549D7..0x006549EC`: FST retains the extended y quotient,
                // which is multiplied by the stored f32 x step before FDIVR.
                X87Chop53::mul(y_step_extended, load_f32(x_step)),
            )
            .expect("positive native radar sample area"),
        );

        let mut columns = Vec::with_capacity(usize::try_from(generated_width).unwrap_or(0));
        let mut x_start = 0.0f32;
        for _ in 0..generated_width {
            let x_end_extended = X87Chop53::add(load_f32(x_start), load_f32(x_step));
            let x_end = store_f32(x_end_extended);
            columns.push(SampleSpan {
                start: x_start,
                end: load_f32(x_end),
                first: native_ftol(load_f32(x_start)).min(raw_width),
                last: native_ftol(x_end_extended).wrapping_add(1).min(raw_width),
            });
            x_start = x_end;
        }
        let mut rows = Vec::with_capacity(usize::try_from(generated_height).unwrap_or(0));
        let mut y_start = 0.0f32;
        for _ in 0..generated_height {
            let y_end_extended = X87Chop53::add(load_f32(y_start), load_f32(y_step));
            rows.push(SampleSpan {
                start: y_start,
                end: y_end_extended,
                first: native_ftol(load_f32(y_start)).min(raw_height),
                last: native_ftol(y_end_extended).wrapping_add(1).min(raw_height),
            });
            y_start = store_f32(y_end_extended);
        }
        Self {
            raw_width,
            x_step,
            y_step,
            normalization,
            columns,
            rows,
        }
    }

    fn generate(&self, raw: &[[u8; 3]]) -> Vec<u16> {
        let mut output = Vec::with_capacity(self.columns.len() * self.rows.len());
        for &row in &self.rows {
            for &column in &self.columns {
                output.push(self.sample(raw, column, row));
            }
        }
        output
    }

    fn sample(&self, raw: &[[u8; 3]], column: SampleSpan, row: SampleSpan) -> u16 {
        let normalization = load_f32(self.normalization);
        let mut accum = [X87Chop53::load_i32(0); 3];
        for source_y in row.first..row.last {
            let y_weight = row.weight(source_y, self.y_step);
            for source_x in column.first..column.last {
                let x_weight = column.weight(source_x, self.x_step);
                let weight = X87Chop53::mul(X87Chop53::mul(x_weight, y_weight), normalization);
                let sample = raw[(source_y * self.raw_width + source_x) as usize];
                for channel in 0..3 {
                    accum[channel] = X87Chop53::add(
                        accum[channel],
                        X87Chop53::mul(X87Chop53::load_i32(sample[channel] as i32), weight),
                    );
                }
            }
        }
        // The local active DDrawCompat R5G6B5 store (`0x004BABBE..0x004BABD9`).
        RGB565.pack_rgb8([
            channel_to_u8(accum[0]),
            channel_to_u8(accum[1]),
            channel_to_u8(accum[2]),
        ])
    }

    /// Resample the pixels of `output` (this sampler's generated surface)
    /// whose spans reach the raw `columns` and `rows`.
    fn resample(&self, raw: &[[u8; 3]], output: &mut [u16], columns: Range<i32>, rows: Range<i32>) {
        let width = self.columns.len();
        let columns = Self::reaching(&self.columns, columns);
        for row in Self::reaching(&self.rows, rows) {
            for column in columns.clone() {
                output[row * width + column] =
                    self.sample(raw, self.columns[column], self.rows[row]);
            }
        }
    }

    /// The columns or rows whose raw span reaches `range`.
    fn reaching(spans: &[SampleSpan], range: Range<i32>) -> Range<usize> {
        spans.partition_point(|span| span.last <= range.start)
            ..spans.partition_point(|span| span.first < range.end)
    }
}

/// The HD radar's terrain: the same sampler at a whole multiple of the
/// generated size ([`super::hd`]). A changed cell resamples only the pixels
/// whose spans reach its raw pixels, which gives what full generation would.
#[derive(Debug, Clone)]
struct HdTerrain {
    scale: u32,
    sampler: RadarSampler,
    rgb565: Vec<u16>,
}

impl HdTerrain {
    fn new(raw: &[[u8; 3]], geometry: NativeRadarSurfaceGeometry, scale: u32) -> Self {
        let sampler = RadarSampler::new(
            geometry.raw_size(),
            geometry.subdivided(scale).generated_size(),
        );
        let rgb565 = sampler.generate(raw);
        Self {
            scale,
            sampler,
            rgb565,
        }
    }
}

fn channel_to_u8(accum: X87Value) -> u8 {
    let biased = X87Chop53::add(accum, load_f64(NativeF64Bits::HALF));
    native_ftol(biased).min(255) as u8
}

fn load_f32(value: f32) -> X87Value {
    X87Chop53::load_f32(NativeF32Bits::from_bits(value.to_bits()))
        .expect("native radar sample f32 remains finite")
}

fn load_f64(value: NativeF64Bits) -> X87Value {
    X87Chop53::load_f64(value).expect("native radar sample f64 remains finite")
}

fn store_f32(value: X87Value) -> f32 {
    f32::from_bits(
        X87Chop53::store_f32(value)
            .expect("native radar sample remains finite")
            .bits(),
    )
}

fn native_ftol(value: X87Value) -> i32 {
    X87Chop53::ftol_i64(value).expect("native radar sample fits i32") as i32
}

#[cfg(test)]
#[path = "terrain_tests.rs"]
mod tests;
