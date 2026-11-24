//! The Phobos damage-multiplier block
//! (`Ext/Techno/Hooks.ReceiveDamage.cpp:48-82`).

use super::apply;
use crate::map::houses::HouseAllianceMap;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::receiver_call::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::world::Simulation;

const VERSES: &str = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n";

/// A victim owned by `Americans` and an attacker owned by `Russians`, plus the
/// rules the warhead's keys come from.
fn fixture(combat: &str, keys: &str) -> (Simulation, RuleSet, u64, u64) {
    let ini = IniFile::from_str(&format!(
        "[CombatDamage]\n{combat}\
         [InfantryTypes]\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n[BuildingTypes]\n\
         [Warheads]\n0=WH\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
         [WH]\n{VERSES}{keys}"
    ));
    let rules = RuleSet::from_ini(&ini).expect("the fixture rules parse");
    let mut sim = Simulation::new();
    let victim = sim
        .spawn_object("MTNK", "Americans", 4, 4, 0, &rules)
        .expect("the victim spawns");
    let attacker = sim
        .spawn_object("MTNK", "Russians", 5, 4, 0, &rules)
        .expect("the attacker spawns");
    (sim, rules, victim, attacker)
}

fn event(sim: &mut Simulation, victim: u64, attacker: u64, damage: i32) -> EntityDamageEvent {
    let source_house = sim.substrate.entities.get(attacker).map(|it| it.owner());
    EntityDamageEvent::area(
        victim,
        damage,
        0,
        attacker,
        source_house,
        sim.interner.intern("WH"),
    )
}

/// `:69-70`: an enemy victim takes the enemy column, which here doubles the
/// packet.
#[test]
fn the_enemy_column_scales_the_packet() {
    let (mut sim, rules, victim, attacker) = fixture("", "DamageEnemiesMultiplier=2\n");
    let event = event(&mut sim, victim, attacker, 10);
    assert_eq!(apply(&sim, &rules, &event, 10), 20);
}

/// `:73-74`: a hit from the victim's own House takes the owner column, so a
/// warhead that only scales enemies leaves it alone.
#[test]
fn the_owner_column_governs_a_same_house_hit() {
    let (mut sim, rules, victim, _) =
        fixture("", "DamageEnemiesMultiplier=2\nDamageOwnerMultiplier=0.5\n");
    let friendly = sim
        .spawn_object("MTNK", "Americans", 6, 4, 0, &rules)
        .expect("the friendly shooter spawns");
    let event = event(&mut sim, victim, friendly, 10);
    assert_eq!(apply(&sim, &rules, &event, 10), 5);
}

/// `:71-72`: an allied shooter takes the ally column.
#[test]
fn the_ally_column_governs_an_allied_hit() {
    let (mut sim, rules, victim, attacker) = fixture(
        "",
        "DamageEnemiesMultiplier=2\nDamageAlliesMultiplier=0.25\n",
    );
    sim.house_alliances = HouseAlliances::new(HouseAllianceMap::from([(
        "Americans".to_string(),
        ["Russians".to_string()].into_iter().collect(),
    )]));
    let event = event(&mut sim, victim, attacker, 8);
    assert_eq!(apply(&sim, &rules, &event, 8), 2);
}

/// `:62-63`: a berserk attacker reads its own `.Berzerk` column, falling back
/// to the plain column only when the warhead names none.
#[test]
fn the_berzerk_column_overrides_the_plain_enemy_column() {
    let (mut sim, rules, victim, attacker) = fixture(
        "",
        "DamageEnemiesMultiplier=2\nDamageEnemiesMultiplier.Berzerk=3\n",
    );
    let event = event(&mut sim, victim, attacker, 10);
    assert_eq!(apply(&sim, &rules, &event, 10), 20);
    sim.substrate.entities.get_mut(attacker).unwrap().berserk =
        crate::sim::combat::BerserkState::for_test(true, 100);
    assert_eq!(apply(&sim, &rules, &event, 10), 30);
}

/// `:72`/`:74`: `AffectsEnemies=no` makes an unset owner column fall back to
/// the global `.NotAffectsEnemies` value instead of the plain global.
#[test]
fn not_affects_enemies_swaps_the_owner_column_fallback() {
    let keys = "AffectsEnemies=no\n";
    let (mut sim, rules, victim, _) = fixture("", keys);
    assert_eq!(rules.phobos_globals.damage_owner_multiplier(), 1.0);
    let owner = sim.substrate.entities.get(victim).unwrap().owner();
    let event = EntityDamageEvent::area(victim, 10, 0, 0, Some(owner), sim.interner.intern("WH"));
    assert_eq!(
        apply(&sim, &rules, &event, 10),
        10,
        "the plain owner global is 1.0"
    );

    let (mut sim, rules, victim, _) = fixture(
        "DamageOwnerMultiplier.NotAffectsEnemies=0.5\n",
        "AffectsEnemies=no\n",
    );
    let owner = sim.substrate.entities.get(victim).unwrap().owner();
    let event = EntityDamageEvent::area(victim, 10, 0, 0, Some(owner), sim.interner.intern("WH"));
    assert_eq!(apply(&sim, &rules, &event, 10), 5);
}

/// `:76-80`: the health terms are added, not multiplied, and scale by the
/// shooter's and the victim's health fraction.
#[test]
fn the_health_terms_add_to_the_multiplier() {
    let (mut sim, rules, victim, attacker) = fixture(
        "",
        "DamageSourceHealthMultiplier=1\nDamageTargetHealthMultiplier=1\n",
    );
    let event = event(&mut sim, victim, attacker, 10);
    // Full health on both sides: 1 + 1 + 1.
    assert_eq!(apply(&sim, &rules, &event, 10), 30);
    sim.substrate
        .entities
        .get_mut(attacker)
        .unwrap()
        .health
        .set_for_test(0);
    sim.substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .health
        .set_for_test(0);
    // 1 + 0 + 0 leaves the packet alone.
    assert_eq!(apply(&sim, &rules, &event, 10), 10);
}

/// `:84-87`: a product that truncates to zero keeps the damage's sign, so a
/// fractional hit still lands as one point.
#[test]
fn a_truncation_to_zero_keeps_the_sign() {
    let (mut sim, rules, victim, attacker) = fixture("", "DamageEnemiesMultiplier=0.4\n");
    let event = event(&mut sim, victim, attacker, 1);
    assert_eq!(apply(&sim, &rules, &event, 1), 1);
    assert_eq!(apply(&sim, &rules, &event, -1), -1);
}

/// `:50`: a forced hit and a zero packet skip the block entirely.
#[test]
fn forced_and_zero_packets_skip_the_block() {
    let (mut sim, rules, victim, attacker) = fixture("", "DamageEnemiesMultiplier=2\n");
    let mut forced = event(&mut sim, victim, attacker, 10);
    forced.receiver_flags = ReceiverCallFlags {
        ignore_defenses: true,
        arg6: false,
    };
    assert_eq!(apply(&sim, &rules, &forced, 10), 10);
    let zero = event(&mut sim, victim, attacker, 0);
    assert_eq!(apply(&sim, &rules, &zero, 0), 0);
}
