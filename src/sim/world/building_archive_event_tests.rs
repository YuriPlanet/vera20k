//! ARCHIVE1E: SetRally443860 -> Event4C6DAA -> naval ExitObject44441A.
//! Retail naval types/assets; supplied arena, Houses and factory-step timing.
use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope, CommandRecord};
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::production::{self, ProductionCategory};
use crate::sim::runtime::{SimResources, SimRuntime};

fn archive_record(actor: u64, target: i32, kind: u8) -> CommandRecord {
    let mut payload = [0; 10];
    payload[..4].copy_from_slice(&i32::try_from(actor).unwrap().to_le_bytes());
    payload[4] = 0x34;
    payload[5..9].copy_from_slice(&target.to_le_bytes());
    payload[9] = kind;
    CommandRecord::encode(0x1e, 0, 0, &payload).unwrap()
}

fn land_scene() -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let (mut sim, rules, registry) = entry_test_fixture::fixture_with_rules(
        "[BuildingTypes]\n1=ARCHIVER\n[ARCHIVER]\nStrength=100\nFoundation=1x1\nCapturable=yes\n\
         [VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\nSpeed=4\nSpeedType=Track\n\
         MovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    );
    for name in ["Americans", "Russians"] {
        let owner = sim.intern(name);
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 5000, 10));
    }
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    (sim, rules, registry)
}

#[test]
fn naval_default_uses_current_frame_wire_and_reaches_a_paid_ship_exit() {
    let Some(mut retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
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
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    crate::sim::arena_fixture::flood(
        &mut sim,
        &rules,
        32,
        (8..32).flat_map(|y| (8..32).map(move |x| (x, y))),
    );
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.get("Americans").unwrap();
    sim.session.set_current_house_for_test(Some(owner));
    sim.session.set_binary_frame_for_test(100);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .add_credits(50_000);
    // Power and a surviving rival keep the ordinary frame out of unrelated holds.
    sim.spawn_object_with_overlay_registry("GAPOWR", "Americans", 4, 4, 0, &rules, &registry)
        .unwrap();
    sim.spawn_object_with_overlay_registry("DEST", "Russians", 26, 26, 0, &rules, &registry)
        .unwrap();
    let yard = sim
        .spawn_object_with_overlay_registry("GAYARD", "Americans", 12, 12, 0, &rules, &registry)
        .unwrap();
    // The finished-building adapter calls the real GrandOpening naval producer.
    assert_eq!(sim.entities().get(yard).unwrap().archive_target(), None);
    let destination = {
        let queues = sim.network_command_queues();
        let records = queues
            .local_records()
            .expect("naval defaults need native ARCHIVE bytes");
        assert_eq!(records.len(), 1);
        let event = records[0];
        assert_eq!(
            (event.opcode(), event.house_id(), event.frame_stamp()),
            (0x1e, 1, 100)
        );
        assert_eq!(
            &event.payload()[..4],
            &i32::try_from(yard).unwrap().to_le_bytes()
        );
        assert_eq!((event.payload()[4], event.payload()[9]), (0x34, 0x0b));
        let cell = i32::from_le_bytes(event.payload()[5..9].try_into().unwrap());
        ((cell % 1000) as u16, (cell / 1000) as u16)
    };
    let terrain_template = sim.resolved_terrain.clone();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources {
            rules,
            overlay_registry: registry,
            terrain_template,
        },
    };
    runtime
        .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    let sim = &mut runtime.simulation;
    let rules = &runtime.resources.rules;
    assert_eq!(sim.session.binary_frame(), 101);
    assert_eq!(
        sim.entities().get(yard).unwrap().rally_cell(),
        Some(destination)
    );
    assert!(
        sim.pending_command_snapshot().is_empty(),
        "dispatch belongs to tail100"
    );
    assert!(production::enqueue_by_type(
        sim,
        rules,
        "Americans",
        "DEST",
        FrameEffects::default(),
        false
    ));
    let ship = sim
        .production
        .factories()
        .view(owner, ProductionCategory::Ship)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();
    let credits = sim.houses[&owner].economy().credits();
    // Supply only elapsed step timing: real Factory steps charge the real wallet.
    let economy = sim.houses.get_mut(&owner).unwrap().economy_mut();
    let factory = sim
        .production
        .factories_mut()
        .test_factory_mut(owner, ProductionCategory::Ship)
        .unwrap();
    for _ in 0..production::PRODUCTION_STEPS {
        factory.advance_one_step(economy);
    }
    assert!(sim.houses[&owner].economy().credits() < credits);
    assert!(production::dispatch_production_changes_for_tests(
        sim,
        rules,
        Some(&runtime.resources.overlay_registry)
    ));
    let ship = sim.entities().get(ship).unwrap();
    assert!(!ship.lifecycle.in_limbo());
    assert_eq!(
        ship.navigation.nav_com(),
        Some(NavTargetRef::cell(destination.0, destination.1))
    );
    assert_eq!(
        ship.mission.queued(),
        MissionId::from_known(MissionType::Move)
    );
}

