//! Stock Pillbox target admission through the concrete setter and its callers.
//! Original443B90 ->6F7780/6F7220 ->4CC310/4CC100/4CC360 rejects an
//! intervening GAWALL for Vulcan2/InvisibleLow/SA. The target Cell at the
//! endpoint remains admissible; the base setter clears burst3B8 on a changed
//! assignment to NULL. These controls use the production retail rule readers.

use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::combat::EntityDamageEvent;
use crate::sim::combat::world_receiver::{ReceiverRun, commit_entities};
use crate::sim::house_state::HouseState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::util::fixed_math::SimFixed;

struct WallScene {
    sim: Simulation,
    rules: RuleSet,
    registry: OverlayTypeRegistry,
    building: u64,
    target: u64,
}

impl WallScene {
    fn new(human: bool) -> Option<Self> {
        let retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
        let registry =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        let mut sim = Simulation::new();
        let mut terrain = test_flat_ground_grid(32);
        terrain.set_land_speed_rules_for_test(&rules.terrain_rules);
        sim.install_resolved_terrain_for_new_map(terrain);
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        sim.overlay_grid = Some(OverlayGrid::new(32, 32));
        for (name, side) in [("Americans", 0), ("Russians", 1)] {
            let owner = sim.intern(name);
            sim.houses
                .register(HouseState::new(owner, side, Some(owner), human, 10_000, 10));
        }
        let building = sim
            .spawn_object_with_overlay_registry("GAPILL", "Americans", 10, 20, 0, &rules, &registry)
            .expect("Pillbox on clear ground");
        let target = sim
            .spawn_object_with_overlay_registry("E2", "Russians", 14, 20, 0, &rules, &registry)
            .expect("enemy on clear ground");
        // Original controls supply a stopped Infantry at the Cell center.
        let enemy = sim.substrate.entities.get_mut(target).unwrap();
        enemy.position.set_sub_x_for_test(SimFixed::from_num(128));
        enemy.position.set_sub_y_for_test(SimFixed::from_num(128));
        enemy.position.set_exact_z_leptons_for_test(Some(0));
        sim.session.set_binary_frame_for_test(FRAME);
        set_mission(
            &mut sim,
            building,
            MissionId::from_known(MissionType::Guard),
            0,
        );
        Some(Self {
            sim,
            rules,
            registry,
            building,
            target,
        })
    }

    fn wall(&mut self, owner: &str, cell: (u16, u16)) {
        self.sim
            .spawn_object_with_overlay_registry(
                "GAWALL",
                owner,
                cell.0,
                cell.1,
                0,
                &self.rules,
                &self.registry,
            )
            .expect("stock wall placement");
        assert_eq!(
            self.sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            self.registry.id_for_name("GAWALL")
        );
    }

    fn old_target(&mut self) {
        let entity = self.sim.substrate.entities.get_mut(self.building).unwrap();
        entity
            .attack_target
            .set_for_test(Some(TargetKind::Entity(self.target)));
        for _ in 0..7 {
            entity.weapon_burst.advance();
        }
    }

    fn target(&self) -> Option<TargetKind> {
        self.sim
            .substrate
            .entities
            .get(self.building)
            .unwrap()
            .attack_target
            .target()
    }
}

