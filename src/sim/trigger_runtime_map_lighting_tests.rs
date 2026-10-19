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
        let light = sim.session.lighting_mut();
        light.normal_mut_for_test().ambient_percent = 87;
        light.normal_mut_for_test().red_percent = 90;
        light.normal_mut_for_test().green_percent = 90;
        light.normal_mut_for_test().blue_percent = 100;
        if input["nuke"] != 0 || input["chrono"] != 0 {
            light.select(ScenarioLightingProfile::Nuke);
        } else if input["dominator"] != 0 {
            light.select(ScenarioLightingProfile::Dominator);
        }
        light.set_current_ambient_for_test(87);
        light.set_target_ambient_for_test(43);
        light.set_transition_timer_for_test(CdTimer::started(12, 8));
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
        let light = sim.session.lighting();
        assert_eq!(
            json!({"normal":light.normal().ambient_percent,"current":light.current_ambient(),
            "target":light.target_ambient(),"normal_rgb_ground_level":[light.normal().red_percent,light.normal().green_percent,light.normal().blue_percent,light.normal().ground_units,light.normal().level_units],
            "timer":[light.transition_timer().start_frame(),light.transition_timer().duration()]}),
            row["after"]
        );
        assert_eq!(light.selected_profile(), prior.selected_profile());
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
    sim.session
        .set_lighting_for_test(ScenarioLightingState::from_map(
            &crate::map::lighting::parse_lighting_profiles(&map.ini),
        ));
    let history = &native()["ambient_history"];
    assert_eq!(
        sim.session.lighting().current_ambient(),
        history["initial"]["current"].as_i64().unwrap() as i32
    );
    let ambient = map.actions["07718EBC"]
        .entries()
        .iter()
        .find(|a| a.kind == 73)
        .unwrap()
        .clone();
    assert_eq!(ambient.literal_value(), Some(110));
    assert!(run(&mut sim, &rules, ambient));
    for row in history["rows"].as_array().unwrap() {
        sim.session
            .set_binary_frame_for_test(row["frame"].as_u64().unwrap() as u32);
        sim.discard_lighting_events();
        sim.tick_scenario_lighting_transition(&rules);
        let light = sim.session.lighting();
        assert_eq!(
            light.current_ambient(),
            row["current"].as_i64().unwrap() as i32
        );
        assert_eq!(
            light.target_ambient(),
            row["target"].as_i64().unwrap() as i32
        );
        assert_eq!(
            json!([
                light.transition_timer().start_frame(),
                light.transition_timer().duration()
            ]),
            row["timer"]
        );
        let emitted = !row["events"].as_array().unwrap().is_empty();
        assert_eq!(sim.lighting_sources.pending.len(), usize::from(emitted));
    }
    // Flat empty declared crop, not Transylv terrain/scene pixel parity.
    sim.session
        .set_map_width_for_test(map.header.width.try_into().unwrap());
    sim.session
        .set_map_height_for_test(map.header.height.try_into().unwrap());
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
        map.header.width.try_into().unwrap(),
        map.header.height.try_into().unwrap(),
        crate::map::resolved_terrain::test_flat_cell,
    ));
    crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, "trigger-map-lighting");
    sim.session.install_waypoints(&map.waypoints);
    let flash = map.actions["09DE99FC"]
        .entries()
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

    // Execute64/65 reuse precisely this waypoint/C4/Flash owner. Replay the
    // existing native flat controls, including scalar131 versus tail130.
    for kind in [64, 65] {
        for name in ["transylv_EA_flat_crop", "scalar_not_tail"] {
            let scalar = if name == "scalar_not_tail" { 131 } else { 130 };
            let mut waypoints = map.waypoints.clone();
            let mut cell = waypoints[&130];
            cell.index = scalar;
            waypoints.insert(scalar, cell);
            if scalar != 130 {
                waypoints.get_mut(&130).unwrap().rx = 12;
                waypoints.get_mut(&130).unwrap().ry = 11;
            }
            sim.session.install_waypoints(&waypoints);
            let rng = (
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex(),
            );
            let mut entry = action(kind, scalar as i32);
            entry.waypoint_index = Some(130);
            assert!(run(&mut sim, &rules, entry));
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
            let row = native()["flash_controls"]["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["definition"]["kind"] == kind && row["input"]["name"] == name)
                .unwrap();
            assert_eq!(*damage, row["calls"][0]["damage"].as_i64().unwrap() as i32);
            assert_eq!(
                sim.interner.resolve(*warhead_ref),
                row["calls"][0]["warhead"].as_str().unwrap()
            );
            assert_eq!(
                json!([
                    coord.x,
                    coord.y,
                    coord.z,
                    i32::from(*force_create),
                    *flags as i32
                ]),
                row["calls"][0]["arguments"]
            );
            assert!(target_id.is_none());
            assert_eq!(
                (
                    sim.main_rng.native_state_hex(),
                    sim.scenario_rng.native_state_hex(),
                    sim.mapgen_rng.native_state_hex()
                ),
                rng
            );
        }
    }
    // The same physical RULESMD -> mode -> map transaction replaces retired
    // runtime values when its AudioVisual keys are present. Transylv itself
    // leaves both keys absent; the physical root supplies .2/.2 (native corpus).
    assert!(run(&mut sim, &rules, action(71, 1036831949)));
    assert!(run(&mut sim, &rules, action(72, 1065353216)));
    assert_eq!(
        sim.session.lighting().ambient_change_terms(&rules),
        (true, 900, 10)
    );
    owner
        .consume_outgoing_scenario_rules(
            sim.retire_object_type_tech_levels(),
            sim.session.lighting_mut().retire_ambient_change_inputs(),
        )
        .unwrap();
    let next = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(Some(&mode.ini)),
            &map.ini,
        )
        .unwrap()
        .into_parts()
        .0;
    assert_eq!(
        (
            next.general.ambient_change_rate_nonzero,
            next.general.ambient_change_interval_frames,
            next.general.ambient_change_step
        ),
        (true, 180, 20)
    );
}

