//! Damage commit and its receivers: condition crossings, considered-aircraft
//! infantry, WAD/wall/wood routing, area dispatch, terrain receivers and the
//! combat death handoff to lifecycle.

use super::*;

fn building_damage_state_aoe_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "\
[InfantryTypes]\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[GAPOWR]\nStrength=100\nArmor=wood\n\n\
[105mm]\nDamage=20\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nCellSpread=1\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\n\
[AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n",
    );
    RuleSet::from_ini(&ini).expect("building damage state AoE rules should parse")
}

#[test]
fn gsi_04_10_projectile_inert_suppresses_bridge_ore_and_collector_rng() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=WH\n\
         [WH]\nWall=yes\nCellSpread=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("inert projectile rules");
    let mut interner = test_interner();
    let warhead = interner.intern("WH");
    let weapon = interner.intern("MissingWeapon");
    let detonation = ProjectileDetonation {
        projectile_id: 1,
        source_id: 99,
        target: ProjectileTarget::Cell { rx: 8, ry: 5 },
        impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
        payload: payload_from_weapon(100, warhead, weapon, &rules, &mut interner),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut entities = EntityStore::new();
    let mut scenario_rng = SimRng::new(77);
    let before_rng = scenario_rng.state();

    let emit = emit_detonations(
        &[detonation],
        &mut entities,
        &rules,
        &mut interner,
        true,
        &mut scenario_rng,
        None,
    );

    assert!(emit.damage_events.is_empty());
    assert!(emit.effects.wall_mutations.is_empty());
    assert!(emit.effects.cell_target_detaches.is_empty());
    assert!(!emit.effects.bridge_state_changed);
    assert!(emit.effects.tiberium_reduction_requests.is_empty());
    assert_eq!(scenario_rng.state(), before_rng);
}

#[test]
fn lifecycle_authority_immediate_combat_death_reaches_uninit_without_precleanup() {
    let rules = test_rules();
    let mut store = EntityStore::new();

    let mut dead = make_entity(1, "MTNK", 5, 5, 0);
    dead.seldom.selection.set_selected_for_test(true);
    dead.attack_target.set_for_test(Some(TargetKind::Entity(2)));
    dead.navigation
        .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::cell(9, 9)));
    dead.seldom.in_logic_vector.set_for_test(true);
    dead.lifecycle.set_in_limbo_for_test(false);
    dead.lifecycle.set_cell_marked_for_test(true);
    dead.seldom.radio_contacts.insert(2);
    store.insert(dead);

    let mut observer = make_entity(2, "MTNK", 6, 5, 300);
    observer
        .attack_target
        .set_for_test(Some(TargetKind::Entity(1)));
    store.insert(observer);

    let mut interner = test_interner();
    let result = run_combat_death_handoff(&mut store, &rules, &mut interner, &[1], &[]);

    assert_eq!(result.immediate_uninit_ids, vec![1]);
    assert_eq!(result.despawned_ids, vec![1]);
    let dead = store
        .get(1)
        .expect("world must still be able to UnInit the victim");
    assert_eq!(dead.health.current(), 0);
    assert!(
        !dead.death.is_dying(),
        "immediate UnInit, not combat, owns the death gate"
    );
    assert!(
        dead.seldom.selection.is_selected(),
        "UnInit owns deselection"
    );
    assert!(
        dead.attack_target.target().is_some(),
        "UnInit owns attack cleanup"
    );
    assert!(
        dead.navigation.nav_com().is_some(),
        "UnInit owns movement cleanup"
    );
    assert!(
        dead.seldom.in_logic_vector.is_member(),
        "UnInit owns LogicVector removal"
    );
    assert!(dead.lifecycle.object_alive());
    assert!(!dead.lifecycle.in_limbo());
    assert!(dead.lifecycle.cell_marked());
    assert!(
        dead.seldom.radio_contacts.contains(2),
        "Techno Limbo owns BREAK"
    );
    assert!(
        !dead.seldom.kill_record.is_recorded(),
        "world lifecycle owns count release"
    );
    assert!(
        store.get(2).unwrap().attack_target.target().is_some(),
        "combat must not bulk-clear other objects' targets"
    );
}

