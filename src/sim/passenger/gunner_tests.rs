//! Production boarding, cargo departure and save/load for stock IFVs.
//!
//! Native constructor/ReceiveGunner/RemoveGunner and cargo-head comparisons:
//! tools/spatial_oracle/ifv_turret_switching.{md,json}. These regressions feed
//! retail Battle rules through the ordinary spawn, command and passenger owners.
//! Boarding tests never assign the selected pair or cargo by hand; the empty
//! removal control starts its stale selection through the shared selector.

use super::{
    DepartureFailure, DepartureRoute, depart_cargo_head, exit_cell_position,
    reveal_unloaded_passenger,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_weapon::current_weapon;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::pathfinding::PathGrid;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;

const OWNER: &str = "Americans";

fn arena(rules: &RuleSet) -> (Simulation, PathGrid) {
    let mut sim = Simulation::new();
    for (name, side) in [(OWNER, 0), ("Russians", 1)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, true, 0, 10));
    }
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    (sim, grid)
}

fn entity(sim: &Simulation, id: u64) -> &GameEntity {
    sim.substrate.entities.get(id).expect("live test object")
}

fn assert_selection(sim: &Simulation, ifv: u64, expected: (i32, i32)) {
    let ifv = entity(sim, ifv);
    assert_eq!(
        (ifv.current_weapon_number(), ifv.current_turret_index()),
        expected
    );
}

fn assert_current_weapon(sim: &Simulation, rules: &RuleSet, ifv: u64, expected: &str) {
    assert_eq!(
        current_weapon(
            entity(sim, ifv),
            rules.object("FV").expect("retail FV"),
            &sim.substrate.entities,
            rules,
            &sim.interner,
        )
        .map(|weapon| weapon.id.as_str()),
        Some(expected)
    );
}

fn spawn_and_board(sim: &mut Simulation, rules: &RuleSet, passenger_type: &str) -> (u64, u64) {
    let ifv = sim
        .spawn_object("FV", OWNER, 16, 16, 64, rules)
        .expect("spawn retail FV");
    assert_selection(sim, ifv, (0, 0));
    assert_current_weapon(sim, rules, ifv, "HoverMissile");
    let passenger = sim
        .spawn_object(passenger_type, OWNER, 16, 15, 64, rules)
        .unwrap_or_else(|| panic!("spawn retail {passenger_type}"));
    assert!(
        sim.object_type(entity(sim, ifv).type_ref(), rules)
            .is_some()
    );
    assert!(
        sim.object_type(entity(sim, passenger).type_ref(), rules)
            .is_some()
    );
    assert!(sim.order_actor_admits(passenger));
    assert!(sim.order_object_token_admits(ifv));
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            passenger,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules),
    ));
    // The Infantry setter's HELLO (`0x0051AC4B`) leaves the IFV its contact.
    assert_eq!(entity(sim, passenger).radio_contacts.slot(0), Some(ifv));
    // Walk to the occupied transport cell through the production entry path.
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..240 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if entity(sim, passenger).passenger_role.is_inside_transport() {
            break;
        }
    }
    assert_boarded(sim, ifv, passenger);
    (ifv, passenger)
}

fn assert_boarded(sim: &Simulation, ifv: u64, passenger: u64) {
    let cargo = entity(sim, ifv).passenger_role.cargo().expect("IFV cargo");
    assert_eq!(cargo.passengers, [passenger]);
    assert_eq!(cargo.passenger_sizes, [1.0]);
    assert_eq!(cargo.total_size, 1);
    let pax = entity(sim, passenger);
    assert_eq!(pax.passenger_role.inside_transport(), Some((ifv, false)));
    assert!(pax.lifecycle.in_limbo());
    assert!(!sim.live_object_order_snapshot().contains(&passenger));
}

fn unload_at_clear_exit(sim: &mut Simulation, rules: &RuleSet, ifv: u64, passenger: u64) {
    let exit = (
        entity(sim, ifv).position.rx(),
        entity(sim, ifv).position.ry() + 1,
    );
    assert_eq!(
        depart_cargo_head(
            sim,
            rules,
            None,
            ifv,
            DepartureRoute::Vehicle,
            |sim, departing| {
                assert_eq!(departing, passenger);
                // FootClass::RemoveFirstPassenger 4DE710 resets the gunner before
                // UnitClass::Mission_Unload attempts the passenger's Unlimbo.
                assert_selection(sim, ifv, (0, 0));
                let position = exit_cell_position(sim, departing, exit.0, exit.1, 0);
                reveal_unloaded_passenger(
                    sim,
                    rules,
                    ifv,
                    departing,
                    position,
                    crate::sim::world::PlacementEvidence::MarkSucceeded,
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
            },
            crate::sim::world::FrameEffects::default()
        ),
        Ok(())
    );
    assert_selection(sim, ifv, (0, 0));
    assert_current_weapon(sim, rules, ifv, "HoverMissile");
    let cargo = entity(sim, ifv).passenger_role.cargo().unwrap();
    assert!(cargo.is_empty());
    assert!(cargo.passenger_sizes.is_empty());
    assert_eq!(cargo.total_size, 0);
    let pax = entity(sim, passenger);
    assert!(pax.passenger_role.is_none());
    assert!(!pax.lifecycle.in_limbo());
    assert_eq!((pax.position.rx(), pax.position.ry()), exit);
    assert_eq!(
        sim.live_object_order_snapshot()
            .iter()
            .filter(|&&id| id == passenger)
            .count(),
        1
    );
}

/// Infantry51F190 -> Foot4D76F8 ->6FFBE0 sends Enter7 with the clicked
/// transport as Destination; Event4C747C preserves that object token. Its
/// coordinate belongs to Foot4DBDF0 (foot_navigation_coordinate corpus).
/// This is a production command regression, not a whole boarding comparison.
#[test]
fn retail_infantry_enter_keeps_the_transport_destination_reference() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let ifv = sim.spawn_object("FV", OWNER, 16, 16, 64, rules).unwrap();
    let passenger = sim.spawn_object("E1", OWNER, 16, 15, 64, rules).unwrap();
    let transport_coord = sim.foot_navigation_coordinate(ifv).unwrap();

    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            passenger,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules),
    ));
    let passenger = entity(&sim, passenger);
    assert_eq!(
        passenger.navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::object(ifv)),
        "the native Enter destination is the clicked object, retaining its identity"
    );
    assert_eq!(
        passenger.locomotor.as_ref().unwrap().walk_destination(),
        Some(transport_coord),
        "the class setter samples the transport's shared +4C coordinate"
    );
}

