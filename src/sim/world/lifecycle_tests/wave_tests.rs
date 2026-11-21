//! Wave lifecycle tests: pointer expiry of wave owners and targets, terminal
//! wave damage, cliff collapse and the death-weapon craters a fatal hit
//! starts.

use super::*;

fn gsi_01_05_damage_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
        &crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=VICTIM\n1=SUCCESSOR\n2=FIRER\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=DUPBLDG\n\
             [Warheads]\n0=KILLWH\n\
             [VICTIM]\nStrength=30\nArmor=light\n\
             [SUCCESSOR]\nStrength=30\nArmor=light\n\
             [FIRER]\nStrength=30\nArmor=light\nPrimary=SONIC\nElitePrimary=SONICE\n\
             [DUPBLDG]\nStrength=10\nArmor=concrete\n\
             [SONIC]\nDamage=1\nAmbientDamage=10\nWarhead=KILLWH\nIsSonic=yes\n\
             [SONICE]\nDamage=2\nAmbientDamage=15\nWarhead=KILLWH\nIsSonic=yes\n\
             [KILLWH]\nCellSpread=0\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ),
        &crate::rules::ini_parser::IniFile::from_str("[DUPBLDG]\nFoundation=2x1\n"),
    )
    .expect("Logic-slot damage fixture rules")
}

#[test]
fn wave_pointer_expiry_target_survives_dying_then_uninit_starts_decay_tail() {
    let rules = receiver_authority_rules();
    let mut sim = Simulation::new();
    let owner_id = sim.substrate.allocate_stable_id();
    let target_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, owner_id, EntityCategory::Unit);
    insert_entity(&mut sim, target_id, EntityCategory::Unit);
    {
        let owner = sim.substrate.entities.get_mut(owner_id).unwrap();
        owner.position.set_rx_for_test(0);
        owner.position.set_ry_for_test(0);
        owner
            .attack_target
            .set_for_test(Some(TargetKind::Entity(target_id)));
    }
    {
        let target = sim.substrate.entities.get_mut(target_id).unwrap();
        target.position.set_rx_for_test(4);
        target.position.set_ry_for_test(0);
        target.health.set_for_test(0);
        target.death.set_dying_for_test(true);
    }

    let wave_id = sim.substrate.allocate_stable_id();
    let wave = Wave::new_owned(
        0,
        owner_id,
        TargetKind::Entity(target_id),
        ProjectileCoord::new(128, 128, 0),
        ProjectileCoord::new(4 * 256 + 128, 128, 50),
    );
    sim.admit_wave(wave_id, wave);
    sim.active_wave_links.insert(owner_id, wave_id);

    let represented = sim.wave_update_context(sim.waves.get(wave_id).unwrap(), &rules);
    assert_eq!(
        represented.target_position,
        Some(ProjectileCoord::new(4 * 256 + 128, 128, 0)),
        "health zero and the death animation do not expire a Wave pointer",
    );
    let _ = sim
        .waves
        .advance_one(wave_id, represented, sim.resolved_terrain.as_ref())
        .expect("represented Wave AI");
    assert!(sim.waves.get(wave_id).unwrap().active_geometry());
    assert!(!sim.waves.get(wave_id).unwrap().decaying());

    sim.uninit_with_rules(target_id, &rules);

    let wave = sim.waves.get(wave_id).expect("Wave remains represented");
    assert_eq!(wave.owner_id(), Some(owner_id));
    assert_eq!(
        wave.target_ref(),
        None,
        "UnInit synchronously expires +0xAC"
    );
    assert_eq!(sim.active_wave_links.get(&owner_id), Some(&wave_id));
    let expired = sim.wave_update_context(sim.waves.get(wave_id).unwrap(), &rules);
    assert_eq!(expired.target_position, None);
    let _ = sim
        .waves
        .advance_one(wave_id, expired, sim.resolved_terrain.as_ref())
        .expect("post-expiry Wave AI");
    let wave = sim.waves.get(wave_id).unwrap();
    assert!(!wave.active_geometry());
    assert!(wave.decaying(), "null target activates the decay/fade tail");
}

