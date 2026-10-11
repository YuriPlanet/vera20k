//! Live CellClass pavement authority for56E990. Bit13 is independent of
//! structural bridge membership and has no implicit Recalc/projection effects.

use super::*;
use crate::map::bridge_pavement::DAMAGED_PAVEMENT;

impl ResolvedTerrainGrid {
    pub(crate) fn pavement_damaged_at(&self, rx: u16, ry: u16) -> bool {
        self.cell(rx, ry)
            .is_some_and(|cell| cell.bridge_facts.raw_flags & DAMAGED_PAVEMENT != 0)
    }

    pub(crate) fn write_pavement_flags(&mut self, cell: NativeCellIdentity, flags: u32) {
        // The core preserves every other flag. Do not route through the bridge
        // setter projection:56E990 writes only the word and marks display dirt.
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.raw_flags = flags,
            NativeCellIdentity::Dummy => self.shared_cell_dummy.write_raw_flags(flags),
        }
    }

    pub(crate) fn pavement_gate(&self, cell: NativeCellIdentity) -> bool {
        let NativeCellIdentity::Real(index) = cell else {
            return false;
        };
        let cell = &self.cells[index];
        if self.tile_registry_len.is_none() {
            // Explicit synthetic/from_cells authority; production owns the
            // pristine registered catalogue, including modulo/sparse semantics.
            return cell.has_damaged_data;
        }
        self.native_tmp_has_damaged_data(cell.final_tile_index, cell.final_sub_tile)
            .unwrap_or_else(|| {
                log::error!(
                    "pavement update lacks resident pristine tile {}",
                    cell.final_tile_index
                );
                false
            })
    }

    /// Native480350 checks file count before the pristine damaged-data gate.
    /// None leaves the ordinary coordinate-selected file choice to its caller.
    pub(crate) fn pavement_draw_variant(&self, rx: u16, ry: u16) -> Option<u8> {
        let cell = self.cell(rx, ry)?;
        if let Some(count) = self.native_tmp_file_count(cell.final_tile_index) {
            if count < 2 {
                return Some(0);
            }
            if !self.native_tmp_has_damaged_data(cell.final_tile_index, cell.final_sub_tile)? {
                return None;
            }
        } else if !cell.has_damaged_data {
            return None;
        }
        Some(u8::from(self.pavement_damaged_at(rx, ry)))
    }
}
