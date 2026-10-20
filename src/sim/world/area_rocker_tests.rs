//! Stock Kirov bomb -> ordinary Bullet469A3F -> AreaDamage489280 ->
//! post-receiver Rocker489B49 -> shared ApplyRocker70B280/RockingUpdate70B570.
//! The open ground and distant unarmed survivors are supplied map inputs.

use super::{FrameEffects, Simulation, TickLane};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::movement::ground_pose::position_world_coord;
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
        .expect("ordinary retail Kirov frame");
    assert!(output.tick.frame_committed);
    output.fire_events
}

#[test]
fn stock_kirov_bomb_impact_rocks_its_live_neighbor_then_rolls_it() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut rules = retail.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    let zep = rules.object("ZEP").unwrap();
    assert_eq!(zep.primary(), Some("BlimpBomb"));
    assert!(zep.balloon_hover && zep.considered_aircraft);
    let bomb = rules.weapon("BlimpBomb").unwrap();
    assert_eq!(bomb.projectile.as_deref(), Some("BlimpBombP"));
    assert_eq!(bomb.warhead.as_deref(), Some("BlimpHE"));
    let projectile = rules.projectile("BlimpBombP").unwrap();
    assert!(projectile.vertical && projectile.voxel);
    // Read the real resolved Bullet image and physical model, not an ART
    // lookup under the projectile's rules name or a supplied loaded flag.
    let (model, _) = crate::rules::voxel_asset_catalog::load_main_voxel(
        &assets,
        projectile.voxel_image_load().unwrap(),
        Some(&rules),
    )
    .expect("physical retail Kirov bomb model");
    assert!(!model.vxl().limbs.is_empty());
    let warhead = rules.warhead("BlimpHE").unwrap();
    assert!(warhead.rocker);
    assert!(warhead.cell_spread_f64 > 0.0);
    assert!(rules.object_has_loaded_voxel(rules.type_handle("MTNK").unwrap()));
    let strength = rules.object("MTNK").unwrap().strength;

    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 0x489b49, 64);
    // The arena supplies terrain/navigation; establish this visibility
    // domain before Unlimbo can create source and border viewer planes.
    sim.fog = crate::sim::vision::FogState::new(64, 64);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    let kirov = sim
        .spawn_object("ZEP", "Americans", 24, 24, 0, &rules)
        .expect("ordinary Kirov ground Unlimbo");
    let target = sim
        .spawn_object("MTNK", "Russians", 25, 24, 0, &rules)
        .expect("ordinary direct tank target");
    // Native Kirov flight controls and this ordinary encounter retain the
    // bomb's launch XY throughout its fall. Put the adjacent tank north of
    // the target, toward the actual drop, rather than assuming target-centered
    // impact coordinates. Both actors must still receive real nonlethal damage.
    let neighbor = sim
        .spawn_object("MTNK", "Russians", 25, 23, 0, &rules)
        .expect("ordinary neighboring tank one cell away");
    let truck = rules.object("TRUCKA").unwrap();
    assert!(truck.primary().is_none() && truck.secondary().is_none());
    for (house, x, y) in [("Americans", 4, 4), ("Russians", 58, 58)] {
        sim.spawn_object("TRUCKA", house, x, y, 0, &rules)
            .expect("ordinary unarmed house survivor");
    }
    let owner = sim.substrate.entities.get(kirov).unwrap().owner();
    let enemy = sim.substrate.entities.get(target).unwrap().owner();
    assert_ne!(owner, enemy);
    sim.session.set_current_house_for_test(Some(owner));
    for (x, y) in [(25, 24), (25, 23)] {
        sim.fog.mark_visible_for_owner(owner, x, y);
    }
    for id in [target, neighbor] {
        sim.queue_command(CommandEnvelope::new(
            enemy,
            1,
            Command::Guard {
                entity_id: id,
                target: None,
            },
        ));
    }
    let source = sim.substrate.entities.get(kirov).unwrap();
    assert_eq!(position_world_coord(&source.position).z, 0);
    let hover = source
        .locomotor
        .as_ref()
        .unwrap()
        .jumpjet_runtime()
        .unwrap()
        .params()
        .height();
    // An idle grounded Jumpjet does not advance its Process. A real Move
    // supplies its takeoff request; every height write remains flight-owned.
    sim.queue_command(CommandEnvelope::new(
        owner,
        1,
        Command::Move {
            entity_id: kirov,
            target_rx: 24,
            target_ry: 22,
        },
    ));
    let mut ascended = false;
    for _ in 0..600 {
        ordinary_frame(&mut sim, &rules);
        let source = sim.substrate.entities.get(kirov).unwrap();
        assert!(source.lifecycle.object_alive() && source.health.current() > 0);
        if position_world_coord(&source.position).z >= hover {
            ascended = true;
            break;
        }
    }
    assert!(
        ascended,
        "ground-admitted Kirov must naturally reach its retail hover height"
    );
    let tick = sim.session.tick() + 1;
    sim.queue_command(CommandEnvelope::new(
        owner,
        tick,
        Command::Stop { entity_id: kirov },
    ));
    sim.queue_command(CommandEnvelope::new(
        owner,
        tick,
        Command::Attack {
            attacker_id: kirov,
            target_id: target,
        },
    ));

    let mut actual_shot = false;
    let mut actual_bomb = false;
    let mut timeline = std::collections::VecDeque::new();
    for _ in 0..600 {
        let shots = ordinary_frame(&mut sim, &rules);
        for shot in shots.iter().filter(|shot| shot.attacker_id == kirov) {
            assert_eq!(sim.interner.resolve(shot.weapon_id), "BlimpBomb");
            assert_eq!(shot.weapon_slot, WeaponSlot::Primary);
            assert_eq!(shot.target, TargetKind::Entity(target));
            assert!(shot.fire_coord.z > 0);
            actual_shot = true;
        }
        actual_bomb |= sim.projectiles.iter().any(|(_, bullet)| {
            bullet.source_id() == kirov
                && bullet.target() == crate::sim::projectile::ProjectileTarget::Entity(target)
                && bullet.in_logic_vector()
                && bullet
                    .payload()
                    .projectile_type()
                    .is_some_and(|id| sim.interner.resolve(id) == "BlimpBombP")
        });
        let direct = sim
            .substrate
            .entities
            .get(target)
            .expect("first stock bomb is nonlethal");
        let adjacent = sim
            .substrate
            .entities
            .get(neighbor)
            .expect("first stock bomb preserves its neighboring tank");
        assert!(direct.lifecycle.object_alive() && direct.health.current() > 0);
        assert!(adjacent.lifecycle.object_alive() && adjacent.health.current() > 0);
        if actual_shot {
            timeline.push_back(format!(
                "frame={} kirov={:?} target={:?} neighbor={:?} shots={:?} bombs={:?}",
                sim.session.binary_frame() - 1,
                sim.substrate.entities.get(kirov).map(|actor| (
                    position_world_coord(&actor.position),
                    actor.mission.current()
                )),
                (
                    direct.health.current(),
                    position_world_coord(&direct.position),
                    direct.mission.current()
                ),
                (
                    adjacent.health.current(),
                    position_world_coord(&adjacent.position),
                    adjacent.mission.current()
                ),
                shots
                    .iter()
                    .map(|shot| (
                        shot.attacker_id,
                        sim.interner.resolve(shot.weapon_id),
                        shot.target,
                        shot.fire_coord
                    ))
                    .collect::<Vec<_>>(),
                sim.projectiles
                    .iter()
                    .filter(|(_, bullet)| bullet.source_id() == kirov)
                    .map(|(id, bullet)| (
                        id,
                        bullet.position(),
                        bullet.launch_origin(),
                        bullet.launch_target(),
                        bullet.velocity(),
                        bullet.trajectory()
                    ))
                    .collect::<Vec<_>>()
            ));
            if timeline.len() > 32 {
                timeline.pop_front();
            }
        }
        if direct.health.current() < strength {
            assert!(
                actual_shot && actual_bomb,
                "actual Primary fire and live physical bomb precede impact"
            );
            assert!(
                adjacent.health.current() < strength,
                "the real impact must reach the one-cell neighbor before testing its area rocker: actual_shot={actual_shot} actual_bomb={actual_bomb}\n{}",
                timeline.iter().cloned().collect::<Vec<_>>().join("\n")
            );
            // Meaningful first red: no receiver result, damage, model gate,
            // flight state, relationship, angle or rate was supplied.
            let rates = adjacent.rocking.as_ref().map_or([0; 2], |state| {
                let words = state.native_bits();
                [words[2], words[3]]
            });
            assert!(
                rates.iter().any(|word| word & 0x7fff_ffff != 0),
                "ordinary BlimpHE impact must assign its live neighbor an area rocker impulse; frame={} direct_hp={} neighbor_hp={}",
                sim.session.binary_frame() - 1,
                direct.health.current(),
                adjacent.health.current()
            );
            sim.queue_command(CommandEnvelope::new(
                owner,
                sim.session.tick() + 1,
                Command::Stop { entity_id: kirov },
            ));
            // Move away through the real command owner after Stop has cleared
            // BalloonHover's attack intent; later bombs must not keep shaking
            // the tanks while this impulse naturally decays.
            sim.queue_command(CommandEnvelope::new(
                owner,
                sim.session.tick() + 1,
                Command::Move {
                    entity_id: kirov,
                    target_rx: 4,
                    target_ry: 20,
                },
            ));
            ordinary_frame(&mut sim, &rules);
            let adjacent = sim.substrate.entities.get(neighbor).unwrap();
            assert!(adjacent.lifecycle.object_alive() && adjacent.health.current() > 0);
            let rock = adjacent.rocking.as_ref().unwrap();
            assert!(
                [rock.angle_sideways(), rock.angle_forwards()]
                    .iter()
                    .any(|angle| angle.abs() > SimFixed::ZERO),
                "next native Techno slot must integrate the blast's actual impulse"
            );
            let retained_health = adjacent.health.current();
            for _ in 0..240 {
                let later = ordinary_frame(&mut sim, &rules);
                assert!(
                    later.iter().all(|shot| shot.attacker_id != kirov),
                    "ordinary Stop/Move must bound the selected first bomb"
                );
                let adjacent = sim.substrate.entities.get(neighbor).unwrap();
                assert!(adjacent.lifecycle.object_alive());
                assert_eq!(adjacent.health.current(), retained_health);
            }
            let adjacent = sim.substrate.entities.get(neighbor).unwrap();
            assert!(
                adjacent
                    .rocking
                    .as_ref()
                    .unwrap()
                    .native_bits()
                    .iter()
                    .all(|word| word & 0x7fff_ffff == 0),
                "the native normal update must naturally settle the actual blast"
            );
            assert!(
                sim.projectiles
                    .iter()
                    .all(|(_, bullet)| bullet.source_id() != kirov),
                "the observed physical bomb must retire through its ordinary impact"
            );
            assert!(
                sim.substrate
                    .entities
                    .get(kirov)
                    .unwrap()
                    .lifecycle
                    .object_alive()
            );
            return;
        }
    }
    panic!(
        "ordinary stock Kirov failed to reach a real nonlethal bomb impact: shot={actual_shot} live_bomb={actual_bomb}\n{}",
        timeline.iter().cloned().collect::<Vec<_>>().join("\n")
    );
}

