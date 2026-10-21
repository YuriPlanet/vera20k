use super::*;

#[test]
fn home_action_uses_current_player_and_preserves_no_target_success() {
    let (mut sim, rules, registry, houses) = tests::event_ingress_fixture_with_rules(
        "[BuildingTypes]\n0=YARD\n[YARD]\nStrength=500\nConstructionYard=yes\nFoundation=1x1\n",
    );
    sim.session.set_current_house_for_test(Some(houses[0]));
    let local = sim
        .spawn_object("YARD", "House0", 15, 15, 0, &rules)
        .unwrap();
    sim.spawn_object("YARD", "House1", 19, 15, 0, &rules)
        .unwrap();
    tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nHOME=Country7,<none>,Home,0,1,1,1,0\n\
         [Events]\nHOME=1,8,0,0\n[Actions]\nHOME=1,145,0,12345,0,0,0,0,A\n\
         [Tags]\nHOMETAG=2,Home,HOME\n",
    );
    let rng = sim.rng_state();
    let point = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(local).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    sim.session.set_binary_frame_for_test(8);
    sim.raise_house_tags(houses[1], &rules, Some(&registry), FrameEffects::default());
    assert_eq!(
        sim.drain_trigger_effects(),
        [TriggerEffect::HomeCamera {
            coord: Some([point.x, point.y, point.z]),
        }]
    );
    assert_eq!(sim.rng_state(), rng);
    let live = sim.trigger_runtime.trigger_order[0];
    let type_index = sim.trigger_runtime.trigger(live).unwrap().type_index;
    let action = sim.trigger_runtime.definitions.trigger_types[type_index].actions[0].clone();
    sim.session.set_current_house_for_test(None);
    let mut effects = Vec::new();
    assert!(sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: Some(houses[1]),
            input: TagEventInput::default(),
        },
        &mut effects,
        &rules,
        Some(&registry),
        FrameEffects::default()
    ));
    assert_eq!(effects, [TriggerEffect::HomeCamera { coord: None }]);
    assert_eq!(sim.rng_state(), rng);
}
