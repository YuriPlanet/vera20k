//! `BounceClass` — the rigid-body physics component that carries flying debris.
//!
//! gamemd-derived: `BounceClass::Init @ 0x004397E0`, `BounceClass::Update @
//! 0x00439B00`, `BounceClass::GetDrawingMatrix @ 0x004399E0` and the quaternion
//! helpers they call (`0x00646480`, `0x00645ED0`, `0x00646980`); native
//! execution in `tools/spatial_oracle/bounce_tumble.py`. RTTI `.?AVBounceClass@@` at `0x00845E38`. It is a component,
//! never a standalone object: it has no vtable use of its own, no `WhatAmI`, no
//! save slot, and is lifecycled entirely by its host. `VoxelAnimClass` embeds
//! one at `+0xB0`; `AnimClass` embeds one too, for the SHP debris that uses the
//! physics path.
//!
//! `Bouncer=yes` on an `AnimType` really does drive this component. The
//! constructor takes the `BounceClass::Init` fork on
//! `!Bouncer(+0x35A) && !IsMeteor(+0x356)` being false
//! (`AnimClass::AnimClass @ 0x00421EA0`), and
//! `AnimClass::ProcessBounceResult @ 0x00423930` calls `BounceClass::Update`.
//!
//! The separate system to keep apart from this one is
//! `AnimClass::BounceAI @ 0x00425670` — a 2D homing drift toward an attach
//! target, with no gravity and no elasticity, which does not call anything
//! here. `AnimClass::AI @ 0x00423AC0` gates that call on
//! `IsFlamingGuy=` (`AnimType+0x354`, one stock section: `FLAMEGUY`), NOT on
//! `Bouncer=`. An earlier revision of this header attributed `BounceAI` to
//! `Bouncer=`; the mechanism description was right and the key was wrong.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on util/ and the rest of sim/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use glam::IVec3;
use serde::{Deserialize, Serialize};

use crate::sim::rng::SimRng;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, NativeX87Error, X87Chop53, X87Ordering, X87Value, sqrt_approx_f32,
};

/// The 0x50-byte physics body, field for field.
///
/// The three configuration values are `double` and the position, velocity and
/// quaternions are `float`, exactly as native stores them. That split is not
/// cosmetic: gravity is subtracted from a float Z each tick from a double
/// source, and the stop test compares a float magnitude against `2.5`, so
/// widening or narrowing either side changes how far debris travels and how
/// many times it bounces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BounceState {
    /// `+0x00`. Bounce coefficient from the type's `Elasticity`.
    pub elasticity: NativeF64Bits,
    /// `+0x08`. Subtracted from `velocity_z` every tick. 1.4 for voxel debris,
    /// hardcoded in `VoxelAnimClass::Constructor`; 3.0 in the terrain-meteor
    /// helper `FUN_00439690`.
    pub gravity: NativeF64Bits,
    /// `+0x10`. Clamp threshold for the velocity vector's length. Zero disables
    /// the clamp, which is the voxel-debris case.
    pub angular_velocity_magnitude: NativeF64Bits,
    /// `+0x18`, `+0x1C`, `+0x20`. World position in leptons, carried as float.
    pub position: [NativeF32Bits; 3],
    /// `+0x24`, `+0x28`, `+0x2C`. Velocity in leptons per tick.
    pub velocity: [NativeF32Bits; 3],
    /// `+0x30` CurrentAngle, the drawn orientation as a quaternion
    /// `(x, y, z, w)`. `Init` sets it to `(+0, +0, +0, 1)`, and every
    /// [`Self::update`] multiplies it by [`Self::rotation`]. Only the draw
    /// reads it ([`Self::drawing_matrix`]).
    pub orientation: [NativeF32Bits; 4],
    /// `+0x40` AngularVelocity, the per-tick rotation `Init` builds from its
    /// three axis draws and its angle argument. A bounce negates its vector
    /// part, reversing the spin.
    pub rotation: [NativeF32Bits; 4],
}

/// What one `BounceClass::Update` reported, from the switch in
/// `AnimClass::ProcessBounceResult @ 0x00423930`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BounceOutcome {
    /// Still in flight, no contact this tick.
    Falling,
    /// Hit the ground this tick. The host may play its `BounceAnim` and apply
    /// the impact damage.
    Bounced,
    /// Came to rest — the velocity magnitude fell below the stop threshold. The
    /// host expires itself.
    Stopped,
}

impl BounceState {
    /// Fold the whole body, field bits in store order per axis. Every field
    /// is simulation state: the quaternions come off the shared stream in
    /// `Init` even though the physics never reads them back.
    pub(crate) fn hash_bits(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.elasticity.bits().hash(hasher);
        self.gravity.bits().hash(hasher);
        self.angular_velocity_magnitude.bits().hash(hasher);
        for axis in 0..3 {
            self.position[axis].bits().hash(hasher);
            self.velocity[axis].bits().hash(hasher);
        }
        for component in self.orientation.iter().chain(&self.rotation) {
            component.bits().hash(hasher);
        }
    }

    /// The host's world coordinate, in leptons.
    ///
    /// `VoxelAnimClass::AI` refreshes its `ObjectClass` coordinate from these
    /// floats through `CoordStruct::FromDoubles` every tick, so this is the
    /// authoritative position and the object's own coordinate is a copy.
    pub fn position_leptons(&self) -> IVec3 {
        IVec3::new(
            f32::from_bits(self.position[0].bits()) as i32,
            f32::from_bits(self.position[1].bits()) as i32,
            f32::from_bits(self.position[2].bits()) as i32,
        )
    }

    /// The velocity vector, for the host's own reads.
    #[cfg(test)]
    pub fn velocity_f32(&self) -> [f32; 3] {
        [
            f32::from_bits(self.velocity[0].bits()),
            f32::from_bits(self.velocity[1].bits()),
            f32::from_bits(self.velocity[2].bits()),
        ]
    }
}

/// `RandomRanged(-0xFFFF, 0xFFFF)`'s bounds, used three times by
/// [`BounceState::init`] to pick the tumble axis.
const AXIS_DRAW_MIN: i32 = -0xFFFF;
const AXIS_DRAW_MAX: i32 = 0xFFFF;

/// The `float` at `0x007E3560` each axis draw is scaled by — `1 / 65535`,
/// carried as bits because it is a float constant in the image, not a computed
/// reciprocal.
const AXIS_DRAW_SCALE: NativeF32Bits = NativeF32Bits::from_bits(0x3780_0080);

