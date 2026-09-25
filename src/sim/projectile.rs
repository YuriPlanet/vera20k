//! Authoritative in-flight projectile state.
//!
//! This module deliberately owns only delayed flight, target expiry, collision,
//! and detonation admission. Combat remains the authority for turning a returned
//! [`ProjectileDetonation`] into damage, warhead effects, and terrain changes.
//! Keeping that handoff narrow lets the world phase place this system at the
//! verified native frame rung without duplicating combat arithmetic here.
//!
//! ## `BulletClass::AI @ 0x004666E0` — the three flight arms
//!
//! Native selects an arm exactly twice, at `0x004668D1` on `ROT < 1` and then
//! at `0x004671D0` on `Vertical` (`BulletTypeClass+0x2C0`). Nothing else takes
//! part: `Arcing`, `SubjectToCliffs`, `SubjectToElevation`, `Proximity`,
//! `FlakScatter`, `Inviso` and `Cluster` are launch-time or detonation-time
//! keys. [`ProjectileTrajectory`] carries the same three arms.
//!
//! Detonation admission at `0x00467C70` is `impact || (!Dropping && fuse != 0)`
//! and contains NO `Arm=` test — `Arm=` is written once into the bullet's
//! embedded `ProximityDetector` at `BulletClass::Fire 0x00468A5D` and gates
//! that one fuse result. A collision, a ground or bridge contact, a
//! `DetonationAltitude` crossing, or a target reach all detonate an unarmed
//! bullet.
//!
//! Homing termination reads the OLD ObjectClass height, before committing the
//! new coordinate (`0x00466DF6`). Ground admission suppresses the arm's target
//! snap; the common tail then clamps a negative committed height and may snap
//! a mode-1 fuse to the target's location (`+0x48`, not its aim point `+0x58`).
//!
//! Ordinary admission (467494..467B7A) and the common probe (468BB0/4CC360)
//! read production world state through world::projectile_collision. Final
//! target adjustment (467CEC) runs only after admission. There is no separate
//! distance-to-target expiry rule for ordinary shots.
//!
//! Ordinary and Vertical flight retain one binary64 velocity authority.
//! `launch` owns the native scalar FireAt math; combat resolves its receivers.
//! RESIDUAL (GSI-08.06/07): FLH/pivot slope translation, directed Building
//! heading, homing launch/steering, the flight of `Inviso=` shrapnel children
//! (native places them at their target, `BulletClass::Fire @ 0x00468670`) and
//! active NukeMaker child production remain open. Those producers can still change the inputs delivered to this exact
//! motion/collision consumer; the complete projectile row remains open.

pub(crate) mod launch;

use std::collections::BTreeMap;

use crate::map::resolved_terrain::{ResolvedTerrainGrid, SharedCellDummy};
use crate::sim::intern::InternedId;
use crate::sim::movement::homing_movement::{
    atan2_bam, cos_bam, sidewinder_cos, sin_bam, step_toward_bam_inclusive,
};
use crate::sim::rng::SimRng;
use crate::util::fixed_math::SimFixed;

/// Lepton-space position for an in-flight projectile.
///
/// Cells contain 256 leptons. Keeping the flight state in signed integer
/// leptons avoids float math and makes the serialized state independent of
/// render coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ProjectileCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Native Bullet +E8/+F0/+F8 binary64 velocity, in leptons per frame.
///
/// Fire 468691 copies all six DWORDs, ordinary AI 467AB2 copies its scratch
/// doubles back, and Vertical 4672A3 scales this same persistent vector.
/// Integer coordinates are projections of this authority, never a second state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ProjectileVelocity {
    pub x: crate::util::native_x87::NativeF64Bits,
    pub y: crate::util::native_x87::NativeF64Bits,
    pub z: crate::util::native_x87::NativeF64Bits,
}

impl ProjectileVelocity {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        use crate::util::native_x87::NativeF64Bits;
        // Every signed i32 is exactly representable as binary64.
        Self {
            x: NativeF64Bits::from_bits((x as f64).to_bits()),
            y: NativeF64Bits::from_bits((y as f64).to_bits()),
            z: NativeF64Bits::from_bits((z as f64).to_bits()),
        }
    }

    pub const fn from_native(components: [crate::util::native_x87::NativeF64Bits; 3]) -> Self {
        Self {
            x: components[0],
            y: components[1],
            z: components[2],
        }
    }

    pub const fn native(self) -> [crate::util::native_x87::NativeF64Bits; 3] {
        [self.x, self.y, self.z]
    }

    pub fn integer_projection(self) -> ProjectileCoord {
        let [x, y, z] = ProjectileCollisionMotion::quantized(self.native());
        ProjectileCoord::new(x, y, z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ProjectileTrajectory {
    Straight,
    Ballistic,
    /// gamemd-derived: `BulletClass::AI @ 0x004666E0`, the `ROT < 1 &&
    /// Vertical` arm entered at `0x004671E0`. No gravity is applied there and
    /// the launch direction is never changed — only the magnitude ramps by
    /// `Acceleration=` (`BulletTypeClass+0x2D0`) toward `MaxSpeed`
    /// (`Bullet+0x110`), and flight ends on `z > DetonationAltitude`
    /// (`+0x2BC`, read at `0x00467334`), a negative altitude, or a bridge-deck
    /// crossing.
    ///
    /// Direction lives only in the persistent binary64 velocity. The native
    /// ramp normalizes and scales that vector in place at 4672A3..4672B4.
    Vertical {
        detonation_altitude: i32,
        acceleration: i32,
        max_speed: i32,
    },
}

/// Persistent inputs and facing state for the `BulletClass::Update` ROT branch.
///
/// The native sidewinder phase includes a BulletClass-identity-derived value
/// whose derivation is not yet closed. `sidewinder_phase` is therefore an
/// explicit serialized seam rather than an invented stable-id formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ProjectileGuidance {
    pub rot: i32,
    pub missile_rot_var: SimFixed,
    /// `BulletTypeClass::CourseLockDuration` (`+0x2E0`). This is the authored
    /// duration, not a countdown: `BulletClass::AI @ 0x0046695B` compares its
    /// own frame counter against it and latches `IsCourseLocked` (`+0x105`,
    /// constructed to 1 at `0x004663B6`) off exactly once.
    pub course_lock_duration: u16,
    pub sidewinder_phase: u8,
    pub airburst: bool,
    /// `BulletTypeClass+0x2A2`, the common final-snap exclusion at `0x00467CDE`.
    /// The earlier homing reach snap at `0x00466E22` does not test this flag.
    pub inaccurate: bool,
    pub very_high: bool,
    pub level: bool,
    /// Flight heading as a math BAM (`0` = +X). Native keeps the direction
    /// implicitly in the double velocity vector it renormalises each frame at
    /// `0x004669F3`; VERA's velocity is integer leptons, so at the 1-lepton
    /// launch magnitude the direction has to be carried explicitly.
    /// `heading_bam == facing16 - 0x4000` for a native launch facing.
    pub heading_bam: u16,
    pub frames_elapsed: u32,
    /// `Bullet+0x110`, written from the weapon's `Speed=` (`WeaponTypeClass
    /// +0xA8`) at `TechnoClass::FireAt 0x006FEA46`. This is the ceiling the
    /// `Acceleration=` ramp climbs toward, never the launch speed.
    pub max_speed: u16,
    /// `BulletTypeClass::Acceleration` (`+0x2D0`, constructor default 3),
    /// consumed by the homing ramp at `BulletClass::AI 0x00466985`.
    pub acceleration: i32,
    /// The proximity fuse's reference coordinate, frozen at launch.
    /// `ProximityDetector::Setup @ 0x004E1130` (sole caller
    /// `BulletClass::Fire 0x00468A93`) copies the target's launch-time
    /// `GetTargetCoords` into detector `+0x18..+0x20` once;
    /// `ProximityDetector::Check @ 0x004E11F0` (sole caller
    /// `BulletClass::AI 0x00467C35`) measures the live bullet coordinate
    /// against that stored copy and never rewrites it.
    pub fuse_reference: ProjectileCoord,
    /// `Bullet+0x118`, the warm-up counter of the closing-rate detonation
    /// heuristic on the homing arm. Native runs the plain accumulate while it
    /// is below 60 and only then admits the decay test — `0x00466FB4
    /// MOV EAX,[EBP+0x118] / CMP EAX,0x3C / JGE`, then `INC EAX /
    /// MOV [EBP+0x118],EAX` on the warm-up side.
    pub closing_frames: u16,
    /// `Bullet+0x120`, the closing-rate accumulator of the same heuristic
    /// (`0x00466FD8 FLD double ptr [EBP+0x120]` / `0x00466FEE FST double ptr
    /// [EBP+0x120]`), held as native `double` bits because the native update is
    /// `accum * 0.9833333333333333 + delta` in x87 doubles.
    pub closing_accumulator_bits: u64,
}

/// The independently proved altitude-policy result of
/// `BulletClass::ComputeArcingTrajectoryStep` at `0x005B20F0`.
///
/// Its live floor/bridge probe is intentionally left to the world collision
/// substrate; ROT steering below does not substitute cell levels for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectileRotAltitudeDecision {
    pub control_terrain_clearance: bool,
    pub clearance_levels: i32,
    pub z_delta: i32,
    pub desired_pitch: Option<u16>,
}

/// Pack the proven high-byte ROT control word. Native FISTP rounding is used
/// before the low-byte truncation.
#[cfg(test)]
pub fn projectile_rot_turn_word(varied_rot: f64, target_distance: i32, course_locked: bool) -> u16 {
    if course_locked {
        return 0;
    }
    let rate = if target_distance < 256 {
        (varied_rot * 1.5).round_ties_even()
    } else {
        varied_rot.round_ties_even()
    } as i32;
    (rate as u16 & 0xff) << 8
}

fn projectile_rot_turn_word_fixed(
    varied_rot: SimFixed,
    target_distance: i32,
    course_locked: bool,
) -> u16 {
    if course_locked {
        return 0;
    }
    let varied_rot = if target_distance < 256 {
        varied_rot * SimFixed::lit("1.5")
    } else {
        varied_rot
    };
    let bits = i64::from(varied_rot.to_bits());
    let whole = bits / 65_536;
    let remainder = bits.unsigned_abs() % 65_536;
    let rounded = if remainder > 32_768 || (remainder == 32_768 && whole & 1 != 0) {
        whole + i64::from(bits.is_positive()) - i64::from(bits.is_negative())
    } else {
        whole
    };
    (rounded as u16 & 0xff) << 8
}

/// Apply the closed VeryHigh/Airburst clearance admission and error bands.
#[cfg(test)]
pub fn projectile_rot_altitude_decision(
    target_is_aircraft: bool,
    airburst: bool,
    very_high: bool,
    level: bool,
    horizontal_distance: i32,
    target_height_difference: i32,
    level_height: i32,
    current_clearance_error: i32,
    turn_word: u16,
) -> ProjectileRotAltitudeDecision {
    let close_threshold = if very_high { 6 } else { 3 } * 256;
    let turn_quantum = (((u32::from(turn_word) >> 7) + 1) >> 1) as u8;
    let clearance_levels = if airburst || very_high {
        10
    } else {
        (target_height_difference / 256).min(5)
    };
    if target_is_aircraft
        || (!airburst && horizontal_distance <= close_threshold)
        || turn_quantum <= 1
        || level
    {
        return ProjectileRotAltitudeDecision {
            control_terrain_clearance: false,
            clearance_levels,
            z_delta: 0,
            desired_pitch: None,
        };
    }

    let z_delta = if current_clearance_error < -20 {
        18
    } else if current_clearance_error > 20 {
        -18
    } else {
        0
    };
    let half_level = level_height / 2;
    let desired_pitch = if current_clearance_error < -half_level {
        0x2000
    } else if current_clearance_error > half_level {
        0x4800
    } else {
        0x4000
    };
    ProjectileRotAltitudeDecision {
        control_terrain_clearance: true,
        clearance_levels,
        z_delta,
        desired_pitch: Some(desired_pitch),
    }
}

/// Serialized `BulletClass` SHP animation bytes (`this+0x12c/+0x12d`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ProjectileVisualState {
    pub anim_low: u8,
    pub anim_high: u8,
    pub anim_rate: u8,
    pub runtime_frame: u8,
    pub runtime_countdown: u8,
}

impl ProjectileVisualState {
    pub const fn new(anim_low: u8, anim_high: u8, anim_rate: u8) -> Self {
        Self {
            anim_low,
            anim_high,
            anim_rate,
            runtime_frame: 0,
            runtime_countdown: anim_rate,
        }
    }

    pub fn advance(&mut self) {
        if self.anim_low == 0 && self.anim_high == 0 {
            return;
        }
        self.runtime_countdown = self.runtime_countdown.wrapping_sub(1);
        if self.runtime_countdown != 0 {
            return;
        }
        self.runtime_countdown = self.anim_rate;
        self.runtime_frame = self.runtime_frame.wrapping_add(1);
        if self.runtime_frame > self.anim_high {
            self.runtime_frame = self.anim_low;
        }
    }
}

impl ProjectileCoord {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// Resolve the current virtual `CellClass::GetTargetCoords` value for a stable
/// CellClass target. The cell identity is retained by the projectile; terrain
/// level/slope and the live CellClass structural bit are read again on every
/// visit.
pub(crate) fn cell_target_coord(
    terrain: Option<&ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
) -> ProjectileCoord {
    let x = i32::from(rx)
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(crate::sim::cell_kernel::CELL_CENTER_LEPTONS);
    let y = i32::from(ry)
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(crate::sim::cell_kernel::CELL_CENTER_LEPTONS);
    let z = terrain
        .and_then(|grid| grid.cell(rx, ry))
        .map(|cell| {
            // gamemd-derived: `CellClass::GetTargetCoords +0x58 @ 0x00486890`
            // delegates `+0x48 @ 0x00486840` to
            // `CellClass::ComputeGroundHeightAtCoord @ 0x0047B3A0`, then adds 416 iff
            // this CellClass's own `+0x140 & 0x100` is set. Bridge runtime
            // walkability is not consulted.
            crate::util::lepton::ground_height_leptons(cell.level, cell.slope_type, x, y)
                .expect("resolved CellClass target must have a supported slope")
                .wrapping_add(
                    if cell.bridge_facts.raw_flags
                        & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                        != 0
                    {
                        crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32
                    } else {
                        0
                    },
                )
        })
        // Mapless store fixtures use a flat cell. Production misses retain
        // ProjectileTarget::DummyCell and use its live level/slope/bridge
        // fields through dummy_cell_target_coord; the native dummy is not
        // all-zero (its ctor initializes tile+38 to DWORD0xFFFF, for example).
        .unwrap_or(0);
    ProjectileCoord::new(x, y, z)
}

/// Resolve the current virtual `CellClass::GetTargetCoords` value for the one
/// shared fallback CellClass. Unlike a stable allocated cell, every later miss
/// can change the coordinate observed through this retained identity.
pub(crate) fn dummy_cell_target_coord(dummy: &SharedCellDummy) -> ProjectileCoord {
    let snapshot = dummy.snapshot();
    let x = snapshot
        .coord
        .0
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(crate::sim::cell_kernel::CELL_CENTER_LEPTONS);
    let y = snapshot
        .coord
        .1
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(crate::sim::cell_kernel::CELL_CENTER_LEPTONS);
    // `CellClass` target virtual +0x58 at `0x00486890` delegates +0x48 at
    // `0x00486840`, which calls
    // `CellClass::ComputeGroundHeightAtCoord @ 0x0047B3A0`.
    // Active retail initializes the Cell-owned scalar independently, but its
    // captured value is the same 104 used by the shared ground evaluator.
    let z =
        crate::util::lepton::ground_height_leptons(snapshot.level as u8, snapshot.slope_type, x, y)
            .expect("shared CellClass target must have a supported slope")
            // `CellClass::GetTargetCoords @ 0x00486890` adds the process-global
            // high-bridge delta when `CellClass+0x140 & 0x100` is live. The floor
            // beneath it remains the verified 104-lepton CellClass kernel above.
            .wrapping_add(
                if snapshot.bridge_flags_0x1180 & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                    != 0
                {
                    crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32
                } else {
                    0
                },
            );
    ProjectileCoord::new(x, y, z)
}

/// The original target retained by a projectile after weapon fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ProjectileTarget {
    Entity(u64),
    /// Stable MapClass CellClass identity. Its target coordinate is resolved
    /// from live terrain state instead of freezing the cleanup-time Vec3.
    Cell {
        rx: u16,
        ry: u16,
    },
    /// Native null AbstractClass target. This is distinct from an expired
    /// entity lookup: BulletClass pointer cleanup has already handled the
    /// reference synchronously, so `TargetExpiryPolicy` must not run.
    None,
    /// MapClass's one process-global fallback CellClass at `0x00ABDC50`.
    /// The enum stores the pointer kind, not a coordinate snapshot; Simulation
    /// owns the live identity and BulletClass AI resolves it every visit.
    DummyCell,
}

/// What to do when an entity target no longer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TargetExpiryPolicy {
    /// Remove the projectile without a damage handoff.
    Expire,
    /// Detonate at the last target coordinate observed by the projectile.
    DetonateAtLastKnown,
}

