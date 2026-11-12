//! Ares 3.0 ParticleSystemExt (`Ext/ParticleSystem`, ExtMap at Ares.dll
//! `100C1B24`): Smoke, Spark and Railgun systems whose held particle type has
//! no LineTrail and no AlphaImage. Ares simulates them in its own particle
//! records instead of ParticleClass objects, drawing from the synchronised
//! Scenario stream, and draws the records itself after the Air layer.
//!
//! The hooks:
//! - `0x62DF05`, the end of `ParticleSystemClass::Constructor @ 0x0062DC50`
//!   (`10028D10` -> `100267E0`): [`AresParticleSystemExt::for_system`] picks
//!   the mode.
//! - `0x62FD60`, the entry of `ParticleSystemClass::AI` (`100290D0`, returning
//!   to its `ret` at `0x62FE43`): [`update`] (`10026C70`) runs instead of the
//!   stock subtype AI; `system_ai` keeps the shared lifetime and retirement
//!   tail, which `10026C70` repeats with these records counted.
//! - `0x62E380`, the entry of `SpawnParticle` (`100290B0`, to its `ret 8` at
//!   `0x62E428`): such a system constructs no ParticleClass (`spawn.rs`).
//! - `0x62E2AD` in `DrawIt` (`10029050`): the one-frame light counts these
//!   records (`app::presentation::combat_lights`).
//! - `0x6D9427` (`10029100` -> `10026760` -> `10026430`): the records draw
//!   after the Air layer (`app::presentation::instances::particles`).
//! - Save/load (`0x62FF20`, `0x630088`, `0x630090`, `0x6300F3`): the records
//!   are part of the saved system; the destructor hook `0x62E26B` frees them
//!   with it.
//!
//! These are reimplementations, not the stock AIs (`AI_Spark @ 0x0062E840`,
//! `AI_Railgun @ 0x0062F230`, `AI_Smoke @ 0x0062ED40`): the number, order and
//! ranges of their Scenario draws differ. The arithmetic is the DLL's x87 code
//! under gamemd's PC53/chop control word; its `sqrt` is the CRT `_CIsqrt`
//! (`10083B00`), which switches the rounding to nearest for the `fsqrt`
//! (`10087F25`), and its integer-to-float position stores are SSE `cvtsi2ss`
//! (nearest) and its float-to-integer reads `cvttss2si` (truncating).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, util/ and the rest of sim/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::hash::{Hash, Hasher};

use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};

use super::ParticleSystem;
use crate::rules::particle_system_type::{ParticleSystemBehavesLike, ParticleSystemType};
use crate::rules::particle_type::{ParticleType, ParticleTypeId};
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::native_x87::{
    MaskedX87Chop53 as X, MaskedX87Ordering, MaskedX87Value, NativeF32Bits, NativeF64Bits,
};

/// Ares.dll `100954C8`: binary64 2^-31, the factor every
/// `RandomRanged(1, 0x7FFFFFFF)` draw here is scaled by.
const RANDOM_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x3e00_0000_0000_0000);
/// Ares.dll `10099EC0`: 0.05, the colour factor's random step.
const COLOR_JITTER: NativeF64Bits = NativeF64Bits::from_bits(0x3fa9_9999_9999_999a);
/// Ares.dll `10099EC8`: 0.1, the railgun speed's random step.
const SPEED_JITTER: NativeF64Bits = NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);
/// Ares.dll `10099ED0` / `10099EE0`: 0.3 and 0.6, the light radius walk.
const RADIUS_STEP_DOWN: NativeF64Bits = NativeF64Bits::from_bits(0x3fd3_3333_3333_3333);
const RADIUS_STEP_UP: NativeF64Bits = NativeF64Bits::from_bits(0x3fe3_3333_3333_3333);
/// Ares.dll `10099ED8`: 0.35, the smoke rise slowdown per spawn frame.
const SMOKE_SPEED_FACTOR: NativeF64Bits = NativeF64Bits::from_bits(0x3fd6_6666_6666_6666);
/// Ares.dll `10095EC8`: 2.0, the slowest a new smoke record rises.
const SMOKE_MIN_RISE: NativeF64Bits = NativeF64Bits::from_bits(0x4000_0000_0000_0000);
/// Ares.dll `10099EE8`: 3.0f, the rise below which `Deacc` stops applying.
const SMOKE_DEACC_FLOOR: NativeF32Bits = NativeF32Bits::from_bits(0x4040_0000);
const ZERO_F32: NativeF32Bits = NativeF32Bits::POSITIVE_ZERO;
const ONE_F32: NativeF32Bits = NativeF32Bits::ONE;
const TWO_F32: NativeF32Bits = NativeF32Bits::from_bits(0x4000_0000);

/// ExtData +0x8: 1 Spark, 2 Railgun, 3 Smoke (0, the stock AI, is no
/// extension here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AresSystemMode {
    Spark,
    Railgun,
    Smoke,
}

/// One 0x2C-byte record of ExtData +0x10 (`ParticleSystemParticle`), a spark
/// or a railgun particle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AresPointParticle {
    /// +0x00: position in leptons.
    position: [NativeF32Bits; 3],
    /// +0x0C: movement per frame (Spark) or unit direction (Railgun).
    velocity: [NativeF32Bits; 3],
    /// +0x18: the railgun speed multiplier; zero for a spark.
    speed: NativeF32Bits,
    /// +0x1C: the fade from `ColorList[color_index]` to the next entry.
    color_factor: NativeF32Bits,
    /// +0x20
    color_index: i32,
    /// +0x24: frames left.
    lifetime: i32,
    /// +0x28: set and removed within one update (`10026DD0`).
    dead: bool,
    /// +0x29: the start colour, drawn for `color_index` 0.
    color: [u8; 3],
}