/// Declared AreaDamage-entry component fixtures. Stock types, physical models
/// and concrete receivers remain real; patches are explicit map-layer inputs
/// for gates and callback routes, not changes to the ordinary Kirov witness.
struct AreaScene {
    rules: RuleSet,
    sim: Simulation,
    overlays: crate::rules::overlay_types::OverlayTypeRegistry,
    source: u64,
    center: u64,
    neighbor: u64,
}

impl AreaScene {
    fn retail(patch: &str) -> Option<Self> {
        use crate::rules::ini_parser::IniFile;
        let fixture = if patch.is_empty() {
            crate::rules::retail_ini_fixture::retail_battle_rules()?
        } else {
            crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
                &IniFile::from_str(patch),
            )?
        };
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
            &fixture.processed_rules,
            Some(&fixture.fixed_art),
        );
        let mut rules = fixture.rules;
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
        rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        assert!(rules.object_has_loaded_voxel(rules.type_handle("MTNK").unwrap()));
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 0x489b49, 64);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.resolve_rule_animation_lists(&rules);
        let source = sim
            .spawn_object("DRON", "Americans", 20, 19, 0, &rules)
            .unwrap();
        let center = sim
            .spawn_object("MTNK", "Russians", 20, 20, 0, &rules)
            .unwrap();
        let neighbor = sim
            .spawn_object("MTNK", "Russians", 21, 20, 0, &rules)
            .unwrap();
        for (house, x, y) in [("Americans", 4, 4), ("Russians", 58, 58)] {
            sim.spawn_object("TRUCKA", house, x, y, 0, &rules)
                .expect("ordinary unarmed house survivor");
        }
        Some(Self {
            rules,
            sim,
            overlays,
            source,
            center,
            neighbor,
        })
    }

    fn impact(&self) -> crate::sim::projectile::ProjectileCoord {
        let p = position_world_coord(
            &self
                .sim
                .substrate
                .entities
                .get(self.center)
                .unwrap()
                .position,
        );
        crate::sim::projectile::ProjectileCoord::new(p.x, p.y, p.z)
    }

    fn health(&self, id: u64) -> i32 {
        self.sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .health
            .current()
    }

    fn words(&self, id: u64) -> [u32; 4] {
        self.sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .rocking
            .as_ref()
            .map_or([0; 4], |state| state.native_bits())
    }

    fn apply(&mut self, warhead: &str, damage: i32, source: Option<u64>, nested: bool) {
        self.apply_at(self.impact(), warhead, damage, source, nested);
    }

    fn apply_at(
        &mut self,
        impact: crate::sim::projectile::ProjectileCoord,
        warhead: &str,
        damage: i32,
        source: Option<u64>,
        nested: bool,
    ) {
        use crate::sim::combat::world_receiver;
        let id = self.sim.interner.intern(warhead);
        let owner = source
            .and_then(|id| self.sim.substrate.entities.get(id))
            .map(|entity| entity.owner());
        let origin = (
            source.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
            owner,
            id,
        );
        let warhead = self.rules.warhead(warhead).unwrap();
        if nested {
            let mut run = world_receiver::ReceiverRun::default();
            world_receiver::apply_area_damage_in_run(
                &mut self.sim,
                &mut run,
                &self.rules,
                Some(&self.overlays),
                impact,
                damage,
                warhead,
                origin,
            );
        } else {
            world_receiver::apply_area_damage(
                &mut self.sim,
                &self.rules,
                Some(&self.overlays),
                impact,
                damage,
                warhead,
                origin,
                FrameEffects::default(),
            );
        }
    }
}