/// Terrain checks admitted for one `BulletClass` flight.
///
/// `native_cell_collision` models the ground-height / bridge-deck / building
/// block at `BulletClass::AI 0x004674AE`..`0x00467778`. That block lives inside
/// the `ROT < 1, Vertical = no` arm: `0x004671D0 MOV CL,[EAX+0x2C0] /
/// 0x004671D6 TEST CL,CL / 0x004671D8 JZ 0x00467402` reaches it only when
/// `Vertical == 0`, and a `Vertical` bullet takes the `0x004671DE`..`0x00467390`
/// arm instead, which runs its own `DetonationAltitude` / floor / bridge probe.
/// That is why the flag is narrowed on `!vertical` — not because a `Vertical`
/// bullet skips every terrain test.
///
/// Shared probe 468BB0 runs for every trajectory when its impact flag remains
/// clear. Production resolves cliff/wall cells, live Flak target aim, the
/// theater WaterSet tile interval, and the target's actual +54/+48 receivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectileCollisionPolicy {
    /// `BulletTypeClass::Level`: detonate after entering a non-water cell.
    pub level_non_water: bool,
    /// `BulletTypeClass::SubjectToWalls`: detonate after entering a live wall.
    pub subject_to_walls: bool,
    /// Ordinary non-guided `BulletClass::Update` floor/bridge/content probe.
    pub native_cell_collision: bool,
    /// Dropping suppresses detector-only admission, after Check runs and may update its watermark.
    pub dropping: bool,
    pub subject_to_cliffs: bool,
    pub flak_scatter: bool,
    pub anti_air: bool,
    pub airburst: bool,
    pub inaccurate: bool,
    /// BulletType +295: 48ACF0 multiplies live Rules gravity by binary64 0.5.
    pub floater: bool,
    /// BulletType +2C8, retained as exact binary64 bits for Eq/hash/save.
    pub elasticity_bits: u64,
    /// `BulletTypeClass::Arcing` (`+0x29B`): impact resolution
    /// (`0x00468ECF`) skips the detector-reference arm for it.
    #[serde(default)]
    pub arcing: bool,
}

