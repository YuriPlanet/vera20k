//! Ares loading boundaries through retained process Rules and the actual
//! external admission, palette decoder, prepared-map and retirement owners.
//! These focused regressions do not certify whole-frame native parity.

use super::*;
use crate::app::process_assets::ProcessAssets;
use crate::map::source::test_support::TestDirectory;

const ROOT: &[u8] = b"[Countries]\n0=PARENT\n1=CHILD\n[Colors]\nFirst=17,128,240\n[PARENT]\nFile.LoadScreen=parent.shp\nFile.LoadScreenPAL=parent.pal\nLoadScreenText.Name=Name:Parent\nLoadScreenText.Color=First\n[CHILD]\nParentCountry=PARENT\nFile.LoadScreen=child.shp\nFile.LoadScreenPAL=child.pal\nLoadScreenText.Name=Name:Child\nLoadScreenText.Color=First\n";

fn fixture() -> (TestDirectory, ProcessAssets, LoadingSession) {
    let dir = TestDirectory::new("ares-country-loading-boundaries");
    dir.write("rulesmd.ini", ROOT);
    dir.write("artmd.ini", b"[Movies]\n");
    dir.write("parent.pal", &[12; 768]);
    dir.write("child.pal", &[20; 768]);
    let manager = AssetManager::from_loose_root_for_test(dir.path());
    let mut assets = ProcessAssets::new(
        crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        false,
    );
    assets.initialize_sources_if_needed(&manager).unwrap();
    assets.return_from_loading(manager);
    let request = crate::app::frontend::cncnet_spawn::SpawnRequest::parse(
        "[Settings]\nScenario=fixture.map\nSide=1\nColor=0\nSpecialHouseIsAlly=No\nAIPlayers=1\nSeed=77\n[HouseHandicaps]\nMulti2=1\n[HouseCountries]\nMulti2=0\n[HouseColors]\nMulti2=1\n[SpawnLocations]\nMulti1=0\nMulti2=1\n",
    ).unwrap();
    let crate::app::frontend::cncnet_spawn::SpawnGame::Skirmish(launch) = request.game else {
        panic!("Battle");
    };
    assert!(
        assets
            .native_rules()
            .unwrap()
            .country_loading(launch.local_slot().country.index())
            .is_none()
    );
    crate::app::frontend::skirmish_session::prepare_external_skirmish_countries(
        &mut assets,
        &launch,
    );
    assert_eq!(request.seed, Some(77));
    let mut session = LoadingSession::from_request(LoadingRequest::unverified_legacy_skirmish(
        *launch,
        tests::unverified_seed(77),
    ));
    session.job.ra2_dir = Some(dir.path().to_owned());
    session.job.asset_manager = assets.lease_for_loading();
    (dir, assets, session)
}

fn prepare_map(session: &mut LoadingSession) {
    let mut map = crate::map::rmg::emit::empty_map_file(&Default::default(), 32, 32);
    map.basic.new_ini_format = Some(4);
    for (index, rx, ry) in [(0, 20, 24), (1, 42, 46)] {
        map.waypoints
            .insert(index, crate::map::waypoints::Waypoint { index, rx, ry });
    }
    let initial = MapLoadInitial::from_test_map_source(
        map,
        crate::map::source::LoadedMapSource::Loose {
            cache_lease: Default::default(),
            path: "fixture.map".into(),
            payload_len: 0,
            source_sha256: crate::util::sha256::sha256_hex(b"synthetic authored map"),
        },
    );
    let stage = std::mem::replace(
        &mut session.stage,
        LoadingStage::Selected(LoadingRequest::generic_map_load("unused")),
    );
    let LoadingStage::Selected(request) = stage else {
        panic!("selected");
    };
    session.stage = LoadingStage::Prepared(request.prepare_initial(initial).unwrap());
}

fn bind(session: &mut LoadingSession, assets: &ProcessAssets) -> anyhow::Result<()> {
    let startup = session.stage.startup();
    bind_early_country_loading(
        session.native.as_mut().unwrap(),
        startup,
        assets.native_rules().unwrap(),
        session.job.asset_manager.as_ref().unwrap(),
    )
}

