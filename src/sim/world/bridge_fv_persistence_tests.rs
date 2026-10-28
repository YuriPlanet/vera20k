//! Save/load boundaries in the ordinary physical FV bridge-collapse chain.
use super::{AmbientAnims, command_paid_fv, prepared_paid_scene};
use crate::headless_scenario::{HeadlessScenario, SIM_TICK_MS};
use crate::rules::mission_data::MissionType;
use crate::sim::mission::MissionId;
use crate::sim::world::bridge_test_evidence::{
    assert_navigation_fields_equal, navigation_authority, rebuilt_graphs, restored_retail,
    retail_anytown_map_bytes, save_scene,
};
use crate::sim::world::{Simulation, TickLane};
use crate::util::save_codec;
use serde_json::{Value, json};

fn authority(sim: &Simulation, source: u64, expected_load: bool) -> Value {
    let mut actor = sim.substrate.entities.get(source).unwrap().clone();
    if expected_load {
        // Unit/Foot Load executes FootClass::Load's audio-reset tail
        // @ 0x004DB60D..0x004DB624, rebuilding Foot+0x544 and clearing the
        // logical MoveSound latch/countdown at +0x53C/+0x540. Compare the
        // saved authority with the original post-Load fields.
        actor.move_sound = crate::sim::world::MoveSoundState::default();
    }
    let bullets: Vec<_> = sim
        .projectiles
        .iter()
        .map(|(_, bullet)| {
            let mut bullet = bullet.clone();
            if expected_load {
                // Original Bullet Load46AE9C..46AEB0 restarts the timer;
                // the remaining saved guidance, launch and fuse state survives.
                bullet
                    .arm_timer_mut_for_test()
                    .start(sim.session.binary_frame() as i32, 0);
            }
            json!({"state":bullet,"in_logic":bullet.in_logic_vector()})
        })
        .collect();
    json!({
        "actor":actor,"actor_in_logic":actor.in_logic_vector.is_member(),
        "bullets":bullets,
        "anims":sim.substrate.anims.iter().map(|(_, anim)|
            json!({"state":anim,"in_logic":anim.in_logic_vector(),
                "building_slot":anim.building_slot(),"damage_fire_slot":anim.damage_fire_slot()})
        ).collect::<Vec<_>>(),
        "logic":sim.substrate.logic.snapshot(),"display":sim.substrate.display,
        "pending_delete":sim.substrate.pending_delete.object_ids(),
        "native_id_cursor":sim.native_unique_ids.as_ref().unwrap().current_raw(),
        "next_stable_id":sim.next_stable_object_id(),
        "clock":[sim.session.tick(), u64::from(sim.session.binary_frame())],
        "bridge_authority":crate::util::sha256::sha256_hex(
            &save_codec::serialize(&sim.bridge_state).unwrap()),
        "cell_memberships":crate::util::sha256::sha256_hex(
            &save_codec::serialize(&sim.substrate.occupancy).unwrap()),
    })
}

fn transient_ids(sim: &Simulation, ambient: &AmbientAnims) -> Vec<u64> {
    sim.projectiles
        .iter()
        .map(|(id, _)| *id)
        .chain(
            sim.substrate
                .anims
                .iter()
                .filter(|(id, _)| !ambient.contains(id))
                .map(|(id, _)| *id),
        )
        .collect()
}

fn settled(sim: &Simulation, source: u64, ambient: &AmbientAnims) -> bool {
    let actor = sim.substrate.entities.get(source).unwrap();
    actor.attack_target.target().is_none()
        && actor.navigation.nav_com().is_none()
        && actor
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .is_some_and(|drive| drive.destination().is_none() && drive.head_to().is_none())
        && actor.mission.current() == MissionId::from_known(MissionType::Guard)
        && actor.mission.queued() == MissionId::NONE
        && transient_ids(sim, ambient).is_empty()
        && sim.substrate.pending_delete.is_empty()
}

