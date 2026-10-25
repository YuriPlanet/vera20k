//! Instruction-established Execute31/HasOccurred53/54 production regressions.
//! Steam gamemd; no whole-map/native rendering claim.
use super::tests::{event_ingress_fixture_with_rules, install_event_ingress_map};
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::UninitContext;

fn fixture() -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    [InternedId; 3],
) {
    // The shared fixture appends this inside its TAGUNIT section. Supply
    // Sight there; a second [TAGUNIT]/[VehicleTypes] body is not a layer.
    event_ingress_fixture_with_rules(
        "Sight=1\n[General]\nAlliedDisguise=E1\nSovietDisguise=E2\nThirdDisguise=INIT\n\
         [InfantryTypes]\n2=SPY\n3=E1\n4=E2\n5=INIT\n\
         [SPY]\nStrength=100\nSpeed=4\nSpeedType=Foot\nAgent=yes\nInfiltrate=yes\nCanDisguise=yes\nPermaDisguise=yes\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [E1]\nStrength=100\n[E2]\nStrength=100\n[INIT]\nStrength=100\n",
    )
}

fn install(sim: &mut Simulation, rules: &RuleSet, event_record: &str) -> usize {
    install_event_ingress_map(
        sim,
        rules,
        &format!(
            "[Triggers]\nSPY=Country0,<none>,Spy,0,1,1,1,0\n\
         [Events]\nSPY={event_record}\n\
         [Actions]\nSPY=1,28,0,7,0,0,0,0,A\n\
         [Tags]\nSPYTAG=0,Spy,SPY\n[CellTags]\n15015=SPYTAG\n"
        ),
    );
    let typ = sim.trigger_runtime.definitions.tag_type("SPYTAG").unwrap();
    let tag = sim.shared_trigger_tag(typ);
    sim.trigger_runtime.tag(tag).unwrap().triggers[0]
}

/// `TechnoClass::Init_Managers` (`0x006F41FE..0x006F4271`) dresses a
/// PermaDisguise spy in its owner side's Rules disguise from construction.
/// Cases about the current player's side fallback take that identity off.
fn without_stored_disguise(sim: &mut Simulation, actor: u64) {
    let entity = sim.substrate.entities.get_mut(actor).unwrap();
    entity.disguise.as_mut().unwrap().clear_unit();
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

fn native_spy_entry() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_spy_entry_events.json")
}

fn native_arrival_kinds(name: &str) -> Vec<i32> {
    native_spy_entry()["arrival_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap()["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|call| call["kind"].as_i64().unwrap() as i32)
        .collect()
}

#[test]
fn spy_house_event_reads_forced_display_owner_and_first_country_house() {
    // Original HasOccurred53 controls: duplicate-Country House registration,
    // forced worn identity/PlayerPtr fallback, admission and unchanged state.
    for row in native_spy_entry()["predicate_controls"].as_array().unwrap() {
        let (mut sim, rules, _, houses) = fixture();
        let order: Vec<_> = row["inputs"]["house_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| houses[index.as_u64().unwrap() as usize])
            .collect();
        sim.houses.reorder_for_test(&order);
        sim.session.set_current_house_for_test(
            row["current_player"]
                .as_u64()
                .map(|index| houses[index as usize]),
        );
        let actor = sim
            .spawn_object("SPY", "House0", 15, 15, 0, &rules)
            .unwrap();
        let live = install(&mut sim, &rules, row["raw"].as_str().unwrap());
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[typ].events[0].retained_house =
            Some(houses[2]);
        let event = sim.trigger_runtime.definitions.trigger_types[typ].events[0].clone();
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let disguise = sim
            .substrate
            .entities
            .get_mut(actor)
            .unwrap()
            .disguise
            .as_mut()
            .unwrap();
        disguise.acquire(
            0,
            None,
            row["display_house"]
                .as_u64()
                .map(|index| houses[index as usize]),
        );
        if !row["raw_disguised"].as_bool().unwrap() {
            disguise.receive_damage_reveal(0, 1, crate::map::entities::EntityCategory::Infantry);
            assert!(!disguise.is_disguised());
        }
        let rng_before = rng(&sim);
        let mut persistence = row["initial_persistence"].as_u64().unwrap() != 0;
        let input = TagEventInput {
            kind: row["inputs"]["raised"].as_i64().unwrap() as i32,
            actor: row["inputs"]["actor_owner"].as_u64().map(|_| actor),
            ..Default::default()
        };
        let incoming_house = row["inputs"]["incoming_house"]
            .as_u64()
            .map(|index| houses[index as usize]);
        let mut call = |sim: &mut Simulation| {
            sim.has_occurred(0, input, incoming_house, &mut persistence, &rules, live)
        };
        let actual = if row["inputs"]["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(call)
        } else {
            call(&mut sim)
        };
        assert_eq!(
            actual,
            row["result"].as_u64().unwrap() != 0,
            "{}",
            row["name"]
        );
        assert_eq!(
            persistence,
            row["persistence"].as_u64().unwrap() != 0,
            "{}",
            row["name"]
        );
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[typ].events[0],
            event
        );
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        assert_eq!(rng(&sim), rng_before);
    }
}

