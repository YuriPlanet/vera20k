//! Weapon fire and wave tests: sonic, psychedelic and Ares laser waves (constructor,
//! tracking, damage, tail order and bridge-plane selection) and a guard-infantry
//! acquire, return-fire and duel scenario.

use super::*;

fn sonic_wave_test_rules(range: u8) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=DLPH\n1=TARGET\n\n\
         [DLPH]\nStrength=200\nArmor=light\nSpeed=8\nPrimary=SonicZap\nElitePrimary=SonicZapE\n\n\
         [TARGET]\nStrength=100\nArmor=wood\n\n\
         [SonicZap]\nDamage=4\nAmbientDamage=10\nROF=20\nRange={range}\nProjectile=Sonic\nSpeed=100\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [SonicZapE]\nDamage=8\nAmbientDamage=15\nROF=20\nRange={range}\nProjectile=Sonic\nSpeed=100\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [Sonic]\nLevel=yes\n\n\
         [SonicWH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    )))
    .expect("Sonic Wave fixture")
}

/// Wave passes its firer and a null source House (75F417..75F42C).
/// Psychedelic checks that nullable House at 701CE5, even for friendly fire.
#[test]
fn stock_psychedelic_wave_can_affect_the_firers_own_house() {
    use crate::sim::wave::{Wave, WaveRecordedCell};

    let _stock = crate::rules::ares_switch::read_ares_way(false);
    let rules = combat_test_rules_with_override(
        "[MTNK]\nPrimary=SonicZap\n[SonicZap]\nIsSonic=yes\nDamage=1\nAmbientDamage=20\nWarhead=SA\n\
         [SA]\nPsychedelic=yes\nAffectsAllies=yes\n",
    );
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 42);
    let firer = sim
        .spawn_object("MTNK", "Americans", 6, 10, 0, &rules)
        .unwrap();
    let friendly = sim
        .spawn_object("E1", "Americans", 7, 10, 0, &rules)
        .unwrap();
    let hostile = sim
        .spawn_object("E1", "Russians", 8, 10, 0, &rules)
        .unwrap();
    let mut wave = Wave::new_owned(
        0,
        firer,
        TargetKind::Cell(8, 10),
        ProjectileCoord::new(6 * 256, 10 * 256, 0),
        ProjectileCoord::new(8 * 256, 10 * 256, 0),
    );
    wave.set_active_geometry_for_test(false);
    wave.set_decaying_for_test(true);
    wave.replace_recorded_cells(vec![
        WaveRecordedCell::real(7, 10),
        WaveRecordedCell::real(8, 10),
    ]);
    let id = sim.substrate.allocate_stable_id();
    sim.admit_wave(id, wave);
    assert!(sim.object_ai_visit_one(id, Some(&rules), &mut Default::default()));
    let berserk = |id| sim.entities().get(id).unwrap().berserk.is_active();
    assert!(berserk(hostile), "the hostile GI went berserk");
    assert!(berserk(friendly), "the friendly GI should also go berserk");
}

