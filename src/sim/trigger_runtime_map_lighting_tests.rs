//! Transylv's authored actions through the shared lighting and impact owners.
use super::*;
use crate::map::map_file::MapFile;
use crate::rules::process_owner::NativeScenarioRulesPrefix;
use crate::rules::ruleset::RuleSet;
use crate::sim::scenario_session::{ScenarioLightingProfile, ScenarioLightingState};
use crate::sim::superweapon::lightning_storm::LightningStorm;
use crate::sim::timer::CdTimer;
use serde_json::{Value, json};

fn native() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_map_lighting.json")
}

fn run(sim: &mut Simulation, rules: &RuleSet, entry: crate::map::actions::ActionEntry) -> bool {
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        None,
        FrameEffects::default(),
    )
}

fn action(kind: i32, value: i32) -> crate::map::actions::ActionEntry {
    crate::map::actions::ActionEntry {
        kind,
        params: vec![
            "0".into(),
            value.to_string(),
            "0".into(),
            "0".into(),
            "0".into(),
            "0".into(),
            "A".into(),
        ],
        waypoint_index: Some(0),
    }
}

#[test]
fn ambient_action_matches_native_controller_writes_without_resetting_timer_or_tint() {
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::empty()).unwrap();
    for row in native()["ambient_controls"].as_array().unwrap() {
        let input = &row["input"];
        let mut sim = Simulation::with_seed(0);
        let storm = input["storm"].as_bool().unwrap();
        if storm {
            let owner = sim.interner.intern("LightingControlOwner");
            sim.lightning_storm = LightningStorm::raging_for_test(owner, (0, 0));
        }
        let light = &mut sim.session.lighting;
        light.normal.ambient_percent = 87;
        light.normal.red_percent = 90;
        light.normal.green_percent = 90;
        light.normal.blue_percent = 100;
        if input["nuke"] != 0 || input["chrono"] != 0 {
            light.select(ScenarioLightingProfile::Nuke);
        } else if input["dominator"] != 0 {
            light.select(ScenarioLightingProfile::Dominator);
        }
        light.current_ambient = 87;
        light.target_ambient = 43;
        light.transition_timer = CdTimer::started(12, 8);
        let prior = *light;
        let rng_before = [
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        ];
        assert_eq!(
            run(
                &mut sim,
                &rules,
                action(73, input["value"].as_i64().unwrap() as i32)
            ),
            row["returned"] == 1
        );
        let light = sim.session.lighting;
        assert_eq!(
            json!({"normal":light.normal.ambient_percent,"current":light.current_ambient,
            "target":light.target_ambient,"normal_rgb_ground_level":[light.normal.red_percent,light.normal.green_percent,light.normal.blue_percent,light.normal.ground_units,light.normal.level_units],
            "timer":[light.transition_timer.start_frame(),light.transition_timer.duration()]}),
            row["after"]
        );
        assert_eq!(light.selected_profile, prior.selected_profile);
        assert_eq!(light.alternate_rgb(), prior.alternate_rgb());
        assert!(
            sim.lighting_sources.pending.is_empty(),
            "73 has no immediate relight"
        );
        assert_eq!(
            rng_before,
            [
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ]
        );
    }
}

