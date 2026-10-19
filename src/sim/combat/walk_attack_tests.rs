//! A walking infantryman's attack order through the production command and
//! frame path: his Walk destination search, the paid Walk head an approach
//! or a range stop leaves running, and the null destinations his missions
//! write.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

fn flat_cell(rx: u16, ry: u16) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    crate::map::resolved_terrain::ResolvedTerrainCell {
        filled_clear: true,
        accepts_smudge: true,
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    }
}

fn walk_attack_scene() -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=1000\nArmor=none\nSpeed=4\nSight=8\nPrimary=Rifle\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [Rifle]\nDamage=0\nROF=100\nRange=3\nProjectile=Bullet\nWarhead=SA\n\
         [Bullet]\nAG=yes\nAA=no\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(0x75bd25);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (index, (name, human)) in [("Local", true), ("Enemy", false)].into_iter().enumerate() {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner,
                index as u8,
                None,
                human,
                0,
                10,
            ));
    }
    sim.session
        .set_current_house_for_test(sim.interner.get("Local"));
    let costs = rules
        .terrain_rules
        .semantics_for_land_type(crate::rules::terrain_rules::LandType::Clear.as_index())
        .unwrap()
        .speed_costs;
    let cells = (0..64)
        .flat_map(|y| {
            (0..64).map(move |x| {
                let mut cell = flat_cell(x, y);
                cell.speed_costs = costs;
                cell.base_speed_costs = costs;
                cell
            })
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(64, 64, cells),
    );
    assert!(sim.rebuild_dynamic_navigation(&rules));
    // The scene lies near the grid's origin. A map `Size=` 16 cells wide puts
    // it in the map's diamond (a 64-wide one starts past x + y = 64), where
    // sight reveals (`MapClass::RevealArea @ 0x005678E0`).
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 16,
        off_fc: -64,
        off_100: -64,
        off_104: 128,
        off_108: 128,
    });
    sim.playfield_size_height = Some(64);
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let actor = sim.spawn_object("E1", "Local", 10, 10, 0, &rules).unwrap();
    // Production visibility is recomputed by the frame host, not by spawn.
    // Establish the actor's sight before introducing an enemy or an order.
    walk_frame(&mut sim, &rules);
    let victim = sim.spawn_object("E1", "Enemy", 16, 10, 0, &rules).unwrap();
    (sim, rules, actor, victim)
}

fn walk_frame(sim: &mut Simulation, rules: &RuleSet) {
    let grid = sim.path_grid_snapshot();
    sim.advance_tick(&[], Some(rules), grid.as_deref(), None, 67);
}

fn walk_command(sim: &mut Simulation, rules: &RuleSet, command: crate::sim::command::Command) {
    assert!(sim.apply_command_with_overlays(
        "Local",
        &command,
        Some(rules),
        None,
        crate::sim::world::FrameEffects::default()
    ));
}

fn wait_for_walk_head(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
) -> crate::sim::components::DriveCoord {
    for _ in 0..50 {
        walk_frame(sim, rules);
        let loco = sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap();
        if let Some(head) = loco.step_head() {
            assert_eq!(loco.walk_is_moving(), Some(true));
            assert_eq!(loco.walk_animation_moving(), Some(true));
            return head;
        }
    }
    panic!("real command/frame path must produce a paid Walk head");
}

