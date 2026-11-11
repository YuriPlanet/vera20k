//! Glyph outlines drawn as hard pixels, as GAME.FNT's letters were: each
//! pixel is on or off, never smoothed. [`super::fnt_fill`] draws GAME.FNT's
//! missing letters this way, and every glyph draws this way on the line's
//! native (800x600) pixel grid, so no interface text is smooth: GAME.FNT's
//! own squares light exactly its pixels, and squeezed narrower, shrunk or off
//! the grid they still light whole ones. A drawn glyph then scales evenly to
//! the window, each native pixel the same block of window pixels.

use std::collections::HashSet;
use std::ops::Range;

use swash::scale::outline::Outline;
use swash::zeno::{Command, PathData as _};

use super::RasterGlyph;

/// Straight segments each curve of an outline is split into.
const CURVE_STEPS: u8 = 8;

/// The `(column, row)` native pixels `outline` lights with its pen `phase`
/// pixels right of and below a pixel corner: columns from that corner, rows
/// down from it, so the row above the pen's baseline is -1 at no phase.
pub(super) fn of_outline(outline: &Outline, phase: [f32; 2]) -> HashSet<(i32, i32)> {
    let mut segments = segments(outline);
    for segment in &mut segments {
        segment[0] += phase[0];
        segment[2] += phase[0];
    }
    let ys = segments.iter().flat_map(|s| [s[1], s[3]]);
    let (bottom, top) = ys.fold((f32::MAX, f32::MIN), |(b, t), y| (b.min(y), t.max(y)));
    if bottom > top {
        return HashSet::new();
    }
    let rows = (phase[1] - top).floor() as i32..(phase[1] - bottom).ceil() as i32;
    lit(&segments, rows, phase[1])
}

