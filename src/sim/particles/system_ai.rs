//! Per-tick particle-system AI dispatch.
//!
//! Drives each active system at its shared LogicVector slot. Gas and Smoke remain
//! in storage because synchronous damage can append children to that same
//! system; other behaviors temporarily take/reinsert their owned body.
//! Ordinary Smoke, Spark and Gas are connected. Fire retains movement/damage
//! residuals; Railgun body is still absent.

use crate::rules::particle_system_type::ParticleSystemBehavesLike;
use crate::rules::particle_type::ParticleBehavesLike;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
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
    #[error(transparent)]
    Kernel(#[from] SparkKernelError),
    #[error(transparent)]
    World(#[from] SparkWorldError),
}

/// The shared signed UID/EC cadence in Smoke62C540, Gas62C313 and
/// Fire62CBD6. EC is zero-extended; additions wrap before signed IDIV.
/// Smoke/Gas pass the type byte; Fire passes its constructor-adjusted byte.
/// Source: original gamemd, tools/spatial_oracle/damage_smoke.md.
pub(super) fn state_advance_due(p: &super::Particle, max_ec: i32, advance: u8) -> bool {
    let numerator = p
        .native_unique_id()
        .wrapping_add(max_ec)
        .wrapping_sub(i32::from(p.lifetime_remaining as u16));
    let divisor = (p.native_unique_id() & 1) + i32::from(advance as i8);
    numerator
        .checked_rem(divisor)
        .expect("native particle cadence requires valid IDIV")
        == 0
}

/// ParticleClass::AI_Dispatch62CE40 tail. Even a class body that returns
/// early or marks deletion still decrements EC once, with word wrap.
pub(super) fn finish_lifetime(p: &mut super::Particle) {
    p.lifetime_remaining = p.lifetime_remaining.wrapping_sub(1);
    if p.lifetime_remaining == 0 {
        p.marked_for_deletion = true;
    }
}

/// One AI_Dispatch62CE40 owner, selected by the PARTICLE's behavior rather
/// than its containing system. Spark retains its begin/query/finish boundary.
pub(super) fn tick_particle(
    p: &mut super::Particle,
    sim: &mut Simulation,
    rules: &RuleSet,
    source_house: Option<InternedId>,
) {
    if let Err(error) =
        tick_particle_with_query(p, sim, rules, source_house, |sim, rules, motion| {
            SparkCollisionWorld::new(sim, rules)?.query(motion)
        })
    {
        log::warn!(
            "particles: particle {} AI failed: {error}",
            p.native_unique_id()
        );
    }
}

fn tick_particle_with_query<F>(
    p: &mut super::Particle,
    sim: &mut Simulation,
    rules: &RuleSet,
    source_house: Option<InternedId>,
    mut query: F,
) -> Result<(), SparkSystemTickError>
where
    F: FnMut(
        &Simulation,
        &RuleSet,
        SparkMotionStep,
    ) -> Result<SparkCollisionFacts, SparkWorldError>,
{
    let pt = rules.particle_type(p.type_id);
    match pt.behaves_like {
        ParticleBehavesLike::Smoke => {
            super::smoke::tick_particle(p, pt, sim.session.binary_frame(), sim.particle_rng())
        }
        // gamemd.exe ParticleClass::AI_Gas (0x0062BD50), 62C2C2..62C2DC:
        // read the parent system's +FC source House regardless of its own
        // BehavesLike. Smoke, Fire and Spark retain that same reference.
        ParticleBehavesLike::Gas => super::gas::tick_particle(
            p,
            sim,
            rules,
            source_house,
            crate::sim::world::UninitContext::with_rules(rules),
        ),
        ParticleBehavesLike::Fire => super::fire::tick_particle(p, pt, sim.particle_rng()),
        ParticleBehavesLike::Spark => {
            let motion = begin_particle_tick(p, gravity_as_stored_f32(rules.general.gravity)?)?;
            let collision = query(sim, rules, motion)?;
            finish_particle_tick(
                p,
                motion,
                collision,
                pt.color_speed,
                pt.color_list.len(),
                sim.particle_rng(),
            )?;
            return Ok(()); // Spark finish calls the same lifetime owner.
        }
        ParticleBehavesLike::Railgun => {
            // RESIDUAL: Railgun body62C3A0 (dispatch62CE81) remains unported;
            // the shared lifetime tail still runs.
        }
    }
    finish_lifetime(p);
    Ok(())
}

/// Dispatch one particle-system object at its current LogicVector slot.
///
/// The caller owns live-vector traversal. Keeping the per-object body here
/// preserves same-pass append and compacting-removal behavior instead of
/// snapshotting particle systems into a later global phase.
pub(crate) fn tick_particle_system(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    context: crate::sim::world::UninitContext<'_>,
) -> bool {
    let Some(sys) = sim.particle_systems().get(id) else {
        return false;
    };
    let behavior = rules.particle_system_type(sys.type_id).behaves_like;
    if behavior == ParticleSystemBehavesLike::Gas {
        // Gas damage may synchronously kill another animal and append to this
        // same system. Retain its body in storage through those callbacks.
        super::gas::tick_system(sim, rules, id, context);
    } else if behavior == ParticleSystemBehavesLike::Smoke {
        super::smoke::tick_system(sim, rules, id);
    } else {
        let mut sys = sim.substrate.particle_systems.take_for_tick(id).unwrap();
        tick_one_system(&mut sys, sim, rules);
        sim.substrate.particle_systems.reinsert_after_tick(sys);
    }
    let Some(sys) = sim.substrate.particle_systems.get_mut(id) else {
        return true;
    };

    // `ParticleSystemClass::AI @ 0x0062FD60`: decrement `+0xEC`, and when the
    // decremented value is exactly zero call vtable `+0xF8` — an entry
    // (`0x006301E0`) whose whole body is `*(byte*)(this+0xF8) = 1`. A negative
    // lifetime (`Lifetime=` absent, which is every stock Spark system) walks
    // past zero and never triggers it, exactly as here.
    sys.lifetime = sys.lifetime.wrapping_sub(1);
    if sys.lifetime == 0 {
        sys.mark_done_spawning();
    }

    // `alive && this[+0xF8] && particle_count == 0`, from the same function.
    let retires = sys.done_spawning && sys.particles.is_empty();
    if retires {
        sim.retire_particle_system(id, context);
    }
    true
}

/// Compatibility batch entry for focused particle tests. Production
/// dispatch uses `tick_particle_system` from the shared live-object walk.
#[cfg(test)]
pub fn tick_particle_systems(sim: &mut Simulation, rules: &RuleSet) {
    for id in sim.live_object_order_snapshot() {
        tick_particle_system(
            sim,
            rules,
            id,
            crate::sim::world::UninitContext::with_rules(rules),
        );
    }
}

fn tick_one_system(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    let behaves_like = rules.particle_system_type(sys.type_id).behaves_like;
    match behaves_like {
        ParticleSystemBehavesLike::Smoke => unreachable!("Smoke retains its body during callbacks"),
        ParticleSystemBehavesLike::Gas => unreachable!("Gas retains its body during callbacks"),
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
    for particle in &mut sys.particles {
        tick_particle_with_query(particle, sim, rules, sys.owner_house, &mut query)?;
    }

    // Native container cleanup walks backward so removals cannot perturb the
    // forward tick/RNG order of surviving particles.
    for index in (0..sys.particles.len()).rev() {
        if sys.particles[index].marked_for_deletion {
            let particle = sys
                .remove_expired_child(sys.particles[index].stable_id())
                .expect("the current child is present");
            sim.retire_particle(particle, rules);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_host_stays_visible_when_gas_child_damage_creates_another_smoke_system() {
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=100\nArmor=light\nDamageParticleSystems=DAMAGE\n\
             [ParticleSystems]\n0=HOST\n1=DAMAGE\n[HOST]\nBehavesLike=Smoke\nHoldsWhat=CLOUD\n\
             [DAMAGE]\nBehavesLike=Smoke\n[Particles]\n0=CLOUD\n\
             [CLOUD]\nBehavesLike=Gas\nMaxEC=100\nMaxDC=1\nDamage=60\nWarhead=HIT\n\
             StateAIAdvance=3\nEndStateAI=10\nDeleteOnStateLimit=no\n\
             [Warheads]\n0=HIT\n[HIT]\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.session.set_binary_frame_for_test(1);
        let target = sim
            .spawn_object("TARGET", "Russians", 20, 20, 0, &rules)
            .unwrap();
        let host = sim
            .spawn_particle_system(
                rules.ps_type_id_by_name("HOST").unwrap(),
                IVec3::new(4608, 4608, 0),
                None,
                None,
                IVec3::ZERO,
                None,
                &rules,
            )
            .unwrap();
        let kind = rules.p_type_id_by_name("CLOUD").unwrap();
        let cloud = Particle::new(
            kind,
            IVec3::new(5248, 5248, 0),
            IVec3::ZERO,
            rules.particle_type(kind),
            &mut sim,
        );
        let system = sim.substrate.particle_systems.get_mut(host).unwrap();
        system.done_spawning = true;
        system.particles.push(cloud);
        assert!(tick_particle_system(
            &mut sim,
            &rules,
            host,
            crate::sim::world::UninitContext::with_rules(&rules),
        ));
        let target = sim.entities().get(target).unwrap();
        assert_eq!(target.health.current(), 40);
        assert!(target.damage_smoke_system_id.system().is_some());
        assert_eq!(sim.particle_systems().get(host).unwrap().particles.len(), 1);
    }
    use crate::rules::ini_parser::IniFile;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::rules::particle_type::ParticleTypeId;
    use crate::sim::particles::{Particle, ParticleSystem, SparkRuntimeState};
    use crate::sim::rng::SimRng;
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    use glam::IVec3;

    #[test]
    fn mixed_hosts_preserve_gas_source_house_for_allied_and_enemy_receivers() {
        for behavior in ["Smoke", "Fire", "Spark"] {
            let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
                 [TARGET]\nStrength=100\nArmor=light\n\
                 [ParticleSystems]\n0=HOST\n[HOST]\nBehavesLike={behavior}\n\
                 [Particles]\n0=CLOUD\n[CLOUD]\nBehavesLike=Gas\nMaxEC=100\n\
                 MaxDC=1\nDamage=5\nWarhead=PSYCH\nStateAIAdvance=3\n\
                 EndStateAI=10\nDeleteOnStateLimit=no\n[Warheads]\n0=PSYCH\n\
                 [PSYCH]\nPsychedelic=yes\n\
                 Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
            ));
            let rules = RuleSet::from_ini(&ini).unwrap();
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.session.set_binary_frame_for_test(1);
            let owner = sim.interner.intern("Americans");
            let host = sim
                .spawn_particle_system(
                    rules.ps_type_id_by_name("HOST").unwrap(),
                    IVec3::ZERO,
                    None,
                    None,
                    IVec3::ZERO,
                    Some(owner),
                    &rules,
                )
                .unwrap();
            sim.substrate
                .particle_systems
                .get_mut(host)
                .unwrap()
                .done_spawning = true;
            let mut targets = Vec::new();
            for (country, x) in [("Americans", 20), ("Russians", 24)] {
                let target = sim
                    .spawn_object("TARGET", country, x, 20, 0, &rules)
                    .unwrap();
                let kind = rules.p_type_id_by_name("CLOUD").unwrap();
                let cloud = Particle::new(
                    kind,
                    IVec3::new(i32::from(x) * 256 + 128, 5248, 0),
                    IVec3::ZERO,
                    rules.particle_type(kind),
                    &mut sim,
                );
                sim.substrate
                    .particle_systems
                    .get_mut(host)
                    .unwrap()
                    .particles
                    .push(cloud);
                targets.push(target);
            }
            assert!(tick_particle_system(
                &mut sim,
                &rules,
                host,
                crate::sim::world::UninitContext::with_rules(&rules),
            ));
            assert!(
                !sim.entities().get(targets[0]).unwrap().berserk.is_active(),
                "{behavior}: the source house protects its ally from Psychedelic"
            );
            assert!(
                sim.entities().get(targets[1]).unwrap().berserk.is_active(),
                "{behavior}: the enemy control must reach the same real damage receiver"
            );
        }
    }

    fn build_rules(behaves_like: &str, lifetime: i32) -> RuleSet {
        let ini_text = format!(
            "[General]\nWindDirection=1\n[ParticleSystems]\n\
             1=Sys\n\
             [Sys]\n\
             BehavesLike={behaves_like}\n\
             SpawnCutoff=13\n\
             Lifetime={lifetime}\n",
        );
        RuleSet::from_ini(&IniFile::from_str(&ini_text)).expect("rules parse")
    }

    fn fake_system(type_id: ParticleSystemTypeId, lifetime: i32) -> ParticleSystem {
        let mut system = ParticleSystem::test_fixture(0, type_id, IVec3::ZERO);
        system.lifetime = lifetime;
        system
    }

    fn insert_live_system(sim: &mut Simulation, mut system: ParticleSystem) -> u64 {
        let stable_id = sim.substrate.allocate_stable_id();
        system.stable_id = stable_id;
        sim.substrate.particle_systems.insert(system);
        assert!(
            sim.reveal_particle_system(stable_id, crate::rules::ruleset::RuleSet::empty_for_test())
        );
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
        let mut particle = Particle::test_fixture(ParticleTypeId(0), IVec3::new(x, 0, 100));
        particle.origin = IVec3::ZERO;
        particle.lifetime_remaining = lifetime;
        particle.spark = Some(SparkRuntimeState {
            velocity_x: NativeF32Bits::POSITIVE_ZERO,
            velocity_y: NativeF32Bits::POSITIVE_ZERO,
            velocity_z: NativeF32Bits::POSITIVE_ZERO,
            start_rgb: [255; 3],
            color_index: 0,
            color_accumulator: NativeF64Bits::POSITIVE_ZERO,
        });
        particle
    }

    fn flat_facts() -> SparkCollisionFacts {
        SparkCollisionFacts {
            ground_z: 0,
            slope_matrix: Some(crate::map::voxel_slope::slope_matrix(0).unwrap()),
            old_has_structural_bridge: false,
            candidate_has_structural_bridge: false,
            accepted_building: false,
            wall_overlay_id: None,
        }
    }

    #[test]
    fn finite_lifetime_uses_common_deferred_finalizer() {
        let rules = build_rules("Smoke", 3);
        let mut sim = Simulation::new();
        let id = insert_live_system(&mut sim, fake_system(ParticleSystemTypeId(0), 1));

        tick_particle_systems(&mut sim, &rules);

        assert!(sim.particle_systems().get(id).is_some());
        assert!(!sim.live_object_order_snapshot().contains(&id));
        assert_eq!(sim.substrate.pending_delete.object_ids(), vec![id]);

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
            sim.substrate.pending_delete.contains_object(&id),
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
        assert!(
            facts.slope_matrix.is_some(),
            "a selected collision performs the final slope lookup"
        );
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
        assert_eq!(
            collision_sim.rng_views().scenario,
            expected_rng.logical_view()
        );
    }
}
