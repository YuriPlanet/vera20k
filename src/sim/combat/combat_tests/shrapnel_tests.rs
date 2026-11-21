//! Projectile shrapnel: admission, counts, aim and the shared dummy lookups.

use super::*;

#[test]
fn campaign_reverse_only_alliance_keeps_shrapnel_hostile_head_before_random_cell_child() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n[MTNK]\nStrength=100\nArmor=heavy\nPrimary=PARENT\nSecondary=CHILD\n\n[PARENT]\nDamage=20\nROF=10\nRange=6\nSpeed=30\nProjectile=PARENTPROJ\nWarhead=WH\n\n[PARENTPROJ]\nAirburst=yes\nShrapnelWeapon=CHILD\nShrapnelCount=2\n\n[CHILD]\nDamage=5\nROF=10\nRange=3\nSpeed=40\nProjectile=CHILDPROJ\nWarhead=WH\n\n[CHILDPROJ]\nSubjectToWalls=yes\n\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("shrapnel rules");
    let mut entities = EntityStore::new();
    let mut source = make_entity_owned(1, "MTNK", 5, 5, 100, "Soviet");
    source.lifecycle.set_cell_marked_for_test(true);
    entities.insert(source);
    // First ring-table entry is (+1,-1).
    let mut target = make_entity_owned(2, "MTNK", 6, 4, 100, "Americans");
    target.lifecycle.set_cell_marked_for_test(true);
    entities.insert(target);
    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut interner = test_interner();
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 7,
        source_id: 1,
        target: crate::sim::projectile::ProjectileTarget::Cell { rx: 5, ry: 5 },
        impact: crate::sim::projectile::ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
        payload: payload_from_weapon(
            20,
            interner.intern("WH"),
            interner.intern("PARENT"),
            &rules,
            &mut interner,
        ),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let mut scenario_rng = SimRng::new(0x46_a310);
    let mut expected_rng = scenario_rng.clone();
    let _ = expected_rng.next_range_u32_inclusive(0, 4);
    let _ = expected_rng.next_range_u32_inclusive(0, 4);

    align_attackers_to_targets(&mut entities);
    // Original46A4FB asks the firer's House;46A501 supplies the candidate
    // object to4F9A90. Its owner's reverse-only bit cannot suppress this
    // child or spend the extra random-cell draws of an excluded candidate.
    let alliances = HouseAllianceMap::from([(
        "AMERICANS".to_owned(),
        std::collections::BTreeSet::from(["SOVIET".to_owned()]),
    )]);
    let result = tick_detonations(
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut crate::sim::houses::Houses::default(),
        &alliances,
        1,
        &[1, 2],
        &[detonation],
        &mut scenario_rng,
        &mut SimRng::new(1),
    );

    assert_eq!(result.projectile_spawns.len(), 2);
    assert_eq!(
        result.projectile_spawns[0].target,
        crate::sim::projectile::ProjectileTarget::Entity(2)
    );
    assert!(matches!(
        result.projectile_spawns[1].target,
        crate::sim::projectile::ProjectileTarget::Cell { .. }
    ));
    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
}

