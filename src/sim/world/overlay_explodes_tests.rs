//! Conditional retail Overlay explosions through the ordinary damage owners.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::{TargetKind, world_receiver};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::projectile::ProjectileCoord;

fn scene() -> Option<(Simulation, RuleSet)> {
    let battle = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &IniFile::from_str("[TROCK05]\nExplodes=yes\n[Animations]\n999=FIRE3\n"),
    )?;
    let mut rules = battle.rules;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, ".tem", "TEMPERATE");
    rules.bind_effect_assets(&assets, ".tem", "TEMPERATE");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    *sim.overlay_grid_mut_for_test() = Some(OverlayGrid::new(32, 32));
    Some((sim, rules))
}

fn rock(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16)) -> u8 {
    let registry = rules.overlay_registry();
    let overlay = registry.id_for_name("TROCK05").unwrap();
    sim.with_object_placement_scope(|sim| {
        sim.mark_overlay_for_test(rules, registry, cell, overlay)
    });
    assert_eq!(
        sim.overlay_grid().unwrap().cell(cell.0, cell.1).overlay_id,
        Some(overlay)
    );
    overlay
}

#[test]
fn queued_retail_mtnk_ground_attack_reaches_exploding_rock() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::projectile::ProjectileTarget;

    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let registry = rules.overlay_registry();
    for cell in [(14, 13), (12, 12)] {
        assert!(crate::map::playfield::size_diamond_contains(
            sim.playfield_bounds().unwrap().base,
            sim.playfield_size_height().unwrap(),
            cell,
        ));
    }
    let tank = sim
        .spawn_object_with_overlay_registry("MTNK", "Americans", 14, 13, 0, &rules, registry)
        .unwrap();
    sim.spawn_object_with_overlay_registry("TRUCKA", "Russians", 26, 26, 0, &rules, registry)
        .unwrap();
    let overlay = rock(&mut sim, &rules, (12, 12));
    assert!(!registry.flags(overlay).unwrap().legal_target);
    let owner = sim.entities().get(tank).unwrap().owner();
    sim.session.set_current_house_for_test(Some(owner));
    // Forced Cell orders bypass Overlay.LegalTarget (Techno700926/7009B0).
    // Ordinary Unit736E3A -> GetFireError740FD0 must still admit real FireAt.
    sim.queue_command(CommandEnvelope::new(
        owner,
        1,
        Command::ForceAttackCell {
            attacker_id: tank,
            target_rx: 12,
            target_ry: 12,
        },
    ));
    let (mut commanded, mut fired, mut bullet) = (false, false, false);
    for _ in 0..120 {
        let tick = sim.advance_tick(&[], Some(&rules), None, Some(registry), 22);
        assert!(tick.frame_committed);
        commanded |= tick.executed_commands != 0;
        fired |= sim
            .fire_events()
            .iter()
            .any(|event| event.attacker_id == tank);
        bullet |= sim.projectiles().iter().any(|(_, projectile)| {
            projectile.source_id() == tank
                && projectile.target() == ProjectileTarget::Cell { rx: 12, ry: 12 }
                && projectile.in_logic_vector()
        });
        if sim
            .overlay_grid()
            .unwrap()
            .cell(12, 12)
            .overlay_id
            .is_none()
        {
            break;
        }
    }
    assert!(
        commanded,
        "ordinary command admission executed the ground order"
    );
    assert!(
        fired,
        "MTNK must fire; error={:?}, target={:?}, turret={:?}, latch={:?}",
        sim.selected_weapon_fire_error(
            &rules,
            tank,
            TargetKind::Cell(12, 12),
            true,
            Some(registry)
        ),
        sim.entities().get(tank).unwrap().attack_target.target(),
        sim.entities().get(tank).unwrap().barrel_facing,
        sim.entities().get(tank).unwrap().turret_rotation_latch,
    );
    assert!(
        bullet,
        "ordinary 105mm fire must create a live Cannon projectile"
    );
    assert_eq!(
        sim.overlay_grid().unwrap().cell(12, 12).overlay_id,
        None,
        "the real projectile impact reaches the Overlay.Explodes tail"
    );
}

