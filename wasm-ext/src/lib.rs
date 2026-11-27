//! The sandbox for Wasm extension modules ([RFC 0001](../../docs/rfcs/0001-wasm-extension-modules.md)).
//!
//! A simulation module (`rules.wasm`) is checked, compiled and called under
//! an operation budget, and reaches the game only through [`SimHost`]. It
//! covers `frame`, `random`, `log`, `has_kind`, `lookup`, `name_of`, `get`,
//! `get_many`, `set`, `modify`, `unmodify`, `var_get`, `var_set`, `find`,
//! `action`, the four `trigger_*` imports, `rules_get`, `rules_set` and
//! `order`, `ini_register` and `send_notify`, calls `on_rules`, delivers
//! `on_events` batches ([`triggers`]) and client messages (`on_message`,
//! RFC 0001 section 10), asks `on_decide` for decisions ([`decide`]) and
//! reads the `vera.vars`, `vera.actions`, `vera.notify` and `vera.delivery`
//! sections ([`sections`]). A client module (`ui.wasm`, [`ClientModuleOn`])
//! runs under `vera:client@1.0` and reaches the game through [`ClientHost`]
//! ([`client`]). Desktop builds run modules with Wasmtime, WASI tools with wasmi,
//! and the browser with its WebAssembly engine, all after the same rewrite.
//!
//! ABI deviation from RFC 0.0.14: an import that returns a value and a status
//! takes a trailing `out` pointer for the value and returns the status. Stable
//! Rust lowers a returned pair to a hidden result pointer and AssemblyScript
//! has no multi-value imports, so neither official SDK language could import
//! the RFC's `(value, status)` signatures. `frame` cannot fail and returns its
//! value directly. See [`IMPORTS`].

mod action_kinds;
mod client_host;
mod client_module;
mod host;
mod instrument;
mod module;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod native;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod browser;

pub use module::{MAX_BATCH, SimModuleOn};

pub mod sections;
pub use sections::{VarDecl, VarScope, Visibility, notify_id};

pub use action_kinds::{
    ATOMIC_ACTIONS, FIRST_ATOMIC_ACTION, STOCK_ACTIONS, action_kind_id, action_kind_name,
};

mod event_kinds;
pub use event_kinds::{DELIVERED_EVENTS, FIRST_DELIVERED_EVENT, STOCK_EVENTS, event_kind_id};

/// A simulation module on this platform's runtime: Wasmtime on the
/// desktop, wasmi on WASI, and the browser's own WebAssembly engine in the
/// browser. All run the same rewritten bytes ([`instrument`]) and give identical results.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type SimModule = SimModuleOn<native::Embedded>;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type SimModule = SimModuleOn<browser::Browser>;

pub mod triggers;

pub use triggers::{EventRecord, TriggerDef};

pub mod decide;

pub mod client;

pub use client::{ClientEvent, ClientEventRecord, ClientHost, NodeInit, Source};
pub use client_module::ClientModuleOn;

/// A client module on this platform's runtime, as [`SimModule`] is.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type ClientModule = ClientModuleOn<native::Embedded>;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type ClientModule = ClientModuleOn<browser::Browser>;

pub use decide::DecideRecord;

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
    (
        "modify",
        "(ref: i64, prop: i32, op: i32, value: i64, frames: i32, source: i64, group: i32, out: ptr) -> status",
    ),
    ("unmodify", "(modifier: i64) -> status"),
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
    ("trigger_create", "(def: ptr, len: i32, out: ptr) -> status"),
    ("trigger_remove", "(t: i64) -> status"),
    ("trigger_bind", "(t: i64, ref: i64) -> status"),
    ("trigger_unbind", "(t: i64, ref: i64) -> status"),
    (
        "rules_get",
        "(sec: ptr, sec_len: i32, key: ptr, key_len: i32, out: ptr, cap: i32, len_out: ptr) -> status",
    ),
    (
        "rules_set",
        "(sec: ptr, sec_len: i32, key: ptr, key_len: i32, val: ptr, val_len: i32) -> status",
    ),
    ("send_notify", "(to: i64, id: i32, ptr, len: i32) -> status"),
    ("order", "(house: i64, ptr, len: i32) -> status"),
    (
        "ini_register",
        "(kind: i32, section: i32, name: ptr, len: i32, key: i32, out: ptr) -> status",
    ),
];