/// SpawnShrapnel lessens a negative ShrapnelCount by the cells from the
/// bullet to its firer's GetCoords (`[bullet+0xB0]`, vt+0x48 at
/// `0x0046A370`): for a building firer its foundation centre (`0x00447AC0`),
/// not its north-west cell, and never the target.
#[test]
fn projectile_shrapnel_count_measures_to_the_firers_get_coords() {
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n[BuildingTypes]\n0=HQ\n\n[HQ]\nStrength=100\n\n[MTNK]\nStrength=100\nArmor=heavy\nPrimary=PARENT\nSecondary=CHILD\n\n[PARENT]\nDamage=20\nROF=10\nRange=6\nSpeed=30\nProjectile=PARENTPROJ\nWarhead=WH\n\n[PARENTPROJ]\nAirburst=yes\nShrapnelWeapon=CHILD\nShrapnelCount=-8\n\n[CHILD]\nDamage=5\nROF=10\nRange=3\nSpeed=40\nProjectile=CHILDPROJ\nWarhead=WH\n\n[CHILDPROJ]\nSubjectToWalls=yes\n\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let art = IniFile::from_str("[HQ]\nFoundation=3x3\n");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("shrapnel rules");
    let mut entities = EntityStore::new();
    // The firer's GetCoords (the 3x3 foundation centre, `0x00447AC0`) is
    // 1280 leptons from the impact, 5 cells; its NW cell's centre is 1557, 6.
    let mut source = make_entity_owned(1, "HQ", 2, 4, 100, "Soviet");
    source.set_category_for_test(EntityCategory::Structure);
    // Construction stamps the type's `Foundation=3x3`.
    source
        .seldom
        .type_facts
        .set_foundation_for_test("3x3".to_string().into());
    source.lifecycle.set_cell_marked_for_test(true);
    entities.insert(source);
    // The target stands in the bullet's cell, which SpawnShrapnel requires.
    let mut target = make_entity_owned(2, "MTNK", 8, 5, 100, "Americans");
    target.lifecycle.set_cell_marked_for_test(true);
    entities.insert(target);
    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut interner = test_interner();
    let impact = crate::sim::projectile::ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0);
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 7,
        source_id: 1,
        target: crate::sim::projectile::ProjectileTarget::Entity(2),
        impact,
        payload: payload_from_weapon(
            20,
            interner.intern("WH"),
            interner.intern("PARENT"),
            &rules,
            &mut interner,
        ),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    align_attackers_to_targets(&mut entities);
    let result = tick_detonations(
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut crate::sim::houses::Houses::default(),
        &HouseAllianceMap::new(),
        1,
        &[1, 2],
        &[detonation],
        &mut SimRng::new(0x46_a370),
        &mut SimRng::new(1),
    );

    // 8 - 5; the firer's NW cell would give 8 - 6 and the target 8 - 0.
    assert_eq!(result.projectile_spawns.len(), 3);
}

/// SpawnShrapnel's object branch aims at the hostile object's GetCoords
/// (vt+0x48 at `0x0046A614`): for a building its foundation center
/// (`0x00447AC0`), not its north-west cell.
#[test]
fn projectile_shrapnel_aims_at_a_building_foundation_center() {
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n[BuildingTypes]\n0=HQ\n\n[HQ]\nStrength=100\n\n[MTNK]\nStrength=100\nArmor=heavy\nPrimary=PARENT\nSecondary=CHILD\n\n[PARENT]\nDamage=20\nROF=10\nRange=6\nSpeed=30\nProjectile=PARENTPROJ\nWarhead=WH\n\n[PARENTPROJ]\nAirburst=yes\nShrapnelWeapon=CHILD\nShrapnelCount=1\n\n[CHILD]\nDamage=5\nROF=10\nRange=3\nSpeed=40\nProjectile=CHILDPROJ\nWarhead=WH\n\n[CHILDPROJ]\nSubjectToWalls=yes\n\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let art = IniFile::from_str("[HQ]\nFoundation=2x2\n");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("shrapnel rules");
    let mut entities = EntityStore::new();
    let mut source = make_entity_owned(1, "MTNK", 5, 5, 100, "Soviet");
    source.lifecycle.set_cell_marked_for_test(true);
    entities.insert(source);
    // First ring-table entry is (+1,-1): the HQ's north-west cell.
    let mut target = make_entity_owned(2, "HQ", 6, 4, 100, "Americans");
    target.set_category_for_test(EntityCategory::Structure);
    // Construction stamps the type's `Foundation=2x2`.
    target
        .seldom
        .type_facts
        .set_foundation_for_test("2x2".to_string().into());
    target.lifecycle.set_cell_marked_for_test(true);
    entities.insert(target);
    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut interner = test_interner();
    let impact = crate::sim::projectile::ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0);
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 7,
        source_id: 1,
        target: crate::sim::projectile::ProjectileTarget::Cell { rx: 5, ry: 5 },
        impact,
        payload: payload_from_weapon(
            20,
            interner.intern("WH"),
            interner.intern("PARENT"),
            &rules,
            &mut interner,
        ),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    align_attackers_to_targets(&mut entities);
    let result = tick_detonations(
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut crate::sim::houses::Houses::default(),
        &HouseAllianceMap::new(),
        1,
        &[1, 2],
        &[detonation],
        &mut SimRng::new(0x46_a310),
        &mut SimRng::new(1),
    );

    assert_eq!(result.projectile_spawns.len(), 1);
    let child = &result.projectile_spawns[0];
    assert_eq!(
        child.target,
        crate::sim::projectile::ProjectileTarget::Entity(2)
    );
    // 0x00447AC0: Location + ((w - 1) * 128, (h - 1) * 128) for a 2x2.
    let hq = entities.get(2).unwrap();
    let center = crate::sim::projectile::ProjectileCoord::new(
        i32::from(hq.position.rx()) * 256 + hq.position.sub_x().to_num::<i32>() + 128,
        i32::from(hq.position.ry()) * 256 + hq.position.sub_y().to_num::<i32>() + 128,
        0,
    );
    assert_eq!(child.initial_target_position, center);
    assert_eq!(
        child.velocity,
        crate::sim::projectile::launch::shrapnel_launch_velocity(
            impact,
            center,
            rules.weapon("CHILD").unwrap().speed,
            false
        )
    );
}