#[test]
fn agent_arrival_keeps_the_cached_tag_for_linked_house_then_type_events() {
    let (mut sim, rules, registry, houses) = fixture();
    sim.session.set_current_house_for_test(Some(houses[1]));
    let actor = sim
        .spawn_object("SPY", "House0", 15, 15, 0, &rules)
        .unwrap();
    let value = rules
        .type_array_index(crate::rules::object_type::ObjectCategory::Infantry, "E1")
        .unwrap();
    install_event_ingress_map(
        &mut sim,
        &rules,
        &format!(
            "[Triggers]\nHOUSE=Country0,TYPE,House,0,1,1,1,0\nTYPE=Country0,<none>,Type,0,1,1,1,0\n\
             [Events]\nHOUSE=1,53,0,0\nTYPE=1,54,0,{value}\n\
             [Actions]\nHOUSE=1,56,0,7,0,0,0,0,A\nTYPE=1,56,0,8,0,0,0,0,A\n\
             [Tags]\nSPYTAG=0,Spy,HOUSE\n[CellTags]\n15015=SPYTAG\n"
        ),
    );
    let tag = *sim.trigger_runtime.cell_tags.values().next().unwrap();
    let lives = sim.trigger_runtime.tag(tag).unwrap().triggers.clone();
    let before = rng(&sim);
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        &rules,
        Some(&registry),
        FrameEffects::default(),
    )
    .unwrap();
    let native = native_spy_entry();
    let linked = &native["linked_history"];
    let after54 = &linked["history"][linked["checkpoints"]["after54"].as_u64().unwrap() as usize];
    assert_eq!(
        serde_json::json!(sim.trigger_runtime.locals_set),
        after54["locals"]
    );
    assert_eq!(
        sim.substrate.pending_delete.retained_tickets().count(),
        after54["pending_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        sim.tag_dispatch_trace_for_test()
            .iter()
            .map(|(_, kind)| *kind)
            .collect::<Vec<_>>(),
        native_arrival_kinds("none")
    );
    assert!(sim.trigger_runtime.cell_tags.is_empty());
    assert!(!sim.trigger_runtime.tag(tag).unwrap().pending);
    assert!(
        lives
            .iter()
            .all(|&live| sim.trigger_runtime.trigger(live).unwrap().pending)
    );
    assert_eq!(rng(&sim), before);
    sim.flush_pending_delete();
    assert!(sim.trigger_runtime.tag(tag).is_none());
    assert!(
        lives
            .iter()
            .all(|&live| sim.trigger_runtime.trigger(live).is_none())
    );
}