/// Infantry PerCellProcess 51A3C0..51A3D4 compares the passenger and
/// transport map coordinates for equality before its CanEnter radio message.
#[test]
fn retail_infantry_transport_boarding_waits_for_transport_cell() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid) = arena(rules);
    let ifv = sim.spawn_object("FV", OWNER, 16, 16, 64, rules).unwrap();
    let passenger = sim.spawn_object("E1", OWNER, 16, 15, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            passenger,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules)
    ));
    sim.per_cell_process(
        passenger,
        crate::sim::movement::PerCellReason::Arrival,
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    let pax = entity(&sim, passenger);
    assert!(
        !pax.passenger_role.is_inside_transport() && !pax.lifecycle.in_limbo(),
        "an adjacent passenger must remain visible until it reaches the vehicle cell"
    );
    assert_eq!(pax.radio_contacts.slot(0), Some(ifv));
    assert!(entity(&sim, ifv).passenger_role.cargo().unwrap().is_empty());
    for _ in 0..180 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if entity(&sim, passenger).passenger_role.is_inside_transport() {
            assert_boarded(&sim, ifv, passenger);
            return;
        }
    }
    panic!("ordinary Enter must carry a walking infantryman into the occupied vehicle cell");
}

#[test]
fn retail_chrono_remote_transport_enter_keeps_object_and_boards() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid) = arena(rules);
    let ifv = sim.spawn_object("FV", OWNER, 24, 16, 64, rules).unwrap();
    let passenger = sim.spawn_object("CLEG", OWNER, 8, 16, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            passenger,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules)
    ));
    assert_eq!(
        entity(&sim, passenger).navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::object(ifv)),
        "a Chrono Enter must retain the transport object through the destination setter"
    );
    for _ in 0..180 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if entity(&sim, passenger).passenger_role.is_inside_transport() {
            assert_boarded(&sim, ifv, passenger);
            assert_current_weapon(&sim, rules, ifv, "CRNeutronRifle");
            // The self-warp epoch continues while cargo is concealed. Its
            // first restored Process observes elapsed global frames rather
            // than restarting a materialization countdown after unloading.
            let delay = entity(&sim, passenger)
                .teleport_state()
                .unwrap()
                .chrono_delay_ticks();
            for _ in 0..=delay {
                sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
                assert_boarded(&sim, ifv, passenger);
            }
            unload_at_clear_exit(&mut sim, rules, ifv, passenger);
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
            assert!(
                !entity(&sim, passenger).is_warping_in(),
                "hidden global time must retire warp-in at the first restored turn"
            );
            assert!(sim.apply_command(
                OWNER,
                &Command::Move {
                    entity_id: passenger,
                    target_rx: 12,
                    target_ry: 16,
                },
                Some(rules)
            ));
            assert_eq!(
                entity(&sim, passenger).navigation.nav_com(),
                Some(crate::sim::components::NavTargetRef::cell(12, 16)),
                "after unloading, the Legionnaire accepts a new teleport destination"
            );
            return;
        }
    }
    panic!("one remote Enter order must teleport the Legionnaire and finish boarding");
}

#[test]
fn retail_vehicle_enter_keeps_live_transport_target() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    for passenger_type in ["MTNK", "CMIN", "FV"] {
        let (mut sim, grid) = arena(rules);
        let transport = sim.spawn_object("LCRF", OWNER, 16, 16, 64, rules).unwrap();
        let passenger = sim
            .spawn_object(passenger_type, OWNER, 16, 14, 64, rules)
            .unwrap();
        assert!(crate::sim::passenger::can_enter_transport(
            &sim, rules, passenger, transport
        ));
        assert!(sim.apply_command(
            OWNER,
            &Command::clicked_mission(
                passenger,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(transport)
            ),
            Some(rules),
        ));
        // Unit741CB8 parks a contactless mobile destination in +500;
        // Foot70D7E0 performs HELLO/CanEnter before restoring live NavCom.
        assert_eq!(entity(&sim, passenger).pending_entry(), Some(transport));
        assert_eq!(entity(&sim, passenger).navigation.nav_com(), None);
        crate::sim::docking::building_dock::try_pending_entry(
            &mut sim,
            rules,
            passenger,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(entity(&sim, passenger).pending_entry(), None);
        assert_eq!(
            entity(&sim, passenger).navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::object(transport)),
            "{passenger_type}: admitted Unit entry keeps its live transport"
        );
        for _ in 0..600 {
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
            if entity(&sim, passenger).passenger_role.is_inside_transport() {
                break;
            }
        }
        let cargo = entity(&sim, transport).passenger_role.cargo().unwrap();
        let size = rules.object(passenger_type).unwrap().size;
        assert_eq!(cargo.passengers, [passenger], "{passenger_type}: cargo");
        assert_eq!(cargo.passenger_sizes, [size]);
        assert!(
            size.fract() == 0.0,
            "stock control size is integral: {size}"
        );
        assert_eq!(cargo.total_size, size as i32);
        assert!(entity(&sim, passenger).lifecycle.in_limbo());
        assert!(!sim.live_object_order_snapshot().contains(&passenger));
    }
}

