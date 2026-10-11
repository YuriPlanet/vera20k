//! Native `MapClass::Get_CellClass` identity: the process-global fallback
//! `CellClass` (shared dummy), the borrowed query that stamps it on misses,
//! and the grid's fixed-index cell reads and writes.

use super::bridge_flags::refresh_runtime_bridge_projection;
use super::*;

/// Live contents of MapClass's one process-global fallback `CellClass`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SharedCellDummySnapshot {
    pub coord: (i32, i32),
    pub level: i8,
    pub slope_type: u8,
    /// Exact modeled subset of `CellClass+0x140`; always masked to `0x1180`.
    pub bridge_flags_0x1180: u32,
}

/// One `MapClass::Get_CellClass` result as consumed by FNPC's isometric
/// projection helper.
///
/// Keeping the signed level and raw bridge subset in one value prevents callers
/// from accidentally performing two dummy-stamping lookups for one native probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellClassProjectionView {
    pub signed_level: i32,
    pub raw_flags_0x1180: u32,
}

/// Send-safe identity handle for MapClass's process-global fallback `CellClass`.
///
/// Active YR owns one object at `0x00ABDC50`. `MapClass::Get_CellClass` at
/// `0x005657A0` and `0x00565730` overwrite only its packed coordinate words at
/// `+0x24`; the independently writable level/slope bytes at `+0x11B/+0x11C`
/// survive those misses. Coordinate/level/slope, overlay identity/state and
/// smudge identity/data each occupy an atomic word; the full flag word has its
/// own authority. Their shared allocation keeps one live identity safe to carry
/// through app loading workers without copying native's global mutable object
/// architecture.
#[derive(Debug, Clone)]
pub struct SharedCellDummy {
    state: Arc<SharedCellDummyState>,
}

#[derive(Debug)]
struct SharedCellDummyState {
    cell: AtomicU64,
    /// CellClass+0xEC; ctor47BC93 sets Clear0, ordinary misses retain it.
    land_type: AtomicI32,
    /// Complete CellClass+0x140, including the setter's bit0x10000.
    raw_flags: AtomicU32,
    /// CellClass+0x2C: 0=null, 1=shared dummy, real storage index+2 otherwise.
    native_anchor: AtomicU64,
    /// Low dword is signed Cell+0x44 identity; bits 32..39 are Cell+0x11E.
    overlay: AtomicU64,
    /// Low dword is signed Cell+0x48 SmudgeTypeIndex; bits 32..39 are
    /// Cell+0x11F SmudgeData, as `SmudgeTypeClass::Place @ 0x006B6080`
    /// (`SmudgeGrid::write_footprint`) leaves them.
    smudge: AtomicU64,
    /// CellClass+0x116, written even when ReadTubesINI resolves a dummy cell.
    tube_index: AtomicI16,
    /// Retained wrapping CellClass+122; ctor47BD34 initializes zero.
    neighbor_count: AtomicU8,
}

/// One native map-query identity domain. Bulk terrain is borrowed; input
/// producers fork only the small fallback CellClass and retain it across every
/// nested miss. Cloning ResolvedTerrainGrid would share its canonical Dummy.
#[derive(Debug)]
pub struct NativeCellQuery<'a> {
    terrain: &'a ResolvedTerrainGrid,
    dummy: SharedCellDummy,
}

impl<'a> NativeCellQuery<'a> {
    /// Simulation queries retain the map's one live fallback identity. The
    /// same nested query APIs also serve isolated ordinary input producers.
    pub(crate) fn canonical(terrain: &'a ResolvedTerrainGrid) -> Self {
        Self {
            terrain,
            dummy: terrain.shared_cell_dummy.clone(),
        }
    }

    pub(crate) fn isolated(terrain: &'a ResolvedTerrainGrid) -> Self {
        Self {
            terrain,
            dummy: terrain.shared_cell_dummy.fork_query_identity(),
        }
    }

