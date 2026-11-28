//! The per-game NPatch switch (`rules::npatch_switch`): a key only an NPatch
//! reader reads turns NPatch on, the same key read by a stock or Ares reader
//! does not, and retail data leaves the switch off.

use crate::rules::ares_switch;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::npatch_switch;
use crate::rules::ruleset::RuleSet;
use crate::util::npatch_launch::NpatchSetting;

fn rules_with(ini: &str, setting: NpatchSetting) -> RuleSet {
    let processed = RulesLayerStack::new(IniFile::from_str(ini))
        .with_npatch_setting(setting)
        .process()
        .unwrap();
    RuleSet::from_processed_rules(&processed).unwrap()
}

/// The switch's answer for one recorded projection over `read`, with no pass
/// read of its own.
fn decides(ini: &IniFile, read: impl FnOnce(&IniSection)) -> bool {
    let (_, reads) = ares_switch::record(|| read(ini.section("General").unwrap()));
    npatch_switch::NPatchInputs::default().decide(None, &reads)
}

/// A key only an NPatch reader found turns NPatch on. The same key read by a
/// stock or an Ares reader is theirs, so it turns neither extension on.
#[test]
fn an_npatch_only_key_turns_npatch_on() {
    let ini = IniFile::from_str("[General]\nVehicleParachute=PARACH\n");
    assert!(decides(&ini, |section| {
        npatch_switch::npatch_read(|| section.read_name("VehicleParachute", 2048));
    }));
    assert!(!decides(&ini, |section| {
        section.read_name("VehicleParachute", 2048);
    }));
    assert!(!decides(&ini, |section| {
        let _ares = ares_switch::ares_reads();
        section.read_name("VehicleParachute", 2048);
    }));
}

/// A TechnoType key read during the passes reaches the switch through
/// `NPatchInputs`, before the projection runs.
#[test]
fn a_pass_key_turns_npatch_on() {
    let ini = IniFile::from_str("[TANK]\nAIBuildLimit=2\n");
    let (_, reads) = ares_switch::record(|| {
        npatch_switch::npatch_read(|| {
            ini.section("TANK")
                .unwrap()
                .read_int("AIBuildLimit", i32::MAX);
        });
    });
    let projection = ares_switch::record(|| {}).1;
    assert!(
        npatch_switch::NPatchInputs::from_passes(&reads, Default::default())
            .decide(None, &projection)
    );
}

/// `[General] NPatch=yes` is the explicit master switch: with no NPatch-only
/// key anywhere, every NPatch path still turns on.
#[test]
fn npatch_global_yes_turns_npatch_on() {
    let rules = RuleSet::from_ini(&IniFile::from_str("[General]\nNPatch=yes\n")).unwrap();
    assert!(rules.npatch());
    assert!(rules.npatch_fixes());
}

/// An individual NPatch feature key turns its own extension paths on, but not
/// the NPExt bug-fix chains.
#[test]
fn a_feature_key_does_not_turn_the_fixes_on() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nDisableParallelAIQueues=yes\n",
    ))
    .unwrap();
    assert!(rules.npatch());
    assert!(!rules.npatch_fixes());
}

/// The launch setting On plays every NPatch feature and fix even with no
/// data key, and overrides `[General] NPatch=no`.
#[test]
fn the_launch_setting_on_forces_features_and_fixes() {
    for ini in ["[General]\n", "[General]\nNPatch=no\n"] {
        let rules = rules_with(ini, NpatchSetting::On);
        assert!(rules.npatch(), "{ini}");
        assert!(rules.npatch_fixes(), "{ini}");
    }
}

/// The launch setting Off ignores the global key and every feature key.
#[test]
fn the_launch_setting_off_ignores_all_npatch_data() {
    let rules = rules_with(
        "[General]\nNPatch=yes\nVehicleParachute=PARACH\n\
         DisableParallelAIQueues=yes\n",
        NpatchSetting::Off,
    );
    assert!(!rules.npatch());
    assert!(!rules.npatch_fixes());
}

/// The launch setting changes what a match plays without changing a byte of
/// its rules, so the config hash must differ from Auto's.
#[test]
fn the_launch_setting_changes_the_config_hash() {
    let auto = rules_with("[General]\n", NpatchSetting::Auto);
    let on = rules_with("[General]\n", NpatchSetting::On);
    assert!(!auto.npatch());
    assert!(on.npatch());
    assert_ne!(auto.simulation_config_hash(), on.simulation_config_hash());
}

