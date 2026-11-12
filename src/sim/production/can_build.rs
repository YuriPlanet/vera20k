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
//! - Not a difference: `RequiredHouses=` also admits the countries in the
//!   house's masks (`+0x2C4..+0x2D0`, `0x004F7A07..0x004F7A51`), but only the
//!   House constructor writes them (zero; operand census of each offset,
//!   2026-10-12), so it admits the house's own country only, as VERA does.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{FactoryType, ObjectCategory, ObjectType};
use crate::rules::prerequisite::{Prerequisite, PrerequisiteGroup};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::house_tracking::HouseTracking;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{SimSoundEvent, Simulation};

#[cfg(test)]
#[path = "thief_capture_tests.rs"]
mod thief_capture_tests;

/// `HouseClass::CanBuild`'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanBuild {
    /// 0.
    No,
    /// 1.
    Yes,
    /// -1: the house is at the type's build limit.
    AtLimit,
}

/// `HouseClass::CanBuild @ 0x004F7870` for house `owner` and type `obj`.
///
/// A type VERA's room bans ([`crate::sim::game_options::RoomBans`]) is
/// refused first, whatever the flags. With `skip_tech` (the first flag),
/// only the build limit is asked ([`build_limit`]). Otherwise, in order (`0x004F7889..0x004F7BE5`):
/// - `TechnoType+0xC98` refuses; only the constructor writes it, with 0
///   (`0x007113CC`);
/// - a `PrerequisiteOverride=` BuildingType the house has on the map
///   (`+0x5550`) skips to the build limit;
/// - a matching owned Secret Lab grant skips to the build limit;
/// - `TechLevel=-1` refuses;
/// - `RequiresStolenThirdTech=`, `RequiresStolenSovietTech=` or
///   `RequiresStolenAlliedTech=` (`+0xD9B`, `+0xD9C`, `+0xD9D`) refuses
///   unless the house stole that tech;
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
///
/// With `ares`, installed CanBuild10021B00 replaces that stock outer route:
/// ordinary qualification includes FactoryOwners100221A0, and a prerequisite
/// override or lab grant still needs the shared factory search10020D40. Offline
/// factories answer AtLimit; online candidates continue to the common limit
/// owner, whose Ares signed-capacity branch has executed component controls in
/// `tools/rules_oracle/ares_factory_owners.json`. This does not claim the whole
/// Ares qualification prefix, every additional prerequisite policy or hijacking.
pub fn can_build(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
    skip_tech: bool,
    count_in_production: bool,
) -> CanBuild {
    #[cfg(feature = "debug-tools")]
    let trainer = sim.debug_flags_for(owner);
    #[cfg(feature = "debug-tools")]
    if trainer.tech_all {
        return build_limit(sim, rules, owner, obj, count_in_production);
    }
    // VERA's room bans (`RoomBans`, no gamemd option) refuse a banned type
    // to every house, ahead of the build-limit-only FindFactory route the
    // computer's teams take.
    if sim.session.game_options().bans.bans_type(obj) {
        return CanBuild::No;
    }
    if skip_tech {
        return build_limit(sim, rules, owner, obj, count_in_production);
    }
    let Some(house) = sim.houses.get(&owner) else {
        return CanBuild::No;
    };
    let controlled_by_human = house.is_controlled_by_human(sim.session.game_mode_nonzero());
    let status = requirements_met(sim, rules, owner, house, obj);
    let bypass_prerequisites = status == RequirementStatus::Overridden;
    #[cfg(not(feature = "ares"))]
    if bypass_prerequisites {
        return build_limit(sim, rules, owner, obj, count_in_production);
    }
    // Ares qualification4 skips ordinary requirements/prerequisites, but its
    // outer CanBuild10021BA1 still asks the real factory and build-limit owners.
    // Ares PrereqValidate10021B00 refuses every answer below Complete, its
    // Unbuildable included.
    if status < RequirementStatus::Complete {
        return CanBuild::No;
    }
    if !rules.ares() && !controlled_by_human {
        return CanBuild::Yes;
    }
    if !bypass_prerequisites && controlled_by_human && !owns_prerequisites(sim, rules, house, obj) {
        return CanBuild::No;
    }
    #[cfg(feature = "ares")]
    match query_factory(sim, rules, owner, obj, true, true, false, true) {
        FactorySearch::Missing | FactorySearch::BuildLimitRefused => return CanBuild::No,
        FactorySearch::Offline => return CanBuild::AtLimit,
        FactorySearch::Online(_) | FactorySearch::Primary(_) => {}
    }
    build_limit(sim, rules, owner, obj, count_in_production)
}

/// `HouseClass::CanBuild`'s qualification ahead of the prerequisites
/// (`0x004F7889..0x004F7BE5`, in the order [`can_build`] lists), which with
/// `ares` is `HouseExt::ExtData::RequirementsMet @ 10021FF0`: the status
/// Ares's CanBuild (`10021B00`) and UpdateSecretLab's boon pick
/// (`sim::secret_lab`) ask. The variants keep Ares's order, which
/// PrereqValidate compares (`< Complete` refuses).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RequirementStatus {
    /// 0: `Prerequisite.RequiredTheaters` without the scenario's theater
    /// (Ares only). `TechnoType+0xC98`, its other source, only the
    /// constructor writes (0, `0x007113CC`).
    #[cfg(feature = "ares")]
    Unbuildable,
    /// 1: an owned `Prerequisite.Negative` entry (Ares), `RequiredHouses=`,
    /// `ForbiddenHouses=` or the disabled shell.
    Forbidden,
    /// 2: `Prerequisite.StolenTechs` (Ares), `TechLevel=-1`, stolen tech,
    /// FactoryOwners or the tech level.
    Incomplete,
    /// 3.
    Complete,
    /// 4: reverse engineered, an on-map `PrerequisiteOverride=` or a lab
    /// grant; the prerequisites are skipped.
    Overridden,
}