/// `IniKind` (RFC 0001 appendix 8.3, enum 23): what `ini_register`
/// registers.
pub const INI_KEY: i32 = 0;
pub const INI_REGISTRY: i32 = 1;
pub const INI_EVENT: i32 = 2;

/// `IniSection` (enum 24): the sections a registered key is read from.
pub const INI_SECTIONS: [&str; 7] = [
    "TechnoType",
    "Weapon",
    "Warhead",
    "Projectile",
    "SuperWeapon",
    "Country",
    "General",
];

/// Registrations one module may make.
pub const MAX_INI_REGISTRATIONS: usize = 4096;
/// Longest registered name in bytes.
pub const MAX_INI_NAME: usize = 63;

/// Whether `name` may be registered: 1 to [`MAX_INI_NAME`] ASCII letters,
/// digits, `.` and `_`.
pub fn ini_name_valid(name: &str) -> bool {
    (1..=MAX_INI_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
}

/// Operations charged per value `get_many` reads and per reference `find`
/// returns (RFC section 15).
pub const ENTRY_COST: u64 = 10;
/// Operations per candidate `find` examines, plus [`CLAUSE_COST`] for each
/// clause of the query.
pub const SCAN_COST: u64 = 2;
pub const CLAUSE_COST: u64 = 6;
/// What a clause on a "derived, high" property (appendix 7: a fire or range
/// check or a nearby search per candidate) adds to [`CLAUSE_COST`].
pub const HIGH_CLAUSE_COST: u64 = 60;
/// The "derived, high" properties a host evaluates.
pub const HIGH_COST_PROPERTIES: &[&str] =
    &["Engage", "NearbyEnemyCount", "TargetInRange", "CanFireAtTarget"];
/// Candidates one `find` examines at most (RFC 15.4); fewer where they
/// would cost more than half the call's budget.
pub const MAX_SCAN: u64 = 16_384;
/// Keys passed over that hold no candidate (unallocated cells) per operation.
pub const SKIPS_PER_OP: u64 = 8;

/// Name kinds (appendix 8.1) and their count; `lookup` and `name_of` take
/// kinds below [`NAME_KINDS`], anything else is invalid.
pub const NAME_TECHNO_TYPE: i32 = 0;
pub const NAME_WEAPON: i32 = 1;
pub const NAME_WARHEAD: i32 = 2;
pub const NAME_PROJECTILE: i32 = 3;
pub const NAME_ANIM: i32 = 4;
pub const NAME_VOXEL_ANIM: i32 = 5;
pub const NAME_PARTICLE_SYSTEM: i32 = 6;
pub const NAME_SUPER_WEAPON: i32 = 7;
pub const NAME_OVERLAY: i32 = 9;
pub const NAME_COUNTRY: i32 = 11;
pub const NAME_TEAM_TYPE: i32 = 12;
pub const NAME_VARIABLE: i32 = 15;
/// Engine-held strings: parameter texts and the INI start records' names
/// and values (RFC 0001 section 6h).
pub const NAME_STRING: i32 = 18;
pub const NAME_PROPERTY: i32 = 19;
/// Answered by the sandbox from [`STOCK_ACTIONS`] and [`ATOMIC_ACTIONS`].
pub const NAME_ACTION_KIND: i32 = 21;
/// Notifications (`vera.notify`, RFC 0001 section 10): the sandbox answers
/// `lookup` and `name_of` from the calling module's own section.
pub const NAME_NOTIFY: i32 = 16;
/// Hotkeys a client module registered (`RegisterHotkey`, appendix 5.4):
/// only `client_lookup` answers it.
pub const NAME_HOTKEY: i32 = 22;
/// Registered module events (RFC 0001 section 6h).
pub const NAME_MODULE_EVENT: i32 = 24;
pub const NAME_KINDS: i32 = 25;

/// `has_kind` tables (appendix 8.3 `Table`).
pub const TABLE_ACTION: i32 = 1;
pub const TABLE_PROPERTY: i32 = 2;
pub const TABLE_NAME_KIND: i32 = 3;
pub const TABLE_SCOPE: i32 = 4;

/// Search scopes (appendix 8.2) and their count.
pub const SCOPE_OBJECTS: i32 = 0;
pub const SCOPE_HOUSES: i32 = 1;
pub const SCOPE_CELLS: i32 = 2;
pub const SCOPE_TYPES: i32 = 3;
pub const SCOPE_MODIFIERS: i32 = 7;
pub const SCOPE_SUPER_WEAPONS: i32 = 8;
pub const SCOPES: i32 = 9;

/// Every property name of appendix section 7, deduplicated (properties with
/// the same name share an ID), in order of first appearance; the ID is the
/// index. Provisional numbering until open question 5 fixes the tables.
/// `Passable.<SpeedType>` (7.6) comes last, one per `SpeedType=` value in
/// the order the INI reader numbers them; the global numeric INI keys (7.7,
/// 7.8) expand from rules data and are not listed yet.
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
    // 7.6 Cells: Passable.<SpeedType>
    "Passable.Foot", "Passable.Track", "Passable.Wheel", "Passable.Hover", "Passable.Winged",
    "Passable.Float", "Passable.Amphibious", "Passable.FloatBeach",
    // RFC 0001 6i (0.0.21): driving a house. 7.1 objects
    "Destination", "WeaponReady", "Deployed", "UnderConstruction", "Parasite", "ControlGroup",
    // (`Visible` shares the client node property's ID)
    "Engage",
    // 7.5 houses
    "ControlDomains",
    "Factory.Building.Type", "Factory.Building.Progress", "Factory.Building.Ready",
    "Factory.Building.Queued",
    "Factory.Defense.Type", "Factory.Defense.Progress", "Factory.Defense.Ready",
    "Factory.Defense.Queued",
    "Factory.Infantry.Type", "Factory.Infantry.Progress", "Factory.Infantry.Ready",
    "Factory.Infantry.Queued",
    "Factory.Vehicle.Type", "Factory.Vehicle.Progress", "Factory.Vehicle.Ready",
    "Factory.Vehicle.Queued",
    "Factory.Naval.Type", "Factory.Naval.Progress", "Factory.Naval.Ready",
    "Factory.Naval.Queued",
    "Factory.Aircraft.Type", "Factory.Aircraft.Progress", "Factory.Aircraft.Ready",
    "Factory.Aircraft.Queued",
    // 7.6 cells: Zone.<MovementZone> in MovementZone order, then Obstructed
    "Zone.Normal", "Zone.Crusher", "Zone.Destroyer", "Zone.AmphibiousDestroyer",
    "Zone.AmphibiousCrusher", "Zone.Amphibious", "Zone.Subterrannean", "Zone.Infantry",
    "Zone.InfantryDestroyer", "Zone.Fly", "Zone.Water", "Zone.WaterBeach", "Zone.CrusherAll",
    "Obstructed",
    // 7.8 global
    "AutomationAllowed",
    // RFC 0001 0.0.23: 7.2 modifiable properties
    "ROTMultiplier",
    // 7.10 client nodes (0.0.25)
    "Backdrop",
    // 7.1 objects (0.2.0): one object's drawn type
    "ImageOverride",
];

