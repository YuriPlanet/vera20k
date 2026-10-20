//! Production system types, constants, and state containers.
//!
//! Shared types used across production sub-modules: queue items, build options,
//! placement previews, and the central `ProductionState` struct.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::rules::object_type::ObjectCategory;
use crate::sim::intern::InternedId;
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
    #[cfg(any(test, feature = "test-support"))]
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
    pub category: ProductionCategory,
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
    /// Whether a membership pass keeps this option through
    /// `production_tech::strip_keeps`. The app's retained strip owns actual
    /// membership between House rechecks. Building4456D0 supplies separate
    /// adding-factory admission. Money never darkens a cameo:
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
    // Building+3D3 belongs to each retained building. Manual selection448160
    // clears matching non-limbo peers, so a limbo peer can retain its flag
    // alongside the chosen on-map factory. This is the sole primary-state
    // owner; the House FactoryClass queue slots remain in `factories`.
    active_producer_by_owner: BTreeMap<InternedId, BTreeMap<ProductionCategory, BTreeSet<u64>>>,
    /// The monotonic temporal stamp the next accepted enqueue takes.
    next_enqueue_order: u64,
    /// Per-(house, category) factory registry — the authoritative production state
    /// machine AND (as of P5d) the queue-of-record: the active build is the `Factory` head
    /// fields, the FIFO tail is `Factory.queue` of `QueueEntry`. Mutated directly by
    /// enqueue/cancel/delivery (no `queues_by_owner` mirror); serialized + hashed. Its
    /// per-step charge runs against the real wallet via
    /// `step_all` at the Phase-7 head, before the house tail (C1).
    factories: FactoryRegistry,
}

impl ProductionState {
    /// The stamp the next enqueue takes; it is spent only once the enqueue
    /// is accepted ([`Self::spend_enqueue_order`]).
    pub fn next_enqueue_order(&self) -> u64 {
        self.next_enqueue_order
    }

    /// An accepted enqueue (or a computer factory's construction) spends the
    /// stamp it read.
    pub(crate) fn spend_enqueue_order(&mut self) {
        self.next_enqueue_order = self.next_enqueue_order.saturating_add(1);
    }

    pub fn factories(&self) -> &FactoryRegistry {
        &self.factories
    }

    /// The registry's own operations carry the factory rules.
    pub(crate) fn factories_mut(&mut self) -> &mut FactoryRegistry {
        &mut self.factories
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_next_enqueue_order_for_test(&mut self, value: u64) {
        self.next_enqueue_order = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn factories_mut_for_test(&mut self) -> &mut FactoryRegistry {
        &mut self.factories
    }

    // Native primary is per Factory RTTI/Naval, so both building sidebar tabs
    // use the same BuildingType primary. No second primary on the Defense tab.
    fn primary_category(category: ProductionCategory) -> ProductionCategory {
        if category == ProductionCategory::Defense {
            ProductionCategory::Building
        } else {
            category
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn primary_factory_ids(
        &self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> impl Iterator<Item = u64> + '_ {
        self.active_producer_by_owner
            .get(&owner)
            .and_then(|categories| categories.get(&Self::primary_category(category)))
            .into_iter()
            .flat_map(|ids| ids.iter().copied())
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
            .entry(Self::primary_category(category))
            .or_default()
            .insert(id);
    }
    /// Clear the native primary byte without choosing a replacement. Unlimbo
    /// and ChangeOwner write it; destruction removes its retained identity.
    /// Limbo preserves the byte, while FindFactory filters the limbo object.
    pub(crate) fn clear_primary_factory(&mut self, id: u64) {
        for categories in self.active_producer_by_owner.values_mut() {
            categories.retain(|_, ids| {
                ids.remove(&id);
                !ids.is_empty()
            });
        }
        self.active_producer_by_owner
            .retain(|_, categories| !categories.is_empty());
    }
    /// Whether this retained building has its native primary flag
    /// (`TechnoClass+0x3D3`). Limbo preserves that flag; production eligibility
    /// and display admission remain with their respective consumers.
    pub fn is_primary_factory(&self, id: u64) -> bool {
        self.active_producer_by_owner
            .values()
            .any(|categories| categories.values().any(|ids| ids.contains(&id)))
    }
    pub(crate) fn retain_primary_factory_links(&mut self, live: &std::collections::BTreeSet<u64>) {
        for categories in self.active_producer_by_owner.values_mut() {
            categories.retain(|_, ids| {
                ids.retain(|id| live.contains(id));
                !ids.is_empty()
            });
        }
        self.active_producer_by_owner
            .retain(|_, categories| !categories.is_empty());
    }
    /// Keep the established singleton hash while retaining every primary
    /// flag in the native limbo-peer case. Neither field storage nor the
    /// category index is exposed to the simulation hash consumer.
    pub(crate) fn hash_primary_factories(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        for (owner, categories) in &self.active_producer_by_owner {
            owner.hash(hasher);
            for (category, ids) in categories {
                category.hash(hasher);
                if ids.len() != 1 {
                    b"retained-primary-factory-set-v1".hash(hasher);
                    (ids.len() as u64).hash(hasher);
                }
                for id in ids {
                    id.hash(hasher);
                }
            }
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_primary_factory_for_test(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        id: u64,
    ) {
        // Existing fixtures install one chosen primary. Native histories
        // inside production tests use the owner's additive setter instead.
        if let Some(categories) = self.active_producer_by_owner.get_mut(&owner) {
            categories.remove(&Self::primary_category(category));
        }
        self.set_primary_factory(owner, category, id);
    }
}

impl Default for ProductionState {
    fn default() -> Self {
        Self {
            active_producer_by_owner: BTreeMap::new(),
            next_enqueue_order: 1,
            factories: FactoryRegistry::default(),
        }
    }
}
