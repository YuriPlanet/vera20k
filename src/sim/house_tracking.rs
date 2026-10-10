//! A house's object counts, as its multiplayer defeat gate, the computer's AI
//! triggers, production times and EVA read them.
//!
//! Native owner: `HouseClass`. Independent sets of counters, each with its own
//! writers:
//! - Tracking (`HouseClass::Add_Tracking @ 0x004FF700`, `Remove_Tracking @
//!   0x004FF550`): added when the Techno is constructed (the class
//!   constructors and InitFromType, `0x007355EA`, `0x00517CD4`, `0x00414068`,
//!   `0x00442C62`), removed by its destructor at the pending-delete drain,
//!   moved by `TechnoClass::ChangeOwner` (`0x007015DE`, `0x007015E6`).
//!   `Insignificant=` and `DontScore=` types are never tracked. A building
//!   counts in `+0x2F0` unless it is a 1x1 undeployer (vtable `+0x80`,
//!   `0x00465D40`) or undeploys into a `ResourceGatherer=` (then `+0x2E8`,
//!   with the units). Every class also counts per type: buildings in
//!   `+0x5500`, units in `+0x5514`, infantry in `+0x5528`, aircraft in
//!   `+0x553C` (`0x004FF7AA`, `0x004FF880`, `0x004FF842`, `0x004FF7E8`). The
//!   short game's defeat gate reads the units'; the computer's CanBuild reads
//!   each class's for its build limit (`sim::production::can_build`); a
//!   harvester's `Dock=` tests read the buildings' ([`HouseTracking::owns_any_building`],
//!   and the dock search's `0x004DEE9B`).
//! - On the map (`HouseClass::Added_To_Game @ 0x00502A80` from
//!   `TechnoClass::Unlimbo` `0x006F6D8F`, `Removed_From_Game @ 0x005025F0`
//!   from `TechnoClass::Limbo` `0x006F6BD1`, both from ChangeOwner
//!   `0x0070159D`/`0x0070178E`): per-type counters (`+0x5564` units,
//!   `+0x5578` infantry, `+0x558C` aircraft, `+0x5550` buildings). The
//!   normal game's defeat gate reads the first three's totals and one building
//!   type; the AI trigger conditions read one type of any
//!   (`sim::ai_team_creation`). `DontScore=` skips them, except that a
//!   Unit is added without the test (`0x00502CF9`) but removed with it, and
//!   an infantry survivor flagged `+0x6D9` is not added (`0x00502C3C`) but is
//!   removed.
//!
//! Added_To_Game and Removed_From_Game also count, before their per-class
//! arms and with no DontScore test, the house's objects whose type is a
//! `ResourceGatherer=` (`+0x158`, `0x00502A95..0x00502A9F` and
//! `0x00502606..0x00502610`) and those whose type is a
//! `ResourceDestination=` (`+0x15C`, `0x00502AAF..0x00502AB9` and
//! `0x00502620..0x0050262A`). The computer reads the first when it decides
//! whether a lost refinery node may be rebuilt (`sim::ai_base_building`) and
//! both when it chooses a harvester (`sim::ai_unit_choice`).
//!
//! Their Aircraft, Infantry and Unit arms first add, and subtract, the
//! object's value, its type's `Cost_Of` (`vt+0x84`, `0x00711F00`) for this
//! house at that moment, again with no DontScore test: infantry to `+0x160A8`
//! unless `ConsideredAircraft=` (`+0xD96`), a unit to `+0x160AC` unless
//! `ConsideredAircraft=` or `Spawns=` (`+0xD58`), everything else of the
//! three to the air total `+0x160B0` (`0x00502B90..0x00502CE1`,
//! `0x005028D0..0x00502A15`); buildings have no value arm. The price follows
//! the house's FactoryPlants, so an object that leaves after they changed
//! takes out a different amount than it put in, and the totals drift as
//! natively. An enemy computer reads them as the house's forces
//! (`sim::ai_base_defense`).
//!
//! The gate (`HouseClass::Update @ 0x004F8E86..0x004F8F82`) reads only these:
//! a short game keeps a house alive while `+0x2F0 > 0` or its tracked
//! `BaseUnit=` types sum above zero; a normal game while `+0x2F0` plus the
//! on-map unit, infantry and aircraft totals and the on-map count of
//! `[AI] BuildRefinery=`'s third type is not zero. `+0x2E8` (the vehicles,
//! plus the buildings tracked with them, `0x004FF72E`/`0x004FF78B`) and
//! `+0x2F4` (the infantry, `0x004FF81A..0x004FF83B`) are read by the crate
//! pickup's Unit and Squad eligibility (`0x00481C27`, `0x00481C3B`). Native
//! counts an infantry once through the `+0x438` latch, which Remove_Tracking
//! (`0x004FF636..0x004FF64D`) tests and clears, and skips one whose `+0x439`
//! byte is set; VERA pairs the add and remove per object and does not model
//! `+0x439` (identity unestablished). `+0x2EC`, `+0x2F8` and the owned-type
//! sets have no reader here and are not kept.
//!
//! Factory counters (`+0x5378` aircraft, `+0x537C` infantry, `+0x5380`
//! vehicle, `+0x5384` building, `+0x5388` naval): a building whose type has
//! `Factory=` joins the counter its `Factory=` and `Naval=` choose
//! (`HouseClass::IncrementFactoryCount @ 0x004FFA50`) when BuildingClass::
//! Unlimbo places it (`0x00440D13`) and ChangeOwner gives it to a house
//! (`0x00448CDD`), and leaves it (`HouseClass::Recount @ 0x004FF980`, a plain
//! decrement) at its first Limbo (`0x00445D8E`) and when ChangeOwner takes it
//! from a house (`0x0044870E`). An ordinary building death limbos it at once:
//! `BuildingClass::ReceiveDamage` UnInits it in its death arm (`0x0044269A`)
//! while the death timer DestructionEffects armed (`+0x528`, 8 frames) runs.
//! Time_To_Build reads one counter (`HouseClass::GetFactoryCount @
//! 0x00500910`); the EVA funds nag sums four.
//!
//! Evidence: `tools/spatial_oracle/house_tracking.py` runs the original
//! Add_Tracking and Remove_Tracking (36 cases);
//! `tools/spatial_oracle/house_defeat_gate.py` runs the gate block with the
//! original counter readers (21 cases). Added_To_Game and Removed_From_Game
//! are read, not executed.
//!
//! RESIDUAL: the survivor flag `+0x6D9` is not written. SpawnSurvivors sets it
//! (`0x00443111..0x00443127`), before the survivor's Unlimbo, on a `Nominal=`
//! survivor (the Technician) of a building with Buildup art (Init_Managers sets
//! `+0x6E9` when the type has a Buildup shape, `0x00442CAA..0x00442CCF`); the
//! sale crew sets it after a successful Unlimbo (`0x0044A733..0x0044A747`;
//! VERA has no sale crew). Nothing clears it. Added_To_Game skips a flagged
//! infantry on every Unlimbo while Removed_From_Game still decrements it on
//! every Limbo, so each time a flagged Technician leaves the map (dies,
//! garrisons, boards) native `+0x5578` drops by one for good. Trigger: a
//! Technician survivor of an armed building (the 15% crew roll) leaving the
//! map in a normal (non-short) game. Effect: native's sum can reach zero while
//! objects stand (the house is defeated and they are blown up) or stay
//! negative when nothing is left (the house is never defeated); VERA does
//! neither.
//!
//! RESIDUAL: a factory whose type is `Explodes=` (TechnoType `+0xD15`) or
//! that dies while selling leaves its counter one frame late natively.
//! DestructionEffects arms its death timer at 0 (`0x00441C43..0x00441C8C`),
//! so ReceiveDamage skips the UnInit, and the next Update limbos it
//! (`0x004400C5`) before its UnInit; VERA UnInits every building at once
//! (`crate::sim::crew_survival` records the same split). Trigger: every
//! shipyard death (GAYARD, NAYARD and YAYARD are `Explodes=yes`) and any
//! factory killed while sold. Effect: a build started or re-rated in that
//! frame counts one factory more natively (one more MultipleFactory step).
//!
//! RESIDUAL: while the game-active byte `[0x00A8E9A0]` is clear, Limbo skips
//! everything after `0x00445AC6`, Recount and TechnoClass::Limbo included;
//! VERA has no such byte. Main__PrepareSession sets it (`0x0052D9D7`) before
//! the scenario starts, and the game-exit paths and the command-queue
//! writers clear it when a game ends. Trigger: a building limboed after the
//! game ended. Effect: none VERA can show; its simulation stops there.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{FactoryType, ObjectType};
use crate::rules::ruleset::{HouseCostFactors, RuleSet};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;

