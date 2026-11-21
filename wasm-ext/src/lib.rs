//! The sandbox for Wasm extension modules ([RFC 0001](../../docs/rfcs/0001-wasm-extension-modules.md)).
//!
//! A simulation module (`rules.wasm`) is checked, compiled and called under
//! an operation budget, and reaches the game only through [`SimHost`]. It
//! covers `frame`, `random`, `log`, `has_kind`, `lookup`, `name_of`, `get`,
//! `get_many`, `set`, `var_get`, `var_set`, `find` and `action`; the other
//! `vera:sim` imports, the client world and the browser runtime come later.
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
    ("has_kind", "(table: i32, id: i32) -> i32"),
    ("lookup", "(kind: i32, ptr, len, out: ptr) -> status"),
    (
        "name_of",
        "(kind: i32, id: i32, out: ptr, cap: i32, len_out: ptr) -> status",
    ),
    ("get", "(ref: i64, prop: i32, out: ptr) -> status"),
    (
        "get_many",
        "(refs: ptr, n: i32, props: ptr, m: i32, out: ptr, count_out: ptr) -> status",
    ),
    ("set", "(ref: i64, prop: i32, value: i64) -> status"),
    ("var_get", "(scope: i64, var: i32, out: ptr) -> status"),
    ("var_set", "(scope: i64, var: i32, value: i64) -> status"),
    (
        "find",
        "(query: ptr, len: i32, out: ptr, cap: i32, res: ptr) -> status",
    ),
    (
        "action",
        "(kind: i32, target: i64, params: ptr, len: i32, out: ptr) -> status",
    ),
];

/// Operations charged per value `get_many` reads and per reference `find`
/// returns, and per candidate `find` examines (RFC section 15).
pub const ENTRY_COST: u64 = 10;
pub const SCAN_COST: u64 = 2;

/// Name kinds (appendix 8.1) and their count; `lookup` and `name_of` take
/// kinds below [`NAME_KINDS`], anything else is invalid.
pub const NAME_TECHNO_TYPE: i32 = 0;
pub const NAME_COUNTRY: i32 = 11;
pub const NAME_PROPERTY: i32 = 19;
pub const NAME_KINDS: i32 = 23;

/// `has_kind` tables (appendix 8.3 `Table`).
pub const TABLE_ACTION: i32 = 1;
pub const TABLE_PROPERTY: i32 = 2;
pub const TABLE_NAME_KIND: i32 = 3;
pub const TABLE_SCOPE: i32 = 4;

/// Search scopes (appendix 8.2) and their count.
pub const SCOPE_OBJECTS: i32 = 0;
pub const SCOPE_HOUSES: i32 = 1;
pub const SCOPES: i32 = 9;