impl AresPointParticle {
    pub fn position(&self) -> [NativeF32Bits; 3] {
        self.position
    }

    pub fn velocity(&self) -> [NativeF32Bits; 3] {
        self.velocity
    }

    pub fn speed(&self) -> NativeF32Bits {
        self.speed
    }

    pub fn color_factor(&self) -> NativeF32Bits {
        self.color_factor
    }

    pub fn color_index(&self) -> i32 {
        self.color_index
    }

    pub fn lifetime(&self) -> i32 {
        self.lifetime
    }

    pub fn color(&self) -> [u8; 3] {
        self.color
    }

    /// The position the draw (`10026430`) truncates with `cvttss2si`.
    pub fn coords(&self) -> IVec3 {
        IVec3::from_array(self.position.map(sse_truncate))
    }

    /// `10026430`'s colour: `ColorList[index]` (the record's own colour for
    /// index 0) faded towards the next entry by the factor through
    /// [`interpolate_color`]. `None` where the next entry is past the list
    /// (Ares reads beyond it).
    pub fn draw_color(&self, color_list: &[[u8; 3]]) -> Option<[u8; 3]> {
        let index = usize::try_from(self.color_index).ok()?;
        let from = if index == 0 {
            self.color
        } else {
            *color_list.get(index)?
        };
        let to = *color_list.get(index.checked_add(1)?)?;
        Some(interpolate_color(to, from, X::load_f32(self.color_factor)))
    }
}

/// One 0x2C-byte record of ExtData +0x1C (`ParticleSystemSmokeParticle`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AresSmokeParticle {
    /// +0x00: location in leptons.
    location: [i32; 3],
    /// +0x0C, +0x10: drift per frame, kept within -5..5.
    drift: [i32; 2],
    /// +0x14: rise per frame, truncated when added.
    rise_speed: NativeF32Bits,
    /// +0x18: the system's UniqueID plus the record count when spawned.
    phase: i32,
    /// +0x1C: the StateAI frame, the shape frame drawn.
    state: i32,
    /// +0x20: frames left.
    lifetime: i32,
    /// +0x24: the held type, or its NextParticle for a replacement record.
    type_id: ParticleTypeId,
    /// +0x28
    translucency: u8,
    /// +0x29: set and removed within one update (`10026DD0`).
    dead: bool,
}

impl AresSmokeParticle {
    pub fn location(&self) -> IVec3 {
        IVec3::from_array(self.location)
    }

    pub fn drift(&self) -> [i32; 2] {
        self.drift
    }

    pub fn rise_speed(&self) -> NativeF32Bits {
        self.rise_speed
    }

    pub fn phase(&self) -> i32 {
        self.phase
    }

    pub fn state(&self) -> i32 {
        self.state
    }

    pub fn lifetime(&self) -> i32 {
        self.lifetime
    }

    pub fn type_id(&self) -> ParticleTypeId {
        self.type_id
    }

    pub fn translucency(&self) -> u8 {
        self.translucency
    }
}

/// ParticleSystemExt::ExtData for a system Ares simulates. Saved and
/// world-hashed with its system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AresParticleSystemExt {
    /// +0x8
    mode: AresSystemMode,
    /// +0xC: the type HoldsWhat named when the system was constructed.
    held_type: ParticleTypeId,
    /// +0x10: Spark and Railgun records.
    particles: Vec<AresPointParticle>,
    /// +0x1C: Smoke records.
    smoke: Vec<AresSmokeParticle>,
}

impl AresParticleSystemExt {
    /// `ExtData::ExtData @ 100267E0`: a system whose type holds a particle
    /// type without `UseLineTrail` and AlphaImage art, whose `BehavesLike`
    /// matches the system's (ParticleType numbers Gas 0 and Smoke 1 the other
    /// way round; Ares maps every value below 2 to `value == 0`), and which
    /// behaves like Smoke, Spark or Railgun.
    ///
    /// RESIDUAL: VERA does not read `UseLineTrail=` (ARTMD.INI, the particle
    /// image's section) for particle types and counts it false; no retail
    /// particle image sets it.
    pub(crate) fn for_system(system: &ParticleSystemType, rules: &RuleSet) -> Option<Self> {
        let held_type = system.holds_what?;
        let held = rules.particle_type(held_type);
        if rules.alpha_image(&held.alpha_image).is_some() {
            return None;
        }
        let behaves = held.behaves_like as i32;
        let behaves = if behaves < 2 {
            i32::from(behaves == 0)
        } else {
            behaves
        };
        if system.behaves_like as i32 != behaves {
            return None;
        }
        let mode = match system.behaves_like {
            ParticleSystemBehavesLike::Smoke => AresSystemMode::Smoke,
            ParticleSystemBehavesLike::Spark => AresSystemMode::Spark,
            ParticleSystemBehavesLike::Railgun => AresSystemMode::Railgun,
            _ => return None,
        };
        Some(Self {
            mode,
            held_type,
            particles: Vec::new(),
            smoke: Vec::new(),
        })
    }

    pub fn mode(&self) -> AresSystemMode {
        self.mode
    }

    pub fn held_type(&self) -> ParticleTypeId {
        self.held_type
    }

    pub fn particles(&self) -> &[AresPointParticle] {
        &self.particles
    }

    pub fn smoke(&self) -> &[AresSmokeParticle] {
        &self.smoke
    }

