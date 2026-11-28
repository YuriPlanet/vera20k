//! Rust SDK for VERA20K simulation modules (`rules.wasm`), RFC 0001 preview,
//! and client modules (`ui.wasm`, [`client`]).
//!
//! Each wrapper makes the host calls appendix section 3 lists, usually one.
//! A failed call returns `Err(Status)`; the host already wrote the empty
//! value. Names and numbers follow the preview API and may change before
//! version 1 (RFC "Process").
//!
//! ```ignore
//! use vera20k_sdk::prelude::*;
//!
//! fn tick() {
//!     let _ = Action::add_money(House::from_index(0), 100);
//! }
//! vera20k_sdk::on_frame!(every = 15, tick);
//! ```
#![no_std]

pub mod client;
pub mod decide;
pub mod ini;
pub mod notify;
mod props;
pub mod sys;
pub mod triggers;
pub mod vars;

pub use decide::{
    ApproachPointDecision, CommandAllowedDecision, CrushAllowedDecision, DamageIncomingDecision, DecideRecord,
    DefeatDecision, DeployAllowedDecision, DestinationSetDecision, FatalDamageDecision,
    FireAllowedDecision, FireDecision, HarvestCellDecision, IdleMissionDecision, LaunchDecision,
    MissionStepDecision, MoveFrameDecision, PathFoundDecision, PathKind,
    ProductionAllowedDecision, ProductionQueuedDecision, ProjectileFrameDecision,
    RetaliateDecision, TakeDamageDecision,
};
pub use ini::{
    EventScope, IniEntryRecord, IniKeyRecord, IniSection, ModuleEvent, ModuleEventRecord,
};
pub use notify::Notify;
pub use triggers::{
    AttackedByEvent, BirthEvent, CloakEvent, CrushedEvent, DeployedEvent, DetonatedEvent,
    ExperienceGainedEvent, FatalDamageEvent, HouseTransferredEvent, KilledByWarheadEvent,
    MissionChangedEvent, ModifierEvent, ModifierStacksEvent, PassengerAddedEvent,
    PassengerRemovedEvent, PollEvent, PromotedEvent, StockEvent, SuperWeaponLaunchedEvent,
    TargetChangedEvent, TransportEnteredEvent, TransportExitedEvent,
};
pub use triggers::{
    ActionDef, DELIVERY_SECTION, Event, EventRecord, FireEvent, KilledEvent, TakeDamageEvent,
    Target, TimerEvent, Trigger, TriggerRef, actions_section, actions_section_len,
};

pub mod prelude {
    pub use crate::vars::{GlobalVar, HouseVar, ObjectVar};
    pub use crate::{
        Action, ActionDef, ApproachPointDecision, Blocked, Category, Cell, Command,
        CommandAllowedDecision, DecideRecord,
        DefeatDecision, Event, EventRecord, EventScope, FIXED_ONE, FatalDamageDecision, Find,
        FireDecision, FireEvent,
        Global, House, LaunchDecision, MoveFrameDecision, PathFoundDecision, PathKind, Projectile,
        ProjectileFrameDecision,
        IniEntryRecord, IniKeyRecord, IniSection, KilledEvent, Layer, Mission, Modifier,
        ModifierOp, ModuleEvent, ModuleEventRecord, NameKind, Op, Order, Outcome,
        ProductionAllowedDecision, ProductionQueuedDecision, Prop, Ref, Relation, Rules, SpeedType,
        Status, TakeDamageDecision, TakeDamageEvent, Target, Techno, TimerEvent, Trigger,
        TriggerRef, TypeRef, Var, Waypoint, anim_id, fail, fixed, frame, ini, log, lookup, random,
    };
    pub use crate::{
        BirthEvent, PassengerAddedEvent, TransportEnteredEvent, TransportExitedEvent,
        PassengerRemovedEvent,
    };
    pub use crate::{NO_HANDLE, Notify};
}

/// A host call's failure (appendix "Status"). `Ok` is never an `Err`.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// The reference is 0 or the object no longer exists.
    Gone = 1,
    /// No matching name or ID, or a missing waypoint.
    NotFound = 2,
    /// The owner refused: occupied cell, unwritable now, or quota full.
    Refused = 3,
    /// Disabled in this match, an unknown entry, or unavailable here.
    Unsupported = 4,
}

pub type Result<T> = core::result::Result<T, Status>;

fn check(status: i32) -> Result<()> {
    match status {
        0 => Ok(()),
        1 => Err(Status::Gone),
        2 => Err(Status::NotFound),
        3 => Err(Status::Refused),
        _ => Err(Status::Unsupported),
    }
}

// ---------------------------------------------------------------- References

/// Bits 48..52 of a reference hold its kind (appendix "References").
const KIND_SHIFT: u32 = 48;
const KIND_TYPE: i64 = 1 << KIND_SHIFT;
const KIND_CELL: i64 = 2 << KIND_SHIFT;
const KIND_WAYPOINT: i64 = 3 << KIND_SHIFT;

/// No object (`NO_HANDLE`).
pub const NO_HANDLE: i64 = 0;

/// Any reference: what `get`, `set` and `find` take.
pub trait Ref: Copy {
    fn raw(self) -> i64;
}

macro_rules! reference {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
        pub struct $name(pub i64);

        impl Ref for $name {
            fn raw(self) -> i64 {
                self.0
            }
        }

        impl $name {
            /// Whether this is `NO_HANDLE`.
            pub const fn is_none(self) -> bool {
                self.0 == NO_HANDLE
            }
        }
    };
}

reference!(
    /// A unit or building: kind 0, its creation serial.
    Techno
);
reference!(
    /// A projectile (bullet): kind 0, its creation serial. Its properties
    /// are appendix 7.4's (`Type`, `X`..`Z`, `VX`..`VZ`, `Source`,
    /// `Target`, `Data0`, `Data1`).
    Projectile
);
reference!(
    /// A house.
    House
);
reference!(
    /// A cell: kind 2, `x << 16 | y`.
    Cell
);
reference!(
    /// A waypoint: kind 3, its number.
    Waypoint
);
reference!(
    /// A type: kind 1, `name kind << 32 | ID`.
    TypeRef
);

impl TypeRef {
    /// TechnoType `name`'s type reference; resolve once, at initialization.
    pub fn techno_type(name: &str) -> Result<Self> {
        lookup(NameKind::TechnoType, name).map(|id| Self::new(NameKind::TechnoType, id))
    }

    /// Weapon `name`'s type reference: a trigger bound to it raises `Fire`
    /// for every shooter using the weapon.
    pub fn weapon(name: &str) -> Result<Self> {
        lookup(NameKind::Weapon, name).map(|id| Self::new(NameKind::Weapon, id))
    }

    /// Warhead `name`'s type reference, bound as [`TypeRef::weapon`] is.
    pub fn warhead(name: &str) -> Result<Self> {
        lookup(NameKind::Warhead, name).map(|id| Self::new(NameKind::Warhead, id))
    }

    /// Projectile (BulletType) `name`'s type reference, bound as
    /// [`TypeRef::weapon`] is.
    pub fn projectile(name: &str) -> Result<Self> {
        lookup(NameKind::Projectile, name).map(|id| Self::new(NameKind::Projectile, id))
    }

    /// SuperWeaponType `name`'s type reference, for
    /// [`Order::launch_super_weapon`].
    pub fn super_weapon(name: &str) -> Result<Self> {
        lookup(NameKind::SuperWeapon, name).map(|id| Self::new(NameKind::SuperWeapon, id))
    }
}

/// The ID of animation `name`: the AnimType array position
/// [`Action::spawn_anim`] takes.
pub fn anim_id(name: &str) -> Result<i32> {
    lookup(NameKind::Anim, name)
}

impl House {
    /// The house at array index `index`. Provisional: the engine preview
    /// encodes houses as bit 47 over the index until houses share the
    /// creation counter.
    pub const fn from_index(index: u32) -> Self {
        Self(1 << 47 | index as i64)
    }
}

impl Cell {
    pub const fn at(x: u16, y: u16) -> Self {
        Self(KIND_CELL | (x as i64) << 16 | y as i64)
    }

