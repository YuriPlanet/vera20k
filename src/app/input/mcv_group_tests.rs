//! Group+214 survives UnitDeploy7397C6 and BuildingSelling44A04C.
//! Ordinary player commands on authored clear ground with unmodified retail
//! rules and the real Construction Yard buildup; no conversion-state writes.
//! Main now owns Group+214 on each GameEntity; exercise its scheduled command
//! and projection instead of restoring the superseded app-side group lists.

use super::*;
use crate::app::input::state::{MatchInputState, PreparedSavedLocalView};
use crate::sim::command::CommandEnvelope;
use crate::sim::house_state::HouseState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, Simulation, TickLane};
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;

fn scene(yard: bool, blocked: bool) -> Option<(SimRuntime, MatchInputState, u64, u64)> {
    let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    rules.bind_building_buildup_assets(&assets, "TEMPERATE");
    let [_, frames, rate] = rules.buildup_control("GACNST");
    assert!(rules.has_buildup("GACNST") && frames > 1 && rate > 0);
    let mut sim = Simulation::with_seed(0x7397cc);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .short_game = false;
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .mcv_redeploy = true;
    for (name, human) in [("Americans", true), ("Russians", false)] {
        let owner = sim.intern(name);
        let side = rules.country_side_index(name).unwrap().0;
        let mut house = HouseState::new(owner, side, Some(owner), human, 0, 10);
        house.project_country_mults(&rules, sim.interner());
        sim.houses_mut_for_test().register(house);
    }
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    *sim.overlay_grid_mut_for_test() = Some(OverlayGrid::new(32, 32));
    sim.set_input_delay_ticks(0);
    // Both bases and the full conversion foundation fit the native Size20
    // diamond. These are initial scene objects admitted through Unlimbo.
    sim.spawn_object("GAPOWR", "Americans", 14, 14, 0, &rules)
        .unwrap();
    sim.spawn_object("NAPOWR", "Russians", 28, 26, 0, &rules)
        .unwrap();
    if blocked {
        sim.spawn_object("GAPOWR", "Americans", 21, 19, 0, &rules)
            .unwrap();
    }
    let original = sim
        .spawn_object(
            if yard { "GACNST" } else { "AMCV" },
            "Americans",
            20,
            20,
            96,
            &rules,
        )
        .unwrap();
    let reserve = sim
        .spawn_object("MTNK", "Americans", 26, 24, 0, &rules)
        .unwrap();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources::new(rules),
    };
    let mut input = MatchInputState::new(Default::default());
    frame(
        &mut runtime,
        &mut input,
        vec![Command::Select {
            entity_ids: vec![reserve],
            additive: false,
        }],
    );
    assert_eq!(input.selection_order, [reserve]);
    schedule_control_group_assignment(
        &mut runtime.simulation,
        "Americans",
        9,
        input.selection_order.clone(),
    )
    .unwrap();
    frame(&mut runtime, &mut input, vec![]);
    frame(
        &mut runtime,
        &mut input,
        vec![Command::Select {
            entity_ids: vec![original],
            additive: false,
        }],
    );
    assert_eq!(input.selection_order, [original]);
    schedule_control_group_assignment(
        &mut runtime.simulation,
        "Americans",
        2,
        input.selection_order.clone(),
    )
    .unwrap();
    frame(&mut runtime, &mut input, vec![]);
    Some((runtime, input, original, reserve))
}

