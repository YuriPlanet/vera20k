//! Regressions for the Phobos `KillWeapon.*` chain through
//! `TechnoClass::ReceiveDamage` (`0x702672`).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

const ALL_HIT: &str = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";

fn scene(text: &str) -> (Simulation, RuleSet) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(text);
    let rules = RuleSet::from_ini(&ini).expect("rules parse");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 23);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

fn scene_with_art(text: &str, art_text: &str) -> (Simulation, RuleSet) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(text);
    let art = IniFile::from_str(art_text);
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("rules parse");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 23);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

fn spawn(
    sim: &mut Simulation,
    rules: &RuleSet,
    type_name: &str,
    owner: &str,
    rx: u16,
    ry: u16,
) -> u64 {
    sim.spawn_object(type_name, owner, rx, ry, 0, rules)
        .expect("arena spawn")
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.substrate
        .entities
        .get(id)
        .expect("live entity")
        .health
        .current()
}

fn kill(sim: &mut Simulation, rules: &RuleSet, victim: u64, attacker: Option<u64>) {
    let warhead = sim.interner.intern("HIT");
    let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
        victim,
        100,
        0,
        attacker.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    let _ = sim.commit_direct_damage_receiver(
        rules,
        None,
        event,
        crate::sim::world::FrameEffects::empty(),
    );
    assert!(
        sim.substrate.entities.get(victim).is_none() || health(sim, victim) == 0,
        "the victim died"
    );
}

/// `KillWeapon=` detonates a weapon on the dying object (its splash reaches a
/// bystander) and `KillWeapon.OnFirer=` hits the killing source.
#[test]
fn kill_weapon_hits_the_dead_object_and_on_firer_hits_the_source() {
    let (mut sim, rules) = scene(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=ATTACKER\n1=TARGET\n2=BYSTANDER\n\
         [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\nPrimary=KILLW\nSecondary=ONW\n\
         [TARGET]\nStrength=50\nSpeed=6\nArmor=none\nSecondary=KILLW2\n\
         [BYSTANDER]\nStrength=1000\nSpeed=6\nArmor=none\n\
         [HIT]\nKillWeapon=KILLW2\nKillWeapon.OnFirer=ONW\n{ALL_HIT}\
         [KILLH]\nCellSpread=1\nAffectsAllies=yes\n{ALL_HIT}\
         [ONH]\n{ALL_HIT}\
         [KILLW]\nDamage=100\nRange=1\nWarhead=HIT\n\
         [KILLW2]\nDamage=9\nRange=1\nWarhead=KILLH\n\
         [ONW]\nDamage=7\nRange=1\nWarhead=ONH\n"
    ));
    let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 18, 20);
    let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
    let bystander = spawn(&mut sim, &rules, "BYSTANDER", "Americans", 22, 20);
    kill(&mut sim, &rules, victim, Some(attacker));
    assert_eq!(
        health(&sim, bystander),
        991,
        "the kill weapon splashed the bystander for 9"
    );
    assert_eq!(
        health(&sim, attacker),
        993,
        "OnFirer hit the killing source for 7"
    );
}

