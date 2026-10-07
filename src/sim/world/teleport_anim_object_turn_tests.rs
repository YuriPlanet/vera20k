//! The teleport locomotor's `[General] WarpOut=` animations are real
//! `AnimClass` instances constructed inside the mover's own object turn.

use super::*;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
use crate::sim::world::common_raw_test_terrain_cell;

fn rules(bind_warp_art: bool) -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nWarpOut=WARPOUT\n\n[InfantryTypes]\n0=CLEG\n\n\
         [VehicleTypes]\n0=CMON\n\n\
         [CLEG]\nStrength=100\nSpeed=4\nSensorsSight=1\nSpeedType=Foot\nMovementZone=Infantry\n\n\
         [CMON]\nStrength=100\nSpeed=4\nSensorsSight=1\n\n[Clear]\nFoot=100%\n",
    ))
    .unwrap();
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(
        "[WARPOUT]\nFlat=yes\nTranslucent=yes\nRate=120\n",
    ));
    if bind_warp_art {
        art.bind_anim_frame_count_for_test("WARPOUT", 13);
    }
    rules.replace_art_registry_for_test(art);
    rules
}

/// Supply the already-revealed fixture's map-dependent Unlimbo producers
///before invoking its first native Cell destination. Keep the physical Z2.
fn install_destination_cells(sim: &mut Simulation, rules: &RuleSet) {
    let speed_costs = rules
        .terrain_rules
        .semantics_by_name("Clear")
        .unwrap()
        .speed_costs;
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
        32,
        32,
        (0..32)
            .flat_map(|y| {
                (0..32).map(move |x| crate::map::resolved_terrain::ResolvedTerrainCell {
                    speed_costs,
                    base_speed_costs: speed_costs,
                    ..common_raw_test_terrain_cell(x, y, 2, false)
                })
            })
            .collect(),
    ));
    sim.playfield_bounds = Some(
        crate::map::playfield::PlayfieldBounds::from_normalized_local_size(32, -32, -32, 64, 64),
    );
    sim.playfield_size_height = Some(32);
    // Reveal preceded map installation in relocating_owner. Finish the same
    // Techno threat/Foot neighbor producers as the existing object-turn fixture.
    sim.spatial_threat_after_unlimbo(1, rules, None);
    sim.foot_neighbors_after_unlimbo(1, Some(rules));
}

/// An infantryman on Teleport with a warp armed from cell 5,5.
fn relocating_legionnaire(target: (u16, u16)) -> Simulation {
    relocating_owner(EntityCategory::Infantry, "CLEG", target)
}

/// A Unit on Teleport without `Teleporter=` (the retail CMON and SMON) with a
/// warp armed from cell 5,5.
fn relocating_chrono_unit(target: (u16, u16)) -> Simulation {
    relocating_owner(EntityCategory::Unit, "CMON", target)
}

fn relocating_owner(category: EntityCategory, type_name: &str, target: (u16, u16)) -> Simulation {
    let mut sim = Simulation::with_seed(0);
    sim.fog.width = 32;
    sim.fog.height = 32;
    // Construct the actual class. Reclassifying a Unit test default leaves
    // Infantry's required fear/action runtime absent from its real AI turn.
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        5,
        5,
        2,
        0,
        sim.intern("Americans"),
        crate::sim::components::Health { current: 100 },
        sim.intern(type_name),
        category,
        0,
        5,
        category == EntityCategory::Unit,
    );
    // Preserve this fixture's original centre pose: the target coordinate is
    // also the centre. An Infantry constructor starts in slot2, so name the
    // supplied centre premise rather than accidentally changing the old test.
    if category == EntityCategory::Infantry {
        entity.position.sub_x = crate::util::lepton::CELL_CENTER_LEPTON;
        entity.position.sub_y = crate::util::lepton::CELL_CENTER_LEPTON;
        entity.sub_cell = Some(0);
    }
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
    entity.install_teleport_state_for_test(Some(TeleportState::for_test(
        TeleportPhase::Relocate,
        target.0,
        target.1,
        0,
    )));
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = 2;
    assert!(matches!(
        sim.reveal(1),
        super::super::RevealOutcome::Revealed { .. }
    ));
    sim
}