#[test]
fn retail_spy_house_entry_survives_restore_and_reaches_the_message_consumer() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let spy = rules.object("SPY").unwrap();
    assert!(spy.agent && spy.perma_disguise);
    let (mut sim, _, registry, houses) = fixture();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    // Execute_DoList64C4FB admits this player Move, including after load.
    // House0 sends it; displayed House1 stays separate for Spy identity.
    let owner = sim.houses.get_mut(&houses[0]).unwrap();
    owner.set_is_human_for_test(true);
    owner.set_player_control_for_test(true);
    for (index, name) in ["Americans", "Russians", "Americans"].iter().enumerate() {
        let country = sim.interner.intern(name);
        sim.houses
            .get_mut(&houses[index])
            .unwrap()
            .set_country_for_test(Some(country));
    }
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .set_is_human_for_test(true);
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .set_player_control_for_test(true);
    sim.session.set_current_house_for_test(Some(houses[1]));
    let actor = sim
        .spawn_object("SPY", "House0", 16, 15, 0, &rules)
        .unwrap();
    let country = rules.country_index("Americans").unwrap().0;
    install_event_ingress_map(
        &mut sim,
        &rules,
        &format!(
            "[Triggers]\nSPY=Americans,<none>,Spy,0,1,1,1,0\nMESSAGE=Americans,<none>,Message,0,1,1,1,0\n\
         [Events]\nSPY=1,53,0,{country}\nMESSAGE=1,36,0,7\n\
         [Actions]\nSPY=1,56,0,7,0,0,0,0,A\nMESSAGE=1,11,4,SpyEntry,0,0,0,0,A\n\
         [Tags]\nSPYTAG=0,Spy,SPY\nMESSAGE_TAG=0,Message,MESSAGE\n[CellTags]\n15015=SPYTAG\n"
        ),
    );
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Spy house entry", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    loaded.reconstruct_cellclass_dummy_for_map_resize();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .unwrap();
    loaded.resolve_type_handles(&rules);
    let mut histories = Vec::new();
    for candidate in [&mut sim, &mut loaded] {
        assert!(!candidate.trigger_local_is_set_for_test(7));
        assert_eq!(
            candidate
                .substrate
                .entities
                .get(actor)
                .unwrap()
                .position
                .rx(),
            16
        );
        let order = crate::sim::command::CommandEnvelope::new(
            houses[0],
            candidate.session.tick() + 1,
            crate::sim::command::Command::Move {
                entity_id: actor,
                target_rx: 15,
                target_ry: 15,
            },
        );
        let mut commands = vec![order];
        let mut messages = Vec::new();
        let mut arrival_frame = None;
        for _ in 0..160 {
            let output = candidate
                .advance_app_frame(
                    &std::mem::take(&mut commands),
                    &rules,
                    Some(&registry),
                    67,
                    crate::sim::world::TickLane::Ordinary,
                    FrameEffects::default(),
                )
                .unwrap();
            assert!(output.tick.frame_committed);
            let spy = candidate.substrate.entities.get(actor).unwrap();
            if candidate.trigger_local_is_set_for_test(7) && arrival_frame.is_none() {
                assert_eq!((spy.position.rx(), spy.position.ry()), (15, 15));
                arrival_frame = Some(output.tick.tick);
            }
            for effect in output.trigger_effects {
                if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                    messages.push((output.tick.tick, csf_label));
                }
            }
        }
        assert!(
            arrival_frame.is_some(),
            "the real Walk must reach the tagged cell"
        );
        assert_eq!(
            messages.len(),
            1,
            "one-shot cell entry must emit one message"
        );
        assert_eq!(messages[0].1, "SpyEntry");
        assert!(messages[0].0 >= arrival_frame.unwrap());
        assert!(candidate.trigger_runtime.cell_tags.is_empty());
        histories.push((arrival_frame, messages));
    }
    assert_eq!(histories[0], histories[1]);
    assert_eq!(sim.trigger_runtime, loaded.trigger_runtime);
}

