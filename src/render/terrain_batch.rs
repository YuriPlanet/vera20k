//! Dependency waves for pixel-local native destination edits.
//!
//! VERA scheduling only: original 4990E0/497390 pixel semantics stay in the
//! existing shaders. Overlapping conservative clips keep strict source order;
//! disjoint clips commute and may share one immutable destination snapshot.
//! A command that can replay an earlier one it overlaps (read-write edits,
//! terrain_edit.wgsl `replay_earlier`) may follow it inside their wave
//! instead, recomputing its pixels from the shared snapshot.
//!
//! Every wave costs a snapshot and an edit pass (#3605), so dependencies are
//! exact rectangle intersections. Tiles only narrow the candidates: a tile
//! lists its latest commands, up to [`TILE_LIST`] (a replay window's worth).
//! A full tile forgets its older half, which every later command there then
//! follows.

use super::DestinationEditCommand;

const TILE: u32 = 32;
/// Commands of one replay window: a wave's members in push order, cut into
/// runs of this many. A command replays only earlier members of its window.
pub(super) const WINDOW: usize = 64;
const TILE_LIST: u16 = WINDOW as u16;
const END: usize = usize::MAX;

#[derive(Clone, Copy, Debug)]
pub(super) struct TerrainCommand<T = DestinationEditCommand> {
    pub draw: T,
    pub rect: [u32; 4],
    /// A bit per earlier member of this command's window, by place in the
    /// window, that it overlaps and replays.
    pub earlier: u64,
    next: usize,
    wave: usize,
    /// Place in its wave, in push order.
    member: usize,
}

impl<T> TerrainCommand<T> {
    pub fn new(draw: T, rect: [u32; 4]) -> Self {
        Self {
            draw,
            rect,
            earlier: 0,
            next: END,
            wave: 0,
            member: 0,
        }
    }
}

/// One tile's dependency history within a span.
#[derive(Clone, Copy, Debug)]
struct Tile {
    /// One past the latest wave of the commands this tile has forgotten; 0
    /// while it lists every command touching it.
    forgotten_end: usize,
    /// Head of this tile's command list in `TerrainBatches::entries`, newest
    /// first; `END` when untouched.
    head: usize,
    listed: u16,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            forgotten_end: 0,
            head: END,
            listed: 0,
        }
    }
}