    pub(crate) fn terrain(&self) -> &'a ResolvedTerrainGrid {
        self.terrain
    }
    pub(crate) fn dummy(&self) -> SharedCellDummy {
        self.dummy.clone()
    }

    pub(crate) fn lookup(&self, coord: (i16, i16)) -> NativeCellIdentity {
        self.terrain
            .native_fixed_cell_index(coord.0, coord.1)
            .map_or_else(
                || {
                    self.dummy
                        .stamp_coord(i32::from(coord.0), i32::from(coord.1));
                    NativeCellIdentity::Dummy
                },
                NativeCellIdentity::Real,
            )
    }

    pub(crate) fn lookup_world(&self, x: i32, y: i32) -> NativeCellIdentity {
        let (x, y) = (x / 256, y / 256);
        let index = y.wrapping_mul(512).wrapping_add(x);
        if (0..0x40000).contains(&index)
            && let Some(real) = self
                .terrain
                .native_fixed_cell_index((index % 512) as i16, (index / 512) as i16)
        {
            return NativeCellIdentity::Real(real);
        }
        self.dummy
            .stamp_coord(i32::from(x as i16), i32::from(y as i16));
        NativeCellIdentity::Dummy
    }

    pub(crate) fn coord(&self, cell: NativeCellIdentity) -> (i16, i16) {
        match cell {
            NativeCellIdentity::Real(_) => self.terrain.native_cell_coord(cell),
            NativeCellIdentity::Dummy => {
                let (x, y) = self.dummy.snapshot().coord;
                (x as i16, y as i16)
            }
        }
    }

    pub(crate) fn ground_fields(&self, cell: NativeCellIdentity) -> (u8, u8) {
        match cell {
            NativeCellIdentity::Real(_) => self.terrain.native_cell_ground_fields(cell),
            NativeCellIdentity::Dummy => {
                let state = self.dummy.snapshot();
                (state.level as u8, state.slope_type)
            }
        }
    }

    pub(crate) fn flags(&self, cell: NativeCellIdentity) -> u32 {
        match cell {
            NativeCellIdentity::Real(_) => self.terrain.native_cell_flags(cell),
            NativeCellIdentity::Dummy => self.dummy.raw_flags(),
        }
    }

    /// Retained CellClass+44, without another lookup or dummy replacement.
    pub(crate) fn overlay_identity(&self, cell: NativeCellIdentity) -> i32 {
        match cell {
            NativeCellIdentity::Real(index) => self.terrain.cells[index]
                .bridge_facts
                .overlay_id
                .map_or(-1, i32::from),
            NativeCellIdentity::Dummy => self.dummy.overlay_identity_state().0,
        }
    }

    /// CellClass+EC through an already retained allocation identity. Reading
    /// Land must not perform another lookup or replace the shared Dummy.
    pub(crate) fn land_type(&self, cell: NativeCellIdentity) -> i32 {
        match cell {
            NativeCellIdentity::Real(index) => {
                i32::from(self.terrain.cells[index].yr_cell_land_type)
            }
            NativeCellIdentity::Dummy => self.dummy.land_type(),
        }
    }

    pub(crate) fn projection_view(&self, x: i32, y: i32) -> CellClassProjectionView {
        let cell = self.lookup((x as i16, y as i16));
        CellClassProjectionView {
            signed_level: i32::from(self.ground_fields(cell).0 as i8),
            raw_flags_0x1180: self.flags(cell) & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        }
    }
}

const SHARED_DUMMY_DEFAULT_OVERLAY: u64 = u32::MAX as u64;

/// Constructor `0x0047BC24` (+0x48 = -1) and `0x0047BD22` (+0x11F = 0).
const SHARED_DUMMY_DEFAULT_SMUDGE: u64 = u32::MAX as u64;

impl Default for SharedCellDummy {
    fn default() -> Self {
        Self::fresh()
    }
}

impl SharedCellDummy {
    pub fn fresh() -> Self {
        Self {
            state: Arc::new(SharedCellDummyState {
                cell: AtomicU64::new(0),
                land_type: AtomicI32::new(0),
                raw_flags: AtomicU32::new(0),
                native_anchor: AtomicU64::new(0),
                overlay: AtomicU64::new(SHARED_DUMMY_DEFAULT_OVERLAY),
                smudge: AtomicU64::new(SHARED_DUMMY_DEFAULT_SMUDGE),
                tube_index: AtomicI16::new(-1),
                neighbor_count: AtomicU8::new(0),
            }),
        }
    }

