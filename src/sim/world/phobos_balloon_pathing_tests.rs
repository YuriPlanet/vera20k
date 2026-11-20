//! `BalloonHoverPathingFix` (`Misc/Hooks.BugFixes.cpp:3499-3547`): the two
//! cell-admission bypasses, against VERA's `foot_can_enter` and
//! `approach_admits` owners.
use super::*;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;

fn rules(key: bool) -> RuleSet {
    let mut rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=ZEP\n1=TANK\n\
         [ZEP]\nBalloonHover=yes\nStrength=100\nSpeed=8\nSight=10\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [TANK]\nStrength=300\nArmor=heavy\nSpeed=6\nSight=10\n",
    ))
    .expect("balloon rules");
    rules.phobos_globals.balloon_hover_pathing_fix = key;
    rules
}

fn scene(
    key: bool,
    balloon: bool,
    airborne: bool,
    occupied: bool,
) -> (Simulation, RuleSet, u64, (u16, u16)) {
    let rules = rules(key);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4D62C0);
    let kind = if balloon { "ZEP" } else { "TANK" };
    let actor = sim
        .spawn_object_at_height(kind, "Americans", 10, 10, 0, 0, &rules)
        .expect("actor spawns");
    if airborne {
        Simulation::aircraft_tracker_add(&sim.session, &mut sim.substrate, actor);
    }
    if occupied {
        let occupant = sim
            .spawn_object_at_height("TANK", "Russians", 20, 10, 0, 0, &rules)
            .expect("occupant spawns");
        assert_ne!(occupant, actor);
    }
    (sim, rules, actor, (20, 10))
}

#[test]
fn balloon_hover_pathing_fix_lets_an_airborne_balloon_enter_an_occupied_cell() {
    let (sim, rules, balloon, _) = scene(true, true, true, true);
    let cell = Cell::Real(10 * 32 + 20);
    assert_eq!(
        sim.foot_can_enter(balloon, cell, InfantryEntryArgs::REPAIR, &rules),
        Ok(0)
    );

    let (sim, rules, balloon, _) = scene(false, true, true, true);
    assert_ne!(
        sim.foot_can_enter(balloon, cell, InfantryEntryArgs::REPAIR, &rules),
        Ok(0)
    );
}

#[test]
fn balloon_hover_pathing_fix_admits_an_airborne_balloons_approach_cell() {
    let (mut sim, rules, balloon, cell) = scene(true, true, true, true);
    assert_eq!(
        sim.phobos_balloon_approach_admits_for_test(
            balloon,
            (cell.0 as i16, cell.1 as i16),
            &rules
        ),
        Ok(true)
    );

    let (mut sim, rules, balloon, cell) = scene(false, true, true, true);
    assert_eq!(
        sim.phobos_balloon_approach_admits_for_test(
            balloon,
            (cell.0 as i16, cell.1 as i16),
            &rules
        ),
        Ok(false)
    );
}

#[test]
fn balloon_hover_pathing_fix_requires_an_airborne_balloon_type() {
    let (mut sim, rules, grounded, cell) = scene(true, true, false, true);
    assert_ne!(
        sim.foot_can_enter(
            grounded,
            Cell::Real(10 * 32 + 20),
            InfantryEntryArgs::REPAIR,
            &rules
        ),
        Ok(0)
    );
    assert_eq!(
        sim.phobos_balloon_approach_admits_for_test(
            grounded,
            (cell.0 as i16, cell.1 as i16),
            &rules
        ),
        Ok(false)
    );

    let (sim, rules, tank, _) = scene(true, false, true, true);
    assert_ne!(
        sim.foot_can_enter(
            tank,
            Cell::Real(10 * 32 + 20),
            InfantryEntryArgs::REPAIR,
            &rules
        ),
        Ok(0)
    );
}