/// [`RequirementStatus`] of `obj` for `house` (owned by `owner`).
pub(crate) fn requirements_met(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    house: &HouseState,
    obj: &ObjectType,
) -> RequirementStatus {
    #[cfg(feature = "debug-tools")]
    let trainer = sim.debug_flags_for(owner);
    #[cfg(not(feature = "debug-tools"))]
    let _ = owner;
    let controlled_by_human = house.is_controlled_by_human(sim.session.game_mode_nonzero());
    let override_on_map = obj.prerequisite_override.iter().any(|&entry| {
        if rules.ares() {
            // Ares3.0 qualification10021FF0 calls
            // HouseOwnsAny100081E0 at100220C0, before its human/AI split.
            prerequisite_on_map(sim, rules, house, entry)
        } else {
            // A negative stock-group code reads before the vanilla counter
            // array: the residual in `crate::rules::prerequisite`.
            match entry {
                Prerequisite::Building(index) => buildings_on_map(sim, rules, house, index) > 0,
                Prerequisite::Group(_) | Prerequisite::GenericGroup(_) => false,
            }
        }
    });
    // House4F78E4..4F7925 scans House+110 and calls459840. This
    // shortcut precedes all ordinary tech/country/prerequisite gates, for
    // humans and AI alike, and still asks the common build-limit owner.
    // Ares 3.0 RequirementsMet (10021FF0) opens with its own gates, in order:
    // the theater mask (Unbuildable), HouseOwnsAny100081E0 of the
    // Prerequisite.Negative list (Forbidden), the house's ReverseEngineered
    // list (`sim::reverse_engineering`, Overridden) and the HouseExt
    // StolenTech bits (+0x40) Prerequisite.StolenTechs needs (Incomplete).
    #[cfg(feature = "ares")]
    {
        let prerequisites = obj.techno_combat().prerequisites();
        if !prerequisites.admits_theater(sim.session.theater()) {
            return RequirementStatus::Unbuildable;
        }
        if prerequisites
            .negatives()
            .iter()
            .any(|&entry| prerequisite_on_map(sim, rules, house, entry))
        {
            return RequirementStatus::Forbidden;
        }
        if house.has_ares_reverse_engineered(obj.category, &obj.id) {
            return RequirementStatus::Overridden;
        }
        if !house.has_ares_stolen_techs(prerequisites.required_stolen_techs()) {
            return RequirementStatus::Incomplete;
        }
    }
    if override_on_map || crate::sim::secret_lab::house_grants(sim, rules, house, obj) {
        return RequirementStatus::Overridden;
    }
    // `0x004F7976..0x004F79E2`: each `RequiresStolen*Tech=` needs the
    // house's stolen-tech byte, which a spy's BuildTech infiltration sets.
    let type_tech_level = sim.object_tech_level(obj);
    // Installed qualification100220FE calls10020AD0, the same session-
    //aware human predicate; only its human arm rejects TechLevel=-1.
    if (type_tech_level == -1 && (!rules.ares() || controlled_by_human))
        || !house.has_stolen_tech_for(obj)
    {
        return RequirementStatus::Incomplete;
    }
    let house_bit = crate::sim::ai_buildable::house_country_bit(
        rules,
        sim.interner.resolve(house.house_type_id()),
    );
    let required = obj.required_houses_mask(rules);
    if required != u32::MAX && required & house_bit == 0 {
        return RequirementStatus::Forbidden;
    }
    // CanBuild4F7A7D treats raw-1 as its skip sentinel, even when a
    // nonempty authored list actually resolved to all32 Country bits.
    let forbidden = obj.forbidden_houses_mask(rules);
    if forbidden != u32::MAX && forbidden & house_bit != 0 {
        return RequirementStatus::Forbidden;
    }
    #[cfg(feature = "ares")]
    if !crate::sim::factory_owners::qualifies(sim, rules, house, obj) {
        return RequirementStatus::Incomplete;
    }
    // Ares disabled-shell predicate10021A00..10021A6A bypasses this
    // skirmish-shell restriction in mode0, before inspecting the Type.
    if (!rules.ares() || sim.session.game_mode_nonzero())
        && !sim.session.game_options().super_weapons
        && obj.category == ObjectCategory::Building
        && let Some(super_weapon) = obj.super_weapon.as_deref()
        && !rules.is_build_tech(&obj.id)
        && rules
            .super_weapon(super_weapon)
            .is_some_and(|super_weapon| super_weapon.disableable_from_shell)
    {
        return RequirementStatus::Forbidden;
    }
    #[cfg(feature = "debug-tools")]
    let exceeds_tech_level = type_tech_level > house.tech_level() && !trainer.tech_boost;
    #[cfg(not(feature = "debug-tools"))]
    let exceeds_tech_level = type_tech_level > house.tech_level();
    if exceeds_tech_level {
        return RequirementStatus::Incomplete;
    }
    RequirementStatus::Complete
}

