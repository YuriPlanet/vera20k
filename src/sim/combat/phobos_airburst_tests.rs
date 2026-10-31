//! Phobos airburst/splits production-reader and emitter regressions.

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::sim::projectile::ProjectileCoord;
use crate::util::native_x87::NativeF64Bits;

const VERSES_ZERO: &str = "Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%";
const VERSES_FULL: &str = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%";

fn rules_ini(
    parent: &str,
    child_weapon: &str,
    child_projectile: &str,
    warhead: &str,
    globals: &str,
    extra: &str,
) -> IniFile {
    rules_ini_full(
        parent,
        child_weapon,
        child_projectile,
        warhead,
        globals,
        extra,
        VERSES_FULL,
        "",
    )
}

#[allow(clippy::too_many_arguments)]
fn rules_ini_full(
    parent: &str,
    child_weapon: &str,
    child_projectile: &str,
    warhead: &str,
    globals: &str,
    extra: &str,
    child_verses: &str,
    victim_keys: &str,
) -> IniFile {
    let text = format!(
        "[CombatDamage]\n{globals}\n\
         [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
         [AircraftTypes]\n0=PLANE\n\
         [SOURCE]\nStrength=100\nPrimary=PARENT_GUN\nSecondary=CHILD_GUN\nSpeed=4\n\
         [VICTIM]\nStrength=1000\nArmor=none\nSpeed=4\n{victim_keys}\n\
         [PLANE]\nStrength=1000\nArmor=none\nSpeed=10\n\
         [PARENT_GUN]\nProjectile=PARENT_SHOT\nWarhead=QUIET\nDamage=0\nRange=10\nSpeed=40\n\
         [CHILD_GUN]\nProjectile=CHILD_SHOT\nWarhead=CHILD_WH\nDamage=10\nSpeed=50\n\
         Bright=yes\nRange=8\n{child_weapon}\n\
         [PARENT_SHOT]\n{parent}\n\
         [CHILD_SHOT]\n{child_projectile}\n\
         [Warheads]\n0=QUIET\n1=CHILD_WH\n\
         [QUIET]\nCellSpread=0\n{VERSES_ZERO}\n\
         [CHILD_WH]\nCellSpread=0\n{child_verses}\n{warhead}\n\
         {extra}\n",
    );
    crate::sim::arena_fixture::clear_ground_ini(&text)
}

enum TargetChoice {
    Cell(u16, u16),
    Entity(usize),
}

struct Scene {
    world: Simulation,
    rules: RuleSet,
    source: u64,
    parent: u64,
    request: ProjectileDetonation,
    victims: Vec<u64>,
}

