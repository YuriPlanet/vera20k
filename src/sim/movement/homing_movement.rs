//! Homing missile flight — per-tick yaw correction toward a tracked target.
//!
//! Used for projectiles with `Ranged=yes`, such as AAHeatSeeker2 fired by
//! Guardian GI's MissileLauncher. Distinct from `rocket_movement.rs`, which
//! handles ballistic-arc projectiles (V3, dumb-fire) — keep them separate;
//! do not merge.
//!
//! ## State machine
//! Arming → Cruise → Detonation
//!         ↘ SelfDestruct (stall failsafe)
//!
//! ## Determinism
//! Sim-critical numeric fields use `SimFixed` for deterministic lockstep.
//! BAM angles are integer `u16` (wrapping arithmetic is exact). Per-tick trig
//! uses integer LUTs — `cos_bam`/`sin_bam` for yaw-to-velocity, `atan2_bam`
//! for delta-to-heading. No `f32` trig anywhere in the hot path. Render-only
//! `pitch` is `f32` and excluded from the state hash.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/entity_store, sim/game_entity.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::sync::OnceLock;

use crate::sim::entity_store::EntityStore;
use crate::util::fixed_math::{
    SIM_ONE, SIM_ZERO, SimFixed, int_distance_to_sim, native_movement_frame_fraction,
};

/// Phase within the homing missile state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HomingPhase {
    /// Arming: per-tick decrement until ready to detonate on impact.
    Arming,
    /// Cruise: tracking target with sidewinder yaw + cruise altitude control.
    Cruise,
    /// Stall failsafe: target unreachable, detonate next tick.
    SelfDestruct,
    /// Impact: caller despawns this tick.
    Detonation,
}

/// Native BulletClass target role used by homing projectiles.
///
/// Object expiry does not have one universal outcome: a normal ground object
/// becomes the `CellClass` at its final coordinates, while a high-flying
/// object becomes null. Keeping the cell variant explicit prevents a later
/// failed entity lookup from being mistaken for the native fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HomingTarget {
    Object(u64),
    Cell { rx: u16, ry: u16 },
}

/// State for an in-flight homing missile.
///
/// Sim-critical numeric fields use `SimFixed` for deterministic lockstep.
/// BAM angles are `u16` (wrapping integer arithmetic is exact).
/// Render-only `pitch` stays `f32`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HomingState {
    pub phase: HomingPhase,

    // Target tracking
    pub target: Option<HomingTarget>,
    pub last_known_rx: u16,
    pub last_known_ry: u16,

    // Flight kinematics
    pub yaw_bam: u16,
    pub pitch_bam: u16,
    pub speed: SimFixed,
    /// Authoritative sub-cell position. `entity.position.rx/ry` is the
    /// truncation of these for cell-grid queries; this keeps fractional
    /// precision so the missile moves smoothly between cells.
    pub pos_x_cells: SimFixed,
    pub pos_y_cells: SimFixed,
    /// Altitude in leptons. Independent of `entity.position.z` (which is a
    /// coarse elevation level u8) — homing missiles fly in their own
    /// altitude space and decay back toward the cruise band.
    pub altitude: SimFixed,
    pub vz: SimFixed,

    // Per-projectile parameters from BulletType / WeaponType / Rules
    pub rot_ini: u16,
    pub missile_rot_var: SimFixed,
    pub floater: bool,
    pub very_high: bool,
    pub arm_ticks_remaining: u16,

    // Sidewinder phase + stall detection
    pub frame_counter: u32,
    pub stall_counter: u8,
    pub stall_ema: SimFixed,
    pub last_distance_to_target: SimFixed,

    /// Render-only pitch in radians. Excluded from the deterministic state
    /// hash — see the manual `Hash` impl below.
    #[serde(skip, default)]
    pub pitch: f32,
}

/// Precomputed cosine modulation table: `cos(2π * i / 15)` for `i` in `0..15`.
///
/// Replaces runtime cosine evaluation in the homing flight loop. Values are
/// stored as `SimFixed` literals (compile-time parsed) so the table is fully
/// deterministic — no f32 trig in the sim layer.
///
/// The 15-frame period is the "sidewinder" name's origin — the modulation
/// produces the characteristic oscillating flight curve.
const SIDEWINDER_TABLE: [SimFixed; 15] = [
    SimFixed::lit("1.0"),                  // cos(0)
    SimFixed::lit("0.91354545764260087"),  // cos(2π/15)
    SimFixed::lit("0.66913060635885821"),  // cos(4π/15)
    SimFixed::lit("0.30901699437494745"),  // cos(6π/15)
    SimFixed::lit("-0.10452846326765346"), // cos(8π/15)
    SimFixed::lit("-0.5"),                 // cos(10π/15)
    SimFixed::lit("-0.80901699437494745"), // cos(12π/15)
    SimFixed::lit("-0.97814760073380562"), // cos(14π/15)
    SimFixed::lit("-0.97814760073380562"), // cos(16π/15)
    SimFixed::lit("-0.80901699437494745"), // cos(18π/15)
    SimFixed::lit("-0.5"),                 // cos(20π/15)
    SimFixed::lit("-0.10452846326765346"), // cos(22π/15)
    SimFixed::lit("0.30901699437494745"),  // cos(24π/15)
    SimFixed::lit("0.66913060635885821"),  // cos(26π/15)
    SimFixed::lit("0.91354545764260087"),  // cos(28π/15)
];

/// Lookup the sidewinder cosine for the given frame counter.
pub(crate) fn sidewinder_cos(frame_counter: u32) -> SimFixed {
    SIDEWINDER_TABLE[(frame_counter % 15) as usize]
}

/// Q16.16 cosine table indexed by BAM angle. `BAM_COS[i]` is `cos(i * 2π/65536)`
/// quantized to i32 Q16.16. Exact 1-BAM resolution, no interpolation.
///
/// Built once at first call from f64 trig and frozen as integers; the rounding
/// step absorbs the sub-ULP differences between platforms' libm so the table
/// is bitwise identical across machines. Boxed to keep the 256 KB array off
/// the stack during init.
fn bam_cos_table() -> &'static [i32; 65536] {
    static TABLE: OnceLock<Box<[i32; 65536]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t: Box<[i32; 65536]> = vec![0i32; 65536]
            .into_boxed_slice()
            .try_into()
            .expect("65536-entry vec to fixed array");
        for i in 0..65536u32 {
            let angle = (i as f64) * (2.0 * std::f64::consts::PI / 65536.0);
            t[i as usize] = (angle.cos() * 65536.0).round() as i32;
        }
        t
    })
}