impl ProjectileCollisionPolicy {
    pub const NONE: Self = Self {
        level_non_water: false,
        subject_to_walls: false,
        native_cell_collision: false,
        dropping: false,
        subject_to_cliffs: false,
        flak_scatter: false,
        anti_air: false,
        airburst: false,
        inaccurate: false,
        floater: false,
        elasticity_bits: 0x3fe8_0000_0000_0000,
        arcing: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectileBridgeCrossing {
    None,
    Up,
    Down,
}

pub fn projectile_bridge_crossing(
    previous_z: i32,
    candidate_z: i32,
    surface_z: i32,
) -> ProjectileBridgeCrossing {
    if previous_z < surface_z && candidate_z >= surface_z {
        ProjectileBridgeCrossing::Up
    } else if previous_z >= surface_z && candidate_z < surface_z {
        ProjectileBridgeCrossing::Down
    } else {
        ProjectileBridgeCrossing::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectileCellObstacle {
    None,
    Building(u64),
    Overlay,
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn projectile_cell_obstacle(
    candidate_z: i32,
    floor_z: i32,
    building_id: Option<u64>,
    overlay_connected: bool,
    building_is_target: bool,
    building_late_exemption: bool,
    building_vslot_exemption: bool,
    target_owner_allied_with_building: bool,
) -> ProjectileCellObstacle {
    if candidate_z < floor_z || candidate_z >= floor_z.saturating_add(150) {
        return ProjectileCellObstacle::None;
    }
    if let Some(building_id) = building_id {
        let exempt = building_is_target
            || building_late_exemption
            || building_vslot_exemption
            || target_owner_allied_with_building;
        return if exempt {
            ProjectileCellObstacle::None
        } else {
            ProjectileCellObstacle::Building(building_id)
        };
    }
    if overlay_connected {
        ProjectileCellObstacle::Overlay
    } else {
        ProjectileCellObstacle::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectileCollisionResponse {
    TargetZClamp(ProjectileCoord),
    SlopeMatrixReflect {
        impact: ProjectileCoord,
        velocity: ProjectileVelocity,
    },
    /// Ordinary pre-commit AI 467494..467B7A, including its early returns.
    Ordinary {
        candidate: ProjectileCoord,
        velocity: ProjectileVelocity,
        impact: bool,
        near_target: bool,
        left_map: bool,
    },
    /// The world's answer to [`ProjectileCollisionPhase::ImpactLadder`].
    ImpactLadder(ImpactLadderWorld),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProjectileCollisionPhase {
    Ordinary {
        /// Native Bullet+E8 at tail entry: pre-scratch for ordinary, post-ramp for Vertical.
        persistent_velocity: ProjectileVelocity,
        motion: ProjectileCollisionMotion,
    },
    Shared,
    TargetLocation,
    /// `0x00468D80` asks the target and the warhead for
    /// [`resolve_impact_coord`]; the candidate is the bullet's Location.
    ImpactLadder,
}

/// The Target facts `BulletClass` impact resolution (`0x00468D80`) reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpactLadderTarget {
    /// vt+0x48: a Building's centre (`0x00447AC0`), a Cell's coordinate
    /// (`0x00486840`), everything else its Location (`0x005F65A0`).
    pub coords: ProjectileCoord,
    /// vt+0x58: every Object forwards to vt+0x48 (`0x00410540`); a Cell
    /// answers `0x00486890`.
    pub aim: ProjectileCoord,
    /// vt+0xA4: vt+0x48 (`0x0041BDD0`) for all but a Building
    /// (`0x004500A0`, its TargetCoordOffset).
    pub offset_coords: ProjectileCoord,
    /// vt+0x54, ObjectClass::IsInAir `0x005F6B90`: on the map and at least
    /// two levels up (Aircraft `0x0041B920` asks the V3/Dreadnought rocket
    /// locomotor); a Cell never is (`0x00410530`).
    pub in_air: bool,
    /// vt+0x78 answers layer 2 (Ground).
    pub ground_layer: bool,
    /// `ObjectClass::DistanceTo 0x005F6360` from the bullet's Location:
    /// vt+0x48 to vt+0x48, less a Building's `(Width + Height) * 64`,
    /// clamped at 0.
    pub distance: i32,
    /// A Building whose type has a nonzero TargetCoordOffset (`+0xEBC`).
    pub building_offset: bool,
}

/// What the world contributes to [`resolve_impact_coord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImpactLadderWorld {
    /// The live Target (`+0x10C`), or none once PointerExpired cleared it.
    pub target: Option<ImpactLadderTarget>,
    /// The warhead's `EMEffect=` (`+0x154`).
    pub em_effect: bool,
}

/// The bullet facts [`resolve_impact_coord`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpactLadderBullet {
    /// Location (`+0x9C`), where the AI committed the impact.
    pub location: ProjectileCoord,
    /// The ProximityDetector reference (`+0xB8 + 0x18` = `+0xD0`).
    pub reference: ProjectileCoord,
    /// The AI's impact flag, the ladder's argument (`0x00467FA2`).
    pub impact_flag: bool,
    pub inaccurate: bool,
    pub airburst: bool,
    pub arcing: bool,
    /// ROT > 0 (`+0x2DC`).
    pub homing: bool,
}

/// `BulletClass` impact resolution `0x00468D80` up to its first
/// `DetonateAtCoord`: the coordinate the (first) cluster detonates at.
/// - An `Inaccurate=` bullet keeps its Location.
/// - A Target within `0x20` of its vt+0x48 (3-D, Sqrt_Approx, ftol) takes
///   vt+0x48 unless the bullet is `Airburst=`.
/// - `EMEffect=` or `Airburst=` stops there.
/// - A fuse detonation (impact flag clear) of a bullet that is neither
///   `Arcing=` nor ROT > 0 takes the ProximityDetector reference when it is
///   not Empty.
/// - An in-air Target off the Ground layer takes vt+0xA4 within `0x80`;
///   otherwise a Target within `0x2A` (DistanceTo) takes vt+0x58, or vt+0xA4
///   for a Building with a TargetCoordOffset.
///
/// Native execution: `tools/projectile_oracle/impact_ladder.py`.
pub fn resolve_impact_coord(
    bullet: &ImpactLadderBullet,
    world: &ImpactLadderWorld,
) -> ProjectileCoord {
    let mut coord = bullet.location;
    if bullet.inaccurate {
        return coord;
    }
    if let Some(target) = world.target
        && coord_distance(bullet.location, target.coords) < 0x20
        && !bullet.airburst
    {
        coord = target.coords;
    }
    if world.em_effect || bullet.airburst {
        return coord;
    }
    if !bullet.impact_flag
        && !bullet.arcing
        && !bullet.homing
        && bullet.reference != ProjectileCoord::new(0, 0, 0)
    {
        coord = bullet.reference;
    }
    let Some(target) = world.target else {
        return coord;
    };
    if target.in_air && !target.ground_layer {
        if target.distance < 0x80 {
            coord = target.offset_coords;
        }
    } else if target.distance < 0x2A {
        coord = if target.building_offset {
            target.offset_coords
        } else {
            target.aim
        };
    }
    coord
}

/// Ordinary AI compares its binary64 candidate before ftol; the bridge and
/// cell lookups separately consume the integer coordinate (467494..4677D3).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProjectileCollisionMotion {
    pub candidate: [crate::util::native_x87::NativeF64Bits; 3],
    pub velocity: [crate::util::native_x87::NativeF64Bits; 3],
}

impl ProjectileCollisionMotion {
    pub fn from_coordinate(candidate: ProjectileCoord, velocity: ProjectileVelocity) -> Self {
        use crate::util::native_x87::NativeF64Bits;
        Self {
            candidate: [candidate.x, candidate.y, candidate.z]
                .map(|value| NativeF64Bits::from_bits(f64::from(value).to_bits())),
            velocity: velocity.native(),
        }
    }

    pub fn candidate_coord(&self) -> ProjectileCoord {
        let [x, y, z] = Self::quantized(self.candidate);
        ProjectileCoord::new(x, y, z)
    }

    pub fn quantized(values: [crate::util::native_x87::NativeF64Bits; 3]) -> [i32; 3] {
        use crate::util::native_x87::X87Chop53;
        values.map(|value| {
            X87Chop53::ftol_i64(X87Chop53::load_f64(value).expect("finite collision input"))
                .expect("representable collision coordinate") as i32
        })
    }
}

/// Original Bullet AI467666..467778: inverse slope, Elasticity scalar,
/// negate LOCAL Z, forward slope, negate world Y. Native stores every matrix
/// intermediate as chopped f32 and retains resulting doubles until copyback.
#[cfg(test)]
pub(crate) fn projectile_slope_reflect_with_elasticity(
    velocity: ProjectileVelocity,
    slope_type: u8,
    elasticity_bits: u64,
) -> Option<ProjectileVelocity> {
    let input = ProjectileCollisionMotion::from_coordinate(ProjectileCoord::new(0, 0, 0), velocity)
        .velocity;
    let reflected = projectile_slope_reflect_double(input, slope_type, elasticity_bits)?;
    Some(ProjectileVelocity::from_native(reflected))
}

pub(crate) fn projectile_slope_reflect_double(
    velocity: [crate::util::native_x87::NativeF64Bits; 3],
    slope_type: u8,
    elasticity_bits: u64,
) -> Option<[crate::util::native_x87::NativeF64Bits; 3]> {
    use crate::util::native_x87::{NativeF64Bits, X87Chop53};
    let matrix = crate::sim::particles::spark_world::slope_matrix(slope_type).ok()?;
    let mut vector = [crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO; 3];
    for (out, input) in vector.iter_mut().zip(velocity) {
        *out = X87Chop53::store_f32(X87Chop53::load_f64(input).ok()?).ok()?;
    }
    let elasticity =
        X87Chop53::store_f32(X87Chop53::load_f64(NativeF64Bits::from_bits(elasticity_bits)).ok()?)
            .ok()?;
    let reflected = crate::sim::particles::spark::reflect_slope_vector_with_elasticity(
        vector, matrix, elasticity,
    )
    .ok()?;
    Some(
        reflected.map(|value| {
            NativeF64Bits::from_bits(f64::from(f32::from_bits(value.bits())).to_bits())
        }),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectileBurstPlan {
    pub detonation_count: u32,
    pub random_radius_rolls: u32,
    pub random_coordinate_calls: u32,
}

#[cfg(test)]
pub fn projectile_burst_plan(airburst: bool, cluster: i32) -> ProjectileBurstPlan {
    if airburst {
        return ProjectileBurstPlan {
            detonation_count: 1,
            random_radius_rolls: 0,
            random_coordinate_calls: 0,
        };
    }
    let count = cluster.max(0) as u32;
    ProjectileBurstPlan {
        detonation_count: count,
        random_radius_rolls: count,
        random_coordinate_calls: count,
    }
}

/// The next cluster coordinate of
/// `BulletClass::ResolveImpactCoordAndDetonate @ 0x00468D80`. The impact is
/// copied once before the loop (`0x00469008..0x0046901C`); after each cluster
/// detonation the loop draws `RandomRanged(0x100, 0x200)` (`0x00469057`) and
/// moves that copy that far in a random direction (`0x0049F420`, no cell
/// snap, `in` = the copy at `0x0046905F`: one raw Scenario draw, the table
/// direction of its low byte, truncated; the impact itself when either axis
/// leaves the 512-cell map). So every cluster after the first lands around
/// the impact, never around the previous cluster.
pub fn projectile_next_cluster_coord(
    impact: ProjectileCoord,
    scenario_rng: &mut SimRng,
) -> ProjectileCoord {
    let distance = scenario_rng.next_range_i32_inclusive(0x100, 0x200);
    let (x, y) = crate::sim::combat::inviso_scatter::random_direction_coord(
        scenario_rng,
        impact.x,
        impact.y,
        distance,
    );
    ProjectileCoord::new(x, y, impact.z)
}

/// Native two-draw random-cell fallback used after hostile shrapnel targets.
pub fn projectile_random_shrapnel_cell(
    center_rx: i32,
    center_ry: i32,
    scenario_rng: &mut SimRng,
) -> (i32, i32) {
    let dx = scenario_rng.next_range_u32_inclusive(0, 4) as i32 - 2;
    let dy = scenario_rng.next_range_u32_inclusive(0, 4) as i32 - 2;
    (center_rx + dx, center_ry + dy)
}

pub fn projectile_shrapnel_count(
    configured_count: i32,
    has_firer: bool,
    distance_to_target_cells: i32,
) -> u32 {
    if configured_count >= 0 {
        return configured_count as u32;
    }
    if !has_firer {
        return 3;
    }
    configured_count
        .saturating_neg()
        .saturating_sub(distance_to_target_cells)
        .max(0) as u32
}

/// Which arm of the native special-detonation chain claims one impact.
///
/// gamemd-derived: `BulletClass::DetonateAtCoord @ 0x004690b0` is a strict
/// twelve-arm `else if` chain over `WarheadTypeClass` flag bytes. Each test's
/// fall-through `JZ` targets the *next* test, so the arms are mutually
/// exclusive and the order below is the native order, read from the flag tests
/// at `0x00469211`, `0x00469343`, `0x0046937a`, `0x004693d3`, `0x00469423`,
/// `0x004694cb`, `0x00469705`, `0x0046978e`, `0x004699ca`, `0x00469a03` and
/// `0x00469a2c`.
///
/// Only the final else at `0x00469a3f` reaches
/// `BulletClass::SpawnShrapnel @ 0x0046a310` and
/// `Apply_area_damage @ 0x00489280`, so any special arm suppresses both. What a
/// special arm does **not** suppress is the shared tail: every arm, including
/// its internal bail-outs (e.g. MindControl with no CaptureManager at
/// `0x00469235`), leaves through `JMP LAB_00469AA4`, and `LAB_00469AA4`
/// (`0x00469aa4 MOV EAX,[EBP+0x8]`) is not the epilogue — the epilogue is
/// `0x0046a290`. Unconditionally the tail is the `Inviso` visual re-scatter
/// (`0x00469ad7`), the explosion-anim *selection*
/// (`Warhead::SelectExplosionAnim` called at `0x00469bcf`), the `AnimList=`
/// `AnimClass` itself (built from `0x00469c46`), the debris loop and the
/// `Airburst=` sub-munition fan (`BulletClass::Init` at `0x00469f7f` /
/// `0x0046a150`). The ordinary arm reaches that same tail only behind the
/// `bullet+0x90` gate at `0x00469a94`; special arms bypass the gate.
///
/// **The combat-light / `CLDisable*` block is NOT part of that unconditional
/// tail.** `0x00469bf0..0x00469c45`, ending in the light spawn
/// `0x00469c41 CALL 0x0048a620`, runs only when `bullet+0xe0 != 0` **and**
/// `[ESP+0xf] == 0` — `0x00469bdc TEST AL,AL` / `0x00469be2 JZ 0x0046a299`
/// then `0x00469be8 TEST AL,AL` / `0x00469bea JNZ 0x0046a2a1` — and
/// `0x0046a29b JZ 0x00469c46` rejoins *past* it, so the `AnimList=` anim is
/// reached either way. `bullet+0xe0` is the per-**weapon** `Bright=` flag, not
/// a warhead key: `BulletClass::Init` stores its last stack argument there
/// (`0x004664e6 MOV byte ptr [ECX+0xe0], DL` from `[ESP+0x1c]`), fed from
/// `TechnoClass::FireAt` (`0x006fe53f MOV CL, byte ptr [EBX+0x12f]`) =
/// `WeaponTypeClass+0x12f`, written by `WeaponTypeClass::ReadINI` at
/// `0x00772817` from the key string at `0x00847dd0` = `"Bright"`. Whoever
/// ports `0x0048a620` must carry that gate, or every non-`Bright=` impact
/// flashes. The block itself folds warhead `+0x151`/`+0x152`/`+0x153`
/// (`CLDisableRed=`/`Green=`/`Blue=`, read at `0x00469bf8`/`0x00469c07`/
/// `0x00469c13`) into a 2/4/8 bit mask and passes it with the impact coord and
/// `bullet+0x6c`; the exact visual `0x0048a620` produces is UNCHECKED here.
///
/// Both deliveries dispatch the chain through one owner,
/// `combat::world_receiver::run_special_detonation_arm`: a visible bullet at
/// its detonation, and VERA's immediate `Inviso=` shot, which natively is the
/// same bullet detonating (every stock special weapon but the dog's and the
/// Terror Drone's jump is Inviso).
///
/// RESIDUAL — **six special effect bodies are not implemented in VERA.**
/// MindControl (`capture_manager`), IvanBomb and BombDisarm (`bomb`), Parasite
/// (`combat/parasite.rs`; the Giant Squid's grapple is its own residual there)
/// and Temporal (`temporal`) run their bodies. ElectricAssault, Locomotor,
/// Airstrike, DirectRocker, MakesDisguise and NukeMaker claim the detonation,
/// suppress damage and shrapnel exactly as native does, and then run the
/// shared tail without performing their effect.
/// - Trigger: a stock weapon whose warhead carries one of those flags: the
///   Tesla Trooper's `[AssaultBolt]` at its own coil, the Magnetron's
///   `[MagneticBeam]`/`[MagneticBeamE]`, Boris's `[Flare]`, the Spy's
///   `[MakeupKit]` and the nuclear missile's `[NukeCarrier]`
///   (`[TankMakeupKit]`/`[CRMakeupKit]` are mounted by nothing; DirectRocker
///   has no live stock line).
/// - Player effect: the shot lands, plays its animation and leaves its crater,
///   but no coil is charged, no vehicle lifted, no airstrike called and no Spy
///   disguised, and the target takes no damage from that shot.
/// - Frequency: every game with Magnetrons (Yuri), Boris or Spies.
/// - Downstream risk: each port adds snapshotted, hashed state (the charger
///   vector, the piggyback lift links, the AirstrikeClass manager, the
///   disguise) with its own `SNAPSHOT_VERSION` bump.
///
/// RESIDUAL — **the `[ESP+0xf]` ordinary-arm visual bypass is not modelled.**
/// `0x004690c9` zeroes `[ESP+0xf]` on entry and `0x00469a9f` is its only
/// writer: it sets 1 when `Apply_area_damage` (called at `0x00469a83`) returns
/// exactly 2 and the `bullet+0x90` gate at `0x00469a94` passed
/// (`0x00469a9a CMP EAX,0x2` / `0x00469a9d JNZ 0x00469aa4`). When set, the tail
/// diverts at `0x00469bea` / `0x0046a299` to `0x0046a2a1`, which builds a
/// different `0x1c8`-byte object from global `[0x008871e0] + 0x350` and falls
/// straight into the epilogue — skipping the `AnimList=` anim, the combat
/// light, the debris loop and the `Airburst=` fan entirely. It is the same
/// `Apply_area_damage` return-value handling as the `bullet+0x90` gate above,
/// and it suppresses far more than that gate does.
/// - Trigger: an ordinary-arm impact where `Apply_area_damage` returns 2. What
///   that return value means is **UNCHECKED** — settled by reading
///   `Apply_area_damage @ 0x00489280`'s return contract.
/// - Player effect: on native such an impact draws the alternate object
///   instead of its explosion, crater, debris and `Airburst=` children; VERA
///   always draws the ordinary set.
/// - Frequency: UNCHECKED on the ordinary arm, and provably **zero on every
///   arm in this enum** — `0x00469a9f` is the flag's only writer and it sits
///   inside the final else at `0x00469a3f`, which `get_xrefs_to 0x00469a3f`
///   shows is entered by exactly one jump, the `NukeMaker` test's `JZ` at
///   `0x00469a34`. No special arm can reach it, so the flag is 0 for all of
///   them.
/// - Downstream risk: none for M15a. It belongs with the rest of the tail
///   (`Inviso`, debris, `Airburst=`) and with the `bullet+0x90` gate, i.e. to
///   whoever ports `Apply_area_damage`'s return contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialDetonationAction {
    /// `MindControl=` (`WarheadTypeClass+0x155`), test `0x00469211` ->
    /// `CaptureManagerClass::CaptureUnit @ 0x00471d40`, ported in
    /// `capture_manager` (the Inviso delivery dispatches it too).
    MindControl,
    /// `IvanBomb=` (`+0x157`), test `0x00469343` ->
    /// `BombListClass::Attach @ 0x00438e70`, ported in `bomb`.
    IvanBomb,
    /// `ElectricAssault=` (`+0x158`), test `0x0046937a`: a Building target
    /// hit by an Infantry owner -> the Tesla-Coil charger-vector move-to-back
    /// at `0x00452820`. UNIMPLEMENTED.
    ElectricAssault,
    /// `Parasite=` (`+0x159`), test `0x004693d3` ->
    /// `ParasiteClass::AttachTo @ 0x0062a980`, ported in `combat/parasite.rs`
    /// (the Giant Squid's grapple is that module's residual).
    Parasite,
    /// `Temporal=` (`+0x15a`), test `0x00469423` ->
    /// `TemporalClass::InitiateWarp @ 0x0071af20`, ported in `temporal` (the
    /// Inviso delivery dispatches it too).
    Temporal,
    /// `IsLocomotor=` (`+0x15b`), test `0x004694cb` -> the Magnetron's lift,
    /// `TechnoClass::ImbueLocomotor @ 0x00710000` with the warhead's
    /// `Locomotor=` CLSID. UNIMPLEMENTED.
    Locomotor,
    /// `Airstrike=` (`+0x16c`), test `0x00469705` ->
    /// `AirstrikeClass::SetTarget @ 0x0041d830`. UNIMPLEMENTED.
    ///
    /// The decompile shows this arm as conditional; the disassembly does not.
    /// `0x0046970d JZ 0x0046978e` is its only fall-through and fires on the
    /// flag alone — every sub-condition failure (`0x00469717`, `0x00469725`,
    /// `0x0046972f`, `0x0046973d`, `0x0046975b`) jumps to `0x00469aa4`
    /// instead. So Airstrike is flag-gated like the other ten.
    Airstrike,
    /// `DirectRocker=` (`+0x14f`), test `0x0046978e` ->
    /// `TechnoClass::ApplyRocker` through vtable `+0x3d8` at `0x004699a1`,
    /// plus the mutual `+0x2a8` link.
    ///
    /// The **only** conditional arm in the chain: `0x00469796` (flag),
    /// `0x004697a4` (`Target != 0`) and `0x004697b2`
    /// (`Target->What_Am_I() == 1`, `UnitClass`) all fall through to
    /// `0x004699c4`, the *next* test, so a `DirectRocker=yes` warhead that hits
    /// infantry, a building or a bare cell still takes ordinary damage.
    /// Failures *after* those three (`0x004697dc`, `0x004697e4`, `0x004697f6`)
    /// jump to the tail like every other arm.
    ///
    /// Dead in stock YR: `rulesmd.ini` carries no live `DirectRocker=` line
    /// (its one textual occurrence is inside a `;` comment), so the arm never
    /// fires in stock play. Kept correct anyway; the impulse body is
    /// deliberately NOT implemented.
    DirectRocker,
    /// `BombDisarm=` (`+0x16e`), test `0x004699ca` ->
    /// `BombClass::Defuse @ 0x004389b0`, ported in `bomb`.
    BombDisarm,
    /// `MakesDisguise=` (`+0x175`), test `0x00469a03` -> the firer disguises as
    /// the target through owner vtable `+0x46c` at `0x00469a24`.
    /// UNIMPLEMENTED.
    MakesDisguise,
    /// `NukeMaker=` (`+0x176`), test `0x00469a2c` ->
    /// `BulletClass::SpawnDownwardNuke @ 0x0046b310`. UNIMPLEMENTED (its only
    /// stock weapon, `[NukeCarrier]`, is the superweapon's launch).
    NukeMaker,
    /// The final else at `0x00469a3f`: shrapnel plus `Apply_area_damage`.
    OrdinaryDamage,
}

impl SpecialDetonationAction {
    /// True for every arm that claims the detonation away from
    /// `Apply_area_damage @ 0x00489280` and
    /// `BulletClass::SpawnShrapnel @ 0x0046a310`.
    pub fn suppresses_ordinary_damage(self) -> bool {
        self != SpecialDetonationAction::OrdinaryDamage
    }
}

/// The eleven `WarheadTypeClass` flag bytes the native chain tests, in native
/// order. Offsets verified from `WarheadTypeClass::ReadINI` string->offset
/// writes (`0x0075d5d8`, `0x0075d7e0`, `0x0075d823`, `0x0075d82e`,
/// `0x0075d84e`, `0x0075d871`, `0x0075d87c`, `0x0075d8f0`, `0x0075d91b`,
/// `0x0075d969`, `0x0075d983`) and cross-checked against the reads in
/// `BulletClass::DetonateAtCoord @ 0x004690b0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpecialDetonationFlags {
    pub mind_control: bool,
    pub ivan_bomb: bool,
    pub electric_assault: bool,
    pub parasite: bool,
    pub temporal: bool,
    pub is_locomotor: bool,
    pub airstrike: bool,
    pub direct_rocker: bool,
    pub bomb_disarm: bool,
    pub makes_disguise: bool,
    pub nuke_maker: bool,
}

impl SpecialDetonationFlags {
    /// The eleven flags of one warhead.
    pub fn of(warhead: &crate::rules::warhead_type::WarheadType) -> Self {
        Self {
            mind_control: warhead.mind_control,
            ivan_bomb: warhead.ivan_bomb,
            electric_assault: warhead.electric_assault,
            parasite: warhead.parasite,
            temporal: warhead.temporal,
            is_locomotor: warhead.is_locomotor,
            airstrike: warhead.airstrike,
            direct_rocker: warhead.direct_rocker,
            bomb_disarm: warhead.bomb_disarm,
            makes_disguise: warhead.makes_disguise,
            nuke_maker: warhead.nuke_maker,
        }
    }
}

/// Target-side context the native chain consults *inside* an arm predicate.
///
/// Only `DirectRocker` reads it. `0x0046979c`/`0x004697aa` load the bullet's
/// raw `+0x10c` target and call `What_Am_I` through vtable `+0x2c`; a null
/// target or any RTTI id other than 1 (`UnitClass::What_Am_I @ 0x00746e20`)
/// falls through to the BombDisarm test instead of claiming the detonation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpecialDetonationTarget {
    /// `Target != 0 && Target->What_Am_I() == 1`.
    pub is_unit: bool,
}

/// Resolve the one arm of `BulletClass::DetonateAtCoord @ 0x004690b0` that owns
/// this impact. The `else if` order is native; see [`SpecialDetonationAction`].
pub fn projectile_special_detonation_action(
    flags: SpecialDetonationFlags,
    target: SpecialDetonationTarget,
) -> SpecialDetonationAction {
    if flags.mind_control {
        SpecialDetonationAction::MindControl
    } else if flags.ivan_bomb {
        SpecialDetonationAction::IvanBomb
    } else if flags.electric_assault {
        SpecialDetonationAction::ElectricAssault
    } else if flags.parasite {
        SpecialDetonationAction::Parasite
    } else if flags.temporal {
        SpecialDetonationAction::Temporal
    } else if flags.is_locomotor {
        SpecialDetonationAction::Locomotor
    } else if flags.airstrike {
        // Flag-gated, not conditional — see the `Airstrike` variant doc.
        SpecialDetonationAction::Airstrike
    } else if flags.direct_rocker && target.is_unit {
        // The chain's only conditional arm: `0x004697a4` / `0x004697b2` fall
        // through to the BombDisarm test at `0x004699c4`, not to the tail.
        SpecialDetonationAction::DirectRocker
    } else if flags.bomb_disarm {
        SpecialDetonationAction::BombDisarm
    } else if flags.makes_disguise {
        SpecialDetonationAction::MakesDisguise
    } else if flags.nuke_maker {
        SpecialDetonationAction::NukeMaker
    } else {
        SpecialDetonationAction::OrdinaryDamage
    }
}

/// Stable projectile payload transferred to combat only at detonation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectilePayload {
    /// Damage after firing-side modifiers, before target/warhead resolution.
    pub base_damage: i32,
    pub warhead: InternedId,
    /// Weapon identity retained for impact-only effects such as radiation.
    pub weapon: InternedId,
}

/// Immutable admission data for an ordinary, non-vertical projectile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectileSpawn {
    /// BulletType+2F7 (`Flat`): Fire468B6D submits through GetLayer468B90,
    /// selecting Surface when true and Air otherwise. The display owner retains
    /// membership; this admission input is not a second live layer authority.
    pub flat: bool,
    pub source_id: u64,
    pub origin: ProjectileCoord,
    pub target: ProjectileTarget,
    /// Target coordinate captured when the weapon fires. Non-homing shots keep
    /// this destination; homing shots replace it from the live target table.
    pub initial_target_position: ProjectileCoord,
    pub payload: ProjectilePayload,
    /// The bullet's CURRENT speed in leptons per frame — native's
    /// `ftol(|velocity|)`, not the weapon's `Speed=`. `TechnoClass::FireAt
    /// @ 0x006FEA3A` launches every `ROT > 0` or `Vertical` bullet at 1, and
    /// `BulletClass::Fire @ 0x00468B2C` renormalises a `ROT > 0` bullet's
    /// vector to magnitude 1.0 regardless; `Speed=` only becomes the ceiling
    /// stored at `Bullet+0x110`. Non-homing, non-vertical bullets do keep the
    /// launch speed here, clamped to `dist/2` at `0x006FE9FE`.
    pub speed_leptons_per_frame: u16,
    pub velocity: ProjectileVelocity,
    pub trajectory: ProjectileTrajectory,
    /// Present only for a `BulletTypeClass::ROT` guided flight.
    pub guidance: Option<ProjectileGuidance>,
    pub visual: ProjectileVisualState,
    /// Signed proximity-only delay supplied to Fire; impacts bypass this gate.
    pub arm_frames: i32,
    /// Optional fuse duration; zero means the fuse detonates on this advance.
    pub fuse_frames: Option<u16>,
    /// `ROT > 0 || Ranged` admits the native closest-approach fuse helper.
    pub ranged_fuse: bool,
    /// Only homing projectiles update their destination from a live target.
    pub tracks_target: bool,
    pub target_expiry: TargetExpiryPolicy,
    pub collision: ProjectileCollisionPolicy,
}

/// Persistent state corresponding to one native `BulletClass` instance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Projectile {
    pub id: u64,
    /// LogicClass membership is reconstructed from the serialized mixed order.
    #[serde(skip)]
    pub in_logic_vector: bool,
    pub source_id: u64,
    pub position: ProjectileCoord,
    /// Bullet +134 and +140 are captured by Fire 4686A5/468722 and never
    /// replaced by the live homing target or proximity detector reference.
    pub launch_origin: ProjectileCoord,
    pub launch_target: ProjectileCoord,
    /// Bullet+14C: packed committed cell from Fire or the preceding AI tail.
    pub previous_cell: (i16, i16),
    pub target: ProjectileTarget,
    pub last_target_position: ProjectileCoord,
    pub payload: ProjectilePayload,
    pub speed_leptons_per_frame: u16,
    pub velocity: ProjectileVelocity,
    pub trajectory: ProjectileTrajectory,
    pub guidance: Option<ProjectileGuidance>,
    pub visual: ProjectileVisualState,
    /// Bullet+C4/CC: global frame anchor and signed proximity delay.
    pub arm_timer: crate::sim::timer::CdTimer,
    pub fuse_frames_remaining: Option<u16>,
    pub ranged_fuse: bool,
    pub last_distance_half: i32,
    pub tracks_target: bool,
    pub target_expiry: TargetExpiryPolicy,
    pub collision: ProjectileCollisionPolicy,
    /// Bullet `+0x8C`: FireAt copies the target's OnBridge onto an `Inviso=`
    /// bullet (`0x006FF08B..0x006FF0B0`); every other bullet keeps the
    /// constructor's false. `ObjectClass::GetHeight @ 0x005F5F40` then
    /// measures from the bridge deck.
    #[serde(default)]
    pub on_bridge: bool,
}

impl Projectile {
    /// The facts [`resolve_impact_coord`] reads, with the bullet at its
    /// committed Location.
    fn impact_ladder_bullet(&self, impact_flag: bool) -> ImpactLadderBullet {
        ImpactLadderBullet {
            location: self.position,
            reference: self.guidance.map_or(self.last_target_position, |guidance| {
                guidance.fuse_reference
            }),
            impact_flag,
            inaccurate: self
                .guidance
                .map_or(self.collision.inaccurate, |guidance| guidance.inaccurate),
            airburst: self
                .guidance
                .map_or(self.collision.airburst, |guidance| guidance.airburst),
            arcing: self.collision.arcing,
            homing: self.tracks_target,
        }
    }
}

