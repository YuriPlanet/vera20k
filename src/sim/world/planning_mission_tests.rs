//! Resolved special clicks through retained plans and the real mission owners.
use super::*;
use crate::sim::command::Command;

fn scene() -> Option<(Simulation, crate::rules::ruleset::RuleSet, InternedId)> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")?;
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 638120, 64);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.input_delay_ticks = 0;
    sim.session.game_options_mut_for_test().short_game = false;
    sim.session.game_options_mut_for_test().multi_engineer = false;
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses
        .insert_for_test(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 50_000, 10,
        ));
    sim.session.set_current_house_for_test(Some(owner));
    Some((sim, rules, owner))
}

fn planned(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner: InternedId,
    command: Command,
) {
    let record = sim
        .encode_planning_order_record(owner, &command)
        .expect("resolved click is representable");
    let command = sim
        .decode_native_command_record(&record, sim.session.tick())
        .unwrap();
    assert!(
        sim.apply_command("Americans", &command.payload, Some(rules)),
        "append {command:?}"
    );
}

fn commit(sim: &mut Simulation, rules: &crate::rules::ruleset::RuleSet, owner: InternedId) {
    let record = sim.encode_plan_commit_record(owner).unwrap();
    let command = sim
        .decode_native_command_record(&record, sim.session.tick())
        .unwrap();
    assert!(sim.apply_command("Americans", &command.payload, Some(rules)));
}

fn step(sim: &mut Simulation, rules: &crate::rules::ruleset::RuleSet) {
    step_with_overlays(sim, rules, None);
}

fn step_with_overlays(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlays: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) {
    let commands = sim.take_due_commands();
    sim.advance_app_frame(
        &commands,
        rules,
        overlays,
        33,
        super::super::TickLane::Ordinary,
        Default::default(),
    )
    .unwrap();
}

#[test]
fn targetless_mobile_self_click_does_not_create_a_planning_node() {
    let Some((mut sim, rules, owner)) = scene() else {
        return;
    };
    let actor = sim
        .spawn_object("AMCV", "Americans", 12, 12, 0, &rules)
        .unwrap();
    // Event633BC0 returns (-1,-1,-1) for three NULL tokens; PlanMgr638158
    // rejects that coordinate. A self-click is not the deploy key's Event9.
    let record = sim
        .encode_planning_order_record(owner, &Command::DeployMcv { entity_id: actor })
        .unwrap();
    let command = sim.decode_native_command_record(&record, 0).unwrap();
    assert!(!sim.apply_command("Americans", &command.payload, Some(&rules)));
    assert!(sim.planning.paths.is_empty());
    assert!(sim.entities().get(actor).is_some());
}

#[test]
fn retail_planned_tanya_plants_c4_and_continues_to_the_next_waypoint() {
    let Some((mut sim, rules, owner)) = scene() else {
        return;
    };
    let target = sim
        .spawn_object("GAPOWR", "Russians", 17, 12, 0, &rules)
        .unwrap();
    let tanya = sim
        .spawn_object("TANY", "Americans", 10, 12, 0, &rules)
        .unwrap();
    assert!(rules.object("TANY").unwrap().c4);
    sim.fog.mark_visible_for_owner(owner, 17, 12);
    planned(
        &mut sim,
        &rules,
        owner,
        Command::Move {
            entity_id: tanya,
            target_rx: 12,
            target_ry: 12,
        },
    );
    planned(
        &mut sim,
        &rules,
        owner,
        Command::PlantC4 {
            attacker_id: tanya,
            target_building_id: target,
        },
    );
    planned(
        &mut sim,
        &rules,
        owner,
        Command::Move {
            entity_id: tanya,
            target_rx: 24,
            target_ry: 12,
        },
    );
    assert_eq!(sim.planning.paths[&tanya].nodes.len(), 3);
    assert!(
        sim.entities()
            .get(target)
            .unwrap()
            .pending_c4_detonation
            .pending()
            .is_none()
    );
    // Save before committing: the new mission stays in the authoritative raw
    // node records, retaining ordering and target through restoration.
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "planned-c4", 0);
    sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    sim.restore_after_snapshot_load().unwrap();
    sim.resolve_type_handles(&rules);
    // Map terrain/navigation are external to a GameSnapshot, as on load.
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    commit(&mut sim, &rules, owner);
    let mut planted = false;
    let mut arrived = false;
    for _ in 0..1500 {
        step(&mut sim, &rules);
        if let Some(charge) = sim
            .entities()
            .get(target)
            .and_then(|entity| entity.pending_c4_detonation.pending())
        {
            assert_eq!(charge.source_entity_id(), Some(tanya));
            planted = true;
        }
        if sim
            .entities()
            .get(tanya)
            .is_some_and(|entity| entity.position.rx() == 24 && entity.position.ry() == 12)
        {
            arrived = true;
            break;
        }
    }
    assert!(
        planted,
        "planned C4 must reach the real arrival/charge owner"
    );
    assert!(arrived, "the route must continue after sabotage");
    assert!(!sim.planning.paths.contains_key(&tanya));
}

