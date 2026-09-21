//! Persistent per-cell occupancy grid — tracks which entities occupy each map cell.
//!
//! Replaces the ephemeral `build_occupancy_maps()` approach with an incrementally
//! maintained grid. Entities are added on spawn/move-in, removed on death/move-out.
//! Structures occupy all their foundation cells.
//!
//! Unified single grid with layer-tagged occupants (no separate ground/bridge maps).
//! Equivalent to the original engine's CellClass::FirstObject/AltObject linked lists.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/movement/locomotor (MovementLayer).
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::entities::EntityCategory;
use crate::sim::components::{DriveLocomotionRuntime, DriveOccupationFootprint};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// UnitClass vehicle-occupation bit in both CellClass occupation planes.
pub(crate) const VEHICLE_OCCUPATION_BIT: u8 = 0x20;
/// Generic ObjectClass occupation bit used by landed AircraftClass objects.
pub(crate) const OBJECT_OCCUPATION_BIT: u8 = 0x40;
pub(crate) const BUILDING_OCCUPATION_BIT: u8 = 0x80;

/// Object identities on a CellClass ground/deck list. Terrain remains owned
/// by ProductionState; this view inserts its member between mobile objects
/// and the building tail, as the native AddContent registration paths do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellObjectMember {
    Entity(u64),
    Terrain(u64),
}

/// Side length of gamemd's global airborne-object spatial bucket grid.
/// `FUN_00412870` constructs exactly 400 vectors as a 20 x 20 grid.
pub(crate) const AIR_SPATIAL_BUCKET_SIDE: u16 = 20;

/// Map a cell coordinate into gamemd's clamped 20 x 20 airborne spatial grid.
///
/// The native helpers divide each non-negative cell component by the matching
/// map span divided by 20, then clamp the bucket component to 19. Retail maps
/// are wider than 20 cells; the `max(1)` only keeps small focused fixtures from
/// dividing by zero while preserving the same partition for production maps.
pub(crate) fn air_spatial_bucket_index(rx: u16, ry: u16, map_width: u16, map_height: u16) -> u16 {
    let bucket_width = (map_width / AIR_SPATIAL_BUCKET_SIDE).max(1);
    let bucket_height = (map_height / AIR_SPATIAL_BUCKET_SIDE).max(1);
    let bx = (rx / bucket_width).min(AIR_SPATIAL_BUCKET_SIDE - 1);
    let by = (ry / bucket_height).min(AIR_SPATIAL_BUCKET_SIDE - 1);
    bx + by * AIR_SPATIAL_BUCKET_SIDE
}

/// Whether the object belongs to the independent airborne spatial index.
/// Native producers are the Fly/Jumpjet/rocket air-entry and movement paths;
/// underground objects are deliberately not inferred to be airborne merely
/// because they are absent from both CellClass object lists.
pub(crate) fn air_spatial_tracks_entity(entity: &GameEntity) -> bool {
    use crate::rules::locomotor_type::LocomotorKind;

    let Some(locomotor) = entity.locomotor.as_ref() else {
        return false;
    };
    locomotor.layer == MovementLayer::Air
        || (entity.category == EntityCategory::Aircraft
            && locomotor.kind == LocomotorKind::Fly
            && locomotor.altitude > SIM_ZERO)
}

/// Exact bucket-copy order used by `FUN_00412B40` for the airborne phase of
/// `Apply_area_damage`.
///
/// Bucket vectors themselves retain insertion order. This helper returns only
/// the vector order: center, east/north/south/west cardinal runs, then the
/// northwest/southwest/northeast/southeast corner runs. The native radius is
/// already ftol-truncated by the caller and values below two become one.
pub(crate) fn air_spatial_query_bucket_order(
    center_rx: u16,
    center_ry: u16,
    radius_cells: i32,
    map_width: u16,
    map_height: u16,
) -> Vec<u16> {
    fn step_bucket(bucket: u16, dx: i16, dy: i16) -> u16 {
        let side = i32::from(AIR_SPATIAL_BUCKET_SIDE);
        let bx = (i32::from(bucket % AIR_SPATIAL_BUCKET_SIDE) + i32::from(dx)).clamp(0, side - 1);
        let by = (i32::from(bucket / AIR_SPATIAL_BUCKET_SIDE) + i32::from(dy)).clamp(0, side - 1);
        (bx + by * side) as u16
    }

    fn push_cardinal(
        order: &mut Vec<u16>,
        mut bucket: u16,
        center: u16,
        step_dx: i16,
        step_dy: i16,
    ) -> usize {
        let mut count = 0;
        while bucket != center {
            order.push(bucket);
            count += 1;
            bucket = step_bucket(bucket, step_dx, step_dy);
        }
        count
    }

    fn push_corner(
        order: &mut Vec<u16>,
        center: u16,
        x_count: usize,
        y_count: usize,
        dx: i16,
        dy: i16,
    ) {
        if x_count == 0 || y_count == 0 {
            return;
        }
        let mut bucket = center;
        for _ in 0..x_count.min(y_count) {
            bucket = step_bucket(bucket, dx, dy);
            order.push(bucket);
        }
        if x_count > 1 && y_count > 1 {
            order.push(step_bucket(center, dx * 2, dy));
            order.push(step_bucket(center, dx, dy * 2));
        }
    }

    let radius = if radius_cells < 2 { 1 } else { radius_cells };
    let center = air_spatial_bucket_index(center_rx, center_ry, map_width, map_height);
    let mut order = vec![center];
    let endpoint = |dx: i32, dy: i32| {
        let rx = (i32::from(center_rx) + dx).clamp(0, i32::from(u16::MAX)) as u16;
        let ry = (i32::from(center_ry) + dy).clamp(0, i32::from(u16::MAX)) as u16;
        air_spatial_bucket_index(rx, ry, map_width, map_height)
    };

    let east = push_cardinal(&mut order, endpoint(radius, 0), center, -1, 0);
    let north = push_cardinal(&mut order, endpoint(0, -radius), center, 0, 1);
    let south = push_cardinal(&mut order, endpoint(0, radius), center, 0, -1);
    let west = push_cardinal(&mut order, endpoint(-radius, 0), center, 1, 0);

    push_corner(&mut order, center, west, north, -1, -1);
    push_corner(&mut order, center, west, south, -1, 1);
    push_corner(&mut order, center, east, north, 1, -1);
    push_corner(&mut order, center, east, south, 1, 1);
    order
}

/// Object-list layer after the native display-layer eligibility gate.
/// Aircraft use Fly height rather than their Air path layer; every other
/// category retains the existing locomotor-layer gate and OnBridge selector.
pub(crate) fn cell_list_layer_for_entity(entity: &GameEntity) -> Option<MovementLayer> {
    if entity.category != EntityCategory::Aircraft {
        return entity.occupancy_list_layer();
    }
    let locomotor = entity.locomotor.as_ref()?;
    if locomotor.kind != crate::rules::locomotor_type::LocomotorKind::Fly
        || locomotor.altitude > SIM_ZERO
    {
        return None;
    }
    Some(if entity.on_bridge {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    })
}

/// Convert a coordinate's exact intra-cell position into the raw Infantry
/// occupation mask. The native selector has no sub-cell-1 result: center and
/// the northwest quadrant both select bit 0, while the other quadrants select
/// bits 2, 3, and 4.
pub(crate) fn infantry_raw_occupation_mask(sub_x: SimFixed, sub_y: SimFixed) -> u8 {
    let sub_cell =
        crate::sim::cell_kernel::infantry_preferred_spot(crate::sim::cell_kernel::CellQueryPoint {
            x: sub_x.to_num::<i32>(),
            y: sub_y.to_num::<i32>(),
        });
    1 << sub_cell
}

/// One cell's two independent raw occupation bytes.
///
/// These bytes and infantry owner identities are authoritative state. Bit
/// producers still use native destructive OR/clear semantics; owner identity is
/// the separate `InfantryOwnerIndex`/`AltInfantryOwnerIndex` projection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct RawCellOccupation {
    ground: u8,
    deck: u8,
    #[serde(default)]
    ground_infantry_owner: Option<InternedId>,
    #[serde(default)]
    deck_infantry_owner: Option<InternedId>,
}

/// Identity of the CellClass holding a raw byte. All missing map slots share
/// one receiver; stamping its coordinate never changes this storage key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawCellKey {
    Real(u16, u16),
    Dummy,
}

impl RawCellKey {
    pub(crate) fn from_native(
        terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
        cell: crate::map::cell_index::NativeCellIdentity,
    ) -> Self {
        match cell {
            crate::map::cell_index::NativeCellIdentity::Real(index) => {
                let cell = &terrain.cells()[index];
                Self::Real(cell.rx, cell.ry)
            }
            crate::map::cell_index::NativeCellIdentity::Dummy => Self::Dummy,
        }
    }
}

/// Sparse canonical storage for the raw ground/deck occupation bytes.
///
/// An absent entry is exactly `(ground = 0, deck = 0)`. Zero entries are
/// removed eagerly, so serialization and deterministic hashing have one
/// representation for an unoccupied cell. This state is independent of both
/// the CellClass-style object lists and the owner-aware Drive compatibility
/// cache below.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RawCellOccupationGrid {
    cells: BTreeMap<(u16, u16), RawCellOccupation>,
    // The process-global fallback Cell is not Scenario save payload. Map
    // Resize reconstructs it: Cell47BC2D/30 owners=-1,47BD95/9B raw bytes=0.
    #[serde(skip)]
    dummy: RawCellOccupation,
}

impl RawCellOccupationGrid {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn bits_at(&self, key: RawCellKey, layer: MovementLayer) -> u8 {
        let cell = match key {
            RawCellKey::Real(x, y) => self.cells.get(&(x, y)),
            RawCellKey::Dummy => Some(&self.dummy),
        };
        cell.map_or(0, |cell| match layer {
            MovementLayer::Ground => cell.ground,
            MovementLayer::Bridge => cell.deck,
            _ => 0,
        })
    }

    pub(crate) fn owner_at(&self, key: RawCellKey, layer: MovementLayer) -> Option<InternedId> {
        match key {
            RawCellKey::Real(x, y) => self.infantry_owner(x, y, layer),
            RawCellKey::Dummy => match layer {
                MovementLayer::Ground => self.dummy.ground_infantry_owner,
                MovementLayer::Bridge => self.dummy.deck_infantry_owner,
                _ => None,
            },
        }
    }

    /// Infantry5217C0/521850 mutate the retained receiver after the ground
    /// query. In particular, a later missing lookup does not allocate a cell.
    pub(crate) fn write_infantry(
        &mut self,
        key: RawCellKey,
        layer: MovementLayer,
        mask: u8,
        owner: InternedId,
        put: bool,
    ) {
        if let RawCellKey::Real(x, y) = key {
            match (layer, put) {
                (MovementLayer::Ground, true) => self.mark_ground_infantry(x, y, mask, owner),
                (MovementLayer::Bridge, true) => self.mark_deck_infantry(x, y, mask, owner),
                (MovementLayer::Ground, false) => self.clear_ground_infantry(x, y, mask),
                (MovementLayer::Bridge, false) => self.clear_deck_infantry(x, y, mask),
                _ => {}
            }
            return;
        }
        let (bits, retained_owner) = match layer {
            MovementLayer::Ground => (
                &mut self.dummy.ground,
                &mut self.dummy.ground_infantry_owner,
            ),
            MovementLayer::Bridge => (&mut self.dummy.deck, &mut self.dummy.deck_infantry_owner),
            _ => return,
        };
        if put {
            *bits |= mask;
            *retained_owner = Some(owner);
        } else {
            *bits &= !mask;
            if *bits & 0x1C == 0 {
                *retained_owner = None;
            }
        }
    }

    pub(crate) fn retain_process_dummy_from(&mut self, live: &Self) {
        self.dummy = live.dummy;
    }

    pub(crate) fn reconstruct_dummy_for_map_resize(&mut self) {
        self.dummy = RawCellOccupation::default();
    }

    pub(crate) fn dummy_for_hash(
        &self,
    ) -> Option<(u8, u8, Option<InternedId>, Option<InternedId>)> {
        (self.dummy != RawCellOccupation::default()).then_some((
            self.dummy.ground,
            self.dummy.deck,
            self.dummy.ground_infantry_owner,
            self.dummy.deck_infantry_owner,
        ))
    }

    pub(crate) fn mark_ground(&mut self, rx: u16, ry: u16, mask: u8) {
        if mask != 0 {
            self.cells.entry((rx, ry)).or_default().ground |= mask;
        }
    }

    pub(crate) fn clear_ground(&mut self, rx: u16, ry: u16, mask: u8) {
        self.update_and_prune(rx, ry, |cell| cell.ground &= !mask);
    }

    pub(crate) fn ground_bits(&self, rx: u16, ry: u16) -> u8 {
        self.cells.get(&(rx, ry)).map_or(0, |cell| cell.ground)
    }