#[test]
fn walk_destination_search_observes_route_opened_before_process() {
    let (mut sim, rules, actor, _) = walk_attack_scene();
    let open_grid = sim.path_grid_snapshot().unwrap();
    // Controlled navigation input: no route crosses this full-height barrier
    // when the order is accepted. The canonical map used by the subsequent
    // object turn is open. This distinguishes deferred search from storing an
    // order-time route (or refusing the accepted destination on A* failure).
    let mut closed_grid = (*open_grid).clone();
    for y in 0..closed_grid.height() {
        closed_grid.set_blocked(12, y, true);
        assert!(!closed_grid.is_any_layer_walkable(12, y));
    }
    // The order reads the canonical grid, so publish the closed one for the
    // command and restore the open one for the object turn.
    sim.path_grid = Some(std::sync::Arc::new(closed_grid));
    assert!(sim.apply_command_with_overlays(
        "Local",
        &crate::sim::command::Command::Move {
            entity_id: actor,
            target_rx: 14,
            target_ry: 10,
        },
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    ));
    sim.path_grid = Some(open_grid.clone());
    let e = sim.substrate.entities.get(actor).unwrap();
    assert!(e.navigation.path_replay.remaining_directions().is_empty());
    assert_eq!(
        e.navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::Cell { rx: 14, ry: 10 })
    );
    assert!(e.locomotor.as_ref().unwrap().step_head().is_none());
    walk_frame(&mut sim, &rules);
    let e = sim.substrate.entities.get(actor).unwrap();
    assert!(e.locomotor.as_ref().unwrap().step_head().is_some());
    assert!(
        e.navigation
            .path_replay
            .remaining_cells_from((e.position.rx() as i16, e.position.ry() as i16))
            .contains(&(12, 10)),
        "the first Process searches the now-open route"
    );

    // Scatter's prepublished setter must also leave an execution request,
    // even though its caller ignores the helper's return value.
    let (mut sim, _rules, actor, _) = walk_attack_scene();
    let destination_coord = crate::sim::movement::target_cell_coord(
        14,
        10,
        sim.resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
            .as_ref(),
    );
    assert!(crate::sim::movement::prepare_walk_destination(
        &mut sim.substrate.entities,
        actor,
        (
            crate::sim::components::NavTargetRef::cell(14, 10),
            destination_coord
        ),
        sim.resolved_terrain.as_ref(),
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let e = sim.substrate.entities.get(actor).unwrap();
    assert!(crate::sim::movement::motion_query::is_moving(&e) == Some(true));
    assert!(e.navigation.nav_com().is_some());
    assert!(e.locomotor.as_ref().unwrap().walk_destination().is_some());
}

#[test]
fn the_read_route_starts_at_the_object_after_a_paid_walk_step() {
    // The suffix a caller reads must step out of the object's own cell. Walk
    // refreshes Foot+558 from the position on every completed step, so this
    // catches a read that quietly keeps anchoring on the acceptance point.
    let (mut sim, rules, actor, _) = walk_attack_scene();
    walk_command(
        &mut sim,
        &rules,
        crate::sim::command::Command::Move {
            entity_id: actor,
            target_rx: 14,
            target_ry: 10,
        },
    );
    let paid_at_order = sim
        .substrate
        .entities
        .get(actor)
        .unwrap()
        .navigation
        .path_replay
        .cursor();
    let mut frames = 0;
    while frames < 200 {
        walk_frame(&mut sim, &rules);
        frames += 1;
        let cursor = sim
            .substrate
            .entities
            .get(actor)
            .unwrap()
            .navigation
            .path_replay
            .cursor();
        if cursor > paid_at_order {
            break;
        }
    }
    let e = sim.substrate.entities.get(actor).unwrap();
    let queue = &e.navigation.path_replay;
    assert!(
        queue.cursor() > paid_at_order,
        "the fixture must pay at least one step (frames={frames})"
    );
    let head = (e.position.rx() as i16, e.position.ry() as i16);
    let Some(first) = queue.remaining_cells_from(head).first().copied() else {
        panic!("a mover mid-route must still have an unpaid suffix");
    };
    let (fx, fy) = first;
    assert!(
        (fx - head.0).abs() <= 1 && (fy - head.1).abs() <= 1 && first != head,
        "the head word must leave the object's own cell: head {head:?} first {first:?}"
    );
}

#[test]
fn walk_cell_order_defers_queue_publication_and_first_head_motion() {
    use crate::sim::components::{FootPathQueue, NavTargetRef};
    let (mut sim, rules, actor, _) = walk_attack_scene();
    let queue = FootPathQueue::for_test(vec![2, 3, 4, 5], 0, Some((9, 8)));
    let entity = sim.substrate.entities.get_mut(actor).unwrap();
    entity.navigation.path_replay = queue.clone();
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .set_powered_for_test(false);
    entity
        .navigation
        .set_nav_queue_for_test(vec![NavTargetRef::Cell { rx: 25, ry: 10 }]);
    let before = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    walk_command(
        &mut sim,
        &rules,
        crate::sim::command::Command::Move {
            entity_id: actor,
            target_rx: 14,
            target_ry: 10,
        },
    );
    let entity = sim.substrate.entities.get(actor).unwrap();
    assert!(
        !entity.locomotor.as_ref().unwrap().is_powered(),
        "ordinary Walk destination also preserves native power state"
    );
    let mut invalidated = queue;
    invalidated.clear_live_head();
    assert_eq!(
        entity.navigation.path_replay, invalidated,
        "accepted setter writes one native path head"
    );
    assert_eq!(
        entity.navigation.nav_queue(),
        vec![NavTargetRef::Cell { rx: 25, ry: 10 }]
    );
    assert_eq!(entity.locomotor.as_ref().unwrap().step_head(), None);
    walk_frame(&mut sim, &rules);
    let entity = sim.substrate.entities.get(actor).unwrap();
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&entity.position),
        before,
        "the original fresh-head Process returns before numerical motion"
    );
    assert!(entity.locomotor.as_ref().unwrap().step_head().is_some());
    assert_eq!(
        entity.locomotor.as_ref().unwrap().walk_animation_moving(),
        Some(true)
    );
    assert!(
        !entity
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(
        entity.navigation.path_replay.reference_cell(),
        Some((10, 10))
    );
    assert_eq!(
        entity.foot_speed.applied_fraction(),
        crate::util::fixed_math::SIM_ONE
    );
    walk_frame(&mut sim, &rules);
    assert_ne!(
        crate::sim::movement::ground_pose::position_world_coord(
            &sim.substrate.entities.get(actor).unwrap().position
        ),
        before,
        "the subsequent paid-head Process advances"
    );
}