    /// Input queries borrow bulk map state but must not mutate the simulation's
    /// retained fallback identity. Copy every modeled byte, not just the
    /// coordinate/height snapshot, and keep one identity for the whole query.
    pub(crate) fn fork_query_identity(&self) -> Self {
        Self {
            state: Arc::new(SharedCellDummyState {
                cell: AtomicU64::new(self.state.cell.load(Ordering::Relaxed)),
                land_type: AtomicI32::new(self.land_type()),
                raw_flags: AtomicU32::new(self.state.raw_flags.load(Ordering::Relaxed)),
                native_anchor: AtomicU64::new(self.state.native_anchor.load(Ordering::Relaxed)),
                overlay: AtomicU64::new(self.state.overlay.load(Ordering::Relaxed)),
                smudge: AtomicU64::new(self.state.smudge.load(Ordering::Relaxed)),
                tube_index: AtomicI16::new(self.state.tube_index.load(Ordering::Relaxed)),
                neighbor_count: AtomicU8::new(self.neighbor_count()),
            }),
        }
    }

    /// Reconstruct the modeled fields in place at the native Resize boundary.
    ///
    /// `MapClass::Resize @ 0x00565C10` unconditionally calls
    /// `CellClass::Constructor @ 0x0047BBF0` on the fixed dummy object at
    /// `0x00ABDC50` (`0x005670E7..0x005670F2`). The address survives, while
    /// coordinate `+0x24`, level `+0x11B`, slope `+0x11C`, and modeled
    /// `+0x140 & 0x1180` bridge bits return to zero, while overlay identity
    /// returns to signed `-1` and OverlayData to zero. Raw Tube index+0x116
    /// returns to-1 at47BC48 (PHASE3_TUBE_HIERARCHY_20260910.md). SmudgeTypeIndex
    /// +0x48 returns to `-1` (`0x0047BC24`) and SmudgeData +0x11F to zero
    /// (`0x0047BD22`). Other constructor-owned fields are not represented by
    /// this handle yet.
    pub(crate) fn reconstruct_for_map_resize(&self) {
        self.state.cell.store(0, Ordering::Relaxed);
        self.state.land_type.store(0, Ordering::Relaxed);
        // Constructor47BBF0: AND FF800000 at47BCE1; preserve upper residue.
        self.state
            .raw_flags
            .fetch_and(0xff80_0000, Ordering::Relaxed);
        self.state.native_anchor.store(0, Ordering::Relaxed);
        self.state.tube_index.store(-1, Ordering::Relaxed);
        self.state.neighbor_count.store(0, Ordering::Relaxed);
        self.state
            .overlay
            .store(SHARED_DUMMY_DEFAULT_OVERLAY, Ordering::Relaxed);
        self.state
            .smudge
            .store(SHARED_DUMMY_DEFAULT_SMUDGE, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> SharedCellDummySnapshot {
        let packed = self.state.cell.load(Ordering::Relaxed);
        SharedCellDummySnapshot {
            coord: (
                i32::from((packed as u16) as i16),
                i32::from(((packed >> 16) as u16) as i16),
            ),
            level: ((packed >> 32) as u8) as i8,
            slope_type: (packed >> 40) as u8,
            bridge_flags_0x1180: self.raw_flags() & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        }
    }

    pub(crate) fn neighbor_count(&self) -> u8 {
        self.state.neighbor_count.load(Ordering::Relaxed)
    }

    pub(crate) fn land_type(&self) -> i32 {
        self.state.land_type.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn test_set_land_type(&self, land_type: i32) {
        self.state.land_type.store(land_type, Ordering::Relaxed);
    }

    pub(crate) fn adjust_neighbor_count(&self, add: bool) {
        self.state
            .neighbor_count
            .fetch_add(if add { 1 } else { u8::MAX }, Ordering::Relaxed);
    }

    pub(crate) fn raw_tube_index(&self) -> i16 {
        self.state.tube_index.load(Ordering::Relaxed)
    }

    pub(crate) fn write_raw_tube_index(&self, index: i16) {
        self.state.tube_index.store(index, Ordering::Relaxed);
    }

    /// Publish an already validated post-Resize candidate onto the retained
    /// process identity. Preserve its later lookup effects; do not reset twice.
    pub(crate) fn adopt_prepared_load_state(&self, prepared: &Self) {
        self.state
            .land_type
            .store(prepared.land_type(), Ordering::Relaxed);
        self.state.cell.store(
            prepared.state.cell.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.state.overlay.store(
            prepared.state.overlay.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.state.smudge.store(
            prepared.state.smudge.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.state
            .raw_flags
            .store(prepared.raw_flags(), Ordering::Relaxed);
        self.write_native_anchor(prepared.native_anchor());
        self.state
            .neighbor_count
            .store(prepared.neighbor_count(), Ordering::Relaxed);
        self.state
            .tube_index
            .store(prepared.raw_tube_index(), Ordering::Relaxed);
    }

    /// Stamp only CellClass+0x24, preserving the live level and slope bytes.
    pub fn stamp_coord(&self, x: i32, y: i32) {
        let coord = u64::from(x as i16 as u16) | (u64::from(y as i16 as u16) << 16);
        let _ = self
            .state
            .cell
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !0xffff_ffff) | coord)
            });
    }

    pub fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    pub(crate) fn raw_flags(&self) -> u32 {
        self.state.raw_flags.load(Ordering::Relaxed)
    }

    pub(crate) fn write_raw_flags(&self, flags: u32) {
        self.state.raw_flags.store(flags, Ordering::Relaxed);
    }

    pub(crate) fn native_anchor(&self) -> Option<NativeCellIdentity> {
        match self.state.native_anchor.load(Ordering::Relaxed) {
            0 => None,
            1 => Some(NativeCellIdentity::Dummy),
            value => Some(NativeCellIdentity::Real((value - 2) as usize)),
        }
    }

    pub(crate) fn write_native_anchor(&self, anchor: Option<NativeCellIdentity>) {
        let encoded = match anchor {
            None => 0,
            Some(NativeCellIdentity::Dummy) => 1,
            Some(NativeCellIdentity::Real(index)) => index as u64 + 2,
        };
        self.state.native_anchor.store(encoded, Ordering::Relaxed);
    }

    /// Read the native signed overlay identity dword and independent state byte.
    pub(crate) fn overlay_identity_state(&self) -> (i32, u8) {
        let packed = self.state.overlay.load(Ordering::Relaxed);
        (packed as u32 as i32, (packed >> 32) as u8)
    }

    pub(crate) fn write_overlay_identity(&self, identity: i32) {
        let identity = u64::from(identity as u32);
        let _ = self
            .state
            .overlay
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !u64::from(u32::MAX)) | identity)
            });
    }

    pub(crate) fn write_overlay_state(&self, state: u8) {
        const STATE_MASK: u64 = 0xff << 32;
        let state = u64::from(state) << 32;
        let _ = self
            .state
            .overlay
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !STATE_MASK) | state)
            });
    }

    pub(crate) fn write_overlay_identity_state(&self, identity: i32, state: u8) {
        self.state.overlay.store(
            u64::from(identity as u32) | (u64::from(state) << 32),
            Ordering::Relaxed,
        );
    }

    /// The fallback CellClass's signed SmudgeTypeIndex (+0x48) and SmudgeData
    /// (+0x11F), as smudge CanPlace reads them.
    pub(crate) fn smudge_identity_data(&self) -> (i32, u8) {
        let packed = self.state.smudge.load(Ordering::Relaxed);
        (packed as u32 as i32, (packed >> 32) as u8)
    }

    /// `SmudgeTypeClass::Place @ 0x006B6080` on the fallback cell.
    pub(crate) fn write_smudge_identity_data(&self, identity: i32, data: u8) {
        self.state.smudge.store(
            u64::from(identity as u32) | (u64::from(data) << 32),
            Ordering::Relaxed,
        );
    }

    /// Read the fallback CellClass overlay identity/data pair as the runtime
    /// crate/Mark writers see it. `None` is the native signed `-1` sentinel,
    /// so runtime overlay index zero stays distinct from an empty dummy.
    pub(crate) fn overlay_fields(&self) -> (Option<u8>, u8) {
        let (identity, state) = self.overlay_identity_state();
        (u8::try_from(identity).ok(), state)
    }

    /// Write only fallback `CellClass+0x44/+0x11E`, preserving every other
    /// process-global dummy field.
    pub(crate) fn set_overlay_fields(&self, overlay_id: Option<u8>, overlay_data: u8) {
        self.write_overlay_identity_state(overlay_id.map_or(-1, i32::from), overlay_data);
    }

    /// Apply one exact `SetBridgeDirection_*` slot to the modeled flag subset
    /// without disturbing coordinate, level, or slope writers.
    #[cfg(test)]
    pub(crate) fn apply_bridge_flag_slot(&self, slot: BridgeStampSlot, set: bool) {
        let _ =
            self.state
                .raw_flags
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    let mut flags = current;
                    crate::map::bridge_facts::apply_modeled_cellclass_bridge_slot(
                        &mut flags, slot, set,
                    );
                    Some(flags)
                });
    }

    pub(crate) fn bridge_flags_0x1180(&self) -> u32 {
        self.snapshot().bridge_flags_0x1180
    }

    pub(crate) fn retained_bridge_flags(&self) -> u32 {
        self.raw_flags() & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK
    }

    #[cfg(test)]
    pub(crate) fn test_set_retained_bridge_flags(&self, flags: u32) {
        self.update_retained_bridge_flags(|_| flags);
    }

    pub(super) fn update_retained_bridge_flags(&self, update: impl Fn(u32) -> u32) {
        let _ =
            self.state
                .raw_flags
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    let flags = update(current) & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK;
                    Some((current & !RETAINED_CELLCLASS_BRIDGE_FLAG_MASK) | flags)
                });
    }

    pub(crate) fn apply_retained_bridge_flag_slot(
        &self,
        slot: BridgeStampSlot,
        set: bool,
        direction: u8,
    ) {
        self.update_retained_bridge_flags(|mut flags| {
            crate::map::bridge_facts::apply_retained_cellclass_bridge_slot(
                &mut flags, slot, set, direction,
            );
            flags
        });
    }

    pub(super) fn apply_full_bridge_flag_slot(
        &self,
        slot: BridgeStampSlot,
        stamp: BridgeFlagStamp,
        family: BridgeStampFamily,
        anchor: NativeCellIdentity,
    ) {
        let mut facts = BridgeCellFacts {
            raw_flags: self.raw_flags(),
            ..Default::default()
        };
        crate::map::bridge_facts::apply_bridge_fact_slot(
            &mut facts,
            slot,
            BridgeAnchorRelation {
                anchor: (stamp.anchor.0 as u16, stamp.anchor.1 as u16),
                slot,
                family,
                direction: stamp.direction,
            },
            stamp.set,
        );
        self.write_raw_flags(facts.raw_flags);
        if slot.writes_native_anchor() {
            self.write_native_anchor(stamp.set.then_some(anchor));
        }
        if matches!(
            slot,
            BridgeStampSlot::Anchor
                | BridgeStampSlot::Forward1
                | BridgeStampSlot::Forward2
                | BridgeStampSlot::Opposite
        ) {
            self.write_overlay_state(facts.state_byte);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_bridge_flags_0x1180(&self, flags: u32) {
        let _ =
            self.state
                .raw_flags
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    Some(
                        (current & !MODELED_CELLCLASS_BRIDGE_FLAG_MASK)
                            | (flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK),
                    )
                });
    }

    #[cfg(test)]
    pub(crate) fn set_level_slope(&self, level: i8, slope_type: u8) {
        const LEVEL_SLOPE_MASK: u64 = 0xffff << 32;
        let value = (u64::from(level as u8) << 32) | (u64::from(slope_type) << 40);
        let _ = self
            .state
            .cell
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !LEVEL_SLOPE_MASK) | value)
            });
    }

    pub(crate) fn set_level(&self, level: i8) {
        const LEVEL_MASK: u64 = 0xff << 32;
        let value = u64::from(level as u8) << 32;
        let _ = self
            .state
            .cell
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !LEVEL_MASK) | value)
            });
    }
}

