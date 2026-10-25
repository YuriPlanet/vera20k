//! Two executed Ares loading-score histories through the production sources,
//! retained early Country/Side owner, and the ordinary Theme request owner.
use super::*;
use crate::app::process_assets::ProcessAssets;
use crate::map::source::test_support::TestDirectory;
use crate::rules::ini_parser::IniFile;
use serde_json::Value;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/rules_oracle/ares_loading_themes.json")
}

fn fixture(extra: &str) -> (TestDirectory, ProcessAssets) {
    let directory = TestDirectory::new("ares-loading-themes");
    // Fresh INI loading keeps the first exact section. Put the case's keys
    // into that same Country body, rather than appending a shadowed duplicate.
    let extra = IniFile::from_str(extra);
    let mut root = String::from("[Countries]\n0=COUNTRY\n[Sides]\nSIDE=COUNTRY\n");
    for name in ["COUNTRY", "SIDE"] {
        root += &format!("[{name}]\n");
        if name == "COUNTRY" {
            root += "Side=SIDE\n";
        }
        let section = extra.section_or_empty(name);
        for key in section.keys() {
            root += &format!("{key}={}\n", section.read_name(key, 128).unwrap());
        }
    }
    directory.write("rulesmd.ini", root.as_bytes());
    directory.write("artmd.ini", b"[Movies]\n");
    let keys = corpus()["histories"][0]["theme_keys"].as_array().unwrap();
    let mut themes = String::from("[Themes]\n");
    for (index, key) in keys.iter().enumerate() {
        themes += &format!("{index}={}\n", key.as_str().unwrap());
    }
    for key in keys {
        let key = key.as_str().unwrap();
        themes += &format!("[{key}]\nSound={key}\nRepeat=yes\n");
    }
    directory.write("thememd.ini", themes.as_bytes());
    let manager = AssetManager::from_loose_root_for_test(directory.path());
    let mut process = ProcessAssets::new(
        crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        false,
    );
    process.initialize_sources_if_needed(&manager).unwrap();
    process.read_multiplayer_country(&IniFile::empty(), 0);
    process.refresh_multiplayer_country_bodies();
    process.return_from_loading(manager);
    (directory, process)
}

#[test]
fn ares_loading_theme_selection_matches_two_native_histories() {
    for history in corpus()["histories"].as_array().unwrap() {
        let campaign = history["name"] == "campaignBasic";
        for stage in history["stages"].as_array().unwrap() {
            let mut text = String::new();
            for row in stage["input"]["rows"].as_array().unwrap() {
                text += &format!(
                    "[{}]\n{}={}\n",
                    row["section"].as_str().unwrap(),
                    row["key"].as_str().unwrap(),
                    row["value"].as_str().unwrap()
                );
            }
            let (_directory, mut process) = fixture(if campaign {
                "[SIDE]\nLoadingTheme=COUNTRY_SCORE\n"
            } else {
                &text
            });
            let request = if campaign {
                LoadingRequest::campaign(tests::prepared_campaign())
            } else {
                LoadingRequest::unverified_legacy_skirmish(
                    tests::test_launch_session(LaunchCountry::America),
                    tests::unverified_seed(31),
                )
            };
            let mut session = LoadingSession::from_request(request);
            session.job.asset_manager = process.lease_for_loading();
            let assets = session.job.asset_manager.as_ref().unwrap();
            let mut audio = tests::test_audio();
            // The native read-call source receipts prove these opposite-source
            // values are not read by the selected branch.
            let map_text = if campaign {
                text
            } else {
                String::from(
                    "[SIDE]\nLoadingTheme=MISSION_SCORE\n[COUNTRY]\nLoadingTheme=MISSION_SCORE\n[Basic]\nLoadingTheme=MISSION_SCORE\n",
                )
            };
            play_ares_loading_theme(
                &process,
                &mut audio,
                session.stage.startup(),
                &IniFile::from_str(&map_text),
                assets,
                500,
            );
            let expected = stage["play_requests"][0].as_i64().unwrap() as i32;
            assert_eq!(
                audio.theme.slots().retained,
                expected,
                "{} / {}",
                history["name"],
                stage["label"]
            );
            session.retire(&mut process);
        }
    }
}

#[test]
fn ares_campaign_theme_plays_once_after_map_parse_through_the_asset_lease() {
    let (directory, mut process) = fixture("");
    directory.write("ALL01UMD.MAP", b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[Basic]\nNewINIFormat=4\nLoadingTheme=MISSION_SCORE\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n");
    let mut audio = tests::test_audio();
    let mut session =
        LoadingSession::from_request(LoadingRequest::campaign(tests::prepared_campaign()));
    session.job.ra2_dir = Some(directory.path().to_owned());
    let mut slot = None;
    let mut startup = crate::app::match_runtime::startup::MatchStartup::default();
    replace_loading_attempt(
        &mut slot,
        &mut startup,
        &mut process,
        &mut audio,
        session,
        100,
    );
    assert_eq!(
        audio.theme.slots().retained,
        -1,
        "Ares defers until the selected map is parsed"
    );
    let session = prepare_loading_session(
        &mut process,
        &mut audio,
        &mut Default::default(),
        None,
        slot.take().unwrap(),
        true,
        Some(directory.path().to_owned()),
        None,
    )
    .unwrap();
    assert!(matches!(session.stage, LoadingStage::Prepared(_)));
    assert!(process.manager().is_none() && process.is_leased());
    assert_eq!(
        audio.theme.slots().retained,
        corpus()["histories"][0]["stages"][1]["play_requests"][0]
            .as_i64()
            .unwrap() as i32
    );
    let assets = audio_service_asset_manager(&process, Some(&session)).unwrap();
    audio.play_theme("COUNTRY_SCORE", assets, 600);
    let replacement = audio.theme.slots().retained;
    let session = prepare_loading_session(
        &mut process,
        &mut audio,
        &mut Default::default(),
        None,
        session,
        false,
        Some(directory.path().to_owned()),
        None,
    )
    .unwrap();
    assert_eq!(
        audio.theme.slots().retained,
        replacement,
        "Prepared visits do not restart loading music"
    );
    session.retire(&mut process);
    assert!(process.is_available() && !process.is_leased());
}
