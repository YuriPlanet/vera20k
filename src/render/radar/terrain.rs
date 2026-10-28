//! Active-YR raw RGB and generated packed radar terrain surfaces.
//!
//! This presentation-local cache mirrors `FillTerrainColors @ 0x00654EA0`
//! and the full-surface path through `GenerateTerrainSurface @ 0x006547C0`.

use std::collections::BTreeMap;

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

    #[cfg(test)]
    pub(super) fn raw_rgb(&self) -> &[[u8; 3]] {
        &self.raw_rgb
    }

    // Original655250 regenerates the dirty union rectangle through6547C0.
    // This cache rebuilds the full surface instead. The consumed ordinary-ore
    // control matches original full generation; incremental generation differs
    // at five pixels in one3x2 patch/fringe row (ore_render.json radar).
    pub fn set_cell_states(
        &mut self,
        changes: impl IntoIterator<Item = NativeRadarCellUpdate>,
    ) -> bool {
        let mut changed = false;
        for update in changes {
            let (rx, ry) = update.base.cell;
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
        }
        if changed {
            self.rebuild();
        }
        changed
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
        self.generated_rgb565 =
            generate_rgb565(&self.raw_rgb, raw_size, self.geometry.generated_size());
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

/// Full-surface active-YR weighted-area sampler. Every f32 store below maps to
/// the explicit `FST[P] float` sites at `0x006549D3..0x006549EC` and the
/// advancing edge stores inside `0x00654AF5..0x00654DD7`.
fn generate_rgb565(raw: &[[u8; 3]], raw_size: (i32, i32), generated_size: (i32, i32)) -> Vec<u16> {
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
    let one = load_f32(1.0);
    let mut output = Vec::with_capacity(area(generated_size));
    let mut y_start = 0.0f32;

    for _ in 0..generated_height {
        let y_end_extended = X87Chop53::add(load_f32(y_start), load_f32(y_step));
        let y_first = native_ftol(load_f32(y_start)).min(raw_height);
        let y_last = native_ftol(y_end_extended).wrapping_add(1).min(raw_height);
        let row_count = y_last.wrapping_sub(y_first);
        let mut x_start = 0.0f32;

        for _ in 0..generated_width {
            let x_end_extended = X87Chop53::add(load_f32(x_start), load_f32(x_step));
            let x_end = store_f32(x_end_extended);
            let x_first = native_ftol(load_f32(x_start)).min(raw_width);
            let x_last = native_ftol(x_end_extended).wrapping_add(1).min(raw_width);
            let column_count = x_last.wrapping_sub(x_first);
            let mut accum = [X87Chop53::load_i32(0); 3];

            for source_y in y_first..y_last {
                let y_weight = if row_count <= 1 {
                    load_f32(y_step)
                } else if source_y == y_first {
                    X87Chop53::sub(
                        X87Chop53::load_i32(source_y.wrapping_add(1)),
                        load_f32(y_start),
                    )
                } else if source_y == y_last.wrapping_sub(1) {
                    X87Chop53::sub(y_end_extended, X87Chop53::load_i32(source_y))
                } else {
                    one
                };
                for source_x in x_first..x_last {
                    let x_weight = if column_count <= 1 {
                        load_f32(x_step)
                    } else if source_x == x_first {
                        X87Chop53::sub(
                            X87Chop53::load_i32(source_x.wrapping_add(1)),
                            load_f32(x_start),
                        )
                    } else if source_x == x_last.wrapping_sub(1) {
                        X87Chop53::sub(load_f32(x_end), X87Chop53::load_i32(source_x))
                    } else {
                        one
                    };
                    let weight =
                        X87Chop53::mul(X87Chop53::mul(x_weight, y_weight), load_f32(normalization));
                    let sample = raw[(source_y * raw_width + source_x) as usize];
                    for channel in 0..3 {
                        accum[channel] = X87Chop53::add(
                            accum[channel],
                            X87Chop53::mul(X87Chop53::load_i32(sample[channel] as i32), weight),
                        );
                    }
                }
            }
            // The local active DDrawCompat R5G6B5 store (`0x004BABBE..0x004BABD9`).
            output.push(RGB565.pack_rgb8([
                channel_to_u8(accum[0]),
                channel_to_u8(accum[1]),
                channel_to_u8(accum[2]),
            ]));
            x_start = x_end;
        }
        y_start = store_f32(y_end_extended);
    }
    output
}

fn channel_to_u8(accum: X87Value) -> u8 {
    let biased = X87Chop53::add(accum, load_f64(NativeF64Bits::HALF));
    native_ftol(biased).min(255) as u8
}

pub(super) fn half_bright_rgb565(pixel: u16) -> [u8; 4] {
    let [red, green, blue] = RGB565.unpack_rgb8(pixel);
    let [red, green, blue] =
        RGB565.unpack_rgb8(RGB565.pack_rgb8([red >> 1, green >> 1, blue >> 1]));
    [red, green, blue, 255]
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