#[test]
fn lifecycle_authority_combat_leaves_transport_cargo_for_carrier_uninit() {
    let rules = test_rules();
    let mut store = EntityStore::new();

    let mut cargo = crate::sim::passenger::PassengerCargo::new(2, 1.0);
    cargo.board_forced_chain(&[(2, 1.0)]);
    let mut carrier = make_entity(1, "MTNK", 5, 5, 0);
    carrier.set_passenger_role_for_test(crate::sim::passenger::PassengerRole::transport(cargo));
    store.insert(carrier);

    let mut passenger = make_infantry_entity(2, "E1", 5, 5, 125);
    passenger.set_passenger_role_for_test(crate::sim::passenger::PassengerRole::inside_for_test(
        1, false,
    ));
    passenger.seldom.selection.set_selected_for_test(true);
    passenger
        .attack_target
        .set_for_test(Some(TargetKind::Entity(3)));
    passenger
        .navigation
        .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::cell(9, 9)));
    store.insert(passenger);
    store.insert(make_entity(3, "MTNK", 6, 5, 300));

    let mut interner = test_interner();
    let result = run_combat_death_handoff(&mut store, &rules, &mut interner, &[1], &[]);

    assert_eq!(result.immediate_uninit_ids, vec![1]);
    let carrier = store.get(1).unwrap();
    assert_eq!(
        carrier.passenger_role().cargo().unwrap().passengers(),
        vec![2]
    );
    let passenger = store.get(2).unwrap();
    assert_eq!(passenger.health.current(), 125);
    assert!(!passenger.death.is_dying());
    assert_eq!(passenger.passenger_role().inside_transport_id(), Some(1));
    assert!(passenger.seldom.selection.is_selected());
    assert!(passenger.attack_target.target().is_some());
    assert!(passenger.navigation.nav_com().is_some());
}

#[test]
fn lifecycle_authority_animated_combat_handoff_changes_only_health_and_sequence() {
    let rules = infantry_fire_frame_rules_with("[SA]\nInfDeath=1\n");
    let mut store = EntityStore::new();

    let mut dead = make_infantry_entity(1, "E1", 5, 5, 0);
    dead.seldom.selection.set_selected_for_test(true);
    dead.attack_target.set_for_test(Some(TargetKind::Entity(2)));
    dead.navigation
        .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::cell(9, 9)));
    dead.seldom.in_logic_vector.set_for_test(true);
    dead.lifecycle.set_in_limbo_for_test(false);
    dead.lifecycle.set_cell_marked_for_test(true);
    dead.seldom.radio_contacts.insert(2);
    store.insert(dead);

    let mut observer = make_entity(2, "MTNK", 6, 5, 300);
    observer
        .attack_target
        .set_for_test(Some(TargetKind::Entity(1)));
    store.insert(observer);

    let mut interner = test_interner();
    // The killing call's record: its InfDeath=1 warhead takes the Die1 arm.
    let killing = EntityDamageEvent::area(1, 125, 0, RAD_NO_ATTACKER, None, interner.intern("SA"));
    let result = run_combat_death_handoff(&mut store, &rules, &mut interner, &[1], &[killing]);

    assert!(result.immediate_uninit_ids.is_empty());
    assert_eq!(result.despawned_ids, vec![1]);
    let dead = store.get(1).unwrap();
    assert_eq!(dead.health.current(), 0);
    // The Die1 corpse carries no mark of its own.
    assert!(!dead.death.is_dying());
    assert_eq!(dead.mission_leaf.as_infantry().unwrap().doing(), 11);
    assert_eq!(dead.native_stage().value(), 0);
    assert_eq!(dead.infantry_sprite_pose(), Some((11, 0)));
    assert!(dead.seldom.selection.is_selected());
    assert!(dead.attack_target.target().is_some());
    assert!(dead.navigation.nav_com().is_some());
    assert!(dead.seldom.in_logic_vector.is_member());
    assert!(dead.lifecycle.object_alive());
    assert!(!dead.lifecycle.in_limbo());
    assert!(dead.lifecycle.cell_marked());
    assert!(dead.seldom.radio_contacts.contains(2));
    assert!(!dead.seldom.kill_record.is_recorded());
    assert!(
        store.get(2).unwrap().attack_target.target().is_some(),
        "selective removal listeners, not combat, own target invalidation"
    );
}

