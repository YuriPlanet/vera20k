//! Stock SQD Attack -> SquidGrab -> SQDJUMP -> ParasitePlus on open Water.
//! Native Bullet4693D3 -> AttachTo62A980 -> ParasiteAI629FD0/6297F0.
//! Retail layers/physical ART enter through production readers. The flat
//! Water arena (including its supplied WaterSet tile identity), placements,
//! initial veterancy and visibility are fixture inputs, not native map proof.

use super::{FrameEffects, Simulation, TickLane};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TargetKind};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::movement::ground_pose::position_world_coord;
use crate::util::fixed_math::SimFixed;

struct Encounter {
    rules: RuleSet,
    sim: Simulation,
    squid: u64,
    ship: u64,
    elite: bool,
}

impl Encounter {
    fn retail(elite: bool) -> Option<Self> {
        Self::retail_at(elite, None)
    }

    fn retail_at(elite: bool, native: Option<&serde_json::Value>) -> Option<Self> {
        let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()
            .expect("retail Battle fixture opened the install");
        let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
        rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        let squid_type = rules.object("SQD").unwrap();
        assert!(squid_type.naval && squid_type.organic);
        assert_eq!(squid_type.primary(), Some("SquidGrab"));
        let ship_type = rules.object("DEST").unwrap();
        assert!(ship_type.naval && !ship_type.organic && !ship_type.unnatural);
        let grab = rules
            .weapon(if elite { "SquidGrabE" } else { "SquidGrab" })
            .unwrap();
        assert!(grab.limbo_launch);
        assert_eq!(grab.projectile.as_deref(), Some("SQDJUMP"));
        assert_eq!(grab.warhead.as_deref(), Some("ParasitePlus"));
        assert!(rules.projectile("SQDJUMP").unwrap().inviso);
        let parasite = rules.warhead("ParasitePlus").unwrap();
        assert!(parasite.parasite && parasite.culling);
        assert_eq!(parasite.paralyzes, 32767);
        let art = rules.art().anim_runtime_config("SQDG").unwrap();
        assert_eq!(
            art.raw_shp_frame_count,
            Some(240),
            "physical SQDG, not supplied animation length"
        );

        let seed = native.map_or(6297, |initial| initial["seed"].as_u64().unwrap());
        let mut sim = crate::sim::arena_fixture::rival_water_arena(&rules, seed, 64);
        if let Some(initial) = native {
            // Original GameOptions speed (A8EB60) is an input to the
            // normalized splash constructor's existing rate calculation.
            sim.session.game_options_mut_for_test().game_speed = initial["game_speed"]
                .as_i64()
                .expect("original game-speed readback")
                as i32;
            // Supplied original component input: the native harness has not
            // run map lighting/recalculation. Import its measured profile,
            // retaining the production Cell ground-Z-adjust calculation.
            sim.session.lighting_mut().set_normal_for_test(
                serde_json::from_value(initial["lighting"]["normal_profile"].clone())
                    .expect("original Scenario lighting profile readback"),
            );
            // Explicit native-recorded component map input, before Unlimbo.
            // This is not a renderer/theater-loader parity assertion.
            let tile = initial["water"]["tile"].as_i64().unwrap() as i32;
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            terrain.set_projectile_water_set_base(tile);
            for y in 0..64 {
                for x in 0..64 {
                    let cell = terrain.cell_mut(x, y).unwrap();
                    cell.source_tile_index = tile;
                    cell.final_tile_index = tile;
                }
            }
        }
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.resolve_rule_animation_lists(&rules);
        let mut actors = Vec::new();
        for (index, (name, owner, cell)) in [
            ("SQD", "Americans", (24, 24)),
            ("DEST", "Russians", (25, 24)),
        ]
        .into_iter()
        .enumerate()
        {
            let (rx, ry, facing) = if let Some(initial) = native {
                let prior = &initial["actor_admissions"][index];
                sim.native_unique_ids = Some(
                    crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(
                        (prior["native_id"].as_i64().unwrap() as u32).wrapping_sub(1),
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
                .spawn_object(name, owner, rx, ry, facing, &rules)
                .unwrap_or_else(|| panic!("ordinary retail {name} Unlimbo on Water"));
            if let Some(initial) = native {
                let prior = &initial["actor_admissions"][index];
                let entity = sim.substrate.entities.get(actor).unwrap();
                assert_eq!(
                    serde_json::json!(entity.native_unique_id()),
                    prior["native_id"]
                );
                assert_eq!(serde_json::json!(entity.health.current()), prior["health"]);
                assert_eq!(
                    serde_json::json!(entity.body_facing_current(sim.session.binary_frame())),
                    prior["primary_current_raw"]
                );
                let coord = position_world_coord(&entity.position);
                assert_eq!(
                    serde_json::json!([coord.x, coord.y, coord.z]),
                    prior["location"]
                );
            }
            actors.push(actor);
        }
        let (squid, ship) = (actors[0], actors[1]);
        if elite {
            // Explicit map actor veterancy, not a promotion manufactured by damage.
            sim.substrate
                .entities
                .get_mut(squid)
                .unwrap()
                .veterancy
                .set_elite();
        }
        let owner = sim.substrate.entities.get(squid).unwrap().owner();
        let target_owner = sim.substrate.entities.get(ship).unwrap().owner();
        sim.session.set_current_house_for_test(Some(owner));
        sim.fog.mark_visible_for_owner(owner, 25, 24);
        sim.fog.mark_visible_for_owner(target_owner, 24, 24);
        assert!(
            sim.substrate
                .entities
                .get(squid)
                .unwrap()
                .parasite
                .class()
                .is_some()
        );
        if native.is_none() {
            sim.queue_command(CommandEnvelope::new(
                owner,
                1,
                Command::Attack {
                    attacker_id: squid,
                    target_id: ship,
                },
            ));
        }
        Some(Self {
            rules,
            sim,
            squid,
            ship,
            elite,
        })
    }

    fn step(&mut self) -> Vec<super::SimFireEvent> {
        let commands = self.sim.take_due_commands();
        let output = self
            .sim
            .advance_app_frame(
                &commands,
                Some(&self.rules),
                None,
                33,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary open-water Squid combat frame");
        assert!(output.tick.frame_committed);
        output.fire_events
    }

    fn attach_after_actual_shot(&mut self) {
        for _ in 0..240 {
            if let Some(shot) = self
                .step()
                .iter()
                .find(|shot| shot.attacker_id == self.squid)
            {
                assert_eq!(
                    self.sim.interner.resolve(shot.weapon_id),
                    if self.elite {
                        "SquidGrabE"
                    } else {
                        "SquidGrab"
                    }
                );
                assert_eq!(shot.weapon_slot, WeaponSlot::Primary);
                assert_eq!(shot.target, TargetKind::Entity(self.ship));
                let source = self.sim.substrate.entities.get(self.squid).unwrap();
                let target = self.sim.substrate.entities.get(self.ship).unwrap();
                assert!(source.lifecycle.object_alive() && source.health.current() > 0);
                assert!(target.lifecycle.object_alive() && target.health.current() > 0);
                // Meaningful first red: the actual admitted shot and live actors
                // above must be established before testing missing attachment.
                assert!(
                    source.lifecycle.in_limbo(),
                    "stock SquidGrab must LimboLaunch before returning"
                );
                // Inviso places the actual guided SQDJUMP on its target;
                // Fire468A49/468A5D retains Arm2 for proximity. Native
                //467C70 also admits an independent impact while unarmed.
                // Observe the retained shot and let ordinary Logic visits
                // choose the real impact/fuse admission; do not force either.
                let (bullet_id, arm) = self
                    .sim
                    .projectiles
                    .iter()
                    .find(|(_, bullet)| {
                        bullet.source_id() == self.squid
                            && bullet.target()
                                == crate::sim::projectile::ProjectileTarget::Entity(self.ship)
                    })
                    .map(|(&id, bullet)| {
                        assert!(bullet.ranged_fuse());
                        assert_eq!(
                            bullet.arm_timer().duration(),
                            self.rules.projectile("SQDJUMP").unwrap().arm
                        );
                        (id, bullet.arm_timer())
                    })
                    .expect("the actual armed SQDJUMP remains represented after its stock launch");
                for _ in 0..=arm.duration().max(0) {
                    if self
                        .sim
                        .substrate
                        .entities
                        .get(self.squid)
                        .unwrap()
                        .parasite
                        .class()
                        .unwrap()
                        .victim()
                        == Some(self.ship)
                    {
                        break;
                    }
                    let pending =
                        self.sim.projectiles.get(bullet_id).expect(
                            "pending SQDJUMP must survive until its real fuse/impact visit",
                        );
                    assert_eq!(pending.source_id(), self.squid);
                    assert_eq!(
                        pending.target(),
                        crate::sim::projectile::ProjectileTarget::Entity(self.ship)
                    );
                    self.step();
                    let source = self.sim.substrate.entities.get(self.squid).unwrap();
                    let target = self.sim.substrate.entities.get(self.ship).unwrap();
                    assert!(source.lifecycle.object_alive() && source.health.current() > 0);
                    assert!(source.lifecycle.in_limbo());
                    assert!(target.lifecycle.object_alive() && target.health.current() > 0);
                }
                let source = self.sim.substrate.entities.get(self.squid).unwrap();
                let target = self.sim.substrate.entities.get(self.ship).unwrap();
                assert_eq!(
                    source.parasite.class().unwrap().victim(),
                    Some(self.ship),
                    "after actual shot and native Arm visits: frame={} logic={:?} projectiles={:?} source_lifecycle={:?} source_parasite={:?} target_lifecycle={:?} target_parasite={:?} can_infect={} water_base={:?} target_tile={:?}",
                    self.sim.session.binary_frame(),
                    self.sim.substrate.logic.snapshot(),
                    self.sim.projectiles.iter().collect::<Vec<_>>(),
                    source.lifecycle,
                    source.parasite,
                    target.lifecycle,
                    target.parasite,
                    self.sim.parasite_can_infect(self.squid, Some(self.ship), &self.rules),
                    self.sim.resolved_terrain.as_ref().map(|terrain| terrain.projectile_water_set_base()),
                    self.sim.resolved_terrain.as_ref().and_then(|terrain| terrain.cell(target.position.rx(), target.position.ry())).map(|cell| cell.final_tile_index),
                );
                assert_eq!(target.parasite.eater(), Some(self.squid));
                return;
            }
        }
        panic!("stock Attack emitted no SquidGrab; attachment was not tested");
    }

    fn anim(&self) -> Option<u64> {
        self.sim
            .substrate
            .entities
            .get(self.squid)?
            .parasite
            .class()?
            .grapple_anim()
    }

    fn wait_for_roll(&mut self) -> u64 {
        for _ in 0..180 {
            self.step();
            if let Some(anim_id) = self.anim() {
                let anim = self.sim.anim(anim_id).expect("held SQDG is live");
                assert_eq!(self.sim.interner.resolve(anim.type_id()), "SQDG");
                assert_eq!(anim.owner_entity(), Some(self.ship));
                assert!((0..240).contains(&anim.runtime().current_frame));
                let target = self.sim.substrate.entities.get(self.ship).unwrap();
                assert!(
                    target
                        .parasite
                        .is_paralyzed(self.sim.session.binary_frame())
                );
                if target
                    .rocking
                    .as_ref()
                    .is_some_and(|r| r.angle_sideways().abs() > SimFixed::lit("0.005"))
                {
                    assert_eq!(
                        anim.runtime().frame_timer.duration(),
                        128,
                        "manager, not independent ART cadence"
                    );
                    return anim_id;
                }
            }
        }
        panic!("attached stock Squid never produced its visible roll and SQDG");
    }

    fn assert_released(&self, old_anim: u64) {
        let source = self
            .sim
            .substrate
            .entities
            .get(self.squid)
            .expect("released squid survives");
        assert!(source.lifecycle.object_alive() && source.health.current() > 0);
        assert!(!source.lifecycle.in_limbo());
        let state = source.parasite.class().unwrap();
        assert_eq!(state.victim(), None);
        assert_eq!(state.grapple_anim(), None);
        assert_eq!(state.grapple_progress_for_test(), (0, 0, 0, false));
        let target = self.sim.substrate.entities.get(self.ship).unwrap();
        assert_eq!(target.parasite.eater(), None);
        assert_eq!(target.parasite.paralysis().duration(), 0);
        assert_eq!(
            target.rocking.as_ref().unwrap().angle_sideways(),
            SimFixed::ZERO
        );
        // Destroy is synchronous, physical removal remains deferred.
        assert!(
            self.sim.anim(old_anim).is_none()
                || self.sim.substrate.pending_delete.contains_object(&old_anim)
        );
    }

    fn receive(&mut self, victim: u64, warhead_name: &str, damage: i32) {
        // Explicit direct receiver boundary for a third-party release/death;
        // does not claim an ordinary Dolphin or another weapon's launch chain.
        let warhead = self.sim.interner.intern(warhead_name);
        let event = EntityDamageEvent::direct_receiver(
            victim,
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
}

#[test]
fn stock_squid_attack_refuses_an_already_iron_curtained_destroyer() {
    use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};

    let Some(mut scene) = Encounter::retail(false) else {
        return;
    };
    let duration = scene.rules.general.iron_curtain_duration;
    assert!(duration > 60);
    // Explicit retained Techno70E2B0 timer prior, before the queued ordinary
    // Attack is delivered. This control does not claim a superweapon launch.
    apply_invulnerability(
        scene.sim.substrate.entities.get_mut(scene.ship).unwrap(),
        scene.sim.session.binary_frame(),
        duration,
        InvulnKind::IronCurtain,
    );
    // GetFireError6FCAFA..6FCB21 refuses a Parasite warhead against an
    // already-curtained Foot. The admitted companion uses this same route.
    for _ in 0..60 {
        let shots = scene.step();
        assert!(
            shots.iter().all(|shot| shot.attacker_id != scene.squid),
            "curtained host must refuse the actual stock Squid shot"
        );
        let source = scene.sim.substrate.entities.get(scene.squid).unwrap();
        let target = scene.sim.substrate.entities.get(scene.ship).unwrap();
        assert!(source.lifecycle.object_alive() && source.health.current() > 0);
        assert!(target.lifecycle.object_alive());
        assert!(!source.lifecycle.in_limbo());
        assert_eq!(source.parasite.class().unwrap().victim(), None);
        assert_eq!(source.parasite.class().unwrap().grapple_anim(), None);
        assert_eq!(target.health.current(), 600);
        assert_eq!(target.parasite.eater(), None);
        assert!(crate::sim::superweapon::invulnerability::is_invulnerable(
            target.invulnerability.as_ref(),
            scene.sim.session.binary_frame(),
        ));
    }
    assert!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.squid)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "native FIRE_ILLEGAL gives up the refused Attack"
    );
}

fn entire_grapple(elite: bool) {
    let Some(mut scene) = Encounter::retail(elite) else {
        return;
    };
    scene.attach_after_actual_shot();
    let anim = scene.wait_for_roll();
    let mut bites = 0;
    let mut culled = false;
    let mut previous_health = scene
        .sim
        .substrate
        .entities
        .get(scene.ship)
        .unwrap()
        .health
        .current();
    for _ in 0..2400 {
        let previous_z = position_world_coord(
            &scene
                .sim
                .substrate
                .entities
                .get(scene.ship)
                .unwrap()
                .position,
        )
        .z;
        scene.step();
        let ship = scene
            .sim
            .substrate
            .entities
            .get(scene.ship)
            .expect("hull remains during grapple");
        if ship.sinking.is_active() {
            assert_eq!(
                ship.health.current(),
                previous_health,
                "cull preserves positive health"
            );
            assert!(ship.lifecycle.object_alive());
            let threshold = if elite {
                scene.rules.general.condition_yellow
            } else {
                scene.rules.general.condition_red
            };
            let strength = scene.rules.object("DEST").unwrap().strength;
            assert_ne!(
                ship.health.compare_ratio(strength, threshold),
                crate::util::native_x87::MaskedX87Ordering::Greater
            );
            assert_eq!(
                position_world_coord(&ship.position).z,
                previous_z - 5,
                "cull sinks in the victim's same Unit turn"
            );
            scene.assert_released(anim);
            assert!(
                !scene
                    .sim
                    .substrate
                    .entities
                    .get(scene.squid)
                    .unwrap()
                    .parasite
                    .is_paralyzed(scene.sim.session.binary_frame())
            );
            culled = true;
            break;
        }
        if ship.health.current() < previous_health {
            bites += 1;
            previous_health = ship.health.current();
        }
        assert_eq!(ship.parasite.eater(), Some(scene.squid));
        assert!(ship.parasite.is_paralyzed(scene.sim.session.binary_frame()));
    }
    assert!(culled, "whole grapple must reach culling, not only attach");
    assert!(
        bites >= 2,
        "ordinary surviving receiver must take repeated grapple bites"
    );
    for _ in 0..150 {
        scene.step();
        if !scene.sim.substrate.entities.contains(scene.ship) {
            break;
        }
    }
    assert!(
        !scene.sim.substrate.entities.contains(scene.ship),
        "sinking hull reaches terminal cleanup"
    );
    assert!(scene.sim.anim(anim).is_none());
    let source = scene.sim.substrate.entities.get(scene.squid).unwrap();
    assert!(source.lifecycle.object_alive() && !source.lifecycle.in_limbo());
    assert_eq!(source.parasite.class().unwrap().victim(), None);
    assert_eq!(source.parasite.class().unwrap().grapple_anim(), None);
}

#[test]
fn stock_normal_and_elite_squid_grapples_roll_bite_cull_and_retire_the_ship() {
    for elite in [false, true] {
        entire_grapple(elite);
    }
}

#[test]
fn stock_squid_sonic_receiver_releases_live_roll_without_stale_animation() {
    let Some(mut scene) = Encounter::retail(false) else {
        return;
    };
    scene.attach_after_actual_shot();
    let anim = scene.wait_for_roll();
    assert!(scene.rules.warhead("SonicWarhead").unwrap().sonic);
    scene.receive(scene.ship, "SonicWarhead", 1);
    scene.assert_released(anim);
    assert!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.squid)
            .unwrap()
            .parasite
            .is_paralyzed(scene.sim.session.binary_frame()),
        "ordinary ExitUnit retains release paralysis"
    );
    scene.step();
    assert!(scene.sim.anim(anim).is_none());
}

#[test]
fn stock_squid_iron_curtain_releases_host_then_kills_revealed_organic_owner() {
    let Some(mut scene) = Encounter::retail(false) else {
        return;
    };
    scene.attach_after_actual_shot();
    let anim = scene.wait_for_roll();
    let ship = scene.sim.substrate.entities.get(scene.ship).unwrap();
    let (owner, rx, ry) = (ship.owner(), ship.position.rx(), ship.position.ry());
    // Supplied charged Super boundary, reusing the existing launch fixture;
    // provider construction/charge provenance is outside this release test.
    let sw = crate::sim::superweapon::chronosphere_tests::charge_super(
        &mut scene.sim,
        owner,
        "IronCurtainSpecial",
    );
    assert!(crate::sim::superweapon::iron_curtain::launch(
        &mut scene.sim,
        &scene.rules,
        owner,
        rx,
        ry,
        sw,
        None,
        FrameEffects::default()
    ));
    // Native IC visits its center first, then the eight neighboring cells.
    // Center DEST runs ExitUnit; the revealed Organic SQD is now in one of
    // those later cells. FootIronCurtain4DEAEE applies Strength as C4 to it,
    // so release does not imply this owner survives the remainder of launch.
    let source = scene
        .sim
        .substrate
        .entities
        .get(scene.squid)
        .expect("released organic owner remains until the pending-delete drain");
    assert_eq!(
        source.health.current(),
        0,
        "IC's later cell visit must kill the revealed Organic SQD: source={source:?}"
    );
    assert!(
        scene
            .sim
            .substrate
            .pending_delete
            .contains_object(&scene.squid),
        "source lifecycle={:?}, parasite={:?}, cell=({}, {})",
        source.lifecycle,
        source.parasite,
        source.position.rx(),
        source.position.ry()
    );
    assert_eq!(source.parasite.class().unwrap().victim(), None);
    assert_eq!(source.parasite.class().unwrap().grapple_anim(), None);
    let host = scene.sim.substrate.entities.get(scene.ship).unwrap();
    assert_eq!(host.parasite.eater(), None);
    assert_eq!(host.parasite.paralysis().duration(), 0);
    assert_eq!(
        host.rocking.as_ref().unwrap().angle_sideways(),
        SimFixed::ZERO
    );

    assert!(crate::sim::superweapon::invulnerability::is_invulnerable(
        scene
            .sim
            .substrate
            .entities
            .get(scene.ship)
            .unwrap()
            .invulnerability
            .as_ref(),
        scene.sim.session.binary_frame()
    ));
    scene.step();
    assert!(scene.sim.anim(anim).is_none());
}

#[test]
fn stock_squid_third_party_victim_death_uninits_suppressed_owner_and_owned_anim() {
    let Some(mut scene) = Encounter::retail(false) else {
        return;
    };
    scene.attach_after_actual_shot();
    let anim = scene.wait_for_roll();
    scene.receive(scene.ship, "Super", 100_000);
    let host = scene.sim.substrate.entities.get(scene.ship).unwrap();
    // The fatal receiver records the loss before Unit737E43..737E58
    // restores Health1/Alive and retains the hull for sinking, not UnInit.
    assert!(host.kill_record.is_recorded());
    assert!(host.sinking.is_active());
    assert!(host.lifecycle.object_alive());
    assert_eq!(
        host.health.current(),
        1,
        "actual fatal receiver must retain the recorded sinking hull"
    );
    let source = scene
        .sim
        .substrate
        .entities
        .get(scene.squid)
        .expect("suppressed owner remains represented until deferred deletion");
    // FootReceiveDamage4D7374..73D1 arms suppression before the third-party
    // fatal receiver. PointerExpired62A2A3..62A2DD then UnInits a suppressed
    // non-protected eater, including Naval SQD; it does not unlimbo it or
    // write owner Health. This is distinct from native ExitUnit's Naval gate.
    assert!(
        source
            .parasite
            .class()
            .unwrap()
            .suppression()
            .remaining(scene.sim.session.binary_frame() as i32)
            > 0,
        "third-party fatal hit must establish suppression: source={source:?}"
    );
    assert!(
        scene
            .sim
            .substrate
            .pending_delete
            .contains_object(&scene.squid),
        "suppressed owner must actually enter UnInit: source={source:?}, host={:?}",
        scene.sim.substrate.entities.get(scene.ship)
    );
    assert_eq!(source.parasite.class().unwrap().victim(), None);
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.ship)
            .unwrap()
            .parasite
            .eater(),
        None
    );
    for _ in 0..3 {
        scene.step();
    }
    assert!(!scene.sim.substrate.entities.contains(scene.squid));
    assert!(
        scene.sim.anim(anim).is_none(),
        "manager destructor retires its owned SQDG"
    );
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.ship)
            .unwrap()
            .parasite
            .eater(),
        None
    );
}

