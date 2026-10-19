//! Command-bar Rules references through the existing pass and fixed-catalog owners.
//! Original gamemd SHA2561cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c:
//! constructor665892/665898=-1; AudioVisual669423..6694B6 reads GUIOpenSound
//! then GUICloseSound with ReadString128/VocFind7514D0/current-ID retention.
//! SidebarInput6D091A/6D0860 consumes them separately from shell combo cues.
//! Shared source/lifetime evidence: tools/audio_catalog_owner.md. These are
//! production-reader and restore regressions, not native audio goldens.

use std::sync::Arc;

use super::{GeneralRules, RuleSet};
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::sound_ini::SoundRegistry;
use crate::util::save_codec;

const SOUND_CATALOG: &str = "\
[SoundList]
0=RootOpen
1=RootClose
2=LanguageOpen
3=ModeClose
4=MapOpen
5=MapClose
6=ComboOpen
7=ComboClose
8=ScoreEmblemSoundLoop
[Unregistered]
Sounds=unregistered_sample
";

#[test]
fn command_bar_sounds_start_invalid_and_wait_for_registered_binding() {
    let defaults = GeneralRules::default();
    assert_eq!(defaults.gui_open_sound(), None, "constructor665892=-1");
    assert_eq!(defaults.gui_close_sound(), None, "constructor665898=-1");
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(SOUND_CATALOG));
    for text in [
        "",
        "[AudioVisual]\nGUIOpenSound=\nGUICloseSound=\n",
        "[AudioVisual]\nGUIOpenSound=Unknown\nGUICloseSound=Unknown\n",
        "[AudioVisual]\nGUIOpenSound=Unregistered\nGUICloseSound=Unregistered\n",
    ] {
        let processed = RulesLayerStack::new(IniFile::from_str(text))
            .process()
            .unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_type_sound_references(processed.ini(), &sounds);
        assert_eq!(rules.general.gui_open_sound(), None, "{text:?}");
        assert_eq!(rules.general.gui_close_sound(), None, "{text:?}");
    }

    let processed = RulesLayerStack::new(IniFile::from_str(
        "[AudioVisual]\nGUIOpenSound=RootOpen\nGUICloseSound=RootClose\n",
    ))
    .process()
    .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(rules.general.gui_open_sound(), None, "no unbound raw ID");
    assert_eq!(rules.general.gui_close_sound(), None, "no unbound raw ID");
    rules.bind_type_sound_references(processed.ini(), &sounds);
    assert_eq!(rules.general.gui_open_sound(), Some("RootOpen"));
    assert_eq!(rules.general.gui_close_sound(), Some("RootClose"));
}

#[test]
fn campaign_score_animation_sound_uses_the_same_audiovisual_binding() {
    // `Dialog0x103__DlgProc` 0x4EC plays Rules+ScoreAnimSound through
    // VocClass__PlayGlobal; the key is in [AudioVisual].
    assert_eq!(GeneralRules::default().score_anim_sound(), None);
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(SOUND_CATALOG));
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[AudioVisual]\nScoreAnimSound=ScoreEmblemSoundLoop\n",
    ))
    .process()
    .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(rules.general.score_anim_sound(), None, "unbound raw ID");
    rules.bind_type_sound_references(processed.ini(), &sounds);
    assert_eq!(
        rules.general.score_anim_sound(),
        Some("ScoreEmblemSoundLoop")
    );

    let processed =
        RulesLayerStack::new(IniFile::from_str("[AudioVisual]\nScoreAnimSound=Unknown\n"))
            .process()
            .unwrap();
    let mut unknown = RuleSet::from_processed_rules(&processed).unwrap();
    unknown.bind_type_sound_references(processed.ini(), &sounds);
    assert_eq!(unknown.general.score_anim_sound(), None);
}

#[test]
fn command_bar_sounds_follow_language_mode_map_order_with_prior_id_retention() {
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(SOUND_CATALOG));
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[AudioVisual]\nGUIOpenSound=RootOpen\nGUICloseSound=RootClose\n",
    ));
    for (kind, text, open, close) in [
        (
            RulesLayerKind::LangRule,
            "[AudioVisual]\nGUIOpenSound=LanguageOpen\nGUICloseSound=Unknown\n",
            "LanguageOpen",
            "RootClose",
        ),
        (
            RulesLayerKind::GameMode,
            "[AudioVisual]\nGUIOpenSound=\nGUICloseSound=ModeClose\n",
            "LanguageOpen",
            "ModeClose",
        ),
        (
            RulesLayerKind::Scenario,
            "[AudioVisual]\nGUIOpenSound=mApOpEn\nGUICloseSound=Unregistered\n",
            "MapOpen",
            "ModeClose",
        ),
        (
            RulesLayerKind::Scenario,
            "[AudioVisual]\nOtherKey=MapClose\n",
            "MapOpen",
            "ModeClose",
        ),
    ] {
        layers.push(kind, IniFile::from_str(text));
        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_type_sound_references(processed.ini(), &sounds);
        assert_eq!(rules.general.gui_open_sound(), Some(open), "{text:?}");
        assert_eq!(rules.general.gui_close_sound(), Some(close), "{text:?}");
    }
}