fn considered_aircraft_weapon_rules() -> RuleSet {
    let ini_str: &str = "\
[InfantryTypes]
0=ROCK
1=E1
[VehicleTypes]
0=IFV
[AircraftTypes]
[BuildingTypes]

[IFV]
Strength=200
Armor=light
Speed=8
Primary=GroundGun
Secondary=AirGun

[ROCK]
Strength=125
Armor=none
Speed=8
ConsideredAircraft=yes

[E1]
Strength=125
Armor=none
Speed=4

[GroundGun]
Damage=10
ROF=20
Range=7
Projectile=GroundProj
Warhead=TestWH

[AirGun]
Damage=10
ROF=20
Range=7
Projectile=AirProj
Warhead=TestWH

[GroundProj]
AG=yes
AA=no

[AirProj]
AG=no
AA=yes

[TestWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";
    let ini: IniFile = IniFile::from_str(ini_str);
    RuleSet::from_ini(&ini).expect("considered-aircraft combat rules should parse")
}

#[test]
/// Re-baselined for GSI-08.02 (row 122). Weapon selection is altitude-driven,
/// not category-driven: `TechnoClass::What_Weapon_Should_I_Use @ 0x006F3330`
/// arm V and the `GetFireError` AA gate both call `ObjectClass::IsHighFlying`
/// (`0x005F6B90`, vtable `+0x54`) = marked on the map and
/// `GetHeight() >= 2 * LeptonsPerLevel`. `ConsideredAircraft=` is not read
/// anywhere in that path. A Rocketeer standing on the ground is therefore an
/// ordinary ground target and draws the ground gun; it becomes an air target
/// only once it climbs past two levels.
fn considered_aircraft_infantry_is_air_only_while_high_flying() {
    // One fresh engagement per altitude — the attacker's ROF cooldown makes a
    // second shot in the same sim unobservable.
    fn fire_at_rocketeer(altitude_leptons: i64) -> (String, WeaponSlot) {
        let rules = considered_aircraft_weapon_rules();
        let mut sim = crate::sim::world::Simulation::new();
        let attacker = sim
            .spawn_object("IFV", "Americans", 5, 5, 0, &rules)
            .expect("IFV should spawn");
        let target = sim
            .spawn_object("ROCK", "Soviet", 8, 5, 0, &rules)
            .expect("Rocketeer should spawn");

        let target_entity = sim
            .substrate
            .entities
            .get(target)
            .expect("target should exist");
        assert_eq!(target_entity.category(), EntityCategory::Infantry);
        assert!(
            sim.interner
                .object_type(target_entity.type_ref(), &rules)
                .is_some_and(|obj| obj.considered_aircraft)
        );

        if altitude_leptons > 0 {
            // Airborne: its Location Z above the mapless ground.
            let rocketeer = sim
                .substrate
                .entities
                .get_mut(target)
                .expect("target should exist");
            rocketeer
                .position
                .set_exact_z_leptons_for_test(Some(altitude_leptons as i32));
        }

        install_entity_attack_target_for_test(&mut sim.substrate.entities, attacker, target);
        let mut main_rng = SimRng::new(1);
        align_attackers_to_targets(&mut sim.substrate.entities);
        let result = tick_combat(
            &mut sim.substrate.entities,
            &mut sim.substrate.occupancy,
            &rules,
            &mut sim.interner,
            0,
            100,
            0,
            &mut main_rng,
        );
        assert_eq!(result.consequences.fire_events().len(), 1);
        (
            sim.interner
                .resolve(result.consequences.fire_events()[0].weapon_id)
                .to_string(),
            result.consequences.fire_events()[0].weapon_slot,
        )
    }

    // Grounded: not high-flying, so the ladder falls through arm V to slot 0
    // and the AG-only GroundProj is legal.
    assert_eq!(
        fire_at_rocketeer(0),
        ("GroundGun".to_string(), WeaponSlot::Primary)
    );
    // Airborne past two levels: arm V picks the AA secondary.
    assert_eq!(
        fire_at_rocketeer(crate::util::lepton::HIGH_FLIGHT_THRESHOLD_LEPTONS),
        ("AirGun".to_string(), WeaponSlot::Secondary)
    );
}

#[test]
fn combat_damage_crosses_live_type_condition_yellow() {
    let rules = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_structure_entity(2, "GAPOWR", 8, 5, 400, 750));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    align_attackers_to_targets(&mut store);
    tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    assert!(
        store
            .get(2)
            .expect("building survives")
            .health
            .compare_ratio(
                rules.object("GAPOWR").unwrap().strength,
                rules.general.condition_yellow
            )
            == crate::util::native_x87::MaskedX87Ordering::Less
    );
}

