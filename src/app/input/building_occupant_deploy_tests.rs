//! Stock retail absorber entry followed by the existing window-independent
//! selection/D-command producer. Arena/session/visibility are supplied inputs;
//! this does not execute a window message, physical key or native whole tick.

use super::*;
use crate::rules::{ini_parser::IniFile, overlay_types::OverlayTypeRegistry};
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, TickLane};

#[test]
fn deploy_key_on_a_selected_loaded_bio_reactor_does_not_unload_absorber_cargo() {
    let Some(mut retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &IniFile::from_str(""),
    ) else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
    retail
        .rules
        .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let rules = retail.rules;
    let reactor_type = rules.object("YAPOWR").unwrap();
    assert!(reactor_type.infantry_absorb);
    assert!(!reactor_type.can_be_occupied);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x730af0);
    sim.install_overlay_grid_for_load(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .short_game = false;
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    for name in ["Americans", "Russians"] {
        let house = sim.interner().get(name).unwrap();
        sim.houses_mut_for_test()
            .get_mut(&house)
            .unwrap()
            .set_current_iq_for_test(0);
        sim.fog_mut_for_test().reveal_all_for_owner(house);
    }
    let reactor = sim
        .spawn_object_with_overlay_registry("YAPOWR", "Americans", 16, 14, 0, &rules, &registry)
        .unwrap();
    sim.spawn_object_with_overlay_registry("NAPOWR", "Russians", 28, 28, 0, &rules, &registry)
        .unwrap();
    let initiate = sim
        .spawn_object_with_overlay_registry("INIT", "Americans", 12, 14, 0, &rules, &registry)
        .unwrap();
    let terrain_template = sim.resolved_terrain().cloned();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources {
            rules,
            overlay_registry: registry,
            terrain_template,
        },
    };
    assert!(runtime.apply_command_for_test(
        "Americans",
        &Command::clicked_mission(
            initiate,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(reactor),
        )
    ));
    for _ in 0..600 {
        let due = runtime.simulation.take_due_commands();
        let output = runtime
            .advance_frame(&due, 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(output.tick.frame_committed);
        if runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .passenger_role
            .inside_transport_id()
            == Some(reactor)
        {
            break;
        }
    }
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .passenger_role
            .inside_transport_id(),
        Some(reactor)
    );
    assert!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert!(runtime.apply_command_for_test(
        "Americans",
        &Command::Select {
            entity_ids: vec![reactor],
            additive: false
        }
    ));
    assert!(
        runtime
            .simulation
            .entities()
            .get(reactor)
            .unwrap()
            .selection
            .is_selected()
    );
    // Physical DeployObject=D reaches Selection730AF0. Its special Building
    // arm queries the garrison vector, then ordinary Building465D30 refuses.
    // Cargo-only Bio Reactor self-click Unload is a separate legal producer.
    let commands = deploy_key_commands(
        &runtime.simulation,
        Some(&runtime.resources.rules),
        &[reactor],
        Some(owner),
    );
    assert!(
        commands.is_empty(),
        "D must not turn absorber Cargo into garrison occupants"
    );
    let due = runtime.simulation.take_due_commands();
    let output = runtime
        .advance_frame(&due, 33, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    assert!(output.tick.frame_committed);
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(reactor)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers(),
        [initiate]
    );
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .passenger_role
            .inside_transport_id(),
        Some(reactor)
    );
    assert!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    // The same physical loaded object still admits its independent self-click
    // Unload through Building447330/44353F and actually releases its Cargo.
    let hover = crate::app::input::entity_pick::HoverTargetKindWithId {
        kind: crate::app::types::HoverTargetKind::FriendlyStructure,
        stable_id: reactor,
    };
    let clicked = crate::app::input::context_order::self_click_command(
        &runtime.simulation,
        &runtime.resources.rules,
        Some(&runtime.resources.overlay_registry),
        owner,
        &[reactor],
        Some(&hover),
        false,
    )
    .expect("loaded stock Bio Reactor self-click remains legal");
    assert_eq!(
        clicked,
        Command::UnloadPassengers {
            transport_id: reactor
        }
    );
    assert!(runtime.apply_command_for_test("Americans", &clicked));
    for _ in 0..180 {
        let due = runtime.simulation.take_due_commands();
        let output = runtime
            .advance_frame(&due, 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(output.tick.frame_committed);
        if runtime
            .simulation
            .entities()
            .get(reactor)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
        {
            break;
        }
    }
    assert!(
        runtime
            .simulation
            .entities()
            .get(reactor)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .passenger_role
            .inside_transport_id(),
        None
    );
    assert!(
        !runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
}
