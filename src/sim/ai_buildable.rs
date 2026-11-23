//! Shared native AI-list type identity predicates.
//!
//! Naval placement and BasePlan Recalc both call the same HouseClass helper;
//! this module keeps that predicate separate from generic production gates,
//! with the house country bit and the harvester lookup the computer's
//! choosers share.

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::{CountryIdx, RuleSet};

pub(crate) fn country_bit(index: CountryIdx) -> u32 {
    1u32.wrapping_shl(u32::from(index.0 & 31))
}

/// Actual HouseType array index (`+B8`) for RequiredHouses/ForbiddenHouses,
/// e.g. CanBuild4F79F3/4F7A82. This is an identity, not a Trigger token or
/// the distinct ParentCountry lookup used by Owner gates.
pub fn house_country_bit(rules: &RuleSet, country_name: &str) -> u32 {
    rules
        .country_index(country_name)
        .map_or(1 << 31, country_bit)
}

/// Read-only inputs for FirstBuildable5051E0 and Recalc5054B0: Owner uses
/// Country+98, while Required/Forbidden use the actual Country+B8. This is
/// derived once per choice; the retained Rules registry owns both identities.
#[derive(Debug, Clone, Copy)]
pub struct CountryBuildMasks {
    owner: u32,
    actual: u32,
}

impl CountryBuildMasks {
    pub fn for_country(rules: &RuleSet, country: &str) -> Self {
        Self {
            owner: rules.country_owner_mask(country),
            actual: house_country_bit(rules, country),
        }
    }

    pub(crate) fn owner_bit(self) -> u32 {
        self.owner
    }
}

/// The first `[General] HarvesterUnit=` type (`Rules+0xB3C`) whose `Owner=`
/// (`OwnerFlags`, `TechnoType+0x6CC`) holds `country_bit`: the loops of
/// BasePlan Recalc, `AI_Choose_Unit @ 0x004FEA60` and the house's chooser
/// dispatch (`0x004F90F7..0x004F913F`).
pub(crate) fn first_owner_compatible_harvester(
    rules: &RuleSet,
    country_bit: u32,
) -> Option<&ObjectType> {
    first_owner_compatible(
        rules,
        &rules.harvester_unit_types,
        ObjectCategory::Vehicle,
        country_bit,
    )
}

/// `HouseClass::FirstOwnableFromArray @ 0x00505310`: the first type of one
/// `RulesClass` type vector whose `Owner=` (`OwnerFlags`, `TechnoType+0x6CC`,
/// `0x00505330..0x0050534E`) holds the house's country bit
/// (`1 << HouseTypeClass::FindIndexByName`, `0x00505316`). The crate pickup
/// asks it for `BaseUnit=` (`Rules+0xB20`) and `HarvesterUnit=`
/// (`Rules+0xB3C`), the multiplayer base-unit spawn for `BaseUnit=`
/// (`0x005D7059`); null when no entry is ownable. The AI's harvester and
/// base-unit checks inline the same loop (e.g. `0x004F6540`, `0x004FEA60`).
pub(crate) fn first_owner_compatible<'a>(
    rules: &'a RuleSet,
    type_ids: &[String],
    category: ObjectCategory,
    country_bit: u32,
) -> Option<&'a ObjectType> {
    type_ids.iter().find_map(|type_id| {
        let candidate = rules.object_in_category(category, type_id)?;
        owner_allows(candidate, country_bit, rules).then_some(candidate)
    })
}

pub(crate) fn owner_allows(candidate: &ObjectType, country_bit: u32, rules: &RuleSet) -> bool {
    !candidate.owner.is_empty() && candidate.owner_mask(rules) & country_bit != 0
}

/// Exact identity/shell tail of `HouseClass__FirstBuildableFromArray`.
///
/// gamemd-derived: `HouseClass__FirstBuildableFromArray @ 0x005051E0`.
/// `TechnoTypeClass` construction at `0x00711193` initializes Owner to zero;
/// reader block `0x007149E1..0x007149F5` preserves that missing-key default.
/// sim-bench also asks it which of a side's types a country may own.
pub fn candidate_allowed(
    candidate: &ObjectType,
    country: CountryBuildMasks,
    side_index: i32,
    super_weapons: bool,
    rules: &RuleSet,
) -> bool {
    if !owner_allows(candidate, country.owner, rules) {
        return false;
    }
    let required = candidate.required_houses_mask(rules);
    if required != u32::MAX && required & country.actual == 0 {
        return false;
    }
    // FirstBuildable505259 skips ForbiddenHouses==-1, including a
    // nonempty authored list whose parsed OR-mask is all bits set.
    let forbidden = candidate.forbidden_houses_mask(rules);
    if forbidden != u32::MAX && forbidden & country.actual != 0 {
        return false;
    }
    if !candidate.planned_for_side(side_index) {
        return false;
    }
    if super_weapons {
        return true;
    }
    let Some(primary) = candidate.super_weapon.as_deref() else {
        return true;
    };
    if rules.is_build_tech(&candidate.id) {
        return true;
    }
    rules
        .super_weapon(primary)
        .is_some_and(|super_weapon| !super_weapon.disableable_from_shell)
}