    pub const fn x(self) -> u16 {
        (self.0 >> 16) as u16
    }

    pub const fn y(self) -> u16 {
        self.0 as u16
    }
}

impl Waypoint {
    pub const fn number(n: u32) -> Self {
        Self(KIND_WAYPOINT | n as i64)
    }
}

impl TypeRef {
    pub const fn new(kind: NameKind, id: i32) -> Self {
        Self(KIND_TYPE | (kind as i64) << 32 | id as u32 as i64)
    }

    /// The type ID within its name kind.
    pub const fn id(self) -> i32 {
        self.0 as i32
    }

    /// The wildcard of name kind `kind` (ID `0xFFFFFFFF`), for
    /// [`Find::types`] only.
    pub const fn any(kind: NameKind) -> Self {
        Self::new(kind, -1)
    }
}

// ------------------------------------------------------------------- Enums

/// Name kinds for `lookup` (appendix 8.1).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameKind {
    TechnoType = 0,
    Weapon = 1,
    Warhead = 2,
    Projectile = 3,
    Anim = 4,
    VoxelAnim = 5,
    ParticleSystem = 6,
    SuperWeapon = 7,
    Armor = 8,
    Overlay = 9,
    Sound = 10,
    Country = 11,
    TeamType = 12,
    ScriptType = 13,
    AiTrigger = 14,
    Variable = 15,
    Notify = 16,
    ModuleAction = 17,
    /// Engine-held text by string ID ([`text`]); `lookup` finds the ID of
    /// text it already holds.
    String = 18,
    Property = 19,
    EventKind = 20,
    ActionKind = 21,
    /// A hotkey this client module registered (`client::Local::register_hotkey`).
    Hotkey = 22,
    /// Module events any source registered ([`ini::event`]); the ID is the
    /// event ID.
    ModuleEvent = 24,
}

/// Filter clause comparison (appendix 8.3, enum 0).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Eq = 0,
    Ne = 1,
    Lt = 2,
    Le = 3,
    Gt = 4,
    Ge = 5,
}

/// `Category` property values (appendix 8.3, enum 10).
#[repr(i64)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Infantry = 0,
    Vehicle = 1,
    Aircraft = 2,
    Building = 3,
    Projectile = 4,
}

// ------------------------------------------------------------- Basic calls

/// The current frame.
pub fn frame() -> i32 {
    unsafe { sys::frame() }
}

/// A number in `lo..=hi` from this module's own random stream. `lo > hi`
/// disables the module.
pub fn random(lo: i32, hi: i32) -> i32 {
    let mut out = 0;
    unsafe { sys::random(lo, hi, &mut out) };
    out
}

/// One line in the in-game log.
pub fn log(line: &str) {
    unsafe { sys::log(line.as_ptr(), line.len() as i32) }
}

/// Logs `why` and stops the module for good: the engine disables it on
/// every machine and tells the players (RFC section 3). For a value the
/// module cannot use, such as a malformed registered INI key; name the
/// section and key in `why`.
pub fn fail(why: &str) -> ! {
    log(why);
    #[cfg(target_arch = "wasm32")]
    core::arch::wasm32::unreachable();
    #[cfg(not(target_arch = "wasm32"))]
    loop {}
}

/// The ID of `name` in name kind `kind`.
pub fn lookup(kind: NameKind, name: &str) -> Result<i32> {
    let mut out = -1;
    check(unsafe { sys::lookup(kind as i32, name.as_ptr(), name.len() as i32, &mut out) })?;
    Ok(out)
}

/// Writes the name of `id` into `buf` and returns it. `Err(Refused)` when
/// `buf` is too small.
pub fn name_of(kind: NameKind, id: i32, buf: &mut [u8]) -> Result<&str> {
    name_of_raw(kind as i32, id, buf)
}

/// The text of string ID `id` (`name_of(String, ..)`): a module action's
/// parameter text, an INI entry name or value. ID 0 is no text, `""`.
pub fn text(id: i32, buf: &mut [u8]) -> Result<&str> {
    if id == 0 {
        return Ok("");
    }
    name_of(NameKind::String, id, buf)
}

fn name_of_raw(kind: i32, id: i32, buf: &mut [u8]) -> Result<&str> {
    let mut len = 0;
    check(unsafe { sys::name_of(kind, id, buf.as_mut_ptr(), buf.len() as i32, &mut len) })?;
    let bytes = buf.get(..len as usize).ok_or(Status::Refused)?;
    core::str::from_utf8(bytes).map_err(|_| Status::Unsupported)
}

// -------------------------------------------------------------- Properties

/// A property ID (appendix section 7). Every property is a constant, as in
/// `Prop::FirepowerMultiplier`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Prop(pub i32);

impl Prop {
    /// The property named `name`, resolved by the engine. Resolve once, at
    /// initialization.
    pub fn named(name: &str) -> Result<Self> {
        lookup(NameKind::Property, name).map(Self)
    }
}

/// The effective value of `prop` on `reference`.
pub fn get(reference: impl Ref, prop: Prop) -> Result<i64> {
    let mut out = 0;
    check(unsafe { sys::get(reference.raw(), prop.0, &mut out) })?;
    Ok(out)
}

/// Writes `prop` through its owner.
pub fn set(reference: impl Ref, prop: Prop, value: i64) -> Result<()> {
    check(unsafe { sys::set(reference.raw(), prop.0, value) })
}

/// Reads `props` of every reference in `refs` into `out`, row-major
/// (`out[i * props.len() + j]`). Returns the first non-`Ok` status, the
/// empty values already written.
pub fn get_many(refs: &[i64], props: &[i32], out: &mut [i64]) -> Result<usize> {
    let needed = refs.len().checked_mul(props.len()).ok_or(Status::Refused)?;
    if out.len() < needed {
        return Err(Status::Refused);
    }
    let mut count = 0;
    let status = unsafe {
        sys::get_many(
            refs.as_ptr(),
            refs.len() as i32,
            props.as_ptr(),
            props.len() as i32,
            out.as_mut_ptr(),
            &mut count,
        )
    };
    check(status).map(|()| count as usize)
}

// --------------------------------------------------------------- Modifiers

/// `1.0` as a `fixed` value (raw `I16F16`, appendix "Values").
pub const FIXED_ONE: i64 = 65536;

/// `value` as a `fixed` multiplier, e.g. `fixed(0.5)`.
pub const fn fixed(value: f64) -> i64 {
    (value * 65536.0) as i64
}

/// How a modifier changes its property (appendix 8.3 `ModifierOp`).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModifierOp {
    Add = 0,
    Multiply = 1,
    Override = 2,
}

reference!(
    /// A modifier (appendix 7.2, 7.3): read it with `get` and the
    /// `Prop::Target`, `Prop::Remaining`, ... constants. It reads as gone
    /// once it expired or was removed.
    Modifier
);

impl Modifier {
    /// Adds a modifier to `prop` of `target` for `frames` frames (negative:
    /// until removed), with a source object (or 0) and an exclusion group
    /// (0: none; another modifier of this module in the same group on the
    /// same object is replaced). One host call.
    pub fn new(
        target: impl Ref,
        prop: Prop,
        op: ModifierOp,
        value: i64,
        frames: i32,
        source: i64,
        group: i32,
    ) -> Result<Self> {
        let mut out = 0;
        check(unsafe {
            sys::modify(
                target.raw(),
                prop.0,
                op as i32,
                value,
                frames,
                source,
                group,
                &mut out,
            )
        })?;
        Ok(Self(out))
    }

    /// Adds `value` to `prop`, e.g. `WeaponRangeExtra` in leptons.
    pub fn add(target: impl Ref, prop: Prop, value: i64, frames: i32) -> Result<Self> {
        Self::new(target, prop, ModifierOp::Add, value, frames, 0, 0)
    }

    /// Multiplies `prop` by the `fixed` `factor`, e.g.
    /// `Modifier::multiply(unit, Prop::SpeedMultiplier, fixed(0.5), 150)`.
    pub fn multiply(target: impl Ref, prop: Prop, factor: i64, frames: i32) -> Result<Self> {
        Self::new(target, prop, ModifierOp::Multiply, factor, frames, 0, 0)
    }

