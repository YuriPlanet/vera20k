//! Native Group+214 copies7397CC/44A052 after successful MCV conversion.
//! Physical stock rules/ART/SHP; clear arena, Houses and capture are priors.
use super::*;
use crate::app::input::state::MatchInputState;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::command::Command;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, TickLane};

fn scene(source_type: &str) -> Option<(SimRuntime, MatchInputState, u64, [u64; 2])> {
    let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
    retail
        .rules
        .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_building_buildup_assets(&assets, "TEMPERATE");
    let [_, frames, rate] = retail.rules.buildup_control("GACNST");
    assert!(retail.rules.has_buildup("GACNST") && frames > 1 && rate > 0);
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x7397cc);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    let rival = sim.interner().get("Russians").unwrap();
    sim.houses_mut_for_test()
        .get_mut(&rival)
        .unwrap()
        .set_is_human_for_test(true);
    sim.houses_mut_for_test()
        .get_mut(&rival)
        .unwrap()
        .set_current_iq_for_test(0);
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .mcv_redeploy = true;
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .short_game = false;
    sim.spawn_object("GAPOWR", "Russians", 3, 3, 0, &rules)
        .unwrap();
    let unrelated = sim
        .spawn_object("GAPOWR", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let source = sim
        .spawn_object(source_type, "Americans", 12, 12, 96, &rules)
        .unwrap();
    let peers = [
        sim.spawn_object("AMCV", "Americans", 4, 8, 0, &rules)
            .unwrap(),
        sim.spawn_object("AMCV", "Americans", 6, 8, 0, &rules)
            .unwrap(),
    ];
    let mut input = MatchInputState::new(Default::default());
    // Exercise the existing assignment owner, including its one-group rule.
    let _ = sim.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: vec![unrelated],
            additive: false,
        },
        Some(&rules),
    );
    reassign_control_group(&mut input.control_groups, 1, &[unrelated], &sim);
    let _ = sim.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: vec![source, peers[0], peers[1]],
            additive: false,
        },
        Some(&rules),
    );
    reconcile_selection_order_for_sim(&mut input, &sim, Some(&rules), &[]);
    reassign_control_group(&mut input.control_groups, 0, &input.selection_order, &sim);
    assert_eq!(input.control_groups[0], [source, peers[0], peers[1]]);
    let terrain_template = sim.resolved_terrain().cloned();
    Some((
        SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                overlay_registry: registry,
                terrain_template,
            },
        },
        input,
        source,
        peers,
    ))
}