#[test]
fn stock_squid_internal_snapshot_preserves_live_roll_progress_and_reference() {
    let Some(mut scene) = Encounter::retail(false) else {
        return;
    };
    scene.attach_after_actual_shot();
    let anim = scene.wait_for_roll();
    let progress = scene
        .sim
        .substrate
        .entities
        .get(scene.squid)
        .unwrap()
        .parasite
        .class()
        .unwrap()
        .grapple_progress_for_test();
    assert!(matches!(progress.0, 2 | 3) && progress.1 > 0);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&scene.sim, 0, 0, "squid-grapple", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert_eq!(
        loaded.scenario_rng.logical_state(),
        crate::sim::rng::SimRng::new(0).logical_state()
    );
    // Explicit Rust continuation cursor, not native/app SaveGame semantics.
    loaded.scenario_rng = scene.sim.scenario_rng.clone();
    let main = scene.sim.main_rng.snapshot_for_test();
    loaded.main_rng = main.clone().into();
    scene.sim.main_rng = main.into();
    loaded.mapgen_rng = scene.sim.mapgen_rng.clone();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        scene.sim.resolved_terrain.clone().unwrap(),
        scene.sim.terrain_speed_config.clone(),
        &scene.rules,
    );
    loaded.resolve_type_handles(&scene.rules);
    assert!(loaded.rebuild_dynamic_navigation(&scene.rules));
    let restored = loaded
        .substrate
        .entities
        .get(scene.squid)
        .unwrap()
        .parasite
        .class()
        .unwrap();
    assert_eq!(restored.victim(), Some(scene.ship));
    assert_eq!(restored.grapple_anim(), Some(anim));
    assert_eq!(restored.grapple_progress_for_test(), progress);
    assert_eq!(loaded.anim(anim).unwrap().owner_entity(), Some(scene.ship));
    for _ in 0..90 {
        scene.step();
        let commands = loaded.take_due_commands();
        loaded
            .advance_app_frame(
                &commands,
                Some(&scene.rules),
                None,
                33,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("internal restored grapple continuation");
        let live = scene
            .sim
            .substrate
            .entities
            .get(scene.squid)
            .unwrap()
            .parasite
            .class()
            .unwrap();
        let restored = loaded
            .substrate
            .entities
            .get(scene.squid)
            .unwrap()
            .parasite
            .class()
            .unwrap();
        assert_eq!(
            restored.grapple_progress_for_test(),
            live.grapple_progress_for_test()
        );
        assert_eq!(restored.victim(), live.victim());
        assert_eq!(restored.grapple_anim(), live.grapple_anim());
        assert_eq!(
            loaded
                .substrate
                .entities
                .get(scene.ship)
                .unwrap()
                .health
                .current(),
            scene
                .sim
                .substrate
                .entities
                .get(scene.ship)
                .unwrap()
                .health
                .current()
        );
        assert_eq!(
            loaded.anim(anim).unwrap().runtime().current_frame,
            scene.sim.anim(anim).unwrap().runtime().current_frame
        );
        assert_eq!(
            loaded.scenario_rng.logical_state(),
            scene.sim.scenario_rng.logical_state()
        );
    }
}