/// Unit ReceiveDamage calls each escaped passenger's Scatter(&EmptyCoord, 1, 0)
/// at 0x00738130..0x0073813D. For a tank, Unit Scatter installs a cell NavCom
/// through its ordinary destination setter and Drive Move_To.
#[test]
fn retail_tanks_scatter_after_their_transport_is_destroyed() {
    use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::movement::track_process::TrackFamily;
    use crate::sim::world::FrameEffects;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid) = arena(rules);
    let transport = sim.spawn_object("LCRF", OWNER, 16, 16, 64, rules).unwrap();
    let mut passengers = Vec::new();
    for _ in 0..2 {
        let passenger = sim.spawn_object("MTNK", OWNER, 16, 14, 64, rules).unwrap();
        assert!(crate::sim::passenger::can_enter_transport(
            &sim, rules, passenger, transport
        ));
        assert!(sim.apply_command(
            OWNER,
            &Command::clicked_mission(
                passenger,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(transport),
            ),
            Some(rules),
        ));
        for _ in 0..600 {
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
            if entity(&sim, passenger).passenger_role.is_inside_transport() {
                break;
            }
        }
        assert_eq!(
            entity(&sim, passenger).passenger_role.inside_transport_id(),
            Some(transport),
            "the stock tank must board through its Enter command"
        );
        assert!(entity(&sim, passenger).lifecycle.in_limbo());
        passengers.push(passenger);
    }
    let cargo = entity(&sim, transport).passenger_role.cargo().unwrap();
    assert_eq!(cargo.passengers, [passengers[1], passengers[0]]);
    assert_eq!(cargo.passenger_sizes, [3.0, 3.0]);
    assert_eq!(cargo.total_size, 6);
    let source = entity(&sim, transport).position.clone();
    let warhead = sim.interner.intern("AP");
    let hit = EntityDamageEvent::direct_receiver(
        transport,
        100_000,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(rules, None, &[hit], FrameEffects::default());
    assert!(sim.substrate.pending_delete.contains_object(&transport));
    assert!(
        entity(&sim, transport)
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
    );

    // The original two_guard row admits the second tank with code 2 after
    // the first tank receives its NavCom. Neither Scatter runs a Process.
    for &id in &passengers {
        let escaped = entity(&sim, id);
        assert!(escaped.lifecycle.object_alive() && !escaped.lifecycle.in_limbo());
        assert!(escaped.passenger_role.is_none());
        assert_eq!(
            (escaped.position.rx(), escaped.position.ry()),
            (source.rx(), source.ry())
        );
        let Some(NavTargetRef::Cell { rx, ry }) = escaped.navigation.nav_com() else {
            panic!("an escaped stock tank must receive Scatter's cell destination");
        };
        assert_ne!((rx, ry), (source.rx(), source.ry()));
        assert_eq!(
            escaped
                .locomotor
                .as_ref()
                .unwrap()
                .track_destination(TrackFamily::Drive),
            Some(DriveCoord {
                x: i32::from(rx) * 256 + 128,
                y: i32::from(ry) * 256 + 128,
                z: 0,
            }),
            "the escape destination must reach the tank's Drive locomotor"
        );
    }
    // The carrier's deferred deletion must not retire its escaped cargo.
    // Subsequent pathfinding and congestion are outside the escape oracle.
    for _ in 0..600 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
    }
    assert!(sim.substrate.entities.get(transport).is_none());
    for id in passengers {
        let escaped = entity(&sim, id);
        assert!(escaped.lifecycle.object_alive() && !escaped.lifecycle.in_limbo());
        assert!(escaped.passenger_role.is_none());
    }
}

