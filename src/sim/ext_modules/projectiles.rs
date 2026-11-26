//! Module flight (RFC 0001 section 9, appendix 4.5 `Launch` and
//! `ProjectileFrame`, 7.4): what modules keep per projectile, the two
//! decision points that steer a bullet, and the projectile properties.
//!
//! [`ModuleProjectiles`] is the side table (RFC 15.1, item 9): per bullet
//! creation serial, the 16 bytes of module data (`Data0`, `Data1`), the frame
//! its flight count starts and a `Launch` path. Only bullets a module touches
//! have an entry; the entry goes with its bullet
//! (`Simulation::forget_ext_module_object`). It is saved with the modules and
//! folded into the state hash and the multiplayer checksum while it holds
//! anything, so stock play hashes as before.
//!
//! Decision points (each checks `TriggerRuntime::module_decides` first):
//! - `Launch` → a `PathSpec`, once per bullet `BulletClass::Fire` launches
//!   through the combat owner (`combat::projectile_delivery::admit_bullet`:
//!   FireAt and a `Fire` plan, shrapnel, airburst, NukeMaker, the silo and
//!   trigger strikes), after its Unlimbo, Ares' launch firestorm test and
//!   before an `Inviso=` bullet's placement, which is never asked (it does
//!   not fly). `subject` is the launcher (0 when gone or none), `other` the
//!   target object, `cell` the target coordinate's cell. Bindings: the
//!   launcher or its type, the weapon, warhead and projectile type.
//! - `ProjectileFrame` → a `ProjectileStep`, in the bullet's own Logic slot
//!   (`Simulation::visit_logic_bullet`, `BulletClass::AI @ 0x004666E0`), after
//!   its head (anim wait), Trailer, target resolution and frame fuse, before
//!   the candidate step, for a bullet of a bound projectile type that reaches
//!   that step ([`ProjectileStore::reaches_candidate_step`]). `subject` is the
//!   bullet, `value` the frames since its entry started.
//!
//! [`ProjectileStore::reaches_candidate_step`]: crate::sim::projectile::ProjectileStore::reaches_candidate_step
//!
//! A step or path replaces the candidate step of the flight arm, guided and
//! Vertical included, and with it the detonation tests inside those arms
//! (`crate::sim::projectile::ModuleMotion`); the ordinary tail and everything
//! after it still run. Answered `ProjectileFrame` steps win over a path for
//! that frame; the path's progress waits. Velocities are leptons per frame
//! with 16 fraction bits, exact in the bullet's binary64 velocity; a step
//! that sets none keeps its bits. Each axis of a velocity, a path's speed and
//! a frame's displacement is clamped to [`MAX_STEP`] leptons.
//!
//! Paths (`PathKind`): the engine advances `progress` by `speed` each frame
//! (16 fraction bits; a speed of 0 or less is the bullet's current speed) and
//! places the bullet at that fraction of the path's length `L`, in integers:
//! - `Bezier`: the curve with the launch position and the points as control
//!   points, at parameter `progress / L`, `L` the control polygon's length;
//! - `Parabola`: the line to the one point, raised by `4 h u (1 - u)`, `h`
//!   `param[0]`, `u = progress / L`, `L` the straight distance;
//! - `Spiral`: the line to the one point, offset across it by
//!   `4 r u (1 - u)` (`r` `param[0]`) at angle `2 pi frames / param[1]`,
//!   sideways and up;
//! - `Waypoints`: the polyline through the points at `progress` leptons.
//!
//! The path's end is an impact there. Each frame sets the velocity to the
//! step taken, so the bullet's facing follows the path.
//!
//! Residuals:
//! - `ProjectileFrame`'s count starts at launch for a bullet whose type a
//!   `ProjectileFrame` decider was bound to then, or that has a path or
//!   module data from launch; otherwise at its first request.
//! - Sines are Bhaskara's rational approximation (error under 0.2%).
//! - `Data0` and `Data1` are read and written through `get`/`set` and
//!   `ProjectileStep`, but no action creates a projectile yet
//!   (`CreateProjectile` stays `Unsupported`).

use std::collections::BTreeMap;

use vera20k_wasm_ext::decide::{EVENT_LAUNCH, EVENT_PROJECTILE_FRAME, PathSpec, ProjectileStep};
use vera20k_wasm_ext::{NAME_PROJECTILE, Status};

