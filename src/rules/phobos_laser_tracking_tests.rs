use super::{LaserTrackingGlobals, PositionFollow};
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;

fn weapon(text: &str) -> WeaponType {
    let ini = IniFile::from_str(text);
    WeaponType::from_ini_section("W", ini.section("W").unwrap())
}

fn layered(root: &str, layer: &str) -> RuleSet {
    let mut stack = RulesLayerStack::new(IniFile::from_str(root));
    stack.push(RulesLayerKind::Scenario, IniFile::from_str(layer));
    RuleSet::from_processed_rules(&stack.process().unwrap()).unwrap()
}

#[test]
fn position_follow_reads_the_four_native_names() {
    assert_eq!(PositionFollow::parse("none"), Some(PositionFollow::None));
    assert_eq!(PositionFollow::parse("FIRER"), Some(PositionFollow::Firer));
    assert_eq!(
        PositionFollow::parse("Target"),
        Some(PositionFollow::Target)
    );
    assert_eq!(PositionFollow::parse("all"), Some(PositionFollow::All));
    assert_eq!(
        PositionFollow::parse("firer,target"),
        None,
        "the reader has no flag syntax"
    );
}

#[test]
fn a_weapon_reads_the_mode_and_its_nullable_stop_override() {
    let target = weapon(
        "[W]\nLaserPositionUpdate=Target\n\
         LaserPositionUpdate.StopOnFirerConvert=yes\n",
    );
    assert_eq!(target.laser_position_update, PositionFollow::Target);
    assert_eq!(
        target.laser_position_update_stop_on_firer_convert,
        Some(true)
    );

    let bare = weapon("[W]\nIsLaser=yes\n");
    assert_eq!(bare.laser_position_update, PositionFollow::None);
    assert_eq!(bare.laser_position_update_stop_on_firer_convert, None);

    // read<PositionFollow> keeps the field on rejected text, and the nullable
    // clears on `<default>`.
    let rejected = weapon(
        "[W]\nLaserPositionUpdate=sideways\n\
         LaserPositionUpdate.StopOnFirerConvert=<default>\n",
    );
    assert_eq!(rejected.laser_position_update, PositionFollow::None);
    assert_eq!(
        rejected.laser_position_update_stop_on_firer_convert, None,
        "IsBlank clears a nullable"
    );
}

#[test]
fn the_audio_visual_global_defaults_off_and_reads_the_key() {
    assert!(
        !LaserTrackingGlobals::from_ini(&IniFile::from_str("[AudioVisual]\n"))
            .stop_on_firer_convert
    );
    let on = LaserTrackingGlobals::from_ini(&IniFile::from_str(
        "[AudioVisual]\nLaserPositionUpdate.StopOnFirerConvert=yes\n",
    ));
    assert!(on.stop_on_firer_convert);
}

#[test]
fn the_rules_projection_keeps_the_earlier_mode_after_rejected_text() {
    let rules = layered(
        "[VehicleTypes]\n0=UNIT\n[UNIT]\nStrength=100\nPrimary=PROBE\n\
         [PROBE]\nIsLaser=yes\nLaserPositionUpdate=Firer\n",
        "[PROBE]\nLaserPositionUpdate=bogus\n\
         [AudioVisual]\nLaserPositionUpdate.StopOnFirerConvert=yes\n",
    );
    let probe = rules.weapon("PROBE").expect("PROBE");
    assert_eq!(
        probe.laser_position_update,
        PositionFollow::Firer,
        "a later rejected name keeps the earlier pass's mode"
    );
    assert!(rules.phobos_laser_tracking.stop_on_firer_convert);
}