/// The fixes change what a match plays, so a game with the same files and
/// feature key but no master switch must not agree with one that has it.
#[test]
fn the_fix_master_switch_changes_the_config_hash() {
    let mut feature = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nDisableParallelAIQueues=yes\n",
    ))
    .unwrap();
    let hash = feature.simulation_config_hash();
    feature.set_npatch_fixes_for_test(true);
    assert_ne!(hash, feature.simulation_config_hash());
}

/// An explicit `no` keeps NPatch off even when an NPatch-only key would
/// otherwise turn it on.
#[test]
fn npatch_global_no_overrides_an_npatch_key() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nNPatch=no\nDisableParallelAIQueues=yes\n",
    ))
    .unwrap();
    assert!(!rules.npatch());
    assert!(!rules.npatch_fixes());
}

/// Stock rules hold none of the extension keys, so a retail game plays as
/// gamemd.
#[test]
fn stock_rules_keep_npatch_off() {
    assert!(
        !RuleSet::from_ini(&IniFile::from_str("[General]\nParachute=PARACH\n"))
            .unwrap()
            .npatch()
    );
    assert!(!RuleSet::from_ini(&IniFile::from_str("")).unwrap().npatch());
}

/// A save carries the pass keys with the processed rules, so a loaded game
/// reaches the same answer from the same files.
#[test]
fn saved_npatch_inputs_restore_the_switch() {
    use crate::rules::native_processing::RulesLayerStack;
    use crate::util::save_codec;

    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Animations]\n0=VEHCHUTE\n[General]\nVehicleParachute=VEHCHUTE\n",
    ))
    .process_with_fixed_art(&IniFile::from_str("[VEHCHUTE]\nRate=900\n"))
    .unwrap();
    let bytes = save_codec::serialize(&processed).unwrap();
    let restored =
        RuleSet::from_processed_rules(&save_codec::deserialize(&bytes).unwrap()).unwrap();
    assert!(restored.npatch());
    assert_eq!(
        restored.general.vehicle_parachute.as_deref(),
        Some("VEHCHUTE")
    );
}

/// A save carries the player's launch setting with the processed rules, so
/// a loaded game keeps On even where the data says no.
#[test]
fn a_save_restores_the_launch_setting() {
    use crate::util::save_codec;

    let processed = RulesLayerStack::new(IniFile::from_str("[General]\nNPatch=no\n"))
        .with_npatch_setting(NpatchSetting::On)
        .process()
        .unwrap();
    let bytes = save_codec::serialize(&processed).unwrap();
    let restored =
        RuleSet::from_processed_rules(&save_codec::deserialize(&bytes).unwrap()).unwrap();
    assert!(restored.npatch());
    assert!(restored.npatch_fixes());
}

/// A retail skirmish map holds no key that only an NPatch reader reads, so the
/// switch stays off where Ares stays off.
#[test]
fn retail_data_keeps_npatch_off() {
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    use crate::rules::retail_ini_fixture::{retail_assets, retail_rules_owner};

    let Some((root, mut assets)) = retail_assets() else {
        return;
    };
    assets.register_neutral_archives().unwrap();
    let map =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, "XMP03T4.MAP", &assets)
            .unwrap();
    let roster = crate::rules::retail_sources::select_ini(&assets, "MPModesMD.ini").unwrap();
    let modes = crate::skirmish_modes::skirmish_modes_from_selected_ini(
        &assets,
        &roster.ini,
        &roster.source.source_archive,
    )
    .unwrap();
    let mode = modes.first().expect("retail modes");
    let mode_ini =
        crate::rules::retail_sources::select_mode_ini(&assets, Some(&mode.override_file)).unwrap();
    let mut owner: NativeRulesProcessOwner = retail_rules_owner(&assets).with_ares_mix(false);
    let rules = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(mode_ini.as_ref()),
            &map.map.ini,
        )
        .unwrap()
        .into_parts()
        .0;
    assert!(!rules.ares(), "{}", mode.override_file);
    assert!(!rules.npatch(), "{}", mode.override_file);
}
