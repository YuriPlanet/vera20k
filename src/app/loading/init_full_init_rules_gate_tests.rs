//! Which Rules a scenario's Full_Init reads, through `loading::init`: a
//! generated map's rules read before generation and carried once through a
//! failure, a campaign start's named INI on mode zero, and the TMCJ4F read.

use super::*;

#[test]
fn generated_rules_are_read_before_generation_and_carried_once_through_failure() {
    // Unoptimized, the frames from here to RuleSet::project hold about
    // 2.2 MiB of Rules copies, past libtest's 2 MiB thread.
    crate::test_fixture::on_game_stack(generated_rules_read_before_generation_and_carried);
}

fn generated_rules_read_before_generation_and_carried() {
    use crate::app::match_runtime::session::SessionMode;
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::powerups::POWERUP_MONEY;
    use crate::rules::process_owner::NativeRulesProcessOwner;
    use std::sync::Arc;

    struct Progress {
        fail: bool,
    }
    impl crate::app::loading::pump::LoadingProgressSink for Progress {
        fn milestone(&mut self, _: u32) -> anyhow::Result<()> {
            anyhow::ensure!(!self.fail, "stop after generated-map preparation");
            Ok(())
        }
    }
    let directory = TestDirectory::new("rmg-early-rules");
    directory.write(
        "temperatmd.ini",
        b"[General]\nClearTile=0\n[TileSet0000]\nFileName=clear\nTilesInSet=1\n",
    );
    directory.write("isotem.pal", &[0; 768]);
    directory.write("unittem.pal", &[0; 768]);
    let Some((_, retail)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    directory.write("aimd.ini", retail.get_ref("AIMD.INI").unwrap());
    directory.write(
        "mode.ini",
        b"[Animations]\n0=MODE\n[Powerups]\nMoney=7,MODE,yes,125\n[Clear]\nWheel=0%\n",
    );
    directory.write(
        "TMCJ4F.INI",
        b"[Animations]\n0=TAIL\n[Powerups]\nMoney=9,TAIL,yes,250\n[Clear]\nWheel=100%\n",
    );
    let options = crate::map::rmg::RmgOptions {
        map_type: 0,
        resources: 0,
        vegetation: 0,
        seed: 4242,
        ..Default::default()
    };
    directory.write("early.sed", &options.to_sed_bytes(false));
    let seed_path = directory.path().join("early.sed");
    let root = IniFile::from_str("[Countries]\n0=C]\n[Animations]\n0=BASE\n[Clear]\nWheel=25%\n");
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        IniFile::empty(),
        Arc::default(),
        Arc::default(),
    )
    .unwrap();
    let mut assets = AssetManager::from_loose_root_for_test(directory.path());
    let initial = load_map_initial_with_assets(
        directory.path().to_owned(),
        &mut assets,
        Some(&mut owner),
        None,
        Some(seed_path.to_str().unwrap()),
        Some("mode.ini"),
        SessionMode::Skirmish,
        crate::rules::retail_sources::AiFile::GameFolder,
        true,
        None,
        &mut Progress { fail: false },
    )
    .unwrap();
    assert!(initial.has_generated_rules());
    assert_eq!(
        initial
            .map_data
            .ini
            .section("C]")
            .unwrap()
            .read_int("TechLevel", -1),
        0
    );
    assert_eq!(
        initial
            .map_data
            .ini
            .section("Basic")
            .unwrap()
            .read_name("Player", 32)
            .as_deref(),
        Some("C]")
    );
    let saved_hash = owner.saved_scenario_rules().unwrap().content_hash();
    let prepared = initial.generated_rules.unwrap();
    assert_eq!(prepared.native.rules().powerups.weights[POWERUP_MONEY], 9);
    assert_eq!(
        prepared.native.rules().anim_type_name_by_index(2),
        Some("TAIL")
    );
    assert_eq!(
        prepared.rules_at_ai_read.as_ref().unwrap().powerups.weights[POWERUP_MONEY],
        7,
        "AI reads before TMCJ4F"
    );
    let (rules, _, _, _, _) = prepared.into_parts();
    assert_eq!(rules.powerups.weights[POWERUP_MONEY], 9);
    assert_eq!(
        owner.saved_scenario_rules().unwrap().content_hash(),
        saved_hash,
        "consuming the carried result does not replay Process"
    );

    // A later admission error does not undo the already reached global read.
    directory.write("TMCJ4F.INI", b"[Powerups]\nMoney=11,MODE,no,333\n");
    let mut assets = AssetManager::from_loose_root_for_test(directory.path());
    assert!(
        load_map_initial_with_assets(
            directory.path().to_owned(),
            &mut assets,
            Some(&mut owner),
            None,
            Some(seed_path.to_str().unwrap()),
            Some("mode.ini"),
            SessionMode::Skirmish,
            crate::rules::retail_sources::AiFile::GameFolder,
            true,
            None,
            &mut Progress { fail: true },
        )
        .is_err()
    );
    assert_eq!(
        owner.saved_scenario_rules().unwrap().powerups().weights[POWERUP_MONEY],
        11
    );
}