/// `Domain` (appendix 8.3, enum 25): the bits of a house's `ControlDomains`.
pub const DOMAIN_PRODUCTION: i64 = 1 << 0;
pub const DOMAIN_FORCES: i64 = 1 << 1;
pub const DOMAIN_ABILITIES: i64 = 1 << 2;
pub const DOMAIN_SUPER_WEAPONS: i64 = 1 << 3;
pub const DOMAIN_ALL: i64 = 0b1111;

/// `RefuseReason` (appendix 8.3, enum 26): `OrderRefused`'s `value2`.
pub mod refuse_reason {
    pub const OTHER: i64 = 0;
    pub const GONE: i64 = 1;
    pub const IGNORES_ORDERS: i64 = 2;
    pub const ILLEGAL_TARGET: i64 = 3;
    pub const PREREQUISITE: i64 = 4;
    pub const CANNOT_AFFORD: i64 = 5;
    pub const QUEUE_FULL: i64 = 6;
    pub const CANNOT_PLACE: i64 = 7;
    pub const NOT_READY: i64 = 8;
    pub const VETOED: i64 = 9;
    pub const RATE_LIMITED: i64 = 10;
}

/// `EngageResult` (appendix 8.3, enum 27): the `Engage` property.
pub const ENGAGE_OK: i64 = 0;
pub const ENGAGE_ILLEGAL: i64 = 1;
pub const ENGAGE_OUT_OF_RANGE: i64 = 2;
pub const ENGAGE_NO_DAMAGE: i64 = 3;

