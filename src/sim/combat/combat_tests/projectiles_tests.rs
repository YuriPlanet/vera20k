//! Projectiles from shot to impact: inviso detonations and their RNG,
//! persistent projectiles, launch coords and speeds, vertical bombs, special
//! arms, clusters and rockers.

use super::*;

fn inviso_weapon_rules(inviso: bool, with_anim: bool) -> RuleSet {
    shooter_rules(
        if inviso {
            "Inviso=yes\n"
        } else {
            "Inviso=no\n"
        },
        if with_anim { "AnimList=PIFF\n" } else { "" },
    )
}

/// A SHOOTER whose `Range=10` GUN fires TESTPROJ, with `projectile` and
/// `warhead` keys, at a TARGET.
fn shooter_rules(projectile: &str, warhead: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "\
[Animations]\n0=PIFF\n\
[InfantryTypes]\n\n\
[VehicleTypes]\n0=SHOOTER\n1=TARGET\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[SHOOTER]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\n\
[TARGET]\nStrength=500\nArmor=heavy\nSpeed=6\n\n\
[GUN]\nDamage=10\nROF=20\nRange=10\nProjectile=TESTPROJ\nWarhead=TESTWH\n\n\
[TESTPROJ]\n{projectile}\n\
[TESTWH]\n{warhead}\
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("shooter test rules should parse")
}

fn persistent_projectile_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\n[VehicleTypes]\n0=SHOOTER\n1=TARGET\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n[SHOOTER]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\n[TARGET]\nStrength=500\nArmor=heavy\nSpeed=6\n\n[GUN]\nDamage=10\nROF=20\nRange=10\nSpeed=128\nProjectile=TESTPROJ\nWarhead=TESTWH\n\n[TESTPROJ]\nInviso=no\nImage=TESTBULLET\n\n[TESTWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("persistent projectile rules should parse")
}

#[test]
fn fire_admission_preserves_flat_projectile_layer_through_save_and_retirement() {
    use crate::sim::world::display_layers::DisplayLayer;
    for flat in [false, true] {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str("[VehicleTypes]\n0=SHOOTER\n1=TARGET\n\
                [SHOOTER]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\
                [TARGET]\nStrength=500\nArmor=heavy\nSpeed=6\n\
                [GUN]\nDamage=10\nROF=20\nRange=10\nSpeed=128\nProjectile=TESTPROJ\nWarhead=TESTWH\n\
                [TESTPROJ]\nImage=SHOT\nROT=0\nArcing=yes\n\
                [TESTWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"),
            &IniFile::from_str(&format!("[SHOT]\nFlat={}\n", if flat { "yes" } else { "no" })),
        ).unwrap();
        assert_eq!(rules.projectile("TESTPROJ").unwrap().flat, flat);
        let mut entities = EntityStore::new();
        entities.insert(make_entity(1, "SHOOTER", 5, 5, 300));
        entities.insert(make_entity(2, "TARGET", 8, 5, 500));
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut entities, 1, 2);
        align_attackers_to_targets(&mut entities);
        let fire = tick(&mut entities, &rules, &mut interner, 0, &mut SimRng::new(1));
        assert_eq!(fire.projectile_spawns.len(), 1);
        assert_eq!(fire.projectile_spawns[0].flat, flat);
        let mut sim = crate::sim::world::Simulation::new();
        *sim.interner = interner;
        // Preserve the real firer and target identities across the save.
        assert_eq!(sim.substrate.allocate_stable_id(), 1);
        assert_eq!(sim.substrate.allocate_stable_id(), 2);
        *sim.substrate.entities = entities;
        let id = sim.substrate.allocate_stable_id();
        sim.admit_projectile(id, fire.projectile_spawns[0]);
        let layer = if flat {
            DisplayLayer::SURFACE
        } else {
            DisplayLayer::AIR
        };
        assert_eq!(sim.substrate.display.members(layer), [id]);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "flat-shot", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.substrate.display.members(layer), [id]);
        assert!(restored.retire_non_entity_object(id, crate::sim::world::UninitContext::default()));
        assert_eq!(restored.substrate.display.layer_of(id), None);
        assert!(
            restored.projectiles.get(id).is_some(),
            "display removal precedes deferred free"
        );
        restored.process_pending_delete();
        assert!(restored.projectiles.get(id).is_none());
    }
}

#[test]
fn gsi_04_11_persistent_projectile_keeps_exact_lepton_z() {
    let rules = persistent_projectile_rules();
    // These are ground objects on levels 7 and 11, not aircraft above a
    // missing map's zero floor. The live flight queries need that terrain;
    // the legacy position level does not own it.
    let mut terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        12,
        12,
        (0..12)
            .flat_map(|y| {
                (0..12).map(move |x| {
                    let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
                    cell.level = if x >= 8 { 11 } else { 7 };
                    cell
                })
            })
            .collect(),
    );
    let mut entities = EntityStore::new();
    let mut shooter = make_entity(1, "SHOOTER", 5, 5, 300);
    shooter.position.set_z_for_test(7);
    shooter.position.set_exact_z_leptons_for_test(Some(733));
    entities.insert(shooter);
    let mut target = make_entity(2, "TARGET", 8, 5, 500);
    target.position.set_z_for_test(11);
    target.position.set_exact_z_leptons_for_test(Some(1_177));
    entities.insert(target);
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut entities, 1, 2);

    align_attackers_to_targets(&mut entities);
    let result = tick_with_map(
        &mut entities,
        &rules,
        &mut interner,
        None,
        None,
        Some(&mut terrain),
        &[],
        &mut SimRng::new(1),
    );

    assert_eq!(result.projectile_spawns.len(), 1);
    assert_eq!(result.projectile_spawns[0].origin.z, 733);
    assert_eq!(result.projectile_spawns[0].initial_target_position.z, 1_177);
}

