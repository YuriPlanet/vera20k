//! Behavior-3 Spark arithmetic, collision, and native-ordered tick kernel.
//!
//! Production callers gather native-ordered live-world collision facts before
//! advancing the authoritative particle RNG.

use glam::IVec3;
use thiserror::Error;

use super::{Particle, SparkRuntimeState};
use crate::sim::rng::SimRng;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, NativeX87Error, X87Chop53, X87Ordering, X87Value,
};

// Spark collision's verified structural-bridge role; keep independently named
// (same retail value as `sim::map::bridge_topology::BRIDGE_DECK_HEIGHT_LEPTONS`;
// see the separation notes there and at `util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS`).
const STRUCTURAL_BRIDGE_HEIGHT: i32 = crate::sim::map::bridge_topology::BRIDGE_DECK_HEIGHT_LEPTONS;
const ASCENDING_BRIDGE_DELETE_OFFSET: i32 = 20;
const GROUND_CLAMP_DEPTH: i32 = 100;
const BUILDING_CONTACT_HEIGHT_F32: NativeF32Bits = NativeF32Bits::from_bits(0x4316_0000);
const MAX_COLOR_RNG_SAMPLE: u32 = 0x7fff_fffe;
const COLOR_RNG_RECIPROCAL: NativeF64Bits = NativeF64Bits::from_bits(0x3e00_0000_0040_0000);
const COLOR_JITTER_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x3fa9_9999_9999_999a);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkCollisionFacts {
    pub ground_z: i32,
    pub slope_matrix: Option<[NativeF32Bits; 12]>,
    pub old_has_structural_bridge: bool,
    pub candidate_has_structural_bridge: bool,
    pub accepted_building: bool,
    pub wall_overlay_id: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SparkCollisionKind {
    DescendingBridge,
    AscendingBridge,
    BelowGroundNear,
    BelowGroundDeep,
    Building,
    Wall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkMotionStep {
    pub old_coords: IVec3,
    pub candidate_coords: IVec3,
    pub candidate_f32: [NativeF32Bits; 3],
    pub persistent_velocity: [NativeF32Bits; 3],
    pub probe_velocity: [NativeF32Bits; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkCollisionResolution {
    pub committed_coords: IVec3,
    pub kind: Option<SparkCollisionKind>,
    pub transient_reflection: Option<[NativeF32Bits; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SparkCollisionDecision {
    selected_coords_f32: [NativeF32Bits; 3],
    kind: Option<SparkCollisionKind>,
    transient_reflection: Option<[NativeF32Bits; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkTickInputs {
    pub gravity: NativeF32Bits,
    pub color_speed: NativeF64Bits,
    pub color_count: usize,
    pub collision: SparkCollisionFacts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkTickResult {
    pub motion: SparkMotionStep,
    pub collision_kind: Option<SparkCollisionKind>,
    pub transient_reflection: Option<[NativeF32Bits; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SparkKernelError {
    #[error("behavior-3 tick requires SparkRuntimeState")]
    MissingRuntimeState,
    #[error("Spark ColorList count {0} is outside the safe valid-state boundary")]
    InvalidColorCount(usize),
    #[error("Spark color RNG sample {0:#x} is outside 0..=0x7ffffffe")]
    InvalidColorRngSample(u32),
    #[error("Spark {0:?} collision requires a slope matrix")]
    MissingSlopeMatrixForCollision(SparkCollisionKind),
    #[error(transparent)]
    NativeX87(#[from] NativeX87Error),
}

/// Signed toward-zero lepton→cell — `v / 256` is bit-identical to the
/// canonical biased shift; delegated so the conversion exists once.
#[cfg(test)]
pub fn lepton_to_cell_trunc(value: i32) -> i32 {
    crate::util::direction_tables::lepton_to_cell(value)
}

/// Convert the signed `[General] Gravity=` storage to the f32 bits consumed by
/// behavior-3 arithmetic at its native boundary.
pub fn gravity_as_stored_f32(value: i32) -> Result<NativeF32Bits, SparkKernelError> {
    integer_as_stored_f32(value).map_err(Into::into)
}

fn persistent_velocity_z(
    old_velocity_z: NativeF32Bits,
    gravity: NativeF32Bits,
) -> Result<NativeF32Bits, SparkKernelError> {
    let gravity_value = X87Chop53::load_f32(gravity)?;
    let old_vz = X87Chop53::load_f32(old_velocity_z)?;
    X87Chop53::store_f32(X87Chop53::sub(old_vz, gravity_value)).map_err(Into::into)
}

fn integrate_motion_after_persistent_z(
    coords: IVec3,
    spark: SparkRuntimeState,
    gravity: NativeF32Bits,
    persistent_z: NativeF32Bits,
) -> Result<SparkMotionStep, SparkKernelError> {
    // Native first stores all three old coordinates as f32, then stores the
    // second-gravity probe. Its ftol call order is Z, Y, X.
    let old_x_f32 = integer_as_stored_f32(coords.x)?;
    let old_y_f32 = integer_as_stored_f32(coords.y)?;
    let old_z_f32 = integer_as_stored_f32(coords.z)?;

    let gravity_value = X87Chop53::load_f32(gravity)?;
    let probe_z = X87Chop53::store_f32(X87Chop53::sub(
        X87Chop53::load_f32(persistent_z)?,
        gravity_value,
    ))?;

    let old_z = ftol_f32_to_i32(old_z_f32)?;
    let old_y = ftol_f32_to_i32(old_y_f32)?;
    let old_x = ftol_f32_to_i32(old_x_f32)?;

    let probe_velocity = [spark.velocity_x, spark.velocity_y, probe_z];
    let candidate_x_f32 = add_stored_f32(old_x_f32, probe_velocity[0])?;
    let candidate_y_f32 = add_stored_f32(old_y_f32, probe_velocity[1])?;
    let candidate_z_f32 = add_stored_f32(old_z_f32, probe_velocity[2])?;

    let candidate_z = ftol_f32_to_i32(candidate_z_f32)?;
    let candidate_y = ftol_f32_to_i32(candidate_y_f32)?;
    let candidate_x = ftol_f32_to_i32(candidate_x_f32)?;

    Ok(SparkMotionStep {
        old_coords: IVec3::new(old_x, old_y, old_z),
        candidate_coords: IVec3::new(candidate_x, candidate_y, candidate_z),
        candidate_f32: [candidate_x_f32, candidate_y_f32, candidate_z_f32],
        persistent_velocity: [spark.velocity_x, spark.velocity_y, persistent_z],
        probe_velocity,
    })
}

fn add_stored_f32(
    lhs: NativeF32Bits,
    rhs: NativeF32Bits,
) -> Result<NativeF32Bits, SparkKernelError> {
    let sum = X87Chop53::add(X87Chop53::load_f32(lhs)?, X87Chop53::load_f32(rhs)?);
    X87Chop53::store_f32(sum).map_err(Into::into)
}

#[cfg(test)]
pub fn resolve_collision(
    motion: SparkMotionStep,
    facts: SparkCollisionFacts,
) -> Result<SparkCollisionResolution, SparkKernelError> {
    finalize_collision(resolve_collision_decision(motion, facts)?)
}

fn resolve_collision_decision(
    motion: SparkMotionStep,
    facts: SparkCollisionFacts,
) -> Result<SparkCollisionDecision, SparkKernelError> {
    // gamemd-derived: behavior-3 ParticleClass AI @ 0x0062C6E0 owns these
    // bridge, ground, building, wall, commit-Z, and slope-reflection decisions.
    let ground_z = facts.ground_z;
    let ground_exact = X87Chop53::load_i32(ground_z);
    let bridge_plane = ground_z.wrapping_add(STRUCTURAL_BRIDGE_HEIGHT);
    let kind = classify_collision_kind(motion, facts)?;
    let ground_stored_bits = X87Chop53::store_f32(ground_exact)?;

    let committed_z_bits = match kind {
        Some(SparkCollisionKind::DescendingBridge) => integer_as_stored_f32(bridge_plane)?,
        Some(SparkCollisionKind::AscendingBridge) => {
            integer_as_stored_f32(bridge_plane.wrapping_sub(ASCENDING_BRIDGE_DELETE_OFFSET))?
        }
        Some(SparkCollisionKind::BelowGroundNear)
        | Some(SparkCollisionKind::Building)
        | Some(SparkCollisionKind::Wall) => ground_stored_bits,
        Some(SparkCollisionKind::BelowGroundDeep) | None => motion.candidate_f32[2],
    };

    let transient_reflection = if let Some(kind) = kind {
        let slope_matrix = facts
            .slope_matrix
            .ok_or(SparkKernelError::MissingSlopeMatrixForCollision(kind))?;
        Some(reflect_slope_vector(motion.probe_velocity, slope_matrix)?)
    } else {
        None
    };

    Ok(SparkCollisionDecision {
        selected_coords_f32: [
            motion.candidate_f32[0],
            motion.candidate_f32[1],
            committed_z_bits,
        ],
        kind,
        transient_reflection,
    })
}

pub(super) fn bridge_collision_kind(
    motion: SparkMotionStep,
    ground_z: i32,
    old_has_structural_bridge: bool,
    candidate_has_structural_bridge: bool,
) -> Option<SparkCollisionKind> {
    let bridge_plane = ground_z.wrapping_add(STRUCTURAL_BRIDGE_HEIGHT);
    let structural = old_has_structural_bridge || candidate_has_structural_bridge;
    if structural && motion.candidate_coords.z < bridge_plane && motion.old_coords.z >= bridge_plane
    {
        Some(SparkCollisionKind::DescendingBridge)
    } else if structural
        && motion.candidate_coords.z >= bridge_plane
        && motion.old_coords.z < bridge_plane
    {
        Some(SparkCollisionKind::AscendingBridge)
    } else {
        None
    }
}

/// Native's raw-candidate contact-band gate, shared with the live-world query
/// owner so building and overlay state stay lazy without duplicating thresholds.
pub(super) fn in_contact_band(
    motion: SparkMotionStep,
    ground_z: i32,
) -> Result<bool, SparkKernelError> {
    let candidate_z = X87Chop53::load_f32(motion.candidate_f32[2])?;
    let ground_exact = X87Chop53::load_i32(ground_z);
    if X87Chop53::compare(candidate_z, ground_exact) == X87Ordering::Less {
        return Ok(false);
    }
    let contact_floor = X87Chop53::sub(
        candidate_z,
        X87Chop53::load_f32(BUILDING_CONTACT_HEIGHT_F32)?,
    );
    Ok(X87Chop53::compare(contact_floor, ground_exact) == X87Ordering::Less)
}

/// Collision selection without the slope transform. This is the single owner
/// used both to decide whether the world must select a slope cell and to commit
/// the final collision.
pub(super) fn classify_collision_kind(
    motion: SparkMotionStep,
    facts: SparkCollisionFacts,
) -> Result<Option<SparkCollisionKind>, SparkKernelError> {
    if let Some(kind) = bridge_collision_kind(
        motion,
        facts.ground_z,
        facts.old_has_structural_bridge,
        facts.candidate_has_structural_bridge,
    ) {
        return Ok(Some(kind));
    }

    let candidate_z = X87Chop53::load_f32(motion.candidate_f32[2])?;
    let ground_exact = X87Chop53::load_i32(facts.ground_z);
    let ground_stored = X87Chop53::load_f32(X87Chop53::store_f32(ground_exact)?)?;
    if X87Chop53::compare(candidate_z, ground_stored) == X87Ordering::Less {
        let clamp_boundary = X87Chop53::load_i32(facts.ground_z.wrapping_sub(GROUND_CLAMP_DEPTH));
        return Ok(Some(
            if X87Chop53::compare(clamp_boundary, candidate_z) == X87Ordering::Less {
                SparkCollisionKind::BelowGroundNear
            } else {
                SparkCollisionKind::BelowGroundDeep
            },
        ));
    }

    if !in_contact_band(motion, facts.ground_z)? {
        return Ok(None);
    }
    if facts.accepted_building {
        Ok(Some(SparkCollisionKind::Building))
    } else if matches!(facts.wall_overlay_id, Some(0x02) | Some(0x1a) | Some(0xf3)) {
        Ok(Some(SparkCollisionKind::Wall))
    } else {
        Ok(None)
    }
}

fn finalize_collision(
    decision: SparkCollisionDecision,
) -> Result<SparkCollisionResolution, SparkKernelError> {
    // Native performs final Math__ftol calls in Z, Y, X order before assembling
    // the coordinate passed to ObjectClass::Set_Raw_Coords.
    let z = ftol_f32_to_i32(decision.selected_coords_f32[2])?;
    let y = ftol_f32_to_i32(decision.selected_coords_f32[1])?;
    let x = ftol_f32_to_i32(decision.selected_coords_f32[0])?;
    Ok(SparkCollisionResolution {
        committed_coords: IVec3::new(x, y, z),
        kind: decision.kind,
        transient_reflection: decision.transient_reflection,
    })
}

fn commit_collision_decision(
    particle: &mut Particle,
    decision: SparkCollisionDecision,
) -> Result<SparkCollisionResolution, SparkKernelError> {
    if decision.kind.is_some() {
        particle.marked_for_deletion = true;
    }
    let collision = finalize_collision(decision)?;
    // The bound native coordinate setter writes X, then Y, then Z dwords.
    particle.coords.x = collision.committed_coords.x;
    particle.coords.y = collision.committed_coords.y;
    particle.coords.z = collision.committed_coords.z;
    Ok(collision)
}

fn integer_as_stored_f32(value: i32) -> Result<NativeF32Bits, NativeX87Error> {
    X87Chop53::store_f32(X87Chop53::load_i32(value))
}

fn ftol_f32_to_i32(value: NativeF32Bits) -> Result<i32, NativeX87Error> {
    Ok(X87Chop53::ftol_i64(X87Chop53::load_f32(value)?)? as i32)
}

pub fn reflect_slope_vector(
    probe_velocity: [NativeF32Bits; 3],
    slope_matrix: [NativeF32Bits; 12],
) -> Result<[NativeF32Bits; 3], SparkKernelError> {
    reflect_slope_vector_with_elasticity(probe_velocity, slope_matrix, NativeF32Bits::ONE)
}

/// Shared original matrix path: Spark uses scalar one; ordinary
/// Bullet AI4676FE..46771C supplies its BulletType Elasticity narrowed to f32.
pub(crate) fn reflect_slope_vector_with_elasticity(
    probe_velocity: [NativeF32Bits; 3],
    slope_matrix: [NativeF32Bits; 12],
    elasticity: NativeF32Bits,
) -> Result<[NativeF32Bits; 3], SparkKernelError> {
    let axis_probe = [
        probe_velocity[0],
        negate_f32(probe_velocity[1])?,
        probe_velocity[2],
    ];
    let inverse = inverse_orthonormal_matrix(slope_matrix)?;
    let inverse_result = matrix_vector(inverse, axis_probe)?;
    let mut local = [
        multiply_store_f32(inverse_result[0], elasticity)?,
        multiply_store_f32(inverse_result[1], elasticity)?,
        multiply_store_f32(inverse_result[2], elasticity)?,
    ];
    local[2] = negate_f32(local[2])?;
    let mut reflected = matrix_vector(slope_matrix, local)?;
    reflected[1] = negate_f32(reflected[1])?;
    Ok(reflected)
}

fn inverse_orthonormal_matrix(
    matrix: [NativeF32Bits; 12],
) -> Result<[NativeF32Bits; 12], SparkKernelError> {
    let zero = NativeF32Bits::POSITIVE_ZERO;
    let mut inverse = [zero; 12];
    inverse[0] = matrix[0];
    inverse[1] = matrix[4];
    inverse[2] = matrix[8];
    inverse[4] = matrix[1];
    inverse[5] = matrix[5];
    inverse[6] = matrix[9];
    inverse[8] = matrix[2];
    inverse[9] = matrix[6];
    inverse[10] = matrix[10];
    inverse[3] = negative_ordered_product_sum([
        (matrix[0], matrix[3]),
        (matrix[8], matrix[11]),
        (matrix[4], matrix[7]),
    ])?;
    inverse[7] = negative_ordered_product_sum([
        (matrix[1], matrix[3]),
        (matrix[9], matrix[11]),
        (matrix[5], matrix[7]),
    ])?;
    inverse[11] = negative_ordered_product_sum([
        (matrix[2], matrix[3]),
        (matrix[10], matrix[11]),
        (matrix[6], matrix[7]),
    ])?;
    Ok(inverse)
}

fn negative_ordered_product_sum(
    pairs: [(NativeF32Bits, NativeF32Bits); 3],
) -> Result<NativeF32Bits, SparkKernelError> {
    let first = multiply_value(pairs[0].0, pairs[0].1)?;
    let second = multiply_value(pairs[1].0, pairs[1].1)?;
    let third = multiply_value(pairs[2].0, pairs[2].1)?;
    let partial = X87Chop53::add(first, second);
    X87Chop53::store_f32(X87Chop53::neg(X87Chop53::add(partial, third)))
        .map_err(SparkKernelError::from)
}

fn matrix_vector(
    matrix: [NativeF32Bits; 12],
    vector: [NativeF32Bits; 3],
) -> Result<[NativeF32Bits; 3], SparkKernelError> {
    Ok([
        ordered_product_sum([
            (matrix[1], vector[1]),
            (matrix[2], vector[2]),
            (matrix[0], vector[0]),
        ])?,
        ordered_product_sum([
            (matrix[5], vector[1]),
            (matrix[4], vector[0]),
            (matrix[6], vector[2]),
        ])?,
        ordered_product_sum([
            (matrix[9], vector[1]),
            (matrix[8], vector[0]),
            (matrix[10], vector[2]),
        ])?,
    ])
}

fn ordered_product_sum(
    pairs: [(NativeF32Bits, NativeF32Bits); 3],
) -> Result<NativeF32Bits, SparkKernelError> {
    let first = multiply_value(pairs[0].0, pairs[0].1)?;
    let second = multiply_value(pairs[1].0, pairs[1].1)?;
    let third = multiply_value(pairs[2].0, pairs[2].1)?;
    let partial = X87Chop53::add(first, second);
    X87Chop53::store_f32(X87Chop53::add(partial, third)).map_err(SparkKernelError::from)
}

fn multiply_value(lhs: NativeF32Bits, rhs: NativeF32Bits) -> Result<X87Value, SparkKernelError> {
    Ok(X87Chop53::mul(
        X87Chop53::load_f32(lhs)?,
        X87Chop53::load_f32(rhs)?,
    ))
}

fn multiply_store_f32(
    lhs: NativeF32Bits,
    rhs: NativeF32Bits,
) -> Result<NativeF32Bits, SparkKernelError> {
    X87Chop53::store_f32(multiply_value(lhs, rhs)?).map_err(SparkKernelError::from)
}

fn negate_f32(value: NativeF32Bits) -> Result<NativeF32Bits, SparkKernelError> {
    X87Chop53::store_f32(X87Chop53::neg(X87Chop53::load_f32(value)?))
        .map_err(SparkKernelError::from)
}

pub fn advance_color(
    spark: &mut SparkRuntimeState,
    color_speed: NativeF64Bits,
    color_rng_sample: u32,
    color_count: usize,
) -> Result<(), SparkKernelError> {
    if color_rng_sample > MAX_COLOR_RNG_SAMPLE {
        return Err(SparkKernelError::InvalidColorRngSample(color_rng_sample));
    }
    let Ok(color_count_i32) = i32::try_from(color_count) else {
        return Err(SparkKernelError::InvalidColorCount(color_count));
    };
    if color_count_i32 < 2 {
        return Err(SparkKernelError::InvalidColorCount(color_count));
    }

    let scaled_rng = X87Chop53::mul(
        X87Chop53::load_i32(color_rng_sample as i32),
        X87Chop53::load_f64(COLOR_RNG_RECIPROCAL)?,
    );
    let jitter = X87Chop53::mul(scaled_rng, X87Chop53::load_f64(COLOR_JITTER_SCALE)?);
    let with_speed = X87Chop53::add(jitter, X87Chop53::load_f64(color_speed)?);
    let accumulated = X87Chop53::add(with_speed, X87Chop53::load_f64(spark.color_accumulator)?);
    spark.color_accumulator = X87Chop53::store_f64(accumulated)?;

    let stored = X87Chop53::load_f64(spark.color_accumulator)?;
    let one = X87Chop53::load_f64(NativeF64Bits::ONE)?;
    if X87Chop53::compare(stored, one) == X87Ordering::Greater {
        if spark.color_index < color_count_i32.wrapping_sub(2) {
            spark.color_index = spark.color_index.wrapping_add(1);
            spark.color_accumulator = NativeF64Bits::POSITIVE_ZERO;
        } else {
            spark.color_accumulator = NativeF64Bits::ONE;
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn tick_particle_with_facts(
    particle: &mut Particle,
    inputs: SparkTickInputs,
    rng: &mut SimRng,
) -> Result<SparkTickResult, SparkKernelError> {
    let motion = begin_particle_tick(particle, inputs.gravity)?;
    finish_particle_tick(
        particle,
        motion,
        inputs.collision,
        inputs.color_speed,
        inputs.color_count,
        rng,
    )
}

/// Apply the native persistent-Z write and compute the candidate probe.
///
/// The persistent velocity write intentionally happens before any live-world
/// query. A genuinely corrupt or unsupported world dependency must retain that
/// write while consuming no RNG and leaving coordinates/lifetime untouched.
///
/// gamemd-derived: behavior-3 ParticleClass AI @ 0x0062C6E0 stores persistent
/// Z velocity and computes the candidate before querying live CellClass facts.
pub fn begin_particle_tick(
    particle: &mut Particle,
    gravity: NativeF32Bits,
) -> Result<SparkMotionStep, SparkKernelError> {
    let spark = particle
        .spark
        .ok_or(SparkKernelError::MissingRuntimeState)?;
    let persistent_z = persistent_velocity_z(spark.velocity_z, gravity)?;
    particle
        .spark
        .as_mut()
        .ok_or(SparkKernelError::MissingRuntimeState)?
        .velocity_z = persistent_z;

    integrate_motion_after_persistent_z(particle.coords, spark, gravity, persistent_z)
}

/// Resolve and commit owned collision facts, then consume color RNG and lifetime
/// in the verified order.
///
/// gamemd-derived: behavior-3 ParticleClass AI @ 0x0062C6E0 commits collision
/// state before the color draw and final lifetime decrement.
pub fn finish_particle_tick(
    particle: &mut Particle,
    motion: SparkMotionStep,
    collision_facts: SparkCollisionFacts,
    color_speed: NativeF64Bits,
    color_count: usize,
    rng: &mut SimRng,
) -> Result<SparkTickResult, SparkKernelError> {
    let decision = resolve_collision_decision(motion, collision_facts)?;
    let collision = commit_collision_decision(particle, decision)?;

    let color_rng_sample = rng.next_range_u32_inclusive(0, MAX_COLOR_RNG_SAMPLE);
    advance_color(
        particle
            .spark
            .as_mut()
            .ok_or(SparkKernelError::MissingRuntimeState)?,
        color_speed,
        color_rng_sample,
        color_count,
    )?;

    let lifetime = particle.lifetime_remaining.wrapping_sub(1);
    particle.lifetime_remaining = lifetime;
    if lifetime == 0 {
        particle.marked_for_deletion = true;
    }

    Ok(SparkTickResult {
        motion,
        collision_kind: collision.kind,
        transient_reflection: collision.transient_reflection,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::particle_type::ParticleTypeId;
    use crate::util::fixed_math::SimFixed;

    const F32_ZERO: NativeF32Bits = NativeF32Bits::POSITIVE_ZERO;
    const F32_ONE: NativeF32Bits = NativeF32Bits::ONE;
    const F32_SIX: NativeF32Bits = NativeF32Bits::from_bits(0x40c0_0000);
    const F32_MAX: NativeF32Bits = NativeF32Bits::from_bits(0x7f7f_ffff);
    const F32_NAN: NativeF32Bits = NativeF32Bits::from_bits(0x7fc0_0000);
    const F64_ZERO: NativeF64Bits = NativeF64Bits::POSITIVE_ZERO;
    const F64_HALF: NativeF64Bits = NativeF64Bits::HALF;

    fn identity_matrix() -> [NativeF32Bits; 12] {
        [
            F32_ONE, F32_ZERO, F32_ZERO, F32_ZERO, F32_ZERO, F32_ONE, F32_ZERO, F32_ZERO, F32_ZERO,
            F32_ZERO, F32_ONE, F32_ZERO,
        ]
    }

    fn facts(ground_z: i32) -> SparkCollisionFacts {
        SparkCollisionFacts {
            ground_z,
            slope_matrix: Some(identity_matrix()),
            old_has_structural_bridge: false,
            candidate_has_structural_bridge: false,
            accepted_building: false,
            wall_overlay_id: None,
        }
    }

    fn spark_state(vz: NativeF32Bits) -> SparkRuntimeState {
        SparkRuntimeState {
            velocity_x: F32_ZERO,
            velocity_y: F32_ZERO,
            velocity_z: vz,
            start_rgb: [80, 255, 255],
            color_index: 0,
            color_accumulator: F64_ZERO,
        }
    }

    fn particle(coords: IVec3, vz: NativeF32Bits, lifetime: i16) -> Particle {
        Particle {
            type_id: ParticleTypeId(0),
            coords,
            previous_coords: coords,
            origin: coords,
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
            spark: Some(spark_state(vz)),
            prev_delta: [SimFixed::from_num(0); 3],
            state_advance_counter: 0,
        }
    }

    fn stored_f32(value: i32) -> NativeF32Bits {
        X87Chop53::store_f32(X87Chop53::load_i32(value)).unwrap()
    }

    fn motion_with_candidate_f32(
        old_z: i32,
        candidate_z: i32,
        candidate_z_f32: NativeF32Bits,
    ) -> SparkMotionStep {
        SparkMotionStep {
            old_coords: IVec3::new(0, 0, old_z),
            candidate_coords: IVec3::new(0, 0, candidate_z),
            candidate_f32: [F32_ZERO, F32_ZERO, candidate_z_f32],
            persistent_velocity: [F32_ZERO; 3],
            probe_velocity: [F32_ZERO; 3],
        }
    }

    fn motion(old_z: i32, candidate_z: i32) -> SparkMotionStep {
        motion_with_candidate_f32(old_z, candidate_z, stored_f32(candidate_z))
    }

    #[test]
    fn flat_ground_trace_preserves_double_gravity_and_commit_order() {
        let mut particle = particle(IVec3::new(2560, 2560, 10), F32_ZERO, 2);
        let mut rng = SimRng::new(1);
        let mut expected_rng = rng.clone();
        expected_rng.next_range_u32_inclusive(0, MAX_COLOR_RNG_SAMPLE);
        let result = tick_particle_with_facts(
            &mut particle,
            SparkTickInputs {
                gravity: F32_SIX,
                color_speed: F64_ZERO,
                color_count: 5,
                collision: facts(0),
            },
            &mut rng,
        )
        .unwrap();
        assert_eq!(result.motion.persistent_velocity[2].bits(), 0xc0c0_0000);
        assert_eq!(result.motion.probe_velocity[2].bits(), 0xc140_0000);
        assert_eq!(result.motion.candidate_coords, IVec3::new(2560, 2560, -2));
        assert_eq!(particle.coords, IVec3::new(2560, 2560, 0));
        assert_eq!(
            result.collision_kind,
            Some(SparkCollisionKind::BelowGroundNear)
        );
        assert!(particle.marked_for_deletion);
        assert_eq!(particle.lifetime_remaining, 1);
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn color_error_keeps_native_prior_commits_and_rng_but_not_lifetime() {
        let mut particle = particle(IVec3::new(2560, 2560, 10), F32_ZERO, 2);
        let mut rng = SimRng::new(1);
        let mut expected_rng = rng.clone();
        expected_rng.next_range_u32_inclusive(0, MAX_COLOR_RNG_SAMPLE);

        let result = tick_particle_with_facts(
            &mut particle,
            SparkTickInputs {
                gravity: F32_SIX,
                color_speed: F64_ZERO,
                color_count: 1,
                collision: facts(0),
            },
            &mut rng,
        );

        assert_eq!(result, Err(SparkKernelError::InvalidColorCount(1)));
        assert_eq!(particle.spark.unwrap().velocity_z.bits(), 0xc0c0_0000);
        assert_eq!(particle.coords, IVec3::new(2560, 2560, 0));
        assert!(particle.marked_for_deletion);
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
        assert_eq!(particle.lifetime_remaining, 2);
    }

    #[test]
    fn persistent_z_commit_survives_a_later_motion_error() {
        let original_coords = IVec3::new(2560, 2560, 10);
        let mut particle = particle(original_coords, F32_ZERO, 2);
        particle.spark.as_mut().unwrap().velocity_x = F32_NAN;
        let mut rng = SimRng::new(1);
        let rng_before = rng.logical_state();

        let result = tick_particle_with_facts(
            &mut particle,
            SparkTickInputs {
                gravity: F32_SIX,
                color_speed: F64_ZERO,
                color_count: 5,
                collision: facts(0),
            },
            &mut rng,
        );

        assert_eq!(
            result,
            Err(SparkKernelError::NativeX87(
                NativeX87Error::NonFiniteInput { format: "f32" }
            ))
        );
        assert_eq!(particle.spark.unwrap().velocity_z.bits(), 0xc0c0_0000);
        assert_eq!(particle.coords, original_coords);
        assert!(!particle.marked_for_deletion);
        assert_eq!(rng.logical_state(), rng_before);
        assert_eq!(particle.lifetime_remaining, 2);
    }

    #[test]
    fn collision_delete_commit_survives_final_coordinate_error() {
        let original_coords = IVec3::new(2560, 2560, 10);
        let mut particle = particle(original_coords, F32_ZERO, 2);
        let decision = SparkCollisionDecision {
            selected_coords_f32: [F32_NAN, F32_ZERO, F32_ZERO],
            kind: Some(SparkCollisionKind::BelowGroundNear),
            transient_reflection: None,
        };

        let result = commit_collision_decision(&mut particle, decision);

        assert_eq!(
            result,
            Err(SparkKernelError::NativeX87(
                NativeX87Error::NonFiniteInput { format: "f32" }
            ))
        );
        assert!(particle.marked_for_deletion);
        assert_eq!(particle.coords, original_coords);
    }

    #[test]
    fn final_coordinate_conversion_order_is_z_then_y_then_x() {
        let base = SparkCollisionDecision {
            selected_coords_f32: [F32_ZERO; 3],
            kind: None,
            transient_reflection: None,
        };

        let z_before_x = finalize_collision(SparkCollisionDecision {
            selected_coords_f32: [F32_NAN, F32_ZERO, F32_MAX],
            ..base
        });
        assert_eq!(
            z_before_x,
            Err(SparkKernelError::NativeX87(
                NativeX87Error::IntegerConversion
            ))
        );

        let y_before_x = finalize_collision(SparkCollisionDecision {
            selected_coords_f32: [F32_NAN, F32_MAX, F32_ZERO],
            ..base
        });
        assert_eq!(
            y_before_x,
            Err(SparkKernelError::NativeX87(
                NativeX87Error::IntegerConversion
            ))
        );
    }

    #[test]
    fn signed_leptons_truncate_toward_zero_at_cell_boundaries() {
        assert_eq!(lepton_to_cell_trunc(-1), 0);
        assert_eq!(lepton_to_cell_trunc(-255), 0);
        assert_eq!(lepton_to_cell_trunc(-256), -1);
        assert_eq!(lepton_to_cell_trunc(255), 0);
        assert_eq!(lepton_to_cell_trunc(256), 1);
    }

    #[test]
    fn gsi_04_03b_structural_bridge_predicates_keep_their_equality_sides() {
        let mut structural = facts(0);
        structural.old_has_structural_bridge = true;
        let descending = resolve_collision(motion(426, 414), structural).unwrap();
        assert_eq!(descending.committed_coords.z, 416);
        assert_eq!(descending.kind, Some(SparkCollisionKind::DescendingBridge));

        let ascending = resolve_collision(motion(406, 424), structural).unwrap();
        assert_eq!(ascending.committed_coords.z, 396);
        assert_eq!(ascending.kind, Some(SparkCollisionKind::AscendingBridge));

        let equality = resolve_collision(motion(426, 416), structural).unwrap();
        assert_eq!(equality.kind, None);
        assert_eq!(equality.committed_coords.z, 416);
    }

    #[test]
    fn gsi_04_03_cell_ground_104_preserves_level_two_spark_bridge_composition() {
        let mut structural = facts(208);
        structural.old_has_structural_bridge = true;

        let descending = resolve_collision(motion(634, 622), structural).unwrap();
        assert_eq!(descending.committed_coords.z, 624);
        assert_eq!(descending.kind, Some(SparkCollisionKind::DescendingBridge));

        let ascending = resolve_collision(motion(614, 632), structural).unwrap();
        assert_eq!(ascending.committed_coords.z, 604);
        assert_eq!(ascending.kind, Some(SparkCollisionKind::AscendingBridge));
    }

    #[test]
    fn ground_and_contact_height_boundaries_are_strict() {
        let near = resolve_collision(motion(0, -99), facts(0)).unwrap();
        assert_eq!(near.committed_coords.z, 0);
        assert_eq!(near.kind, Some(SparkCollisionKind::BelowGroundNear));

        let exact_deep = resolve_collision(motion(0, -100), facts(0)).unwrap();
        assert_eq!(exact_deep.committed_coords.z, -100);
        assert_eq!(exact_deep.kind, Some(SparkCollisionKind::BelowGroundDeep));

        let mut building = facts(0);
        building.accepted_building = true;
        assert_eq!(
            resolve_collision(motion(0, 149), building).unwrap().kind,
            Some(SparkCollisionKind::Building),
        );
        assert_eq!(
            resolve_collision(motion(0, 150), building).unwrap().kind,
            None
        );
    }

    #[test]
    fn gsi_04_03_slope_matrix_is_required_only_after_collision_selection() {
        let mut clear = facts(0);
        clear.slope_matrix = None;
        assert_eq!(
            resolve_collision(motion(200, 200), clear).unwrap().kind,
            None
        );

        let mut collision = facts(0);
        collision.slope_matrix = None;
        assert_eq!(
            resolve_collision(motion(0, -1), collision),
            Err(SparkKernelError::MissingSlopeMatrixForCollision(
                SparkCollisionKind::BelowGroundNear
            ))
        );
    }

    #[test]
    fn fractional_candidate_remains_below_ground_until_final_ftol() {
        let result = resolve_collision(
            motion_with_candidate_f32(0, 0, NativeF32Bits::from_bits(0xbf00_0000)),
            facts(0),
        )
        .unwrap();
        assert_eq!(result.kind, Some(SparkCollisionKind::BelowGroundNear));
        assert_eq!(result.committed_coords.z, 0);
    }

    #[test]
    fn wall_fallback_accepts_only_the_three_native_overlay_ids() {
        for overlay in [0x02, 0x1a, 0xf3] {
            let mut wall = facts(0);
            wall.wall_overlay_id = Some(overlay);
            assert_eq!(
                resolve_collision(motion(0, 100), wall).unwrap().kind,
                Some(SparkCollisionKind::Wall),
            );
        }
        for overlay in [0x01, 0x03, 0x19, 0x1b, 0xf2, 0xf4] {
            let mut wall = facts(0);
            wall.wall_overlay_id = Some(overlay);
            assert_eq!(resolve_collision(motion(0, 100), wall).unwrap().kind, None);
        }
    }

    #[test]
    fn identity_slope_reflects_probe_z_but_never_replaces_persistent_velocity() {
        let reflected = reflect_slope_vector(
            [F32_ZERO, F32_ZERO, NativeF32Bits::from_bits(0xc140_0000)],
            identity_matrix(),
        )
        .unwrap();
        assert_eq!(reflected[0].bits(), 0x0000_0000);
        assert_eq!(reflected[1].bits(), 0x8000_0000);
        assert_eq!(reflected[2].bits(), 0x4140_0000);
    }

    #[test]
    fn matrix_vector_keeps_native_non_associative_dot_order() {
        let matrix = [
            NativeF32Bits::ONE,
            NativeF32Bits::from_bits(0x7180_0000),
            NativeF32Bits::from_bits(0xf180_0000),
            F32_ZERO,
            F32_ZERO,
            F32_ZERO,
            F32_ZERO,
            F32_ZERO,
            F32_ZERO,
            F32_ZERO,
            F32_ZERO,
            F32_ZERO,
        ];
        let result = matrix_vector(matrix, [NativeF32Bits::ONE; 3]).unwrap();
        assert_eq!(result[0], NativeF32Bits::ONE);
    }

    #[test]
    fn color_progression_uses_strict_greater_than_and_count_minus_two() {
        let mut state = spark_state(F32_ZERO);
        state.color_accumulator = NativeF64Bits::ONE;
        advance_color(&mut state, F64_ZERO, 0, 5).unwrap();
        assert_eq!(state.color_index, 0);
        assert_eq!(state.color_accumulator, NativeF64Bits::ONE);

        advance_color(&mut state, F64_HALF, 0, 5).unwrap();
        assert_eq!(state.color_index, 1);
        assert_eq!(state.color_accumulator, NativeF64Bits::POSITIVE_ZERO);

        state.color_index = 3;
        state.color_accumulator = NativeF64Bits::ONE;
        advance_color(&mut state, F64_HALF, 0, 5).unwrap();
        assert_eq!(state.color_index, 3);
        assert_eq!(state.color_accumulator, NativeF64Bits::ONE);
    }

    #[test]
    fn lifetime_zero_wraps_to_negative_one_without_lifetime_deletion() {
        let mut particle = particle(IVec3::new(0, 0, 1000), F32_ZERO, 0);
        let mut rng = SimRng::new(1);
        tick_particle_with_facts(
            &mut particle,
            SparkTickInputs {
                gravity: F32_ZERO,
                color_speed: F64_ZERO,
                color_count: 5,
                collision: facts(0),
            },
            &mut rng,
        )
        .unwrap();
        assert_eq!(particle.lifetime_remaining, -1);
        assert!(!particle.marked_for_deletion);
    }
}