#[test]
fn wave_pointer_expiry_owner_allows_dying_damage_then_uninit_nulls_later_calls() {
    let rules = gsi_01_05_damage_rules();
    let mut sim = Simulation::new();
    let owner_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, owner_id, EntityCategory::Unit);
    {
        let owner = sim.substrate.entities.get_mut(owner_id).unwrap();
        owner.set_type_ref_for_test(sim.interner.intern("FIRER"));
        owner.health.set_for_test(0);
        owner.death.set_dying_for_test(true);
        owner
            .attack_target
            .set_for_test(Some(TargetKind::Cell(4, 5)));
    }
    let receiver_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, receiver_id, EntityCategory::Unit);
    {
        let receiver = sim.substrate.entities.get_mut(receiver_id).unwrap();
        receiver.set_type_ref_for_test(sim.interner.intern("VICTIM"));
        receiver.health = Health::new(30);
    }
    assert!(matches!(
        sim.try_reveal_entity(receiver_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let wave_id = sim.substrate.allocate_stable_id();
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
        ares_weapon: None,
    };
    assert!(
        sim.wave_update_context(
            sim.waves.get(wave_id).unwrap(),
            crate::rules::ruleset::RuleSet::empty_for_test(),
        )
        .owner_position
        .is_some(),
        "the dying-but-represented owner still supplies live Wave coordinates",
    );
    assert_eq!(sim.active_wave_links.get(&owner_id), Some(&wave_id));

    sim.commit_logic_wave_damage_request(
        &rules,
        None,
        &request,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(receiver_id)
            .unwrap()
            .health
            .current(),
        20,
        "DamageArea consults the represented owner pointer, not health/dying state",
    );

    sim.uninit(owner_id);
    assert_eq!(sim.waves.get(wave_id).unwrap().owner_id(), None);
    assert_eq!(
        sim.waves.get(wave_id).unwrap().target_ref(),
        Some(TargetKind::Cell(4, 5)),
        "owner expiry does not clear an unrelated CellClass target",
    );
    assert!(!sim.active_wave_links.contains_key(&owner_id));

    sim.commit_logic_wave_damage_request(
        &rules,
        None,
        &request,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(receiver_id)
            .unwrap()
            .health
            .current(),
        20,
        "the now-null live owner makes every later DamageArea call a no-op",
    );
}

#[test]
fn wave_pointer_expiry_ignores_unrelated_object_and_preserves_owner_link() {
    let mut sim = Simulation::new();
    let owner_id = sim.substrate.allocate_stable_id();
    let target_id = sim.substrate.allocate_stable_id();
    let unrelated_id = sim.substrate.allocate_stable_id();
    for id in [owner_id, target_id, unrelated_id] {
        insert_entity(&mut sim, id, EntityCategory::Unit);
    }
    sim.substrate
        .entities
        .get_mut(owner_id)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(target_id)));
    let wave_id = sim.substrate.allocate_stable_id();
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
    assert_eq!(wave.owner_id(), Some(owner_id));
    assert_eq!(wave.target_ref(), Some(TargetKind::Entity(target_id)));
    assert_eq!(sim.active_wave_links.get(&owner_id), Some(&wave_id));
}