#[test]
fn persistent_projectile_delays_damage_across_save_load_continuation() {
    let rules = persistent_projectile_rules();
    assert!(matches!(
        classify_projectile_delivery(rules.weapon("GUN").unwrap(), &rules),
        ProjectileDelivery { .. }
    ));
    let mut entities = EntityStore::new();
    entities.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    entities.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut entities, 1, 2);

    let mut scenario_rng = SimRng::new(1);
    align_attackers_to_targets(&mut entities);
    let fire = tick(&mut entities, &rules, &mut interner, 0, &mut scenario_rng);
    assert_eq!(entities.get(2).unwrap().health.current(), 500);
    assert_eq!(fire.projectile_spawns.len(), 1);

    let mut sim = crate::sim::world::Simulation::new();
    let projectile_id = sim.substrate.allocate_stable_id();
    sim.admit_projectile(projectile_id, fire.projectile_spawns[0]);
    let target_positions =
        BTreeMap::from([(2, ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0))]);
    let shared_cell_dummy = sim.effective_shared_cell_dummy();
    assert!(
        sim.projectiles
            .advance(0, &target_positions, None, &shared_cell_dummy, |_, _| None,)
            .detonations
            .is_empty()
    );

    let snapshot = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "projectile-flight", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&snapshot)
        .expect("pending projectile snapshot should load")
        .sim;
    let restored_shared_cell_dummy = restored.effective_shared_cell_dummy();
    let mut detonations = Vec::new();
    for _ in 0..8 {
        detonations = restored
            .projectiles
            .advance(
                0,
                &target_positions,
                None,
                &restored_shared_cell_dummy,
                |_, _| {
                    Some(
                        crate::sim::projectile::ProjectileCollisionResponse::TargetZClamp(
                            target_positions[&2],
                        ),
                    )
                },
            )
            .detonations;
        if !detonations.is_empty() {
            break;
        }
    }
    assert_eq!(
        detonations.len(),
        1,
        "resumed projectile must deliver the supplied world admission"
    );

    entities.remove(1);
    align_attackers_to_targets(&mut entities);
    tick_detonations(
        &mut entities,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        &mut crate::sim::houses::Houses::default(),
        &HouseAllianceMap::new(),
        1,
        &[2],
        &detonations,
        &mut scenario_rng,
        &mut SimRng::new(1),
    );
    assert_eq!(entities.get(2).unwrap().health.current(), 490);
}

/// One Inviso bullet's detonation draws in the frame's tail: the anim
/// scatter (one raw draw), then the cluster successor
/// (`RandomRanged(0x100, 0x200)` and one raw draw, `0x00469057`).
fn inviso_detonation_draws(
    rng: &mut SimRng,
    coord: (u16, u16, SimFixed, SimFixed),
) -> (u16, u16, SimFixed, SimFixed) {
    let effect = inviso_scatter::scatter_inviso_effect_coord(
        crate::map::cell_index::CellGrid::Retail,
        rng,
        coord.0,
        coord.1,
        coord.2,
        coord.3,
    );
    let _ = crate::sim::projectile::projectile_next_cluster_coord(
        crate::map::cell_index::CellGrid::Retail,
        ProjectileCoord::new(0, 0, 0),
        rng,
    );
    effect
}

// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn inviso_scatter_uses_scenario_rng_only_for_effect_and_paired_smudge() {
    let mut rules = inviso_weapon_rules(true, true);
    initialize_fixture_anim_types(&mut rules);
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    let target_coord = (
        store.get(2).unwrap().position.rx(),
        store.get(2).unwrap().position.ry(),
        store.get(2).unwrap().position.sub_x(),
        store.get(2).unwrap().position.sub_y(),
    );

    let mut scenario_rng = SimRng::new(1);
    let mut expected_rng = scenario_rng.clone();
    // FireAt's `GetROF @ 0x006FCFA0` jitter, then the bullet's detonation in
    // the same frame's tail.
    expected_rng.next_range_u32_inclusive(0, 2);
    let expected_effect = inviso_detonation_draws(&mut expected_rng, target_coord);
    align_attackers_to_targets(&mut store);
    let result = tick(&mut store, &rules, &mut interner, 0, &mut scenario_rng);

    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert_eq!(store.get(2).unwrap().health.current(), 490);
    assert_eq!(result.fixture_anims.len(), 1);
    assert_eq!(
        constructed_anim_coord(&result.fixture_anims[0]),
        expected_effect
    );
    assert_ne!(expected_effect, target_coord);
    assert!(
        result
            .consequences
            .effects()
            .tiberium_reduction_requests
            .is_empty(),
        "a non-Tiberium warhead without authoritative overlay context must not reduce ore"
    );
}

/// Stock `[FlakProj]` (`Inviso=`, `FlakScatter=`, `Inaccurate=`, `Ranged=`)
/// fired at an aircraft. `BulletClass::Fire` moves the burst off its target
/// by two Scenario draws ahead of GetROF's jitter (`0x00468734..0x004688A5`)
/// and hands the moved coordinate to `ProximityDetector::Setup`
/// (`0x00468A80..0x00468A93`), so the fuse measures from there and the burst
/// goes off at its first AI however far it moved. The impact ladder keeps an
/// `Inaccurate=` bullet's Location (`0x00468D80`), so the detonation scatters
/// from the placement. `BallisticScatter=` is raised so the burst can land
/// more than `0x200` out, where a fuse measured from the target would not
/// fire. The shooter fires from its centre.
// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn a_flak_burst_goes_off_where_fire_moved_it() {
    let mut rules = shooter_rules(
        "Inviso=yes\nAA=yes\nAG=no\nRanged=yes\nInaccurate=yes\nFlakScatter=yes\n",
        "AnimList=PIFF\n",
    );
    rules.combat_damage.ballistic_scatter = 0x400;
    initialize_fixture_anim_types(&mut rules);
    let altitude = 1_177;
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    let mut aircraft = make_entity(2, "TARGET", 8, 5, 500);
    aircraft
        .position
        .set_exact_z_leptons_for_test(Some(altitude));
    store.insert(aircraft);
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    let centre = |cell: i32| cell * 256 + 128;
    let target = ProjectileCoord::new(centre(8), centre(5), altitude);
    let place = |rng: &mut SimRng| {
        crate::sim::projectile::launch::bullet_fire_flak_placement(
            ProjectileCoord::new(centre(5), centre(5), 0),
            target,
            rules.combat_damage.ballistic_scatter,
            rules.weapon("GUN").unwrap().range_leptons,
            rng,
        )
    };
    let seed = (1..)
        .find(|&seed| {
            crate::sim::projectile::coord_distance(place(&mut SimRng::new(seed)), target) >= 0x240
        })
        .unwrap();

    let mut scenario_rng = SimRng::new(seed);
    let mut expected_rng = scenario_rng.clone();
    let placement = place(&mut expected_rng);
    expected_rng.next_range_u32_inclusive(0, 2);
    let expected_effect = inviso_detonation_draws(
        &mut expected_rng,
        (
            placement.x.div_euclid(256) as u16,
            placement.y.div_euclid(256) as u16,
            SimFixed::from_num(placement.x.rem_euclid(256)),
            SimFixed::from_num(placement.y.rem_euclid(256)),
        ),
    );
    align_attackers_to_targets(&mut store);
    let result = tick(&mut store, &rules, &mut interner, 0, &mut scenario_rng);

    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert_eq!(result.fixture_anims.len(), 1);
    assert_eq!(
        constructed_anim_coord(&result.fixture_anims[0]),
        expected_effect
    );
}

