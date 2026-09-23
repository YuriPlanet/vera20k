//! Integration tests for turret rotation + fire decision parity.
//!
//! Verifies the FacingClass-driven combat behavior end-to-end through
//! `Simulation::advance_tick`, covering 1-tick acquisition latency,
//! mid-rotation retarget, slow vs fast ROT alignment timing, and the
//! flipped Phase 5 tick order.

use std::collections::BTreeMap;

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::AttackTarget;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::movement::FacingClass;
use crate::sim::movement::turret::{body_facing_to_turret, desired_turret_facing};
use crate::sim::power_system::PowerState;
use crate::sim::world::Simulation;

fn empty_height_map() -> BTreeMap<(u16, u16), u8> {
    BTreeMap::new()
}

/// Minimal rules with MTNK at the given ROT byte. tick_turret_rotation
/// re-applies this each tick via barrel.set_rot, so it drives the
/// per-test rotation rate.
fn rules_with_mtnk_rot(rot: u32) -> RuleSet {
    let ini_str: String = format!(
        "[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n0=ENGI\n\n\
[BuildingTypes]\n0=GAPILE\n\n\
[AircraftTypes]\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\nROT={rot}\n\n\
[ENGI]\nStrength=75\nArmor=none\nSpeed=4\n\n\
[GAPILE]\nStrength=300\nArmor=heavy\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n"
    );
    let ini: IniFile = IniFile::from_str(&ini_str);
    RuleSet::from_ini(&ini).expect("rules_with_mtnk_rot should parse")
}

/// Spawn a turreted attacker at (rx, ry) facing north (0) with the given ROT byte.
fn spawn_turreted(sim: &mut Simulation, stable_id: u64, rx: u16, ry: u16, rot_byte: u8) {
    let mut entity = GameEntity::test_default(stable_id, "MTNK", "Americans", rx, ry);
    entity.barrel_facing = Some(FacingClass::new(body_facing_to_turret(0), rot_byte));
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.reveal(stable_id),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
}

/// Spawn a passive target at (rx, ry).
fn spawn_target(sim: &mut Simulation, stable_id: u64, rx: u16, ry: u16) {
    let entity = GameEntity::test_default(stable_id, "GAPILE", "Soviet", rx, ry);
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.reveal(stable_id),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
}

/// Replace sim's interner with the thread-local test interner so entity
/// type_ref / owner IDs from `GameEntity::test_default` (which uses
/// `test_intern`) resolve correctly inside sim functions.
fn use_test_interner(sim: &mut Simulation) {
    sim.interner = crate::sim::intern::test_interner();
}

#[test]
fn one_tick_acquisition_latency_first_tick_no_fire() {
    // After issuing an attack, the binary takes 1+ frames to rotate the turret
    // before firing (combat reads last-frame's facing). Even with ROT large
    // enough to fully rotate in 1 frame, the FIRST tick after target-set
    // produces no fire because combat ran BEFORE turret_rotation.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100); // ROT=100 → rot_per_frame=25600
    spawn_target(&mut sim, 2, 8, 5);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);

    // Attach attack_target so combat will try to fire on the next tick.
    if let Some(e) = sim.substrate.entities.get_mut(1) {
        e.attack_target = Some(AttackTarget::new(2));
    }

    let initial_target_health = sim.substrate.entities.get(2).unwrap().health.current;
    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);

    // Target should still be alive — combat ran before turret rotation, so
    // turret was at facing 0 (body), not aligned with target.
    let target_health_after_one_tick = sim.substrate.entities.get(2).unwrap().health.current;
    assert_eq!(
        target_health_after_one_tick, initial_target_health,
        "First tick after acquisition should not fire (1-tick latency)"
    );
}

#[test]
fn slow_rot_takes_more_frames_to_align_than_fast_rot() {
    // ROT=1 vs ROT=10: same acquisition geometry, the slow turret takes
    // proportionally more binary frames to align. Fixes the current
    // is_turret_aligned_u16 flat-tolerance bug.
    let mut sim_slow = Simulation::new();
    let mut sim_fast = Simulation::new();
    spawn_turreted(&mut sim_slow, 1, 5, 5, 1); // ROT=1 → rot_per_frame=256
    spawn_turreted(&mut sim_fast, 1, 5, 5, 10); // ROT=10 → rot_per_frame=2560
    spawn_target(&mut sim_slow, 2, 5, 8); // 3 cells south
    spawn_target(&mut sim_fast, 2, 5, 8);
    use_test_interner(&mut sim_slow);
    use_test_interner(&mut sim_fast);
    let rules_slow = rules_with_mtnk_rot(1);
    let rules_fast = rules_with_mtnk_rot(10);

    // Attach attack_target on both.
    sim_slow
        .substrate
        .entities
        .get_mut(1)
        .unwrap()
        .attack_target = Some(AttackTarget::new(2));
    sim_fast
        .substrate
        .entities
        .get_mut(1)
        .unwrap()
        .attack_target = Some(AttackTarget::new(2));

    // Compute the expected duration: from facing 0 (north, after body_facing_to_turret(0))
    // to facing south (~32768). Diff = 32768. ROT=1: duration = 32768/256 = 128 frames.
    // ROT=10: duration = 32768/2560 = 12 frames.
    // Run 13 binary frames worth of ticks. Fast turret should be done; slow not.

    // Each 67ms tick advances binary_frame by ~1.
    for _ in 0..13 {
        sim_slow.advance_tick(&[], Some(&rules_slow), &empty_height_map(), None, None, 67);
        sim_fast.advance_tick(&[], Some(&rules_fast), &empty_height_map(), None, None, 67);
    }

    let slow_rotating = sim_slow
        .substrate
        .entities
        .get(1)
        .unwrap()
        .barrel_facing
        .as_ref()
        .map(|f| f.is_rotating(sim_slow.session.binary_frame))
        .unwrap_or(false);
    let fast_rotating = sim_fast
        .substrate
        .entities
        .get(1)
        .unwrap()
        .barrel_facing
        .as_ref()
        .map(|f| f.is_rotating(sim_fast.session.binary_frame))
        .unwrap_or(false);

    assert!(
        slow_rotating,
        "ROT=1 turret should still be rotating after 13 frames"
    );
    assert!(
        !fast_rotating,
        "ROT=10 turret should be done rotating after 13 frames"
    );
}

