//! What a house may build: `HouseClass::CanBuild @ 0x004F7870`,
//! `ObjectTypeClass::FindFactory @ 0x005F7900` (vtable `+0x94` of every
//! TechnoType class) and `HouseClass::CheckBuildLimit @ 0x0050B370`.
//!
//! The computer's unit choosers (`sim::ai_unit_choice`) ask CanBuild with
//! neither flag; FindFactory asks it with both, which skips to the build
//! limit. The chooser dispatch (`sim::ai_base_building`) and the AI trigger
//! factory check (`sim::ai_team_creation`) ask FindFactory. The player's
//! sidebar, its PRODUCE event and the per-frame production check ask all
//! three (`production_tech`).
//!
//! Evidence: instruction reading of all three bodies;
//! `tools/can_build_oracle.py` runs the original CanBuild and
//! CheckBuildLimit (replayed by `can_build_tests`), and
//! `tools/ai_team_oracle.py` runs the choosers with CanBuild's answers
//! hooked.
//!
//! RESIDUALS:
//! - The house's secret-lab types (`HouseClass+0x114`, each building's
//!   `0x00459840`) are not kept, so a type a captured Secret Lab grants goes
//!   through the ordinary gates instead of straight to the build limit.
//!   Trigger: a computer house owning a Tech Secret Lab whose TaskForces
//!   need its type. Effect: the type is refused where its TechLevel or
//!   `RequiredHouses=` would refuse it. Frequency: rare.
//! - Not a difference: `RequiredHouses=` also admits the countries in the
//!   house's masks (`+0x2C4..+0x2D0`, `0x004F7A07..0x004F7A51`), but only the
//!   House constructor writes them (zero; operand census of each offset,
//!   2026-10-12), so it admits the house's own country only, as VERA does.
//! - A build limit below 1 counts what the house has produced of the type
//!   (`+0x55A0`, `+0x55B4`, `+0x55C8`, `+0x55DC`), which is not kept: VERA
//!   counts none. Dormant: retail `rulesmd.ini` sets `BuildLimit=1` on twelve
//!   types and nothing lower, and the nine mode INIs set none.
//! - An infantry type with `VehicleThief=` also counts the house's units
//!   that hold one of it (`UnitClass+0x338`); VERA has no hijacking, so the
//!   count adds none.
//! - Dormant in retail data: a prerequisite BuildingType with
//!   `PowersUpBuilding=` (`+0xE88`) is met only when the house's last
//!   building that is out of limbo, powered and not selling holds it as an
//!   upgrade (`0x004F7DF8..0x004F7E4E`). VERA reads no upgrades and treats it
//!   as any other BuildingType; no retail type sets the key.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{FactoryType, ObjectCategory, ObjectType};
use crate::rules::prerequisite::{Prerequisite, PrerequisiteGroup};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::house_tracking::HouseTracking;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;

/// `HouseClass::CanBuild`'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanBuild {
    /// 0.
    No,
    /// 1.
    Yes,
    /// -1: the house is at the type's build limit.
    AtLimit,
}