use super::{ExtModules, Host, ModuleEvent, Reader, Ref, cell_ref, decode_ref, type_ref};
use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::{
    ModuleMotion, Projectile, ProjectileCoord, ProjectileTarget, ProjectileVelocity,
};
use crate::sim::world::Simulation;
use crate::util::native_x87::NativeF64Bits;

/// The largest per-axis velocity, path speed and frame displacement, in
/// leptons per frame (RFC 15.4).
pub(crate) const MAX_STEP: i32 = 2048;
/// One in 16.16 fixed point.
const ONE: i64 = 1 << 16;

/// `PathKind` (appendix 8.3, enum 22).
const PATH_BEZIER: u8 = 0;
const PATH_PARABOLA: u8 = 1;
const PATH_SPIRAL: u8 = 2;
const PATH_WAYPOINTS: u8 = 3;

/// The side table (module docs), by bullet creation serial.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ModuleProjectiles {
    entries: BTreeMap<u64, ModuleProjectile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct ModuleProjectile {
    /// The frame `ProjectileFrame`'s count starts from.
    started: u32,
    /// `Data0` (bytes 0..7) and `Data1` (8..15), little endian.
    data: [u8; 16],
    path: Option<Path>,
}

/// A `Launch` path in flight.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct Path {
    kind: u8,
    /// Leptons per frame, 16 fraction bits.
    speed: i64,
    param: [i64; 2],
    /// The launch position, the first control point.
    start: [i32; 3],
    points: Vec<[i32; 3]>,
    /// Leptons, at least 1.
    length: i64,
    /// Leptons travelled, 16 fraction bits.
    progress: i64,
    /// Frames flown on the path (the spiral's angle).
    frames: u32,
}

impl ExtModules {
    pub(in crate::sim) fn valid_saved_projectile_paths(&self) -> bool {
        self.projectiles
            .entries
            .values()
            .filter_map(|entry| entry.path.as_ref())
            .all(|path| {
                Path::valid_shape(path.kind, path.points.len(), path.param[1])
                    && (1..=i64::MAX / ONE).contains(&path.length)
                    && (1..=i64::from(MAX_STEP) * ONE).contains(&path.speed)
                    && path.progress >= 0
            })
    }
}

impl ModuleProjectiles {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn forget(&mut self, id: u64) {
        if !self.entries.is_empty() {
            self.entries.remove(&id);
        }
    }

    /// `id`'s entry, started at `frame` when new.
    fn entry(&mut self, id: u64, frame: u32) -> &mut ModuleProjectile {
        self.entries.entry(id).or_insert(ModuleProjectile {
            started: frame,
            data: [0; 16],
            path: None,
        })
    }

    /// `Data0` (`half` 0) or `Data1` (1); 0 without an entry.
    pub(crate) fn data(&self, id: u64, half: usize) -> i64 {
        self.entries.get(&id).map_or(0, |entry| {
            i64::from_le_bytes(entry.data[half * 8..half * 8 + 8].try_into().unwrap())
        })
    }

    fn set_data(&mut self, id: u64, frame: u32, half: usize, value: i64) {
        self.entry(id, frame).data[half * 8..half * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }

    /// Starts `id`'s entry with `data` at `frame` (a `Fire` plan entry); an
    /// entry of all zero data needs none.
    pub(crate) fn launch_with_data(&mut self, id: u64, frame: u32, data: [u8; 16]) {
        if data != [0; 16] {
            self.entry(id, frame).data = data;
        }
    }

    /// The table as checksum bytes, in serial order.
    pub(crate) fn state_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.entries.iter().flat_map(|(id, entry)| {
            let path = entry.path.iter().flat_map(|path| {
                [path.kind]
                    .into_iter()
                    .chain(
                        [path.speed, path.param[0], path.param[1], path.length, path.progress]
                            .into_iter()
                            .flat_map(i64::to_le_bytes),
                    )
                    .chain(path.frames.to_le_bytes())
                    .chain(
                        std::iter::once(&path.start)
                            .chain(path.points.iter())
                            .flat_map(|point| point.iter().flat_map(|v| v.to_le_bytes())),
                    )
            });
            id.to_le_bytes()
                .into_iter()
                .chain(entry.started.to_le_bytes())
                .chain(entry.data)
                .chain(path)
        })
    }
}

