//! Stock DroneJump -> Bullet4693D3 -> Parasite629FD0 -> Rock70B280.
//! The flat arena and distant unarmed house keepalives are supplied map
//! inputs. Rules, effect/model assets, actor admission and combat are real.

use super::{FrameEffects, Simulation, TickLane};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::command::{Command, CommandEnvelope};
use crate::util::fixed_math::SimFixed;

fn ordinary_frame(sim: &mut Simulation, rules: &RuleSet) -> Vec<super::SimFireEvent> {
    let commands = sim.take_due_commands();
    let output = sim
        .advance_app_frame(
            &commands,
            Some(rules),
            None,
            33,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .expect("ordinary retail Drone combat frame");
    assert!(output.tick.frame_committed);
    output.fire_events
}

struct DroneScene {
    rules: RuleSet,
    sim: Simulation,
    drone: u64,
    tank: u64,
}

impl DroneScene {
    fn retail() -> Option<Self> {
        Self::retail_at(None)
    }

    fn retail_at(native: Option<&serde_json::Value>) -> Option<Self> {
        let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
        rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        let drone_type = rules.object("DRON").unwrap();
        assert_eq!(drone_type.primary(), Some("DroneJump"));
        let drone_strength = drone_type.strength;
        let tank_type = rules.object("MTNK").unwrap();
        assert_eq!(tank_type.strength, 300);
        let parasite = rules.warhead("Parasite").unwrap();
        assert!(parasite.parasite);
        assert_eq!(parasite.paralyzes, 0);
        assert_eq!(
            parasite.verses_f64[crate::sim::combat::armor_index(&tank_type.armor)],
            1.0
        );
        let jump = rules.weapon("DroneJump").unwrap();
        assert!(jump.limbo_launch);
        assert_eq!(jump.projectile.as_deref(), Some("JUMP"));
        assert_eq!(jump.warhead.as_deref(), Some("Parasite"));
        assert_eq!(jump.damage, 50);

        let seed = native.map_or(0x629fd0, |input| input["seed"].as_u64().unwrap());
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, seed, 64);
        if let Some(input) = native {
            sim.session.game_options_mut_for_test().game_speed =
                input["game_speed"].as_i64().unwrap() as i32;
            sim.session.lighting_mut().set_normal_for_test(
                serde_json::from_value(input["lighting"]["normal_profile"].clone()).unwrap(),
            );
        }
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.resolve_rule_animation_lists(&rules);
        let mut actors = Vec::new();
        for (index, (name, house, cell)) in [
            ("DRON", "Americans", (24, 24)),
            ("MTNK", "Russians", (25, 24)),
        ]
        .into_iter()
        .enumerate()
        {
            let (rx, ry, facing) = if let Some(input) = native {
                let prior = &input["actor_admissions"][index];
                sim.native_unique_ids = Some(
                    crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(
                        (prior["native_id"].as_u64().unwrap() as u32).wrapping_sub(1),
                    ),
                );
                (
                    (prior["location"][0].as_i64().unwrap() / 256) as u16,
                    (prior["location"][1].as_i64().unwrap() / 256) as u16,
                    prior["unlimbo_facing_input"].as_u64().unwrap() as u8,
                )
            } else {
                (cell.0, cell.1, 0)
            };
            let actor = sim
                .spawn_object(name, house, rx, ry, facing, &rules)
                .expect("ordinary stock actor Unlimbo on clear ground");
            if let Some(input) = native {
                compare_drone_actor(&sim, &rules, actor, &input["actor_admissions"][index]);
            }
            actors.push(actor);
        }
        let (drone, tank) = (actors[0], actors[1]);
        assert_eq!(
            sim.substrate.entities.get(drone).unwrap().health.current(),
            drone_strength
        );
        let truck = rules.object("TRUCKA").unwrap();
        assert!(truck.primary().is_none() && truck.secondary().is_none());
        if native.is_none() {
            for (house, cell) in [("Americans", (4, 4)), ("Russians", (58, 58))] {
                sim.spawn_object("TRUCKA", house, cell.0, cell.1, 0, &rules)
                    .expect("unarmed stock house keepalive");
            }
        }
        let owner = sim.substrate.entities.get(drone).unwrap().owner();
        let opponent = sim.substrate.entities.get(tank).unwrap().owner();
        sim.session.set_current_house_for_test(Some(owner));
        sim.fog.mark_visible_for_owner(owner, 25, 24);
        sim.fog.mark_visible_for_owner(opponent, 24, 24);
        if native.is_none() {
            sim.queue_command(CommandEnvelope::new(
                owner,
                1,
                Command::Attack {
                    attacker_id: drone,
                    target_id: tank,
                },
            ));
        }

        Some(Self {
            rules,
            sim,
            drone,
            tank,
        })
    }

    fn step(&mut self) -> Vec<super::SimFireEvent> {
        ordinary_frame(&mut self.sim, &self.rules)
    }

    fn first_bite(&mut self) -> u32 {
        let mut actual_shot = false;
        let mut actual_bullet = false;
        let mut previous_health = 300;
        for _ in 0..240 {
            let shots = self.step();
            for shot in shots.iter().filter(|shot| shot.attacker_id == self.drone) {
                assert_eq!(self.sim.interner.resolve(shot.weapon_id), "DroneJump");
                assert_eq!(shot.weapon_slot, WeaponSlot::Primary);
                assert_eq!(shot.target, TargetKind::Entity(self.tank));
                actual_shot = true;
                let source = self.sim.substrate.entities.get(self.drone).unwrap();
                assert!(source.lifecycle.object_alive() && source.health.current() > 0);
                assert!(source.lifecycle.in_limbo());
            }
            actual_bullet |= self.sim.projectiles.iter().any(|(_, bullet)| {
                bullet.source_id == self.drone
                    && bullet.target == crate::sim::projectile::ProjectileTarget::Entity(self.tank)
                    && bullet
                        .payload
                        .projectile_type()
                        .is_some_and(|id| self.sim.interner.resolve(id) == "JUMP")
            });
            let source = self.sim.substrate.entities.get(self.drone).unwrap();
            let victim = self.sim.substrate.entities.get(self.tank).unwrap();
            let health = victim.health.current();
            assert!(source.lifecycle.object_alive() && source.health.current() > 0);
            assert!(victim.lifecycle.object_alive() && health > 0);
            if health < previous_health {
                assert!(
                    actual_shot && actual_bullet,
                    "real DroneJump and retained JUMP precede bite"
                );
                assert_eq!(
                    previous_health - health,
                    self.rules.weapon("DroneJump").unwrap().damage
                );
                assert!(source.lifecycle.in_limbo());
                assert_eq!(source.parasite.class().unwrap().victim(), Some(self.tank));
                assert_eq!(victim.parasite.eater(), Some(self.drone));
                return self.sim.session.binary_frame();
            }
            previous_health = health;
        }
        panic!("stock Drone never reached its real shot, attachment and first nonlethal bite");
    }

    fn rates(&self) -> [SimFixed; 2] {
        let rocking = self
            .sim
            .substrate
            .entities
            .get(self.tank)
            .unwrap()
            .rocking
            .as_ref()
            .expect("actual bite installs its authoritative rocking state");
        [rocking.vel_sideways(), rocking.vel_forwards()]
    }

    fn angles(&self) -> [SimFixed; 2] {
        let rocking = self
            .sim
            .substrate
            .entities
            .get(self.tank)
            .unwrap()
            .rocking
            .as_ref()
            .expect("actual bite retains its authoritative rocking state");
        [rocking.angle_sideways(), rocking.angle_forwards()]
    }

    fn assert_impulse(&self) {
        assert!(
            self.rates().iter().any(|rate| *rate != SimFixed::ZERO),
            "real stock bite must assign the live tank's rocker impulse"
        );
    }

    fn receive(&mut self, target: u64, warhead: &str, damage: i32) {
        use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
        // Explicit direct receiver component boundary for release/death, not
        // an ordinary Dolphin weapon producer or an extra combat mechanism.
        let warhead = self.sim.interner.intern(warhead);
        let event = EntityDamageEvent::direct_receiver(
            target,
            damage,
            0,
            RAD_NO_ATTACKER,
            None,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: true,
            },
        );
        self.sim
            .commit_direct_damage_receiver(&self.rules, None, event, FrameEffects::default());
    }

    fn assert_linked_and_mobile(&self) {
        let source = self.sim.substrate.entities.get(self.drone).unwrap();
        let target = self.sim.substrate.entities.get(self.tank).unwrap();
        assert!(source.lifecycle.object_alive() && source.health.current() > 0);
        assert!(source.lifecycle.in_limbo());
        assert!(target.lifecycle.object_alive() && target.health.current() > 0);
        assert_eq!(source.parasite.class().unwrap().victim(), Some(self.tank));
        assert_eq!(target.parasite.eater(), Some(self.drone));
        assert!(
            !target
                .parasite
                .is_paralyzed(self.sim.session.binary_frame())
        );
    }
}

