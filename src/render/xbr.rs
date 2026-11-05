// Ported from FFmpeg's libavfilter/vf_xbr.c.
// Copyright (c) 2011, 2012 Hyllian/Jararaca <sergiogdb@gmail.com>
// Copyright (c) 2014 Arwa Arif <arwaarif1994@gmail.com>
// Licensed LGPL-2.1-or-later. See LICENSES/LGPL-2.1-or-later.txt in the repo root.

//! xBR, Hyllian's pixel-art scaler, at 2x, 3x and 4x.
//!
//! It draws what FFmpeg 7.1.1's `ffmpeg -vf xbr=n=<factor>` draws: the same
//! colour distance and thresholds, the same packed-pixel blends, and the same
//! edges, where the pixels past an edge repeat it. FFmpeg spells out the four
//! turns of its corner rule; here they are one rule turned. Presentation only.

/// `rgba`, `width` x `height` pixels, at `factor` (2, 3 or 4) times its size.
/// Like FFmpeg's, xBR reads and blends colour only: clear pixels count as
/// black, and each output pixel keeps the alpha of the source pixel it is in.
pub fn scale(rgba: &[u8], width: usize, height: usize, factor: usize) -> Vec<u8> {
    assert!((2..=4).contains(&factor), "xBR scales by 2, 3 or 4");
    assert_eq!(rgba.len(), width * height * 4);
    if width == 0 || height == 0 {
        return Vec::new();
    }
    // The source with its edge pixels repeated as far as xBR reads past them,
    // two: FFmpeg reads a pixel past an edge as the edge pixel. Reading by a
    // fixed offset instead of clamping each read makes the scale about four
    // times faster at opt-level 3.
    let stride = width + 4;
    let mut padded = Vec::with_capacity(stride * (height + 4));
    for row in 0..height + 4 {
        let y = row.saturating_sub(2).min(height - 1);
        for column in 0..stride {
            let x = column.saturating_sub(2).min(width - 1);
            padded.push(Pixel::new(&rgba[(y * width + x) * 4..][..4]));
        }
    }
    let turns: [[isize; 12]; 4] = std::array::from_fn(|turn| {
        ROLES.map(|offset| {
            let (dx, dy) = turned(offset, turn);
            dy * stride as isize + dx
        })
    });
    let slots = slots(factor);
    let line = width * factor;
    let mut out = vec![0; line * height * factor * 4];
    for y in 0..height {
        for x in 0..width {
            let centre = (y + 2) * stride + x + 2;
            let mut block = [padded[centre].rgb; 16];
            for (offsets, slots) in turns.iter().zip(&slots) {
                let roles = offsets.map(|offset| padded[centre.wrapping_add_signed(offset)]);
                if let Some((px, corner)) = corner(roles, factor == 3) {
                    blend(&mut block, slots, factor, corner, px);
                }
            }
            let alpha = rgba[(y * width + x) * 4 + 3];
            for row in 0..factor {
                for column in 0..factor {
                    let [_, r, g, b] = block[row * factor + column].to_be_bytes();
                    let at = ((y * factor + row) * line + x * factor + column) * 4;
                    out[at..at + 4].copy_from_slice(&[r, g, b, alpha]);
                }
            }
        }
    }
    out
}

/// A source pixel: its colour as `0x00RRGGBB` and FFmpeg's YUV for it.
#[derive(Clone, Copy)]
struct Pixel {
    rgb: u32,
    yuv: u32,
}

impl Pixel {
    fn new(rgba: &[u8]) -> Self {
        let rgb = if rgba[3] == 0 {
            0
        } else {
            u32::from_be_bytes([0, rgba[0], rgba[1], rgba[2]])
        };
        Self { rgb, yuv: yuv(rgb) }
    }

    /// `pixel_diff`: the summed YUV distance.
    fn df(self, other: Self) -> u32 {
        let channel = |shift: u32| {
            ((self.yuv >> shift & 0xff) as i32 - (other.yuv >> shift & 0xff) as i32).unsigned_abs()
        };
        channel(16) + channel(8) + channel(0)
    }

    /// `eq`: close enough to count as one colour.
    fn eq(self, other: Self) -> bool {
        self.df(other) < 155
    }
}