/// Whether `house` has every `Prerequisite=` entry of `obj` on the map
/// ([`prerequisite_on_map`]; an empty list is met): CanBuild's human arm, and
/// with `ares` the power plant filter `0x10021CA0` of `sim::ai_base_building`.
/// That filter asks HouseOwnsAll `0x10008110` of each of the type's
/// prerequisite lists (TechnoTypeExt `+0x40`,
/// `ObjectType::ares_prerequisite_lists`) and is met by any one; the same
/// `PrerequisitesMet @ 10021CA0` is CanBuild's human arm under Ares
/// (PrereqValidate10021B00).
pub(crate) fn owns_prerequisites(
    sim: &Simulation,
    rules: &RuleSet,
    house: &HouseState,
    obj: &ObjectType,
) -> bool {
    #[cfg(feature = "ares")]
    let mut lists = obj.ares_prerequisite_lists();
    #[cfg(not(feature = "ares"))]
    let mut lists = std::iter::once(obj.prerequisite.as_slice());
    lists.any(|list| {
        list.iter()
            .all(|&entry| prerequisite_on_map(sim, rules, house, entry))
    })
}

/// Ares `TechnoTypeExt::ExtData::CanBeBuiltAt @ 1003D9A0`: an explicit
/// `BuiltAt=` list names `factory`'s type; an empty one admits any factory
/// type that is not `Factory.ExplicitOnly=`. No country aliases.
#[cfg(feature = "ares")]
pub(crate) fn ares_can_be_built_at(
    rules: &RuleSet,
    product: &ObjectType,
    factory: &ObjectType,
) -> bool {
    if product.built_at().is_empty() {
        return !factory.factory_explicit_only();
    }
    rules
        .type_array_index(ObjectCategory::Building, &factory.id)
        .is_some_and(|index| product.built_at().contains(&index))
}

/// One `Prerequisite=` entry of CanBuild's human arm
/// (`0x004F7C01..0x004F7E5E`). A group is met by any member of its
/// `[General] Prerequisite*` list on the map (`HouseClass+0x5550` above
/// zero), never by an empty list; `PROC` also by an on-map
/// `PrerequisiteProcAlternate=` unit (`+0x5564`, `0x004F7DB0..0x004F7DCE`), a
/// Slave Miner in retail. An ordinary BuildingType is met while its on-map
/// count is not zero (`0x004F7E50..0x004F7E5E`). A BuildingType with nonempty
/// `PowersUpBuilding=` instead asks only the last eligible House building's
/// three installed types (`0x004F7DF8..0x004F7E4E`).
/// Ares3.0 HouseOwnsAll10008110 and HouseOwnsAny100081E0 use retained
/// generic definitions, then Specific100082B0 for each direct/member building
/// and CountOwnedNow100032F0 for runtime alternates. Saved executable controls:
/// `tools/rules_oracle/ares_prerequisites.json`.
fn prerequisite_on_map(
    sim: &Simulation,
    rules: &RuleSet,
    house: &HouseState,
    entry: Prerequisite,
) -> bool {
    #[cfg(feature = "ares")]
    if let Some(group) = rules.ares_prerequisite_group(entry) {
        // Ares3.0 HouseOwnsAll10008110/Any100081E0 call Specific100082B0
        // for each immediate building member; generic codes do not recurse.
        return group.members().iter().any(|member| match *member {
            Prerequisite::Building(index) => specific_prerequisite_on_map(sim, rules, house, index),
            Prerequisite::Group(_) | Prerequisite::GenericGroup(_) => false,
        }) || group.alternates().iter().any(|alternate| {
            sim.interner.get(alternate.id()).is_some_and(|id| {
                // CountOwnedNow100032F0 reads ctor-to-dtor tracking. Limbo
                // does not remove it;1000819F tests nonzero, not positive.
                house
                    .tracking()
                    .owned_count(EntityCategory::from(alternate.category()), id)
                    != 0
            })
        });
    }
    let group = match entry {
        Prerequisite::Building(index) => {
            return specific_prerequisite_on_map(sim, rules, house, index);
        }
        Prerequisite::Group(group) => group,
        Prerequisite::GenericGroup(_) => return false,
    };
    rules
        .prerequisite_list(group)
        .iter()
        .any(|member| match *member {
            Prerequisite::Building(index) => buildings_on_map(sim, rules, house, index) > 0,
            // A group's code reads before the counter array: the residual in
            // `crate::rules::prerequisite`.
            Prerequisite::Group(_) | Prerequisite::GenericGroup(_) => false,
        })
        || (group == PrerequisiteGroup::Proc
            && rules
                .general
                .prerequisite_proc_alternate
                .as_deref()
                .and_then(|unit| rules.object_in_category(ObjectCategory::Vehicle, unit))
                .and_then(|unit| sim.interner.get(&unit.id))
                .is_some_and(|unit| house.tracking().active_count(EntityCategory::Unit, unit) > 0))
}

