//! Decidable events (RFC 0001 section 6f, open question 10): the numbers of
//! the events this engine asks modules to decide, the `DecideRecord`
//! `on_decide` reads and writes in place, and the decision budget.
//!
//! The decidable events, each at a clean commit point:
//! - `TakeDamage` (appendix 4.3): the final damage, just before Object's
//!   ReceiveDamage writes Strength;
//! - `ProductionAllowed` (4.5): 1 allow, 0 disallow, once per frame for each
//!   house and bound type; the engine's buildability answer reads it;
//! - `ProductionQueued` (4.5): 1 allow, 0 refuse, when a PRODUCE event or
//!   the AI's factory adds a type;
//! - `Defeat` (4.5): 1 defeat, 0 veto, when the house defeat gate is about
//!   to defeat a house;
//! - `CommandAllowed` (4.5): 1 allow, 0 refuse, when the simulation
//!   executes a player command (a module order included) on a bound object;
//! - `Fire` (4.3, preview): the shot plan, a count of [`ProjectileSpec`]s in
//!   the result area, just before FireAt launches its bullet;
//! - `Launch` (4.5, preview): a [`PathSpec`] in the result area, its byte
//!   count in `result`, when a bullet of a bound type is launched;
//! - `ProjectileFrame` (4.5, preview): a [`ProjectileStep`] in the result
//!   area, before a bound bullet's candidate position is computed.
//!
//! Provisional numbering (open question 5): new events continue from 1000
//! in appendix order, 4.3 (1000..1025), then 4.4 (1026..1030), then 4.5
//! (1031..1036).

use crate::triggers::{EVENT_FIRE, EVENT_TAKE_DAMAGE, EventRecord};

pub const EVENT_PRODUCTION_ALLOWED: i32 = 1031;
pub const EVENT_PRODUCTION_QUEUED: i32 = 1032;
pub const EVENT_DEFEAT: i32 = 1033;
pub const EVENT_COMMAND_ALLOWED: i32 = 1034;
pub const EVENT_LAUNCH: i32 = 1035;
pub const EVENT_PROJECTILE_FRAME: i32 = 1036;

/// Events with a decision form in this engine.
pub const DECIDABLE_EVENTS: [i32; 8] = [
    EVENT_TAKE_DAMAGE,
    EVENT_FIRE,
    EVENT_PRODUCTION_ALLOWED,
    EVENT_PRODUCTION_QUEUED,
    EVENT_DEFEAT,
    EVENT_COMMAND_ALLOWED,
    EVENT_LAUNCH,
    EVENT_PROJECTILE_FRAME,
];
/// Of those, the decision-only ones (appendix 4.5): they need a decision
/// handler and are never delivered.
pub const DECISION_ONLY_EVENTS: [i32; 6] = [
    EVENT_PRODUCTION_ALLOWED,
    EVENT_PRODUCTION_QUEUED,
    EVENT_DEFEAT,
    EVENT_COMMAND_ALLOWED,
    EVENT_LAUNCH,
    EVENT_PROJECTILE_FRAME,
];

/// Plan entries one `Fire` decision may hold (RFC 15.4: 768 bytes).
pub const MAX_FIRE_PLAN: usize = 16;
/// Points one `PathSpec` may hold (RFC 15.4: 416 bytes).
pub const MAX_PATH_POINTS: usize = 16;

/// The result area of event `kind` in bytes (RFC 15.4), 0 for none.
pub const fn result_area_bytes(kind: i32) -> usize {
    match kind {
        EVENT_FIRE => MAX_FIRE_PLAN * ProjectileSpec::SIZE,
        EVENT_LAUNCH => PathSpec::HEADER + MAX_PATH_POINTS * PathSpec::POINT,
        EVENT_PROJECTILE_FRAME => ProjectileStep::SIZE,
        _ => 0,
    }
}

/// The bytes of the result area a handled answer `result` of `kind` uses,
/// within `cap`: `Fire`'s plan entries, `Launch`'s `PathSpec`, the one
/// `ProjectileStep`, 0 for the kinds without an area. `None` rejects the
/// answer: the previous result stands.
pub fn result_len(kind: i32, result: i64, cap: usize) -> Option<usize> {
    let len = match kind {
        EVENT_FIRE => usize::try_from(result)
            .ok()
            .filter(|count| *count <= MAX_FIRE_PLAN)?
            .checked_mul(ProjectileSpec::SIZE)?,
        EVENT_LAUNCH => usize::try_from(result).ok().filter(|len| {
            *len >= PathSpec::HEADER + PathSpec::POINT
                && (*len - PathSpec::HEADER) % PathSpec::POINT == 0
        })?,
        EVENT_PROJECTILE_FRAME => ProjectileStep::SIZE,
        _ => 0,
    };
    (len <= cap).then_some(len)
}