#[allow(clippy::too_many_arguments)]
fn build_scene(
    ini: &IniFile,
    source_cell: (u16, u16),
    parent_cell: (u16, u16),
    parent_z: i32,
    target: TargetChoice,
    victims: &[(&str, u16, u16, i32)],
    parent_firepower: f64,
) -> Scene {
    let mut rules = RuleSet::from_ini(ini).unwrap();
    rules.set_weapon_speed_for_test("CHILD_GUN", 50);
    rules.set_weapon_speed_for_test("PARENT_GUN", 40);
    let mut world = crate::sim::arena_fixture::rival_arena(&rules, 31);
    world.intern_rule_type_ids(&rules);
    world.resolve_type_handles(&rules);
    world.session.set_binary_frame_for_test(100);
    let source = world
        .spawn_object(
            "SOURCE",
            "Americans",
            source_cell.0,
            source_cell.1,
            0,
            &rules,
        )
        .unwrap();
    world.add_entity_occupancy(source);
    world
        .substrate
        .entities
        .get_mut(source)
        .unwrap()
        .set_in_playfield(true);
    let mut victim_ids = Vec::new();
    for &(kind, rx, ry, height) in victims {
        let id = world
            .spawn_object(kind, "Russians", rx, ry, 0, &rules)
            .unwrap();
        world.add_entity_occupancy(id);
        world
            .substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_in_playfield(true);
        if world.substrate.entities.get(id).unwrap().category()
            == crate::map::entities::EntityCategory::Aircraft
        {
            crate::sim::world::Simulation::aircraft_tracker_add(
                &world.session,
                &mut world.substrate,
                id,
            );
        }
        if height != 0 {
            crate::sim::world::Simulation::set_object_height_unmarked(
                &mut world.substrate,
                &world.resolved_terrain,
                id,
                height,
            );
        }
        victim_ids.push(id);
    }
    let target = match target {
        TargetChoice::Cell(rx, ry) => ProjectileTarget::Cell { rx, ry },
        TargetChoice::Entity(index) => ProjectileTarget::Entity(victim_ids[index]),
    };
    let payload = ProjectilePayload::new(
        0,
        world.interner.intern("QUIET"),
        Some(world.interner.intern("PARENT_SHOT")),
        true,
    )
    .with_firepower_multiplier(NativeF64Bits::from_bits(parent_firepower.to_bits()));
    let parent = world.construct_projectile(source, target, payload, &rules);
    let position = ProjectileCoord::new(
        i32::from(parent_cell.0) * 256 + 128,
        i32::from(parent_cell.1) * 256 + 128,
        parent_z,
    );
    world
        .projectiles
        .get_mut(parent)
        .unwrap()
        .set_position_for_test(position);
    let request = ProjectileDetonation {
        projectile_id: parent,
        source_id: source,
        target,
        impact: ProjectileCoord::new(position.x + 11, position.y + 11, position.z + 10),
        payload,
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    Scene {
        world,
        rules,
        source,
        parent,
        request,
        victims: victim_ids,
    }
}

fn detonate(scene: &mut Scene) {
    let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
    crate::sim::combat::world_receiver::commit_projectiles(
        &mut scene.world,
        &mut run,
        &[scene.request],
        &scene.rules,
        None,
    );
    run.finish();
}

fn child_ids(scene: &Scene) -> Vec<u64> {
    scene
        .world
        .projectiles
        .iter()
        .filter(|(id, _)| **id != scene.parent)
        .map(|(id, _)| *id)
        .collect()
}

fn child_targets(scene: &Scene) -> Vec<ProjectileTarget> {
    child_ids(scene)
        .into_iter()
        .map(|id| scene.world.projectiles.get(id).unwrap().target())
        .collect()
}

fn child_damage(scene: &Scene) -> Vec<i32> {
    child_ids(scene)
        .into_iter()
        .map(|id| {
            scene
                .world
                .projectiles
                .get(id)
                .unwrap()
                .payload()
                .base_damage
        })
        .collect()
}

fn component(velocity: ProjectileVelocity, index: usize) -> f64 {
    f64::from_bits(velocity.native()[index].bits())
}

#[test]
fn default_airburst_spread_is_the_native_three_by_three_area() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=2\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        600,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    let targets = child_targets(&scene);
    assert_eq!(targets.len(), 9, "AirburstSpread defaults to 1.5 cells");
    let mut cells: Vec<(u16, u16)> = targets
        .iter()
        .map(|target| match target {
            ProjectileTarget::Cell { rx, ry } => (*rx, *ry),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    cells.sort_unstable();
    let mut expected = Vec::new();
    for ry in 19..=21 {
        for rx in 9..=11 {
            expected.push((rx, ry));
        }
    }
    expected.sort_unstable();
    assert_eq!(cells, expected, "Cluster is ignored without UseCluster");
}

#[test]
fn airburst_use_cluster_selects_even_and_random_cells() {
    for random in [false, true] {
        let random_key = if random {
            "Airburst.RandomClusters=yes\n"
        } else {
            ""
        };
        let ini = rules_ini(
            &format!(
                "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=3\n\
                 AirburstSpread=2\nAirburst.UseCluster=yes\n{random_key}"
            ),
            "",
            "",
            "",
            "",
            "",
        );
        let mut scene = build_scene(
            &ini,
            (6, 20),
            (10, 20),
            0,
            TargetChoice::Cell(10, 20),
            &[],
            1.0,
        );
        detonate(&mut scene);
        let targets = child_targets(&scene);
        assert_eq!(targets.len(), 3, "random={random}");
        let mut unique: Vec<String> = targets.iter().map(|target| format!("{target:?}")).collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 3, "random={random}");
        if !random {
            let mut cells: Vec<(u16, u16)> = targets
                .iter()
                .map(|target| match target {
                    ProjectileTarget::Cell { rx, ry } => (*rx, *ry),
                    other => panic!("unexpected {other:?}"),
                })
                .collect();
            cells.sort_unstable();
            assert_eq!(cells, vec![(10, 18), (10, 20), (10, 22)]);
        }
    }
}

#[test]
fn airburst_spread_beyond_the_old_cap_covers_the_map() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\nAirburstSpread=300\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    assert_eq!(
        child_targets(&scene).len(),
        32 * 32,
        "a spread above 256 cells still enumerates every real map cell"
    );
}

