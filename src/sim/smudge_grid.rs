//! Per-cell mutable smudge state — runtime craters, scorch marks, and pre-placed map decals.
//!
//! Seeded from the map's [Smudge] section at sim init, mutated by the smudge
//! dispatcher during combat.
//!
//! Dependency rules: depends on rules/, map/, and other sim/ modules.
//! Never depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::map_file::MapSmudgeEntry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::smudge_type::SmudgeTypeRegistry;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::rng::SimRng;

/// Smudge category — Burn for scorches, Crater for explosion craters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmudgeKind {
    Burn,
    Crater,
}

/// Per-cell smudge slot.
///
/// `type_id` indexes into SmudgeTypeRegistry. None = no smudge on this cell.
/// `footprint_origin` is the top-left cell of the W×H footprint that owns this cell.
/// `frame_offset` distinguishes the footprint origin (== 0) from non-origin
/// cells of multi-cell smudges. Computed as
/// `(rx - origin.rx) + (ry - origin.ry) * footprint_width`. The origin cell
/// of any placed footprint always has `frame_offset == 0`. Non-origin cells
/// are skipped at render time — multi-cell SmudgeType SHPs have a single
/// composite frame, drawn once per footprint at the origin cell.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct SmudgeCell {
    pub type_id: Option<u16>,
    pub footprint_origin: Option<(u16, u16)>,
    pub frame_offset: u8,
}

/// Per-cell smudge grid. Flat Vec indexed by `ry * width + rx`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SmudgeGrid {
    width: u16,
    height: u16,
    cells: Vec<SmudgeCell>,
    /// Cells mutated this tick — drained per tick by the render-update path.
    /// Not part of game state; never serialized.
    #[serde(skip, default)]
    dirty_cells: Vec<(u16, u16)>,
}