#[test]
fn stock_squid_cull_thresholds_include_red_and_elite_yellow_boundaries() {
    // Explicit initial hull HP boundary inputs; the ordinary stock encounter
    // above retains its full starting health and reaches these bands by bites.
    for (elite, health, should_cull) in [
        (false, 150, true),
        (false, 151, false),
        (true, 300, true),
        (true, 301, false),
    ] {
        let Some(mut scene) = Encounter::retail(elite) else {
            return;
        };
        assert_eq!(scene.rules.object("DEST").unwrap().strength, 600);
        assert_eq!(scene.rules.general.condition_red, 0.25);
        assert_eq!(scene.rules.general.condition_yellow, 0.5);
        scene
            .sim
            .substrate
            .entities
            .get_mut(scene.ship)
            .unwrap()
            .health
            .set_for_test(health);
        scene.attach_after_actual_shot();
        let mut reached = false;
        for _ in 0..180 {
            scene.step();
            let target = scene.sim.substrate.entities.get(scene.ship).unwrap();
            if target.sinking.is_active() || target.health.current() != health {
                assert_eq!(
                    target.sinking.is_active(),
                    should_cull,
                    "elite={elite}, supplied HP={health}"
                );
                if should_cull {
                    assert_eq!(
                        target.health.current(),
                        health,
                        "cull is not a fatal damage substitute"
                    );
                    assert!(target.lifecycle.object_alive());
                } else {
                    assert!(target.health.current() < health && target.health.current() > 0);
                    assert_eq!(target.parasite.eater(), Some(scene.squid));
                }
                reached = true;
                break;
            }
        }
        assert!(
            reached,
            "first full grapple cycle must reach damage/cull boundary"
        );
    }
}