    #[cfg(test)]
    pub(crate) fn ground_infantry_owner(&self, rx: u16, ry: u16) -> Option<InternedId> {
        self.cells
            .get(&(rx, ry))
            .and_then(|cell| cell.ground_infantry_owner)
    }

    /// Native: `InfantryClass::MarkCellOccupancy` @ `0x005217C0` (Infantry
    /// vtable `+0xF0`, `0x007EB148`) writes the owner **house index** — from
    /// vtable `+0x38` — after setting the quadrant bit.
    ///
    /// The interned house identity survives entity retirement and ownership
    /// changes, matching native's retained House index. Repair CanEnter uses
    /// this raw slot even when no infantry object remains in the selected list.
    pub(crate) fn mark_ground_infantry(&mut self, rx: u16, ry: u16, mask: u8, owner: InternedId) {
        if mask == 0 {
            return;
        }
        let cell = self.cells.entry((rx, ry)).or_default();
        cell.ground |= mask;
        cell.ground_infantry_owner = Some(owner);
    }

    /// Native: `InfantryClass::UnmarkCellOccupancy` @ `0x00521850` (vtable
    /// `+0xF4`, `0x007EB14C`) resets the selected owner to `0xFFFFFFFF` only
    /// once `byte & 0x1C == 0`, i.e. after functional sub-cells 2..4 are all
    /// clear. Bits 0/1 do not retain it.
    pub(crate) fn clear_ground_infantry(&mut self, rx: u16, ry: u16, mask: u8) {
        self.update_and_prune(rx, ry, |cell| {
            cell.ground &= !mask;
            if cell.ground & 0x1C == 0 {
                cell.ground_infantry_owner = None;
            }
        });
    }

    /// The active bridge-avoidance consumer treats every nonzero ground byte as
    /// occupied, including the consumer-visible bit without an active producer.
    pub(crate) fn ground_is_occupied(&self, rx: u16, ry: u16) -> bool {
        self.ground_bits(rx, ry) != 0
    }

    pub(crate) fn mark_deck(&mut self, rx: u16, ry: u16, mask: u8) {
        if mask != 0 {
            self.cells.entry((rx, ry)).or_default().deck |= mask;
        }
    }

    pub(crate) fn clear_deck(&mut self, rx: u16, ry: u16, mask: u8) {
        self.update_and_prune(rx, ry, |cell| cell.deck &= !mask);
    }

    pub(crate) fn deck_bits(&self, rx: u16, ry: u16) -> u8 {
        self.cells.get(&(rx, ry)).map_or(0, |cell| cell.deck)
    }

    /// Infantry owner identity paired with the selected native occupation byte.
    /// Ground (`+0x124`) and deck (`+0x128`) are independent planes.
    pub(crate) fn infantry_owner(
        &self,
        rx: u16,
        ry: u16,
        layer: MovementLayer,
    ) -> Option<InternedId> {
        self.cells.get(&(rx, ry)).and_then(|cell| match layer {
            MovementLayer::Ground => cell.ground_infantry_owner,
            MovementLayer::Bridge => cell.deck_infantry_owner,
            MovementLayer::Air | MovementLayer::Underground => None,
        })
    }

    #[cfg(test)]
    pub(crate) fn deck_infantry_owner(&self, rx: u16, ry: u16) -> Option<InternedId> {
        self.cells
            .get(&(rx, ry))
            .and_then(|cell| cell.deck_infantry_owner)
    }

    pub(crate) fn mark_deck_infantry(&mut self, rx: u16, ry: u16, mask: u8, owner: InternedId) {
        if mask == 0 {
            return;
        }
        let cell = self.cells.entry((rx, ry)).or_default();
        cell.deck |= mask;
        cell.deck_infantry_owner = Some(owner);
    }

    pub(crate) fn clear_deck_infantry(&mut self, rx: u16, ry: u16, mask: u8) {
        self.update_and_prune(rx, ry, |cell| {
            cell.deck &= !mask;
            if cell.deck & 0x1C == 0 {
                cell.deck_infantry_owner = None;
            }
        });
    }

    fn update_and_prune(&mut self, rx: u16, ry: u16, update: impl FnOnce(&mut RawCellOccupation)) {
        let key = (rx, ry);
        let should_remove = self.cells.get_mut(&key).is_some_and(|cell| {
            update(cell);
            cell.ground == 0
                && cell.deck == 0
                && cell.ground_infantry_owner.is_none()
                && cell.deck_infantry_owner.is_none()
        });
        if should_remove {
            self.cells.remove(&key);
        }
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.cells.len()
    }

    /// Canonical coordinate-key iteration used by the deterministic hash.
    pub(crate) fn entries(
        &self,
    ) -> impl Iterator<Item = (u16, u16, u8, u8, Option<InternedId>, Option<InternedId>)> + '_ {
        self.cells.iter().map(|(&(rx, ry), cell)| {
            (
                rx,
                ry,
                cell.ground,
                cell.deck,
                cell.ground_infantry_owner,
                cell.deck_infantry_owner,
            )
        })
    }
}

/// Sparse authoritative storage for `CellClass`'s building hidden-object count.
///
/// This counter is independent of object lists and both raw occupation planes.
/// An absent entry is exactly zero; wrapping increments that reach zero and
/// guarded decrements that reach zero remove the sparse entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct HiddenOccupationGrid {
    cells: BTreeMap<(u16, u16), u32>,
}

impl HiddenOccupationGrid {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn count(&self, rx: u16, ry: u16) -> u32 {
        self.cells.get(&(rx, ry)).copied().unwrap_or(0)
    }

    /// Apply the post-object-list enter contribution for one building.
    pub(crate) fn enter_building(
        &mut self,
        origin: (u16, u16),
        foundation: &str,
        profile: crate::rules::object_type::BuildingHiddenOccupancyProfile,
        map_size: Option<(u16, u16)>,
    ) -> bool {
        if !profile.can_hide_things {
            return false;
        }

        for cell in hidden_diagonal_cells(origin, foundation, profile.occupy_height, map_size) {
            self.increment(cell);
        }
        for slot in 0..crate::rules::object_type::HIDDEN_OCCUPY_SLOT_COUNT {
            if let Some(offset) = profile.add_occupy[slot]
                && let Some(cell) = hidden_offset_cell(origin, offset, map_size)
            {
                self.increment(cell);
            }
            if let Some(offset) = profile.remove_occupy[slot]
                && let Some(cell) = hidden_offset_cell(origin, offset, map_size)
            {
                self.decrement_guarded(cell);
            }
        }
        true
    }

    /// Reverse the post-object-list contribution for one building. RemoveOccupy
    /// slots deliberately have no exit-side inverse.
    pub(crate) fn exit_building(
        &mut self,
        origin: (u16, u16),
        foundation: &str,
        profile: crate::rules::object_type::BuildingHiddenOccupancyProfile,
        map_size: Option<(u16, u16)>,
    ) -> bool {
        if !profile.can_hide_things {
            return false;
        }

        for cell in hidden_diagonal_cells(origin, foundation, profile.occupy_height, map_size) {
            self.decrement_guarded(cell);
        }
        for offset in profile.add_occupy.into_iter().flatten() {
            if let Some(cell) = hidden_offset_cell(origin, offset, map_size) {
                self.decrement_guarded(cell);
            }
        }
        true
    }

    fn increment(&mut self, cell: (u16, u16)) {
        let next = self.count(cell.0, cell.1).wrapping_add(1);
        if next == 0 {
            self.cells.remove(&cell);
        } else {
            self.cells.insert(cell, next);
        }
    }

    fn decrement_guarded(&mut self, cell: (u16, u16)) {
        let Some(current) = self.cells.get_mut(&cell) else {
            return;
        };
        if *current == 1 {
            self.cells.remove(&cell);
        } else {
            *current -= 1;
        }
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.cells.len()
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (u16, u16, u32)> + '_ {
        self.cells.iter().map(|(&(rx, ry), &count)| (rx, ry, count))
    }
}

/// `CellClass+0xE0`, the single airborne object a cell holds (the AltObject
/// slot).
///
/// gamemd-derived: `0x004135A0` answers whether the slot holds an object other
/// than the caller, and `0x00487D70` writes it — a zero argument clears it, and
/// a claim is refused while a different object holds it. One hovering Jumpjet
/// owns a cell's air slot; the next one to reach cruise height over that cell
/// scatters to a neighbour instead (`JumpjetLocomotionClass` States 1, 3 and 4).
///
/// Serialized verbatim alongside the raw occupation planes: which owner holds a
/// slot, and when it was released, is not reconstructible from entity positions
/// or locomotor phase.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AirSlotGrid {
    cells: BTreeMap<(u16, u16), u64>,
}

impl AirSlotGrid {
    /// The object holding the cell's slot, if any.
    pub(crate) fn holder(&self, rx: u16, ry: u16) -> Option<u64> {
        self.cells.get(&(rx, ry)).copied()
    }

    /// `0x00487D70(cell, owner)`. Refused while another object holds the slot,
    /// which is how the original keeps one hoverer per cell.
    pub(crate) fn claim(&mut self, rx: u16, ry: u16, owner: u64) -> bool {
        match self.cells.get(&(rx, ry)) {
            Some(&held) if held != owner => false,
            _ => {
                self.cells.insert((rx, ry), owner);
                true
            }
        }
    }

    /// `0x00487D70(cell, 0)`.
    pub(crate) fn release(&mut self, rx: u16, ry: u16) {
        self.cells.remove(&(rx, ry));
    }

    /// Drop every slot an object still holds. A Jumpjet removed while hovering
    /// never runs State 4's release, so lifecycle teardown owns this.
    pub(crate) fn release_owner(&mut self, owner: u64) {
        self.cells.retain(|_, held| *held != owner);
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.cells.len()
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (u16, u16, u64)> + '_ {
        self.cells.iter().map(|(&(rx, ry), &owner)| (rx, ry, owner))
    }
}

fn hidden_diagonal_cells(
    origin: (u16, u16),
    foundation: &str,
    occupy_height: i32,
    map_size: Option<(u16, u16)>,
) -> BTreeSet<(u16, u16)> {
    let depth = occupy_height.saturating_sub(1).max(1);
    let mut cells = BTreeSet::new();
    for (dx, dy) in crate::rules::foundation::foundation_cell_offsets(foundation) {
        for k in 0..depth {
            let rx = i32::from(origin.0) + i32::from(dx) - k;
            let ry = i32::from(origin.1) + i32::from(dy) - k;
            if let Some(cell) = hidden_map_cell(rx, ry, map_size) {
                cells.insert(cell);
            }
        }
    }
    cells
}

fn hidden_offset_cell(
    origin: (u16, u16),
    offset: (i16, i16),
    map_size: Option<(u16, u16)>,
) -> Option<(u16, u16)> {
    hidden_map_cell(
        i32::from(origin.0) + i32::from(offset.0),
        i32::from(origin.1) + i32::from(offset.1),
        map_size,
    )
}

fn hidden_map_cell(rx: i32, ry: i32, map_size: Option<(u16, u16)>) -> Option<(u16, u16)> {
    let rx = u16::try_from(rx).ok()?;
    let ry = u16::try_from(ry).ok()?;
    if let Some((width, height)) = map_size.filter(|&(width, height)| width != 0 && height != 0)
        && (rx >= width || ry >= height)
    {
        return None;
    }
    Some((rx, ry))
}

/// Entity-aware ownership behind one native occupation plane.
///
/// The public observation remains the native ORed bit. Keeping the contributing
/// entity IDs lets a Unit ignore its own head-to mark without manufacturing a
/// phantom `CellOccupant` in the destination cell.
#[derive(Debug, Clone, Default)]
struct VehicleOccupationPlane {
    owners: BTreeMap<u64, u8>,
}

impl VehicleOccupationPlane {
    fn mark(&mut self, entity_id: u64) {
        self.owners.insert(entity_id, VEHICLE_OCCUPATION_BIT);
    }

    fn clear(&mut self, entity_id: u64) {
        self.owners.remove(&entity_id);
    }

    #[cfg(test)]
    fn bits(&self) -> u8 {
        self.owners
            .values()
            .copied()
            .fold(0, |bits, mark| bits | mark)
    }

    fn bits_ignoring(&self, entity_id: u64) -> u8 {
        self.owners
            .iter()
            .filter(|(owner, _)| **owner != entity_id)
            .map(|(_, mark)| *mark)
            .fold(0, |bits, mark| bits | mark)
    }

    fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
struct CellVehicleOccupation {
    ground: VehicleOccupationPlane,
    deck: VehicleOccupationPlane,
}

impl CellVehicleOccupation {
    fn plane(&self, layer: MovementLayer) -> Option<&VehicleOccupationPlane> {
        match layer {
            MovementLayer::Ground => Some(&self.ground),
            MovementLayer::Bridge => Some(&self.deck),
            MovementLayer::Air | MovementLayer::Underground => None,
        }
    }

    fn plane_mut(&mut self, layer: MovementLayer) -> Option<&mut VehicleOccupationPlane> {
        match layer {
            MovementLayer::Ground => Some(&mut self.ground),
            MovementLayer::Bridge => Some(&mut self.deck),
            MovementLayer::Air | MovementLayer::Underground => None,
        }
    }