    /// Both record vectors empty, the extra condition `10026C70` adds to the
    /// stock retirement.
    pub(crate) fn is_empty(&self) -> bool {
        self.particles.is_empty() && self.smoke.is_empty()
    }

    pub(crate) fn hash_state(&self, hasher: &mut impl Hasher) {
        b"ares-particle-system-ext-v1".hash(hasher);
        (match self.mode {
            AresSystemMode::Spark => 1_u8,
            AresSystemMode::Railgun => 2,
            AresSystemMode::Smoke => 3,
        })
        .hash(hasher);
        self.held_type.0.hash(hasher);
        self.particles.len().hash(hasher);
        for particle in &self.particles {
            particle.position.map(|value| value.bits()).hash(hasher);
            particle.velocity.map(|value| value.bits()).hash(hasher);
            particle.speed.bits().hash(hasher);
            particle.color_factor.bits().hash(hasher);
            particle.color_index.hash(hasher);
            particle.lifetime.hash(hasher);
            particle.dead.hash(hasher);
            particle.color.hash(hasher);
        }
        self.smoke.len().hash(hasher);
        for smoke in &self.smoke {
            smoke.location.hash(hasher);
            smoke.drift.hash(hasher);
            smoke.rise_speed.bits().hash(hasher);
            smoke.phase.hash(hasher);
            smoke.state.hash(hasher);
            smoke.lifetime.hash(hasher);
            smoke.type_id.0.hash(hasher);
            smoke.translucency.hash(hasher);
            smoke.dead.hash(hasher);
        }
    }

    /// `RemoveDeadParticles @ 10026DD0`: an order-keeping erase of the dead
    /// records of both vectors.
    fn remove_dead(&mut self) {
        self.particles.retain(|particle| !particle.dead);
        self.smoke.retain(|smoke| !smoke.dead);
    }
}

/// `ExtData::Update @ 10026C70`'s dispatch by mode. The caller keeps the
/// lifetime count and the retirement that follow it.
pub(super) fn update(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    let Some(mut ext) = sys.ares.take() else {
        return;
    };
    match ext.mode {
        AresSystemMode::Spark => update_spark(&mut ext, sys, sim, rules),
        AresSystemMode::Railgun => update_railgun(&mut ext, sys, sim, rules),
        AresSystemMode::Smoke => update_smoke(&mut ext, sys, sim, rules),
    }
    sys.ares = Some(ext);
}

/// `UpdateSpark @ 10028370`: while `SparkSpawnFrames` lasts, the light radius
/// (`+0xF4`) walks by 3 within 17..41, and on the last frame or with
/// `SpawnSparkPercentage` a burst of `ParticleCap/2 .. ParticleCap` sparks
/// starts at the system location; then every spark moves, the dead are
/// removed and the colours advance.
fn update_spark(
    ext: &mut AresParticleSystemExt,
    sys: &mut ParticleSystem,
    sim: &mut Simulation,
    rules: &RuleSet,
) {
    let pst = rules.particle_system_type(sys.type_id);
    let frames = sys.spark_spawn_frames;
    let remaining = frames.wrapping_sub(1);
    if remaining >= 0 {
        sys.spark_spawn_frames = remaining;
        if remaining < 1 {
            sys.mark_done_spawning();
        }
        let jitter = unit_draw(sim.particle_rng());
        if greater(X::load_f64(RADIUS_STEP_DOWN), jitter) {
            sys.facing = (i32::from(sys.facing) - 3).max(0x11) as u8;
        } else if greater(X::load_f64(RADIUS_STEP_UP), jitter) {
            sys.facing = (i32::from(sys.facing) + 3).min(0x29) as u8;
        }
        if remaining == 0
            || greater(
                X::load_f64(pst.spawn_spark_percentage),
                unit_draw(sim.particle_rng()),
            )
        {
            let held = rules.particle_type(ext.held_type);
            let half = (pst.particle_cap as i32).max(0) / 2;
            let count = sim
                .particle_rng()
                .next_range_i32_inclusive(0, half)
                .wrapping_add(half);
            for _ in 0..count.max(0) {
                let spark = spawn_spark(sys.coords, pst, held, sim.particle_rng());
                ext.particles.push(spark);
            }
            // 1002886E: the persistent light of the first spawn frame; the
            // client applies the Detail == 2 gate.
            if frames == pst.spark_spawn_frames as i32 && !pst.one_frame_light && pst.light_size > 0
            {
                sim.combat_light_requests
                    .emit(crate::sim::combat::CombatLightRequest::Spark {
                        coord: crate::sim::projectile::ProjectileCoord::new(
                            sys.coords.x,
                            sys.coords.y,
                            sys.coords.z,
                        ),
                        base_size: pst.light_size,
                    });
            }
        }
    }
    spark_movement(ext, sim, rules);
    ext.remove_dead();
    update_colors(ext, rules, sim.particle_rng());
}