#[test]
fn stock_squid_paralysis_refuses_an_ordinary_ship_move_command() {
    let Some(mut scene) = Encounter::retail(false) else {
        return;
    };
    scene.attach_after_actual_shot();
    scene.wait_for_roll();
    let ship = scene.sim.substrate.entities.get(scene.ship).unwrap();
    let initial = position_world_coord(&ship.position);
    scene.sim.queue_command(CommandEnvelope::new(
        ship.owner(),
        scene.sim.session.tick() + 1,
        Command::Move {
            entity_id: scene.ship,
            target_rx: 30,
            target_ry: 24,
        },
    ));
    for _ in 0..10 {
        scene.step();
        let ship = scene.sim.substrate.entities.get(scene.ship).unwrap();
        let actual = position_world_coord(&ship.position);
        assert_eq!((actual.x, actual.y), (initial.x, initial.y));
        assert!(ship.parasite.is_paralyzed(scene.sim.session.binary_frame()));
        assert_eq!(ship.parasite.eater(), Some(scene.squid));
    }
}

#[test]
fn stock_squid_owner_receiver_death_destroys_owned_anim_without_unparalyzing_host() {
    let Some(mut scene) = Encounter::retail(false) else {
        return;
    };
    scene.attach_after_actual_shot();
    let anim = scene.wait_for_roll();
    let paralysis = scene
        .sim
        .substrate
        .entities
        .get(scene.ship)
        .unwrap()
        .parasite
        .paralysis();
    scene.receive(scene.squid, "Super", 100_000);
    assert!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.squid)
            .is_none_or(|source| source.health.current() == 0
                && (scene
                    .sim
                    .substrate
                    .pending_delete
                    .contains_object(&scene.squid)
                    || !source.lifecycle.object_alive())),
        "direct receiver must actually kill the limbo owner before testing its destruction"
    );
    let host = scene.sim.substrate.entities.get(scene.ship).unwrap();
    assert_eq!(
        host.parasite.eater(),
        None,
        "dead-owner expiry clears the host backlink"
    );
    // Native Foot4D998C clears the dead eater link, not its victim's timer.
    // ParasiteDtor6293E0 owns Anim/progress cleanup, not ExitUnit here.
    assert_eq!(host.parasite.paralysis(), paralysis);
    for _ in 0..3 {
        scene.step();
    }
    assert!(!scene.sim.substrate.entities.contains(scene.squid));
    assert!(scene.sim.anim(anim).is_none());
    let host = scene.sim.substrate.entities.get(scene.ship).unwrap();
    assert_eq!(host.parasite.eater(), None);
    assert_eq!(host.parasite.paralysis(), paralysis);
    assert_eq!(
        host.rocking.as_ref().unwrap().angle_sideways(),
        SimFixed::ZERO,
        "next native Techno rocking visit clears zero-velocity roll after owner death"
    );
}

