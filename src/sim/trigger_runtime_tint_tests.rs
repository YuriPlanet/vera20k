//! Original1426E2E60/1436E2EB0/1446E2F00 controls, Steam gamemd.exe
//! SHA2563e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600.
//! Reproduce: PYTHONPATH=. python ~/scratch/trigger-tint-controls/native_tint_controls.py
//! Recalc53AD00 is an observed graphics sink in these30 action controls.
use super::*;
use crate::map::map_file::MapFile;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::light_sources::{LightingEvent, PaletteUpdate};
use crate::sim::scenario_session::{ScenarioLightingProfile, ScenarioLightingState};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::timer::CdTimer;
use crate::sim::world::TickLane;

fn action(kind: i32, operand: &str) -> RuntimeAction {
    let ini = IniFile::from_str(&format!("[Actions]\nCASE=1,{kind},0,{operand},0,0,0,0,A\n"));
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("CASE")
        .unwrap()
        .entries()[0]
        .clone();
    RuntimeDefinitions::default().resolve_action(entry, None, None)
}

fn run(sim: &mut Simulation, action: &RuntimeAction) -> bool {
    sim.apply_live_trigger_action(
        action,
        TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        None,
        None,
        FrameEffects::default(),
    )
}

fn normal_rgb(state: &ScenarioLightingState) -> [i32; 3] {
    [
        state.normal().red_percent,
        state.normal().green_percent,
        state.normal().blue_percent,
    ]
}

#[test]
fn tint_actions_match_all_thirty_original_reader_and_wrapping_call_controls() {
    // Literal outputs of the original reader/LEA/SHL controls. Do not replace
    // the golden third column with a Rust multiply or saturating conversion.
    let controls = [
        ("0", 0, 0),
        ("1", 1, 10),
        ("-1", -1, -10),
        ("100", 100, 1000),
        ("200", 200, 2000),
        ("214748365", 214748365, -2147483646),
        ("-2147483648", i32::MIN, 0),
        ("2147483647", i32::MAX, -10),
        (".2", 0, 0),
        ("89suffix", 89, 890),
    ];
    for kind in 142..=144 {
        for (operand, value, milli) in controls {
            let mut sim = Simulation::with_seed(0);
            let light = sim.session.lighting_mut();
            light.normal_mut_for_test().ambient_percent = 60;
            light.set_current_ambient_for_test(70);
            light.set_target_ambient_for_test(80);
            light.normal_mut_for_test().red_percent = 123;
            light.normal_mut_for_test().green_percent = 87;
            light.normal_mut_for_test().blue_percent = 65;
            light.normal_mut_for_test().ground_units = 10;
            light.normal_mut_for_test().level_units = 20;
            light.set_transition_timer_for_test(CdTimer::from_raw(12, 8));
            // The native leaf ignores the selected alternate profile.
            light.set_selected_profile_for_test(ScenarioLightingProfile::Ion);
            let previous = *light;
            let rng = [
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex(),
            ];
            let action = action(kind, operand);
            assert_eq!(action.value, Some(value));
            assert!(run(&mut sim, &action));
            let mut expected = [123, 87, 65];
            expected[(kind - 142) as usize] = value;
            assert_eq!(
                normal_rgb(&sim.session.lighting()),
                expected,
                "{kind}: {operand}"
            );
            let mut expected_call = [1230, 870, 650];
            expected_call[(kind - 142) as usize] = milli;
            assert!(
                matches!(sim.lighting_sources.pending.as_slice(), [LightingEvent::Global { palette: PaletteUpdate::Normal(rgb), .. }] if *rgb == expected_call)
            );
            let mut unchanged = *sim.session.lighting();
            unchanged.normal_mut_for_test().red_percent = previous.normal().red_percent;
            unchanged.normal_mut_for_test().green_percent = previous.normal().green_percent;
            unchanged.normal_mut_for_test().blue_percent = previous.normal().blue_percent;
            assert_eq!(unchanged, previous);
            assert_eq!(
                [
                    sim.main_rng.native_state_hex(),
                    sim.scenario_rng.native_state_hex(),
                    sim.mapgen_rng.native_state_hex()
                ],
                rng
            );
        }
    }
}

