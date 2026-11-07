//! Behavior-4 Railgun particles: the spiral `ParticleSystemClass::AI_Railgun
//! @ 0x0062F230` lays once along the shot, and the drift
//! `ParticleClass::AI_Railgun @ 0x0062C3A0` gives each particle every frame.
//!
//! The system is created by `TechnoClass::FireAt` (`0x006FF1FB..0x006FF26E`)
//! from the coordinate `combat::railgun` returns, and stays linked to the
//! firer as `TechnoClass+0x314` until its last particle expires.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, util/ and the rest of sim/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use glam::{IVec3, Mat4, Vec3};
use thiserror::Error;

use super::{Particle, ParticleSystem};
use crate::rules::particle_type::ParticleType;
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::native_random::RANDOM_RANGED_UNIT_SCALE;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, NativeX87Error, X87Chop53 as Fpu, X87Ordering, X87Value,
    sqrt_approx_length,
};
use crate::util::retail_trig::{AcosTable, TrigTable};

/// `RandomRanged(0, 0x7ffffffe)`, the draw every perturbation here takes.
const MAX_SAMPLE: u32 = 0x7fff_fffe;
/// `[0x007E3860]`: the 0.1 the particle speed jitter is scaled by.
const SPEED_JITTER_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);
/// `PUSH 0xa` at `0x0062F8AD`: the laser's duration.
const LASER_DURATION: i32 = 10;
/// `PUSH 0x3f000000` at `0x0062F8A4`: the laser's starting intensity.
const LASER_FADE_START: NativeF32Bits = NativeF32Bits::from_bits(0x3f00_0000);