#[test]
fn idle_turret_returns_to_body_facing() {
    // No attack_target, body facing east (64) — turret should rotate to match.
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    entity.facing = 64; // body east
    entity.barrel_facing = Some(FacingClass::new(body_facing_to_turret(0), 100));
    // ROT=100 → rot_per_frame=25600. Diff from 0 (north turret) to body_facing_to_turret(64) =
    // 64*256 = 16384. Duration = 16384/25600 = 0 → snaps in 1 frame.
    sim.substrate.entities.insert(entity);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);

    // Run 2 ticks to ensure turret_rotation has had a chance to act.
    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);

    let barrel = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .barrel_facing
        .as_ref()
        .unwrap();
    assert_eq!(
        barrel.destination(),
        body_facing_to_turret(64),
        "Idle turret should target body facing"
    );
}

#[test]
fn mid_rotation_retarget_snapshots_into_prev() {
    // Start a rotation, advance partway, set a new target. The new prev
    // should equal the animated value at the moment of the new set (not the
    // original prev) — visible smoothness of mid-rotation retarget.
    let mut fc = FacingClass::new(0, 5);
    fc.set(12800, 0); // rotation 0 → 12800 over 10 frames.
    let animated_at_5 = fc.current(5);
    fc.set(25600, 5); // retarget mid-rotation.

    // After re-set, prev should equal the animated value at frame 5, NOT 0.
    assert_eq!(
        fc.current(5),
        animated_at_5,
        "Animated value immediately after re-set should equal pre-set animated value (no jump)"
    );
}

// --- L2 unit_post shadow acceptance tests ---

#[test]
fn unit_cooldown_decrement_order_independent() {
    // The future per-object flip moves the cooldown/burst-delay decrement from the
    // legacy id-order pre-pass to a per-object live-order step. `saturating_sub(1)`
    // is per-entity with no cross-entity dependency, so any visitation order yields
    // identical results — pin it empirically across two opposite orders.
    let start = [(1u64, 7u16, 3u8), (2u64, 4u16, 0u8)];
    let dec = |v: &mut [(u64, u16, u8)], order: &[usize]| {
        for &i in order {
            v[i].1 = v[i].1.saturating_sub(1);
            v[i].2 = v[i].2.saturating_sub(1);
        }
    };
    let mut ascending = start.to_vec();
    let mut descending = start.to_vec();
    dec(&mut ascending, &[0, 1]);
    dec(&mut descending, &[1, 0]);
    assert_eq!(
        ascending, descending,
        "per-entity cooldown decrement must be order-independent"
    );
}

#[test]
fn unit_facing_pass_drives_turret_to_target() {
    // The authoritative live-order Unit facing pass must drive the barrel to the
    // per-entity desired facing (toward the target) — proving the pass is faithful
    // and the id-order→live-order reorder is output-neutral (per-entity facing).
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 5);
    spawn_target(&mut sim, 2, 5, 9); // due south
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(5);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    let want = {
        let e = sim.substrate.entities.get(1).unwrap();
        desired_turret_facing(
            e,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            sim.session.binary_frame,
        )
        .expect("turreted unit has a desired facing")
    };
    let result = run_combat_direct(&mut sim, &rules);
    crate::sim::world::unit_post::apply_unit_facing(
        &mut sim.substrate.entities,
        &result.unit_facing,
        &rules,
        &sim.interner,
        sim.session.binary_frame,
    );
    let got = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .barrel_facing
        .as_ref()
        .unwrap()
        .destination();
    assert_eq!(
        got, want,
        "the P2-window compute + apply_unit_facing must drive the Unit barrel to the desired facing"
    );
}

#[test]
fn unit_authoritative_fire_kills_target_via_advance_tick() {
    // Drive a turreted Unit through acquisition, alignment, repeated fire, and the
    // target's death via advance_tick — exercising the authoritative path end-to-end:
    // shared-body fire + the per-object unit_post facing pass (incl. the retarget after
    // the kill) + the deferred-death batch. A facing/fire break would show as the
    // target never dying or a panic; the per-tick state_hash gate is the stronger net
    // for hash-affecting drift.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100); // fast ROT — aligns within a frame
    spawn_target(&mut sim, 2, 5, 8); // 3 cells south, in range (Range=6)
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mark_live_contact_with(2);
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .mark_live_contact_with(1);
    let start_hp = sim.substrate.entities.get(2).unwrap().health.current;
    let mut fired = false;
    let mut target_gone = false;
    for _ in 0..400 {
        sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
        match sim.substrate.entities.get(2) {
            Some(t) => {
                if t.health.current < start_hp {
                    fired = true;
                }
            }
            None => {
                target_gone = true;
                break;
            }
        }
    }
    assert!(fired, "the Unit should have fired and damaged the target");
    assert!(
        target_gone,
        "repeated fire should have killed and despawned the target"
    );
    let survivor = sim.substrate.entities.get(1).unwrap();
    assert!(survivor.attack_target.is_none());
    assert!(!survivor.has_live_contact_with(2));
}

// --- S3 per-object facing-destination tests (read in the P2 window) ---

/// Call `tick_combat_with_fog` directly (mirrors the combat_tests direct-call
/// pattern) so `result.unit_facing` — a transient emit consumed by the world
/// apply site — is observable.
fn run_combat_direct(
    sim: &mut Simulation,
    rules: &RuleSet,
) -> crate::sim::combat::CombatTickResult {
    let live_order = sim.live_object_order_snapshot();
    crate::sim::combat::tick_combat_with_fog(
        &mut sim.substrate.entities,
        &mut sim.substrate.occupancy,
        rules,
        &mut sim.interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        None,
        None,
        None,
        sim.session.tick,
        67,
        sim.session.binary_frame,
        &live_order,
        None, // radiation state — not under test here
        &mut sim.scenario_rng,
    )
}

fn unit_facing_of(result: &crate::sim::combat::CombatTickResult, id: u64) -> Option<u16> {
    result
        .unit_facing
        .iter()
        .find(|u| u.entity_id == id)
        .and_then(|u| u.turret_destination)
}

#[test]
fn s3_unit_facing_emitted_for_attacker_and_idle() {
    // The P2 window computes a destination for every Unit: attackers right
    // after their own fire resolution, target-less Units in the residual pass.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 5);
    spawn_target(&mut sim, 2, 5, 8); // hostile, 3 cells south, in range
    spawn_turreted(&mut sim, 3, 10, 10, 5); // idle — residual pass
    sim.substrate.entities.get_mut(3).unwrap().facing = 64; // body east
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(5);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    let want_attacker = {
        let e = sim.substrate.entities.get(1).unwrap();
        desired_turret_facing(
            e,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            sim.session.binary_frame,
        )
        .expect("turreted")
    };
    let result = run_combat_direct(&mut sim, &rules);

    assert_eq!(
        unit_facing_of(&result, 1),
        Some(want_attacker),
        "attacker destination = toward its (live) target"
    );
    assert_eq!(
        unit_facing_of(&result, 3),
        Some(body_facing_to_turret(64)),
        "idle Unit destination = body facing (residual pass)"
    );
}

