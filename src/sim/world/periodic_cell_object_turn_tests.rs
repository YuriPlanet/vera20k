//! Periodic59's position relative to the existing locomotor/Arrival owners.
use super::*;

#[test]
fn high_flight_cell_tag_is_queried_before_the_locomotor_crosses_the_threshold() {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let actor = sim
        .spawn_object_with_overlay_registry("JUMPJET", "Americans", 15, 15, 0, &rules, &registry)
        .unwrap();
    assert!(sim.apply_command(
        "Americans",
        &crate::sim::command::Command::Move {
            entity_id: actor,
            target_rx: 23,
            target_ry: 15,
        },
        Some(&rules)
    ));
    let map = crate::map::map_file::MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n\
          [Triggers]\nAIR=Americans,<none>,Air,0,1,1,1,0\n\
          [Events]\nAIR=1,59,0,-1\n[Actions]\nAIR=1,28,0,7,0,0,0,0,A\n\
          [Tags]\nAIR_TAG=2,Air,AIR\n[CellTags]\n15015=AIR_TAG\n",
    )
    .unwrap();
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.install_authored_cell_trigger_tags(&map);
    sim.classify_map_triggers(&rules);
    // Controlled prior at a real ascent: the production integration test
    // separately establishes natural construction, liftoff and hover.
    sim.advance_live_object_turn(actor, &rules, techno_ai::ObjectAiCtx::default())
        .unwrap();
    sim.set_object_z(
        actor,
        200,
        &rules,
        Some(&registry),
        super::super::FrameEffects::default(),
    );
    sim.session.set_binary_frame_for_test(16);
    sim.clear_tag_dispatch_trace_for_test();
    sim.advance_live_object_turn(actor, &rules, techno_ai::ObjectAiCtx::default())
        .unwrap();
    assert!(
        sim.tag_dispatch_trace_for_test().is_empty(),
        "this visit's pre-Process height is below208"
    );
    assert!(
        crate::sim::movement::air_movement::is_high_flying(
            sim.substrate.entities.get(actor).unwrap(),
            sim.resolved_terrain.as_ref(),
            Some((&rules, &sim.interner)),
        ),
        "Process crosses208 after the periodic query"
    );
    sim.advance_live_object_turn(actor, &rules, techno_ai::ObjectAiCtx::default())
        .unwrap();
    assert_eq!(
        sim.tag_dispatch_trace_for_test()
            .iter()
            .map(|(_, kind)| *kind)
            .collect::<Vec<_>>(),
        vec![59]
    );
    assert!(sim.global_variables()[7]);
    // A real PerCell Arrival is a separate caller even at the same frame.
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        &rules,
        Some(&registry),
        super::super::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        sim.tag_dispatch_trace_for_test()
            .iter()
            .map(|(_, kind)| *kind)
            .collect::<Vec<_>>(),
        vec![59, 1, 59]
    );
}