#[derive(Debug, Error)]
pub(crate) enum RailgunParticleError {
    #[error(transparent)]
    NativeX87(#[from] NativeX87Error),
    #[error(transparent)]
    World(#[from] super::spark_world::SparkWorldError),
    #[error("Railgun spiral point left the finite affine domain")]
    NonFiniteRing,
}

/// `ParticleSystemClass::AI_Railgun @ 0x0062F230`: while the system is not
/// done and holds no particles, lay the spiral and the laser and mark it done;
/// then dispatch every particle and run the backward destroy walk.
pub(super) fn tick_system(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    if !sys.done_spawning && sys.particles.is_empty() {
        if let Err(error) = lay_spiral(sys, sim, rules) {
            log::warn!(
                "particles: Railgun system {} spiral failed: {error}",
                sys.stable_id
            );
        }
        let pst = rules.particle_system_type(sys.type_id);
        if pst.laser {
            // `0x0062F890..0x0062F940`: LaserDraw from Location to the target
            // coordinate, width 1, `LaserColor` inside and black outside,
            // fading from 0.5.
            let point = |coord: IVec3| {
                crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z)
            };
            let birth = crate::sim::combat::laser::construct(
                &sim.session,
                &mut sim.lifecycle_outputs,
                point(sys.coords),
                point(sys.target_coords),
                0,
                LASER_DURATION,
                crate::sim::combat::laser::LaserColor::Explicit {
                    inner: pst.laser_color,
                    outer: [0; 3],
                    spread: [0; 3],
                },
                NativeF32Bits::POSITIVE_ZERO,
            );
            birth.fade_start = LASER_FADE_START;
        }
        sys.mark_done_spawning();
    }
    if let Err(error) = super::system_ai::tick_spark_system_compat(sys, sim, rules) {
        log::warn!(
            "particles: Railgun system {} particle pass failed: {error}",
            sys.stable_id
        );
    }
}

/// The spawn half of `AI_Railgun` (`0x0062F25E..0x0062F87C`).
fn lay_spiral(
    sys: &mut ParticleSystem,
    sim: &mut Simulation,
    rules: &RuleSet,
) -> Result<(), RailgunParticleError> {
    let pst = rules.particle_system_type(sys.type_id);
    let location = sys.coords;
    let target = sys.target_coords;
    let delta = [
        target.x.wrapping_sub(location.x),
        target.y.wrapping_sub(location.y),
        target.z.wrapping_sub(location.z),
    ];
    // 62F295..62F2F1: the 3D and horizontal lengths through Sqrt_Approx and
    // ftol; the horizontal sum is dy² + dx², the same value.
    let length = sqrt_approx_length(delta);
    let horizontal = sqrt_approx_length([delta[0], delta[1]]);
    let count = Fpu::ftol_i32_low_masked(Fpu::mul(
        Fpu::load_i32(length),
        Fpu::load_f64(pst.particles_per_coord)?,
    ));
    if count <= 0 {
        // The angles below are pure, so skipping them changes nothing.
        return Ok(());
    }
    // `HoldsWhat=` absent leaves a NULL particle that native dereferences
    // (`0x0062F5F6`, then `0x0062F66B`); VERA lays nothing instead.
    let Some(particle_type_id) = pst.holds_what else {
        return Ok(());
    };
    let particle_type = rules.particle_type(particle_type_id);

    // 62F2FE..62F32A: clamp dz to ±length and dx to ±horizontal.
    let clamp = |value: i32, bound: i32| {
        let upper = if value < bound { value } else { bound };
        if upper > bound.wrapping_neg() {
            upper
        } else {
            bound.wrapping_neg()
        }
    };
    let dz = clamp(delta[2], length);
    let dx = clamp(delta[0], horizontal);
    let length_f32 = Fpu::store_f32(Fpu::load_i32(length))?;
    // 62F354..62F38D: elevation = asin(dz / length), azimuth = acos(dx /
    // horizontal), negated when dy < 0. A vertical shot divides 0 by 0.
    let acos = AcosTable::retail();
    let (_, elevation) = acos.asin_lookup(Fpu::div(Fpu::load_i32(dz), Fpu::load_f32(length_f32)?)?);
    let elevation = Fpu::store_f32(elevation)?;
    let azimuth = if horizontal == 0 {
        acos.acos_lookup_nan()
    } else {
        acos.acos_lookup(Fpu::div(Fpu::load_i32(dx), Fpu::load_i32(horizontal))?)
            .1
    };
    let mut azimuth = Fpu::store_f32(azimuth)?;
    if delta[1] < 0 {
        azimuth = NativeF32Bits::from_bits(azimuth.bits() ^ 0x8000_0000);
    }
    // 62F38D..62F3BB: Identity, RotateZ(azimuth), RotateX(elevation). Each
    // rotation reads the table sine and cosine of its float angle.
    let trig = TrigTable::embedded();
    let sin_cos = |angle: NativeF32Bits| -> Result<(f32, f32), NativeX87Error> {
        let angle = Fpu::load_f32(angle)?;
        Ok((
            to_f32(Fpu::store_f32(trig.sin_from_table(angle))?),
            to_f32(Fpu::store_f32(trig.cos_from_table(angle))?),
        ))
    };
    let (sin, cos) = sin_cos(azimuth)?;
    let matrix = crate::util::native_affine::rotate_z(Mat4::IDENTITY, sin, cos);
    let (sin, cos) = sin_cos(elevation)?;
    let matrix = crate::util::native_affine::rotate_x(matrix, sin, cos);

    let count_f32 = Fpu::store_f32(Fpu::load_i32(count))?;
    let scale = Fpu::load_f64(RANDOM_RANGED_UNIT_SCALE)?;
    let half = Fpu::load_f64(NativeF64Bits::HALF)?;
    let perturbation =
        |sample: u32, coefficient: NativeF64Bits| -> Result<X87Value, NativeX87Error> {
            Ok(Fpu::mul(
                Fpu::sub(Fpu::mul(Fpu::load_i32(sample as i32), scale), half),
                Fpu::load_f64(coefficient)?,
            ))
        };
    let mut previous_speed = NativeF32Bits::POSITIVE_ZERO;
    for index in 0..count {
        // 62F3DB..62F443: t = i / count, the spiral angle
        // length * t * SpiralDeltaPerCoord, and the unit circle point
        // (0, cos, sin) turned onto the shot.
        let t = Fpu::store_f32(Fpu::div(Fpu::load_i32(index), Fpu::load_f32(count_f32)?)?)?;
        let theta = Fpu::store_f64(Fpu::mul(
            Fpu::mul(Fpu::load_f32(length_f32)?, Fpu::load_f32(t)?),
            Fpu::load_f64(pst.spiral_delta_per_coord)?,
        ))?;
        let theta = Fpu::load_f64(theta)?;
        let sin = Fpu::store_f32(trig.sin_from_table(theta))?;
        let cos = Fpu::store_f32(trig.cos_from_table(theta))?;
        let ring = crate::util::native_affine::transform_point(
            matrix,
            Vec3::new(0.0, to_f32(cos), to_f32(sin)),
        )
        .ok_or(RailgunParticleError::NonFiniteRing)?;
        let ring = [ring.x, ring.y, ring.z].map(from_f32);
        // 62F469..62F4AF: the spiral offset, ring * SpiralRadius.
        let radius = Fpu::load_f64(pst.spiral_radius)?;
        let offset = [
            Fpu::store_f32(Fpu::mul(Fpu::load_f32(ring[0])?, radius))?,
            Fpu::store_f32(Fpu::mul(Fpu::load_f32(ring[1])?, radius))?,
            Fpu::store_f32(Fpu::mul(Fpu::load_f32(ring[2])?, radius))?,
        ];
        // 62F4B3..62F5A4: three position draws; the third perturbs X on the
        // x87 stack, the second Y and the first Z through float stores.
        let rng = sim.particle_rng();
        let first = rng.next_range_u32_inclusive(0, MAX_SAMPLE);
        let second = rng.next_range_u32_inclusive(0, MAX_SAMPLE);
        let third = rng.next_range_u32_inclusive(0, MAX_SAMPLE);
        let coefficient = pst.position_perturbation_coefficient;
        let offset_x = Fpu::ftol_i32_low_masked(Fpu::add(
            perturbation(third, coefficient)?,
            Fpu::load_f32(offset[0])?,
        ));
        let offset_y = Fpu::store_f32(Fpu::add(
            Fpu::load_f32(Fpu::store_f32(perturbation(second, coefficient)?)?)?,
            Fpu::load_f32(offset[1])?,
        ))?;
        let offset_z = Fpu::store_f32(Fpu::add(
            Fpu::load_f32(Fpu::store_f32(perturbation(first, coefficient)?)?)?,
            Fpu::load_f32(offset[2])?,
        ))?;
        let offset_y = Fpu::ftol_i32_low_masked(Fpu::load_f32(offset_y)?);
        let offset_z = Fpu::ftol_i32_low_masked(Fpu::load_f32(offset_z)?);
        // 62F5A9..62F5E6: Lerp(Location, target, t) plus the offset.
        let along = crate::util::native_coord::interpolate(
            location.to_array(),
            target.to_array(),
            Fpu::load_f32(t)?,
        );
        let point = IVec3::new(
            along[0].wrapping_add(offset_x),
            along[1].wrapping_add(offset_y),
            along[2].wrapping_add(offset_z),
        );

        // 62F5FA..62F654: the constructor, both coordinates the point.
        let mut particle = Particle::new_with_ground(
            particle_type_id,
            point,
            point,
            particle_type,
            sim,
            |sim, x, y| super::spark_world::constructor_ground_height(sim, x, y),
        )?;

        // 62F658..62F7AF: the drift direction is the ring vector plus three
        // movement draws (the third on X), normalized when non-zero.
        let rng = sim.particle_rng();
        let first = rng.next_range_u32_inclusive(0, MAX_SAMPLE);
        let second = rng.next_range_u32_inclusive(0, MAX_SAMPLE);
        let third = rng.next_range_u32_inclusive(0, MAX_SAMPLE);
        let coefficient = pst.movement_perturbation_coefficient;
        let mut direction = [
            Fpu::store_f32(Fpu::add(
                perturbation(third, coefficient)?,
                Fpu::load_f32(ring[0])?,
            ))?,
            Fpu::store_f32(Fpu::add(
                Fpu::load_f32(Fpu::store_f32(perturbation(second, coefficient)?)?)?,
                Fpu::load_f32(ring[1])?,
            ))?,
            Fpu::store_f32(Fpu::add(
                Fpu::load_f32(Fpu::store_f32(perturbation(first, coefficient)?)?)?,
                Fpu::load_f32(ring[2])?,
            ))?,
        ];
        let magnitude = super::spark_spawn::magnitude(direction[0], direction[1], direction[2])?;
        let magnitude = Fpu::load_f32(magnitude)?;
        if Fpu::compare(magnitude, Fpu::load_f64(NativeF64Bits::POSITIVE_ZERO)?)
            != X87Ordering::Equal
        {
            for component in &mut direction {
                *component = Fpu::store_f32(Fpu::div(Fpu::load_f32(*component)?, magnitude)?)?;
            }
        }

        // 62F7B7..62F870: the speed draw, centred on the previous particle's,
        // capped at VelocityPerturbationCoefficient and floored at minus
        // MovementPerturbationCoefficient (native reads +0x2E8 for the floor).
        let sample = sim.particle_rng().next_range_u32_inclusive(0, MAX_SAMPLE);
        let velocity_limit = Fpu::load_f64(pst.velocity_perturbation_coefficient)?;
        let wobble = Fpu::mul(
            Fpu::sub(
                Fpu::add(
                    Fpu::mul(Fpu::load_i32(sample as i32), scale),
                    Fpu::load_f32(previous_speed)?,
                ),
                half,
            ),
            Fpu::mul(velocity_limit, half),
        );
        let capped = if Fpu::compare(velocity_limit, wobble) == X87Ordering::Greater {
            wobble
        } else {
            velocity_limit
        };
        let floor = Fpu::load_f64(Fpu::store_f64(Fpu::neg(Fpu::load_f64(
            pst.movement_perturbation_coefficient,
        )?))?)?;
        let speed = if Fpu::compare(capped, floor) == X87Ordering::Greater {
            capped
        } else {
            floor
        };
        previous_speed = Fpu::store_f32(speed)?;
        particle.velocity = Fpu::store_f32(Fpu::add(
            Fpu::load_f32(previous_speed)?,
            Fpu::load_f32(particle_type.velocity)?,
        ))?;

        particle.direction = direction;
        Simulation::reveal_particle(&mut sim.substrate, particle.stable_id());
        sys.particles.push(particle);
    }
    Ok(())
}

/// `ParticleClass::AI_Railgun @ 0x0062C3A0`: move along the direction at the
/// old speed, jitter the speed, write the truncated position as Location, then
/// step the colour. The dispatcher's lifetime tail follows.
pub(super) fn tick_particle(
    particle: &mut Particle,
    particle_type: &ParticleType,
    rng: &mut SimRng,
) -> Result<(), RailgunParticleError> {
    let speed = Fpu::load_f32(particle.velocity)?;
    let mut step = [NativeF32Bits::POSITIVE_ZERO; 3];
    for (axis, direction) in particle.direction.into_iter().enumerate() {
        step[axis] = Fpu::store_f32(Fpu::mul(Fpu::load_f32(direction)?, speed))?;
    }
    let sample = rng.next_range_u32_inclusive(0, MAX_SAMPLE);
    let jitter = Fpu::mul(
        Fpu::sub(
            Fpu::mul(
                Fpu::load_i32(sample as i32),
                Fpu::load_f64(RANDOM_RANGED_UNIT_SCALE)?,
            ),
            Fpu::load_f64(NativeF64Bits::HALF)?,
        ),
        Fpu::load_f64(SPEED_JITTER_SCALE)?,
    );
    particle.velocity = Fpu::store_f32(Fpu::add(jitter, speed))?;
    for axis in 0..3 {
        particle.float_position[axis] = Fpu::store_f32(Fpu::add(
            Fpu::load_f32(step[axis])?,
            Fpu::load_f32(particle.float_position[axis])?,
        ))?;
    }
    // vt+0x1B4 at 0x0062C48E is a plain Location write for a particle.
    let mut location = [0; 3];
    for (axis, value) in particle.float_position.into_iter().enumerate() {
        location[axis] = Fpu::ftol_i32_low_masked(Fpu::load_f32(value)?);
    }
    particle.coords = IVec3::from_array(location);
    let color_count = i32::try_from(particle_type.color_list.len()).unwrap_or(i32::MAX);
    let sample = rng.next_range_u32_inclusive(0, MAX_SAMPLE);
    super::spark::advance_point_color(
        &mut particle.color_index,
        &mut particle.color_accumulator,
        particle_type.color_speed,
        sample,
        color_count,
    )?;
    Ok(())
}

fn to_f32(bits: NativeF32Bits) -> f32 {
    f32::from_bits(bits.bits())
}

fn from_f32(value: f32) -> NativeF32Bits {
    NativeF32Bits::from_bits(value.to_bits())
}