#[test]
fn stock_infested_tank_moves_and_fires_while_its_body_is_rocking() {
    let Some(mut scene) = DroneScene::retail() else {
        return;
    };
    // Ordinary Move admission on the initial encounter, with an unarmed
    // hostile nearby; no health or weapon tuning.
    let mcv = scene.rules.object("AMCV").unwrap();
    assert!(mcv.primary().is_none() && mcv.secondary().is_none());
    let _movement_target = scene
        .sim
        .spawn_object("AMCV", "Americans", 27, 24, 0, &scene.rules)
        .expect("ordinary stock AMCV target");
    scene.first_bite();
    scene.assert_impulse();
    let target = scene.sim.substrate.entities.get(scene.tank).unwrap();
    let house = target.owner();
    let before = crate::sim::movement::ground_pose::position_world_coord(&target.position);
    scene.sim.queue_command(CommandEnvelope::new(
        house,
        scene.sim.session.tick() + 1,
        Command::Move {
            entity_id: scene.tank,
            target_rx: 25,
            target_ry: 27,
        },
    ));
    let mut moved = false;
    for _ in 0..30 {
        scene.step();
        scene.assert_linked_and_mobile();
        let current = crate::sim::movement::ground_pose::position_world_coord(
            &scene
                .sim
                .substrate
                .entities
                .get(scene.tank)
                .unwrap()
                .position,
        );
        if (current.x, current.y) != (before.x, before.y) {
            moved = true;
            break;
        }
    }
    assert!(moved, "Parasite.Paralyzes0 must admit an ordinary Move");
    // Use a fresh ordinary encounter for firing. Moving first changes the
    // turret/rearm phase enough that its shots can fall between the lurches.
    // After the real Drone launch/first bite, admit a real unarmed MCV farther
    // along the tank's acquired aim. New bullets take their first AI in the
    // firing frame (native Logic55B613); at close contact that visit can retire
    // them before observation. This ordinary in-range separation allows
    // observing the shared muzzle/launch coordinate without extending a bullet
    // lifetime or supplying facing, rearm, pose or attachment state.
    let mut scene = DroneScene::retail().unwrap();
    scene.first_bite();
    scene.assert_impulse();
    let other = scene
        .sim
        .spawn_object("AMCV", "Americans", 22, 24, 0, &scene.rules)
        .expect("ordinary AMCV Unlimbo along the tank's acquired aim");
    let house = scene
        .sim
        .substrate
        .entities
        .get(scene.tank)
        .unwrap()
        .owner();
    scene.sim.queue_command(CommandEnvelope::new(
        house,
        scene.sim.session.tick() + 1,
        Command::Attack {
            attacker_id: scene.tank,
            target_id: other,
        },
    ));
    let mut timeline = Vec::new();
    let mut previous = None;
    for _ in 0..280 {
        let shots = scene.step();
        let source = scene.sim.substrate.entities.get(scene.drone);
        let tank = scene.sim.substrate.entities.get(scene.tank);
        let state = (
            source.map(|entity| (entity.health.current(), entity.lifecycle.in_limbo())),
            tank.map(|entity| entity.health.current()),
            tank.and_then(|entity| entity.rocking.as_ref())
                .is_some_and(|rocking| {
                    [rocking.angle_sideways(), rocking.angle_forwards()]
                        .iter()
                        .any(|angle| angle.abs() > SimFixed::lit("0.005"))
                }),
        );
        if previous != Some(state)
            || !shots.is_empty()
            || scene.sim.session.binary_frame() % 30 == 0
        {
            timeline.push(format!(
                "frame{} source/host/visible={state:?} pose={:?} rock={:?} shots={:?}",
                scene.sim.session.binary_frame() - 1,
                tank.map(
                    |entity| crate::sim::movement::ground_pose::position_world_coord(
                        &entity.position
                    )
                ),
                tank.and_then(|entity| entity.rocking.as_ref())
                    .map(|rocking| rocking.native_bits()),
                shots
                    .iter()
                    .map(|shot| (
                        shot.attacker_id,
                        scene.sim.interner.resolve(shot.weapon_id),
                        shot.target
                    ))
                    .collect::<Vec<_>>()
            ));
        }
        previous = Some(state);
        assert!(
            source.is_some() && tank.is_some(),
            "live firing encounter retired an actor before a rocked shot:\n{}",
            timeline.join("\n")
        );
        scene.assert_linked_and_mobile();
        if !state.2 {
            continue;
        }
        if let Some(shot) = shots
            .iter()
            .find(|shot| shot.attacker_id == scene.tank && shot.target == TargetKind::Entity(other))
        {
            assert_eq!(scene.sim.interner.resolve(shot.weapon_id), "105mm");
            assert_eq!(shot.weapon_slot, WeaponSlot::Primary);
            let (_, bullet) = scene
                .sim
                .projectiles
                .iter()
                .find(|(_, bullet)| {
                    bullet.source_id == scene.tank
                        && bullet.target == crate::sim::projectile::ProjectileTarget::Entity(other)
                })
                .unwrap_or_else(|| panic!(
                    "rocked105mm Bullet retired in its firing frame: fire={:?} target={:?} bullets={:?}\n{}",
                    shot.fire_coord,
                    scene.sim.substrate.entities.get(other).map(|entity| (
                        entity.health.current(),
                        crate::sim::movement::ground_pose::position_world_coord(&entity.position)
                    )),
                    scene.sim.projectiles.iter().collect::<Vec<_>>(),
                    timeline.join("\n")
                ));
            assert_eq!(bullet.launch_origin, shot.fire_coord);
            // Exact tilted geometry belongs to the consumed native Drive/FLH
            // controls. Here the real event, Bullet and common muzzle owner
            // must stay connected while the infested actor moves/fires.
            return;
        }
    }
    panic!(
        "live infested MTNK never fired its real105mm during a visible rock:\n{}",
        timeline.join("\n")
    );
}

