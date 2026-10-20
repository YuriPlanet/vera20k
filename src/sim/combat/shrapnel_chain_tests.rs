//! Retail Prism bounce integration through FireAt and the live detonation
//! receiver. Original SpawnShrapnel46A310 precedes Apply_area_damage469A83;
//! every child is constructed/fired and its beam copied before either damage
//! callbacks or the next child. These tests do not emulate the native driver.

use super::{
    CombatEmit, TargetKind, build_attacker_snapshot, combat_weapon, world_fire, world_receiver,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::{ProjectileDetonation, ProjectileDetonationReason, ProjectileTarget};
use crate::sim::world::{FrameEffects, LifecycleOutput, LifecycleTestEvent, Simulation};

struct PrismScene {
    world: Simulation,
    rules: RuleSet,
    source: u64,
    victim: u64,
    neighbor: u64,
}

impl PrismScene {
    fn new(truck: bool, elite: bool) -> Option<Self> {
        Self::at(truck, elite, 7)
    }

    fn at(truck: bool, elite: bool, source_rx: u16) -> Option<Self> {
        let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        // These stock children never read the absent native Bullet+130
        // WeaponType through RadLevel or Inviso FlakScatter. The general
        // null-weapon distinction is a separate unresolved Bullet path.
        for name in ["CometFragment", "SuperCometFragment"] {
            let weapon = rules.weapon(name).unwrap();
            assert_eq!(weapon.rad_level, 0);
            assert!(
                !rules
                    .projectile(weapon.projectile.as_deref().unwrap())
                    .unwrap()
                    .flak_scatter
            );
        }
        let mut world = crate::sim::arena_fixture::rival_arena(&rules, 31);
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        let source = world
            .spawn_object("SREF", "Americans", source_rx, 16, 64, &rules)
            .unwrap();
        let victim = world
            .spawn_object(
                if truck { "DTRUCK" } else { "MTNK" },
                "Russians",
                16,
                16,
                0,
                &rules,
            )
            .unwrap();
        let neighbor = world
            .spawn_object("E1", "Russians", 17, 16, 0, &rules)
            .unwrap();
        world
            .spawn_object("E1", "Russians", 16, 17, 0, &rules)
            .unwrap();
        if truck {
            world
                .substrate
                .entities
                .get_mut(victim)
                .unwrap()
                .health
                .set_for_test(1);
        }
        if elite {
            world
                .substrate
                .entities
                .get_mut(source)
                .unwrap()
                .veterancy
                .set_elite();
        }
        world
            .assign_target_represented(
                source,
                Some(TargetKind::Entity(victim)),
                Some(&rules),
                None,
                FrameEffects::default(),
            )
            .unwrap();
        Some(Self {
            world,
            rules,
            source,
            victim,
            neighbor,
        })
    }

    fn launch(&mut self) -> u64 {
        self.launch_from(self.source, TargetKind::Entity(self.victim))
    }

    fn launch_from(&mut self, source: u64, target: TargetKind) -> u64 {
        self.world.session.set_binary_frame_for_test(100);
        let (target_coords, target_type_ref) = match target {
            TargetKind::Entity(id) => {
                let target = self.world.substrate.entities.get(id).unwrap();
                (
                    (
                        target.position.rx(),
                        target.position.ry(),
                        target.position.sub_x(),
                        target.position.sub_y(),
                    ),
                    target.type_ref(),
                )
            }
            TargetKind::Cell(rx, ry) => (
                (
                    rx,
                    ry,
                    crate::util::fixed_math::SimFixed::from_num(128),
                    crate::util::fixed_math::SimFixed::from_num(128),
                ),
                self.world.interner.intern(""),
            ),
        };
        let actor = self.world.substrate.entities.get(source).unwrap();
        let object = self
            .world
            .interner
            .object_type(actor.type_ref(), &self.rules)
            .unwrap();
        let selected = combat_weapon::resolve_weapon_index(
            &self.rules,
            actor,
            object,
            0,
            &self.world.substrate.entities,
            &self.world.interner,
        )
        .unwrap();
        let shot = world_fire::AdmittedFire {
            snap: build_attacker_snapshot(actor, target, None),
            obj: object,
            selected,
            target_coords,
            target_type_ref,
            is_garrison: false,
        };
        world_fire::emit_admitted_fire(
            &mut self.world,
            &self.rules,
            shot,
            100,
            &mut CombatEmit::default(),
            None,
            FrameEffects::default(),
        )
        .expect("retail Comet launch")
    }

    fn admit_tree(&mut self, rx: u16, ry: u16) -> u64 {
        assert_eq!(
            crate::sim::terrain_spawn::construct_terrain_objects(
                &mut self.world,
                &[crate::map::overlay::TerrainObject {
                    rx,
                    ry,
                    name: "TREE01".into()
                }],
                &self.rules,
                false,
            ),
            1
        );
        self.world.terrain_objects.cell_index()[&(rx, ry)]
    }

    fn detonate(&mut self, parent: u64) {
        let bullet = self.world.projectiles.get(parent).unwrap();
        let impact = ProjectileDetonation {
            projectile_id: parent,
            source_id: bullet.source_id(),
            target: bullet.target(),
            impact: bullet.position(),
            payload: bullet.payload(),
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        let mut run = world_receiver::ReceiverRun::default();
        world_receiver::commit_projectiles(&mut self.world, &mut run, &[impact], &self.rules, None);
        run.finish();
    }
}

#[test]
fn retail_comet_selects_a_terrain_head_and_copies_its_bounce_beam() {
    let Some(mut scene) = PrismScene::new(false, false) else {
        return;
    };
    // SpawnShrapnel46A4D7..46A509 accepts a houseless Terrain head. The
    // first CellSpread offset is (1,-1), before either existing E1 neighbor.
    // Use the map Terrain owner, including its real cell and Logic admission.
    let tree_id = scene.admit_tree(17, 15);
    let tree = &scene.world.terrain_objects.objects()[&tree_id];
    let tree_coord = tree.world_coord();
    let tree_health = tree.health();
    assert!(tree.is_live() && tree.in_logic_vector());
    assert_eq!(tree_health, 200, "retail TREE01 uses TreeStrength");
    assert_eq!(
        scene
            .world
            .substrate
            .occupancy
            .cell_objects(
                17,
                15,
                crate::sim::movement::locomotor::MovementLayer::Ground,
                Some(tree_id),
            )
            .next(),
        Some(crate::sim::occupancy::CellObjectMember::Terrain(tree_id))
    );
    assert!(!scene.rules.warhead("CometWH").unwrap().wood);

    let parent = scene.launch();
    let origin = scene.world.projectiles.get(parent).unwrap().position();
    scene.world.lifecycle_outputs.clear_for_test();
    scene.detonate(parent);
    let mut children: Vec<_> = scene
        .world
        .projectiles
        .iter()
        .filter(|(id, _)| **id != parent)
        .map(|(_, child)| child)
        .collect();
    children.sort_by_key(|child| child.id());
    assert_eq!(
        children.len(),
        5,
        "the tree consumes one ordinary child slot"
    );
    assert_eq!(
        children[0].launch_target(),
        tree_coord,
        "first ring head must retain Terrain identity through child Fire"
    );
    assert_eq!(children[0].target(), ProjectileTarget::Terrain(tree_id));
    assert_eq!(
        children[0].position(),
        tree_coord,
        "stock Inviso child is placed at Terrain +58"
    );
    let beams: Vec<_> = scene
        .world
        .lifecycle_outputs
        .iter()
        .filter_map(|output| match output {
            LifecycleOutput::LaserCreated(birth) => Some(birth),
            _ => None,
        })
        .collect();
    assert_eq!(beams.len(), 5);
    assert_eq!(beams[0].from, origin);
    assert_eq!(beams[0].to, tree_coord, "first beam copies Terrain +A4");
    assert_eq!(
        scene.world.terrain_objects.objects()[&tree_id].health(),
        tree_health,
        "CometWH Wood=false does not damage the tree"
    );
}

#[test]
fn retail_comet_terrain_child_keeps_construction_height_through_first_logic() {
    let Some(mut scene) = PrismScene::new(false, false) else {
        return;
    };
    scene
        .world
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(17, 15)
        .unwrap()
        .level = 6;
    let tree_id = scene.admit_tree(17, 15);
    let tree_coord = scene.world.terrain_objects.objects()[&tree_id].world_coord();
    assert_eq!(tree_coord.z, 624, "native Terrain construction on level6");
    scene
        .world
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(17, 15)
        .unwrap()
        .level = 0;
    let parent = scene.launch();
    scene.detonate(parent);
    let child = scene
        .world
        .projectiles
        .iter()
        .find(|(_, p)| p.target() == ProjectileTarget::Terrain(tree_id))
        .map(|(&id, p)| (id, p.position(), p.launch_target()))
        .unwrap();
    assert_eq!((child.1, child.2), (tree_coord, tree_coord));
    assert_eq!(
        super::fire_coord::effect_target_coordinate(
            &scene.world,
            &scene.rules,
            ProjectileTarget::Terrain(tree_id)
        ),
        Some(tree_coord)
    );
    scene
        .world
        .session
        .set_binary_frame_for_test(scene.world.session.binary_frame() + 1);
    scene
        .world
        .visit_combat_tail(child.0, &scene.rules, None, FrameEffects::default());
    scene.world.flush_pending_delete();
    // Lowering the ground creates an above-ground Inviso bullet, not an
    // immediate impact. Ordinary AI467494..467B7A sees old height624, so
    // neither its same-cell <208 nor slow-speed <10 ground gate admits it.
    // The existing gravity owner advances it while +10C still names Terrain.
    let advanced = scene.world.projectiles.get(child.0).unwrap();
    assert_eq!(advanced.target(), ProjectileTarget::Terrain(tree_id));
    assert_eq!(advanced.launch_target(), tree_coord);
    assert_eq!(
        advanced.position(),
        crate::sim::projectile::ProjectileCoord::new(
            tree_coord.x,
            tree_coord.y,
            tree_coord.z - scene.rules.general.gravity
        )
    );
    assert_eq!(
        scene.world.terrain_objects.objects()[&tree_id].health(),
        200
    );
    assert_eq!(
        scene.world.terrain_objects.objects()[&tree_id].world_coord(),
        tree_coord
    );
}

#[test]
fn retail_elite_comet_terrain_child_recurses_from_its_tree_on_first_logic() {
    let Some(mut scene) = PrismScene::new(false, true) else {
        return;
    };
    let tree_id = scene.admit_tree(17, 15);
    let tree_coord = scene.world.terrain_objects.objects()[&tree_id].world_coord();
    let parent = scene.launch();
    scene.detonate(parent);
    let child = scene
        .world
        .projectiles
        .iter()
        .find(|(_, p)| p.target() == ProjectileTarget::Terrain(tree_id))
        .map(|(&id, p)| {
            assert_eq!(
                scene
                    .world
                    .interner
                    .resolve(p.payload().projectile_type().unwrap()),
                "SuperSmallCometP"
            );
            id
        })
        .unwrap();
    scene.world.lifecycle_outputs.clear_for_test();
    scene
        .world
        .session
        .set_binary_frame_for_test(scene.world.session.binary_frame() + 1);
    scene
        .world
        .visit_combat_tail(child, &scene.rules, None, FrameEffects::default());
    assert!(
        scene.world.lifecycle_outputs.iter().any(|output| matches!(
            output, LifecycleOutput::LaserCreated(beam) if beam.from == tree_coord
        )),
        "the tree admits its own elite child detonation and ordinary fragment beams"
    );
    assert_eq!(
        scene.world.terrain_objects.objects()[&tree_id].health(),
        200
    );
}

#[test]
fn retiring_prism_source_preserves_live_terrain_targets_and_copied_beams() {
    let Some(mut scene) = PrismScene::new(false, false) else {
        return;
    };
    let tree_id = scene.admit_tree(17, 15);
    let parent = scene.launch();
    scene.detonate(parent);
    let child = scene
        .world
        .projectiles
        .iter()
        .find(|(_, p)| p.target() == ProjectileTarget::Terrain(tree_id))
        .map(|(&id, _)| id)
        .unwrap();
    let beam_count = scene
        .world
        .lifecycle_outputs
        .iter()
        .filter(|e| matches!(e, LifecycleOutput::LaserCreated(_)))
        .count();
    scene.world.uninit_with_rules(scene.source, &scene.rules);
    let child = scene.world.projectiles.get(child).unwrap();
    assert_eq!(child.source_id(), super::RAD_NO_ATTACKER);
    assert_eq!(child.target(), ProjectileTarget::Terrain(tree_id));
    assert_eq!(
        scene
            .world
            .lifecycle_outputs
            .iter()
            .filter(|e| matches!(e, LifecycleOutput::LaserCreated(_)))
            .count(),
        beam_count
    );
    assert!(scene.world.terrain_objects.objects()[&tree_id].is_live());
}

#[test]
fn stock_shock_repairs_the_live_prism_terrain_child_before_limbo() {
    let Some(mut scene) = PrismScene::new(false, false) else {
        return;
    };
    let tree_id = scene.admit_tree(17, 15);
    let tree_coord = scene.world.terrain_objects.objects()[&tree_id].world_coord();
    let shock_source = scene
        .world
        .spawn_object("SHK", "Americans", 14, 15, 0, &scene.rules)
        .unwrap();
    let spacer_source = scene
        .world
        .spawn_object("E1", "Americans", 14, 14, 0, &scene.rules)
        .unwrap();
    let shock = scene.rules.weapon("ElectricBolt").unwrap();
    assert_eq!(shock.damage, 50);
    assert_eq!(shock.warhead.as_deref(), Some("Shock"));
    let warhead = scene.rules.warhead("Shock").unwrap();
    assert!(warhead.wood);
    let warhead_ref = scene.world.interner.intern("Shock");
    let owner = scene
        .world
        .substrate
        .entities
        .get(shock_source)
        .unwrap()
        .owner();
    // Seven ordinary stock Shock receives prepare the legal 25HP state;
    // the final hit is still a real ElectricBolt Fire/AI transaction.
    for _ in 0..7 {
        world_receiver::apply_area_damage(
            &mut scene.world,
            &scene.rules,
            None,
            tree_coord,
            shock.damage,
            warhead,
            (shock_source, Some(owner), warhead_ref),
            FrameEffects::default(),
        );
    }
    assert_eq!(scene.world.terrain_objects.objects()[&tree_id].health(), 25);

    let parent = scene.launch();
    let spacer = scene.launch_from(spacer_source, TargetKind::Cell(2, 2));
    let shock = scene.launch_from(shock_source, TargetKind::Cell(17, 15));
    let position = |id| {
        scene
            .world
            .substrate
            .logic
            .as_slice()
            .iter()
            .position(|entry| *entry == id)
            .unwrap()
    };
    assert!(position(parent) < position(spacer) && position(spacer) < position(shock));
    scene.world.clear_lifecycle_test_events_for_test();
    scene
        .world
        .session
        .set_binary_frame_for_test(scene.world.session.binary_frame() + 1);
    scene
        .world
        .visit_combat_tail(parent, &scene.rules, None, FrameEffects::default());

    // Native Logic55B608..55B619 increments after removal: parent skips
    // spacer, then Shock kills Terrain before the first appended child AI.
    assert!(scene.world.substrate.logic.as_slice().contains(&spacer));
    assert!(!scene.world.substrate.logic.as_slice().contains(&parent));
    assert!(!scene.world.substrate.logic.as_slice().contains(&shock));
    let events = scene.world.lifecycle_test_events_for_test();
    // The Shock bullet itself also targets this Cell. Identify the Prism
    // child by the actual Terrain-pointer repair and its original firer.
    let child = events
        .iter()
        .find_map(|event| match event {
            LifecycleTestEvent::ProjectilePointerExpiredVisited {
                expired_id,
                projectile_id,
                source_id,
                target: ProjectileTarget::Cell { rx: 17, ry: 15 },
                ..
            } if *expired_id == tree_id && *source_id == scene.source => Some(*projectile_id),
            _ => None,
        })
        .expect("a live Prism child receives the Terrain expiry callback");
    let boundary = events.iter().position(|event| matches!(event,
        LifecycleTestEvent::TerrainExpiryBoundary { stable_id, indexed: true, in_logic: true, live: true, .. }
        if *stable_id == tree_id)).expect("expiry begins while Terrain is indexed and in Logic");
    let repaired = events.iter().position(|event| matches!(event,
        LifecycleTestEvent::ProjectilePointerExpiredVisited { expired_id, projectile_id, expired_resolvable: true, target: ProjectileTarget::Cell { rx: 17, ry: 15 }, .. }
        if *expired_id == tree_id && *projectile_id == child)).expect("live observer repairs before Limbo");
    assert!(boundary < repaired);
    assert_eq!(events.iter().filter(|event| matches!(event,
        LifecycleTestEvent::TerrainExpiryBoundary { stable_id, indexed: true, in_logic: true, live: true, .. }
        if *stable_id == tree_id)).count(), 4);
    assert!(
        !scene
            .world
            .terrain_objects
            .cell_index()
            .contains_key(&(17, 15))
    );
    assert!(!scene.world.terrain_objects.objects()[&tree_id].in_logic_vector());
    assert!(!scene.world.terrain_objects.objects()[&tree_id].is_live());
    // Later appended-child AI can already retire the repaired observer in
    // this same pass. Its expiry trace above is the lifetime assertion;
    // the independent source and repeated-expiry tests keep a live observer.
    scene.world.flush_pending_delete();
    assert!(!scene.world.terrain_objects.objects().contains_key(&tree_id));
    if let Some(child) = scene.world.projectiles.get(child) {
        assert_eq!(
            child.target(),
            ProjectileTarget::Cell { rx: 17, ry: 15 },
            "a surviving repaired observer remains a Cell through deferred deletion"
        );
    }
}

#[test]
fn allied_head_hides_terrain_until_that_head_leaves_the_cell() {
    let Some(mut scene) = PrismScene::new(false, false) else {
        return;
    };
    let allied = scene
        .world
        .spawn_object("E1", "Americans", 17, 15, 0, &scene.rules)
        .unwrap();
    let tree_id = scene.admit_tree(17, 15);
    let parent = scene.launch();
    scene.detonate(parent);
    assert!(
        !scene
            .world
            .projectiles
            .iter()
            .any(|(_, child)| child.target() == ProjectileTarget::Terrain(tree_id)),
        "native head-only selection must not scan past an allied mobile object"
    );
    scene.world.uninit_with_rules(allied, &scene.rules);
    let parent = scene.launch();
    scene.detonate(parent);
    assert!(
        scene
            .world
            .projectiles
            .iter()
            .any(|(_, child)| child.target() == ProjectileTarget::Terrain(tree_id))
    );
}

/// Terrain71B920 with a live Bullet4684E0 observer. The native fixture supplies
/// the receiver, source and Bullet boundaries; this test uses their existing
/// production admission, damage and common expiry owners.
#[test]
fn terrain_bullet_expiry_matches_original_live_observer() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/terrain_bullet_expiry.json",
    ))
    .unwrap();
    for row in corpus["cases"].as_array().unwrap() {
        let Some(mut scene) = PrismScene::new(false, false) else {
            return;
        };
        let expected = &row["observer"];
        scene.world.session.set_binary_frame_for_test(1000);
        scene.world.scenario_rng = crate::sim::rng::SimRng::new(1);
        scene
            .world
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(10, 20)
            .unwrap()
            .level = 2;
        let tree_id = scene.admit_tree(10, 20);
        let point = scene.world.terrain_objects.objects()[&tree_id].world_coord();
        assert_eq!(
            point,
            native_coord(&expected["initial"]["target"]["location"])
        );
        scene
            .world
            .terrain_objects
            .objects_mut_for_test()
            .get_mut(&tree_id)
            .unwrap()
            .set_health_for_test(row["case"]["health"].as_i64().unwrap() as i32);
        let weapon = scene.rules.weapon("CometFragment").unwrap();
        let payload = super::combat_tests::payload_from_weapon(
            weapon.damage,
            scene
                .world
                .interner
                .intern(weapon.warhead.as_deref().unwrap()),
            scene.world.interner.intern(&weapon.id),
            &scene.rules,
            &mut scene.world.interner,
        )
        .with_installed_weapon(None);
        let bullet = super::projectile_delivery::FiredBullet::launched(
            &scene.world,
            &scene.rules,
            1,
            scene.source,
            ProjectileTarget::Terrain(tree_id),
            payload,
            weapon.speed,
            point,
            crate::sim::projectile::ProjectileVelocity::new(0, 0, 0),
        )
        .unwrap();
        let child = scene.world.allocate_stable_id();
        super::projectile_delivery::admit_fired_bullet(
            &mut scene.world,
            &scene.rules,
            weapon,
            child,
            bullet,
        );
        let check = |scene: &PrismScene, expected: &serde_json::Value| {
            let tree = &scene.world.terrain_objects.objects()[&tree_id];
            let bullet = scene.world.projectiles.get(child).unwrap();
            assert_eq!(
                bullet.source_id() == scene.source,
                expected["source"] == "source"
            );
            let target = match expected["target"]["kind"].as_str().unwrap() {
                "terrain" => ProjectileTarget::Terrain(tree_id),
                "cell" => ProjectileTarget::Cell { rx: 10, ry: 20 },
                kind => panic!("unhandled target {kind}"),
            };
            assert_eq!(bullet.target(), target);
            assert_eq!(tree.is_live(), expected["tree_alive"] == true);
            assert_eq!(!tree.is_live(), expected["tree_limbo"] == true);
            assert_eq!(tree.in_logic_vector(), expected["tree_in_logic"] == true);
            assert_eq!(
                scene.world.terrain_objects.cell_index().get(&(10, 20)) == Some(&tree_id),
                expected["tree_is_ground_head"] == true
            );
        };
        check(&scene, &expected["initial"]);
        assert_eq!(
            scene.world.scenario_rng.native_state_hex(),
            expected["scenario_before"]
        );
        scene.world.clear_lifecycle_test_events_for_test();
        if row["case"]["source_first"] == true {
            scene.world.notify_pointer_expired(
                scene.source,
                crate::sim::world::UninitContext::with_rules(&scene.rules),
            );
            check(&scene, &expected["after_source_before_damage"]);
        }
        let event = super::TerrainDamageEvent {
            stable_id: tree_id,
            rx: 10,
            ry: 20,
            damage: row["case"]["damage"].as_i64().unwrap() as i32,
            distance_leptons: 0,
            warhead_ref: scene.world.interner.intern("HE"),
            near_center_ic_isolation_eligible: true,
        };
        let mut run = world_receiver::ReceiverRun::default();
        world_receiver::commit_terrain(
            &mut scene.world,
            &mut run,
            event,
            false,
            &scene.rules,
            None,
        );
        run.finish();
        check(&scene, &expected["after_receiver"]);
        scene.world.notify_pointer_expired(
            tree_id,
            crate::sim::world::UninitContext::with_rules(&scene.rules),
        );
        check(&scene, &expected["after_repeat"]);
        scene.world.notify_pointer_expired(
            scene.source,
            crate::sim::world::UninitContext::with_rules(&scene.rules),
        );
        check(&scene, &expected["after_source_expiry"]);
        assert_eq!(
            scene.world.scenario_rng.native_state_hex(),
            expected["scenario_after"]
        );
        let actual: Vec<_> = scene
            .world
            .lifecycle_test_events_for_test()
            .iter()
            .filter_map(|event| match event {
                LifecycleTestEvent::TerrainExpiryBoundary {
                    stable_id,
                    indexed,
                    in_logic,
                    live,
                    ..
                } if *stable_id == tree_id => Some((*indexed, *in_logic, *live)),
                _ => None,
            })
            .collect();
        let native: Vec<_> = expected["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| {
                event["event"] == "bullet_pointer_expired" && event["expired"] == "tree"
            })
            .map(|event| &event["before"])
            .map(|state| {
                (
                    state["tree_is_ground_head"] == true,
                    state["tree_in_logic"] == true,
                    state["tree_alive"] == true,
                )
            })
            .collect();
        assert_eq!(
            actual, native,
            "four native broadcasts precede removal; repeated expiry follows it"
        );
        for event in scene.world.lifecycle_test_events_for_test() {
            if let LifecycleTestEvent::ProjectilePointerExpiredVisited {
                expired_id,
                projectile_id,
                target,
                ..
            } = event
                && *expired_id == tree_id
                && *projectile_id == child
            {
                assert_eq!(*target, ProjectileTarget::Cell { rx: 10, ry: 20 });
            }
        }
    }
}

