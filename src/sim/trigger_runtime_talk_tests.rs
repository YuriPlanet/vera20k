//! Steam3e81a617: Execute6DF588, TeamType6F1F70, Talk70F120.
use super::*;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::util::save_codec;

#[test]
fn talk_time_matches_original_unsigned_default_and_ftol_controls() {
    // Complete original671E3B..671E83 with original5283D0/CRT/ftol;
    // inputs are raw cached INI entries, not VERA-derived scalar results.
    // Scratch command/script and selected binary hash are recorded in the PR.
    for (current, raw, expected) in [
        (300, None, 300),
        (301, None, 301),
        (-1, None, 222),
        (300, Some("2.5"), 150),
        (300, Some("-1"), -60),
        (300, Some("1.1"), 66),
        (300, Some("junk"), 0),
        (300, Some("50%"), 30),
        (300, Some("nan"), 0),
        (300, Some("1e20"), 0),
    ] {
        let text = raw.map_or("[General]\n".to_owned(), |s| {
            format!("[General]\nTalkBubbleTime={s}\n")
        });
        let ini = IniFile::from_str(&text);
        assert_eq!(
            ini.section_or_empty("General")
                .read_talk_bubble_time(current),
            expected,
            "{current}/{raw:?}"
        );
    }
}

#[test]
fn talk_time_uses_production_rules_layers_and_retail_defaults() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let mut layers = RulesLayerStack::new(IniFile::from_str("[General]\nTalkBubbleTime=2.5\n"));
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .general
            .talk_bubble_time,
        150
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\ntalkbubbletime=9\nTalkBubbleTime=\n"),
    );
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .general
            .talk_bubble_time,
        150
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nTalkBubbleTime=-1\n"),
    );
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .general
            .talk_bubble_time,
        -60
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    // Actual RULESMD and Battle contain no override; the native selected
    // reader produced300. This fixture also loads LANGRULE-if-present/Hills.
    assert_eq!(retail.rules.general.talk_bubble_time, 300);
    let Some(patched) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str("[General]\nTalkBubbleTime=2.5\n"),
    ) else {
        return;
    };
    assert_eq!(patched.rules.general.talk_bubble_time, 150);
}

fn dispatch(
    sim: &mut Simulation,
    rules: &RuleSet,
    reference: Option<DefinitionReference>,
    value: i32,
) -> (bool, Vec<TriggerEffect>) {
    let operand = match reference {
        Some(DefinitionReference::Team(i)) => i as i32,
        None => -1,
        _ => unreachable!(),
    };
    let ini = IniFile::from_str(&format!(
        "[Actions]\nSPEECH=1,105,5,{operand},0,0,0,0,{value}\n"
    ));
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("SPEECH")
        .unwrap()
        .entries()[0]
        .clone();
    let action = sim
        .trigger_runtime
        .definitions
        .resolve_action(entry, None, None);
    assert_eq!(action.reference, reference);
    assert_eq!(action.value, Some(value));
    let context = sim.trigger_action_context(0, None, TagEventInput::default(), Some(rules));
    let mut effects = Vec::new();
    let result = sim.apply_live_trigger_action(
        &action,
        context,
        &mut effects,
        Some(rules),
        None,
        FrameEffects::default(),
    );
    // The production frame output is the authoritative receipt queue.
    sim.publish_trigger_effects(effects);
    (result, sim.drain_trigger_effects())
}

#[test]
fn talk_dispatch_uses_first_team_head_and_current_player_reveal_without_rng() {
    let (mut sim, rules, _, houses) = tests::event_ingress_fixture_with_rules("");
    sim.fog = crate::sim::vision::FogState::new(33, 33);
    sim.session.current_house = Some(houses[0]);
    let first = tests::spawn_event_ingress_unit(&mut sim, &rules, houses[1], (15, 15));
    let second = tests::spawn_event_ingress_unit(&mut sim, &rules, houses[1], (16, 15));
    let third = tests::spawn_event_ingress_unit(&mut sim, &rules, houses[2], (17, 15));
    let team =
        crate::sim::team_script_vm::join_team_for_test(&mut sim, &[first, second], false, false);
    crate::sim::team_script_vm::join_team_for_test(&mut sim, &[third], false, false);
    assert_eq!(
        sim.team_script_vm.team(team).unwrap().members().next(),
        Some(first)
    );
    sim.substrate
        .entities
        .get_mut(first)
        .unwrap()
        .health
        .set_for_test(0);
    let mut defs = RuntimeDefinitions::default();
    defs.trigger_type("TALK").unwrap();
    let index = defs.team_type("TEST_TEAM").unwrap();
    let missing = defs.team_type("MISSING").unwrap();
    sim.interner.intern("MISSING");
    sim.trigger_runtime.definitions = defs;
    let streams = (
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    );
    let (ok, effects) = dispatch(&mut sim, &rules, Some(DefinitionReference::Team(index)), 2);
    assert!(ok);
    assert_eq!(
        effects,
        vec![TriggerEffect::TalkBubble {
            actor: Some(first),
            value: 2,
            duration: 300
        }]
    );
    assert!(sim.fog.is_cell_revealed(houses[0], 15, 15));
    // Missing live Team is false and leaves the app's previous cue untouched.
    assert_eq!(
        dispatch(
            &mut sim,
            &rules,
            Some(DefinitionReference::Team(missing)),
            9
        ),
        (false, vec![])
    );
    let (_, negative) = dispatch(&mut sim, &rules, Some(DefinitionReference::Team(index)), -2);
    assert_eq!(
        negative,
        vec![TriggerEffect::TalkBubble {
            actor: Some(first),
            value: -2,
            duration: 300
        }]
    );
    assert_eq!(
        dispatch(&mut sim, &rules, None, 9),
        (
            true,
            vec![TriggerEffect::TalkBubble {
                actor: None,
                value: 0,
                duration: 0
            }]
        )
    );
    assert_eq!(
        dispatch(&mut sim, &rules, Some(DefinitionReference::Team(index)), 0),
        (
            true,
            vec![TriggerEffect::TalkBubble {
                actor: None,
                value: 0,
                duration: 0
            }]
        )
    );
    assert_eq!(
        (
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex()
        ),
        streams
    );
}

