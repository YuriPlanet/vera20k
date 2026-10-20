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
use crate::sim::world::FrameEffects;
use crate::sim::world::common_raw_test_terrain_cell;
use crate::util::save_codec;

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
        .semantics_for_land_type(crate::rules::terrain_rules::LandType::Clear.as_index())
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
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    // Construct the actual class. Reclassifying a Unit test default leaves
    // Infantry's required fear/action runtime absent from its real AI turn.
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        5,
        5,
        2,
        0,
        sim.intern("Americans"),
        crate::sim::combat::Health::new(100),
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
        entity
            .position
            .set_sub_x_for_test(crate::util::lepton::CELL_CENTER_LEPTON);
        entity
            .position
            .set_sub_y_for_test(crate::util::lepton::CELL_CENTER_LEPTON);
        entity.set_sub_cell(Some(0));
    }
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
    entity.install_teleport_state_for_test(Some(TeleportState::for_test(
        TeleportPhase::Relocate,
        target.0,
        target.1,
        0,
    )));
    sim.substrate.entities.insert(entity);
    sim.set_next_stable_object_id_for_test(2);
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
        .filter(|anim| anim.type_id() == warp_out)
        .collect();
    assert_eq!(anims.len(), 2, "one at the origin, one at the destination");
    let mut cells: Vec<_> = anims
        .iter()
        .map(|anim| {
            let (rx, ry, _, _, z) = anim.world_coord().to_cell_sub_z();
            (rx, ry, z)
        })
        .collect();
    cells.sort_unstable();
    // Each at the exact Location: the arrival's is the destination's (z 0).
    assert_eq!(cells, vec![(5, 5, 2), (8, 9, 0)]);
    for anim in &anims {
        assert_eq!(anim.draw_flags(), 0x600);
        assert_eq!(anim.z_adjust(), 0);
        assert_eq!(anim.runtime().delay_remaining, 0);
        assert_eq!(
            anim.runtime().rate_reload,
            crate::rules::art_data::art_rate_to_logic_frames(120),
            "the art section's own Rate=, through the one AnimType rule"
        );
    }
}

#[test]
fn teleport_without_relocation_keeps_incoming_bullet_target() {
    use crate::sim::projectile::ProjectileTarget;
    let rules = rules(false);
    for post_warp in [false, true] {
        let mut sim = relocating_legionnaire(if post_warp { (8, 9) } else { (5, 5) });
        install_destination_cells(&mut sim, &rules);
        let mut state = TeleportState::for_test(
            if post_warp {
                TeleportPhase::ChronoDelay
            } else {
                TeleportPhase::Relocate
            },
            8,
            9,
            10,
        );
        if !post_warp {
            // The native refusal compares full XYZ, not just the cell.
            state.set_destination_for_test(
                crate::sim::movement::ground_pose::position_world_coord(
                    &sim.substrate.entities.get(1).unwrap().position,
                ),
            );
        }
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .install_teleport_state_for_test(Some(state));
        let mut shot = super::super::lifecycle_tests::gsi_05_02_projectile(1, None);
        shot.target = ProjectileTarget::Entity(1);
        sim.projectiles.spawn(99, shot);
        let expected = sim.projectiles.get(99).unwrap().clone();

        sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
            .unwrap();

        assert_eq!(sim.projectiles.get(99), Some(&expected));
    }
}

