//! Production MindControl pip submission after ordinary retail captures.
//! The shared test-only simulation fixture stays independent of presentation.

use crate::sim::arena_fixture::mastermind::{MindScene, captive_ring_ids, spark_ids};
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::command::{Command, CommandEnvelope};
use std::collections::BTreeSet;

fn pip_frames(scene: &MindScene, hovered: Option<u64>) -> Vec<usize> {
    let sim = scene.simulation();
    let Some(controller) = sim.entities().get(scene.mind()) else {
        return Vec::new();
    };
    super::scale_pip_draws(
        sim,
        controller,
        hovered,
        Some(sim.interner().resolve(controller.owner())),
        scene.rules(),
    )
    .into_iter()
    .map(|pip| pip.frame)
    .collect()
}

#[test]
fn stock_selected_mastermind_submits_real_capture_count_and_overload_pips() {
    let Some(mut scene) = MindScene::retail() else {
        return;
    };
    scene.select_controller();
    let mut observed_counts = BTreeSet::new();
    scene.capture_four_observing(|scene| {
        let controller = scene.simulation().entities().get(scene.mind()).unwrap();
        if controller.selection.is_selected() {
            let count = scene.victims().len();
            let mut expected = vec![0; 3];
            expected[..count.min(3)].fill(1);
            if count > 3 {
                expected.push(4);
            }
            assert_eq!(pip_frames(scene, None), expected);
            observed_counts.insert(count);
        }
    });
    assert_eq!(observed_counts, BTreeSet::from([0, 1, 2, 3, 4]));
    let owner = scene
        .simulation()
        .entities()
        .get(scene.mind())
        .unwrap()
        .owner();
    let controller = scene.simulation().entities().get(scene.mind()).unwrap();
    assert!(controller.selection.is_selected());
    assert!(controller.lifecycle.object_alive() && !controller.lifecycle.in_limbo());
    assert_eq!(
        controller.health.current(),
        scene.rules().object("MIND").unwrap().strength,
        "real four-capture feedback is observed before the first overload pulse"
    );
    assert_eq!(scene.victims().len(), 4);
    assert!(
        scene
            .simulation()
            .display_layers()
            .layer_of(scene.mind())
            .is_some()
    );
    // Native DrawPipScale709A90 case5: the current primary weapon supplies
    // three ordinary slots, then IsOverloading4726C0 adds exactly one idle
    // overload slot. Exercise the same status submission used by rendering,
    // after actual FireAt/CaptureUnit and ordinary selection/body admission.
    let frames: Vec<_> = crate::app::presentation::ui_overlays::scale_pip_draws(
        scene.simulation(),
        controller,
        None,
        Some(scene.simulation().interner().resolve(owner)),
        scene.rules(),
    )
    .into_iter()
    .map(|pip| pip.frame)
    .collect();
    assert_eq!(frames, [1, 1, 1, 4]);

    scene.queue_command(CommandEnvelope::new(
        owner,
        scene.simulation().session().tick() + 1,
        Command::Select {
            entity_ids: Vec::new(),
            additive: false,
        },
    ));
    scene.step();
    assert!(
        !scene
            .simulation()
            .entities()
            .get(scene.mind())
            .unwrap()
            .selection
            .is_selected()
    );
    assert!(pip_frames(&scene, None).is_empty());
    assert_eq!(pip_frames(&scene, Some(scene.mind())), [1, 1, 1, 4]);
}

#[test]
fn ordinary_overload_pip_uses_ten_manager_visits_and_queries_preserve_rng() {
    let Some(mut scene) = MindScene::retail() else {
        return;
    };
    scene.select_controller();
    scene.capture_four_through_real_attacks();
    let initial_health = scene
        .simulation()
        .entities()
        .get(scene.mind())
        .unwrap()
        .health
        .current();
    assert_eq!(pip_frames(&scene, None), [1, 1, 1, 4]);
    let mut saw_pulse = false;
    for _ in 0..180 {
        scene.step();
        let controller = scene.simulation().entities().get(scene.mind()).unwrap();
        if controller.health.current() < initial_health {
            assert_eq!(
                initial_health - controller.health.current(),
                scene.rules().mind_control.overload_damage[1]
            );
            saw_pulse = true;
            break;
        }
        assert_eq!(pip_frames(&scene, None), [1, 1, 1, 4]);
    }
    assert!(
        saw_pulse,
        "the feedback window follows an actual ordinary damage pulse"
    );
    // Native471ADF writes10 on the damaging manager visit.471A64 decrements
    // before the next countdown-return visits; the pip consumer70A17F reads
    // only positivity, without a separate presentation clock.
    for visits_since_pulse in 0..=10 {
        let expected = if visits_since_pulse < 10 { 3 } else { 4 };
        let before = (
            scene.simulation().main_rng().native_state_hex(),
            scene.simulation().scenario_rng().native_state_hex(),
            scene.simulation().mapgen_rng().native_state_hex(),
        );
        let manager_before = scene
            .simulation()
            .entities()
            .get(scene.mind())
            .unwrap()
            .capture_manager
            .clone();
        for _ in 0..4 {
            assert_eq!(pip_frames(&scene, None), [1, 1, 1, expected]);
            assert_eq!(pip_frames(&scene, Some(scene.mind())), [1, 1, 1, expected]);
        }
        assert_eq!(
            before,
            (
                scene.simulation().main_rng().native_state_hex(),
                scene.simulation().scenario_rng().native_state_hex(),
                scene.simulation().mapgen_rng().native_state_hex(),
            )
        );
        assert_eq!(
            manager_before,
            scene
                .simulation()
                .entities()
                .get(scene.mind())
                .unwrap()
                .capture_manager
        );
        if visits_since_pulse < 10 {
            scene.step();
        }
    }
}

