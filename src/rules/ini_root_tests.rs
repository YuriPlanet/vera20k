//! Test root selection through the retained VFS and actual Rules/AI readers.
use super::*;
use crate::map::source::test_support::TestDirectory;
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

fn write_mo(dir: &TestDirectory) {
    dir.write(
        "rulesmo.ini",
        b"[VehicleTypes]\n0=TEST\n[TEST]\nStrength=250\n[General]\nBuildSpeed=.7\n",
    );
    dir.write("artmo.ini", b"[TEST]\nVoxel=yes\n");
    dir.write("aimo.ini", &ai("MO"));
}

#[test]
fn ini_roots_mo_triplet_reaches_rules_art_and_standalone_ai_without_yr_mixing() {
    let dir = TestDirectory::new("ini-roots-production");
    write_mo(&dir);
    dir.write(
        "rulesmd.ini",
        b"[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\n",
    );
    dir.write("artmd.ini", b"[TEST]\nVoxel=no\n");
    dir.write("aimd.ini", &ai("YR"));
    let config = IniRootConfig {
        profile: IniProfile::Mo,
        ..Default::default()
    };
    let assets = AssetManager::from_loose_root_for_test(dir.path())
        .with_ini_roots(&config)
        .unwrap();
    let sources = RetailRulesSources::select(&assets).unwrap();
    assert_eq!(sources.rulesmd.source.logical_name, "rulesmo.ini");
    assert_eq!(sources.artmd.source.logical_name, "artmo.ini");
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
        false,
    )
    .unwrap();
    assert_eq!(registry.team_types[0].id, "TEAMMO");
    assert_eq!(registry.scripts[0].id, "SCRIPTMO");
    assert_eq!(registry.task_forces[0].id, "FORCEMO");
    let saved = crate::util::save_codec::serialize(&art).unwrap();
    let restored: IniFile = crate::util::save_codec::deserialize(&saved).unwrap();
    assert_eq!(restored.content_hash(), art.content_hash());
}

#[test]
fn ini_roots_process_diagnostics_freeze_selected_names_and_hashes() {
    for (config, rules_name, art_name, ai_name) in [
        (
            IniRootConfig::default(),
            "rulesmd.ini",
            "artmd.ini",
            "aimd.ini",
        ),
        (
            IniRootConfig {
                profile: IniProfile::Mo,
                ..Default::default()
            },
            "rulesmo.ini",
            "artmo.ini",
            "aimo.ini",
        ),
        (
            IniRootConfig {
                profile: IniProfile::Custom,
                rules: Some("r.ini".into()),
                art: Some("a.ini".into()),
                ai: Some("i.ini".into()),
            },
            "r.ini",
            "a.ini",
            "i.ini",
        ),
    ] {
        let dir = TestDirectory::new("ini-roots-diagnostic-names");
        dir.write(rules_name, b"[General]\nBuildSpeed=.7\n");
        dir.write(art_name, b"[TEST]\nVoxel=yes\n");
        dir.write(ai_name, &ai("DIAG"));
        dir.write("langrule.ini", b"[General]\nBuildSpeed=.8\n");
        let assets = AssetManager::from_loose_root_for_test(dir.path())
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
fn ini_roots_auto_freezes_provider_selection_and_default_yr_stays_explicit() {
    let dir = TestDirectory::new("ini-roots-auto");
    write_mo(&dir);
    let auto = IniRootConfig {
        profile: IniProfile::Auto,
        ..Default::default()
    };
    let assets = AssetManager::from_loose_root_for_test(dir.path())
        .with_ini_roots(&auto)
        .unwrap();
    assert_eq!(assets.ini_roots().profile(), IniProfile::Mo);
    // A later mount changes availability, not the frozen selection.
    let mut assets = assets;
    let payload = crate::map::source::test_support::make_new_format_mix_bytes(&[(
        "rulesmd.ini",
        b"[YR]\nValue=yes\n",
    )]);
    dir.write("other.mix", &payload);
    assets.load_all_disk_mixes().unwrap();
    assert!(assets.contains("rulesmd.ini"));
    assert_eq!(assets.ini_roots().profile(), IniProfile::Mo);
    assert!(RetailRulesSources::select(&assets).is_ok());
    let default_assets = AssetManager::from_loose_root_for_test(dir.path());
    assert_eq!(default_assets.ini_roots().profile(), IniProfile::Yr);
    assert!(
        RetailRulesSources::select(&default_assets).is_err(),
        "default YR must not silently select MO"
    );
    assert!(
        AssetManager::from_loose_root_for_test(dir.path())
            .with_ini_roots(&auto)
            .is_ok(),
        "unmounted MIX is not part of this provider"
    );
    assert!(
        assets.with_ini_roots(&auto).is_err(),
        "a fresh Auto decision must reject partial alternate roots"
    );
}

#[test]
fn ini_roots_empty_loose_winner_does_not_fall_through_to_archive_or_yr() {
    let dir = TestDirectory::new("ini-roots-empty");
    write_mo(&dir);
    dir.write("aimo.ini", b"");
    dir.write("aimd.ini", &ai("YR"));
    let payload =
        crate::map::source::test_support::make_new_format_mix_bytes(&[("aimo.ini", &ai("MO"))]);
    dir.write("other.mix", &payload);
    let mut assets = AssetManager::from_loose_root_for_test(dir.path());
    assets.load_all_disk_mixes().unwrap();
    let error = assets
        .with_ini_roots(&IniRootConfig {
            profile: IniProfile::Mo,
            ..Default::default()
        })
        .err()
        .unwrap();
    assert!(error.to_string().contains("aimo.ini"));
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
        false,
    )
    .unwrap();
    assert_eq!(registry.team_types[0].id, "TEAMCUSTOM");
}

#[test]
#[ignore = "requires VERA20K_INI_ROOT_TEST_DIR; read-only local MOD source probe, not a game load"]
fn ini_roots_real_install_reads_selected_mo_triplet_with_explicit_tooling_mix_scan() {
    let path = std::path::PathBuf::from(
        std::env::var_os("VERA20K_INI_ROOT_TEST_DIR").expect("set VERA20K_INI_ROOT_TEST_DIR"),
    );
    let mut assets = AssetManager::from_loose_root_for_test(&path);
    assets.load_all_disk_mixes().unwrap();
    let assets = assets
        .with_ini_roots(&IniRootConfig {
            profile: IniProfile::Mo,
            ..Default::default()
        })
        .unwrap();
    for name in assets.ini_roots().names() {
        let selected = select_ini(&assets, name).unwrap();
        assert_eq!(selected.source.logical_name, name);
        println!("{}", serde_json::to_string(&selected.source).unwrap());
    }
}
