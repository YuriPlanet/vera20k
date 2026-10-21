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
//! both when it chooses a harvester (`sim::ai_unit_choice`). Their building
//! arms, again with no DontScore test, count the buildings whose type is a
//! `WeaponsFactory=` (`+0x16BD`) and not `Naval=` (`+0xCCE`) in `+0x160`
//! (`0x00502B28..0x00502B42`, `0x005027D4..0x005027EE`); the computer's
//! economy check reads all three (`sim::ai_build_need`).
//!
//! Building arms also retain signed House+310 capacity, independently of
//! DontScore/Insignificant: ADD32 at502B78, SUB32 at502815. Constructor
//! 4F58DE starts0; this is not the separate four-slot resource amount total.
//! `spatial_oracle/building_storage_withdrawals.py` executes those transfer
//! slices; its consumed capacity controls do not prove the whole callbacks.
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
//! counts an infantry once through object `+0x438`; `+0x439` skips only that
//! total increment (`0x004FF824..0x004FF83B`), while per-type `+0x5528` is
//! always updated. Remove_Tracking tests and clears `+0x438`
//! (`0x004FF636..0x004FF64D`). Bio Reactor entry sets `+0x439`, removes an
//! existing `+0x2F4` contribution and clears `+0x438`; successful unload,
//! SpawnSurvivors and Selling reveals restore the contribution and clear
//! `+0x439` (`0x0051A30F`, `0x0051A327..0x0051A348`,
//! `0x0044DBEA..0x0044DC02`, `0x00442F6D..0x00442FA1`,
//! `0x0044A510..0x0044A548`). The tracked navy (`+0x2EC`) counts only Units
//! whose `Naval=` is set and whose signed `Passengers=` is exactly zero
//! (`0x004FF8C0`, `0x004FF6C8`). TEvent55 reads this scalar; TEvent56 reads
//! vehicles minus navy, then tracked infantry (`0x0071F159..0x0071F187`).
//! Limbo does not remove tracking, so transports and constructed limbo objects
//! must not be inferred from the on-map sets. `+0x2F8` is not kept.
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
//! An `Explodes=` type (every shipyard) or a building dying while Selling gets
//! a zero timer and leaves at its next Update's Limbo (`0x004400C5`).
//! Time_To_Build reads one counter (`HouseClass::GetFactoryCount @
//! 0x00500910`); the EVA funds nag sums four.
//!
//! Evidence: `tools/spatial_oracle/house_tracking.py` runs the original
//! Add_Tracking and Remove_Tracking (36 cases);
//! `tools/spatial_oracle/house_defeat_gate.py` runs the gate block with the
//! original counter readers (21 cases). `tag_lifecycle.py --naval-land-events`
//! replays the tracking payload on the Steam image and executes TEvent55/56;
//! its metadata distinguishes the old payload's provenance from this replay.
//! Whole Added_To_Game and Removed_From_Game are read, not executed; only
//! their capacity/amount transfer slices have executable controls here.
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
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;

/// The type facts the counters test, fixed when the Techno is constructed
/// ([`TrackingFacts::of`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackingFacts {
    /// `DontScore=`: the score bookkeeping and the house counts skip the
    /// object. Persisted, so a count removed after a reload skips it as its
    /// addition did.
    #[serde(default)]
    dont_score: bool,
    /// `ObjectType+0x232` Insignificant.
    insignificant: bool,
    /// A building tracked with the units: a 1x1 undeployer, or one that
    /// undeploys into a `ResourceGatherer=` (a deployed Slave Miner).
    unit_like_building: bool,
    /// Unit RTTI, `Naval=` nonzero and signed `Passengers=` exactly zero:
    /// the native Add/Remove_Tracking navy arm (`4FF8C0` / `4FF6C8`).
    #[serde(default)]
    naval_combatant: bool,
    /// `TechnoType+0x5EC`, `ResourceGatherer=` (ReadINI `0x007143DF`).
    resource_gatherer: bool,
    /// `TechnoType+0x5ED`, `ResourceDestination=` (ReadINI `0x007143FE`).
    #[serde(default)]
    resource_destination: bool,
    /// A building whose type is a `WeaponsFactory=` and not `Naval=`.
    #[serde(default)]
    war_factory: bool,
    /// The value arm of the on-map writers; none for a building.
    #[serde(default)]
    force_value: Option<ForceValueFacts>,
    /// A building's factory counter, from its type's `Factory=` and
    /// `Naval=`.
    #[serde(default)]
    factory: Option<FactorySlot>,
    /// Live BuildingType+800 (`Storage=`); other classes' cargo capacity
    /// does not join House+310. Production types are immutable for this object;
    /// deploy/undeploy constructs a replacement with its own type facts.
    #[serde(default)]
    storage_capacity: i32,
}