fn rules_owner() -> crate::rules::process_owner::NativeRulesProcessOwner {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::process_owner::NativeRulesProcessOwner;
    use std::sync::Arc;

    NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str("[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nTechLevel=1\n"),
        None,
        IniFile::empty(),
        Arc::default(),
        Arc::default(),
    )
    .unwrap()
}

fn color_add(rules: &crate::rules::ruleset::RuleSet) -> [[u8; 3]; 2] {
    [
        rules.color_add.entries()[0].rgb,
        rules.color_add.entries()[1].rgb,
    ]
}

fn native_rules_fixture() -> serde_json::Value {
    crate::test_fixture::json("tools/input_oracle/campaign_native_rules.json").clone()
}

#[test]
fn campaign_start_descriptor_and_rules_owner_skip_a_present_named_ini_on_mode_zero() {
    use crate::app::match_bootstrap::{
        LoadingStartup, MatchSeedClock, MatchSeedSource, prepare_campaign_startup,
    };
    use crate::app::match_runtime::session::SessionMode;
    use crate::assets::asset_manager::AssetManager;
    use crate::map::rmg::{RmgOptions, emit};
    use crate::map::source::{LoadedMapSource, test_support::TestDirectory};
    use crate::rules::campaigns::CampaignRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::movies::MovieRegistry;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;

    struct ControlledClock;
    impl MatchSeedClock for ControlledClock {
        fn low_u32(&mut self) -> u32 {
            73
        }
        fn source(&self) -> MatchSeedSource {
            MatchSeedSource::Controlled
        }
        fn seed_authority_certifying(&self) -> bool {
            true
        }
    }

    let campaigns_ini = IniFile::from_str("[Battles]\n0=ALLIED\n[ALLIED]\nScenario=ALL01UMD.MAP\n");
    let mut campaigns = CampaignRegistry::default();
    campaigns.apply_ini(&campaigns_ini, &MovieRegistry::default());
    let mut clock = ControlledClock;
    let prepared = prepare_campaign_startup(&campaigns, "ALLIED", 1, None, None, &mut clock)
        .expect("campaign selection is admitted");
    let startup = LoadingStartup::Campaign(prepared.clone());
    let map = emit::empty_map_file(&RmgOptions::default(), 8, 8);
    let source = LoadedMapSource::Mix {
        logical_name: prepared.scenario().to_owned(),
        source_archive: "MAPSMD03.MIX".into(),
        entry_id: 1,
        payload_len: 0,
        source_sha256: String::new(),
    };
    let mut accepted_rmg_start_staging = None;
    let descriptor = FreshScenarioLoadContextDescriptor::admit(
        &startup,
        &map,
        &source,
        &mut accepted_rmg_start_staging,
    )
    .unwrap();
    descriptor
        .validate_terminal_transfer(&startup, &source, map.basic.new_ini_format.unwrap_or(0))
        .unwrap();
    let FreshScenarioParts::Campaign {
        startup: campaign, ..
    } = descriptor.into_parts()
    else {
        panic!("campaign startup retains its campaign descriptor");
    };
    assert_eq!(startup.session_mode(), SessionMode::Campaign);

    let directory = TestDirectory::new("campaign-basename-rules-gate");
    directory.write("rulesmd.ini", b"[InfantryTypes]\n0=E1\n[E1]\nTechLevel=1\n");
    directory.write("artmd.ini", b"");
    directory.write(
        "ALL01UMD.INI",
        b"[InfantryTypes]\n0=CAMPAIGNONLY\n[CAMPAIGNONLY]\nStrength=100\nTechLevel=1\n",
    );
    let mut assets = AssetManager::from_loose_root_for_test(directory.path());
    // A Campaign 0 -> 0 start reads the file on Full_Init's second pass.
    let selected = select_campaign_named_ini_for_full_init(
        &mut assets,
        Some(&campaign),
        startup.session_mode(),
    )
    .unwrap();
    assert!(selected.is_some());
    let loaded = rules_owner()
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(selected.as_ref()),
            &map.ini,
        )
        .unwrap();
    let (_, _, _, receipt) = loaded.into_parts();
    assert_eq!(receipt.pre_reset_event_count(), 1);

    let fixture = native_rules_fixture();
    let campaign_cases = fixture["campaign_named_ini_cases"]
        .as_array()
        .expect("campaign gate rows");
    for case in campaign_cases {
        let input = &case["input"];
        let file = &input["file"];
        let case_name = input["case"].as_str().expect("case name");
        let filename = file["filename"].as_str().expect("scenario basename");
        let raw_ini = file["raw_ini"].as_str().expect("controlled INI bytes");
        assert!(file["present"].as_bool().expect("file presence"));
        assert_eq!(input["scenario_file"], campaign.scenario());

        let entry_raw = input["entry_mode"].as_i64().unwrap() as i32;
        let current_raw = input["current_mode"].as_i64().unwrap() as i32;
        assert_eq!(
            input["cached_entry_flag"].as_i64().unwrap() == 1,
            entry_raw == 0,
            "Full_Init sete cache must reflect the entry mode"
        );
        assert_eq!(file["digest_variant"].as_str(), Some("valid"));
        if entry_raw == 0 {
            assert_eq!(startup.session_mode(), SessionMode::Campaign);
        }

        let directory = TestDirectory::new(&format!("campaign-native-{case_name}"));
        directory.write("rulesmd.ini", b"[InfantryTypes]\n0=E1\n");
        directory.write("artmd.ini", b"");
        directory.write(filename, raw_ini.as_bytes());
        let mut assets = AssetManager::from_loose_root_for_test(directory.path());
        let entry_mode = SessionMode::from_game_mode(entry_raw);
        let current_mode = SessionMode::from_game_mode(current_raw);
        let selected =
            select_campaign_named_ini_for_full_init(&mut assets, Some(&campaign), current_mode)
                .unwrap();
        // Each row is one pass from 0x00686C8B. An entry-0 row's skip is
        // followed by the 0x00687599 rerun, which reads the file.
        let process_reached = case["process_reached"].as_bool().unwrap();
        if entry_mode == SessionMode::Campaign && current_mode == SessionMode::Campaign {
            assert!(!process_reached, "{case_name}: the first pass skips");
            assert!(selected.is_some(), "{case_name}: the rerun reads it");
            continue;
        }
        assert_eq!(selected.is_some(), process_reached, "{case_name}");
        assert_eq!(
            case["requested_filename"].as_str(),
            selected.as_ref().map(|_| filename),
            "{case_name}"
        );
        assert_eq!(
            case["bytes_read"].as_u64().unwrap() as usize,
            selected.as_ref().map_or(0, |_| raw_ini.as_bytes().len()),
            "{case_name}"
        );
        assert_eq!(
            case["readccfile_status"].as_u64(),
            selected.as_ref().map(|_| 1u64),
            "campaign basename calls ReadCCFile with digest checks disabled"
        );
        assert_eq!(
            case["digest_key_present_after"].as_bool(),
            selected.as_ref().map(|ini| ini.section("Digest").is_some()),
            "campaign basename leaves Digest in the unchecked INIClass"
        );
        // The prepared entry5/current0 row tests only the gated branch.
        // The native caller/writer that could create this transition is
        // still unproved; it does not extend the campaign startup mode.
        let _loaded = rules_owner()
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(selected.as_ref()),
                &map.ini,
            )
            .unwrap();
    }
}

