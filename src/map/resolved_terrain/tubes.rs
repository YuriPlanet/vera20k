//! Tube registry: explicit map tubes, automatic low-bridge tubes and the
//! per-cell tube lookups and direction walks.

use super::*;

#[derive(Debug, thiserror::Error)]
pub enum NativeTubeRegistryBindError {
    #[error("pending authored terrain Tube registry is already populated")]
    RegistryAlreadyPopulated,
    #[error("pending authored terrain Tube registry could not reserve {entries} entries")]
    RegistryCapacity { entries: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathWalkError {
    DirectionData { raw_token: i32, native_address: u32 },
    TubeIndex(i16),
}

impl ResolvedTerrainGrid {
    /// Path walk at 0x00429780: used by both Run_AStar (0x004CBC0C) and
    /// the tube hierarchy (0x00582D70). Direction 8 follows the tube exit.
    pub(crate) fn walk_native_path(
        &self,
        mut coord: (u16, u16),
        steps: &[i32],
    ) -> Result<(u16, u16), PathWalkError> {
        for &step in steps {
            if step == 8 {
                let index = self.raw_tube_index_at_native_coord(coord);
                coord = if index == -1 {
                    (0, 0)
                } else {
                    self.tube(crate::map::tube_facts::TubeId(index as u16))
                        .filter(|_| index >= 0)
                        .ok_or(PathWalkError::TubeIndex(index))?
                        .exit
                };
            } else {
                // x86 effective address89F688 + step*4 wraps in32 bits. Exact8
                // above is special; address aliases of slot8 are ordinary reads.
                let slot = step as u32 & 0x3fff_ffff;
                let (dx, dy) = match slot {
                    0..=7 => crate::util::direction::DIRECTION_DELTAS[slot as usize],
                    // Original49F2D0/49F280 initialize slots8..12 to0; slot13
                    // is zero BSS with no direct xrefs in the examined image.
                    8..=13 => (0, 0),
                    // Original startup49F0E0/49F190 at captured FPCW0xE7F.
                    // Their initialized bytes match the unpatched process capture
                    // (tube_startup_capture.json); arbitrary neighboring data stays open.
                    14 => (0, 0x6000),
                    15 => (-24418, 16502),
                    0x3fff_fffe => (11544, 21572),
                    0x3fff_ffff => (8699, 16377),
                    _ => {
                        return Err(PathWalkError::DirectionData {
                            raw_token: step,
                            native_address: 0x0089_f688u32
                                .wrapping_add((step as u32).wrapping_mul(4)),
                        });
                    }
                };
                coord = (
                    coord.0.wrapping_add(dx as u16),
                    coord.1.wrapping_add(dy as u16),
                );
            }
        }
        Ok(coord)
    }

    pub fn tube_facts(&self) -> &[TubeFact] {
        &self.tube_facts
    }

    /// Consume the successful raw `[Tubes]` constructor receipt into the one
    /// live registry before authored OverlayClass Mark/Recalc begins.
    pub fn bind_native_map_tubes(
        &mut self,
        receipt: NativeMapTubeReceipt,
    ) -> Result<usize, NativeTubeRegistryBindError> {
        if !self.tube_facts.is_empty() || !self.tube_native_ids.is_empty() {
            return Err(NativeTubeRegistryBindError::RegistryAlreadyPopulated);
        }
        let entries = receipt.entries.len();
        self.tube_facts
            .try_reserve(entries)
            .map_err(|_| NativeTubeRegistryBindError::RegistryCapacity { entries })?;
        self.tube_native_ids
            .try_reserve(entries)
            .map_err(|_| NativeTubeRegistryBindError::RegistryCapacity { entries })?;

        for constructed in receipt.entries {
            let source_ordinal = constructed.native_init.source_entry_ordinal;
            let native_unique_id = constructed.native_init.native_unique_id;
            let entry = constructed.fact.entry;
            self.tube_facts.push(constructed.fact);
            self.tube_native_ids.push(Some(native_unique_id));

            let x = entry.0 as i16;
            let y = entry.1 as i16;
            if let Some(index) = self.native_fixed_cell_index(x, y) {
                let raw = NativeTubeCellIndex::from_registry_ordinal(source_ordinal);
                self.native_tube_indices[index] = raw;
                self.cells[index].tube_index = raw.validated_id(self.tube_facts.len());
            } else {
                self.shared_cell_dummy
                    .stamp_coord(i32::from(x), i32::from(y));
                // ReadTubesINI728515 writes through the returned pointer,
                // including the one retained shared dummy CellClass.
                self.shared_cell_dummy
                    .write_raw_tube_index(source_ordinal as i16);
            }
        }
        Ok(entries)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn tube_native_id(&self, tube_id: TubeId) -> Option<i32> {
        self.tube_native_ids
            .get(tube_id.as_usize())
            .copied()
            .flatten()
    }

    pub fn tube(&self, tube_id: TubeId) -> Option<&TubeFact> {
        self.tube_facts.get(tube_id.as_usize())
    }

    pub fn tube_at_cell(&self, rx: u16, ry: u16) -> Option<&TubeFact> {
        // No index validates against an empty registry.
        if self.tube_facts.is_empty() {
            return None;
        }
        let index = self.index(rx, ry)?;
        let tube_id = self
            .native_tube_indices
            .get(index)
            .copied()?
            .validated_id(self.tube_facts.len())?;
        self.tube(tube_id)
    }

    /// Get_CellClass5657A0 followed by the raw Cell+116 read used by429780.
    pub(crate) fn raw_tube_index_at_native_coord(&self, coord: (u16, u16)) -> i16 {
        let (x, y) = (coord.0 as i16, coord.1 as i16);
        if let Some(index) = self.native_fixed_cell_index(x, y) {
            self.native_tube_indices[index].raw()
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            self.shared_cell_dummy.raw_tube_index()
        }
    }

    /// GetTube484F20 validates the signed registry index, without a Land gate.
    pub(crate) fn tube_at_native_coord(&self, coord: (u16, u16)) -> Option<&TubeFact> {
        let index = NativeTubeCellIndex::from_raw(self.raw_tube_index_at_native_coord(coord));
        self.tube(index.validated_id(self.tube_facts.len())?)
    }

    /// Cell484F20 reads its retained receiver's signed index; unlike a map
    /// coordinate helper this performs no additional lookup or Dummy stamp.
    pub(crate) fn tube_for_native_cell(&self, cell: NativeCellIdentity) -> Option<&TubeFact> {
        // No index validates against an empty registry.
        if self.tube_facts.is_empty() {
            return None;
        }
        let index = match cell {
            NativeCellIdentity::Real(index) => self.native_tube_indices[index],
            NativeCellIdentity::Dummy => {
                NativeTubeCellIndex::from_raw(self.shared_cell_dummy.raw_tube_index())
            }
        };
        self.tube(index.validated_id(self.tube_facts.len())?)
    }

    pub fn step_coord_by_direction(&self, coord: (u16, u16), direction: u8) -> Option<(u16, u16)> {
        if crate::util::direction::is_tube_step_direction(direction) {
            return Some(
                self.tube_at_cell(coord.0, coord.1)
                    .map_or((0, 0), |tube| tube.exit),
            );
        }
        let (dx, dy) = crate::util::direction::direction_delta(direction)?;
        let nx = coord.0 as i32 + dx;
        let ny = coord.1 as i32 + dy;
        if nx < 0 || ny < 0 || nx >= self.width as i32 || ny >= self.height as i32 {
            return None;
        }
        Some((nx as u16, ny as u16))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn walk_directions_from(&self, start: (u16, u16), directions: &[u8]) -> Option<(u16, u16)> {
        let mut coord = start;
        for &direction in directions {
            coord = self.step_coord_by_direction(coord, direction)?;
        }
        Some(coord)
    }
}

impl NativeCellQuery<'_> {
    /// CellClass::IsTubeCell (0x00484AB0) also tests land; the record alone
    /// is sufficient only for GetTubeAtCell (0x00484F20).
    pub(crate) fn is_tube_cell(&self, cell: NativeCellIdentity) -> bool {
        self.land_type(cell) == i32::from(LandType::Tunnel.as_index())
            && self.terrain().tube_for_native_cell(cell).is_some()
    }

    /// CellClass::IsNearTubeNW (0x00484AE0). A run of mouths ends at the
    /// first or second cell north or west of the receiver.
    pub(crate) fn is_near_tube_northwest(&self, cell: NativeCellIdentity) -> bool {
        if self.is_tube_cell(cell) {
            return true;
        }
        for direction in [0, 6] {
            let first = self.neighbor(cell, direction);
            let second = self.neighbor(first, direction);
            if self.is_tube_cell(first) && !self.is_tube_cell(second) {
                return true;
            }
            let third = self.neighbor(second, direction);
            if self.is_tube_cell(second) && !self.is_tube_cell(third) {
                return true;
            }
        }
        false
    }

    /// CellClass::IsAtTubeEntrance (0x00484D60), the JumpJet infantry
    /// What_Action check at 0x0051F89D. The side cells branch from southeast.
    pub(crate) fn is_at_tube_entrance(&self, cell: NativeCellIdentity) -> bool {
        if self.is_tube_cell(cell) {
            return true;
        }
        let southeast = self.neighbor(cell, 3);
        let diagonal = self.neighbor(southeast, 3);
        if self.is_tube_cell(southeast) || self.is_tube_cell(diagonal) {
            return true;
        }
        let south = self.neighbor(southeast, 4);
        let east = self.neighbor(southeast, 2);
        self.is_tube_cell(south) || self.is_tube_cell(east)
    }
}

pub(super) const AUTO_TUBE_DIRECTIONS: [u8; 4] = [2, 4, 6, 0];

pub(super) fn build_auto_low_bridge_tubes(
    cells: &mut [ResolvedTerrainCell],
    width: u16,
    height: u16,
    theater_data: Option<&TheaterData>,
    tubes: &mut Vec<TubeFact>,
) {
    for cell in cells.iter_mut() {
        if cell.land_type != LandType::Tunnel.as_index() || cell.tube_index.is_some() {
            continue;
        }
        let Some(direction) = auto_tube_direction_for_tile(cell.final_tile_index, theater_data)
        else {
            continue;
        };
        let Some(_idx) = (cell.rx < width && cell.ry < height).then_some(()) else {
            continue;
        };
        let Ok(raw_id) = u16::try_from(tubes.len()) else {
            log::warn!(
                "ResolvedTerrain: tube registry exceeded u16::MAX; skipping tube at ({}, {})",
                cell.rx,
                cell.ry
            );
            continue;
        };
        let tube_id = TubeId(raw_id);
        tubes.push(TubeFact::automatic_recalc_shell(
            (cell.rx, cell.ry),
            direction,
        ));
        cell.tube_index = Some(tube_id);
    }
}

pub(super) fn auto_tube_direction_for_tile(
    final_tile_index: i32,
    theater_data: Option<&TheaterData>,
) -> Option<u8> {
    let td = theater_data?;
    auto_tube_direction_from_bases(final_tile_index, td.automatic_tube_bases)
}

pub(super) fn auto_tube_direction_from_bases(final_tile_index: i32, bases: [i32; 4]) -> Option<u8> {
    for base in bases {
        if final_tile_index >= base && final_tile_index <= base.wrapping_add(3) {
            let ordinal = final_tile_index.wrapping_sub(base);
            if ordinal != -1 {
                return usize::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| AUTO_TUBE_DIRECTIONS.get(ordinal).copied());
            }
        }
    }
    None
}