/// The `AffectsHouse`, `AffectsTarget` and `SuppressKillWeapons` gates all
/// drop the `OnFirer` shot for the listed reason.
#[test]
fn kill_weapon_gates_house_target_and_suppression() {
    let run = |warhead_lines: &str, victim_lines: &str, victim_owner: &str| -> i32 {
        let (mut sim, rules) = scene(&format!(
            "[Countries]\n0=Americans\n1=Russians\n\
             [VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
             [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\nPrimary=KILLW\nSecondary=ONW\n\
             [TARGET]\nStrength=50\nSpeed=6\nArmor=none\n{victim_lines}\
             [HIT]\nKillWeapon.OnFirer=ONW\n{warhead_lines}{ALL_HIT}\
             [ONH]\n{ALL_HIT}\
             [KILLW]\nDamage=100\nRange=1\nWarhead=HIT\n\
             [ONW]\nDamage=7\nRange=1\nWarhead=ONH\n"
        ));
        let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 18, 20);
        let victim = spawn(&mut sim, &rules, "TARGET", victim_owner, 21, 20);
        kill(&mut sim, &rules, victim, Some(attacker));
        let damage = 1000 - health(&sim, attacker);
        damage
    };
    // Default flags: every house and target, victim an enemy unit.
    assert_eq!(run("", "", "Americans"), 7);
    // Same-house victim: only the Owner flag admits it.
    assert_eq!(
        run("KillWeapon.OnFirer.AffectsHouse=enemies\n", "", "Russians"),
        0
    );
    assert_eq!(
        run("KillWeapon.OnFirer.AffectsHouse=owner\n", "", "Russians"),
        7
    );
    // The victim's category must be in AffectsTarget.
    assert_eq!(
        run(
            "KillWeapon.OnFirer.AffectsTarget=buildings\n",
            "",
            "Americans"
        ),
        0
    );
    assert_eq!(
        run("KillWeapon.OnFirer.AffectsTarget=units\n", "", "Americans"),
        7
    );
    // SuppressKillWeapons drops everything unless a filter list omits the
    // weapon.
    assert_eq!(run("", "SuppressKillWeapons=yes\n", "Americans"), 0);
    assert_eq!(
        run(
            "",
            "SuppressKillWeapons=yes\nSuppressKillWeapons.Types=ONW\n",
            "Americans"
        ),
        0
    );
    assert_eq!(
        run(
            "",
            "SuppressKillWeapons=yes\nSuppressKillWeapons.Types=OTHER\n",
            "Americans"
        ),
        7
    );
}

/// `KillWeapon.AffectsHouse` reads the killing hit's source house, and
/// `AffectsTarget` reads the victim's category, not the source's.
#[test]
fn kill_weapon_gates_read_the_killing_house_and_the_victim() {
    let run = |warhead_lines: &str, source_is_building: bool| -> (i32, i32) {
        let (mut sim, rules) = scene_with_art(
            &format!(
                "[Countries]
0=Americans
1=Russians
                 [VehicleTypes]
0=ATTACKER
1=TARGET
2=BYSTANDER
                 [BuildingTypes]
0=TOWER
                 [ATTACKER]
Strength=1000
Speed=6
Armor=none
Primary=KILLW
                 [TARGET]
Strength=50
Speed=6
Armor=none
Secondary=KILLW2
                 [BYSTANDER]
Strength=1000
Speed=6
Armor=none
                 [TOWER]
Strength=1000
                 [HIT]
KillWeapon=KILLW2
{warhead_lines}{ALL_HIT}                 [KILLH]
CellSpread=1
AffectsAllies=yes
{ALL_HIT}                 [KILLW]
Damage=100
Range=1
Warhead=HIT
                 [KILLW2]
Damage=9
Range=1
Warhead=KILLH
"
            ),
            "[TOWER]
Foundation=1x1
",
        );
        let source = if source_is_building {
            spawn(&mut sim, &rules, "TOWER", "Russians", 18, 20)
        } else {
            spawn(&mut sim, &rules, "ATTACKER", "Russians", 18, 20)
        };
        let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
        let bystander = spawn(&mut sim, &rules, "BYSTANDER", "Americans", 22, 20);
        kill(&mut sim, &rules, victim, Some(source));
        (health(&sim, source), health(&sim, bystander))
    };
    // An enemy source fails the owner flag; the enemies flag admits it.
    assert_eq!(
        run(
            "KillWeapon.AffectsHouse=owner
",
            false
        ),
        (1000, 1000)
    );
    assert_eq!(
        run(
            "KillWeapon.AffectsHouse=enemies
",
            false
        ),
        (1000, 991)
    );
    // The victim is a unit, so `units` admits and `buildings` refuses it even
    // though the killing source is a building.
    assert_eq!(
        run(
            "KillWeapon.AffectsTarget=units
",
            true
        ),
        (1000, 991)
    );
    assert_eq!(
        run(
            "KillWeapon.AffectsTarget=buildings
",
            true
        ),
        (1000, 1000)
    );
}