    /// Replaces `prop` with `value` while it lasts.
    pub fn override_(target: impl Ref, prop: Prop, value: i64, frames: i32) -> Result<Self> {
        Self::new(target, prop, ModifierOp::Override, value, frames, 0, 0)
    }

    /// Removes the modifier early; `Err(Gone)` once it expired.
    pub fn remove(self) -> Result<()> {
        check(unsafe { sys::unmodify(self.0) })
    }
}

// --------------------------------------------------------------- Variables

/// A Red Alert 2 variable (appendix "IDs"): locals 0..99, globals 100..149.
/// Variables a module declares use [`vars!`] instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Var(pub i32);

impl Var {
    /// Red Alert 2 local variable `n` (0..99).
    pub const fn local(n: i32) -> Self {
        Self(n)
    }

    /// Red Alert 2 global variable `n` (0..49).
    pub const fn global(n: i32) -> Self {
        Self(100 + n)
    }

    pub fn get(self) -> i64 {
        let mut out = 0;
        unsafe { sys::var_get(0, self.0, &mut out) };
        out
    }

    pub fn set(self, value: i64) -> Result<()> {
        check(unsafe { sys::var_set(0, self.0, value) })
    }
}

// ------------------------------------------------------------------ Search

/// Most clauses one [`Find`] holds.
pub const MAX_CLAUSES: usize = 8;

#[repr(C)]
struct QueryBuffer {
    query: sys::Query,
    clauses: [sys::Clause; MAX_CLAUSES],
}

/// A `find` query (appendix 8.2): one host call per [`Find::page`];
/// [`Find::first`] calls until it has a match or the last page.
pub struct Find(QueryBuffer);

impl Find {
    fn scope(scope: i32) -> Self {
        Self(QueryBuffer {
            query: sys::Query {
                scope,
                ..Default::default()
            },
            clauses: [sys::Clause::default(); MAX_CLAUSES],
        })
    }

    /// Units, buildings and projectiles, by creation serial.
    pub fn objects() -> Self {
        Self::scope(0)
    }

    /// Houses, by house index.
    pub fn houses() -> Self {
        Self::scope(1)
    }

    /// Only within `radius` leptons of `center`.
    pub fn near(mut self, center: Cell, radius: i32) -> Self {
        self.0.query.area = 1;
        self.0.query.cell = center.0;
        self.0.query.radius = radius;
        self
    }

    /// Only in the `width` by `height` cell rectangle from `top_left`.
    pub fn within(mut self, top_left: Cell, width: i32, height: i32) -> Self {
        self.0.query.area = 2;
        self.0.query.cell = top_left.0;
        self.0.query.radius = width;
        self.0.query.height = height;
        self
    }

    pub fn of_type(mut self, ty: TypeRef) -> Self {
        self.0.query.type_ = ty.0;
        self
    }

    pub fn owned_by(mut self, house: House) -> Self {
        self.0.query.owner = house.0;
        self
    }

    /// Keeps candidates whose `prop` compares to `value`. Clauses past
    /// [`MAX_CLAUSES`] are dropped.
    pub fn where_prop(mut self, prop: Prop, op: Op, value: i64) -> Self {
        let count = self.0.query.clause_count as usize;
        if let Some(clause) = self.0.clauses.get_mut(count) {
            *clause = sys::Clause {
                op: op as u8,
                prop: prop.0,
                value,
                ..Default::default()
            };
            self.0.query.clause_count += 1;
        }
        self
    }

    /// Fills `out` from position `after` (0 first), returning how many it
    /// wrote and where the next page starts (0 on the last page). One call
    /// examines at most 16,384 candidates, fewer where they would cost over
    /// half the callback's budget, so a page that is not full, even an empty
    /// one, is the last only when that position is 0.
    pub fn page(&mut self, out: &mut [i64], after: i64) -> Result<(usize, i64)> {
        let (query, len) = self.raw(after);
        let mut result = sys::FindResult::default();
        let status = unsafe {
            sys::find(
                query,
                len as i32,
                out.as_mut_ptr(),
                out.len() as i32,
                &mut result,
            )
        };
        check(status)?;
        Ok((result.count as usize, result.next))
    }

    /// The `Query` with its clauses, from position `after`, and its length:
    /// what `find` and `client_find` take.
    pub(crate) fn raw(&mut self, after: i64) -> (*const u8, usize) {
        self.0.query.after = after;
        let len =
            size_of::<sys::Query>() + size_of::<sys::Clause>() * self.0.query.clause_count as usize;
        ((&raw const self.0).cast(), len)
    }

    /// The first match, or `NO_HANDLE`. Follows pages until there is one,
    /// paying for every candidate examined on the way.
    pub fn first(mut self) -> i64 {
        let mut out = [NO_HANDLE];
        let mut after = 0;
        loop {
            match self.page(&mut out, after) {
                Ok((1, _)) => return out[0],
                Ok((_, next)) if next != 0 => after = next,
                _ => return NO_HANDLE,
            }
        }
    }
}

// ----------------------------------------------------------------- Actions

/// `EndFor` results (appendix 8.3, enum 17).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Win = 0,
    Lose = 1,
}

/// Missions (appendix 8.3, enum 8); the numbers are the map format's.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mission {
    Sleep = 0,
    Attack = 1,
    Move = 2,
    QMove = 3,
    Retreat = 4,
    Guard = 5,
    Sticky = 6,
    Enter = 7,
    Capture = 8,
    Eaten = 9,
    Harvest = 10,
    AreaGuard = 11,
    Return = 12,
    Stop = 13,
    Ambush = 14,
    Hunt = 15,
    Unload = 16,
    Sabotage = 17,
    Construction = 18,
    Selling = 19,
    Repair = 20,
    Rescue = 21,
    Missile = 22,
    Harmless = 23,
    Open = 24,
    Patrol = 25,
    ParadropApproach = 26,
    ParadropOverfly = 27,
    Deliberate = 28,
    AttackMove = 29,
    SpyplaneApproach = 30,
    SpyplaneOverfly = 31,
}

/// What `Teleport` does with a cell it cannot enter (appendix 8.3, enum 18).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blocked {
    /// Land on a nearby cell.
    Nearby = 0,
    /// Refuse the teleport.
    Fail = 1,
}

/// Property setters and getters of a unit or building (appendix 3): one
/// `get` or `set` each.
impl Techno {
    pub fn health(self) -> Result<i64> {
        get(self, Prop::Health)
    }

    /// Sets health without kill credit; 0 or less is `Err(Refused)`.
    pub fn set_health(self, health: i64) -> Result<()> {
        set(self, Prop::Health, health)
    }

    pub fn set_owner(self, house: House) -> Result<()> {
        set(self, Prop::Owner, house.0)
    }

    /// The current mission's number ([`Mission`]), 255 for none.
    pub fn mission(self) -> Result<i64> {
        get(self, Prop::Mission)
    }

    /// Queues `mission` untargeted, as a player's order does.
    pub fn set_mission(self, mission: Mission) -> Result<()> {
        set(self, Prop::Mission, mission as i64)
    }

    pub fn ammo(self) -> Result<i64> {
        get(self, Prop::Ammo)
    }

    /// Sets the ammo, clamped to the type's `Ammo=`.
    pub fn set_ammo(self, ammo: i64) -> Result<()> {
        set(self, Prop::Ammo, ammo)
    }

    /// Turns a building off or on, as stock actions 61 and 62 do.
    pub fn set_online(self, online: bool) -> Result<()> {
        set(self, Prop::Online, i64::from(online))
    }
}

/// One function per action kind the engine runs (appendix section 5).
/// Atomic actions are numbered from 1000 in table order, provisionally
/// (RFC open question 5); stock actions keep their stock numbers. A stock
/// action that addresses a waypoint takes a [`Waypoint`]; one with a house
/// takes a [`House`]. Type arguments are [`TypeRef`]s from [`lookup`]
/// (superweapons, animations and weapons use their rules array position as
/// the ID until those name kinds resolve names).
pub struct Action;

/// Parameters with the given fields set.
macro_rules! params {
    ($($field:ident: $value:expr),* $(,)?) => {
        sys::ActionParams { $($field: $value,)* ..Default::default() }
    };
}

