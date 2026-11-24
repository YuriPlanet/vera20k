//! The eleven `OpenTopped.*` type reads and the two fallback chains
//! (`Ext/TechnoType/Body.cpp:879-889`, `Ext/Rules/Body.cpp:455-462`).
use super::PhobosOpenTopped;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_globals::PhobosGlobals;
use crate::rules::ruleset::RuleSet;

#[test]
fn open_topped_task_d_rules() {
    // All eleven type keys construct unset (Body.h:546-556).
    let empty = PhobosOpenTopped::from_section(
        IniFile::from_str("[TRANSPORT]\nStrength=100\n").section_or_empty("TRANSPORT"),
    );
    assert_eq!(empty, PhobosOpenTopped::default());
    assert_eq!(empty.range_bonus, None);
    assert_eq!(empty.damage_multiplier, None);
    assert_eq!(empty.warp_distance, None);
    assert_eq!(empty.ignore_rangefinding, None);
    assert_eq!(empty.allow_firing_if_deactivated, None);
    assert_eq!(empty.allow_firing_if_attacked_by_locomotor, None);
    assert_eq!(empty.share_transport_target, None);
    assert_eq!(empty.use_transport_range_modifiers, None);
    assert_eq!(empty.check_transport_disable_weapons, None);
    assert_eq!(empty.decloak_to_fire, None);
    assert_eq!(empty.fire_while_moving, None);

    // The eight `[General]` `Valueable` defaults (Rules/Body.h:870-877).
    let globals = PhobosGlobals::default();
    assert!(!globals.open_topped_ignore_rangefinding);
    assert!(globals.open_topped_allow_firing_if_deactivated);
    assert!(globals.open_topped_allow_firing_if_attacked_by_locomotor);
    assert!(globals.open_topped_share_transport_target);
    assert!(!globals.open_topped_use_transport_range_modifiers);
    assert!(!globals.open_topped_check_transport_disable_weapons);
    assert!(!globals.open_topped_decloak_to_fire);
    assert!(globals.open_topped_fire_while_moving);

    // Reading all eleven keys. DamageMultiplier is a Nullable<float>: the
    // reader yields f64 and VERA narrows to f32.
    let ini = IniFile::from_str(
        "[SET]\nOpenTopped.RangeBonus=9\nOpenTopped.DamageMultiplier=2.5\n\
         OpenTopped.WarpDistance=7\nOpenTopped.IgnoreRangefinding=yes\n\
         OpenTopped.AllowFiringIfDeactivated=no\n\
         OpenTopped.AllowFiringIfAttackedByLocomotor=no\n\
         OpenTopped.ShareTransportTarget=no\n\
         OpenTopped.UseTransportRangeModifiers=yes\n\
         OpenTopped.CheckTransportDisableWeapons=yes\n\
         OpenTopped.DecloakToFire=yes\nOpenTopped.FireWhileMoving=no\n\
         [CLEARED]\nOpenTopped.RangeBonus=<default>\n\
         OpenTopped.DamageMultiplier=none\nOpenTopped.IgnoreRangefinding=<none>\n",
    );
    let set = PhobosOpenTopped::from_section(ini.section_or_empty("SET"));
    assert_eq!(set.range_bonus, Some(9));
    assert_eq!(set.damage_multiplier, Some(2.5_f32));
    assert_eq!(set.warp_distance, Some(7));
    assert_eq!(set.ignore_rangefinding, Some(true));
    assert_eq!(set.allow_firing_if_deactivated, Some(false));
    assert_eq!(set.allow_firing_if_attacked_by_locomotor, Some(false));
    assert_eq!(set.share_transport_target, Some(false));
    assert_eq!(set.use_transport_range_modifiers, Some(true));
    assert_eq!(set.check_transport_disable_weapons, Some(true));
    assert_eq!(set.decloak_to_fire, Some(true));
    assert_eq!(set.fire_while_moving, Some(false));

    // `<default>`, `none` and `<none>` clear a Nullable back to unset.
    let cleared = PhobosOpenTopped::from_section(ini.section_or_empty("CLEARED"));
    assert_eq!(cleared.range_bonus, None);
    assert_eq!(cleared.damage_multiplier, None);
    assert_eq!(cleared.ignore_rangefinding, None);

    // The three numeric keys fall back to the YR `[CombatDamage]` fields, not
    // to any Phobos `[General]` key; the eight bools fall back to `[General]`.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[CombatDamage]\nOpenToppedRangeBonus=3\n\
         OpenToppedDamageMultiplier=1.5\nOpenToppedWarpDistance=8\n\
         [General]\nOpenTopped.IgnoreRangefinding=yes\n\
         OpenTopped.CheckTransportDisableWeapons=yes\n",
    ))
    .unwrap();
    let inherited = PhobosOpenTopped::default();
    assert_eq!(inherited.range_bonus(&rules), 3);
    assert_eq!(inherited.damage_multiplier(&rules), 1.5);
    assert_eq!(inherited.warp_distance(&rules), 8);
    assert!(inherited.ignore_rangefinding(&rules));
    assert!(inherited.check_transport_disable_weapons(&rules));
    assert!(!inherited.decloak_to_fire(&rules));

    // A set type key overrides the fallback; an unset one inherits it.
    let mut overridden = set;
    overridden.range_bonus = Some(9);
    overridden.damage_multiplier = Some(2.5);
    assert_eq!(overridden.range_bonus(&rules), 9);
    assert_eq!(overridden.damage_multiplier(&rules), 2.5);
    let mut partial = PhobosOpenTopped::default();
    partial.warp_distance = Some(4);
    assert_eq!(partial.warp_distance(&rules), 4);
}
