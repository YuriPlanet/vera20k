//! Focused regression tests for ordered lifecycle authority.

use std::collections::BTreeMap;

use crate::map::bridge_facts::{
    BRIDGE_FLAG_ANCHOR_SELF, BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts, BridgeStampFamily,
};
use crate::map::entities::{EntityCategory, MapEntity};
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
use crate::sim::anim_class::{AnimObject, AnimRuntime, AnimWorldCoord};
use crate::sim::animation::{Animation, SequenceKind};
use crate::sim::bridge_state::StateOutcome;
use crate::sim::combat::{AttackTarget, PendingInfantryFire, TargetKind};
use crate::sim::components::{
    C4PlantState, DriveLocomotionRuntime, DriveOccupationFootprint, Health, NavTargetRef,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::homing_movement::{HomingTarget, attach_homing_state};
use crate::sim::movement::locomotor::{AirMovePhase, LocomotorState, MovementLayer};
use crate::sim::occupancy::CellListInsertion;
use crate::sim::particles::ParticleSystem;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectileGuidance, ProjectilePayload,
    ProjectileSpawn, ProjectileTarget, ProjectileTrajectory, ProjectileVelocity,
    ProjectileVisualState, TargetExpiryPolicy, cell_target_coord,
};
use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};
use crate::sim::wave::{Wave, WaveRecordedCell};
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF64Bits;
use glam::IVec3;

use super::techno_ai::ObjectAiCtx;
use super::{
    LifecycleOutput, LifecycleTestEvent, PlacementEvidence, RevealFailure, RevealOutcome,
    RevealPosition, RevealRequest, Simulation,
};

pub(super) fn insert_entity(sim: &mut Simulation, stable_id: u64, category: EntityCategory) {
    let owner = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern("TEST");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        stable_id,
        2,
        3,
        0,
        0,
        owner,
        Health { current: 100 },
        type_ref,
        category,
        0,
        5,
        category != EntityCategory::Infantry,
    );
    entity.category = category;
    sim.substrate.entities.insert(entity);
}

fn insert_reservation_building(
    sim: &mut Simulation,
    stable_id: u64,
    owner_name: &str,
    rx: u16,
    ry: u16,
    foundation: &str,
    spacing: i32,
) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern(owner_name);
    sim.houses.entry(owner).or_insert_with(|| {
        HouseState::new(
            owner,
            0,
            None,
            owner_name.eq_ignore_ascii_case("Americans"),
            0,
            10,
        )
    });
    let type_ref = sim.interner.intern(&format!("BUILDING{stable_id}"));
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        stable_id,
        rx,
        ry,
        0,
        0,
        owner,
        Health { current: 100 },
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    entity.foundation = foundation.to_string();
    entity.base_reservation_spacing = Some(spacing);
    sim.substrate.entities.insert(entity);
    owner
}

fn request(rx: u16, ry: u16, placement: PlacementEvidence) -> RevealRequest {
    RevealRequest {
        position: RevealPosition {
            rx,
            ry,
            z: 2,
            sub_x: SimFixed::from_num(128),
            sub_y: SimFixed::from_num(64),
        },
        placement,
        logic_eligible: true,
    }
}

fn drive_ship_slope_rules() -> crate::rules::ruleset::RuleSet {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=DRIVE\n1=SHIP\n2=TTRAIN\n\
         [DRIVE]\nStrength=100\nSpeed=6\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [SHIP]\nStrength=100\nSpeed=6\nLocomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
         [TTRAIN]\nStrength=100\nSpeed=6\nIsTrain=yes\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    crate::rules::ruleset::RuleSet::from_ini(&ini).expect("Drive/Ship slope rules")
}

fn zero_speed_drive_ship_slope_rules() -> crate::rules::ruleset::RuleSet {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=ZUNITD\n1=ZMAPD\n2=ZLIMBOS\n3=ZWALK\n\
         [InfantryTypes]\n0=ZINFD\n\
         [AircraftTypes]\n0=ZAIRS\n\
         [BuildingTypes]\n0=ZBUILDD\n\
         [ZUNITD]\nStrength=100\nSpeed=0\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [ZMAPD]\nStrength=100\nSpeed=0\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [ZLIMBOS]\nStrength=100\nSpeed=0\nLocomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
         [ZWALK]\nStrength=100\nSpeed=0\nLocomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [ZINFD]\nStrength=100\nSpeed=0\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [ZAIRS]\nStrength=100\nSpeed=0\nLocomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
         [ZBUILDD]\nStrength=100\nSpeed=0\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    crate::rules::ruleset::RuleSet::from_ini(&ini).expect("zero-speed Drive/Ship rules")
}

fn packed_reservation_test_coord(x: i32, y: i32) -> u32 {
    u32::from(x as i16 as u16) | (u32::from(y as i16 as u16) << 16)
}

fn receiver_authority_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=TEST\n[BuildingTypes]\n0=BUILDING41\n\
         [TEST]\nStrength=100\nArmor=light\n\
         [BUILDING41]\nStrength=100\nArmor=concrete\nFoundation=1x1\n\
         [Warheads]\n0=KILLWH\n[KILLWH]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("receiver world-authority rules")
}

fn commit_receiver_authority_lethal_hit(sim: &mut Simulation, target_id: u64) {
    let rules = receiver_authority_rules();
    let warhead = sim.interner.intern("KILLWH");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        target_id,
        100,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit]);
}

#[test]
fn receiver_fatal_limbo_clears_elevated_ground_occupation() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 16, 16, 4, None);
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(3, 4, 4, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    commit_receiver_authority_lethal_hit(&mut sim, 1);

    assert!(sim.substrate.entities.get(1).unwrap().lifecycle.in_limbo);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(3, 4),
        0,
        "fatal Limbo must clear the elevated ground plane marked by Reveal"
    );
}

#[test]
fn receiver_fatal_limbo_clears_sparse_map_dummy_reservation() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);
    let allocated: Vec<_> = (0..16)
        .flat_map(|y| (0..16).map(move |x| (x, y)))
        .filter(|&cell| cell != (9, 9))
        .collect();
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .test_set_native_allocated_cells(&allocated);
    let owner = insert_reservation_building(&mut sim, 41, "Americans", 10, 10, "1x1", 1);
    sim.session.house_order.push(owner);
    assert!(matches!(
        sim.try_reveal_entity(41, common_raw_request(10, 10, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert_ne!(sim.substrate.base_reservations.dummy_mask(), 0);

    commit_receiver_authority_lethal_hit(&mut sim, 41);

    assert!(sim.substrate.entities.get(41).unwrap().lifecycle.in_limbo);
    assert_eq!(
        sim.substrate.base_reservations.dummy_mask(),
        0,
        "fatal Limbo must resolve the same sparse-map CellClass identity as Reveal"
    );
}

#[test]
fn receiver_garrison_survivor_keeps_height_aware_playfield_membership() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=OCCUPANT\n[BuildingTypes]\n0=GARRISON\n\
             [OCCUPANT]\nStrength=100\nArmor=none\n\
             [GARRISON]\nStrength=100\nArmor=concrete\nFoundation=2x2\nCanBeOccupied=yes\n\
             [Warheads]\n0=KILLWH\n[KILLWH]\nCellSpread=0\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("garrison survivor rules");
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    sim.playfield_bounds =
        Some(crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(16, 2, 2, 12, 3));
    install_common_raw_terrain(&mut sim, 16, 16, 4, None);
    let building_id = sim.allocate_stable_id();
    insert_entity(&mut sim, building_id, EntityCategory::Structure);
    let passenger_id = sim.allocate_stable_id();
    insert_entity(&mut sim, passenger_id, EntityCategory::Infantry);
    let building_type = sim.interner.intern("GARRISON");
    let passenger_type = sim.interner.intern("OCCUPANT");
    {
        let passenger = sim.substrate.entities.get_mut(passenger_id).unwrap();
        passenger.type_ref = passenger_type;
        passenger.passenger_role = PassengerRole::Inside {
            transport_id: building_id,
        };
    }
    {
        let building = sim.substrate.entities.get_mut(building_id).unwrap();
        building.type_ref = building_type;
        building.foundation = "2x2".to_string();
        let mut cargo = PassengerCargo::new(5, 1);
        assert!(cargo.board(passenger_id, 1));
        building.passenger_role = PassengerRole::Transport { cargo };
    }
    assert!(matches!(
        sim.try_reveal_entity(building_id, common_raw_request(13, 13, 4, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert!(
        sim.substrate
            .entities
            .get(building_id)
            .unwrap()
            .in_playfield
    );

    let warhead = sim.interner.intern("KILLWH");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        building_id,
        100,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit]);

    let passenger = sim.substrate.entities.get(passenger_id).unwrap();
    assert!(passenger.lifecycle.object_alive);
    assert!(!passenger.lifecycle.in_limbo);
    assert_eq!(
        (
            passenger.position.rx,
            passenger.position.ry,
            passenger.position.z
        ),
        (15, 15, 4)
    );
    // SellBuilding @ 0x00458060 calls Infantry Unlimbo; Techno Unlimbo
    // @ 0x006F6CC0..0x006F6CFE writes mode-one global MapClass membership,
    // including during the ejection helper's temporary editor-mode bracket.
    assert!(
        passenger.in_playfield,
        "survivor must use actual level-four map membership"
    );
}

#[test]
fn receiver_borrowed_map_uninit_clears_aircraft_bridge_occupation() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0xFE, Some((3, 4)));
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    sim.substrate.entities.get_mut(1).unwrap().on_bridge = true;
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(3, 4, 2, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);

    // Exercise the borrowed-map UnInit boundary, not Aircraft ReceiveDamage:
    // fatal Aircraft damage starts a crash and does not immediately UnInit.
    let terrain = sim.resolved_terrain.take();
    let bridge_state = sim.bridge_state.take();
    sim.uninit_with_context(
        1,
        super::UninitContext::with_terrain(terrain.as_ref())
            .with_bridge_state(bridge_state.as_ref()),
    );
    sim.resolved_terrain = terrain;
    sim.bridge_state = bridge_state;

    assert!(sim.substrate.entities.get(1).unwrap().lifecycle.in_limbo);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_11_structure_mark_clears_full_smudge_footprints_and_refinery_hole() {
    let mut sim = Simulation::with_seed(1);
    let stable_id = 41;
    insert_reservation_building(&mut sim, stable_id, "Americans", 5, 5, "3x3Refinery", 0);

    let mut grid = crate::sim::smudge_grid::SmudgeGrid::new(16, 16);
    for (rx, ry, frame_offset) in [(4, 5, 0), (5, 5, 1), (4, 6, 2), (5, 6, 3)] {
        grid.test_force_set(
            rx,
            ry,
            crate::sim::smudge_grid::SmudgeCell {
                type_id: Some(1),
                footprint_origin: Some((4, 5)),
                frame_offset,
            },
        );
    }
    grid.test_force_set(
        7,
        6,
        crate::sim::smudge_grid::SmudgeCell {
            type_id: Some(2),
            footprint_origin: Some((7, 6)),
            frame_offset: 0,
        },
    );
    grid.test_force_set(
        9,
        9,
        crate::sim::smudge_grid::SmudgeCell {
            type_id: Some(3),
            footprint_origin: Some((9, 9)),
            frame_offset: 0,
        },
    );
    let _ = grid.drain_dirty();
    sim.smudge_grid = Some(grid);

    assert!(matches!(
        sim.try_reveal_entity(stable_id, request(5, 5, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));

    let grid = sim.smudge_grid.as_ref().unwrap();
    for cell in [(4, 5), (5, 5), (4, 6), (5, 6), (7, 6)] {
        assert!(grid.cell(cell.0, cell.1).type_id.is_none(), "cell {cell:?}");
    }
    assert!(
        grid.cell(9, 9).type_id.is_some(),
        "a footprint outside the full refinery rectangle survives"
    );
    let expected = vec![(4, 5), (5, 5), (4, 6), (5, 6), (7, 6)];
    assert_eq!(sim.tactical_dirty_cells, expected);
    assert_eq!(sim.radar_terrain_dirty_cells, expected);
    assert_eq!(sim.radar_terrain_dirty_generation, 5);
}

fn gsi_04_16_edge_gate_rules() -> crate::rules::ruleset::RuleSet {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[BuildingTypes]\n0=GACNST\n1=CAOILD\n\
         [GACNST]\nStrength=1000\nFactory=BuildingType\n\
         [CAOILD]\nStrength=1000\n",
    );
    crate::rules::ruleset::RuleSet::from_ini(&ini).expect("edge-gate fixture rules")
}

fn gsi_04_16_dustbowl_bounds() -> crate::sim::cell_rect::PlayfieldBounds {
    crate::sim::cell_rect::PlayfieldBounds {
        base: 70,
        off_fc: 2,
        off_100: 8,
        off_104: 65,
        off_108: 62,
    }
}

fn common_raw_request(rx: u16, ry: u16, z: u8, sub_x: i32, sub_y: i32) -> RevealRequest {
    RevealRequest {
        position: RevealPosition {
            rx,
            ry,
            z,
            sub_x: SimFixed::from_num(sub_x),
            sub_y: SimFixed::from_num(sub_y),
        },
        placement: PlacementEvidence::MarkSucceeded,
        logic_eligible: true,
    }
}

pub(crate) fn common_raw_terrain_cell(
    rx: u16,
    ry: u16,
    level: u8,
    has_bridge_deck: bool,
) -> ResolvedTerrainCell {
    let bridge_facts = if has_bridge_deck {
        BridgeCellFacts {
            raw_flags: BRIDGE_FLAG_ANCHOR_SELF | BRIDGE_FLAG_STRUCTURAL,
            family: BridgeStampFamily::Nesw,
            direction: Some(0),
            ..BridgeCellFacts::default()
        }
    } else {
        BridgeCellFacts::default()
    };
    ResolvedTerrainCell {
        rx,
        ry,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level,
        filled_clear: false,
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: level,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: TerrainClass::Clear,
        speed_costs: SpeedCostProfile::default(),
        is_water: false,
        is_cliff_like: false,
        is_rough: false,
        is_road: false,
        accepts_smudge: false,
        allows_tiberium: false,
        height_in_pixels: 0,
        variant: 0,
        has_ramp: false,
        canonical_ramp: None,
        ground_walk_blocked: false,
        terrain_object_blocks: false,
        terrain_object_occupation: None,
        overlay_blocks: false,
        overlay_zone_type: None,
        outside_playfield: false,
        zone_type: 0,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: TerrainClass::Clear,
        base_speed_costs: SpeedCostProfile::default(),
        build_blocked: false,
        has_bridge_deck,
        bridge_walkable: has_bridge_deck,
        bridge_transition: false,
        bridge_deck_level: if has_bridge_deck {
            level.saturating_add(4)
        } else {
            level
        },
        bridge_layer: None,
        bridge_facts,
        tube_index: None,
        radar_left: [0, 0, 0],
        radar_right: [0, 0, 0],
        has_damaged_data: false,
        bridgehead_anchor_class_at_load: None,
    }
}

pub(super) fn install_common_raw_terrain(
    sim: &mut Simulation,
    width: u16,
    height: u16,
    level: u8,
    bridge_cell: Option<(u16, u16)>,
) {
    let cells = (0..height)
        .flat_map(|ry| {
            (0..width).map(move |rx| {
                common_raw_terrain_cell(rx, ry, level, bridge_cell == Some((rx, ry)))
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
    terrain.bind_shared_cell_dummy(sim.shared_cell_dummy.clone());
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain(&terrain, true, 1500),
    );
    sim.resolved_terrain = Some(terrain);
}

#[test]
fn grounded_ramp_reveal_commits_native_height_before_occupation() {
    // Ground=52 and deck=468 are original-code results in
    // tools/ramp_height_vectors.json: ramp_1_sub_128_128 and
    // signed_level_0_bridge_1. Elevated input is a coarse API preservation case.
    for (category, kind) in [
        (EntityCategory::Unit, LocomotorKind::Drive),
        (EntityCategory::Unit, LocomotorKind::Ship),
        (EntityCategory::Infantry, LocomotorKind::Walk),
    ] {
        for (on_bridge, requested_level, expected_z, deck) in [
            (false, 0, 52, false),
            (true, 4, 468, true),
            (false, 4, 416, false),
            (true, 7, 728, true),
        ] {
            let mut sim = Simulation::with_seed(71);
            install_common_raw_terrain(&mut sim, 8, 8, 0, Some((2, 2)));
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(2, 2)
                .unwrap()
                .slope_type = 1;
            insert_entity(&mut sim, 1, category);
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.locomotor = Some(LocomotorState::for_test_kind(kind));
            entity.on_bridge = on_bridge;
            let before_rng = sim.scenario_rng.logical_state();
            assert!(matches!(
                sim.try_reveal_entity(1, common_raw_request(2, 2, requested_level, 128, 128)),
                RevealOutcome::Revealed { .. }
            ));
            assert_eq!(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .position
                    .exact_z_leptons,
                Some(expected_z),
                "{kind:?} bridge={on_bridge} level={requested_level}"
            );
            assert_eq!(
                sim.substrate.raw_cell_occupation.deck_bits(2, 2) != 0,
                deck,
                "Mark(PUT) must read the committed ramp/deck coordinate"
            );
            assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        }
    }
}

#[test]
fn grounded_ramp_reveal_preserves_independent_height_owners_and_headless_inputs() {
    for kind in [
        LocomotorKind::Hover,
        LocomotorKind::Fly,
        LocomotorKind::Jumpjet,
        LocomotorKind::Rocket,
    ] {
        let mut sim = Simulation::new();
        install_common_raw_terrain(&mut sim, 8, 8, 0, None);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(2, 2)
            .unwrap()
            .slope_type = 1;
        insert_entity(&mut sim, 1, EntityCategory::Unit);
        sim.substrate.entities.get_mut(1).unwrap().locomotor =
            Some(LocomotorState::for_test_kind(kind));
        assert!(matches!(
            sim.try_reveal_entity(1, common_raw_request(2, 2, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .position
                .exact_z_leptons,
            None,
            "{kind:?}"
        );
    }

    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    sim.substrate.entities.get_mut(1).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(2, 2, 3, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .position
            .exact_z_leptons,
        None
    );

    // Actual paradrop ordering: attach while limbo, then Reveal. The ground
    // adapter must not reinstate a stale exact Z over the descent integrator.
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    let entity = sim.substrate.entities.get_mut(2).unwrap();
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    entity.position.exact_z_leptons = Some(52);
    assert!(
        crate::sim::movement::parachute_descent::begin_parachute_descent(
            &mut sim.substrate.entities,
            2,
            SimFixed::from_num(1200)
        )
    );
    assert!(matches!(
        sim.try_reveal_entity(2, common_raw_request(2, 2, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    let entity = sim.substrate.entities.get(2).unwrap();
    assert_eq!(entity.position.exact_z_leptons, None);
    assert_eq!(
        entity.parachute_state.as_ref().unwrap().altitude,
        SimFixed::from_num(1200)
    );
}

#[test]
fn grounded_ramp_position_survives_snapshot_and_idle_continuation() {
    let mut sim = Simulation::with_seed(71);
    assert_eq!(sim.allocate_stable_id(), 1);
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(2, 2)
        .unwrap()
        .slope_type = 1;
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.entities.get_mut(1).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(2, 2, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    // Native snapshot load resets this stream. Canonicalize the original too
    // so whole-world comparison measures continuation, not that load policy.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let bytes = GameSnapshot::save(&sim, 0, 0, "ramp-height", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("ramp snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("ramp restore");
    install_common_raw_terrain(&mut restored, 8, 8, 0, None);
    restored
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(2, 2)
        .unwrap()
        .slope_type = 1;
    assert_eq!(
        restored
            .substrate
            .entities
            .get(1)
            .unwrap()
            .position
            .exact_z_leptons,
        Some(52)
    );
    for _ in 0..3 {
        sim.advance_tick(&[], None, &BTreeMap::new(), None, None, 66);
        restored.advance_tick(&[], None, &BTreeMap::new(), None, None, 66);
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .position
                .exact_z_leptons,
            Some(52)
        );
        assert_eq!(restored.state_hash(), sim.state_hash());
    }
}

#[test]
fn drive_ship_slope_production_spawn_unlimbo_snaps_without_manual_rocking_state() {
    let rules = drive_ship_slope_rules();
    for (type_id, cell, slope, native_z) in [
        ("DRIVE", (2, 2), 5, 0),
        ("SHIP", (4, 2), 9, 104),
        ("TTRAIN", (6, 2), 12, 104),
    ] {
        let mut sim = Simulation::with_seed(0x51_0f_e);
        sim.session.binary_frame = 37;
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        });
        install_common_raw_terrain(&mut sim, 10, 5, 0, None);
        let terrain_cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.slope_type = slope;
        terrain_cell.speed_costs = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            float: Some(100),
            amphibious: Some(100),
            float_beach: Some(100),
            hover: Some(100),
        };
        terrain_cell.base_speed_costs = terrain_cell.speed_costs;
        let mut expected_rng = sim.scenario_rng.clone();
        let _constructor_word = expected_rng.next_u32();

        let stable_id = sim
            .spawn_object(
                type_id,
                "Americans",
                cell.0,
                cell.1,
                0,
                &rules,
                &BTreeMap::new(),
            )
            .expect("production spawn/unlimbo");
        let entity = sim.substrate.entities.get(stable_id).unwrap();
        // Original-code ramp_{5,9,12}_sub_128_128 fixtures; this reaches the
        // production constructor and Reveal, with no preloaded exact pose.
        assert_eq!(entity.position.exact_z_leptons, Some(native_z));
        assert!(
            entity.rocking.is_none(),
            "slope state is not manually injected"
        );
        assert_eq!(
            crate::sim::movement::slope_transition::state_for_entity(entity)
                .expect("active Drive/Ship state")
                .hash_fields(),
            (slope, slope, 37, 0),
            "successful Foot unlimbo snaps both slope bytes at the current frame"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "the Techno constructor consumes its one proven word and slope Unlimbo adds no draw"
        );
    }
}

#[test]
fn zero_speed_foot_drive_ship_payloads_survive_all_world_spawn_paths() {
    let rules = zero_speed_drive_ship_slope_rules();
    let mut sim = Simulation::with_seed(0x51_0f_e);
    sim.session.binary_frame = 71;
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    install_common_raw_terrain(&mut sim, 12, 8, 0, None);
    for (cell, slope) in [((2, 2), 5), ((4, 2), 8), ((6, 2), 11), ((8, 2), 14)] {
        let terrain_cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.slope_type = slope;
        terrain_cell.speed_costs = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            float: Some(100),
            amphibious: Some(100),
            float_beach: Some(100),
            hover: Some(100),
        };
        terrain_cell.base_speed_costs = terrain_cell.speed_costs;
    }

    for (type_id, cell, expected_kind, slope) in [
        ("ZUNITD", (2, 2), LocomotorKind::Drive, 5),
        ("ZINFD", (4, 2), LocomotorKind::Drive, 8),
        ("ZAIRS", (6, 2), LocomotorKind::Ship, 11),
    ] {
        let stable_id = sim
            .spawn_object(
                type_id,
                "Americans",
                cell.0,
                cell.1,
                0,
                &rules,
                &BTreeMap::new(),
            )
            .expect("zero-speed Foot production spawn/reveal");
        let entity = sim.substrate.entities.get(stable_id).unwrap();
        assert_eq!(
            entity.locomotor.as_ref().unwrap().active_kind(),
            expected_kind
        );
        assert_eq!(
            crate::sim::movement::slope_transition::state_for_entity(entity)
                .expect("constructor-owned Drive/Ship payload")
                .hash_fields(),
            (slope, slope, 71, 0),
            "successful reveal snaps the payload without test injection"
        );
    }

    let placement = MapEntity {
        owner: "Americans".to_string(),
        type_id: "ZMAPD".to_string(),
        health: 256,
        cell_x: 8,
        cell_y: 2,
        facing: 0,
        category: EntityCategory::Unit,
        sub_cell: 0,
        veterancy: 0,
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        structure_upgrades: [None, None, None],
        structure_ai_sellable: false,
    };
    assert_eq!(
        sim.spawn_from_map(&[placement], Some(&rules), &BTreeMap::new()),
        1
    );
    let map_entity = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref) == "ZMAPD")
        .unwrap();
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(map_entity)
            .unwrap()
            .hash_fields(),
        (14, 14, 71, 0),
        "scenario world spawn also constructs and reveals the zero-speed Drive payload"
    );

    let limbo_ship = sim
        .spawn_object_limbo_at_height("ZLIMBOS", "Americans", 10, 2, 0, 0, &rules)
        .expect("zero-speed limbo Ship");
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(limbo_ship).unwrap()
        )
        .unwrap()
        .hash_fields(),
        (0, 0, 71, 0),
        "limbo construction owns fresh class state without an Unlimbo snap"
    );

    for type_id in ["ZBUILDD", "ZWALK"] {
        let stable_id = sim
            .spawn_object_limbo_at_height(type_id, "Americans", 10, 3, 0, 0, &rules)
            .unwrap();
        assert!(
            sim.substrate
                .entities
                .get(stable_id)
                .unwrap()
                .locomotor
                .is_none(),
            "{type_id}: structures and non-Drive/Ship zero-speed types stay excluded"
        );
    }
}

#[test]
fn drive_ship_slope_failed_reveal_does_not_snap_or_consume_rng() {
    let mut sim = Simulation::with_seed(0x51_0f_e);
    sim.session.binary_frame = 40;
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(3, 4)
        .unwrap()
        .slope_type = 8;
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.entities.get_mut(1).unwrap().locomotor = Some(
        LocomotorState::for_test_kind_at_frame(LocomotorKind::Drive, 11),
    );
    let rng_before = sim.scenario_rng.logical_state();

    assert_eq!(
        sim.try_reveal_entity(1, request(3, 4, PlacementEvidence::MarkFailed)),
        RevealOutcome::Failed(RevealFailure::MarkFailed)
    );
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(1).unwrap()
        )
        .unwrap()
        .hash_fields(),
        (0, 0, 11, 0)
    );
    assert_eq!(sim.scenario_rng.logical_state(), rng_before);
}

#[test]
fn techno_playfield_ctor_unlimbo_and_movement_hysteresis() {
    use crate::map::playfield::PlayfieldBounds;

    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 32, 32, 0, None);
    let bounds = PlayfieldBounds::from_normalized_local_size(32, 2, 2, 24, 20);
    sim.playfield_bounds = Some(bounds);
    let inside = (0u16..32)
        .flat_map(|ry| (0u16..32).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
        .expect("mode-one inside cell");
    let outside = (0u16..32)
        .flat_map(|ry| (0u16..32).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| !bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
        .expect("mode-one outside cell");

    insert_entity(&mut sim, 1, EntityCategory::Unit);
    assert!(!sim.substrate.entities.get(1).unwrap().in_playfield);
    assert!(matches!(
        sim.try_reveal_entity(1, common_raw_request(inside.0, inside.1, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert!(
        sim.substrate.entities.get(1).unwrap().in_playfield,
        "TechnoClass::Unlimbo @ 0x006F6CFE establishes exact mode-one membership"
    );

    sim.substrate.entities.get_mut(1).unwrap().in_playfield = false;
    sim.promote_entity_playfield_membership_after_move(1);
    assert!(sim.substrate.entities.get(1).unwrap().in_playfield);
    sim.substrate.entities.get_mut(1).unwrap().position.rx = outside.0;
    sim.substrate.entities.get_mut(1).unwrap().position.ry = outside.1;
    sim.promote_entity_playfield_membership_after_move(1);
    assert!(
        sim.substrate.entities.get(1).unwrap().in_playfield,
        "ordinary Foot movement @ 0x006F511A promotes but never demotes"
    );
}

fn install_fly_aircraft(sim: &mut Simulation, stable_id: u64, altitude: SimFixed) {
    insert_entity(sim, stable_id, EntityCategory::Aircraft);
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Fly);
    locomotor.altitude = altitude;
    locomotor.set_fly_target_height(altitude.to_num::<i32>());
    sim.substrate
        .entities
        .get_mut(stable_id)
        .expect("aircraft")
        .locomotor = Some(locomotor);
}

#[test]
fn gsi_04_12_common_raw_occupation_ground_unit_links_marks_then_unlinks_clears() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));

    assert!(sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
    let linked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListLinked)
        .expect("object list link event");
    let marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationMarked)
        .expect("raw mark event");
    assert!(
        linked < marked,
        "selected object list must link before raw mark"
    );

    sim.lifecycle_test_events.clear();
    let _ = sim.object_conceal(1);

    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    let unlinked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("object list unlink event");
    let cleared = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationCleared)
        .expect("raw clear event");
    assert!(
        unlinked < cleared,
        "selected object list must unlink before raw clear"
    );
}

#[test]
fn gsi_04_12_common_raw_occupation_structural_deck_unit_tracks_production_collapse() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 2, Some((3, 4)));
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 6, 128, 128));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x20);
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Ground),
        1,
        "raw deck selection must not reuse the OnBridge object-list selector"
    );

    let _ = sim.object_conceal(1);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    {
        let terrain = sim.resolved_terrain.as_mut().expect("resolved terrain");
        let bridge_state = sim.bridge_state.as_mut().expect("bridge runtime state");
        assert!(matches!(
            bridge_state.body_cell_advance_state(3, 4, true, terrain),
            StateOutcome::Absorbed { .. }
        ));
        assert!(matches!(
            bridge_state.body_cell_advance_state(3, 4, true, terrain),
            StateOutcome::Collapsed { .. }
        ));
        assert!(
            bridge_state
                .cell(3, 4)
                .expect("collapsed bridge cell")
                .deck_present,
            "collapse leaves the structural deck record present"
        );
        assert!(!bridge_state.is_bridge_walkable(3, 4));
    }

    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.try_reveal_entity(2, common_raw_request(3, 4, 6, 128, 128));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    let _ = sim.object_conceal(2);

    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(3, 4),
        0x20,
        "height-only clear still targets deck after the collapsed mark used ground"
    );
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_signed_z_marks_ground_on_live_bridge() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, Some((3, 4)));
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0x80, 128, 128));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    let _ = sim.object_conceal(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);

    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    let _ = sim.try_reveal_entity(2, common_raw_request(3, 4, 0x80, 192, 64));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x04);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_signed_ground_pins_mark_and_height_clear() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0xFE, Some((3, 4)));
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 2, 128, 128));

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x20);

    let _ = sim.object_conceal(1);

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_high_nonstructural_unit_keeps_native_stale_ground_bit() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 2, None);
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 6, 128, 128));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x20);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    let _ = sim.object_conceal(1);

    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(3, 4),
        0x20,
        "height-only clear targets deck even though nonstructural mark used ground"
    );
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_infantry_masks_follow_coordinates_and_never_bit_02() {
    let mut sim = Simulation::new();
    let cases = [
        (1, 1, 1, 128, 128, 0x01),
        (2, 2, 1, 64, 64, 0x01),
        (3, 3, 1, 192, 64, 0x04),
        (4, 4, 1, 64, 192, 0x08),
        (5, 5, 1, 192, 192, 0x10),
        (6, 6, 1, 128, 64, 0x01),
        (7, 7, 1, 192, 128, 0x04),
        (8, 8, 1, 128, 192, 0x08),
        (9, 9, 1, 64, 128, 0x01),
    ];

    for &(stable_id, rx, ry, sub_x, sub_y, expected_mask) in &cases {
        insert_entity(&mut sim, stable_id, EntityCategory::Infantry);
        let _ = sim.try_reveal_entity(stable_id, common_raw_request(rx, ry, 0, sub_x, sub_y));
        let bits = sim.substrate.raw_cell_occupation.ground_bits(rx, ry);
        assert_eq!(bits, expected_mask, "coordinate ({sub_x},{sub_y})");
        assert_eq!(bits & 0x02, 0, "GetSubCell never produces raw bit 0x02");
    }
}