fn has_rates(words: [u32; 4]) -> bool {
    words[2..].iter().any(|word| word & 0x7fff_ffff != 0)
}

#[test]
fn standalone_and_in_run_area_entries_commit_receivers_then_the_same_live_rocker() {
    let mut observations = Vec::new();
    for nested in [false, true] {
        let Some(mut scene) = AreaScene::retail("") else {
            return;
        };
        // A declared call boundary for the two shared entries: no simulated
        // caller, Bullet, receiver return or result state is supplied.
        scene.apply("BlimpHE", 250, Some(scene.source), nested);
        for id in [scene.center, scene.neighbor] {
            assert!(
                scene.health(id) > 0
                    && scene.health(id) < scene.rules.object("MTNK").unwrap().strength
            );
            assert!(
                has_rates(scene.words(id)),
                "entry nested={nested}, actor={id}"
            );
        }
        observations.push((
            scene.health(scene.center),
            scene.health(scene.neighbor),
            scene.words(scene.center),
            scene.words(scene.neighbor),
        ));
    }
    assert_eq!(
        observations[0], observations[1],
        "both real entry families share one continuation"
    );
}

#[test]
fn zero_and_scenario_no_damage_stop_before_receivers_and_area_rocking() {
    for (damage, no_damage) in [(0, false), (250, true)] {
        let Some(mut scene) = AreaScene::retail("") else {
            return;
        };
        scene.sim.session.set_no_damage_for_test(no_damage);
        let rng = (
            scene.sim.main_rng.native_state_hex(),
            scene.sim.scenario_rng.native_state_hex(),
            scene.sim.mapgen_rng.native_state_hex(),
        );
        scene.apply("BlimpHE", damage, Some(scene.source), false);
        for id in [scene.center, scene.neighbor] {
            assert_eq!(
                scene.health(id),
                scene.rules.object("MTNK").unwrap().strength
            );
            assert!(
                scene
                    .sim
                    .substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .rocking
                    .is_none()
            );
        }
        assert_eq!(
            rng,
            (
                scene.sim.main_rng.native_state_hex(),
                scene.sim.scenario_rng.native_state_hex(),
                scene.sim.mapgen_rng.native_state_hex()
            )
        );
    }
}