#[test]
fn teleport_target_loss_and_pointer_expiry_match_native_controls() {
    use crate::sim::components::DriveCoord;
    use crate::sim::projectile::ProjectileTarget;

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/teleport_targets.json",
    ))
    .unwrap();
    let rules = rules(false);
    // These controls must pass the earlier high-flight gate and actually
    // query MapClass; otherwise a null result would hide the intended branch.
    for (name, kind) in [("unallocated_cell", "dummy"), ("word_wrap", "cell")] {
        let row = corpus["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == name)
            .unwrap();
        assert_eq!(row["target"]["kind"], kind);
        assert_eq!(row["map_queries"].as_array().unwrap().len(), 1);
    }
    for row in corpus["rows"].as_array().unwrap() {
        for expire in [false, true] {
            let mut sim = relocating_chrono_unit((9, 8));
            let mut terrain = ResolvedTerrainGrid::from_cells(
                512,
                32,
                (0..32)
                    .flat_map(|y| {
                        (0..512).map(move |x| common_raw_test_terrain_cell(x, y, 6, false))
                    })
                    .collect(),
            );
            let mut allocated: Vec<_> = (16..25)
                .flat_map(|y| (6..25).map(move |x| (x, y)))
                .collect();
            allocated.extend([(0, 20), (511, 20)]);
            terrain.test_set_native_allocated_cells(&allocated);
            sim.install_resolved_terrain_for_new_map(terrain);
            let input = &row["input"];
            let xyz: [i32; 3] = serde_json::from_value(input["xyz"].clone()).unwrap();
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.position.put_location(DriveCoord {
                x: xyz[0],
                y: xyz[1],
                z: xyz[2],
            });
            entity
                .lifecycle
                .set_cell_marked_for_test(input["marked"].as_bool().unwrap());
            let mut shot = super::super::lifecycle_tests::gsi_05_02_projectile(1, None);
            shot.target = ProjectileTarget::Entity(1);
            sim.admit_projectile(99, shot);
            let mut expected = sim.projectiles.get(99).unwrap().clone();
            expected.set_target_for_test(match row["target"]["kind"].as_str().unwrap() {
                "none" => ProjectileTarget::None,
                "dummy" => ProjectileTarget::DummyCell,
                "cell" => ProjectileTarget::Cell {
                    rx: row["target"]["cell"][0].as_u64().unwrap() as u16,
                    ry: row["target"]["cell"][1].as_u64().unwrap() as u16,
                },
                other => panic!("unexpected native target {other}"),
            });
            let repair = |sim: &mut Simulation| {
                if expire {
                    sim.detach_all_pointer_expired(1, &rules, None, FrameEffects::default());
                } else {
                    sim.lose_projectile_targets(1, Some(&rules));
                }
            };
            if input["scope"].as_bool().unwrap() {
                sim.with_object_placement_scope(repair);
            } else {
                repair(&mut sim);
            }
            if expire {
                assert_eq!(row["expiry_source_cleared"], true);
                expected.set_source_id_for_test(crate::sim::combat::RAD_NO_ATTACKER);
            } else {
                assert_eq!(row["source_preserved"], true);
            }
            assert_eq!(
                sim.projectiles.get(99),
                Some(&expected),
                "{} expiry={expire}",
                input["name"]
            );
        }
    }
}

