//! Execute61/62 admission, retained Building state and native map reader order.
use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;

fn fixture() -> (Simulation, RuleSet, OverlayTypeRegistry, [InternedId; 3]) {
    let (mut sim, rules, registry, houses) = tests::event_ingress_fixture_with_rules(
        "[BuildingTypes]\n0=LOAD\n1=PRODUCER\n2=POWERED\n3=TECH\n\
         [LOAD]\nStrength=500\nFoundation=1x1\nPower=-10\nFactory=InfantryType\nLightVisibility=1000\nLightIntensity=.5\n\
         [PRODUCER]\nStrength=500\nFoundation=1x1\nPower=100\n\
         [POWERED]\nStrength=500\nFoundation=1x1\nPowered=yes\n\
         [TECH]\nStrength=500\nFoundation=1x1\nPower=-10\nNeedsEngineer=yes\n",
    );
    sim.session.current_house = Some(houses[0]);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nCONTROL=Country0,<none>,CONTROL,0,1,1,1,0\n\
         [Events]\nCONTROL=1,8,0,0\n\
         [Actions]\nCONTROL=2,61,0,-987,0,0,0,0,A,62,0,12345,0,0,0,0,A\n\
         [Tags]\nCONTROLTAG=2,CONTROL,CONTROL\n",
    );
    (sim, rules, registry, houses)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, name: &str, house: &str, x: u16) -> u64 {
    sim.spawn_object(name, house, x, 15, 0, rules).unwrap()
}

fn run(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    kind: i32,
    live: bool,
) -> bool {
    let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
        "[Actions]\nA=1,{kind},0,-987,0,0,0,0,A\n"
    ));
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("A")
        .unwrap()
        .entries()[0]
        .clone();
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
    let live_index = live.then(|| sim.trigger_runtime.trigger_order[0]);
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index,
            incoming_house: Some(sim.session.house_order[2]),
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        Some(registry),
        FrameEffects::default(),
    )
}

