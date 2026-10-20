//! Structural fallout membership/restoration and concrete hut caller regressions.
use super::{
    blow_up_bridge_cell_fallout,
    tests::{seed_bridge_overlay, water_below_bridge_terrain},
};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{
    bridge_state::BridgeRuntimeState, components::DriveCoord, movement::locomotor::MovementLayer,
    world::Simulation,
};
use crate::util::save_codec;

#[test]
fn structural_fallout_retires_effect_only_ground_victim() {
    use crate::sim::house_state::HouseState;
    use crate::sim::world::{LifecycleTestEvent, SimSoundEvent};
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=100\nSpeed=4\n[CombatDamage]\nC4Warhead=KILL\n\
         [Warheads]\n0=KILL\n[KILL]\nInfDeath=3\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let mut terrain = water_below_bridge_terrain(4);
    // Admit the initial victim on clear ground before supplying the later
    // structural bridge state. Its ordinary Unlimbo needs Clear/Foot, and
    // this effect-only corridor supplies no OverlayType reader context.
    let clear = terrain.cell_mut_for_test(4, 4).unwrap();
    clear.speed_costs.foot = Some(100);
    clear.base_speed_costs.foot = Some(100);
    sim.resolved_terrain = Some(terrain);
    sim.bridge_state = Some(BridgeRuntimeState::default());
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 1000, 10));
    let victim = sim
        .spawn_object_at_height("E1", "Americans", 4, 4, 0, 0, &rules)
        .unwrap();
    seed_bridge_overlay(
        sim.resolved_terrain.as_mut().unwrap(),
        &[(4, 3), (4, 4), (4, 5)],
        0xD4,
    );
    assert!(!sim.substrate.entities.get(victim).unwrap().on_bridge());
    sim.substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    // Supplied structural47DD70 callback. Concrete ground overlays205..232
    // do not call this owner; the former hut fixture invented that dependency.
    blow_up_bridge_cell_fallout(
        &mut sim,
        &rules,
        4,
        4,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let object = sim.substrate.entities.get(victim).unwrap();
    assert!(!object.lifecycle.object_alive());
    assert!(object.death.is_dying() && !object.selection.is_selected());
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
    sim.advance_tick(&[], Some(&rules), None, None, 100);
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
fn structural_drop_in_owns_order_footprints_and_restore_without_teardown_side_effects() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n[BuildingTypes]\n0=BIG\n\
         [MTNK]\nStrength=300\nSpeed=6\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n[BIG]\nStrength=1000\n",
        ),
        &IniFile::from_str("[BIG]\nFoundation=2x1\n"),
    )
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
            .cell_mut_for_test(x, y)
            .unwrap();
        cell.level = 0;
        cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_deck_level = 4;
    }
    seed_bridge_overlay(
        sim.resolved_terrain.as_mut().unwrap(),
        &[(4, 3), (4, 4), (4, 5)],
        0xD4,
    );
    sim.bridge_state = Some(BridgeRuntimeState::default());
    let mut construct = |name: &str, x: u16, marked: bool| {
        let id = sim
            .construct_object_limbo_at_height(
                name,
                "Americans",
                x,
                4,
                0,
                4,
                &rules,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        {
            let actor = sim.substrate.entities.get_mut(id).unwrap();
            actor.position.put_location(DriveCoord {
                x: i32::from(x) * 256 + 128,
                y: 1152,
                z: 416,
            });
            // The unmarked twin retains the supplied legacy level as well as
            // exact Location; it deliberately never enters Unlimbo below.
            actor.position.set_z_for_test(4);
            actor.set_on_bridge(true);
        }
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
            .cell_marked()
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
        .locomotor
        .as_mut()
        .unwrap();
    assert!(drive.ensure_installed_track_state());
    let drive_before =
        save_codec::serialize(drive.selected_drive_runtime().unwrap().retained().unwrap()).unwrap();
    sim.substrate
        .entities
        .get_mut(newer)
        .unwrap()
        .set_foot_occupation_enabled(false);
    let next_order = sim.substrate.next_air_tracker_order.current();
    let raw_before = sim.substrate.raw_cell_occupation.clone();
    let mut smudge = crate::sim::smudge_grid::SmudgeGrid::new(10, 10);
    let decal = crate::sim::smudge_grid::SmudgeCell {
        type_id: Some(1),
        footprint_origin: Some((3, 4)),
        frame_offset: 0,
    };
    smudge.test_force_set(3, 4, decal);
    sim.smudge_grid = Some(smudge);

    // Supplied structural47DD70 callback. Concrete ground overlays205..232
    // do not call this owner; the former hut fixture invented that dependency.
    blow_up_bridge_cell_fallout(
        &mut sim,
        &rules,
        4,
        4,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let ground = vec![older, newer, building];
    for (x, expected) in [(4, ground.clone()), (3, vec![building])] {
        let cell = sim.substrate.occupancy.get(x, 4).unwrap();
        assert!(cell.snapshot_layer(MovementLayer::Bridge).is_empty());
        assert_eq!(cell.snapshot_layer(MovementLayer::Ground), expected);
    }
    for id in [newer, older, building] {
        let object = sim.substrate.entities.get(id).unwrap();
        assert!(!object.on_bridge() && object.lifecycle.cell_marked());
        assert_eq!(
            object.position.z(),
            4,
            "DropAsBomb retains the stored Location level"
        );
        assert_eq!(
            object.position.exact_z_leptons(),
            Some(if id == building { 0 } else { 416 })
        );
        assert!(object.was_dropped_as_bomb() && object.is_falling_down());
    }
    assert_eq!(sim.substrate.next_air_tracker_order.current(), next_order);
    let twin = sim.substrate.entities.get(unmarked).unwrap();
    assert!(twin.on_bridge() && twin.lifecycle.in_limbo() && !twin.lifecycle.cell_marked());
    assert_eq!(twin.position.z(), 4);
    assert_ne!(
        sim.substrate.raw_cell_occupation, raw_before,
        "the stationary Unit's raw receiver clears its old deck occupation"
    );
    let raw_after = sim.substrate.raw_cell_occupation.clone();
    assert_eq!(
        save_codec::serialize(
            sim.substrate
                .entities
                .get(older)
                .unwrap()
                .locomotor
                .as_ref()
                .and_then(|l| l.selected_drive_runtime())
                .and_then(|r| r.retained())
                .unwrap()
        )
        .unwrap(),
        drive_before
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
            .foot_occupation_enabled()
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
            .foot_occupation_enabled()
    );
    assert_eq!(restored.substrate.raw_cell_occupation, raw_after);
    assert_eq!(
        restored.substrate.next_air_tracker_order.current(),
        next_order,
        "ground list entries do not draw AirTracker orders"
    );
}

/// A bomb on a bridge-repair hut drops the hut's bridge after its blast
/// (`BombClass::Detonate`, `0x00438982`), whether its fuse runs out or the hut
/// dies carrying it (`0x00702672`); the same death without a bomb leaves the
/// bridge standing.
#[test]
fn a_bombed_bridge_hut_drops_its_bridge() {
    use crate::sim::house_state::HouseState;
    let ini = "[InfantryTypes]\n0=IVAN\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n0=CABHUT\n\
         [IVAN]\nStrength=125\nSpeed=4\n[CABHUT]\nStrength=1000\n\
         BridgeRepairHut=yes\n[CombatDamage]\nIvanWarhead=IvanWH\nIvanDamage=450\n\
         IvanTimedDelay=450\n[Warheads]\n0=IvanWH\n1=Super\n\
         [IvanWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         CellSpread=1.5\n[Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
    for (bombed, killed) in [(true, false), (true, true), (false, true)] {
        let (mut sim, rules, registry) =
            crate::sim::world::entry_test_fixture::fixture_with_rules(ini);
        for y in [14, 15, 16] {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut_for_test(17, y)
                .unwrap()
                .bridge_facts
                .overlay_id = Some(0xD4);
            sim.overlay_grid
                .as_mut()
                .unwrap()
                .place_overlay(17, y, 0xD4, 0);
        }
        sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain_with_map_size(
            sim.resolved_terrain.as_ref().unwrap(),
            true,
            300,
            (16, 16),
        ));
        for (side, name) in ["Americans", "Russians"].into_iter().enumerate() {
            let house = sim.interner.intern(name);
            sim.houses
                .register(HouseState::new(house, side as u8, None, false, 1000, 10));
        }
        // Beside the span, inside its 5x5 scan.
        let hut = sim
            .spawn_object_at_height("CABHUT", "Americans", 15, 15, 0, 0, &rules)
            .unwrap();
        let ivan = sim
            .spawn_object_at_height("IVAN", "Russians", 12, 15, 0, 0, &rules)
            .unwrap();
        sim.session.set_binary_frame_for_test(100);
        if bombed {
            sim.bomb_attach(ivan, Some(hut), &rules);
        }
        if killed {
            let super_wh = sim.interner.intern("Super");
            let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
                hut,
                1000,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                super_wh,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: false,
                },
            );
            sim.commit_direct_damage_receiver(
                &rules,
                Some(&registry),
                hit,
                crate::sim::world::FrameEffects::default(),
            );
        } else {
            sim.session.set_binary_frame_for_test(100 + 451);
            sim.bomb_fuse_step(
                hut,
                &rules,
                Some(&registry),
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(
                sim.substrate.entities.get(hut).unwrap().health.current(),
                1000 - 450,
                "the hut takes the blast"
            );
        }
        assert!(sim.bomb_carriers().is_empty());
        for y in [14, 15, 16] {
            let cell = sim.resolved_terrain.as_ref().unwrap().cell(17, y).unwrap();
            assert_eq!(
                cell.bridge_facts.overlay_id == Some(0xE7),
                bombed,
                "bombed {bombed}, killed {killed}: cell (17, {y})"
            );
        }
    }
}
