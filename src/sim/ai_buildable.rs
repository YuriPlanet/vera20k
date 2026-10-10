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

/// `1 << HouseTypeClass::FindIndexOfName(name)` for a house whose type (or
/// `ParentCountry=`, `HouseTypeClass+0x98`, which retail leaves at the type's
/// own name) is `country_name`: an unknown name answers -1, whose low five
/// bits make bit 31.
pub(crate) fn house_country_bit(rules: &RuleSet, country_name: &str) -> u32 {
    rules
        .trigger_house_type_index(country_name)
        .map_or(1 << 31, country_bit)
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
/// (`Rules+0xB3C`); null when no entry is ownable.
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

pub(crate) fn house_token_mask(tokens: &[String], rules: &RuleSet) -> u32 {
    tokens.iter().fold(0u32, |mask, token| {
        rules
            .trigger_house_type_index(token)
            .map_or(mask, |index| mask | country_bit(index))
    })
}

pub(crate) fn owner_allows(candidate: &ObjectType, country_bit: u32, rules: &RuleSet) -> bool {
    !candidate.owner.is_empty() && house_token_mask(&candidate.owner, rules) & country_bit != 0
}

/// Exact identity/shell tail of `HouseClass__FirstBuildableFromArray`.
///
/// gamemd-derived: `HouseClass__FirstBuildableFromArray @ 0x005051E0`.
/// `TechnoTypeClass` construction at `0x00711193` initializes Owner to zero;
/// reader block `0x007149E1..0x007149F5` preserves that missing-key default.
pub(crate) fn candidate_allowed(
    candidate: &ObjectType,
    country_bit: u32,
    side_index: u8,
    super_weapons: bool,
    rules: &RuleSet,
) -> bool {
    if !owner_allows(candidate, country_bit, rules) {
        return false;
    }
    if !candidate.required_houses.is_empty()
        && house_token_mask(&candidate.required_houses, rules) & country_bit == 0
    {
        return false;
    }
    if !candidate.forbidden_houses.is_empty()
        && house_token_mask(&candidate.forbidden_houses, rules) & country_bit != 0
    {
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
    if rules
        .build_tech_types
        .iter()
        .any(|type_id| type_id.eq_ignore_ascii_case(&candidate.id))
    {
        return true;
    }
    rules
        .super_weapon(primary)
        .is_some_and(|super_weapon| !super_weapon.disableable_from_shell)
}

/// Resolve the first passing BuildingType pointer from one authored AI list.
pub(crate) fn first_buildable_from_array<'a>(
    rules: &'a RuleSet,
    type_ids: &[String],
    country_name: &str,
    side_index: u8,
    super_weapons: bool,
) -> Option<&'a ObjectType> {
    let bit = country_bit(rules.trigger_house_type_index(country_name)?);
    type_ids.iter().find_map(|type_id| {
        let candidate = rules.object_in_category(ObjectCategory::Building, type_id)?;
        candidate_allowed(candidate, bit, side_index, super_weapons, rules).then_some(candidate)
    })
}