impl Path {
    fn valid_shape(kind: u8, count: usize, period: i64) -> bool {
        match kind {
            PATH_BEZIER | PATH_WAYPOINTS => {
                (1..=vera20k_wasm_ext::decide::MAX_PATH_POINTS).contains(&count)
            }
            PATH_PARABOLA => count == 1,
            PATH_SPIRAL => count == 1 && period > 0,
            _ => false,
        }
    }

    /// The path `spec` describes from `start`; `None` for a kind, point
    /// count or parameter it does not admit. `native_speed` is the bullet's
    /// current speed in leptons per frame.
    fn new(spec: &PathSpec, start: ProjectileCoord, native_speed: i64) -> Option<Self> {
        let kind = u8::try_from(spec.kind).ok()?;
        if !Self::valid_shape(kind, spec.points.len(), spec.param[1]) {
            return None;
        }
        let points: Vec<[i32; 3]> = spec
            .points
            .iter()
            .map(|point| point.map(|v| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32))
            .collect();
        let limit = i64::from(MAX_STEP) * ONE;
        let speed = match spec.speed {
            speed if speed > 0 => speed.min(limit),
            _ => (native_speed.max(1) * ONE).min(limit),
        };
        let start = [start.x, start.y, start.z];
        let length = match kind {
            PATH_PARABOLA | PATH_SPIRAL => distance(start, points[0]),
            _ => std::iter::once(&start)
                .chain(points.iter())
                .zip(points.iter())
                .map(|(a, b)| distance(*a, *b))
                .sum::<i64>(),
        }
        .max(1);
        Some(Self {
            kind,
            speed,
            param: spec.param,
            start,
            points,
            length,
            progress: 0,
            frames: 0,
        })
    }

    /// One frame along the path: the new position and whether it is the end.
    fn advance(&mut self) -> ([i32; 3], bool) {
        self.frames = self.frames.wrapping_add(1);
        self.progress = self.progress.saturating_add(self.speed);
        let done = self.progress >= self.length.saturating_mul(ONE);
        let end = *self.points.last().expect("validated path has a point");
        if done {
            return (end, true);
        }
        let u = self.progress / self.length;
        let at = match self.kind {
            PATH_WAYPOINTS => self.along_polyline().map(clamp_i32),
            PATH_BEZIER => {
                let mut points = [[0; 3]; vera20k_wasm_ext::decide::MAX_PATH_POINTS + 1];
                points[0] = self.start.map(i64::from);
                for (slot, point) in points[1..].iter_mut().zip(&self.points) {
                    *slot = point.map(i64::from);
                }
                for width in (1..=self.points.len()).rev() {
                    for i in 0..width {
                        points[i] = lerp(points[i], points[i + 1], u);
                    }
                }
                points[0].map(clamp_i32)
            }
            _ => {
                let mut at = lerp(self.start.map(i64::from), end.map(i64::from), u).map(i128::from);
                // 4 u (1 - u), 16 fraction bits: 0 at both ends, 1 midway.
                let bulge = 4 * u * (ONE - u) / ONE;
                // Module radii span i64. Keep the projection and its addition to the
                // base coordinate wide until the final coordinate clamp.
                let amplitude = mul_q16(i128::from(self.param[0]), bulge);
                if self.kind == PATH_PARABOLA {
                    at[2] += amplitude;
                } else {
                    let turn = ((i64::from(self.frames) * ONE) / self.param[1]).rem_euclid(ONE);
                    let across = mul_q16(amplitude, cos_q16(turn));
                    let up = mul_q16(amplitude, sin_q16(turn));
                    let dx = i128::from(end[0]) - i128::from(self.start[0]);
                    let dy = i128::from(end[1]) - i128::from(self.start[1]);
                    let flat = i128::from(isqrt((dx * dx + dy * dy) as u128));
                    if flat == 0 {
                        at[0] += across;
                    } else {
                        at[0] += -dy * across / flat;
                        at[1] += dx * across / flat;
                    }
                    at[2] += up;
                }
                at.map(|v| v.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32)
            }
        };
        (at, false)
    }

