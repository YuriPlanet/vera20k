//! The per-game Kratos switch (`rules::kratos_switch`): a key only a Kratos
//! reader reads turns Kratos on, the same key read by a stock, Ares or Phobos
//! reader does not, and retail data leaves the switch off.

use crate::rules::ares_switch;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::kratos_switch;

/// The switch's answer for one recorded projection over `read`, with no pass
/// read of its own.
fn decides(ini: &IniFile, read: impl FnOnce(&IniSection)) -> bool {
    let (_, reads) = ares_switch::record(|| read(ini.section("General").unwrap()));
    kratos_switch::KratosInputs::default().decide(&reads)
}

/// A key only a Kratos reader found turns Kratos on. The same key read by a
/// stock, an Ares or a Phobos reader is theirs, since Kratos readers parse
/// with their readers and a key two extensions read turns neither on.
#[test]
fn a_kratos_only_key_turns_kratos_on() {
    let ini = IniFile::from_str("[General]\nKratosProbe=yes\n");
    assert!(decides(&ini, |section| {
        kratos_switch::kratos_read(|| section.read_bool("KratosProbe", false));
    }));
    assert!(!decides(&ini, |section| {
        section.read_bool("KratosProbe", false);
    }));
    assert!(!decides(&ini, |section| {
        let _ares = ares_switch::ares_reads();
        section.read_bool("KratosProbe", false);
    }));
    assert!(!decides(&ini, |section| {
        crate::rules::phobos_switch::phobos_read(|| section.read_bool("KratosProbe", false));
    }));
}

/// An Image or Fearless override in the map changes which ART Kratos reads.
#[test]
fn only_an_eligible_infantrys_final_image_can_enable_crawling_flh() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[InfantryTypes]\n0=GI\n1=HERO\n[VehicleTypes]\n0=TANK\n\
         [GI]\nImage=OLD\n[HERO]\nFearless=no\n[TANK]\nStrength=100",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[GI]\nImage=NEW\n[HERO]\nFearless=yes"),
    );
    let art = IniFile::from_str(
        "[OLD]\nPrimaryCrawlingFLH=1,2,3\n[HERO]\nPrimaryCrawlingFLH=4,5,6\n\
         [TANK]\nSecondaryCrawlingFLH=7,8,9\n[NEW]\nEliteSecondaryCrawlingFLH=9,8,7",
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    assert!(!RuleSet::from_processed_rules(&processed).unwrap().kratos());
}

/// A retail skirmish map holds no key that only a Kratos reader reads, so the
/// switch stays off where Ares and Phobos stay off.
#[test]
fn retail_data_keeps_kratos_off() {
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
    assert!(!rules.kratos(), "{}", mode.override_file);
}

/// The player's setting overrides the key answer: On plays Kratos without a
/// key (its defaults too), Off keeps a game's own key from turning it on, and
/// Auto is the key answer.
#[test]
fn the_player_setting_overrides_the_key_answer() {
    use crate::rules::ares_switch::KeyReads;
    use crate::util::kratos_launch::KratosSetting;

    let ini = IniFile::from_str("[General]\nKratosProbe=yes\n");
    let (_, reads) = ares_switch::record(|| {
        kratos_switch::kratos_read(|| {
            ini.section("General")
                .unwrap()
                .read_bool("KratosProbe", false);
        });
    });
    let empty = KeyReads::default();
    assert!(kratos_switch::KratosInputs::from_passes(&reads, KratosSetting::Auto).decide(&empty));
    assert!(kratos_switch::KratosInputs::from_passes(&reads, KratosSetting::On).decide(&empty));
    assert!(!kratos_switch::KratosInputs::from_passes(&reads, KratosSetting::Off).decide(&empty));
    assert!(
        kratos_switch::KratosInputs::from_passes(&empty_reads(), KratosSetting::On).decide(&empty)
    );
    assert!(!kratos_switch::KratosInputs::default().decide(&empty));
}

/// The reads of a projection that found no key.
fn empty_reads() -> crate::rules::ares_switch::KeyReads {
    let (_, reads) = ares_switch::record(|| {});
    reads
}

#[test]
fn replay_identity_covers_effective_kratos_and_its_firing_offsets() {
    use crate::rules::native_processing::RulesLayerStack;
    use crate::rules::ruleset::RuleSet;
    use crate::util::kratos_launch::KratosSetting::{Auto, Off, On};

    let source = IniFile::from_str("[InfantryTypes]\n0=GI\n[GI]\nPrimary=GUN\nFearless=no\n");
    let hash = |setting, art: &str| {
        let processed = RulesLayerStack::new(source.clone())
            .with_kratos_setting(setting)
            .process_with_fixed_art(&IniFile::from_str(art))
            .unwrap();
        RuleSet::from_processed_rules(&processed)
            .unwrap()
            .simulation_config_hash()
    };
    let plain = "[GI]\nPrimaryFireFLH=12,0,60\n";
    assert_eq!(hash(Auto, plain), hash(Off, plain));
    assert_ne!(hash(On, plain), hash(Off, plain));
    let first = "[GI]\nPrimaryCrawlingFLH=17,0,20\n";
    let second = "[GI]\nPrimaryCrawlingFLH=57,0,20\n";
    assert_eq!(hash(Auto, first), hash(On, first));
    assert_ne!(hash(On, first), hash(On, second));
}

#[test]
fn saved_kratos_inputs_restore_the_switch_and_infantry_offsets() {
    use crate::rules::native_processing::RulesLayerStack;
    use crate::rules::ruleset::RuleSet;
    use crate::util::{kratos_launch::KratosSetting, save_codec};

    let ini = IniFile::from_str("[InfantryTypes]\n0=GI\n[GI]\nPrimary=GUN\n");
    let art = IniFile::from_str("[GI]\nPrimaryFireFLH=32,8,60\nPrimaryCrawlingFLH=17,-2,12\n");
    for (setting, plays) in [(KratosSetting::Auto, true), (KratosSetting::Off, false)] {
        let processed = RulesLayerStack::new(ini.clone())
            .with_kratos_setting(setting)
            .process_with_fixed_art(&art)
            .unwrap();
        let bytes = save_codec::serialize(&processed).unwrap();
        let restored =
            RuleSet::from_processed_rules(&save_codec::deserialize(&bytes).unwrap()).unwrap();
        assert_eq!(restored.kratos(), plays);
        let offsets = restored.object("GI").unwrap().kratos_crawling();
        assert_eq!(offsets.flh(0, false, true), [17, -2, 12].into());
        assert_eq!(offsets.flh(0, false, false), [32, 8, 60].into());
    }
}