#[test]
fn stock_terror_drone_sonic_release_retains_rates_then_naturally_settles() {
    let Some(mut scene) = DroneScene::retail() else {
        return;
    };
    scene.first_bite();
    scene.assert_impulse();
    scene.step();
    let rates = scene.rates();
    let angles = scene.angles();
    assert!(scene.rules.warhead("SonicWarhead").unwrap().sonic);
    scene.receive(scene.tank, "SonicWarhead", 1);
    let source = scene.sim.substrate.entities.get(scene.drone).unwrap();
    assert!(source.lifecycle.object_alive() && source.health.current() > 0);
    assert!(!source.lifecycle.in_limbo());
    assert_eq!(source.parasite.class().unwrap().victim(), None);
    let host = scene.sim.substrate.entities.get(scene.tank).unwrap();
    assert_eq!(host.parasite.eater(), None);
    assert!(!host.parasite.is_paralyzed(scene.sim.session.binary_frame()));
    // ExitUnit62A862 clears only sideways angle, not forwards or either rate.
    assert_eq!(scene.angles(), [SimFixed::ZERO, angles[1]]);
    assert_eq!(scene.rates(), rates);
    let health = host.health.current();
    let mut settled = false;
    for _ in 0..180 {
        scene.step();
        let host = scene.sim.substrate.entities.get(scene.tank).unwrap();
        assert!(host.lifecycle.object_alive());
        assert_eq!(host.parasite.eater(), None);
        assert_eq!(host.health.current(), health, "release must end real bites");
        if scene.rates() == [SimFixed::ZERO; 2] && scene.angles() == [SimFixed::ZERO; 2] {
            settled = true;
            break;
        }
    }
    assert!(settled, "ordinary Techno rocking must settle after release");
}

