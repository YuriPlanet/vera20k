// A landed vehicle keeps the erase head, and Teleport state 2 has already
// cleared its own +270. These are inputs to the historical writer.
fn write_temporal(path: &std::path::Path) {
    fn actors(world: &sim::world::Simulation) -> (u64, u64) {
        let id = |name| {
            world
                .entities()
                .iter_sorted()
                .find_map(|(id, object)| {
                    (world.interner().resolve(object.type_ref()) == name).then_some(id)
                })
                .unwrap()
        };
        (id("APC"), id("LEG"))
    }
    let scenario =
        writer::Scenario {
            ini: "[Map]\nTheater=TEMPERATE\n[Basic]\nName=Compatibility\n\
              [InfantryTypes]\n0=LEG\n[LEG]\nStrength=100\nSpeed=4\nPrimary=ERASE\n\
              [ERASE]\nDamage=8\nRange=5\nROF=120\nProjectile=INVIS\nWarhead=BEAM\n\
              [INVIS]\nInviso=yes\n[Warheads]\n0=BEAM\n[BEAM]\nTemporal=yes\n\
              [Units]\n0=Americans,APC,256,1,0,0,Sleep,None,0,-1,0,-1,1,1\n\
              [Infantry]\n0=Americans,LEG,256,0,1,2,Sleep,0,None,0,-1,0,1,1\n",
            populate: |world, rules, ini| {
                // The two ground units keep their House alive through the saved turn.
                world
                    .session_mut_for_test()
                    .game_options_mut_for_test()
                    .short_game = false;
                let rows = map::entities::parse_map_entities(ini, 4);
                assert_eq!(world.spawn_from_map(&rows, Some(rules)), 2);
                let (tank, firer) = actors(world);
                // The firer still has the ordinary Attack order which produced
                // the shot. An idle firer would release the beam on its first turn.
                assert!(world.apply_command(
                    "Americans",
                    &sim::command::Command::Attack {
                        attacker_id: firer,
                        target_id: tank,
                    },
                    Some(rules)
                ));
                let mut_objects = world.entities_mut_for_test();
                let shooter = mut_objects.get_mut_for_test(firer).unwrap();
                assert!(shooter.temporal.has_link());
                shooter.set_temporal_for_test(serde_json::from_value(serde_json::json!({
                "link": {"target": tank, "prev": null, "next": null, "warp_remaining": 1000},
                "head": null,
            })).unwrap());
                let victim = mut_objects.get_mut_for_test(tank).unwrap();
                victim.temporal = sim::temporal::TemporalState::warped_by_for_test(firer);
                assert!(!victim.is_warped_out());
            },
            verify: |world, advanced| {
                let (tank, firer) = actors(world);
                let victim = world.entities().get(tank).unwrap();
                let shot = &world.entities().get(firer).unwrap().temporal;
                assert_eq!(victim.temporal.chain_head(), Some(firer));
                assert!(!victim.is_warped_out() && !victim.ai_frozen());
                assert_eq!(shot.warp_target(), Some(tank));
                assert_eq!(
                    shot.warp_remaining(),
                    Some(if advanced { 992 } else { 1000 })
                );
            },
        };
    writer::write_world(
        sim::world::Simulation::with_seed(71),
        map::resolved_terrain::test_flat_ground_grid(3),
        path,
        sim::scenario_session::ScenarioGameMode::Skirmish,
        scenario,
    );
}
