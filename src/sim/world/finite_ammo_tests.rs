//! Registered Scenario ammo overrides with physical retail weapons/art/assets.
//! Clear arena, human roster, visibility and pre-Logic semantic command delivery
//! are supplied boundaries; these are not original whole-frame or input goldens.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::combat::fire_error::FireError;
use crate::sim::combat::fire_error_world::FireSubject;
use crate::sim::command::Command;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::timer::CdTimer;

struct Scene(SimRuntime);

impl Scene {
    fn new(patch: &str) -> Option<Self> {
        let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &IniFile::from_str(patch),
        )?;
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
        retail
            .rules
            .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
        retail
            .rules
            .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
        let registry =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xa220);
        // The supplied clear map still owns its empty overlay cells and saved
        // wall-neighbor plane; full map-authority restore requires both.
        sim.install_overlay_grid_for_load(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        for name in ["Americans", "Russians"] {
            let owner = sim.interner.get(name).unwrap();
            let house = sim.houses.get_mut(&owner).unwrap();
            house.initialize_scenario_name(name);
            house.set_is_human_for_test(true);
            house.set_current_iq_for_test(0);
        }
        let owner = sim.interner.get("Americans").unwrap();
        sim.session.set_current_house_for_test(Some(owner));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session.game_options_mut_for_test().short_game = false;
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        for name in ["Americans", "Russians"] {
            sim.fog
                .reveal_all_for_owner(sim.interner.get(name).unwrap());
        }
        let terrain_template = sim.resolved_terrain().cloned();
        let mut scene = Self(SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                overlay_registry: registry,
                terrain_template,
            },
        });
        scene.spawn("GAPOWR", "Americans", (4, 24));
        scene.spawn("NAPOWR", "Russians", (28, 28));
        // Assess real reactor output before constructing any Powered firer.
        scene.frame();
        Some(scene)
    }

    fn spawn(&mut self, kind: &str, owner: &str, cell: (u16, u16)) -> u64 {
        self.0
            .simulation
            .spawn_object_with_overlay_registry(
                kind,
                owner,
                cell.0,
                cell.1,
                0,
                &self.0.resources.rules,
                &self.0.resources.overlay_registry,
            )
            .expect("retail constructor and ordinary Unlimbo")
    }

    fn actor(&self, id: u64) -> &crate::sim::game_entity::GameEntity {
        self.0.simulation.entities().get(id).expect("live actor")
    }

    fn count(&self, id: u64) -> i32 {
        self.actor(id)
            .ammo()
            .expect("constructed Techno ammo")
            .current()
    }

    fn reload(&self, id: u64) -> CdTimer {
        // Observe the actual serialized clock without depending on a new API:
        // the same test source compiles against the pre-fix production tree.
        let entity = serde_json::to_value(self.actor(id)).unwrap();
        serde_json::from_value(entity["ammo_reload_timer"].clone())
            .expect("authoritative saved reload clock")
    }

    fn now(&self) -> i32 {
        self.0.simulation.session.binary_frame() as i32
    }

    fn command(&mut self, owner: &str, command: Command) {
        // The existing receiver is the supplied delivery boundary. No authored
        // target, mission, facing, position, ammunition or timer state follows.
        assert!(self.0.apply_command_for_test(owner, &command));
    }

    fn frame(&mut self) -> SimFrameOutput {
        let commands = self.0.simulation.take_due_commands();
        let output = self
            .0
            .advance_frame(&commands, 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(output.tick.frame_committed);
        output
    }

    fn error(&self, id: u64, target: u64) -> FireError {
        let sim = &self.0.simulation;
        let firer = self.actor(id);
        FireSubject {
            world: sim,
            rules: &self.0.resources.rules,
            overlay_registry: Some(&self.0.resources.overlay_registry),
            fog: Some(sim.fog.fire_view(sim.house_alliances.as_map())),
            firer,
            obj: sim
                .interner
                .object_type(firer.type_ref(), &self.0.resources.rules)
                .unwrap(),
            target: Some(TargetKind::Entity(target)),
            weapon_index: 0,
        }
        .fire_error(true)
    }

    fn shot(&mut self, source: u64, target: u64) -> i32 {
        for _ in 0..180 {
            let now = self.now();
            let output = self.frame();
            if let Some(shot) = output
                .fire_events
                .iter()
                .find(|shot| shot.attacker_id == source)
            {
                assert_eq!(shot.target, TargetKind::Entity(target));
                assert_eq!(shot.weapon_slot, WeaponSlot::Primary);
                let obj = self
                    .0
                    .simulation
                    .interner
                    .object_type(self.actor(source).type_ref(), &self.0.resources.rules)
                    .unwrap();
                assert_eq!(
                    self.0.simulation.interner.resolve(shot.weapon_id),
                    obj.primary().unwrap()
                );
                return now;
            }
        }
        panic!("ordinary target/facing/FireAt never produced a shot");
    }

    fn restored(&self, bytes: &[u8]) -> Simulation {
        let live = &self.0.simulation;
        let resources = &self.0.resources;
        let mut restored = GameSnapshot::load_validated(
            bytes,
            0, // supplied arena, not a saved physical-map hash claim
            resources.rules.simulation_config_hash(),
            live.session.map_name(),
        )
        .unwrap()
        .sim;
        // The real load owner reseeds Scenario RNG to zero. Preserve the live
        // process cursors through their owner; never overwrite Scenario RNG.
        assert_eq!(
            restored.scenario_rng.logical_state(),
            crate::sim::rng::SimRng::new(0).logical_state()
        );
        restored.retain_process_state_for_load(SnapshotProcessState::Live(live));
        restored.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
        restored.reconstruct_cellclass_dummy_for_map_resize();
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(
            resources.terrain_template.clone().unwrap(),
            live.terrain_speed_config.clone(),
            &resources.rules,
        );
        restored
            .restore_map_authority_after_snapshot_load(
                &resources.rules,
                &resources.overlay_registry,
                None,
            )
            .unwrap();
        restored.resolve_type_handles(&resources.rules);
        restored.rebuild_lighting_sources_after_load(&resources.rules);
        restored
    }
}