/// One building reference in gamemd4F7DF8..4F7E5E, or Ares3.0
/// HouseOwnsSpecific100082B0. Generic members call this same owner.
fn specific_prerequisite_on_map(
    sim: &Simulation,
    rules: &RuleSet,
    house: &HouseState,
    index: i32,
) -> bool {
    let Some(building_type) = rules.building_type_at(index) else {
        return false;
    };
    let count = buildings_on_map(sim, rules, house, index);
    if rules.ares() && count > 0 {
        // Ares HouseOwnsSpecific100082B0 accepts a positive direct/member
        // count before PowersUpBuilding, including a separately on-map upgrade.
        return true;
    }
    if building_type.building_upgrade().host().is_none() {
        return !rules.ares() && count != 0;
    }
    let Some(type_id) = sim.interner.get(&building_type.id) else {
        return false;
    };
    if rules.ares() {
        // Ares HouseOwnsSpecific100082B0 resolves the declared host, requires
        // its active count>0, then scans every House+6C building of that type. It reads
        // all three installed slots and no HP/limbo/online/Selling/count gate.
        let Some(host) = building_type
            .building_upgrade()
            .host()
            .and_then(|name| rules.ares_find_building(name))
            .and_then(|index| rules.building_type_at(index))
            .and_then(|host| sim.interner.get(&host.id))
        else {
            return false;
        };
        if house
            .tracking()
            .active_count(EntityCategory::Structure, host)
            <= 0
        {
            return false;
        }
        return matching_installed_upgrades(sim, house, host, type_id)
            .next()
            .is_some();
    }
    // Steam gamemd:4F7DF8..4F7E4E walks House+6C backwards, skips only limbo,
    // offline and either Selling selector, then checks all three
    // Building+5EC types. A miss stops; count/host type/HP are unread.
    let selling = MissionId::from_known(MissionType::Selling);
    house
        .base_projection()
        .buildings()
        .iter()
        .rev()
        .filter_map(|&id| sim.substrate.entities.get(id))
        .find(|building| {
            !building.lifecycle.in_limbo()
                && building.building_online()
                && building.mission.current() != selling
                && building.mission.queued() != selling
        })
        .is_some_and(|building| {
            building
                .building_upgrades()
                .types()
                .contains(&Some(type_id))
        })
}

/// The house's on-map count of BuildingType `index` (`HouseClass+0x5550`).
fn buildings_on_map(sim: &Simulation, rules: &RuleSet, house: &HouseState, index: i32) -> i32 {
    rules
        .building_type_at(index)
        .and_then(|building| sim.interner.get(&building.id))
        .map_or(0, |type_id| {
            house
                .tracking()
                .active_count(EntityCategory::Structure, type_id)
        })
}

/// The shared installed-slot traversal: Specific100082B0 owns its active-host
///gate; CountOwned10020AF0 does not. Neither checks count/limbo/power/Selling.
fn matching_installed_upgrades<'a>(
    sim: &'a Simulation,
    house: &'a HouseState,
    host: InternedId,
    upgrade: InternedId,
) -> impl Iterator<Item = &'a Option<InternedId>> {
    house
        .base_projection()
        .buildings()
        .iter()
        .filter_map(|&id| sim.substrate.entities.get(id))
        .filter(move |building| building.type_ref() == host)
        .flat_map(|building| building.building_upgrades().types().iter())
        .filter(move |&&entry| entry == Some(upgrade))
}

/// Installed Ares CountOwned10020AF0. Counter values remain authoritative in
///HouseTracking; conversion partners and installed slots are queried, not cached.
#[cfg(feature = "ares")]
fn ares_owned_build_limit_count(
    sim: &Simulation,
    rules: &RuleSet,
    house: &HouseState,
    obj: &ObjectType,
) -> i32 {
    let tracked = |category, id: &str| {
        sim.interner
            .get(id)
            .map_or(0, |id| house.tracking().owned_count(category, id))
    };
    if obj.category == ObjectCategory::Building
        && let Some(host) = obj.building_upgrade().host()
    {
        let Some((host, upgrade)) = rules
            .ares_find_building(host)
            .and_then(|index| rules.building_type_at(index))
            .and_then(|host| sim.interner.get(&host.id))
            .zip(sim.interner.get(&obj.id))
        else {
            return 0;
        };
        return matching_installed_upgrades(sim, house, host, upgrade)
            .fold(0_i32, |count, _| count.wrapping_add(1));
    }
    // Ares CountOwned10020C23..10020C81 keeps the original Infantry
    // VehicleThief census. The existing Unit+338 owner supplies this count.
    let own = tracked(EntityCategory::from(obj.category), &obj.id)
        .wrapping_add(captured_vehicle_thief_count(sim, rules, house.name(), obj));
    let converted = match obj.category {
        ObjectCategory::Building => obj
            .undeploys_into
            .as_deref()
            .and_then(|id| rules.object_in_category(ObjectCategory::Vehicle, id)),
        ObjectCategory::Vehicle => obj
            .deploys_into
            .as_deref()
            .and_then(|id| rules.object_in_category(ObjectCategory::Building, id)),
        ObjectCategory::Infantry | ObjectCategory::Aircraft => None,
    };
    own.wrapping_add(converted.map_or(0, |converted| {
        tracked(EntityCategory::from(converted.category), &converted.id)
    }))
}

