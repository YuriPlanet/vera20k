//! Immutable lexical audio definitions selected once for a process.
//!
//! Init_Game reads SOUNDMD at 52C763/52C796, then EVAMD at 52C843/52C8A0.
//! The ordinary shell/scenario loop at 48CCD8..48CFAA does not repeat these
//! reads. Neither reader opens the RA2 base INI. Sources are selected through
//! the shared AssetManager/physical byte parser, independently of an audio
//! device or sample index. Lexical sample names are not native admitted slots.
//!
//! Missing inputs retain VERA's tolerant empty-catalog policy; native startup
//! fails instead. Evidence and bounds: tools/audio_catalog_owner.md.

use std::sync::Arc;

use crate::assets::asset_manager::AssetManager;
use crate::rules::retail_sources::{IniSourceIdentity, select_ini};
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};

#[derive(Debug)]
pub struct AudioDefinitions {
    sounds: Arc<SoundRegistry>,
    eva: Arc<EvaRegistry>,
    sound_source: Option<IniSourceIdentity>,
    eva_source: Option<IniSourceIdentity>,
}

impl AudioDefinitions {
    pub fn select(assets: &AssetManager) -> Self {
        let sound = select_ini(assets, "soundmd.ini")
            .map_err(|error| log::warn!("Audio definitions: {error}"))
            .ok();
        let sounds = Arc::new(
            sound
                .as_ref()
                .map_or_else(SoundRegistry::default, |selected| {
                    SoundRegistry::from_ini(&selected.ini)
                }),
        );
        let eva = select_ini(assets, "evamd.ini")
            .map_err(|error| log::warn!("Audio definitions: {error}"))
            .ok();
        let eva_registry = eva.as_ref().map_or_else(EvaRegistry::default, |selected| {
            EvaRegistry::from_ini(&selected.ini)
        });
        Self {
            sounds,
            eva: Arc::new(eva_registry),
            sound_source: sound.map(|selected| selected.source),
            eva_source: eva.map(|selected| selected.source),
        }
    }

    pub fn sounds(&self) -> &Arc<SoundRegistry> {
        &self.sounds
    }

    pub fn eva(&self) -> &Arc<EvaRegistry> {
        &self.eva
    }

    pub fn sound_source(&self) -> Option<&IniSourceIdentity> {
        self.sound_source.as_ref()
    }

