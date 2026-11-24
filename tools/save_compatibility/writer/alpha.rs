// The v0.4.0 object Unlimbo and pointer expiry create the two historical
// records; the normal save occurs before the next tick purges the first.
fn write_alpha(path: &std::path::Path) {
    let scenario = writer::Scenario {
        ini: "[Map]\nTheater=TEMPERATE\n[Basic]\nName=Compatibility\n\
              [APC]\nAlphaImage=COMPATGLOW\n\
              [Units]\n0=Americans,APC,256,1,0,0,None,None,None,-1,0\n\
              1=Americans,APC,256,0,1,0,None,None,None,-1,0\n",
        populate: |sim, rules, ini| {
            let units = map::entities::parse_map_entities(ini, 4);
            assert_eq!(sim.spawn_from_map(&units, Some(rules)), 2);
            let origins: Vec<_> = sim.alpha_shapes().collect();
            assert_eq!(origins.len(), 2);
            assert_ne!(origins[0].0, origins[1].0);
            let first = sim
                .entities()
                .values()
                .find(|e| e.position.rx() == 1)
                .unwrap()
                .stable_id();
            sim.detach_all_pointer_expired(first, rules, &mut sim::world::FrameEffects::default());
        },
        verify: |sim, advanced| {
            let glows: Vec<_> = sim.alpha_shapes().collect();
            assert_eq!(glows.len(), if advanced { 1 } else { 2 });
            assert!(glows.iter().all(|(_, name)| *name == "COMPATGLOW"));
            println!("v0.4.0 tick {}: {glows:?}", sim.session().tick());
        },
    };
    writer::write_world(
        sim::world::Simulation::with_seed(17),
        map::resolved_terrain::test_flat_ground_grid(3),
        path,
        sim::scenario_session::ScenarioGameMode::Skirmish,
        scenario,
    );
}