fn sonic_tail_order_test_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=DLPH\n1=LATER\n2=TARGET\n\n\
         [DLPH]\nStrength=200\nArmor=light\nSpeed=8\nSight=8\nPrimary=SonicZap\n\n\
         [LATER]\nStrength=200\nArmor=light\nSpeed=8\nSight=8\nPrimary=LaterGun\n\n\
         [TARGET]\nStrength=100\nArmor=none\nSpeed=1\n\n\
         [SonicZap]\nDamage=4\nAmbientDamage=10\nROF=20\nRange=6\nProjectile=Sonic\nSpeed=100\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [LaterGun]\nDamage=7\nROF=20\nRange=6\nWarhead=LaterWH\n\n\
         [Sonic]\nLevel=yes\n\n\
         [SonicWH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\n\
         [LaterWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("Sonic Logic-tail ordering fixture")
}

fn admit_test_wave(sim: &mut Simulation, rules: &RuleSet, event: &SimFireEvent) {
    let wave = sim
        .prepare_fired_wave(rules, event, 0)
        .expect("wave prepared");
    sim.admit_fired_wave(event.attacker_id, wave, rules);
}

fn sonic_fire_event(
    sim: &mut Simulation,
    rules: &RuleSet,
    attacker_id: u64,
    target_id: u64,
) -> SimFireEvent {
    let entity = sim.substrate.entities.get_mut(attacker_id).unwrap();
    // These fixtures supply a successful FireAt caller, whose TarCom is
    // already admitted. Preserve the explicit Cell-target fixture.
    if entity.attack_target.target().is_none() {
        entity
            .attack_target
            .set_for_test(Some(TargetKind::Entity(target_id)));
    }
    let entity = sim.substrate.entities.get(attacker_id).unwrap();
    let object = sim.interner.object_type(entity.type_ref(), rules).unwrap();
    let fire_coord = crate::sim::combat::fire_coord::fire_coordinate(
        sim,
        rules,
        &crate::sim::combat::fire_coord::FireSource::of_entity(entity),
        object,
        0,
        0,
        crate::rules::flh::Flh::default(),
    )
    .coord;
    SimFireEvent {
        attacker_id,
        weapon_slot: crate::sim::combat::combat_weapon::WeaponSlot::Primary,
        weapon_id: sim.interner.intern("SonicZap"),
        facing: 0,
        origin_snapshot: FireOriginSnapshot { facing: 0 },
        target: crate::sim::combat::TargetKind::Entity(target_id),
        report_sound_id: None,
        fire_coord,
        fire_offset_y: 0,
        muzzle_anim: None,
        occupied_building: false,
        firer_category: EntityCategory::Unit,
    }
}

#[test]
fn sonic_constructor_dead_pointer_link_lives_until_deferred_delete_at_239() {
    let rules = sonic_wave_test_rules(6);
    let mut sim = Simulation::new();
    let firer_id = sim.substrate.allocate_stable_id();
    let target_id = sim.substrate.allocate_stable_id();
    let owner = sim.interner.intern("Americans");
    let firer_type = sim.interner.intern("DLPH");
    let target_type = sim.interner.intern("TARGET");
    let mut firer = GameEntity::test_default(firer_id, "DLPH", "Americans", 0, 0);
    firer.set_owner_for_test(owner);
    firer.set_type_ref_for_test(firer_type);
    firer.position.set_sub_x_for_test(SimFixed::from_num(0));
    firer.position.set_sub_y_for_test(SimFixed::from_num(0));
    let mut target = GameEntity::test_default(target_id, "TARGET", "Russians", 0, 0);
    target.set_type_ref_for_test(target_type);
    target.position.set_sub_x_for_test(SimFixed::from_num(239));
    target.position.set_sub_y_for_test(SimFixed::from_num(0));
    sim.substrate.entities.insert(firer);
    sim.substrate.entities.insert(target);
    let event = sonic_fire_event(&mut sim, &rules, firer_id, target_id);

    let cursor = sim.native_identity_cursor();
    admit_test_wave(&mut sim, &rules, &event);
    assert_eq!(
        sim.native_identity_cursor(),
        cursor,
        "short ctor abort precedes CreateID"
    );

    let wave_id = *sim
        .active_wave_links
        .get(&firer_id)
        .expect("dead pointer stored");
    assert!(sim.waves.get(wave_id).is_some());
    assert!(!sim.waves.get(wave_id).unwrap().in_logic_vector());
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![wave_id]);

    sim.process_pending_delete();
    assert!(!sim.active_wave_links.contains_key(&firer_id));
    assert!(sim.waves.get(wave_id).is_none());
}

#[test]
fn target_coord_offset_moves_initial_wave_endpoint_but_not_tracking() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = fixture.rules;
    let mut sim = Simulation::new();
    let dolphin = sim
        .spawn_object("DLPH", "Americans", 2, 2, 0, &rules)
        .unwrap();
    let yard = sim
        .spawn_object("GAYARD", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let centre = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(yard).unwrap(),
        None,
    );
    let event = sonic_fire_event(&mut sim, &rules, dolphin, yard);
    let wave = sim.prepare_fired_wave(&rules, &event, 0).unwrap();
    // Ares CreateWave passes the launch source first instead.
    if !rules.ares() {
        assert_eq!(
            wave.source(),
            crate::sim::projectile::ProjectileCoord::new(centre.x + 300, centre.y + 200, centre.z,)
        );
    }
    admit_test_wave(&mut sim, &rules, &event);
    let id = sim.active_wave_links[&dolphin];
    assert_eq!(
        sim.wave_update_context(sim.waves.get(id).unwrap(), &rules)
            .target_position,
        Some(crate::sim::projectile::ProjectileCoord::new(
            centre.x, centre.y, centre.z
        ))
    );
}

