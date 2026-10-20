//! Native-shaped CellRect passability and occupancy validators, and the
//! shared cell leaf `CellClass::CheckCellPassability @ 0x004834A0`
//! ([`check_cell_passability`]) that the passability rectangle and every
//! single-cell caller use.
//!
//! These are read-only substrate facades over the existing Rust grids. They do
//! not collapse terrain, overlays, reservations, and object-list occupancy into
//! one store; the point is to expose the two distinct gamemd validator surfaces
//! while preserving the current Rust-native ownership split.

use std::collections::BTreeMap;

use crate::map::playfield::{lepton_to_packed_cell_component, rect_playfield_corners};
#[cfg(any(test, feature = "test-support"))]
use crate::map::resolved_terrain::SharedCellDummySnapshot;
use crate::map::resolved_terrain::{
    OverlayPassability, ResolvedTerrainCell, ResolvedTerrainGrid, SharedCellDummy, zone_class,
};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::terrain_rules::{LandType, SpeedCostProfile};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::util::direction::DIRECTION_DELTAS;

// Fixed cell indexing is map-owned (map::cell_index, F05); sim re-exports
// so runtime consumers keep their paths.
pub use crate::map::cell_index::{CELL_ROW_STRIDE, MAX_CELL_INDEX, cell_linear_index};
pub(crate) use crate::map::cell_index::{canonical_cell_coord, packed_cell_coord};
pub use crate::map::playfield::PlayfieldBounds;

/// A non-null cell reference — `Real` for an in-range, present cell, or `Dummy`
/// viewing the one live fallback CellClass identity after an out-of-range /
/// missing lookup.
///
/// Never the absence of a value: the engine's coord→cell lookup returns a
/// non-null dummy that stores the requested coord and lets the caller keep
/// dispatching on it. Coordinate writes do not reconstruct or clear its
/// independently persistent level/slope state.
#[derive(Debug, Clone)]
pub enum CellRef<'a> {
    Real(&'a ResolvedTerrainCell),
    Dummy { cell: SharedCellDummy },
}

// `ResolvedTerrainCell` is not `PartialEq`; compare `Real` by pointer identity
// (same backing cell) and `Dummy` by the process-global CellClass identity.
impl PartialEq for CellRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (CellRef::Real(a), CellRef::Real(b)) => std::ptr::eq(*a, *b),
            (CellRef::Dummy { cell: a }, CellRef::Dummy { cell: b }) => a.same_identity(b),
            _ => false,
        }
    }
}
impl Eq for CellRef<'_> {}

impl CellRef<'_> {
    #[cfg(any(test, feature = "test-support"))]
    pub fn dummy_snapshot(&self) -> Option<SharedCellDummySnapshot> {
        match self {
            CellRef::Real(_) => None,
            CellRef::Dummy { cell } => Some(cell.snapshot()),
        }
    }

    /// `CellClass+0x11B`, the signed level of the real cell or of the shared
    /// dummy the stamping lookup selected.
    pub(crate) fn signed_level(&self) -> i8 {
        match self {
            CellRef::Real(cell) => cell.level as i8,
            CellRef::Dummy { cell } => cell.snapshot().level,
        }
    }

    /// Live `CellClass+0x140 & 0x1180` view for consumers that must inspect
    /// the same real-or-dummy result selected by the stamping lookup.
    pub(crate) fn bridge_flags_0x1180(&self) -> u32 {
        match self {
            CellRef::Real(cell) => {
                cell.bridge_facts.raw_flags
                    & crate::map::bridge_facts::MODELED_CELLCLASS_BRIDGE_FLAG_MASK
            }
            CellRef::Dummy { cell } => cell.bridge_flags_0x1180(),
        }
    }
}

fn detached_dummy(x: i32, y: i32) -> SharedCellDummy {
    let dummy = SharedCellDummy::fresh();
    dummy.stamp_coord(x, y);
    dummy
}

/// Engine `Get_CellClass`: coord → cell via the fixed stride; an out-of-range or
/// missing cell returns `CellRef::Dummy` viewing the packed requested coord and
/// preserved shared fallback bytes (NOT `(0,0)`, NOT `None`). The width-based
/// `PathGrid`/`ResolvedTerrainGrid` index stays as the cache; this is the
/// never-null parity lookup. Components are not checked separately: any valid
/// linear index aliases its canonical 512-wide slot.
pub fn get_cellclass_fallback<'a>(
    terrain: Option<&'a ResolvedTerrainGrid>,
    x: i32,
    y: i32,
) -> CellRef<'a> {
    let (x, y) = packed_cell_coord(x, y);
    let Some(terrain) = terrain else {
        return CellRef::Dummy {
            cell: detached_dummy(x, y),
        };
    };
    let query = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    native_cell_ref(&query, query.lookup((x as i16, y as i16)))
}

/// The view of one identity a Map lookup returned through `query`.
fn native_cell_ref<'a>(
    query: &crate::map::resolved_terrain::NativeCellQuery<'a>,
    cell: crate::map::cell_index::NativeCellIdentity,
) -> CellRef<'a> {
    match cell {
        crate::map::cell_index::NativeCellIdentity::Real(index) => {
            CellRef::Real(&query.terrain().cells()[index])
        }
        crate::map::cell_index::NativeCellIdentity::Dummy => CellRef::Dummy {
            cell: query.dummy(),
        },
    }
}

/// The view of `cell`, an identity the canonical lookup on `terrain` returned.
pub(crate) fn canonical_cell_ref(
    terrain: &ResolvedTerrainGrid,
    cell: crate::map::cell_index::NativeCellIdentity,
) -> CellRef<'_> {
    native_cell_ref(
        &crate::map::resolved_terrain::NativeCellQuery::canonical(terrain),
        cell,
    )
}

pub(crate) fn get_cellclass_in_query<'a>(
    terrain: Option<&'a ResolvedTerrainGrid>,
    x: i32,
    y: i32,
    query: Option<&crate::map::resolved_terrain::NativeCellQuery<'a>>,
) -> CellRef<'a> {
    let Some(query) = query else {
        return get_cellclass_fallback(terrain, x, y);
    };
    debug_assert!(terrain.is_some_and(|terrain| std::ptr::eq(terrain, query.terrain())));
    native_cell_ref(query, query.lookup((x as i16, y as i16)))
}

/// Engine world/lepton coordinate lookup, `MapClass::Get_CellClass @
/// 0x00565730`, as a [`CellRef`] view of the one port,
/// [`NativeCellQuery::lookup_world`](crate::map::resolved_terrain::NativeCellQuery::lookup_world):
/// full signed-i32 `/256` quotients and wrapping fixed-stride index
/// arithmetic. A real slot leaves the shared dummy untouched, while either an
/// invalid slot or a null entry stamps the converted packed coordinate before
/// returning the dummy.
pub fn get_cellclass_fallback_leptons<'a>(
    terrain: Option<&'a ResolvedTerrainGrid>,
    x_leptons: i32,
    y_leptons: i32,
) -> CellRef<'a> {
    let Some(terrain) = terrain else {
        let (x, y) = packed_cell_coord(x_leptons / 256, y_leptons / 256);
        return CellRef::Dummy {
            cell: detached_dummy(x, y),
        };
    };
    let query = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    native_cell_ref(&query, query.lookup_world(x_leptons, y_leptons))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The seven arguments of `CellClass::CheckCellPassability @ 0x004834A0`,
/// in native order: `(SpeedType, ignoreInfantry, ignoreVehicles, zone,
/// MovementZone, level, isBridge)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellPassabilityArgs {
    pub speed_type: SpeedType,
    pub ignore_infantry: bool,
    pub ignore_vehicles: bool,
    /// `None` is native zone `-1`: no zone comparison.
    pub zone: Option<u32>,
    pub movement_zone: MovementZone,
    /// `None` is native level `-1`: the deck of a bridge cell, else ground.
    pub level: Option<i16>,
    /// Forwarded to the nested `GetZoneID`, and admits the ground plane of a
    /// bridge cell at its base level.
    pub is_bridge: bool,
}

/// The live map [`check_cell_passability`] reads through one CellClass.
#[derive(Clone, Copy)]
pub(crate) struct CellPassabilityMap<'a> {
    /// The CellClass array. `None` only before Fill, when every CellClass
    /// still holds constructor defaults (pre-Fill `Gather_Start_Positions`).
    pub terrain: Option<&'a ResolvedTerrainGrid>,
    /// Owner of a real cell's `+0x44` overlay identity.
    pub overlays: Option<&'a OverlayGrid>,
    /// `MapClass::GetZoneID @ 0x0056D230` for a zone argument other than -1.
    pub zones: Option<&'a ZoneGrid>,
    /// The query that resolved the cell, reused by the nested zone lookup.
    pub native_cells: Option<&'a crate::map::resolved_terrain::NativeCellQuery<'a>>,
    /// Live `+0x124`/`+0x128` occupation planes.
    pub raw_occupation: Option<&'a RawCellOccupationGrid>,
    /// Without raw planes, a listed object on a layer projects to bit 0x40.
    pub listed_occupancy: Option<&'a OccupancyGrid>,
}

impl crate::sim::world::Simulation {
    /// The live map's cells, overlays, zones and raw occupation planes, for
    /// [`check_cell_passability`] on a cell from the canonical lookup.
    pub(crate) fn cell_passability_map(&self) -> CellPassabilityMap<'_> {
        CellPassabilityMap {
            terrain: self.resolved_terrain.as_ref(),
            overlays: self.overlay_grid.as_ref(),
            zones: self.zone_grid.as_ref(),
            native_cells: None,
            raw_occupation: Some(&self.substrate.raw_cell_occupation),
            listed_occupancy: None,
        }
    }
}

/// Deterministic result of `CellClass::CheckCellPassability @ 0x004834A0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IsClearToMoveResult {
    /// Winged returns before CellClass selects a terrain or occupation plane.
    ClearWinged,
    Clear {
        selected_layer: MovementLayer,
    },
    ZoneMismatch,
    LevelMismatch,
    Occupied {
        remaining_bits: u8,
    },
    WallBlocked,
    LandBlocked,
}

impl IsClearToMoveResult {
    /// Whether `0x004834A0` returns true.
    pub(crate) fn admits(self) -> bool {
        matches!(self, Self::ClearWinged | Self::Clear { .. })
    }
}

