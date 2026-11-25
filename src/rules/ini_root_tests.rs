//! Test root selection through the retained VFS and actual Rules/AI readers.
use super::*;
use crate::map::source::test_support::TestDirectory;
use crate::rules::rule_policy::RulePolicy;
use crate::util::ini_roots::{IniProfile, IniRootConfig};

fn ai(tag: &str) -> Vec<u8> {
    format!(
        "[TeamTypes]\n0=TEAM{tag}\n[TEAM{tag}]\nScript=SCRIPT{tag}\nTaskForce=FORCE{tag}\n\
         [ScriptTypes]\n0=SCRIPT{tag}\n[SCRIPT{tag}]\n0=2,0\n\
         [TaskForces]\n0=FORCE{tag}\n[FORCE{tag}]\n0=1,E1\nGroup=-1\n\
         [AITriggerTypes]\nTRIGGER{tag}=Trigger,TEAM{tag},<all>,2,4,<none>,{},40.0,10.0,40.0,1,0,1,0,<none>,1,1,1\n",
        "00".repeat(32)
    ).into_bytes()
}

fn custom_config() -> IniRootConfig {
    IniRootConfig {
        profile: IniProfile::Custom,
        rules: Some("rules-alt.ini".into()),
        art: Some("art-alt.ini".into()),
        ai: Some("ai-alt.ini".into()),
        ..Default::default()
    }
}

/// VERA: a network match of the original game that seats no computer reads
/// the AI INI a retail install ships, whatever the game folder holds. One
/// that seats a computer, a match off the network and a selected mod's read
/// the folder's.
#[test]
fn a_network_match_without_computers_reads_the_original_ai_ini() {
    use crate::assets::asset_manager::MediaArchiveMode;
    use crate::assets::asset_manager::test_support::audio_test_install;
    use crate::map::source::test_support::make_new_format_mix_bytes;
    let dir = audio_test_install("match-ai-file");
    let shipped = ai("ORIGINAL");
    let local = make_new_format_mix_bytes(&[("aimd.ini", shipped.as_slice())]);
    std::fs::remove_file(dir.path().join("localmd.mix")).unwrap();
    std::fs::write(
        dir.path().join("ra2md.mix"),
        make_new_format_mix_bytes(&[("localmd.mix", &local)]),
    )
    .unwrap();
    std::fs::write(dir.path().join("aimd.ini"), ai("FOLDER")).unwrap();
    let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let team = |file| {
        crate::rules::team_ai_ini::TeamAiIniRegistry::load_for_match(
            &assets,
            &IniFile::empty(),
            RulePolicy::stock_multiplayer(),
            file,
        )
        .unwrap()
        .team_types[0]
            .id
            .clone()
    };
    assert_eq!(match_ai_file(&assets, true, false), AiFile::Original);
    assert_eq!(team(AiFile::Original), "TEAMORIGINAL");
    assert_eq!(match_ai_file(&assets, true, true), AiFile::GameFolder);
    assert_eq!(match_ai_file(&assets, false, false), AiFile::GameFolder);
    assert_eq!(team(AiFile::GameFolder), "TEAMFOLDER");

    let mod_dir = dir.path().join("mods-under-test").join("Ai");
    std::fs::create_dir_all(&mod_dir).unwrap();
    let modded =
        AssetManager::with_mod(dir.path(), Some(&mod_dir), MediaArchiveMode::STOCK_DIGITAL)
            .unwrap();
    assert_eq!(match_ai_file(&modded, true, false), AiFile::GameFolder);
}

fn write_custom(dir: &TestDirectory) {
    dir.write(
        "rules-alt.ini",
        b"[VehicleTypes]\n0=TEST\n[TEST]\nStrength=250\n[General]\nBuildSpeed=.7\n",
    );
    dir.write("art-alt.ini", b"[TEST]\nVoxel=yes\n");
    dir.write("ai-alt.ini", &ai("ALT"));
}