    fn is_empty(&self) -> bool {
        self.ground.is_empty() && self.deck.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellOccupationFootprint {
    rx: u16,
    ry: u16,
    layer: MovementLayer,
}

/// Reverse lookup for one Unit's at-most-current/handoff/head occupation marks.
///
/// Native storage is one ORed bit per owner and plane, not a reference count:
/// coincident roles therefore occupy one slot here as well.
///
/// A turning Drive curve transiently holds three distinct cells: the one its
/// body still stands in, the RawTrack handoff cell it is about to pass through,
/// and the head cell it comes to rest on. `Apply_Track_Occupation_Mode` marks
/// the handoff coordinate and then the head coordinate on modes 1 and 3, and the
/// mover's own cell mark is only dropped once a track point has been paid.
///
/// The store is a plain insertion-ordered list rather than a fixed bound. The
/// original has NO per-owner reverse index at all — its cell mask is one ORed
/// bit per cell, and the owner side is not tracked — so a bound here would be a
/// VERA-internal invariant with nothing behind it, and overrunning it must not
/// take the player's game down. Three is what every current path produces; the
/// list simply records what was marked, so a fourth mark is released correctly
/// instead of being asserted away.
#[derive(Debug, Clone, Default)]
struct OwnerOccupationFootprints {
    marks: Vec<CellOccupationFootprint>,
}

impl OwnerOccupationFootprints {
    fn insert(&mut self, footprint: CellOccupationFootprint) {
        // Native storage is one ORed bit per owner and plane, not a reference
        // count: coincident roles collapse onto one entry here as well.
        if self.marks.contains(&footprint) {
            return;
        }
        self.marks.push(footprint);
    }

    fn remove(&mut self, footprint: CellOccupationFootprint) {
        if let Some(index) = self.marks.iter().position(|mark| *mark == footprint) {
            self.marks.remove(index);
        }
    }

    fn iter(&self) -> impl Iterator<Item = CellOccupationFootprint> + '_ {
        self.marks.iter().copied()
    }

    fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.marks.len()
    }
}

/// Independent CellClass-style vehicle-occupation bit planes.
///
/// This is deliberately separate from [`OccupancyGrid`]: object-list identity
/// and order come only from `CellOccupant`, while an accepted Drive track can
/// mark its head-to cell before the unit is linked there. The index is transient
/// and rebuilt from serialized entity/Drive state.
#[derive(Debug, Clone, Default)]
pub struct CellOccupationGrid {
    cells: BTreeMap<(u16, u16), CellVehicleOccupation>,
    footprints_by_owner: BTreeMap<u64, OwnerOccupationFootprints>,
}

impl CellOccupationGrid {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn rebuild(entities: &crate::sim::entity_store::EntityStore) -> Self {
        let mut grid = Self::new();
        for entity in entities.values() {
            if entity.category != EntityCategory::Unit
                || !entity.lifecycle.cell_marked
                || entity.passenger_role.is_inside_transport()
            {
                continue;
            }
            let current_cleared = !entity.foot_occupation_enabled;
            if !current_cleared && let Some(layer) = entity.occupancy_list_layer() {
                grid.mark_vehicle_on_layer(
                    entity.position.rx,
                    entity.position.ry,
                    entity.stable_id(),
                    layer,
                );
            }
            for mark in entity
                .drive_locomotion
                .as_ref()
                .into_iter()
                .flat_map(|drive| [drive.occupation_handoff, drive.occupation_head_to])
                .chain(
                    entity
                        .ship_locomotion
                        .as_ref()
                        .into_iter()
                        .flat_map(|ship| [ship.occupation_handoff, ship.occupation_head_to]),
                )
                .flatten()
            {
                grid.mark_vehicle_on_layer(mark.rx, mark.ry, entity.stable_id(), mark.layer);
            }
        }
        grid
    }

    /// Reconcile one entity from its serialized current/head-to footprint.
    ///
    /// Most world command paths update this transient index directly. This
    /// narrow reconciliation also covers internal direct/scatter orders whose
    /// command surface owns only `EntityStore`.
    pub(crate) fn reconcile_entity(&mut self, entity: &GameEntity) {
        let old_footprints = self
            .footprints_by_owner
            .get_mut(&entity.stable_id())
            .map(std::mem::take)
            .unwrap_or_default();
        for footprint in old_footprints.iter() {
            self.clear_vehicle_plane(footprint, entity.stable_id());
        }

        if entity.category != EntityCategory::Unit
            || !entity.lifecycle.cell_marked
            || entity.passenger_role.is_inside_transport()
        {
            self.footprints_by_owner.remove(&entity.stable_id());
            return;
        }
        let current_cleared = !entity.foot_occupation_enabled;
        if !current_cleared && let Some(layer) = entity.occupancy_list_layer() {
            self.mark_vehicle_on_layer(
                entity.position.rx,
                entity.position.ry,
                entity.stable_id(),
                layer,
            );
        }
        for mark in entity
            .drive_locomotion
            .as_ref()
            .into_iter()
            .flat_map(|drive| [drive.occupation_handoff, drive.occupation_head_to])
            .chain(
                entity
                    .ship_locomotion
                    .as_ref()
                    .into_iter()
                    .flat_map(|ship| [ship.occupation_handoff, ship.occupation_head_to]),
            )
            .flatten()
        {
            self.mark_vehicle_on_layer(mark.rx, mark.ry, entity.stable_id(), mark.layer);
        }
        if self
            .footprints_by_owner
            .get(&entity.stable_id())
            .is_some_and(OwnerOccupationFootprints::is_empty)
        {
            self.footprints_by_owner.remove(&entity.stable_id());
        }
    }

    /// Exact explicit-plane mark. Ground/deck storage is independent.
    pub(crate) fn mark_vehicle_on_layer(
        &mut self,
        rx: u16,
        ry: u16,
        entity_id: u64,
        layer: MovementLayer,
    ) {
        if !matches!(layer, MovementLayer::Ground | MovementLayer::Bridge) {
            return;
        }
        let cell = self.cells.entry((rx, ry)).or_default();
        cell.plane_mut(layer)
            .expect("ground/deck layer has an occupation plane")
            .mark(entity_id);
        self.footprints_by_owner
            .entry(entity_id)
            .or_default()
            .insert(CellOccupationFootprint { rx, ry, layer });
    }

    /// Exact explicit-plane clear. It intentionally accepts the selected layer
    /// directly so an elevated clear never depends on a cell's current bridge
    /// structural flag.
    pub(crate) fn clear_vehicle_on_layer(
        &mut self,
        rx: u16,
        ry: u16,
        entity_id: u64,
        layer: MovementLayer,
    ) {
        let footprint = CellOccupationFootprint { rx, ry, layer };
        self.clear_vehicle_plane(footprint, entity_id);
        let remove_owner = self
            .footprints_by_owner
            .get_mut(&entity_id)
            .is_some_and(|footprints| {
                footprints.remove(footprint);
                footprints.is_empty()
            });
        if remove_owner {
            self.footprints_by_owner.remove(&entity_id);
        }
    }

    fn clear_vehicle_plane(&mut self, footprint: CellOccupationFootprint, entity_id: u64) {
        let remove_cell = self
            .cells
            .get_mut(&(footprint.rx, footprint.ry))
            .is_some_and(|cell| {
                if let Some(plane) = cell.plane_mut(footprint.layer) {
                    plane.clear(entity_id);
                }
                cell.is_empty()
            });
        if remove_cell {
            self.cells.remove(&(footprint.rx, footprint.ry));
        }
    }

    /// Mark-layer selection: elevated objects use the deck only when the cell
    /// still carries the structural bridge fact.
    #[cfg(test)]
    pub(crate) fn mark_vehicle_by_height(
        &mut self,
        rx: u16,
        ry: u16,
        entity_id: u64,
        at_or_above_bridge_height: bool,
        has_structural_bridge: bool,
    ) -> MovementLayer {
        let layer = if at_or_above_bridge_height && has_structural_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        self.mark_vehicle_on_layer(rx, ry, entity_id, layer);
        layer
    }