#[test]
fn relocation_constructs_departure_and_arrival_warp_anims_in_the_mover_turn() {
    let rules = rules(true);
    let mut sim = relocating_legionnaire((8, 9));
    let warp_out = sim.intern("WARPOUT");

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    let anims: Vec<_> = sim
        .substrate
        .anims
        .iter()
        .map(|(_, anim)| anim)
        .filter(|anim| anim.type_id == warp_out)
        .collect();
    assert_eq!(anims.len(), 2, "one at the origin, one at the destination");
    let mut cells: Vec<_> = anims
        .iter()
        .map(|anim| {
            let (rx, ry, _, _, z) = anim.world_coord.to_cell_sub_z();
            (rx, ry, z)
        })
        .collect();
    cells.sort_unstable();
    // Each at the exact Location: the arrival's is the destination's (z 0).
    assert_eq!(cells, vec![(5, 5, 2), (8, 9, 0)]);
    for anim in &anims {
        assert_eq!(anim.draw_flags, 0x600);
        assert_eq!(anim.z_adjust, 0);
        assert_eq!(anim.runtime.delay_remaining, 0);
        assert_eq!(
            anim.runtime.rate_reload,
            crate::rules::art_data::art_rate_to_logic_frames(120),
            "the art section's own Rate=, through the one AnimType rule"
        );
    }
}

#[test]
fn unbound_warp_art_relocates_without_an_anim() {
    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!((mover.position.rx, mover.position.ry), (8, 9));
    assert_eq!(sim.substrate.anims.len(), 0);
}

#[test]
fn teleport_object_turn_moves_retained_foot_neighbor_counts() {
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
    use crate::sim::overlay_grid::OverlayGrid;
    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(
        32,
        32,
        (0..32)
            .flat_map(|y| (0..32).map(move |x| test_flat_cell(x, y)))
            .collect(),
    ));
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    // This component fixture's Reveal preceded map installation. Complete its
    // admitted Unlimbo's map-dependent producers in native order before the
    // object turn: Techno6F6EDE threat, then Foot4D72xx neighbor counters.
    sim.spatial_threat_after_unlimbo(1, &rules, None);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .cached_spatial_threat()
            .is_some()
    );
    sim.foot_neighbors_after_unlimbo(1, Some(&rules));
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();
    let plane = sim
        .overlay_grid
        .as_ref()
        .unwrap()
        .retained_neighbor_counts();
    assert_eq!(plane.iter().map(|v| u32::from(*v)).sum::<u32>(), 8);
    assert_eq!(plane[4 * 32 + 4], 0, "released origin neighbors");
    assert_eq!(
        plane[8 * 32 + 7],
        1,
        "destination neighbors see the retained update"
    );
    sim.techno_limbo(1);
    assert!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .retained_neighbor_counts()
            .iter()
            .all(|v| *v == 0),
        "Limbo removes the destination source saved by Teleport's callback"
    );
}

/// The cell's raw occupation bits on the Ground layer (`CellClass+0x3F`).
fn raw_bits(sim: &Simulation, cell: (u16, u16)) -> u8 {
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::RawCellKey;
    sim.substrate
        .raw_cell_occupation
        .bits_at(RawCellKey::Real(cell.0, cell.1), MovementLayer::Ground)
}

/// Teleport Process brackets the relocation with Mark(UP) (`0x007195DB`) and
/// Mark(DOWN) (`0x007196BF`): a Unit's cell list entry, raw vehicle bit and
/// vehicle plane leave the origin and mark the destination in the warp's own
/// turn.
#[test]
fn warp_moves_a_units_occupation_through_the_mark_pair() {
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::VEHICLE_OCCUPATION_BIT;
    let rules = rules(false);
    let mut sim = relocating_chrono_unit((8, 9));
    let occupied = |sim: &Simulation, cell: (u16, u16)| {
        sim.substrate
            .cell_occupation
            .vehicle_bits(cell.0, cell.1, MovementLayer::Ground)
            & VEHICLE_OCCUPATION_BIT
            != 0
    };
    assert!(occupied(&sim, (5, 5)));
    assert_ne!(raw_bits(&sim, (5, 5)) & 0x20, 0);

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    assert!(!occupied(&sim, (5, 5)), "the origin is released");
    assert!(occupied(&sim, (8, 9)), "the destination is marked");
    assert_eq!(raw_bits(&sim, (5, 5)) & 0x20, 0);
    assert_ne!(raw_bits(&sim, (8, 9)) & 0x20, 0);
    assert!(!sim.substrate.occupancy.contains_entity(5, 5, 1));
    assert!(sim.substrate.occupancy.contains_entity(8, 9, 1));
}

