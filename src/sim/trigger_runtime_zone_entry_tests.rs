//! Foot arrival -> Map zone event24 -> live Tag actions and retirement.
//! Source: original Foot4D8B66..4D8CA5 and Map56D100 instructions. These
//! authored-map regressions exercise the Rust production chain; they are not
//! a comparison with an executed native movement history.

use super::tests::{
    event_ingress_fixture, event_ingress_fixture_with_rules, install_event_ingress_map,
    spawn_event_ingress_unit,
};
use super::*;
use crate::sim::movement::PerCellReason;

fn install_zone_event(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    repeat: u8,
    house: i32,
) {
    install_event_ingress_map(
        sim,
        rules,
        &format!(
            "[Triggers]\nZONE=Country0,<none>,Zone,0,1,1,1,0\n\
         [Events]\nZONE=1,24,0,{house}\n\
         [Actions]\nZONE=1,28,0,7,0,0,0,0,A\n\
         [Tags]\nTAG={repeat},Zone,ZONE\n\
         [CellTags]\n15017=TAG\n"
        ),
    );
}

#[test]
fn zone_entry_arrival_runs_same_zone_action_and_retires_one_shot_tag() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    install_zone_event(&mut sim, &rules, 0, -1);
    assert!(!sim.trigger_runtime.map_tags.is_empty());
    let rng = (
        sim.scenario_rng.logical_state(),
        sim.main_rng.logical_state(),
        sim.mapgen_rng.logical_state(),
    );
    sim.per_cell_process(
        actor,
        PerCellReason::TurnComplete,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(!sim.trigger_runtime.globals_set.contains(&7));
    sim.per_cell_process(
        actor,
        PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(
        sim.trigger_runtime.globals_set.contains(&7),
        "arrival in the attached cell's movement zone must spring event24"
    );
    assert!(
        sim.trigger_runtime.map_tags.is_empty(),
        "repeat0 action retires its map registration synchronously"
    );
    assert!(
        sim.trigger_runtime
            .dispatch_trace
            .iter()
            .any(|call| call.1 == 24)
    );
    assert_eq!(
        (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state()
        ),
        rng
    );
}

#[test]
fn zone_entry_infantry_arrival_reaches_the_same_live_tag_owner() {
    let (mut sim, rules, overlays, _) = event_ingress_fixture();
    let actor = sim
        .spawn_object_at_height("ENGINEER", "House0", 15, 15, 0, 0, &rules)
        .unwrap();
    install_zone_event(&mut sim, &rules, 2, -1);
    sim.per_cell_process(
        actor,
        PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(sim.trigger_runtime.globals_set.contains(&7));
}

#[test]
fn zone_entry_country_predicate_rejects_another_house() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (15, 15));
    install_zone_event(&mut sim, &rules, 2, 0);
    sim.per_cell_process(
        actor,
        PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(
        sim.trigger_runtime
            .dispatch_trace
            .iter()
            .any(|call| call.1 == 24),
        "Map admission precedes the existing House predicate"
    );
    assert!(!sim.trigger_runtime.globals_set.contains(&7));
}

/// Supply a different raw native zone label at one cell. This isolates the
/// arrival consumer from the already-tested topology producer.
fn separate_zone(sim: &mut Simulation, cell: (u16, u16)) {
    let (x, y) = crate::sim::pathfinding::zone_build::native_zone_grid_position(
        sim.map_size_diamond().unwrap(),
        (cell.0 as i16, cell.1 as i16),
    )
    .unwrap();
    let zones = sim.zone_grid.as_mut().unwrap();
    let index = y as usize * 33 + x as usize;
    let topology = zones.base_topology_mut_for_test();
    let cluster = u16::try_from(topology.raw_zone_ids_by_row()[0].len()).unwrap();
    topology.zone_ids_mut_for_test()[index] = cluster;
    for row in topology.raw_zone_ids_by_row_mut_for_test() {
        row.push(1234);
    }
    zones.project_adopted_base_cell(index);
}

#[test]
fn zone_entry_rejects_distinct_supplied_native_zone_labels() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    install_zone_event(&mut sim, &rules, 2, -1);
    separate_zone(&mut sim, (17, 15));
    sim.per_cell_process(
        actor,
        PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(
        !sim.trigger_runtime
            .dispatch_trace
            .iter()
            .any(|call| call.1 == 24)
    );
    assert!(!sim.trigger_runtime.globals_set.contains(&7));
}

#[test]
fn zone_entry_uses_tag_as_source_for_the_asymmetric_fringe_shortcut() {
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::pathfinding::zone_map::ZoneQueryCell;
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nZONE=Country0,<none>,Zone,0,1,1,1,0\n\
         [Events]\nZONE=1,24,0,-1\n\
         [Actions]\nZONE=1,28,0,7,0,0,0,0,A\n\
         [Tags]\nTAG=2,Zone,ZONE\n[CellTags]\n16001=TAG\n",
    );
    let bounds = crate::map::playfield::PlayfieldBounds::from_normalized_local_size(16, 6, 6, 4, 4);
    sim.set_playfield_bounds_for_test(Some(bounds));
    separate_zone(&mut sim, (1, 16));
    let cells = NativeCellQuery::canonical(sim.resolved_terrain.as_ref().unwrap());
    let reaches = |from, to| {
        sim.zone_grid.as_ref().unwrap().can_reach_native(
            &cells,
            ZoneQueryCell::Copied(from),
            ZoneQueryCell::Copied(to),
            MovementZone::Normal,
            false,
            false,
            false,
            bounds,
            (16, 16),
        )
    };
    assert_eq!(reaches((1, 16), (15, 15)), Some(true));
    assert_eq!(reaches((15, 15), (1, 16)), Some(false));
    sim.per_cell_process(
        actor,
        PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(sim.trigger_runtime.globals_set.contains(&7));
}

#[test]
fn zone_entry_uses_retained_navigation_head_instead_of_location() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    install_zone_event(&mut sim, &rules, 2, -1);
    separate_zone(&mut sim, (15, 15));
    assert!(
        sim.substrate
            .entities
            .get_mut(actor)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                crate::sim::movement::DriveLocomotionRuntime::default().with_head_to_for_test(
                    Some(crate::sim::components::DriveCoord::cell(16, 15, 0))
                )
            ))
    );
    // Supplied paid-head boundary, not a claim that this manual Arrival
    // reproduces the surrounding native locomotor Process history.
    sim.per_cell_process(
        actor,
        PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(sim.trigger_runtime.globals_set.contains(&7));
}