    /// Clear-layer selection: the height result alone selects the deck. This
    /// preserves cleanup after the structural bridge flag has disappeared.
    #[cfg(test)]
    pub(crate) fn clear_vehicle_by_height(
        &mut self,
        rx: u16,
        ry: u16,
        entity_id: u64,
        at_or_above_bridge_height: bool,
    ) -> MovementLayer {
        let layer = if at_or_above_bridge_height {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        self.clear_vehicle_on_layer(rx, ry, entity_id, layer);
        layer
    }

    #[cfg(test)]
    pub(crate) fn vehicle_bits(&self, rx: u16, ry: u16, layer: MovementLayer) -> u8 {
        self.cells
            .get(&(rx, ry))
            .and_then(|cell| cell.plane(layer))
            .map_or(0, VehicleOccupationPlane::bits)
    }

    pub(crate) fn vehicle_bits_ignoring(
        &self,
        rx: u16,
        ry: u16,
        layer: MovementLayer,
        entity_id: u64,
    ) -> u8 {
        self.cells
            .get(&(rx, ry))
            .and_then(|cell| cell.plane(layer))
            .map_or(0, |plane| plane.bits_ignoring(entity_id))
    }

    pub(crate) fn occupied_by_other(
        &self,
        rx: u16,
        ry: u16,
        layer: MovementLayer,
        entity_id: u64,
    ) -> bool {
        self.vehicle_bits_ignoring(rx, ry, layer, entity_id) & VEHICLE_OCCUPATION_BIT != 0
    }

    pub(crate) fn occupied_cells_ignoring(
        &self,
        layer: MovementLayer,
        entity_id: u64,
    ) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.cells.iter().filter_map(move |(&(rx, ry), cell)| {
            let occupied = cell
                .plane(layer)
                .is_some_and(|plane| plane.bits_ignoring(entity_id) & VEHICLE_OCCUPATION_BIT != 0);
            occupied.then_some((rx, ry))
        })
    }
}

/// Replace the Drive head-to mark without disturbing a still-valid current-cell
/// mark. The old auxiliary cell is cleared before the new one is installed.
pub(crate) fn replace_drive_head_to_occupation(
    foot_occupation_enabled: &mut bool,
    drive: &mut DriveLocomotionRuntime,
    occupation: &mut CellOccupationGrid,
    entity_id: u64,
    current_cell: (u16, u16),
    current_layer: MovementLayer,
    next: DriveOccupationFootprint,
) {
    if let Some(old) = drive.occupation_head_to.take() {
        let aliases_marked_current = (old.rx, old.ry, old.layer)
            == (current_cell.0, current_cell.1, current_layer)
            && *foot_occupation_enabled;
        if !aliases_marked_current {
            occupation.clear_vehicle_on_layer(old.rx, old.ry, entity_id, old.layer);
        }
    }
    occupation.mark_vehicle_on_layer(next.rx, next.ry, entity_id, next.layer);
    drive.occupation_head_to = Some(next);
}

/// Install (or drop) the forward RawTrack handoff mark that accompanies a Drive
/// curve's head mark.
///
/// `Apply_Track_Occupation_Mode` applies the caller's mode to the handoff
/// coordinate first and to the head coordinate second, so the two marks are
/// installed and released together.
///
/// Two recorded differences from the original, neither of them a "held for the
/// whole curve" guarantee — an earlier revision of this comment claimed one, and
/// the code does not provide it:
///
/// 1. VERA does not release the handoff the moment the point cursor passes the
///    handoff index the way the original's cursor guard does. It releases at
///    curve end (or at replacement).
/// 2. The cell plane stores one entry per owner with no per-role reference
///    count, matching the original's single ORed bit. So once the mover is
///    standing IN its own handoff cell, `clear_current_drive_occupation_for_paid_point`
///    drops that owner's bit for the cell and the handoff role loses its claim
///    with it, until the next tick's `reconcile_entity` re-marks from
///    `drive.occupation_handoff`. The gap is deterministic and inside one tick,
///    so it is not a desync — but the cell is genuinely unclaimed across it.
///
/// Both are UNCHECKED against the original's behaviour for a third mover
/// arriving in that window.
pub(crate) fn replace_drive_handoff_occupation(
    foot_occupation_enabled: &mut bool,
    drive: &mut DriveLocomotionRuntime,
    occupation: &mut CellOccupationGrid,
    entity_id: u64,
    current_cell: (u16, u16),
    current_layer: MovementLayer,
    next: Option<DriveOccupationFootprint>,
) {
    if let Some(old) = drive.occupation_handoff.take() {
        let aliases_marked_current = (old.rx, old.ry, old.layer)
            == (current_cell.0, current_cell.1, current_layer)
            && *foot_occupation_enabled;
        let aliases_head = drive.occupation_head_to == Some(old);
        if !aliases_marked_current && !aliases_head {
            occupation.clear_vehicle_on_layer(old.rx, old.ry, entity_id, old.layer);
        }
    }
    if let Some(next) = next {
        occupation.mark_vehicle_on_layer(next.rx, next.ry, entity_id, next.layer);
        drive.occupation_handoff = Some(next);
    }
}

/// Drop the handoff mark without touching the head mark. Used wherever the head
/// mark's own lifecycle ends — completion, replacement by a new curve, a new
/// order, or world removal. `Apply_Track_Occupation_Mode` mode 0 clears the
/// handoff coordinate and the head coordinate together, so no site may release
/// one and keep the other: a stranded handoff refuses every later mover entry to
/// a cell nothing is in.
pub(crate) fn drop_drive_handoff_occupation(
    foot_occupation_enabled: &mut bool,
    drive: &mut DriveLocomotionRuntime,
    occupation: &mut CellOccupationGrid,
    entity_id: u64,
    current_cell: (u16, u16),
    current_layer: MovementLayer,
) {
    replace_drive_handoff_occupation(
        foot_occupation_enabled,
        drive,
        occupation,
        entity_id,
        current_cell,
        current_layer,
        None,
    );
}

/// Clear an obsolete head-to mark during accepted track replacement while
/// preserving a coincident committed current-cell mark.
pub(crate) fn clear_drive_head_to_occupation_for_replacement(
    foot_occupation_enabled: &mut bool,
    drive: &mut DriveLocomotionRuntime,
    occupation: &mut CellOccupationGrid,
    entity_id: u64,
    current_cell: (u16, u16),
    current_layer: MovementLayer,
) {
    let Some(old) = drive.occupation_head_to.take() else {
        return;
    };
    let aliases_marked_current = (old.rx, old.ry, old.layer)
        == (current_cell.0, current_cell.1, current_layer)
        && *foot_occupation_enabled;
    if !aliases_marked_current {
        occupation.clear_vehicle_on_layer(old.rx, old.ry, entity_id, old.layer);
    }
}

/// A paid Drive point clears the owner's current-coordinate occupation before
/// the coordinate commit. Object-list membership is intentionally untouched.
#[cfg(test)]
pub(crate) fn clear_current_drive_occupation_for_paid_point(
    foot_occupation_enabled: &mut bool,
    _drive: &mut DriveLocomotionRuntime,
    occupation: &mut CellOccupationGrid,
    entity_id: u64,
    current_cell: (u16, u16),
    current_layer: MovementLayer,
) {
    occupation.clear_vehicle_on_layer(current_cell.0, current_cell.1, entity_id, current_layer);
    *foot_occupation_enabled = false;
}

/// A refused selection keeps the standing mover's claim on its own cell.
///
/// **VERA-internal, gamemd equivalent UNCHECKED.** The binary supports the
/// INVARIANT — a refused mover still holds a cell — and not this mechanism. The
/// Drive code-2 arm nulls the head-to coordinate with three direct stores
/// (0x004B3607-0x004B3646) and never calls `Apply_Track_Occupation_Mode`, the
/// only writer of the cell bit, so retail's refusal performs ONE operation:
/// nothing. It never releases, so it never has to re-mark. This function does
/// release-then-re-mark, which is load-bearing only because VERA's paid-point
/// path clears the bit up front (`clear_current_drive_occupation_for_paid_point`
/// above). That early clear IS native — `Process_Drive_Track` calls the
/// owner's `+0xF4` on its current coordinate and zeroes `+0x6B6` at
/// `0x004B1611..161A` when the first point of a transit is paid — so the gap
/// this closes is VERA's own: retail's refusal never reaches a paid point
/// without a curve, whereas VERA's refusal can follow a curve that already
/// paid. Corrected 2026-09-15; an earlier revision said retail never cleared.
///
/// Without this, a mover whose previous curve had already paid a point holds NO
/// bit at all once its head-to mark is dropped: its own cell reads as free to
/// every other mover, and the next follower drives its hull straight into it.
/// Measured on `repro_group_move_of_eight_vehicles_to_one_cell` against an
/// INTERMEDIATE BUILD OF THIS CHANGE — two hulls 29 leptons apart for 28 ticks,
/// on a cell whose occupation byte read `0x00` under a standing tank. That is
/// evidence this change needs the restore for its own construction. It is NOT
/// evidence about the shipped defect the player reported: the pre-change tree
/// has no refusal path at all, so it could not reach this state by this route.
pub(crate) fn restore_current_drive_occupation_after_refusal(
    foot_occupation_enabled: &mut bool,
    _drive: &mut DriveLocomotionRuntime,
    occupation: &mut CellOccupationGrid,
    entity_id: u64,
    current_cell: (u16, u16),
    current_layer: MovementLayer,
) {
    if *foot_occupation_enabled {
        return;
    }
    occupation.mark_vehicle_on_layer(current_cell.0, current_cell.1, entity_id, current_layer);
    *foot_occupation_enabled = true;
}

/// Re-marks the committed cell after a crossing on the legacy Drive step.
///
/// Reachability: Drive and Ship production turns hand their crossings to
/// `track_host.rs`, whose `track_place` gates the raw mark on the Foot
/// occupation enable exactly as `CellClass::AddContent 0x0047E8A0` does
/// (`+0xC0` → `FootClass::IsCellOccupationEnabled 0x0041C070`). This helper
/// runs only from the non-suspending `tick_movement_with_grids` entry, so its
/// unconditional re-enable is a test-path shape, not the native crossing.
pub(crate) fn mark_current_drive_occupation_after_crossing(
    foot_occupation_enabled: &mut bool,
    _drive: &mut DriveLocomotionRuntime,
    occupation: &mut CellOccupationGrid,
    entity_id: u64,
    current_cell: (u16, u16),
    current_layer: MovementLayer,
) {
    occupation.mark_vehicle_on_layer(current_cell.0, current_cell.1, entity_id, current_layer);
    *foot_occupation_enabled = true;
}

/// Normal track completion promotes an aliased head-to mark into the current
/// mark. There is no endpoint clear when the unit has actually relinked there.
pub(crate) fn finish_drive_head_to_occupation(
    foot_occupation_enabled: &mut bool,
    drive: &mut DriveLocomotionRuntime,
    occupation: &mut CellOccupationGrid,
    entity_id: u64,
    current_cell: (u16, u16),
    current_layer: MovementLayer,
) {
    // The curve is over, so its handoff claim goes with it.
    drop_drive_handoff_occupation(
        foot_occupation_enabled,
        drive,
        occupation,
        entity_id,
        current_cell,
        current_layer,
    );
    let Some(head) = drive.occupation_head_to.take() else {
        return;
    };
    if (head.rx, head.ry, head.layer) == (current_cell.0, current_cell.1, current_layer) {
        occupation.mark_vehicle_on_layer(current_cell.0, current_cell.1, entity_id, current_layer);
        *foot_occupation_enabled = true;
    } else {
        occupation.clear_vehicle_on_layer(head.rx, head.ry, entity_id, head.layer);
    }
}

/// Hard limbo/world removal clears the pending head-to cell before the ordinary
/// current-cell RemoveContent clear.
pub(crate) fn clear_drive_head_to_occupation_for_remove(
    drive: &mut DriveLocomotionRuntime,
    occupation: &mut CellOccupationGrid,
    entity_id: u64,
) {
    for mark in [
        drive.occupation_handoff.take(),
        drive.occupation_head_to.take(),
    ]
    .into_iter()
    .flatten()
    {
        occupation.clear_vehicle_on_layer(mark.rx, mark.ry, entity_id, mark.layer);
    }
}

/// Single occupant entry in a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CellOccupant {
    pub entity_id: u64,
    pub layer: MovementLayer,
    /// Infantry sub-cell (2, 3, or 4). None for vehicles/structures.
    /// Lookup cache owned by GameEntity::sub_cell; restored after entity fixups.
    #[serde(skip)]
    pub sub_cell: Option<u8>,
    /// Whether this occupant is a structure, carried over from the insertion
    /// category. gamemd's per-cell building lookup walks the object list and
    /// returns only BuildingClass objects, so gates that ask "is there a
    /// building in this cell" must not be satisfied by a tank parked on it.
    /// Insertion order alone cannot answer that — a lone occupant carries no
    /// ordering information — so the category is recorded per occupant.
    #[serde(skip)]
    pub is_building: bool,
}

/// Requested insertion order for a cell's selected gamemd object list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellListInsertion {
    PrependNonBuilding,
    AppendBuilding,
}

impl CellListInsertion {
    pub fn from_category(category: EntityCategory) -> Self {
        if category == EntityCategory::Structure {
            Self::AppendBuilding
        } else {
            Self::PrependNonBuilding
        }
    }
}

/// All occupants of a single cell.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CellOccupancy {
    /// Occupant list. Common case is 0-3 infantry or 1 vehicle per cell.
    pub occupants: Vec<CellOccupant>,
}

impl CellOccupancy {
    /// All occupants on a selected movement layer in gamemd list order.
    pub fn iter_layer(&self, layer: MovementLayer) -> impl Iterator<Item = &CellOccupant> + '_ {
        self.occupants.iter().filter(move |o| o.layer == layer)
    }

    /// Read the selected CellClass object-list head at the time of the call.
    pub(crate) fn first_on_layer(&self, layer: MovementLayer) -> Option<u64> {
        self.iter_layer(layer).next().map(|object| object.entity_id)
    }

    /// Read the live successor after an object callback. Removal splices the
    /// list and clears the removed object's next pointer (CellClass::RemoveContent
    /// 0x0047EA90, clear at 0x0047EAF0), so an absent current member has no successor.
    /// Callers that require a pre-callback successor must read it before dispatch.
    pub(crate) fn next_on_layer(&self, layer: MovementLayer, current: u64) -> Option<u64> {
        let mut objects = self.iter_layer(layer);
        objects.find(|object| object.entity_id == current)?;
        objects.next().map(|object| object.entity_id)
    }

    /// Non-infantry occupants on a given layer, preserving layer-list order.
    pub fn blockers(&self, layer: MovementLayer) -> impl Iterator<Item = u64> + '_ {
        self.occupants
            .iter()
            .filter(move |o| o.layer == layer && o.sub_cell.is_none())
            .map(|o| o.entity_id)
    }

    /// Infantry occupants on a given layer, preserving layer-list order.
    pub fn infantry(&self, layer: MovementLayer) -> impl Iterator<Item = (u64, u8)> + '_ {
        self.occupants
            .iter()
            .filter(move |o| o.layer == layer && o.sub_cell.is_some())
            .map(|o| (o.entity_id, o.sub_cell.unwrap()))
    }

    /// Whether this cell has any occupants on the given layer.
    pub fn is_empty_on(&self, layer: MovementLayer) -> bool {
        !self.occupants.iter().any(|o| o.layer == layer)
    }

    /// Whether this cell has any non-infantry occupants on the given layer.
    pub fn has_blockers_on(&self, layer: MovementLayer) -> bool {
        self.occupants
            .iter()
            .any(|o| o.layer == layer && o.sub_cell.is_none())
    }

    /// Whether this cell holds a structure on the given layer.
    ///
    /// Strictly narrower than `has_blockers_on`, which also reports vehicles.
    /// This is the predicate for gamemd's per-cell building lookup, which
    /// returns only BuildingClass objects.
    pub fn has_building_on(&self, layer: MovementLayer) -> bool {
        self.occupants
            .iter()
            .any(|o| o.layer == layer && o.is_building)
    }

    /// Count occupants on a given layer.
    pub fn count_on(&self, layer: MovementLayer) -> usize {
        self.occupants.iter().filter(|o| o.layer == layer).count()
    }

    /// Snapshot one selected native Cell object list before callbacks mutate it.
    /// Native: `CellClass::Scatter_Objects` @ `0x00481670` re-reads `+0xE4`/`+0xE8`,
    /// snapshots all objects in an array, then dispatches `+0x174` over that
    /// saved order. Native481727..33 grows capacity by ten via40CE50; the
    /// allocation sets owned+0D at40CED5, permitting subsequent growth.
    /// Ten is not a recipient cap. There is no `CellClass::ScatterContent` in this
    /// program — the name this comment used to carry was invented.
    pub fn snapshot_layer(&self, layer: MovementLayer) -> Vec<u64> {
        self.iter_layer(layer)
            .map(|occupant| occupant.entity_id)
            .collect()
    }
}

/// Persistent per-cell occupancy index, owned by `ObjectSubstrate`.
///
/// These are actual ordered memberships, not a projection of current position or
/// locomotor phase. Cell Save483C10 -> Abstract410320 preserves the native heads;
/// Load4839F0 swizzles +E4/+E8 at483BAB/483BBC without re-running the layer query.
/// A grounded Jumpjet remains on its Cell list after initial Process changes its
/// phase to1. Rebuilding from the flight adapter would silently drop that member.
/// See tools/spatial_oracle/jumpjet_entry_discovery.{py,json,meta.json}.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OccupancyGrid {
    cells: BTreeMap<(u16, u16), CellOccupancy>,
    /// Monotonic counter bumped on every cell-membership mutation. Lets the
    /// movement tick detect when a mover's pathfinding entity-block snapshot is
    /// stale and must be rebuilt before a same-tick repath. Transient scheduling
    /// state only: never serialized, never part of the state hash — it gates
    /// *when* a deterministic rebuild happens, never *what* it produces.
    #[serde(skip)]
    generation: u64,
}

