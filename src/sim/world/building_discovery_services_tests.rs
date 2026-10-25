//! Campaign-controlled building services retain the current player's discovery.
use super::*;
use crate::map::map_file::MapFile;
use crate::rules::process_owner::NativeScenarioRulesPrefix;
use crate::sim::campaign_carry_over::CampaignCarryOver;
use crate::sim::snapshot::GameSnapshot;

fn campaign_fixture(triggers: &str) -> Option<(Simulation, RuleSet, InternedId)> {
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let map = MapFile::from_bytes(
        format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,16,16\nLocalSize=0,0,16,16\n\
             [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
             [Basic]\nNewINIFormat=4\nPlayer=Player\n\
             [Houses]\n0=Remote\n1=Player\n\
             [Remote]\nCountry=Americans\nPlayerControl=yes\n\
             [Player]\nCountry=Russians\nPlayerControl=yes\n\
             [Waypoints]\n0=10010\n1=10018\n2=18010\n{triggers}"
        )
        .as_bytes(),
    )
    .unwrap();
    let (rules, _, _, _) = crate::rules::retail_ini_fixture::retail_rules_owner(&assets)
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &map.ini)
        .unwrap()
        .into_parts();
    // Reuse CreateBuilding's complete placement substrate, including its
    // native tile catalog. Only the empty world is reused: provider types,
    // foundations, power and sight come from the retail campaign reader.
    let (mut sim, _, _) = entry_test_fixture::fixture();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.fog = vision::FogState::new(33, 33);
    sim.session
        .initialize_campaign_startup(1, 1, None, 0, CampaignCarryOver::CLEARED)
        .unwrap();
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&map.ini),
    );
    crate::sim::scenario_bootstrap::initialize_campaign_current_house(
        &mut sim.session,
        &mut sim.houses,
        &map.ini,
    );
    sim.session.install_waypoints(&map.waypoints);
    sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
    sim.classify_map_triggers(&rules);
    sim.session.publish_started();
    let remote = sim.interner.get("Remote").unwrap();
    let player = sim.interner.get("Player").unwrap();
    sim.fog.ensure_viewer(player);
    sim.fog.ensure_viewer(remote);
    assert_eq!(sim.session.current_house(), Some(player));
    assert!(sim.houses[&remote].is_controlled_by_human(false));
    assert!(!sim.houses[&remote].is_human());
    assert!(
        !sim.house_alliances()
            .get("REMOTE")
            .is_some_and(|allies| allies.contains("PLAYER"))
    );
    Some((sim, rules, remote))
}

fn frame(sim: &mut Simulation, rules: &RuleSet) {
    let output = sim
        .advance_app_frame(
            &[],
            rules,
            Some(rules.overlay_registry()),
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(output.tick.frame_committed);
}

fn provider_ids(sim: &Simulation, owner: InternedId) -> Vec<u64> {
    sim.houses[&owner].base_projection().buildings().to_vec()
}

#[test]
fn remote_campaign_services_wait_for_ordinary_discovery_and_retain_it_on_load() {
    // Execute125/6E4200 binds Country through TriggerSpring7265C0/502D30;
    // no direct action context or owner/discovery/power fixture bypass.
    let Some((mut sim, rules, remote)) = campaign_fixture(
        "[Triggers]\nCREATE=Americans,<none>,Create,0,1,1,1,0\n\
         REVEAL=Russians,<none>,Reveal,0,1,1,1,0\n\
         [Events]\nCREATE=1,13,0,0\nREVEAL=1,13,0,1\n\
         [Actions]\n\
         CREATE=3,125,10,GAPOWR,0,0,0,0,A,125,10,GAAIRC,0,0,0,0,B,125,10,GASPYSAT,0,0,0,0,C\n\
         REVEAL=3,17,0,0,0,0,0,0,A,17,0,1,0,0,0,0,B,17,0,2,0,0,0,0,C\n\
         [Tags]\nCREATE_TAG=0,Create,CREATE\nREVEAL_TAG=0,Reveal,REVEAL\n",
    ) else {
        return;
    };
    assert_eq!(rules.object("GAPOWR").unwrap().power, 200);
    assert_eq!(rules.object("GAAIRC").unwrap().power, -50);
    assert_eq!(rules.object("GASPYSAT").unwrap().power, -100);
    assert!(rules.object("GAAIRC").unwrap().radar);
    assert!(rules.object("GASPYSAT").unwrap().spy_sat);
    frame(&mut sim, &rules);
    frame(&mut sim, &rules);
    let ids = provider_ids(&sim, remote);
    assert_eq!(
        ids.len(),
        3,
        "the timed trigger creates all retail providers"
    );
    for &id in &ids {
        let building = sim.substrate.entities.get(id).unwrap();
        assert_eq!(building.owner(), remote);
        assert!(building.lifecycle.cell_marked() && !building.lifecycle.in_limbo());
        assert!(!building.discovery.discovered_by_current_house());
    }
    // House508CC7,508ECD,508FAB reject +41B=false only for controlled
    // campaign Houses. Added_To_Game's immediate drain is not this scan.
    assert_eq!(sim.power_states()[&remote].total_output(), 0);
    assert_eq!(sim.power_states()[&remote].total_drain(), 0);
    assert!(!power_system::has_active_radar(sim.power_states(), remote));
    assert!(!sim.houses[&remote].spy_sat_active());

    let bytes = GameSnapshot::save_validated(&sim, 0, 0, "discovery services", 0);
    let mut hidden = GameSnapshot::load_validated(&bytes, 0, 0, sim.session.map_name())
        .unwrap()
        .sim;
    hidden.restore_after_snapshot_load().unwrap();
    for &id in &ids {
        assert!(
            !hidden
                .substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house()
        );
    }
    // The same undiscovered providers are admitted outside campaign mode.
    // This invokes the production House prefix on the restored hidden prior;
    // the fixture does not run skirmish defeat on its intentionally empty Player.
    hidden
        .session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    Simulation::invalidate_house_power(&hidden.houses, &mut hidden.power_states, remote, true);
    hidden.assess_house_derived_state(remote, &rules, FrameEffects::default());
    assert_eq!(hidden.power_states()[&remote].total_output(), 200);
    assert!(power_system::has_active_radar(
        hidden.power_states(),
        remote
    ));
    assert!(hidden.houses[&remote].spy_sat_active());
    // Event13's ordinary 15-frame timer admits Action17. Its cell Notify
    // reaches DiscoveredBy6F4960 and dirties both retained House services.
    while sim.session.binary_frame() <= 16 {
        frame(&mut sim, &rules);
    }
    for &id in &ids {
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house()
        );
    }
    assert!(sim.houses[&remote].discovered_by_current_house());
    assert_eq!(sim.power_states()[&remote].total_output(), 200);
    assert_eq!(sim.power_states()[&remote].total_drain(), 150);
    assert!(power_system::has_active_radar(sim.power_states(), remote));
    assert!(sim.houses[&remote].spy_sat_active());
    let bytes = GameSnapshot::save_validated(&sim, 0, 0, "discovery services", 0);
    let mut discovered = GameSnapshot::load_validated(&bytes, 0, 0, sim.session.map_name())
        .unwrap()
        .sim;
    discovered.restore_after_snapshot_load().unwrap();
    for id in ids {
        assert!(
            discovered
                .substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house()
        );
    }
    assert_eq!(discovered.power_states()[&remote].total_output(), 200);
    assert!(discovered.houses[&remote].spy_sat_active());
}