/// One spark of `10028370`'s burst (`100284E0..10028868`): a random speed
/// along `SpawnDirection` plus three draws, a lifetime of `MaxEC` plus
/// `0..MaxEC` (only the low word of the first), and the start colour.
fn spawn_spark(
    location: IVec3,
    pst: &ParticleSystemType,
    held: &ParticleType,
    rng: &mut SimRng,
) -> AresPointParticle {
    let x = rng.next_range_i32_inclusive(0, held.x_velocity);
    let y = rng.next_range_i32_inclusive(0, held.y_velocity);
    let z = rng
        .next_range_i32_inclusive(0, held.z_velocity_range)
        .wrapping_add(held.min_z_velocity);
    // fild, fstp dword: each draw as a float.
    let draws = [x, y, z].map(|draw| X::store_f32_masked_chop(X::load_i32(draw)));
    let [x, y, z] = draws.map(X::load_f32);
    let speed = crt_sqrt(X::add(X::add(X::mul(y, y), X::mul(x, x)), X::mul(z, z)));
    // VERA keeps SpawnDirection as whole leptons; the DLL adds its floats.
    let spawn_direction = pst.spawn_direction.to_array();
    let mut direction = [0, 1, 2].map(|axis| {
        X::store_f32_masked_chop(X::add(
            X::load_f32(X::store_f32_masked_chop(X::load_i32(spawn_direction[axis]))),
            X::load_f32(draws[axis]),
        ))
    });
    let [dx, dy, dz] = direction.map(X::load_f32);
    let length = crt_sqrt(X::add(
        X::add(X::mul(dx, dx), X::mul(dy, dy)),
        X::mul(dz, dz),
    ));
    if X::compare(length, X::load_i32(0)) != MaskedX87Ordering::Equal {
        let inverse = X::div(X::load_f64(NativeF64Bits::ONE), length);
        direction = direction
            .map(|component| X::store_f32_masked_chop(X::mul(X::load_f32(component), inverse)));
    }
    let velocity =
        direction.map(|component| X::store_f32_masked_chop(X::mul(X::load_f32(component), speed)));
    let lifetime = rng
        .next_range_i32_inclusive(0, held.max_ec)
        .wrapping_add(i32::from(held.max_ec as u16));
    AresPointParticle {
        position: location.to_array().map(int_to_f32),
        velocity,
        speed: ZERO_F32,
        color_factor: ZERO_F32,
        color_index: 0,
        lifetime,
        dead: false,
        color: start_color(held, rng),
    }
}

/// `UpdateSparkMovement @ 10028900`: a spark dies when its lifetime ends;
/// otherwise gravity slows it (the Z step subtracts `Gravity` a second time,
/// as the DLL does) and it dies on the world the cell query reports
/// ([`super::spark_world::ares_spark_dies`]).
fn spark_movement(ext: &mut AresParticleSystemExt, sim: &Simulation, rules: &RuleSet) {
    // cvtsi2ss of [General] Gravity.
    let gravity = X::load_f32(int_to_f32(rules.general.gravity));
    for spark in &mut ext.particles {
        spark.lifetime = spark.lifetime.wrapping_sub(1);
        if spark.lifetime <= 0 {
            spark.dead = true;
            continue;
        }
        spark.velocity[2] =
            X::store_f32_masked_chop(X::sub(X::load_f32(spark.velocity[2]), gravity));
        let old = spark.coords();
        for axis in 0..2 {
            spark.position[axis] = X::store_f32_masked_chop(X::add(
                X::load_f32(spark.position[axis]),
                X::load_f32(spark.velocity[axis]),
            ));
        }
        let fall = X::store_f32_masked_chop(X::sub(X::load_f32(spark.velocity[2]), gravity));
        spark.position[2] =
            X::store_f32_masked_chop(X::add(X::load_f32(fall), X::load_f32(spark.position[2])));
        match super::spark_world::ares_spark_dies(sim, rules, old, spark.coords()) {
            Ok(dies) => spark.dead |= dies,
            Err(error) => {
                // Native has no failure exit; VERA retires a spark whose
                // cell it cannot evaluate (a slope type above 20).
                log::warn!("particles: Ares spark retired, its cell was refused: {error}");
                spark.dead = true;
            }
        }
    }
}

/// `UpdateParticleColors @ 10026D30`: each record's factor rises by
/// `ColorSpeed` plus a random fraction of 0.05; past 1.0 it moves to the next
/// colour (restarting at 0) until the second-last entry, where it holds 1.0.
fn update_colors(ext: &mut AresParticleSystemExt, rules: &RuleSet, rng: &mut SimRng) {
    let held = rules.particle_type(ext.held_type);
    let color_speed = X::load_f64(held.color_speed);
    let last = (held.color_list.len() as i32).wrapping_sub(2);
    for particle in &mut ext.particles {
        let factor = X::add(
            X::add(
                X::mul(unit_draw(rng), X::load_f64(COLOR_JITTER)),
                color_speed,
            ),
            X::load_f32(particle.color_factor),
        );
        particle.color_factor = if greater(factor, X::load_i32(1)) {
            if particle.color_index < last {
                particle.color_index = particle.color_index.wrapping_add(1);
                ZERO_F32
            } else {
                ONE_F32
            }
        } else {
            X::store_f32_masked_chop(factor)
        };
    }
}

/// `UpdateRailgun @ 10026E70`: on its first frame (not done, no records) the
/// system lays `ParticlesPerCoord` records per lepton on a spiral from its
/// target to its location, and the laser for `Laser=yes`; every frame each
/// record moves at the speed of the previous frame and that speed jitters.
fn update_railgun(
    ext: &mut AresParticleSystemExt,
    sys: &mut ParticleSystem,
    sim: &mut Simulation,
    rules: &RuleSet,
) {
    if !sys.done_spawning && ext.particles.is_empty() {
        let (source, target) = (sys.coords, sys.target_coords);
        sys.mark_done_spawning();
        let pst = rules.particle_system_type(sys.type_id);
        lay_rail(ext, source, target, pst, rules, sim.particle_rng());
        if pst.laser {
            super::railgun::draw_laser(sim, pst, source, target);
        }
    }
    let rng = sim.particle_rng();
    for particle in &mut ext.particles {
        let speed = X::load_f32(particle.speed);
        particle.speed = X::store_f32_masked_chop(X::add(
            X::mul(centred_draw(rng), X::load_f64(SPEED_JITTER)),
            speed,
        ));
        particle.lifetime = particle.lifetime.wrapping_sub(1);
        for axis in 0..3 {
            let step =
                X::store_f32_masked_chop(X::mul(X::load_f32(particle.velocity[axis]), speed));
            particle.position[axis] = X::store_f32_masked_chop(X::add(
                X::load_f32(step),
                X::load_f32(particle.position[axis]),
            ));
        }
        if particle.lifetime <= 0 {
            particle.dead = true;
        }
    }
    ext.remove_dead();
    update_colors(ext, rules, rng);
}

