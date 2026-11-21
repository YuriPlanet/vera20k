//! The fire path: sonic gate, cooldown, range, infantry fire frames, garrison
//! fire, guard scans and acquisition, and live object order.

use super::*;

#[test]
fn sonic_active_wave_gate_precedes_target_resolution_and_all_shot_work() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=DLPH\n1=TARGET\n\n\
         [DLPH]\nStrength=200\nArmor=light\nSpeed=8\nPrimary=SonicZap\n\n\
         [TARGET]\nStrength=100\nArmor=wood\n\n\
         [SonicZap]\nDamage=4\nAmbientDamage=10\nROF=20\nRange=6\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [SonicWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("Sonic whole-shot gate fixture");
    let mut entities = EntityStore::new();
    let mut firer = GameEntity::test_default(1, "DLPH", "Americans", 4, 5);
    firer
        .attack_target
        .set_for_test(Some(TargetKind::Entity(999)));
    firer.set_gunner_selection_for_test(1, -1);
    entities.insert(firer);
    let mut interner = test_interner();
    let snap = build_attacker_snapshot(
        entities.get(1).expect("Dolphin"),
        TargetKind::Entity(999),
        None,
    );
    let mut rng = SimRng::new(0x50_4e_49_43);
    let rng_before = rng.logical_state();
    let rearm_before = entities.get(1).unwrap().seldom.rearm_timer.timer();
    let mut hooks: Option<&mut FixtureTrace> = None;
    let mut emit = CombatEmit::default();

    resolve_attacker_fire(
        &snap,
        &mut entities,
        &rules,
        &mut interner,
        None,
        None,
        &OccupancyGrid::new(),
        None,
        None,
        None,
        None,
        None,
        false,
        17,
        67,
        true,
        &mut rng,
        None,
        &mut hooks,
        &mut emit,
    );

    assert!(emit.fire_events.is_empty());
    assert!(emit.damage_events.is_empty());
    assert_eq!(
        entities.get(1).unwrap().seldom.rearm_timer.timer(),
        rearm_before
    );
    assert!(emit.remove_attack.is_empty());
    assert_eq!(rng.logical_state(), rng_before);
    assert_eq!(entities.get(1).unwrap().current_weapon_number(), 1);
}

fn guardian_gi_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "\
[InfantryTypes]\n0=GGI\n1=E2\n2=ROCK\n\n\
[VehicleTypes]\n0=HTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[General]\nMissileROTVar=.25\n\n\
[GGI]\nStrength=100\nArmor=none\nSpeed=4\nImage=GGI\nPrimary=M60\nSecondary=MissileLauncher\nDeployFire=yes\n\n\
[E2]\nStrength=125\nArmor=none\nSpeed=4\n\n\
[ROCK]\nStrength=125\nArmor=none\nSpeed=8\nConsideredAircraft=yes\n\n\
[HTNK]\nStrength=400\nArmor=heavy\nSpeed=5\n\n\
[M60]\nDamage=15\nROF=20\nRange=4\nWarhead=SA\nReport=GGIAttack\n\n\
[MissileLauncher]\nDamage=40\nROF=40\nRange=8\nBurst=1\nProjectile=AAHeatSeeker2\nSpeed=30\nWarhead=GUARDWH\nReport=GuardianGIDeployedAttack\nMinimumRange=1\n\n\
[AAHeatSeeker2]\nArm=2\nShadow=no\nProximity=no\nRanged=yes\nAA=yes\nAG=yes\nImage=DRAGON\nROT=60\nSubjectToCliffs=no\nSubjectToElevation=no\nSubjectToWalls=no\n\n\
[SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n\n\
[GUARDWH]\nVerses=20%,20%,20%,100%,50%,100%,10%,10%,10%,100%,100%\n",
    );
    // Reader-built Deployed/DeployedFire records admit the real Do_Action.
    // The discharge keys are deliberately absent: their native defaults are 0.
    let art_ini = IniFile::from_str(
        "[GGI]\nSequence=GGISequence\n\
         [GGISequence]\nReady=0,1,1\nGuard=0,1,1\nDeployed=292,1,0\nDeployedFire=292,6,0\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini)
        .expect("guardian GI rules should parse");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    rules
}

