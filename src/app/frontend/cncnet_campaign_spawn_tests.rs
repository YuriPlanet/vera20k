//! External campaign admission through the production parser, retained catalog
//! and ordinary startup/continuation owner. Synthetic inputs contain no mod data.
//! This is Rust adapter coverage, not native CnCNet patch parity.

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
    // ART movie selection stays on its existing owner even in this synthetic
    // fixture; no private cross-crate constructor or second movie list.
    let owner = crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::empty(),
        None,
        IniFile::from_str("[Movies]\n0=FIRST\n1=FINISH\n"),
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
    ))
    .unwrap()
}

const CUSTOM_CATALOG: &str = "\
[Battles]
0=FIRST_MISSION
1=SECOND_MISSION
[FIRST_MISSION]
CD=1
Scenario=OTHER.MAP
[SECOND_MISSION]
CD=2
Scenario=MISSION.MAP
FinalMovie=FINISH
";

#[cfg(feature = "ares")]
#[test]
fn cncnet_campaign_spawn_custom_scenario_preserves_catalog_and_physical_identity() {
    use crate::map::source::test_support::TestDirectory;

    let catalog = catalog(CUSTOM_CATALOG);
    let client = TestDirectory::new("cncnet-custom-campaign");
    client.write("mission.Map", b"synthetic path identity fixture");
    for side in [0, 2, 3, 255] {
        client.write(
            "spawn.ini",
            format!(
                "[Settings]\nIsSinglePlayer=Yes\nScenario=mission.Map\nSide={side}\nCampaignID=0\nGameSpeed=2\nDifficultyModeHuman=2\nDifficultyModeComputer=0\n"
            )
            .as_bytes(),
        );
        let request = SpawnRequest::read(&client.path().join("spawn.ini")).unwrap();
        let SpawnGame::Campaign(campaign) = request.game else {
            panic!("campaign request");
        };
        let mut clock = CountingClock::default();
        let startup = campaign
            .prepare_startup(&catalog, request.game_speed, &mut clock)
            .unwrap();
        assert_eq!(
            startup.campaign_index(),
            1,
            "CampaignID=0 is not a retail index"
        );
        assert_eq!(startup.campaign().id(), "SECOND_MISSION");
        assert_eq!(startup.campaign().cd(), 2);
        assert_eq!(startup.campaign().final_movie(), 1);
        assert_eq!(startup.scenario(), "MISSION.MAP");
        assert_eq!(
            startup.selected_map_file(),
            client.path().join("mission.Map").to_str().unwrap()
        );
        assert_eq!(startup.difficulty(), 2);
        assert_eq!(startup.game_options().unwrap().game_speed, 2);
        assert_eq!(clock.0, 1);
        let next = startup.next_mission(
            "NEXT.Map".into(),
            2,
            2,
            2,
            crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
            &mut clock,
        );
        assert_eq!(next.campaign_index(), startup.campaign_index());
        assert_eq!(next.campaign(), startup.campaign());
        assert_eq!(next.scenario(), "NEXT.MAP");
        assert_eq!(clock.0, 2);
    }
}

#[test]
fn cncnet_campaign_spawn_stock_override_and_compiled_lookup_priority() {
    let catalog = catalog(
        "[Battles]\n0=ALL1\n1=SOV1\n2=CUSTOM\n[ALL1]\nScenario=ALL01UMD.MAP\n[SOV1]\nScenario=SOV01UMD.MAP\n[CUSTOM]\nScenario=CUSTOM.MAP\n",
    );
    for (side, scenario, index) in [(0, "ALL04uMd.map", 0), (1, "SOV03uMd.map", 1)] {
        let request = request(scenario, side, 91);
        let SpawnGame::Campaign(campaign) = request.game else {
            panic!("campaign request");
        };
        let mut clock = CountingClock::default();
        let startup = campaign
            .prepare_startup(&catalog, request.game_speed, &mut clock)
            .unwrap();
        assert_eq!(startup.campaign_index(), index);
        assert_eq!(startup.selected_map_file(), scenario);
        assert_eq!(clock.0, 1);
    }
    let request = request("CUSTOM.MAP", 0, 0);
    let SpawnGame::Campaign(campaign) = request.game else {
        panic!("campaign request");
    };
    let mut clock = CountingClock::default();
    let startup = campaign.prepare_startup(&catalog, 2, &mut clock).unwrap();
    assert_eq!(
        startup.campaign_index(),
        if crate::rules::ARES_ENABLED { 2 } else { 0 }
    );
    assert_eq!(clock.0, 1);
}

#[test]
fn cncnet_campaign_spawn_unknown_mission_never_uses_client_index_or_reads_seed() {
    let catalog = catalog(CUSTOM_CATALOG);
    for side in [0, 2, 3] {
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
        assert_eq!(clock.0, 0, "failed admission precedes seed consumption");
    }
    for side in [-1, 256] {
        assert!(
            SpawnRequest::parse(&format!("[Settings]\nIsSinglePlayer=Yes\nSide={side}\n")).is_err()
        );
    }
}

#[cfg(feature = "ares")]
#[test]
fn cncnet_campaign_spawn_scenario_keeps_index_when_native_stored_ids_collide() {
    let stored = "ABCDEFGHIJKLMNOPQRSTUVWX";
    let incoming = format!("{stored}TAIL");
    let mut catalog = catalog(&format!(
        "[Battles]\n0={incoming}\n[{stored}]\nScenario=FIRST.MAP\n"
    ));
    catalog.apply_ini(
        &IniFile::from_str(&format!(
            "[Battles]\n0={incoming}\n[{stored}]\nScenario=SECOND.MAP\nCD=2\n"
        )),
        &crate::rules::movies::MovieRegistry::default(),
    );
    assert_eq!(catalog.get(0).unwrap().id(), catalog.get(1).unwrap().id());
    assert_eq!(catalog.find_scenario_index("second.map"), Some(1));
    assert_eq!(catalog.find_scenario_index(""), None);
    assert_eq!(catalog.find_scenario_index("path/SECOND.MAP"), None);
    let SpawnGame::Campaign(campaign) = request("second.map", 3, 0).game else {
        panic!("campaign request");
    };
    let mut clock = CountingClock::default();
    let startup = campaign.prepare_startup(&catalog, 2, &mut clock).unwrap();
    assert_eq!(startup.campaign_index(), 1);
    assert_eq!(startup.campaign().cd(), 2);
    assert_eq!(startup.campaign().scenario(), "SECOND.MAP");
    assert_eq!(clock.0, 1);
}
