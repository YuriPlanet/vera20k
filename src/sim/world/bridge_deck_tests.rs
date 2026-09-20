//! Production hut-collapse membership and restoration regressions.
use super::{
    dispatch_bridge_collapse_from_hut_with_overlay_registry,
    tests::{seed_bridge_cell, water_below_bridge_terrain},
};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{
    bridge_state::{BridgeRuntimeState, DamageState},
    movement::locomotor::MovementLayer,
    world::Simulation,
};

#[test]
fn infantry_terminal_hut_collapse_retires_effect_only_ground_victim() {
    use crate::sim::house_state::HouseState;
    use crate::sim::world::{LifecycleTestEvent, SimSoundEvent};
    use std::collections::BTreeMap;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=100\nSpeed=4\n[CombatDamage]\nC4Warhead=KILL\n\
         [Warheads]\n0=KILL\n[KILL]\nInfDeath=3\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolved_terrain = Some(water_below_bridge_terrain(4));
    let mut bridge = BridgeRuntimeState::default();
    for y in [3, 4, 5] {
        bridge.test_seed_cell(4, y, seed_bridge_cell(0xD4));
    }
    sim.bridge_state = Some(bridge);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1000, 10));
    sim.session.house_order.push(owner);
    let victim = sim
        .spawn_object_at_height("E1", "Americans", 4, 4, 0, 0, &rules)
        .unwrap();
    assert!(!sim.substrate.entities.get(victim).unwrap().on_bridge);
    sim.substrate.entities.get_mut(victim).unwrap().selected = true;
    assert!(dispatch_bridge_collapse_from_hut_with_overlay_registry(
        &mut sim,
        &rules,
        (4, 4),
        None
    ));
    let object = sim.substrate.entities.get(victim).unwrap();
    assert!(object.infantry_terminal.is_none());
    assert!(!object.lifecycle.object_alive);
    assert!(object.dying && !object.selected);
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, victim));
    assert!(!sim.live_object_order_snapshot().contains(&victim));
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(event,
        SimSoundEvent::UnitLost { owner: lost, .. } if *lost == owner))
            .count(),
        1
    );
    sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 100);
    assert!(!sim.substrate.entities.contains(victim));
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, victim));
    assert!(!sim.live_object_order_snapshot().contains(&victim));
    assert_eq!(
        sim.lifecycle_test_events_for_test()
            .iter()
            .filter(|event| matches!(event,
        LifecycleTestEvent::FinalizedCommon { stable_id } if *stable_id == victim))
            .count(),
        1
    );
}