impl BounceState {
    /// `BounceClass::Init @ 0x004397E0`.
    ///
    /// Ghidra's recovered signature is one argument pair short. The function is
    /// `RET 0x28` — ten stack dwords — and the trailing pair is the per-tick
    /// rotation ANGLE, which `Quaternion_FromAxisAngle @ 0x00646480` reads as a
    /// single f32 stack argument narrowed from that double at `0x0043993C` /
    /// `0x00439950`. The corrected order is
    /// `(startCoord, elasticity, gravity, angVelMagnitude, startVelocity,
    /// rotAnglePerTick)`.
    ///
    /// Three `RandomRanged(-0xFFFF, 0xFFFF)` draws pick the tumble axis
    /// (`0x00439857`, `0x00439875`, `0x00439897`), each scaled by the float at
    /// `0x007E3560`. The axis takes them in reverse: X is the third draw and Z
    /// the first (`0x004398A3..0x004398CC`). The normalising sum associates
    /// `(y*y + z*z) + x*x` (`0x004398D0`). When the magnitude is exactly zero
    /// native divides nothing and uses the unnormalised triple as-is.
    ///
    /// The rotation is `Quaternion_FromAxisAngle @ 0x00646480` of that axis
    /// and the angle narrowed to a float (`0x0043993C..0x00439950`); the
    /// orientation is what `0x00646730` makes of the identity matrix
    /// `0x005AE860` builds ([`IDENTITY_QUATERNION`]).
    ///
    /// Every caller passes `angVelMagnitude` as two literal zeros
    /// (`VoxelAnimClass::Constructor` at `0x00749648`/`0x00749800`,
    /// `AnimClass::Constructor` at `0x00422648`,
    /// `BounceClass::SpawnRandom` at `0x004397C6`), so the field is always 0.0
    /// and the clamp it gates in `Update` is dead — see [`Self::update`].
    pub fn init(
        start_coord: IVec3,
        elasticity: NativeF64Bits,
        gravity: NativeF64Bits,
        angular_velocity_magnitude: NativeF64Bits,
        start_velocity: [NativeF32Bits; 3],
        rotation_angle_per_tick: NativeF64Bits,
        rng: &mut SimRng,
    ) -> Result<Self, NativeX87Error> {
        // `FILD` of the integer coordinate into the float position slots.
        let position = [
            X87Chop53::store_f32(X87Chop53::load_i32(start_coord.x))?,
            X87Chop53::store_f32(X87Chop53::load_i32(start_coord.y))?,
            X87Chop53::store_f32(X87Chop53::load_i32(start_coord.z))?,
        ];

        let scale = X87Chop53::load_f32(AXIS_DRAW_SCALE)?;
        let mut draws = [NativeF32Bits::POSITIVE_ZERO; 3];
        for component in draws.iter_mut() {
            let drawn = ranged_i32(rng, AXIS_DRAW_MIN, AXIS_DRAW_MAX);
            *component = X87Chop53::store_f32(X87Chop53::mul(X87Chop53::load_i32(drawn), scale))?;
        }
        let mut axis = [draws[2], draws[1], draws[0]];

        // `(y*y + z*z) + x*x`, then `Sqrt_Approx @ 0x004CAC40` — a table-driven
        // f32 approximation, not `FSQRT`.
        let x = X87Chop53::load_f32(axis[0])?;
        let y = X87Chop53::load_f32(axis[1])?;
        let z = X87Chop53::load_f32(axis[2])?;
        let magnitude = sqrt_approx_f32(X87Chop53::add(
            X87Chop53::add(X87Chop53::mul(y, y), X87Chop53::mul(z, z)),
            X87Chop53::mul(x, x),
        ))?;
        if magnitude != NativeF32Bits::POSITIVE_ZERO {
            let divisor = X87Chop53::load_f32(magnitude)?;
            for component in axis.iter_mut() {
                *component = X87Chop53::store_f32(X87Chop53::div(
                    X87Chop53::load_f32(*component)?,
                    divisor,
                )?)?;
            }
        }
        let angle = X87Chop53::store_f32(X87Chop53::load_f64(rotation_angle_per_tick)?)?;

        Ok(Self {
            elasticity,
            gravity,
            angular_velocity_magnitude,
            position,
            velocity: start_velocity,
            orientation: IDENTITY_QUATERNION,
            rotation: quaternion_from_axis_angle(axis, angle)?,
        })
    }

    /// `BounceClass::GetDrawingMatrix @ 0x004399E0`: the orientation as a
    /// rotation matrix, `Quaternion_ToMatrix @ 0x00646980`. Twelve floats, row
    /// by row, each row's fourth (translation) entry zero.
    pub fn drawing_matrix(&self) -> Result<[NativeF32Bits; 12], NativeX87Error> {
        quaternion_to_matrix(self.orientation)
    }
}

/// `(+0, +0, +0, 1)`: `Matrix3D → Quaternion @ 0x00646730` on the identity
/// `0x005AE860` builds takes its positive-trace arm, `Sqrt_Approx(3 + 1)` is
/// exactly 2, `w` is `2 * 0.5` and each vector term `(0 - 0) * 0.25`.
const IDENTITY_QUATERNION: [NativeF32Bits; 4] = [
    NativeF32Bits::POSITIVE_ZERO,
    NativeF32Bits::POSITIVE_ZERO,
    NativeF32Bits::POSITIVE_ZERO,
    NativeF32Bits::ONE,
];

/// The float `0.5` at `0x007E5168` that halves the angle.
const HALF: NativeF32Bits = NativeF32Bits::from_bits(0x3F00_0000);

fn load_all<const N: usize>(bits: [NativeF32Bits; N]) -> Result<[X87Value; N], NativeX87Error> {
    let mut values = [X87Chop53::load_i32(0); N];
    for (value, bits) in values.iter_mut().zip(bits) {
        *value = X87Chop53::load_f32(bits)?;
    }
    Ok(values)
}

/// An `FSTP float` the code reloads later.
fn stored(value: X87Value) -> Result<X87Value, NativeX87Error> {
    X87Chop53::load_f32(X87Chop53::store_f32(value)?)
}

/// `Quaternion_FromAxisAngle @ 0x00646480`: `(axis * sin(a/2), cos(a/2))`.
///
/// It normalises the axis again by `Sqrt_Approx` of `(z*z + y*y) + x*x`
/// (`0x00646493..0x006464BA`), keeping the raw axis when that is zero, and
/// reads both trig values from the retail table (`Math__SinFromTable @
/// 0x004CACB0`, `Math__CosFromTable @ 0x004CAD00`) at the halved angle, which
/// it passes as a double.
fn quaternion_from_axis_angle(
    axis: [NativeF32Bits; 3],
    angle: NativeF32Bits,
) -> Result<[NativeF32Bits; 4], NativeX87Error> {
    let [x, y, z] = load_all(axis)?;
    let length = sqrt_approx_f32(X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(z, z), X87Chop53::mul(y, y)),
        X87Chop53::mul(x, x),
    ))?;
    let mut normal = axis;
    if length != NativeF32Bits::POSITIVE_ZERO {
        let divisor = X87Chop53::load_f32(length)?;
        for component in normal.iter_mut() {
            *component =
                X87Chop53::store_f32(X87Chop53::div(X87Chop53::load_f32(*component)?, divisor)?)?;
        }
    }
    let half = X87Chop53::load_f64(X87Chop53::store_f64(X87Chop53::mul(
        X87Chop53::load_f32(angle)?,
        X87Chop53::load_f32(HALF)?,
    ))?)?;
    let table = crate::map::retail_trig::TrigTable::embedded();
    let sin = table.sin_from_table(half);
    let cos = table.cos_from_table(half);
    let [nx, ny, nz] = load_all(normal)?;
    Ok([
        X87Chop53::store_f32(X87Chop53::mul(nx, sin))?,
        X87Chop53::store_f32(X87Chop53::mul(ny, sin))?,
        X87Chop53::store_f32(X87Chop53::mul(nz, sin))?,
        X87Chop53::store_f32(cos)?,
    ])
}

