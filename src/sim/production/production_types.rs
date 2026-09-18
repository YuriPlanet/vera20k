//! Production system types, constants, and state containers.
//!
//! Shared types used across production sub-modules: queue items, build options,
//! placement previews, and the central `ProductionState` struct.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::rules::object_type::ObjectCategory;
use crate::sim::intern::InternedId;
use crate::sim::miner::ResourceNode;
use crate::sim::miner::miner_dock::RefineryDockContacts;
use crate::sim::ore_growth::{OreGrowthConfig, OreGrowthState};
use crate::sim::production::factory::FactoryRegistry;

/// Initial credits for the local player.
pub const STARTING_CREDITS: i32 = 5000;
/// Fixed-point precision for dynamic production-rate application.
pub(super) const PRODUCTION_RATE_SCALE: u64 = 1_000_000;

/// One queued item formatted for UI rendering.
#[derive(Debug, Clone)]
pub struct QueueItemView {
    pub type_id: InternedId,
    pub display_name: String,
    pub queue_category: ProductionCategory,
    pub state: BuildQueueState,
    pub remaining_ms: u32,
    pub total_ms: u32,
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
    pub type_id: InternedId,
    pub rx: u16,
    pub ry: u16,
    pub width: u16,
    pub height: u16,
    pub valid: bool,
    pub reason: Option<BuildingPlacementError>,
    /// Per-cell validity (row-major, width*height). True = cell is placeable.
    pub cell_valid: Vec<bool>,
    /// Sim-owned regular-wall filler cells in native N/E/S/W, nearest-first order.
    pub wall_autofill_cells: Vec<(u16, u16)>,
}

/// Why an item cannot currently be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildDisabledReason {
    UnbuildableTechLevel,
    WrongOwner,
    WrongHouse,
    ForbiddenHouse,
    RequiresStolenTech,
    MissingPrerequisite(String),
    NoFactory,
    AtBuildLimit,
    InsufficientCredits,
    PlacementModeUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildingPlacementError {
    NotReady,
    NotBuilding,
    BlockedTerrain,
    OverlapsStructure,
    OutOfBuildArea,
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
    NoFunds,
    Paused,
    Done,
}

impl BuildQueueState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Building => "Building",
            Self::NoFunds => "On Hold",
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
    pub cost: i32,
    pub object_category: ObjectCategory,
    pub queue_category: ProductionCategory,
    pub enabled: bool,
    pub reason: Option<BuildDisabledReason>,
}

impl BuildOption {
    /// Whether the sidebar should show a cameo for this option.
    ///
    /// Tech-tree, faction, and factory failures hide the item entirely — the
    /// player never sees a cameo they cannot act on. A credit shortfall or a
    /// reached build limit keeps the cameo visible (greyed): the item is still
    /// part of the player's tech tree, it just can't start right now.
    pub fn visible_in_sidebar(&self) -> bool {
        self.enabled
            || matches!(
                self.reason,
                Some(BuildDisabledReason::InsufficientCredits)
                    | Some(BuildDisabledReason::AtBuildLimit)
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuildMode {
    Strict,
    PrototypeRelaxed,
}

/// Player production state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionState {
    pub ready_by_owner: BTreeMap<InternedId, VecDeque<InternedId>>,
    pub active_producer_by_owner: BTreeMap<InternedId, BTreeMap<ProductionCategory, u64>>,
    pub next_enqueue_order: u64,
    /// Legacy test/save compatibility only. Live YR maps derive resource type
    /// and raw quantity from `Simulation::overlay_grid`; this map is neither
    /// seeded nor read/hashed when the production registries are available.
    pub resource_nodes: BTreeMap<(u16, u16), ResourceNode>,
    /// Refinery dock reservation state — one dock per refinery, FIFO queue.
    pub dock_reservations: RefineryDockContacts,
    /// Ore growth/spread configuration resolved from merged INI sources.
    pub ore_growth_config: OreGrowthConfig,
    /// Incremental scan state for ore growth/spread system.
    pub ore_growth_state: OreGrowthState,
    /// Slave Miner bindings: master entity stable_id → vec of slave entity stable_ids.
    /// Used to track which SLAV infantry belong to which deployed SMIN/YAREFN.
    pub slave_bindings: BTreeMap<u64, Vec<u64>>,
    /// TIBTRE-style ore-spawning terrain objects, keyed by map cell.
    /// Derived from live `terrain_objects`; removal/limbo must remove this index.
    pub terrain_spawners: BTreeMap<(u16, u16), crate::sim::terrain_spawn::TerrainSpawnerState>,
    /// Live `TerrainClass`-style map objects, keyed by deterministic stable id.
    pub terrain_objects: BTreeMap<u64, crate::sim::terrain_object::TerrainObjectState>,
    /// Live terrain object cell index, cell -> stable id.
    pub terrain_object_cells: BTreeMap<(u16, u16), u64>,
    /// Terrain occupation mask by cell, mirroring CellClass+0x124 bits 0x04/0x08/0x10.
    pub terrain_occupation_bits: BTreeMap<(u16, u16), u8>,
    /// Cells occupied by terrain objects whose type has `SpawnsTiberium=yes`.
    ///
    /// This is broader than `terrain_spawners`: non-animated legacy spawners do
    /// not tick, but still reject new Tiberium placement in the native gate.
    pub tiberium_spawning_terrain_cells: BTreeSet<(u16, u16)>,
    /// Fallback overlay_id used for new ore cells when no overlay registry is
    /// available. Runtime placement prefers the data-driven `TIB01..TIB12`
    /// registry set and uses this only for headless/fallback contexts.
    pub default_ore_overlay_id: Option<u8>,
    /// Airfield dock reservations — multi-slot (NumberOfDocks per airfield).
    pub airfield_docks: crate::sim::docking::aircraft_dock::AirfieldDocks,
    /// Per-(house, category) factory registry — the authoritative production state
    /// machine AND (as of P5d) the queue-of-record: the active build is the `Factory` head
    /// fields, the FIFO tail is `Factory.queue` of `QueueEntry`. Mutated directly by
    /// enqueue/cancel/delivery (no `queues_by_owner` mirror); serialized + hashed. Its
    /// per-step charge runs against the real wallet via
    /// `step_all` at the Phase-7 head, before the house tail (C1).
    pub factory_shadow: FactoryRegistry,
}

impl Default for ProductionState {
    fn default() -> Self {
        Self {
            ready_by_owner: BTreeMap::new(),
            active_producer_by_owner: BTreeMap::new(),
            next_enqueue_order: 1,
            resource_nodes: BTreeMap::new(),
            dock_reservations: RefineryDockContacts::default(),
            ore_growth_config: OreGrowthConfig::disabled(),
            ore_growth_state: OreGrowthState::new(0, 0),
            slave_bindings: BTreeMap::new(),
            terrain_spawners: BTreeMap::new(),
            terrain_objects: BTreeMap::new(),
            terrain_object_cells: BTreeMap::new(),
            terrain_occupation_bits: BTreeMap::new(),
            tiberium_spawning_terrain_cells: BTreeSet::new(),
            default_ore_overlay_id: None,
            airfield_docks: crate::sim::docking::aircraft_dock::AirfieldDocks::default(),
            factory_shadow: FactoryRegistry::default(),
        }
    }
}