#[test]
fn gsi_04_12_common_raw_occupation_infantry_marks_after_link_then_clears() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 192, 64));
    let linked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListLinked)
        .expect("Infantry list link");
    let marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationMarked)
        .expect("Infantry raw mark");
    assert!(linked < marked);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x04);
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(3, 4),
        Some(sim.substrate.entities.get(1).unwrap().owner())
    );

    sim.lifecycle_test_events.clear();
    let _ = sim.object_conceal(1);

    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(3, 4),
        None
    );
    assert!(
        sim.lifecycle_test_events
            .contains(&LifecycleTestEvent::RawOccupationListUnlinked),
        "object-list unlink precedes the independent raw occupation clear"
    );
    assert!(
        sim.lifecycle_test_events
            .contains(&LifecycleTestEvent::RawOccupationCleared)
    );
}

#[test]
fn gsi_04_12_common_raw_occupation_infantry_above_deck_height_marks_and_clears_deck() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 1, Some((3, 4)));
    insert_entity(&mut sim, 1, EntityCategory::Infantry);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 6, 192, 64));

    assert!(sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x04);
    assert_eq!(
        sim.substrate.raw_cell_occupation.deck_infantry_owner(3, 4),
        Some(sim.substrate.entities.get(1).unwrap().owner())
    );
    assert!(
        sim.lifecycle_test_events
            .contains(&LifecycleTestEvent::RawOccupationListLinked)
    );
    assert!(
        sim.lifecycle_test_events
            .contains(&LifecycleTestEvent::RawOccupationMarked)
    );

    let _ = sim.object_conceal(1);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
    assert_eq!(
        sim.substrate.raw_cell_occupation.deck_infantry_owner(3, 4),
        None
    );
}

#[test]
fn gsi_04_12_common_raw_occupation_high_nonstructural_infantry_retains_mark_plane_asymmetry() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 2, None);
    insert_entity(&mut sim, 1, EntityCategory::Infantry);

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 6, 192, 64));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x04);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    let _ = sim.object_conceal(1);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(3, 4),
        0x04,
        "InfantryClass::Unmark selects the high plane by Z without retesting Flags&0x100"
    );
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_04_12_common_raw_occupation_building_foundation_is_ground_only() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Structure);
    {
        let building = sim.substrate.entities.get_mut(1).expect("building");
        building.foundation = "2x2".to_string();
        building.on_bridge = true;
    }

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 9, 128, 128));

    for (rx, ry) in [(3, 4), (3, 5), (4, 4), (4, 5)] {
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(rx, ry), 0x80);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(rx, ry), 0);
        assert_eq!(
            sim.substrate
                .occupancy
                .count_on_layer(rx, ry, MovementLayer::Bridge),
            1
        );
    }

    let _ = sim.object_conceal(1);
    for (rx, ry) in [(3, 4), (3, 5), (4, 4), (4, 5)] {
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(rx, ry), 0);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(rx, ry), 0);
    }
}

#[test]
fn gsi_04_05_hidden_lifecycle_follows_base_lists_without_expanding_them() {
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    insert_entity(&mut sim, 1, EntityCategory::Structure);
    {
        let building = sim.substrate.entities.get_mut(1).expect("building");
        building.foundation = "2x2".to_string();
        let profile = building
            .building_hidden_occupancy
            .as_mut()
            .expect("structure constructor profile");
        profile.add_occupy[0] = Some((-1, 0));
        profile.remove_occupy[0] = Some((1, 1));
    }

    let _ = sim.try_reveal_entity(1, common_raw_request(4, 4, 0, 128, 128));
    assert!(sim.substrate.occupancy.contains_entity(5, 5, 1));
    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.hidden_occupation.count(3, 4), 1);
    assert_eq!(sim.substrate.hidden_occupation.count(5, 5), 0);
    let linked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListLinked)
        .expect("base lists linked");
    let hidden = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::HiddenOccupationEntered)
        .expect("hidden counters entered");
    assert!(linked < hidden);

    sim.lifecycle_test_events.clear();
    let _ = sim.object_conceal(1);
    assert!(!sim.substrate.occupancy.contains_entity(5, 5, 1));
    assert_eq!(sim.substrate.hidden_occupation.entry_count(), 0);
    let unlinked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("base lists unlinked");
    let hidden = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::HiddenOccupationExited)
        .expect("hidden counters exited");
    assert!(unlinked < hidden);
}

#[test]
fn gsi_04_12_common_raw_occupation_skips_transport_and_airborne_entities() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.entities.get_mut(1).unwrap().passenger_role =
        PassengerRole::Inside { transport_id: 99 };
    let _ = sim.try_reveal_entity(1, common_raw_request(2, 3, 0, 128, 128));
    assert!(!sim.substrate.occupancy.contains_entity(2, 3, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(2, 3), 0);

    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Drive);
    locomotor.layer = MovementLayer::Air;
    sim.substrate.entities.get_mut(2).unwrap().locomotor = Some(locomotor);
    let _ = sim.try_reveal_entity(2, common_raw_request(5, 6, 0, 128, 128));
    assert!(!sim.substrate.occupancy.contains_entity(5, 6, 2));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(5, 6), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(5, 6), 0);
}

#[test]
fn gsi_04_12_object_raw_occupation_landed_fly_reveal_and_conceal_are_ordered() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));

    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Ground),
        1
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
    let linked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListLinked)
        .expect("landed Fly list link");
    let marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationMarked)
        .expect("landed Fly raw mark");
    assert!(linked < marked);

    sim.lifecycle_test_events.clear();
    let _ = sim.object_conceal(1);

    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    let unlinked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("landed Fly list unlink");
    let cleared = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationCleared)
        .expect("landed Fly raw clear");
    assert!(unlinked < cleared);
}

#[test]
fn gsi_04_12_object_raw_occupation_airborne_and_non_fly_aircraft_are_excluded() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(1));
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));

    insert_entity(&mut sim, 2, EntityCategory::Aircraft);
    sim.substrate.entities.get_mut(2).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Rocket));
    let _ = sim.try_reveal_entity(2, common_raw_request(5, 6, 0, 128, 128));

    for (stable_id, rx, ry) in [(1, 3, 4), (2, 5, 6)] {
        assert!(!sim.substrate.occupancy.contains_entity(rx, ry, stable_id));
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(rx, ry), 0);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(rx, ry), 0);
    }
}

#[test]
fn gsi_04_12_object_raw_occupation_deck_clear_rechecks_live_structural_state() {
    let mut sim = Simulation::new();
    // Signed level -2 makes z=2 exactly four normalized levels above ground.
    install_common_raw_terrain(&mut sim, 8, 8, 0xFE, Some((3, 4)));
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    sim.substrate.entities.get_mut(1).unwrap().on_bridge = true;

    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 2, 128, 128));
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Bridge),
        1
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);

    let _ = sim.object_conceal(1);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    install_fly_aircraft(&mut sim, 2, SimFixed::from_num(0));
    sim.substrate.entities.get_mut(2).unwrap().on_bridge = true;
    let _ = sim.try_reveal_entity(2, common_raw_request(3, 4, 2, 128, 128));
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);

    {
        let terrain = sim.resolved_terrain.as_mut().expect("resolved terrain");
        let bridge_state = sim.bridge_state.as_mut().expect("bridge runtime state");
        assert!(matches!(
            bridge_state.body_cell_advance_state(3, 4, true, terrain),
            StateOutcome::Absorbed { .. }
        ));
        assert!(matches!(
            bridge_state.body_cell_advance_state(3, 4, true, terrain),
            StateOutcome::Collapsed { .. }
        ));
        assert!(!bridge_state.is_bridge_walkable(3, 4));
    }

    let _ = sim.object_conceal(2);
    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 2));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(
        sim.substrate.raw_cell_occupation.deck_bits(3, 4),
        0x40,
        "generic clear retargets ground after collapse and leaves the deck bit stale"
    );
}

#[test]
fn gsi_04_12_object_raw_occupation_production_fly_tick_unmarks_takeoff_and_marks_landing() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, None);
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);

    {
        let locomotor = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();

        locomotor.set_fly_target_height(600);
    }
    sim.tick_air_movement_with_cell_lists_one(1, None);

    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert!(aircraft.locomotor.as_ref().unwrap().altitude > SimFixed::from_num(0));
    assert!(
        aircraft.lifecycle.cell_marked,
        "the post-process Mark transaction completed"
    );
    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);

    {
        let locomotor = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();
        locomotor.begin_fly_landing();
        locomotor.altitude = SimFixed::from_num(1);
        locomotor.set_fly_target_height(0);
    }
    // Supply the matching physical state for this independent landing visit.
    // Changing cached controller height cannot move an exact Object coordinate.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .exact_z_leptons = Some(1);
    sim.tick_air_movement_with_cell_lists_one(1, None);

    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        aircraft.locomotor.as_ref().unwrap().altitude,
        SimFixed::from_num(0)
    );
    assert!(aircraft.lifecycle.cell_marked);
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Ground),
        1
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);
}

#[test]
fn gsi_05_05_fly_takeoff_commits_absolute_z_after_remove_process() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 2, None);
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 2, 128, 128));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);

    {
        let locomotor = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();

        locomotor.set_fly_target_height(600);
    }
    sim.tick_air_movement_with_cell_lists_one(1, None);

    let aircraft = sim.substrate.entities.get(1).unwrap();
    let altitude = aircraft
        .locomotor
        .as_ref()
        .unwrap()
        .altitude
        .to_num::<i32>();
    assert!(altitude > 0);
    assert_eq!(aircraft.position.exact_z_leptons, Some(208 + altitude));
    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_05_05_fly_landing_on_bridge_uses_absolute_z_for_deck_put() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 0, Some((3, 4)));
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(1));
    sim.substrate.entities.get_mut(1).unwrap().on_bridge = true;
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);

    {
        let locomotor = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();
        locomotor.begin_fly_landing();
        locomotor.set_fly_target_height(0);
    }
    sim.tick_air_movement_with_cell_lists_one(1, None);

    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        aircraft.locomotor.as_ref().unwrap().altitude,
        SimFixed::from_num(0)
    );
    assert_eq!(aircraft.position.exact_z_leptons, Some(416));
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 4, MovementLayer::Bridge),
        1
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);
}

#[test]
fn gsi_05_05_object_raw_occupation_uses_exact_sloped_ground_z_for_put_and_remove() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 8, 8, 1, Some((3, 4)));
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(3, 4)
        .unwrap()
        .slope_type = 1;
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(0));
    sim.substrate.entities.get_mut(1).unwrap().on_bridge = true;
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 64, 192));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);
    sim.remove_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);

    // Level 1 plus slope 1 at local X=64 produces exact ground Z 130.
    // Keep coarse Z deliberately above the deck so only exact Object Z can
    // distinguish the two sides of the inclusive 416-lepton threshold.
    {
        let aircraft = sim.substrate.entities.get_mut(1).unwrap();
        aircraft.position.z = 0x7f;
        aircraft.position.exact_z_leptons = Some(130 + 415);
    }
    sim.add_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0x40);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
    sim.remove_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .exact_z_leptons = Some(130 + 416);
    sim.add_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0x40);
    sim.remove_entity_occupancy(1);
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(3, 4), 0);
}

#[test]
fn gsi_05_05_mapless_fly_uses_dummy_ground_then_bridge_height() {
    let mut sim = Simulation::new();
    install_fly_aircraft(&mut sim, 1, SimFixed::from_num(100));
    {
        let aircraft = sim.substrate.entities.get_mut(1).unwrap();
        aircraft.on_bridge = true;
        aircraft
            .locomotor
            .as_mut()
            .unwrap()
            .set_fly_target_height(100);
    }
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 2, 128, 128));

    sim.tick_air_movement_with_cell_lists_one(1, None);

    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        aircraft.position.exact_z_leptons,
        Some(416 + 100),
        "missing terrain uses native dummy-cell ground zero before OnBridge"
    );
}

#[test]
fn gsi_04_07_damage_air_spatial_entry_crossing_and_exit_keep_vector_order() {
    let mut sim = Simulation::new();
    sim.session.map_width = 40;
    sim.session.map_height = 40;
    install_common_raw_terrain(&mut sim, 40, 40, 0, None);
    // Foot Unlimbo4D72B2 adds ConsideredAircraft only above the native
    // high-flight threshold (208 leptons). The altitude cache uses leptons,
    // whereas the Reveal request below uses coarse levels: four levels = 416.
    install_fly_aircraft(&mut sim, 20, SimFixed::from_num(416));
    install_fly_aircraft(&mut sim, 10, SimFixed::from_num(416));

    let _ = sim.try_reveal_entity(20, common_raw_request(2, 4, 4, 128, 128));
    let _ = sim.try_reveal_entity(10, common_raw_request(3, 4, 4, 128, 128));
    let first = sim.substrate.entities.get(20).unwrap();
    let second = sim.substrate.entities.get(10).unwrap();
    assert!(first.air_spatial_bucket.is_some());
    assert_eq!(first.air_spatial_bucket, second.air_spatial_bucket);
    assert!(
        first.air_spatial_enter_order < second.air_spatial_enter_order,
        "same-bucket vector retains append order, not stable-ID order"
    );
    let first_order = first.air_spatial_enter_order;
    let shared_bucket = second.air_spatial_bucket;
    let second_order = second.air_spatial_enter_order;

    sim.tick_air_movement_with_cell_lists_one(20, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(20)
            .unwrap()
            .air_spatial_enter_order,
        first_order,
        "Fly's temporary cell-list transaction is not an air-vector re-entry"
    );

    sim.substrate.entities.get_mut(20).unwrap().position.rx = 12;
    sim.tick_air_movement_with_cell_lists_one(20, None);
    let crossed = sim.substrate.entities.get(20).unwrap();
    assert_ne!(crossed.air_spatial_bucket, shared_bucket);
    assert!(crossed.air_spatial_enter_order > second_order);

    let _ = sim.object_conceal(20);
    let exited = sim.substrate.entities.get(20).unwrap();
    assert_eq!(exited.air_spatial_bucket, None);
    assert_eq!(exited.air_spatial_enter_order, 0);
}

fn insert_anim(sim: &mut Simulation, stable_id: u64, inactive: bool) {
    let type_id = sim.interner.intern("TESTANIM");
    let anim = AnimObject {
        stable_id,
        native_unique_id: stable_id as i32,
        type_id,
        world_coord: AnimWorldCoord { x: 0, y: 0, z: 0 },
        draw_flags: 0,
        z_adjust: 0,
        remap_color: None,
        effective_end: 1,
        effective_loop_end: 1,
        runtime: AnimRuntime {
            current_frame: 0,
            frame_step: 1,
            delay_remaining: 0,
            rate_reload: 1,
            frame_timer: crate::sim::timer::CdTimer::started(0, 1),
            loop_remaining: 0,
            first_ai_guard: false,
            constructor_reverse: false,
            inactive,
            paused: false,
        },
        draw_runtime: crate::sim::anim_class::AnimDrawRuntime::default(),
        use_cell_drawer: false,
        terrain_attached: false,
        in_logic_vector: false,
        owner_entity: None,
        building_slot: None,
        damage_fire_slot: None,
        start_sound_active: false,
        stop_sound_id: None,
        display: Default::default(),
        bounce: None,
    };
    assert!(sim.substrate.anims.insert(anim).is_none());
}

fn insert_particle_system(sim: &mut Simulation, stable_id: u64) {
    sim.substrate.particle_systems.insert(ParticleSystem {
        stable_id,
        in_logic_vector: false,
        type_id: crate::rules::particle_system_type::ParticleSystemTypeId(0),
        coords: IVec3::ZERO,
        offset: IVec3::ZERO,
        particles: Vec::new(),
        spawn_timer: SimFixed::from_num(0),
        lifetime: -1,
        spark_spawn_frames: 0,
        facing: 0,
        directionless: true,
        attached_entity: None,
        owner_entity: None,
        target_coords: IVec3::ZERO,
        owner_house: None,
        done_spawning: false,
    });
}

#[test]
fn lifecycle_authority_reveal_commits_coords_then_marks_then_registers() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed {
            logic_registered: true
        }
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (entity.position.rx, entity.position.ry, entity.position.z),
        (10, 20, 2)
    );
    assert_eq!(entity.position.sub_x, SimFixed::from_num(128));
    assert_eq!(entity.position.sub_y, SimFixed::from_num(64));
    assert!(!entity.lifecycle.in_limbo);
    assert!(entity.lifecycle.cell_marked);
    assert!(entity.in_logic_vector);
    assert!(sim.substrate.occupancy.contains_entity(10, 20, 1));
    assert_eq!(sim.live_object_order_snapshot(), vec![1]);

    let expected = [
        LifecycleTestEvent::RevealLimboCleared,
        LifecycleTestEvent::RevealCoordinatesCommitted,
        LifecycleTestEvent::MarkPut,
        LifecycleTestEvent::RawOccupationListLinked,
        LifecycleTestEvent::RawOccupationMarked,
        LifecycleTestEvent::CellMarked,
        LifecycleTestEvent::RevealDisplayBoundary,
        LifecycleTestEvent::LogicAppended,
        LifecycleTestEvent::LogicMembershipSet,
    ];
    assert_eq!(sim.lifecycle_test_events.as_slice(), expected.as_slice());
}

#[test]
fn reveal_stops_after_mark_when_object_is_not_alive() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .lifecycle
        .object_alive = false;

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed {
            logic_registered: false
        }
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.lifecycle.in_limbo);
    assert!(entity.lifecycle.cell_marked);
    assert!(!entity.in_logic_vector);
    assert!(sim.lifecycle_outputs.is_empty());
}

#[test]
fn lifecycle_authority_reveal_mark_failure_keeps_adjusted_coords_alive_limbo() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkFailed)),
        RevealOutcome::Failed(RevealFailure::MarkFailed)
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (entity.position.rx, entity.position.ry, entity.position.z),
        (10, 20, 2)
    );
    assert!(entity.lifecycle.object_alive);
    assert!(entity.lifecycle.in_limbo);
    assert!(!entity.lifecycle.cell_marked);
    assert!(!entity.in_logic_vector);
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::RevealLimboCleared,
            LifecycleTestEvent::RevealCoordinatesCommitted,
            LifecycleTestEvent::MarkPut,
        ]
    );
}

#[test]
fn lifecycle_authority_reveal_early_reject_commits_nothing() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let before = sim.substrate.entities.get(1).unwrap().position.clone();

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::RejectedEarly)),
        RevealOutcome::Failed(RevealFailure::RejectedEarly)
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (before.rx, before.ry)
    );
    assert!(entity.lifecycle.in_limbo);
    assert!(sim.lifecycle_test_events.is_empty());
}

#[test]
fn lifecycle_authority_reveal_rejects_an_already_marked_limbo_object() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let before = sim.substrate.entities.get(1).unwrap().position.clone();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .lifecycle
        .cell_marked = true;

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Failed(RevealFailure::RejectedEarly)
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (before.rx, before.ry)
    );
    assert!(entity.lifecycle.in_limbo);
    assert!(entity.lifecycle.cell_marked);
    assert!(sim.lifecycle_test_events.is_empty());
}

#[test]
fn lifecycle_authority_reveal_logic_failure_keeps_successful_mark() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.logic.force_next_insert_failure_for_test();

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed {
            logic_registered: false
        }
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.lifecycle.in_limbo);
    assert!(entity.lifecycle.cell_marked);
    assert!(!entity.in_logic_vector);
    assert!(sim.substrate.occupancy.contains_entity(10, 20, 1));
}

#[test]
fn lifecycle_authority_logic_flag_sets_after_append() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    sim.register_live_object(1);
    assert_eq!(sim.live_object_order_snapshot(), vec![1]);
    assert!(sim.substrate.entities.get(1).unwrap().in_logic_vector);
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::LogicAppended,
            LifecycleTestEvent::LogicMembershipSet,
        ]
    );
}

#[test]
fn lifecycle_authority_logic_append_failure_leaves_flag_clear() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.logic.force_next_insert_failure_for_test();

    sim.register_live_object(1);
    assert!(sim.live_object_order_snapshot().is_empty());
    assert!(!sim.substrate.entities.get(1).unwrap().in_logic_vector);
    assert!(sim.lifecycle_test_events.is_empty());
}

#[test]
fn lifecycle_authority_logic_remove_compacts_then_clears_flag() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    insert_entity(&mut sim, 3, EntityCategory::Unit);
    sim.register_live_object(1);
    sim.register_live_object(2);
    sim.register_live_object(3);

    sim.unregister_live_object(2);
    assert_eq!(sim.live_object_order_snapshot(), vec![1, 3]);
    assert!(sim.substrate.entities.get(1).unwrap().in_logic_vector);
    assert!(!sim.substrate.entities.get(2).unwrap().in_logic_vector);
    assert!(sim.substrate.entities.get(3).unwrap().in_logic_vector);
}

#[test]
fn lifecycle_authority_flagged_missing_remove_still_clears_flag() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.entities.get_mut(1).unwrap().in_logic_vector = true;
    assert!(sim.live_object_order_snapshot().is_empty());

    sim.unregister_live_object(1);
    assert!(sim.live_object_order_snapshot().is_empty());
    assert!(!sim.substrate.entities.get(1).unwrap().in_logic_vector);
}

#[test]
fn particle_logic_membership_uses_the_object_local_guard_and_rebuilds_it() {
    let mut sim = Simulation::new();
    insert_particle_system(&mut sim, 7);

    assert!(sim.reveal_particle_system(7, None));
    assert!(
        sim.substrate
            .particle_systems
            .get(7)
            .unwrap()
            .in_logic_vector
    );
    assert_eq!(sim.live_object_order_snapshot(), vec![7]);

    assert!(sim.reveal_particle_system(7, None));
    assert_eq!(sim.live_object_order_snapshot(), vec![7]);

    sim.substrate
        .particle_systems
        .get_mut(7)
        .unwrap()
        .in_logic_vector = false;
    sim.rebuild_logic_membership();
    assert!(
        sim.substrate
            .particle_systems
            .get(7)
            .unwrap()
            .in_logic_vector
    );
    sim.debug_assert_logic_membership_consistent();

    assert!(sim.conceal_particle_system(7));
    assert!(
        !sim.substrate
            .particle_systems
            .get(7)
            .unwrap()
            .in_logic_vector
    );
    assert!(sim.live_object_order_snapshot().is_empty());
}

#[test]
fn open_topped_direct_registration_keeps_hidden_passenger_live() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let _ = sim.reveal(2);

    assert_eq!(sim.techno_limbo(1), super::ConcealOutcome::Concealed);
    sim.substrate.entities.get_mut(1).unwrap().passenger_role =
        PassengerRole::Inside { transport_id: 99 };

    assert!(sim.register_open_topped_passenger(1));
    let passenger = sim.substrate.entities.get(1).unwrap();
    assert!(passenger.lifecycle.object_alive);
    assert!(passenger.lifecycle.in_limbo);
    assert!(!passenger.lifecycle.cell_marked);
    assert!(passenger.in_logic_vector);
    assert_eq!(sim.live_object_order_snapshot(), vec![2, 1]);

    assert!(sim.register_open_topped_passenger(1));
    assert_eq!(sim.live_object_order_snapshot(), vec![2, 1]);
}

#[test]
fn lifecycle_authority_second_reveal_is_idempotent() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed {
            logic_registered: true
        }
    );
    let first_position = sim.substrate.entities.get(1).unwrap().position.clone();
    let first_enter_order = sim.substrate.entities.get(1).unwrap().occupancy_enter_order;
    sim.lifecycle_outputs.clear();
    sim.lifecycle_test_events.clear();

    assert_eq!(
        sim.try_reveal_entity(1, request(30, 40, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::AlreadyRevealed
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (
            entity.position.rx,
            entity.position.ry,
            entity.position.z,
            entity.position.sub_x,
            entity.position.sub_y,
        ),
        (
            first_position.rx,
            first_position.ry,
            first_position.z,
            first_position.sub_x,
            first_position.sub_y,
        )
    );
    assert_eq!(entity.occupancy_enter_order, first_enter_order);
    assert!(sim.substrate.occupancy.contains_entity(10, 20, 1));
    assert!(!sim.substrate.occupancy.contains_entity(30, 40, 1));
    assert_eq!(sim.live_object_order_snapshot(), vec![1]);
    assert!(sim.lifecycle_outputs.is_empty());
    assert!(sim.lifecycle_test_events.is_empty());
}

#[test]
fn lifecycle_authority_conceal_without_dirty_eligibility_still_clears_drawn_state() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    sim.lifecycle_outputs.clear();

    let _ = sim.object_conceal(1);
    assert_eq!(
        sim.lifecycle_outputs,
        vec![
            LifecycleOutput::DisplayRemove { stable_id: 1 },
            LifecycleOutput::DetachAttachedAnims { stable_id: 1 },
            LifecycleOutput::StopVoc { stable_id: 1 },
            LifecycleOutput::ClearDrawnState { stable_id: 1 },
            LifecycleOutput::ClearRedraw { stable_id: 1 },
        ]
    );
}

#[test]
fn lifecycle_authority_conceal_dirty_eligibility_emits_dirty_before_drawn_clear() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .dirty_rect_eligible = true;
    let _ = sim.reveal(1);
    sim.lifecycle_outputs.clear();

    let _ = sim.object_conceal(1);
    assert_eq!(
        &sim.lifecycle_outputs[3..],
        &[
            LifecycleOutput::DirtyTacticalRect { stable_id: 1 },
            LifecycleOutput::ClearDrawnState { stable_id: 1 },
            LifecycleOutput::ClearRedraw { stable_id: 1 },
        ]
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.lifecycle.object_alive);
    assert!(entity.lifecycle.in_limbo);
    assert!(!entity.lifecycle.cell_marked);
    assert!(!entity.in_logic_vector);
}

#[test]
fn lifecycle_authority_conceal_deselects_unmarks_unregisters_then_sets_limbo() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.selected = true;
    entity.dirty_rect_eligible = true;
    sim.lifecycle_test_events.clear();

    let _ = sim.object_conceal(1);
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::ConcealDeselected,
            LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                stable_id: 1,
                object_alive: true,
                cell_marked: true,
                resolvable: true,
            },
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 1,
                listener_id: 1,
                target_alive: true,
                target_in_limbo: false,
            },
            LifecycleTestEvent::RawOccupationListUnlinked,
            LifecycleTestEvent::RawOccupationCleared,
            LifecycleTestEvent::ConcealUnmarked,
            LifecycleTestEvent::ConcealDisplayBoundary,
            LifecycleTestEvent::ConcealAnimBoundary,
            LifecycleTestEvent::ConcealVocBoundary,
            LifecycleTestEvent::ConcealLogicRemoved,
            LifecycleTestEvent::ConcealDirtyTacticalRectBoundary,
            LifecycleTestEvent::ConcealClearDrawnStateBoundary,
            LifecycleTestEvent::ConcealLimboSet,
            LifecycleTestEvent::ConcealClearRedrawBoundary,
        ]
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.selected);
    assert!(!entity.lifecycle.cell_marked);
    assert!(!entity.in_logic_vector);
    assert!(entity.lifecycle.in_limbo);
}

#[test]
fn gsi_04_05_conceal_removes_only_selected_object_list_layer() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);

    let (rx, ry) = {
        let entity = sim.substrate.entities.get(1).unwrap();
        (entity.position.rx, entity.position.ry)
    };
    sim.substrate.occupancy.add(
        rx,
        ry,
        1,
        MovementLayer::Bridge,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(rx, ry, MovementLayer::Ground),
        1
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(rx, ry, MovementLayer::Bridge),
        1
    );

    let _ = sim.object_conceal(1);

    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(rx, ry, MovementLayer::Ground),
        0,
        "conceal must unlink the current ground-list entry"
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(rx, ry, MovementLayer::Bridge),
        1,
        "conceal must not cross-scan the bridge object list"
    );
    assert!(!sim.substrate.entities.get(1).unwrap().lifecycle.cell_marked);
}