/// Stock actions of one shape: `$name($args) -> kind, target, params`.
macro_rules! stock {
    ($($(#[$doc:meta])* $name:ident($($arg:ident: $ty:ty),*) = $kind:expr, $target:expr, $params:expr;)*) => {
        $(
            $(#[$doc])*
            pub fn $name($($arg: $ty),*) -> Result<()> {
                Self::run($kind, $target, &$params).map(drop)
            }
        )*
    };
}

impl Action {
    pub const FIRE_WEAPON: i32 = 1000;
    pub const LAUNCH_SUPER_WEAPON: i32 = 1001;
    pub const SPAWN_ANIM: i32 = 1002;
    pub const ATTACH_ANIM: i32 = 1003;
    pub const REMOVE: i32 = 1004;
    pub const KILL: i32 = 1005;
    pub const DAMAGE: i32 = 1006;
    pub const HEAL: i32 = 1007;
    pub const CREATE_OBJECT: i32 = 1008;
    pub const SET_MISSION: i32 = 1009;
    pub const TELEPORT: i32 = 1010;
    pub const CLEAR_TARGET: i32 = 1011;
    pub const CLEAR_DISGUISE: i32 = 1012;
    pub const CLEAR_MIND_CONTROL: i32 = 1013;
    pub const ADD_EXPERIENCE: i32 = 1014;
    pub const ADD_MONEY: i32 = 1015;
    pub const ALLOW_TYPE: i32 = 1016;
    pub const SET_OWNER: i32 = 1017;
    pub const ADD_AMMO: i32 = 1018;
    pub const ADD_PASSENGER: i32 = 1019;
    pub const SET_PROPERTY: i32 = 1021;
    pub const MODIFY: i32 = 1022;
    pub const REMOVE_MODIFIERS: i32 = 1023;
    pub const DETONATE_WEAPON: i32 = 1025;
    pub const DETONATE_WARHEAD: i32 = 1026;
    pub const END_FOR: i32 = 1030;
    pub const SET_SCORE: i32 = 1031;
    pub const ADD_SCORE: i32 = 1032;
    pub const LOG: i32 = 1033;
    pub const CONVERT: i32 = 1034;
    pub const PUSH: i32 = 1035;
    pub const RAISE_EVENT: i32 = 1042;
    pub const SET_FACING: i32 = 1045;
    pub const DETACH_ANIM: i32 = 1046;
    /// Trigger-only kinds ([`ActionDef::set_var`], [`ActionDef::module`]).
    pub const SET_VAR: i32 = 1024;
    pub const MODULE: i32 = 1036;

    /// Runs action `kind` on `target` (a reference, 0 for none); returns
    /// the created object or 0.
    pub fn run(kind: i32, target: i64, params: &sys::ActionParams) -> Result<i64> {
        let mut out = 0;
        check(unsafe {
            sys::action(
                kind,
                target,
                params,
                size_of::<sys::ActionParams>() as i32,
                &mut out,
            )
        })?;
        Ok(out)
    }

    /// Runs action `kind` with `text` as its string argument.
    pub fn run_text(kind: i32, target: i64, text: &str) -> Result<i64> {
        let params = params!(text: text.as_ptr() as i32, text_len: text.len() as i32);
        Self::run(kind, target, &params)
    }

    // Atomic actions (5.3).

    /// Launches `house`'s superweapon `kind` at `at` (a cell, waypoint or
    /// object).
    pub fn launch_super_weapon(house: House, kind: TypeRef, at: impl Ref) -> Result<()> {
        let params = params!(id: kind.id(), cell: at.raw());
        Self::run(Self::LAUNCH_SUPER_WEAPON, house.0, &params).map(drop)
    }

    /// Removes `target` without death, survivors or kill credit.
    pub fn remove(target: Techno) -> Result<()> {
        Self::run(Self::REMOVE, target.0, &params!()).map(drop)
    }

    /// Kills `target` through the damage path, crediting `source`
    /// (`Techno(NO_HANDLE)` for none).
    pub fn kill(target: Techno, source: Techno) -> Result<()> {
        Self::run(Self::KILL, target.0, &params!(other: source.0)).map(drop)
    }

    /// Deals `amount` damage of `warhead` to `target` under armor rules.
    pub fn damage(target: Techno, amount: i32, warhead: TypeRef) -> Result<()> {
        Self::damage_from(target, amount, warhead, Techno(NO_HANDLE))
    }

    /// As [`Action::damage`], crediting `source` with the kill and experience.
    pub fn damage_from(
        target: Techno,
        amount: i32,
        warhead: TypeRef,
        source: Techno,
    ) -> Result<()> {
        let params = params!(id: warhead.id(), value: i64::from(amount), other: source.0);
        Self::run(Self::DAMAGE, target.0, &params).map(drop)
    }

    /// Creates a `kind` owned by `house` at `at` (a cell or waypoint) and
    /// returns it. An occupied cell is `Err(Refused)`.
    pub fn create_object(at: impl Ref, kind: TypeRef, house: House) -> Result<Techno> {
        let params = params!(id: kind.id(), arg: 255, house: house.0);
        Self::run(Self::CREATE_OBJECT, at.raw(), &params).map(Techno)
    }

    /// Drops `target`'s current target.
    pub fn clear_target(target: Techno) -> Result<()> {
        Self::run(Self::CLEAR_TARGET, target.0, &params!()).map(drop)
    }

    /// Gives `house` `amount` credits (negative takes them).
    pub fn add_money(house: House, amount: i32) -> Result<()> {
        Self::run(Self::ADD_MONEY, house.0, &params!(value: i64::from(amount))).map(drop)
    }

    /// Gives `target` to `house`.
    pub fn set_owner(target: Techno, house: House) -> Result<()> {
        Self::run(Self::SET_OWNER, target.0, &params!(house: house.0)).map(drop)
    }

    /// Applies `warhead`'s area damage at `at`.
    pub fn detonate_warhead(at: impl Ref, warhead: TypeRef, damage: i32) -> Result<()> {
        let params = params!(id: warhead.id(), value: i64::from(damage));
        Self::run(Self::DETONATE_WARHEAD, at.raw(), &params).map(drop)
    }

    /// Makes `house` win or lose.
    pub fn end_for(house: House, outcome: Outcome) -> Result<()> {
        Self::run(Self::END_FOR, house.0, &params!(arg: outcome as i32)).map(drop)
    }

    /// Writes `line` to the in-game log, as an action.
    pub fn log(line: &str) -> Result<()> {
        Self::run_text(Self::LOG, 0, line).map(drop)
    }

    /// Plays animation `anim` (its rules array position, as stock 41 takes)
    /// at `at` (a cell, waypoint or object).
    pub fn spawn_anim(at: impl Ref, anim: i32) -> Result<()> {
        Self::run(Self::SPAWN_ANIM, at.raw(), &params!(id: anim)).map(drop)
    }

    /// Restores `amount` health to `target`, up to its Strength, without
    /// kill credit.
    pub fn heal(target: Techno, amount: i32) -> Result<()> {
        Self::run(Self::HEAL, target.0, &params!(value: i64::from(amount))).map(drop)
    }

    /// Queues `mission` on `target` as a player's order does, aimed at `at`
    /// (a cell or waypoint; `Cell(NO_HANDLE)` for none). `Move` with a cell
    /// moves there.
    pub fn set_mission(target: Techno, mission: Mission, at: impl Ref) -> Result<()> {
        let params = params!(arg: mission as i32, cell: at.raw());
        Self::run(Self::SET_MISSION, target.0, &params).map(drop)
    }

    /// As [`Action::set_mission`], aimed at the object `on`.
    pub fn set_mission_on(target: Techno, mission: Mission, on: Techno) -> Result<()> {
        let params = params!(arg: mission as i32, other: on.0);
        Self::run(Self::SET_MISSION, target.0, &params).map(drop)
    }

    /// Chronoshifts `target` (a vehicle or infantryman) to `to`.
    pub fn teleport(target: Techno, to: impl Ref, blocked: Blocked) -> Result<()> {
        let params = params!(arg: blocked as i32, cell: to.raw());
        Self::run(Self::TELEPORT, target.0, &params).map(drop)
    }

    /// Removes `target`'s disguise.
    pub fn clear_disguise(target: Techno) -> Result<()> {
        Self::run(Self::CLEAR_DISGUISE, target.0, &params!()).map(drop)
    }

    /// Releases `target` from its mind controller, back to its old owner.
    pub fn clear_mind_control(target: Techno) -> Result<()> {
        Self::run(Self::CLEAR_MIND_CONTROL, target.0, &params!()).map(drop)
    }

    /// Adds the experience a kill worth `points` credits gives `target`.
    pub fn add_experience(target: Techno, points: i32) -> Result<()> {
        let params = params!(value: i64::from(points));
        Self::run(Self::ADD_EXPERIENCE, target.0, &params).map(drop)
    }

    /// Adds `amount` ammo (negative takes it), within the type's `Ammo=`.
    pub fn add_ammo(target: Techno, amount: i32) -> Result<()> {
        let params = params!(value: i64::from(amount));
        Self::run(Self::ADD_AMMO, target.0, &params).map(drop)
    }

    /// Writes `prop`, as [`set`] does (the trigger form of a write).
    pub fn set_property(target: impl Ref, prop: Prop, value: i64) -> Result<()> {
        let params = params!(id: prop.0, value: value);
        Self::run(Self::SET_PROPERTY, target.raw(), &params).map(drop)
    }

    /// Adds a modifier, as [`Modifier::new`] does, and returns it.
    pub fn modify(
        target: impl Ref,
        prop: Prop,
        op: ModifierOp,
        value: i64,
        frames: i32,
        group: i32,
    ) -> Result<Modifier> {
        let params = params!(
            id: prop.0,
            arg: op as i32,
            arg2: group,
            value: value,
            value2: i64::from(frames),
        );
        Self::run(Self::MODIFY, target.raw(), &params).map(Modifier)
    }

    /// Removes this module's modifiers on `target` in `group`, only those
    /// of `prop` when given.
    pub fn remove_modifiers(target: impl Ref, group: i32, prop: Option<Prop>) -> Result<()> {
        let params = params!(id: prop.map_or(-1, |p| p.0), arg2: group);
        Self::run(Self::REMOVE_MODIFIERS, target.raw(), &params).map(drop)
    }

    /// Detonates `weapon` (a `Weapon` type reference) at `at` without a
    /// projectile, crediting `source` (`Techno(NO_HANDLE)` for none).
    pub fn detonate_weapon(at: impl Ref, weapon: TypeRef, source: Techno) -> Result<()> {
        let params = params!(id: weapon.id(), other: source.0);
        Self::run(Self::DETONATE_WEAPON, at.raw(), &params).map(drop)
    }

    /// Sets `house`'s score.
    pub fn set_score(house: House, score: i32) -> Result<()> {
        Self::run(Self::SET_SCORE, house.0, &params!(value: i64::from(score))).map(drop)
    }

    /// Adds to `house`'s score.
    pub fn add_score(house: House, score: i32) -> Result<()> {
        Self::run(Self::ADD_SCORE, house.0, &params!(value: i64::from(score))).map(drop)
    }

    /// Converts `target` to `kind` for good.
    pub fn convert(target: Techno, kind: TypeRef) -> Result<()> {
        Self::run(Self::CONVERT, target.0, &params!(id: kind.id())).map(drop)
    }

    /// Converts `target` to `kind` for `frames` frames, then back to the
    /// type it had before.
    pub fn convert_for(target: Techno, kind: TypeRef, frames: u32) -> Result<()> {
        let params = params!(id: kind.id(), value2: i64::from(frames));
        Self::run(Self::CONVERT, target.0, &params).map(drop)
    }

    /// Pushes `target` (a vehicle or infantryman on the ground) `distance`
    /// leptons toward the center of `toward` (negative: away from it), over
    /// `frames` frames (0: one per 64 leptons), on top of its own movement.
    /// 0 ends its push.
    pub fn push(target: Techno, toward: Cell, distance: i32, frames: u32) -> Result<()> {
        let params = params!(
            cell: toward.raw(),
            value: i64::from(distance),
            value2: i64::from(frames),
        );
        Self::run(Self::PUSH, target.0, &params).map(drop)
    }

    /// Turns `target`'s body toward `facing` (0..255) through its
    /// locomotor.
    pub fn set_facing(target: Techno, facing: u8) -> Result<()> {
        let params = params!(value: i64::from(facing));
        Self::run(Self::SET_FACING, target.0, &params).map(drop)
    }

    /// Fires `shooter`'s weapon `slot` (0 primary, 1 secondary) at its
    /// current target now; `Err(Refused)` when it cannot fire now (reloading,
    /// out of range, ...), `Err(Gone)` without a target.
    pub fn fire_weapon(shooter: Techno, slot: i32) -> Result<()> {
        let params = params!(id: -1, value: i64::from(slot));
        Self::run(Self::FIRE_WEAPON, shooter.0, &params).map(drop)
    }

    /// As [`Action::fire_weapon`], at the object `at`.
    pub fn fire_weapon_at(shooter: Techno, slot: i32, at: Techno) -> Result<()> {
        let params = params!(id: -1, value: i64::from(slot), other: at.0);
        Self::run(Self::FIRE_WEAPON, shooter.0, &params).map(drop)
    }

    /// As [`Action::fire_weapon`], at the cell `at` (a cell or waypoint).
    pub fn fire_weapon_at_cell(shooter: Techno, slot: i32, at: impl Ref) -> Result<()> {
        let params = params!(id: -1, value: i64::from(slot), cell: at.raw());
        Self::run(Self::FIRE_WEAPON, shooter.0, &params).map(drop)
    }

    /// Plays animation `anim` (its rules array position, as
    /// [`Action::spawn_anim`] takes) once, attached to `target`.
    pub fn attach_anim(target: Techno, anim: i32) -> Result<()> {
        Self::run(Self::ATTACH_ANIM, target.0, &params!(id: anim)).map(drop)
    }

    /// As [`Action::attach_anim`], but the animation keeps looping on
    /// `target` for `frames` frames, or until [`Action::detach_anim`] when
    /// `frames` is negative. It goes with the object.
    pub fn attach_anim_for(target: Techno, anim: i32, frames: i64) -> Result<()> {
        Self::run(Self::ATTACH_ANIM, target.0, &params!(id: anim, value2: frames)).map(drop)
    }

    /// Removes the animations [`Action::attach_anim_for`] of this module
    /// keeps on `target`: those of animation `anim`, or all with -1.
    pub fn detach_anim(target: Techno, anim: i32) -> Result<()> {
        Self::run(Self::DETACH_ANIM, target.0, &params!(id: anim)).map(drop)
    }

    /// Forbids (`false`) TechnoType `kind` to `house`, or lifts this
    /// module's forbid (`true`); prerequisites still apply.
    pub fn allow_type(house: House, kind: TypeRef, allow: bool) -> Result<()> {
        let params = params!(id: kind.id(), value: i64::from(allow));
        Self::run(Self::ALLOW_TYPE, house.0, &params).map(drop)
    }

    /// Boards `passenger` into `transport` (a vehicle or aircraft) at once,
    /// if the transport would load it; `Err(Refused)` otherwise.
    pub fn add_passenger(transport: Techno, passenger: Techno) -> Result<()> {
        let params = params!(other: passenger.0);
        Self::run(Self::ADD_PASSENGER, transport.0, &params).map(drop)
    }

    /// Raises module event `event` (RFC 0001 section 6h) on
    /// `target`: an object for an object event, anything (`Global`) for a
    /// global one. `other`, `value` and `value2` go into the record; its
    /// triggers' module actions are queued. Not in a decision handler.
    pub fn raise_event(
        event: ModuleEvent,
        target: impl Ref,
        other: Techno,
        value: i64,
        value2: i64,
    ) -> Result<()> {
        Self::raise_event_with_house(event, target, other, value, value2, House(NO_HANDLE))
    }

    /// As [`Action::raise_event`], with the record's `house`.
    pub fn raise_event_with_house(
        event: ModuleEvent,
        target: impl Ref,
        other: Techno,
        value: i64,
        value2: i64,
        house: House,
    ) -> Result<()> {
        let params = params!(
            id: event.0,
            other: other.0,
            value: value,
            value2: value2,
            house: house.0,
        );
        Self::run(Self::RAISE_EVENT, target.raw(), &params).map(drop)
    }

    // Stock actions (5.2) taking a string.

    /// Stock 1: the player wins if their country is `country`, else loses.
    pub fn declare_winner(country: &str) -> Result<()> {
        Self::run_text(1, 0, country).map(drop)
    }
    /// Stock 2: the player loses if their country is `country`, else wins.
    pub fn declare_loser(country: &str) -> Result<()> {
        Self::run_text(2, 0, country).map(drop)
    }
    /// Stock 11: shows a CSF entry.
    pub fn show_text(label: &str) -> Result<()> {
        Self::run_text(11, 0, label).map(drop)
    }
    /// Stock 12: removes the map trigger `id`.
    pub fn destroy_trigger(id: &str) -> Result<()> {
        Self::run_text(12, 0, id).map(drop)
    }
    /// Stock 20: plays a theme.
    pub fn play_theme(theme: &str) -> Result<()> {
        Self::run_text(20, 0, theme).map(drop)
    }
    /// Stock 21: plays an EVA line.
    pub fn play_speech(eva: &str) -> Result<()> {
        Self::run_text(21, 0, eva).map(drop)
    }
    /// Stock 22: fires the map trigger `id`.
    pub fn force_trigger(id: &str) -> Result<()> {
        Self::run_text(22, 0, id).map(drop)
    }
    /// Stock 53: enables the map trigger `id`.
    pub fn enable_trigger(id: &str) -> Result<()> {
        Self::run_text(53, 0, id).map(drop)
    }
    /// Stock 54: disables the map trigger `id`.
    pub fn disable_trigger(id: &str) -> Result<()> {
        Self::run_text(54, 0, id).map(drop)
    }
    /// Stock 70: destroys the map tag `id`.
    pub fn destroy_tag(id: &str) -> Result<()> {
        Self::run_text(70, 0, id).map(drop)
    }
    /// Stock 103: the mission timer's caption.
    pub fn timer_text(label: &str) -> Result<()> {
        Self::run_text(103, 0, label).map(drop)
    }

    stock! {
        // No target.
        /// Stock 16.
        reveal_all() = 16, 0, params!();
        /// Stock 23.
        timer_start() = 23, 0, params!();
        /// Stock 24.
        timer_stop() = 24, 0, params!();
        /// Stock 31.
        grow_shroud() = 31, 0, params!();
        /// Stock 45.
        ion_storm_stop() = 45, 0, params!();
        /// Stock 46.
        lock_input() = 46, 0, params!();
        /// Stock 47.
        unlock_input() = 47, 0, params!();
        /// Stock 67.
        announce_win() = 67, 0, params!();
        /// Stock 68.
        announce_lose() = 68, 0, params!();
        /// Stock 69.
        end_scenario() = 69, 0, params!();
        /// Stock 97.
        toggle_train_cargo() = 97, 0, params!();
        /// Stock 118.
        clear_smudges() = 118, 0, params!();
        /// Stock 145.
        home_camera() = 145, 0, params!();
        // A value.
        /// Stock 10.
        play_movie(movie: i32) = 10, 0, params!(value: i64::from(movie));
        /// Stock 25.
        timer_extend(seconds: i32) = 25, 0, params!(value: i64::from(seconds));
        /// Stock 26.
        timer_shorten(seconds: i32) = 26, 0, params!(value: i64::from(seconds));
        /// Stock 27.
        timer_set(seconds: i32) = 27, 0, params!(value: i64::from(seconds));
        /// Stock 28: Red Alert 2 global `n` (0..49).
        set_global(n: i32) = 28, 0, params!(value: i64::from(n));
        /// Stock 29.
        clear_global(n: i32) = 29, 0, params!(value: i64::from(n));
        /// Stock 44.
        ion_storm_start(duration: i32) = 44, 0, params!(value: i64::from(duration));
        /// Stock 56: Red Alert 2 local `n` (0..99).
        set_local(n: i32) = 56, 0, params!(value: i64::from(n));
        /// Stock 57.
        clear_local(n: i32) = 57, 0, params!(value: i64::from(n));
        /// Stock 73.
        set_ambient_light(value: i32) = 73, 0, params!(value: i64::from(value));
        /// Stock 84.
        wake_group(group: i32) = 84, 0, params!(value: i64::from(group));
        /// Stock 100.
        play_radar_movie(movie: i32) = 100, 0, params!(value: i64::from(movie));
        /// Stock 110.
        pause_game(duration: i32) = 110, 0, params!(value: i64::from(duration));
        /// Stock 114.
        set_sidebar_tab(tab: i32) = 114, 0, params!(value: i64::from(tab));
        /// Stock 127.
        chrono_screen_effect(duration: i32) = 127, 0, params!(value: i64::from(duration));
        // TeamTypes (lookup(TeamType, name)).
        /// Stock 4.
        create_team(team: TypeRef) = 4, 0, params!(id: team.id());
        /// Stock 5.
        destroy_teams(team: TypeRef) = 5, 0, params!(id: team.id());
        /// Stock 7: at the TeamType's own waypoint.
        reinforce(team: TypeRef) = 7, 0, params!(id: team.id());
        /// Stock 80.
        reinforce_at(team: TypeRef, at: Waypoint) = 80, at.0, params!(id: team.id());
        /// Stock 107.
        reinforce_by_chrono(team: TypeRef, at: Waypoint) = 107, at.0, params!(id: team.id());
        // A waypoint.
        /// Stock 8.
        drop_zone_flare(at: Waypoint) = 8, at.0, params!();
        /// Stock 17.
        reveal_around(at: Waypoint) = 17, at.0, params!();
        /// Stock 18.
        reveal_zone(at: Waypoint) = 18, at.0, params!();
        /// Stock 90.
        lightning_strike(at: Waypoint) = 90, at.0, params!();
        /// Stock 94.
        ion_cannon_strike(at: Waypoint) = 94, at.0, params!();
        /// Stock 95.
        nuke_strike(at: Waypoint) = 95, at.0, params!();
        /// Stock 96.
        chem_strike(at: Waypoint) = 96, at.0, params!();
        /// Stock 101.
        reshroud_at(at: Waypoint) = 101, at.0, params!();
        /// Stock 109.
        iron_curtain_at(at: Waypoint) = 109, at.0, params!();
        /// Stock 112.
        jump_camera(at: Waypoint) = 112, at.0, params!();
        /// Stock 41: animation by rules array position.
        play_anim_at(at: Waypoint, anim: i32) = 41, at.0, params!(id: anim);
        /// Stock 42: weapon by rules array position.
        detonate_weapon_at(at: Waypoint, weapon: i32) = 42, at.0, params!(id: weapon);
        /// Stock 48.
        center_camera(at: Waypoint, speed: i32) = 48, at.0, params!(value: i64::from(speed));
        /// Stock 58.
        meteor_shower(at: Waypoint, intensity: i32) = 58, at.0, params!(value: i64::from(intensity));
        /// Stock 108: 20 picks a random crate.
        create_crate(at: Waypoint, kind: i32) = 108, at.0, params!(value: i64::from(kind));
        /// Stock 102: the storm belongs to `house`.
        lightning_storm_strike(at: Waypoint, house: House) = 102, at.0, params!(house: house.0);
        /// Stock 125.
        create_building_at(at: Waypoint, kind: TypeRef, house: House) = 125, at.0, params!(id: kind.id(), house: house.0);
        // Types.
        /// Stock 106.
        set_tech_level(kind: TypeRef, level: i32) = 106, 0, params!(id: kind.id(), value: i64::from(level));
        /// Stock 115.
        flash_cameo(kind: TypeRef, frames: i32) = 115, 0, params!(id: kind.id(), value: i64::from(frames));
        // A house.
        /// Stock 30.
        auto_base_building(house: House, on: bool) = 30, house.0, params!(value: i64::from(on));
        /// Stock 51.
        reshroud(house: House) = 51, house.0, params!();
        /// Stock 82.
        wake_sleepers(house: House) = 82, house.0, params!();
        /// Stock 83.
        wake_harmless(house: House) = 83, house.0, params!();
        /// Stock 139.
        radar_blackout(house: House, frames: i32) = 139, house.0, params!(value: i64::from(frames));
        /// Stock 33: superweapon by `[SuperWeaponTypes]` position.
        one_time_super_weapon(house: House, kind: TypeRef) = 33, house.0, params!(id: kind.id());
        /// Stock 34.
        repeating_super_weapon(house: House, kind: TypeRef) = 34, house.0, params!(id: kind.id());
        /// Stock 129.
        super_weapon_set_charge(house: House, kind: TypeRef, percent: i32) = 129, house.0, params!(id: kind.id(), value: i64::from(percent));
        /// Stock 134.
        super_weapon_reset(house: House, kind: TypeRef) = 134, house.0, params!(id: kind.id());
        /// Stock 128.
        teleport_all(house: House, to: Waypoint) = 128, house.0, params!(cell: to.0);
        /// Stock 137.
        set_alternate_base(house: House, at: Waypoint) = 137, house.0, params!(cell: at.0);
        /// Stock 138.
        clear_alternate_base(house: House) = 138, house.0, params!();
        /// Stock 131.
        flash_buildings(house: House, kind: TypeRef, frames: i32) = 131, house.0, params!(id: kind.id(), value: i64::from(frames));
        // An object.
        /// Stock 14.
        change_house(target: Techno, house: House) = 14, target.0, params!(house: house.0);
        /// Stock 32.
        destroy_object(target: Techno) = 32, target.0, params!();
        /// Stock 60.
        sell_building(target: Techno) = 60, target.0, params!();
        /// Stock 61.
        turn_off_building(target: Techno) = 61, target.0, params!();
        /// Stock 62.
        turn_on_building(target: Techno) = 62, target.0, params!();
    }
}

// ------------------------------------------------------------------ Orders

/// Player commands an [`Order`] carries (appendix 8.3 `Command`).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Move = 0,
    MoveToObject = 1,
    Stop = 2,
    Attack = 3,
    AttackMove = 4,
    Guard = 5,
    DeployMcv = 6,
    UndeployBuilding = 7,
    SetRally = 8,
    QueueProduction = 9,
    SuspendProduction = 10,
    CancelProductionByType = 11,
    PlaceReadyBuilding = 12,
    SellBuilding = 13,
    ToggleRepair = 14,
    EjectBunker = 15,
    UnloadPassengers = 16,
    CaptureBuilding = 17,
    PlantC4 = 18,
    LaunchSuperWeapon = 19,
    ToggleInfantryDeploy = 20,
    ForceAttackCell = 21,
    SellWallAtCell = 22,
    Scatter = 23,
    TogglePrimaryFactory = 24,
    Enter = 25,
    /// Cell target, harvester units.
    Harvest = 26,
    /// No units; `value` is a factory kind index (`FACTORY_KINDS`, 2..5).
    TakeProduct = 27,
}

/// Most units one [`Order`] sent through [`House::order`] carries.
pub const ORDER_UNITS: usize = 64;

/// A player command for a computer house (RFC 6g), built with the
/// constructors below and sent with [`House::order`].
#[derive(Clone, Copy, Debug)]
pub struct Order<'a> {
    pub command: Command,
    pub target: i64,
    pub value: i64,
    pub units: &'a [Techno],
}

impl<'a> Order<'a> {
    /// Any command, with its target and value as the command table lists.
    pub const fn new(command: Command, target: i64, value: i64, units: &'a [Techno]) -> Self {
        Self {
            command,
            target,
            value,
            units,
        }
    }

    pub const fn move_to(units: &'a [Techno], cell: Cell) -> Self {
        Self::new(Command::Move, cell.0, 0, units)
    }

    pub const fn move_to_object(units: &'a [Techno], target: Techno) -> Self {
        Self::new(Command::MoveToObject, target.0, 0, units)
    }

    pub const fn stop(units: &'a [Techno]) -> Self {
        Self::new(Command::Stop, 0, 0, units)
    }

    pub const fn attack(units: &'a [Techno], target: Techno) -> Self {
        Self::new(Command::Attack, target.0, 0, units)
    }

    pub const fn attack_move(units: &'a [Techno], cell: Cell) -> Self {
        Self::new(Command::AttackMove, cell.0, 0, units)
    }

    /// Guard `target`, or the units' own place with `NO_HANDLE`.
    pub const fn guard(units: &'a [Techno], target: Techno) -> Self {
        Self::new(Command::Guard, target.0, 0, units)
    }

    pub const fn deploy_mcv(units: &'a [Techno]) -> Self {
        Self::new(Command::DeployMcv, 0, 0, units)
    }

    pub const fn set_rally(factories: &'a [Techno], cell: Cell) -> Self {
        Self::new(Command::SetRally, cell.0, 0, factories)
    }

    pub const fn queue_production(ty: TypeRef) -> Self {
        Self::new(Command::QueueProduction, 0, ty.0, &[])
    }

    pub const fn cancel_production(ty: TypeRef) -> Self {
        Self::new(Command::CancelProductionByType, 0, ty.0, &[])
    }

    pub const fn place_ready_building(ty: TypeRef, cell: Cell) -> Self {
        Self::new(Command::PlaceReadyBuilding, cell.0, ty.0, &[])
    }

    pub const fn sell(buildings: &'a [Techno]) -> Self {
        Self::new(Command::SellBuilding, 0, 0, buildings)
    }

    pub const fn launch_super_weapon(ty: TypeRef, cell: Cell) -> Self {
        Self::new(Command::LaunchSuperWeapon, cell.0, ty.0, &[])
    }

    /// Lets the finished unit out of the house's factory of `kind`, the
    /// index of `Factory.<Kind>` (2 Infantry, 3 Vehicle, 4 Naval,
    /// 5 Aircraft), as a player's sidebar does for a ready unit.
    pub const fn take_product(kind: i64) -> Self {
        Self::new(Command::TakeProduct, 0, kind, &[])
    }
}

impl Order<'_> {
    /// The `Order` layout as i64 words (kind and count, target, value, then
    /// the unit handles) and its byte length. `Err(Refused)` past
    /// [`ORDER_UNITS`] units.
    pub(crate) fn words(&self) -> Result<([i64; 3 + ORDER_UNITS], usize)> {
        let count = self.units.len();
        if count > ORDER_UNITS {
            return Err(Status::Refused);
        }
        let mut words = [0i64; 3 + ORDER_UNITS];
        words[0] = (self.command as i32 as u32 as i64) | ((count as i64) << 32);
        words[1] = self.target;
        words[2] = self.value;
        for (word, unit) in words[3..].iter_mut().zip(self.units) {
            *word = unit.0;
        }
        Ok((words, (3 + count) * 8))
    }
}

