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
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::collections::BTreeSet;

use crate::map::entities::EntityCategory;
use crate::sim::cell_map::CellMap;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::fixed_math::SimFixed;

/// UnitClass vehicle-occupation bit in both CellClass occupation planes.
pub(crate) const VEHICLE_OCCUPATION_BIT: u8 = 0x20;
/// Generic ObjectClass occupation bit used by landed AircraftClass objects.
pub(crate) const OBJECT_OCCUPATION_BIT: u8 = 0x40;
pub(crate) const BUILDING_OCCUPATION_BIT: u8 = 0x80;

/// Object identities on a CellClass ground/deck list. Terrain remains owned
/// by ProductionState; this view inserts its member between mobile objects
/// and the building tail, as the native AddContent registration paths do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellObjectMember {
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

/// The cell list Mark addresses for this object now: none unless its layer
/// query (vt+0x78, [`entity_layer`]) answers Ground, then the OnBridge
/// (`+0x8C`) one. `MapClass::Place_Down` (`0x005683C0`) and `Pick_Up`
/// (`0x005687F0`) ask that query for every object (`0x00568405`,
/// `0x00568835`) and touch the cell only on a Ground answer, passing OnBridge
/// to AddContent (`0x005684B1`); a Foot's Mark also asks it first
/// (`0x004D37A6`). A Ground answer does not depend on rules.
///
/// It answers for the object as it stands, marked or not, because Mark(UP)
/// asks after clearing +0x74. Whether the object is listed now also takes
/// `cell_marked`.
///
/// [`entity_layer`]: crate::sim::world::entity_layer
pub(crate) fn cell_list_layer_for_entity(
    entity: &GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<MovementLayer> {
    (crate::sim::world::entity_layer(entity, terrain, None)
        == crate::sim::world::display_layers::DisplayLayer::GROUND)
        .then_some(if entity.on_bridge() {
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

impl RawCellOccupation {
    /// One plane of an object's Mark (`put`) or Clear: Mark ORs `mask` into
    /// the byte and stores the object's owner index beside it; Clear removes
    /// `mask` and resets the owner once functional sub-cells 2..4 (`0x1C`) are
    /// clear. Bits 0/1 do not retain it.
    fn write(&mut self, layer: MovementLayer, mask: u8, owner: Option<InternedId>, put: bool) {
        let (bits, retained_owner) = match layer {
            MovementLayer::Ground => (&mut self.ground, &mut self.ground_infantry_owner),
            MovementLayer::Bridge => (&mut self.deck, &mut self.deck_infantry_owner),
            MovementLayer::Air | MovementLayer::Underground => return,
        };
        if put {
            *bits |= mask;
            *retained_owner = owner;
        } else {
            *bits &= !mask;
            if *bits & 0x1C == 0 {
                *retained_owner = None;
            }
        }
    }
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
            crate::map::cell_index::NativeCellIdentity::Real(_) => {
                let (x, y) = terrain.native_cell_coord(cell);
                Self::Real(x as u16, y as u16)
            }
            crate::map::cell_index::NativeCellIdentity::Dummy => Self::Dummy,
        }
    }
}

/// Sparse canonical storage for the raw ground/deck occupation bytes.
///
/// An absent entry is exactly `(ground = 0, deck = 0)`. Zero entries are
/// removed eagerly, so serialization and deterministic hashing have one
/// representation for an unoccupied cell. This state is independent of the
/// CellClass-style object lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RawCellOccupationGrid {
    cells: CellMap<RawCellOccupation>,
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
        self.occupant_at(key, layer).0
    }

    /// The receiver's raw byte on `layer` and the infantry owner retained
    /// beside it, from one lookup.
    pub(crate) fn occupant_at(
        &self,
        key: RawCellKey,
        layer: MovementLayer,
    ) -> (u8, Option<InternedId>) {
        let cell = match key {
            RawCellKey::Real(x, y) => self.cells.get(&(x, y)),
            RawCellKey::Dummy => Some(&self.dummy),
        };
        cell.map_or((0, None), |cell| match layer {
            MovementLayer::Ground => (cell.ground, cell.ground_infantry_owner),
            MovementLayer::Bridge => (cell.deck, cell.deck_infantry_owner),
            MovementLayer::Air | MovementLayer::Underground => (0, None),
        })
    }

    /// An object's Mark/Clear on the raw byte and owner index of the receiver
    /// it retained after the ground query: `InfantryClass` (`0x005217C0` /
    /// `0x00521850`) and `AnimClass` (`0x00426270` / `0x00426300`). In
    /// particular, a later missing lookup does not allocate a cell. `owner` is
    /// the object's vtable `+0x38` answer: an infantryman's house, `None` (-1)
    /// for an anim (`0x00410490`).
    pub(crate) fn write_occupant(
        &mut self,
        key: RawCellKey,
        layer: MovementLayer,
        mask: u8,
        owner: Option<InternedId>,
        put: bool,
    ) {
        match key {
            RawCellKey::Real(x, y) if put => {
                if mask != 0 && matches!(layer, MovementLayer::Ground | MovementLayer::Bridge) {
                    self.cells
                        .get_or_insert_with((x, y), Default::default)
                        .write(layer, mask, owner, true);
                }
            }
            RawCellKey::Real(x, y) => {
                self.update_and_prune(x, y, |cell| cell.write(layer, mask, None, false));
            }
            RawCellKey::Dummy => self.dummy.write(layer, mask, owner, put),
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
        self.mark_ground_at(RawCellKey::Real(rx, ry), mask);
    }

    /// Literal ground OR, preserving the independent Infantry owner. Terrain
    /// MarkOccupation71C110 may select Dummy from retained Object.Location
    /// even when PlaceDown5683C0 admitted a different real footprint cell.
    pub(crate) fn mark_ground_at(&mut self, key: RawCellKey, mask: u8) {
        if mask == 0 {
            return;
        }
        match key {
            RawCellKey::Real(rx, ry) => {
                self.cells
                    .get_or_insert_with((rx, ry), Default::default)
                    .ground |= mask;
            }
            RawCellKey::Dummy => self.dummy.ground |= mask,
        }
    }

    pub(crate) fn clear_ground(&mut self, rx: u16, ry: u16, mask: u8) {
        self.clear_ground_at(RawCellKey::Real(rx, ry), mask);
    }

    /// Literal ground-bit AND, including TerrainLimbo71C9E7..71C9F6's
    /// retained CellClass receiver. Unlike UnmarkCellOccupancy this leaves
    /// the Infantry owner untouched, even on the shared Dummy.
    pub(crate) fn clear_ground_at(&mut self, key: RawCellKey, mask: u8) {
        match key {
            RawCellKey::Real(rx, ry) => {
                self.update_and_prune(rx, ry, |cell| cell.ground &= !mask);
            }
            RawCellKey::Dummy => self.dummy.ground &= !mask,
        }
    }

    pub(crate) fn ground_bits(&self, rx: u16, ry: u16) -> u8 {
        self.cells.get(&(rx, ry)).map_or(0, |cell| cell.ground)
    }

    #[cfg(any(test, feature = "test-support"))]
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
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn mark_ground_infantry(&mut self, rx: u16, ry: u16, mask: u8, owner: InternedId) {
        let key = RawCellKey::Real(rx, ry);
        self.write_occupant(key, MovementLayer::Ground, mask, Some(owner), true);
    }

    /// Native: `InfantryClass::UnmarkCellOccupancy` @ `0x00521850` (vtable
    /// `+0xF4`, `0x007EB14C`) resets the selected owner to `0xFFFFFFFF` only
    /// once `byte & 0x1C == 0`, i.e. after functional sub-cells 2..4 are all
    /// clear. Bits 0/1 do not retain it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn clear_ground_infantry(&mut self, rx: u16, ry: u16, mask: u8) {
        let key = RawCellKey::Real(rx, ry);
        self.write_occupant(key, MovementLayer::Ground, mask, None, false);
    }

    pub(crate) fn mark_deck(&mut self, rx: u16, ry: u16, mask: u8) {
        if mask != 0 {
            self.cells
                .get_or_insert_with((rx, ry), Default::default)
                .deck |= mask;
        }
    }

    pub(crate) fn clear_deck(&mut self, rx: u16, ry: u16, mask: u8) {
        self.update_and_prune(rx, ry, |cell| cell.deck &= !mask);
    }

    #[cfg(test)]
    pub(crate) fn deck_bits(&self, rx: u16, ry: u16) -> u8 {
        self.cells.get(&(rx, ry)).map_or(0, |cell| cell.deck)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn deck_infantry_owner(&self, rx: u16, ry: u16) -> Option<InternedId> {
        self.cells
            .get(&(rx, ry))
            .and_then(|cell| cell.deck_infantry_owner)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn mark_deck_infantry(&mut self, rx: u16, ry: u16, mask: u8, owner: InternedId) {
        let key = RawCellKey::Real(rx, ry);
        self.write_occupant(key, MovementLayer::Bridge, mask, Some(owner), true);
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
        self.cells.iter().map(|((rx, ry), cell)| {
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
    cells: CellMap<u32>,
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
        foundation: &crate::rules::foundation::Foundation,
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
        foundation: &crate::rules::foundation::Foundation,
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
        // Installed Ares10015504 uses signed JLE; the original568A97 only
        // tests zero. Both share this counter owner, not separate grids.
        #[cfg(feature = "ares")]
        if (*current as i32) <= 0 {
            return;
        }
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
        self.cells.iter().map(|((rx, ry), &count)| (rx, ry, count))
    }
}

/// `CellClass+0xE0`, the single airborne object a cell holds (the AltObject
/// slot).
///
/// Raw storage only. The shared Foot air owner implements Cell487D70's
/// admission and ordered Foot4E00B0 notifications. Tracker membership and the
/// Foot's two cached Cells are independent of this grid.
///
/// Serialized verbatim alongside the raw occupation planes: which owner holds a
/// slot, and when it was released, is not reconstructible from entity positions
/// or locomotor phase.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AirSlotGrid {
    cells: CellMap<u64>,
}

impl AirSlotGrid {
    /// The object holding the cell's slot, if any.
    pub fn holder(&self, rx: u16, ry: u16) -> Option<u64> {
        self.cells.get(&(rx, ry)).copied()
    }

    /// Raw Cell+E0 write, without admission or a Foot notification. Native
    /// Foot4E00B0 also clears a matching old slot directly through this storage.
    pub(crate) fn set_holder(&mut self, cell: (i16, i16), owner: Option<u64>) {
        let cell = (cell.0 as u16, cell.1 as u16);
        if let Some(owner) = owner {
            self.cells.insert(cell, owner);
        } else {
            self.cells.remove(&cell);
        }
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.cells.len()
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (u16, u16, u64)> + '_ {
        self.cells.iter().map(|((rx, ry), &owner)| (rx, ry, owner))
    }
}

fn hidden_diagonal_cells(
    origin: (u16, u16),
    foundation: &crate::rules::foundation::Foundation,
    occupy_height: i32,
    map_size: Option<(u16, u16)>,
) -> BTreeSet<(u16, u16)> {
    let depth = occupy_height.saturating_sub(1).max(1);
    let mut cells = BTreeSet::new();
    for (dx, dy) in foundation.cell_offsets() {
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

    /// Infantry occupants on a given layer, preserving layer-list order.
    #[cfg(test)]
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

    /// Whether this cell holds a structure on the given layer.
    ///
    /// Vehicles do not count: this is the predicate for gamemd's per-cell
    /// building lookup, which returns only BuildingClass objects.
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
    cells: CellMap<CellOccupancy>,
    /// Monotonic counter bumped on every cell-membership mutation, so tests can
    /// tell whether membership changed between two points. Never serialized,
    /// never part of the state hash.
    #[cfg(any(test, feature = "test-support"))]
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
        for (cell, occupants) in self.cells.iter() {
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
                occupant.is_building = entity.category() == EntityCategory::Structure;
                occupant.sub_cell = if entity.category() == EntityCategory::Infantry {
                    entity.sub_cell()
                } else {
                    None
                };
            }
        }
        Ok(())
    }

    /// Hash each native ground/deck list independently: the Vec's interleaving
    /// of the two lists is only Rust storage. Empty retained map buckets and the
    /// test-only generation counter do not change membership. This stronger Rust peer
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
        for (cell, occupants) in self.cells.iter() {
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
    /// VERA-only: projects the CellClass lists from each entity's Mark state
    /// (`lifecycle.cell_marked`) and transport role; live play's lists come from Mark.
    #[cfg(test)]
    pub fn rebuild(entities: &crate::sim::entity_store::EntityStore) -> Self {
        let mut grid = Self::new();
        let mut ordered: Vec<&GameEntity> = entities.values().collect();
        ordered.sort_by_key(|entity| entity.stable_id());
        for entity in ordered {
            // Global storage, native-alive, limbo, and cell-list membership are
            // independent facts. Only an object whose Mark transaction succeeded
            // participates in this rebuilt cache.
            if !entity.lifecycle.cell_marked() {
                continue;
            }
            // Entities inside transports don't occupy cells.
            if entity.passenger_role.is_inside_transport() {
                continue;
            }
            let Some(layer) = cell_list_layer_for_entity(entity, None) else {
                continue;
            };
            let sid = entity.stable_id();
            let sub = if entity.category() == EntityCategory::Infantry {
                entity.sub_cell()
            } else {
                None
            };
            let insertion = CellListInsertion::from_category(entity.category());
            for (rx, ry) in entity_occupancy_cells(entity) {
                grid.add(rx, ry, sid, layer, sub, insertion);
            }
        }
        grid
    }
}

impl GameEntity {
    /// Derived membership in this object's retained Mark footprint. Lifecycle
    /// admission and actual per-cell lists remain with their existing owners.
    /// This exposes a read decision across the app/core boundary, not the list.
    pub fn footprint_contains_cell(&self, cell: (u16, u16)) -> bool {
        entity_occupancy_cells(self).contains(&cell)
    }
}

/// The cells an object's Mark lists it in: its own cell, or a building's
/// occupy list (`vt+0x108`, Type `+0xDFC`), which Place_Down's AddContent
/// (`0x0047E8A0`) walks. The lists are row-major rectangles except
/// `3x3Refinery`'s, which omits `(2, 1)`.
pub(crate) fn entity_occupancy_cells(entity: &GameEntity) -> Vec<(u16, u16)> {
    if entity.category() != EntityCategory::Structure {
        return vec![(entity.position.rx(), entity.position.ry())];
    }
    entity
        .type_facts
        .foundation()
        .cell_offsets()
        .into_iter()
        .filter_map(|(dx, dy)| {
            Some((
                entity.position.rx().checked_add_signed(dx)?,
                entity.position.ry().checked_add_signed(dy)?,
            ))
        })
        .collect()
}

impl OccupancyGrid {
    /// Create an empty occupancy grid.
    pub fn new() -> Self {
        Self {
            cells: CellMap::new(),
            #[cfg(any(test, feature = "test-support"))]
            generation: 0,
        }
    }

    /// Current mutation generation. Bumped on every `add`/`remove`/`remove_on_layer`
    /// (and thus `move_entity`). Compare across two points to detect whether cell
    /// membership changed in between. Transient — not hashed, resets to 0 on rebuild.
    #[cfg(any(test, feature = "test-support"))]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Bump the test-support `generation`; a no-op in other builds.
    #[inline]
    fn bump_generation(&mut self) {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.generation = self.generation.wrapping_add(1);
        }
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
        self.bump_generation();
        let new_occupant = CellOccupant {
            entity_id,
            layer,
            sub_cell,
            is_building: insertion == CellListInsertion::AppendBuilding,
        };
        let occ = self.cells.get_or_insert_with((rx, ry), Default::default);
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
    #[cfg(test)]
    pub fn remove(&mut self, rx: u16, ry: u16, entity_id: u64) {
        self.bump_generation();
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
    /// crossing the removal must observe the OLD (pre-transition) layer. No-op
    /// if no matching-layer entry exists.
    ///
    /// For the single-entry-per-cell invariant this grid maintains (each entity is
    /// `add`ed exactly once per cell), the per-layer result equals the layer-agnostic
    /// `remove`; the per-layer form is the authoritative two-layer selector and keeps
    /// the remove/add halves on the verified independent layers.
    pub fn remove_on_layer(&mut self, rx: u16, ry: u16, entity_id: u64, layer: MovementLayer) {
        self.bump_generation();
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
    /// across the move (teleport, same-layer steps).
    #[cfg(test)]
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
    pub(crate) fn cell_objects<'a, I>(
        &'a self,
        rx: u16,
        ry: u16,
        layer: MovementLayer,
        terrain_ids: I,
    ) -> impl Iterator<Item = CellObjectMember> + 'a
    where
        I: IntoIterator<Item = u64> + 'a,
        I::IntoIter: 'a,
    {
        let mut terrain = terrain_ids
            .into_iter()
            .filter(move |_| layer == MovementLayer::Ground)
            .peekable();
        let mut entities = self
            .get(rx, ry)
            .into_iter()
            .flat_map(move |cell| cell.iter_layer(layer))
            .peekable();
        std::iter::from_fn(move || {
            if terrain.peek().is_some() && entities.peek().is_none_or(|member| member.is_building) {
                return terrain.next().map(CellObjectMember::Terrain);
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
    /// Callers own native GameActive (`A8E9A0`) admission; this is the stored
    /// list query for an admitted active context. Health, dying and limbo are
    /// not query predicates: Mark owns list membership and lifetime. Original
    /// 47C520..54E is exercised by the consumed engineer_repair_admission
    /// controls in tools/spatial_oracle; inactive app/teardown lifetime is not
    /// inferred from ScenarioSession.started (a different native byte).
    pub fn first_building_on_layer(&self, rx: u16, ry: u16, layer: MovementLayer) -> Option<u64> {
        self.get(rx, ry)?
            .iter_layer(layer)
            .find(|occupant| occupant.is_building)
            .map(|occupant| occupant.entity_id)
    }

    /// Typed Cell query over a selected native list. Unit `47EBA0` and Infantry
    /// `47EC40` share this traversal; a nonzero native layer argument selects
    /// Bridge, otherwise Ground. Select the first category match before caller
    /// eligibility checks, without health, dying or limbo filters.
    /// Callers own native GameActive (`A8E9A0`) admission. Original Unit query
    /// and caller controls are consumed from `tools/spatial_oracle/cmin_dock`
    /// and `cell_entry_crush_tail`; this method owns stored-list selection only.
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
                .is_some_and(|entity| entity.category() == category)
                .then_some(occupant.entity_id)
        })
    }

    /// Check if a cell has no occupants on a given layer.
    pub fn is_empty_on_layer(&self, rx: u16, ry: u16, layer: MovementLayer) -> bool {
        self.cells
            .get(&(rx, ry))
            .is_none_or(|occ| occ.is_empty_on(layer))
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
            .filter_map(move |(cell, occupants)| (!occupants.is_empty_on(layer)).then_some(cell))
    }

    /// Check if a specific entity is in a specific cell.
    pub fn contains_entity(&self, rx: u16, ry: u16, entity_id: u64) -> bool {
        self.cells
            .get(&(rx, ry))
            .is_some_and(|occ| occ.occupants.iter().any(|o| o.entity_id == entity_id))
    }

    /// The list (ground or deck) this cell links the entity in, if any.
    pub(crate) fn listed_layer(&self, rx: u16, ry: u16, entity_id: u64) -> Option<MovementLayer> {
        self.cells
            .get(&(rx, ry))?
            .occupants
            .iter()
            .find(|o| o.entity_id == entity_id)
            .map(|o| o.layer)
    }

    /// Total number of occupied cells (for diagnostics).
    #[cfg(any(test, feature = "test-support"))]
    pub fn occupied_cell_count(&self) -> usize {
        self.cells.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        grid.remove(2, 2, 10);
        assert!(grid.generation() > g2, "remove must bump generation");
    }

    #[test]
    fn gsi_04_05_hidden_garefn_exact_counter_contribution() {
        let mut grid = HiddenOccupationGrid::new();
        let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile {
            occupy_height: 2,
            ..Default::default()
        };
        profile.add_occupy[0] = Some((-1, 0));
        profile.add_occupy[1] = Some((-1, -1));
        profile.remove_occupy[0] = Some((3, 1));

        assert!(grid.enter_building((10, 10), &"4x3".into(), profile, Some((32, 32))));
        assert_eq!(grid.entry_count(), 13);
        assert_eq!(grid.count(10, 10), 1);
        assert_eq!(grid.count(13, 11), 0);
        assert_eq!(grid.count(9, 10), 1);
        assert_eq!(grid.count(9, 9), 1);

        assert!(grid.exit_building((10, 10), &"4x3".into(), profile, Some((32, 32))));
        assert_eq!(grid.entry_count(), 0);
    }
}