#[test]
fn removed_attacker_returns_to_body_same_tick() {
    // A unit whose own resolution cleared its attack (dead target, nothing to
    // acquire) returns to body facing the same tick — matching both today's
    // output and gamemd's upstream same-pass target-validation clear.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 5);
    spawn_target(&mut sim, 2, 5, 8);
    sim.substrate.entities.get_mut(2).unwrap().health.current = 0; // dead at resolve
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(5);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    let result = run_combat_direct(&mut sim, &rules);

    assert_eq!(
        unit_facing_of(&result, 1),
        Some(body_facing_to_turret(0)),
        "own-remove → body facing same tick"
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .attack_target
            .is_none(),
        "the remove was applied by the batch"
    );
}

#[test]
fn retargeted_attacker_aims_new_target_same_tick() {
    // A unit whose own resolution retargeted aims at the NEW target the same
    // tick — the gamemd analog is upstream same-pass acquisition, which the
    // unit's Facing_Update sees in the same AI pass.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 5);
    spawn_target(&mut sim, 2, 5, 8);
    sim.substrate.entities.get_mut(2).unwrap().health.current = 0; // dead → re-acquire
    // Hostile alternative in range (2 cells east).
    let mut alt = GameEntity::test_default(4, "MTNK", "Soviet", 7, 5);
    alt.barrel_facing = Some(FacingClass::new(body_facing_to_turret(0), 5));
    alt.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(alt);
    sim.add_entity_occupancy(4);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(5);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    let want = {
        let e = sim.substrate.entities.get(1).unwrap();
        let t = sim.substrate.entities.get(4).unwrap();
        crate::sim::movement::turret::facing_toward_lepton(
            e.position.rx,
            e.position.ry,
            e.position.sub_x,
            e.position.sub_y,
            t.position.rx,
            t.position.ry,
            t.position.sub_x,
            t.position.sub_y,
        )
    };
    let result = run_combat_direct(&mut sim, &rules);

    assert_eq!(
        unit_facing_of(&result, 1),
        Some(want),
        "own-retarget → aim the new target same tick"
    );
}

#[test]
fn kill_tick_unit_facing_holds_target() {
    // THE S3 fidelity pin: a unit whose target dies from this tick's fire
    // keeps aiming at it this tick (gamemd: the munition is deferred and the
    // bullet's AI runs after the firing unit's pass, so Facing_Update reads a
    // live TarCom on the kill tick). This receiver fixture disables the death
    // callbacks, so it pins only the barrel; in production the killing hit's
    // Destroy (0x005F57AF) expires the target, see
    // `co_attacker_facing_matches_killer`.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100);
    spawn_target(&mut sim, 2, 5, 8);
    sim.substrate.entities.get_mut(2).unwrap().health.current = 10; // lethal: Damage=65
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    // Pre-align the barrel so the fire gate (destination match + not rotating)
    // passes on the first resolution — same facing_toward_lepton formula.
    let toward_target = {
        let e = sim.substrate.entities.get(1).unwrap();
        desired_turret_facing(
            e,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            sim.session.binary_frame,
        )
        .expect("turreted")
    };
    sim.substrate.entities.get_mut(1).unwrap().barrel_facing =
        Some(FacingClass::new(toward_target, 100));

    let result = run_combat_direct(&mut sim, &rules);

    let target_dead = sim
        .substrate
        .entities
        .get(2)
        .map(|t| t.health.current == 0 || t.dying)
        .unwrap_or(true);
    assert!(
        target_dead,
        "precondition: the shot this tick killed the target"
    );
    assert_eq!(
        unit_facing_of(&result, 1),
        Some(toward_target),
        "kill tick: barrel destination holds the dying target's facing"
    );
}

#[test]
fn kill_tick_barrel_holds_target_facing() {
    // End-to-end through advance_tick: on the tick the target dies, the
    // killer's barrel destination still points at it (gamemd: Facing_Update
    // runs inside the unit's own AI pass before any same-tick detonation
    // effect is visible); idle-return to body begins the NEXT tick.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100);
    spawn_target(&mut sim, 2, 5, 8);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    let toward_target = {
        let e = sim.substrate.entities.get(1).unwrap();
        desired_turret_facing(
            e,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            sim.session.binary_frame,
        )
        .expect("turreted")
    };
    // Pre-align so the fire gate passes on the first combat tick, and make the
    // first shot lethal.
    sim.substrate.entities.get_mut(1).unwrap().barrel_facing =
        Some(FacingClass::new(toward_target, 100));
    sim.substrate.entities.get_mut(2).unwrap().health.current = 10; // Damage=65

    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);

    let attacker = sim.substrate.entities.get(1).unwrap();
    assert!(
        sim.substrate
            .entities
            .get(2)
            .map(|t| t.health.current == 0 || t.dying)
            .unwrap_or(true),
        "precondition: the target died on this tick"
    );
    assert_eq!(
        attacker.barrel_facing.as_ref().unwrap().destination(),
        toward_target,
        "kill tick: barrel destination holds the dying target's facing"
    );

    // Next tick the target is gone, but the idle return does NOT begin. Native
    // gates it on `frame - LastFireFrame(+0x120) >= GuardAreaTargetingDelay + 5`
    // (`0x00736B35`..`0x00736B7C`), and `+0x120` was just stamped with this
    // frame by `TechnoClass::Fire_At @ 0x006FF743` — so the dwell is measured
    // from the unit's OWN LAST SHOT, not from target loss, and the barrel holds.
    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        attacker.barrel_facing.as_ref().unwrap().destination(),
        toward_target,
        "one tick after the kill: the barrel still holds the dead target's aim"
    );

    // 36 (`[General] GuardAreaTargetingDelay=`, the RuleSet default) + 5 frames
    // after that shot, the turret returns to the hull heading.
    for _ in 0..45 {
        sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
    }
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        attacker.barrel_facing.as_ref().unwrap().destination(),
        body_facing_to_turret(attacker.facing),
        "past the dwell: idle-return to the hull heading"
    );
}