#[test]
fn gsi_08_05_tick_combat_respects_the_jittered_cooldown() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    let mut main_rng = SimRng::new(1);

    // One call per frame: the rearm countdown is frame-anchored.
    let fire_at =
        |frame: u32, store: &mut EntityStore, interner: &mut StringInterner, rng: &mut SimRng| {
            align_attackers_to_targets(store);
            tick_combat(
                store,
                &mut OccupancyGrid::new(),
                &rules,
                interner,
                u64::from(frame),
                100,
                frame,
                rng,
            );
        };

    // First shot fires immediately (no reload running).
    fire_at(0, &mut store, &mut interner, &mut main_rng);
    let h1: i32 = store.get(2).unwrap().health.current();
    assert!(h1 < 300, "the first shot lands at once");

    // `TechnoClass::GetROF @ 0x006FCFA0` returns `ROF + RandomRanged(0, 2)`,
    // so a `ROF=50` weapon reloads in 50, 51 or 52 frames — the exact value is
    // drawn, which is why this test reads the armed countdown instead of
    // hardcoding a frame number.
    let cooldown = store.get(1).unwrap().seldom.rearm_timer.duration() as u32;
    assert!(
        (50..=52).contains(&cooldown),
        "ROF=50 must reload in 50..=52 frames, got {cooldown}"
    );

    // Every frame before the countdown ends leaves the target untouched.
    for frame in 1..cooldown {
        fire_at(frame, &mut store, &mut interner, &mut main_rng);
    }
    assert_eq!(
        store.get(2).unwrap().health.current(),
        h1,
        "no second shot before the countdown reaches zero"
    );
    assert_eq!(
        store
            .get(1)
            .unwrap()
            .seldom
            .rearm_timer
            .remaining(cooldown as i32 - 1),
        1
    );

    // The countdown's last frame fires.
    fire_at(cooldown, &mut store, &mut interner, &mut main_rng);
    assert!(
        store.get(2).unwrap().health.current() < h1,
        "the shot lands on the frame the countdown clears"
    );
}

#[test]
fn test_tick_combat_out_of_range() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    // 105mm range = 6 cells. Target at distance 10.
    store.insert(make_entity(1, "MTNK", 0, 0, 300));
    store.insert(make_entity(2, "MTNK", 10, 0, 300));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    let target_health = store.get(2).unwrap().health.current();
    assert_eq!(
        target_health, 300,
        "Out-of-range target should not take damage"
    );
    // Range failure preserves attack_target; the mission's Approach (run from
    // advance_tick, not from tick_combat in isolation) walks the unit into range.
    assert!(
        store.get(1).unwrap().attack_target.target().is_some(),
        "the target is kept when out of range — Approach closes the gap"
    );
}

/// `TechnoClass::FireAt 0x006FE5E2..0x006FE622`: every launched shot whose
/// BulletType is not `Inaccurate=` takes `EstimateDamage` off its TarCom's
/// retained estimate (`+0x70`), apart from the bullet's own damage.
#[test]
fn a_shot_debits_its_targets_estimate_unless_inaccurate() {
    for inaccurate in [false, true] {
        let ini = format!(
            "[InfantryTypes]\n0=GGI\n1=E2\n\n[VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
             [GGI]\nStrength=100\nArmor=none\nSpeed=4\nPrimary=M60\n\n\
             [E2]\nStrength=125\nArmor=none\nSpeed=4\n\n\
             [M60]\nDamage=15\nROF=20\nRange=4\nProjectile=Shot\nWarhead=SA\n\n\
             [Shot]\nInviso=yes\nInaccurate={}\n\n\
             [SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n",
            if inaccurate { "yes" } else { "no" }
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let mut store = EntityStore::new();
        store.insert(make_infantry_entity(1, "GGI", 0, 0, 100));
        store.insert(make_infantry_entity(2, "E2", 3, 0, 125));
        let before = store.get(2).unwrap().estimated_health.get();
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 2);
        tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));
        let target = store.get(2).unwrap();
        assert_eq!(target.health.current(), 110, "the bullet lands either way");
        assert_eq!(
            target.estimated_health.get(),
            if inaccurate { before } else { before - 15 },
            "Inaccurate={inaccurate}"
        );
    }
}

/// `BulletClass::DetonateAtCoord 0x00469BD6..0x00469C41`: a `Bright=` weapon's
/// bullet lights its detonation with its damage, force 1 and the warhead's
/// CLDisable channels; a dim one lights nothing.
#[test]
fn a_bright_shot_lights_its_detonation() {
    for bright in [true, false] {
        let ini = format!(
            "[InfantryTypes]\n\n[VehicleTypes]\n0=SHOOTER\n1=TARGET\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
             [SHOOTER]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\n\
             [TARGET]\nStrength=500\nArmor=heavy\nSpeed=6\n\n\
             [GUN]\nDamage=90\nROF=20\nRange=10\nProjectile=Shot\nWarhead=WH\nBright={}\n\n\
             [Shot]\nInviso=yes\n\n\
             [WH]\nCLDisableGreen=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            if bright { "yes" } else { "no" }
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let mut store = EntityStore::new();
        store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
        store.insert(make_entity(2, "TARGET", 8, 5, 500));
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 2);
        align_attackers_to_targets(&mut store);
        let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));
        assert_eq!(result.consequences.fire_events().len(), 1);
        let lights = &result.consequences.effects().combat_light_requests;
        if bright {
            assert_eq!(lights.len(), 1);
            assert!(
                matches!(
                    lights[0],
                    CombatLightRequest::Impact {
                        damage: 90,
                        flags: 4,
                        force_create: true,
                        target_id: None,
                        ..
                    }
                ),
                "Bright impact / CLDisableGreen"
            );
        } else {
            assert!(lights.is_empty());
        }
    }
}