fn native_anim_runtime(anim: &crate::sim::anim_class::AnimObject) -> serde_json::Value {
    serde_json::json!({
        "current_frame": anim.runtime().current_frame,
        "frame_step": anim.runtime().frame_step,
        "delay_remaining": anim.runtime().delay_remaining,
        "rate_reload": anim.runtime().rate_reload,
        "frame_timer": [anim.runtime().frame_timer.start_frame(), anim.runtime().frame_timer.duration()],
        "loop_remaining": anim.runtime().loop_remaining,
        "first_ai_guard": u8::from(anim.runtime().first_ai_guard),
        "constructor_reverse": u8::from(anim.runtime().constructor_reverse),
        "inactive": u8::from(anim.runtime().inactive),
        "paused": u8::from(anim.runtime().paused),
    })
}

fn compare_native_anim(scene: &Encounter, id: u64, expected: &serde_json::Value, absolute: bool) {
    use serde_json::json;
    let anim = scene
        .sim
        .anim(id)
        .expect("native-observed Anim remains represented");
    assert_eq!(json!(anim.native_unique_id()), expected["native_id"]);
    let coord = if absolute {
        scene.sim.anim_absolute_coord(id).unwrap()
    } else {
        anim.world_coord()
    };
    assert_eq!(json!([coord.x, coord.y, coord.z]), expected["location"]);
    assert_eq!(json!(anim.draw_flags()), expected["draw_flags"]);
    assert_eq!(json!(anim.z_adjust()), expected["z_adjust"]);
    assert_eq!(native_anim_runtime(anim), expected["runtime"]);
    assert_eq!(
        json!(anim.bounce().is_some()),
        json!(expected["is_bouncing"] == 1)
    );
    assert!(!scene.sim.substrate.pending_delete.contains_object(&id));
    if let Some(registered) = expected.get("logic_registered") {
        assert_eq!(json!(anim.in_logic_vector()), *registered);
    }
    if let Some(layers) = expected.get("display_layers") {
        let actual: Vec<_> = scene
            .sim
            .display_layers()
            .layer_of(id)
            .map(|layer| layer.index())
            .into_iter()
            .collect();
        assert_eq!(json!(actual), *layers);
    }
}

