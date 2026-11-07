//! Dependency waves for pixel-local native destination edits.
//!
//! VERA scheduling only: original 4990E0/497390 pixel semantics stay in the
//! existing shaders. Overlapping conservative clips keep strict source order;
//! disjoint clips commute and may share one immutable destination snapshot.
//!
//! Every wave costs a snapshot and an edit pass (#3605), so dependencies are
//! exact rectangle intersections. Tiles only narrow the candidates: a tile
//! lists up to [`TILE_LIST`] commands, and a busier tile falls back to its
//! conservative latest wave.

use super::DestinationEditCommand;

const TILE: u32 = 32;
const TILE_LIST: u16 = 16;
const END: usize = usize::MAX;

#[derive(Clone, Copy, Debug)]
pub(super) struct TerrainCommand<T = DestinationEditCommand> {
    pub draw: T,
    pub rect: [u32; 4],
    next: usize,
    wave: usize,
}

impl<T> TerrainCommand<T> {
    pub fn new(draw: T, rect: [u32; 4]) -> Self {
        Self {
            draw,
            rect,
            next: END,
            wave: 0,
        }
    }
}

/// One tile's dependency history within a span.
#[derive(Clone, Copy, Debug)]
struct Tile {
    /// One past the latest wave of any command touching this tile; 0 when
    /// untouched.
    wave_end: usize,
    /// Head of this tile's command list in `TerrainBatches::entries`.
    head: usize,
    listed: u16,
    /// More than `TILE_LIST` commands: only `wave_end` is authoritative.
    overflow: bool,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            wave_end: 0,
            head: END,
            listed: 0,
            overflow: false,
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
    /// Actual render passes; read-only reduction uses four per transaction.
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
        self.tile_dependencies = 0;
    }

    /// The caller supplies the *same* cached clipped rect used for snapshot and
    /// edit scissors. Out-of-attachment rectangles retain the old sequential
    /// draw/validation path instead of acquiring unchecked grid addressing.
    pub fn push(&mut self, command: TerrainCommand<T>) -> bool {
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
        // A command follows every earlier command it intersects. Two
        // intersecting rectangles share a tile, where the earlier one is
        // either listed or covered by the tile's conservative `wave_end`, so
        // intersecting rectangles never share a wave.
        let mut wave = 0;
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let tile = self.grid[(ty * self.columns + tx) as usize];
                self.tile_dependencies += 1;
                if tile.overflow {
                    wave = wave.max(tile.wave_end);
                    continue;
                }
                let mut entry = tile.head;
                while entry != END {
                    let (other, next) = self.entries[entry];
                    let other = &self.commands[other];
                    if intersects(other.rect, command.rect) {
                        wave = wave.max(other.wave + 1);
                    }
                    entry = next;
                }
            }
        }
        let index = self.commands.len();
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let cell = (ty * self.columns + tx) as usize;
                let tile = &mut self.grid[cell];
                if tile.wave_end == 0 {
                    self.touched.push(cell);
                }
                tile.wave_end = tile.wave_end.max(wave + 1);
                if tile.overflow {
                    continue;
                }
                if tile.listed == TILE_LIST {
                    tile.overflow = true;
                    continue;
                }
                self.entries.push((index, tile.head));
                tile.head = self.entries.len() - 1;
                tile.listed += 1;
            }
        }
        self.heads.resize(self.heads.len().max(wave + 1), END);
        self.tails.resize(self.tails.len().max(wave + 1), END);
        if self.heads[wave] == END {
            self.heads[wave] = index;
        } else {
            self.commands[self.tails[wave]].next = index;
        }
        self.tails[wave] = index;
        self.commands.push(TerrainCommand { wave, ..command });
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