#[test]
fn deployed_guardian_gi_vs_rhino_at_six_cells_uses_missilelauncher() {
    let rules = guardian_gi_rules();
    let mut store = EntityStore::new();
    let mut ggi = make_infantry_entity(1, "GGI", 0, 0, 100);
    ggi.mission_leaf.set_infantry_doing_verified(28).unwrap();
    store.insert(ggi);
    store.insert(make_entity(2, "HTNK", 6, 0, 400));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert_eq!(interner.resolve(ev.weapon_id), "MissileLauncher");
    assert_eq!(ev.weapon_slot, WeaponSlot::Secondary);
    assert_eq!(store.get(2).unwrap().health.current(), 400);
    assert_eq!(result.projectile_spawns.len(), 1);
}

#[test]
fn infantry_standing_fire_waits_for_fire_frame() {
    let rules = infantry_fire_frame_rules();
    let mut store = EntityStore::new();
    store.insert(make_infantry_entity(1, "E1", 5, 5, 125));
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    let mut main_rng = SimRng::new(1);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut main_rng);

    assert_eq!(store.get(2).unwrap().health.current(), 125);
    assert!(result.consequences.fire_events().is_empty());
    let attacker = store.get(1).unwrap();
    assert_eq!(attacker.mission_leaf.foot_firing_sequence_latch(), 1);
    assert_eq!(attacker.mission_leaf.as_infantry().unwrap().doing(), 4);
    assert_eq!(attacker.native_stage().value(), 0);
    assert_eq!(attacker.infantry_sprite_pose(), Some((4, 0)));

    set_infantry_stage(&mut store, 1, 1);
    let result = tick(&mut store, &rules, &mut interner, 1, &mut main_rng);
    assert_eq!(store.get(2).unwrap().health.current(), 125);
    assert!(result.consequences.fire_events().is_empty());

    set_infantry_stage(&mut store, 1, 2);
    let result = tick(&mut store, &rules, &mut interner, 2, &mut main_rng);
    assert_eq!(store.get(2).unwrap().health.current(), 100);
    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert_eq!(interner.resolve(ev.weapon_id), "M60");
    assert_eq!(ev.weapon_slot, WeaponSlot::Primary);
    assert_eq!(
        ev.report_sound_id.map(|id| interner.resolve(id)),
        Some("GIAttack")
    );
    assert!(!ev.occupied_building);
    assert_eq!(
        store
            .get(1)
            .unwrap()
            .mission_leaf
            .foot_firing_sequence_latch(),
        0
    );
}