#[test]
fn command_bar_keys_are_exact_case_and_independent_of_combo_references() {
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(SOUND_CATALOG));
    for (later, expected) in [
        (
            "[AudioVisual]\nguiopensound=MapOpen\nguiclosesound=MapClose\n",
            ("RootOpen", "RootClose"),
        ),
        (
            "[audiovisual]\nGUIOpenSound=MapOpen\nGUICloseSound=MapClose\n",
            ("RootOpen", "RootClose"),
        ),
        (
            "[General]\nGUIOpenSound=MapOpen\nGUICloseSound=MapClose\n",
            ("RootOpen", "RootClose"),
        ),
        (
            "[AudioVisual]\nGUIOpenSound=mapopen\nGUICloseSound=MAPCLOSE\n",
            ("MapOpen", "MapClose"),
        ),
    ] {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            // The existing combo projection requires a nonempty General section;
            // exercise its ordinary reader without changing that owner.
            "[General]\nFlightLevel=500\n[AudioVisual]\nGUIOpenSound=RootOpen\nGUICloseSound=RootClose\n\
             GUIComboOpenSound=ComboOpen\nGUIComboCloseSound=ComboClose\n",
        ));
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(later));
        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_type_sound_references(processed.ini(), &sounds);
        assert_eq!(
            rules.general.gui_open_sound(),
            Some(expected.0),
            "{later:?}"
        );
        assert_eq!(
            rules.general.gui_close_sound(),
            Some(expected.1),
            "{later:?}"
        );
        assert_eq!(
            rules.general.gui_combo_open_sound.as_deref(),
            Some("ComboOpen")
        );
        assert_eq!(
            rules.general.gui_combo_close_sound.as_deref(),
            Some("ComboClose")
        );
    }
}

#[test]
fn command_bar_sound_readstring_capacity_applies_before_trimming() {
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(SOUND_CATALOG));
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[AudioVisual]\nGUIOpenSound=RootOpen\nGUICloseSound=RootClose\n",
    ));
    // The physical INI parser preserves these internal spaces. ReadString's
    // 127-byte payload cuts the open value just before X; trim then resolves
    // MapOpen. The close value keeps X and remains an unregistered name.
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(&format!(
            "[AudioVisual]\nGUIOpenSound=MapOpen{}X\nGUICloseSound=MapClose{}X\n",
            " ".repeat(120),
            " ".repeat(118),
        )),
    );
    let processed = layers.process().unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.bind_type_sound_references(processed.ini(), &sounds);
    assert_eq!(rules.general.gui_open_sound(), Some("MapOpen"));
    assert_eq!(rules.general.gui_close_sound(), Some("RootClose"));
}

#[test]
fn command_bar_sound_history_survives_saved_rules_and_normal_restore_rebinding() {
    let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(SOUND_CATALOG)));
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str("[AudioVisual]\nGUIOpenSound=RootOpen\nGUICloseSound=RootClose\n"),
        Some(IniFile::from_str("[AudioVisual]\nGUICloseSound=MapClose\n")),
        IniFile::empty(),
        Arc::clone(&sounds),
        Default::default(),
    )
    .unwrap();
    let mode = IniFile::from_str("[AudioVisual]\nGUIOpenSound=\n");
    let map = IniFile::from_str("[AudioVisual]\nGUIOpenSound=Unknown\nGUICloseSound=Unknown\n");
    let (rules, _, _, _) = owner
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(Some(&mode)), &map)
        .unwrap()
        .into_parts();
    assert_eq!(rules.general.gui_open_sound(), Some("RootOpen"));
    assert_eq!(rules.general.gui_close_sound(), Some("MapClose"));
    let saved = owner.saved_scenario_rules().unwrap();
    assert_eq!(
        saved
            .ini()
            .section("AudioVisual")
            .unwrap()
            .read_name("GUIOpenSound", 0x80),
        Some("Unknown")
    );
    let bytes = save_codec::serialize(saved).unwrap();
    let saved: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    // SavedScenarioResources carries this same existing processed authority;
    // the app's load route projects it against the current process catalog.
    let restored = owner.project_saved_scenario_rules(&saved).unwrap();
    owner.install_saved_scenario_rules(saved);
    assert!(Arc::ptr_eq(owner.fixed_sounds(), &sounds));
    assert_eq!(restored.general.gui_open_sound(), Some("RootOpen"));
    assert_eq!(restored.general.gui_close_sound(), Some("MapClose"));
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn physical_command_bar_sounds_use_current_battle_rules_and_fixed_sound_catalog() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let Some(audio) = crate::rules::retail_ini_fixture::retail_audio_definitions() else {
        return;
    };
    // This fixture loads physical RULESMD, optional LANGRULE, MPBattleMD and
    // the actual Hills map through NativeRulesProcessOwner, including binding
    // against the process-selected fixed SOUNDMD rather than an old asset CLI.
    assert_eq!(retail.rules.general.gui_open_sound(), Some("MenuACBOpen"));
    assert_eq!(retail.rules.general.gui_close_sound(), Some("MenuACBClose"));
    assert_ne!(
        retail.rules.general.gui_open_sound(),
        retail.rules.general.gui_close_sound()
    );
    for sound in [
        retail.rules.general.gui_open_sound().unwrap(),
        retail.rules.general.gui_close_sound().unwrap(),
    ] {
        let entry = audio.sounds().get(sound).expect("bound fixed SOUNDMD ID");
        assert_eq!(entry.sounds, ["uacbopen"]);
        // Authored Volume=70 feeds the existing native linear converter;
        // this percentage is only its display copy, not a new numeric golden.
        assert_eq!(entry.volume, 70);
        assert_eq!(entry.control, 0);
        assert_eq!(
            entry.type_flags,
            crate::rules::sound_ini::sound_type::SCREEN
                | crate::rules::sound_ini::sound_type::UNSHROUD
        );
        assert_eq!((entry.priority, entry.limit, entry.range), (2, 5, 10));
        assert_eq!((entry.loop_count, entry.attack, entry.decay), (0, 0, 0));
        assert_eq!(
            (entry.delay_ms, entry.fshift, entry.vshift),
            ((0, 0), (0, 0), 0)
        );
    }
}
