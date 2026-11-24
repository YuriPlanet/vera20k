//! The eight `WarheadTypeExt` damage-multiplier keys
//! (`Ext/WarheadType/Body.cpp:347-354`).

use crate::rules::phobos_damage_multiplier::{DamageMultiplierRules, DamageRelation};
use crate::rules::ruleset::RuleSet;

fn rules_with(keys: &str) -> RuleSet {
    let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [Warheads]\n0=WH\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n{keys}"
    ));
    RuleSet::from_ini(&ini).expect("the fixture rules parse")
}

fn values(keys: &str) -> DamageMultiplierRules {
    rules_with(keys)
        .warhead("WH")
        .expect("WH is registered")
        .phobos_damage_multiplier
        .clone()
}

/// `Body.h:471-478`: every nullable column constructs unset and both health
/// terms construct `0.0`, so a warhead that names nothing leaves the global
/// columns free to act on their own.
#[test]
fn the_eight_keys_construct_unset_and_zero() {
    let defaults = DamageMultiplierRules::default();
    assert_eq!(values(""), defaults);
    assert_eq!(defaults.owner, None);
    assert_eq!(defaults.allies, None);
    assert_eq!(defaults.enemies, None);
    assert_eq!(defaults.owner_berzerk, None);
    assert_eq!(defaults.allies_berzerk, None);
    assert_eq!(defaults.enemies_berzerk, None);
    assert_eq!(defaults.source_health, 0.0);
    assert_eq!(defaults.target_health, 0.0);
}

/// The three plain columns and the three `.Berzerk` columns are independent
/// nullables: naming one leaves the others unset.
#[test]
fn each_column_reads_independently() {
    let set = values(
        "DamageOwnerMultiplier=0.5\n\
         DamageAlliesMultiplier=0.25\n\
         DamageEnemiesMultiplier=2\n\
         DamageOwnerMultiplier.Berzerk=3\n\
         DamageAlliesMultiplier.Berzerk=4\n\
         DamageEnemiesMultiplier.Berzerk=5\n",
    );
    assert_eq!(set.owner, Some(0.5));
    assert_eq!(set.allies, Some(0.25));
    assert_eq!(set.enemies, Some(2.0));
    assert_eq!(set.owner_berzerk, Some(3.0));
    assert_eq!(set.allies_berzerk, Some(4.0));
    assert_eq!(set.enemies_berzerk, Some(5.0));

    let half = values("DamageEnemiesMultiplier.Berzerk=5\n");
    assert_eq!(half.enemies, None);
    assert_eq!(half.owner, None);
    assert_eq!(half.enemies_berzerk, Some(5.0));
}

/// `Nullable<double>` clears on the `<none>` spellings Phobos accepts, so a
/// later layer can drop an earlier multiplier.
#[test]
fn a_none_spelling_clears_a_column() {
    assert_eq!(
        values("DamageEnemiesMultiplier=<none>\n").enemies,
        None,
        "<none> clears"
    );
    assert_eq!(
        values("DamageAlliesMultiplier=none\n").allies,
        None,
        "none clears"
    );
}

/// The two health terms are `Valueable<double>`: an absent key keeps the
/// constructor `0.0` and a later read replaces the earlier one.
#[test]
fn the_health_terms_are_valueable() {
    let set = values("DamageSourceHealthMultiplier=1\nDamageTargetHealthMultiplier=0.5\n");
    assert_eq!(set.source_health, 1.0);
    assert_eq!(set.target_health, 0.5);
    assert_eq!(
        values("DamageSourceHealthMultiplier=1\n").target_health,
        0.0
    );
}

/// `Ext/Techno/Hooks.ReceiveDamage.cpp:69-74`: the enemy column wins whenever
/// the source House is missing or not allied; a same-House hit reaches the
/// owner column through the reflexive ally test.
#[test]
fn the_relation_follows_the_native_column_order() {
    assert_eq!(
        DamageRelation::classify(None, 7, false),
        DamageRelation::Enemies
    );
    assert_eq!(
        DamageRelation::classify(Some(3), 7, false),
        DamageRelation::Enemies
    );
    assert_eq!(
        DamageRelation::classify(Some(3), 7, true),
        DamageRelation::Allies
    );
    assert_eq!(
        DamageRelation::classify(Some(7), 7, true),
        DamageRelation::Owner
    );
}
