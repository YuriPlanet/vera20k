//! Production system types, constants, and state containers.
//!
//! Shared types used across production sub-modules: queue items, build options,
//! placement previews, and the central `ProductionState` struct.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::rules::object_type::ObjectCategory;
use crate::sim::intern::InternedId;
use crate::sim::ore_growth::{OreGrowthConfig, OreGrowthState};
use crate::sim::production::factory::FactoryRegistry;

/// A test house's opening balance.
#[cfg(any(test, feature = "test-support"))]
pub const STARTING_CREDITS: i32 = 5000;

/// One queued item formatted for UI rendering.
#[derive(Debug, Clone)]
pub struct QueueItemView {
    pub type_id: InternedId,
    pub display_name: String,
    pub queue_category: ProductionCategory,
    pub state: BuildQueueState,
    /// Factory progress in steps (`0..=PRODUCTION_STEPS`, Factory `+0x24`, read by
    /// the sidebar through `0x004CA120`); `0` for a queued item.
    pub progress: u16,
}

/// One completed building waiting for placement.
#[derive(Debug, Clone)]
pub struct ReadyBuildingView {
    pub type_id: InternedId,
    pub display_name: String,
    pub queue_category: ProductionCategory,
}

/// Active producer/facility focus for one queue category.
#[derive(Debug, Clone)]
pub struct ProducerFocusView {
    pub stable_id: u64,
    pub display_name: String,
    pub category: ProductionCategory,
    pub rx: u16,
    pub ry: u16,
}

/// Placement preview/evaluation for a ready building.
#[derive(Debug, Clone)]
pub struct BuildingPlacementPreview {
    pub rx: u16,
    pub ry: u16,
    pub width: u16,
    pub height: u16,
    pub valid: bool,
    /// Independent native Display+1180 and +1181 gates. LeftUp checks these
    /// before emitting PLACE; a red terrain cell is refused by queued Unlimbo.
    pub in_build_area: bool,
    pub foundation_visible: bool,
    pub reason: Option<BuildingPlacementError>,
    /// Current ground fields and admission in foundation row order. Rebuilt by
    /// the production preview owner each frame; never an independent terrain or
    /// placement authority. Empty when there is no installed terrain.
    pub cells: Vec<BuildingPlacementCell>,
    /// Sim-owned regular-wall filler cells in native N/E/S/W, nearest-first order.
    pub wall_autofill_cells: Vec<BuildingPlacementCell>,
}

/// One read-only current Cell snapshot used by the placement drawing consumer.
/// Cell47EC90 reads signed +11B and unsigned slope +11C independently of its
/// IsClearToBuild47C620 result. No renderer-local height/admission cache applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingPlacementCell {
    pub coord: (u16, u16),
    pub valid: bool,
    pub level: i8,
    pub slope: u8,
}

/// Why the player cannot build an item now
/// (`production_tech::build_option_for_owner`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildDisabledReason {
    /// No building of the house could build it (`FindFactory(1, 0, 0)`):
    /// rejected by the next strip membership pass.
    NoFactory,
    /// `HouseClass::CanBuild(type, 0, 1)` refuses it on a membership pass.
    CannotBuild,
    /// Unlinked cameo is darkened: no online factory may build it now
    /// (`FindFactory(1, 1, 1)`).
    NoReadyFactory,
    /// Unlinked cameo is darkened: at its build limit.
    AtBuildLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildingPlacementError {
    NotReady,
    NotBuilding,
    BlockedTerrain,
    OverlapsStructure,
    OutOfBuildArea,
}

/// Arguments consumed by the one HouseClass::Place_Production4FB0E0 owner.
/// Buildings carry the clicked type/cell; automatic mobile PLACE resolves the
/// current complete Factory head, including a head replaced before dispatch.
#[derive(Debug, Clone, Copy)]
pub enum ProductionPlacement<'a> {
    Building { type_id: &'a str, cell: (u16, u16) },
    Mobile { category: ProductionCategory },
}

impl BuildingPlacementError {
    pub fn label(&self) -> &'static str {
        match self {
            Self::NotReady => "Not ready for placement",
            Self::NotBuilding => "Not a building",
            Self::BlockedTerrain => "Blocked terrain",
            Self::OverlapsStructure => "Overlaps structure",
            Self::OutOfBuildArea => "Outside build radius",
        }
    }
}

/// Sidebar queue/category for build options.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum ProductionCategory {
    /// Default variant so the `Factory`/`FactoryRegistry` value-types can derive
    /// `Default`. Serde/hash-neutral: adds a `::default()` ctor, changes no value.
    #[default]
    Building,
    Defense,
    Infantry,
    Vehicle,
    Aircraft,
    /// HouseClass Primary_ForShips (+0x53B8), independent of the land-vehicle
    /// Primary_ForVehicles (+0x53B4) slot. Appended to preserve existing serde
    /// variant indices for saved production categories.
    Ship,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuildQueueState {
    Queued,
    Building,
    Paused,
    Done,
}

impl BuildQueueState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Building => "Building",
            Self::Paused => "Paused",
            Self::Done => "Done",
        }
    }
}

impl ProductionCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Building => "Building",
            Self::Defense => "Defense",
            Self::Infantry => "Infantry",
            Self::Vehicle => "Vehicle",
            Self::Aircraft => "Aircraft",
            Self::Ship => "Ship",
        }
    }
}

