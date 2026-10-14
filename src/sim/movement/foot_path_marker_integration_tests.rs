//! Rust production integration for the existing42ACF0 marker owner.
//! Native marker arithmetic/ordering has its own oracle; this fixture checks
//! that the modern Foot Find_Path core actually consumes those owned marks.
use super::*;

#[test]
fn marked_probe_survives_modern_foot_search_and_mark_restoration() {
    // On empty ground, urgency2 toggles the body-facing probe. A* may pass it
    // at its4x cost, while both native finishing passes must reject it. The
    // unmarked urgency0 route exercises the same producer/consumer corridor.
    // This is a Rust integration contrast, not a native whole-route golden.
    for urgency in [0, 2] {
        let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=400\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
        );
        let start = (13, 15);
        let probe = (14, 15);
        let goal_cell = (17, 15);
        let id = sim
            .spawn_object("MTNK", "Americans", start.0, start.1, 0, &rules)
            .unwrap();
        let frame = sim.session.binary_frame;
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        actor.body_facing.snap(0x4000, frame);
        let goal = cell_centre((goal_cell.0 as i16, goal_cell.1 as i16));
        let request = FootPathRequest::track(
            &sim.substrate.entities,
            id,
            goal,
            urgency,
            sim.playfield_bounds,
        )
        .unwrap();
        let rng = sim.rng_state();
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .lifecycle
                .cell_marked
        );
        assert!(
            sim.substrate
                .occupancy
                .contains_entity(start.0, start.1, id)
        );
        assert!(matches!(
            sim.search_foot_path(
                &request,
                goal,
                &rules,
                Some(&registry),
                crate::sim::world::FrameEffects::default()
            ),
            Ok(Ok(()))
        ));
        assert_eq!(sim.rng_state(), rng);
        let actor = sim.substrate.entities.get(id).unwrap();
        assert!(actor.lifecycle.cell_marked, "Mark1 must restore the actor");
        assert!(
            sim.substrate
                .occupancy
                .contains_entity(start.0, start.1, id)
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(start.0, start.1)
                & 0x20,
            0x20
        );
        // Find_Path installs the finished route as Foot+5E0 words from the
        // current cell.
        let route = &actor.navigation.path_replay.route_cells();
        assert_eq!(route.first().copied(), Some(start));
        assert_eq!(route.last().copied(), Some(goal_cell));
        if urgency == 0 {
            assert_eq!(route.get(1).copied(), Some(probe));
        } else {
            assert!(
                !route.contains(&probe),
                "search/finishing discarded its owned marker: {route:?}"
            );
        }
        // A later ordinary request must not inherit the temporary overlay.
        if urgency == 2 {
            let next =
                FootPathRequest::track(&sim.substrate.entities, id, goal, 0, sim.playfield_bounds)
                    .unwrap();
            assert!(matches!(
                sim.search_foot_path(
                    &next,
                    goal,
                    &rules,
                    Some(&registry),
                    crate::sim::world::FrameEffects::default()
                ),
                Ok(Ok(()))
            ));
            let actor = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                actor.navigation.path_replay.route_cells().get(1).copied(),
                Some(probe)
            );
            assert_eq!(sim.rng_state(), rng);
        }
    }
}