/// Ares `HouseExt::BuildLimitRemaining` (inlined in CanBuild at
/// `10021BEB..10021C46` and in HouseClass_CanInstantiateTeam `100240D0`):
/// signed wrapping capacity left under `BuildLimit=`. A negative limit
/// counts what the house produced, zero or more what it owns now.
#[cfg(feature = "ares")]
pub(crate) fn ares_build_limit_remaining(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> i32 {
    let limit = obj.build_limit;
    if limit < 0 {
        let produced = rules
            .type_array_index(obj.category, &obj.id)
            .zip(sim.houses.get(&owner))
            .map_or(0, |(index, house)| {
                house.produced_count(obj.category, index)
            });
        limit.wrapping_add(produced).wrapping_neg()
    } else {
        let owned = sim.houses.get(&owner).map_or(0, |house| {
            ares_owned_build_limit_count(sim, rules, house, obj)
        });
        limit.wrapping_sub(owned)
    }
}

/// CanBuild's build limit (`0x004F7F9E..0x004F8357`): a type at or over
/// its positive `BuildLimit=` by the house's tracked count
/// (`HouseTracking::owned_count`) is at its limit, unless
/// `count_in_production` and one of the house's factories holds one
/// (`FactoryClass::Get_Object @ 0x004CA160`), which is Yes. For infantry the
/// factory scan runs only when the count equals the limit exactly.
fn build_limit(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
    count_in_production: bool,
) -> CanBuild {
    let limit = obj.build_limit;
    #[cfg(feature = "ares")]
    if sim.houses.get(&owner).is_some_and(|house| {
        !house.is_controlled_by_human(sim.session.game_mode_nonzero())
            && rules.ares_ai_bypasses_build_limit(house.difficulty().table_index())
    }) {
        //10021BD2..10021BE9 consult the retained RulesExt difficulty slot,
        //including limit-only calls. Humans never receive this bypass.
        return CanBuild::Yes;
    }
    let type_id = sim.interner.get(&obj.id);
    #[cfg(not(feature = "ares"))]
    let owned = type_id
        .zip(sim.houses.get(&owner))
        .map_or(0, |(type_id, house)| {
            house
                .tracking()
                .owned_count(EntityCategory::from(obj.category), type_id)
        })
        .wrapping_add(captured_vehicle_thief_count(sim, rules, owner, obj));
    #[cfg(not(feature = "ares"))]
    let produced = || {
        rules
            .type_array_index(obj.category, &obj.id)
            .zip(sim.houses.get(&owner))
            .map_or(0, |(index, house)| {
                house.produced_count(obj.category, index)
            })
    };
    let held_in_factory = || {
        type_id.is_some_and(|type_id| {
            sim.production
                .factories()
                .iter_insertion_ordered()
                .into_iter()
                .any(|factory| {
                    factory.owner() == owner
                        && factory.object().is_some_and(|object| {
                            object.type_id == type_id && object.entity_id.is_some()
                        })
                })
        })
    };
    #[cfg(feature = "ares")]
    {
        // Installed CanBuild10021BEB..10021C46 uses signed wrapping
        // remaining capacity for every class. Its held-object exception also
        // admits Infantry above the limit, unlike the original
        // gamemd-specific class branch below.
        let remaining = ares_build_limit_remaining(sim, rules, owner, obj);
        if limit > 0 && remaining <= 0 {
            return if count_in_production && held_in_factory() {
                CanBuild::Yes
            } else {
                CanBuild::AtLimit
            };
        }
        if remaining > 0 {
            CanBuild::Yes
        } else {
            CanBuild::No
        }
    }
    #[cfg(not(feature = "ares"))]
    {
        //4F7FD6/4F80A1/4F81D0/4F829B use the retained produced counter;
        //CDQ/XOR/SUB keeps INT_MIN's magnitude negative. The owner/corpus is
        //tools/can_build_produced_history, shared with Record4FB6B0.
        if limit < 1 {
            if produced() >= limit.wrapping_abs() {
                return CanBuild::No;
            }
            if obj.category != ObjectCategory::Infantry {
                return CanBuild::Yes;
            }
        }
        if type_id.is_none() || limit < 1 || owned < limit {
            return CanBuild::Yes;
        }
        if !count_in_production || (obj.category == ObjectCategory::Infantry && owned != limit) {
            return CanBuild::AtLimit;
        }
        if held_in_factory() {
            CanBuild::Yes
        } else {
            CanBuild::AtLimit
        }
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
/// magnitude is reached by what the house produced and what the factory
/// holds; with a positive one, once the
/// tracked count (`owned_count`) and the factory's reach it. An infantry
/// `VehicleThief=` adds the units that hold one (`0x0050B4C4..0x0050B506`).
#[cfg(not(feature = "ares"))]
pub(crate) fn check_build_limit(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> bool {
    let strip = build_limit_strip(obj);
    if obj.category == ObjectCategory::Aircraft && obj.airport_bound {
        return airport_docks_full(sim, rules, owner, strip);
    }
    let limit = obj.build_limit;
    let type_id = sim.interner.get(&obj.id);
    let queued = type_id.map_or(0, |type_id| {
        sim.production
            .factories()
            .count_total(owner, strip, type_id)
    });
    if limit < 1 {
        // 50B405/48B/537/60B use the same retained produced item as
        // CanBuild, add Factory CountTotal with ADD32, then compare signed.
        let produced = rules
            .type_array_index(obj.category, &obj.id)
            .zip(sim.houses.get(&owner))
            .map_or(0, |(index, house)| {
                house.produced_count(obj.category, index)
            });
        return produced.wrapping_add(queued) >= limit.wrapping_abs();
    }
    let owned = type_id
        .zip(sim.houses.get(&owner))
        .map_or(0, |(type_id, house)| {
            house
                .tracking()
                .owned_count(EntityCategory::from(obj.category), type_id)
        });
    owned
        .wrapping_add(captured_vehicle_thief_count(sim, rules, owner, obj))
        .wrapping_add(queued)
        >= limit
}

/// With `ares`, hook `0x0050B370` (HouseClass_ShouldDisableCameo, Ares.dll
/// `10024F80`) replaces the whole of CheckBuildLimit and returns at
/// `0x0050B669`, for every caller. It counts what the house's primary factory
/// for the type holds (GetPrimaryFactory `0x00500510` with BuildCat DontCare,
/// the Buildings factory for every structure). An `AirportBound=` aircraft is
/// still at its limit once the `[General] PadAircraft=` aircraft present and
/// queued reach the AirportDocks, but otherwise goes on to its `BuildLimit=`.
/// Under a positive limit the object that factory holds counts once less, as
/// the house's counters hold it too. The type is at its limit when
/// [`ares_build_limit_remaining`] less that count is not positive, and
/// otherwise unless the shared search `10020D40`, with Ares CanBuild's
/// arguments, finds a usable factory.
#[cfg(feature = "ares")]
pub(crate) fn check_build_limit(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> bool {
    let strip = build_limit_strip(obj);
    if obj.category == ObjectCategory::Aircraft
        && obj.airport_bound
        && airport_docks_full(sim, rules, owner, strip)
    {
        return true;
    }
    let factories = sim.production.factories();
    let queued = sim.interner.get(&obj.id).map_or(0, |type_id| {
        let total = factories.count_total(owner, strip, type_id);
        let held = obj.build_limit > 0
            && factories
                .view(owner, strip)
                .and_then(|factory| factory.object)
                .is_some_and(|object| object.type_id == type_id);
        if held { total.wrapping_sub(1) } else { total }
    });
    ares_build_limit_remaining(sim, rules, owner, obj).wrapping_sub(queued) <= 0
        || !matches!(
            query_factory(sim, rules, owner, obj, true, true, false, true),
            FactorySearch::Online(_) | FactorySearch::Primary(_)
        )
}

/// The strip whose House factory CheckBuildLimit counts: both structure
/// strips count the Buildings factory (`0x0050B3B9`; Ares' GetPrimaryFactory
/// `0x00500510` with BuildCat DontCare answers the same one).
fn build_limit_strip(obj: &ObjectType) -> super::ProductionCategory {
    match obj.category {
        ObjectCategory::Building => super::ProductionCategory::Building,
        _ => super::production_tech::production_category_for_object(obj),
    }
}

/// CheckBuildLimit's `AirportBound=` arm (`0x0050B570..0x0050B5EB`, kept by
/// Ares `10024F80`): the `[General] PadAircraft=` aircraft on the map
/// (`+0x558C`) and in the house's factory for `strip` have reached its
/// AirportDocks ([`HouseTracking::airport_docks`]).
fn airport_docks_full(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    strip: super::ProductionCategory,
) -> bool {
    let tracking = sim.houses.get(&owner).map(HouseState::tracking);
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
            let in_factory = type_id.map_or(0, |type_id| {
                sim.production
                    .factories()
                    .count_total(owner, strip, type_id)
            });
            on_map.wrapping_add(in_factory)
        })
        .fold(0, i32::wrapping_add);
    pads >= tracking.map_or(0, HouseTracking::airport_docks)
}

/// The shared Unit census in House::CanBuild4F80E6..4F8128 and
/// CheckBuildLimit50B4C4..50B506 (retail gamemd.exe). Both include limbo
/// and pending destruction; only owner and retained InfantryType index are
/// tested. The entity store owns membership; no second counter is needed.
fn captured_vehicle_thief_count(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> i32 {
    if obj.category == ObjectCategory::Infantry && obj.vehicle_thief {
        rules
            .type_array_index(ObjectCategory::Infantry, &obj.id)
            .map_or(0, |index| {
                sim.entities()
                    .values()
                    .filter(|entity| {
                        entity.category() == EntityCategory::Unit
                            && entity.owner() == owner
                            && entity.capture_infantry_type_index.get() == index
                    })
                    .fold(0i32, |count, _| count.wrapping_add(1))
            })
    } else {
        0
    }
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
    let object_type = sim.entity_object_type(object, rules)?;
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
pub fn find_factory(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
    skip_busy_airfield: bool,
    require_online: bool,
    require_can_build: bool,
) -> Option<u64> {
    query_factory(
        sim,
        rules,
        owner,
        obj,
        skip_busy_airfield,
        require_online,
        require_can_build,
        false,
    )
    .available_factory()
}

/// The shared factory search preserves the distinction needed by Ares
/// CanBuild10021BB5: status0/1 refuses,2 is unavailable,3/4 is available.
/// Existing object/production consumers take only its available pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FactorySearch {
    Missing,
    #[cfg(feature = "ares")]
    BuildLimitRefused,
    #[cfg(feature = "ares")]
    Offline,
    Online(u64),
    Primary(u64),
}

impl FactorySearch {
    fn available_factory(self) -> Option<u64> {
        match self {
            Self::Online(id) | Self::Primary(id) => Some(id),
            _ => None,
        }
    }
}

/// One building of a house that a FindFactory scan may pick for some type.
#[derive(Clone, Copy)]
struct FactoryCandidate<'a> {
    id: u64,
    building: &'a crate::sim::game_entity::GameEntity,
    building_type: &'a ObjectType,
    factory_type: FactoryType,
}

/// `house`'s buildings that pass the tests of the 5F7900 scan which do not
/// read the wanted type, in House+6C order: out of limbo, a factory of some
/// kind, and neither Selling selector.
fn factory_candidates<'a>(
    sim: &'a Simulation,
    rules: &'a RuleSet,
    house: &'a HouseState,
) -> impl Iterator<Item = FactoryCandidate<'a>> + 'a {
    let selling = MissionId::from_known(MissionType::Selling);
    house
        .base_projection()
        .buildings()
        .iter()
        .filter_map(move |&id| {
            let building = sim.substrate.entities.get(id)?;
            let building_type = sim.entity_object_type(building, rules)?;
            let factory_type = building_type.factory_type()?;
            (!building.lifecycle.in_limbo()
                && building.mission.effective() != selling
                && building.mission.queued() != selling)
                .then_some(FactoryCandidate {
                    id,
                    building,
                    building_type,
                    factory_type,
                })
        })
}

/// VERA-only: one house's [`factory_candidates`], kept for a caller that asks
/// FindFactory of many types in a row ([`find_factory_among`]) while the
/// world stands, so each question walks the factories and not every building
/// of the house. It borrows the world, which therefore cannot change under it.
pub(crate) struct FactoryRoster<'a>(Vec<FactoryCandidate<'a>>);

impl<'a> FactoryRoster<'a> {
    /// `None` for an owner without a House: its every search is empty.
    pub(crate) fn of(sim: &'a Simulation, rules: &'a RuleSet, owner: InternedId) -> Option<Self> {
        let house = sim.houses.get(&owner)?;
        Some(Self(factory_candidates(sim, rules, house).collect()))
    }
}

/// [`find_factory`] for `owner`, asked of the `roster` built for that house.
pub(crate) fn find_factory_among(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    roster: &FactoryRoster<'_>,
    obj: &ObjectType,
    skip_busy_airfield: bool,
    require_online: bool,
    require_can_build: bool,
) -> Option<u64> {
    scan_factories(
        sim,
        rules,
        owner,
        roster.0.iter().copied(),
        obj,
        skip_busy_airfield,
        require_online,
        require_can_build,
        false,
    )
    .available_factory()
}

/// One scan owner for stock5F7900 and the installed Ares10020D40 replacement.
/// Ares CanBuild asks stop_first=true; the object-type hook asks false and
/// retains the last eligible nonprimary, exactly as the stock route does.
fn query_factory(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
    skip_busy_airfield: bool,
    require_online: bool,
    require_can_build: bool,
    stop_first: bool,
) -> FactorySearch {
    let Some(house) = sim.houses.get(&owner) else {
        return FactorySearch::Missing;
    };
    scan_factories(
        sim,
        rules,
        owner,
        factory_candidates(sim, rules, house),
        obj,
        skip_busy_airfield,
        require_online,
        require_can_build,
        stop_first,
    )
}

/// The 5F7900 scan over `owner`'s `candidates`, in their order.
fn scan_factories<'a>(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    candidates: impl Iterator<Item = FactoryCandidate<'a>>,
    obj: &ObjectType,
    skip_busy_airfield: bool,
    require_online: bool,
    require_can_build: bool,
    stop_first: bool,
) -> FactorySearch {
    //10020D43..10020D7A asks the limit-only CanBuild once, before scanning.
    // The first flag bypasses qualification and this factory query, so this
    // does not recurse through either admission gate.
    #[cfg(feature = "ares")]
    if require_can_build && can_build(sim, rules, owner, obj, true, true) != CanBuild::Yes {
        return FactorySearch::BuildLimitRefused;
    }
    #[cfg(not(feature = "ares"))]
    let _ = owner;
    let factory_type = FactoryType::for_category(obj.category);
    let game_mode_nonzero = sim.session.game_mode_nonzero();
    let ownable = get_ownable(obj, rules, game_mode_nonzero);
    let naval_unit = obj.category == ObjectCategory::Vehicle && obj.naval;
    let mut candidate = FactorySearch::Missing;
    for FactoryCandidate {
        id,
        building,
        building_type,
        factory_type: building_factory_type,
    } in candidates
    {
        if building_factory_type == factory_type
            && (rules.ares() || !require_online || building.building_online())
            && (rules.ares()
                || !require_can_build
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
            #[cfg(feature = "ares")]
            {
                //10020E65 ->1003D9A0.
                if !ares_can_be_built_at(rules, obj, building_type) {
                    continue;
                }
                //10020E73..10020E92 preserves offline availability separately
                // from an earlier online candidate; it cannot downgrade one.
                if require_online
                    && (!building.building_online() || building.deactivated.is_offline())
                {
                    if !matches!(
                        candidate,
                        FactorySearch::Online(_) | FactorySearch::Primary(_)
                    ) {
                        candidate = FactorySearch::Offline;
                    }
                    continue;
                }
            }
            let primary = sim.production.is_primary_factory(id);
            candidate = if primary {
                FactorySearch::Primary(id)
            } else {
                FactorySearch::Online(id)
            };
            if primary || (rules.ares() && stop_first) {
                break;
            }
        }
    }
    candidate
}