/// Why a projectile reached its combat detonation handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProjectileDetonationReason {
    ReachedTarget,
    Fuse,
    Collision,
    TargetExpired,
}

/// One deferred `BulletClass::Detonate` handoff for combat to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectileDetonation {
    pub projectile_id: u64,
    pub source_id: u64,
    pub target: ProjectileTarget,
    pub impact: ProjectileCoord,
    pub payload: ProjectilePayload,
    pub reason: ProjectileDetonationReason,
}

/// Results from one stable-order projectile pass.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ProjectileAdvanceResult {
    pub detonations: Vec<ProjectileDetonation>,
    pub expired: Vec<u64>,
}

/// Serialized, stable-id ordered projectile collection.
///
/// `BTreeMap` makes creation-order IDs and processing order explicit. New
/// projectiles are only advanced by the next call, matching the usual
/// object-pass boundary instead of recursively advancing a newly fired shot.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectileStore {
    projectiles: BTreeMap<u64, Projectile>,
}

impl ProjectileStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.projectiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.projectiles.is_empty()
    }

    pub fn get(&self, id: u64) -> Option<&Projectile> {
        self.projectiles.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: u64) -> Option<&mut Projectile> {
        self.projectiles.get_mut(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u64, &Projectile)> {
        self.projectiles.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (&u64, &mut Projectile)> {
        self.projectiles.iter_mut()
    }

    pub(crate) fn remove(&mut self, id: u64) -> Option<Projectile> {
        self.projectiles.remove(&id)
    }

    /// Apply `BulletClass::PointerExpired @ 0x004684E0` to one stored Bullet.
    ///
    /// Source and target are independent arms: when both match the expired
    /// object, both change in the same synchronous callback. The projectile
    /// itself remains stored and registered; repeated callbacks are no-ops.
    pub(crate) fn pointer_expired(
        &mut self,
        projectile_id: u64,
        expired_id: u64,
        replacement_target: ProjectileTarget,
    ) -> bool {
        let Some(projectile) = self.projectiles.get_mut(&projectile_id) else {
            return false;
        };
        if projectile.source_id == expired_id {
            projectile.source_id = crate::sim::combat::RAD_NO_ATTACKER;
        }
        if projectile.target == ProjectileTarget::Entity(expired_id) {
            projectile.target = replacement_target;
        }
        true
    }

    /// Admit one projectile. All three `BulletClass::AI` flight arms are
    /// represented — `ROT >= 1` homing, the `ROT < 1` ballistic arm, and the
    /// `ROT < 1, Vertical` arm. An `Inviso` bullet is then placed on its
    /// target by [`Self::fire_inviso`] and detonates on its first AI.
    // AbstractClass::AssignUniqueID @ 0x00410230 obtains this identity from
    // ScenarioClass::NextUniqueID @ 0x0068BCB0; the store never owns a second
    // allocator.
    #[cfg(test)]
    pub fn spawn(&mut self, id: u64, spawn: ProjectileSpawn) -> u64 {
        self.spawn_at(id, 0, spawn)
    }

    pub(crate) fn spawn_at(&mut self, id: u64, binary_frame: u32, spawn: ProjectileSpawn) -> u64 {
        self.projectiles.insert(
            id,
            Projectile {
                id,
                in_logic_vector: false,
                source_id: spawn.source_id,
                position: spawn.origin,
                launch_origin: spawn.origin,
                launch_target: spawn.initial_target_position,
                previous_cell: ((spawn.origin.x / 256) as i16, (spawn.origin.y / 256) as i16),
                target: spawn.target,
                last_target_position: spawn.initial_target_position,
                payload: spawn.payload,
                speed_leptons_per_frame: spawn.speed_leptons_per_frame,
                velocity: spawn.velocity,
                trajectory: spawn.trajectory,
                guidance: spawn.guidance,
                visual: spawn.visual,
                arm_timer: crate::sim::timer::CdTimer::started(
                    binary_frame as i32,
                    spawn.arm_frames,
                ),
                fuse_frames_remaining: spawn.fuse_frames,
                ranged_fuse: spawn.ranged_fuse,
                // `ProximityDetector::Setup @ 0x004E1130` seeds `+0x24` with
                // the FULL launch distance from the bullet to its reference
                // coordinate, not the halved form `Check` writes afterwards.
                last_distance_half: proximity_initial_distance(
                    spawn.origin,
                    spawn.initial_target_position,
                ),
                tracks_target: spawn.tracks_target,
                target_expiry: spawn.target_expiry,
                collision: spawn.collision,
                on_bridge: false,
            },
        );
        id
    }

    /// `BulletClass::Construct @ 0x004664C0`'s Owner (`+0xB0`) on a re-fired
    /// bullet.
    pub(crate) fn set_owner(&mut self, id: u64, owner: u64) {
        if let Some(projectile) = self.projectiles.get_mut(&id) {
            projectile.source_id = owner;
        }
    }

    /// `BulletClass::Fire @ 0x00468670` for an `Inviso=` BulletType
    /// (`+0x29E`, `0x004688B7..0x00468A39`), after the common Unlimbo at the
    /// launch source: the bullet stands on `placement`, the target coordinate
    /// (`0x0046897D`; the firestorm and cliff walks are dormant or
    /// overwritten), its speed `+0x110` is 0, and its velocity is scaled by
    /// `0.0 / |v|` (an all-zero vector first becoming `(100, 0, 0)`), which a
    /// `ROT > 0` type then renormalises to `(1, 0, 0)` (`0x00468A98..0x00468B57`).
    /// The proximity detector's reference is the placement, so its starting
    /// distance is 0 (`ProximityDetector::Setup @ 0x004E1130`). FireAt then
    /// copies the target's OnBridge (`0x006FF08B`).
    pub(crate) fn fire_inviso(&mut self, id: u64, placement: ProjectileCoord, on_bridge: bool) {
        let Some(projectile) = self.projectiles.get_mut(&id) else {
            return;
        };
        projectile.position = placement;
        projectile.speed_leptons_per_frame = 0;
        projectile.velocity = if projectile.guidance.is_some() {
            ProjectileVelocity::new(1, 0, 0)
        } else {
            ProjectileVelocity::new(0, 0, 0)
        };
        if let Some(guidance) = projectile.guidance.as_mut() {
            guidance.max_speed = 0;
            guidance.fuse_reference = placement;
        }
        projectile.last_distance_half = proximity_initial_distance(placement, placement);
        projectile.on_bridge = on_bridge;
    }

    /// Advance every currently admitted projectile in ascending stable id.
    ///
    /// `target_positions` must contain live entity targets in lepton space.
    /// `terrain` supplies the current CellClass ground surface and live
    /// structural bit for stable cell targets; headless callers may omit it
    /// and receive the flat fallback.
    /// `collides_at` is a world-owned terrain/wall admission predicate for the
    /// candidate next coordinate; object collision remains a later port.
    pub fn advance(
        &mut self,
        binary_frame: u32,
        target_positions: &BTreeMap<u64, ProjectileCoord>,
        terrain: Option<&ResolvedTerrainGrid>,
        shared_cell_dummy: &SharedCellDummy,
        mut collides_at: impl FnMut(&Projectile, ProjectileCoord) -> Option<ProjectileCollisionResponse>,
    ) -> ProjectileAdvanceResult {
        let ids: Vec<u64> = self.projectiles.keys().copied().collect();
        self.advance_selected(
            &ids,
            binary_frame,
            |id| target_positions.get(&id).copied(),
            terrain,
            shared_cell_dummy,
            crate::rules::ruleset::GeneralRules::default().gravity,
            false,
            |projectile, candidate, phase| match phase {
                ProjectileCollisionPhase::Ordinary { .. } => None,
                ProjectileCollisionPhase::Shared => collides_at(projectile, candidate),
                ProjectileCollisionPhase::TargetLocation => None,
                ProjectileCollisionPhase::ImpactLadder => None,
            },
            true,
        )
    }

    pub(crate) fn advance_one(
        &mut self,
        id: u64,
        binary_frame: u32,
        target_position: impl FnMut(u64) -> Option<ProjectileCoord>,
        terrain: Option<&ResolvedTerrainGrid>,
        shared_cell_dummy: &SharedCellDummy,
        rules_gravity: i32,
        source_is_jumpjet: bool,
        collides_at: impl FnMut(
            &Projectile,
            ProjectileCoord,
            ProjectileCollisionPhase,
        ) -> Option<ProjectileCollisionResponse>,
    ) -> Option<ProjectileAdvanceResult> {
        if !self.projectiles.contains_key(&id) {
            return None;
        }
        Some(self.advance_selected(
            &[id],
            binary_frame,
            target_position,
            terrain,
            shared_cell_dummy,
            rules_gravity,
            source_is_jumpjet,
            collides_at,
            false,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn advance_selected(
        &mut self,
        ids: &[u64],
        binary_frame: u32,
        mut resolve_target_position: impl FnMut(u64) -> Option<ProjectileCoord>,
        terrain: Option<&ResolvedTerrainGrid>,
        shared_cell_dummy: &SharedCellDummy,
        rules_gravity: i32,
        source_is_jumpjet: bool,
        mut collides_at: impl FnMut(
            &Projectile,
            ProjectileCoord,
            ProjectileCollisionPhase,
        ) -> Option<ProjectileCollisionResponse>,
        remove_terminal: bool,
    ) -> ProjectileAdvanceResult {
        let mut result = ProjectileAdvanceResult::default();

        for &id in ids {
            let Some(projectile) = self.projectiles.get_mut(&id) else {
                continue;
            };

            let target_position = match projectile.target {
                ProjectileTarget::Cell { rx, ry } => cell_target_coord(terrain, rx, ry),
                // BulletClass::AI resolves a null AbstractClass target through
                // the process-global zero CoordStruct before steering, fuse,
                // collision, and reached-target decisions.
                ProjectileTarget::None => ProjectileCoord::new(0, 0, 0),
                ProjectileTarget::DummyCell => dummy_cell_target_coord(shared_cell_dummy),
                ProjectileTarget::Entity(target_id) => match resolve_target_position(target_id) {
                    Some(position) => {
                        if projectile.tracks_target {
                            projectile.last_target_position = position;
                        }
                        projectile.last_target_position
                    }
                    None => match projectile.target_expiry {
                        TargetExpiryPolicy::Expire => {
                            result.expired.push(id);
                            continue;
                        }
                        // `Arm=` is NOT consulted here. The detonation
                        // admission at `BulletClass::AI 0x00467C70` tests only
                        // the impact flag and the proximity-fuse result; the
                        // Arm counter lives inside `ProximityDetector::Check`
                        // and gates that one result alone.
                        TargetExpiryPolicy::DetonateAtLastKnown => {
                            result.detonations.push(detonation(
                                projectile,
                                projectile.last_target_position,
                                ProjectileDetonationReason::TargetExpired,
                            ));
                            continue;
                        }
                    },
                },
            };

            if let Some(fuse) = projectile.fuse_frames_remaining.as_mut() {
                if *fuse == 0 {
                    result.detonations.push(detonation(
                        projectile,
                        projectile.position,
                        ProjectileDetonationReason::Fuse,
                    ));
                    continue;
                }
                *fuse -= 1;
            }

            // YR BulletClass::Update @ 0x004666e0 advances the image bytes
            // before entering the trajectory portion of BulletClass::AI.
            projectile.visual.advance();

            let previous_position = projectile.position;
            let previous_velocity = projectile.velocity;
            // Native `local_198` — set by any arm that hits something. It is
            // the only value besides the fuse that admits a detonation.
            let mut impact_flag = false;
            // Native carries one impact flag; VERA keeps the reason beside it
            // purely as diagnostic provenance for the combat handoff.
            let mut impact_reason = ProjectileDetonationReason::Collision;
            // Native `local_190 == 2`: `FUN_00568350` says the new coordinate
            // left the map, and `LAB_00467B7A` removes the bullet through
            // `vtable+0x124(2)` and `ObjectClass::UnInit` with NO detonation.
            let mut left_the_map = false;
            let mut snap_impact: Option<ProjectileCoord> = None;
            let mut near_target = false;

            let mut ordinary_motion = None;
            let mut candidate = if let Some(mut guidance) = projectile.guidance {
                // ---- ARM C, `BulletClass::AI 0x004668D9`..`0x00466A33` ----
                // The speed ramp runs before any steering. `Bullet+0x110` is
                // MaxSpeed; the current magnitude is `ftol(|v|)`, which native
                // keeps integral from the 1.0 launch onward.
                let max_speed = i32::from(guidance.max_speed);
                let current_speed = i32::from(projectile.speed_leptons_per_frame);
                // `0x00466925`..`0x00466978`: with `CourseLockDuration == 0`
                // the lock clears once MaxSpeed reaches 40 (`CMP ...,0x28` /
                // `JGE`) or the magnitude comes within 0.5 of MaxSpeed;
                // otherwise a per-bullet counter runs the authored duration
                // out. Both releases latch, and both are monotone in the
                // ramping speed, so deriving them per frame is equivalent to
                // native's stored `IsCourseLocked` byte.
                let course_locked = if guidance.course_lock_duration == 0 {
                    max_speed < 40 && max_speed > current_speed
                } else {
                    guidance.frames_elapsed.saturating_add(1)
                        < u32::from(guidance.course_lock_duration)
                };
                // `0x0046699D`: a still-locked bullet with no authored
                // duration ramps at one lepton on even frames and none on odd.
                let acceleration = if course_locked && guidance.course_lock_duration == 0 {
                    i32::from(binary_frame.is_multiple_of(2))
                } else {
                    guidance.acceleration
                };
                let next_speed = if current_speed < max_speed {
                    (current_speed + acceleration).min(max_speed)
                } else if current_speed > max_speed {
                    // `0x00466A33`: overspeed sheds `Acceleration / 2` a frame
                    // and floors at zero.
                    (current_speed - acceleration / 2).max(0)
                } else {
                    current_speed
                };

                // `BulletClass::HomingTrack` steering. The turn word is zero
                // while course-locked (`bVar4 &= ~-(IsCourseLocked != 0)`).
                let target_distance = horizontal_distance(projectile.position, target_position);
                let phase = guidance
                    .frames_elapsed
                    .wrapping_add(u32::from(guidance.sidewinder_phase));
                let varied_rot = (sidewinder_cos(phase) * guidance.missile_rot_var
                    + guidance.missile_rot_var
                    + SimFixed::from_num(1))
                    * SimFixed::from_num(guidance.rot);
                let turn_word =
                    projectile_rot_turn_word_fixed(varied_rot, target_distance, course_locked);
                let desired_yaw = atan2_bam(
                    SimFixed::from_num(target_position.y - projectile.position.y),
                    SimFixed::from_num(target_position.x - projectile.position.x),
                );
                let yaw = step_toward_bam_inclusive(guidance.heading_bam, desired_yaw, turn_word);
                guidance.heading_bam = yaw;
                let horizontal_velocity = ProjectileVelocity::new(
                    (SimFixed::from_num(next_speed) * cos_bam(yaw)).to_num::<i32>(),
                    (SimFixed::from_num(next_speed) * sin_bam(yaw)).to_num::<i32>(),
                    0,
                );
                projectile.velocity.x = horizontal_velocity.x;
                projectile.velocity.y = horizontal_velocity.y;
                projectile.speed_leptons_per_frame =
                    next_speed.clamp(0, i32::from(u16::MAX)) as u16;
                guidance.frames_elapsed = guidance.frames_elapsed.wrapping_add(1);

                let step = projectile.velocity.integer_projection();
                let candidate = ProjectileCoord::new(
                    projectile.position.x.wrapping_add(step.x),
                    projectile.position.y.wrapping_add(step.y),
                    projectile.position.z.wrapping_add(step.z),
                );

                // gamemd-derived: `BulletClass::AI 0x00466DB1..0x00466E6B`.
                // The +0x1C8 receiver is ObjectClass::GetHeight @ 0x005F5F40
                // on the OLD object coordinate; HomingTrack has only updated
                // the stack candidate. Only an Inviso bullet can be OnBridge
                // (`Projectile::on_bridge`); it measures from the deck.
                let reached_distance = coord_distance(candidate, target_position);
                let old_height = previous_position
                    .z
                    .wrapping_sub(projectile_ground_z(
                        terrain,
                        shared_cell_dummy,
                        previous_position,
                    ))
                    .wrapping_sub(if projectile.on_bridge {
                        crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32
                    } else {
                        0
                    });
                let (admit_impact, snap_to_target) = homing_impact_admission(
                    reached_distance,
                    projectile.velocity,
                    old_height,
                    guidance.airburst,
                    target_position != ProjectileCoord::new(0, 0, 0),
                );
                if admit_impact {
                    impact_flag = true;
                    impact_reason = if old_height <= 0 {
                        ProjectileDetonationReason::Collision
                    } else {
                        ProjectileDetonationReason::ReachedTarget
                    };
                    if snap_to_target {
                        snap_impact = Some(target_position);
                    }
                }

                // The closing-rate heuristic. `delta` is how much closer this
                // step got; native accumulates it raw for the first 60 frames
                // and then decays, detonating inside the `[0, 60)` window when
                // the bullet is neither `Airburst` nor `VeryHigh`. It runs
                // only once the course lock has cleared.
                if !course_locked {
                    let delta = f64::from(
                        coord_distance(previous_position, target_position) - reached_distance,
                    );
                    let accumulator = f64::from_bits(guidance.closing_accumulator_bits);
                    if guidance.closing_frames < 60 {
                        guidance.closing_frames += 1;
                        guidance.closing_accumulator_bits = (accumulator + delta).to_bits();
                    } else {
                        let decayed = accumulator * 0.983_333_333_333_333_3 + delta;
                        guidance.closing_accumulator_bits = decayed.to_bits();
                        if (0.0..60.0).contains(&decayed)
                            && !guidance.airburst
                            && !guidance.very_high
                        {
                            impact_flag = true;
                            impact_reason = ProjectileDetonationReason::ReachedTarget;
                        }
                    }
                }

                projectile.guidance = Some(guidance);
                candidate
            } else {
                match projectile.trajectory {
                    ProjectileTrajectory::Straight => {
                        let candidate = step_toward(
                            projectile.position,
                            target_position,
                            i32::from(projectile.speed_leptons_per_frame),
                        );
                        projectile.velocity = ProjectileVelocity::new(
                            candidate.x - projectile.position.x,
                            candidate.y - projectile.position.y,
                            candidate.z - projectile.position.z,
                        );
                        candidate
                    }
                    ProjectileTrajectory::Ballistic => {
                        let motion = ordinary_motion_candidate(
                            projectile.position,
                            projectile.velocity,
                            rules_gravity,
                            projectile.collision.floater,
                        );
                        let candidate = motion.candidate_coord();
                        ordinary_motion = Some(motion);
                        candidate
                    }
                    // ---- ARM A, `BulletClass::AI 0x004671E0`..`0x00467390` ----
                    ProjectileTrajectory::Vertical {
                        detonation_altitude,
                        acceleration,
                        max_speed,
                    } => {
                        projectile.velocity =
                            vertical_velocity_ramp(projectile.velocity, acceleration, max_speed);
                        // This legacy display/cache field is not the ramp's authority.
                        projectile.speed_leptons_per_frame =
                            projectile_velocity_magnitude(projectile.velocity)
                                .clamp(0.0, f64::from(u16::MAX)) as u16;
                        let step = projectile.velocity.integer_projection();
                        let candidate = ProjectileCoord::new(
                            projectile.position.x.wrapping_add(step.x),
                            projectile.position.y.wrapping_add(step.y),
                            projectile.position.z.wrapping_add(step.z),
                        );
                        // `0x00467334`: `DetonationAltitude` is compared
                        // against the new WORLD z, not against an altitude
                        // above ground. Then a negative altitude, then the
                        // bridge-deck crossing test. No gravity is applied on
                        // this arm at all.
                        if candidate.z > detonation_altitude {
                            impact_flag = true;
                        } else if previous_position.z.wrapping_sub(projectile_ground_z(
                            terrain,
                            shared_cell_dummy,
                            previous_position,
                        )) < 0
                        {
                            impact_flag = true;
                        } else if let Some(surface) = bridge_surface_z(
                            terrain,
                            shared_cell_dummy,
                            previous_position,
                            candidate,
                        ) && projectile_bridge_crossing(
                            previous_position.z,
                            candidate.z,
                            surface,
                        ) != ProjectileBridgeCrossing::None
                        {
                            impact_flag = true;
                        }
                        candidate
                    }
                }
            };

            // Both ROT<1 arms enter the ordinary cell/nearest/map/slow tail.
            // Early returns preserve Bullet+E8 at tail entry. Ordinary scratch
            // gravity/reflection commits only at 467AB2; Vertical already
            // committed its ramp at 4672A3..4672B4 and skips that copyback.
            if projectile.guidance.is_none()
                && let Some(ProjectileCollisionResponse::Ordinary {
                    candidate: selected,
                    velocity,
                    impact,
                    near_target: near,
                    left_map,
                }) = collides_at(
                    projectile,
                    candidate,
                    ProjectileCollisionPhase::Ordinary {
                        persistent_velocity: if matches!(
                            projectile.trajectory,
                            ProjectileTrajectory::Vertical { .. }
                        ) {
                            projectile.velocity
                        } else {
                            previous_velocity
                        },
                        motion: ordinary_motion.unwrap_or_else(|| {
                            ProjectileCollisionMotion::from_coordinate(
                                candidate,
                                projectile.velocity,
                            )
                        }),
                    },
                )
            {
                candidate = selected;
                projectile.velocity = velocity;
                impact_flag |= impact;
                near_target = near;
                left_the_map = left_map;
            } else if projectile.guidance.is_none()
                && let Some(motion) = ordinary_motion
            {
                // Mapless store callbacks can omit the world tail; its normal
                // fallthrough commits the scratch here without a second owner.
                projectile.velocity = ProjectileVelocity::from_native(motion.velocity);
            }

            if left_the_map {
                result.expired.push(id);
                continue;
            }

            // 467B9E commits the selected candidate before the shared probe.
            // Every admitted impact bypasses 468BB0, irrespective of ROT.
            let selected_candidate = snap_impact.unwrap_or(candidate);
            projectile.position = selected_candidate;
            let collision_impact = (!impact_flag)
                .then(|| {
                    collides_at(
                        projectile,
                        selected_candidate,
                        ProjectileCollisionPhase::Shared,
                    )
                })
                .flatten()
                .map(|response| match response {
                    ProjectileCollisionResponse::TargetZClamp(impact) => impact,
                    ProjectileCollisionResponse::SlopeMatrixReflect { impact, velocity } => {
                        projectile.velocity = velocity;
                        impact
                    }
                    ProjectileCollisionResponse::Ordinary { .. }
                    | ProjectileCollisionResponse::ImpactLadder(_) => {
                        unreachable!("only the shared probe answers here")
                    }
                });
            if let Some(impact) = collision_impact {
                impact_flag = true;
                impact_reason = ProjectileDetonationReason::Collision;
                snap_impact = Some(impact);
            }

            let mut impact = snap_impact.unwrap_or(candidate);
            if impact_flag {
                // `0x00467BF0..0x00467C06`: the getter and, when negative,
                // setter both query the committed coordinate before the fuse.
                // The setter does not rewrite the stack coordinate used below.
                let floor = projectile_ground_z(terrain, shared_cell_dummy, impact);
                if impact.z.wrapping_sub(floor) < 0 {
                    impact.z = projectile_ground_z(terrain, shared_cell_dummy, impact);
                }
            }

            // `ProximityDetector::Check @ 0x004E11F0`. The reference is the
            // coordinate frozen at launch, the Arm counter gates this result
            // and nothing else. Dropping suppresses only detector-driven admission
            // after Check, preserving its watermark and collision-side result.
            let mut fuse_mode = 0;
            if projectile.ranged_fuse {
                let reference = projectile
                    .guidance
                    .map_or(projectile.last_target_position, |guidance| {
                        guidance.fuse_reference
                    });
                if projectile.arm_timer.expired(binary_frame as i32) {
                    let distance = proximity_check_distance(selected_candidate, reference);
                    let (mode, next_distance) =
                        ranged_fuse_distance_step(distance, projectile.last_distance_half);
                    projectile.last_distance_half = next_distance;
                    fuse_mode = mode;
                }
            }
            // `0x00467C3C..0x00467C66`: Bullet+0xB0 is the live firer,
            // whose type +0xD94 (JumpJet) changes overshoot mode 2 into 1.
            // Pointer cleanup can clear the source between flight visits.
            fuse_mode = projectile_fuse_mode(fuse_mode, source_is_jumpjet);

            if !projectile_impact_admitted(impact_flag, fuse_mode, projectile.collision.dropping) {
                projectile.position = candidate;
                projectile.previous_cell = ((candidate.x / 256) as i16, (candidate.y / 256) as i16);
                continue;
            }

            {
                // `0x00467CA9..0x00467E4D`: a mode-1 fuse unconditionally
                // selects target +0x48 after the Airburst/Inaccurate gates.
                // Re-read a shared dummy here: earlier ground lookups may
                // have stamped a different coordinate into that same target.
                let airburst = projectile
                    .guidance
                    .map_or(projectile.collision.airburst, |g| g.airburst);
                let inaccurate = projectile
                    .guidance
                    .map_or(projectile.collision.inaccurate, |g| g.inaccurate);
                if (fuse_mode == 1 || near_target) && !airburst && !inaccurate {
                    // +58 is fetched even in the unconditional mode-one arm.
                    let aim = match projectile.target {
                        ProjectileTarget::Entity(id) => match collides_at(
                            projectile,
                            selected_candidate,
                            ProjectileCollisionPhase::TargetLocation,
                        ) {
                            Some(ProjectileCollisionResponse::TargetZClamp(location)) => {
                                Some(location)
                            }
                            _ => resolve_target_position(id),
                        },
                        ProjectileTarget::Cell { rx, ry } => {
                            Some(cell_target_coord(terrain, rx, ry))
                        }
                        ProjectileTarget::DummyCell => {
                            Some(dummy_cell_target_coord(shared_cell_dummy))
                        }
                        ProjectileTarget::None => None,
                    };
                    let snap = aim.is_some_and(|aim| {
                        projectile_final_snap_admitted(
                            selected_candidate,
                            aim,
                            projectile.velocity,
                            fuse_mode,
                            near_target,
                        )
                    });
                    let location = match projectile.target {
                        _ if !snap => None,
                        ProjectileTarget::Entity(id) => resolve_target_position(id),
                        ProjectileTarget::Cell { rx, ry } => {
                            let mut location = cell_target_coord(terrain, rx, ry);
                            location.z = projectile_ground_z(terrain, shared_cell_dummy, location);
                            Some(location)
                        }
                        ProjectileTarget::DummyCell => {
                            let mut location = dummy_cell_target_coord(shared_cell_dummy);
                            let snapshot = shared_cell_dummy.snapshot();
                            location.z = crate::util::lepton::ground_height_leptons(
                                snapshot.level as u8,
                                snapshot.slope_type,
                                location.x,
                                location.y,
                            )
                            .expect("shared CellClass target must have a supported slope");
                            Some(location)
                        }
                        ProjectileTarget::None => None,
                    };
                    if let Some(location) = location {
                        impact = location;
                    }
                }
            }
            let reason = if impact_flag {
                impact_reason
            } else {
                ProjectileDetonationReason::Fuse
            };
            projectile.position = impact;
            // `0x00467FA2`: the AI hands its impact flag to the resolution
            // ladder, which picks where the detonation lands.
            let world =
                match collides_at(projectile, impact, ProjectileCollisionPhase::ImpactLadder) {
                    Some(ProjectileCollisionResponse::ImpactLadder(world)) => world,
                    _ => ImpactLadderWorld::default(),
                };
            let resolved =
                resolve_impact_coord(&projectile.impact_ladder_bullet(impact_flag), &world);
            result
                .detonations
                .push(detonation(projectile, resolved, reason));
        }

        if remove_terminal {
            for id in result.expired.iter().chain(
                result
                    .detonations
                    .iter()
                    .map(|detonation| &detonation.projectile_id),
            ) {
                self.projectiles.remove(id);
            }
        }
        result
    }
}

/// AI 467C70..467C84 preserves Check state even when Dropping suppresses admission.
fn projectile_impact_admitted(impact: bool, detector_mode: i32, dropping: bool) -> bool {
    impact || (!dropping && detector_mode != 0)
}

/// Detector result: 0 continues, 1 is within 0x20 half-distance, 2 passed nearby.
pub fn ranged_fuse_distance_step(distance_fistp: i32, last_distance: i32) -> (i32, i32) {
    let distance_half = (distance_fistp - (distance_fistp >> 31)) >> 1;
    if distance_half < 0x20 {
        (1, last_distance)
    } else if distance_half < 0x100 && distance_half > last_distance {
        (2, last_distance)
    } else {
        (0, distance_half)
    }
}

/// `Math__ftol(sqrt(dx*dx + dy*dy + dz*dz))` — the truncating straight-line
/// lepton distance native uses for the proximity fuse, the homing reach test
/// and the closing-rate accumulator.
pub(crate) fn coord_distance(a: ProjectileCoord, b: ProjectileCoord) -> i32 {
    native_vector_magnitude([
        a.x.wrapping_sub(b.x),
        a.z.wrapping_sub(b.z),
        a.y.wrapping_sub(b.y),
    ]) as i32
}

/// Detector Init 4E11A9 evaluates (dx²+dz²)+dy², then returns ftol's low DWORD.
fn proximity_initial_distance(a: ProjectileCoord, b: ProjectileCoord) -> i32 {
    crate::util::native_x87::distance_3d_leptons([a.x, a.z, a.y], [b.x, b.z, b.y])
}

/// Detector Check 4E1241 instead evaluates (dx²+dy²)+dz².
fn proximity_check_distance(a: ProjectileCoord, b: ProjectileCoord) -> i32 {
    crate::util::native_x87::distance_3d_leptons([a.x, a.y, a.z], [b.x, b.y, b.z])
}

/// `CellClass::GetGroundHeight @ 0x00578080`: signed /256, fixed-512
/// allocation lookup, and live shared-dummy fallback, evaluated at the original
/// lepton XY. The fallback stamp is observable by a retained DummyCell target.
pub(crate) fn projectile_ground_z(
    terrain: Option<&ResolvedTerrainGrid>,
    shared_cell_dummy: &SharedCellDummy,
    coord: ProjectileCoord,
) -> i32 {
    use crate::sim::cell_rect::{CellRef, get_cellclass_fallback_leptons};
    let (level, slope) = if let Some(terrain) = terrain {
        match get_cellclass_fallback_leptons(Some(terrain), coord.x, coord.y) {
            CellRef::Real(cell) => (cell.level, cell.slope_type),
            CellRef::Dummy { cell } => {
                let snapshot = cell.snapshot();
                (snapshot.level as u8, snapshot.slope_type)
            }
        }
    } else {
        shared_cell_dummy.stamp_coord(coord.x / 256, coord.y / 256);
        let snapshot = shared_cell_dummy.snapshot();
        (snapshot.level as u8, snapshot.slope_type)
    };
    crate::util::lepton::ground_height_leptons(level, slope, coord.x, coord.y)
        .expect("projectile ground probe requires a supported CellClass slope")
}

/// `BulletClass::AI 0x00466DB1..0x00466E6B`, with its already-computed
/// distance, represented velocity and OLD ObjectClass height as inputs.
/// The persistent velocity retains native double components. The current
/// integer XY homing producer remains an open trajectory discrepancy; this
/// predicate reads the vector rather than substituting a cached scalar speed.
fn homing_impact_admission(
    distance: i32,
    velocity: ProjectileVelocity,
    old_height: i32,
    airburst: bool,
    nonzero_target: bool,
) -> (bool, bool) {
    let half_speed = projectile_velocity_magnitude(velocity) * 0.5;
    let impact = f64::from(distance) <= half_speed || old_height <= 0;
    (
        impact,
        impact && old_height > 0 && !airburst && nonzero_target,
    )
}

fn projectile_fuse_mode(detector_mode: i32, source_is_jumpjet: bool) -> i32 {
    if source_is_jumpjet && detector_mode == 2 {
        1
    } else {
        detector_mode
    }
}

/// Common Bullet AI 467CEC..467E35. The midpoint's signed division occurs
/// before subtracting target Z; replacing it with `(candidate-target)/2`
/// differs on odd signed coordinates. The near-target bit divides the
/// integer distance by three, and only then compares with max(128, 2|v|).
pub(crate) fn projectile_final_snap_admitted(
    candidate: ProjectileCoord,
    target_aim: ProjectileCoord,
    velocity: ProjectileVelocity,
    fuse_mode: i32,
    near_target: bool,
) -> bool {
    let midpoint = target_aim.z.wrapping_add(candidate.z) / 2;
    let distance = coord_distance(
        ProjectileCoord::new(candidate.x, candidate.y, midpoint),
        target_aim,
    );
    let distance = if near_target { distance / 3 } else { distance };
    fuse_mode == 1
        || f64::from(distance) <= (projectile_velocity_magnitude(velocity) * 2.0).max(128.0)
}

/// Live Rules +16B8, with the Type +295 Floater override (4671B9/48ACF0).
pub(crate) fn projectile_gravity(
    rules_gravity: i32,
    floater: bool,
) -> crate::util::native_x87::NativeF64Bits {
    use crate::util::native_x87::{NativeF64Bits, X87Chop53};
    let gravity = X87Chop53::load_i32(rules_gravity);
    let gravity = if floater {
        X87Chop53::mul(gravity, X87Chop53::load_f64(NativeF64Bits::HALF).unwrap())
    } else {
        gravity
    };
    X87Chop53::store_f64(gravity).expect("finite Rules gravity")
}

/// Ordinary 46718F..467494: live gravity scratch, double coordinate sums,
/// then a separate integer projection. Persistent velocity commits in the tail.
fn ordinary_motion_candidate(
    position: ProjectileCoord,
    velocity: ProjectileVelocity,
    rules_gravity: i32,
    floater: bool,
) -> ProjectileCollisionMotion {
    use crate::util::native_x87::X87Chop53;
    let mut motion = ProjectileCollisionMotion::from_coordinate(position, velocity);
    // 4671B9/48ACF0 select live gravity, then 467415/467429
    // store the subtraction only in the ordinary scratch.
    let gravity = projectile_gravity(rules_gravity, floater);
    motion.velocity[2] = X87Chop53::store_f64(X87Chop53::sub(
        X87Chop53::load_f64(motion.velocity[2]).expect("finite velocity"),
        X87Chop53::load_f64(gravity).expect("finite gravity"),
    ))
    .expect("finite gravity-adjusted velocity");
    for (position, velocity) in motion.candidate.iter_mut().zip(motion.velocity) {
        *position = X87Chop53::store_f64(X87Chop53::add(
            X87Chop53::load_f64(*position).expect("finite position"),
            X87Chop53::load_f64(velocity).expect("finite velocity"),
        ))
        .expect("finite ordinary candidate");
    }
    motion
}

#[cfg(test)]
#[test]
fn original_ordinary_repeated_live_gravity_preserves_both_coordinate_versions() {
    use crate::util::native_x87::NativeF64Bits;
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../tools/projectile_oracle/ordinary_motion.json"
    ))
    .unwrap();
    for (index, row) in rows.iter().enumerate() {
        let mut velocity = ProjectileVelocity::from_native(std::array::from_fn(|i| {
            NativeF64Bits::from_bits(
                u64::from_str_radix(row["input_bits"][i].as_str().unwrap(), 16).unwrap(),
            )
        }));
        let o = &row["origin"];
        let mut position = ProjectileCoord::new(
            o[0].as_i64().unwrap() as i32,
            o[1].as_i64().unwrap() as i32,
            o[2].as_i64().unwrap() as i32,
        );
        for (frame, expected) in row["frames"].as_array().unwrap().iter().enumerate() {
            let motion = ordinary_motion_candidate(
                position,
                velocity,
                row["gravity_sequence"][frame].as_i64().unwrap() as i32,
                row["floater"].as_bool().unwrap(),
            );
            for (name, values) in [
                ("bits", motion.velocity),
                ("candidate_bits", motion.candidate),
            ] {
                let actual = values.map(|v| format!("{:016x}", v.bits()));
                let expected_bits: Vec<_> = expected[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                assert_eq!(
                    actual.as_slice(),
                    expected_bits.as_slice(),
                    "native ordinary row{index} frame{frame} {name}"
                );
            }
            let expected_position: [i32; 3] =
                std::array::from_fn(|i| expected["candidate"][i].as_i64().unwrap() as i32);
            position = motion.candidate_coord();
            assert_eq!(
                [position.x, position.y, position.z],
                expected_position,
                "native ordinary row{index} frame{frame}"
            );
            velocity = ProjectileVelocity::from_native(motion.velocity);
        }
    }
}

/// Vertical AI 4671E0..4672B4 scales Bullet+E8 in place. The first magnitude
/// uses (x*x+y*y)+z*z; the denominator uses (z*z+y*y)+x*x. Each native store
/// and the approximate sqrt's f32 return are retained explicitly.
fn vertical_velocity_ramp(
    velocity: ProjectileVelocity,
    acceleration: i32,
    max_speed: i32,
) -> ProjectileVelocity {
    use crate::util::native_x87::{NativeF64Bits, X87Chop53};
    let speed = X87Chop53::ftol_i64(
        X87Chop53::load_f64(NativeF64Bits::from_bits(
            projectile_velocity_magnitude(velocity).to_bits(),
        ))
        .unwrap(),
    )
    .expect("finite native Vertical speed conversion") as i32;
    if speed >= max_speed {
        return velocity;
    }
    let next_speed = speed.wrapping_add(acceleration);
    let mut components = velocity.native();
    if components
        .iter()
        .all(|component| component.bits() & 0x7fff_ffff_ffff_ffff == 0)
    {
        components[0] = NativeF64Bits::from_bits(100.0f64.to_bits());
    }
    let denominator =
        projectile_velocity_magnitude_double([components[2], components[1], components[0]]);
    let scale = X87Chop53::div(
        X87Chop53::load_i32(next_speed),
        X87Chop53::load_f64(NativeF64Bits::from_bits(denominator.to_bits())).unwrap(),
    )
    .expect("nonzero native Vertical denominator");
    ProjectileVelocity::from_native(components.map(|component| {
        X87Chop53::store_f64(X87Chop53::mul(
            scale,
            X87Chop53::load_f64(component).expect("finite Vertical velocity"),
        ))
        .expect("finite ramped Vertical velocity")
    }))
}

#[cfg(test)]
#[test]
fn original_vertical_repeated_motion_preserves_binary64_and_integer_add_order() {
    use crate::util::native_x87::NativeF64Bits;
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../tools/projectile_oracle/vertical_motion.json"
    ))
    .unwrap();
    for (index, row) in rows.iter().enumerate() {
        let mut velocity = ProjectileVelocity::from_native(std::array::from_fn(|i| {
            NativeF64Bits::from_bits(
                u64::from_str_radix(row["input_bits"][i].as_str().unwrap(), 16).unwrap(),
            )
        }));
        let mut position: [i32; 3] =
            std::array::from_fn(|i| row["origin"][i].as_i64().unwrap() as i32);
        for (frame, expected) in row["frames"].as_array().unwrap().iter().enumerate() {
            velocity = vertical_velocity_ramp(
                velocity,
                row["acceleration"].as_i64().unwrap() as i32,
                row["maximum"].as_i64().unwrap() as i32,
            );
            let step = velocity.integer_projection();
            for (position, step) in position.iter_mut().zip([step.x, step.y, step.z]) {
                *position = position.wrapping_add(step);
            }
            let actual_bits = velocity.native().map(|v| format!("{:016x}", v.bits()));
            let expected_bits: Vec<_> = expected["bits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(
                actual_bits.as_slice(),
                expected_bits.as_slice(),
                "native Vertical row{index} frame{frame}"
            );
            let expected_position: [i32; 3] =
                std::array::from_fn(|i| expected["candidate"][i].as_i64().unwrap() as i32);
            assert_eq!(
                position, expected_position,
                "native Vertical row{index} frame{frame}"
            );
        }
    }
}

pub(crate) fn projectile_velocity_magnitude(velocity: ProjectileVelocity) -> f64 {
    projectile_velocity_magnitude_double(velocity.native())
}

fn native_vector_magnitude(ordered_components: [i32; 3]) -> f64 {
    use crate::util::native_x87::NativeF64Bits;
    projectile_velocity_magnitude_double(
        ordered_components.map(|value| NativeF64Bits::from_bits(f64::from(value).to_bits())),
    )
}

pub(crate) fn projectile_velocity_magnitude_double(
    components: [crate::util::native_x87::NativeF64Bits; 3],
) -> f64 {
    use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};
    let terms = components.map(|component| {
        let value = X87Chop53::load_f64(component).expect("finite velocity");
        X87Chop53::mul(value, value)
    });
    let squared = X87Chop53::add(X87Chop53::add(terms[0], terms[1]), terms[2]);
    let bits = sqrt_approx_f32(squared).expect("finite represented projectile vector");
    f64::from(f32::from_bits(bits.bits()))
}