/// One build option exposed to UI.
#[derive(Debug, Clone)]
pub struct BuildOption {
    pub type_id: InternedId,
    pub display_name: String,
    /// The type's Cost_Of for the owner (TechnoType virtual `+0x84`).
    pub cost: i32,
    pub object_category: ObjectCategory,
    pub queue_category: ProductionCategory,
    pub enabled: bool,
    /// Select6AB656's independent CheckBuildLimit50B370 result. Unavailable
    /// painting (including NoFactory) does not itself reject a cameo press.
    pub at_build_limit: bool,
    pub reason: Option<BuildDisabledReason>,
}

impl BuildOption {
    /// Whether a membership pass admits/keeps this option through
    /// `production_tech::strip_keeps`. The app's retained strip owns actual
    /// membership between House rechecks. Money never darkens a cameo:
    /// a build the house cannot pay for starts and stalls its paid steps.
    pub fn visible_in_sidebar(&self) -> bool {
        !matches!(
            self.reason,
            Some(BuildDisabledReason::NoFactory | BuildDisabledReason::CannotBuild)
        )
    }
}

/// Player production state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionState {
    active_producer_by_owner: BTreeMap<InternedId, BTreeMap<ProductionCategory, u64>>,
    pub next_enqueue_order: u64,
    /// Ore growth/spread configuration resolved from merged INI sources.
    pub ore_growth_config: OreGrowthConfig,
    /// Per-TiberiumClass growth and spread queues, bitmaps and timers.
    pub ore_growth_state: OreGrowthState,
    /// Retained animations for all IsAnimated terrain, keyed by map cell.
    /// Derived from live `terrain_objects`; removal/limbo must remove this index.
    pub terrain_animations: BTreeMap<(u16, u16), crate::sim::terrain_spawn::TerrainAnimationState>,
    /// Live `TerrainClass`-style map objects, keyed by deterministic stable id.
    pub terrain_objects: BTreeMap<u64, crate::sim::terrain_object::TerrainObjectState>,
    /// Live terrain object cell index, cell -> stable id.
    pub terrain_object_cells: BTreeMap<(u16, u16), u64>,
    /// Cells occupied by terrain objects whose type has `SpawnsTiberium=yes`.
    ///
    /// This has a different gate from `terrain_animations`: a non-animated
    /// spawner rejects new ore, while an animated non-spawner still draws RNG.
    pub tiberium_spawning_terrain_cells: BTreeSet<(u16, u16)>,
    /// Per-(house, category) factory registry — the authoritative production state
    /// machine AND (as of P5d) the queue-of-record: the active build is the `Factory` head
    /// fields, the FIFO tail is `Factory.queue` of `QueueEntry`. Mutated directly by
    /// enqueue/cancel/delivery (no `queues_by_owner` mirror); serialized + hashed. Its
    /// per-step charge runs against the real wallet via
    /// `step_all` at the Phase-7 head, before the house tail (C1).
    pub factories: FactoryRegistry,
}

impl ProductionState {
    // Native primary is per Factory RTTI/Naval, so both building sidebar tabs
    // use the same BuildingType primary. No second primary on the Defense tab.
    fn primary_category(category: ProductionCategory) -> ProductionCategory {
        if category == ProductionCategory::Defense {
            ProductionCategory::Building
        } else {
            category
        }
    }
    pub(crate) fn primary_factory(
        &self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> Option<u64> {
        self.active_producer_by_owner
            .get(&owner)?
            .get(&Self::primary_category(category))
            .copied()
    }
    pub(super) fn set_primary_factory(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        id: u64,
    ) {
        self.active_producer_by_owner
            .entry(owner)
            .or_default()
            .insert(Self::primary_category(category), id);
    }
    /// Clear the native primary byte without choosing a replacement. Unlimbo
    /// and ChangeOwner write it; destruction removes its retained identity.
    /// Limbo preserves the byte, while FindFactory filters the limbo object.
    pub(crate) fn clear_primary_factory(&mut self, id: u64) {
        for categories in self.active_producer_by_owner.values_mut() {
            categories.retain(|_, value| *value != id);
        }
        self.active_producer_by_owner
            .retain(|_, categories| !categories.is_empty());
    }
    /// The building's own primary byte (`TechnoClass+0x3D3`): it is its
    /// house's primary factory of some category.
    pub(crate) fn is_primary_factory(&self, id: u64) -> bool {
        self.active_producer_by_owner
            .values()
            .any(|categories| categories.values().any(|&primary| primary == id))
    }
    pub(crate) fn retain_primary_factory_links(&mut self, live: &std::collections::BTreeSet<u64>) {
        for categories in self.active_producer_by_owner.values_mut() {
            categories.retain(|_, id| live.contains(id));
        }
        self.active_producer_by_owner
            .retain(|_, categories| !categories.is_empty());
    }
    pub(crate) fn primary_factory_entries(
        &self,
    ) -> impl Iterator<Item = (&InternedId, &BTreeMap<ProductionCategory, u64>)> {
        self.active_producer_by_owner.iter()
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_primary_factory_for_test(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        id: u64,
    ) {
        self.set_primary_factory(owner, category, id);
    }
}

impl Default for ProductionState {
    fn default() -> Self {
        Self {
            active_producer_by_owner: BTreeMap::new(),
            next_enqueue_order: 1,
            ore_growth_config: OreGrowthConfig::disabled(),
            ore_growth_state: OreGrowthState::new(0, 0),
            terrain_animations: BTreeMap::new(),
            terrain_objects: BTreeMap::new(),
            terrain_object_cells: BTreeMap::new(),
            tiberium_spawning_terrain_cells: BTreeSet::new(),
            factories: FactoryRegistry::default(),
        }
    }
}