#[test]
fn retail_loaded_ifv_survives_transport_snapshot_unload_and_reentry() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid) = arena(rules);
    let (ifv, engineer) = spawn_and_board(&mut sim, rules, "ENGINEER");
    let initial_cargo =
        serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap();
    let boat = sim.spawn_object("LCRF", OWNER, 16, 19, 64, rules).unwrap();
    assert!(crate::sim::passenger::can_enter_transport(
        &sim, rules, ifv, boat
    ));
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            ifv,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(boat)
        ),
        Some(rules)
    ));
    for _ in 0..600 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        if entity(&sim, ifv).passenger_role.is_inside_transport() {
            break;
        }
    }
    assert_eq!(
        entity(&sim, ifv).passenger_role.inside_transport_id(),
        Some(boat)
    );
    assert_boarded(&sim, ifv, engineer);
    assert_selection(&sim, ifv, (1, 2));
    let outer_cargo = entity(&sim, boat).passenger_role.cargo().unwrap();
    assert_eq!(outer_cargo.passengers, [ifv]);
    assert_eq!(outer_cargo.total_size, 3);

    let terrain_template = sim.resolved_terrain.as_ref().unwrap().clone();
    let bytes = GameSnapshot::save(&sim, 0, 0, "nested repair IFV", 0);
    let mut dangling = GameSnapshot::load(&bytes).unwrap().sim;
    dangling
        .substrate
        .entities
        .get_mut(ifv)
        .unwrap()
        .passenger_role
        .set_inside_transport(u64::MAX, false);
    let error = dangling.restore_after_snapshot_load().unwrap_err();
    assert!(format!("{error:?}").contains("passenger_role.inside"));
    let mut sim = GameSnapshot::load(&bytes)
        .expect("nested cargo snapshot")
        .sim;
    sim.restore_after_snapshot_load()
        .expect("both parent and own cargo resolve");
    sim.resolve_type_handles(rules);
    sim.install_resolved_terrain_for_new_map(terrain_template);
    assert!(sim.rebuild_dynamic_navigation(rules));
    assert_eq!(
        entity(&sim, ifv).passenger_role.inside_transport_id(),
        Some(boat)
    );
    assert_boarded(&sim, ifv, engineer);
    assert_current_weapon(&sim, rules, ifv, "RepairBullet");

    assert_eq!(
        depart_cargo_head(
            &mut sim,
            rules,
            None,
            boat,
            DepartureRoute::Vehicle,
            |sim, departing| {
                assert_eq!(departing, ifv);
                let position = exit_cell_position(sim, departing, 16, 20, 0);
                reveal_unloaded_passenger(
                    sim,
                    rules,
                    boat,
                    departing,
                    position,
                    crate::sim::world::PlacementEvidence::MarkSucceeded,
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
            },
            crate::sim::world::FrameEffects::default()
        ),
        Ok(())
    );
    assert_eq!(entity(&sim, ifv).passenger_role.inside_transport_id(), None);
    assert!(!entity(&sim, ifv).lifecycle.in_limbo());
    assert_eq!(
        serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap(),
        initial_cargo
    );
    assert_boarded(&sim, ifv, engineer);
    assert_current_weapon(&sim, rules, ifv, "RepairBullet");
    unload_at_clear_exit(&mut sim, rules, ifv, engineer);
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            engineer,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules)
    ));
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..240 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        if entity(&sim, engineer).passenger_role.is_inside_transport() {
            break;
        }
    }
    assert_boarded(&sim, ifv, engineer);
    assert_current_weapon(&sim, rules, ifv, "RepairBullet");
}

#[test]
fn retail_replacement_move_cancels_pending_vehicle_boarding() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid) = arena(rules);
    let ifv = sim.spawn_object("FV", OWNER, 16, 16, 64, rules).unwrap();
    let passenger = sim.spawn_object("E1", OWNER, 16, 15, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            passenger,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules)
    ));
    assert!(sim.apply_command(
        OWNER,
        &Command::Move {
            entity_id: passenger,
            target_rx: 16,
            target_ry: 17,
        },
        Some(rules)
    ));
    for _ in 0..180 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
    }
    assert!(
        entity(&sim, ifv).passenger_role.cargo().unwrap().is_empty(),
        "crossing the old destination cannot board after Move replaced NavCom"
    );
    assert!(entity(&sim, passenger).passenger_role.is_none());
    assert!(!entity(&sim, passenger).lifecycle.in_limbo());
}

#[test]
fn retail_ifv_boarding_and_departure_retry_keep_one_selection_owner() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    // Original physical RULESMD readers plus ReceiveGunner 746420 establish
    // these values. The final production Battle/Hills rules remain the inputs.
    for (passenger_type, expected, weapon) in [
        ("E1", (2, 1), "CRM60"),
        ("ENGINEER", (1, 2), "RepairBullet"),
        ("SHK", (6, 3), "CRElectricBolt"),
        ("GGI", (16, 3), "CRMissileLauncher"),
    ] {
        let (mut sim, _) = arena(rules);
        let (ifv, passenger) = spawn_and_board(&mut sim, rules, passenger_type);
        assert_selection(&sim, ifv, expected);
        assert_current_weapon(&sim, rules, ifv, weapon);
        let before_cargo =
            serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap();
        assert_eq!(
            depart_cargo_head(
                &mut sim,
                rules,
                None,
                ifv,
                DepartureRoute::Vehicle,
                |sim, departing| {
                    assert_eq!(departing, passenger);
                    assert_selection(sim, ifv, (0, 0));
                    Err(DepartureFailure::Placement)
                },
                crate::sim::world::FrameEffects::default(),
            ),
            Err(DepartureFailure::Placement)
        );
        assert_selection(&sim, ifv, expected);
        assert_current_weapon(&sim, rules, ifv, weapon);
        assert_boarded(&sim, ifv, passenger);
        assert_eq!(
            serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap(),
            before_cargo,
            "{passenger_type}: a refused exit restores exact cargo bookkeeping"
        );
        unload_at_clear_exit(&mut sim, rules, ifv, passenger);
    }
}

#[test]
fn retail_empty_ifv_departure_resets_selection_without_a_passenger() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let ifv = sim
        .spawn_object("FV", OWNER, 16, 16, 64, rules)
        .expect("spawn retail FV");
    assert_selection(&sim, ifv, (0, 0));
    let initial_cargo =
        serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap();
    for initial in [(0, 0), (2, 1)] {
        // The native Foot4DE710 -> Unit7464E0 null-passenger control resets
        // even an already-empty hold. Select the stale mode with the same
        // owner used by ReceiveGunner, without fabricating a cargo entry.
        sim.substrate
            .entities
            .get_mut(ifv)
            .unwrap()
            .set_gunner_weapon(initial.0, rules.object("FV").unwrap());
        assert_selection(&sim, ifv, initial);
        assert_eq!(
            depart_cargo_head(
                &mut sim,
                rules,
                None,
                ifv,
                DepartureRoute::Vehicle,
                |_, _| { panic!("an empty hold must never attempt passenger placement") },
                crate::sim::world::FrameEffects::default()
            ),
            Err(DepartureFailure::NoCargo)
        );
        assert_selection(&sim, ifv, (0, 0));
        assert_current_weapon(&sim, rules, ifv, "HoverMissile");
        assert_eq!(
            serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap(),
            initial_cargo
        );
    }
}