fn frame(runtime: &mut SimRuntime, input: &mut MatchInputState) {
    let output = runtime
        .advance_frame(&[], 33, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    // Same ordered lifecycle consumer as app post-sim reconciliation, before
    // projection can discard the retired source identity.
    apply_control_group_conversions(&mut input.control_groups, &output.lifecycle_outputs);
    reconcile_selection_order_for_sim(
        input,
        &runtime.simulation,
        Some(&runtime.resources.rules),
        &logic_selects(&output.lifecycle_outputs),
    );
}

fn visits(runtime: &SimRuntime) -> u32 {
    let [_, frames, rate] = runtime.resources.rules.buildup_control("GACNST");
    u32::try_from(frames * rate).unwrap() * 4 + 240
}

fn sell(runtime: &mut SimRuntime, yard: u64, owner: &str) {
    // Existing semantic receiver/consumer owners; no new SELL/ARCHIVE wire API.
    assert!(runtime.simulation.apply_command(
        owner,
        &Command::SetRally {
            rx: 20,
            ry: 20,
            producer_ids: vec![yard]
        },
        Some(&runtime.resources.rules)
    ));
    assert!(crate::sim::production::can_sell_building(
        &runtime.simulation,
        &runtime.resources.rules,
        yard
    ));
    assert!(runtime.simulation.apply_command(
        owner,
        &Command::SellBuilding { entity_id: yard },
        Some(&runtime.resources.rules)
    ));
}

#[test]
fn stock_mcv_round_trip_appends_replacements_and_preserves_group_recall_and_save() {
    let Some((mut runtime, mut input, mcv, peers)) = scene("AMCV") else {
        return;
    };
    let untouched = input.control_groups[1].clone();
    assert!(
        runtime
            .simulation
            .entities()
            .get(mcv)
            .unwrap()
            .selection
            .is_selected()
    );
    assert!(runtime.simulation.apply_command(
        "Americans",
        &Command::DeployMcv { entity_id: mcv },
        Some(&runtime.resources.rules)
    ));
    let mut yard = None;
    for _ in 0..visits(&runtime) {
        frame(&mut runtime, &mut input);
        yard = runtime
            .simulation
            .entities()
            .values()
            .find(|entity| {
                runtime.simulation.interner().resolve(entity.type_ref()) == "GACNST"
                    && !entity.building_up()
            })
            .map(|entity| entity.stable_id());
        if yard.is_some() {
            break;
        }
    }
    let yard = yard.expect("ordinary deploy visits must finish the stock yard");
    assert!(runtime.simulation.entities().get(mcv).is_none());
    assert_eq!(
        input.control_groups[0],
        [peers[0], peers[1], yard],
        "fresh Techno appends"
    );
    assert_eq!(input.control_groups[1], untouched);
    let recalled = live_group_members(Some(&runtime.simulation), &input.control_groups[0]);
    assert_eq!(recalled, [peers[0], peers[1], yard]);
    let _ = runtime.simulation.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: recalled,
            additive: false,
        },
        Some(&runtime.resources.rules),
    );
    assert!(
        runtime
            .simulation
            .entities()
            .get(yard)
            .unwrap()
            .selection
            .is_selected()
    );
    sell(&mut runtime, yard, "Americans");
    let mut replacement = None;
    for _ in 0..visits(&runtime) {
        frame(&mut runtime, &mut input);
        replacement = runtime
            .simulation
            .entities()
            .values()
            .find(|entity| {
                runtime.simulation.interner().resolve(entity.type_ref()) == "AMCV"
                    && !peers.contains(&entity.stable_id())
            })
            .map(|entity| entity.stable_id());
        if replacement.is_some() {
            break;
        }
    }
    let replacement = replacement.expect("scheduled sale must return the stock AMCV");
    assert!(runtime.simulation.entities().get(yard).is_none());
    assert_eq!(input.control_groups[0], [peers[0], peers[1], replacement]);
    assert_eq!(input.control_groups[1], untouched);
    assert_eq!(
        live_group_members(Some(&runtime.simulation), &input.control_groups[0]),
        [peers[0], peers[1], replacement]
    );
    let saved = input
        .capture_saved_local_view(
            &runtime.simulation,
            &runtime.resources.rules,
            &crate::ui::sidebar::gadget_flash::SidebarGadgetState::new(),
        )
        .unwrap();
    assert_eq!(saved.control_groups()[0], [peers[0], peers[1], replacement]);
}

#[test]
fn captured_unselected_yard_preserves_latent_group_until_ownership_returns() {
    let Some((mut runtime, mut input, yard, peers)) = scene("GACNST") else {
        return;
    };
    let old = runtime.simulation.interner().get("Americans").unwrap();
    let captured = runtime.simulation.interner().get("Russians").unwrap();
    // Public baseline ownership adapter invokes the real chokepoint with empty
    // rules; these captures are supplied transitions, not Engineer-walk proof.
    runtime.simulation.change_owner(yard, captured);
    let _ = runtime.simulation.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: Vec::new(),
            additive: false,
        },
        Some(&runtime.resources.rules),
    );
    assert!(
        !runtime
            .simulation
            .entities()
            .get(yard)
            .unwrap()
            .selection
            .is_selected()
    );
    assert_eq!(
        live_group_members(Some(&runtime.simulation), &input.control_groups[0]),
        peers
    );
    sell(&mut runtime, yard, "Russians");
    let mut replacement = None;
    for _ in 0..visits(&runtime) {
        frame(&mut runtime, &mut input);
        replacement = runtime
            .simulation
            .entities()
            .values()
            .find(|entity| {
                runtime.simulation.interner().resolve(entity.type_ref()) == "AMCV"
                    && !peers.contains(&entity.stable_id())
            })
            .map(|entity| entity.stable_id());
        if replacement.is_some() {
            break;
        }
    }
    let replacement = replacement.expect("captured human-controlled yard must repack");
    assert!(runtime.simulation.entities().get(yard).is_none());
    let converted = runtime.simulation.entities().get(replacement).unwrap();
    assert_eq!(converted.owner(), captured);
    assert!(!converted.selection.is_selected());
    assert_eq!(input.control_groups[0], [peers[0], peers[1], replacement]);
    assert_eq!(
        live_group_members(Some(&runtime.simulation), &input.control_groups[0]),
        peers
    );
    runtime.simulation.change_owner(replacement, old);
    assert_eq!(
        live_group_members(Some(&runtime.simulation), &input.control_groups[0]),
        [peers[0], peers[1], replacement]
    );
}