/// The type facts the counters test, fixed when the Techno is constructed
/// ([`TrackingFacts::of`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackingFacts {
    /// `ObjectType+0x232` Insignificant.
    insignificant: bool,
    /// A building tracked with the units: a 1x1 undeployer, or one that
    /// undeploys into a `ResourceGatherer=` (a deployed Slave Miner).
    unit_like_building: bool,
    /// `TechnoType+0x5EC`, `ResourceGatherer=` (ReadINI `0x007143DF`).
    resource_gatherer: bool,
    /// `TechnoType+0x5ED`, `ResourceDestination=` (ReadINI `0x007143FE`).
    #[serde(default)]
    resource_destination: bool,
    /// The value arm of the on-map writers; none for a building.
    #[serde(default)]
    force_value: Option<ForceValueFacts>,
    /// A building's factory counter, from its type's `Factory=` and
    /// `Naval=`.
    #[serde(default)]
    factory: Option<FactorySlot>,
}

impl TrackingFacts {
    /// The facts of an object of class `category` and type `ty`. A building
    /// is tracked with the units (Add_Tracking's building arm
    /// `0x004FF761..0x004FF791`) when its vtable `+0x80` answers (a 1x1
    /// undeployer) or it undeploys (`+0x408`) into a `ResourceGatherer=`
    /// (`+0x5EC`).
    pub(crate) fn of(
        category: EntityCategory,
        ty: Option<&ObjectType>,
        rules: Option<&RuleSet>,
    ) -> Self {
        let Some(ty) = ty else {
            return Self::default();
        };
        let unit_like_building = category == EntityCategory::Structure
            && (ty.is_1x1_with_undeploy()
                || ty
                    .undeploys_into
                    .as_deref()
                    .and_then(|undeploys| rules.and_then(|rules| rules.object(undeploys)))
                    .is_some_and(|undeploys| undeploys.resource_gatherer));
        Self {
            insignificant: ty.insignificant,
            unit_like_building,
            resource_gatherer: ty.resource_gatherer,
            resource_destination: ty.resource_destination,
            force_value: ForceValueFacts::of(category, ty),
            factory: (category == EntityCategory::Structure)
                .then(|| ty.factory.map(|factory| FactorySlot::of(factory, ty.naval)))
                .flatten(),
        }
    }