#[test]
fn aoe_damage_crosses_live_type_condition_yellow() {
    let rules = building_damage_state_aoe_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_structure_entity(2, "GAPOWR", 8, 5, 60, 100));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    align_attackers_to_targets(&mut store);
    tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    assert!(
        store
            .get(2)
            .expect("building survives")
            .health
            .compare_ratio(
                rules.object("GAPOWR").unwrap().strength,
                rules.general.condition_yellow
            )
            == crate::util::native_x87::MaskedX87Ordering::Less
    );
}

#[test]
fn gsi_04_07_damage_wad_precedes_wall_and_wood_armor_routing() {
    fn fire(extra_warhead_flags: &str, overlay_armor: &str) -> (CombatTickResult, OverlayGrid) {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=KillWH\n1=WallWH\n2=NoWallWH\n\
             [OverlayTypes]\n0=TESTWALL\n\
             [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=Gun\n\
             [Gun]\nDamage=65\nROF=50\nRange=6\nWarhead=WH\n\
             [WH]\n{extra_warhead_flags}\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TESTWALL]\nWall=yes\nArmor={overlay_armor}\nStrength=1\n"
        ));
        let rules = RuleSet::from_ini(&ini).expect("wall route rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let mut entities = EntityStore::new();
        entities.insert(make_entity(1, "MTNK", 5, 5, 300));
        entities.insert(make_entity(2, "MTNK", 8, 5, 300));
        let mut interner = test_interner();
        let _handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        install_entity_attack_target_for_test(&mut entities, 1, 2);
        let mut overlays = OverlayGrid::new(12, 12);
        let mut terrain = crate::sim::tiberium::test_support::flat_terrain(12, 12);
        overlays.place_overlay(8, 5, 0, 0);
        align_attackers_to_targets(&mut entities);
        let result = tick_with_map(
            &mut entities,
            &rules,
            &mut interner,
            Some(&mut overlays),
            Some(&registry),
            Some(&mut terrain),
            &[],
            &mut SimRng::new(1),
        );
        (result, overlays)
    }

    let (absolute, absolute_grid) = fire("WallAbsoluteDestroyer=yes\nWall=yes", "concrete");
    assert_eq!(
        absolute.consequences.effects().wall_mutations,
        vec![crate::sim::overlay_grid::WallMutation {
            rx: 8,
            ry: 5,
            kind: crate::sim::overlay_grid::WallMutationKind::DirectRemoved,
        }],
        "WallAbsoluteDestroyer wins and commits forced removal inline"
    );
    assert_eq!(absolute_grid.cell(8, 5).overlay_id, None);
    assert!(!absolute.consequences.effects().bridge_state_changed);

    let (wood, wood_grid) = fire("Wood=yes", "wood");
    assert!(!wood.consequences.effects().wall_mutations.is_empty());
    assert_eq!(wood_grid.cell(8, 5).overlay_id, None);
    let (concrete, concrete_grid) = fire("Wood=yes", "concrete");
    assert!(concrete.consequences.effects().wall_mutations.is_empty());
    assert_eq!(concrete_grid.cell(8, 5).overlay_id, Some(0));
}

// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn gsi_04_07_damage_wall_dies_in_the_tail_after_both_attackers_fire() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [OverlayTypes]\n0=TESTWALL\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=Gun\n\
         [Gun]\nDamage=1\nROF=50\nRange=8\nWarhead=WH\n\
         [WH]\nWallAbsoluteDestroyer=yes\nWall=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("live-order wall rules");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let mut entities = EntityStore::new();
    entities.insert(make_entity(10, "MTNK", 5, 5, 300));
    // Both tanks fire at the wall cell in the object pass. Their (Inviso)
    // bullets detonate in the same frame's Logic tail, where the first one
    // razes the wall and its detach restores the second tank's suspended
    // target (`0x0070D4A0`) for its next AI.
    let mut second = make_entity(20, "MTNK", 4, 5, 300);
    set_mission(
        &mut second,
        MissionType::Attack,
        MissionId::from_known(MissionType::Guard),
    );
    second
        .seldom
        .suspended_attack_target
        .set_for_test(Some(TargetKind::Entity(10)));
    entities.insert(second);
    let mut interner = test_interner();
    let _handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    assert!(install_cell_attack_target_for_test(
        &mut entities,
        10,
        8,
        5,
        Some(&rules),
        &interner,
    ));
    assert!(install_cell_attack_target_for_test(
        &mut entities,
        20,
        8,
        5,
        Some(&rules),
        &interner,
    ));

    let mut overlays = OverlayGrid::new(16, 16);
    let mut terrain = crate::sim::tiberium::test_support::flat_terrain(16, 16);
    overlays.place_overlay(8, 5, 0, 0);
    let mut scenario_rng = SimRng::new(31);
    // Two attackers fire this tick, so the scenario stream advances by exactly
    // two `GetROF` reload jitters (`RandomRanged(0, 2)` @ `0x006FD0B0`) and
    // nothing else — in particular the WAD warhead still draws no Strength.
    let mut expected_rng = scenario_rng.clone();
    expected_rng.next_range_u32_inclusive(0, 2);
    expected_rng.next_range_u32_inclusive(0, 2);
    let before = expected_rng.state();
    align_attackers_to_targets(&mut entities);
    let result = tick_with_map(
        &mut entities,
        &rules,
        &mut interner,
        Some(&mut overlays),
        Some(&registry),
        Some(&mut terrain),
        &[10, 20],
        &mut scenario_rng,
    );

    assert_eq!(overlays.cell(8, 5).overlay_id, None);
    assert_eq!(
        scenario_rng.state(),
        before,
        "WAD consumes no Strength draw beyond the two reload jitters"
    );
    assert_eq!(
        result
            .consequences
            .fire_events()
            .iter()
            .map(|event| (event.attacker_id, event.target))
            .collect::<Vec<_>>(),
        vec![(10, TargetKind::Cell(8, 5)), (20, TargetKind::Cell(8, 5))],
        "both shots are Inviso bullets, so the wall stands until the tail"
    );
    assert_eq!(
        result
            .consequences
            .effects()
            .cell_target_detaches
            .iter()
            .map(|event| (event.listener_id, event.restored, event.cleared))
            .collect::<Vec<_>>(),
        vec![(20, true, false), (10, false, true)]
    );
}

#[test]
fn gsi_04_07_damage_repair_bullet_cellspread_zero_keeps_signed_area_record() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=REPAIRER\n1=TARGET\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [REPAIRER]\nStrength=100\nArmor=light\nPrimary=RepairBullet\n\
         [TARGET]\nStrength=180\nArmor=heavy\n\
         [RepairBullet]\nDamage=-50\nROF=80\nRange=1.8\nProjectile=Invisible\nSpeed=100\nWarhead=Mechanical\n\
         [Mechanical]\nVerses=0%,0%,0%,100%,100%,100%,0%,0%,0%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("RepairBullet/Mechanical rules");
    let weapon = rules.weapon("RepairBullet").expect("RepairBullet");
    let warhead = rules.warhead("Mechanical").expect("Mechanical");
    assert_eq!(weapon.damage, -50);
    assert_eq!(warhead.cell_spread_f64, 0.0);

    let mut entities = EntityStore::new();
    let mut target = make_entity(10, "TARGET", 8, 5, 180);
    target.health.set_for_test(100);
    entities.insert(target);
    let mut interner = test_interner();
    let warhead_ref = interner.intern("Mechanical");
    let weapon_ref = interner.intern("RepairBullet");
    let detonation = ProjectileDetonation {
        projectile_id: 1,
        source_id: 77,
        target: ProjectileTarget::Entity(10),
        impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
        payload: payload_from_weapon(
            weapon.damage,
            warhead_ref,
            weapon_ref,
            &rules,
            &mut interner,
        ),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let emitted = emit_detonations(
        &[detonation],
        &mut entities,
        &rules,
        &mut interner,
        false,
        &mut SimRng::new(9),
        None,
    );
    // Firer 77 is gone, so the bullet has no Owner and DamageArea takes no
    // source house (`0x00469A69..0x00469A75`).
    let mut expected = EntityDamageEvent::area(10, -50, 0, 77, None, warhead_ref);
    expected.near_center_ic_isolation_eligible = true;
    assert_eq!(
        emitted.damage_events,
        vec![combat_aoe::AreaDamageReceiver::Entity(expected)],
        "CellSpread=0 still enters the center receiver scan with raw signed damage"
    );

    // The detonation commits its receivers inline: one heal of 50.
    assert_eq!(entities.get(10).unwrap().health.current(), 150);
}

