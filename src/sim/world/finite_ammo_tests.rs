//! Registered Scenario ammo overrides with physical retail weapons/art/assets.
//! Clear arena, human roster, visibility and pre-Logic semantic command delivery
//! are supplied boundaries; these are not original whole-frame or input goldens.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::combat::TargetKind;
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
fn finite_infantry_reloads_and_resumes_fire_only_with_ares() {
    let Some(mut scene) =
        Scene::new("[E1]\nAmmo=1\nInitialAmmo=1\nReload=8\nEmptyReload=12\nCanPassiveAquire=no\n")
    else {
        return;
    };
    let target = scene.spawn("GACNST", "Russians", (16, 16));
    let infantry = scene.spawn("E1", "Americans", (16, 20));
    scene.command(
        "Americans",
        Command::Attack {
            attacker_id: infantry,
            target_id: target,
        },
    );
    let first = scene.shot(infantry, target);
    assert_eq!(scene.count(infantry), 0);
    #[cfg(feature = "ares")]
    assert_eq!(
        (
            scene.reload(infantry).start_frame(),
            scene.reload(infantry).duration()
        ),
        (first, 12)
    );
    #[cfg(not(feature = "ares"))]
    let _ = first;
    let mut fired_again = false;
    for _ in 0..100 {
        if fired(&scene.frame(), infantry) {
            fired_again = true;
            break;
        }
    }
    assert_eq!(
        fired_again,
        cfg!(feature = "ares"),
        "Ares adds the Infantry reload visit; stock does not"
    );
}

#[cfg(feature = "ares")]
#[test]
fn ares_weapon_ammo_cost_controls_admission_debit_and_reload() {
    let weapon =
        "[AmmoProbe]\nDamage=1\nROF=15\nRange=6\nProjectile=InvisibleLow\nWarhead=SA\nAmmo=";
    let Some(mut free) = Scene::new(&format!(
        "[E1]\nAmmo=0\nInitialAmmo=0\nPrimary=AmmoProbe\nElitePrimary=AmmoProbe\nCanPassiveAquire=no\n{weapon}0\n"
    )) else {
        return;
    };
    let target = free.spawn("GACNST", "Russians", (16, 16));
    let infantry = free.spawn("E1", "Americans", (16, 20));
    free.command(
        "Americans",
        Command::Attack {
            attacker_id: infantry,
            target_id: target,
        },
    );
    free.shot(infantry, target);
    assert_eq!(
        free.count(infantry),
        0,
        "zero-cost weapons fire with an empty magazine"
    );
    assert_eq!(free.reload(infantry).duration(), 0);

    let Some(mut pair) = Scene::new(&format!(
        "[E1]\nAmmo=3\nInitialAmmo=3\nReload=12\nPrimary=AmmoProbe\nElitePrimary=AmmoProbe\nCanPassiveAquire=no\n{weapon}2\n"
    )) else {
        return;
    };
    let target = pair.spawn("GACNST", "Russians", (16, 16));
    let infantry = pair.spawn("E1", "Americans", (16, 20));
    pair.command(
        "Americans",
        Command::Attack {
            attacker_id: infantry,
            target_id: target,
        },
    );
    let first = pair.shot(infantry, target);
    assert_eq!(pair.count(infantry), 1, "one shot consumes two rounds");
    assert_eq!(
        (
            pair.reload(infantry).start_frame(),
            pair.reload(infantry).duration()
        ),
        (first, 12)
    );
    pair.shot(infantry, target);
}

// This is the stock GetROF6FCFA9 magazine shortcut. Installed Ares hook
// 10047A60 skips it (continues at6FCFC1), independently of this ammo port.
#[cfg(not(feature = "ares"))]
#[test]
fn finite_building_fires_a_rapid_pair_refills_zero_and_keeps_last_round_rearm() {
    let Some(mut scene) = Scene::new("[GAPILL]\nAmmo=2\nInitialAmmo=2\nCanPassiveAquire=no\n")
    else {
        return;
    };
    let target = scene.spawn("GACNST", "Russians", (16, 16));
    let gun = scene.spawn("GAPILL", "Americans", (16, 20));
    scene.command(
        "Americans",
        Command::Attack {
            attacker_id: gun,
            target_id: target,
        },
    );
    let first = scene.shot(gun, target);
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
    assert!(rearm.duration() > 1);
    while scene.now() < rearm.start_frame() + rearm.duration() {
        assert!(
            !fired(&scene.frame(), gun),
            "refill must preserve the long cooldown"
        );
    }
    assert_eq!(scene.actor(gun).rearm_timer.timer(), rearm);
    let third = scene.shot(gun, target);
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
    // Resume through the actual load owner, including its Scenario RNG reseed.
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
        if saw_next_shot {
            break;
        }
    }
    assert!(saw_refill && saw_next_shot);
}

#[test]
fn damage_reduces_readiness_before_object_immunity_and_restarts_reload() {
    use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};

    let Some(mut scene) = Scene::new(
        "[LTNK]\nAmmo=4\nInitialAmmo=4\nStrength=200\nImmune=yes\nTypeImmune=yes\nReload=12\n\
         EmptyReload=25\nDamageReducesReadiness=yes\nReadinessReductionMultiplier=1\n",
    ) else {
        return;
    };
    let tank = scene.spawn("LTNK", "Americans", (12, 12));
    let now = scene.now();
    let hit = |scene: &mut Scene, damage, source| {
        let event = EntityDamageEvent::direct_receiver(
            tank,
            damage,
            0,
            source,
            None,
            scene.0.simulation.interner.intern("AP"),
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        scene.0.simulation.commit_direct_damage_receiver(
            &scene.0.resources.rules,
            Some(&scene.0.resources.overlay_registry),
            event,
            FrameEffects::default(),
        );
    };
    hit(&mut scene, 100, tank);
    assert_eq!(scene.count(tank), 4, "TypeImmune returns before readiness");
    // Native701AEF uses defended damage, before Object's Immune/Verses
    // gates. Half the type's Strength consumes half its four-round magazine.
    hit(&mut scene, 100, 0);
    assert_eq!(scene.actor(tank).health.current(), 200);
    assert_eq!(scene.count(tank), 2);
    assert_eq!(scene.reload(tank), CdTimer::started(now, 12));
    hit(&mut scene, 100, 0);
    assert_eq!(scene.count(tank), 0);
    assert_eq!(scene.reload(tank), CdTimer::started(now, 25));
    hit(&mut scene, -100, tank);
    assert_eq!(scene.count(tank), 2, "healing restores readiness too");
    assert_eq!(scene.reload(tank), CdTimer::started(now, 12));
    while scene.now() <= now + 12 {
        scene.frame();
    }
    assert_eq!(
        scene.count(tank),
        3,
        "the shared Unit reload adds one round"
    );
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
