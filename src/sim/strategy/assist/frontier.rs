//! One construction queue, several bases. A second MCV is useful only when
//! the next refinery and a defensible production line stand beside its ore.
//! Choose a yard from known resources, never from the unobserved overlay grid.

use std::collections::BTreeMap;

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::advance::cell_distance;
use super::census::Census;
use super::plan::Item;

const DISTINCT_BASE: i32 = 22;
const OUTPOST_RADIUS: i32 = 13;
const ORE_SERVICE_RADIUS: i32 = 12;

/// An outpost that has a reachable strategic reason to exist. The refineries
/// are counted per base, not globally: three at home do not unload an ore
/// carrier that must drive across the map from the new yard.
pub(super) fn remote_yard(
    census: &Census,
    known: &BTreeMap<(u16, u16), u8>,
) -> Option<(u16, u16)> {
    let first = *census.yard_cells.first()?;
    census.yard_cells.iter().skip(1).copied()
        .filter(|yard| cell_distance(*yard, first) >= DISTINCT_BASE)
        .filter(|yard| known.keys().any(|ore| {
            cell_distance(*yard, *ore) <= ORE_SERVICE_RADIUS
        }))
        .min_by_key(|yard| {
            (census.refinery_cells.iter()
                .filter(|bay| cell_distance(**bay, *yard) <= OUTPOST_RADIUS)
                .count(), *yard)
        })
}

fn local_count(
    rules: &RuleSet,
    obs: &Observation<'_>,
    center: (u16, u16),
    predicate: impl Fn(&ObjectType) -> bool,
) -> usize {
    obs.own_entities().filter(|entity| cell_distance(entity.cell, center) <= OUTPOST_RADIUS)
        .filter(|entity| obs.object_type(rules, entity.type_id)
            .is_some_and(&predicate))
        .count()
}

/// A bounded, rule-driven build request. Only cash that is not already
/// committed to emergency income repair can establish new outpost facilities.
pub(super) fn needed(
    rules: &RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    known: &BTreeMap<(u16, u16), u8>,
    free: i32,
) -> Option<(Item, (u16, u16))> {
    let yard = remote_yard(census, known)?;
    let count = |pred: fn(&ObjectType) -> bool| local_count(rules, obs, yard, pred);
    if count(|ty| ty.resource_destination) == 0 {
        return Some((Item::Refinery, yard));
    }
    if free >= 3_000 && count(|ty| ty.power > 0) == 0 {
        return Some((Item::Power, yard));
    }
    if free >= 4_500 && count(|ty| {
        ty.factory_type() == Some(crate::rules::object_type::FactoryType::UnitType)
    }) == 0 {
        return Some((Item::Weapons, yard));
    }
    if free >= 4_000 && count(|ty| ty.category == ObjectCategory::Building && ty.is_base_defense) == 0 {
        return Some((Item::Defense, yard));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outpost_needs_known_ore_and_a_distinct_yard() {
        let mut census = Census::default();
        census.yard_cells = vec![(10, 10), (70, 70)];
        let mut known = BTreeMap::new();
        assert_eq!(remote_yard(&census, &known), None);
        known.insert((72, 70), 0);
        assert_eq!(remote_yard(&census, &known), Some((70, 70)));
        census.yard_cells[1] = (18, 18);
        assert_eq!(remote_yard(&census, &known), None);
    }

    #[test]
    fn existing_refineries_at_home_do_not_supply_remote_ore() {
        let mut census = Census::default();
        census.yard_cells = vec![(10, 10), (70, 70)];
        census.refinery_cells = vec![(14, 10), (11, 14), (18, 12)];
        let mut known = BTreeMap::new();
        known.insert((72, 70), 4);
        assert_eq!(remote_yard(&census, &known), Some((70, 70)));
        assert!(census.refinery_cells.iter().all(|bay| cell_distance(*bay, (70,70)) > OUTPOST_RADIUS));
    }
}