/// The spawn half of `10026E70` (`10026EB2..10027799`). Per record, in this
/// order: two position draws (X, Y), one more (Z), three direction draws, the
/// speed walk draw, a lifetime draw of 0..9 and the start colour's draw.
fn lay_rail(
    ext: &mut AresParticleSystemExt,
    source: IVec3,
    target: IVec3,
    pst: &ParticleSystemType,
    rules: &RuleSet,
    rng: &mut SimRng,
) {
    use crate::util::native_x87::X87Chop53 as Fpu;
    use crate::util::retail_trig::{AcosTable, TrigTable};

    let delta = [
        source.x.wrapping_sub(target.x),
        source.y.wrapping_sub(target.y),
        source.z.wrapping_sub(target.z),
    ];
    let [x, y, z] = delta.map(X::load_i32);
    let plane = X::add(X::mul(y, y), X::mul(x, x));
    let distance = ftol(crt_sqrt(X::add(plane, X::mul(z, z))));
    let distance_xy = ftol(crt_sqrt(plane));
    let length = X::load_i32(distance);
    let count = ftol(X::mul(X::load_f64(pst.particles_per_coord), length));
    if count <= 0 {
        // The angles below are pure, so skipping them changes nothing.
        return;
    }
    let held = rules.particle_type(ext.held_type);

    // 10026F5B..10026F93: dz clamped to +-distance, dx to +-distance_xy.
    let clamp = |value: i32, bound: i32| {
        let upper = if bound >= value { value } else { bound };
        if upper < bound.wrapping_neg() {
            bound.wrapping_neg()
        } else {
            upper
        }
    };
    let dz = clamp(delta[2], distance);
    let dx = clamp(delta[0], distance_xy);
    // gamemd Math::asin 4CAD80 and Math::acos 4CADB0; a vertical shot divides
    // zero by zero (acos of NaN). `count > 0` makes distance nonzero.
    let acos = AcosTable::retail();
    let Ok(argument) = Fpu::div(Fpu::load_i32(dz), Fpu::load_i32(distance)) else {
        return;
    };
    let (_, pitch) = acos.asin_lookup(argument);
    let yaw = match Fpu::div(Fpu::load_i32(dx), Fpu::load_i32(distance_xy)) {
        Ok(argument) => acos.acos_lookup(argument).1,
        Err(_) => acos.acos_lookup_nan(),
    };
    // RotateZ takes (float)(yaw * sign); the chop store is sign-symmetric.
    let mut azimuth = X::store_f32_masked_chop(yaw.into());
    if delta[1] < 0 {
        azimuth = NativeF32Bits::from_bits(azimuth.bits() ^ 0x8000_0000);
    }
    let elevation = X::store_f32_masked_chop(pitch.into());
    let Ok(matrix) = super::railgun::rail_rotation(azimuth, elevation) else {
        return;
    };

    let trig = TrigTable::embedded();
    let count_value = X::load_i32(count);
    let one = X::load_f64(NativeF64Bits::ONE);
    let radius = X::load_f64(pst.spiral_radius);
    let position_scale = X::load_f64(pst.position_perturbation_coefficient);
    let movement_scale = X::load_f64(pst.movement_perturbation_coefficient);
    let velocity_scale = X::load_f64(pst.velocity_perturbation_coefficient);
    let floor = X::neg(movement_scale);
    let mut perturbation = X::load_i32(0);
    for index in 0..count {
        let ratio = X::div(X::load_i32(index), count_value);
        let angle = X::mul(
            X::mul(ratio, length),
            X::load_f64(pst.spiral_delta_per_coord),
        );
        let table = |value: crate::util::native_x87::X87Value| {
            f32::from_bits(X::store_f32_masked_chop(value.into()).bits())
        };
        let spiral = Vec3::new(
            0.0,
            table(trig.cos_from_table(angle)),
            table(trig.sin_from_table(angle)),
        );
        let Some(ring) = crate::util::native_affine::transform_point(matrix, spiral) else {
            log::warn!("particles: Ares railgun spiral left the finite affine domain");
            return;
        };
        let ring = [ring.x, ring.y, ring.z].map(|value| NativeF32Bits::from_bits(value.to_bits()));
        let [ring_x, ring_y, ring_z] = ring.map(X::load_f32);

        // 10027212..100273A8: each axis is three truncated terms: the spiral
        // offset plus the position jitter, then Location and the target
        // weighted by the record's place on the rail.
        let remainder = X::sub(one, ratio);
        let along = |source: i32, target: i32| {
            ftol(X::mul(X::load_i32(source), ratio))
                .wrapping_add(ftol(X::mul(X::load_i32(target), remainder)))
        };
        let jitter_x = centred_draw(rng);
        let jitter_y = centred_draw(rng);
        let px = ftol(X::add(
            X::mul(ring_x, radius),
            X::mul(position_scale, jitter_x),
        ))
        .wrapping_add(along(source.x, target.x));
        let py = ftol(X::add(
            X::mul(position_scale, jitter_y),
            X::mul(radius, ring_y),
        ))
        .wrapping_add(along(source.y, target.y));
        let jitter_z = centred_draw(rng);
        let pz = ftol(X::add(
            X::mul(jitter_z, position_scale),
            X::mul(ring_z, radius),
        ))
        .wrapping_add(along(source.z, target.z));

        // 100273AA..100274E5: the ring vector plus three movement draws,
        // each product stored as float first, normalized when nonzero.
        let jitter_vx = centred_draw(rng);
        let jitter_vy = centred_draw(rng);
        let jitter_vz = centred_draw(rng);
        let mut velocity = [
            (jitter_vx, ring[0]),
            (jitter_vy, ring[1]),
            (jitter_vz, ring[2]),
        ]
        .map(|(jitter, ring)| {
            X::store_f32_masked_chop(X::add(
                X::load_f32(X::store_f32_masked_chop(X::mul(jitter, movement_scale))),
                X::load_f32(ring),
            ))
        });
        let [vx, vy, vz] = velocity.map(X::load_f32);
        let magnitude = crt_sqrt(X::add(
            X::add(X::mul(vx, vx), X::mul(vy, vy)),
            X::mul(vz, vz),
        ));
        if X::compare(magnitude, X::load_i32(0)) != MaskedX87Ordering::Equal {
            let inverse = X::div(one, magnitude);
            velocity = velocity
                .map(|component| X::store_f32_masked_chop(X::mul(X::load_f32(component), inverse)));
        }

        // 100274E9..10027555: the speed walks along the rail, capped at
        // VelocityPerturbationCoefficient and floored at minus
        // MovementPerturbationCoefficient (as the DLL reads it).
        perturbation = X::mul(
            X::mul(X::add(centred_draw(rng), perturbation), velocity_scale),
            X::load_f64(NativeF64Bits::HALF),
        );
        if greater(perturbation, velocity_scale) {
            perturbation = velocity_scale;
        }
        if greater(floor, perturbation) {
            perturbation = floor;
        }
        let speed = X::store_f32_masked_chop(X::add(X::load_f32(held.velocity), perturbation));
        let lifetime = rng
            .next_range_i32_inclusive(0, 9)
            .wrapping_add(i32::from(held.max_ec as u16));
        let color = start_color(held, rng);
        ext.particles.push(AresPointParticle {
            position: [px, py, pz].map(int_to_f32),
            velocity,
            speed,
            color_factor: ZERO_F32,
            color_index: 0,
            lifetime,
            dead: false,
            color,
        });
    }
}

