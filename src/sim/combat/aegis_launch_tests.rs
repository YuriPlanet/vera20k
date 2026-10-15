//! Retail Aegis shots through the production FireAt owner. Launch fixtures
//! begin after admission; scalable enrollment also exercises UnitTarget's
//! actual admission. Release captures cover commands and the scheduler.
//! Native launch-vector comparisons
//! live in projectile::launch's radial_launch.json consumer.

use super::{CombatEmit, TargetKind, build_attacker_snapshot, combat_weapon, world_fire};
use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::{Projectile, ProjectileVelocity};
use crate::sim::world::{FrameEffects, Simulation};

struct AegisScene {
    world: Simulation,
    rules: RuleSet,
    aegis: u64,
    target: u64,
}

impl AegisScene {
    fn new(elite: bool) -> Option<Self> {
        let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        let mut world = crate::sim::arena_fixture::rival_arena(&rules, 31);
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        // This component fixture starts with admitted actors. The all-land
        // arena is not a ship-placement test; retain its production cells for
        // FireAt's coordinates and projectile registration after construction.
        let terrain = world.resolved_terrain.take();
        let aegis = world
            .spawn_object("AEGIS", "Americans", 10, 20, 0, &rules)
            .expect("admitted retail Aegis");
        world.resolved_terrain = terrain;
        let target = world
            .spawn_object("ZEP", "Russians", 15, 20, 0, &rules)
            .expect("retail airborne target");
        world
            .substrate
            .entities
            .get_mut(target)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(700));
        if elite {
            world
                .substrate
                .entities
                .get_mut(aegis)
                .unwrap()
                .veterancy
                .set_elite();
        }
        assert!(
            world
                .assign_target_represented(
                    aegis,
                    Some(TargetKind::Entity(target)),
                    Some(&rules),
                    None,
                    FrameEffects::default(),
                )
                .is_ok()
        );
        // These launch/burst controls retain an explicit Attack target. Guard
        // DistributedFire's history/null setter is exercised by the joined
        // world::aegis_distributed_fire_tests production frame fixture.
        world
            .mission_assign_exact(
                aegis,
                crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Attack,
                ),
                world.session.binary_frame,
            )
            .unwrap();
        Some(Self {
            world,
            rules,
            aegis,
            target,
        })
    }

    fn radial_index(&self) -> i32 {
        self.world
            .substrate
            .entities
            .get(self.aegis)
            .unwrap()
            .weapon_burst
            .radial_index()
    }

    fn burst_index(&self) -> i32 {
        self.world
            .substrate
            .entities
            .get(self.aegis)
            .unwrap()
            .weapon_burst
            .index()
    }

    fn fire(&mut self, frame: u32) -> Projectile {
        self.world.session.binary_frame = frame;
        let actor = self.world.substrate.entities.get(self.aegis).unwrap();
        let object = self.rules.object("AEGIS").unwrap();
        let selected = combat_weapon::resolve_weapon_index(
            &self.rules,
            actor,
            object,
            0,
            &self.world.substrate.entities,
            &self.world.interner,
        )
        .expect("retail normal/elite primary");
        let expected_weapon =
            if actor.veterancy.rank_kind() == super::veterancy::VeterancyRank::Elite {
                "MedusaE"
            } else {
                "Medusa"
            };
        assert_eq!(selected.weapon.id, expected_weapon);
        let target = self.world.substrate.entities.get(self.target).unwrap();
        let shot = world_fire::AdmittedFire {
            snap: build_attacker_snapshot(actor, TargetKind::Entity(self.target), None),
            obj: object,
            selected,
            target_coords: (
                target.position.rx(),
                target.position.ry(),
                target.position.sub_x(),
                target.position.sub_y(),
            ),
            target_type_ref: target.type_ref(),
            is_garrison: false,
        };
        let mut emitted = CombatEmit::default();
        let id = world_fire::emit_admitted_fire(
            &mut self.world,
            &self.rules,
            shot,
            frame,
            &mut emitted,
            None,
            FrameEffects::default(),
        )
        .expect("admitted retail Medusa creates a Bullet");
        assert_eq!(emitted.fire_events.len(), 1);
        let bullet = self.world.projectiles.get(id).unwrap();
        assert_eq!(
            self.world
                .interner
                .resolve(bullet.payload.installed_weapon().unwrap()),
            expected_weapon
        );
        assert_eq!(bullet.source_id, self.aegis);
        assert_eq!(
            bullet.target,
            crate::sim::projectile::ProjectileTarget::Entity(self.target)
        );
        assert_eq!(bullet.guidance.unwrap().rot, 20);
        assert_eq!(bullet.guidance.unwrap().course_lock_duration, 15);
        bullet.clone()
    }
}