#[test]
fn gsi_04_05_hard_limbo_clears_pending_then_current_vehicle_occupation() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let current = {
        let entity = sim.substrate.entities.get(1).unwrap();
        (entity.position.rx, entity.position.ry)
    };
    let head = (current.0 + 1, current.1);
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entity.drive_locomotion = Some(DriveLocomotionRuntime {
            occupation_head_to: Some(DriveOccupationFootprint {
                rx: head.0,
                ry: head.1,
                layer: MovementLayer::Ground,
            }),
            ..Default::default()
        });
    }
    sim.substrate
        .cell_occupation
        .mark_vehicle_on_layer(head.0, head.1, 1, MovementLayer::Ground);

    let _ = sim.object_conceal(1);

    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(head.0, head.1, MovementLayer::Ground),
        0
    );
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(current.0, current.1, MovementLayer::Ground),
        0
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .drive_locomotion
            .as_ref()
            .unwrap()
            .occupation_head_to,
        None
    );
}

#[test]
fn lifecycle_authority_conceal_outputs_match_release_order() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .dirty_rect_eligible = true;
    let _ = sim.reveal(1);
    sim.lifecycle_outputs.clear();

    let _ = sim.object_conceal(1);
    assert_eq!(
        sim.lifecycle_outputs,
        vec![
            LifecycleOutput::DisplayRemove { stable_id: 1 },
            LifecycleOutput::DetachAttachedAnims { stable_id: 1 },
            LifecycleOutput::StopVoc { stable_id: 1 },
            LifecycleOutput::DirtyTacticalRect { stable_id: 1 },
            LifecycleOutput::ClearDrawnState { stable_id: 1 },
            LifecycleOutput::ClearRedraw { stable_id: 1 },
        ]
    );
}

#[test]
fn lifecycle_authority_conceal_keeps_object_alive() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);

    let _ = sim.object_conceal(1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.lifecycle.object_alive);
    assert!(entity.lifecycle.in_limbo);
    assert!(!entity.dying);
    assert!(!sim.substrate.pending_delete.contains(&1));
}

#[test]
fn lifecycle_authority_techno_limbo_breaks_contacts_before_common_conceal() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let _ = sim.reveal(2);
    assert_eq!(
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .radio_contacts
            .insert(2),
        Some(0)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .radio_contacts
            .insert(1),
        Some(0)
    );
    sim.lifecycle_test_events.clear();

    let _ = sim.techno_limbo(1);
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::BreakSlot {
                slot: 0,
                target: Some(2),
            },
            LifecycleTestEvent::BreakSenderCleared { target: 2 },
            LifecycleTestEvent::BreakReceiverClassEffect { target: 2 },
            LifecycleTestEvent::BreakReceiverCleared { target: 2 },
            LifecycleTestEvent::ConcealDeselected,
            LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                stable_id: 1,
                object_alive: true,
                cell_marked: true,
                resolvable: true,
            },
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 1,
                listener_id: 1,
                target_alive: true,
                target_in_limbo: false,
            },
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 1,
                listener_id: 2,
                target_alive: true,
                target_in_limbo: false,
            },
            LifecycleTestEvent::RawOccupationListUnlinked,
            LifecycleTestEvent::RawOccupationCleared,
            LifecycleTestEvent::ConcealUnmarked,
            LifecycleTestEvent::ConcealDisplayBoundary,
            LifecycleTestEvent::ConcealAnimBoundary,
            LifecycleTestEvent::ConcealVocBoundary,
            LifecycleTestEvent::ConcealLogicRemoved,
            LifecycleTestEvent::ConcealClearDrawnStateBoundary,
            LifecycleTestEvent::ConcealLimboSet,
            LifecycleTestEvent::ConcealClearRedrawBoundary,
        ]
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .radio_contacts
            .is_empty()
    );
    assert!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .radio_contacts
            .is_empty()
    );
}

#[test]
fn lifecycle_authority_uninit_limbos_while_alive_then_clears_alive_then_queues() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    sim.lifecycle_test_events.clear();

    sim.uninit(1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.lifecycle.object_alive);
    assert!(entity.lifecycle.in_limbo);
    assert!(!entity.lifecycle.cell_marked);
    assert!(entity.dying);
    assert_eq!(sim.substrate.pending_delete, vec![1]);
    let limbo = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::ConcealLimboSet)
        .unwrap();
    let alive_clear = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                *event,
                LifecycleTestEvent::UninitAliveCleared { stable_id: 1 }
            )
        })
        .unwrap();
    let queued = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                *event,
                LifecycleTestEvent::PendingDeleteQueued { stable_id: 1 }
            )
        })
        .unwrap();
    assert!(limbo < alive_clear && alive_clear < queued);
}

#[test]
fn lifecycle_authority_uninit_removal_boundary_sees_alive_marked_target() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    sim.lifecycle_test_events.clear();

    sim.uninit(1);
    let removal = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                *event,
                LifecycleTestEvent::UninitRemovalNotifyBoundary {
                    stable_id: 1,
                    object_alive: true,
                    cell_marked: true,
                    resolvable: true,
                }
            )
        })
        .expect("removal boundary");
    let conceal = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::ConcealDeselected)
        .expect("common Conceal");
    assert!(removal < conceal);
}

#[test]
fn gsi_05_03_conceal_destroy_and_uninit_broadcast_before_unmark() {
    let mut conceal_only = Simulation::new();
    insert_entity(&mut conceal_only, 1, EntityCategory::Unit);
    let _ = conceal_only.reveal(1);
    conceal_only.lifecycle_test_events.clear();

    assert_eq!(
        conceal_only.object_conceal(1),
        super::ConcealOutcome::Concealed
    );
    let conceal_boundary = conceal_only
        .lifecycle_test_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                    stable_id: 1,
                    object_alive: true,
                    cell_marked: true,
                    resolvable: true,
                }
        })
        .expect("Conceal Destroy notification boundary");
    let conceal_unmark = conceal_only
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("Conceal unmark");
    assert!(conceal_boundary < conceal_unmark);
    assert!(conceal_only.substrate.entities.contains(1));
    assert!(
        conceal_only
            .substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .object_alive
    );

    let mut uninit = Simulation::new();
    insert_entity(&mut uninit, 1, EntityCategory::Unit);
    insert_entity(&mut uninit, 2, EntityCategory::Unit);
    let _ = uninit.reveal(2);
    uninit.lifecycle_test_events.clear();
    uninit.uninit(2);

    let direct = uninit
        .lifecycle_test_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::UninitRemovalNotifyBoundary {
                    stable_id: 2,
                    object_alive: true,
                    cell_marked: true,
                    resolvable: true,
                }
        })
        .expect("direct UnInit expiry boundary");
    let destroy = uninit
        .lifecycle_test_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                    stable_id: 2,
                    object_alive: true,
                    cell_marked: true,
                    resolvable: true,
                }
        })
        .expect("virtual Conceal Destroy expiry boundary");
    let unmark = uninit
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("UnInit Conceal unmark");
    let alive_clear = uninit
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::UninitAliveCleared { stable_id: 2 })
        .expect("UnInit alive clear");
    assert!(direct < destroy && destroy < unmark && unmark < alive_clear);
    assert_eq!(
        uninit
            .lifecycle_test_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 2, .. }
            ))
            .count(),
        4,
        "two listeners observe both expiry broadcasts"
    );
}

#[test]
fn gsi_05_03_dead_nonlimbo_stored_object_still_runs_conceal() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.lifecycle.object_alive = false;
        assert!(!entity.lifecycle.in_limbo);
        assert!(entity.lifecycle.cell_marked);
    }
    sim.lifecycle_test_events.clear();

    assert_eq!(sim.techno_limbo(1), super::ConcealOutcome::Concealed);
    let destroy = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                    stable_id: 1,
                    object_alive: false,
                    cell_marked: true,
                    resolvable: true,
                }
        })
        .expect("dead non-limbo object reached Conceal's Destroy boundary");
    let unmark = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("dead non-limbo object reached Conceal's Mark(REMOVE)");
    let limbo_set = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::ConcealLimboSet)
        .expect("dead non-limbo object completed Conceal");
    assert!(destroy < unmark && unmark < limbo_set);

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.lifecycle.object_alive);
    assert!(entity.lifecycle.in_limbo);
    assert!(!entity.lifecycle.cell_marked);
    assert!(!entity.in_logic_vector);
}

#[test]
fn lifecycle_authority_uninit_dead_limbo_remains_resolvable_until_drain() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);

    sim.uninit(1);
    let entity = sim
        .substrate
        .entities
        .get(1)
        .expect("dead object remains stored until the ordinary drain");
    assert!(!entity.lifecycle.object_alive);
    assert!(entity.lifecycle.in_limbo);
    assert!(!entity.lifecycle.cell_marked);
    assert!(!entity.in_logic_vector);
    assert_eq!(sim.substrate.pending_delete, vec![1]);

    sim.process_pending_delete();
    assert!(!sim.substrate.entities.contains(1));
    assert!(sim.substrate.pending_delete.is_empty());
}

#[test]
fn lifecycle_authority_transport_uninits_passengers_in_cargo_order_before_carrier_notify() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    insert_entity(&mut sim, 3, EntityCategory::Infantry);
    let _ = sim.reveal(1);
    let mut cargo = PassengerCargo::new(5, 1);
    cargo.board_forced(2, 1);
    cargo.board_forced(3, 1);
    sim.substrate.entities.get_mut(1).unwrap().passenger_role = PassengerRole::Transport { cargo };
    sim.substrate.entities.get_mut(2).unwrap().passenger_role =
        PassengerRole::Inside { transport_id: 1 };
    sim.substrate.entities.get_mut(3).unwrap().passenger_role =
        PassengerRole::Inside { transport_id: 1 };
    sim.lifecycle_test_events.clear();

    sim.uninit(1);
    // Cargo list order is head-first: `CargoClass::AddPassenger @ 0x004733A0`
    // prepends, so the passenger boarded last (3) is walked first.
    assert_eq!(sim.substrate.pending_delete, vec![3, 2, 1]);
    let notify_order = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::UninitRemovalNotifyBoundary { stable_id, .. } => Some(*stable_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(notify_order, vec![3, 2, 1]);
    assert!(matches!(
        sim.substrate.entities.get(2).unwrap().passenger_role,
        PassengerRole::None
    ));
    assert!(matches!(
        sim.substrate.entities.get(3).unwrap().passenger_role,
        PassengerRole::None
    ));
    let cargo = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .passenger_role
        .cargo()
        .unwrap();
    assert!(cargo.passengers.is_empty());
    assert!(cargo.passenger_sizes.is_empty());
    assert_eq!(cargo.total_size, 0);
    assert_eq!(cargo.garrison_fire_index, 0);
}

#[test]
fn uninit_pointer_expiry_walks_global_object_order_before_break_and_conceal() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_anim(&mut sim, 2, false);
    insert_particle_system(&mut sim, 3);
    insert_entity(&mut sim, 4, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let _ = sim.reveal(4);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .radio_contacts
        .insert(4);
    sim.substrate
        .entities
        .get_mut(4)
        .unwrap()
        .radio_contacts
        .insert(1);
    sim.lifecycle_test_events.clear();

    sim.uninit(4);

    let visited = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 4,
                listener_id,
                target_alive,
                target_in_limbo,
            } => Some((*listener_id, *target_alive, *target_in_limbo)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        visited,
        vec![
            (1, true, false),
            (2, true, false),
            (3, true, false),
            (4, true, false),
            (1, true, false),
            (2, true, false),
            (3, true, false),
            (4, true, false),
        ]
    );

    let break_slot = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleTestEvent::BreakSlot {
                    slot: 0,
                    target: Some(1)
                }
            )
        })
        .unwrap();
    let conceal = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::ConcealDeselected)
        .unwrap();
    let direct_last_listener = sim.lifecycle_test_events[..break_slot]
        .iter()
        .rposition(|event| {
            matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 4, .. }
            )
        })
        .unwrap();
    let conceal_notify = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleTestEvent::ConcealDestroyNotifyBoundary { stable_id: 4, .. }
            )
        })
        .unwrap();
    let second_last_listener = sim
        .lifecycle_test_events
        .iter()
        .rposition(|event| {
            matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 4, .. }
            )
        })
        .unwrap();
    let unmark = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .unwrap();
    assert!(direct_last_listener < break_slot && break_slot < conceal);
    assert!(conceal < conceal_notify && conceal_notify < second_last_listener);
    assert!(second_last_listener < unmark);
}

#[test]
fn pointer_expiry_clears_live_target_and_navigation_refs() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener.attack_target = Some(AttackTarget {
        target: TargetKind::Entity(2),
        pending_infantry_fire: None,
    });
    listener.suspended_attack_target = Some(TargetKind::Entity(2));
    listener.navigation.suspended_nav_com = Some(NavTargetRef::Entity { id: 2 });
    listener.navigation.nav_com_aux = Some(NavTargetRef::Object { id: 2 });
    listener.navigation.nav_com = Some(NavTargetRef::Building { id: 2 });
    listener.navigation.nav_queue = vec![
        NavTargetRef::Entity { id: 2 },
        NavTargetRef::Cell { rx: 7, ry: 8 },
        NavTargetRef::Object { id: 2 },
    ];
    listener.capture_target = Some(2);
    listener.c4_plant = Some(C4PlantState {
        target_building_id: 2,
    });

    sim.uninit(2);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert!(listener.attack_target.is_none());
    assert!(listener.suspended_attack_target.is_none());
    assert!(listener.navigation.suspended_nav_com.is_none());
    assert!(listener.navigation.nav_com_aux.is_none());
    assert!(listener.navigation.nav_com.is_none());
    assert_eq!(
        listener.navigation.nav_queue,
        vec![NavTargetRef::Cell { rx: 7, ry: 8 }]
    );
    assert!(listener.capture_target.is_none());
    assert!(listener.c4_plant.is_none());
}

#[test]
fn occupier_capture_retains_live_non_selling_nav_target() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Structure);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));
    sim.mission_assign_exact(
        1,
        MissionId::from_known(MissionType::Capture),
        sim.session.binary_frame,
    )
    .unwrap();

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener.occupier = true;
    listener.navigation.suspended_nav_com = Some(NavTargetRef::Building { id: 2 });
    listener.navigation.nav_com_aux = Some(NavTargetRef::Building { id: 2 });
    listener.navigation.nav_com = Some(NavTargetRef::Building { id: 2 });
    listener.navigation.nav_queue = vec![NavTargetRef::Building { id: 2 }];

    sim.uninit(2);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert!(listener.navigation.suspended_nav_com.is_none());
    assert_eq!(
        listener.navigation.nav_com_aux,
        Some(NavTargetRef::Building { id: 2 })
    );
    assert_eq!(
        listener.navigation.nav_com,
        Some(NavTargetRef::Building { id: 2 })
    );
    assert!(listener.navigation.nav_queue.is_empty());
}

#[test]
fn selling_target_disables_occupier_capture_nav_retention() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Structure);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));
    sim.mission_assign_exact(
        1,
        MissionId::from_known(MissionType::Capture),
        sim.session.binary_frame,
    )
    .unwrap();
    sim.mission_assign_exact(
        2,
        MissionId::from_known(MissionType::Selling),
        sim.session.binary_frame,
    )
    .unwrap();

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener.occupier = true;
    listener.navigation.nav_com_aux = Some(NavTargetRef::Building { id: 2 });
    listener.navigation.nav_com = Some(NavTargetRef::Building { id: 2 });

    sim.uninit(2);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert!(listener.navigation.nav_com_aux.is_none());
    assert!(listener.navigation.nav_com.is_none());
}

#[test]
fn target_expiry_rearms_passive_scan_and_restores_suspended_mission() {
    let mut sim = Simulation::with_seed(0x1234);
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    insert_entity(&mut sim, 3, EntityCategory::Unit);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));
    sim.session.binary_frame = 105;

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener.attack_target = Some(AttackTarget::new(2));
    listener.suspended_attack_target = Some(TargetKind::Entity(3));
    listener.navigation.suspended_nav_com = Some(NavTargetRef::Cell { rx: 7, ry: 8 });
    listener.passive_scan_timer.arm(100, 20);
    listener.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Attack),
        suspended: MissionId::from_known(MissionType::Guard),
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 100,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(100),
    });

    let mut expected_rng = sim.scenario_rng.clone();
    let delay = expected_rng.next_range_u32_inclusive(4, 8);
    sim.uninit(2);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert_eq!(listener.passive_scan_timer.start_frame, 105);
    assert_eq!(listener.passive_scan_timer.duration, delay);
    assert_eq!(
        listener.mission.current(),
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(listener.mission.suspended(), MissionId::NONE);
    assert_eq!(
        listener.attack_target.as_ref().map(|target| target.target),
        Some(TargetKind::Entity(3))
    );
    assert_eq!(
        listener.navigation.nav_com,
        Some(NavTargetRef::Cell { rx: 7, ry: 8 })
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
}

#[test]
fn infantry_target_expiry_clears_firing_action_before_target() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener.attack_target = Some(AttackTarget {
        target: TargetKind::Entity(2),
        pending_infantry_fire: Some(PendingInfantryFire {
            sequence: SequenceKind::Attack,
            fire_frame: 4,
        }),
    });
    listener.animation = Some(Animation::new(SequenceKind::Attack));
    listener.mission_leaf = crate::sim::mission::MissionLeafState::infantry_raw_for_test(7, 12);

    sim.uninit(2);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert!(listener.attack_target.is_none());
    assert_eq!(
        listener.animation.as_ref().unwrap().sequence,
        SequenceKind::Stand
    );
    let leaf = listener.mission_leaf.as_infantry().unwrap();
    assert_eq!(leaf.firing_sequence_latch(), 0);
    assert_eq!(leaf.doing(), -1);
}

#[test]
fn pointer_expiry_clears_particle_owner_and_deletes_attached_system() {
    let mut sim = Simulation::new();
    insert_particle_system(&mut sim, 1);
    insert_particle_system(&mut sim, 2);
    insert_entity(&mut sim, 3, EntityCategory::Unit);
    let _ = sim.reveal(3);
    sim.substrate
        .particle_systems
        .get_mut(1)
        .unwrap()
        .owner_entity = Some(3);
    let attached = sim.substrate.particle_systems.get_mut(2).unwrap();
    attached.owner_entity = Some(3);
    attached.attached_entity = Some(3);

    sim.uninit(3);

    let owned = sim.substrate.particle_systems.get(1).unwrap();
    assert!(owned.owner_entity.is_none());
    assert!(!owned.done_spawning);
    let attached = sim.substrate.particle_systems.get(2).unwrap();
    assert!(attached.owner_entity.is_none());
    assert!(attached.attached_entity.is_none());
    assert!(attached.done_spawning);
}

#[test]
fn homing_target_expiry_uses_ground_cell_but_nulls_high_flying_target() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    insert_entity(&mut sim, 3, EntityCategory::Unit);
    insert_entity(&mut sim, 4, EntityCategory::Aircraft);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));
    let _ = sim.try_reveal_entity(4, request(13, 15, PlacementEvidence::MarkSucceeded));
    let mut high_locomotor = LocomotorState::for_test_kind(LocomotorKind::Fly);
    high_locomotor.altitude = SimFixed::from_num(208);
    sim.substrate.entities.get_mut(4).unwrap().locomotor = Some(high_locomotor);
    assert!(attach_homing_state(
        &mut sim.substrate.entities,
        1,
        (2, 3),
        2,
        (9, 11),
        SimFixed::from_num(10),
        5,
        0,
        false,
        false,
        SimFixed::from_num(1),
    ));
    assert!(attach_homing_state(
        &mut sim.substrate.entities,
        3,
        (2, 3),
        4,
        (13, 15),
        SimFixed::from_num(10),
        5,
        0,
        false,
        false,
        SimFixed::from_num(1),
    ));

    sim.uninit(2);
    sim.uninit(4);

    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .homing_state
            .as_ref()
            .unwrap()
            .target,
        Some(HomingTarget::Cell { rx: 9, ry: 11 })
    );
    assert!(
        sim.substrate
            .entities
            .get(3)
            .unwrap()
            .homing_state
            .as_ref()
            .unwrap()
            .target
            .is_none()
    );
}

#[test]
fn gsi_05_11_homing_building_target_expiry_uses_foundation_center_cell() {
    // `BulletClass::PointerExpired @ 0x004684E0` derives its replacement cell
    // from the expiring object's `ObjectClass::GetCoords` (vtable +0x48), so an
    // entity-hosted missile must land on the same foundation-centre cell the
    // store-hosted arm already uses (see
    // `gsi_05_04_building_get_coords_uses_foundation_center_cell`).
    let mut sim = Simulation::new();
    sim.session.map_width = 20;
    sim.session.map_height = 20;
    install_common_raw_terrain(&mut sim, 20, 20, 0, None);

    let target_id = sim.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Structure);
    let gapowr = sim.interner.intern("GAPOWR");
    {
        let target = sim.substrate.entities.get_mut(target_id).unwrap();
        target.type_ref = gapowr;
        target.foundation = "2x2".to_string();
    }
    assert!(matches!(
        sim.try_reveal_entity(target_id, common_raw_request(9, 11, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let missile_id = sim.allocate_stable_id();
    insert_entity(&mut sim, missile_id, EntityCategory::Unit);
    assert!(attach_homing_state(
        &mut sim.substrate.entities,
        missile_id,
        (2, 3),
        target_id,
        (9, 11),
        SimFixed::from_num(10),
        5,
        0,
        false,
        false,
        SimFixed::from_num(1),
    ));

    sim.uninit(target_id);

    let homing = sim
        .substrate
        .entities
        .get(missile_id)
        .unwrap()
        .homing_state
        .clone()
        .unwrap();
    assert_eq!(
        homing.target,
        Some(HomingTarget::Cell { rx: 10, ry: 12 }),
        "the entity-hosted arm takes the GetCoords foundation centre, not the stored NW anchor"
    );
    assert_eq!(
        (homing.last_known_rx, homing.last_known_ry),
        (10, 12),
        "the cached last-known cell follows the same derivation"
    );
}

#[test]
fn expiring_mixed_size_passenger_updates_transport_total_exactly() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    insert_entity(&mut sim, 3, EntityCategory::Infantry);
    let mut cargo = PassengerCargo::new(5, 0);
    cargo.board_forced(2, 3);
    cargo.board_forced(3, 1);
    sim.substrate.entities.get_mut(1).unwrap().passenger_role = PassengerRole::Transport { cargo };
    sim.substrate.entities.get_mut(2).unwrap().passenger_role =
        PassengerRole::Inside { transport_id: 1 };
    sim.substrate.entities.get_mut(3).unwrap().passenger_role =
        PassengerRole::Inside { transport_id: 1 };

    sim.uninit(2);

    let cargo = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .passenger_role
        .cargo()
        .unwrap();
    assert_eq!(cargo.passengers, vec![3]);
    assert_eq!(cargo.passenger_sizes, vec![1]);
    assert_eq!(cargo.total_size, 1);
}

#[test]
fn gsi_05_03_duplicate_uninit_repeats_direct_expiry_and_queue_but_finalizes_once() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    // As its construction would: Add_Tracking.
    sim.update_house_tracking(1, crate::sim::house_tracking::HouseTracking::add_tracking);
    let _ = sim.reveal(1);

    sim.lifecycle_test_events.clear();
    sim.uninit(1);
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::UninitRemovalNotifyBoundary { stable_id: 1, .. }
            ))
            .count(),
        1
    );
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::ConcealDestroyNotifyBoundary { stable_id: 1, .. }
            ))
            .count(),
        1
    );
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 1, .. }
            ))
            .count(),
        2,
        "the first active UnInit performs its direct dispatch and Conceal's Destroy dispatch"
    );

    let repeat_start = sim.lifecycle_test_events.len();
    sim.uninit(1);
    let repeated_events = &sim.lifecycle_test_events[repeat_start..];
    // Removed_From_Game ran once (the first Limbo); the tracking waits for
    // the destructor at the drain.
    let tracking = &sim.houses.get(&owner).unwrap().tracking;
    assert_eq!(tracking.active_for_test().0, 0);
    assert_eq!(tracking.units_for_test(), 1);
    assert!(sim.substrate.entities.get(1).unwrap().destruction_recorded);
    assert_eq!(sim.substrate.pending_delete, vec![1, 1]);
    let direct = repeated_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::UninitRemovalNotifyBoundary {
                    stable_id: 1,
                    object_alive: false,
                    cell_marked: false,
                    resolvable: true,
                }
        })
        .expect("repeat UnInit direct expiry boundary");
    let direct_listener = repeated_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::UninitRemovalListenerVisited {
                    expired_id: 1,
                    listener_id: 1,
                    target_alive: false,
                    target_in_limbo: true,
                }
        })
        .expect("repeat UnInit direct expiry dispatch");
    let conceal_return = repeated_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::ConcealAlreadyLimboReturn {
                    stable_id: 1,
                    object_alive: false,
                    resolvable: true,
                }
        })
        .expect("repeat Limbo reached Conceal's InLimbo return");
    assert!(direct < direct_listener && direct_listener < conceal_return);
    assert_eq!(
        repeated_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 1, .. }
            ))
            .count(),
        1,
        "the repeated UnInit adds exactly one direct expiry dispatch"
    );
    assert!(
        !repeated_events.iter().any(|event| matches!(
            event,
            LifecycleTestEvent::ConcealDestroyNotifyBoundary { stable_id: 1, .. }
        )),
        "Conceal's InLimbo return occurs before Destroy"
    );

    sim.lifecycle_test_events.clear();
    sim.process_pending_delete();
    assert!(!sim.substrate.entities.contains(1));
    assert!(sim.substrate.pending_delete.is_empty());
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::PendingDeleteDrainStarted,
            LifecycleTestEvent::FinalizedCommon { stable_id: 1 },
        ]
    );
}

/// UnInit records the destruction once; the tracking leaves with the
/// destructor at the drain, once however often the object was queued.
#[test]
fn lifecycle_authority_uninit_records_once_and_the_drain_removes_the_tracking() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.update_house_tracking(1, crate::sim::house_tracking::HouseTracking::add_tracking);

    sim.uninit(1);
    sim.uninit(1);
    assert_eq!(sim.houses.get(&owner).unwrap().tracking.units_for_test(), 1);
    assert!(sim.substrate.entities.get(1).unwrap().destruction_recorded);
    sim.flush_pending_delete();
    assert_eq!(sim.houses.get(&owner).unwrap().tracking.units_for_test(), 0);
}

#[test]
fn gsi_04_05_reservation_successful_reveal_marks_expanded_rect_after_lists() {
    let mut sim = Simulation::new();
    let owner = insert_reservation_building(&mut sim, 101, "Americans", 10, 20, "2x3", 1);
    sim.session.house_order.push(owner);

    assert!(matches!(
        sim.try_reveal_entity(101, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    for x in 9..=12 {
        for y in 19..=23 {
            assert_eq!(sim.substrate.base_reservations.raw_mask(None, x, y), 1);
        }
    }
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 8, 20), 0);
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 13, 20), 0);
    let house = sim.houses.get(&owner).expect("reservation owner");
    assert_eq!(house.base_reservation.bounds(), (9, 19, 4, 5));
    assert_eq!(
        house.base_reservation.perimeter_cells(),
        &[
            packed_reservation_test_coord(9, 19),
            packed_reservation_test_coord(9, 20),
            packed_reservation_test_coord(9, 21),
            packed_reservation_test_coord(9, 22),
            packed_reservation_test_coord(9, 23),
            packed_reservation_test_coord(10, 19),
            packed_reservation_test_coord(10, 23),
            packed_reservation_test_coord(11, 19),
            packed_reservation_test_coord(11, 23),
            packed_reservation_test_coord(12, 19),
            packed_reservation_test_coord(12, 20),
            packed_reservation_test_coord(12, 21),
            packed_reservation_test_coord(12, 22),
            packed_reservation_test_coord(12, 23),
        ]
    );

    let cell_marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::CellMarked)
        .unwrap();
    let reservation_marked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::BaseReservationMarked)
        .unwrap();
    let display = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RevealDisplayBoundary)
        .unwrap();
    assert!(cell_marked < reservation_marked && reservation_marked < display);
}

#[test]
fn gsi_04_16_dustbowl_conyard_unlimbo_uses_local_size_edge() {
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(gsi_04_16_dustbowl_bounds());
    let rules = gsi_04_16_edge_gate_rules();
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, true, 0, 10);
    house.base_center = Some((70, 116));
    sim.houses.insert(owner, house);

    let conyard = sim
        .spawn_object("GACNST", "Americans", 69, 115, 0, &rules, &BTreeMap::new())
        .expect("GACNST reveals");
    assert!(
        sim.substrate
            .entities
            .get(conyard)
            .unwrap()
            .determines_waypoint_edge,
        "Factory=BuildingType must freeze onto the entity before Reveal"
    );
    let house = sim.houses.get(&owner).expect("launch house");
    assert_eq!(house.base_center, Some((70, 116)));
    assert_eq!(house.waypoint_edge, 2);
}

#[test]
fn gsi_04_16_committed_structure_owner_change_refreshes_new_house_edge() {
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(gsi_04_16_dustbowl_bounds());
    let rules = gsi_04_16_edge_gate_rules();
    let old_owner = sim.interner.intern("Americans");
    let new_owner = sim.interner.intern("Soviets");
    sim.houses
        .insert(old_owner, HouseState::new(old_owner, 0, None, true, 0, 10));
    let mut new_house = HouseState::new(new_owner, 1, None, false, 0, 10);
    new_house.base_center = Some((68, 114));
    sim.houses.insert(new_owner, new_house);
    let conyard = sim
        .spawn_object("GACNST", "Americans", 69, 115, 0, &rules, &BTreeMap::new())
        .expect("GACNST reveals");

    sim.change_owner(conyard, new_owner);

    let new_house = sim.houses.get(&new_owner).expect("new owner house");
    assert_eq!(new_house.base_center, Some((68, 114)));
    assert_eq!(new_house.waypoint_edge, 2);
}

