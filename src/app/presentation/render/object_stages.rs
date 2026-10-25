//! Overlap-ordered stages for one retained Display layer replay.
//!
//! VERA scheduling only; no native counterpart. gamemd blits each Display
//! parent in order into one software surface, where switching blitters is
//! free. Here an ordinary GPU draw, a packed composite and a destination edit
//! each need their own render passes, so replaying them strictly in parent
//! order closes and reopens passes around every Anim body and shadow.
//!
//! Draws whose pixel footprints are disjoint commute exactly: each pipeline
//! reads and writes colour and Z only inside its footprint (the packed read
//! halo included). So every draw goes to the earliest stage after every
//! earlier draw it overlaps. Same-kind draws that already compose in sequence
//! inside one transaction (ordinary draws in one render pass, read-only edits
//! in one reduction interval) may share a stage even when they overlap; their
//! order inside the stage stays parent order. Stages replay in index order.
//! Footprints are tested on a tile grid: a false overlap only costs a stage.

use crate::render::tactical_draw_plan::RenderZPolicy;

const TILE: u32 = 16;

/// Which transaction family replays a stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StageKind {
    /// One ordinary render pass; draws compose in submission order.
    Ordinary,
    /// One packed wave: equal selector and Z policy, pairwise disjoint.
    Packed(u32, RenderZPolicy),
    /// A packed composite with displaced reads, replayed alone.
    PackedAlone,
    /// One read-only reduction interval; overlapping edits reduce in order.
    EditReadOnly,
    /// One read-write edit wave, pairwise disjoint.
    EditReadWrite,
}

impl StageKind {
    /// Whether `next` may share a stage with an earlier draw it overlaps.
    fn composes_in_order(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Ordinary, Self::Ordinary) | (Self::EditReadOnly, Self::EditReadOnly)
        )
    }

    fn joins(self, next: Self) -> bool {
        self == next && self != Self::PackedAlone
    }
}

/// Stages of one replay segment. Barrier commands end a segment: the caller
/// replays [`Self::take`] and starts again from an empty plan.
pub(super) struct ObjectStages<T> {
    columns: u32,
    rows: u32,
    /// Per tile: one past the highest stage touching it; 0 when untouched.
    grid: Vec<u32>,
    touched: Vec<usize>,
    stages: Vec<(StageKind, Vec<T>)>,
}

impl<T> ObjectStages<T> {
    pub(super) fn new(size: [u32; 2]) -> Self {
        let columns = size[0].div_ceil(TILE).max(1);
        let rows = size[1].div_ceil(TILE).max(1);
        Self {
            columns,
            rows,
            grid: vec![0; (columns * rows) as usize],
            touched: Vec::new(),
            stages: Vec::new(),
        }
    }

    /// Place one draw whose colour/Z reads and writes stay inside `rect`
    /// (`[x, y, width, height]` in attachment pixels, non-empty).
    pub(super) fn push(&mut self, kind: StageKind, rect: [u32; 4], item: T) {
        let [x0, y0, x1, y1] = self.tiles(rect);
        let mut first = 0;
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let above = self.grid[(ty * self.columns + tx) as usize] as usize;
                if above != 0 {
                    let stage = above - 1;
                    let after = if self.stages[stage].0.composes_in_order(kind) {
                        stage
                    } else {
                        stage + 1
                    };
                    first = first.max(after);
                }
            }
        }
        let stage = match (first..self.stages.len()).find(|&s| self.stages[s].0.joins(kind)) {
            Some(stage) => stage,
            None => {
                self.stages.push((kind, Vec::new()));
                self.stages.len() - 1
            }
        };
        self.stages[stage].1.push(item);
        let mark = stage as u32 + 1;
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let cell = (ty * self.columns + tx) as usize;
                if self.grid[cell] == 0 {
                    self.touched.push(cell);
                }
                self.grid[cell] = self.grid[cell].max(mark);
            }
        }
    }

    /// The planned stages in replay order; the plan is empty afterwards.
    pub(super) fn take(&mut self) -> Vec<(StageKind, Vec<T>)> {
        for cell in self.touched.drain(..) {
            self.grid[cell] = 0;
        }
        std::mem::take(&mut self.stages)
    }

    fn tiles(&self, [x, y, width, height]: [u32; 4]) -> [u32; 4] {
        #[cfg(test)]
        if SERIAL.get() {
            return [0, 0, self.columns - 1, self.rows - 1];
        }
        let last = |start: u32, extent: u32| start.saturating_add(extent.max(1) - 1);
        [
            (x / TILE).min(self.columns - 1),
            (y / TILE).min(self.rows - 1),
            (last(x, width) / TILE).min(self.columns - 1),
            (last(y, height) / TILE).min(self.rows - 1),
        ]
    }
}

#[cfg(test)]
thread_local! {
    /// Test hook: every footprint covers the whole surface, so the replay
    /// keeps strict parent order. The reference for pixel-equality tests.
    pub(super) static SERIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage_items(plan: &mut ObjectStages<u32>) -> Vec<(StageKind, Vec<u32>)> {
        plan.take()
    }

    #[test]
    fn disjoint_anims_share_stages_and_overlaps_keep_parent_order() {
        let packed = StageKind::Packed(2, RenderZPolicy::ReadOnly);
        let mut plan = ObjectStages::new([256, 64]);
        // Building, its translucent Anim on top, its shadow; then a second
        // building group far to the right; then a sprite clear of both groups
        // and one over the first Anim.
        plan.push(StageKind::Ordinary, [0, 0, 40, 40], 0);
        plan.push(packed, [10, 10, 20, 20], 1);
        plan.push(StageKind::EditReadOnly, [12, 12, 20, 20], 2);
        plan.push(StageKind::Ordinary, [128, 0, 40, 40], 3);
        plan.push(packed, [138, 10, 20, 20], 4);
        plan.push(StageKind::EditReadOnly, [140, 12, 20, 20], 5);
        plan.push(StageKind::Ordinary, [64, 40, 16, 16], 6);
        plan.push(StageKind::Ordinary, [15, 15, 4, 4], 7);
        assert_eq!(
            stage_items(&mut plan),
            [
                (StageKind::Ordinary, vec![0, 3, 6]),
                (packed, vec![1, 4]),
                (StageKind::EditReadOnly, vec![2, 5]),
                (StageKind::Ordinary, vec![7]),
            ]
        );
    }

    #[test]
    fn exclusive_waves_never_take_an_overlapping_member() {
        let mut plan = ObjectStages::new([64, 64]);
        let packed = StageKind::Packed(2, RenderZPolicy::None);
        plan.push(packed, [0, 0, 20, 20], 0);
        plan.push(packed, [10, 10, 20, 20], 1);
        plan.push(StageKind::EditReadWrite, [0, 0, 8, 8], 2);
        plan.push(StageKind::EditReadWrite, [4, 4, 8, 8], 3);
        plan.push(StageKind::PackedAlone, [48, 48, 4, 4], 4);
        plan.push(StageKind::PackedAlone, [56, 56, 4, 4], 5);
        assert_eq!(
            stage_items(&mut plan),
            [
                (packed, vec![0]),
                (packed, vec![1]),
                (StageKind::EditReadWrite, vec![2]),
                (StageKind::EditReadWrite, vec![3]),
                (StageKind::PackedAlone, vec![4]),
                (StageKind::PackedAlone, vec![5]),
            ]
        );
        // The grid is cleared with the plan.
        plan.push(StageKind::Ordinary, [0, 0, 64, 64], 6);
        assert_eq!(stage_items(&mut plan), [(StageKind::Ordinary, vec![6])]);
    }
}