impl Default for OccupancyGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl OccupancyGrid {
    /// Check identity/list invariants without reinterpreting saved membership
    /// through current terrain, alive state, phase, or a new admission query.
    /// Buildings may occur in several cells; neither that nor pending Uninit is
    /// a broken reference. Only the selected list may not repeat a pointer.
    pub(crate) fn validate_memberships(
        &self,
        entities: &crate::sim::entity_store::EntityStore,
    ) -> Result<(), String> {
        for (&cell, occupants) in &self.cells {
            let mut seen = BTreeSet::new();
            for occupant in &occupants.occupants {
                if !matches!(
                    occupant.layer,
                    MovementLayer::Ground | MovementLayer::Bridge
                ) {
                    return Err(format!(
                        "cell {cell:?} has unsupported list {:?}",
                        occupant.layer
                    ));
                }
                if !seen.insert((occupant.layer as u8, occupant.entity_id)) {
                    return Err(format!(
                        "cell {cell:?} repeats object {} on {:?}",
                        occupant.entity_id, occupant.layer
                    ));
                }
                let Some(_entity) = entities.get(occupant.entity_id) else {
                    return Err(format!(
                        "cell {cell:?} references missing object {}",
                        occupant.entity_id
                    ));
                };
            }
        }
        Ok(())
    }

    /// Fix up lookup metadata after validating all saved object references.
    /// This changes neither selected-list membership nor list order. Native
    /// Cell lists store pointers: category and sub-cell information belong to
    /// the pointed-to objects, not to a second serialized Cell record.
    pub(crate) fn restore_memberships(
        &mut self,
        entities: &crate::sim::entity_store::EntityStore,
    ) -> Result<(), String> {
        self.validate_memberships(entities)?;
        for occupants in self.cells.values_mut() {
            for occupant in &mut occupants.occupants {
                let entity = entities
                    .get(occupant.entity_id)
                    .expect("validated reference");
                occupant.is_building = entity.category == EntityCategory::Structure;
                occupant.sub_cell = if entity.category == EntityCategory::Infantry {
                    entity.sub_cell
                } else {
                    None
                };
            }
        }
        Ok(())
    }

    /// Hash each native ground/deck list independently: the Vec's interleaving
    /// of the two lists is only Rust storage. Empty retained map buckets and the
    /// transient generation do not change membership. This stronger Rust peer
    /// hash is not a claim that native64DAB0 directly folds Cell pointers.
    pub(crate) fn hash_memberships(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        b"cell-membership-v1".hash(hasher);
        let list_count: usize = self
            .cells
            .values()
            .map(|occupants| {
                [MovementLayer::Ground, MovementLayer::Bridge]
                    .into_iter()
                    .filter(|&layer| !occupants.is_empty_on(layer))
                    .count()
            })
            .sum();
        list_count.hash(hasher);
        for (&cell, occupants) in &self.cells {
            for layer in [MovementLayer::Ground, MovementLayer::Bridge] {
                let count = occupants.iter_layer(layer).count();
                if count == 0 {
                    continue;
                }
                cell.hash(hasher);
                layer.hash(hasher);
                count.hash(hasher);
                for occupant in occupants.iter_layer(layer) {
                    occupant.entity_id.hash(hasher);
                }
            }
        }
    }

    /// Rebuild occupancy from scratch by scanning all entities.
    /// Legacy construction/fixture adapter only. Snapshot restoration preserves
    /// the serialized lists and never calls this lossy phase-based projection.
    pub fn rebuild(entities: &crate::sim::entity_store::EntityStore) -> Self {
        let mut grid = Self::new();
        let mut ordered: Vec<&GameEntity> = entities.values().collect();
        ordered.sort_by_key(|entity| (entity.occupancy_enter_order, entity.stable_id()));
        for entity in ordered {
            // Global storage, native-alive, limbo, and cell-list membership are
            // independent facts. Only an object whose Mark transaction succeeded
            // participates in this rebuilt cache.
            if !entity.lifecycle.cell_marked {
                continue;
            }
            // Entities inside transports don't occupy cells.
            if entity.passenger_role.is_inside_transport() {
                continue;
            }
            let Some(layer) = cell_list_layer_for_entity(entity) else {
                continue;
            };
            let sid = entity.stable_id();
            let sub = if entity.category == EntityCategory::Infantry {
                entity.sub_cell
            } else {
                None
            };
            let insertion = CellListInsertion::from_category(entity.category);
            for (rx, ry) in entity_occupancy_cells(entity) {
                grid.add(rx, ry, sid, layer, sub, insertion);
            }
        }
        grid
    }
}

pub(crate) fn entity_occupancy_cells(entity: &GameEntity) -> Vec<(u16, u16)> {
    if entity.category != EntityCategory::Structure {
        return vec![(entity.position.rx, entity.position.ry)];
    }

    let (w, h) = crate::rules::foundation::foundation_dimensions(&entity.foundation);
    let mut cells = Vec::with_capacity(w as usize * h as usize);
    for dx in 0..w {
        for dy in 0..h {
            let Some(rx) = entity.position.rx.checked_add(dx) else {
                continue;
            };
            let Some(ry) = entity.position.ry.checked_add(dy) else {
                continue;
            };
            cells.push((rx, ry));
        }
    }
    cells
}

impl OccupancyGrid {
    /// Create an empty occupancy grid.
    pub fn new() -> Self {
        Self {
            cells: BTreeMap::new(),
            generation: 0,
        }
    }

    /// Current mutation generation. Bumped on every `add`/`remove`/`update_sub_cell`
    /// (and thus `move_entity`). Compare across two points to detect whether cell
    /// membership changed in between. Transient — not hashed, resets to 0 on rebuild.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Add an entity to a cell. For structures, caller must invoke once per
    /// foundation cell.
    pub fn add(
        &mut self,
        rx: u16,
        ry: u16,
        entity_id: u64,
        layer: MovementLayer,
        sub_cell: Option<u8>,
        insertion: CellListInsertion,
    ) {
        self.generation = self.generation.wrapping_add(1);
        let new_occupant = CellOccupant {
            entity_id,
            layer,
            sub_cell,
            is_building: insertion == CellListInsertion::AppendBuilding,
        };
        let occ = self.cells.entry((rx, ry)).or_default();
        match insertion {
            CellListInsertion::PrependNonBuilding => {
                let index = occ
                    .occupants
                    .iter()
                    .position(|o| o.layer == layer)
                    .unwrap_or(0);
                occ.occupants.insert(index, new_occupant);
            }
            CellListInsertion::AppendBuilding => {
                let index = occ
                    .occupants
                    .iter()
                    .rposition(|o| o.layer == layer)
                    .map_or(occ.occupants.len(), |i| i + 1);
                occ.occupants.insert(index, new_occupant);
            }
        }
    }

