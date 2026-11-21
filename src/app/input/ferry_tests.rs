//! Ferry order tests: when a loading transport leaves, the legs of a round
//! trip, and one whole trip through the simulation.

use super::{
    ARRIVE_CELLS, CHECK_INTERVAL, DROP_RADIUS_CELLS, FerryAction, FerryRoute, FerryState, Leg,
    QUIET_TICKS, RESEND_TICKS, TransportFacts, within,
};
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

const PICKUP: (u16, u16) = (3, 5);
const DROP: (u16, u16) = (13, 5);

fn route_for(
    sim: &Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner: crate::sim::intern::InternedId,
    transport: u64,
    clicked: (u16, u16),
    now: u64,
) -> Option<FerryRoute> {
    super::route_for(
        sim,
        &sim.control_projection(),
        rules,
        owner,
        transport,
        clicked,
        now,
    )
}

#[test]
fn a_previous_worlds_ferry_cannot_board_units_in_a_fresh_world() {
    // Without retail data (RA2_DIR) the fixture prints SKIPPED; each scene
    // then loads its own copy.
    if crate::rules::retail_ini_fixture::retail_battle_rules().is_none() {
        return;
    }
    let scene = || {
        let retail = crate::rules::retail_ini_fixture::retail_battle_rules()
            .expect("retail rules are required for the ferry isolation regression");
        let (mut sim, rules, gi) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let owner = sim.interner().get("Local").unwrap();
        let transport = sim.spawn_object("LCRF", "Local", 4, 8, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        (sim, rules, owner, transport, gi)
    };
    let (old_sim, old_rules, old_owner, old_transport, _) = scene();
    let mut input = crate::app::input::state::MatchInputState::new(Default::default());
    input
        .ferry
        .add(route_for(&old_sim, &old_rules, old_owner, old_transport, (13, 2), 0).unwrap());

    input.reset_targeting_for_new_world();
    let (fresh, rules, owner, transport, gi) = scene();
    assert_eq!(transport, old_transport, "the new world reuses object ids");
    assert!(
        input.ferry.check(&fresh, &rules, owner, 0).is_empty(),
        "a prior match's ferry must not issue commands in the new world"
    );

    // A route explicitly requested in this world still boards the nearby GI.
    input
        .ferry
        .add(route_for(&fresh, &rules, owner, transport, (13, 2), 0).unwrap());
    assert!(
        input
            .ferry
            .check(&fresh, &rules, owner, 0)
            .iter()
            .any(|order| matches!(order,
        Command::MegaMission { entity_id, mission: MissionType::Enter,
            destination: Some(TargetKind::Entity(carrier)), .. }
        if *entity_id == gi && *carrier == transport))
    );
}

fn facts(cell: (u16, u16), idle: bool, passengers: u32) -> TransportFacts {
    TransportFacts {
        cell,
        idle,
        passengers,
        full: false,
        boarding: false,
        heading: None,
        reach: 2,
    }
}

#[test]
fn a_loading_transport_leaves_when_full_or_after_a_quiet_spell() {
    let mut route = FerryRoute::new(7, PICKUP, DROP, 0, 0);
    // Empty, it waits however long.
    assert_eq!(route.step(&facts(PICKUP, true, 0), 1000), FerryAction::None);
    // Someone on the way aboard holds it and restarts the quiet spell.
    let boarding = TransportFacts {
        boarding: true,
        ..facts(PICKUP, true, 3)
    };
    assert_eq!(route.step(&boarding, 1008), FerryAction::None);
    let loaded = facts(PICKUP, true, 3);
    assert_eq!(
        route.step(&loaded, 1008 + QUIET_TICKS - 1),
        FerryAction::None
    );
    assert_eq!(
        route.step(&loaded, 1008 + QUIET_TICKS),
        FerryAction::MoveTo(DROP)
    );

    // A full hold leaves at once, whoever is still trying to board.
    let mut route = FerryRoute::new(7, PICKUP, DROP, 0, 0);
    let full = TransportFacts {
        full: true,
        ..boarding
    };
    assert_eq!(route.step(&full, 8), FerryAction::MoveTo(DROP));
}

#[test]
fn a_round_trip_unloads_and_returns_and_a_detour_ends_it() {
    let full = TransportFacts {
        full: true,
        ..facts(PICKUP, true, 4)
    };
    let mut route = FerryRoute::new(7, PICKUP, DROP, 4, 0);
    assert_eq!(route.step(&full, 8), FerryAction::MoveTo(DROP));
    assert_eq!(route.step(&facts((9, 5), false, 4), 16), FerryAction::None);
    // Stopped within reach of the drop spot: unload.
    assert_eq!(
        route.step(&facts((12, 5), true, 4), 24),
        FerryAction::Unload
    );
    assert_eq!(route.step(&facts((12, 5), false, 2), 32), FerryAction::None);
    // Empty and at rest once the order has had its time: go back.
    assert_eq!(route.step(&facts((12, 5), true, 0), 40), FerryAction::None);
    assert_eq!(
        route.step(&facts((12, 5), true, 0), 24 + RESEND_TICKS),
        FerryAction::MoveTo(PICKUP)
    );
    assert_eq!(route.step(&facts((8, 5), false, 0), 100), FerryAction::None);
    // Back at the pick-up spot it loads again and the next full hold leaves.
    assert_eq!(route.step(&facts(PICKUP, true, 0), 108), FerryAction::None);
    assert_eq!(route.step(&full, 116), FerryAction::MoveTo(DROP));

    // Passengers left aboard after an unload: cross again, to find them room.
    let mut stuck = FerryRoute::new(7, PICKUP, DROP, 4, 0);
    assert_eq!(stuck.step(&full, 8), FerryAction::MoveTo(DROP));
    assert_eq!(stuck.step(&facts((9, 5), false, 4), 16), FerryAction::None);
    assert_eq!(stuck.step(&facts(DROP, true, 4), 24), FerryAction::Unload);
    assert_eq!(
        stuck.step(&facts(DROP, true, 1), 24 + RESEND_TICKS),
        FerryAction::MoveTo(DROP)
    );

    // Through a waypoint: sent on as it comes near, never told to stop
    // there, and back through it on the return.
    let mut detour = FerryRoute::new(7, PICKUP, DROP, 4, 0).with_via(vec![(8, 12)]);
    assert_eq!(detour.step(&full, 8), FerryAction::MoveVia((8, 12)));
    assert_eq!(detour.step(&facts((6, 9), false, 4), 16), FerryAction::None);
    assert_eq!(
        detour.step(&facts((8, 11), false, 4), 24),
        FerryAction::MoveTo(DROP)
    );
    assert_eq!(detour.step(&facts(DROP, true, 4), 200), FerryAction::Unload);
    assert_eq!(
        detour.step(&facts(DROP, true, 0), 200 + RESEND_TICKS),
        FerryAction::MoveVia((8, 12))
    );
    assert_eq!(
        detour.step(&facts((8, 12), false, 0), 400),
        FerryAction::MoveTo(PICKUP)
    );

    // Three ways past a waypoint that the transport never comes within two
    // cells of. Sent there with others, its Move was given a cell beside the
    // waypoint, and that cell counts.
    let beside = |cell, idle| TransportFacts {
        heading: Some((8, 16)),
        ..facts(cell, idle, 4)
    };
    let mut spread = FerryRoute::new(7, PICKUP, DROP, 4, 0).with_via(vec![(8, 12)]);
    assert_eq!(spread.step(&full, 8), FerryAction::MoveVia((8, 12)));
    assert_eq!(spread.step(&beside((8, 19), false), 16), FerryAction::None);
    assert_eq!(
        spread.step(&beside((8, 15), false), 24),
        FerryAction::MoveTo(DROP)
    );
    // Stopped as near as its Move took it: it carries on, not ends.
    let mut stopped = FerryRoute::new(7, PICKUP, DROP, 4, 0).with_via(vec![(8, 12)]);
    assert_eq!(stopped.step(&full, 8), FerryAction::MoveVia((8, 12)));
    assert_eq!(
        stopped.step(&facts((8, 16), false, 4), 16),
        FerryAction::None
    );
    assert_eq!(
        stopped.step(&facts((8, 16), true, 4), 100),
        FerryAction::MoveTo(DROP)
    );
    // An aircraft is sent on from farther out, before it slows to land.
    let flying = TransportFacts {
        reach: 4,
        ..facts((8, 16), false, 4)
    };
    let mut air = FerryRoute::new(7, PICKUP, DROP, 4, 0).with_via(vec![(8, 12)]);
    assert_eq!(air.step(&full, 8), FerryAction::MoveVia((8, 12)));
    assert_eq!(air.step(&flying, 16), FerryAction::MoveTo(DROP));
    // Stopped far from the waypoint, it was sent elsewhere: the ferry ends.
    let mut elsewhere = FerryRoute::new(7, PICKUP, DROP, 4, 0).with_via(vec![(8, 12)]);
    assert_eq!(elsewhere.step(&full, 8), FerryAction::MoveVia((8, 12)));
    assert_eq!(
        elsewhere.step(&facts((20, 30), false, 4), 16),
        FerryAction::None
    );
    assert_eq!(
        elsewhere.step(&facts((20, 30), true, 4), 100),
        FerryAction::End
    );

    // Moving, then stopped away from the drop spot: the ferry is over.
    assert_eq!(route.step(&facts((6, 9), false, 4), 124), FerryAction::None);
    assert_eq!(route.step(&facts((6, 12), true, 4), 132), FerryAction::End);
}

#[test]
fn ferry_and_autoload_accept_only_locally_delegated_carriers_and_passengers() {
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel};
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_session::ScenarioGameMode;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    let local = sim.interner().get("Local").unwrap();
    let delegated = sim.intern("Delegated");
    sim.register_house(HouseState::new(delegated, 1, None, false, 0, 10));
    let delegated_index = sim.houses().index_of(delegated).unwrap() as u8;
    let transport = sim
        .spawn_object("LCRF", "Delegated", 4, 8, 0, &rules)
        .unwrap();
    let passenger = sim
        .spawn_object("E1", "Delegated", 5, 8, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    sim.install_session_mode(ScenarioGameMode::Lan);
    let now = sim.session().tick();
    assert!(route_for(&sim, &rules, local, transport, (13, 2), now).is_none());
    assert!(
        crate::app::input::autoload::autoload_orders(&sim, &rules, &[passenger, transport])
            .is_empty()
    );

    let frame = sim.session().binary_frame();
    sim.session_mut_for_test().set_binary_frame_for_test(0);
    sim.install_control_grants(vec![ControlGrant {
        source: 0,
        house: delegated_index,
        level: ControlGrantLevel::UnitsOnly,
    }])
    .unwrap();
    sim.session_mut_for_test().set_binary_frame_for_test(frame);
    let mut ferry = FerryState::default();
    ferry.add(route_for(&sim, &rules, local, transport, (13, 2), now).unwrap());
    let boards = |orders: &[Command]| {
        orders.iter().any(|order| {
            matches!(order, Command::MegaMission {
            entity_id, mission: MissionType::Enter,
            destination: Some(TargetKind::Entity(target)), ..
        } if *entity_id == passenger && *target == transport)
        })
    };
    assert!(boards(&ferry.check(&sim, &rules, local, now)));
    assert!(boards(&crate::app::input::autoload::autoload_orders(
        &sim,
        &rules,
        &[passenger, transport]
    )));
}

/// One whole trip through the simulation, on a 16 x 16 scene with water on
/// columns 8..=10: the GI at rest inside the circle is told aboard, the
/// transport crosses, sets him down on the far bank and comes back to load.
#[test]
fn a_ferry_carries_a_unit_across_the_river_and_comes_back() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    // The scene's own GI stands at (5, 5), inside the circle around the
    // pick-up spot. The far bank's cells lie more than seven cells away.
    let (mut sim, rules, gi) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    let owner = sim.interner().get("Local").unwrap();
    let pickup = (4, 8);
    let transport = sim
        .spawn_object("LCRF", "Local", pickup.0, pickup.1, 0, &rules)
        .expect("retail amphibious transport spawns");
    // Outside the circle: never told to board.
    sim.spawn_object("E1", "Local", 5, 13, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);

    let start = sim.session().tick();
    // A drop spot beside the pick-up circle is refused.
    assert!(route_for(&sim, &rules, owner, transport, (6, 4), start).is_none());
    let mut ferry = FerryState::default();
    ferry.add(
        route_for(&sim, &rules, owner, transport, (13, 2), start)
            .expect("a route across the river"),
    );

    let mut told_aboard = Vec::new();
    let (mut set_down, mut back) = (false, false);
    for _ in 0..2000 {
        let now = sim.session().tick();
        if now % CHECK_INTERVAL == 0 {
            for order in ferry.check(&sim, &rules, owner, now) {
                if let Command::MegaMission {
                    entity_id,
                    mission: MissionType::Enter,
                    destination: Some(TargetKind::Entity(carrier)),
                    ..
                } = order
                {
                    told_aboard.push((entity_id, carrier));
                }
                sim.queue_command(CommandEnvelope::new(owner, now, order));
            }
        }
        let due = sim.take_due_commands();
        let result = sim.advance_tick(&due, Some(&rules), None, None, 67);
        assert!(result.frame_committed, "{result:?}");
        set_down |= sim.entities().get(gi).is_some_and(|rider| {
            !rider.passenger_role().is_inside_transport() && rider.position.rx() > 10
        });
        let carrier = sim
            .entities()
            .get(transport)
            .expect("the transport lives")
            .position;
        back = set_down
            && ferry
                .routes
                .first()
                .is_some_and(|route| route.leg == Leg::Loading)
            && within((carrier.rx(), carrier.ry()), pickup, ARRIVE_CELLS);
        if back {
            break;
        }
    }
    assert_eq!(told_aboard, [(gi, transport)]);
    assert!(set_down, "the GI was set down on the far bank");
    assert!(back, "the transport came back to load: {:?}", ferry.routes);
}