/// Whether `country` may own and build `candidate` outside the AI's base-plan
/// chooser: the `Owner=`/`RequiredHouses=`/`ForbiddenHouses=` gates, the
/// `DoubleOwned=` multiplayer override and the side's `AIBasePlanningSide`.
///
/// The load-time render preload sizes its voxel bake from this. It
/// deliberately ignores TechLevel, prerequisites and the shell super-weapon
/// switch, so the answer is a superset of what the country ends up building:
/// baking one model too many only costs load time, while one too few costs a
/// mid-match atlas refresh.
pub fn producible_by_country(
    candidate: &ObjectType,
    country: CountryBuildMasks,
    side_index: i32,
    game_mode_nonzero: bool,
    rules: &RuleSet,
) -> bool {
    // Get_Ownable711EC0: a DoubleOwned type belongs to every house outside a
    // campaign, regardless of its authored Owner= mask.
    if !(candidate.double_owned && game_mode_nonzero)
        && !owner_allows(candidate, country.owner, rules)
    {
        return false;
    }
    if !candidate.required_houses.is_empty()
        && rules.house_type_mask(&candidate.required_houses) & country.actual == 0
    {
        return false;
    }
    let forbidden = rules.house_type_mask(&candidate.forbidden_houses);
    if forbidden != u32::MAX && forbidden & country.actual != 0 {
        return false;
    }
    candidate.planned_for_side(side_index)
}

/// One Country that actually plays the match, with the masks its production
/// gates ask for.
pub struct ParticipatingCountry<'a> {
    pub name: &'a str,
    pub masks: CountryBuildMasks,
    pub side_index: i32,
}

/// Every distinct Country named by a House that is still in the match.
///
/// A passive House (Neutral, Civilian) sits the match out, and a defeated one
/// builds nothing more. Houses that share a Country are read once, so a retail
/// map with several Houses per faction does not repeat the roster scan.
/// Registration order is the Houses' own, which keeps the caller's bake order
/// stable across runs.
pub fn participating_countries<'a>(
    houses: &crate::sim::houses::Houses,
    rules: &RuleSet,
    interner: &'a crate::sim::intern::StringInterner,
) -> Vec<ParticipatingCountry<'a>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for house in houses.values() {
        if house.multiplay_passive() || house.is_defeated() {
            continue;
        }
        let country = interner.resolve(house.house_type_id());
        if !seen.insert(country) {
            continue;
        }
        // An unregistered Country cannot pass any Owner= gate; its House would
        // never build from a rules roster.
        let Some(side_index) = rules.country_side_index_raw(country) else {
            continue;
        };
        out.push(ParticipatingCountry {
            name: country,
            masks: CountryBuildMasks::for_country(rules, country),
            side_index,
        });
    }
    out
}

/// The non-voxel types the participating Houses can put on the field,
/// deduplicated: infantry bodies, building hulls and any vehicle or aircraft
/// whose ART row turns its body back into an SHP.
///
/// This feeds the sprite-atlas preload rather than the voxel atlas. A match
/// otherwise opens a growth page for its first power plant seconds after the
/// start, and again for every defense, infantry type or building it later
/// fields for the first time.
pub fn producible_shp_types<'a>(
    houses: &crate::sim::houses::Houses,
    rules: &'a RuleSet,
    interner: &crate::sim::intern::StringInterner,
    game_mode_nonzero: bool,
) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for country in participating_countries(houses, rules, interner) {
        for type_id in producible_shp_types_for_country(
            country.masks,
            country.side_index,
            rules,
            game_mode_nonzero,
        ) {
            if !out.contains(&type_id) {
                out.push(type_id);
            }
        }
    }
    out
}

/// One Country's share of [`producible_shp_types`], in rules category order.
pub fn producible_shp_types_for_country<'a>(
    masks: CountryBuildMasks,
    side_index: i32,
    rules: &'a RuleSet,
    game_mode_nonzero: bool,
) -> Vec<&'a str> {
    use crate::rules::object_type::ObjectCategory;
    let mut out: Vec<&str> = Vec::new();
    for type_id in rules
        .infantry_ids
        .iter()
        .chain(&rules.building_ids)
        .chain(&rules.vehicle_ids)
        .chain(&rules.aircraft_ids)
    {
        let Some(candidate) = rules.object(type_id) else {
            continue;
        };
        if !producible_by_country(
            candidate,
            masks,
            side_index,
            game_mode_nonzero,
            rules,
        ) {
            continue;
        }
        // A vehicle or aircraft defaults to a voxel body; only an explicit ART
        // row can take it back to SHP. Buildings and infantry are SHP unless
        // the same row says otherwise.
        let voxel = rules
            .art()
            .resolve_metadata_entry(type_id, &candidate.image)
            .map(|entry| entry.voxel)
            .unwrap_or(matches!(
                candidate.category,
                ObjectCategory::Vehicle | ObjectCategory::Aircraft
            ));
        if !voxel {
            out.push(type_id);
        }
    }
    out
}

/// Resolve the first passing BuildingType pointer from one authored AI list.
/// Ares 3.0 HouseClass_FirstBuildableFromArray 100238E0 (hook `0x005051E0`)
/// replaces the body with HouseExt::FindBuildable: CanExpectToBuild with
/// the parent country's Owner bit and the shell-disabled test, the same
/// [`candidate_allowed`] gates.
pub(crate) fn first_buildable_from_array<'a>(
    rules: &'a RuleSet,
    type_ids: &[String],
    country_name: &str,
    side_index: i32,
    super_weapons: bool,
) -> Option<&'a ObjectType> {
    // VERA-only: native scans a country outside [Countries] with bit 31/30
    // and almost always finds nothing.
    rules.country_index(country_name)?;
    let country = CountryBuildMasks::for_country(rules, country_name);
    type_ids.iter().find_map(|type_id| {
        let candidate = rules.object_in_category(ObjectCategory::Building, type_id)?;
        candidate_allowed(candidate, country, side_index, super_weapons, rules).then_some(candidate)
    })
}

#[cfg(all(test, tests_sim_a_b))]
#[path = "ai_buildable_parent_tests.rs"]
mod parent_tests;
