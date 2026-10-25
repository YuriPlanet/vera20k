//! The group spread's height reads against the original `0x0064CDA0` slices:
//! tools/spatial_oracle/group_spread_gates.{py,json,meta.json}.

use super::{spread_height, within_spread_band};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::world::Simulation;
use crate::sim::world::common_raw_test_terrain_cell;

/// `0x0064CDA0` returns at once unless the destination token is a Cell
/// (`0x0064CDCA..0x0064CDDC`): a Move onto an object and an attack-move with a
/// target or an object destination have no run key.
#[test]
fn only_a_cell_destination_has_a_spread_key() {
    let attack_move = |target, destination| Command::AttackMove {
        entity_id: 1,
        target,
        destination,
    };
    let move_to_cell = Command::Move {
        entity_id: 1,
        target_rx: 4,
        target_ry: 5,
    };
    assert_eq!(
        Simulation::formation_key(&move_to_cell),
        Some(((2, 0, 0, 5004), 4, 5))
    );
    assert_eq!(
        Simulation::formation_key(&attack_move(None, Some(TargetKind::Cell(4, 5)))),
        Some(((29, 0, 0, 5004), 4, 5))
    );
    for command in [
        Command::MoveToObject {
            entity_id: 1,
            target_id: 2,
        },
        attack_move(None, Some(TargetKind::Entity(2))),
        attack_move(Some(TargetKind::Entity(2)), None),
    ] {
        assert_eq!(Simulation::formation_key(&command), None, "{command:?}");
    }
}

#[test]
fn spread_heights_and_band_match_the_original_slices() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/group_spread_gates.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 524);
    for row in rows {
        let input = &row["input"];
        let cell = |x, level: &str, flags: &str| {
            let level = input[level].as_i64().unwrap() as i8 as u8;
            let mut cell = common_raw_test_terrain_cell(x, 0, level, false);
            cell.bridge_facts.raw_flags = input[flags].as_u64().unwrap() as u32;
            cell
        };
        let terrain = ResolvedTerrainGrid::from_cells(
            2,
            1,
            vec![
                cell(0, "target_level", "target_flags"),
                cell(1, "candidate_level", "candidate_flags"),
            ],
        );
        let target = spread_height(&terrain, terrain.native_cell_identity((0, 0)));
        assert_eq!(
            i64::from(target),
            row["target_height"].as_i64().unwrap(),
            "{input}"
        );
        // VERA keeps a reservation (`+0x140 & 0x8000`) in the distributor's
        // reserved set, which it tests before the band, as native does.
        if input["candidate_flags"].as_u64().unwrap() & 0x8000 != 0 {
            assert_eq!(row["outcome"], "reserved", "{input}");
            continue;
        }
        let candidate = spread_height(&terrain, terrain.native_cell_identity((1, 0)));
        assert_eq!(
            within_spread_band(target, candidate),
            row["outcome"] == "code_test",
            "{input}"
        );
    }
}

