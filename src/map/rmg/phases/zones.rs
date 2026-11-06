//! Passability-zone recomputation for the starts phase.
//!
//! Before selecting start regions the original reclassifies every cell into a
//! passability class, scanline-flood-fills base zones over the linear cell
//! array, collects zone adjacency edges, and derives per-movement-kind zone
//! tables. The starts phase then keeps only cells whose derived zone matches
//! the derived zone of the largest base component. This module reproduces
//! that pipeline over the generator's grid.
//!
//! Cells are classified through the shared `RecalcZoneType @ 0x00483C80`
//! owner from their terrain, the low-bridge deck overlays the connector pass
//! stamps and the bridge-repair huts it places; nothing else is on the map
//! when the starts phase asks. Native comparisons:
//! `tools/rmg_oracle/start_zones.{py,json}`.

use crate::map::resolved_terrain::{
    cliff_back_normal_reclass_applies, is_behind_cliff, overlay_reduced_zone_type,
    recalc_zone_type_for_wheel_block,
};
use crate::map::rmg::grid::{CellRef, RmgGrid};
use crate::rules::overlay_types::{
    OverlayTypeFlags, OverlayTypeRegistry, uses_early_recalc_land_branch,
};

use super::shore::TileBlocks;

/// Number of land types the class tables index.
pub const LAND_TYPES: usize = 16;

/// Passability classes the zone tables index.
const CLASS_CLEAR: u8 = 0;
const CLASS_BEACH: u8 = 3;
const CLASS_WATER: u8 = 4;
const CLASS_WHEEL_BLOCKED: u8 = 6;
const CLASS_OUTSIDE: u8 = 7;

/// The land type the CliffBack reclass writes.
pub const LAND_ROCK: u8 = 3;

/// What `RecalcAttributes @ 0x0047D2B0` and `RecalcZoneType @ 0x00483C80`
/// read from one overlay type: its `Land=` (`+0x298`), whether that land
/// replaces the tile's (the early branch: Wall, Railroad or
/// `NoUseTileLandType=`), and the class its Crushable, Wall, Wheel-0 Land,
/// IsARock and IsRubble keys give.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayZoneFacts {
    pub land: u8,
    pub early_land: bool,
    pub zone_type: Option<u8>,
}

impl OverlayZoneFacts {
    pub fn from_flags(flags: &OverlayTypeFlags) -> Self {
        Self {
            land: flags.land.as_index(),
            early_land: uses_early_recalc_land_branch(flags),
            zone_type: overlay_reduced_zone_type(Some(flags)),
        }
    }

    /// The facts for every overlay id the rules list, in id order.
    pub fn from_registry(registry: &OverlayTypeRegistry) -> Vec<Self> {
        (0..=u8::MAX)
            .map_while(|id| registry.flags(id).map(Self::from_flags))
            .collect()
    }
}

/// The movement kind the starts phase filters by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneKind {
    /// Map types 0..=2.
    Amphibious,
    /// Map types 3..=4.
    Normal,
}

impl ZoneKind {
    pub fn for_map_type(map_type: i32) -> Self {
        if matches!(map_type, 3 | 4) {
            Self::Normal
        } else {
            Self::Amphibious
        }
    }

    /// The passability-matrix row: which classes the kind can traverse.
    fn passable(self, class: u8) -> bool {
        match self {
            Self::Amphibious => matches!(class, CLASS_CLEAR | CLASS_BEACH | CLASS_WATER),
            Self::Normal => class == CLASS_CLEAR,
        }
    }
}

/// Geometry inputs to the classifier and frame tests.
#[derive(Debug, Clone, Copy)]
pub struct ZoneParams<'a> {
    /// The playfield/view frame base width (the map's padded width).
    pub map_w: i32,
    /// Local (playable) rect `{x, y, w, h}`; generated maps use
    /// `(2, 5, gen_w, gen_h)`.
    pub local_rect: [i32; 4],
    /// Per-land-type "Wheel speed <= 1%" flags from the rules terrain data.
    pub wheel_impassable: [bool; LAND_TYPES],
    /// Low byte of `[General] CliffBackImpassability=`.
    pub cliff_back_impassability: u8,
    /// [`OverlayZoneFacts`] by overlay id.
    pub overlays: &'a [OverlayZoneFacts],
}

