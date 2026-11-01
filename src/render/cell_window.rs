//! Cell windows: the grid cells whose isometric anchor can reach a screen
//! rectangle.
//!
//! [`iso_to_screen`] anchors cell `(rx, ry)` at signed level `z` at
//! `x = 30(rx - ry) - 30`, `y = 15(rx + ry) + 15 - 15z` world pixels. A box of
//! anchor positions therefore bounds `rx - ry` by its x range alone and
//! `rx + ry` by its y range and the levels a cell can hold, and each row `ry`
//! keeps one contiguous `rx` range. Rows ascend in `ry` and `rx` ascends within
//! a row: the storage order of the row-major cell grids (`ResolvedTerrainGrid`,
//! `SmudgeGrid`), so a window walk visits a subset of the whole-grid walk in
//! the same order.
//!
//! A consumer grows its view by everything its unchanged per-cell test adds to
//! a cell's anchor (cull margins, fixed offsets, the largest art in its atlas),
//! so the window holds every cell that test admits and the consumer's output
//! does not change. Debug builds compare each consumer's windowed output with
//! its whole-grid output.
//!
//! [`iso_to_screen`]: crate::map::terrain::iso_to_screen

use crate::map::resolved_terrain::{LevelSpan, ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::map::terrain::{HEIGHT_STEP, TILE_HEIGHT, TILE_WIDTH};
use crate::render::overlay_atlas::OverlaySpriteEntry;

/// Inclusive world-pixel bounds a consumer's per-cell test requires of a
/// cell's `iso_to_screen` anchor.
#[derive(Debug, Clone, Copy)]
pub struct AnchorBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// The union of an atlas's sprite rectangles (`offset .. offset + pixel_size`)
/// relative to the point each is drawn from. Zero for an atlas without
/// sprites, which draws nothing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ArtExtent {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl ArtExtent {
    pub fn of<'a>(sprites: impl IntoIterator<Item = &'a OverlaySpriteEntry>) -> Self {
        sprites
            .into_iter()
            .map(|sprite| Self {
                left: sprite.offset_x,
                top: sprite.offset_y,
                right: sprite.offset_x + sprite.pixel_size[0],
                bottom: sprite.offset_y + sprite.pixel_size[1],
            })
            .reduce(|a, b| Self {
                left: a.left.min(b.left),
                top: a.top.min(b.top),
                right: a.right.max(b.right),
                bottom: a.bottom.max(b.bottom),
            })
            .unwrap_or_default()
    }

    /// An upper bound on any one sprite's width and height.
    pub fn max_side(&self) -> f32 {
        (self.right - self.left).max(self.bottom - self.top)
    }
}

/// Beyond this anchor magnitude, f32 rounding in a consumer's own test could
/// outgrow the window's padding; such bounds walk the whole grid.
const PADDED_PIXEL_LIMIT: f64 = (1u32 << 22) as f64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellWindow {
    /// Every cell: the reference walk, and the window for bounds that are not
    /// finite or are too far out.
    Whole,
    /// Inclusive bounds on `rx - ry` and `rx + ry`.
    Bands { diff: (i32, i32), sum: (i32, i32) },
}

impl CellWindow {
    /// The cells whose anchor, at any level in `levels`, can lie in `bounds`,
    /// padded by one cell on every side.
    pub fn covering(bounds: AnchorBounds, levels: LevelSpan) -> Self {
        let [left, top, right, bottom] =
            [bounds.left, bounds.top, bounds.right, bounds.bottom].map(f64::from);
        if ![left, top, right, bottom]
            .iter()
            .all(|value| value.is_finite() && value.abs() <= PADDED_PIXEL_LIMIT)
        {
            return Self::Whole;
        }
        let half_width = f64::from(TILE_WIDTH / 2.0);
        let half_height = f64::from(TILE_HEIGHT / 2.0);
        let step = f64::from(HEIGHT_STEP);
        // x = half_width * (rx - ry) - half_width
        let diff = (
            ((left + half_width) / half_width).floor() as i32 - 1,
            ((right + half_width) / half_width).ceil() as i32 + 1,
        );
        // y = half_height * (rx + ry) + half_height - step * level
        let sum = (
            ((top - half_height + step * f64::from(levels.min)) / half_height).floor() as i32 - 1,
            ((bottom - half_height + step * f64::from(levels.max)) / half_height).ceil() as i32 + 1,
        );
        Self::Bands { diff, sum }
    }

