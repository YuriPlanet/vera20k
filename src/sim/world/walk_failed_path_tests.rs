//! Live `FootClass::Find_Path` failure continuation for a Walk infantryman:
//! Infantry receiver 0x0051DAF0, the 0x4D404A..0x4D41F0 tail and the outer
//! Walk Process cancellation. Native expectations come from the astar_null
//! rows of tools/spatial_oracle/walk_failed_path (near Cell 11,10 and far
//! Cell 13,10 from Cell 10,10; current Clear and Water contrasts). This file
//! borrows the flat 33x33 ENGINEER/CABHUT fixture of the bridge repair tests.
use super::tests::fixture;
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::mission::MissionId;
use crate::sim::world::Simulation;

fn human_house(sim: &mut Simulation) {
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 0);
    house.player_control = true;
    sim.houses.insert(owner, house);
}

fn engineer_at(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16)) -> u64 {
    let id = sim
        .spawn_object("ENGINEER", "Americans", cell.0, cell.1, 0, rules)
        .unwrap();
    sim.mission_assign_exact(
        id,
        MissionId::from_known(MissionType::Move),
        sim.session.binary_frame,
    )
    .unwrap();
    id
}

/// Infantry 0x51AA40(cell, true) through the shared Walk setter owner.
fn order_walk(sim: &mut Simulation, rules: &RuleSet, id: u64, target: (u16, u16)) {
    assert!(
        sim.assign_infantry_destination(
            id,
            crate::sim::components::NavTargetRef::cell(target.0, target.1),
            rules,
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap()
    );
}

/// Eight Buildings around `centre`: the zone labels stay connected while the
/// cell search cannot reach the centre, which is the supplied-NULL core result
/// of the oracle's astar_null rows.
pub(super) fn enclose(sim: &mut Simulation, rules: &RuleSet, centre: (u16, u16)) {
    for dx in -1i32..=1 {
        for dy in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            sim.spawn_object(
                "CABHUT",
                "Soviets",
                (i32::from(centre.0) + dx) as u16,
                (i32::from(centre.1) + dy) as u16,
                0,
                rules,
            )
            .unwrap();
        }
    }
}

#[test]
fn far_failure_stops_walk_clears_target_and_queues_guard_for_a_human_house() {
    let (mut sim, rules, registry) = fixture();
    human_house(&mut sim);
    enclose(&mut sim, &rules, (15, 15));
    let id = engineer_at(&mut sim, &rules, (10, 10));
    let retries_before = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .navigation
        .path_runtime
        .retries_left;
    order_walk(&mut sim, &rules, id, (15, 15));
    let guard = MissionId::from_known(MissionType::Guard);
    let mut frames = 0;
    for _ in 0..30 {
        sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
        frames += 1;
        let mission = sim.substrate.entities.get(id).unwrap().mission;
        if mission.queued() == guard || mission.current() == guard {
            break;
        }
    }
    // 0x4D41D6..0x4D41DF: human -> Queue_Mission(Guard, 0) with Move still
    // current at the return (oracle far row); the per-object AI host then
    // promotes the queued Guard at its Ready->Commence, which the frame loop
    // above may already have run.
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(
        e.mission.current() == guard || e.mission.queued() == guard,
        "no Guard after {frames} frames: {:?}/{:?}",
        e.mission.current(),
        e.mission.queued()
    );
    // 0x51DBB2 Walk Stop, then 0x4D413A SetDestination(NULL,1) and the outer
    // Process cancellation: destination and NavCom are gone.
    assert!(e.locomotor.as_ref().unwrap().walk_destination().is_none());
    assert_eq!(e.locomotor.as_ref().unwrap().walk_is_moving(), Some(false));
    assert!(e.navigation.nav_com.is_none());
    assert!(crate::sim::movement::motion_query::is_moving(&e) != Some(true));
    // 0x4D4149 Assign_Target(NULL).
    assert!(e.attack_target.is_none());
    // A Clear current cell answers 0 at 0x51DBAC.
    assert!(!e.infantry.as_ref().unwrap().cell_entry_blocked);
    // The failure tail never touches +64C; the oracle far row keeps 10.
    assert_eq!(e.navigation.path_runtime.retries_left, retries_before);
    // The NULL setter's Set_Destination_Internal tail rewrote +640 to duration 0.
    assert_eq!(e.navigation.path_runtime.movement_timer.duration(), 0);
    assert_eq!(e.position.rx, 10);
    assert_eq!(e.position.ry, 10);
}