#[test]
fn ambient_rate_and_step_match_original_action_bits_and_due_logic_rung() {
    // Original Steam gamemd.exe SHA256:
    // 3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600.
    // ActionFixture executes ctor6DD000/read6DD5B0/Execute6DD8B0, then
    // Logic55B33D..55B4D7 with current60, target100, timer12/8 at frame20.
    // Relight/redraw are output sinks. These are native outputs, not Rust
    // calculations. The PR records the ActionFixture reproduction command.
    // (kind, raw operand, parsed integer, stored binary64, current, timer, relight)
    let controls: &[(i32, &str, i32, u64, i32, [i32; 2], bool)] = &[
        (71, "0", 0, 0x0000000000000000, 60, [20, 180], true),
        (71, "1", 1, 0x36a0000000000000, 60, [20, 180], true),
        (71, "10", 10, 0x36d4000000000000, 60, [20, 180], true),
        (
            71,
            "1036831949",
            1036831949,
            0x3fb99999a0000000,
            70,
            [20, 180],
            true,
        ),
        (
            71,
            "1045220557",
            1045220557,
            0x3fc99999a0000000,
            80,
            [20, 180],
            true,
        ),
        (
            71,
            "1050253722",
            1050253722,
            0x3fd3333340000000,
            90,
            [20, 180],
            true,
        ),
        (
            71,
            "-1110651699",
            -1110651699,
            0xbfb99999a0000000,
            50,
            [20, 180],
            true,
        ),
        (
            71,
            "1065353216",
            1065353216,
            0x3ff0000000000000,
            100,
            [20, 180],
            true,
        ),
        (
            71,
            "-2147483648",
            -2147483648,
            0x8000000000000000,
            60,
            [20, 180],
            true,
        ),
        (
            71,
            "1325400065",
            1325400065,
            0x41e0000020000000,
            100,
            [20, 180],
            true,
        ),
        (
            71,
            "2139095040",
            2139095040,
            0x7ff0000000000000,
            60,
            [20, 180],
            true,
        ),
        (
            71,
            "2143289344",
            2143289344,
            0x7ff8000000000000,
            60,
            [20, 180],
            true,
        ),
        (71, ".2", 0, 0x0000000000000000, 60, [20, 180], true),
        (
            71,
            "1036831949suffix",
            1036831949,
            0x3fb99999a0000000,
            70,
            [20, 180],
            true,
        ),
        (72, "0", 0, 0x0000000000000000, 60, [12, 8], false),
        (72, "1", 1, 0x36a0000000000000, 70, [20, 0], true),
        (72, "10", 10, 0x36d4000000000000, 70, [20, 0], true),
        (
            72,
            "1036831949",
            1036831949,
            0x3fb99999a0000000,
            70,
            [20, 90],
            true,
        ),
        (
            72,
            "1045220557",
            1045220557,
            0x3fc99999a0000000,
            70,
            [20, 180],
            true,
        ),
        (
            72,
            "1050253722",
            1050253722,
            0x3fd3333340000000,
            70,
            [20, 270],
            true,
        ),
        (
            72,
            "-1110651699",
            -1110651699,
            0xbfb99999a0000000,
            70,
            [20, -90],
            true,
        ),
        (
            72,
            "1065353216",
            1065353216,
            0x3ff0000000000000,
            70,
            [20, 900],
            true,
        ),
        (
            72,
            "-2147483648",
            -2147483648,
            0x8000000000000000,
            60,
            [12, 8],
            false,
        ),
        (
            72,
            "1325400065",
            1325400065,
            0x41e0000020000000,
            70,
            [20, 230400],
            true,
        ),
        (
            72,
            "2139095040",
            2139095040,
            0x7ff0000000000000,
            70,
            [20, 0],
            true,
        ),
        (
            72,
            "2143289344",
            2143289344,
            0x7ff8000000000000,
            60,
            [12, 8],
            false,
        ),
        (72, ".2", 0, 0x0000000000000000, 60, [12, 8], false),
        (
            72,
            "1036831949suffix",
            1036831949,
            0x3fb99999a0000000,
            70,
            [20, 90],
            true,
        ),
    ];
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::empty()).unwrap();
    for &(kind, operand, value, stored, current, timer, relight) in controls {
        let mut sim = Simulation::with_seed(0);
        sim.session.lighting_mut().set_current_ambient_for_test(60);
        sim.session.lighting_mut().set_target_ambient_for_test(100);
        sim.session
            .lighting_mut()
            .set_transition_timer_for_test(CdTimer::started(12, 8));
        let rng = (
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        let parsed =
            crate::map::actions::parse_actions(&crate::rules::ini_parser::IniFile::from_str(
                &format!("[Actions]\nA=1,{kind},0,{operand},0,0,0,0,A\n"),
            ));
        let entry = parsed["A"].entries()[0].clone();
        assert_eq!(entry.literal_value(), Some(value));
        assert!(run(&mut sim, &rules, entry), "Action{kind}: {operand}");
        assert_eq!(sim.session.lighting().current_ambient(), 60);
        assert_eq!(
            *sim.session.lighting().transition_timer(),
            CdTimer::started(12, 8)
        );
        assert!(sim.lighting_sources.pending.is_empty());
        sim.session.set_binary_frame_for_test(20);
        sim.tick_scenario_lighting_transition(&rules);
        assert_eq!(
            sim.session.lighting().current_ambient(),
            current,
            "{kind}: {operand}"
        );
        assert_eq!(sim.session.lighting().target_ambient(), 100);
        assert_eq!(
            [
                sim.session.lighting().transition_timer().start_frame(),
                sim.session.lighting().transition_timer().duration()
            ],
            timer,
            "{kind}: {operand}"
        );
        assert_eq!(!sim.lighting_sources.pending.is_empty(), relight);
        let changed = sim.session.lighting_mut().retire_ambient_change_inputs();
        assert_eq!(changed[usize::from(kind == 71)].unwrap().bits(), stored);
        assert!(changed[usize::from(kind == 72)].is_none());
        assert_eq!(
            (
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            rng
        );
    }
}

#[test]
fn ambient_action_values_survive_save_and_transfer_to_retained_rules_at_retirement() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::process_owner::NativeRulesProcessOwner;
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::empty(),
        None,
        IniFile::empty(),
        std::sync::Arc::default(),
        Default::default(),
    )
    .unwrap();
    let rules = owner
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
        .unwrap()
        .into_parts()
        .0;
    let mut sim = Simulation::with_seed(0);
    let initial_hash = sim.state_hash();
    assert!(run(&mut sim, &rules, action(71, 1045220557)));
    assert!(run(&mut sim, &rules, action(72, 1036831949)));
    let changed_hash = sim.state_hash();
    assert_ne!(changed_hash, initial_hash);
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Ambient", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    assert_eq!(restored.state_hash(), changed_hash);
    assert_eq!(
        restored.session.lighting().ambient_change_terms(&rules),
        (true, 90, 20)
    );
    let mut reversed = Simulation::with_seed(0);
    assert!(run(&mut reversed, &rules, action(72, 1036831949)));
    assert!(run(&mut reversed, &rules, action(71, 1045220557)));
    assert_eq!(
        reversed.state_hash(),
        changed_hash,
        "hash current values, not action order"
    );
    for sim in [&mut sim, &mut restored] {
        sim.session.lighting_mut().set_current_ambient_for_test(60);
        sim.session.lighting_mut().set_target_ambient_for_test(100);
        sim.session.set_binary_frame_for_test(20);
        sim.tick_scenario_lighting_transition(&rules);
    }
    assert_eq!(restored.session.lighting(), sim.session.lighting());
    owner
        .consume_outgoing_scenario_rules(
            restored.retire_object_type_tech_levels(),
            restored
                .session
                .lighting_mut()
                .retire_ambient_change_inputs(),
        )
        .unwrap();
    assert_eq!(
        restored
            .session
            .lighting_mut()
            .retire_ambient_change_inputs(),
        [None; 2]
    );
    assert!(owner.saved_scenario_rules().is_none());
    let retained = owner
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
        .unwrap()
        .into_parts()
        .0;
    assert_eq!(
        (
            retained.general.ambient_change_interval_frames,
            retained.general.ambient_change_step
        ),
        (90, 20)
    );
    // A reached map AudioVisual pass replaces only its present key. General
    // and wrong-case keys are ignored by the existing production reader.
    let overridden = owner.load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::from_str(
        "[General]\nAmbientChangeRate=9\n[AudioVisual]\nAmbientChangeStep=.3\nambientchangerate=9\n",
    )).unwrap().into_parts().0;
    assert_eq!(
        (
            overridden.general.ambient_change_interval_frames,
            overridden.general.ambient_change_step
        ),
        (90, 30)
    );
}