impl SmudgeGrid {
    pub fn new(width: u16, height: u16) -> Self {
        let count: usize = width as usize * height as usize;
        Self {
            width,
            height,
            cells: vec![SmudgeCell::default(); count],
            dirty_cells: Vec::new(),
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }
    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn cell(&self, rx: u16, ry: u16) -> &SmudgeCell {
        match self.index_of(rx, ry) {
            Some(i) => &self.cells[i],
            None => &DEFAULT_CELL,
        }
    }

    fn index_of(&self, rx: u16, ry: u16) -> Option<usize> {
        if rx >= self.width || ry >= self.height {
            None
        } else {
            Some(ry as usize * self.width as usize + rx as usize)
        }
    }

    pub fn drain_dirty(&mut self) -> Vec<(u16, u16)> {
        std::mem::take(&mut self.dirty_cells)
    }

    /// Clear every complete smudge footprint touched by the supplied cells.
    /// Intersections are visited in caller order; each owning footprint is
    /// erased immediately in row-major cell order, so a later intersection
    /// with that same footprint observes an empty cell and emits no duplicate.
    pub(crate) fn clear_intersecting_footprints(&mut self, intersections: &[(u16, u16)]) -> usize {
        let mut cleared = 0;
        for &(rx, ry) in intersections {
            let Some(origin) = self
                .index_of(rx, ry)
                .and_then(|index| self.cells[index].footprint_origin)
            else {
                continue;
            };
            for index in 0..self.cells.len() {
                if self.cells[index].type_id.is_none()
                    || self.cells[index].footprint_origin != Some(origin)
                {
                    continue;
                }
                self.cells[index] = SmudgeCell::default();
                let cell_rx = (index % usize::from(self.width)) as u16;
                let cell_ry = (index / usize::from(self.width)) as u16;
                self.dirty_cells.push((cell_rx, cell_ry));
                cleared += 1;
            }
        }
        cleared
    }

    /// Raw CellClass Mark(1) writer: clear only this cell's native smudge slot
    /// and data byte, without expanding to the owning Rust footprint.
    pub(crate) fn clear_cell_slot(&mut self, rx: u16, ry: u16) -> bool {
        let Some(index) = self.index_of(rx, ry) else {
            return false;
        };
        if self.cells[index] == SmudgeCell::default() {
            return false;
        }
        self.cells[index] = SmudgeCell::default();
        self.dirty_cells.push((rx, ry));
        true
    }

    pub fn iter_occupied(&self) -> impl Iterator<Item = (u16, u16, &SmudgeCell)> {
        self.cells.iter().enumerate().filter_map(move |(idx, c)| {
            if c.type_id.is_some() {
                let rx = (idx % self.width as usize) as u16;
                let ry = (idx / self.width as usize) as u16;
                Some((rx, ry, c))
            } else {
                None
            }
        })
    }

    /// Test-only direct cell mutation. Bypasses CanPlaceHere — use only in
    /// unit tests that need to seed a known SmudgeGrid state for hashing or
    /// snapshot round-trip verification.
    #[cfg(test)]
    pub fn test_force_set(&mut self, rx: u16, ry: u16, cell: SmudgeCell) {
        if let Some(idx) = self.index_of(rx, ry) {
            self.cells[idx] = cell;
            self.dirty_cells.push((rx, ry));
        }
    }
}

const DEFAULT_CELL: SmudgeCell = SmudgeCell {
    type_id: None,
    footprint_origin: None,
    frame_offset: 0,
};

impl SmudgeGrid {
    pub fn from_map_entries(
        entries: &[MapSmudgeEntry],
        registry: &SmudgeTypeRegistry,
        terrain: &ResolvedTerrainGrid,
        overlay: &OverlayGrid,
        width: u16,
        height: u16,
    ) -> Self {
        let mut grid = Self::new(width, height);
        for entry in entries {
            let Some(type_id) = registry.find_by_name(&entry.type_name) else {
                log::warn!(
                    "[Smudge] entry references unknown SmudgeType '{}', skipping",
                    entry.type_name
                );
                continue;
            };
            let Some(def) = registry.get(type_id) else {
                continue;
            };
            // Map load has no occupancy grid yet, so `allow_building` is moot.
            if !grid.passes_placement_gates(
                entry.rx, entry.ry, def.width, def.height, terrain, overlay, None, false,
            ) {
                continue;
            }
            grid.write_footprint(entry.rx, entry.ry, type_id, def.width, def.height);
        }
        // Keep native construction invalidation visible; app initialization
        // publishes this queue synchronously after installing the grid.
        grid
    }
}

impl SmudgeGrid {
    /// `SmudgeTypeClass::CanPlace @ 0x006B5F80`: every cell of the W×H
    /// footprint must be allocated (native tests the origin's In_Bounds; the
    /// allocated cells are exactly the Size diamond), unsloped, free of smudge
    /// and overlay, on a Morphable current tile (`accepts_smudge`, with its
    /// tile-0 fallback) and, unless `allow_building`, hold no building.
    ///
    /// `allow_building` is gamemd's force argument: when set, the building
    /// lookup is skipped entirely and a smudge may land under a structure. The
    /// spawners pass their `forceBig` flag straight through, so a
    /// building-destruction centre mark is exempt while ordinary anim and
    /// survivor marks are not.
    ///
    /// Residual: native `GetCell @ 0x005657A0` answers a footprint cell off the
    /// Size diamond with the shared dummy cell, whose slope, smudge, overlay,
    /// objects and tile (the constructor's 0xFFFF, read as theater tile 0)
    /// CanPlace then reads, and `SmudgeTypeClass::Place @ 0x006B6080` writes
    /// the dummy's smudge slot, after which no footprint touching the dummy
    /// passes. The dummy's smudge slot has no Rust owner, so such a cell fails
    /// here. Trigger: a footprint over one cell wide or tall at an origin on the
    /// diamond's right or bottom edge. Effect: while the dummy is unmarked and
    /// tile 0 is Morphable (as in the retail theaters), native admits the type
    /// and places the in-map part of its mark; here the pick runs over the
    /// remaining types (one pick draw either way with the retail types, since
    /// their 1x1 types fit such an origin). Frequency: marks on the outermost
    /// cells of the full map, outside the usual playable area. Evidence:
    /// `smudge_can_place` rows whose passing CanPlace read the dummy.
    #[allow(clippy::too_many_arguments)]
    fn passes_placement_gates(
        &self,
        rx: u16,
        ry: u16,
        w: u8,
        h: u8,
        terrain: &ResolvedTerrainGrid,
        overlay: &OverlayGrid,
        occupancy: Option<&OccupancyGrid>,
        allow_building: bool,
    ) -> bool {
        for dy in 0..h as u16 {
            for dx in 0..w as u16 {
                let cx = rx + dx;
                let cy = ry + dy;
                if cx >= self.width || cy >= self.height {
                    return false;
                }
                if self.cell(cx, cy).type_id.is_some() {
                    return false;
                }
                if overlay.cell(cx, cy).overlay_id.is_some() {
                    return false;
                }
                let Some(tcell) = terrain.cell(cx, cy) else {
                    return false;
                };
                if tcell.slope_type != 0 {
                    return false;
                }
                if !tcell.accepts_smudge {
                    return false;
                }
                if !allow_building {
                    if let Some(occ) = occupancy {
                        if cell_has_building(occ, cx, cy) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn write_footprint(&mut self, rx: u16, ry: u16, type_id: u16, w: u8, h: u8) {
        for dy in 0..h as u16 {
            for dx in 0..w as u16 {
                let cx = rx + dx;
                let cy = ry + dy;
                let Some(idx) = self.index_of(cx, cy) else {
                    continue;
                };
                self.cells[idx] = SmudgeCell {
                    type_id: Some(type_id),
                    footprint_origin: Some((rx, ry)),
                    frame_offset: (dx as u8) + (dy as u8) * w,
                };
                self.dirty_cells.push((cx, cy));
            }
        }
    }
}

/// gamemd's placement check asks the cell for a *building*, not for an
/// obstruction: its lookup walks the cell's object list and returns only
/// BuildingClass objects. A vehicle sitting on the cell is never returned, so a
/// shell landing on a tank that survives still leaves its scorch.
fn cell_has_building(occupancy: &OccupancyGrid, rx: u16, ry: u16) -> bool {
    use crate::sim::movement::locomotor::MovementLayer;
    occupancy
        .get(rx, ry)
        .map_or(false, |c| c.has_building_on(MovementLayer::Ground))
}

impl SmudgeGrid {
    /// Try to place a smudge of the given kind at `coord` (lepton-space).
    ///
    /// Mirrors the two-pass selection both gamemd smudge spawners share.
    /// Pass 1 walks every SmudgeType carrying the requested Crater/Burn flag
    /// and runs the placement check (`CanPlaceHere`) on each candidate *at the
    /// target cell*, keeping only the types whose W×H footprint actually fits —
    /// this is the *placeable* list. Pass 2 applies the size preference over
    /// that placeable list. The random pick then runs over a list every entry
    /// of which is already known to fit, so the pick can never fail: if any
    /// type fits, a smudge is placed.
    ///
    /// Two consequences of that ordering, both load-bearing:
    /// - a cell that already carries one smudge, or sits under a structure on
    ///   the `force_big` path, still receives a mark whenever *some* smaller or
    ///   offset-free type fits, instead of silently dropping the mark because
    ///   the drawn type happened not to fit;
    /// - when nothing fits at all the function returns before touching the RNG,
    ///   so the shared scenario cursor does not advance on a no-op.
    ///
    /// `force_big` doubles as gamemd's `allowBuilding` argument to the
    /// placement check, so building-destruction centre marks skip the
    /// building-occupancy gate.
    ///
    /// Returns true if a smudge was placed, false otherwise. Native execution
    /// of both placers over MapClass's cell table, through Place's writes:
    /// `tools/spatial_oracle/smudge_can_place.py` (`placer`).
    /// Ore mutation is caller-specific: `AnimClass::Middle @ 0x00424F00`
    /// reduces tiberium before its crater attempt, even on placement failure;
    /// direct `BuildingClass::DestructionEffects @ 0x004415F0` and
    /// `BuildingClass::SpawnSurvivors @ 0x00442D90` callers do not.
    #[allow(clippy::too_many_arguments)]
    pub fn try_place(
        &mut self,
        kind: SmudgeKind,
        coord: SimCoord,
        dmg: i32,
        dmg2: i32,
        force_big: bool,
        registry: &SmudgeTypeRegistry,
        terrain: &ResolvedTerrainGrid,
        overlay: &OverlayGrid,
        occupancy: &OccupancyGrid,
        rng: &mut SimRng,
    ) -> bool {
        // The placers' cell: the coordinate over 256 toward zero (`cdq; and edx,
        // 0xFF; add; sar 8`). Cell (0, 0), the sentinel `0x00B0B788` (zeroed
        // by `0x006B5210`), places nothing without a draw; a negative cell, like
        // any cell off the map, fails every candidate's origin check.
        let (Ok(rx), Ok(ry)) = (u16::try_from(coord.x / 256), u16::try_from(coord.y / 256)) else {
            return false;
        };
        if rx == 0 && ry == 0 {
            return false;
        }

        // Pass 1 — flag filter and per-candidate placement check.
        let mut placeable: Vec<(u16, u8, u8)> = Vec::new();
        for (id, def) in registry.iter_with_id() {
            let flagged = match kind {
                SmudgeKind::Burn => def.burn,
                SmudgeKind::Crater => def.crater,
            };
            if !flagged {
                continue;
            }
            if self.passes_placement_gates(
                rx,
                ry,
                def.width,
                def.height,
                terrain,
                overlay,
                Some(occupancy),
                force_big,
            ) {
                placeable.push((id, def.width, def.height));
            }
        }
        let Some(chosen_id) = pick_smudge_candidate(&placeable, dmg, dmg2, force_big, rng) else {
            return false;
        };
        let chosen = registry.get(chosen_id).unwrap();
        self.write_footprint(rx, ry, chosen_id, chosen.width, chosen.height);
        true
    }
}

/// The pick of `0x006B59A0` / `0x006B5C90` over the `placeable` candidates
/// (type id, width, height, in registry order):
/// - nothing placeable: no pick and no draw;
/// - the preferred list: with `force_big`, types at least 2x2; otherwise 1x1
///   types, or every type when the anim frame is wider than 0x3C and taller
///   than 0x32;
/// - one `RandomRanged(0, n - 1)` over the preferred list, or over the whole
///   placeable list when none is preferred (no draw when `n` is 1).
///
/// Native execution: `tools/spatial_oracle/anim_middle.py` and
/// `smudge_can_place.py` (`placer`, `centre_mark`).
pub(crate) fn pick_smudge_candidate(
    placeable: &[(u16, u8, u8)],
    width: i32,
    height: i32,
    force_big: bool,
    rng: &mut SimRng,
) -> Option<u16> {
    if placeable.is_empty() {
        return None;
    }
    let preferred: Vec<u16> = placeable
        .iter()
        .filter(|&&(_, w, h)| {
            if force_big {
                w >= 2 && h >= 2
            } else {
                (w == 1 && h == 1) || (0x3C < width && 0x32 < height)
            }
        })
        .map(|&(id, _, _)| id)
        .collect();
    let all: Vec<u16> = placeable.iter().map(|&(id, _, _)| id).collect();
    let pool = if preferred.is_empty() {
        &all
    } else {
        &preferred
    };
    Some(pool[rng.next_range_u32(pool.len() as u32) as usize])
}

/// Lepton-space coord (256 leptons = 1 cell, matches gamemd's CoordStruct).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// `tools/spatial_oracle/smudge_can_place.json`: native CanPlace, placer and
/// DestructionEffects step 7 rows over a synthetic MapClass cell table, and
/// that table rebuilt in the Rust owners.
#[cfg(test)]
pub(crate) mod oracle_fixture {
    use super::*;
    use crate::map::playfield::size_diamond_contains;
    use crate::map::resolved_terrain::{current_tile_permissions, test_flat_cell};
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::CellListInsertion;
    use serde_json::Value;
    use std::sync::OnceLock;

    /// The grid around the rows' Size 6x4 diamond (x and y 1..=9): its right
    /// column is allocated, so a cell past it is off the grid.
    pub(crate) const SIDE: u16 = 10;

    pub(crate) fn corpus() -> &'static Value {
        static CORPUS: OnceLock<Value> = OnceLock::new();
        CORPUS.get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../tools/spatial_oracle/smudge_can_place.json"
            ))
            .unwrap()
        })
    }

