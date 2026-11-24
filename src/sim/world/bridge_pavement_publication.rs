//! Original56E990 through live terrain flags and immediate retained writes.
use super::*;
use crate::map::bridge_pavement::{self, PavementHost};

impl LivePublication<'_> {
    pub(super) fn pavement_at(&mut self, requested: CellCoord, state: bool) {
        bridge_pavement::set_connected(&mut LivePavement { live: self }, requested, state);
    }
}

struct LivePavement<'a, 'world> {
    live: &'a mut LivePublication<'world>,
}

impl PavementHost for LivePavement<'_, '_> {
    type Cell = Cell;
    fn lookup(&mut self, requested: CellCoord) -> Cell {
        self.live.lookup(requested)
    }
    fn tile(&self, cell: Cell) -> i32 {
        match cell {
            Cell::Real(index) => self.live.terrain().cells()[index].final_tile_index,
            Cell::Dummy => 0xffff,
        }
    }
    fn flags(&self, cell: Cell) -> u32 {
        self.live.flags(cell)
    }
    fn write_flags(&mut self, cell: Cell, flags: u32) {
        self.live
            .sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .write_pavement_flags(cell, flags);
        self.live.retain_real_write(cell);
    }
    fn has_damaged_data(&mut self, cell: Cell) -> bool {
        self.live.terrain().pavement_gate(cell)
    }
    fn initial_screen(&mut self, _: CellCoord, _: Cell) {
        // Terrain batches are rebuilt each frame from the live raw flag;
        // native's redundant kickoff redraw requires no extra scalar mutation.
    }
    fn radar(&mut self, cell: Cell) {
        self.live.radar(cell);
    }
}