struct Checkpoint {
    name: &'static str,
    bytes: Vec<u8>,
    authority: Value,
    navigation: Value,
    transients: Vec<u64>,
    collapsed: bool,
}

fn checkpoint(
    scene: &HeadlessScenario,
    source: u64,
    ambient: &AmbientAnims,
    name: &'static str,
) -> Checkpoint {
    let sim = scene.sim();
    eprintln!(
        "FV restore checkpoint {name} before frame{}",
        sim.session.binary_frame()
    );
    Checkpoint {
        name,
        bytes: save_scene(scene, name),
        authority: authority(sim, source, true),
        navigation: navigation_authority(
            sim,
            (48..=59).flat_map(|y| (84..=90).map(move |x| (x, y))),
        ),
        transients: transient_ids(sim, ambient),
        collapsed: sim.overlay_grid.as_ref().unwrap().cell(87, 54).overlay_id == Some(232),
    }
}

/// Stock `[CombatDamage] BridgeStrength=1500` admits this 25-damage hit only
/// when the area-damage roll `RandomRanged(1, BridgeStrength)` @ 0x0065C7E0
/// (caller 0x0048A29F, `CMP EAX, damage; JGE` skip at 0x0048A2A4) is below 25,
/// so whether the stock bridge falls within the run depends on which Scenario
/// words reach the impacts. The v26 native packet collapsed at Logic 32 (roll
/// 10) only under its captured outside-call schedule; the current scene's
/// outside producers draw a different schedule and the original admission
/// rejects both stock hits (#2260). A stock-strength restore test therefore
/// tested the outside schedule, not save/load, and is retired.
///
/// RulesClass::ReadCombatDamage @ 0x0066CD66..0x0066CD86 reads this live map
/// layer over the stock value. With strength 1, RandomRanged(1, 1) returns 1
/// without drawing (`JZ` at 0x0065C7EF), so the first impact admits; the rest
/// of the original map is unchanged.
#[test]
#[ignore = "requires physical Anytown and retail TEMPERATE assets"]
fn retail_fv_pursuit_missiles_collapse_and_guard_survive_restore_with_map_strength_one() {
    let directory =
        crate::map::source::test_support::TestDirectory::new("fv-pursuit-bridge-strength-one");
    let mut bytes = retail_anytown_map_bytes();
    bytes.extend_from_slice(b"\r\n[CombatDamage]\r\nBridgeStrength=1\r\n");
    directory.write("XMP03T4.MAP", &bytes);
    let map_path = directory.path().join("XMP03T4.MAP");
    let pristine = {
        let (healthy, _) = prepared_paid_scene("healthy", Some(&map_path));
        healthy.sim().resolved_terrain.as_ref().unwrap().clone()
    };
    let (mut scene, ambient) = prepared_paid_scene("damaged", Some(&map_path));
    assert_eq!(
        scene.runtime.resources.rules.bridge_rules.strength, 1,
        "production map layer BridgeStrength"
    );
    // The v26 damaged case's declared seed 31 and FV coordinate; the
    // collapse no longer depends on the Scenario words the impacts draw.
    let source = command_paid_fv(&mut scene, [22400, 12414, 416], 31, 3);
    let mut checkpoints = Vec::new();
    for _ in 0..96 {
        let output = scene
            .runtime
            .advance_frame(
                &[],
                SIM_TICK_MS,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        let sim = scene.sim();
        let actor = sim.substrate.entities.get(source).unwrap();
        let name = match checkpoints.len() {
            0 if actor.attack_target.target().is_some()
                && actor.navigation.nav_com().is_some()
                && actor
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .is_some_and(|drive| {
                        drive.destination().is_some()
                            && drive.head_to().is_some()
                            && drive.track_valid()
                    }) =>
            {
                Some("pursuing")
            }
            1 if sim
                .projectiles
                .iter()
                .filter(|(_, bullet)| bullet.source_id() == source && bullet.guidance().is_some())
                .count()
                == 2 =>
            {
                Some("both_missiles")
            }
            2 if output.tick.bridge_state_changed
                && actor.attack_target.target().is_none()
                && sim.overlay_grid.as_ref().unwrap().cell(87, 54).overlay_id == Some(232) =>
            {
                Some("collapsed")
            }
            3 if settled(sim, source, &ambient) => Some("drained_guard"),
            _ => None,
        };
        if let Some(name) = name {
            checkpoints.push(checkpoint(&scene, source, &ambient, name));
        }
        if checkpoints.len() == 4 {
            break;
        }
    }
    assert_eq!(
        checkpoints.iter().map(|c| c.name).collect::<Vec<_>>(),
        ["pursuing", "both_missiles", "collapsed", "drained_guard"]
    );

    for saved in checkpoints {
        // Both loads receive the same live process input. Native Scenario Load
        // reseeds to zero; Main/MapGen retain the process cursors. Neither these
        // streams nor rebuilt hierarchy IDs must match an uninterrupted future.
        let first = restored_retail(&scene, &pristine, &saved.bytes);
        let mut second = restored_retail(&scene, &pristine, &saved.bytes);
        for restored in [&first, &second] {
            assert_eq!(
                authority(restored, source, false),
                saved.authority,
                "{} restored authority",
                saved.name
            );
            assert_navigation_fields_equal(
                &navigation_authority(
                    restored,
                    (48..=59).flat_map(|y| (84..=90).map(move |x| (x, y))),
                ),
                &saved.navigation,
                saved.name,
            );
            assert_eq!(
                restored.scenario_rng.logical_state(),
                crate::sim::rng::SimRng::new(0).logical_state()
            );
            assert_eq!(
                restored.main_rng.logical_state(),
                scene.sim().main_rng.logical_state()
            );
            assert_eq!(
                restored.mapgen_rng.logical_state(),
                scene.sim().mapgen_rng.logical_state()
            );
        }
        assert_eq!(
            first.state_hash(),
            second.state_hash(),
            "{} independent loads",
            saved.name
        );
        assert_eq!(first.rng_state(), second.rng_state());
        assert!(
            rebuilt_graphs(&first) == rebuilt_graphs(&second),
            "{} rebuilt graphs",
            saved.name
        );
        let live = std::mem::replace(&mut scene.runtime.simulation, first);
        for frame in 0..64 {
            for _ in 0..2 {
                scene
                    .runtime
                    .advance_frame(
                        &[],
                        SIM_TICK_MS,
                        TickLane::Ordinary,
                        crate::sim::world::FrameEffects::default(),
                    )
                    .unwrap();
                std::mem::swap(&mut scene.runtime.simulation, &mut second);
            }
            assert_eq!(
                scene.sim().state_hash(),
                second.state_hash(),
                "{} restored future{frame}",
                saved.name
            );
            assert_eq!(
                scene.sim().rng_state(),
                second.rng_state(),
                "{} restored RNG{frame}",
                saved.name
            );
            assert_eq!(
                authority(scene.sim(), source, false),
                authority(&second, source, false),
                "{} selected future{frame}",
                saved.name
            );
        }
        // A pre-collapse load can change later damage admission. Require the
        // saved missiles/effects to retire, without forcing a new collapse.
        for id in saved.transients {
            assert!(scene.sim().projectiles.get(id).is_none());
            assert!(scene.sim().substrate.anims.get(id).is_none());
            assert!(!scene.sim().substrate.logic.snapshot().contains(&id));
            assert!(!scene.sim().substrate.pending_delete.contains_object(&id));
        }
        if saved.collapsed {
            assert!(
                settled(scene.sim(), source, &ambient),
                "{} restored cleanup",
                saved.name
            );
        }
        assert!(
            rebuilt_graphs(scene.sim()) == rebuilt_graphs(&second),
            "{} graph futures",
            saved.name
        );
        // Retain the same process donor for the next checkpoint pair.
        scene.runtime.simulation = live;
    }
}