#[test]
fn central_iron_curtain_receipt_suppresses_the_area_continuation_only_when_isolated() {
    use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};
    // Native489280's near-center isolation receipt is restricted to
    // CellSpread<=0.5. Stock BlimpHE=2 protects an individual receiver but
    // does not return2 or suppress the later rocker continuation.
    for (patch, central, isolated) in [
        ("", true, false),
        ("", false, false),
        ("[BlimpHE]\nCellSpread=0.5\n", true, true),
    ] {
        let Some(mut scene) = AreaScene::retail(patch) else {
            return;
        };
        let protected = if central {
            scene.center
        } else {
            scene.neighbor
        };
        // A retained native Techno70E2B0 timer prior; this control makes no
        // claim about the IronCurtain superweapon's area/cell producer.
        apply_invulnerability(
            scene.sim.substrate.entities.get_mut(protected).unwrap(),
            scene.sim.session.binary_frame(),
            scene.rules.general.iron_curtain_duration,
            InvulnKind::IronCurtain,
        );
        scene.apply("BlimpHE", 250, Some(scene.source), false);
        assert_eq!(
            scene.health(protected),
            scene.rules.object("MTNK").unwrap().strength
        );
        if isolated {
            assert_eq!(
                scene.health(scene.neighbor),
                scene.rules.object("MTNK").unwrap().strength
            );
            assert_eq!(scene.words(scene.center), [0; 4]);
            assert_eq!(scene.words(scene.neighbor), [0; 4]);
        } else {
            let unprotected = if central {
                scene.neighbor
            } else {
                scene.center
            };
            assert!(scene.health(unprotected) < scene.rules.object("MTNK").unwrap().strength);
            assert!(has_rates(scene.words(scene.center)));
            assert!(
                has_rates(scene.words(scene.neighbor)),
                "an individually protected object still receives a nonisolated area's rocker"
            );
        }
    }
}