#[test]
fn inviso_empty_animlist_still_consumes_one_draw() {
    let rules = inviso_weapon_rules(true, false);
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    let mut scenario_rng = SimRng::new(1);
    let mut expected_rng = scenario_rng.clone();
    // GetROF in FireAt, then the tail's scatter and cluster draws.
    expected_rng.next_range_u32_inclusive(0, 2);
    let _ = inviso_detonation_draws(
        &mut expected_rng,
        (8, 5, SimFixed::from_num(128), SimFixed::from_num(128)),
    );
    align_attackers_to_targets(&mut store);
    let result = tick(&mut store, &rules, &mut interner, 0, &mut scenario_rng);

    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert!(result.consequences.effects().explosion_effects.is_empty());
    assert!(
        result
            .consequences
            .effects()
            .smudge_spawn_requests
            .is_empty()
    );
}

// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn gsi_08_05_non_inviso_projectile_advances_scenario_rng_by_the_reload_jitter() {
    let rules = inviso_weapon_rules(false, true);
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    let target_coord = (
        store.get(2).unwrap().position.rx(),
        store.get(2).unwrap().position.ry(),
        store.get(2).unwrap().position.sub_x(),
        store.get(2).unwrap().position.sub_y(),
    );

    let mut scenario_rng = SimRng::new(1);
    // A non-inviso shot takes no scatter draw, but it still reloads, and
    // `TechnoClass::GetROF @ 0x006FCFA0` draws `RandomRanged(0, 2)` for the
    // reload unconditionally. Exactly one draw, on the scenario instance.
    let mut expected_rng = scenario_rng.clone();
    expected_rng.next_range_u32_inclusive(0, 2);
    align_attackers_to_targets(&mut store);
    let result = tick(&mut store, &rules, &mut interner, 0, &mut scenario_rng);

    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert!(
        result.consequences.effects().explosion_effects.is_empty(),
        "non-Inviso Speed=0 still creates a persistent native shot"
    );
    assert_eq!(result.projectile_spawns.len(), 1);
    let target = result.projectile_spawns[0].initial_target_position;
    assert_eq!(
        (target.x, target.y),
        (
            i32::from(target_coord.0) * 256 + target_coord.2.to_num::<i32>(),
            i32::from(target_coord.1) * 256 + target_coord.3.to_num::<i32>()
        )
    );
}

/// Both shots' FireAt draws come first, in live order; then the tail visits
/// the bullets in the order FireAt appended them. The first bullet's
/// detonation removes it from the Logic vector, which shifts the second into
/// its slot, and the cursor moves past it (`0x0055B613`): the second bullet
/// detonates next frame.
// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn two_inviso_attackers_fire_in_live_order_and_the_second_bullet_waits_a_frame() {
    let mut rules = inviso_weapon_rules(true, true);
    initialize_fixture_anim_types(&mut rules);
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "SHOOTER", 6, 5, 300));
    store.insert(make_entity(3, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 3);
    install_entity_attack_target_for_test(&mut store, 2, 3);
    let target = store.get(3).unwrap();
    let target_coord = (
        target.position.rx(),
        target.position.ry(),
        target.position.sub_x(),
        target.position.sub_y(),
    );

    let mut scenario_rng = SimRng::new(1);
    let mut expected_rng = scenario_rng.clone();
    expected_rng.next_range_u32_inclusive(0, 2);
    expected_rng.next_range_u32_inclusive(0, 2);
    let expected = inviso_detonation_draws(&mut expected_rng, target_coord);
    align_attackers_to_targets(&mut store);
    let result = tick_with_map(
        &mut store,
        &rules,
        &mut interner,
        None,
        None,
        None,
        &[2, 1],
        &mut scenario_rng,
    );

    assert_eq!(
        result
            .consequences
            .fire_events()
            .iter()
            .map(|event| event.attacker_id)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert_eq!(result.fixture_anims.len(), 1);
    assert_eq!(constructed_anim_coord(&result.fixture_anims[0]), expected);
    assert_eq!(
        result.projectile_spawns.len(),
        1,
        "attacker 1's bullet was skipped"
    );
    assert_eq!(result.projectile_spawns[0].source_id, 1);
}

/// `inviso_weapon_rules(true, true)` with one special warhead key added.
fn inviso_special_rules(special_key: &str) -> RuleSet {
    shooter_rules("Inviso=yes\n", &format!("AnimList=PIFF\n{special_key}\n"))
}

/// `BulletClass::DetonateAtCoord @ 0x004690B0`: every special arm, its own
/// refusals included, leaves by `JMP 0x00469AA4`, and `Apply_area_damage`
/// (`0x00469A83`) is reachable only from the final else at `0x00469A3F`. An
/// Inviso shot is that same detonation, so a warhead selecting one of these
/// arms deals no damage and still runs the shared tail:
/// the Inviso re-scatter draw (`0x00469AD7`) and the `AnimList=` anim. Before,
/// the immediate path took ordinary area damage for these arms: the
/// Magnetron's `[MagneticBeam]` dealt 5000 to the vehicle it should lift.
// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn inviso_special_arms_claim_the_impact_and_keep_the_shared_tail() {
    // DirectRocker is the chain's one conditional arm; TARGET is a UnitClass.
    for special_key in [
        "ElectricAssault=yes",
        "IsLocomotor=yes",
        "Airstrike=yes",
        "DirectRocker=yes",
        "MakesDisguise=yes",
        "NukeMaker=yes",
    ] {
        let mut rules = inviso_special_rules(special_key);
        initialize_fixture_anim_types(&mut rules);
        let mut store = EntityStore::new();
        store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
        store.insert(make_entity(2, "TARGET", 8, 5, 500));
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 2);
        let target = store.get(2).unwrap();
        let target_coord = (
            target.position.rx(),
            target.position.ry(),
            target.position.sub_x(),
            target.position.sub_y(),
        );
        let mut scenario_rng = SimRng::new(1);
        let mut expected_rng = scenario_rng.clone();
        expected_rng.next_range_u32_inclusive(0, 2);
        let expected_effect = inviso_detonation_draws(&mut expected_rng, target_coord);
        align_attackers_to_targets(&mut store);
        let result = tick(&mut store, &rules, &mut interner, 0, &mut scenario_rng);

        assert_eq!(
            store.get(2).unwrap().health.current(),
            500,
            "{special_key}: the arm claims the impact, so no area damage"
        );
        assert_eq!(
            result.consequences.fire_events().len(),
            1,
            "{special_key}: the shot itself is fired"
        );
        assert_eq!(
            scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "{special_key}: the reload jitter, then the tail's scatter and cluster draws"
        );
        let anims = &result.fixture_anims;
        assert_eq!(anims.len(), 1, "{special_key}");
        assert_eq!(
            constructed_anim_coord(&anims[0]),
            expected_effect,
            "{special_key}: the shared tail places the AnimList anim"
        );
    }
}