#[test]
fn stock_captive_force_attack_death_reduces_overload_then_the_controller_settles() {
    let Some(mut scene) = MindScene::retail() else {
        return;
    };
    scene.select_controller();
    scene.capture_four_through_real_attacks();
    scene.first_pulse();
    let owner = scene
        .simulation()
        .entities()
        .get(scene.mind())
        .unwrap()
        .owner();
    let attacker = scene.targets()[0];
    let victim = scene.targets()[1];
    let rings_before = captive_ring_ids(&scene);
    assert_eq!(rings_before.len(), 4);
    eprintln!(
        "MIND reduction Attack issue_tick={} due_tick={} attacker={attacker} target={victim}",
        scene.simulation().session().tick(),
        scene.simulation().session().tick() + 1
    );
    scene.queue_command(CommandEnvelope::new(
        owner,
        scene.simulation().session().tick() + 1,
        Command::Attack {
            attacker_id: attacker,
            target_id: victim,
        },
    ));
    let weapon = scene.rules().weapon("105mm").unwrap();
    assert_eq!(weapon.projectile.as_deref(), Some("Cannon"));
    assert_eq!(weapon.warhead.as_deref(), Some("AP"));
    assert!(!scene.rules().projectile("Cannon").unwrap().inviso);
    let ap = scene.rules().warhead("AP").unwrap();
    assert!(ap.affects_allies, "actual retail AP permits allied damage");
    assert!(
        !scene.rules().object("MTNK").unwrap().type_immune,
        "retail MTNK is not same-type immune"
    );
    let mut flight = std::collections::VecDeque::new();
    let mut genuine_fire = false;
    let mut reduced = false;
    for _ in 0..500 {
        let shots = scene.step();
        let controller = scene
            .simulation()
            .entities()
            .get(scene.mind())
            .expect("ordinary captive destruction must precede fatal overload");
        assert!(controller.lifecycle.object_alive() && controller.health.current() > 0);
        for shot in shots.iter().filter(|shot| {
            shot.attacker_id == attacker && shot.target == TargetKind::Entity(victim)
        }) {
            assert_eq!(
                scene.simulation().interner().resolve(shot.weapon_id),
                "105mm"
            );
            assert_eq!(shot.weapon_slot, WeaponSlot::Primary);
            genuine_fire = true;
            assert!(crate::map::playfield::size_diamond_contains(
                scene.simulation().playfield_bounds().unwrap().base,
                scene.simulation().playfield_size_height().unwrap(),
                (
                    (shot.fire_coord.x / 256) as i16,
                    (shot.fire_coord.y / 256) as i16,
                ),
            ));
            eprintln!(
                "MIND reduction105mm visited_frame={} fire={:?} target_hp={:?} controller_hp={}",
                scene.simulation().session().binary_frame() - 1,
                shot.fire_coord,
                scene
                    .simulation()
                    .entities()
                    .get(victim)
                    .map(|actor| actor.health.current()),
                controller.health.current()
            );
        }
        if genuine_fire {
            flight.push_back(format!(
                "frame={} target={:?} shooter={:?} bullets={:?}",
                scene.simulation().session().binary_frame() - 1,
                scene.simulation().entities().get(victim).map(|actor| (
                    actor.health.current(),
                    crate::sim::movement::ground_pose::position_world_coord(&actor.position)
                )),
                scene.simulation().entities().get(attacker).map(|actor| {
                    crate::sim::movement::ground_pose::position_world_coord(&actor.position)
                }),
                scene
                    .simulation()
                    .projectiles()
                    .iter()
                    .filter(|(_, bullet)| bullet.source_id() == attacker)
                    .map(|(id, bullet)| (
                        id,
                        bullet.position(),
                        bullet.launch_origin(),
                        bullet.launch_target(),
                        bullet.target(),
                        bullet.velocity(),
                        bullet.payload()
                    ))
                    .collect::<Vec<_>>()
            ));
            if flight.len() > 24 {
                flight.pop_front();
            }
        }
        if scene.victims().len() == 3 {
            assert_eq!(pip_frames(&scene, None), [1, 1, 1]);
            assert!(
                genuine_fire,
                "ordinary friendly-target105mm shots must cause node removal"
            );
            assert!(
                scene
                    .simulation()
                    .entities()
                    .get(victim)
                    .is_none_or(
                        |actor| actor.health.current() == 0 && !actor.lifecycle.object_alive()
                    )
            );
            assert!(!scene.victims().contains(&victim));
            eprintln!(
                "MIND reduction completed visited_frame={} Stop issue_tick={} due_tick={} controller_hp={}",
                scene.simulation().session().binary_frame() - 1,
                scene.simulation().session().tick(),
                scene.simulation().session().tick() + 1,
                controller.health.current()
            );
            scene.queue_command(CommandEnvelope::new(
                owner,
                scene.simulation().session().tick() + 1,
                Command::Stop {
                    entity_id: attacker,
                },
            ));
            reduced = true;
            break;
        }
    }
    assert!(
        reduced,
        "a real Attack must destroy one captive before the controller overloads fatally:\n{}",
        flight.iter().cloned().collect::<Vec<_>>().join("\n")
    );
    let mut retained_health = scene
        .simulation()
        .entities()
        .get(scene.mind())
        .unwrap()
        .health
        .current();
    for _ in 0..180 {
        scene.step();
        let controller = scene.simulation().entities().get(scene.mind()).unwrap();
        assert!(controller.lifecycle.object_alive());
        assert!(
            controller.health.current() >= retained_health,
            "three retained nodes select the nondamaging row while native self-healing continues"
        );
        retained_health = controller.health.current();
        assert_eq!(scene.victims().len(), 3);
        assert_eq!(pip_frames(&scene, None), [1, 1, 1]);
    }
    assert!(
        scene.words().iter().all(|word| word & 0x7fff_ffff == 0),
        "removing a node must allow the existing native rocking owner to settle naturally"
    );
    assert!(!scene.simulation().entities().contains(victim));
    let remaining_rings = captive_ring_ids(&scene);
    assert_eq!(remaining_rings.len(), 3);
    assert_eq!(
        rings_before.difference(&remaining_rings).count(),
        1,
        "the dead captive's actual owned ring must retire without erasing surviving links"
    );
}