#[test]
fn real_map_layer_rocker_and_spread_gates_control_the_shared_continuation() {
    for patch in ["[BlimpHE]\nRocker=no\n", "[BlimpHE]\nCellSpread=0\n"] {
        let Some(mut scene) = AreaScene::retail(patch) else {
            return;
        };
        scene.apply("BlimpHE", 250, Some(scene.source), false);
        assert!(
            scene.health(scene.center) > 0
                && scene.health(scene.center) < scene.rules.object("MTNK").unwrap().strength
        );
        if scene.rules.warhead("BlimpHE").unwrap().rocker {
            assert_eq!(scene.rules.warhead("BlimpHE").unwrap().cell_spread_f64, 0.0);
            assert!(
                has_rates(scene.words(scene.center)),
                "nonnull source keeps the impact-cell arm"
            );
            assert_eq!(
                scene.health(scene.neighbor),
                scene.rules.object("MTNK").unwrap().strength
            );
        } else {
            assert!(scene.health(scene.neighbor) < scene.rules.object("MTNK").unwrap().strength);
            assert_eq!(scene.words(scene.center), [0; 4]);
        }
        assert_eq!(scene.words(scene.neighbor), [0; 4]);
    }
}

#[test]
fn synchronous_source_death_retains_its_raw_location_for_the_original_area_argument() {
    let mut observations = Vec::new();
    for damage_self in [false, true] {
        let Some(mut scene) = AreaScene::retail(if damage_self {
            "[DRON]\nDamageSelf=yes\n"
        } else {
            ""
        }) else {
            return;
        };
        let source_before = position_world_coord(
            &scene
                .sim
                .substrate
                .entities
                .get(scene.source)
                .unwrap()
                .position,
        );
        scene.apply("BlimpHE", 250, Some(scene.source), false);
        let source = scene
            .sim
            .substrate
            .entities
            .get(scene.source)
            .expect("UnInit retains raw storage until frame-tail destruction");
        assert_eq!(position_world_coord(&source.position), source_before);
        if damage_self {
            assert_eq!(
                source.health.current(),
                0,
                "actual shared area receiver must kill the authored DamageSelf source"
            );
            assert!(!source.lifecycle.object_alive());
            assert!(
                scene
                    .sim
                    .substrate
                    .pending_delete
                    .contains_object(&scene.source)
            );
            assert!(!source.lifecycle.cell_marked());
            assert!(!scene.sim.substrate.logic.as_slice().contains(&scene.source));
        } else {
            assert!(source.health.current() > 0 && source.lifecycle.object_alive());
        }
        assert!(scene.health(scene.center) > 0);
        assert!(has_rates(scene.words(scene.center)));
        observations.push(scene.words(scene.center));
        if damage_self {
            ordinary_frame(&mut scene.sim, &scene.rules);
            assert!(
                scene.sim.substrate.entities.get(scene.source).is_none(),
                "normal deferred drain eventually frees the original source"
            );
            assert!(
                scene
                    .sim
                    .substrate
                    .entities
                    .get(scene.center)
                    .unwrap()
                    .lifecycle
                    .object_alive()
            );
        }
    }
    assert_eq!(
        observations[0], observations[1],
        "callback expiry must not replace the call's nonnull source with impact or reject its retained coordinates"
    );
}

