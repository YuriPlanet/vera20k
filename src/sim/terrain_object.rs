//! Live map terrain object state and lifecycle helpers.
//!
//! This module owns deterministic sim state for `TerrainClass`-style objects
//! loaded from map `[Terrain]`. TIBTRE ore spawners are a derived index of this
//! live state, not the lifecycle owner.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_object_type::TerrainObjectType;
use crate::rules::warhead_type::WarheadType;
use crate::sim::cell_kernel::{WorldCoordinate, cell_center};
use crate::sim::combat::damage;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::occupancy::RawCellOccupationGrid;
use crate::sim::terrain_spawn::TerrainAnimationState;

#[cfg(test)]
#[path = "terrain_coordinate_tests.rs"]
mod coordinate_tests;

const TERRAIN_LIMBO_CLEAR_BIT: u8 = 0x40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TerrainObjectLifecycle {
    Live,
    Limbo,
    Destroyed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerrainObjectState {
    stable_id: u64,
    /// Load-only numeric `AbstractClass` identity. Stable Rust storage remains
    /// the runtime owner, but retaining this value prevents later constructor
    /// chronology from becoming an unobservable counter approximation.
    #[serde(default)]
    native_unique_id: Option<i32>,
    /// LogicClass membership is reconstructed from the serialized mixed order.
    #[serde(skip)]
    in_logic_vector: bool,
    type_ref: InternedId,
    rx: u16,
    ry: u16,
    /// Terrain-relative CellClass list order. CellAddContent47E91C prepends
    /// each constructor; OccupancyGrid merges this subsequence with Technos
    /// for the supported Terrain-before-Techno scenario loading boundary.
    next_in_cell: Option<u64>,
    /// Retained ObjectClass Location.Z after TerrainType+6C (71E0D0)
    /// clamps the constructor coordinate to Map578080 ground height.
    /// Ground changes do not relocate the object. See terrain_coordinate.json.
    world_z_leptons: i32,
    health: i32,
    max_health: i32,
    /// Immutable current-theater type projection. Keep the full dword for
    /// CellClass::RecalcZoneType483C80; Mark/Unmark use only bits1/2/4.
    occupation_bits: i32,
    /// Immutable type projection used to maintain cell admission after unlink.
    spawns_tiberium: bool,
    lifecycle: TerrainObjectLifecycle,
}

impl std::hash::Hash for TerrainObjectState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.stable_id.hash(state);
        self.type_ref.hash(state);
        self.rx.hash(state);
        self.ry.hash(state);
        self.next_in_cell.hash(state);
        self.world_z_leptons.hash(state);
        self.health.hash(state);
        self.max_health.hash(state);
        self.occupation_bits.hash(state);
        self.spawns_tiberium.hash(state);
        self.lifecycle.hash(state);
    }
}

