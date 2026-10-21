use super::*;

const CLIENT_MAP: &[u8] = b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n";

#[test]
fn cncnet_skirmish_loads_the_client_map_instead_of_a_stale_retail_copy() {
    use crate::assets::asset_manager::AssetManager;
    use crate::map::source::{
        LoadedMapSource, load_map_by_name_or_path_with_assets, test_support::TestDirectory,
    };

    let client = TestDirectory::new("cncnet-client");
    let retail = TestDirectory::new("cncnet-retail");
    client.write("spawn.ini", SKIRMISH.as_bytes());
    client.write("spawnmap.ini", CLIENT_MAP);
    retail.write("spawnmap.ini", b"stale retail file, not the selected map");
    let assets = AssetManager::from_loose_root_for_test(retail.path());
    let request = SpawnRequest::read(&client.path().join("spawn.ini")).unwrap();
    let SpawnGame::Skirmish(session) = request.game else {
        panic!("skirmish")
    };
    let selected = session.selected_map_file.unwrap();
    let loaded = load_map_by_name_or_path_with_assets(retail.path(), &selected, &assets).unwrap();
    assert!(
        matches!(loaded.source, LoadedMapSource::Loose { path, .. } if path == client.path().join("spawnmap.ini"))
    );
    assert_eq!(loaded.map.header.theater, "TEMPERATE");

    // An absent client file must fail admission rather than reuse retail data.
    client.write(
        "spawn.ini",
        SKIRMISH.replace("spawnmap.ini", "missing.ini").as_bytes(),
    );
    retail.write("missing.ini", CLIENT_MAP);
    assert!(SpawnRequest::read(&client.path().join("spawn.ini")).is_err());
}

#[test]
fn cncnet_campaign_keeps_logical_identity_for_loose_and_archived_missions() {
    use crate::app::match_bootstrap::{
        CampaignLaunchOptions, OrdinaryMatchSeedClock, prepare_campaign_startup,
    };
    use crate::assets::asset_manager::AssetManager;
    use crate::map::source::{
        LoadedMapSource, load_map_by_name_or_path_with_assets,
        test_support::{TestDirectory, make_new_format_mix_bytes},
    };

    let client = TestDirectory::new("cncnet-missions");
    let retail = TestDirectory::new("cncnet-campaign-retail");
    client.write(
        "spawn.ini",
        b"[Settings]\nIsSinglePlayer=Yes\nScenario=ALL01uMd.map\n",
    );
    client.write("ALL01uMd.map", CLIENT_MAP);
    client.write("ALL02uMd.map", CLIENT_MAP);
    retail.write(
        "mapsmd03.mix",
        &make_new_format_mix_bytes(&[("ALL03MD.MAP", CLIENT_MAP)]),
    );
    let mut assets = AssetManager::from_loose_root_for_test(retail.path());
    assets.load_nested("mapsmd03.mix").unwrap();
    let request = SpawnRequest::read(&client.path().join("spawn.ini")).unwrap();
    let SpawnGame::Campaign(campaign) = request.game else {
        panic!("campaign")
    };
    let mut catalog = crate::rules::campaigns::CampaignRegistry::default();
    catalog.apply_ini(
        &IniFile::from_str("[Battles]\n0=ALL1\n[ALL1]\nCD=2\nScenario=ALL01UMD.MAP\n"),
        &crate::rules::movies::MovieRegistry::default(),
    );
    let startup = prepare_campaign_startup(
        &catalog,
        "ALL1",
        campaign.difficulty,
        Some(&campaign.scenario),
        Some(CampaignLaunchOptions {
            game_speed: request.game_speed,
            build_off_ally: campaign.build_off_ally,
            scenario_directory: campaign.scenario_directory,
        }),
        &mut OrdinaryMatchSeedClock,
    )
    .unwrap();
    assert_eq!(startup.scenario(), "ALL01UMD.MAP");
    let loaded =
        load_map_by_name_or_path_with_assets(retail.path(), startup.selected_map_file(), &assets)
            .unwrap();
    assert!(
        matches!(loaded.source, LoadedMapSource::Loose { path, .. } if path == client.path().join("ALL01uMd.map"))
    );
    let mut metadata = crate::rules::campaign_loading::CampaignLoadingMetadata::default();
    metadata.apply_ini(
        &IniFile::from_str("[ALL01UMD.MAP]\nLSLoadMessage=MISSION:AL1\n"),
        startup.scenario(),
    );
    assert_eq!(metadata.load_message_key(), "MISSION:AL1");

    let next = startup.next_mission(
        "ALL02uMd.map".into(),
        2,
        1,
        3,
        crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
        &mut OrdinaryMatchSeedClock,
    );
    assert_eq!(next.scenario(), "ALL02UMD.MAP");
    let loaded =
        load_map_by_name_or_path_with_assets(retail.path(), next.selected_map_file(), &assets)
            .unwrap();
    assert!(
        matches!(loaded.source, LoadedMapSource::Loose { path, .. } if path == client.path().join("ALL02uMd.map"))
    );

    let archived = next.next_mission(
        "all03md.map".into(),
        3,
        1,
        3,
        crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
        &mut OrdinaryMatchSeedClock,
    );
    let loaded =
        load_map_by_name_or_path_with_assets(retail.path(), archived.selected_map_file(), &assets)
            .unwrap();
    assert_eq!(archived.scenario(), "ALL03MD.MAP");
    assert!(
        matches!(loaded.source, LoadedMapSource::Mix { logical_name, .. } if logical_name.eq_ignore_ascii_case("ALL03MD.MAP"))
    );
}

