//! Per-tick particle-system AI dispatch.
//!
//! Drives each active `ParticleSystem` in shared LogicVector order: temporarily
//! takes it from storage, runs per-`BehavesLike` AI, decrements lifetime, then
//! reinserts it. Completed systems enter the common deferred-delete path.
//! Smoke (C2), Gas (C3), and Fire (C4) have
//! full bodies; Spark dispatch is active while Railgun remains a no-op.
//!
//! Pulling each system out before ticking lets the inner AI take `&mut Simulation`
//! freely — needed for spawning child systems and applying damage to entities —
//! without aliasing the store. `reinsert` puts the system back at its original
//! `stable_id` so deterministic iteration order is preserved.

use crate::rules::particle_system_type::ParticleSystemBehavesLike;
use crate::rules::particle_type::{ParticleBehavesLike, ParticleTypeId};
use crate::rules::ruleset::RuleSet;
use crate::sim::particles::ParticleSystem;
use crate::sim::world::Simulation;
use thiserror::Error;

use super::spark::{
    SparkCollisionFacts, SparkKernelError, SparkMotionStep, begin_particle_tick,
    finish_particle_tick, gravity_as_stored_f32,
};
use super::spark_world::{SparkCollisionWorld, SparkWorldError};

// Errors retained at the production Spark begin/query/finish boundary. Railgun
// alone remains dormant in the behavior dispatch below.
#[derive(Debug, Error)]
pub(crate) enum SparkSystemTickError {
    #[error("particle type {0:?} is not behavior-3 Spark")]
    NonSparkParticle(ParticleTypeId),
    #[error(transparent)]
    Kernel(#[from] SparkKernelError),
    #[error(transparent)]
    World(#[from] SparkWorldError),
}

/// Resolve the SHP frame count for a particle's `Image=` field from the
/// immutable rules-side asset catalog. Returns 0 when the type has no image
/// or the optional SHP was not bound.
pub(super) fn resolve_image_frame_count(
    rules: &RuleSet,
    pt: &crate::rules::particle_type::ParticleType,
) -> u16 {
    let Some(image) = pt.image.as_deref() else {
        return 0;
    };
    rules.effect_frame_count(image).unwrap_or(0)
}

/// Advance one Tier-2 particle's animation-state machine by one tick.
///
/// A sub-tick counter increments every call; when it hits a per-type
/// denominator computed from the SHP's frame-count parity and the type's
/// `StateAIAdvance` divisor, `animation_state` advances by 1. Reaching
/// `EndStateAI` either marks the particle for deletion (when
/// `DeleteOnStateLimit`) or resets the state to 0. Reaching
/// `Translucent50State` or `Translucent25State` writes the corresponding
/// translucency byte the renderer reads.
///
/// `image_frame_count` is the SHP frame count from
/// the rules-side asset catalog. When it's 0 (image not registered or missing
/// SHP), the denominator falls through to `1 + StateAIAdvance`.
pub(super) fn advance_state(
    p: &mut crate::sim::particles::Particle,
    pt: &crate::rules::particle_type::ParticleType,
    image_frame_count: u16,
) {
    let parity_bit = (image_frame_count % 2) as u8;
    let denom = (parity_bit + 1).saturating_add(pt.state_ai_advance).max(1);

    p.state_advance_counter = p.state_advance_counter.wrapping_add(1);
    if p.state_advance_counter % denom != 0 {
        return;
    }

    p.animation_state = p.animation_state.saturating_add(1);

    if p.animation_state == pt.end_state_ai {
        if pt.delete_on_state_limit {
            p.marked_for_deletion = true;
        } else {
            p.animation_state = 0;
        }
    }

    if pt.translucent_50_state != 0xFF && p.animation_state >= pt.translucent_50_state {
        p.translucency = 0x19;
    }
    if pt.translucent_25_state != 0xFF && p.animation_state >= pt.translucent_25_state {
        p.translucency = 0x32;
    }
}

/// Dispatch one particle-system object at its current LogicVector slot.
///
/// The caller owns live-vector traversal. Keeping the per-object body here
/// preserves same-pass append and compacting-removal behavior instead of
/// snapshotting particle systems into a later global phase.
pub(crate) fn tick_particle_system(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(mut sys) = sim.particle_systems_mut().take_for_tick(id) else {
        return false;
    };

    tick_one_system(&mut sys, sim, rules);

    // `ParticleSystemClass::AI @ 0x0062FD60`: decrement `+0xEC`, and when the
    // decremented value is exactly zero call vtable `+0xF8` — an entry
    // (`0x006301E0`) whose whole body is `*(byte*)(this+0xF8) = 1`. A negative
    // lifetime (`Lifetime=` absent, which is every stock Spark system) walks
    // past zero and never triggers it, exactly as here.
    sys.lifetime -= 1;
    if sys.lifetime == 0 {
        sys.done_spawning = true;
    }

    // `alive && this[+0xF8] && particle_count == 0`, from the same function.
    let retires = sys.done_spawning && sys.particles.is_empty();
    sim.particle_systems_mut().reinsert_after_tick(sys);
    if retires {
        sim.retire_particle_system(id);
    }
    true
}

/// Compatibility batch entry for focused particle tests and tools. Production
/// dispatch uses `tick_particle_system` from the shared live-object walk.
pub fn tick_particle_systems(sim: &mut Simulation, rules: &RuleSet) {
    for id in sim.live_object_order_snapshot() {
        tick_particle_system(sim, rules, id);
    }
}

fn tick_one_system(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    let behaves_like = rules.particle_system_type(sys.type_id).behaves_like;
    match behaves_like {
        ParticleSystemBehavesLike::Smoke => tick_smoke(sys, sim, rules),
        ParticleSystemBehavesLike::Gas => tick_gas(sys, sim, rules),
        ParticleSystemBehavesLike::Fire => tick_fire(sys, sim, rules),
        ParticleSystemBehavesLike::Spark => tick_spark(sys, sim, rules),
        ParticleSystemBehavesLike::Railgun => {
            // Tier 3 — no-op.
        }
    }
}

/// gamemd-derived: `ParticleSystemClass::AI_Spark @ 0x0062E840` in its native
/// order — the burst/countdown/facing half first (`spark_spawn.rs`), then the
/// per-particle dispatch and the backward destroy walk at `0x0062ECE9`.
fn tick_spark(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    if let Err(error) = super::spark_spawn::spark_spawn_pass(sys, sim, rules) {
        log::warn!(
            "particles: Spark system {} spawn pass failed: {error}",
            sys.stable_id
        );
        return;
    }
    if let Err(error) = tick_spark_system_compat(sys, sim, rules) {
        log::warn!(
            "particles: Spark system {} particle pass failed: {error}",
            sys.stable_id
        );
    }
}

fn tick_smoke(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    super::smoke::tick_system(sys, sim, rules);
}

fn tick_gas(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    super::gas::tick_system(sys, sim, rules);
}

fn tick_fire(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    super::fire::tick_system(sys, sim, rules);
}

/// The per-particle half of `AI_Spark`: `ParticleClass::AI_Dispatch` forward
/// over every particle, then the backward destroy walk at `0x0062ED0E`.
///
/// Production dispatch reaches this through [`tick_spark`]; parity tests call
/// it directly so both use the same begin/query/finish ownership boundary.
pub(crate) fn tick_spark_system_compat(
    sys: &mut ParticleSystem,
    sim: &mut Simulation,
    rules: &RuleSet,
) -> Result<(), SparkSystemTickError> {
    tick_spark_system_with_query(sys, sim, rules, |sim, rules, motion| {
        SparkCollisionWorld::new(sim, rules)?.query(motion)
    })
}

// Shared kernel behind both the production dispatch and the parity tests.
fn tick_spark_system_with_query<F>(
    sys: &mut ParticleSystem,
    sim: &mut Simulation,
    rules: &RuleSet,
    mut query: F,
) -> Result<(), SparkSystemTickError>
where
    F: FnMut(
        &Simulation,
        &RuleSet,
        SparkMotionStep,
    ) -> Result<SparkCollisionFacts, SparkWorldError>,
{
    let gravity = gravity_as_stored_f32(rules.general.gravity)?;
    for particle in &mut sys.particles {
        let particle_type = rules.particle_type(particle.type_id);
        if particle_type.behaves_like != ParticleBehavesLike::Spark {
            return Err(SparkSystemTickError::NonSparkParticle(particle.type_id));
        }

        let motion = begin_particle_tick(particle, gravity)?;
        let collision = query(sim, rules, motion)?;
        finish_particle_tick(
            particle,
            motion,
            collision,
            particle_type.color_speed,
            particle_type.color_list.len(),
            sim.particle_rng(),
        )?;
    }

    // Native container cleanup walks backward so removals cannot perturb the
    // forward tick/RNG order of surviving particles.
    for index in (0..sys.particles.len()).rev() {
        if sys.particles[index].marked_for_deletion {
            sys.particles.remove(index);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::rules::particle_type::ParticleTypeId;
    use crate::sim::particles::{Particle, ParticleSystem, SparkRuntimeState};
    use crate::sim::rng::SimRng;
    use crate::util::fixed_math::SimFixed;
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    use glam::IVec3;

    fn empty_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str("")).expect("empty rules parse")
    }

    fn build_rules(behaves_like: &str, lifetime: i32) -> RuleSet {
        let ini_text = format!(
            "[ParticleSystems]\n\
             1=Sys\n\
             [Sys]\n\
             BehavesLike={behaves_like}\n\
             Lifetime={lifetime}\n",
        );
        RuleSet::from_ini(&IniFile::from_str(&ini_text)).expect("rules parse")
    }

    fn fake_system(type_id: ParticleSystemTypeId, lifetime: i32) -> ParticleSystem {
        ParticleSystem {
            stable_id: 0,
            in_logic_vector: false,
            type_id,
            coords: IVec3::ZERO,
            offset: IVec3::ZERO,
            particles: Vec::new(),
            spawn_timer: SimFixed::from_num(0),
            lifetime,
            spark_spawn_frames: 0,
            facing: 0x1D,
            directionless: false,
            attached_entity: None,
            owner_entity: None,
            target_coords: IVec3::ZERO,
            owner_house: None,
            done_spawning: false,
        }
    }

    fn insert_live_system(sim: &mut Simulation, mut system: ParticleSystem) -> u64 {
        let stable_id = sim.allocate_stable_id();
        system.stable_id = stable_id;
        sim.particle_systems_mut().insert(system);
        assert!(sim.reveal_particle_system(stable_id, None));
        stable_id
    }

    fn spark_rules(gravity: i32) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[General]\nFixtureOnly=1\n[AudioVisual]\nGravity={gravity}\n\
             [Particles]\n1=SparkP\n\
             [SparkP]\nBehavesLike=Spark\nColorList=255,255,255,128,128,64\nColorSpeed=.13\n"
        ));
        RuleSet::from_ini(&ini).expect("Spark rules parse")
    }

    fn spark_particle(x: i32, lifetime: i16) -> Particle {
        Particle {
            type_id: ParticleTypeId(0),
            coords: IVec3::new(x, 0, 100),
            previous_coords: IVec3::ZERO,
            origin: IVec3::ZERO,
            direction: [SimFixed::from_num(0); 3],
            velocity: SimFixed::from_num(0),
            lifetime_remaining: lifetime,
            damage_counter: 0,
            state_ai_advance: 0,
            animation_state: 0,
            translucency: 0,
            hit_ground: false,
            marked_for_deletion: false,
            drift_x: 0,
            drift_y: 0,
            drift_z: 0,
            current_color: [0; 3],
            color_index: 0,
            color_accumulator: SimFixed::from_num(0),
            spark: Some(SparkRuntimeState {
                velocity_x: NativeF32Bits::POSITIVE_ZERO,
                velocity_y: NativeF32Bits::POSITIVE_ZERO,
                velocity_z: NativeF32Bits::POSITIVE_ZERO,
                start_rgb: [255; 3],
                color_index: 0,
                color_accumulator: NativeF64Bits::POSITIVE_ZERO,
            }),
            prev_delta: [SimFixed::from_num(0); 3],
            state_advance_counter: 0,
        }
    }

    fn flat_facts() -> SparkCollisionFacts {
        SparkCollisionFacts {
            ground_z: 0,
            slope_matrix: Some(super::super::spark_world::slope_matrix(0).unwrap()),
            old_has_structural_bridge: false,
            candidate_has_structural_bridge: false,
            accepted_building: false,
            wall_overlay_id: None,
        }
    }

    #[test]
    fn empty_store_tick_is_a_no_op() {
        let mut sim = Simulation::new();
        let rules = empty_rules();
        tick_particle_systems(&mut sim, &rules);
        assert_eq!(sim.particle_systems().len(), 0);
    }

    #[test]
    fn finite_lifetime_uses_common_deferred_finalizer() {
        let rules = build_rules("Smoke", 3);
        let mut sim = Simulation::new();
        let id = insert_live_system(&mut sim, fake_system(ParticleSystemTypeId(0), 1));

        tick_particle_systems(&mut sim, &rules);

        assert!(sim.particle_systems().get(id).is_some());
        assert!(!sim.live_object_order_snapshot().contains(&id));
        assert_eq!(sim.substrate.pending_delete, vec![id]);

        sim.process_pending_delete();
        assert!(sim.particle_systems().get(id).is_none());
        assert!(sim.substrate.pending_delete.is_empty());
    }

    #[test]
    fn live_object_dispatch_ticks_particle_system_at_slot_and_preserves_compaction_skip() {
        let rules = build_rules("Smoke", 100);
        let mut sim = Simulation::new();
        let retiring = insert_live_system(&mut sim, fake_system(ParticleSystemTypeId(0), 1));
        let successor = insert_live_system(&mut sim, fake_system(ParticleSystemTypeId(0), 3));

        sim.object_ai_stage(Some(&rules));

        assert!(!sim.live_object_order_snapshot().contains(&retiring));
        assert_eq!(
            sim.particle_systems().get(successor).unwrap().lifetime,
            3,
            "removing the current live-vector slot shifts and skips its successor"
        );

        sim.object_ai_stage(Some(&rules));
        assert_eq!(
            sim.particle_systems().get(successor).unwrap().lifetime,
            2,
            "the surviving system runs on the next live-vector pass"
        );
    }

    #[test]
    fn infinite_lifetime_survives_when_empty() {
        let rules = build_rules("Smoke", -1);
        let mut sim = Simulation::new();
        insert_live_system(&mut sim, fake_system(ParticleSystemTypeId(0), -1));
        // -1 → -2, never == 0; marked stays false; survives.
        tick_particle_systems(&mut sim, &rules);
        assert_eq!(sim.particle_systems().len(), 1);
        assert_eq!(sim.live_object_order_snapshot().len(), 1);
    }

    #[test]
    fn reinsert_preserves_stable_id() {
        let rules = build_rules("Smoke", 100);
        let mut sim = Simulation::new();
        let id = insert_live_system(&mut sim, fake_system(ParticleSystemTypeId(0), 100));
        tick_particle_systems(&mut sim, &rules);
        assert!(sim.particle_systems().get(id).is_some());
        assert_eq!(sim.particle_systems().get(id).unwrap().lifetime, 99);
    }

    #[test]
    fn particle_frame_lookup_uses_rules_without_mutating_sim_interner() {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[Particles]\n0=SmokeP\n\
             [SmokeP]\nBehavesLike=Smoke\nImage=SMOKEIMG\n",
        ))
        .expect("particle rules");
        rules.set_effect_frame_count_for_test("SMOKEIMG", 5, 5);
        let mut sim = Simulation::new();
        let before = sim.interner.len();

        let count = resolve_image_frame_count(&rules, rules.particle_type(ParticleTypeId(0)));

        assert_eq!(count, 5);
        assert_eq!(sim.interner.len(), before);
        assert!(sim.interner.get("SMOKEIMG").is_none());
        let marker = sim.interner.intern("AFTER_EFFECT_LOOKUP");
        assert_eq!(marker.index() as usize, before);
    }

    #[test]
    fn gsi_05_13_lifetimeless_spark_system_retires_without_a_lifetime() {
        // `[SparkSys]` — the `[CombatDamage] DefaultSparkSystem` every electric
        // bolt resolves through — authors no `Lifetime=`, so `pst.lifetime` is
        // -1 and the countdown at `0x0062FD8B` walks past zero forever.
        // Removal therefore cannot come from the lifetime; it comes from the
        // shared `+0xF8` byte, which `AI_Spark` sets at `0x0062EC73` when the
        // spark countdown runs out. Modelling that byte as two Rust booleans
        // and retiring on only one of them leaked a system per Tesla discharge
        // for the rest of the match.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]
FixtureOnly=1
             [ParticleSystems]
1=SparkSys
             [SparkSys]
BehavesLike=Spark
ParticleCap=6
             SparkSpawnFrames=1
SpawnSparkPercentage=1
",
        ))
        .expect("lifetimeless spark rules parse");
        assert_eq!(
            rules.particle_system_type(ParticleSystemTypeId(0)).lifetime,
            -1,
            "the fixture must reproduce the stock missing-Lifetime default"
        );

        let mut sim = Simulation::with_seed(17);
        let mut system = fake_system(ParticleSystemTypeId(0), -1);
        system.spark_spawn_frames = 1;
        let id = insert_live_system(&mut sim, system);

        tick_particle_systems(&mut sim, &rules);

        // `retire_particle_system` conceals and queues; the store entry is
        // dropped by the later pending-delete flush, so the queue is the
        // observable decision here.
        assert!(
            sim.substrate.pending_delete.contains(&id),
            "a done-spawning, empty system retires even with Lifetime absent"
        );
    }

    #[test]
    fn spark_and_railgun_systems_survive_a_tick_with_nothing_to_do() {
        // Spark dispatch is live (`tick_spark`), Railgun is still a no-op.
        // Neither may retire a system early: the fixture system has exhausted
        // its spawn frames and holds no particles, so `AI_Spark`'s guard at
        // `0x0062E855` falls straight through and the lifetime alone governs.
        for behaves in ["Spark", "Railgun"] {
            let rules = build_rules(behaves, 100);
            let mut sim = Simulation::new();
            insert_live_system(&mut sim, fake_system(ParticleSystemTypeId(0), 100));
            tick_particle_systems(&mut sim, &rules);
            assert_eq!(sim.particle_systems().len(), 1, "{behaves} should survive");
        }
    }

    #[test]
    fn compat_owner_ticks_forward_then_removes_backward() {
        let rules = spark_rules(0);
        let mut sim = Simulation::with_seed(123);
        let mut sys = fake_system(ParticleSystemTypeId(0), 100);
        sys.particles.push(spark_particle(11, 1));
        sys.particles.push(spark_particle(22, 2));
        let mut seen = Vec::new();

        tick_spark_system_with_query(&mut sys, &mut sim, &rules, |_, _, motion| {
            seen.push(motion.old_coords.x);
            Ok(flat_facts())
        })
        .unwrap();

        assert_eq!(seen, vec![11, 22]);
        assert_eq!(sys.particles.len(), 1);
        assert_eq!(sys.particles[0].coords.x, 22);
        assert_eq!(sys.particles[0].lifetime_remaining, 1);

        let mut expected_rng = SimRng::new(123);
        expected_rng.next_range_u32_inclusive(0, 0x7fff_fffe);
        expected_rng.next_range_u32_inclusive(0, 0x7fff_fffe);
        assert_eq!(sim.rng_views().scenario, expected_rng.logical_view());
    }

    #[test]
    fn unavailable_query_keeps_persistent_z_but_consumes_no_rng_or_lifetime() {
        let rules = spark_rules(6);
        let mut sim = Simulation::with_seed(456);
        let rng_before = sim.rng_state().scenario;
        let mut sys = fake_system(ParticleSystemTypeId(0), 100);
        sys.particles.push(spark_particle(11, 9));

        let error = tick_spark_system_with_query(&mut sys, &mut sim, &rules, |_, _, _| {
            Err(SparkWorldError::MissingTerrain)
        })
        .unwrap_err();

        assert!(matches!(
            error,
            SparkSystemTickError::World(SparkWorldError::MissingTerrain)
        ));
        assert_eq!(sys.particles[0].coords, IVec3::new(11, 0, 100));
        assert_eq!(sys.particles[0].lifetime_remaining, 9);
        assert_eq!(
            sys.particles[0].spark.unwrap().velocity_z.bits(),
            0xc0c0_0000
        );
        assert_eq!(sim.rng_state().scenario, rng_before);
    }

    #[test]
    fn gsi_04_03_spark_shared_dummy_query_order_and_miss_continuation() {
        use super::super::spark_world::{SparkCellSelectionRole, SparkCollisionWorld};
        use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        use crate::map::resolved_terrain::ResolvedTerrainGrid;

        let rules = spark_rules(0);
        let mut sim = Simulation::with_seed(789);
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(
            1,
            1,
            vec![super::super::spark_world::tests::terrain_cell(0, 0)],
        ));
        sim.overlay_grid = None;
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .shared_cell_dummy()
            .set_bridge_flags_0x1180(BRIDGE_FLAG_STRUCTURAL);
        let mut sys = fake_system(ParticleSystemTypeId(0), 100);
        let mut particle = spark_particle(256, 2);
        particle.coords.z = 100;
        sys.particles.push(particle);

        let motion = SparkMotionStep {
            old_coords: IVec3::new(256, 0, 100),
            candidate_coords: IVec3::new(256, 0, 100),
            candidate_f32: [
                NativeF32Bits::from_bits(256.0f32.to_bits()),
                NativeF32Bits::POSITIVE_ZERO,
                NativeF32Bits::from_bits(100.0f32.to_bits()),
            ],
            persistent_velocity: [NativeF32Bits::POSITIVE_ZERO; 3],
            probe_velocity: [NativeF32Bits::POSITIVE_ZERO; 3],
        };
        let (facts, transcript) = SparkCollisionWorld::new(&sim, &rules)
            .unwrap()
            .query_with_transcript(motion)
            .unwrap();
        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 256, 0),
                (SparkCellSelectionRole::Cell, 256, 0),
            ]
        );
        assert!(facts.candidate_has_structural_bridge);
        assert!(!facts.old_has_structural_bridge);
        assert!(!facts.accepted_building);
        assert_eq!(facts.wall_overlay_id, None);
        assert!(facts.slope_matrix.is_none());

        tick_spark_system_compat(&mut sys, &mut sim, &rules).unwrap();

        assert_eq!(sys.particles.len(), 1);
        assert_eq!(sys.particles[0].coords, IVec3::new(256, 0, 100));
        assert_eq!(sys.particles[0].lifetime_remaining, 1);
        assert!(!sys.particles[0].marked_for_deletion);
        let mut expected_rng = SimRng::new(789);
        expected_rng.next_range_u32_inclusive(0, 0x7fff_fffe);
        assert_eq!(sim.rng_views().scenario, expected_rng.logical_view());
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .shared_cell_dummy()
                .snapshot()
                .coord,
            (1, 0)
        );

        let mut collision_sim = Simulation::with_seed(790);
        collision_sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(
            1,
            1,
            vec![super::super::spark_world::tests::terrain_cell(0, 0)],
        ));
        collision_sim.overlay_grid = None;
        let mut collision_particle = spark_particle(256, 2);
        collision_particle.coords.z = -1;
        let motion = begin_particle_tick(
            &mut collision_particle,
            gravity_as_stored_f32(rules.general.gravity).unwrap(),
        )
        .unwrap();
        let facts = SparkCollisionWorld::new(&collision_sim, &rules)
            .unwrap()
            .query(motion)
            .unwrap();
        assert!(facts.slope_matrix.is_some(), "a selected collision performs the final slope lookup");
        let particle_type = rules.particle_type(collision_particle.type_id);
        finish_particle_tick(
            &mut collision_particle,
            motion,
            facts,
            particle_type.color_speed,
            particle_type.color_list.len(),
            collision_sim.particle_rng(),
        )
        .unwrap();
        assert!(collision_particle.marked_for_deletion);
        assert_eq!(collision_particle.coords, IVec3::new(256, 0, 0));
        assert_eq!(collision_particle.lifetime_remaining, 1);
        let mut expected_rng = SimRng::new(790);
        expected_rng.next_range_u32_inclusive(0, 0x7fff_fffe);
        assert_eq!(collision_sim.rng_views().scenario, expected_rng.logical_view());
    }

    mod advance_state_tests {
        use super::*;
        use crate::rules::particle_type::ParticleTypeId;
        use crate::sim::particles::Particle;
        use crate::util::fixed_math::SimFixed;
        use glam::IVec3;

        fn pt_rules(extra: &str) -> RuleSet {
            let ini = format!("[Particles]\n1=Smk\n[Smk]\nBehavesLike=Smoke\nMaxEC=10\n{extra}\n");
            RuleSet::from_ini(&IniFile::from_str(&ini)).expect("rules parse")
        }

        fn fake_particle(pt: &crate::rules::particle_type::ParticleType) -> Particle {
            Particle {
                type_id: ParticleTypeId(0),
                coords: IVec3::ZERO,
                previous_coords: IVec3::ZERO,
                origin: IVec3::ZERO,
                direction: [SimFixed::from_num(0); 3],
                velocity: SimFixed::from_num(0),
                lifetime_remaining: 100,
                damage_counter: 0,
                state_ai_advance: pt.state_ai_advance,
                animation_state: pt.start_state_ai,
                translucency: pt.translucency,
                hit_ground: false,
                marked_for_deletion: false,
                drift_x: 0,
                drift_y: 0,
                drift_z: 0,
                current_color: [0; 3],
                color_index: 0,
                color_accumulator: SimFixed::from_num(0),
                spark: None,
                prev_delta: [SimFixed::from_num(0); 3],
                state_advance_counter: 0,
            }
        }

        #[test]
        fn even_frame_count_denom_is_state_ai_advance_plus_1() {
            // image_frame_count=20 (even), StateAIAdvance=4 → denom = (0+1) + 4 = 5.
            // After 4 ticks: counter=4, no advance. After 5: counter=5, animation_state=1.
            let rules = pt_rules("StateAIAdvance=4\nEndStateAI=99");
            let pt = rules.particle_type(ParticleTypeId(0));
            let mut p = fake_particle(pt);
            for _ in 0..4 {
                advance_state(&mut p, pt, 20);
            }
            assert_eq!(p.animation_state, 0, "no advance before denom");
            advance_state(&mut p, pt, 20);
            assert_eq!(p.animation_state, 1, "advance on tick 5");
        }

        #[test]
        fn odd_frame_count_denom_is_state_ai_advance_plus_2() {
            // image_frame_count=21 (odd), StateAIAdvance=4 → denom = (1+1) + 4 = 6.
            let rules = pt_rules("StateAIAdvance=4\nEndStateAI=99");
            let pt = rules.particle_type(ParticleTypeId(0));
            let mut p = fake_particle(pt);
            for _ in 0..5 {
                advance_state(&mut p, pt, 21);
            }
            assert_eq!(p.animation_state, 0);
            advance_state(&mut p, pt, 21);
            assert_eq!(p.animation_state, 1);
        }

        #[test]
        fn end_state_with_delete_on_state_limit_marks_for_deletion() {
            let rules = pt_rules("StateAIAdvance=0\nEndStateAI=2\nDeleteOnStateLimit=yes");
            let pt = rules.particle_type(ParticleTypeId(0));
            let mut p = fake_particle(pt);
            // denom = (0+1)+0 = 1 → advance every tick.
            advance_state(&mut p, pt, 4); // state 0→1
            assert!(!p.marked_for_deletion);
            advance_state(&mut p, pt, 4); // state 1→2 (==EndStateAI)
            assert!(p.marked_for_deletion);
        }

        #[test]
        fn end_state_without_delete_resets_to_zero() {
            let rules = pt_rules("StateAIAdvance=0\nEndStateAI=2");
            let pt = rules.particle_type(ParticleTypeId(0));
            let mut p = fake_particle(pt);
            advance_state(&mut p, pt, 4); // 0→1
            advance_state(&mut p, pt, 4); // 1→2 → reset to 0
            assert_eq!(p.animation_state, 0);
            assert!(!p.marked_for_deletion);
        }

        #[test]
        fn translucent_50_state_writes_0x19() {
            let rules = pt_rules("StateAIAdvance=0\nEndStateAI=99\nTranslucent50State=3");
            let pt = rules.particle_type(ParticleTypeId(0));
            let mut p = fake_particle(pt);
            for _ in 0..3 {
                advance_state(&mut p, pt, 4);
            }
            assert_eq!(p.translucency, 0x19, "Translucent50State sets 0x19");
        }

        #[test]
        fn translucent_25_state_writes_0x32() {
            let rules = pt_rules("StateAIAdvance=0\nEndStateAI=99\nTranslucent25State=2");
            let pt = rules.particle_type(ParticleTypeId(0));
            let mut p = fake_particle(pt);
            for _ in 0..2 {
                advance_state(&mut p, pt, 4);
            }
            assert_eq!(p.translucency, 0x32);
        }

        #[test]
        fn translucent_state_0xff_means_never() {
            // Both Translucent25State and Translucent50State default to 0xFF.
            let rules = pt_rules("StateAIAdvance=0\nEndStateAI=99");
            let pt = rules.particle_type(ParticleTypeId(0));
            let mut p = fake_particle(pt);
            for _ in 0..50 {
                advance_state(&mut p, pt, 4);
            }
            // Translucency should still be the spawn-time value.
            assert_eq!(p.translucency, pt.translucency);
        }

        #[test]
        fn frame_count_zero_uses_even_denominator() {
            // image_frame_count=0 → parity_bit = 0 → denom = (0+1)+0 = 1.
            let rules = pt_rules("StateAIAdvance=0\nEndStateAI=99");
            let pt = rules.particle_type(ParticleTypeId(0));
            let mut p = fake_particle(pt);
            advance_state(&mut p, pt, 0);
            assert_eq!(p.animation_state, 1, "denom=1 advances every tick");
        }

        #[test]
        fn counter_wraps_without_breaking_modulo() {
            // denom=5; let counter overflow.
            let rules = pt_rules("StateAIAdvance=4\nEndStateAI=99");
            let pt = rules.particle_type(ParticleTypeId(0));
            let mut p = fake_particle(pt);
            for _ in 0..260 {
                advance_state(&mut p, pt, 20);
            }
            // 260 ticks / denom 5 = 52 advances expected. EndStateAI=99, no reset.
            assert_eq!(p.animation_state, 52);
        }
    }
}