/// The diamond-frame containment test shared by the playfield and
/// view-region checks: anti-diagonal band plus off-axis bounds against a
/// rect, with an isometric height correction (a sloped cell in the upper
/// half counts one level higher).
pub fn diamond_frame_contains(
    map_w: i32,
    rect: [i32; 4],
    x: i32,
    y: i32,
    level: u8,
    sloped: bool,
) -> bool {
    let s = x + y;
    let mut lvl = i32::from(level as i8);
    if sloped && s < map_w + 4 + rect[1] * 2 + lvl {
        lvl += 1;
    }
    s > map_w + rect[1] * 2 + lvl
        && s <= map_w + 2 + (rect[3] + rect[1]) * 2 + lvl
        && (x - y) < (rect[2] + rect[0]) * 2 - map_w
        && (y - x) < map_w - rect[0] * 2
}

/// One zone record: class, level, base zone id (0 = unzoned).
#[derive(Debug, Clone, Copy)]
struct Record {
    class: u8,
    level: u8,
    zone: u16,
}

/// The computed zone field the starts phase consumes.
#[derive(Debug)]
pub struct ZoneField {
    stride: usize,
    records: Vec<Record>,
    /// Derived zone per base zone id; entry 0 is the 0xFFFF sentinel,
    /// impassable zones hold 1, merged passable groups 2..
    derived: Vec<u16>,
    /// The base zone `RebuildZoneConnectivity @ 0x0056C510` returns: the
    /// strictly largest component, the first (lowest id) on ties.
    largest: u16,
    /// Derived zone of the largest base component (first wins ties).
    pub reference: u16,
}

impl ZoneField {
    fn index(&self, x: i32, y: i32) -> usize {
        // The original's cell->record index clamps into the array instead of
        // rejecting out-of-range coordinates.
        let raw = y as i64 * self.stride as i64 + x as i64;
        raw.clamp(0, self.records.len() as i64 - 1) as usize
    }

    /// Derived zone id for the active kind at a cell.
    pub fn zone(&self, x: i32, y: i32) -> u16 {
        self.derived[usize::from(self.records[self.index(x, y)].zone)]
    }

    /// Passability class at a cell.
    pub fn class(&self, x: i32, y: i32) -> u8 {
        self.records[self.index(x, y)].class
    }

    /// Base zone record at a cell, the derived table and the largest base
    /// zone, for native comparisons.
    #[cfg(test)]
    pub(crate) fn native_parts(&self, x: i32, y: i32) -> (u16, &[u16], u16) {
        (
            self.records[self.index(x, y)].zone,
            &self.derived,
            self.largest,
        )
    }

    /// The start-region cell filter: plain clear ground in the reference
    /// derived zone.
    pub fn is_reference_ground(&self, x: i32, y: i32) -> bool {
        self.class(x, y) == CLASS_CLEAR && self.zone(x, y) == self.reference
    }

    /// A declared field for native comparisons that supply the zones: `typed`
    /// cells take a non-clear class, `ground` cells the reference zone and
    /// every other cell a second zone.
    #[cfg(test)]
    pub(crate) fn declared(
        stride: usize,
        typed: impl Fn(i32, i32) -> bool,
        ground: impl Fn(i32, i32) -> bool,
    ) -> Self {
        let records = (0..stride * stride)
            .map(|index| {
                let (x, y) = ((index % stride) as i32, (index / stride) as i32);
                Record {
                    class: if typed(x, y) {
                        CLASS_BEACH
                    } else {
                        CLASS_CLEAR
                    },
                    level: 0,
                    zone: if ground(x, y) { 1 } else { 2 },
                }
            })
            .collect();
        Self {
            stride,
            records,
            derived: vec![0xFFFF, 2, 3],
            largest: 1,
            reference: 2,
        }
    }
}