fn ares_sonic_rules(weapon_keys: &str) -> RuleSet {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=DLPH\n1=TARGET\n\n\
         [DLPH]\nStrength=200\nArmor=light\nSpeed=8\nPrimary=SonicZap\nElitePrimary=SonicZapE\n\n\
         [TARGET]\nStrength=100\nArmor=wood\n\n\
         [SonicZap]\nDamage=4\nROF=20\nRange=10\nProjectile=Sonic\nSpeed=100\nWarhead=SonicWH\nIsSonic=yes\n{weapon_keys}\n\n\
         [SonicZapE]\nDamage=8\nAmbientDamage=15\nROF=20\nRange=10\nProjectile=Sonic\nSpeed=100\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [Sonic]\nLevel=yes\n\n\
         [SonicWH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    )))
    .expect("Ares Sonic Wave fixture")
}

/// Ares WaveClass_Update_Beam (Ares.dll 10057290, hook 0x762B62): a Sonic
/// wave keeps tracking within its weapon's Range (here ten cells, beyond the
/// stock 2172 leptons) and draws from the target end for a reversing class.
#[test]
fn ares_wave_tracks_within_weapon_range_and_reverses_by_target_class() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    for reverse in [false, true] {
        let rules = ares_sonic_rules(if reverse {
            "Wave.ReverseAgainstVehicles=yes"
        } else {
            ""
        });
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5EED_A2E5);
        let dolphin = sim
            .spawn_object("DLPH", "Americans", 6, 10, 64, &rules)
            .unwrap();
        let target = sim
            .spawn_object("TARGET", "Russians", 16, 10, 0, &rules)
            .unwrap();
        let event = sonic_fire_event(&mut sim, &rules, dolphin, target);
        sim.commit_fired_wave(&rules, &event, 0, 0);
        let wave = sim.waves.get(sim.active_wave_links[&dolphin]).unwrap();
        assert!(
            wave.active_geometry() && !wave.decaying(),
            "reverse={reverse}"
        );
        let aim = sim
            .wave_update_context(wave, &rules)
            .target_position
            .unwrap();
        let expected = if reverse { aim } else { event.fire_coord };
        assert_eq!(wave.source(), expected, "reverse={reverse}");
    }
}

/// Ares WaveClass_Update (Ares.dll 100571A0, hook 0x760F50) and
/// WaveClass_DamageCell (10056FA0, hook 0x75F38F): the recorded cells take
/// the fired weapon's AmbientDamage, not the elite owner's GetWeapon(0), and
/// a weapon without AmbientDamage damages nothing.
#[test]
fn ares_wave_damage_uses_the_fired_weapon_and_its_ambient_damage() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    for ambient in [10, 0] {
        let rules = ares_sonic_rules(&format!("AmbientDamage={ambient}"));
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5EED_A2E6);
        let dolphin = sim
            .spawn_object("DLPH", "Americans", 6, 10, 64, &rules)
            .unwrap();
        let receiver = sim
            .spawn_object("TARGET", "Russians", 7, 10, 0, &rules)
            .unwrap();
        let target = sim
            .spawn_object("TARGET", "Russians", 12, 10, 0, &rules)
            .unwrap();
        let event = sonic_fire_event(&mut sim, &rules, dolphin, target);
        sim.commit_fired_wave(&rules, &event, 0, 0);
        sim.substrate
            .entities
            .get_mut(dolphin)
            .unwrap()
            .veterancy
            .set_rank_for_test(200);
        let health = |sim: &Simulation| {
            sim.substrate
                .entities
                .get(receiver)
                .unwrap()
                .health
                .current()
        };
        for _ in 0..20 {
            sim.visit_combat_tail(
                0,
                &rules,
                None,
                &mut crate::sim::world::FrameEffects::default(),
            );
            if health(&sim) != 100 {
                break;
            }
        }
        assert_eq!(health(&sim), 100 - ambient, "AmbientDamage={ambient}");
    }
}