    pub fn eva_source(&self) -> Option<&IniSourceIdentity> {
        self.eva_source.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::process_owner::NativeRulesProcessOwner;
    use crate::rules::sound_ini::EvaSide;
    use crate::util::sha256::sha256_hex;

    #[test]
    fn selected_soundmd_is_the_only_sound_source_and_preserves_reader_order() {
        let directory = TestDirectory::new("fixed-soundmd");
        let soundmd =
            b"[Defaults]\nVolume=37\n[SoundList]\n0=SoundA\n1=sounda\n[SoundA]\nSounds=a.wav\n";
        directory.write("soundmd.ini", soundmd);
        directory.write("sound.ini", b"[SoundList]\n0=SoundA\n1=SoundB\n[SoundA]\nSounds=old.wav\n[SoundB]\nSounds=base.wav\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let definitions = AudioDefinitions::select(&assets);
        let sound = definitions.sounds().get("SoundA").unwrap();
        assert_eq!(sound.sounds, ["a.wav", "a.wav"]);
        assert_eq!(sound.volume, 37);
        assert!(definitions.sounds().get("SoundB").is_none());
        let source = definitions.sound_source().unwrap();
        assert_eq!(source.logical_name, "soundmd.ini");
        assert_eq!(source.payload_len, soundmd.len());
        assert_eq!(source.source_sha256, sha256_hex(soundmd));
    }

    /// Migrated from the app loader: the selected EVAMD source neither inherits
    /// a missing side column nor imports an EVA.INI-only event.
    #[test]
    fn selected_evamd_ignores_base_rows_and_side_columns() {
        let directory = TestDirectory::new("fixed-evamd");
        let evamd = b"[DialogList]\n0=EVA_UnitLost\n[EVA_UnitLost]\nAllied=ceva064\n";
        directory.write("evamd.ini", evamd);
        directory.write("eva.ini", b"[DialogList]\n0=EVA_UnitLost\n1=EVA_Ra2Only\n[EVA_UnitLost]\nAllied=old064\nRussian=old064r\n[EVA_Ra2Only]\nAllied=ra2only\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let definitions = AudioDefinitions::select(&assets);
        assert_eq!(definitions.eva().len(), 1);
        assert_eq!(
            definitions.eva().get("EVA_UnitLost", EvaSide::Allied),
            Some("ceva064")
        );
        assert_eq!(
            definitions.eva().get("EVA_UnitLost", EvaSide::Russian),
            None
        );
        assert!(definitions.eva().entry("EVA_Ra2Only").is_none());
        let source = definitions.eva_source().unwrap();
        assert_eq!(source.logical_name, "evamd.ini");
        assert_eq!(source.payload_len, evamd.len());
        assert_eq!(source.source_sha256, sha256_hex(evamd));
    }

    #[test]
    fn base_ini_only_keeps_tolerated_missing_yr_catalogs_empty() {
        let directory = TestDirectory::new("base-audio-only");
        directory.write("sound.ini", b"[SoundList]\n0=Base\n[Base]\nSounds=base\n");
        directory.write(
            "eva.ini",
            b"[DialogList]\n0=EVA_Base\n[EVA_Base]\nAllied=base\n",
        );
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let definitions = AudioDefinitions::select(&assets);
        assert!(definitions.sounds().is_empty());
        assert!(definitions.eva().is_empty());
        assert!(definitions.sound_source().is_none());
        assert!(definitions.eva_source().is_none());
    }

    #[test]
    fn selected_definitions_use_physical_byte_widening_for_both_readers() {
        let directory = TestDirectory::new("audio-byte-reader");
        directory.write(
            "soundmd.ini",
            b"[SoundList]\n0=Voice\n[Voice]\nSounds=voice\xff\n",
        );
        directory.write(
            "evamd.ini",
            b"[DialogList]\n0=EVA_Test\n[EVA_Test]\nAllied=voice\xff\n",
        );
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let definitions = AudioDefinitions::select(&assets);
        assert_eq!(
            definitions.sounds().get("Voice").unwrap().sounds,
            ["voice\u{ff}"]
        );
        assert_eq!(
            definitions.eva().get("EVA_Test", EvaSide::Allied),
            Some("voice\u{ff}")
        );
    }

    #[test]
    fn retained_definitions_are_shared_through_rules_rebuild_and_failure() {
        let directory = TestDirectory::new("retained-audio");
        directory.write("soundmd.ini", b"[SoundList]\n0=Hull\n[Hull]\nSounds=old\n");
        directory.write(
            "evamd.ini",
            b"[DialogList]\n0=EVA_Test\n[EVA_Test]\nAllied=old\n",
        );
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let definitions = AudioDefinitions::select(&assets);
        let root = IniFile::from_str("[VehicleTypes]\n0=SHIP\n[SHIP]\nSinkingSound=Hull\n");
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            IniFile::empty(),
            Arc::clone(definitions.sounds()),
            Arc::clone(definitions.eva()),
        )
        .unwrap();
        directory.write(
            "soundmd.ini",
            b"[SoundList]\n0=Other\n[Other]\nSounds=new\n",
        );
        directory.write(
            "evamd.ini",
            b"[DialogList]\n0=EVA_Test\n[EVA_Test]\nAllied=new\n",
        );
        for _ in 0..2 {
            let (rules, _, _, _) = owner
                .load_scenario(
                    crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                    &IniFile::empty(),
                )
                .unwrap()
                .into_parts();
            assert!(Arc::ptr_eq(definitions.sounds(), owner.fixed_sounds()));
            assert_eq!(
                rules.object("SHIP").unwrap().sinking_sound.as_deref(),
                Some("Hull")
            );
        }
        assert!(
            owner
                .load_scenario(
                    crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                    &IniFile::from_str("[Tiberiums]\n-1=INVALID\n")
                )
                .is_err()
        );
        assert!(Arc::ptr_eq(definitions.sounds(), owner.fixed_sounds()));
        assert_eq!(definitions.sounds().get("Hull").unwrap().sounds, ["old"]);
        assert_eq!(
            definitions.eva().get("EVA_Test", EvaSide::Allied),
            Some("old")
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn superweapon_money_eva_reads_retain_catalog_matches_through_layers_and_save() {
        use crate::rules::native_processing::ProcessedRulesLayers;
        use crate::rules::process_owner::NativeScenarioRulesPrefix;

        let long_name = "E".repeat(160);
        let catalog = Arc::new(EvaRegistry::from_ini(&IniFile::from_str(&format!(
            "[DialogList]\n0=EVA_InsufficientFunds\n1=Known\n2={long_name}\n"
        ))));
        for (root_cue, mode_cue, map_cue, expected) in [
            ("", "", "", Some("EVA_InsufficientFunds")),
            ("Known", "Missing", "", Some("Known")),
            ("Known", "", "Missing", Some("Known")),
            ("Known", "none", "Missing", None),
            ("Known", "<none>", "", None),
            ("Known", "none", "Known", Some("Known")),
            ("Known", "", long_name.as_str(), Some(long_name.as_str())),
        ] {
            let root = IniFile::from_str(&format!(
                "[SuperWeaponTypes]\n0=Paid\n[Paid]\nType=PsychicReveal\nMoney.Amount=-500\nEVA.InsufficientFunds={root_cue}\n"
            ));
            let mode = IniFile::from_str(&format!("[Paid]\nEVA.InsufficientFunds={mode_cue}\n"));
            let map = IniFile::from_str(&format!("[Paid]\nEVA.InsufficientFunds={map_cue}\n"));
            let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
                root,
                None,
                IniFile::empty(),
                Arc::default(),
                Arc::clone(&catalog),
            )
            .unwrap();
            let (rules, _, _, _) = owner
                .load_scenario(NativeScenarioRulesPrefix::NonCampaign(Some(&mode)), &map)
                .unwrap()
                .into_parts();
            assert_eq!(
                rules
                    .super_weapon("Paid")
                    .unwrap()
                    .insufficient_funds_eva
                    .as_deref(),
                expected,
                "root={root_cue}, mode={mode_cue}, map={map_cue}"
            );
            let saved =
                crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
            let restored: ProcessedRulesLayers =
                crate::util::save_codec::deserialize(&saved).unwrap();
            let loaded = owner.project_saved_scenario_rules(&restored).unwrap();
            assert_eq!(
                loaded
                    .super_weapon("Paid")
                    .unwrap()
                    .insufficient_funds_eva
                    .as_deref(),
                expected
            );
            assert_eq!(
                loaded.simulation_config_hash(),
                rules.simulation_config_hash()
            );
        }
        let root = IniFile::from_str(
            "[SuperWeaponTypes]\n0=Paid\n[Paid]\nType=PsychicReveal\nMoney.Amount=-500\nEVA.InsufficientFunds=Missing\n",
        );
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            IniFile::empty(),
            Arc::default(),
            Arc::default(),
        )
        .unwrap();
        let (rules, _, _, _) = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap()
            .into_parts();
        assert!(
            rules
                .super_weapon("Paid")
                .unwrap()
                .insufficient_funds_eva
                .is_none()
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn superweapon_money_eva_keeps_startup_catalog_after_files_change() {
        use crate::rules::process_owner::NativeScenarioRulesPrefix;
        use crate::rules::retail_sources::RetailRulesSources;

        let directory = TestDirectory::new("money-fixed-eva");
        directory.write("rulesmd.ini", b"[SuperWeaponTypes]\n0=Paid\n[Paid]\nType=PsychicReveal\nMoney.Amount=-500\nEVA.InsufficientFunds=Known\n");
        directory.write("artmd.ini", b"; fixed art\n");
        directory.write(
            "evamd.ini",
            b"[DialogList]\n0=EVA_InsufficientFunds\n1=Known\n",
        );
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let definitions = AudioDefinitions::select(&assets);
        let (startup, _, mut owner) = RetailRulesSources::select(&assets)
            .unwrap()
            .into_startup(
                Arc::clone(definitions.sounds()),
                Arc::clone(definitions.eva()),
            )
            .unwrap()
            .into_parts();
        assert_eq!(
            startup
                .unwrap()
                .super_weapon("Paid")
                .unwrap()
                .insufficient_funds_eva
                .as_deref(),
            Some("Known")
        );
        directory.write("evamd.ini", b"[DialogList]\n0=Missing\n");
        let (rules, _, _, _) = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::from_str("[Paid]\nEVA.InsufficientFunds=Missing\n"),
            )
            .unwrap()
            .into_parts();
        assert_eq!(
            rules
                .super_weapon("Paid")
                .unwrap()
                .insufficient_funds_eva
                .as_deref(),
            Some("Known")
        );
        assert!(definitions.eva().entry("Missing").is_none());
    }
}