#[test]
fn facing_apply_point_equivalence_no_kill() {
    // The ledger's apply-point-equivalence pin: on ticks with no deaths and no
    // retargets, the per-object P2-window read must produce exactly the
    // destination the legacy post-batch read would have produced — i.e. the
    // live post-tick barrel destination always equals desired_turret_facing
    // recomputed at the legacy read point (after the batch). Guards G4: any
    // future system that writes barrel state or attack targets between the P2
    // read and the post-batch apply site breaks this equality and fails here.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 1); // ROT=1 — rotation spans many ticks
    spawn_target(&mut sim, 2, 5, 9); // far enough that no kill happens quickly
    spawn_turreted(&mut sim, 3, 12, 12, 1); // idle co-unit (residual path)
    sim.substrate.entities.get_mut(3).unwrap().facing = 96;
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(1);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    let mut previous_destination: BTreeMap<u64, u16> = BTreeMap::new();
    for tick in 0..12 {
        sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
        // No deaths/retargets in this scenario — assert preconditions hold.
        assert!(
            sim.substrate.entities.get(2).is_some_and(|t| !t.dying),
            "tick {tick}: scenario must stay kill-free"
        );
        for id in [1u64, 3] {
            let e = sim.substrate.entities.get(id).unwrap();
            let legacy_read = desired_turret_facing(
                e,
                &sim.substrate.entities,
                Some(&rules),
                &sim.interner,
                sim.session.binary_frame,
            );
            let live = e.barrel_facing.as_ref().unwrap().destination();
            match legacy_read {
                Some(want) => assert_eq!(
                    live, want,
                    "tick {tick} unit {id}: P2-window destination must equal the \
                     legacy post-batch read on no-kill ticks"
                ),
                // `None` is the rotation latch (`+0x6AF`) or the idle dwell
                // suppressing the `Set` entirely — the destination must then be
                // byte-identical to the previous tick's.
                None => {
                    if let Some(&prev) = previous_destination.get(&id) {
                        assert_eq!(
                            live, prev,
                            "tick {tick} unit {id}: a frame where native calls no \
                             `Set` must leave the destination untouched"
                        );
                    }
                }
            }
            previous_destination.insert(id, live);
        }
    }
}

#[test]
fn co_attacker_facing_matches_killer() {
    // Two attackers on one target; the killer's shot lands this tick. The
    // co-attacker's barrel destination this tick must ALSO hold the dying
    // target's facing because its facing read happens before lethal damage.
    // The killing hit's Destroy (0x005F57AF) then clears its target reference
    // without rewriting that already-computed barrel destination.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100); // killer
    spawn_turreted(&mut sim, 3, 8, 8, 100); // co-attacker (out of its own ROF this tick)
    spawn_target(&mut sim, 2, 5, 8);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
    sim.substrate.entities.get_mut(3).unwrap().attack_target = Some(AttackTarget::new(2));

    let killer_aim = {
        let e = sim.substrate.entities.get(1).unwrap();
        desired_turret_facing(
            e,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            sim.session.binary_frame,
        )
        .expect("turreted")
    };
    let co_aim = {
        let e = sim.substrate.entities.get(3).unwrap();
        desired_turret_facing(
            e,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            sim.session.binary_frame,
        )
        .expect("turreted")
    };
    sim.substrate.entities.get_mut(1).unwrap().barrel_facing =
        Some(FacingClass::new(killer_aim, 100));
    sim.substrate.entities.get_mut(2).unwrap().health.current = 10;

    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);

    assert!(
        sim.substrate
            .entities
            .get(2)
            .map(|t| t.health.current == 0 || t.dying)
            .unwrap_or(true),
        "precondition: the target died on this tick"
    );
    let co = sim.substrate.entities.get(3).unwrap();
    assert!(
        co.attack_target.is_none(),
        "the killing hit's Destroy clears the co-attacker's expired target"
    );
    assert_eq!(
        co.barrel_facing.as_ref().unwrap().destination(),
        co_aim,
        "kill tick: co-attacker barrel destination holds the dying target's facing"
    );
}

#[test]
fn save_load_round_trip_on_kill_tick() {
    // A save taken on the kill tick — where the barrel destination still holds
    // a now-dead target's facing and the hashed mission value reflects the
    // dispatch-time machines — must restore an identical state hash (barrel
    // FacingClass and MissionCom both round-trip via serde; load trusts the
    // serialized values, no post-load re-derive).
    use crate::sim::snapshot::GameSnapshot;
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100);
    spawn_target(&mut sim, 2, 5, 8);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
    let toward_target = {
        let e = sim.substrate.entities.get(1).unwrap();
        desired_turret_facing(
            e,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            sim.session.binary_frame,
        )
        .expect("turreted")
    };
    sim.substrate.entities.get_mut(1).unwrap().barrel_facing =
        Some(FacingClass::new(toward_target, 100));
    sim.substrate.entities.get_mut(2).unwrap().health.current = 10;

    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67); // kill tick

    // Native in-scenario load restarts Scenario RNG from Seed0; isolate the
    // kill-tick/turret persistence hash on that same post-load cursor.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash_before = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("load").sim;
    restored.rebuild_logic_membership(); // the real post-deserialize step
    assert_eq!(
        restored.state_hash(),
        hash_before,
        "kill-tick save/load must restore an identical state hash"
    );
}

#[test]
fn non_unit_barrel_still_driven_by_global_sweep() {
    // Non-Unit categories keep the legacy post-batch sweep until their slices
    // land (S7/S8): a turreted Structure's barrel is still driven toward its
    // target by tick_turret_rotation after the flip.
    let mut sim = Simulation::new();
    // Armed type (the fixture GAPILE has no Primary, which would remove the
    // attack pre-sweep); the CATEGORY is what routes facing ownership.
    let mut tower = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    tower.category = crate::map::entities::EntityCategory::Structure;
    tower.lifecycle.in_limbo = false;
    tower.barrel_facing = Some(FacingClass::new(body_facing_to_turret(0), 100));
    tower.attack_target = Some(AttackTarget::new(2));
    sim.substrate.entities.insert(tower);
    spawn_target(&mut sim, 2, 5, 9);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);

    let want = {
        let e = sim.substrate.entities.get(1).unwrap();
        desired_turret_facing(
            e,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            sim.session.binary_frame,
        )
        .expect("turreted structure")
    };
    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .barrel_facing
            .as_ref()
            .unwrap()
            .destination(),
        want,
        "Structure barrel still driven by the legacy tick_turret_rotation sweep"
    );
}

#[test]
fn unit_facing_pass_idles_turret_to_body_without_target() {
    // A turreted Unit with no attack_target: the residual P2-window pass +
    // apply returns the barrel to body facing. Covers idle Units, which the
    // fire path never sees (no snapshot) — the regression the residual pass
    // exists to prevent.
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    entity.facing = 64; // body east
    entity.barrel_facing = Some(FacingClass::new(body_facing_to_turret(0), 100));
    sim.substrate.entities.insert(entity);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);

    let result = run_combat_direct(&mut sim, &rules);
    crate::sim::world::unit_post::apply_unit_facing(
        &mut sim.substrate.entities,
        &result.unit_facing,
        &rules,
        &sim.interner,
        sim.session.binary_frame,
    );
    let dest = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .barrel_facing
        .as_ref()
        .unwrap()
        .destination();
    assert_eq!(
        dest,
        body_facing_to_turret(64),
        "idle Unit barrel should return to body facing via the residual pass + apply"
    );
}