#[test]
fn zone_entry_live_removal_skips_shifted_tag_until_the_next_arrival() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nFIRST=Country0,<none>,First,0,1,1,1,0\nSECOND=Country0,<none>,Second,0,1,1,1,0\n\
         [Events]\nFIRST=1,24,0,-1\nSECOND=1,24,0,-1\n\
         [Actions]\nFIRST=1,28,0,7,0,0,0,0,A\nSECOND=1,28,0,9,0,0,0,0,A\n\
         [Tags]\nA=0,First,FIRST\nB=2,Second,SECOND\n\
         [CellTags]\n15017=A\n16017=B\n",
    );
    sim.per_cell_process(
        actor,
        PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([7]));
    sim.per_cell_process(
        actor,
        PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([7, 9]));
}

#[test]
fn zone_entry_from_an_ordinary_move_publishes_the_authored_camera_action() {
    use crate::sim::command::{Command, CommandEnvelope};
    let (mut sim, rules, overlays, houses) = event_ingress_fixture_with_rules("[TAGUNIT]\nROT=5\n");
    // Ordinary player orders require a human/player-controlled House at
    // DoList admission, and this authored Drive unit must be able to turn.
    let owner = sim.houses.get_mut(&houses[0]).unwrap();
    owner.set_is_human_for_test(true);
    owner.set_player_control_for_test(true);
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let map = install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nZONE=Country0,<none>,Zone,0,1,1,1,0\n\
         [Events]\nZONE=1,24,0,-1\n\
         [Actions]\nZONE=1,112,0,0,0,0,0,0,A\n\
         [Tags]\nTAG=0,Zone,ZONE\n[CellTags]\n15017=TAG\n\
         [Waypoints]\n0=17017\n",
    );
    sim.session.install_waypoints(&map.waypoints);
    sim.queue_command(CommandEnvelope::new(
        houses[0],
        sim.session.tick() + 1,
        Command::Move {
            entity_id: actor,
            target_rx: 17,
            target_ry: 15,
        },
    ));
    let mut camera = Vec::new();
    for _ in 0..120 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), None, Some(&overlays), 33);
        camera.extend(sim.drain_trigger_effects());
    }
    assert_eq!(
        camera,
        vec![TriggerEffect::CameraAtCoord {
            coord: [4480, 4480, 0],
            velocity: None
        }]
    );
    assert_eq!(sim.substrate.entities.get(actor).unwrap().position.rx(), 17);
    assert!(sim.trigger_runtime.map_tags.is_empty());
}

#[test]
fn zone_entry_observes_destructor_changes_to_the_shared_map_cursor() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    sim.session.publish_started();
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nPRELOAD=Country0,<none>,Preload,0,1,1,1,0\nFIRST=Country0,<none>,First,0,1,1,1,0\nSECOND=Country0,<none>,Second,0,1,1,1,0\nTHIRD=Country0,<none>,Third,0,1,1,1,0\n\
         [Events]\nFIRST=1,24,0,-1\nSECOND=1,24,0,-1\nTHIRD=1,24,0,-1\n\
         [Actions]\nPRELOAD=1,70,3,FIRST_TAG,0,0,0,0,A\nFIRST=1,70,3,SECOND_TAG,0,0,0,0,A\nSECOND=1,28,0,8,0,0,0,0,A\nTHIRD=1,28,0,9,0,0,0,0,A\n\
         [Tags]\nFIRST_TAG=2,First,FIRST\nSECOND_TAG=2,Second,SECOND\nTHIRD_TAG=2,Third,THIRD\n\
         [CellTags]\n15017=FIRST_TAG\n16017=SECOND_TAG\n17017=THIRD_TAG\n",
    );
    let tags = sim.trigger_runtime.map_tags.clone();
    // Action references construct TagTypes before [Tags] is read. The
    // eventless PRELOAD definition reserves FIRST_TAG before FIRST's action
    // reserves SECOND_TAG, giving this case the intended live vector order.
    let names: Vec<_> = tags
        .iter()
        .map(|&tag| {
            let type_index = sim.trigger_runtime.tag(tag).unwrap().type_index;
            sim.trigger_runtime.definitions.tag_types[type_index]
                .id
                .as_str()
        })
        .collect();
    assert_eq!(names, ["FIRST_TAG", "SECOND_TAG", "THIRD_TAG"]);
    sim.per_cell_process(
        actor,
        PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    let calls: Vec<_> = sim
        .trigger_runtime
        .dispatch_trace
        .iter()
        .filter(|call| call.1 == 24)
        .map(|call| call.0)
        .collect();
    // Deleting the following Tag decrements the shared cursor regardless of
    // its slot. The first Tag is therefore visited twice, then the third.
    // The destroyed Tag itself is never the active dispatcher in this case.
    assert_eq!(calls, vec![tags[0], tags[0], tags[2]]);
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([9]));
}