/// The entry `init` puts in FFmpeg's 2^24-entry `rgbtoyuv` table for `rgb`,
/// computed rather than looked up. `y` there counts up from a floored
/// division, so it floors here; `u` and `v` keep C's truncation.
fn yuv(rgb: u32) -> u32 {
    let [_, r, g, b] = rgb.to_be_bytes().map(i32::from);
    let (rg, bg) = (r - g, b - g);
    let y = g + (299 * rg + 114 * bg).div_euclid(1000);
    let u = (-169 * rg + 500 * bg) / 1000 + 128;
    let v = (500 * rg - 81 * bg) / 1000 + 128;
    (y as u32) << 16 | (u as u32) << 8 | v as u32
}

/// The neighbours each turn reads, as offsets in its first turn (x right, y
/// down), in FFmpeg's names: PE PI PH PF PG PC PD PB F4 I4 H5 I5.
const ROLES: [(isize, isize); 12] = [
    (0, 0),
    (1, 1),
    (0, 1),
    (1, 0),
    (-1, 1),
    (1, -1),
    (-1, 0),
    (0, -1),
    (2, 0),
    (2, 1),
    (0, 2),
    (1, 2),
];

/// `offset` turned a quarter `turns` times, as FFmpeg's second, third and
/// fourth filter calls turn the first's neighbours.
fn turned((mut dx, mut dy): (isize, isize), turns: usize) -> (isize, isize) {
    for _ in 0..turns {
        (dx, dy) = (dy, -dx);
    }
    (dx, dy)
}

/// Each turn's output slots, as indexes into a `factor`-wide block. The first
/// turn's, around the block's bottom-right corner, are FFmpeg's own names for
/// them in the order its filter macro takes them (FILT4's N15 is index 15);
/// each later turn moves them a quarter, as FFmpeg's later calls do.
fn slots(factor: usize) -> [Vec<usize>; 4] {
    let first: &[usize] = match factor {
        2 => &[1, 2, 3],
        3 => &[2, 5, 6, 7, 8],
        _ => &[15, 14, 11, 3, 7, 10, 13, 12],
    };
    std::array::from_fn(|turn| {
        first
            .iter()
            .map(|&slot| {
                let (mut row, mut column) = (slot / factor, slot % factor);
                for _ in 0..turn {
                    (row, column) = (factor - 1 - column, row);
                }
                row * factor + column
            })
            .collect()
    })
}

/// How one turn blends the colour it picked into its corner of the block.
#[derive(Clone, Copy)]
enum Corner {
    /// The edge is shallow or unsure: only the corner pixel moves.
    Soft,
    Diagonal,
    Left,
    Up,
    LeftAndUp,
}

/// One turn of FFmpeg's `FILT2`/`FILT3`/`FILT4` test (`three` takes
/// `FILT3`'s): the colour to blend in and how, or None to leave the corner.
fn corner(roles: [Pixel; 12], three: bool) -> Option<(u32, Corner)> {
    let [pe, pi, ph, pf, pg, pc, pd, pb, f4, i4, h5, i5] = roles;
    if pe.rgb == ph.rgb || pe.rgb == pf.rgb {
        return None;
    }
    let e = pe.df(pc) + pe.df(pg) + pi.df(h5) + pi.df(f4) + (ph.df(pf) << 2);
    let i = ph.df(pd) + ph.df(i5) + pf.df(i4) + pf.df(pb) + (pe.df(pi) << 2);
    if e > i {
        return None;
    }
    let px = if pe.df(pf) <= pe.df(ph) {
        pf.rgb
    } else {
        ph.rgb
    };
    let sharp = e < i
        && if three {
            !pf.eq(pb) && !pf.eq(pc)
                || !ph.eq(pd) && !ph.eq(pg)
                || pe.eq(pi) && (!pf.eq(f4) && !pf.eq(i4) || !ph.eq(h5) && !ph.eq(i5))
                || pe.eq(pg)
                || pe.eq(pc)
        } else {
            !pf.eq(pb) && !ph.eq(pd)
                || pe.eq(pi) && !pf.eq(i4) && !ph.eq(i5)
                || pe.eq(pg)
                || pe.eq(pc)
        };
    if !sharp {
        return Some((px, Corner::Soft));
    }
    let (ke, ki) = (pf.df(pg), ph.df(pc));
    let left = ke << 1 <= ki && pe.rgb != pg.rgb && pd.rgb != pg.rgb;
    let up = ke >= ki << 1 && pe.rgb != pc.rgb && pb.rgb != pc.rgb;
    let corner = match (left, up) {
        (true, true) => Corner::LeftAndUp,
        (true, false) => Corner::Left,
        (false, true) => Corner::Up,
        (false, false) => Corner::Diagonal,
    };
    Some((px, corner))
}