    pub(crate) fn int(value: &Value) -> i64 {
        value.as_i64().unwrap()
    }

    pub(crate) struct OracleMap {
        pub(crate) terrain: ResolvedTerrainGrid,
        pub(crate) overlay: OverlayGrid,
        pub(crate) smudges: SmudgeGrid,
        pub(crate) occupancy: OccupancyGrid,
    }

    /// `maps.<name>`: the allocated Size diamond and each cell's current tile
    /// (Morphable through the production query over a one-tile-per-set theater
    /// INI and the production reader; the constructor's 0xFFFF where unset),
    /// slope, overlay, smudge and ground objects. The dummy cell's smudge state
    /// has no Rust owner (the residual at `passes_placement_gates`).
    pub(crate) fn map(name: &str) -> OracleMap {
        let spec = &corpus()["maps"][name];
        let size: Vec<i32> = spec["size"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| int(v) as i32)
            .collect();
        let lookup = tile_lookup(spec);
        let cells = spec["cells"].as_array().unwrap();
        let spec_cell = |rx: u16, ry: u16| {
            cells
                .iter()
                .find(|cell| int(&cell["x"]) == i64::from(rx) && int(&cell["y"]) == i64::from(ry))
        };
        let mut terrain_cells = Vec::new();
        for ry in 0..SIDE {
            for rx in 0..SIDE {
                let field = |key: &str| spec_cell(rx, ry).and_then(|cell| cell[key].as_i64());
                let mut cell = test_flat_cell(rx, ry);
                cell.final_tile_index = field("tile").unwrap_or(0xFFFF) as i32;
                cell.slope_type = field("slope").unwrap_or(0) as u8;
                cell.accepts_smudge = current_tile_permissions(&lookup, cell.final_tile_index).0;
                terrain_cells.push(cell);
            }
        }
        let mut terrain = ResolvedTerrainGrid::from_cells(SIDE, SIDE, terrain_cells);
        let diamond: Vec<(u16, u16)> = (0..SIDE)
            .flat_map(|ry| (0..SIDE).map(move |rx| (rx, ry)))
            .filter(|&(rx, ry)| size_diamond_contains(size[0], size[1], (rx as i16, ry as i16)))
            .collect();
        assert_eq!(diamond.len(), 44);
        terrain.test_set_native_allocated_cells(&diamond);
        let mut overlay = OverlayGrid::new(SIDE, SIDE);
        let mut smudges = SmudgeGrid::new(SIDE, SIDE);
        let mut occupancy = OccupancyGrid::new();
        let mut entity_id = 0;
        for cell in cells {
            let (rx, ry) = (int(&cell["x"]) as u16, int(&cell["y"]) as u16);
            if let Some(id) = cell["overlay"].as_i64() {
                let slot = overlay.cell_mut(rx, ry);
                slot.overlay_id = Some(id as u8);
                slot.overlay_data = int(&cell["overlay_data"]) as u8;
            }
            if let Some(id) = cell["smudge"].as_i64() {
                smudges.test_force_set(
                    rx,
                    ry,
                    SmudgeCell {
                        type_id: Some(id as u16),
                        footprint_origin: Some((rx, ry)),
                        frame_offset: int(&cell["smudge_data"]) as u8,
                    },
                );
            }
            for kind in cell["objects"].as_array().into_iter().flatten() {
                entity_id += 1;
                let insertion = if kind == "building" {
                    CellListInsertion::AppendBuilding
                } else {
                    CellListInsertion::PrependNonBuilding
                };
                occupancy.add(rx, ry, entity_id, MovementLayer::Ground, None, insertion);
            }
        }
        let _ = smudges.drain_dirty();
        OracleMap {
            terrain,
            overlay,
            smudges,
            occupancy,
        }
    }