#[test]
fn stock_fatal_overload_releases_real_captives_and_retires_rings_and_sparks() {
    let Some(mut scene) = MindScene::retail() else {
        return;
    };
    scene.select_controller();
    scene.capture_four_through_real_attacks();
    let original = scene.simulation().interner().get("Russians").unwrap();
    let rings = captive_ring_ids(&scene);
    assert_eq!(rings.len(), 4);
    let damage = scene.rules().mind_control.overload_damage[1];
    let mut previous_health = scene.rules().object("MIND").unwrap().strength;
    let mut pulses = 0;
    let mut fatal = false;
    for _ in 0..720 {
        let before = spark_ids(&scene);
        let before_words = scene.words();
        let shots = scene.step();
        let health = scene
            .simulation()
            .entities()
            .get(scene.mind())
            .filter(|actor| actor.lifecycle.object_alive())
            .map_or(0, |actor| actor.health.current());
        if health < previous_health {
            let after = spark_ids(&scene);
            eprintln!(
                "MIND actual HPdrop visited_frame={} previous={} health={} words_before={before_words:?} words_after={:?} new_sparks={} shots={:?}",
                scene.simulation().session().binary_frame() - 1,
                previous_health,
                health,
                scene
                    .simulation()
                    .entities()
                    .get(scene.mind())
                    .map(|_| scene.words()),
                after.difference(&before).count(),
                shots
                    .iter()
                    .map(|shot| (
                        shot.attacker_id,
                        scene.simulation().interner().resolve(shot.weapon_id),
                        shot.target
                    ))
                    .collect::<Vec<_>>()
            );
            if health > 0 {
                assert_eq!(
                    previous_health - health,
                    damage,
                    "actual nonlethal ordinary C4 pulse damage"
                );
            } else {
                assert!(
                    previous_health <= damage,
                    "a fatal50 packet can clip the remaining live HP"
                );
            }
            assert_eq!(
                after.difference(&before).count(),
                5,
                "even the fatal concrete receiver returns to five real SparkSys constructors"
            );
            pulses += 1;
            eprintln!(
                "MIND natural overload visited_frame={} pulse={pulses} health={health}",
                scene.simulation().session().binary_frame() - 1
            );
        }
        if health == 0 {
            assert!(
                pip_frames(&scene, None).is_empty(),
                "fatal controller cleanup cannot retain a pip strip"
            );
            assert!(
                previous_health > 0 && previous_health <= damage,
                "real preceding pulses and self-healing reached the fatal boundary"
            );
            assert!(
                scene
                    .simulation()
                    .entities()
                    .get(scene.mind())
                    .is_none_or(
                        |actor| actor.health.current() == 0 && !actor.lifecycle.object_alive()
                    )
            );
            for &id in &scene.targets() {
                let actor = scene.simulation().entities().get(id).unwrap();
                assert_eq!(actor.owner(), original);
                assert_eq!(actor.mind_control.controller(), None);
                assert!(!actor.mind_control.is_mind_controlled());
            }
            assert!(
                captive_ring_ids(&scene).is_empty(),
                "real FreeAll releases rings inside the fatal receiver"
            );
            fatal = true;
            break;
        }
        previous_health = health;
    }
    assert!(
        fatal,
        "ordinary overload pulses must eventually kill the stock controller"
    );
    assert!(
        pulses > 1,
        "actual repeated pulses, including healing intervals, must precede fatal overload"
    );
    // Released AI units receive real Stop inputs so their native fate/Hunt
    // does not turn the cleanup observation into a second distant battle.
    // The human Americans send them: the DoList skips computer Houses' events
    // (0x0064C4FB..0x0064C50D), and IDLE never compares sender and actor
    // (0x004C74CB..0x004C76BB).
    let sender = scene.simulation().interner().get("Americans").unwrap();
    for &id in &scene.targets() {
        scene.queue_command(CommandEnvelope::new(
            sender,
            scene.simulation().session().tick() + 1,
            Command::Stop { entity_id: id },
        ));
    }
    for _ in 0..240 {
        scene.step();
    }
    assert!(!scene.simulation().entities().contains(scene.mind()));
    assert!(pip_frames(&scene, Some(scene.mind())).is_empty());
    assert!(
        rings
            .iter()
            .all(|&id| scene.simulation().anim(id).is_none())
    );
    assert!(
        spark_ids(&scene).is_empty(),
        "actual overload systems/particles must finish through their normal owners"
    );
    assert!(
        scene
            .simulation()
            .projectiles()
            .iter()
            .all(|(_, bullet)| bullet.source_id() != scene.mind())
    );
}

