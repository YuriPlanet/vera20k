//! Production boarding, cargo departure and save/load for stock IFVs.
//!
//! Native constructor/ReceiveGunner/RemoveGunner and cargo-head comparisons:
//! tools/spatial_oracle/ifv_turret_switching.{md,json}. These regressions feed
//! retail Battle rules through the ordinary spawn, command and passenger owners.
//! Boarding tests never assign the selected pair or cargo by hand; the empty
//! removal control starts its stale selection through the shared selector.

use super::{
    DepartureFailure, DepartureRoute, PassengerRole, depart_cargo_head, reveal_unloaded_passenger,
    tick_passenger_system,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_weapon::current_weapon;
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
            .insert(id, HouseState::new(id, side, None, true, 0, 10));
        sim.session.house_order.push(id);
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
        &Command::EnterTransport {
            passenger_id: passenger,
            transport_id: ifv,
        },
        Some(rules),
    ));
    assert!(matches!(
        entity(sim, passenger).passenger_role,
        PassengerRole::Boarding { target_transport_id } if target_transport_id == ifv
    ));
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
    assert_eq!(cargo.passenger_sizes, [1]);
    assert_eq!(cargo.total_size, 1);
    let pax = entity(sim, passenger);
    assert!(matches!(
        pax.passenger_role,
        PassengerRole::Inside { transport_id, open_topped: false } if transport_id == ifv
    ));
    assert!(pax.lifecycle.in_limbo);
    assert!(!sim.live_object_order_snapshot().contains(&passenger));
}

fn unload_at_clear_exit(sim: &mut Simulation, rules: &RuleSet, ifv: u64, passenger: u64) {
    let exit = (
        entity(sim, ifv).position.rx,
        entity(sim, ifv).position.ry + 1,
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
                reveal_unloaded_passenger(
                    sim,
                    rules,
                    ifv,
                    departing,
                    exit.0,
                    exit.1,
                    0,
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
    assert!(matches!(pax.passenger_role, PassengerRole::None));
    assert!(!pax.lifecycle.in_limbo);
    assert_eq!((pax.position.rx, pax.position.ry), exit);
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
        &Command::EnterTransport {
            passenger_id: passenger,
            transport_id: ifv,
        },
        Some(rules),
    ));
    let passenger = entity(&sim, passenger);
    assert_eq!(
        passenger.navigation.nav_com,
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
        &Command::EnterTransport {
            passenger_id: passenger,
            transport_id: ifv,
        },
        Some(rules)
    ));
    tick_passenger_system(
        &mut sim,
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(
        matches!(
            entity(&sim, passenger).passenger_role,
            PassengerRole::Boarding { .. }
        ),
        "an adjacent passenger must remain visible until it reaches the vehicle cell"
    );
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
        &Command::EnterTransport {
            passenger_id: passenger,
            transport_id: ifv,
        },
        Some(rules)
    ));
    assert_eq!(
        entity(&sim, passenger).navigation.nav_com,
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
                    queue: false,
                },
                Some(rules)
            ));
            assert_eq!(
                entity(&sim, passenger).navigation.nav_com,
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
            &Command::EnterTransport {
                passenger_id: passenger,
                transport_id: transport,
            },
            Some(rules),
        ));
        assert_eq!(
            entity(&sim, passenger).navigation.nav_com,
            Some(crate::sim::components::NavTargetRef::object(transport)),
            "{passenger_type}: Unit boarding must retain its live transport"
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
        assert_eq!(cargo.total_size, size);
        assert!(entity(&sim, passenger).lifecycle.in_limbo);
        assert!(!sim.live_object_order_snapshot().contains(&passenger));
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
        &Command::EnterTransport {
            passenger_id: ifv,
            transport_id: boat
        },
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
    assert_eq!(outer_cargo.total_size, rules.object("FV").unwrap().size);

    let terrain_template = sim.resolved_terrain.as_ref().unwrap().clone();
    let bytes = GameSnapshot::save(&sim, 0, 0, "nested repair IFV", 0);
    let mut dangling = GameSnapshot::load(&bytes).unwrap().sim;
    dangling
        .entities_mut()
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
                reveal_unloaded_passenger(
                    sim,
                    rules,
                    boat,
                    departing,
                    16,
                    20,
                    0,
                    crate::sim::world::FrameEffects::default(),
                )
            },
            crate::sim::world::FrameEffects::default()
        ),
        Ok(())
    );
    assert_eq!(entity(&sim, ifv).passenger_role.inside_transport_id(), None);
    assert!(!entity(&sim, ifv).lifecycle.in_limbo);
    assert_eq!(
        serde_json::to_value(entity(&sim, ifv).passenger_role.cargo().unwrap()).unwrap(),
        initial_cargo
    );
    assert_boarded(&sim, ifv, engineer);
    assert_current_weapon(&sim, rules, ifv, "RepairBullet");
    unload_at_clear_exit(&mut sim, rules, ifv, engineer);
    assert!(sim.apply_command(
        OWNER,
        &Command::EnterTransport {
            passenger_id: engineer,
            transport_id: ifv
        },
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
        &Command::EnterTransport {
            passenger_id: passenger,
            transport_id: ifv,
        },
        Some(rules)
    ));
    assert!(sim.apply_command(
        OWNER,
        &Command::Move {
            entity_id: passenger,
            target_rx: 16,
            target_ry: 17,
            queue: false,
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
    assert!(matches!(
        entity(&sim, passenger).passenger_role,
        PassengerRole::None
    ));
    assert!(!entity(&sim, passenger).lifecycle.in_limbo);
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
        sim.fire_events.clear();
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