/// Building448070/448160's primary peer group: the same raw nonzero
/// BuildingType+EB8 Factory value (`0x004480D5`, `0x004481CB`) and the same
/// Naval byte (`0x004480DD..0x00448102`, `0x004481D3..0x004481F2`). Raw
/// Unit1 and UnitType40 are distinct groups although FindFactory accepts
/// neither as the other's exact Type.
fn same_primary_group(a: &ObjectType, b: &ObjectType) -> bool {
    a.factory_rtti() == b.factory_rtti() && a.naval == b.naval
}

/// Building448070, called from admitted Unlimbo4411B1 and ChangeOwner448C69:
/// a raw Factory of 0 returns at once (`0x0044807C..0x00448088`); otherwise
/// preserve an existing non-limbo primary of the same group, or make this
/// building primary. This is the lifecycle producer, not the fallback
/// choice returned by FindFactory. Exact `Factory=BuildingType` (raw 7) then
/// has its House determine the edge, whether or not it took the flag
/// (`0x0044812D..0x0044813D`, [`Simulation::determine_house_edge`]).
///
/// RESIDUAL: 448070 has no limbo gate; this port returns early for a
/// building in limbo, which ChangeOwner can pass (trigger Action36,
/// `trigger_change_all_house_objects`, has no limbo filter). Natively it can
/// take the flag, which Unlimbo clears again (`0x0044119C`), and a
/// `Factory=BuildingType` one runs DetermineEdge. No stock path is known to
/// leave a `Factory=BuildingType` building in limbo for Action36 to reach.
pub(crate) fn initialize_factory_primary(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let Some(building) = sim.substrate.entities.get(id) else {
        return;
    };
    if building.category() != EntityCategory::Structure || building.lifecycle.in_limbo() {
        return;
    }
    let Some(ty) = sim.entity_object_type(building, rules) else {
        return;
    };
    if !ty.has_factory() {
        return;
    }
    let owner = building.owner();
    //4480B8..448114 walks House+6C, rather than choosing one retained ID.
    // A limbo primary does not prevent this admitted building becoming primary.
    let keep = sim.houses.get(&owner).is_some_and(|house| {
        house
            .base_projection()
            .buildings()
            .iter()
            .any(|&candidate_id| {
                sim.substrate
                    .entities
                    .get(candidate_id)
                    .is_some_and(|candidate| {
                        !candidate.lifecycle.in_limbo()
                            && sim.production.is_primary_factory(candidate_id)
                            && sim.entity_object_type(candidate, rules).is_some_and(
                                |candidate_type| same_primary_group(candidate_type, ty),
                            )
                    })
            })
    });
    if !keep {
        sim.production.set_primary_factory(id);
    }
    if ty.factory_type() == Some(FactoryType::BuildingType) {
        sim.determine_house_edge(owner, id, rules);
    }
}