/// The Giant Squid's `[SquidGrab]` is an Inviso Parasite shot. Its arm
/// (`0x004693D3`) claims the impact, so no ordinary area damage runs.
/// Whole stock attachment/grapple coverage lives in world::squid_grapple_tests.
#[test]
fn inviso_parasite_grapple_claims_the_impact() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=SQUID\n1=SHIP\n\
         [SQUID]\nStrength=300\nArmor=heavy\nSpeed=6\nNaval=yes\nOrganic=yes\nPrimary=GRAB\n\
         [SHIP]\nStrength=500\nArmor=heavy\nSpeed=6\nNaval=yes\n\
         [GRAB]\nDamage=40\nROF=99\nRange=2\nProjectile=TESTPROJ\nWarhead=TESTWH\n\
         [TESTPROJ]\nInviso=yes\n\
         [TESTWH]\nParasite=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SQUID", 5, 5, 300));
    store.insert(make_entity(2, "SHIP", 6, 5, 500));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    align_attackers_to_targets(&mut store);
    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    assert_eq!(result.consequences.fire_events().len(), 1);
    assert_eq!(store.get(2).unwrap().health.current(), 500);
}

/// The conditional DirectRocker arm (`0x00469796..0x004697B2`) claims the
/// impact only for a UnitClass target; at a cell it falls through to the
/// ordinary arm, so the ground shot still damages what stands there.
#[test]
fn inviso_direct_rocker_at_a_cell_keeps_ordinary_damage() {
    let rules = inviso_special_rules("DirectRocker=yes\nCellSpread=1");
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    store
        .get_mut(1)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Cell(8, 5)));
    align_attackers_to_targets(&mut store);
    tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    assert!(
        store.get(2).unwrap().health.current() < 500,
        "a cell target is never a UnitClass, so the ordinary arm runs"
    );
}

/// Retail data: every stock weapon whose warhead selects an unported special
/// arm is an Inviso shot, so all of them reach this path.
#[test]
fn retail_special_inviso_weapons_claim_their_impact() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    for (weapon_id, expected) in [
        ("AssaultBolt", SpecialDetonationAction::ElectricAssault),
        ("MagneticBeam", SpecialDetonationAction::Locomotor),
        ("MagneticBeamE", SpecialDetonationAction::Locomotor),
        ("Flare", SpecialDetonationAction::Airstrike),
        ("MakeupKit", SpecialDetonationAction::MakesDisguise),
    ] {
        let weapon = rules.weapon(weapon_id).unwrap();
        let warhead = rules.warhead(weapon.warhead.as_deref().unwrap()).unwrap();
        let action = projectile_special_detonation_action(
            SpecialDetonationFlags::of(warhead),
            SpecialDetonationTarget { is_unit: true },
        );
        assert_eq!(action, expected, "{weapon_id}");
        assert!(action.suppresses_ordinary_damage(), "{weapon_id}");
        assert!(
            classify_projectile_delivery(weapon, &rules).inviso,
            "{weapon_id} is an Inviso shot"
        );
    }
}

// SelectAnim behavior and RNG are compared against the original executable
// in detonation_anim::tests, including the native zero-damage no-animation case.