    /// The polyline position `progress` leptons from the start.
    fn along_polyline(&self) -> [i64; 3] {
        let mut left = self.progress / ONE;
        let mut from = self.start;
        for &to in &self.points {
            let segment = distance(from, to);
            if left <= segment {
                return match segment {
                    0 => to.map(i64::from),
                    _ => lerp(from.map(i64::from), to.map(i64::from), left * ONE / segment),
                };
            }
            left -= segment;
            from = to;
        }
        from.map(i64::from)
    }
}

/// `a + (b - a) * u`, `u` with 16 fraction bits, rounded toward minus
/// infinity.
fn lerp(a: [i64; 3], b: [i64; 3], u: i64) -> [i64; 3] {
    [0, 1, 2].map(|i| a[i] + (((b[i] - a[i]) as i128 * u as i128) >> 16) as i64)
}

/// `a * b`, `b` with 16 fraction bits.
fn mul_q16(a: i128, b: i64) -> i128 {
    (a * i128::from(b)) >> 16
}

fn clamp_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Integer distance in leptons, rounded down.
fn distance(a: [i32; 3], b: [i32; 3]) -> i64 {
    let square: u128 = (0..3)
        .map(|i| {
            let d = (i64::from(b[i]) - i64::from(a[i])).unsigned_abs() as u128;
            d * d
        })
        .sum();
    isqrt(square) as i64
}

/// The integer square root, rounded down.
fn isqrt(n: u128) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = 1u128 << (128 - n.leading_zeros()).div_ceil(2);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x as u64;
        }
        x = y;
    }
}

/// The sine of `turn` (a whole turn is [`ONE`]), 16 fraction bits:
/// Bhaskara I's `16 h (1 - h) / (5 - 4 h (1 - h))` on each half turn.
fn sin_q16(turn: i64) -> i64 {
    let turn = turn.rem_euclid(ONE);
    let (half, sign) = if turn < ONE / 2 {
        (turn * 2, 1)
    } else {
        ((turn - ONE / 2) * 2, -1)
    };
    let p = half * (ONE - half) / ONE;
    sign * (16 * p * ONE) / (5 * ONE - 4 * p)
}

fn cos_q16(turn: i64) -> i64 {
    sin_q16(turn + ONE / 4)
}

/// A 16.16 velocity component as the bullet's binary64 (exact).
fn velocity_component(value: i64) -> NativeF64Bits {
    let limit = i64::from(MAX_STEP) * ONE;
    let value = value.clamp(-limit, limit) as f64 / ONE as f64;
    NativeF64Bits::from_bits(value.to_bits())
}

/// The bullet's binary64 velocity component in 16.16, toward zero.
fn fixed_component(value: NativeF64Bits) -> i64 {
    (f64::from_bits(value.bits()) * ONE as f64) as i64
}

/// `candidate` with each axis at most [`MAX_STEP`] from `from`.
fn clamp_step(from: ProjectileCoord, candidate: [i64; 3]) -> ProjectileCoord {
    let axis = |from: i32, to: i64| {
        let from = i64::from(from);
        clamp_i32(to.clamp(from - i64::from(MAX_STEP), from + i64::from(MAX_STEP)))
    };
    ProjectileCoord::new(
        axis(from.x, candidate[0]),
        axis(from.y, candidate[1]),
        axis(from.z, candidate[2]),
    )
}

/// What a `ProjectileSpec.target` names: an object or a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpecTarget {
    Object(u64),
    Cell(u16, u16),
}

/// `reference` as a `ProjectileSpec.target`; `None` for 0 or any other
/// reference.
pub(crate) fn spec_target(reference: i64) -> Option<SpecTarget> {
    match decode_ref(reference).ok()?? {
        Ref::Object(id) => Some(SpecTarget::Object(id)),
        Ref::Cell { x, y } => Some(SpecTarget::Cell(x, y)),
        _ => None,
    }
}

/// The rules index of `projectile`'s type, −1 without one.
fn projectile_type_index(sim: &Simulation, rules: &RuleSet, projectile: &Projectile) -> i32 {
    projectile
        .payload()
        .projectile_type()
        .and_then(|id| rules.projectile_index(sim.interner.resolve(id)))
        .map_or(-1, |index| index as i32)
}