/// `InfantryClass::Fire_At_Target`'s tail (`0x00520A57..0x00520AC7`): on the
/// fire frame, a weapon 0 slower than `[CombatDamage] Incoming=` scatters the
/// target's cell away from the shooter. ReadSpeed makes `Incoming=10` 25 and
/// `Speed=5` 12, so a computer tank three cells east of the GI heads further
/// east with a queued Move; at `Speed=10` (25, not below) it stays put,
/// though the GI fires all the same. A shot refused on the fire frame (here
/// by a running rearm) takes the tail too (`0x005209FD..0x00520A51`).
#[test]
fn a_slow_infantry_shot_scatters_its_targets_cell_away_from_the_shooter() {
    use crate::sim::combat::world_fire::FireVisit;
    use crate::sim::components::NavTargetRef;
    for (speed, refused, scatters) in [(5, false, true), (5, true, true), (10, false, false)] {
        let rules = infantry_fire_frame_rules_with(&format!(
            "[CombatDamage]\nIncoming=10\n[M60]\nSpeed={speed}\n\
             [MTNK]\nArmor=light\nLocomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n"
        ));
        assert_eq!(rules.combat_damage.incoming, 25);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
        let gi = sim
            .spawn_object("E1", "Americans", 5, 5, 0, &rules)
            .unwrap();
        let tank = sim
            .spawn_object("MTNK", "Russians", 8, 5, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .mission
            .set_current_for_test(MissionId::from_known(MissionType::Guard), 0);
        assert!(install_entity_attack_target_for_test(
            &mut sim.substrate.entities,
            gi,
            tank
        ));
        let fire = |sim: &mut Simulation| {
            sim.commit_fire_visit(
                FireVisit::InfantryTarget(gi),
                &rules,
                None,
                &mut crate::sim::world::FrameEffects::empty(),
            );
        };
        fire(&mut sim);
        assert!(
            sim.substrate
                .entities
                .get(tank)
                .unwrap()
                .navigation
                .nav_com()
                .is_none(),
            "not the fire frame"
        );
        let rearm = crate::sim::timer::CdTimer::started(0, 40);
        let shooter = sim.substrate.entities.get_mut(gi).unwrap();
        if refused {
            shooter.seldom.rearm_timer.set_for_test(rearm);
        }
        shooter.set_native_stage_value(2);
        fire(&mut sim);
        let rearm_after = sim
            .substrate
            .entities
            .get(gi)
            .unwrap()
            .seldom
            .rearm_timer
            .timer();
        if refused {
            assert_eq!(rearm_after, rearm, "Speed={speed}: the shot is refused");
        } else {
            assert!(rearm_after.duration() > 0, "Speed={speed}: the GI fired");
        }
        let tank = sim.substrate.entities.get(tank).unwrap();
        if scatters {
            assert!(
                matches!(
                    tank.navigation.nav_com(),
                    Some(NavTargetRef::Cell { rx: 9, .. })
                ),
                "{:?}",
                tank.navigation.nav_com()
            );
            assert_eq!(
                tank.mission.queued(),
                MissionId::from_known(MissionType::Move)
            );
        } else {
            assert_eq!(tank.navigation.nav_com(), None);
            assert_eq!(tank.mission.queued(), MissionId::NONE);
        }
    }
}

#[test]
fn deployed_gi_uses_deployed_fire_visual_with_deploy_fire_weapon() {
    let rules = infantry_fire_frame_rules();
    let mut store = EntityStore::new();
    let mut attacker = make_infantry_entity(1, "E1", 5, 5, 125);
    attacker
        .mission_leaf
        .set_infantry_doing_verified(28)
        .unwrap();
    store.insert(attacker);
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    let mut main_rng = SimRng::new(1);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut main_rng);
    assert!(result.consequences.fire_events().is_empty());
    let attacker = store.get(1).unwrap();
    assert_eq!(attacker.mission_leaf.foot_firing_sequence_latch(), 1);
    assert_eq!(attacker.mission_leaf.as_infantry().unwrap().doing(), 29);
    assert_eq!(attacker.infantry_sprite_pose(), Some((29, 0)));
    assert!(!attacker.infantry.as_ref().unwrap().is_prone());
    assert_eq!(store.get(2).unwrap().health.current(), 300);

    // This supplied GI has SecondaryFire=4 and a standing secondary record.
    set_infantry_stage(&mut store, 1, 3);
    let result = tick(&mut store, &rules, &mut interner, 1, &mut main_rng);
    assert!(result.consequences.fire_events().is_empty());
    assert_eq!(store.get(2).unwrap().health.current(), 300);
    assert_eq!(
        store
            .get(1)
            .unwrap()
            .mission_leaf
            .foot_firing_sequence_latch(),
        1
    );

    set_infantry_stage(&mut store, 1, 4);
    let result = tick(&mut store, &rules, &mut interner, 1, &mut main_rng);
    assert_eq!(
        store.get(2).unwrap().health.current(),
        260,
        "deployed-fire should use the DeployFireWeapon secondary slot"
    );
    assert_eq!(result.consequences.fire_events().len(), 1);
    let attacker = store.get(1).unwrap();
    assert_eq!(attacker.mission_leaf.foot_firing_sequence_latch(), 0);
    assert_eq!(attacker.infantry_sprite_pose(), Some((29, 4)));
    let ev = &result.consequences.fire_events()[0];
    assert_eq!(interner.resolve(ev.weapon_id), "Para");
    assert_eq!(ev.weapon_slot, WeaponSlot::Secondary);
    assert_eq!(
        ev.report_sound_id.map(|id| interner.resolve(id)),
        Some("GIAttackDeployed")
    );
}