/// `0x00645ED0`: the Hamilton product `a * b`, divided by its SQUARED norm
/// unless that is zero. Read from the disassembly, with every float store the
/// code makes along the way: the decompiler regroups the sums.
fn quaternion_multiply(
    a: [NativeF32Bits; 4],
    b: [NativeF32Bits; 4],
) -> Result<[NativeF32Bits; 4], NativeX87Error> {
    let [ax, ay, az, aw] = load_all(a)?;
    let [bx, by, bz, bw] = load_all(b)?;
    let mul = X87Chop53::mul;
    let ax_bw = mul(ax, bw);
    let ay_bw = stored(mul(ay, bw))?;
    let az_bw = stored(mul(az, bw))?;
    let aw_bx = stored(mul(aw, bx))?;
    let by_aw = stored(mul(by, aw))?;
    let bz_aw = stored(mul(bz, aw))?;
    let cross_x = X87Chop53::sub(mul(ay, bz), mul(az, by));
    let cross_y = stored(X87Chop53::sub(mul(az, bx), mul(bz, ax)))?;
    let cross_z = stored(X87Chop53::sub(mul(by, ax), mul(ay, bx)))?;
    let product = [
        X87Chop53::store_f32(X87Chop53::add(X87Chop53::add(cross_x, aw_bx), ax_bw))?,
        X87Chop53::store_f32(X87Chop53::add(X87Chop53::add(ay_bw, by_aw), cross_y))?,
        X87Chop53::store_f32(X87Chop53::add(X87Chop53::add(az_bw, bz_aw), cross_z))?,
        X87Chop53::store_f32(X87Chop53::sub(
            mul(aw, bw),
            X87Chop53::add(X87Chop53::add(mul(az, bz), mul(ay, by)), mul(ax, bx)),
        ))?,
    ];
    let [rx, ry, rz, rw] = load_all(product)?;
    let norm = X87Chop53::add(
        X87Chop53::add(X87Chop53::add(mul(rz, rz), mul(ry, ry)), mul(rw, rw)),
        mul(rx, rx),
    );
    if X87Chop53::compare(norm, X87Chop53::load_i32(0)) == X87Ordering::Equal {
        return Ok(product);
    }
    let mut out = product;
    for component in out.iter_mut() {
        *component = X87Chop53::store_f32(X87Chop53::div(X87Chop53::load_f32(*component)?, norm)?)?;
    }
    Ok(out)
}

/// `Quaternion_ToMatrix @ 0x00646980`, rows `[m0, m1, m2, 0]`. Some products
/// are stored as floats before their sums and some stay on the stack, which
/// is why the rows are not written symmetrically.
fn quaternion_to_matrix(q: [NativeF32Bits; 4]) -> Result<[NativeF32Bits; 12], NativeX87Error> {
    let [x, y, z, w] = load_all(q)?;
    let mul = X87Chop53::mul;
    let one = X87Chop53::load_f32(NativeF32Bits::ONE)?;
    let double = |value: X87Value| X87Chop53::add(value, value);
    let zz = mul(z, z);
    let yy = stored(mul(y, y))?;
    let xx = stored(mul(x, x))?;
    let yx = mul(y, x);
    let wz = mul(w, z);
    let wy = stored(mul(w, y))?;
    let xz = mul(x, z);
    let yz = mul(y, z);
    let wx = mul(w, x);
    let zero = NativeF32Bits::POSITIVE_ZERO;
    let entry = X87Chop53::store_f32;
    Ok([
        entry(X87Chop53::sub(one, double(X87Chop53::add(yy, zz))))?,
        entry(double(X87Chop53::sub(yx, wz)))?,
        entry(double(X87Chop53::add(xz, wy)))?,
        zero,
        entry(double(X87Chop53::add(wz, yx)))?,
        entry(X87Chop53::sub(one, double(X87Chop53::add(xx, zz))))?,
        entry(double(X87Chop53::sub(yz, wx)))?,
        zero,
        entry(double(X87Chop53::sub(stored(xz)?, wy)))?,
        entry(double(X87Chop53::add(wx, yz)))?,
        entry(X87Chop53::sub(one, double(X87Chop53::add(xx, yy))))?,
        zero,
    ])
}