/// Ares TechnoClass_Fire_OtherWaves (Ares.dll 10056E60, hook 0x6FF5F5) and
/// WaveClass_Update (100571A0, hook 0x760F50): a `Wave.IsLaser` or
/// `Wave.IsBigLaser` shot creates a TS laser wave of type 1 or 2, whose
/// AmbientDamage strikes the cells under the beam on each of its 22 updates
/// (nLaserIntensity 160, less 6 each, leaves below 32), and then it leaves.
#[test]
fn ares_laser_wave_damages_its_cells_on_each_update_until_it_fades() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    for (key, wave_type) in [("Wave.IsLaser", 1), ("Wave.IsBigLaser", 2)] {
        let Some(patched) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
            &IniFile::from_str(&format!(
                "[MTNK]\nPrimary=LaserWave\n\
                     [LaserWave]\nDamage=0\nAmbientDamage=3\nROF=200\nRange=6\n\
                     Projectile=InvisibleLow\nSpeed=100\nWarhead=RailShot2\n{key}=yes\n",
            )),
        ) else {
            return;
        };
        let rules = &patched.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x1A5E_0001);
        *sim.fog = crate::sim::vision::FogState::new(32, 32);
        let owner = sim.interner.intern("Americans");
        sim.session.set_current_house_for_test(Some(owner));
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let source = sim
            .spawn_object("MTNK", "Americans", 16, 16, 64, rules)
            .unwrap();
        let target = sim
            .spawn_object("SMCV", "Russians", 20, 16, 192, rules)
            .unwrap();
        let on_line = sim
            .spawn_object("SMCV", "Russians", 18, 16, 192, rules)
            .unwrap();
        sim.fog.mark_visible_for_owner(owner, 20, 16);
        assert!(sim.apply_command(
            "Americans",
            &Command::Attack {
                attacker_id: source,
                target_id: target
            },
            Some(rules),
        ));
        let grid = sim.path_grid_snapshot().unwrap();
        let health = |sim: &Simulation| {
            sim.substrate
                .entities
                .get(on_line)
                .unwrap()
                .health
                .current()
        };
        let mut wave_id = None;
        let mut damaged_updates = 0;
        for _ in 0..96 {
            let before = health(&sim);
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
            if health(&sim) != before {
                assert_eq!(before - health(&sim), 3, "{key}: one AmbientDamage");
                damaged_updates += 1;
            }
            if let Some(id) = sim.active_wave_id(source) {
                assert_eq!(*wave_id.get_or_insert(id), id, "{key}: one shot");
                assert_eq!(sim.waves.get(id).unwrap().wave_type(), wave_type, "{key}");
            }
        }
        let id = wave_id.expect("the laser-wave shot creates its wave");
        assert_eq!(damaged_updates, 22, "{key}");
        assert!(sim.waves.get(id).is_none(), "{key}: the faded wave leaves");
    }
}

#[test]
fn sonic_constructor_at_240_registers_then_runs_at_same_pass_tail() {
    let rules = sonic_wave_test_rules(6);
    let mut sim = Simulation::new();
    let firer_id = sim.substrate.allocate_stable_id();
    let target_id = sim.substrate.allocate_stable_id();
    let owner = sim.interner.intern("Americans");
    let firer_type = sim.interner.intern("DLPH");
    let target_type = sim.interner.intern("TARGET");
    let mut firer = GameEntity::test_default(firer_id, "DLPH", "Americans", 0, 0);
    firer.set_owner_for_test(owner);
    firer.set_type_ref_for_test(firer_type);
    firer.position.set_sub_x_for_test(SimFixed::from_num(0));
    firer.position.set_sub_y_for_test(SimFixed::from_num(0));
    let mut target = GameEntity::test_default(target_id, "TARGET", "Russians", 0, 0);
    target.set_type_ref_for_test(target_type);
    target.position.set_sub_x_for_test(SimFixed::from_num(240));
    target.position.set_sub_y_for_test(SimFixed::from_num(0));
    sim.substrate.entities.insert(firer);
    sim.substrate.entities.insert(target);
    let event = sonic_fire_event(&mut sim, &rules, firer_id, target_id);

    let cursor = sim.native_identity_cursor().unwrap();
    admit_test_wave(&mut sim, &rules, &event);
    let native = &crate::test_fixture::json("tools/procedural_drawing_oracle/magnetic_wave.json")["birth_rows"]
        [0];
    let delta = (native["id_after"].as_u64().unwrap() as u32)
        .wrapping_sub(native["id_before"].as_u64().unwrap() as u32);
    assert_eq!(
        sim.native_identity_cursor(),
        Some(cursor.wrapping_add(delta)),
        "admitted Wave spends the original CreateID count through the Scenario owner"
    );

    let wave_id = *sim
        .active_wave_links
        .get(&firer_id)
        .expect("live owner link");
    let wave = sim.waves.get(wave_id).expect("registered Wave");
    assert!(wave.in_logic_vector());
    assert_eq!(wave.lifetime(), 100, "FireAt only registers the Logic tail");

    sim.visit_combat_tail(
        0,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );

    let wave = sim.waves.get(wave_id).expect("Wave survives first tail AI");
    assert_eq!(wave.lifetime(), 99, "first AI belongs to the firing pass");
    assert_eq!(
        wave.target().z,
        50,
        "type-0 uses the live target-side Sonic Z adjustment"
    );
    assert!(sim.substrate.pending_delete.is_empty());
}