#[test]
fn building_target_redirects_to_a_nearby_cell_and_the_walk_completes_there() {
    let (mut sim, rules, registry) = fixture();
    human_house(&mut sim);
    sim.spawn_object("CABHUT", "Soviets", 13, 10, 0, &rules)
        .unwrap();
    let id = engineer_at(&mut sim, &rules, (10, 10));
    order_walk(&mut sim, &rules, id, (13, 10));
    for _ in 0..400 {
        sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
        let e = sim.substrate.entities.get(id).unwrap();
        if crate::sim::movement::motion_query::is_moving(&e) != Some(true)
            && e.locomotor.as_ref().unwrap().walk_destination().is_none()
        {
            break;
        }
    }
    let e = sim.substrate.entities.get(id).unwrap();
    // Code 7 with a Building (0x47C520) redirected the Find_Path target to an
    // FNPC cell beside the hut; no failure tail ran.
    assert_ne!(
        (e.position.rx, e.position.ry),
        (10, 10),
        "the engineer walked"
    );
    assert_ne!((e.position.rx, e.position.ry), (13, 10));
    assert!(
        (i32::from(e.position.rx) - 13).abs() <= 1 && (i32::from(e.position.ry) - 10).abs() <= 1,
        "ended at {:?}",
        (e.position.rx, e.position.ry)
    );
    // Successful stopped-Move arrival also enters Infantry idle:
    //520F8C ->51CBA0(0,1) ->51CD6E/51CD96 queues human Guard.
    // This is separate from the Find_Path failure tail.
    let guard = MissionId::from_known(MissionType::Guard);
    assert!(e.mission.queued() == guard || e.mission.current() == guard);
    assert!(!e.infantry.as_ref().unwrap().cell_entry_blocked);
}