/// `HouseClass::CanBuild @ 0x004F7870` for house `owner` and type `obj`.
///
/// With `skip_tech` (the first flag), only the build limit is asked
/// ([`build_limit`]). Otherwise, in order (`0x004F7889..0x004F7BE5`):
/// - `TechnoType+0xC98` refuses; only the constructor writes it, with 0
///   (`0x007113CC`);
/// - a `PrerequisiteOverride=` BuildingType the house has on the map
///   (`+0x5550`) skips to the build limit;
/// - `TechLevel=-1` refuses;
/// - `RequiresStolenThirdTech=`, `RequiresStolenSovietTech=` or
///   `RequiresStolenAlliedTech=` (`+0xD9B`, `+0xD9C`, `+0xD9D`) refuses
///   unless the house stole that tech (module residual);
/// - `RequiredHouses=` (`+0xDA0`) without the house's bit refuses;
/// - `ForbiddenHouses=` (`+0xDA4`) with it refuses;
/// - with super weapons off (`0xA8B263`), a BuildingType whose
///   `SuperWeapon=` is `DisableableFromShell=` refuses unless `[General]
///   BuildTech=` lists it;
/// - a TechLevel above the house's (`+0x1D4`, signed) refuses;
/// - a house no human controls is answered Yes: neither its prerequisites
///   nor its build limit are asked;
/// - a human house needs every `Prerequisite=` entry on the map
///   ([`prerequisite_on_map`], `0x004F7BF1..0x004F7F52`), then the build
///   limit.
///
/// Nothing here reads `Owner=`: [`find_factory`] matches it against the
/// factory building's, so a captured factory of another side builds that
/// side's types.
pub(crate) fn can_build(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
    skip_tech: bool,
    count_in_production: bool,
) -> CanBuild {
    if skip_tech {
        return build_limit(sim, owner, obj, count_in_production);
    }
    let Some(house) = sim.houses.get(&owner) else {
        return CanBuild::No;
    };
    // `CounterClass::GetItemCount @ 0x0049FAE0` at a group's negative code
    // reads before the counter array: the residual in
    // `crate::rules::prerequisite`.
    let override_on_map = obj.prerequisite_override.iter().any(|entry| match *entry {
        Prerequisite::Building(index) => buildings_on_map(sim, rules, house, index) > 0,
        Prerequisite::Group(_) => false,
    });
    if override_on_map {
        return build_limit(sim, owner, obj, count_in_production);
    }
    // `0x004F7976..0x004F79E2`: each `RequiresStolen*Tech=` needs the
    // house's stolen-tech byte, which a spy's BuildTech infiltration sets.
    if obj.tech_level == -1 || !house.has_stolen_tech_for(obj) {
        return CanBuild::No;
    }
    let house_bit = crate::sim::ai_buildable::house_country_bit(
        rules,
        sim.interner.resolve(house.house_type_id()),
    );
    let house_mask = |tokens: &[String]| crate::sim::ai_buildable::house_token_mask(tokens, rules);
    if !obj.required_houses.is_empty() && house_mask(&obj.required_houses) & house_bit == 0 {
        return CanBuild::No;
    }
    if !obj.forbidden_houses.is_empty() && house_mask(&obj.forbidden_houses) & house_bit != 0 {
        return CanBuild::No;
    }
    if !sim.session.game_options.super_weapons
        && obj.category == ObjectCategory::Building
        && let Some(super_weapon) = obj.super_weapon.as_deref()
        && !rules.is_build_tech(&obj.id)
        && rules
            .super_weapon(super_weapon)
            .is_some_and(|super_weapon| super_weapon.disableable_from_shell)
    {
        return CanBuild::No;
    }
    if obj.tech_level > house.tech_level {
        return CanBuild::No;
    }
    if !house.is_controlled_by_human(sim.session.game_mode_nonzero) {
        return CanBuild::Yes;
    }
    if !obj
        .prerequisite
        .iter()
        .all(|&entry| prerequisite_on_map(sim, rules, house, entry))
    {
        return CanBuild::No;
    }
    build_limit(sim, owner, obj, count_in_production)
}