#[test]
fn retail_boarded_ifv_snapshot_restores_selection_and_cargo_then_unloads() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let (ifv, passenger) = spawn_and_board(&mut sim, rules, "ENGINEER");
    assert_selection(&sim, ifv, (1, 2));
    let cargo = serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap();
    let terrain_template = sim.resolved_terrain.as_ref().unwrap().clone();
    let bytes = GameSnapshot::save(&sim, 0, 0, "ifv gunner", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("IFV snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("passenger and transport identities resolve");
    restored.resolve_type_handles(rules);
    // The app supplies map geometry again after load. This clear arena has no
    // dynamic overlay or bridge changes to replay onto that template.
    restored.install_resolved_terrain_for_new_map(terrain_template);
    assert!(restored.rebuild_dynamic_navigation(rules));
    assert_selection(&restored, ifv, (1, 2));
    assert_current_weapon(&restored, rules, ifv, "RepairBullet");
    assert_boarded(&restored, ifv, passenger);
    assert_eq!(
        serde_json::to_value(entity(&restored, ifv).passenger_role.cargo().unwrap()).unwrap(),
        cargo
    );
    unload_at_clear_exit(&mut restored, rules, ifv, passenger);
}

#[test]
fn retail_gi_ifv_fires_its_selected_weapon_without_changing_the_turret() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid) = arena(rules);
    let (ifv, passenger) = spawn_and_board(&mut sim, rules, "E1");
    assert_selection(&sim, ifv, (2, 1));
    assert_current_weapon(&sim, rules, ifv, "CRM60");
    let target = sim
        .spawn_object("HTNK", "Russians", 20, 16, 192, rules)
        .expect("spawn target");
    assert!(sim.apply_command(
        OWNER,
        &Command::Attack {
            attacker_id: ifv,
            target_id: target,
        },
        Some(rules),
    ));
    let mut fired = false;
    for _ in 0..120 {
        sim.fire_events.clear_for_test();
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        assert_selection(&sim, ifv, (2, 1));
        if let Some(event) = sim
            .fire_events
            .iter()
            .find(|event| event.attacker_id == ifv)
        {
            assert_eq!(sim.interner.resolve(event.weapon_id), "CRM60");
            assert!(entity(&sim, ifv).rearm_timer.duration() > 0);
            fired = true;
            break;
        }
    }
    assert!(fired, "the boarded GI IFV never fired its retail weapon");
    assert_boarded(&sim, ifv, passenger);
    assert_current_weapon(&sim, rules, ifv, "CRM60");
}

#[test]
fn retail_sniper_ifv_guard_scan_uses_its_boarded_weapon_range() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert_eq!(rules.object("SNIPE").unwrap().ifv_mode, 5);
    assert_eq!(rules.weapon("AWPE").unwrap().range_leptons, 14 * 256);
    assert_eq!(rules.weapon("HoverMissile").unwrap().range_leptons, 6 * 256);

    for (distance, expected_pick) in [(12, true), (15, false)] {
        let (mut sim, _) = arena(rules);
        let (ifv, passenger) = spawn_and_board(&mut sim, rules, "SNIPE");
        assert_selection(&sim, ifv, (5, 1));
        assert_current_weapon(&sim, rules, ifv, "AWPE");
        let (x, y) = (
            entity(&sim, ifv).position.rx(),
            entity(&sim, ifv).position.ry(),
        );
        // The target appears after boarding. Shared sight from another unit
        // can reveal it beyond the IFV's own Sight=8.
        let target = sim
            .spawn_object("E2", "Russians", x + distance, y, 192, rules)
            .expect("spawn visible infantry target");
        let owner = entity(&sim, ifv).owner();
        sim.fog.reveal_all_for_owner(owner);
        let pick = sim.greatest_threat_represented(rules, None, ifv, ScanMission::Guard, None);
        assert_eq!(
            pick,
            expected_pick.then_some(target),
            "sniper IFV Guard scan at {distance} cells"
        );
        assert_boarded(&sim, ifv, passenger);
        assert_selection(&sim, ifv, (5, 1));

        // RemoveGunner restores HoverMissile. The same Guard scan must stop
        // accepting the distant infantry once its longer-range gunner leaves.
        unload_at_clear_exit(&mut sim, rules, ifv, passenger);
        assert_eq!(
            sim.greatest_threat_represented(rules, None, ifv, ScanMission::Guard, None),
            None,
            "empty IFV Guard scan at {distance} cells"
        );
    }
}

