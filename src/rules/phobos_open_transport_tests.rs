//! `OpenTransport.*` reader: the three `Nullable` keys and their
//! `[CombatDamage]`/`[General]` fallbacks (`Ext/TechnoType/Body.cpp:890-892`).

use super::*;
use crate::rules::ini_parser::IniFile;

fn rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=RIDER\n\
         [RIDER]\nStrength=100\nSpeed=4\n\
         [CombatDamage]\nOpenTransport.RangeBonus=4\nOpenTransport.DamageMultiplier=0.25\n\
         [General]\nOpenTransport.FireWhileMoving=no\n",
    ))
    .expect("rules")
}

#[test]
fn absent_type_keys_take_the_global_fallbacks() {
    let rules = rules();
    let rider = rules.object("RIDER").expect("rider type");
    let values = rider.phobos_open_transport();
    assert_eq!(rules.phobos_globals.open_transport_range_bonus, 4);
    assert_eq!(
        f32::from_bits(rules.phobos_globals.open_transport_damage_multiplier.bits()),
        0.25
    );
    assert!(!rules.phobos_globals.open_transport_fire_while_moving);
    assert_eq!(values.range_bonus(&rules), 4);
    assert_eq!(values.damage_multiplier(&rules), 0.25);
    assert!(!values.fire_while_moving(&rules));
}

#[test]
fn present_type_keys_replace_the_globals() {
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=RIDER\n\
         [RIDER]\nStrength=100\nSpeed=4\n\
         OpenTransport.RangeBonus=1\nOpenTransport.DamageMultiplier=2\n\
         OpenTransport.FireWhileMoving=yes\n\
         [CombatDamage]\nOpenTransport.RangeBonus=9\nOpenTransport.DamageMultiplier=9\n\
         [General]\nOpenTransport.FireWhileMoving=no\n",
    );
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("rules");
    let rider = rules.object("RIDER").expect("rider type");
    let values = rider.phobos_open_transport();
    assert_eq!(values.range_bonus(&rules), 1);
    assert_eq!(values.damage_multiplier(&rules), 2.0);
    assert!(values.fire_while_moving(&rules));
}