#[test]
fn random_clusters_skips_the_one_past_draw_like_the_null_vector_slot() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=4\nAirburstSpread=0\n\
         Airburst.UseCluster=yes\nAirburst.RandomClusters=yes\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    let children = child_ids(&scene).len();
    assert!(
        (1..=3).contains(&children),
        "one draw lands on the null one-past slot and fires nothing: {children}"
    );
}

#[test]
fn child_laser_draws_with_the_retained_house_and_phobos_thickness() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\nAirburstSpread=0\n",
        "IsLaser=yes\nIsHouseColor=yes\nIsSingleColor=yes\nLaserThickness=5\n",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        600,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    let americans = scene.world.interner.intern("Americans");
    scene.world.substrate.entities.remove(scene.source);
    detonate(&mut scene);
    let births: Vec<_> = scene
        .world
        .lifecycle_outputs
        .as_slice()
        .iter()
        .filter_map(|output| match output {
            crate::sim::world::LifecycleOutput::LaserCreated(birth) => Some(*birth),
            _ => None,
        })
        .collect();
    assert_eq!(births.len(), 1, "the expired firer no longer blocks it");
    assert_eq!(births[0].width, 5, "LaserThickness");
    assert!(births[0].supported, "IsSupported follows Thickness > 3");
    assert_eq!(
        births[0].color,
        crate::sim::combat::laser::LaserColor::House(americans),
        "IsHouseColor takes the retained firing House"
    );
}

#[test]
fn child_rad_beam_takes_the_house_color_and_the_scattered_source_coords() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\nAirburstSpread=0\n",
        "IsRadBeam=yes\nBeam.IsHouseColor=yes\n",
        "Inviso=yes\nFlakScatter=yes\n",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        600,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    let americans = scene.world.interner.intern("Americans");
    scene.world.substrate.entities.remove(scene.source);
    detonate(&mut scene);
    let births: Vec<_> = scene
        .world
        .lifecycle_outputs
        .as_slice()
        .iter()
        .filter_map(|output| match output {
            crate::sim::world::LifecycleOutput::RadiationBeamCreated(birth) => Some(*birth),
            _ => None,
        })
        .collect();
    assert_eq!(births.len(), 1, "the expired firer no longer blocks it");
    assert_eq!(births[0].house, Some(americans));
    assert!(births[0].house_color);
    assert_eq!(
        births[0].to, births[0].from,
        "GetTargetCoordsForFiring: Inviso + FlakScatter uses the bullet's own location"
    );
    assert_eq!(births[0].from.z, 600, "the child's unlimbo source z");
}

#[test]
fn child_electric_bolt_draws_after_the_firer_expires() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\nAirburstSpread=0\n",
        "IsElectricBolt=yes\n",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    scene.world.substrate.entities.remove(scene.source);
    detonate(&mut scene);
    let bolts = scene
        .world
        .lifecycle_outputs
        .as_slice()
        .iter()
        .filter(|output| {
            matches!(
                output,
                crate::sim::world::LifecycleOutput::ElectricBoltCreated(_)
            )
        })
        .count();
    assert_eq!(bolts, 1, "the expired firer no longer blocks it");
}

#[test]
fn target_as_source_moves_the_origin_and_skip_height_keeps_the_bullet_z() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n\
         Airburst.TargetAsSource=yes\n",
        "",
        "",
        "",
        "Airburst.TargetAsSource.SkipHeight=yes\n",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        600,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    let ids = child_ids(&scene);
    assert_eq!(ids.len(), 1);
    let origin = scene.world.projectiles.get(ids[0]).unwrap().launch_origin();
    assert_eq!(origin.z, 600, "SkipHeight keeps the projectile's height");
    assert_ne!(
        (origin.x, origin.y),
        (10 * 256 + 128, 20 * 256 + 128),
        "TargetAsSource uses the selected cell, not the detonation point"
    );

    // Without SkipHeight the target cell's ground Z is used.
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n\
         Airburst.TargetAsSource=yes\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        600,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    let ids = child_ids(&scene);
    assert_eq!(ids.len(), 1);
    assert_eq!(
        scene
            .world
            .projectiles
            .get(ids[0])
            .unwrap()
            .launch_origin()
            .z,
        0
    );
}

