//! Production bridge-drop/ObjectAI/C4/Unit/passenger regressions. Expected
//! flow is instruction-derived (5F4160, 5F3F11..405B, 737F80..7381BC);
//! these tests establish Rust integration, not rendered gamemd parity.

use super::{FrameEffects, LifecycleTestEvent, Simulation};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet, terrain_rules::LandType};
use crate::sim::world::bridge_orchestrator::blow_up_bridge_cell_fallout;
use crate::sim::{
    command::Command,
    components::DriveCoord,
    house_state::HouseState,
    movement::{PerCellReason, ground_pose, locomotor::MovementLayer},
};
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;

const CELL: (u16, u16) = (15, 15);
const EXTRA: &str = "[General]\nWake=DROPWAKE\nAlliedCrew=ENGINEER\nAlliedSurvivorDivisor=1\n\
    [VehicleTypes]\n0=TRANSPORT\n\
    [TRANSPORT]\nStrength=200\nSpeed=6\nPassengers=5\nSizeLimit=2\nCrewed=yes\n\
    Cost=1000\nExplosion=DROPBOOM\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
    [CombatDamage]\nC4Warhead=FORCEDC4\nSplashList=DROPSMALL,DROPLARGE\n\
    [Warheads]\n2=FORCEDC4\n\
    [FORCEDC4]\nVerses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\nInfDeath=3\n";
const ART: &str = "[DROPWAKE]\nEnd=40\n[DROPSMALL]\nEnd=40\n\
    [DROPLARGE]\nEnd=40\n[DROPBOOM]\nEnd=40\n";

fn houses(sim: &mut Simulation) {
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let owner = sim.interner.intern(name);
        sim.houses
            .insert(owner, HouseState::new(owner, side, None, true, 0, 10));
        sim.session.house_order.push(owner);
    }
}

/// Board through the shared command/mission/arrival operation; no independent
/// cargo mutation or escape implementation is introduced by this fixture.
fn boarded_transport(sim: &mut Simulation, rules: &RuleSet, kind: &str) -> (u64, u64) {
    houses(sim);
    let carrier = sim
        .spawn_object(kind, "Americans", CELL.0, CELL.1, 0, rules)
        .unwrap();
    let passenger = sim
        .spawn_object("ENGINEER", "Americans", CELL.0 - 1, CELL.1, 0, rules)
        .unwrap();
    assert!(
        sim.apply_command(
            "Americans",
            &Command::EnterTransport {
                passenger_id: passenger,
                transport_id: carrier
            },
            Some(rules),
        ),
        "board failed: passenger={:?} transport={:?} admitted={}",
        sim.substrate.entities.get(passenger).map(|e| (
            e.is_deployed(),
            e.position.rx(),
            e.position.ry(),
            e.lifecycle.in_limbo()
        )),
        sim.substrate.entities.get(carrier).map(|e| (
            e.position.rx(),
            e.position.ry(),
            e.lifecycle.in_limbo()
        )),
        crate::sim::passenger::can_enter_transport(sim, rules, passenger, carrier)
    );
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..240 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        if sim
            .substrate
            .entities
            .get(passenger)
            .unwrap()
            .passenger_role
            .is_inside_transport()
        {
            break;
        }
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(passenger)
            .unwrap()
            .passenger_role
            .inside_transport_id(),
        Some(carrier)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(carrier)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers(),
        [passenger],
    );
    assert!(
        sim.substrate
            .entities
            .get(passenger)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    (carrier, passenger)
}

/// Supplied intact-deck scene; the ordinary Mark owner relayers the transport
/// around its initial Location. Collapse below will use the real producer.
fn put_on_deck(sim: &mut Simulation, rules: &RuleSet, id: u64, water: bool) {
    sim.foot_mark_remove(id, Some(rules), None, FrameEffects::default());
    let cell = sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(CELL.0, CELL.1)
        .unwrap();
    cell.bridge_facts.raw_flags |= crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    cell.has_bridge_deck = true;
    cell.bridge_walkable = true;
    cell.bridge_deck_level = cell.level + 4;
    if water {
        cell.land_type = LandType::Water.as_index();
        cell.base_land_type = cell.land_type;
    }
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    let at = ground_pose::position_world_coord(&entity.position);
    entity.set_on_bridge(true);
    entity.position.set_z_for_test(4);
    entity.position.put_location(DriveCoord {
        z: BRIDGE_DECK_HEIGHT_LEPTONS,
        ..at
    });
    assert!(sim.foot_mark_put(id, Some(rules), None, FrameEffects::default()));
    assert_eq!(
        sim.substrate.occupancy.listed_layer(CELL.0, CELL.1, id),
        Some(MovementLayer::Bridge)
    );
}

fn remove_deck(sim: &mut Simulation) {
    let cell = sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(CELL.0, CELL.1)
        .unwrap();
    cell.bridge_facts.raw_flags &= !crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    cell.has_bridge_deck = false;
    cell.bridge_walkable = false;
}