// --- M4: the turretless body gate, the latch, the dwell and the widened
//     tolerance (rows 126 GSI-08.14 / 124 GSI-08.04) ---

/// Spawn a `Turret=no` attacker at (rx, ry) facing north. `rules_with_mtnk_rot`
/// authors no `Turret=` for `[MTNK]`, so leaving `barrel_facing` unset is what
/// makes this the turretless case the body gate covers.
fn spawn_turretless(sim: &mut Simulation, stable_id: u64, rx: u16, ry: u16) {
    let entity = GameEntity::test_default(stable_id, "MTNK", "Americans", rx, ry);
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.reveal(stable_id),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
}

#[test]
fn gsi_08_04_turretless_vehicle_turns_its_hull_before_it_fires() {
    // The headline scenario: an artillery-shaped vehicle with no turret must
    // rotate its BODY to face the target before it may shoot, so its first shot
    // is delayed instead of instant.
    //
    // gamemd: `UnitClass::GetFireError @ 0x00740FD0` step 17 compares the HULL
    // `+0x388` for a `Turret=no` firer (`0x0074129F`..`0x007412AA`) and returns
    // 2 outside `0x0800`; `UnitClass::Fire_At_Target @ 0x00736DF0` case 2 then
    // turns the hull at `ROT=` with `FacingClass::Set(+0x388)` at `0x00737004`,
    // but only while the unit is stationary with no destination.
    let mut sim = Simulation::new();
    spawn_turretless(&mut sim, 1, 5, 5); // facing north (0)
    spawn_target(&mut sim, 2, 5, 9); // due south, inside Range=6
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(5);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    // A target assignment writes the pointer and nothing else - no snap.
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().facing,
        0,
        "assigning a target must not rotate the hull by itself"
    );

    let start_hp = sim.substrate.entities.get(2).unwrap().health.current;
    for tick in 0..8 {
        sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().health.current,
            start_hp,
            "tick {tick}: an off-axis turretless firer must be refused"
        );
    }
    assert_ne!(
        sim.substrate.entities.get(1).unwrap().facing,
        0,
        "the hull must be turning toward the target meanwhile"
    );

    // And it must eventually line up and shoot - the gate must not deadlock a
    // unit that has no turret to aim with.
    let mut fired = false;
    for _ in 0..80 {
        sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
        if sim
            .substrate
            .entities
            .get(2)
            .is_none_or(|t| t.health.current < start_hp)
        {
            fired = true;
            break;
        }
    }
    assert!(
        fired,
        "once the hull is inside 0x0800 the shot must be allowed"
    );
}

#[test]
fn gsi_08_04_moving_turretless_vehicle_does_not_turn_to_fire() {
    // `Fire_At_Target` case 2 turns the hull only when `NavCom == 0` and the
    // locomotor reports not-moving (`0x00736FB6`..`0x00736FE6`). A unit under a
    // move order keeps its travel heading; nothing re-aims it for the shot.
    let mut sim = Simulation::new();
    spawn_turretless(&mut sim, 1, 5, 5);
    spawn_target(&mut sim, 2, 5, 9);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(5);
    {
        let attacker = sim.substrate.entities.get_mut(1).unwrap();
        attacker.attack_target = Some(AttackTarget::new(2));
        attacker.passively_acquired_target = true; // pursuit leaves this one alone
        attacker.movement_target = Some(crate::sim::components::MovementTarget::default());
    }

    let facing_before = sim.substrate.entities.get(1).unwrap().facing;
    let result = run_combat_direct(&mut sim, &rules);
    let slot = result
        .unit_facing
        .iter()
        .find(|u| u.entity_id == 1)
        .expect("every Unit gets a Facing slot entry");
    assert_eq!(
        slot.hull_destination, None,
        "a moving turretless firer emits no hull turn"
    );
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().facing,
        facing_before,
        "and its heading is untouched by the fire decision"
    );
}

#[test]
fn gsi_08_14_idle_turret_dwell_is_measured_from_the_units_own_last_shot() {
    // `0x00736B2F`..`0x00736B7C`: the idle return needs
    // `frame - LastFireFrame(+0x120) >= GuardAreaTargetingDelay + 5`. A unit
    // that has never fired carries the constructor's `-100` (`0x006F2B9C`), so
    // it is already past the dwell and returns on the first idle frame.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100);
    sim.substrate.entities.get_mut(1).unwrap().facing = 64;
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);

    let never_fired = run_combat_direct(&mut sim, &rules);
    assert_eq!(
        never_fired
            .unit_facing
            .iter()
            .find(|u| u.entity_id == 1)
            .and_then(|u| u.turret_destination),
        Some(body_facing_to_turret(64)),
        "a unit that never fired is already past the dwell"
    );

    // Stamp a shot on this frame and the same idle unit holds instead.
    sim.substrate.entities.get_mut(1).unwrap().last_fire_frame =
        i64::from(sim.session.binary_frame);
    let just_fired = run_combat_direct(&mut sim, &rules);
    assert_eq!(
        just_fired
            .unit_facing
            .iter()
            .find(|u| u.entity_id == 1)
            .and_then(|u| u.turret_destination),
        None,
        "inside the dwell native calls no Set at all - the aim is held"
    );
}

#[test]
fn gsi_08_14_idle_turret_leads_toward_the_move_destination() {
    // `0x00736BA3`..`0x00736BC8`: when the idle return fires and a NavCom is
    // set, the destination is `DirectionToTarget(this, NavCom)` - the turret
    // leads toward where the unit is going, not back to the hull.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100);
    sim.substrate.entities.get_mut(1).unwrap().facing = 0; // hull north
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);
    {
        let movement = crate::sim::components::MovementTarget {
            path: vec![(5, 5), (9, 5)], // destination four cells EAST
            ..Default::default()
        };
        sim.substrate.entities.get_mut(1).unwrap().movement_target = Some(movement);
    }

    let result = run_combat_direct(&mut sim, &rules);
    let desired = result
        .unit_facing
        .iter()
        .find(|u| u.entity_id == 1)
        .and_then(|u| u.turret_destination)
        .expect("an idle turret past its dwell takes a destination");
    let want = crate::sim::movement::turret::facing_toward_lepton(
        5,
        5,
        crate::util::fixed_math::SimFixed::from_num(128),
        crate::util::fixed_math::SimFixed::from_num(128),
        9,
        5,
        crate::util::fixed_math::SimFixed::from_num(128),
        crate::util::fixed_math::SimFixed::from_num(128),
    );
    assert_eq!(
        desired, want,
        "the idle turret leads toward the move destination, not the hull heading"
    );
    assert_ne!(
        desired,
        body_facing_to_turret(0),
        "and specifically not back to the hull heading"
    );
}

