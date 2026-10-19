//! The IonBlast ripple tables and their displacements.
//!
//! `Show_Loading_Screen` builds the 80 ripple frames once per process
//! (`0x0053D330`, latch `0x00AA014C`): 512x256 one-byte surfaces
//! (`0x00A9FFC8..0x00AA0108`) filled with -1. Frame `n` writes every point
//! whose distance `d = Sqrt_Approx(x² + 4y²)` from the centre lies in a ring
//! `[lo, hi]`: the spiral index of `(0, v)` (`0x0053D960`), with
//! `v = ftol((SinFromTable(((d - n * step) + 38) * 0.11) * 3.5 + 3) /
//! (d / 51 + 1) + 0.5)`, mirrored into the four quadrants around (256, 128).
//! The ring starts at `[-57, 0]`; after each frame `hi` grows by `step` while
//! `256 - step > hi`, `lo` by `1.2 * step`, and `lo` stops at `hi`. From frame
//! 36 the ring is empty and the frame stays -1.
//!
//! The draw (`IonBlastClass @ 0x0053D580`) moves a destination pixel whose
//! byte is positive to the pixel its spiral point gives (`0x0053D8E0`, the
//! table DrawAll builds at `0x00A9FAE8`): the table holds `y * pitch + x`
//! with the pitch in bytes, read through a 16-bit pixel pointer, so the
//! point's `y` moves two rows.
//!
//! Native execution: `tools/superweapon_oracle.py` section `ion_blast_ripple`
//! (the generator and both initializers, under the control words below).

use crate::map::retail_trig::TrigTable;
use crate::util::native_x87::{NativeF64Bits, X87Chop53, X87Ordering, X87Value, sqrt_approx_f32};

/// Width of a ripple frame (`BSurface` width `0x200`, `0x0053D39E`).
pub const FRAME_WIDTH: usize = 512;
/// Height of a ripple frame (`0x100`, `0x0053D3A5`).
pub const FRAME_HEIGHT: usize = 256;
/// Frames in the table (`0x00A9FFC8..0x00AA0108`).
pub const FRAME_COUNT: usize = 80;

/// The ring's step (`0x00A9FF80`, copied to `0x00A9FFA8`):
/// `(512 - 0 + 57) * 0.0125` (`0x0053CA30`), a C++ static initializer.
/// It runs from `_cinit` (`0x007CBDAF`) after `_setdefaultprecision`
/// (53-bit) and before WinMain sets truncation (`_controlfp(0x300, 0x300)`
/// at `0x006BBFC1`), so it rounds to nearest: `7.112500000000001`.
pub(crate) const RING_STEP: NativeF64Bits = NativeF64Bits::from_bits(0x401c_7333_3333_3334);

const F64_57: NativeF64Bits = NativeF64Bits::from_bits(0x404c_8000_0000_0000);
const F64_38: NativeF64Bits = NativeF64Bits::from_bits(0x4043_0000_0000_0000);
const F64_0_11: NativeF64Bits = NativeF64Bits::from_bits(0x3fbc_28f5_c28f_5c29);
const F64_3_5: NativeF64Bits = NativeF64Bits::from_bits(0x400c_0000_0000_0000);
const F64_3: NativeF64Bits = NativeF64Bits::from_bits(0x4008_0000_0000_0000);
const F64_51: NativeF64Bits = NativeF64Bits::from_bits(0x4049_8000_0000_0000);
const F64_256: NativeF64Bits = NativeF64Bits::from_bits(0x4070_0000_0000_0000);
const F64_1_2: NativeF64Bits = NativeF64Bits::from_bits(0x3ff3_3333_3333_3333);

/// The 80 ripple frames, each `FRAME_WIDTH * FRAME_HEIGHT` signed bytes in
/// rows.
pub struct RippleFrames {
    bytes: Vec<u8>,
}