pub(super) fn native_resolved_cell_index(
    width: u16,
    height: u16,
    native_allocated: Option<&[bool]>,
    cell_count: usize,
    x: i32,
    y: i32,
) -> Option<usize> {
    let linear = crate::map::cell_index::cell_linear_index(x, y)?;
    let rx = (linear % crate::map::cell_index::CELL_ROW_STRIDE) as usize;
    let ry = (linear / crate::map::cell_index::CELL_ROW_STRIDE) as usize;
    if rx >= usize::from(width) || ry >= usize::from(height) {
        return None;
    }
    let index = ry * usize::from(width) + rx;
    (index < cell_count
        && native_allocated.is_none_or(|mask| mask.get(index).copied().unwrap_or(false)))
    .then_some(index)
}

impl ResolvedTerrainGrid {
    pub(crate) fn shared_cell_dummy(&self) -> SharedCellDummy {
        self.shared_cell_dummy.clone()
    }

    /// Resolve one already-narrowed native fixed-grid coordinate to an
    /// allocated real CellClass slot. The caller owns dummy coordinate stamping
    /// on `None`, because some native probes test admission without GetCell.
    pub(crate) fn native_fixed_cell_index(&self, x: i16, y: i16) -> Option<usize> {
        native_resolved_cell_index(
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            self.cells.len(),
            i32::from(x),
            i32::from(y),
        )
    }