#[test]
fn sonic_cell_target_uses_persistent_dummy_gettargetcoords_on_create_and_refresh() {
    let rules = sonic_wave_test_rules(6);
    let mut sim = Simulation::new();
    let terrain = ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
    terrain.test_set_dummy_cell_level_slope(2, 0);
    let process_dummy = terrain.shared_cell_dummy();
    process_dummy.set_bridge_flags_0x1180(crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL);
    sim.install_resolved_terrain_for_new_map(terrain);
    assert!(
        process_dummy.same_identity(&sim.effective_shared_cell_dummy()),
        "the Wave lookup must retain the map's process-global dummy identity",
    );

    let firer_id = sim.substrate.allocate_stable_id();
    let owner = sim.interner.intern("Americans");
    let firer_type = sim.interner.intern("DLPH");
    let mut firer = GameEntity::test_default(firer_id, "DLPH", "Americans", 0, 0);
    firer.set_owner_for_test(owner);
    firer.set_type_ref_for_test(firer_type);
    firer.position.set_sub_x_for_test(SimFixed::from_num(0));
    firer.position.set_sub_y_for_test(SimFixed::from_num(0));
    firer
        .attack_target
        .set_for_test(Some(TargetKind::Cell(u16::MAX, 7)));
    sim.substrate.entities.insert(firer);

    let mut event = sonic_fire_event(&mut sim, &rules, firer_id, u64::MAX);
    event.target = crate::sim::combat::TargetKind::Cell(u16::MAX, 7);
    admit_test_wave(&mut sim, &rules, &event);

    let wave_id = *sim
        .active_wave_links
        .get(&firer_id)
        .expect("off-map Cell target produces a live Wave");
    let wave = sim.waves.get(wave_id).expect("registered Wave");
    assert_eq!(
        wave.target(),
        crate::sim::projectile::ProjectileCoord::new(-128, 1_920, 674),
        "CellClass ground 2*104 plus structural +416 and Sonic +50",
    );
    assert_eq!(process_dummy.snapshot().coord, (-1, 7));

    process_dummy.stamp_coord(-9, -9);
    let context = sim.wave_update_context(sim.waves.get(wave_id).unwrap(), &rules);
    assert_eq!(process_dummy.snapshot().coord, (-1, 7));
    assert_eq!(
        context.target_position,
        Some(crate::sim::projectile::ProjectileCoord::new(
            -128, 1_920, 624,
        )),
        "every live refresh re-enters GetCellClass then GetTargetCoords",
    );
    assert_eq!(process_dummy.snapshot().level, 2);
    assert_eq!(
        process_dummy.snapshot().bridge_flags_0x1180,
        crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL,
        "coordinate restamps preserve the dummy's live non-coordinate fields",
    );

    // Update_Wave puts the owner's GetFLH first (0x00762D15..0x00762D52), so
    // UpdateCells walks out from the firer's corner: the first AI's two
    // samples stay in its cell and look nothing up.
    sim.visit_combat_tail(
        0,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    let wave = sim.waves.get(wave_id).expect("Wave survives first live AI");
    assert_eq!(wave.lifetime(), 99);
    assert_eq!(wave.target().z, 674);
    assert!(wave.recorded_cells().is_empty());
    assert_eq!(process_dummy.snapshot().coord, (-1, 7));
    // The second AI's third sample enters (0, 1), off this empty map.
    sim.visit_combat_tail(
        0,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    let wave = sim
        .waves
        .get(wave_id)
        .expect("Wave survives second live AI");
    assert_eq!(wave.lifetime(), 98);
    assert_eq!(
        wave.recorded_cells(),
        [crate::sim::wave::WaveRecordedCell {
            identity: crate::sim::wave::WaveCellIdentity::SharedDummy,
        }]
    );
    assert_eq!(
        process_dummy.snapshot().coord,
        (0, 1),
        "UpdateCells keeps the shared identity but leaves the final miss restamp live",
    );
}

/// Chain 1 end to end through the production frame: a GI placed on the map
/// enters Guard on Unlimbo (`0x0051CBA0`), its passive block
/// (`0x006FA65A`) scans the enemy infantryman into range, and it fires Inviso
/// bullets that land in each frame's Logic tail until the target dies, then
/// holds no target.
// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn guard_infantry_acquire_return_fire_and_finish_a_duel() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n[VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
         [E1]\nStrength=125\nArmor=none\nSpeed=4\nSight=5\nPrimary=M60\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nProjectile=InvisibleLow\nSpeed=100\nWarhead=SA\n\n\
         [InvisibleLow]\nInviso=yes\nImage=none\n\n\
         [SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n",
    ))
    .expect("GI fixture");
    let mut sim = Simulation::with_seed(0x5EED_0061);
    *sim.input_delay_ticks = 0;
    let terrain = ResolvedTerrainGrid::from_cells(
        12,
        3,
        (0..3)
            .flat_map(|ry| (0..12).map(move |rx| bridgehead_base_cell(rx, ry)))
            .collect(),
    );
    install_rectangular_test_playfield(&mut sim, terrain.width(), terrain.height());
    sim.install_resolved_terrain_for_new_map(terrain);
    // Both full-frame receivers can reach InfantryClass::AI's native fear
    // owner after taking damage. A live map actor has its real House.
    for name in ["Americans", "Russians"] {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
    }
    let gi = sim
        .spawn_object("E1", "Americans", 2, 1, 64, &rules)
        .expect("GI");
    let enemy = sim
        .spawn_object("E1", "Russians", 5, 1, 64, &rules)
        .expect("enemy infantryman");
    // Both Houses are human, so the campaign scan gate admits each guard's
    // enemy only as the player's GI (+0x41A) and an enemy it has seen (+0x41B).
    crate::sim::arena_fixture::own(&mut sim, gi);
    crate::sim::arena_fixture::discover(&mut sim, enemy);
    *sim.path_grid = Some(std::sync::Arc::new(PathGrid::test_all_passable(12, 3)));
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    runtime.resources_mut().rules = rules;

    // The two identical guards both acquire and return fire. A fixed winner
    // was an accidental property of the old globally delayed firing host:
    // the live Infantry51BF59 slot interleaves GetROF's jitter before the next
    // actor's Techno AI draws. Winner identity here is not a native golden.
    // Follow both actors through the same production chain instead.
    let actors = [gi, enemy];
    let mut shots = [0; 2];
    let mut health_history = [Vec::new(), Vec::new()];
    let mut survivor = None;
    for _ in 0..600 {
        let output = runtime
            .advance_frame(
                &[],
                67,
                TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        assert!(output.tick.frame_committed);
        let sim = &runtime.simulation;
        let mut alive = Vec::new();
        for (index, &id) in actors.iter().enumerate() {
            shots[index] += output
                .fire_events
                .iter()
                .filter(|event| event.attacker_id == id)
                .count();
            // A killed guard stays stored as a corpse at Health 0.
            if let Some(target) = sim.substrate.entities.get(id)
                && target.is_ai_alive()
            {
                health_history[index].push(target.health.current());
                alive.push(index);
            }
        }
        if alive.len() < 2 {
            assert_eq!(alive.len(), 1, "this seeded duel has one survivor");
            survivor = alive.first().copied();
            break;
        }
    }
    let survivor = survivor.expect("one guard must finish the duel");
    for history in &mut health_history {
        history.dedup();
        assert_eq!(history.first(), Some(&125));
        assert!(
            history.windows(2).all(|pair| pair[0] - pair[1] == 25),
            "each hit is one M60 bullet: {history:?}"
        );
    }
    assert_eq!(shots[survivor], 5, "five 25-damage shots kill 125 HP");
    assert_eq!(shots[1 - survivor], 4, "the other guard returned fire");
    let survivor = actors[survivor];
    assert_eq!(
        runtime
            .simulation
            .substrate
            .entities
            .get(survivor)
            .unwrap()
            .health
            .current(),
        25,
        "the survivor received four bullets"
    );
    // Return fire puts the survivor on Attack. Once its target dies, the
    // next Mission_Attack visits return it to Guard, holding no target.
    let guard = crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard);
    for _ in 0..60 {
        if runtime
            .simulation
            .substrate
            .entities
            .get(survivor)
            .unwrap()
            .mission
            .current()
            == guard
        {
            break;
        }
        runtime
            .advance_frame(
                &[],
                67,
                TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
    }
    let survivor = runtime
        .simulation
        .substrate
        .entities
        .get(survivor)
        .expect("the winning guard survives");
    assert_eq!(survivor.mission.current(), guard);
    assert!(survivor.attack_target.target().is_none());
}

// Ares WaveClass_Update damages before the update; see the ares_wave tests.
#[test]
fn sonic_fire_registers_immediately_but_later_techno_fires_before_wave_tail_ai() {
    let rules = sonic_tail_order_test_rules();
    let mut sim = Simulation::with_seed(0x5EED_760F);
    *sim.input_delay_ticks = 0;
    let terrain = ResolvedTerrainGrid::from_cells(
        8,
        3,
        (0..3)
            .flat_map(|ry| (0..8).map(move |rx| bridgehead_base_cell(rx, ry)))
            .collect(),
    );
    install_rectangular_test_playfield(&mut sim, terrain.width(), terrain.height());
    sim.install_resolved_terrain_for_new_map(terrain);
    let dolphin_id = sim
        .spawn_object("DLPH", "Americans", 0, 0, 64, &rules)
        .expect("Dolphin placed first in Logic order");
    let later_id = sim
        .spawn_object("LATER", "Americans", 0, 2, 64, &rules)
        .expect("later shooter placed after Dolphin");
    let sonic_endpoint_id = sim
        .spawn_object("TARGET", "Russians", 6, 0, 0, &rules)
        .expect("Sonic endpoint");
    // Update_Wave walks out from the Dolphin's GetFLH (0x00762D15..0x00762D52);
    // its first AI's samples at 0.05 and 0.10 record only (1, 0).
    let wave_receiver_id = sim
        .spawn_object("TARGET", "Russians", 1, 0, 0, &rules)
        .expect("cell-list Wave receiver");
    let later_target_id = sim
        .spawn_object("TARGET", "Russians", 2, 2, 0, &rules)
        .expect("later shooter's target");
    // These later Units take their Techno AI between the firers and the Wave.
    // Hold their unrelated Guard/scan timers so the RNG equality below
    // isolates the two FireAt transactions and the Wave receiver boundary.
    for id in [sonic_endpoint_id, wave_receiver_id, later_target_id] {
        let target = sim.substrate.entities.get_mut(id).unwrap();
        target.mission.write_dispatch_epilogue(0, 1000);
        target.passive_scan.timer_mut_for_test().arm(0, 1000);
    }
    assert!(crate::sim::combat::install_entity_attack_target_for_test(
        &mut sim.substrate.entities,
        dolphin_id,
        sonic_endpoint_id
    ));
    assert!(crate::sim::combat::install_entity_attack_target_for_test(
        &mut sim.substrate.entities,
        later_id,
        later_target_id
    ));
    sim.clear_lifecycle_test_events_for_test();

    let path = PathGrid::test_all_passable(8, 3);
    *sim.path_grid = Some(std::sync::Arc::new(path));
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    runtime.resources_mut().rules = rules;
    let output = runtime
        .advance_frame(
            &[],
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    assert!(output.tick.frame_committed);
    let sim = &runtime.simulation;

    assert_eq!(
        sim.substrate
            .entities
            .get(later_target_id)
            .expect("later target survives")
            .health
            .current(),
        93,
        "the later pre-existing Techno completed its FireAt effects",
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(wave_receiver_id)
            .expect("Wave receiver survives")
            .health
            .current(),
        90,
        "the appended Wave still damages its first recorded cell this frame",
    );
    let wave_id = *sim
        .active_wave_links
        .get(&dolphin_id)
        .expect("Dolphin retains the live Wave link");
    assert_eq!(
        sim.waves
            .get(wave_id)
            .expect("Wave remains live")
            .lifetime(),
        99,
        "the new Logic tail owns its first AI in the firing pass",
    );

    let events = sim.lifecycle_test_events_for_test();
    let dolphin_boundary = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            LifecycleTestEvent::CombatFireEffectsCommitted {
                attacker_id,
                scenario_rng_state,
            } if *attacker_id == dolphin_id => Some((index, *scenario_rng_state)),
            _ => None,
        })
        .expect("Dolphin FireAt boundary traced");
    let later_boundary = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            LifecycleTestEvent::CombatFireEffectsCommitted {
                attacker_id,
                scenario_rng_state,
            } if *attacker_id == later_id => Some((index, *scenario_rng_state)),
            _ => None,
        })
        .expect("later FireAt boundary traced");
    let wave_receiver = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            LifecycleTestEvent::WaveDamageReceiverSelected {
                wave_id: selected_wave,
                target_id,
                scenario_rng_state,
            } if *selected_wave == wave_id && *target_id == wave_receiver_id => {
                Some((index, *scenario_rng_state))
            }
            _ => None,
        })
        .expect("Wave receiver boundary traced");
    assert!(dolphin_boundary.0 < later_boundary.0);
    assert!(
        later_boundary.0 < wave_receiver.0,
        "later FireAt/effects must finish before the appended Wave receiver walk",
    );
    assert_ne!(
        dolphin_boundary.1, later_boundary.1,
        "the later FireAt consumed its own ROF RNG before the Wave",
    );
    assert_eq!(
        later_boundary.1, wave_receiver.1,
        "the Wave receiver begins from the RNG state left by the later Techno",
    );
}