/// Factory kinds of the `Factory.<Kind>.*` properties and `TakeProduct`'s
/// value (appendix 7.5, 8.3), in that order.
pub const FACTORY_KINDS: [&str; 6] = ["Building", "Defense", "Infantry", "Vehicle", "Naval", "Aircraft"];

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
    /// Candidates to examine at most: [`MAX_SCAN`], or the fewer the caller
    /// can pay for.
    pub max_scan: u64,
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
            max_scan: MAX_SCAN,
        })
    }

    /// Operations for each candidate examined: [`HIGH_CLAUSE_COST`] more
    /// for each clause on a [`HIGH_COST_PROPERTIES`] property.
    pub fn scan_cost(&self) -> u64 {
        self.clauses.iter().fold(SCAN_COST, |cost, clause| {
            let high = matches!(
                clause.subject,
                ClauseSubject::Property(prop)
                    if property_name(prop).is_some_and(|name| HIGH_COST_PROPERTIES.contains(&name))
            );
            cost + CLAUSE_COST + if high { HIGH_CLAUSE_COST } else { 0 }
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
    /// Candidates examined, charged at [`Query::scan_cost`] each.
    pub scanned: u64,
    /// Keys passed over that hold no candidate, [`SKIPS_PER_OP`] an operation.
    pub skipped: u64,
    /// The first key left when a limit ended the scan.
    pub resume: Option<i64>,
}

impl FindPage {
    /// Counts candidate `key` as examined. False at the query's limit: the
    /// page then ends before it.
    pub fn examine(&mut self, key: i64, query: &Query) -> bool {
        if self.scanned >= query.max_scan {
            self.resume = Some(key);
            return false;
        }
        self.scanned += 1;
        true
    }

    /// Passes over `key`, which holds no candidate. False once one call has
    /// passed over as many as it may: the page then ends before it.
    pub fn skip(&mut self, key: i64) -> bool {
        if self.skipped >= MAX_SCAN * SKIPS_PER_OP {
            self.resume = Some(key);
            return false;
        }
        self.skipped += 1;
        true
    }
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

/// Largest notification `send_notify` sends (RFC 15.4); a longer one is
/// `Refused`.
pub const MAX_NOTIFY_BYTES: usize = 256;
/// Notifications one module sends per frame (RFC 15.4); past that
/// `send_notify` answers `Refused`.
pub const MAX_NOTIFIES_PER_FRAME: u32 = 256;
/// Largest client message (RFC 15.4, `client_send_message`). It always
/// fits a valid delivery area, which holds at least one 120-byte record.
pub const MAX_MESSAGE_BYTES: usize = 96;
/// Messages one player sends one source per frame, and in any
/// [`MESSAGE_WINDOW`] frames (RFC 15.4).
pub const MESSAGES_PER_FRAME: usize = 2;
pub const MESSAGES_PER_WINDOW: usize = 16;
pub const MESSAGE_WINDOW: u32 = 15;

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
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ActionParams {
    pub id: i32,
    pub arg: i32,
    pub arg2: i32,
    pub value: i64,
    pub value2: i64,
    pub cell: i64,
    pub house: i64,
    pub other: i64,
    /// `text` and `extra`: pointer and length into module memory. The
    /// sandbox reads `text` for `action`; `extra` is not used yet.
    pub text: i32,
    pub text_len: i32,
    pub extra: i32,
    pub extra_len: i32,
}

impl ActionParams {
    pub const SIZE: usize = 72;

    /// Reads the fixed layout.
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
            text: i32_at(56),
            text_len: i32_at(60),
            extra: i32_at(64),
            extra_len: i32_at(68),
        })
    }
}