/// The shot leaves the BARREL, not the hull centre.
///
/// `TechnoClass::Fire_At` launches from `GetFLH @ 0x006F3AD0`, and the stock
/// MTNK fixture (`PrimaryFireFLH=190,25,120`, body north, turret east) puts that
/// muzzle at `+189, -25, +120` leptons from the object coordinate — 189, not
/// 190, because retail composes two table rotations whose residual truncates the
/// X term down a lepton.
#[test]
fn gsi_08_04_projectile_spawns_at_the_muzzle_not_the_hull_centre() {
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n1=HTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\nTurret=yes\n[HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n[105mm]\nDamage=65\nROF=50\nRange=6\nSpeed=40\nProjectile=Cannon\nWarhead=AP\n[Cannon]\nArcing=true\n[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ),

        &IniFile::from_str("[MTNK]\nPrimaryFireFLH=190,25,120\n"),
).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[MTNK]\nPrimaryFireFLH=190,25,120\n"),
    ));

    let mut store = EntityStore::new();
    let mut shooter = make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet");
    // Body facing north; the turret is aimed east at the target.
    shooter.body_facing.snap(0x0000, 0);
    shooter.barrel_facing = Some(crate::sim::movement::facing_class::FacingClass::new(
        0x4000, 0,
    ));
    store.insert(shooter);
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 2000, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(3));

    let spawn = result
        .projectile_spawns
        .first()
        .expect("the shot creates a tracked projectile");
    let hull_x = i32::from(store.get(1).unwrap().position.rx()) * 256
        + store.get(1).unwrap().position.sub_x().to_num::<i32>();
    let hull_y = i32::from(store.get(1).unwrap().position.ry()) * 256
        + store.get(1).unwrap().position.sub_y().to_num::<i32>();
    assert_eq!(
        (spawn.origin.x - hull_x, spawn.origin.y - hull_y),
        (189, -25),
        "muzzle offset in leptons"
    );
}

/// A `Dropping=` shell (BulletType `+0x29C`) is the exception: FireAt swaps
/// its launch source for the firer's GetCoords (`0x006FE2D8..0x006FE2FE`)
/// before the launch distance and the delta, so it leaves the hull centre.
/// Same fixture as `gsi_08_04`, whose `Arcing=` shell keeps the muzzle.
#[test]
fn gsi_08_04_a_dropping_shell_leaves_the_hull_centre() {
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n1=HTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\nTurret=yes\n[HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n[105mm]\nDamage=65\nROF=50\nRange=6\nSpeed=40\nProjectile=Bomb\nWarhead=AP\n[Bomb]\nDropping=yes\n[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ),

        &IniFile::from_str("[MTNK]\nPrimaryFireFLH=190,25,120\n"),
).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[MTNK]\nPrimaryFireFLH=190,25,120\n"),
    ));

    let mut store = EntityStore::new();
    let mut shooter = make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet");
    shooter.body_facing.snap(0x0000, 0);
    shooter.barrel_facing = Some(crate::sim::movement::facing_class::FacingClass::new(
        0x4000, 0,
    ));
    store.insert(shooter);
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 2000, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(3));

    let spawn = result
        .projectile_spawns
        .first()
        .expect("the shot creates a tracked projectile");
    let shooter = store.get(1).unwrap();
    let hull = (
        i32::from(shooter.position.rx()) * 256 + shooter.position.sub_x().to_num::<i32>(),
        i32::from(shooter.position.ry()) * 256 + shooter.position.sub_y().to_num::<i32>(),
    );
    assert_eq!((spawn.origin.x, spawn.origin.y), hull);
}

/// `TechnoClass::FireAt 0x006FEA36`..`0x006FEA4C`: a `ROT > 0` shot leaves the
/// tube at ONE lepton per frame and the weapon's `Speed=` is stored as
/// `Bullet+0x110` instead, which `BulletTypeClass::Acceleration` (`+0x2D0`)
/// then ramps toward. The launch direction is the aim facing, not the bearing
/// to the target.
#[test]
fn gsi_08_06_homing_launch_uses_one_lepton_and_stores_speed_as_the_ceiling() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=LNCHR\n1=HTNK\n\
         [LNCHR]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=Rocket\nTurret=yes\n\
         [HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=Rocket\n\
         [Rocket]\nDamage=65\nROF=50\nRange=6\nSpeed=30\nProjectile=Seeker\nWarhead=AP\n\
         [Seeker]\nROT=60\nArm=2\nRanged=yes\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("homing launch fixture parses");

    let mut store = EntityStore::new();
    let mut shooter = make_entity_owned(1, "LNCHR", 5, 5, 300, "Soviet");
    shooter.body_facing.snap(0x0000, 0);
    shooter.barrel_facing = Some(crate::sim::movement::facing_class::FacingClass::new(
        0x4000, 0,
    ));
    store.insert(shooter);
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 2000, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(3));

    let spawn = result
        .projectile_spawns
        .first()
        .expect("the missile is a tracked projectile");
    assert_eq!(
        spawn.speed_leptons_per_frame, 1,
        "every ROT > 0 launch starts at one lepton per frame"
    );
    let guidance = spawn.guidance.expect("a ROT > 0 shot carries guidance");
    // Original ReadSpeed528A90 converts authored30 to76; the saved
    // weapon_speed.json control establishes the native retained DWORD.
    assert_eq!(
        guidance.max_speed, 76,
        "effective weapon Speed is the ceiling"
    );
    assert_eq!(guidance.acceleration, 3, "BulletTypeClass ctor default");
    // This fixture checks the shot's facing, not an idealized unit vector:
    // native Bullet::Fire renormalizes using its approximate square root.
    // Full launch bits are compared with execution in projectile::launch.
    assert_eq!(
        spawn.velocity.integer_projection(),
        ProjectileCoord::new(1, 0, 0)
    );
}