fn intersects(a: [u32; 4], b: [u32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerrainBatchStats {
    pub pieces: usize,
    /// Independent snapshot/commit transactions, including read-only chunks.
    pub waves: usize,
    /// Actual render passes.
    pub passes: usize,
    pub tile_dependencies: usize,
}

impl TerrainBatchStats {
    pub fn accumulate(&mut self, other: Self) {
        self.pieces += other.pieces;
        self.waves += other.waves;
        self.passes += other.passes;
        self.tile_dependencies += other.tile_dependencies;
    }
}

/// Storage is reused across spans; no entity/asset identity or prior image is
/// retained. Linked wave lists avoid one allocation per fully overlapping piece.
pub(super) struct TerrainBatches<T = DestinationEditCommand> {
    size: [u32; 2],
    columns: u32,
    grid: Vec<Tile>,
    touched: Vec<usize>,
    /// Per-tile command lists: (command index, next entry).
    entries: Vec<(usize, usize)>,
    commands: Vec<TerrainCommand<T>>,
    heads: Vec<usize>,
    tails: Vec<usize>,
    /// Members per wave.
    counts: Vec<usize>,
    tile_dependencies: usize,
}

impl<T> Default for TerrainBatches<T> {
    fn default() -> Self {
        Self {
            size: [0; 2],
            columns: 0,
            grid: Vec::new(),
            touched: Vec::new(),
            entries: Vec::new(),
            commands: Vec::new(),
            heads: Vec::new(),
            tails: Vec::new(),
            counts: Vec::new(),
            tile_dependencies: 0,
        }
    }
}

impl<T> TerrainBatches<T> {
    pub fn begin_span(&mut self, size: [u32; 2]) {
        // Clear only cells touched by the preceding TREE span. Ordinary draw
        // fences must not turn this into a whole-screen clear per object.
        for index in self.touched.drain(..) {
            self.grid[index] = Tile::default();
        }
        self.entries.clear();
        if self.size != size {
            self.size = size;
            self.columns = size[0].div_ceil(TILE);
            let cells = (self.columns as usize)
                .checked_mul(size[1].div_ceil(TILE) as usize)
                .expect("terrain tile extent fits address space");
            self.grid.clear();
            self.grid.resize(cells, Tile::default());
        }
        self.commands.clear();
        self.heads.clear();
        self.tails.clear();
        self.counts.clear();
        self.tile_dependencies = 0;
    }

    /// The caller supplies the *same* cached clipped rect used for snapshot and
    /// edit scissors. Out-of-attachment rectangles retain the old sequential
    /// draw/validation path instead of acquiring unchecked grid addressing.
    pub fn push(&mut self, command: TerrainCommand<T>) -> bool {
        self.push_replaying(command, |_, _| false)
    }

    /// [`Self::push`], where `replays(earlier, command)` says whether the
    /// command can replay an earlier command it overlaps. It then joins that
    /// command's wave when every earlier member it overlaps there shares its
    /// window, and records them in [`TerrainCommand::earlier`].
    pub fn push_replaying(
        &mut self,
        mut command: TerrainCommand<T>,
        replays: impl Fn(&T, &T) -> bool,
    ) -> bool {
        let [x, y, width, height] = command.rect;
        let (Some(right), Some(bottom)) = (x.checked_add(width), y.checked_add(height)) else {
            return false;
        };
        if width == 0 || height == 0 || right > self.size[0] || bottom > self.size[1] {
            return false;
        }
        let x0 = x / TILE;
        let x1 = (right - 1) / TILE;
        let y0 = y / TILE;
        let y1 = (bottom - 1) / TILE;
        // A command follows every earlier command it intersects: in a later
        // wave, or in the same wave when it replays it. Two intersecting
        // rectangles share a tile, where the earlier one is either listed or
        // forgotten, before the tile's `forgotten_end`, so an intersecting
        // rectangle the command does not replay never shares its wave.
        let mut wave = 0;
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let tile = self.grid[(ty * self.columns + tx) as usize];
                self.tile_dependencies += 1;
                wave = wave.max(tile.forgotten_end);
                let mut entry = tile.head;
                while entry != END {
                    let (other, next) = self.entries[entry];
                    let other = &self.commands[other];
                    if intersects(other.rect, command.rect) {
                        let after = !replays(&other.draw, &command.draw);
                        wave = wave.max(other.wave + usize::from(after));
                    }
                    entry = next;
                }
            }
        }
        // Every overlap left in `wave` is listed and replayable. They must
        // share the command's window; otherwise it takes the next wave, which
        // nothing it overlaps has reached.
        let member = self.counts.get(wave).copied().unwrap_or(0);
        command.earlier = 0;
        'tiles: for ty in y0..=y1 {
            for tx in x0..=x1 {
                let tile = self.grid[(ty * self.columns + tx) as usize];
                let mut entry = tile.head;
                while entry != END {
                    let (other, next) = self.entries[entry];
                    let other = &self.commands[other];
                    if other.wave == wave && intersects(other.rect, command.rect) {
                        if other.member / WINDOW != member / WINDOW {
                            wave += 1;
                            command.earlier = 0;
                            break 'tiles;
                        }
                        command.earlier |= 1 << (other.member % WINDOW);
                    }
                    entry = next;
                }
            }
        }
        let member = self.counts.get(wave).copied().unwrap_or(0);
        let index = self.commands.len();
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let cell = (ty * self.columns + tx) as usize;
                let tile = &mut self.grid[cell];
                if tile.head == END {
                    self.touched.push(cell);
                }
                if tile.listed == TILE_LIST {
                    // Forget the older half; later commands follow all of it.
                    let mut entry = tile.head;
                    for _ in 1..TILE_LIST / 2 {
                        entry = self.entries[entry].1;
                    }
                    let mut forgotten = std::mem::replace(&mut self.entries[entry].1, END);
                    while forgotten != END {
                        let (other, next) = self.entries[forgotten];
                        tile.forgotten_end = tile.forgotten_end.max(self.commands[other].wave + 1);
                        forgotten = next;
                    }
                    tile.listed = TILE_LIST / 2;
                }
                self.entries.push((index, tile.head));
                tile.head = self.entries.len() - 1;
                tile.listed += 1;
            }
        }
        self.heads.resize(self.heads.len().max(wave + 1), END);
        self.tails.resize(self.tails.len().max(wave + 1), END);
        self.counts.resize(self.counts.len().max(wave + 1), 0);
        if self.heads[wave] == END {
            self.heads[wave] = index;
        } else {
            self.commands[self.tails[wave]].next = index;
        }
        self.tails[wave] = index;
        self.counts[wave] += 1;
        self.commands.push(TerrainCommand {
            wave,
            member,
            ..command
        });
        true
    }

    pub fn stats(&self) -> TerrainBatchStats {
        TerrainBatchStats {
            pieces: self.commands.len(),
            waves: self.heads.len(),
            passes: self.heads.len() * 2,
            tile_dependencies: self.tile_dependencies,
        }
    }

    pub fn waves(&self) -> impl ExactSizeIterator<Item = TerrainWave<'_, T>> {
        self.heads.iter().map(|&next| TerrainWave {
            commands: &self.commands,
            next,
        })
    }
}

