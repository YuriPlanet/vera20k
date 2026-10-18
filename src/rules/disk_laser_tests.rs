//! DiskLaser's Rules+28C reference uses the existing process sound binder.
//! Original66A355..66A38C retains a prior valid Voc ID; missing/unknown
//! names do not erase it. Fixed SOUNDMD is never a scenario rules layer.

use super::ini_parser::IniFile;
use super::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use super::ruleset::RuleSet;
use super::sound_ini::SoundRegistry;
use std::sync::Arc;

#[test]
fn charge_up_uses_fixed_catalog_and_reached_rules_history() {
    let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=Charge\n1=Replacement\n\
         [Charge]\nSounds=first\n[Replacement]\nSounds=second\n",
    )));
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str("[AudioVisual]\nDiskLaserChargeUp=Charge\n"),
        Some(IniFile::from_str(
            "[AudioVisual]\nDiskLaserChargeUp=Replacement\n",
        )),
        IniFile::empty(),
        sounds,
    )
    .unwrap();
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(Some(&IniFile::from_str(
                "[AudioVisual]\nDiskLaserChargeUp=Unregistered\n",
            ))),
            &IniFile::from_str("[AudioVisual]\nDiskLaserChargeUp=\ndisklaserchargeup=Charge\n"),
        )
        .unwrap()
        .into_parts();
    assert_eq!(rules.general.disk_laser_charge_up(), Some("Replacement"));
    assert_eq!(
        RuleSet::from_ini(&IniFile::empty())
            .unwrap()
            .general
            .disk_laser_charge_up(),
        None
    );
}

#[test]
fn stock_disc_reads_battle_map_weapon_art_and_charge_reference() {
    let Some(retail) = super::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let rules = &retail.rules;
    let disc = rules.object("DISK").expect("retail Floating Disc");
    let weapon = rules.weapon("DiskLaser").expect("retail selected primary");
    assert_eq!(disc.primary(), Some("DiskLaser"));
    assert!(weapon.disk_laser);
    assert!(!weapon.is_house_color);
    assert_eq!(weapon.range_leptons, 7 * 256);
    assert_eq!(weapon.laser_inner_color, [216, 0, 184]);
    assert_eq!(weapon.laser_outer_color, [80, 0, 88]);
    assert_eq!(weapon.laser_outer_spread, [0, 0, 0]);
    let art = rules.art().get("DISK").expect("fixed retail DISK art");
    assert_eq!(art.primary_fire_flh, [0, 0, 75].into());
    assert_eq!(art.turret_offset, 0);
    assert_eq!(
        rules.general.disk_laser_charge_up(),
        Some("FloatingDiscChargeUp")
    );
}
