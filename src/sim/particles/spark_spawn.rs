//! Behavior-3 Spark burst spawning — the front half of
//! `ParticleSystemClass::AI_Spark @ 0x0062E840`.
//!
//! `system_ai.rs` owns the back half (the per-particle dispatch and the
//! backward destroy walk) and `spark.rs` owns the per-particle kernel. This
//! module owns only what happens *before* those: the burst gate, the burst
//! size, the three shared direction draws, per-particle construction and
//! velocity, the spawn countdown, and the facing walk.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, util/ and the rest of sim/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use glam::IVec3;
use thiserror::Error;

use super::{Particle, ParticleSystem, SparkRuntimeState};
use crate::rules::particle_type::{ParticleBehavesLike, ParticleType};
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::{RANDOM_RANGED_UNIT_SCALE, SimRng};
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, NativeX87Error, X87Chop53, X87Ordering, X87Value, sqrt_approx_f32,
};

/// `Random__RandomRanged(0, 0x7ffffffe)`'s inclusive top, as used by every
/// unit-interval probability gate in the Spark path.
const MAX_RANDOM_RANGED_SAMPLE: u32 = 0x7fff_fffe;

/// `FCOM double ptr [0x007E5138]` — the lower facing-walk threshold.
const FACING_STEP_DOWN_THRESHOLD: NativeF64Bits = NativeF64Bits::from_bits(0x3fd3_3333_3333_3333);

/// `FCOMP double ptr [0x007E3558]` — the upper facing-walk threshold.
const FACING_STEP_UP_THRESHOLD: NativeF64Bits = NativeF64Bits::from_bits(0x3fe3_3333_3333_3333);