#[test]
fn retail_cmin_teleport_preserves_departure_cell_through_guidance_and_snapshot() {
    use crate::sim::combat::{FiredBullet, RAD_NO_ATTACKER, admit_fired_bullet};
    use crate::sim::projectile::{
        ProjectileCoord, ProjectilePayload, ProjectileTarget, ProjectileVelocity,
    };
    use crate::sim::snapshot::GameSnapshot;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    assert_eq!(
        rules.object("CMIN").unwrap().locomotor,
        LocomotorKind::Teleport
    );
    let weapon = rules.weapon("MissileLauncher").unwrap();
    assert_eq!(weapon.projectile.as_deref(), Some("AAHeatSeeker2"));
    assert!(rules.projectile("AAHeatSeeker2").unwrap().ag);
    assert_eq!(rules.projectile("AAHeatSeeker2").unwrap().rot, 60);
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/teleport_targets.json",
    ))
    .unwrap();
    let flight = &corpus["flight"];
    // Translate the supplied native level6 scene to this fixture's level2
    // centre(5,5). Relative target/flight coordinates and binary frame/ID agree.
    let translated = |value: &serde_json::Value| {
        let xyz: [i32; 3] = serde_json::from_value(value.clone()).unwrap();
        ProjectileCoord::new(xyz[0] - 1792, xyz[1] - 3840, xyz[2] - 416)
    };
    let mut sim = relocating_owner(EntityCategory::Unit, "CMIN", (9, 8));
    install_destination_cells(&mut sim, &rules);
    sim.session.set_binary_frame_for_test(1);
    let bullet_id = sim.allocate_stable_id();
    let payload = ProjectilePayload::new(
        weapon.damage,
        sim.intern(weapon.warhead.as_deref().unwrap()),
        weapon.projectile.as_deref().map(|name| sim.intern(name)),
        weapon.bright,
    )
    .with_installed_weapon(Some(sim.intern("MissileLauncher")));
    let bullet = FiredBullet::launched(
        &sim,
        &rules,
        100,
        RAD_NO_ATTACKER,
        ProjectileTarget::Entity(1),
        payload,
        102,
        translated(&flight["old"]),
        ProjectileVelocity::new(1, 0, 0),
    )
    .unwrap();
    admit_fired_bullet(&mut sim, &rules, weapon, bullet_id, bullet);
    let original = sim.projectiles.get(bullet_id).unwrap().clone();

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    let mut expected = original;
    expected.set_target_for_test(ProjectileTarget::Cell { rx: 5, ry: 5 });
    assert_eq!(sim.projectiles.get(bullet_id), Some(&expected));
    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!((mover.position.rx(), mover.position.ry()), (9, 8));
    let repaired_hash = sim.state_hash();
    sim.projectiles
        .get_mut(bullet_id)
        .unwrap()
        .set_target_for_test(ProjectileTarget::Entity(1));
    assert_ne!(repaired_hash, sim.state_hash());
    sim.projectiles
        .get_mut(bullet_id)
        .unwrap()
        .set_target_for_test(expected.target());

    // Native load resets the Scenario stream; keep that independent policy
    // equal while checking the repaired target and its next flight step.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let saved = GameSnapshot::save(&sim, 0, 0, "teleport-target", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    // Bullet Load46AE9C..46AEB0 deliberately restarts the arm timer at zero.
    // Canonicalize that existing load policy before comparing continuation.
    sim.projectiles
        .get_mut(bullet_id)
        .unwrap()
        .arm_timer_mut_for_test()
        .start(sim.session.binary_frame() as i32, 0);
    assert_eq!(
        restored.projectiles.get(bullet_id),
        sim.projectiles.get(bullet_id)
    );
    for world in [&mut sim, &mut restored] {
        assert!(world.object_ai_visit_one(
            bullet_id,
            Some(&rules),
            techno_ai::ObjectAiCtx::default()
        ));
        let advanced = world.projectiles.get(bullet_id).unwrap();
        assert_eq!(advanced.position(), translated(&flight["candidate"]));
        assert_eq!(
            advanced.last_target_position(),
            translated(&flight["homing_targets"][0])
        );
        let actual = advanced
            .velocity()
            .native()
            .map(|v| format!("{:016x}", v.bits()));
        let expected: Vec<_> = flight["velocity"]["bits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(actual.as_slice(), expected.as_slice());
    }
    assert_eq!(sim.state_hash(), restored.state_hash());
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
/// Mark(DOWN) (`0x007196BF`): a Unit's cell list entry and raw vehicle bit
/// leave the origin and mark the destination in the warp's own turn.
#[test]
fn warp_moves_a_units_occupation_through_the_mark_pair() {
    let rules = rules(false);
    let mut sim = relocating_chrono_unit((8, 9));
    assert_ne!(raw_bits(&sim, (5, 5)) & 0x20, 0);

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

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

/// The warp's first step is the shared detach sweep (`0x007193C7` ->
/// `0x0070D4A0`): an attacker aiming at the owner restores its suspended
/// mission and takes the class NULL setter, which also clears its
/// passive-acquire flag.
#[test]
fn the_warp_restores_its_attackers_suspended_missions() {
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));
    let mut attacker = GameEntity::test_default(2, "CMON", "Russians", 4, 5);
    attacker
        .attack_target
        .set_for_test(Some(crate::sim::combat::TargetKind::Entity(1)));
    attacker
        .passive_scan
        .set_passively_acquired_target_for_test(true);
    attacker.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Attack),
        suspended: MissionId::from_known(MissionType::Guard),
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    sim.substrate.entities.insert(attacker);

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    let attacker = sim.substrate.entities.get(2).unwrap();
    assert!(attacker.attack_target.target().is_none());
    assert!(!attacker.passive_scan.acquired());
    assert_eq!(
        attacker.mission.current(),
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(attacker.mission.suspended(), MissionId::NONE);
}