#[test]
fn stock_terror_drone_own_bites_kill_the_host_and_release_its_live_owner() {
    let Some(mut scene) = DroneScene::retail() else {
        return;
    };
    let first = scene.first_bite();
    scene.assert_impulse();
    let rof = scene.rules.weapon("DroneJump").unwrap().rof;
    let mut fatal = false;
    for _ in 0..rof * 5 + 3 {
        scene.step();
        if scene
            .sim
            .substrate
            .entities
            .get(scene.tank)
            .is_none_or(|host| !host.lifecycle.object_alive())
        {
            fatal = true;
            assert_eq!(scene.sim.session.binary_frame(), first + (rof * 5) as u32);
            break;
        }
    }
    assert!(
        fatal,
        "six real50-damage bites must kill the stock300-HP tank"
    );
    for _ in 0..3 {
        scene.step();
    }
    assert!(!scene.sim.substrate.entities.contains(scene.tank));
    let source = scene.sim.substrate.entities.get(scene.drone).unwrap();
    assert!(source.lifecycle.object_alive() && source.health.current() > 0);
    assert!(!source.lifecycle.in_limbo());
    assert_eq!(source.parasite.class().unwrap().victim(), None);
    assert_eq!(source.parasite.class().unwrap().grapple_anim(), None);
}

