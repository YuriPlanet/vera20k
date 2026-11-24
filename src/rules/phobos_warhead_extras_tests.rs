//! The nine `WarheadTypeExt` keys owned by
//! [`crate::rules::phobos_warhead_extras`].

use crate::rules::phobos_warhead_extras::PhobosWarheadExtras;
use crate::rules::ruleset::RuleSet;

fn values(keys: &str) -> PhobosWarheadExtras {
    let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [Warheads]\n0=WH\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n{keys}"
    ));
    RuleSet::from_ini(&ini)
        .expect("the fixture rules parse")
        .warhead("WH")
        .expect("WH is registered")
        .phobos_extras
        .clone()
}

/// `Body.h:328-329`, `:334`, `:463`, `:486-489`, `:189`/`:468`: five `no`
/// valueables (`CLIsBlack` last in `LoadFromINI`, `Body.cpp:344`) and six
/// unset nullables.
#[test]
fn the_nine_keys_construct_at_their_native_defaults() {
    let defaults = PhobosWarheadExtras::default();
    assert_eq!(values(""), defaults);
    assert!(!defaults.remove_mind_control);
    assert_eq!(defaults.remove_mind_control_silent, None);
    assert_eq!(defaults.shake_is_local, None);
    assert_eq!(defaults.merge_building_damage, None);
    assert!(!defaults.building_sell);
    assert!(!defaults.building_sell_ignore_unsellable);
    assert!(!defaults.building_undeploy);
    assert!(!defaults.building_undeploy_leave);
    assert!(!defaults.cl_is_black);
}

#[test]
fn every_key_reads_on_its_own() {
    let set = values(
        "RemoveMindControl=yes\nRemoveMindControl.Silent=yes\nShakeIsLocal=yes\n\
         MergeBuildingDamage=no\n\
         BuildingSell=yes\nBuildingSell.IgnoreUnsellable=yes\n\
         BuildingUndeploy=yes\nBuildingUndeploy.Leave=yes\nCLIsBlack=yes\n",
    );
    assert!(set.remove_mind_control);
    assert_eq!(set.remove_mind_control_silent, Some(true));
    assert_eq!(set.shake_is_local, Some(true));
    // An explicit `no` is a value, not "unset": the global cannot override it.
    assert_eq!(set.merge_building_damage, Some(false));
    assert!(set.building_sell);
    assert!(set.building_sell_ignore_unsellable);
    assert!(set.building_undeploy);
    assert!(set.building_undeploy_leave);
    assert!(set.cl_is_black);

    let one = values("BuildingUndeploy=yes\n");
    assert!(one.building_undeploy);
    assert!(!one.building_sell);
    assert!(!one.building_undeploy_leave);
}

/// `<none>` returns the two nullables to unset, so a later layer can hand the
/// decision back to the `[Rules]` global.
#[test]
fn a_none_spelling_clears_the_nullables() {
    assert_eq!(
        values("MergeBuildingDamage=<none>\n").merge_building_damage,
        None
    );
    assert_eq!(
        values("RemoveMindControl.Silent=none\n").remove_mind_control_silent,
        None
    );
    assert_eq!(values("ShakeIsLocal=<none>\n").shake_is_local, None);
}

/// `Detonate.cpp:478`, `Ext/WarheadType/Hooks.cpp:414` and
/// `Ext/Bullet/Hooks.DetonateLogics.cpp:38`: an unset nullable takes the
/// global, a set one keeps its own answer.
#[test]
fn the_three_globals_only_apply_when_the_key_is_unset() {
    let unset = PhobosWarheadExtras::default();
    assert!(unset.mind_control_silent(true));
    assert!(!unset.mind_control_silent(false));
    assert!(unset.merges_building_damage(true));
    assert!(!unset.merges_building_damage(false));
    assert!(unset.shake_is_local(true));
    assert!(!unset.shake_is_local(false));

    let set = values("RemoveMindControl.Silent=no\nMergeBuildingDamage=no\nShakeIsLocal=no\n");
    assert!(!set.mind_control_silent(true));
    assert!(!set.merges_building_damage(true));
    assert!(!set.shake_is_local(true));
}

/// The three arms that make the per-target walk visit this warhead
/// (`Detonate.cpp:207-208,213-214`); the flags that only refine those arms do
/// not.
#[test]
fn only_the_three_primary_arms_ask_for_a_visit() {
    assert!(!PhobosWarheadExtras::default().has_per_target_effect());
    assert!(values("RemoveMindControl=yes\n").has_per_target_effect());
    assert!(values("BuildingSell=yes\n").has_per_target_effect());
    assert!(values("BuildingUndeploy=yes\n").has_per_target_effect());
    assert!(!values("BuildingSell.IgnoreUnsellable=yes\n").has_per_target_effect());
    assert!(!values("BuildingUndeploy.Leave=yes\n").has_per_target_effect());
    assert!(!values("MergeBuildingDamage=yes\n").has_per_target_effect());
}

/// `Ext/WarheadType/Body.cpp:345`: `Particle.AlphaImageIsLightFlash=`,
/// `Nullable<bool>`, and `Ext/Rules/Body.cpp:403` falls back to
/// `[AudioVisual] WarheadParticleAlphaImageIsLightFlash=` (default no) when the
/// warhead leaves it unset.
#[test]
fn particle_alpha_image_is_light_flash_reads_nullable() {
    assert_eq!(values("").particle_alpha_image_is_light_flash, None);
    assert_eq!(
        values("Particle.AlphaImageIsLightFlash=yes\n").particle_alpha_image_is_light_flash,
        Some(true)
    );
    assert_eq!(
        values("Particle.AlphaImageIsLightFlash=no\n").particle_alpha_image_is_light_flash,
        Some(false)
    );
}