/// A GI on Attack whose target comes in range mid-step finishes his paid
/// Walk head, and the per-cell range stop at its completion ends the
/// approach his Attack's Approach began.
#[test]
fn walk_attack_range_entry_finishes_paid_head_then_accepts_new_move() {
    use crate::sim::command::Command;
    let (mut sim, rules, actor, victim) = walk_attack_scene();
    let owner = sim.substrate.entities.get(actor).unwrap().owner();
    assert!(
        sim.fog.is_cell_visible(owner, 16, 10),
        "the ordered target starts outside Range3 but inside Sight8"
    );
    walk_command(
        &mut sim,
        &rules,
        Command::Attack {
            attacker_id: actor,
            target_id: victim,
        },
    );
    let head = wait_for_walk_head(&mut sim, &rules, actor);
    assert_eq!(
        sim.substrate
            .entities
            .get(actor)
            .unwrap()
            .attack_target
            .target(),
        Some(crate::sim::combat::TargetKind::Entity(victim)),
        "the real first-step path retains the visible attack target"
    );
    // Controlled opponent movement input, using the actual membership writers.
    // The mover's command/Process/head and the subsequent range call are real.
    sim.remove_entity_occupancy(victim);
    {
        let e = sim.substrate.entities.get_mut(victim).unwrap();
        e.position.set_rx_for_test((head.x / 256 + 2) as u16);
        e.position.set_ry_for_test((head.y / 256) as u16);
        e.position.set_exact_z_leptons_for_test(Some(0));
    }
    sim.add_entity_occupancy(victim);
    assert!(sim.owns_foot_approach(actor, &rules));
    walk_frame(&mut sim, &rules);
    let e = sim.substrate.entities.get(actor).unwrap();
    assert_eq!(e.locomotor.as_ref().unwrap().step_head(), Some(head));
    assert!(
        crate::sim::movement::motion_query::is_moving(&e) == Some(true),
        "range entry must not strand the paid head"
    );
    assert!(e.navigation.nav_com().is_some());
    for _ in 0..80 {
        walk_frame(&mut sim, &rules);
        if crate::sim::movement::motion_query::is_moving(sim.substrate.entities.get(actor).unwrap())
            != Some(true)
        {
            break;
        }
    }
    let e = sim.substrate.entities.get(actor).unwrap();
    assert!(
        crate::sim::movement::motion_query::is_moving(&e) != Some(true),
        "completed-head PerCell range stop ends the approach"
    );
    assert_eq!(e.locomotor.as_ref().unwrap().step_head(), None);
    assert_eq!(e.locomotor.as_ref().unwrap().walk_destination(), None);
    assert_eq!(e.locomotor.as_ref().unwrap().walk_is_moving(), Some(false));
    assert_eq!(
        e.locomotor.as_ref().unwrap().walk_animation_moving(),
        Some(false)
    );
    assert!(e.navigation.nav_com().is_none());
    assert_eq!(
        (e.position.rx(), e.position.ry()),
        ((head.x / 256) as u16, (head.y / 256) as u16)
    );
    let from = (e.position.rx(), e.position.ry());
    let raw_bits: u32 = sim
        .substrate
        .raw_cell_occupation
        .entries()
        .map(|(_, _, ground, deck, _, _)| {
            u32::from(ground & 0x1c).count_ones() + u32::from(deck & 0x1c).count_ones()
        })
        .sum();
    assert_eq!(
        raw_bits, 2,
        "only the two current Infantry positions remain marked"
    );
    walk_command(
        &mut sim,
        &rules,
        Command::Move {
            entity_id: actor,
            target_rx: from.0,
            target_ry: from.1 + 4,
        },
    );
    let next_head = wait_for_walk_head(&mut sim, &rules, actor);
    assert_ne!(
        next_head, head,
        "a later Move must not resurrect the retired head"
    );
}