/// `CellClass::CheckCellPassability @ 0x004834A0` on `cell`, the CellClass
/// its caller looked up. Every port of a native call to `0x004834A0` comes
/// here, directly or through [`check_rect_cell_passability`]
/// (`CellRect::CheckPassability @ 0x0056E7C0`). In order:
/// - Winged admits before any Cell read (`0x004834A7`);
/// - a zone other than -1 must equal `GetZoneID(+0x24, MovementZone,
///   isBridge)` (`0x004834BF..0x004834D6`);
/// - the level selects the ground (`+0x124`) or deck (`+0x128`) occupation
///   byte, or refuses (`0x004834E1..0x00483552`); the ignore arguments mask
///   it with `0xE0`/`0x5F` and any bit left refuses (`0x00483558..0x00483572`);
/// - the land half reads the cell ([`CellLand`], `0x0048357D..0x004835F4`).
pub(crate) fn check_cell_passability(
    cell: &CellRef<'_>,
    args: CellPassabilityArgs,
    map: &CellPassabilityMap<'_>,
) -> IsClearToMoveResult {
    if args.speed_type == SpeedType::Winged {
        return IsClearToMoveResult::ClearWinged;
    }
    let requested_zone = args.zone.filter(|&zone| zone != u32::MAX);
    // Raw row 0xFFFF and the missing-record DWORD 0xFFFFFFFF are distinct;
    // neither is projected through reduced layers. No answer never matches.
    let actual_zone = match (requested_zone, map.terrain, map.zones) {
        (None, _, _) => 0,
        (Some(_), Some(terrain), Some(zones)) => {
            let coord = match cell {
                CellRef::Real(cell) => (cell.rx, cell.ry),
                CellRef::Dummy { cell } => {
                    let (x, y) = cell.snapshot().coord;
                    (x as u16, y as u16)
                }
            };
            zones
                .get_zone_id_native_in_query(
                    terrain,
                    coord,
                    args.movement_zone,
                    args.is_bridge,
                    map.native_cells,
                )
                .unwrap_or(u32::MAX)
        }
        (Some(_), _, _) => u32::MAX,
    };
    let canonical = match cell {
        CellRef::Real(cell) => Some((cell.rx, cell.ry)),
        CellRef::Dummy { .. } => None,
    };
    let (ground_occupation_bits, deck_occupation_bits) = match map.raw_occupation {
        Some(raw) => {
            let key = canonical.map_or(crate::sim::occupancy::RawCellKey::Dummy, |(x, y)| {
                crate::sim::occupancy::RawCellKey::Real(x, y)
            });
            (
                raw.bits_at(key, MovementLayer::Ground),
                raw.bits_at(key, MovementLayer::Bridge),
            )
        }
        None => {
            let listed = |layer| {
                u8::from(canonical.is_some_and(|(x, y)| {
                    map.listed_occupancy
                        .is_some_and(|grid| grid.count_on_layer(x, y, layer) > 0)
                })) * 0x40
            };
            (listed(MovementLayer::Ground), listed(MovementLayer::Bridge))
        }
    };
    evaluate_is_clear_to_move(IsClearToMoveRequest {
        speed_type: args.speed_type,
        movement_zone: args.movement_zone,
        requested_zone,
        actual_zone,
        base_level: i16::from(cell.signed_level()),
        has_bridge: cell.bridge_flags_0x1180() & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
            != 0,
        requested_level: args.level.filter(|&level| level != -1),
        is_bridge: args.is_bridge,
        ground_occupation_bits,
        deck_occupation_bits,
        ignore_infantry: args.ignore_infantry,
        ignore_vehicles: args.ignore_vehicles,
        land: map
            .terrain
            .map(|terrain| CellLand::read(cell, terrain, map.overlays)),
    })
}

/// What the land half of `0x004834A0` reads from its CellClass
/// (`0x0048357D..0x004835DE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellLand {
    /// The row of table `0x0089EA40` the cell's Land (`+0xEC`) selects. A
    /// real cell keeps its resolved row; the shared Dummy keeps only its
    /// Land and indexes the grid's projection of the same rules.
    row: SpeedCostProfile,
    /// The Clear row, which a wall the mover passes switches to
    /// (`0x004835D3`).
    clear_row: SpeedCostProfile,
    /// `Wall=`/`Crushable=` of the type at `+0x44`: the OverlayGrid holds a
    /// real cell's identity, the Dummy its own.
    overlay: Option<OverlayPassability>,
}

impl CellLand {
    fn read(
        cell: &CellRef<'_>,
        terrain: &ResolvedTerrainGrid,
        overlays: Option<&OverlayGrid>,
    ) -> Self {
        let (row, overlay) = match cell {
            CellRef::Real(cell) => (
                cell.speed_costs,
                overlays
                    .and_then(|grid| grid.cell(cell.rx, cell.ry).overlay_id)
                    .map_or(-1, i32::from),
            ),
            CellRef::Dummy { cell } => (
                terrain.land_speed_costs(cell.land_type()),
                cell.overlay_identity_state().0,
            ),
        };
        Self {
            row,
            clear_row: terrain.land_speed_costs(i32::from(LandType::Clear.as_index())),
            overlay: terrain.overlay_passability(overlay),
        }
    }
}

/// The values `0x004834A0` decides on, once read from its CellClass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IsClearToMoveRequest {
    speed_type: SpeedType,
    movement_zone: MovementZone,
    /// `None` corresponds to native zone `-1` (no zone comparison).
    requested_zone: Option<u32>,
    actual_zone: u32,
    /// Native signed `CellClass+0x11B` base level.
    base_level: i16,
    /// Native `CellClass::Flags & 0x100` bridge gate.
    has_bridge: bool,
    /// `None` corresponds to native level `-1` (select bridge occupation on a bridge cell).
    requested_level: Option<i16>,
    is_bridge: bool,
    /// Native normal `OccupationFlags+0x124` low byte.
    ground_occupation_bits: u8,
    /// Native alternate `AltOccupationFlags+0x128` low byte.
    deck_occupation_bits: u8,
    ignore_infantry: bool,
    ignore_vehicles: bool,
    /// `None` without a CellClass array (pre-Fill): VERA admits. Native's
    /// constructor-default cells read the Clear row, which refuses Float and
    /// FloatBeach; the one such caller, the pre-Fill start search, passes
    /// Track, which that row admits.
    land: Option<CellLand>,
}

fn evaluate_is_clear_to_move(input: IsClearToMoveRequest) -> IsClearToMoveResult {
    if input.speed_type == SpeedType::Winged {
        return IsClearToMoveResult::ClearWinged;
    }

    if input
        .requested_zone
        .is_some_and(|requested| requested != input.actual_zone)
    {
        return IsClearToMoveResult::ZoneMismatch;
    }

    let selected_layer = match input.requested_level {
        Some(level) if level == input.base_level => {
            if input.has_bridge && !input.is_bridge {
                return IsClearToMoveResult::LevelMismatch;
            }
            MovementLayer::Ground
        }
        Some(level) if input.has_bridge && level == input.base_level.saturating_add(4) => {
            MovementLayer::Bridge
        }
        Some(_) => return IsClearToMoveResult::LevelMismatch,
        None if input.has_bridge => MovementLayer::Bridge,
        None => MovementLayer::Ground,
    };

    let mut remaining_bits = match selected_layer {
        MovementLayer::Bridge => input.deck_occupation_bits,
        _ => input.ground_occupation_bits,
    };
    if input.ignore_infantry {
        remaining_bits &= 0xE0;
    }
    if input.ignore_vehicles {
        remaining_bits &= 0x5F;
    }
    if remaining_bits != 0 {
        return IsClearToMoveResult::Occupied { remaining_bits };
    }

    let Some(land) = input.land else {
        return IsClearToMoveResult::Clear { selected_layer };
    };
    // A Wall= type admits Destroyer, AmphibiousDestroyer, InfantryDestroyer
    // (`0x004835A2..0x004835AC`) and CrusherAll (`0x004835C5`); Crusher and
    // AmphibiousCrusher only when it is Crushable= (`0x004835B1..0x004835C3`).
    // The test precedes the deck bypass, so a wall refuses on a deck too.
    let row = match land.overlay {
        Some(overlay) if overlay.wall => {
            let passes = matches!(
                input.movement_zone,
                MovementZone::Destroyer
                    | MovementZone::AmphibiousDestroyer
                    | MovementZone::InfantryDestroyer
                    | MovementZone::CrusherAll
            ) || (overlay.crushable
                && matches!(
                    input.movement_zone,
                    MovementZone::Crusher | MovementZone::AmphibiousCrusher
                ));
            if !passes {
                return IsClearToMoveResult::WallBlocked;
            }
            land.clear_row
        }
        _ => land.row,
    };
    // `0x004835DE..0x004835F4`: a cost equal to 0.0 refuses unless the deck
    // was selected. A row the scenario rules lack reads as 0.0.
    if selected_layer != MovementLayer::Bridge
        && row.cost_for_speed_type(input.speed_type).unwrap_or(0) == 0
    {
        return IsClearToMoveResult::LandBlocked;
    }
    IsClearToMoveResult::Clear { selected_layer }
}

impl CellRect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn single(rx: u16, ry: u16) -> Self {
        Self::new(rx as i32, ry as i32, 1, 1)
    }
}

/// Native CellRect scan: x outer, y inner, wrapping endpoints and signed
/// comparisons. Returning false from `visit` stops at the first failed cell.
pub(crate) fn scan_cell_rect(rect: CellRect, mut visit: impl FnMut(i32, i32) -> bool) -> bool {
    let end_x = rect.x.wrapping_add(rect.width);
    let end_y = rect.y.wrapping_add(rect.height);
    let mut x = rect.x;
    while x < end_x {
        let mut y = rect.y;
        while y < end_y {
            if !visit(x, y) {
                return false;
            }
            y = y.wrapping_add(1);
        }
        x = x.wrapping_add(1);
    }
    true
}

/// Sparse authority for CellClass `+0xDC`. A fixed-stride coordinate selects a
/// real entry only when the active terrain has an allocated CellClass pointer;
/// every invalid or valid-but-unallocated lookup shares `dummy_mask`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CellReservationGrid {
    masks: BTreeMap<(u16, u16), u32>,
    /// Native save iterates allocated CellClass pointers only. The process-global
    /// fallback CellClass is reconstructed cleared during load/Resize.
    #[serde(skip)]
    dummy_mask: u32,
}

#[derive(Debug, Clone)]
enum ReservationCellSelection {
    Real(u16, u16),
    SharedDummy(SharedCellDummy),
    /// Unit-level/detached callers have no persistent shared dummy coordinate,
    /// but retain the existing real fixed-stride versus fallback mask split.
    DetachedDummy {
        requested: (i32, i32),
    },
}

impl ReservationCellSelection {
    fn live_coord(&self) -> (i32, i32) {
        match self {
            Self::Real(rx, ry) => (i32::from(*rx), i32::from(*ry)),
            Self::SharedDummy(cell) => cell.snapshot().coord,
            Self::DetachedDummy { requested } => *requested,
        }
    }
}