#[test]
fn retail_pillbox_target_assignment_observes_wall_geometry() {
    for (wall_owner, endpoint) in [
        (None, false),
        (Some("Russians"), false),
        (Some("Americans"), false),
        (Some("Russians"), true),
    ] {
        let Some(mut scene) = WallScene::new(true) else {
            return;
        };
        if let Some(owner) = wall_owner {
            // Keep the endpoint wall separate from the Infantry's occupied Cell.
            scene.wall(owner, if endpoint { (13, 20) } else { (12, 20) });
        }
        scene.old_target();
        let requested = if endpoint {
            TargetKind::Cell(13, 20)
        } else {
            TargetKind::Entity(scene.target)
        };
        let before = scene
            .sim
            .substrate
            .entities
            .get(scene.building)
            .unwrap()
            .mission;
        let rng = (
            scene.sim.main_rng.logical_state(),
            scene.sim.scenario_rng.logical_state(),
            scene.sim.mapgen_rng.logical_state(),
        );
        scene
            .sim
            .assign_target_represented(
                scene.building,
                Some(requested),
                &scene.rules,
                Some(&scene.registry),
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        let blocked = wall_owner.is_some() && !endpoint;
        assert_eq!(
            scene.target(),
            (!blocked).then_some(requested),
            "{wall_owner:?} endpoint={endpoint}"
        );
        let entity = scene.sim.substrate.entities.get(scene.building).unwrap();
        assert_eq!(entity.weapon_burst.index(), if blocked { 0 } else { 7 });
        assert_eq!(
            entity.mission, before,
            "the setter does not change mission/timer state"
        );
        assert_eq!(
            (
                scene.sim.main_rng.logical_state(),
                scene.sim.scenario_rng.logical_state(),
                scene.sim.mapgen_rng.logical_state()
            ),
            rng,
            "the setter draws no RNG"
        );
    }
}

#[test]
fn a_player_attack_order_cannot_keep_a_pillbox_target_behind_a_wall() {
    let Some(mut scene) = WallScene::new(true) else {
        return;
    };
    scene.wall("Russians", (12, 20));
    scene.old_target();
    assert!(scene.sim.apply_command_with_overlays(
        "Americans",
        &Command::Attack {
            attacker_id: scene.building,
            target_id: scene.target,
        },
        &scene.rules,
        Some(&scene.registry),
        crate::sim::world::FrameEffects::default()
    ));
    assert_eq!(scene.target(), None);
    let entity = scene.sim.substrate.entities.get(scene.building).unwrap();
    assert_eq!(
        entity.mission.current(),
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(
        entity.mission.queued(),
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.building)
            .unwrap()
            .weapon_burst
            .index(),
        0
    );
    scene
        .sim
        .advance_tick(&[], Some(&scene.rules), None, Some(&scene.registry), 67);
    assert_eq!(
        scene.target(),
        None,
        "the following Logic visit cannot acquire through the wall"
    );
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.building)
            .unwrap()
            .mission
            .current(),
        MissionId::from_known(MissionType::Guard),
        "Attack's null-target arm returns to Guard on its first visit"
    );
}

#[test]
fn generic_retaliation_archives_and_overrides_before_wall_target_admission() {
    for blocked in [false, true] {
        let Some(mut scene) = WallScene::new(false) else {
            return;
        };
        scene.old_target();
        if blocked {
            scene.wall("Russians", (12, 20));
        }
        // Observe the generic wrapper directly: the later Building damage
        // response must not be allowed to mask an earlier incorrect assignment.
        assert!(scene.sim.override_mission_on_damage_response(
            scene.building,
            scene.target,
            &scene.rules,
            Some(&scene.registry),
            crate::sim::world::FrameEffects::default(),
        ));
        let entity = scene.sim.substrate.entities.get(scene.building).unwrap();
        assert_eq!(
            entity.mission.current(),
            MissionId::from_known(MissionType::Attack)
        );
        assert_eq!(
            entity.mission.suspended(),
            MissionId::from_known(MissionType::Guard)
        );
        assert_eq!(
            entity.suspended_attack_target.target(),
            Some(TargetKind::Entity(scene.target))
        );
        assert_eq!(
            entity.attack_target.target(),
            (!blocked).then_some(TargetKind::Entity(scene.target))
        );
        assert_eq!(entity.weapon_burst.index(), if blocked { 0 } else { 7 });
    }
}

#[test]
fn canonical_restore_rechecks_a_wall_added_after_the_target_was_archived() {
    let Some(mut scene) = WallScene::new(false) else {
        return;
    };
    scene.old_target();
    assert!(scene.sim.override_mission_on_damage_response(
        scene.building,
        scene.target,
        &scene.rules,
        Some(&scene.registry),
        crate::sim::world::FrameEffects::default(),
    ));
    scene.wall("Russians", (12, 20));
    assert!(
        scene
            .sim
            .mission_restore_represented(
                scene.building,
                &scene.rules,
                Some(&scene.registry),
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap()
    );
    assert_eq!(scene.target(), None);
    let entity = scene.sim.substrate.entities.get(scene.building).unwrap();
    assert_eq!(entity.weapon_burst.index(), 0);
    assert_eq!(
        entity.mission.current(),
        MissionId::from_known(MissionType::Guard)
    );
}

#[test]
fn damage_retaliation_keeps_wall_geometry_through_the_mission_override() {
    let Some(mut scene) = WallScene::new(false) else {
        return;
    };
    scene.wall("Russians", (12, 20));
    let owner = scene
        .sim
        .substrate
        .entities
        .get(scene.target)
        .unwrap()
        .owner();
    let warhead = scene.sim.intern("SA");
    let hit = EntityDamageEvent::area(scene.building, 10, 0, scene.target, Some(owner), warhead);
    commit_entities(
        &mut scene.sim,
        &mut ReceiverRun::default(),
        &[hit],
        None,
        &scene.rules,
        Some(&scene.registry),
    );
    assert_eq!(
        scene.target(),
        None,
        "generic retaliation must use the concrete wall-aware setter"
    );
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.building)
            .unwrap()
            .mission
            .current(),
        MissionId::from_known(MissionType::Attack),
        "the override itself still happens"
    );
}
