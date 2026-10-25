//! Callback integration around the MindControl detonation ingress.
//!
//! These are Rust integration controls, not additional native executions.
//! The destruction row keeps the firer alive: it does not claim a graceful
//! native continuation after Bullet+B0 becomes NULL (4692CA dereferences it).

use super::*;
use crate::sim::components::DriveCoord;
use crate::sim::projectile::{
    ProjectileDetonation, ProjectileDetonationReason, ProjectileTrajectory, ProjectileVelocity,
    ProjectileVisualState,
};
use crate::sim::terrain_object::TerrainObjectLifecycle;
use crate::sim::world::SimSoundEvent;

/// Admit the requested target class through the same stored Bullet owner as
/// ordinary shots, including its expiry subscriptions. This supplies a
/// detonation boundary; it does not pretend to replay launch or flight.
fn projectile_to(
    sim: &mut Simulation,
    source: u64,
    target: ProjectileTarget,
) -> ProjectileDetonation {
    let projectile_id = sim.substrate.allocate_stable_id();
    let payload = ProjectilePayload::new(
        1,
        sim.interner.intern("TagController"),
        Some(sim.interner.intern("TagPsychicControl")),
        false,
    );
    let impact = ProjectileCoord::new(15 * 256 + 128, 15 * 256 + 128, 0);
    sim.admit_projectile(
        projectile_id,
        ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: false,
            source_id: source,
            origin: ProjectileCoord::new(17 * 256 + 128, 15 * 256 + 128, 0),
            target,
            initial_target_position: impact,
            payload,
            speed_leptons_per_frame: 100,
            velocity: ProjectileVelocity::new(0, 0, 0),
            trajectory: ProjectileTrajectory::Straight,
            guidance: None,
            visual: ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        },
    );
    ProjectileDetonation {
        projectile_id,
        source_id: source,
        target,
        impact,
        payload,
        reason: ProjectileDetonationReason::ReachedTarget,
    }
}

#[test]
fn mind_control_event6_destroy_target_refreshes_bullet_but_retains_tag_actor() {
    let (mut sim, rules, overlay, houses) =
        event_ingress_fixture_with_rules(MIND_CONTROL_TAG_RULES);
    let source = sim
        .spawn_object_at_height("TAGYURI", "House0", 17, 15, 0, 0, &rules)
        .unwrap();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[2], (15, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nATTACKED=Country0,SOURCE,Attacked,0,1,1,1,0\nSOURCE=Country0,<none>,Source,0,1,1,1,0\n\
         [Tags]\nTAG=2,Attacked,ATTACKED\n[Events]\nATTACKED=1,6,0,0\nSOURCE=1,44,0,0\n\
         [Actions]\nATTACKED=2,32,0,0,0,0,0,0,A,28,0,6,0,0,0,0,A\nSOURCE=1,28,0,44,0,0,0,0,A\n",
    );
    // Only the victim is attached. Action32 uses the ordinary fatal Unit
    // receiver and UnInit, not a test-injected PointerExpired callback.
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    let detonation = projectile_to(&mut sim, source, ProjectileTarget::Entity(victim));
    sim.mind_control_detonation(detonation, &rules, Some(&overlay), FrameEffects::default());

    let live = sim.projectiles.current_detonation(detonation);
    assert_eq!(live.source_id, source);
    assert_ne!(live.target, ProjectileTarget::Entity(victim));
    let victim = sim.substrate.entities.get(victim).unwrap();
    assert_eq!(victim.health.current(), 0);
    assert!(!victim.is_object_alive());
    assert_eq!(victim.mind_control.controller(), None);
    assert!(
        sim.substrate
            .entities
            .get(source)
            .unwrap()
            .capture_manager
            .as_ref()
            .unwrap()
            .victims()
            .next()
            .is_none(),
        "the stale victim ID must not reach CaptureUnit, which has no HP gate"
    );
    assert_eq!(
        sim.trigger_runtime.globals_set,
        BTreeSet::from([6, 44]),
        "event44 still uses the original actor's retained Tag, without an Alive reread"
    );
}

