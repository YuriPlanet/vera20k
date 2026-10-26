//! Sonic and magnetic WaveClass raster on the shared packed destination.
//!
//! Wave world geometry belongs to `sim::wave`; this module consumes its
//! projected polygon. Original controls: procedural_drawing_oracle/magnetic_wave.

use crate::util::native_x87::{NativeF64Bits, X87Chop53 as X};
use std::sync::OnceLock;

/// Native7602E0/75FA90 have different clipping at its contiguous and ring-wrapped Z
/// branches. The screen-texture renderer uses Fast; Fallback exists only in
/// tests, for the original component controls, without inventing simulation
/// ring state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaveClipPath {
    Fast,
    #[cfg(test)]
    Fallback,
}

/// The two native Wave polygon bodies have different vertex counts, with one
/// shared63F080/63F2E0 scan conversion owner. The shape also selects its native
/// sine table and packed-color operation; it is derived presentation input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WavePolygon {
    Magnetic([[i32; 2]; 4]),
    Sonic([[i32; 2]; 6]),
}

impl WavePolygon {
    pub(crate) fn points(&self) -> &[[i32; 2]] {
        match self {
            Self::Magnetic(points) => points,
            Self::Sonic(points) => points,
        }
    }
}

/// All XY values are integer, unzoomed screen pixels after the same camera
/// translation. `source_point` is the projection of native B4/CC, not a corner.
/// No renderer reconstructs the sim-owned761640/762070 polygon or direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WaveDraw {
    pub polygon: WavePolygon,
    pub source_point: [i32; 2],
    pub source_z: i32,
    pub phase: i32,
    pub direction: u8,
    pub clip_path: WaveClipPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PolygonSpans {
    pub top: i32,
    pub rows: Vec<[i32; 2]>,
}