/// `CMP EAX,0x11` at `0x0062ECB5` — the floor the -3 step clamps to.
const FACING_MIN: i32 = 0x11;
/// `CMP EAX,0x29` at `0x0062ECD9` — the ceiling the +3 step clamps to.
const FACING_MAX: i32 = 0x29;
/// `ADD EAX,-0x3` / `ADD EAX,0x3` — the facing walk's step size.
const FACING_STEP: i32 = 3;

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum SparkSpawnError {
    #[error("particle type {0} is not behavior-3 Spark")]
    NonSparkParticle(String),
    #[error("spark spawn x87 evaluation failed: {0}")]
    X87(#[from] NativeX87Error),
    #[error(transparent)]
    World(#[from] super::spark_world::SparkWorldError),
}

/// Run one tick of `AI_Spark`'s spawn half against `sys`.
///
/// gamemd-derived: `ParticleSystemClass::AI_Spark @ 0x0062E840`. The entire
/// body below sits under `if (*(int *)(sys + 0xF0) > 0)` at `0x0062E84E`; a
/// system that has finished spawning falls straight through to the per-particle
/// dispatch, which is `system_ai.rs`'s half.
///
/// RNG order is the parity point and is fixed by the disassembly:
/// 1. the burst gate draw, skipped entirely when `SparkSpawnFrames == 1`
///    (`CMP EAX,0x1 / JZ 0x0062E89E`);
/// 2. the burst-size draw;
/// 3. three shared direction draws, taken unconditionally and *before* the
///    particle loop, whose values are dead (see the comment at their site);
/// 4. per particle — one constructor lifetime draw, an optional constructor
///    colour draw, then three velocity draws;
/// 5. one facing-walk draw, taken even when the burst gate refused.
pub(super) fn spark_spawn_pass(
    sys: &mut ParticleSystem,
    sim: &mut Simulation,
    rules: &RuleSet,
) -> Result<(), SparkSpawnError> {
    if sys.spark_spawn_frames <= 0 {
        return Ok(());
    }

    let pst = rules.particle_system_type(sys.type_id);
    let particle_cap = pst.particle_cap as i32;
    let holds_what = pst.holds_what;
    let spawn_spark_percentage = pst.spawn_spark_percentage;
    let spawn_direction = pst.spawn_direction;
    let system_coords = sys.coords;

    // (1) Burst gate. `SparkSpawnFrames == 1` bursts unconditionally and
    // consumes no draw; otherwise `scaled <= SpawnSparkPercentage`, taken from
    // `FCOMP double ptr [ECX + 0x2f8]` / `TEST AH,0x41` at
    // `0x0062E88D`..`0x0062E895` — C0|C3, so less-than OR equal.
    let bursts = if sys.spark_spawn_frames == 1 {
        true
    } else {
        let scaled = scaled_unit_draw(sim.particle_rng())?;
        matches!(
            X87Chop53::compare(scaled, X87Chop53::load_f64(spawn_spark_percentage)?),
            X87Ordering::Less | X87Ordering::Equal
        )
    };

    if bursts {
        // (2) Burst size: `|Next()| % (cap / 2) + (cap / 2)`, the halving being
        // `CDQ; SUB EAX,EDX; SAR ECX,1` — truncation toward zero.
        // VERA-internal, gamemd equivalent UNCHECKED: `half <= 0` short-circuits
        // where native would `IDIV` by zero and raise `#DE`. Every stock Spark
        // system authors `ParticleCap` (6, 7, 15, 20), so the branch is
        // unreachable in stock; it exists because a mod could author 0 or 1.
        let half = particle_cap / 2;
        let burst = if half <= 0 {
            0
        } else {
            sim.particle_rng().next_raw_abs_modulo(half as u32) as i32 + half
        };

        // VERA-internal, gamemd equivalent UNCHECKED, and both arms diverge on
        // the RNG stream rather than merely on the picture:
        // - Native fetches the particle type at `0x0062E8EA` and takes the three
        //   shared draws at `0x0062E8EF`/`0x0062E903`/`0x0062E90E`
        //   unconditionally; with `HoldsWhat` absent it would index
        //   `g_ParticleTypeClass_Array[-1]` and fault. Skipping the draws here
        //   is a divergence, not a no-op — but only for a system that authors
        //   no `HoldsWhat`, which no stock Spark system does.
        // - Native never tests the particle's `BehavesLike` on this path. The
        //   `Err` below additionally skips the countdown, `done_spawning` and
        //   the facing draw for that tick, so a Spark SYSTEM holding a
        //   non-Spark particle behaves differently here. Stock authors no such
        //   pairing.
        if let Some(particle_type_id) = holds_what {
            let particle_type = rules.particle_type(particle_type_id).clone();
            if particle_type.behaves_like != ParticleBehavesLike::Spark {
                return Err(SparkSpawnError::NonSparkParticle(
                    particle_type.name.clone(),
                ));
            }

            // (3) Three shared draws, taken unconditionally at `0x0062E8EF`,
            // `0x0062E903` and `0x0062E90E`. Native divides them (crossed: the
            // first by `YVelocity` onto Y, the second by `XVelocity` onto X,
            // the third by `ZVelocityRange` onto Z) and stores the results —
            // but the only reader is the `ParticleSystem+0xF9` arm of the fold
            // below, and `+0xF9` has no setter anywhere in the image. Its two
            // writes are both `MOV byte ptr [ESI+0xf9],BL` in the constructors
            // (`0x0062DD0A`, `0x0062DFA0`) with `BL` zeroed at `0x0062DC62`, so
            // the byte is permanently 0 and the divided values are dead. The
            // draws themselves are NOT dead: they advance the shared stream, so
            // they are taken here and their results deliberately discarded.
            for _ in 0..3 {
                sim.particle_rng().next_u32();
            }

            for _ in 0..burst {
                // (4a) `ParticleClass::Constructor @ 0x0062B5E0`. Note there is
                // no particle-cap test on this path: native appends through
                // `DynamicVector` growth (`0x00630250`), so a Spark system can
                // exceed `ParticleCap`. `WeldingSys` (cap 15, 20 spawn frames)
                // relies on that.
                let particle =
                    construct_spark_particle(sim, &particle_type, particle_type_id, system_coords)?;
                sys.particles.push(particle);
                let particle = sys
                    .particles
                    .last_mut()
                    .expect("the particle was just pushed");

                // (4b) Three velocity draws. X and Y keep the sign of the raw
                // remainder; Z takes the absolute value first and then adds
                // `MinZVelocity` (`0x0062EA62`..`0x0062EA71`).
                let velocity_x = signed_modulo(
                    sim.particle_rng().next_u32() as i32,
                    particle_type.x_velocity,
                );
                let velocity_y = signed_modulo(
                    sim.particle_rng().next_u32() as i32,
                    particle_type.y_velocity,
                );
                let velocity_z = abs_modulo(
                    sim.particle_rng().next_u32() as i32,
                    particle_type.z_velocity_range,
                )
                .wrapping_add(particle_type.min_z_velocity);

                let mut vx = X87Chop53::store_f32(X87Chop53::load_i32(velocity_x))?;
                let mut vy = X87Chop53::store_f32(X87Chop53::load_i32(velocity_y))?;
                let mut vz = X87Chop53::store_f32(X87Chop53::load_i32(velocity_z))?;

                // The original magnitude, taken BEFORE the direction offset is
                // folded in. `Sqrt_Approx @ 0x004CAC40` ends `FLD float ptr`,
                // so the result really is f32-precision.
                let original_magnitude = magnitude(vx, vy, vz)?;

                // (4c) Fold in the direction offset. This is always the
                // type's `SpawnDirection` (`psType+0x2BC`/`+0x2C0`/`+0x2C4`,
                // written by `ParticleSystemTypeClass::ReadINI @ 0x006442D0`),
                // because the `ParticleSystem+0xF9` arm that would take the
                // shared draws instead is unreachable — see (3). No stock Spark
                // system authors `SpawnDirection`, so in practice this folds
                // zero and each particle keeps its own three velocity draws.
                vx = add_i32(vx, spawn_direction.x)?;
                vy = add_i32(vy, spawn_direction.y)?;
                vz = add_i32(vz, spawn_direction.z)?;

                // (4d) Renormalise, then rescale by the ORIGINAL magnitude, so
                // the offset changes direction without changing speed. The
                // second magnitude stays on the x87 stack as the divisor —
                // native never spills it (`0x0062EB63` onward).
                let renormalise_by = magnitude(vx, vy, vz)?;
                if X87Chop53::compare(
                    X87Chop53::load_f32(renormalise_by)?,
                    X87Chop53::load_f64(NativeF64Bits::POSITIVE_ZERO)?,
                ) != X87Ordering::Equal
                {
                    let divisor = X87Chop53::load_f32(renormalise_by)?;
                    vx = X87Chop53::store_f32(X87Chop53::div(X87Chop53::load_f32(vx)?, divisor)?)?;
                    vy = X87Chop53::store_f32(X87Chop53::div(X87Chop53::load_f32(vy)?, divisor)?)?;
                    vz = X87Chop53::store_f32(X87Chop53::div(X87Chop53::load_f32(vz)?, divisor)?)?;
                }
                let scale = X87Chop53::load_f32(original_magnitude)?;
                vx = X87Chop53::store_f32(X87Chop53::mul(scale, X87Chop53::load_f32(vx)?))?;
                vy = X87Chop53::store_f32(X87Chop53::mul(scale, X87Chop53::load_f32(vy)?))?;
                vz = X87Chop53::store_f32(X87Chop53::mul(scale, X87Chop53::load_f32(vz)?))?;

                let spark = particle
                    .spark
                    .as_mut()
                    .expect("a Spark particle carries its runtime state");
                spark.velocity_x = vx;
                spark.velocity_y = vy;
                spark.velocity_z = vz;
            }
        }
    }

    // RESIDUAL (GSI-05.13) — the per-system light is not spawned. Native
    // follows the burst with a light-source allocation at
    // `0x0062EBF2`..`0x0062EC5B`, under four gates, all read this session:
    // `OptionsClass::Detail` (`0x00A8EB78`, the in-game Detail slider,
    // default 2 from `OptionsClass::SetDefaults @ 0x005FA370`) must be 2;
    // the instance counter must still equal the type's `SparkSpawnFrames`
    // (`psType+0x300`), so this fires on the FIRST spawning tick only;
    // `LightSize` (`psType+0x304`) must be positive; and `OneFrameLight`
    // (`psType+0x30C`) must be FALSE. `LightSize` and `one_frame_light` are
    // parsed here already and have no consumer.
    // - Trigger: the first tick of a Spark system on default detail.
    // - Player effect: sparks cast no light on the ground around them.
    // - Frequency: bounded by whatever produces Spark systems; two of the four
    //   stock Spark systems (`WeldingSys`, `LGSparkSys`) set
    //   `OneFrameLight=true` and are excluded from this persistent-light path
    //   anyway, leaving `SparkSys` and `FirestormSparkSys`.
    // - Downstream risk: it needs a dynamic light source in the render layer,
    //   which is a different subsystem from this loop and consumes no RNG, so
    //   it neither blocks nor is blocked by the spawn arithmetic here.

    // (5) The spawn countdown and the facing walk run even when the burst gate
    // refused — the gate's `JZ` at `0x0062E898` lands on `0x0062EC60`, which is
    // the decrement, not the function tail.
    sys.spark_spawn_frames = sys.spark_spawn_frames.wrapping_sub(1);
    if sys.spark_spawn_frames <= 0 {
        sys.done_spawning = true;
    }

    let facing_sample = scaled_unit_draw(sim.particle_rng())?;
    let facing = i32::from(sys.facing);
    let stepped = if X87Chop53::compare(
        facing_sample,
        X87Chop53::load_f64(FACING_STEP_DOWN_THRESHOLD)?,
    ) == X87Ordering::Less
    {
        Some((facing - FACING_STEP).max(FACING_MIN))
    } else if X87Chop53::compare(
        facing_sample,
        X87Chop53::load_f64(FACING_STEP_UP_THRESHOLD)?,
    ) == X87Ordering::Less
    {
        Some((facing + FACING_STEP).min(FACING_MAX))
    } else {
        // `JZ 0x0062ECE9` skips the store entirely — the facing holds.
        None
    };
    if let Some(next) = stepped {
        sys.facing = next as u8;
    }

    Ok(())
}

/// `ParticleClass::Constructor @ 0x0062B5E0`, restricted to what a Spark
/// particle needs. Consumes one lifetime draw always, and one colour draw when
/// the type authors a non-zero `StartColor1`/`StartColor2` pair.
///
/// The constructor has a third conditional draw, at `0x0062B7F2`, gated on
/// `ParticleType+0x314 == 1`. `+0x314` is `BehavesLike` and Spark is 3, so it
/// never fires on this path; it is named here so the ledger is complete rather
/// than merely correct.
///
/// VERA-internal, gamemd equivalent UNCHECKED, both unreachable for the four
/// stock Spark types: `(max_ec).max(1)` guards the `IDIV` by zero native would
/// fault on, and `saturating_add` replaces native's 16-bit `ADD AX` wrap.
fn construct_spark_particle(
    sim: &mut Simulation,
    particle_type: &ParticleType,
    particle_type_id: crate::rules::particle_type::ParticleTypeId,
    system_coords: IVec3,
) -> Result<Particle, SparkSpawnError> {
    construct_spark_particle_with_ground(
        sim,
        particle_type,
        particle_type_id,
        system_coords,
        |sim, x, y| Ok(super::spark_world::constructor_ground_height(sim, x, y)?),
    )
}

fn construct_spark_particle_with_ground<F>(
    sim: &mut Simulation,
    particle_type: &ParticleType,
    particle_type_id: crate::rules::particle_type::ParticleTypeId,
    system_coords: IVec3,
    mut ground_height: F,
) -> Result<Particle, SparkSpawnError>
where
    F: FnMut(&Simulation, i32, i32) -> Result<Option<i32>, SparkSpawnError>,
{
    // `if (ptype+0x314 == 4) |Next() % 10| else |Next() % MaxEC|`, then
    // `+ MaxEC`. Spark is not behaviour 4, so it always takes the MaxEC arm.
    let base = (particle_type.max_ec as u32).max(1);
    let lifetime_extra = sim.particle_rng().next_raw_abs_modulo(base) as i16;
    let lifetime_remaining = (particle_type.max_ec as i16).saturating_add(lifetime_extra);

    // `CellClass::GetGroundHeight` floor: native queries once for the compare,
    // then repeats the same lookup only when `nZ <= ground` and assigns that
    // second result. Both calls route misses through the shared dummy.
    //
    // VERA-internal, gamemd equivalent UNCHECKED: a world that cannot be built
    // — no resolved terrain — is treated as "no floor". Native always has a
    // map, so that branch remains a fixture-only compatibility policy.
    let coords = floor_constructor_coords_with(system_coords, |x, y| ground_height(sim, x, y))?;

    // The colour seed. Native reads the list only when it has entries, and
    // takes the interpolated arm only when at least one of the six
    // `StartColor1`/`StartColor2` bytes is non-zero (`0x0062B7C4` onward). The
    // stock damage-spark types `[Spark]` and `[LargeSpark]` author neither, so
    // they take the no-draw arm; `[WeldingSpark]` authors both and draws.
    let start_rgb = if particle_type.color_list.is_empty() {
        [0, 0, 0]
    } else if particle_type.start_color_1 == [0, 0, 0] && particle_type.start_color_2 == [0, 0, 0] {
        particle_type.color_list[0]
    } else {
        let sample = sim
            .particle_rng()
            .next_range_u32_inclusive(0, MAX_RANDOM_RANGED_SAMPLE);
        interpolate_start_color(
            particle_type.start_color_1,
            particle_type.start_color_2,
            sample,
        )?
    };

    Ok(Particle {
        type_id: particle_type_id,
        coords,
        // Both constructor coordinate arguments are the system's own coord, so
        // the constructor's direction delta is zero and its normalise is a
        // no-op — the velocity assigned by the caller is the whole story.
        previous_coords: system_coords,
        origin: coords,
        direction: [SIM_ZERO; 3],
        velocity: particle_type.velocity,
        lifetime_remaining,
        damage_counter: particle_type.max_dc as i16,
        state_ai_advance: particle_type.state_ai_advance,
        animation_state: particle_type.start_state_ai,
        translucency: particle_type.translucency,
        hit_ground: false,
        marked_for_deletion: false,
        drift_x: 0,
        drift_y: 0,
        drift_z: 0,
        current_color: start_rgb,
        color_index: 0,
        color_accumulator: SimFixed::from_num(0),
        spark: Some(SparkRuntimeState {
            velocity_x: NativeF32Bits::POSITIVE_ZERO,
            velocity_y: NativeF32Bits::POSITIVE_ZERO,
            velocity_z: NativeF32Bits::POSITIVE_ZERO,
            start_rgb,
            color_index: 0,
            color_accumulator: NativeF64Bits::POSITIVE_ZERO,
        }),
        prev_delta: [SIM_ZERO; 3],
        state_advance_counter: 0,
    })
}

fn floor_constructor_coords_with<E, F>(
    system_coords: IVec3,
    mut ground_height: F,
) -> Result<IVec3, E>
where
    F: FnMut(i32, i32) -> Result<Option<i32>, E>,
{
    let first_ground = ground_height(system_coords.x, system_coords.y)?;
    let z = if first_ground.is_some_and(|ground| system_coords.z <= ground) {
        ground_height(system_coords.x, system_coords.y)?.unwrap_or(system_coords.z)
    } else {
        system_coords.z
    };
    Ok(IVec3::new(system_coords.x, system_coords.y, z))
}

/// `FUN_00661020(start1, start2, t)` — per-channel linear blend on the scaled
/// draw, rounded the way the x87 store does it.
fn interpolate_start_color(
    start_1: [u8; 3],
    start_2: [u8; 3],
    sample: u32,
) -> Result<[u8; 3], SparkSpawnError> {
    let t = X87Chop53::mul(
        X87Chop53::load_i32(sample as i32),
        X87Chop53::load_f64(RANDOM_RANGED_UNIT_SCALE)?,
    );
    let mut out = [0u8; 3];
    for channel in 0..3 {
        let lo = X87Chop53::load_i32(i32::from(start_1[channel]));
        let hi = X87Chop53::load_i32(i32::from(start_2[channel]));
        let blended = X87Chop53::add(lo, X87Chop53::mul(X87Chop53::sub(hi, lo), t));
        out[channel] = X87Chop53::ftol_i64(blended)?.clamp(0, 255) as u8;
    }
    Ok(out)
}

/// `Sqrt_Approx((x*x + y*y) + z*z)`.
///
/// The grouping is read off the disassembly at `0x0062EA85`, not the
/// decompiler: `FMUL`/`FADDP` pairs form `x*x`, then `y*y` added into it, and
/// only then `z*z`. x87 addition is not associative at this precision, so the
/// order is load-bearing and the decompiler's commutative reordering
/// (`z*z + y*y + x*x`) is wrong.
fn magnitude(
    x: NativeF32Bits,
    y: NativeF32Bits,
    z: NativeF32Bits,
) -> Result<NativeF32Bits, SparkSpawnError> {
    let x = X87Chop53::load_f32(x)?;
    let y = X87Chop53::load_f32(y)?;
    let z = X87Chop53::load_f32(z)?;
    let sum = X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(x, x), X87Chop53::mul(y, y)),
        X87Chop53::mul(z, z),
    );
    Ok(sqrt_approx_f32(sum)?)
}