#[test]
fn stock_terror_drone_owner_death_clears_the_link_without_erasing_host_rock() {
    let Some(mut scene) = DroneScene::retail() else {
        return;
    };
    scene.first_bite();
    scene.assert_impulse();
    scene.step();
    let rates = scene.rates();
    let angles = scene.angles();
    scene.receive(scene.drone, "Super", 100_000);
    let source = scene.sim.substrate.entities.get(scene.drone).unwrap();
    assert_eq!(source.health.current(), 0, "actual fatal owner receiver");
    assert!(
        scene
            .sim
            .substrate
            .pending_delete
            .contains_object(&scene.drone)
    );
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.tank)
            .unwrap()
            .parasite
            .eater(),
        None
    );
    // FootPointerExpired4D998C clears the dead eater backlink. No native
    // angle/rate write belongs to this notification or Parasite destructor.
    assert_eq!(scene.rates(), rates);
    assert_eq!(scene.angles(), angles);
    for _ in 0..3 {
        scene.step();
    }
    assert!(!scene.sim.substrate.entities.contains(scene.drone));
    let host = scene.sim.substrate.entities.get(scene.tank).unwrap();
    assert!(host.lifecycle.object_alive() && host.health.current() > 0);
    assert_eq!(host.parasite.eater(), None);
}

#[test]
fn stock_terror_drone_internal_restore_keeps_rock_and_restarts_its_bite_timer() {
    let Some(mut scene) = DroneScene::retail() else {
        return;
    };
    scene.first_bite();
    scene.assert_impulse();
    scene.step();
    let rates = scene.rates();
    let angles = scene.angles();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&scene.sim, 0, 0, "drone-rocking", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    // Explicit process-stream continuation input for this internal Rust
    // state test; no app/native SaveGame or RNG persistence claim.
    loaded.scenario_rng = scene.sim.scenario_rng.clone();
    loaded.main_rng = scene.sim.main_rng.snapshot_for_test().into();
    loaded.mapgen_rng = scene.sim.mapgen_rng.clone();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        scene.sim.resolved_terrain.clone().unwrap(),
        scene.sim.terrain_speed_config.clone(),
        &scene.rules,
    );
    loaded.resolve_type_handles(&scene.rules);
    assert!(loaded.rebuild_dynamic_navigation(&scene.rules));
    scene.sim = loaded;
    assert_eq!(scene.rates(), rates);
    assert_eq!(scene.angles(), angles);
    scene.assert_linked_and_mobile();
    let state = scene
        .sim
        .substrate
        .entities
        .get(scene.drone)
        .unwrap()
        .parasite
        .class()
        .unwrap();
    let (suppression, damage) = state.grapple_timers_for_test();
    // Load6295DB..6295F3 resets these timers, not the retained body rates.
    for timer in [suppression, damage] {
        assert_eq!(timer.start_frame(), scene.sim.session.binary_frame() as i32);
        assert_eq!(timer.duration(), 0);
    }
    let before = scene
        .sim
        .substrate
        .entities
        .get(scene.tank)
        .unwrap()
        .health
        .current();
    scene.step();
    scene.assert_linked_and_mobile();
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.tank)
            .unwrap()
            .health
            .current(),
        before - 50
    );
    scene.assert_impulse();
}

fn compare_drone_actor(sim: &Simulation, rules: &RuleSet, id: u64, native: &serde_json::Value) {
    use serde_json::json;
    let entity = sim.substrate.entities.get(id).unwrap();
    let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    assert_eq!(json!(entity.native_unique_id()), native["native_id"]);
    assert_eq!(json!(entity.health.current()), native["health"]);
    assert_eq!(
        json!(u8::from(entity.lifecycle.object_alive())),
        native["alive"]
    );
    assert_eq!(
        json!(u8::from(entity.lifecycle.in_limbo())),
        native["limbo"]
    );
    assert_eq!(
        json!([location.x, location.y, location.z]),
        native["location"]
    );
    if let Some(expected) = native.get("primary_current_raw") {
        assert_eq!(
            json!(entity.body_facing_current(sim.session.binary_frame())),
            *expected
        );
    }
    if let Some(expected) = native.get("secondary_current_raw") {
        let secondary = if let Some(facing) = entity.barrel_facing {
            facing.current(sim.session.binary_frame())
        } else {
            // Unit TurretFacing746E30 selects Primary+388 when Turret+CA1
            // is false. Rust omits that unused inline Secondary controller;
            // compare its existing semantic fallback, not an invented zero.
            assert!(
                !rules
                    .object(sim.interner.resolve(entity.type_ref()))
                    .unwrap()
                    .has_turret
            );
            assert_eq!(
                native["secondary_current_raw"],
                native["primary_current_raw"]
            );
            entity.body_facing_current(sim.session.binary_frame())
        };
        assert_eq!(json!(secondary), *expected);
    }
}