#[test]
fn ini_roots_custom_triplet_reaches_rules_art_and_standalone_ai_without_yr_mixing() {
    let dir = TestDirectory::new("ini-roots-production");
    write_custom(&dir);
    dir.write(
        "rulesmd.ini",
        b"[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\n",
    );
    dir.write("artmd.ini", b"[TEST]\nVoxel=no\n");
    dir.write("aimd.ini", &ai("YR"));
    let config = custom_config();
    let assets = AssetManager::from_loose_root_for_test(dir.path())
        .with_ini_roots(&config)
        .unwrap();
    let sources = RetailRulesSources::select(&assets).unwrap();
    assert_eq!(sources.rulesmd.source.logical_name, "rules-alt.ini");
    assert_eq!(sources.artmd.source.logical_name, "art-alt.ini");
    let (_, _, mut owner) = sources
        .into_startup(Default::default(), Default::default())
        .unwrap()
        .into_parts();
    let (rules, _, art, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts();
    assert_eq!(rules.object("TEST").unwrap().strength, 250);
    assert!(art.section_or_empty("TEST").read_bool("Voxel", false));
    let registry = crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(
        &assets,
        &IniFile::empty(),
        RulePolicy::stock_campaign(),
    )
    .unwrap();
    assert_eq!(registry.team_types[0].id, "TEAMALT");
    assert_eq!(registry.scripts[0].id, "SCRIPTALT");
    assert_eq!(registry.task_forces[0].id, "FORCEALT");
    let saved = crate::util::save_codec::serialize(&art).unwrap();
    let restored: IniFile = crate::util::save_codec::deserialize(&saved).unwrap();
    assert_eq!(restored.content_hash(), art.content_hash());
}

#[test]
fn ini_roots_process_diagnostics_freeze_selected_names_and_hashes() {
    use crate::assets::asset_manager::{MediaArchiveMode, test_support::audio_test_install};

    let install = audio_test_install("ini-roots-diagnostic-install");
    for (config, rules_name, art_name, ai_name) in [
        (
            IniRootConfig::default(),
            "rulesmd.ini",
            "artmd.ini",
            "aimd.ini",
        ),
        (
            custom_config(),
            "rules-alt.ini",
            "art-alt.ini",
            "ai-alt.ini",
        ),
    ] {
        let dir = TestDirectory::new("ini-roots-diagnostic-names");
        dir.write(rules_name, b"[General]\nBuildSpeed=.7\n");
        dir.write(art_name, b"[TEST]\nVoxel=yes\n");
        dir.write(ai_name, &ai("DIAG"));
        dir.write(
            "langrule.ini",
            crate::rules::retail_ini_fixture::DIGESTED_LANGRULE_CONTROL.as_bytes(),
        );
        let assets = AssetManager::with_mod(
            install.path(),
            Some(dir.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap()
        .with_ini_roots(&config)
        .unwrap();
        let sources = RetailRulesSources::select(&assets).unwrap();
        let expected = [
            (rules_name, Some(sources.rulesmd.ini.content_hash())),
            (
                "LANGRULE.INI",
                Some(sources.langrule.as_ref().unwrap().ini.content_hash()),
            ),
            (art_name, Some(sources.artmd.ini.content_hash())),
        ];
        let mut owner = sources
            .into_owner(Default::default(), Default::default())
            .unwrap();
        assert_eq!(owner.selected_source_ini_hashes(), expected);
        // Diagnostics describe consumed snapshots, not reopened loose files.
        dir.write(rules_name, b"[General]\nBuildSpeed=9\n");
        dir.write(art_name, b"[TEST]\nVoxel=no\n");
        owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap();
        assert_eq!(owner.selected_source_ini_hashes(), expected);
    }
}

#[test]
fn ini_roots_custom_selection_stays_frozen_and_auto_requires_stock_sources() {
    let dir = TestDirectory::new("ini-roots-auto");
    write_custom(&dir);
    let auto = IniRootConfig {
        profile: IniProfile::Auto,
        ..Default::default()
    };
    let assets = AssetManager::from_loose_root_for_test(dir.path())
        .with_ini_roots(&custom_config())
        .unwrap();
    assert_eq!(assets.ini_roots().profile(), IniProfile::Custom);
    // A later mount changes availability, not the frozen selection.
    let mut assets = assets;
    let stock_ai = ai("YR");
    let payload = crate::map::source::test_support::make_new_format_mix_bytes(&[
        ("rulesmd.ini", b"[YR]\nValue=yes\n"),
        ("artmd.ini", b"[TEST]\nVoxel=no\n"),
        ("aimd.ini", &stock_ai),
    ]);
    dir.write("other.mix", &payload);
    assets.load_all_disk_mixes().unwrap();
    assert!(assets.contains("rulesmd.ini"));
    assert_eq!(assets.ini_roots().profile(), IniProfile::Custom);
    assert_eq!(
        RetailRulesSources::select(&assets)
            .unwrap()
            .rulesmd
            .source
            .logical_name,
        "rules-alt.ini"
    );
    let default_assets = AssetManager::from_loose_root_for_test(dir.path());
    assert_eq!(default_assets.ini_roots().profile(), IniProfile::Yr);
    assert!(
        RetailRulesSources::select(&default_assets).is_err(),
        "default YR must not silently select custom roots"
    );
    assert!(
        AssetManager::from_loose_root_for_test(dir.path())
            .with_ini_roots(&auto)
            .is_err(),
        "unmounted MIX is not part of this provider"
    );
    let reselected = assets.with_ini_roots(&auto).unwrap();
    assert_eq!(reselected.ini_roots().profile(), IniProfile::Yr);
    assert_eq!(
        RetailRulesSources::select(&reselected)
            .unwrap()
            .rulesmd
            .source
            .logical_name,
        "rulesmd.ini"
    );
}

#[test]
fn ini_roots_empty_loose_winner_does_not_fall_through_to_archive_or_yr() {
    let dir = TestDirectory::new("ini-roots-empty");
    write_custom(&dir);
    dir.write("ai-alt.ini", b"");
    dir.write("aimd.ini", &ai("YR"));
    let payload =
        crate::map::source::test_support::make_new_format_mix_bytes(&[("ai-alt.ini", &ai("ALT"))]);
    dir.write("other.mix", &payload);
    let mut assets = AssetManager::from_loose_root_for_test(dir.path());
    assets.load_all_disk_mixes().unwrap();
    let error = assets.with_ini_roots(&custom_config()).err().unwrap();
    assert!(error.to_string().contains("ai-alt.ini"));
}

#[test]
fn ini_roots_custom_sources_use_the_same_physical_selector() {
    let dir = TestDirectory::new("ini-roots-custom");
    dir.write("r.ini", b"[General]\nBuildSpeed=.7\n");
    dir.write("a.ini", b"[TEST]\nVoxel=yes\n");
    dir.write("i.ini", &ai("CUSTOM"));
    let config = IniRootConfig {
        profile: IniProfile::Custom,
        rules: Some("r.ini".into()),
        art: Some("a.ini".into()),
        ai: Some("i.ini".into()),
        ..Default::default()
    };
    let assets = AssetManager::from_loose_root_for_test(dir.path())
        .with_ini_roots(&config)
        .unwrap();
    let sources = RetailRulesSources::select(&assets).unwrap();
    assert_eq!(sources.rulesmd.source.logical_name, "r.ini");
    assert_eq!(sources.artmd.source.logical_name, "a.ini");
    let registry = crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(
        &assets,
        &IniFile::empty(),
        RulePolicy::stock_campaign(),
    )
    .unwrap();
    assert_eq!(registry.team_types[0].id, "TEAMCUSTOM");
}

#[test]
#[ignore = "requires VERA20K_INI_ROOT_TEST_DIR and VERA20K_INI_ROOT_TEST_RULES/ART/AI; read-only local MOD source probe, not a game load"]
fn ini_roots_real_install_reads_selected_custom_triplet_with_explicit_tooling_mix_scan() {
    let path = std::path::PathBuf::from(
        std::env::var_os("VERA20K_INI_ROOT_TEST_DIR").expect("set VERA20K_INI_ROOT_TEST_DIR"),
    );
    let mut assets = AssetManager::from_loose_root_for_test(&path);
    assets.load_all_disk_mixes().unwrap();
    let assets = assets
        .with_ini_roots(&IniRootConfig {
            profile: IniProfile::Custom,
            rules: Some(
                std::env::var("VERA20K_INI_ROOT_TEST_RULES")
                    .expect("set VERA20K_INI_ROOT_TEST_RULES"),
            ),
            art: Some(
                std::env::var("VERA20K_INI_ROOT_TEST_ART").expect("set VERA20K_INI_ROOT_TEST_ART"),
            ),
            ai: Some(
                std::env::var("VERA20K_INI_ROOT_TEST_AI").expect("set VERA20K_INI_ROOT_TEST_AI"),
            ),
            ..Default::default()
        })
        .unwrap();
    for name in assets.ini_roots().names() {
        let selected = select_ini(&assets, name).unwrap();
        assert_eq!(selected.source.logical_name, name);
        println!("{}", serde_json::to_string(&selected.source).unwrap());
    }
}
