//! Engineer519C07 ->570050/573540 -> ordinary strip/ramp world publication.
//! The scalar controllers retain cells across synchronous lifecycle effects.
//! Native evidence: bridge_repair, bridge_ordinary_repair and bridge_occupants
//! corpora in tools/spatial_oracle; the callbacks below own actual world state.
use super::*;
use crate::map::bridge_rim_tiles::HighBridgeRimTiles;
use crate::sim::bridge_state::ordinary_repair::OrdinaryRepairHost;
use crate::sim::bridge_state::ramp_repair::{Family, Rect, RepairHost};
use crate::sim::bridge_state::{ordinary_repair, ramp_repair};

#[path = "bridge_repair_occupants.rs"]
mod occupants;

/// The caller has admitted Infantry PerCell2's live engineer/hut receiver.
pub(crate) fn repair_from_engineer(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    engineer: u64,
) -> Result<bool, String> {
    let mut live = LivePublication {
        sim,
        rules,
        registry,
        collapsed: false,
    };
    let mut family = Family::High;
    //519C07 scans Y-major and visits every member, independently of the
    //family entry's subsequent X-major first-overlay scan.
    for y in -2..=2i16 {
        for x in -2..=2i16 {
            let current = live
                .sim
                .substrate
                .entities
                .get(engineer)
                .ok_or("repair engineer disappeared before family scan")?;
            let point = (
                (current.position.rx as i16).wrapping_add(x),
                (current.position.ry as i16).wrapping_add(y),
            );
            let cell = live.lookup(point);
            let tile = live.tile(cell);
            let cell = live.lookup(point);
            let overlay = LiveRepair {
                live: &mut live,
                changed: false,
            }
            .overlay_identity(cell);
            let wood = live.terrain().wood_bridge_set_base();
            if (wood..=wood.wrapping_add(16)).contains(&tile) || (74..=101).contains(&overlay) {
                family = Family::Low;
            }
        }
    }
    let current = live
        .sim
        .substrate
        .entities
        .get(engineer)
        .ok_or("repair engineer disappeared before family entry")?;
    let input = (current.position.rx as i16, current.position.ry as i16);
    let mut host = LiveRepair {
        live: &mut live,
        changed: false,
    };
    ramp_repair::repair(&mut host, input, family)?;
    Ok(host.changed)
}

struct LiveRepair<'a, 'world> {
    live: &'a mut LivePublication<'world>,
    changed: bool,
}

impl LiveRepair<'_, '_> {
    fn overlay_identity(&self, cell: Cell) -> i32 {
        match cell {
            Cell::Real(index) => self.live.terrain().cells()[index]
                .bridge_facts
                .overlay_id
                .map_or(-1, i32::from),
            Cell::Dummy => {
                self.live
                    .terrain()
                    .shared_cell_dummy()
                    .overlay_identity_state()
                    .0
            }
        }
    }
}