/// The verified `BounceClass::Update @ 0x00439B00` contract, recorded ahead of
/// its implementation so the next slice has a spec rather than a re-read.
///
/// Per tick, in order:
/// 1. Snapshot position and velocity — the slope branch below rolls back to it.
/// 2. `Velocity.Z -= Gravity`.
/// 3. **The angular clamp is dead and must not be ported.** Raw bytes at
///    `0x00439B7A` show `FLD ST(0)` / `FDIVRP` on an empty-above stack, i.e.
///    the scale is `|V| / |V|` = exactly 1.0 — a gamemd bug; the intent was
///    `AVM / |V|`. It is moot regardless: all three `Init` callers pass
///    `angVelMagnitude` as two literal zeros, so `0.0 < AVM` is false
///    everywhere in the image.
/// 4. `Position += Velocity` (`FUN_0043A100 @ 0x0043A100`), between the pre-
///    and post-move coordinate captures.
/// 5. Ground lookup, then the deck plane `groundHeight + DAT_0089C76C`.
///    `bVar4`/`bVar5` are DECK-CROSSING DIRECTION, not height comparisons:
///    `bVar5` = rose through the deck, `bVar4` = fell through it.
/// 6. Snap ladder: `bVar4` -> `Position.Z = deck`; `bVar5` -> `deck - 20`;
///    else the building/wall arm's `bVar6` -> the clamp; else **return
///    `Falling` with no reflection at all**. The clamp is
///    `if (groundHeight - 100 < Position.Z) Position.Z = groundHeight`.
///    The `150.0f` proximity gate (`0x007E3DA8`) gates ONLY the building/wall
///    lookup, not general ground contact.
/// 7. The reflection, from `0x00439D91`:
///    ```text
///    M1 = MATRIX_TABLE[cell(newPos).ramp]   // VXL_GetFacingMatrix @ 0x007559B0
///    M2 = M1 transposed                      // FUN_005AFC20, rotation only
///    v  = (Vx, -Vy, Vz)
///    v  = M2 * v                             // translation column ignored
///    v  = f32(Elasticity) * v                // all three components
///    v.Z = -v.Z
///    q  = M1 * v
///    Velocity = (q.X, -q.Y, q.Z)
///    ```
///    The matrix-vector rows do NOT associate alike — row 0 pairs the y/z terms
///    first, rows 1 and 2 pair y/x first. The decompiler canonicalises that
///    away, so it must be read from disassembly.
/// 8. Slope re-bounce, when `cell(new).level - cell(old).level >= 2` and the
///    entry-tick velocity conditions against `-0.0002` / `-0.0003` hold: roll
///    back to the snapshot, transform the velocity by one of four planar
///    mirror matrices (`FUN_00755C60 @ 0x00755C60`), then scale all three
///    components by `Elasticity` — as a `double` here, unlike step 7's f32
///    narrowing. This REPLACES step 7 rather than following it.
/// 9. Stop test — and it is not `|Velocity|`. `FUN_00439A10 @ 0x00439A10`
///    `ftol`-truncates each velocity component to an integer, uses a
///    PREDICTIVE Z of `heightAboveGround * Gravity + Velocity.Z`, sums
///    `(vz*vz + vx*vx) + vy*vy`, and `ftol`-truncates the `Sqrt_Approx` result
///    — so the magnitude is always an integer and `>= 2.5` means `>= 3`.
///    Below it, return `Stopped`; otherwise `Bounced` or `Falling`.
///
/// Two consequences worth knowing before implementing:
///
/// - `Elasticity = 0` stops on FIRST ground contact and exits via `Stopped`,
///   not `Bounced`, so it never plays its `BounceAnim`. Every stock
///   `[VoxelAnims]` type is `Elasticity=0` except `[TIRE]` at `0.8` — so the
///   reflection produces real motion for exactly one stock type, and `PIECE`,
///   `GASTANK` and the other seven land dead. The death-debris producer never
///   reaches them: all 36 stock `DebrisTypes=` lines name `TIRE`, so no stock
///   section throws `PIECE` or `GASTANK` on death.
/// - The building/wall arm's first rejection — `type+0x16BF` (`LaserFence=`)
///   with `building+0x618 >= 8` — is Tiberian Sun legacy. No stock RA2/YR
///   building authors `LaserFence=`, and the string occurs exactly once in
///   `rulesmd.ini` — line 3652, a comment describing the neighbouring
///   `LaserFencePost=` key, which nothing authors either. Do not port it as a
///   default.
///
/// Ground/deck queries are delivered by the current
/// Phase 3 Bounce ground-height comparison. The cliff mirror remains a
/// separate residual below.
const FLAT_RAMP: u8 = 0;

/// The drop applied when the body rises back through the deck plane
/// (`0x00439D5D`, `local_118 + -0x14`).
const DECK_RISE_DROP_LEPTONS: i32 = 20;

/// The window below the ground height inside which the non-deck arm clamps the
/// position up to the surface (`0x00439D71`, `local_114 + -100`).
const GROUND_CLAMP_WINDOW_LEPTONS: i32 = 100;

/// The proximity gate on the building/wall lookup alone (`0x007E3DA8`).
const BUILDING_LOOKUP_PROXIMITY_LEPTONS: f32 = 150.0;

/// The stop threshold. `FCOMP double [0x007E3D80]` at `0x0043A08C` compares the
/// magnitude against the double at that address, whose bytes are `2.5`, and the
/// `TEST AH,1 / JNZ` pair below it returns 2 (`Stopped`) only when the compare
/// set C0 — i.e. strictly BELOW the threshold. `FUN_00439A10` returns an
/// `ftol`-truncated integer, so that is `< 3`.
const STOP_MAGNITUDE_THRESHOLD: f32 = 2.5;

/// The cliff-face entry test's pre-tick Z-velocity bounds (`0x007E3DA0`,
/// `0x007E3D98`, doubles) and its one-lepton margin (`0x007E2AC8`, float).
const CLIFF_FALLING_VZ: NativeF64Bits = NativeF64Bits::from_bits((-0.0002f64).to_bits());
const CLIFF_RISING_VZ: NativeF64Bits = NativeF64Bits::from_bits((-0.0003f64).to_bits());
const CLIFF_RISE_MARGIN: NativeF32Bits = NativeF32Bits::from_bits(1.0f32.to_bits());
/// A cliff face is a step of at least this many height levels
/// (`CMP EDX, 0x2`, `0x00439F5E`).
const CLIFF_FACE_LEVELS: i32 = 2;

/// The map facts one `BounceClass::Update` reads.
///
/// `BounceClass` itself calls `CellClass::GetGroundHeight`,
/// `MapClass::Get_CellClass_At_Coord`, `Look_up_building_in_cell` and
/// `CellClass::IsWallConnectableInDirection` directly. Those live in the world,
/// so the integrator takes them through this port rather than reaching for a
/// map grid — `sim/` keeps the physics pure and the caller supplies terrain.
pub trait BounceTerrain {
    /// Retain native Cell identity across later lookups, including shared dummy.
    type Cell;
    fn select_cell(&self, coord: IVec3) -> Self::Cell;
    fn selected_is_bridge(&self, cell: &Self::Cell) -> bool;
    /// `CellClass::GetGroundHeight` at the given world coordinate, in leptons.
    fn ground_height_leptons(&self, coord: IVec3) -> i32;
    /// `CellClass+0x140 & 0x100` — the cell carries a bridge deck.
    fn is_bridge_cell(&self, coord: IVec3) -> bool {
        self.selected_is_bridge(&self.select_cell(coord))
    }
    /// `CellClass+0x11B`, the cell's height level. Only the difference between
    /// the pre- and post-move cells is read.
    fn cell_height_level(&self, coord: IVec3) -> i32;
    /// The cell's ramp index, which selects the reflection matrix.
    fn ramp(&self, coord: IVec3) -> u8;
    /// `Look_up_building_in_cell` non-null, or
    /// `CellClass::IsWallConnectableInDirection(-1, -1)`.
    fn has_bounce_surface(&self, cell: &Self::Cell) -> bool;
    /// `CellClass+0xEC == 2` — LandType WATER. `BounceClass::Update` itself
    /// never reads it; the hosts do, both at the death gate and on a landing.
    fn is_water(&self, coord: IVec3) -> bool;
}

/// `ftol` each float position component into the `CoordStruct` native builds
/// with `FUN_00437090` before and after the move.
fn ftol_coord(position: [NativeF32Bits; 3]) -> Result<IVec3, NativeX87Error> {
    Ok(IVec3::new(
        X87Chop53::ftol_i64(X87Chop53::load_f32(position[0])?)? as i32,
        X87Chop53::ftol_i64(X87Chop53::load_f32(position[1])?)? as i32,
        X87Chop53::ftol_i64(X87Chop53::load_f32(position[2])?)? as i32,
    ))
}

fn f32_of(bits: NativeF32Bits) -> f32 {
    f32::from_bits(bits.bits())
}