    /// Remove an entity from a cell, walking ALL layers. No-op if entity not found.
    /// For structures, caller must invoke once per foundation cell.
    pub fn remove(&mut self, rx: u16, ry: u16, entity_id: u64) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(occ) = self.cells.get_mut(&(rx, ry)) {
            occ.occupants.retain(|o| o.entity_id != entity_id);
            if occ.occupants.is_empty() {
                self.cells.remove(&(rx, ry));
            }
        }
    }

    /// Remove an entity from ONLY the given layer's object list in a cell.
    ///
    /// This is the gamemd-native `RemoveContent` behavior: it walks only the
    /// selected per-cell list (ground vs bridge/deck) chosen by the occupant's
    /// `OnBridge` byte at the call site, never the other layer. On a bridge cell
    /// crossing the removal must observe the OLD (pre-transition) layer — see
    /// `move_entity_layered`. No-op if no matching-layer entry exists.
    ///
    /// For the single-entry-per-cell invariant this grid maintains (each entity is
    /// `add`ed exactly once per cell), the per-layer result equals the layer-agnostic
    /// `remove`; the per-layer form is the authoritative two-layer selector and keeps
    /// the remove/add halves on the verified independent layers.
    pub fn remove_on_layer(&mut self, rx: u16, ry: u16, entity_id: u64, layer: MovementLayer) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(occ) = self.cells.get_mut(&(rx, ry)) {
            occ.occupants
                .retain(|o| !(o.entity_id == entity_id && o.layer == layer));
            if occ.occupants.is_empty() {
                self.cells.remove(&(rx, ry));
            }
        }
    }

    /// Move an entity from one cell to another (layer-agnostic remove + add).
    ///
    /// Convenience for callers that do NOT change the occupant's object-list layer
    /// across the move (teleport, same-layer steps). For a bridge cell crossing that
    /// may flip `on_bridge`, use `move_entity_layered` so the old-cell removal
    /// observes the OLD layer and the new-cell insertion the NEW layer.
    pub fn move_entity(
        &mut self,
        old_rx: u16,
        old_ry: u16,
        new_rx: u16,
        new_ry: u16,
        entity_id: u64,
        layer: MovementLayer,
        sub_cell: Option<u8>,
        insertion: CellListInsertion,
    ) {
        self.remove(old_rx, old_ry, entity_id);
        self.add(new_rx, new_ry, entity_id, layer, sub_cell, insertion);
    }

    /// Authoritative two-layer cell crossing — the verified gamemd order:
    /// 1. remove from the OLD cell on the **OLD** object-list layer (the
    ///    pre-transition `on_bridge` layer; `RemoveContent` walks only that list),
    /// 2. add to the NEW cell on the **NEW** object-list layer (the post-transition
    ///    `on_bridge` layer; `AddContent` selects the list by the new byte).
    ///
    /// Old-cell removal observes the pre-transition layer; new-cell insertion the
    /// post-transition layer. The two halves may target different layers when the
    /// occupant stepped on/off the deck during the crossing — this asymmetry is the
    /// load-bearing part of the contract (the list layer is selected by the
    /// occupant's `OnBridge` byte sampled at each call site, not a single layer
    /// reused for both halves).
    #[allow(clippy::too_many_arguments)]
    pub fn move_entity_layered(
        &mut self,
        old_rx: u16,
        old_ry: u16,
        new_rx: u16,
        new_ry: u16,
        entity_id: u64,
        old_layer: MovementLayer,
        new_layer: MovementLayer,
        sub_cell: Option<u8>,
        insertion: CellListInsertion,
    ) {
        self.remove_on_layer(old_rx, old_ry, entity_id, old_layer);
        self.add(new_rx, new_ry, entity_id, new_layer, sub_cell, insertion);
    }

    /// Update an entity's sub-cell within the same cell.
    pub fn update_sub_cell(&mut self, rx: u16, ry: u16, entity_id: u64, new_sub_cell: Option<u8>) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(occ) = self.cells.get_mut(&(rx, ry)) {
            if let Some(o) = occ.occupants.iter_mut().find(|o| o.entity_id == entity_id) {
                o.sub_cell = new_sub_cell;
            }
        }
    }

    /// Get occupancy for a cell (all layers).
    pub fn get(&self, rx: u16, ry: u16) -> Option<&CellOccupancy> {
        self.cells.get(&(rx, ry))
    }

    /// Borrow the live mixed list without allocating a replacement member
    /// vector. Callers decide whether to capture the next member before or
    /// after a receiver callback; those two native traversals are different.
    /// Terrain precedes buildings and follows mobile objects for the current
    /// terrain-first scenario registration. Generic native AddContent prepends
    /// every nonbuilding; this projection does not model later Terrain insertion.
    pub(crate) fn cell_objects(
        &self,
        rx: u16,
        ry: u16,
        layer: MovementLayer,
        terrain_id: Option<u64>,
    ) -> impl Iterator<Item = CellObjectMember> + '_ {
        let mut terrain = terrain_id.filter(|_| layer == MovementLayer::Ground);
        let mut entities = self
            .get(rx, ry)
            .into_iter()
            .flat_map(move |cell| cell.iter_layer(layer))
            .peekable();
        std::iter::from_fn(move || {
            if terrain.is_some() && entities.peek().is_none_or(|member| member.is_building) {
                return terrain.take().map(CellObjectMember::Terrain);
            }
            entities
                .next()
                .map(|member| CellObjectMember::Entity(member.entity_id))
        })
    }

    /// First BuildingClass identity on a selected native list, preserving
    /// literal list order. Projectile collision supplies `Ground` because
    /// `Look_up_building_in_cell` @ `0x0047C520` walks `+0xE4` only and returns
    /// the first `What_Am_I() == 6`, so it never selects the deck list. (There
    /// is no `CellClass::GetBuilding` in this program.)
    pub fn first_building_on_layer(&self, rx: u16, ry: u16, layer: MovementLayer) -> Option<u64> {
        self.get(rx, ry)?
            .iter_layer(layer)
            .find(|occupant| occupant.is_building)
            .map(|occupant| occupant.entity_id)
    }

    /// Typed Cell query over a selected native list. The caller owns the native
    /// scenario-ready gate; this method owns only literal head-to-tail traversal.
    pub fn first_category_on_layer(
        &self,
        rx: u16,
        ry: u16,
        layer: MovementLayer,
        category: EntityCategory,
        entities: &crate::sim::entity_store::EntityStore,
    ) -> Option<u64> {
        self.get(rx, ry)?.iter_layer(layer).find_map(|occupant| {
            entities
                .get(occupant.entity_id)
                .is_some_and(|entity| entity.category == category)
                .then_some(occupant.entity_id)
        })
    }

    /// Check if a cell has no occupants on a given layer.
    pub fn is_empty_on_layer(&self, rx: u16, ry: u16, layer: MovementLayer) -> bool {
        self.cells
            .get(&(rx, ry))
            .map_or(true, |occ| occ.is_empty_on(layer))
    }

    /// Count total occupants on a layer in a cell.
    pub fn count_on_layer(&self, rx: u16, ry: u16, layer: MovementLayer) -> usize {
        self.cells
            .get(&(rx, ry))
            .map_or(0, |occ| occ.count_on(layer))
    }

    /// Cells whose selected native object list has at least one member.
    ///
    /// This exposes cell identity, not occupant order; callers that need the
    /// literal list must continue through [`Self::get`] and `iter_layer`.
    pub(crate) fn occupied_cells_on_layer(
        &self,
        layer: MovementLayer,
    ) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.cells
            .iter()
            .filter_map(move |(&cell, occupants)| (!occupants.is_empty_on(layer)).then_some(cell))
    }

    /// Check if a specific entity is in a specific cell.
    pub fn contains_entity(&self, rx: u16, ry: u16, entity_id: u64) -> bool {
        self.cells
            .get(&(rx, ry))
            .is_some_and(|occ| occ.occupants.iter().any(|o| o.entity_id == entity_id))
    }

    /// Total number of occupied cells (for diagnostics).
    #[cfg(test)]
    pub fn occupied_cell_count(&self) -> usize {
        self.cells.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsi_04_12_raw_occupation_preserves_every_raw_bit() {
        let mut grid = RawCellOccupationGrid::new();
        for mask in [
            0x01,
            0x02,
            0x04,
            0x08,
            0x10,
            VEHICLE_OCCUPATION_BIT,
            0x40,
            0x80,
        ] {
            grid.mark_ground(7, 9, mask);
        }

        assert_eq!(grid.ground_bits(7, 9), u8::MAX);
        assert!(grid.ground_is_occupied(7, 9));
        assert_eq!(grid.ground_bits(9, 7), 0);
        assert!(!grid.ground_is_occupied(9, 7));
    }

    #[test]
    fn infantry_owner_indices_follow_selected_plane_and_functional_bits() {
        let mut grid = RawCellOccupationGrid::new();
        grid.mark_ground_infantry(7, 9, 1 << 2, InternedId::from_index(41));
        grid.mark_ground_infantry(7, 9, 1 << 3, InternedId::from_index(42));
        grid.mark_deck_infantry(7, 9, 1 << 4, InternedId::from_index(99));

        assert_eq!(grid.ground_bits(7, 9) & 0x1C, 0x0C);
        assert_eq!(
            grid.ground_infantry_owner(7, 9),
            Some(InternedId::from_index(42))
        );
        assert_eq!(
            grid.deck_infantry_owner(7, 9),
            Some(InternedId::from_index(99))
        );

        grid.clear_ground_infantry(7, 9, 1 << 3);
        assert_eq!(
            grid.ground_infantry_owner(7, 9),
            Some(InternedId::from_index(42))
        );
        grid.clear_ground_infantry(7, 9, 1 << 2);
        assert_eq!(grid.ground_infantry_owner(7, 9), None);
        assert_eq!(
            grid.deck_infantry_owner(7, 9),
            Some(InternedId::from_index(99))
        );
    }

    #[test]
    fn gsi_04_12_raw_occupation_clear_is_destructive_not_reference_counted() {
        let mut grid = RawCellOccupationGrid::new();
        grid.mark_ground(3, 4, VEHICLE_OCCUPATION_BIT);
        grid.mark_ground(3, 4, VEHICLE_OCCUPATION_BIT);
        assert_eq!(grid.ground_bits(3, 4), VEHICLE_OCCUPATION_BIT);

        grid.clear_ground(3, 4, VEHICLE_OCCUPATION_BIT);

        assert_eq!(grid.ground_bits(3, 4), 0);
        assert_eq!(grid.entry_count(), 0);
    }

    #[test]
    fn gsi_04_12_raw_occupation_ground_and_deck_planes_are_independent() {
        let mut grid = RawCellOccupationGrid::new();
        grid.mark_ground(11, 12, 0x21);
        grid.mark_deck(11, 12, 0xC0);
        assert_eq!(grid.ground_bits(11, 12), 0x21);
        assert_eq!(grid.deck_bits(11, 12), 0xC0);

        grid.clear_ground(11, 12, VEHICLE_OCCUPATION_BIT);
        assert_eq!(grid.ground_bits(11, 12), 0x01);
        assert_eq!(grid.deck_bits(11, 12), 0xC0);

        grid.clear_deck(11, 12, 0x40);
        assert_eq!(grid.ground_bits(11, 12), 0x01);
        assert_eq!(grid.deck_bits(11, 12), 0x80);
    }

    #[test]
    fn generation_bumps_on_every_mutation() {
        let mut grid = OccupancyGrid::new();
        let g0 = grid.generation();
        grid.add(
            1,
            1,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let g1 = grid.generation();
        assert!(g1 > g0, "add must bump generation");
        grid.move_entity(
            1,
            1,
            2,
            2,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let g2 = grid.generation();
        assert!(g2 > g1, "move_entity (remove+add) must bump generation");
        grid.update_sub_cell(2, 2, 10, Some(3));
        let g3 = grid.generation();
        assert!(g3 > g2, "update_sub_cell must bump generation");
        grid.remove(2, 2, 10);
        assert!(grid.generation() > g3, "remove must bump generation");
    }

    #[test]
    fn add_and_get() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let occ = grid.get(5, 5).unwrap();
        assert_eq!(occ.occupants.len(), 1);
        assert_eq!(occ.occupants[0].entity_id, 1);
        assert_eq!(occ.occupants[0].layer, MovementLayer::Ground);
        assert!(occ.occupants[0].sub_cell.is_none());
    }

    #[test]
    fn remove_cleans_up_empty_cell() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.remove(5, 5, 1);
        assert!(grid.get(5, 5).is_none());
        assert_eq!(grid.occupied_cell_count(), 0);
    }

    #[test]
    fn remove_nonexistent_is_noop() {
        let mut grid = OccupancyGrid::new();
        grid.remove(5, 5, 99);
        assert!(grid.get(5, 5).is_none());
    }

    #[test]
    fn move_entity_transfers_between_cells() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.move_entity(
            5,
            5,
            6,
            6,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        assert!(grid.get(5, 5).is_none());
        let occ = grid.get(6, 6).unwrap();
        assert_eq!(occ.occupants.len(), 1);
        assert_eq!(occ.occupants[0].entity_id, 1);
    }

    #[test]
    fn layer_filtering() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            2,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            3,
            MovementLayer::Ground,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );

        let occ = grid.get(5, 5).unwrap();
        let ground_ids: Vec<u64> = occ
            .iter_layer(MovementLayer::Ground)
            .map(|o| o.entity_id)
            .collect();
        assert_eq!(ground_ids, vec![3, 1]);
        let ground_blockers: Vec<u64> = occ.blockers(MovementLayer::Ground).collect();
        assert_eq!(ground_blockers, vec![1]);
        let bridge_blockers: Vec<u64> = occ.blockers(MovementLayer::Bridge).collect();
        assert_eq!(bridge_blockers, vec![2]);
        let ground_inf: Vec<(u64, u8)> = occ.infantry(MovementLayer::Ground).collect();
        assert_eq!(ground_inf, vec![(3, 2)]);
        assert_eq!(occ.infantry(MovementLayer::Bridge).count(), 0);
    }

    #[test]
    fn is_empty_on_layer() {
        let mut grid = OccupancyGrid::new();
        assert!(grid.is_empty_on_layer(5, 5, MovementLayer::Ground));
        grid.add(
            5,
            5,
            1,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        assert!(grid.is_empty_on_layer(5, 5, MovementLayer::Ground));
        assert!(!grid.is_empty_on_layer(5, 5, MovementLayer::Bridge));
    }

    #[test]
    fn infantry_subcells() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            10,
            MovementLayer::Ground,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            11,
            MovementLayer::Ground,
            Some(3),
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            12,
            MovementLayer::Ground,
            Some(4),
            CellListInsertion::PrependNonBuilding,
        );

        let occ = grid.get(5, 5).unwrap();
        let inf: Vec<(u64, u8)> = occ.infantry(MovementLayer::Ground).collect();
        assert_eq!(inf.len(), 3);
        assert!(!occ.has_blockers_on(MovementLayer::Ground));
    }

    #[test]
    fn multi_cell_building() {
        let mut grid = OccupancyGrid::new();
        for dy in 0..2u16 {
            for dx in 0..2u16 {
                grid.add(
                    10 + dx,
                    10 + dy,
                    100,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            }
        }
        assert!(grid.contains_entity(10, 10, 100));
        assert!(grid.contains_entity(11, 10, 100));
        assert!(grid.contains_entity(10, 11, 100));
        assert!(grid.contains_entity(11, 11, 100));
        assert!(!grid.contains_entity(12, 12, 100));

        for dy in 0..2u16 {
            for dx in 0..2u16 {
                grid.remove(10 + dx, 10 + dy, 100);
            }
        }
        assert_eq!(grid.occupied_cell_count(), 0);
    }

    #[test]
    fn update_sub_cell() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );
        grid.update_sub_cell(5, 5, 1, Some(4));
        let occ = grid.get(5, 5).unwrap();
        let inf: Vec<(u64, u8)> = occ.infantry(MovementLayer::Ground).collect();
        assert_eq!(inf, vec![(1, 4)]);
    }

    #[test]
    fn count_on_layer() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            2,
            MovementLayer::Ground,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            3,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        assert_eq!(grid.count_on_layer(5, 5, MovementLayer::Ground), 2);
        assert_eq!(grid.count_on_layer(5, 5, MovementLayer::Bridge), 1);
        assert_eq!(grid.count_on_layer(5, 5, MovementLayer::Air), 0);
    }

    #[test]
    fn non_buildings_prepend_on_same_layer() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let ids: Vec<u64> = grid
            .get(5, 5)
            .unwrap()
            .iter_layer(MovementLayer::Ground)
            .map(|o| o.entity_id)
            .collect();
        assert_eq!(ids, vec![2, 1]);
    }

    #[test]
    fn buildings_append_on_same_layer() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            100,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        grid.add(
            5,
            5,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let ids: Vec<u64> = grid
            .get(5, 5)
            .unwrap()
            .iter_layer(MovementLayer::Ground)
            .map(|o| o.entity_id)
            .collect();
        assert_eq!(ids, vec![2, 1, 100]);
    }

    #[test]
    fn ordered_queries_select_exact_layer_and_first_matching_category() {
        let mut entities = crate::sim::entity_store::EntityStore::new();
        let mut ground_aircraft =
            crate::sim::game_entity::GameEntity::test_default(10, "ORCA", "Allies", 5, 5);
        ground_aircraft.category = EntityCategory::Aircraft;
        let mut ground_building =
            crate::sim::game_entity::GameEntity::test_default(20, "GAPOWR", "Allies", 5, 5);
        ground_building.category = EntityCategory::Structure;
        let mut deck_building =
            crate::sim::game_entity::GameEntity::test_default(30, "GAPOWR", "Allies", 5, 5);
        deck_building.category = EntityCategory::Structure;
        entities.insert(ground_aircraft);
        entities.insert(ground_building);
        entities.insert(deck_building);

        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            20,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        grid.add(
            5,
            5,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            30,
            MovementLayer::Bridge,
            None,
            CellListInsertion::AppendBuilding,
        );

        assert_eq!(
            grid.first_category_on_layer(
                5,
                5,
                MovementLayer::Ground,
                EntityCategory::Aircraft,
                &entities,
            ),
            Some(10)
        );
        assert_eq!(
            grid.first_building_on_layer(5, 5, MovementLayer::Ground),
            Some(20)
        );
        assert_eq!(
            grid.first_building_on_layer(5, 5, MovementLayer::Bridge),
            Some(30)
        );
        assert_eq!(
            grid.get(5, 5)
                .unwrap()
                .snapshot_layer(MovementLayer::Ground),
            vec![10, 20]
        );
    }

    #[test]
    fn layers_have_independent_order() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            10,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            20,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let ground: Vec<u64> = grid
            .get(5, 5)
            .unwrap()
            .iter_layer(MovementLayer::Ground)
            .map(|o| o.entity_id)
            .collect();
        let bridge: Vec<u64> = grid
            .get(5, 5)
            .unwrap()
            .iter_layer(MovementLayer::Bridge)
            .map(|o| o.entity_id)
            .collect();
        assert_eq!(ground, vec![2, 1]);
        assert_eq!(bridge, vec![20, 10]);
    }

    #[test]
    fn remove_preserves_remaining_order() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            5,
            5,
            3,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.remove(5, 5, 2);
        let ids: Vec<u64> = grid
            .get(5, 5)
            .unwrap()
            .iter_layer(MovementLayer::Ground)
            .map(|o| o.entity_id)
            .collect();
        assert_eq!(ids, vec![3, 1]);
    }

    #[test]
    fn transition_removes_old_layer_inserts_new_layer() {
        // GATE A2 / P5: the authoritative two-layer crossing removes from the OLD
        // cell on the OLD object-list layer and inserts into the NEW cell on the
        // NEW layer. Step-onto-deck: occupant starts on Ground at the old cell and
        // lands on Bridge at the new cell.
        let mut grid = OccupancyGrid::new();
        grid.add(
            1,
            1,
            9,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.move_entity_layered(
            1,
            1,
            2,
            2,
            9,
            MovementLayer::Ground, // OLD layer
            MovementLayer::Bridge, // NEW layer
            None,
            CellListInsertion::PrependNonBuilding,
        );
        // Old cell emptied (the ground entry was removed); new cell holds the
        // occupant on the BRIDGE layer, not Ground.
        assert!(grid.get(1, 1).is_none());
        assert_eq!(grid.count_on_layer(2, 2, MovementLayer::Ground), 0);
        assert_eq!(grid.count_on_layer(2, 2, MovementLayer::Bridge), 1);
        assert!(grid.contains_entity(2, 2, 9));
    }

    #[test]
    fn remove_on_layer_walks_only_the_selected_layer() {
        // GATE A2: RemoveContent walks ONLY the selected per-cell list. With a
        // single occupant tagged Ground, removing on the WRONG (Bridge) layer is a
        // no-op; removing on the right (Ground) layer removes it.
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            7,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.remove_on_layer(5, 5, 7, MovementLayer::Bridge);
        assert!(
            grid.contains_entity(5, 5, 7),
            "wrong-layer remove must miss"
        );
        grid.remove_on_layer(5, 5, 7, MovementLayer::Ground);
        assert!(grid.get(5, 5).is_none(), "right-layer remove must hit");
    }

    #[test]
    fn saved_memberships_retain_lists_and_derive_only_lookup_metadata() {
        let mut entities = crate::sim::entity_store::EntityStore::new();
        let mut infantry = GameEntity::test_default(1, "JUMPJET", "Americans", 5, 5);
        infantry.category = EntityCategory::Infantry;
        infantry.sub_cell = Some(3);
        infantry.lifecycle.cell_marked = true;
        infantry.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Jumpjet,
            ),
        );
        infantry.locomotor.as_mut().unwrap().layer = MovementLayer::Air;
        entities.insert(infantry);
        let mut building = GameEntity::test_default(2, "CABHUT", "Neutral", 5, 5);
        building.category = EntityCategory::Structure;
        entities.insert(building);
        let mut grid = OccupancyGrid::new();
        grid.add(
            5,
            5,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        grid.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            Some(3),
            CellListInsertion::PrependNonBuilding,
        );
        let raw = bincode::serialize(&grid).unwrap();
        let mut restored: OccupancyGrid = bincode::deserialize(&raw).unwrap();
        restored.restore_memberships(&entities).unwrap();
        assert_eq!(
            restored
                .get(5, 5)
                .unwrap()
                .snapshot_layer(MovementLayer::Ground),
            vec![1, 2]
        );
        assert_eq!(
            restored
                .get(5, 5)
                .unwrap()
                .infantry(MovementLayer::Ground)
                .collect::<Vec<_>>(),
            vec![(1, 3)]
        );
        assert!(
            restored
                .get(5, 5)
                .unwrap()
                .has_building_on(MovementLayer::Ground)
        );
        assert!(
            !OccupancyGrid::rebuild(&entities).contains_entity(5, 5, 1),
            "the legacy live-layer projection loses this saved member"
        );
        restored.remove_on_layer(5, 5, 1, MovementLayer::Ground);
        assert_eq!(
            restored
                .get(5, 5)
                .unwrap()
                .first_on_layer(MovementLayer::Ground),
            Some(2)
        );
        entities.remove(2);
        assert!(
            restored
                .restore_memberships(&entities)
                .unwrap_err()
                .contains("missing object 2")
        );
    }

    #[test]
    fn membership_hash_uses_selected_lists_not_storage_interleaving_or_generation() {
        let hash = |grid: &OccupancyGrid| {
            use std::hash::Hasher;
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            grid.hash_memberships(&mut hasher);
            hasher.finish()
        };
        let mut first = OccupancyGrid::new();
        first.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        first.add(
            5,
            5,
            2,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let mut second = OccupancyGrid::new();
        second.add(
            5,
            5,
            2,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        second.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        second.remove(9, 9, 99);
        assert_eq!(hash(&first), hash(&second));
        second.add(
            5,
            5,
            3,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        assert_ne!(hash(&first), hash(&second));
    }

    #[test]
    fn move_entity_reinserts_with_requested_order() {
        let mut grid = OccupancyGrid::new();
        grid.add(
            1,
            1,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.add(
            2,
            2,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        grid.move_entity(
            1,
            1,
            2,
            2,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let ids: Vec<u64> = grid
            .get(2, 2)
            .unwrap()
            .iter_layer(MovementLayer::Ground)
            .map(|o| o.entity_id)
            .collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn rebuild_uses_category_insertion() {
        let mut entities = crate::sim::entity_store::EntityStore::new();
        let mut first = crate::sim::game_entity::GameEntity::test_default(1, "E1", "Allies", 5, 5);
        first.category = EntityCategory::Infantry;
        first.sub_cell = Some(2);
        first.lifecycle.cell_marked = true;
        let mut second =
            crate::sim::game_entity::GameEntity::test_default(2, "HTNK", "Allies", 5, 5);
        second.category = EntityCategory::Unit;
        second.lifecycle.cell_marked = true;
        let mut structure =
            crate::sim::game_entity::GameEntity::test_default(100, "GAPOWR", "Allies", 5, 5);
        structure.category = EntityCategory::Structure;
        structure.lifecycle.cell_marked = true;
        entities.insert(first);
        entities.insert(second);
        entities.insert(structure);
        let grid = OccupancyGrid::rebuild(&entities);
        let ids: Vec<u64> = grid
            .get(5, 5)
            .unwrap()
            .iter_layer(MovementLayer::Ground)
            .map(|o| o.entity_id)
            .collect();
        assert_eq!(ids, vec![2, 1, 100]);
    }

    #[test]
    fn rebuild_uses_cell_entry_order_not_stable_id_order() {
        let mut entities = crate::sim::entity_store::EntityStore::new();
        let mut structure =
            crate::sim::game_entity::GameEntity::test_default(100, "GAPOWR", "Allies", 5, 5);
        structure.category = EntityCategory::Structure;
        structure.occupancy_enter_order = 1;
        structure.lifecycle.cell_marked = true;
        let mut older_mobile =
            crate::sim::game_entity::GameEntity::test_default(50, "MTNK", "Allies", 5, 5);
        older_mobile.category = EntityCategory::Unit;
        older_mobile.occupancy_enter_order = 2;
        older_mobile.lifecycle.cell_marked = true;
        let mut newer_mobile =
            crate::sim::game_entity::GameEntity::test_default(10, "HTNK", "Allies", 5, 5);
        newer_mobile.category = EntityCategory::Unit;
        newer_mobile.occupancy_enter_order = 3;
        newer_mobile.lifecycle.cell_marked = true;

        entities.insert(newer_mobile);
        entities.insert(older_mobile);
        entities.insert(structure);

        let grid = OccupancyGrid::rebuild(&entities);
        let ids: Vec<u64> = grid
            .get(5, 5)
            .unwrap()
            .iter_layer(MovementLayer::Ground)
            .map(|o| o.entity_id)
            .collect();
        assert_eq!(ids, vec![10, 50, 100]);
    }

    #[test]
    fn rebuild_expands_structure_foundation_cells() {
        let mut entities = crate::sim::entity_store::EntityStore::new();
        let mut structure =
            crate::sim::game_entity::GameEntity::test_default(100, "GAPOWR", "Allies", 5, 5);
        structure.category = EntityCategory::Structure;
        structure.foundation = "2x2".to_string();
        structure.lifecycle.cell_marked = true;
        entities.insert(structure);

        let grid = OccupancyGrid::rebuild(&entities);

        for cell in [(5, 5), (5, 6), (6, 5), (6, 6)] {
            assert!(
                grid.contains_entity(cell.0, cell.1, 100),
                "structure should occupy foundation cell {cell:?}"
            );
        }
        assert_eq!(grid.occupied_cell_count(), 4);
    }

    #[test]
    fn lifecycle_authority_alive_limbo_does_not_rebuild_occupancy() {
        let mut entities = crate::sim::entity_store::EntityStore::new();
        let limbo = crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Allies", 5, 5);

        assert!(limbo.lifecycle.object_alive);
        assert!(limbo.lifecycle.in_limbo);
        assert!(!limbo.lifecycle.cell_marked);
        entities.insert(limbo);

        let grid = OccupancyGrid::rebuild(&entities);
        assert!(!grid.contains_entity(5, 5, 1));
        assert_eq!(grid.occupied_cell_count(), 0);
    }

    fn gsi_04_05_unit(stable_id: u64, rx: u16, ry: u16) -> crate::sim::game_entity::GameEntity {
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(stable_id, "MTNK", "Allies", rx, ry);
        entity.category = EntityCategory::Unit;
        entity.lifecycle.cell_marked = true;
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        entity.drive_locomotion = Some(DriveLocomotionRuntime::default());
        entity
    }

    #[test]
    fn gsi_04_05_head_to_premark_is_separate_from_object_list() {
        let mut objects = OccupancyGrid::new();
        objects.add(
            2,
            2,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let mut bits = CellOccupationGrid::new();
        bits.mark_vehicle_on_layer(2, 2, 1, MovementLayer::Ground);
        let mut drive = DriveLocomotionRuntime::default();
        replace_drive_head_to_occupation(
            &mut true,
            &mut drive,
            &mut bits,
            1,
            (2, 2),
            MovementLayer::Ground,
            DriveOccupationFootprint {
                rx: 3,
                ry: 2,
                layer: MovementLayer::Ground,
            },
        );

        assert!(objects.contains_entity(2, 2, 1));
        assert!(!objects.contains_entity(3, 2, 1));
        assert_eq!(bits.vehicle_bits(2, 2, MovementLayer::Ground), 0x20);
        assert_eq!(bits.vehicle_bits(3, 2, MovementLayer::Ground), 0x20);
    }

    #[test]
    fn gsi_04_05_paid_same_cell_point_clears_current_not_head_or_list() {
        let mut foot_occupation_enabled = true;
        let mut objects = OccupancyGrid::new();
        objects.add(
            2,
            2,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let mut bits = CellOccupationGrid::new();
        bits.mark_vehicle_on_layer(2, 2, 1, MovementLayer::Ground);
        bits.mark_vehicle_on_layer(3, 2, 1, MovementLayer::Ground);
        let mut drive = DriveLocomotionRuntime {
            occupation_head_to: Some(DriveOccupationFootprint {
                rx: 3,
                ry: 2,
                layer: MovementLayer::Ground,
            }),
            ..Default::default()
        };

        clear_current_drive_occupation_for_paid_point(
            &mut foot_occupation_enabled,
            &mut drive,
            &mut bits,
            1,
            (2, 2),
            MovementLayer::Ground,
        );

        assert!(objects.contains_entity(2, 2, 1));
        assert_eq!(bits.vehicle_bits(2, 2, MovementLayer::Ground), 0);
        assert_eq!(bits.vehicle_bits(3, 2, MovementLayer::Ground), 0x20);
        assert!(!foot_occupation_enabled);
    }

    #[test]
    fn gsi_04_05_actual_crossing_relinks_and_remarks_committed_cell() {
        let mut foot_occupation_enabled = false;
        let mut objects = OccupancyGrid::new();
        objects.add(
            2,
            2,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let mut bits = CellOccupationGrid::new();
        let mut drive = DriveLocomotionRuntime {
            occupation_head_to: Some(DriveOccupationFootprint {
                rx: 3,
                ry: 2,
                layer: MovementLayer::Ground,
            }),
            ..Default::default()
        };
        bits.mark_vehicle_on_layer(3, 2, 1, MovementLayer::Ground);

        objects.move_entity(
            2,
            2,
            3,
            2,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        mark_current_drive_occupation_after_crossing(
            &mut foot_occupation_enabled,
            &mut drive,
            &mut bits,
            1,
            (3, 2),
            MovementLayer::Ground,
        );

        assert!(!objects.contains_entity(2, 2, 1));
        assert!(objects.contains_entity(3, 2, 1));
        assert_eq!(bits.vehicle_bits(3, 2, MovementLayer::Ground), 0x20);
        assert!(foot_occupation_enabled);
    }

    #[test]
    fn gsi_04_05_soft_stop_preserves_committed_head_to_reservation() {
        let mut entity = gsi_04_05_unit(1, 2, 2);
        let head = DriveOccupationFootprint {
            rx: 3,
            ry: 2,
            layer: MovementLayer::Ground,
        };
        entity.drive_locomotion.as_mut().unwrap().occupation_head_to = Some(head);
        let mut bits = CellOccupationGrid::rebuild(&{
            let mut entities = crate::sim::entity_store::EntityStore::new();
            entities.insert(entity.clone());
            entities
        });

        crate::sim::movement::clear_navigation_for_entity(&mut entity);

        assert_eq!(
            entity.drive_locomotion.as_ref().unwrap().occupation_head_to,
            Some(head)
        );
        assert_eq!(bits.vehicle_bits(3, 2, MovementLayer::Ground), 0x20);
        bits.reconcile_entity(&entity);
        assert_eq!(bits.vehicle_bits(3, 2, MovementLayer::Ground), 0x20);
    }

    #[test]
    fn gsi_04_05_reconcile_one_owner_leaves_large_unrelated_set_untouched() {
        const UNRELATED_OWNERS: u16 = 2_048;

        let mut entity = gsi_04_05_unit(1, 1, 1);
        entity.drive_locomotion.as_mut().unwrap().occupation_head_to =
            Some(DriveOccupationFootprint {
                rx: 2,
                ry: 1,
                layer: MovementLayer::Ground,
            });
        let mut entities = crate::sim::entity_store::EntityStore::new();
        entities.insert(entity.clone());
        let mut bits = CellOccupationGrid::rebuild(&entities);

        for offset in 0..UNRELATED_OWNERS {
            let entity_id = u64::from(offset) + 2;
            let rx = offset + 100;
            let layer = if offset & 1 == 0 {
                MovementLayer::Ground
            } else {
                MovementLayer::Bridge
            };
            bits.mark_vehicle_on_layer(rx, 10, entity_id, layer);
        }

        entity.position.rx = 3;
        entity.drive_locomotion.as_mut().unwrap().occupation_head_to =
            Some(DriveOccupationFootprint {
                rx: 3,
                ry: 1,
                layer: MovementLayer::Ground,
            });
        bits.reconcile_entity(&entity);

        assert_eq!(bits.vehicle_bits(1, 1, MovementLayer::Ground), 0);
        assert_eq!(bits.vehicle_bits(2, 1, MovementLayer::Ground), 0);
        assert_eq!(bits.vehicle_bits(3, 1, MovementLayer::Ground), 0x20);
        assert_eq!(
            bits.footprints_by_owner
                .get(&1)
                .expect("reconciled owner footprint")
                .len(),
            1,
            "coincident current/head roles are one native OR-bit footprint"
        );
        assert_eq!(
            bits.footprints_by_owner.len(),
            usize::from(UNRELATED_OWNERS) + 1
        );
        for offset in 0..UNRELATED_OWNERS {
            let rx = offset + 100;
            let layer = if offset & 1 == 0 {
                MovementLayer::Ground
            } else {
                MovementLayer::Bridge
            };
            assert_eq!(bits.vehicle_bits(rx, 10, layer), 0x20);
        }
    }

    #[test]
    fn gsi_04_05_hard_remove_clears_head_then_current_footprint() {
        let mut bits = CellOccupationGrid::new();
        bits.mark_vehicle_on_layer(2, 2, 1, MovementLayer::Ground);
        bits.mark_vehicle_on_layer(3, 2, 1, MovementLayer::Ground);
        let mut drive = DriveLocomotionRuntime {
            occupation_head_to: Some(DriveOccupationFootprint {
                rx: 3,
                ry: 2,
                layer: MovementLayer::Ground,
            }),
            ..Default::default()
        };

        clear_drive_head_to_occupation_for_remove(&mut drive, &mut bits, 1);
        bits.clear_vehicle_on_layer(2, 2, 1, MovementLayer::Ground);

        assert_eq!(drive.occupation_head_to, None);
        assert_eq!(bits.vehicle_bits(2, 2, MovementLayer::Ground), 0);
        assert_eq!(bits.vehicle_bits(3, 2, MovementLayer::Ground), 0);
    }

    #[test]
    fn gsi_04_05_owner_ignores_own_reservation_other_mover_does_not() {
        let mut bits = CellOccupationGrid::new();
        bits.mark_vehicle_on_layer(3, 2, 1, MovementLayer::Ground);

        assert!(!bits.occupied_by_other(3, 2, MovementLayer::Ground, 1));
        assert!(bits.occupied_by_other(3, 2, MovementLayer::Ground, 2));
    }

    #[test]
    fn gsi_04_05_ground_deck_independent_and_elevated_clear_needs_no_bridge_flag() {
        let mut bits = CellOccupationGrid::new();
        bits.mark_vehicle_by_height(4, 4, 1, false, true);
        bits.mark_vehicle_by_height(4, 4, 2, true, true);
        assert_eq!(bits.vehicle_bits(4, 4, MovementLayer::Ground), 0x20);
        assert_eq!(bits.vehicle_bits(4, 4, MovementLayer::Bridge), 0x20);

        let cleared = bits.clear_vehicle_by_height(4, 4, 2, true);
        assert_eq!(cleared, MovementLayer::Bridge);
        assert_eq!(bits.vehicle_bits(4, 4, MovementLayer::Bridge), 0);
        assert_eq!(bits.vehicle_bits(4, 4, MovementLayer::Ground), 0x20);
    }

    #[test]
    fn gsi_04_05_normal_finish_promotes_endpoint_and_clears_runtime_head() {
        let mut foot_occupation_enabled = false;
        let mut bits = CellOccupationGrid::new();
        bits.mark_vehicle_on_layer(3, 2, 1, MovementLayer::Ground);
        let mut drive = DriveLocomotionRuntime {
            occupation_head_to: Some(DriveOccupationFootprint {
                rx: 3,
                ry: 2,
                layer: MovementLayer::Ground,
            }),
            ..Default::default()
        };

        finish_drive_head_to_occupation(
            &mut foot_occupation_enabled,
            &mut drive,
            &mut bits,
            1,
            (3, 2),
            MovementLayer::Ground,
        );

        assert_eq!(drive.occupation_head_to, None);
        assert!(foot_occupation_enabled);
        assert_eq!(bits.vehicle_bits(3, 2, MovementLayer::Ground), 0x20);
    }

    #[test]
    fn gsi_04_05_rebuild_restores_active_current_and_head_footprint() {
        let mut entities = crate::sim::entity_store::EntityStore::new();
        let mut unit = gsi_04_05_unit(1, 2, 2);
        unit.drive_locomotion.as_mut().unwrap().occupation_head_to =
            Some(DriveOccupationFootprint {
                rx: 3,
                ry: 2,
                layer: MovementLayer::Ground,
            });
        entities.insert(unit);

        let bits = CellOccupationGrid::rebuild(&entities);

        assert_eq!(bits.vehicle_bits(2, 2, MovementLayer::Ground), 0x20);
        assert_eq!(bits.vehicle_bits(3, 2, MovementLayer::Ground), 0x20);
    }

    #[test]
    fn gsi_04_05_hidden_garefn_exact_counter_contribution() {
        let mut grid = HiddenOccupationGrid::new();
        let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
        profile.occupy_height = 2;
        profile.add_occupy[0] = Some((-1, 0));
        profile.add_occupy[1] = Some((-1, -1));
        profile.remove_occupy[0] = Some((3, 1));

        assert!(grid.enter_building((10, 10), "4x3", profile, Some((32, 32))));
        assert_eq!(grid.entry_count(), 13);
        assert_eq!(grid.count(10, 10), 1);
        assert_eq!(grid.count(13, 11), 0);
        assert_eq!(grid.count(9, 10), 1);
        assert_eq!(grid.count(9, 9), 1);

        assert!(grid.exit_building((10, 10), "4x3", profile, Some((32, 32))));
        assert_eq!(grid.entry_count(), 0);
    }

    #[test]
    fn gsi_04_05_hidden_narefn_exact_counter_contribution() {
        let mut grid = HiddenOccupationGrid::new();
        let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
        profile.occupy_height = 4;
        profile.remove_occupy = [
            Some((0, -2)),
            Some((1, -1)),
            Some((1, -2)),
            Some((2, -1)),
            Some((-2, 0)),
            Some((-2, -1)),
            Some((-2, -2)),
            Some((3, 1)),
        ];

        assert!(grid.enter_building((10, 10), "4x3", profile, Some((32, 32))));
        assert_eq!(grid.entry_count(), 16);
        for offset in profile.remove_occupy.into_iter().flatten() {
            assert_eq!(
                grid.count(
                    (i32::from(10u16) + i32::from(offset.0)) as u16,
                    (i32::from(10u16) + i32::from(offset.1)) as u16,
                ),
                0,
                "RemoveOccupy slot {offset:?}"
            );
        }
        assert_eq!(grid.count(10, 10), 1);
        assert_eq!(grid.count(9, 9), 1);

        assert!(grid.exit_building((10, 10), "4x3", profile, Some((32, 32))));
        assert_eq!(grid.entry_count(), 0);
    }

    #[test]
    fn gsi_04_05_hidden_overlap_counts_slots_and_exit_is_guarded() {
        let mut grid = HiddenOccupationGrid::new();
        let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
        profile.add_occupy[0] = Some((0, 0));
        profile.add_occupy[1] = Some((0, 0));
        profile.remove_occupy[0] = Some((0, 0));

        grid.enter_building((8, 8), "1x1", profile, Some((20, 20)));
        assert_eq!(grid.count(8, 8), 2, "diagonal + two Add - one Remove");
        grid.exit_building((8, 8), "1x1", profile, Some((20, 20)));
        assert_eq!(grid.count(8, 8), 0, "final extra Add decrement is guarded");
    }

    #[test]
    fn gsi_04_05_hidden_remove_is_not_readded_on_exit() {
        let mut grid = HiddenOccupationGrid::new();
        let base = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
        grid.enter_building((20, 20), "1x1", base, Some((40, 40)));
        assert_eq!(grid.count(20, 20), 1);

        let mut suppressor = base;
        suppressor.remove_occupy[0] = Some((10, 10));
        grid.enter_building((10, 10), "1x1", suppressor, Some((40, 40)));
        assert_eq!(grid.count(20, 20), 0);
        grid.exit_building((10, 10), "1x1", suppressor, Some((40, 40)));
        assert_eq!(grid.count(20, 20), 0);
    }

    #[test]
    fn gsi_04_05_hidden_refinery_hole_is_not_a_diagonal_source() {
        let mut grid = HiddenOccupationGrid::new();
        let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
        profile.occupy_height = 4;

        grid.enter_building((10, 10), "3x3Refinery", profile, Some((32, 32)));
        assert_eq!(grid.count(12, 11), 0, "native offset (2,1) is a hole");
        assert_eq!(grid.count(12, 10), 1);
        assert_eq!(grid.count(12, 12), 1);

        grid.exit_building((10, 10), "3x3Refinery", profile, Some((32, 32)));
        assert_eq!(grid.entry_count(), 0);
    }

    #[test]
    fn gsi_04_05_hidden_entry_interleaves_add_then_remove_per_slot() {
        let mut grid = HiddenOccupationGrid::new();
        let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
        profile.remove_occupy[0] = Some((1, 0));
        profile.add_occupy[1] = Some((1, 0));

        grid.enter_building((10, 10), "0x0", profile, Some((32, 32)));
        assert_eq!(grid.count(10, 10), 0, "0x0 has no foundation offsets");
        assert_eq!(
            grid.count(11, 10),
            1,
            "slot 0 Remove is guarded before slot 1 Add increments"
        );

        grid.exit_building((10, 10), "0x0", profile, Some((32, 32)));
        assert_eq!(grid.entry_count(), 0);
    }

    #[test]
    fn gsi_04_05_hidden_off_map_cells_are_ignored_and_increment_wraps() {
        let mut grid = HiddenOccupationGrid::new();
        let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
        profile.occupy_height = 3;
        profile.add_occupy[0] = Some((-1, 0));
        profile.add_occupy[1] = Some((4, 0));
        grid.enter_building((0, 0), "1x1", profile, Some((4, 4)));
        assert_eq!(grid.entry_count(), 1);
        assert_eq!(grid.count(0, 0), 1);

        grid.cells.insert((2, 2), u32::MAX);
        grid.increment((2, 2));
        assert_eq!(grid.count(2, 2), 0);

        let before = grid.clone();
        profile.can_hide_things = false;
        assert!(!grid.enter_building((1, 1), "1x1", profile, Some((4, 4))));
        assert_eq!(grid, before);
    }
}
