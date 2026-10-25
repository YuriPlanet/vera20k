//! Retail IFV shots across the FireAt → first live Bullet AI boundary.
//! Naval placement is admitted explicitly on the synthetic clear arena;
//! production firing, guidance, impact and retirement owners run unchanged.

use super::{CombatEmit, TargetKind, build_attacker_snapshot, combat_weapon, world_fire};
use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::{ProjectileCoord, ProjectileTarget};
use crate::sim::world::{FrameEffects, Simulation};

struct Scene {
    world: Simulation,
    rules: RuleSet,
    source: u64,
    target: u64,
}

impl Scene {
    fn new(kind: &str) -> Option<Self> {
        let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        let mut world = crate::sim::arena_fixture::rival_arena(&rules, 31);
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        let source = world
            .spawn_object("FV", "Americans", 9, 16, 64, &rules)
            .unwrap();
        // This fixture starts after naval placement, on the same production
        // geometry used by the real FireAt and Bullet AI owners below.
        let terrain = world.resolved_terrain.take();
        let target = world
            .spawn_object(kind, "Russians", 16, 16, 0, &rules)
            .unwrap();
        world.resolved_terrain = terrain;
        world
            .assign_target_represented(
                source,
                Some(TargetKind::Entity(target)),
                &rules,
                None,
                FrameEffects::default(),
            )
            .unwrap();
        Some(Self {
            world,
            rules,
            source,
            target,
        })
    }

    fn centre(&self) -> ProjectileCoord {
        let point = crate::sim::movement::ground_pose::object_get_coords(
            self.world.substrate.entities.get(self.target).unwrap(),
            self.world.resolved_terrain.as_ref(),
        );
        ProjectileCoord::new(point.x, point.y, point.z)
    }

    fn launch(&mut self) -> u64 {
        self.world.session.set_binary_frame_for_test(100);
        let actor = self.world.substrate.entities.get(self.source).unwrap();
        let object = self.rules.object("FV").unwrap();
        let selected = combat_weapon::resolve_weapon_index(
            &self.rules,
            actor,
            object,
            0,
            &self.world.substrate.entities,
            &self.world.interner,
        )
        .unwrap();
        assert_eq!(selected.weapon.id, "HoverMissile");
        let target = self.world.substrate.entities.get(self.target).unwrap();
        let shot = world_fire::AdmittedFire {
            snap: build_attacker_snapshot(actor, TargetKind::Entity(self.target), None),
            obj: object,
            selected,
            target_coords: super::target_coords(target),
            target_type_ref: target.type_ref(),
            is_garrison: false,
        };
        let id = world_fire::emit_admitted_fire(
            &mut self.world,
            &self.rules,
            shot,
            100,
            &mut CombatEmit::default(),
            None,
            FrameEffects::default(),
        )
        .unwrap();
        let bullet = self.world.projectiles.get(id).unwrap();
        assert_eq!(bullet.guidance().unwrap().rot, 60);
        assert_eq!(bullet.target(), ProjectileTarget::Entity(self.target));
        assert_eq!(bullet.launch_target(), self.centre());
        assert_eq!(bullet.proximity_reference(), self.centre());
        assert_eq!(bullet.last_target_position(), self.centre());
        id
    }

    fn visit(&mut self, id: u64) {
        self.world
            .session
            .set_binary_frame_for_test(self.world.session.binary_frame() + 1);
        self.world
            .visit_combat_tail(id, &self.rules, None, FrameEffects::default());
    }
}

fn native_offset(kind: &str) -> [i32; 3] {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/shipyard_offsets.json",
    ))
    .unwrap();
    std::array::from_fn(|i| {
        native["sources"][kind]["reader"]["after"]["target_offset"][i]
            .as_i64()
            .unwrap() as i32
    })
}

// Bullet AI466B5D..466BAB replaces +58 with Object+A4 before HomingTrack.
// Building4500A0 contributes the type offset observed by the native corpus.
#[test]
fn retail_ifv_first_logic_visit_uses_live_shipyard_target_offset() {
    for kind in ["GAYARD", "NAYARD", "YAYARD"] {
        let Some(mut scene) = Scene::new(kind) else {
            return;
        };
        let offset = native_offset(kind);
        assert_eq!(
            scene.rules.object(kind).unwrap().target_coord_offset(),
            offset
        );
        let centre = scene.centre();
        let id = scene.launch();
        scene.visit(id);
        let bullet = scene.world.projectiles.get(id).unwrap();
        assert!(
            bullet.in_logic_vector(),
            "first guided visit is nonterminal"
        );
        assert_eq!(
            bullet.last_target_position(),
            ProjectileCoord::new(
                centre.x + offset[0],
                centre.y + offset[1],
                centre.z + offset[2]
            ),
            "{kind}: live guidance must use+A4"
        );
        assert_eq!(bullet.launch_target(), centre);
        assert_eq!(bullet.proximity_reference(), centre);
        assert_eq!(bullet.target(), ProjectileTarget::Entity(scene.target));
    }
}