    /// An `Insignificant=` object's facts.
    #[cfg(test)]
    pub(crate) fn insignificant_for_test() -> Self {
        Self {
            insignificant: true,
            ..Self::default()
        }
    }
}

/// One of a house's factory counters, in their order from `+0x5378`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FactorySlot {
    Aircraft,
    Infantry,
    Vehicle,
    Building,
    Naval,
}

impl FactorySlot {
    /// The counter of a building with `Factory=factory`: the switch on Type
    /// `+0xEB8` in IncrementFactoryCount and Recount, where a `UnitType`
    /// factory whose type is `Naval=` (`+0xCCE`) is a shipyard.
    pub(crate) const fn of(factory: FactoryType, naval: bool) -> Self {
        match factory {
            FactoryType::AircraftType => Self::Aircraft,
            FactoryType::InfantryType => Self::Infantry,
            FactoryType::UnitType if naval => Self::Naval,
            FactoryType::UnitType => Self::Vehicle,
            FactoryType::BuildingType => Self::Building,
        }
    }
}

/// The house total an object's value joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ForceKind {
    /// `HouseClass+0x160A8`.
    Infantry,
    /// `HouseClass+0x160AC`.
    Vehicles,
    /// `HouseClass+0x160B0`.
    Air,
}

/// What the on-map writers price: the total and the type's `Cost_Of` inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ForceValueFacts {
    kind: ForceKind,
    /// `TechnoType+0x610`, the `Cost=` that `Cost_Of` scales (read through
    /// virtual `+0xAC`, `0x00711EB0`).
    cost: i32,
    /// [`ObjectType::factor_slot`].
    factor_slot: u8,
}