impl BounceState {
    /// `BounceClass::Update`'s reflection block, `0x00439D91`..`0x00439E89`.
    ///
    /// gamemd-derived, and the operand order matters at every step:
    /// ```text
    /// M1 = MATRIX_TABLE_0x00B45188[cell(newPos).ramp]   // VXL_GetFacingMatrix @ 0x007559B0
    /// M2 = inverse_rigid(M1)                            // FUN_005AFC20
    /// v  = (Velocity.X, -Velocity.Y, Velocity.Z)        // FUN_0043A0B0
    /// v  = M2 * v                                       // FUN_005AF4D0, rotation only
    /// v  = f32(Elasticity) * v                          // FUN_0043A0D0, ALL THREE components
    /// v.Z = -v.Z
    /// q  = M1 * v
    /// Velocity = (q.X, -q.Y, q.Z)
    /// ```
    /// Y is negated on the way in and on the way out; Z is the axis actually
    /// reflected, in surface space. Elasticity is applied BEFORE the Z flip, so
    /// it damps the tangential and normal components alike, and it is the f32
    /// truncation of the stored double — narrowed at `0x00439E2A`, before the
    /// first transform overwrites that stack slot.
    ///
    /// `FUN_005AF4D0` reads only the rotation columns; the translation column
    /// (`0x0C`/`0x1C`/`0x2C`) is never touched, so `M2 ≡ M1ᵀ` here and the block
    /// is a rotate-into-surface-space, reflect, rotate-back round trip.
    ///
    /// RESIDUAL (GSI-05.14) — only the FLAT ramp is modelled. On ramp 0 the
    /// table entry is the identity, so both transforms drop out and the whole
    /// block collapses to `Velocity = (e·Vx, e·Vy, -e·Vz)` — which this
    /// function computes directly rather than through two identity matrices.
    /// - Trigger: debris bouncing on a sloped cell.
    /// - Player effect: it would rebound along the slope's normal in retail and
    ///   rebounds vertically here, so a chunk landing on a hillside runs
    ///   downhill in retail and hops in place here.
    /// - Frequency: `[TIRE]` is the only stock `[VoxelAnims]` entry with a
    ///   non-zero `Elasticity` (0.8), and `Elasticity = 0` zeroes the velocity
    ///   whatever the matrix — but all 36 stock `DebrisTypes=` lines name
    ///   `TIRE`, so every stock section that throws VOXEL debris throws exactly
    ///   the bouncing type. (Most stock vehicles throw SHP debris instead and
    ///   never reach this code.) The remaining bound is terrain alone: debris
    ///   lands on flat ground far more often than on a ramp.
    /// - Downstream risk: closing it needs the runtime contents of
    ///   `MATRIX_TABLE_0x00B45188`, which `VXL_MasterLighting_Init` builds from
    ///   `Matrix3x4_BuildFromRotateXAndFacing` — the eight facings and two
    ///   slope constants are known, the matrices themselves are UNCHECKED. The
    ///   `FUN_005AF4D0` row associations differ per row (row 0 pairs the y/z
    ///   terms first, rows 1 and 2 pair y/x first), so they must be read from
    ///   disassembly rather than written naturally.
    ///
    /// Returns the reflected velocity, or `None` for a ramp this does not
    /// model.
    pub fn reflect_off_ground(
        velocity: [NativeF32Bits; 3],
        elasticity: NativeF64Bits,
        ramp: u8,
    ) -> Result<Option<[NativeF32Bits; 3]>, NativeX87Error> {
        if ramp != FLAT_RAMP {
            return Ok(None);
        }
        // The elasticity multiplier is the f32 narrowing of the stored double,
        // not the double itself — `FLD double [ESP+0x64]` / `FSTP float [ESP]`.
        let scale = X87Chop53::load_f32(X87Chop53::store_f32(X87Chop53::load_f64(elasticity)?)?)?;

        let mut out = [NativeF32Bits::POSITIVE_ZERO; 3];
        for (axis, component) in velocity.iter().enumerate() {
            let scaled = X87Chop53::mul(scale, X87Chop53::load_f32(*component)?);
            // Y is negated going in and coming out, so it survives unflipped;
            // Z is the reflected axis and keeps a single negation.
            out[axis] = X87Chop53::store_f32(if axis == 2 {
                X87Chop53::neg(scaled)
            } else {
                scaled
            })?;
        }
        Ok(Some(out))
    }

    /// Pre-existing GSI-05.14 slope response remains a flat approximation.
    /// Ordinary slopes were already selected before the Phase 3 query repair;
    /// their reflection is separate physics work, not completed by this slice.
    fn reflect_off_ground_or_flat(
        velocity: [NativeF32Bits; 3],
        elasticity: NativeF64Bits,
        ramp: u8,
    ) -> Result<[NativeF32Bits; 3], NativeX87Error> {
        if let Some(reflected) = Self::reflect_off_ground(velocity, elasticity, ramp)? {
            return Ok(reflected);
        }
        Ok(Self::reflect_off_ground(velocity, elasticity, FLAT_RAMP)?
            .expect("the flat ramp is always modelled"))
    }

    /// `FUN_00439A10 @ 0x00439A10` — the magnitude the stop test compares
    /// against `2.5`.
    ///
    /// Every term is `ftol`-truncated to an integer before the sum, so the
    /// result is an integer and the threshold is effectively `>= 3`. The Z term
    /// is PREDICTIVE: `heightAboveGround * Gravity + Velocity.Z`
    /// (`0x00439A9D`..`0x00439AAB`), not the stored Z velocity, so a body still
    /// high above the ground reads far from rest whatever it is doing. The sum
    /// associates `(vz*vz + vx*vx) + vy*vy` — read from the `FLD ST(n)/FMUL
    /// ST(n)/FADDP` ladder at `0x00439AC3`..`0x00439AD1`, because the
    /// decompiler canonicalises the commutative sum.
    fn stop_magnitude(&self, terrain: &impl BounceTerrain) -> Result<i32, NativeX87Error> {
        let coord = ftol_coord(self.position)?;
        let ground = terrain.ground_height_leptons(coord);
        // `FILD ground` then `FSUBR float [pos.Z]` — the reverse subtract makes
        // this `pos.Z - ground`, not `ground - pos.Z`.
        let mut height_above = X87Chop53::ftol_i64(X87Chop53::sub(
            X87Chop53::load_f32(self.position[2])?,
            X87Chop53::load_i32(ground),
        ))? as i32;
        if terrain.is_bridge_cell(coord)
            && height_above >= crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
        {
            height_above -= crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
        }
        let vx = X87Chop53::ftol_i64(X87Chop53::load_f32(self.velocity[0])?)? as i32;
        let vy = X87Chop53::ftol_i64(X87Chop53::load_f32(self.velocity[1])?)? as i32;
        let predictive_z = X87Chop53::ftol_i64(X87Chop53::add(
            X87Chop53::mul(
                X87Chop53::load_i32(height_above),
                X87Chop53::load_f64(self.gravity)?,
            ),
            X87Chop53::load_f32(self.velocity[2])?,
        ))? as i32;

        let pz = X87Chop53::load_i32(predictive_z);
        let fx = X87Chop53::load_i32(vx);
        let fy = X87Chop53::load_i32(vy);
        let root = sqrt_approx_f32(X87Chop53::add(
            X87Chop53::add(X87Chop53::mul(pz, pz), X87Chop53::mul(fx, fx)),
            X87Chop53::mul(fy, fy),
        ))?;
        Ok(X87Chop53::ftol_i64(X87Chop53::load_f32(root)?)? as i32)
    }