fn frame(runtime: &mut SimRuntime, input: &mut MatchInputState, payloads: Vec<Command>) {
    let owner = runtime.simulation.interner().get("Americans").unwrap();
    let mut commands = runtime.simulation.take_due_commands();
    commands.extend(payloads.into_iter().map(|payload| {
        CommandEnvelope::new(owner, runtime.simulation.session().tick() + 1, payload)
    }));
    let output = runtime
        .advance_frame(&commands, 33, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    assert!(output.tick.frame_committed);
    for fact in &output.lifecycle_outputs {
        input.apply_lifecycle_output(fact, Some(owner));
    }
    reconcile_selection_order_for_sim(
        input,
        &runtime.simulation,
        Some(&runtime.resources.rules),
        &logic_selects(&output.lifecycle_outputs),
    );
}

fn deploy(runtime: &mut SimRuntime, input: &mut MatchInputState, original: u64) {
    let commands = deploy_key_commands(
        &runtime.simulation,
        Some(&runtime.resources.rules),
        &input.selection_order,
        runtime.simulation.session().current_house(),
    );
    assert!(
        matches!(commands.as_slice(), [Command::DeployMcv { entity_id }] if *entity_id == original)
    );
    frame(runtime, input, commands);
}

fn converted(
    runtime: &mut SimRuntime,
    input: &mut MatchInputState,
    original: u64,
    type_id: &str,
) -> u64 {
    for _ in 0..600 {
        if runtime.simulation.entities().get(original).is_none() {
            let replacement = runtime
                .simulation
                .entities()
                .values()
                .find(|entity| {
                    runtime
                        .simulation
                        .interner()
                        .object_type(entity.type_ref(), &runtime.resources.rules)
                        .is_some_and(|object| object.id == type_id)
                })
                .expect("conversion constructed its new object");
            assert!(replacement.lifecycle.object_alive());
            assert!(
                !replacement.in_limbo(),
                "successful Unlimbo precedes the native Group copy"
            );
            return replacement.stable_id();
        }
        frame(runtime, input, vec![]);
    }
    panic!("ordinary conversion did not retire source {original}");
}

fn assert_recall_and_saved_view(
    runtime: &mut SimRuntime,
    input: &mut MatchInputState,
    converted: u64,
    reserve: u64,
) {
    // Clear automatic conversion selection through an ordinary selection
    // command, so the group key must find and select the replacement itself.
    frame(
        runtime,
        input,
        vec![Command::Select {
            entity_ids: vec![reserve],
            additive: false,
        }],
    );
    let recall = team_selection(
        Some(&runtime.simulation),
        GroupPressAction::Recall,
        &runtime.simulation.projected_control_groups()[2],
        &input.selection_order,
    )
    .unwrap();
    assert!(recall.clear);
    assert_eq!(
        recall.select,
        [converted],
        "recall follows the native Group copy to the new object"
    );
    frame(
        runtime,
        input,
        vec![Command::Select {
            entity_ids: recall.select,
            additive: !recall.clear,
        }],
    );
    assert_eq!(input.selection_order, [converted]);
    assert_eq!(runtime.simulation.projected_control_groups()[9], [reserve]);
    let saved = input
        .capture_saved_local_view(
            &runtime.simulation,
            &runtime.resources.rules,
            &SidebarGadgetState::new(),
        )
        .unwrap();
    let saved =
        crate::util::save_codec::deserialize(&crate::util::save_codec::serialize(&saved).unwrap())
            .unwrap();
    let mut restored = MatchInputState::new(Default::default());
    PreparedSavedLocalView::new(saved, &runtime.simulation, &runtime.resources.rules)
        .unwrap()
        .commit_into(&mut restored, &mut SidebarGadgetState::new());
    assert_eq!(restored.selection_order, [converted]);
    assert_eq!(
        team_walk_members(
            Some(&runtime.simulation),
            &runtime.simulation.projected_control_groups()[2]
        ),
        [converted]
    );
    assert_eq!(runtime.simulation.projected_control_groups()[9], [reserve]);
}

#[test]
fn mcv_deploy_preserves_group_and_refused_deploy_keeps_original_member() {
    let Some((mut blocked, mut blocked_input, original, reserve)) = scene(false, true) else {
        return;
    };
    deploy(&mut blocked, &mut blocked_input, original);
    for _ in 0..120 {
        frame(&mut blocked, &mut blocked_input, vec![]);
    }
    assert!(blocked.simulation.entities().get(original).is_some());
    assert_eq!(
        blocked.simulation.projected_control_groups()[2],
        [original],
        "refusal does not group a discarded constructor"
    );
    assert!(!blocked.simulation.entities().values().any(|entity| {
        blocked
            .simulation
            .interner()
            .object_type(entity.type_ref(), &blocked.resources.rules)
            .is_some_and(|object| object.id == "GACNST")
    }));
    assert_recall_and_saved_view(&mut blocked, &mut blocked_input, original, reserve);

    let (mut runtime, mut input, original, reserve) = scene(false, false).unwrap();
    deploy(&mut runtime, &mut input, original);
    let yard = converted(&mut runtime, &mut input, original, "GACNST");
    assert_ne!(yard, original);
    assert_recall_and_saved_view(&mut runtime, &mut input, yard, reserve);
    // Continue the genuinely deployed yard through its stock buildup and
    // the ordinary ground-click repack, to a third stable identity.
    for _ in 0..600 {
        if !runtime
            .simulation
            .entities()
            .get(yard)
            .unwrap()
            .building_up()
        {
            break;
        }
        frame(&mut runtime, &mut input, vec![]);
    }
    assert!(
        !runtime
            .simulation
            .entities()
            .get(yard)
            .unwrap()
            .building_up()
    );
    let unit = repack(&mut runtime, &mut input, yard);
    assert_ne!(unit, original);
    assert_recall_and_saved_view(&mut runtime, &mut input, unit, reserve);
}

fn repack(runtime: &mut SimRuntime, input: &mut MatchInputState, original: u64) -> u64 {
    let owner = runtime.simulation.session().current_house().unwrap();
    let (commands, _) = crate::app::input::context_order::building_cell_orders(
        &runtime.simulation,
        &runtime.resources.rules,
        Some(&runtime.resources.overlay_registry),
        &input.selection_order,
        owner,
        (26, 20),
        false,
        false,
    );
    assert!(
        matches!(commands.as_slice(), [Command::SetRally { .. }, Command::SellBuilding { entity_id }] if *entity_id == original)
    );
    frame(runtime, input, commands);
    let unit = converted(runtime, input, original, "AMCV");
    assert_ne!(unit, original);
    unit
}

#[test]
fn yard_ground_click_preserves_group_through_repacking() {
    let Some((mut runtime, mut input, original, reserve)) = scene(true, false) else {
        return;
    };
    let unit = repack(&mut runtime, &mut input, original);
    assert_recall_and_saved_view(&mut runtime, &mut input, unit, reserve);
}