#[test]
fn hut_drop_in_owns_order_footprints_and_restore_without_teardown_side_effects() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n[BuildingTypes]\n0=BIG\n\
         [MTNK]\nStrength=300\nSpeed=6\n[BIG]\nStrength=1000\nFoundation=2x1\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolved_terrain = Some(water_below_bridge_terrain(4));
    for (x, y) in [(4, 3), (4, 4), (4, 5), (3, 4)] {
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(x, y)
            .unwrap();
        cell.level = 0;
        cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_deck_level = 4;
    }
    let mut bridge = BridgeRuntimeState::default();
    for y in [3, 4, 5] {
        bridge.test_seed_cell(4, y, seed_bridge_cell(0xD4));
    }
    sim.bridge_state = Some(bridge);
    let mut construct = |name: &str, x: u16, marked: bool| {
        let id = sim
            .construct_object_limbo_at_height(name, "Americans", x, 4, 0, 4, &rules)
            .unwrap();
        sim.substrate.entities.get_mut(id).unwrap().on_bridge = true;
        if marked {
            sim.reveal(id);
        }
        id
    };
    let older = construct("MTNK", 4, true);
    let newer = construct("MTNK", 4, true);
    let building = construct("BIG", 3, true);
    let unmarked = construct("MTNK", 4, false);
    assert!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .lifecycle
            .cell_marked
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .get(4, 4)
            .unwrap()
            .snapshot_layer(MovementLayer::Bridge),
        vec![newer, older, building]
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .get(3, 4)
            .unwrap()
            .snapshot_layer(MovementLayer::Bridge),
        vec![building]
    );
    let drive = sim
        .substrate
        .entities
        .get_mut(older)
        .unwrap()
        .drive_locomotion
        .get_or_insert_with(Default::default);
    drive.occupation_head_to = Some(crate::sim::components::DriveOccupationFootprint {
        rx: 8,
        ry: 8,
        layer: MovementLayer::Ground,
    });
    drive.occupation_handoff = Some(crate::sim::components::DriveOccupationFootprint {
        rx: 9,
        ry: 8,
        layer: MovementLayer::Bridge,
    });
    let drive_before = bincode::serialize(drive).unwrap();
    sim.substrate
        .cell_occupation
        .reconcile_entity(sim.substrate.entities.get(older).unwrap());
    sim.substrate
        .entities
        .get_mut(newer)
        .unwrap()
        .foot_occupation_enabled = false;
    sim.substrate
        .cell_occupation
        .reconcile_entity(sim.substrate.entities.get(newer).unwrap());
    let next_order = sim.substrate.next_occupancy_enter_order.current();
    let raw_before = sim.substrate.raw_cell_occupation.clone();
    let mut smudge = crate::sim::smudge_grid::SmudgeGrid::new(10, 10);
    let decal = crate::sim::smudge_grid::SmudgeCell {
        type_id: Some(1),
        footprint_origin: Some((3, 4)),
        frame_offset: 0,
    };
    smudge.test_force_set(3, 4, decal);
    sim.smudge_grid = Some(smudge);

    assert!(dispatch_bridge_collapse_from_hut_with_overlay_registry(
        &mut sim,
        &rules,
        (4, 4),
        None
    ));
    for y in [3, 4, 5] {
        let cell = sim.bridge_state.as_ref().unwrap().cell(4, y).unwrap();
        assert_eq!(cell.overlay_byte, 0xE7);
        assert_eq!(cell.damage_state, DamageState::Destroyed);
    }
    let ground = vec![older, newer, building];
    for (x, expected) in [(4, ground.clone()), (3, vec![building])] {
        let cell = sim.substrate.occupancy.get(x, 4).unwrap();
        assert!(cell.snapshot_layer(MovementLayer::Bridge).is_empty());
        assert_eq!(cell.snapshot_layer(MovementLayer::Ground), expected);
    }
    for (offset, id) in [newer, older, building].into_iter().enumerate() {
        let object = sim.substrate.entities.get(id).unwrap();
        assert!(!object.on_bridge && object.lifecycle.cell_marked);
        assert_eq!(
            object.position.z, 0,
            "existing represented ground snap retained"
        );
        assert_eq!(object.occupancy_enter_order, next_order + offset as u64);
    }
    assert_eq!(
        sim.substrate.next_occupancy_enter_order.current(),
        next_order + 3
    );
    let twin = sim.substrate.entities.get(unmarked).unwrap();
    assert!(twin.on_bridge && twin.lifecycle.in_limbo && !twin.lifecycle.cell_marked);
    assert_eq!(twin.position.z, 4);
    assert_eq!(
        sim.substrate.raw_cell_occupation, raw_before,
        "raw callback/falling drift is not rewritten from the legacy snapped Z"
    );
    assert_eq!(
        bincode::serialize(
            sim.substrate
                .entities
                .get(older)
                .unwrap()
                .drive_locomotion
                .as_ref()
                .unwrap()
        )
        .unwrap(),
        drive_before
    );
    for (x, y, layer) in [
        (4, 4, MovementLayer::Ground),
        (8, 8, MovementLayer::Ground),
        (9, 8, MovementLayer::Bridge),
    ] {
        assert_ne!(sim.substrate.cell_occupation.vehicle_bits(x, y, layer), 0);
    }
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(4, 4, MovementLayer::Bridge),
        0
    );
    assert_eq!(
        *sim.smudge_grid.as_ref().unwrap().cell(3, 4),
        decal,
        "relayer is not a building placement"
    );

    assert!(
        !sim.substrate
            .entities
            .get(newer)
            .unwrap()
            .foot_occupation_enabled
    );
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits_ignoring(4, 4, MovementLayer::Ground, older),
        0,
        "a cleared current footprint stays cleared while list membership relayers"
    );
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "bridge-deck", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored
            .substrate
            .occupancy
            .get(4, 4)
            .unwrap()
            .snapshot_layer(MovementLayer::Ground),
        ground
    );
    assert_eq!(
        restored
            .substrate
            .occupancy
            .get(3, 4)
            .unwrap()
            .snapshot_layer(MovementLayer::Ground),
        vec![building]
    );
    assert!(
        !restored
            .substrate
            .entities
            .get(newer)
            .unwrap()
            .foot_occupation_enabled
    );
    assert_eq!(
        restored.substrate.cell_occupation.vehicle_bits_ignoring(
            4,
            4,
            MovementLayer::Ground,
            older
        ),
        0
    );
    assert_eq!(restored.substrate.raw_cell_occupation, raw_before);
    assert_eq!(
        restored.substrate.next_occupancy_enter_order.current(),
        next_order + 3
    );
    for (x, y, layer) in [
        (4, 4, MovementLayer::Ground),
        (8, 8, MovementLayer::Ground),
        (9, 8, MovementLayer::Bridge),
    ] {
        assert_eq!(
            restored.substrate.cell_occupation.vehicle_bits(x, y, layer),
            sim.substrate.cell_occupation.vehicle_bits(x, y, layer)
        );
    }
}