#[test]
fn gsi_01_05_lethal_bullet_commits_receiver_before_retirement_and_double_compaction() {
    let rules = gsi_01_05_damage_rules();
    let mut sim = Simulation::new();
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

    let successor_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, successor_id, EntityCategory::Unit);
    let successor_type = sim.interner.intern("SUCCESSOR");
    sim.substrate
        .entities
        .get_mut(successor_id)
        .unwrap()
        .set_type_ref_for_test(successor_type);
    assert!(matches!(
        sim.try_reveal_entity(successor_id, common_raw_request(8, 9, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let projectile_id = sim.substrate.allocate_stable_id();
    let impact = ProjectileCoord::new(5 * 256 + 128, 6 * 256 + 128, 0);
    let mut spawn = gsi_05_02_projectile(crate::sim::combat::RAD_NO_ATTACKER, Some(0));
    spawn.origin = impact;
    spawn.target = ProjectileTarget::Entity(victim_id);
    spawn.initial_target_position = impact;
    spawn.payload = ProjectilePayload::new(10, sim.interner.intern("KILLWH"), None, false);
    sim.admit_projectile(projectile_id, spawn);
    // Supply an already-arrived impact boundary for the lethal transaction;
    // native launch-floor placement is covered by projectile_trailer tests.
    sim.projectiles
        .get_mut(projectile_id)
        .unwrap()
        .set_position_for_test(impact);
    sim.set_logic_order_for_test(vec![projectile_id, victim_id, successor_id]);
    sim.lifecycle_test_events.clear();

    let mut visited = Vec::new();
    sim.for_each_live_object(|sim, id| {
        visited.push(id);
        assert!(sim.object_ai_visit_one(id, Some(&rules), &mut ObjectAiCtx::default()));
    });

    assert_eq!(visited, vec![projectile_id]);
    assert_eq!(sim.live_object_order_snapshot(), vec![successor_id]);
    let victim = sim
        .substrate
        .entities
        .get(victim_id)
        .expect("fatal receiver remains resolvable until common drain");
    assert!(!victim.lifecycle.object_alive());
    assert!(victim.lifecycle.in_limbo());
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
    assert_eq!(
        sim.substrate.pending_delete.object_ids(),
        vec![victim_id, projectile_id]
    );
    assert!(sim.projectiles.get(projectile_id).is_some());
    assert!(
        !sim.projectiles
            .get(projectile_id)
            .unwrap()
            .in_logic_vector()
    );

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
    let victim_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, victim_id, EntityCategory::Unit);
    let victim_type = sim.interner.intern("VICTIM");
    sim.substrate
        .entities
        .get_mut(victim_id)
        .unwrap()
        .set_type_ref_for_test(victim_type);
    sim.substrate.entities.get_mut(victim_id).unwrap().health = Health::new(30);
    assert!(matches!(
        sim.try_reveal_entity(victim_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let successor_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, successor_id, EntityCategory::Unit);
    let successor_type = sim.interner.intern("SUCCESSOR");
    sim.substrate
        .entities
        .get_mut(successor_id)
        .unwrap()
        .set_type_ref_for_test(successor_type);
    assert!(matches!(
        sim.try_reveal_entity(successor_id, common_raw_request(8, 9, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let wave_id = sim.substrate.allocate_stable_id();
    let firer_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(firer_id)
        .unwrap()
        .set_type_ref_for_test(sim.interner.intern("FIRER"));
    sim.substrate
        .entities
        .get_mut(firer_id)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(victim_id)));
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Entity(victim_id),
        ProjectileCoord::new(4 * 256, 5 * 256, 0),
        ProjectileCoord::new(5 * 256, 5 * 256, 0),
    );
    wave.set_active_geometry_for_test(false);
    wave.set_decaying_for_test(true);
    wave.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 5)]);
    sim.admit_wave(wave_id, wave);
    sim.set_logic_order_for_test(vec![wave_id, victim_id, successor_id]);
    sim.lifecycle_test_events.clear();

    let mut visited = Vec::new();
    sim.for_each_live_object(|sim, id| {
        visited.push(id);
        assert!(sim.object_ai_visit_one(id, Some(&rules), &mut ObjectAiCtx::default()));
    });

    assert_eq!(visited, vec![wave_id, successor_id]);
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .health
            .current(),
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
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![wave_id]);
    assert_eq!(
        sim.live_object_order_snapshot(),
        vec![victim_id, successor_id]
    );
    assert!(sim.waves.get(wave_id).is_some());
    assert!(!sim.waves.get(wave_id).unwrap().in_logic_vector());
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
    let firer_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    {
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.set_type_ref_for_test(sim.interner.intern("FIRER"));
        firer
            .attack_target
            .set_for_test(Some(TargetKind::Cell(4, 5)));
    }
    let wave_id = sim.substrate.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Cell(4, 5),
        ProjectileCoord::new(4 * 256, 5 * 256, 0),
        ProjectileCoord::new(5 * 256, 5 * 256, 0),
    );
    wave.set_active_geometry_for_test(false);
    wave.set_decaying_for_test(true);
    wave.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    sim.admit_wave(wave_id, wave);
    sim.active_wave_links.insert(firer_id, wave_id);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x45_4d_50_54_59);
    let expected_rng = sim.scenario_rng.logical_state();

    assert!(sim.object_ai_visit_one(wave_id, Some(&rules), &mut ObjectAiCtx::default()));

    assert_eq!(sim.scenario_rng.logical_state(), expected_rng);
    assert!(!sim.active_wave_links.contains_key(&firer_id));
    assert!(sim.dynamic_terrain_cells.is_empty());
    assert!(sim.dirty_cells.radar().is_empty());
    assert!(sim.dirty_cells.tactical_for_test().is_empty());
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![wave_id]);
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
    let second_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, second_id, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(second_id)
        .unwrap()
        .set_type_ref_for_test(sim.interner.intern("SECOND"));
    assert!(matches!(
        sim.try_reveal_entity(second_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let first_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, first_id, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(first_id)
        .unwrap()
        .set_type_ref_for_test(sim.interner.intern("FIRST"));
    assert!(matches!(
        sim.try_reveal_entity(first_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let next_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, next_id, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(next_id)
        .unwrap()
        .set_type_ref_for_test(sim.interner.intern("NEXT"));
    assert!(matches!(
        sim.try_reveal_entity(next_id, common_raw_request(5, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));

    let firer_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    {
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.set_type_ref_for_test(sim.interner.intern("FIRER"));
        firer.veterancy.set_rank_for_test(200);
        firer
            .attack_target
            .set_for_test(Some(TargetKind::Entity(next_id)));
    }

    let wave_id = sim.substrate.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Entity(next_id),
        ProjectileCoord::new(4 * 256, 5 * 256, 0),
        ProjectileCoord::new(6 * 256, 5 * 256, 0),
    );
    wave.set_active_geometry_for_test(false);
    wave.set_decaying_for_test(true);
    wave.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.replace_recorded_cells(vec![
        WaveRecordedCell::real(4, 5),
        WaveRecordedCell::real(5, 5),
    ]);
    sim.admit_wave(wave_id, wave);
    sim.active_wave_links.insert(firer_id, wave_id);
    sim.lifecycle_test_events.clear();

    assert!(sim.object_ai_visit_one(wave_id, Some(&rules), &mut ObjectAiCtx::default()));

    assert_eq!(
        sim.substrate
            .entities
            .get(first_id)
            .unwrap()
            .health
            .current(),
        93
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(second_id)
            .unwrap()
            .health
            .current(),
        93,
        "second occupant receives the first callback's 7-point mutable value",
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(next_id)
            .unwrap()
            .health
            .current(),
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

/// A unit still inside the War Factory in its radio contact 0 returns at the
/// head of `UnitClass::ReceiveDamage` (`0x00737CB6..0x00737D2E`): it takes no
/// damage and leaves the wave's shared damage alone, so the cell's next
/// occupant takes the whole `AmbientDamage=`, not the flak-halved value.
#[test]
fn wave_shared_damage_passes_a_unit_inside_its_war_factory() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=FIRST\n1=SECOND\n2=FIRER\n\
             [AircraftTypes]\n[BuildingTypes]\n0=WEAP\n\
             [FIRST]\nStrength=100\nArmor=flak\n\
             [SECOND]\nStrength=100\nArmor=none\n\
             [WEAP]\nStrength=1000\nArmor=concrete\nWeaponsFactory=yes\n\
             [FIRER]\nStrength=100\nArmor=light\nPrimary=SONIC\n\
             [SONIC]\nDamage=4\nAmbientDamage=10\nWarhead=WH\nIsSonic=yes\n\
             [WH]\nCellSpread=0\nPercentAtMax=1\n\
             Verses=100%,50%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("Wave war-factory fixture");
    let mut sim = Simulation::new();
    let unit = |sim: &mut Simulation, kind: &str| {
        let id = sim.substrate.allocate_stable_id();
        insert_entity(sim, id, EntityCategory::Unit);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_type_ref_for_test(sim.interner.intern(kind));
        assert!(matches!(
            sim.try_reveal_entity(id, common_raw_request(4, 5, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));
        id
    };
    let second_id = unit(&mut sim, "SECOND");
    let first_id = unit(&mut sim, "FIRST");
    let factory_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, factory_id, EntityCategory::Structure);
    {
        let factory = sim.substrate.entities.get_mut(factory_id).unwrap();
        factory.set_type_ref_for_test(sim.interner.intern("WEAP"));
        factory
            .seldom
            .type_facts
            .set_foundation_for_test("1x1".to_string().into());
    }
    assert!(matches!(
        sim.try_reveal_entity(factory_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    sim.substrate
        .entities
        .get_mut(first_id)
        .unwrap()
        .seldom
        .radio_contacts
        .insert(factory_id)
        .unwrap();

    let firer_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    {
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.set_type_ref_for_test(sim.interner.intern("FIRER"));
        firer
            .attack_target
            .set_for_test(Some(TargetKind::Entity(second_id)));
    }
    let wave_id = sim.substrate.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Entity(second_id),
        ProjectileCoord::new(4 * 256, 5 * 256, 0),
        ProjectileCoord::new(6 * 256, 5 * 256, 0),
    );
    wave.set_active_geometry_for_test(false);
    wave.set_decaying_for_test(true);
    wave.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 5)]);
    sim.admit_wave(wave_id, wave);

    assert!(sim.object_ai_visit_one(wave_id, Some(&rules), &mut ObjectAiCtx::default()));
    let health = |id: u64| sim.substrate.entities.get(id).unwrap().health.current();
    assert_eq!(health(first_id), 100);
    assert_eq!(health(second_id), 90);
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
                 [BLDG]\nStrength=100\nArmor=none\n\
                 [FIRER]\nStrength=100\nArmor=light\nPrimary=SONIC\n\
                 [TREE01]\nStrength=100\nArmor=wood\nImmune=no\n\
                 [SONIC]\nDamage=4\nAmbientDamage=10\nWarhead=WH\nIsSonic=yes\n\
                 [WH]\nWood={}\nCellSpread=0\nPercentAtMax=1\n\
                 Verses=100%,100%,100%,100%,100%,100%,50%,100%,100%,100%,100%\n",
                if wood { "yes" } else { "no" },
            )),
        )
        .expect("Wave Terrain fixture");
        let mut sim = Simulation::new();

        let unit_id = sim.substrate.allocate_stable_id();
        insert_entity(&mut sim, unit_id, EntityCategory::Unit);
        sim.substrate
            .entities
            .get_mut(unit_id)
            .unwrap()
            .set_type_ref_for_test(sim.interner.intern("UNIT"));
        assert!(matches!(
            sim.try_reveal_entity(unit_id, common_raw_request(4, 5, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));

        let building_id = sim.substrate.allocate_stable_id();
        insert_entity(&mut sim, building_id, EntityCategory::Structure);
        {
            let building = sim.substrate.entities.get_mut(building_id).unwrap();
            building.set_type_ref_for_test(sim.interner.intern("BLDG"));
            building
                .seldom
                .type_facts
                .set_foundation_for_test("1x1".to_string().into());
        }
        assert!(matches!(
            sim.try_reveal_entity(building_id, common_raw_request(4, 5, 0, 128, 128)),
            RevealOutcome::Revealed { .. }
        ));

        let terrain_id = sim.substrate.allocate_stable_id();
        sim.terrain_objects.objects_mut_for_test().insert(
            terrain_id,
            TerrainObjectState::for_test(terrain_id, sim.interner.intern("TREE01"), 4, 5),
        );
        sim.terrain_objects
            .cell_index_mut_for_test()
            .insert((4, 5), terrain_id);

        let firer_id = sim.substrate.allocate_stable_id();
        insert_entity(&mut sim, firer_id, EntityCategory::Unit);
        {
            let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
            firer.set_type_ref_for_test(sim.interner.intern("FIRER"));
            firer
                .attack_target
                .set_for_test(Some(TargetKind::Entity(building_id)));
        }
        let wave_id = sim.substrate.allocate_stable_id();
        let mut wave = Wave::new_owned(
            0,
            firer_id,
            TargetKind::Entity(building_id),
            ProjectileCoord::new(4 * 256, 5 * 256, 0),
            ProjectileCoord::new(6 * 256, 5 * 256, 0),
        );
        wave.set_active_geometry_for_test(false);
        wave.set_decaying_for_test(true);
        wave.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
        wave.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
        wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 5)]);
        sim.admit_wave(wave_id, wave);
        sim.lifecycle_test_events.clear();

        assert!(sim.object_ai_visit_one(wave_id, Some(&rules), &mut ObjectAiCtx::default()));
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
            sim.substrate
                .entities
                .get(unit_id)
                .unwrap()
                .health
                .current(),
            sim.terrain_objects.objects()[&terrain_id].health(),
            sim.substrate
                .entities
                .get(building_id)
                .unwrap()
                .health
                .current(),
        )
    }

    let (selected, unit_health, tree_health, building_health) = run(true);
    assert_eq!(
        selected,
        vec![1, 3, 2],
        "one-cell order is nonbuilding, Terrain, then Building",
    );
    assert_eq!(unit_health, 90);
    assert_eq!(tree_health, 95);
    assert_eq!(
        building_health, 95,
        "the tree passes its five-point damage to the building"
    );

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
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = Simulation::new();
    let firer_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    {
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.set_type_ref_for_test(sim.interner.intern("FIRER"));
        firer
            .attack_target
            .set_for_test(Some(TargetKind::Cell(4, 1)));
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

    let wave_id = sim.substrate.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Cell(4, 1),
        ProjectileCoord::new(0, 0, 0),
        ProjectileCoord::new(4 * 256, 256, 50),
    );
    wave.set_active_geometry_for_test(false);
    wave.set_decaying_for_test(true);
    wave.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.replace_recorded_cells(vec![WaveRecordedCell::real(4, 1)]);
    sim.admit_wave(wave_id, wave);

    sim.scenario_rng = crate::sim::rng::SimRng::new(0x57_41_4c_4c);
    let mut expected = sim.scenario_rng.clone();
    // Ares WaveClass_DamageCell_Wall (1005D7A0, hook 0x75F46E) leaves the
    // wall to a `Wall=yes` warhead; WH is not one, so no wall roll.
    if !rules.ares() {
        let _wall_roll = expected.next_range_u32_inclusive(0, 100);
    }
    let _cliff_chance_roll = expected.next_range_u32_inclusive(0, 99);

    assert!(sim.object_ai_visit_one(
        wave_id,
        Some(&rules),
        &mut ObjectAiCtx {
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
         [Warheads]\n0=KILLWH\n1=CRATERWH\n[SmudgeTypes]\n0=CR1\n[Animations]\n0=CRATERANIM\n\
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
    rules.replace_art_registry_for_test(crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[CRATERANIM]\nCrater=yes\nScorch=no\nStart=0\n"),
    ));
    // One raw frame: no middle frame, so Start runs Middle at construction.
    rules.bind_anim_frame_count_for_test("CRATERANIM", 1);
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
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
    sim.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(10, 10);
    let victim_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, victim_id, EntityCategory::Unit);
    let victim = sim.substrate.entities.get_mut(victim_id).unwrap();
    victim.set_type_ref_for_test(sim.interner.intern("VICTIM"));
    victim.health = Health::new(1);
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
            &mut crate::sim::world::FrameEffects::default(),
        );
    } else {
        let firer_id = sim.substrate.allocate_stable_id();
        insert_entity(&mut sim, firer_id, EntityCategory::Unit);
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.set_type_ref_for_test(sim.interner.intern("FIRER"));
        firer
            .attack_target
            .set_for_test(Some(TargetKind::Entity(victim_id)));
        let wave_id = sim.substrate.allocate_stable_id();
        let mut wave = Wave::new_owned(
            0,
            firer_id,
            TargetKind::Entity(victim_id),
            ProjectileCoord::new(4 * 256, 5 * 256, 0),
            ProjectileCoord::new(5 * 256, 5 * 256, 0),
        );
        wave.set_active_geometry_for_test(false);
        wave.set_decaying_for_test(true);
        wave.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
        wave.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
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
            &mut ObjectAiCtx {
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
    let overlay_registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let art_ini = crate::rules::ini_parser::IniFile::from_str(
        "[XGRYMED1]\nEnd=1\nRate=1\nRandomRate=900,300\n\
             [XGRYMED2]\nEnd=1\nRate=1\nRandomRate=900,300\n\
             [XGRYSML1]\nEnd=1\nRate=1\nRandomRate=900,300\n",
    );
    let mut rules =
        crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini)
            .expect("cliff body rules");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    for name in ["XGRYMED1", "XGRYMED2", "XGRYSML1"] {
        art.bind_anim_frame_count_for_test(name, 1);
    }
    rules.install_art_data(art);
    let mut sim = Simulation::new();
    let firer_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    {
        let firer = sim.substrate.entities.get_mut(firer_id).unwrap();
        firer.set_type_ref_for_test(sim.interner.intern("FIRER"));
        firer
            .attack_target
            .set_for_test(Some(TargetKind::Cell(4, 1)));
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
                    &ZonePrecheckExclusions::default(),
                    None,
                )
                .unwrap(),
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
        sim.resolved_terrain.as_ref().unwrap(),
        &[],
        16,
        16,
    );
    assert_eq!(remote_bridge_route(&without_bridge_records), (false, false));
    let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(16, 16);
    let decal = overlay_registry.id_for_name("DECAL").expect("test decal");
    overlay.place_overlay(0, 0, decal, 11);
    overlay.place_overlay(1, 0, decal, 12);
    sim.overlay_grid = Some(overlay);
    let mut smudge = crate::sim::smudge_grid::SmudgeGrid::new(16, 16);
    for (rx, frame_offset) in [(0, 0), (1, 1)] {
        smudge.test_force_set(
            rx,
            0,
            crate::sim::smudge_grid::SmudgeCell {
                type_id: Some(9),
                frame_offset,
            },
        );
    }
    sim.smudge_grid = Some(smudge);

    let wave_id = sim.substrate.allocate_stable_id();
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Cell(4, 1),
        ProjectileCoord::new(0, 0, 0),
        ProjectileCoord::new(4 * 256, 256, 50),
    );
    wave.set_active_geometry_for_test(false);
    wave.set_decaying_for_test(true);
    wave.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
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
                let jitter_y = expected_rng.next_range_u32_inclusive(0, 16) as i32 - 8;
                let jitter_x = expected_rng.next_range_u32_inclusive(0, 24) as i32 - 12;
                let delay = expected_rng.next_range_u32_inclusive(0, 2) as i32;
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
        &mut ObjectAiCtx {
            overlay_registry: Some(&overlay_registry),
            ..ObjectAiCtx::default()
        },
    ));

    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    assert_eq!(sim.dynamic_terrain_cells.len(), 20);
    let published = sim
        .lighting_sources
        .pending()
        .iter()
        .find_map(|event| match event {
            crate::sim::light_sources::LightingEvent::CellLevels { cells } => Some(cells),
            _ => None,
        })
        .expect("the actual cliff stamp publishes Cell+11B before later callbacks");
    assert!(!published.is_empty());
    for &(coord, level) in published {
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(coord.0, coord.1)
                .unwrap()
                .level,
            level
        );
    }
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
        sim.overlay_grid.as_ref(),
    );
    for rx in 9..=12 {
        assert_eq!(
            sim.path_grid.as_ref().unwrap().cell(rx, 10),
            canonical_path.cell(rx, 10)
        );
    }
    assert_eq!(sim.dirty_cells.radar().len(), 20);
    assert_eq!(sim.dirty_cells.tactical_for_test().len(), 20);
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
                    .position(|name| *name == sim.interner.resolve(anim.type_id()))
                    .expect("one of the three cliff anim types"),
                anim.world_coord(),
                anim.runtime().delay_remaining,
                anim.runtime().rate_reload,
                anim.draw_flags(),
                anim.runtime().loop_remaining,
                anim.runtime().constructor_reverse,
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
            .target()
            .is_none()
    );

    // GameSnapshot's established full-deserialize seam canonicalizes Scenario
    // RNG to seed zero; the collapse draw-order assertion above already pins
    // the live cursor before normalizing this independent snapshot fixture.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_dynamic_terrain = sim.dynamic_terrain_cells.clone();
    // Simulation's saved stores here use stable vectors/BTreeMaps; runtime
    // waypoint HashMaps belong to SimResources and are not serialized. The
    // ZoneGrid encoding includes native retained base bytes, not hierarchy
    // history, so this compares exact saved authority without normalizing a
    // live graph merely to make its hash agree with ordinary load.
    let expected_authority = save_codec::serialize(&sim).unwrap();
    let expected_base = save_codec::serialize(sim.zone_grid.as_ref().unwrap()).unwrap();
    let bytes = GameSnapshot::save(&sim, 0, 0, "collapsed-dcliff", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("collapsed cliff snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("collapsed cliff stable identities");
    assert!(
        restored
            .zone_grid
            .as_ref()
            .unwrap()
            .is_native_load_pending()
    );
    assert_eq!(
        save_codec::serialize(&restored).unwrap(),
        expected_authority
    );
    assert_eq!(
        save_codec::serialize(restored.zone_grid.as_ref().unwrap()).unwrap(),
        expected_base
    );
    restored.rebuild_caches_after_load(pristine_terrain, Default::default(), &rules);
    restored
        .restore_map_authority_after_snapshot_load(&rules, &overlay_registry, None)
        .expect("dynamic cliff terrain reprojects over the pristine map");
    assert_eq!(restored.dynamic_terrain_cells, expected_dynamic_terrain);
    assert!(
        !restored
            .zone_grid
            .as_ref()
            .unwrap()
            .is_native_load_pending()
    );
    assert_eq!(
        save_codec::serialize(&restored).unwrap(),
        expected_authority
    );
    assert_eq!(
        save_codec::serialize(restored.zone_grid.as_ref().unwrap()).unwrap(),
        expected_base
    );
    // Native LoadContent67E8CD ->581F50 clears the incremental hierarchy
    // and rebuilds it from this saved base. The physical Anytown restore
    // packet establishes that its history-sensitive live hash may change.
    assert_eq!(
        remote_bridge_route(restored.zone_grid.as_ref().unwrap()),
        (true, true)
    );
    let restored_terrain = restored.resolved_terrain.as_ref().unwrap();
    for (&(rx, ry), expected) in expected_dynamic_terrain.iter() {
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
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);

    let building_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, building_id, EntityCategory::Structure);
    let building_type = sim.interner.intern("DUPBLDG");
    {
        let building = sim.substrate.entities.get_mut(building_id).unwrap();
        building.set_type_ref_for_test(building_type);
        building
            .seldom
            .type_facts
            .set_foundation_for_test("2x1".to_string().into());
        building.health.set_for_test(10);
    }
    assert!(matches!(
        sim.try_reveal_entity(building_id, common_raw_request(4, 5, 0, 128, 128)),
        RevealOutcome::Revealed { .. }
    ));
    assert!(sim.substrate.occupancy.contains_entity(4, 5, building_id));
    assert!(sim.substrate.occupancy.contains_entity(5, 5, building_id));

    let wave_id = sim.substrate.allocate_stable_id();
    let firer_id = sim.substrate.allocate_stable_id();
    insert_entity(&mut sim, firer_id, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(firer_id)
        .unwrap()
        .set_type_ref_for_test(sim.interner.intern("FIRER"));
    sim.substrate
        .entities
        .get_mut(firer_id)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(building_id)));
    let mut wave = Wave::new_owned(
        0,
        firer_id,
        TargetKind::Entity(building_id),
        ProjectileCoord::new(4 * 256, 5 * 256, 0),
        ProjectileCoord::new(6 * 256, 5 * 256, 0),
    );
    wave.set_active_geometry_for_test(false);
    wave.set_decaying_for_test(true);
    wave.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fa9_9999_a000_0000));
    wave.replace_recorded_cells(vec![
        WaveRecordedCell::real(4, 5),
        WaveRecordedCell::real(5, 5),
    ]);
    sim.admit_wave(wave_id, wave);
    sim.set_logic_order_for_test(vec![wave_id, building_id]);
    sim.lifecycle_test_events.clear();

    assert!(sim.object_ai_visit_one(wave_id, Some(&rules), &mut ObjectAiCtx::default()));

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
    assert!(!building.lifecycle.object_alive());
    assert!(building.lifecycle.in_limbo());
    assert!(!sim.substrate.occupancy.contains_entity(4, 5, building_id));
    assert!(!sim.substrate.occupancy.contains_entity(5, 5, building_id));
    assert_eq!(
        sim.substrate.pending_delete.object_ids(),
        vec![building_id, wave_id]
    );
}
