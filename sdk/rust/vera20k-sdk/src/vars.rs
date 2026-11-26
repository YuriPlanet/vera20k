//! Declared variables (RFC 0001 section 6e, appendix "Custom sections").
//!
//! [`vars!`](crate::vars!) writes the module's `vera.vars` section at build
//! time and one accessor per variable. An accessor resolves its ID with
//! `lookup(Variable, name)` on first use and keeps it in module memory, which
//! saves carry.
//!
//! ```ignore
//! vera20k_sdk::vars! {
//!     pub static WAVE: GlobalVar = ("Wave", Public, 0);
//!     pub static SCORE: HouseVar = ("Score", Allies, 0);
//!     pub static PANICKED: ObjectVar = ("Panicked", Private, 0);
//! }
//! ```
//!
//! Limitation: a module declares all its variables in one `vars!` call. The
//! linker joins same-named custom sections from several statics or crates
//! into one byte run, which the engine rejects as malformed.

use core::sync::atomic::{AtomicI32, Ordering};

use crate::{House, NameKind, Ref, Result, check, lookup, sys};

/// A variable's scope (appendix 8.3 `Scope`).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Global = 0,
    House = 1,
    Object = 2,
}

/// Which clients may read a variable (appendix 8.3 `Visibility`).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visibility {
    Private = 0,
    Allies = 1,
    /// Object scope only.
    InSight = 2,
    Public = 3,
}

/// One declaration, as [`vars!`](crate::vars!) collects them.
pub struct Decl {
    /// ASCII.
    pub name: &'static str,
    pub scope: Scope,
    pub visibility: Visibility,
    pub default: i64,
}

/// The `vera.vars` section's length for `decls`.
pub const fn section_len(decls: &[Decl]) -> usize {
    let mut len = 1 + 4;
    let mut i = 0;
    while i < decls.len() {
        len += 2 + decls[i].name.len() + 1 + 1 + 8;
        i += 1;
    }
    len
}

/// The `vera.vars` section for `decls`; `N` is [`section_len`].
pub const fn section<const N: usize>(decls: &[Decl]) -> [u8; N] {
    let mut out = [0u8; N];
    out[0] = 1;
    let count = (decls.len() as u32).to_le_bytes();
    let mut at = 1;
    let mut b = 0;
    while b < 4 {
        out[at] = count[b];
        at += 1;
        b += 1;
    }
    let mut i = 0;
    while i < decls.len() {
        let name = decls[i].name.as_bytes();
        let len = (name.len() as u16).to_le_bytes();
        out[at] = len[0];
        out[at + 1] = len[1];
        at += 2;
        let mut c = 0;
        while c < name.len() {
            out[at] = name[c];
            at += 1;
            c += 1;
        }
        out[at] = decls[i].scope as u8;
        out[at + 1] = decls[i].visibility as u8;
        at += 2;
        let default = decls[i].default.to_le_bytes();
        let mut d = 0;
        while d < 8 {
            out[at] = default[d];
            at += 1;
            d += 1;
        }
        i += 1;
    }
    out
}

/// A declared variable's name and its ID once resolved.
pub struct Declared {
    name: &'static str,
    id: AtomicI32,
}

impl Declared {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            id: AtomicI32::new(-1),
        }
    }

    /// The variable's ID in this match.
    pub fn id(&self) -> Result<i32> {
        let id = self.id.load(Ordering::Relaxed);
        if id >= 0 {
            return Ok(id);
        }
        let id = lookup(NameKind::Variable, self.name)?;
        self.id.store(id, Ordering::Relaxed);
        Ok(id)
    }

    fn get(&self, scope: i64) -> Result<i64> {
        let mut out = 0;
        check(unsafe { sys::var_get(scope, self.id()?, &mut out) })?;
        Ok(out)
    }

    fn set(&self, scope: i64, value: i64) -> Result<()> {
        check(unsafe { sys::var_set(scope, self.id()?, value) })
    }
}

/// A global variable from [`vars!`](crate::vars!).
pub struct GlobalVar(Declared);

impl GlobalVar {
    pub const SCOPE: Scope = Scope::Global;

    pub const fn new(name: &'static str) -> Self {
        Self(Declared::new(name))
    }

    pub fn get(&self) -> Result<i64> {
        self.0.get(0)
    }

    pub fn set(&self, value: i64) -> Result<()> {
        self.0.set(0, value)
    }
}

/// A per-house variable from [`vars!`](crate::vars!).
pub struct HouseVar(Declared);

impl HouseVar {
    pub const SCOPE: Scope = Scope::House;

    pub const fn new(name: &'static str) -> Self {
        Self(Declared::new(name))
    }

    pub fn get(&self, house: House) -> Result<i64> {
        self.0.get(house.0)
    }

    pub fn set(&self, house: House, value: i64) -> Result<()> {
        self.0.set(house.0, value)
    }
}

/// A per-object variable from [`vars!`](crate::vars!); it goes with its
/// object.
pub struct ObjectVar(Declared);

impl ObjectVar {
    pub const SCOPE: Scope = Scope::Object;

    pub const fn new(name: &'static str) -> Self {
        Self(Declared::new(name))
    }

    pub fn get(&self, object: impl Ref) -> Result<i64> {
        self.0.get(object.raw())
    }

    pub fn set(&self, object: impl Ref, value: i64) -> Result<()> {
        self.0.set(object.raw(), value)
    }
}

/// Declares the module's variables: writes its one `vera.vars` section and a
/// static accessor per variable. Each entry is
/// `static NAME: GlobalVar | HouseVar | ObjectVar = ("Name", Visibility, default);`.
#[macro_export]
macro_rules! vars {
    ($($v:vis static $ident:ident : $kind:ident = ($name:literal, $visibility:ident, $default:expr);)*) => {
        #[doc(hidden)]
        const __VERA_VARS: &[$crate::vars::Decl] = &[$($crate::vars::Decl {
            name: $name,
            scope: $crate::vars::$kind::SCOPE,
            visibility: $crate::vars::Visibility::$visibility,
            default: $default,
        }),*];
        #[doc(hidden)]
        #[used]
        #[unsafe(link_section = "vera.vars")]
        static __VERA_VARS_SECTION: [u8; $crate::vars::section_len(__VERA_VARS)] =
            $crate::vars::section(__VERA_VARS);
        $($v static $ident: $crate::vars::$kind = $crate::vars::$kind::new($name);)*
    };
}