    /// A map spec's IsoTileType Morphable bits as a theater INI of one tile per
    /// set, through the production reader.
    pub(crate) fn tile_lookup(spec: &Value) -> crate::map::theater::TilesetLookup {
        let tile_count = int(&spec["tile_count"]) as usize;
        let morphable: Vec<i64> = spec["morphable"]
            .as_array()
            .unwrap()
            .iter()
            .map(int)
            .collect();
        let ini: String = (0..tile_count)
            .map(|tile| {
                let morph = if morphable.contains(&(tile as i64)) {
                    "yes"
                } else {
                    "no"
                };
                format!(
                    "[TileSet{tile:04}]\nTilesInSet=1\nFileName=tile{tile}\nMorphable={morph}\n"
                )
            })
            .collect();
        let lookup = crate::map::theater::parse_tileset_ini(ini.as_bytes(), "tem").unwrap();
        assert_eq!(lookup.len(), tile_count);
        lookup
    }

    /// The rows' [SmudgeTypes] through the production reader; its ids are the
    /// native ArrayIndex (`+0x294`, list order) that Place writes.
    pub(crate) fn registry() -> SmudgeTypeRegistry {
        let types = corpus()["smudge_types"].as_array().unwrap();
        let name = |def: &Value| def["name"].as_str().unwrap().to_string();
        let flag = |def: &Value, key: &str| if int(&def[key]) != 0 { "yes" } else { "no" };
        let mut ini = String::from("[SmudgeTypes]\n");
        for (index, def) in types.iter().enumerate() {
            ini += &format!("{}={}\n", index + 1, name(def));
        }
        for def in types {
            ini += &format!(
                "[{}]\nBurn={}\nCrater={}\nWidth={}\nHeight={}\n",
                name(def),
                flag(def, "burn"),
                flag(def, "crater"),
                int(&def["width"]),
                int(&def["height"])
            );
        }
        let registry = SmudgeTypeRegistry::from_rules_ini(
            &crate::rules::ini_parser::IniFile::from_bytes(ini.as_bytes()).unwrap(),
        );
        for (index, def) in types.iter().enumerate() {
            assert_eq!(registry.find_by_name(&name(def)), Some(index as u16));
        }
        registry
    }