impl RepairHost for LiveRepair<'_, '_> {
    type Cell = Cell;
    type Error = String;
    fn tiles(&self, family: Family) -> HighBridgeRimTiles {
        let mut keys = self
            .live
            .terrain()
            .high_bridge_rim_tiles()
            .unwrap_or_else(|| HighBridgeRimTiles::from_ini(-1, &[]));
        if family == Family::Low {
            keys.base = self.live.terrain().wood_bridge_set_base();
        }
        keys
    }
    fn lookup(&mut self, p: CellCoord) -> Cell {
        self.live.lookup(p)
    }
    fn coord(&self, cell: Cell) -> CellCoord {
        self.live.coord(cell)
    }
    fn flags(&self, cell: Cell) -> u32 {
        self.live.flags(cell)
    }
    fn tile(&self, cell: Cell) -> i32 {
        self.live.tile(cell)
    }
    fn subtile(&self, cell: Cell) -> u8 {
        self.live.subtile(cell)
    }
    fn level(&self, cell: Cell) -> u8 {
        self.live.level(cell) as u8
    }
    fn write_level(&mut self, cell: Cell, level: u8) -> Result<(), String> {
        self.changed = true;
        self.live.write_raw_bridge_level(cell, level)
    }
    fn anchor(&self, cell: Cell) -> Result<Cell, String> {
        self.live
            .terrain()
            .native_cell_anchor(cell)
            .ok_or("repair has no retained anchor".into())
    }
    fn overlay(&self, cell: Cell) -> i32 {
        self.overlay_identity(cell)
    }
    fn search_in_bounds(&self, p: CellCoord, family: Family) -> bool {
        let Some((w, h)) = self
            .live
            .sim
            .bridge_state
            .as_ref()
            .and_then(|s| s.native_zone_source_size())
        else {
            return false;
        };
        let (x, y) = (i32::from(p.0), i32::from(p.1));
        match family {
            // Map565C10 stores(1,1,W+H-1,W+H-1);5738B1 adds the
            //origin before its inclusive comparison. This is not LocalSize.
            Family::High => x >= 1 && y >= 1 && x <= w + h && y <= w + h,
            Family::Low => x + y > w && x - y < w && y - x < w && x + y <= w + 2 * h,
        }
    }
    fn allocated(&self, p: CellCoord) -> bool {
        self.live
            .terrain()
            .native_fixed_cell_index(p.0, p.1)
            .is_some()
    }
    fn ordinary_repair(&mut self, p: CellCoord, family: Family) -> Result<(), String> {
        ordinary_repair::repair(self, p, family)
    }
    fn pavement_clear(&mut self, p: CellCoord) {
        self.live.pavement_at(p, false);
    }
    fn replace(&mut self, p: CellCoord, tile: i32) -> Result<(), String> {
        self.changed = true;
        self.live.replace_tiles(p, tile, -1)
    }
    fn validate(&mut self, p: CellCoord) -> Result<bool, String> {
        self.live.validate_bridge_zones(p)
    }
    fn construct(&mut self, p: CellCoord, overlay: u8, frame: i32) -> Result<(), String> {
        self.changed = true;
        self.live
            .construct_bridge_overlay(p, overlay, frame)
            .map(|_| ())
    }
    fn connectivity(&mut self) -> Result<(), String> {
        self.live.rebuild_bridge_connectivity()
    }
    fn rebuild(&mut self, cells: &[CellCoord]) -> Result<(), String> {
        self.live.recalculate_bridge_zones(cells)
    }
    fn project(&mut self, p: CellCoord, level: i8) -> [i32; 2] {
        let (x, y) = crate::util::lepton::project_absolute_lepton_xy(
            i32::from(p.0) * 256,
            i32::from(p.1) * 256,
        );
        [x, y - i32::from(level) * 15]
    }
    fn dirty_screen(&mut self, _: Option<Rect>) {
        // The renderer reads the live terrain each frame, as for pavement.
    }
}

impl OrdinaryRepairHost for LiveRepair<'_, '_> {
    type Cell = Cell;
    type Error = String;
    fn lookup(&mut self, p: CellCoord) -> Cell {
        self.live.lookup(p)
    }
    fn overlay(&self, cell: Cell) -> i32 {
        self.overlay_identity(cell)
    }
    fn write_overlay(&mut self, cell: Cell, overlay: u8) {
        self.changed = true;
        self.live
            .sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .write_native_cell_overlay(cell, Some(overlay));
        if let Some((x, y)) = self.live.real_coord(cell) {
            if let Some(grid) = self.live.sim.overlay_grid.as_mut() {
                grid.write_bridge_overlay_identity(x, y, overlay);
            }
            if let Some(runtime) = self
                .live
                .sim
                .bridge_state
                .as_mut()
                .and_then(|s| s.cell_mut(x, y))
            {
                runtime.overlay_byte = overlay;
            }
        }
        self.live.retain_real_write(cell);
    }
    fn variant(&mut self) -> u8 {
        self.live
            .sim
            .mapgen_rng
            .next_range_u32_inclusive_scaled(0, 3) as u8
    }
    fn redraw(&mut self, _: Cell) {}
    fn radar(&mut self, p: CellCoord) {
        self.live
            .sim
            .mark_radar_terrain_dirty_cells([(p.0 as u16, p.1 as u16)]);
    }
    fn recalc(&mut self, cell: Cell) -> Result<(), String> {
        self.live.recalc_cell(cell, -1)
    }
    fn occupants(&mut self, cell: Cell) -> Result<(), String> {
        occupants::apply(self.live, cell)
    }
    fn connectivity(&mut self) -> Result<(), String> {
        self.live.rebuild_bridge_connectivity()
    }
    fn rebuild_rectangle(&mut self, rect: Rect) -> Result<(), String> {
        crate::sim::pathfinding::zone_incremental::recalculate_zone_rectangle(self.live, rect)
    }
}

#[cfg(test)]
#[path = "bridge_repair_publication_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "track_fresh_response_tests.rs"]
mod track_fresh_response_tests;
#[cfg(test)]
#[path = "track_path_continuation_tests.rs"]
mod track_path_continuation_tests;
#[cfg(test)]
#[path = "walk_failed_path_tests.rs"]
mod walk_failed_path_tests;