#[test]
fn planning_flags_use_the_production_group_destination_distributor() {
    use crate::sim::command::{CommandEnvelope, MegaMissionRecord};
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[VehicleTypes]\n0=PLANNER\n[PLANNER]\nStrength=100\nSpeed=8\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    );
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    sim.session.set_current_house_for_test(Some(owner));
    let actors: Vec<_> = [(15, 15), (16, 15), (15, 16)]
        .into_iter()
        .map(|(x, y)| {
            sim.spawn_object_with_overlay_registry(
                "PLANNER",
                "Americans",
                x,
                y,
                0,
                &rules,
                &registry,
            )
            .unwrap()
        })
        .collect();
    let commands: Vec<_> = actors
        .iter()
        .map(|actor| {
            CommandEnvelope::new(
                owner,
                1,
                Command::Move {
                    entity_id: *actor,
                    target_rx: 19,
                    target_ry: 17,
                },
            )
        })
        .collect();
    let mut ordinary = commands.clone();
    sim.adjust_staged_megamission_destinations(&mut ordinary, &rules, Some(&registry));
    assert!(
        ordinary
            .iter()
            .zip(&commands)
            .any(|(a, b)| a.payload != b.payload),
        "fixture must exercise actual destination redistribution"
    );
    // Getter6E6E20 stores quotient/remainder as WORDs. These two raw Cell
    // values therefore project to one destination, but64CB42..64CB5C keeps
    // them in separate runs. Neither single-member run may rewrite bytes.
    let mut distinct: Vec<_> = commands[..2]
        .iter()
        .map(|command| {
            let record = sim
                .encode_megamission_record(owner, &command.payload)
                .unwrap();
            super::QueuedCommand::from_envelope(CommandEnvelope::synchronized(record, 1))
        })
        .collect();
    let record = distinct[1].record_mut().unwrap();
    let destination = i32::from_le_bytes(record.payload()[12..16].try_into().unwrap());
    record.payload_mut()[12..16].copy_from_slice(&(destination + 65536 * 1000).to_le_bytes());
    let before = distinct.clone();
    sim.adjust_staged_megamission_destinations(&mut distinct, &rules, Some(&registry));
    assert_eq!(
        distinct, before,
        "raw destination identity, not projected geometry, owns the run"
    );
    for flag in [1, 2] {
        let mut planned: Vec<_> = commands
            .iter()
            .map(|command| {
                let mut record = sim
                    .encode_planning_order_record(owner, &command.payload, None)
                    .unwrap();
                record.payload_mut()[22] = flag;
                CommandEnvelope::new(owner, 1, Command::PlanningRecord { record })
            })
            .collect();
        sim.adjust_staged_megamission_destinations(&mut planned, &rules, Some(&registry));
        for (actual, expected) in planned.iter().zip(&ordinary) {
            let Command::PlanningRecord { record } = &actual.payload else {
                unreachable!()
            };
            assert_eq!(record.payload()[22], flag);
            let mut ordinary_record = record.clone();
            ordinary_record.payload_mut()[22] = 0;
            let decoded = MegaMissionRecord::decode(&ordinary_record).unwrap();
            let Command::Move {
                target_rx,
                target_ry,
                ..
            } = expected.payload
            else {
                unreachable!()
            };
            assert_eq!(
                decoded.order,
                crate::sim::command::MegaMissionOrder::Move {
                    target_x: target_rx as i16,
                    target_y: target_ry as i16
                }
            );
        }
    }

    // A group click must retain one shared planning node even though the
    // scheduled group distributor gives the actors different destinations.
    let records: Vec<_> = commands
        .iter()
        .map(|command| {
            CommandEnvelope::synchronized(
                sim.encode_planning_order_record(owner, &command.payload, None)
                    .unwrap(),
                0,
            )
        })
        .collect();
    sim.apply_due_commands(
        &records,
        &rules,
        0,
        Some(&registry),
        crate::sim::world::FrameEffects::empty(),
    );
    let graph = sim.planning_draw_graph(owner, &actors, None, None);
    assert_eq!(graph.nodes.len(), 1, "one click is one shared waypoint");
    assert_eq!(graph.nodes[0].members, actors);
    assert_eq!(
        graph.nodes[0].coord,
        crate::sim::components::DriveCoord::cell(19, 17, 0)
    );
    for (x, y) in [(20, 15), (18, 14)] {
        let batch: Vec<_> = actors
            .iter()
            .map(|&actor| {
                CommandEnvelope::synchronized(
                    sim.encode_planning_order_record(
                        owner,
                        &Command::Move {
                            entity_id: actor,
                            target_rx: x,
                            target_ry: y,
                        },
                        None,
                    )
                    .unwrap(),
                    0,
                )
            })
            .collect();
        sim.apply_due_commands(&batch, &rules, 0, Some(&registry), Default::default());
    }
    let graph = sim.planning_draw_graph(owner, &actors, None, None);
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(graph.nodes[1].incoming.len(), 1);
    assert_eq!(graph.nodes[2].incoming.len(), 1);
    assert!(
        graph
            .actors
            .iter()
            .all(|actor| actor.connection.as_ref().unwrap().target == graph.nodes[0].coord)
    );

    let commit = sim.encode_plan_commit_record(owner).unwrap();
    let command = sim.decode_native_command_record(&commit, 0).unwrap();
    assert!(sim.apply_command("Americans", &command.payload, Some(&rules)));
    let due = sim.take_frame_commands_for_test();
    sim.session.set_binary_frame_for_test(1);
    sim.apply_due_commands(&due, &rules, 2, Some(&registry), Default::default());
    for actor in actors {
        let entity = sim.entities().get(actor).unwrap();
        assert_eq!(
            entity.navigation.nav_com_aux(),
            Some(crate::sim::components::NavTargetRef::cell(19, 17))
        );
        assert!(entity.navigation.nav_com().is_some());
    }
}