    /// A row's Scenario RNG as seeded, checked against the native state.
    pub(crate) fn seeded_rng(row: &Value) -> SimRng {
        let seed = row["input"]["seed"].as_u64().unwrap();
        let rng = SimRng::new(seed);
        assert_eq!(
            rng.native_state_hex(),
            corpus()["seed_states"][seed.to_string()].as_str().unwrap()
        );
        rng
    }

    /// Whether some CanPlace passed on the dummy cell: the residual at
    /// `passes_placement_gates`, where Rust rejects that type.
    pub(crate) fn passed_on_dummy(row: &Value) -> bool {
        row["events"].as_array().unwrap().iter().any(|event| {
            event["call"] == "can_place"
                && event["dummy_read"] == true
                && int(&event["result"]) == 1
        })
    }

    /// Place's writes as (cell, SmudgeTypeIndex, SmudgeData), in its y-outer
    /// order; `after`'s marks that `before` lacks, in the same order.
    pub(crate) fn assert_marks(before: &SmudgeGrid, after: &SmudgeGrid, row: &Value) {
        let native: Vec<_> = row["marked"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mark| {
                assert_eq!(mark["dummy"], false, "{}", row["input"]);
                let cell = mark["cell"].as_array().unwrap();
                (
                    (int(&cell[1]) as u16, int(&cell[0]) as u16),
                    Some(int(&mark["type"]) as u16),
                    int(&mark["data"]) as u8,
                )
            })
            .collect();
        let mut actual: Vec<_> = after
            .iter_occupied()
            .filter(|&(rx, ry, cell)| before.cell(rx, ry) != cell)
            .map(|(rx, ry, cell)| ((ry, rx), cell.type_id, cell.frame_offset))
            .collect();
        actual.sort();
        assert_eq!(actual, native, "{}", row["input"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainCell;

    fn make_terrain(w: u16, h: u16, accepts: bool) -> ResolvedTerrainGrid {
        let mut cells: Vec<ResolvedTerrainCell> = Vec::with_capacity((w * h) as usize);
        for ry in 0..h {
            for rx in 0..w {
                cells.push(ResolvedTerrainCell {
                    rx,
                    ry,
                    source_tile_index: 0,
                    source_sub_tile: 0,
                    final_tile_index: 0,
                    final_sub_tile: 0,
                    is_wood_bridge_repair_tile: false,
                    level: 0,
                    filled_clear: true,
                    tileset_index: Some(0),
                    land_type: 0,
                    yr_cell_land_type: 0,
                    slope_type: 0,
                    template_height: 0,
                    render_offset_x: 0,
                    render_offset_y: 0,
                    terrain_class: Default::default(),
                    speed_costs: Default::default(),
                    is_water: false,
                    is_cliff_like: false,
                    is_rough: false,
                    is_road: false,
                    height_in_pixels: 0,
                    variant: 0,
                    has_ramp: false,
                    canonical_ramp: None,
                    ground_walk_blocked: false,
                    terrain_object_blocks: false,
                    terrain_object_occupation: None,
                    overlay_blocks: false,
                    overlay_zone_type: None,
                    outside_playfield: false,
                    zone_type: 0,
                    base_ground_walk_blocked: false,
                    base_build_blocked: false,
                    base_land_type: 0,
                    base_yr_cell_land_type: 0,
                    base_terrain_class: Default::default(),
                    base_speed_costs: Default::default(),
                    build_blocked: false,
                    has_bridge_deck: false,
                    bridge_walkable: false,
                    bridge_transition: false,
                    bridge_deck_level: 0,
                    bridge_layer: None,
                    bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                    tube_index: None,
                    radar_left: [0; 3],
                    radar_right: [0; 3],
                    accepts_smudge: accepts,
                    allows_tiberium: false,
                    has_damaged_data: false,
                    bridgehead_anchor_class_at_load: None,
                });
            }
        }
        ResolvedTerrainGrid::from_cells(w, h, cells)
    }

    fn make_registry_with_one_crater_1x1() -> SmudgeTypeRegistry {
        let ini = crate::rules::ini_parser::IniFile::from_bytes(
            b"[SmudgeTypes]\n1=CR1\n[CR1]\nCrater=yes\nWidth=1\nHeight=1\n",
        )
        .unwrap();
        SmudgeTypeRegistry::from_rules_ini(&ini)
    }

    #[test]
    fn try_place_writes_one_cell_for_1x1() {
        let mut grid = SmudgeGrid::new(8, 8);
        let registry = make_registry_with_one_crater_1x1();
        let terrain = make_terrain(8, 8, true);
        let overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 0,
        };
        assert!(grid.try_place(
            SmudgeKind::Crater,
            coord,
            30,
            30,
            false,
            &registry,
            &terrain,
            &overlay,
            &occupancy,
            &mut rng,
        ));
        assert!(grid.cell(4, 4).type_id.is_some());
    }

    #[test]
    fn rejects_when_accepts_smudge_false() {
        let mut grid = SmudgeGrid::new(8, 8);
        let registry = make_registry_with_one_crater_1x1();
        let terrain = make_terrain(8, 8, false); // Morphable=no
        let overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 0,
        };
        assert!(!grid.try_place(
            SmudgeKind::Crater,
            coord,
            30,
            30,
            false,
            &registry,
            &terrain,
            &overlay,
            &occupancy,
            &mut rng,
        ));
        assert!(grid.cell(4, 4).type_id.is_none());
    }

    #[test]
    fn rejects_when_overlay_present() {
        let mut grid = SmudgeGrid::new(8, 8);
        let registry = make_registry_with_one_crater_1x1();
        let terrain = make_terrain(8, 8, true);
        let mut overlay = OverlayGrid::new(8, 8);
        overlay.place_overlay(4, 4, 0, 0);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 0,
        };
        assert!(!grid.try_place(
            SmudgeKind::Crater,
            coord,
            30,
            30,
            false,
            &registry,
            &terrain,
            &overlay,
            &occupancy,
            &mut rng,
        ));
    }