impl House {
    /// Queues `order` for this computer house; it runs next frame, checked
    /// like its player's command. `Err(Refused)` for a player's house or a
    /// command-table violation, or more than [`ORDER_UNITS`] units.
    pub fn order(self, order: &Order) -> Result<()> {
        let (words, len) = order.words()?;
        check(unsafe { sys::order(self.0, words.as_ptr().cast(), len as i32) })
    }

    /// The house's credits.
    pub fn money(self) -> Result<i64> {
        get(self, Prop::Money)
    }
}

// ------------------------------------------------------------------- Rules

/// Rules text (RFC 8): any key in `on_rules`, an allowlisted key in a match.
pub struct Rules;

impl Rules {
    /// The raw text of `[section] key` in the merged rules, written into
    /// `buf`. `Err(NotFound)` for a missing key, `Err(Refused)` when `buf`
    /// is too small.
    pub fn get<'b>(section: &str, key: &str, buf: &'b mut [u8]) -> Result<&'b str> {
        let mut len = 0;
        check(unsafe {
            sys::rules_get(
                section.as_ptr(),
                section.len() as i32,
                key.as_ptr(),
                key.len() as i32,
                buf.as_mut_ptr(),
                buf.len() as i32,
                &mut len,
            )
        })?;
        let bytes = buf.get(..len as usize).ok_or(Status::Refused)?;
        core::str::from_utf8(bytes).map_err(|_| Status::Unsupported)
    }

    /// Writes `[section] key=value` as INI text: any key in `on_rules`;
    /// during a match only allowlisted keys, otherwise `Err(Refused)`.
    pub fn set(section: &str, key: &str, value: &str) -> Result<()> {
        check(unsafe {
            sys::rules_set(
                section.as_ptr(),
                section.len() as i32,
                key.as_ptr(),
                key.len() as i32,
                value.as_ptr(),
                value.len() as i32,
            )
        })
    }
}