#[test]
fn gsi_04_16_caoild_reveal_and_owner_change_preserve_waypoint_edges() {
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(gsi_04_16_dustbowl_bounds());
    let rules = gsi_04_16_edge_gate_rules();
    let old_owner = sim.interner.intern("Americans");
    let new_owner = sim.interner.intern("Soviets");
    let mut old_house = HouseState::new(old_owner, 0, None, true, 0, 10);
    old_house.waypoint_edge = 1;
    sim.houses.insert(old_owner, old_house);
    let mut new_house = HouseState::new(new_owner, 1, None, false, 0, 10);
    new_house.waypoint_edge = 3;
    sim.houses.insert(new_owner, new_house);

    let oil = sim
        .spawn_object("CAOILD", "Americans", 69, 115, 0, &rules, &BTreeMap::new())
        .expect("CAOILD reveals");
    assert!(
        !sim.substrate
            .entities
            .get(oil)
            .unwrap()
            .determines_waypoint_edge,
        "missing Factory= must freeze as an ineligible edge profile"
    );
    assert_eq!(sim.houses.get(&old_owner).unwrap().waypoint_edge, 1);

    sim.change_owner(oil, new_owner);

    assert_eq!(sim.houses.get(&new_owner).unwrap().waypoint_edge, 3);
}

#[test]
fn gsi_04_05_reservation_failed_reveal_never_writes() {
    let mut sim = Simulation::new();
    let owner = insert_reservation_building(&mut sim, 102, "Americans", 10, 20, "2x2", 1);
    sim.session.house_order.push(owner);

    assert_eq!(
        sim.try_reveal_entity(102, request(10, 20, PlacementEvidence::MarkFailed)),
        RevealOutcome::Failed(RevealFailure::MarkFailed)
    );
    assert_eq!(sim.substrate.base_reservations.entries().count(), 0);
    assert_eq!(sim.substrate.base_reservations.dummy_mask(), 0);
    assert!(
        !sim.lifecycle_test_events
            .contains(&LifecycleTestEvent::BaseReservationMarked)
    );
}

#[test]
fn gsi_04_05_reservation_clear_removes_perimeter_but_retains_bounds() {
    let mut sim = Simulation::new();
    let owner = insert_reservation_building(&mut sim, 111, "Americans", 10, 10, "1x1", 1);
    sim.session.house_order.push(owner);
    let _ = sim.try_reveal_entity(111, request(10, 10, PlacementEvidence::MarkSucceeded));
    assert_eq!(
        sim.houses
            .get(&owner)
            .unwrap()
            .base_reservation
            .perimeter_cells()
            .len(),
        8
    );

    assert_eq!(sim.techno_limbo(111), super::ConcealOutcome::Concealed);
    let state = &sim.houses.get(&owner).unwrap().base_reservation;
    assert_eq!(state.bounds(), (9, 9, 3, 3));
    assert!(state.perimeter_cells().is_empty());
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 10, 10), 0);
}

#[test]
fn gsi_04_05_reservation_limbo_clears_before_unlink_repairs_overlap_and_preserves_other_house() {
    let mut sim = Simulation::new();
    let owner = insert_reservation_building(&mut sim, 103, "Americans", 10, 10, "1x1", 1);
    let other = sim.interner.intern("Russians");
    sim.session.house_order.extend([owner, other]);
    insert_reservation_building(&mut sim, 104, "Americans", 12, 10, "1x1", 1);
    let _ = sim.try_reveal_entity(103, request(10, 10, PlacementEvidence::MarkSucceeded));
    let _ = sim.try_reveal_entity(104, request(12, 10, PlacementEvidence::MarkSucceeded));
    sim.substrate.base_reservations.reserve(None, 9, 10, 1);

    sim.lifecycle_test_events.clear();
    assert_eq!(sim.techno_limbo(103), super::ConcealOutcome::Concealed);

    assert_eq!(
        sim.substrate.base_reservations.raw_mask(None, 9, 10),
        1 << 1,
        "AND-not clears only the leaving owner's bit"
    );
    assert_eq!(
        sim.substrate.base_reservations.raw_mask(None, 11, 10) & 1,
        1,
        "neighbor repair restores the same-house overlap"
    );
    let cleared = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::BaseReservationCleared)
        .unwrap();
    let unlinked = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .unwrap();
    assert!(cleared < unlinked);
}

#[test]
fn gsi_04_05_reservation_repair_scan_reaches_asymmetric_high_edge() {
    assert_eq!(
        super::lifecycle::building_base_reservation_repair_rect(10, 20, 4, 4, 1),
        crate::sim::cell_rect::CellRect::new(8, 18, 9, 9),
        "half-open repair bounds are [8,17) x [18,27)"
    );
    assert_eq!(
        super::lifecycle::building_base_reservation_repair_rect(10, 20, 6, 6, -1),
        crate::sim::cell_rect::CellRect::new(12, 22, 1, 1),
        "signed negative spacing yields [12,13) x [22,23)"
    );

    let mut sim = Simulation::new();
    let owner = insert_reservation_building(&mut sim, 107, "Americans", 10, 20, "4x4", 1);
    sim.session.house_order.push(owner);
    insert_reservation_building(&mut sim, 108, "Americans", 16, 20, "1x1", 2);
    let _ = sim.try_reveal_entity(107, request(10, 20, PlacementEvidence::MarkSucceeded));
    let _ = sim.try_reveal_entity(108, request(16, 20, PlacementEvidence::MarkSucceeded));
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 14, 20), 1);

    assert_eq!(sim.techno_limbo(107), super::ConcealOutcome::Concealed);
    assert_eq!(
        sim.substrate.base_reservations.raw_mask(None, 14, 20),
        1,
        "the x=16 neighbor, omitted by the old [8,16) scan, re-marks its overlap"
    );
}

#[test]
fn gsi_04_05_reservation_origins_truncate_after_first_spacing_subtraction() {
    assert_eq!(
        super::lifecycle::building_base_reservation_rect(0, 0, "1x1", 32_769),
        crate::sim::cell_rect::CellRect::new(32_767, 32_767, 65_539, 65_539)
    );
    assert_eq!(
        super::lifecycle::building_base_reservation_repair_rect(0, 0, 1, 1, 32_769),
        crate::sim::cell_rect::CellRect::new(-2, -2, 163_846, 163_846),
        "repair subtracts spacing from the already signed-16-truncated primary start"
    );
}

#[test]
fn gsi_04_05_reservation_repair_replays_multicell_neighbor_for_every_hit() {
    let mut sim = Simulation::new();
    let owner = insert_reservation_building(&mut sim, 109, "Americans", 10, 10, "1x1", 1);
    sim.session.house_order.push(owner);
    insert_reservation_building(&mut sim, 110, "Americans", 12, 10, "2x2", 1);
    let _ = sim.try_reveal_entity(109, request(10, 10, PlacementEvidence::MarkSucceeded));
    let _ = sim.try_reveal_entity(110, request(12, 10, PlacementEvidence::MarkSucceeded));

    sim.lifecycle_test_events.clear();
    assert_eq!(sim.techno_limbo(109), super::ConcealOutcome::Concealed);
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| **event == LifecycleTestEvent::BaseReservationMarked)
            .count(),
        4,
        "the 2x2 neighbor is invoked immediately once for each occupied repair cell"
    );
}

#[test]
fn gsi_04_05_reservation_art_foundation_recomputes_writer_before_lifecycle_mark() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[AI]\nAIBaseSpacing=2\n\
         [BuildingTypes]\n0=GACNST\n1=ONECNST\n\
         [GACNST]\nUndeploysInto=AMCV\n\
         [ONECNST]\nUndeploysInto=AMCV\n",
    ))
    .expect("split rules-side construction-yard data");
    assert_eq!(
        rules.object("GACNST").unwrap().base_reservation_spacing,
        None,
        "the provisional Rules-only 1x1 foundation is ineligible"
    );

    rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(
        "[GACNST]\nFoundation=4x4\n\
         [ONECNST]\nFoundation=1x1\n",
    )));
    let gacnst = rules.object("GACNST").unwrap();
    assert_eq!(gacnst.foundation, "4x4");
    assert_eq!(gacnst.base_reservation_spacing, Some(2));
    assert_eq!(
        rules.object("ONECNST").unwrap().base_reservation_spacing,
        None,
        "an effective ART 1x1 undeployer remains ineligible"
    );
    let foundation = gacnst.foundation.clone();
    let spacing = gacnst.base_reservation_spacing.unwrap();

    let mut sim = Simulation::new();
    let owner =
        insert_reservation_building(&mut sim, 106, "Americans", 30, 40, &foundation, spacing);
    sim.session.house_order.push(owner);
    assert!(matches!(
        sim.try_reveal_entity(106, request(30, 40, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 28, 38), 1);
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 35, 45), 1);
    assert_eq!(sim.substrate.base_reservations.raw_mask(None, 36, 45), 0);
}

#[test]
fn score_stats_credit_the_killer_and_charge_the_victim_once() {
    let mut sim = Simulation::new();
    let victim_owner = sim.interner.intern("Americans");
    let killer_owner = sim.interner.intern("Russians");
    sim.houses.insert(
        victim_owner,
        HouseState::new(victim_owner, 0, None, true, 0, 10),
    );
    sim.houses.insert(
        killer_owner,
        HouseState::new(killer_owner, 1, None, false, 0, 10),
    );
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let victim = sim.substrate.entities.get_mut(1).unwrap();
    victim.health.current = 0;
    victim.killed_by = Some(killer_owner);
    // A stock Rhino (HTNK) is Cost=900, and the award is the victim's cost.
    victim.kill_award_points = 900;

    // A repeated uninit must not double-count: the destruction is recorded
    // exactly once.
    sim.uninit(1);
    sim.uninit(1);

    assert_eq!(sim.houses.get(&victim_owner).unwrap().stats.losses(), 1);
    assert_eq!(sim.houses.get(&victim_owner).unwrap().stats.kills(), 0);
    let killer = sim.houses.get(&killer_owner).unwrap();
    assert_eq!(killer.stats.kills(), 1);
    assert_eq!(
        killer.stats.units_killed, 1,
        "a unit victim must land in the unit bucket, not the building one"
    );
    assert_eq!(killer.stats.score_points, 900);
}

#[test]
fn score_stats_come_from_the_kill_record_captured_at_destruction() {
    // Dying infantry linger in the logic vector before they are removed. The
    // kill record is captured at the instant of destruction.
    let mut sim = Simulation::new();
    let victim_owner = sim.interner.intern("Americans");
    let killer_owner = sim.interner.intern("Russians");
    sim.houses.insert(
        victim_owner,
        HouseState::new(victim_owner, 0, None, true, 0, 10),
    );
    sim.houses.insert(
        killer_owner,
        HouseState::new(killer_owner, 1, None, false, 0, 10),
    );
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    let victim = sim.substrate.entities.get_mut(1).unwrap();
    victim.health.current = 0;
    victim.killed_by = Some(killer_owner);
    // A stock GI (E1) is Cost=200.
    victim.kill_award_points = 200;

    sim.uninit(1);

    assert_eq!(sim.houses.get(&killer_owner).unwrap().stats.kills(), 1);
    assert_eq!(
        sim.houses.get(&killer_owner).unwrap().stats.score_points,
        200
    );
}

#[test]
fn score_stats_ignore_a_removal_that_was_not_a_destruction() {
    // Selling or otherwise despawning a healthy object is not a loss.
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Structure);
    sim.substrate.entities.get_mut(1).unwrap().health.current = 400;

    sim.uninit(1);

    assert_eq!(sim.houses.get(&owner).unwrap().stats.losses(), 0);
}

#[test]
fn score_stats_count_a_self_inflicted_kill_but_award_no_points() {
    // Self-inflicted destruction (own death weapon, own splash) is both a loss
    // and a kill for the house â€” native increments the kill table regardless of
    // relation and suppresses only the points.
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Structure);
    let victim = sim.substrate.entities.get_mut(1).unwrap();
    victim.health.current = 0;
    victim.killed_by = Some(owner);
    victim.kill_award_points = 800;

    sim.uninit(1);

    let house = sim.houses.get(&owner).unwrap();
    assert_eq!(house.stats.buildings_lost, 1);
    assert_eq!(house.stats.buildings_killed, 1);
    assert_eq!(
        house.stats.score_points, 0,
        "an allied or self-inflicted victim is worth no score"
    );
}

#[test]
fn dont_score_victims_book_no_kill_no_loss_and_no_points() {
    // Stock `DontScore=yes` covers SLAV and the three spawner missiles
    // (V3ROCKET/DMISL/CMISL). Slaves die and respawn all match and AA intercepts
    // are routine, so a victim that native ignores entirely must not show up in
    // any of the three columns.
    let mut sim = Simulation::new();
    let victim_owner = sim.interner.intern("Americans");
    let killer_owner = sim.interner.intern("Russians");
    sim.houses.insert(
        victim_owner,
        HouseState::new(victim_owner, 0, None, true, 0, 10),
    );
    sim.houses.insert(
        killer_owner,
        HouseState::new(killer_owner, 1, None, false, 0, 10),
    );
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    let victim = sim.substrate.entities.get_mut(1).unwrap();
    victim.health.current = 0;
    victim.dont_score = true;
    // Even with a credit already recorded, the loss half stays suppressed.
    victim.killed_by = Some(killer_owner);
    victim.kill_award_points = 500;

    sim.uninit(1);

    let victim_house = sim.houses.get(&victim_owner).unwrap();
    assert_eq!(victim_house.stats.losses(), 0, "no phantom loss");
    let killer = sim.houses.get(&killer_owner).unwrap();
    assert_eq!(killer.stats.kills(), 0, "no phantom kill");
    assert_eq!(killer.stats.score_points, 0, "no phantom points");
}

#[test]
fn score_column_sums_the_harvest_and_kill_feeders() {
    // The native score field has two feeders. Drive the kill half through the
    // real award helper on stock costs rather than a hand-picked total: one
    // Rhino (Cost=900) plus one veteran GI (Cost=200, doubled).
    use crate::rules::ini_parser::IniFile;
    use crate::rules::object_type::{ObjectCategory, ObjectType};
    use crate::sim::combat::score_award_for_victim;
    use crate::sim::house_state::MatchStatistics;

    let of = |body: &str, category| {
        let ini = IniFile::from_str(&format!(
            "[T]
{body}"
        ));
        ObjectType::from_ini_section("T", ini.section("T").expect("section"), category)
    };
    let rhino = of(
        "Cost=900
",
        ObjectCategory::Vehicle,
    );
    let gi = of(
        "Cost=200
",
        ObjectCategory::Infantry,
    );

    let kill_half =
        score_award_for_victim(Some(&rhino), 0) + score_award_for_victim(Some(&gi), 100);
    assert_eq!(kill_half, 1_300);

    let stats = MatchStatistics {
        score_points: kill_half,
        ..Default::default()
    };
    // Harvest half: 240 bales deposited at the x5.0 statistics rate.
    assert_eq!(stats.score(1_200), 2_500);
}

#[test]
fn lifecycle_authority_animated_death_stays_represented_until_uninit() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.update_house_tracking(1, crate::sim::house_tracking::HouseTracking::add_tracking);
    let _ = sim.reveal(1);

    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.health.current = 0;
    entity.dying = true;
    let tracking = &sim.houses.get(&owner).unwrap().tracking;
    assert_eq!(
        (tracking.units_for_test(), tracking.active_for_test().0),
        (1, 1)
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.destruction_recorded);
    assert!(entity.lifecycle.object_alive);
    assert!(!entity.lifecycle.in_limbo);
    assert!(entity.lifecycle.cell_marked);
    assert!(entity.in_logic_vector);

    sim.uninit(1);
    let tracking = &sim.houses.get(&owner).unwrap().tracking;
    assert_eq!(
        (tracking.units_for_test(), tracking.active_for_test().0),
        (1, 0)
    );
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .object_alive
    );
    assert!(!sim.substrate.entities.get(1).unwrap().in_logic_vector);
}

#[test]
fn lifecycle_authority_duplicate_queue_finalizes_once() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.uninit(1);
    sim.uninit(1);
    assert_eq!(sim.substrate.pending_delete, vec![1, 1]);
    sim.lifecycle_test_events.clear();

    sim.process_pending_delete();
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::PendingDeleteDrainStarted,
            LifecycleTestEvent::FinalizedCommon { stable_id: 1 },
        ]
    );
    assert!(!sim.substrate.entities.contains(1));
    assert!(sim.substrate.pending_delete.is_empty());
}

#[test]
fn lifecycle_authority_entity_and_anim_share_ordered_drain() {
    let mut sim = Simulation::new();
    insert_anim(&mut sim, 2, true);
    sim.substrate.pending_delete.push(2);
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.uninit(1);
    assert_eq!(sim.substrate.pending_delete, vec![2, 1]);
    sim.lifecycle_test_events.clear();

    sim.process_pending_delete();
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::PendingDeleteDrainStarted,
            LifecycleTestEvent::FinalizedCommon { stable_id: 2 },
            LifecycleTestEvent::FinalizedCommon { stable_id: 1 },
        ]
    );
    assert!(!sim.substrate.anims.contains_key(2));
    assert!(!sim.substrate.entities.contains(1));
    assert!(sim.substrate.pending_delete.is_empty());
}

#[test]
fn lifecycle_authority_late_tail_commits_frame_before_drain() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.uninit(1);
    sim.lifecycle_test_events.clear();

    let height_map = BTreeMap::new();
    let _ = sim.advance_tick(&[], None, &height_map, None, None, 67);
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::BinaryFrameCommitted,
            LifecycleTestEvent::PendingDeleteDrainStarted,
            LifecycleTestEvent::FinalizedCommon { stable_id: 1 },
        ]
    );
    assert_eq!(sim.session.binary_frame, 1);
    assert!(!sim.substrate.entities.contains(1));
}

#[test]
fn lifecycle_authority_set_logic_order_for_test_synchronizes_all_membership_flags() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    insert_anim(&mut sim, 3, false);
    insert_particle_system(&mut sim, 4);

    sim.set_logic_order_for_test(vec![3, 4, 1]);
    assert_eq!(sim.live_object_order_snapshot(), vec![3, 4, 1]);
    assert!(sim.substrate.anims.get(3).unwrap().in_logic_vector);
    assert!(
        sim.substrate
            .particle_systems
            .get(4)
            .unwrap()
            .in_logic_vector
    );
    assert!(sim.substrate.entities.get(1).unwrap().in_logic_vector);
    assert!(!sim.substrate.entities.get(2).unwrap().in_logic_vector);
    sim.debug_assert_logic_membership_consistent();

    sim.set_logic_order_for_test(vec![2]);
    assert_eq!(sim.live_object_order_snapshot(), vec![2]);
    assert!(!sim.substrate.anims.get(3).unwrap().in_logic_vector);
    assert!(
        !sim.substrate
            .particle_systems
            .get(4)
            .unwrap()
            .in_logic_vector
    );
    assert!(!sim.substrate.entities.get(1).unwrap().in_logic_vector);
    assert!(sim.substrate.entities.get(2).unwrap().in_logic_vector);
    sim.debug_assert_logic_membership_consistent();
}

pub(super) fn gsi_05_02_projectile(source_id: u64, fuse_frames: Option<u16>) -> ProjectileSpawn {
    ProjectileSpawn {
        flat: false,
        source_id,
        origin: ProjectileCoord::new(0, 0, 0),
        target: ProjectileTarget::Cell { rx: 16, ry: 0 },
        initial_target_position: ProjectileCoord::new(4096, 0, 0),
        payload: ProjectilePayload {
            base_damage: 1,
            warhead: crate::sim::intern::InternedId::from_index(0),
            weapon: crate::sim::intern::InternedId::from_index(0),
        },
        speed_leptons_per_frame: 64,
        velocity: ProjectileVelocity::new(64, 0, 0),
        trajectory: ProjectileTrajectory::Straight,
        guidance: None,
        visual: ProjectileVisualState::new(0, 0, 0),
        arm_frames: 0,
        fuse_frames,
        ranged_fuse: false,
        tracks_target: false,
        target_expiry: TargetExpiryPolicy::Expire,
        collision: ProjectileCollisionPolicy::NONE,
    }
}

#[test]
fn persistent_bullet_logic_slot_publishes_native_wall_dirty_visits() {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=WALLWH\n\
         [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
         [WALLWH]\nWall=yes\nCellSpread=0\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [GAWALL]\nWall=yes\nStrength=1\n",
    );
    let art = crate::rules::ini_parser::IniFile::from_str("[GAWALL]\nDamageLevels=4\n");
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("Bullet wall rules");
    let overlays = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));

    let run = |initial_wall_data: u8| {
        let mut sim = Simulation::new();
        let mut grid = crate::sim::overlay_grid::OverlayGrid::new(12, 12);
        grid.place_overlay(5, 5, 2, initial_wall_data);
        let _ = grid.take_dirty_cells();
        sim.overlay_grid = Some(grid);

        let projectile_id = sim.allocate_stable_id();
        let impact = ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0);
        let mut spawn = gsi_05_02_projectile(crate::sim::combat::RAD_NO_ATTACKER, Some(0));
        spawn.origin = impact;
        spawn.target = ProjectileTarget::Cell { rx: 5, ry: 5 };
        spawn.initial_target_position = impact;
        spawn.payload = ProjectilePayload {
            base_damage: 1,
            warhead: sim.interner.intern("WALLWH"),
            weapon: sim.interner.intern("MISSINGWEAPON"),
        };
        sim.admit_projectile(projectile_id, spawn);

        assert!(sim.object_ai_visit_one(
            projectile_id,
            Some(&rules),
            ObjectAiCtx {
                overlay_registry: Some(&overlays),
                ..ObjectAiCtx::default()
            },
        ));
        assert!(!sim.projectiles.get(projectile_id).unwrap().in_logic_vector);
        sim
    };

    let partial = run(0);
    assert_eq!(
        partial
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .overlay_data,
        0x10,
    );
    assert!(partial.radar_terrain_dirty_cells.is_empty());
    assert_eq!(partial.radar_terrain_dirty_generation, 0);
    assert_eq!(partial.tactical_dirty_cells, vec![(5, 5)]);

    let terminal = run(0x30);
    assert_eq!(
        terminal
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .overlay_id,
        None
    );
    assert_eq!(
        terminal.radar_terrain_dirty_cells,
        vec![
            (5, 5),
            (5, 3),
            (6, 4),
            (4, 4),
            (5, 4),
            (4, 6),
            (3, 5),
            (4, 5),
            (6, 6),
            (5, 7),
            (5, 6),
            (7, 5),
            (6, 5),
        ],
    );
    assert_eq!(terminal.radar_terrain_dirty_generation, 13);
    assert_eq!(
        terminal.tactical_dirty_cells,
        vec![
            (5, 5),
            (5, 3),
            (6, 4),
            (5, 5),
            (4, 4),
            (5, 4),
            (4, 4),
            (5, 5),
            (4, 6),
            (3, 5),
            (4, 5),
            (5, 5),
            (6, 6),
            (5, 7),
            (4, 6),
            (5, 6),
            (6, 4),
            (7, 5),
            (6, 6),
            (5, 5),
            (6, 5),
        ],
        "tactical dirties retain every native call, including repeated cleanup cells"
    );
}

fn gsi_05_04_guided_projectile(
    source_id: u64,
    target: ProjectileTarget,
    initial_target_position: ProjectileCoord,
) -> ProjectileSpawn {
    let mut spawn = gsi_05_02_projectile(source_id, None);
    // These fixtures test pointer lifetime/steering, above the native ground
    // impact plane (including their level-2 terrain). Impact tests override it.
    spawn.origin.z = 1000;
    spawn.target = target;
    spawn.initial_target_position = initial_target_position;
    spawn.guidance = Some(ProjectileGuidance {
        rot: 60,
        missile_rot_var: SimFixed::lit("0.25"),
        course_lock_duration: 0,
        sidewinder_phase: 0,
        airburst: false,
        inaccurate: false,
        very_high: false,
        level: false,
        heading_bam: 0,
        max_speed: 0,
        acceleration: 3,
        fuse_reference: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
        closing_frames: 0,
        closing_accumulator_bits: 0,
        frames_elapsed: 0,
    });
    spawn.tracks_target = true;
    spawn.target_expiry = TargetExpiryPolicy::DetonateAtLastKnown;
    spawn
}

#[test]
fn homing_ground_impact_reaches_damage_and_cleanup_through_runtime_frame() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::runtime::SimRuntime;

    // Retail AAHeatSeeker2's active ROT/Arm/collision policy, with a one-point
    // wall receiver to expose the final impact cell and ground-layer handoff.
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nJumpJet=yes\nStrength=100\n\
         [Warheads]\n0=WALLWH\n[OverlayTypes]\n0=GAWALL\n\
         [WALLWH]\nWall=yes\nCellSpread=0\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [GAWALL]\nWall=yes\nStrength=1\n",
    );
    let art = IniFile::from_str("[GAWALL]\nDamageLevels=4\n");
    for (old_height, with_source, expire_source) in [
        (0, false, false),
        (1, false, false),
        (0, true, false),
        (0, true, true),
    ] {
        let mut sim = Simulation::new();
        install_common_raw_terrain(&mut sim, 16, 16, 2, None);
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(16, 16);
        overlays.place_overlay(5, 5, 0, 0);
        overlays.place_overlay(12, 5, 0, 0);
        let _ = overlays.take_dirty_cells();
        sim.overlay_grid = Some(overlays);
        let origin = ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 208 + old_height);
        let mut shot = gsi_05_04_guided_projectile(
            crate::sim::combat::RAD_NO_ATTACKER,
            ProjectileTarget::Cell { rx: 12, ry: 5 },
            ProjectileCoord::new(12 * 256 + 128, 5 * 256 + 128, 208),
        );
        shot.origin = origin;
        shot.speed_leptons_per_frame = 4;
        shot.velocity = ProjectileVelocity::new(4, 0, -2);
        shot.arm_frames = 2;
        shot.ranged_fuse = true;
        let guidance = shot.guidance.as_mut().unwrap();
        guidance.max_speed = 4;
        guidance.fuse_reference = shot.initial_target_position;
        let source_id = if with_source {
            let source_id = sim.allocate_stable_id();
            insert_entity(&mut sim, source_id, EntityCategory::Unit);
            let type_ref = sim.interner.intern("MTNK");
            sim.substrate.entities.get_mut(source_id).unwrap().type_ref = type_ref;
            shot.source_id = source_id;
            shot.arm_frames = 0;
            guidance.fuse_reference = ProjectileCoord::new(origin.x + 104, origin.y, origin.z - 2);
            Some(source_id)
        } else {
            None
        };
        shot.payload = ProjectilePayload {
            base_damage: 1,
            warhead: sim.interner.intern("WALLWH"),
            weapon: sim.interner.intern("MISSINGWEAPON"),
        };
        let id = sim.allocate_stable_id();
        sim.admit_projectile(id, shot);
        if with_source {
            sim.projectiles.get_mut(id).unwrap().last_distance_half = 0;
        }
        if expire_source {
            sim.uninit(source_id.unwrap());
            assert_eq!(
                sim.projectiles.get(id).unwrap().source_id,
                crate::sim::combat::RAD_NO_ATTACKER
            );
        }
        let mut runtime = SimRuntime::from_simulation(sim);
        runtime.resources.rules = RuleSet::from_ini(&ini).unwrap();
        runtime.resources.overlay_registry =
            crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));

        let _ = runtime
            .advance_frame(&[], 16, super::TickLane::Ordinary)
            .expect("fixture frame must complete");
        if old_height == 1 {
            let bullet = runtime.simulation.projectiles.get(id).expect(
                "crossing below ground does not trigger the OLD-height predicate until next visit",
            );
            assert!(bullet.in_logic_vector);
            assert_eq!(bullet.position.z, 207);
            assert_eq!(
                runtime
                    .simulation
                    .overlay_grid
                    .as_ref()
                    .unwrap()
                    .cell(5, 5)
                    .overlay_data,
                0
            );
            let _ = runtime
                .advance_frame(&[], 16, super::TickLane::Ordinary)
                .expect("fixture frame must complete");
        }
        assert!(runtime.simulation.projectiles.get(id).is_none());
        let hits_target = with_source && !expire_source;
        let overlays = runtime.simulation.overlay_grid.as_ref().unwrap();
        assert_eq!(
            overlays.cell(5, 5).overlay_data,
            if hits_target { 0 } else { 0x10 }
        );
        assert_eq!(
            overlays.cell(12, 5).overlay_data,
            if hits_target { 0x10 } else { 0 },
            "only a still-live JumpJet firer converts overshoot mode2 to the target-location mode1 snap"
        );
        assert!(
            runtime.simulation.pending_projectile_detonations.is_empty(),
            "production commits in the Bullet slot, with no test-only pending handoff"
        );
    }
}

#[test]
fn homing_mode_one_fuse_snaps_to_cell_location_below_bridge_aim_point() {
    let mut sim = Simulation::new();
    install_common_raw_terrain(&mut sim, 16, 16, 2, Some((5, 5)));
    let aim = cell_target_coord(sim.resolved_terrain.as_ref(), 5, 5);
    assert_eq!(aim.z, 624);
    let mut shot = gsi_05_04_guided_projectile(
        crate::sim::combat::RAD_NO_ATTACKER,
        ProjectileTarget::Cell { rx: 5, ry: 5 },
        aim,
    );
    shot.origin = ProjectileCoord::new(4 * 256 + 128, 5 * 256 + 128, 400);
    shot.speed_leptons_per_frame = 4;
    shot.velocity = ProjectileVelocity::new(4, 0, 0);
    shot.ranged_fuse = true;
    let guidance = shot.guidance.as_mut().unwrap();
    guidance.max_speed = 4;
    guidance.fuse_reference = shot.origin;
    let id = sim.allocate_stable_id();
    sim.admit_projectile(id, shot);
    let result = sim.projectiles.advance(
        0,
        &BTreeMap::new(),
        sim.resolved_terrain.as_ref(),
        &sim.shared_cell_dummy,
        |_, _| None,
    );
    assert_eq!(result.detonations.len(), 1);
    assert_eq!(
        result.detonations[0].impact,
        ProjectileCoord::new(aim.x, aim.y, 208)
    );
    assert_eq!(
        result.detonations[0].reason,
        crate::sim::projectile::ProjectileDetonationReason::Fuse
    );
}