#[test]
fn garrison_fire_keeps_occupant_anim_and_sound_path() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n1=E2\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=CAGAS\n\n\
[CAGAS]\nStrength=800\nArmor=wood\nCanBeOccupied=yes\nCanOccupyFire=yes\nMaxNumberOccupants=5\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\nOccupyWeapon=M60\n\n\
[E2]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
[M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\nReport=GIAttack\nOccupantAnim=UCFLASH\n\n\
[SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n",
    ))
    .expect("garrison rules parse");
    let mut store = EntityStore::new();
    let mut building = make_entity(10, "CAGAS", 5, 5, 800);
    building.set_category_for_test(EntityCategory::Structure);
    building
        .passenger_role_mut_for_test()
        .occupants_mut()
        .add_for_test(1);
    store.insert(building);
    let mut occupant = make_infantry_entity(1, "E1", 5, 5, 125);
    occupant.set_passenger_role_for_test(crate::sim::passenger::PassengerRole::inside_for_test(
        10, false,
    ));
    store.insert(occupant);
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));

    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 10, 2);
    let mut sounds = Vec::new();
    let mut main_rng = SimRng::new(1);
    let result = tick_combat_with_fog(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        Some(&mut sounds),
        None,
        None,
        None,
        0,
        100,
        0,
        &[],
        None,
        &mut main_rng,
    );

    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert!(ev.occupied_building);
    assert_eq!(
        ev.muzzle_anim.map(|id| interner.resolve(id)),
        Some("UCFLASH"),
        "an occupied building's shot constructs the weapon's OccupantAnim="
    );
    assert_eq!(
        ev.report_sound_id.map(|id| interner.resolve(id)),
        Some("GIAttack")
    );
    let [
        SimSoundEvent::VocAt {
            sound_id,
            audible_to,
            rx,
            ry,
            sub_x,
            sub_y,
            world_z_leptons,
        },
    ] = sounds.as_slice()
    else {
        panic!("expected one garrison weapon report, got {sounds:?}");
    };
    let fire_coord = crate::sim::anim_class::AnimWorldCoord::from(ev.fire_coord);
    let (fire_rx, fire_ry, fire_sub_x, fire_sub_y, _) = fire_coord.to_cell_sub_z();
    assert_eq!(sound_id.name_in(&rules), "GIAttack");
    assert_eq!(audible_to, &None);
    assert_eq!(
        (*rx, *ry, *sub_x, *sub_y, *world_z_leptons),
        (fire_rx, fire_ry, fire_sub_x, fire_sub_y, fire_coord.z)
    );
}

#[test]
fn delayed_infantry_fire_cancels_when_target_dies_before_fire_frame() {
    let rules = infantry_fire_frame_rules();
    let mut store = EntityStore::new();
    store.insert(make_infantry_entity(1, "E1", 5, 5, 125));
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    let mut main_rng = SimRng::new(1);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut main_rng);
    assert!(result.consequences.fire_events().is_empty());
    assert_eq!(
        store
            .get(1)
            .unwrap()
            .mission_leaf
            .foot_firing_sequence_latch(),
        1
    );
    store.get_mut(2).unwrap().health.set_for_test(0);
    set_infantry_stage(&mut store, 1, 2);

    let result = tick(&mut store, &rules, &mut interner, 1, &mut main_rng);

    assert_eq!(
        store
            .get(1)
            .unwrap()
            .mission_leaf
            .foot_firing_sequence_latch(),
        0
    );
    assert_eq!(store.get(2).unwrap().health.current(), 0);
    assert!(result.consequences.fire_events().is_empty());
    assert!(
        store.get(1).unwrap().attack_target.target().is_none(),
        "dead target should cancel delayed shot instead of spawning stale damage"
    );
}

