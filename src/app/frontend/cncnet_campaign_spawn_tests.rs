//! External campaign admission and ordinary startup against native prefix outputs.
//! Synthetic inputs contain no mod data; full map/gameplay parity is not claimed.

use super::*;
use crate::app::match_bootstrap::{MatchSeedClock, MatchSeedSource};
use crate::rules::campaigns::CampaignRegistry;

#[derive(Default)]
struct CountingClock(usize);

impl MatchSeedClock for CountingClock {
    fn low_u32(&mut self) -> u32 {
        self.0 += 1;
        137
    }
    fn source(&self) -> MatchSeedSource {
        MatchSeedSource::Controlled
    }
    fn seed_authority_certifying(&self) -> bool {
        false
    }
}

fn catalog(ini: &str) -> CampaignRegistry {
    let owner = crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::empty(),
        None,
        IniFile::from_str("[Movies]\n0=END0\n1=END1\n"),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let mut catalog = CampaignRegistry::default();
    catalog.apply_ini(&IniFile::from_str(ini), owner.movies());
    catalog
}

fn request(scenario: &str, side: u8, campaign_id: i32) -> SpawnRequest {
    SpawnRequest::parse(&format!(
        "[Settings]\nIsSinglePlayer=Yes\nScenario={scenario}\nSide={side}\nCampaignID={campaign_id}\nGameSpeed=2\nDifficultyModeHuman=2\nDifficultyModeComputer=0\n"
    )).unwrap()
}

const CUSTOM_CATALOG: &str = "\
[Battles]
0=FIRST
1=SECOND
[FIRST]
CD=2
Scenario=FIRST.MAP
FinalMovie=END0
[SECOND]
CD=1
Scenario=SECOND.MAP
FinalMovie=END1
";

#[test]
fn cncnet_campaign_spawn_explicit_scenario_keeps_first_record_and_physical_identity() {
    use crate::map::source::test_support::TestDirectory;
    let catalog = catalog(CUSTOM_CATALOG);
    let client = TestDirectory::new("cncnet-explicit-campaign");
    for scenario in ["other.Map", "second.Map"] {
        client.write(scenario, b"synthetic path identity fixture");
        for side in [0, 1, 2, 3, 255] {
            client.write("spawn.ini", format!(
                "[Settings]\nIsSinglePlayer=Yes\nScenario={scenario}\nSide={side}\nCampaignID=1\nGameSpeed=2\nDifficultyModeHuman=2\nDifficultyModeComputer=0\n"
            ).as_bytes());
            let request = SpawnRequest::read(&client.path().join("spawn.ini"));
            if !crate::rules::ARES_ENABLED && side > 1 {
                assert!(request.is_err());
                continue;
            }
            let request = request.unwrap();
            let SpawnGame::Campaign(campaign) = request.game else {
                panic!("campaign request");
            };
            let mut clock = CountingClock::default();
            let startup = campaign
                .prepare_startup(&catalog, request.game_speed, &mut clock)
                .unwrap();
            assert_eq!(
                startup.campaign_index(),
                0,
                "Side/CampaignID/Scenario never select a record"
            );
            assert_eq!(startup.campaign(), catalog.get(0).unwrap());
            assert_eq!(startup.campaign().cd(), 2);
            assert_eq!(startup.campaign().final_movie(), 0);
            assert_eq!(startup.scenario(), scenario.to_ascii_uppercase());
            assert_eq!(
                startup.selected_map_file(),
                client.path().join(scenario).to_str().unwrap()
            );
            assert_eq!(startup.difficulty(), 2);
            assert_eq!(clock.0, 1);
            let restarted = startup.restart(1, 2, &mut clock);
            assert_eq!(restarted.campaign(), startup.campaign());
            assert_eq!(restarted.campaign_index(), 0);
            assert_eq!(restarted.scenario(), startup.scenario());
            assert_eq!(restarted.selected_map_file(), startup.selected_map_file());
            assert!(!restarted.plays_action_movie());
            let next = restarted.next_mission(
                "NEXT.Map".into(),
                2,
                1,
                2,
                crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
                &mut clock,
            );
            assert_eq!(next.campaign_index(), 0);
            assert_eq!(next.campaign(), startup.campaign());
            assert_eq!(next.scenario(), "NEXT.MAP");
            assert_eq!(clock.0, 3);
        }
    }
}