/// `Apply_area_damage`'s dispatch skips an `InvisibleInGame=` building
/// (BuildingType `+0x1701`, `0x00489A1B..0x00489A29`), such as the invisible
/// light posts, while a visible building in the same blast takes its record.
#[test]
fn gsi_04_07_area_dispatch_skips_an_invisible_in_game_building() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=LAMP\n1=SHED\n\
         [LAMP]\nStrength=6000\nArmor=wood\nInvisibleInGame=yes\n\
         [SHED]\nStrength=300\nArmor=wood\n\
         [Warheads]\n0=HE\n\
         [HE]\nCellSpread=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("invisible-building fixture parses");
    let mut entities = EntityStore::new();
    entities.insert(make_structure_entity(1, "LAMP", 5, 5, 6000, 6000));
    entities.insert(make_structure_entity(2, "SHED", 6, 5, 300, 300));
    let mut interner = test_interner();
    let warhead = interner.intern("HE");
    let house = Some(interner.intern("Test"));
    let receivers = [1, 2].map(|id| {
        combat_aoe::AreaDamageReceiver::Entity(EntityDamageEvent::area(
            id, 100, 0, 77, house, warhead,
        ))
    });
    let mut main_rng = SimRng::new(3);
    let mut scenario_rng = SimRng::new(4);
    let mut handled_deaths = Vec::new();
    let mut houses = crate::sim::houses::Houses::default();
    let mut fatal_lifecycle = None;
    let mut sound_sink = None;
    commit_area_damage_receivers(
        &receivers,
        &mut entities,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        &mut houses,
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        None,
        0,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );
    assert_eq!(entities.get(1).unwrap().health.current(), 6000);
    assert_eq!(entities.get(2).unwrap().health.current(), 200);
}

#[test]
fn gsi_04_10_near_center_iron_curtain_isolates_earlier_terrain_receiver() {
    use crate::sim::combat::combat_aoe::AreaDamageReceiver;
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    use crate::sim::terrain_object::TerrainObjectState;

    fn run(kind: InvulnKind, techno_distance: i32) -> i32 {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nTreeStrength=100\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=VICTIM\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n\
             [Warheads]\n0=WOODWH\n\
             [VICTIM]\nStrength=100\nArmor=wood\n\
             [TREE01]\nStrength=100\nArmor=wood\nImmune=no\n\
             [WOODWH]\nWood=yes\nCellSpread=.5\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
        ))
        .expect("Terrain isolation rules");
        let mut sim = crate::sim::world::Simulation::new();
        sim.resolve_type_handles(&rules);
        let victim_id = sim
            .spawn_object("VICTIM", "VictimHouse", 5, 5, 0, &rules)
            .expect("protected Techno spawns");
        sim.substrate
            .entities
            .get_mut(victim_id)
            .unwrap()
            .invulnerability = Some(InvulnerabilityState::new(
            crate::sim::timer::CdTimer::started(0, 100),
            kind,
        ));

        let terrain_id = 700;
        let terrain_ref = sim.interner.intern("TREE01");
        sim.terrain_objects
            .objects_mut_for_test()
            .insert(terrain_id, {
                let mut terrain = TerrainObjectState::for_test(terrain_id, terrain_ref, 5, 5);
                terrain.set_occupation_bits_for_test(4);
                terrain
            });
        sim.terrain_objects
            .cell_index_mut_for_test()
            .insert((5, 5), terrain_id);

        let warhead_ref = sim.interner.intern("WOODWH");
        let mut entity_event = EntityDamageEvent::area(
            victim_id,
            10,
            techno_distance,
            RAD_NO_ATTACKER,
            None,
            warhead_ref,
        );
        entity_event.near_center_ic_isolation_eligible = true;
        let receivers = [
            AreaDamageReceiver::Terrain(TerrainDamageEvent {
                damage_writeback: None,
                stable_id: terrain_id,
                rx: 5,
                ry: 5,
                damage: 10,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: true,
            }),
            AreaDamageReceiver::Entity(entity_event),
        ];
        sim.commit_noncombat_aoe_receivers(
            &rules,
            None,
            &receivers,
            &mut crate::sim::world::FrameEffects::default(),
        );
        sim.terrain_objects.objects()[&terrain_id].health()
    }

    assert_eq!(
        run(InvulnKind::IronCurtain, 84),
        100,
        "the later active IC record arms isolation for an earlier Terrain record"
    );
    assert_eq!(
        run(InvulnKind::IronCurtain, 85),
        90,
        "the native distance boundary is strict less-than 85"
    );
    assert_eq!(
        run(InvulnKind::ForceShield, 84),
        90,
        "Force Shield receives through isolation but never arms it"
    );
}