/// Vertical AI 467371..4673C2 queries candidate floor first, then candidate
/// Cell, then old Cell only if needed. The floor always belongs to candidate.
fn bridge_surface_z(
    terrain: Option<&ResolvedTerrainGrid>,
    shared_cell_dummy: &SharedCellDummy,
    previous: ProjectileCoord,
    candidate: ProjectileCoord,
) -> Option<i32> {
    use crate::sim::cell_rect::{CellRef, get_cellclass_fallback_leptons};
    let floor = projectile_ground_z(terrain, shared_cell_dummy, candidate);
    let structural = |coord: ProjectileCoord| -> bool {
        let cell = if terrain.is_some() {
            get_cellclass_fallback_leptons(terrain, coord.x, coord.y)
        } else {
            shared_cell_dummy.stamp_coord(coord.x / 256, coord.y / 256);
            CellRef::Dummy {
                cell: shared_cell_dummy.clone(),
            }
        };
        cell.bridge_flags_0x1180() & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0
    };
    (structural(candidate) || structural(previous))
        .then(|| floor.wrapping_add(crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32))
}

fn squared_horizontal_distance(a: ProjectileCoord, b: ProjectileCoord) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dy = i64::from(a.y) - i64::from(b.y);
    dx * dx + dy * dy
}