#[test]
fn ordered_walk_attack_nulls_destination_but_preserves_paid_head_and_queues() {
    use crate::sim::command::Command;
    use crate::sim::components::{FootPathQueue, NavTargetRef};
    for force_cell in [false, true] {
        let (mut sim, rules, actor, victim) = walk_attack_scene();
        walk_command(
            &mut sim,
            &rules,
            Command::Move {
                entity_id: actor,
                target_rx: 20,
                target_ry: 10,
            },
        );
        let head = wait_for_walk_head(&mut sim, &rules, actor);
        // Distinguish the two Foot queues using supplied existing backing state.
        let queue = FootPathQueue::for_test(vec![6, 2, 3, 4, 5], 1, Some((9, 10)));
        let e = sim.substrate.entities.get_mut(actor).unwrap();
        e.navigation.path_replay = queue.clone();
        e.navigation
            .push_nav_queue_for_test(NavTargetRef::Cell { rx: 25, ry: 10 });
        let queued = e.navigation.nav_queue().to_vec();
        walk_command(
            &mut sim,
            &rules,
            if force_cell {
                Command::ForceAttackCell {
                    attacker_id: actor,
                    target_rx: 12,
                    target_ry: 10,
                }
            } else {
                Command::Attack {
                    attacker_id: actor,
                    target_id: victim,
                }
            },
        );
        let e = sim.substrate.entities.get(actor).unwrap();
        assert_eq!(e.locomotor.as_ref().unwrap().step_head(), Some(head));
        assert_eq!(e.locomotor.as_ref().unwrap().walk_destination(), None);
        assert_eq!(e.locomotor.as_ref().unwrap().walk_is_moving(), Some(true));
        assert_eq!(
            e.locomotor.as_ref().unwrap().walk_animation_moving(),
            Some(true)
        );
        assert!(crate::sim::movement::motion_query::is_moving(&e) == Some(true));
        assert!(e.navigation.nav_com().is_none());
        let mut expected = queue;
        expected.clear_live_head();
        assert_eq!(e.navigation.path_replay, expected);
        assert_eq!(e.navigation.nav_queue(), queued);
        for _ in 0..80 {
            walk_frame(&mut sim, &rules);
            if sim
                .substrate
                .entities
                .get(actor)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .step_head()
                .is_none()
            {
                break;
            }
        }
        let e = sim.substrate.entities.get(actor).unwrap();
        assert!(
            e.locomotor.as_ref().unwrap().step_head().is_none(),
            "null destination still finishes its paid step"
        );
        assert_eq!(
            (e.position.rx(), e.position.ry()),
            ((head.x / 256) as u16, (head.y / 256) as u16)
        );
        assert_eq!(e.navigation.nav_queue(), queued);
    }
}