/// A mission Restore represents `Assign_Destination(saved, 1)` by NavCom and
/// the deferred flag. The Teleport Process entry finishes it through the
/// Infantry setter, so the man warps (`FootClass::Restore_Mission`
/// `0x004D8F99`, then `0x0051AA40` and Teleport Move_To `0x00718100`); no
/// route is built.
#[test]
fn a_restored_destination_warps_at_the_teleport_process_entry() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::concrete_effects::represented_assign_destination;
    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));
    install_destination_cells(&mut sim, &rules);
    let mover = sim.substrate.entities.get_mut(1).unwrap();
    mover.install_teleport_state_for_test(None);
    represented_assign_destination(mover, Some(NavTargetRef::cell(8, 9)));
    mover
        .navigation
        .set_pending_destination_setter_for_test(true);

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!((mover.position.rx(), mover.position.ry()), (8, 9));
    assert!(!mover.navigation.pending_destination_setter());
    assert!(crate::sim::movement::motion_query::is_moving(&mover) != Some(true));
}

/// Supplied Clear-overlay inputs exercise the production callback context,
/// not a native overlay-admission golden. Restore must validate its borrowed
/// registry before changing Mission, NavCom, reservations or any RNG stream.
#[test]
fn restoration_callbacks_forward_overlay_inputs_before_any_destination_write() {
    use crate::rules::overlay_types::OverlayTypeRegistry;
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
    mover
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(8, 9)));
    mover
        .navigation
        .set_suspended_nav_com_for_test(Some(destination));
    // In OverlayGrid, which owns a real cell's overlay.
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(32, 32);
    overlays.place_overlay(12, 7, overlay, 0);
    sim.overlay_grid = Some(overlays);
    let before = (
        save_codec::serialize(sim.substrate.entities.get(1).unwrap()).unwrap(),
        save_codec::serialize(&sim.substrate.raw_cell_occupation).unwrap(),
        sim.rng_state(),
    );

    for missing in [None, Some(&unregistered)] {
        let result = sim.mission_restore_represented(
            1,
            Some(&rules),
            missing,
            crate::sim::world::FrameEffects::default(),
        );
        assert!(matches!(
            result,
            Err(MissionAuthorityError::AuthorityUnavailable(_))
        ));
        assert_eq!(
            (
                save_codec::serialize(sim.substrate.entities.get(1).unwrap()).unwrap(),
                save_codec::serialize(&sim.substrate.raw_cell_occupation).unwrap(),
                sim.rng_state(),
            ),
            before,
            "unavailable inputs refuse before the Restore transaction"
        );
    }

    let result = sim.mission_restore_represented(
        1,
        Some(&rules),
        Some(&registry),
        crate::sim::world::FrameEffects::default(),
    );
    assert!(result.unwrap());
    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!(mover.mission.current().known(), Some(MissionType::Move));
    assert_eq!(mover.mission.suspended(), MissionId::NONE);
    assert_eq!(mover.navigation.nav_com(), Some(destination));
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(mover),
        Some(true)
    );
    assert_eq!(mover.teleport_state().unwrap().target_cell(), Some((12, 7)));
    assert_eq!((mover.position.rx(), mover.position.ry()), (5, 5));
    assert_ne!(raw_bits(&sim, (12, 7)), 0, "the restored Cell is reserved");
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
    assert_eq!((mover.position.rx(), mover.position.ry()), (5, 5));
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
    assert_eq!((mover.position.rx(), mover.position.ry()), (8, 9));
}