#[test]
fn real_nested_death_weapon_area_rocks_after_its_nonrocking_parent_receivers() {
    for explodes in [false, true] {
        // The outer authored warhead damages retail DRON's actual armor but has zero heavy
        // verses and no Rocker. Only its real nested BlimpBomb death weapon
        // can hurt/rock the neighboring stock MTNK actors.
        let patch = format!(
            "[Warheads]\n9000=AreaRockerTrigger\n\
            [AreaRockerTrigger]\nCellSpread=2\nPercentAtMax=1\nRocker=no\n\
            Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,100%,0%\n\
            [DRON]\nExplodes={}\nDeathWeapon=BlimpBomb\n",
            if explodes { "yes" } else { "no" }
        );
        let Some(mut scene) = AreaScene::retail(&patch) else {
            return;
        };
        let trigger = scene.rules.warhead("AreaRockerTrigger").unwrap();
        assert_eq!(
            trigger.verses_f64
                [crate::sim::combat::armor_index(&scene.rules.object("DRON").unwrap().armor)],
            1.0
        );
        assert_eq!(
            trigger.verses_f64
                [crate::sim::combat::armor_index(&scene.rules.object("MTNK").unwrap().armor)],
            0.0
        );
        assert!(!scene.rules.warhead("AreaRockerTrigger").unwrap().rocker);
        scene.apply("AreaRockerTrigger", 250, None, false);
        let source = scene.sim.substrate.entities.get(scene.source).unwrap();
        assert_eq!(
            source.health.current(),
            0,
            "real parent receiver must take the fatal path"
        );
        assert!(!source.lifecycle.object_alive());
        for id in [scene.center, scene.neighbor] {
            if explodes {
                assert!(
                    scene.health(id) > 0
                        && scene.health(id) < scene.rules.object("MTNK").unwrap().strength
                );
                assert!(
                    has_rates(scene.words(id)),
                    "only the synchronous nested area has an admitted rocker"
                );
            } else {
                assert_eq!(
                    scene.health(id),
                    scene.rules.object("MTNK").unwrap().strength
                );
                assert_eq!(scene.words(id), [0; 4]);
            }
        }
    }
}