/// An owner change nulls the Walk destination through the class setter
/// (0x007014E9). Walk75AEC0 still runs on its paid head, so the man finishes
/// that step instead of stalling mid-cell.
#[test]
fn owner_change_finishes_the_paid_walk_head() {
    use crate::sim::command::Command;
    let (mut sim, rules, actor, victim) = walk_attack_scene();
    walk_command(
        &mut sim,
        &rules,
        Command::Move {
            entity_id: actor,
            target_rx: 20,
            target_ry: 10,
        },
    );
    let head = wait_for_walk_head(&mut sim, &rules, actor);
    let new_owner = sim.substrate.entities.get(victim).unwrap().owner();
    sim.change_owner_with_rules(
        actor,
        new_owner,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let e = sim.substrate.entities.get(actor).unwrap();
    assert_eq!(e.locomotor.as_ref().unwrap().walk_destination(), None);
    assert_eq!(e.locomotor.as_ref().unwrap().step_head(), Some(head));
    for _ in 0..80 {
        walk_frame(&mut sim, &rules);
        if sim
            .substrate
            .entities
            .get(actor)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .step_head()
            .is_none()
        {
            break;
        }
    }
    let e = sim.substrate.entities.get(actor).unwrap();
    assert!(
        e.locomotor.as_ref().unwrap().step_head().is_none(),
        "paid head stalls"
    );
    assert_eq!(
        (e.position.rx(), e.position.ry()),
        ((head.x / 256) as u16, (head.y / 256) as u16)
    );
}

/// The Area Guard MEGAMISSION gives a Foot the event's NULL destination
/// (0x004C7420): a walking man stops at his paid head instead of walking on
/// to the old destination.
#[test]
fn area_guard_nulls_the_walk_destination() {
    use crate::sim::command::Command;
    let (mut sim, rules, actor, _) = walk_attack_scene();
    walk_command(
        &mut sim,
        &rules,
        Command::Move {
            entity_id: actor,
            target_rx: 20,
            target_ry: 10,
        },
    );
    let head = wait_for_walk_head(&mut sim, &rules, actor);
    walk_command(
        &mut sim,
        &rules,
        Command::Guard {
            entity_id: actor,
            target: None,
        },
    );
    let loco = sim
        .substrate
        .entities
        .get(actor)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap();
    assert_eq!(
        (loco.walk_destination(), loco.step_head()),
        (None, Some(head))
    );
}

#[test]
fn walk_null_setter_matches_original_caller_rows() {
    use crate::sim::components::{DriveCoord, FootPathQueue, NavTargetRef};
    use crate::sim::mission::{MissionDispatchTimer, MissionId, state::MissionTestFixture};
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_percell_stop.json",
    ))
    .unwrap();
    for row in corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["setter_only"] == true)
    {
        let input = &row["input"];
        let (mut sim, mut rules, actor, victim) = walk_attack_scene();
        rules.general.blockage_path_delay_ticks = 22;
        let e = sim.substrate.entities.get_mut(actor).unwrap();
        e.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(input["mission"].as_i64().unwrap_or(1) as i32),
            queued: MissionId::NONE,
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        e.navigation
            .set_nav_com_for_test(Some(NavTargetRef::Object { id: victim }));
        e.navigation
            .set_nav_com_aux_for_test(Some(NavTargetRef::Object { id: victim }));
        e.navigation.path_replay = FootPathQueue::for_test(vec![2, 3, 4, 5], 0, Some((10, 10)));
        if input["nav_queue"] == 1 {
            e.navigation
                .push_nav_queue_for_test(NavTargetRef::Object { id: victim });
        }
        if input["contact"] == true {
            e.radio_contacts.insert(victim);
        }
        e.navigation.path_runtime.start_movement(0, 5);
        e.navigation.path_runtime.start_blocked(0, 6);
        e.navigation.path_runtime.set_path_blocked(true);
        let loco = e.locomotor.as_mut().unwrap();
        loco.set_walk_destination(Some(DriveCoord {
            x: 7808,
            y: 2688,
            z: 0,
        }));
        let head = DriveCoord {
            x: 2880,
            y: 2624,
            z: 0,
        };
        if input["head"] == true || input["process_motion"] == true {
            loco.set_step_head(Some(head));
        }
        if input["process_motion"] == true {
            loco.begin_walk_motion();
            if input["head"] != true {
                loco.set_step_head(None);
            }
        }
        assert!(sim.set_infantry_null_destination(
            actor,
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default()
        ));
        let e = sim.substrate.entities.get(actor).unwrap();
        let loco = e.locomotor.as_ref().unwrap();
        assert_eq!(loco.walk_is_moving(), Some(row["moving"] == 1), "{input}");
        assert_eq!(
            loco.walk_animation_moving(),
            Some(row["animation_moving"] == 1),
            "{input}"
        );
        assert_eq!(
            loco.step_head(),
            if input["head"] == true {
                Some(head)
            } else {
                None
            }
        );
        assert_eq!(loco.walk_destination(), None);
        assert!(e.navigation.nav_com().is_none());
        assert!(e.navigation.nav_com_aux().is_none());
        let expected: Vec<u8> = row["queue"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as u8)
            .collect();
        assert_eq!(e.navigation.path_replay.directions(), expected, "{input}");
        assert_eq!(e.navigation.path_replay.reference_cell(), Some((10, 10)));
        assert_eq!(
            e.navigation.nav_queue().len(),
            row["nav_queue_count"].as_u64().unwrap() as usize
        );
        let path = &e.navigation.path_runtime;
        assert!(!path.path_blocked());
        assert_eq!(
            path.movement_timer()
                .remaining(sim.session.binary_frame() as i32),
            0
        );
        assert_eq!(
            path.blocked_timer()
                .remaining(sim.session.binary_frame() as i32),
            22
        );
    }
}