impl Simulation {
    /// `Launch` for bullet `id`, just launched (module docs), and the start
    /// of its `ProjectileFrame` count when its type is bound to a decider.
    pub(crate) fn module_projectile_launched(&mut self, rules: &RuleSet, id: u64) {
        let launch = self.trigger_runtime.module_decides(EVENT_LAUNCH);
        let frame = self.trigger_runtime.module_decides(EVENT_PROJECTILE_FRAME);
        if !launch && !frame {
            return;
        }
        let Some(projectile) = self.projectiles.get(id) else {
            return;
        };
        let now = self.session.binary_frame();
        let projectile_type = projectile_type_index(self, rules, projectile);
        let (source, payload, at, target) = (
            projectile.source_id(),
            projectile.payload(),
            projectile.launch_target(),
            projectile.target(),
        );
        let (start, speed) = (
            projectile.position(),
            i64::from(projectile.speed_leptons_per_frame()),
        );
        if frame && self.binds_projectile_frame(projectile_type) {
            self.ext_modules.projectiles.entry(id, now);
        }
        if !launch {
            return;
        }
        let weapon = payload
            .installed_weapon()
            .and_then(|weapon| rules.weapon_index(self.interner.resolve(weapon)))
            .map_or(-1, |index| index as i32);
        let warhead = rules
            .warhead_index(self.interner.resolve(payload.warhead))
            .map_or(-1, |index| index as i32);
        let event = ModuleEvent {
            other: match target {
                ProjectileTarget::Entity(target) => target,
                _ => 0,
            },
            projectile: id,
            cell: Some(((at.x / 256) as u16, (at.y / 256) as u16)),
            weapon,
            warhead,
            projectile_type,
            ..ModuleEvent::new(
                EVENT_LAUNCH,
                if self.entities().get(source).is_some() {
                    source
                } else {
                    0
                },
            )
        };
        let mut area = Vec::new();
        let Some(len) = self.decide_module_area(rules, &event, 0, &mut area) else {
            return;
        };
        let path = usize::try_from(len)
            .ok()
            .and_then(|len| PathSpec::from_bytes(&area, len))
            .and_then(|spec| Path::new(&spec, start, speed));
        if let Some(path) = path {
            self.ext_modules.projectiles.entry(id, now).path = Some(path);
        }
    }

    /// A `Fire` plan entry's module data for bullet `id`, before its
    /// launch, so `Launch` reads it ([`ModuleProjectiles::launch_with_data`]).
    pub(crate) fn module_projectile_data(&mut self, id: u64, data: [u8; 16]) {
        let now = self.session.binary_frame();
        self.ext_modules
            .projectiles
            .launch_with_data(id, now, data);
    }

    /// Drops what modules keep for bullet `id`, which was never launched.
    pub(crate) fn forget_module_projectile(&mut self, id: u64) {
        self.ext_modules.projectiles.forget(id);
    }

    /// Whether a `ProjectileFrame` decider is bound to projectile type
    /// `projectile_type` (−1 for none).
    fn binds_projectile_frame(&self, projectile_type: i32) -> bool {
        u32::try_from(projectile_type).is_ok_and(|id| {
            !self
                .trigger_runtime
                .module_triggers()
                .deciders(EVENT_PROJECTILE_FRAME, 0, None, &[(NAME_PROJECTILE, id)])
                .is_empty()
        })
    }

    /// Whether a bullet's flight visit asks [`Self::module_projectile_motion`]:
    /// a `ProjectileFrame` decider exists or some bullet has a path.
    pub(crate) fn module_flight_active(&self) -> bool {
        !self.ext_modules.projectiles.is_empty()
            || self
                .trigger_runtime
                .module_decides(EVENT_PROJECTILE_FRAME)
    }