impl ForceValueFacts {
    /// The value arm an object of class `category` and type `ty` takes (see
    /// the module doc).
    fn of(category: EntityCategory, ty: &ObjectType) -> Option<Self> {
        let kind = match category {
            EntityCategory::Aircraft => ForceKind::Air,
            EntityCategory::Infantry if ty.considered_aircraft => ForceKind::Air,
            EntityCategory::Infantry => ForceKind::Infantry,
            EntityCategory::Unit if ty.considered_aircraft || ty.spawns.is_some() => ForceKind::Air,
            EntityCategory::Unit => ForceKind::Vehicles,
            EntityCategory::Structure => return None,
        };
        Some(Self {
            kind,
            cost: ty.cost,
            factor_slot: ty.factor_slot() as u8,
        })
    }
}

/// `HouseClass+0x160A8`, `+0x160AC` and `+0x160B0`, which the constructor
/// zeroes (`0x004F5DB4..0x004F5DC0`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ForceValues {
    pub infantry: i32,
    pub vehicles: i32,
    pub air: i32,
}

impl ForceValues {
    fn total(&mut self, kind: ForceKind) -> &mut i32 {
        match kind {
            ForceKind::Infantry => &mut self.infantry,
            ForceKind::Vehicles => &mut self.vehicles,
            ForceKind::Air => &mut self.air,
        }
    }
}

/// The counters the defeat gate reads, and the on-map gatherer count (see
/// the module doc).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HouseTracking {
    /// `HouseClass+0x2F0`.
    buildings: i32,
    /// `HouseClass+0x2E8`: the tracked vehicles and the buildings tracked
    /// with them. Derived from the hashed object set; not folded separately.
    #[serde(default)]
    vehicles: i32,
    /// `HouseClass+0x2F4`: the tracked infantry.
    #[serde(default)]
    infantry: i32,
    /// `HouseClass+0x5514`, the tracked count of each UnitType.
    unit_types: BTreeMap<InternedId, i32>,
    /// `HouseClass+0x5500`, the tracked count of each BuildingType.
    #[serde(default)]
    building_types: BTreeMap<InternedId, i32>,
    /// `HouseClass+0x5528`, the tracked count of each InfantryType.
    #[serde(default)]
    infantry_types: BTreeMap<InternedId, i32>,
    /// `HouseClass+0x553C`, the tracked count of each AircraftType.
    #[serde(default)]
    aircraft_types: BTreeMap<InternedId, i32>,
    /// `HouseClass+0x5564`, the on-map count of each UnitType; its total is
    /// the sum.
    active_unit_types: BTreeMap<InternedId, i32>,
    /// `HouseClass+0x5578`, the on-map count of each InfantryType.
    active_infantry_types: BTreeMap<InternedId, i32>,
    /// `HouseClass+0x558C`, the on-map count of each AircraftType.
    active_aircraft_types: BTreeMap<InternedId, i32>,
    /// `HouseClass+0x5550`, the on-map count of each BuildingType.
    active_building_types: BTreeMap<InternedId, i32>,
    /// `HouseClass+0x158`, the on-map objects whose type is a
    /// `ResourceGatherer=`.
    #[serde(default)]
    resource_gatherers: i32,
    /// `HouseClass+0x15C`, the on-map objects whose type is a
    /// `ResourceDestination=`.
    #[serde(default)]
    resource_destinations: i32,
    /// The value totals of the house's forces on the map.
    #[serde(default)]
    force_values: ForceValues,
    /// `HouseClass+0x2D4`, the docks of the house's `Helipad=` buildings
    /// ([`HouseTracking::airport_docks`]).
    #[serde(default)]
    airport_docks: i32,
    /// `HouseClass+0x5378..+0x5388`, in [`FactorySlot`] order.
    #[serde(default)]
    factories: [i32; 5],
}