impl RippleFrames {
    /// One immutable native-generated table shared by simulation and drawing.
    pub fn shared() -> &'static Self {
        static FRAMES: std::sync::OnceLock<RippleFrames> = std::sync::OnceLock::new();
        FRAMES.get_or_init(|| Self::generate(TrigTable::embedded()))
    }

    /// Update53CF39 samples the middle row at projected distance plus8.
    pub(crate) fn force_sample(&self, frame: i32, radius: i32) -> i8 {
        self.bytes[frame as usize * FRAME_WIDTH * FRAME_HEIGHT
            + FRAME_HEIGHT / 2 * FRAME_WIDTH
            + FRAME_WIDTH / 2
            + radius as usize] as i8
    }

    /// `0x0053D330` under the process control word (53-bit, truncating),
    /// reading the retail sine table.
    pub(crate) fn generate(trig: &TrigTable) -> Self {
        let f64 = |bits| X87Chop53::load_f64(bits).expect("finite constant");
        let step = f64(RING_STEP);
        // `0x0053D34A..0x0053D376`: 0 - 57 and 0.
        let mut lo = X87Chop53::sub(f64(NativeF64Bits::POSITIVE_ZERO), f64(F64_57));
        let mut hi = f64(NativeF64Bits::POSITIVE_ZERO);
        // Sqrt_Approx of x² + 4y² does not depend on the frame.
        let distances: Vec<X87Value> = (0..FRAME_HEIGHT as i32 / 2)
            .flat_map(|y| (0..FRAME_WIDTH as i32 / 2).map(move |x| x * x + y * y * 4))
            .map(|sum| {
                let root = sqrt_approx_f32(X87Chop53::load_i32(sum)).expect("finite sum");
                X87Chop53::load_f32(root).expect("Sqrt_Approx returns a finite value")
            })
            .collect();
        let mut bytes = vec![0xff; FRAME_COUNT * FRAME_WIDTH * FRAME_HEIGHT];
        for (n, frame) in bytes
            .as_chunks_mut::<{ FRAME_WIDTH * FRAME_HEIGHT }>()
            .0
            .iter_mut()
            .enumerate()
        {
            let advance = X87Chop53::mul(X87Chop53::load_i32(n as i32), step);
            for y in (0..FRAME_HEIGHT / 2).rev() {
                for x in (0..FRAME_WIDTH / 2).rev() {
                    let d = distances[y * FRAME_WIDTH / 2 + x];
                    if X87Chop53::compare(d, lo) == X87Ordering::Less
                        || X87Chop53::compare(d, hi) == X87Ordering::Greater
                    {
                        continue;
                    }
                    let angle = X87Chop53::mul(
                        X87Chop53::add(X87Chop53::sub(d, advance), f64(F64_38)),
                        f64(F64_0_11),
                    );
                    let wave = X87Chop53::add(
                        X87Chop53::mul(trig.sin_from_table(angle), f64(F64_3_5)),
                        f64(F64_3),
                    );
                    let falloff = X87Chop53::add(
                        X87Chop53::div(d, f64(F64_51)).expect("nonzero divisor"),
                        f64(NativeF64Bits::ONE),
                    );
                    let height = X87Chop53::add(
                        X87Chop53::div(wave, falloff).expect("the falloff is at least 1"),
                        f64(NativeF64Bits::HALF),
                    );
                    let byte = spiral_index(0, X87Chop53::ftol_i32_low_masked(height)) as u8;
                    let (cx, cy) = (FRAME_WIDTH / 2, FRAME_HEIGHT / 2);
                    for (px, py) in [
                        (cx + x, cy + y),
                        (cx - x, cy + y),
                        (cx + x, cy - y),
                        (cx - x, cy - y),
                    ] {
                        frame[py * FRAME_WIDTH + px] = byte;
                    }
                }
            }
            // `0x0053D4CB..0x0053D51F`.
            let room = X87Chop53::sub(f64(F64_256), step);
            if X87Chop53::compare(room, hi) == X87Ordering::Greater {
                hi = X87Chop53::add(step, hi);
            }
            lo = X87Chop53::add(X87Chop53::mul(step, f64(F64_1_2)), lo);
            if X87Chop53::compare(lo, hi) == X87Ordering::Greater {
                lo = hi;
            }
        }
        Self { bytes }
    }

    /// Every frame, in order, each in rows of [`FRAME_WIDTH`].
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// `0x0053D960`: the index of point `(x, y)` on the square spiral around the
/// origin, ring `m = max(|x|, |y|)` starting at `1 + 4m(m - 1)`.
fn spiral_index(x: i32, y: i32) -> i32 {
    if x == 0 && y == 0 {
        return 0;
    }
    let m = x.wrapping_abs().max(y.wrapping_abs());
    let base = 1 + (1..m).map(|k| k * 8).sum::<i32>();
    if x == m {
        base + y + m
    } else if y == m {
        base + (m * 3 - x)
    } else if x == -m {
        base + (m * 5 - y)
    } else {
        base + m * 7 + x
    }
}

/// `0x0053D8E0`: the point at spiral index `index` (1 or more), the inverse
/// of [`spiral_index`].
pub(crate) fn spiral_point(index: i32) -> (i32, i32) {
    let mut rest = index - 1;
    let mut ring = 1;
    if rest >= 8 {
        let mut length = 8;
        loop {
            rest -= length;
            length += 8;
            ring += 1;
            if rest < length {
                break;
            }
        }
    }
    if rest < ring * 2 + 1 {
        (ring, rest - ring)
    } else if rest < ring * 4 + 1 {
        (ring * 3 - rest, ring)
    } else if rest < ring * 6 + 1 {
        (-ring, ring * 5 - rest)
    } else {
        (rest - ring * 7, -ring)
    }
}

/// The pixels the draw moves a destination pixel by for a positive ripple
/// byte: `(x, 2y)` of its spiral point (module doc).
pub fn displacement(byte: u8) -> Option<[i32; 2]> {
    let index = i32::from(byte as i8);
    (index > 0).then(|| {
        let (x, y) = spiral_point(index);
        [x, y * 2]
    })
}

#[cfg(test)]
#[path = "ion_blast_ripple_tests.rs"]
mod tests;