#[test]
fn real_wall_routing_still_reaches_the_area_rocker_continuation() {
    let Some(mut scene) = AreaScene::retail("") else {
        return;
    };
    let wall = scene
        .overlays
        .id_for_name("GAWALL")
        .expect("retail concrete wall");
    scene.sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(64, 64));
    scene
        .sim
        .overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(19, 20, wall, 0);
    let impact = crate::sim::projectile::ProjectileCoord::new(19 * 256 + 128, 20 * 256 + 128, 0);
    assert!(crate::sim::combat::world_receiver::area_routes_to_wall(
        &scene.sim,
        Some(&scene.overlays),
        (19, 20),
        scene.rules.warhead("BlimpHE").unwrap()
    ));
    scene.apply_at(impact, "BlimpHE", 250, Some(scene.source), false);
    assert!(scene.health(scene.center) > 0);
    assert!(
        has_rates(scene.words(scene.center)),
        "wall routing suppresses the bridge tail, not the preceding area rocker"
    );
}

#[test]
fn negative_impact_uses_the_original_signed_cell_for_the_live_49_cell_span() {
    let Some(mut scene) = AreaScene::retail("") else {
        return;
    };
    let included = scene
        .sim
        .spawn_object("MTNK", "Russians", 2, 20, 0, &scene.rules)
        .expect("ordinary tank on the signed scan's last column");
    let excluded = scene
        .sim
        .spawn_object("MTNK", "Russians", 3, 20, 0, &scene.rules)
        .expect("ordinary tank just beyond the signed scan");
    // Original489309..489330 truncates -257/256 to signed cell-1, retaining
    // subcell-1. Its x-major seven-column continuation ends at +2, not +3.
    // These actors are outside the selected CellSpread2 damage radius but
    // within shared ApplyRocker's force/distance gate; no rate was supplied.
    scene.apply_at(
        crate::sim::projectile::ProjectileCoord::new(-257, 5248, 0),
        "BlimpHE",
        400,
        None,
        false,
    );
    assert!(
        has_rates(scene.words(included)),
        "the original signed scan includes cell2"
    );
    assert!(
        scene
            .sim
            .substrate
            .entities
            .get(excluded)
            .unwrap()
            .rocking
            .is_none(),
        "clamping the impact to cell0 incorrectly includes cell3"
    );
    for id in [included, excluded] {
        assert_eq!(
            scene.health(id),
            scene.rules.object("MTNK").unwrap().strength,
            "rocker traversal is distinct from the splash receiver radius"
        );
    }
}

#[test]
fn primary_wall_route_reads_the_fixed_stride_alias_before_the_real_wall_is_removed() {
    let Some(mut scene) = AreaScene::retail("[BlimpHE]\nWallAbsoluteDestroyer=yes\n") else {
        return;
    };
    let wall = scene.overlays.id_for_name("GAWALL").unwrap();
    scene.sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(64, 64));
    scene
        .sim
        .overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(5, 20, wall, 0);
    // Packed517,19 selects real Cell5,20 by fixed512 indexing. It is neither
    // rectangular out-of-bounds nor the shared Dummy. The primary route must
    // read that same Cell before the collector's real WAD removal mutates it.
    let route_before = crate::sim::combat::world_receiver::area_routes_to_wall(
        &scene.sim,
        Some(&scene.overlays),
        (517, 19),
        scene.rules.warhead("BlimpHE").unwrap(),
    );
    assert!(
        route_before,
        "the retained pre-receiver route is the real aliased wall"
    );
    scene.apply_at(
        crate::sim::projectile::ProjectileCoord::new(517 * 256 + 128, 19 * 256 + 128, 0),
        "BlimpHE",
        250,
        None,
        false,
    );
    assert_eq!(
        scene
            .sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(5, 20)
            .overlay_id,
        None,
        "actual area collection must remove the aliased concrete wall"
    );
    assert!(
        !crate::sim::combat::world_receiver::area_routes_to_wall(
            &scene.sim,
            Some(&scene.overlays),
            (517, 19),
            scene.rules.warhead("BlimpHE").unwrap(),
        ),
        "a post-receiver lookup cannot reconstruct the saved primary wall route"
    );
}