#[test]
fn accepted_object_archive_survives_capture_but_the_old_owners_sell_does_not() {
    let (mut sim, rules, registry) = land_scene();
    let old = sim.interner.get("Americans").unwrap();
    let new = sim.interner.get("Russians").unwrap();
    let actor = sim
        .spawn_object_with_overlay_registry("ARCHIVER", "Americans", 15, 15, 0, &rules, &registry)
        .unwrap();
    let target = sim
        .spawn_object_with_overlay_registry("TEST", "Americans", 17, 14, 0, &rules, &registry)
        .unwrap();
    assert!(sim.issue_local_command(CommandEnvelope::new(
        old,
        1,
        Command::ArchiveObject {
            entity_id: actor,
            target_id: target
        }
    )));
    sim.change_owner_with_rules(actor, new, &rules, Some(&registry), FrameEffects::default());
    assert_eq!(sim.entities().get(actor).unwrap().owner(), new);
    assert_eq!(sim.entities().get(actor).unwrap().archive_target(), None);
    let mut accepted = sim.take_frame_commands_for_test();
    assert_eq!(accepted.len(), 1);
    let command = accepted.pop().unwrap();
    assert_eq!(command.owner, old);
    assert!(sim.apply_command("Americans", &command.payload, Some(&rules)));
    assert_eq!(
        sim.entities().get(actor).unwrap().archive_target(),
        Some(TargetKind::Entity(target))
    );
    // SELL4C6F45 retains its current-owner gate; no SELL wire/LAN claim.
    assert!(!sim.apply_command(
        "Americans",
        &Command::SellBuilding { entity_id: actor },
        Some(&rules)
    ));
    assert_eq!(sim.entities().get(actor).unwrap().owner(), new);
}

#[test]
fn dead_resolved_archive_actor_loses_only_its_plan_without_resolving_the_cell() {
    let (mut sim, rules, registry) = land_scene();
    let owner = sim.interner.get("Americans").unwrap();
    let actor = sim
        .spawn_object_with_overlay_registry("TEST", "Americans", 15, 15, 0, &rules, &registry)
        .unwrap();
    let survivor = sim
        .spawn_object_with_overlay_registry("TEST", "Americans", 17, 14, 0, &rules, &registry)
        .unwrap();
    for id in [actor, survivor] {
        let plan = sim
            .encode_planning_order_record(
                owner,
                &Command::Move {
                    entity_id: id,
                    target_rx: 17,
                    target_ry: 16,
                },
            )
            .unwrap();
        let command = sim.decode_native_command_record(&plan, 0).unwrap();
        assert!(sim.apply_command("Americans", &command.payload, Some(&rules)));
    }
    let graph = sim.planning_draw_graph(owner, &[], None, None);
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.nodes[0].members, [actor, survivor]);
    let before = sim
        .planning_paths(owner)
        .into_iter()
        .find(|path| path.actor_id == survivor)
        .unwrap();
    let entity = sim.substrate.entities.get_mut(actor).unwrap();
    entity.set_archive_target(Some(TargetKind::Cell(15, 15)));
    entity.lifecycle.set_object_alive_for_test(false);
    let dummy = sim.effective_shared_cell_dummy().snapshot();
    // Resolved dead Techno is a supplied receiver prior. Off-map Cell200,200
    // must never resolve/stamp Dummy after Event4C6DBD refuses Alive.
    let record = archive_record(actor, 200_200, 0x0b);
    let command = sim
        .decode_native_command_record(&record, 0)
        .expect("ARCHIVE must reach its receiver");
    assert!(!sim.apply_command("Americans", &command.payload, Some(&rules)));
    assert_eq!(sim.effective_shared_cell_dummy().snapshot(), dummy);
    assert_eq!(
        sim.entities().get(actor).unwrap().archive_target(),
        Some(TargetKind::Cell(15, 15))
    );
    let remaining = sim.planning_paths(owner);
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        (
            remaining[0].actor_id,
            &remaining[0].cells,
            remaining[0].loop_start,
            remaining[0].editing
        ),
        (survivor, &before.cells, before.loop_start, before.editing)
    );
    let graph = sim.planning_draw_graph(owner, &[], None, None);
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.nodes[0].members, [survivor]);
    sim.validate_planning_references().unwrap();
}