/// SpawnShrapnel rules: `ShrapnelCount=count` fragments of a child whose
/// Range walks three rings.
fn shrapnel_admission_rules(count: i32) -> RuleSet {
    shrapnel_rules(count, "SubjectToWalls=yes")
}

/// [`shrapnel_admission_rules`] with the child's projectile body `child`.
fn shrapnel_rules(count: i32, child: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MTNK\n\n[BuildingTypes]\n0=HQ\n\n[HQ]\nStrength=100\n\n\
         [MTNK]\nStrength=100\nArmor=heavy\nPrimary=PARENT\nSecondary=CHILD\n\n\
         [PARENT]\nDamage=20\nROF=10\nRange=6\nSpeed=30\nProjectile=PARENTPROJ\nWarhead=WH\n\n\
         [PARENTPROJ]\nAirburst=yes\nShrapnelWeapon=CHILD\nShrapnelCount={count}\n\n\
         [CHILD]\nDamage=5\nROF=10\nRange=3\nSpeed=40\nProjectile=CHILDPROJ\nWarhead=WH\n\n\
         [CHILDPROJ]\n{child}\n\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("shrapnel rules")
}

/// A world for SpawnShrapnel commits, `entities` marked into the cell lists.
fn shrapnel_world(rules: &RuleSet, entities: Vec<GameEntity>) -> Simulation {
    let mut store = EntityStore::new();
    let mut next_id = 1;
    for mut entity in entities {
        entity.lifecycle.set_cell_marked_for_test(true);
        next_id = next_id.max(entity.stable_id() + 1);
        store.insert(entity);
    }
    let mut sim = Simulation::with_seed(1);
    sim.interner = test_interner();
    sim.substrate.occupancy = OccupancyGrid::rebuild(&store);
    sim.substrate.entities = store;
    sim.set_next_stable_object_id_for_test(next_id);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
    sim
}

/// Entity 1's PARENT bullet `projectile_id` detonating at `impact`,
/// committed through the receiver: the fragments it spawned.
fn commit_shrapnel(
    sim: &mut Simulation,
    rules: &RuleSet,
    projectile_id: u64,
    target: crate::sim::projectile::ProjectileTarget,
    impact: crate::sim::projectile::ProjectileCoord,
) -> Vec<crate::sim::projectile::Projectile> {
    let payload = payload_from_weapon(
        20,
        sim.interner.intern("WH"),
        sim.interner.intern("PARENT"),
        &rules,
        &mut sim.interner,
    );
    let before: BTreeSet<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    world_receiver::commit_projectiles(
        sim,
        &mut world_receiver::ReceiverRun::default(),
        &[crate::sim::projectile::ProjectileDetonation {
            projectile_id,
            source_id: 1,
            target,
            impact,
            payload,
            reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
        }],
        rules,
        None,
    );
    sim.projectiles
        .iter()
        .filter(|(id, _)| !before.contains(id))
        .map(|(_, bullet)| bullet.clone())
        .collect()
}

