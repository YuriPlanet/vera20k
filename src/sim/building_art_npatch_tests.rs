//! NPatch regressions for the Grand_Opening active-animation slots.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::house_state::HouseState;

/// NPatch (NPExt bug-fix changelog item 95, issue #2971): a tech building's
/// active animations start once a non-passive house owns it. Native fills
/// Idle18 + Active3..6 at first opening with no ownership arm
/// (`0x0044632E..0x00446374`), so an un-owned oil derrick pumps from
/// scenario placement; the capture's `Grand_Opening` re-run (Building
/// `0x00448CEF`) starts it, and one placed for a real house pumps at once.
#[test]
fn npatch_tech_derrick_pump_starts_on_capture_not_placement() {
    let art_ini = IniFile::from_str(
        "[CAOILD]\nActiveAnim=N\nActiveAnimDamaged=D\n\
         [CIVAN]\nActiveAnim=N\nActiveAnimDamaged=D\n\
         [N]\nLoopCount=-1\nRate=150\n[D]\nLoopCount=-1\nRate=300\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[General]\nVehicleParachute=VEHCHUTE\n\
             [BuildingTypes]\n0=CAOILD\n1=CIVAN\n\
             [CAOILD]\nStrength=1000\nNeedsEngineer=yes\n\
             [CIVAN]\nStrength=1000\n\
             [Animations]\n0=N\n1=D\n",
        ),
        &art_ini,
    )
    .unwrap();
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    for name in ["N", "D"] {
        art.bind_anim_frame_count_for_test(name, 40);
    }
    rules.install_art_data(art);

    let mut sim = Simulation::with_seed(3);
    let neutral = sim.interner.intern("Neutral");
    let americans = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(neutral, 0, None, false, 0, 10));
    sim.houses
        .register(HouseState::new(americans, 1, None, true, 0, 10));
    sim.houses
        .get_mut(&neutral)
        .unwrap()
        .make_multiplay_passive();

    let pump =
        |sim: &Simulation, id: u64| sim.entities().get(id).unwrap().building_art.anim_slots[3];

    let derrick = sim
        .spawn_object("CAOILD", "Neutral", 10, 10, 0, &rules)
        .unwrap();
    // Scenario placement (`discover_techno_in_views`'s authored-map call).
    sim.grand_opening(
        derrick,
        false,
        true,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert!(
        pump(&sim, derrick).is_none(),
        "uncaptured derrick stays still"
    );
    // Damage below yellow must not introduce the damaged pump either.
    sim.substrate
        .entities
        .get_mut(derrick)
        .unwrap()
        .health
        .set_for_test(250);
    sim.refresh_building_damage_state(derrick, &rules);
    assert!(
        pump(&sim, derrick).is_none(),
        "damage alone does not start the pump"
    );
    sim.substrate
        .entities
        .get_mut(derrick)
        .unwrap()
        .health
        .set_for_test(1000);
    sim.refresh_building_damage_state(derrick, &rules);
    // An engineer's capture through the ownership chokepoint starts it.
    sim.change_owner_with_rules(
        derrick,
        americans,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    let anim = pump(&sim, derrick).expect("capture starts the pump");
    assert_eq!(
        sim.interner.resolve(sim.anim(anim).unwrap().type_id()),
        "N",
        "the normal pump, not the damaged variant"
    );
    // Handed back to the passive house, it keeps that pump: no refill.
    sim.change_owner_with_rules(
        derrick,
        neutral,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(pump(&sim, derrick), Some(anim));

    // A derrick placed for a real house pumps from its first opening.
    let owned = sim
        .spawn_object("CAOILD", "Americans", 18, 10, 0, &rules)
        .unwrap();
    sim.grand_opening(
        owned,
        false,
        true,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert!(pump(&sim, owned).is_some(), "an owned derrick pumps");

    // The gate is scoped to `NeedsEngineer=`: a non-tech building still
    // animates from its first opening while neutral-owned.
    let civilian = sim
        .spawn_object("CIVAN", "Neutral", 14, 10, 0, &rules)
        .unwrap();
    sim.grand_opening(
        civilian,
        false,
        true,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert!(pump(&sim, civilian).is_some(), "non-tech keeps its anim");
}

/// NPatch (NPExt item 30, #2973): an owner change clears every attacker
/// whose Target still names the captured building, so the capture is not
/// demolished by the capturing side's own Tanya.
#[test]
fn npatch_capture_drops_attackers_that_still_target_the_building() {
    let mut rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=E1\n[BuildingTypes]\n0=GAPILE\n\
         [E1]\nStrength=125\n[GAPILE]\nStrength=1000\nArmor=wood\n\
         [CombatDamage]\nC4Warhead=SA\n",
    ))
    .unwrap();
    rules.set_npatch_for_test(true);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 41);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let building = sim
        .spawn_object("GAPILE", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let tanya = sim
        .spawn_object("E1", "Americans", 12, 10, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(tanya)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(building)));
    let americans = sim.interner.intern("Americans");
    sim.change_owner_with_rules(
        building,
        americans,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert!(
        sim.substrate
            .entities
            .get(tanya)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "the stale attacker target was cleared"
    );
}