fn compare_drone_component(scene: &DroneScene, native: &serde_json::Value) {
    use serde_json::json;
    compare_drone_actor(&scene.sim, &scene.rules, scene.drone, &native["source"]);
    compare_drone_actor(&scene.sim, &scene.rules, scene.tank, &native["victim"]);
    let source = scene.sim.substrate.entities.get(scene.drone).unwrap();
    let state = source.parasite.class().unwrap();
    assert_eq!(state.victim(), Some(scene.tank));
    assert_eq!(state.grapple_anim(), None);
    assert!(native["held_anim"].is_null());
    assert_eq!(native["manager"]["native_id"], -1);
    let (phase, stage, delay, invalidated) = state.grapple_progress_for_test();
    assert_eq!(
        json!([phase, stage, delay, i32::from(invalidated)]),
        json!([
            native["manager"]["phase"],
            native["manager"]["stage"],
            native["manager"]["delay"],
            native["manager"]["invalidated"]
        ])
    );
    for (timer, key) in [
        (state.grapple_timers_for_test().0, "timer_2c"),
        (state.grapple_timers_for_test().1, "timer_38"),
    ] {
        assert_eq!(
            json!([timer.start_frame(), timer.duration()]),
            json!([
                native["manager"][key]["start"],
                native["manager"][key]["duration"]
            ])
        );
    }
    let victim = scene.sim.substrate.entities.get(scene.tank).unwrap();
    assert_eq!(victim.parasite.eater(), Some(scene.drone));
    let timer = victim.parasite.paralysis();
    assert_eq!(
        json!([timer.start_frame(), timer.duration()]),
        json!([
            native["victim"]["paralysis_timer"][0],
            native["victim"]["paralysis_timer"][2]
        ])
    );
    assert_eq!(
        json!(u8::from(victim.sinking.is_active())),
        native["victim"]["sinking"]
    );
    let actual = victim
        .rocking
        .as_ref()
        .map_or([0; 4], |state| state.native_bits());
    assert_eq!(
        json!(actual),
        native["victim"]["rocking_bits"],
        "frame{}: complete authoritative native words",
        scene.sim.session.binary_frame()
    );
}

fn compare_drone_rng(sim: &Simulation, native: &serde_json::Value) {
    for (actual, key) in [
        (sim.main_rng.native_state_hex(), "main"),
        (sim.scenario_rng.native_state_hex(), "scenario"),
        (sim.mapgen_rng.native_state_hex(), "mapgen"),
    ] {
        assert_eq!(
            actual,
            native[key].as_str().unwrap(),
            "frame{} {key}",
            sim.session.binary_frame()
        );
    }
}