/// The global rules reference (kind 4, appendix 7.8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Global;

impl Ref for Global {
    fn raw(self) -> i64 {
        4 << KIND_SHIFT
    }
}

// ------------------------------------------------------------------- Cells

impl Cell {
    /// `LandType` (appendix 8.3 enum 12).
    pub fn land_type(self) -> Result<i64> {
        get(self, Prop::LandType)
    }

    /// The cell's height level.
    pub fn height(self) -> Result<i64> {
        get(self, Prop::Height)
    }

    /// The overlay type, or `NO_HANDLE` without one.
    pub fn overlay(self) -> Result<TypeRef> {
        get(self, Prop::Overlay).map(TypeRef)
    }

    /// The cell's ore and gem value in credits.
    pub fn ore(self) -> Result<i64> {
        get(self, Prop::Ore)
    }

    /// The waypoint number at this cell, or -1.
    pub fn waypoint(self) -> Result<i64> {
        get(self, Prop::Waypoint)
    }
}

impl Find {
    /// Cells, by cell index (y, then x); narrow with [`Find::near`] or
    /// [`Find::within`].
    pub fn cells() -> Self {
        Self::scope(2)
    }

    /// Types, by type ID: [`TypeRef::any`] lists every type of its name
    /// kind, a specific type only itself (TechnoTypes, Weapons, Warheads).
    /// A `Buildable` clause reads for [`Find::seen_by`]'s house.
    pub fn types(ty: TypeRef) -> Self {
        Self::scope(3).of_type(ty)
    }
}