/// Techno6FEADA..6FEBDC consumes +43C per actual FireAt call, independently
/// of CurrentBurstIndex. The elite retail Burst=2 still visits two successive
/// fan directions, rather than incrementing once per volley.
#[test]
fn retail_normal_and_elite_medusa_advance_the_fan_per_actual_shot() {
    for elite in [false, true] {
        let Some(mut scene) = AegisScene::new(elite) else {
            return;
        };
        assert_eq!(
            scene.rules.object("AEGIS").unwrap().radial_fire_segments,
            10
        );
        let mut velocities: Vec<ProjectileVelocity> = Vec::new();
        for shot in 0..12 {
            assert_eq!(scene.radial_index(), shot % 10);
            assert_eq!(scene.burst_index(), if elite { shot % 2 } else { 0 });
            let bullet = scene.fire(100 + shot as u32 * 40);
            velocities.push(bullet.velocity);
            assert_eq!(scene.radial_index(), (shot + 1) % 10);
            assert_eq!(scene.burst_index(), if elite { (shot + 1) % 2 } else { 0 });
        }
        assert_eq!(
            velocities[0], velocities[10],
            "same fan position after ten shots"
        );
        assert_eq!(velocities[1], velocities[11], "second position repeats too");
        for (index, velocity) in velocities[..10].iter().enumerate() {
            assert!(
                !velocities[..index].contains(velocity),
                "distinct fan position {index}"
            );
        }
    }
}

/// Assign_Target6FCF5B resets only the ordinary burst word on a changed null
/// target. +43C survives both replacement and clearing the target.
#[test]
fn target_replacement_and_null_preserve_the_next_radial_shot() {
    let Some(mut scene) = AegisScene::new(true) else {
        return;
    };
    scene.fire(100);
    assert_eq!((scene.radial_index(), scene.burst_index()), (1, 1));
    let other = scene
        .world
        .spawn_object("ZEP", "Russians", 15, 21, 0, &scene.rules)
        .unwrap();
    assert!(
        scene
            .world
            .assign_target_represented(
                scene.aegis,
                Some(TargetKind::Entity(other)),
                Some(&scene.rules),
                None,
                FrameEffects::default(),
            )
            .is_ok()
    );
    assert_eq!((scene.radial_index(), scene.burst_index()), (1, 1));
    assert!(
        scene
            .world
            .assign_target_represented(
                scene.aegis,
                None,
                Some(&scene.rules),
                None,
                FrameEffects::default(),
            )
            .is_ok()
    );
    assert_eq!((scene.radial_index(), scene.burst_index()), (1, 0));
    assert!(
        scene
            .world
            .assign_target_represented(
                scene.aegis,
                Some(TargetKind::Entity(scene.target)),
                Some(&scene.rules),
                None,
                FrameEffects::default(),
            )
            .is_ok()
    );
    scene.fire(140);
    assert_eq!((scene.radial_index(), scene.burst_index()), (2, 1));
}

/// Unit's native Size746DD0 includes +43C and Load70BF50 does not reset it.
/// The two histories below continue from the same save with real emitted
/// bullets; the deliberately canonical RNG accounts for retail's load reset.
#[test]
fn mid_fan_snapshot_retains_counter_hash_and_continued_shots() {
    let Some(mut scene) = AegisScene::new(true) else {
        return;
    };
    for shot in 0..5 {
        scene.fire(100 + shot * 40);
    }
    assert_eq!((scene.radial_index(), scene.burst_index()), (5, 1));
    scene.world.scenario_rng = crate::sim::rng::SimRng::new(0);
    let terrain = scene.world.resolved_terrain.as_ref().unwrap().clone();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&scene.world, 0, 0, "aegis", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    loaded.restore_after_snapshot_load().unwrap();
    loaded.resolve_type_handles(&scene.rules);
    loaded.install_resolved_terrain_for_new_map(terrain);
    assert!(loaded.rebuild_dynamic_navigation(&scene.rules));
    let mut restored = AegisScene {
        world: loaded,
        rules: crate::rules::retail_ini_fixture::retail_battle_rules()
            .expect("same retail rule sources")
            .rules,
        aegis: scene.aegis,
        target: scene.target,
    };
    assert_eq!((restored.radial_index(), restored.burst_index()), (5, 1));
    // BulletLoad46AE9C..46AEB0 restarts both timers at the loaded frame;
    // C4/CC is represented by arm_timer. Compare against that native load
    // result, then apply the same documented reset to the reference history.
    let loaded_frame = scene.world.session.binary_frame as i32;
    for (id, before) in scene.world.projectiles.iter_mut() {
        let after = restored.world.projectiles.get(*id).unwrap();
        assert_eq!(after.arm_timer.start_frame(), loaded_frame);
        assert_eq!(after.arm_timer.duration(), 0);
        before.arm_timer.start(loaded_frame, 0);
        assert_eq!(
            before, after,
            "BulletLoad changes only the represented timer for bullet {id}"
        );
    }
    // LoadContent67E739 clears the old scalable vector; BulletLoad46AE70
    // does not reenroll. Raw BulletType+2E8 survives its46C6A0 load.
    assert!(restored.world.projectiles.scalable_members().is_empty());
    let kind = scene.world.interner.get("MedusaProjectile").unwrap();
    assert_eq!(
        scene.world.projectiles.scaled_spawn_delay(kind),
        restored.world.projectiles.scaled_spawn_delay(kind)
    );
    scene.world.projectiles.restore_scalable_after_load();
    assert_eq!(scene.world.state_hash(), restored.world.state_hash());
    let hash = restored.world.state_hash();
    let saved_burst = restored
        .world
        .substrate
        .entities
        .get(restored.aegis)
        .unwrap()
        .weapon_burst;
    restored
        .world
        .substrate
        .entities
        .get_mut(restored.aegis)
        .unwrap()
        .weapon_burst
        .take_radial_index(10);
    assert_ne!(
        hash,
        restored.world.state_hash(),
        "independent radial state participates in hash"
    );
    restored
        .world
        .substrate
        .entities
        .get_mut(restored.aegis)
        .unwrap()
        .weapon_burst = saved_burst;
    assert_eq!(hash, restored.world.state_hash());
    for shot in 5..12 {
        let expected = scene.fire(100 + shot * 40);
        let actual = restored.fire(100 + shot * 40);
        assert_eq!(actual, expected, "continued shot {shot}");
        assert_eq!(restored.world.state_hash(), scene.world.state_hash());
    }
}