fn fired(output: &SimFrameOutput, source: u64) -> bool {
    output
        .fire_events
        .iter()
        .any(|shot| shot.attacker_id == source)
}

#[test]
fn finite_patriot_fires_a_rapid_pair_refills_zero_and_keeps_last_round_rearm() {
    let Some(mut scene) = Scene::new(
        "[NASAM]\nAmmo=2\nInitialAmmo=2\nCanPassiveAquire=no\n\
         [ORCA]\nSpeed=1\nStrength=1000\n",
    ) else {
        return;
    };
    let patriot = scene.0.resources.rules.object("NASAM").unwrap();
    assert_eq!((patriot.ammo, patriot.initial_ammo), (2, 2));
    assert_eq!(patriot.primary(), Some("RedEye2"));
    let plane = scene.spawn("ORCA", "Russians", (16, 16));
    // These legal target type controls isolate sustained real flight/survival;
    // this is not an unmodified-stock-aircraft engagement comparison.
    scene.command(
        "Russians",
        Command::Move {
            entity_id: plane,
            target_rx: 16,
            target_ry: 4,
        },
    );
    let mut high = false;
    for _ in 0..240 {
        scene.frame();
        high = crate::sim::movement::air_movement::is_high_flying(
            scene.actor(plane),
            scene.0.simulation.resolved_terrain(),
            Some((&scene.0.resources.rules, scene.0.simulation.interner())),
        );
        if high {
            break;
        }
    }
    assert!(high, "actual Aircraft Move must take off");
    let gun = scene.spawn("NASAM", "Americans", (16, 20));
    let [source, destination] = [gun, plane].map(|id| {
        let coord = crate::sim::movement::ground_pose::object_location(
            scene.actor(id),
            scene.0.simulation.resolved_terrain(),
        );
        [coord.x, coord.y, coord.z]
    });
    let range = scene
        .0
        .resources
        .rules
        .weapon("RedEye2")
        .unwrap()
        .range_leptons;
    assert!(crate::util::native_x87::distance_3d_leptons(source, destination) <= range);
    scene.command(
        "Americans",
        Command::Attack {
            attacker_id: gun,
            target_id: plane,
        },
    );
    let first = scene.shot(gun, plane);
    assert_eq!(
        scene.count(gun),
        1,
        "the actual first ordinary shot debits two to one"
    );
    assert_eq!(scene.actor(gun).rearm_timer.duration(), 1);
    let second = scene.now();
    let output = scene.frame();
    assert!(fired(&output, gun), "ROF1 admits the next ordinary visit");
    assert_eq!(second, first + 1);
    assert_eq!(
        scene.count(gun),
        2,
        "zero refills in the same outer Building visit"
    );
    let rearm = scene.actor(gun).rearm_timer.timer();
    assert_eq!(rearm.start_frame(), second);
    assert!((55..=57).contains(&rearm.duration()));
    while scene.now() < rearm.start_frame() + rearm.duration() {
        assert_eq!(scene.error(gun, plane), FireError::Rearm);
        assert!(
            !fired(&scene.frame(), gun),
            "refill must preserve the long cooldown"
        );
        assert_eq!(scene.count(gun), 2);
        assert_eq!(scene.actor(gun).rearm_timer.timer(), rearm);
        assert!(scene.actor(plane).is_object_alive());
    }
    let third = scene.shot(gun, plane);
    assert!(third >= rearm.start_frame() + rearm.duration());
    assert_eq!(scene.count(gun), 1);
}