#[test]
fn retail_transylv_ambient_action_reaches_native_fade_and_large_flash_request() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw = assets.get_ref("Transylv.yro").unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(raw),
        native()["retail"]["map"]["sha256"].as_str().unwrap()
    );
    let map = MapFile::from_bytes(raw).unwrap();
    let root_source = crate::rules::retail_sources::select_ini(&assets, "RULESMD.INI").unwrap();
    assert_eq!(
        root_source.source.source_sha256,
        native()["flash_controls"]["warhead_inputs"]["rules_sha256"]
            .as_str()
            .unwrap()
    );
    let mode = crate::rules::retail_sources::select_ini(&assets, "MPBattleMD.INI").unwrap();
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(Some(&mode.ini)),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.lighting =
        ScenarioLightingState::from_map(&crate::map::lighting::parse_lighting_profiles(&map.ini));
    let history = &native()["ambient_history"];
    assert_eq!(
        sim.session.lighting.current_ambient,
        history["initial"]["current"].as_i64().unwrap() as i32
    );
    let ambient = map.actions["07718EBC"]
        .entries
        .iter()
        .find(|a| a.kind == 73)
        .unwrap()
        .clone();
    assert_eq!(ambient.literal_value(), Some(110));
    assert!(run(&mut sim, &rules, ambient));
    for row in history["rows"].as_array().unwrap() {
        sim.session.binary_frame = row["frame"].as_u64().unwrap() as u32;
        sim.discard_lighting_events();
        sim.tick_scenario_lighting_transition(&rules);
        let light = sim.session.lighting;
        assert_eq!(
            light.current_ambient,
            row["current"].as_i64().unwrap() as i32
        );
        assert_eq!(light.target_ambient, row["target"].as_i64().unwrap() as i32);
        assert_eq!(
            json!([
                light.transition_timer.start_frame(),
                light.transition_timer.duration()
            ]),
            row["timer"]
        );
        let emitted = !row["events"].as_array().unwrap().is_empty();
        assert_eq!(sim.lighting_sources.pending.len(), usize::from(emitted));
    }
    // Flat empty declared crop, not Transylv terrain/scene pixel parity.
    sim.session.map_width = map.header.width.try_into().unwrap();
    sim.session.map_height = map.header.height.try_into().unwrap();
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
        map.header.width.try_into().unwrap(),
        map.header.height.try_into().unwrap(),
        crate::map::resolved_terrain::test_flat_cell,
    ));
    crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, "trigger-map-lighting");
    sim.session.install_waypoints(&map.waypoints);
    let flash = map.actions["09DE99FC"]
        .entries
        .iter()
        .find(|a| a.kind == 66)
        .unwrap()
        .clone();
    assert_eq!(flash.literal_value(), Some(130));
    assert!(run(&mut sim, &rules, flash));
    let row = native()["flash_controls"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["definition"]["kind"] == 66 && row["input"]["name"] == "transylv_EA_flat_crop"
        })
        .unwrap();
    let crate::sim::combat::CombatLightRequest::Impact {
        damage,
        warhead_ref,
        coord,
        force_create,
        flags,
        target_id,
    } = sim.combat_light_requests.last().unwrap()
    else {
        panic!("expected impact request");
    };
    let call = &row["calls"][0];
    assert_eq!(*damage, call["damage"].as_i64().unwrap() as i32);
    assert_eq!(
        sim.interner.resolve(*warhead_ref),
        call["warhead"].as_str().unwrap()
    );
    assert_eq!(
        json!([
            coord.x,
            coord.y,
            coord.z,
            i32::from(*force_create),
            *flags as i32
        ]),
        call["arguments"]
    );
    assert!(target_id.is_none());

    // Original +90 versus tail+44 discrimination: keep tailEA130 but bind
    // scalar131 to the native crop's coordinate through the waypoint owner.
    let mut waypoints = map.waypoints.clone();
    let mut scalar_cell = waypoints[&130];
    scalar_cell.index = 131;
    waypoints.insert(131, scalar_cell);
    // Make the tail different, so using it cannot satisfy the native result.
    waypoints.get_mut(&130).unwrap().rx = 12;
    waypoints.get_mut(&130).unwrap().ry = 11;
    sim.session.install_waypoints(&waypoints);
    let mut entry = action(66, 131);
    entry.waypoint_index = Some(130);
    assert!(run(&mut sim, &rules, entry));
    let crate::sim::combat::CombatLightRequest::Impact { coord, .. } =
        sim.combat_light_requests.last().unwrap()
    else {
        panic!("expected scalar waypoint impact");
    };
    let row = native()["flash_controls"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["definition"]["kind"] == 66 && row["input"]["name"] == "scalar_not_tail")
        .unwrap();
    assert_eq!(
        json!([coord.x, coord.y, coord.z]),
        json!([
            row["calls"][0]["arguments"][0],
            row["calls"][0]["arguments"][1],
            row["calls"][0]["arguments"][2],
        ])
    );
}