    /// `BounceClass::Update @ 0x00439B00` — one tick of the physics body.
    ///
    /// The verified order, from the body and the disassembly cited per step:
    /// 1. `Velocity.Z -= Gravity`, stored back by `FSTP float [EBX+0x8]` at
    ///    `0x00439B5F` — `EBX` is the velocity base `+0x24`, so `+0x8` is Z.
    /// 2. The angular clamp at `0x00439B7A` is DEAD and is deliberately not
    ///    ported: the raw bytes show `FLD ST(0)` / `FDIVRP` on an empty-above
    ///    stack, so the scale is `|V| / |V|` = 1.0, and every `Init` caller
    ///    passes `angVelMagnitude` as two literal zeros anyway, which makes the
    ///    `0.0 < AVM` gate false everywhere in the image.
    /// 3. `Position += Velocity` (`FUN_0043A100`), between the pre- and
    ///    post-move `ftol` coordinate captures.
    /// 4. Ground height at the POST-move coordinate, then the deck plane
    ///    `ground + DAT_0089C76C`.
    /// 5. Deck crossing, gated on either cell carrying the bridge flag: `fell`
    ///    = new below the deck and old at or above it; `rose` = the reverse.
    /// 6. Building/wall lookup, gated by the `150.0` proximity window ALONE —
    ///    that gate does not apply to ordinary ground contact.
    /// 7. Snap ladder. Below the ground surface the body always reflects; at or
    ///    above it, only a deck crossing or a building/wall surface does, and
    ///    otherwise the tick returns `Falling` with no reflection at all.
    /// 8. The reflection round trip, then the stop test.
    ///
    /// 9. The cliff face: a step of two levels or more restores the pre-tick
    ///    body (native execution: `tools/spatial_oracle/anim_bouncer_flight.py`).
    ///
    /// 10. The tumble: a reflection negates the rotation's vector part, and
    ///     every tick integrates the orientation ([`Self::finish_tick`]).
    ///
    /// One arm is NOT modelled, recorded where it is skipped: the cliff-face
    /// mirror for a bouncy body.
    pub fn update(
        &mut self,
        terrain: &impl BounceTerrain,
    ) -> Result<BounceOutcome, NativeX87Error> {
        // The pre-tick body, velocity before gravity (`0x00439B26..0x00439B62`).
        let snapshot = (self.position, self.velocity);
        self.velocity[2] = X87Chop53::store_f32(X87Chop53::sub(
            X87Chop53::load_f32(self.velocity[2])?,
            X87Chop53::load_f64(self.gravity)?,
        ))?;

        let old_coord = ftol_coord(self.position)?;
        for axis in 0..3 {
            self.position[axis] = X87Chop53::store_f32(X87Chop53::add(
                X87Chop53::load_f32(self.position[axis])?,
                X87Chop53::load_f32(self.velocity[axis])?,
            ))?;
        }
        let new_coord = ftol_coord(self.position)?;

        let ground = terrain.ground_height_leptons(new_coord);
        let deck = ground.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        let new_cell = terrain.select_cell(new_coord);

        let mut fell_through_deck = false;
        let mut rose_through_deck = false;
        if terrain.selected_is_bridge(&new_cell) || terrain.is_bridge_cell(old_coord) {
            if new_coord.z < deck {
                fell_through_deck = deck <= old_coord.z;
            } else {
                rose_through_deck = old_coord.z < deck;
            }
        }

        let position_z = f32_of(self.position[2]);
        let ground_f = ground as f32;
        // The `150.0` window gates the building/wall LOOKUP only.
        let building_surface = !fell_through_deck
            && !rose_through_deck
            && ground_f <= position_z
            && position_z - BUILDING_LOOKUP_PROXIMITY_LEPTONS < ground_f
            && terrain.has_bounce_surface(&new_cell);

        // The LaserFence-dependent rejection (type+0x16BF and building+0x618
        // >=8) remains outside this comparison. Its fixtures have that flag
        // clear; this does not establish universal dormancy.
        let clamp_to_surface = |state: &mut Self| {
            let clamp_floor = (ground - GROUND_CLAMP_WINDOW_LEPTONS) as f32;
            if clamp_floor < f32_of(state.position[2]) {
                state.position[2] = NativeF32Bits::from_bits(ground_f.to_bits());
            }
        };
        if position_z < ground_f {
            if fell_through_deck {
                self.position[2] = NativeF32Bits::from_bits((deck as f32).to_bits());
            } else if rose_through_deck {
                self.position[2] =
                    NativeF32Bits::from_bits(((deck - DECK_RISE_DROP_LEPTONS) as f32).to_bits());
            } else {
                clamp_to_surface(self);
            }
        } else if fell_through_deck {
            self.position[2] = NativeF32Bits::from_bits((deck as f32).to_bits());
        } else if rose_through_deck {
            self.position[2] =
                NativeF32Bits::from_bits(((deck - DECK_RISE_DROP_LEPTONS) as f32).to_bits());
        } else if !building_surface {
            // `uVar8 = 0; goto LAB_0043A066` — no reflection this tick, but the
            // stop test still runs.
            return self.finish_tick(terrain, BounceOutcome::Falling);
        } else {
            clamp_to_surface(self);
        }

        self.velocity = Self::reflect_off_ground_or_flat(
            self.velocity,
            self.elasticity,
            terrain.ramp(new_coord),
        )?;
        // The spin reverses (`0x00439E89..0x00439EE3`: each vector component
        // through `FLD`/`FCHS`/`FSTP float`, an exact sign flip). The cliff
        // restore below puts back position and velocity only.
        for component in &mut self.rotation[..3] {
            *component = NativeF32Bits::from_bits(component.bits() ^ 0x8000_0000);
        }

        // The cliff face (`0x00439F3A..0x0043A05A`): the OLD then the post-snap
        // coordinate's cell height levels. A step of two levels or more whose
        // entry test holds restores the pre-tick body, then replaces the
        // velocity with a planar mirror of the restored one, scaled by the
        // double `Elasticity`. The entry test: the pre-tick Z velocity below
        // -0.0002 with the body now above where it was, or else not below
        // -0.0003 with the body now more than `vz + 1` above it.
        let old_level = terrain.cell_height_level(old_coord);
        let new_level = terrain.cell_height_level(ftol_coord(self.position)?);
        if new_level.wrapping_sub(old_level) >= CLIFF_FACE_LEVELS
            && Self::enters_cliff_face(snapshot, self.position[2])?
        {
            (self.position, self.velocity) = snapshot;
            // RESIDUAL (GSI-05.14): the mirror is one of the planar matrices
            // `FUN_00755C60` builds for the face's direction (`0x004848B0`),
            // unreadable through this instrument. `Elasticity=0` — every
            // stock SHP debris chunk — zeroes it whatever the matrix (the
            // sign of each zero is not modelled); any other elasticity keeps
            // the flat reflection of the restored velocity.
            // - Trigger: a bouncing body entering a cell two or more levels up.
            // - Player effect: a bouncy piece rebounds off the face vertically
            //   instead of away from it.
            // - Frequency: rare; only `[TIRE]` (0.8) bounces among stock types.
            // - Downstream risk: none to the stream; no draws are involved.
            self.velocity = if self.elasticity.bits() & !(1 << 63) == 0 {
                [NativeF32Bits::POSITIVE_ZERO; 3]
            } else {
                Self::reflect_off_ground_or_flat(
                    self.velocity,
                    self.elasticity,
                    terrain.ramp(new_coord),
                )?
            };
        }

        self.finish_tick(terrain, BounceOutcome::Bounced)
    }