/// [`commit_shrapnel`] for a bullet with no live record (its Location is the
/// impact) at the centre of cell (5,5).
fn commit_shrapnel_at_5_5(
    sim: &mut Simulation,
    rules: &RuleSet,
) -> Vec<crate::sim::projectile::Projectile> {
    commit_shrapnel(
        sim,
        rules,
        99,
        crate::sim::projectile::ProjectileTarget::Cell { rx: 5, ry: 5 },
        crate::sim::projectile::ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
    )
}

/// One PARENT bullet of `source_id` detonating at the centre of cell (5,5)
/// among `entities`, with no live bullet: its fragments and the Scenario
/// stream after them.
fn shrapnel_at_cell_5_5(
    rules: &RuleSet,
    mut entities: EntityStore,
    source_id: u64,
) -> (Vec<crate::sim::projectile::ProjectileSpawn>, SimRng) {
    let ids = entities.keys_sorted();
    for &id in &ids {
        entities
            .get_mut(id)
            .unwrap()
            .lifecycle
            .set_cell_marked_for_test(true);
    }
    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut interner = test_interner();
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 7,
        source_id,
        target: crate::sim::projectile::ProjectileTarget::Cell { rx: 5, ry: 5 },
        impact: crate::sim::projectile::ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
        payload: payload_from_weapon(
            20,
            interner.intern("WH"),
            interner.intern("PARENT"),
            &rules,
            &mut interner,
        ),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let mut scenario_rng = SimRng::new(0x46_a3d6);
    let result = tick_detonations(
        &mut entities,
        &mut occupancy,
        rules,
        &mut interner,
        &mut crate::sim::houses::Houses::default(),
        &HouseAllianceMap::new(),
        1,
        &ids,
        &[detonation],
        &mut scenario_rng,
        &mut SimRng::new(1),
    );
    (result.projectile_spawns, scenario_rng)
}

/// SpawnShrapnel throws fragments only when the bullet's cell holds a first
/// ground object that is not a building (`0x0046A3D6..0x0046A406`): none on
/// bare ground or on a building.
#[test]
fn projectile_shrapnel_needs_a_non_building_first_object_in_the_bullets_cell() {
    let rules = shrapnel_admission_rules(1);
    let firer = || make_entity_owned(1, "MTNK", 2, 2, 100, "Soviet");

    let mut bare = EntityStore::new();
    bare.insert(firer());
    assert!(shrapnel_at_cell_5_5(&rules, bare, 1).0.is_empty());

    let mut building = EntityStore::new();
    building.insert(firer());
    let mut hq = make_entity_owned(2, "HQ", 5, 5, 100, "Americans");
    hq.set_category_for_test(EntityCategory::Structure);
    hq.seldom
        .type_facts
        .set_foundation_for_test("1x1".to_string().into());
    building.insert(hq);
    assert!(shrapnel_at_cell_5_5(&rules, building, 1).0.is_empty());

    let mut vehicle = EntityStore::new();
    vehicle.insert(firer());
    vehicle.insert(make_entity_owned(2, "MTNK", 5, 5, 100, "Americans"));
    assert_eq!(shrapnel_at_cell_5_5(&rules, vehicle, 1).0.len(), 1);
}