/// Q16.16 sine table; see `bam_cos_table` for the determinism rationale.
fn bam_sin_table() -> &'static [i32; 65536] {
    static TABLE: OnceLock<Box<[i32; 65536]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t: Box<[i32; 65536]> = vec![0i32; 65536]
            .into_boxed_slice()
            .try_into()
            .expect("65536-entry vec to fixed array");
        for i in 0..65536u32 {
            let angle = (i as f64) * (2.0 * std::f64::consts::PI / 65536.0);
            t[i as usize] = (angle.sin() * 65536.0).round() as i32;
        }
        t
    })
}

/// Deterministic `cos(bam)` as `SimFixed`. O(1) table lookup.
#[inline]
pub(crate) fn cos_bam(bam: u16) -> SimFixed {
    SimFixed::from_bits(bam_cos_table()[bam as usize])
}

/// Deterministic `sin(bam)` as `SimFixed`. O(1) table lookup.
#[inline]
pub(crate) fn sin_bam(bam: u16) -> SimFixed {
    SimFixed::from_bits(bam_sin_table()[bam as usize])
}

/// Inclusive ROT cap check: returns `true` when current yaw can snap directly
/// to target this tick (i.e. `|delta| <= cap`).
///
/// Inclusive comparison (`<=`) matches the original's IsWithinROT — equality
/// at the boundary snaps; off-by-one would over-rotate by one BAM step.
pub(crate) fn within_rot_bam(cur: u16, tgt: u16, cap: u16) -> bool {
    let delta_signed = (cur.wrapping_sub(tgt)) as i16;
    (delta_signed.unsigned_abs() as u16) <= cap
}

/// Step current BAM angle toward target by at most `cap`; snap to target when
/// within `cap`. Picks the shortest-arc direction via wrapping `i16`
/// subtraction.
pub(crate) fn step_toward_bam_inclusive(cur: u16, tgt: u16, cap: u16) -> u16 {
    if within_rot_bam(cur, tgt, cap) {
        return tgt;
    }
    let delta_signed = (tgt.wrapping_sub(cur)) as i16;
    if delta_signed > 0 {
        cur.wrapping_add(cap)
    } else {
        cur.wrapping_sub(cap)
    }
}

/// Compute the BAM heading from a delta vector via deterministic integer
/// arithmetic. 0 BAM = +x, 0x4000 = +y, 0x8000 = −x, 0xC000 = −y.
///
/// Reduces to the first octant via abs+min/max, looks up `atan(min/max)` in
/// `atan_lut`, then assembles the full circle from the quadrant signs and the
/// which-of-|x|,|y|-is-larger bit. The lookup is by integer ratio
/// `min * 65536 / max`, so no `f32` enters the per-tick computation.
pub(crate) fn atan2_bam(dy: SimFixed, dx: SimFixed) -> u16 {
    int_atan2_bam(dy.to_bits() as i64, dx.to_bits() as i64)
}

/// Integer atan2 → BAM. Inputs are arbitrary signed integers (Q-form is
/// irrelevant — atan2 depends only on the ratio).
fn int_atan2_bam(y: i64, x: i64) -> u16 {
    if x == 0 && y == 0 {
        return 0;
    }
    let ax = x.unsigned_abs();
    let ay = y.unsigned_abs();
    // First-octant angle: atan(min/max) in [0, 0x2000] BAM.
    let phi: u32 = if ay <= ax {
        atan_octant_bam(ay, ax) as u32
    } else {
        atan_octant_bam(ax, ay) as u32
    };
    // Quadrant base (each quadrant is 0x4000 BAM wide). Within each quadrant,
    // `in_high_half` selects which half of the quadrant the angle lies in — the
    // half closer to the next cardinal direction, where the BAM is computed by
    // subtracting `phi` from the next cardinal's BAM rather than adding it to
    // the quadrant base.
    let (base, in_high_half) = match (x >= 0, y >= 0) {
        (true, true) => (0x0000u32, ay > ax),
        (false, true) => (0x4000u32, ay <= ax),
        (false, false) => (0x8000u32, ay > ax),
        (true, false) => (0xC000u32, ay <= ax),
    };
    let bam = if in_high_half {
        base + 0x4000 - phi
    } else {
        base + phi
    };
    (bam & 0xFFFF) as u16
}

/// First-octant atan in BAM. `num` and `den` satisfy `0 <= num <= den`,
/// `den > 0`. Returns `round(atan(num/den) * 32768/π)` in `[0, 0x2000]`.
///
/// Uses the integer ratio `num * 65536 / den` (with round-to-nearest) as the
/// LUT index, so the only fp involved is in the one-shot table build.
fn atan_octant_bam(num: u64, den: u64) -> u16 {
    if den == 0 {
        return 0;
    }
    let scaled = num
        .checked_mul(65536)
        .expect("atan2 inputs exceed i48 range");
    let ratio = ((scaled + den / 2) / den).min(65536) as usize;
    atan_lut()[ratio]
}

/// First-octant atan LUT: `ATAN[r]` = `round(atan(r / 65536) * 32768/π)` for
/// `r ∈ [0, 65536]`. Output is BAM in `[0, 0x2000]` (u16). See
/// [`bam_cos_table`] for the platform-determinism rationale of the f64 init.
fn atan_lut() -> &'static [u16; 65537] {
    static TABLE: OnceLock<Box<[u16; 65537]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t: Box<[u16; 65537]> = vec![0u16; 65537]
            .into_boxed_slice()
            .try_into()
            .expect("65537-entry vec to fixed array");
        for i in 0..=65536u32 {
            let ratio = (i as f64) / 65536.0;
            let bam = (ratio.atan() * (32768.0 / std::f64::consts::PI)).round() as i32;
            t[i as usize] = bam.clamp(0, 0x2000) as u16;
        }
        t
    })
}

