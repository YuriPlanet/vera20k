//! Projectile lifecycle tests: bullet logic slots, mixed six-family logic
//! order, projectile source and target retargeting on expiry, and graph
//! restore of fired objects.

use super::*;

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
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));

    let run = |initial_wall_data: u8| {
        let mut sim = Simulation::new();
        let mut grid = crate::sim::overlay_grid::OverlayGrid::new(12, 12);
        grid.place_overlay(5, 5, 2, initial_wall_data);
        let _ = grid.take_dirty_cells();
        sim.overlay_grid = Some(grid);
        sim.resolved_terrain = Some(crate::sim::tiberium::test_support::flat_terrain(12, 12));

        let projectile_id = sim.substrate.allocate_stable_id();
        let impact = ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0);
        let mut spawn = gsi_05_02_projectile(crate::sim::combat::RAD_NO_ATTACKER, Some(0));
        spawn.origin = impact;
        spawn.target = ProjectileTarget::Cell { rx: 5, ry: 5 };
        spawn.initial_target_position = impact;
        spawn.payload = ProjectilePayload::new(1, sim.interner.intern("WALLWH"), None, false);
        sim.admit_projectile(projectile_id, spawn);

        assert!(sim.object_ai_visit_one(
            projectile_id,
            Some(&rules),
            &mut ObjectAiCtx {
                overlay_registry: Some(&overlays),
                ..ObjectAiCtx::default()
            },
        ));
        assert!(
            !sim.projectiles
                .get(projectile_id)
                .unwrap()
                .in_logic_vector()
        );
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
    assert!(partial.dirty_cells.radar().is_empty());
    assert_eq!(partial.dirty_cells.radar_generation(), 0);
    assert_eq!(partial.dirty_cells.tactical_for_test(), vec![(5, 5)]);

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
        terminal.dirty_cells.radar(),
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
    assert_eq!(terminal.dirty_cells.radar_generation(), 13);
    assert_eq!(
        terminal.dirty_cells.tactical_for_test(),
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
        missile_rot_var: crate::util::native_x87::NativeF64Bits::from_bits(0.25f64.to_bits()),
        course_lock_duration: 0,
        course_frames: 0,
        course_locked: true,
        very_high: false,
        max_speed: 0,
        acceleration: 3,
        closing_frames: 0,
        closing_accumulator_bits: 0,
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

    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/ifv_lifecycle_controls.json",
    ))
    .unwrap();
    // Supplied guided motion isolates the OLD-height collision predicate and
    // JumpJet fuse handoff, with a one-point wall receiver exposing the final
    // impact cell. Full retail AAHeatSeeker2 steering has separate native
    // flight comparisons; it may climb here rather than cross the ground.
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
        // Original HomingTrack5B20F0 honors Level by skipping cruise-height
        // correction. No acceleration keeps the supplied first [4,0,-2]
        // step; otherwise native ramp/clearance changes this fixture's crossing.
        // Exact supplied controls: ifv_lifecycle_controls.{py,json}.
        shot.collision.level = true;
        let guidance = shot.guidance.as_mut().unwrap();
        guidance.max_speed = 4;
        guidance.acceleration = 0;
        let source_id = if with_source {
            let source_id = sim.substrate.allocate_stable_id();
            insert_entity(&mut sim, source_id, EntityCategory::Unit);
            let type_ref = sim.interner.intern("MTNK");
            sim.substrate
                .entities
                .get_mut(source_id)
                .unwrap()
                .set_type_ref_for_test(type_ref);
            shot.source_id = source_id;
            shot.arm_frames = 0;
            Some(source_id)
        } else {
            None
        };
        shot.payload = ProjectilePayload::new(1, sim.interner.intern("WALLWH"), None, false);
        let id = sim.substrate.allocate_stable_id();
        sim.admit_projectile(id, shot);
        // The native control supplies an in-flight OLD-height boundary.
        // Restore it after admission's independent 46C4F0 launch-floor fixup.
        sim.projectiles
            .get_mut(id)
            .unwrap()
            .set_position_for_test(origin);
        if with_source {
            let bullet = sim.projectiles.get_mut(id).unwrap();
            bullet.set_proximity_reference_for_test(ProjectileCoord::new(
                origin.x + 104,
                origin.y,
                origin.z - 2,
            ));
            bullet.set_last_distance_half_for_test(0);
        }
        if expire_source {
            sim.uninit(source_id.unwrap());
            assert_eq!(
                sim.projectiles.get(id).unwrap().source_id(),
                crate::sim::combat::RAD_NO_ATTACKER
            );
        }
        let mut runtime = SimRuntime::from_simulation(sim);
        runtime.resources_mut().rules =
            RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();

        let _ = runtime
            .advance_frame(
                &[],
                16,
                super::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        if old_height == 1 {
            let bullet = runtime.simulation.projectiles.get(id).expect(
                "crossing below ground does not trigger the OLD-height predicate until next visit",
            );
            assert!(bullet.in_logic_vector());
            // Native ground_old1 uses level6; this fixture uses level2.
            let row = &native["rows"][0];
            assert_eq!(row["input"]["name"], "ground_old1_level_no_acceleration");
            let native_relative_z = row["candidate"][2].as_i64().unwrap()
                - row["prepared"]["level"].as_i64().unwrap() * 104;
            assert_eq!(i64::from(bullet.position().z), 208 + native_relative_z);
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
                .advance_frame(
                    &[],
                    16,
                    super::TickLane::Ordinary,
                    &mut crate::sim::world::FrameEffects::default(),
                )
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
    shot.guidance.as_mut().unwrap().max_speed = 4;
    let origin = shot.origin;
    let id = sim.substrate.allocate_stable_id();
    sim.admit_projectile(id, shot);
    sim.projectiles
        .get_mut(id)
        .unwrap()
        .set_proximity_reference_for_test(origin);
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

/// The Wave owner's type (`insert_entity`'s TEST), whose fire coordinate
/// keeps a Wave tracking; the warhead a detonating fixture projectile names;
/// the particle system type `insert_particle_system` places; and the ART of
/// `insert_anim`'s TESTANIM.
fn gsi_05_02_rules() -> crate::rules::ruleset::RuleSet {
    use crate::rules::ini_parser::IniFile;
    let mut rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\n\
         [Warheads]\n0=GSIWH\n[GSIWH]\nCellSpread=0\n\
         [ParticleSystems]\n0=GSISYS\n[GSISYS]\nBehavesLike=Smoke\n",
    ))
    .expect("GSI fixture rules");
    let mut art =
        crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str("[TESTANIM]\nEnd=10\n"));
    art.bind_anim_frame_count_for_test("TESTANIM", 10);
    rules.replace_art_registry_for_test(art);
    rules
}