fn gsi_05_02_mixed_fixture() -> (Simulation, [u64; 6]) {
    let mut sim = Simulation::new();

    let entity_id = sim.allocate_stable_id();
    insert_entity(&mut sim, entity_id, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .attack_target = Some(AttackTarget::for_cell(1, 0));
    sim.register_live_object(entity_id);

    let anim_id = sim.allocate_stable_id();
    insert_anim(&mut sim, anim_id, true);
    sim.register_live_object(anim_id);

    let particle_id = sim.allocate_stable_id();
    insert_particle_system(&mut sim, particle_id);
    sim.register_live_object(particle_id);

    let terrain_id = sim.allocate_stable_id();
    sim.production.terrain_objects.insert(
        terrain_id,
        TerrainObjectState {
            stable_id: terrain_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref: sim.interner.intern("TREE01"),
            rx: 8,
            ry: 9,
            health: 10,
            max_health: 10,
            occupation_bits: 0,
            lifecycle: TerrainObjectLifecycle::Live,
        },
    );
    assert!(sim.register_terrain_object(terrain_id, None));

    let projectile_id = sim.allocate_stable_id();
    sim.admit_projectile(projectile_id, gsi_05_02_projectile(entity_id, None));

    let wave_id = sim.allocate_stable_id();
    sim.admit_wave(
        wave_id,
        Wave::new_owned(
            3,
            entity_id,
            TargetKind::Cell(1, 0),
            ProjectileCoord::new(0, 0, 0),
            ProjectileCoord::new(256, 0, 0),
        ),
    );

    let mixed = [
        terrain_id,
        entity_id,
        wave_id,
        anim_id,
        projectile_id,
        particle_id,
    ];
    sim.set_logic_order_for_test(mixed.to_vec());
    (sim, mixed)
}

#[test]
fn gsi_05_02_mixed_six_family_order_roundtrips_and_dispatches_every_slot() {
    let (sim, mixed) = gsi_05_02_mixed_fixture();
    let bytes = GameSnapshot::save(&sim, 0, 0, "logic-membership", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("mixed Logic snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("mixed Logic identities restore");

    assert_eq!(restored.live_object_order_snapshot(), mixed);
    assert!(
        restored
            .production
            .terrain_objects
            .get(&mixed[0])
            .unwrap()
            .in_logic_vector
    );
    assert!(
        restored
            .substrate
            .entities
            .get(mixed[1])
            .unwrap()
            .in_logic_vector
    );
    assert!(restored.waves.get(mixed[2]).unwrap().in_logic_vector);
    assert!(
        restored
            .substrate
            .anims
            .get(mixed[3])
            .unwrap()
            .in_logic_vector
    );
    assert!(restored.projectiles.get(mixed[4]).unwrap().in_logic_vector);
    assert!(
        restored
            .substrate
            .particle_systems
            .get(mixed[5])
            .unwrap()
            .in_logic_vector
    );

    let mut visited = Vec::new();
    restored.for_each_live_object(|sim, id| {
        if sim.object_ai_visit_one(id, None, ObjectAiCtx::default()) {
            visited.push(id);
        }
    });
    assert_eq!(visited, mixed);
    restored.debug_assert_logic_membership_consistent();
}

/// F13: the centralized object-kind dispatch (`classify_object` +
/// membership-flag get/set) preserves the exact pre-consolidation
/// registration/removal contract across all six object families — member
/// gate, tail append, first-match compacting erase, flag repair, and the
/// unrepresented-id rejection.
#[test]
fn substrate_registration_and_removal_dispatch_is_order_identical() {
    let (mut sim, mixed) = gsi_05_02_mixed_fixture();
    let [
        terrain_id,
        entity_id,
        wave_id,
        anim_id,
        projectile_id,
        particle_id,
    ] = mixed;

    // Re-registering an existing member of every family reports success and
    // appends nothing (the membership gate short-circuits before the push).
    for id in mixed {
        assert!(sim.register_live_object(id), "member re-register {id}");
    }
    assert_eq!(sim.live_object_order_snapshot(), mixed);

    // An id represented nowhere is rejected by both directions and leaves the
    // order untouched.
    let ghost = sim.allocate_stable_id();
    assert!(!sim.register_live_object(ghost));
    assert!(!sim.unregister_live_object(ghost));
    assert_eq!(sim.live_object_order_snapshot(), mixed);

    // First-match compacting erase from the middle: relative order of the
    // survivors is preserved and only the removed object's flag is repaired.
    assert!(sim.unregister_live_object(wave_id));
    assert_eq!(
        sim.live_object_order_snapshot(),
        vec![terrain_id, entity_id, anim_id, projectile_id, particle_id]
    );
    assert!(!sim.waves.get(wave_id).unwrap().in_logic_vector);
    assert!(
        sim.production
            .terrain_objects
            .get(&terrain_id)
            .unwrap()
            .in_logic_vector
    );
    assert!(
        sim.substrate
            .entities
            .get(entity_id)
            .unwrap()
            .in_logic_vector
    );
    assert!(sim.substrate.anims.get(anim_id).unwrap().in_logic_vector);
    assert!(sim.projectiles.get(projectile_id).unwrap().in_logic_vector);
    assert!(
        sim.substrate
            .particle_systems
            .get(particle_id)
            .unwrap()
            .in_logic_vector
    );

    // The flag gates a second removal; re-registration tail-appends rather
    // than restoring the old slot.
    assert!(!sim.unregister_live_object(wave_id));
    assert!(sim.register_live_object(wave_id));
    assert_eq!(
        sim.live_object_order_snapshot(),
        vec![
            terrain_id,
            entity_id,
            anim_id,
            projectile_id,
            particle_id,
            wave_id
        ]
    );
    assert!(sim.waves.get(wave_id).unwrap().in_logic_vector);

    // The retire subset dispatches through the same classifier: Terrain /
    // Bullet / Wave retire; Entity and Anim do not.
    assert!(!sim.retire_non_entity_object(entity_id));
    assert!(!sim.retire_non_entity_object(anim_id));
    assert!(sim.retire_non_entity_object(terrain_id));
    assert!(sim.substrate.pending_delete.contains(&terrain_id));
    assert!(!sim.live_object_order_snapshot().contains(&terrain_id));

    sim.debug_assert_logic_membership_consistent();
}

#[test]
fn gsi_05_02_tail_appends_run_same_pass_and_terminal_current_skips_successor() {
    let mut sim = Simulation::new();
    let entity_id = sim.allocate_stable_id();
    insert_entity(&mut sim, entity_id, EntityCategory::Unit);
    sim.register_live_object(entity_id);

    let mut appended = None;
    let mut visited = Vec::new();
    sim.for_each_live_object(|sim, id| {
        visited.push(id);
        let _ = sim.object_ai_visit_one(id, None, ObjectAiCtx::default());
        if id == entity_id {
            let projectile_id = sim.allocate_stable_id();
            sim.admit_projectile(projectile_id, gsi_05_02_projectile(entity_id, None));
            let wave_id = sim.allocate_stable_id();
            sim.admit_wave(
                wave_id,
                Wave::new(
                    3,
                    ProjectileCoord::new(0, 0, 0),
                    ProjectileCoord::new(256, 0, 0),
                ),
            );
            appended = Some((projectile_id, wave_id));
        }
    });
    let (projectile_id, wave_id) = appended.expect("tail objects appended");
    assert_eq!(visited, vec![entity_id, projectile_id, wave_id]);
    assert_eq!(
        sim.projectiles.get(projectile_id).unwrap().position,
        // CellClass target coordinates resolve from the live cell center on
        // every visit, so Cell(16, 0) contributes a small positive Y step.
        ProjectileCoord::new(64, 1, 0)
    );
    assert_eq!(sim.waves.get(wave_id).unwrap().lifetime, 99);

    let mut removal = Simulation::new();
    let terminal_id = removal.allocate_stable_id();
    removal.admit_projectile(terminal_id, gsi_05_02_projectile(0, Some(0)));
    let successor_id = removal.allocate_stable_id();
    removal.admit_wave(
        successor_id,
        Wave::new(
            3,
            ProjectileCoord::new(0, 0, 0),
            ProjectileCoord::new(256, 0, 0),
        ),
    );
    let mut removal_visited = Vec::new();
    removal.for_each_live_object(|sim, id| {
        removal_visited.push(id);
        let _ = sim.object_ai_visit_one(id, None, ObjectAiCtx::default());
    });
    assert_eq!(removal_visited, vec![terminal_id]);
    assert_eq!(removal.live_object_order_snapshot(), vec![successor_id]);
    assert!(removal.projectiles.get(terminal_id).is_some());
    assert_eq!(removal.substrate.pending_delete, vec![terminal_id]);
    assert_eq!(removal.waves.get(successor_id).unwrap().lifetime, 100);
    removal.debug_assert_logic_membership_consistent();
}

#[test]
fn gsi_05_02_terrain_map_construction_registers_before_later_techno() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n[TREE01]\nStrength=10\n",
        ))
        .expect("terrain fixture rules");
    let mut sim = Simulation::new();
    assert_eq!(
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &[crate::map::overlay::TerrainObject {
                rx: 3,
                ry: 4,
                name: "TREE01".to_owned(),
            }],
            &rules,
            false,
        ),
        1
    );
    let terrain_id = *sim.production.terrain_objects.keys().next().unwrap();

    let entity_id = sim.allocate_stable_id();
    insert_entity(&mut sim, entity_id, EntityCategory::Unit);
    sim.register_live_object(entity_id);
    assert_eq!(
        sim.live_object_order_snapshot(),
        vec![terrain_id, entity_id]
    );
}