// Field layout and zero-based house/start values follow the client's
// GameLobbyBase.WriteSpawnIni and AllianceHolder.WriteAlliances. These exercise
// the adapter into production launch types, not a second parser/model.
const SKIRMISH: &str = "\
[Settings]
Name=玩家 zzh
Scenario=spawnmap.ini
PlayerCount=1
Side=0
Color=2
AIPlayers=2
Seed=-2147483648
GameSpeed=1
SpecialHouseIsAlly=No
Credits=5000
Superweapons=No
AlliesAllowed=Yes
[HouseHandicaps]
Multi2=0
Multi3=2
[HouseCountries]
Multi2=8
Multi3=9
[HouseColors]
Multi2=1
Multi3=5
[SpawnLocations]
Multi1=3
Multi2=0
[Multi1_Alliances]
HouseAllyOne=1
[Multi2_Alliances]
HouseAllyOne=0
";

#[test]
fn client_skirmish_preserves_roster_options_and_native_indices() {
    let request = SpawnRequest::parse(SKIRMISH).unwrap();
    assert_eq!(request.seed, Some(0x8000_0000));
    assert_eq!(request.game_speed, 1);
    let SpawnGame::Skirmish(session) = request.game else {
        panic!("expected skirmish")
    };
    assert_eq!(session.humans[session.local_player_index].name, "玩家 zzh");
    assert_eq!(session.selected_map_file.as_deref(), Some("spawnmap.ini"));
    assert_eq!(session.local_slot().country, LaunchCountry::America);
    assert_eq!(session.local_slot().color_index, 2);
    assert_eq!(
        session.local_slot().start_position,
        LaunchStartPosition::Position(3)
    );
    assert_eq!(session.opponents[0].country, LaunchCountry::Russia);
    assert_eq!(session.opponents[0].difficulty, AiDifficulty::Hard);
    assert_eq!(
        session.opponents[0].start_position,
        LaunchStartPosition::Position(0)
    );
    assert_eq!(session.opponents[1].country, LaunchCountry::Yuri);
    assert_eq!(session.opponents[1].difficulty, AiDifficulty::Easy);
    assert_eq!(
        session.opponents[1].start_position,
        LaunchStartPosition::Auto
    );
    assert_eq!(session.local_slot().team, session.opponents[0].team);
    assert_eq!(session.local_slot().team, LaunchTeam::Team(0));
    assert_eq!(session.opponents[1].team, LaunchTeam::None);
    assert_eq!(session.required_start_count(), 3);
    assert_eq!(session.pre_fill_house_roster.created_house_count(), 5);
    assert!(session.house_roster_is_consistent());
    assert_eq!(session.options.starting_credits, 5000);
    assert!(!session.options.super_weapons);
    assert!(session.options.ally_change_allowed);
    // These defaults intentionally differ from VERA's lobby defaults.
    assert_eq!(session.options.unit_count, 0);
    assert_eq!(session.options.default_ai_difficulty, 1);
    assert!(!session.options.short_game);
    assert!(!session.options.crates);
    assert!(!session.options.build_off_ally);
}