/// `lit` pixels drawn `scale` window pixels each, as coverage, pixel (0, 0)'s
/// top-left corner `phase` window pixels right of the anchor the bitmap's
/// offset counts from. A whole scale gives solid blocks; a fractional one
/// blends only where blocks meet a window pixel's edge, as GAME.FNT's own
/// glyphs scale. `None` without pixels.
pub(super) fn scaled(lit: &HashSet<(i32, i32)>, scale: f32, phase: f32) -> Option<RasterGlyph> {
    let columns = lit.iter().map(|&(column, _)| column);
    let rows = lit.iter().map(|&(_, row)| row);
    let (first_column, last_column) = (columns.clone().min()?, columns.max()?);
    let (first_row, last_row) = (rows.clone().min()?, rows.max()?);
    let left = (phase + first_column as f32 * scale).floor();
    let top = (first_row as f32 * scale).floor();
    let width = ((phase + (last_column + 1) as f32 * scale).ceil() - left) as usize;
    let height = (((last_row + 1) as f32 * scale).ceil() - top) as usize;
    let mut cover = vec![0.0f32; width * height];
    for &(column, row) in lit {
        let x0 = phase + column as f32 * scale - left;
        let y0 = row as f32 * scale - top;
        let (x1, y1) = (x0 + scale, y0 + scale);
        for y in y0.floor() as usize..(y1.ceil() as usize).min(height) {
            let dy = y1.min(y as f32 + 1.0) - y0.max(y as f32);
            for x in x0.floor() as usize..(x1.ceil() as usize).min(width) {
                let dx = x1.min(x as f32 + 1.0) - x0.max(x as f32);
                cover[y * width + x] += dx * dy;
            }
        }
    }
    Some(RasterGlyph {
        size: [width as u32, height as u32],
        offset: [left as i32, top as i32],
        alpha: cover
            .iter()
            .map(|&a| (a.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect(),
    })
}

/// A scaled outline (pixels, y up, the pen at the origin) as straight
/// segments `[x0, y0, x1, y1]`, each curve split into [`CURVE_STEPS`].
pub(super) fn segments(outline: &Outline) -> Vec<[f32; 4]> {
    let steps = || (1..=CURVE_STEPS).map(|step| f32::from(step) / f32::from(CURVE_STEPS));
    let mut segments = Vec::new();
    let (mut start, mut pen) = ([0.0; 2], [0.0; 2]);
    for command in outline.path().commands() {
        let ends: Vec<[f32; 2]> = match command {
            Command::MoveTo(p) => {
                close_contour(&mut segments, pen, start);
                (start, pen) = ([p.x, p.y], [p.x, p.y]);
                continue;
            }
            Command::Close => {
                close_contour(&mut segments, pen, start);
                pen = start;
                continue;
            }
            Command::LineTo(p) => vec![[p.x, p.y]],
            Command::QuadTo(c, p) => steps()
                .map(|t| {
                    let u = 1.0 - t;
                    let at = |a: f32, b: f32, d: f32| u * u * a + 2.0 * u * t * b + t * t * d;
                    [at(pen[0], c.x, p.x), at(pen[1], c.y, p.y)]
                })
                .collect(),
            Command::CurveTo(c1, c2, p) => steps()
                .map(|t| {
                    let u = 1.0 - t;
                    let at = |a: f32, b: f32, c: f32, d: f32| {
                        u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
                    };
                    [at(pen[0], c1.x, c2.x, p.x), at(pen[1], c1.y, c2.y, p.y)]
                })
                .collect(),
        };
        for end in ends {
            segments.push([pen[0], pen[1], end[0], end[1]]);
            pen = end;
        }
    }
    close_contour(&mut segments, pen, start);
    segments
}

/// The segment from `pen` back to its contour's `start`, unless it is there.
fn close_contour(segments: &mut Vec<[f32; 4]>, pen: [f32; 2], start: [f32; 2]) {
    if pen != start {
        segments.push([pen[0], pen[1], start[0], start[1]]);
    }
}

/// Where the line `y = at` (`row`) or `x = at` runs inside `segments`, by the
/// nonzero winding rule: `[start, end]` along the line, in order.
fn inside(segments: &[[f32; 4]], at: f32, row: bool) -> Vec<[f32; 2]> {
    let mut crossings: Vec<(f32, i32)> = segments
        .iter()
        .filter_map(|&[x0, y0, x1, y1]| {
            // `a` runs across the line, `b` along it.
            let ([a0, b0], [a1, b1]) = if row {
                ([y0, x0], [y1, x1])
            } else {
                ([x0, y0], [x1, y1])
            };
            let crosses = (a0 <= at && at < a1) || (a1 <= at && at < a0);
            crosses.then(|| {
                let b = b0 + (at - a0) * (b1 - b0) / (a1 - a0);
                (b, if a1 > a0 { 1 } else { -1 })
            })
        })
        .collect();
    crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut stretches = Vec::new();
    let (mut winding, mut start) = (0, 0.0);
    for (b, direction) in crossings {
        if winding == 0 {
            start = b;
        }
        winding += direction;
        if winding == 0 {
            stretches.push([start, b]);
        }
    }
    stretches
}

/// The `(column, row)` pixels an outline's `segments` light in `rows`, row 0
/// the one whose bottom edge lies `baseline` above the outline's y = 0,
/// columns counted from x = 0: each pixel whose centre the outline covers,
/// as an unsmoothed rasterizer draws, and, where a stroke crosses a row's or
/// column's centre line between two pixel centres, the pixel at its middle
/// (dropout control), so strokes thinner than a pixel stay unbroken.
pub(super) fn lit(segments: &[[f32; 4]], rows: Range<i32>, baseline: f32) -> HashSet<(i32, i32)> {
    let mut lit = HashSet::new();
    for row in rows.clone() {
        let centre = baseline - row as f32 - 0.5;
        for [start, end] in inside(segments, centre, true) {
            let (first, last) = ((start - 0.5).ceil() as i32, (end - 0.5).floor() as i32);
            if first <= last {
                lit.extend((first..=last).map(|column| (column, row)));
            } else {
                lit.insert((((start + end) / 2.0).floor() as i32, row));
            }
        }
    }
    let xs = segments.iter().flat_map(|s| [s[0], s[2]]);
    let (left, right) = xs.fold((f32::MAX, f32::MIN), |(l, r), x| (l.min(x), r.max(x)));
    for column in left.floor() as i32..right.ceil() as i32 {
        for [start, end] in inside(segments, column as f32 + 0.5, false) {
            let (first, last) = (
                (baseline - 0.5 - end).ceil() as i32,
                (baseline - 0.5 - start).floor() as i32,
            );
            let row = (baseline - (start + end) / 2.0).floor() as i32;
            if first > last && rows.contains(&row) {
                lit.insert((column, row));
            }
        }
    }
    lit
}