impl HouseTracking {
    /// `HouseClass::Add_Tracking @ 0x004FF700`.
    pub(crate) fn add_tracking(&mut self, entity: &GameEntity) {
        self.track(entity, 1);
    }

    /// `HouseClass::Remove_Tracking @ 0x004FF550`.
    pub(crate) fn remove_tracking(&mut self, entity: &GameEntity) {
        self.track(entity, -1);
    }

    fn track(&mut self, entity: &GameEntity, delta: i32) {
        let facts = entity.tracking_facts;
        if facts.insignificant || entity.dont_score {
            return;
        }
        match entity.category {
            EntityCategory::Structure if !facts.unit_like_building => {
                self.buildings = self.buildings.wrapping_add(delta);
            }
            EntityCategory::Structure | EntityCategory::Unit => {
                self.vehicles = self.vehicles.wrapping_add(delta);
            }
            EntityCategory::Infantry => self.infantry = self.infantry.wrapping_add(delta),
            EntityCategory::Aircraft => {}
        }
        let counts = match entity.category {
            EntityCategory::Unit => &mut self.unit_types,
            EntityCategory::Structure => &mut self.building_types,
            EntityCategory::Infantry => &mut self.infantry_types,
            EntityCategory::Aircraft => &mut self.aircraft_types,
        };
        let count = counts.entry(entity.type_ref()).or_default();
        *count = count.wrapping_add(delta);
    }

    /// The tracked count of one type of `category` (`0x0049FAE0` on
    /// `+0x5500`, `+0x5514`, `+0x5528` or `+0x553C`).
    /// Whether any of `building_types` has a tracked instance: `+0x5500`
    /// through `0x0049FAE0`, signed `> 0` (`JG`), as the Harvest preamble
    /// (`0x0073E69B..0x0073E6A2`) and Guard's computer-harvester arm
    /// (`0x007408AF..0x007408B6`) test a harvester's `Dock=` list.
    pub(crate) fn owns_any_building(
        &self,
        building_types: impl IntoIterator<Item = InternedId>,
    ) -> bool {
        building_types
            .into_iter()
            .any(|type_id| self.owned_count(EntityCategory::Structure, type_id) > 0)
    }

    /// `HouseClass+0x2D4`: how many `PadAircraft=` aircraft the house may
    /// have on the map and in production (`HouseClass::CheckBuildLimit`,
    /// `0x0050B5E1`). Its writers are a `Helipad=` building's first
    /// Grand_Opening (`+= NumberOfDocks`, `0x004463C0..0x004463E0`), its
    /// Limbo once it has opened (`-=`, clamped at zero,
    /// `0x00445946..0x00445988`) and its ChangeOwner (`-=` on the old house,
    /// unclamped, `0x00448B4C..0x00448B6A`; `+=` on the new one,
    /// `0x00449229..0x00449245`), none of which test the house.
    pub(crate) const fn airport_docks(&self) -> i32 {
        self.airport_docks
    }

    /// Add `docks` to [`Self::airport_docks`]; negative for the old house at
    /// a capture.
    pub(crate) const fn add_airport_docks(&mut self, docks: i32) {
        self.airport_docks = self.airport_docks.wrapping_add(docks);
    }

    /// BuildingClass::Limbo's share of [`Self::airport_docks`]: subtract
    /// `docks`, then raise a negative count to zero.
    pub(crate) fn limbo_airport_docks(&mut self, docks: i32) {
        self.airport_docks = self.airport_docks.wrapping_sub(docks).max(0);
    }

    /// `HouseClass::IncrementFactoryCount @ 0x004FFA50`: a building with
    /// `Factory=` joins its counter.
    pub(crate) fn increment_factory_count(&mut self, entity: &GameEntity) {
        self.count_factory(entity, 1);
    }