#[test]
fn mind_control_terrain_tags_use_object_alive_without_capturing_terrain() {
    for (lifecycle, health, expected) in [
        (TerrainObjectLifecycle::Live, 100, true),
        (TerrainObjectLifecycle::Live, 0, true),
        (TerrainObjectLifecycle::Limbo, 0, true),
        (TerrainObjectLifecycle::Destroyed, 100, false),
    ] {
        let extra = format!(
            "{MIND_CONTROL_TAG_RULES}\n[TerrainTypes]\n0=TAGTREE\n[TAGTREE]\nStrength=100\n"
        );
        let (mut sim, rules, overlay, _) = event_ingress_fixture_with_rules(&extra);
        assert_eq!(
            crate::sim::terrain_spawn::construct_terrain_objects(
                &mut sim,
                &[crate::map::overlay::TerrainObject {
                    rx: 15,
                    ry: 15,
                    name: "TAGTREE".into(),
                }],
                &rules,
                false,
            ),
            1
        );
        let terrain = sim.terrain_objects.id_at((15, 15)).unwrap();
        let source = sim
            .spawn_object_at_height("TAGYURI", "House0", 17, 15, 0, 0, &rules)
            .unwrap();
        install_event_ingress_map(&mut sim, &rules, MIND_CONTROL_TAG_MAP);
        sim.attach_object_trigger_tag(terrain, Some("TAG"));
        // Declared boundary priors on the constructed Terrain owner isolate
        // ObjectAlive from HP/limbo; this is not a Terrain death replay.
        let state = sim
            .terrain_objects
            .objects_mut_for_test()
            .get_mut(&terrain)
            .unwrap();
        state.set_health_for_test(health);
        state.set_lifecycle_for_test(lifecycle);
        let before = state.clone();
        let detonation = projectile_to(&mut sim, source, ProjectileTarget::Terrain(terrain));
        sim.mind_control_detonation(detonation, &rules, Some(&overlay), FrameEffects::default());
        let expected_globals = if expected {
            BTreeSet::from([6, 44])
        } else {
            BTreeSet::new()
        };
        assert_eq!(
            sim.trigger_runtime.globals_set, expected_globals,
            "{lifecycle:?}, HP={health}"
        );
        assert_eq!(sim.terrain_objects.get(terrain), Some(&before));
        assert!(
            sim.substrate
                .entities
                .get(source)
                .unwrap()
                .capture_manager
                .as_ref()
                .unwrap()
                .victims()
                .next()
                .is_none()
        );
        assert_eq!(
            sim.projectiles.current_detonation(detonation).target,
            ProjectileTarget::Terrain(terrain)
        );
    }
}

#[test]
fn mind_control_cell_and_null_targets_do_not_raise_nearby_object_tags() {
    for target in [
        ProjectileTarget::Cell { rx: 15, ry: 15 },
        ProjectileTarget::DummyCell,
        ProjectileTarget::None,
    ] {
        let (mut sim, rules, overlay, houses) =
            event_ingress_fixture_with_rules(MIND_CONTROL_TAG_RULES);
        let source = sim
            .spawn_object_at_height("TAGYURI", "House0", 17, 15, 0, 0, &rules)
            .unwrap();
        let nearby = spawn_event_ingress_unit(&mut sim, &rules, houses[2], (15, 15));
        install_event_ingress_map(&mut sim, &rules, MIND_CONTROL_TAG_MAP);
        sim.attach_object_trigger_tag(nearby, Some("TAG"));
        sim.attach_object_trigger_tag(source, Some("TAG"));
        let detonation = projectile_to(&mut sim, source, target);
        sim.mind_control_detonation(detonation, &rules, Some(&overlay), FrameEffects::default());
        assert!(sim.trigger_runtime.globals_set.is_empty(), "{target:?}");
        let nearby = sim.substrate.entities.get(nearby).unwrap();
        assert_eq!(nearby.owner(), houses[2]);
        assert_eq!(nearby.health.current(), 100);
        assert_eq!(nearby.mind_control.controller(), None);
        assert!(
            sim.substrate
                .entities
                .get(source)
                .unwrap()
                .capture_manager
                .as_ref()
                .unwrap()
                .victims()
                .next()
                .is_none()
        );
    }
}

#[test]
fn mind_control_sound_keeps_pre_capture_house_and_original_raw_location() {
    let extra = format!(
        "{MIND_CONTROL_TAG_RULES}\n[AudioVisual]\nYuriMindControlSound=CallbackCaptureSound\n"
    );
    let (mut sim, rules, overlay, houses) = event_ingress_fixture_with_rules(&extra);
    let source = sim
        .spawn_object_at_height("TAGYURI", "House0", 17, 15, 0, 0, &rules)
        .unwrap();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[2], (15, 15));
    let raw = DriveCoord {
        x: 15 * 256 + 91,
        y: 15 * 256 + 173,
        z: 47,
    };
    sim.foot_set_location_marked(victim, raw, &rules, Some(&overlay), FrameEffects::default());
    install_event_ingress_map(&mut sim, &rules, MIND_CONTROL_TAG_MAP);
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    let detonation = projectile_to(&mut sim, source, ProjectileTarget::Entity(victim));
    assert_ne!(
        (raw.x, raw.y, raw.z),
        (
            detonation.impact.x,
            detonation.impact.y,
            detonation.impact.z
        )
    );
    sim.sound_events.clear_for_test();
    sim.mind_control_detonation(detonation, &rules, Some(&overlay), FrameEffects::default());

    let victim = sim.substrate.entities.get(victim).unwrap();
    assert_eq!(victim.owner(), houses[0]);
    assert_eq!(victim.mind_control.controller(), Some(source));
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([6, 44]));
    let sounds: Vec<_> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::VocAt {
                sound_id,
                audible_to,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } if sound_id == "CallbackCaptureSound" => Some((
                *audible_to,
                crate::util::lepton::absolute_lepton_axis(*rx, *sub_x),
                crate::util::lepton::absolute_lepton_axis(*ry, *sub_y),
                *world_z_leptons,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        sounds,
        [(Some([houses[0], houses[2]]), raw.x, raw.y, raw.z)]
    );
    // This checks simulation event fields only. Campaign IsHumanPlayer and
    // presentation-local filtering are not certified by the owner-ID event.
}
