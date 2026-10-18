//! Steam3e81a617: Execute6DF588, TeamType6F1F70, Talk70F120.
use super::*;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};

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
        .entries
        .remove(0);
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
    (result, effects)
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
        bincode::deserialize(&bincode::serialize(&state).unwrap()).unwrap();
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