    #[test]
    fn threshold_strict_less_than_for_size_filter() {
        // Registry: one 1x1 crater + one 2x2 crater. With dmg=60, dmg2=50 (strict < fails),
        // only the 1x1 should be selectable.
        let ini = crate::rules::ini_parser::IniFile::from_bytes(
            b"[SmudgeTypes]\n1=CR1\n2=CR2\n\
              [CR1]\nCrater=yes\nWidth=1\nHeight=1\n\
              [CR2]\nCrater=yes\nWidth=2\nHeight=2\n",
        )
        .unwrap();
        let registry = SmudgeTypeRegistry::from_rules_ini(&ini);
        let terrain = make_terrain(8, 8, true);
        let overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 0,
        };
        // Run try_place 50 times; with dmg=60, dmg2=50 only CR1 (1x1) should be picked.
        // Verify no 2x2 footprints land (CR2 would write 4 cells; CR1 writes 1).
        for _ in 0..50 {
            let mut grid = SmudgeGrid::new(8, 8);
            grid.try_place(
                SmudgeKind::Crater,
                coord,
                60,
                50,
                false,
                &registry,
                &terrain,
                &overlay,
                &occupancy,
                &mut rng,
            );
            // Count occupied cells; must be 0 or 1, never 4.
            let occupied = grid.iter_occupied().count();
            assert!(occupied <= 1, "1x1 only; saw {} cells", occupied);
        }
    }

