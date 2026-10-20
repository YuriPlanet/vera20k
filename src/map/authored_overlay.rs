//! Live CellClass overlay identity/state surface for fresh map finalization.
//!
//! The authored reader mutates this surface synchronously in native fixed-grid
//! lookup order. Only after OverlayData, the shared drain, and the first live
//! Recalc sweep does it move a linear payload into the simulation OverlayGrid.

use std::collections::BTreeSet;

use crate::map::bridge_facts::{
    BridgeFlagStamp, BridgeStampFamily, BridgeStampSlot, high_bridge_stamp_for_overlay,
};
use crate::map::map_file::AuthoredOverlayPackReceipt;
use crate::map::overlay_mark::{
    MarkCrateImages, OverlayCellAnim, OverlayMarkHost, OverlayMarkOutcome, OverlayMarkRequest,
    mark_overlay,
};
use crate::map::resolved_terrain::{
    AutomaticTubeAllocation, AutomaticTubeRequest, LoadCellRecalcEffects, LoadCellRecalcError,
    LoadCellRecalcOutcome, LoadCellRecalcState, ResolvedTerrainGrid, SharedCellDummy,
    TerrainTileAnimation,
};
use crate::rules::locomotor_type::SpeedType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::tiberium_type::TiberiumTypeRegistry;

pub(crate) const NO_OVERLAY_IDENTITY: i32 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FinalizedOverlayCell {
    identity: i32,
    state: u8,
}

impl Default for FinalizedOverlayCell {
    fn default() -> Self {
        Self {
            identity: NO_OVERLAY_IDENTITY,
            state: 0,
        }
    }
}

impl FinalizedOverlayCell {
    pub(crate) const fn from_parts(identity: i32, state: u8) -> Self {
        Self { identity, state }
    }

    pub(crate) const fn identity(self) -> i32 {
        self.identity
    }

    pub(crate) const fn overlay_id(self) -> Option<u8> {
        if self.identity >= 0 && self.identity < u8::MAX as i32 {
            Some(self.identity as u8)
        } else {
            None
        }
    }

    pub(crate) const fn state(self) -> u8 {
        self.state
    }
}

/// Consumed-once final identity/state/authored-wall-count authority.
/// Deliberately not `Clone`.
#[derive(Debug)]
pub(crate) struct FinalizedOverlayPayload {
    width: u16,
    height: u16,
    cells: Vec<FinalizedOverlayCell>,
    authored_wall_neighbor_counts: Vec<u8>,
}

impl FinalizedOverlayPayload {
    pub(crate) fn into_parts(self) -> (u16, u16, Vec<FinalizedOverlayCell>, Vec<u8>) {
        (
            self.width,
            self.height,
            self.cells,
            self.authored_wall_neighbor_counts,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn from_cells_for_test(
        width: u16,
        height: u16,
        cells: Vec<(i32, u8)>,
        authored_wall_neighbor_counts: Vec<u8>,
    ) -> Self {
        let expected = usize::from(width) * usize::from(height);
        assert_eq!(cells.len(), expected);
        assert_eq!(authored_wall_neighbor_counts.len(), expected);
        Self {
            width,
            height,
            cells: cells
                .into_iter()
                .map(|(identity, state)| FinalizedOverlayCell { identity, state })
                .collect(),
            authored_wall_neighbor_counts,
        }
    }
}

pub(crate) use crate::map::cell_index::NativeCellIdentity as NativeOverlayCellTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuthoredOverlayCellRef {
    pub(crate) target: NativeOverlayCellTarget,
    pub(crate) coord: (i16, i16),
}

/// The synchronous effects of the load-time wall cleanup in native execution
/// order. Callers must apply each effect before the cleanup advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthoredWallEffect {
    TacticalDirty(AuthoredOverlayCellRef),
    RadarDirty(AuthoredOverlayCellRef),
    CleanupRecalcAndZone(AuthoredOverlayCellRef),
}

/// Raw `[Map] Size` dimensions used by the native radar-diamond predicate.
/// These are deliberately independent of the rectangular storage dimensions
/// of `ResolvedTerrainGrid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeOverlayMapShape {
    width: i32,
    height: i32,
}

impl NativeOverlayMapShape {
    pub(crate) const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }

    /// `MapClass::In_Bounds @ 0x00568300` after the caller has narrowed both
    /// coordinate words to signed 16-bit values.
    pub(crate) const fn admits(self, x: i16, y: i16) -> bool {
        crate::map::playfield::size_diamond_contains(self.width, self.height, (x, y))
    }

    /// Exact real-cell anti-diagonal order used by `Full_Init @ 0x00686B20`
    /// through `CellIterator_Init @ 0x00578350` / `Next @ 0x00578290`:
    /// increasing `x`, decreasing `y` for each sum in
    /// `W+1 ..= W+2H`. Valid fresh maps keep both dimensions in `1..=512`.
    pub(crate) fn recalc_cells(self) -> Vec<(i16, i16)> {
        if !(1..=512).contains(&self.width) || !(1..=512).contains(&self.height) {
            return Vec::new();
        }
        let mut cells =
            Vec::with_capacity(usize::try_from(self.height * (2 * self.width - 1)).unwrap_or(0));
        for sum in self.width + 1..=self.width + 2 * self.height {
            let first_x = (sum - self.width) / 2 + 1;
            let last_x = (sum + self.width - 1) / 2;
            for x in first_x..=last_x {
                cells.push((x as i16, (sum - x) as i16));
            }
        }
        cells
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapLoadDirtyKind {
    BaseMarkTactical,
    WallTactical,
    WallRadar,
}

/// Simulation-owned effects requested synchronously by the map-owned authored
/// reader. The map layer retains all cell/geometry/order authority; this host
/// owns only lifecycle, native identity, RNG continuation, animation, dirty,
/// and zone side effects.
pub(crate) trait AuthoredOverlayLoadHost {
    type Handle: Copy;
    type Error;

    fn try_construct_overlay(
        &mut self,
        overlay_id: u8,
        cell: (u16, u16),
    ) -> Result<Option<Self::Handle>, Self::Error>;

    fn begin_mark(
        &mut self,
        handle: Self::Handle,
        anchor: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error>;

    fn next_scenario_raw(&mut self) -> u32;

    fn allocate_automatic_tube(
        &mut self,
        request: AutomaticTubeRequest,
    ) -> Result<AutomaticTubeAllocation, Self::Error>;

    fn publish_dirty(
        &mut self,
        kind: MapLoadDirtyKind,
        cell: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error>;

    fn construct_terrain_attached_anim(
        &mut self,
        request: &TerrainTileAnimation,
    ) -> Result<(), Self::Error>;

    /// Test/diagnostic observation only. The host receives a copied result and
    /// has no terrain or overlay mutation authority.
    fn observe_recalc(&mut self, _cell: AuthoredOverlayCellRef, _value: FinalizedOverlayCell) {}

    fn merge_wall_zone(&mut self, cell: AuthoredOverlayCellRef) -> Result<(), Self::Error>;

    fn observe_blocker_count_increment(
        &mut self,
        cell: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error>;

    /// `OverlayClass::Mark @ 0x005FD112..0x005FD1F4`'s CellAnim; `terrain`
    /// is the live load-time CellClass array its ground height reads.
    fn spawn_cell_anim(
        &mut self,
        handle: Self::Handle,
        request: OverlayCellAnim<'_>,
        terrain: &ResolvedTerrainGrid,
    ) -> Result<(), Self::Error>;

    fn finish_common(&mut self, handle: Self::Handle) -> Result<(), Self::Error>;

    fn finish_slope_survivor(&mut self, handle: Self::Handle) -> Result<(), Self::Error>;

    fn drain_deferred(&mut self) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub enum AuthoredOverlayFinalizeError<E> {
    InvalidMapShape {
        width: i32,
        height: i32,
    },
    MalformedOverlayType {
        overlay_id: u8,
    },
    ConstructedUnallocatedAnchor {
        overlay_id: u8,
        cell: (u16, u16),
    },
    /// `Is_Clear_To_Build` refused a wall while ScenarioInit was raised.
    WallRefused {
        overlay_id: u8,
        cell: (u16, u16),
    },
    /// Mark reached a callback the load host does not provide; each one is
    /// gated behind a lowered ScenarioInit bracket, which map loading keeps
    /// raised.
    UnhostedMarkCallback {
        callback: &'static str,
    },
    RecalcMalformedOverlayIdentity {
        identity: i32,
    },
    RecalcMissingOverlayType {
        overlay_id: u8,
    },
    RecalcCellIndexOutOfBounds {
        index: usize,
    },
    RecalcResidentInput {
        reason: String,
    },
    Host(E),
}

impl<E: std::fmt::Display> std::fmt::Display for AuthoredOverlayFinalizeError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMapShape { width, height } => {
                write!(f, "invalid authored overlay map shape {width}x{height}")
            }
            Self::MalformedOverlayType { overlay_id } => {
                write!(f, "authored overlay type {overlay_id} is unresolved")
            }
            Self::ConstructedUnallocatedAnchor { overlay_id, cell } => write!(
                f,
                "authored overlay {overlay_id} constructed at unallocated cell ({},{})",
                cell.0, cell.1
            ),
            Self::WallRefused { overlay_id, cell } => write!(
                f,
                "authored wall {overlay_id} refused at ({},{}) during ScenarioInit",
                cell.0, cell.1
            ),
            Self::UnhostedMarkCallback { callback } => {
                write!(f, "authored Mark reached unhosted callback {callback}")
            }
            Self::RecalcMalformedOverlayIdentity { identity } => {
                write!(
                    f,
                    "authored Recalc observed malformed overlay identity {identity}"
                )
            }
            Self::RecalcMissingOverlayType { overlay_id } => {
                write!(
                    f,
                    "authored Recalc cannot resolve overlay type {overlay_id}"
                )
            }
            Self::RecalcCellIndexOutOfBounds { index } => {
                write!(f, "authored Recalc cell index {index} is out of bounds")
            }
            Self::RecalcResidentInput { reason } => {
                write!(
                    f,
                    "authored Recalc received invalid resident inputs: {reason}"
                )
            }
            Self::Host(error) => std::fmt::Display::fmt(error, f),
        }
    }
}

impl<E> std::error::Error for AuthoredOverlayFinalizeError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Host(error) => Some(error),
            _ => None,
        }
    }
}

struct TerrainAnimHostAdapter<'a, H>(&'a mut H);

impl<H: AuthoredOverlayLoadHost> LoadCellRecalcEffects for TerrainAnimHostAdapter<'_, H> {
    type Error = H::Error;

    fn allocate_automatic_tube(
        &mut self,
        request: AutomaticTubeRequest,
    ) -> Result<AutomaticTubeAllocation, Self::Error> {
        self.0.allocate_automatic_tube(request)
    }

    fn construct_terrain_attached_anim(
        &mut self,
        request: &TerrainTileAnimation,
    ) -> Result<(), Self::Error> {
        self.0.construct_terrain_attached_anim(request)
    }
}

/// Execute one cell of the final authored `InitCellAttributes(0)` sweep
/// through the same Simulation-owned effect host used by the first sweep.
/// The caller owns anti-diagonal traversal, the immediately preceding latch
/// clear, and committing the returned identity/state into its live grid.
pub(crate) fn recalc_final_authored_overlay_cell<H: AuthoredOverlayLoadHost>(
    terrain: &mut ResolvedTerrainGrid,
    recalc: &mut LoadCellRecalcState<'_>,
    index: usize,
    current: FinalizedOverlayCell,
    host: &mut H,
) -> Result<FinalizedOverlayCell, AuthoredOverlayFinalizeError<H::Error>> {
    let outcome = {
        let mut effects = TerrainAnimHostAdapter(host);
        terrain.recalc_authored_load_cell(recalc, index, current, &mut effects)
    }
    .map_err(map_recalc_error)?;
    Ok(outcome.finalized)
}