/// The land type `CellClass::RecalcAttributes @ 0x0047D2B0` leaves on a
/// generated cell, which `RecalcZoneType @ 0x00483C80` and the tech-building
/// foundation gate read: the sub-tile's TMP terrain byte through the fixed
/// table, then the CliffBack reclass. Under `CliffBackImpassability=2`
/// (retail) a Clear, Water, Beach or Ice cell behind a cliff becomes Rock
/// (its final normal site; the sparse-subtile site is Clear-only, and
/// missing block data reads Clear here). This is the tile path: the zone
/// classifier gives an early-branch overlay (a low-bridge deck) its own land,
/// and the tech and tree gates refuse overlay cells before reading land. This
/// reads the levels current when the zone and tech stages ask, not a stored
/// Recalc result.
pub fn recalc_land(
    grid: &RmgGrid,
    blocks: &dyn TileBlocks,
    cliff_back_impassability: u8,
    x: i32,
    y: i32,
) -> u8 {
    let Some(cell) = grid.get(x, y) else {
        return 0;
    };
    // Sub-tile terrain byte; unassigned/clear tiles and missing block data
    // read as terrain 0 (clear), matching the retail clear tiles.
    let terrain = blocks
        .block(cell.tile)
        .and_then(|block| {
            block
                .subtiles
                .get(cell.sub_tile as usize)
                .copied()
                .flatten()
        })
        .map_or(0, |sub| sub.terrain);
    let land = crate::rules::terrain_rules::tmp_terrain_to_land_type(terrain).as_index();
    if cliff_back_impassability == 2
        && cliff_back_normal_reclass_applies(land)
        && behind_cliff(grid, x, y)
    {
        LAND_ROCK
    } else {
        land
    }
}

/// The CliffBack neighbour test on the generated grid; a missing neighbour
/// reads the shared Dummy's level.
fn behind_cliff(grid: &RmgGrid, x: i32, y: i32) -> bool {
    let level = grid.get(x, y).map_or(0, |cell| cell.level as i8);
    is_behind_cliff(x as i16, y as i16, level, |nx, ny| {
        let (nx, ny) = (i32::from(nx), i32::from(ny));
        grid.get(nx, ny)
            .unwrap_or_else(|| grid.ref_cell(CellRef::Border))
            .level as i8
    })
}

/// One cell's class: `RecalcZoneType @ 0x00483C80` over the land type
/// `RecalcAttributes @ 0x0047D2B0` leaves.
///
/// An overlay on the early branch (every low-bridge deck: `Land=Road`,
/// `NoUseTileLandType=yes`) takes its own land, and the branch's CliffBack
/// test (`0x0047D30E..`) turns any land behind a cliff into Rock; other cells
/// read the tile's land through [`recalc_land`]. The only occupier at starts
/// time is the bridge-repair hut (`PlaceBridgeRepairHut @ 0x005904B0`), a
/// building: the occupier walk's class-6 write needs a `FirestormWall=`
/// type (`+0x16C0`, `0x00483D8E..0x00483DAC`) or is overwritten by the class-0
/// exit at `0x00483DD4`, so the hut leaves its cell class 0.
fn classify(grid: &RmgGrid, blocks: &dyn TileBlocks, params: &ZoneParams, x: i32, y: i32) -> u8 {
    let cell = match grid.get(x, y) {
        Some(cell) => *cell,
        None => return CLASS_OUTSIDE,
    };
    let outside = !diamond_frame_contains(
        params.map_w,
        params.local_rect,
        x,
        y,
        cell.level,
        cell.slope != 0,
    );
    let overlay = usize::try_from(cell.overlay)
        .ok()
        .and_then(|id| params.overlays.get(id));
    let land = match overlay {
        Some(facts) if facts.early_land => {
            if params.cliff_back_impassability == 2 && behind_cliff(grid, x, y) {
                LAND_ROCK
            } else {
                facts.land
            }
        }
        _ => recalc_land(grid, blocks, params.cliff_back_impassability, x, y),
    };
    let wheel_blocked = params
        .wheel_impassable
        .get(usize::from(land))
        .copied()
        .unwrap_or(false);
    recalc_zone_type_for_wheel_block(
        outside,
        overlay.and_then(|facts| facts.zone_type),
        land,
        wheel_blocked,
        None,
    )
}

