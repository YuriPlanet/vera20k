//! FootAI4DA7B0..4DA80C: periodic airborne CellTag59 before Process.
//! Instruction-established ingress, exercised through the production object
//! turn. Supplied airborne state is not a native movement-replay comparison.

use super::*;
use crate::map::map_file::MapFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::house_state::HouseState;
use crate::sim::world::FrameEffects;

fn arena(retail: Option<RuleSet>) -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let (mut sim, authored, registry) = super::super::entry_test_fixture::fixture_with_rules(
        "[Countries]\n0=Americans\n1=Russians\n[Americans]\n[Russians]\n",
    );
    let rules = retail.unwrap_or(authored);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (index, name) in ["Americans", "Russians"].iter().enumerate() {
        let house = sim.interner.intern(name);
        sim.houses.register(HouseState::new(
            house,
            index as u8,
            Some(house),
            index == 0,
            0,
            10,
        ));
    }
    // Neither movement nor alliance with the local viewer gates this ingress.
    let viewer = sim.interner.get("Russians").unwrap();
    sim.session.set_current_house_for_test(Some(viewer));
    (sim, rules, registry)
}

fn install_tag(
    sim: &mut Simulation,
    rules: &RuleSet,
    repeat: u8,
    country: i32,
    cell: (i16, i16),
    actions: &str,
) -> MapFile {
    let text = format!(
        "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Basic]\nNewINIFormat=4\n\
         [Triggers]\nAIR=Americans,<none>,Air,0,1,1,1,0\n\
         [Events]\nAIR=1,59,0,{country}\n\
         [Actions]\nAIR={actions}\n\
         [Tags]\nAIR_TAG={repeat},Air,AIR\n[CellTags]\n{}=AIR_TAG\n\
         [Waypoints]\n0=17017\n",
        i32::from(cell.1) * 1000 + i32::from(cell.0),
    );
    let map = MapFile::from_bytes(text.as_bytes()).unwrap();
    sim.initialize_trigger_definitions(&map, &TeamAiIniRegistry::default(), None, None);
    sim.install_authored_cell_trigger_tags(&map);
    sim.classify_map_triggers(rules);
    sim.session.install_waypoints(&map.waypoints);
    map
}

fn scene() -> (Simulation, RuleSet, OverlayTypeRegistry, u64) {
    let (mut sim, rules, registry) = arena(None);
    let actor = sim
        .spawn_object("JUMPJET", "Americans", 15, 15, 0, &rules)
        .unwrap();
    Simulation::set_object_height_unmarked(&mut sim.substrate, &sim.resolved_terrain, actor, 500);
    let runtime = sim
        .substrate
        .entities
        .get_mut(actor)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .jumpjet_runtime_mut()
        .unwrap();
    *runtime = runtime
        .clone()
        .with_phase_for_test(crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_HOLD);
    install_tag(&mut sim, &rules, 0, -1, (15, 15), "1,56,0,7,0,0,0,0,A");
    (sim, rules, registry, actor)
}

fn raise(sim: &mut Simulation, rules: &RuleSet, registry: &OverlayTypeRegistry, actor: u64) {
    sim.dispatch_airborne_cell_tag_event(
        actor,
        super::super::UninitContext::new(rules, Some(registry)),
    );
}

#[test]
fn airborne_cell_tag_uses_the_signed_16_frame_boundary_without_rng_or_sight_timer_writes() {
    for frame in [0, 1, 15, 16, 17, 31, 32, 0xffff_fff0, 0xffff_ffff] {
        let (mut sim, rules, registry, actor) = scene();
        sim.session.set_binary_frame_for_test(frame);
        let rng = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let viewer = sim.session.current_house().unwrap();
        let timer = sim
            .substrate
            .entities
            .get(actor)
            .unwrap()
            .sight_refresh_timers
            .timer(viewer);
        raise(&mut sim, &rules, &registry, actor);
        assert_eq!(
            sim.trigger_local_is_set_for_test(7),
            frame % 16 == 0,
            "frame {frame}"
        );
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng,
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(actor)
                .unwrap()
                .sight_refresh_timers
                .timer(viewer),
            timer
        );
    }
}

#[test]
fn airborne_cell_tag_reuses_marked_and_relative_height_admission() {
    for (marked, height, expected) in [(false, 500, false), (true, 207, false), (true, 208, true)] {
        let (mut sim, rules, registry, actor) = scene();
        sim.session.set_binary_frame_for_test(16);
        Simulation::set_object_height_unmarked(
            &mut sim.substrate,
            &sim.resolved_terrain,
            actor,
            height,
        );
        sim.substrate
            .entities
            .get_mut(actor)
            .unwrap()
            .lifecycle
            .set_cell_marked_for_test(marked);
        raise(&mut sim, &rules, &registry, actor);
        assert_eq!(
            sim.trigger_local_is_set_for_test(7),
            expected,
            "marked={marked}, height={height}"
        );
    }
}