#[test]
fn forced_defaults_and_makeup_kit_copy_use_the_same_retained_type_owner() {
    let (mut sim, rules, _, houses) = fixture();
    let actor = sim
        .spawn_object("SPY", "House0", 15, 15, 0, &rules)
        .unwrap();
    without_stored_disguise(&mut sim, actor);
    for (side, name) in [
        (Some(0), "E1"),
        (Some(1), "E2"),
        (Some(2), "INIT"),
        (Some(9), "INIT"),
        (Some(-1), "INIT"),
        (None, "INIT"),
    ] {
        sim.session.set_current_house_for_test(side.map(|side| {
            sim.houses
                .get_mut(&houses[2])
                .unwrap()
                .set_side_index_for_test(side);
            houses[2]
        }));
        assert_eq!(
            sim.forced_disguise_type(actor, &rules),
            sim.interner.get(name)
        );
    }
    sim.session.set_current_house_for_test(Some(houses[1]));
    let target = sim
        .spawn_object("SPY", "House0", 16, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .disguise
        .as_mut()
        .unwrap()
        .acquire(0, None, None);
    sim.disguise_as(actor, Some(target), true, &rules);
    let disguise = sim
        .substrate
        .entities
        .get(actor)
        .unwrap()
        .disguise
        .as_ref()
        .unwrap();
    assert_eq!(disguise.type_id(), sim.interner.get("E2"));
    assert_eq!(disguise.house(), Some(houses[1]));
}

#[test]
fn non_agent_arrival_does_not_raise_the_spy_cell_event() {
    let (mut sim, rules, registry, houses) = fixture();
    sim.session.set_current_house_for_test(Some(houses[0]));
    let actor = sim
        .spawn_object("ENGINEER", "House0", 15, 15, 0, &rules)
        .unwrap();
    install(&mut sim, &rules, "1,53,0,0");
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        &rules,
        Some(&registry),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(!sim.trigger_runtime.globals_set.contains(&7));
    assert_eq!(
        sim.tag_dispatch_trace_for_test()
            .iter()
            .map(|(_, kind)| *kind)
            .collect::<Vec<_>>(),
        native_arrival_kinds("non_agent")
    );
}

#[test]
fn spy_type_event_uses_forced_identity_side_fallback_and_native_persistence_gate() {
    use crate::map::entities::EntityCategory;
    use crate::rules::object_type::ObjectCategory;
    for (side, stored, raised, bypass, prior, expected) in [
        (Some(0), None, 54, false, false, true),
        (Some(1), None, 54, false, false, false),
        (Some(2), None, 54, false, false, false),
        (Some(9), None, 54, false, true, false),
        (None, None, 54, false, false, false),
        (Some(1), Some("E1"), 54, false, false, true),
        (Some(0), Some("E2"), 54, false, true, false),
        (Some(0), Some("E1"), 53, false, false, false),
        (Some(0), Some("E1"), 54, true, false, false),
    ] {
        let (mut sim, rules, _, houses) = fixture();
        sim.session.set_current_house_for_test(side.map(|side| {
            sim.houses
                .get_mut(&houses[2])
                .unwrap()
                .set_side_index_for_test(side);
            houses[2]
        }));
        let actor = sim
            .spawn_object("SPY", "House0", 15, 15, 0, &rules)
            .unwrap();
        let expected_index = rules
            .type_array_index(ObjectCategory::Infantry, "E1")
            .unwrap();
        let live = install(&mut sim, &rules, &format!("1,54,0,{expected_index}"));
        if let Some(name) = stored {
            let typ = sim.interner.get(name).unwrap();
            let entity = sim.substrate.entities.get_mut(actor).unwrap();
            let disguise = entity.disguise.as_mut().unwrap();
            disguise.acquire(0, Some(typ), None);
            disguise.receive_damage_reveal(0, 1, EntityCategory::Infantry);
            assert!(!disguise.is_disguised(), "GetDisguise(1) ignores the flag");
        } else {
            without_stored_disguise(&mut sim, actor);
        }
        let mut persistence = prior;
        let before = rng(&sim);
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let mut call = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: raised,
                    actor: Some(actor),
                    ..Default::default()
                },
                None,
                &mut persistence,
                &rules,
                live,
            )
        };
        let actual = if bypass {
            sim.with_buildup_bypass(call)
        } else {
            call(&mut sim)
        };
        assert_eq!(
            actual, expected,
            "side{side:?} stored{stored:?} raised{raised} bypass{bypass}"
        );
        assert_eq!(persistence, prior || expected);
        assert_eq!(rng(&sim), before);
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        let mut persistence = false;
        assert!(!sim.has_occurred(
            0,
            TagEventInput {
                kind: 54,
                ..Default::default()
            },
            None,
            &mut persistence,
            &rules,
            live
        ));
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[typ].events[0]
            .condition
            .value = -1;
        assert!(!sim.has_occurred(
            0,
            TagEventInput {
                kind: 54,
                actor: Some(actor),
                ..Default::default()
            },
            None,
            &mut persistence,
            &rules,
            live
        ));
    }
}