/// `UpdateSmoke @ 10027950`: the system follows its owner object, the records
/// move, each dead record whose type has a `NextParticle` is replaced by two
/// of that type spread by its `Radius`, and every `SpawnFrames` frames until
/// `SpawnCutoff` one record of the held type spawns near the system.
fn update_smoke(
    ext: &mut AresParticleSystemExt,
    sys: &mut ParticleSystem,
    sim: &mut Simulation,
    rules: &RuleSet,
) {
    // 10027966..100279CB: any owner object (Ares tests the Object flag, not
    // the stock Foot one) moves the system to its GetCoords plus the
    // constructor's offset.
    if let Some(owner) = sys
        .owner_entity
        .and_then(|owner| sim.substrate.entities.get(owner))
    {
        let origin = crate::sim::movement::ground_pose::object_get_coords(
            owner,
            sim.resolved_terrain.as_ref(),
        );
        sys.coords = IVec3::new(
            sys.offset.x.wrapping_add(origin.x),
            sys.offset.y.wrapping_add(origin.y),
            sys.offset.z.wrapping_add(origin.z),
        );
    }
    smoke_movement(ext, sim, rules);

    // 100279D8..10027D95: backwards over the records present now; the
    // replacements appended here are not visited.
    let rng = sim.particle_rng();
    for index in (0..ext.smoke.len()).rev() {
        let parent = ext.smoke[index].clone();
        if !parent.dead {
            continue;
        }
        let Some(next_id) = rules.particle_type(parent.type_id).next_particle else {
            continue;
        };
        let next = rules.particle_type(next_id);
        let lifetime = rng
            .next_range_i32_inclusive(0, next.max_ec)
            .wrapping_add(i32::from(next.max_ec as u16));
        let count = ext.smoke.len() as i32;
        let fade = if rng.next_range_i32_inclusive(0, 5) != 0 {
            25
        } else {
            0
        };
        let spread = next.radius.wrapping_shl(8);
        let offset_x = rng.next_range_i32_inclusive(spread.wrapping_neg(), spread);
        let offset_y = rng.next_range_i32_inclusive(spread.wrapping_neg(), spread);
        let [x, y, z] = parent.location;
        let child = |location: [i32; 3], phase: i32| AresSmokeParticle {
            location,
            drift: [0, 0],
            rise_speed: parent.rise_speed,
            phase,
            state: i32::from(next.start_state_ai),
            lifetime,
            type_id: next_id,
            translucency: parent.translucency.wrapping_add(fade),
            dead: false,
        };
        ext.smoke.push(child(
            [x.wrapping_add(offset_x), y.wrapping_add(offset_y), z],
            parent.phase.wrapping_add(count).wrapping_add(1),
        ));
        ext.smoke.push(child(
            [x.wrapping_sub(offset_x), y.wrapping_sub(offset_y), z],
            count.wrapping_add(2).wrapping_add(parent.phase),
        ));
    }
    ext.remove_dead();

    // 10027DAA..1002807C: the spawn. The period is the truncated SpawnFrames
    // float (+0xE8); a Foot owner in a tube (signed +0x684 >= 0) spawns
    // nothing.
    let pst = rules.particle_system_type(sys.type_id);
    if !sys.done_spawning && sys.alive {
        // Ares divides by the period unchecked (zero, or INT_MIN by -1,
        // traps); VERA spawns nothing that frame.
        let period = sse_truncate(sys.spawn_timer);
        let due = (sim.session.binary_frame() as i32).checked_rem(period) == Some(0);
        if due && !super::smoke::owner_in_tube(sim, sys.owner_entity) {
            let held = rules.particle_type(ext.held_type);
            let range = pst.spawn_radius.wrapping_add(1);
            let rng = sim.particle_rng();
            let offset_x = rng.next_range_i32_inclusive(range.wrapping_neg(), range);
            let offset_y = rng.next_range_i32_inclusive(range.wrapping_neg(), range);
            let coords = IVec3::new(
                sys.coords.x.wrapping_add(offset_x),
                sys.coords.y.wrapping_add(offset_y),
                sys.coords.z.wrapping_add(10),
            );
            // MapClass::GetCellFloorHeight 578080, raised to the coordinate.
            let floor = match super::spark_world::constructor_ground_height(sim, coords.x, coords.y)
            {
                Ok(floor) => floor.unwrap_or(coords.z),
                Err(error) => {
                    log::warn!("particles: Ares smoke spawn floor refused: {error}");
                    coords.z
                }
            };
            let mut translucency = held.translucency;
            if greater(
                X::load_f32(sys.spawn_timer),
                X::load_f32(pst.spawn_translucency_cutoff),
            ) {
                translucency = translucency.wrapping_add(25);
            }
            let speed = X::sub(
                X::load_f32(held.velocity),
                X::mul(
                    X::sub(
                        X::load_f32(sys.spawn_timer),
                        X::load_f32(X::store_f32_masked_chop(X::load_i32(pst.spawn_frames))),
                    ),
                    X::load_f64(SMOKE_SPEED_FACTOR),
                ),
            );
            let rise_speed = if greater(X::load_f64(SMOKE_MIN_RISE), speed) {
                TWO_F32
            } else {
                X::store_f32_masked_chop(speed)
            };
            // The UniqueID plus the record count with the new one.
            let phase = sys
                .native_unique_id
                .wrapping_add(ext.smoke.len() as i32)
                .wrapping_add(1);
            let lifetime = sim
                .particle_rng()
                .next_range_i32_inclusive(0, held.max_ec)
                .wrapping_add(i32::from(held.max_ec as u16));
            ext.smoke.push(AresSmokeParticle {
                location: [coords.x, coords.y, floor.max(coords.z)],
                drift: [0, 0],
                rise_speed,
                phase,
                state: i32::from(held.start_state_ai),
                lifetime,
                type_id: ext.held_type,
                translucency,
                dead: false,
            });
        }
    }

    // 10028080..100280AC: the period grows by Slowdown; past SpawnCutoff the
    // system is done. The stored float is compared.
    sys.spawn_timer = X::store_f32_masked_chop(X::add(
        X::load_f32(pst.slowdown),
        X::load_f32(sys.spawn_timer),
    ));
    if greater(X::load_f32(sys.spawn_timer), X::load_f32(pst.spawn_cutoff)) {
        sys.mark_done_spawning();
    }
}