fn map_recalc_error<E>(error: LoadCellRecalcError<E>) -> AuthoredOverlayFinalizeError<E> {
    match error {
        LoadCellRecalcError::MissingResidentInputs => {
            AuthoredOverlayFinalizeError::RecalcResidentInput {
                reason: "missing resident catalog".into(),
            }
        }
        LoadCellRecalcError::ResidentInput(error) => {
            AuthoredOverlayFinalizeError::RecalcResidentInput {
                reason: error.to_string(),
            }
        }
        LoadCellRecalcError::MalformedOverlayIdentity { identity } => {
            AuthoredOverlayFinalizeError::RecalcMalformedOverlayIdentity { identity }
        }
        LoadCellRecalcError::MissingOverlayType { overlay_id } => {
            AuthoredOverlayFinalizeError::RecalcMissingOverlayType { overlay_id }
        }
        LoadCellRecalcError::CellIndexOutOfBounds { index } => {
            AuthoredOverlayFinalizeError::RecalcCellIndexOutOfBounds { index }
        }
        LoadCellRecalcError::Effect(error) => AuthoredOverlayFinalizeError::Host(error),
    }
}

fn recalc_target<H: AuthoredOverlayLoadHost>(
    terrain: &mut ResolvedTerrainGrid,
    recalc: &mut LoadCellRecalcState<'_>,
    cells: &mut LiveOverlayCells,
    host: &mut H,
    cell: AuthoredOverlayCellRef,
) -> Result<Option<LoadCellRecalcOutcome>, AuthoredOverlayFinalizeError<H::Error>> {
    let NativeOverlayCellTarget::Real(index) = cell.target else {
        return Ok(None);
    };
    let current = cells.read(cell.target);
    let outcome = {
        let mut effects = TerrainAnimHostAdapter(host);
        terrain.recalc_authored_load_cell(recalc, index, current, &mut effects)
    }
    .map_err(map_recalc_error)?;
    cells.write(
        cell.target,
        outcome.finalized.identity(),
        outcome.finalized.state(),
    );
    host.observe_recalc(cell, outcome.finalized);
    Ok(Some(outcome))
}

fn mirror_real_overlay_pair(
    terrain: &mut ResolvedTerrainGrid,
    cells: &LiveOverlayCells,
    cell: AuthoredOverlayCellRef,
) {
    let NativeOverlayCellTarget::Real(index) = cell.target else {
        return;
    };
    let value = cells.read(cell.target);
    terrain.mirror_authored_overlay_pair(index, value.overlay_id(), value.state());
}

/// One consuming authored-reader transaction. It owns the exact packed y/x
/// traversal, Mark dispatch, independent data pass, reader drain, and first
/// anti-diagonal Recalc boundary.
enum LoadRecalcOwner<'load, 'resources> {
    Borrowed(&'load mut LoadCellRecalcState<'resources>),
    #[cfg(any(test, feature = "test-support"))]
    Synthetic(Box<LoadCellRecalcState<'resources>>),
}

impl<'load, 'resources> LoadRecalcOwner<'load, 'resources> {
    fn as_mut(&mut self) -> &mut LoadCellRecalcState<'resources> {
        match self {
            Self::Borrowed(recalc) => recalc,
            #[cfg(any(test, feature = "test-support"))]
            Self::Synthetic(recalc) => recalc,
        }
    }
}

pub(crate) struct AuthoredOverlayFinalizer<'load, 'resources, H> {
    terrain: &'load mut ResolvedTerrainGrid,
    recalc: LoadRecalcOwner<'load, 'resources>,
    cells: LiveOverlayCells,
    shape: NativeOverlayMapShape,
    overlay_types: &'resources OverlayTypeRegistry,
    tiberium_types: &'resources TiberiumTypeRegistry,
    overlay_shp_ids: &'resources BTreeSet<u8>,
    signed_new_ini_format: i32,
    game_mode_nonzero: bool,
    host: &'load mut H,
}