#[test]
fn finite_unit_empty_reload_expires_after_fire_and_survives_actual_snapshot_restore() {
    let Some(mut scene) = Scene::new(
        "[LTNK]\nAmmo=1\nInitialAmmo=1\nReload=18\nEmptyReload=36\nCanPassiveAquire=no\n",
    ) else {
        return;
    };
    let target = scene.spawn("GACNST", "Russians", (12, 12));
    let tank = scene.spawn("LTNK", "Americans", (12, 16));
    assert_eq!(
        scene.count(tank),
        1,
        "full constructor count avoids prefire reload"
    );
    scene.command(
        "Americans",
        Command::Attack {
            attacker_id: tank,
            target_id: target,
        },
    );
    let shot = scene.shot(tank, target);
    assert_eq!(
        scene.count(tank),
        0,
        "the real returned Bullet empties the Unit"
    );
    let reload = scene.reload(tank);
    let rearm = scene.actor(tank).rearm_timer.timer();
    assert_eq!((reload.start_frame(), reload.duration()), (shot, 36));
    assert!((60..=62).contains(&rearm.duration()));
    for _ in 0..7 {
        assert!(!fired(&scene.frame(), tank));
        assert_eq!(scene.count(tank), 0);
        assert_eq!(scene.reload(tank), reload);
    }
    let bytes = GameSnapshot::save_validated(
        &scene.0.simulation,
        0,
        scene.0.resources.rules.simulation_config_hash(),
        "pending empty reload",
        0,
    );
    // Two admitted loads compare deterministic continuation after the real
    // Scenario reseed; no live-versus-loaded global RNG/hash identity is claimed.
    let mut other = scene.restored(&bytes);
    let loaded = scene.restored(&bytes);
    scene.0.replace_simulation(loaded);
    assert_eq!(scene.reload(tank), reload);
    assert_eq!(scene.actor(tank).rearm_timer.timer(), rearm);
    let mut saw_refill = false;
    let mut saw_next_shot = false;
    for _ in 0..80 {
        let now = scene.now();
        let output = scene.frame();
        let shot_again = fired(&output, tank);
        if now < shot + 36 {
            assert_eq!(scene.count(tank), 0);
        }
        if now == shot + 36 {
            assert!(!shot_again);
            assert_eq!(scene.count(tank), 1);
            assert!(rearm.remaining(now) > 0, "reload never clears weapon rearm");
            assert_eq!(scene.actor(tank).rearm_timer.timer(), rearm);
            saw_refill = true;
        }
        if shot_again {
            assert!(now >= rearm.start_frame() + rearm.duration());
            assert_eq!(scene.count(tank), 0);
            assert_eq!(scene.reload(tank).start_frame(), now);
            assert_eq!(scene.reload(tank).duration(), 36);
            saw_next_shot = true;
        }
        assert_eq!(
            scene.actor(tank).attack_target.target(),
            Some(TargetKind::Entity(target))
        );
        let observed = (
            scene.count(tank),
            scene.reload(tank),
            shot_again,
            scene.0.simulation.state_hash(),
        );
        std::mem::swap(&mut scene.0.simulation, &mut other);
        let output = scene.frame();
        assert_eq!(
            (
                scene.count(tank),
                scene.reload(tank),
                fired(&output, tank),
                scene.0.simulation.state_hash()
            ),
            observed,
            "saved count/clock/events/hash continuation at frame {now}"
        );
        std::mem::swap(&mut scene.0.simulation, &mut other);
    }
    assert!(saw_refill && saw_next_shot);
}