/// A launch with no ballistic solution skips the rest of the shot. Under
/// `Gravity=200` an `Arcing=` shell one cell out has none: the speed FireAt
/// clamps to half the distance (128) is far short of an arc. The bullet is
/// deleted (`0x006FF000` -> `0x006FF93C`) and FireAt resumes at `0x006FF749`,
/// past the burst step, GetROF, the rearm and the `+0x120` store, so the tank
/// may try again next frame. The same shell with `Arcing=no` launches.
#[test]
fn gsi_08_06_a_failed_arc_launch_skips_the_rest_of_the_shot() {
    let shoot = |arcing: &str| {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nFixtureOnly=1\n[AudioVisual]\nGravity=200\n\
             [VehicleTypes]\n0=ARTY\n1=HTNK\n\
             [ARTY]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=Shell\n\
             [HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\n\
             [Shell]\nDamage=65\nROF=50\nBurst=2\nRange=6\nSpeed=200\nProjectile=Lob\nWarhead=AP\n\
             [Lob]\nArcing={arcing}\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        )))
        .expect("failed launch fixture parses");
        let mut store = EntityStore::new();
        let mut shooter = make_entity_owned(1, "ARTY", 5, 5, 300, "Soviet");
        shooter.body_facing.snap(0x4000, 0);
        store.insert(shooter);
        let _ = test_intern("HTNK");
        store.insert(make_entity_owned(2, "HTNK", 6, 5, 2000, "Americans"));
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 2);
        let before = store.get(1).unwrap().clone();
        let mut rng = SimRng::new(3);
        let rng_before = rng.logical_state();
        let result = tick(&mut store, &rules, &mut interner, 0, &mut rng);
        let shooter = store.get(1).unwrap();
        (
            result.projectile_spawns.len(),
            result.consequences.fire_events().len(),
            rng.logical_state() != rng_before,
            shooter.seldom.rearm_timer.timer() != before.seldom.rearm_timer.timer(),
            shooter.seldom.weapon_burst.index() != before.seldom.weapon_burst.index(),
            shooter.last_fire_frame != before.last_fire_frame,
        )
    };
    assert_eq!(
        shoot("no"),
        (1, 1, true, true, true, true),
        "the control launches"
    );
    assert_eq!(
        shoot("yes"),
        (0, 0, false, false, false, false),
        "no bullet, no report or anim, no GetROF draw, no rearm, no burst step, no \
         last-fire store"
    );
}

/// `TechnoClass::FireAt 0x006FE9FE`: EVERY launch speed is clamped to half the
/// straight-line distance to the target before the homing/vertical override.
/// For a `ROT=0` shell the speed clamped is `WeaponTypeClass::GetSpeed`'s
/// (`ftol(Sqrt_Approx(d * Gravity * 1.2))`, `0x00773070`), not `Speed=`;
/// under `[AudioVisual] Gravity=200` one cell out that is 247, above the
/// clamp's 128. The shell is straight (not `Arcing=`), so no arc solution is
/// needed at that gravity.
#[test]
fn gsi_08_06_point_blank_shot_clamps_the_launch_speed_to_half_the_distance() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nFixtureOnly=1\n[AudioVisual]\nGravity=200\n\
         [VehicleTypes]\n0=ARTY\n1=HTNK\n\
         [ARTY]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=Shell\n\
         [HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=Shell\n\
         [Shell]\nDamage=65\nROF=50\nRange=6\nSpeed=200\nProjectile=Lob\nWarhead=AP\n\
         [Lob]\nArcing=no\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("clamp fixture parses");

    let mut store = EntityStore::new();
    let mut shooter = make_entity_owned(1, "ARTY", 5, 5, 300, "Soviet");
    // Turretless: the hull facing is what `GetFireError` gates on, so aim it
    // east at the target before the shot.
    shooter.body_facing.snap(0x4000, 0);
    store.insert(shooter);
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 6, 5, 2000, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(3));

    let spawn = result
        .projectile_spawns
        .first()
        .expect("the shell is a tracked projectile");
    let dx = spawn.initial_target_position.x - spawn.origin.x;
    let dy = spawn.initial_target_position.y - spawn.origin.y;
    let dz = spawn.initial_target_position.z - spawn.origin.z;
    let distance = (f64::from(dx * dx + dy * dy + dz * dz)).sqrt().trunc() as i32;
    let get_speed = crate::sim::projectile::launch::weapon_launch_speed(
        200,
        Some(crate::sim::projectile::launch::LaunchSpeedProjectile {
            rot: 0,
            floater: false,
            gravity: None,
        }),
        200,
        crate::sim::projectile::launch::fireat_launch_distance(
            spawn.origin,
            spawn.initial_target_position,
            false,
        ),
    );
    assert!(
        distance / 2 < get_speed,
        "the fixture must be closer than twice GetSpeed's launch speed ({get_speed})"
    );
    assert_eq!(
        i32::from(spawn.speed_leptons_per_frame),
        distance / 2,
        "the launch speed is clamped to dist/2"
    );
}

