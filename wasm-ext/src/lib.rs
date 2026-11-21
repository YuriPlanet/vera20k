//! The sandbox for Wasm extension modules ([RFC 0001](../../docs/rfcs/0001-wasm-extension-modules.md)).
//!
//! This is the minimal first step: a simulation module (`rules.wasm`) is
//! checked, compiled and called under an operation budget, and reaches the
//! game only through [`SimHost`]. It covers `frame`, `random`, `log`,
//! `var_get`, `var_set` and `action`; the other `vera:sim` imports, the
//! client world, saves and the browser runtime come later.
//!
//! ABI deviation from RFC 0.0.14: an import that returns a value and a status
//! takes a trailing `out` pointer for the value and returns the status. Stable
//! Rust lowers a returned pair to a hidden result pointer and AssemblyScript
//! has no multi-value imports, so neither official SDK language could import
//! the RFC's `(value, status)` signatures. `frame` cannot fail and returns its
//! value directly. See [`IMPORTS`].

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod native;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use native::SimModule;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod browser;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use browser::SimModule;

/// The import module name for API 1.0 (`vera:sim@<major>.<minor>`).
pub const SIM_IMPORT_MODULE: &str = "vera:sim@1.0";

/// The simulation imports this engine provides, with their core Wasm
/// signatures as text (`params -> results`).
pub const IMPORTS: &[(&str, &str)] = &[
    ("frame", "() -> i32"),
    ("random", "(lo: i32, hi: i32, out: ptr) -> status"),
    ("log", "(ptr, len)"),
    ("var_get", "(scope: i64, var: i32, out: ptr) -> status"),
    ("var_set", "(scope: i64, var: i32, value: i64) -> status"),
    (
        "action",
        "(kind: i32, target: i64, params: ptr, len: i32, out: ptr) -> status",
    ),
];

/// Bytes in one Wasm memory page.
pub const PAGE_BYTES: usize = 65_536;

/// A module's state between calls (RFC section 4): its memory and mutable
/// globals. Saves carry it beside the module bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModuleState {
    /// Memory size in pages.
    pub pages: u32,
    /// Pages that are not all zero: (page index, [`PAGE_BYTES`] bytes).
    pub nonzero_pages: Vec<(u32, Vec<u8>)>,
    /// The raw bits of each mutable global, in global index order.
    pub globals: Vec<u64>,
}

/// Operations charged for each host call, on top of the call's own work.
pub const HOST_CALL_COST: u64 = 50;

/// RFC appendix "Status".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    Gone = 1,
    NotFound = 2,
    Refused = 3,
    Unsupported = 4,
}

/// A host call the module made with arguments the API calls invalid. The
/// module is disabled (RFC section 3, "Error model").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid(pub String);

/// `ActionParams` (RFC appendix "Fixed layouts"), 72 bytes little endian.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActionParams {
    pub id: i32,
    pub arg: i32,
    pub arg2: i32,
    pub value: i64,
    pub value2: i64,
    pub cell: i64,
    pub house: i64,
    pub other: i64,
}

impl ActionParams {
    pub const SIZE: usize = 72;

    /// Reads the fixed layout. `text` and `extra` are not used by any action
    /// this engine implements yet.
    pub fn parse(bytes: &[u8]) -> Result<Self, Invalid> {
        if bytes.len() != Self::SIZE {
            return Err(Invalid(format!(
                "ActionParams is {} bytes, expected {}",
                bytes.len(),
                Self::SIZE
            )));
        }
        let i32_at =
            |o: usize| i32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        let i64_at = |o: usize| {
            let mut b = [0u8; 8];
            b.copy_from_slice(&bytes[o..o + 8]);
            i64::from_le_bytes(b)
        };
        Ok(Self {
            id: i32_at(0),
            arg: i32_at(4),
            arg2: i32_at(8),
            value: i64_at(16),
            value2: i64_at(24),
            cell: i64_at(32),
            house: i64_at(40),
            other: i64_at(48),
        })
    }
}

/// What a simulation module may do to the game. The engine implements it over
/// its state owners; every call happens synchronously inside a module call.
pub trait SimHost {
    /// The current logic frame.
    fn frame(&mut self) -> i32;
    /// A draw in `[lo, hi]` from the module's own stream; `lo <= hi` is
    /// checked before the call.
    fn random(&mut self, lo: i32, hi: i32) -> i32;
    /// One line for the in-game log.
    fn log(&mut self, line: &str);
    fn var_get(&mut self, scope: i64, var: i32) -> Result<(i64, Status), Invalid>;
    fn var_set(&mut self, scope: i64, var: i32, value: i64) -> Result<Status, Invalid>;
    /// Runs action `kind`; returns the created object or 0, and the status.
    fn action(
        &mut self,
        kind: i32,
        target: i64,
        params: &ActionParams,
    ) -> Result<(i64, Status), Invalid>;
}

/// Size and budget limits for one module (RFC section 15.2, open question 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum linear memory in bytes.
    pub memory_bytes: usize,
}

impl Limits {
    /// Map modules: 16 MiB.
    pub const MAP: Self = Self {
        memory_bytes: 16 << 20,
    };
    /// Mod modules: 32 MiB.
    pub const MOD: Self = Self {
        memory_bytes: 32 << 20,
    };
}

/// Why a module could not be loaded. It never runs; the match should not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The module breaks a load rule (RFC section 3).
    Invalid(String),
    /// This build cannot run modules (the browser build, for now).
    Unsupported(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(why) => write!(f, "invalid module: {why}"),
            Self::Unsupported(why) => write!(f, "modules unsupported: {why}"),
        }
    }
}

/// The lifecycle exports this engine calls (RFC appendix 1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    Init,
    ScenarioStart,
    Frame,
}

impl Export {
    pub const ALL: [Self; 3] = [Self::Init, Self::ScenarioStart, Self::Frame];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Init => "vera_init",
            Self::ScenarioStart => "on_scenario_start",
            Self::Frame => "on_frame",
        }
    }
}

/// Why a module call stopped. Every fault disables the module on every
/// machine in the same frame, since each is a function of module bytes,
/// game state and the budget alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The call used up its operation budget.
    Budget,
    /// The module trapped (unreachable, out-of-bounds access, division by
    /// zero, stack exhaustion, ...).
    Trap(String),
    /// A host call had invalid arguments.
    Invalid(String),
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Budget => f.write_str("exceeded its operation budget"),
            Self::Trap(why) => write!(f, "trapped: {why}"),
            Self::Invalid(why) => write!(f, "made an invalid call: {why}"),
        }
    }
}

#[cfg(test)]
mod sandbox_tests;