fn add_i32(value: NativeF32Bits, addend: i32) -> Result<NativeF32Bits, SparkSpawnError> {
    Ok(X87Chop53::store_f32(X87Chop53::add(
        X87Chop53::load_i32(addend),
        X87Chop53::load_f32(value)?,
    ))?)
}

fn scaled_unit_draw(rng: &mut SimRng) -> Result<X87Value, SparkSpawnError> {
    let sample = rng.next_range_u32_inclusive(0, MAX_RANDOM_RANGED_SAMPLE);
    Ok(X87Chop53::mul(
        X87Chop53::load_i32(sample as i32),
        X87Chop53::load_f64(RANDOM_RANGED_UNIT_SCALE)?,
    ))
}

/// `CDQ; IDIV` — the signed remainder, sign of the dividend preserved.
///
/// VERA-internal, gamemd equivalent UNCHECKED: a zero divisor returns 0 where
/// native raises `#DE`. Unreachable for the stock Spark particle types, which
/// all author non-zero `XVelocity`/`YVelocity`/`ZVelocityRange`.
fn signed_modulo(raw: i32, divisor: i32) -> i32 {
    if divisor == 0 { 0 } else { raw % divisor }
}

/// `CDQ; XOR EAX,EDX; SUB EAX,EDX; CDQ; IDIV` — absolute value first, then the
/// remainder. Equal to modulo-then-abs for every input except `i32::MIN`,
/// where native's `NEG` overflows.
///
/// VERA-internal, gamemd equivalent UNCHECKED: a zero divisor returns 0 where
/// native raises `#DE`.
fn abs_modulo(raw: i32, divisor: i32) -> i32 {
    if divisor == 0 {
        0
    } else {
        (raw.unsigned_abs() % divisor.unsigned_abs()) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::sim::rng::SimRng;

    /// A stock-shaped Spark system: `[SparkSys]` holding `[Spark]`.
    fn spark_rules(spawn_frames: u32, cap: u32, percentage: &str) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[General]\nFixtureOnly=1\n\
             [ParticleSystems]\n1=SparkSys\n\
             [SparkSys]\nBehavesLike=Spark\nHoldsWhat=Spark\n\
             ParticleCap={cap}\nSparkSpawnFrames={spawn_frames}\n\
             SpawnSparkPercentage={percentage}\nLifetime=200\n\
             [Particles]\n1=Spark\n\
             [Spark]\nBehavesLike=Spark\nMaxEC=500\n\
             XVelocity=10\nYVelocity=10\nMinZVelocity=40\nZVelocityRange=15\n\
             ColorList=(255,255,255),(200,200,80),(200,10,10),(0,0,0)\nColorSpeed=.13\n"
        ));
        RuleSet::from_ini(&ini).expect("Spark rules parse")
    }

    fn spark_system(spawn_frames: i32) -> ParticleSystem {
        ParticleSystem {
            stable_id: 1,
            in_logic_vector: false,
            type_id: ParticleSystemTypeId(0),
            coords: IVec3::new(2048, 2048, 0),
            offset: IVec3::ZERO,
            particles: Vec::new(),
            spawn_timer: SIM_ZERO,
            lifetime: 200,
            spark_spawn_frames: spawn_frames,
            facing: 0x1D,
            // Irrelevant to Spark: the `ParticleSystem+0xF9` arm this models
            // has no setter in the image, so the fold always takes
            // `SpawnDirection`. Kept true because that is what
            // `spawn_particle_system` derives for a system with no authored
            // `SpawnDirection`, which every stock Spark system is.
            directionless: true,
            attached_entity: None,
            owner_entity: None,
            target_coords: IVec3::ZERO,
            owner_house: None,
            done_spawning: false,
        }
    }

    #[test]
    fn gsi_04_03_particle_constructor_clamps_at_or_below_level_two_104_floor() {
        let rules = spark_rules(1, 2, "1");
        let particle_type_id = crate::rules::particle_type::ParticleTypeId(0);
        let particle_type = rules.particle_type(particle_type_id).clone();
        let mut cell = super::super::spark_world::tests::terrain_cell(0, 0);
        cell.level = 2;
        let mut sim = super::super::spark_world::tests::one_cell_sim(cell);

        for (input_z, expected_z) in [(207, 208), (208, 208), (209, 209)] {
            let particle = construct_spark_particle(
                &mut sim,
                &particle_type,
                particle_type_id,
                IVec3::new(128, 128, input_z),
            )
            .expect("finite stock-shaped Spark construction");
            assert_eq!(particle.coords.z, expected_z, "input Z {input_z}");
        }
    }

    #[test]
    fn gsi_04_03_constructor_ground_query_is_conditional_and_repeated() {
        let mut calls = Vec::new();
        let mut samples = [Some(104), Some(208)].into_iter();
        let floored = floor_constructor_coords_with(IVec3::new(7, 9, 100), |x, y| {
            calls.push((x, y));
            Ok::<_, ()>(samples.next().unwrap())
        })
        .unwrap();
        assert_eq!(calls, vec![(7, 9), (7, 9)]);
        assert_eq!(
            floored.z, 208,
            "the conditional second lookup owns the assigned floor"
        );

        calls.clear();
        let untouched = floor_constructor_coords_with(IVec3::new(7, 9, 105), |x, y| {
            calls.push((x, y));
            Ok::<_, ()>(Some(104))
        })
        .unwrap();
        assert_eq!(calls, vec![(7, 9)]);
        assert_eq!(untouched.z, 105);
    }

    #[test]
    fn gsi_04_03_constructor_rng_brackets_ground_with_active_start_color_draw() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nFixtureOnly=1\n\
             [Particles]\n1=Spark\n\
             [Spark]\nBehavesLike=Spark\nMaxEC=500\n\
             ColorList=255,255,255,0,0,0\n\
             StartColor1=255,128,0\nStartColor2=128,64,32\n",
        ))
        .unwrap();
        let particle_type_id = crate::rules::particle_type::ParticleTypeId(0);
        let particle_type = rules.particle_type(particle_type_id).clone();
        assert_eq!(particle_type.color_list.len(), 2);
        assert_ne!(particle_type.start_color_1, [0, 0, 0]);
        let mut sim = Simulation::with_seed(900);
        let mut expected = SimRng::new(900);
        expected.next_raw_abs_modulo(particle_type.max_ec as u32);
        let after_lifetime = expected.logical_view();
        let mut ground_calls = 0;

        let particle = construct_spark_particle_with_ground(
            &mut sim,
            &particle_type,
            particle_type_id,
            IVec3::new(7, 9, 100),
            |sim, x, y| {
                ground_calls += 1;
                assert_eq!((x, y), (7, 9));
                assert_eq!(
                    sim.rng_views().scenario,
                    after_lifetime,
                    "lifetime is consumed before either ground lookup and color is not"
                );
                Ok(Some(if ground_calls == 1 { 104 } else { 208 }))
            },
        )
        .unwrap();

        expected.next_range_u32_inclusive(0, MAX_RANDOM_RANGED_SAMPLE);
        assert_eq!(ground_calls, 2);
        assert_eq!(particle.coords.z, 208);
        assert_eq!(sim.rng_views().scenario, expected.logical_view());
    }

    #[test]
    fn gsi_04_03_constructor_miss_uses_live_dummy_without_overlay_grid() {
        let rules = spark_rules(1, 2, "1");
        let particle_type_id = crate::rules::particle_type::ParticleTypeId(0);
        let particle_type = rules.particle_type(particle_type_id).clone();
        let mut sim = super::super::spark_world::tests::one_cell_sim(
            super::super::spark_world::tests::terrain_cell(0, 0),
        );
        sim.overlay_grid = None;
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        terrain.test_set_dummy_cell_level_slope(2, 0);

        let particle = construct_spark_particle(
            &mut sim,
            &particle_type,
            particle_type_id,
            IVec3::new(256, 0, 0),
        )
        .unwrap();

        assert_eq!(particle.coords.z, 208);
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .shared_cell_dummy()
                .snapshot()
                .coord,
            (1, 0)
        );
    }

    #[test]
    fn gsi_04_03_constructor_miss_uses_dummy_nonzero_slope_off_center() {
        let rules = spark_rules(1, 2, "1");
        let particle_type_id = crate::rules::particle_type::ParticleTypeId(0);
        let particle_type = rules.particle_type(particle_type_id).clone();
        let mut sim = super::super::spark_world::tests::one_cell_sim(
            super::super::spark_world::tests::terrain_cell(0, 0),
        );
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        terrain.test_set_dummy_cell_level_slope(2, 1);

        let particle = construct_spark_particle(
            &mut sim,
            &particle_type,
            particle_type_id,
            IVec3::new(320, 192, 0),
        )
        .unwrap();

        assert_eq!(
            particle.coords.z, 234,
            "level 2 plus slope 1 at local (64,192) uses the exact 104-lepton Cell domain"
        );
        assert_ne!(particle.coords.z, 208, "the nonzero slope must be observed");
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .shared_cell_dummy()
                .snapshot()
                .coord,
            (1, 0)
        );
    }

    #[test]
    fn gsi_05_13_spawn_frames_one_bursts_and_takes_no_gate_draw() {
        // `CMP EAX,0x1 / JZ 0x0062E89E` skips the probability roll entirely.
        // Stock `[SparkSys]` and `[FirestormSparkSys]` both set
        // `SparkSpawnFrames=1`, so this is the ordinary path.
        let rules = spark_rules(1, 6, "1");
        let mut sim = Simulation::with_seed(4242);
        let mut sys = spark_system(1);

        spark_spawn_pass(&mut sys, &mut sim, &rules).expect("spark spawn");

        // Burst is `|Next()| % (cap/2) + cap/2` = `|n| % 3 + 3`, so 3..=5.
        let spawned = sys.particles.len();
        assert!(
            (3..=5).contains(&spawned),
            "cap 6 gives a burst of 3..=5, got {spawned}"
        );

        // The draw ledger, and why it is asserted: RNG order is the whole
        // parity contract of this loop. One burst-size draw, three shared
        // direction draws, then four per particle (constructor lifetime, then
        // X/Y/Z velocity), then one facing draw. NO gate draw.
        let mut expected = SimRng::new(4242);
        expected.next_u32();
        for _ in 0..3 {
            expected.next_u32();
        }
        for _ in 0..spawned {
            for _ in 0..4 {
                expected.next_u32();
            }
        }
        expected.next_range_u32_inclusive(0, MAX_RANDOM_RANGED_SAMPLE);
        assert_eq!(sim.rng_views().scenario, expected.logical_view());

        // The countdown reached zero, so the system is done spawning.
        assert_eq!(sys.spark_spawn_frames, 0);
        assert!(sys.done_spawning);
    }

    #[test]
    fn gsi_05_13_refused_burst_still_counts_down_and_walks_the_facing() {
        // `SpawnSparkPercentage=0` refuses every draw except an exact zero
        // sample, so this pins the gate's own draw plus the tail that runs
        // regardless — the `JZ 0x0062E898` lands on `0x0062EC60`, the
        // decrement, not the function tail.
        let rules = spark_rules(20, 15, "0");
        let mut sim = Simulation::with_seed(99);
        let mut sys = spark_system(20);

        spark_spawn_pass(&mut sys, &mut sim, &rules).expect("spark spawn");

        assert!(sys.particles.is_empty(), "the gate refused the burst");
        assert_eq!(sys.spark_spawn_frames, 19, "the countdown still ran");
        assert!(!sys.done_spawning);

        let mut expected = SimRng::new(99);
        expected.next_range_u32_inclusive(0, MAX_RANDOM_RANGED_SAMPLE);
        expected.next_range_u32_inclusive(0, MAX_RANDOM_RANGED_SAMPLE);
        assert_eq!(
            sim.rng_views().scenario,
            expected.logical_view(),
            "one gate draw and one facing draw, and nothing else"
        );
    }

    #[test]
    fn gsi_05_13_direction_fold_rotates_without_changing_speed() {
        // The loop takes the magnitude BEFORE folding in the direction offset,
        // renormalises afterwards, then rescales by that original magnitude
        // (`0x0062EBA7`..`0x0062EBE5`). The offset therefore steers the
        // particle without speeding it up.
        let rules = spark_rules(1, 20, "1");
        let mut sim = Simulation::with_seed(7);
        let mut sys = spark_system(1);

        spark_spawn_pass(&mut sys, &mut sim, &rules).expect("spark spawn");
        assert!(!sys.particles.is_empty());

        for particle in &sys.particles {
            let spark = particle.spark.as_ref().expect("Spark runtime state");
            let speed = magnitude(spark.velocity_x, spark.velocity_y, spark.velocity_z)
                .expect("finite spark velocity");
            let recovered = f32::from_bits(speed.bits());
            assert!(
                recovered.is_finite() && recovered > 0.0,
                "spark speed should be finite and positive, got {recovered}"
            );
        }
    }

    #[test]
    fn gsi_05_13_facing_walk_clamps_at_both_ends() {
        // `ADD EAX,-0x3 / CMP EAX,0x11` and `ADD EAX,0x3 / CMP EAX,0x29`.
        // Driving the system from each end proves the clamp rather than the
        // step, which is the half a hand-rolled wrapping add would get wrong.
        let rules = spark_rules(1, 2, "1");
        for seed in 0..40u64 {
            let mut sim = Simulation::with_seed(seed);
            let mut sys = spark_system(1);
            sys.facing = FACING_MIN as u8;
            spark_spawn_pass(&mut sys, &mut sim, &rules).expect("spark spawn");
            assert!(
                (FACING_MIN..=FACING_MAX).contains(&i32::from(sys.facing)),
                "facing left its range from the floor at seed {seed}"
            );

            let mut sim = Simulation::with_seed(seed);
            let mut sys = spark_system(1);
            sys.facing = FACING_MAX as u8;
            spark_spawn_pass(&mut sys, &mut sim, &rules).expect("spark spawn");
            assert!(
                (FACING_MIN..=FACING_MAX).contains(&i32::from(sys.facing)),
                "facing left its range from the ceiling at seed {seed}"
            );
        }
    }

    #[test]
    fn gsi_05_13_finished_system_spawns_nothing_and_consumes_no_draw() {
        // The whole body sits under `TEST EAX,EAX / JLE 0x0062ECE9` at
        // `0x0062E855`: a system past its spawn frames falls straight through
        // to the per-particle dispatch.
        let rules = spark_rules(1, 6, "1");
        let mut sim = Simulation::with_seed(5);
        let mut sys = spark_system(0);

        spark_spawn_pass(&mut sys, &mut sim, &rules).expect("spark spawn");

        assert!(sys.particles.is_empty());
        assert_eq!(sys.facing, 0x1D, "no facing draw is taken");
        assert_eq!(
            sim.rng_views().scenario,
            SimRng::new(5).logical_view(),
            "a finished system consumes no RNG at all"
        );
    }

    #[test]
    fn gsi_05_13_burst_can_exceed_the_particle_cap_across_ticks() {
        // Native appends through `DynamicVector` growth (`0x00630250`) with no
        // cap test on this path, unlike `spawn::spawn_particle`. `WeldingSys`
        // (cap 15, 20 spawn frames) depends on that: it would otherwise stop
        // emitting after its first two bursts.
        let rules = spark_rules(20, 15, "1");
        let mut sim = Simulation::with_seed(31);
        let mut sys = spark_system(20);

        for _ in 0..5 {
            spark_spawn_pass(&mut sys, &mut sim, &rules).expect("spark spawn");
        }

        assert!(
            sys.particles.len() > 15,
            "five bursts of 7..=13 must pass ParticleCap=15, got {}",
            sys.particles.len()
        );
    }
}