/// GSI-08.08 end to end: the Kirov bomb has to FALL and explode.
///
/// `[BlimpBombP]` is `Vertical=yes` with `Acceleration=1`, no `ROT=` and no
/// `Ranged=`, so it carries no fuse and no steering — its only terminations are
/// the `Vertical` arm's own probes at `BulletClass::AI 0x00467334`ff.
/// (`DetonationAltitude`, then `GetAltitude() < 0`, then the bridge deck).
/// `TechnoClass::FireAt` gives a `Vertical` launch a pitch only when the
/// muzzle-to-target separation exceeds 200 leptons, so if the firer's hover
/// altitude failed to reach the launch coordinate the bomb would leave level,
/// never cross `DetonationAltitude=20000`, never drop below the floor, and
/// drift off the map unexploded. This pins the whole chain instead: the Jumpjet
/// firer's `JumpjetHeight=` becomes the locomotor's hover target, that hover
/// altitude reaches `object_world_z_leptons`, the launch pitch points down, and
/// the flight ends in a detonation.
///
/// The fixture is stock-faithful on the jumpjet keys: like retail `[ZEP]` it
/// omits `JumpJet=` and takes its Jumpjet locomotor from the GUID alone, and
/// `JumpjetHeight=750` still lands, because `TechnoTypeClass::ReadINI @
/// 0x00715151` stores it into `TechnoType+0xD80` unconditionally (over the
/// constructor default 500 at `0x007115D3`). A native Kirov really does hover
/// at 750.
#[test]
fn gsi_08_08_kirov_vertical_bomb_falls_and_detonates() {
    use crate::map::resolved_terrain::SharedCellDummy;
    use crate::sim::projectile::{ProjectileStore, ProjectileTrajectory};

    // Stock `[BlimpBombP]`, `[BlimpBomb]` and the Kirov's `JumpjetHeight=750`,
    // with one deliberate change: `Range=` is widened from the stock 1.5 so the
    // shot clears the fire-range gate while the firer hovers 750 leptons up
    // (nothing on the `Vertical` arm reads `Range=`). `JumpJet=` stays absent,
    // as in stock. See the doc comment.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=ZEP\n1=HTNK\n\
         [ZEP]\nStrength=2000\nArmor=medium\nSpeed=5\nPrimary=BlimpBomb\n\
         BalloonHover=yes\nJumpjetHeight=750\nConsideredAircraft=yes\n\
         MovementZone=Fly\nSpeedType=Hover\n\
         Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n\
         [HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\n\
         [BlimpBomb]\nDamage=250\nBurst=1\nROF=50\nRange=6\nSpeed=20\n\
         Projectile=BlimpBombP\nWarhead=BlimpHE\nOmniFire=yes\n\
         [BlimpBombP]\nArm=10\nAcceleration=1\nVertical=yes\nDetonationAltitude=20000\n\
         [BlimpHE]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("Kirov bomb fixture parses");

    let mut store = EntityStore::new();
    // Retail lists ZEP under `[VehicleTypes]`: the Kirov is a UnitClass.
    let mut kirov = make_entity_owned(1, "ZEP", 5, 5, 2000, "Soviet");
    let zep_object = rules.object("ZEP").expect("ZEP object type");
    let locomotor =
        crate::sim::movement::locomotor::LocomotorState::from_object_type(zep_object, 0);
    // The hover altitude is NOT hand-set: `JumpjetHeight=750` has to arrive
    // through the native Jumpjet Link_To_Object copy as the hover target, and
    // the airship is then placed at the top of its climb. Assert the rules hop
    // at its source so a broken parse fails here rather than downstream.
    assert_eq!(
        locomotor.jumpjet_runtime().unwrap().params().height(),
        750,
        "`JumpjetHeight=750` must reach the locomotor's hover target; a 500 here \
         means the rules->locomotor hop is broken, not the flight model"
    );
    kirov
        .position
        .set_exact_z_leptons_for_test(Some(locomotor.jumpjet_runtime().unwrap().params().height()));
    kirov.locomotor = Some(locomotor);
    store.insert(kirov);
    let _ = test_intern("HTNK");
    // Directly beneath the airship, the way a Kirov bombs.
    store.insert(make_entity_owned(2, "HTNK", 5, 5, 2000, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    // The order writes no facing, and this fixture runs no mission that turns
    // the airship: face it at the target before the fire gate asks.
    align_attackers_to_targets(&mut store);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(7));

    let spawn = *result
        .projectile_spawns
        .first()
        .expect("the Kirov bomb is a tracked projectile");
    assert!(
        matches!(spawn.trajectory, ProjectileTrajectory::Vertical { .. }),
        "Vertical=yes takes the third arm"
    );
    assert!(
        spawn.origin.z >= 750,
        "the Jumpjet hover altitude must reach the launch coordinate, else the \
         bomb leaves level and never terminates; got {}",
        spawn.origin.z
    );
    assert!(
        f64::from_bits(spawn.velocity.z.bits()) < 0.0,
        "the launch pitch has to point the bomb DOWN; got {:?}",
        spawn.velocity
    );

    let terrain = flat_level_zero_terrain(10, 10);
    let ground = ProjectileCoord::new(5 * 256, 5 * 256, 0);
    let mut projectiles = ProjectileStore::new();
    let id = projectiles.spawn(1, spawn);
    let dummy = SharedCellDummy::fresh();
    for frame in 1..600u32 {
        let step = projectiles
            .advance_one(
                id,
                frame,
                |_| Some(ground),
                Some(&terrain),
                &dummy,
                rules.general.gravity,
                false,
                false,
                rules.general.safety_altitude,
                |_, _, _| None,
                None,
            )
            .expect("the bomb is still in flight");
        assert!(
            step.expired.is_empty(),
            "the bomb must not vanish unexploded at frame {frame}"
        );
        if let Some(detonation) = step.detonations.first() {
            assert_eq!(detonation.projectile_id, id);
            // The descending bomb admits at the floor probe: 467350..467368.
            // The common admitted-impact clamp at 467BF0..467C06 then moves
            // its final coordinate to the floor before the damage handoff.
            assert_eq!(
                detonation.impact.z, 0,
                "the below-floor admission hands damage off at the level-0 floor"
            );
            return;
        }
    }
    panic!("the Kirov bomb never detonated");
}

/// GSI-08.08 — a special detonation arm suppresses shrapnel and area damage,
/// but NOT the shared tail.
///
/// `BulletClass::DetonateAtCoord @ 0x004690b0`: only the final else at
/// `0x00469a3f` runs `BulletClass::SpawnShrapnel @ 0x0046a310` and
/// `Apply_area_damage @ 0x00489280`, so any arm shadows both. Every arm then
/// leaves through `JMP LAB_00469AA4`, which is the explosion-anim /
/// scorch-crater / debris / `Airburst` tail — `Warhead::SelectExplosionAnim`
/// is called from inside it at `0x00469bcf`. Stock `[Controller]` carries
/// `AnimList=YURICNTL`, so a Yuri beam impact must still draw.
#[test]
fn gsi_08_08_special_arm_suppresses_damage_but_keeps_the_detonation_tail() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=TARGET\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TARGET]\nStrength=200\nArmor=heavy\n\
         [Warheads]\n0=Controller\n1=Plain\n[Animations]\n0=YURICNTL\n\
         [Controller]\nMindControl=yes\nAnimList=YURICNTL\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Plain]\nAnimList=YURICNTL\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("Controller/Plain warhead rules");
    initialize_fixture_anim_types(&mut rules);

    struct TailOutcome {
        damage_events: usize,
        anims: Vec<(String, u16, u16, u8)>,
    }

    fn run(rules: &RuleSet, warhead_name: &str) -> TailOutcome {
        let mut interner = test_interner();
        let mut entities = EntityStore::new();
        entities.insert(make_entity(10, "TARGET", 8, 5, 200));
        let warhead_ref = interner.intern(warhead_name);
        let detonation = ProjectileDetonation {
            projectile_id: 1,
            source_id: 77,
            target: ProjectileTarget::Entity(10),
            impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: payload_from_weapon(
                50,
                warhead_ref,
                interner.intern("MissingWeapon"),
                &rules,
                &mut interner,
            ),
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        let mut trace = FixtureTrace::default();
        let emitted = emit_detonations(
            &[detonation],
            &mut entities,
            rules,
            &mut interner,
            false,
            &mut SimRng::new(9),
            Some(&mut trace),
        );
        let anims: Vec<(String, u16, u16, u8)> = trace
            .constructed_anims
            .iter()
            .map(|effect| {
                (
                    interner.resolve(effect.type_id).to_string(),
                    (effect.world_coord.x / 256) as u16,
                    (effect.world_coord.y / 256) as u16,
                    (effect.world_coord.z / 104) as u8,
                )
            })
            .collect();
        TailOutcome {
            damage_events: emitted.damage_events.len(),
            anims,
        }
    }

    let mind_control = run(&rules, "Controller");
    assert_eq!(
        mind_control.damage_events, 0,
        "the MindControl arm at 0x00469211 shadows Apply_area_damage"
    );
    assert_eq!(
        mind_control.anims,
        vec![("YURICNTL".to_string(), 8, 5, 0)],
        "LAB_00469AA4 still selects and starts the AnimList explosion"
    );

    let plain = run(&rules, "Plain");
    assert_eq!(
        plain.damage_events, 1,
        "the ordinary arm at 0x00469a3f is unchanged"
    );
    assert_eq!(
        plain.anims, mind_control.anims,
        "the tail selects the same anim at the same place whichever arm reached it"
    );
}

