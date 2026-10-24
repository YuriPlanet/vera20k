//! Production trigger chains on the two retail skirmish maps named by issue1227.

use super::*;
use crate::assets::asset_manager::AssetManager;
use crate::map::source::{LoadedMapSource, load_map_by_name_or_path_with_assets};
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::retail_scenario_assets;
use crate::sim::runtime::SimRuntime;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{SimSoundEvent, TickLane};
use crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND;
use std::path::Path;

const MAP_SEED: u32 = 0x1227;

fn assert_pkt_lists_map(assets: &AssetManager, pkt_name: &str, map_name: &str) {
    let bytes = assets
        .get_ref(pkt_name)
        .unwrap_or_else(|| panic!("retail scenario VFS has no {pkt_name}"));
    let pkt = IniFile::from_bytes(bytes).unwrap();
    let multimaps = pkt.section("MultiMaps").unwrap();
    let stem = Path::new(map_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap();
    assert!(
        multimaps.keys().any(|key| {
            multimaps
                .read_name(key, 0x40)
                .is_some_and(|name| name.eq_ignore_ascii_case(stem))
        }),
        "{map_name} is absent from {pkt_name} [MultiMaps]"
    );
}

fn load_skirmish(
    root: &Path,
    map_name: &str,
    seed: u32,
    assets: &mut AssetManager,
) -> crate::headless_scenario::HeadlessScenario {
    let launch = crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(
        crate::headless_scenario::battle_session(map_name, &[]),
    )
    .unwrap();
    crate::headless_scenario::load_with_registered_scenario_assets(
        root, map_name, seed, launch, assets,
    )
    .unwrap()
}

fn trigger_key_for_tag_name(map: &crate::map::map_file::MapFile, label: &str) -> String {
    map.tags
        .iter()
        .find(|(_, tag)| tag.name().eq_ignore_ascii_case(label))
        .and_then(|(_, tag)| tag.trigger())
        .unwrap_or_else(|| panic!("retail map has no Tag for {label}"))
        .to_owned()
}

fn trigger_type_index(sim: &Simulation, id: &str) -> usize {
    sim.trigger_runtime
        .definitions
        .trigger_types
        .iter()
        .position(|trigger| trigger.id.eq_ignore_ascii_case(id))
        .unwrap_or_else(|| panic!("retail TriggerType {id} was not read"))
}

fn live_trigger_index(sim: &Simulation, id: &str) -> usize {
    let type_index = trigger_type_index(sim, id);
    sim.trigger_runtime
        .trigger_order
        .iter()
        .copied()
        .find(|&index| {
            sim.trigger_runtime
                .trigger(index)
                .is_some_and(|trigger| trigger.type_index == type_index)
        })
        .unwrap_or_else(|| panic!("retail TriggerType {id} has no live instance"))
}

fn trigger_enabled(sim: &Simulation, id: &str) -> bool {
    sim.trigger_runtime
        .trigger(live_trigger_index(sim, id))
        .unwrap()
        .enabled
}

fn trigger_timer(sim: &Simulation, id: &str) -> crate::sim::timer::CdTimer {
    sim.trigger_runtime
        .trigger(live_trigger_index(sim, id))
        .unwrap()
        .timer
}

fn event_value(sim: &Simulation, id: &str, event_kind: i32) -> i32 {
    let type_index = trigger_type_index(sim, id);
    sim.trigger_runtime.definitions.trigger_types[type_index]
        .events
        .iter()
        .find(|event| event.kind == event_kind)
        .unwrap_or_else(|| panic!("TriggerType {id} has no event {event_kind}"))
        .value
}

fn action_trigger_targets(sim: &Simulation, id: &str, action_kind: i32) -> Vec<String> {
    let type_index = trigger_type_index(sim, id);
    sim.trigger_runtime.definitions.trigger_types[type_index]
        .actions
        .iter()
        .filter(|action| action.entry.kind == action_kind)
        .filter_map(|action| match action.reference {
            Some(DefinitionReference::Trigger(target)) => Some(
                sim.trigger_runtime.definitions.trigger_types[target]
                    .id
                    .clone(),
            ),
            _ => None,
        })
        .collect()
}

fn action_waypoints(sim: &Simulation, id: &str, action_kind: i32) -> Vec<u32> {
    let type_index = trigger_type_index(sim, id);
    sim.trigger_runtime.definitions.trigger_types[type_index]
        .actions
        .iter()
        .filter(|action| action.entry.kind == action_kind)
        .filter_map(|action| action.entry.waypoint_index)
        .collect()
}

fn action_kinds(sim: &Simulation, id: &str) -> Vec<i32> {
    let type_index = trigger_type_index(sim, id);
    sim.trigger_runtime.definitions.trigger_types[type_index]
        .actions
        .iter()
        .map(|action| action.entry.kind)
        .collect()
}

fn event_due(timer: crate::sim::timer::CdTimer) -> i32 {
    timer.start_frame().wrapping_add(timer.duration())
}

fn advance_queued_frame(runtime: &mut SimRuntime) -> crate::sim::world::SimFrameOutput {
    runtime.simulation.clear_tag_dispatch_trace_for_test();
    let due = runtime.simulation.take_due_commands();
    runtime
        .advance_frame(
            &due,
            crate::headless_scenario::SIM_TICK_MS,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .expect("the production order and master frame complete")
}

/// Run ordinary master frames up to the named trigger's due frame.
fn advance_to_trigger_due(runtime: &mut SimRuntime, trigger: &str, due: i32) {
    while (runtime.simulation.session().binary_frame() as i32) < due {
        let before = runtime.simulation.session().binary_frame() as i32;
        assert!(trigger_enabled(&runtime.simulation, trigger));
        let _ = advance_queued_frame(runtime);
        let after = runtime.simulation.session().binary_frame() as i32;
        assert_eq!(after, before + 1, "one ordinary frame commits per call");
        assert!(trigger_enabled(&runtime.simulation, trigger));
    }
    assert_eq!(runtime.simulation.session().binary_frame() as i32, due);
    assert!(trigger_enabled(&runtime.simulation, trigger));
}

/// Poll once at the due frame through the ordinary trigger/master phases.
fn poll_expired_trigger(
    runtime: &mut SimRuntime,
    trigger: &str,
) -> crate::sim::world::SimFrameOutput {
    let output = advance_queued_frame(runtime);
    assert!(!trigger_enabled(&runtime.simulation, trigger));
    output
}

/// Advance through the named trigger's expiry, including ordinary LightningStorm updates.
fn advance_until_trigger_expires(
    runtime: &mut SimRuntime,
    trigger: &str,
    due: i32,
) -> crate::sim::world::SimFrameOutput {
    advance_to_trigger_due(runtime, trigger, due);
    poll_expired_trigger(runtime, trigger)
}

/// Snapshot the active map/program state, then restore it through the same
/// derived-cache and map-authority owners used by a loaded headless match.
fn save_restore(runtime: &mut SimRuntime) {
    runtime.simulation.clear_tag_dispatch_trace_for_test();
    let rules = &runtime.resources.rules;
    let sim = &runtime.simulation;
    let terrain = sim.resolved_terrain().unwrap().clone();
    let terrain_speed_config = sim.terrain_speed_config().clone();
    let trigger_runtime = sim.trigger_runtime.clone();
    let binary_frame = sim.session().binary_frame();
    let bytes = GameSnapshot::save_validated(
        sim,
        0,
        rules.simulation_config_hash(),
        "issue1227-retail-map",
        0,
    );
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(terrain, terrain_speed_config, rules);
    restored
        .restore_map_authority_after_snapshot_load(rules, &runtime.resources.overlay_registry, None)
        .unwrap();
    restored.resolve_type_handles(rules);
    // Native in-scenario load resets only Scenario RNG to Seed0; the shared
    // trigger owner, active timers, map authority and clock continue from save.
    assert_eq!(restored.trigger_runtime, trigger_runtime);
    assert_eq!(restored.session().binary_frame(), binary_frame);
    assert_eq!(
        restored.scenario_rng.logical_state(),
        crate::sim::rng::SimRng::new(0).logical_state()
    );
    runtime.replace_simulation(restored);
}

/// Reset726400 @ 0x00726400 uses one inclusive Scenario RandomRanged call,
/// then (value/2 + draw) * 15 logic frames for Event51. The shared SimRng
/// owner supplies the draw; this keeps the test free of a second RNG port.
fn event51_duration(rng: &mut crate::sim::rng::SimRng, value: i32) -> i32 {
    (value / 2)
        .wrapping_add(rng.next_range_i32_inclusive(0, value))
        .wrapping_mul(RA2_LOGIC_FRAMES_PER_SECOND as i32)
}

fn advance_weather_transition(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    source: &str,
    target: &str,
    source_delay: i32,
    target_delay: i32,
    strike_waypoint: Option<u32>,
) {
    let sim = &scenario.runtime.simulation;
    assert!(
        trigger_enabled(sim, source),
        "{source} must be enabled before expiry"
    );
    assert_eq!(event_value(sim, source, 51), source_delay, "{source} delay");
    assert_eq!(event_value(sim, target, 51), target_delay, "{target} delay");
    if let Some(waypoint) = strike_waypoint {
        assert_eq!(action_waypoints(sim, source, 102), [waypoint]);
    } else {
        assert!(action_waypoints(sim, source, 102).is_empty());
    }
    assert_eq!(action_trigger_targets(sim, source, 53), [target.to_owned()]);
    assert_eq!(action_trigger_targets(sim, source, 54), [source.to_owned()]);

    let timer = trigger_timer(sim, source);
    let due = event_due(timer);
    advance_to_trigger_due(&mut scenario.runtime, source, due);
    let storm_before = scenario
        .runtime
        .simulation
        .lightning_storm()
        .globals_for_test();
    let mut expected_rng = scenario.runtime.simulation.scenario_rng.clone();
    let source_duration = event51_duration(&mut expected_rng, source_delay);
    let target_duration = event51_duration(&mut expected_rng, target_delay);
    let frame_output = poll_expired_trigger(&mut scenario.runtime, source);
    assert!(frame_output.tick.frame_committed);

    let sim = &scenario.runtime.simulation;
    assert!(
        !trigger_enabled(sim, source),
        "{source} disables itself after expiry"
    );
    assert!(
        trigger_enabled(sim, target),
        "{target} is enabled by {source}"
    );
    let source_timer = trigger_timer(sim, source);
    assert_eq!(source_timer.start_frame(), due, "repeat reset for {source}");
    assert_eq!(
        source_timer.duration(),
        source_duration,
        "repeat reset for {source}"
    );
    let target_timer = trigger_timer(sim, target);
    assert_eq!(target_timer.start_frame(), due, "enable reset for {target}");
    assert_eq!(
        target_timer.duration(),
        target_duration,
        "enable reset for {target}"
    );

    if let Some(waypoint) = strike_waypoint {
        let target = scenario.map.waypoints.get(&waypoint).unwrap();
        let storm = sim.lightning_storm().globals_for_test();
        assert_eq!(
            storm.2,
            scenario.runtime.resources.rules.general.lightning_deferment - 1
        );
        assert_eq!(
            storm.3,
            scenario
                .runtime
                .resources
                .rules
                .general
                .lightning_storm_duration
        );
        assert_eq!(
            storm.4, storm_before.4,
            "deferred Start does not write StartTime"
        );
        assert_eq!(storm.5, (target.rx as i16, target.ry as i16));
    }
}

#[test]
#[ignore = "slow; requires retail assets and ordinary frames for the native timers"]
fn xbbermuda_weather_cycle_polls_real_event51_tags_through_the_skirmish_runtime() {
    let (root, mut assets) = retail_scenario_assets().unwrap();
    let map_name = "XBermuda.MAP";
    assert_pkt_lists_map(&assets, "MISSIONSMD.PKT", map_name);
    let loaded = load_map_by_name_or_path_with_assets(&root, map_name, &assets).unwrap();
    assert!(matches!(loaded.source, LoadedMapSource::Mix { .. }));

    let mut scenario = load_skirmish(&root, map_name, MAP_SEED, &mut assets);
    let sim = &scenario.runtime.simulation;
    let ids = [
        "Stormy Weather 1",
        "Stormy Weather 2",
        "Stormy Weather 3",
        "Stormy Weather 4",
    ]
    .map(|label| trigger_key_for_tag_name(&scenario.map, label));
    let [storm1, storm2, storm3, storm4] = &ids;
    assert_eq!(
        [
            event_value(sim, storm1, 51),
            event_value(sim, storm2, 51),
            event_value(sim, storm3, 51),
            event_value(sim, storm4, 51),
        ],
        [100, 300, 150, 200]
    );
    assert_eq!(action_trigger_targets(sim, storm2, 53), [storm1.clone()]);
    assert!(action_waypoints(sim, storm2, 102).is_empty());
    assert_eq!(action_waypoints(sim, storm1, 102), [78]);
    assert_eq!(action_trigger_targets(sim, storm1, 53), [storm3.clone()]);
    assert_eq!(action_waypoints(sim, storm3, 102), [79]);
    assert_eq!(action_trigger_targets(sim, storm3, 53), [storm4.clone()]);
    assert_eq!(action_waypoints(sim, storm4, 102), [80]);
    assert_eq!(action_trigger_targets(sim, storm4, 53), [storm2.clone()]);
    assert!(trigger_enabled(sim, storm2));
    assert!(
        trigger_enabled(sim, storm4),
        "Battle uses Medium difficulty"
    );
    assert!(!trigger_enabled(sim, storm1));
    assert!(!trigger_enabled(sim, storm3));

    let initial_storm2_due = event_due(trigger_timer(sim, storm2));
    let initial_storm4_due = event_due(trigger_timer(sim, storm4));
    assert!(
        initial_storm4_due < initial_storm2_due,
        "fixed retail seed must exercise the initially-enabled Storm4 path first"
    );
    for (id, value) in [(storm1, 100), (storm2, 300), (storm3, 150), (storm4, 200)] {
        let timer = trigger_timer(sim, id);
        let minimum = (value / 2) * RA2_LOGIC_FRAMES_PER_SECOND as i32;
        let maximum = (value / 2 + value) * RA2_LOGIC_FRAMES_PER_SECOND as i32;
        assert!((minimum..=maximum).contains(&timer.duration()), "{id}");
    }

    // The physical Tag order resets disabled Storm1/3 and difficulty-enabled
    // Storm4 during Fill. Restore the actual four timers before the first poll.
    save_restore(&mut scenario.runtime);
    advance_weather_transition(&mut scenario, storm4, storm2, 200, 300, Some(80));
    let deferred = scenario
        .runtime
        .simulation
        .lightning_storm()
        .globals_for_test();
    save_restore(&mut scenario.runtime);
    assert_eq!(
        scenario
            .runtime
            .simulation
            .lightning_storm()
            .globals_for_test(),
        deferred,
        "the deferred Storm4 state survives native save/load"
    );
    for _ in 0..300 {
        if crate::sim::superweapon::lightning_storm::raging(&scenario.runtime.simulation) {
            break;
        }
        let _ = advance_queued_frame(&mut scenario.runtime);
    }
    assert!(crate::sim::superweapon::lightning_storm::raging(
        &scenario.runtime.simulation
    ));
    advance_weather_transition(&mut scenario, storm2, storm1, 300, 100, None);
    advance_weather_transition(&mut scenario, storm1, storm3, 100, 150, Some(78));
    advance_weather_transition(&mut scenario, storm3, storm4, 150, 200, Some(79));
    advance_weather_transition(&mut scenario, storm4, storm2, 200, 300, Some(80));
}

#[test]
#[ignore = "slow; requires retail assets and ordinary frames through the 600 second event"]
fn high_explosive_revenge_runs_countdown_lab_entry_and_nukes_through_normal_polls() {
    let (root, mut assets) = retail_scenario_assets().unwrap();
    let map_name = "HighExpR.MAP";
    assert_pkt_lists_map(&assets, "HighExpR.PKT", map_name);
    let loaded = load_map_by_name_or_path_with_assets(&root, map_name, &assets).unwrap();
    let LoadedMapSource::Mix { source_archive, .. } = loaded.source else {
        panic!("HighExpR.MAP must resolve through its registered loose YRO");
    };
    assert!(source_archive.eq_ignore_ascii_case("HighExpR.yro"));

    let mut scenario = load_skirmish(&root, map_name, MAP_SEED, &mut assets);
    let countdown = scenario
        .map
        .tags
        .values()
        .find(|tag| tag.name().eq_ignore_ascii_case("Nuke Countdown Enable"))
        .and_then(|tag| tag.trigger())
        .unwrap()
        .to_owned();
    let sim = &scenario.runtime.simulation;
    assert_eq!(event_value(sim, &countdown, 13), 600);
    let countdown_timer = trigger_timer(sim, &countdown);
    assert_eq!(
        countdown_timer.duration(),
        600 * RA2_LOGIC_FRAMES_PER_SECOND as i32,
        "Reset726400 @ 0x0072641C converts Event13 seconds to 15 logic frames per second"
    );
    assert!(trigger_enabled(sim, &countdown));

    let countdown_due = event_due(countdown_timer);
    let output = advance_until_trigger_expires(&mut scenario.runtime, &countdown, countdown_due);
    assert!(output.tick.frame_committed);
    let sim = &scenario.runtime.simulation;
    let entered: Vec<_> = (b'A'..=b'F')
        .map(|suffix| {
            trigger_key_for_tag_name(&scenario.map, &format!("Lab {}", char::from(suffix)))
        })
        .collect();
    let enabled_targets = action_trigger_targets(sim, &countdown, 53);
    for target in &entered {
        assert!(
            enabled_targets.contains(target),
            "countdown enables {target}"
        );
        assert!(trigger_enabled(sim, target), "countdown enabled {target}");
    }
    let eva_index = sim.trigger_runtime.definitions.trigger_types
        [trigger_type_index(sim, &countdown)]
    .actions
    .iter()
    .find(|action| action.entry.kind == 21)
    .and_then(|action| action.value)
    .unwrap();
    assert!(
        output.sound_events.iter().any(
            |event| matches!(event, SimSoundEvent::TriggerEva { index } if *index == eva_index)
        ),
        "Action21 reaches the EVAMD-resolved NuclearMissileReady cue"
    );

    // Lab A is a real CALAB [Structures] entry attached to the map's Lab A
    // object Tag. The ordinary CaptureBuilding order and active-object walk
    // reach the existing Building-entry owner with a live Engineer.
    let (lab_tag_id, lab) = scenario
        .map
        .tags
        .iter()
        .find(|(_, tag)| {
            tag.name().eq_ignore_ascii_case("Lab A")
                && tag
                    .trigger()
                    .is_some_and(|id| id.eq_ignore_ascii_case(&entered[0]))
        })
        .and_then(|(tag_id, tag)| {
            let display_name = tag.name().to_owned();
            scenario
                .map
                .entities
                .iter()
                .find(|entity| {
                    entity.category == crate::map::entities::EntityCategory::Structure
                        && entity.type_id.eq_ignore_ascii_case("CALAB")
                        && entity.trigger_tag.as_deref().is_some_and(|authored_tag| {
                            authored_tag.eq_ignore_ascii_case(tag_id)
                                || authored_tag.eq_ignore_ascii_case(&display_name)
                        })
                })
                .map(|lab| (tag_id.clone(), lab))
        })
        .expect("Lab A is an authored CALAB with its Entered A Tag");
    let rules = &scenario.runtime.resources.rules;
    let overlays = &scenario.runtime.resources.overlay_registry;
    let lab_tag_index = {
        let sim = &mut scenario.runtime.simulation;
        let lab_id = sim
            .entities()
            .values()
            .find(|entity| {
                entity.position.rx() == lab.cell_x
                    && entity.position.ry() == lab.cell_y
                    && sim.resolve(entity.type_ref()) == lab.type_id
            })
            .map(|entity| entity.stable_id())
            .expect("the headless scenario attached the map's CALAB");
        assert!(sim.object_has_trigger_tag(lab_id));
        let current_house = sim.session().current_house().unwrap();
        let engineer_name = sim.interner.resolve(current_house).to_owned();
        let engineer = sim
            .spawn_object_with_overlay_registry(
                "ENGINEER",
                &engineer_name,
                lab.cell_x.saturating_sub(3),
                lab.cell_y.saturating_add(3),
                0,
                rules,
                overlays,
            )
            .expect("an Engineer can be staged near Lab A");
        let tag_index = sim
            .trigger_runtime
            .tag_order
            .iter()
            .copied()
            .find(|&index| {
                sim.trigger_runtime.tag(index).is_some_and(|tag| {
                    sim.trigger_runtime.definitions.tag_types[tag.type_index]
                        .id
                        .eq_ignore_ascii_case(&lab_tag_id)
                })
            })
            .unwrap();
        sim.clear_tag_dispatch_trace_for_test();
        sim.queue_command(crate::sim::command::CommandEnvelope::new(
            current_house,
            sim.session().tick(),
            crate::sim::command::Command::CaptureBuilding {
                engineer_id: engineer,
                target_building_id: lab_id,
            },
        ));
        tag_index
    };
    let mut entered_lab = false;
    for _ in 0..300 {
        let _ = advance_queued_frame(&mut scenario.runtime);
        entered_lab = scenario
            .runtime
            .simulation
            .tag_dispatch_trace_for_test()
            .iter()
            .any(|&(tag, event)| tag == lab_tag_index && event == 1);
        if entered_lab {
            break;
        }
    }
    assert!(entered_lab, "CaptureBuilding walks the Engineer into CALAB");
    let sim = &scenario.runtime.simulation;
    assert!(
        !trigger_enabled(sim, &entered[0]),
        "Entered A disables after lab entry"
    );
    let entered_targets = action_trigger_targets(sim, &entered[0], 53);
    let nukes = trigger_key_for_tag_name(&scenario.map, "A Nukes");
    assert!(
        entered_targets.contains(&nukes),
        "Entered A enables A Nukes among the authored action53 targets"
    );
    for target in &entered_targets {
        assert!(
            trigger_enabled(sim, target),
            "Engineer entry enables {target}"
        );
    }
    assert!(trigger_enabled(sim, &nukes));
    assert_eq!(event_value(sim, &nukes, 13), 0);

    // Save while the map-authored zero-second Nukes timer is armed. The next
    // ordinary master-frame poll enables its Hits and the lab/countdown routes.
    save_restore(&mut scenario.runtime);
    let _ = advance_queued_frame(&mut scenario.runtime);
    let sim = &scenario.runtime.simulation;
    assert!(
        !trigger_enabled(sim, &nukes),
        "A Nukes disables after arming Hits"
    );
    let nukes_targets = action_trigger_targets(sim, &nukes, 53);
    let expected_targets = [
        "Hit B",
        "Hit C",
        "Hit D",
        "Hit E",
        "Hit F",
        "Restore All Labs",
        "Reset It All",
    ]
    .map(|label| trigger_key_for_tag_name(&scenario.map, label));
    assert_eq!(nukes_targets, expected_targets);
    let hit_keys = expected_targets[..5].to_vec();
    let restore_all = expected_targets[5].clone();
    let reset_all = expected_targets[6].clone();
    for (hit, waypoint) in hit_keys.iter().zip([1, 2, 3, 4, 5]) {
        assert!(trigger_enabled(sim, hit));
        assert_eq!(event_value(sim, hit, 13), 10);
        assert_eq!(
            trigger_timer(sim, hit).duration(),
            10 * RA2_LOGIC_FRAMES_PER_SECOND as i32
        );
        assert_eq!(action_waypoints(sim, hit, 95), [waypoint]);
    }

    assert_eq!(event_value(sim, &restore_all, 13), 0);
    assert_eq!(
        action_kinds(sim, &restore_all),
        [130, 130, 130, 130, 130, 130, 54]
    );
    let restore_index = trigger_type_index(sim, &restore_all);
    let restore_selectors: Vec<_> = sim.trigger_runtime.definitions.trigger_types[restore_index]
        .actions
        .iter()
        .filter(|action| action.entry.kind == 130)
        .filter_map(|action| action.value)
        .collect();
    assert_eq!(restore_selectors, [4475, 4476, 4477, 4478, 4479, 4480]);
    assert_eq!(
        action_trigger_targets(sim, &restore_all, 54),
        [restore_all.clone()]
    );

    assert_eq!(event_value(sim, &reset_all, 13), 10);
    assert_eq!(
        action_kinds(sim, &reset_all),
        [53, 53, 53, 53, 53, 53, 53, 53, 54]
    );
    let countdown_targets = [
        "Nuke Countdown Timer",
        "Nuke Countdown Enable",
        "Nuke Countdown 1",
        "Nuke Countdown 2",
        "Nuke Countdown 3",
        "Nuke Countdown 4",
        "Nuke Countdown 5",
        "Nuke Countdown 6",
    ]
    .map(|label| trigger_key_for_tag_name(&scenario.map, label));
    assert_eq!(
        action_trigger_targets(sim, &reset_all, 53),
        countdown_targets
    );
    assert_eq!(
        action_trigger_targets(sim, &reset_all, 54),
        [reset_all.clone()]
    );
    assert!(trigger_enabled(sim, &reset_all));
    assert_eq!(
        trigger_timer(sim, &reset_all).duration(),
        10 * RA2_LOGIC_FRAMES_PER_SECOND as i32
    );
    for _ in 0..2 {
        if !trigger_enabled(&scenario.runtime.simulation, &restore_all) {
            break;
        }
        let _ = advance_queued_frame(&mut scenario.runtime);
    }
    assert!(!trigger_enabled(&scenario.runtime.simulation, &restore_all));

    save_restore(&mut scenario.runtime);
    let sim = &scenario.runtime.simulation;
    let hit_b = trigger_key_for_tag_name(&scenario.map, "Hit B");
    let due = event_due(trigger_timer(sim, &hit_b));
    advance_to_trigger_due(&mut scenario.runtime, &hit_b, due);
    let projectile_ids_before: Vec<_> = scenario
        .runtime
        .simulation
        .projectiles
        .iter()
        .map(|(id, _)| *id)
        .collect();
    let output = poll_expired_trigger(&mut scenario.runtime, &hit_b);
    assert!(output.tick.frame_committed);
    let sim = &scenario.runtime.simulation;
    assert!(hit_keys.iter().all(|hit| !trigger_enabled(sim, hit)));
    assert!(!trigger_enabled(sim, &reset_all));
    let countdown_enable = trigger_key_for_tag_name(&scenario.map, "Nuke Countdown Enable");
    assert!(trigger_enabled(sim, &countdown_enable));
    assert_eq!(trigger_timer(sim, &countdown_enable).start_frame(), due);
    assert_eq!(
        trigger_timer(sim, &countdown_enable).duration(),
        600 * RA2_LOGIC_FRAMES_PER_SECOND as i32
    );
    let projectiles: Vec<_> = sim
        .projectiles
        .iter()
        .filter(|(id, _)| !projectile_ids_before.contains(*id))
        .collect();
    assert_eq!(
        projectiles.len(),
        5,
        "Hit B-F add the five authored missiles"
    );
    let mut targets: Vec<_> = projectiles
        .iter()
        .map(|(_, projectile)| match projectile.target() {
            crate::sim::projectile::ProjectileTarget::Cell { rx, ry } => (rx, ry),
            other => panic!("nuclear action targets a map cell, got {other:?}"),
        })
        .collect();
    let mut expected_targets: Vec<_> = [1, 2, 3, 4, 5]
        .into_iter()
        .map(|index| {
            let waypoint = scenario.map.waypoints.get(&index).unwrap();
            (waypoint.rx, waypoint.ry)
        })
        .collect();
    targets.sort_unstable();
    expected_targets.sort_unstable();
    assert_eq!(targets, expected_targets);
    assert!(projectiles.iter().all(|(_, projectile)| {
        let payload = projectile.payload();
        payload.base_damage == 350
            && payload.installed_weapon().is_some_and(|weapon| {
                sim.interner
                    .resolve(weapon)
                    .eq_ignore_ascii_case("NukePayload")
            })
    }));
}