fn horizontal_distance(a: ProjectileCoord, b: ProjectileCoord) -> i32 {
    squared_horizontal_distance(a, b)
        .isqrt()
        .min(i64::from(i32::MAX)) as i32
}

/// YR `BulletClass_GetAnimFrame` @ 0x00468000.
pub fn projectile_shp_frame(projectile: &Projectile) -> u8 {
    if projectile.visual.anim_low != 0 || projectile.visual.anim_high != 0 {
        return projectile.visual.runtime_frame;
    }
    const FACING_FRAMES: [u8; 32] = [
        28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5,
        4, 3, 2, 1, 0, 31, 30, 29,
    ];
    let angle = (-f64::from_bits(projectile.velocity.y.bits()))
        .atan2(f64::from_bits(projectile.velocity.x.bits()));
    let word =
        (((angle - std::f64::consts::FRAC_PI_2) * -10430.06004058427).trunc() as i32) & 0xffff;
    let bucket = ((((word as u32 >> 10) + 1) >> 1) & 31) as usize;
    FACING_FRAMES[bucket]
}

// YR BulletClass::AI linkage: this is the bounded ordinary-flight rung; exact trajectory kernels remain separate.
fn step_toward(from: ProjectileCoord, target: ProjectileCoord, speed: i32) -> ProjectileCoord {
    if speed <= 0 || from == target {
        return from;
    }
    let dx = target.x - from.x;
    let dy = target.y - from.y;
    let dz = target.z - from.z;
    let max_delta = dx.abs().max(dy.abs()).max(dz.abs());
    if max_delta <= speed {
        return target;
    }
    ProjectileCoord::new(
        from.x + ((i64::from(dx) * i64::from(speed)) / i64::from(max_delta)) as i32,
        from.y + ((i64::from(dy) * i64::from(speed)) / i64::from(max_delta)) as i32,
        from.z + ((i64::from(dz) * i64::from(speed)) / i64::from(max_delta)) as i32,
    )
}