/// With its firer gone, SpawnShrapnel aims at no ring object
/// (`0x0046A4E5..0x0046A4F5`): every fragment takes a random cell, two
/// Scenario draws each, though a hostile stands in the first ring cell.
#[test]
fn projectile_shrapnel_without_a_firer_throws_only_at_random_cells() {
    use crate::sim::projectile::{ProjectileTarget, projectile_random_shrapnel_cell};
    let rules = shrapnel_admission_rules(2);
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(2, "MTNK", 5, 5, 100, "Americans"));
    // The first ring-table entry is (+1,-1).
    entities.insert(make_entity_owned(3, "MTNK", 6, 4, 100, "Americans"));
    let (fragments, rng) = shrapnel_at_cell_5_5(&rules, entities, 1);

    let mut expected = SimRng::new(0x46_a3d6);
    let cells = [
        projectile_random_shrapnel_cell(5, 5, &mut expected),
        projectile_random_shrapnel_cell(5, 5, &mut expected),
    ];
    assert_eq!(
        fragments
            .iter()
            .map(|fragment| fragment.target)
            .collect::<Vec<_>>(),
        cells.map(|(rx, ry)| ProjectileTarget::Cell {
            rx: rx as u16,
            ry: ry as u16,
        })
    );
    assert_eq!(rng.logical_state(), expected.logical_state());
}

/// SpawnShrapnel reads the bullet's Location (`+0x9C`), not the coordinate
/// the impact ladder handed DetonateAtCoord (`0x00468D80` never moves the
/// bullet): the Location's cell is the one admitted and the ring centre, and
/// the Location is every fragment's origin.
#[test]
fn projectile_shrapnel_reads_the_bullets_location_not_its_impact() {
    use crate::sim::projectile::{
        ProjectileCollisionPolicy, ProjectileCoord, ProjectileSpawn, ProjectileTarget,
        ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
        projectile_random_shrapnel_cell,
    };
    let rules = shrapnel_admission_rules(1);
    let mut sim = shrapnel_world(
        &rules,
        vec![
            make_entity_owned(1, "MTNK", 2, 2, 100, "Soviet"),
            make_entity_owned(2, "MTNK", 5, 5, 100, "Americans"),
        ],
    );
    let payload = payload_from_weapon(
        20,
        sim.interner.intern("WH"),
        sim.interner.intern("PARENT"),
        &rules,
        &mut sim.interner,
    );
    // Supply an already-admitted point above ground. BulletType46C4F0 raises
    // a ground-level launch; its separate native placement comparison covers
    // that prefix, while this fixture isolates the shrapnel Location read.
    let location = ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 1);
    let target = ProjectileTarget::Cell { rx: 8, ry: 5 };
    let id = sim.substrate.allocate_stable_id();
    sim.admit_projectile(
        id,
        ProjectileSpawn {
            native_unique_id: 0x100,
            line_trail: None,
            flat: false,
            source_id: 1,
            origin: location,
            target,
            initial_target_position: location,
            payload,
            speed_leptons_per_frame: 1,
            velocity: ProjectileVelocity::new(0, 0, 0),
            trajectory: ProjectileTrajectory::Straight,
            guidance: None,
            visual: ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
            collision: ProjectileCollisionPolicy::NONE,
        },
    );
    assert_eq!(sim.projectiles.get(id).unwrap().position(), location);
    let mut expected = sim.scenario_rng.clone();
    let cell = projectile_random_shrapnel_cell(5, 5, &mut expected);

    // The detonation coordinate is three cells east, over bare ground.
    let fragments = commit_shrapnel(
        &mut sim,
        &rules,
        id,
        target,
        ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
    );

    assert_eq!(fragments.len(), 1);
    let fragment = &fragments[0];
    assert_eq!(fragment.launch_origin(), location);
    assert_eq!(
        fragment.target(),
        ProjectileTarget::Cell {
            rx: cell.0 as u16,
            ry: cell.1 as u16,
        }
    );
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

/// Ares `BulletClass_Shrapnel_WeaponType1/2` (1001ABF0, hooks 0x46A5B2 and
/// 0x46AA27) give each fragment its shrapnel weapon (+0x130), which the
/// child's detonation (RadLevel, IvanBomb) reads; without Ares it has none.
#[test]
fn projectile_shrapnel_fragment_holds_its_weapon_only_in_ares() {
    let rules = shrapnel_admission_rules(1);
    let mut sim = shrapnel_world(
        &rules,
        vec![make_entity_owned(1, "MTNK", 2, 2, 100, "Soviet")],
    );
    let tree_type = sim.interner.intern("TREE01");
    sim.terrain_objects.insert_constructed(
        crate::sim::terrain_object::TerrainObjectState::for_test(900, tree_type, 5, 5),
        false,
    );
    sim.terrain_objects.add_to_cell(900, (5, 5));
    let fragments = commit_shrapnel_at_5_5(&mut sim, &rules);
    let child = sim.interner.intern("CHILD");
    assert_eq!(
        fragments
            .iter()
            .map(|fragment| fragment.payload().installed_weapon())
            .collect::<Vec<_>>(),
        vec![rules.ares().then_some(child)]
    );
}