    /// Native5657A0 lookup; retain the returned identity across callbacks.
    pub(crate) fn native_cell_identity(&self, coord: (i16, i16)) -> NativeCellIdentity {
        self.native_fixed_cell_index(coord.0, coord.1).map_or_else(
            || {
                self.shared_cell_dummy
                    .stamp_coord(i32::from(coord.0), i32::from(coord.1));
                NativeCellIdentity::Dummy
            },
            NativeCellIdentity::Real,
        )
    }

    pub(crate) fn native_cell_coord(&self, cell: NativeCellIdentity) -> (i16, i16) {
        match cell {
            NativeCellIdentity::Real(index) => {
                (self.cells[index].rx as i16, self.cells[index].ry as i16)
            }
            NativeCellIdentity::Dummy => {
                let (x, y) = self.shared_cell_dummy.snapshot().coord;
                (x as i16, y as i16)
            }
        }
    }

    pub(crate) fn native_cell_flags(&self, cell: NativeCellIdentity) -> u32 {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.raw_flags,
            NativeCellIdentity::Dummy => self.shared_cell_dummy.raw_flags(),
        }
    }

    /// Read retained CellClass+38 without another Map lookup. The shared
    /// fallback keeps the constructor's 0xFFFF tile, independently of its
    /// mutable coordinate, level, slope and overlay fields.
    pub(crate) fn native_cell_tile_index(&self, cell: NativeCellIdentity) -> i32 {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].final_tile_index,
            NativeCellIdentity::Dummy => 0xFFFF,
        }
    }

    /// Read retained CellClass+11A, the iso sub-tile. Constructor 47BBF0
    /// gives the shared fallback 0; live dummy tile replacement is not
    /// modeled, so no malformed-map parity is claimed.
    pub(crate) fn native_cell_sub_tile(&self, cell: NativeCellIdentity) -> u8 {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].final_sub_tile,
            NativeCellIdentity::Dummy => 0,
        }
    }

    /// Smudge CanPlace's Morphable read (`0x006B601A..0x006B603A`) on one
    /// resolved cell: a real cell's current-tile query; the shared dummy keeps
    /// the constructor's 0xFFFF tile, which reads theater tile 0.
    pub(crate) fn native_cell_accepts_smudge(&self, cell: NativeCellIdentity) -> bool {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].accepts_smudge,
            NativeCellIdentity::Dummy => self.dummy_accepts_smudge,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_set_dummy_accepts_smudge(&mut self, accepts: bool) {
        self.dummy_accepts_smudge = accepts;
    }

    /// Cell47B3A0 samples the receiver's own signed level and slope; unlike
    /// Map578080 it neither resolves a new cell nor restamps the shared Dummy.
    pub(crate) fn native_cell_ground_fields(&self, cell: NativeCellIdentity) -> (u8, u8) {
        match cell {
            NativeCellIdentity::Real(index) => {
                let cell = &self.cells[index];
                (cell.level, cell.slope_type)
            }
            NativeCellIdentity::Dummy => {
                let state = self.shared_cell_dummy.snapshot();
                (state.level as u8, state.slope_type)
            }
        }
    }

    pub(crate) fn native_cell_state(&self, cell: NativeCellIdentity) -> u8 {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.state_byte,
            NativeCellIdentity::Dummy => self.shared_cell_dummy.overlay_identity_state().1,
        }
    }

    pub(crate) fn native_cell_anchor(
        &self,
        cell: NativeCellIdentity,
    ) -> Option<NativeCellIdentity> {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.native_anchor,
            NativeCellIdentity::Dummy => self.shared_cell_dummy.native_anchor(),
        }
    }

    /// Literal scalar writes do not dispatch Recalc, radar or fallout. The
    /// Simulation host publishes its serialized/read projections before effects.
    pub(crate) fn write_native_cell_flags(&mut self, cell: NativeCellIdentity, flags: u32) {
        match cell {
            NativeCellIdentity::Real(index) => {
                let previous = self.cells[index].bridge_facts.raw_flags;
                self.cells[index].bridge_facts.raw_flags = flags;
                // Runtime constructor47E040/47E470 writes the orientation in
                // bit0x800. Keep the derived direction used by span rebuilding
                // current even when this cell has no authored stamp receipt.
                if flags
                    & (BRIDGE_FLAG_ANCHOR_SELF
                        | BRIDGE_FLAG_STRUCTURAL
                        | BRIDGE_FLAG_DESTROYED_OR_RAMP)
                    != 0
                {
                    self.cells[index].bridge_facts.direction =
                        Some(if flags & 0x800 != 0 { 0 } else { 6 });
                }
                // 47E040 removes the deck from stamped non-anchor cells too;
                // those cells need not carry an overlay/bridge_layer. Preserve
                // unrelated ramp projections when F3/extra only change markers.
                let structural_removed =
                    previous & BRIDGE_FLAG_STRUCTURAL != 0 && flags & BRIDGE_FLAG_STRUCTURAL == 0;
                refresh_runtime_bridge_projection(&mut self.cells[index], structural_removed);
                if structural_removed
                    || (previous ^ flags) & crate::map::bridge_facts::BRIDGE_FLAG_TRANSITION != 0
                {
                    self.cells[index].bridge_transition =
                        self.cells[index].bridge_facts.has_transition_flag();
                }
            }
            NativeCellIdentity::Dummy => self.shared_cell_dummy.write_raw_flags(flags),
        }
    }

    pub(crate) fn write_native_cell_state(&mut self, cell: NativeCellIdentity, state: u8) {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.state_byte = state,
            NativeCellIdentity::Dummy => self.shared_cell_dummy.write_overlay_state(state),
        }
    }

    /// Native568E40/569760 raw Cell+11B write, without47D2B0 Recalc.
    /// Evidence: tools/spatial_oracle/bridge_repair.{py,json}. The world owns
    /// subsequent publication to current readers; retained zone inputs stay intact.
    pub(crate) fn write_native_cell_level(&mut self, cell: NativeCellIdentity, level: u8) {
        match cell {
            NativeCellIdentity::Real(index) => {
                self.cells[index].level = level;
                self.cells[index].refresh_current_height_projection();
            }
            NativeCellIdentity::Dummy => self.shared_cell_dummy.set_level(level as i8),
        }
    }

    pub(crate) fn write_native_cell_anchor(
        &mut self,
        cell: NativeCellIdentity,
        anchor: Option<NativeCellIdentity>,
    ) {
        match cell {
            NativeCellIdentity::Real(index) => {
                self.cells[index].bridge_facts.native_anchor = anchor
            }
            NativeCellIdentity::Dummy => self.shared_cell_dummy.write_native_anchor(anchor),
        }
    }

    pub(crate) fn clear_native_cell_overlay(&mut self, cell: NativeCellIdentity) {
        self.write_native_cell_overlay(cell, None);
    }

    /// Raw Cell+44 publication; derived Recalc fields remain untouched until
    /// the caller reaches47D2B0. Ordinary repair stores all three identities first.
    pub(crate) fn write_native_cell_overlay(
        &mut self,
        cell: NativeCellIdentity,
        overlay: Option<u8>,
    ) {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.overlay_id = overlay,
            NativeCellIdentity::Dummy => self
                .shared_cell_dummy
                .write_overlay_identity(overlay.map_or(-1, i32::from)),
        }
    }

    /// Borrow the sparse native CellClass allocation plane used by fixed-grid
    /// map lookups. Load-local owners may snapshot this once so later overlay
    /// writes do not need to keep an immutable borrow of the live terrain grid.
    pub(crate) fn native_allocation_mask(&self) -> Option<&[bool]> {
        self.native_allocated.as_deref()
    }

    pub(crate) fn bind_shared_cell_dummy(&mut self, shared_cell_dummy: SharedCellDummy) {
        self.shared_cell_dummy = shared_cell_dummy;
    }

    /// One stamping lookup for FNPC's level-plus-flags projection probe.
    ///
    /// Native `MapClass::Get_CellClass @ 0x005657A0` returns one real CellClass
    /// or stamps `+0x24` on the process-global dummy before returning that same
    /// live object (`0x005657C8..0x005657D5`). `FUN_006D6410` then reads signed
    /// level `+0x11B` and, conditionally, flags `+0x140` from that one result.
    pub(crate) fn cellclass_projection_view(&self, x: i32, y: i32) -> CellClassProjectionView {
        let (x, y) = crate::map::cell_index::packed_cell_coord(x, y);
        if let Some(index) = native_resolved_cell_index(
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            self.cells.len(),
            x,
            y,
        ) {
            let cell = &self.cells[index];
            return CellClassProjectionView {
                signed_level: i32::from(cell.level as i8),
                raw_flags_0x1180: cell.bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            };
        }
        self.shared_cell_dummy.stamp_coord(x, y);
        let dummy = self.shared_cell_dummy.snapshot();
        CellClassProjectionView {
            signed_level: i32::from(dummy.level),
            raw_flags_0x1180: dummy.bridge_flags_0x1180,
        }
    }

    /// Stamping lookup view for later CellClass flag-only consumers.
    /// A miss updates the dummy coordinate before exposing its live `0x1180`.
    pub(crate) fn cellclass_bridge_flags_0x1180(&self, x: i32, y: i32) -> u32 {
        self.cellclass_projection_view(x, y).raw_flags_0x1180
    }

    pub(crate) fn dummy_cell_level_slope(&self) -> (i8, u8) {
        let snapshot = self.shared_cell_dummy.snapshot();
        (snapshot.level, snapshot.slope_type)
    }

    pub(crate) fn dummy_cell_requested_coord(&self) -> (i32, i32) {
        self.shared_cell_dummy.snapshot().coord
    }

    /// Stamp only the coordinate words of the shared fallback cell.
    ///
    /// Verified against `MapClass::Get_CellClass @ 0x005657A0` and the
    /// world/lepton overload at `0x00565730`: both miss paths overwrite
    /// CellClass+0x24 but preserve the independently mutable level/slope bytes.
    pub(crate) fn stamp_dummy_cell_requested_coord(&self, x: i32, y: i32) {
        self.shared_cell_dummy.stamp_coord(x, y);
    }

    /// Native `MapClass` allocation probe without dummy fallback side effects.
    ///
    /// `MapClass::Is_Cell_Allocated @ 0x005657E0` computes the signed packed
    /// `y*512+x` slot and directly tests its pointer. Its active save-table and
    /// EMP callers establish valid coordinates; Rust therefore returns `false`
    /// outside the fixed array instead of reproducing native out-of-array UB.
    #[cfg(test)]
    pub(crate) fn cellclass_allocation_probe(&self, x: i32, y: i32) -> bool {
        let Some((rx, ry)) = crate::map::cell_index::canonical_cell_coord(x, y) else {
            return false;
        };
        self.cell(rx, ry).is_some()
    }

    /// Update the shared fallback level without exposing an unsupported runtime
    /// slope writer.
    #[cfg(test)]
    pub(crate) fn set_dummy_cell_level(&self, level: i8) {
        self.shared_cell_dummy.set_level(level);
    }

    #[cfg(test)]
    pub(crate) fn test_set_dummy_cell_level_slope(&self, level: i8, slope_type: u8) {
        self.shared_cell_dummy.set_level_slope(level, slope_type);
    }

    #[cfg(test)]
    pub(crate) fn test_set_native_allocated_cells(&mut self, allocated: &[(u16, u16)]) {
        let mut mask = vec![false; usize::from(self.width) * usize::from(self.height)];
        for &(rx, ry) in allocated {
            if rx < self.width && ry < self.height {
                mask[usize::from(ry) * usize::from(self.width) + usize::from(rx)] = true;
            }
        }
        self.native_allocated = Some(mask);
    }

    /// `CellIterator_Init @ 0x00578350` / `Next @ 0x00578290`: advance
    /// anti-diagonals and stop at the first null pointer, without dummy lookup.
    /// The signed Size width is independent of rectangular storage dimensions.
    pub(crate) fn native_cell_iterator(
        &self,
        size_width: i32,
    ) -> impl Iterator<Item = &ResolvedTerrainCell> {
        let (mut x, mut y, mut remaining) = (1i32, size_width, size_width.wrapping_sub(1));
        let mut ended = false;
        std::iter::from_fn(move || {
            if ended {
                return None;
            }
            let index = y.wrapping_mul(512).wrapping_add(x);
            let cell = (0..0x40000)
                .contains(&index)
                .then(|| self.cell((index % 512) as u16, (index / 512) as u16))
                .flatten();
            if remaining != 0 {
                x = x.wrapping_add(1);
                y = y.wrapping_sub(1);
                remaining = remaining.wrapping_sub(1);
            } else {
                (x, y) = (y, x);
                if x.wrapping_sub(size_width).wrapping_sub(1).wrapping_add(y) & 1 == 0 {
                    remaining = size_width.wrapping_sub(2);
                    x = x.wrapping_add(1);
                } else {
                    remaining = size_width.wrapping_sub(1);
                    y = y.wrapping_add(1);
                }
            }
            ended = cell.is_none();
            cell
        })
    }
}