/// Unit746502..74658F transfers a running rearm to the departing gunner
/// before Unlimbo. A refused exit re-enters ReceiveGunner746420, which
/// advances/rearms the IFV again instead of restoring its former countdown.
#[test]
fn retail_temporal_gunner_unload_rearms_before_exit_and_retry() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _, ifv, passenger, target) = temporal_attack_arena(rules);
    let frame = sim.session.binary_frame() as i32;
    let cargo_before =
        serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap();
    assert_eq!(
        depart_cargo_head(
            &mut sim,
            rules,
            None,
            ifv,
            DepartureRoute::Vehicle,
            |sim, departing| {
                assert_eq!(departing, passenger);
                assert_selection(sim, ifv, (0, 0));
                assert!(entity(sim, passenger).temporal.has_link());
                assert!(!entity(sim, ifv).temporal.has_link());
                assert!(!entity(sim, target).temporal.is_warped());
                assert_eq!(
                    entity(sim, passenger).weapon_burst.index(),
                    1,
                    "RemoveGunner increments the receiving gunner before exit"
                );
                assert_eq!(entity(sim, passenger).rearm_timer.start_frame(), frame);
                assert!(entity(sim, passenger).rearm_timer.remaining(frame) > 0);
                Err(DepartureFailure::Placement)
            },
            crate::sim::world::FrameEffects::default(),
        ),
        Err(DepartureFailure::Placement)
    );
    assert_boarded(&sim, ifv, passenger);
    assert_selection(&sim, ifv, (10, 3));
    assert_eq!(
        serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap(),
        cargo_before
    );
    assert!(entity(&sim, ifv).temporal.has_link());
    assert!(!entity(&sim, passenger).temporal.has_link());
    assert!(!entity(&sim, ifv).temporal.is_warping_someone());
    assert_eq!(entity(&sim, ifv).weapon_burst.index(), 1);
    assert_eq!(entity(&sim, ifv).rearm_timer.start_frame(), frame);
    assert!(entity(&sim, ifv).rearm_timer.remaining(frame) > 0);
}

fn temporal_attack_arena(rules: &RuleSet) -> (Simulation, PathGrid, u64, u64, u64) {
    let (mut sim, grid) = arena(rules);
    let (ifv, passenger) = spawn_and_board(&mut sim, rules, "CLEG");
    assert_selection(&sim, ifv, (10, 3));
    assert!(entity(&sim, ifv).temporal.has_link());
    assert!(!entity(&sim, passenger).temporal.has_link());
    let target = sim
        .spawn_object("HTNK", "Russians", 20, 16, 192, rules)
        .expect("retail Temporal target");
    assert!(sim.apply_command(
        OWNER,
        &Command::Attack {
            attacker_id: ifv,
            target_id: target,
        },
        Some(rules),
    ));
    let mut fired = false;
    for _ in 0..120 {
        sim.fire_events.clear_for_test();
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        fired |= sim.fire_events.iter().any(|event| event.attacker_id == ifv);
        if fired && entity(&sim, target).temporal.is_warped() {
            break;
        }
    }
    assert!(
        fired,
        "the boarded CLEG IFV must fire through ordinary Attack"
    );
    let frame = sim.session.binary_frame() as i32;
    assert!(entity(&sim, ifv).rearm_timer.remaining(frame) > 0);
    assert!(entity(&sim, target).temporal.is_warped());
    (sim, grid, ifv, passenger, target)
}

#[path = "gunner_temporal_tests.rs"]
mod temporal;

#[path = "gunner_temporal_oracle_tests.rs"]
mod temporal_oracle;

/// Infantry PerCellProcess51A46E reaches vt+0x394 AddPassenger710670, which
/// plays the stock FV's EnterTransportSound=IFVTransform at the IFV's Location
/// through Voc750E20 once the passenger is in the cargo.
#[test]
fn retail_boarding_plays_the_transports_enter_transport_sound() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid) = arena(rules);
    let ifv = sim.spawn_object("FV", OWNER, 16, 16, 64, rules).unwrap();
    let passenger = sim.spawn_object("E1", OWNER, 16, 15, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            passenger,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules)
    ));
    let location =
        crate::sim::movement::ground_pose::position_world_coord(&entity(&sim, ifv).position);
    sim.sound_events.clear_for_test();
    for _ in 0..180 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if entity(&sim, passenger).passenger_role.is_inside_transport() {
            break;
        }
    }
    let played: Vec<_> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            crate::sim::world::SimSoundEvent::VocAtCoord {
                sound_id,
                coordinate,
                flags,
            } if sound_id == "IFVTransform" => Some((*coordinate, *flags)),
            _ => None,
        })
        .collect();
    assert_boarded(&sim, ifv, passenger);
    assert_eq!(played, [(location, 0)]);
}

/// The Infantry setter's Enter arm (`0x0051AC51..0x0051ACC5`): a transport
/// already in radio contact becomes the second passenger's pending entry
/// (`+0x500`), with no destination. Foot's AI asks it again
/// (`TryEnterTransport @ 0x0070D7E0`) once the first is in and his Limbo
/// broke that contact, and both board.
#[test]
fn retail_a_second_passenger_waits_for_a_busy_transport() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid) = arena(rules);
    let htk = sim.spawn_object("HTK", OWNER, 16, 16, 64, rules).unwrap();
    let first = sim.spawn_object("E1", OWNER, 16, 14, 64, rules).unwrap();
    let second = sim.spawn_object("E1", OWNER, 13, 16, 64, rules).unwrap();
    for passenger in [first, second] {
        assert!(sim.apply_command(
            OWNER,
            &Command::clicked_mission(
                passenger,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(htk)
            ),
            Some(rules),
        ));
    }
    assert_eq!(entity(&sim, first).radio_contacts.slot(0), Some(htk));
    let waiting = entity(&sim, second);
    assert_eq!(waiting.pending_entry(), Some(htk));
    assert_eq!(waiting.navigation.nav_com(), None);
    assert!(waiting.radio_contacts.is_empty());
    for _ in 0..900 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if entity(&sim, second).passenger_role.is_inside_transport() {
            break;
        }
    }
    let cargo = entity(&sim, htk).passenger_role.cargo().unwrap();
    assert_eq!(
        cargo.passengers(),
        [second, first],
        "the first boards first"
    );
}