#[test]
fn retail_planned_capture_consumes_engineer_and_clears_its_path() {
    let Some((mut sim, rules, owner)) = scene() else {
        return;
    };
    let target = sim
        .spawn_object("GAPOWR", "Russians", 17, 12, 0, &rules)
        .unwrap();
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 12, 12, 0, &rules)
        .unwrap();
    planned(
        &mut sim,
        &rules,
        owner,
        Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: target,
        },
    );
    commit(&mut sim, &rules, owner);
    for _ in 0..700 {
        step(&mut sim, &rules);
        if sim
            .entities()
            .get(target)
            .is_some_and(|entity| entity.owner() == owner)
        {
            break;
        }
    }
    assert_eq!(sim.entities().get(target).unwrap().owner(), owner);
    assert!(
        sim.entities()
            .get(engineer)
            .is_none_or(|entity| !entity.lifecycle.object_alive())
    );
    assert!(!sim.planning.paths.contains_key(&engineer));
}

#[test]
fn retail_planned_enter_reaches_transport_and_bunker_adapters() {
    for bunker in [false, true] {
        let Some((mut sim, rules, owner)) = scene() else {
            return;
        };
        let target = sim
            .spawn_object(
                if bunker { "NATBNK" } else { "FV" },
                "Americans",
                17,
                12,
                0,
                &rules,
            )
            .unwrap();
        let actor = sim
            .spawn_object(
                if bunker { "MTNK" } else { "E1" },
                "Americans",
                12,
                12,
                0,
                &rules,
            )
            .unwrap();
        let order = Command::clicked_mission(
            actor,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(target),
        );
        planned(&mut sim, &rules, owner, order);
        commit(&mut sim, &rules, owner);
        let mut entered = false;
        for _ in 0..1000 {
            step(&mut sim, &rules);
            entered = if bunker {
                sim.entities()
                    .get(target)
                    .is_some_and(|entity| entity.bunker.occupant() == Some(actor))
            } else {
                sim.entities().get(actor).is_some_and(|entity| {
                    entity.passenger_role.inside_transport_id() == Some(target)
                })
            };
            if entered {
                break;
            }
        }
        assert!(entered, "planned Enter, bunker={bunker}");
        assert!(!sim.planning.paths.contains_key(&actor));
    }
}