// Nonbuildings and zero-offset buildings retain their ordinary coordinates;
// a subsequent AI visit reads the live target instead of capturing+A4 once.
#[test]
fn live_guidance_refreshes_offsets_and_preserves_zero_offset_targets() {
    for kind in ["GAYARD", "GAPOWR", "MTNK"] {
        let Some(mut scene) = Scene::new(kind) else {
            return;
        };
        let offset = if kind == "GAYARD" {
            native_offset(kind)
        } else {
            [0; 3]
        };
        assert_eq!(
            scene.rules.object(kind).unwrap().target_coord_offset(),
            offset
        );
        let initial = scene.centre();
        let id = scene.launch();
        scene.visit(id);
        assert_eq!(
            scene
                .world
                .projectiles
                .get(id)
                .unwrap()
                .last_target_position(),
            ProjectileCoord::new(
                initial.x + offset[0],
                initial.y + offset[1],
                initial.z + offset[2]
            )
        );
        // Supplied target movement within its cell isolates the query; the
        // moving-target locomotor and placement transaction are not under test.
        let target = scene
            .world
            .substrate
            .entities
            .get_mut(scene.target)
            .unwrap();
        let x = target.position.sub_x();
        target
            .position
            .set_sub_x_for_test(x + crate::util::fixed_math::SimFixed::from_num(32));
        let moved = scene.centre();
        assert_eq!(moved.x, initial.x + 32);
        scene.visit(id);
        let bullet = scene.world.projectiles.get(id).unwrap();
        assert_eq!(
            bullet.last_target_position(),
            ProjectileCoord::new(
                moved.x + offset[0],
                moved.y + offset[1],
                moved.z + offset[2]
            )
        );
        assert_eq!(bullet.launch_target(), initial);
        assert_eq!(bullet.proximity_reference(), initial);
    }
}

// AI467CA9..467E4D selects target+48 for a mode-one fuse, even though live
// steering used+A4. The independent impact ladder468D80 can then select+A4
// for damage. Observe the retained pre-ladder Bullet Location before delete.
#[test]
fn guided_mode_one_snap_keeps_get_coords_before_the_impact_ladder() {
    let Some(mut scene) = Scene::new("GAYARD") else {
        return;
    };
    let centre = scene.centre();
    let offset = native_offset("GAYARD");
    let id = scene.launch();
    // A supplied in-flight state isolates the established mode-one boundary
    // (same fixture policy as the Cell bridge-snap test). A one-lepton
    // step stays within 64 leptons of this reference and away from ground.
    let position = ProjectileCoord::new(centre.x - 1500, centre.y, centre.z + 1000);
    {
        let bullet = scene.world.projectiles.get_mut(id).unwrap();
        bullet.set_position_for_test(position);
        bullet.set_velocity_for_test(crate::sim::projectile::ProjectileVelocity::new(1, 0, 0));
        bullet.set_speed_leptons_per_frame_for_test(1);
        bullet.set_proximity_reference_for_test(position);
        bullet.arm_timer_mut_for_test().start(100, 0);
        let guidance = bullet.guidance_mut_for_test().as_mut().unwrap();
        guidance.max_speed = 1;
        guidance.acceleration = 0;
        guidance.course_locked = true;
        guidance.course_frames = 0;
    }
    let health = scene
        .world
        .substrate
        .entities
        .get(scene.target)
        .unwrap()
        .health
        .current();
    scene.visit(id);
    let bullet = scene
        .world
        .projectiles
        .get(id)
        .expect("late delete retains the fired Bullet");
    assert!(
        !bullet.in_logic_vector(),
        "mode-one detonation retires this visit"
    );
    assert_eq!(
        bullet.last_target_position(),
        ProjectileCoord::new(
            centre.x + offset[0],
            centre.y + offset[1],
            centre.z + offset[2]
        )
    );
    assert_eq!(
        bullet.position(),
        centre,
        "final fuse snap uses+48, not the+A4 live guidance point"
    );
    assert!(
        scene
            .world
            .substrate
            .entities
            .get(scene.target)
            .unwrap()
            .health
            .current()
            < health,
        "the normal damage/impact receiver runs after the snap"
    );
    scene.world.flush_pending_delete();
    assert!(scene.world.projectiles.get(id).is_none());
}
