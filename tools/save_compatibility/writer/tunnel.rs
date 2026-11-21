pub fn historical_tunnel() -> (Simulation, ResolvedTerrainGrid) {
    let (mut sim, rules) = unit_driving_into(0);
    sim.set_next_stable_object_id_for_test(2);
    let terrain = sim.resolved_terrain.take().unwrap();
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    sim.session.set_map_width_for_test(4);
    sim.session.set_map_height_for_test(1);
    let owner = sim.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 1000, 10);
    house.set_player_control_for_test(true);
    sim.houses.register(house);
    sim.session.set_current_house_for_test(Some(owner));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .seldom
        .in_logic_vector
        .set_for_test(true);
    sim.substrate
        .logic
        .try_push(1, crate::sim::world::ObjectKind::Entity)
        .unwrap();
    assert!(
        sim.track_tube_head(
            1,
            TrackFamily::Drive,
            &rules,
            None,
            &mut FrameEffects::default()
        )
        .unwrap()
    );
    sim.tube_object_turn(1, &rules, None, &mut FrameEffects::default())
        .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .low_bridge_tube_state
            .is_some()
    );
    (sim, terrain)
}