impl TrackingFacts {
    /// The facts of an object of class `category` and type `ty`. A building
    /// is tracked with the units (Add_Tracking's building arm
    /// `0x004FF761..0x004FF791`) when its vtable `+0x80` answers (a 1x1
    /// undeployer) or it undeploys (`+0x408`) into a `ResourceGatherer=`
    /// (`+0x5EC`).
    pub(crate) fn of(category: EntityCategory, ty: Option<&ObjectType>, rules: &RuleSet) -> Self {
        let Some(ty) = ty else {
            return Self::default();
        };
        let unit_like_building = category == EntityCategory::Structure
            && (ty.is_1x1_with_undeploy()
                || ty
                    .undeploys_into
                    .as_deref()
                    .and_then(|undeploys| rules.object(undeploys))
                    .is_some_and(|undeploys| undeploys.resource_gatherer));
        Self {
            dont_score: ty.dont_score,
            insignificant: ty.insignificant,
            unit_like_building,
            naval_combatant: category == EntityCategory::Unit && ty.naval && ty.passengers == 0,
            resource_gatherer: ty.resource_gatherer,
            resource_destination: ty.resource_destination,
            war_factory: category == EntityCategory::Structure && ty.weapons_factory && !ty.naval,
            force_value: ForceValueFacts::of(category, ty),
            factory: (category == EntityCategory::Structure)
                .then(|| {
                    FactoryType::for_production_rtti(ty.factory_rtti())
                        .map(|factory| FactorySlot::of(factory, ty.naval))
                })
                .flatten(),
            storage_capacity: if category == EntityCategory::Structure {
                ty.storage
            } else {
                0
            },
        }
    }

    /// The retained input affects a later Added/Removed even while in limbo.
    pub(crate) fn hash_storage_capacity(self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if self.storage_capacity != 0 {
            b"building-storage-capacity-input-v1".hash(hasher);
            self.storage_capacity.hash(hasher);
        }
    }

    /// `DontScore=`.
    pub(crate) const fn dont_score(self) -> bool {
        self.dont_score
    }

    /// The retained type admission used by both tracking writers and hashing.
    pub(crate) const fn naval_combatant(self) -> bool {
        self.naval_combatant
    }

    /// Install a fixture's `DontScore=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_dont_score_for_test(&mut self, value: bool) {
        self.dont_score = value;
    }

    /// An `Insignificant=` object's facts.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn insignificant_for_test() -> Self {
        Self {
            insignificant: true,
            ..Self::default()
        }
    }
}

/// The two object-owned latches that gate an Infantry's `HouseClass+0x2F4`
/// total. Add_Tracking sets `counted_infantry_total` only when both bytes are
/// clear; Remove_Tracking tests and clears it. InfantryAbsorb entry can clear
/// the count latch while the passenger remains in its per-type `+0x5528`
/// count; any successful reveal from an absorber, SpawnSurvivors or Selling
/// may restore it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct InfantryTrackingLatches {
    /// Techno `+0x438`: Add/Remove_Tracking's once-counted total latch.
    counted_infantry_total: bool,
    /// Techno `+0x439`: Add_Tracking skips the total arm while set.
    skip_infantry_total_on_add: bool,
}

impl InfantryTrackingLatches {
    /// Add_Tracking `0x004FF81A..0x004FF83B` increments the house total and
    /// sets `+0x438` only when both latches are clear.
    fn add_total(&mut self) -> bool {
        if self.counted_infantry_total || self.skip_infantry_total_on_add {
            return false;
        }
        self.counted_infantry_total = true;
        true
    }

    /// Remove_Tracking `0x004FF636..0x004FF64D` subtracts only when `+0x438`
    /// is set, then clears it.
    fn remove_total(&mut self) -> bool {
        let counted = self.counted_infantry_total;
        self.counted_infantry_total = false;
        counted
    }