#[test]
fn agent_arrival_delivers_cell_spy_event_after_snapshot_and_cleans_up_one_shot() {
    let (mut sim, rules, registry, houses) = fixture();
    // The current player is side 1, but the spy wears its owner's side-0
    // disguise (E1) from construction, and GetDisguise(1) answers that.
    sim.session.set_current_house_for_test(Some(houses[1]));
    let actor = sim
        .spawn_object("SPY", "House0", 15, 15, 0, &rules)
        .unwrap();
    let value = rules
        .type_array_index(crate::rules::object_type::ObjectCategory::Infantry, "E1")
        .unwrap();
    let live = install(&mut sim, &rules, &format!("1,54,0,{value}"));
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Spy CellTag", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    loaded.reconstruct_cellclass_dummy_for_map_resize();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    loaded.resolve_type_handles(&rules);
    for sim in [&mut sim, &mut loaded] {
        let before = rng(sim);
        sim.per_cell_process(
            actor,
            crate::sim::movement::PerCellReason::TurnComplete,
            &rules,
            Some(&registry),
            FrameEffects::default(),
        )
        .unwrap();
        assert!(!sim.trigger_runtime.globals_set.contains(&7));
        sim.per_cell_process(
            actor,
            crate::sim::movement::PerCellReason::Arrival,
            &rules,
            Some(&registry),
            FrameEffects::default(),
        )
        .unwrap();
        assert!(sim.trigger_runtime.globals_set.contains(&7));
        assert_eq!(rng(sim), before);
        assert!(
            sim.substrate.entities.contains(actor),
            "crossing a CellTag does not consume the spy"
        );
        sim.flush_pending_delete();
        assert!(sim.trigger_runtime.trigger(live).is_none());
    }
}

#[test]
fn grow_action_uses_current_map_then_known_objects_without_resetting_house_flags() {
    let (mut sim, rules, registry, houses) = fixture();
    sim.fog = crate::sim::vision::FogState::new(33, 33);
    sim.session.set_current_house_for_test(Some(houses[0]));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let known = sim
        .spawn_object("TAGUNIT", "House0", 14, 15, 0, &rules)
        .unwrap();
    let unknown = sim
        .spawn_object("TAGUNIT", "House0", 23, 15, 0, &rules)
        .unwrap();
    let air = sim
        .spawn_object("TAGUNIT", "House0", 23, 23, 0, &rules)
        .unwrap();
    // Supply the AIR registration boundary. This check is about the map's
    // display cursor, not an emulated aircraft/locomotor transition.
    crate::sim::world::Simulation::submit_object_display(
        &sim.interner,
        &sim.terrain_objects,
        &mut sim.substrate,
        air,
        crate::sim::world::display_layers::DisplayLayer::AIR,
        &rules,
    );
    sim.substrate
        .entities
        .get_mut(air)
        .unwrap()
        .discovery
        .set_discovered_by_current_house_for_test(true);
    sim.substrate
        .entities
        .get_mut(known)
        .unwrap()
        .discovery
        .set_discovered_by_current_house_for_test(true);
    sim.substrate
        .entities
        .get_mut(unknown)
        .unwrap()
        .discovery
        .set_discovered_by_current_house_for_test(false);
    for cell in [(14, 15), (23, 15), (23, 23), (8, 11)] {
        sim.fog
            .set_ground_bits_for_test(houses[0], cell.0, cell.1, 8);
    }
    sim.fog.set_ground_bits_for_test(houses[1], 8, 11, 24);
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .set_map_is_clear_for_test(true);
    let rng_before = rng(&sim);
    sim.grow_shroud(UninitContext::new(&rules, Some(&registry)));
    assert!(sim.fog.is_ground_unshrouded(houses[0], 14, 15));
    assert!(!sim.fog.is_ground_unshrouded(houses[0], 23, 15));
    assert!(!sim.fog.is_ground_unshrouded(houses[0], 23, 23));
    assert!(!sim.fog.is_ground_unshrouded(houses[0], 8, 11));
    assert!(sim.fog.is_ground_open(houses[1], 8, 11));
    assert!(sim.houses[&houses[0]].map_is_clear());
    assert_eq!(rng(&sim), rng_before);
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nGROW=Country7,<none>,Grow,0,1,1,1,0\n[Events]\nGROW=1,8,0,0\n[Actions]\nGROW=2,31,0,0,0,0,0,0,A,28,0,7,0,0,0,0,A\n[Tags]\nGROWTAG=2,Grow,GROW\n",
    );
    sim.fog.set_ground_bits_for_test(houses[0], 8, 11, 8);
    sim.session.set_binary_frame_for_test(8);
    sim.raise_house_tags(houses[1], &rules, Some(&registry), FrameEffects::default());
    assert!(!sim.fog.is_ground_unshrouded(houses[0], 8, 11));
    assert!(sim.trigger_runtime.globals_set.contains(&7));
}
