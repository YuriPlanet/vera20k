//! Live CellClass overlay identity/state surface for fresh map finalization.
//!
//! The authored reader mutates this surface synchronously in native fixed-grid
//! lookup order. Only after OverlayData, the shared drain, and the first live
//! Recalc sweep does it move a linear payload into the simulation OverlayGrid.

use std::collections::BTreeSet;

use crate::map::bridge_facts::{
    BridgeFlagStamp, BridgeStampSlot, high_bridge_stamp_for_overlay,
};
use crate::map::map_file::AuthoredOverlayPackReceipt;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::{
    AutomaticTubeAllocation, AutomaticTubeRequest, LoadCellRecalcEffects, LoadCellRecalcError,
    LoadCellRecalcOutcome, LoadCellRecalcState, ResolvedTerrainGrid, SharedCellDummy,
    TerrainTileAnimation,
};
use crate::rules::terrain_rules::LandType;
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
        if self.identity >= 0 && self.identity <= u8::MAX as i32 - 1 {
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

    #[cfg(test)]
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

/// Synchronous wall-local effects in native execution order. Callers must
/// apply each effect before this transaction advances to the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthoredWallEffect {
    TacticalDirty(AuthoredOverlayCellRef),
    RadarDirty(AuthoredOverlayCellRef),
    CleanupRecalcAndZone(AuthoredOverlayCellRef),
    BlockerCountIncrement(AuthoredOverlayCellRef),
    CommonAnchorRecalc(AuthoredOverlayCellRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthoredWallMarkResult {
    Completed,
    RejectedUnallocatedAnchor,
    RejectedSteepSlope,
    RejectedNonWallType,
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

    /// `Cell_in_bounds_check @ 0x00568300` after the caller has narrowed both
    /// coordinate words to signed 16-bit values.
    pub(crate) const fn admits(self, x: i16, y: i16) -> bool {
        let x = x as i32;
        let y = y as i32;
        let sum = x.wrapping_add(y);
        self.width < sum
            && x.wrapping_sub(y) < self.width
            && y.wrapping_sub(x) < self.width
            && sum <= self.width.wrapping_add(self.height.wrapping_mul(2))
    }

    /// Exact real-cell anti-diagonal order used by `Full_Init @ 0x00686B20`
    /// through `CellIterator_Init @ 0x00578350` / `Next @ 0x00578290`:
    /// increasing `x`, decreasing `y` for each sum in
    /// `W+1 ..= W+2H`. Valid fresh maps keep both dimensions in `1..=512`.
    pub(crate) fn recalc_cells(self) -> Vec<(i16, i16)> {
        if !(1..=512).contains(&self.width) || !(1..=512).contains(&self.height) {
            return Vec::new();
        }
        let mut cells = Vec::with_capacity(
            usize::try_from(self.height * (2 * self.width - 1)).unwrap_or(0),
        );
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
pub(crate) enum AuthoredLowMarkResult {
    OccupiedFixedRow,
    FixedEndWithoutOpposite,
    BodyRows { rows: u32, scenario_draws: u32 },
}

/// Narrow synchronous seam used only by the fixed-map low-overlay Mark arm.
/// `recalc` receives the already-mutated live overlay surface and must finish
/// the complete real-cell effect before the Mark transaction advances.
pub(crate) trait AuthoredLowMarkHost {
    type Error;

    fn next_scenario_raw(&mut self) -> u32;

    fn recalc(
        &mut self,
        cells: &mut LiveOverlayCells,
        cell: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error>;
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
    fn observe_recalc(
        &mut self,
        _cell: AuthoredOverlayCellRef,
        _value: FinalizedOverlayCell,
    ) {
    }

    fn merge_wall_zone(
        &mut self,
        cell: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error>;

    fn observe_blocker_count_increment(
        &mut self,
        cell: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error>;

    fn spawn_cell_anim(
        &mut self,
        handle: Self::Handle,
        anim_name: &str,
        cell: AuthoredOverlayCellRef,
        world_z: i32,
    ) -> Result<(), Self::Error>;

    fn finish_common(&mut self, handle: Self::Handle) -> Result<(), Self::Error>;

    fn finish_slope_survivor(
        &mut self,
        handle: Self::Handle,
    ) -> Result<(), Self::Error>;

    fn drain_deferred(&mut self) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub(crate) enum AuthoredOverlayFinalizeError<E> {
    InvalidMapShape { width: i32, height: i32 },
    MalformedOverlayType { overlay_id: u8 },
    ConstructedUnallocatedAnchor { overlay_id: u8, cell: (u16, u16) },
    WallInvariant(AuthoredWallMarkResult),
    RecalcMalformedOverlayIdentity { identity: i32 },
    RecalcMissingOverlayType { overlay_id: u8 },
    RecalcCellIndexOutOfBounds { index: usize },
    RecalcResidentInput { reason: String },
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
            Self::WallInvariant(result) => {
                write!(f, "authored wall Mark invariant failed: {result:?}")
            }
            Self::RecalcMalformedOverlayIdentity { identity } => {
                write!(f, "authored Recalc observed malformed overlay identity {identity}")
            }
            Self::RecalcMissingOverlayType { overlay_id } => {
                write!(f, "authored Recalc cannot resolve overlay type {overlay_id}")
            }
            Self::RecalcCellIndexOutOfBounds { index } => {
                write!(f, "authored Recalc cell index {index} is out of bounds")
            }
            Self::RecalcResidentInput { reason } => {
                write!(f, "authored Recalc received invalid resident inputs: {reason}")
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

struct FinalizerLowHost<'a, 'resources, H> {
    terrain: &'a mut ResolvedTerrainGrid,
    recalc: &'a mut LoadCellRecalcState<'resources>,
    host: &'a mut H,
}

impl<H: AuthoredOverlayLoadHost> AuthoredLowMarkHost
    for FinalizerLowHost<'_, '_, H>
{
    type Error = AuthoredOverlayFinalizeError<H::Error>;

    fn next_scenario_raw(&mut self) -> u32 {
        self.host.next_scenario_raw()
    }

    fn recalc(
        &mut self,
        cells: &mut LiveOverlayCells,
        cell: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error> {
        recalc_target(self.terrain, self.recalc, cells, self.host, cell)
            .map(|_| ())
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
        LoadCellRecalcError::ResidentInput(error) => AuthoredOverlayFinalizeError::RecalcResidentInput {
            reason: error.to_string(),
        },
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
    #[cfg(test)]
    Synthetic(LoadCellRecalcState<'resources>),
}

impl<'load, 'resources> LoadRecalcOwner<'load, 'resources> {
    fn as_mut(&mut self) -> &mut LoadCellRecalcState<'resources> {
        match self {
            Self::Borrowed(recalc) => recalc,
            #[cfg(test)]
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

impl<'load, 'resources, H: AuthoredOverlayLoadHost>
    AuthoredOverlayFinalizer<'load, 'resources, H>
{
    #[allow(clippy::too_many_arguments)]
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

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
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
        let recalc = LoadCellRecalcState::synthetic(overlay_types, terrain.cells.len());
        Self {
            terrain,
            recalc: LoadRecalcOwner::Synthetic(recalc),
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
        if !(1..=512).contains(&self.shape.width)
            || !(1..=512).contains(&self.shape.height)
        {
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
        let flags = self
            .overlay_types
            .flags(overlay_id)
            .cloned()
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
        let high = high_bridge_stamp_for_overlay(overlay_id);
        let saved_high_state = high.map(|_| self.cells.read(anchor.target).state());
        let Some(handle) = self
            .host
            .try_construct_overlay(overlay_id, (x, y))
            .map_err(AuthoredOverlayFinalizeError::Host)?
        else {
            self.restore_high_anchor_state(packed, saved_high_state);
            return Ok(());
        };
        let NativeOverlayCellTarget::Real(anchor_index) = anchor.target else {
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
        let slope_type = self.terrain.cells()[anchor_index].slope_type;
        if slope_type > 4 && overlay_id != 0xB2 {
            self.host
                .finish_slope_survivor(handle)
                .map_err(AuthoredOverlayFinalizeError::Host)?;
            self.restore_high_anchor_state(packed, saved_high_state);
            return Ok(());
        }

        if let Some((family, direction)) = high {
            self.apply_high_stamp(anchor, overlay_id, family, direction);
            self.finish_ordinary_or_high(handle, anchor, &flags, false)?;
            self.restore_high_anchor_state(packed, saved_high_state);
            return Ok(());
        }
        if flags.wall {
            self.finish_wall(handle, anchor, overlay_id, slope_type)?;
            return Ok(());
        }

        let low_result = {
            let (terrain, recalc, host) = (
                &mut *self.terrain,
                self.recalc.as_mut(),
                &mut *self.host,
            );
            let mut adapter = FinalizerLowHost {
                terrain,
                recalc,
                host,
            };
            self.cells
                .try_mark_authored_low(self.shape, packed.0, packed.1, overlay_id, &mut adapter)
                ?
        };
        if low_result.is_some() {
            self.recalc_cell(anchor)?;
            self.host
                .finish_common(handle)
                .map_err(AuthoredOverlayFinalizeError::Host)?;
            return Ok(());
        }

        self.cells
            .write_identity(anchor.target, i32::from(overlay_id));
        self.finish_ordinary_or_high(handle, anchor, &flags, true)
    }

    fn apply_high_stamp(
        &mut self,
        anchor: AuthoredOverlayCellRef,
        overlay_id: u8,
        family: crate::map::bridge_facts::BridgeStampFamily,
        direction: u8,
    ) {
        // gamemd-derived: CellClass::SetBridgeDirection_NESW @ 0x0047E040 and
        // SetBridgeDirection_NWSE @ 0x0047E470.
        let stamp = BridgeFlagStamp::new(
            (anchor.coord.0 as u16, anchor.coord.1 as u16),
            direction,
            true,
        );
        for (slot, requested) in stamp.slots().expect("verified high direction") {
            let target = self
                .terrain
                .apply_authored_bridge_flag_slot(anchor.target, stamp, family, slot, requested)
                .map_or(NativeOverlayCellTarget::Dummy, NativeOverlayCellTarget::Real);
            if matches!(
                slot,
                BridgeStampSlot::Anchor
                    | BridgeStampSlot::Forward1
                    | BridgeStampSlot::Forward2
                    | BridgeStampSlot::Opposite
            ) {
                self.cells
                    .write_state(target, if direction == 0 { 0 } else { 9 });
            }
        }
        self.cells
            .write_identity(anchor.target, i32::from(overlay_id));
        if let NativeOverlayCellTarget::Real(index) = anchor.target {
            self.terrain.mirror_authored_overlay_identity(index, Some(overlay_id));
        }
    }

    fn finish_ordinary_or_high(
        &mut self,
        handle: H::Handle,
        anchor: AuthoredOverlayCellRef,
        flags: &crate::rules::overlay_types::OverlayTypeFlags,
        write_ordinary_state: bool,
    ) -> Result<(), AuthoredOverlayFinalizeError<H::Error>> {
        // gamemd-derived: OverlayClass::Mark ordinary/high tail @
        // 0x005FD09F..0x005FD227.
        if write_ordinary_state {
            self.cells.write_state(anchor.target, 0);
            if flags.land == LandType::Tiberium {
                self.cells.write_state(anchor.target, 1);
                self.cells.germinate_authored_tiberium(
                    self.overlay_types,
                    self.tiberium_types,
                    anchor,
                );
            }
            if flags.crate_type {
                self.cells.write_state(anchor.target, u8::MAX);
            }
        }
        if let Some(anim_name) = flags.cell_anim.as_deref() {
            mirror_real_overlay_pair(self.terrain, &self.cells, anchor);
            let world_z = match anchor.target {
                NativeOverlayCellTarget::Real(index) => i32::from(
                    self.terrain.cells()[index].level as i8,
                )
                .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS),
                NativeOverlayCellTarget::Dummy => 0,
            };
            self.host
                .spawn_cell_anim(handle, anim_name, anchor, world_z)
                .map_err(AuthoredOverlayFinalizeError::Host)?;
        }
        self.recalc_cell(anchor)?;
        self.host
            .finish_common(handle)
            .map_err(AuthoredOverlayFinalizeError::Host)
    }

    fn finish_wall(
        &mut self,
        handle: H::Handle,
        anchor: AuthoredOverlayCellRef,
        overlay_id: u8,
        slope_type: u8,
    ) -> Result<(), AuthoredOverlayFinalizeError<H::Error>> {
        let (terrain, recalc, host) = (
            &mut *self.terrain,
            self.recalc.as_mut(),
            &mut *self.host,
        );
        let result = self
            .cells
            .try_mark_authored_wall(
                self.overlay_types,
                anchor.coord.0,
                anchor.coord.1,
                overlay_id,
                slope_type,
                |cells, effect| match effect {
                    AuthoredWallEffect::TacticalDirty(cell) => {
                        mirror_real_overlay_pair(terrain, cells, cell);
                        host.publish_dirty(MapLoadDirtyKind::WallTactical, cell)
                            .map_err(AuthoredOverlayFinalizeError::Host)
                    }
                    AuthoredWallEffect::RadarDirty(cell) => {
                        host.publish_dirty(MapLoadDirtyKind::WallRadar, cell)
                            .map_err(AuthoredOverlayFinalizeError::Host)
                    }
                    AuthoredWallEffect::CleanupRecalcAndZone(cell) => {
                        let outcome = recalc_target(terrain, recalc, cells, host, cell)?;
                        if outcome.is_some_and(|outcome| {
                            outcome.zone_before != outcome.zone_after
                        }) {
                            host.merge_wall_zone(cell)
                                .map_err(AuthoredOverlayFinalizeError::Host)?;
                        }
                        Ok(())
                    }
                    AuthoredWallEffect::BlockerCountIncrement(cell) => {
                        host.observe_blocker_count_increment(cell)
                            .map_err(AuthoredOverlayFinalizeError::Host)
                    }
                    AuthoredWallEffect::CommonAnchorRecalc(cell) => {
                        recalc_target(terrain, recalc, cells, host, cell)?;
                        Ok(())
                    }
                },
            )
            ?;
        if result != AuthoredWallMarkResult::Completed {
            return Err(AuthoredOverlayFinalizeError::WallInvariant(result));
        }
        self.host
            .finish_common(handle)
            .map_err(AuthoredOverlayFinalizeError::Host)
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

#[derive(Debug, Clone, Copy)]
struct AuthoredLowTriggerSpec {
    fixed_id: u8,
    fixed_start: (i16, i16),
    fixed_step: (i16, i16),
    join_step: (i16, i16),
    opposite_id: u8,
    body_base: u8,
    body_cross: [(i16, i16); 3],
}

fn authored_low_trigger_spec(overlay_id: u8) -> Option<AuthoredLowTriggerSpec> {
    const EW_CROSS: [(i16, i16); 3] = [(0, -1), (0, 0), (0, 1)];
    const NS_CROSS: [(i16, i16); 3] = [(-1, 0), (0, 0), (1, 0)];
    let (family_base, trigger) = match overlay_id {
        0x7A..=0x7D => (0u8, overlay_id - 0x7A),
        0xE9..=0xEC => (1u8, overlay_id - 0xE9),
        _ => return None,
    };
    let fixed_ids = if family_base == 0 {
        [0x5C, 0x5E, 0x60, 0x62]
    } else {
        [0xDF, 0xE1, 0xE3, 0xE5]
    };
    let opposite_ids = if family_base == 0 {
        [0x5E, 0x5C, 0x62, 0x60]
    } else {
        [0xE1, 0xDF, 0xE5, 0xE3]
    };
    let index = usize::from(trigger);
    Some(AuthoredLowTriggerSpec {
        fixed_id: fixed_ids[index],
        fixed_start: if index < 2 { (0, -1) } else { (-1, 0) },
        fixed_step: if index < 2 { (0, 1) } else { (1, 0) },
        join_step: [(-1, 0), (1, 0), (0, 1), (0, -1)][index],
        opposite_id: opposite_ids[index],
        body_base: if family_base == 0 {
            if index < 2 { 0x4A } else { 0x53 }
        } else if index < 2 {
            0xCD
        } else {
            0xD6
        },
        body_cross: if index < 2 { EW_CROSS } else { NS_CROSS },
    })
}

const fn wrapping_step(cell: (i16, i16), delta: (i16, i16)) -> (i16, i16) {
    (
        cell.0.wrapping_add(delta.0),
        cell.1.wrapping_add(delta.1),
    )
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
            cells: vec![FinalizedOverlayCell::default();
                usize::from(width) * usize::from(height)],
            authored_wall_neighbor_counts: vec![0; usize::from(width) * usize::from(height)],
            shared_dummy: terrain.shared_cell_dummy(),
        }
    }

    /// `MapClass::Get_CellClass` narrows both operands to signed words before
    /// sign-extending `y * 512 + x`. A true miss stamps only dummy coordinates.
    pub(crate) fn target(
        &self,
        x: i16,
        y: i16,
    ) -> NativeOverlayCellTarget {
        let real = crate::map::cell_index::cell_linear_index(i32::from(x), i32::from(y))
            .and_then(|linear| {
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
            });
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

    pub(crate) fn write_identity(
        &mut self,
        target: NativeOverlayCellTarget,
        identity: i32,
    ) {
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

    pub(crate) fn write(
        &mut self,
        target: NativeOverlayCellTarget,
        identity: i32,
        state: u8,
    ) {
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

    fn wrapping_increment_authored_wall_count(&mut self, target: NativeOverlayCellTarget) {
        if let NativeOverlayCellTarget::Real(index) = target {
            self.authored_wall_neighbor_counts[index] =
                self.authored_wall_neighbor_counts[index].wrapping_add(1);
        } else {
            self.shared_dummy.adjust_neighbor_count(true);
        }
    }

    fn germinate_authored_tiberium(
        &mut self,
        overlay_types: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        anchor: AuthoredOverlayCellRef,
    ) {
        // gamemd-derived: CellClass::SpreadCellGerminate(0) @ 0x004818E0,
        // called by OverlayClass::Mark @ 0x005FD0EC.
        const ADJACENT_8: [(i16, i16); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ];
        const DENSITY_FOR_MATCHING_NEIGHBORS: [u8; 9] = [0, 1, 3, 4, 6, 7, 8, 10, 11];

        let current = self.read(anchor.target);
        let Some(overlay_id) = current.overlay_id() else {
            return;
        };
        let Some(tiberium_type) =
            overlay_types.tiberium_type_for_overlay(tiberium_types, overlay_id)
        else {
            return;
        };

        let mut matching = 0usize;
        for offset in ADJACENT_8 {
            let coord = wrapping_step(anchor.coord, offset);
            let neighbor = self.cell_ref(coord.0, coord.1);
            let neighbor_type = self
                .read(neighbor.target)
                .overlay_id()
                .and_then(|id| overlay_types.tiberium_type_for_overlay(tiberium_types, id));
            matching += usize::from(neighbor_type == Some(tiberium_type));
        }
        self.write_state(
            anchor.target,
            DENSITY_FOR_MATCHING_NEIGHBORS[matching],
        );
    }

    /// Execute the active-retail fixed-map low-overlay branch. The caller owns
    /// base dirty, the universal slope gate, and the one common anchor Recalc;
    /// this method owns only the fixed/search/body transaction and its inline
    /// per-write Recalcs.
    ///
    /// Native evidence: `OverlayClass::Mark @ 0x005FC570`, wood corridor
    /// `0x005FC790..0x005FCB70`, concrete corridor
    /// `0x005FCBB9..0x005FCF9E`, and the settled tables at
    /// `0x008333C0..0x00833440`.
    pub(crate) fn try_mark_authored_low<H: AuthoredLowMarkHost>(
        &mut self,
        shape: NativeOverlayMapShape,
        x: i16,
        y: i16,
        overlay_id: u8,
        host: &mut H,
    ) -> Result<Option<AuthoredLowMarkResult>, H::Error> {
        let Some(spec) = authored_low_trigger_spec(overlay_id) else {
            return Ok(None);
        };

        let origin = (x, y);
        let first_fixed = wrapping_step(origin, spec.fixed_start);
        let mut probe = first_fixed;
        let mut all_clear = true;
        for _ in 0..3 {
            let cell = self.cell_ref(probe.0, probe.1);
            if self.read(cell.target).identity != NO_OVERLAY_IDENTITY {
                all_clear = false;
            }
            probe = wrapping_step(probe, spec.fixed_step);
        }
        if !all_clear {
            return Ok(Some(AuthoredLowMarkResult::OccupiedFixedRow));
        }

        let mut fixed = first_fixed;
        for state in 0..3u8 {
            let cell = self.cell_ref(fixed.0, fixed.1);
            self.write_identity(cell.target, i32::from(spec.fixed_id));
            self.write_state(cell.target, state);
            host.recalc(self, cell)?;
            fixed = wrapping_step(fixed, spec.fixed_step);
        }

        let mut search = wrapping_step(origin, spec.join_step);
        let found = loop {
            if !shape.admits(search.0, search.1) {
                break None;
            }
            let cell = self.cell_ref(search.0, search.1);
            let current = self.read(cell.target);
            if current.identity == i32::from(spec.opposite_id) && current.state == 1 {
                break Some(search);
            }
            search = wrapping_step(search, spec.join_step);
        };
        let Some(found) = found else {
            return Ok(Some(AuthoredLowMarkResult::FixedEndWithoutOpposite));
        };

        let reverse = (-spec.join_step.0, -spec.join_step.1);
        let mut work = wrapping_step(found, reverse);
        let rows = (i32::from(work.0) - i32::from(first_fixed.0))
            .abs()
            .max((i32::from(work.1) - i32::from(first_fixed.1)).abs())
            as u32;
        let mut scenario_draws = 0u32;
        for _ in 0..rows {
            for (state, offset) in spec.body_cross.into_iter().enumerate() {
                let coord = wrapping_step(work, offset);
                let cell = self.cell_ref(coord.0, coord.1);
                let raw = host.next_scenario_raw();
                scenario_draws = scenario_draws.wrapping_add(1);
                self.write_identity(
                    cell.target,
                    i32::from(spec.body_base.wrapping_add((raw & 3) as u8)),
                );
                self.write_state(cell.target, state as u8);
                host.recalc(self, cell)?;
            }
            work = wrapping_step(work, reverse);
        }

        Ok(Some(AuthoredLowMarkResult::BodyRows {
            rows,
            scenario_draws,
        }))
    }

    /// Complete the authored `Wall=yes` Mark arm after reader admission and
    /// allocation. Successful Full_Init keeps ScenarioInit nonzero, so there is
    /// intentionally no Rust approximation of the counter-zero build predicate.
    ///
    /// Native evidence: `OverlayClass::Mark @ 0x005FC570`, wall success corridor
    /// `0x005FC6F4..0x005FC775`, and common tail `0x005FD1FA..0x005FD227`.
    #[cfg(test)]
    pub(crate) fn mark_authored_wall(
        &mut self,
        registry: &OverlayTypeRegistry,
        x: i16,
        y: i16,
        overlay_id: u8,
        anchor_slope_type: u8,
        mut apply_effect: impl FnMut(AuthoredWallEffect),
    ) -> AuthoredWallMarkResult {
        match self.try_mark_authored_wall(
            registry,
            x,
            y,
            overlay_id,
            anchor_slope_type,
            |_, effect| {
                apply_effect(effect);
                Ok::<(), std::convert::Infallible>(())
            },
        ) {
            Ok(result) => result,
            Err(never) => match never {},
        }
    }

    /// Fallible form used by the production load-effect host. Every callback
    /// completes before native wall Mark advances to the next effect.
    pub(crate) fn try_mark_authored_wall<E>(
        &mut self,
        registry: &OverlayTypeRegistry,
        x: i16,
        y: i16,
        overlay_id: u8,
        anchor_slope_type: u8,
        mut apply_effect: impl FnMut(&mut LiveOverlayCells, AuthoredWallEffect) -> Result<(), E>,
    ) -> Result<AuthoredWallMarkResult, E> {
        const CARDINAL: [(i16, i16); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        const CLEANUP_CROSS: [(i16, i16); 5] = [(0, -1), (1, 0), (0, 1), (-1, 0), (0, 0)];
        const ADJACENT_8: [(i16, i16); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ];

        let anchor = self.cell_ref(x, y);
        let NativeOverlayCellTarget::Real(_) = anchor.target else {
            return Ok(AuthoredWallMarkResult::RejectedUnallocatedAnchor);
        };
        let anchor_coord = self.coord_for_target(anchor.target, anchor.coord);
        if anchor_slope_type > 4 && overlay_id != 0xB2 {
            return Ok(AuthoredWallMarkResult::RejectedSteepSlope);
        }
        if !registry.flags(overlay_id).is_some_and(|flags| flags.wall) {
            return Ok(AuthoredWallMarkResult::RejectedNonWallType);
        }

        // Native writes state before compact identity.
        self.write_state(anchor.target, 0);
        self.write_identity(anchor.target, i32::from(overlay_id));

        for (dx, dy) in CLEANUP_CROSS {
            let visit = self.cell_ref(
                anchor_coord.0.wrapping_add(dx),
                anchor_coord.1.wrapping_add(dy),
            );
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

        for (dx, dy) in ADJACENT_8 {
            let neighbor = self.cell_ref(
                anchor_coord.0.wrapping_add(dx),
                anchor_coord.1.wrapping_add(dy),
            );
            self.wrapping_increment_authored_wall_count(neighbor.target);
            apply_effect(self, AuthoredWallEffect::BlockerCountIncrement(neighbor))?;
        }

        apply_effect(self, AuthoredWallEffect::CommonAnchorRecalc(anchor))?;
        Ok(AuthoredWallMarkResult::Completed)
    }

    pub(crate) fn finish(self) -> FinalizedOverlayPayload {
        FinalizedOverlayPayload {
            width: self.width,
            height: self.height,
            cells: self.cells,
            authored_wall_neighbor_counts: self.authored_wall_neighbor_counts,
        }
    }

    #[cfg(test)]
    pub(crate) fn real_cell(&self, index: usize) -> FinalizedOverlayCell {
        self.cells[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, SharedCellDummy, zone_class};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
    use std::convert::Infallible;

    fn flat_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        let land = LandType::Clear.as_index();
        let speed_costs = SpeedCostProfile::default();
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: true,
            tileset_index: None,
            land_type: land,
            yr_cell_land_type: land,
            slope_type: 0,
            template_height: 0,
            height_in_pixels: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs,
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: true,
            allows_tiberium: false,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: zone_class::GROUND,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: land,
            base_yr_cell_land_type: land,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: speed_costs,
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
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let cells = (0..height)
            .flat_map(|ry| (0..width).map(move |rx| flat_cell(rx, ry)))
            .collect();
        ResolvedTerrainGrid::from_cells(width, height, cells)
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

    #[derive(Default)]
    struct LowHost {
        raw: Vec<u32>,
        next_raw: usize,
        recalcs: Vec<(AuthoredOverlayCellRef, FinalizedOverlayCell)>,
    }

    impl LowHost {
        fn with_raw(raw: impl IntoIterator<Item = u32>) -> Self {
            Self {
                raw: raw.into_iter().collect(),
                ..Self::default()
            }
        }
    }

    impl AuthoredLowMarkHost for LowHost {
        type Error = Infallible;

        fn next_scenario_raw(&mut self) -> u32 {
            let raw = self.raw.get(self.next_raw).copied().unwrap_or(0);
            self.next_raw += 1;
            raw
        }

        fn recalc(
            &mut self,
            cells: &mut LiveOverlayCells,
            cell: AuthoredOverlayCellRef,
        ) -> Result<(), Self::Error> {
            self.recalcs.push((cell, cells.read(cell.target)));
            Ok(())
        }
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
        CellAnim(u32, String, AuthoredOverlayCellRef),
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

        fn observe_recalc(
            &mut self,
            cell: AuthoredOverlayCellRef,
            value: FinalizedOverlayCell,
        ) {
            self.events.push(LoadEvent::Recalc(cell, value));
        }

        fn merge_wall_zone(
            &mut self,
            cell: AuthoredOverlayCellRef,
        ) -> Result<(), Self::Error> {
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
            anim_name: &str,
            cell: AuthoredOverlayCellRef,
            _world_z: i32,
        ) -> Result<(), Self::Error> {
            self.events
                .push(LoadEvent::CellAnim(handle, anim_name.to_string(), cell));
            Ok(())
        }

        fn finish_common(&mut self, handle: Self::Handle) -> Result<(), Self::Error> {
            self.events.push(LoadEvent::FinishCommon(handle));
            Ok(())
        }

        fn finish_slope_survivor(
            &mut self,
            handle: Self::Handle,
        ) -> Result<(), Self::Error> {
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
    fn payload_retains_signed_identity_and_independent_state_until_consumed() {
        let payload = FinalizedOverlayPayload::from_cells_for_test(
            2,
            1,
            vec![(NO_OVERLAY_IDENTITY, 41), (0xee, 9)],
            vec![7, 11],
        );
        let (width, height, cells, counts) = payload.into_parts();

        assert_eq!((width, height), (2, 1));
        assert_eq!(cells[0].overlay_id(), None);
        assert_eq!(cells[0].state(), 41);
        assert_eq!(cells[1].overlay_id(), Some(0xee));
        assert_eq!(cells[1].state(), 9);
        assert_eq!(counts, vec![7, 11]);
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
            LoadEvent::Recalc(_, FinalizedOverlayCell {
                identity: NO_OVERLAY_IDENTITY,
                state: 0,
            })
        )));
        let (_, _, cells, _) = payload.into_parts();
        assert!(cells.iter().all(|cell| *cell == FinalizedOverlayCell::default()));
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
        assert_eq!(null_payload.into_parts().2[8], FinalizedOverlayCell::default());

        let mut slope_terrain = flat_terrain(6, 6);
        slope_terrain.cells[8].slope_type = 5;
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
        assert_eq!(slope_payload.into_parts().2[8], FinalizedOverlayCell::default());
    }

    #[test]
    fn slope_exempt_b2_cellanim_and_crate_last_follow_native_row_order() {
        let shape = NativeOverlayMapShape::new(2, 2);
        let tiberium_types = TiberiumTypeRegistry::default();

        let mut b2_terrain = flat_terrain(6, 6);
        b2_terrain.cells[8].slope_type = 5;
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
            LoadEvent::Recalc(_, FinalizedOverlayCell { identity: 0xB2, state: 0 })
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
                LoadEvent::CellAnim(1, "SPARK".to_string(), real(8, (2, 1))),
                LoadEvent::Recalc(
                    real(8, (2, 1)),
                    FinalizedOverlayCell { identity: 0, state: 0 },
                ),
                LoadEvent::FinishCommon(1),
            ]
        );
        assert_eq!(cell_anim_payload.into_parts().2[8].overlay_id(), Some(0));

        let crate_ini = IniFile::from_str(
            "[OverlayTypes]\n0=CRATE\n[CRATE]\nCrate=yes\n",
        );
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
        assert_eq!(rejected_payload.into_parts().2[8], FinalizedOverlayCell::default());

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
            LoadEvent::Recalc(_, FinalizedOverlayCell { identity: 0, state: u8::MAX })
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
        assert_eq!(
            indices.map(|index| data_cells[index].state()),
            [9, 8, 7, 6]
        );

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
            assert_eq!(host.events[first_scenario + draw * 2], LoadEvent::Scenario(draw as u32));
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
                    terrain.cells[index].bridge_facts.overlay_id,
                    cells[index].overlay_id()
                );
                assert_eq!(terrain.cells[index].bridge_facts.state_byte, state as u8);
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
            terrain.cells[8].slope_type = slope;
            let mut recalc = LoadCellRecalcState::synthetic(&registry, terrain.cells.len());
            let mut cells = LiveOverlayCells::empty_for_terrain(&terrain);
            let target = real(8, (2, 1));
            cells.write(target.target, 0, 7);
            let mut host = LoadHost::default();

            recalc_target(
                &mut terrain,
                &mut recalc,
                &mut cells,
                &mut host,
                target,
            )
            .expect("map-owned resource Recalc");

            assert_eq!(cells.real_cell(8), expected, "slope {slope}");
            assert_eq!(terrain.cells[8].bridge_facts.overlay_id, expected.overlay_id());
            assert_eq!(terrain.cells[8].bridge_facts.state_byte, expected.state());
            assert_eq!(host.events, vec![LoadEvent::Recalc(target, expected)]);
        }
    }

    #[test]
    fn map_owned_recalc_returns_before_reading_or_observing_the_shared_dummy() {
        let mut terrain = flat_terrain(2, 2);
        let before = format!("{:?}", terrain.cells);
        let registry = registry_with_count(1);
        let mut recalc = LoadCellRecalcState::synthetic(&registry, terrain.cells.len());
        let mut cells = LiveOverlayCells::empty_for_terrain(&terrain);
        cells.write(NativeOverlayCellTarget::Dummy, 0, 9);
        let dummy = AuthoredOverlayCellRef {
            target: NativeOverlayCellTarget::Dummy,
            coord: (-1, 3),
        };
        let mut host = LoadHost::default();

        recalc_target(
            &mut terrain,
            &mut recalc,
            &mut cells,
            &mut host,
            dummy,
        )
        .expect("dummy Recalc is a total no-op");

        assert_eq!(
            cells.read(NativeOverlayCellTarget::Dummy),
            FinalizedOverlayCell::from_parts(0, 9)
        );
        assert_eq!(format!("{:?}", terrain.cells), before);
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
                        if cell.coord == (7, y as i16) && value.overlay_id() == Some(later_dir0) =>
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
            assert_eq!(terrain.cells[anchor_index].bridge_facts.state_byte, 9);
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
        assert!(host.events[15..23]
            .iter()
            .all(|event| matches!(event, LoadEvent::BlockerIncrement(_))));
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
        terrain.cells[14].zone_type = zone_class::WALL;
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
        let terrain = flat_terrain(12, 12);
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
            let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
            let anchor = live.cell_ref(5, 5);
            live.write(anchor.target, 0, 1);
            for &coord in &neighbors[..matching] {
                let target = live.target(coord.0, coord.1);
                live.write(target, 1, u8::MAX);
            }
            live.germinate_authored_tiberium(&overlay_types, &tiberium_types, anchor);
            assert_eq!(live.read(anchor.target).state(), expected[matching]);
        }

        let mut land_only = LiveOverlayCells::empty_for_terrain(&terrain);
        let anchor = land_only.cell_ref(5, 5);
        land_only.write(anchor.target, 2, 1);
        land_only.germinate_authored_tiberium(&overlay_types, &tiberium_types, anchor);
        assert_eq!(
            land_only.read(anchor.target).state(),
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
        let anchor = live.cell_ref(0, 0);
        live.write(anchor.target, 0, 1);

        live.germinate_authored_tiberium(&overlay_types, &tiberium_types, anchor);

        assert_eq!(live.read(anchor.target).state(), 11);
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
        let terrain = flat_terrain(20, 20);
        let shape = NativeOverlayMapShape::new(10, 10);
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
            let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
            let mut host = LowHost::default();
            assert_eq!(
                live.try_mark_authored_low(shape, 8, 8, trigger, &mut host),
                Ok(Some(AuthoredLowMarkResult::FixedEndWithoutOpposite))
            );
            assert_eq!(host.next_raw, 0, "trigger {trigger:#04X}");
            assert_eq!(host.recalcs.len(), 3, "trigger {trigger:#04X}");
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
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let occupied = live.target(8, 7);
        live.write(occupied, 3, 4);
        let mut host = LowHost::default();

        assert_eq!(
            live.try_mark_authored_low(shape, 8, 8, 0x7B, &mut host),
            Ok(Some(AuthoredLowMarkResult::OccupiedFixedRow))
        );
        assert_eq!(host.next_raw, 0);
        assert!(host.recalcs.is_empty());
        assert_eq!(terrain.dummy_cell_requested_coord(), (8, 9));
        assert_eq!(live.read(occupied).overlay_id(), Some(3));
        assert_eq!(live.read(occupied).state(), 4);
    }

    #[test]
    fn authored_low_body_runs_opposite_to_trigger_with_three_raw_draws_per_row() {
        let terrain = flat_terrain(24, 24);
        let shape = NativeOverlayMapShape::new(12, 12);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let opposite = live.target(12, 8);
        live.write(opposite, 0x5C, 1);
        let mut host = LowHost::with_raw(0..9);

        assert_eq!(
            live.try_mark_authored_low(shape, 8, 8, 0x7B, &mut host),
            Ok(Some(AuthoredLowMarkResult::BodyRows {
                rows: 3,
                scenario_draws: 9,
            }))
        );
        assert_eq!(host.next_raw, 9);
        assert_eq!(host.recalcs.len(), 12, "three fixed plus nine body");
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
        let terrain = flat_terrain(24, 24);
        let shape = NativeOverlayMapShape::new(10, 10);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let opposite = live.target(8, 4);
        live.write(opposite, 0x60, 1);
        let mut host = LowHost::with_raw(0..9);

        assert_eq!(
            live.try_mark_authored_low(shape, 8, 8, 0x7D, &mut host),
            Ok(Some(AuthoredLowMarkResult::BodyRows {
                rows: 3,
                scenario_draws: 9,
            }))
        );
        assert_eq!(host.next_raw, 9);
        assert_eq!(host.recalcs.len(), 12, "three fixed plus nine body");
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
        let terrain = flat_terrain(24, 24);
        let shape = NativeOverlayMapShape::new(12, 12);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let wrong_id = live.target(9, 8);
        let wrong_state = live.target(10, 8);
        let first_match = live.target(11, 8);
        let later_match = live.target(13, 8);
        live.write(wrong_id, 0x5E, 1);
        live.write(wrong_state, 0x5C, 2);
        live.write(first_match, 0x5C, 1);
        live.write(later_match, 0x5C, 1);
        let mut host = LowHost::with_raw(0..6);

        assert_eq!(
            live.try_mark_authored_low(shape, 8, 8, 0x7B, &mut host),
            Ok(Some(AuthoredLowMarkResult::BodyRows {
                rows: 2,
                scenario_draws: 6,
            }))
        );
        assert_eq!(host.next_raw, 6);
        assert_eq!(host.recalcs.len(), 9, "three fixed plus six body");
        assert_eq!(live.read(first_match).overlay_id(), Some(0x5C));
        assert_eq!(live.read(first_match).state(), 1);
        assert_eq!(live.read(later_match).overlay_id(), Some(0x5C));
        assert_eq!(live.read(later_match).state(), 1);
    }

    #[test]
    fn authored_low_recalc_failure_stops_before_later_writes_or_rng() {
        #[derive(Default)]
        struct FailingHost {
            recalc_calls: usize,
            raw_calls: usize,
        }

        impl AuthoredLowMarkHost for FailingHost {
            type Error = &'static str;

            fn next_scenario_raw(&mut self) -> u32 {
                self.raw_calls += 1;
                0
            }

            fn recalc(
                &mut self,
                _cells: &mut LiveOverlayCells,
                _cell: AuthoredOverlayCellRef,
            ) -> Result<(), Self::Error> {
                self.recalc_calls += 1;
                if self.recalc_calls == 2 {
                    Err("injected Recalc failure")
                } else {
                    Ok(())
                }
            }
        }

        let terrain = flat_terrain(20, 20);
        let shape = NativeOverlayMapShape::new(10, 10);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let mut host = FailingHost::default();

        assert_eq!(
            live.try_mark_authored_low(shape, 8, 8, 0x7B, &mut host),
            Err("injected Recalc failure")
        );
        assert_eq!(host.recalc_calls, 2);
        assert_eq!(host.raw_calls, 0);
        assert_eq!(live.read(live.target(8, 7)).overlay_id(), Some(0x5E));
        assert_eq!(live.read(live.target(8, 8)).overlay_id(), Some(0x5E));
        assert_eq!(
            live.read(live.target(8, 9)),
            FinalizedOverlayCell::default(),
            "the third fixed write must not run after the synchronous failure"
        );
    }

    #[test]
    fn authored_low_adjacent_centers_overwrite_the_new_fixed_row() {
        let terrain = flat_terrain(20, 20);
        let shape = NativeOverlayMapShape::new(10, 10);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let opposite = live.target(9, 8);
        live.write(opposite, 0x5C, 1);
        let mut host = LowHost::with_raw([3, 2, 1]);

        assert_eq!(
            live.try_mark_authored_low(shape, 8, 8, 0x7B, &mut host),
            Ok(Some(AuthoredLowMarkResult::BodyRows {
                rows: 1,
                scenario_draws: 3,
            }))
        );
        for (state, (y, id)) in [(7i16, 0x4D), (8, 0x4C), (9, 0x4B)]
            .into_iter()
            .enumerate()
        {
            let cell = live.read(live.target(8, y));
            assert_eq!(cell.overlay_id(), Some(id));
            assert_eq!(cell.state(), state as u8);
        }
    }

    #[test]
    fn authored_low_search_length_uses_requested_coordinate_before_real_alias_canonicalization() {
        let terrain = flat_terrain(512, 4);
        let shape = NativeOverlayMapShape::new(512, 2);
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let aliased_opposite = live.target(512, 2);
        assert_eq!(aliased_opposite, NativeOverlayCellTarget::Real(1536));
        live.write(aliased_opposite, 0x5C, 1);
        let mut host = LowHost::with_raw([0, 1, 2]);

        assert_eq!(
            live.try_mark_authored_low(shape, 511, 2, 0x7B, &mut host),
            Ok(Some(AuthoredLowMarkResult::BodyRows {
                rows: 1,
                scenario_draws: 3,
            }))
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
        let terrain = flat_terrain(5, 5);
        let registry = wall_registry();
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let north = live.target(2, 1);
        let east = live.target(3, 2);
        live.write(north, 0, 0);
        live.write(east, 2, 0);

        let mut effects = Vec::new();
        assert_eq!(
            live.mark_authored_wall(&registry, 2, 2, 0, 0, |effect| {
                effects.push(effect)
            }),
            AuthoredWallMarkResult::Completed
        );

        assert_eq!(
            effects,
            vec![
                AuthoredWallEffect::TacticalDirty(real(7, (2, 1))),
                AuthoredWallEffect::RadarDirty(real(7, (2, 1))),
                AuthoredWallEffect::CleanupRecalcAndZone(real(7, (2, 1))),
                AuthoredWallEffect::TacticalDirty(real(13, (3, 2))),
                AuthoredWallEffect::RadarDirty(real(13, (3, 2))),
                AuthoredWallEffect::CleanupRecalcAndZone(real(13, (3, 2))),
                AuthoredWallEffect::TacticalDirty(real(17, (2, 3))),
                AuthoredWallEffect::RadarDirty(real(17, (2, 3))),
                AuthoredWallEffect::TacticalDirty(real(11, (1, 2))),
                AuthoredWallEffect::RadarDirty(real(11, (1, 2))),
                AuthoredWallEffect::TacticalDirty(real(12, (2, 2))),
                AuthoredWallEffect::RadarDirty(real(12, (2, 2))),
                AuthoredWallEffect::CleanupRecalcAndZone(real(12, (2, 2))),
                AuthoredWallEffect::BlockerCountIncrement(real(7, (2, 1))),
                AuthoredWallEffect::BlockerCountIncrement(real(8, (3, 1))),
                AuthoredWallEffect::BlockerCountIncrement(real(13, (3, 2))),
                AuthoredWallEffect::BlockerCountIncrement(real(18, (3, 3))),
                AuthoredWallEffect::BlockerCountIncrement(real(17, (2, 3))),
                AuthoredWallEffect::BlockerCountIncrement(real(16, (1, 3))),
                AuthoredWallEffect::BlockerCountIncrement(real(11, (1, 2))),
                AuthoredWallEffect::BlockerCountIncrement(real(6, (1, 1))),
                AuthoredWallEffect::CommonAnchorRecalc(real(12, (2, 2))),
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
        terrain.cell_mut(1, 1).expect("anchor").slope_type = 5;
        let registry = wall_registry();
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        let mut effects = Vec::new();

        assert_eq!(
            live.mark_authored_wall(&registry, 1, 1, 0, 5, |effect| {
                effects.push(effect)
            }),
            AuthoredWallMarkResult::RejectedSteepSlope
        );
        assert!(effects.is_empty());
        assert_eq!(live.real_cell(4), FinalizedOverlayCell::default());
        assert!(
            live.authored_wall_neighbor_counts
                .iter()
                .all(|&count| count == 0)
        );
    }

    #[test]
    fn authored_wall_retains_wrapping_alias_counts_across_data_and_low_body_overwrite() {
        let terrain = flat_terrain(512, 2);
        let registry = wall_registry();
        let mut live = LiveOverlayCells::empty_for_terrain(&terrain);
        // West of (0,1) linearizes to fixed slot 511 -> real (511,0).
        live.authored_wall_neighbor_counts[511] = u8::MAX;
        let mut effects = Vec::new();
        assert_eq!(
            live.mark_authored_wall(&registry, 0, 1, 0, 0, |effect| {
                effects.push(effect)
            }),
            AuthoredWallMarkResult::Completed
        );
        assert_eq!(live.authored_wall_neighbor_counts[511], 0);
        assert_eq!(
            effects
                .iter()
                .filter(|effect| matches!(
                    effect,
                    AuthoredWallEffect::BlockerCountIncrement(AuthoredOverlayCellRef {
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