/// The same Mark pair moves an infantryman's cell list entry but not his raw
/// sub-cell bit: Remove/AddContent return for Infantry before any receiver
/// (`0x0047EAFE` / `0x0047E9EA`), and nothing from `0x007195DB` to
/// `0x007196BF` calls vt+0xF0/+0xF4. Foot SetLocation (`0x004DB810`) marks
/// only while +0x74 is set, which Mark(UP) has cleared.
#[test]
fn warp_moves_an_infantrymans_list_entry_and_leaves_his_raw_bit() {
    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));
    let origin_bits = raw_bits(&sim, (5, 5));
    assert_ne!(origin_bits, 0);
    assert_eq!(raw_bits(&sim, (8, 9)), 0);

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    assert_eq!(raw_bits(&sim, (5, 5)), origin_bits, "the origin keeps it");
    assert_eq!(raw_bits(&sim, (8, 9)), 0, "the destination gets none");
    assert!(!sim.substrate.occupancy.contains_entity(5, 5, 1));
    assert!(sim.substrate.occupancy.contains_entity(8, 9, 1));
}

/// Only the relocating frame builds the two WarpOut animations: a turn in
/// the chrono delay that follows builds none.
#[test]
fn a_chrono_delay_turn_builds_no_warp_anim() {
    let rules = rules(true);
    let mut sim = relocating_legionnaire((8, 9));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .teleport_state_for_test_mut()
        .unwrap()
        .set_ticks_for_test(5);

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();
    assert_eq!(sim.substrate.anims.len(), 2);
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .teleport_state()
            .map(|state| state.phase()),
        Some(TeleportPhase::ChronoDelay)
    );
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();
    assert_eq!(sim.substrate.anims.len(), 2);
}

/// The warp's first step is the Techno detach sweep (`0x007193C7`): an
/// attacker's lock on the owner is dropped in the owner's warp turn.
#[test]
fn the_warp_drops_attack_locks_on_its_owner() {
    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));
    let mut attacker = GameEntity::test_default(2, "CMON", "Russians", 4, 5);
    attacker.attack_target = Some(crate::sim::combat::AttackTarget::new(1));
    sim.substrate.entities.insert(attacker);

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    assert_eq!(
        (
            sim.substrate.entities.get(1).unwrap().position.rx,
            sim.substrate.entities.get(1).unwrap().position.ry
        ),
        (8, 9)
    );
    assert!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .attack_target
            .is_none()
    );
}

/// A mission Restore represents `Assign_Destination(saved, 1)` by NavCom and
/// the deferred flag. The Teleport Process entry finishes it through the
/// Infantry setter, so the man warps (`FootClass::Restore_Mission`
/// `0x004D8F99`, then `0x0051AA40` and Teleport Move_To `0x00718100`); no
/// route is built.
#[test]
fn a_restored_destination_warps_at_the_teleport_process_entry() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::concrete_effects::represented_assign_destination_mode_one;
    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));
    install_destination_cells(&mut sim, &rules);
    let mover = sim.substrate.entities.get_mut(1).unwrap();
    mover.install_teleport_state_for_test(None);
    represented_assign_destination_mode_one(mover, Some(NavTargetRef::cell(8, 9)));
    mover.navigation.pending_arrival_clear = true;

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!((mover.position.rx, mover.position.ry), (8, 9));
    assert!(!mover.navigation.pending_arrival_clear);
    assert!(mover.movement_target.is_none());
}