#[test]
fn airborne_cell_tag_uses_physical_cell_and_ignores_arrival_bridge_suppression() {
    let (mut sim, rules, registry, actor) = scene();
    sim.session.set_binary_frame_for_test(16);
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    let cell = terrain.native_cell_identity((15, 15));
    terrain.write_native_cell_flags(cell, 0x500);
    let entity = sim.substrate.entities.get_mut(actor).unwrap();
    let runtime = entity
        .locomotor
        .as_mut()
        .unwrap()
        .jumpjet_runtime_mut()
        .unwrap();
    *runtime = runtime
        .clone()
        .with_destination_for_test(crate::sim::components::DriveCoord::cell(17, 15, 0));
    assert_ne!(
        crate::sim::movement::coord_cell(sim.foot_navigation_coordinate(actor).unwrap()),
        (15, 15)
    );
    assert!(!sim.substrate.entities.get(actor).unwrap().on_bridge());
    raise(&mut sim, &rules, &registry, actor);
    assert!(sim.trigger_local_is_set_for_test(7));
}

#[test]
fn airborne_cell_tag_preserves_the_existing_country_predicate_and_one_shot_retirement() {
    for country in [0, 1] {
        let (mut sim, rules, registry, actor) = scene();
        install_tag(&mut sim, &rules, 0, country, (15, 15), "1,56,0,7,0,0,0,0,A");
        sim.session.set_binary_frame_for_test(16);
        raise(&mut sim, &rules, &registry, actor);
        assert_eq!(sim.tag_dispatch_trace_for_test().len(), 1);
        assert_eq!(sim.trigger_local_is_set_for_test(7), country == 0);
        let identity = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((15, 15));
        assert_eq!(
            sim.cell_trigger_tag_for_test(identity).is_none(),
            country == 0
        );
        raise(&mut sim, &rules, &registry, actor);
        assert_eq!(
            sim.tag_dispatch_trace_for_test().len(),
            if country == 0 { 1 } else { 2 }
        );
    }
}

#[test]
fn airborne_cell_tag_damage_and_remaining_actions_run_synchronously() {
    let (mut sim, rules, registry, actor) = scene();
    install_tag(
        &mut sim,
        &rules,
        0,
        -1,
        (15, 15),
        "2,32,0,0,0,0,0,0,A,56,0,9,0,0,0,0,A",
    );
    sim.attach_object_trigger_tag(actor, Some("AIR_TAG"));
    sim.session.set_binary_frame_for_test(16);
    sim.advance_live_object_turn(
        actor,
        &rules,
        techno_ai::ObjectAiCtx {
            overlay_registry: Some(&registry),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        sim.trigger_local_is_set_for_test(9),
        "the surviving dispatcher continues its action list"
    );
    assert!(
        sim.substrate
            .entities
            .get(actor)
            .is_none_or(|entity| !entity.lifecycle.object_alive())
    );
}

#[test]
fn retail_rocketeer_move_reaches_airborne_cell_tag_and_camera_effect_once() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::trigger_runtime::TriggerEffect;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert!(retail.rules.object("JUMPJET").unwrap().balloon_hover);
    let (mut sim, rules, _) = arena(Some(retail.rules));
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let actor = sim
        .spawn_object("JUMPJET", "Americans", 15, 15, 0, &rules)
        .unwrap();
    install_tag(&mut sim, &rules, 0, -1, (17, 15), "1,112,0,0,0,0,0,0,A");
    let house = sim.interner.get("Americans").unwrap();
    sim.queue_command(CommandEnvelope::new(
        house,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: actor,
            target_rx: 17,
            target_ry: 15,
        },
    ));
    let mut effects = Vec::new();
    for _ in 0..240 {
        let commands = sim.take_due_commands();
        let grid = sim.path_grid.clone();
        sim.advance_tick(
            &commands,
            Some(&rules),
            grid.as_deref(),
            Some(&registry),
            33,
        );
        effects.extend(sim.drain_trigger_effects());
    }
    assert_eq!(
        effects,
        [TriggerEffect::CameraAtCoord {
            coord: [4480, 4480, 0],
            velocity: None
        }]
    );
    assert_eq!(sim.substrate.entities.get(actor).unwrap().position.rx(), 17);
    assert!(crate::sim::movement::air_movement::is_high_flying(
        sim.substrate.entities.get(actor).unwrap(),
        sim.resolved_terrain.as_ref(),
        Some((&rules, &sim.interner))
    ));
    assert_eq!(
        sim.tag_dispatch_trace_for_test()
            .iter()
            .filter(|call| call.1 == 59)
            .count(),
        1
    );
}

#[test]
fn stationary_hostile_rocketeer_raises_the_airborne_cell_tag_on_frame_16() {
    let (mut sim, rules, registry, actor) = scene();
    sim.session.set_binary_frame_for_test(16);
    assert!(crate::sim::movement::air_movement::is_high_flying(
        sim.substrate.entities.get(actor).unwrap(),
        sim.resolved_terrain.as_ref(),
        Some((&rules, &sim.interner)),
    ));
    sim.advance_live_object_turn(
        actor,
        &rules,
        techno_ai::ObjectAiCtx {
            overlay_registry: Some(&registry),
            effects: FrameEffects::default(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(sim.trigger_local_is_set_for_test(7));
    assert_eq!(
        sim.tag_dispatch_trace_for_test()
            .iter()
            .map(|call| call.1)
            .collect::<Vec<_>>(),
        [59]
    );
}