#[test]
fn retail_comet_creates_each_bounce_beam_at_the_live_child_birth() {
    let Some(mut scene) = PrismScene::new(false, false) else {
        return;
    };
    let parent = scene.launch();
    let origin = scene.world.projectiles.get(parent).unwrap().position();
    scene.world.lifecycle_outputs.clear_for_test();
    scene.detonate(parent);
    let beams: Vec<_> = scene
        .world
        .lifecycle_outputs
        .iter()
        .filter_map(|output| match output {
            LifecycleOutput::LaserCreated(birth) => Some(birth),
            _ => None,
        })
        .collect();
    assert_eq!(
        beams.len(),
        5,
        "normal Comet creates five visible bounce beams"
    );
    assert_eq!(
        scene.world.projectiles.len(),
        6,
        "children are live before the detonation receiver returns"
    );
    for beam in beams {
        assert_eq!(beam.from, origin);
        assert_eq!(beam.width, 1);
        assert!(!beam.supported);
        assert_eq!(beam.duration, 15);
    }
}

#[test]
fn stock_truck_death_notifies_a_child_selected_before_parent_damage() {
    let Some(mut scene) = PrismScene::new(true, false) else {
        return;
    };
    let parent = scene.launch();
    scene.world.clear_lifecycle_test_events_for_test();
    scene.detonate(parent);
    assert!(
        !scene
            .world
            .substrate
            .entities
            .get(scene.neighbor)
            .unwrap()
            .lifecycle
            .object_alive(),
        "stock Demobomb callback kills the selected ring neighbor"
    );
    assert!(scene.world.lifecycle_test_events_for_test().iter().any(|event| matches!(event,
        LifecycleTestEvent::ProjectilePointerExpiredVisited { expired_id, projectile_id, projectile_resolvable: true, target: ProjectileTarget::Cell { rx: 17, ry: 16 }, .. }
            if *expired_id == scene.neighbor && *projectile_id != parent
    )), "an already-registered child receives the target's native expiry callback");
}