#[test]
fn ordered_timer_trigger_updates_live_ambient_inputs_before_the_next_fade() {
    let (mut sim, rules, overlays, _) = super::tests::event_ingress_fixture();
    super::tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nAMBIENT=Country0,<none>,Ambient,0,1,1,1,0\n\
         [Events]\nAMBIENT=1,13,0,0\n\
         [Actions]\nAMBIENT=3,71,0,1045220557,0,0,0,0,A,72,0,1036831949,0,0,0,0,A,73,0,100,0,0,0,0,A\n\
         [Tags]\nAMBIENTTAG=0,Ambient,AMBIENT\n",
    );
    sim.session.lighting_mut().set_current_ambient_for_test(60);
    sim.session.lighting_mut().set_target_ambient_for_test(60);
    sim.session
        .lighting_mut()
        .set_transition_timer_for_test(CdTimer::started(12, 8));
    sim.session.set_binary_frame_for_test(20);
    sim.advance_triggers(Some(&rules), Some(&overlays), FrameEffects::default());
    assert_eq!(
        sim.trigger_runtime
            .action_trace
            .iter()
            .map(|entry| entry.0)
            .collect::<Vec<_>>(),
        [71, 72, 73]
    );
    assert_eq!(sim.session.lighting().current_ambient(), 60);
    assert_eq!(sim.session.lighting().target_ambient(), 100);
    assert_eq!(
        *sim.session.lighting().transition_timer(),
        CdTimer::started(12, 8)
    );
    sim.tick_scenario_lighting_transition(&rules);
    assert_eq!(sim.session.lighting().current_ambient(), 80);
    assert_eq!(
        *sim.session.lighting().transition_timer(),
        CdTimer::started(20, 90)
    );
}

#[test]
fn live_normal_rate_still_gates_nuke_fades_and_the_step_is_shared() {
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::empty()).unwrap();
    for (bits, enabled) in [
        (1036831949, true),
        (0, false),
        (-2147483648, false),
        (2143289344, false),
    ] {
        let mut sim = Simulation::with_seed(0);
        sim.session.lighting_mut().set_nuke_flash_for_test(1, 0, 10);
        sim.session.lighting_mut().set_current_ambient_for_test(60);
        sim.session.lighting_mut().set_target_ambient_for_test(100);
        assert!(run(&mut sim, &rules, action(71, 1045220557)));
        assert!(run(&mut sim, &rules, action(72, bits)));
        sim.session.set_binary_frame_for_test(20);
        sim.tick_scenario_lighting_transition(&rules);
        assert_eq!(
            sim.session.lighting().current_ambient(),
            if enabled { 80 } else { 60 }
        );
        if enabled {
            assert_eq!(
                sim.session.lighting().transition_timer().duration(),
                crate::map::lighting::NUKE_CHANGE_RATE_DEFAULT
            );
        }
    }
}