#[test]
fn building_toggle_scans_tagged_live_marked_unlimboed_buildings_and_returns_a_receipt() {
    //6DDF65..6DE045: no health, incoming-House or value gate; receipt AL1
    //once GoOffline is called, even when a producer's callee refuses.
    let (mut sim, rules, registry, houses) = fixture();
    let local = spawn(&mut sim, &rules, "LOAD", "House0", 15);
    let foreign = spawn(&mut sim, &rules, "LOAD", "House1", 17);
    let producer = spawn(&mut sim, &rules, "PRODUCER", "House0", 19);
    let unmarked = spawn(&mut sim, &rules, "LOAD", "House0", 21);
    let dead = spawn(&mut sim, &rules, "LOAD", "House0", 23);
    let limbo = spawn(&mut sim, &rules, "LOAD", "House0", 25);
    let untagged = spawn(&mut sim, &rules, "LOAD", "House0", 27);
    let unit = spawn(&mut sim, &rules, "TAGUNIT", "House0", 16);
    for id in [local, foreign, producer, unmarked, dead, limbo, unit] {
        sim.attach_object_trigger_tag(id, Some("CONTROLTAG"));
    }
    sim.substrate
        .entities
        .get_mut(local)
        .unwrap()
        .health
        .set_for_test(0);
    sim.substrate
        .entities
        .get_mut(unmarked)
        .unwrap()
        .lifecycle
        .set_cell_marked(false);
    sim.substrate
        .entities
        .get_mut(dead)
        .unwrap()
        .lifecycle
        .set_object_alive_for_test(false);
    sim.substrate
        .entities
        .get_mut(limbo)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    let rng = sim.rng_state();
    sim.sound_events.clear();
    assert!(!run(&mut sim, &rules, &registry, 61, false));
    assert!(run(&mut sim, &rules, &registry, 61, true));
    for id in [local, foreign] {
        assert!(!sim.entities().get(id).unwrap().building_online());
    }
    for id in [producer, unmarked, dead, limbo, untagged, unit] {
        assert!(sim.entities().get(id).unwrap().building_online());
    }
    assert!(sim.houses[&houses[0]].tech_recheck_pending());
    assert!(sim.power_states[&houses[0]].power_recheck_for_test());
    assert!(sim.power_states[&houses[0]].radar_recheck_for_test());
    assert_eq!(
        sim.lighting_sources.buildings.get(&local).map(|s| s.active),
        Some(false)
    );
    assert_eq!(
        std::mem::take(&mut sim.sound_events)
            .into_iter()
            .filter_map(|event| {
                if let SimSoundEvent::HouseEva { event, owner } = event {
                    Some((event, owner))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        [("EVA_BuildingOffLine", houses[0])]
    );
    assert!(
        run(&mut sim, &rules, &registry, 61, true),
        "producer remains an admitted no-op"
    );
    assert!(run(&mut sim, &rules, &registry, 62, true));
    assert!(
        !run(&mut sim, &rules, &registry, 62, true),
        "no offline tagged candidate remains"
    );
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn counted_tag_runs_off_and_on_in_order_without_a_second_state_or_rng_draws() {
    let (mut sim, rules, registry, houses) = fixture();
    let load = spawn(&mut sim, &rules, "LOAD", "House0", 15);
    sim.attach_object_trigger_tag(load, Some("CONTROLTAG"));
    let rng = sim.rng_state();
    sim.sound_events.clear();
    sim.session.binary_frame = 8;
    sim.raise_house_tags(
        houses[0],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(sim.entities().get(load).unwrap().building_online());
    assert!(
        sim.entities()
            .get(load)
            .unwrap()
            .building_art
            .stuff_enabled()
    );
    assert_eq!(
        std::mem::take(&mut sim.sound_events)
            .into_iter()
            .filter_map(|event| {
                if let SimSoundEvent::HouseEva { event, .. } = event {
                    Some(event)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        ["EVA_BuildingOffLine", "EVA_BuildingOnLine"]
    );
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn authored_power_flag_and_needs_engineer_use_distinct_latches_and_survive_save_load() {
    let (mut sim, rules, registry, _) = fixture();
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[Structures]\n\
         0=House0,LOAD,256,15,15,0,CONTROLTAG,0,0,0\n\
         1=House0,PRODUCER,256,17,15,0,None,0,0,0\n\
         2=House0,TECH,256,19,15,0,None,0,0,1\n\
         3=House0,POWERED,256,21,15,0,None,0,0,-7\n",
    );
    let placed = crate::map::entities::parse_map_entities(&ini);
    assert_eq!(
        placed
            .iter()
            .map(|e| e.structure_online)
            .collect::<Vec<_>>(),
        [false, false, true, true]
    );
    assert_eq!(sim.spawn_from_map(&placed, Some(&rules)), 4);
    let id = |name: &str| {
        sim.entities()
            .values()
            .find(|e| sim.interner.resolve(e.type_ref()) == name)
            .unwrap()
            .stable_id()
    };
    let (load, producer, tech, powered) = (id("LOAD"), id("PRODUCER"), id("TECH"), id("POWERED"));
    assert!(!sim.entities().get(load).unwrap().building_online());
    assert!(
        sim.entities().get(producer).unwrap().building_online(),
        "GoOffline rejects a pure producer"
    );
    assert!(sim.entities().get(tech).unwrap().building_online());
    assert!(
        !sim.entities()
            .get(tech)
            .unwrap()
            .building_art
            .stuff_enabled(),
        "NeedsEngineer does not clear660"
    );
    assert!(sim.go_building_offline(tech, &rules, FrameEffects::default()));
    assert!(sim.go_building_online(tech, &rules, FrameEffects::default()));
    assert!(
        sim.entities()
            .get(tech)
            .unwrap()
            .building_art
            .stuff_enabled(),
        "GoOnline does not reapply the map's HasEngineer gate"
    );
    assert_eq!(sim.building_operational_state(tech, &rules), Some(false));
    assert!(
        sim.go_building_offline(powered, &rules, FrameEffects::default()),
        "Powered accepts zero drain"
    );
    let hash = sim.state_hash();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Building power", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.resolve_type_handles(&rules);
    restored.rebuild_lighting_sources_after_load(&rules);
    assert!(!restored.entities().get(load).unwrap().building_online());
    assert_eq!(
        restored
            .lighting_sources
            .buildings
            .get(&load)
            .map(|source| source.active),
        Some(false)
    );
    assert!(!restored.entities().get(powered).unwrap().building_online());
    assert!(run(&mut restored, &rules, &registry, 62, true));
    assert!(restored.entities().get(load).unwrap().building_online());
    assert!(sim.go_building_online(load, &rules, FrameEffects::default()));
    assert_ne!(sim.state_hash(), hash);
}

#[test]
fn retail_toggle_types_do_not_require_dormant_laser_fence_or_cloak_generators() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    assert!(rules.object("NAHAND").unwrap().power < 0);
    assert!(!rules.object("NAPOWR").unwrap().powered);
    assert!(
        rules
            .all_objects()
            .filter(|o| o.category == crate::rules::object_type::ObjectCategory::Building)
            .all(|o| !o.laser_fence_post && !o.cloak_generator)
    );
}