/// One `Prerequisite=` entry of CanBuild's human arm
/// (`0x004F7C01..0x004F7E5E`). A group is met by any member of its
/// `[General] Prerequisite*` list on the map (`HouseClass+0x5550` above
/// zero), never by an empty list; `PROC` also by an on-map
/// `PrerequisiteProcAlternate=` unit (`+0x5564`, `0x004F7DB0..0x004F7DCE`), a
/// Slave Miner in retail. A BuildingType is met while its on-map count is not
/// zero (`0x004F7E50..0x004F7E5E`).
fn prerequisite_on_map(
    sim: &Simulation,
    rules: &RuleSet,
    house: &HouseState,
    entry: Prerequisite,
) -> bool {
    let group = match entry {
        Prerequisite::Building(index) => return buildings_on_map(sim, rules, house, index) != 0,
        Prerequisite::Group(group) => group,
    };
    rules
        .prerequisite_list(group)
        .iter()
        .any(|member| match *member {
            Prerequisite::Building(index) => buildings_on_map(sim, rules, house, index) > 0,
            // A group's code reads before the counter array: the residual in
            // `crate::rules::prerequisite`.
            Prerequisite::Group(_) => false,
        })
        || (group == PrerequisiteGroup::Proc
            && rules
                .general
                .prerequisite_proc_alternate
                .as_deref()
                .and_then(|unit| rules.object_in_category(ObjectCategory::Vehicle, unit))
                .and_then(|unit| sim.interner.get(&unit.id))
                .is_some_and(|unit| house.tracking.active_count(EntityCategory::Unit, unit) > 0))
}

/// The house's on-map count of BuildingType `index` (`HouseClass+0x5550`).
fn buildings_on_map(sim: &Simulation, rules: &RuleSet, house: &HouseState, index: i32) -> i32 {
    rules
        .building_type_at(index)
        .and_then(|building| sim.interner.get(&building.id))
        .map_or(0, |type_id| {
            house
                .tracking
                .active_count(EntityCategory::Structure, type_id)
        })
}

/// CanBuild's build limit (`0x004F7F9E..0x004F8357`): a type at or over
/// its positive `BuildLimit=` by the house's tracked count
/// (`HouseTracking::owned_count`) is at its limit, unless
/// `count_in_production` and one of the house's factories holds one
/// (`FactoryClass::Get_Object @ 0x004CA160`), which is Yes. For infantry the
/// factory scan runs only when the count equals the limit exactly.
fn build_limit(
    sim: &Simulation,
    owner: InternedId,
    obj: &ObjectType,
    count_in_production: bool,
) -> CanBuild {
    let limit = obj.build_limit;
    // `limit < 1` compares `|limit|` with the produced count, which VERA
    // does not keep (module residual): a count of zero refuses a zero limit
    // (and `i32::MIN`, whose magnitude stays negative), and passes any other.
    // Only aircraft, buildings and units answer Yes at once.
    if limit < 1 {
        if limit.wrapping_abs() <= 0 {
            return CanBuild::No;
        }
        if obj.category != ObjectCategory::Infantry {
            return CanBuild::Yes;
        }
    }
    let Some(type_id) = sim.interner.get(&obj.id) else {
        return CanBuild::Yes;
    };
    let owned = sim.houses.get(&owner).map_or(0, |house| {
        house
            .tracking
            .owned_count(EntityCategory::from(obj.category), type_id)
    });
    if limit < 1 || owned < limit {
        return CanBuild::Yes;
    }
    if !count_in_production || (obj.category == ObjectCategory::Infantry && owned != limit) {
        return CanBuild::AtLimit;
    }
    let in_production =
        sim.production
            .factories
            .iter_insertion_ordered()
            .into_iter()
            .any(|factory| {
                factory.owner == owner
                    && factory.object.as_ref().is_some_and(|object| {
                        object.type_id == type_id && object.entity_id.is_some()
                    })
            });
    if in_production {
        CanBuild::Yes
    } else {
        CanBuild::AtLimit
    }
}