#[test]
fn client_campaign_uses_human_difficulty_and_not_catalog_index() {
    let request = SpawnRequest::parse("\u{feff}[Settings]\nIsSinglePlayer=Yes\nScenario=ALL03MD.MAP\nCampaignID=-1\nSide=0\nRa2Mode=False\nGameSpeed=2\nDifficultyModeHuman=2\nDifficultyModeComputer=0\n").unwrap();
    assert_eq!(request.seed, None);
    assert_eq!(request.game_speed, 2);
    let SpawnGame::Campaign(campaign) = request.game else {
        panic!("expected campaign")
    };
    assert_eq!(campaign.scenario, "ALL03MD.MAP");
    assert_eq!(campaign.campaign_id, -1);
    assert_eq!(campaign.side, 0);
    assert_eq!(campaign.difficulty, 2);
    assert!(!campaign.build_off_ally);
    assert!(
        SpawnRequest::parse(
            "[Settings]\nIsSinglePlayer=Yes\nDifficultyModeHuman=1\nDifficultyModeComputer=0"
        )
        .is_err(),
        "PlayerAlwaysOnNormal must not silently change computer difficulty"
    );
}

#[test]
fn rejects_network_sessions_unsupported_extensions_and_invalid_assignments() {
    for append in [
        "[Other1]\nName=Remote",
        "[HandicapDifficulty]\nMulti2=1",
        "[CreditsFactor]\nMulti2=2.0",
        "[IsSpectator]\nMulti2=Yes",
    ] {
        assert!(
            SpawnRequest::parse(&format!("{SKIRMISH}\n{append}")).is_err(),
            "{append}"
        );
    }
    for (from, to) in [
        ("PlayerCount=1", "PlayerCount=2"),
        ("Side=0", "Side=65536"),
        ("Color=2", "Color=1"),
        ("Color=2", "Color=4294967298"),
        ("GameSpeed=1", "GameSpeed=7"),
        ("GameSpeed=1", "GameSpeed=9"),
        ("GameSpeed=1", "GameSpeed=broken"),
        ("AIPlayers=2", "AIPlayers=3"),
        ("Multi1=3", "Multi1=0"),
        ("HouseAllyOne=0", "HouseAllyOne=2"),
        ("SpecialHouseIsAlly=No", "SpecialHouseIsAlly=Yes"),
    ] {
        assert!(
            SpawnRequest::parse(&SKIRMISH.replacen(from, to, 1)).is_err(),
            "{from} -> {to}"
        );
    }
    for key in [
        "ForceMultiplayer",
        "Ra2Mode",
        "LoadSaveGame",
        "ReadMissionSection",
    ] {
        let fixture = SKIRMISH.replacen("[Settings]", &format!("[Settings]\n{key}=Yes"), 1);
        assert!(SpawnRequest::parse(&fixture).is_err(), "{key}");
    }
    assert!(SpawnRequest::parse("[HouseColors]\nMulti1=0").is_err());
}

#[test]
fn cncnet_country_indices_respect_the_compiled_capability_and_identity_width() {
    for index in [10, 11, i32::from(u16::MAX)] {
        let text = SKIRMISH.replacen("Side=0", &format!("Side={index}"), 1);
        let result = SpawnRequest::parse(&text);
        if crate::rules::ARES_ENABLED {
            let SpawnGame::Skirmish(session) = result.unwrap().game else {
                panic!("Battle Country identity");
            };
            assert_eq!(i32::from(session.local_slot().country.raw_index()), index);
            assert!(!session.local_slot().country_random);
        } else {
            assert!(result.is_err(), "stock adapter rejects Country {index}");
        }
    }
    for value in ["-1", "65536", "4294967306", "$10000", "broken"] {
        for text in [
            SKIRMISH.replacen("Side=0", &format!("Side={value}"), 1),
            SKIRMISH.replacen("Multi2=8", &format!("Multi2={value}"), 1),
        ] {
            assert!(SpawnRequest::parse(&text).is_err(), "Country {value}");
        }
    }
    assert!(
        SpawnRequest::parse("[Settings]\nIsSinglePlayer=Yes\nSide=10\n").is_err(),
        "campaign Side keeps its separate Allied/Soviet meaning"
    );
}