#[test]
fn retail_planned_depot_and_refinery_orders_reach_their_dock_owners() {
    for refinery in [false, true] {
        let Some((mut sim, rules, owner)) = scene() else {
            return;
        };
        let target = sim
            .spawn_object(
                if refinery { "NAREFN" } else { "GADEPT" },
                "Americans",
                17,
                12,
                0,
                &rules,
            )
            .unwrap();
        let kind = if refinery { "HARV" } else { "MTNK" };
        let actor = sim
            .spawn_object(kind, "Americans", 12, 12, 0, &rules)
            .unwrap();
        if refinery {
            crate::sim::arena_fixture::install_status_miner_cargo(
                sim.substrate.entities.get_mut(actor).unwrap(),
                rules.object(kind).unwrap(),
                &rules,
                3,
                0,
            );
        } else {
            sim.substrate
                .entities
                .get_mut(actor)
                .unwrap()
                .health
                .set_for_test(rules.object(kind).unwrap().strength / 2);
        }
        assert!(sim.apply_command(
            "Americans",
            &Command::Stop { entity_id: actor },
            Some(&rules)
        ));
        for _ in 0..3 {
            step(&mut sim, &rules);
        }
        let order = Command::clicked_mission(
            actor,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(target),
        );
        planned(&mut sim, &rules, owner, order);
        commit(&mut sim, &rules, owner);
        let mut docked = false;
        for _ in 0..1000 {
            step(&mut sim, &rules);
            docked |= sim
                .entities()
                .get(actor)
                .is_some_and(|entity| entity.radio_contacts.slot(0) == Some(target));
            let completed = sim.entities().get(actor).is_some_and(|entity| {
                if refinery {
                    entity
                        .miner
                        .as_ref()
                        .is_some_and(|miner| miner.cargo().is_empty())
                } else {
                    entity.health.is_full(rules.object(kind).unwrap().strength)
                }
            });
            if completed && docked {
                break;
            }
        }
        assert!(docked, "planned Enter must reach dock, refinery={refinery}");
        let actor = sim.entities().get(actor).unwrap();
        if refinery {
            assert!(actor.miner.as_ref().unwrap().cargo().is_empty());
        } else {
            assert!(actor.health.is_full(rules.object(kind).unwrap().strength));
        }
    }
}

#[test]
fn planned_special_order_keeps_shared_nodes_after_stop_and_expired_target() {
    let Some((mut sim, rules, owner)) = scene() else {
        return;
    };
    let target = sim
        .spawn_object("GAPOWR", "Russians", 17, 12, 0, &rules)
        .unwrap();
    let actors: Vec<_> = [10, 11]
        .map(|x| {
            sim.spawn_object("TANY", "Americans", x, 12, 0, &rules)
                .unwrap()
        })
        .into();
    for &actor in &actors {
        planned(
            &mut sim,
            &rules,
            owner,
            Command::PlantC4 {
                attacker_id: actor,
                target_building_id: target,
            },
        );
    }
    assert_eq!(sim.planning.nodes.len(), 1);
    assert!(sim.apply_command(
        "Americans",
        &Command::Stop {
            entity_id: actors[0]
        },
        Some(&rules)
    ));
    assert!(!sim.planning.paths.contains_key(&actors[0]));
    assert_eq!(sim.planning.paths[&actors[1]].nodes.len(), 1);
    // Removing a clicked target invalidates only its target node, through
    // the existing expiry owner; it must not leave a dangling saved plan.
    sim.uninit_with_rules(target, &rules);
    sim.validate_planning_references().unwrap();
    commit(&mut sim, &rules, owner);
    for _ in 0..4 {
        step(&mut sim, &rules);
    }
    sim.validate_planning_references().unwrap();
}