#[test]
fn tmcj4f_native_cases_gate_the_read_and_ignore_its_digest() {
    use crate::app::match_runtime::session::SessionMode;
    use crate::assets::asset_manager::AssetManager;
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;

    let fixture = native_rules_fixture();
    let cases = fixture["tmcj_cases"].as_array().expect("TMCJ rows");
    for case in cases {
        let input = &case["input"];
        let file = &input["file"];
        let case_name = input["case"].as_str().expect("case name");
        let filename = file["filename"].as_str().expect("TMCJ filename");
        let raw_ini = file["raw_ini"].as_str();
        let present = file["present"].as_bool().expect("file presence");
        assert_eq!(raw_ini.is_some(), present, "{case_name}");

        let directory = TestDirectory::new(&format!("tmcj-native-{case_name}"));
        if let Some(raw_ini) = raw_ini {
            directory.write(filename, raw_ini.as_bytes());
        }
        let mut assets = AssetManager::from_loose_root_for_test(directory.path());
        let mode = SessionMode::from_game_mode(input["mode"].as_i64().unwrap() as i32);
        let team_flag = input["flag"].as_u64().unwrap() == 1;
        let gate_open = should_read_tmcj4f(mode, team_flag);
        let selected = select_tmcj4f_for_full_init(&mut assets, mode, team_flag).unwrap();
        let process_reached = case["process_reached"].as_bool().unwrap();
        assert_eq!(selected.is_some(), process_reached, "{case_name}");
        assert_eq!(
            case["requested_filename"].as_str(),
            gate_open.then_some(filename),
            "the native availability probe follows mode/flag before file presence"
        );
        assert_eq!(
            case["bytes_read"].as_u64().unwrap() as usize,
            selected
                .as_ref()
                .map_or(0, |_| raw_ini.expect("selected TMCJ has bytes").len()),
            "{case_name}"
        );

        let mut owner = rules_owner();
        let map_ini = crate::rules::ini_parser::IniFile::empty();
        let loaded = if let Some(selected) = selected.as_ref() {
            assert_eq!(case["readccfile_status"].as_u64(), Some(1), "{case_name}");
            assert_eq!(
                selected.section("Digest").is_some(),
                case["digest_key_present_after"].as_bool().unwrap(),
                "{case_name}"
            );
            owner
                .load_scenario_with_post_map(
                    NativeScenarioRulesPrefix::NonCampaign(None),
                    &map_ini,
                    selected,
                    |_| (),
                )
                .unwrap()
                .0
        } else {
            assert!(case["readccfile_status"].is_null(), "{case_name}");
            assert!(case["digest_key_present_after"].is_null(), "{case_name}");
            owner
                .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &map_ini)
                .unwrap()
        };
        let (rules, _, _, _) = loaded.into_parts();
        let native_before: [[u8; 3]; 2] =
            serde_json::from_value(case["color_add_before"].clone()).unwrap();
        let native_after: [[u8; 3]; 2] =
            serde_json::from_value(case["color_add_after"].clone()).unwrap();
        assert_eq!(native_before, [[0; 3]; 2], "{case_name}");
        assert_eq!(color_add(&rules), native_after, "{case_name}");
    }
}
