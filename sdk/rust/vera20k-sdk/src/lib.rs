//! Rust SDK for VERA20K simulation modules (`rules.wasm`), RFC 0001 preview.
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

pub mod sys;

pub mod prelude {
    pub use crate::{
        Action, Category, Cell, Find, House, NameKind, Op, Prop, Ref, Status, Techno, TypeRef,
        Var, Waypoint, frame, log, random,
    };
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
    String = 18,
    Property = 19,
    EventKind = 20,
    ActionKind = 21,
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

/// The ID of `name` in name kind `kind`.
pub fn lookup(kind: NameKind, name: &str) -> Result<i32> {
    let mut out = -1;
    check(unsafe { sys::lookup(kind as i32, name.as_ptr(), name.len() as i32, &mut out) })?;
    Ok(out)
}

/// Writes the name of `id` into `buf` and returns it. `Err(Refused)` when
/// `buf` is too small.
pub fn name_of(kind: NameKind, id: i32, buf: &mut [u8]) -> Result<&str> {
    let mut len = 0;
    check(unsafe {
        sys::name_of(kind as i32, id, buf.as_mut_ptr(), buf.len() as i32, &mut len)
    })?;
    let bytes = buf.get(..len as usize).ok_or(Status::Refused)?;
    core::str::from_utf8(bytes).map_err(|_| Status::Unsupported)
}

// -------------------------------------------------------------- Properties

/// A property ID (appendix section 7).
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

// --------------------------------------------------------------- Variables

/// A variable (appendix "IDs"): Red Alert 2 locals 0..99, globals 100..149.
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

/// A `find` query (appendix 8.2): one host call per [`Find::first`] or
/// [`Find::page`].
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
    /// wrote and where the next page starts (0 on the last page).
    pub fn page(&mut self, out: &mut [i64], after: i64) -> Result<(usize, i64)> {
        self.0.query.after = after;
        let len = size_of::<sys::Query>()
            + size_of::<sys::Clause>() * self.0.query.clause_count as usize;
        let mut result = sys::FindResult::default();
        let status = unsafe {
            sys::find(
                (&raw const self.0).cast(),
                len as i32,
                out.as_mut_ptr(),
                out.len() as i32,
                &mut result,
            )
        };
        check(status)?;
        Ok((result.count as usize, result.next))
    }

    /// The first match, or `NO_HANDLE`.
    pub fn first(mut self) -> i64 {
        let mut out = [NO_HANDLE];
        match self.page(&mut out, 0) {
            Ok((1, _)) => out[0],
            _ => NO_HANDLE,
        }
    }
}

// ----------------------------------------------------------------- Actions

/// Atomic actions (appendix 5.3), numbered from 1000 in table order.
/// Provisional numbers (RFC open question 5).
pub struct Action;

impl Action {
    pub const ADD_MONEY: i32 = 1015;

    /// Runs action `kind` on `target`; returns the created object or 0.
    pub fn run(kind: i32, target: impl Ref, params: &sys::ActionParams) -> Result<i64> {
        let mut out = 0;
        check(unsafe {
            sys::action(
                kind,
                target.raw(),
                params,
                size_of::<sys::ActionParams>() as i32,
                &mut out,
            )
        })?;
        Ok(out)
    }

    /// Gives `house` `amount` credits (negative takes them).
    pub fn add_money(house: House, amount: i32) -> Result<()> {
        let params = sys::ActionParams {
            value: i64::from(amount),
            ..Default::default()
        };
        Self::run(Self::ADD_MONEY, house, &params).map(drop)
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