fn resolve_reservation_cell(
    terrain: Option<&ResolvedTerrainGrid>,
    x: i32,
    y: i32,
) -> ReservationCellSelection {
    let Some(terrain) = terrain else {
        return canonical_cell_coord(x, y).map_or_else(
            || ReservationCellSelection::DetachedDummy {
                requested: packed_cell_coord(x, y),
            },
            |(rx, ry)| ReservationCellSelection::Real(rx, ry),
        );
    };
    match get_cellclass_fallback(Some(terrain), x, y) {
        CellRef::Real(cell) => ReservationCellSelection::Real(cell.rx, cell.ry),
        CellRef::Dummy { cell } => ReservationCellSelection::SharedDummy(cell),
    }
}

pub(crate) fn resolve_reservation_real_cell(
    terrain: Option<&ResolvedTerrainGrid>,
    x: i32,
    y: i32,
) -> Option<(u16, u16)> {
    match resolve_reservation_cell(terrain, x, y) {
        ReservationCellSelection::Real(rx, ry) => Some((rx, ry)),
        ReservationCellSelection::SharedDummy(_)
        | ReservationCellSelection::DetachedDummy { .. } => None,
    }
}

impl CellReservationGrid {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reserve(
        &mut self,
        terrain: Option<&ResolvedTerrainGrid>,
        x: i32,
        y: i32,
        reservation_arg: i32,
    ) {
        let mask = reservation_mask(reservation_arg);
        if mask == 0 {
            return;
        }
        match resolve_reservation_cell(terrain, x, y) {
            ReservationCellSelection::Real(rx, ry) => {
                *self.masks.entry((rx, ry)).or_default() |= mask;
            }
            ReservationCellSelection::SharedDummy(_)
            | ReservationCellSelection::DetachedDummy { .. } => self.dummy_mask |= mask,
        }
    }

    pub fn clear(
        &mut self,
        terrain: Option<&ResolvedTerrainGrid>,
        x: i32,
        y: i32,
        reservation_arg: i32,
    ) {
        let mask = reservation_mask(reservation_arg);
        if mask == 0 {
            return;
        }
        match resolve_reservation_cell(terrain, x, y) {
            ReservationCellSelection::Real(rx, ry) => {
                let cell = (rx, ry);
                if let Some(bits) = self.masks.get_mut(&cell) {
                    *bits &= !mask;
                    if *bits == 0 {
                        self.masks.remove(&cell);
                    }
                }
            }
            ReservationCellSelection::SharedDummy(_)
            | ReservationCellSelection::DetachedDummy { .. } => self.dummy_mask &= !mask,
        }
    }

    pub fn raw_mask(&self, terrain: Option<&ResolvedTerrainGrid>, x: i32, y: i32) -> u32 {
        self.raw_mask_for_selection(&resolve_reservation_cell(terrain, x, y))
    }

    fn raw_mask_for_selection(&self, cell: &ReservationCellSelection) -> u32 {
        match cell {
            ReservationCellSelection::Real(rx, ry) => {
                self.masks.get(&(*rx, *ry)).copied().unwrap_or(0)
            }
            ReservationCellSelection::SharedDummy(_)
            | ReservationCellSelection::DetachedDummy { .. } => self.dummy_mask,
        }
    }

    pub(crate) fn raw_mask_for_cell_ref(&self, cell: &CellRef<'_>) -> u32 {
        match cell {
            CellRef::Real(cell) => self.masks.get(&(cell.rx, cell.ry)).copied().unwrap_or(0),
            CellRef::Dummy { .. } => self.dummy_mask,
        }
    }

    pub fn has_reservation(
        &self,
        terrain: Option<&ResolvedTerrainGrid>,
        x: i32,
        y: i32,
        reservation_arg: i32,
    ) -> bool {
        let mask = reservation_mask(reservation_arg);
        mask != 0 && self.raw_mask(terrain, x, y) & mask != 0
    }

    pub(crate) fn reserve_rect(
        &mut self,
        terrain: Option<&ResolvedTerrainGrid>,
        rect: CellRect,
        reservation_arg: i32,
    ) {
        scan_cell_rect(rect, |x, y| {
            self.reserve(terrain, x, y, reservation_arg);
            true
        });
    }

    pub(crate) fn clear_rect(
        &mut self,
        terrain: Option<&ResolvedTerrainGrid>,
        rect: CellRect,
        reservation_arg: i32,
    ) {
        scan_cell_rect(rect, |x, y| {
            self.clear(terrain, x, y, reservation_arg);
            true
        });
    }

    /// Same-house reservation connectivity around a center cell. Bits are
    /// N, NE, E, SE, S, SW, W, NW. A center without the requested house bit
    /// returns the native `-1` sentinel as `u32::MAX`.
    pub fn house_reservation_neighbor_mask(
        &self,
        terrain: Option<&ResolvedTerrainGrid>,
        x: i32,
        y: i32,
        reservation_arg: i32,
    ) -> u32 {
        let mask = reservation_mask(reservation_arg);
        if mask == 0 {
            return u32::MAX;
        }
        let center = resolve_reservation_cell(terrain, x, y);
        if self.raw_mask_for_selection(&center) & mask == 0 {
            return u32::MAX;
        }
        let mut result = 0u32;
        for (index, (dx, dy)) in DIRECTION_DELTAS.into_iter().enumerate() {
            let (origin_x, origin_y) = center.live_coord();
            let neighbor = resolve_reservation_cell(
                terrain,
                origin_x.wrapping_add(dx),
                origin_y.wrapping_add(dy),
            );
            if self.raw_mask_for_selection(&neighbor) & mask != 0 {
                result |= 1u32 << index;
            }
        }
        result
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (u16, u16, u32)> + '_ {
        self.masks.iter().map(|(&(rx, ry), &mask)| (rx, ry, mask))
    }

    pub fn dummy_mask(&self) -> u32 {
        self.dummy_mask
    }

    /// Reconstruct only the split `+0xDC` state of the fixed fallback
    /// CellClass; real-cell reservation authority is deliberately untouched.
    ///
    /// `MapClass::Resize @ 0x00565C10` reconstructs the fixed dummy through
    /// `CellClass::Constructor @ 0x0047BBF0`, whose write at `0x0047BC76`
    /// clears `CellClass+0xDC`.
    pub(crate) fn reconstruct_dummy_for_map_resize(&mut self) {
        self.dummy_mask = 0;
    }
}

pub struct CellRectPassabilityContext<'a> {
    /// Optional input-owned fallback identity for this whole nested query.
    pub native_cells: Option<&'a crate::map::resolved_terrain::NativeCellQuery<'a>>,
    pub rect: CellRect,
    pub speed_type: SpeedType,
    pub required_zone_id: Option<u32>,
    pub movement_zone: MovementZone,
    pub required_height_or_level: Option<i16>,
    pub bridge_aware_zone: bool,
    pub reject_any_overlay: bool,
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    pub overlay_grid: Option<&'a OverlayGrid>,
    pub occupancy: Option<&'a OccupancyGrid>,
    pub zone_grid: Option<&'a ZoneGrid>,
}

pub struct CellRectOccupancyContext<'a> {
    /// Optional input-owned fallback identity for this whole nested query.
    pub native_cells: Option<&'a crate::map::resolved_terrain::NativeCellQuery<'a>>,
    pub rect: CellRect,
    pub reservation_arg: i32,
    pub reservations: Option<&'a CellReservationGrid>,
    pub occupancy: Option<&'a OccupancyGrid>,
    /// Derived index of live TerrainClass objects in the active ground list.
    pub terrain_object_cells: Option<&'a BTreeMap<(u16, u16), u64>>,
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    pub overlay_grid: Option<&'a OverlayGrid>,
    /// The configured map's final normalized isometric-diamond fields. Absence
    /// rejects the query: active MapClass has no rectangular/unbounded substitute
    /// for `IsRectInPlayfield @ 0x00578390`.
    pub playfield_bounds: Option<PlayfieldBounds>,
}

#[cfg(any(test, feature = "test-support"))]
pub fn check_passability_rect(ctx: CellRectPassabilityContext<'_>) -> bool {
    check_passability_rect_with_raw_occupation(ctx, None)
}

/// Native rectangle query with the caller's live +124/+128 planes. Legacy
/// callers without this authority retain their declared list projection.
pub(crate) fn check_passability_rect_with_raw_occupation(
    ctx: CellRectPassabilityContext<'_>,
    raw: Option<&RawCellOccupationGrid>,
) -> bool {
    scan_cell_rect(ctx.rect, |x, y| {
        check_rect_cell_passability(&ctx, raw, x, y)
    })
}