// ------------------------------------------------------------- Properties

/// `Layer` values (appendix 8.3, enum 11).
#[repr(i64)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Underground = 0,
    Surface = 1,
    Ground = 2,
    Air = 3,
    Top = 4,
}

/// `Relation` values (appendix 8.3, enum 9): how the viewpoint house sees
/// an object's owner or a house.
#[repr(i64)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Relation {
    /// The house itself (appendix `Self`).
    SameHouse = 0,
    Ally = 1,
    Enemy = 2,
    /// A civilian or other passive house.
    Neutral = 3,
}

/// `SpeedType` values (appendix 8.3, enum 13), each with its
/// `Passable.<SpeedType>` cell property.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpeedType {
    Foot = 0,
    Track = 1,
    Wheel = 2,
    Hover = 3,
    Winged = 4,
    Float = 5,
    Amphibious = 6,
    FloatBeach = 7,
}

impl SpeedType {
    /// Its `Passable.<SpeedType>` cell property.
    pub const fn passable(self) -> Prop {
        Prop(Prop::Passable_Foot.0 + self as i32)
    }
}

/// Getters of a unit or building's appendix 7.1 properties: one `get` each.
/// `Relation` is a viewpoint property: read it in a filter clause or a
/// [`Find::seen_by`] query, not here.
impl Techno {
    /// The object's type.
    pub fn type_ref(self) -> Result<TypeRef> {
        get(self, Prop::Type).map(TypeRef)
    }