/// A transport answers its passenger's DOCKING (Unit `0x007377D8..
/// 0x00737A95`) on Water (`0x0073796B`) by taking him as its destination,
/// and its DOCKING answer ends his setter before Foot's (`0x0051AD0F`): he
/// waits ashore while it comes for him.
#[test]
fn retail_an_amphibious_transport_comes_ashore_for_its_passenger() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    crate::sim::arena_fixture::flood(
        &mut sim,
        rules,
        32,
        (0..32).flat_map(|y| (19..32).map(move |x| (x, y))),
    );
    let grid = sim.path_grid_snapshot().unwrap();
    let boat = sim.spawn_object("LCRF", OWNER, 23, 16, 64, rules).unwrap();
    let gi = sim.spawn_object("E1", OWNER, 15, 16, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            gi,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(boat)
        ),
        Some(rules),
    ));
    assert_eq!(
        entity(&sim, boat).navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::object(gi)),
        "the transport comes for him"
    );
    assert_eq!(entity(&sim, gi).navigation.nav_com(), None, "he waits");
    assert_eq!(entity(&sim, gi).radio_contacts.slot(0), Some(boat));
    for _ in 0..900 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if entity(&sim, gi).passenger_role.is_inside_transport() {
            break;
        }
    }
    assert_eq!(
        entity(&sim, gi).passenger_role.inside_transport_id(),
        Some(boat)
    );
}

/// Infantry run Mission_Enter (`0x004D9290`, InfantryClass vt+0x240) on
/// their way in: DOCKING to the transport, then the Rate epilogue,
/// `ftol([Enter] Rate × 900)` plus one RandomRanged(0, 2) on the Scenario
/// stream (`0x004D946C..0x004D9497`).
#[test]
fn retail_a_walking_passenger_runs_mission_enter() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let ifv = sim.spawn_object("FV", OWNER, 16, 16, 64, rules).unwrap();
    let gi = sim.spawn_object("E1", OWNER, 10, 16, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            gi,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules),
    ));
    assert_eq!(
        entity(&sim, gi).mission.current(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Enter)
    );
    let mut replay = sim.scenario_rng.clone();
    let delay = crate::sim::mission::authority::rate_epilogue(
        rules,
        crate::sim::mission::MissionType::Enter,
        &mut replay,
    );
    crate::sim::world::dispatch_foot_mission(
        &mut sim,
        gi,
        rules,
        crate::sim::world::ObjectAiCtx::default(),
    );
    assert_eq!(sim.scenario_rng.state(), replay.state());
    assert_eq!(entity(&sim, gi).mission.dispatch_timer().delay(), delay);
    assert_eq!(entity(&sim, gi).radio_contacts.slot(0), Some(ifv));
}

/// Mission_Enter's DOCKING (`0x004D92B2..0x004D92E8`) to a transport on
/// Move is answered NEGATORY (Unit `0x007377D8`): the passenger walking to
/// it breaks the contact and goes idle.
#[test]
fn retail_a_passenger_gives_up_when_its_transport_drives_off() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid) = arena(rules);
    let ifv = sim.spawn_object("FV", OWNER, 16, 16, 64, rules).unwrap();
    let gi = sim.spawn_object("E1", OWNER, 10, 16, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            gi,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules),
    ));
    assert_eq!(entity(&sim, gi).radio_contacts.slot(0), Some(ifv));
    assert!(sim.apply_command(
        OWNER,
        &Command::Move {
            entity_id: ifv,
            target_rx: 28,
            target_ry: 16,
        },
        Some(rules),
    ));
    for _ in 0..240 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
    }
    let pax = entity(&sim, gi);
    assert!(!pax.passenger_role.is_inside_transport());
    assert!(pax.radio_contacts.is_empty());
    assert_ne!(
        pax.mission.effective(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Enter)
    );
    assert!(entity(&sim, ifv).passenger_role.cargo().unwrap().is_empty());
}

/// The Unit setter (`0x00741AD1..0x00741B22`): a transport whose first
/// contact is an infantryman walking aboard breaks it when sent to a Techno
/// it holds no contact with.
#[test]
fn retail_a_transport_sent_elsewhere_breaks_its_passengers_contact() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let htk = sim.spawn_object("HTK", OWNER, 16, 16, 64, rules).unwrap();
    let gi = sim.spawn_object("E1", OWNER, 16, 13, 64, rules).unwrap();
    let other = sim.spawn_object("MTNK", OWNER, 22, 16, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            gi,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(htk)
        ),
        Some(rules),
    ));
    assert_eq!(entity(&sim, htk).radio_contacts.slot(0), Some(gi));
    let _ = sim.assign_destination_represented(
        htk,
        Some(crate::sim::components::NavTargetRef::object(other)),
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(entity(&sim, htk).radio_contacts.is_empty());
    assert!(entity(&sim, gi).radio_contacts.is_empty());
    assert_eq!(
        entity(&sim, htk).navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::object(other))
    );
}