pub fn check_occupancy_rect(ctx: CellRectOccupancyContext<'_>) -> bool {
    let mask = reservation_mask(ctx.reservation_arg);

    if !scan_cell_rect(ctx.rect, |x, y| {
        occupancy_blocker_at(&ctx, x, y, mask).is_none()
    }) {
        return false;
    }

    rect_playfield_corners(ctx.rect.x, ctx.rect.y, ctx.rect.width, ctx.rect.height)
        .into_iter()
        .all(|cell| {
            cell_is_in_playfield_height_aware_in_query(
                cell,
                ctx.playfield_bounds,
                ctx.resolved_terrain,
                ctx.native_cells,
            )
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OccupancyBlocker {
    TerrainObject,
    Reservation,
    Overlay,
    ZoneType,
    Slope,
    Building,
}

fn occupancy_blocker_at(
    ctx: &CellRectOccupancyContext<'_>,
    x: i32,
    y: i32,
    reservation_mask: u32,
) -> Option<OccupancyBlocker> {
    // Native `CellRect::CheckOccupancy @ 0x00586780` resolves the never-null
    // CellClass first. All following columns therefore observe the lookup's
    // shared-dummy coordinate write before their own first-blocker return.
    let cell = get_cellclass_in_query(ctx.resolved_terrain, x, y, ctx.native_cells);
    let canonical = match &cell {
        CellRef::Real(cell) => Some((cell.rx, cell.ry)),
        CellRef::Dummy { .. } => None,
    };

    if canonical.is_some_and(|cell| {
        ctx.terrain_object_cells
            .is_some_and(|terrain| terrain.contains_key(&cell))
    }) {
        return Some(OccupancyBlocker::TerrainObject);
    }
    let reserved = reservation_mask != 0
        && ctx.reservations.is_some_and(|reservations| {
            let raw_mask = match &cell {
                CellRef::Real(_) => reservations.raw_mask_for_cell_ref(&cell),
                CellRef::Dummy { .. } if ctx.resolved_terrain.is_some() => {
                    reservations.raw_mask_for_cell_ref(&cell)
                }
                // Detached tests have no shared CellClass identity to stamp.
                // Preserve CellReservationGrid's established canonical
                // real-versus-fallback split without inventing dummy state.
                CellRef::Dummy { .. } => reservations.raw_mask(None, x, y),
            };
            raw_mask & reservation_mask != 0
        });
    if reserved {
        return Some(OccupancyBlocker::Reservation);
    }
    if canonical.is_some_and(|(rx, ry)| overlay_present(ctx.overlay_grid, rx, ry)) {
        return Some(OccupancyBlocker::Overlay);
    }

    if matches!(&cell, CellRef::Real(cell) if cell.zone_type != zone_class::GROUND) {
        return Some(OccupancyBlocker::ZoneType);
    }
    if match &cell {
        CellRef::Real(cell) => cell.slope_type != 0,
        CellRef::Dummy { cell } => cell.snapshot().slope_type != 0,
    } {
        return Some(OccupancyBlocker::Slope);
    }
    // Original 586846 asks47C520 after every preceding blocker gate.
    if canonical.is_some_and(|(rx, ry)| {
        ctx.occupancy.is_some_and(|occupancy| {
            occupancy
                .first_building_on_layer(rx, ry, MovementLayer::Ground)
                .is_some()
        })
    }) {
        return Some(OccupancyBlocker::Building);
    }
    None
}

/// One cell of `CellRect::CheckPassability @ 0x0056E7C0`: the packed
/// `MapClass::GetCellClass @ 0x005657A0` lookup (`0x0056E7E8..0x0056E832`),
/// which Winged performs too, then the optional any-overlay refusal
/// (`0x0056E832..0x0056E83E`), then [`check_cell_passability`] with both
/// ignore arguments 0 (`0x0056E840..0x0056E859`). Fixed-stride aliases use
/// the real backing CellClass.
pub(crate) fn check_rect_cell_passability(
    ctx: &CellRectPassabilityContext<'_>,
    raw: Option<&RawCellOccupationGrid>,
    x: i32,
    y: i32,
) -> bool {
    let cell = get_cellclass_in_query(ctx.resolved_terrain, x, y, ctx.native_cells);
    if ctx.reject_any_overlay
        && matches!(&cell, CellRef::Real(cell) if overlay_present(ctx.overlay_grid, cell.rx, cell.ry))
    {
        return false;
    }
    check_cell_passability(
        &cell,
        CellPassabilityArgs {
            speed_type: ctx.speed_type,
            ignore_infantry: false,
            ignore_vehicles: false,
            zone: ctx.required_zone_id,
            movement_zone: ctx.movement_zone,
            level: ctx.required_height_or_level,
            is_bridge: ctx.bridge_aware_zone,
        },
        &CellPassabilityMap {
            terrain: ctx.resolved_terrain,
            overlays: ctx.overlay_grid,
            zones: ctx.zone_grid,
            native_cells: ctx.native_cells,
            raw_occupation: raw,
            listed_occupancy: ctx.occupancy,
        },
    )
    .admits()
}

fn overlay_present(overlay_grid: Option<&OverlayGrid>, rx: u16, ry: u16) -> bool {
    overlay_grid
        .map(|grid| grid.cell(rx, ry).overlay_id.is_some())
        .unwrap_or(false)
}

/// Height-aware `MapClass::IsRectInPlayfield @ 0x00578390`: test NW, NE,
/// SW, then SE with native wrapping/truncation and short-circuit order.
/// Missing configured bounds reject instead of substituting a rectangular or
/// unbounded approximation that active MapClass does not have.
#[cfg(any(test, feature = "test-support"))]
fn rect_is_in_playfield_height_aware(
    rect: CellRect,
    bounds: Option<PlayfieldBounds>,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    let Some(bounds) = bounds else {
        return false;
    };
    rect_playfield_corners(rect.x, rect.y, rect.width, rect.height)
        .into_iter()
        .all(|cell| cell_is_in_playfield_height_aware(cell, Some(bounds), terrain))
}

/// Explicit mode-zero `MapClass::IsCellInPlayfield @ 0x00578460` seam.
/// No CellClass lookup or dummy state is touched.
#[cfg(any(test, feature = "test-support"))]
pub fn cell_is_in_playfield_geometry_only(cell: (i32, i32), bounds: PlayfieldBounds) -> bool {
    bounds.contains_geometry_packed(cell.0, cell.1)
}

/// Explicit mode-one `MapClass::IsCellInPlayfield @ 0x00578460` seam.
/// Missing configured bounds reject; active native callers do not replace the
/// playfield diamond with a terrain rectangle or unconditional success.
pub fn cell_is_in_playfield_height_aware(
    cell: (i32, i32),
    bounds: Option<PlayfieldBounds>,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    cell_is_in_playfield_height_aware_in_query(cell, bounds, terrain, None)
}

/// Map578540 mode1 reads an already-retained Cell pointer. Unlike578460,
/// it does not look up the coordinates or stamp the shared dummy again.
pub(crate) fn retained_cell_is_in_playfield(
    cell: crate::map::cell_index::NativeCellIdentity,
    bounds: Option<PlayfieldBounds>,
    cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
) -> bool {
    let Some(bounds) = bounds else { return false };
    let coord = cells.coord(cell);
    let (level, slope) = cells.ground_fields(cell);
    bounds.contains_height_aware_packed(i32::from(coord.0), i32::from(coord.1), level as i8, slope)
}

pub(crate) fn cell_is_in_playfield_height_aware_in_query(
    cell: (i32, i32),
    bounds: Option<PlayfieldBounds>,
    terrain: Option<&ResolvedTerrainGrid>,
    query: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> bool {
    let Some(bounds) = bounds else {
        return false;
    };
    let (x, y) = packed_cell_coord(cell.0, cell.1);
    let (level, slope) = match get_cellclass_in_query(terrain, x, y, query) {
        CellRef::Real(cell) => (cell.level as i8, cell.slope_type),
        CellRef::Dummy { cell } => {
            let snapshot = cell.snapshot();
            (snapshot.level, snapshot.slope_type)
        }
    };
    bounds.contains_height_aware_packed(x, y, level, slope)
}

/// `MapClass::Clamp_To_Playfield @ 0x00586E50`, used by House501AC0 before
/// Find_Nearby_Cell. Correct the two side edges once, then move both components
/// inward until the original height-aware membership query accepts them.
///
/// Side correction retains full wrapping i32 coordinates; each Cell lookup,
/// membership call and the result use packed signed i16 components. Every miss
/// stamps the same live Dummy. Bounds must describe a nonempty native playfield:
/// the original loop supplies no fallback for an impossible diamond.
/// Native comparisons: `tools/spatial_oracle/house_cell_clamp.{py,json}` (75
/// sparse-map rows, including signed levels/slopes and final Dummy coordinates).
pub(crate) fn clamp_cell_to_playfield<'a>(
    cell: (i32, i32),
    bounds: PlayfieldBounds,
    terrain: Option<&'a ResolvedTerrainGrid>,
    query: Option<&crate::map::resolved_terrain::NativeCellQuery<'a>>,
) -> (i32, i32) {
    let (mut x, mut y) = packed_cell_coord(cell.0, cell.1);
    let right = bounds
        .off_104
        .wrapping_add(bounds.off_fc)
        .wrapping_mul(2)
        .wrapping_sub(bounds.base);
    if x.wrapping_sub(y) >= right {
        let correction = 1_i32
            .wrapping_sub(bounds.off_fc)
            .wrapping_sub(bounds.off_104)
            .wrapping_mul(2)
            .wrapping_sub(y)
            .wrapping_add(bounds.base)
            .wrapping_add(x)
            / 2;
        x = x.wrapping_sub(correction);
        y = y.wrapping_add(correction);
    } else if y.wrapping_sub(x) >= bounds.base.wrapping_sub(bounds.off_fc.wrapping_mul(2)) {
        let correction = bounds
            .off_fc
            .wrapping_mul(2)
            .wrapping_add(2)
            .wrapping_sub(bounds.base)
            .wrapping_sub(x)
            .wrapping_add(y)
            / 2;
        y = y.wrapping_sub(correction);
        x = x.wrapping_add(correction);
    }

    let (level, slope) = match get_cellclass_in_query(terrain, x, y, query) {
        CellRef::Real(cell) => (cell.level as i8, cell.slope_type),
        CellRef::Dummy { cell } => {
            let snapshot = cell.snapshot();
            (snapshot.level, snapshot.slope_type)
        }
    };
    let sum = x.wrapping_add(y);
    let height = bounds.slope_adjusted_height(sum, level, slope);
    let low = bounds
        .base
        .wrapping_add(bounds.off_100.wrapping_mul(2))
        .wrapping_add(height);
    let high = bounds
        .base
        .wrapping_add(2)
        .wrapping_add(bounds.off_108.wrapping_add(bounds.off_100).wrapping_mul(2))
        .wrapping_add(height);
    let step = if sum <= low {
        1
    } else if sum > high {
        -1
    } else {
        return packed_cell_coord(x, y);
    };
    loop {
        x = x.wrapping_add(step);
        y = y.wrapping_add(step);
        if cell_is_in_playfield_height_aware_in_query((x, y), Some(bounds), terrain, query) {
            return packed_cell_coord(x, y);
        }
    }
}

/// Forced-mode-one signed-lepton wrapper from
/// `MapClass::IsCoordInPlayfield @ 0x005785F0`. Division by 256 truncates
/// toward zero, the quotients truncate to signed i16, and z is ignored.
pub fn cell_is_in_playfield_leptons(
    coord: (i32, i32, i32),
    bounds: Option<PlayfieldBounds>,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    let cell = (
        lepton_to_packed_cell_component(coord.0),
        lepton_to_packed_cell_component(coord.1),
    );
    cell_is_in_playfield_height_aware(cell, bounds, terrain)
}

fn reservation_mask(reservation_arg: i32) -> u32 {
    if reservation_arg == -1 {
        0
    } else {
        1u32 << ((reservation_arg as u32) & 0x1F)
    }
}

#[cfg(test)]
#[path = "cell_rect_clamp_tests.rs"]
mod clamp_tests;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    use crate::map::map_file::MapHeader;
    use crate::map::resolved_terrain::TEST_RETAIL_CLEAR_SPEED_COSTS;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainRules};
    use crate::sim::occupancy::CellListInsertion;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::pathfinding::zone_map::ZoneGrid;

    include!("cell_rect_native_tests.rs");

    fn map_header_with_rects(size: (i32, i32), local: [i32; 4]) -> MapHeader {
        MapHeader {
            theater: "TEMPERATE".to_string(),
            fill: "Clear".to_string(),
            level: 0,
            width: size.0 as u32,
            height: size.1 as u32,
            local_left: local[0] as u32,
            local_top: local[1] as u32,
            local_width: local[2] as u32,
            local_height: local[3] as u32,
        }
    }

    fn clear_land() -> CellLand {
        CellLand {
            row: TEST_RETAIL_CLEAR_SPEED_COSTS,
            clear_row: TEST_RETAIL_CLEAR_SPEED_COSTS,
            overlay: None,
        }
    }

    fn clear_to_move_request() -> IsClearToMoveRequest {
        IsClearToMoveRequest {
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
            requested_zone: None,
            actual_zone: 7,
            base_level: 2,
            has_bridge: false,
            requested_level: Some(2),
            is_bridge: false,
            ground_occupation_bits: 0,
            deck_occupation_bits: 0,
            ignore_infantry: false,
            ignore_vehicles: false,
            land: Some(clear_land()),
        }
    }

    /// The land half of a request whose cell row refuses every SpeedType.
    fn zero_row_land(overlay: Option<OverlayPassability>) -> Option<CellLand> {
        Some(CellLand {
            row: SpeedCostProfile {
                foot: Some(0),
                track: Some(0),
                wheel: Some(0),
                float: Some(0),
                amphibious: Some(0),
                float_beach: Some(0),
                hover: Some(0),
            },
            overlay,
            ..clear_land()
        })
    }

    const WALL: OverlayPassability = OverlayPassability {
        wall: true,
        crushable: false,
    };

    #[test]
    fn is_clear_to_move_keeps_winged_and_raw_ignore_masks_separate() {
        let winged = IsClearToMoveRequest {
            speed_type: SpeedType::Winged,
            requested_zone: Some(99),
            ground_occupation_bits: 0xFF,
            deck_occupation_bits: 0xFF,
            land: zero_row_land(Some(WALL)),
            ..clear_to_move_request()
        };
        assert_eq!(
            evaluate_is_clear_to_move(winged),
            IsClearToMoveResult::ClearWinged
        );

        let infantry_ignored = IsClearToMoveRequest {
            ground_occupation_bits: 0x01,
            ignore_infantry: true,
            ..clear_to_move_request()
        };
        assert!(matches!(
            evaluate_is_clear_to_move(infantry_ignored),
            IsClearToMoveResult::Clear { .. }
        ));

        let vehicle_ignored = IsClearToMoveRequest {
            ground_occupation_bits: 0x20,
            ignore_vehicles: true,
            ..clear_to_move_request()
        };
        assert!(matches!(
            evaluate_is_clear_to_move(vehicle_ignored),
            IsClearToMoveResult::Clear { .. }
        ));

        let generic_remains = IsClearToMoveRequest {
            ground_occupation_bits: 0x61,
            ignore_infantry: true,
            ignore_vehicles: true,
            ..clear_to_move_request()
        };
        assert_eq!(
            evaluate_is_clear_to_move(generic_remains),
            IsClearToMoveResult::Occupied {
                remaining_bits: 0x40
            }
        );
    }

    #[test]
    fn is_clear_to_move_preserves_bridge_and_wall_admission_order() {
        let bridge_unspecified = IsClearToMoveRequest {
            has_bridge: true,
            requested_level: None,
            deck_occupation_bits: 0,
            land: zero_row_land(None),
            ..clear_to_move_request()
        };
        assert_eq!(
            evaluate_is_clear_to_move(bridge_unspecified),
            IsClearToMoveResult::Clear {
                selected_layer: MovementLayer::Bridge
            }
        );

        let bridge_uses_deck_occupation = IsClearToMoveRequest {
            has_bridge: true,
            requested_level: None,
            ground_occupation_bits: 0x40,
            deck_occupation_bits: 0x20,
            ..clear_to_move_request()
        };
        assert_eq!(
            evaluate_is_clear_to_move(bridge_uses_deck_occupation),
            IsClearToMoveResult::Occupied {
                remaining_bits: 0x20
            }
        );

        let base_without_bridge_flag = IsClearToMoveRequest {
            has_bridge: true,
            requested_level: Some(2),
            is_bridge: false,
            ..clear_to_move_request()
        };
        assert_eq!(
            evaluate_is_clear_to_move(base_without_bridge_flag),
            IsClearToMoveResult::LevelMismatch
        );

        let crusher_wall = IsClearToMoveRequest {
            movement_zone: MovementZone::Crusher,
            land: zero_row_land(Some(OverlayPassability {
                wall: true,
                crushable: true,
            })),
            ..clear_to_move_request()
        };
        assert!(matches!(
            evaluate_is_clear_to_move(crusher_wall),
            IsClearToMoveResult::Clear {
                selected_layer: MovementLayer::Ground
            }
        ));

        let normal_wall = IsClearToMoveRequest {
            land: zero_row_land(Some(WALL)),
            ..clear_to_move_request()
        };
        assert_eq!(
            evaluate_is_clear_to_move(normal_wall),
            IsClearToMoveResult::WallBlocked
        );

        // `0x00483598` precedes the `0x004835F2` deck bypass.
        let wall_on_deck = IsClearToMoveRequest {
            has_bridge: true,
            requested_level: None,
            land: zero_row_land(Some(WALL)),
            ..clear_to_move_request()
        };
        assert_eq!(
            evaluate_is_clear_to_move(wall_on_deck),
            IsClearToMoveResult::WallBlocked
        );
    }

    /// A 3x3 grid of retail Clear cells whose centre carries `overlay`,
    /// written by the production overlay attribute writer, with the retail
    /// land rows and overlay types bound as a loaded scenario binds them.
    fn retail_overlay_scene(overlay: &str) -> Option<(ResolvedTerrainGrid, OverlayGrid, u8)> {
        let ini = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")?;
        let rules = TerrainRules::from_ini(&ini);
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let clear = rules
            .semantics_for_land_type(LandType::Clear.as_index())
            .unwrap()
            .speed_costs;
        let mut terrain =
            crate::map::resolved_terrain::test_grid(3, 3, |rx, ry| ResolvedTerrainCell {
                speed_costs: clear,
                base_speed_costs: clear,
                ..crate::map::resolved_terrain::test_clear_cell(rx, ry)
            });
        terrain.set_land_speed_rules_for_test(&rules);
        terrain.set_overlay_types_for_test(&registry);
        let id = registry.id_for_name(overlay).unwrap();
        let mut overlays = OverlayGrid::new(3, 3);
        overlays.place_overlay(1, 1, id, 0);
        crate::sim::overlay_grid::recalc_overlay_passability(
            &mut overlays,
            &mut terrain,
            &registry,
            1,
            1,
        );
        Some((terrain, overlays, id))
    }

    fn mover(speed_type: SpeedType, movement_zone: MovementZone) -> CellPassabilityArgs {
        CellPassabilityArgs {
            speed_type,
            ignore_infantry: false,
            ignore_vehicles: false,
            zone: None,
            movement_zone,
            level: None,
            is_bridge: true,
        }
    }

    /// Retail sandbags (`[GASAND]` `Wall=yes`, `Crushable=yes`) reduce to
    /// zone class Crushable, but `0x004834A0` reads the overlay type itself
    /// through `+0x44`: the wall refuses Infantry and Normal (`0x004835CA`),
    /// admits Crusher, AmphibiousCrusher on `+0x22D` (`0x004835BB`),
    /// Destroyer, InfantryDestroyer (`0x004835A2..0x004835AC`) and CrusherAll
    /// (`0x004835C5`). The non-crushable `[GAWALL]` refuses a Crusher.
    #[test]
    fn crushable_wall_admits_only_crushers_and_destroyers() {
        use IsClearToMoveResult::{Clear, WallBlocked};
        use MovementZone::*;
        let ground = Clear {
            selected_layer: MovementLayer::Ground,
        };
        for (overlay, cases) in [
            (
                "GASAND",
                vec![
                    (SpeedType::Foot, Infantry, WallBlocked),
                    (SpeedType::Track, Normal, WallBlocked),
                    (SpeedType::Wheel, Normal, WallBlocked),
                    (SpeedType::Track, Crusher, ground),
                    (SpeedType::Amphibious, AmphibiousCrusher, ground),
                    (SpeedType::Track, Destroyer, ground),
                    (SpeedType::Foot, InfantryDestroyer, ground),
                    (SpeedType::Track, CrusherAll, ground),
                ],
            ),
            (
                "GAWALL",
                vec![
                    (SpeedType::Foot, Infantry, WallBlocked),
                    (SpeedType::Track, Crusher, WallBlocked),
                    (SpeedType::Track, Destroyer, ground),
                ],
            ),
        ] {
            let Some((terrain, overlays, _)) = retail_overlay_scene(overlay) else {
                return;
            };
            let map = CellPassabilityMap {
                terrain: Some(&terrain),
                overlays: Some(&overlays),
                zones: None,
                native_cells: None,
                raw_occupation: None,
                listed_occupancy: None,
            };
            let cell = CellRef::Real(terrain.cell(1, 1).unwrap());
            for (speed_type, movement_zone, expected) in cases {
                assert_eq!(
                    check_cell_passability(&cell, mover(speed_type, movement_zone), &map),
                    expected,
                    "{overlay} {speed_type:?} {movement_zone:?}"
                );
            }
            // The passability rectangle reads the same cell.
            let mut ctx = clear_passability_context(CellRect::single(1, 1), Some(&terrain));
            ctx.overlay_grid = Some(&overlays);
            ctx.speed_type = SpeedType::Foot;
            ctx.movement_zone = Infantry;
            assert!(!check_passability_rect(ctx), "{overlay} rectangle");
        }
    }

    /// A wall the mover passes switches to the Clear row (`0x004835D3`)
    /// before the 0.0 test (`0x004835DE..0x004835F4`). Retail Clear gives
    /// Float 0%, so a Destroyer-zone Float mover stays off a wall while
    /// Track and Hover pass. The Dummy's own Water row (Float 100%, Track
    /// 0%) is not read once its wall passes. A Land the rules lack reads 0.0.
    #[test]
    fn passed_wall_takes_the_clear_row_cost() {
        use IsClearToMoveResult::{Clear, LandBlocked};
        let Some((terrain, overlays, wall)) = retail_overlay_scene("GAWALL") else {
            return;
        };
        let ground = Clear {
            selected_layer: MovementLayer::Ground,
        };
        let map = CellPassabilityMap {
            terrain: Some(&terrain),
            overlays: Some(&overlays),
            zones: None,
            native_cells: None,
            raw_occupation: None,
            listed_occupancy: None,
        };
        let real = CellRef::Real(terrain.cell(1, 1).unwrap());
        let dummy = terrain.shared_cell_dummy();
        dummy.test_set_land_type(i32::from(LandType::Water.as_index()));
        dummy.write_overlay_identity_state(i32::from(wall), 0);
        let dummy = CellRef::Dummy { cell: dummy };
        for (name, cell) in [("real", &real), ("dummy", &dummy)] {
            for (speed_type, expected) in [
                (SpeedType::Float, LandBlocked),
                (SpeedType::Track, ground),
                (SpeedType::Hover, ground),
            ] {
                assert_eq!(
                    check_cell_passability(cell, mover(speed_type, MovementZone::Destroyer), &map),
                    expected,
                    "{name} {speed_type:?}"
                );
            }
        }

        let CellRef::Dummy { cell } = &dummy else {
            unreachable!()
        };
        cell.write_overlay_identity_state(-1, 0);
        for (land, speed_type, expected) in [
            (LandType::Water, SpeedType::Float, ground),
            (LandType::Water, SpeedType::Track, LandBlocked),
        ] {
            cell.test_set_land_type(i32::from(land.as_index()));
            assert_eq!(
                check_cell_passability(&dummy, mover(speed_type, MovementZone::Normal), &map),
                expected,
                "{land:?} {speed_type:?}"
            );
        }
        cell.test_set_land_type(99);
        assert_eq!(
            check_cell_passability(&dummy, mover(SpeedType::Track, MovementZone::Normal), &map),
            LandBlocked,
            "an absent row is 0.0"
        );
    }

    /// Clear cells carrying the retail Clear row, with the grid's rule rows
    /// bound as a loaded scenario binds them.
    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_retail_clear_grid(width, height)
    }

    fn assert_dummy(cell: CellRef<'_>, coord: (i32, i32), level: i8, slope_type: u8) {
        assert_eq!(
            cell.dummy_snapshot(),
            Some(SharedCellDummySnapshot {
                coord,
                level,
                slope_type,
                bridge_flags_0x1180: 0,
            })
        );
    }

    fn wide_test_playfield() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 0,
            off_fc: -1_000,
            off_100: -1_000,
            off_104: 2_000,
            off_108: 2_000,
        }
    }

    fn clear_passability_context<'a>(
        rect: CellRect,
        terrain: Option<&'a ResolvedTerrainGrid>,
    ) -> CellRectPassabilityContext<'a> {
        CellRectPassabilityContext {
            native_cells: None,
            rect,
            speed_type: SpeedType::Track,
            required_zone_id: None,
            movement_zone: MovementZone::Normal,
            required_height_or_level: None,
            bridge_aware_zone: false,
            reject_any_overlay: false,
            resolved_terrain: terrain,
            overlay_grid: None,
            occupancy: None,
            zone_grid: None,
        }
    }

    fn clear_occupancy_context<'a>(
        rect: CellRect,
        terrain: Option<&'a ResolvedTerrainGrid>,
    ) -> CellRectOccupancyContext<'a> {
        CellRectOccupancyContext {
            native_cells: None,
            rect,
            reservation_arg: -1,
            reservations: None,
            occupancy: None,
            terrain_object_cells: None,
            resolved_terrain: terrain,
            overlay_grid: None,
            playfield_bounds: Some(wide_test_playfield()),
        }
    }

    fn square_zone_grid(terrain: &ResolvedTerrainGrid) -> ZoneGrid {
        assert_eq!(terrain.width(), terrain.height());
        ZoneGrid::build_with_terrain(
            &PathGrid::from_resolved_terrain(terrain),
            terrain,
            &[],
            terrain.width(),
            terrain.height(),
        )
    }

    #[test]
    fn gsi_04_01_missing_cell_ignores_real_grid_overlay_and_object_projections() {
        let mut terrain = flat_terrain(1, 1);
        terrain.test_set_native_allocated_cells(&[]);
        let mut overlays = OverlayGrid::new(1, 1);
        overlays.place_overlay(0, 0, 7, 0);

        let mut passability = clear_passability_context(CellRect::single(0, 0), Some(&terrain));
        passability.reject_any_overlay = true;
        passability.overlay_grid = Some(&overlays);
        assert!(
            check_passability_rect(passability),
            "dummy +0x44 has constructor-default no-overlay state"
        );

        let terrain_objects = BTreeMap::from([((0, 0), 88)]);
        let mut occupancy = clear_occupancy_context(CellRect::single(0, 0), Some(&terrain));
        occupancy.terrain_object_cells = Some(&terrain_objects);
        occupancy.overlay_grid = Some(&overlays);
        assert!(
            check_occupancy_rect(occupancy),
            "the dummy owns neither the real slot's ground list nor its +0x44 overlay"
        );
        assert_eq!(terrain.dummy_cell_requested_coord(), (0, 0));
    }

    #[test]
    fn gsi_04_01_passability_scan_is_x_outer_y_inner_and_first_failure_stops() {
        let terrain = flat_terrain(1, 2);
        let mut overlays = OverlayGrid::new(1, 2);
        overlays.place_overlay(0, 1, 7, 0);
        let mut ctx = clear_passability_context(CellRect::new(-1, 0, 2, 3), Some(&terrain));
        ctx.reject_any_overlay = true;
        ctx.overlay_grid = Some(&overlays);

        assert!(!check_passability_rect(ctx));
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (-1, 2),
            "the real (0,1) overlay stops before the later (0,2) miss"
        );

        assert!(check_passability_rect(clear_passability_context(
            CellRect::new(2, 3, 2, 2),
            Some(&terrain),
        )));
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (3, 4),
            "a clear scan visits (2,3),(2,4),(3,3),(3,4)"
        );
    }

    #[test]
    fn gsi_04_01_occupancy_lookup_precedes_terrain_object_blocker() {
        let terrain = flat_terrain(1, 1);
        let terrain_objects = BTreeMap::from([((0, 0), 88)]);
        let mut ctx = clear_occupancy_context(CellRect::new(-1, 0, 3, 1), Some(&terrain));
        ctx.terrain_object_cells = Some(&terrain_objects);

        assert!(!check_occupancy_rect(ctx));
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (-1, 0),
            "lookup of the first miss precedes the real terrain-object blocker, which stops before (1,0)"
        );
    }

    #[test]
    fn gsi_04_01_occupancy_clear_scan_leaves_se_playfield_corner_as_last_writer() {
        let terrain = flat_terrain(1, 1);

        assert!(check_occupancy_rect(clear_occupancy_context(
            CellRect::new(100, 100, 0, 0),
            Some(&terrain),
        )));
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (99, 99),
            "zero-size corners are NW (100,100), NE (99,100), SW (100,99), SE (99,99)"
        );
    }

    #[test]
    fn gsi_04_01_occupancy_failed_nw_playfield_corner_short_circuits() {
        let terrain = flat_terrain(1, 1);
        let mut ctx = clear_occupancy_context(CellRect::new(6, 6, 0, 0), Some(&terrain));
        ctx.playfield_bounds = Some(diamond_bounds());

        assert!(!check_occupancy_rect(ctx));
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (6, 6),
            "failed NW must stop before the distinct decremented corners"
        );
    }

    #[test]
    fn cellrect_dummy_land_speed_matches_original_4834a0() {
        use crate::map::resolved_terrain::NativeCellQuery;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::terrain_rules::TerrainRules;
        use crate::sim::occupancy::RawCellKey;

        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/rally_input.json",
        ))
        .unwrap();
        let rows = native["passability_cases"].as_array().unwrap();
        assert_eq!(rows.len(), 28);
        for row in rows {
            let input = &row["input"];
            let clear = input["table_clear_factor"].as_f64().unwrap();
            let water = input["table_water_factor"].as_f64().unwrap();
            let rules = TerrainRules::from_ini(&IniFile::from_str(&format!(
                "[Clear]\nFoot={clear}\nAmphibious={clear}\n\
                 [Water]\nFoot={water}\nAmphibious={water}\n"
            )));
            let mut terrain = flat_terrain(1, 1);
            terrain.set_land_speed_rules_for_test(&rules);
            let land = input["land_type"].as_i64().unwrap() as i32;
            let flags = input["flags"].as_u64().unwrap() as u32;
            let level = input["level"].as_i64().unwrap() as i8;
            let dummy = input["identity"] == "dummy";
            let coord = if dummy { (1, 0) } else { (0, 0) };
            if dummy {
                let cell = terrain.shared_cell_dummy();
                cell.test_set_land_type(land);
                cell.write_raw_flags(flags);
                terrain.test_set_dummy_cell_level_slope(level, 0);
            } else {
                let costs = terrain.land_speed_costs(land);
                let cell = &mut terrain.cells[0];
                cell.land_type = land as u8;
                cell.speed_costs = costs;
                cell.zone_type = zone_class::GROUND;
                cell.level = level as u8;
                cell.bridge_facts.raw_flags = flags;
            }
            let key = if dummy {
                RawCellKey::Dummy
            } else {
                RawCellKey::Real(0, 0)
            };
            let mut raw = RawCellOccupationGrid::default();
            for (layer, field) in [
                (MovementLayer::Ground, "ground_raw"),
                (MovementLayer::Bridge, "deck_raw"),
            ] {
                raw.write_occupant(key, layer, input[field].as_u64().unwrap() as u8, None, true);
            }
            assert_eq!(input["ignore_infantry"], false);
            assert_eq!(input["ignore_vehicles"], false);
            for isolated in [false, true] {
                terrain.stamp_dummy_cell_requested_coord(99, 98);
                let query = if isolated {
                    NativeCellQuery::isolated(&terrain)
                } else {
                    NativeCellQuery::canonical(&terrain)
                };
                let mut ctx =
                    clear_passability_context(CellRect::single(coord.0, coord.1), Some(&terrain));
                ctx.native_cells = Some(&query);
                ctx.speed_type = match input["speed_type"].as_u64().unwrap() {
                    0 => SpeedType::Foot,
                    4 => SpeedType::Winged,
                    6 => SpeedType::Amphibious,
                    value => panic!("uncovered native speed {value}"),
                };
                ctx.movement_zone = match input["movement_zone"].as_u64().unwrap() {
                    0 => MovementZone::Normal,
                    4 => MovementZone::AmphibiousCrusher,
                    9 => MovementZone::Fly,
                    value => panic!("uncovered native movement zone {value}"),
                };
                let zone = input["required_zone"].as_i64().unwrap();
                ctx.required_zone_id = (zone != -1).then_some(zone as u32);
                let height = input["required_level"].as_i64().unwrap();
                ctx.required_height_or_level = (height != -1).then_some(height as i16);
                ctx.bridge_aware_zone = input["bridge_aware"].as_bool().unwrap();
                assert_eq!(
                    check_rect_cell_passability(
                        &ctx,
                        Some(&raw),
                        i32::from(coord.0),
                        i32::from(coord.1)
                    ),
                    row["accepted"].as_bool().unwrap(),
                    "{input}; isolated={isolated}"
                );
                if isolated {
                    assert_eq!(terrain.dummy_cell_requested_coord(), (99, 98));
                }
            }
        }
    }

    #[test]
    fn gsi_04_01_cellrect_nonbridge_zone_uses_resolved_cell_canonical_coord() {
        let terrain = flat_terrain(2, 2);
        let mut zone_grid = square_zone_grid(&terrain);
        {
            let base = zone_grid.base_topology_mut();
            base.movement_classes = vec![0; 4];
            base.zone_ids = vec![2, 3, 4, 5];
            let row = MovementZone::Normal.matrix_row().unwrap();
            base.raw_zone_ids_by_row[row].resize(6, 0);
            base.raw_zone_ids_by_row[row][0] = 10;
            base.raw_zone_ids_by_row[row][4] = 44;
        }
        let _ = get_cellclass_fallback(Some(&terrain), -1, 0);

        let mut ctx = clear_passability_context(CellRect::single(512, 0), Some(&terrain));
        ctx.zone_grid = Some(&zone_grid);
        ctx.required_zone_id = Some(44);
        assert!(
            check_passability_rect(ctx),
            "packed (512,0) resolves backing slot 512, whose stored coordinate is (0,1)"
        );
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (-1, 0),
            "the real fixed-stride alias and nested non-bridge GetZoneID do not stamp the dummy"
        );
    }

    #[test]
    fn gsi_04_01_cellrect_dummy_routes_raw_nonbridge_zone_without_restamping() {
        let mut terrain = flat_terrain(2, 2);
        let mut zone_grid = square_zone_grid(&terrain);
        {
            let base = zone_grid.base_topology_mut();
            base.movement_classes = vec![0; 4];
            base.zone_ids = vec![2, 3, 4, 5];
            let row = MovementZone::Normal.matrix_row().unwrap();
            base.raw_zone_ids_by_row[row].resize(6, 0);
            base.raw_zone_ids_by_row[row][3] = 77;
        }
        terrain.test_set_native_allocated_cells(&[(0, 0)]);

        let check = |required_zone_id, zone_grid: &ZoneGrid| {
            let mut ctx = clear_passability_context(CellRect::single(1, 0), Some(&terrain));
            ctx.zone_grid = Some(zone_grid);
            ctx.required_zone_id = Some(required_zone_id);
            check_passability_rect(ctx)
        };

        assert!(check(77, &zone_grid));
        assert_eq!(terrain.dummy_cell_requested_coord(), (1, 0));
        assert!(!check(78, &zone_grid));
        assert_eq!(terrain.dummy_cell_requested_coord(), (1, 0));

        let row = MovementZone::Normal.matrix_row().unwrap();
        zone_grid.base_topology_mut().raw_zone_ids_by_row[row][3] = 1;
        assert!(
            !check(77, &zone_grid),
            "raw reserved label 1 stays distinct"
        );
        assert_eq!(terrain.dummy_cell_requested_coord(), (1, 0));
        zone_grid.base_topology_mut().raw_zone_ids_by_row[row][3] = u16::MAX;
        assert!(!check(77, &zone_grid), "raw 0xffff stays distinct");
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (1, 0),
            "non-bridge GetZoneID performs no second CellClass lookup or dummy write"
        );
    }

    #[test]
    fn gsi_04_01_bridge_required_zone_uses_raw_labels_and_missing_record_dword() {
        let mut terrain = flat_terrain(2, 2);
        let mut zone_grid = square_zone_grid(&terrain);
        let cluster = zone_grid.base_topology_mut().zone_ids[0] as usize;
        let row = MovementZone::Normal.matrix_row().unwrap();
        zone_grid.base_topology_mut().raw_zone_ids_by_row[row].resize(cluster + 1, 0);
        zone_grid.base_topology_mut().raw_zone_ids_by_row[row][cluster] = 91;
        let check = |terrain: &ResolvedTerrainGrid, zones: &ZoneGrid, required, bridge| {
            let mut ctx = clear_passability_context(CellRect::single(0, 0), Some(terrain));
            ctx.zone_grid = Some(zones);
            ctx.required_zone_id = Some(required);
            ctx.bridge_aware_zone = bridge;
            check_passability_rect(ctx)
        };
        //4834A0 always forwards the bridge flag to56D230; a nonstructural
        //Cell still returns its raw row, without the flattened-map projection.
        assert!(check(&terrain, &zone_grid, 91, true));
        assert!(!check(&terrain, &zone_grid, 92, true));
        zone_grid.base_topology_mut().raw_zone_ids_by_row[row][cluster] = u16::MAX;
        assert!(check(&terrain, &zone_grid, 0xFFFF, true));
        terrain.cells[0].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        assert_eq!(
            zone_grid.get_zone_id_native(&terrain, (0, 0), MovementZone::Normal, true),
            Some(u32::MAX)
        );
        assert!(
            !check(&terrain, &zone_grid, 0xFFFF, true),
            "missing record DWORDFFFFFFFF is not raw WORDFFFF"
        );
        assert!(
            check(&terrain, &zone_grid, 0xFFFF, false),
            "literal false bypasses the structural record query"
        );
    }

    #[test]
    fn cellrect_passability_bridge_bits_are_not_occupancy_rect_blockers() {
        let mut terrain = flat_terrain(1, 1);
        terrain.cells[0].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        terrain.cells[0].has_bridge_deck = true;
        terrain.cells[0].bridge_walkable = true;
        terrain.cells[0].bridge_deck_level = 4;
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            0,
            0,
            10,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        let passability = CellRectPassabilityContext {
            native_cells: None,
            rect: CellRect::single(0, 0),
            speed_type: SpeedType::Track,
            required_zone_id: None,
            movement_zone: MovementZone::Normal,
            required_height_or_level: None,
            bridge_aware_zone: true,
            reject_any_overlay: false,
            resolved_terrain: Some(&terrain),
            overlay_grid: None,
            occupancy: Some(&occupancy),
            zone_grid: None,
        };
        assert!(!check_passability_rect(passability));

        let occupancy_rect = CellRectOccupancyContext {
            native_cells: None,
            rect: CellRect::single(0, 0),
            reservation_arg: -1,
            reservations: None,
            occupancy: Some(&occupancy),
            terrain_object_cells: None,
            resolved_terrain: Some(&terrain),
            overlay_grid: None,
            playfield_bounds: Some(wide_test_playfield()),
        };
        assert!(check_occupancy_rect(occupancy_rect));
    }

    #[test]
    fn gsi_04_01_lookup_allocation_probe_has_no_dummy_side_effect() {
        let mut terrain = flat_terrain(512, 2);
        terrain.test_set_native_allocated_cells(&[(0, 0), (511, 0)]);
        terrain.test_set_dummy_cell_level_slope(-4, 9);
        let _ = get_cellclass_fallback(Some(&terrain), -3, 0);
        assert_eq!(terrain.dummy_cell_requested_coord(), (-3, 0));

        assert!(terrain.cellclass_allocation_probe(0, 0));
        assert!(terrain.cellclass_allocation_probe(-1, 1));
        assert!(!terrain.cellclass_allocation_probe(1, 0));
        assert!(!terrain.cellclass_allocation_probe(512, 0));
        assert!(!terrain.cellclass_allocation_probe(-1, 0));
        assert_eq!(terrain.dummy_cell_requested_coord(), (-3, 0));
        assert_eq!(terrain.dummy_cell_level_slope(), (-4, 9));
    }

    #[test]
    fn gsi_04_01_shared_dummy_ref_and_grid_clone_retain_live_identity() {
        let terrain = flat_terrain(1, 1);
        terrain.test_set_dummy_cell_level_slope(-6, 11);
        let miss_a = get_cellclass_fallback(Some(&terrain), -1, 0);
        let real = get_cellclass_fallback(Some(&terrain), 0, 0);

        let cloned = terrain.clone();
        assert_eq!(cloned.dummy_cell_requested_coord(), (-1, 0));
        assert_eq!(cloned.dummy_cell_level_slope(), (-6, 11));
        let miss_b = get_cellclass_fallback(Some(&cloned), -2, 0);
        assert_eq!(miss_a, miss_b, "both misses return one CellClass identity");
        assert_dummy(miss_a, (-2, 0), -6, 11);
        assert_eq!(cloned.dummy_cell_requested_coord(), (-2, 0));
        assert_eq!(terrain.dummy_cell_requested_coord(), (-2, 0));
        assert_eq!(cloned.dummy_cell_level_slope(), (-6, 11));
        assert_eq!(terrain.dummy_cell_level_slope(), (-6, 11));
        assert_eq!(
            real,
            CellRef::Real(terrain.cell(0, 0).expect("allocated cell stays stable"))
        );

        let reconstructed = flat_terrain(1, 1);
        assert!(
            !terrain
                .shared_cell_dummy()
                .same_identity(&reconstructed.shared_cell_dummy())
        );
        assert_eq!(reconstructed.dummy_cell_requested_coord(), (0, 0));
        assert_eq!(reconstructed.dummy_cell_level_slope(), (0, 0));
    }

    #[test]
    fn playfield_rect_wrapper_matches_native_corner_contract() {
        let terrain = flat_terrain(512, 2);

        // Only the low word of each requested component reaches the native
        // lookup: 0xFFFF is -1, so (-1,1) aliases canonical (511,0).
        assert_eq!(cell_linear_index(0xFFFF, 1), Some(511));
        assert_eq!(cell_linear_index(0x1_0000, 0), Some(0));
        assert_eq!(
            get_cellclass_fallback(Some(&terrain), 0xFFFF, 1),
            CellRef::Real(terrain.cell(511, 0).expect("canonical index 511"))
        );
        assert_dummy(
            get_cellclass_fallback(Some(&flat_terrain(2, 1)), 0xFFFF, 0),
            (-1, 0),
            0,
            0,
        );

        // Far corners use x+width-1/y+height-1, then truncate each component
        // to its stored word, without saturating at the i32 or i16 boundary.
        assert_eq!(
            rect_playfield_corners(i32::from(i16::MAX), i32::from(i16::MIN), 2, 0),
            [
                (i32::from(i16::MAX), i32::from(i16::MIN)),
                (i32::from(i16::MIN), i32::from(i16::MIN)),
                (i32::from(i16::MAX), i32::from(i16::MAX)),
                (i32::from(i16::MIN), i32::from(i16::MAX)),
            ]
        );
        assert!(rect_is_in_playfield_height_aware(
            CellRect::new(7, 6, 0x1_0001, 1),
            Some(diamond_bounds()),
            None,
        ));
        assert_eq!(
            rect_playfield_corners(7, 6, -1, -2),
            [(7, 6), (5, 6), (7, 3), (5, 3)]
        );
        assert!(!rect_is_in_playfield_height_aware(
            CellRect::new(7, 6, -1, -2),
            Some(diamond_bounds()),
            None,
        ));
    }

    #[test]
    fn playfield_absence_does_not_use_rectangular_or_unbounded_fallback() {
        assert!(!cell_is_in_playfield_height_aware((1, 1), None, None));
        assert!(!rect_is_in_playfield_height_aware(
            CellRect::single(1, 1),
            None,
            None,
        ));
    }

    /// A diamond bounds fixture chosen so the playable region is a clean interior:
    /// pass iff `12 < sx+sy <= 26` AND `sx-sy < 14` AND `sy-sx < 6` (flat terrain,
    /// so the height extension `h = 0`). Derived from the resolved formula in
    /// the canonical MapClass playfield predicate with these five values:
    ///   base=10, off_fc=2, off_100=1, off_104=10, off_108=6
    ///   LOW=off_100*2 = 2; HIGH=2+(off_108+off_100)*2 = 16;
    ///   RIGHT=(off_104+off_fc)*2-base = 14; LEFT=base-off_fc*2 = 6;
    /// so base+LOW=12 (strict low), base+HIGH=26 (inclusive high).
    fn diamond_bounds() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 10,
            off_fc: 2,
            off_100: 1,
            off_104: 10,
            off_108: 6,
        }
    }

    fn occupancy_with_bounds(rect: CellRect) -> CellRectOccupancyContext<'static> {
        CellRectOccupancyContext {
            native_cells: None,
            rect,
            reservation_arg: -1,
            reservations: None,
            occupancy: None,
            terrain_object_cells: None,
            resolved_terrain: None,
            overlay_grid: None,
            playfield_bounds: Some(diamond_bounds()),
        }
    }

    #[test]
    fn gsi_04_05_reservation_masks_aliases_dummy_and_neighbors_are_native_shaped() {
        assert_eq!(reservation_mask(-1), 0);
        assert_eq!(reservation_mask(-2), 1 << 30);
        assert_eq!(reservation_mask(32), 1);
        assert_eq!(reservation_mask(63), 1 << 31);

        let mut grid = CellReservationGrid::new();
        grid.reserve(None, -1, 1, 32);
        assert_eq!(grid.raw_mask(None, 511, 0), 1);
        grid.reserve(None, 512, 0, 1);
        assert_eq!(grid.raw_mask(None, 0, 1), 2);

        grid.reserve(None, -1, 0, 3);
        assert_eq!(grid.raw_mask(None, -512, 0), 1 << 3);
        assert_eq!(grid.dummy_mask(), 1 << 3);
        grid.clear(None, -513, 0, 3);
        assert_eq!(grid.dummy_mask(), 0, "every invalid lookup shares +0xDC");

        let mut neighbors = CellReservationGrid::new();
        neighbors.reserve(None, 10, 10, 4);
        assert_eq!(
            neighbors.house_reservation_neighbor_mask(None, 9, 9, 4),
            u32::MAX
        );
        assert_eq!(
            neighbors.house_reservation_neighbor_mask(None, 10, 10, 4),
            0
        );
        for (dx, dy) in [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ] {
            neighbors.reserve(None, 10 + dx, 10 + dy, 4);
        }
        assert_eq!(
            neighbors.house_reservation_neighbor_mask(None, 10, 10, 4),
            0xff
        );
    }

    #[test]
    fn gsi_04_05_reservation_lookups_stamp_dummy_but_real_alias_does_not() {
        let mut terrain = flat_terrain(512, 2);
        terrain.test_set_native_allocated_cells(&[(0, 0), (511, 0)]);
        let mut grid = CellReservationGrid::new();

        grid.reserve(Some(&terrain), -2, 0, 3);
        assert_eq!(terrain.dummy_cell_requested_coord(), (-2, 0));
        assert_eq!(grid.dummy_mask(), 1 << 3);

        assert_eq!(grid.raw_mask(Some(&terrain), -3, 0), 1 << 3);
        assert_eq!(terrain.dummy_cell_requested_coord(), (-3, 0));
        grid.clear(Some(&terrain), -4, 0, 3);
        assert_eq!(terrain.dummy_cell_requested_coord(), (-4, 0));
        assert_eq!(grid.dummy_mask(), 0);

        grid.reserve(Some(&terrain), -1, 1, 4);
        assert_eq!(grid.raw_mask(Some(&terrain), 511, 0), 1 << 4);
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (-4, 0),
            "the fixed-stride real alias must not stamp the shared dummy"
        );
    }

    #[test]
    fn gsi_04_05_neighbor_mask_dummy_center_rolls_and_real_hits_preserve_origin() {
        let mut all_dummy = flat_terrain(1, 1);
        all_dummy.test_set_native_allocated_cells(&[]);
        let mut grid = CellReservationGrid::new();
        grid.reserve(Some(&all_dummy), -1, 0, 0);
        assert_eq!(
            grid.house_reservation_neighbor_mask(Some(&all_dummy), -1, 0, 0),
            0xff
        );
        assert_eq!(
            all_dummy.dummy_cell_requested_coord(),
            (-1, 0),
            "each miss moves the shared center used by the next direction"
        );

        let mut with_real_hits = flat_terrain(3, 1);
        with_real_hits.test_set_native_allocated_cells(&[(0, 0), (2, 0)]);
        let mut mixed = CellReservationGrid::new();
        mixed.reserve(Some(&with_real_hits), -1, 0, 0);
        assert_eq!(
            mixed.house_reservation_neighbor_mask(Some(&with_real_hits), -1, 0, 0),
            0xaf,
            "the real S and W hits lack the owner bit"
        );
        assert_eq!(
            with_real_hits.dummy_cell_requested_coord(),
            (0, -1),
            "real hits do not stamp and therefore preserve the rolling dummy origin"
        );
    }

    #[test]
    fn gsi_04_01_resize_reconstruction_clears_only_dummy_reservation() {
        let mut grid = CellReservationGrid::new();
        grid.reserve(None, 3, 4, 2);
        grid.reserve(None, -1, 0, 5);

        grid.reconstruct_dummy_for_map_resize();

        assert_eq!(grid.raw_mask(None, 3, 4), 1 << 2);
        assert_eq!(grid.dummy_mask(), 0);
    }

    #[test]
    fn gsi_04_05_reservation_valid_unallocated_slots_share_dummy_but_allocated_alias_is_real() {
        let mut terrain = flat_terrain(3, 1);
        terrain.test_set_native_allocated_cells(&[(0, 0)]);
        assert!(terrain.cell(0, 0).is_some());
        assert!(terrain.cell(1, 0).is_none());
        assert!(terrain.cell(2, 0).is_none());

        let mut grid = CellReservationGrid::new();
        grid.reserve(Some(&terrain), 1, 0, 6);
        assert_eq!(grid.raw_mask(Some(&terrain), 2, 0), 1 << 6);
        assert!(grid.has_reservation(Some(&terrain), 2, 0, 6));
        assert_eq!(
            grid.house_reservation_neighbor_mask(Some(&terrain), 2, 0, 6),
            0xff,
            "the center and all null neighbors dereference the shared dummy"
        );
        assert!(!check_occupancy_rect(CellRectOccupancyContext {
            native_cells: None,
            rect: CellRect::single(2, 0),
            reservation_arg: 6,
            reservations: Some(&grid),
            occupancy: None,
            terrain_object_cells: None,
            resolved_terrain: Some(&terrain),
            overlay_grid: None,
            playfield_bounds: None,
        }));

        grid.reserve(Some(&terrain), 0, 0, 7);
        assert_eq!(grid.raw_mask(Some(&terrain), 0, 0), 1 << 7);
        assert_eq!(grid.raw_mask(Some(&terrain), 1, 0), 1 << 6);
        grid.clear(Some(&terrain), 2, 0, 6);
        assert_eq!(grid.raw_mask(Some(&terrain), 1, 0), 0);
        assert_eq!(grid.raw_mask(Some(&terrain), 0, 0), 1 << 7);

        let alias_terrain = flat_terrain(512, 1);
        let mut aliases = CellReservationGrid::new();
        aliases.reserve(Some(&alias_terrain), -1, 1, 0);
        assert_eq!(aliases.raw_mask(Some(&alias_terrain), 511, 0), 1);
        assert_eq!(aliases.dummy_mask(), 0);
    }

    #[test]
    fn gsi_04_05_reservation_rect_scan_wraps_and_empty_rects_still_test_corners() {
        let mut visited = Vec::new();
        assert!(scan_cell_rect(CellRect::new(i32::MAX, 0, 2, 1), |x, y| {
            visited.push((x, y));
            true
        }));
        assert!(
            visited.is_empty(),
            "wrapped endpoint is below the signed start, so native scan skips"
        );
        assert!(check_occupancy_rect(occupancy_with_bounds(CellRect::new(
            13, 13, 0, 0
        ))));
        assert!(!check_occupancy_rect(occupancy_with_bounds(CellRect::new(
            7, 6, 0, 0
        ))));
        assert!(!check_occupancy_rect(occupancy_with_bounds(CellRect::new(
            7, 6, -1, -1
        ))));
    }

    #[test]
    fn gsi_04_05_reservation_checkoccupancy_first_blocker_order_is_exact() {
        let mut terrain = flat_terrain(1, 1);
        terrain.cells[0].zone_type = zone_class::WATER;
        terrain.cells[0].slope_type = 2;
        let mut reservations = CellReservationGrid::new();
        reservations.reserve(Some(&terrain), 0, 0, 0);
        let mut overlays = OverlayGrid::new(1, 1);
        overlays.place_overlay(0, 0, 7, 0);
        let mut terrain_objects = BTreeMap::new();
        terrain_objects.insert((0, 0), 88);
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            0,
            0,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );

        macro_rules! blocker {
            () => {
                occupancy_blocker_at(
                    &CellRectOccupancyContext {
                        native_cells: None,
                        rect: CellRect::single(0, 0),
                        reservation_arg: 0,
                        reservations: Some(&reservations),
                        occupancy: Some(&occupancy),
                        terrain_object_cells: Some(&terrain_objects),
                        resolved_terrain: Some(&terrain),
                        overlay_grid: Some(&overlays),
                        playfield_bounds: None,
                    },
                    0,
                    0,
                    reservation_mask(0),
                )
            };
        }

        assert_eq!(blocker!(), Some(OccupancyBlocker::TerrainObject));
        terrain_objects.clear();
        assert_eq!(blocker!(), Some(OccupancyBlocker::Reservation));
        reservations.clear(Some(&terrain), 0, 0, 0);
        assert_eq!(blocker!(), Some(OccupancyBlocker::Overlay));
        overlays.clear_overlay(0, 0);
        assert_eq!(blocker!(), Some(OccupancyBlocker::ZoneType));
        terrain.cells[0].zone_type = zone_class::GROUND;
        assert_eq!(blocker!(), Some(OccupancyBlocker::Slope));
        terrain.cells[0].slope_type = 0;
        assert_eq!(blocker!(), Some(OccupancyBlocker::Building));
    }

    #[test]
    fn gsi_04_05_reservation_checkoccupancy_ignores_deck_buildings_and_ground_units() {
        let terrain = flat_terrain(2, 1);
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            0,
            0,
            1,
            MovementLayer::Bridge,
            None,
            CellListInsertion::AppendBuilding,
        );
        occupancy.add(
            1,
            0,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        for x in 0..=1 {
            assert_eq!(
                occupancy_blocker_at(
                    &CellRectOccupancyContext {
                        native_cells: None,
                        rect: CellRect::single(x, 0),
                        reservation_arg: -1,
                        reservations: None,
                        occupancy: Some(&occupancy),
                        terrain_object_cells: None,
                        resolved_terrain: Some(&terrain),
                        overlay_grid: None,
                        playfield_bounds: None,
                    },
                    i32::from(x),
                    0,
                    0,
                ),
                None
            );
        }
    }
}