    pub fn owner(self) -> Result<House> {
        get(self, Prop::Owner).map(House)
    }

    /// The display layer ([`Layer`] numbers).
    pub fn layer(self) -> Result<i64> {
        get(self, Prop::Layer)
    }

    /// The current target: an object or a cell reference, `NO_HANDLE`
    /// without one.
    pub fn target(self) -> Result<i64> {
        get(self, Prop::Target)
    }

    /// Veterancy in percent: 100 is veteran, 200 elite.
    pub fn experience(self) -> Result<i64> {
        get(self, Prop::Experience)
    }

    /// Ore carried, in bales.
    pub fn storage(self) -> Result<i64> {
        get(self, Prop::Storage)
    }

    /// The transport or building this object is in, `NO_HANDLE` outside.
    pub fn transport(self) -> Result<Techno> {
        get(self, Prop::Transport).map(Techno)
    }

    pub fn passenger_count(self) -> Result<i64> {
        get(self, Prop::PassengerCount)
    }

    pub fn transport_space_left(self) -> Result<i64> {
        get(self, Prop::TransportSpaceLeft)
    }

    /// The type a spy or Mirage shows, `NO_HANDLE` when undisguised.
    pub fn disguise_type(self) -> Result<TypeRef> {
        get(self, Prop::DisguiseType).map(TypeRef)
    }

    /// The house a spy shows, `NO_HANDLE` when undisguised.
    pub fn disguise_owner(self) -> Result<House> {
        get(self, Prop::DisguiseOwner).map(House)
    }

    pub fn moving(self) -> Result<bool> {
        get(self, Prop::Moving).map(|v| v != 0)
    }

    /// Body facing 0..255, as map unit facings count.
    pub fn facing(self) -> Result<i64> {
        get(self, Prop::Facing)
    }

    /// Turret facing 0..255; `Err(Unsupported)` without a turret.
    pub fn turret_facing(self) -> Result<i64> {
        get(self, Prop::TurretFacing)
    }

    /// The armor's ID.
    pub fn armor(self) -> Result<i64> {
        get(self, Prop::Armor)
    }
}

impl Cell {
    /// Whether the terrain lets `speed` through (`Passable.<SpeedType>`).
    pub fn passable(self, speed: SpeedType) -> Result<bool> {
        get(self, speed.passable()).map(|v| v != 0)
    }
}

impl Find {
    /// Reads viewpoint properties (`Relation`, `Revealed`, `Placeable`) for
    /// `house`.
    pub fn seen_by(mut self, house: House) -> Self {
        self.0.query.viewpoint = house.0;
        self
    }
}

/// Global properties (appendix 7.8): one `get` each.
impl Global {
    pub fn frame() -> Result<i64> {
        get(Global, Prop::Frame)
    }

    /// The trigger difficulty: 0 easy, 1 normal, 2 hard.
    pub fn difficulty() -> Result<i64> {
        get(Global, Prop::Difficulty)
    }

    /// Frames left on the mission timer, also while it is stopped.
    pub fn mission_timer() -> Result<i64> {
        get(Global, Prop::MissionTimer)
    }

    pub fn mission_timer_running() -> Result<bool> {
        get(Global, Prop::MissionTimerRunning).map(|v| v != 0)
    }

    pub fn ambient_light() -> Result<i64> {
        get(Global, Prop::AmbientLight)
    }

    /// Whether a lightning storm rages.
    pub fn ion_storm_active() -> Result<bool> {
        get(Global, Prop::IonStormActive).map(|v| v != 0)
    }
}

// ----------------------------------------------------------------- Exports

/// Exports `on_scenario_start`, calling `$f`.
#[macro_export]
macro_rules! on_scenario_start {
    ($f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn on_scenario_start() {
            $f()
        }
    };
}

/// Exports `on_frame`, calling `$f` every frame or, with `every = N`, on
/// frames divisible by `N`.
#[macro_export]
macro_rules! on_frame {
    (every = $n:expr, $f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn on_frame() {
            if $crate::frame() % ($n) == 0 {
                $f()
            }
        }
    };
    ($f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn on_frame() {
            $f()
        }
    };
}

/// Exports `on_rules`, calling `$f` once at the module's rules layer, where
/// [`Rules::set`] writes any key before the rules are processed.
#[macro_export]
macro_rules! on_rules {
    ($f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn on_rules() {
            $f()
        }
    };
}

/// Exports `vera_init`, the module's one-time setup.
#[macro_export]
macro_rules! init {
    ($f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn vera_init() {
            $f()
        }
    };
}

#[cfg(all(feature = "panic-handler", target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // A trap disables only this module (RFC section 3).
    core::arch::wasm32::unreachable()
}