#[test]
fn cncnet_campaign_spawn_native_start_prefix_keeps_filename_index_and_cd_independent() {
    let native: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/input_oracle/campaign_start_prefix.json"
    ))
    .unwrap();
    let catalog = catalog(CUSTOM_CATALOG);
    for row in native["rows"].as_array().unwrap() {
        let mut clock = CountingClock::default();
        let startup = crate::app::match_bootstrap::prepare_campaign_startup_at_index(
            &catalog,
            row["campaign_index"].as_u64().unwrap() as usize,
            1,
            row["filename"].as_str(),
            None,
            &mut clock,
        )
        .unwrap();
        assert_eq!(
            startup.scenario(),
            row["logical_scenario"].as_str().unwrap()
        );
        assert_eq!(
            startup.campaign_index() as u64,
            row["stored_campaign_index"].as_u64().unwrap()
        );
        assert_eq!(
            i64::from(startup.campaign().cd()),
            row["selected_media_cd"].as_i64().unwrap()
        );
        assert_eq!(
            startup.campaign().id(),
            row["selected_record"]["id"].as_str().unwrap()
        );
        assert_eq!(
            i64::from(startup.campaign().final_movie()),
            row["selected_record"]["final_movie"].as_i64().unwrap()
        );
        assert_eq!(clock.0, 1);
    }
}

#[test]
fn cncnet_campaign_spawn_keeps_ordinary_named_menu_selection() {
    let catalog = catalog(CUSTOM_CATALOG);
    let mut clock = CountingClock::default();
    let startup = crate::app::match_bootstrap::prepare_campaign_startup(
        &catalog, "SECOND", 1, None, None, &mut clock,
    )
    .unwrap();
    assert_eq!(startup.campaign_index(), 1);
    assert_eq!(startup.campaign(), catalog.get(1).unwrap());
    assert_eq!(startup.scenario(), "SECOND.MAP");
    assert_eq!(clock.0, 1);
}

#[test]
fn cncnet_campaign_spawn_empty_catalog_fails_before_seed() {
    let catalog = CampaignRegistry::default();
    for side in [0, 1, 2, 3] {
        let parsed = SpawnRequest::parse(&format!(
            "[Settings]\nIsSinglePlayer=Yes\nScenario=UNKNOWN.MAP\nSide={side}\nCampaignID=1\n"
        ));
        if !crate::rules::ARES_ENABLED && side > 1 {
            assert!(parsed.is_err());
            continue;
        }
        let SpawnGame::Campaign(campaign) = parsed.unwrap().game else {
            panic!("campaign request");
        };
        let mut clock = CountingClock::default();
        assert!(campaign.prepare_startup(&catalog, 2, &mut clock).is_err());
        assert_eq!(clock.0, 0);
    }
    for side in [-1, 256] {
        assert!(
            SpawnRequest::parse(&format!("[Settings]\nIsSinglePlayer=Yes\nSide={side}\n")).is_err()
        );
    }
}

#[test]
fn cncnet_campaign_spawn_unlisted_body_does_not_repair_the_native_catalog() {
    let catalog = catalog(
        "[Battles]\n0=FIRST\n1=BROKEN\n[FIRST]\nCD=2\nScenario=FIRST.MAP\n[TYPO]\nScenario=OTHER.MAP\n",
    );
    assert_eq!(catalog.get(1).unwrap().scenario(), "");
    let SpawnGame::Campaign(campaign) = request("OTHER.MAP", 0, 1).game else {
        panic!("campaign request");
    };
    let mut clock = CountingClock::default();
    let startup = campaign.prepare_startup(&catalog, 2, &mut clock).unwrap();
    assert_eq!(startup.campaign_index(), 0);
    assert_eq!(startup.campaign().id(), "FIRST");
    assert_eq!(startup.scenario(), "OTHER.MAP");
    assert_eq!(catalog.get(1).unwrap().scenario(), "");
    assert_eq!(clock.0, 1);
}