#[test]
fn gsi_05_02_lethal_terrain_unregisters_and_inactive_slot_cannot_roundtrip() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n[Warheads]\n0=WOODWH\n\
             [TREE01]\nStrength=10\nArmor=wood\nImmune=no\n\
             [WOODWH]\nWood=yes\nCellSpread=1\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
        ))
        .expect("lethal terrain rules");
    let mut sim = Simulation::new();
    let terrain_id = sim.allocate_stable_id();
    let type_ref = sim.interner.intern("TREE01");
    let warhead_ref = sim.interner.intern("WOODWH");
    sim.production.terrain_objects.insert(
        terrain_id,
        TerrainObjectState {
            stable_id: terrain_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref,
            rx: 5,
            ry: 6,
            health: 10,
            max_health: 10,
            occupation_bits: 0,
            lifecycle: TerrainObjectLifecycle::Live,
        },
    );
    sim.production
        .terrain_object_cells
        .insert((5, 6), terrain_id);
    assert!(sim.register_terrain_object(terrain_id, None));

    sim.commit_noncombat_aoe_receivers(
        &rules,
        None,
        &[crate::sim::combat::combat_aoe::AreaDamageReceiver::Terrain(
            crate::sim::combat::TerrainDamageEvent {
                stable_id: terrain_id,
                rx: 5,
                ry: 6,
                damage: 10,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
        )],
    );
    let terrain = &sim.production.terrain_objects[&terrain_id];
    assert_eq!(terrain.lifecycle, TerrainObjectLifecycle::Destroyed);
    assert!(!terrain.in_logic_vector);
    assert!(sim.live_object_order_snapshot().is_empty());
    assert_eq!(sim.substrate.pending_delete, vec![terrain_id]);

    let bytes = GameSnapshot::save(&sim, 0, 0, "inactive-terrain", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("inactive terrain snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("inactive terrain remains outside Logic");
    assert!(restored.live_object_order_snapshot().is_empty());

    restored.set_logic_order_for_test(vec![terrain_id]);
    assert_eq!(
        restored.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::InactiveLogicIdentity {
            registry: "TerrainObjectStore",
            object_id: terrain_id,
        })
    );
}

#[test]
fn gsi_05_03_terminal_non_entities_remain_resolvable_until_common_drain() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n[Warheads]\n0=WOODWH\n\
             [TREE01]\nStrength=10\nArmor=wood\nImmune=no\n\
             [WOODWH]\nWood=yes\nCellSpread=1\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
        ))
        .expect("terminal non-entity rules");
    let mut sim = Simulation::new();

    let terrain_id = sim.allocate_stable_id();
    let terrain_type = sim.interner.intern("TREE01");
    let warhead_ref = sim.interner.intern("WOODWH");
    sim.production.terrain_objects.insert(
        terrain_id,
        TerrainObjectState {
            stable_id: terrain_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref: terrain_type,
            rx: 5,
            ry: 6,
            health: 10,
            max_health: 10,
            occupation_bits: 0,
            lifecycle: TerrainObjectLifecycle::Live,
        },
    );
    sim.production
        .terrain_object_cells
        .insert((5, 6), terrain_id);
    assert!(sim.register_terrain_object(terrain_id, None));
    sim.commit_noncombat_aoe_receivers(
        &rules,
        None,
        &[crate::sim::combat::combat_aoe::AreaDamageReceiver::Terrain(
            crate::sim::combat::TerrainDamageEvent {
                stable_id: terrain_id,
                rx: 5,
                ry: 6,
                damage: 10,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
        )],
    );

    let projectile_id = sim.allocate_stable_id();
    sim.admit_projectile(projectile_id, gsi_05_02_projectile(0, Some(0)));
    assert!(sim.object_ai_visit_one(projectile_id, None, ObjectAiCtx::default()));

    let wave_id = sim.allocate_stable_id();
    let mut terminal_wave = Wave::new(
        3,
        ProjectileCoord::new(0, 0, 0),
        ProjectileCoord::new(256, 0, 0),
    );
    terminal_wave.lifetime = 0;
    sim.admit_wave(wave_id, terminal_wave);
    assert!(sim.object_ai_visit_one(wave_id, None, ObjectAiCtx::default()));

    assert_eq!(
        sim.production.terrain_objects[&terrain_id].lifecycle,
        TerrainObjectLifecycle::Destroyed
    );
    assert!(sim.production.terrain_objects.contains_key(&terrain_id));
    assert!(sim.projectiles.get(projectile_id).is_some());
    assert!(sim.waves.get(wave_id).is_some());
    assert!(!sim.production.terrain_objects[&terrain_id].in_logic_vector);
    assert!(!sim.projectiles.get(projectile_id).unwrap().in_logic_vector);
    assert!(!sim.waves.get(wave_id).unwrap().in_logic_vector);
    assert_eq!(
        sim.substrate.pending_delete,
        vec![terrain_id, projectile_id, wave_id]
    );
    assert!(sim.live_object_order_snapshot().is_empty());

    let bytes = GameSnapshot::save(&sim, 0, 0, "terminal-non-entities", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("terminal non-entity snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("deferred non-entities restore outside Logic");
    assert_eq!(
        restored.substrate.pending_delete,
        vec![terrain_id, projectile_id, wave_id]
    );
    assert!(
        restored
            .production
            .terrain_objects
            .contains_key(&terrain_id)
    );
    assert!(restored.projectiles.get(projectile_id).is_some());
    assert!(restored.waves.get(wave_id).is_some());

    restored.process_pending_delete();
    assert!(
        !restored
            .production
            .terrain_objects
            .contains_key(&terrain_id)
    );
    assert!(restored.projectiles.get(projectile_id).is_none());
    assert!(restored.waves.get(wave_id).is_none());
    assert!(restored.substrate.pending_delete.is_empty());
}

#[test]
fn gsi_05_04_ground_source_and_target_retarget_before_removal_without_expiry() {
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    install_common_raw_terrain(&mut sim, 16, 16, 2, None);

    let target_id = sim.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(target_id, request(9, 11, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    let target_position = ProjectileCoord::new(
        9 * 256 + 128,
        11 * 256 + 64,
        2 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
    );
    let projectile_id = sim.allocate_stable_id();
    sim.admit_projectile(
        projectile_id,
        gsi_05_04_guided_projectile(
            target_id,
            ProjectileTarget::Entity(target_id),
            target_position,
        ),
    );
    sim.lifecycle_test_events.clear();

    sim.uninit(target_id);

    let cell_target = ProjectileTarget::Cell { rx: 9, ry: 11 };
    let projectile = sim
        .projectiles
        .get(projectile_id)
        .expect("Bullet remains stored throughout pointer cleanup");
    assert_eq!(projectile.source_id, crate::sim::combat::RAD_NO_ATTACKER);
    assert_eq!(projectile.target, cell_target);
    assert!(projectile.in_logic_vector);
    assert!(sim.substrate.entities.contains(target_id));
    assert!(sim.lifecycle_test_events.iter().any(|event| {
        *event
            == LifecycleTestEvent::ProjectilePointerExpiredVisited {
                expired_id: target_id,
                projectile_id,
                expired_resolvable: true,
                projectile_resolvable: true,
                source_id: crate::sim::combat::RAD_NO_ATTACKER,
                target: cell_target,
            }
    }));

    let _ = crate::sim::cell_rect::get_cellclass_fallback(sim.resolved_terrain.as_ref(), 20, -1);
    assert!(sim.object_ai_visit_one(projectile_id, None, ObjectAiCtx::default()));
    assert!(sim.pending_projectile_detonations.is_empty());
    assert!(sim.projectiles.get(projectile_id).is_some());
    assert_eq!(
        sim.projectiles.get(projectile_id).unwrap().target,
        cell_target
    );
    assert!(
        f64::from_bits(
            sim.projectiles
                .get(projectile_id)
                .unwrap()
                .velocity
                .y
                .bits()
        ) > 0.0,
        "allocated Cell target remains at (9,11) despite the unrelated dummy stamp"
    );
}

#[test]
fn gsi_05_04_building_get_coords_uses_foundation_center_cell() {
    let mut sim = Simulation::new();
    sim.session.map_width = 20;
    sim.session.map_height = 20;
    install_common_raw_terrain(&mut sim, 20, 20, 0, None);

    let target_id = sim.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Structure);
    let gapowr = sim.interner.intern("GAPOWR");
    {
        let target = sim.substrate.entities.get_mut(target_id).unwrap();
        target.type_ref = gapowr;
        target.foundation = "2x2".to_string();
    }
    assert!(matches!(
        sim.try_reveal_entity(target_id, common_raw_request(9, 11, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    let projectile_id = sim.allocate_stable_id();
    sim.admit_projectile(
        projectile_id,
        gsi_05_04_guided_projectile(
            crate::sim::combat::RAD_NO_ATTACKER,
            ProjectileTarget::Entity(target_id),
            ProjectileCoord::new(9 * 256 + 128, 11 * 256 + 128, 0),
        ),
    );

    sim.uninit(target_id);

    assert_eq!(
        sim.projectiles.get(projectile_id).unwrap().target,
        ProjectileTarget::Cell { rx: 10, ry: 12 },
        "BuildingClass GetCoords shifts GAPOWR's NW anchor by 128 leptons per 2x2 axis before truncation"
    );
}

#[test]
fn gsi_04_01_cell_target_uses_live_structural_bit_when_runtime_unwalkable() {
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    install_common_raw_terrain(&mut sim, 16, 16, 0, Some((6, 7)));
    {
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(6, 7)
            .unwrap();
        cell.level = 2;
        cell.slope_type = 1;
    }

    let target_id = sim.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(target_id, common_raw_request(6, 7, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    let projectile_id = sim.allocate_stable_id();
    let center_x = 6 * 256 + 128;
    let center_y = 7 * 256 + 128;
    let bridge_z = crate::util::lepton::ground_height_leptons(2, 1, center_x, center_y)
        .unwrap()
        .wrapping_add(crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32);
    let center = ProjectileCoord::new(center_x, center_y, bridge_z);
    let mut spawn = gsi_05_04_guided_projectile(
        crate::sim::combat::RAD_NO_ATTACKER,
        ProjectileTarget::Entity(target_id),
        center,
    );
    spawn.origin = center;
    spawn.guidance = None;
    spawn.tracks_target = false;
    sim.admit_projectile(projectile_id, spawn);

    sim.uninit(target_id);
    assert_eq!(
        sim.projectiles.get(projectile_id).unwrap().target,
        ProjectileTarget::Cell { rx: 6, ry: 7 }
    );
    assert_eq!(
        cell_target_coord(sim.resolved_terrain.as_ref(), 6, 7),
        center
    );
    {
        let terrain = sim.resolved_terrain.as_mut().expect("resolved terrain");
        let bridge_state = sim.bridge_state.as_mut().expect("bridge runtime state");
        assert!(matches!(
            bridge_state.body_cell_advance_state(6, 7, true, terrain),
            StateOutcome::Absorbed { .. }
        ));
        assert!(matches!(
            bridge_state.body_cell_advance_state(6, 7, true, terrain),
            StateOutcome::Collapsed { .. }
        ));
        assert!(!bridge_state.is_bridge_walkable(6, 7));
    }
    assert_eq!(
        cell_target_coord(sim.resolved_terrain.as_ref(), 6, 7),
        center,
        "CellClass target height follows live +0x100, not bridge runtime walkability"
    );

    assert!(sim.object_ai_visit_one(projectile_id, None, ObjectAiCtx::default()));

    // The actual Bullet visit consumes the live Cell target coordinate even
    // though runtime bridge walkability is false. Being stationary at that
    // target is not an admission: old height is 416, above both native tail
    // gates (4677D3: <208; 467B68: <10).
    assert!(sim.pending_projectile_detonations.is_empty());
    assert_eq!(sim.projectiles.get(projectile_id).unwrap().position, center);
}

#[test]
fn gsi_05_04_intact_bridge_cell_target_reaches_shrapnel_consumer() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=WH\n\
             [MTNK]\nStrength=100\nArmor=heavy\nPrimary=PARENT\nSecondary=CHILD\n\
             [PARENT]\nDamage=0\nROF=10\nRange=6\nSpeed=30\nProjectile=PARENTPROJ\nWarhead=WH\n\
             [PARENTPROJ]\nAirburst=yes\nShrapnelWeapon=CHILD\nShrapnelCount=-2\n\
             [CHILD]\nDamage=5\nROF=10\nRange=3\nSpeed=40\nProjectile=CHILDPROJ\nWarhead=WH\n\
             [CHILDPROJ]\nSubjectToWalls=yes\n\
             [WH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("bridge shrapnel rules");
    let mut sim = Simulation::with_seed(0x46_a310);
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    install_common_raw_terrain(&mut sim, 16, 16, 0, Some((6, 7)));

    let source_id = sim.allocate_stable_id();
    insert_entity(&mut sim, source_id, EntityCategory::Unit);
    let source_type = sim.interner.intern("MTNK");
    sim.substrate.entities.get_mut(source_id).unwrap().type_ref = source_type;
    let projectile_id = sim.allocate_stable_id();
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id,
        source_id,
        target: ProjectileTarget::Cell { rx: 6, ry: 7 },
        impact: ProjectileCoord::new(6 * 256 + 128, 7 * 256 + 128, 0),
        payload: ProjectilePayload {
            base_damage: 0,
            warhead: sim.interner.intern("WH"),
            weapon: sim.interner.intern("PARENT"),
        },
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };

    assert!(
        sim.bridge_state
            .as_ref()
            .is_some_and(|state| state.is_bridge_walkable(6, 7))
    );
    let result = sim.tick_combat_with_fatal_lifecycle(
        &rules,
        None,
        100,
        &[],
        &std::collections::BTreeSet::new(),
        &std::collections::BTreeSet::new(),
        &[detonation],
        &[],
    );

    assert_eq!(
        result.projectile_spawns.len(),
        1,
        "ShrapnelCount=-2 subtracts the intact deck target's one-cell vertical distance; suppressing the live +416 deck term would emit two children"
    );
}

#[test]
fn gsi_05_04_combat_fatal_expiry_keeps_authoritative_cell_target() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=VICTIM\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=KILLWH\n\
             [VICTIM]\nStrength=10\nArmor=light\n\
             [KILLWH]\nCellSpread=0\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("combat-fatal expiry rules");
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);

    let victim_id = sim.allocate_stable_id();
    insert_entity(&mut sim, victim_id, EntityCategory::Unit);
    let victim_type = sim.interner.intern("VICTIM");
    {
        let victim = sim.substrate.entities.get_mut(victim_id).unwrap();
        victim.type_ref = victim_type;
        victim.health.current = 10;
    }
    assert!(matches!(
        sim.try_reveal_entity(victim_id, common_raw_request(5, 6, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let listener_id = sim.allocate_stable_id();
    sim.admit_projectile(
        listener_id,
        gsi_05_04_guided_projectile(
            crate::sim::combat::RAD_NO_ATTACKER,
            ProjectileTarget::Entity(victim_id),
            ProjectileCoord::new(5 * 256 + 128, 6 * 256 + 128, 0),
        ),
    );
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 9000,
        source_id: crate::sim::combat::RAD_NO_ATTACKER,
        target: ProjectileTarget::Entity(victim_id),
        impact: ProjectileCoord::new(5 * 256 + 128, 6 * 256 + 128, 0),
        payload: ProjectilePayload {
            base_damage: 10,
            warhead: sim.interner.intern("KILLWH"),
            weapon: sim.interner.intern("MISSINGWEAPON"),
        },
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let logic_order = sim.live_object_order_snapshot();

    let _ = sim.tick_combat_with_fatal_lifecycle(
        &rules,
        None,
        100,
        &logic_order,
        &std::collections::BTreeSet::new(),
        &std::collections::BTreeSet::new(),
        &[detonation],
        &[],
    );

    let victim = sim
        .substrate
        .entities
        .get(victim_id)
        .expect("fatal target remains resolvable before the common drain");
    assert!(!victim.lifecycle.object_alive);
    assert!(victim.lifecycle.in_limbo);
    assert!(sim.substrate.pending_delete.contains(&victim_id));
    assert_eq!(
        sim.projectiles.get(listener_id).unwrap().target,
        ProjectileTarget::Cell { rx: 5, ry: 6 },
        "the synchronous fatal UnInit must read the same allocated CellClass terrain authority as combat before late deletion"
    );
}

#[test]
fn gsi_05_04_combat_fatal_garrison_recursion_keeps_cell_target() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=OCCUPANT\n\
             [VehicleTypes]\n0=BLOCKER\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=GARRISON\n\
             [Warheads]\n0=KILLWH\n\
             [OCCUPANT]\nStrength=10\nArmor=none\n\
             [BLOCKER]\nStrength=100\nArmor=heavy\n\
             [GARRISON]\nStrength=10\nArmor=concrete\nFoundation=2x2\nCanBeOccupied=yes\n\
             [KILLWH]\nCellSpread=0\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("fatal garrison recursion rules");
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);

    let building_id = sim.allocate_stable_id();
    insert_entity(&mut sim, building_id, EntityCategory::Structure);
    let passenger_id = sim.allocate_stable_id();
    insert_entity(&mut sim, passenger_id, EntityCategory::Infantry);
    let building_type = sim.interner.intern("GARRISON");
    let passenger_type = sim.interner.intern("OCCUPANT");
    {
        let passenger = sim.substrate.entities.get_mut(passenger_id).unwrap();
        passenger.type_ref = passenger_type;
        passenger.passenger_role = PassengerRole::Inside {
            transport_id: building_id,
        };
    }
    {
        let building = sim.substrate.entities.get_mut(building_id).unwrap();
        building.type_ref = building_type;
        building.foundation = "2x2".to_string();
        building.health.current = 10;
        let mut cargo = PassengerCargo::new(5, 1);
        assert!(cargo.board(passenger_id, 1));
        building.passenger_role = PassengerRole::Transport { cargo };
    }
    assert!(matches!(
        sim.try_reveal_entity(building_id, common_raw_request(8, 8, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    // Destruction's native no-exit arm is selected only when every perimeter
    // probe rejects. Occupied blockers establish that production condition.
    let blocker_type = sim.interner.intern("BLOCKER");
    for (rx, ry) in [
        (10, 10),
        (10, 9),
        (10, 8),
        (10, 7),
        (9, 10),
        (8, 10),
        (7, 10),
        (8, 7),
        (9, 7),
        (7, 8),
        (7, 9),
    ] {
        let blocker_id = sim.allocate_stable_id();
        insert_entity(&mut sim, blocker_id, EntityCategory::Unit);
        sim.substrate.entities.get_mut(blocker_id).unwrap().type_ref = blocker_type;
        assert!(matches!(
            sim.try_reveal_entity(blocker_id, common_raw_request(rx, ry, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));
    }

    let listener_id = sim.allocate_stable_id();
    sim.admit_projectile(
        listener_id,
        gsi_05_04_guided_projectile(
            crate::sim::combat::RAD_NO_ATTACKER,
            ProjectileTarget::Entity(passenger_id),
            ProjectileCoord::new(2 * 256, 3 * 256, 0),
        ),
    );
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 9001,
        source_id: crate::sim::combat::RAD_NO_ATTACKER,
        target: ProjectileTarget::Entity(building_id),
        impact: ProjectileCoord::new(8 * 256 + 128, 8 * 256 + 128, 0),
        payload: ProjectilePayload {
            base_damage: 10,
            warhead: sim.interner.intern("KILLWH"),
            weapon: sim.interner.intern("MISSINGWEAPON"),
        },
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let logic_order = sim.live_object_order_snapshot();

    let _ = sim.tick_combat_with_fatal_lifecycle(
        &rules,
        None,
        100,
        &logic_order,
        &std::collections::BTreeSet::new(),
        &std::collections::BTreeSet::new(),
        &[detonation],
        &[],
    );

    let passenger = sim
        .substrate
        .entities
        .get(passenger_id)
        .expect("no-exit occupant remains resolvable before common drain");
    assert!(!passenger.lifecycle.object_alive);
    assert!(passenger.lifecycle.in_limbo);
    assert!(sim.substrate.pending_delete.contains(&passenger_id));
    assert_eq!(
        sim.projectiles.get(listener_id).unwrap().target,
        ProjectileTarget::Cell { rx: 2, ry: 3 },
        "recursive no-exit passenger UnInit inherits combat's allocated CellClass authority"
    );
}

#[test]
fn gsi_04_01_unallocated_expiry_retains_live_dummy_identity_for_bullet_ai() {
    assert_unallocated_expiry_retains_live_dummy_identity(false);
}

#[test]
fn receiver_fatal_expiry_retains_live_dummy_identity_for_bullet_ai() {
    assert_unallocated_expiry_retains_live_dummy_identity(true);
}

fn assert_unallocated_expiry_retains_live_dummy_identity(through_receiver: bool) {
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);

    let target_id = sim.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(target_id, request(8, 8, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));

    let allocated = (0..16)
        .flat_map(|ry| (0..16).map(move |rx| (rx, ry)))
        .filter(|&cell| cell != (8, 8))
        .collect::<Vec<_>>();
    sim.resolved_terrain
        .as_mut()
        .expect("terrain")
        .test_set_native_allocated_cells(&allocated);

    let projectile_id = sim.allocate_stable_id();
    sim.admit_projectile(
        projectile_id,
        gsi_05_04_guided_projectile(
            crate::sim::combat::RAD_NO_ATTACKER,
            ProjectileTarget::Entity(target_id),
            ProjectileCoord::new(8 * 256 + 128, 8 * 256 + 128, 0),
        ),
    );

    if through_receiver {
        commit_receiver_authority_lethal_hit(&mut sim, target_id);
    } else {
        sim.uninit(target_id);
    }

    assert_eq!(
        sim.projectiles.get(projectile_id).unwrap().target,
        ProjectileTarget::DummyCell,
        "MapClass::Get_CellClass @ 0x005657A0 answers an unallocated slot with \
         the shared dummy CellClass carrying the requested coord, never NULL"
    );
    assert!(sim.projectiles.get(projectile_id).unwrap().in_logic_vector);
    let dummy = sim.effective_shared_cell_dummy();
    assert_eq!(dummy.snapshot().coord, (8, 8));

    // Any later packed miss writes the same object. BulletClass::AI @
    // 0x004666E0 dispatches the retained pointer, so steering observes this
    // later coordinate rather than a cleanup-time snapshot.
    assert!(matches!(
        crate::sim::cell_rect::get_cellclass_fallback(sim.resolved_terrain.as_ref(), 20, -1,),
        crate::sim::cell_rect::CellRef::Dummy { .. }
    ));
    assert_eq!(dummy.snapshot().coord, (20, -1));
    assert!(sim.object_ai_visit_one(projectile_id, None, ObjectAiCtx::default()));
    assert!(
        f64::from_bits(
            sim.projectiles
                .get(projectile_id)
                .unwrap()
                .velocity
                .y
                .bits()
        ) < 0.0,
        "guided Bullet must steer toward the later south-negative dummy stamp"
    );
}

/// The sentinel arm of `BulletClass::PointerExpired`: `0x0046856E`/`0x0046857C`
/// compare the truncated cell against `DAT_0089DDF0`/`DAT_0089DDF2`, both of
/// which are zero, and a match writes the zeroed `EBX` into the target slot.
///
/// The scenario is production-unreachable on both sides, and deliberately so:
/// native's diamond guard never allocates index 0, and a production
/// `ResolvedTerrainGrid` carries the same allocation mask. The rectangular
/// `from_cells` grid this test builds is what lets a unit stand there at all.
/// The arm is modelled because it is the native gate, not because play reaches
/// it.
#[test]
fn gsi_05_04_sentinel_origin_cell_target_becomes_explicit_null() {
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);

    let target_id = sim.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(target_id, request(0, 0, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));

    let projectile_id = sim.allocate_stable_id();
    sim.admit_projectile(
        projectile_id,
        gsi_05_04_guided_projectile(
            crate::sim::combat::RAD_NO_ATTACKER,
            ProjectileTarget::Entity(target_id),
            ProjectileCoord::new(128, 128, 0),
        ),
    );

    sim.uninit(target_id);

    assert_eq!(
        sim.projectiles.get(projectile_id).unwrap().target,
        ProjectileTarget::None,
        "cell (0, 0) is the native sentinel, so the target is cleared rather \
         than retained as a Cell"
    );
    assert!(sim.projectiles.get(projectile_id).unwrap().in_logic_vector);
}

#[test]
fn gsi_05_04_high_flying_source_and_target_become_explicit_null() {
    let mut sim = Simulation::new();
    sim.session.map_width = 32;
    sim.session.map_height = 32;
    // Installed so the case is production-shaped rather than a bare store; the
    // replacement no longer reads terrain either way.
    // `ObjectClass::IsHighFlying @ 0x005F6B90` is the only thing keeping this
    // target from becoming the aircraft's Cell: drop that gate and the three
    // *target* assertions below flip to `Cell { rx: 13, ry: 15 }`. The two
    // `source_id` assertions do not move — native's firer clear at
    // `0x00468503` is gated on the firer matching the expiring pointer
    // (`0x004684FF`), never on the high-flying predicate.
    install_common_raw_terrain(&mut sim, 32, 32, 0, None);

    let target_id = sim.allocate_stable_id();
    install_fly_aircraft(&mut sim, target_id, SimFixed::from_num(208));
    assert!(matches!(
        sim.try_reveal_entity(target_id, request(13, 15, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    let projectile_id = sim.allocate_stable_id();
    let mut spawn = gsi_05_04_guided_projectile(
        target_id,
        ProjectileTarget::Entity(target_id),
        ProjectileCoord::new(13 * 256 + 128, 15 * 256 + 64, 208),
    );
    spawn.origin = ProjectileCoord::new(1024, 1024, 1000);
    sim.admit_projectile(projectile_id, spawn);
    sim.lifecycle_test_events.clear();

    sim.uninit(target_id);

    let projectile = sim.projectiles.get(projectile_id).unwrap();
    assert_eq!(projectile.source_id, crate::sim::combat::RAD_NO_ATTACKER);
    assert_eq!(projectile.target, ProjectileTarget::None);
    assert!(projectile.in_logic_vector);
    assert!(sim.lifecycle_test_events.iter().any(|event| {
        *event
            == LifecycleTestEvent::ProjectilePointerExpiredVisited {
                expired_id: target_id,
                projectile_id,
                expired_resolvable: true,
                projectile_resolvable: true,
                source_id: crate::sim::combat::RAD_NO_ATTACKER,
                target: ProjectileTarget::None,
            }
    }));

    let bytes = GameSnapshot::save(&sim, 0, 0, "null-bullet-target", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("null Bullet target snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("null Bullet target remains valid on restore");
    assert_eq!(
        restored.projectiles.get(projectile_id).unwrap().target,
        ProjectileTarget::None
    );
    assert!(restored.object_ai_visit_one(projectile_id, None, ObjectAiCtx::default()));
    assert!(restored.pending_projectile_detonations.is_empty());
    let advanced = restored.projectiles.get(projectile_id).unwrap();
    assert_eq!(advanced.target, ProjectileTarget::None);
    assert!(
        f64::from_bits(advanced.velocity.y.bits()) < 0.0,
        "guided Bullet AI steers toward native null's zero CoordStruct, not its cached target"
    );
}

#[test]
fn gsi_05_04_cell_target_cleanup_is_idempotent_across_duplicate_uninit() {
    let mut sim = Simulation::new();
    sim.session.map_width = 8;
    sim.session.map_height = 8;

    let target_id = sim.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(target_id, request(9, 9, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    let projectile_id = sim.allocate_stable_id();
    sim.admit_projectile(
        projectile_id,
        gsi_05_04_guided_projectile(
            crate::sim::combat::RAD_NO_ATTACKER,
            ProjectileTarget::Entity(target_id),
            ProjectileCoord::new(9 * 256 + 128, 9 * 256 + 64, 0),
        ),
    );

    sim.uninit(target_id);
    let after_first = sim.projectiles.get(projectile_id).unwrap().clone();
    assert_eq!(after_first.target, ProjectileTarget::Cell { rx: 9, ry: 9 });
    sim.lifecycle_test_events.clear();

    sim.uninit(target_id);

    assert_eq!(sim.projectiles.get(projectile_id).unwrap(), &after_first);
    assert_eq!(sim.substrate.pending_delete, vec![target_id, target_id]);
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::ProjectilePointerExpiredVisited {
                    expired_id,
                    projectile_id: visited_id,
                    source_id: crate::sim::combat::RAD_NO_ATTACKER,
                    target: ProjectileTarget::Cell { rx: 9, ry: 9 },
                    ..
                } if *expired_id == target_id && *visited_id == projectile_id
            ))
            .count(),
        1,
        "the repeated UnInit performs one direct, idempotent Bullet callback"
    );
}

#[test]
fn gsi_05_04_projectile_listener_keeps_mixed_object_construction_order() {
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;

    let entity_id = sim.allocate_stable_id();
    insert_entity(&mut sim, entity_id, EntityCategory::Unit);

    let projectile_id = sim.allocate_stable_id();
    let target_id = 5;
    sim.admit_projectile(
        projectile_id,
        gsi_05_04_guided_projectile(
            target_id,
            ProjectileTarget::Entity(target_id),
            ProjectileCoord::new(2 * 256, 3 * 256, 0),
        ),
    );

    let anim_id = sim.allocate_stable_id();
    insert_anim(&mut sim, anim_id, false);
    let particle_id = sim.allocate_stable_id();
    insert_particle_system(&mut sim, particle_id);
    assert_eq!(sim.allocate_stable_id(), target_id);
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    let _ = sim.reveal(target_id);
    sim.lifecycle_test_events.clear();

    sim.uninit(target_id);

    let visited = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id,
                listener_id,
                ..
            } if *expired_id == target_id => Some(*listener_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        visited,
        vec![
            entity_id,
            projectile_id,
            anim_id,
            particle_id,
            target_id,
            entity_id,
            projectile_id,
            anim_id,
            particle_id,
            target_id,
        ]
    );
    assert!(sim.lifecycle_test_events.iter().any(|event| matches!(
        event,
        LifecycleTestEvent::ProjectilePointerExpiredVisited {
            expired_id,
            projectile_id: visited_id,
            expired_resolvable: true,
            projectile_resolvable: true,
            ..
        } if *expired_id == target_id && *visited_id == projectile_id
    )));
    assert!(sim.projectiles.get(projectile_id).is_some());
    assert!(sim.projectiles.get(projectile_id).unwrap().in_logic_vector);
}

fn gsi_01_05_damage_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n0=VICTIM\n1=SUCCESSOR\n2=FIRER\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=DUPBLDG\n\
             [Warheads]\n0=KILLWH\n\
             [VICTIM]\nStrength=30\nArmor=light\n\
             [SUCCESSOR]\nStrength=30\nArmor=light\n\
             [FIRER]\nStrength=30\nArmor=light\nPrimary=SONIC\nElitePrimary=SONICE\n\
             [DUPBLDG]\nStrength=10\nArmor=concrete\nFoundation=2x1\n\
             [SONIC]\nDamage=1\nAmbientDamage=10\nWarhead=KILLWH\nIsSonic=yes\n\
             [SONICE]\nDamage=2\nAmbientDamage=15\nWarhead=KILLWH\nIsSonic=yes\n\
             [KILLWH]\nCellSpread=0\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("Logic-slot damage fixture rules")
}

#[test]
fn wave_pointer_expiry_target_survives_dying_then_uninit_starts_decay_tail() {
    let mut sim = Simulation::new();
    let owner_id = sim.allocate_stable_id();
    let target_id = sim.allocate_stable_id();
    insert_entity(&mut sim, owner_id, EntityCategory::Unit);
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    {
        let owner = sim.substrate.entities.get_mut(owner_id).unwrap();
        owner.position.rx = 0;
        owner.position.ry = 0;
        owner.attack_target = Some(AttackTarget::new(target_id));
    }
    {
        let target = sim.substrate.entities.get_mut(target_id).unwrap();
        target.position.rx = 4;
        target.position.ry = 0;
        target.health.current = 0;
        target.dying = true;
    }

    let wave_id = sim.allocate_stable_id();
    let wave = Wave::new_owned(
        0,
        owner_id,
        TargetKind::Entity(target_id),
        ProjectileCoord::new(128, 128, 0),
        ProjectileCoord::new(4 * 256 + 128, 128, 50),
    );
    sim.admit_wave(wave_id, wave);
    sim.active_wave_links.insert(owner_id, wave_id);

    let represented = sim.wave_update_context(wave_id);
    assert_eq!(
        represented.target_position,
        Some(ProjectileCoord::new(4 * 256 + 128, 128, 0)),
        "health zero and the death animation do not expire a Wave pointer",
    );
    let _ = sim
        .waves
        .advance_one(wave_id, represented, sim.resolved_terrain.as_ref())
        .expect("represented Wave AI");
    assert!(sim.waves.get(wave_id).unwrap().active_geometry);
    assert!(!sim.waves.get(wave_id).unwrap().decaying);

    sim.uninit(target_id);

    let wave = sim.waves.get(wave_id).expect("Wave remains represented");
    assert_eq!(wave.owner_id, Some(owner_id));
    assert_eq!(wave.target_ref, None, "UnInit synchronously expires +0xAC");
    assert_eq!(sim.active_wave_links.get(&owner_id), Some(&wave_id));
    let expired = sim.wave_update_context(wave_id);
    assert_eq!(expired.target_position, None);
    let _ = sim
        .waves
        .advance_one(wave_id, expired, sim.resolved_terrain.as_ref())
        .expect("post-expiry Wave AI");
    let wave = sim.waves.get(wave_id).unwrap();
    assert!(!wave.active_geometry);
    assert!(wave.decaying, "null target activates the decay/fade tail");
}

#[test]
fn wave_pointer_expiry_owner_allows_dying_damage_then_uninit_nulls_later_calls() {
    let rules = gsi_01_05_damage_rules();
    let mut sim = Simulation::new();
    let owner_id = sim.allocate_stable_id();
    insert_entity(&mut sim, owner_id, EntityCategory::Unit);
    {
        let owner = sim.substrate.entities.get_mut(owner_id).unwrap();
        owner.type_ref = sim.interner.intern("FIRER");
        owner.health.current = 0;
        owner.dying = true;
        owner.attack_target = Some(AttackTarget::for_cell(4, 5));
    }
    let receiver_id = sim.allocate_stable_id();
    insert_entity(&mut sim, receiver_id, EntityCategory::Unit);
    {
        let receiver = sim.substrate.entities.get_mut(receiver_id).unwrap();
        receiver.type_ref = sim.interner.intern("VICTIM");
        receiver.health = Health { current: 30 };
    }
    assert!(matches!(
        sim.try_reveal_entity(receiver_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let wave_id = sim.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        owner_id,
        TargetKind::Cell(4, 5),
        ProjectileCoord::new(0, 0, 0),
        ProjectileCoord::new(4 * 256, 5 * 256, 50),
    );
    wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 5)]);
    sim.admit_wave(wave_id, wave);
    sim.active_wave_links.insert(owner_id, wave_id);
    let request = crate::sim::wave::WaveDamageRequest {
        wave_id,
        firer_id: owner_id,
        recorded_cells: vec![WaveRecordedCell::real(4, 5)],
        wave_z: 0,
    };
    assert!(
        sim.wave_update_context(wave_id).owner_position.is_some(),
        "the dying-but-represented owner still supplies live Wave coordinates",
    );
    assert_eq!(sim.active_wave_links.get(&owner_id), Some(&wave_id));

    sim.commit_logic_wave_damage_request(&rules, None, &request);
    assert_eq!(
        sim.substrate
            .entities
            .get(receiver_id)
            .unwrap()
            .health
            .current,
        20,
        "DamageArea consults the represented owner pointer, not health/dying state",
    );

    sim.uninit(owner_id);
    assert_eq!(sim.waves.get(wave_id).unwrap().owner_id, None);
    assert_eq!(
        sim.waves.get(wave_id).unwrap().target_ref,
        Some(TargetKind::Cell(4, 5)),
        "owner expiry does not clear an unrelated CellClass target",
    );
    assert!(!sim.active_wave_links.contains_key(&owner_id));

    sim.commit_logic_wave_damage_request(&rules, None, &request);
    assert_eq!(
        sim.substrate
            .entities
            .get(receiver_id)
            .unwrap()
            .health
            .current,
        20,
        "the now-null live owner makes every later DamageArea call a no-op",
    );
}

#[test]
fn wave_pointer_expiry_ignores_unrelated_object_and_preserves_owner_link() {
    let mut sim = Simulation::new();
    let owner_id = sim.allocate_stable_id();
    let target_id = sim.allocate_stable_id();
    let unrelated_id = sim.allocate_stable_id();
    for id in [owner_id, target_id, unrelated_id] {
        insert_entity(&mut sim, id, EntityCategory::Unit);
    }
    sim.substrate
        .entities
        .get_mut(owner_id)
        .unwrap()
        .attack_target = Some(AttackTarget::new(target_id));
    let wave_id = sim.allocate_stable_id();
    sim.admit_wave(
        wave_id,
        Wave::new_owned(
            0,
            owner_id,
            TargetKind::Entity(target_id),
            ProjectileCoord::new(0, 0, 0),
            ProjectileCoord::new(1_024, 0, 50),
        ),
    );
    sim.active_wave_links.insert(owner_id, wave_id);

    sim.uninit(unrelated_id);

    let wave = sim.waves.get(wave_id).expect("unrelated expiry keeps Wave");
    assert_eq!(wave.owner_id, Some(owner_id));
    assert_eq!(wave.target_ref, Some(TargetKind::Entity(target_id)));
    assert_eq!(sim.active_wave_links.get(&owner_id), Some(&wave_id));
}

#[test]
fn gsi_01_05_lethal_bullet_commits_receiver_before_retirement_and_double_compaction() {
    let rules = gsi_01_05_damage_rules();
    let mut sim = Simulation::new();
    let victim_id = sim.allocate_stable_id();
    insert_entity(&mut sim, victim_id, EntityCategory::Unit);
    let victim_type = sim.interner.intern("VICTIM");
    {
        let victim = sim.substrate.entities.get_mut(victim_id).unwrap();
        victim.type_ref = victim_type;
        victim.health.current = 10;
    }
    assert!(matches!(
        sim.try_reveal_entity(victim_id, common_raw_request(5, 6, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let successor_id = sim.allocate_stable_id();
    insert_entity(&mut sim, successor_id, EntityCategory::Unit);
    let successor_type = sim.interner.intern("SUCCESSOR");
    sim.substrate
        .entities
        .get_mut(successor_id)
        .unwrap()
        .type_ref = successor_type;
    assert!(matches!(
        sim.try_reveal_entity(successor_id, common_raw_request(8, 9, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let projectile_id = sim.allocate_stable_id();
    let impact = ProjectileCoord::new(5 * 256 + 128, 6 * 256 + 128, 0);
    let mut spawn = gsi_05_02_projectile(crate::sim::combat::RAD_NO_ATTACKER, Some(0));
    spawn.origin = impact;
    spawn.target = ProjectileTarget::Entity(victim_id);
    spawn.initial_target_position = impact;
    spawn.payload = ProjectilePayload {
        base_damage: 10,
        warhead: sim.interner.intern("KILLWH"),
        weapon: sim.interner.intern("MISSINGWEAPON"),
    };
    sim.admit_projectile(projectile_id, spawn);
    sim.set_logic_order_for_test(vec![projectile_id, victim_id, successor_id]);
    sim.lifecycle_test_events.clear();

    let mut visited = Vec::new();
    sim.for_each_live_object(|sim, id| {
        visited.push(id);
        assert!(sim.object_ai_visit_one(id, Some(&rules), ObjectAiCtx::default()));
    });

    assert_eq!(visited, vec![projectile_id]);
    assert_eq!(sim.live_object_order_snapshot(), vec![successor_id]);
    let victim = sim
        .substrate
        .entities
        .get(victim_id)
        .expect("fatal receiver remains resolvable until common drain");
    assert!(!victim.lifecycle.object_alive);
    assert!(victim.lifecycle.in_limbo);
    assert_eq!(victim.mission.ai_counter(), 0);
    assert_eq!(
        sim.substrate
            .entities
            .get(successor_id)
            .unwrap()
            .mission
            .ai_counter(),
        0,
        "receiver removal followed by current Bullet removal skips the shifted successor"
    );
    assert_eq!(sim.substrate.pending_delete, vec![victim_id, projectile_id]);
    assert!(sim.pending_projectile_detonations.is_empty());
    assert!(sim.projectiles.get(projectile_id).is_some());
    assert!(!sim.projectiles.get(projectile_id).unwrap().in_logic_vector);

    let queued = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::PendingDeleteQueued { stable_id } => Some(*stable_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        queued,
        vec![victim_id, projectile_id],
        "fatal receiver UnInit completes before Bullet UnInit/retirement"
    );
}

#[test]
fn gsi_01_05_terminal_wave_damages_once_before_single_current_removal() {
    let rules = gsi_01_05_damage_rules();
    let mut sim = Simulation::new();
    let victim_id = sim.allocate_stable_id();
    insert_entity(&mut sim, victim_id, EntityCategory::Unit);
    let victim_type = sim.interner.intern("VICTIM");
    sim.substrate.entities.get_mut(victim_id).unwrap().type_ref = victim_type;
    sim.substrate.entities.get_mut(victim_id).unwrap().health = Health { current: 30 };
    assert!(matches!(
        sim.try_reveal_entity(victim_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let successor_id = sim.allocate_stable_id();
    insert_entity(&mut sim, successor_id, EntityCategory::Unit);
    let successor_type = sim.interner.intern("SUCCESSOR");
    sim.substrate
        .entities
        .get_mut(successor_id)
        .unwrap()
        .type_ref = successor_type;
    assert!(matches!(
        sim.try_reveal_entity(successor_id, common_raw_request(8, 9, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let wave_id = sim.allocate_stable_id();
    let firer_id = sim.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    sim.substrate.entities.get_mut(firer_id).unwrap().type_ref = sim.interner.intern("FIRER");
    sim.substrate
        .entities
        .get_mut(firer_id)
        .unwrap()
        .attack_target = Some(AttackTarget {
        target: TargetKind::Entity(victim_id),
        pending_infantry_fire: None,
    });
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Entity(victim_id),
        ProjectileCoord::new(4 * 256, 5 * 256, 0),
        ProjectileCoord::new(5 * 256, 5 * 256, 0),
    );
    wave.active_geometry = false;
    wave.decaying = true;
    wave.fade_in = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.fade_out = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 5)]);
    sim.admit_wave(wave_id, wave);
    sim.set_logic_order_for_test(vec![wave_id, victim_id, successor_id]);
    sim.lifecycle_test_events.clear();

    let mut visited = Vec::new();
    sim.for_each_live_object(|sim, id| {
        visited.push(id);
        assert!(sim.object_ai_visit_one(id, Some(&rules), ObjectAiCtx::default()));
    });

    assert_eq!(visited, vec![wave_id, successor_id]);
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .health
            .current,
        20,
        "Wave receiver fires once at its Logic slot and is not replayed later"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .mission
            .ai_counter(),
        0,
        "retiring the current Wave shifts and skips the victim slot once"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(successor_id)
            .unwrap()
            .mission
            .ai_counter(),
        1
    );
    assert!(sim.pending_wave_damage_requests.is_empty());
    assert_eq!(sim.substrate.pending_delete, vec![wave_id]);
    assert_eq!(
        sim.live_object_order_snapshot(),
        vec![victim_id, successor_id]
    );
    assert!(sim.waves.get(wave_id).is_some());
    assert!(!sim.waves.get(wave_id).unwrap().in_logic_vector);
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| {
                **event == (LifecycleTestEvent::PendingDeleteQueued { stable_id: wave_id })
            })
            .count(),
        1
    );
}

#[test]
fn terminal_type_zero_wave_with_empty_recorded_vector_has_no_damage_area_tail() {
    let rules = gsi_01_05_damage_rules();
    let mut sim = Simulation::new();
    let firer_id = sim.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    {
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.type_ref = sim.interner.intern("FIRER");
        firer.attack_target = Some(AttackTarget {
            target: TargetKind::Cell(4, 5),
            pending_infantry_fire: None,
        });
    }
    let wave_id = sim.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Cell(4, 5),
        ProjectileCoord::new(4 * 256, 5 * 256, 0),
        ProjectileCoord::new(5 * 256, 5 * 256, 0),
    );
    wave.active_geometry = false;
    wave.decaying = true;
    wave.fade_in = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.fade_out = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    sim.admit_wave(wave_id, wave);
    sim.active_wave_links.insert(firer_id, wave_id);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x45_4d_50_54_59);
    let expected_rng = sim.scenario_rng.logical_state();

    assert!(sim.object_ai_visit_one(wave_id, Some(&rules), ObjectAiCtx::default()));

    assert_eq!(sim.scenario_rng.logical_state(), expected_rng);
    assert!(sim.active_wave_links.get(&firer_id).is_none());
    assert!(sim.pending_wave_damage_requests.is_empty());
    assert!(sim.dynamic_terrain_cells.is_empty());
    assert!(sim.radar_terrain_dirty_cells.is_empty());
    assert!(sim.tactical_dirty_cells.is_empty());
    assert_eq!(sim.substrate.pending_delete, vec![wave_id]);
}

#[test]
fn wave_elite_ambient_damage_carries_within_cell_and_resets_on_next_cell() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=FIRST\n1=SECOND\n2=NEXT\n3=FIRER\n\
             [AircraftTypes]\n[BuildingTypes]\n\
             [FIRST]\nStrength=100\nArmor=flak\n\
             [SECOND]\nStrength=100\nArmor=none\n\
             [NEXT]\nStrength=100\nArmor=none\n\
             [FIRER]\nStrength=100\nArmor=light\nPrimary=SONIC\nElitePrimary=SONICE\n\
             [SONIC]\nDamage=4\nAmbientDamage=10\nWarhead=WH\nIsSonic=yes\n\
             [SONICE]\nDamage=8\nAmbientDamage=15\nWarhead=WH\nIsSonic=yes\n\
             [WH]\nCellSpread=0\nPercentAtMax=1\n\
             Verses=100%,50%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("Wave shared-damage fixture");
    let mut sim = Simulation::new();
    let second_id = sim.allocate_stable_id();
    insert_entity(&mut sim, second_id, EntityCategory::Unit);
    sim.substrate.entities.get_mut(second_id).unwrap().type_ref = sim.interner.intern("SECOND");
    assert!(matches!(
        sim.try_reveal_entity(second_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let first_id = sim.allocate_stable_id();
    insert_entity(&mut sim, first_id, EntityCategory::Unit);
    sim.substrate.entities.get_mut(first_id).unwrap().type_ref = sim.interner.intern("FIRST");
    assert!(matches!(
        sim.try_reveal_entity(first_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let next_id = sim.allocate_stable_id();
    insert_entity(&mut sim, next_id, EntityCategory::Unit);
    sim.substrate.entities.get_mut(next_id).unwrap().type_ref = sim.interner.intern("NEXT");
    assert!(matches!(
        sim.try_reveal_entity(next_id, common_raw_request(5, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let firer_id = sim.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    {
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.type_ref = sim.interner.intern("FIRER");
        firer.veterancy = 200;
        firer.attack_target = Some(AttackTarget {
            target: TargetKind::Entity(next_id),
            pending_infantry_fire: None,
        });
    }

    let wave_id = sim.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Entity(next_id),
        ProjectileCoord::new(4 * 256, 5 * 256, 0),
        ProjectileCoord::new(6 * 256, 5 * 256, 0),
    );
    wave.active_geometry = false;
    wave.decaying = true;
    wave.fade_in = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.fade_out = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.replace_recorded_cells(vec![
        WaveRecordedCell::real(4, 5),
        WaveRecordedCell::real(5, 5),
    ]);
    sim.admit_wave(wave_id, wave);
    sim.active_wave_links.insert(firer_id, wave_id);
    sim.lifecycle_test_events.clear();

    assert!(sim.object_ai_visit_one(wave_id, Some(&rules), ObjectAiCtx::default()));

    assert_eq!(
        sim.substrate.entities.get(first_id).unwrap().health.current,
        93
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(second_id)
            .unwrap()
            .health
            .current,
        93,
        "second occupant receives the first callback's 7-point mutable value",
    );
    assert_eq!(
        sim.substrate.entities.get(next_id).unwrap().health.current,
        85,
        "the next recorded cell reloads elite AmbientDamage=15, never Damage=8",
    );
    assert!(!sim.active_wave_links.contains_key(&firer_id));
    let selected = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::WaveDamageReceiverSelected {
                wave_id: selected_wave,
                target_id,
                ..
            } if *selected_wave == wave_id => Some(*target_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(selected, vec![first_id, second_id, next_id]);
}

#[test]
fn wave_walks_nonbuilding_terrain_building_order_and_terrain_owns_wood_gate() {
    fn run(wood: bool) -> (Vec<u64>, i32, i32, i32) {
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[InfantryTypes]\n\
                 [VehicleTypes]\n0=UNIT\n1=FIRER\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n0=BLDG\n\
                 [TerrainTypes]\n0=TREE01\n\
                 [UNIT]\nStrength=100\nArmor=none\n\
                 [BLDG]\nStrength=100\nArmor=wood\nFoundation=1x1\n\
                 [FIRER]\nStrength=100\nArmor=light\nPrimary=SONIC\n\
                 [TREE01]\nStrength=100\nArmor=wood\nImmune=no\n\
                 [SONIC]\nDamage=4\nAmbientDamage=10\nWarhead=WH\nIsSonic=yes\n\
                 [WH]\nWood={}\nCellSpread=0\nPercentAtMax=1\n\
                 Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
                if wood { "yes" } else { "no" },
            )),
        )
        .expect("Wave Terrain fixture");
        let mut sim = Simulation::new();

        let unit_id = sim.allocate_stable_id();
        insert_entity(&mut sim, unit_id, EntityCategory::Unit);
        sim.substrate.entities.get_mut(unit_id).unwrap().type_ref = sim.interner.intern("UNIT");
        assert!(matches!(
            sim.try_reveal_entity(unit_id, common_raw_request(4, 5, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));

        let building_id = sim.allocate_stable_id();
        insert_entity(&mut sim, building_id, EntityCategory::Structure);
        {
            let building = sim.substrate.entities.get_mut(building_id).unwrap();
            building.type_ref = sim.interner.intern("BLDG");
            building.foundation = "1x1".to_string();
        }
        assert!(matches!(
            sim.try_reveal_entity(building_id, common_raw_request(4, 5, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));

        let terrain_id = sim.allocate_stable_id();
        sim.production.terrain_objects.insert(
            terrain_id,
            TerrainObjectState {
                stable_id: terrain_id,
                native_unique_id: None,
                in_logic_vector: false,
                type_ref: sim.interner.intern("TREE01"),
                rx: 4,
                ry: 5,
                health: 100,
                max_health: 100,
                occupation_bits: 0,
                lifecycle: TerrainObjectLifecycle::Live,
            },
        );
        sim.production
            .terrain_object_cells
            .insert((4, 5), terrain_id);

        let firer_id = sim.allocate_stable_id();
        insert_entity(&mut sim, firer_id, EntityCategory::Unit);
        {
            let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
            firer.type_ref = sim.interner.intern("FIRER");
            firer.attack_target = Some(AttackTarget {
                target: TargetKind::Entity(building_id),
                pending_infantry_fire: None,
            });
        }
        let wave_id = sim.allocate_stable_id();
        let mut wave = Wave::new_owned(
            0,
            firer_id,
            TargetKind::Entity(building_id),
            ProjectileCoord::new(4 * 256, 5 * 256, 0),
            ProjectileCoord::new(6 * 256, 5 * 256, 0),
        );
        wave.active_geometry = false;
        wave.decaying = true;
        wave.fade_in = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
        wave.fade_out = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
        wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 5)]);
        sim.admit_wave(wave_id, wave);
        sim.lifecycle_test_events.clear();

        assert!(sim.object_ai_visit_one(wave_id, Some(&rules), ObjectAiCtx::default()));
        let selected = sim
            .lifecycle_test_events
            .iter()
            .filter_map(|event| match event {
                LifecycleTestEvent::WaveDamageReceiverSelected {
                    wave_id: selected_wave,
                    target_id,
                    ..
                } if *selected_wave == wave_id => Some(*target_id),
                _ => None,
            })
            .collect::<Vec<_>>();
        (
            selected,
            i32::from(sim.substrate.entities.get(unit_id).unwrap().health.current),
            sim.production.terrain_objects[&terrain_id].health,
            i32::from(
                sim.substrate
                    .entities
                    .get(building_id)
                    .unwrap()
                    .health
                    .current,
            ),
        )
    }

    let (selected, unit_health, tree_health, building_health) = run(true);
    assert_eq!(
        selected,
        vec![1, 3, 2],
        "one-cell order is nonbuilding, Terrain, then Building",
    );
    assert_eq!(unit_health, 90);
    assert_eq!(tree_health, 90);
    assert_eq!(building_health, 90);

    let (_, unit_health, tree_health, building_health) = run(false);
    assert_eq!(unit_health, 90);
    assert_eq!(tree_health, 100, "TerrainClass owns the Warhead Wood gate");
    assert_eq!(building_health, 90);
}

#[test]
fn wave_tail_consumes_wall_roll_before_mandatory_cliff_chance_roll() {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[CombatDamage]\nCollapseChance=0\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=FIRER\n\
         [AircraftTypes]\n[BuildingTypes]\n\
         [OverlayTypes]\n0=WALLX\n\
         [FIRER]\nStrength=100\nArmor=light\nPrimary=SONIC\n\
         [SONIC]\nDamage=4\nAmbientDamage=10\nWarhead=WH\nIsSonic=yes\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [WALLX]\nWall=yes\nChainReaction=yes\nStrength=100\nDamageLevels=4\n",
    );
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("wall+cliff rules");
    let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = Simulation::new();
    let firer_id = sim.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    {
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.type_ref = sim.interner.intern("FIRER");
        firer.attack_target = Some(AttackTarget {
            target: TargetKind::Cell(4, 1),
            pending_infantry_fire: None,
        });
    }

    let mut cells = Vec::new();
    for ry in 0..4 {
        for rx in 0..6 {
            let mut cell = common_raw_terrain_cell(rx, ry, 1, false);
            let sub_tile = (ry * 6 + rx) as u8;
            if ![0, 5, 18, 23].contains(&usize::from(sub_tile)) {
                cell.final_tile_index = 100;
                cell.final_sub_tile = sub_tile;
            }
            cells.push(cell);
        }
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(6, 4, cells);
    terrain.test_install_destroyable_cliff_catalog(100);
    sim.resolved_terrain = Some(terrain);
    let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(6, 4);
    overlay.place_overlay(
        4,
        1,
        registry.id_for_name("WALLX").expect("wall overlay"),
        0,
    );
    sim.overlay_grid = Some(overlay);

    let wave_id = sim.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Cell(4, 1),
        ProjectileCoord::new(0, 0, 0),
        ProjectileCoord::new(4 * 256, 256, 50),
    );
    wave.active_geometry = false;
    wave.decaying = true;
    wave.fade_in = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.fade_out = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 1)]);
    sim.admit_wave(wave_id, wave);

    sim.scenario_rng = crate::sim::rng::SimRng::new(0x57_41_4c_4c);
    let mut expected = sim.scenario_rng.clone();
    let _wall_roll = expected.next_range_u32_inclusive(0, 100);
    let _cliff_chance_roll = expected.next_range_u32_inclusive(0, 99);

    assert!(sim.object_ai_visit_one(
        wave_id,
        Some(&rules),
        ObjectAiCtx {
            overlay_registry: Some(&registry),
            ..ObjectAiCtx::default()
        },
    ));

    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    assert!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .is_destroyable_cliff(4, 1)
    );
    assert!(sim.dynamic_terrain_cells.is_empty());
}

#[test]
fn wave_fatal_death_weapon_starts_crater_with_live_smudge_authority() {
    assert_direct_fatal_death_weapon_starts_crater(false);
}

#[test]
fn bridge_ground_fatal_death_weapon_starts_crater_before_receiver_returns() {
    assert_direct_fatal_death_weapon_starts_crater(true);
}

fn assert_direct_fatal_death_weapon_starts_crater(bridge: bool) {
    use crate::rules::ini_parser::IniFile;

    let ini = IniFile::from_str(
        "[CombatDamage]\nC4Warhead=KILLWH\n[InfantryTypes]\n[VehicleTypes]\n0=VICTIM\n1=FIRER\n\
         [AircraftTypes]\n[BuildingTypes]\n[OverlayTypes]\n\
         [Warheads]\n0=KILLWH\n1=CRATERWH\n[SmudgeTypes]\n0=CR1\n\
         [VICTIM]\nStrength=1\nArmor=light\nExplodes=yes\nDeathWeapon=DeathBoom\n\
         [FIRER]\nStrength=100\nArmor=light\nPrimary=SONIC\n\
         [SONIC]\nDamage=1\nAmbientDamage=10\nWarhead=KILLWH\nIsSonic=yes\n\
         [DeathBoom]\nDamage=1\nWarhead=CRATERWH\n\
         [KILLWH]\nCellSpread=0\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CRATERWH]\nCellSpread=0\nTiberium=no\nAnimList=CRATERANIM\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CR1]\nCrater=yes\nWidth=1\nHeight=1\n",
    );
    let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
    rules.art_registry = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[CRATERANIM]\nCrater=yes\nScorch=no\nStart=0\n",
    ));
    // One raw frame: no middle frame, so Start runs Middle at construction.
    rules
        .art_registry
        .bind_anim_frame_count_for_test("CRATERANIM", 1);
    let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = Simulation::with_seed(1);
    let mut cells = Vec::new();
    for ry in 0..10 {
        for rx in 0..10 {
            let mut cell = common_raw_terrain_cell(rx, ry, 0, false);
            cell.filled_clear = true;
            cell.accepts_smudge = true;
            cell.allows_tiberium = true;
            cells.push(cell);
        }
    }
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(10, 10, cells));
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(10, 10));
    sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(10, 10));
    sim.production.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(10, 10);
    let victim_id = sim.allocate_stable_id();
    insert_entity(&mut sim, victim_id, EntityCategory::Unit);
    let victim = sim.substrate.entities.get_mut(victim_id).unwrap();
    victim.type_ref = sim.interner.intern("VICTIM");
    victim.health = Health { current: 1 };
    assert!(matches!(
        sim.try_reveal_entity(victim_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    if bridge {
        super::bridge_orchestrator::kill_ground_occupants_at(
            &mut sim,
            &rules,
            4,
            5,
            Some(&registry),
        );
    } else {
        let firer_id = sim.allocate_stable_id();
        insert_entity(&mut sim, firer_id, EntityCategory::Unit);
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.type_ref = sim.interner.intern("FIRER");
        firer.attack_target = Some(AttackTarget::new(victim_id));
        let wave_id = sim.allocate_stable_id();
        let mut wave = Wave::new_owned(
            0,
            firer_id,
            TargetKind::Entity(victim_id),
            ProjectileCoord::new(4 * 256, 5 * 256, 0),
            ProjectileCoord::new(5 * 256, 5 * 256, 0),
        );
        wave.active_geometry = false;
        wave.decaying = true;
        wave.fade_in = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
        wave.fade_out = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
        wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 5)]);
        sim.admit_wave(wave_id, wave);

        // Wave vslot call 0x0075F42C -> DeathWeapon Detonate 0x0070D782 ->
        // zero-delay Anim constructor 0x00469C93 -> 0x00422702/0x00424D5A.
        // A type without a middle frame (+0x298 == 0) runs Middle from Start
        // (0x00424D5A), so its crater work, including Reduce_Tiberium at
        // 0x004250E7, is synchronous. Evidence: active gamemd.exe bodies/call
        // instructions.
        assert!(sim.object_ai_visit_one(
            wave_id,
            Some(&rules),
            ObjectAiCtx {
                overlay_registry: Some(&registry),
                ..ObjectAiCtx::default()
            },
        ));
    }
    assert!(!sim.substrate.entities.get(victim_id).unwrap().is_alive());
    let crater = rules.smudge_types.find_by_name("CR1").unwrap();
    assert_eq!(
        sim.smudge_grid.as_ref().unwrap().cell(4, 5).type_id,
        Some(crater),
        "nested DeathWeapon starts its crater before direct caller returns, without a later effects drain",
    );
}

#[test]
fn wave_cliff_collapse_consumes_exact_body_rng_and_spawns_row_major_anims() {
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::pathfinding::zone_hierarchy::{
        ZonePrecheckExclusions, ZonePrecheckOutcome, zone_precheck_flat,
    };

    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[CombatDamage]\nCollapseChance=100\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=FIRER\n\
         [AircraftTypes]\n[BuildingTypes]\n[OverlayTypes]\n0=DECAL\n\
         [FIRER]\nStrength=100\nArmor=light\nPrimary=SONIC\n\
         [SONIC]\nDamage=4\nAmbientDamage=10\nWarhead=WH\nIsSonic=yes\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [DECAL]\n",
    );
    let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("cliff body rules");
    let overlay_registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(
        &crate::rules::ini_parser::IniFile::from_str(
            "[XGRYMED1]\nEnd=1\nRate=1\nRandomRate=900,300\n\
             [XGRYMED2]\nEnd=1\nRate=1\nRandomRate=900,300\n\
             [XGRYSML1]\nEnd=1\nRate=1\nRandomRate=900,300\n",
        ),
    );
    for name in ["XGRYMED1", "XGRYMED2", "XGRYSML1"] {
        art.bind_anim_frame_count_for_test(name, 1);
    }
    rules.merge_art_data(&art);
    let mut sim = Simulation::new();
    let firer_id = sim.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    {
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.type_ref = sim.interner.intern("FIRER");
        firer.attack_target = Some(AttackTarget {
            target: TargetKind::Cell(4, 1),
            pending_infantry_fire: None,
        });
    }
    let mut cells = Vec::new();
    for ry in 0..16 {
        for rx in 0..16 {
            let on_bridge = ry == 10 && (10..=11).contains(&rx);
            let mut cell = common_raw_terrain_cell(rx, ry, 1, on_bridge);
            if rx < 6 && ry < 4 {
                let sub_tile = (ry * 6 + rx) as u8;
                if ![0, 5, 18, 23].contains(&usize::from(sub_tile)) {
                    cell.final_tile_index = 100;
                    cell.final_sub_tile = sub_tile;
                }
            }
            // The far bank is isolated by impassable cells. Connectivity to it
            // must come from the remote bridge's native endpoint record.
            if on_bridge
                || ((11..=13).contains(&rx) && (9..=11).contains(&ry) && (rx, ry) != (12, 10))
            {
                cell.ground_walk_blocked = true;
                cell.zone_type = 6;
            }
            if ry == 10 && (rx == 9 || rx == 12) {
                cell.final_tile_index = 1006;
                cell.final_sub_tile = 4;
            }
            cells.push(cell);
        }
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(16, 16, cells);
    terrain.test_install_destroyable_cliff_catalog(100);
    terrain.test_set_high_bridge_set_starts(Some(1000), None);
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain(&terrain, true, 1500),
    );
    let bridge_records = sim
        .bridge_state
        .as_ref()
        .unwrap()
        .endpoint_records()
        .to_vec();
    assert_eq!(bridge_records.len(), 1);
    assert!(bridge_records[0].active);
    assert_eq!(bridge_records[0].endpoint_a, (9, 10));
    assert_eq!(bridge_records[0].endpoint_b, (12, 10));
    let pristine_terrain = terrain.clone();
    sim.resolved_terrain = Some(terrain);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let remote_bridge_route = |zones: &crate::sim::pathfinding::zone_map::ZoneGrid| {
        let hierarchy = zones.hierarchy_for(MovementZone::Normal).unwrap();
        let start = hierarchy.level(0).unwrap().zone_at(9, 10);
        let goal = hierarchy.level(0).unwrap().zone_at(12, 10);
        (
            zones.can_reach(
                MovementZone::Normal,
                (9, 10),
                MovementLayer::Ground,
                (12, 10),
                MovementLayer::Ground,
            ),
            matches!(
                zone_precheck_flat(
                    hierarchy,
                    start,
                    goal,
                    MovementZone::Normal,
                    &ZonePrecheckExclusions::default()
                ),
                ZonePrecheckOutcome::Passed(_)
            ),
        )
    };
    assert_eq!(
        remote_bridge_route(sim.zone_grid.as_ref().unwrap()),
        (true, true)
    );
    let without_bridge_records = crate::sim::pathfinding::zone_map::ZoneGrid::build_with_terrain(
        sim.path_grid.as_ref().unwrap(),
        &sim.terrain_costs,
        sim.resolved_terrain.as_ref(),
        &[],
        16,
        16,
    );
    assert_eq!(remote_bridge_route(&without_bridge_records), (false, false));
    let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(16, 16);
    let decal = overlay_registry.id_for_name("DECAL").expect("test decal");
    overlay.place_overlay(0, 0, decal, 11);
    overlay.place_overlay(1, 0, decal, 12);
    overlay.retain_zero_wall_plane_for_tests();
    sim.overlay_grid = Some(overlay);
    let mut smudge = crate::sim::smudge_grid::SmudgeGrid::new(16, 16);
    for (rx, frame_offset) in [(0, 0), (1, 1)] {
        smudge.test_force_set(
            rx,
            0,
            crate::sim::smudge_grid::SmudgeCell {
                type_id: Some(9),
                footprint_origin: Some((0, 0)),
                frame_offset,
            },
        );
    }
    sim.smudge_grid = Some(smudge);

    let wave_id = sim.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Cell(4, 1),
        ProjectileCoord::new(0, 0, 0),
        ProjectileCoord::new(4 * 256, 256, 50),
    );
    wave.active_geometry = false;
    wave.decaying = true;
    wave.fade_in = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.fade_out = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 1)]);
    sim.admit_wave(wave_id, wave);

    sim.scenario_rng = crate::sim::rng::SimRng::new(0x43_4c_49_46_46);
    let mut expected_rng = sim.scenario_rng.clone();
    let chance = expected_rng.next_range_u32_inclusive(0, 99);
    assert!(chance < 100);
    let mut expected_spawns = Vec::new();
    for ry in 0..3i32 {
        for rx in 0..5i32 {
            for _ in 0..2 {
                let type_index = expected_rng.next_range_u32_inclusive(0, 2) as usize;
                let jitter_x = expected_rng.next_range_u32_inclusive(0, 16) as i32 - 8;
                let jitter_y = expected_rng.next_range_u32_inclusive(0, 24) as i32 - 12;
                let delay = expected_rng.next_range_u32_inclusive(0, 2) as u16;
                let rate = expected_rng.next_range_u32_inclusive(1, 3) as u16;
                expected_spawns.push((
                    type_index,
                    AnimWorldCoord {
                        x: rx * 256 + 128 + jitter_x,
                        y: ry * 256 + 128 + jitter_y,
                        z: 104,
                    },
                    delay,
                    rate,
                ));
            }
        }
    }

    assert!(sim.object_ai_visit_one(
        wave_id,
        Some(&rules),
        ObjectAiCtx {
            overlay_registry: Some(&overlay_registry),
            ..ObjectAiCtx::default()
        },
    ));

    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    assert_eq!(sim.dynamic_terrain_cells.len(), 20);
    // Check before any later frame repair: native cliff collapse rebuilds
    // connectivity synchronously with the retained global bridge vector.
    assert_eq!(
        remote_bridge_route(sim.zone_grid.as_ref().unwrap()),
        (true, true)
    );
    assert_eq!(
        sim.zone_grid.as_ref().unwrap().bridge_records(),
        bridge_records
    );
    let canonical_path = crate::sim::pathfinding::PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().unwrap(),
        sim.bridge_state.as_ref(),
    );
    for rx in 9..=12 {
        assert_eq!(
            sim.path_grid.as_ref().unwrap().cell(rx, 10),
            canonical_path.cell(rx, 10)
        );
    }
    assert_eq!(sim.radar_terrain_dirty_cells.len(), 20);
    assert_eq!(sim.tactical_dirty_cells.len(), 20);
    let overlay = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(overlay.cell(1, 0).overlay_id, None);
    assert_eq!(
        overlay.cell(0, 0).overlay_id,
        Some(decal),
        "sparse hole untouched"
    );
    let smudge = sim.smudge_grid.as_ref().unwrap();
    assert_eq!(
        smudge.cell(1, 0),
        &crate::sim::smudge_grid::SmudgeCell::default()
    );
    assert_eq!(
        smudge.cell(0, 0).type_id,
        Some(9),
        "raw clear must not expand footprint"
    );
    assert_eq!(sim.substrate.anims.len(), 30);
    let names = ["XGRYMED1", "XGRYMED2", "XGRYSML1"];
    let actual_spawns = sim
        .substrate
        .anims
        .iter()
        .map(|(_, anim)| {
            (
                names
                    .iter()
                    .position(|name| *name == sim.interner.resolve(anim.type_id))
                    .expect("one of the three cliff anim types"),
                anim.world_coord,
                anim.runtime.delay_remaining,
                anim.runtime.rate_reload,
                anim.draw_flags,
                anim.runtime.loop_remaining,
                anim.runtime.constructor_reverse,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual_spawns,
        expected_spawns
            .into_iter()
            .map(|(kind, coord, delay, rate)| { (kind, coord, delay, rate, 0x600, 1, false) })
            .collect::<Vec<_>>(),
    );
    assert!(
        sim.substrate
            .entities
            .get(firer_id)
            .unwrap()
            .attack_target
            .is_none()
    );

    // GameSnapshot's established full-deserialize seam canonicalizes Scenario
    // RNG to seed zero; the collapse draw-order assertion above already pins
    // the live cursor before normalizing this independent snapshot fixture.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_dynamic_terrain = sim.dynamic_terrain_cells.clone();
    let expected_hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "collapsed-dcliff", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("collapsed cliff snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("collapsed cliff stable identities");
    assert_eq!(
        restored.state_hash(),
        expected_hash,
        "serialized collapse state must hash equally before derived map caches rebuild",
    );
    restored.rebuild_caches_after_load(
        pristine_terrain,
        Default::default(),
        Vec::new(),
        Vec::new(),
    );
    restored
        .restore_map_authority_after_snapshot_load(&rules, &overlay_registry)
        .expect("dynamic cliff terrain reprojects over the pristine map");
    assert_eq!(restored.dynamic_terrain_cells, expected_dynamic_terrain);
    assert_eq!(restored.state_hash(), expected_hash);
    let restored_terrain = restored.resolved_terrain.as_ref().unwrap();
    for (&(rx, ry), expected) in &expected_dynamic_terrain {
        assert_eq!(
            crate::map::resolved_terrain::DynamicTerrainCellState::capture(
                restored_terrain.cell(rx, ry).unwrap(),
            ),
            *expected,
        );
    }
}

#[test]
fn gsi_01_05_wave_reselects_live_cell_list_after_fatal_receiver_unmark() {
    let rules = gsi_01_05_damage_rules();
    let mut sim = Simulation::new();
    sim.session.map_width = 16;
    sim.session.map_height = 16;

    let building_id = sim.allocate_stable_id();
    insert_entity(&mut sim, building_id, EntityCategory::Structure);
    let building_type = sim.interner.intern("DUPBLDG");
    {
        let building = sim.substrate.entities.get_mut(building_id).unwrap();
        building.type_ref = building_type;
        building.foundation = "2x1".to_string();
        building.health.current = 10;
    }
    assert!(matches!(
        sim.try_reveal_entity(building_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert!(sim.substrate.occupancy.contains_entity(4, 5, building_id));
    assert!(sim.substrate.occupancy.contains_entity(5, 5, building_id));

    let wave_id = sim.allocate_stable_id();
    let firer_id = sim.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    sim.substrate.entities.get_mut(firer_id).unwrap().type_ref = sim.interner.intern("FIRER");
    sim.substrate
        .entities
        .get_mut(firer_id)
        .unwrap()
        .attack_target = Some(AttackTarget {
        target: TargetKind::Entity(building_id),
        pending_infantry_fire: None,
    });
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Entity(building_id),
        ProjectileCoord::new(4 * 256, 5 * 256, 0),
        ProjectileCoord::new(6 * 256, 5 * 256, 0),
    );
    wave.active_geometry = false;
    wave.decaying = true;
    wave.fade_in = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.fade_out = NativeF64Bits::from_bits(0x3fa9_9999_a000_0000);
    wave.replace_recorded_cells(vec![
        WaveRecordedCell::real(4, 5),
        WaveRecordedCell::real(5, 5),
    ]);
    sim.admit_wave(wave_id, wave);
    sim.set_logic_order_for_test(vec![wave_id, building_id]);
    sim.lifecycle_test_events.clear();

    assert!(sim.object_ai_visit_one(wave_id, Some(&rules), ObjectAiCtx::default()));

    let selected = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::WaveDamageReceiverSelected {
                wave_id: selected_wave,
                target_id,
                ..
            } if *selected_wave == wave_id => Some(*target_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        selected,
        vec![building_id],
        "fatal UnInit from the first cell removes the 2x1 foundation entry before the second recorded cell selects its current occupants"
    );
    let building = sim.substrate.entities.get(building_id).unwrap();
    assert!(!building.lifecycle.object_alive);
    assert!(building.lifecycle.in_limbo);
    assert!(!sim.substrate.occupancy.contains_entity(4, 5, building_id));
    assert!(!sim.substrate.occupancy.contains_entity(5, 5, building_id));
    assert_eq!(sim.substrate.pending_delete, vec![building_id, wave_id]);
    assert!(sim.pending_wave_damage_requests.is_empty());
}

#[test]
fn gsi_05_02_restore_rejects_each_live_modeled_family_missing_from_logic() {
    let mut terrain = Simulation::new();
    let terrain_id = terrain.allocate_stable_id();
    terrain.production.terrain_objects.insert(
        terrain_id,
        TerrainObjectState {
            stable_id: terrain_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref: terrain.interner.intern("TREE01"),
            rx: 1,
            ry: 1,
            health: 1,
            max_health: 1,
            occupation_bits: 0,
            lifecycle: TerrainObjectLifecycle::Live,
        },
    );
    assert_eq!(
        terrain.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
            registry: "TerrainObjectStore",
            object_id: terrain_id,
        })
    );

    let mut projectile = Simulation::new();
    let projectile_id = projectile.allocate_stable_id();
    projectile
        .projectiles
        .spawn(projectile_id, gsi_05_02_projectile(0, None));
    assert_eq!(
        projectile.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
            registry: "ProjectileStore",
            object_id: projectile_id,
        })
    );

    let mut wave = Simulation::new();
    let wave_id = wave.allocate_stable_id();
    wave.waves.spawn(
        wave_id,
        Wave::new(
            3,
            ProjectileCoord::new(0, 0, 0),
            ProjectileCoord::new(1, 0, 0),
        ),
    );
    assert_eq!(
        wave.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
            registry: "WaveStore",
            object_id: wave_id,
        })
    );
}

#[test]
fn lifecycle_authority_alive_queued_object_remains_queued() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.pending_delete.push(1);

    sim.process_pending_delete();
    assert!(sim.substrate.entities.contains(1));
    assert_eq!(sim.substrate.pending_delete, vec![1]);
}

fn attack_fixture(current: MissionType, suspended: MissionId) -> MissionTestFixture {
    MissionTestFixture {
        current: MissionId::from_known(current),
        suspended,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    }
}

/// Two attackers share one target that then leaves play ALIVE â€” sold, captured,
/// mind-controlled or teleported. Nothing dies, so the pointer-expiry broadcast
/// never runs and this sweep is the only thing that releases them.
///
/// Pins the three clauses that make the detach sweep different from the expiry
/// one: the Restore runs before the target clear, the clear is skipped when the
/// Restore replaced the target, and the walk is descending.
#[test]
fn detach_sweep_restores_before_clearing_target_in_descending_id_order() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    insert_entity(&mut sim, 3, EntityCategory::Structure);
    insert_entity(&mut sim, 4, EntityCategory::Unit);

    for attacker in [1, 2] {
        let entity = sim.substrate.entities.get_mut(attacker).unwrap();
        entity.attack_target = Some(AttackTarget::new(3));
        entity.suspended_attack_target = Some(TargetKind::Entity(4));
        entity.navigation.nav_com = None;
        entity.navigation.suspended_nav_com = Some(NavTargetRef::Cell { rx: 7, ry: 8 });
        entity.mission.apply_test_fixture(attack_fixture(
            MissionType::Attack,
            MissionId::from_known(MissionType::Move),
        ));
    }
    sim.lifecycle_test_events.clear();

    sim.stop_all_targeting_on_detach(3, None);

    // The detaching object is untouched and still present: this is not removal.
    assert!(sim.substrate.entities.contains(3));

    for attacker in [1, 2] {
        let entity = sim.substrate.entities.get(attacker).unwrap();
        assert_eq!(
            entity.mission.current(),
            MissionId::from_known(MissionType::Move),
            "attacker {attacker} restored its suspended mission"
        );
        assert_eq!(entity.mission.suspended(), MissionId::NONE);
        // Restore-then-clear: the archived target is reinstalled and the
        // null-out is skipped because it no longer matches the detaching object.
        assert_eq!(
            entity.attack_target.as_ref().map(|target| target.target),
            Some(TargetKind::Entity(4)),
            "attacker {attacker} kept the target the Restore installed"
        );
        assert_eq!(
            entity.navigation.nav_com,
            Some(NavTargetRef::Cell { rx: 7, ry: 8 }),
            "attacker {attacker} got its destination back"
        );
    }

    let visits: Vec<(u64, bool, bool)> = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::DetachTargetingSweepVisited {
                detach_id: 3,
                listener_id,
                restored,
                target_cleared,
            } => Some((*listener_id, *restored, *target_cleared)),
            _ => None,
        })
        .collect();
    assert_eq!(
        visits,
        vec![(2, true, false), (1, true, false)],
        "the sweep walks descending stable-ID order"
    );
}

/// The other half of the sweep: an attacker with no suspended mission. The
/// Restore writes nothing, so the conditional null-out is the clause that fires
/// and the attacker is left with no target â€” which is what hands it to the
/// Attack handler's idle exit on its next dispatch.
#[test]
fn detach_sweep_clears_target_when_no_mission_was_suspended() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Structure);

    let attacker = sim.substrate.entities.get_mut(1).unwrap();
    attacker.attack_target = Some(AttackTarget::new(2));
    attacker.passively_acquired_target = true;
    attacker
        .mission
        .apply_test_fixture(attack_fixture(MissionType::Attack, MissionId::NONE));
    sim.lifecycle_test_events.clear();

    sim.stop_all_targeting_on_detach(2, None);

    let attacker = sim.substrate.entities.get(1).unwrap();
    assert!(attacker.attack_target.is_none());
    assert!(!attacker.passively_acquired_target);
    assert_eq!(
        attacker.mission.current(),
        MissionId::from_known(MissionType::Attack),
        "a Restore with nothing suspended writes no mission field"
    );
    assert_eq!(attacker.mission.suspended(), MissionId::NONE);
    assert!(matches!(
        sim.lifecycle_test_events.as_slice(),
        [LifecycleTestEvent::DetachTargetingSweepVisited {
            detach_id: 2,
            listener_id: 1,
            restored: false,
            target_cleared: true,
        }]
    ));
}

/// A cell target is not the departing Techno pointer, so the live-object detach
/// sweep does not match it. CellClass pointer expiry has its own forward,
/// clear-before-Restore transaction and is tested with wall removal.
#[test]
fn detach_sweep_never_matches_a_cell_target() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Structure);

    let attacker = sim.substrate.entities.get_mut(1).unwrap();
    attacker.attack_target = Some(AttackTarget::for_cell(9, 11));
    attacker.mission.apply_test_fixture(attack_fixture(
        MissionType::Attack,
        MissionId::from_known(MissionType::Move),
    ));

    sim.stop_all_targeting_on_detach(2, None);

    let attacker = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        attacker.attack_target.as_ref().map(|target| target.target),
        Some(TargetKind::Cell(9, 11))
    );
    assert_eq!(
        attacker.mission.current(),
        MissionId::from_known(MissionType::Attack),
        "a cell-targeted object is never restored by the detach sweep"
    );
}

#[test]
fn bridge_cell_success_restores_before_conditional_target_clear() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let attacker = sim.substrate.entities.get_mut(1).unwrap();
    attacker.attack_target = Some(AttackTarget::for_cell(9, 11));
    attacker.suspended_attack_target = Some(TargetKind::Entity(2));
    attacker.navigation.suspended_nav_com = Some(NavTargetRef::Cell { rx: 7, ry: 8 });
    attacker.mission.apply_test_fixture(attack_fixture(
        MissionType::Attack,
        MissionId::from_known(MissionType::Move),
    ));
    sim.stop_all_targeting_cell(9, 11, None);
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        attacker.attack_target.as_ref().map(|target| target.target),
        Some(TargetKind::Entity(2))
    );
    assert_eq!(
        attacker.navigation.nav_com,
        Some(NavTargetRef::Cell { rx: 7, ry: 8 })
    );
    assert_eq!(
        attacker.mission.current(),
        MissionId::from_known(MissionType::Move)
    );
}