fn run_drop(sim: &mut Simulation, rules: &RuleSet, carrier: u64, passenger: u64, water: bool) {
    let explosion_anims = rules
        .object(
            sim.interner
                .resolve(sim.substrate.entities.get(carrier).unwrap().type_ref()),
        )
        .unwrap()
        .explosion_anims
        .clone();
    put_on_deck(sim, rules, carrier, water);
    sim.substrate
        .entities
        .get_mut(carrier)
        .unwrap()
        .health
        .set_for_test(37);
    let owner = sim.substrate.entities.get(carrier).unwrap().owner();
    let landing_xy =
        ground_pose::position_world_xy(&sim.substrate.entities.get(carrier).unwrap().position);
    let losses = sim.houses[&owner].stats.units_lost();
    let existing = sim.substrate.entities.keys_sorted();
    sim.clear_lifecycle_test_events_for_test();
    remove_deck(sim);
    blow_up_bridge_cell_fallout(sim, rules, CELL.0, CELL.1, None, FrameEffects::default());
    let transport = sim.substrate.entities.get(carrier).unwrap();
    assert!(transport.is_falling_down() && transport.was_dropped_as_bomb());
    assert!(!transport.on_bridge());
    assert_eq!(
        transport.health.current(),
        37,
        "bridge callback arms fall without early damage"
    );
    assert_eq!(
        ground_pose::position_world_coord(&transport.position).z,
        BRIDGE_DECK_HEIGHT_LEPTONS
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .listed_layer(CELL.0, CELL.1, carrier),
        Some(MovementLayer::Ground)
    );

    let mut saw_descent = false;
    let mut anims_before_landing = Vec::new();
    for _ in 0..100 {
        let before_z = sim
            .substrate
            .entities
            .get(carrier)
            .map(|e| ground_pose::position_world_coord(&e.position).z);
        let grid = sim.path_grid_snapshot().unwrap();
        let before_anims = sim
            .substrate
            .anims
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let tick = sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
        assert!(tick.frame_committed);
        let Some(entity) = sim.substrate.entities.get(carrier) else {
            anims_before_landing = before_anims;
            break;
        };
        let z = ground_pose::position_world_coord(&entity.position).z;
        saw_descent |= before_z.is_some_and(|old| z < old);
        assert!(entity.is_falling_down());
        assert_eq!(entity.health.current(), 37, "damage waits for grounding");
        assert!(
            sim.substrate
                .entities
                .get(passenger)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
    }
    assert!(saw_descent, "normal ObjectAI advances the bridge fall");
    assert!(
        !sim.substrate.entities.contains(carrier),
        "landing's synchronous forced C4 kills carrier"
    );
    assert!(
        !sim.substrate.entities.contains(passenger),
        "bomb cargo dies without escape"
    );
    assert_eq!(sim.houses[&owner].stats.units_lost(), losses + 2);
    assert!(
        sim.substrate
            .entities
            .keys_sorted()
            .iter()
            .all(|id| existing.contains(id)),
        "arg6 suppresses vehicle crew"
    );
    assert!(
        !sim.substrate
            .occupancy
            .contains_entity(CELL.0, CELL.1, carrier)
    );
    let retired = sim
        .lifecycle_test_events_for_test()
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::UninitAliveCleared { stable_id } => Some(*stable_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        retired,
        [passenger, carrier],
        "cargo UnInit precedes carrier UnInit"
    );
    let played = sim
        .substrate
        .anims
        .iter()
        .filter(|(id, _)| !anims_before_landing.contains(id))
        .map(|(_, anim)| {
            (
                sim.interner.resolve(anim.type_id).to_owned(),
                anim.world_coord,
                anim.draw_flags,
                anim.z_adjust,
            )
        })
        .collect::<Vec<_>>();
    if water {
        let expected = [
            &rules.general.wake.name,
            rules.combat_damage.splash_list.last().unwrap(),
        ];
        let splash = played
            .iter()
            .filter(|anim| {
                expected.iter().any(|name| **name == anim.0) && [anim.1.x, anim.1.y] == landing_xy
            })
            .collect::<Vec<_>>();
        assert_eq!(splash.len(), 2, "Wake and last SplashList: {played:?}");
        assert_eq!(&splash[0].0, expected[0]);
        assert_eq!(&splash[1].0, expected[1]);
        assert_eq!(splash[0].1.z, 0);
        assert_eq!(splash[1].1.z, 5);
        for anim in splash {
            assert_eq!((anim.2, anim.3), (0x600, 0));
        }
    } else {
        assert!(!played.iter().any(|anim| anim.0 == rules.general.wake.name));
        assert!(played.iter().any(|anim| explosion_anims.contains(&anim.0)));
    }
}

#[test]
fn bridge_transport_falls_then_forced_c4_kills_cargo_on_land_and_water() {
    for water in [false, true] {
        let (mut sim, mut rules, _) = super::entry_test_fixture::fixture_with_rules_and_fixed_art(
            EXTRA,
            &IniFile::from_str(ART),
        );
        for name in ["DROPWAKE", "DROPSMALL", "DROPLARGE", "DROPBOOM"] {
            rules.bind_anim_frame_count_for_test(name, 40);
        }
        let (carrier, passenger) = boarded_transport(&mut sim, &rules, "TRANSPORT");
        run_drop(&mut sim, &rules, carrier, passenger, water);
    }
}

#[test]
fn vanished_structural_deck_per_cell_arms_the_same_object_fall() {
    let (mut sim, rules, _) = super::entry_test_fixture::fixture_with_rules(EXTRA);
    houses(&mut sim);
    let id = sim
        .spawn_object("TRANSPORT", "Americans", CELL.0, CELL.1, 0, &rules)
        .unwrap();
    put_on_deck(&mut sim, &rules, id, false);
    let before = sim.rng_state();
    remove_deck(&mut sim);
    sim.per_cell_process(
        id,
        PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    let entity = sim.substrate.entities.get(id).unwrap();
    assert!(
        entity.is_falling_down() && entity.was_dropped_as_bomb(),
        "percell state: on_bridge={} marked={} z={:?} layer={:?}",
        entity.on_bridge(),
        entity.lifecycle.cell_marked(),
        entity.position.exact_z_leptons(),
        entity.locomotor.as_ref().map(|l| l.layer())
    );
    assert!(!entity.on_bridge());
    assert_eq!(
        ground_pose::position_world_coord(&entity.position).z,
        BRIDGE_DECK_HEIGHT_LEPTONS
    );
    assert_eq!(entity.health.current(), 200);
    assert_eq!(sim.rng_state(), before);
    assert_eq!(
        sim.substrate.occupancy.listed_layer(CELL.0, CELL.1, id),
        Some(MovementLayer::Ground)
    );
}

/// The bomb byte kills cargo independently of the killing hit's force
/// flags (Unit737F80): being shot while still falling must never escape a
/// passenger merely because this ordinary hit did not use ignoreDefenses.
#[test]
fn a_transport_shot_mid_fall_kills_its_passenger_without_the_landing_force_flags() {
    use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
    let (mut sim, rules, _) = super::entry_test_fixture::fixture_with_rules(EXTRA);
    let (carrier, passenger) = boarded_transport(&mut sim, &rules, "TRANSPORT");
    let health_before = sim
        .substrate
        .entities
        .get(passenger)
        .unwrap()
        .health
        .current();
    put_on_deck(&mut sim, &rules, carrier, false);
    remove_deck(&mut sim);
    blow_up_bridge_cell_fallout(
        &mut sim,
        &rules,
        CELL.0,
        CELL.1,
        None,
        FrameEffects::default(),
    );
    assert!(
        sim.substrate
            .entities
            .get(carrier)
            .unwrap()
            .is_falling_down()
    );
    sim.clear_lifecycle_test_events_for_test();
    // Fixture SA has full Verses: this is a normal hit, unlike the zero-
    // Verses FORCEDC4 landing control. The byte itself selects KillPassengers.
    let warhead = sim.interner.intern("SA");
    sim.commit_direct_damage_receiver(
        &rules,
        None,
        EntityDamageEvent::direct_receiver(
            carrier,
            1000,
            0,
            RAD_NO_ATTACKER,
            None,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        ),
        FrameEffects::default(),
    );
    let passenger = sim.substrate.entities.get(passenger).unwrap();
    assert!(!passenger.lifecycle.object_alive());
    assert!(passenger.lifecycle.in_limbo(), "never Unlimboed to escape");
    assert_eq!(
        passenger.health.current(),
        health_before,
        "RecordKill and UnInit store no Health"
    );
    assert!(
        sim.substrate
            .entities
            .get(carrier)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers()
            .is_empty()
    );
}

#[test]
fn retail_ifv_bridge_fall_uses_retail_c4_and_water_splash_rules() {
    let Some(mut retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let wake = retail.rules.general.wake.name.clone();
    let splash = retail
        .rules
        .combat_damage
        .splash_list
        .last()
        .unwrap()
        .clone();
    retail.rules.bind_anim_frame_count_for_test(&wake, 100);
    retail.rules.bind_anim_frame_count_for_test(&splash, 100);
    let (mut sim, _, _) = super::entry_test_fixture::fixture();
    sim.intern_rule_type_ids(&retail.rules);
    sim.resolve_type_handles(&retail.rules);
    let (carrier, passenger) = boarded_transport(&mut sim, &retail.rules, "FV");
    run_drop(&mut sim, &retail.rules, carrier, passenger, true);
}
