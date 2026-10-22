//! Native Airburst fan and production receiver integration. Native goldens
//! cover the fan/Construct/Fire/RNG; target damage below is a Rust regression
//! through the shared damage owner, not a claim of whole native combat parity.

use super::*;
use crate::sim::projectile::{ProjectileCoord, ProjectileDetonationReason};
#[cfg(not(feature = "ares"))]
use crate::sim::rng::SimRng;
#[cfg(not(feature = "ares"))]
use crate::sim::world::{FrameEffects, UninitContext};
#[cfg(not(feature = "ares"))]
use serde_json::Value;

fn rules() -> RuleSet {
    // Secondary registers the child BEFORE the stock Weapon sweep. The
    // Airburst factory in the later Bullet sweep never recursively reads it.
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
         [SOURCE]\nStrength=100\nPrimary=PARENT_GUN\nSecondary=CHILD_GUN\nSpeed=4\n\
         [VICTIM]\nStrength=1000\nArmor=none\nSpeed=4\n\
         [PARENT_GUN]\nProjectile=PARENT_SHOT\nWarhead=QUIET\nDamage=0\nRange=10\n\
         [CHILD_GUN]\nProjectile=CHILD_SHOT\nWarhead=CHILD_WH\nDamage=37\nSpeed=73\nBright=yes\nRange=4\n\
         [PARENT_SHOT]\nAirburst=yes\nAirburstWeapon=CHILD_GUN\nCluster=2\n\
         [CHILD_SHOT]\nFirersPalette=yes\n\
         [Warheads]\n0=QUIET\n1=CHILD_WH\n\
         [QUIET]\nVerses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\n\
         [CHILD_WH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    // The native fixture supplies a post-Process Weapon field73, independent
    // of the already-tested ROT0 ballistic Speed postpass.
    rules.set_weapon_speed_for_test("CHILD_GUN", 73);
    rules
}

fn scene(z: i32) -> (Simulation, RuleSet, u64, ProjectileDetonation) {
    let rules = rules();
    let mut world = crate::sim::arena_fixture::rival_arena(&rules, 31);
    world.intern_rule_type_ids(&rules);
    world.resolve_type_handles(&rules);
    world.session.set_binary_frame_for_test(100);
    let source = world
        .spawn_object("SOURCE", "Americans", 6, 20, 0, &rules)
        .unwrap();
    let wh = world.interner.intern("QUIET");
    let kind = world.interner.intern("PARENT_SHOT");
    let payload = ProjectilePayload::new(0, wh, Some(kind), true).with_damage_multiplier(768);
    let target = ProjectileTarget::Cell { rx: 10, ry: 20 };
    let parent = world.construct_projectile(source, target, payload, &rules);
    world
        .projectiles
        .get_mut(parent)
        .unwrap()
        .set_position_for_test(ProjectileCoord::new(2688, 5248, z));
    let request = ProjectileDetonation {
        projectile_id: parent,
        source_id: source,
        target,
        impact: ProjectileCoord::new(2699, 5259, z + 10),
        payload,
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    (world, rules, source, request)
}

fn detonate(world: &mut Simulation, rules: &RuleSet, request: ProjectileDetonation) {
    let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
    crate::sim::combat::world_receiver::commit_projectiles(
        world,
        &mut run,
        &[request],
        rules,
        None,
    );
    run.finish();
}

#[test]
#[cfg(not(feature = "ares"))]
fn stock_airburst_tail_matches_native_children_rng_and_cleanup() {
    let rows: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/airburst.json",
    ))
    .unwrap();
    let native = &rows[0];
    let (mut world, rules, source, request) = scene(600);
    world.scenario_rng =
        SimRng::from_native_state_hex_for_test(native["rng_before"]["scenario"].as_str().unwrap());
    let main = world.main_rng.native_state_hex();
    let mapgen = world.mapgen_rng.native_state_hex();
    detonate(&mut world, &rules, request);
    let mut children: Vec<_> = world
        .projectiles
        .iter()
        .filter(|(id, _)| **id != request.projectile_id)
        .collect();
    children.sort_by_key(|(id, _)| **id);
    assert_eq!(children.len(), native["children"].as_array().unwrap().len());
    for ((_, child), expected) in children.iter().zip(native["children"].as_array().unwrap()) {
        let target = &expected["target"];
        assert_eq!(
            child.target(),
            ProjectileTarget::Cell {
                rx: target[0].as_u64().unwrap() as u16,
                ry: target[1].as_u64().unwrap() as u16,
            }
        );
        let origin = child.launch_origin();
        assert_eq!([origin.x, origin.y, origin.z], [2688, 5248, 600]);
        let bits: Vec<_> = child
            .velocity()
            .native()
            .iter()
            .map(|value| format!("{:016x}", value.bits()))
            .collect();
        assert_eq!(
            serde_json::json!(bits),
            expected["after_fire"]["velocity"]["bits"]
        );
        assert_eq!(child.source_id(), source);
        assert_eq!(child.payload().base_damage, 37);
        assert_eq!(child.payload().damage_multiplier(), 256);
        assert!(!child.payload().bright());
        assert!(child.payload().installed_weapon().is_none());
        assert_eq!(
            child.firer_house(),
            Some(world.substrate.entities.get(source).unwrap().owner()),
        );
        assert_eq!(child.arm_timer().start_frame(), 100);
        assert_eq!(child.arm_timer().duration(), 0);
        assert!(child.in_logic_vector());
    }
    assert_eq!(
        world.scenario_rng.native_state_hex(),
        native["rng_after"]["scenario"]
    );
    assert_eq!(world.main_rng.native_state_hex(), main);
    assert_eq!(world.mapgen_rng.native_state_hex(), mapgen);
    let ids: Vec<_> = children.iter().map(|(id, _)| **id).collect();
    assert!(
        world.retire_non_entity_object(request.projectile_id, UninitContext::with_rules(&rules))
    );
    // Parent removal must leave the children alive. Each owns its source,
    // cell target, Logic slot and expiry registration through existing owners.
    assert!(ids.iter().all(|id| world.projectiles.get(*id).is_some()));
    world.uninit_with_rules(source, &rules);
    assert!(
        ids.iter()
            .all(|id| world.projectiles.get(*id).unwrap().source_id() == 0)
    );
    for id in ids {
        world.retire_non_entity_object(id, UninitContext::with_rules(&rules));
    }
    world.flush_pending_delete();
    assert!(world.projectiles.is_empty());
}