/// No fire path reads shroud or fog: GetFireError `0x006FC0B0`, the class
/// fire routines and Greatest_Threat never call `IsShrouded @ 0x00586360`,
/// and `IsFogged @ 0x005865E0` is a constant false. A target on a cell its
/// attacker's house cannot see is shot like any other.
#[test]
fn an_unseen_target_is_fired_at() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Americans"));
    store.insert(make_entity_owned(2, "MTNK", 8, 5, 300, "Soviet"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    align_attackers_to_targets(&mut store);

    let fog = FogState::default();
    assert!(!fog.is_cell_visible(test_intern("Americans"), 8, 5));
    let mut occupancy = mark_fixture_entities(&mut store);
    let mut main_rng = SimRng::new(1);
    let result = tick_combat_with_fog(
        &mut store,
        &mut occupancy,
        &rules,
        &mut interner,
        Some(&fog),
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        None,
        None,
        None,
        0u64,
        100,
        0u32,
        &[],
        None,
        &mut main_rng,
    );

    assert!(!result.consequences.fire_events().is_empty());
    assert!(store.get(2).expect("target alive").health.current() < 300);
}

/// Two identical enemies share one cell. `TechnoClass::Scan_Cell_For_Target @
/// 0x006F8960` walks ONE object list and stops at the first hostile entry, so
/// the cell offers exactly one candidate and the other is never evaluated —
/// there is no stable-id tie-break anywhere in the native scan.
///
/// The list head is the most recently added non-building
/// (`CellListInsertion::PrependNonBuilding`). This fixture Marks in ascending
/// ID order, so20 is actually linked ahead of3 and the scan picks20.
#[test]
fn gsi_08_01_a_shared_cell_offers_only_its_list_head() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity_owned(10, "MTNK", 5, 5, 300, "Americans"));
    store.insert(make_entity_owned(99, "MTNK", 6, 5, 0, "Soviet")); // dead
    store.insert(make_entity_owned(20, "MTNK", 7, 5, 300, "Soviet"));
    store.insert(make_entity_owned(3, "MTNK", 7, 5, 300, "Soviet"));
    let interner = test_interner();

    let mut fog = FogState::default();
    fog.mark_visible_for_owner(test_intern("Americans"), 7, 5);
    let occupancy = mark_fixture_entities(&mut store);
    // `TechnoClass::Greatest_Threat @ 0x006F8DF0` with the passive mask.
    let pick = crate::sim::combat::combat_targeting::greatest_threat_for_entity(
        &store,
        &occupancy,
        &rules,
        &interner,
        10,
        Some(&fog),
        None,
        false,
        crate::sim::combat::ScanMission::Guard,
        None,
        crate::sim::combat::line_of_fire::LineOfFireInputs {
            alliances: Some(&HouseAlliances::default()),
            ..Default::default()
        },
        None,
        None,
    )
    .object();
    assert!(
        pick == Some(20),
        "the cell's list head is the candidate, not the lower stable id"
    );
}

/// A terrain-free Guard scan's pick between an enemy one cell away and one
/// five cells away, for an `ART` armed with `shell` (`Range=8`).
fn mapless_guard_pick(shell: &str) -> Option<u64> {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n0=ART\n1=MTNK\n[AircraftTypes]\n[BuildingTypes]\n\
         [ART]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=Shell\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
         [Shell]\nDamage=65\nROF=50\nRange=8\nWarhead=AP\n{shell}\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("mapless scan rules");
    let mut store = EntityStore::new();
    store.insert(make_entity_owned(10, "ART", 5, 5, 300, "Americans"));
    store.insert(make_entity_owned(20, "MTNK", 6, 5, 300, "Soviet"));
    store.insert(make_entity_owned(30, "MTNK", 10, 5, 300, "Soviet"));
    let interner = test_interner();
    let mut fog = FogState::default();
    for x in [6, 10] {
        fog.mark_visible_for_owner(test_intern("Americans"), x, 5);
    }
    let occupancy = mark_fixture_entities(&mut store);
    crate::sim::combat::combat_targeting::greatest_threat_for_entity(
        &store,
        &occupancy,
        &rules,
        &interner,
        10,
        Some(&fog),
        None,
        false,
        crate::sim::combat::ScanMission::Guard,
        None,
        crate::sim::combat::line_of_fire::LineOfFireInputs {
            alliances: Some(&HouseAlliances::default()),
            ..Default::default()
        },
        None,
        None,
    )
    .object()
}

/// A mapless Guard scan by a human player's guarding `ART` (with `art`) of
/// one enemy `MTNK` (with `target`) a cell away.
fn ares_guarding_pick(art: &str, target: &str) -> Option<u64> {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n0=ART\n1=MTNK\n[AircraftTypes]\n[BuildingTypes]\n\
         [ART]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=Shell\n{art}\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n{target}\n\
         [Shell]\nDamage=65\nROF=50\nRange=8\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("guard scan rules");
    let mut store = EntityStore::new();
    let mut scanner = make_entity_owned(10, "ART", 5, 5, 300, "Americans");
    scanner.mission.set_current_for_test(
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
        0,
    );
    store.insert(scanner);
    store.insert(make_entity_owned(20, "MTNK", 6, 5, 300, "Soviet"));
    let interner = test_interner();
    let mut fog = FogState::default();
    fog.mark_visible_for_owner(test_intern("Americans"), 6, 5);
    let occupancy = mark_fixture_entities(&mut store);
    crate::sim::combat::combat_targeting::greatest_threat_for_entity(
        &store,
        &occupancy,
        &rules,
        &interner,
        10,
        Some(&fog),
        None,
        false,
        crate::sim::combat::ScanMission::Guard,
        None,
        crate::sim::combat::line_of_fire::LineOfFireInputs {
            alliances: Some(&HouseAlliances::default()),
            ..Default::default()
        },
        None,
        None,
    )
    .object()
}