#[derive(Clone)]
pub(super) struct TerrainWave<'a, T = DestinationEditCommand> {
    commands: &'a [TerrainCommand<T>],
    next: usize,
}

impl<'a, T> Iterator for TerrainWave<'a, T> {
    type Item = &'a TerrainCommand<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next == END {
            return None;
        }
        let command = &self.commands[self.next];
        self.next = command.next;
        Some(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(index: u32, rect: [u32; 4]) -> TerrainCommand {
        TerrainCommand::new(
            DestinationEditCommand {
                index,
                piece: if index.is_multiple_of(2) {
                    super::super::TerrainPiece::Body
                } else {
                    super::super::TerrainPiece::Shadow
                },
                render_z: crate::render::tactical_draw_plan::RenderZPolicy::ReadWrite,
                atlas_slot: 0,
            },
            rect,
        )
    }

    fn overlaps(a: [u32; 4], b: [u32; 4]) -> bool {
        a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
    }

    /// Rectangles that only share a 32-px tile commute: one wave, so one
    /// snapshot and one edit pass instead of two of each.
    #[test]
    fn terrain_batches_keep_disjoint_rectangles_in_one_tile_together() {
        let mut batches = TerrainBatches::default();
        batches.begin_span([64, 64]);
        for (i, rect) in [[0, 0, 8, 8], [16, 16, 8, 8], [4, 4, 16, 16]]
            .into_iter()
            .enumerate()
        {
            assert!(batches.push(command(i as u32, rect)));
        }
        assert_eq!(
            batches
                .waves()
                .map(|w| w.map(|c| c.draw.index).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![vec![0, 1], vec![2]]
        );
    }

    /// A command that replays the earlier commands it overlaps shares their
    /// wave and records them, but only inside its window; one that cannot
    /// replay them still waits for the next wave.
    #[test]
    fn terrain_batches_let_replaying_overlaps_share_a_wave_within_a_window() {
        let mut batches = TerrainBatches::default();
        batches.begin_span([256, 64]);
        let rects = [
            [0, 0, 16, 16],
            [8, 8, 16, 16],
            [100, 0, 8, 8],
            [4, 4, 8, 8],
            [12, 12, 4, 4],
        ];
        for (i, rect) in rects.into_iter().enumerate() {
            assert!(batches.push_replaying(command(i as u32, rect), |_, later| later.index != 3));
        }
        let waves = |batches: &TerrainBatches| {
            batches
                .waves()
                .map(|w| w.map(|c| (c.draw.index, c.earlier)).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            waves(&batches),
            vec![vec![(0, 0), (1, 0b1), (2, 0), (4, 0b11)], vec![(3, 0)]]
        );
        batches.begin_span([256, 64]);
        for i in 0..WINDOW as u32 {
            assert!(batches.push_replaying(command(i, [0, 0, 8, 8]), |_, _| true));
        }
        assert_eq!(waves(&batches)[0][WINDOW - 1].1, u64::MAX >> 1);
        // A wave of a full window sends a later overlap to the next one.
        batches.begin_span([256, 64]);
        for i in 0..WINDOW as u32 {
            let rect = [i % 32 * 8, i / 32 * 8, 8, 8];
            assert!(batches.push_replaying(command(i, rect), |_, _| true));
        }
        let last = command(WINDOW as u32, [4, 4, 8, 8]);
        assert!(batches.push_replaying(last, |_, _| true));
        let waves = waves(&batches);
        assert_eq!(waves[0].len(), WINDOW);
        assert_eq!(waves[1], vec![(WINDOW as u32, 0)]);
    }

    #[test]
    fn terrain_batches_preserve_overlap_order_and_half_open_tile_boundaries() {
        let mut batches = TerrainBatches::default();
        batches.begin_span([128, 96]);
        let rects = [
            [0, 0, 32, 32],
            [32, 0, 32, 32],
            [31, 0, 2, 32],
            [96, 64, 32, 32],
        ];
        for (i, &rect) in rects.iter().enumerate() {
            assert!(batches.push(command(i as u32, rect)));
        }
        assert_eq!(
            batches
                .waves()
                .map(|w| w.map(|c| c.draw.index).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![vec![0, 1, 3], vec![2]]
        );
        assert!(!batches.push(command(4, [128, 0, 1, 1])));
        assert!(!batches.push(command(4, [0, 0, 0, 1])));
        assert!(!batches.push(command(4, [u32::MAX, 0, 2, 1])));
        assert_eq!(batches.stats().pieces, 4);
        // A hard fence clears the dependency history. Resize clears old tile
        // coordinates before reallocating, including a smaller attachment.
        batches.begin_span([16, 16]);
        assert!(batches.push(command(0, [0, 0, 16, 16])));
        assert_eq!(batches.stats().waves, 1);
        batches.begin_span([128, 96]);
        assert!(batches.push(command(0, rects[2])));
        assert_eq!(batches.stats().waves, 1);
    }

    #[test]
    fn terrain_batches_check_every_generated_dependency_and_bound_20k_overlap_work() {
        let mut batches = TerrainBatches::default();
        batches.begin_span([800, 600]);
        let mut seed = 0x742134u32;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seed
        };
        let mut rects = Vec::new();
        for i in 0..1024 {
            let rect = [
                next() % 700,
                next() % 500,
                1 + next() % 100,
                1 + next() % 100,
            ];
            rects.push(rect);
            assert!(batches.push(command(i, rect)));
        }
        let mut assigned = vec![usize::MAX; rects.len()];
        for (wave, commands) in batches.waves().enumerate() {
            let mut previous = None;
            for command in commands {
                let index = command.draw.index as usize;
                assert_eq!(assigned[index], usize::MAX, "each piece once");
                assert!(
                    previous.is_none_or(|old| old < index),
                    "stable order within wave"
                );
                previous = Some(index);
                assigned[index] = wave;
            }
        }
        assert!(assigned.iter().all(|&w| w != usize::MAX));
        for (i, &a) in rects.iter().enumerate() {
            for (j, &b) in rects.iter().enumerate().skip(i + 1) {
                if overlaps(a, b) {
                    assert!(assigned[i] < assigned[j]);
                }
                if assigned[i] == assigned[j] {
                    assert!(!overlaps(a, b));
                }
            }
        }
        batches.begin_span([800, 600]);
        for i in 0..20_000 {
            assert!(batches.push(command(i, [350, 250, 33, 78])));
        }
        assert_eq!(
            batches.stats(),
            TerrainBatchStats {
                pieces: 20_000,
                waves: 20_000,
                passes: 40_000,
                tile_dependencies: 160_000
            }
        );
        assert_eq!(batches.grid.len(), 475);
        assert_eq!(batches.touched.len(), 8);
        assert!(
            batches
                .waves()
                .enumerate()
                .all(|(i, mut w)| w.next().unwrap().draw.index == i as u32 && w.next().is_none())
        );
    }
}