/// The whole loop end to end, in the shape the deferred item names: an
/// infantryman is overridden onto a blocker, the blocker LEAVES ALIVE (owner
/// change â€” engineer capture, Yuri, Psychic Beacon), and the infantryman comes
/// back to what it was doing instead of sitting on Attack forever.
#[test]
fn overridden_attacker_is_released_when_its_blocker_leaves_alive() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let soviets = sim.interner.intern("Soviets");
    sim.substrate.entities.get_mut(2).unwrap().owner = soviets;

    let mover = sim.substrate.entities.get_mut(1).unwrap();
    mover.navigation.nav_com = Some(NavTargetRef::Cell { rx: 20, ry: 21 });
    mover
        .mission
        .apply_test_fixture(attack_fixture(MissionType::Move, MissionId::NONE));

    assert!(sim.mission_override_blocked_by_object(1, 2));

    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        mover.mission.current(),
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        mover.mission.suspended(),
        MissionId::from_known(MissionType::Move)
    );
    assert_eq!(
        mover.attack_target.as_ref().map(|target| target.target),
        Some(TargetKind::Entity(2))
    );
    assert!(mover.navigation.nav_com.is_none(), "the mover stops");
    assert_eq!(
        mover.navigation.suspended_nav_com,
        Some(NavTargetRef::Cell { rx: 20, ry: 21 })
    );

    // The blocker changes hands and stays alive.
    let americans = sim.interner.intern("Americans");
    sim.change_owner(2, americans);

    assert!(sim.substrate.entities.contains(2), "the blocker is alive");
    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        mover.mission.current(),
        MissionId::from_known(MissionType::Move),
        "the mover goes back to its original order"
    );
    assert_eq!(mover.mission.suspended(), MissionId::NONE);
    assert_eq!(
        mover.navigation.nav_com,
        Some(NavTargetRef::Cell { rx: 20, ry: 21 }),
        "and re-paths to the destination it was archived with"
    );
    assert!(
        mover.attack_target.is_none(),
        "nothing was archived, so the conditional null-out fires"
    );
}