#[test]
fn retail_exploding_overlay_clears_only_identity_and_recurses_with_the_original_house() {
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let cell = (12, 12);
    let target = sim
        .spawn_object_with_overlay_registry(
            "E1",
            "Russians",
            cell.0,
            cell.1,
            0,
            &rules,
            rules.overlay_registry(),
        )
        .unwrap();
    rock(&mut sim, &rules, cell);
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .write_cell_data(cell.0, cell.1, 7);
    assert_eq!(rules.combat_damage.ammo_crate_damage, 200);
    let house = sim.interner.intern("Americans");
    let warhead_ref = sim.interner.intern("SA");
    let warhead = rules.warhead("SA").unwrap();
    assert!(
        !warhead.wall,
        "the bridge-only guard must not suppress the tail"
    );
    // Stock Super has CellSpread=0. Infantry placement481180 chooses a
    // subcell: AreaDamage4898A7/489A91 requires its exact Location, not the
    // nominal cell center, to admit this ordinary receiver.
    let location = crate::sim::movement::ground_pose::object_location(
        sim.entities().get(target).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    let impact = ProjectileCoord::new(location.x, location.y, location.z);
    let rng = sim.rng_state();
    world_receiver::apply_area_damage(
        &mut sim,
        &rules,
        Some(rules.overlay_registry()),
        impact,
        0,
        warhead,
        (0, Some(house), warhead_ref),
        FrameEffects::default(),
    );
    assert_eq!(
        sim.rng_state(),
        rng,
        "damage zero returns before the suffix"
    );
    world_receiver::apply_area_damage(
        &mut sim,
        &rules,
        Some(rules.overlay_registry()),
        impact,
        -1,
        warhead,
        (0, Some(house), warhead_ref),
        FrameEffects::default(),
    );
    let overlay = sim.overlay_grid().unwrap().cell(cell.0, cell.1);
    assert_eq!(
        overlay.overlay_id, None,
        "48A2F0 removes the Explodes overlay"
    );
    assert_eq!(overlay.overlay_data, 7, "48A2F0 preserves the data byte");
    assert_eq!(
        sim.entities().get(target).unwrap().health.current(),
        0,
        "the recursive C4 actually kills the ordinary receiver"
    );
    assert_eq!(
        sim.entities().get(target).unwrap().kill_record.killer(),
        Some(house)
    );
    assert!(
        sim.anims()
            .any(|(_, anim)| sim.interner.resolve(anim.type_id()) == "EXPLOLRG")
    );
    assert!(
        sim.voxel_anims().len() <= 1,
        "first successful debris trial exits the list"
    );
    assert!(sim.path_grid().unwrap().is_walkable(cell.0, cell.1));
}

#[test]
fn retail_barrel_death_checks_cardinal_overlay_before_arming_its_death_fuse() {
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    let cell = (12, 12);
    let neighbor = (13, 12);
    let barrel = sim
        .spawn_object_with_overlay_registry(
            "CAMISC02",
            "Russians",
            cell.0,
            cell.1,
            0,
            &rules,
            rules.overlay_registry(),
        )
        .unwrap();
    let overlay = rock(&mut sim, &rules, neighbor);
    assert!(rules.object("CAMISC02").unwrap().explodes);
    assert!(
        rules
            .anim_type_art_read_states()
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("FIRE3"))
    );
    sim.receive_own_health_c4(
        barrel,
        &rules,
        Some(rules.overlay_registry()),
        FrameEffects::default(),
    );
    assert_eq!(
        sim.overlay_grid()
            .unwrap()
            .cell(neighbor.0, neighbor.1)
            .overlay_id,
        Some(overlay)
    );
    let fires: Vec<_> = sim
        .anims()
        .filter(|(_, anim)| sim.interner.resolve(anim.type_id()) == "FIRE3")
        .collect();
    assert_eq!(
        fires.len(),
        1,
        "DestructionEffects441A31 checks the neighboring overlay"
    );
    let fire = fires[0].1;
    assert_eq!(
        fire.world_coord(),
        crate::sim::anim_class::AnimWorldCoord {
            x: 13 * 256 + 128,
            y: 12 * 256 + 128,
            z: 0
        }
    );
    assert_eq!(fire.draw_flags(), 0x600);
    assert_eq!(fire.z_adjust(), 0);
    assert!(fire.in_logic_vector());
}