/// Supplied Clear-overlay inputs exercise the production callback context,
/// not a native overlay-admission golden. Restore must validate its borrowed
/// registry before changing Mission, NavCom, reservations or any RNG stream.
#[test]
fn restoration_callbacks_forward_overlay_inputs_before_any_destination_write() {
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::authority::MissionAuthorityError;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    let rules = rules(false);
    let registry = OverlayTypeRegistry::from_ini(
        &IniFile::from_str(
            "[OverlayTypes]\n0=RESTORE_OVERLAY\n[RESTORE_OVERLAY]\nLand=Clear\n\
             [Clear]\nFoot=100%\n",
        ),
        None,
    );
    let overlay = registry.id_for_name("RESTORE_OVERLAY").unwrap();
    let unregistered = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);
    let destination = NavTargetRef::cell(12, 7);

    for expiry in [true, false] {
        let mut sim = relocating_legionnaire((8, 9));
        install_destination_cells(&mut sim, &rules);
        let mover = sim.substrate.entities.get_mut(1).unwrap();
        mover.install_teleport_state_for_test(None);
        mover.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Attack),
            suspended: MissionId::from_known(MissionType::Move),
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        mover.navigation.nav_com = Some(NavTargetRef::cell(8, 9));
        mover.navigation.suspended_nav_com = Some(destination);
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let cell = terrain.native_cell_identity((12, 7));
        terrain.write_native_cell_overlay(cell, Some(overlay));
        let before = (
            bincode::serialize(sim.substrate.entities.get(1).unwrap()).unwrap(),
            bincode::serialize(&sim.substrate.raw_cell_occupation).unwrap(),
            sim.rng_state(),
        );

        for missing in [None, Some(&unregistered)] {
            let result = if expiry {
                sim.mission_restore_after_target_expiry(1, Some(&rules), missing)
            } else {
                sim.mission_restore_on_target_detach(1, Some(&rules), missing)
            };
            assert!(matches!(
                result,
                Err(MissionAuthorityError::AuthorityUnavailable(_))
            ));
            assert_eq!(
                (
                    bincode::serialize(sim.substrate.entities.get(1).unwrap()).unwrap(),
                    bincode::serialize(&sim.substrate.raw_cell_occupation).unwrap(),
                    sim.rng_state(),
                ),
                before,
                "expiry={expiry}: unavailable inputs refuse before the Restore transaction"
            );
        }

        let result = if expiry {
            sim.mission_restore_after_target_expiry(1, Some(&rules), Some(&registry))
        } else {
            sim.mission_restore_on_target_detach(1, Some(&rules), Some(&registry))
        };
        assert!(result.unwrap());
        let mover = sim.substrate.entities.get(1).unwrap();
        assert_eq!(mover.mission.current().known(), Some(MissionType::Move));
        assert_eq!(mover.mission.suspended(), MissionId::NONE);
        assert_eq!(mover.navigation.nav_com, Some(destination));
        assert_eq!(
            crate::sim::movement::motion_query::is_moving(mover),
            Some(true)
        );
        assert_eq!(mover.teleport_state().unwrap().target_cell(), Some((12, 7)));
        assert_eq!((mover.position.rx, mover.position.ry), (5, 5));
        assert_ne!(raw_bits(&sim, (12, 7)), 0, "the restored Cell is reserved");
    }
}

/// A ground order to an infantryman on Teleport reaches the Infantry setter,
/// which arms the warp, not a route no Process follows.
#[test]
fn a_ground_order_arms_the_warp() {
    use crate::sim::world::GroundMove;
    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));
    install_destination_cells(&mut sim, &rules);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .install_teleport_state_for_test(None);

    let accepted = sim.issue_ground_move(
        GroundMove {
            entity_id: 1,
            target: (12, 7),
            speed: crate::util::fixed_math::SimFixed::from_num(4),
            queue: false,
            speed_type: None,
            owner_blocks: true,
            object_destination: None,
        },
        Some(&rules),
        None,
    );

    assert!(accepted);
    let mover = sim.substrate.entities.get(1).unwrap();
    assert!(mover.movement_target.is_none());
    let warp = mover.teleport_state().expect("armed warp");
    assert_eq!(warp.target_cell().unwrap(), (12, 7));
}

/// A warp the Teleport armed waits while a Drive piggybacks over it: the
/// object turn calls only the active locomotor's Process (`0x004DA877`), so
/// the suspended Teleport runs once End_Piggyback hands it back.
#[test]
fn an_armed_warp_waits_while_a_drive_piggyback_is_active() {
    let rules = rules(false);
    let mut sim = relocating_chrono_unit((8, 9));
    let locomotor = sim
        .substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap();
    assert!(locomotor.begin_piggyback(LocomotorKind::Drive, 0));

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();
    let mover = sim.substrate.entities.get_mut(1).unwrap();
    assert_eq!((mover.position.rx, mover.position.ry), (5, 5));
    assert_eq!(
        mover.teleport_state().map(|state| state.phase()),
        Some(TeleportPhase::Relocate)
    );

    let locomotor = mover.locomotor.as_mut().unwrap();
    if locomotor.active_kind() == LocomotorKind::Drive {
        assert!(locomotor.end_piggyback());
    }
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();
    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!((mover.position.rx, mover.position.ry), (8, 9));
}