#[test]
fn retail_planned_and_ordinary_harvest_extracts_ore_after_walking() {
    for planning in [false, true] {
        for dock_owner in [None, Some("Americans"), Some("Russians")] {
            let Some((mut sim, rules, owner)) = scene() else {
                return;
            };
            let retail =
                crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
                    .unwrap();
            let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
                &retail.processed_rules,
                Some(&retail.fixed_art),
            );
            let ore = overlays.id_for_name("TIB01").unwrap();
            let mut grid = crate::sim::overlay_grid::OverlayGrid::new(64, 64);
            grid.place_overlay(17, 12, ore, 11);
            grid.recalculate_runtime_cell(
                sim.resolved_terrain.as_mut().unwrap(),
                &overlays,
                (17, 12),
            );
            sim.overlay_grid = Some(grid);
            assert!(sim.rebuild_dynamic_navigation(&rules));
            // Native Harvest73E67F..73E6C2 requires an owned instance of a
            // type in Dock=. An enemy refinery must not satisfy that gate.
            if let Some(dock_owner) = dock_owner {
                sim.spawn_object("NAREFN", dock_owner, 40, 40, 0, &rules)
                    .unwrap();
            }
            let actor = sim
                .spawn_object("HARV", "Americans", 12, 12, 0, &rules)
                .unwrap();
            // Unlimbo starts a miner on Harvest. Native PlanMgr638C70 does not
            // interrupt that mission at commit; begin this route from idle Guard,
            // as for the depot/refinery scenario above.
            assert!(sim.apply_command(
                "Americans",
                &Command::Stop { entity_id: actor },
                Some(&rules)
            ));
            for _ in 0..3 {
                step_with_overlays(&mut sim, &rules, Some(&overlays));
            }
            assert_eq!(
                sim.entities()
                    .get(actor)
                    .unwrap()
                    .mission
                    .effective()
                    .known(),
                Some(crate::sim::mission::MissionType::Guard)
            );
            let order = Command::clicked_mission(
                actor,
                crate::sim::mission::MissionType::Harvest,
                crate::sim::combat::TargetKind::Cell(17, 12),
            );
            assert_eq!(
                sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(17, 12)
                    .unwrap()
                    .land_type,
                crate::sim::pathfinding::passability::LandType::Tiberium.as_index()
            );
            if planning {
                planned(&mut sim, &rules, owner, order);
                commit(&mut sim, &rules, owner);
            } else {
                assert!(sim.apply_command("Americans", &order, Some(&rules)));
            }
            let mut harvest_dispatched = false;
            let mut guard_after_harvest = false;
            for _ in 0..1000 {
                harvest_dispatched |= sim
                    .entities()
                    .get(actor)
                    .unwrap()
                    .mission
                    .effective()
                    .known()
                    == Some(crate::sim::mission::MissionType::Harvest);
                step_with_overlays(&mut sim, &rules, Some(&overlays));
                let mission = &sim.entities().get(actor).unwrap().mission;
                guard_after_harvest |= harvest_dispatched
                    && (mission.current().known() == Some(crate::sim::mission::MissionType::Guard)
                        || mission.queued().known()
                            == Some(crate::sim::mission::MissionType::Guard));
                if !sim
                    .entities()
                    .get(actor)
                    .unwrap()
                    .miner
                    .as_ref()
                    .unwrap()
                    .cargo()
                    .is_empty()
                {
                    break;
                }
            }
            let actor = sim.entities().get(actor).unwrap();
            assert!(
                harvest_dispatched,
                "Harvest must reach the mission owner, planning={planning}"
            );
            let owns_dock = dock_owner == Some("Americans");
            assert_eq!(
                !actor.miner.as_ref().unwrap().cargo().is_empty(),
                owns_dock,
                "planning={planning}, dock_owner={dock_owner:?}, mission={:?}",
                actor.mission
            );
            if owns_dock {
                assert_eq!((actor.position.rx(), actor.position.ry()), (17, 12));
            } else {
                // An armed miner may subsequently acquire the other house's
                // units. Check the Harvest exit, not its later Guard AI.
                assert!(guard_after_harvest);
            }
        }
    }
}

#[test]
fn retail_planned_garrison_capture_and_self_unload_reach_passenger_owners() {
    let Some((mut sim, rules, owner)) = scene() else {
        return;
    };
    let building = sim
        .spawn_object("CAGAS01", "Americans", 17, 12, 0, &rules)
        .unwrap();
    assert!(rules.object("CAGAS01").unwrap().can_be_occupied);
    let actor = sim
        .spawn_object("E1", "Americans", 12, 12, 0, &rules)
        .unwrap();
    planned(
        &mut sim,
        &rules,
        owner,
        Command::CaptureBuilding {
            engineer_id: actor,
            target_building_id: building,
        },
    );
    commit(&mut sim, &rules, owner);
    for _ in 0..1000 {
        step(&mut sim, &rules);
        if sim
            .entities()
            .get(actor)
            .is_some_and(|e| e.passenger_role.inside_transport_id() == Some(building))
        {
            break;
        }
    }
    assert_eq!(
        sim.entities()
            .get(actor)
            .unwrap()
            .passenger_role
            .inside_transport_id(),
        Some(building)
    );
    planned(
        &mut sim,
        &rules,
        owner,
        Command::UnloadPassengers {
            transport_id: building,
        },
    );
    commit(&mut sim, &rules, owner);
    for _ in 0..1000 {
        step(&mut sim, &rules);
        if sim
            .entities()
            .get(actor)
            .is_some_and(|e| !e.passenger_role.is_inside_transport())
        {
            assert!(!sim.entities().get(actor).unwrap().lifecycle.in_limbo());
            return;
        }
    }
    panic!("planned building self-Unload must release the garrison");
}
