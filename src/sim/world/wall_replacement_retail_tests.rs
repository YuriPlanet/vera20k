//! Building 0x44065C replaces a same-ToOverlay owned wall through House 0x4FCE80
//! before new Mark. House 0x4FD018 runs the shared Cell 0x70D4A0 targeting sweep.
//! Original gamemd; ordinary paid production and MTNK/AP damage only.

use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::{FrameEffects, TickLane};

/// Original588750 creates a Building for each gap. Successful440580 wall
/// entry leaves its constructor Location unchanged and queues Overlay before
/// Building;725C70 removes them later. Executed native controls are produced
/// by tools/spatial_oracle/building_death_anims.py --wall-autofill.
#[test]
fn retail_paid_two_gap_wall_retains_constructor_identities_until_deferred_drain() {
    let Some((root, _assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let profile = crate::test_fixture::json("tools/map_observation.concrete-wall.example.json");
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    let map = launch.selected_map_file.as_deref().unwrap().to_owned();
    let mut scene = crate::headless_scenario::load_session(
        &root,
        &map,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .unwrap();
    scene.runtime.simulation.batch_path_searches();
    // Periodic frame hashes feed replay and sync checks, which nothing here
    // reads; an interval no tick reaches skips them (11-12% of each frame in
    // this unoptimized test build).
    scene
        .runtime
        .simulation
        .hash_state_every(std::num::NonZeroU64::MAX);
    let owner = scene.sim().session.current_house().unwrap();
    let owner_name = scene.sim().resolve(owner).to_owned();
    let wall_type = scene.sim().interner.get("GAWALL").unwrap();
    let orders: Vec<_> = profile["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            CommandEnvelope::new(
                owner,
                row["issue_after_step"].as_u64().unwrap(),
                serde_json::from_value::<Command>(row["payload"].clone()).unwrap(),
            )
        })
        .collect();
    let placements: Vec<_> = orders
        .iter()
        .filter_map(|order| match order.payload {
            Command::PlaceReadyBuilding { type_id, rx, ry } if type_id == wall_type => {
                Some((order.execute_tick, (rx, ry)))
            }
            _ => None,
        })
        .collect();
    let [(.., endpoint), (placing_step, origin)] = placements.as_slice() else {
        panic!("ordinary retail profile must contain two paid wall endpoints");
    };
    assert_eq!((endpoint.0, endpoint.1 + 3), *origin);
    for step in 0..*placing_step {
        let due: Vec<_> = orders
            .iter()
            .filter(|order| order.execute_tick == step)
            .cloned()
            .collect();
        scene
            .runtime
            .advance_frame(&due, 22, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
    }
    let rules = &scene.runtime.resources.rules;
    let registry = &scene.runtime.resources.overlay_registry;
    let sim = &mut scene.runtime.simulation;
    let overlay = registry.id_for_name("GAWALL").unwrap();
    assert!(sim.houses[&owner].is_human());
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(endpoint.0, endpoint.1)
            .overlay_id,
        Some(overlay)
    );
    for y in endpoint.1 + 1..=origin.1 {
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .cell(origin.0, y)
                .overlay_id,
            None
        );
    }
    let product =
        crate::sim::production::ready_building_entity_for_owner(sim, rules, owner, wall_type)
            .expect("normal charging must finish the second paid wall");
    let held = sim.entities().get(product).unwrap();
    assert!(held.lifecycle.object_alive() && held.lifecycle.in_limbo());
    let credits = sim.houses[&owner].economy().credits();
    assert!(sim.substrate.pending_delete.is_empty());
    assert!(crate::sim::production::place_production_with_overlays(
        sim,
        rules,
        &owner_name,
        crate::sim::production::ProductionPlacement::Building {
            type_id: "GAWALL",
            cell: *origin,
        },
        Some(registry),
        FrameEffects::default(),
    ));
    // Observe the production call before the ordinary frame's shared drain.
    // No health, rule, pose, timer or RNG input is overridden by this witness.
    let held = sim
        .entities()
        .get(product)
        .expect("successful wall Unlimbo retains the paid constructor until725C70");
    assert!(!held.lifecycle.object_alive() && held.lifecycle.in_limbo());
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&held.position),
        crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 }
    );
    let buildings: Vec<_> = sim
        .entities()
        .values()
        .filter(|object| object.type_ref() == wall_type)
        .map(|object| object.stable_id())
        .collect();
    assert_eq!(
        buildings.len(),
        3,
        "paid origin and two constructed fillers"
    );
    let pending: Vec<_> = sim.substrate.pending_delete.object_tickets().collect();
    assert_eq!(pending.len(), buildings.len());
    assert_eq!(pending[0], product);
    for &id in &buildings {
        let object = sim.entities().get(id).unwrap();
        assert!(!object.lifecycle.object_alive() && object.lifecycle.in_limbo());
        assert!(pending.contains(&id));
        assert_eq!(
            crate::sim::movement::ground_pose::position_world_coord(&object.position),
            crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 }
        );
    }
    for y in endpoint.1..=origin.1 {
        let cell = sim.overlay_grid.as_ref().unwrap().cell(origin.0, y);
        assert_eq!(
            (cell.overlay_id, cell.wall_owner),
            (Some(overlay), Some(owner))
        );
    }
    assert_eq!(sim.houses[&owner].economy().credits(), credits);
    assert!(
        crate::sim::production::ready_building_entity_for_owner(sim, rules, owner, wall_type)
            .is_none()
    );
    sim.process_pending_delete_with(rules, Some(registry), FrameEffects::default(), 1, true);
    for id in buildings {
        assert!(sim.entities().get(id).is_none());
    }
    assert!(sim.substrate.pending_delete.is_empty());
}