/// The bullet cell's first ground object may be a tree, which is no
/// building: shrapnel is thrown (`0x0046A3F8..0x0046A406`).
#[test]
fn projectile_shrapnel_is_admitted_by_a_tree_heading_the_bullets_cell() {
    let rules = shrapnel_admission_rules(1);
    let mut sim = shrapnel_world(
        &rules,
        vec![make_entity_owned(1, "MTNK", 2, 2, 100, "Soviet")],
    );
    assert!(commit_shrapnel_at_5_5(&mut sim, &rules).is_empty());
    // The cell walker follows represented Terrain instances, not dangling
    // head IDs. This fixture isolates admission rather than map construction.
    let tree_type = sim.interner.intern("TREE01");
    sim.terrain_objects.insert_constructed(
        crate::sim::terrain_object::TerrainObjectState::for_test(900, tree_type, 5, 5),
        false,
    );
    sim.terrain_objects.add_to_cell(900, (5, 5));
    assert_eq!(commit_shrapnel_at_5_5(&mut sim, &rules).len(), 1);
}

/// A unit on a bridge deck stands on its cell's other list (`+0xE8`); the
/// gate reads only the ground list (`+0xE4`), so it admits nothing.
#[test]
fn projectile_shrapnel_ignores_a_unit_on_the_bridge_deck() {
    let rules = shrapnel_admission_rules(1);
    let mut deck = make_entity_owned(2, "MTNK", 5, 5, 100, "Americans");
    deck.set_on_bridge(true);
    let mut sim = shrapnel_world(
        &rules,
        vec![make_entity_owned(1, "MTNK", 2, 2, 100, "Soviet"), deck],
    );
    assert!(commit_shrapnel_at_5_5(&mut sim, &rules).is_empty());
}

/// A bullet whose cell has no native CellClass gets the shared dummy from
/// GetCell (`0x005F6960`), which holds no object: no shrapnel, though the
/// cell lists VERA keeps by coordinate hold a vehicle there.
#[test]
fn projectile_shrapnel_is_refused_in_a_missing_cell() {
    let rules = shrapnel_admission_rules(1);
    let mut sim = shrapnel_world(
        &rules,
        vec![
            make_entity_owned(1, "MTNK", 2, 2, 100, "Soviet"),
            make_entity_owned(2, "MTNK", 5, 5, 100, "Americans"),
        ],
    );
    let mut terrain = super::impact_height_tests::terrain_at_level(0);
    terrain.test_set_native_allocated_cells(&[(5, 5)]);
    sim.resolved_terrain = Some(terrain.clone());
    assert_eq!(commit_shrapnel_at_5_5(&mut sim, &rules).len(), 1);
    terrain.test_set_native_allocated_cells(&[(2, 2)]);
    sim.resolved_terrain = Some(terrain);
    assert!(commit_shrapnel_at_5_5(&mut sim, &rules).is_empty());
}