    /// The module step for bullet `id`'s flight visit (module docs), which
    /// reaches its candidate step: `ProjectileFrame`'s answer, else its
    /// path's next point, else `None` (native flight).
    pub(crate) fn module_projectile_motion(
        &mut self,
        rules: &RuleSet,
        id: u64,
    ) -> Option<ModuleMotion> {
        let now = self.session.binary_frame();
        let projectile = self.projectiles.get(id)?;
        let position = projectile.position();
        let projectile_type = projectile_type_index(self, rules, projectile);
        if self
            .trigger_runtime
            .module_decides(EVENT_PROJECTILE_FRAME)
            && self.binds_projectile_frame(projectile_type)
        {
            let started = self.ext_modules.projectiles.entry(id, now).started;
            let event = ModuleEvent {
                projectile: id,
                value: i64::from(now.wrapping_sub(started)),
                projectile_type,
                ..ModuleEvent::new(EVENT_PROJECTILE_FRAME, id)
            };
            let mut area = Vec::new();
            if self.decide_module_area(rules, &event, 0, &mut area).is_some()
                && let Some(step) = ProjectileStep::from_bytes(&area)
            {
                return Some(self.apply_projectile_step(id, now, position, &step));
            }
        }
        let path = self
            .ext_modules
            .projectiles
            .entries
            .get_mut(&id)?
            .path
            .as_mut()?;
        let (at, done) = path.advance();
        let candidate = clamp_step(position, at.map(i64::from));
        Some(ModuleMotion {
            velocity: Some(ProjectileVelocity::new(
                candidate.x.wrapping_sub(position.x),
                candidate.y.wrapping_sub(position.y),
                candidate.z.wrapping_sub(position.z),
            )),
            position: Some(candidate),
            detonate: done,
        })
    }

    /// A `ProjectileStep` as the flight's motion; its module data is kept.
    fn apply_projectile_step(
        &mut self,
        id: u64,
        now: u32,
        position: ProjectileCoord,
        step: &ProjectileStep,
    ) -> ModuleMotion {
        if step.flags & ProjectileStep::SET_DATA != 0 {
            self.ext_modules.projectiles.entry(id, now).data = step.data;
        }
        let velocity = (step.flags & ProjectileStep::SET_VELOCITY != 0).then(|| {
            let [x, y, z] = step.velocity.map(|v| velocity_component(i64::from(v)));
            ProjectileVelocity::from_native([x, y, z])
        });
        let position = (step.flags & ProjectileStep::SET_POSITION != 0)
            .then(|| clamp_step(position, step.position.map(i64::from)));
        ModuleMotion {
            velocity,
            position,
            detonate: step.flags & ProjectileStep::DETONATE != 0,
        }
    }
}

impl Reader<'_> {
    /// A projectile's property (appendix 7.4).
    pub(super) fn projectile_property(&self, id: u64, name: &str) -> (i64, Status) {
        let Some(projectile) = self.sim.projectiles.get(id) else {
            return (0, Status::Gone);
        };
        let position = projectile.position();
        let [vx, vy, vz] = projectile.velocity().native().map(fixed_component);
        let value = match name {
            "Type" => match projectile_type_index(self.sim, self.rules, projectile) {
                -1 => return (0, Status::NotFound),
                index => type_ref(NAME_PROJECTILE, index as u32),
            },
            // `Category` 4: Projectile (appendix 8.3).
            "Category" => 4,
            "X" => i64::from(position.x),
            "Y" => i64::from(position.y),
            "Z" => i64::from(position.z),
            "Cell" => match (u16::try_from(position.x / 256), u16::try_from(position.y / 256)) {
                (Ok(x), Ok(y)) => cell_ref(x, y),
                _ => return (0, Status::Gone),
            },
            "VX" => vx,
            "VY" => vy,
            "VZ" => vz,
            "Source" => match projectile.source_id() {
                source if self.sim.entities().get(source).is_some() => source as i64,
                _ => 0,
            },
            "Target" => match projectile.target() {
                ProjectileTarget::Entity(target) => target as i64,
                ProjectileTarget::Cell { rx, ry } => cell_ref(rx, ry),
                _ => 0,
            },
            "Data0" => self.sim.ext_modules.projectiles.data(id, 0),
            "Data1" => self.sim.ext_modules.projectiles.data(id, 1),
            _ => return (0, Status::Unsupported),
        };
        (value, Status::Ok)
    }
}

impl Host<'_, '_> {
    /// `set` of `Data0` or `Data1` on a projectile; the other projectile
    /// properties are read-only (appendix 7.4).
    pub(super) fn write_projectile_property(&mut self, id: u64, name: &str, value: i64) -> Status {
        if self.sim.projectiles.get(id).is_none() {
            return Status::Gone;
        }
        let half = match name {
            "Data0" => 0,
            "Data1" => 1,
            _ => return Status::Refused,
        };
        let now = self.sim.session.binary_frame();
        self.sim
            .ext_modules
            .projectiles
            .set_data(id, now, half, value);
        Status::Ok
    }
}