/// `UpdateSmokeMovement @ 100280D0`, three passes over the records: on odd
/// frames each changes its drift by -1..1 on one axis with a chance of 1 in
/// 4; the state animation advances (dying at `EndStateAI` with
/// `DeleteOnStateLimit`), the rise slows by `Deacc` above 3 and the lifetime
/// counts down; then wind, drift and rise move it, and it dies rising through
/// a bridge deck.
fn smoke_movement(ext: &mut AresParticleSystemExt, sim: &mut Simulation, rules: &RuleSet) {
    if sim.session.binary_frame() & 1 != 0 {
        let rng = sim.particle_rng();
        for smoke in &mut ext.smoke {
            if rng.next_range_i32_inclusive(0, 3) != 0 {
                continue;
            }
            let delta = rng.next_range_i32_inclusive(-1, 1);
            let [mut x, mut y] = smoke.drift;
            if rng.next_range_i32_inclusive(0, 1) != 0 {
                x = x.wrapping_add(delta);
            } else {
                y = y.wrapping_add(delta);
            }
            smoke.drift = [x.clamp(-5, 5), y.clamp(-5, 5)];
        }
    }
    for smoke in &mut ext.smoke {
        let kind = rules.particle_type(smoke.type_id);
        let end = i32::from(kind.end_state_ai);
        if smoke.state < end {
            // The phase offsets the start and makes the period
            // StateAIAdvance +-1 (C remainder). Ares divides unchecked; VERA
            // calls a zero period (or INT_MIN by -1) never due.
            let age = kind
                .max_ec
                .wrapping_sub(smoke.lifetime)
                .wrapping_add(smoke.phase);
            let period = i32::from(kind.state_ai_advance).wrapping_add(smoke.phase.wrapping_rem(2));
            if age.checked_rem(period) == Some(0) {
                smoke.state = smoke.state.wrapping_add(1);
                if smoke.state >= end && kind.delete_on_state_limit {
                    smoke.dead = true;
                }
            }
        }
        if greater(
            X::load_f32(smoke.rise_speed),
            X::load_f32(SMOKE_DEACC_FLOOR),
        ) {
            smoke.rise_speed = X::store_f32_masked_chop(X::sub(
                X::load_f32(smoke.rise_speed),
                X::load_f32(kind.deacc),
            ));
        }
        smoke.lifetime = smoke.lifetime.wrapping_sub(1);
        if smoke.lifetime <= 0 {
            smoke.dead = true;
        }
    }
    // The wind tables at 8366A4/8366C4, indexed unchecked by [General]
    // WindDirection; VERA gives a value outside 0..7 no wind, as the stock
    // smoke does.
    let (wind_x, wind_y) = usize::try_from(rules.general.wind_direction())
        .ok()
        .filter(|&index| index < super::wind::SMOKE_WIND_DX.len())
        .map_or((0, 0), |index| {
            (
                super::wind::SMOKE_WIND_DX[index],
                super::wind::SMOKE_WIND_DY[index],
            )
        });
    for smoke in &mut ext.smoke {
        let old = smoke.location;
        let effect = rules.particle_type(smoke.type_id).wind_effect;
        let new_z = old[2].wrapping_add(sse_truncate(smoke.rise_speed));
        smoke.location = [
            wind_x
                .wrapping_mul(effect)
                .wrapping_add(smoke.drift[0])
                .wrapping_add(old[0]),
            wind_y
                .wrapping_mul(effect)
                .wrapping_add(smoke.drift[1])
                .wrapping_add(old[1]),
            new_z,
        ];
        // 10028270..10028354: the old cell's bridge deck (floor + 0x1A0);
        // rising from below it to floor + 0x9C or higher dies.
        match super::spark_world::smoke_bridge_deck(sim, IVec3::from_array(old)) {
            Ok(Some(deck)) => {
                if old[2] < deck && new_z >= deck.wrapping_sub(0x104) {
                    smoke.dead = true;
                }
            }
            Ok(None) => {}
            Err(error) => {
                log::warn!("particles: Ares smoke retired, its cell was refused: {error}");
                smoke.dead = true;
            }
        }
    }
}