#[test]
fn ordered_tag_rgb_actions_then_the_production_fade_keep_distinct_palette_operations() {
    let map = MapFile::from_bytes(b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n[Triggers]\nRGB=Neutral,<none>,RGB,0,1,1,1,0\n[Events]\nRGB=1,13,0,0\n[Actions]\nRGB=3,142,0,120,0,0,0,0,A,143,0,80,0,0,0,0,A,144,0,60,0,0,0,0,A\n[Tags]\nRGBTAG=0,RGB,RGB\n").unwrap();
    let rules = RuleSet::from_ini(&IniFile::empty()).unwrap();
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
    sim.session.lighting_mut().set_target_ambient_for_test(80);
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
    let palette: Vec<_> = frame
        .lighting_events
        .iter()
        .map(|event| match event {
            LightingEvent::Global { palette, .. } => *palette,
            _ => panic!("unexpected source operation"),
        })
        .collect();
    assert_eq!(
        palette,
        [
            PaletteUpdate::Normal([1200, 1000, 1000]),
            PaletteUpdate::Normal([1200, 800, 1000]),
            PaletteUpdate::Normal([1200, 800, 600]),
            PaletteUpdate::Preserve
        ]
    );
    assert_eq!(sim.session.lighting().current_ambient(), 90);
    assert_eq!(normal_rgb(&sim.session.lighting()), [120, 80, 60]);
    assert!(sim.trigger_runtime.logic_tags.is_empty());
}

#[test]
fn raw_tint_fields_roundtrip_without_serializing_palette_caches() {
    let mut sim = Simulation::with_seed(0);
    assert!(run(&mut sim, &action(142, "-2147483648")));
    assert!(run(&mut sim, &action(143, "2147483647")));
    assert!(run(&mut sim, &action(144, "89suffix")));
    let hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "Tint", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        normal_rgb(&restored.session.lighting()),
        [i32::MIN, i32::MAX, 89]
    );
    assert_eq!(restored.state_hash(), hash);
    assert!(restored.lighting_sources.pending.is_empty());
}

#[test]
fn palette_constructor_priority_matches_eight_original_suffix_controls() {
    // Original555DC3..55606C including full556090; supplied post-base-ctor
    // state, RGB512/640/768 and scene flags. native_palette_history.py.
    for (storm, nuke, chrono, dom, expected) in [
        (false, 0, false, 0, None),
        (true, 0, false, 0, Some([300, 400, 750])),
        (false, 1, false, 0, Some([1750, 1500, 1250])),
        (false, 0, true, 0, Some([1750, 1500, 1250])),
        (false, 0, false, 1, Some([850, 200, 300])),
        (false, 0, false, 5, Some([850, 200, 300])),
        (true, 1, true, 1, Some([300, 400, 750])),
        (false, 1, false, 1, Some([1750, 1500, 1250])),
    ] {
        let mut sim = Simulation::with_seed(0);
        if storm {
            sim.lightning_storm =
                crate::sim::superweapon::lightning_storm::LightningStorm::raging_for_test(
                    crate::sim::intern::InternedId::default(),
                    (0, 0),
                );
        }
        sim.session
            .lighting_mut()
            .set_nuke_flash_for_test(nuke, 0, 30);
        if chrono {
            sim.session.start_chrono_screen_state(10);
        }
        sim.psychic_dominator =
            crate::sim::superweapon::psychic_dominator::PsychicDominatorState::for_test(
                dom,
                (0, 0),
                None,
                None,
            );
        assert_eq!(
            sim.palette_constructor_rgb(),
            expected,
            "{storm}/{nuke}/{chrono}/{dom}"
        );
    }
}