/// Compute the zone field for a generated map.
pub fn compute(
    grid: &RmgGrid,
    blocks: &dyn TileBlocks,
    params: &ZoneParams,
    kind: ZoneKind,
) -> ZoneField {
    let stride = grid.width();
    let mut records = vec![
        // Records allocate to class 7 / level 0; slots without a cell keep it.
        Record {
            class: CLASS_OUTSIDE,
            level: 0,
            zone: 0,
        };
        stride * stride
    ];
    for y in 0..stride as i32 {
        for x in 0..stride as i32 {
            if let Some(cell) = grid.get(x, y) {
                let record = &mut records[y as usize * stride + x as usize];
                record.class = classify(grid, blocks, params, x, y);
                record.level = cell.level;
            }
        }
    }

    // Base zones: linear-order scanline fill, ids from 1.
    let mut fill = Fill {
        records,
        stride,
        edges: Vec::new(),
        edge_cache: 0,
    };
    let mut sizes: Vec<i32> = vec![0];
    let mut classes: Vec<u8> = vec![CLASS_OUTSIDE];
    let mut largest = (i32::MIN, 0u16);
    let mut next_zone = 1u16;
    let mut index = 0usize;
    while index < fill.records.len() {
        let record = fill.records[index];
        if record.class == CLASS_OUTSIDE || record.zone != 0 {
            index += 1;
            continue;
        }
        fill.edge_cache = 0;
        let (size, advance) = fill.run(index, next_zone);
        if size > largest.0 {
            largest = (size, next_zone);
        }
        sizes.push(size);
        classes.push(record.class);
        next_zone += 1;
        index += advance.max(1) as usize;
    }

    // Derived table: passable zones grouped over the adjacency edges, ids
    // from 2 in ascending seed order; impassable zones keep 1; entry 0 is
    // the sentinel.
    let zone_count = usize::from(next_zone);
    let mut neighbors: Vec<Vec<u16>> = vec![Vec::new(); zone_count];
    for &(a, b) in &fill.edges {
        neighbors[usize::from(a)].push(b);
        neighbors[usize::from(b)].push(a);
    }
    let mut derived: Vec<u16> = (0..zone_count)
        .map(|zone| u16::from(!kind.passable(classes[zone])))
        .collect();
    let mut next_group = 2u16;
    for zone in 0..zone_count {
        if derived[zone] != 0 {
            continue;
        }
        derived[zone] = next_group;
        let mut stack = vec![zone as u16];
        while let Some(current) = stack.pop() {
            // The original walks each neighbor list back-to-front; only the
            // grouping matters, ids are compared for equality alone.
            for &neighbor in neighbors[usize::from(current)].iter().rev() {
                if kind.passable(classes[usize::from(neighbor)])
                    && derived[usize::from(neighbor)] == 0
                {
                    derived[usize::from(neighbor)] = next_group;
                    stack.push(neighbor);
                }
            }
        }
        next_group += 1;
    }
    derived[0] = 0xFFFF;

    ZoneField {
        stride,
        records: fill.records,
        derived: derived.clone(),
        largest: largest.1,
        reference: derived[usize::from(largest.1)],
    }
}

/// The scanline fill, ported instruction-faithfully: runs join on exact
/// class equality with an asymmetric level rule (leftward and vertical steps
/// tolerate one level, rightward steps three), and every contact with an
/// already-zoned run registers an adjacency edge when levels are within one
/// (or unconditionally for class-6 seeds).
struct Fill {
    records: Vec<Record>,
    stride: usize,
    /// Undirected edges as (older, newer) zone pairs, deduplicated.
    edges: Vec<(u16, u16)>,
    /// Last neighbor zone considered for registration — a redundant-work
    /// skip in the original; kept because it is cheap and exact.
    edge_cache: u16,
}

impl Fill {
    fn level(&self, index: usize) -> i32 {
        i32::from(self.records[index].level)
    }

    /// Register an edge between `neighbor_zone` and the active `zone`.
    fn edge(&mut self, neighbor_index: usize, prev_level: i32, zone: u16, seed_class6: bool) {
        let neighbor_zone = self.records[neighbor_index].zone;
        if neighbor_zone == 0 {
            return;
        }
        let level_ok = (self.level(neighbor_index) - prev_level).abs() < 2;
        if (level_ok || seed_class6) && neighbor_zone != self.edge_cache && neighbor_zone != zone {
            let pair = (neighbor_zone, zone);
            if !self.edges.contains(&pair) {
                self.edges.push(pair);
            }
            self.edge_cache = neighbor_zone;
        }
    }