    /// The cliff-face entry test (`0x00439F67..0x00439FB1`) on the pre-tick
    /// body and the post-snap Z.
    fn enters_cliff_face(
        (position, velocity): ([NativeF32Bits; 3], [NativeF32Bits; 3]),
        now_z: NativeF32Bits,
    ) -> Result<bool, NativeX87Error> {
        use crate::util::native_x87::X87Ordering::{Greater, Less};
        let vz = X87Chop53::load_f32(velocity[2])?;
        let was_z = X87Chop53::load_f32(position[2])?;
        let now_z = X87Chop53::load_f32(now_z)?;
        if X87Chop53::compare(vz, X87Chop53::load_f64(CLIFF_FALLING_VZ)?) == Less
            && X87Chop53::compare(now_z, was_z) == Greater
        {
            return Ok(true);
        }
        if X87Chop53::compare(vz, X87Chop53::load_f64(CLIFF_RISING_VZ)?) == Less {
            return Ok(false);
        }
        let risen = X87Chop53::add(
            X87Chop53::add(vz, was_z),
            X87Chop53::load_f32(CLIFF_RISE_MARGIN)?,
        );
        Ok(X87Chop53::compare(risen, now_z) == Less)
    }

    /// `LAB_0043A066` — the tail both the contact and the no-contact arms
    /// reach: `orientation = product(orientation, rotation)` (`0x00645ED0`,
    /// copied into `+0x30` by `0x00645D20`), then the stop test.
    fn finish_tick(
        &mut self,
        terrain: &impl BounceTerrain,
        contact: BounceOutcome,
    ) -> Result<BounceOutcome, NativeX87Error> {
        self.orientation = quaternion_multiply(self.orientation, self.rotation)?;
        let magnitude = self.stop_magnitude(terrain)?;
        if (magnitude as f32) < STOP_MAGNITUDE_THRESHOLD {
            return Ok(BounceOutcome::Stopped);
        }
        Ok(contact)
    }
}