    /// `HouseClass::Recount @ 0x004FF980`: a building with `Factory=` leaves
    /// its counter.
    pub(crate) fn recount(&mut self, entity: &GameEntity) {
        self.count_factory(entity, -1);
    }

    /// Both test What_Am_I for a building first (`0x004FF98B`,
    /// `0x004FFA5B`).
    fn count_factory(&mut self, entity: &GameEntity, delta: i32) {
        if entity.category != EntityCategory::Structure {
            return;
        }
        if let Some(slot) = entity.tracking_facts.factory {
            let count = &mut self.factories[slot as usize];
            *count = count.wrapping_add(delta);
        }
    }

    /// `HouseClass::GetFactoryCount @ 0x00500910`: one factory counter.
    pub(crate) const fn factory_count(&self, slot: FactorySlot) -> i32 {
        self.factories[slot as usize]
    }

    /// The EVA funds nag's factory sum (`0x004F8B74..0x004F8B92`): infantry,
    /// vehicle, building and naval, without aircraft.
    pub(crate) const fn funds_nag_factories(&self) -> i32 {
        let [_, infantry, vehicle, building, naval] = self.factories;
        naval
            .wrapping_add(building)
            .wrapping_add(vehicle)
            .wrapping_add(infantry)
    }

    /// The factory counters, for the world hash.
    pub(crate) const fn factories(&self) -> [i32; 5] {
        self.factories
    }

    pub(crate) fn owned_count(&self, category: EntityCategory, type_id: InternedId) -> i32 {
        let counts = match category {
            EntityCategory::Unit => &self.unit_types,
            EntityCategory::Structure => &self.building_types,
            EntityCategory::Infantry => &self.infantry_types,
            EntityCategory::Aircraft => &self.aircraft_types,
        };
        counts.get(&type_id).copied().unwrap_or(0)
    }

    /// Fold the counters schema v238 (`AiTeams`) added, tagged, once any is
    /// set: the tracked counts of the building, infantry and aircraft types,
    /// the on-map counts of the unit, infantry and aircraft types (the AI
    /// trigger conditions read them per type; the defeat counters fold only
    /// their totals) and the on-map count of `ResourceDestination=` objects.
    pub(crate) fn hash_ai_team_counters(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if self.building_types.is_empty()
            && self.infantry_types.is_empty()
            && self.aircraft_types.is_empty()
            && self.active_unit_types.is_empty()
            && self.active_infantry_types.is_empty()
            && self.active_aircraft_types.is_empty()
            && self.resource_destinations == 0
        {
            return;
        }
        b"house-type-counts-v1".hash(hasher);
        self.building_types.hash(hasher);
        self.infantry_types.hash(hasher);
        self.aircraft_types.hash(hasher);
        self.active_unit_types.hash(hasher);
        self.active_infantry_types.hash(hasher);
        self.active_aircraft_types.hash(hasher);
        self.resource_destinations.hash(hasher);
    }

    /// Fold the defeat counters as the derived hash of this struct did before
    /// the gatherer count joined it (schema `HouseDefeatTracking`).
    pub(crate) fn hash_defeat_counters(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.buildings.hash(hasher);
        self.unit_types.hash(hasher);
        // The totals, which the per-type counts replaced (schema v238 keeps
        // their bytes).
        self.active_units().hash(hasher);
        self.active_infantry().hash(hasher);
        self.active_aircraft().hash(hasher);
        self.active_building_types.hash(hasher);
    }

    /// `HouseClass+0x2E8`.
    pub(crate) const fn vehicles(&self) -> i32 {
        self.vehicles
    }

    /// `HouseClass+0x2F4`.
    pub(crate) const fn infantry(&self) -> i32 {
        self.infantry
    }

    /// `HouseClass+0x158`.
    pub(crate) const fn resource_gatherers(&self) -> i32 {
        self.resource_gatherers
    }