fn compare_native_grapple_state(scene: &Encounter, expected: &serde_json::Value) {
    use serde_json::json;
    let source = scene.sim.substrate.entities.get(scene.squid).unwrap();
    let victim = scene.sim.substrate.entities.get(scene.ship).unwrap();
    let manager = source.parasite.class().unwrap();
    assert_eq!(manager.victim(), Some(scene.ship));
    assert_eq!(victim.parasite.eater(), Some(scene.squid));
    let progress = manager.grapple_progress_for_test();
    assert_eq!(
        json!([progress.0, progress.1, progress.2, i32::from(progress.3)]),
        json!([
            expected["manager"]["phase"],
            expected["manager"]["stage"],
            expected["manager"]["delay"],
            expected["manager"]["invalidated"],
        ])
    );
    let (suppression, damage) = manager.grapple_timers_for_test();
    for (actual, key) in [(suppression, "timer_2c"), (damage, "timer_38")] {
        assert_eq!(
            json!(actual.start_frame()),
            expected["manager"][key]["start"]
        );
        assert_eq!(
            json!(actual.duration()),
            expected["manager"][key]["duration"]
        );
        // Native middle timer dword is opaque stack residue, not modeled time.
    }
    match manager.grapple_anim() {
        Some(id) => {
            let anim = scene.sim.anim(id).unwrap();
            assert_eq!(
                json!(anim.native_unique_id()),
                expected["manager"]["anim_native_id"]
            );
            assert_eq!(scene.sim.interner.resolve(anim.type_id()), "SQDG");
            assert_eq!(anim.owner_entity(), Some(scene.ship));
            compare_native_anim(scene, id, &expected["held_anim"], false);
        }
        None => assert!(expected["manager"]["anim_native_id"].is_null()),
    }
    for (entity, key) in [(source, "source"), (victim, "victim")] {
        let native = &expected[key];
        let coord = position_world_coord(&entity.position);
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
        assert_eq!(json!([coord.x, coord.y, coord.z]), native["location"]);
        if let Some(facing) = native.get("primary_current_raw") {
            assert_eq!(
                json!(entity.body_facing_current(scene.sim.session.binary_frame())),
                *facing
            );
        }
    }
    let timer = victim.parasite.paralysis();
    assert_eq!(
        json!([timer.start_frame(), timer.duration()]),
        json!([
            expected["victim"]["paralysis_timer"][0],
            expected["victim"]["paralysis_timer"][2],
        ])
    );
    assert_eq!(
        json!(u8::from(victim.sinking.is_active())),
        expected["victim"]["sinking"]
    );
    let expected_roll = f32::from_bits(expected["victim"]["angle_bits"].as_u64().unwrap() as u32);
    // No sparse component is the native constructor's zero angle.
    let actual_roll = victim
        .rocking
        .as_ref()
        .map_or(0.0, |rocking| rocking.angle_sideways().to_num::<f64>());
    assert!(
        (actual_roll - f64::from(expected_roll)).abs() <= 1.0 / 65536.0,
        "frame {}: fixed roll {actual_roll} versus native {expected_roll}",
        scene.sim.session.binary_frame()
    );
}