#[test]
fn gsi_04_10_entity_fatal_hook_and_later_terrain_share_raw_occupation() {
    use crate::sim::combat::combat_aoe::AreaDamageReceiver;
    use crate::sim::terrain_object::{
        TerrainObjectLifecycle, TerrainObjectState, mark_terrain_raw_occupation,
    };

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nTreeStrength=10\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TerrainTypes]\n0=TREE01\n\
         [Warheads]\n0=WOODWH\n\
         [VICTIM]\nStrength=10\nArmor=wood\nSpeed=6\n\
         [TREE01]\nStrength=10\nArmor=wood\nImmune=no\n\
         [WOODWH]\nWood=yes\nCellSpread=1\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("shared raw-occupation rules");
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolve_type_handles(&rules);
    let entity_id = sim
        .spawn_object("VICTIM", "VictimHouse", 4, 5, 0, &rules)
        .expect("fatal vehicle spawns");
    let terrain_id = 701;
    let terrain_ref = sim.interner.intern("TREE01");
    sim.terrain_objects
        .objects_mut_for_test()
        .insert(terrain_id, {
            let mut terrain = TerrainObjectState::for_test(terrain_id, terrain_ref, 5, 5);
            terrain.set_health_for_test(10);
            terrain.set_max_health_for_test(10);
            terrain.set_occupation_bits_for_test(4);
            terrain
        });
    sim.terrain_objects
        .cell_index_mut_for_test()
        .insert((5, 5), terrain_id);
    mark_terrain_raw_occupation(&mut sim.substrate.raw_cell_occupation, (5, 5), 4);
    assert_ne!(sim.substrate.raw_cell_occupation.ground_bits(4, 5), 0);
    assert_ne!(sim.substrate.raw_cell_occupation.ground_bits(5, 5), 0);

    let warhead_ref = sim.interner.intern("WOODWH");
    let receivers = [
        AreaDamageReceiver::Entity(EntityDamageEvent::area(
            entity_id,
            10,
            0,
            RAD_NO_ATTACKER,
            None,
            warhead_ref,
        )),
        AreaDamageReceiver::Terrain(TerrainDamageEvent {
            damage_writeback: None,
            stable_id: terrain_id,
            rx: 5,
            ry: 5,
            damage: 10,
            distance_leptons: 0,
            warhead_ref,
            near_center_ic_isolation_eligible: false,
        }),
    ];
    sim.commit_noncombat_aoe_receivers(
        &rules,
        None,
        &receivers,
        &mut crate::sim::world::FrameEffects::default(),
    );

    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(4, 5),
        0,
        "World UnInit clears the Techno bit through the lent authoritative raw grid"
    );
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(5, 5),
        0,
        "the later Terrain finalize observes and mutates that same grid"
    );
    assert_eq!(
        sim.terrain_objects.objects()[&terrain_id].lifecycle(),
        TerrainObjectLifecycle::Destroyed
    );
}