// Ares WaveClass_Update damages before the update; see the ares_wave tests.
#[test]
fn sonic_cell_fire_wave_damage_selects_level_two_bridge_plane() {
    // Range6 cannot reach this six-cell horizontal separation plus the
    // source deck/target WaterSet-ground difference. Keep the Wave's tested
    // geometry while placing the shot strictly inside the weapon's range.
    let rules = sonic_wave_test_rules(7);
    let mut sim = Simulation::with_seed(0x5EED_6240);
    *sim.input_delay_ticks = 0;
    let mut cells = (0..8)
        .map(|rx| bridgehead_base_cell(rx, 0))
        .collect::<Vec<_>>();
    for cell in &mut cells {
        cell.level = 2;
        cell.template_height = 2;
        cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
        cell.zone_type = crate::map::resolved_terrain::zone_class::WATER;
        cell.ground_walk_blocked = true;
        cell.base_ground_walk_blocked = true;
        cell.base_build_blocked = true;
        cell.base_land_type = cell.land_type;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_deck_level = 6;
        cell.bridge_facts.raw_flags |= crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    }
    install_rectangular_test_playfield(&mut sim, 8, 1);
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(8, 1, cells));
    // Stay inside Range=6 after the native Cell range query includes the
    // 416-lepton height difference; this test covers the Wave receiver plane.
    let dolphin_id = sim
        .spawn_object("DLPH", "Americans", 1, 0, 64, &rules)
        .expect("bridge Dolphin");
    // The Wave walks out from the Dolphin's GetFLH; its first AI records (2, 0).
    let receiver_id = sim
        .spawn_object("TARGET", "Russians", 2, 0, 0, &rules)
        .expect("bridge receiver");
    for id in [dolphin_id, receiver_id] {
        sim.remove_entity_occupancy(id);
        let entity = sim.substrate.entities.get_mut(id).expect("live entity");
        entity.set_on_bridge(true);
        // This fixture relocates spawned ground objects onto the deck. The
        // native Cell-target range query reads the retained Object coordinate,
        // so update it through the same height owner as ground movement.
        entity
            .position
            .set_height(true, 0, sim.resolved_terrain.as_ref(), None);
        assert_eq!(entity.position.exact_z_leptons(), Some(624));
        sim.add_entity_occupancy(id);
    }
    assert!(crate::sim::combat::install_cell_attack_target_for_test(
        &mut sim.substrate.entities,
        dolphin_id,
        6,
        0,
        Some(&rules),
        &sim.interner,
    ));
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    let target = cells.lookup((6, 0));
    assert_eq!(
        crate::sim::combat::in_range::native_cell_range_coords(target, &cells),
        Some((6 * 256 + 128, 128, 208)),
        "WaterSet Cell range uses its ground coordinate before Wave construction"
    );
    let source = sim.substrate.entities.get(dolphin_id).unwrap();
    let source_z = i64::from(crate::sim::movement::ground_pose::object_world_z_leptons(
        source,
        Some(terrain),
    ));
    assert_eq!(source_z, 624);
    assert!(crate::sim::combat::in_range::compute_in_range(
        source,
        (128, 128, source_z),
        &crate::sim::combat::TargetKind::Cell(6, 0),
        rules.weapon("SonicZap").unwrap(),
        &rules,
        &sim.interner,
        &sim,
        terrain,
        &Default::default(),
    ));

    let path = PathGrid::test_all_passable(8, 1);
    *sim.path_grid = Some(std::sync::Arc::new(path));
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    runtime.resources_mut().rules = rules;
    let output = runtime
        .advance_frame(
            &[],
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    assert!(output.tick.frame_committed);
    let wave_id = *runtime
        .simulation
        .active_wave_links
        .get(&dolphin_id)
        .expect("cell FireAt registered its Wave");
    // The Sonic bullet, Logic-appended just ahead of its Wave, strikes the
    // deck on its first AI in the same tail. Its removal shifts the Wave into
    // the visited slot (`0x0055B608..0x0055B619`), so the Wave's first AI is
    // the next frame's.
    assert_eq!(
        runtime.simulation.waves.get(wave_id).unwrap().lifetime(),
        100
    );
    let output = runtime
        .advance_frame(
            &[],
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    assert!(output.tick.frame_committed);
    let sim = &runtime.simulation;
    let wave = sim.waves.get(wave_id).expect("Wave survives its first AI");
    assert_eq!(wave.lifetime(), 99);
    assert_eq!(
        wave.target().z,
        674,
        "level 2 CellClass target is 2*104 + structural 416 + Sonic 50",
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(receiver_id)
            .expect("bridge receiver survives")
            .health
            .current(),
        90,
        "B4 Z 624, the Dolphin's deck GetFLH, meets the level-2 threshold 624 and walks AltObject",
    );
}