/// `ShrapnelCount=0` (the key's default) throws a fragment at every hostile
/// ring head and none at a random cell: the walk tests its count after each
/// fragment (`0x0046A8D9..0x0046A8E0`), and the random cells only below it
/// (`0x0046A92C..0x0046A936`).
#[test]
fn projectile_shrapnel_count_zero_takes_every_hostile_ring_head() {
    use crate::sim::projectile::ProjectileTarget;
    let rules = shrapnel_admission_rules(0);
    let mut sim = shrapnel_world(
        &rules,
        vec![
            make_entity_owned(1, "MTNK", 2, 2, 100, "Soviet"),
            make_entity_owned(2, "MTNK", 5, 5, 100, "Americans"),
            make_entity_owned(3, "MTNK", 6, 4, 100, "Americans"),
            make_entity_owned(4, "MTNK", 8, 5, 100, "Americans"),
            // The firer's own house: no target.
            make_entity_owned(5, "MTNK", 4, 6, 100, "Soviet"),
        ],
    );
    let before = sim.scenario_rng.logical_state();
    let mut targets = commit_shrapnel_at_5_5(&mut sim, &rules)
        .into_iter()
        .map(|fragment| fragment.target())
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| match target {
        ProjectileTarget::Entity(id) => *id,
        _ => u64::MAX,
    });
    assert_eq!(
        targets,
        [ProjectileTarget::Entity(3), ProjectileTarget::Entity(4)]
    );
    assert_eq!(sim.scenario_rng.logical_state(), before);
}

/// SpawnShrapnel Fires each fragment (vt+0x1F0, `0x0046A875`): an `Inviso=`
/// one stands on its target's vt+0x58 at once, at speed 0
/// (`0x004688B7..0x00468986`), instead of flying there from the bullet.
#[test]
fn an_inviso_fragment_is_admitted_standing_on_its_target() {
    use crate::sim::projectile::{ProjectileCoord, ProjectileTarget};
    let rules = shrapnel_rules(1, "Inviso=yes");
    let mut sim = shrapnel_world(
        &rules,
        vec![
            make_entity_owned(1, "MTNK", 2, 2, 100, "Soviet"),
            make_entity_owned(2, "MTNK", 5, 5, 100, "Americans"),
            make_entity_owned(3, "MTNK", 6, 4, 100, "Americans"),
        ],
    );
    let fragments = commit_shrapnel_at_5_5(&mut sim, &rules);
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].target(), ProjectileTarget::Entity(3));
    let aim = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(3).unwrap(),
        None,
    );

    let fragment = sim
        .projectiles
        .get(fragments[0].id())
        .expect("the admitted fragment");
    assert_eq!(
        fragment.position(),
        ProjectileCoord::new(aim.x, aim.y, aim.z)
    );
    assert_eq!(fragment.speed_leptons_per_frame(), 0);
}