/// `BulletClass::ResolveImpactCoordAndDetonate @ 0x00468D80`: a `Cluster=5`
/// shot detonates at its impact, then four times 256..512 leptons around that
/// impact (`0x0049F420` is handed the copy made at `0x00469008`), never around
/// the previous cluster.
#[test]
fn clusters_scatter_around_the_impact() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=LAUNCHER\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [LAUNCHER]\nStrength=100\nPrimary=ClusterGun\n\
         [ClusterGun]\nDamage=10\nProjectile=ClusterP\nWarhead=Blast\n\
         [ClusterP]\nCluster=5\n\
         [Warheads]\n0=Blast\n[Animations]\n0=EXPLOSML\n\
         [Blast]\nAnimList=EXPLOSML\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("cluster rules");
    initialize_fixture_anim_types(&mut rules);
    let mut interner = test_interner();
    let mut entities = EntityStore::new();
    let impact = ProjectileCoord::new(40 * 256 + 64, 50 * 256 + 200, 0);
    let detonation = ProjectileDetonation {
        projectile_id: 1,
        source_id: 77,
        target: ProjectileTarget::Cell { rx: 40, ry: 50 },
        impact,
        payload: payload_from_weapon(
            10,
            interner.intern("Blast"),
            interner.intern("ClusterGun"),
            &rules,
            &mut interner,
        ),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut trace = FixtureTrace::default();
    emit_detonations(
        &[detonation],
        &mut entities,
        &rules,
        &mut interner,
        false,
        &mut SimRng::new(11),
        Some(&mut trace),
    );
    let points: Vec<(i32, i32)> = trace
        .constructed_anims
        .iter()
        .map(|effect| (effect.world_coord.x, effect.world_coord.y))
        .collect();
    assert_eq!(points.len(), 5, "one anim per cluster");
    assert_eq!(points[0], (impact.x, impact.y), "the first at the impact");
    for &(x, y) in &points[1..] {
        let (dx, dy) = (f64::from(x - impact.x), f64::from(y - impact.y));
        let distance = dx.hypot(dy);
        assert!(
            (255.0..=513.0).contains(&distance),
            "each later cluster lies 256..512 leptons from the impact, got {distance}"
        );
    }
}

/// GSI-08.33 — `DirectRocker` is the chain's only conditional arm.
///
/// `BulletClass::DetonateAtCoord @ 0x0046978e` tests the flag, then
/// `Target != 0` (`0x004697a4`) and `Target->What_Am_I() == 1`
/// (`0x004697b2`, `UnitClass`). All three failures jump to `0x004699c4`, the
/// *next* test, so a `DirectRocker=yes` warhead that hits infantry still runs
/// ordinary damage. Dead in stock — `rulesmd.ini` carries no live
/// `DirectRocker=` line — but kept correct.
#[test]
fn gsi_08_33_direct_rocker_only_claims_a_vehicle_target() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=FOOT\n\
         [VehicleTypes]\n0=TARGET\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TARGET]\nStrength=200\nArmor=heavy\n\
         [FOOT]\nStrength=200\nArmor=none\n\
         [Warheads]\n0=Rocker\n[Animations]\n0=YURICNTL\n\
         [Rocker]\nDirectRocker=yes\nAnimList=YURICNTL\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("DirectRocker warhead rules");
    initialize_fixture_anim_types(&mut rules);
    assert!(
        rules.warhead("Rocker").expect("Rocker").direct_rocker,
        "DirectRocker= must reach the parsed field, not a dead raw byte"
    );

    fn run(rules: &RuleSet, category: EntityCategory, type_ref: &str) -> usize {
        let mut interner = test_interner();
        let mut entities = EntityStore::new();
        let mut target = make_entity(10, type_ref, 8, 5, 200);
        target.set_category_for_test(category);
        entities.insert(target);
        let detonation = ProjectileDetonation {
            projectile_id: 1,
            source_id: 77,
            target: ProjectileTarget::Entity(10),
            impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: payload_from_weapon(
                50,
                interner.intern("Rocker"),
                interner.intern("MissingWeapon"),
                &rules,
                &mut interner,
            ),
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        let mut trace = FixtureTrace::default();
        let emitted = emit_detonations(
            &[detonation],
            &mut entities,
            rules,
            &mut interner,
            false,
            &mut SimRng::new(9),
            Some(&mut trace),
        );
        assert_eq!(
            trace.constructed_anims.len(),
            1,
            "both branches reach LAB_00469AA4"
        );
        emitted.damage_events.len()
    }

    assert_eq!(
        run(&rules, EntityCategory::Unit, "TARGET"),
        0,
        "a vehicle target enters the arm and shadows Apply_area_damage"
    );
    assert_eq!(
        run(&rules, EntityCategory::Infantry, "FOOT"),
        1,
        "infantry falls through 0x004697b2 to the next test, so damage runs"
    );
}