    #[test]
    fn empty_filter_falls_back_to_unfiltered() {
        // Registry has only a 2x2 crater; with force_big=false and dmg below threshold,
        // size filter eliminates it but fallback to unfiltered should still pick it.
        let ini = crate::rules::ini_parser::IniFile::from_bytes(
            b"[SmudgeTypes]\n1=CR2\n[CR2]\nCrater=yes\nWidth=2\nHeight=2\n",
        )
        .unwrap();
        let registry = SmudgeTypeRegistry::from_rules_ini(&ini);
        let mut grid = SmudgeGrid::new(8, 8);
        let terrain = make_terrain(8, 8, true);
        let overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 0,
        };
        assert!(grid.try_place(
            SmudgeKind::Crater,
            coord,
            30,
            30,
            false,
            &registry,
            &terrain,
            &overlay,
            &occupancy,
            &mut rng,
        ));
        // 2x2 footprint placed at (4,4): 4 cells written.
        assert_eq!(grid.iter_occupied().count(), 4);
    }

    fn registry_1x1_and_2x2_craters() -> SmudgeTypeRegistry {
        let ini = crate::rules::ini_parser::IniFile::from_bytes(
            b"[SmudgeTypes]\n1=CR1\n2=CR2\n\
              [CR1]\nCrater=yes\nWidth=1\nHeight=1\n\
              [CR2]\nCrater=yes\nWidth=2\nHeight=2\n",
        )
        .unwrap();
        SmudgeTypeRegistry::from_rules_ini(&ini)
    }

    const CENTER_COORD: SimCoord = SimCoord {
        x: 4 * 256 + 128,
        y: 4 * 256 + 128,
        z: 0,
    };

    #[test]
    fn occupied_neighbour_blocks_2x2_but_1x1_still_lands_without_a_draw() {
        // gamemd builds the placeable list BEFORE the random pick, so a cell
        // whose 2x2 footprint is fouled by an earlier crater still receives the
        // 1x1 that does fit. Picking first and gating afterwards would drop the
        // mark whenever the 2x2 came up.
        let registry = registry_1x1_and_2x2_craters();
        let mut grid = SmudgeGrid::new(8, 8);
        // Earlier smudge at (5,4): inside CR2's footprint from (4,4), but not
        // on the impact cell itself.
        grid.test_force_set(
            5,
            4,
            SmudgeCell {
                type_id: Some(0),
                footprint_origin: Some((5, 4)),
                frame_offset: 0,
            },
        );
        let _ = grid.drain_dirty();
        let terrain = make_terrain(8, 8, true);
        let overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);

        // dmg/dmg2 above both thresholds, so the size pass keeps every
        // placeable type and cannot itself be what eliminates CR2.
        assert!(grid.try_place(
            SmudgeKind::Crater,
            CENTER_COORD,
            100,
            100,
            false,
            &registry,
            &terrain,
            &overlay,
            &occupancy,
            &mut rng,
        ));
        // CR1 (id 0) is the only type that fits.
        assert_eq!(grid.cell(4, 4).type_id, Some(0));
        // A single-entry pool means the ranged draw is a no-op, so the shared
        // cursor must not have moved. Under a pick-then-gate order the pool
        // would have held both types and consumed a draw.
        assert_eq!(rng.state(), SimRng::new(1).state());
    }

    #[test]
    fn force_big_centre_mark_places_on_a_building_cell() {
        // gamemd forwards `forceBig` as CanPlaceHere's `allowBuilding`, so the
        // building-destruction centre mark lands under the wreck.
        let registry = registry_1x1_and_2x2_craters();
        let mut grid = SmudgeGrid::new(8, 8);
        let terrain = make_terrain(8, 8, true);
        let overlay = OverlayGrid::new(8, 8);
        let mut occupancy = OccupancyGrid::new();
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::CellListInsertion;
        for (cx, cy) in [(4, 4), (5, 4), (4, 5), (5, 5)] {
            occupancy.add(
                cx,
                cy,
                7,
                MovementLayer::Ground,
                None,
                CellListInsertion::AppendBuilding,
            );
        }
        let mut rng = SimRng::new(1);

        assert!(grid.try_place(
            SmudgeKind::Crater,
            CENTER_COORD,
            100,
            100,
            true, // force_big => allowBuilding
            &registry,
            &terrain,
            &overlay,
            &occupancy,
            &mut rng,
        ));
        // force_big prefers >=2x2, so CR2 lands across all four cells.
        assert_eq!(grid.iter_occupied().count(), 4);

        // Same cell, same occupancy, force_big=false: the building gate applies
        // and nothing is placed.
        let mut plain_grid = SmudgeGrid::new(8, 8);
        let mut plain_rng = SimRng::new(1);
        assert!(!plain_grid.try_place(
            SmudgeKind::Crater,
            CENTER_COORD,
            100,
            100,
            false,
            &registry,
            &terrain,
            &overlay,
            &occupancy,
            &mut plain_rng,
        ));
        assert_eq!(plain_grid.iter_occupied().count(), 0);
    }

    #[test]
    fn surviving_vehicle_does_not_block_a_smudge_but_a_structure_does() {
        // gamemd's building lookup returns BuildingClass objects only, so a
        // shell landing on a tank that lives still scorches the ground. Testing
        // "any ground blocker" instead would swallow the mark on every cell
        // holding a surviving vehicle.
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::CellListInsertion;
        let registry = registry_1x1_and_2x2_craters();
        let terrain = make_terrain(8, 8, true);
        let overlay = OverlayGrid::new(8, 8);

        // A vehicle on the impact cell: non-force_big smudge still lands.
        let mut vehicle_occupancy = OccupancyGrid::new();
        vehicle_occupancy.add(
            4,
            4,
            11,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let mut grid = SmudgeGrid::new(8, 8);
        let mut rng = SimRng::new(1);
        assert!(
            grid.try_place(
                SmudgeKind::Crater,
                CENTER_COORD,
                100,
                100,
                false,
                &registry,
                &terrain,
                &overlay,
                &vehicle_occupancy,
                &mut rng,
            ),
            "a surviving vehicle is not a building and must not block the mark"
        );
        assert!(grid.cell(4, 4).type_id.is_some());

        // A structure on the same cell: the non-force_big gate still applies.
        let mut building_occupancy = OccupancyGrid::new();
        building_occupancy.add(
            4,
            4,
            12,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let mut blocked = SmudgeGrid::new(8, 8);
        let mut blocked_rng = SimRng::new(1);
        assert!(!blocked.try_place(
            SmudgeKind::Crater,
            CENTER_COORD,
            100,
            100,
            false,
            &registry,
            &terrain,
            &overlay,
            &building_occupancy,
            &mut blocked_rng,
        ));
        assert_eq!(blocked.iter_occupied().count(), 0);
    }

    #[test]
    fn no_rng_draw_when_nothing_can_be_placed() {
        // Overlay on the impact cell rejects every candidate; gamemd returns
        // from pass 1 without reaching RandomRanged.
        let registry = registry_1x1_and_2x2_craters();
        let mut grid = SmudgeGrid::new(8, 8);
        let terrain = make_terrain(8, 8, true);
        let mut overlay = OverlayGrid::new(8, 8);
        overlay.place_overlay(4, 4, 0, 0);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);

        assert!(!grid.try_place(
            SmudgeKind::Crater,
            CENTER_COORD,
            100,
            100,
            false,
            &registry,
            &terrain,
            &overlay,
            &occupancy,
            &mut rng,
        ));
        assert_eq!(grid.iter_occupied().count(), 0);
        assert_eq!(
            rng.state(),
            SimRng::new(1).state(),
            "no candidate fits, so the scenario cursor must not advance"
        );
    }

    #[test]
    fn gsi_04_11_runtime_zero_zero_sentinel_is_not_an_axis_wide_gate() {
        let registry = registry_1x1_and_2x2_craters();
        let terrain = make_terrain(8, 8, true);
        let overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();

        let mut origin_grid = SmudgeGrid::new(8, 8);
        let mut origin_rng = SimRng::new(9);
        let origin_state = origin_rng.logical_state();
        assert!(!origin_grid.try_place(
            SmudgeKind::Crater,
            SimCoord {
                x: 128,
                y: 128,
                z: 0,
            },
            100,
            100,
            false,
            &registry,
            &terrain,
            &overlay,
            &occupancy,
            &mut origin_rng,
        ));
        assert_eq!(origin_rng.logical_state(), origin_state);

        for (rx, ry) in [(0_u16, 1_u16), (1, 0)] {
            let mut grid = SmudgeGrid::new(8, 8);
            let mut rng = SimRng::new(9);
            assert!(grid.try_place(
                SmudgeKind::Crater,
                SimCoord {
                    x: i32::from(rx) * 256 + 128,
                    y: i32::from(ry) * 256 + 128,
                    z: 0,
                },
                100,
                100,
                false,
                &registry,
                &terrain,
                &overlay,
                &occupancy,
                &mut rng,
            ));
            assert_ne!(rng.logical_state(), SimRng::new(9).logical_state());
        }

        let mut loaded = SmudgeGrid::from_map_entries(
            &[MapSmudgeEntry {
                type_name: "CR1".to_string(),
                rx: 0,
                ry: 0,
            }],
            &registry,
            &terrain,
            &overlay,
            8,
            8,
        );
        assert!(loaded.cell(0, 0).type_id.is_some());
        let _ = loaded.drain_dirty();
    }

    #[test]
    fn gsi_04_11_map_load_retains_row_major_footprint_dirties() {
        let registry = registry_1x1_and_2x2_craters();
        let terrain = make_terrain(8, 8, true);
        let overlay = OverlayGrid::new(8, 8);
        let mut loaded = SmudgeGrid::from_map_entries(
            &[MapSmudgeEntry {
                type_name: "CR2".to_string(),
                rx: 2,
                ry: 3,
            }],
            &registry,
            &terrain,
            &overlay,
            8,
            8,
        );

        assert_eq!(loaded.drain_dirty(), vec![(2, 3), (3, 3), (2, 4), (3, 4)]);
        assert!(loaded.drain_dirty().is_empty());
    }

    /// `SmudgeTypeClass::CanPlace @ 0x006B5F80` executed over MapClass's cell
    /// table (`tools/spatial_oracle/smudge_can_place.py`, `can_place`): every
    /// origin of a Size 6x4 map and seven off it, six footprints, both force
    /// values, on three variants of the map. A passing CanPlace that read the
    /// dummy cell is the residual at `passes_placement_gates`; a negative
    /// origin is no cell here (`try_place` rejects it; the `placer` rows).
    #[test]
    fn can_place_matches_the_original_over_the_cell_table() {
        use oracle_fixture::{corpus, int};
        let rows = corpus()["can_place"].as_array().unwrap();
        let mut maps = std::collections::HashMap::new();
        let (mut compared, mut residual) = (0, 0);
        for row in rows {
            let name = row["map"].as_str().unwrap();
            let map = maps
                .entry(name)
                .or_insert_with(|| oracle_fixture::map(name));
            let origin = row["origin"].as_array().unwrap();
            let (Ok(rx), Ok(ry)) = (
                u16::try_from(int(&origin[0])),
                u16::try_from(int(&origin[1])),
            ) else {
                continue;
            };
            let native = int(&row["result"]) == 1;
            if native && row["dummy_read"] == true {
                // Only a clean dummy on a theater whose tile 0 is Morphable passes.
                assert_eq!(name, "gates");
                residual += 1;
                continue;
            }
            let actual = map.smudges.passes_placement_gates(
                rx,
                ry,
                int(&row["width"]) as u8,
                int(&row["height"]) as u8,
                &map.terrain,
                &map.overlay,
                Some(&map.occupancy),
                int(&row["force"]) == 1,
            );
            assert_eq!(actual, native, "{row}");
            compared += 1;
        }
        assert!(compared > 1500 && residual > 0, "{compared} {residual}");
    }

    /// The Burn/Crater placers `0x006B59A0` / `0x006B5C90` executed over the
    /// same cell table (`smudge_can_place.py`, `placer`) against `try_place`:
    /// the truncated cell, the (0, 0) sentinel, cells off the map, the CanPlace
    /// sweep, the preference and pick, Place's footprint (type and SmudgeData
    /// per cell) and the Scenario RNG afterwards.
    #[test]
    fn placers_match_the_original_over_the_cell_table() {
        use oracle_fixture::{corpus, int};
        let registry = oracle_fixture::registry();
        let (mut compared, mut residual) = (0, 0);
        for row in corpus()["placer"].as_array().unwrap() {
            if oracle_fixture::passed_on_dummy(row) {
                residual += 1;
                continue;
            }
            let input = &row["input"];
            let mut map = oracle_fixture::map(input["map"].as_str().unwrap());
            let before = map.smudges.clone();
            let mut rng = oracle_fixture::seeded_rng(row);
            let coord = input["coord"].as_array().unwrap();
            let kind = if input["kind"] == "burn" {
                SmudgeKind::Burn
            } else {
                SmudgeKind::Crater
            };
            let placed = map.smudges.try_place(
                kind,
                SimCoord {
                    x: int(&coord[0]) as i32,
                    y: int(&coord[1]) as i32,
                    z: int(&coord[2]) as i32,
                },
                int(&input["width"]) as i32,
                int(&input["height"]) as i32,
                int(&input["force"]) == 1,
                &registry,
                &map.terrain,
                &map.overlay,
                &map.occupancy,
                &mut rng,
            );
            oracle_fixture::assert_marks(&before, &map.smudges, row);
            assert_eq!(placed, !row["marked"].as_array().unwrap().is_empty());
            assert_eq!(
                rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{input}"
            );
            compared += 1;
        }
        assert!(compared > 100 && residual > 0, "{compared} {residual}");
    }
}