#[test]
fn gsi_08_14_rotation_latch_suppresses_the_aim_while_the_arc_runs() {
    // `0x00736AEA`..`0x00736B16`: while `FacingClass::Is_Rotating` holds, the
    // latch `+0x6AF` is armed and the WHOLE aim block is skipped on the next
    // frame, so native commits to each arc instead of re-snapshotting `prev`
    // against a moving target every frame.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 1); // ROT=1 gives a long arc
    spawn_target(&mut sim, 2, 5, 9);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(1);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    // The first tick's read window sees an untouched barrel; the destination
    // and the latch both land at the post-batch apply, the latch armed by that
    // same `Set` (`0x00736B16` follows `0x00736A89`). A second tick leaves the
    // arc running, which is the state under test.
    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert!(
        attacker.turret_rotation_latch,
        "an arc in progress must arm +0x6AF"
    );
    let committed = attacker.barrel_facing.as_ref().unwrap().destination();

    // While it is armed the aim block produces no new destination, even though
    // the target has not moved and the turret is still short of it.
    let held = run_combat_direct(&mut sim, &rules);
    assert_eq!(
        held.unit_facing
            .iter()
            .find(|u| u.entity_id == 1)
            .and_then(|u| u.turret_destination),
        None,
        "the latch suppresses the aim arm"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .barrel_facing
            .as_ref()
            .unwrap()
            .destination(),
        committed,
        "so the committed arc destination survives the frame"
    );
}

/// The exact 16-bit facing from cell (5,5) to cell (5,9) that the fire gate
/// computes. `atan2`+`ftol` does not land on a round 0x8000, so offsets in the
/// tolerance tests are taken from this value rather than from a constant.
fn facing_from_5_5_to_5_9() -> u16 {
    crate::sim::movement::turret::facing_toward_lepton(
        5,
        5,
        crate::util::fixed_math::SimFixed::from_num(128),
        crate::util::fixed_math::SimFixed::from_num(128),
        5,
        9,
        crate::util::fixed_math::SimFixed::from_num(128),
        crate::util::fixed_math::SimFixed::from_num(128),
    )
}

/// Rules with an `MTNK` whose 105mm fires a HOMING projectile (`ROT=` non-zero
/// on the `[Projectile]` section) - the input that widens the fire tolerance.
fn rules_with_homing_projectile(projectile_rot: u32) -> RuleSet {
    let ini_str: String = format!(
        "[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n\n\
[BuildingTypes]\n0=GAPILE\n\n\
[AircraftTypes]\n\n\
[Projectiles]\n0=Homer\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\nROT=5\nTurret=yes\n\n\
[GAPILE]\nStrength=300\nArmor=heavy\n\n\
[Homer]\nROT={projectile_rot}\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\nProjectile=Homer\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n"
    );
    let ini: IniFile = IniFile::from_str(&ini_str);
    RuleSet::from_ini(&ini).expect("homing fixture should parse")
}

/// MTNK with an `OmniFire=` primary and no projectile — a weapon the step-17
/// angle arm never tests (`WeaponTypeClass+0x12B`, read at `0x0074125C`) but
/// step 14 still refuses.
fn rules_with_omni_fire() -> RuleSet {
    let ini_str: String = "[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n\n\
[BuildingTypes]\n0=GAPILE\n\n\
[AircraftTypes]\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\nROT=5\nTurret=yes\n\n\
[GAPILE]\nStrength=300\nArmor=heavy\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\nOmniFire=yes\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n"
        .to_string();
    let ini: IniFile = IniFile::from_str(&ini_str);
    RuleSet::from_ini(&ini).expect("omni-fire fixture should parse")
}

#[test]
fn gsi_08_03_homing_projectile_widens_the_fire_tolerance_to_0x1000() {
    // `0x007412BC`..`0x007412CC`: the tolerance byte is built from the
    // PROJECTILE's `ROT=` (`WeaponType+0xA0` -> `BulletTypeClass+0x2DC`) with
    // `NEG`/`SBB`/`AND 8`/`ADD 8` - 8 when that ROT is zero, 0x10 when it is
    // not - then `MOV CH,BL` shifts it into the high byte. A missile turret
    // therefore shoots up to 1/16 of a turn off-axis.
    fn fires_at_offset(rules: &RuleSet, offset: u16) -> bool {
        let mut sim = Simulation::new();
        spawn_turreted(&mut sim, 1, 5, 5, 5);
        spawn_target(&mut sim, 2, 5, 9); // due south -> 0x8000
        use_test_interner(&mut sim);
        sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
        sim.substrate.entities.get_mut(1).unwrap().barrel_facing = Some(FacingClass::new(
            facing_from_5_5_to_5_9().wrapping_add(offset),
            5,
        ));
        !run_combat_direct(&mut sim, rules)
            .consequences
            .fire_events()
            .is_empty()
    }

    let straight = rules_with_homing_projectile(0);
    let homing = rules_with_homing_projectile(30);
    assert!(
        fires_at_offset(&straight, 0x0800),
        "0x0800 itself passes - the native JGE skips the error return"
    );
    assert!(
        !fires_at_offset(&straight, 0x0801),
        "a straight-flying projectile is refused one unit past 0x0800"
    );
    assert!(
        fires_at_offset(&homing, 0x0801),
        "a homing projectile widens the slack"
    );
    assert!(
        fires_at_offset(&homing, 0x1000),
        "all the way to 0x1000 inclusive"
    );
    assert!(!fires_at_offset(&homing, 0x1001), "and no further");
}

