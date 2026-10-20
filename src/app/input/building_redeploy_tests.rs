//! Building4436F0: archive the clicked Cell, then enqueue SELL16.
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

fn step(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    sim.advance_app_frame(
        &commands,
        Some(rules),
        None,
        33,
        crate::sim::world::TickLane::Ordinary,
        Default::default(),
    )
    .unwrap();
}

#[test]
fn retail_mcvs_repack_and_follow_the_ground_click_after_deployment() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut rules = retail.rules;
    for yard in ["GACNST", "NACNST", "YACNST"] {
        // This INI fixture does not load SHPs. Supply a finite build-up
        // animation binding; its timing is an authored test input.
        rules.set_buildup_control_for_test(yard, [0, 29, 1]);
    }
    for (mcv, yard_type, contact) in [("AMCV", "GACNST"), ("SMCV", "NACNST"), ("PCV", "YACNST")]
        .into_iter()
        .flat_map(|(mcv, yard)| [false, true].map(move |contact| (mcv, yard, contact)))
    {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4436F0);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.get("Americans").unwrap();
        sim.session.set_current_house_for_test(Some(owner));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session.game_options_mut_for_test().mcv_redeploy = true;
        sim.session.game_options_mut_for_test().short_game = false;
        sim.input_delay_ticks = 0;
        sim.spawn_object("GAPOWR", "Russians", 3, 3, 0, &rules)
            .unwrap();
        let actor = sim
            .spawn_object(mcv, "Americans", 12, 12, 96, &rules)
            .unwrap();
        assert!(sim.apply_command(
            "Americans",
            &Command::DeployMcv { entity_id: actor },
            Some(&rules)
        ));
        for _ in 0..120 {
            step(&mut sim, &rules);
        }
        let yard = sim
            .entities()
            .values()
            .find(|e| sim.interner.resolve(e.type_ref()) == yard_type)
            .unwrap_or_else(|| panic!("{mcv} must deploy into {yard_type}"))
            .stable_id();
        assert!(
            !sim.entities().get(yard).unwrap().building_up(),
            "{yard_type}"
        );
        let peer = sim
            .spawn_object("GAPOWR", "Americans", 5, 5, 0, &rules)
            .unwrap();
        if contact {
            // Supplied existing radio state. The sale owner must retire both
            // sides, rather than requiring an empty link at input time.
            sim.entities_mut()
                .get_mut(yard)
                .unwrap()
                .radio_contacts
                .insert(peer)
                .unwrap();
            sim.entities_mut()
                .get_mut(peer)
                .unwrap()
                .radio_contacts
                .insert(yard)
                .unwrap();
        }
        let (orders, announce) = building_cell_orders(&sim, &rules, &[yard], owner, (23, 20));
        assert!(!announce);
        assert!(
            matches!(orders.as_slice(), [Command::SetRally { .. }, Command::SellBuilding { entity_id }]
            if *entity_id == yard),
            "{yard_type}: {orders:?}"
        );
        let goal = match orders[0] {
            Command::SetRally { rx, ry, .. } => (rx, ry),
            _ => unreachable!(),
        };
        let envelopes: Vec<_> = orders
            .into_iter()
            .map(|payload| CommandEnvelope::new(owner, sim.session.tick(), payload))
            .collect();
        sim.queue_commands(envelopes);
        let mut converted = None;
        for _ in 0..80 {
            step(&mut sim, &rules);
            converted = sim
                .entities()
                .values()
                .find(|e| sim.interner.resolve(e.type_ref()) == mcv)
                .map(|e| e.stable_id());
            if converted.is_some() {
                break;
            }
        }
        assert!(
            sim.entities().get(yard).is_none(),
            "{yard_type} did not repack: {:?}",
            sim.entities().get(yard).map(|e| (
                e.mission.effective(),
                e.archive_target(),
                e.building_down()
            ))
        );
        let converted = converted.expect("MCV must return");
        assert!(sim.entities().get(peer).unwrap().radio_contacts.is_empty());
        let unit = sim.entities().get(converted).unwrap();
        assert!(
            [unit.mission.current(), unit.mission.queued()]
                .iter()
                .any(|mission| mission.known() == Some(MissionType::Move)),
            "{mcv} must receive the archived move"
        );
        for _ in 0..600 {
            step(&mut sim, &rules);
            let unit = sim.entities().get(converted).unwrap();
            if (unit.position.rx(), unit.position.ry()) == goal {
                break;
            }
        }
        let unit = sim.entities().get(converted).unwrap();
        assert_eq!((unit.position.rx(), unit.position.ry()), goal, "{mcv}");
    }
}

/// Original Building4436F0 admits both empty and populated RadioClass
/// contacts; option/mode/House/mind-control gates are44374F..443778.
/// Contact membership is a supplied state here, not a stock producer claim.
#[test]
fn construction_yard_ground_orders_match_native_redeploy_admission() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    for (name, enabled, skirmish, human, contact, admitted) in [
        ("enabled", true, true, true, false, true),
        ("radio contact", true, true, true, true, true),
        ("disabled", false, true, true, false, false),
        ("campaign", true, false, true, false, false),
        ("computer", true, true, false, false, false),
    ] {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4436F0);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.get("Americans").unwrap();
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_is_human_for_test(human);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(skirmish),
        );
        sim.session.game_options_mut_for_test().mcv_redeploy = enabled;
        let yard = sim
            .spawn_object("GACNST", "Americans", 12, 12, 0, &rules)
            .unwrap();
        let peer = sim
            .spawn_object("GAPOWR", "Americans", 5, 5, 0, &rules)
            .unwrap();
        if contact {
            sim.entities_mut()
                .get_mut(yard)
                .unwrap()
                .radio_contacts
                .insert(peer)
                .unwrap();
        }
        assert_eq!(
            sim.takes_rally_point(yard, "Americans", &rules),
            admitted,
            "{name}"
        );
        let (orders, _) = building_cell_orders(&sim, &rules, &[yard], owner, (23, 20));
        assert_eq!(!orders.is_empty(), admitted, "{name}");
    }
}