/// `HouseClass::CheckBuildLimit @ 0x0050B370`: whether `owner` is at its
/// limit for one more `obj`, counting what its factory for the type's strip
/// holds (`FactoryRegistry::count_total`; both structure strips count the
/// Buildings factory, `0x0050B3B9`).
///
/// An `AirportBound=` aircraft is limited by the house's AirportDocks
/// ([`HouseTracking::airport_docks`]) alone: the `[General] PadAircraft=`
/// aircraft on the map (`+0x558C`) and in that factory must stay below it
/// (`0x0050B570..0x0050B5EB`); its `BuildLimit=` is not read. Any other
/// type with a `BuildLimit=` below 1 is at its limit once the limit's
/// magnitude is reached by what the house produced (module residual: VERA
/// counts none) and what the factory holds; with a positive one, once the
/// tracked count (`owned_count`) and the factory's reach it. An infantry
/// `VehicleThief=` would add the units that hold one (module residual).
pub(crate) fn check_build_limit(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> bool {
    let strip = match obj.category {
        ObjectCategory::Building => super::ProductionCategory::Building,
        _ => super::production_tech::production_category_for_object(obj),
    };
    let tracking = sim.houses.get(&owner).map(|house| &house.tracking);
    let in_factory = |type_id: Option<InternedId>| {
        type_id.map_or(0, |type_id| {
            sim.production.factories.count_total(owner, strip, type_id)
        })
    };
    if obj.category == ObjectCategory::Aircraft && obj.airport_bound {
        let pads = rules
            .general
            .pad_aircraft_types
            .iter()
            .filter_map(|name| rules.object_in_category(ObjectCategory::Aircraft, name))
            .map(|pad| {
                let type_id = sim.interner.get(&pad.id);
                let on_map = type_id.zip(tracking).map_or(0, |(type_id, tracking)| {
                    tracking.active_count(EntityCategory::Aircraft, type_id)
                });
                on_map.wrapping_add(in_factory(type_id))
            })
            .fold(0, i32::wrapping_add);
        return pads >= tracking.map_or(0, HouseTracking::airport_docks);
    }
    let limit = obj.build_limit;
    let type_id = sim.interner.get(&obj.id);
    let queued = in_factory(type_id);
    if limit < 1 {
        return limit.wrapping_abs() <= queued;
    }
    let owned = type_id.zip(tracking).map_or(0, |(type_id, tracking)| {
        tracking.owned_count(EntityCategory::from(obj.category), type_id)
    });
    owned.wrapping_add(queued) >= limit
}

/// ObjectClass::FindFactory5F5C20 resolves this object's actual type and owner,
/// then calls TypeFindFactory5F7900 without the CanBuild gate. Both the ready
/// cameo6AB31A and the PLACE receiver4FB1DA use (0,0); mobile delivery uses
/// (0,1), with an aircraft-only (1,1) retry. Source: cameo_order.md's retained
/// ready-building histories and the existing factory output oracle.
pub fn object_find_factory(
    sim: &Simulation,
    rules: &RuleSet,
    object_id: u64,
    skip_busy_airfield: bool,
    require_online: bool,
) -> Option<u64> {
    let object = sim.entities().get(object_id)?;
    let object_type = sim.object_type(object.type_ref(), rules)?;
    find_factory(
        sim,
        rules,
        object.owner(),
        object_type,
        skip_busy_airfield,
        require_online,
        false,
    )
}

/// Original ObjectType5F7900, scanned in House+6C insertion order. A first
/// eligible primary wins; without one, return the last eligible building.
/// Human PLACE4FB1DA retains this transient identity through its radio/release
/// path. Existing null-test callers consume this same result.
pub(crate) fn find_factory(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
    skip_busy_airfield: bool,
    require_online: bool,
    require_can_build: bool,
) -> Option<u64> {
    let house = sim.houses.get(&owner)?;
    let factory_type = FactoryType::for_category(obj.category);
    let game_mode_nonzero = sim.session.game_mode_nonzero;
    let ownable = get_ownable(obj, rules, game_mode_nonzero);
    let selling = MissionId::from_known(MissionType::Selling);
    let naval_unit = obj.category == ObjectCategory::Vehicle && obj.naval;
    let primary_category = super::production_tech::production_category_for_object(obj);
    let primary = sim.production.primary_factory(owner, primary_category);
    let mut candidate = None;
    for &id in house.base_projection.buildings() {
        let Some(building) = sim.substrate.entities.get(id) else {
            continue;
        };
        let Some(building_type) = sim.object_type(building.type_ref(), rules) else {
            continue;
        };
        if !building.lifecycle.in_limbo()
            && building_type.factory_type() == Some(factory_type)
            && (!require_online || building.building_online())
            && building.mission.effective() != selling
            && building.mission.queued() != selling
            && (!require_can_build
                || can_build(sim, rules, building.owner(), obj, true, true) == CanBuild::Yes)
            && ownable & get_ownable(building_type, rules, game_mode_nonzero) != 0
            && if !skip_busy_airfield
                && obj.category == ObjectCategory::Aircraft
                && !building.radio_contacts.is_empty()
            {
                // 0x005F79C7..0x005F79FB: unless arg1 skips it, an
                // AircraftType (`What_Am_I` 3) needs a free contact slot in a
                // factory In_Radio_Contact (0x0065AE30, 0x0065ADC0).
                building.radio_contacts.first_free().is_some()
            } else {
                // 0x005F7A09..0x005F7A49: a naval UnitType needs a naval
                // factory, anything else a non-naval one.
                building_type.naval == naval_unit
            }
        {
            candidate = Some(id);
            if primary == Some(id) {
                break;
            }
        }
    }
    candidate
}

/// Building448070, called from admitted Unlimbo4411B1 and ChangeOwner448C69:
/// preserve an existing live non-limbo primary of matching Factory/Naval;
/// otherwise make this building primary. This is the lifecycle producer, not
/// the fallback choice returned by FindFactory.
///
/// Residual #1709: 44807C/4480D5 and 4481C5 compare every nonzero raw
/// RTTI and Naval, including distinct Unit1/UnitType40 groups. The existing
/// serialized ProductionCategory index represents ordinary retail queues
/// only; custom raw/Naval groups need a separate lifecycle/state migration.
/// See https://github.com/YuriPlanet/vera20k/issues/1709.
pub(crate) fn initialize_factory_primary(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let Some(building) = sim.substrate.entities.get(id) else {
        return;
    };
    if building.category() != EntityCategory::Structure || building.lifecycle.in_limbo() {
        return;
    }
    let Some(ty) = sim.object_type(building.type_ref(), rules) else {
        return;
    };
    let Some(factory) = ty.factory_type() else {
        return;
    };
    let category = match factory {
        FactoryType::InfantryType => super::ProductionCategory::Infantry,
        FactoryType::UnitType if ty.naval => super::ProductionCategory::Ship,
        FactoryType::UnitType => super::ProductionCategory::Vehicle,
        FactoryType::AircraftType => super::ProductionCategory::Aircraft,
        FactoryType::BuildingType => super::ProductionCategory::Building,
    };
    let owner = building.owner();
    let current = sim.production.primary_factory(owner, category);
    let keep = current
        .and_then(|primary| sim.substrate.entities.get(primary))
        .is_some_and(|candidate| {
            !candidate.lifecycle.in_limbo()
                && candidate.owner() == owner
                && sim
                    .object_type(candidate.type_ref(), rules)
                    .is_some_and(|candidate_type| {
                        candidate_type.factory_type() == Some(factory)
                            && candidate_type.naval == ty.naval
                    })
        });
    if !keep {
        sim.production.set_primary_factory(owner, category, id);
    }
}

/// `TechnoTypeClass::Get_Ownable @ 0x00711EC0` (vtable `+0x70` of every
/// TechnoType class): every house for a `DoubleOwned=` type outside a
/// campaign, else the `Owner=` houses (`+0x6CC`).
fn get_ownable(obj: &ObjectType, rules: &RuleSet, game_mode_nonzero: bool) -> u32 {
    if obj.double_owned && game_mode_nonzero {
        return 0x7FFF_FFFF;
    }
    crate::sim::ai_buildable::house_token_mask(&obj.owner, rules)
}

#[cfg(test)]
#[path = "can_build_tests.rs"]
mod tests;