impl TerrainObjectState {
    pub fn new(
        stable_id: u64,
        type_ref: InternedId,
        cell: (u16, u16),
        terrain_type: &TerrainObjectType,
        snow_theater: bool,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> Self {
        let WorldCoordinate { x, y, .. } = cell_center((cell.0 as i16, cell.1 as i16).into(), 0);
        // Terrain ctor71BC4A..71BC76 passes centered signed XY and Z0;
        // Object Unlimbo5F4F9C dispatches TerrainType71E0D0 before the raw
        // coordinate write5F6940. The type callback retains max(0, ground).
        let world_z_leptons =
            crate::sim::movement::ground_pose::ground_surface_z_at([x, y], false, terrain, None)
                .unwrap_or(0)
                .max(0);
        Self {
            stable_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref,
            rx: cell.0,
            ry: cell.1,
            next_in_cell: None,
            world_z_leptons,
            health: terrain_type.strength,
            max_health: terrain_type.strength,
            occupation_bits: terrain_type.raw_occupation_bits(snow_theater),
            spawns_tiberium: terrain_type.spawns_tiberium,
            lifecycle: TerrainObjectLifecycle::Live,
        }
    }

    /// Terrain VT7F522C+48 inherits Object GetCoords5F65A0. Its XY is
    /// the authored cell center; Z is the coordinate retained at construction.
    pub fn world_coord(&self) -> crate::sim::projectile::ProjectileCoord {
        let (rx, ry) = self.cell();
        let WorldCoordinate { x, y, .. } = cell_center((rx as i16, ry as i16).into(), 0);
        crate::sim::projectile::ProjectileCoord::new(x, y, self.world_z_leptons)
    }

    /// Terrain retains Object5F5F40/5F6B90/5F4260. Its live admitted
    /// lifecycle represents Marked; Limbo removes that membership.
    pub(crate) fn height(&self, terrain: Option<&ResolvedTerrainGrid>) -> i32 {
        let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        self.height_in_query(cells.as_ref())
    }

    pub(crate) fn height_in_query(
        &self,
        cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    ) -> i32 {
        let coord = self.world_coord();
        crate::sim::movement::air_movement::object_height_in_query(
            crate::sim::components::DriveCoord {
                x: coord.x,
                y: coord.y,
                z: coord.z,
            },
            false,
            cells,
        )
    }

    pub(crate) fn is_in_air(&self, terrain: Option<&ResolvedTerrainGrid>) -> bool {
        let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        self.is_high_flying_in_query(cells.as_ref())
    }

    /// Object+50/5F6B60: the Marked guard precedes the live ground query.
    pub(crate) fn is_low_flying_in_query(
        &self,
        cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    ) -> bool {
        crate::sim::movement::air_movement::object_flight_predicate(
            self.is_live(),
            || self.height_in_query(cells),
            false,
        )
    }

    /// Object+54/5F6B90, retaining the caller's canonical/isolated policy.
    pub(crate) fn is_high_flying_in_query(
        &self,
        cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    ) -> bool {
        crate::sim::movement::air_movement::object_flight_predicate(
            self.is_live(),
            || self.height_in_query(cells),
            true,
        )
    }

    pub(crate) fn display_layer(
        &self,
        terrain: Option<&ResolvedTerrainGrid>,
        rules: &RuleSet,
    ) -> crate::sim::world::display_layers::DisplayLayer {
        crate::sim::world::display_layers::DisplayLayer::for_object(
            self.is_in_air(terrain),
            || self.height(terrain),
            rules.general.display_cruise_height,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(stable_id: u64, type_ref: InternedId, rx: u16, ry: u16) -> Self {
        Self {
            stable_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref,
            rx,
            ry,
            next_in_cell: None,
            world_z_leptons: 0,
            health: 100,
            max_health: 100,
            occupation_bits: 0,
            spawns_tiberium: false,
            lifecycle: TerrainObjectLifecycle::Live,
        }
    }

    pub fn cell(&self) -> (u16, u16) {
        (self.rx, self.ry)
    }

    pub const fn stable_id(&self) -> u64 {
        self.stable_id
    }

    pub const fn native_unique_id(&self) -> Option<i32> {
        self.native_unique_id
    }

    pub const fn in_logic_vector(&self) -> bool {
        self.in_logic_vector
    }

    /// ObjectClass::Unlimbo5F4FEF..5F5040 (original instructions): Terrain
    /// joins Logic only for IsLogic/SpawnsTiberium types. Modes0/5 bypass
    /// the native non-player identity (-2) exclusion; Display is independent.
    pub(crate) fn joins_logic_on_unlimbo(
        &self,
        terrain_type: &TerrainObjectType,
        game_mode: crate::sim::scenario_session::ScenarioGameMode,
    ) -> bool {
        terrain_type.is_logic()
            && terrain_type.spawns_tiberium
            && (game_mode.is_single_player() || self.native_unique_id != Some(-2))
    }

    pub const fn type_ref(&self) -> InternedId {
        self.type_ref
    }

    pub const fn health(&self) -> i32 {
        self.health
    }

    pub const fn max_health(&self) -> i32 {
        self.max_health
    }

    /// The type's full signed occupation value for the theater.
    pub const fn occupation_bits(&self) -> i32 {
        self.occupation_bits
    }

    pub const fn lifecycle(&self) -> TerrainObjectLifecycle {
        self.lifecycle
    }

    /// Fresh-authored construction spends and retains a native ID.
    pub(crate) fn with_native_unique_id(mut self, native_unique_id: Option<i32>) -> Self {
        self.native_unique_id = native_unique_id;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_health_for_test(&mut self, health: i32) {
        self.health = health;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_max_health_for_test(&mut self, max_health: i32) {
        self.max_health = max_health;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_occupation_bits_for_test(&mut self, occupation_bits: u8) {
        self.occupation_bits = i32::from(occupation_bits);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_lifecycle_for_test(&mut self, lifecycle: TerrainObjectLifecycle) {
        self.lifecycle = lifecycle;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_in_logic_vector_for_test(&mut self, member: bool) {
        self.in_logic_vector = member;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_native_unique_id_for_test(&mut self, native_unique_id: Option<i32>) {
        self.native_unique_id = native_unique_id;
    }

    pub fn is_live(&self) -> bool {
        self.lifecycle == TerrainObjectLifecycle::Live
    }

    /// Object+90 is independent of InLimbo+81. ObjectUnInit5F661F calls
    /// TerrainLimbo before clearing Alive at5F6625. The represented lethal
    /// finalizer makes that final state Destroyed; standalone Limbo retains
    /// Alive, as ObjectCtor5F3930 does before its first Unlimbo.
    pub(crate) fn object_alive(&self) -> bool {
        self.lifecycle != TerrainObjectLifecycle::Destroyed
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainDamageResult {
    Ignored,
    Damaged { remaining: i32 },
    Destroyed,
}

/// Captured lethal Terrain receiver state needed by the synchronous finalize tail.
///
/// The object remains represented with exact-zero health between receive and
/// finalize so a caller can run the verified nested C4 transaction first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerrainLethalDamage {
    pub(crate) stable_id: u64,
    pub(crate) cell: (u16, u16),
    pub(crate) spawns_tiberium: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerrainAreaReceiveResult {
    Ignored,
    Damaged { remaining: i32 },
    Lethal(TerrainLethalDamage),
}

/// The live `TerrainClass` objects and the indexes derived from them (the
/// snapshot's `TerrainObjectStore` identity registry).
///
/// Besides the objects by stable id it keeps three indexes of live objects:
/// the Terrain head on each cell (the non-Techno half of `Cell+0xE4`), the retained
/// animation of each IsAnimated object (TIBTRE ore spawners among them), and
/// the cells whose first Terrain has `SpawnsTiberium=yes`. Construction,
/// Limbo and disposal keep the indexes in step with the objects; nothing
/// outside this owner writes them.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TerrainObjectStore {
    animations: BTreeMap<u64, TerrainAnimationState>,
    objects: BTreeMap<u64, TerrainObjectState>,
    cells: BTreeMap<(u16, u16), u64>,
    tiberium_spawning_cells: BTreeSet<(u16, u16)>,
    #[serde(skip)]
    touched: crate::sim::touch_log::TouchLogs<1>,
    /// Derived movement-cache invalidation; membership changes count even
    /// when a surviving cell head has the same occupation mask.
    #[serde(skip)]
    membership_epoch: u64,
}

impl TerrainObjectStore {
    /// Every represented object by stable id.
    pub fn objects(&self) -> &BTreeMap<u64, TerrainObjectState> {
        &self.objects
    }

    pub fn get(&self, stable_id: u64) -> Option<&TerrainObjectState> {
        self.objects.get(&stable_id)
    }

    /// First Terrain on each cell, as GetTerrain47C550 selects it.
    pub fn cell_index(&self) -> &BTreeMap<(u16, u16), u64> {
        &self.cells
    }

    pub fn id_at(&self, cell: (u16, u16)) -> Option<u64> {
        self.cells.get(&cell).copied()
    }

    /// CellAddContent47E8A0 / RemoveContent47EA90 retain every Terrain in
    /// latest-first cell order, independently of constructor/Logic order.
    pub fn members_at(&self, cell: (u16, u16)) -> impl Iterator<Item = u64> + '_ {
        let mut next = self.id_at(cell);
        std::iter::from_fn(move || {
            let id = next?;
            let object = self.objects.get(&id)?;
            next = object.next_in_cell;
            Some(id)
        })
    }

    pub(crate) fn next_in_cell(&self, id: u64) -> Option<u64> {
        self.objects.get(&id)?.next_in_cell
    }

    pub(crate) fn take_touched(&mut self) -> crate::sim::touch_log::Touched {
        self.touched.take(0)
    }

    pub(crate) fn membership_epoch(&self) -> u64 {
        self.membership_epoch
    }

    /// TerrainPointerExpired71CFD0 delegates ObjectPointerExpired5F5230.
    /// The inherited removed=true callback bypasses an expired successor
    /// before its later Limbo, retaining that successor's own Next for now.
    pub(crate) fn pointer_expired(&mut self, listener: u64, expired: u64, removed: bool) -> bool {
        if !removed || self.next_in_cell(listener) != Some(expired) {
            return false;
        }
        let next = self.next_in_cell(expired);
        self.objects.get_mut(&listener).unwrap().next_in_cell = next;
        self.touched.note(listener, self.objects.len());
        true
    }

    /// Validate serialized links before any iterator can follow them. Native
    /// ObjectLoad5F5E80 swizzles the saved Next; it does not reconstruct order.
    pub(crate) fn validate_membership(&self) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for (&cell, &head) in &self.cells {
            let mut next = Some(head);
            while let Some(id) = next {
                let Some(object) = self.objects.get(&id) else {
                    return Err(format!("Terrain cell {cell:?} links missing object {id}"));
                };
                if !object.is_live() || object.cell() != cell || !seen.insert(id) {
                    return Err(format!(
                        "Terrain cell {cell:?} has invalid or repeated member {id}"
                    ));
                }
                next = object.next_in_cell;
            }
        }
        for (&id, object) in &self.objects {
            if object.is_live() != seen.contains(&id) {
                return Err(format!("Terrain {id} lifetime and cell membership differ"));
            }
            let mut links = BTreeSet::new();
            let mut next = object.next_in_cell;
            while let Some(link) = next {
                let Some(successor) = self.objects.get(&link) else {
                    return Err(format!("Terrain {id} links missing successor {link}"));
                };
                if link == id || successor.cell() != object.cell() || !links.insert(link) {
                    return Err(format!("Terrain {id} has invalid successor {link}"));
                }
                next = successor.next_in_cell;
            }
        }
        for (&id, animation) in &self.animations {
            if !self
                .objects
                .get(&id)
                .is_some_and(|object| object.is_live() && object.type_ref == animation.type_ref())
            {
                return Err(format!("Terrain animation {id} has no matching live owner"));
            }
        }
        let spawning: BTreeSet<_> = self
            .cells
            .iter()
            .filter(|(_, id)| self.objects[id].spawns_tiberium)
            .map(|(&cell, _)| cell)
            .collect();
        if spawning != self.tiberium_spawning_cells {
            return Err("Terrain spawner-cell index differs from live types".into());
        }
        Ok(())
    }

    /// The retained animation of each IsAnimated live object. TerrainAI71C730
    /// and Save71CF30 address Stage/rate/timer on the instance, never its cell.
    pub fn animations(&self) -> &BTreeMap<u64, TerrainAnimationState> {
        &self.animations
    }

    /// Cells whose first Terrain has `SpawnsTiberium=yes`. This is a
    /// different gate from [`Self::animations`]: a non-animated spawner
    /// rejects new ore, while an animated non-spawner still draws RNG.
    pub fn spawning_cells(&self) -> &BTreeSet<(u16, u16)> {
        &self.tiberium_spawning_cells
    }

    /// The world hash's terrain fold, in its established order.
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        for (&stable_id, spawner) in &self.animations {
            stable_id.hash(hasher);
            spawner.hash(hasher);
        }
        for (&stable_id, terrain) in &self.objects {
            stable_id.hash(hasher);
            terrain.hash(hasher);
        }
        for (&(rx, ry), &stable_id) in &self.cells {
            rx.hash(hasher);
            ry.hash(hasher);
            stable_id.hash(hasher);
        }
        for &(rx, ry) in &self.tiberium_spawning_cells {
            rx.hash(hasher);
            ry.hash(hasher);
        }
    }

    /// `[Terrain]` construction starts from an empty store.
    pub(crate) fn clear(&mut self) {
        let next_epoch = self.membership_epoch.wrapping_add(1);
        *self = Self::default();
        self.membership_epoch = next_epoch;
    }

    /// A constructed object takes its cell, and a `SpawnsTiberium=` type its
    /// spawner cell.
    pub(crate) fn insert_constructed(
        &mut self,
        mut terrain: TerrainObjectState,
        spawns_tiberium: bool,
    ) {
        let cell = terrain.cell();
        terrain.spawns_tiberium = spawns_tiberium;
        terrain.next_in_cell = self.cells.insert(cell, terrain.stable_id);
        self.membership_epoch = self.membership_epoch.wrapping_add(1);
        self.touched.note(terrain.stable_id, self.objects.len());
        self.objects.insert(terrain.stable_id, terrain);
        if spawns_tiberium {
            self.tiberium_spawning_cells.insert(cell);
        } else {
            self.tiberium_spawning_cells.remove(&cell);
        }
    }

    /// The animation index is reseeded from the live objects once the
    /// overlay registry is installed.
    pub(crate) fn clear_animations(&mut self) {
        self.animations.clear();
    }

    pub(crate) fn seed_animation(&mut self, stable_id: u64, animation: TerrainAnimationState) {
        self.animations.insert(stable_id, animation);
    }

    /// The TerrainClass AI leaf's view: the instance's animation to advance, beside
    /// the spawner cells and the cell index its ore placement reads.
    pub(crate) fn animation_with_indexes_mut(
        &mut self,
        stable_id: u64,
    ) -> Option<(
        &mut TerrainAnimationState,
        &BTreeSet<(u16, u16)>,
        &BTreeMap<(u16, u16), u64>,
    )> {
        let animation = self.animations.get_mut(&stable_id)?;
        Some((animation, &self.tiberium_spawning_cells, &self.cells))
    }

    /// LogicClass membership of a represented object; false when the store
    /// holds no such object.
    pub(crate) fn set_logic_membership(&mut self, stable_id: u64, member: bool) -> bool {
        let Some(terrain) = self.objects.get_mut(&stable_id) else {
            return false;
        };
        terrain.in_logic_vector = member;
        true
    }

    /// Every object leaves LogicClass before the membership is rebuilt from
    /// the retained order.
    pub(crate) fn clear_logic_memberships(&mut self) {
        for terrain in self.objects.values_mut() {
            terrain.in_logic_vector = false;
        }
    }

    /// RemoveContent47EA90 replaces only this member's head/predecessor link.
    fn unlink(&mut self, stable_id: u64) {
        let Some(object) = self.objects.get(&stable_id) else {
            return;
        };
        let cell = object.cell();
        let successor = object.next_in_cell;
        let predecessor = self
            .members_at(cell)
            .find(|&id| self.next_in_cell(id) == Some(stable_id));
        let found = if self.id_at(cell) == Some(stable_id) {
            if let Some(successor) = successor {
                self.cells.insert(cell, successor);
            } else {
                self.cells.remove(&cell);
            }
            true
        } else if let Some(predecessor) = predecessor {
            self.objects.get_mut(&predecessor).unwrap().next_in_cell = successor;
            self.touched.note(predecessor, self.objects.len());
            true
        } else {
            false
        };
        // RemoveContent47EAF0 clears Next only on a found member. A prior
        // removed=true expiry may already have bypassed a non-head object;
        // its retained Next still participates in later expiry while deferred.
        if found {
            self.objects.get_mut(&stable_id).unwrap().next_in_cell = None;
            self.touched.note(stable_id, self.objects.len());
        }
    }

    /// Storage disposal cannot erase the successor's animation or membership.
    pub(crate) fn remove(&mut self, stable_id: u64) -> Option<TerrainObjectState> {
        self.unlink(stable_id);
        self.animations.remove(&stable_id);
        let terrain = self.objects.remove(&stable_id)?;
        self.refresh_spawning_cell(terrain.cell());
        self.membership_epoch = self.membership_epoch.wrapping_add(1);
        self.touched.note(stable_id, self.objects.len());
        Some(terrain)
    }

    fn refresh_spawning_cell(&mut self, cell: (u16, u16)) {
        // CanPlaceTiberium483969..48399A stops at the first Terrain and
        // reads only that type's Spawns flag. Older spawners do not veto it.
        if self
            .id_at(cell)
            .is_some_and(|id| self.objects[&id].spawns_tiberium)
        {
            self.tiberium_spawning_cells.insert(cell);
        } else {
            self.tiberium_spawning_cells.remove(&cell);
        }
    }

    /// TerrainLimbo71C930 / Unmark71C070 remove the requested instance.
    /// Raw bits are literally AND-cleared even with another Terrain present;
    /// RecalcZone483C80 instead reads the first surviving Terrain's full mask.
    pub(crate) fn limbo(
        &mut self,
        stable_id: u64,
        raw_occupation: &mut RawCellOccupationGrid,
        resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    ) -> bool {
        let Some(snapshot) = self.objects.get(&stable_id).cloned() else {
            return false;
        };
        if !snapshot.is_live() {
            return false;
        }

        let source_cell = snapshot.cell();
        raw_occupation.clear_ground(source_cell.0, source_cell.1, TERRAIN_LIMBO_CLEAR_BIT);
        self.unlink(stable_id);
        unmark_terrain_raw_occupation(raw_occupation, source_cell, snapshot.occupation_bits);
        if let Some(grid) = resolved_terrain {
            let occupation = self
                .id_at(source_cell)
                .map(|id| self.objects[&id].occupation_bits);
            grid.set_terrain_object_occupation(source_cell, occupation);
        }
        if let Some(terrain) = self.objects.get_mut(&stable_id) {
            terrain.lifecycle = TerrainObjectLifecycle::Limbo;
        }
        self.membership_epoch = self.membership_epoch.wrapping_add(1);
        self.animations.remove(&stable_id);
        // The cell admission index represents the first Terrain type, not animation state.
        // Immutable type projection on every instance keeps its invalidation
        // local to this owner even for non-animated SpawnsTiberium types.
        self.refresh_spawning_cell(source_cell);
        true
    }

    /// ObjectUnInit5F661F..5F6625 delegates Limbo and clears Alive even when
    /// the caller is not a lethal damage receiver.
    pub(crate) fn uninit(
        &mut self,
        stable_id: u64,
        raw_occupation: &mut RawCellOccupationGrid,
        resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    ) -> bool {
        if !self
            .objects
            .get(&stable_id)
            .is_some_and(TerrainObjectState::object_alive)
        {
            return false;
        }
        self.limbo(stable_id, raw_occupation, resolved_terrain);
        self.objects.get_mut(&stable_id).unwrap().lifecycle = TerrainObjectLifecycle::Destroyed;
        true
    }

    /// Receive against the live objects. The recursion guard belongs to the
    /// outer damage operation; no persisted state leaves the store.
    pub(crate) fn receive_damage(
        &mut self,
        finalizing_terrain: &mut BTreeSet<u64>,
        stable_id: u64,
        cell: (u16, u16),
        raw_damage: i32,
        distance_leptons: i32,
        warhead: &WarheadType,
        rules: &RuleSet,
        interner: &StringInterner,
        scenario_no_damage: bool,
        ignore_defenses: bool,
    ) -> TerrainAreaReceiveResult {
        if finalizing_terrain.contains(&stable_id) {
            return TerrainAreaReceiveResult::Ignored;
        }

        let Some(snapshot) = self.objects.get(&stable_id) else {
            return TerrainAreaReceiveResult::Ignored;
        };
        // ObjectClass::ReceiveDamage 005F53A1..005F53B7 rejects these before
        // either ordinary armor calculation or the forced direct-damage path.
        if !snapshot.is_live() || snapshot.cell() != cell || snapshot.health <= 0 || raw_damage == 0
        {
            return TerrainAreaReceiveResult::Ignored;
        }
        let Some(terrain_type) =
            rules.terrain_object_type_case_insensitive(interner.resolve(snapshot.type_ref))
        else {
            return TerrainAreaReceiveResult::Ignored;
        };
        if !warhead.wood || terrain_type.immune {
            return TerrainAreaReceiveResult::Ignored;
        }

        // TerrainClass 0071B920 always checks Wood/Immune above, then forwards
        // ignore_defenses to ObjectClass 005F5390. BlowUpBridge 0047DD70 passes
        // true: current Health bypasses verses, falloff, NoDamage and MaxDamage.
        let resolved_damage = if ignore_defenses {
            raw_damage
        } else {
            damage::kernel::apply_warhead_damage(
                raw_damage,
                warhead.cell_spread_f64,
                warhead.percent_at_max_f64,
                warhead.interaction(terrain_type.armor_id()).multiplier(),
                distance_leptons,
                scenario_no_damage,
                rules.combat_damage.max_damage,
            )
        };
        if resolved_damage == 0 {
            return TerrainAreaReceiveResult::Ignored;
        }

        let terrain = self
            .objects
            .get_mut(&stable_id)
            .expect("captured Terrain receiver remains represented");
        if resolved_damage < 0 {
            terrain.health = terrain
                .health
                .wrapping_sub(resolved_damage)
                .min(terrain.max_health);
            return TerrainAreaReceiveResult::Damaged {
                remaining: terrain.health,
            };
        }

        let remaining = terrain.health.wrapping_sub(resolved_damage);
        if remaining > 0 {
            terrain.health = remaining;
            return TerrainAreaReceiveResult::Damaged { remaining };
        }

        terrain.health = 0;
        finalizing_terrain.insert(stable_id);
        TerrainAreaReceiveResult::Lethal(TerrainLethalDamage {
            stable_id,
            cell,
            spawns_tiberium: terrain_type.spawns_tiberium,
        })
    }

    /// Finalize only after nested C4 receivers return, against the same live
    /// objects.
    pub(crate) fn finalize_lethal(
        &mut self,
        raw_occupation: &mut RawCellOccupationGrid,
        finalizing_terrain: &mut BTreeSet<u64>,
        navigation_changed_cells: &mut Vec<(u16, u16)>,
        lethal: TerrainLethalDamage,
        resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    ) -> bool {
        if !finalizing_terrain.contains(&lethal.stable_id) {
            return false;
        }
        let finalized = self.uninit(lethal.stable_id, raw_occupation, resolved_terrain);
        if finalized {
            if !navigation_changed_cells.contains(&lethal.cell) {
                navigation_changed_cells.push(lethal.cell);
            }
        }
        finalizing_terrain.remove(&lethal.stable_id);
        finalized
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn objects_mut_for_test(&mut self) -> &mut BTreeMap<u64, TerrainObjectState> {
        self.touched.note_all();
        self.membership_epoch = self.membership_epoch.wrapping_add(1);
        &mut self.objects
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn cell_index_mut_for_test(&mut self) -> &mut BTreeMap<(u16, u16), u64> {
        &mut self.cells
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn animations_mut_for_test(&mut self) -> &mut BTreeMap<u64, TerrainAnimationState> {
        &mut self.animations
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn spawning_cells_mut_for_test(&mut self) -> &mut BTreeSet<(u16, u16)> {
        &mut self.tiberium_spawning_cells
    }
}

/// Inputs and outputs for phase-level Terrain receiver fixtures.
/// The adapter lends the store to a private world that runs the production
/// receiver. Production Terrain authority stays resident in Simulation; this
/// container and its transfers exist only in tests.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default, Clone)]
pub(crate) struct TerrainAreaState {
    store: TerrainObjectStore,
    raw_occupation: RawCellOccupationGrid,
    navigation_changed_cells: Vec<(u16, u16)>,
    finalizing_terrain: BTreeSet<u64>,
}

#[cfg(any(test, feature = "test-support"))]
impl TerrainAreaState {
    pub(crate) fn take_fixture_progress(&mut self) -> (Vec<(u16, u16)>, BTreeSet<u64>) {
        (
            std::mem::take(&mut self.navigation_changed_cells),
            std::mem::take(&mut self.finalizing_terrain),
        )
    }
    pub(crate) fn restore_fixture_progress(
        &mut self,
        cells: Vec<(u16, u16)>,
        finalizing: BTreeSet<u64>,
    ) {
        self.navigation_changed_cells = cells;
        self.finalizing_terrain = finalizing;
    }

    pub(crate) fn take_from(
        store: &mut TerrainObjectStore,
        raw_occupation: &mut RawCellOccupationGrid,
    ) -> Self {
        Self {
            store: std::mem::take(store),
            raw_occupation: std::mem::take(raw_occupation),
            navigation_changed_cells: Vec::new(),
            finalizing_terrain: BTreeSet::new(),
        }
    }

    /// Lend fixture fields to the adapter world, then reclaim its mutations.
    pub(crate) fn swap_authority(
        &mut self,
        store: &mut TerrainObjectStore,
        raw_occupation: &mut RawCellOccupationGrid,
    ) {
        std::mem::swap(&mut self.store, store);
        std::mem::swap(&mut self.raw_occupation, raw_occupation);
    }

    /// Return fixture fields and the receiver's ordered navigation receipts.
    pub(crate) fn restore_into(
        mut self,
        store: &mut TerrainObjectStore,
        raw_occupation: &mut RawCellOccupationGrid,
    ) -> Vec<(u16, u16)> {
        debug_assert!(
            self.finalizing_terrain.is_empty(),
            "every lethal Terrain receiver must finalize before authority restoration"
        );
        self.swap_authority(store, raw_occupation);
        self.navigation_changed_cells
    }

    pub(crate) fn navigation_changed_cells(&self) -> &[(u16, u16)] {
        &self.navigation_changed_cells
    }

    pub(crate) fn is_finalizing(&self, stable_id: u64) -> bool {
        self.finalizing_terrain.contains(&stable_id)
    }

    /// Enter the shared Object damage kernel for one captured Terrain receiver.
    pub(crate) fn receive_area_damage(
        &mut self,
        stable_id: u64,
        cell: (u16, u16),
        raw_damage: i32,
        distance_leptons: i32,
        warhead: &WarheadType,
        rules: &RuleSet,
        interner: &StringInterner,
    ) -> TerrainAreaReceiveResult {
        self.store.receive_damage(
            &mut self.finalizing_terrain,
            stable_id,
            cell,
            raw_damage,
            distance_leptons,
            warhead,
            rules,
            interner,
            false,
            false,
        )
    }

    /// Complete a lethal receiver after any nested C4 transaction has returned.
    pub(crate) fn finalize_lethal(
        &mut self,
        lethal: TerrainLethalDamage,
        resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    ) -> bool {
        self.store.finalize_lethal(
            &mut self.raw_occupation,
            &mut self.finalizing_terrain,
            &mut self.navigation_changed_cells,
            lethal,
            resolved_terrain,
        )
    }
}

/// TerrainClass::Mark_Occupation71C110 / Unmark_Occupation71C070,
/// original instructions: TEST BL,1/2/4 maps only those bits from the raw
/// signed INI dword into Cell+124 (`0x04|0x08|0x10`), without clamping.
pub(crate) fn terrain_raw_occupation_mask(source_mask: i32) -> u8 {
    ((source_mask & 0x07) as u8) << 2
}

pub(crate) fn mark_terrain_raw_occupation(
    raw_occupation: &mut RawCellOccupationGrid,
    cell: (u16, u16),
    source_mask: i32,
) {
    raw_occupation.mark_ground(cell.0, cell.1, terrain_raw_occupation_mask(source_mask));
}

fn unmark_terrain_raw_occupation(
    raw_occupation: &mut RawCellOccupationGrid,
    cell: (u16, u16),
    source_mask: i32,
) {
    raw_occupation.clear_ground(cell.0, cell.1, terrain_raw_occupation_mask(source_mask));
}

pub fn mark_terrain_occupation(
    terrain: &TerrainObjectState,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
) {
    if let Some(grid) = resolved_terrain {
        grid.set_terrain_object_occupation(terrain.cell(), Some(terrain.occupation_bits));
    }
}

pub fn unmark_terrain_occupation(
    terrain: &TerrainObjectState,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
) {
    if let Some(grid) = resolved_terrain {
        grid.set_terrain_object_occupation(terrain.cell(), None);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn damage_terrain_object_at_cell(
    store: &mut TerrainObjectStore,
    raw_occupation: &mut RawCellOccupationGrid,
    rules: &RuleSet,
    interner: &StringInterner,
    cell: (u16, u16),
    base_damage: i32,
    warhead: &WarheadType,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
) -> TerrainDamageResult {
    let mut area_state = TerrainAreaState::take_from(store, raw_occupation);
    let receive_result =
        area_state
            .store
            .id_at(cell)
            .map_or(TerrainAreaReceiveResult::Ignored, |stable_id| {
                area_state.receive_area_damage(
                    stable_id,
                    cell,
                    base_damage,
                    0,
                    warhead,
                    rules,
                    interner,
                )
            });
    let result = match receive_result {
        TerrainAreaReceiveResult::Ignored => TerrainDamageResult::Ignored,
        TerrainAreaReceiveResult::Damaged { remaining } => {
            TerrainDamageResult::Damaged { remaining }
        }
        TerrainAreaReceiveResult::Lethal(lethal) => {
            if area_state.finalize_lethal(lethal, resolved_terrain) {
                TerrainDamageResult::Destroyed
            } else {
                TerrainDamageResult::Ignored
            }
        }
    };
    let _ = area_state.restore_into(store, raw_occupation);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::overlay::TerrainObject;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, zone_class};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::terrain_rules::LandType;
    use crate::sim::terrain_spawn::seed_terrain_animations;
    use crate::sim::world::Simulation;

    fn terrain_rules(type_name: &str, wood: bool, type_section: &str) -> RuleSet {
        let wood = if wood { "yes" } else { "no" };
        let ini = IniFile::from_str(&format!(
            "[General]\nTreeStrength=10\n\
              [InfantryTypes]\n\
              [VehicleTypes]\n0=DUMMY\n\
              [AircraftTypes]\n\
              [BuildingTypes]\n\
              [TerrainTypes]\n0={}\n\
              [DUMMY]\nPrimary=Gun\nStrength=100\nArmor=heavy\n\
              [Gun]\nDamage=10\nWarhead=WH\n\
              [WH]\nWood={}\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\
              [{}]\nFixtureOnly=1\n{}",
            type_name, wood, type_name, type_section
        ));
        RuleSet::from_ini(&ini).expect("rules")
    }

    fn terrain_kernel_rules(warhead_section: &str, max_damage: i32) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[General]\nTreeStrength=100\n\
              [CombatDamage]\nMaxDamage={}\n\
              [InfantryTypes]\n\
              [VehicleTypes]\n0=DUMMY\n\
              [AircraftTypes]\n\
              [BuildingTypes]\n\
              [TerrainTypes]\n0=TREE01\n\
              [DUMMY]\nPrimary=Gun\nStrength=100\nArmor=heavy\n\
              [Gun]\nDamage=10\nWarhead=WH\n\
              [WH]\nWood=yes\n{}\n\
              [TREE01]\nStrength=100\nArmor=wood\nTemperateOccupationBits=7\n",
            max_damage, warhead_section
        ));
        RuleSet::from_ini(&ini).expect("kernel rules")
    }

    #[test]
    fn bridge_direct_terrain_damage_bypasses_kernel_and_finishes_removal() {
        let rules = terrain_kernel_rules(
            "Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\nCellSpread=1\nPercentAtMax=0",
            1,
        );
        let mut sim = Simulation::new();
        sim.session.set_no_damage_for_test(true);
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.terrain_objects.cell_index()[&(0, 0)];
        let health = sim.terrain_objects.objects()[&stable_id].health;
        let warhead_ref = sim.interner.intern("WH");
        sim.commit_direct_terrain_damage_receiver(
            &rules,
            None,
            crate::sim::combat::TerrainDamageEvent {
                stable_id,
                rx: 0,
                ry: 0,
                damage: health,
                distance_leptons: 512,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
            crate::sim::world::FrameEffects::default(),
        );
        let terrain = &sim.terrain_objects.objects()[&stable_id];
        assert_eq!(terrain.health, 0);
        assert_eq!(terrain.lifecycle, TerrainObjectLifecycle::Destroyed);
        assert!(
            !terrain.in_logic_vector,
            "direct receiver finishes retirement before returning"
        );
        assert!(!sim.terrain_objects.cell_index().contains_key(&(0, 0)));
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0);
    }

    #[test]
    fn bridge_direct_terrain_damage_keeps_wood_and_immune_gates() {
        for (wood, section) in [(false, ""), (true, "Immune=yes\n")] {
            let rules = terrain_rules("TREE01", wood, section);
            let mut sim = Simulation::new();
            seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
            let stable_id = sim.terrain_objects.cell_index()[&(0, 0)];
            let health = sim.terrain_objects.objects()[&stable_id].health;
            let warhead_ref = sim.interner.intern("WH");
            sim.commit_direct_terrain_damage_receiver(
                &rules,
                None,
                crate::sim::combat::TerrainDamageEvent {
                    stable_id,
                    rx: 0,
                    ry: 0,
                    damage: health,
                    distance_leptons: 0,
                    warhead_ref,
                    near_center_ic_isolation_eligible: false,
                },
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(sim.terrain_objects.objects()[&stable_id].health, health);
            assert!(sim.terrain_objects.objects()[&stable_id].is_live());
            assert_eq!(sim.terrain_objects.cell_index()[&(0, 0)], stable_id);
        }
    }

    #[test]
    fn bridge_direct_terrain_receiver_rejects_zero_health_during_nested_death() {
        let rules = terrain_rules("TREE01", true, "");
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.terrain_objects.cell_index()[&(0, 0)];
        // During a nested death callback, Health is zero before outer Limbo.
        sim.terrain_objects
            .objects_mut_for_test()
            .get_mut(&stable_id)
            .unwrap()
            .health = 0;
        let warhead_ref = sim.interner.intern("WH");
        sim.commit_direct_terrain_damage_receiver(
            &rules,
            None,
            crate::sim::combat::TerrainDamageEvent {
                stable_id,
                rx: 0,
                ry: 0,
                damage: -10,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.terrain_objects.objects()[&stable_id].health, 0);
        assert!(
            sim.terrain_objects.objects()[&stable_id].is_live(),
            "outer receiver still owns finalization"
        );
    }

    fn rules(tib_section: &str) -> RuleSet {
        let mut rules = terrain_rules(
            "TIBTRE01",
            true,
            &format!(
                "SpawnsTiberium=yes\nIsAnimated=yes\nAnimationRate=3\nAnimationProbability=1\n{}",
                tib_section
            ),
        );
        rules.set_terrain_spawner_frame_count_for_test("TIBTRE01", 22);
        rules
    }

    fn seed_one_at(sim: &mut Simulation, rules: &RuleSet, type_name: &str, cell: (u16, u16)) {
        seed_terrain_animations(
            sim,
            &[TerrainObject {
                rx: cell.0,
                ry: cell.1,
                name: type_name.to_string(),
            }],
            rules,
            false,
        );
    }

    fn resolved_clear_grid() -> ResolvedTerrainGrid {
        ResolvedTerrainGrid::from_cells(1, 1, vec![ResolvedTerrainCell::clear_for_test(0, 0)])
    }

    #[cfg(feature = "ares")]
    #[test]
    fn ares_custom_armor_terrain_receiver_uses_the_same_effective_record() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[ArmorTypes]\npaper=steel\n[General]\nTreeStrength=100\n\
             [TerrainTypes]\n0=TREE01\n[TREE01]\nStrength=100\nArmor=paper\n\
             [Warheads]\n0=WH\n[WH]\nWood=yes\nCellSpread=0\nVersus.paper=25%\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.terrain_objects.cell_index()[&(0, 0)];
        assert!(
            rules
                .terrain_object_type_case_insensitive("TREE01")
                .unwrap()
                .armor_id()
                .index()
                >= 11
        );
        let mut area = TerrainAreaState::take_from(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
        );
        // The executed custom-armor native kernel control101 *25% =25.
        assert_eq!(
            area.receive_area_damage(
                stable_id,
                (0, 0),
                101,
                0,
                rules.warhead("WH").unwrap(),
                &rules,
                &sim.interner,
            ),
            TerrainAreaReceiveResult::Damaged { remaining: 75 },
        );
        let _ = area.restore_into(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
        );
    }

    #[test]
    fn gsi_04_10_terrain_area_receive_uses_shared_fractional_kernel_and_max_damage() {
        let fractional_rules = terrain_kernel_rules(
            "CellSpread=1\n\
             PercentAtMax=0.5\n\
             Verses=100%,100%,100%,100%,100%,100%,50.5%,100%,100%,0%,0%",
            10_000,
        );
        let mut fractional = Simulation::new();
        seed_one_at(&mut fractional, &fractional_rules, "TREE01", (0, 0));
        let stable_id = fractional.terrain_objects.cell_index()[&(0, 0)];
        let warhead = fractional_rules.warhead("WH").expect("warhead");
        let mut area = TerrainAreaState::take_from(
            &mut fractional.terrain_objects,
            &mut fractional.substrate.raw_cell_occupation,
        );
        assert_eq!(
            area.receive_area_damage(
                stable_id,
                (0, 0),
                99,
                128,
                warhead,
                &fractional_rules,
                &fractional.interner,
            ),
            TerrainAreaReceiveResult::Damaged { remaining: 63 }
        );
        let _ = area.restore_into(
            &mut fractional.terrain_objects,
            &mut fractional.substrate.raw_cell_occupation,
        );

        let capped_rules = terrain_kernel_rules(
            "CellSpread=0\n\
             PercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,200%,100%,100%,0%,0%",
            25,
        );
        let mut capped = Simulation::new();
        seed_one_at(&mut capped, &capped_rules, "TREE01", (0, 0));
        let stable_id = capped.terrain_objects.cell_index()[&(0, 0)];
        let warhead = capped_rules.warhead("WH").expect("warhead");
        let mut area = TerrainAreaState::take_from(
            &mut capped.terrain_objects,
            &mut capped.substrate.raw_cell_occupation,
        );
        assert_eq!(
            area.receive_area_damage(
                stable_id,
                (0, 0),
                8_000,
                0,
                warhead,
                &capped_rules,
                &capped.interner,
            ),
            TerrainAreaReceiveResult::Damaged { remaining: 75 }
        );
        let _ = area.restore_into(
            &mut capped.terrain_objects,
            &mut capped.substrate.raw_cell_occupation,
        );
    }

    #[test]
    fn gsi_04_10_terrain_area_receive_heals_near_and_clamps_to_max() {
        let rules = terrain_rules("TREE01", true, "TemperateOccupationBits=7\n");
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.terrain_objects.cell_index()[&(0, 0)];
        let warhead = rules.warhead("WH").expect("warhead");
        let mut area = TerrainAreaState::take_from(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
        );
        area.store
            .objects_mut_for_test()
            .get_mut(&stable_id)
            .unwrap()
            .health = 4;

        assert_eq!(
            area.receive_area_damage(stable_id, (0, 0), -50, 7, warhead, &rules, &sim.interner,),
            TerrainAreaReceiveResult::Damaged { remaining: 10 }
        );
        area.store
            .objects_mut_for_test()
            .get_mut(&stable_id)
            .unwrap()
            .health = 4;
        assert_eq!(
            area.receive_area_damage(stable_id, (0, 0), -50, 8, warhead, &rules, &sim.interner,),
            TerrainAreaReceiveResult::Ignored
        );
        assert_eq!(area.store.objects()[&stable_id].health, 4);
        let _ = area.restore_into(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
        );
    }

    #[test]
    fn gsi_04_10_terrain_area_lethal_is_exact_zero_guarded_then_finalized_once() {
        let rules = rules("Immune=no\nTemperateOccupationBits=7\n");
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TIBTRE01", (0, 0));
        let stable_id = sim.terrain_objects.cell_index()[&(0, 0)];
        let terrain = sim.terrain_objects.objects()[&stable_id].clone();
        let mut grid = resolved_clear_grid();
        mark_terrain_occupation(&terrain, Some(&mut grid));
        let warhead = rules.warhead("WH").expect("warhead");
        let mut area = TerrainAreaState::take_from(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
        );

        let TerrainAreaReceiveResult::Lethal(lethal) =
            area.receive_area_damage(stable_id, (0, 0), 100, 0, warhead, &rules, &sim.interner)
        else {
            panic!("expected lethal Terrain receiver");
        };
        assert!(lethal.spawns_tiberium);
        assert_eq!(area.store.objects()[&stable_id].health, 0);
        assert_eq!(
            area.store.objects()[&stable_id].lifecycle,
            TerrainObjectLifecycle::Live
        );
        assert!(area.is_finalizing(stable_id));
        assert_eq!(
            area.receive_area_damage(stable_id, (0, 0), 100, 0, warhead, &rules, &sim.interner,),
            TerrainAreaReceiveResult::Ignored,
            "nested Wood=yes C4 reentry must not receive or finalize twice"
        );

        assert!(area.finalize_lethal(lethal, Some(&mut grid)));
        assert!(!area.finalize_lethal(lethal, Some(&mut grid)));
        assert!(!area.is_finalizing(stable_id));
        assert_eq!(area.navigation_changed_cells(), &[(0, 0)]);
        assert_eq!(
            area.store.objects()[&stable_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
        assert_eq!(area.raw_occupation.ground_bits(0, 0), 0);
        assert!(!area.store.cell_index().contains_key(&(0, 0)));
        assert!(!area.store.animations().contains_key(&stable_id));
        assert!(!area.store.spawning_cells().contains(&(0, 0)));
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, None);
        assert!(!cell.terrain_object_blocks);

        let changed = area.restore_into(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
        );
        assert_eq!(changed, vec![(0, 0)]);
        assert_eq!(
            sim.terrain_objects.objects()[&stable_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
    }

    #[test]
    fn gsi_04_04_runtime_terrain_occupation_uses_shared_zone_writer() {
        let rules = rules("TemperateOccupationBits=7\n");
        let terrain_type = rules
            .terrain_object_type_case_insensitive("TIBTRE01")
            .expect("terrain type");
        let mut interner = StringInterner::default();
        let type_ref = interner.intern("TIBTRE01");
        let mut terrain = TerrainObjectState::new(1, type_ref, (0, 0), terrain_type, false, None);
        let mut grid = resolved_clear_grid();

        mark_terrain_occupation(&terrain, Some(&mut grid));
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, Some(7));
        assert!(cell.terrain_object_blocks);
        assert_eq!(cell.zone_type, zone_class::WALL);

        unmark_terrain_occupation(&terrain, Some(&mut grid));
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, None);
        assert!(!cell.terrain_object_blocks);
        assert_eq!(cell.zone_type, zone_class::GROUND);
        assert_eq!(cell.land_type, LandType::Clear.as_index());

        terrain.occupation_bits = 4;
        mark_terrain_occupation(&terrain, Some(&mut grid));
        assert_eq!(grid.cell(0, 0).unwrap().zone_type, zone_class::BUILDING);
        unmark_terrain_occupation(&terrain, Some(&mut grid));

        terrain.occupation_bits = 0;
        mark_terrain_occupation(&terrain, Some(&mut grid));
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, Some(0));
        assert!(!cell.terrain_object_blocks);
        assert_eq!(cell.zone_type, zone_class::BUILDING);

        let cell = grid.cell_mut_for_test(0, 0).unwrap();
        cell.overlay_zone_type = Some(zone_class::GROUND);
        terrain.occupation_bits = 7;
        mark_terrain_occupation(&terrain, Some(&mut grid));
        assert_eq!(
            grid.cell(0, 0).unwrap().zone_type,
            zone_class::GROUND,
            "terminal rubble result outranks terrain occupation"
        );
    }

    #[test]
    fn gsi_04_10_non_wood_and_immune_damage_do_not_mutate_ordinary_tree() {
        for (wood, type_section) in [(false, ""), (true, "Immune=yes\n")] {
            let rules = terrain_rules("TREE01", wood, type_section);
            let mut sim = Simulation::new();
            seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
            let stable_id = sim.terrain_objects.cell_index()[&(0, 0)];
            let terrain = sim.terrain_objects.objects()[&stable_id].clone();
            let mut grid = resolved_clear_grid();
            mark_terrain_occupation(&terrain, Some(&mut grid));
            let before = sim.terrain_objects.objects()[&stable_id].clone();
            let warhead = rules.warhead("WH").expect("warhead");

            let result = damage_terrain_object_at_cell(
                &mut sim.terrain_objects,
                &mut sim.substrate.raw_cell_occupation,
                &rules,
                &sim.interner,
                (0, 0),
                5,
                warhead,
                Some(&mut grid),
            );

            assert_eq!(result, TerrainDamageResult::Ignored);
            assert_eq!(sim.terrain_objects.objects()[&stable_id], before);
            assert_eq!(sim.terrain_objects.cell_index()[&(0, 0)], stable_id);
            let cell = grid.cell(0, 0).unwrap();
            assert_eq!(cell.terrain_object_occupation, Some(7));
            assert!(cell.terrain_object_blocks);
        }
    }

    #[test]
    fn gsi_04_10_lethal_ordinary_tree_uninits_and_clears_spatial_authority_same_call() {
        let rules = terrain_rules("TREE01", true, "");
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.terrain_objects.cell_index()[&(0, 0)];
        let terrain = sim.terrain_objects.objects()[&stable_id].clone();
        let mut grid = resolved_clear_grid();
        mark_terrain_occupation(&terrain, Some(&mut grid));
        assert!(grid.cell(0, 0).unwrap().terrain_object_blocks);
        let warhead = rules.warhead("WH").expect("warhead");

        let result = damage_terrain_object_at_cell(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (0, 0),
            10,
            warhead,
            Some(&mut grid),
        );

        assert_eq!(result, TerrainDamageResult::Destroyed);
        assert!(!sim.terrain_objects.cell_index().contains_key(&(0, 0)));
        assert!(!sim.terrain_objects.animations().contains_key(&stable_id));
        assert!(!sim.terrain_objects.spawning_cells().contains(&(0, 0)));
        assert_eq!(
            sim.terrain_objects.objects()[&stable_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, None);
        assert!(!cell.terrain_object_blocks);
        assert!(!cell.ground_walk_blocked);
        assert_eq!(cell.zone_type, zone_class::GROUND);
    }

    #[test]
    fn gsi_04_12_terrain_raw_occupation_damage_preserves_until_lethal_limbo() {
        let rules = terrain_rules("TREE01", true, "TemperateOccupationBits=7\n");
        let mut sim = Simulation::new();
        sim.substrate.raw_cell_occupation.mark_ground(0, 0, 0xE0);
        sim.substrate.raw_cell_occupation.mark_deck(0, 0, 0x5A);
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.terrain_objects.cell_index()[&(0, 0)];
        let warhead = rules.warhead("WH").expect("warhead");

        let ignored = damage_terrain_object_at_cell(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (0, 0),
            0,
            warhead,
            None,
        );
        assert_eq!(ignored, TerrainDamageResult::Ignored);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0xFC);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(0, 0), 0x5A);

        let damaged = damage_terrain_object_at_cell(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (0, 0),
            4,
            warhead,
            None,
        );
        assert_eq!(damaged, TerrainDamageResult::Damaged { remaining: 6 });
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0xFC);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(0, 0), 0x5A);

        let destroyed = damage_terrain_object_at_cell(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (0, 0),
            6,
            warhead,
            None,
        );
        assert_eq!(destroyed, TerrainDamageResult::Destroyed);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0xA0);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(0, 0), 0x5A);
        assert_eq!(
            sim.terrain_objects.objects()[&stable_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
    }
}