/// Bounded original70B570 then whole629FD0 visits. No other actor/particle
/// AI or full Logic runs in either engine. Actor admission, direct Limbo and
/// real AttachTo are the declared entry; only one measured RNG/ID reinstall.
#[test]
fn stock_drone_monotonic_bites_match_native_state_rng_spark_births_and_flh() {
    use crate::sim::native_identity::NativeUniqueIdCursor;
    use crate::sim::rng::SimRng;
    use serde_json::{Value, json};
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/parasite_drone_rng.json",
    ))
    .unwrap();
    let initial = &corpus["initial"];
    let Some(mut scene) = DroneScene::retail_at(Some(initial)) else {
        return;
    };
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    for (name, expected) in initial["model_input"]["files"].as_object().unwrap() {
        let physical = assets.load_file_from_mix(name).unwrap();
        assert_eq!(json!(physical.bytes.len()), expected["bytes"]);
        assert_eq!(
            crate::util::sha256::sha256_hex(&physical.bytes),
            expected["sha256"].as_str().unwrap()
        );
    }
    assert_eq!(initial["model_input"]["joined_is_voxel_al"], 1);
    let mtnk = scene.rules.type_handle("MTNK").unwrap();
    assert!(scene.rules.object_has_loaded_voxel(mtnk));
    let (body, _) =
        crate::rules::voxel_asset_catalog::load_main_voxel(&assets, "MTNK", Some(&scene.rules))
            .unwrap();
    let section = &body.vxl().limbs[0];
    assert_eq!(
        json!([section.size_x, section.size_y, section.size_z]),
        initial["model_input"]["first_section_size"]
    );
    for expected in initial["type_inputs"].as_array().unwrap() {
        let name = expected["name"].as_str().unwrap();
        let object = scene.rules.object(name).unwrap();
        assert_eq!(json!(object.strength), expected["strength"]);
        assert_eq!(
            format!("{:016x}", object.weight.to_num::<f64>().to_bits()),
            expected["weight_bits"].as_str().unwrap()
        );
        let flh = scene.rules.art().get(&object.image).unwrap().weapon_flh(
            object.turret_count,
            object.weapon_count,
            0,
            false,
        );
        assert_eq!(
            json!([flh.forward, flh.lateral, flh.height]),
            expected["primary_flh"]
        );
    }
    let weapon = scene
        .rules
        .weapon(initial["weapon_input"]["name"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        json!([weapon.damage, weapon.rof]),
        json!([
            initial["weapon_input"]["damage"],
            initial["weapon_input"]["rof"]
        ])
    );
    assert_eq!(
        weapon.warhead.as_deref().unwrap(),
        initial["weapon_input"]["warhead"].as_str().unwrap()
    );
    assert_eq!(
        json!(
            scene
                .rules
                .warhead(weapon.warhead.as_deref().unwrap())
                .unwrap()
                .paralyzes
        ),
        initial["weapon_input"]["paralyzes"]
    );
    let spark_name = scene
        .rules
        .combat_damage
        .default_spark_system
        .as_deref()
        .unwrap();
    let spark = scene
        .rules
        .particle_system_type(scene.rules.ps_type_id_by_name(spark_name).unwrap());
    let expected = &initial["spark_type_inputs"]["system"];
    assert_eq!(json!(spark.behaves_like as u8), expected["behavior"]);
    assert_eq!(json!(spark.lifetime), expected["lifetime"]);
    assert_eq!(
        json!(spark.spark_spawn_frames),
        expected["spark_spawn_frames"]
    );
    assert_eq!(json!(spark.particle_cap), expected["particle_cap"]);
    assert_eq!(json!(spark.light_size), expected["light_size"]);
    assert_eq!(json!(spark.one_frame_light), expected["one_frame_light"]);
    // Native slots/alliance masks are distinct; the partial legacy native
    // owner ID is zero for both and is not an owner-identity mapping.
    for (index, slot, mask) in [(0, 0, 1), (1, 1, 2)] {
        assert_eq!(initial["actor_admissions"][index]["owner_slot"], slot);
        assert_eq!(initial["actor_admissions"][index]["owner_alliances"], mask);
    }
    assert_ne!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.drone)
            .unwrap()
            .owner(),
        scene
            .sim
            .substrate
            .entities
            .get(scene.tank)
            .unwrap()
            .owner()
    );
    scene
        .sim
        .session
        .set_binary_frame_for_test(initial["frame"].as_u64().unwrap() as u32);
    scene.sim.techno_limbo_with_context(
        scene.drone,
        super::UninitContext::with_rules(&scene.rules).with_effects(FrameEffects::default()),
    );
    for (index, id) in [(0, scene.drone), (1, scene.tank)] {
        compare_drone_actor(
            &scene.sim,
            &scene.rules,
            id,
            &initial["attach_entry"][index],
        );
    }
    assert!(
        scene
            .sim
            .parasite_can_infect(scene.drone, Some(scene.tank), &scene.rules)
    );
    scene.sim.parasite_attach(
        scene.drone,
        Some(scene.tank),
        &scene.rules,
        FrameEffects::default(),
    );
    for (index, id) in [(0, scene.drone), (1, scene.tank)] {
        compare_drone_actor(
            &scene.sim,
            &scene.rules,
            id,
            &initial["grapple_entry"][index],
        );
    }
    let rows = corpus["rows"].as_array().unwrap();
    scene.sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
        initial["native_cursor"].as_u64().unwrap() as u32,
    ));
    scene.sim.main_rng =
        SimRng::from_native_state_hex_for_test(rows[0]["rng_before"]["main"].as_str().unwrap())
            .into();
    scene.sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(rows[0]["rng_before"]["scenario"].as_str().unwrap());
    scene.sim.mapgen_rng =
        SimRng::from_native_state_hex_for_test(rows[0]["rng_before"]["mapgen"].as_str().unwrap());
    let mut compared = 0;
    for frame in 0..=corpus["visits_end"].as_u64().unwrap() as u32 {
        scene.sim.session.set_binary_frame_for_test(frame);
        let row = rows.iter().find(|row| row["input"]["frame"] == frame);
        if let Some(row) = row {
            assert_eq!(row["input"]["entry"], json!(["0070B570", "00629FD0"]));
            compare_drone_component(&scene, &row["before"]);
            compare_drone_rng(&scene.sim, &row["rng_before"]);
            assert_eq!(
                json!(scene.sim.native_unique_ids.as_ref().unwrap().current_raw()),
                row["native_cursor_before"]
            );
        }
        let before: std::collections::BTreeSet<_> = scene
            .sim
            .particle_systems()
            .iter()
            .map(|(&id, _)| id)
            .collect();
        let anim_count = scene.sim.anims().count();
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            let target = scene.sim.substrate.entities.get_mut(scene.tank).unwrap();
            assert!(!crate::sim::rocking::update(
                target,
                scene.rules.object("MTNK").unwrap(),
                scene.rules.general.fallback_coefficient,
                frame
            ));
            scene.sim.parasite_ai_for_victim(
                scene.tank,
                &scene.rules,
                None,
                FrameEffects::default(),
            );
        });
        let births: Vec<_> = scene
            .sim
            .substrate
            .logic
            .as_slice()
            .iter()
            .copied()
            .filter(|id| !before.contains(id) && scene.sim.particle_systems().contains_key(*id))
            .collect();
        assert_eq!(
            scene.sim.anims().count(),
            anim_count,
            "selected DroneJump has no Anim"
        );
        if let Some(row) = row {
            let words: Vec<_> = draws.iter().map(|draw| draw["value"].clone()).collect();
            let expected_words: Vec<_> = row["rng_requests"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|request| request["raw_draws"].as_array().unwrap().iter().cloned())
                .collect();
            assert_eq!(words, expected_words, "frame{frame} ordered raw words");
            assert_eq!(json!(draws.len()), row["raw_draw_count"]);
            compare_drone_rng(&scene.sim, &row["rng_after"]);
            assert_eq!(
                json!(scene.sim.native_unique_ids.as_ref().unwrap().current_raw()),
                row["native_cursor_after"]
            );
            compare_drone_component(&scene, &row["after"]);
            let expected = row["particle_systems"].as_array().unwrap();
            assert_eq!(
                births.len(),
                expected.len(),
                "frame{frame} real SparkSys births"
            );
            for (id, expected) in births.into_iter().zip(expected) {
                let system = scene.sim.particle_systems().get(id).unwrap();
                let state = &expected["state"];
                assert_eq!(
                    scene.rules.particle_system_type(system.type_id).name,
                    expected["type"].as_str().unwrap()
                );
                assert_eq!(json!(system.native_unique_id()), state["native_id"]);
                assert_eq!(json!(system.coords.to_array()), state["position"]);
                assert_eq!(json!(system.lifetime), state["lifetime"]);
                assert_eq!(
                    json!(system.spark_spawn_frames),
                    state["spark_spawn_frames"]
                );
                assert_eq!(json!(system.done_spawning), state["time_to_die"]);
                assert_eq!(json!(system.in_logic_vector), state["logic"]);
                assert!(system.alive() && !system.in_limbo());
                assert!(
                    system.owner_entity.is_none()
                        && system.target_ref.is_none()
                        && system.owner_house.is_none()
                );
                assert_eq!(json!(system.target_coords.to_array()), state["target"]);
                assert_eq!(json!(system.particles.len()), state["particle_count"]);
                assert!(system.particles.is_empty());
                assert_eq!(
                    expected["rng_request_before"], expected["rng_request_after"],
                    "native constructor draws no RNG"
                );
            }
            let target = scene.sim.substrate.entities.get(scene.tank).unwrap();
            let flh = crate::sim::combat::fire_coord::fire_coordinate(
                &scene.sim,
                &scene.rules,
                &crate::sim::combat::fire_coord::FireSource::of_entity(target),
                scene.rules.object("MTNK").unwrap(),
                0,
                0,
                Default::default(),
            )
            .coord;
            assert_eq!(
                json!([flh.x, flh.y, flh.z]),
                row["victim_flh"],
                "frame{frame} actual ground GetFLH owner"
            );
            compared += 1;
        } else {
            assert!(
                draws.is_empty() && births.is_empty(),
                "unselected visit cannot hide work"
            );
        }
    }
    assert_eq!(compared, 7);
}