#[test]
fn moving_finite_unit_extends_its_quotient_reload_then_adds_only_one_round_at_a_time() {
    let Some(mut scene) = Scene::new(
        "[LTNK]\nAmmo=6\nInitialAmmo=6\nReload=60\nReloadIncrement=3\nPipWrap=2\n\
         MobileFire=no\nCanPassiveAquire=no\n",
    ) else {
        return;
    };
    let target = scene.spawn("GACNST", "Russians", (12, 12));
    let tank = scene.spawn("LTNK", "Americans", (12, 16));
    scene.command(
        "Americans",
        Command::Attack {
            attacker_id: tank,
            target_id: target,
        },
    );
    scene.shot(tank, target);
    assert_eq!(
        scene.count(tank),
        5,
        "actual first shot from a full magazine"
    );
    scene.shot(tank, target);
    assert_eq!(
        scene.count(tank),
        4,
        "second shot precedes the positive reload expiry"
    );
    assert_eq!(scene.reload(tank).duration(), 72, "60 + 3 * (4 / 2)^2");
    scene.command(
        "Americans",
        Command::Move {
            entity_id: tank,
            target_rx: 12,
            target_ry: 28,
        },
    );
    let mut moving_visits = 0;
    for _ in 0..80 {
        let before = scene.reload(tank);
        let now = scene.now();
        assert!(!fired(&scene.frame(), tank));
        assert_eq!(scene.count(tank), 4);
        if crate::sim::movement::motion_query::is_moving(scene.actor(tank)) == Some(true) {
            let after = scene.reload(tank);
            assert_eq!(after.start_frame(), now);
            assert_eq!(after.duration(), before.remaining(now) + 1);
            moving_visits += 1;
            if moving_visits == 12 {
                break;
            }
        }
    }
    assert_eq!(
        moving_visits, 12,
        "actual installed Drive +10 must report movement"
    );
    scene.command("Americans", Command::Stop { entity_id: tank });
    for _ in 0..80 {
        scene.frame();
        if crate::sim::movement::motion_query::is_moving(scene.actor(tank)) == Some(false) {
            break;
        }
    }
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(scene.actor(tank)),
        Some(false)
    );
    let stopped = scene.reload(tank);
    assert!(stopped.remaining(scene.now()) > 0);
    while scene.now() < stopped.start_frame() + stopped.duration() {
        scene.frame();
        assert_eq!(scene.count(tank), 4);
        assert_eq!(scene.reload(tank), stopped);
    }
    scene.frame();
    assert_eq!(
        scene.count(tank),
        5,
        "expiry adds one, rather than filling the magazine"
    );
    let next = scene.reload(tank);
    assert_eq!(
        next.duration(),
        72,
        "the next real count5 retains quotient2"
    );
    while scene.now() < next.start_frame() + next.duration() {
        scene.frame();
        assert_eq!(scene.count(tank), 5);
    }
    scene.frame();
    assert_eq!(scene.count(tank), 6);
    assert_eq!(
        scene.reload(tank),
        next,
        "a full magazine leaves the old timer"
    );
}
