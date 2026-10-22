//! Fresh physical duplicate keys; expectations reuse the retained original
//! 7275D0/727240, ResetTimers726400 and Logic55AFB0 controls.
use super::tests::install_event_ingress_map;
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::snapshot::GameSnapshot;

#[test]
fn duplicate_headers_read_the_crc_selected_value_on_every_physical_visit() {
    let mut sim = Simulation::with_seed(31);
    let map = install_event_ingress_map(
        &mut sim,
        RuleSet::empty_for_test(),
        "[Triggers]\n\
         ROOT=<none>,<none>,First,0,1,1,1,0\n\
         ROOT=<none>,<none>,Second,0,1,1,1,0\n\
         SHOW=<none>,<none>,Show,0,1,1,1,0\n\
         [Events]\nROOT=2,47,0,0,13,0,3\n\
         [Actions]\nROOT=1,56,0,7,0,0,0,0,A\n",
    );
    assert_eq!(
        map.ini
            .section_or_empty("Triggers")
            .read_string("ROOT", "", 512),
        "<none>,<none>,Second,0,1,1,1,0"
    );
    let root = sim
        .trigger_runtime
        .definitions
        .trigger_types
        .iter()
        .find(|typ| typ.id == "ROOT")
        .unwrap();
    assert_eq!(
        root.events
            .iter()
            .map(|event| (event.kind, event.value))
            .collect::<Vec<_>>(),
        [(13, 3), (47, 0), (13, 3), (47, 0)]
    );
    assert_eq!(
        root.actions
            .iter()
            .map(|action| (action.entry.kind, action.value))
            .collect::<Vec<_>>(),
        [(56, Some(7))],
        "each reread replaces the active action list"
    );
}

#[test]
fn duplicate_random_timer_message_keeps_its_schedule_through_core_restore() {
    let rules = RuleSet::empty_for_test();
    let mut original = Simulation::with_seed(31);
    original.session.set_game_mode(ScenarioGameMode::Skirmish);
    let before = original.rng_state();
    install_event_ingress_map(
        &mut original,
        rules,
        "[Triggers]\n\
         ROOT=<none>,<none>,Root,0,1,1,1,0\n\
         ROOT=<none>,<none>,Root,0,1,1,1,0\n\
         SHOW=<none>,<none>,Show,0,1,1,1,0\n\
         [Events]\nROOT=2,47,0,0,51,0,6\nSHOW=1,36,0,7\n\
         [Actions]\nROOT=1,56,0,7,0,0,0,0,A\n\
         SHOW=1,11,4,DESC:SOV1,0,0,0,0,A\n\
         [Tags]\nRTAG=0,Root,ROOT\nMTAG=0,Message,SHOW\n",
    );
    // Original Seed31 draws3,5: the second51 overwrites the shared timer
    // from90 to120. The fixture's production map reader sets NewINIFormat4.
    let after = original.rng_state();
    assert_eq!(after.scenario.index_a, before.scenario.index_a + 2);
    assert_eq!(after.scenario.index_b, before.scenario.index_b + 2);
    assert_eq!((after.main, after.mapgen), (before.main, before.mapgen));
    let root = original.trigger_runtime.trigger_order[0];
    let timer = original.trigger_runtime.trigger(root).unwrap().timer;
    assert_eq!((timer.start_frame(), timer.duration()), (0, 120));
    for frame in (0..=64).step_by(8) {
        original.session.set_binary_frame_for_test(frame);
        assert!(
            original
                .advance_triggers(rules, None, FrameEffects::default())
                .is_empty()
        );
    }
    assert!(!original.trigger_local_is_set_for_test(7));
    let bytes = GameSnapshot::save_validated(&original, 0, 0, "duplicate trigger", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored.trigger_runtime.definitions,
        original.trigger_runtime.definitions
    );
    assert_eq!(restored.trigger_runtime.trigger(root).unwrap().timer, timer);

    // The existing native poll schedule submits one message at128. This
    // exercises core restore, not the app's PreparedSavedGame/resource handoff.
    let mut messages = Vec::new();
    for frame in (72..=128).step_by(8) {
        original.session.set_binary_frame_for_test(frame);
        restored.session.set_binary_frame_for_test(frame);
        let effects = original.advance_triggers(rules, None, FrameEffects::default());
        assert_eq!(
            restored.advance_triggers(rules, None, FrameEffects::default()),
            effects
        );
        assert_eq!(
            restored.trigger_runtime.triggers,
            original.trigger_runtime.triggers
        );
        assert_eq!(original.trigger_local_is_set_for_test(7), frame >= 120);
        assert_eq!(restored.trigger_local_is_set_for_test(7), frame >= 120);
        messages.extend(effects.into_iter().map(|effect| (frame, effect)));
    }
    assert_eq!(
        messages,
        [(
            128,
            TriggerEffect::LocalizedMessage {
                csf_label: "DESC:SOV1".into()
            }
        )]
    );
}