    fn set_absorbed(&mut self, absorbed: bool) {
        self.skip_infantry_total_on_add = absorbed;
    }

    fn restore_absorbed(&mut self) -> bool {
        self.skip_infantry_total_on_add = false;
        if self.counted_infantry_total {
            false
        } else {
            self.counted_infantry_total = true;
            true
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn is_counted(&self) -> bool {
        self.counted_infantry_total
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn is_absorbed(&self) -> bool {
        self.skip_infantry_total_on_add
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
    /// `+0xEB8` in IncrementFactoryCount and Recount, where a Unit/UnitType
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
    /// `HouseClass+0x2EC`: tracked Naval Units without a passenger capacity.
    /// Zeroed with the other scalars at House construction (`4F58B5`).
    #[serde(default)]
    navy: i32,
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
    /// `HouseClass+0x160`, the on-map buildings whose type is a land
    /// `WeaponsFactory=`.
    #[serde(default)]
    war_factories: i32,
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
    /// House+310, zeroed by4F58DE. Retained event total, independent of
    /// filtered active-building counts and the separate raw amount stores.
    #[serde(default)]
    storage_capacity: i32,
}

impl HouseTracking {
    /// `HouseClass::Add_Tracking @ 0x004FF700`.
    pub(crate) fn add_tracking(&mut self, entity: &mut GameEntity) {
        self.track(entity, 1);
    }

    /// `HouseClass::Remove_Tracking @ 0x004FF550`.
    pub(crate) fn remove_tracking(&mut self, entity: &mut GameEntity) {
        self.track(entity, -1);
    }

    /// Shared body of `Add_Tracking @ 0x004FF700` (+1) and `Remove_Tracking @ 0x004FF550` (-1);
    /// Infantry latches `+0x438`/`+0x439`: `0x004FF81A..0x004FF83B`, `0x004FF636..0x004FF64D`.
    fn track(&mut self, entity: &mut GameEntity, delta: i32) {
        let facts = entity.tracking_facts;
        if facts.insignificant || entity.tracking_facts.dont_score() {
            return;
        }
        match entity.category() {
            EntityCategory::Structure if !facts.unit_like_building => {
                self.buildings = self.buildings.wrapping_add(delta);
            }
            EntityCategory::Structure | EntityCategory::Unit => {
                self.vehicles = self.vehicles.wrapping_add(delta);
                if facts.naval_combatant() {
                    // Steam gamemd: 4FF8C0 INC / 4FF6C8 DEC.
                    // tools/spatial_oracle/house_tracking.py (36 controls),
                    // replayed by tag_lifecycle.py --naval-land-events.
                    self.navy = self.navy.wrapping_add(delta);
                }
            }
            EntityCategory::Infantry => {
                let count_total = if delta > 0 {
                    entity.house_tracking_latches_mut().add_total()
                } else {
                    entity.house_tracking_latches_mut().remove_total()
                };
                if count_total {
                    self.infantry = self.infantry.wrapping_add(delta);
                }
            }
            EntityCategory::Aircraft => {}
        }
        let counts = match entity.category() {
            EntityCategory::Unit => &mut self.unit_types,
            EntityCategory::Structure => &mut self.building_types,
            EntityCategory::Infantry => &mut self.infantry_types,
            EntityCategory::Aircraft => &mut self.aircraft_types,
        };
        let count = counts.entry(entity.type_ref()).or_default();
        *count = count.wrapping_add(delta);
    }

    /// `InfantryClass::PerCellProcess @ 0x00519630`'s absorber entry
    /// (`0x0051A327..0x0051A348`): a set `+0x438` lowers the total and clears.
    fn absorber_entry(&mut self, entity: &mut GameEntity) {
        if entity.category() == EntityCategory::Infantry
            && entity.house_tracking_latches_mut().remove_total()
        {
            self.infantry = self.infantry.wrapping_sub(1);
        }
    }

    /// A reveal's restore (Mission_Unload `0x0044DBEA..0x0044DC02`, SpawnSurvivors
    /// `0x00442F6D..0x00442FA1`, Mission_Selling `0x0044A510..0x0044A548`).
    fn restore_absorbed_infantry(&mut self, entity: &mut GameEntity) {
        if entity.category() == EntityCategory::Infantry
            && entity.house_tracking_latches_mut().restore_absorbed()
        {
            self.infantry = self.infantry.wrapping_add(1);
        }
    }

    /// The tracked count of one type of `category` (`0x0049FAE0` on
    /// `+0x5500`, `+0x5514`, `+0x5528` or `+0x553C`).
    /// Whether any of `building_types` has a tracked instance: `+0x5500`
    /// through `0x0049FAE0`, signed `> 0` (`JG`), as the Harvest preamble
    /// (`0x0073E69B..0x0073E6A2`) and Guard's computer-harvester arm
    /// (`0x007408AF..0x007408B6`) test a harvester's `Dock=` list.
    pub fn owns_any_building(&self, building_types: impl IntoIterator<Item = InternedId>) -> bool {
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
        if entity.category() != EntityCategory::Structure {
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
        if self.navy != 0 {
            b"house-tracked-navy-v1".hash(hasher);
            self.navy.hash(hasher);
        }
        if self.storage_capacity != 0 {
            b"house-storage-capacity-v1".hash(hasher);
            self.storage_capacity.hash(hasher);
        }
    }

    /// Retained House+310, not AvailableStorage4F69D0 for nonempty stores.
    pub(crate) const fn storage_capacity(&self) -> i32 {
        self.storage_capacity
    }

    /// `HouseClass+0x2E8`.
    pub(crate) const fn vehicles(&self) -> i32 {
        self.vehicles
    }

    /// `HouseClass+0x2EC`, independent of the on-map and factory counters.
    pub(crate) const fn navy(&self) -> i32 {
        self.navy
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

    /// `HouseClass+0x160`.
    pub(crate) const fn war_factories(&self) -> i32 {
        self.war_factories
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
        if entity.tracking_facts.war_factory {
            self.war_factories = self.war_factories.wrapping_add(1);
        }
        // Added502B57..502B78: independent of DontScore/Insignificant.
        // Current Added's live power/COM prelude remains deferred to the
        // existing power owner; no callback occurs inside this counter phase.
        if entity.category() == EntityCategory::Structure {
            self.storage_capacity = self
                .storage_capacity
                .wrapping_add(entity.tracking_facts.storage_capacity);
        }
        if let Some(value) = entity.tracking_facts.force_value {
            let total = self.force_values.total(value.kind);
            *total = total.wrapping_add(factors.adjust(value.cost, value.factor_slot.into()));
        }
        let dont_score = entity.tracking_facts.dont_score();
        let counts = match entity.category() {
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

    /// Removed502606..50262A precedes the powered-center callback. Its
    /// receiving House is retained by the Simulation entry point.
    pub(crate) fn removed_from_game_prefix(&mut self, entity: &GameEntity) {
        if entity.tracking_facts.resource_gatherer {
            self.resource_gatherers = self.resource_gatherers.wrapping_sub(1);
        }
        if entity.tracking_facts.resource_destination {
            self.resource_destinations = self.resource_destinations.wrapping_sub(1);
        }
    }

    /// The class phase of Removed_From_Game50277B..502A55, after the
    /// powered-center prelude. Type facts are read at this phase, not retained
    /// across its callbacks. Actual Limbo/ChangeOwner callers pass flag0;
    /// nonempty stores and the distinct flag1 withdrawal remain unported.
    pub(crate) fn removed_from_game(&mut self, entity: &GameEntity, factors: &HouseCostFactors) {
        if entity.tracking_facts.war_factory {
            self.war_factories = self.war_factories.wrapping_sub(1);
        }
        //502801..502815 SUB32 of the live BuildingType.Storage. No clamp
        // or per-object contribution latch; DontScore does not skip it.
        if entity.category() == EntityCategory::Structure {
            self.storage_capacity = self
                .storage_capacity
                .wrapping_sub(entity.tracking_facts.storage_capacity);
        }
        if let Some(value) = entity.tracking_facts.force_value {
            let total = self.force_values.total(value.kind);
            *total = total.wrapping_sub(factors.adjust(value.cost, value.factor_slot.into()));
        }
        if entity.tracking_facts.dont_score() {
            return;
        }
        let counts = match entity.category() {
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
    pub fn buildings(&self) -> i32 {
        self.buildings
    }

    /// TEvent9 / DestroyedUnitsAll (`71F12F..71F187`, Steam
    /// gamemd) reads the two CounterClass totals independently.
    /// Aircraft and tracked/limbo units do not participate; a negative total
    /// is accepted rather than canceling a positive total of the other class.
    /// Source: tools/spatial_oracle/tag_lifecycle.py --destroyed-house-events.
    pub(crate) fn units_destroyed(&self) -> bool {
        self.active_units() <= 0 && self.active_infantry() <= 0
    }

    /// TEvent11 / DestroyedAll (`71F189..71F1B1`) additionally tests the
    /// tracked Building scalar +2F0. It remains positive after a building's
    /// Limbo until its actual destructor removes tracking.
    pub(crate) fn all_destroyed(&self) -> bool {
        self.buildings <= 0 && self.units_destroyed()
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

impl Simulation {
    /// After the absorber's Limbo, native `0x0051A327..0x0051A348` removes
    /// the once-counted infantry total and clears `+0x438`; per-type tracking
    /// remains intact while the passenger is hidden in the building.
    pub(crate) fn account_infantry_absorber_entry(
        substrate: &mut ObjectSubstrate,
        houses: &mut Houses,
        stable_id: u64,
    ) {
        let Some(owner) = substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Infantry)
            .map(GameEntity::owner)
        else {
            return;
        };
        let (Some(entity), Some(house)) = (
            substrate.entities.get_mut(stable_id),
            houses.get_mut(&owner),
        ) else {
            return;
        };
        house.tracking_mut().absorber_entry(entity);
    }

    /// Successful reveals from a Bio Reactor, SpawnSurvivors or Selling restore
    /// the absorbed Infantry's total once and clear its Add_Tracking skip byte.
    /// Native writers: Building Mission_Unload `0x0044DBEA..0x0044DC02`,
    /// SpawnSurvivors `0x00442F6D..0x00442FA1`, and Mission_Selling
    /// `0x0044A510..0x0044A548`. The per-type counter never changes here.
    pub(crate) fn restore_absorbed_infantry_tracking(
        substrate: &mut ObjectSubstrate,
        houses: &mut Houses,
        stable_id: u64,
    ) {
        let Some(owner) = substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Infantry)
            .map(GameEntity::owner)
        else {
            return;
        };
        let (Some(entity), Some(house)) = (
            substrate.entities.get_mut(stable_id),
            houses.get_mut(&owner),
        ) else {
            return;
        };
        house.tracking_mut().restore_absorbed_infantry(entity);
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// Native Infantry::PerCellProcess sets `+0x439` immediately before the
    /// absorber's Limbo (`0x0051A2ED..0x0051A316`). This only suppresses a
    /// subsequent Add_Tracking total increment; it does not affect `+0x5528`.
    pub(crate) fn set_infantry_absorber_entry(&mut self, stable_id: u64, absorbed: bool) {
        if let Some(entity) = self
            .entities
            .get_mut(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Infantry)
        {
            entity.house_tracking_latches_mut().set_absorbed(absorbed);
        }
    }
}

/// A CounterClass total: the wrapping sum of its per-type counts.
fn wrapping_total(counts: &BTreeMap<InternedId, i32>) -> i32 {
    counts
        .values()
        .fold(0, |total, &count| total.wrapping_add(count))
}

#[cfg(any(test, feature = "test-support"))]
impl HouseTracking {
    /// Stand in for tracked buildings a fixture places without the lifecycle.
    pub fn set_buildings_for_test(&mut self, buildings: i32) {
        self.buildings = buildings;
    }

    /// Explicit native TEvent55/56 scalar priors, without fabricating objects.
    pub(crate) fn set_naval_land_for_test(&mut self, vehicles: i32, navy: i32, infantry: i32) {
        self.vehicles = vehicles;
        self.navy = navy;
        self.infantry = infantry;
    }

    /// Stand in for the on-map gatherers, destinations and war factories
    /// (`+0x158`, `+0x15C`, `+0x160`) a fixture places without the lifecycle.
    pub(crate) fn set_economy_for_test(
        &mut self,
        gatherers: i32,
        destinations: i32,
        war_factories: i32,
    ) {
        self.resource_gatherers = gatherers;
        self.resource_destinations = destinations;
        self.war_factories = war_factories;
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

#[cfg(test)]
#[path = "house_storage_capacity_tests.rs"]
mod storage_capacity_tests;