/// A land trip on the flat arena for the other carriers: a Flak Track and a
/// Nighthawk each take the GI standing in the circle to the drop zone and
/// come back to load. A tank stands on the drop spot, so each heads for a
/// free cell beside it.
#[test]
fn a_flak_track_and_a_nighthawk_ferry_too() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    for carrier in ["HTK", "SHAD"] {
        let mut sim = Simulation::new();
        for (name, side) in [("Americans", 0), ("Russians", 1)] {
            let house = sim.intern(name);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    house, side, None, true, 0, 10,
                ));
        }
        let owner = sim.interner().get("Americans").unwrap();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        crate::sim::arena_fixture::flat_arena(&mut sim, rules);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        // Each House keeps a building, so the skirmish has no winner.
        sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
            .unwrap();
        sim.spawn_object("GAPOWR", "Americans", 16, 26, 0, rules)
            .unwrap();
        let pickup = (10, 18);
        let transport = sim
            .spawn_object(carrier, "Americans", pickup.0, pickup.1, 0, rules)
            .unwrap_or_else(|| panic!("{carrier} spawns"));
        let gi = sim
            .spawn_object("E1", "Americans", 12, 18, 0, rules)
            .unwrap();
        sim.resolve_type_handles(rules);

        let start = sim.session().tick();
        let mut ferry = FerryState::default();
        ferry.add(
            route_for(&sim, rules, owner, transport, (24, 18), start)
                .unwrap_or_else(|| panic!("{carrier} takes the route")),
        );
        let drop = (24, 18);
        sim.spawn_object("MTNK", "Americans", drop.0, drop.1, 0, rules)
            .unwrap();
        sim.resolve_type_handles(rules);
        let mut crossing_to = None;
        let (mut set_down, mut back) = (false, false);
        for _ in 0..3000 {
            let now = sim.session().tick();
            if now % CHECK_INTERVAL == 0 {
                for order in ferry.check(&sim, rules, owner, now) {
                    if let Command::Move {
                        target_rx,
                        target_ry,
                        ..
                    } = order
                    {
                        crossing_to.get_or_insert((target_rx, target_ry));
                    }
                    sim.queue_command(CommandEnvelope::new(owner, now, order));
                }
            }
            let due = sim.take_due_commands();
            let result = sim.advance_tick(&due, Some(rules), None, None, 67);
            assert!(result.frame_committed, "{result:?}");
            set_down |= sim.entities().get(gi).is_some_and(|rider| {
                !rider.passenger_role().is_inside_transport() && rider.position.rx() > 18
            });
            let entity = sim.entities().get(transport).expect("the transport lives");
            back = set_down
                && ferry
                    .routes
                    .first()
                    .is_some_and(|route| route.leg == Leg::Loading)
                && within(
                    (entity.position.rx(), entity.position.ry()),
                    pickup,
                    ARRIVE_CELLS,
                );
            if back {
                break;
            }
        }
        let crossing_to = crossing_to.expect("a crossing was ordered");
        assert!(
            crossing_to != drop && within(crossing_to, drop, DROP_RADIUS_CELLS),
            "{carrier} crosses to a free cell of the drop zone: {crossing_to:?}"
        );
        assert!(set_down, "{carrier} set the GI down: {:?}", ferry.routes);
        assert!(back, "{carrier} came back to load: {:?}", ferry.routes);
    }
}