#[test]
fn retail_speech_shape_has_no_invented_substitute() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    assert!(
        assets.get("TALKBUBL.SHP").is_none(),
        "physical retail stack unexpectedly gained speech art; inspect and expand rendering coverage"
    );
}

#[test]
fn talk_time_survives_registry_snapshot_handoff_and_type_reset() {
    use crate::rules::native_processing::{NativeRulesRegistryState, RulesLayerStack};
    let first = RulesLayerStack::new(IniFile::from_str("[General]\nTalkBubbleTime=2.5\n"))
        .process()
        .unwrap();
    let (_, trace) = first.into_ini_and_native_type_construction_trace();
    let state = trace.into_registry_state_discarding_events();
    let loaded: NativeRulesRegistryState =
        save_codec::deserialize(&save_codec::serialize(&state).unwrap()).unwrap();
    let next = RulesLayerStack::new(IniFile::from_str("[General]\nTalkBubbleTime=\n"))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), loaded.destructive_reset())
        .unwrap();
    assert_eq!(
        RuleSet::from_processed_rules(&next)
            .unwrap()
            .general
            .talk_bubble_time,
        150
    );
}

#[test]
fn talk_reveal_passes_native_s28_height_gate_to_the_shared_visibility_owner() {
    // Original70F15B S28=1,56772E..5677AF. Existing reveal_area_row
    //executed full5673A0 for center15,15/size16,16/radius2/z0/S28=1/S32=1
    //and obstruction17,17=4: target16,15 is omitted only with Rules17EE=1.
    //This pins the native mirror/+2 sample, not a fitted height fixture.
    for (by_height, expected) in [(true, false), (false, true)] {
        let (mut sim, rules, _, houses) = tests::event_ingress_fixture_with_rules(&format!(
            "[General]\nRevealByHeight={}\n",
            if by_height { "yes" } else { "no" }
        ));
        sim.fog = crate::sim::vision::FogState::new(33, 33);
        sim.session.current_house = Some(houses[0]);
        let actor = tests::spawn_event_ingress_unit(&mut sim, &rules, houses[1], (15, 15));
        assert_eq!(
            crate::sim::movement::ground_pose::object_get_coords(
                sim.entities().get(actor).unwrap(),
                sim.resolved_terrain.as_ref()
            )
            .z,
            0
        );
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let raised = terrain.native_cell_identity((17, 17));
        terrain.write_native_cell_level(raised, 4);
        // SightHeights reads the navigation owner's derived level plane.
        // Publish the declared terrain input through that existing owner.
        assert!(sim.rebuild_dynamic_navigation(&rules));
        crate::sim::team_script_vm::join_team_for_test(&mut sim, &[actor], false, false);
        let mut defs = RuntimeDefinitions::default();
        defs.trigger_type("SPEECH").unwrap();
        let ty = defs.team_type("TEST_TEAM").unwrap();
        sim.trigger_runtime.definitions = defs;
        assert!(!sim.fog.is_cell_revealed(houses[0], 16, 15));
        assert!(dispatch(&mut sim, &rules, Some(DefinitionReference::Team(ty)), 1).0);
        assert_eq!(
            sim.fog.is_cell_revealed(houses[0], 16, 15),
            expected,
            "RevealByHeight={by_height}"
        );
    }
}

#[test]
fn nested_discovery_speech_overrides_outer_request_in_native_write_order() {
    let (mut sim, rules, _, houses) = tests::event_ingress_fixture_with_rules("");
    sim.fog = crate::sim::vision::FogState::new(33, 33);
    sim.session.current_house = Some(houses[0]);
    let actor = tests::spawn_event_ingress_unit(&mut sim, &rules, houses[1], (15, 15));
    crate::sim::team_script_vm::join_team_for_test(&mut sim, &[actor], false, false);
    tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nINNER=Country7,<none>,Discovered speech,0,1,1,1,0\n\
         [Tags]\nTAG=0,Discovered speech,INNER\n\
         [Events]\nINNER=1,4,0,0\n\
         [Actions]\nINNER=1,105,5,TEST_TEAM,0,0,0,0,2\n",
    );
    sim.attach_object_trigger_tag(actor, Some("TAG"));
    let ty = sim
        .trigger_runtime
        .definitions
        .team_types
        .iter()
        .position(|id| id == "TEST_TEAM")
        .unwrap();
    let (ok, effects) = dispatch(&mut sim, &rules, Some(DefinitionReference::Team(ty)), 1);
    assert!(ok);
    let values = effects
        .iter()
        .filter_map(|effect| match effect {
            TriggerEffect::TalkBubble { value, .. } => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    //70F134/13A precedes70F189; Map->6F4A03..4A18 raises Event4
    //synchronously. The inner assignment survives after the outer returns.
    assert_eq!(values, [1, 2]);
}