/// Blend `px` into the block's `slots` as FFmpeg's filter macro for `factor`
/// does for `corner`, in its order.
fn blend(block: &mut [u32; 16], slots: &[usize], factor: usize, corner: Corner, px: u32) {
    let mut at = |slot: usize, m: u32, s: u32| {
        let index = slots[slot];
        block[index] = toward(block[index], px, m, s);
    };
    match (factor, corner) {
        // FILT2: N1 N2 N3.
        (2, Corner::Soft | Corner::Diagonal) => at(2, 4, 3),
        (2, Corner::Left) => {
            at(2, 3, 2);
            at(1, 1, 2);
        }
        (2, Corner::Up) => {
            at(2, 3, 2);
            at(0, 1, 2);
        }
        (2, Corner::LeftAndUp) => {
            at(2, 7, 3);
            at(1, 1, 2);
            block[slots[0]] = block[slots[1]];
        }
        // FILT3: N2 N5 N6 N7 N8.
        (3, Corner::Soft) => at(4, 4, 3),
        (3, Corner::Diagonal) => {
            at(4, 7, 3);
            at(1, 1, 3);
            at(3, 1, 3);
        }
        (3, Corner::Left) => {
            at(3, 3, 2);
            at(1, 1, 2);
            at(2, 1, 2);
            block[slots[4]] = px;
        }
        (3, Corner::Up) => {
            at(1, 3, 2);
            at(3, 1, 2);
            at(0, 1, 2);
            block[slots[4]] = px;
        }
        (3, Corner::LeftAndUp) => {
            at(3, 3, 2);
            at(2, 1, 2);
            block[slots[1]] = block[slots[3]];
            block[slots[0]] = block[slots[2]];
            block[slots[4]] = px;
        }
        // FILT4: N15 N14 N11 N3 N7 N10 N13 N12.
        (_, Corner::Soft) => at(0, 4, 3),
        (_, Corner::Diagonal) => {
            at(2, 4, 3);
            at(1, 4, 3);
            block[slots[0]] = px;
        }
        (_, Corner::Left) => {
            at(2, 3, 2);
            at(6, 3, 2);
            at(5, 1, 2);
            at(7, 1, 2);
            block[slots[1]] = px;
            block[slots[0]] = px;
        }
        (_, Corner::Up) => {
            at(1, 3, 2);
            at(4, 3, 2);
            at(5, 1, 2);
            at(3, 1, 2);
            block[slots[2]] = px;
            block[slots[0]] = px;
        }
        (_, Corner::LeftAndUp) => {
            at(6, 3, 2);
            at(7, 1, 2);
            for slot in [0, 1, 2] {
                block[slots[slot]] = px;
            }
            for slot in [5, 3] {
                block[slots[slot]] = block[slots[7]];
            }
            block[slots[4]] = block[slots[6]];
        }
    }
}

/// `a` moved `m / 2^s` of the way to `b`, each channel of the packed pixel
/// at once with FFmpeg's wrapping arithmetic: `ALPHA_BLEND_BASE`, except that
/// a half (4/8) is `ALPHA_BLEND_128_W`, which halves both and adds.
fn toward(a: u32, b: u32, m: u32, s: u32) -> u32 {
    if (m, s) == (4, 3) {
        const LOW_BITS_CLEAR: u32 = 0x00FE_FEFE;
        return ((a & LOW_BITS_CLEAR) >> 1) + ((b & LOW_BITS_CLEAR) >> 1);
    }
    let channels = |mask: u32| {
        mask & (a & mask).wrapping_add((b & mask).wrapping_sub(a & mask).wrapping_mul(m) >> s)
    };
    channels(0x00FF_00FF) | channels(0x0000_FF00)
}

#[cfg(test)]
#[path = "xbr_tests.rs"]
mod tests;