#[test]
fn gsi_08_14_building_turret_holds_its_last_aim() {
    // A LEA census over `BuildingClass::Update`, `Mission_Guard` and every idle
    // path finds no `Set`/`UpdateFacing` of `+0x388` outside the attack,
    // construction and sell paths: a building turret never swings back to a
    // "body" heading, because a building has no hull.
    let mut sim = Simulation::new();
    let mut tower = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    tower.category = crate::map::entities::EntityCategory::Structure;
    tower.lifecycle.in_limbo = false;
    tower.barrel_facing = Some(FacingClass::new(body_facing_to_turret(0), 100));
    tower.attack_target = Some(AttackTarget::new(2));
    sim.substrate.entities.insert(tower);
    spawn_target(&mut sim, 2, 5, 9); // due south
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(100);

    sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
    let aimed = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .barrel_facing
        .as_ref()
        .unwrap()
        .destination();
    assert_ne!(aimed, body_facing_to_turret(0), "it aimed at the target");

    // Drop the target and let a long time pass - the aim must not move.
    sim.substrate.entities.get_mut(1).unwrap().attack_target = None;
    for _ in 0..60 {
        sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .barrel_facing
            .as_ref()
            .unwrap()
            .destination(),
        aimed,
        "a target-less building turret keeps its last aim forever"
    );
}

#[test]
fn gsi_08_04_voxel_turret_building_needs_an_exact_match_then_snaps_within_one_rot_step() {
    // `BuildingClass::GetFireError @ 0x00448010`..`0x00448045` narrows the
    // tolerance to `0x0000` when `BuildingTypeClass+0x16C5` (`TurretAnimIsVoxel=`)
    // is set, and `BuildingClass::Mission_Attack @ 0x0044B068`..`0x0044B0B3`
    // gives it a second chance in the SAME visit: within one `ROT=` step it
    // snaps `+0x388` with `UpdateFacing` and re-runs the error check.
    fn tower_rules(voxel: bool, rot: i32) -> RuleSet {
        let ini_str: String = format!(
            "[VehicleTypes]\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GTGCAN\n1=TGT\n\n\
[GTGCAN]\nStrength=900\nArmor=concrete\nPrimary=105mm\nROT={rot}\nTurret=yes\n\
TurretAnimIsVoxel={}\n\n\
[TGT]\nStrength=300\nArmor=heavy\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
            if voxel { "true" } else { "false" }
        );
        RuleSet::from_ini(&IniFile::from_str(&ini_str)).expect("tower fixture should parse")
    }
    fn fires_at_offset(voxel: bool, rot: i32, offset: u16) -> bool {
        let rules = tower_rules(voxel, rot);
        let mut sim = Simulation::new();
        let mut tower = GameEntity::test_default(1, "GTGCAN", "Americans", 5, 5);
        tower.category = crate::map::entities::EntityCategory::Structure;
        tower.lifecycle.in_limbo = false;
        tower.barrel_facing = Some(FacingClass::new(
            facing_from_5_5_to_5_9().wrapping_add(offset),
            rot,
        ));
        tower.attack_target = Some(AttackTarget::new(2));
        sim.substrate.entities.insert(tower);
        let target = GameEntity::test_default(2, "TGT", "Soviet", 5, 9); // due south -> 0x8000
        sim.substrate.entities.insert(target);
        sim.reveal(1);
        sim.reveal(2);
        use_test_interner(&mut sim);
        !run_combat_direct(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    }

    // ROT=1 means one step is 0x0100.
    assert!(fires_at_offset(true, 1, 0), "an exact match always passes");
    assert!(
        fires_at_offset(true, 1, 0x0100),
        "within one ROT step the voxel turret snaps and fires the same visit"
    );
    assert!(
        !fires_at_offset(true, 1, 0x0101),
        "one unit further and it must turn gradually instead"
    );
    // An SHP turret keeps the ordinary 0x0800 slack and never snaps.
    assert!(
        fires_at_offset(false, 1, 0x0800),
        "an SHP-turret building keeps the 0x0800 tolerance"
    );
    assert!(
        !fires_at_offset(false, 1, 0x0801),
        "and is refused past it, with no snap-and-retry"
    );
    let native: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/building_fire_turn.json"
    ))
    .unwrap();
    for row in native.as_array().unwrap() {
        let rot = row["rot"].as_i64().unwrap() as i32;
        let delta = row["delta"].as_u64().unwrap() as u16;
        assert_eq!(
            fires_at_offset(true, rot, delta),
            row["retry"].as_bool().unwrap(),
            "{row}"
        );
    }
}