/// Full original6297F0 visits and constructor/RNG controls, not a whole
/// native Logic/playback comparison. Both actors are normally admitted;
/// source Limbo + AttachTo are explicit production entry preparation. Actor
/// positions/facing/Water tile and all streams come from original readbacks.
#[test]
fn stock_squid_grapple_monotonic_visits_match_native_rng_state_and_anim_constructors() {
    use crate::sim::native_identity::NativeUniqueIdCursor;
    use crate::sim::rng::SimRng;
    use serde_json::{Value, json};
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/parasite_grapple_rng.json",
    ))
    .unwrap();
    let initial = &corpus["initial"];
    let Some(mut scene) = Encounter::retail_at(false, Some(initial)) else {
        return;
    };
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    for (name, input) in initial["assets"].as_object().unwrap() {
        let physical = assets
            .load_file_from_mix(name)
            .expect("same original animation input");
        assert_eq!(json!(physical.bytes.len()), input["bytes"], "{name}");
        assert_eq!(
            crate::util::sha256::sha256_hex(&physical.bytes),
            input["sha256"].as_str().unwrap(),
            "{name}"
        );
    }
    for native in initial["anim_types"].as_array().unwrap() {
        let name = native["name"].as_str().unwrap();
        let config = scene.rules.art().anim_runtime_config(name).unwrap();
        assert_eq!(
            json!(config.raw_shp_frame_count),
            native["raw_shp_frame_count"],
            "{name}"
        );
        assert_eq!(json!(config.rate_logic_frames), native["rate"], "{name}");
        assert!(
            !config.bouncer,
            "selected original constructors take no Bounce RNG"
        );
    }
    // Native legacy owner IDs both read zero, while real House slots0/1 and
    // alliance masks1/2 are distinct. Map them to the arena's rival Houses,
    // never infer a shared owner from the partial native ID field.
    assert_eq!(initial["actor_admissions"][0]["owner_slot"], 0);
    assert_eq!(initial["actor_admissions"][0]["owner_alliances"], 1);
    assert_eq!(initial["actor_admissions"][1]["owner_slot"], 1);
    assert_eq!(initial["actor_admissions"][1]["owner_alliances"], 2);
    assert_ne!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.squid)
            .unwrap()
            .owner(),
        scene
            .sim
            .substrate
            .entities
            .get(scene.ship)
            .unwrap()
            .owner()
    );
    scene
        .sim
        .session
        .set_binary_frame_for_test(initial["frame"].as_u64().unwrap() as u32);
    scene.sim.techno_limbo_with_context(
        scene.squid,
        super::UninitContext::with_rules(&scene.rules).with_effects(FrameEffects::default()),
    );
    assert!(
        scene
            .sim
            .parasite_can_infect(scene.squid, Some(scene.ship), &scene.rules)
    );
    scene.sim.parasite_attach(
        scene.squid,
        Some(scene.ship),
        &scene.rules,
        FrameEffects::default(),
    );
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.squid)
            .unwrap()
            .parasite
            .class()
            .unwrap()
            .victim(),
        Some(scene.ship)
    );

    let rows = corpus["rows"].as_array().unwrap();
    scene.sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
        initial["native_cursor"].as_u64().unwrap() as u32,
    ));
    scene.sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(rows[0]["rng_before"]["scenario"].as_str().unwrap());
    scene.sim.main_rng =
        SimRng::from_native_state_hex_for_test(rows[0]["rng_before"]["main"].as_str().unwrap())
            .into();
    scene.sim.mapgen_rng =
        SimRng::from_native_state_hex_for_test(rows[0]["rng_before"]["mapgen"].as_str().unwrap());
    let mut compared = 0;
    for frame in 0..=corpus["visits_end"].as_u64().unwrap() as u32 {
        scene.sim.session.set_binary_frame_for_test(frame);
        let row = rows.iter().find(|row| row["input"]["frame"] == frame);
        if let Some(row) = row {
            compare_native_grapple_state(&scene, &row["before"]);
            for (actual, key) in [
                (scene.sim.scenario_rng.native_state_hex(), "scenario"),
                (scene.sim.main_rng.native_state_hex(), "main"),
                (scene.sim.mapgen_rng.native_state_hex(), "mapgen"),
            ] {
                assert_eq!(
                    actual,
                    row["rng_before"][key].as_str().unwrap(),
                    "frame {frame} before/{key}"
                );
            }
            assert_eq!(
                json!(scene.sim.native_unique_ids.as_ref().unwrap().current_raw()),
                row["native_cursor_before"]
            );
        }
        let before: std::collections::BTreeSet<_> = scene.sim.anims().map(|(&id, _)| id).collect();
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            scene.sim.parasite_ai_for_victim(
                scene.ship,
                &scene.rules,
                None,
                FrameEffects::default(),
            );
        });
        if let Some(row) = row {
            let actual_words: Vec<_> = draws.iter().map(|draw| draw["value"].clone()).collect();
            let native_words: Vec<_> = row["rng_requests"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|request| request["raw_draws"].as_array().unwrap().iter().cloned())
                .collect();
            assert_eq!(
                actual_words, native_words,
                "frame {frame}: ordered raw RNG words, including rejection"
            );
            assert_eq!(json!(draws.len()), row["raw_draw_count"]);
            for (actual, key) in [
                (scene.sim.scenario_rng.native_state_hex(), "scenario"),
                (scene.sim.main_rng.native_state_hex(), "main"),
                (scene.sim.mapgen_rng.native_state_hex(), "mapgen"),
            ] {
                assert_eq!(
                    actual,
                    row["rng_after"][key].as_str().unwrap(),
                    "frame {frame} after/{key}"
                );
            }
            assert_eq!(
                json!(scene.sim.native_unique_ids.as_ref().unwrap().current_raw()),
                row["native_cursor_after"]
            );
            compare_native_grapple_state(&scene, &row["after"]);
            let births: Vec<_> = scene
                .sim
                .substrate
                .logic
                .as_slice()
                .iter()
                .copied()
                .filter(|id| !before.contains(id) && scene.sim.anim(*id).is_some())
                .collect();
            let expected = row["anims"].as_array().unwrap();
            assert_eq!(
                births.len(),
                expected.len(),
                "frame {frame}: synchronous constructor admissions"
            );
            for (id, native) in births.into_iter().zip(expected) {
                assert_eq!(
                    scene
                        .sim
                        .interner
                        .resolve(scene.sim.anim(id).unwrap().type_id()),
                    native["type"].as_str().unwrap()
                );
                compare_native_anim(&scene, id, native, true);
            }
            compared += 1;
        } else {
            assert!(
                draws.is_empty(),
                "unselected frame {frame} must not hide native RNG work"
            );
            assert!(
                scene.sim.anims().all(|(&id, _)| before.contains(&id)),
                "unselected frame {frame} must not hide constructors"
            );
        }
    }
    assert_eq!(compared, 8);
}