#[test]
fn splits_targets_nearby_technos_and_excludes_the_source() {
    let ini = rules_ini(
        "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=2\n\
         Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\nRetargetSelf=no\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Entity(0),
        &[("VICTIM", 10, 20, 0), ("VICTIM", 12, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    let targets = child_targets(&scene);
    assert_eq!(targets.len(), 2);
    assert!(targets.contains(&ProjectileTarget::Entity(scene.victims[0])));
    assert!(targets.contains(&ProjectileTarget::Entity(scene.victims[1])));
}

#[test]
fn retarget_accuracy_one_keeps_the_original_target() {
    let ini = rules_ini(
        "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=2\nRetargetAccuracy=1.0\n\
         Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\nRetargetSelf=no\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Entity(0),
        &[("VICTIM", 10, 20, 0), ("VICTIM", 12, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    let targets = child_targets(&scene);
    assert_eq!(targets.len(), 2);
    assert!(
        targets
            .iter()
            .all(|target| *target == ProjectileTarget::Entity(scene.victims[0])),
        "RetargetAccuracy=1 must keep the existing target"
    );
}

#[test]
fn retarget_self_controls_whether_the_firer_is_a_candidate() {
    let ini = rules_ini(
        "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\nRetargetSelf=no\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (9, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    assert!(
        child_ids(&scene).is_empty(),
        "RetargetSelf=no leaves no Techno target"
    );

    let ini = rules_ini(
        "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\n\
         RetargetSelf=yes\nRetargetSelf.Probability=1.0\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (9, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    let targets = child_targets(&scene);
    assert_eq!(targets, vec![ProjectileTarget::Entity(scene.source)]);
}

#[test]
fn allow_repeat_targets_cycles_instead_of_adding_random_cells() {
    let base = "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=3\n\
                Splits.TargetingDistance=5\nRetargetSelf=no\n";
    let mut scene = build_scene(
        &rules_ini(
            &format!("{base}Splits.TargetCellRange=2\n"),
            "",
            "",
            "",
            "",
            "",
        ),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Entity(0),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    let targets = child_targets(&scene);
    assert_eq!(targets.len(), 3);
    assert_eq!(
        targets
            .iter()
            .filter(|target| **target == ProjectileTarget::Entity(scene.victims[0]))
            .count(),
        1,
        "without repeats the CellRange cells fill the remaining cluster slots"
    );

    let mut scene = build_scene(
        &rules_ini(
            &format!("{base}Splits.TargetCellRange=-1\nSplits.AllowRepeatTargets=yes\n"),
            "",
            "",
            "",
            "",
            "",
        ),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Entity(0),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    let targets = child_targets(&scene);
    assert_eq!(targets.len(), 3);
    assert!(
        targets
            .iter()
            .all(|target| *target == ProjectileTarget::Entity(scene.victims[0])),
        "AllowRepeatTargets cycles the single target"
    );
}

#[test]
fn weapon_targeting_overrides_warhead_affects_and_reads_can_target_houses() {
    let base = "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
                Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\nRetargetSelf=no\n";
    let warhead = "AffectsEnemies=no\n";
    let mut scene = build_scene(
        &rules_ini(base, "", "", warhead, "", ""),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    assert!(
        child_ids(&scene).is_empty(),
        "without UseWeaponTargeting the warhead AffectsEnemies gate applies"
    );

    let mut scene = build_scene(
        &rules_ini(
            &format!("{base}Splits.UseWeaponTargeting=yes\n"),
            "",
            "",
            warhead,
            "",
            "",
        ),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    let targets = child_targets(&scene);
    assert_eq!(targets, vec![ProjectileTarget::Entity(scene.victims[0])]);

    let mut scene = build_scene(
        &rules_ini(
            &format!("{base}Splits.UseWeaponTargeting=yes\n"),
            "CanTargetHouses=allies\n",
            "",
            warhead,
            "",
            "",
        ),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    assert!(child_ids(&scene).is_empty(), "CanTargetHouses=allies");
}

#[test]
fn split_child_retains_the_house_after_the_source_expires() {
    let base = "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
                Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\nRetargetSelf=no\n";
    let mut scene = build_scene(
        &rules_ini(base, "", "", "", "", ""),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    let americans = scene.world.interner.intern("Americans");
    scene.world.substrate.entities.remove(scene.source);
    detonate(&mut scene);
    let children = child_ids(&scene);
    assert_eq!(children.len(), 1);
    assert_eq!(
        scene
            .world
            .projectiles
            .get(children[0])
            .unwrap()
            .phobos_firer_house(),
        Some(americans),
        "SimulatedFiringUnlimbo keeps the parent's retained house"
    );
}

#[test]
fn weapon_targeting_reads_can_target_health_and_veterancy() {
    let base = "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
                Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\n\
                RetargetSelf=no\nSplits.UseWeaponTargeting=yes\n";
    let run = |weapon_keys: &str| {
        let mut scene = build_scene(
            &rules_ini(base, weapon_keys, "", "", "", ""),
            (6, 20),
            (10, 20),
            0,
            TargetChoice::Cell(10, 20),
            &[("VICTIM", 10, 20, 0)],
            1.0,
        );
        detonate(&mut scene);
        child_ids(&scene).len()
    };
    let build = |weapon_keys: &str| {
        build_scene(
            &rules_ini(base, weapon_keys, "", "", "", ""),
            (6, 20),
            (10, 20),
            0,
            TargetChoice::Cell(10, 20),
            &[("VICTIM", 10, 20, 0)],
            1.0,
        )
    };
    assert_eq!(run("CanTarget=units\n"), 1);
    assert_eq!(run("CanTarget=infantry\n"), 0);
    assert_eq!(run("CanTarget=land\n"), 1);
    assert_eq!(run("CanTarget=water\n"), 0);
    assert_eq!(run("CanTarget.MinHealth=0.5\n"), 1);
    assert_eq!(run("CanTarget.MinHealth=1\n"), 0, "hp > min is strict");
    assert_eq!(run("CanTarget.MaxHealth=1\n"), 1);
    assert_eq!(run("CanTarget.MaxHealth=0.5\n"), 0);
    assert_eq!(run("CanTargetVeterancy=veteran\n"), 0);

    let mut scene = build("CanTargetVeterancy=veteran\n");
    scene
        .world
        .substrate
        .entities
        .get_mut(scene.victims[0])
        .unwrap()
        .veterancy
        .set_rank_for_test(crate::sim::combat::veterancy::RANK_VETERAN_U16);
    detonate(&mut scene);
    assert_eq!(child_ids(&scene).len(), 1, "a veteran target passes");

    let mut scene = build("CanTarget.MinHealth=0.5\n");
    scene
        .world
        .substrate
        .entities
        .get_mut(scene.victims[0])
        .unwrap()
        .health
        .set_for_test(100);
    detonate(&mut scene);
    assert!(
        child_ids(&scene).is_empty(),
        "100/1000 health is below MinHealth"
    );

    let mut scene = build("CanTarget.MaxHealth=0.5\n");
    scene
        .world
        .substrate
        .entities
        .get_mut(scene.victims[0])
        .unwrap()
        .health
        .set_for_test(100);
    detonate(&mut scene);
    assert_eq!(
        child_ids(&scene).len(),
        1,
        "100/1000 health is within MaxHealth"
    );
}

#[test]
fn split_projectile_aa_and_aa_only_filter_air_and_ground_targets() {
    let base = "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
                Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\n\
                RetargetSelf=no\nSplits.UseWeaponTargeting=yes\n";
    // AA=no refuses an airborne target.
    let mut scene = build_scene(
        &rules_ini(base, "", "", "", "", ""),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("PLANE", 10, 20, 300)],
        1.0,
    );
    detonate(&mut scene);
    assert!(child_ids(&scene).is_empty());

    // AA=yes accepts it.
    let mut scene = build_scene(
        &rules_ini(base, "", "AA=yes\n", "", "", ""),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("PLANE", 10, 20, 300)],
        1.0,
    );
    detonate(&mut scene);
    assert_eq!(child_targets(&scene).len(), 1);

    // AA=yes + AAOnly=yes refuses a grounded target.
    let mut scene = build_scene(
        &rules_ini(base, "", "AA=yes\nAAOnly=yes\n", "", "", ""),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    assert!(child_ids(&scene).is_empty());
}

#[test]
fn apply_firepower_mult_scales_the_child_damage() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n\
         AirburstWeapon.ApplyFirepowerMult=yes\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        2.0,
    );
    detonate(&mut scene);
    assert_eq!(child_damage(&scene), vec![20]);
    assert_eq!(
        child_ids(&scene)
            .into_iter()
            .map(|id| scene
                .world
                .projectiles
                .get(id)
                .unwrap()
                .payload()
                .firepower_multiplier())
            .collect::<Vec<_>>(),
        vec![NativeF64Bits::from_bits(2.0_f64.to_bits())]
    );

    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        2.0,
    );
    detonate(&mut scene);
    assert_eq!(child_damage(&scene), vec![10]);
}

#[test]
fn radial_fire_segments_fan_around_the_original_target() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=2\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n\
         AirburstWeapon.HeadToTarget=yes\nAirburstWeapon.RadialFireSegments=2\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (6, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    let ids = child_ids(&scene);
    assert_eq!(ids.len(), 2);
    let first = scene.world.projectiles.get(ids[0]).unwrap().velocity();
    let second = scene.world.projectiles.get(ids[1]).unwrap().velocity();
    // Segment 0 points at -90 degrees from the target direction, segment 1
    // at 0 degrees; the retail trig table keeps a 0.0015 rad quarter-turn
    // residue, well below a visible direction change.
    assert!(component(first, 0).abs() < 0.1);
    assert!((component(first, 1) - 50.0).abs() < 0.1);
    assert!((component(second, 0) - 50.0).abs() < 0.1);
    assert!(component(second, 1).abs() < 0.1);
}

#[test]
fn source_scatter_moves_the_child_origin_around_the_parent() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n\
         AirburstWeapon.SourceScatterMin=2\nAirburstWeapon.SourceScatterMax=2\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    let ids = child_ids(&scene);
    assert_eq!(ids.len(), 1);
    let origin = scene.world.projectiles.get(ids[0]).unwrap().launch_origin();
    let dx = f64::from(origin.x - (10 * 256 + 128));
    let dy = f64::from(origin.y - (20 * 256 + 128));
    let distance = (dx * dx + dy * dy).sqrt();
    assert!(
        (500.0..=524.0).contains(&distance),
        "the scatter distance is 512 leptons, got {distance}"
    );
    assert_eq!(origin.z, 0);
}

#[test]
fn firing_effects_play_the_child_weapon_anim_and_report() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n",
        "Anim=GUNFIRE\nReport=GUNRPT\n",
        "",
        "",
        "AirburstWeapon.UseFiringEffects=yes\n",
        "[Animations]\n0=GUNFIRE\n",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    let mut art = ArtRegistry::from_ini(&IniFile::from_str("[GUNFIRE]\nRate=900\n"));
    art.bind_anim_frame_count_for_test("GUNFIRE", 6);
    scene.rules.install_art_fixture(art);
    detonate(&mut scene);
    assert!(
        scene.world.anims().count() >= 1,
        "weapon Anim is constructed"
    );
    assert!(
        scene.world.sound_events().iter().any(|event| match event {
            crate::sim::world::SimSoundEvent::VocAt { sound_id, .. } => sound_id == "GUNRPT",
            _ => false,
        }),
        "weapon Report is played"
    );

    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n",
        "Anim=GUNFIRE\nReport=GUNRPT\n",
        "",
        "",
        "",
        "[Animations]\n0=GUNFIRE\n",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    let mut art = ArtRegistry::from_ini(&IniFile::from_str("[GUNFIRE]\nRate=900\n"));
    art.bind_anim_frame_count_for_test("GUNFIRE", 6);
    scene.rules.install_art_fixture(art);
    detonate(&mut scene);
    assert_eq!(scene.world.anims().count(), 0);
    assert!(!scene.world.sound_events().iter().any(|event| match event {
        crate::sim::world::SimSoundEvent::VocAt { sound_id, .. } => sound_id == "GUNRPT",
        _ => false,
    }));
}

#[test]
fn around_target_chooses_between_the_original_target_and_the_impact() {
    let base = "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
                Splits.TargetingDistance=2\nSplits.TargetCellRange=-1\nRetargetSelf=no\n";
    let victims = [("VICTIM", 10, 22, 0)];
    let mut scene = build_scene(
        &rules_ini(base, "", "", "", "", ""),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(20, 20),
        &victims,
        1.0,
    );
    detonate(&mut scene);
    assert!(
        child_ids(&scene).is_empty(),
        "AroundTarget defaults to Splits"
    );

    let mut scene = build_scene(
        &rules_ini(&format!("{base}AroundTarget=no\n"), "", "", "", "", ""),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(20, 20),
        &victims,
        1.0,
    );
    detonate(&mut scene);
    assert_eq!(
        child_targets(&scene),
        vec![ProjectileTarget::Entity(scene.victims[0])]
    );
}

#[test]
fn weapon_targeting_checks_legal_target_and_warhead_verses() {
    let base = "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
                Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\n\
                RetargetSelf=no\nSplits.UseWeaponTargeting=yes\n";
    let mut scene = build_scene(
        &rules_ini_full(base, "", "", "", "", "", VERSES_ZERO, ""),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    assert!(
        child_ids(&scene).is_empty(),
        "zero Verses rejects the target"
    );

    let mut scene = build_scene(
        &rules_ini_full(base, "", "", "", "", "", VERSES_FULL, "LegalTarget=no\n"),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    assert!(
        child_ids(&scene).is_empty(),
        "LegalTarget=no rejects the target"
    );
}

#[test]
fn targeting_distance_cylindrical_ignores_aircraft_height() {
    let base = "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
                Splits.TargetingDistance=5\nSplits.TargetCellRange=-1\n\
                RetargetSelf=no\nSplits.UseWeaponTargeting=yes\n";
    // 4 cells horizontal plus 2600 leptons of height exceeds 5 cells in 3-D
    // even after Phobos' quarter-squared-distance aircraft discount.
    let targets = [("PLANE", 14, 20, 2600)];
    let mut scene = build_scene(
        &rules_ini(base, "", "AA=yes\n", "", "", ""),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &targets,
        1.0,
    );
    detonate(&mut scene);
    assert!(
        child_ids(&scene).is_empty(),
        "3-D distance rejects the plane"
    );

    let mut scene = build_scene(
        &rules_ini(
            &format!("{base}Splits.TargetingDistance.Cylindrical=yes\n"),
            "",
            "AA=yes\n",
            "",
            "",
            "",
        ),
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &targets,
        1.0,
    );
    detonate(&mut scene);
    assert_eq!(child_targets(&scene).len(), 1);
}

#[test]
fn around_target_uses_the_live_target_position() {
    let ini = rules_ini(
        "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         Splits.TargetingDistance=2\nSplits.TargetCellRange=-1\nRetargetSelf=no\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (6, 20),
        0,
        TargetChoice::Entity(0),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    let victim = scene.victims[0];
    scene.world.remove_entity_occupancy(victim);
    scene
        .world
        .substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .position = crate::sim::components::Position::new(
        10,
        24,
        0,
        Some(0),
        crate::util::fixed_math::SimFixed::from_num(128),
        crate::util::fixed_math::SimFixed::from_num(128),
    );
    scene.world.add_entity_occupancy(victim);
    detonate(&mut scene);
    assert_eq!(
        child_targets(&scene),
        vec![ProjectileTarget::Entity(victim)],
        "the search is centered on the target's current GetCoords"
    );
}

#[test]
fn split_cell_content_uses_the_bridge_deck_list() {
    let ini = rules_ini(
        "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         Splits.TargetingDistance=2\nSplits.TargetCellRange=-1\nRetargetSelf=no\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (6, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    let victim = scene.victims[0];
    scene
        .world
        .substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .set_on_bridge(true);
    scene.world.remove_entity_occupancy(victim);
    scene.world.add_entity_occupancy(victim);
    scene
        .world
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(10, 20)
        .unwrap()
        .bridge_facts
        .raw_flags |= crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    detonate(&mut scene);
    assert_eq!(
        child_targets(&scene),
        vec![ProjectileTarget::Entity(victim)],
        "CellClass::GetContent reads the deck list on a bridge cell"
    );
}

#[test]
fn splits_parent_impact_ladder_uses_the_explodes_arm() {
    let ini = rules_ini(
        "Splits=yes
AirburstWeapon=CHILD_GUN
Cluster=1
",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    let delivery = crate::sim::combat::projectile_delivery::classify_projectile_delivery(
        scene.rules.weapon("PARENT_GUN").unwrap(),
        &scene.rules,
    );
    let parent = scene.parent;
    *scene
        .world
        .projectiles
        .get_mut(parent)
        .unwrap()
        .collision_mut_for_test() = delivery.collision;
    let bullet = scene
        .world
        .projectiles
        .get(parent)
        .unwrap()
        .impact_ladder_bullet(true);
    assert!(
        bullet.airburst,
        "Splits takes the Explodes airburst arm used by the impact ladder"
    );
    assert_eq!(
        scene
            .world
            .projectiles
            .get(parent)
            .unwrap()
            .collision()
            .airburst,
        false,
        "the Bullet AI airburst flag stays native"
    );
}

#[test]
fn empty_repeat_target_list_still_consumes_the_initial_draw() {
    let run = |repeat: &str| {
        let ini = rules_ini(
            &format!(
                "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
                 Splits.TargetingDistance=2\nSplits.TargetCellRange=-1\nRetargetSelf=no\n{repeat}"
            ),
            "",
            "",
            "",
            "",
            "",
        );
        let rules = RuleSet::from_ini(&ini).unwrap();
        let repeat_key = rules
            .projectile("PARENT_SHOT")
            .unwrap()
            .phobos_airburst
            .splits_allow_repeat_targets;
        if repeat.is_empty() {
            assert_eq!(repeat_key, None, "{repeat}");
        } else {
            assert_eq!(repeat_key, Some(true), "{repeat}");
        }
        let mut scene = build_scene(
            &ini,
            (6, 20),
            (10, 20),
            0,
            TargetChoice::Cell(10, 20),
            &[],
            1.0,
        );
        detonate(&mut scene);
        assert!(child_ids(&scene).is_empty());
        scene.world.scenario_rng.native_state_hex()
    };
    let without = run("");
    let with = run("Splits.AllowRepeatTargets=yes\n");
    assert_ne!(
        without, with,
        "Phobos draws RandomRanged(0, Count - 1) once even for an empty list"
    );
}

#[test]
fn splits_takes_the_airburst_impact_arms() {
    for (parent, expected) in [
        ("Splits=yes\nAirburstWeapon=CHILD_GUN\n", (false, true)),
        ("Airburst=yes\nAirburstWeapon=CHILD_GUN\n", (true, true)),
        ("", (false, false)),
    ] {
        let ini = rules_ini(parent, "", "", "", "", "");
        let rules = RuleSet::from_ini(&ini).unwrap();
        let delivery = crate::sim::combat::projectile_delivery::classify_projectile_delivery(
            rules.weapon("PARENT_GUN").unwrap(),
            &rules,
        );
        assert_eq!(
            delivery.collision.airburst, expected.0,
            "the Bullet AI airburst field stays native"
        );
        assert_eq!(
            delivery.collision.explodes_airburst(),
            expected.1,
            "Phobos Explodes makes Splits use the Airburst arm"
        );
    }
}

#[test]
fn an_inviso_child_takes_fires_snap_to_its_target() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n",
        "",
        "Inviso=yes\n",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    let ids = child_ids(&scene);
    assert_eq!(ids.len(), 1);
    let child = scene.world.projectiles.get(ids[0]).unwrap();
    let ProjectileTarget::Cell { rx, ry } = child.target() else {
        panic!("expected a cell target");
    };
    // Phobos' `MoveTo` is the `0x00468670` virtual VERA ports as Fire.
    assert_eq!(
        (child.position().x, child.position().y),
        (i32::from(rx) * 256 + 128, i32::from(ry) * 256 + 128),
        "Fire's Inviso arm stands the child on its target"
    );
}

#[test]
fn splits_detonates_once_and_emits_only_its_cluster() {
    let ini = rules_ini(
        "Splits=yes\nAirburstWeapon=CHILD_GUN\nCluster=3\n\
         Splits.TargetingDistance=5\nSplits.TargetCellRange=2\nRetargetSelf=no\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Entity(0),
        &[("VICTIM", 10, 20, 0)],
        1.0,
    );
    detonate(&mut scene);
    assert_eq!(
        child_ids(&scene).len(),
        3,
        "a Splits parent takes one detonation, not Cluster detonations"
    );
}

#[test]
#[cfg(all(feature = "phobos", feature = "ares"))]
fn phobos_replaces_ares_airburst() {
    let ini = rules_ini(
        "Airburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=1\n\
         AirburstSpread=1.5\nAirburst.UseCluster=yes\n",
        "",
        "",
        "",
        "",
        "",
    );
    let mut scene = build_scene(
        &ini,
        (6, 20),
        (10, 20),
        0,
        TargetChoice::Cell(10, 20),
        &[],
        1.0,
    );
    detonate(&mut scene);
    assert_eq!(
        child_ids(&scene).len(),
        1,
        "Phobos' airburst hook takes precedence over Ares' replaced tail"
    );
}