/// Ares `CanPassiveAquire.Guard=no` stops a human player's guarding Foot
/// from scanning (hook 0x4D9920), and `CivilianEnemy=yes` makes an
/// `Insignificant=` candidate an enemy (hook 0x6F826E).
#[test]
fn ares_passive_acquire_and_civilian_enemy_gate_the_guard_scan() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    assert_eq!(ares_guarding_pick("", ""), Some(20));
    assert_eq!(ares_guarding_pick("CanPassiveAquire.Guard=no", ""), None);
    assert_eq!(ares_guarding_pick("", "Insignificant=yes"), None);
    assert_eq!(
        ares_guarding_pick("", "Insignificant=yes\nCivilianEnemy=yes"),
        Some(20)
    );
}

/// Without resolved terrain the Guard scan's CanFireAt arm asks the shared
/// mapless range (`in_range::mapless_in_range`), MinimumRange included: the
/// nearer enemy is refused inside it, and the scan takes the other.
#[test]
fn a_mapless_guard_scan_refuses_an_enemy_inside_minimum_range() {
    assert_eq!(mapless_guard_pick(""), Some(20), "the nearer enemy");
    assert_eq!(mapless_guard_pick("MinimumRange=3"), Some(30));
}

#[test]
fn gsi_08_01_unarmed_building_loses_to_a_tank_at_equal_distance() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity_owned(10, "MTNK", 5, 5, 300, "Americans"));
    store.insert(make_entity_owned(99, "MTNK", 6, 5, 0, "Soviet")); // dead
    let mut building = make_entity_owned(1, "GAPOWR", 7, 5, 750, "Soviet");
    building.set_category_for_test(crate::map::entities::EntityCategory::Structure);
    store.insert(building);
    store.insert(make_entity_owned(200, "MTNK", 7, 5, 300, "Soviet"));
    let interner = test_interner();

    let mut fog = FogState::default();
    fog.mark_visible_for_owner(test_intern("Americans"), 7, 5);
    let occupancy = mark_fixture_entities(&mut store);
    // `TechnoClass::Greatest_Threat @ 0x006F8DF0` with the passive mask.
    let pick = crate::sim::combat::combat_targeting::greatest_threat_for_entity(
        &store,
        &occupancy,
        &rules,
        &interner,
        10,
        Some(&fog),
        None,
        false,
        crate::sim::combat::ScanMission::Guard,
        None,
        crate::sim::combat::line_of_fire::LineOfFireInputs {
            alliances: Some(&HouseAlliances::default()),
            ..Default::default()
        },
        None,
        None,
    )
    .object();
    // Not a "threat class" tie-break — gamemd has none. `[GAPOWR]` carries no
    // weapon and `ThreatPosed=0`, so the human-attacker building gate at
    // `TechnoClass::Evaluate_Candidate @ 0x006F85AB` refuses it outright and
    // the tank is the only candidate the cell can offer that survives.
    assert!(
        pick == Some(200),
        "an unarmed enemy building is not a legal passive target for a human unit"
    );
}

/// The range in leptons of the weapon SelectWeapon picks against `target`, as
/// Approach reads it ([`combat_weapon::select_weapon_against`]); None for an
/// empty slot.
fn selected_weapon_range(
    attacker: &GameEntity,
    target: &TargetKind,
    sim: &crate::sim::world::Simulation,
    rules: &RuleSet,
    interner: &StringInterner,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<i32> {
    let obj = interner.object_type(attacker.type_ref(), rules)?;
    combat_weapon::select_weapon_against(
        rules,
        attacker,
        obj,
        &combat_weapon::attacker_facts(attacker, obj),
        Some(target),
        sim,
        interner,
        terrain,
        None,
    )
    .and_then(|(_, selected)| selected)
    .map(|selected| selected.weapon.range_leptons)
}

/// `TechnoClass::CanFireAtTarget @ 0x006F7780` is `CanFireAt(target,
/// SelectWeapon(target))` (`0x006F77B0`, then `InRange @ 0x006F7220`), and
/// `FootClass::Approach_Target @ 0x004D5690` measures the same way: neither
/// asks GetFireError. A gun whose warhead cannot hurt the target (T54,
/// ILLEGAL) still has it in range, and Approach still measures with it.
#[test]
fn an_illegal_shot_is_still_in_range() {
    use crate::sim::combat::TargetKind;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n1=HTNK\n[AircraftTypes]\n[BuildingTypes]\n\
         [MTNK]\nStrength=300\nArmor=light\nSpeed=6\nPrimary=Pea\n\
         [HTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
         [Pea]\nDamage=50\nROF=30\nRange=6\nWarhead=Soft\n\
         [Soft]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
    ))
    .expect("illegal-shot rules");
    let mut sim = crate::sim::world::Simulation::new();
    sim.substrate
        .entities
        .insert(make_entity_owned(1, "MTNK", 2, 2, 300, "Soviet"));
    sim.substrate
        .entities
        .insert(make_entity_owned(2, "HTNK", 5, 2, 300, "Americans"));
    sim.interner = test_interner();
    let cells = (0..16u16)
        .flat_map(|ry| {
            (0..16u16).map(move |rx| crate::map::resolved_terrain::test_flat_cell(rx, ry))
        })
        .collect();
    sim.resolved_terrain =
        Some(crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(16, 16, cells));
    let target = TargetKind::Entity(2);
    let tank = sim.substrate.entities.get(1).unwrap();
    let code = crate::sim::combat::fire_error_world::FireSubject {
        world: &sim,
        rules: &rules,
        fog: None,
        firer: tank,
        obj: rules.object("MTNK").unwrap(),
        target: Some(target),
        weapon_index: 0,
    }
    .fire_error(false);
    assert_eq!(code, crate::sim::combat::fire_error::FireError::Illegal);

    assert!(can_fire_at_target(
        &sim,
        &rules,
        &sim.interner,
        1,
        &target,
        sim.resolved_terrain.as_ref().unwrap(),
        None,
        &crate::sim::combat::line_of_fire::LineOfFireInputs::default(),
    ));
    let range = selected_weapon_range(
        tank,
        &target,
        &sim,
        &rules,
        &sim.interner,
        sim.resolved_terrain.as_ref(),
    );
    assert_eq!(range, Some(6 * 256));
}