impl std::hash::Hash for HomingState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.phase.hash(state);
        self.target.hash(state);
        self.last_known_rx.hash(state);
        self.last_known_ry.hash(state);
        self.yaw_bam.hash(state);
        self.pitch_bam.hash(state);
        self.speed.to_bits().hash(state);
        self.pos_x_cells.to_bits().hash(state);
        self.pos_y_cells.to_bits().hash(state);
        self.altitude.to_bits().hash(state);
        self.vz.to_bits().hash(state);
        self.rot_ini.hash(state);
        self.missile_rot_var.to_bits().hash(state);
        self.floater.hash(state);
        self.very_high.hash(state);
        self.arm_ticks_remaining.hash(state);
        self.frame_counter.hash(state);
        self.stall_counter.hash(state);
        self.stall_ema.to_bits().hash(state);
        self.last_distance_to_target.to_bits().hash(state);
        // `pitch: f32` intentionally omitted — render-only.
    }
}

impl HomingState {
    /// Apply BulletClass's pointer-expired target branch.
    ///
    /// Normal gameplay has map-editor suppression disabled, so the retail split
    /// is ground object -> explicit cell, high-flying object -> null, and the
    /// coordinate sentinel -> null. That sentinel is the cell (0, 0): both
    /// `DAT_0089DDF0`/`DAT_0089DDF2` words are zero in the image and the
    /// program's only writer zeroes them again, so a `u16` entity cell does
    /// reach it and the gate applies here exactly as it does to a stored Bullet.
    ///
    /// gamemd-derived: `BulletClass::PointerExpired @ 0x004684E0` has exactly
    /// one target-repair arm, and it derives its replacement cell from the
    /// expiring object's own `ObjectClass::GetCoords` (primary vtable `+0x48`),
    /// truncated by the `(v + (v >> 31 & 0xFF)) >> 8` pair feeding
    /// `MapCoord_Set`. A missile modelled as an entity with `HomingState` and a
    /// missile stored in the projectile store are the same native Bullet, so
    /// both are fed that one derivation — `target_cell` here is the caller's
    /// `object_get_coords_cell`, which is what shifts a BuildingClass off its
    /// stored NW anchor onto the geometric foundation centre.
    ///
    /// `None` stands for a coordinate that does not resolve to a map cell at
    /// all. Native cannot reach it — `MapCoord_Set` always packs — so it is a
    /// VERA-internal arm, shared with the store side, which takes the same
    /// null-target outcome. It leaves the cached `last_known_*` steering coord
    /// alone: there is no cell to cache, and the missile is target-null from
    /// that point on.
    pub(crate) fn expire_object_target(
        &mut self,
        expired_id: u64,
        target_cell: Option<(u16, u16)>,
        target_is_high_flying: bool,
    ) -> bool {
        if self.target != Some(HomingTarget::Object(expired_id)) {
            return false;
        }

        if let Some((rx, ry)) = target_cell {
            self.last_known_rx = rx;
            self.last_known_ry = ry;
        }
        self.target = match target_cell {
            Some(cell)
                if !target_is_high_flying
                    && cell != crate::sim::world::NULL_TARGET_CELL_SENTINEL =>
            {
                Some(HomingTarget::Cell {
                    rx: cell.0,
                    ry: cell.1,
                })
            }
            _ => None,
        };
        true
    }
}

/// Attach a homing missile state to an entity at the given origin, targeting
/// `target_id`. The entity should already exist in the EntityStore with a
/// position. Returns `false` if the entity doesn't exist.
///
/// Parameters:
/// - `weapon_speed`: from `WeaponType.Speed`
/// - `rot_ini`: from `BulletType.ROT` (raw INI int, NOT pre-scaled)
/// - `arm_frames`: from `BulletType.Arm`
/// - `floater`, `very_high`: from `BulletType`
/// - `missile_rot_var`: from `[General].MissileROTVar` (stock .25; missing-key fallback 1.0)
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn attach_homing_state(
    entities: &mut EntityStore,
    bullet_id: u64,
    origin: (u16, u16),
    target_id: u64,
    target_pos: (u16, u16),
    weapon_speed: SimFixed,
    rot_ini: u16,
    arm_frames: u16,
    floater: bool,
    very_high: bool,
    missile_rot_var: SimFixed,
) -> bool {
    let Some(entity) = entities.get_mut(bullet_id) else {
        return false;
    };

    let initial_yaw_bam = atan2_bam(
        SimFixed::from_num(target_pos.1 as i32 - origin.1 as i32),
        SimFixed::from_num(target_pos.0 as i32 - origin.0 as i32),
    );
    // Seed last_distance_to_target with the true starting distance in
    // leptons so the first tick's `prev - now` delta is ~zero rather than
    // a huge negative spike (which would poison the stall EMA before any
    // real motion happens).
    let initial_dist_sf = int_distance_to_sim(
        (target_pos.0 as i32 - origin.0 as i32) * 256,
        (target_pos.1 as i32 - origin.1 as i32) * 256,
    );

    entity.homing_state = Some(HomingState {
        phase: if arm_frames > 0 {
            HomingPhase::Arming
        } else {
            HomingPhase::Cruise
        },
        target: Some(HomingTarget::Object(target_id)),
        last_known_rx: target_pos.0,
        last_known_ry: target_pos.1,
        yaw_bam: initial_yaw_bam,
        pitch_bam: 0x4000, // 90° BAM = horizontal at start
        speed: weapon_speed.max(SIM_ONE),
        pos_x_cells: SimFixed::from_num(origin.0 as i32),
        pos_y_cells: SimFixed::from_num(origin.1 as i32),
        altitude: SIM_ZERO,
        vz: SIM_ZERO,
        rot_ini,
        missile_rot_var,
        floater,
        very_high,
        arm_ticks_remaining: arm_frames,
        frame_counter: 0,
        stall_counter: 0,
        stall_ema: SIM_ZERO,
        last_distance_to_target: initial_dist_sf,
        pitch: 0.0,
    });
    true
}