/// Original polygon63F080 and edge63F2E0, from gamemd.exe. Wave0/3 supply
/// six-/four-point polygons; projected or interpolated edges can fold back.
/// Top-chain orientation, initial skip and the right-side minus-one are
/// preserved before clipping; GPU triangulation would differ.
///
/// Once fade_in reaches fade_out, the Sonic projection
/// (`Wave::sonic_draw_geometry`) collapses to the out-and-back slit
/// `[c, source, d, d, source, c]`. When the source vertex is the vertical
/// extreme, a side walks past the maximum vertex into the scratch tail below
/// (the dolphin pile-on in #3103).
pub(crate) fn polygon_spans(points: &[[i32; 2]]) -> PolygonSpans {
    let mut minimum = 0;
    let mut maximum = 0;
    for i in 1..points.len() {
        if points[i][1] < points[minimum][1] {
            minimum = i;
        } else if points[i][1] > points[maximum][1] {
            maximum = i;
        }
    }
    let min_y = points[minimum][1];
    let max_y = points[maximum][1];
    if min_y == max_y {
        return PolygonSpans {
            top: 0,
            rows: Vec::new(),
        };
    }
    let next = |i: usize| (i + 1) % points.len();
    let previous = |i: usize| (i + points.len() - 1) % points.len();
    let mut after_top = minimum;
    while points[after_top][1] == min_y {
        after_top = next(after_top);
    }
    let mut right = previous(after_top);
    let mut before_top = minimum;
    while points[before_top][1] == min_y {
        before_top = previous(before_top);
    }
    let mut left = next(before_top);
    let include_top = points[left][0] != points[right][0];
    let mut direction = -1;
    let swap = if include_top {
        points[right][0] < points[left][0]
    } else {
        let origin = points[left];
        let a = points[previous(left)];
        let b = points[next(right)];
        a[1].wrapping_sub(origin[1])
            .wrapping_mul(b[0].wrapping_sub(origin[0]))
            .wrapping_sub(
                b[1].wrapping_sub(origin[1])
                    .wrapping_mul(a[0].wrapping_sub(origin[0])),
            )
            < 0
    };
    if swap {
        std::mem::swap(&mut left, &mut right);
        direction = 1;
    }
    let count = max_y.wrapping_sub(min_y).wrapping_sub(1) + i32::from(include_top);
    if count <= 0 {
        return PolygonSpans {
            top: 0,
            rows: Vec::new(),
        };
    }
    let mut spans = PolygonSpans {
        top: min_y.wrapping_sub(i32::from(include_top)).wrapping_add(1),
        rows: vec![[0; 2]; count as usize],
    };
    for (side, mut index, step) in [(0, left, direction), (1, right, -direction)] {
        let mut row = 0;
        let mut skip = !include_top;
        loop {
            let following = if step > 0 {
                next(index)
            } else {
                previous(index)
            };
            let mut start = points[index];
            let mut end = points[following];
            if side == 1 {
                start[0] = start[0].wrapping_sub(1);
                end[0] = end[0].wrapping_sub(1);
            }
            span_edge(start, end, skip, |x| {
                // gamemd.exe FUN_0063F080 @ 0x63F080 allocates 200 scratch
                // rows after the logical count (0x63F1DF). Folded edges can
                // write into that tail;
                // Draw_Sonic75FA90/Draw_MagBeam7602E0 read only this prefix.
                if let Some(span) = spans.rows.get_mut(row) {
                    span[side] = x;
                }
                row += 1;
            });
            index = following;
            skip = false;
            if index == maximum {
                break;
            }
        }
        assert!(row >= spans.rows.len(), "native polygon edge coverage");
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::polygon_spans;

    #[test]
    fn folded_edges_keep_the_native_visible_prefix_on_both_sides() {
        // Original FUN_0063F080/0063F2E0 (gamemd.exe) execution: two extra
        // right-edge rows for the quad, one extra left-edge row for the hexagon.
        let quad = polygon_spans(&[[0, 0], [4, 4], [2, 2], [0, 5]]);
        assert_eq!(quad.top, 1);
        assert_eq!(quad.rows, [[0, 0], [0, 1], [0, 2], [0, 1]]);
        let hexagon = polygon_spans(&[[0, 0], [0, 5], [1, 4], [1, 5], [0, 4], [0, 1]]);
        assert_eq!(hexagon.top, 1);
        assert_eq!(hexagon.rows, [[0, -1]; 4]);
    }
}

///63F2E0 uses signed division and sign-dependent initial error. Its endpoint Y
/// is excluded; only the first edge can skip its first row.
fn span_edge(start: [i32; 2], end: [i32; 2], skip: bool, mut emit: impl FnMut(i32)) {
    let dx = end[0].wrapping_sub(start[0]);
    let dy = end[1].wrapping_sub(start[1]);
    if dy <= 0 {
        return;
    }
    let width = dx.wrapping_abs();
    let step = if dx > 0 { 1_i32 } else { -1_i32 };
    let mut x = start[0];
    let skip = i32::from(skip);
    if width == 0 {
        for _ in 0..dy.wrapping_sub(skip) {
            emit(x);
        }
        return;
    }
    if width == dy {
        if skip != 0 {
            x = x.wrapping_add(step);
        }
        for _ in 0..dy.wrapping_sub(skip) {
            emit(x);
            x = x.wrapping_add(step);
        }
        return;
    }
    let (advance, remainder) = if width < dy {
        (0, width)
    } else {
        ((width / dy).wrapping_mul(step), width % dy)
    };
    let mut error = if dx < 0 { 1_i32.wrapping_sub(dy) } else { 0 };
    if skip != 0 {
        error = error.wrapping_add(remainder);
        x = x.wrapping_add(advance);
        if error > 0 {
            x = x.wrapping_add(step);
            error = error.wrapping_sub(dy);
        }
    }
    for _ in 0..dy.wrapping_sub(skip) {
        emit(x);
        error = error.wrapping_add(remainder);
        x = x.wrapping_add(advance);
        if error > 0 {
            x = x.wrapping_add(step);
            error = error.wrapping_sub(dy);
        }
    }
}

/// Immutable75F020 derived tables. Numeric kernels remain owned by the shared
/// native x87/trig modules; only this Wave-specific initialization lives here.
pub(crate) struct WaveTables {
    pub distance: Vec<u16>,
    pub magnetic: [i16; 496],
    pub sonic: [i16; 500],
    pub brightness: [i32; 13],
}

impl WaveTables {
    ///7601D1 reads a raw linear address. The initialized adjacent window is
    ///mag496, six zero-coordinate shorts (75E8F0), then Sonic500 (75F020).
    ///Do not replace an index beyond495 by a clamp or a periodic sine lookup.
    pub(crate) fn magnetic_sine(&self, index: i32) -> Option<i16> {
        match index {
            0..=495 => Some(self.magnetic[index as usize]),
            496..=501 => Some(0),
            502..=1001 => Some(self.sonic[index as usize - 502]),
            _ => None,
        }
    }
}

fn wave_sine_table<const N: usize>(frequency: NativeF64Bits) -> [i16; N] {
    std::array::from_fn(|i| {
        let frequency = X::load_f64(frequency).unwrap();
        let angle = X::store_f64(X::mul(X::load_i32(i as i32), frequency)).unwrap();
        let sine = crate::util::retail_trig::TrigTable::embedded()
            .sin_from_table(X::load_f64(angle).unwrap());
        let amplitude = X::load_f64(NativeF64Bits::from_bits(0x4028_0000_0000_0000)).unwrap();
        let bias = X::load_f64(NativeF64Bits::from_bits(0x3fdf_5c28_f5c2_8f5c)).unwrap();
        X::ftol_i32_low_masked(X::add(X::mul(sine, amplitude), bias)) as i16
    })
}

pub(crate) fn wave_tables() -> &'static WaveTables {
    static TABLES: OnceLock<WaveTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let distance = (0..300_i32)
            .flat_map(|y| {
                (0..300_i32).map(move |x| {
                    let root = crate::util::native_x87::sqrt_approx_f32(X::load_i32(x * x + y * y))
                        .expect("finite native Wave table squared distance");
                    X::ftol_i32_low_masked(X::load_f32(root).expect("finite native sqrt table"))
                        as u16
                })
            })
            .collect();
        WaveTables {
            distance,
            magnetic: wave_sine_table(NativeF64Bits::from_bits(0x3fb9_9999_9999_999a)),
            sonic: wave_sine_table(NativeF64Bits::from_bits(0x3fc0_0000_0000_0000)),
            brightness: std::array::from_fn(|i| 110 + i as i32 * 8),
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WavePixel {
    pub point: [i32; 2],
    pub source: [i32; 2],
    pub candidate_z: u16,
    pub brightness: i32,
    pub distance_index: i32,
    pub sine_index: i32,
}

/// These are unsupported native memory domains, never a request to clamp or
/// substitute an empty pixel. Production must surface the exact offending draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaveRasterError {
    Surface { size: [i32; 2], pitch_words: i32 },
    Direction(u8),
    DistanceIndex(i32),
    SineIndex(i32),
    Destination([i32; 2]),
    SourceWord(i32),
}

impl WaveDraw {
    ///7602E0/75FA90: native traversal order, strict-depth candidates and exact
    /// displaced reads. The caller alone owns live color/Z; this emits no A
    /// query and never writes Z. `pitch_words` includes declared native padding.
    pub(crate) fn rasterize(
        self,
        clip: [i32; 4],
        size: [i32; 2],
        pitch_words: i32,
        z_origin_y: i32,
        mut emit: impl FnMut(WavePixel),
    ) -> Result<(), WaveRasterError> {
        if size[0] <= 1 || size[1] <= 0 || pitch_words < size[0] {
            return Err(WaveRasterError::Surface { size, pitch_words });
        }
        let forward = match self.direction {
            2..=5 => true,
            0 | 1 | 6 | 7 => false,
            other => return Err(WaveRasterError::Direction(other)),
        };
        let spans = polygon_spans(self.polygon.points());
        if spans.rows.is_empty() {
            return Ok(());
        }
        let inset = match self.clip_path {
            WaveClipPath::Fast => 3,
            #[cfg(test)]
            WaveClipPath::Fallback => 2,
        };
        let last_y = spans
            .top
            .wrapping_add(spans.rows.len() as i32)
            .wrapping_sub(1)
            .min(clip[1].wrapping_add(clip[3]));
        if last_y < spans.top {
            return Ok(());
        }
        let adjust_z = crate::util::native_x87::adjust_for_z_standard(self.source_z);
        let mut row = |y: i32| -> Result<(), WaveRasterError> {
            if y < clip[1].wrapping_add(inset)
                || y > clip[1].wrapping_add(clip[3]).wrapping_sub(inset)
            {
                return Ok(());
            }
            let candidate_z = crate::render::native_z::DEFAULT_Z
                .wrapping_add(z_origin_y)
                .wrapping_sub(adjust_z)
                .wrapping_sub(y)
                .wrapping_sub(2) as u16;
            // Every native stored Z is u16; none can pass >65535. The
            //original Pixel/table/source reads are unreachable on this row.
            if candidate_z == u16::MAX {
                return Ok(());
            }
            let [left, right] = spans.rows[y.wrapping_sub(spans.top) as usize];
            let left = left.max(clip[0].wrapping_add(inset));
            let right = right.min(clip[0].wrapping_add(clip[2]).wrapping_sub(inset));
            let mut pixel = |x: i32| -> Result<(), WaveRasterError> {
                emit(self.pixel_operation([x, y], size, pitch_words, candidate_z)?);
                Ok(())
            };
            if forward {
                for x in left..=right {
                    pixel(x)?;
                }
            } else {
                for x in (left..=right).rev() {
                    pixel(x)?;
                }
            }
            Ok(())
        };
        if forward {
            for y in spans.top..=last_y {
                row(y)?;
            }
        } else {
            for y in (spans.top..=last_y).rev() {
                row(y)?;
            }
        }
        Ok(())
    }
    /// Original Pixel760190/75EDF0 lookup/displacement, shared with the
    /// whole7602E0/75FA90 raster. The supplied depth belongs to its caller; this
    /// leaf performs no depth/A read or packed-color store on the CPU.
    pub(crate) fn pixel_operation(
        self,
        point: [i32; 2],
        size: [i32; 2],
        pitch_words: i32,
        candidate_z: u16,
    ) -> Result<WavePixel, WaveRasterError> {
        let [x, y] = point;
        let steps = [
            pitch_words.wrapping_neg(),
            1_i32.wrapping_sub(pitch_words),
            1,
            pitch_words.wrapping_add(1),
            pitch_words,
            pitch_words.wrapping_sub(1),
            -1,
            pitch_words.wrapping_neg().wrapping_sub(1),
        ];
        let step = *steps
            .get(usize::from(self.direction))
            .ok_or(WaveRasterError::Direction(self.direction))?;
        let tables = wave_tables();
        if x < 0 || y < 0 || x >= size[0] || y >= size[1] {
            return Err(WaveRasterError::Destination([x, y]));
        }
        let distance_index = y
            .wrapping_sub(self.source_point[1])
            .wrapping_abs()
            .wrapping_mul(300)
            .wrapping_add(x.wrapping_sub(self.source_point[0]).wrapping_abs());
        let distance = usize::try_from(distance_index)
            .ok()
            .and_then(|i| tables.distance.get(i))
            .copied()
            .ok_or(WaveRasterError::DistanceIndex(distance_index))?;
        let sine_index = i32::from(distance).wrapping_add(self.phase);
        let sine = match self.polygon {
            WavePolygon::Magnetic(_) => tables.magnetic_sine(sine_index),
            WavePolygon::Sonic(_) => usize::try_from(sine_index)
                .ok()
                .and_then(|i| tables.sonic.get(i).copied()),
        }
        .ok_or(WaveRasterError::SineIndex(sine_index))?;
        let magnitude = match self.polygon {
            WavePolygon::Magnetic(_) => i32::from(sine).abs().min(9),
            WavePolygon::Sonic(_) => i32::from(sine).abs(),
        } as usize;
        let multiple = match magnitude {
            0..=1 => 0,
            2..=6 => 1,
            7..=9 => 2,
            10..=12 => 3,
            _ => return Err(WaveRasterError::SineIndex(sine_index)),
        };
        let source_word = y
            .wrapping_mul(pitch_words)
            .wrapping_add(x)
            .wrapping_add(step.wrapping_mul(multiple));
        if source_word < 0 || source_word >= pitch_words.wrapping_mul(size[1]) {
            return Err(WaveRasterError::SourceWord(source_word));
        }
        let source = [source_word % pitch_words, source_word / pitch_words];
        if source[0] >= size[0] {
            return Err(WaveRasterError::SourceWord(source_word));
        }
        Ok(WavePixel {
            point: [x, y],
            source,
            candidate_z,
            brightness: tables.brightness[magnitude],
            distance_index,
            sine_index,
        })
    }
}
