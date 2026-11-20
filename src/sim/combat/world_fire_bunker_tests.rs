//! Phobos `BunkerROFMultMultiplier` through the live bunker link and
//! `snapshot_get_rof`'s multiplier selection.

use super::*;

/// Phobos `TechnoClass_RearmDelay_BuildingBunkerROFMult` (`0x6FD1C7`,
/// `Ext/BuildingType/Hooks.cpp:332-345`): the bunker building type's
/// `BunkerROFMultMultiplier` wins over the `[CombatDamage]` global for the
/// bunkered unit's GetROF.
#[test]
fn the_bunker_building_type_overrides_the_rof_multiplier() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n\
             [VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\nPrimary=GUN\n\
             [BuildingTypes]\n0=BUNK\n\
             [BUNK]\nStrength=1000\nFoundation=1x1\nBunkerROFMultMultiplier=2.5\n\
             [Weapons]\n0=GUN\n\
             [GUN]\nDamage=5\nROF=100\nRange=5\nWarhead=WH\nProjectile=PIFF\n\
             [Warheads]\n0=WH\n[WH]\n[PIFF]\nInviso=yes\n",
        ))
        .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let tank = sim
        .spawn_object("TANK", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let bunk = sim
        .spawn_object("BUNK", "Americans", 7, 7, 0, &rules)
        .unwrap();
    assert_eq!(
        phobos_bunker_rof_multiplier(
            &sim.substrate,
            sim.substrate.entities.get(tank),
            &rules,
            &sim.interner
        ),
        None,
        "an unlinked unit keeps the [CombatDamage] global"
    );
    crate::sim::docking::bunker_link::install_bunker_link(&mut sim, bunk, tank, &rules);
    assert_eq!(
        phobos_bunker_rof_multiplier(
            &sim.substrate,
            sim.substrate.entities.get(tank),
            &rules,
            &sim.interner
        ),
        Some(2.5),
        "the bunker building type overrides the global"
    );
}