    pub fn contains(&self, rx: u16, ry: u16) -> bool {
        match *self {
            Self::Whole => true,
            Self::Bands { diff, sum } => {
                let (rx, ry) = (i32::from(rx), i32::from(ry));
                (diff.0..=diff.1).contains(&(rx - ry)) && (sum.0..=sum.1).contains(&(rx + ry))
            }
        }
    }

    /// The inclusive range of rows that can hold a window cell.
    pub fn rows(&self) -> Option<(u16, u16)> {
        match *self {
            Self::Whole => Some((0, u16::MAX)),
            Self::Bands { diff, sum } => {
                // ry = ((rx + ry) - (rx - ry)) / 2
                let first = (sum.0 - diff.1).div_euclid(2).max(0);
                let last = (sum.1 - diff.0 + 1).div_euclid(2).min(i32::from(u16::MAX));
                (first <= last).then_some((first as u16, last as u16))
            }
        }
    }

    /// Row `ry`'s inclusive `rx` range in the window.
    pub fn row(&self, ry: u16) -> Option<(u16, u16)> {
        match *self {
            Self::Whole => Some((0, u16::MAX)),
            Self::Bands { diff, sum } => {
                let ry = i32::from(ry);
                let first = (ry + diff.0).max(sum.0 - ry).max(0);
                let last = (ry + diff.1).min(sum.1 - ry).min(i32::from(u16::MAX));
                (first <= last).then_some((first as u16, last as u16))
            }
        }
    }

    /// The window's cells of a `width` x `height` row-major grid, in storage
    /// order.
    pub fn cells(self, width: u16, height: u16) -> impl Iterator<Item = (u16, u16)> {
        let clamp = |range: Option<(u16, u16)>, len: u16| {
            let (first, last) = range?;
            let last = last.min(len.checked_sub(1)?);
            (first <= last).then_some(first..=last)
        };
        clamp(self.rows(), height)
            .into_iter()
            .flatten()
            .flat_map(move |ry| {
                clamp(self.row(ry), width)
                    .into_iter()
                    .flatten()
                    .map(move |rx| (rx, ry))
            })
    }

    /// The window's allocated cells of `terrain`, in storage order. The whole
    /// window is exactly [`ResolvedTerrainGrid::iter`].
    pub fn terrain_cells<'a>(
        self,
        terrain: &'a ResolvedTerrainGrid,
    ) -> impl Iterator<Item = &'a ResolvedTerrainCell> + 'a {
        let whole = (self == Self::Whole).then(|| terrain.iter());
        let banded = (self != Self::Whole).then(|| {
            self.cells(terrain.width(), terrain.height())
                .filter_map(|(rx, ry)| terrain.cell(rx, ry))
        });
        whole
            .into_iter()
            .flatten()
            .chain(banded.into_iter().flatten())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::terrain::iso_to_screen;

    /// The anchor a level-span extreme puts exactly on each bound stays in
    /// the window, the walk keeps row-major order, and a cell one padded band
    /// past a bound leaves it.
    #[test]
    fn window_holds_bound_anchors_at_both_level_extremes_in_storage_order() {
        let levels = LevelSpan { min: -2, max: 6 };
        // (rx, ry, level) on the left, top, right and bottom bounds.
        let (left, top, right, bottom) = ((9, 14, 0), (12, 10, -2), (20, 11, 0), (16, 18, 6));
        let anchor = |(rx, ry, level): (u16, u16, i8)| iso_to_screen(rx, ry, level as u8);
        let bounds = AnchorBounds {
            left: anchor(left).0,
            top: anchor(top).1,
            right: anchor(right).0,
            bottom: anchor(bottom).1,
        };
        let window = CellWindow::covering(bounds, levels);
        for (rx, ry, _) in [left, top, right, bottom] {
            assert!(window.contains(rx, ry), "({rx},{ry})");
        }
        let walked: Vec<_> = window.cells(40, 40).collect();
        let mut sorted = walked.clone();
        sorted.sort_by_key(|&(rx, ry)| (ry, rx));
        assert_eq!(walked, sorted);
        assert!(walked.iter().all(|&(rx, ry)| window.contains(rx, ry)));
        assert_eq!(
            walked.len(),
            (0..40)
                .flat_map(|ry| (0..40).map(move |rx| (rx, ry)))
                .filter(|&(rx, ry)| window.contains(rx, ry))
                .count()
        );
        // Two cells (one past the padding) left of the left bound's diagonal.
        assert!(!window.contains(left.0 - 2, left.1));
        assert_eq!(
            CellWindow::covering(
                AnchorBounds {
                    left: f32::NAN,
                    ..bounds
                },
                levels
            ),
            CellWindow::Whole
        );
    }
}