/// `Order` (RFC appendix "Fixed layouts"): a 24-byte header, then `count`
/// unit handles. Input to `order` (RFC 6g).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// `Command` (appendix 8.3).
    pub kind: i32,
    /// Object or cell reference, 0 for none.
    pub target: i64,
    /// Type reference where the command table lists one, otherwise 0.
    pub value: i64,
    pub units: Vec<i64>,
}

impl Order {
    pub const HEADER: usize = 24;
    /// Unit entries per order (open question 7).
    pub const MAX_UNITS: usize = 1024;

    /// Reads the layout. Bytes that do not hold exactly the header and its
    /// unit entries are an invalid argument; the command table is the
    /// host's to check.
    pub fn parse(bytes: &[u8]) -> Result<Self, Invalid> {
        let invalid = |why: String| Err(Invalid(format!("Order: {why}")));
        let Some(header) = bytes.get(..Self::HEADER) else {
            return invalid(format!("{} bytes, header is {}", bytes.len(), Self::HEADER));
        };
        let count = le_i32(header, 4);
        let Some(count) = usize::try_from(count)
            .ok()
            .filter(|n| *n <= Self::MAX_UNITS)
        else {
            return invalid(format!("{count} units"));
        };
        if bytes.len() != Self::HEADER + count * 8 {
            return invalid(format!("{} bytes for {count} units", bytes.len()));
        }
        Ok(Self {
            kind: le_i32(header, 0),
            target: le_i64(header, 8),
            value: le_i64(header, 16),
            units: bytes[Self::HEADER..]
                .chunks_exact(8)
                .map(|unit| le_i64(unit, 0))
                .collect(),
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
    /// Adds a modifier (appendix 7.2, 7.3) and returns its handle; `op` is a
    /// `ModifierOp` (0..=2), `prop` a [`PROPERTIES`] ID, both checked before
    /// the call.
    #[allow(clippy::too_many_arguments)]
    fn modify(
        &mut self,
        _reference: i64,
        _prop: i32,
        _op: i32,
        _value: i64,
        _frames: i32,
        _source: i64,
        _group: i32,
    ) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }
    /// Removes a modifier early; `Gone` once it expired or was removed.
    fn unmodify(&mut self, _modifier: i64) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
    fn var_get(&mut self, scope: i64, var: i32) -> Result<(i64, Status), Invalid>;
    fn var_set(&mut self, scope: i64, var: i32, value: i64) -> Result<Status, Invalid>;
    /// Runs action `kind` with `params.text` read as `text`; returns the
    /// created object or 0, and the status.
    fn action(
        &mut self,
        kind: i32,
        target: i64,
        params: &ActionParams,
        text: &str,
    ) -> Result<(i64, Status), Invalid>;
    /// Creates a trigger (`trigger_create`); returns its handle or 0, and
    /// the status. A host without triggers answers `Unsupported`.
    fn trigger_create(&mut self, _def: &TriggerDef) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }
    fn trigger_remove(&mut self, _trigger: i64) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
    fn trigger_bind(&mut self, _trigger: i64, _reference: i64) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
    fn trigger_unbind(&mut self, _trigger: i64, _reference: i64) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
    /// The raw text of `[section] key` in the merged rules, `None` for a
    /// missing key, with the call's status.
    fn rules_get(
        &mut self,
        _section: &str,
        _key: &str,
    ) -> Result<(Option<String>, Status), Invalid> {
        Ok((None, Status::Unsupported))
    }
    /// Writes `[section] key=value` as INI text (RFC 8).
    fn rules_set(&mut self, _section: &str, _key: &str, _value: &str) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
    /// Sends notification `id` (declared in the module's `vera.notify`,
    /// which the sandbox checks, as is the size limit) with `data` to the
    /// clients of house `to`'s players, or to the observers when `to` is 0
    /// (RFC 0001 section 10). `Refused` past the per-frame count.
    fn send_notify(&mut self, _to: i64, _id: i32, _data: &[u8]) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
    /// Queues `order` for computer house `house` (RFC 6g).
    fn order(&mut self, _house: i64, _order: &Order) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
    /// Registers an INI name (`ini_register`, only from `vera_init`, which
    /// the sandbox checks along with the name's form): returns the event
    /// ID for an event, else 0, and the status.
    fn ini_register(
        &mut self,
        _kind: i32,
        _section: i32,
        _name: &str,
        _key: i32,
    ) -> Result<(i32, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }
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

    /// Memory a "mod AI" seat adds to the mod module (RFC 0001 15.4).
    pub const SEAT_MEMORY: usize = 8 << 20;

    /// These limits with `seats` "mod AI" seats added.
    pub const fn with_seats(self, seats: usize) -> Self {
        Self {
            memory_bytes: self.memory_bytes + seats * Self::SEAT_MEMORY,
        }
    }
}

/// Why a module could not be loaded. It never runs; the match should not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The module breaks a load rule (RFC section 3).
    Invalid(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(why) => write!(f, "invalid module: {why}"),
        }
    }
}