#[test]
#[cfg(not(feature = "ares"))]
fn stock_airburst_children_reach_shared_damage_and_expire() {
    let (mut world, rules, _source, request) = scene(0);
    let victim = world
        .spawn_object("VICTIM", "Russians", 10, 20, 0, &rules)
        .unwrap();
    let before = world
        .substrate
        .entities
        .get(victim)
        .unwrap()
        .health
        .current();
    detonate(&mut world, &rules, request);
    let first = world
        .projectiles
        .iter()
        .filter(|(id, _)| **id != request.projectile_id)
        .map(|(id, _)| *id)
        .min()
        .unwrap();
    world.retire_non_entity_object(request.projectile_id, UninitContext::with_rules(&rules));
    let (_, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
        for frame in 101..106 {
            world.session.set_binary_frame_for_test(frame);
            world.visit_combat_tail(first, &rules, None, FrameEffects::default());
            world.flush_pending_delete();
        }
    });
    assert!(
        visits
            .iter()
            .any(|visit| visit["warhead"] == "CHILD_WH" && visit["damage"] == 37),
        "actual area entries: {:?}; remaining Bullets: {}",
        visits
            .iter()
            .map(|visit| (
                &visit["frame"],
                &visit["coord"],
                &visit["warhead"],
                &visit["damage"]
            ))
            .collect::<Vec<_>>(),
        world.projectiles.iter().count(),
    );
    assert!(
        world
            .substrate
            .entities
            .get(victim)
            .unwrap()
            .health
            .current()
            < before
    );
    assert!(world.projectiles.is_empty());
}

#[test]
#[cfg(not(feature = "ares"))]
fn stock_airburst_reads_live_source_and_requires_a_represented_parent() {
    let (mut world, rules, source, request) = scene(600);
    // Keep the old packet's source intentionally stale. The authoritative
    // Bullet pointer expiry must determine every newly constructed child.
    world.notify_pointer_expired(source, UninitContext::with_rules(&rules));
    detonate(&mut world, &rules, request);
    let children: Vec<_> = world
        .projectiles
        .iter()
        .filter(|(id, _)| **id != request.projectile_id)
        .collect();
    assert_eq!(children.len(), 9);
    assert!(
        children
            .iter()
            .all(|(_, child)| child.source_id() == 0 && child.firer_house().is_none())
    );
    let before = world.projectiles.iter().count();
    let rng = world.scenario_rng.native_state_hex();
    emit(
        &mut world,
        &rules,
        &ProjectileDetonation {
            projectile_id: u64::MAX,
            ..request
        },
    );
    assert_eq!(world.projectiles.iter().count(), before);
    assert_eq!(world.scenario_rng.native_state_hex(), rng);
}

#[test]
#[cfg(feature = "ares")]
fn stock_airburst_does_not_run_in_ares_build() {
    let (mut world, rules, _, request) = scene(600);
    let rng = world.scenario_rng.native_state_hex();
    detonate(&mut world, &rules, request);
    assert_eq!(world.projectiles.iter().count(), 1);
    assert_eq!(world.scenario_rng.native_state_hex(), rng);
}