#[test]
fn ares_country_loading_attempt_freezes_early_palette_color_and_binds_late_text() {
    let (_dir, mut assets, mut session) = fixture();
    bind(&mut session, &assets).unwrap();
    let early = session
        .native
        .as_ref()
        .unwrap()
        .early_country_loading
        .as_ref()
        .unwrap();
    assert_eq!(early.palette_name, "parent.pal");
    assert_eq!(
        early.palette.as_ref().unwrap().color(1),
        Color::rgb(48, 48, 48)
    );
    assert_eq!(
        assets
            .native_rules()
            .unwrap()
            .country_loading_text_hsv(early.country),
        Some([17, 128, 240])
    );
    let early_rgb = early.text_rgb.expect("the Country names a loading color");
    prepare_map(&mut session);
    let LoadingStage::Prepared(prepared) = &mut session.stage else {
        panic!("prepared");
    };
    let mut outgoing = crate::sim::world::Simulation::with_seed(77);
    outgoing.publish_scenario_started();
    let before_rng = outgoing.main_rng().logical_state();
    prepare_country_loading_prefix(
        &mut assets,
        Some(&mut outgoing),
        prepared,
        session.native.as_mut().unwrap(),
    )
    .unwrap();
    assert!(
        !outgoing.session().started(),
        "a retired scene cannot resume after cancellation"
    );
    assert_eq!(before_rng, outgoing.main_rng().logical_state());
    let native = session.native.as_mut().unwrap();
    let late = native.late_country_loading.as_ref().unwrap();
    assert_eq!(late.files(), ("child.shp", "child.pal"));
    assert_eq!(
        native
            .early_country_loading
            .as_ref()
            .unwrap()
            .palette
            .as_ref()
            .unwrap()
            .color(1),
        Color::rgb(48, 48, 48)
    );
    let csf =
        crate::assets::csf_file::test_csf(&[("Name:Parent", "Parent"), ("Name:Child", "Child")]);
    let mut composition = build_loading_composition(
        prepared.initial.map_data(),
        prepared.request.startup().launch_session().unwrap(),
        Some(&csf),
        [800, 600],
        ShellScale::RETAIL,
        &[],
    );
    composition.bind_country_loading(late, Some(&csf));
    assert_eq!(composition.text.country_name.as_deref(), Some("Child"));
    native.resolve_player_colors(&[], &HouseColorRamps::from_schemes(&[]));
    assert_eq!(
        native.text_rgb, early_rgb,
        "later compatibility projection cannot replace the bound color"
    );
    session.retire(&mut assets);
    assert!(assets.is_available() && !assets.is_leased());
}

#[test]
fn ares_country_loading_attempt_freezes_absent_palette_without_aborting_prefix() {
    for malformed in [false, true] {
        let (dir, mut assets, mut session) = fixture();
        if malformed {
            // Invalid bytes use the same safe omission boundary; only missing
            // native resources have established instruction-level equivalence.
            dir.write("parent.pal", b"bad palette");
        } else {
            std::fs::remove_file(dir.path().join("parent.pal")).unwrap();
        }
        bind(&mut session, &assets).unwrap();
        prepare_map(&mut session);
        let LoadingStage::Prepared(prepared) = &mut session.stage else {
            panic!("prepared");
        };
        prepare_country_loading_prefix(
            &mut assets,
            None,
            prepared,
            session.native.as_mut().unwrap(),
        )
        .unwrap();
        let native = session.native.as_ref().unwrap();
        let early = native.early_country_loading.as_ref().unwrap();
        assert_eq!(early.palette_name, "parent.pal");
        assert!(
            early.palette.is_none(),
            "late child.pal cannot replace absent early PAL"
        );
        assert_eq!(
            native.late_country_loading.as_ref().unwrap().files().1,
            "child.pal"
        );
        session.retire(&mut assets);
        assert!(assets.is_available() && !assets.is_leased());
    }
}

#[test]
fn ares_country_loading_attempt_failures_cancel_pending_prefix_and_preserve_actual_registry() {
    for failed_attempt in [false, true] {
        let (_dir, mut assets, mut session) = fixture();
        let country = crate::rules::ruleset::CountryIdx(1);
        bind(&mut session, &assets).unwrap();
        prepare_map(&mut session);
        let LoadingStage::Prepared(prepared) = &mut session.stage else {
            panic!("prepared");
        };
        prepare_country_loading_prefix(
            &mut assets,
            None,
            prepared,
            session.native.as_mut().unwrap(),
        )
        .unwrap();
        let mut slot = Some(session);
        if failed_attempt {
            retire_failed_loading_attempt(
                &mut slot,
                &mut Default::default(),
                &mut assets,
                LoadingFailurePolicy::ReportNativeFailure,
            );
        } else {
            retire_loading_attempt(&mut slot, &mut assets);
        }
        assert!(slot.is_none() && assets.is_available() && !assets.is_leased());
        assert_eq!(
            assets
                .native_rules()
                .unwrap()
                .country_loading(country)
                .unwrap()
                .0
                .files()
                .0,
            "child.shp"
        );
        let owner = assets.native_rules_mut_with_tile_cache().0.unwrap();
        let next = owner.prepare_noncampaign_scenario_prefix();
        owner.cancel_prepared_noncampaign_scenario_prefix(next);
    }
}

#[test]
fn ares_country_loading_attempt_keeps_observer_random_map_and_campaign_on_existing_routes() {
    let (_dir, mut assets, mut session) = fixture();
    let LoadingStage::Selected(request) = &mut session.stage else {
        panic!("selected");
    };
    let LoadingStartup::UnverifiedLegacy {
        session: launch, ..
    } = &mut request.startup
    else {
        panic!("legacy");
    };
    launch.humans[0].role = crate::skirmish_launch::LaunchHumanRole::from_observer(true);
    assert!(ordinary_country_loading_index(request.startup()).is_none());
    let mut launch = tests::test_launch_session(LaunchCountry::from_country_index(1));
    launch.selected_map_file = Some("RandMap.Sed".into());
    let random = LoadingRequest::unverified_legacy_skirmish(launch, tests::unverified_seed(77));
    assert!(ordinary_country_loading_index(random.startup()).is_none());
    let generic = LoadingRequest::generic_map_load("fixture.map");
    assert!(ordinary_country_loading_index(generic.startup()).is_none());
    let campaign = LoadingRequest::campaign(tests::prepared_campaign());
    assert!(ordinary_country_loading_index(campaign.startup()).is_none());
    session.retire(&mut assets);
}