/// The lifecycle exports this engine calls (RFC appendix 1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    Init,
    ScenarioStart,
    Frame,
    /// `on_rules`, at the module's rules layer (RFC section 8).
    Rules,
    /// `on_events(ptr, count)`, called through `SimModule::deliver`.
    Events,
    /// `on_decide(ptr, count)`, called through `SimModule::decide`. Only the
    /// imports appendix 1.1 marks `decide` are available; a write stops
    /// the module.
    Decide,
    /// `on_message(sender: i64, ptr, len)`, called through
    /// `SimModule::deliver_message` (RFC 0001 section 10, appendix 1.2).
    Message,
}

impl Export {
    /// The exports of signature `() -> ()`, which `SimModule::call` takes.
    pub const ALL: [Self; 4] = [Self::Init, Self::ScenarioStart, Self::Frame, Self::Rules];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Init => "vera_init",
            Self::ScenarioStart => "on_scenario_start",
            Self::Frame => "on_frame",
            Self::Rules => "on_rules",
            Self::Events => "on_events",
            Self::Decide => "on_decide",
            Self::Message => "on_message",
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
    /// More event records waited than the backlog limit allows: the module
    /// cannot keep up (RFC section 5, "Budgets").
    Backlog(usize),
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backlog(records) => {
                write!(f, "cannot keep up: {records} event records are waiting")
            }
            Self::Budget => f.write_str("exceeded its operation budget"),
            Self::Trap(why) => write!(f, "trapped: {why}"),
            Self::Invalid(why) => write!(f, "made an invalid call: {why}"),
        }
    }
}

#[cfg(test)]
mod sandbox_tests;

#[cfg(test)]
mod sdk_props_tests;

#[cfg(test)]
mod trigger_tests;

#[cfg(test)]
mod decide_tests;

#[cfg(test)]
mod rewrite_tests;

#[cfg(test)]
mod client_tests;

#[cfg(test)]
mod message_tests;