/// Every property name of appendix section 7, deduplicated (properties with
/// the same name share an ID), in order of first appearance; the ID is the
/// index. Provisional numbering until open question 5 fixes the tables.
/// `Passable.<SpeedType>` (7.6) and the global numeric INI keys (7.7, 7.8)
/// expand from rules data and are not listed yet.
#[rustfmt::skip]
pub const PROPERTIES: &[&str] = &[
    // 7.1 Objects
    "Type", "Owner", "Category", "Health", "Strength", "HealthPercent", "X", "Y", "Z", "Cell",
    "Layer", "LandType", "Mission", "Target", "Rank", "Experience", "Ammo", "Storage",
    "Transport", "PassengerCount", "TransportSpaceLeft", "Team", "Tag", "DisguiseType",
    "DisguiseOwner", "Cloaked", "Moving", "Facing", "TurretFacing", "Armor", "Online",
    "Selected", "Relation", "NearbyEnemyCount", "TargetInRange", "CanFireAtTarget",
    "DoorOpen", "Animating",
    // 7.2 Modifiable properties
    "ArmorMultiplier", "FirepowerMultiplier", "SpeedMultiplier", "ROFMultiplier",
    "WeaponRangeMultiplier", "WeaponRangeExtra", "TargetPriorityMultiplier", "CritMultiplier",
    "CritExtraChance", "DamageReceivedExtra", "ReflectDamagePercent", "RevengeWeapon",
    "PrimaryWeapon", "SecondaryWeapon", "Unkillable", "Cloakable", "ForceDecloak",
    "DisableWeapons", "DisableTargeting", "Selectable", "IgnoreOrders", "Targetable",
    "PenetratesForceShield", "ImmunityFlags", "TintColor", "TintIntensity", "LaserTrailType",
    "PowerOutage",
    // 7.3 Modifiers
    "Property", "Op", "Value", "Remaining", "Source", "Group", "Origin",
    // 7.4 Projectiles
    "VX", "VY", "VZ", "Data0", "Data1",
    // 7.5 Houses
    "Name", "Country", "Side", "Index", "Money", "PowerOutput", "PowerDrain", "LowPower",
    "TechLevel", "IsDefeated", "IsHuman", "IsCivilian", "Kills", "Losses", "AiTriggersActive",
    // 7.6 Cells
    "Height", "Overlay", "Ore", "Revealed", "Placeable", "CellTag", "Waypoint",
    // 7.7 Types and rules
    "Buildable", "Enabled", "Damage", "ROF", "Burst", "MinimumRange", "Range", "CellSpread",
    "PercentAtMax", "Wall", "Wood", "Sparky", "Fire", "Conventional", "Rocker", "InfDeath",
    "Cost", "Speed", "ROT", "Points", "Soylent", "Crushable", "Selfhealing", "Explodes",
    "Power", "Inaccurate", "Arcing", "Shadow", "Sight", "InitialAmmo", "Passengers",
    "SizeLimit",
    // 7.8 Global references
    "Frame", "Difficulty", "GameMode", "PlayerCount", "MissionTimer", "MissionTimerRunning",
    "AmbientLight", "IonStormActive", "GameSpeed",
    // 7.9 Triggers, tags, teams and superweapons
    "House", "Repeat", "MapId", "Script", "Cursor", "Succeeded", "MemberCount", "Ready",
    "ChargePercent", "RechargeTime",
    // 7.10 Client nodes and client globals
    "Label", "Art", "Width", "Hold", "Gap", "Visible", "Dismissable", "Clickable",
    "Hoverable", "Items", "ItemProgress", "ItemTooltips", "Fill", "Anchor", "Palette", "Tint",
    "Anim", "HideAnchor", "Number", "Viewer", "IsSpectator", "CaptureMapClick",
];

/// A property's ID by name, ASCII case-insensitive.
pub fn property_id(name: &str) -> Option<i32> {
    PROPERTIES
        .iter()
        .position(|p| p.eq_ignore_ascii_case(name))
        .map(|i| i as i32)
}

/// A property's name; `None` outside the table.
pub fn property_name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|i| PROPERTIES.get(i).copied())
}

/// Filter clause comparison (appendix 8.3 `Op`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Op {
    fn parse(op: u8) -> Result<Self, Invalid> {
        Ok(match op {
            0 => Self::Eq,
            1 => Self::Ne,
            2 => Self::Lt,
            3 => Self::Le,
            4 => Self::Gt,
            5 => Self::Ge,
            _ => return Err(Invalid(format!("unknown clause op {op}"))),
        })
    }

    /// Whether `reading op constant` holds.
    pub fn holds(self, reading: i64, constant: i64) -> bool {
        match self {
            Self::Eq => reading == constant,
            Self::Ne => reading != constant,
            Self::Lt => reading < constant,
            Self::Le => reading <= constant,
            Self::Gt => reading > constant,
            Self::Ge => reading >= constant,
        }
    }
}

/// What a query clause compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClauseSubject {
    /// A property of the candidate.
    Property(i32),
    /// A variable: `subject` scope is the candidate's, otherwise global.
    Var { var: i32, subject: bool },
}

/// One filter clause of a query (appendix "Filter clause").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clause {
    pub subject: ClauseSubject,
    pub op: Op,
    pub value: i64,
}

/// Where a query looks (`Query.area`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Map,
    /// Within `radius` leptons of the cell's center.
    Circle {
        cell: i64,
        radius: i32,
    },
    /// `width` by `height` cells from the top-left cell.
    Rect {
        cell: i64,
        width: i32,
        height: i32,
    },
}

/// A `find` query (appendix "Query"), checked for the shape rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Below [`SCOPES`].
    pub scope: i32,
    pub area: Area,
    pub type_ref: i64,
    pub owner: i64,
    pub viewpoint: i64,
    /// Sort keys below this are skipped.
    pub after: i64,
    pub clauses: Vec<Clause>,
}

impl Query {
    pub const HEADER: usize = 64;
    pub const CLAUSE: usize = 16;
    /// Clauses per query (open question 7).
    pub const MAX_CLAUSES: usize = 16;