#[cfg(feature = "ares")]
#[test]
fn cncnet_ares_country_indices_reach_house_side_and_mcv_selection() {
    use crate::rules::ruleset::RuleSet;
    use crate::sim::house_state::house_state_for_owner;
    use crate::sim::scenario_bootstrap::{
        MatchLaunchDescriptor, launch_mcv_type_for_country, normalized_launch_slots,
        populate_launch_houses,
    };
    use crate::sim::world::Simulation;

    let mut ini = String::from("[Countries]\n");
    for index in 0..12 {
        ini.push_str(&format!("{index}=C{index:02}\n"));
    }
    ini.push_str(
        "12=Neutral\n13=Special\n\
         [Sides]\nGDI=C00,C01,C02\nNod=C03,C04,C05\n\
         ThirdSide=C06,C07,C08\nFourthSide=C09,C10,C11\n\
         Civilian=Neutral\nMutant=Special\n\
         [General]\nBaseUnit=BASEMCV,FOURMCV\n\
         [VehicleTypes]\n0=BASEMCV\n1=FOURMCV\n\
         [BASEMCV]\nOwner=C00,C01,C02,C03,C04,C05,C06,C07,C08\nStrength=100\n\
         [FOURMCV]\nOwner=C09,C10,C11\nStrength=100\n",
    );
    for index in 0..12 {
        let side = ["GDI", "Nod", "ThirdSide", "FourthSide"][index / 3];
        ini.push_str(&format!("[C{index:02}]\nSide={side}\nMultiplay=yes\n"));
    }
    let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
    // Explicit settings cross the same retained Country/Side/House and
    // BaseUnit owners as production. No mod rules, art or parallel bootstrap.
    for (index, side, mcv) in [(10, 3, "FOURMCV"), (0, 0, "BASEMCV")] {
        let text = SKIRMISH
            .replacen("Side=0", &format!("Side={index}"), 1)
            .replacen("Multi2=8", "Multi2=11", 1);
        let request = SpawnRequest::parse(&text).unwrap();
        assert_eq!(request.seed, Some(0x8000_0000));
        let SpawnGame::Skirmish(session) = request.game else {
            panic!("Battle Country identity");
        };
        let launch = MatchLaunchDescriptor::from_resolved(*session).unwrap();
        launch.validate_countries(&rules).unwrap();
        let slots = normalized_launch_slots(launch.session());
        let mut sim = Simulation::new();
        let rng = sim.rng_state();
        populate_launch_houses(&mut sim, &slots, &rules);
        for (slot, country_index, country, expected_side, expected_mcv) in [
            (
                &slots[0],
                launch.session().local_slot().country,
                format!("C{index:02}"),
                side,
                mcv,
            ),
            (
                &slots[1],
                launch.session().opponents[0].country,
                "C11".to_owned(),
                3,
                "FOURMCV",
            ),
        ] {
            let house = house_state_for_owner(sim.houses(), &slot.owner_name, sim.interner())
                .expect("canonical launch House");
            assert_eq!(house.side_index(), expected_side);
            assert_eq!(sim.interner().resolve(house.house_type_id()), country);
            assert_eq!(
                launch_mcv_type_for_country(country_index, &rules),
                Some(expected_mcv)
            );
        }
        assert_eq!(
            sim.rng_state(),
            rng,
            "resolved client identities add no draw"
        );
    }

    for text in [
        SKIRMISH.replacen("Side=0", "Side=65535", 1),
        SKIRMISH.replacen("Multi2=8", "Multi2=65535", 1),
    ] {
        let SpawnGame::Skirmish(session) = SpawnRequest::parse(&text).unwrap().game else {
            panic!("Battle Country identity");
        };
        let launch = MatchLaunchDescriptor::from_resolved(*session).unwrap();
        assert!(
            launch.validate_countries(&rules).is_err(),
            "unallocated identity must fail before House creation"
        );
    }
}
