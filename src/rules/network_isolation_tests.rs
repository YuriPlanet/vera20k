use super::*;
use crate::rules::powerups::POWERUP_MONEY;
use crate::rules::terrain_rules::{LandType, test_speed_percent};

fn sparse_owner() -> NativeRulesProcessOwner {
    NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str("[Animations]\n0=BASE\n"),
        None,
        IniFile::empty(),
        Arc::default(),
        Arc::default(),
    )
    .unwrap()
}

#[test]
fn network_match_discards_retained_scalars_and_tables_but_keeps_host_options() {
    let mut warm = sparse_owner();
    let history = IniFile::from_str(
        "[AudioVisual]\nGravity=19\n[General]\nMaximumCheerRate=37\n\
         [Clear]\nWheel=25%\n[Animations]\n0=HISTORY\n\
         [Powerups]\nMoney=7,HISTORY,yes,125\n",
    );
    warm.load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &history)
        .unwrap();
    let target = IniFile::empty();
    let offline = warm
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &target)
        .unwrap();
    assert_eq!(offline.rules.general.gravity, 19);
    assert_eq!(offline.rules.general.maximum_cheer_rate(), 37);
    assert_eq!(offline.rules.powerups.weights[POWERUP_MONEY], 7);
    assert_eq!(
        offline
            .rules
            .terrain_rules
            .row(LandType::Clear)
            .speed_costs
            .wheel,
        Some(test_speed_percent(25.0)),
        "offline Reset6686C0 retains unauthored land rows"
    );

    warm.set_terrain_deformation(true);
    let host_options = warm.personal_rules_compatibility_crc();
    warm.reset_for_network_match().unwrap();
    let actual = warm
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &target)
        .unwrap();
    let mut cold = sparse_owner();
    cold.set_terrain_deformation(true);
    let expected = cold
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &target)
        .unwrap();
    assert_eq!(warm.personal_rules_compatibility_crc(), host_options);
    assert!(actual.rules.combat_damage.terrain_deformation);
    assert_eq!(actual.rules.general.gravity, expected.rules.general.gravity);
    assert_eq!(
        actual.rules.terrain_rules.row(LandType::Clear),
        expected.rules.terrain_rules.row(LandType::Clear)
    );
    assert_eq!(actual.rules.powerups, expected.rules.powerups);
    assert_eq!(
        actual.rules.simulation_config_hash(),
        expected.rules.simulation_config_hash()
    );
    assert_eq!(
        actual.receipt.pre_reset.events,
        expected.receipt.pre_reset.events
    );
    assert_eq!(
        actual.receipt.post_reset.events,
        expected.receipt.post_reset.events
    );
}

#[test]
fn retail_network_match_removes_prior_map_tiberium_and_restores_cold_type_ids() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let target = IniFile::empty();
    let mut cold = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let expected = cold
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &target)
        .unwrap();
    let slot = expected.rules.tiberium_types.len();
    let history = IniFile::from_str(&format!(
        "[Tiberiums]\n{slot}=ONLINELEAK\n[ONLINELEAK]\nName=Previous map ore\nValue=99\n"
    ));
    let mut warm = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let previous = warm
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &history)
        .unwrap();
    assert_eq!(previous.rules.tiberium_types.len(), slot + 1);
    assert_eq!(
        previous.rules.tiberium_types.types()[slot].section,
        "ONLINELEAK"
    );

    warm.reset_for_network_match().unwrap();
    let actual = warm
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &target)
        .unwrap();
    assert_eq!(actual.rules.tiberium_types.len(), slot);
    assert_eq!(
        actual.rules.source_ini_hash(),
        expected.rules.source_ini_hash()
    );
    assert_eq!(
        actual.rules.simulation_config_hash(),
        expected.rules.simulation_config_hash()
    );
    // A solo network game consumes these constructor events for its native IDs;
    // equal final registries alone would miss the discarded cold-start prefix.
    assert_eq!(
        actual.receipt.pre_reset.events,
        expected.receipt.pre_reset.events
    );
    assert_eq!(
        actual.receipt.post_reset.events,
        expected.receipt.post_reset.events
    );
}
