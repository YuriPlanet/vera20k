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
pub(crate) fn can_build(
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
    if !crate::rules::ARES_ENABLED && !controlled_by_human {
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
        if crate::rules::ARES_ENABLED {
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
    if (type_tech_level == -1 && (!crate::rules::ARES_ENABLED || controlled_by_human))
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
    if (!crate::rules::ARES_ENABLED || sim.session.game_mode_nonzero())
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
    if crate::rules::ARES_ENABLED && count > 0 {
        // Ares HouseOwnsSpecific100082B0 accepts a positive direct/member
        // count before PowersUpBuilding, including a separately on-map upgrade.
        return true;
    }
    if building_type.building_upgrade().hosts().is_empty() {
        return !crate::rules::ARES_ENABLED && count != 0;
    }
    let Some(type_id) = sim.interner.get(&building_type.id) else {
        return false;
    };
    if crate::rules::ARES_ENABLED {
        // Ares HouseOwnsSpecific100082B0 resolves one declared host, requires
        // its active count>0, then scans every House+6C building of that type. It reads
        // all three installed slots and no HP/limbo/online/Selling/count gate.
        // Phobos `BuildingTypeExt::CanUpgrade` (`Ext/BuildingType/Hooks.Upgrade.cpp:
        // 21-25`) walks every `PowersUp.Buildings` entry instead, so any
        // declared host that resolves and carries the slot satisfies this.
        for name in building_type.building_upgrade().hosts() {
            let Some(host) = rules
                .ares_find_building(name)
                .and_then(|index| rules.building_type_at(index))
                .and_then(|host| sim.interner.get(&host.id))
            else {
                continue;
            };
            if house
                .tracking()
                .active_count(EntityCategory::Structure, host)
                <= 0
            {
                continue;
            }
            if matching_installed_upgrades(sim, house, host, type_id)
                .next()
                .is_some()
            {
                return true;
            }
        }
        return false;
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
    if obj.category == ObjectCategory::Building && !obj.building_upgrade().hosts().is_empty() {
        // Phobos `BuildingTypeExt::GetUpgradesAmount`
        // (`Ext/BuildingType/Body.cpp:160-161`) sums the installed slots over
        // every `PowersUp.Buildings` host; a name the reader left unresolved
        // contributes nothing. The sum keeps `i32` wrapping semantics.
        let Some(upgrade) = sim.interner.get(&obj.id) else {
            return 0;
        };
        return obj
            .building_upgrade()
            .hosts()
            .into_iter()
            .filter_map(|name: &str| {
                rules
                    .ares_find_building(name)
                    .and_then(|index| rules.building_type_at(index))
                    .and_then(|host| sim.interner.get(&host.id))
            })
            .fold(0_i32, |count, host| {
                count.wrapping_add(
                    matching_installed_upgrades(sim, house, host, upgrade)
                        .fold(0_i32, |count, _| count.wrapping_add(1)),
                )
            });
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
    // Phobos `BuildLimitGroup.*` group cap (`HouseExt::BuildLimitGroupUpgradeCheck
    // @ Ext/House/Body.cpp:802-1013`; gamemd entry `HouseClass_CanBuild_
    // UpgradesInteraction @ 0x4F8361`, `Ext/BuildingType/Hooks.Upgrade.cpp:107`).
    // The native replacement only runs for a building type with
    // `PowersUp.Buildings`, and its tail lets a copy in production keep the
    // type buildable when `includeInProduction` is set.
    #[cfg(feature = "phobos")]
    if build_limit_group_upgrade_check(sim, rules, owner, obj, count_in_production) {
        return CanBuild::AtLimit;
    }
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
pub(crate) fn check_build_limit(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> bool {
    // `HouseExt::ReachedBuildLimit @ Ext/House/Body.cpp:1036-1188`, reached
    // through `HouseClass_ShouldDisableCameo_GreyCameo @ 0x50B669`
    // (`Ext/House/Hooks.cpp:493-503`): grey the cameo / drop its queue.
    #[cfg(feature = "phobos")]
    if build_limit_group_reached(sim, rules, owner, obj) {
        return true;
    }
    let strip = match obj.category {
        ObjectCategory::Building => super::ProductionCategory::Building,
        _ => super::production_tech::production_category_for_object(obj),
    };
    let tracking = sim.houses.get(&owner).map(HouseState::tracking);
    let in_factory = |type_id: Option<InternedId>| {
        type_id.map_or(0, |type_id| {
            sim.production
                .factories()
                .count_total(owner, strip, type_id)
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
    let owned = type_id.zip(tracking).map_or(0, |(type_id, tracking)| {
        tracking.owned_count(EntityCategory::from(obj.category), type_id)
    });
    owned
        .wrapping_add(captured_vehicle_thief_count(sim, rules, owner, obj))
        .wrapping_add(queued)
        >= limit
}

/// Phobos `BuildLimitGroup.*` group cap. Native evidence: `HouseExt::
/// BuildLimitGroupUpgradeCheck @ Ext/House/Body.cpp:802-1013` (entered from
/// `HouseClass_CanBuild_UpgradesInteraction @ 0x4F8361`,
/// `Ext/BuildingType/Hooks.Upgrade.cpp:93-111`) and `HouseExt::
/// ReachedBuildLimit @ Ext/House/Body.cpp:1036-1188` (entered from
/// `HouseClass_ShouldDisableCameo_GreyCameo @ 0x50B669`,
/// `Ext/House/Hooks.cpp:493-503`). VERA's owners are `HouseClass::CanBuild
/// @ 0x004F7870` / `HouseClass::CheckBuildLimit @ 0x0050B370`, with
/// `FactoryClass::CountTotal @ 0x004CA670` for the queue. The two native
/// bodies share the census below but disagree on the reach formula, the
/// owned counter and the queue flag's effect, so they stay distinct.
///
/// RESIDUAL: both native bodies also call `RemoveProduction` to prune queued
/// copies when a group reaches its limit (`Body.cpp:896,943,962,995,1008,
/// 1152,1179`). This gate is a query and does not mutate the queue, so the
/// prune runs from the queue-admission owner instead
/// ([`phobos_trim_build_limit_group_queue`]); the `UpgradeCheck` arm's own
/// prune (`Body.cpp:896-1008`) has no mutable caller yet.
#[cfg(feature = "phobos")]
struct BuildLimitGroupCensus {
    /// `Nums` after the ExtraLimit pass.
    limits: Vec<i32>,
    /// Resolved `Types` members in list order; `limits[i]` pairs with
    /// `members[i]` exactly as the native read-time filter pairs them.
    members: Vec<String>,
    /// `CountOwnedNow` per member (`ReachedBuildLimit`'s counter).
    owned_now: Vec<i32>,
    /// `CountOwnedIncludeDeploy` per member (`UpgradeCheck`'s counter).
    owned_deploy: Vec<i32>,
    /// `QueuedNum` per member.
    queued: Vec<i32>,
    /// `FactoryClass::FindByOwnerAndProduct` per member.
    in_production: Vec<bool>,
    /// Each member's own `BuildLimitGroup.Factor`.
    member_factor: Vec<i32>,
    /// `obj`'s own `BuildLimitGroup.Factor`.
    factor: i32,
    content_if_any: bool,
    /// `BuildLimitGroup_NotBuildableIfQueueMatch`, type value over the global.
    not_buildable: bool,
}

#[cfg(feature = "phobos")]
fn build_limit_group_census(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> Option<BuildLimitGroupCensus> {
    let group = &obj.phobos_build_limit_group;
    let house = sim.houses.get(&owner)?;
    let resolve = |name: &str| rules.object(name);
    let tracked = |ty: &ObjectType| -> i32 {
        sim.interner.get(&ty.id).map_or(0, |id| {
            house
                .tracking()
                .owned_count(EntityCategory::from(ty.category), id)
        })
    };
    // `CountOwnedNow` with the native upgrade substitution: a type whose
    // `PowersUp_Buildings` is non-empty counts installed slots instead.
    let owned_now = |ty: &ObjectType| -> i32 {
        sim.count_owned_building_upgrades(owner, ty, rules)
            .unwrap_or_else(|| tracked(ty))
    };
    // `CountOwnedIncludeDeploy @ Ext/House/Body.cpp:774-780`: the type plus
    // its DeploysInto / UndeploysInto types' plain counts.
    let owned_deploy = |ty: &ObjectType| -> i32 {
        let mut count = owned_now(ty);
        for converted in [ty.deploys_into.as_deref(), ty.undeploys_into.as_deref()]
            .into_iter()
            .flatten()
        {
            if let Some(other) = resolve(converted) {
                count = count.wrapping_add(tracked(other));
            }
        }
        count
    };
    // `QueuedNum @ Ext/House/Body.cpp:1015-1034` = CountTotal(pType) minus the
    // factory's held object when it is that exact type. `held_in_factory`
    // mirrors CanBuild's held-object query over the existing factory registry;
    // queue matches are exact TechnoType (ID), not by group or category.
    let held_in_factory = |type_id: InternedId| {
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
    };
    let queued_num = |ty: &ObjectType| -> i32 {
        let Some(type_id) = sim.interner.get(&ty.id) else {
            return 0;
        };
        let strip = match ty.category {
            ObjectCategory::Building => super::ProductionCategory::Building,
            _ => super::production_tech::production_category_for_object(ty),
        };
        let total = sim
            .production
            .factories()
            .count_total(owner, strip, type_id);
        (total - i32::from(held_in_factory(type_id))).max(0)
    };

    // ExtraLimit quota (`UpgradeCheck:809-843` / `ReachedBuildLimit:1049-1077`):
    // a local copy of Nums. Each ExtraLimit type adds `count * Nums[i]` to
    // every limit; `MaxCount[i]` caps the type's counted units, `MaxNum` caps
    // each limit. Unresolved names were already dropped by the native reader,
    // and out-of-range / `<= 0` tokens are silently ignored.
    let mut limits = group.nums.clone();
    if !group.extra_limit_types.is_empty() && !group.extra_limit_nums.is_empty() {
        let extras: Vec<&ObjectType> = group
            .extra_limit_types
            .iter()
            .filter_map(|name| resolve(name.as_str()))
            .collect();
        for (i, extra) in extras.iter().enumerate() {
            let mut count = owned_now(extra);
            if let Some(&max_count) = group.extra_limit_max_count.get(i) {
                if max_count > 0 {
                    count = count.min(max_count);
                }
            }
            let Some(&extra_num) = group.extra_limit_nums.get(i) else {
                continue;
            };
            if extra_num <= 0 {
                continue;
            }
            for limit in &mut limits {
                *limit = limit.wrapping_add(count.wrapping_mul(extra_num));
                if group.extra_limit_max_num > 0 {
                    *limit = (*limit).min(group.extra_limit_max_num);
                }
            }
        }
    }

    // Group contributions (`UpgradeCheck:845-861`): the native reader dropped
    // unresolved names, so the surviving members pair with `Nums` from the
    // front.
    let members: Vec<&ObjectType> = group
        .types
        .iter()
        .filter_map(|name| resolve(name.as_str()))
        .collect();
    Some(BuildLimitGroupCensus {
        limits,
        members: members.iter().map(|ty| ty.id.clone()).collect(),
        owned_now: members.iter().map(|ty| owned_now(ty)).collect(),
        owned_deploy: members.iter().map(|ty| owned_deploy(ty)).collect(),
        queued: members.iter().map(|ty| queued_num(ty)).collect(),
        in_production: members
            .iter()
            .map(|ty| sim.interner.get(&ty.id).is_some_and(held_in_factory))
            .collect(),
        member_factor: members
            .iter()
            .map(|ty| ty.phobos_build_limit_group.factor)
            .collect(),
        factor: group.factor,
        content_if_any: group
            .content_if_any_match
            .unwrap_or(rules.phobos_globals.build_limit_group_content_if_any_match),
        not_buildable: group.not_buildable_if_queue_match.unwrap_or(
            rules
                .phobos_globals
                .build_limit_group_not_buildable_if_queue_match,
        ),
    })
}

/// What `ReachedBuildLimit` answers and what it asks the queue owner to prune:
/// a non-negative entry means "drop that many queued copies of the queried
/// type", `-1` means every copy (`RemoveProduction @ Ext/House/Body.cpp:
/// 768-784` calling `FactoryClass::RemoveFromQueue @ 0x004CA620`).
#[cfg(feature = "phobos")]
struct BuildLimitGroupGate {
    reached: bool,
    removals: Vec<i32>,
}

/// `HouseExt::ReachedBuildLimit @ Ext/House/Body.cpp:1036-1188`, the gate
/// GreyCameo calls with `ignoreQueued=false`. It returns true on a
/// queue-inclusive reach; `NotBuildableIfQueueMatch` only widens the
/// `RemoveProduction` calls, never the return value, and the count is
/// `CountOwnedNow`. The multi-limit arm uses Phobos's
/// `owned * (Factor - limits[i])` formula (mathematically different from
/// `UpgradeCheck`'s `owned * Factor - limits[i]`; a Phobos defect kept here
/// because it is what the grey-out sees). Every `reached` arm carries the
/// trim native performs on the way out.
#[cfg(feature = "phobos")]
fn build_limit_group_gate(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> BuildLimitGroupGate {
    let unreached = || BuildLimitGroupGate {
        reached: false,
        removals: Vec::new(),
    };
    let group = &obj.phobos_build_limit_group;
    if group.types.is_empty() || group.nums.is_empty() {
        return unreached();
    }
    let Some(c) = build_limit_group_census(sim, rules, owner, obj) else {
        return unreached();
    };
    let factor = c.factor;
    if c.limits.len() == 1 {
        let mut count = 0i32;
        let mut queued = 0i32;
        let mut inside = false;
        for i in 0..c.members.len() {
            count = count.wrapping_add(c.owned_now[i].wrapping_mul(c.member_factor[i]));
            queued = queued.wrapping_add(c.queued[i].wrapping_mul(c.member_factor[i]));
            if c.members[i].eq_ignore_ascii_case(&obj.id) {
                inside = true;
            }
        }
        let num = count.wrapping_sub(c.limits[0]);
        if num.wrapping_add(queued) < 1i32.wrapping_sub(factor) {
            return unreached();
        }
        let removals = if inside {
            vec![
                num.wrapping_add(queued)
                    .wrapping_add(factor)
                    .wrapping_sub(1)
                    / factor,
            ]
        } else if num >= 1i32.wrapping_sub(factor) || c.not_buildable {
            vec![-1]
        } else {
            Vec::new()
        };
        return BuildLimitGroupGate {
            reached: true,
            removals,
        };
    }
    let size = c.limits.len().min(c.members.len());
    let mut reached = true;
    let mut real_reached = true;
    for i in 0..size {
        let queued = c.queued[i].wrapping_mul(c.member_factor[i]);
        let num = c.owned_now[i].wrapping_mul(c.member_factor[i].wrapping_sub(c.limits[i]));
        let item = c.members[i].eq_ignore_ascii_case(&obj.id);
        let with_queued = num.wrapping_add(queued);
        if item && with_queued >= 1i32.wrapping_sub(factor) {
            if c.content_if_any {
                let removals = if num >= 1i32.wrapping_sub(factor) || c.not_buildable {
                    vec![
                        num.wrapping_add(queued)
                            .wrapping_add(factor)
                            .wrapping_sub(1)
                            / factor,
                    ]
                } else {
                    Vec::new()
                };
                return BuildLimitGroupGate {
                    reached: true,
                    removals,
                };
            } else if num < 1i32.wrapping_sub(factor) {
                real_reached = false;
            }
        } else if !item && with_queued >= 0 {
            if c.content_if_any {
                // Native takes the same `-1` in both sub-arms.
                return BuildLimitGroupGate {
                    reached: true,
                    removals: vec![-1],
                };
            } else if num < 0 {
                real_reached = false;
            }
        } else {
            reached = false;
        }
    }
    if !reached {
        return unreached();
    }
    // `realReached || NotBuildableIfQueueMatch`: the trailing prune.
    BuildLimitGroupGate {
        reached: true,
        removals: if real_reached || c.not_buildable {
            vec![-1]
        } else {
            Vec::new()
        },
    }
}

/// The boolean half, for the read-only gates (`GreyCameo`, `CanBuild`).
#[cfg(feature = "phobos")]
fn build_limit_group_reached(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> bool {
    build_limit_group_gate(sim, rules, owner, obj).reached
}

/// `RemoveProduction @ Ext/House/Body.cpp:768-784`: resolve the queried type's
/// own factory, then drop `num` queued copies (`-1` = all it holds). The held
/// product stays; native's loop is a no-op once no queued copy is left.
#[cfg(feature = "phobos")]
fn phobos_remove_production(sim: &mut Simulation, owner: InternedId, obj: &ObjectType, num: i32) {
    let Some(type_id) = sim.interner.get(&obj.id) else {
        return;
    };
    let category = super::production_tech::production_category_for_object(obj);
    let total = sim
        .production
        .factories()
        .count_total(owner, category, type_id);
    let count = if num >= 0 { num.min(total) } else { total };
    for _ in 0..count {
        if !sim
            .production
            .factories_mut()
            .remove_one_queued(owner, category, type_id)
        {
            break;
        }
    }
}

/// The queue half of `ReachedBuildLimit`: native runs the trim inside the
/// grey query, which is read-only here, so the queue-admission owner applies
/// the same `RemoveProduction` calls before it inspects the limit
/// (`queue_production` in `factory_lifecycle.rs`). A gate that did not reach
/// trims nothing.
#[cfg(feature = "phobos")]
pub(crate) fn phobos_trim_build_limit_group_queue(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) {
    let gate = build_limit_group_gate(sim, rules, owner, obj);
    if !gate.reached {
        return;
    }
    for num in gate.removals {
        phobos_remove_production(sim, owner, obj, num);
    }
}

/// `HouseExt::BuildLimitGroupUpgradeCheck @ Ext/House/Body.cpp:802-1013`,
/// the check `HouseClass::CanBuild` reaches through
/// `HouseClass_CanBuild_UpgradesInteraction @ 0x4F8361` for a building type
/// with a non-empty `PowersUp.Buildings`. Returns true when the item must
/// not start production. The first pass ignores the item's own slot in the
/// multi-limit AND (Phobos's `realReached` assignments there are
/// unreachable), and the second pass treats any in-production copy as a
/// reason to stay buildable when `include_queued` is set.
#[cfg(feature = "phobos")]
fn build_limit_group_upgrade_check(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
    include_queued: bool,
) -> bool {
    // `Hooks.Upgrade.cpp:104-108`: the native replacement only runs for a
    // BuildingType whose `PowersUp_Buildings` is non-empty.
    if obj.category != ObjectCategory::Building
        || obj.building_upgrade().powers_up_buildings().is_empty()
    {
        return false;
    }
    let group = &obj.phobos_build_limit_group;
    // `UpgradeCheck:806` returns Buildable on empty Types; empty Nums with a
    // non-empty Types drops into the multi branch with `size = 0`, which is
    // `reached` and therefore `TemporarilyUnbuildable`.
    if group.types.is_empty() {
        return false;
    }
    let Some(c) = build_limit_group_census(sim, rules, owner, obj) else {
        return false;
    };
    if c.members.is_empty() {
        return false;
    }
    let factor = c.factor;
    let contributions: Vec<i32> = (0..c.members.len())
        .map(|i| c.owned_deploy[i].wrapping_mul(c.member_factor[i]))
        .collect();
    let item_at = |i: usize| c.members[i].eq_ignore_ascii_case(&obj.id);

    if c.limits.len() == 1 {
        let mut sum = 0i32;
        for contribution in &contributions {
            sum = sum.wrapping_add(*contribution);
        }
        if sum >= c.limits[0].wrapping_add(1).wrapping_sub(factor) {
            return true;
        }
    } else {
        let size = c.limits.len().min(contributions.len());
        let mut reached = true;
        for i in 0..size {
            let num = contributions[i].wrapping_sub(c.limits[i]);
            if item_at(i) {
                if num >= 1i32.wrapping_sub(factor) && c.content_if_any {
                    return true;
                }
            } else if num >= 0 {
                if c.content_if_any {
                    return true;
                }
            } else {
                reached = false;
            }
        }
        if reached {
            return true;
        }
    }

    // Second pass (`UpgradeCheck:949-1012`). With ContentIfAny one slot at
    // its threshold without a copy in production blocks; otherwise every
    // slot must be at its threshold and have no copy in production.
    let size = c.limits.len().min(c.members.len());
    if c.content_if_any {
        for i in 0..size {
            if contributions[i] >= c.limits[i].wrapping_add(1).wrapping_sub(factor)
                && (!include_queued || !c.in_production[i])
            {
                return true;
            }
        }
        return false;
    }
    if c.limits.len() == 1 {
        let mut sum = 0i32;
        for contribution in &contributions {
            sum = sum.wrapping_add(*contribution);
        }
        if sum >= c.limits[0].wrapping_add(1).wrapping_sub(factor) {
            let any_in_production = include_queued && c.in_production.iter().any(|&v| v);
            if !any_in_production {
                return true;
            }
        }
        return false;
    }
    for i in 0..size {
        let threshold = if item_at(i) {
            c.limits[i].wrapping_add(1).wrapping_sub(factor)
        } else {
            c.limits[i]
        };
        if contributions[i] < threshold {
            return false;
        }
        if include_queued && c.in_production[i] {
            return false;
        }
    }
    true
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
    //10020D43..10020D7A asks the limit-only CanBuild once, before scanning.
    // The first flag bypasses qualification and this factory query, so this
    // does not recurse through either admission gate.
    #[cfg(feature = "ares")]
    if require_can_build && can_build(sim, rules, owner, obj, true, true) != CanBuild::Yes {
        return FactorySearch::BuildLimitRefused;
    }
    let factory_type = FactoryType::for_category(obj.category);
    let game_mode_nonzero = sim.session.game_mode_nonzero();
    let ownable = get_ownable(obj, rules, game_mode_nonzero);
    let selling = MissionId::from_known(MissionType::Selling);
    let naval_unit = obj.category == ObjectCategory::Vehicle && obj.naval;
    let mut candidate = FactorySearch::Missing;
    for &id in house.base_projection().buildings() {
        let Some(building) = sim.substrate.entities.get(id) else {
            continue;
        };
        let Some(building_type) = sim.entity_object_type(building, rules) else {
            continue;
        };
        if !building.lifecycle.in_limbo()
            && building_type.factory_type() == Some(factory_type)
            && (crate::rules::ARES_ENABLED || !require_online || building.building_online())
            && building.mission.effective() != selling
            && building.mission.queued() != selling
            && (crate::rules::ARES_ENABLED
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
            if primary || (crate::rules::ARES_ENABLED && stop_first) {
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