#[test]
fn internal_snapshot_continues_real_overload_links_rocking_and_next_pulse() {
    let Some(mut scene) = MindScene::retail() else {
        return;
    };
    scene.select_controller();
    scene.capture_four_through_real_attacks();
    scene.first_pulse();
    for _ in 0..6 {
        scene.step();
    }
    let before = scene.words();
    assert!(before[0] & 0x7fff_ffff != 0);
    assert_eq!(pip_frames(&scene, None), [1, 1, 1, 3]);
    let mut restored = scene.internal_snapshot_continuation();
    assert_eq!(restored.words(), before);
    assert_eq!(pip_frames(&restored, None), pip_frames(&scene, None));
    let mut saw_next_pulse = false;
    let health = scene
        .simulation()
        .entities()
        .get(scene.mind())
        .unwrap()
        .health
        .current();
    for _ in 0..70 {
        scene.step();
        restored.step();
        let live = scene.simulation().entities().get(scene.mind()).unwrap();
        let replay = restored
            .simulation()
            .entities()
            .get(restored.mind())
            .unwrap();
        assert_eq!(live.health.current(), replay.health.current());
        assert_eq!(pip_frames(&scene, None), pip_frames(&restored, None));
        assert_eq!(scene.words(), restored.words());
        assert_eq!(
            live.capture_manager, replay.capture_manager,
            "owned countdown/voice/node state continues"
        );
        assert_eq!(spark_ids(&scene), spark_ids(&restored));
        assert_eq!(
            scene.simulation().main_rng().native_state_hex(),
            restored.simulation().main_rng().native_state_hex()
        );
        assert_eq!(
            scene.simulation().scenario_rng().native_state_hex(),
            restored.simulation().scenario_rng().native_state_hex()
        );
        assert_eq!(
            scene.simulation().mapgen_rng().native_state_hex(),
            restored.simulation().mapgen_rng().native_state_hex()
        );
        for &id in &scene.targets() {
            let a = scene.simulation().entities().get(id).unwrap();
            let b = restored.simulation().entities().get(id).unwrap();
            assert_eq!((a.owner(), a.mind_control), (b.owner(), b.mind_control));
        }
        if live.health.current() < health {
            saw_next_pulse = true;
        }
    }
    assert!(
        saw_next_pulse,
        "continuation must pass a real new overload pulse"
    );
}