/// Building WhatAction447210's ordinary factory self arm4472EC..447348.
/// Techno7000B9..7000E8 owns the separate lone-selected/current-player gate.
/// A current primary or InvisibleInGame factory answers Action0, not Action4.
/// Source: original gamemd.exe.
pub fn can_select_factory_primary(sim: &Simulation, rules: &RuleSet, id: u64) -> bool {
    sim.substrate.entities.get(id).is_some_and(|building| {
        building.category() == EntityCategory::Structure
            && !sim.production.is_primary_factory(id)
            && sim
                .entity_object_type(building, rules)
                .is_some_and(|ty| !ty.invisible_in_game && ty.has_factory())
    })
}

/// Building448160's manual primary toggle, reached by Event17 at4C71B9.
/// Setting clears matching non-limbo House peers and announces only for the
/// current player's House; a real queued duplicate clears the target instead.
/// The receiver owns its live-Building gate. There is no owner, limbo, online,
/// Selling or health admission in this native body. Returns control admission;
/// the resulting native+3D3 value is the existing owner's per-ID query.
/// Source: original gamemd.exe;
/// consumed controls: tools/spatial_oracle/building_manual_primary.json.
pub(crate) fn toggle_factory_primary(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    // 0x00448170..0x00448178: a raw Factory of 0 changes nothing.
    let Some((owner, ty)) = sim.substrate.entities.get(id).and_then(|building| {
        if building.category() != EntityCategory::Structure {
            return None;
        }
        let ty = sim.entity_object_type(building, rules)?;
        ty.has_factory().then_some((building.owner(), ty))
    }) else {
        return false;
    };
    if sim.production.is_primary_factory(id) {
        sim.production.clear_primary_factory(id);
    } else {
        let Some(house) = sim.houses.get(&owner) else {
            return false;
        };
        let peers: Vec<_> = house
            .base_projection()
            .buildings()
            .iter()
            .copied()
            .filter(|&peer| {
                sim.substrate.entities.get(peer).is_some_and(|candidate| {
                    !candidate.lifecycle.in_limbo()
                        && sim
                            .entity_object_type(candidate, rules)
                            .is_some_and(|peer_type| same_primary_group(peer_type, ty))
                })
            })
            .collect();
        for peer in peers {
            sim.production.clear_primary_factory(peer);
        }
        sim.production.set_primary_factory(id);
        if sim.house_is_human_player(owner) {
            sim.sound_events.emit(SimSoundEvent::HouseEva {
                owner,
                event: "EVA_PrimaryBuildingSelected",
            });
        }
        if ty.factory_type() == Some(FactoryType::BuildingType) {
            //44822B..448241 is setting-only: old-primary clear jumps448246.
            // The ordinary GAWEAP(raw40) chain does not enter this branch.
            sim.determine_house_edge(owner, id, rules);
        }
    }
    //44824C Mark(CHANGE) redraws and propagates palette/cloak presentation,
    // without the PlaceUp/PlaceDown occupancy arms (43FB0C[2]=43F9A6).
    // Healthy GAWEAP's inputs stay unchanged; existing live-frame owners
    // rebuild its rendering/label, without a separate dirty/primary mirror.
    // Wider cloak/flash/attached-Anim refresh remains outside this chain.
    true
}

/// `TechnoTypeClass::Get_Ownable @ 0x00711EC0` (vtable `+0x70` of every
/// TechnoType class): every house for a `DoubleOwned=` type outside a
/// campaign, else the `Owner=` houses (`+0x6CC`).
fn get_ownable(obj: &ObjectType, rules: &RuleSet, game_mode_nonzero: bool) -> u32 {
    if obj.double_owned && game_mode_nonzero {
        return 0x7FFF_FFFF;
    }
    obj.owner_mask(rules)
}

#[cfg(test)]
#[path = "can_build_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "can_build_produced_history_tests.rs"]
mod produced_history_tests;

#[cfg(all(test, feature = "ares"))]
#[path = "can_build_factory_owners_tests.rs"]
mod factory_owners_tests;
