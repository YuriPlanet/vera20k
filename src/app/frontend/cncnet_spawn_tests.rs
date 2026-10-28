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

/// `[MPColors] Color=` carries the client's colour index: YR's eight
/// priorities, or a `[Colors]` entry when a Phobos build admits
/// SkirmishUnlimitedColors.
#[test]
fn spawn_colour_index_width_follows_the_phobos_capability() {
    assert!(SpawnRequest::parse(SKIRMISH).is_ok());
    let beyond_eight = SKIRMISH.replacen("Color=2", "Color=9", 1);
    assert_eq!(
        SpawnRequest::parse(&beyond_eight).is_ok(),
        crate::rules::PHOBOS_ENABLED
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
    assert_eq!(
        SpawnRequest::parse("[Settings]\nIsSinglePlayer=Yes\nSide=10\n").is_ok(),
        crate::rules::ARES_ENABLED,
        "campaign Side has its own byte-wide adapter identity in Ares builds"
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

/// One participant's multiplayer file (docs/spawn.md): every human `P{s}`
/// has SourceIndex `s`; this file is written by source `local`, so its
/// `[OtherK]` lists the remaining sources in ascending order.
fn network_spawn(count: usize, local: usize, extra: &str) -> String {
    let color = |source: usize| if count <= 8 { source } else { source % 8 };
    let mut text = format!(
        "[Settings]\nName=P{local}\nSide={}\nColor={}\nPlayerCount={count}\nSourceIndex={local}\n\
         Seed=77\nSpecialHouseIsAlly=No\nPort=1300\n",
        local % 10,
        color(local)
    );
    for (other, source) in (0..count).filter(|&source| source != local).enumerate() {
        text.push_str(&format!(
            "[Other{}]\nName=P{source}\nSide={}\nColor={}\nSourceIndex={source}\nIp=10.0.0.{}\n",
            other + 1,
            source % 10,
            color(source),
            source + 1
        ));
        if source != 1 {
            text.push_str(&format!("Port={}\n", 1234 + source));
        }
    }
    text + extra
}

/// `MultiN` key of `source` in the file written by `local`.
fn house_key(local: usize, source: usize) -> String {
    let house = match source.cmp(&local) {
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Less => source + 1,
        std::cmp::Ordering::Greater => source,
    };
    format!("Multi{}", house + 1)
}

fn parse_network(text: &str) -> NetworkSpawn {
    match SpawnRequest::parse(text).unwrap().game {
        SpawnGame::Network(network) => *network,
        other => panic!("expected a network spawn, got {other:?}"),
    }
}

fn relay_section(source: u8, sources_mask: u32) -> String {
    relay_section_at(source, sources_mask, 45)
}

/// A `[Relay]` section whose ordering certificate runs `tick_hz`.
fn relay_section_at(source: u8, sources_mask: u32, tick_hz: u16) -> String {
    use crate::net::udp::{OrderingCertificate, RelayTicket, test_support::SecretKey};
    use base64::Engine;
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let key = SecretKey::new([9; 32]);
    let ticket = RelayTicket {
        room: [0x11; 16],
        player: [0x22; 16],
        source,
        generation: 1,
        expires: 300,
        nonce: [3; 16],
    }
    .sign(&key)
    .unwrap();
    let certificate = OrderingCertificate {
        room: [0x11; 16],
        sources_mask,
        tick_hz,
        expires: 86400,
    }
    .sign(&key)
    .unwrap();
    format!(
        "[Relay]\nAddress=203.0.113.5:5000\nMatchId=11111111-1111-1111-1111-111111111111\n\
         PlayerId=22222222-2222-2222-2222-222222222222\nTicket={}\nPacketKey={}\nPayloadKey={}\n\
         OrderingCertificate={}\nServiceUrl=https://match.example.com/\nToken=bearer-secret\n",
        encode(&ticket),
        encode(&[5; 32]),
        encode(&[7; 32]),
        encode(&certificate)
    )
}

#[test]
fn two_player_relay_file_admits_the_grant_in_canonical_source_order() {
    let text = network_spawn(2, 1, &relay_section(1, 0b11));
    let request = SpawnRequest::parse(&text).unwrap();
    assert_eq!(request.seed, Some(77));
    let SpawnGame::Network(network) = request.game else {
        panic!("network spawn")
    };
    let session = &network.session;
    assert_eq!(session.local_player_index, 1);
    assert_eq!(session.humans[0].name, "P0");
    assert_eq!(session.humans[1].name, "P1");
    assert_eq!(session.humans[0].raw_slot_index, Some(0));
    assert_eq!(session.humans[1].slot.color_index, 1);
    assert!(session.house_roster_is_consistent());
    let SpawnTransport::Relay(relay) = &network.transport else {
        panic!("relay transport")
    };
    assert_eq!(relay.address.to_string(), "203.0.113.5:5000");
    assert_eq!(relay.grant.payload_key, [7; 32]);
    assert_eq!(relay.grant.key, [5; 32]);
    assert_eq!(relay.service_url, "https://match.example.com");
    assert_eq!(relay.token, "bearer-secret");
    let debug = format!("{network:?}");
    assert!(!debug.contains("bearer-secret") && !debug.contains("Ticket"));

    // The ticket names source 1; the roster must be exactly both humans.
    let foreign = network_spawn(2, 0, &relay_section(1, 0b11));
    assert!(SpawnRequest::parse(&foreign).is_err());
    assert!(SpawnRequest::parse(&network_spawn(2, 1, &relay_section(1, 0b111))).is_err());
    for (from, to) in [
        ("https://match.example.com/", "http://match.example.com"),
        ("https://match.example.com/", "https://match.example.com/v1"),
        (
            "https://match.example.com/",
            "https://user@match.example.com",
        ),
        ("PayloadKey=", "PayloadKey=*"),
        ("PayloadKey=", "PayloadKey=AAAA"),
        ("Address=203.0.113.5:5000", "Address=match.example.com:5000"),
        ("Token=bearer-secret", "Token=bearer secret"),
        ("MatchId=11111111", "MatchId=33333333"),
    ] {
        let changed = if to == "PayloadKey=AAAA" {
            let line = text.lines().find(|line| line.starts_with(from)).unwrap();
            text.replacen(line, to, 1)
        } else {
            text.replacen(from, to, 1)
        };
        assert!(SpawnRequest::parse(&changed).is_err(), "{from} -> {to}");
    }
    let loopback = text.replacen("https://match.example.com/", "http://127.0.0.1:8080", 1);
    let SpawnTransport::Relay(relay) = parse_network(&loopback).transport else {
        panic!("relay transport")
    };
    assert_eq!(relay.service_url, "http://127.0.0.1:8080");
}

#[test]
fn four_player_direct_files_agree_on_one_session_with_house_teams() {
    // Sources 0+2 and 1+3 are teams; each file keys them by its own houses.
    let file = |local: usize| {
        let mut extra = String::from("[HouseTeams]\n");
        for source in 0..4 {
            extra.push_str(&format!(
                "{}={}\n",
                house_key(local, source),
                5 + source % 2
            ));
        }
        extra.push_str(&format!("[SpawnLocations]\n{}=9\n", house_key(local, 3)));
        parse_network(&network_spawn(4, local, &extra))
    };
    let host = file(0);
    let guest = file(2);
    assert_eq!(host.session.local_player_index, 0);
    assert_eq!(guest.session.local_player_index, 2);
    let mut shared = guest.session.clone();
    shared.local_player_index = 0;
    assert_eq!(shared, host.session, "only the local index differs");
    let teams: Vec<_> = host.session.humans.iter().map(|h| h.slot.team).collect();
    assert_eq!(
        teams,
        [5, 6, 5, 6].map(LaunchTeam::Team),
        "teams follow SourceIndex"
    );
    assert_eq!(
        host.session.humans[3].slot.start_position,
        LaunchStartPosition::Position(9)
    );
    let SpawnTransport::Direct(direct) = guest.transport else {
        panic!("direct transport")
    };
    assert_eq!(direct.local_port, 1300);
    assert_eq!(
        direct.peers,
        [
            Some("10.0.0.1:1234".parse().unwrap()),
            // [Other] Port defaults to the LAN port.
            Some("10.0.0.2:1234".parse().unwrap()),
            None,
            Some("10.0.0.4:1237".parse().unwrap()),
        ]
    );
    for broken in [
        network_spawn(4, 0, "")
            .replacen("Ip=10.0.0.2", "Ip=10.0.0.4", 1)
            .replacen("Port=1237", "Port=1234", 1),
        network_spawn(4, 0, "").replacen("Ip=10.0.0.2", "Ip=0.0.0.0", 1),
        network_spawn(4, 0, "").replacen("Ip=10.0.0.2\n", "", 1),
        network_spawn(4, 0, "[HouseTeams]\nMulti5=0\n"),
        network_spawn(4, 0, "[HouseTeams]\nMulti1=30\n"),
    ] {
        assert!(SpawnRequest::parse(&broken).is_err(), "{broken}");
    }
}

#[test]
fn thirty_humans_fill_every_network_row_and_reuse_colours() {
    let session = parse_network(&network_spawn(30, 17, "")).session;
    assert_eq!(session.humans.len(), 30);
    assert_eq!(session.local_player_index, 17);
    assert_eq!(session.local_player().name, "P17");
    assert_eq!(session.humans[29].raw_slot_index, Some(29));
    assert_eq!(session.humans[9].slot.color_index, 1);
    assert_eq!(session.required_start_count(), 30);
    assert!(session.house_roster_is_consistent());
    // No AI house remains and a 31st human has no row.
    assert!(
        SpawnRequest::parse(&network_spawn(30, 0, "").replacen(
            "[Settings]\n",
            "[Settings]\nAIPlayers=1\n",
            1
        ))
        .is_err()
    );
    assert!(SpawnRequest::parse(&network_spawn(31, 0, "")).is_err());
    // Up to eight houses colours stay distinct, as in single player.
    let clash = network_spawn(3, 0, "").replacen("Color=1", "Color=0", 1);
    assert!(SpawnRequest::parse(&clash).is_err());
}

#[test]
fn network_ai_houses_follow_every_human() {
    let session = parse_network(
        &network_spawn(
            3,
            1,
            "[HouseHandicaps]\nMulti4=1\nMulti6=0\n[HouseCountries]\nMulti4=3\nMulti6=4\n\
         [HouseColors]\nMulti4=6\nMulti6=7\n[SpawnLocations]\nMulti6=20\n",
        )
        .replacen("[Settings]\n", "[Settings]\nAIPlayers=2\n", 1),
    )
    .session;
    let rows: Vec<_> = session
        .opponents
        .iter()
        .map(|ai| ai.raw_slot_index)
        .collect();
    assert_eq!(rows, [3, 5]);
    assert_eq!(session.opponents[0].difficulty, AiDifficulty::Normal);
    assert_eq!(
        session.opponents[1].start_position,
        LaunchStartPosition::Position(20)
    );
    assert_eq!(session.required_start_count(), 5);
    assert!(session.house_roster_is_consistent());
    // A human's house is never an AI slot.
    let human_ai = network_spawn(3, 1, "[HouseHandicaps]\nMulti3=1\n").replacen(
        "[Settings]\n",
        "[Settings]\nAIPlayers=1\n",
        1,
    );
    assert!(SpawnRequest::parse(&human_ai).is_err());
}

#[test]
fn network_humans_need_a_complete_source_permutation_seed_and_one_team_form() {
    let valid = network_spawn(3, 0, "");
    assert!(SpawnRequest::parse(&valid).is_ok());
    for (from, to) in [
        ("SourceIndex=2", "SourceIndex=1"),
        ("SourceIndex=2", "SourceIndex=3"),
        ("SourceIndex=2", "SourceIndex=-1"),
        ("SourceIndex=2\n", ""),
        ("SourceIndex=0\n", ""),
        ("Seed=77\n", ""),
        ("Name=P2", "Name=p1"),
        ("[Other2]", "[Other3]"),
        ("PlayerCount=3", "PlayerCount=4"),
        ("PlayerCount=3", "PlayerCount=2"),
        ("[Settings]", "[Settings]\nIsSinglePlayer=Yes"),
        ("[Settings]", "[Settings]\nIsSpectator=Yes"),
    ] {
        assert!(
            SpawnRequest::parse(&valid.replacen(from, to, 1)).is_err(),
            "{from} -> {to}"
        );
    }
    let alliances = "[Multi1_Alliances]\nHouseAllyOne=1\n[Multi2_Alliances]\nHouseAllyOne=0\n";
    let session = parse_network(&format!("{valid}{alliances}")).session;
    assert_eq!(session.humans[0].slot.team, LaunchTeam::Team(0));
    assert_eq!(session.humans[1].slot.team, LaunchTeam::Team(0));
    assert_eq!(session.humans[2].slot.team, LaunchTeam::None);
    assert!(
        SpawnRequest::parse(&format!("{valid}{alliances}[HouseTeams]\nMulti3=0\n")).is_err(),
        "a file may not mix team forms"
    );
}

#[test]
fn single_player_spawn_keeps_its_offline_admission() {
    let SpawnGame::Skirmish(session) = SpawnRequest::parse(SKIRMISH).unwrap().game else {
        panic!("single-player skirmish")
    };
    assert_eq!(session.humans.len(), 1);
    assert_eq!(session.humans[0].raw_slot_index, Some(0));
    let rows: Vec<_> = session
        .opponents
        .iter()
        .map(|ai| ai.raw_slot_index)
        .collect();
    assert_eq!(rows, [1, 2]);
    for append in [relay_section(0, 1), "[Other12]\nName=Remote\n".into()] {
        assert!(SpawnRequest::parse(&format!("{SKIRMISH}\n{append}")).is_err());
    }
    // Retail's eight waypoints still bound a single-player start.
    let far = SKIRMISH.replacen("Multi2=0\n[Multi1", "Multi2=8\n[Multi1", 1);
    assert!(SpawnRequest::parse(&far).is_err());
}

#[test]
fn relay_clock_names_the_game_speed_and_admission_is_bounded() {
    let text = network_spawn(2, 1, &relay_section(1, 0b11));
    let request = SpawnRequest::parse(&text).unwrap();
    assert_eq!(request.game_speed, 1, "45 Hz is launch speed 1");
    let network = parse_network(&text);
    assert_eq!(network.session.options.game_speed, 1);
    let SpawnTransport::Relay(relay) = network.transport else {
        panic!("relay transport")
    };
    assert_eq!(relay.admission_timeout.as_secs(), 30);
    let slowest = network_spawn(2, 1, &relay_section_at(1, 0b11, 10));
    assert_eq!(SpawnRequest::parse(&slowest).unwrap().game_speed, 6);
    let agreed = text.replacen("[Settings]\n", "[Settings]\nGameSpeed=1\n", 1);
    assert_eq!(SpawnRequest::parse(&agreed).unwrap().game_speed, 1);
    let longest = text.replacen("[Relay]\n", "[Relay]\nAdmissionTimeout=180\n", 1);
    let SpawnTransport::Relay(relay) = parse_network(&longest).transport else {
        panic!("relay transport")
    };
    assert_eq!(relay.admission_timeout.as_secs(), 180);
    for broken in [
        text.replacen("[Settings]\n", "[Settings]\nGameSpeed=2\n", 1),
        text.replacen("[Relay]\n", "[Relay]\nAdmissionTimeout=29\n", 1),
        text.replacen("[Relay]\n", "[Relay]\nAdmissionTimeout=181\n", 1),
    ] {
        assert!(SpawnRequest::parse(&broken).is_err(), "{broken}");
    }
}

#[test]
fn network_spectators_watch_without_ai_start_or_team() {
    // Source 2 watches: source 0's file keys it Multi3, its own file Multi1.
    let session = parse_network(&network_spawn(3, 0, "[IsSpectator]\nMulti3=Yes\n")).session;
    let observers: Vec<_> = session.humans.iter().map(|h| h.is_observer()).collect();
    assert_eq!(observers, [false, false, true]);
    assert!(session.house_roster_is_consistent());
    let own = parse_network(&network_spawn(3, 2, "[IsSpectator]\nMulti1=Yes\n")).session;
    assert!(own.local_player().is_observer());
    assert_eq!(
        own.humans
            .iter()
            .map(|h| h.is_observer())
            .collect::<Vec<_>>(),
        observers
    );
    for broken in [
        network_spawn(
            3,
            0,
            "[IsSpectator]\nMulti3=Yes\n[HouseHandicaps]\nMulti4=1\n\
             [HouseCountries]\nMulti4=3\n[HouseColors]\nMulti4=6\n",
        )
        .replacen("[Settings]\n", "[Settings]\nAIPlayers=1\n", 1),
        network_spawn(2, 0, "[IsSpectator]\nMulti2=Yes\n"),
        network_spawn(
            3,
            0,
            "[IsSpectator]\nMulti3=Yes\n[SpawnLocations]\nMulti3=4\n",
        ),
        network_spawn(3, 0, "[IsSpectator]\nMulti3=Yes\n[HouseTeams]\nMulti3=1\n"),
    ] {
        assert!(SpawnRequest::parse(&broken).is_err(), "{broken}");
    }
    // Single player still refuses a spectator.
    assert!(SpawnRequest::parse(&format!("{SKIRMISH}\n[IsSpectator]\nMulti1=Yes")).is_err());
}

#[test]
fn expected_values_are_optional_validated_and_rejected_precisely() {
    let request = SpawnRequest::parse(SKIRMISH).unwrap();
    assert_eq!(request.expected, SpawnExpectations::default());
    let digest = "AB".repeat(32);
    let text = SKIRMISH.replace(
        "[Settings]\n",
        &format!(
            "[Settings]\nExpectedVersion=1.0:x:000\nExpectedRulesDigest={digest}\nExpectedMapDigest={}\n",
            "cd".repeat(32)
        ),
    );
    let request = SpawnRequest::parse(&text).unwrap();
    assert_eq!(request.expected.version.as_deref(), Some("1.0:x:000"));
    assert_eq!(request.expected.rules_digest, Some("ab".repeat(32)));
    let ok = || Ok::<_, anyhow::Error>("ab".repeat(32));
    let map = || Ok::<_, anyhow::Error>("cd".repeat(32));
    request.expected.mismatch("1.0:x:000", ok, map).unwrap();
    let error = request
        .expected
        .mismatch("1.0:y:000", ok, map)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("ExpectedVersion") && error.contains("1.0:y:000"),
        "{error}"
    );
    let error = request
        .expected
        .mismatch("1.0:x:000", || Ok("00".repeat(32)), map)
        .unwrap_err()
        .to_string();
    assert!(error.contains("ExpectedRulesDigest"), "{error}");
    let error = request
        .expected
        .mismatch("1.0:x:000", ok, || Ok("11".repeat(32)))
        .unwrap_err()
        .to_string();
    assert!(error.contains("ExpectedMapDigest"), "{error}");
    // Absent values are never computed.
    SpawnExpectations::default()
        .mismatch("v", || panic!("rules"), || panic!("map"))
        .unwrap();
    assert!(
        SpawnRequest::parse(
            &SKIRMISH.replace("[Settings]\n", "[Settings]\nExpectedMapDigest=xyz\n")
        )
        .is_err()
    );
    assert!(
        SpawnRequest::parse(&format!(
            "[Settings]\nIsSinglePlayer=Yes\nScenario=ALL01uMd.map\nExpectedMapDigest={digest}\n"
        ))
        .is_err()
    );
    // Campaigns may still pin the build.
    SpawnRequest::parse(
        "[Settings]\nIsSinglePlayer=Yes\nScenario=ALL01uMd.map\nExpectedVersion=v\n",
    )
    .unwrap();
}