    /// `HouseClass+0x15C`.
    pub(crate) const fn resource_destinations(&self) -> i32 {
        self.resource_destinations
    }

    /// `HouseClass+0x160A8`, `+0x160AC` and `+0x160B0`.
    pub(crate) const fn force_values(&self) -> ForceValues {
        self.force_values
    }

    /// The on-map count of one type of `category` (`0x0049FAE0` on
    /// `+0x5564`, `+0x5578`, `+0x558C` or `+0x5550`).
    pub(crate) fn active_count(&self, category: EntityCategory, type_id: InternedId) -> i32 {
        let counts = match category {
            EntityCategory::Unit => &self.active_unit_types,
            EntityCategory::Infantry => &self.active_infantry_types,
            EntityCategory::Aircraft => &self.active_aircraft_types,
            EntityCategory::Structure => &self.active_building_types,
        };
        counts.get(&type_id).copied().unwrap_or(0)
    }

    fn active_units(&self) -> i32 {
        wrapping_total(&self.active_unit_types)
    }

    fn active_infantry(&self) -> i32 {
        wrapping_total(&self.active_infantry_types)
    }

    fn active_aircraft(&self) -> i32 {
        wrapping_total(&self.active_aircraft_types)
    }

    /// `HouseClass::Added_To_Game @ 0x00502A80`, pricing with this house's
    /// `factors`.
    pub(crate) fn added_to_game(&mut self, entity: &GameEntity, factors: &HouseCostFactors) {
        if entity.tracking_facts.resource_gatherer {
            self.resource_gatherers = self.resource_gatherers.wrapping_add(1);
        }
        if entity.tracking_facts.resource_destination {
            self.resource_destinations = self.resource_destinations.wrapping_add(1);
        }
        if let Some(value) = entity.tracking_facts.force_value {
            let total = self.force_values.total(value.kind);
            *total = total.wrapping_add(factors.adjust(value.cost, value.factor_slot.into()));
        }
        let dont_score = entity.dont_score;
        let counts = match entity.category {
            // The Unit case (increment at `0x00502CF9`) has no DontScore test.
            EntityCategory::Unit => &mut self.active_unit_types,
            EntityCategory::Aircraft if !dont_score => &mut self.active_aircraft_types,
            EntityCategory::Structure if !dont_score => &mut self.active_building_types,
            EntityCategory::Infantry if !dont_score => &mut self.active_infantry_types,
            _ => return,
        };
        let count = counts.entry(entity.type_ref()).or_default();
        *count = count.wrapping_add(1);
    }

    /// `HouseClass::Removed_From_Game @ 0x005025F0`, pricing with this
    /// house's `factors`.
    pub(crate) fn removed_from_game(&mut self, entity: &GameEntity, factors: &HouseCostFactors) {
        if entity.tracking_facts.resource_gatherer {
            self.resource_gatherers = self.resource_gatherers.wrapping_sub(1);
        }
        if entity.tracking_facts.resource_destination {
            self.resource_destinations = self.resource_destinations.wrapping_sub(1);
        }
        if let Some(value) = entity.tracking_facts.force_value {
            let total = self.force_values.total(value.kind);
            *total = total.wrapping_sub(factors.adjust(value.cost, value.factor_slot.into()));
        }
        if entity.dont_score {
            return;
        }
        let counts = match entity.category {
            EntityCategory::Unit => &mut self.active_unit_types,
            EntityCategory::Aircraft => &mut self.active_aircraft_types,
            EntityCategory::Structure => &mut self.active_building_types,
            EntityCategory::Infantry => &mut self.active_infantry_types,
        };
        let count = counts.entry(entity.type_ref()).or_default();
        *count = count.wrapping_sub(1);
    }

    /// The short game's test (`0x004F8EC6..0x004F8F1D`): alive while
    /// `+0x2F0 > 0` or the tracked counts of `BaseUnit=` entries 1, 2 and 0
    /// sum above zero.
    pub(crate) fn short_game_alive(&self, base_units: &[Option<InternedId>; 3]) -> bool {
        let tracked = |slot: usize| {
            base_units[slot].map_or(0, |unit| self.unit_types.get(&unit).copied().unwrap_or(0))
        };
        let base = tracked(1).wrapping_add(tracked(2)).wrapping_add(tracked(0));
        self.buildings > 0 || base > 0
    }