/// `Random__RandomRanged(low, high)` over a signed span.
///
/// `SimRng::next_range_u32_inclusive` models the native helper directly; the
/// shift here only moves the span onto the unsigned domain and back.
fn ranged_i32(rng: &mut SimRng, low: i32, high: i32) -> i32 {
    let span = (high as i64 - low as i64) as u32;
    let drawn = rng.next_range_u32_inclusive(0, span);
    (low as i64 + drawn as i64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f64bits(value: f64) -> NativeF64Bits {
        NativeF64Bits::from_bits(value.to_bits())
    }

    fn f32bits(value: f32) -> NativeF32Bits {
        NativeF32Bits::from_bits(value.to_bits())
    }

    /// `VoxelAnimClass::Constructor`'s shared arguments: elasticity from the
    /// type, gravity 1.4 hardcoded, angular-clamp magnitude two literal zeros.
    fn init_at(coord: IVec3, elasticity: f64, rng: &mut SimRng) -> BounceState {
        BounceState::init(
            coord,
            f64bits(elasticity),
            f64bits(1.4),
            NativeF64Bits::POSITIVE_ZERO,
            [f32bits(3.0), f32bits(-4.0), f32bits(12.0)],
            f64bits(0.2),
            rng,
        )
        .expect("init stays inside the verified x87 domain")
    }

    #[test]
    fn gsi_05_14_flat_reflection_flips_z_and_damps_all_three() {
        // Y is negated on the way in and on the way out, so it survives
        // UNFLIPPED; Z is the axis actually reflected. Getting that backwards
        // is the natural mistake — the decompile shows two Y negations and one
        // Z negation, and a reading that keeps only the outer pair would flip
        // the wrong axis.
        let v = [f32bits(3.0), f32bits(-4.0), f32bits(12.0)];
        let out = BounceState::reflect_off_ground(v, f64bits(0.5), 0)
            .expect("flat reflection stays in the x87 domain")
            .expect("ramp 0 is modelled");
        let got = [
            f32::from_bits(out[0].bits()),
            f32::from_bits(out[1].bits()),
            f32::from_bits(out[2].bits()),
        ];
        assert_eq!(got, [1.5, -2.0, -6.0]);
    }

    #[test]
    fn gsi_05_14_zero_elasticity_kills_the_bounce() {
        // Nine of the ten stock `[VoxelAnims]` types are `Elasticity=0`, so
        // this is the path every one of them takes: the reflection runs in full
        // and produces no motion, which is why such a piece stops on first
        // contact and exits through the STOP test rather than the bounce one —
        // and so never plays its `BounceAnim`. The death-debris producer is not
        // one of their callers: all 36 stock `DebrisTypes=` lines name `TIRE`,
        // the one type with `Elasticity=0.8`.
        let v = [f32bits(3.0), f32bits(-4.0), f32bits(12.0)];
        let out = BounceState::reflect_off_ground(v, NativeF64Bits::POSITIVE_ZERO, 0)
            .expect("zero elasticity is in domain")
            .expect("ramp 0 is modelled");
        for component in out {
            assert_eq!(f32::from_bits(component.bits()).abs(), 0.0);
        }
    }

    #[test]
    fn gsi_05_14_sloped_ramps_are_refused_rather_than_approximated() {
        // The matrix table's runtime contents are UNCHECKED, so a sloped ramp
        // returns None instead of silently reusing the flat collapse. A caller
        // that ignored this would rebound vertically off a hillside.
        let v = [f32bits(3.0), f32bits(-4.0), f32bits(12.0)];
        for ramp in 1..=16u8 {
            assert!(
                BounceState::reflect_off_ground(v, f64bits(0.8), ramp)
                    .expect("in domain")
                    .is_none(),
                "ramp {ramp} must be refused, not approximated by the flat case"
            );
        }
    }

    #[test]
    fn gsi_05_14_elasticity_is_narrowed_to_f32_before_scaling() {
        // `FLD double [ESP+0x64]` / `FSTP float [ESP]` — the multiplier is the
        // f32 narrowing of the stored double, taken before the first transform
        // overwrites that slot.
        //
        // The narrowing CHOPS. This process runs x87 at 53-bit precision with
        // truncate-toward-zero rounding, so `FSTP float` drops the low mantissa
        // bits rather than rounding them. Rust's `as f32` rounds to nearest, so
        // it disagrees by one ULP on a value like 0.1 — and anyone "simplifying"
        // this to `f64::from_bits(elasticity.bits()) as f32` would introduce
        // that error on every single bounce. This test exists to fail if they
        // do.
        let v = [
            f32bits(1.0),
            NativeF32Bits::POSITIVE_ZERO,
            NativeF32Bits::POSITIVE_ZERO,
        ];
        let out = BounceState::reflect_off_ground(v, f64bits(0.1), 0)
            .expect("in domain")
            .expect("ramp 0 is modelled");
        assert_eq!(
            out[0].bits(),
            0x3DCC_CCCC,
            "the chopped narrowing of 0.1, not the rounded one"
        );
        assert_ne!(
            out[0].bits(),
            (0.1_f64 as f32).to_bits(),
            "Rust's `as f32` rounds to nearest and would give 0x3DCCCCCD"
        );
    }

    #[test]
    fn gsi_05_14_init_takes_exactly_three_axis_draws() {
        // `0x00439857`, `0x00439875`, `0x00439897` — three
        // `RandomRanged(-0xFFFF, 0xFFFF)` and nothing else. The count is the
        // lockstep contract: the constructor takes four `Next()` draws before
        // this, so a spawn costs seven in total and a miscount here desyncs
        // every later consumer that tick.
        let mut rng = SimRng::new(77);
        let _ = init_at(IVec3::new(1024, 2048, 96), 0.8, &mut rng);

        let mut expected = SimRng::new(77);
        for _ in 0..3 {
            expected.next_range_u32_inclusive(0, (0xFFFF - -0xFFFF) as u32);
        }
        assert_eq!(rng.logical_view(), expected.logical_view());
    }

    #[test]
    fn gsi_05_14_init_copies_position_and_velocity_verbatim() {
        // Position is `FILD`ed from the integer coordinate; velocity is copied
        // float for float from the constructor's vector. Neither is scaled.
        let mut rng = SimRng::new(3);
        let state = init_at(IVec3::new(1024, 2048, 96), 0.8, &mut rng);
        assert_eq!(state.position_leptons(), IVec3::new(1024, 2048, 96));
        assert_eq!(state.velocity_f32(), [3.0, -4.0, 12.0]);
        assert_eq!(f64::from_bits(state.gravity.bits()), 1.4);
        assert_eq!(f64::from_bits(state.elasticity.bits()), 0.8);
        assert_eq!(
            state.angular_velocity_magnitude,
            NativeF64Bits::POSITIVE_ZERO,
            "every caller passes two literal zeros, which is what makes the \
             Update clamp dead"
        );
    }

    fn golden() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bounce_tumble.json",
        ))
        .unwrap()
    }

    fn bits<const N: usize>(value: &serde_json::Value) -> [NativeF32Bits; N] {
        std::array::from_fn(|i| NativeF32Bits::from_bits(value[i].as_u64().unwrap() as u32))
    }

    fn raw<const N: usize>(values: [NativeF32Bits; N]) -> [u32; N] {
        values.map(NativeF32Bits::bits)
    }

    /// `Quaternion_FromAxisAngle @ 0x00646480`, the product `0x00645ED0` and
    /// `Quaternion_ToMatrix @ 0x00646980`, executed on the original over unit,
    /// unnormalised, short, zero and signed-zero axes, small to large and
    /// negative angles, and zero and non-unit quaternions.
    #[test]
    fn quaternion_kernels_match_original_execution() {
        let golden = golden();
        for row in golden["from_axis_angle"].as_array().unwrap() {
            let angle = NativeF32Bits::from_bits(row["angle_bits"].as_u64().unwrap() as u32);
            let actual = quaternion_from_axis_angle(bits(&row["axis_bits"]), angle).unwrap();
            assert_eq!(raw(actual), raw(bits::<4>(&row["out_bits"])), "{row}");
        }
        for row in golden["multiply"].as_array().unwrap() {
            let actual = quaternion_multiply(bits(&row["a_bits"]), bits(&row["b_bits"])).unwrap();
            assert_eq!(raw(actual), raw(bits::<4>(&row["out_bits"])), "{row}");
        }
        for row in golden["to_matrix"].as_array().unwrap() {
            let actual = quaternion_to_matrix(bits(&row["q_bits"])).unwrap();
            assert_eq!(raw(actual), raw(bits::<12>(&row["out_bits"])), "{row}");
        }
    }

    /// Init's body from the original (`bounce_tumble.py` `flight`): the axis
    /// takes the draws in reverse, the rotation is built from it and the
    /// narrowed angle, the orientation starts at `(+0, +0, +0, 1)`, and the
    /// three draws leave the generator where native leaves it.
    #[test]
    fn init_builds_the_original_tumble() {
        for row in golden()["flight"].as_array().unwrap() {
            let input = &row["input"];
            let coord = &input["coord"];
            let at = |i: usize| coord[i].as_i64().unwrap() as i32;
            let mut rng = SimRng::new(input["seed"].as_u64().unwrap());
            let state = BounceState::init(
                IVec3::new(at(0), at(1), at(2)),
                f64bits(input["elasticity"].as_f64().unwrap()),
                f64bits(1.4),
                NativeF64Bits::POSITIVE_ZERO,
                bits(&input["velocity_bits"]),
                NativeF64Bits::from_bits(input["angle_bits"].as_u64().unwrap()),
                &mut rng,
            )
            .unwrap();
            let body = hex_bytes(row["init_body_hex"].as_str().unwrap());
            let f32_at =
                |offset: usize| u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            let expected_orientation: [u32; 4] = std::array::from_fn(|i| f32_at(0x30 + i * 4));
            let expected_rotation: [u32; 4] = std::array::from_fn(|i| f32_at(0x40 + i * 4));
            assert_eq!(raw(state.orientation), expected_orientation, "{input}");
            assert_eq!(raw(state.rotation), expected_rotation, "{input}");
            for axis in 0..3 {
                assert_eq!(state.position[axis].bits(), f32_at(0x18 + axis * 4));
                assert_eq!(state.velocity[axis].bits(), f32_at(0x24 + axis * 4));
            }
            assert_eq!(rng.native_state_hex(), row["rng_after"].as_str().unwrap());
        }
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }
}