/// Scalable's global bookkeeping affects the Trailer cadence (46C840).
/// Bullet AI466826 tests Trailer before reading the scaled delay466834;
/// the selected stock scalable types cannot observe it without a Trailer.
#[test]
fn retail_scalable_projectiles_have_no_trailer_and_medusa_keeps_its_line_trail() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let scalable: Vec<_> = fixture
        .rules
        .projectiles_iter()
        .filter(|kind| kind.scalable)
        .collect();
    assert!(
        !scalable.is_empty(),
        "retail scalable family must be present"
    );
    for kind in scalable {
        assert!(
            kind.trailer.is_none(),
            "{} observes scaled Trailer cadence",
            kind.id
        );
    }
    let medusa = fixture.rules.projectile("MedusaProjectile").unwrap();
    assert!(medusa.scalable);
    assert!(medusa.use_line_trail);
    assert!(medusa.trailer.is_none());
    assert_eq!(medusa.rot, 20);
    assert_eq!(medusa.course_lock_duration, 15);
    assert_eq!(medusa.arm, 1);
    assert_eq!(fixture.rules.weapon("Medusa").unwrap().burst, 1);
    assert_eq!(fixture.rules.weapon("MedusaE").unwrap().burst, 2);
}

/// The actual Unit FireAt wrapper enrolls both normal and elite Medusa
/// returns, despite their null Trailer. Elite Burst2 still counts each shot.
#[test]
fn retail_normal_and_elite_aegis_unit_fire_enrolls_each_returned_bullet() {
    for elite in [false, true] {
        let Some(mut scene) = AegisScene::new(elite) else {
            return;
        };
        assert!(
            scene
                .rules
                .weapon(if elite { "MedusaE" } else { "Medusa" })
                .unwrap()
                .omni_fire
        );
        let owner = scene
            .world
            .substrate
            .entities
            .get(scene.aegis)
            .unwrap()
            .owner();
        let victim = &scene
            .world
            .substrate
            .entities
            .get(scene.target)
            .unwrap()
            .position;
        scene
            .world
            .fog
            .mark_visible_for_owner(owner, victim.rx(), victim.ry());
        for index in 0..8 {
            let before = scene.world.projectiles.len();
            let visit = if index == 0 {
                world_fire::FireVisit::UnitTarget(scene.aegis)
            } else {
                world_fire::FireVisit::Direct {
                    id: scene.aegis,
                    target: TargetKind::Entity(scene.target),
                    weapon_index: 0,
                }
            };
            scene
                .world
                .commit_fire_visit(visit, &scene.rules, None, FrameEffects::default());
            assert_eq!(scene.world.projectiles.len(), before + 1);
        }
        let ids: Vec<_> = scene.world.projectiles.iter().map(|(&id, _)| id).collect();
        assert_eq!(scene.world.projectiles.scalable_members(), ids);
        let type_id = scene.world.interner.get("MedusaProjectile").unwrap();
        let kind = scene.rules.projectile("MedusaProjectile").unwrap();
        assert!(kind.scalable);
        assert_eq!(kind.trailer, None);
        assert_eq!(scene.world.projectiles.scaled_spawn_delay(type_id), 3);
        // No Trailer means no cadence IDIV or Anim allocation at the header;
        // the separate retained line trail remains the existing visual owner.
        let bullet = ids[0];
        let head = scene
            .world
            .projectiles
            .begin_ai_visit(bullet, |_| false)
            .unwrap();
        assert!(scene.world.projectiles.trailer(&head, kind, 3).is_none());
    }
}
