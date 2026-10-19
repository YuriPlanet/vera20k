//! Logic-polled ambient threshold events, native71EAE7 /71EB06.
//! Original gamemd.exe SHA256:
//! 3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600.
//! Instruction-established signed <= / >=; these are production regressions,
//! not a capture or exhaustive native execution comparison.
use super::*;
use crate::map::map_file::MapFile;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{snapshot::GameSnapshot, world::TickLane};

fn fixture(threshold: i32, target_action: bool) -> Simulation {
    let repeat = if target_action { 0 } else { 2 };
    // Boundary controls keep both independent predicates registered. The
    // saved-frame scenario selects just Below's one-shot continuation.
    let above_tag = if target_action {
        ""
    } else {
        "ABOVETAG=2,Above,ABOVE\n"
    };
    let (trigger, event, action, tag) = if target_action {
        (
            "TARGET=Neutral,<none>,Target,0,1,1,1,0\n",
            "TARGET=1,13,0,0\n",
            "TARGET=1,73,0,80,0,0,0,0,A\n",
            "TARGETTAG=0,Target,TARGET\n",
        )
    } else {
        ("", "", "", "")
    };
    let map = MapFile::from_bytes(
        format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Basic]\nNewINIFormat=4\n\
         [Triggers]\nBELOW=Neutral,<none>,Below,0,1,1,1,0\nABOVE=Neutral,<none>,Above,0,1,1,1,0\n{trigger}\
         [Events]\nBELOW=1,45,0,{threshold}\nABOVE=1,46,0,{threshold}\n{event}\
         [Actions]\nBELOW=1,56,0,0,0,0,0,0,A\nABOVE=1,56,0,1,0,0,0,0,A\n{action}\
         [Tags]\nBELOWTAG={repeat},Below,BELOW\n{above_tag}{tag}\
         [VariableNames]\n0=Below,0\n1=Above,0\n"
        )
        .as_bytes(),
    )
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.classify_map_triggers(None);
    sim.session.set_started(true);
    sim.session.set_map_width_for_test(32);
    sim.session.set_map_height_for_test(32);
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
        32,
        32,
        crate::map::resolved_terrain::test_flat_cell,
    ));
    sim
}

#[test]
fn ambient_events_poll_current_signed_scalar_and_include_equality() {
    for (current, threshold, below, above) in [
        (94, 95, true, false),
        (95, 95, true, true),
        (96, 95, false, true),
        (-1, 0, true, false),
        (0, 0, true, true),
        (1, 0, false, true),
        (i32::MIN, i32::MAX, true, false),
        (i32::MAX, i32::MIN, false, true),
        (i32::MIN, i32::MIN, true, true),
    ] {
        let mut sim = fixture(threshold, false);
        // Different normal and target values expose accidental profile reads.
        sim.session
            .lighting_mut()
            .normal_mut_for_test()
            .ambient_percent = 777;
        sim.session.lighting_mut().set_target_ambient_for_test(-888);
        sim.session
            .lighting_mut()
            .set_current_ambient_for_test(current);
        let prior = *sim.session.lighting();
        let rng = (
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        sim.advance_triggers(None, None, FrameEffects::default());
        assert_eq!(
            sim.trigger_local_is_set_for_test(0),
            below,
            "45: {current}, {threshold}"
        );
        assert_eq!(
            sim.trigger_local_is_set_for_test(1),
            above,
            "46: {current}, {threshold}"
        );
        assert_eq!(*sim.session.lighting(), prior);
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
fn saved_pending_ambient_event_fires_after_the_production_frame_fade() {
    let mut original = fixture(90, true);
    let rules = RuleSet::from_ini(&IniFile::empty()).unwrap();
    let first = original
        .advance_app_frame(
            &[],
            Some(&rules),
            None,
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(first.tick.frame_committed);
    // Logic polls before this frame's fade. The target action changes 100
    // to 80, then the fade reaches 90; Below waits for the next Logic poll.
    assert!(!original.trigger_local_is_set_for_test(0));
    assert_eq!(original.session.lighting().current_ambient(), 90);
    assert_eq!(original.session.lighting().target_ambient(), 80);
    let saved_hash = original.state_hash();
    let bytes = GameSnapshot::save(&original, 0, 0, "Ambient events", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), saved_hash);
    restored.resolved_terrain = original.resolved_terrain.clone();
    // A8ED5C is process admission state, not saved. The committed app load
    // republishes it before resuming frames (app/persistence/mod.rs).
    restored.session.set_started(true);
    for sim in [&mut original, &mut restored] {
        let frame = sim
            .advance_app_frame(
                &[],
                Some(&rules),
                None,
                67,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        assert!(frame.tick.frame_committed);
        assert_eq!(sim.session.lighting().current_ambient(), 90);
        assert!(
            sim.trigger_local_is_set_for_test(0),
            "Logic sees the preceding frame's fade value"
        );
        assert!(
            sim.trigger_runtime.logic_tags.is_empty(),
            "one-shot tags retire through the shared lifecycle"
        );
    }
    assert_eq!(restored.state_hash(), original.state_hash());
}