// YR BulletClass::Detonate linkage: only this handoff permits combat damage/effects.
fn detonation(
    projectile: &Projectile,
    impact: ProjectileCoord,
    reason: ProjectileDetonationReason,
) -> ProjectileDetonation {
    ProjectileDetonation {
        projectile_id: projectile.id,
        source_id: projectile.source_id,
        target: projectile.target,
        impact,
        payload: projectile.payload,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_impact_coord_matches_the_original() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../tools/projectile_oracle/impact_ladder.json"
        ))
        .unwrap();
        const TARGET: [i32; 3] = [5000, 6000, 416];
        let coord = |value: &serde_json::Value| {
            ProjectileCoord::new(
                value[0].as_i64().unwrap() as i32,
                value[1].as_i64().unwrap() as i32,
                value[2].as_i64().unwrap() as i32,
            )
        };
        let shifted = |offset: [i32; 3]| {
            ProjectileCoord::new(
                TARGET[0] + offset[0],
                TARGET[1] + offset[1],
                TARGET[2] + offset[2],
            )
        };
        let mut compared = 0;
        for row in &rows {
            let input = &row["input"];
            let flag = |key: &str| input[key].as_i64().unwrap() != 0;
            let location = coord(&input["location"]);
            // Cluster <= 0 never reaches DetonateAtCoord; VERA's cluster loop
            // (`world_receiver`) runs no iteration for it.
            if input["cluster"].as_i64().unwrap() <= 0 {
                assert!(row["detonation"].is_null());
                continue;
            }
            let target = input["target"].as_object().map(|target| {
                let field = |key: &str| target[key].as_i64().unwrap() as i32;
                let building = field("what_am_i") == 6;
                let foundation = &target["foundation"];
                let adjust = if building {
                    (foundation[0].as_i64().unwrap() + foundation[1].as_i64().unwrap()) as i32 * 64
                } else {
                    0
                };
                ImpactLadderTarget {
                    coords: shifted([0, 0, 0]),
                    aim: shifted([3, 5, 7]),
                    offset_coords: shifted([11, 13, 17]),
                    in_air: field("in_air") != 0,
                    ground_layer: field("layer") == 2,
                    distance: (coord_distance(location, shifted([0, 0, 0])) - adjust).max(0),
                    building_offset: building
                        && target["target_coord_offset"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|axis| axis.as_i64().unwrap() != 0),
                }
            });
            let bullet = ImpactLadderBullet {
                location,
                reference: coord(&input["reference"]),
                impact_flag: flag("impact_flag"),
                inaccurate: flag("inaccurate"),
                airburst: flag("airburst"),
                arcing: flag("arcing"),
                homing: input["rot"].as_i64().unwrap() > 0,
            };
            let world = ImpactLadderWorld {
                target,
                em_effect: flag("em_effect"),
            };
            assert_eq!(
                resolve_impact_coord(&bullet, &world),
                coord(&row["detonation"]),
                "{input}"
            );
            compared += 1;
        }
        assert_eq!(compared, 378);
    }

    #[test]
    fn homing_impact_admission_matches_executed_retail_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/projectile_oracle/homing_impact_vectors.json"
        ))
        .unwrap();
        for row in vectors["admissions"].as_array().unwrap() {
            let (admit, snap) = homing_impact_admission(
                row["distance"].as_i64().unwrap() as i32,
                ProjectileVelocity::new(
                    row["velocity"][0].as_i64().unwrap() as i32,
                    row["velocity"][1].as_i64().unwrap() as i32,
                    row["velocity"][2].as_i64().unwrap() as i32,
                ),
                row["height"].as_i64().unwrap() as i32,
                row["airburst"].as_bool().unwrap(),
                !row["empty_target"].as_bool().unwrap(),
            );
            assert_eq!(admit, row["impact"].as_bool().unwrap(), "{row}");
            let candidate = if snap {
                [640, 128, 624]
            } else {
                [504, 128, 207]
            };
            assert_eq!(serde_json::json!(candidate), row["candidate"], "{row}");
        }
    }

    #[test]
    fn homing_impact_handoff_matches_executed_retail_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/projectile_oracle/homing_impact_vectors.json"
        ))
        .unwrap();
        for row in vectors["handoffs"].as_array().unwrap() {
            let height = row["height"].as_i64().unwrap() as i32;
            let mode = row["fuse_mode"].as_i64().unwrap();
            let mut shot = guided_spawn(4, 0);
            shot.origin = ProjectileCoord::new(500, 128, 208 + height);
            shot.speed_leptons_per_frame = 4;
            shot.initial_target_position = ProjectileCoord::new(640, 128, 208);
            shot.target = if row["target_present"].as_bool().unwrap() {
                ProjectileTarget::Entity(42)
            } else {
                ProjectileTarget::None
            };
            shot.ranged_fuse = true;
            let guidance = shot.guidance.as_mut().unwrap();
            guidance.rot = 0; // Keep the externally supplied candidate fixed.
            guidance.airburst = row["airburst"].as_bool().unwrap();
            guidance.inaccurate = row["inaccurate"].as_bool().unwrap();
            guidance.fuse_reference = ProjectileCoord::new(
                match mode {
                    1 => 504,
                    2 => 604,
                    _ => 4000,
                },
                128,
                208 + height,
            );
            let dummy = SharedCellDummy::fresh();
            dummy.set_level_slope(2, 0);
            let mut store = ProjectileStore::new();
            let id = store.spawn(1, shot);
            store.projectiles.get_mut(&id).unwrap().last_distance_half = 0;
            let targets = BTreeMap::from([(42, ProjectileCoord::new(640, 128, 208))]);
            let result = store.advance(0, &targets, None, &dummy, |_, candidate| {
                Some(ProjectileCollisionResponse::TargetZClamp(candidate))
            });
            let impact = result.detonations[0].impact;
            assert_eq!(
                serde_json::json!([impact.x, impact.y, impact.z]),
                row["impact"],
                "{row}"
            );
        }
    }

    #[test]
    fn homing_source_fuse_mode_matches_executed_retail_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/projectile_oracle/homing_impact_vectors.json"
        ))
        .unwrap();
        for row in vectors["source_modes"].as_array().unwrap() {
            let mode = projectile_fuse_mode(
                row["detector_mode"].as_i64().unwrap() as i32,
                row["source_present"].as_bool().unwrap()
                    && row["source_jumpjet"].as_bool().unwrap(),
            );
            assert_eq!(
                i64::from(mode),
                row["admitted_mode"].as_i64().unwrap(),
                "{row}"
            );
        }
    }

    fn spawn(target: ProjectileTarget) -> ProjectileSpawn {
        ProjectileSpawn {
            flat: false,
            source_id: 7,
            origin: ProjectileCoord::new(0, 0, 0),
            target,
            initial_target_position: match target {
                ProjectileTarget::Entity(_) => ProjectileCoord::new(128, 0, 0),
                ProjectileTarget::Cell { rx, ry } => cell_target_coord(None, rx, ry),
                ProjectileTarget::None => ProjectileCoord::new(0, 0, 0),
                ProjectileTarget::DummyCell => ProjectileCoord::new(0, 0, 0),
            },
            payload: ProjectilePayload {
                base_damage: 40,
                warhead: InternedId::from_index(3),
                weapon: InternedId::from_index(4),
            },
            speed_leptons_per_frame: 64,
            velocity: ProjectileVelocity::new(64, 0, 0),
            trajectory: ProjectileTrajectory::Straight,
            guidance: None,
            visual: ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: true,
            target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
            collision: ProjectileCollisionPolicy::NONE,
        }
    }

    #[test]
    fn guided_rot_turn_word_matches_closed_vectors() {
        assert_eq!(projectile_rot_turn_word(4.0, 255, false), 0x0600);
        assert_eq!(projectile_rot_turn_word(4.0, 256, false), 0x0400);
        assert_eq!(projectile_rot_turn_word(4.0, 10, true), 0);
    }

    #[test]
    fn guided_rot_altitude_policy_matches_closed_very_high_vectors() {
        assert_eq!(
            projectile_rot_altitude_decision(false, false, false, false, 769, 2048, 104, -53, 1024),
            ProjectileRotAltitudeDecision {
                control_terrain_clearance: true,
                clearance_levels: 5,
                z_delta: 18,
                desired_pitch: Some(0x2000),
            }
        );
        assert_eq!(
            projectile_rot_altitude_decision(false, false, true, false, 1536, 0, 104, 100, 1024),
            ProjectileRotAltitudeDecision {
                control_terrain_clearance: false,
                clearance_levels: 10,
                z_delta: 0,
                desired_pitch: None,
            }
        );
        assert_eq!(
            projectile_rot_altitude_decision(false, true, false, false, 100, 0, 104, 53, 1024),
            ProjectileRotAltitudeDecision {
                control_terrain_clearance: true,
                clearance_levels: 10,
                z_delta: -18,
                desired_pitch: Some(0x4800),
            }
        );
        assert_eq!(
            projectile_rot_altitude_decision(false, false, false, true, 1000, 2048, 104, 100, 1024),
            ProjectileRotAltitudeDecision {
                control_terrain_clearance: false,
                clearance_levels: 5,
                z_delta: 0,
                desired_pitch: None,
            }
        );
        assert_eq!(
            projectile_rot_altitude_decision(false, false, false, false, 1000, 0, 104, 20, 1024),
            ProjectileRotAltitudeDecision {
                control_terrain_clearance: true,
                clearance_levels: 0,
                z_delta: 0,
                desired_pitch: Some(0x4000),
            }
        );
    }

    #[test]
    fn guided_projectile_turns_with_persisted_rot_state() {
        let mut store = ProjectileStore::new();
        let mut guided = spawn(ProjectileTarget::Cell { rx: 0, ry: 4 });
        guided.origin.z = 1; // Steering fixture: above the native ground-impact plane.
        guided.guidance = Some(ProjectileGuidance {
            rot: 4,
            missile_rot_var: SimFixed::from_num(0),
            course_lock_duration: 0,
            sidewinder_phase: 0,
            airburst: false,
            inaccurate: false,
            very_high: true,
            level: false,
            heading_bam: 0,
            max_speed: 0,
            acceleration: 3,
            fuse_reference: ProjectileCoord::new(0, 0, 0),
            closing_frames: 0,
            closing_accumulator_bits: 0,
            frames_elapsed: 0,
        });
        let id = store.spawn(1, guided);

        store.advance(
            0,
            &BTreeMap::new(),
            None,
            &SharedCellDummy::fresh(),
            |_, _| None,
        );

        let guided = store
            .get(id)
            .expect("guided projectile survives first turn");
        assert!(
            f64::from_bits(guided.velocity.y.bits()) > 0.0,
            "ROT turns toward the +Y target"
        );
        assert_eq!(guided.guidance.unwrap().frames_elapsed, 1);
    }

    #[test]
    fn gsi_04_01_dummy_target_reads_live_coord_level_and_slope() {
        let dummy = SharedCellDummy::fresh();
        dummy.set_level_slope(-1, 0);
        dummy.stamp_coord(0, 0);
        let flat = dummy_cell_target_coord(&dummy);
        assert_eq!(
            flat,
            ProjectileCoord::new(128, 128, -103),
            "CellClass::GetGroundHeight uses the verified 104-lepton domain"
        );

        dummy.set_level_slope(-1, 1);
        dummy.stamp_coord(4, 5);
        let target = dummy_cell_target_coord(&dummy);
        assert_eq!((target.x, target.y), (4 * 256 + 128, 5 * 256 + 128));
        assert_eq!(
            target.z,
            crate::util::lepton::ground_height_leptons(0xff, 1, target.x, target.y).unwrap()
        );
        assert_ne!(
            target.z, flat.z,
            "the live slope byte participates in dummy floor resolution"
        );

        dummy.stamp_coord(-2, 7);
        let moved = dummy_cell_target_coord(&dummy);
        assert_eq!((moved.x, moved.y), (-2 * 256 + 128, 7 * 256 + 128));
        assert_eq!(
            moved.z,
            crate::util::lepton::ground_height_leptons(0xff, 1, moved.x, moved.y).unwrap(),
            "coord stamps preserve and reuse the level/slope bytes"
        );
    }

    #[test]
    fn gsi_04_01_dummy_target_adds_native_high_bridge_height() {
        let dummy = SharedCellDummy::fresh();
        dummy.set_level_slope(2, 0);
        dummy.stamp_coord(4, 5);
        let ground = dummy_cell_target_coord(&dummy);

        dummy.apply_bridge_flag_slot(crate::map::bridge_facts::BridgeStampSlot::Anchor, true);
        let bridge = dummy_cell_target_coord(&dummy);

        assert_eq!(bridge.x, ground.x);
        assert_eq!(bridge.y, ground.y);
        assert_eq!(
            bridge.z - ground.z,
            crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32
        );
    }

    #[test]
    fn advance_preserves_stable_creation_order_and_delays_new_projectiles() {
        let mut store = ProjectileStore::new();
        let mut first_spawn = spawn(ProjectileTarget::Cell { rx: 0, ry: 0 });
        first_spawn.speed_leptons_per_frame = 256;
        let first = store.spawn(1, first_spawn);
        let second = store.spawn(2, spawn(ProjectileTarget::Cell { rx: 1, ry: 0 }));

        let result = store.advance(
            0,
            &BTreeMap::new(),
            None,
            &SharedCellDummy::fresh(),
            |projectile, candidate| {
                (projectile.id == first)
                    .then_some(ProjectileCollisionResponse::TargetZClamp(candidate))
            },
        );

        assert_eq!(
            result
                .detonations
                .iter()
                .map(|detonation| detonation.projectile_id)
                .collect::<Vec<_>>(),
            vec![first]
        );
        assert!(store.get(first).is_none());
        assert_ne!(
            store.get(second).unwrap().position,
            ProjectileCoord::new(0, 0, 0)
        );
    }

    #[test]
    fn homing_projectile_uses_current_target_position() {
        let mut store = ProjectileStore::new();
        let id = store.spawn(1, spawn(ProjectileTarget::Entity(42)));
        let targets = BTreeMap::from([(42, ProjectileCoord::new(128, 128, 0))]);

        store.advance(0, &targets, None, &SharedCellDummy::fresh(), |_, _| None);

        assert_eq!(
            store.get(id).unwrap().position,
            ProjectileCoord::new(64, 64, 0)
        );
    }

    #[test]
    fn target_expiry_detonates_at_last_known_position() {
        let mut store = ProjectileStore::new();
        let id = store.spawn(1, spawn(ProjectileTarget::Entity(42)));
        let targets = BTreeMap::from([(42, ProjectileCoord::new(128, 0, 0))]);
        store.advance(0, &targets, None, &SharedCellDummy::fresh(), |_, _| None);

        let result = store.advance(
            0,
            &BTreeMap::new(),
            None,
            &SharedCellDummy::fresh(),
            |_, _| None,
        );

        assert_eq!(result.detonations.len(), 1);
        assert_eq!(result.detonations[0].projectile_id, id);
        assert_eq!(
            result.detonations[0].impact,
            ProjectileCoord::new(128, 0, 0)
        );
        assert_eq!(
            result.detonations[0].reason,
            ProjectileDetonationReason::TargetExpired
        );
    }

    #[test]
    fn fuse_and_collision_are_deferred_detonations() {
        let mut store = ProjectileStore::new();
        let mut fused = spawn(ProjectileTarget::Cell { rx: 1, ry: 0 });
        fused.fuse_frames = Some(0);
        let fuse_id = store.spawn(1, fused);
        let collision_id = store.spawn(2, spawn(ProjectileTarget::Cell { rx: 1, ry: 0 }));

        let result = store.advance(
            0,
            &BTreeMap::new(),
            None,
            &SharedCellDummy::fresh(),
            |projectile, coord| {
                (projectile.id == collision_id)
                    .then_some(ProjectileCollisionResponse::TargetZClamp(coord))
            },
        );

        assert_eq!(result.detonations.len(), 2);
        assert_eq!(result.detonations[0].projectile_id, fuse_id);
        assert_eq!(
            result.detonations[0].reason,
            ProjectileDetonationReason::Fuse
        );
        assert_eq!(result.detonations[1].projectile_id, collision_id);
        assert_eq!(
            result.detonations[1].reason,
            ProjectileDetonationReason::Collision
        );
    }

    #[test]
    fn store_round_trips_through_snapshot_serialization() {
        let mut store = ProjectileStore::new();
        store.spawn(1, spawn(ProjectileTarget::Cell { rx: 1, ry: 0 }));

        let bytes = bincode::serialize(&store).unwrap();
        let restored: ProjectileStore = bincode::deserialize(&bytes).unwrap();

        assert_eq!(restored, store);
    }

    #[test]
    fn simulation_hash_and_save_preserve_pending_projectile() {
        let mut empty = crate::sim::world::Simulation::new();
        let mut sim = crate::sim::world::Simulation::new();
        let stable_id = sim.allocate_stable_id();
        sim.admit_projectile(stable_id, spawn(ProjectileTarget::Cell { rx: 1, ry: 0 }));
        // Native in-scenario load restarts Scenario RNG from Seed0. Normalize
        // both controls so this fixture isolates projectile persistence/hash.
        empty.scenario_rng = crate::sim::rng::SimRng::new(0);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        assert_ne!(empty.state_hash(), expected_hash);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "projectile", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("projectile snapshot should load")
            .sim;

        assert_eq!(restored.state_hash(), expected_hash);
        assert_eq!(restored.projectiles.len(), 1);
    }

    #[test]
    fn shp_facing_and_animation_match_yr_vectors() {
        let mut store = ProjectileStore::new();
        let id = store.spawn(1, spawn(ProjectileTarget::Cell { rx: 0, ry: 0 }));
        assert_eq!(projectile_shp_frame(store.get(id).unwrap()), 20);

        let projectile = store.projectiles.get_mut(&id).unwrap();
        projectile.visual = ProjectileVisualState {
            anim_low: 2,
            anim_high: 4,
            anim_rate: 3,
            runtime_frame: 4,
            runtime_countdown: 1,
        };
        projectile.visual.advance();
        assert_eq!(projectile.visual.runtime_frame, 2);
        assert_eq!(projectile.visual.runtime_countdown, 3);
    }

    #[test]
    fn projectile_load_timers_match_original_fire_save_load_and_check() {
        use crate::sim::timer::CdTimer;
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../tools/projectile_oracle/load_timers.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 877);
        for row in rows {
            let produced = &row["produced"]["arm"];
            let mut timer = CdTimer::from_raw(
                produced[0].as_i64().unwrap() as i32,
                produced[1].as_i64().unwrap() as i32,
            );
            let now = row["saved_frame"].as_u64().unwrap() as i32;
            let point = |name: &str| {
                let xyz = &row[name];
                ProjectileCoord::new(
                    xyz[0].as_i64().unwrap() as i32,
                    xyz[1].as_i64().unwrap() as i32,
                    xyz[2].as_i64().unwrap() as i32,
                )
            };
            let distance = proximity_check_distance(point("candidate"), point("reference"));
            let initial_watermark = proximity_initial_distance(point("origin"), point("reference"));
            assert_eq!(
                i64::from(initial_watermark),
                row["produced"]["watermark"].as_i64().unwrap()
            );
            let (mode, watermark) = if timer.expired(now) {
                ranged_fuse_distance_step(distance, initial_watermark)
            } else {
                (0, initial_watermark)
            };
            assert_eq!(
                i64::from(mode),
                row["before_mode"].as_i64().unwrap(),
                "{row}"
            );
            assert_eq!(
                i64::from(watermark),
                row["before"]["watermark"].as_i64().unwrap(),
                "{row}"
            );
            if !row["failed_load"].as_bool().unwrap() {
                timer.start(now, 0);
            }
            assert_eq!(
                i64::from(timer.start_frame()),
                row["loaded"]["arm"][0].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(timer.duration()),
                row["loaded"]["arm"][1].as_i64().unwrap()
            );
            let (mode, watermark) = if timer.expired(now) {
                ranged_fuse_distance_step(distance, watermark)
            } else {
                (0, watermark)
            };
            assert_eq!(
                i64::from(mode),
                row["after_mode"].as_i64().unwrap(),
                "{row}"
            );
            assert_eq!(
                i64::from(watermark),
                row["after"]["watermark"].as_i64().unwrap(),
                "{row}"
            );
            for admission in row["admission"].as_array().unwrap() {
                let mut mode = 0;
                let mut distance_mark = row["loaded"]["watermark"].as_i64().unwrap() as i32;
                if (admission["rot"].as_i64().unwrap() > 0 || admission["ranged"] == true)
                    && timer.expired(now)
                {
                    (mode, distance_mark) = ranged_fuse_distance_step(distance, distance_mark);
                }
                assert_eq!(
                    i64::from(mode),
                    admission["mode"].as_i64().unwrap(),
                    "{row}"
                );
                assert_eq!(
                    i64::from(distance_mark),
                    admission["watermark"].as_i64().unwrap(),
                    "{row}"
                );
                assert_eq!(
                    projectile_impact_admitted(
                        admission["impact"] == true,
                        mode,
                        admission["dropping"] == true
                    ),
                    admission["detonate"] == true,
                    "{row}"
                );
            }
        }
    }

    #[test]
    fn ranged_fuse_thresholds_match_executable_spec() {
        assert_eq!(ranged_fuse_distance_step(63, 999), (1, 999));
        assert_eq!(ranged_fuse_distance_step(64, 999), (0, 32));
        assert_eq!(ranged_fuse_distance_step(80, 10), (2, 10));
        assert_eq!(ranged_fuse_distance_step(80, 50), (0, 40));
    }

    #[test]
    fn closed_collision_predicate_vectors_match_yr() {
        assert_eq!(
            projectile_bridge_crossing(99, 100, 100),
            ProjectileBridgeCrossing::Up
        );
        assert_eq!(
            projectile_bridge_crossing(100, 99, 100),
            ProjectileBridgeCrossing::Down
        );
        assert_eq!(
            projectile_bridge_crossing(100, 100, 100),
            ProjectileBridgeCrossing::None
        );
        assert_eq!(
            projectile_cell_obstacle(249, 100, None, true, false, false, false, false),
            ProjectileCellObstacle::Overlay
        );
        assert_eq!(
            projectile_cell_obstacle(250, 100, None, true, false, false, false, false),
            ProjectileCellObstacle::None
        );
        assert_eq!(
            projectile_cell_obstacle(100, 100, Some(7), true, true, false, false, false),
            ProjectileCellObstacle::None
        );
    }

    #[test]
    fn burst_shrapnel_and_special_priority_match_closed_vectors() {
        assert_eq!(
            projectile_burst_plan(true, 8),
            ProjectileBurstPlan {
                detonation_count: 1,
                random_radius_rolls: 0,
                random_coordinate_calls: 0
            }
        );
        assert_eq!(
            projectile_burst_plan(false, 3),
            ProjectileBurstPlan {
                detonation_count: 3,
                random_radius_rolls: 3,
                random_coordinate_calls: 3
            }
        );
        assert_eq!(projectile_shrapnel_count(-8, true, 3), 5);
        assert_eq!(projectile_shrapnel_count(-8, false, 99), 3);
        assert_eq!(
            projectile_special_detonation_action(
                SpecialDetonationFlags {
                    mind_control: true,
                    nuke_maker: true,
                    ..SpecialDetonationFlags::default()
                },
                SpecialDetonationTarget::default()
            ),
            SpecialDetonationAction::MindControl
        );
    }

    /// `BulletClass::DetonateAtCoord @ 0x0046978e` is the chain's only
    /// conditional arm: `0x00469796` (flag), `0x004697a4` (`Target != 0`) and
    /// `0x004697b2` (`Target->What_Am_I() == 1`) all fall through to the
    /// BombDisarm test at `0x004699c4`, so a `DirectRocker=yes` warhead that
    /// misses a vehicle still runs ordinary damage. Airstrike only *looks*
    /// conditional in the decompile — `0x0046970d` is its sole fall-through
    /// and fires on the flag alone.
    #[test]
    fn direct_rocker_is_the_only_conditional_arm() {
        let rocker = SpecialDetonationFlags {
            direct_rocker: true,
            ..SpecialDetonationFlags::default()
        };
        assert_eq!(
            projectile_special_detonation_action(rocker, SpecialDetonationTarget { is_unit: true }),
            SpecialDetonationAction::DirectRocker
        );
        assert_eq!(
            projectile_special_detonation_action(
                rocker,
                SpecialDetonationTarget { is_unit: false }
            ),
            SpecialDetonationAction::OrdinaryDamage
        );

        // A non-vehicle target falls through to the NEXT test, not to the
        // tail: BombDisarm still claims the impact.
        assert_eq!(
            projectile_special_detonation_action(
                SpecialDetonationFlags {
                    direct_rocker: true,
                    bomb_disarm: true,
                    ..SpecialDetonationFlags::default()
                },
                SpecialDetonationTarget { is_unit: false }
            ),
            SpecialDetonationAction::BombDisarm
        );

        // Airstrike is entered on the flag alone, whatever the target is.
        for is_unit in [false, true] {
            assert_eq!(
                projectile_special_detonation_action(
                    SpecialDetonationFlags {
                        airstrike: true,
                        ..SpecialDetonationFlags::default()
                    },
                    SpecialDetonationTarget { is_unit }
                ),
                SpecialDetonationAction::Airstrike
            );
        }
    }

    /// `tools/projectile_oracle/launch_scatter.json`'s cluster-loop rows: the
    /// original loop (`0x00469008..0x00469091`) hands the first detonation the
    /// impact and every later one `0x0049F420(impact, distance, draw)`.
    #[test]
    fn native_cluster_loop_scatters_around_the_impact() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/projectile_oracle/launch_scatter.json"
        ))
        .unwrap();
        let rows = corpus["cluster_loop"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        let coord = |value: &serde_json::Value| {
            let axis = |index: usize| value[index].as_i64().unwrap() as i32;
            ProjectileCoord::new(axis(0), axis(1), axis(2))
        };
        for (index, row) in rows.iter().enumerate() {
            let input = &row["input"];
            let impact = coord(&input["impact"]);
            let draws = input["distances"]
                .as_array()
                .unwrap()
                .iter()
                .zip(input["raws"].as_array().unwrap());
            let expected: Vec<ProjectileCoord> = std::iter::once(impact)
                .chain(draws.map(|(distance, raw)| {
                    let (x, y) =
                        crate::sim::combat::inviso_scatter::random_direction_coord_for_byte(
                            (raw.as_u64().unwrap() & 0xff) as u8,
                            impact.x,
                            impact.y,
                            distance.as_i64().unwrap() as i32,
                        );
                    ProjectileCoord::new(x, y, impact.z)
                }))
                .take(input["cluster"].as_u64().unwrap() as usize)
                .collect();
            let native: Vec<ProjectileCoord> = row["detonations"]
                .as_array()
                .unwrap()
                .iter()
                .map(coord)
                .collect();
            assert_eq!(expected, native, "row {index}");
        }
    }

    #[test]
    fn cluster_and_random_shrapnel_consume_native_draw_counts() {
        let mut cluster_rng = SimRng::new(0x46_8d80);
        let mut cluster_reference = cluster_rng.clone();
        let _ = cluster_reference.next_range_u32_inclusive(256, 512);
        let _ = cluster_reference.next_u32();
        let coordinate = projectile_next_cluster_coord(
            ProjectileCoord::new(20 * 256 + 128, 20 * 256 + 128, 77),
            &mut cluster_rng,
        );
        assert_eq!(
            cluster_rng.logical_state(),
            cluster_reference.logical_state()
        );
        assert_eq!(coordinate.z, 77);

        let mut shrapnel_rng = SimRng::new(0x46_a310);
        let mut shrapnel_reference = shrapnel_rng.clone();
        let expected_x = shrapnel_reference.next_range_u32_inclusive(0, 4) as i32 - 2;
        let expected_y = shrapnel_reference.next_range_u32_inclusive(0, 4) as i32 - 2;
        assert_eq!(
            projectile_random_shrapnel_cell(10, 20, &mut shrapnel_rng),
            (10 + expected_x, 20 + expected_y)
        );
        assert_eq!(
            shrapnel_rng.logical_state(),
            shrapnel_reference.logical_state()
        );
    }

    fn guided_spawn(max_speed: u16, acceleration: i32) -> ProjectileSpawn {
        let mut spawn = spawn(ProjectileTarget::Entity(42));
        spawn.origin.z = 1; // Flight/fuse fixtures must not start in ground contact.
        spawn.speed_leptons_per_frame = 1;
        spawn.velocity = ProjectileVelocity::new(1, 0, 0);
        spawn.guidance = Some(ProjectileGuidance {
            rot: 4,
            missile_rot_var: SimFixed::from_num(0),
            course_lock_duration: 0,
            sidewinder_phase: 0,
            airburst: false,
            inaccurate: false,
            very_high: false,
            level: false,
            heading_bam: 0,
            frames_elapsed: 0,
            max_speed,
            acceleration,
            fuse_reference: ProjectileCoord::new(0, 0, 0),
            closing_frames: 0,
            closing_accumulator_bits: 0,
        });
        spawn
    }

    /// `TechnoClass::FireAt 0x006FEA3A` launches at one lepton per frame and
    /// `BulletClass::AI 0x004669CD` adds `Acceleration=` per frame up to
    /// `Bullet+0x110`. A `MaxSpeed >= 40` bullet clears the course lock on its
    /// first AI frame (`CMP [EBP+0x110],0x28` / `JGE` at `0x00466936`), so it
    /// gets the full acceleration immediately.
    #[test]
    fn gsi_08_06_fast_missile_ramps_from_one_by_full_acceleration() {
        let mut store = ProjectileStore::new();
        let id = store.spawn(1, guided_spawn(100, 3));
        let targets = BTreeMap::from([(42, ProjectileCoord::new(10_000, 0, 0))]);

        store.advance(0, &targets, None, &SharedCellDummy::fresh(), |_, _| None);
        let after_one = store.get(id).expect("missile still flying");
        assert_eq!(after_one.speed_leptons_per_frame, 4);
        assert_eq!(after_one.position, ProjectileCoord::new(4, 0, 1));

        store.advance(1, &targets, None, &SharedCellDummy::fresh(), |_, _| None);
        let after_two = store.get(id).expect("missile still flying");
        assert_eq!(after_two.speed_leptons_per_frame, 7);
        assert_eq!(after_two.position, ProjectileCoord::new(11, 0, 1));
    }

    /// With `CourseLockDuration = 0` and `MaxSpeed < 40` the lock survives, and
    /// `0x0046699D` replaces `Acceleration=` with one lepton on even global
    /// frames and none on odd ones. The parity is the GLOBAL frame counter's,
    /// not the bullet's own age.
    #[test]
    fn gsi_08_06_course_locked_missile_ramps_at_half_rate_on_even_frames() {
        let mut store = ProjectileStore::new();
        let id = store.spawn(1, guided_spawn(25, 3));
        let targets = BTreeMap::from([(42, ProjectileCoord::new(10_000, 0, 0))]);

        for (frame, expected) in [(0u32, 2u16), (1, 2), (2, 3), (3, 3), (4, 4)] {
            store.advance(frame, &targets, None, &SharedCellDummy::fresh(), |_, _| {
                None
            });
            assert_eq!(
                store
                    .get(id)
                    .expect("missile still flying")
                    .speed_leptons_per_frame,
                expected,
                "frame {frame}"
            );
        }
    }

    /// `BulletClass::AI 0x004671E0`: the `Vertical` arm applies no gravity,
    /// preserves the launch direction, ramps the magnitude by `Acceleration=`
    /// with no clamp on the crossing frame, and ends when the new world z
    /// passes `DetonationAltitude` (`0x00467334`).
    #[test]
    fn gsi_08_08_vertical_projectile_climbs_and_ends_at_detonation_altitude() {
        let mut store = ProjectileStore::new();
        let mut vertical = spawn(ProjectileTarget::Cell { rx: 0, ry: 0 });
        vertical.speed_leptons_per_frame = 1;
        vertical.velocity = ProjectileVelocity::new(0, 0, 1);
        vertical.origin = ProjectileCoord::new(0, 0, 0);
        vertical.trajectory = ProjectileTrajectory::Vertical {
            detonation_altitude: 10,
            acceleration: 1,
            max_speed: 50,
        };
        let id = store.spawn(1, vertical);

        let mut heights = Vec::new();
        for frame in 0..8u32 {
            let result = store.advance(
                frame,
                &BTreeMap::new(),
                None,
                &SharedCellDummy::fresh(),
                |_, _| None,
            );
            if let Some(detonation) = result.detonations.first() {
                assert_eq!(detonation.projectile_id, id);
                assert!(
                    detonation.impact.z > 10,
                    "the arm ends only once z passes DetonationAltitude"
                );
                assert_eq!(detonation.impact.x, 0, "no horizontal drift straight up");
                let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
                    "../../tools/projectile_oracle/vertical_motion.json"
                ))
                .unwrap();
                let row = rows
                    .iter()
                    .find(|row| {
                        row["origin"] == serde_json::json!([0, 0, 0]) && row["maximum"] == 50
                    })
                    .unwrap();
                let expected: Vec<i32> = row["frames"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|frame| frame["candidate"][2].as_i64().unwrap() as i32)
                    .take_while(|&z| z <= 10)
                    .collect();
                assert_eq!(
                    heights, expected,
                    "native successive ramp and integer projections"
                );
                return;
            }
            heights.push(store.get(id).expect("still climbing").position.z);
        }
        panic!("the vertical arm never reached DetonationAltitude");
    }

    /// `BulletClass::AI 0x00467C70` detonates on ANY impact flag with no `Arm`
    /// test at all — `Arm=` is written once into the ProximityDetector at
    /// `BulletClass::Fire 0x00468A5D` and gates only that fuse. An unarmed
    /// shot that hits a wall must explode, not vanish.
    #[test]
    fn gsi_08_07_unarmed_collision_detonates_instead_of_vanishing() {
        let mut store = ProjectileStore::new();
        let mut unarmed = spawn(ProjectileTarget::Cell { rx: 4, ry: 0 });
        unarmed.arm_frames = 5;
        let id = store.spawn(1, unarmed);

        let result = store.advance(
            0,
            &BTreeMap::new(),
            None,
            &SharedCellDummy::fresh(),
            |_, coord| Some(ProjectileCollisionResponse::TargetZClamp(coord)),
        );

        assert!(result.expired.is_empty(), "an unarmed hit is not a vanish");
        assert_eq!(result.detonations.len(), 1);
        assert_eq!(result.detonations[0].projectile_id, id);
        assert_eq!(
            result.detonations[0].reason,
            ProjectileDetonationReason::Collision
        );
    }

    /// `ProximityDetector::Check @ 0x004E11F0` measures against the coordinate
    /// `ProximityDetector::Setup @ 0x004E1130` froze at launch, never against
    /// the live target.
    #[test]
    fn gsi_08_07_proximity_fuse_measures_the_frozen_launch_reference() {
        let targets = BTreeMap::from([(42, ProjectileCoord::new(10_000, 0, 0))]);

        let mut frozen_near = ProjectileStore::new();
        let mut spawn_near = guided_spawn(1, 3);
        spawn_near.ranged_fuse = true;
        if let Some(guidance) = spawn_near.guidance.as_mut() {
            guidance.fuse_reference = ProjectileCoord::new(1, 0, 0);
        }
        frozen_near.spawn(1, spawn_near);
        let near = frozen_near.advance(0, &targets, None, &SharedCellDummy::fresh(), |_, _| None);
        assert_eq!(near.detonations.len(), 1, "the frozen reference is reached");
        assert_eq!(near.detonations[0].reason, ProjectileDetonationReason::Fuse);

        let mut frozen_far = ProjectileStore::new();
        let mut spawn_far = guided_spawn(1, 3);
        spawn_far.ranged_fuse = true;
        if let Some(guidance) = spawn_far.guidance.as_mut() {
            guidance.fuse_reference = ProjectileCoord::new(10_000, 0, 0);
        }
        frozen_far.spawn(1, spawn_far);
        let far = frozen_far.advance(0, &targets, None, &SharedCellDummy::fresh(), |_, _| None);
        assert!(
            far.detonations.is_empty(),
            "a distant frozen reference keeps the shot flying even though the \
             bullet is one lepton from where it started"
        );
    }

    /// Both `TechnoClass::FireAt` scatter arms consume exactly two Scenario
    /// draws — magnitude then angle — and both OFFSET the target delta,
    /// leaving z untouched.
    #[test]
    fn gsi_08_07_launch_scatter_consumes_two_draws_and_offsets_the_delta() {
        let mut rng = SimRng::new(0x6f_e7fe);
        let mut reference = rng.clone();
        let _ = reference.next_range_u32_inclusive(128, 256);
        let _ = reference.next_range_u32_inclusive(0, 0x7fff_fffe);

        let scattered = launch::fireat_launch_scatter(
            ProjectileCoord::new(1024, 0, 77),
            256,
            1280,
            false,
            &mut rng,
        );
        assert_eq!(rng.logical_state(), reference.logical_state());
        assert_eq!(scattered.z, 77, "z is carried through unchanged");
        let offset_x = scattered.x - 1024;
        let offset_y = scattered.y;
        let magnitude = ((offset_x * offset_x + offset_y * offset_y) as f64).sqrt();
        assert!(
            (128.0..=257.0).contains(&magnitude),
            "the plain arm draws its magnitude in [BallisticScatter/2, BallisticScatter], got {magnitude}"
        );

        // The flak arm scales by distance over weapon range, so a target at
        // exactly the weapon's range can be displaced by the full scatter.
        let mut flak_rng = SimRng::new(0x6f_e6ad);
        let mut flak_reference = flak_rng.clone();
        let _ = flak_reference.next_range_u32_inclusive(0, 256);
        let _ = flak_reference.next_range_u32_inclusive(0, 0x7fff_fffe);
        let flak = launch::fireat_launch_scatter(
            ProjectileCoord::new(1280, 0, 0),
            256,
            1280,
            true,
            &mut flak_rng,
        );
        assert_eq!(flak_rng.logical_state(), flak_reference.logical_state());
        let flak_magnitude = (((flak.x - 1280) * (flak.x - 1280) + flak.y * flak.y) as f64).sqrt();
        assert!(
            flak_magnitude <= 257.0,
            "at exactly one weapon range the flak arm cannot exceed BallisticScatter, got {flak_magnitude}"
        );
    }

    #[test]
    fn native_common_final_handoff_near_target_vectors() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/projectile_oracle/ordinary_collision_vectors.json"
        ))
        .unwrap();
        for row in oracle["final_handoffs"].as_array().unwrap() {
            let coord = |key: &str| {
                ProjectileCoord::new(
                    row[key][0].as_i64().unwrap() as i32,
                    row[key][1].as_i64().unwrap() as i32,
                    row[key][2].as_i64().unwrap() as i32,
                )
            };
            let v = coord("velocity");
            let mode = row["mode"].as_i64().unwrap() as i32;
            let near = row["near_target"].as_bool().unwrap();
            let actual = (mode == 1 || near)
                && !row["airburst"].as_bool().unwrap()
                && !row["inaccurate"].as_bool().unwrap()
                && row["target_present"].as_bool().unwrap()
                && projectile_final_snap_admitted(
                    coord("candidate"),
                    coord("target_aim"),
                    ProjectileVelocity::new(v.x, v.y, v.z),
                    mode,
                    near,
                );
            assert_eq!(
                actual,
                row["result"] == row["target_location"],
                "native finalhandoff: {row}"
            );
        }
    }

    #[test]
    fn native_slope_matrix_and_elastic_reflection_vectors() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/projectile_oracle/ordinary_collision_vectors.json"
        ))
        .unwrap();
        for (slope, row) in oracle["slope_matrices"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let actual = crate::sim::particles::spark_world::slope_matrix(slope as u8).unwrap();
            for (actual, expected) in actual.into_iter().zip(row.as_array().unwrap()) {
                assert_eq!(
                    u64::from(actual.bits()),
                    expected.as_u64().unwrap(),
                    "slope {slope}"
                );
            }
        }
        for row in oracle["reflections"].as_array().unwrap() {
            let components: Vec<i32> = row["velocity"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_i64().unwrap() as i32)
                .collect();
            use crate::util::native_x87::{NativeF64Bits, X87Chop53};
            let matrix = crate::sim::particles::spark_world::slope_matrix(
                row["slope"].as_u64().unwrap() as u8,
            )
            .unwrap();
            let vector = std::array::from_fn(|i| {
                X87Chop53::store_f32(X87Chop53::load_i32(components[i])).unwrap()
            });
            let elasticity = X87Chop53::store_f32(
                X87Chop53::load_f64(NativeF64Bits::from_bits(
                    row["elasticity"].as_f64().unwrap().to_bits(),
                ))
                .unwrap(),
            )
            .unwrap();
            let reflected = crate::sim::particles::spark::reflect_slope_vector_with_elasticity(
                vector, matrix, elasticity,
            )
            .unwrap();
            assert_eq!(
                serde_json::json!(reflected.map(|value| value.bits())),
                row["result_f32_bits"],
                "native f32 reflection: {row}"
            );
            let actual = projectile_slope_reflect_with_elasticity(
                ProjectileVelocity::new(components[0], components[1], components[2]),
                row["slope"].as_u64().unwrap() as u8,
                row["elasticity"].as_f64().unwrap().to_bits(),
            )
            .unwrap();
            assert_eq!(
                actual.native().map(|value| value.bits()),
                std::array::from_fn::<_, 3, _>(|i| f64::from(f32::from_bits(
                    row["result_f32_bits"][i].as_u64().unwrap() as u32
                ))
                .to_bits()),
                "{row}"
            );
        }
    }
}