#[test]
fn retail_voxel_contact_cap_survives_snapshot_and_piece_retirement() {
    use crate::sim::snapshot::GameSnapshot;
    let Some((mut sim, rules)) = scene() else {
        return;
    };
    // Match Runtime's native per-Type initialization before a save. The arena
    // starts with no ore classes; native map restore rebuilds all registered
    // classes even on this empty overlay grid (runtime1123 / snapshot2204).
    let (width, height) = sim.map_size_diamond().unwrap();
    sim.ore_growth_state.reset_native_tiberium_classes_for_rect(
        (width as u16, height as u16),
        rules.tiberium_types.len(),
        sim.session.binary_frame(),
    );
    let type_id = rules.voxel_anim_type_id_by_name("GASTANK").unwrap();
    let kind = rules.voxel_anim_type(type_id);
    assert_eq!(kind.damage, 20);
    assert_eq!(kind.warhead.as_deref(), Some("TankOGas"));
    let victim = sim
        .spawn_object_with_overlay_registry(
            "E1",
            "Russians",
            12,
            12,
            0,
            &rules,
            rules.overlay_registry(),
        )
        .unwrap();
    sim.substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .health
        .set(3);
    let coord = crate::sim::movement::ground_pose::object_location(
        sim.entities().get(victim).unwrap(),
        sim.resolved_terrain(),
    );
    let position = glam::IVec3::new(coord.x, coord.y, coord.z);
    sim.emit_debris_voxel(type_id, kind, None, position, true, &mut Vec::new())
        .unwrap();
    let piece = sim.voxel_anims().ids()[0];
    let warhead = sim.interner.intern("TankOGas");
    // Voxel74A884..929's shared contact walk and concrete Object receiver.
    // Object5F54A4 caps the *shared Type.Damage packet* to current Health.
    sim.apply_bounce_contact_damage(
        position,
        kind.damage_radius,
        BounceContactDamage::VoxelType(type_id),
        warhead,
        &rules,
        FrameEffects::default(),
    );
    assert_eq!(sim.entities().get(victim).unwrap().health.current(), 0);
    assert_eq!(sim.voxel_type_damage(type_id, &rules), 3);
    sim.uninit_with_context(piece, UninitContext::with_rules(&rules));
    sim.process_pending_delete_with(
        &rules,
        Some(rules.overlay_registry()),
        FrameEffects::default(),
        1,
        sim.session().started(),
    );
    assert!(sim.voxel_anims().is_empty());
    // Isolated snapshot continuation: the native full-load RNG reset remains
    // separate; restore the supplied pre-save cursor for this equality check.
    let rng = sim.scenario_rng.clone();
    let expected_hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "shared Voxel damage", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.scenario_rng = rng;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        sim.resolved_terrain().unwrap().clone(),
        sim.terrain_speed_config.clone(),
        &rules,
    );
    restored.resolve_type_handles(&rules);
    restored
        .restore_map_authority_after_snapshot_load(&rules, rules.overlay_registry(), None)
        .unwrap();
    assert!(restored.voxel_anims().is_empty());
    assert_eq!(restored.voxel_type_damage(type_id, &rules), 3);
    assert_eq!(
        restored.state_hash(),
        expected_hash,
        "source owners={:?}; restored owners={:?}",
        sim.hash_owner_diagnostics_for_test(),
        restored.hash_owner_diagnostics_for_test()
    );
    // No piece owns this mutable value: process retirement transfers it by
    // exact family name even after every piece has physically left storage.
    assert_eq!(
        restored.retire_voxel_type_damage(),
        vec![("GASTANK".to_owned(), 3)]
    );
    assert_eq!(restored.voxel_type_damage(type_id, &rules), 20);
    assert_ne!(restored.state_hash(), expected_hash);
}