#[test]
fn obstructed_target_beyond_close_enough_redirects_only_when_the_nearby_cell_is_closer() {
    let (mut sim, rules, registry) = fixture();
    human_house(&mut sim);
    let id = engineer_at(&mut sim, &rules, (10, 10));
    // A code-6 target is occupied (0x51C61F: an infantryman disguised to the
    // actor's House); FNPC's raw-occupation admission then skips the seed
    // itself. The supplied answer stands in for that occupant, and a Building
    // keeps FNPC off the seed cell the same way.
    sim.spawn_object("CABHUT", "Soviets", 13, 10, 0, &rules)
        .unwrap();
    // 0x4D3A9B: 768 leptons > CloseEnough 576 enters the code-6 arm. FNPC from
    // the target picks the passable cell nearest the actor, which lies 256
    // leptons from the target: closer than the actor (0x4D3C39), same zone
    // (EstimateZoneCost = Chebyshev), so SetDestination(cell, 1) retargets.
    order_walk(&mut sim, &rules, id, (13, 10));
    let goal = sim
        .find_path_goal_for_answer(
            id,
            DriveCoord::cell(13, 10, 0),
            6,
            &rules,
            Some(&registry),
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert_eq!((goal.x / 256, goal.y / 256), (12, 10));
    let e = sim.substrate.entities.get(id).unwrap();
    let destination = e.locomotor.as_ref().unwrap().walk_destination().unwrap();
    assert_eq!((destination.x / 256, destination.y / 256), (12, 10));
    assert_eq!(
        e.navigation.nav_com,
        Some(crate::sim::components::NavTargetRef::Cell { rx: 12, ry: 10 })
    );
    // Within CloseEnough the obstructed target is searched unchanged.
    order_walk(&mut sim, &rules, id, (12, 10));
    let goal = sim
        .find_path_goal_for_answer(
            id,
            DriveCoord::cell(12, 10, 0),
            6,
            &rules,
            Some(&registry),
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert_eq!((goal.x / 256, goal.y / 256), (12, 10));
}

#[test]
fn near_failure_returns_before_the_null_setter_and_guard_queue() {
    let (mut sim, rules, registry) = fixture();
    human_house(&mut sim);
    let id = engineer_at(&mut sim, &rules, (10, 10));
    order_walk(&mut sim, &rules, id, (11, 10));
    sim.infantry_stop_driver(
        id,
        &rules,
        Some(&registry),
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    // 0x4D40A6..0x4D40D4: Chebyshev 1 to a non-structural target returns.
    sim.finish_find_path_failure(
        id,
        DriveCoord::cell(11, 10, 0),
        &rules,
        Some(&registry),
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    let e = sim.substrate.entities.get(id).unwrap();
    assert_eq!(e.mission.queued(), MissionId::NONE);
    assert_eq!(
        e.mission.current(),
        MissionId::from_known(MissionType::Move)
    );
    // The receiver's Stop already cleared the Walk destination; NavCom is
    // retained until the outer Process (oracle near row: nav kept, dest 0).
    assert!(e.locomotor.as_ref().unwrap().walk_destination().is_none());
    assert!(e.navigation.nav_com.is_some());
    assert!(!e.infantry.as_ref().unwrap().cell_entry_blocked);
}

/// A Walk destination in the infantryman's own cell reaches Find_Path like
/// any other (Walk has no same-cell arm before the request at 0x0075AFC5),
/// and AStar has no route for a goal in its start cell
/// (0x00429BF3..0x00429C0A). The receiver's Stop clears the destination, the
/// tail's Chebyshev test (0 here, 0x4D40A6) returns before its NULL setter
/// and Guard queue, and the outer Process's recheck of the cleared
/// destination (0x75AFD3) runs the NULL setter.
#[test]
fn own_cell_destination_fails_and_clears_the_order() {
    let (mut sim, rules, registry) = fixture();
    human_house(&mut sim);
    let id = engineer_at(&mut sim, &rules, (10, 10));
    order_walk(&mut sim, &rules, id, (10, 10));
    let grid = sim.path_grid.clone();
    sim.process_ground_locomotor_for_test(id, Some(&rules), grid.as_deref(), Some(&registry))
        .unwrap();
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(e.locomotor.as_ref().unwrap().walk_destination().is_none());
    assert_eq!(e.locomotor.as_ref().unwrap().walk_is_moving(), Some(false));
    assert!(e.navigation.nav_com.is_none());
    assert!(crate::sim::movement::motion_query::is_moving(&e) != Some(true));
    assert_eq!(e.mission.queued(), MissionId::NONE);
    assert_eq!(
        e.mission.current(),
        MissionId::from_known(MissionType::Move)
    );
    assert_eq!((e.position.rx, e.position.ry), (10, 10));
    // The NULL setter's Set_Destination_Internal tail rewrote +640 to
    // duration 0.
    assert_eq!(e.navigation.path_runtime.movement_timer.duration(), 0);
}

#[test]
fn receiver_records_an_impassable_current_cell() {
    let (mut sim, rules, registry) = fixture();
    human_house(&mut sim);
    let id = engineer_at(&mut sim, &rules, (10, 10));
    // The oracle's current_water rows: LandType 2 with Foot speed 0 makes the
    // current-cell Can_Enter_Cell answer 7, so 0x51DBBE stores 1.
    {
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(10, 10)
            .unwrap();
        cell.yr_cell_land_type = 2;
        cell.speed_costs.foot = Some(0);
        cell.base_speed_costs.foot = Some(0);
    }
    order_walk(&mut sim, &rules, id, (11, 10));
    let before = sim.state_hash();
    sim.infantry_stop_driver(
        id,
        &rules,
        Some(&registry),
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(e.infantry.as_ref().unwrap().cell_entry_blocked);
    assert!(e.locomotor.as_ref().unwrap().walk_destination().is_none());
    assert_ne!(sim.state_hash(), before);
}

#[test]
fn receiver_writes_the_ready_action_only_when_do_action_admits_it() {
    let (mut sim, rules, registry) = fixture();
    human_house(&mut sim);
    let id = engineer_at(&mut sim, &rules, (10, 10));
    order_walk(&mut sim, &rules, id, (11, 10));
    // Doing -1 admits request 0 when the type carries a Ready sequence; the
    // fixture rules define no sequences, so 0x51D70F refuses before any write.
    sim.infantry_stop_driver(
        id,
        &rules,
        Some(&registry),
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    let doing = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .mission_leaf
        .as_infantry()
        .unwrap()
        .doing();
    // The gate reads the type's native Ready record (Type `+0xE3C`), whose
    // count the constructor leaves 0 without a `Sequence=`.
    let has_ready = rules
        .animation_sequence("ENGINEER")
        .and_then(|set| set.infantry_action(0))
        .is_some_and(|record| record.frames_per_facing != 0);
    assert_eq!(doing, if has_ready { 0 } else { -1 });
}

/// A Jumpjet man's damage Scatter takes the Infantry setter (`0x0051AA40`) to
/// Foot's (`0x004D94B0`): the NavCom, then the Jumpjet Move_To, then Foot's
/// timer tail. Scatter then runs his locomotor's Process at once
/// (`0x0051D45B..0x0051D478`), which lifts him off (State 0 to 1).
#[test]
fn jumpjet_infantry_damage_scatter_takes_the_infantry_setter() {
    use crate::sim::combat::{EntityDamageEvent, world_receiver};
    use crate::sim::components::NavTargetRef;
    let (mut sim, rules, registry) = super::tests::fixture_with_rules(
        "[JUMPJET]\nFraidycat=yes\n[Guard]\nScatter=yes\n[CombatDamage]\nPlayerScatter=yes\n",
    );
    let victim = sim
        .spawn_object("JUMPJET", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let attacker = engineer_at(&mut sim, &rules, (8, 10));
    sim.mission_assign_exact(
        victim,
        MissionId::from_known(MissionType::Guard),
        sim.session.binary_frame,
    )
    .unwrap();
    sim.substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(-1)
        .unwrap();
    let attacker_house = sim.substrate.entities.get(attacker).unwrap().owner();
    let warhead = sim.interner.intern("SA");
    let event = EntityDamageEvent::area(victim, 10, 0, attacker, Some(attacker_house), warhead);
    world_receiver::commit_entities(
        &mut sim,
        &mut world_receiver::ReceiverRun::default(),
        &[event],
        None,
        &rules,
        Some(&registry),
    );
    let e = sim.substrate.entities.get(victim).unwrap();
    assert!(e.health.current < 125);
    assert!(
        matches!(e.navigation.nav_com, Some(NavTargetRef::Cell { .. })),
        "Scatter published the cell NavCom"
    );
    let state = e.locomotor.as_ref().unwrap().jumpjet_runtime().unwrap();
    assert!(state.moving(), "Jumpjet Move_To accepted the scatter cell");
    assert_eq!(
        state.phase(),
        1,
        "Scatter's immediate Process lifted him off"
    );
    assert_eq!(e.mission.queued(), MissionId::from_known(MissionType::Move));
}

#[test]
fn infantry_damage_scatter_reaches_the_ordinary_walk_process() {
    use crate::sim::combat::{EntityDamageEvent, world_receiver};
    use crate::sim::components::NavTargetRef;
    use crate::sim::movement::ground_pose::position_world_coord;
    let (mut sim, rules, registry) = super::tests::fixture_with_rules(
        "[General]\nFixtureOnly=1\n[ENGINEER]\nFraidycat=yes\n[Guard]\nScatter=yes\n[CombatDamage]\nPlayerScatter=yes\n",
    );
    assert!(rules.object("ENGINEER").unwrap().fraidycat);
    assert!(
        rules.general.player_scatter,
        "human Scatter input was parsed"
    );
    let owner = sim.interner.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
    );
    let victim = engineer_at(&mut sim, &rules, (10, 10));
    let attacker = engineer_at(&mut sim, &rules, (8, 10));
    sim.mission_assign_exact(
        victim,
        MissionId::from_known(MissionType::Guard),
        sim.session.binary_frame,
    )
    .unwrap();
    let e = sim.substrate.entities.get_mut(victim).unwrap();
    e.mission_leaf.set_infantry_doing_verified(-1).unwrap();
    // A Target for the hit, so TechnoClass::ReceiveDamage's own scatter tail
    // refuses (`0x00702C58`) and the source Scatter is the only one. That
    // Scatter reads it only in a non-Fraidycat gate (`0x0051D19A`) and in
    // Can_Enter_Cell against candidate occupants (`0x0051C3C2`); the
    // attacker is two cells away.
    e.attack_target = Some(crate::sim::combat::AttackTarget::new(attacker));
    let before = position_world_coord(&e.position);
    let facing = e.body_facing;
    let attacker_house = sim.substrate.entities.get(attacker).unwrap().owner();
    let warhead = sim.interner.intern("SA");
    let event = EntityDamageEvent::area(victim, 10, 0, attacker, Some(attacker_house), warhead);
    world_receiver::commit_entities(
        &mut sim,
        &mut world_receiver::ReceiverRun::default(),
        &[event],
        None,
        &rules,
        Some(&registry),
    );
    let e = sim.substrate.entities.get(victim).unwrap();
    assert_eq!(e.health.current, 65);
    assert_eq!(e.body_facing, facing, "Scatter setter does not snap facing");
    assert_eq!(
        position_world_coord(&e.position),
        before,
        "no immediate Process for source Scatter"
    );
    let Some(NavTargetRef::Cell { rx, ry }) = e.navigation.nav_com else {
        panic!("scatter cell");
    };
    assert_eq!(
        e.locomotor.as_ref().unwrap().walk_destination(),
        Some(DriveCoord::cell(rx, ry, 0))
    );
    assert!(
        e.navigation.path_replay.remaining_directions().is_empty(),
        "first path belongs to Process"
    );
    assert_eq!(e.locomotor.as_ref().unwrap().step_head(), None);
    assert_eq!(e.mission.queued(), MissionId::from_known(MissionType::Move));
    assert_eq!(e.infantry.as_ref().unwrap().fear_level, 300);
    // Isolate the damage-created destination's ordinary Walk continuation.
    // The independently native-compared5200B0 panic tail can issue another
    // NULL Scatter immediately on arrival while this Fraidycat's fear>50.
    let e = sim.substrate.entities.get_mut(victim).unwrap();
    e.infantry.as_mut().unwrap().fear_level = 0;
    e.attack_target = None;
    // The same production tick host that handles normal orders must consume
    // the damage-created request. The native corpus proves the first FindPath
    // boundary; this regression continues through our real search/head/step.
    sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
    let e = sim.substrate.entities.get(victim).unwrap();
    assert_eq!(position_world_coord(&e.position), before);
    assert!(e.locomotor.as_ref().unwrap().step_head().is_some());
    sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
    assert_ne!(
        position_world_coord(&sim.substrate.entities.get(victim).unwrap().position),
        before
    );
    for _ in 0..200 {
        sim.advance_tick(&[], Some(&rules), None, Some(&registry), 67);
        if crate::sim::movement::motion_query::is_moving(
            sim.substrate.entities.get(victim).unwrap(),
        ) != Some(true)
        {
            break;
        }
    }
    let e = sim.substrate.entities.get(victim).unwrap();
    assert_eq!((e.position.rx, e.position.ry), (rx, ry));
    assert!(e.navigation.nav_com.is_none());
    assert!(e.locomotor.as_ref().unwrap().walk_destination().is_none());
}

/// FootClass::Limbo (0x004DB260) runs Infantry +0x500 on the first Limbo:
/// the receiver's +0x6DC answer is written and Walk Stop clears the moving
/// byte, which the Lock that follows leaves alone.
#[test]
fn first_limbo_runs_the_infantry_stop_driver() {
    let (mut sim, rules, registry) = fixture();
    human_house(&mut sim);
    let id = engineer_at(&mut sim, &rules, (10, 10));
    {
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(10, 10)
            .unwrap();
        cell.yr_cell_land_type = 2;
        cell.speed_costs.foot = Some(0);
        cell.base_speed_costs.foot = Some(0);
    }
    order_walk(&mut sim, &rules, id, (11, 10));
    let _ = sim.techno_limbo_with_rules(id, &rules, Some(&registry));
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(e.infantry.as_ref().unwrap().cell_entry_blocked);
    let loco = e.locomotor.as_ref().unwrap();
    assert_eq!((loco.walk_destination(), loco.step_head()), (None, None));
    assert_eq!(loco.walk_is_moving(), Some(false));
}
