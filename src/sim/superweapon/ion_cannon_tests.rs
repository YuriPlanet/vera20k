//! The Tiberian Sun Ion Cannon (`Type=IonCannon`) through production
//! frames: the player's click (Firestorm 2.0 SuperClass::Place case 3) and
//! the computer's target (Firestorm 2.0 `0x4C9F30`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::superweapon::chronosphere_tests::{charge_super, click, place, step, world_with};
use crate::sim::world::FrameEffects;

/// `ion_blast_tests`' blast rules (IonCannonDamage=10 with the ION warhead),
/// an Ion Cannon, a type with the same Action= but no Type=, a 5000-strength
/// TANK with a passenger slot (an APC, worth 5) and a 10-strength WEAK unit.
fn rules() -> RuleSet {
    let art = IniFile::from_str(
        "[RING1]\nRate=1\nEnd=8\n[SPLASH0]\nRate=1\nEnd=8\n[SPLASH1]\nRate=1\nEnd=8\n[IONBEAM]\nRate=1\nEnd=8\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(
        "[Animations]\n0=SPLASH0\n1=SPLASH1\n\
         [General]\nIonBlast=RING1\nIonBeam=IONBEAM\nAIIonCannonAPCValue=5,5,5\n\
         [CombatDamage]\nIonCannonDamage=10\nIonCannonWarhead=ION\nSplashList=SPLASH0,SPLASH1\n\
         [SuperWeaponTypes]\n0=IonCannonSpecial\n1=PlainSpecial\n\
         [IonCannonSpecial]\nType=IonCannon\nAction=IonCannon\nRechargeTime=8.5\n\
         [PlainSpecial]\nAction=IonCannon\nRechargeTime=8.5\n\
         [Warheads]\n0=ION\n[ION]\nCellSpread=0.5\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [VehicleTypes]\n0=TANK\n1=WEAK\n\
         [TANK]\nStrength=5000\nPassengers=1\nArmor=none\nSpeed=4\nCrewed=no\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [WEAK]\nStrength=10\nArmor=none\nSpeed=4\nCrewed=no\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    ), &art)
    .unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    for name in ["RING1", "IONBEAM", "SPLASH0", "SPLASH1"] {
        rules.bind_anim_frame_count_for_test(name, 32);
    }
    rules
}

/// The click builds one damaging blast at the cell's centre at its Level
/// times 104 and spends the charge; the next frame's blast hurts the tank.
#[test]
fn click_blasts_the_cell_centre_at_its_level() {
    let (rules, mut sim, americans) = world_with(rules(), 64, &[((20, 20), 2)]);
    let tank = place(&mut sim, &rules, "TANK", (20, 20), 2, false, false);
    let before = sim.entities().get(tank).unwrap().health.current();
    let sw = charge_super(&mut sim, americans, "IonCannonSpecial");

    click(&mut sim, &rules, americans, "IonCannonSpecial", (20, 20));
    assert_eq!(
        sim.ion_blasts().collect::<Vec<_>>(),
        [([5248, 5248, 208], 0)]
    );
    assert!(!super::super::is_charged(&sim, americans, sw));

    step(&mut sim, &rules);
    assert!(sim.entities().get(tank).unwrap().health.current() < before);
}

/// `Action=IonCannon` is an action only for the Ion Cannon type; the same
/// keys without `Type=IonCannon` keep gamemd's reading and fire nothing.
#[test]
fn only_type_ion_cannon_reads_its_action_and_fires() {
    let rules = rules();
    let ion = rules.super_weapon("IonCannonSpecial").unwrap();
    assert_eq!(
        (ion.kind, ion.action.as_deref()),
        (SuperWeaponKind::IonCannon, Some("IonCannon"))
    );
    let plain = rules.super_weapon("PlainSpecial").unwrap();
    assert_eq!(
        (plain.kind, plain.action.as_deref()),
        (SuperWeaponKind::NoEffect, Some("None"))
    );

    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let tank = place(&mut sim, &rules, "TANK", (20, 20), 0, false, false);
    charge_super(&mut sim, americans, "PlainSpecial");
    click(&mut sim, &rules, americans, "PlainSpecial", (20, 20));
    step(&mut sim, &rules);
    assert_eq!(sim.ion_blasts().count(), 0);
    assert_eq!(sim.entities().get(tank).unwrap().health.current(), 5000);
}

/// The computer aims at the enemy object the blast can kill: WEAK's 10
/// strength is not above IonCannonDamage=10 (worth 2); TANK's is, so it is
/// worth 1, not gamemd's APC value of 5.
#[test]
fn computer_aims_at_the_enemy_the_blast_can_kill() {
    let (rules, mut sim, americans) = world_with(rules(), 64, &[]);
    let russians = sim.interner.intern("Russians");
    place(&mut sim, &rules, "TANK", (20, 20), 0, false, false);
    place(&mut sim, &rules, "WEAK", (30, 24), 0, false, false);
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_enemy_house_for_test(Some(americans));
    charge_super(&mut sim, russians, "IonCannonSpecial");

    crate::sim::superweapon::ai_fire::try_fire(
        &mut sim,
        &rules,
        russians,
        None,
        &mut FrameEffects::default(),
    );
    assert_eq!(
        sim.ion_blasts().collect::<Vec<_>>(),
        [([30 * 256 + 128, 24 * 256 + 128, 0], 0)]
    );
}