/// A decision's result area (appendix "Fixed layouts", `DecideRecord.area`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultArea {
    /// The bytes the handler may write: the kind's size
    /// ([`result_area_bytes`]), lowered by [`crate::SimModule::decide`] to
    /// what the module's delivery area holds past the record; 0 for none.
    pub cap: usize,
    /// Going in, the previous link's result (`previous_len` bytes, at most
    /// `cap`). After a handled call, the whole area as the handler left it
    /// (`cap` bytes); unchanged otherwise.
    pub bytes: Vec<u8>,
}

impl ResultArea {
    /// No result area.
    pub const fn none() -> Self {
        Self {
            cap: 0,
            bytes: Vec::new(),
        }
    }

    /// Event `kind`'s area holding `previous`.
    pub fn of(kind: i32, previous: Vec<u8>) -> Self {
        Self {
            cap: result_area_bytes(kind),
            bytes: previous,
        }
    }
}

/// `ProjectileSpec` (appendix "Fixed layouts"), 48 bytes: one entry of a
/// `Fire` shot plan (and `CreateProjectile`'s data).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectileSpec {
    /// Projectile type ID (name kind `Projectile`).
    pub projectile_type: i32,
    /// Weapon ID giving damage and warhead; −1 for the shot's own weapon.
    pub weapon: i32,
    /// Object or cell reference; 0 for the shot's target.
    pub target: i64,
    /// Initial module data (`Data0` bytes 0..7, `Data1` 8..15).
    pub data: [u8; 16],
    /// Bit 0: `origin` is absolute; otherwise an offset from the native
    /// launch position.
    pub flags: i32,
    /// x, y, z in leptons.
    pub origin: [i32; 3],
}

impl ProjectileSpec {
    pub const SIZE: usize = 48;
    pub const ABSOLUTE_ORIGIN: i32 = 1;

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::SIZE] = bytes.get(..Self::SIZE)?.try_into().ok()?;
        let mut data = [0u8; 16];
        data.copy_from_slice(&bytes[16..32]);
        Some(Self {
            projectile_type: crate::le_i32(bytes, 0),
            weapon: crate::le_i32(bytes, 4),
            target: crate::le_i64(bytes, 8),
            data,
            flags: crate::le_i32(bytes, 32),
            origin: [36, 40, 44].map(|at| crate::le_i32(bytes, at)),
        })
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[0..4].copy_from_slice(&self.projectile_type.to_le_bytes());
        out[4..8].copy_from_slice(&self.weapon.to_le_bytes());
        out[8..16].copy_from_slice(&self.target.to_le_bytes());
        out[16..32].copy_from_slice(&self.data);
        out[32..36].copy_from_slice(&self.flags.to_le_bytes());
        for (i, value) in self.origin.iter().enumerate() {
            out[36 + 4 * i..40 + 4 * i].copy_from_slice(&value.to_le_bytes());
        }
        out
    }
}

/// `PathSpec` (appendix "Fixed layouts"): a 32-byte header and 24 bytes per
/// point, `Launch`'s result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathSpec {
    /// `PathKind` (appendix 8.3, enum 22).
    pub kind: i32,
    /// Leptons per frame along the path, 16 fraction bits; 0 or less for
    /// the projectile's own speed.
    pub speed: i64,
    pub param: [i64; 2],
    /// x, y, z in leptons.
    pub points: Vec<[i64; 3]>,
}

impl PathSpec {
    pub const HEADER: usize = 32;
    pub const POINT: usize = 24;