/// The ally arm: the blocked-step Override does not fire on a friendly blocker,
/// and it writes none of the five fields.
#[test]
fn blocked_step_override_does_not_fire_on_an_allied_blocker() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);

    let mover = sim.substrate.entities.get_mut(1).unwrap();
    mover.navigation.nav_com = Some(NavTargetRef::Cell { rx: 20, ry: 21 });
    mover
        .mission
        .apply_test_fixture(attack_fixture(MissionType::Move, MissionId::NONE));

    // Same house: always an ally.
    assert!(!sim.mission_override_blocked_by_object(1, 2));

    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        mover.mission.current(),
        MissionId::from_known(MissionType::Move)
    );
    assert_eq!(mover.mission.suspended(), MissionId::NONE);
    assert!(mover.attack_target.is_none());
    assert_eq!(
        mover.navigation.nav_com,
        Some(NavTargetRef::Cell { rx: 20, ry: 21 })
    );
    assert!(mover.navigation.suspended_nav_com.is_none());
}

/// A retask empties the Override archives but leaves the archived SELECTOR, and
/// only the MEGAMISSION event does it — Stop is its own opcode and clears
/// neither archive.
#[test]
fn retasking_clears_the_override_archives_but_not_the_archived_selector() {
    use crate::sim::mission::retask::DockTeardown;

    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let soviets = sim.interner.intern("Soviets");
    sim.substrate.entities.get_mut(2).unwrap().owner = soviets;
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mission
        .apply_test_fixture(attack_fixture(MissionType::Move, MissionId::NONE));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .navigation
        .nav_com = Some(crate::sim::components::NavTargetRef::cell(7, 7));

    // Override archives the selector, the target and the destination.
    assert!(sim.mission_override_blocked_by_object(1, 2));
    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        mover.mission.suspended(),
        MissionId::from_known(MissionType::Move)
    );
    assert!(mover.navigation.suspended_nav_com.is_some());
    // Plant an archived target too, so both halves of the clear are live rather
    // than trivially already-`None`.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .suspended_attack_target = Some(TargetKind::Entity(2));

    // A player order retasks it. EventClass__Execute empties both archives
    // right after its Queue_Mission, and deliberately leaves the archived
    // SELECTOR alone — so a later Restore hands back the old mission with a
    // null target and a null destination.
    // Stop is NOT a MEGAMISSION: StopCommandClass::Execute pushes opcode 6 at
    // 0x00730EE7. Case 6 does queue one mission — the ore-miner Guard at
    // 0x004C7685 — but stores to neither archive anywhere in
    // 0x004C74CB-0x004C76BB, so both must survive it.
    sim.queue_mission_with_teardown(1, MissionType::Stop, DockTeardown::All);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .navigation
            .suspended_nav_com
            .is_some(),
        "Stop is its own opcode and clears no archive"
    );

    sim.queue_megamission_with_teardown(1, MissionType::Move, DockTeardown::All, None);
    let mover = sim.substrate.entities.get(1).unwrap();
    assert!(
        mover.suspended_attack_target.is_none(),
        "SuspendedTarCom is cleared on every path"
    );
    assert!(
        mover.navigation.suspended_nav_com.is_none(),
        "SuspendedNavCom is cleared on the Foot arm"
    );
    assert_eq!(
        mover.mission.suspended(),
        MissionId::from_known(MissionType::Move),
        "the archived selector itself survives"
    );
}

/// Two blocked steps against two different blockers with no Restore between
/// them: the second Override archives the FIRST override's mission, so a later
/// Restore lands the object back on Attack and the original order is lost.
/// Native, and it must survive the wiring — no caller-side clobber guard.
#[test]
fn second_blocked_step_override_clobbers_the_archived_mission() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    insert_entity(&mut sim, 3, EntityCategory::Unit);
    let soviets = sim.interner.intern("Soviets");
    for blocker in [2, 3] {
        sim.substrate.entities.get_mut(blocker).unwrap().owner = soviets;
    }
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mission
        .apply_test_fixture(attack_fixture(MissionType::Move, MissionId::NONE));

    assert!(sim.mission_override_blocked_by_object(1, 2));
    assert!(sim.mission_override_blocked_by_object(1, 3));

    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        mover.mission.suspended(),
        MissionId::from_known(MissionType::Attack),
        "the second Override archived the first Override's mission"
    );
    assert_eq!(
        mover.attack_target.as_ref().map(|target| target.target),
        Some(TargetKind::Entity(3))
    );
}

fn insert_naval_build_const_fixture(
    sim: &mut Simulation,
    stable_id: u64,
    owner: crate::sim::intern::InternedId,
    rx: u16,
    ry: u16,
) {
    let type_ref = sim.interner.intern("GACNST");
    let mut building = GameEntity::new_at_frame_zero_for_test(
        stable_id,
        rx,
        ry,
        0,
        0,
        owner,
        Health { current: 1000 },
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    building.foundation = "1x1".to_string();
    building.build_const_eligible = true;
    sim.substrate.entities.insert(building);
}

#[test]
fn naval_build_const_reveal_conceal_and_reentry_preserve_acquisition_order() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
    insert_naval_build_const_fixture(&mut sim, 20, owner, 2, 2);
    insert_naval_build_const_fixture(&mut sim, 10, owner, 4, 2);
    insert_naval_build_const_fixture(&mut sim, 30, owner, 6, 2);

    assert!(matches!(
        sim.try_reveal_entity(20, request(2, 2, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    assert!(matches!(
        sim.try_reveal_entity(10, request(4, 2, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(
        sim.houses[&owner].build_const_order,
        vec![20, 10],
        "acquisition order differs from stable-ID order"
    );

    assert_eq!(
        sim.try_reveal_entity(20, request(2, 2, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::AlreadyRevealed
    );
    assert_eq!(sim.houses[&owner].build_const_order, vec![20, 10]);
    assert_eq!(
        sim.try_reveal_entity(30, request(6, 2, PlacementEvidence::MarkFailed)),
        RevealOutcome::Failed(RevealFailure::MarkFailed)
    );
    assert_eq!(sim.houses[&owner].build_const_order, vec![20, 10]);

    // A native DynamicVector stable-removes one matching pointer and shifts
    // one tail; this intentionally corrupt duplicate proves it is not retain.
    sim.houses.get_mut(&owner).unwrap().build_const_order = vec![20, 10, 20];
    sim.remove_build_const_from_owner(20);
    assert_eq!(sim.houses[&owner].build_const_order, vec![10, 20]);
    sim.houses.get_mut(&owner).unwrap().build_const_order = vec![20, 10];

    assert_eq!(sim.conceal(20), super::ConcealOutcome::Concealed);
    assert_eq!(sim.houses[&owner].build_const_order, vec![10]);

    // Already-limbo returns before the House expiry callback, so even an
    // intentionally stale fixture entry is untouched by this no-op path.
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .build_const_order
        .push(20);
    assert_eq!(sim.conceal(20), super::ConcealOutcome::AlreadyConcealed);
    assert_eq!(sim.houses[&owner].build_const_order, vec![10, 20]);
    sim.houses.get_mut(&owner).unwrap().build_const_order.pop();

    assert!(matches!(
        sim.try_reveal_entity(20, request(8, 2, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(
        sim.houses[&owner].build_const_order,
        vec![10, 20],
        "successful re-entry appends at the live tail"
    );
}

#[test]
fn naval_build_const_capture_moves_old_entry_to_new_owner_tail() {
    let mut sim = Simulation::new();
    let old_owner = sim.interner.intern("Americans");
    let new_owner = sim.interner.intern("Russians");
    sim.houses
        .insert(old_owner, HouseState::new(old_owner, 0, None, true, 0, 10));
    sim.houses
        .insert(new_owner, HouseState::new(new_owner, 1, None, false, 0, 10));
    insert_naval_build_const_fixture(&mut sim, 50, new_owner, 8, 8);
    insert_naval_build_const_fixture(&mut sim, 40, old_owner, 2, 2);
    insert_naval_build_const_fixture(&mut sim, 12, old_owner, 4, 2);
    for (stable_id, rx, ry) in [(50, 8, 8), (40, 2, 2), (12, 4, 2)] {
        assert!(matches!(
            sim.try_reveal_entity(stable_id, request(rx, ry, PlacementEvidence::MarkSucceeded)),
            RevealOutcome::Revealed { .. }
        ));
    }
    assert_eq!(sim.houses[&old_owner].build_const_order, vec![40, 12]);
    assert_eq!(sim.houses[&new_owner].build_const_order, vec![50]);

    sim.change_owner(40, new_owner);

    assert_eq!(sim.houses[&old_owner].build_const_order, vec![12]);
    assert_eq!(sim.houses[&new_owner].build_const_order, vec![50, 40]);
    assert_eq!(sim.substrate.entities.get(40).unwrap().owner, new_owner);
}

#[test]
fn infantry_raw_owner_remains_the_marking_house_after_live_owner_change() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    let old = sim.substrate.entities.get(1).unwrap().owner();
    sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 192, 64));
    let changed = sim.interner.intern("Russians");
    sim.change_owner(1, changed);
    assert_eq!(sim.substrate.entities.get(1).unwrap().owner(), changed);
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(3, 4),
        Some(old)
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 4);
}

#[test]
fn walk_first_limbo_releases_head_but_repeated_limbo_preserves_new_claim() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 192, 64));
    let head = crate::sim::components::DriveCoord {
        x: 4 * 256 + 192,
        y: 4 * 256 + 64,
        z: 0,
    };
    let owner = sim.substrate.entities.get(1).unwrap().owner();
    let e = sim.substrate.entities.get_mut(1).unwrap();
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    e.locomotor.as_mut().unwrap().set_step_head(Some(head));
    crate::sim::movement::walk_head::raw_at(
        &mut sim.substrate.raw_cell_occupation,
        owner,
        head,
        true,
        None,
        None,
    );
    sim.techno_limbo(1);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(4, 4) & 0x1c,
        0
    );
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(4, 4),
        None
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .step_head(),
        Some(head)
    );
    let other = sim.interner.intern("Russians");
    crate::sim::movement::walk_head::raw_at(
        &mut sim.substrate.raw_cell_occupation,
        other,
        head,
        true,
        None,
        None,
    );
    sim.techno_limbo(1);
    sim.uninit(1);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(4, 4) & 0x1c,
        4
    );
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(4, 4),
        Some(other)
    );
}

#[test]
fn production_air_wrapper_keeps_fly_exact_producer_and_reads_live_dummy_for_legacy() {
    for exact in [None, Some(731)] {
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(1, 1, Vec::new()));
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .test_set_dummy_cell_level_slope(3, 1);
        install_fly_aircraft(&mut sim, 1, SimFixed::from_num(125));
        let e = sim.substrate.entities.get_mut(1).unwrap();
        e.position.sub_x = SimFixed::from_num(64);
        e.position.sub_y = SimFixed::from_num(192);
        e.position.z = 99;
        e.position.exact_z_leptons = exact;
        e.on_bridge = true;
        e.locomotor.as_mut().unwrap().set_fly_target_height(125);
        let xy = crate::sim::movement::ground_pose::position_world_xy(&e.position);
        let ground = crate::util::lepton::ground_height_leptons(3, 1, xy[0], xy[1]).unwrap();
        let expected = exact.unwrap_or(ground + 416 + 125);
        // Keep the physical height steady despite the stale controller cache.
        e.locomotor
            .as_mut()
            .unwrap()
            .set_fly_target_height((SimFixed::from_num(expected - ground - 416)).to_num::<i32>());
        sim.tick_air_movement_with_cell_lists_one(1, None);
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(e.position.exact_z_leptons, Some(expected));
        assert_eq!(sim.foot_navigation_coordinate(1).unwrap().z, expected);
        let dummy = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .shared_cell_dummy()
            .snapshot();
        assert_eq!(dummy.level, 3);
        assert_eq!(dummy.slope_type, 1);
    }
}

#[test]
fn production_air_wrapper_retains_native_jumpjet_result_even_when_height_cache_clamps() {
    fn fixture() -> Simulation {
        let mut sim = Simulation::new();
        install_common_raw_terrain(&mut sim, 8, 8, 2, None);
        insert_entity(&mut sim, 1, EntityCategory::Unit);
        let e = sim.substrate.entities.get_mut(1).unwrap();
        e.position.exact_z_leptons = Some(-37);
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Jumpjet);
        loco.balloon_hover = true;
        loco.altitude = SimFixed::from_num(500);
        let runtime = loco.jumpjet_runtime_mut().unwrap();
        runtime.phase = crate::sim::movement::jumpjet_flight::STATE_HOLD;
        runtime.moving = false;
        e.locomotor = Some(loco);
        sim
    }
    let mut direct = fixture();
    assert!(direct.tick_jumpjet_cruise_one(1, None).is_some());
    let expected = direct
        .substrate
        .entities
        .get(1)
        .unwrap()
        .position
        .exact_z_leptons;
    assert_eq!(
        expected,
        Some(-37),
        "stationary native hold retains raw owner Z"
    );
    assert_eq!(
        direct
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .altitude,
        SimFixed::from_num(0)
    );
    let mut wrapped = fixture();
    wrapped.tick_air_movement_with_cell_lists_one(1, None);
    assert_eq!(
        wrapped
            .substrate
            .entities
            .get(1)
            .unwrap()
            .position
            .exact_z_leptons,
        expected
    );
    assert_eq!(wrapped.foot_navigation_coordinate(1).unwrap().z, -37);
}

#[test]
fn fly_cross_level_move_lands_on_destination_surface_after_restore() {
    use crate::util::fixed_math::SIM_ONE;

    // Fly4CDD07/4CDD1A: XY integration precedes physical-height feedback.
    // Rates remain the existing fixed-point adapter policy.
    for (origin_level, destination_level) in [(0, 2), (2, 0)] {
        let mut sim = Simulation::with_seed(0);
        assert_eq!(sim.allocate_stable_id(), 1);
        install_common_raw_terrain(&mut sim, 8, 8, origin_level, None);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(2, 2)
            .unwrap()
            .level = destination_level;
        install_fly_aircraft(&mut sim, 1, SimFixed::from_num(600));
        assert!(matches!(
            sim.try_reveal_entity(1, common_raw_request(2, 3, origin_level, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));
        let origin_z = i32::from(origin_level) * 104 + 600;
        let destination_ground = i32::from(destination_level) * 104;
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.position.exact_z_leptons = Some(origin_z);
        entity.facing = 0;
        let loco = entity.locomotor.as_mut().unwrap();
        loco.altitude = SimFixed::from_num(600);
        loco.set_fly_target_height(600);

        loco.fly_current_speed = SIM_ONE;
        loco.speed_fraction = SIM_ONE;
        loco.rot = 0;
        assert!(sim.issue_air_cell_destination(1, (2, 2), SimFixed::from_num(3840), None,));
        sim.tick_air_movement_with_cell_lists_one(1, None);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        assert_eq!((entity.position.rx, entity.position.ry), (2, 2));
        let moved_z = entity.position.exact_z_leptons.unwrap();
        // Crossing itself does not add the ground delta. Only one bounded
        // vertical rate step adjusts the physical coordinate.
        assert!((moved_z - origin_z).abs() <= 21);
        assert_ne!(moved_z, origin_z);
        assert_eq!(
            entity.locomotor.as_ref().unwrap().altitude.to_num::<i32>(),
            moved_z - destination_ground,
        );
        entity.movement_target = None;
        let loco = entity.locomotor.as_mut().unwrap();
        loco.begin_fly_landing();
        loco.set_fly_target_height(0);

        let map_terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        // In-scenario load reconstructs Scenario RNG from Seed0.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = GameSnapshot::save(&sim, 0, 0, "Fly terrain landing", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.resolved_terrain = Some(map_terrain);
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), sim.state_hash());
        let mut landed = false;
        for frame in 1..80 {
            for instance in [&mut sim, &mut restored] {
                instance.session.tick = frame;
                instance.session.binary_frame = frame as u32;
                instance.tick_air_movement_with_cell_lists_one(1, None);
            }
            assert_eq!(restored.state_hash(), sim.state_hash());
            let entity = sim.substrate.entities.get(1).unwrap();
            if entity.locomotor.as_ref().unwrap().air_phase() == AirMovePhase::Landed {
                assert_eq!(entity.position.exact_z_leptons, Some(destination_ground));
                assert_eq!(
                    sim.foot_navigation_coordinate(1).unwrap().z,
                    destination_ground
                );
                landed = true;
                break;
            }
        }
        assert!(landed, "Fly must finish its actual descent");
    }
}

#[test]
fn display_lifecycle_is_independent_of_logic_and_survives_production_save() {
    use super::display_layers::DisplayLayer;
    let mut sim = Simulation::new();
    for _ in 0..4 {
        let id = sim.allocate_stable_id();
        insert_entity(&mut sim, id, EntityCategory::Unit);
        let mut req = common_raw_request(2 + id as u16, 3, 0, 128, 128);
        req.logic_eligible = id != 2;
        assert!(matches!(
            sim.try_reveal_entity(id, req),
            RevealOutcome::Revealed { .. }
        ));
    }
    assert_eq!(sim.substrate.logic.as_slice(), [1, 3, 4]);
    assert_eq!(
        sim.substrate.display.members(DisplayLayer::GROUND),
        [1, 2, 3, 4]
    );
    // Coordinates change without resubmitting. MainTick's single adjacent
    // pass has a different result from either Logic order or a full sort.
    for id in 1..=4 {
        sim.substrate.entities.get_mut(id).unwrap().position.rx = 7 - id as u16;
    }
    let before_sort = sim.state_hash();
    let before_sort_without_display =
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(182));
    sim.sort_display_ground(None);
    assert_ne!(sim.state_hash(), before_sort);
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(182)),
        before_sort_without_display
    );
    assert_eq!(
        sim.substrate.display.members(DisplayLayer::GROUND),
        [2, 3, 4, 1]
    );
    let bytes = GameSnapshot::save(&sim, 0, 0, "display-order", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored.substrate.display.members(DisplayLayer::GROUND),
        [2, 3, 4, 1]
    );
    restored.sort_display_ground(None);
    assert_eq!(
        restored.substrate.display.members(DisplayLayer::GROUND),
        [3, 4, 2, 1]
    );
    let hash = restored.state_hash();
    restored.object_conceal(2);
    assert_eq!(
        restored.substrate.display.members(DisplayLayer::GROUND),
        [3, 4, 1]
    );
    assert_eq!(restored.substrate.logic.as_slice(), [1, 3, 4]);
    assert_ne!(hash, restored.state_hash());
    restored
        .substrate
        .display
        .submit(999, Some(DisplayLayer::TOP), |_| 0);
    assert!(matches!(
        restored.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::MissingDisplayIdentity { object_id: 999 })
    ));
}

#[test]
fn mixed_display_lifecycle_matches_original_sequences_and_save_restore() {
    use super::display_layers::DisplayLayer;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::voxel_anim_type::{VoxelAnimType, VoxelAnimTypeId};
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/display_non_entity.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 9);
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=TEST\n[TEST]\nStrength=100\nTurretAnimIsVoxel=yes\n",
    ))
    .unwrap();
    let voxel_type = VoxelAnimType::from_ini_section(
        "TIRE",
        IniFile::from_str("[TIRE]\nDuration=150\n")
            .section("TIRE")
            .unwrap(),
    );
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let actors = input["actors"].as_array().unwrap();
        let mut sim = Simulation::new();
        // Native Scenario Load reseeds Random(0). These display operations
        // consume no Scenario draws, so the complete saved hash can match.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        for (index, actor) in actors.iter().enumerate() {
            let id = sim.allocate_stable_id();
            assert_eq!(id, index as u64 + 1);
            let xyz: [i32; 3] = std::array::from_fn(|i| actor["xyz"][i].as_i64().unwrap() as i32);
            match actor["kind"].as_str().unwrap() {
                "unit" | "building" => {
                    let category = if actor["kind"] == "unit" {
                        EntityCategory::Unit
                    } else {
                        EntityCategory::Structure
                    };
                    insert_entity(&mut sim, id, category);
                    let entity = sim.substrate.entities.get_mut(id).unwrap();
                    entity.position.rx = (xyz[0] / 256) as u16;
                    entity.position.ry = (xyz[1] / 256) as u16;
                    entity.position.sub_x = SimFixed::from_num(xyz[0] % 256);
                    entity.position.sub_y = SimFixed::from_num(xyz[1] % 256);
                    entity.position.exact_z_leptons = Some(xyz[2]);
                }
                "particle" => {
                    insert_particle_system(&mut sim, id);
                    sim.substrate.particle_systems.get_mut(id).unwrap().coords =
                        IVec3::from_array(xyz);
                }
                "terrain" => {
                    let terrain = TerrainObjectState {
                        stable_id: id,
                        native_unique_id: None,
                        in_logic_vector: false,
                        type_ref: sim.interner.intern("TREE"),
                        rx: (xyz[0] / 256) as u16,
                        ry: (xyz[1] / 256) as u16,
                        health: 10,
                        max_health: 10,
                        occupation_bits: 0,
                        lifecycle: TerrainObjectLifecycle::Live,
                    };
                    sim.production
                        .terrain_object_cells
                        .insert(terrain.cell(), id);
                    sim.production.terrain_objects.insert(id, terrain);
                }
                "bullet" => {
                    let mut shot = gsi_05_02_projectile(0, None);
                    shot.flat = actor["flat"].as_bool().unwrap();
                    shot.origin = ProjectileCoord::new(xyz[0], xyz[1], xyz[2]);
                    sim.admit_projectile(id, shot);
                }
                "wave" => {
                    sim.admit_wave(
                        id,
                        Wave::new(
                            3,
                            ProjectileCoord::new(xyz[0], xyz[1], xyz[2]),
                            ProjectileCoord::new(xyz[0] + 256, xyz[1], xyz[2]),
                        ),
                    );
                }
                "voxel" => {
                    let debris = crate::sim::voxel_anim::spawn_debris_piece(
                        id,
                        VoxelAnimTypeId(0),
                        &voxel_type,
                        None,
                        IVec3::from_array(xyz),
                        &mut crate::sim::rng::SimRng::new(1),
                    )
                    .unwrap();
                    sim.substrate.voxel_anims.insert(debris);
                }
                kind => panic!("unexpected {kind}"),
            }
        }
        for (step, expected) in input["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["after_steps"].as_array().unwrap())
        {
            let id = step["actor"].as_u64().map_or(0, |i| i + 1);
            match step["op"].as_str().unwrap() {
                "submit" => match actors[id as usize - 1]["kind"].as_str().unwrap() {
                    "unit" | "building" => sim.submit_entity_display(id, Some(&rules), None),
                    "particle" => {
                        sim.reveal_particle_system(id, Some(&rules));
                    }
                    "terrain" => {
                        sim.register_terrain_object(id, Some(&rules));
                    }
                    "bullet" => {
                        sim.register_projectile(
                            id,
                            actors[id as usize - 1]["flat"].as_bool().unwrap(),
                        );
                    }
                    "wave" => {
                        sim.register_wave(id);
                    }
                    "voxel" => {
                        sim.reveal_voxel_anim(id);
                    }
                    _ => unreachable!(),
                },
                "remove" => {
                    sim.substrate.display.remove(id);
                }
                "sort" => sim.sort_display_ground(Some(&rules)),
                "coordinates" => {
                    let xyz: [i32; 3] =
                        std::array::from_fn(|i| step["xyz"][i].as_i64().unwrap() as i32);
                    if let Some(entity) = sim.substrate.entities.get_mut(id) {
                        entity.position.rx = (xyz[0] / 256) as u16;
                        entity.position.ry = (xyz[1] / 256) as u16;
                        entity.position.sub_x = SimFixed::from_num(xyz[0] % 256);
                        entity.position.sub_y = SimFixed::from_num(xyz[1] % 256);
                    } else {
                        sim.substrate.particle_systems.get_mut(id).unwrap().coords =
                            IVec3::from_array(xyz);
                    }
                }
                _ => unreachable!(),
            }
            for layer in 0..5 {
                let expected: Vec<u64> = expected[layer]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| i.as_u64().unwrap() + 1)
                    .collect();
                assert_eq!(
                    sim.substrate
                        .display
                        .members(DisplayLayer::from_index(layer as u8).unwrap()),
                    expected,
                    "{name}: {step}"
                );
            }
        }
        let bytes = GameSnapshot::save(&sim, 0, 0, "mixed-display", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), sim.state_hash(), "{name}");
        sim.sort_display_ground(Some(&rules));
        restored.sort_display_ground(Some(&rules));
        assert_eq!(restored.state_hash(), sim.state_hash(), "{name}: next sort");
        for id in 1..=actors.len() as u64 {
            if restored.substrate.entities.contains(id) {
                // Corpus entities were submitted directly, independently of
                // Reveal/Logic; their production Conceal is tested separately.
                restored.substrate.display.remove(id);
            } else {
                restored.unregister_non_entity_object(id);
            }
            assert_eq!(restored.substrate.display.layer_of(id), None);
        }
    }
}

#[test]
fn jumpjet_process_compares_live_layer_queries_not_cached_registration() {
    use super::display_layers::DisplayLayer;
    let mut sim = Simulation::new();
    let id = sim.allocate_stable_id();
    insert_entity(&mut sim, id, EntityCategory::Unit);
    sim.substrate.entities.get_mut(id).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));
    sim.try_reveal_entity(id, common_raw_request(3, 3, 0, 128, 128));
    // Explicit stale display cache witness. Native compares its two +74
    // queries and leaves this history alone while the Process answer is stable.
    sim.substrate
        .display
        .submit(id, Some(DisplayLayer::TOP), |_| 0);
    sim.tick_air_movement_with_cell_lists_one(id, None);
    assert_eq!(sim.substrate.display.layer_of(id), Some(DisplayLayer::TOP));

    // A real changed query re-submits even if cached membership is absent.
    sim.substrate.display.remove(id);
    let before = sim.entity_display_layer(id, None).unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .altitude = SimFixed::from_num(600);
    sim.complete_jumpjet_display_process(id, before, None);
    assert_eq!(sim.substrate.display.layer_of(id), Some(DisplayLayer::TOP));
    // The native tail's alive gate prevents another submission after death.
    let before = sim.entity_display_layer(id, None).unwrap();
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.lifecycle.object_alive = false;
    entity.locomotor.as_mut().unwrap().altitude = SimFixed::ZERO;
    sim.complete_jumpjet_display_process(id, before, None);
    assert_eq!(sim.substrate.display.layer_of(id), Some(DisplayLayer::TOP));
}

/// Fly's phase tail (4CD4DE) resubmits Display after callbacks that may have
/// UnInit an owner (landing retry's C4 receiver). Mark5F5850 refuses the
/// Limbo owner; store removal expires the Display registration so the next
/// Ground sort cannot meet a removed identity.
#[test]
fn display_registration_expires_with_a_resubmitted_uninit_owner() {
    use super::display_layers::DisplayLayer;
    let mut sim = Simulation::new();
    let id = sim.allocate_stable_id();
    insert_entity(&mut sim, id, EntityCategory::Unit);
    sim.try_reveal_entity(id, common_raw_request(3, 3, 0, 128, 128));
    sim.uninit(id);
    assert!(sim.substrate.entities.get(id).unwrap().lifecycle.in_limbo);
    sim.submit_entity_display(id, None, None);
    assert_eq!(
        sim.substrate.display.layer_of(id),
        Some(DisplayLayer::GROUND)
    );
    sim.process_pending_delete();
    assert!(!sim.substrate.entities.contains(id));
    assert_eq!(sim.substrate.display.layer_of(id), None);
    sim.sort_display_ground(None);
}