/// Advance all in-flight homing missile state machines.
///
/// Called once per simulation tick from `World::advance_tick` in the
/// "air + special movement" phase, after `tick_rocket_movement`.
///
/// Returns the list of entity IDs that detonated this tick (impact or
/// stall self-destruct). The caller is responsible for damage dispatch
/// and despawn.
pub fn tick_homing_movement(
    entities: &mut EntityStore,
    live_order: &[u64],
    _sim_tick: u64,
) -> Vec<u64> {
    let mut detonated: Vec<u64> = Vec::new();
    let dt = native_movement_frame_fraction();
    let fallback_order;
    let entity_order: &[u64] = if live_order.is_empty() {
        fallback_order = entities.keys_sorted();
        &fallback_order
    } else {
        live_order
    };
    for &id in entity_order {
        // Read object/cell target position without holding a mutable borrow on
        // the bullet.
        let (target_pos_opt, lost_object_target): (Option<(u16, u16)>, bool) = {
            let Some(bullet) = entities.get(id) else {
                continue;
            };
            let Some(h) = bullet.homing_state.as_ref() else {
                continue;
            };
            match h.target {
                Some(HomingTarget::Object(target_id)) => (
                    entities
                        .get(target_id)
                        .map(|target| (target.position.rx, target.position.ry)),
                    !entities.contains(target_id),
                ),
                Some(HomingTarget::Cell { rx, ry }) => (Some((rx, ry)), false),
                None => (None, false),
            }
        };

        let Some(bullet) = entities.get_mut(id) else {
            continue;
        };
        let Some(h) = bullet.homing_state.as_mut() else {
            continue;
        };

        // 1. Refresh object/cell target coordinates. A hard-removed object is
        // only a safety fallback; the ordinary retail path converts it during
        // the earlier UnInit pointer-expired notification.
        if let Some(pos) = target_pos_opt {
            h.last_known_rx = pos.0;
            h.last_known_ry = pos.1;
        } else if lost_object_target {
            h.target = None;
        }

        // 2. Desired yaw from current cell-precision pos -> last-known.
        let dx_cells = SimFixed::from_num(h.last_known_rx as i32) - h.pos_x_cells;
        let dy_cells = SimFixed::from_num(h.last_known_ry as i32) - h.pos_y_cells;
        let desired_yaw = atan2_bam(dy_cells, dx_cells);

        // 3. Sidewinder ROT modulation: 1 + var + cos(2π * frame / 15) * var,
        //    yielding the [1, 1+2*var] range. The truncation-to-byte step
        //    matches the original's LowByte(ftol(...)) << 8 shape.
        let sidewinder =
            sidewinder_cos(h.frame_counter) * h.missile_rot_var + h.missile_rot_var + SIM_ONE;
        let delta_far_sf: SimFixed = sidewinder * SimFixed::from_num(h.rot_ini as i32);
        let delta_far: i32 = delta_far_sf.to_num::<i32>();

        // 4. Close-range branch: when within one cell, use a frame-counter
        //    driven step instead of the sidewinder magnitude. Avoids
        //    over-rotating the final approach.
        let dx_int = dx_cells.to_num::<i32>().abs();
        let dy_int = dy_cells.to_num::<i32>().abs();
        let close_range = (dx_int + dy_int) <= 1;
        let delta_int: i32 = if close_range {
            ((h.frame_counter % 15) as i32 * 3) / 2
        } else {
            delta_far
        };

        // 5. ROT_BAM per tick = LowByte(delta) << 8 (matches original shift).
        let delta_byte: u8 = (delta_int as u32 & 0xFF) as u8;
        let rot_bam_per_tick: u16 = (delta_byte as u16) << 8;

        // 6. Yaw step with inclusive snap.
        h.yaw_bam = step_toward_bam_inclusive(h.yaw_bam, desired_yaw, rot_bam_per_tick);

        // 7. Horizontal velocity from yaw + speed. Sin/cos via deterministic
        //    BAM-indexed Q16.16 table — no f32 in the sim hot path so the
        //    integration into hashed `pos_x/y_cells` is lockstep-safe.
        let v_cells_this_tick = h.speed * dt;
        let vx_sf = v_cells_this_tick * cos_bam(h.yaw_bam);
        let vy_sf = v_cells_this_tick * sin_bam(h.yaw_bam);

        // 8. Integrate sub-cell position, then write truncated cell back to
        //    the entity's render-visible position.
        h.pos_x_cells += vx_sf;
        h.pos_y_cells += vy_sf;
        let new_rx = h.pos_x_cells.to_num::<i32>().clamp(0, u16::MAX as i32) as u16;
        let new_ry = h.pos_y_cells.to_num::<i32>().clamp(0, u16::MAX as i32) as u16;
        bullet.position.rx = new_rx;
        bullet.position.ry = new_ry;

        // 9. vz damper: non-Floater missiles decay vertical velocity each
        //    tick toward 0 (`(vz + 3*sgn(vz)) / 4` rounds toward 0 by 1/4).
        if !h.floater {
            let signum: i32 = if h.vz > SIM_ZERO {
                1
            } else if h.vz < SIM_ZERO {
                -1
            } else {
                0
            };
            h.vz = (h.vz + SimFixed::from_num(signum * 3)) / SimFixed::from_num(4);
        }

        // 9b. Cruise altitude controller. Drives `altitude` toward the
        //     band determined by the projectile flags. Floater/VeryHigh
        //     missiles cruise at ~10 cells of altitude (640 leptons);
        //     normal homing missiles cruise low at ~1 cell (320 leptons).
        //     Dead-band ±20 leptons -> no snap. Outside the band, snap by
        //     18 leptons toward the band. Pitch BAM follows dz sign.
        let high_alt_branch = h.altitude > SimFixed::from_num(3 * 256) || h.rot_ini > 1;
        if high_alt_branch {
            let target_alt_leptons: i32 = if h.floater || h.very_high {
                10 * 64
            } else {
                5 * 64
            };
            let self_alt: i32 = h.altitude.to_num::<i32>();
            let dz: i32 = self_alt - target_alt_leptons;

            if dz.abs() > 20 {
                let snap: i32 = if dz > 0 { -18 } else { 18 };
                h.altitude = SimFixed::from_num((self_alt + snap).max(0));
            }

            let pitch_target: u16 = if dz < -32 {
                0x2000 // tilt up
            } else if dz > 32 {
                0x4800 // tilt down
            } else {
                0x4000 // level off
            };
            let pitch_step: u16 = rot_bam_per_tick / 2;
            h.pitch_bam = step_toward_bam_inclusive(h.pitch_bam, pitch_target, pitch_step);
        }

        // 10. Arm decrement -> Cruise.
        if h.arm_ticks_remaining > 0 {
            h.arm_ticks_remaining -= 1;
            if h.arm_ticks_remaining == 0 && h.phase == HomingPhase::Arming {
                h.phase = HomingPhase::Cruise;
            }
        }

        // 11. Detonation proximity: sub-cell lepton distance computed from
        //     the SimFixed position fields, not the truncated bullet.position
        //     integers. Cell-grid match alone is too strict — the missile's
        //     sub-cell pos may orbit a target's cell without ever landing on
        //     an exact (rx, ry).
        let dx_lep_sf =
            (SimFixed::from_num(h.last_known_rx as i32) - h.pos_x_cells) * SimFixed::from_num(256);
        let dy_lep_sf =
            (SimFixed::from_num(h.last_known_ry as i32) - h.pos_y_cells) * SimFixed::from_num(256);
        let dist_now_sf: SimFixed =
            int_distance_to_sim(dx_lep_sf.to_num::<i32>(), dy_lep_sf.to_num::<i32>());
        // 192 leptons = three-quarters of a cell; tight enough that the
        // missile is visually on top of the target and loose enough to
        // absorb sub-cell drift from f32 cos/sin in the velocity ramp.
        let proximity_hit = dist_now_sf <= SimFixed::from_num(192) && h.arm_ticks_remaining == 0;
        if proximity_hit {
            h.phase = HomingPhase::Detonation;
            detonated.push(id);
        }

        // 12. Stall detection failsafe. After 60 frames of warm-up where we
        //     just accumulate raw delta-distance, switch to EMA smoothing.
        //     If the smoothed closure rate falls below 0.5 leptons/tick,
        //     the missile is judged unreachable and self-destructs.
        //     Non-Floater only — floater projectiles can hover indefinitely.
        //
        //     Distance is measured in leptons (256 leptons per cell), matching
        //     the original engine's distance scale. A normal native-frame
        //     closing rate is hundreds of leptons, so the 0.5-lepton threshold
        //     only trips when the missile is truly stalled.
        if h.phase != HomingPhase::Detonation {
            let delta_dist = h.last_distance_to_target - dist_now_sf;
            h.last_distance_to_target = dist_now_sf;

            if h.stall_counter < 60 {
                h.stall_counter += 1;
                h.stall_ema += delta_dist;
            } else {
                h.stall_ema =
                    h.stall_ema * SimFixed::lit("0.9") + delta_dist * SimFixed::lit("0.1");
                if h.stall_ema <= SimFixed::lit("0.5") && !h.floater {
                    h.phase = HomingPhase::SelfDestruct;
                    detonated.push(id);
                }
            }
        }

        h.frame_counter = h.frame_counter.wrapping_add(1);
    }

    detonated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidewinder_table_min_max() {
        let max = SIDEWINDER_TABLE
            .iter()
            .copied()
            .fold(SimFixed::from_num(-2), SimFixed::max);
        let min = SIDEWINDER_TABLE
            .iter()
            .copied()
            .fold(SimFixed::from_num(2), SimFixed::min);
        assert!(max <= SimFixed::from_num(1));
        assert!(max >= SimFixed::lit("0.9"));
        assert!(min <= SimFixed::lit("-0.9"));
        assert!(min >= SimFixed::from_num(-1));
    }

    #[test]
    fn sidewinder_cos_wraps_at_15() {
        assert_eq!(sidewinder_cos(0), sidewinder_cos(15));
        assert_eq!(sidewinder_cos(7), sidewinder_cos(22));
        assert_eq!(sidewinder_cos(0), SimFixed::from_num(1));
    }

    #[test]
    fn cos_bam_cardinal_angles() {
        // 0 BAM = 0 rad: cos = 1
        assert_eq!(cos_bam(0), SIM_ONE);
        // 0x4000 BAM = π/2: cos ≈ 0
        let c = cos_bam(0x4000);
        assert!(c.abs() <= SimFixed::from_bits(2), "cos(π/2) = {:?}", c);
        // 0x8000 BAM = π: cos = -1
        assert_eq!(cos_bam(0x8000), SimFixed::from_num(-1));
        // 0xC000 BAM = 3π/2: cos ≈ 0
        let c = cos_bam(0xC000);
        assert!(c.abs() <= SimFixed::from_bits(2), "cos(3π/2) = {:?}", c);
    }

    #[test]
    fn sin_bam_cardinal_angles() {
        // 0 BAM: sin = 0
        assert_eq!(sin_bam(0), SIM_ZERO);
        // 0x4000 BAM = π/2: sin = 1
        assert_eq!(sin_bam(0x4000), SIM_ONE);
        // 0x8000 BAM = π: sin ≈ 0
        let s = sin_bam(0x8000);
        assert!(s.abs() <= SimFixed::from_bits(2), "sin(π) = {:?}", s);
        // 0xC000 BAM = 3π/2: sin = -1
        assert_eq!(sin_bam(0xC000), SimFixed::from_num(-1));
    }

    #[test]
    fn bam_trig_pythagorean_identity() {
        // cos² + sin² ≈ 1 for arbitrary BAMs. Q16.16 rounding budget is a
        // few ULPs per multiplication.
        for &bam in &[0u16, 0x1234, 0x2BCD, 0x4001, 0x7FFF, 0xABCD, 0xFFFF] {
            let c = cos_bam(bam);
            let s = sin_bam(bam);
            let sum = c * c + s * s;
            let err = (sum - SIM_ONE).abs();
            assert!(
                err <= SimFixed::lit("0.001"),
                "bam={:#06X}: cos²+sin² = {:?} (err {:?})",
                bam,
                sum,
                err
            );
        }
    }

    #[test]
    fn bam_trig_repeatable() {
        // Trivially true for a table lookup, but worth asserting so future
        // refactors that swap the impl out don't silently lose determinism.
        for bam in (0..65535u16).step_by(7919) {
            assert_eq!(cos_bam(bam), cos_bam(bam));
            assert_eq!(sin_bam(bam), sin_bam(bam));
        }
    }

    #[test]
    fn within_rot_bam_inclusive_at_boundary() {
        // At exact ROT distance, snap (inclusive `<=`).
        assert!(within_rot_bam(0x0000, 0x0100, 0x0100));
        assert!(within_rot_bam(0x0100, 0x0000, 0x0100));
        // One past the cap — no snap.
        assert!(!within_rot_bam(0x0000, 0x0101, 0x0100));
    }

    #[test]
    fn step_toward_bam_inclusive_snaps_at_cap() {
        // Exactly at cap distance -> snap.
        assert_eq!(step_toward_bam_inclusive(0x0000, 0x0100, 0x0100), 0x0100);
    }

    #[test]
    fn step_toward_bam_inclusive_steps_outside_cap() {
        // Beyond cap -> step by cap toward target.
        assert_eq!(step_toward_bam_inclusive(0x0000, 0x0200, 0x0100), 0x0100);
        assert_eq!(step_toward_bam_inclusive(0x0000, 0xFE00, 0x0100), 0xFF00);
    }

    #[test]
    fn step_toward_bam_wraps_around() {
        // Shortest arc across the wrap (going CCW is closer).
        assert_eq!(step_toward_bam_inclusive(0x0000, 0xFF00, 0x0100), 0xFF00);
    }

    #[test]
    fn attach_homing_state_initializes() {
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));

        let attached = attach_homing_state(
            &mut entities,
            /* bullet_id */ 1,
            /* origin */ (5, 5),
            /* target_id */ 42,
            /* target_pos */ (15, 5),
            /* weapon_speed */ SimFixed::from_num(30),
            /* rot_ini */ 60,
            /* arm_frames */ 2,
            /* floater */ false,
            /* very_high */ false,
            /* missile_rot_var */ SimFixed::from_num(1),
        );
        assert!(attached);

        let h = entities.get(1).unwrap().homing_state.as_ref().unwrap();
        assert_eq!(h.phase, HomingPhase::Arming);
        assert_eq!(h.target, Some(HomingTarget::Object(42)));
        assert_eq!(h.last_known_rx, 15);
        assert_eq!(h.last_known_ry, 5);
        assert_eq!(h.arm_ticks_remaining, 2);
        // Initial yaw ~ +x toward target.
        assert!(h.yaw_bam < 8 || h.yaw_bam > 0xFFF8);
        // Speed floor 1.
        assert!(h.speed >= SIM_ONE);
    }

    #[test]
    fn attach_homing_state_zero_arm_starts_in_cruise() {
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(2, "PROJ", "Allied", 0, 0));
        attach_homing_state(
            &mut entities,
            2,
            (0, 0),
            99,
            (10, 0),
            SimFixed::from_num(20),
            48,
            0, // arm_frames=0 -> Cruise immediately
            false,
            false,
            SIM_ONE,
        );
        let h = entities.get(2).unwrap().homing_state.as_ref().unwrap();
        assert_eq!(h.phase, HomingPhase::Cruise);
        assert_eq!(h.arm_ticks_remaining, 0);
    }

    #[test]
    fn homing_missile_reaches_static_target() {
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(42, "KIROV", "Soviet", 25, 5));
        entities.insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));

        attach_homing_state(
            &mut entities,
            1,
            (5, 5),
            42,
            (25, 5),
            SimFixed::from_num(30),
            60,
            0,
            false,
            false,
            SIM_ONE,
        );

        let mut detonated = false;
        for _ in 0..200 {
            let det = tick_homing_movement(&mut entities, &[], 0);
            if det.contains(&1) {
                detonated = true;
                break;
            }
        }
        assert!(
            detonated,
            "homing missile should detonate when reaching static target"
        );
    }

    /// Spawn a missile with a target far enough away that the cruise branch
    /// activates (rot_ini > 1) and at a controlled initial altitude.
    fn spawn_test_homing_at_altitude(altitude: SimFixed) -> (EntityStore, u64) {
        use crate::sim::game_entity::GameEntity;
        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(42, "KIROV", "Soviet", 25, 5));
        entities.insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));
        attach_homing_state(
            &mut entities,
            1,
            (5, 5),
            42,
            (25, 5),
            SimFixed::from_num(30),
            60,
            0,
            false,
            false,
            SIM_ONE,
        );
        if let Some(h) = entities.get_mut(1).unwrap().homing_state.as_mut() {
            h.altitude = altitude;
        }
        (entities, 1)
    }

    #[test]
    fn cruise_dead_band_no_snap() {
        // Cruise target = 5*64 = 320 leptons. Start at 320 + 10 leptons (|dz|=10).
        // Inside dead-band -> altitude unchanged by the snap step.
        let (mut entities, bullet_id) =
            spawn_test_homing_at_altitude(SimFixed::from_num(5 * 64 + 10));
        tick_homing_movement(&mut entities, &[], 0);
        let alt_after = entities
            .get(bullet_id)
            .unwrap()
            .homing_state
            .as_ref()
            .unwrap()
            .altitude
            .to_num::<i32>();
        assert!(
            (alt_after - (5 * 64 + 10)).abs() <= 1,
            "dead-band should keep altitude near 330, got {}",
            alt_after
        );
    }

    #[test]
    fn cruise_outside_dead_band_snaps_by_18() {
        // |dz| = 30 -> outside dead-band -> snap by -18 toward target.
        let (mut entities, bullet_id) =
            spawn_test_homing_at_altitude(SimFixed::from_num(5 * 64 + 30));
        tick_homing_movement(&mut entities, &[], 0);
        let alt_after = entities
            .get(bullet_id)
            .unwrap()
            .homing_state
            .as_ref()
            .unwrap()
            .altitude
            .to_num::<i32>();
        let delta = (5 * 64 + 30) - alt_after;
        assert_eq!(delta, 18, "expected snap of 18 leptons toward cruise band");
    }

    #[test]
    fn cruise_below_dead_band_snaps_up_by_18() {
        // |dz| = 30 below target -> snap up by 18.
        let (mut entities, bullet_id) =
            spawn_test_homing_at_altitude(SimFixed::from_num(5 * 64 - 30));
        tick_homing_movement(&mut entities, &[], 0);
        let alt_after = entities
            .get(bullet_id)
            .unwrap()
            .homing_state
            .as_ref()
            .unwrap()
            .altitude
            .to_num::<i32>();
        let delta = alt_after - (5 * 64 - 30);
        assert_eq!(
            delta, 18,
            "expected snap of 18 leptons up toward cruise band"
        );
    }

    #[test]
    fn stall_detect_self_destructs_when_ema_falls_below_threshold() {
        // Unit-test the EMA threshold gate directly: synthesise a homing
        // state already past warm-up, with EMA at 0 and the missile parked
        // (zero speed, zero closure). One tick should trip the gate.
        //
        // Forcing the scenario via state mutation is necessary because the
        // production speed floor (SimFixed::ONE) leaves even a zero-INI
        // missile closing at ~5.6 leptons/tick, well above the 0.5-lepton
        // EMA threshold — there's no natural way for a single missile to
        // genuinely stall in the air without world-state interference.
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(42, "KIROV", "Soviet", 25, 5));
        entities.insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));
        attach_homing_state(
            &mut entities,
            1,
            (5, 5),
            42,
            (25, 5),
            SimFixed::from_num(20),
            60,
            0, // arm=0 -> phase starts in Cruise, no decrement needed
            false,
            false,
            SIM_ONE,
        );

        // Synthesise a fully-warmed-up, stalled missile parked on the
        // target. ema=0 and last_distance_to_target=0 mean the next tick's
        // delta_dist is 0 → ema stays at 0 → threshold trips.
        if let Some(h) = entities.get_mut(1).unwrap().homing_state.as_mut() {
            h.phase = HomingPhase::Cruise;
            h.stall_ema = SIM_ZERO;
            h.stall_counter = 60;
            h.speed = SIM_ZERO;
            h.last_distance_to_target = SIM_ZERO;
            // Park the bullet beyond proximity range so proximity_hit
            // doesn't fire first — stall must be what self-destructs it.
            h.pos_x_cells = SimFixed::from_num(0);
            h.pos_y_cells = SimFixed::from_num(0);
            h.last_known_rx = 100;
            h.last_known_ry = 100;
        }

        let det = tick_homing_movement(&mut entities, &[], 1);
        assert!(
            det.contains(&1),
            "missile with EMA<=0.5 and zero closure must self-destruct"
        );
        let h = entities.get(1).unwrap().homing_state.as_ref().unwrap();
        assert_eq!(h.phase, HomingPhase::SelfDestruct);
    }

    #[test]
    fn stall_detect_does_not_fire_for_floater() {
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(42, "KIROV", "Soviet", 25, 5));
        entities.insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));
        attach_homing_state(
            &mut entities,
            1,
            (5, 5),
            42,
            (25, 5),
            SimFixed::lit("0.01"),
            60,
            0,
            /* floater */ true,
            false,
            SIM_ONE,
        );

        // Isolate the stall gate exactly as the non-Floater sibling test does.
        // A long free-flight loop can legitimately reach proximity detonation,
        // which says nothing about whether the stall failsafe fired.
        if let Some(h) = entities.get_mut(1).unwrap().homing_state.as_mut() {
            h.phase = HomingPhase::Cruise;
            h.stall_ema = SIM_ZERO;
            h.stall_counter = 60;
            h.speed = SIM_ZERO;
            h.last_distance_to_target = SIM_ZERO;
            h.pos_x_cells = SimFixed::from_num(0);
            h.pos_y_cells = SimFixed::from_num(0);
            h.last_known_rx = 100;
            h.last_known_ry = 100;
        }

        let det = tick_homing_movement(&mut entities, &[], 1);
        assert!(
            !det.contains(&1),
            "Floater must bypass the warmed-up stall self-destruct gate"
        );
        assert_eq!(
            entities
                .get(1)
                .unwrap()
                .homing_state
                .as_ref()
                .unwrap()
                .phase,
            HomingPhase::Cruise
        );
    }

    #[test]
    fn attach_homing_state_missing_entity_returns_false() {
        let mut entities = EntityStore::new();
        let attached = attach_homing_state(
            &mut entities,
            999,
            (0, 0),
            1,
            (10, 0),
            SimFixed::from_num(20),
            48,
            0,
            false,
            false,
            SIM_ONE,
        );
        assert!(!attached);
    }

    #[test]
    fn homing_detonations_return_in_live_object_order_not_stable_id() {
        use crate::sim::game_entity::GameEntity;

        fn build_entities() -> EntityStore {
            let mut entities = EntityStore::new();
            entities.insert(GameEntity::test_default(10, "KIROV", "Soviet", 5, 5));
            entities.insert(GameEntity::test_default(20, "KIROV", "Soviet", 6, 5));
            entities.insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));
            entities.insert(GameEntity::test_default(2, "AAHeatSeeker2", "Allied", 6, 5));
            attach_homing_state(
                &mut entities,
                1,
                (5, 5),
                10,
                (5, 5),
                SIM_ONE,
                60,
                0,
                false,
                false,
                SIM_ONE,
            );
            attach_homing_state(
                &mut entities,
                2,
                (6, 5),
                20,
                (6, 5),
                SIM_ONE,
                60,
                0,
                false,
                false,
                SIM_ONE,
            );
            for id in [1, 2] {
                let h = entities.get_mut(id).unwrap().homing_state.as_mut().unwrap();
                h.speed = SIM_ZERO;
            }
            entities
        }

        let mut live_entities = build_entities();
        let mut stable_entities = build_entities();

        let live = tick_homing_movement(&mut live_entities, &[2, 1], 0);
        let stable = tick_homing_movement(&mut stable_entities, &[], 0);

        assert_eq!(
            live,
            vec![2, 1],
            "live object order controls homing detonation order"
        );
        assert_eq!(
            stable,
            vec![1, 2],
            "empty live order preserves prior stable-id fallback"
        );
    }

    #[test]
    fn homing_missile_tracks_moving_target() {
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(42, "KIROV", "Soviet", 30, 5));
        entities.insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));

        attach_homing_state(
            &mut entities,
            1,
            (5, 5),
            42,
            (30, 5),
            SimFixed::from_num(20),
            60,
            0,
            false,
            false,
            SIM_ONE,
        );

        // After tick 10, jog the target off-axis so the missile must
        // re-yaw mid-flight. A non-homing rocket flying a straight line
        // toward (30, 5) would miss the new (35, 10) target — only the
        // sidewinder yaw correction can pursue.
        let mut detonated = false;
        for tick in 0..400 {
            if tick == 10 {
                let t = entities.get_mut(42).unwrap();
                t.position.rx = 35;
                t.position.ry = 10;
            }
            let det = tick_homing_movement(&mut entities, &[], tick as u64);
            if det.contains(&1) {
                detonated = true;
                break;
            }
        }
        assert!(
            detonated,
            "missile should track the moved target and detonate"
        );

        // After detonation the missile must be at (or very near) the moved
        // target's sub-cell position, not the original launch trajectory's
        // endpoint. Cell-grid match is too strict — proximity detonation
        // fires at sub-cell distance ≤ 192 leptons (~0.75 cell), so the
        // truncated bullet.position may be one cell short of the target.
        let bullet = entities.get(1).unwrap();
        let h = bullet.homing_state.as_ref().unwrap();
        let dx_sf = h.pos_x_cells - SimFixed::from_num(35);
        let dy_sf = h.pos_y_cells - SimFixed::from_num(10);
        let dist_sq = dx_sf * dx_sf + dy_sf * dy_sf;
        // 0.75 cells -> dist <= 0.5625 cells²; we expect well under that.
        assert!(
            dist_sq <= SimFixed::lit("1.0"),
            "missile should detonate within one cell of moved target (35, 10), \
             got sub-cell pos ({:?}, {:?})",
            h.pos_x_cells,
            h.pos_y_cells
        );
        // And the trajectory must have curved south-east, not stayed on the
        // pure +x flight path toward the original (30, 5) target. ry >= 8
        // is impossible without re-yawing south after the target moved.
        assert!(
            h.pos_y_cells >= SimFixed::from_num(8),
            "missile y-coord should reflect the southward re-yaw, got {:?}",
            h.pos_y_cells
        );
    }

    #[test]
    fn atan2_bam_cardinal_directions() {
        // Exact-cardinal inputs must hit exact-cardinal BAM with no slack.
        assert_eq!(atan2_bam(SimFixed::from_num(0), SimFixed::from_num(1)), 0); // +x
        assert_eq!(
            atan2_bam(SimFixed::from_num(1), SimFixed::from_num(0)),
            0x4000 // +y
        );
        assert_eq!(
            atan2_bam(SimFixed::from_num(0), SimFixed::from_num(-1)),
            0x8000 // -x
        );
        assert_eq!(
            atan2_bam(SimFixed::from_num(-1), SimFixed::from_num(0)),
            0xC000 // -y
        );
    }

    #[test]
    fn atan2_bam_diagonal_directions() {
        // Equal-magnitude diagonals must land on the octant midpoints.
        assert_eq!(
            atan2_bam(SimFixed::from_num(1), SimFixed::from_num(1)),
            0x2000 // NE in math = +x +y at 45°
        );
        assert_eq!(
            atan2_bam(SimFixed::from_num(1), SimFixed::from_num(-1)),
            0x6000
        );
        assert_eq!(
            atan2_bam(SimFixed::from_num(-1), SimFixed::from_num(-1)),
            0xA000
        );
        assert_eq!(
            atan2_bam(SimFixed::from_num(-1), SimFixed::from_num(1)),
            0xE000
        );
    }

    #[test]
    fn atan2_bam_zero_zero_is_zero() {
        assert_eq!(atan2_bam(SIM_ZERO, SIM_ZERO), 0);
    }

    #[test]
    fn atan2_bam_matches_f64_reference() {
        // Sample 32 evenly-spaced cell deltas around the unit circle and
        // check the integer LUT result is within ±1 BAM of an f64 reference.
        // ±1 BAM tolerance covers the rounding budget of the table build.
        for n in 0..32 {
            let angle = (n as f64) * (2.0 * std::f64::consts::PI / 32.0);
            // Scale up to avoid the small-magnitude rounding band — atan2 is
            // ratio-only so any radius works.
            let dx = (angle.cos() * 1024.0) as i32;
            let dy = (angle.sin() * 1024.0) as i32;
            if dx == 0 && dy == 0 {
                continue;
            }
            let got = atan2_bam(SimFixed::from_num(dy), SimFixed::from_num(dx));
            let expected = (((dy as f64).atan2(dx as f64) * 32768.0 / std::f64::consts::PI) as i32)
                .rem_euclid(65536) as u16;
            let diff = (got as i32 - expected as i32).rem_euclid(65536);
            let signed_diff = diff.min(65536 - diff);
            assert!(
                signed_diff <= 1,
                "angle={:.2}rad (dx={},dy={}): got 0x{:04X}, expected 0x{:04X}",
                angle,
                dx,
                dy,
                got,
                expected
            );
        }
    }

    #[test]
    fn atan2_bam_sub_cell_inputs() {
        // SimFixed deltas can be sub-cell. Verify the LUT handles them by
        // their Q16.16 bit representation rather than truncating to integers.
        // (1.5, 0) → +x → 0 BAM
        assert_eq!(atan2_bam(SimFixed::lit("0"), SimFixed::lit("1.5")), 0);
        // (0.5, 0.5) → 45° → 0x2000 BAM
        assert_eq!(
            atan2_bam(SimFixed::lit("0.5"), SimFixed::lit("0.5")),
            0x2000
        );
    }
}