fn native_coord(value: &serde_json::Value) -> crate::sim::projectile::ProjectileCoord {
    crate::sim::projectile::ProjectileCoord::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

/// Original46A310 executes its complete selected/random branches, child
/// Construct/Fire and laser calls. Its Unit/Terrain vtables, lists and coordinates
/// are supplied boundaries; it does not run a whole native skirmish.
#[test]
fn shrapnel_children_and_lasers_match_original_spawn_shrapnel() {
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::native_identity::NativeUniqueIdCursor;
    use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
    use crate::sim::rng::{MainRng, SimRng};
    use crate::util::fixed_math::SimFixed;
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let corpora = [
        "tools/projectile_oracle/shrapnel_chain.json",
        "tools/projectile_oracle/terrain_shrapnel.json",
    ]
    .map(|path| {
        serde_json::from_str::<serde_json::Value>(crate::test_fixture::text(path)).unwrap()
    });
    for row in corpora
        .iter()
        .flat_map(|corpus| corpus["cases"].as_array().unwrap())
    {
        // Count0 is an explicit authored control, not a retail value. Keep
        // its INI read/postpass with the production rules owner.
        let count_rules = row["input"]["count"].as_i64().map(|count| {
            let weapon = fixture
                .rules
                .weapon(row["input"]["parent_weapon"].as_str().unwrap())
                .unwrap();
            let projectile = weapon.projectile.as_deref().unwrap();
            let patch = crate::rules::ini_parser::IniFile::from_str(&format!(
                "[{projectile}]\nShrapnelCount={count}\n"
            ));
            crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
                .expect("same retail fixture with explicit map control")
                .rules
        });
        let rules = count_rules.as_ref().unwrap_or(&fixture.rules);
        let name = row["input"]["name"].as_str().unwrap();
        let seed = row["input"]["seed"].as_u64().unwrap();
        let mut world = Simulation::with_seed(seed);
        crate::sim::arena_fixture::flat_arena(&mut world, rules);
        for y in 0..32 {
            for x in 0..32 {
                world
                    .resolved_terrain
                    .as_mut()
                    .unwrap()
                    .cell_mut_for_test(x, y)
                    .unwrap()
                    .level = 6;
            }
        }
        // guided_step.create supplies only these native CellClass slots.
        // Other array coordinates retain the shared Dummy instead of a
        // convenient extra clear cell, including the elite outer ring.
        let allocated: Vec<_> = (16..25)
            .flat_map(|y| (6..25).map(move |x| (x, y)))
            .collect();
        world
            .resolved_terrain
            .as_mut()
            .unwrap()
            .test_set_native_allocated_cells(&allocated);
        let owners = [
            world.interner.intern("Americans"),
            world.interner.intern("Russians"),
        ];
        let mut ids = std::collections::BTreeMap::new();
        for (index, actor) in row["actors"].as_array().unwrap().iter().enumerate() {
            if actor["native_class"] == "Terrain" {
                continue;
            }
            let id = index as u64 + 1;
            let actor_name = actor["name"].as_str().unwrap();
            ids.insert(actor_name, id);
            let coord = native_coord(&actor["location"]);
            let mut entity = GameEntity::test_default(
                id,
                "MTNK",
                "Americans",
                (coord.x / 256) as u16,
                (coord.y / 256) as u16,
            );
            entity.set_category_for_test(EntityCategory::Unit);
            entity.set_type_ref_for_test(world.interner.intern("MTNK"));
            entity.set_owner_for_test(owners[actor["house"].as_u64().unwrap() as usize]);
            entity
                .position
                .set_sub_x_for_test(SimFixed::from_num(coord.x % 256));
            entity
                .position
                .set_sub_y_for_test(SimFixed::from_num(coord.y % 256));
            entity.position.set_exact_z_leptons_for_test(Some(coord.z));
            entity.lifecycle.set_cell_marked_for_test(
                !(row["input"]["refused"] == true && actor_name == "victim"),
            );
            world.substrate.entities.insert(entity);
        }
        // Native actors are supplied boundaries, but Terrain representation
        // still enters through its real constructor/registration owner.
        world
            .set_next_stable_object_id_for_test(row["actors"].as_array().unwrap().len() as u64 + 1);
        let terrain_actors: Vec<_> = row["actors"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|actor| actor["native_class"] == "Terrain")
            .collect();
        if !terrain_actors.is_empty() {
            let authored: Vec<_> = terrain_actors
                .iter()
                .map(|actor| crate::map::overlay::TerrainObject {
                    rx: actor["cell"][0].as_u64().unwrap() as u16,
                    ry: actor["cell"][1].as_u64().unwrap() as u16,
                    name: "TREE01".into(),
                })
                .collect();
            assert_eq!(
                crate::sim::terrain_spawn::construct_terrain_objects(
                    &mut world, &authored, rules, false
                ),
                authored.len()
            );
            for actor in terrain_actors {
                let cell = (
                    actor["cell"][0].as_u64().unwrap() as u16,
                    actor["cell"][1].as_u64().unwrap() as u16,
                );
                let id = world.terrain_objects.cell_index()[&cell];
                ids.insert(actor["name"].as_str().unwrap(), id);
                assert_eq!(
                    world.terrain_objects.objects()[&id].world_coord(),
                    native_coord(&actor["location"]),
                    "{name}: constructed Terrain coordinates"
                );
                if row["input"]["lower_tree_ground"] == true {
                    world
                        .resolved_terrain
                        .as_mut()
                        .unwrap()
                        .cell_mut_for_test(cell.0, cell.1)
                        .unwrap()
                        .level = 0;
                }
            }
        }
        world.substrate.occupancy = OccupancyGrid::new();
        // The native fixture supplies explicit head->next lists. Feed that
        // list through the existing prepend owner, without relying on IDs.
        for actor in row["actors"].as_array().unwrap().iter().rev() {
            if actor["native_class"] == "Terrain" {
                continue;
            }
            if row["input"]["refused"] == true && actor["name"] == "victim" {
                continue;
            }
            let id = ids[actor["name"].as_str().unwrap()];
            world.substrate.occupancy.add(
                actor["cell"][0].as_u64().unwrap() as u16,
                actor["cell"][1].as_u64().unwrap() as u16,
                id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        world.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            row["native_id_before"].as_u64().unwrap() as u32,
        ));
        world.main_rng = MainRng::new(seed);
        world.scenario_rng = SimRng::new(seed + 1);
        world.mapgen_rng = SimRng::new(seed + 2);
        assert_eq!(
            world.scenario_rng.native_state_hex(),
            row["rng_before"]["scenario"],
            "{name}: initial Scenario"
        );
        assert_eq!(
            world.main_rng.native_state_hex(),
            row["rng_before"]["main"],
            "{name}: initial Main"
        );
        let parent_weapon = rules
            .weapon(row["input"]["parent_weapon"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            rules.general.gravity,
            row["effective"]["gravity"].as_i64().unwrap() as i32,
            "{name}: original AudioVisual Gravity reader"
        );
        let source_id = if row["input"]["detached"] == true {
            super::RAD_NO_ATTACKER
        } else {
            ids["source"]
        };
        let detonation = ProjectileDetonation {
            projectile_id: 999,
            source_id,
            target: ProjectileTarget::Entity(ids["victim"]),
            impact: native_coord(&row["origin"]),
            payload: super::combat_tests::payload_from_weapon(
                parent_weapon.damage,
                world
                    .interner
                    .intern(parent_weapon.warhead.as_deref().unwrap()),
                world.interner.intern(&parent_weapon.id),
                &rules,
                &mut world.interner,
            ),
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        super::emit_projectile_shrapnel(
            &mut world,
            rules,
            &detonation,
            native_coord(&row["origin"]),
        );
        let children: Vec<_> = world.projectiles.iter().map(|(_, p)| p).collect();
        assert_eq!(
            children.len(),
            row["children"].as_array().unwrap().len(),
            "{name}"
        );
        for (actual, expected) in children.iter().zip(row["children"].as_array().unwrap()) {
            let target = match expected["target"]["kind"].as_str().unwrap() {
                "entity" => {
                    ProjectileTarget::Entity(ids[expected["target"]["name"].as_str().unwrap()])
                }
                "terrain" => {
                    ProjectileTarget::Terrain(ids[expected["target"]["name"].as_str().unwrap()])
                }
                "cell" => ProjectileTarget::Cell {
                    rx: expected["target"]["cell"][0].as_u64().unwrap() as u16,
                    ry: expected["target"]["cell"][1].as_u64().unwrap() as u16,
                },
                kind => panic!("unhandled native target {kind}"),
            };
            assert_eq!(actual.target(), target, "{name}");
            if !expected["retained_target"].is_null() {
                assert_eq!(
                    expected["retained_target"], expected["target"],
                    "{name}: native Fire preserves its supplied target identity"
                );
            }
            assert_eq!(actual.source_id(), source_id, "{name}");
            assert_eq!(
                actual.native_unique_id(),
                expected["native_id"].as_i64().unwrap() as i32,
                "{name}"
            );
            assert_eq!(
                actual.launch_origin(),
                native_coord(&expected["fire_location"]),
                "{name}"
            );
            assert_eq!(
                actual.launch_target(),
                native_coord(&expected["target_location"]),
                "{name}"
            );
            assert_eq!(
                actual.position(),
                native_coord(&expected["placed_location"]),
                "{name}"
            );
            assert_eq!(
                actual.payload().base_damage,
                expected["damage"].as_i64().unwrap() as i32,
                "{name}"
            );
            assert_eq!(
                world.interner.resolve(actual.payload().warhead),
                expected["warhead"].as_str().unwrap(),
                "{name}"
            );
            let child_weapon = rules
                .weapon(row["effective"]["child_weapon"].as_str().unwrap())
                .unwrap();
            assert_eq!(
                child_weapon.speed,
                expected["speed"].as_i64().unwrap() as i32,
                "{name}: processed weapon speed"
            );
            assert_eq!(
                world
                    .interner
                    .resolve(actual.payload().projectile_type().unwrap()),
                child_weapon.projectile.as_deref().unwrap(),
                "{name}"
            );
            assert_eq!(
                actual.payload().installed_weapon(),
                None,
                "{name}: Construct does not SetWeapon"
            );
            assert_eq!(
                actual.payload().bright(),
                expected["bright"] == 1,
                "{name}: explicit Construct Bright"
            );
            assert_eq!(
                child_weapon.projectile.as_deref(),
                expected["projectile"].as_str(),
                "{name}"
            );
            let launch = crate::sim::projectile::launch::shrapnel_launch_velocity(
                native_coord(&expected["launch_location"]),
                native_coord(&expected["target_location"]),
                child_weapon.speed,
                expected["target"]["kind"] == "cell",
            );
            for (component, bits) in launch
                .native()
                .iter()
                .zip(expected["launch_velocity"]["bits"].as_array().unwrap())
            {
                assert_eq!(
                    component.bits(),
                    u64::from_str_radix(bits.as_str().unwrap(), 16).unwrap(),
                    "{name}: original pre-Fire vector"
                );
            }
            assert_eq!(
                actual.arm_timer().start_frame(),
                expected["arm"][0].as_i64().unwrap() as i32,
                "{name}"
            );
            assert_eq!(
                actual.arm_timer().duration(),
                expected["arm"][1].as_i64().unwrap() as i32,
                "{name}"
            );
            assert!(
                actual.in_logic_vector(),
                "{name}: original child is registered"
            );
            assert!(!actual.on_bridge(), "{name}");
            // Existing Fire's zero-vector representation canonicalizes the
            // sign of zero; compare numeric values, not +0/-0 storage bits.
            for (component, expected) in actual
                .velocity()
                .native()
                .iter()
                .zip(expected["fired_velocity"]["value"].as_array().unwrap())
            {
                assert_eq!(
                    f64::from_bits(component.bits()),
                    expected.as_f64().unwrap(),
                    "{name}"
                );
            }
        }
        let beams: Vec<_> = world
            .lifecycle_outputs
            .iter()
            .filter_map(|e| {
                if let LifecycleOutput::LaserCreated(b) = e {
                    Some(b)
                } else {
                    None
                }
            })
            .collect();
        let child_ids: Vec<_> = children.iter().map(|child| child.id()).collect();
        let logic_order: Vec<_> = world
            .substrate
            .logic
            .as_slice()
            .iter()
            .filter_map(|id| child_ids.iter().position(|child_id| child_id == id))
            .collect();
        let native_logic_order: Vec<_> = row["logic_child_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| index.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(
            logic_order, native_logic_order,
            "{name}: original Logic append order"
        );
        assert_eq!(
            beams.len(),
            row["lasers"].as_array().unwrap().len(),
            "{name}"
        );
        for (actual, expected) in beams.iter().zip(row["lasers"].as_array().unwrap()) {
            assert_eq!(actual.from, native_coord(&expected["source"]), "{name}");
            assert_eq!(actual.to, native_coord(&expected["target"]), "{name}");
            assert_eq!(
                actual.width,
                expected["width"].as_i64().unwrap() as i32,
                "{name}"
            );
            assert_eq!(
                actual.duration,
                expected["duration"].as_i64().unwrap() as i32,
                "{name}"
            );
            assert_eq!(actual.supported, expected["supported"] == 1, "{name}");
            assert_eq!(
                actual.z_adjust,
                expected["z_adjust"].as_i64().unwrap() as i32,
                "{name}"
            );
            assert_eq!(
                actual.color,
                super::laser::LaserColor::House(owners[0]),
                "{name}: native fixture supplies owner color"
            );
        }
        assert_eq!(
            world.native_unique_ids.as_ref().unwrap().current_raw(),
            row["native_id_after"].as_u64().unwrap() as u32,
            "{name}"
        );
        assert_eq!(
            world.scenario_rng.native_state_hex(),
            row["rng_after"]["scenario"],
            "{name}: Scenario"
        );
        assert_eq!(
            world.main_rng.native_state_hex(),
            row["rng_after"]["main"],
            "{name}: Main"
        );
        assert_eq!(
            world.mapgen_rng.native_state_hex(),
            row["rng_after"]["mapgen"],
            "{name}: MapGen"
        );
    }
}

#[test]
fn building_bounce_places_child_at_get_coords_and_beam_at_target_offset() {
    let Some(mut scene) = PrismScene::new(false, false) else {
        return;
    };
    let terrain = scene.world.resolved_terrain.take();
    let yard = scene
        .world
        .spawn_object("GAYARD", "Russians", 18, 15, 0, &scene.rules)
        .unwrap();
    scene.world.resolved_terrain = terrain;
    let target = scene.world.substrate.entities.get(yard).unwrap();
    let center = crate::sim::movement::ground_pose::object_get_coords(
        target,
        scene.world.resolved_terrain.as_ref(),
    );
    let center = crate::sim::projectile::ProjectileCoord::new(center.x, center.y, center.z);
    let endpoint = super::fire_coord::effect_target_coordinate(
        &scene.world,
        &scene.rules,
        ProjectileTarget::Entity(yard),
    )
    .unwrap();
    assert_ne!(
        center, endpoint,
        "retail yard TargetCoordOffset exercises the different target virtuals"
    );
    let parent = scene.launch();
    scene.world.lifecycle_outputs.clear_for_test();
    scene.detonate(parent);
    let child = scene
        .world
        .projectiles
        .iter()
        .map(|(_, p)| p)
        .find(|p| p.target() == ProjectileTarget::Entity(yard))
        .expect("yard participates in the rings");
    assert_eq!(child.position(), center);
    assert_eq!(child.launch_target(), center);
    assert!(
        scene
            .world
            .lifecycle_outputs
            .iter()
            .any(|e| matches!(e,LifecycleOutput::LaserCreated(b) if b.to == endpoint))
    );
}

#[test]
fn elite_bounces_recurse_only_when_children_get_their_logic_visits() {
    let Some(mut scene) = PrismScene::new(false, true) else {
        return;
    };
    let parent = scene.launch();
    scene.world.lifecycle_outputs.clear_for_test();
    scene.detonate(parent);
    let first_child = scene
        .world
        .projectiles
        .iter()
        .filter(|(id, _)| **id != parent)
        .map(|(&id, _)| id)
        .min()
        .unwrap();
    assert_eq!(scene.world.projectiles.len(), 6);
    assert!(
        scene
            .world
            .projectiles
            .iter()
            .filter(|(id, _)| **id != parent)
            .all(|(_, p)| scene
                .world
                .interner
                .resolve(p.payload().projectile_type().unwrap())
                == "SuperSmallCometP")
    );
    assert_eq!(
        scene
            .world
            .lifecycle_outputs
            .iter()
            .filter(|e| matches!(e, LifecycleOutput::LaserCreated(_)))
            .count(),
        5
    );
    for _ in 0..20 {
        scene
            .world
            .session
            .set_binary_frame_for_test(scene.world.session.binary_frame() + 1);
        scene
            .world
            .visit_combat_tail(first_child, &scene.rules, None, FrameEffects::default());
        scene.world.flush_pending_delete();
    }
    assert_eq!(
        scene.world.projectiles.len(),
        1,
        "all child generations retire; parent is deliberately outside this tail boundary"
    );
    assert!(scene.world.projectiles.get(parent).is_some());
    assert!(
        scene
            .world
            .lifecycle_outputs
            .iter()
            .filter(|e| matches!(e, LifecycleOutput::LaserCreated(_)))
            .count()
            > 5,
        "visited elite children emit ordinary Comet fragments"
    );
}

#[test]
fn stock_truck_death_clears_already_born_child_owners_but_preserves_beams() {
    let Some(mut scene) = PrismScene::at(true, false, 14) else {
        return;
    };
    scene
        .world
        .substrate
        .entities
        .get_mut(scene.source)
        .unwrap()
        .health
        .set_for_test(1);
    let owner = scene
        .world
        .substrate
        .entities
        .get(scene.source)
        .unwrap()
        .owner();
    let parent = scene.launch();
    scene.world.lifecycle_outputs.clear_for_test();
    scene.detonate(parent);
    assert!(
        !scene
            .world
            .substrate
            .entities
            .get(scene.source)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    let children: Vec<_> = scene
        .world
        .projectiles
        .iter()
        .filter(|(id, _)| **id != parent)
        .map(|(_, p)| p)
        .collect();
    assert_eq!(children.len(), 5);
    assert!(
        children
            .iter()
            .all(|p| p.source_id() == super::RAD_NO_ATTACKER)
    );
    let beams: Vec<_> = scene
        .world
        .lifecycle_outputs
        .iter()
        .filter_map(|e| {
            if let LifecycleOutput::LaserCreated(b) = e {
                Some(b)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(beams.len(), 5);
    assert!(
        beams
            .iter()
            .all(|b| b.color == super::laser::LaserColor::House(owner))
    );
}