/// Path arithmetic the flight tests (`ext_modules_flight_tests`) check.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// The first point of a one-point `Waypoints` path from `start` to
    /// `end` at `speed` leptons a frame.
    pub(crate) fn waypoint_step(start: [i32; 3], end: [i32; 3], speed: i64) -> [i32; 3] {
        let spec = PathSpec {
            kind: i32::from(PATH_WAYPOINTS),
            speed: speed << 16,
            param: [0, 0],
            points: vec![end.map(i64::from)],
        };
        let start = ProjectileCoord::new(start[0], start[1], start[2]);
        Path::new(&spec, start, 0).expect("a path").advance().0
    }

    pub(crate) fn check_arithmetic() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(u128::from(u64::MAX) * u128::from(u64::MAX)), u64::MAX);
        assert_eq!(sin_q16(0), 0);
        assert_eq!(sin_q16(ONE / 4), ONE);
        assert_eq!(sin_q16(ONE / 2), 0);
        assert_eq!(sin_q16(3 * ONE / 4), -ONE);
        assert_eq!(cos_q16(0), ONE);
        let origin = ProjectileCoord::new(0, 0, 0);
        // A Bezier ends on its last control point.
        let bezier = PathSpec {
            kind: i32::from(PATH_BEZIER),
            speed: 16 << 16,
            param: [0, 0],
            points: vec![[512, 512, 0], [1024, 0, 0]],
        };
        let mut path = Path::new(&bezier, origin, 0).expect("a Bezier");
        let mut last = ([0; 3], false);
        for _ in 0..1000 {
            last = path.advance();
            if last.1 {
                break;
            }
        }
        assert_eq!(last, ([1024, 0, 0], true));
        // A parabola peaks midway, at its height.
        let parabola = PathSpec {
            kind: i32::from(PATH_PARABOLA),
            speed: 512 << 16,
            param: [300, 0],
            points: vec![[1024, 0, 0]],
        };
        let mut path = Path::new(&parabola, origin, 0).expect("a parabola");
        assert_eq!(path.advance(), ([512, 0, 300], false));
        assert_eq!(path.advance(), ([1024, 0, 0], true));
        // A spiral needs a period; a parabola exactly one point.
        let spiral = PathSpec {
            kind: i32::from(PATH_SPIRAL),
            speed: 0,
            param: [64, 0],
            points: vec![[1, 0, 0]],
        };
        assert!(Path::new(&spiral, origin, 0).is_none());
        // Full-width radii project onto an ordinary one-cell path.
        let wide = PathSpec {
            speed: 128 * ONE,
            param: [i64::MIN, 2],
            points: vec![[256, 0, 0]],
            ..spiral
        };
        let mut path = Path::new(&wide, origin, 0).expect("a spiral");
        assert_eq!(path.advance(), ([128, i32::MAX, 0], false));
        let two = PathSpec {
            points: vec![[1, 0, 0], [2, 0, 0]],
            ..parabola
        };
        assert!(Path::new(&two, origin, 0).is_none());
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::*;
    use crate::sim::snapshot::{GameSnapshot, SnapshotError};

    #[test]
    fn a_save_with_a_zero_length_module_path_is_refused_on_load() {
        crate::test_fixture::on_game_stack(|| {
            let mut sim = Simulation::new();
            let mut path = Path::new(
                &PathSpec {
                    kind: i32::from(PATH_WAYPOINTS),
                    speed: ONE,
                    param: [0; 2],
                    points: vec![[256, 0, 0]],
                },
                ProjectileCoord::new(0, 0, 0),
                0,
            )
            .unwrap();
            path.length = 0;
            sim.ext_modules.projectiles.entry(1, 0).path = Some(path);
            let bytes = GameSnapshot::save_validated(&sim, 0, 0, "damaged path", 0);
            assert!(matches!(
                GameSnapshot::load_with_resources(&bytes),
                Err(SnapshotError::InvalidModuleProjectilePath)
            ));
        });
    }
}