    pub fn parse(bytes: &[u8]) -> Result<Self, Invalid> {
        let invalid = |why: String| Err(Invalid(format!("Query: {why}")));
        let Some(header) = bytes.get(..Self::HEADER) else {
            return invalid(format!("{} bytes, header is {}", bytes.len(), Self::HEADER));
        };
        let scope = le_i32(header, 0);
        let area = le_i32(header, 4);
        let cell = le_i64(header, 8);
        let radius = le_i32(header, 16);
        let height = le_i32(header, 20);
        let clause_count = le_i32(header, 56);
        if !(0..SCOPES).contains(&scope) {
            return invalid(format!("unknown scope {scope}"));
        }
        if le_i32(header, 60) != 0 {
            return invalid("reserved is not 0".into());
        }
        let after = le_i64(header, 48);
        if after < 0 {
            return invalid(format!("after {after} is negative"));
        }
        let count = usize::try_from(clause_count)
            .ok()
            .filter(|n| *n <= Self::MAX_CLAUSES)
            .ok_or_else(|| Invalid(format!("Query: {clause_count} clauses")))?;
        if bytes.len() != Self::HEADER + count * Self::CLAUSE {
            return invalid(format!("{} bytes for {count} clauses", bytes.len()));
        }
        let area = match area {
            0 => Area::Map,
            1 if radius >= 0 => Area::Circle { cell, radius },
            2 if radius >= 0 && height >= 0 => Area::Rect {
                cell,
                width: radius,
                height,
            },
            _ => return invalid(format!("area {area} with radius {radius}, height {height}")),
        };
        let clauses = bytes[Self::HEADER..]
            .chunks_exact(Self::CLAUSE)
            .map(Clause::parse)
            .collect::<Result<_, _>>()?;
        Ok(Self {
            scope,
            area,
            type_ref: le_i64(header, 24),
            owner: le_i64(header, 32),
            viewpoint: le_i64(header, 40),
            after,
            clauses,
        })
    }
}

impl Clause {
    fn parse(bytes: &[u8]) -> Result<Self, Invalid> {
        let invalid = |why: String| Err(Invalid(format!("Clause: {why}")));
        let (subject, op, scope, reserved) = (bytes[0], bytes[1], bytes[2], bytes[3]);
        let prop = le_i32(bytes, 4);
        if reserved != 0 {
            return invalid("reserved is not 0".into());
        }
        let subject = match (subject, scope) {
            (0, 0) if property_name(prop).is_some() => ClauseSubject::Property(prop),
            (0, _) => return invalid(format!("property {prop}, scope {scope}")),
            // Queries take only Global and Subject variables.
            (3, 0 | 1) => ClauseSubject::Var {
                var: prop,
                subject: scope == 1,
            },
            (3, _) => return invalid(format!("variable scope {scope} in a query")),
            (1 | 2, _) => {
                return invalid("Other and House clauses are not valid in a query".into());
            }
            _ => return invalid(format!("unknown subject {subject}")),
        };
        Ok(Self {
            subject,
            op: Op::parse(op)?,
            value: le_i64(bytes, 8),
        })
    }
}

/// One page of `find` results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindPage {
    /// (sort key, reference), in sort-key order, at most the page capacity.
    pub found: Vec<(i64, i64)>,
    /// Candidates examined, charged at [`SCAN_COST`] each.
    pub scanned: u64,
}

fn le_i32(bytes: &[u8], at: usize) -> i32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&bytes[at..at + 4]);
    i32::from_le_bytes(b)
}

fn le_i64(bytes: &[u8], at: usize) -> i64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&bytes[at..at + 8]);
    i64::from_le_bytes(b)
}

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
    /// Whether entry `id` of `table` (appendix 8.3 `Table`) is available.
    fn has_kind(&mut self, table: i32, id: i32) -> bool;
    /// A name's ID in name kind `kind` (below [`NAME_KINDS`], not
    /// [`NAME_PROPERTY`], which the sandbox answers).
    fn lookup(&mut self, kind: i32, name: &str) -> Result<(i32, Status), Invalid>;
    /// An ID's name, for the kinds [`SimHost::lookup`] takes; `id` is not −1.
    fn name_of(&mut self, kind: i32, id: i32) -> Result<(String, Status), Invalid>;
    /// A property's effective value; `prop` is a [`PROPERTIES`] ID.
    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid>;
    /// Writes a property through its owner; `prop` is a [`PROPERTIES`] ID.
    fn set(&mut self, reference: i64, prop: i32, value: i64) -> Result<Status, Invalid>;
    /// Up to `cap` (at least 1) matches of `query`, from sort key
    /// `query.after` on.
    fn find(&mut self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid>;
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