/// The Unit setter's break (`0x00741AD1..0x00741B22`) names its trigger: an
/// infantryman as first contact, a target the transport holds no contact
/// with, and a type with `Passengers=`. A transport sent to that infantryman,
/// one whose first contact is a vehicle, and a tank without `Passengers=`
/// each keep the contact.
#[test]
fn retail_the_setters_break_spares_every_other_contact() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let hello = |sim: &mut Simulation, from: u64, to: u64| {
        assert_eq!(
            crate::sim::radio::transmit(
                sim,
                from,
                to,
                crate::sim::radio::RadioMessage::Hello,
                crate::sim::radio::RadioPayload::default(),
                rules,
                crate::sim::world::FrameEffects::default(),
            ),
            crate::sim::radio::RadioResponse::Roger
        );
    };
    let send = |sim: &mut Simulation, unit: u64, target: u64| {
        let _ = sim.assign_destination_represented(
            unit,
            Some(crate::sim::components::NavTargetRef::object(target)),
            rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
    };
    let htk = sim.spawn_object("HTK", OWNER, 16, 16, 64, rules).unwrap();
    let gi = sim.spawn_object("E1", OWNER, 16, 13, 64, rules).unwrap();
    hello(&mut sim, gi, htk);
    send(&mut sim, htk, gi);
    assert_eq!(
        entity(&sim, htk).radio_contacts.slot(0),
        Some(gi),
        "sent to it"
    );

    let carrier = sim.spawn_object("HTK", OWNER, 16, 22, 64, rules).unwrap();
    let rider = sim.spawn_object("MTNK", OWNER, 12, 22, 64, rules).unwrap();
    let elsewhere = sim.spawn_object("MTNK", OWNER, 24, 22, 64, rules).unwrap();
    hello(&mut sim, rider, carrier);
    send(&mut sim, carrier, elsewhere);
    assert_eq!(
        entity(&sim, carrier).radio_contacts.slot(0),
        Some(rider),
        "a vehicle contact"
    );

    let tank = sim.spawn_object("MTNK", OWNER, 16, 27, 64, rules).unwrap();
    let walker = sim.spawn_object("E1", OWNER, 13, 27, 64, rules).unwrap();
    hello(&mut sim, walker, tank);
    send(&mut sim, tank, elsewhere);
    assert_eq!(
        entity(&sim, tank).radio_contacts.slot(0),
        Some(walker),
        "no Passengers="
    );
}

/// A transport's DOCKING asks a passenger it holds NEED_TO_MOVE and sends
/// him MOVE_HERE with itself (Unit `0x00737994..0x00737A95`). One already in
/// its cell answers ALREADY_THERE (Foot `0x004D9139..0x004D9189`), and the
/// transport BREAKs the contact but answers ROGER, so his Mission_Enter
/// (`0x004D9290`) does not idle him.
#[test]
fn retail_a_refused_move_here_breaks_the_passengers_contact() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let ifv = sim.spawn_object("FV", OWNER, 16, 16, 64, rules).unwrap();
    let gi = sim.spawn_object("E1", OWNER, 15, 16, 64, rules).unwrap();
    let enter = crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Enter);
    sim.substrate.mission_assign_exact(gi, enter, 0).unwrap();
    assert_eq!(
        crate::sim::radio::transmit(
            &mut sim,
            gi,
            ifv,
            crate::sim::radio::RadioMessage::Hello,
            crate::sim::radio::RadioPayload::default(),
            rules,
            crate::sim::world::FrameEffects::default(),
        ),
        crate::sim::radio::RadioResponse::Roger
    );
    let at = crate::sim::movement::ground_pose::position_world_coord(&entity(&sim, ifv).position);
    sim.foot_set_location_marked(
        gi,
        at,
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    crate::sim::world::dispatch_foot_mission(
        &mut sim,
        gi,
        rules,
        crate::sim::world::ObjectAiCtx::default(),
    );

    let pax = entity(&sim, gi);
    assert!(pax.radio_contacts.is_empty(), "the transport broke it");
    assert!(entity(&sim, ifv).radio_contacts.is_empty());
    assert_eq!(pax.mission.effective(), enter);
    assert_eq!(
        pax.mission.queued(),
        crate::sim::mission::MissionId::NONE,
        "he does not idle"
    );
    assert!(!pax.passenger_role.is_inside_transport());
}

/// A vehicle asks DOCKING only from Mission_Enter (`0x004D92B2`), once
/// TryEnterTransport (`0x0070D7E0`) has made the transport its contact. One
/// on Water takes the vehicle as its destination and answers DOCKING (Unit
/// `0x0073796B`), which Mission_Enter, untethered, takes as a refusal
/// (`0x004D92CE..0x004D92E8`): the vehicle breaks the contact and idles
/// while the transport heads for it.
#[test]
fn retail_a_vehicle_gives_up_on_a_transport_at_sea_that_comes_for_it() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    crate::sim::arena_fixture::flood(
        &mut sim,
        rules,
        32,
        (0..32).flat_map(|y| (19..32).map(move |x| (x, y))),
    );
    let grid = sim.path_grid_snapshot().unwrap();
    let boat = sim.spawn_object("SAPC", OWNER, 23, 16, 64, rules).unwrap();
    let tank = sim.spawn_object("HTNK", OWNER, 15, 16, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            tank,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(boat)
        ),
        Some(rules),
    ));
    assert_eq!(entity(&sim, tank).pending_entry(), Some(boat));
    let mut came = false;
    for _ in 0..300 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if entity(&sim, boat).navigation.nav_com()
            == Some(crate::sim::components::NavTargetRef::object(tank))
        {
            came = true;
            break;
        }
    }
    assert!(came, "the transport takes the vehicle as its destination");
    assert!(entity(&sim, tank).radio_contacts.is_empty());
    assert!(entity(&sim, boat).radio_contacts.is_empty());
    for _ in 0..30 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
    }
    let vehicle = entity(&sim, tank);
    assert_ne!(
        vehicle.mission.effective(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Enter),
        "the vehicle gave up"
    );
    assert!(!vehicle.passenger_role.is_inside_transport());
    assert!(
        entity(&sim, boat)
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
    );
}