    /// The spec in the first `len` bytes of `bytes`; `None` when the
    /// header's point count disagrees with `len`.
    pub fn from_bytes(bytes: &[u8], len: usize) -> Option<Self> {
        let bytes = bytes.get(..len).filter(|_| len >= Self::HEADER)?;
        let count = usize::try_from(crate::le_i32(bytes, 4)).ok()?;
        if len != Self::HEADER + count * Self::POINT || count == 0 {
            return None;
        }
        let points = (0..count)
            .map(|i| {
                let at = Self::HEADER + i * Self::POINT;
                [at, at + 8, at + 16].map(|at| crate::le_i64(bytes, at))
            })
            .collect();
        Some(Self {
            kind: crate::le_i32(bytes, 0),
            speed: crate::le_i64(bytes, 8),
            param: [crate::le_i64(bytes, 16), crate::le_i64(bytes, 24)],
            points,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::HEADER + self.points.len() * Self::POINT);
        out.extend_from_slice(&self.kind.to_le_bytes());
        out.extend_from_slice(&(self.points.len() as i32).to_le_bytes());
        out.extend_from_slice(&self.speed.to_le_bytes());
        for value in self.param {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for point in &self.points {
            for value in point {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out
    }
}

/// `ProjectileStep` (appendix "Fixed layouts"), 56 bytes:
/// `ProjectileFrame`'s result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectileStep {
    /// Bit 0 detonate, 1 set velocity, 2 set position, 3 set module data.
    pub flags: i32,
    /// Leptons per frame, 16 fraction bits.
    pub velocity: [i32; 3],
    /// Leptons.
    pub position: [i32; 3],
    pub data: [u8; 16],
}

impl ProjectileStep {
    pub const SIZE: usize = 56;
    pub const DETONATE: i32 = 1;
    pub const SET_VELOCITY: i32 = 2;
    pub const SET_POSITION: i32 = 4;
    pub const SET_DATA: i32 = 8;

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::SIZE] = bytes.get(..Self::SIZE)?.try_into().ok()?;
        let mut data = [0u8; 16];
        data.copy_from_slice(&bytes[40..56]);
        Some(Self {
            flags: crate::le_i32(bytes, 0),
            velocity: [8, 12, 16].map(|at| crate::le_i32(bytes, at)),
            position: [24, 28, 32].map(|at| crate::le_i32(bytes, at)),
            data,
        })
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[0..4].copy_from_slice(&self.flags.to_le_bytes());
        for (i, value) in self.velocity.iter().enumerate() {
            out[8 + 4 * i..12 + 4 * i].copy_from_slice(&value.to_le_bytes());
        }
        for (i, value) in self.position.iter().enumerate() {
            out[24 + 4 * i..28 + 4 * i].copy_from_slice(&value.to_le_bytes());
        }
        out[40..56].copy_from_slice(&self.data);
        out
    }
}

/// Operations one `on_decide` call may use (open question 7): a tenth of
/// the default frame budget, since decisions run inside the engine's own
/// loops. A call that runs out stops the module.
pub const DECIDE_BUDGET: u64 = 20_000;

/// `DecideRecord` (appendix "Fixed layouts"), 160 bytes little endian
/// (RFC 0001 0.0.21 grew `EventRecord` to 120).
///
/// The result area (`area`, `area_cap`, `previous_len`) follows the record
/// in the module's delivery area, for the kinds that have one (`Fire`,
/// `Launch`, `ProjectileFrame`); [`crate::SimModule::decide`] places it and
/// fills those fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecideRecord {
    pub event: EventRecord,
    /// The engine's own result.
    pub native: i64,
    /// The previous link's result, `native` for the first link.
    pub previous: i64,
}

impl DecideRecord {
    pub const SIZE: usize = 160;

    /// The record as `on_decide` receives it: `result` starts as
    /// `previous` and `handled` as 0; `area`, `area_cap` and `previous_len`
    /// as given (all 0 without a result area).
    pub fn to_bytes(&self, area: i32, area_cap: i32, previous_len: i32) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[..EventRecord::SIZE].copy_from_slice(&self.event.to_bytes());
        out[120..128].copy_from_slice(&self.native.to_le_bytes());
        out[128..136].copy_from_slice(&self.previous.to_le_bytes());
        out[136..144].copy_from_slice(&self.previous.to_le_bytes());
        out[148..152].copy_from_slice(&area.to_le_bytes());
        out[152..156].copy_from_slice(&area_cap.to_le_bytes());
        out[156..160].copy_from_slice(&previous_len.to_le_bytes());
        out
    }

    /// The handler's answer in a record it returned: `result` when it set
    /// `handled`, else `None` (the previous result stands).
    pub fn answer(bytes: &[u8; Self::SIZE]) -> Option<i64> {
        let handled = crate::le_i32(bytes, 144);
        (handled != 0).then(|| crate::le_i64(bytes, 136))
    }
}