#[test]
fn mapped_retained_objects_promote_silently_in_ordinary_ai_and_action40() {
    let Some((mut sim, rules, remote)) = campaign_fixture(
        "[Triggers]\nCREATE=Americans,<none>,Create,0,1,1,1,0\n\
         DISCOVER=Americans,<none>,Discovered,0,1,1,1,0\n\
         [Events]\nCREATE=1,13,0,0\nDISCOVER=1,4,0,0\n\
         [Actions]\n\
         CREATE=3,125,10,GAPOWR,0,0,0,0,A,125,10,GAAIRC,0,0,0,0,B,125,10,GASPYSAT,0,0,0,0,C\n\
         DISCOVER=1,28,0,7,0,0,0,0,A\n\
         [Tags]\nCREATE_TAG=0,Create,CREATE\nDISCOVER_TAG=0,Discovered,DISCOVER\n",
    ) else {
        return;
    };
    frame(&mut sim, &rules);
    frame(&mut sim, &rules);
    let ids = provider_ids(&sim, remote);
    assert_eq!(ids.len(), 3);
    let clean_power = sim.power_states()[&remote].clone();
    assert!(!clean_power.power_recheck_for_test());
    assert!(!clean_power.radar_recheck_for_test());
    assert!(!sim.houses[&remote].spy_sat_active());
    for waypoint in 0..3 {
        assert!(sim.trigger_reveal_at_waypoint(
            waypoint,
            UninitContext::new(&rules, Some(rules.overlay_registry())),
        ));
    }
    // Supply a retained +41B=false history over genuinely mapped ground.
    // Keep the real prior House assessment, rather than inventing power or
    // visibility. Attach the Tag after mapping to isolate70D990's effects.
    for &id in &ids {
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house()
        );
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .discovery
            .set_discovered_by_current_house_for_test(false);
        sim.attach_object_trigger_tag(id, Some("DISCOVER_TAG"));
    }
    sim.power_states
        .map_mut_for_test()
        .insert(remote, clean_power);
    frame(&mut sim, &rules);
    for &id in &ids {
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house()
        );
    }
    assert!(!sim.global_variables()[7], "70D990 does not raise event4");
    assert!(!sim.power_states()[&remote].power_recheck_for_test());
    assert!(!sim.power_states()[&remote].radar_recheck_for_test());
    assert_eq!(sim.power_states()[&remote].total_output(), 0);
    assert!(
        !sim.houses[&remote].spy_sat_active(),
        "a clean House does not recheck SpySat"
    );

    for &id in &ids {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .discovery
            .set_discovered_by_current_house_for_test(false);
    }
    // Action40/6E225C..6E227C calls the same virtual+4A0 prelude for every
    // Building before returning, even with no following object-AI visit.
    assert!(sim.change_visible_map_area([0, 0, 16, 16], &rules));
    for id in ids {
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .discovery
                .discovered_by_current_house()
        );
    }
    assert!(!sim.global_variables()[7]);
    assert!(!sim.power_states()[&remote].power_recheck_for_test());
    assert!(!sim.power_states()[&remote].radar_recheck_for_test());
    assert!(!sim.houses[&remote].spy_sat_active());
}
