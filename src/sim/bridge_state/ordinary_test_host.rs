use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cell {
    Real(CellCoord),
    Dummy,
}

pub(crate) struct Host {
    pub cells: BTreeMap<CellCoord, i32>,
    pub dummy: (CellCoord, i32),
    pub variant: u8,
    pub trace: Vec<Value>,
}

impl Host {
    fn coord(&self, cell: Cell) -> CellCoord {
        match cell {
            Cell::Real(c) => c,
            Cell::Dummy => self.dummy.0,
        }
    }
}

impl OrdinaryBridgeHost for Host {
    type Cell = Cell;
    type Error = ();
    fn lookup(&mut self, coord: CellCoord) -> Cell {
        // Native fixed-stride aliases, rather than independent X/Y clipping.
        let index = i32::from(coord.1) * 512 + i32::from(coord.0);
        if (0..0x40000).contains(&index) {
            let canonical = ((index % 512) as i16, (index / 512) as i16);
            if self.cells.contains_key(&canonical) {
                return Cell::Real(canonical);
            }
        }
        self.dummy.0 = coord;
        Cell::Dummy
    }
    fn coord(&self, cell: Cell) -> CellCoord {
        Host::coord(self, cell)
    }
    fn overlay(&self, cell: Cell) -> i32 {
        match cell {
            Cell::Real(c) => self.cells[&c],
            Cell::Dummy => self.dummy.1,
        }
    }
    fn write_overlay(&mut self, cell: Cell, overlay: u8) {
        self.trace
            .push(json!({"kind":"overlay","coord":self.coord(cell),"overlay":overlay}));
        match cell {
            Cell::Real(c) => {
                self.cells.insert(c, i32::from(overlay));
            }
            Cell::Dummy => self.dummy.1 = i32::from(overlay),
        }
    }
    fn redraw(&mut self, _: Cell) {
        self.trace.push(json!({"kind":"screen"}));
    }
    fn radar(&mut self, coord: CellCoord) {
        self.trace.push(json!({"kind":"radar","coord":coord}));
    }
    fn recalc(&mut self, cell: Cell) -> Result<(), ()> {
        self.trace
            .push(json!({"kind":"recalc","coord":self.coord(cell),"level":-1}));
        Ok(())
    }
    fn occupants(&mut self, cell: Cell, mode: u8) -> Result<(), ()> {
        self.trace
            .push(json!({"kind":"occupants","coord":self.coord(cell),"mode":mode}));
        Ok(())
    }
    fn connectivity(&mut self) -> Result<(), ()> {
        self.trace.push(json!({"kind":"connectivity"}));
        Ok(())
    }
    fn rebuild_rectangle(&mut self, [x, y, w, h]: Rect) -> Result<(), ()> {
        let cells: Vec<_> = (x..x + w)
            .flat_map(|x| (y..y + h).map(move |y| [x as i16, y as i16]))
            .collect();
        self.trace.push(json!({"kind":"rebuild","cells":cells}));
        Ok(())
    }
}