    /// `+0x2F0`, the tracked buildings.
    pub(crate) fn buildings(&self) -> i32 {
        self.buildings
    }

    /// The normal game's test (`0x004F8F21..0x004F8F77`): alive while
    /// `+0x2F0` plus the on-map totals and the on-map count of `[AI]
    /// BuildRefinery=`'s third type is not zero.
    pub(crate) fn normal_game_alive(&self, build_refinery_2: Option<InternedId>) -> bool {
        let refinery = build_refinery_2.map_or(0, |refinery| {
            self.active_building_types
                .get(&refinery)
                .copied()
                .unwrap_or(0)
        });
        self.buildings
            .wrapping_add(self.active_units())
            .wrapping_add(self.active_infantry())
            .wrapping_add(self.active_aircraft())
            .wrapping_add(refinery)
            != 0
    }
}

/// A CounterClass total: the wrapping sum of its per-type counts.
fn wrapping_total(counts: &BTreeMap<InternedId, i32>) -> i32 {
    counts
        .values()
        .fold(0, |total, &count| total.wrapping_add(count))
}

#[cfg(test)]
impl HouseTracking {
    /// Stand in for tracked buildings a fixture places without the lifecycle.
    pub(crate) fn set_buildings_for_test(&mut self, buildings: i32) {
        self.buildings = buildings;
    }

    /// The tracked Units of every type.
    pub(crate) fn units_for_test(&self) -> i32 {
        self.unit_types.values().sum()
    }

    /// Stand in for on-map units a fixture places without the lifecycle,
    /// counted under no type.
    pub(crate) fn set_active_units_for_test(&mut self, units: i32) {
        self.active_unit_types = BTreeMap::from([(InternedId::default(), units)]);
    }

    /// The on-map unit, infantry and aircraft totals.
    pub(crate) fn active_for_test(&self) -> (i32, i32, i32) {
        (
            self.active_units(),
            self.active_infantry(),
            self.active_aircraft(),
        )
    }

    /// Set one type's tracked count (`+0x5500..`).
    pub(crate) fn set_owned_for_test(
        &mut self,
        category: EntityCategory,
        type_id: InternedId,
        count: i32,
    ) {
        let counts = match category {
            EntityCategory::Unit => &mut self.unit_types,
            EntityCategory::Structure => &mut self.building_types,
            EntityCategory::Infantry => &mut self.infantry_types,
            EntityCategory::Aircraft => &mut self.aircraft_types,
        };
        counts.insert(type_id, count);
    }

    /// Set one type's on-map count (`+0x5550..`).
    pub(crate) fn set_active_for_test(
        &mut self,
        category: EntityCategory,
        type_id: InternedId,
        count: i32,
    ) {
        let counts = match category {
            EntityCategory::Unit => &mut self.active_unit_types,
            EntityCategory::Infantry => &mut self.active_infantry_types,
            EntityCategory::Aircraft => &mut self.active_aircraft_types,
            EntityCategory::Structure => &mut self.active_building_types,
        };
        counts.insert(type_id, count);
    }

    /// Set every counter the gate reads.
    pub(crate) fn set_for_test(
        &mut self,
        buildings: i32,
        unit_types: &[(InternedId, i32)],
        active: (i32, i32, i32),
        active_building_types: &[(InternedId, i32)],
    ) {
        self.buildings = buildings;
        self.unit_types = unit_types.iter().copied().collect();
        // The fixture's totals, counted under no type.
        let untyped = |total: i32| BTreeMap::from([(InternedId::default(), total)]);
        self.active_unit_types = untyped(active.0);
        self.active_infantry_types = untyped(active.1);
        self.active_aircraft_types = untyped(active.2);
        self.active_building_types = active_building_types.iter().copied().collect();
    }
}

#[cfg(test)]
#[path = "house_tracking_tests.rs"]
mod tests;
