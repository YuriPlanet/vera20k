//! The player's build options and production checks, asked of the one
//! CanBuild owner ([`super::can_build`]), and factory matching and spawn
//! cell logic.

use crate::map::entities::EntityCategory;
use crate::rules::foundation::foundation_dimensions;
use crate::rules::object_type::{BuildCategory, FactoryType, ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

use super::can_build::{CanBuild, can_build, check_build_limit, find_factory};
use super::production_types::*;

/// What the player's sidebar shows for `type_id`, from [`sidebar_state`].
/// No money check: a build starts without the funds and stalls its steps
/// instead (`SelectClass::Action @ 0x006AAD00` and `HouseClass::Begin_Production
/// @ 0x004FA350` test none).
pub(super) fn build_option_for_owner(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
) -> Option<BuildOption> {
    let obj = rules.object(type_id)?;
    let reason = match sim.interner.get(owner) {
        Some(owner_id) => sidebar_state(sim, rules, owner_id, obj),
        None => Some(BuildDisabledReason::NoFactory),
    };
    let type_interned = sim.interner.get(type_id).unwrap_or_default();
    let cost = match sim.interner.get(owner) {
        Some(owner_id) => sim.cost_of(owner_id, obj, rules),
        None => rules.cost_of(obj, None),
    };
    Some(BuildOption {
        type_id: type_interned,
        display_name: obj.name.clone().unwrap_or_else(|| obj.id.clone()),
        cost,
        object_category: obj.category,
        queue_category: production_category_for_object(obj),
        enabled: reason.is_none(),
        reason,
    })
}

/// Whether the player's strip keeps a cameo of `obj` (`StripClass::Recalculate
/// @ 0x006AA600`, `0x006AA74D..0x006AA78C`): a building of the house could
/// build it (`FindFactory(1, 0, 0)`) and that building's house may
/// (`HouseClass::CanBuild(type, 0, 1)` is not 0). `BuildingClass::
/// UpdateConstructionOptions @ 0x004456D0` adds the cameos with the same
/// CanBuild call.
pub(super) fn strip_keeps(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> bool {
    strip_refusal(sim, rules, owner, obj).is_none()
}

/// Which of [`strip_keeps`]'s two tests drops `obj` from the strip.
fn strip_refusal(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> Option<BuildDisabledReason> {
    if find_factory(sim, rules, owner, obj, true, false, false).is_none() {
        Some(BuildDisabledReason::NoFactory)
    } else if can_build(sim, rules, owner, obj, false, true) == CanBuild::No {
        Some(BuildDisabledReason::CannotBuild)
    } else {
        None
    }
}

/// Whether the sidebar lists `obj` for `owner` ([`strip_keeps`]), and, if it
/// does, why its cameo is darkened: `StripClass::Draw @ 0x006A9540`
/// (`0x006A97A8..0x006A97F9`) darkens it while no online factory may build
/// it (`FindFactory(1, 1, 1)`), or while it is at its limit
/// (`CanBuild(type, 0, 0)` answers -1, or [`check_build_limit`]).
///
/// Residual: the strip adds cameos only when the house's tech tree is
/// rechecked (House `+0x1FC`, `HouseClass::Update @ 0x004F926C`), and
/// `UpdateConstructionOptions` then asks only powered, discovered factories
/// of the type's kind; VERA lists a type as soon as it qualifies. Trigger:
/// a new option while every factory of its kind is unpowered. Effect: VERA
/// shows the cameo before gamemd does. Rare.
fn sidebar_state(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> Option<BuildDisabledReason> {
    if let Some(refusal) = strip_refusal(sim, rules, owner, obj) {
        return Some(refusal);
    }
    if find_factory(sim, rules, owner, obj, true, true, true).is_none() {
        return Some(BuildDisabledReason::NoReadyFactory);
    }
    if can_build(sim, rules, owner, obj, false, false) == CanBuild::AtLimit
        || check_build_limit(sim, rules, owner, obj)
    {
        return Some(BuildDisabledReason::AtBuildLimit);
    }
    None
}

/// Every rules type's build option for `owner`, by category, including rejected
/// options. Retained sidebar insertions need current comparison keys before
/// Recalculate removes an entry; admission-only consumers filter the same query.
pub(crate) fn all_build_options_for_owner(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
) -> Vec<BuildOption> {
    let mut out: Vec<BuildOption> = Vec::new();
    for id in &rules.building_ids {
        if let Some(opt) = build_option_for_owner(sim, rules, owner, id) {
            out.push(opt);
        }
    }
    for id in &rules.infantry_ids {
        if let Some(opt) = build_option_for_owner(sim, rules, owner, id) {
            out.push(opt);
        }
    }
    for id in &rules.vehicle_ids {
        if let Some(opt) = build_option_for_owner(sim, rules, owner, id) {
            out.push(opt);
        }
    }
    for id in &rules.aircraft_ids {
        if let Some(opt) = build_option_for_owner(sim, rules, owner, id) {
            out.push(opt);
        }
    }
    out.sort_by_key(|opt| opt.queue_category);
    out
}

pub(super) fn production_category_for_object(
    obj: &crate::rules::object_type::ObjectType,
) -> ProductionCategory {
    match obj.category {
        ObjectCategory::Infantry => ProductionCategory::Infantry,
        ObjectCategory::Vehicle if obj.naval => ProductionCategory::Ship,
        ObjectCategory::Vehicle => ProductionCategory::Vehicle,
        ObjectCategory::Aircraft => ProductionCategory::Aircraft,
        ObjectCategory::Building => match obj.build_cat {
            Some(BuildCategory::Combat) => ProductionCategory::Defense,
            _ => ProductionCategory::Building,
        },
    }
}

/// Check if a structure is a production factory for the given category.
///
/// Uses the data-driven Factory= key from rules.ini via RuleSet.factory_map.
/// A building with `Factory=InfantryType` produces infantry, `Factory=UnitType`
/// produces vehicles, etc. Buildings without Factory= are never factories.
pub(super) fn is_production_factory(
    rules: &RuleSet,
    structure_id: &str,
    category: ProductionCategory,
) -> bool {
    let Some(factory_type) = rules.factory_type(structure_id) else {
        return false;
    };
    match category {
        ProductionCategory::Infantry => factory_type == FactoryType::InfantryType,
        ProductionCategory::Vehicle => {
            factory_type == FactoryType::UnitType
                && rules
                    .object(structure_id)
                    .is_some_and(|object| !object.naval)
        }
        ProductionCategory::Ship => {
            factory_type == FactoryType::UnitType
                && rules
                    .object(structure_id)
                    .is_some_and(|object| object.naval)
        }
        ProductionCategory::Aircraft => factory_type == FactoryType::AircraftType,
        ProductionCategory::Building | ProductionCategory::Defense => {
            factory_type == FactoryType::BuildingType
        }
    }
}

pub fn producer_candidates_for_owner_category(
    entities: &EntityStore,
    rules: &RuleSet,
    owner: &str,
    category: ProductionCategory,
    require_matching_factory: bool,
    interner: &crate::sim::intern::StringInterner,
) -> Vec<(u64, u16, u16, String)> {
    let mut preferred_factories: Vec<(u64, u16, u16, String)> = Vec::new();
    for e in entities.values() {
        // A Dying factory corpse must not be selected as a unit's producer/exit.
        if e.dying {
            continue;
        }
        if e.lifecycle.in_limbo {
            continue;
        }
        if !interner.resolve(e.owner()).eq_ignore_ascii_case(owner) {
            continue;
        }
        if e.category() != EntityCategory::Structure {
            continue;
        }
        if e.building_up() {
            continue;
        }
        let type_ref_str = interner.resolve(e.type_ref());
        let is_match = is_production_factory(rules, type_ref_str, category);
        if require_matching_factory && !is_match {
            continue;
        }
        if !require_matching_factory || is_match {
            preferred_factories.push((
                e.stable_id(),
                e.position.rx,
                e.position.ry,
                type_ref_str.to_string(),
            ));
        }
    }
    preferred_factories.sort_by_key(|a| a.0);
    preferred_factories
}

pub fn is_matching_factory(
    rules: &RuleSet,
    structure_id: &str,
    produced_category: ObjectCategory,
) -> bool {
    match produced_category {
        ObjectCategory::Infantry => {
            is_production_factory(rules, structure_id, ProductionCategory::Infantry)
        }
        ObjectCategory::Vehicle => {
            is_production_factory(rules, structure_id, ProductionCategory::Vehicle)
                || is_production_factory(rules, structure_id, ProductionCategory::Ship)
        }
        ObjectCategory::Aircraft => {
            is_production_factory(rules, structure_id, ProductionCategory::Aircraft)
        }
        ObjectCategory::Building => {
            is_production_factory(rules, structure_id, ProductionCategory::Building)
        }
    }
}

/// Returns the base foundation cells for normal building occupancy.
///
/// gamemd keeps these cells separate from `AddOccupy`/`RemoveOccupy`, which only
/// adjust hidden occupancy counters behind `CanHideThings`.
pub fn building_base_foundation_cells(
    origin_rx: u16,
    origin_ry: u16,
    foundation: &str,
) -> Vec<(u16, u16)> {
    use std::collections::BTreeSet;
    let (w, h) = foundation_dimensions(foundation);
    let mut cells: BTreeSet<(u16, u16)> = BTreeSet::new();

    for dx in 0..w {
        for dy in 0..h {
            let rx = origin_rx as i32 + dx as i32;
            let ry = origin_ry as i32 + dy as i32;
            if rx >= 0 && rx <= u16::MAX as i32 && ry >= 0 && ry <= u16::MAX as i32 {
                cells.insert((rx as u16, ry as u16));
            }
        }
    }

    cells.into_iter().collect()
}

/// Cells that block static grid movement, given the base foundation and whether
/// the building has a bib (`Bib=yes` in rules.ini).
///
/// For non-bib buildings, this is just the base foundation. For `Bib=yes`
/// buildings, the east-edge column of the foundation is excluded: those cells
/// are unit-passable in the original engine via the per-occupant-chain bib
/// relaxation in `Can_Enter_Cell` (probes the east neighbor; if it isn't part
/// of the same building, the building stops blocking the cell).
///
/// "East edge" = any cell in `base_foundation` whose east neighbor `(x+1, y)`
/// is outside the base foundation.
///
/// `NumberImpassableRows` is deliberately not applied here. That field is a
/// live `UnitClass::Can_Enter_Cell` object-list skip (`0x0073F57C..5A2` radio
/// contact, `0x0073F74B..774` UnitRepair/Bunker through `0x00458A00`), which
/// the Unit Find_Path search reaches through `Simulation::foot_can_enter`; it
/// is not static terrain data.
pub fn building_movement_blocking_cells(
    base_foundation: &[(u16, u16)],
    has_bib: bool,
) -> Vec<(u16, u16)> {
    use std::collections::BTreeSet;
    if !has_bib {
        return base_foundation.to_vec();
    }
    let set: BTreeSet<(u16, u16)> = base_foundation.iter().copied().collect();
    base_foundation
        .iter()
        .copied()
        .filter(|&(x, y)| x.checked_add(1).is_some_and(|nx| set.contains(&(nx, y))))
        .collect()
}

#[cfg(test)]
mod footprint_tests {
    use super::*;

    #[test]
    fn rectangle_only_4x3() {
        let cells = building_base_foundation_cells(10, 20, "4x3");
        assert_eq!(cells.len(), 12);
        assert!(cells.contains(&(10, 20)));
        assert!(cells.contains(&(13, 22)));
    }

    #[test]
    fn garefn_base_foundation_ignores_hidden_occupy_modifiers() {
        // GAREFN: Foundation=4x3, AddOccupy1=-1,0, AddOccupy2=-1,-1, RemoveOccupy1=3,1
        let cells = building_base_foundation_cells(10, 20, "4x3");
        assert_eq!(cells.len(), 12);
        assert!(cells.contains(&(13, 21)));
        assert!(!cells.contains(&(9, 19)));
        assert!(!cells.contains(&(9, 20)));
    }

    #[test]
    fn narefn_base_foundation_keeps_dock_pad_despite_remove_occupy() {
        let cells = building_base_foundation_cells(10, 20, "4x3");
        assert_eq!(cells.len(), 12);
        assert!(cells.contains(&(13, 21)));
        assert!(!cells.contains(&(8, 20)));
        assert!(!cells.contains(&(8, 19)));
        assert!(!cells.contains(&(8, 18)));
    }

    #[test]
    fn movement_blocking_no_bib_keeps_full_footprint() {
        let footprint = building_base_foundation_cells(10, 20, "4x3");
        let blocking = building_movement_blocking_cells(&footprint, false);
        assert_eq!(blocking.len(), footprint.len());
    }

    #[test]
    fn movement_blocking_with_bib_drops_east_edge_rectangle() {
        // Plain 4x3 with bib → east column (x = 13) drops.
        let footprint = building_base_foundation_cells(10, 20, "4x3");
        let blocking = building_movement_blocking_cells(&footprint, true);
        // 12 base cells - 3 east-edge column (13, 20), (13, 21), (13, 22) = 9.
        assert_eq!(blocking.len(), 9);
        assert!(!blocking.contains(&(13, 20)));
        assert!(!blocking.contains(&(13, 21)));
        assert!(!blocking.contains(&(13, 22)));
        assert!(blocking.contains(&(12, 20)));
        assert!(blocking.contains(&(10, 22)));
    }

    #[test]
    fn movement_blocking_with_bib_garefn_uses_base_foundation_topology() {
        let footprint = building_base_foundation_cells(10, 20, "4x3");
        let blocking = building_movement_blocking_cells(&footprint, true);
        assert_eq!(blocking.len(), 9);
        assert!(!blocking.contains(&(9, 19)));
        assert!(!blocking.contains(&(9, 20)));
        assert!(blocking.contains(&(12, 21)));
        assert!(!blocking.contains(&(13, 20)));
        assert!(!blocking.contains(&(13, 21)));
        assert!(blocking.contains(&(10, 20)));
    }

    #[test]
    fn static_movement_blocking_keeps_number_rows_out_of_the_grid() {
        let footprint = building_base_foundation_cells(10, 20, "4x3");
        let blocking = building_movement_blocking_cells(&footprint, false);
        assert_eq!(blocking.len(), 12);
        assert!(blocking.contains(&(12, 21)));
        assert!(blocking.contains(&(13, 21)));
    }
}