#[test]
fn retail_paid_owned_wall_replacement_clears_the_old_cell_attack_target() {
    let Some((root, _assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let profile = crate::test_fixture::json("tools/map_observation.concrete-wall.example.json");
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    let map = launch.selected_map_file.as_deref().unwrap().to_owned();
    let mut scene = crate::headless_scenario::load_session(
        &root,
        &map,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .unwrap();
    scene.runtime.simulation.batch_path_searches();
    // Periodic frame hashes feed replay and sync checks, which nothing here
    // reads; an interval no tick reaches skips them (11-12% of each frame in
    // this unoptimized test build).
    scene
        .runtime
        .simulation
        .hash_state_every(std::num::NonZeroU64::MAX);
    let owner = scene.sim().session.current_house().unwrap();
    let owner_name = scene.sim().resolve(owner).to_owned();
    assert_eq!(owner_name, "VERA-OBSERVER");
    assert!(scene.sim().houses[&owner].is_human());
    let wall_type = scene.sim().interner.get("GAWALL").unwrap();
    let rules = &scene.runtime.resources.rules;
    let object = rules.object("GAWALL").unwrap();
    assert!(object.wall && !object.unsellable);
    assert_eq!(object.cost, 100);
    let wall = scene
        .runtime
        .resources
        .overlay_registry
        .id_for_name("GAWALL")
        .unwrap();
    let mut orders: Vec<_> = profile["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            CommandEnvelope::new(
                owner,
                row["issue_after_step"].as_u64().unwrap(),
                serde_json::from_value::<Command>(row["payload"].clone()).unwrap(),
            )
        })
        .collect();
    // Isolate one paid wall. Native gap filling constructs temporary
    // Buildings, a separate lifecycle not represented by the current autofill
    // helper. This selected input produces no gaps at initial/replacement PLACE.
    let source = orders
        .iter()
        .find_map(|order| match order.payload {
            Command::PlaceReadyBuilding { type_id, rx, ry } if type_id == wall_type => {
                Some((rx, ry))
            }
            _ => None,
        })
        .unwrap();
    let (mut wall_queues, mut wall_places) = (0, 0);
    orders.retain(|order| match order.payload {
        Command::QueueProduction { type_id } if type_id == wall_type => {
            wall_queues += 1;
            wall_queues == 1
        }
        Command::PlaceReadyBuilding { type_id, .. } if type_id == wall_type => {
            wall_places += 1;
            wall_places == 1
        }
        _ => true,
    });
    for order in &mut orders {
        if let Command::ForceAttackCell {
            target_rx,
            target_ry,
            ..
        } = &mut order.payload
        {
            (*target_rx, *target_ry) = source;
        }
    }
    let (attacker, source) = orders
        .iter()
        .find_map(|order| match order.payload {
            Command::ForceAttackCell {
                attacker_id,
                target_rx,
                target_ry,
            } => Some((attacker_id, (target_rx, target_ry))),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        scene
            .sim()
            .resolve(scene.sim().entities().get(attacker).unwrap().type_ref()),
        "MTNK"
    );
    let mut replacement = None;
    for step in 0..3600 {
        let mut due: Vec<_> = orders
            .iter()
            .filter(|order| order.execute_tick == step)
            .cloned()
            .collect();
        if step == 2210 {
            due.push(CommandEnvelope::new(
                owner,
                scene.sim().session.tick(),
                Command::QueueProduction { type_id: wall_type },
            ));
        }
        let sim = scene.sim();
        let cell = *sim.overlay_grid.as_ref().unwrap().cell(source.0, source.1);
        let target = sim.entities().get(attacker).unwrap().attack_target.target();
        let ready = crate::sim::production::ready_building_entity_for_owner(
            sim,
            &scene.runtime.resources.rules,
            owner,
            wall_type,
        );
        let placing = cell.overlay_id == Some(wall)
            && cell.wall_owner == Some(owner)
            && cell.overlay_data >> 4 == 1
            && target == Some(TargetKind::Cell(source.0, source.1))
            && ready.is_some();
        if placing {
            let preview = crate::sim::production::placement_preview_for_owner_with_overlays(
                sim,
                &scene.runtime.resources.rules,
                &owner_name,
                "GAWALL",
                source.0,
                source.1,
                Some(&scene.runtime.resources.overlay_registry),
            )
            .unwrap();
            assert!(
                preview.valid && preview.in_build_area && preview.foundation_visible,
                "replacement preview: {preview:?}"
            );
            let product = ready.unwrap();
            let held = sim.entities().get(product).unwrap();
            assert!(held.lifecycle.object_alive() && held.lifecycle.in_limbo());
            replacement = Some((
                product,
                sim.houses[&owner].economy().credits(),
                sim.houses[&owner].economy().spent_credits(),
            ));
            eprintln!(
                "ordinary owned replacement step{step} data{:x} target{target:?} paidproduct{product}",
                cell.overlay_data
            );
            due.push(CommandEnvelope::new(
                owner,
                sim.session.tick(),
                Command::PlaceReadyBuilding {
                    type_id: wall_type,
                    rx: source.0,
                    ry: source.1,
                },
            ));
        }
        let output = scene
            .runtime
            .advance_frame(&due, 22, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        if let Some((product, credits, spent)) = replacement {
            let sim = scene.sim();
            let cell = sim.overlay_grid.as_ref().unwrap().cell(source.0, source.1);
            assert_eq!(cell.overlay_id, Some(wall));
            assert_eq!(cell.wall_owner, Some(owner));
            assert_eq!(
                cell.overlay_data >> 4,
                0,
                "ordinary PLACE must replace damage"
            );
            assert!(sim.entities().get(product).is_none());
            assert_eq!(sim.houses[&owner].economy().credits(), credits, "no refund");
            assert_eq!(sim.houses[&owner].economy().spent_credits(), spent);
            assert!(
                !output.sound_events.iter().any(|event| matches!(
                    event,
                    crate::sim::world::SimSoundEvent::WallSold { .. }
                ))
            );
            assert_ne!(
                sim.entities().get(attacker).unwrap().attack_target.target(),
                Some(TargetKind::Cell(source.0, source.1)),
                "House4FD018 must Restore/retest/clear the old Cell target before new Mark"
            );
            // Keep normal turns running beyond multiple 105mm rearm windows.
            // No Stop/order cancels the attack; the native sale sweep must.
            for _ in 0..180 {
                let output = scene
                    .runtime
                    .advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())
                    .unwrap();
                assert!(
                    !output
                        .fire_events
                        .iter()
                        .any(|event| event.attacker_id == attacker)
                );
                assert_ne!(
                    scene
                        .sim()
                        .entities()
                        .get(attacker)
                        .unwrap()
                        .attack_target
                        .target(),
                    Some(TargetKind::Cell(source.0, source.1)),
                );
            }
            let wall_after_wait = scene
                .sim()
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(source.0, source.1);
            assert_eq!(wall_after_wait.overlay_id, Some(wall));
            assert_eq!(wall_after_wait.wall_owner, Some(owner));
            assert_eq!(wall_after_wait.overlay_data >> 4, 0);
            let attack = CommandEnvelope::new(
                owner,
                scene.sim().session.tick(),
                Command::ForceAttackCell {
                    attacker_id: attacker,
                    target_rx: source.0,
                    target_ry: source.1,
                },
            );
            let mut fired_again = false;
            for next in 0..240 {
                let commands = if next == 0 {
                    std::slice::from_ref(&attack)
                } else {
                    &[]
                };
                let output = scene
                    .runtime
                    .advance_frame(commands, 22, TickLane::Ordinary, FrameEffects::default())
                    .unwrap();
                if output
                    .fire_events
                    .iter()
                    .any(|event| event.attacker_id == attacker)
                {
                    fired_again = true;
                    break;
                }
            }
            assert!(
                fired_again,
                "new wall must remain targetable by a new ordinary attack"
            );
            let before_sell = scene.sim().houses[&owner].economy().credits();
            let sell = CommandEnvelope::new(
                owner,
                scene.sim().session.tick(),
                Command::SellWallAtCell {
                    x: source.0 as i16,
                    y: source.1 as i16,
                },
            );
            let output = scene
                .runtime
                .advance_frame(&[sell], 22, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            assert!(
                scene
                    .sim()
                    .overlay_grid
                    .as_ref()
                    .unwrap()
                    .cell(source.0, source.1)
                    .overlay_id
                    .is_none()
            );
            assert_eq!(
                scene.sim().houses[&owner].economy().credits(),
                before_sell,
                "ordinary SELL also refunds no wall money"
            );
            assert!(
                output.sound_events.iter().any(|event| matches!(event,
                    crate::sim::world::SimSoundEvent::WallSold { receiver } if *receiver == owner
                )),
                "Event4C6FCE passes0 and keeps its audible cue"
            );
            return;
        }
    }
    panic!(
        "ordinary paid replacement, live owned stage-one wall and active Cell target were not admitted"
    );
}