#[test]
fn gsi_08_04_rotation_latch_refuses_the_shot_until_the_arc_finishes() {
    // `UnitClass::GetFireError @ 0x00740FD0` step 14
    // (`0x00741229`..`0x00741259`): firing-sequence byte `+0x68D` clear AND
    // rotation latch `+0x6AF` set AND the projectile's `ROT=`
    // (`BulletTypeClass+0x2DC`) zero -> `MOV EAX,0x4; RET 0xc`, i.e.
    // FIRE_ROTATING, returned BEFORE the OmniFire skip at `0x0074125C` and
    // before the step-17 angle test. A turret that has committed to an arc may
    // not fire partway through it, even once it is inside 0x0800.
    fn fires_with_latch(latch: bool, projectile_rot: u32, offset: u16) -> bool {
        let rules = rules_with_homing_projectile(projectile_rot);
        let mut sim = Simulation::new();
        spawn_turreted(&mut sim, 1, 5, 5, 5);
        spawn_target(&mut sim, 2, 5, 9);
        use_test_interner(&mut sim);
        let attacker = sim.substrate.entities.get_mut(1).unwrap();
        attacker.attack_target = Some(AttackTarget::new(2));
        attacker.barrel_facing = Some(FacingClass::new(
            facing_from_5_5_to_5_9().wrapping_add(offset),
            5,
        ));
        attacker.turret_rotation_latch = latch;
        !run_combat_direct(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    }

    // Exactly on target, so nothing but the latch can be doing the refusing.
    assert!(
        fires_with_latch(false, 0, 0),
        "a finished arc fires: this is the control"
    );
    assert!(
        !fires_with_latch(true, 0, 0),
        "an armed +0x6AF refuses the shot even when the turret is dead on"
    );
    // A shot the angle arm would have passed at the very edge of 0x0800 is
    // still refused. (This does not by itself pin WHERE the refusal sits: a
    // refusal placed after a passing angle test returns the same answer. The
    // OmniFire pair below is the case that does pin it.)
    assert!(
        !fires_with_latch(true, 0, 0x0800),
        "an armed latch refuses even at the edge the angle arm accepts"
    );

    // The placement pin. `0x0074125C` — the OmniFire test that skips the whole
    // step-17 angle arm — is the JUMP TARGET of all three step-14 skips, so
    // step 14 runs BEFORE it: an OmniFire weapon, which never turns its turret
    // and is never angle-gated, is still refused while `+0x6AF` holds. Put the
    // refusal inside VERA's `!weapon.omni_fire` block instead and this case
    // fires. The offset is a quarter turn, far outside any tolerance, to prove
    // the angle arm really is being skipped.
    fn fires_with_latch_omni(latch: bool) -> bool {
        let rules = rules_with_omni_fire();
        let mut sim = Simulation::new();
        spawn_turreted(&mut sim, 1, 5, 5, 5);
        spawn_target(&mut sim, 2, 5, 9);
        use_test_interner(&mut sim);
        let attacker = sim.substrate.entities.get_mut(1).unwrap();
        attacker.attack_target = Some(AttackTarget::new(2));
        attacker.barrel_facing = Some(FacingClass::new(
            facing_from_5_5_to_5_9().wrapping_add(0x4000),
            5,
        ));
        attacker.turret_rotation_latch = latch;
        !run_combat_direct(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    }
    assert!(
        fires_with_latch_omni(false),
        "an OmniFire weapon shoots a quarter turn off-axis: the angle arm is skipped"
    );
    assert!(
        !fires_with_latch_omni(true),
        "but step 14 precedes that skip, so an armed +0x6AF still refuses it"
    );

    // `MOV EDX,[EBX+0xA0]; MOV EAX,[EDX+0x2DC]; TEST; JNZ 0x0074125C` at
    // `0x0074123D`..`0x0074124B` skips the whole refusal when the projectile
    // homes: a missile turret keeps shooting mid-arc where a cannon may not.
    // It is the same `+0x2DC` that widens the step-17 tolerance to 0x1000.
    assert!(
        fires_with_latch(true, 30, 0),
        "a homing projectile skips FIRE_ROTATING and fires mid-arc"
    );
    assert!(
        fires_with_latch(true, 30, 0x1000),
        "with its widened tolerance still applying"
    );
}

#[test]
fn gsi_08_04_no_shot_lands_while_the_turret_is_still_swinging() {
    // The same step 14 refusal, driven through the production
    // `Simulation::advance_tick` path rather than the combat entry directly,
    // so the latch really is committed by `apply_unit_facing` and really is
    // read back by the gate.
    //
    // A half-turn arc at `ROT=1` (0x0100 per frame) takes ~128 frames, and its
    // last 0x0800 takes 8 of them. Without the FIRE_ROTATING refusal the tank
    // opens fire for those 8 frames while the turret is visibly still swinging;
    // with it, no shot lands until the arc is over.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 1); // facing north
    spawn_target(&mut sim, 2, 5, 9); // due south: half a turn away
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(1);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    let start_hp = sim.substrate.entities.get(2).unwrap().health.current;
    let mut fired_on_tick = None;
    for tick in 0..400u32 {
        let frame = sim.session.binary_frame;
        let rotating = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .barrel_facing
            .as_ref()
            .is_some_and(|barrel| barrel.is_rotating(frame));
        sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
        let hp = sim
            .substrate
            .entities
            .get(2)
            .map_or(0, |target| target.health.current);
        if hp < start_hp {
            assert!(
                !rotating,
                "tick {tick}: a shot landed while the turret was still mid-arc"
            );
            fired_on_tick = Some(tick);
            break;
        }
    }
    assert!(
        fired_on_tick.is_some(),
        "the refusal must not deadlock the unit - the arc ends and the shot lands"
    );
}

#[test]
fn gsi_08_04_a_two_step_re_aim_is_refused_for_the_whole_arc_not_only_its_first_frame() {
    // Where the `+0x6AF` store SITS, in frames a player can count.
    //
    // Native writes the latch at `0x00736B16`, which is AFTER arm A's
    // `FacingClass::Set @ 0x004C9220` (`CALL` at `0x00736A89`) and after the
    // `Is_Rotating @ 0x004C9480` call at `0x00736B11` that supplies its value.
    // `Set` stores `duration = abs(delta)/rate` and `start = frame`, so a `Set`
    // of one or more `ROT=` steps makes `Is_Rotating` true on that same frame:
    // the arc is latched from frame one, not from frame two.
    //
    // At `[MTNK] ROT=5` the step is 0x0500 and a 0x0A00 re-aim (about 14
    // degrees, the band a tracked target drifts into between shots) takes two
    // frames. The whole engagement, per the binary:
    //
    //   tick 0  arm A `Set`s; the angle test refuses (0x0A00 > 0x0800);
    //           latch := Is_Rotating == true
    //   tick 1  latch refuses, and the animated turret is ALREADY inside
    //           0x0800 — a latch committed pre-`Set` would fire here
    //   tick 2  latch still refuses; the turret has arrived
    //   tick 3  the arc is over, the latch clears, the shot lands
    //
    // Committing the latch before the `Set` reads false on tick 0 and fires on
    // tick 1: two frames early, which moves the damage frame and this shot's
    // position in the RNG stream.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 5);
    spawn_target(&mut sim, 2, 5, 9);
    use_test_interner(&mut sim);
    let rules = rules_with_mtnk_rot(5);
    let desired = facing_from_5_5_to_5_9();
    {
        let attacker = sim.substrate.entities.get_mut(1).unwrap();
        attacker.attack_target = Some(AttackTarget::new(2));
        // A finished arc that the target has since drifted two ROT steps away
        // from: not rotating, so nothing but this tick's own `Set` can arm the
        // latch.
        attacker.barrel_facing = Some(FacingClass::new(desired.wrapping_add(0x0A00), 5));
    }

    let start_hp = sim.substrate.entities.get(2).unwrap().health.current;
    let mut fired_on_tick = None;
    let mut aimed_on_tick = None;
    for tick in 0..16u32 {
        let frame = sim.session.binary_frame;
        let attacker = sim.substrate.entities.get(1).unwrap();
        let animated = attacker.barrel_facing.as_ref().unwrap().current(frame);
        let off_axis = i32::from(animated.wrapping_sub(desired) as i16).abs();
        if off_axis <= 0x0800 && aimed_on_tick.is_none() {
            aimed_on_tick = Some(tick);
        }
        sim.advance_tick(&[], Some(&rules), &empty_height_map(), None, None, 67);
        let hp = sim
            .substrate
            .entities
            .get(2)
            .map_or(0, |target| target.health.current);
        if hp < start_hp {
            fired_on_tick = Some(tick);
            break;
        }
    }

    // The angle arm alone would have let the shot through here, so this is what
    // separates a latch committed post-`Set` from one committed pre-`Set`.
    assert_eq!(
        aimed_on_tick,
        Some(1),
        "fixture check: the animated turret must already be inside 0x0800 on tick 1"
    );
    assert_eq!(
        fired_on_tick,
        Some(3),
        "gamemd holds fire for the whole two-frame arc; firing on tick 1 means the \
         latch was committed before arm A's Set instead of after it"
    );
}