    /// Fill starting at `seed`; returns (cells filled, seed-run advance).
    fn run(&mut self, seed: usize, zone: u16) -> (i32, i32) {
        let class = self.records[seed].class;
        let class6 = class == CLASS_WHEEL_BLOCKED;
        let stride = self.stride;

        // Leftward extension (seed inclusive): claim while class matches and
        // each cell is within one level of the cell to its right.
        let mut prev_level = self.level(seed);
        let mut cursor = seed;
        loop {
            if (self.level(cursor) - prev_level).abs() >= 2 {
                break;
            }
            self.records[cursor].zone = zone;
            prev_level = self.level(cursor);
            if cursor == 0 {
                // Unreachable with the class-7 guard ring; stop rather than
                // wrap.
                debug_assert!(false, "run reached the array origin");
                break;
            }
            cursor -= 1;
            if self.records[cursor].class != class {
                break;
            }
        }
        let left_bound = cursor;
        self.edge(left_bound, prev_level, zone, class6);

        // Rightward extension: three levels of tolerance. The level chain
        // deliberately carries over from the leftmost claimed cell.
        let mut cursor = seed;
        while self.records[cursor].class == class && (self.level(cursor) - prev_level).abs() < 4 {
            self.records[cursor].zone = zone;
            prev_level = self.level(cursor);
            cursor += 1;
            if cursor >= self.records.len() {
                debug_assert!(false, "run reached the array end");
                break;
            }
        }
        let right_bound = cursor.min(self.records.len() - 1);
        self.edge(right_bound, prev_level, zone, class6);

        let mut count = (right_bound - left_bound) as i32 - 1;
        let advance = (right_bound - seed) as i32 - 1;

        // Vertical scans over the run plus its unclaimed flanks: the row
        // above from left to right, then the row below. The level reference
        // for each probed cell is the claimed cell diagonally toward the
        // run's interior (the two right-end cells reference the rightmost
        // claimed cell instead).
        for row_below in [false, true] {
            let (start, end) = if row_below {
                (left_bound + stride, right_bound + stride)
            } else {
                (
                    left_bound.wrapping_sub(stride),
                    right_bound.wrapping_sub(stride),
                )
            };
            if start >= self.records.len() || end >= self.records.len() {
                // Unreachable with the guard rings (border rows are class 7
                // and never host runs); skip rather than index out.
                debug_assert!(false, "vertical scan left the array");
                continue;
            }
            let mut probe = start;
            while probe <= end {
                let reference = if probe < end - 1 {
                    if row_below {
                        probe - stride + 1
                    } else {
                        probe + stride + 1
                    }
                } else if probe == end - 1 {
                    if row_below {
                        probe - stride
                    } else {
                        probe + stride
                    }
                } else if row_below {
                    probe - stride - 1
                } else {
                    probe + stride - 1
                };
                let probe_zone = self.records[probe].zone;
                if probe_zone != 0 {
                    let prev = self.level(reference);
                    self.edge(probe, prev, zone, class6);
                    probe += 1;
                    continue;
                }
                let level_ok = (self.level(probe) - self.level(reference)).abs() < 2;
                if self.records[probe].class == class && level_ok {
                    let (filled, run_advance) = self.run(probe, zone);
                    count += filled;
                    probe += run_advance.max(1) as usize;
                } else {
                    probe += 1;
                }
            }
        }
        (count, advance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock, TileBlocks};

    /// Terrain bytes per tile id: 0 clear, 9 water, 0xA beach, 7 rock.
    struct TerrainBlocks;

    impl TileBlocks for TerrainBlocks {
        fn block(&self, tile: i32) -> Option<&TileBlock> {
            static CLEAR: std::sync::OnceLock<TileBlock> = std::sync::OnceLock::new();
            static WATER: std::sync::OnceLock<TileBlock> = std::sync::OnceLock::new();
            static BEACH: std::sync::OnceLock<TileBlock> = std::sync::OnceLock::new();
            static ROCK: std::sync::OnceLock<TileBlock> = std::sync::OnceLock::new();
            let block = |terrain: u8| TileBlock {
                width: 1,
                height: 1,
                subtiles: vec![Some(SubTile {
                    height: 0,
                    terrain,
                    slope: 0,
                })],
            };
            Some(match tile {
                500..=599 => WATER.get_or_init(|| block(9)),
                400..=449 => BEACH.get_or_init(|| block(0xA)),
                300 => ROCK.get_or_init(|| block(7)),
                _ => CLEAR.get_or_init(|| block(0)),
            })
        }
    }

    fn wheel() -> [bool; LAND_TYPES] {
        // Stock rules: only Rock (3) has Wheel=0% among the land types the
        // generator can produce.
        let mut flags = [false; LAND_TYPES];
        flags[3] = true;
        flags
    }

    fn world() -> (RmgGrid, ZoneParams<'static>) {
        let (gen_w, gen_h) = (20, 16);
        let (map_w, map_h) = (gen_w + 4, gen_h + 12);
        let stride = (map_w + map_h + 1) as usize;
        let mut grid = RmgGrid::new(stride, map_w, map_w + 2 * map_h);
        let coords: Vec<(i32, i32)> = grid.native_cells().collect();
        for (x, y) in coords {
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        let params = ZoneParams {
            map_w,
            local_rect: [2, 5, gen_w, gen_h],
            wheel_impassable: wheel(),
            cliff_back_impassability: 2,
            overlays: &[],
        };
        (grid, params)
    }

    fn playable(grid: &RmgGrid, params: &ZoneParams) -> Vec<(i32, i32)> {
        grid.native_cells()
            .filter(|&(x, y)| {
                let cell = grid.get(x, y).unwrap();
                diamond_frame_contains(
                    params.map_w,
                    params.local_rect,
                    x,
                    y,
                    cell.level,
                    cell.slope != 0,
                )
            })
            .collect()
    }

    #[test]
    fn uniform_land_is_one_reference_zone() {
        let (grid, params) = world();
        let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Amphibious);
        let cells = playable(&grid, &params);
        assert!(!cells.is_empty());
        for &(x, y) in &cells {
            assert_eq!(field.class(x, y), 0, "({x},{y}) is clear");
            assert!(field.is_reference_ground(x, y), "({x},{y}) in reference");
        }
    }

    #[test]
    fn border_ring_is_class_seven() {
        let (grid, params) = world();
        let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Amphibious);
        let playable: std::collections::HashSet<(i32, i32)> =
            playable(&grid, &params).into_iter().collect();
        let mut outside = 0;
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            if !playable.contains(&(x, y)) {
                assert_eq!(field.class(x, y), 7, "({x},{y}) outside the frame");
                assert!(!field.is_reference_ground(x, y));
                outside += 1;
            }
        }
        assert!(outside > 0, "the local frame is tighter than the diamond");
    }

    /// A small rectangle of playable cells around the playable centroid.
    fn patch(cells: &[(i32, i32)], half_w: i32, half_h: i32) -> Vec<(i32, i32)> {
        let center = cells[cells.len() / 2];
        cells
            .iter()
            .copied()
            .filter(|&(x, y)| (x - center.0).abs() <= half_w && (y - center.1).abs() <= half_h)
            .collect()
    }

    #[test]
    fn amphibious_merges_water_and_land_when_levels_touch() {
        let (mut grid, params) = world();
        // A lake at land level: same level means shoreline edges register.
        let cells = playable(&grid, &params);
        let lake = patch(&cells, 2, 1);
        assert!(!lake.is_empty());
        for &(x, y) in &lake {
            grid.get_mut(x, y).unwrap().tile = 500;
        }
        let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Amphibious);
        let land = cells
            .iter()
            .copied()
            .find(|coord| !lake.contains(coord))
            .unwrap();
        assert_eq!(field.class(lake[0].0, lake[0].1), 4, "water class");
        assert_eq!(
            field.zone(lake[0].0, lake[0].1),
            field.zone(land.0, land.1),
            "amphibious groups the lake with the shore"
        );
        assert!(
            !field.is_reference_ground(lake[0].0, lake[0].1),
            "water is not start ground"
        );
        assert!(field.is_reference_ground(land.0, land.1));
    }

    #[test]
    fn normal_kind_keeps_water_out_of_the_reference() {
        let (mut grid, params) = world();
        let cells = playable(&grid, &params);
        let lake = patch(&cells, 2, 1);
        for &(x, y) in &lake {
            grid.get_mut(x, y).unwrap().tile = 500;
        }
        let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Normal);
        assert_ne!(
            field.zone(lake[0].0, lake[0].1),
            field.reference,
            "normal kind never merges water into the land group"
        );
    }

    #[test]
    fn level_gaps_split_zones_and_block_edges() {
        let (mut grid, params) = world();
        let cells = playable(&grid, &params);
        // A raised plateau: same class, level gap of 4 -> separate base
        // zone, no adjacency edge, so Normal derives two groups.
        let plateau = patch(&cells, 2, 1);
        for &(x, y) in &plateau {
            grid.get_mut(x, y).unwrap().level = 8;
        }
        let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Normal);
        let land = cells
            .iter()
            .copied()
            .find(|coord| !plateau.contains(coord))
            .unwrap();
        assert_ne!(
            field.zone(plateau[0].0, plateau[0].1),
            field.zone(land.0, land.1),
            "a 4-level cliff splits the derived zones"
        );
    }

    /// `RecalcAttributes @ 0x0047D2B0`'s final CliffBack site turns Clear and
    /// Water behind a cliff into Rock under the retail
    /// `CliffBackImpassability=2`; `RecalcZoneType @ 0x00483C80` then blocks
    /// the Rock (Wheel 0%) instead of classing it ground or water.
    #[test]
    fn cells_behind_a_cliff_are_rock() {
        let (mut grid, mut params) = world();
        let cells = playable(&grid, &params);
        let (x, y) = cells[cells.len() / 2];
        let (wx, wy) = cells[cells.len() / 3];
        grid.get_mut(wx, wy).unwrap().tile = 500;
        // Three levels above is not a cliff; four is (the (x, y-1) probe).
        grid.get_mut(x, y - 1).unwrap().level = 7;
        let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Amphibious);
        assert_eq!(field.class(x, y), CLASS_CLEAR);
        grid.get_mut(x, y - 1).unwrap().level = 8;
        // The water cell: the (x+2, y+2) probe reaches over a neighbour.
        grid.get_mut(wx + 2, wy + 2).unwrap().level = 8;
        let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Amphibious);
        assert_eq!(field.class(x, y), CLASS_WHEEL_BLOCKED);
        assert_eq!(field.class(wx, wy), CLASS_WHEEL_BLOCKED);
        assert!(!field.is_reference_ground(x, y));
        params.cliff_back_impassability = 1;
        let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Amphibious);
        assert_eq!(field.class(x, y), CLASS_CLEAR, "only 2 reclasses");
        assert_eq!(field.class(wx, wy), CLASS_WATER);
    }

    #[test]
    fn rock_is_wheel_blocked_and_never_reference_ground() {
        let (mut grid, params) = world();
        let cells = playable(&grid, &params);
        let rock = cells[cells.len() / 2];
        grid.get_mut(rock.0, rock.1).unwrap().tile = 300;
        let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Amphibious);
        assert_eq!(field.class(rock.0, rock.1), 6);
        assert!(!field.is_reference_ground(rock.0, rock.1));
    }

    #[test]
    fn zone_field_is_deterministic() {
        let build = || {
            let (mut grid, params) = world();
            let cells = playable(&grid, &params);
            for &(x, y) in cells.iter().filter(|&&(x, y)| (x + y) % 7 == 0) {
                grid.get_mut(x, y).unwrap().tile = 500;
            }
            let field = compute(&grid, &TerrainBlocks, &params, ZoneKind::Amphibious);
            let snapshot: Vec<(u8, u16)> = grid
                .native_cells()
                .map(|(x, y)| (field.class(x, y), field.zone(x, y)))
                .collect();
            (field.reference, snapshot)
        };
        assert_eq!(build(), build());
    }
}