/// The start colour inlined in both spawns (`10028787..10028856`,
/// `100276C7..10027774`): black without a ColorList; a draw blending
/// `StartColor2` into `StartColor1` when both are non-black; else the first
/// listed colour.
fn start_color(held: &ParticleType, rng: &mut SimRng) -> [u8; 3] {
    let Some(&first) = held.color_list.first() else {
        return [0; 3];
    };
    if held.start_color_1 == [0; 3] || held.start_color_2 == [0; 3] {
        return first;
    }
    let ratio = unit_draw(rng);
    interpolate_color(held.start_color_1, held.start_color_2, ratio)
}

/// `InterpolateParticleColor @ 10026230`: `to * ratio + from * (1 - ratio)`
/// per channel, each truncated (`_ftol2`) and clamped to 0..255.
pub fn interpolate_color(to: [u8; 3], from: [u8; 3], ratio: MaskedX87Value) -> [u8; 3] {
    let inverse = X::sub(X::load_f64(NativeF64Bits::ONE), ratio);
    [0, 1, 2].map(|channel| {
        let value = ftol(X::add(
            X::mul(X::load_i32(i32::from(from[channel])), inverse),
            X::mul(X::load_i32(i32::from(to[channel])), ratio),
        ));
        value.clamp(0, 255) as u8
    })
}

/// `RandomRanged(1, 0x7FFFFFFF)` times 2^-31 (`100954C8`).
fn unit_draw(rng: &mut SimRng) -> MaskedX87Value {
    X::mul(
        X::load_i32(rng.next_range_i32_inclusive(1, i32::MAX)),
        X::load_f64(RANDOM_SCALE),
    )
}

/// [`unit_draw`] less 0.5 (`10095EC0`).
fn centred_draw(rng: &mut SimRng) -> MaskedX87Value {
    X::sub(unit_draw(rng), X::load_f64(NativeF64Bits::HALF))
}

/// The CRT `_CIsqrt @ 10083B00`: `fsqrt` at the current (53-bit) precision
/// with the rounding switched to nearest (`10087F25`), which is the binary64
/// square root. Every operand here is a sum of squares; a NaN or infinity
/// passes through as `fsqrt` passes it.
fn crt_sqrt(value: MaskedX87Value) -> MaskedX87Value {
    let operand = f64::from_bits(X::store_f64_masked_chop(value).bits());
    if operand.is_nan() || operand == f64::INFINITY {
        return value;
    }
    if operand < 0.0 {
        // fsqrt of a negative operand: the indefinite NaN.
        return X::div(X::load_i32(0), X::load_i32(0));
    }
    X::load_f64(NativeF64Bits::from_bits(operand.sqrt().to_bits()))
}

/// Ares' `_ftol2 @ 100772B0`: truncation, `INT_MIN` out of range.
fn ftol(value: MaskedX87Value) -> i32 {
    X::fistp_i32_masked(value)
}

/// SSE `cvttss2si`: truncation, `INT_MIN` for NaN and out of range.
fn sse_truncate(bits: NativeF32Bits) -> i32 {
    let value = f32::from_bits(bits.bits());
    if value.is_nan() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

/// SSE `cvtsi2ss`: the nearest float.
fn int_to_f32(value: i32) -> NativeF32Bits {
    NativeF32Bits::from_bits((value as f32).to_bits())
}

/// x87 `fcomi` followed by `ja`: ordered and greater.
fn greater(lhs: MaskedX87Value, rhs: MaskedX87Value) -> bool {
    X::compare(lhs, rhs) == MaskedX87Ordering::Greater
}

// The switch is always off without the feature.
#[cfg(all(test, feature = "ares"))]
#[path = "ares_system_tests.rs"]
mod tests;