fn gsi_05_02_mixed_fixture() -> (Simulation, [u64; 6]) {
    let mut sim = Simulation::new();

    let entity_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, entity_id, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(entity_id)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Cell(1, 0)));
    sim.register_live_object(entity_id);

    let anim_id = sim.substrate.allocate_stable_id();
    insert_anim(&mut sim, anim_id, false);
    sim.register_live_object(anim_id);

    let particle_id = sim.substrate.allocate_stable_id();
    insert_particle_system(&mut sim, particle_id);
    sim.register_live_object(particle_id);

    let terrain_id = sim.substrate.allocate_stable_id();
    sim.terrain_objects.insert_constructed(
        {
            let mut terrain =
                TerrainObjectState::for_test(terrain_id, sim.interner.intern("TREE01"), 8, 9);
            terrain.set_health_for_test(10);
            terrain.set_max_health_for_test(10);
            terrain
        },
        false,
    );
    sim.terrain_objects.add_to_cell(terrain_id, (8, 9));
    assert!(
        sim.register_terrain_object(terrain_id, crate::rules::ruleset::RuleSet::empty_for_test())
    );

    let projectile_id = sim.substrate.allocate_stable_id();
    sim.admit_projectile(projectile_id, gsi_05_02_projectile(entity_id, None));

    let wave_id = sim.substrate.allocate_stable_id();
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

    // Each visit before the last keeps its object, since an object leaving
    // the vector during its own visit makes the walk skip its successor (see
    // the terminal-current test below). So the Wave precedes its owner, whose
    // visit drops the cell target (TEST has no weapon) that keeps the Wave
    // tracking, and the particle system, which has nothing to spawn and
    // retires at once, comes last.
    let mixed = [
        terrain_id,
        wave_id,
        entity_id,
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
    let [
        terrain_id,
        wave_id,
        entity_id,
        anim_id,
        projectile_id,
        particle_id,
    ] = mixed;
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
            .terrain_objects
            .objects()
            .get(&terrain_id)
            .unwrap()
            .in_logic_vector()
    );
    assert!(
        restored
            .substrate
            .entities
            .get(entity_id)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    assert!(restored.waves.get(wave_id).unwrap().in_logic_vector());
    assert!(
        restored
            .substrate
            .anims
            .get(anim_id)
            .unwrap()
            .in_logic_vector()
    );
    assert!(
        restored
            .projectiles
            .get(projectile_id)
            .unwrap()
            .in_logic_vector()
    );
    assert!(
        restored
            .substrate
            .particle_systems
            .get(particle_id)
            .unwrap()
            .in_logic_vector()
    );

    let rules = gsi_05_02_rules();
    let mut visited = Vec::new();
    restored.for_each_live_object(|sim, id| {
        if sim.object_ai_visit_one(id, Some(&rules), &mut ObjectAiCtx::default()) {
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
        wave_id,
        entity_id,
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
    let ghost = sim.substrate.allocate_stable_id();
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
    assert!(!sim.waves.get(wave_id).unwrap().in_logic_vector());
    assert!(
        sim.terrain_objects
            .objects()
            .get(&terrain_id)
            .unwrap()
            .in_logic_vector()
    );
    assert!(
        sim.substrate
            .entities
            .get(entity_id)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    assert!(sim.substrate.anims.get(anim_id).unwrap().in_logic_vector());
    assert!(
        sim.projectiles
            .get(projectile_id)
            .unwrap()
            .in_logic_vector()
    );
    assert!(
        sim.substrate
            .particle_systems
            .get(particle_id)
            .unwrap()
            .in_logic_vector()
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
    assert!(sim.waves.get(wave_id).unwrap().in_logic_vector());

    // The retire subset dispatches through the same classifier: Terrain /
    // Bullet / Wave retire; Entity and Anim do not.
    assert!(!sim.retire_non_entity_object(entity_id, crate::sim::world::UninitContext::default()));
    assert!(!sim.retire_non_entity_object(anim_id, crate::sim::world::UninitContext::default()));
    assert!(sim.retire_non_entity_object(terrain_id, crate::sim::world::UninitContext::default()));
    assert!(sim.substrate.pending_delete.contains_object(&terrain_id));
    assert!(!sim.live_object_order_snapshot().contains(&terrain_id));

    sim.debug_assert_logic_membership_consistent();
}

#[test]
fn gsi_05_02_tail_appends_run_same_pass_and_terminal_current_skips_successor() {
    let mut sim = Simulation::new();
    let entity_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, entity_id, EntityCategory::Unit);
    sim.register_live_object(entity_id);

    let mut appended = None;
    let mut visited = Vec::new();
    sim.for_each_live_object(|sim, id| {
        visited.push(id);
        let _ = sim.object_ai_visit_one(id, None, &mut ObjectAiCtx::default());
        if id == entity_id {
            let projectile_id = sim.substrate.allocate_stable_id();
            sim.admit_projectile(projectile_id, gsi_05_02_projectile(entity_id, None));
            let wave_id = sim.substrate.allocate_stable_id();
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
        sim.projectiles.get(projectile_id).unwrap().position(),
        // CellClass target coordinates resolve from the live cell center on
        // every visit, so Cell(16, 0) contributes a small positive Y step.
        // BulletType 46C4F0 places this ground-level launch at floor + 1.
        ProjectileCoord::new(64, 1, 1)
    );
    assert_eq!(sim.waves.get(wave_id).unwrap().lifetime(), 99);

    let rules = gsi_05_02_rules();
    let mut removal = Simulation::new();
    let terminal_id = removal.substrate.allocate_stable_id();
    let mut terminal = gsi_05_02_projectile(0, Some(0));
    terminal.payload = ProjectilePayload::new(1, removal.interner.intern("GSIWH"), None, false);
    removal.admit_projectile(terminal_id, terminal);
    let successor_id = removal.substrate.allocate_stable_id();
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
        let _ = sim.object_ai_visit_one(id, Some(&rules), &mut ObjectAiCtx::default());
    });
    assert_eq!(removal_visited, vec![terminal_id]);
    assert_eq!(removal.live_object_order_snapshot(), vec![successor_id]);
    assert!(removal.projectiles.get(terminal_id).is_some());
    assert_eq!(
        removal.substrate.pending_delete.object_ids(),
        vec![terminal_id]
    );
    assert_eq!(removal.waves.get(successor_id).unwrap().lifetime(), 100);
    removal.debug_assert_logic_membership_consistent();
}

#[test]
fn gsi_05_02_terrain_spawner_registers_before_later_techno() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n[TREE01]\nStrength=10\nSpawnsTiberium=yes\n",
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
    let terrain_id = *sim.terrain_objects.objects().keys().next().unwrap();

    let entity_id = sim.substrate.allocate_stable_id();
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
    let terrain_id = sim.substrate.allocate_stable_id();
    let type_ref = sim.interner.intern("TREE01");
    let warhead_ref = sim.interner.intern("WOODWH");
    sim.terrain_objects
        .objects_mut_for_test()
        .insert(terrain_id, {
            let mut terrain = TerrainObjectState::for_test(terrain_id, type_ref, 5, 6);
            terrain.set_health_for_test(10);
            terrain.set_max_health_for_test(10);
            terrain
        });
    sim.terrain_objects
        .cell_index_mut_for_test()
        .insert((5, 6), terrain_id);
    assert!(
        sim.register_terrain_object(terrain_id, crate::rules::ruleset::RuleSet::empty_for_test())
    );

    sim.commit_noncombat_aoe_receivers(
        &rules,
        None,
        &[crate::sim::combat::combat_aoe::AreaDamageReceiver::Terrain(
            crate::sim::combat::TerrainDamageEvent {
                damage_writeback: None,
                stable_id: terrain_id,
                rx: 5,
                ry: 6,
                damage: 10,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
        )],
        &mut crate::sim::world::FrameEffects::default(),
    );
    let terrain = &sim.terrain_objects.objects()[&terrain_id];
    assert_eq!(terrain.lifecycle(), TerrainObjectLifecycle::Destroyed);
    assert!(!terrain.in_logic_vector());
    assert!(sim.live_object_order_snapshot().is_empty());
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![terrain_id]);

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

    let terrain_id = sim.substrate.allocate_stable_id();
    let terrain_type = sim.interner.intern("TREE01");
    let warhead_ref = sim.interner.intern("WOODWH");
    sim.terrain_objects
        .objects_mut_for_test()
        .insert(terrain_id, {
            let mut terrain = TerrainObjectState::for_test(terrain_id, terrain_type, 5, 6);
            terrain.set_health_for_test(10);
            terrain.set_max_health_for_test(10);
            terrain
        });
    sim.terrain_objects
        .cell_index_mut_for_test()
        .insert((5, 6), terrain_id);
    assert!(
        sim.register_terrain_object(terrain_id, crate::rules::ruleset::RuleSet::empty_for_test())
    );
    sim.commit_noncombat_aoe_receivers(
        &rules,
        None,
        &[crate::sim::combat::combat_aoe::AreaDamageReceiver::Terrain(
            crate::sim::combat::TerrainDamageEvent {
                damage_writeback: None,
                stable_id: terrain_id,
                rx: 5,
                ry: 6,
                damage: 10,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
        )],
        &mut crate::sim::world::FrameEffects::default(),
    );

    let projectile_id = sim.substrate.allocate_stable_id();
    sim.admit_projectile(projectile_id, gsi_05_02_projectile(0, Some(0)));
    assert!(sim.object_ai_visit_one(projectile_id, None, &mut ObjectAiCtx::default()));

    let wave_id = sim.substrate.allocate_stable_id();
    let mut terminal_wave = Wave::new(
        3,
        ProjectileCoord::new(0, 0, 0),
        ProjectileCoord::new(256, 0, 0),
    );
    terminal_wave.set_lifetime_for_test(0);
    sim.admit_wave(wave_id, terminal_wave);
    assert!(sim.object_ai_visit_one(wave_id, None, &mut ObjectAiCtx::default()));

    assert_eq!(
        sim.terrain_objects.objects()[&terrain_id].lifecycle(),
        TerrainObjectLifecycle::Destroyed
    );
    assert!(sim.terrain_objects.objects().contains_key(&terrain_id));
    assert!(sim.projectiles.get(projectile_id).is_some());
    assert!(sim.waves.get(wave_id).is_some());
    assert!(!sim.terrain_objects.objects()[&terrain_id].in_logic_vector());
    assert!(
        !sim.projectiles
            .get(projectile_id)
            .unwrap()
            .in_logic_vector()
    );
    assert!(!sim.waves.get(wave_id).unwrap().in_logic_vector());
    assert_eq!(
        sim.substrate.pending_delete.object_ids(),
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
        restored.substrate.pending_delete.object_ids(),
        vec![terrain_id, projectile_id, wave_id]
    );
    assert!(restored.terrain_objects.objects().contains_key(&terrain_id));
    assert!(restored.projectiles.get(projectile_id).is_some());
    assert!(restored.waves.get(wave_id).is_some());

    restored.process_pending_delete();
    assert!(!restored.terrain_objects.objects().contains_key(&terrain_id));
    assert!(restored.projectiles.get(projectile_id).is_none());
    assert!(restored.waves.get(wave_id).is_none());
    assert!(restored.substrate.pending_delete.is_empty());
}

#[test]
fn gsi_05_04_ground_source_and_target_retarget_before_removal_without_expiry() {
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);
    install_common_raw_terrain(&mut sim, 16, 16, 2, None);

    let target_id = sim.substrate.allocate_stable_id();
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
    let projectile_id = sim.substrate.allocate_stable_id();
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
    assert_eq!(projectile.source_id(), crate::sim::combat::RAD_NO_ATTACKER);
    assert_eq!(projectile.target(), cell_target);
    assert!(projectile.in_logic_vector());
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
    assert!(sim.object_ai_visit_one(projectile_id, None, &mut ObjectAiCtx::default()));
    assert!(sim.projectiles.get(projectile_id).is_some());
    assert_eq!(
        sim.projectiles.get(projectile_id).unwrap().target(),
        cell_target
    );
    assert!(
        f64::from_bits(
            sim.projectiles
                .get(projectile_id)
                .unwrap()
                .velocity()
                .y
                .bits()
        ) > 0.0,
        "allocated Cell target remains at (9,11) despite the unrelated dummy stamp"
    );
}

#[test]
fn gsi_05_04_building_get_coords_uses_foundation_center_cell() {
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(20);
    sim.session.set_map_height_for_test(20);
    install_common_raw_terrain(&mut sim, 20, 20, 0, None);

    let target_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Structure);
    let gapowr = sim.interner.intern("GAPOWR");
    {
        let target = sim.substrate.entities.get_mut(target_id).unwrap();
        target.set_type_ref_for_test(gapowr);
        target
            .seldom
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
    }
    assert!(matches!(
        sim.try_reveal_entity(target_id, common_raw_request(9, 11, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    let projectile_id = sim.substrate.allocate_stable_id();
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
        sim.projectiles.get(projectile_id).unwrap().target(),
        ProjectileTarget::Cell { rx: 10, ry: 12 },
        "BuildingClass GetCoords shifts GAPOWR's NW anchor by 128 leptons per 2x2 axis before truncation"
    );
}

#[test]
fn gsi_04_01_cell_target_uses_live_structural_bit() {
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);
    install_common_raw_terrain(&mut sim, 16, 16, 0, Some((6, 7)));
    {
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(6, 7)
            .unwrap();
        cell.level = 2;
        cell.slope_type = 1;
    }

    let target_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(target_id, common_raw_request(6, 7, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    let projectile_id = sim.substrate.allocate_stable_id();
    let center_x = 6 * 256 + 128;
    let center_y = 7 * 256 + 128;
    let bridge_z = crate::util::lepton::ground_height_leptons(2, 1, center_x, center_y)
        .unwrap()
        .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
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
        sim.projectiles.get(projectile_id).unwrap().target(),
        ProjectileTarget::Cell { rx: 6, ry: 7 }
    );
    assert_eq!(
        cell_target_coord(sim.resolved_terrain.as_ref(), 6, 7),
        center
    );
    assert!(sim.object_ai_visit_one(projectile_id, None, &mut ObjectAiCtx::default()));

    // The actual Bullet visit consumes the live Cell target coordinate. Being
    // stationary at that target is not an admission: old height is 416, above
    // both native tail gates (4677D3: <208; 467B68: <10).
    assert_eq!(
        sim.projectiles.get(projectile_id).unwrap().position(),
        center
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
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);

    let victim_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, victim_id, EntityCategory::Unit);
    let victim_type = sim.interner.intern("VICTIM");
    {
        let victim = sim.substrate.entities.get_mut(victim_id).unwrap();
        victim.set_type_ref_for_test(victim_type);
        victim.health.set_for_test(10);
    }
    assert!(matches!(
        sim.try_reveal_entity(victim_id, common_raw_request(5, 6, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let listener_id = sim.substrate.allocate_stable_id();
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
        payload: ProjectilePayload::new(10, sim.interner.intern("KILLWH"), None, false),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let logic_order = sim.live_object_order_snapshot();

    let _ = sim.tick_combat_with_fatal_lifecycle(
        &rules,
        None,
        100,
        &logic_order,
        &std::collections::BTreeSet::new(),
        &[detonation],
        &mut crate::sim::world::FrameEffects::default(),
    );

    let victim = sim
        .substrate
        .entities
        .get(victim_id)
        .expect("fatal target remains resolvable before the common drain");
    assert!(!victim.lifecycle.object_alive());
    assert!(victim.lifecycle.in_limbo());
    assert!(sim.substrate.pending_delete.contains_object(&victim_id));
    assert_eq!(
        sim.projectiles.get(listener_id).unwrap().target(),
        ProjectileTarget::Cell { rx: 5, ry: 6 },
        "the synchronous fatal UnInit must read the same allocated CellClass terrain authority as combat before late deletion"
    );
}

#[test]
fn gsi_05_04_combat_fatal_garrison_recursion_keeps_cell_target() {
    let rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
        &crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=OCCUPANT\n\
             [VehicleTypes]\n0=BLOCKER\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=GARRISON\n\
             [Warheads]\n0=KILLWH\n\
             [OCCUPANT]\nStrength=10\nArmor=none\n\
             [BLOCKER]\nStrength=100\nArmor=heavy\n\
             [GARRISON]\nStrength=10\nArmor=concrete\nCanBeOccupied=yes\n\
             [KILLWH]\nCellSpread=0\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ),
        &crate::rules::ini_parser::IniFile::from_str("[GARRISON]\nFoundation=2x2\n"),
    )
    .expect("fatal garrison recursion rules");
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);

    let building_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, building_id, EntityCategory::Structure);
    let passenger_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, passenger_id, EntityCategory::Infantry);
    let building_type = sim.interner.intern("GARRISON");
    let passenger_type = sim.interner.intern("OCCUPANT");
    {
        let passenger = sim.substrate.entities.get_mut(passenger_id).unwrap();
        passenger.set_type_ref_for_test(passenger_type);
        passenger.set_passenger_role_for_test(PassengerRole::inside_for_test(building_id, false));
    }
    {
        let building = sim.substrate.entities.get_mut(building_id).unwrap();
        building.set_type_ref_for_test(building_type);
        building
            .seldom
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
        building.health.set_for_test(10);
        building
            .passenger_role_mut_for_test()
            .occupants_mut()
            .add_for_test(passenger_id);
    }
    assert!(matches!(
        sim.try_reveal_entity_with_context(
            building_id,
            common_raw_request(8, 8, 0, 128, 128),
            super::UninitContext::new(&rules)
        ),
        RevealOutcome::Revealed { .. }
    ));
    assert!(
        sim.substrate
            .entities
            .get(building_id)
            .unwrap()
            .cached_spatial_threat()
            .is_some()
    );

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
        let blocker_id = sim.substrate.allocate_stable_id();
        insert_entity(&mut sim, blocker_id, EntityCategory::Unit);
        sim.substrate
            .entities
            .get_mut(blocker_id)
            .unwrap()
            .set_type_ref_for_test(blocker_type);
        assert!(matches!(
            sim.try_reveal_entity(blocker_id, common_raw_request(rx, ry, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));
    }

    let listener_id = sim.substrate.allocate_stable_id();
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
        payload: ProjectilePayload::new(10, sim.interner.intern("KILLWH"), None, false),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let logic_order = sim.live_object_order_snapshot();

    let _ = sim.tick_combat_with_fatal_lifecycle(
        &rules,
        None,
        100,
        &logic_order,
        &std::collections::BTreeSet::new(),
        &[detonation],
        &mut crate::sim::world::FrameEffects::default(),
    );

    let passenger = sim
        .substrate
        .entities
        .get(passenger_id)
        .expect("no-exit occupant remains resolvable before common drain");
    assert!(!passenger.lifecycle.object_alive());
    assert!(passenger.lifecycle.in_limbo());
    assert!(sim.substrate.pending_delete.contains_object(&passenger_id));
    assert_eq!(
        sim.projectiles.get(listener_id).unwrap().target(),
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
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);

    let target_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(target_id, common_raw_request(8, 8, 0, 128, 64)),
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

    let projectile_id = sim.substrate.allocate_stable_id();
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
        sim.projectiles.get(projectile_id).unwrap().target(),
        ProjectileTarget::DummyCell,
        "MapClass::Get_CellClass @ 0x005657A0 answers an unallocated slot with \
         the shared dummy CellClass carrying the requested coord, never NULL"
    );
    assert!(
        sim.projectiles
            .get(projectile_id)
            .unwrap()
            .in_logic_vector()
    );
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
    assert!(sim.object_ai_visit_one(projectile_id, None, &mut ObjectAiCtx::default()));
    assert!(
        f64::from_bits(
            sim.projectiles
                .get(projectile_id)
                .unwrap()
                .velocity()
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
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);
    install_common_raw_terrain(&mut sim, 16, 16, 0, None);

    let target_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(target_id, request(0, 0, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));

    let projectile_id = sim.substrate.allocate_stable_id();
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
        sim.projectiles.get(projectile_id).unwrap().target(),
        ProjectileTarget::None,
        "cell (0, 0) is the native sentinel, so the target is cleared rather \
         than retained as a Cell"
    );
    assert!(
        sim.projectiles
            .get(projectile_id)
            .unwrap()
            .in_logic_vector()
    );
}

#[test]
fn gsi_05_04_high_flying_source_and_target_become_explicit_null() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/ifv_lifecycle_controls.json",
    ))
    .unwrap();
    let control = &native["rows"][2];
    assert_eq!(control["input"]["name"], "expired_null_below_safety");
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(32);
    sim.session.set_map_height_for_test(32);
    // Installed so the case is production-shaped rather than a bare store; the
    // replacement no longer reads terrain either way.
    // `ObjectClass::IsHighFlying @ 0x005F6B90` is the only thing keeping this
    // target from becoming the aircraft's Cell: drop that gate and the three
    // *target* assertions below flip to `Cell { rx: 13, ry: 15 }`. The two
    // `source_id` assertions do not move — native's firer clear at
    // `0x00468503` is gated on the firer matching the expiring pointer
    // (`0x004684FF`), never on the high-flying predicate.
    install_common_raw_terrain(&mut sim, 32, 32, 0, None);

    let target_id = sim.substrate.allocate_stable_id();
    install_fly_aircraft(&mut sim, target_id, SimFixed::from_num(208));
    assert!(matches!(
        sim.try_reveal_entity(target_id, request(13, 15, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    // A Reveal without rules commits no exact Z; SetHeight puts the aircraft
    // at 208, the high-flight threshold.
    sim.set_object_height(
        target_id,
        208,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        &mut crate::sim::world::FrameEffects::default(),
    );
    let projectile_id = sim.substrate.allocate_stable_id();
    let mut spawn = gsi_05_04_guided_projectile(
        target_id,
        ProjectileTarget::Entity(target_id),
        ProjectileCoord::new(13 * 256 + 128, 15 * 256 + 64, 208),
    );
    // Null-target Bullet466E70 detonates at old height>=MissileSafetyAltitude.
    // Stay below its constructor500 to observe the separate null pitch arm.
    // ifv_lifecycle_controls preserves the executed ID/frame and velocity;
    // translating native floor624 to0 keeps the same400-lepton height.
    spawn.origin = ProjectileCoord::new(1024, 1024, 400);
    spawn.native_unique_id = control["prepared"]["unique_id"].as_i64().unwrap() as i32;
    sim.session
        .set_binary_frame_for_test(control["prepared"]["binary_frame"].as_u64().unwrap() as u32);
    sim.admit_projectile(projectile_id, spawn);
    sim.lifecycle_test_events.clear();

    sim.uninit(target_id);

    let projectile = sim.projectiles.get(projectile_id).unwrap();
    assert_eq!(projectile.source_id(), crate::sim::combat::RAD_NO_ATTACKER);
    assert_eq!(projectile.target(), ProjectileTarget::None);
    assert!(projectile.in_logic_vector());
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
        restored.projectiles.get(projectile_id).unwrap().target(),
        ProjectileTarget::None
    );
    assert!(restored.object_ai_visit_one(projectile_id, None, &mut ObjectAiCtx::default()));
    let advanced = restored.projectiles.get(projectile_id).unwrap();
    assert_eq!(advanced.target(), ProjectileTarget::None);
    let expected_velocity: [u64; 3] = std::array::from_fn(|i| {
        u64::from_str_radix(control["velocity"]["bits"][i].as_str().unwrap(), 16).unwrap()
    });
    assert_eq!(
        [
            advanced.velocity().x.bits(),
            advanced.velocity().y.bits(),
            advanced.velocity().z.bits()
        ],
        expected_velocity,
        "native null-target tracking preserves yaw and pitches upward, rather than tracking the cached target"
    );
    // Closing-accumulator distance depends on the translated absolute XYZ;
    // this pointer-lifecycle comparison deliberately does not compare it.
}

#[test]
fn gsi_05_04_cell_target_cleanup_is_idempotent_across_duplicate_uninit() {
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(8);
    sim.session.set_map_height_for_test(8);

    let target_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    assert!(matches!(
        sim.try_reveal_entity(target_id, request(9, 9, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    let projectile_id = sim.substrate.allocate_stable_id();
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
    assert_eq!(
        after_first.target(),
        ProjectileTarget::Cell { rx: 9, ry: 9 }
    );
    sim.lifecycle_test_events.clear();

    sim.uninit(target_id);

    assert_eq!(sim.projectiles.get(projectile_id).unwrap(), &after_first);
    assert_eq!(
        sim.substrate.pending_delete.object_ids(),
        vec![target_id, target_id]
    );
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
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);

    let entity_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, entity_id, EntityCategory::Unit);

    let projectile_id = sim.substrate.allocate_stable_id();
    let target_id = 5;
    sim.admit_projectile(
        projectile_id,
        gsi_05_04_guided_projectile(
            target_id,
            ProjectileTarget::Entity(target_id),
            ProjectileCoord::new(2 * 256, 3 * 256, 0),
        ),
    );

    let anim_id = sim.substrate.allocate_stable_id();
    insert_anim(&mut sim, anim_id, false);
    let particle_id = sim.substrate.allocate_stable_id();
    insert_particle_system(&mut sim, particle_id);
    assert_eq!(sim.substrate.allocate_stable_id(), target_id);
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
    assert!(
        sim.projectiles
            .get(projectile_id)
            .unwrap()
            .in_logic_vector()
    );
}

#[test]
fn gsi_05_02_graph_restore_allows_static_terrain_and_requires_fired_objects() {
    let mut terrain = Simulation::new();
    let terrain_id = terrain.substrate.allocate_stable_id();
    terrain.terrain_objects.insert_constructed(
        {
            let mut terrain =
                TerrainObjectState::for_test(terrain_id, terrain.interner.intern("TREE01"), 1, 1);
            terrain.set_health_for_test(1);
            terrain.set_max_health_for_test(1);
            terrain
        },
        false,
    );
    terrain.terrain_objects.add_to_cell(terrain_id, (1, 1));
    terrain
        .restore_after_snapshot_load()
        .expect("static Terrain has no required Logic slot");

    let mut projectile = Simulation::new();
    let projectile_id = projectile.substrate.allocate_stable_id();
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
    let wave_id = wave.substrate.allocate_stable_id();
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