#[test]
fn gsi_04_01_projectile_shrapnel_captures_each_shared_dummy_lookup() {
    use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeStampSlot};
    use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};
    use crate::sim::projectile::{
        ProjectileCoord, ProjectileTarget, dummy_cell_target_coord, projectile_random_shrapnel_cell,
    };
    use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, ground_height_leptons};

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n[MTNK]\nStrength=100\nArmor=heavy\nPrimary=PARENT\nSecondary=CHILD\n\n[PARENT]\nDamage=20\nROF=10\nRange=6\nSpeed=30\nProjectile=PARENTPROJ\nWarhead=WH\n\n[PARENTPROJ]\nAirburst=yes\nShrapnelWeapon=CHILD\nShrapnelCount=3\n\n[CHILD]\nDamage=5\nROF=10\nRange=3\nSpeed=40\nProjectile=CHILDPROJ\nWarhead=WH\n\n[CHILDPROJ]\nSubjectToWalls=yes\n\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("shrapnel rules");
    let mut entities = EntityStore::new();
    let mut source = make_entity_owned(1, "MTNK", 5, 5, 100, "Soviet");
    source.lifecycle.set_cell_marked_for_test(true);
    entities.insert(source);
    let occupancy = OccupancyGrid::rebuild(&entities);
    let mut interner = test_interner();
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 7,
        source_id: 1,
        target: ProjectileTarget::Cell { rx: 5, ry: 5 },
        impact: ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
        payload: payload_from_weapon(
            20,
            interner.intern("WH"),
            interner.intern("PARENT"),
            &rules,
            &mut interner,
        ),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };

    let scenario_rng = SimRng::new(0x46_a310);
    let mut expected_rng = scenario_rng.clone();
    let expected_cells = [
        projectile_random_shrapnel_cell(5, 5, &mut expected_rng),
        projectile_random_shrapnel_cell(5, 5, &mut expected_rng),
        projectile_random_shrapnel_cell(5, 5, &mut expected_rng),
    ];
    assert_ne!(expected_cells[0], expected_cells[1]);
    assert_ne!(expected_cells[1], expected_cells[2]);

    // The declared map has storage for every request, but only the first
    // random coordinate has a native CellClass pointer. The next two lookups
    // both return and restamp one shared dummy identity.
    let mut terrain = super::impact_height_tests::terrain_at_level(2);
    // The bullet's own cell exists too: its first ground object, the firer,
    // admits the shrapnel.
    terrain.test_set_native_allocated_cells(&[
        (5, 5),
        (expected_cells[0].0 as u16, expected_cells[0].1 as u16),
    ]);
    terrain.test_set_dummy_cell_level_slope(2, 0);
    let dummy = terrain.shared_cell_dummy();
    dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);

    let expected_target = |(rx, ry): (i32, i32), structural: bool| {
        let x = rx * 256 + 128;
        let y = ry * 256 + 128;
        let z = ground_height_leptons(2, 0, x, y).expect("flat CellClass surface is supported")
            + if structural {
                BRIDGE_DECK_HEIGHT_LEPTONS
            } else {
                0
            };
        ProjectileCoord::new(x, y, z)
    };
    let expected_positions = [
        expected_target(expected_cells[0], false),
        expected_target(expected_cells[1], true),
        expected_target(expected_cells[2], true),
    ];
    let mut sim = Simulation::with_seed(0x46_a310);
    sim.substrate.entities = entities;
    sim.substrate.occupancy = occupancy;
    sim.set_next_stable_object_id_for_test(2);
    sim.interner = interner;
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    emit_projectile_shrapnel(&mut sim, &rules, &detonation, detonation.impact);
    let children: Vec<_> = sim
        .projectiles
        .iter()
        .map(|(_, bullet)| bullet.clone())
        .collect();

    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    assert_eq!(children.len(), 3);
    assert_eq!(
        children[0].target(),
        ProjectileTarget::Cell {
            rx: expected_cells[0].0 as u16,
            ry: expected_cells[0].1 as u16,
        }
    );
    assert_eq!(children[1].target(), ProjectileTarget::DummyCell);
    assert_eq!(children[2].target(), ProjectileTarget::DummyCell);
    for (index, child) in children.iter().enumerate() {
        assert_eq!(child.launch_target(), expected_positions[index]);
        assert_eq!(
            child.velocity(),
            crate::sim::projectile::launch::shrapnel_launch_velocity(
                detonation.impact,
                expected_positions[index],
                rules.weapon("CHILD").unwrap().speed,
                true,
            )
        );
    }
    assert_ne!(children[1].launch_target(), children[2].launch_target());

    let final_snapshot = dummy.snapshot();
    assert_eq!(final_snapshot.coord, expected_cells[2]);
    assert_ne!(
        final_snapshot.bridge_flags_0x1180 & BRIDGE_FLAG_STRUCTURAL,
        0
    );
    assert!(dummy.same_identity(&terrain.shared_cell_dummy()));

    // A later miss restamps the retained pointer for BulletClass::AI without
    // rewriting either child's constructor-time launch coordinate.
    let later = get_cellclass_fallback(Some(&terrain), 9, 10);
    let CellRef::Dummy { cell: later_dummy } = later else {
        panic!("native-unallocated later lookup must retain the shared dummy");
    };
    assert!(dummy.same_identity(&later_dummy));
    assert_eq!(dummy.snapshot().coord, (9, 10));
    let later_target = dummy_cell_target_coord(&later_dummy);
    assert_eq!(later_target.x, 9 * 256 + 128);
    assert_eq!(later_target.y, 10 * 256 + 128);
    assert_eq!(later_target.z, 2 * 104 + BRIDGE_DECK_HEIGHT_LEPTONS);
    assert_ne!(later_target, children[2].launch_target());
    assert_eq!(
        children
            .iter()
            .map(|spawn| spawn.launch_target())
            .collect::<Vec<_>>(),
        expected_positions
    );
}