#[test]
fn combat_resolves_in_live_object_order_not_stable_id() {
    // Two attackers A (stable_id 1) and B (stable_id 2), same owner, both fire
    // at a shared enemy target T (stable_id 3) this tick. Each 105mm shot deals
    // 48 (65 * 75% AP-vs-heavy) to T's 50 HP. The first-resolved attacker's
    // bullet leads the Logic tail and lands this frame; the second is skipped
    // past when the first leaves the vector, so T survives on 2.
    //
    // Phase 4 of tick_combat_with_fog applies damage_events in resolution order;
    // damage_events is built in Phase 2 by walking the snapshots in their sorted
    // order. fire_events is pushed in that same order, so fire_events[0].attacker_id
    // is exactly the first-resolved attacker. The new sort keys on live_order
    // position (stable_id tiebreak), so passing live_order = [2, 1] must make B
    // resolve first, and live_order = &[] must fall back to stable-id order (A first).
    fn build() -> (EntityStore, RuleSet, StringInterner) {
        let rules = test_rules();
        let mut store = EntityStore::new();
        // A and B are co-located is irrelevant; both are in range (<= 6) of T.
        store.insert(make_entity(1, "MTNK", 5, 5, 300)); // attacker A
        store.insert(make_entity(2, "MTNK", 6, 5, 300)); // attacker B
        store.insert(make_entity(3, "MTNK", 5, 6, 50)); // shared target T
        let interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 3);
        install_entity_attack_target_for_test(&mut store, 2, 3);
        (store, rules, interner)
    }
    let mut main_rng = SimRng::new(1);

    // Run 1: live order [B(2), A(1)] (reversed vs stable-id). B resolves first:
    // it fires first and lands the lethal shot.
    {
        let (mut store, rules, mut interner) = build();
        align_attackers_to_targets(&mut store);
        let result = tick_with_map(
            &mut store,
            &rules,
            &mut interner,
            None,
            None,
            None,
            &[2, 1],
            &mut main_rng,
        );
        assert_eq!(
            result.consequences.fire_events()[0].attacker_id,
            2,
            "live order [2,1]: B (live-order-first) must fire first"
        );
        // B's bullet leads the Logic tail and lands; A's is skipped this frame.
        assert_eq!(store.get(3).unwrap().health.current(), 2);
        assert_eq!(result.projectile_spawns.len(), 1);
        assert_eq!(result.projectile_spawns[0].source_id, 1);
    }

    // Run 2: empty live order falls back to stable-id order [A(1), B(2)]. A now
    // resolves first and fires first - the OPPOSITE of run 1 - proving live_order
    // controls the resolution sequence and that &[] reproduces the prior order.
    {
        let (mut store, rules, mut interner) = build();
        align_attackers_to_targets(&mut store);
        let result = tick(&mut store, &rules, &mut interner, 0, &mut main_rng);
        assert_eq!(
            result.consequences.fire_events()[0].attacker_id,
            1,
            "empty live order: stable-id fallback fires A first"
        );
        assert_eq!(store.get(3).unwrap().health.current(), 2);
        assert_eq!(result.projectile_spawns.len(), 1);
        assert_eq!(result.projectile_spawns[0].source_id, 2);
    }
}
