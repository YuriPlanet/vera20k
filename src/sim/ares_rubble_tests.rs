//! Ares advanced rubble: `BuildingTypeExt::SwitchRubble @ 1000F2B0` from the
//! destruction hook (`0x00441F12`, `10017160`) and the engineer's rebuild
//! (`0x00519FAF`, `100257A0`), and `EngineerRepairable` (`10014850`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{EngineerBuildingAction, FrameEffects, Simulation};

const RULES: &str = "[InfantryTypes]\n0=ENGI\n\
    [ENGI]\nStrength=75\nSpeed=4\nSpeedType=Foot\nEngineer=yes\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [BuildingTypes]\n0=BLDG\n1=RUIN\n2=SHED\n\
    [BLDG]\nStrength=1000\nRepairable=yes\nCapturable=yes\n\
    Rubble.Destroyed=RUIN\nRubble.Destroyed.Strength=-50\n\
    [RUIN]\nStrength=400\nRepairable=yes\nRubble.Intact=BLDG\n\
    [SHED]\nStrength=500\nRepairable=yes\nEngineerRepairable=no\n\
    [Warheads]\n0=Super\n[Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";

fn arena() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str(
            "[BLDG]\nFoundation=2x2\n[RUIN]\nFoundation=2x2\n[SHED]\nFoundation=2x2\n",
        ),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x441F12);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

/// The live, placed buildings of `kind`.
fn placed(sim: &Simulation, kind: &str) -> Vec<u64> {
    sim.substrate
        .entities
        .values()
        .filter(|entity| {
            sim.interner.resolve(entity.type_ref()) == kind
                && entity.is_object_alive()
                && !entity.in_limbo()
        })
        .map(|entity| entity.stable_id())
        .collect()
}

fn origin(sim: &Simulation, id: u64) -> (u16, u16) {
    let position = sim.substrate.entities.get(id).unwrap().position;
    (position.rx(), position.ry())
}

/// An Americans engineer on Capture arriving in `building`; then PerCell's
/// building entry.
fn arrive(sim: &mut Simulation, rules: &RuleSet, building: u64) -> u64 {
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::mission::{MissionId, MissionType};
    let engineer = sim
        .spawn_object("ENGI", "Americans", 13, 4, 0, rules)
        .unwrap();
    sim.substrate
        .mission_assign_exact(engineer, MissionId::from_known(MissionType::Capture), 0)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(engineer)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::Object { id: building }));
    let (rx, ry) = origin(sim, building);
    sim.foot_set_location_marked(
        engineer,
        DriveCoord {
            x: i32::from(rx) * 256 + 128,
            y: i32::from(ry) * 256 + 128,
            z: 0,
        },
        rules,
        None,
        FrameEffects::default(),
    );
    let result = sim
        .infantry_per_cell_building_entry(engineer, rules, None, FrameEffects::default())
        .unwrap();
    assert!(result.return_before_foot);
    engineer
}

/// A destroyed building turns into its `Rubble.Destroyed` at its own origin
/// and for its own house (`Default`), with half the rubble's Strength for
/// `Rubble.Destroyed.Strength=-50`.
#[test]
fn a_destroyed_building_turns_into_its_rubble() {
    let (mut sim, rules) = arena();
    let building = sim
        .spawn_object("BLDG", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        building,
        1000,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        sim.interner.intern("Super"),
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    let _ = sim.commit_direct_damage_receiver(&rules, None, hit, FrameEffects::default());

    let ruins = placed(&sim, "RUIN");
    assert_eq!(ruins.len(), 1, "one rubble");
    let ruin = sim.substrate.entities.get(ruins[0]).unwrap();
    assert_eq!(origin(&sim, ruins[0]), (10, 10));
    assert_eq!(ruin.owner(), sim.interner.intern("Russians"));
    assert_eq!(ruin.health.current(), 200);
    assert!(placed(&sim, "BLDG").is_empty());
}

/// A friendly engineer's action over a full-health rubble is the repair
/// action, and entering it rebuilds `Rubble.Intact` at 1% (the -1 default)
/// in its place; he is kicked out of the new building, not consumed.
#[test]
fn an_engineer_rebuilds_a_friendly_rubble_and_survives() {
    let (mut sim, rules) = arena();
    let ruin = sim
        .spawn_object("RUIN", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let probe = sim
        .spawn_object("ENGI", "Americans", 4, 4, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.engineer_building_action(probe, ruin, &rules),
        Some(EngineerBuildingAction::Repair(true))
    );

    let engineer = arrive(&mut sim, &rules, ruin);

    let rebuilt = placed(&sim, "BLDG");
    assert_eq!(rebuilt.len(), 1, "one rebuilt building");
    assert_eq!(origin(&sim, rebuilt[0]), (10, 10));
    let health = sim.substrate.entities.get(rebuilt[0]).unwrap().health;
    assert_eq!(health.current(), 10);
    assert!(placed(&sim, "RUIN").is_empty(), "the rubble is gone");
    let engineer = sim.substrate.entities.get(engineer).unwrap();
    assert!(engineer.is_object_alive() && !engineer.in_limbo());
}

/// `EngineerRepairable=no` overrides `Repairable=yes`: no repair action over
/// the damaged building, and an arriving engineer is turned away unspent.
#[test]
fn engineer_repairable_no_keeps_engineers_off_a_damaged_building() {
    let (mut sim, rules) = arena();
    let shed = sim
        .spawn_object("SHED", "Americans", 10, 10, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(shed)
        .unwrap()
        .health
        .set_for_test(100);
    let probe = sim
        .spawn_object("ENGI", "Americans", 4, 4, 0, &rules)
        .unwrap();
    assert_eq!(sim.engineer_building_action(probe, shed, &rules), None);

    let engineer = arrive(&mut sim, &rules, shed);

    assert_eq!(
        sim.substrate.entities.get(shed).unwrap().health.current(),
        100
    );
    assert!(
        sim.substrate
            .entities
            .get(engineer)
            .unwrap()
            .is_object_alive()
    );
}