impl<'load, 'resources, H: AuthoredOverlayLoadHost> AuthoredOverlayFinalizer<'load, 'resources, H> {
    pub(crate) fn with_recalc(
        terrain: &'load mut ResolvedTerrainGrid,
        recalc: &'load mut LoadCellRecalcState<'resources>,
        shape: NativeOverlayMapShape,
        overlay_types: &'resources OverlayTypeRegistry,
        tiberium_types: &'resources TiberiumTypeRegistry,
        overlay_shp_ids: &'resources BTreeSet<u8>,
        signed_new_ini_format: i32,
        game_mode_nonzero: bool,
        host: &'load mut H,
    ) -> Self {
        let cells = LiveOverlayCells::empty_for_terrain(terrain);
        Self {
            terrain,
            recalc: LoadRecalcOwner::Borrowed(recalc),
            cells,
            shape,
            overlay_types,
            tiberium_types,
            overlay_shp_ids,
            signed_new_ini_format,
            game_mode_nonzero,
            host,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn new(
        terrain: &'load mut ResolvedTerrainGrid,
        shape: NativeOverlayMapShape,
        overlay_types: &'resources OverlayTypeRegistry,
        tiberium_types: &'resources TiberiumTypeRegistry,
        overlay_shp_ids: &'resources BTreeSet<u8>,
        signed_new_ini_format: i32,
        game_mode_nonzero: bool,
        host: &'load mut H,
    ) -> Self {
        let cells = LiveOverlayCells::empty_for_terrain(terrain);
        let recalc = LoadCellRecalcState::synthetic(overlay_types, terrain.cells().len());
        Self {
            terrain,
            recalc: LoadRecalcOwner::Synthetic(Box::new(recalc)),
            cells,
            shape,
            overlay_types,
            tiberium_types,
            overlay_shp_ids,
            signed_new_ini_format,
            game_mode_nonzero,
            host,
        }
    }

    pub(crate) fn run(
        mut self,
        packs: AuthoredOverlayPackReceipt,
    ) -> Result<FinalizedOverlayPayload, AuthoredOverlayFinalizeError<H::Error>> {
        // gamemd-derived: ReadMapOverlayPacks @ 0x005FD2E0, followed by the
        // Full_Init first Recalc boundary @ 0x00687A3E..0x00687A6B.
        if !(1..=512).contains(&self.shape.width) || !(1..=512).contains(&self.shape.height) {
            return Err(AuthoredOverlayFinalizeError::InvalidMapShape {
                width: self.shape.width,
                height: self.shape.height,
            });
        }
        let (identity, data) = packs.into_parts();
        if self.signed_new_ini_format > 1 && identity.has_positive_length() {
            for y in 0..512u16 {
                for x in 0..512u16 {
                    let Some(overlay_id) = identity.read_byte(x, y) else {
                        continue;
                    };
                    if overlay_id == u8::MAX {
                        continue;
                    }
                    self.run_identity_row(x, y, overlay_id)?;
                }
            }
        }

        if self.signed_new_ini_format > 1 && data.has_positive_length() {
            for y in 0..512u16 {
                for x in 0..512u16 {
                    let cell = (x as i16, y as i16);
                    if !self.shape.admits(cell.0, cell.1) {
                        continue;
                    }
                    let value = data.read_byte(x, y).unwrap_or(0);
                    let target = self.cells.target(cell.0, cell.1);
                    if let NativeOverlayCellTarget::Real(index) = target {
                        self.cells.write_state(target, value);
                        mirror_real_overlay_pair(
                            self.terrain,
                            &self.cells,
                            AuthoredOverlayCellRef {
                                target: NativeOverlayCellTarget::Real(index),
                                coord: cell,
                            },
                        );
                    }
                }
            }
        }

        self.host
            .drain_deferred()
            .map_err(AuthoredOverlayFinalizeError::Host)?;
        for coord in self.shape.recalc_cells() {
            let cell = self.cells.cell_ref(coord.0, coord.1);
            self.recalc_cell(cell)?;
        }
        Ok(self.cells.finish())
    }

    fn run_identity_row(
        &mut self,
        x: u16,
        y: u16,
        overlay_id: u8,
    ) -> Result<(), AuthoredOverlayFinalizeError<H::Error>> {
        // gamemd-derived: ReadMapOverlayPacks row corridor @
        // 0x005FD36D..0x005FD55D -> OverlayClass::Mark @ 0x005FC570.
        let overlay_types = self.overlay_types;
        let flags = overlay_types
            .flags(overlay_id)
            .ok_or(AuthoredOverlayFinalizeError::MalformedOverlayType { overlay_id })?;
        if !self.overlay_shp_ids.contains(&overlay_id) && flags.cell_anim.is_none() {
            return Ok(());
        }
        if self.game_mode_nonzero && flags.crate_type {
            return Ok(());
        }
        let packed = (x as i16, y as i16);
        if !self.shape.admits(packed.0, packed.1) {
            return Ok(());
        }

        let anchor = self.cells.cell_ref(packed.0, packed.1);
        // 0x005FD4A4/0x005FD502: the reader saves the anchor's data byte
        // around a high-bridge Mark and restores it afterwards.
        let saved_high_state = high_bridge_stamp_for_overlay(overlay_id)
            .map(|_| self.cells.read(anchor.target).state());
        let Some(handle) = self
            .host
            .try_construct_overlay(overlay_id, (x, y))
            .map_err(AuthoredOverlayFinalizeError::Host)?
        else {
            self.restore_high_anchor_state(packed, saved_high_state);
            return Ok(());
        };
        let NativeOverlayCellTarget::Real(_) = anchor.target else {
            return Err(AuthoredOverlayFinalizeError::ConstructedUnallocatedAnchor {
                overlay_id,
                cell: (x, y),
            });
        };

        self.host
            .begin_mark(handle, anchor)
            .map_err(AuthoredOverlayFinalizeError::Host)?;
        self.host
            .publish_dirty(MapLoadDirtyKind::BaseMarkTactical, anchor)
            .map_err(AuthoredOverlayFinalizeError::Host)?;
        // The constructor (0x005FC380) centres Location on the cell with Z 0.
        // ScenarioInit admits before the crate-image comparisons.
        let request = OverlayMarkRequest {
            overlay_id,
            flags,
            cell: packed,
            location: [
                i32::from(x).wrapping_mul(256).wrapping_add(128),
                i32::from(y).wrapping_mul(256).wrapping_add(128),
                0,
            ],
            registry: overlay_types,
            tiberium_types: Some(self.tiberium_types),
            crate_images: MarkCrateImages::default(),
        };
        let mut mark = FinalizerMarkHost {
            terrain: &mut *self.terrain,
            recalc: self.recalc.as_mut(),
            cells: &mut self.cells,
            shape: self.shape,
            overlay_types,
            host: &mut *self.host,
            handle,
        };
        match mark_overlay(&mut mark, &request)? {
            OverlayMarkOutcome::SlopeRefused => self
                .host
                .finish_slope_survivor(handle)
                .map_err(AuthoredOverlayFinalizeError::Host)?,
            OverlayMarkOutcome::WallRefused => {
                return Err(AuthoredOverlayFinalizeError::WallRefused {
                    overlay_id,
                    cell: (x, y),
                });
            }
            OverlayMarkOutcome::Marked => self
                .host
                .finish_common(handle)
                .map_err(AuthoredOverlayFinalizeError::Host)?,
        }
        self.restore_high_anchor_state(packed, saved_high_state);
        Ok(())
    }

    fn restore_high_anchor_state(&mut self, packed: (i16, i16), state: Option<u8>) {
        let Some(state) = state else {
            return;
        };
        let target = self.cells.target(packed.0, packed.1);
        self.cells.write_state(target, state);
        if let NativeOverlayCellTarget::Real(index) = target {
            self.terrain.mirror_authored_overlay_state(index, state);
        }
    }

    fn recalc_cell(
        &mut self,
        cell: AuthoredOverlayCellRef,
    ) -> Result<(), AuthoredOverlayFinalizeError<H::Error>> {
        recalc_target(
            self.terrain,
            self.recalc.as_mut(),
            &mut self.cells,
            self.host,
            cell,
        )
        .map(|_| ())
    }
}

/// The load-local CellClass surface as the host of `OverlayClass::Mark`. The
/// map reader keeps ScenarioInit raised, so the passability, Overrides and
/// zone-merge branches never run here. Identity and data stores reach the
/// terrain's bridge mirror as they happen; Recalc is the load Recalc.
struct FinalizerMarkHost<'a, 'resources, H: AuthoredOverlayLoadHost> {
    terrain: &'a mut ResolvedTerrainGrid,
    recalc: &'a mut LoadCellRecalcState<'resources>,
    cells: &'a mut LiveOverlayCells,
    shape: NativeOverlayMapShape,
    overlay_types: &'resources OverlayTypeRegistry,
    host: &'a mut H,
    handle: H::Handle,
}

impl<H: AuthoredOverlayLoadHost> OverlayMarkHost for FinalizerMarkHost<'_, '_, H> {
    type Error = AuthoredOverlayFinalizeError<H::Error>;

    fn cell(&mut self, coord: (i16, i16)) -> NativeOverlayCellTarget {
        self.cells.target(coord.0, coord.1)
    }

    fn coord(&self, cell: NativeOverlayCellTarget) -> (i16, i16) {
        self.cells.coord_of(cell)
    }

    fn in_bounds(&mut self, coord: (i16, i16)) -> Result<bool, Self::Error> {
        Ok(self.shape.admits(coord.0, coord.1))
    }

    fn slope(&self, cell: NativeOverlayCellTarget) -> u8 {
        self.terrain.native_cell_ground_fields(cell).1
    }

    fn identity(&self, cell: NativeOverlayCellTarget) -> i32 {
        self.cells.read(cell).identity()
    }

    fn data(&self, cell: NativeOverlayCellTarget) -> u8 {
        self.cells.read(cell).state()
    }

    fn write_identity(
        &mut self,
        cell: NativeOverlayCellTarget,
        overlay_id: u8,
    ) -> Result<(), Self::Error> {
        self.cells.write_identity(cell, i32::from(overlay_id));
        if let NativeOverlayCellTarget::Real(index) = cell {
            self.terrain
                .mirror_authored_overlay_identity(index, Some(overlay_id));
        }
        Ok(())
    }

    fn write_data(&mut self, cell: NativeOverlayCellTarget, data: u8) -> Result<(), Self::Error> {
        self.cells.write_state(cell, data);
        if let NativeOverlayCellTarget::Real(index) = cell {
            self.terrain.mirror_authored_overlay_state(index, data);
        }
        Ok(())
    }

    fn scenario_init(&self) -> bool {
        true
    }

    fn next_scenario_raw(&mut self) -> u32 {
        self.host.next_scenario_raw()
    }

    fn passable(
        &mut self,
        _cell: NativeOverlayCellTarget,
        _speed: SpeedType,
        _ignore_occupants: bool,
    ) -> Result<bool, Self::Error> {
        Err(AuthoredOverlayFinalizeError::UnhostedMarkCallback {
            callback: "CellClass::CheckCellPassability",
        })
    }

    fn set_bridge_direction(
        &mut self,
        anchor: NativeOverlayCellTarget,
        family: BridgeStampFamily,
        direction: u8,
    ) -> Result<(), Self::Error> {
        // gamemd-derived: CellClass::SetBridgeDirection_NESW @ 0x0047E040 and
        // SetBridgeDirection_NWSE @ 0x0047E470 with (direction, 1).
        let coord = self.cells.coord_of(anchor);
        let stamp = BridgeFlagStamp::new((coord.0 as u16, coord.1 as u16), direction, true);
        let Some(slots) = stamp.slots() else {
            return Ok(());
        };
        for (slot, requested) in slots {
            let target = self
                .terrain
                .apply_authored_bridge_flag_slot(anchor, stamp, family, slot, requested)
                .map_or(
                    NativeOverlayCellTarget::Dummy,
                    NativeOverlayCellTarget::Real,
                );
            if matches!(
                slot,
                BridgeStampSlot::Anchor
                    | BridgeStampSlot::Forward1
                    | BridgeStampSlot::Forward2
                    | BridgeStampSlot::Opposite
            ) {
                self.write_data(target, if direction == 0 { 0 } else { 9 })?;
            }
        }
        Ok(())
    }

    fn wall_clear_to_build(
        &mut self,
        _anchor: NativeOverlayCellTarget,
    ) -> Result<bool, Self::Error> {
        // CellClass::Is_Clear_To_Build @ 0x0047C632 returns true while the
        // ScenarioInit bracket is raised.
        Ok(true)
    }

    fn wall_cleanup(&mut self, anchor: NativeOverlayCellTarget) -> Result<(), Self::Error> {
        let anchor = self.cells.coord_of(anchor);
        let (terrain, recalc, host) = (&mut *self.terrain, &mut *self.recalc, &mut *self.host);
        self.cells
            .post_wall_cleanup(self.overlay_types, anchor, |cells, effect| match effect {
                AuthoredWallEffect::TacticalDirty(cell) => {
                    mirror_real_overlay_pair(terrain, cells, cell);
                    host.publish_dirty(MapLoadDirtyKind::WallTactical, cell)
                        .map_err(AuthoredOverlayFinalizeError::Host)
                }
                AuthoredWallEffect::RadarDirty(cell) => host
                    .publish_dirty(MapLoadDirtyKind::WallRadar, cell)
                    .map_err(AuthoredOverlayFinalizeError::Host),
                AuthoredWallEffect::CleanupRecalcAndZone(cell) => {
                    let outcome = recalc_target(terrain, recalc, cells, host, cell)?;
                    if outcome.is_some_and(|outcome| outcome.zone_before != outcome.zone_after) {
                        host.merge_wall_zone(cell)
                            .map_err(AuthoredOverlayFinalizeError::Host)?;
                    }
                    Ok(())
                }
            })
    }

    fn merge_wall_zones(&mut self, _anchor: NativeOverlayCellTarget) -> Result<(), Self::Error> {
        Err(AuthoredOverlayFinalizeError::UnhostedMarkCallback {
            callback: "MapClass::MergeAdjacentCellZone",
        })
    }

    fn increment_wall_neighbor_count(
        &mut self,
        cell: NativeOverlayCellTarget,
    ) -> Result<(), Self::Error> {
        self.cells.increment_authored_wall_count(cell);
        let cell = self.cells.cell_ref_of(cell);
        self.host
            .observe_blocker_count_increment(cell)
            .map_err(AuthoredOverlayFinalizeError::Host)
    }

    fn spawn_cell_anim(&mut self, request: OverlayCellAnim<'_>) -> Result<(), Self::Error> {
        self.host
            .spawn_cell_anim(self.handle, request, self.terrain)
            .map_err(AuthoredOverlayFinalizeError::Host)
    }

    fn recalc(&mut self, cell: NativeOverlayCellTarget) -> Result<(), Self::Error> {
        let cell = self.cells.cell_ref_of(cell);
        recalc_target(self.terrain, self.recalc, self.cells, self.host, cell).map(|_| ())
    }
}

/// Mutable, load-local native overlay cell surface. It cannot escape except by
/// moving its real-cell values into `FinalizedOverlayPayload`.
#[derive(Debug)]
pub(crate) struct LiveOverlayCells {
    width: u16,
    height: u16,
    native_allocated: Option<Vec<bool>>,
    cells: Vec<FinalizedOverlayCell>,
    /// Exact real-cell `CellClass+0x122` contribution made by authored walls.
    /// True-dummy increments are output-inert and deliberately not exported.
    authored_wall_neighbor_counts: Vec<u8>,
    shared_dummy: SharedCellDummy,
}

impl LiveOverlayCells {
    pub(crate) fn empty_for_terrain(terrain: &ResolvedTerrainGrid) -> Self {
        let width = terrain.width();
        let height = terrain.height();
        Self {
            width,
            height,
            native_allocated: terrain.native_allocation_mask().map(<[bool]>::to_vec),
            cells: vec![FinalizedOverlayCell::default(); usize::from(width) * usize::from(height)],
            authored_wall_neighbor_counts: vec![0; usize::from(width) * usize::from(height)],
            shared_dummy: terrain.shared_cell_dummy(),
        }
    }

    /// `MapClass::Get_CellClass` narrows both operands to signed words before
    /// sign-extending `y * 512 + x`. A true miss stamps only dummy coordinates.
    pub(crate) fn target(&self, x: i16, y: i16) -> NativeOverlayCellTarget {
        let real = crate::map::cell_index::cell_linear_index(i32::from(x), i32::from(y)).and_then(
            |linear| {
                let rx = (linear % crate::map::cell_index::CELL_ROW_STRIDE) as usize;
                let ry = (linear / crate::map::cell_index::CELL_ROW_STRIDE) as usize;
                if rx >= usize::from(self.width) || ry >= usize::from(self.height) {
                    return None;
                }
                let index = ry * usize::from(self.width) + rx;
                (index < self.cells.len()
                    && self
                        .native_allocated
                        .as_deref()
                        .is_none_or(|mask| mask.get(index).copied().unwrap_or(false)))
                .then_some(index)
            },
        );
        if let Some(index) = real {
            return NativeOverlayCellTarget::Real(index);
        }
        self.shared_dummy.stamp_coord(i32::from(x), i32::from(y));
        NativeOverlayCellTarget::Dummy
    }

    pub(crate) fn read(&self, target: NativeOverlayCellTarget) -> FinalizedOverlayCell {
        match target {
            NativeOverlayCellTarget::Real(index) => self.cells[index],
            NativeOverlayCellTarget::Dummy => {
                let (identity, state) = self.shared_dummy.overlay_identity_state();
                FinalizedOverlayCell { identity, state }
            }
        }
    }

    pub(crate) fn write_identity(&mut self, target: NativeOverlayCellTarget, identity: i32) {
        match target {
            NativeOverlayCellTarget::Real(index) => self.cells[index].identity = identity,
            NativeOverlayCellTarget::Dummy => {
                self.shared_dummy.write_overlay_identity(identity);
            }
        }
    }

    pub(crate) fn write_state(&mut self, target: NativeOverlayCellTarget, state: u8) {
        match target {
            NativeOverlayCellTarget::Real(index) => self.cells[index].state = state,
            NativeOverlayCellTarget::Dummy => self.shared_dummy.write_overlay_state(state),
        }
    }

    pub(crate) fn write(&mut self, target: NativeOverlayCellTarget, identity: i32, state: u8) {
        match target {
            NativeOverlayCellTarget::Real(index) => {
                self.cells[index] = FinalizedOverlayCell { identity, state };
            }
            NativeOverlayCellTarget::Dummy => {
                self.shared_dummy
                    .write_overlay_identity_state(identity, state);
            }
        }
    }

    fn cell_ref(&self, x: i16, y: i16) -> AuthoredOverlayCellRef {
        let target = self.target(x, y);
        let coord = self.coord_for_target(target, (x, y));
        AuthoredOverlayCellRef { target, coord }
    }

    fn coord_for_target(
        &self,
        target: NativeOverlayCellTarget,
        dummy_fallback: (i16, i16),
    ) -> (i16, i16) {
        match target {
            NativeOverlayCellTarget::Real(index) => (
                (index % usize::from(self.width)) as i16,
                (index / usize::from(self.width)) as i16,
            ),
            NativeOverlayCellTarget::Dummy => {
                let coord = self.shared_dummy.snapshot().coord;
                if coord == (i32::from(dummy_fallback.0), i32::from(dummy_fallback.1)) {
                    dummy_fallback
                } else {
                    (coord.0 as i16, coord.1 as i16)
                }
            }
        }
    }

    /// `CellClass+0x24` of a CellClass target: a real cell's own coordinate,
    /// or the shared dummy's latest stamp.
    fn coord_of(&self, target: NativeOverlayCellTarget) -> (i16, i16) {
        match target {
            NativeOverlayCellTarget::Real(index) => (
                (index % usize::from(self.width)) as i16,
                (index / usize::from(self.width)) as i16,
            ),
            NativeOverlayCellTarget::Dummy => {
                let coord = self.shared_dummy.snapshot().coord;
                (coord.0 as i16, coord.1 as i16)
            }
        }
    }

    fn cell_ref_of(&self, target: NativeOverlayCellTarget) -> AuthoredOverlayCellRef {
        AuthoredOverlayCellRef {
            target,
            coord: self.coord_of(target),
        }
    }

    /// One wrapping `INC byte [CellClass+0x122]` from the wall arm
    /// (`0x005FC762..0x005FC773`). True-dummy increments stay on the dummy.
    fn increment_authored_wall_count(&mut self, target: NativeOverlayCellTarget) {
        if let NativeOverlayCellTarget::Real(index) = target {
            self.authored_wall_neighbor_counts[index] =
                self.authored_wall_neighbor_counts[index].wrapping_add(1);
        } else {
            self.shared_dummy.adjust_neighbor_count(true);
        }
    }

    /// `CellClass::PostDestructionWallCleanup @ 0x00480630` with argument 1,
    /// as the wall arm of `OverlayClass::Mark` calls it while the map reader
    /// keeps ScenarioInit raised: the north, east, south, west and self visits
    /// publish their dirty cells, and every wall among them recomputes its
    /// connectivity nibble and recalculates. Every callback completes before
    /// the cleanup advances to the next effect.
    pub(crate) fn post_wall_cleanup<E>(
        &mut self,
        registry: &OverlayTypeRegistry,
        anchor: (i16, i16),
        mut apply_effect: impl FnMut(&mut LiveOverlayCells, AuthoredWallEffect) -> Result<(), E>,
    ) -> Result<(), E> {
        const CARDINAL: [(i16, i16); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        const CLEANUP_CROSS: [(i16, i16); 5] = [(0, -1), (1, 0), (0, 1), (-1, 0), (0, 0)];

        for (dx, dy) in CLEANUP_CROSS {
            let visit = self.cell_ref(anchor.0.wrapping_add(dx), anchor.1.wrapping_add(dy));
            apply_effect(self, AuthoredWallEffect::TacticalDirty(visit))?;
            apply_effect(self, AuthoredWallEffect::RadarDirty(visit))?;

            let current = self.read(visit.target);
            let Some(current_id) = current.overlay_id() else {
                continue;
            };
            if !registry.flags(current_id).is_some_and(|flags| flags.wall) {
                continue;
            }

            let mut connectivity = 0u8;
            for (bit, (neighbor_dx, neighbor_dy)) in CARDINAL.into_iter().enumerate() {
                // A real CellClass keeps its own packed coordinate. The shared
                // dummy does not: every miss restamps that same object's +0x24,
                // so re-read its coordinate before each native Adjacent_Cell.
                let base = self.coord_for_target(visit.target, visit.coord);
                let neighbor = self.cell_ref(
                    base.0.wrapping_add(neighbor_dx),
                    base.1.wrapping_add(neighbor_dy),
                );
                if current.identity != NO_OVERLAY_IDENTITY
                    && self.read(neighbor.target).identity == current.identity
                {
                    connectivity |= 1 << bit;
                }
            }
            self.write_state(visit.target, (current.state & 0xF0) | connectivity);
            let recalc = AuthoredOverlayCellRef {
                target: visit.target,
                coord: self.coord_for_target(visit.target, visit.coord),
            };
            apply_effect(self, AuthoredWallEffect::CleanupRecalcAndZone(recalc))?;
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> FinalizedOverlayPayload {
        FinalizedOverlayPayload {
            width: self.width,
            height: self.height,
            cells: self.cells,
            authored_wall_neighbor_counts: self.authored_wall_neighbor_counts,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn real_cell(&self, index: usize) -> FinalizedOverlayCell {
        self.cells[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{SharedCellDummy, zone_class};
    use crate::rules::ini_parser::IniFile;
    use std::convert::Infallible;

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(
            width,
            height,
            crate::map::resolved_terrain::test_loader_clear_cell,
        )
    }

    fn wall_registry() -> OverlayTypeRegistry {
        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=WALLA\n1=OTHER\n2=WALLB\n\
             [WALLA]\nWall=yes\n\
             [OTHER]\nIsARock=yes\n\
             [WALLB]\nWall=yes\n",
        );
        OverlayTypeRegistry::from_ini(&ini, None)
    }

    fn registry_with_count(count: usize) -> OverlayTypeRegistry {
        let mut text = String::from("[OverlayTypes]\n");
        for index in 0..count {
            text.push_str(&format!("{index}=TYPE{index}\n"));
        }
        OverlayTypeRegistry::from_ini(&IniFile::from_str(&text), None)
    }

    fn raw_packs(
        identity_rows: &[(u16, u16, u8)],
        data_rows: &[(u16, u16, u8)],
    ) -> AuthoredOverlayPackReceipt {
        let identity_len = identity_rows
            .iter()
            .map(|&(x, y, _)| usize::from(y) * 512 + usize::from(x) + 1)
            .max()
            .unwrap_or(0);
        let data_len = data_rows
            .iter()
            .map(|&(x, y, _)| usize::from(y) * 512 + usize::from(x) + 1)
            .max()
            .unwrap_or(0);
        let mut identity = vec![u8::MAX; identity_len];
        for &(x, y, value) in identity_rows {
            identity[usize::from(y) * 512 + usize::from(x)] = value;
        }
        let mut data = vec![0; data_len];
        for &(x, y, value) in data_rows {
            data[usize::from(y) * 512 + usize::from(x)] = value;
        }
        AuthoredOverlayPackReceipt::from_parts_for_test(
            crate::map::overlay::OverlayIdentityPack::from_decoded(identity),
            crate::map::overlay::OverlayDataPack::from_decoded(data),
        )
    }

    fn tiberium_registries() -> (OverlayTypeRegistry, TiberiumTypeRegistry) {
        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=TIB01\n1=TIB02\n2=LANDONLY\n\
             [TIB01]\nTiberium=yes\nLand=Tiberium\n\
             [TIB02]\nTiberium=yes\nLand=Tiberium\n\
             [LANDONLY]\nLand=Tiberium\n\
             [Tiberiums]\n0=Riparius\n\
             [Riparius]\nImage=1\nMaxDensity=12\n",
        );
        (
            OverlayTypeRegistry::from_ini(&ini, None),
            TiberiumTypeRegistry::from_ini(&ini),
        )
    }

    fn real(index: usize, coord: (i16, i16)) -> AuthoredOverlayCellRef {
        AuthoredOverlayCellRef {
            target: NativeOverlayCellTarget::Real(index),
            coord,
        }
    }

    /// The one Mark port on load-local cells through the production load
    /// host, outside a reader row (handle 0, synthetic Recalc inputs).
    #[allow(clippy::too_many_arguments)]
    fn mark_live(
        terrain: &mut ResolvedTerrainGrid,
        cells: &mut LiveOverlayCells,
        shape: NativeOverlayMapShape,
        overlay_types: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        host: &mut LoadHost,
        coord: (i16, i16),
        overlay_id: u8,
    ) -> Result<OverlayMarkOutcome, AuthoredOverlayFinalizeError<Infallible>> {
        let mut recalc = LoadCellRecalcState::synthetic(overlay_types, terrain.cells().len());
        let request = OverlayMarkRequest {
            overlay_id,
            flags: overlay_types.flags(overlay_id).expect("registered type"),
            cell: coord,
            location: [
                i32::from(coord.0) * 256 + 128,
                i32::from(coord.1) * 256 + 128,
                0,
            ],
            registry: overlay_types,
            tiberium_types: Some(tiberium_types),
            crate_images: MarkCrateImages::default(),
        };
        mark_overlay(
            &mut FinalizerMarkHost {
                terrain,
                recalc: &mut recalc,
                cells,
                shape,
                overlay_types,
                host,
                handle: 0,
            },
            &request,
        )
    }

    fn recalcs(host: &LoadHost) -> Vec<(AuthoredOverlayCellRef, FinalizedOverlayCell)> {
        host.events
            .iter()
            .filter_map(|event| match event {
                LoadEvent::Recalc(cell, value) => Some((*cell, *value)),
                _ => None,
            })
            .collect()
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum LoadEvent {
        Construct(u8, (u16, u16), u32),
        Begin(u32, AuthoredOverlayCellRef),
        Dirty(MapLoadDirtyKind, AuthoredOverlayCellRef),
        Scenario(u32),
        Recalc(AuthoredOverlayCellRef, FinalizedOverlayCell),
        WallZone(AuthoredOverlayCellRef),
        BlockerIncrement(AuthoredOverlayCellRef),
        CellAnim(u32, String, NativeOverlayCellTarget, [i32; 2], Option<u8>),
        FinishCommon(u32),
        FinishSlope(u32),
        Drain,
    }

    #[derive(Default)]
    struct LoadHost {
        next_handle: u32,
        raw: Vec<u32>,
        next_raw: usize,
        null_allocations: BTreeSet<(u16, u16)>,
        drained: bool,
        events: Vec<LoadEvent>,
    }

    impl AuthoredOverlayLoadHost for LoadHost {
        type Handle = u32;
        type Error = Infallible;

        fn try_construct_overlay(
            &mut self,
            overlay_id: u8,
            cell: (u16, u16),
        ) -> Result<Option<Self::Handle>, Self::Error> {
            if self.null_allocations.contains(&cell) {
                return Ok(None);
            }
            self.next_handle += 1;
            let handle = self.next_handle;
            self.events
                .push(LoadEvent::Construct(overlay_id, cell, handle));
            Ok(Some(handle))
        }

        fn begin_mark(
            &mut self,
            handle: Self::Handle,
            anchor: AuthoredOverlayCellRef,
        ) -> Result<(), Self::Error> {
            self.events.push(LoadEvent::Begin(handle, anchor));
            Ok(())
        }

        fn next_scenario_raw(&mut self) -> u32 {
            let raw = self.raw.get(self.next_raw).copied().unwrap_or(0);
            self.next_raw += 1;
            self.events.push(LoadEvent::Scenario(raw));
            raw
        }

        fn allocate_automatic_tube(
            &mut self,
            _request: AutomaticTubeRequest,
        ) -> Result<AutomaticTubeAllocation, Self::Error> {
            Ok(AutomaticTubeAllocation::AllocationNull)
        }

        fn publish_dirty(
            &mut self,
            kind: MapLoadDirtyKind,
            cell: AuthoredOverlayCellRef,
        ) -> Result<(), Self::Error> {
            self.events.push(LoadEvent::Dirty(kind, cell));
            Ok(())
        }

        fn construct_terrain_attached_anim(
            &mut self,
            _request: &TerrainTileAnimation,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn observe_recalc(&mut self, cell: AuthoredOverlayCellRef, value: FinalizedOverlayCell) {
            self.events.push(LoadEvent::Recalc(cell, value));
        }

        fn merge_wall_zone(&mut self, cell: AuthoredOverlayCellRef) -> Result<(), Self::Error> {
            self.events.push(LoadEvent::WallZone(cell));
            Ok(())
        }

        fn observe_blocker_count_increment(
            &mut self,
            cell: AuthoredOverlayCellRef,
        ) -> Result<(), Self::Error> {
            self.events.push(LoadEvent::BlockerIncrement(cell));
            Ok(())
        }

        fn spawn_cell_anim(
            &mut self,
            handle: Self::Handle,
            request: OverlayCellAnim<'_>,
            _terrain: &ResolvedTerrainGrid,
        ) -> Result<(), Self::Error> {
            self.events.push(LoadEvent::CellAnim(
                handle,
                request.anim.to_string(),
                request.cell,
                request.anim_xy,
                request.tiberium.map(|tiberium| tiberium.0),
            ));
            Ok(())
        }

        fn finish_common(&mut self, handle: Self::Handle) -> Result<(), Self::Error> {
            self.events.push(LoadEvent::FinishCommon(handle));
            Ok(())
        }

        fn finish_slope_survivor(&mut self, handle: Self::Handle) -> Result<(), Self::Error> {
            self.events.push(LoadEvent::FinishSlope(handle));
            Ok(())
        }

        fn drain_deferred(&mut self) -> Result<(), Self::Error> {
            self.drained = true;
            self.events.push(LoadEvent::Drain);
            Ok(())
        }
    }

    #[test]
    fn shared_dummy_overlay_pair_persists_across_coordinate_misses_and_resets_on_resize() {
        let dummy = SharedCellDummy::fresh();
        let retained = dummy.clone();
        dummy.write_overlay_identity_state(0x5c, 2);
        dummy.stamp_coord(-510, 2);

        assert!(dummy.same_identity(&retained));
        assert_eq!(retained.overlay_identity_state(), (0x5c, 2));
        assert_eq!(retained.snapshot().coord, (-510, 2));

        dummy.reconstruct_for_map_resize();
        assert_eq!(retained.overlay_identity_state(), (NO_OVERLAY_IDENTITY, 0));
        assert_eq!(retained.snapshot().coord, (0, 0));
    }

    #[test]
    fn native_overlay_shape_keeps_all_four_strict_radar_boundaries() {
        let shape = NativeOverlayMapShape::new(10, 6);
        assert!(!shape.admits(5, 5), "W < x+y is strict");
        assert!(!shape.admits(10, 0), "x-y < W is strict");
        assert!(!shape.admits(0, 10), "y-x < W is strict");
        assert!(shape.admits(11, 11), "upper sum equality is admitted");
        assert!(!shape.admits(12, 11), "upper sum overflow is rejected");
    }

    #[test]
    fn native_overlay_shape_emits_exact_first_sweep_order_and_count() {
        let shape = NativeOverlayMapShape::new(2, 2);
        assert_eq!(
            shape.recalc_cells(),
            vec![(1, 2), (2, 1), (2, 2), (2, 3), (3, 2), (3, 3)]
        );
    }

    #[test]
    fn authored_rows_data_drain_and_first_sweep_form_one_consuming_transaction() {
        let mut terrain = flat_terrain(6, 6);
        let shape = NativeOverlayMapShape::new(2, 2);
        let registry = registry_with_count(1);
        let tiberium_types = TiberiumTypeRegistry::default();
        let shp_ids = BTreeSet::from([0]);
        let packs = raw_packs(&[(2, 1, 0), (1, 2, 0)], &[(2, 1, 7), (1, 2, 9)]);
        let mut host = LoadHost::default();

        let payload = AuthoredOverlayFinalizer::new(
            &mut terrain,
            shape,
            &registry,
            &tiberium_types,
            &shp_ids,
            4,
            false,
            &mut host,
        )
        .run(packs)
        .expect("exact authored transaction");

        let first = real(8, (2, 1));
        let second = real(13, (1, 2));
        assert_eq!(
            &host.events[..10],
            &[
                LoadEvent::Construct(0, (2, 1), 1),
                LoadEvent::Begin(1, first),
                LoadEvent::Dirty(MapLoadDirtyKind::BaseMarkTactical, first),
                LoadEvent::Recalc(
                    first,
                    FinalizedOverlayCell {
                        identity: 0,
                        state: 0,
                    },
                ),
                LoadEvent::FinishCommon(1),
                LoadEvent::Construct(0, (1, 2), 2),
                LoadEvent::Begin(2, second),
                LoadEvent::Dirty(MapLoadDirtyKind::BaseMarkTactical, second),
                LoadEvent::Recalc(
                    second,
                    FinalizedOverlayCell {
                        identity: 0,
                        state: 0,
                    },
                ),
                LoadEvent::FinishCommon(2),
            ]
        );
        assert_eq!(host.events[10], LoadEvent::Drain);
        let sweep = host.events[11..]
            .iter()
            .map(|event| match event {
                LoadEvent::Recalc(cell, value) => (cell.coord, value.overlay_id(), value.state()),
                other => panic!("unexpected first-sweep event: {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            sweep,
            vec![
                ((1, 2), Some(0), 9),
                ((2, 1), Some(0), 7),
                ((2, 2), None, 0),
                ((2, 3), None, 0),
                ((3, 2), None, 0),
                ((3, 3), None, 0),
            ]
        );

        let (_, _, cells, _) = payload.into_parts();
        assert_eq!(cells[8].overlay_id(), Some(0));
        assert_eq!(cells[8].state(), 7);
        assert_eq!(cells[13].overlay_id(), Some(0));
        assert_eq!(cells[13].state(), 9);
    }

    #[test]
    fn format_inactive_still_drains_and_runs_the_first_sweep() {
        let mut terrain = flat_terrain(6, 6);
        let shape = NativeOverlayMapShape::new(2, 2);
        let registry = registry_with_count(1);
        let tiberium_types = TiberiumTypeRegistry::default();
        let shp_ids = BTreeSet::from([0]);
        let packs = raw_packs(&[(2, 1, 0)], &[(2, 1, 7)]);
        let mut host = LoadHost::default();

        let payload = AuthoredOverlayFinalizer::new(
            &mut terrain,
            shape,
            &registry,
            &tiberium_types,
            &shp_ids,
            1,
            false,
            &mut host,
        )
        .run(packs)
        .expect("format-inactive boundaries remain live");

        assert_eq!(host.events.first(), Some(&LoadEvent::Drain));
        assert_eq!(host.events.len(), 1 + shape.recalc_cells().len());
        assert!(host.events[1..].iter().all(|event| matches!(
            event,
            LoadEvent::Recalc(
                _,
                FinalizedOverlayCell {
                    identity: NO_OVERLAY_IDENTITY,
                    state: 0,
                }
            )
        )));
        let (_, _, cells, _) = payload.into_parts();
        assert!(
            cells
                .iter()
                .all(|cell| *cell == FinalizedOverlayCell::default())
        );
    }

    #[test]
    fn malformed_type_allocation_null_and_steep_slope_stop_at_their_exact_boundaries() {
        let shape = NativeOverlayMapShape::new(2, 2);
        let tiberium_types = TiberiumTypeRegistry::default();

        let mut malformed_terrain = flat_terrain(6, 6);
        let malformed_registry = registry_with_count(1);
        let malformed_shp = BTreeSet::from([1]);
        let mut malformed_host = LoadHost::default();
        let malformed = AuthoredOverlayFinalizer::new(
            &mut malformed_terrain,
            shape,
            &malformed_registry,
            &tiberium_types,
            &malformed_shp,
            4,
            false,
            &mut malformed_host,
        )
        .run(raw_packs(&[(0, 0, 1)], &[]));
        assert!(matches!(
            malformed,
            Err(AuthoredOverlayFinalizeError::MalformedOverlayType { overlay_id: 1 })
        ));
        assert!(malformed_host.events.is_empty());

        let mut null_terrain = flat_terrain(6, 6);
        let registry = registry_with_count(1);
        let shp = BTreeSet::from([0]);
        let mut null_host = LoadHost::default();
        null_host.null_allocations.insert((2, 1));
        let null_payload = AuthoredOverlayFinalizer::new(
            &mut null_terrain,
            shape,
            &registry,
            &tiberium_types,
            &shp,
            4,
            false,
            &mut null_host,
        )
        .run(raw_packs(&[(2, 1, 0)], &[]))
        .expect("allocation-null row is skipped");
        assert_eq!(null_host.events.first(), Some(&LoadEvent::Drain));
        assert_eq!(
            null_payload.into_parts().2[8],
            FinalizedOverlayCell::default()
        );

        let mut slope_terrain = flat_terrain(6, 6);
        slope_terrain.cells_mut_for_test()[8].slope_type = 5;
        let mut slope_host = LoadHost::default();
        let slope_payload = AuthoredOverlayFinalizer::new(
            &mut slope_terrain,
            shape,
            &registry,
            &tiberium_types,
            &shp,
            4,
            false,
            &mut slope_host,
        )
        .run(raw_packs(&[(2, 1, 0)], &[]))
        .expect("steep-slope survivor remains registered but unmarked");
        assert_eq!(
            &slope_host.events[..4],
            &[
                LoadEvent::Construct(0, (2, 1), 1),
                LoadEvent::Begin(1, real(8, (2, 1))),
                LoadEvent::Dirty(MapLoadDirtyKind::BaseMarkTactical, real(8, (2, 1))),
                LoadEvent::FinishSlope(1),
            ]
        );
        assert_eq!(slope_host.events[4], LoadEvent::Drain);
        assert_eq!(
            slope_payload.into_parts().2[8],
            FinalizedOverlayCell::default()
        );
    }

    #[test]
    fn slope_exempt_b2_cellanim_and_crate_last_follow_native_row_order() {
        let shape = NativeOverlayMapShape::new(2, 2);
        let tiberium_types = TiberiumTypeRegistry::default();

        let mut b2_terrain = flat_terrain(6, 6);
        b2_terrain.cells_mut_for_test()[8].slope_type = 5;
        let b2_registry = registry_with_count(0xB3);
        let b2_shp = BTreeSet::from([0xB2]);
        let mut b2_host = LoadHost::default();
        let b2_payload = AuthoredOverlayFinalizer::new(
            &mut b2_terrain,
            shape,
            &b2_registry,
            &tiberium_types,
            &b2_shp,
            4,
            false,
            &mut b2_host,
        )
        .run(raw_packs(&[(2, 1, 0xB2)], &[]))
        .expect("0xB2 bypasses the universal steep-slope rejection");
        assert_eq!(b2_payload.into_parts().2[8].overlay_id(), Some(0xB2));
        assert!(matches!(
            b2_host.events[3],
            LoadEvent::Recalc(
                _,
                FinalizedOverlayCell {
                    identity: 0xB2,
                    state: 0
                }
            )
        ));
        assert_eq!(b2_host.events[4], LoadEvent::FinishCommon(1));

        let cell_anim_ini = IniFile::from_str(
            "[Animations]\n0=SPARK\n\
             [OverlayTypes]\n0=ANIMOVER\n\
             [ANIMOVER]\nCellAnim=SPARK\n",
        );
        let cell_anim_registry = OverlayTypeRegistry::from_ini(&cell_anim_ini, None);
        let mut cell_anim_terrain = flat_terrain(6, 6);
        let mut cell_anim_host = LoadHost::default();
        let cell_anim_payload = AuthoredOverlayFinalizer::new(
            &mut cell_anim_terrain,
            shape,
            &cell_anim_registry,
            &tiberium_types,
            &BTreeSet::new(),
            4,
            false,
            &mut cell_anim_host,
        )
        .run(raw_packs(&[(2, 1, 0)], &[]))
        .expect("resolved CellAnim admits an image-less type");
        assert_eq!(
            &cell_anim_host.events[..6],
            &[
                LoadEvent::Construct(0, (2, 1), 1),
                LoadEvent::Begin(1, real(8, (2, 1))),
                LoadEvent::Dirty(MapLoadDirtyKind::BaseMarkTactical, real(8, (2, 1))),
                // 0x005FD171..0x005FD177: Location plus 0x180 on X and Y.
                LoadEvent::CellAnim(
                    1,
                    "SPARK".to_string(),
                    NativeOverlayCellTarget::Real(8),
                    [2 * 256 + 128 + 0x180, 256 + 128 + 0x180],
                    None,
                ),
                LoadEvent::Recalc(
                    real(8, (2, 1)),
                    FinalizedOverlayCell {
                        identity: 0,
                        state: 0
                    },
                ),
                LoadEvent::FinishCommon(1),
            ]
        );
        assert_eq!(cell_anim_payload.into_parts().2[8].overlay_id(), Some(0));

        let crate_ini = IniFile::from_str("[OverlayTypes]\n0=CRATE\n[CRATE]\nCrate=yes\n");
        let crate_registry = OverlayTypeRegistry::from_ini(&crate_ini, None);
        let crate_shp = BTreeSet::from([0]);
        let mut rejected_terrain = flat_terrain(6, 6);
        let mut rejected_host = LoadHost::default();
        let rejected_payload = AuthoredOverlayFinalizer::new(
            &mut rejected_terrain,
            shape,
            &crate_registry,
            &tiberium_types,
            &crate_shp,
            4,
            true,
            &mut rejected_host,
        )
        .run(raw_packs(&[(2, 1, 0)], &[]))
        .expect("nonzero game mode rejects a crate before allocation");
        assert_eq!(rejected_host.events.first(), Some(&LoadEvent::Drain));
        assert_eq!(
            rejected_payload.into_parts().2[8],
            FinalizedOverlayCell::default()
        );

        let mut accepted_terrain = flat_terrain(6, 6);
        let mut accepted_host = LoadHost::default();
        let accepted_payload = AuthoredOverlayFinalizer::new(
            &mut accepted_terrain,
            shape,
            &crate_registry,
            &tiberium_types,
            &crate_shp,
            4,
            false,
            &mut accepted_host,
        )
        .run(raw_packs(&[(2, 1, 0)], &[]))
        .expect("zero game mode accepts the crate");
        assert!(matches!(
            accepted_host.events[3],
            LoadEvent::Recalc(
                _,
                FinalizedOverlayCell {
                    identity: 0,
                    state: u8::MAX
                }
            )
        ));
        assert_eq!(accepted_payload.into_parts().2[8].state(), u8::MAX);
    }

    #[test]
    fn data_is_independent_and_failed_short_reads_store_initialized_zero() {
        let shape = NativeOverlayMapShape::new(2, 2);
        let tiberium_types = TiberiumTypeRegistry::default();

        let mut rejected_identity_terrain = flat_terrain(6, 6);
        let rejected_registry = registry_with_count(1);
        let mut rejected_host = LoadHost::default();
        let rejected_payload = AuthoredOverlayFinalizer::new(
            &mut rejected_identity_terrain,
            shape,
            &rejected_registry,
            &tiberium_types,
            &BTreeSet::new(),
            4,
            false,
            &mut rejected_host,
        )
        .run(raw_packs(&[(2, 1, 0)], &[(2, 1, 7)]))
        .expect("data ignores the identity image gate");
        assert_eq!(rejected_host.events.first(), Some(&LoadEvent::Drain));
        let rejected_cell = rejected_payload.into_parts().2[8];
        assert_eq!(rejected_cell.overlay_id(), None);
        assert_eq!(rejected_cell.state(), 7);

        let mut short_terrain = flat_terrain(6, 6);
        let low_registry = registry_with_count(0x7C);
        let low_shp = BTreeSet::from([0x7B]);
        let mut short_host = LoadHost::default();
        let short_payload = AuthoredOverlayFinalizer::new(
            &mut short_terrain,
            shape,
            &low_registry,
            &tiberium_types,
            &low_shp,
            4,
            false,
            &mut short_host,
        )
        .run(raw_packs(&[(2, 2, 0x7B)], &[(0, 0, 37)]))
        .expect("positive short data body still visits every radar cell");
        let (_, _, short_cells, _) = short_payload.into_parts();
        for index in [8usize, 14, 20] {
            assert_eq!(short_cells[index].state(), 0);
        }
        assert!(short_host.events.iter().any(|event| matches!(
            event,
            LoadEvent::Recalc(
                AuthoredOverlayCellRef { coord: (2, 3), .. },
                FinalizedOverlayCell { state: 2, .. }
            )
        )));
    }

    #[test]
    fn finalizer_germinates_in_yx_order_then_data_wins_and_crate_writes_last() {
        let shape = NativeOverlayMapShape::new(4, 4);
        let (registry, tiberium_types) = tiberium_registries();
        let shp = BTreeSet::from([0]);
        let identity_rows = [(4, 3, 0), (5, 3, 0), (4, 4, 0), (5, 4, 0)];
        let indices = [40usize, 41, 52, 53];

        let mut no_data_terrain = flat_terrain(12, 12);
        let mut no_data_host = LoadHost::default();
        let no_data = AuthoredOverlayFinalizer::new(
            &mut no_data_terrain,
            shape,
            &registry,
            &tiberium_types,
            &shp,
            4,
            false,
            &mut no_data_host,
        )
        .run(raw_packs(&identity_rows, &[]))
        .expect("inline no-data germination");
        let (_, _, no_data_cells, _) = no_data.into_parts();
        assert_eq!(
            indices.map(|index| no_data_cells[index].state()),
            [0, 1, 3, 4]
        );
        let drain = no_data_host
            .events
            .iter()
            .position(|event| *event == LoadEvent::Drain)
            .expect("reader drain");
        let mark_states = no_data_host.events[..drain]
            .iter()
            .filter_map(|event| match event {
                LoadEvent::Recalc(cell, value)
                    if matches!(cell.coord, (4, 3) | (5, 3) | (4, 4) | (5, 4)) =>
                {
                    Some((cell.coord, value.state()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            mark_states,
            vec![((4, 3), 0), ((5, 3), 1), ((4, 4), 3), ((5, 4), 4)]
        );

        let mut data_terrain = flat_terrain(12, 12);
        let mut data_host = LoadHost::default();
        let data_rows = [(4, 3, 9), (5, 3, 8), (4, 4, 7), (5, 4, 6)];
        let with_data = AuthoredOverlayFinalizer::new(
            &mut data_terrain,
            shape,
            &registry,
            &tiberium_types,
            &shp,
            4,
            false,
            &mut data_host,
        )
        .run(raw_packs(&identity_rows, &data_rows))
        .expect("later OverlayData replaces germination bytes");
        let (_, _, data_cells, _) = with_data.into_parts();
        assert_eq!(indices.map(|index| data_cells[index].state()), [9, 8, 7, 6]);

        let crate_ini = IniFile::from_str(
            "[OverlayTypes]\n0=TIBCRATE\n\
             [TIBCRATE]\nTiberium=yes\nLand=Tiberium\nCrate=yes\n\
             [Tiberiums]\n0=Riparius\n\
             [Riparius]\nImage=1\n",
        );
        let crate_registry = OverlayTypeRegistry::from_ini(&crate_ini, None);
        let crate_tiberium = TiberiumTypeRegistry::from_ini(&crate_ini);
        let mut crate_terrain = flat_terrain(12, 12);
        let mut crate_host = LoadHost::default();
        let crate_payload = AuthoredOverlayFinalizer::new(
            &mut crate_terrain,
            shape,
            &crate_registry,
            &crate_tiberium,
            &shp,
            4,
            false,
            &mut crate_host,
        )
        .run(raw_packs(&[(4, 3, 0)], &[]))
        .expect("crate writes after Land-5 germination");
        assert_eq!(crate_payload.into_parts().2[40].state(), u8::MAX);
        assert!(crate_host.events.iter().any(|event| matches!(
            event,
            LoadEvent::Recalc(
                AuthoredOverlayCellRef { coord: (4, 3), .. },
                FinalizedOverlayCell { state: u8::MAX, .. }
            )
        )));
    }

    #[test]
    fn finalizer_low_body_interleaves_raw_scenario_words_and_recalcs_before_common_tail() {
        let mut terrain = flat_terrain(24, 24);
        let shape = NativeOverlayMapShape::new(10, 10);
        let registry = registry_with_count(0x7C);
        let tiberium_types = TiberiumTypeRegistry::default();
        let shp = BTreeSet::from([0x7A, 0x7B]);
        let mut host = LoadHost {
            raw: (0..9).collect(),
            ..LoadHost::default()
        };

        let payload = AuthoredOverlayFinalizer::new(
            &mut terrain,
            shape,
            &registry,
            &tiberium_types,
            &shp,
            4,
            false,
            &mut host,
        )
        .run(raw_packs(&[(8, 8, 0x7B), (12, 8, 0x7A)], &[]))
        .expect("successful low-body finalizer transaction");

        let second = host
            .events
            .iter()
            .position(|event| matches!(event, LoadEvent::Construct(0x7A, (12, 8), 2)))
            .expect("second endpoint constructor");
        let first_scenario = host.events[second..]
            .iter()
            .position(|event| matches!(event, LoadEvent::Scenario(0)))
            .map(|offset| second + offset)
            .expect("first raw body draw");
        for draw in 0..9usize {
            assert_eq!(
                host.events[first_scenario + draw * 2],
                LoadEvent::Scenario(draw as u32)
            );
            assert!(matches!(
                host.events[first_scenario + draw * 2 + 1],
                LoadEvent::Recalc(_, _)
            ));
        }
        let common_recalc = first_scenario + 18;
        assert!(matches!(
            host.events[common_recalc],
            LoadEvent::Recalc(AuthoredOverlayCellRef { coord: (12, 8), .. }, _)
        ));
        assert_eq!(host.events[common_recalc + 1], LoadEvent::FinishCommon(2));
        assert_eq!(host.events[common_recalc + 2], LoadEvent::Drain);

        let (_, _, cells, _) = payload.into_parts();
        for (row, center_x) in [9usize, 10, 11].into_iter().enumerate() {
            for (state, y) in [7usize, 8, 9].into_iter().enumerate() {
                let draw = row * 3 + state;
                let index = y * 24 + center_x;
                assert_eq!(cells[index].overlay_id(), Some(0x4A + (draw as u8 & 3)));
                assert_eq!(cells[index].state(), state as u8);
                assert_eq!(
                    terrain.cells()[index].bridge_facts.overlay_id,
                    cells[index].overlay_id()
                );
                assert_eq!(terrain.cells()[index].bridge_facts.state_byte, state as u8);
            }
        }
    }

    #[test]
    fn map_owned_recalc_clears_sloped_resource_pair_on_both_live_surfaces() {
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=SLOPED_RESOURCE\n\
                 [SLOPED_RESOURCE]\nTiberium=yes\nLand=Tiberium\nNoUseTileLandType=no\n",
            ),
            None,
        );

        for (slope, expected) in [
            (4, FinalizedOverlayCell::from_parts(0, 7)),
            (5, FinalizedOverlayCell::default()),
        ] {
            let mut terrain = flat_terrain(6, 6);
            terrain.cells_mut_for_test()[8].slope_type = slope;
            let mut recalc = LoadCellRecalcState::synthetic(&registry, terrain.cells().len());
            let mut cells = LiveOverlayCells::empty_for_terrain(&terrain);
            let target = real(8, (2, 1));
            cells.write(target.target, 0, 7);
            let mut host = LoadHost::default();

            recalc_target(&mut terrain, &mut recalc, &mut cells, &mut host, target)
                .expect("map-owned resource Recalc");

            assert_eq!(cells.real_cell(8), expected, "slope {slope}");
            assert_eq!(
                terrain.cells()[8].bridge_facts.overlay_id,
                expected.overlay_id()
            );
            assert_eq!(terrain.cells()[8].bridge_facts.state_byte, expected.state());
            assert_eq!(host.events, vec![LoadEvent::Recalc(target, expected)]);
        }
    }

    #[test]
    fn map_owned_recalc_returns_before_reading_or_observing_the_shared_dummy() {
        let mut terrain = flat_terrain(2, 2);
        let before = format!("{:?}", terrain.cells());
        let registry = registry_with_count(1);
        let mut recalc = LoadCellRecalcState::synthetic(&registry, terrain.cells().len());
        let mut cells = LiveOverlayCells::empty_for_terrain(&terrain);
        cells.write(NativeOverlayCellTarget::Dummy, 0, 9);
        let dummy = AuthoredOverlayCellRef {
            target: NativeOverlayCellTarget::Dummy,
            coord: (-1, 3),
        };
        let mut host = LoadHost::default();

        recalc_target(&mut terrain, &mut recalc, &mut cells, &mut host, dummy)
            .expect("dummy Recalc is a total no-op");

        assert_eq!(
            cells.read(NativeOverlayCellTarget::Dummy),
            FinalizedOverlayCell::from_parts(0, 9)
        );
        assert_eq!(format!("{:?}", terrain.cells()), before);
        assert!(host.events.is_empty());
    }

    #[test]
    fn high_mark_recalcs_with_temporary_state_then_restores_the_saved_anchor_byte() {
        let shape = NativeOverlayMapShape::new(6, 6);
        let registry = registry_with_count(0xEF);
        let tiberium_types = TiberiumTypeRegistry::default();
        let shp_ids = BTreeSet::from([0x18, 0x19, 0xED, 0xEE]);

        for (earlier_dir6, later_dir0, y) in [(0x19, 0x18, 5u16), (0xEE, 0xED, 6)] {
            let mut terrain = flat_terrain(16, 16);
            let packs = raw_packs(&[(6, y, earlier_dir6), (7, y, later_dir0)], &[]);
            let mut host = LoadHost::default();

            let payload = AuthoredOverlayFinalizer::new(
                &mut terrain,
                shape,
                &registry,
                &tiberium_types,
                &shp_ids,
                4,
                false,
                &mut host,
            )
            .run(packs)
            .expect("high bridge rows");

            let drain = host
                .events
                .iter()
                .position(|event| *event == LoadEvent::Drain)
                .expect("reader drain");
            let later_anchor_mark = host.events[..drain]
                .iter()
                .find_map(|event| match event {
                    LoadEvent::Recalc(cell, value)
                        if cell.coord == (7, y as i16)
                            && value.overlay_id() == Some(later_dir0) =>
                    {
                        Some(*value)
                    }
                    _ => None,
                })
                .expect("later direction-0 high common Recalc");
            assert_eq!(later_anchor_mark.state(), 0);

            let (_, _, cells, _) = payload.into_parts();
            let anchor_index = usize::from(y) * 16 + 7;
            assert_eq!(cells[anchor_index].overlay_id(), Some(later_dir0));
            assert_eq!(cells[anchor_index].state(), 9);
            assert_eq!(terrain.cells()[anchor_index].bridge_facts.state_byte, 9);
        }
    }

    #[test]
    fn authored_wall_row_publishes_cleanup_counts_and_common_tail_inline() {
        let mut terrain = flat_terrain(6, 6);
        let shape = NativeOverlayMapShape::new(2, 2);
        let registry = wall_registry();
        let tiberium_types = TiberiumTypeRegistry::default();
        let shp_ids = BTreeSet::from([0]);
        let packs = raw_packs(&[(2, 2, 0)], &[]);
        let mut host = LoadHost::default();

        let payload = AuthoredOverlayFinalizer::new(
            &mut terrain,
            shape,
            &registry,
            &tiberium_types,
            &shp_ids,
            4,
            false,
            &mut host,
        )
        .run(packs)
        .expect("authored wall row");

        let anchor = real(14, (2, 2));
        let drain = host
            .events
            .iter()
            .position(|event| *event == LoadEvent::Drain)
            .expect("reader drain");
        assert_eq!(
            &host.events[..15],
            &[
                LoadEvent::Construct(0, (2, 2), 1),
                LoadEvent::Begin(1, anchor),
                LoadEvent::Dirty(MapLoadDirtyKind::BaseMarkTactical, anchor),
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, real(8, (2, 1))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, real(8, (2, 1))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, real(15, (3, 2))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, real(15, (3, 2))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, real(20, (2, 3))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, real(20, (2, 3))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, real(13, (1, 2))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, real(13, (1, 2))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, anchor),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, anchor),
                LoadEvent::Recalc(
                    anchor,
                    FinalizedOverlayCell {
                        identity: 0,
                        state: 0,
                    },
                ),
                LoadEvent::WallZone(anchor),
            ]
        );
        assert_eq!(drain, 25);
        assert!(
            host.events[15..23]
                .iter()
                .all(|event| matches!(event, LoadEvent::BlockerIncrement(_)))
        );
        assert!(matches!(host.events[23], LoadEvent::Recalc(cell, _) if cell == anchor));
        assert_eq!(host.events[24], LoadEvent::FinishCommon(1));

        let (_, _, cells, counts) = payload.into_parts();
        assert_eq!(cells[14].overlay_id(), Some(0));
        for index in [7usize, 8, 9, 13, 15, 19, 20, 21] {
            assert_eq!(counts[index], 1);
        }
        assert_eq!(counts[14], 0);
    }

    #[test]
    fn authored_wall_cleanup_skips_zone_merge_when_recalc_keeps_zone() {
        let mut terrain = flat_terrain(6, 6);
        terrain.cells_mut_for_test()[14].zone_type = zone_class::WALL;
        let shape = NativeOverlayMapShape::new(2, 2);
        let registry = wall_registry();
        let tiberium_types = TiberiumTypeRegistry::default();
        let shp_ids = BTreeSet::from([0]);
        let packs = raw_packs(&[(2, 2, 0)], &[]);
        let mut host = LoadHost::default();

        AuthoredOverlayFinalizer::new(
            &mut terrain,
            shape,
            &registry,
            &tiberium_types,
            &shp_ids,
            4,
            false,
            &mut host,
        )
        .run(packs)
        .expect("authored wall row");

        assert!(
            !host
                .events
                .iter()
                .any(|event| matches!(event, LoadEvent::WallZone(_)))
        );
    }

    #[test]
    fn authored_tiberium_germination_uses_same_class_neighbor_density_table() {
        let shape = NativeOverlayMapShape::new(12, 12);
        let (overlay_types, tiberium_types) = tiberium_registries();
        let neighbors = [
            (5, 4),
            (6, 4),
            (6, 5),
            (6, 6),
            (5, 6),
            (4, 6),
            (4, 5),
            (4, 4),
        ];
        let expected = [0, 1, 3, 4, 6, 7, 8, 10, 11];

        for matching in 0..=8 {
            let mut terrain = flat_terrain(12, 12);
            let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
            for &coord in &neighbors[..matching] {
                let target = live.target(coord.0, coord.1);
                live.write(target, 1, u8::MAX);
            }
            let mut host = LoadHost::default();
            assert_eq!(
                mark_live(
                    &mut terrain,
                    &mut live,
                    shape,
                    &overlay_types,
                    &tiberium_types,
                    &mut host,
                    (5, 5),
                    0,
                )
                .expect("Mark"),
                OverlayMarkOutcome::Marked
            );
            assert_eq!(live.read(live.target(5, 5)).state(), expected[matching]);
        }

        let mut terrain = flat_terrain(12, 12);
        let mut land_only = LiveOverlayCells::empty_for_terrain(&terrain);
        let mut host = LoadHost::default();
        mark_live(
            &mut terrain,
            &mut land_only,
            shape,
            &overlay_types,
            &tiberium_types,
            &mut host,
            (5, 5),
            2,
        )
        .expect("Land=Tiberium Mark");
        assert_eq!(
            land_only.read(land_only.target(5, 5)).state(),
            1,
            "Land=5 without Tiberium=yes returns before neighbor lookup"
        );
    }

    #[test]
    fn authored_tiberium_germination_counts_the_persistent_dummy_once_per_miss() {
        let mut terrain = flat_terrain(4, 4);
        terrain.test_set_native_allocated_cells(&[(0, 0)]);
        let (overlay_types, tiberium_types) = tiberium_registries();
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let dummy = live.target(-1, 0);
        live.write(dummy, 1, 9);
        let mut host = LoadHost::default();

        mark_live(
            &mut terrain,
            &mut live,
            NativeOverlayMapShape::new(4, 4),
            &overlay_types,
            &tiberium_types,
            &mut host,
            (0, 0),
            0,
        )
        .expect("germinating Mark");

        assert_eq!(live.read(live.target(0, 0)).state(), 11);
        assert_eq!(terrain.dummy_cell_requested_coord(), (-1, -1));
        assert_eq!(live.read(dummy).overlay_id(), Some(1));
        assert_eq!(live.read(dummy).state(), 9);
    }

    #[test]
    fn live_overlay_lookup_preserves_signed_fixed_stride_real_alias() {
        let terrain = flat_terrain(512, 2);
        let live = LiveOverlayCells::empty_for_terrain(&terrain);
        assert_eq!(live.target(-510, 2), NativeOverlayCellTarget::Real(514));
    }

    #[test]
    fn authored_low_fixed_tables_cover_both_retail_families_without_rng() {
        let shape = NativeOverlayMapShape::new(10, 10);
        let registry = registry_with_count(0xF0);
        let tiberium_types = TiberiumTypeRegistry::default();
        let fixtures = [
            (0x7A, 0x5C, [(8, 7), (8, 8), (8, 9)]),
            (0x7B, 0x5E, [(8, 7), (8, 8), (8, 9)]),
            (0x7C, 0x60, [(7, 8), (8, 8), (9, 8)]),
            (0x7D, 0x62, [(7, 8), (8, 8), (9, 8)]),
            (0xE9, 0xDF, [(8, 7), (8, 8), (8, 9)]),
            (0xEA, 0xE1, [(8, 7), (8, 8), (8, 9)]),
            (0xEB, 0xE3, [(7, 8), (8, 8), (9, 8)]),
            (0xEC, 0xE5, [(7, 8), (8, 8), (9, 8)]),
        ];

        for (trigger, fixed_id, coords) in fixtures {
            let mut terrain = flat_terrain(20, 20);
            let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
            let mut host = LoadHost::default();
            assert_eq!(
                mark_live(
                    &mut terrain,
                    &mut live,
                    shape,
                    &registry,
                    &tiberium_types,
                    &mut host,
                    (8, 8),
                    trigger,
                )
                .expect("Mark"),
                OverlayMarkOutcome::Marked
            );
            assert_eq!(host.next_raw, 0, "trigger {trigger:#04X}");
            assert_eq!(
                recalcs(&host).len(),
                4,
                "three fixed Recalcs and the common one, trigger {trigger:#04X}"
            );
            for (state, coord) in coords.into_iter().enumerate() {
                let cell = live.read(live.target(coord.0, coord.1));
                assert_eq!(cell.overlay_id(), Some(fixed_id), "at {coord:?}");
                assert_eq!(cell.state(), state as u8, "at {coord:?}");
            }
        }
    }

    #[test]
    fn authored_low_occupied_probe_still_visits_all_three_fixed_cells() {
        let mut terrain = flat_terrain(12, 12);
        terrain.test_set_native_allocated_cells(&[(8, 7)]);
        let shape = NativeOverlayMapShape::new(10, 10);
        let registry = registry_with_count(0x7C);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let occupied = live.target(8, 7);
        live.write(occupied, 3, 4);
        let mut host = LoadHost::default();

        assert_eq!(
            mark_live(
                &mut terrain,
                &mut live,
                shape,
                &registry,
                &TiberiumTypeRegistry::default(),
                &mut host,
                (8, 8),
                0x7B,
            )
            .expect("Mark"),
            OverlayMarkOutcome::Marked
        );
        assert_eq!(host.next_raw, 0);
        assert!(
            recalcs(&host).is_empty(),
            "the trigger cell is the dummy, whose common Recalc returns at once"
        );
        assert_eq!(terrain.dummy_cell_requested_coord(), (8, 9));
        assert_eq!(live.read(occupied).overlay_id(), Some(3));
        assert_eq!(live.read(occupied).state(), 4);
    }

    #[test]
    fn authored_low_body_runs_opposite_to_trigger_with_three_raw_draws_per_row() {
        let mut terrain = flat_terrain(24, 24);
        let shape = NativeOverlayMapShape::new(12, 12);
        let registry = registry_with_count(0x7C);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let opposite = live.target(12, 8);
        live.write(opposite, 0x5C, 1);
        let mut host = LoadHost {
            raw: (0..9).collect(),
            ..LoadHost::default()
        };

        assert_eq!(
            mark_live(
                &mut terrain,
                &mut live,
                shape,
                &registry,
                &TiberiumTypeRegistry::default(),
                &mut host,
                (8, 8),
                0x7B,
            )
            .expect("Mark"),
            OverlayMarkOutcome::Marked
        );
        assert_eq!(host.next_raw, 9);
        assert_eq!(
            recalcs(&host).len(),
            13,
            "three fixed, nine body and the common Recalc"
        );
        for (row, center_x) in [11i16, 10, 9].into_iter().enumerate() {
            for (state, y) in [7i16, 8, 9].into_iter().enumerate() {
                let draw = row * 3 + state;
                let cell = live.read(live.target(center_x, y));
                assert_eq!(cell.overlay_id(), Some(0x4A + (draw as u8 & 3)));
                assert_eq!(cell.state(), state as u8);
            }
        }
    }

    #[test]
    fn authored_low_north_south_body_uses_transverse_west_center_east_order() {
        let mut terrain = flat_terrain(24, 24);
        let shape = NativeOverlayMapShape::new(10, 10);
        let registry = registry_with_count(0x7E);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let opposite = live.target(8, 4);
        live.write(opposite, 0x60, 1);
        let mut host = LoadHost {
            raw: (0..9).collect(),
            ..LoadHost::default()
        };

        assert_eq!(
            mark_live(
                &mut terrain,
                &mut live,
                shape,
                &registry,
                &TiberiumTypeRegistry::default(),
                &mut host,
                (8, 8),
                0x7D,
            )
            .expect("Mark"),
            OverlayMarkOutcome::Marked
        );
        assert_eq!(host.next_raw, 9);
        assert_eq!(
            recalcs(&host).len(),
            13,
            "three fixed, nine body and the common Recalc"
        );
        for (row, center_y) in [5i16, 6, 7].into_iter().enumerate() {
            for (state, x) in [7i16, 8, 9].into_iter().enumerate() {
                let draw = row * 3 + state;
                let cell = live.read(live.target(x, center_y));
                assert_eq!(cell.overlay_id(), Some(0x53 + (draw as u8 & 3)));
                assert_eq!(cell.state(), state as u8);
            }
        }
    }

    #[test]
    fn authored_low_search_skips_wrong_id_and_state_then_uses_first_exact_match() {
        let mut terrain = flat_terrain(24, 24);
        let shape = NativeOverlayMapShape::new(12, 12);
        let registry = registry_with_count(0x7C);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let wrong_id = live.target(9, 8);
        let wrong_state = live.target(10, 8);
        let first_match = live.target(11, 8);
        let later_match = live.target(13, 8);
        live.write(wrong_id, 0x5E, 1);
        live.write(wrong_state, 0x5C, 2);
        live.write(first_match, 0x5C, 1);
        live.write(later_match, 0x5C, 1);
        let mut host = LoadHost {
            raw: (0..6).collect(),
            ..LoadHost::default()
        };

        assert_eq!(
            mark_live(
                &mut terrain,
                &mut live,
                shape,
                &registry,
                &TiberiumTypeRegistry::default(),
                &mut host,
                (8, 8),
                0x7B,
            )
            .expect("Mark"),
            OverlayMarkOutcome::Marked
        );
        assert_eq!(host.next_raw, 6);
        assert_eq!(
            recalcs(&host).len(),
            10,
            "three fixed, six body and the common Recalc"
        );
        assert_eq!(live.read(first_match).overlay_id(), Some(0x5C));
        assert_eq!(live.read(first_match).state(), 1);
        assert_eq!(live.read(later_match).overlay_id(), Some(0x5C));
        assert_eq!(live.read(later_match).state(), 1);
    }

    #[test]
    fn authored_low_adjacent_centers_overwrite_the_new_fixed_row() {
        let mut terrain = flat_terrain(20, 20);
        let shape = NativeOverlayMapShape::new(10, 10);
        let registry = registry_with_count(0x7C);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let opposite = live.target(9, 8);
        live.write(opposite, 0x5C, 1);
        let mut host = LoadHost {
            raw: vec![3, 2, 1],
            ..LoadHost::default()
        };

        assert_eq!(
            mark_live(
                &mut terrain,
                &mut live,
                shape,
                &registry,
                &TiberiumTypeRegistry::default(),
                &mut host,
                (8, 8),
                0x7B,
            )
            .expect("Mark"),
            OverlayMarkOutcome::Marked
        );
        assert_eq!(host.next_raw, 3);
        for (state, (y, id)) in [(7i16, 0x4D), (8, 0x4C), (9, 0x4B)].into_iter().enumerate() {
            let cell = live.read(live.target(8, y));
            assert_eq!(cell.overlay_id(), Some(id));
            assert_eq!(cell.state(), state as u8);
        }
    }

    #[test]
    fn authored_low_search_length_uses_requested_coordinate_before_real_alias_canonicalization() {
        let mut terrain = flat_terrain(512, 4);
        let shape = NativeOverlayMapShape::new(512, 2);
        let registry = registry_with_count(0x7C);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let aliased_opposite = live.target(512, 2);
        assert_eq!(aliased_opposite, NativeOverlayCellTarget::Real(1536));
        live.write(aliased_opposite, 0x5C, 1);
        let mut host = LoadHost {
            raw: vec![0, 1, 2],
            ..LoadHost::default()
        };

        assert_eq!(
            mark_live(
                &mut terrain,
                &mut live,
                shape,
                &registry,
                &TiberiumTypeRegistry::default(),
                &mut host,
                (511, 2),
                0x7B,
            )
            .expect("Mark"),
            OverlayMarkOutcome::Marked
        );
        assert_eq!(host.next_raw, 3);
        for (state, y) in [1i16, 2, 3].into_iter().enumerate() {
            let cell = live.read(live.target(511, y));
            assert_eq!(cell.overlay_id(), Some(0x4A + state as u8));
            assert_eq!(cell.state(), state as u8);
        }
    }

    #[test]
    fn authored_wall_runs_cleanup_counts_and_common_recalc_in_native_order() {
        let mut terrain = flat_terrain(5, 5);
        let registry = wall_registry();
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let north = live.target(2, 1);
        let east = live.target(3, 2);
        live.write(north, 0, 0);
        live.write(east, 2, 0);
        let mut host = LoadHost::default();

        assert_eq!(
            mark_live(
                &mut terrain,
                &mut live,
                NativeOverlayMapShape::new(5, 5),
                &registry,
                &TiberiumTypeRegistry::default(),
                &mut host,
                (2, 2),
                0,
            )
            .expect("Mark"),
            OverlayMarkOutcome::Marked
        );

        let cell = FinalizedOverlayCell::from_parts;
        assert_eq!(
            host.events,
            vec![
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, real(7, (2, 1))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, real(7, (2, 1))),
                LoadEvent::Recalc(real(7, (2, 1)), cell(0, 0x04)),
                LoadEvent::WallZone(real(7, (2, 1))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, real(13, (3, 2))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, real(13, (3, 2))),
                LoadEvent::Recalc(real(13, (3, 2)), cell(2, 0)),
                LoadEvent::WallZone(real(13, (3, 2))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, real(17, (2, 3))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, real(17, (2, 3))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, real(11, (1, 2))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, real(11, (1, 2))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallTactical, real(12, (2, 2))),
                LoadEvent::Dirty(MapLoadDirtyKind::WallRadar, real(12, (2, 2))),
                LoadEvent::Recalc(real(12, (2, 2)), cell(0, 0x01)),
                LoadEvent::WallZone(real(12, (2, 2))),
                LoadEvent::BlockerIncrement(real(7, (2, 1))),
                LoadEvent::BlockerIncrement(real(8, (3, 1))),
                LoadEvent::BlockerIncrement(real(13, (3, 2))),
                LoadEvent::BlockerIncrement(real(18, (3, 3))),
                LoadEvent::BlockerIncrement(real(17, (2, 3))),
                LoadEvent::BlockerIncrement(real(16, (1, 3))),
                LoadEvent::BlockerIncrement(real(11, (1, 2))),
                LoadEvent::BlockerIncrement(real(6, (1, 1))),
                LoadEvent::Recalc(real(12, (2, 2)), cell(0, 0x01)),
            ]
        );
        assert_eq!(live.read(north).state(), 0x04, "north connects south");
        assert_eq!(
            live.read(east).state(),
            0,
            "different wall id stays isolated"
        );
        let anchor = live.target(2, 2);
        assert_eq!(live.read(anchor).state(), 0x01, "anchor connects north");
        for index in [6usize, 7, 8, 11, 13, 16, 17, 18] {
            assert_eq!(live.authored_wall_neighbor_counts[index], 1);
        }
        assert_eq!(live.authored_wall_neighbor_counts[12], 0);
    }

    #[test]
    fn authored_wall_slope_rejects_before_stamp_or_effect() {
        let mut terrain = flat_terrain(3, 3);
        terrain.cell_mut_for_test(1, 1).expect("anchor").slope_type = 5;
        let registry = wall_registry();
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let mut host = LoadHost::default();

        assert_eq!(
            mark_live(
                &mut terrain,
                &mut live,
                NativeOverlayMapShape::new(3, 3),
                &registry,
                &TiberiumTypeRegistry::default(),
                &mut host,
                (1, 1),
                0,
            )
            .expect("Mark"),
            OverlayMarkOutcome::SlopeRefused
        );
        assert!(host.events.is_empty());
        assert_eq!(live.real_cell(4), FinalizedOverlayCell::default());
        assert!(
            live.authored_wall_neighbor_counts
                .iter()
                .all(|&count| count == 0)
        );
    }

    #[test]
    fn authored_wall_retains_wrapping_alias_counts_across_data_and_low_body_overwrite() {
        let mut terrain = flat_terrain(512, 2);
        let registry = wall_registry();
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        // West of (0,1) linearizes to fixed slot 511 -> real (511,0).
        live.authored_wall_neighbor_counts[511] = u8::MAX;
        let mut host = LoadHost::default();
        assert_eq!(
            mark_live(
                &mut terrain,
                &mut live,
                NativeOverlayMapShape::new(512, 2),
                &registry,
                &TiberiumTypeRegistry::default(),
                &mut host,
                (0, 1),
                0,
            )
            .expect("Mark"),
            OverlayMarkOutcome::Marked
        );
        assert_eq!(live.authored_wall_neighbor_counts[511], 0);
        assert_eq!(
            host.events
                .iter()
                .filter(|event| matches!(
                    event,
                    LoadEvent::BlockerIncrement(AuthoredOverlayCellRef {
                        target: NativeOverlayCellTarget::Dummy,
                        ..
                    })
                ))
                .count(),
            3,
            "SE, S, and NW are true dummy targets in this fixed-grid fixture"
        );

        let anchor = live.target(0, 1);
        assert_eq!(
            live.read(anchor).state(),
            0,
            "absent data retains Mark state"
        );
        let retained = live.authored_wall_neighbor_counts.clone();
        live.write_state(anchor, 0xA7);
        assert_eq!(live.authored_wall_neighbor_counts, retained);
        live.write(anchor, 0x7A, 2);
        assert_eq!(live.authored_wall_neighbor_counts, retained);

        let (_, _, cells, counts) = live.finish().into_parts();
        assert_eq!(cells[512].overlay_id(), Some(0x7A));
        assert_eq!(cells[512].state(), 2);
        assert_eq!(
            counts, retained,
            "low body overwrite cannot reverse wall counts"
        );
        assert_eq!(
            counts.iter().map(|&count| u32::from(count)).sum::<u32>(),
            4,
            "only five real targets exist and the aliased 255 increment wraps to zero"
        );
    }
}
