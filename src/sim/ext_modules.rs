//! Wasm extension modules in the simulation ([RFC 0001](../../docs/rfcs/0001-wasm-extension-modules.md)).
//!
//! Minimal first step. A loaded `rules.wasm` runs in the module phase, which
//! sits at the end of the Triggers step (open question 8), mod first, then
//! map:
//! - `on_scenario_start` once on its first phase;
//! - `on_frame` every frame.
//!
//! Each call gets an operation budget. A fault disables only that module, on
//! every machine in the same frame. The host offers:
//! - Red Alert 2 local and global variables, through the trigger runtime's
//!   own writer;
//! - the module's own random stream;
//! - the log;
//! - names of TechnoTypes, countries and properties (`lookup`, `name_of`);
//! - reads of the object and house properties [`Host::get`] lists, and
//!   `find` over objects and houses;
//! - the atomic `AddMoney` action.
//!
//! Modules come from the selected mod's folder and the map's
//! `[VeraWasmRulesPack]` ([`Simulation::install_scenario_ext_modules`]).
//! Saves carry each module's bytes, memory pages that are not all zero,
//! mutable globals, random stream and status. A load puts them back
//! ([`Simulation::restore_ext_modules`]) without running module code. The
//! presentation copy carries no modules (RFC 15.1, item 8).
//!
//! Residuals before this leaves preview:
//! - The state hash and the multiplayer frame checksum read all of a
//!   module's memory each time, not the RFC's 1/15 page rotation.
//! - The in-game notice of a stopped module is English only.
//! - House references use a provisional encoding ([`house_ref`]); the RFC
//!   puts houses on the objects' creation counter.
//! - TechnoType IDs are the rules' registry handles (`TypeHandle`), found by
//!   name; a type whose ID two categories share reads as the first.
//! - Every property is read-only for now: `set` refuses the read-only ones
//!   and answers `Unsupported` for `Health` and `Owner`, whose writes need
//!   an owner path without kill credit (`Health`) or the `SetOwner` action.

use std::hash::{Hash, Hasher};

use vera20k_wasm_ext::{
    ActionParams, Area, ClauseSubject, Export, Fault, FindPage, Invalid, Limits, ModuleState,
    NAME_COUNTRY, NAME_PROPERTY, NAME_TECHNO_TYPE, Query, SCOPE_HOUSES, SCOPE_OBJECTS, SimHost,
    SimModule, Status, TABLE_ACTION, TABLE_NAME_KIND, TABLE_PROPERTY, TABLE_SCOPE, property_name,
};

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::{CountryIdx, RuleSet};
use crate::sim::game_entity::GameEntity;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;

/// Operations per `on_frame` call (RFC 15.2 proposes about 2 ms on the
/// slowest platform; open question 7).
pub const FRAME_BUDGET: u64 = 200_000;
/// Operations for each one-time call: `vera_init` and `on_scenario_start`.
pub const ONCE_BUDGET: u64 = 5_000_000;

/// Variable IDs (RFC appendix "IDs"): Red Alert 2 locals 0..99, globals 100..149.
const LOCAL_VARS: std::ops::Range<i32> = 0..100;
const GLOBAL_VARS: std::ops::Range<i32> = 100..150;

/// Atomic action `AddMoney`: atomic actions are numbered from 1000 in
/// appendix 5.3 table order, where it is entry 15 (numbers are open question 5).
pub const ACTION_ADD_MONEY: i32 = 1015;

/// Provisional house reference: kind 0 with bit 47 set over the house array
/// index. Houses here have no creation serial; the RFC's shared counter is a
/// later step.
const HOUSE_REF_BIT: i64 = 1 << 47;

pub const fn house_ref(index: usize) -> i64 {
    HOUSE_REF_BIT | index as i64
}

/// Where a module came from (RFC section 2); mod modules run first.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ModuleSource {
    Mod,
    Map,
}

struct Slot {
    source: ModuleSource,
    module: SimModule,
    rng: SimRng,
    started: bool,
    /// Why the module stopped; it is never called again.
    disabled: Option<String>,
    /// Operations used by the latest `on_frame`.
    last_frame_ops: u64,
}

impl ModuleSource {
    const fn limits(self) -> Limits {
        match self {
            Self::Mod => Limits::MOD,
            Self::Map => Limits::MAP,
        }
    }
}

/// One module as a save holds it.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedSlot {
    source: ModuleSource,
    bytes: Vec<u8>,
    state: ModuleState,
    rng: SimRng,
    started: bool,
    disabled: Option<String>,
    last_frame_ops: u64,
}

/// The loaded simulation modules, in run order.
#[derive(Default)]
pub struct ExtModules {
    slots: Vec<Slot>,
    /// Modules read from a save, instantiated by
    /// [`Simulation::restore_ext_modules`]. Deserializing never compiles a
    /// module, so describing the save layout stays data-only.
    pending: Vec<SavedSlot>,
    /// Log lines from modules, drained by the caller.
    log: Vec<String>,
}

impl serde::Serialize for ExtModules {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.slots.len()))?;
        for slot in &self.slots {
            seq.serialize_element(&SavedSlot {
                source: slot.source,
                bytes: slot.module.bytes().to_vec(),
                state: slot.module.state(),
                rng: slot.rng.clone(),
                started: slot.started,
                disabled: slot.disabled.clone(),
                last_frame_ops: slot.last_frame_ops,
            })?;
        }
        seq.end()
    }
}

impl<'de> serde::Deserialize<'de> for ExtModules {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            pending: Vec::deserialize(deserializer)?,
            ..Self::default()
        })
    }
}

/// The presentation copy excludes module state (RFC 15.1, item 8).
impl Clone for ExtModules {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl ExtModules {
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Each loaded module's source and, once it stopped, why.
    pub fn status(&self) -> Vec<(ModuleSource, Option<String>)> {
        self.slots
            .iter()
            .map(|s| (s.source, s.disabled.clone()))
            .collect()
    }

    /// Operations used by each module's latest `on_frame`.
    pub fn last_frame_ops(&self) -> Vec<u64> {
        self.slots.iter().map(|s| s.last_frame_ops).collect()
    }

    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    /// One 32-bit FNV-1a digest per module over its memory and globals, for
    /// the multiplayer frame checksum. Byte-wise, so 32- and 64-bit
    /// platforms agree. Residual: it reads all memory each frame instead of
    /// the RFC's 1/15 page rotation.
    pub(crate) fn checksum_digests(&self) -> impl Iterator<Item = u32> + '_ {
        self.slots.iter().map(|slot| {
            let mut digest = 0x811C_9DC5u32;
            let globals = slot.module.globals().iter().flat_map(|g| g.to_le_bytes());
            for byte in globals.chain(slot.module.memory().iter().copied()) {
                digest = (digest ^ u32::from(byte)).wrapping_mul(0x0100_0193);
            }
            digest
        })
    }

    pub fn hash_state(&self, hasher: &mut impl Hasher) {
        b"ext-modules-v1".hash(hasher);
        for slot in &self.slots {
            slot.source.hash(hasher);
            slot.started.hash(hasher);
            slot.disabled.is_some().hash(hasher);
            slot.rng.hash_state(hasher);
            slot.module.globals().hash(hasher);
            slot.module.memory().hash(hasher);
        }
    }
}

/// Reference kinds (appendix "References"): bits 48..52 of a reference.
const REF_KIND_SHIFT: u32 = 48;
const REF_TYPE: i64 = 1;
const REF_CELL: i64 = 2;
/// References are below 2^53.
const REF_LIMIT: i64 = 1 << 53;

/// A type reference: name kind and ID.
const fn type_ref(kind: i32, id: u32) -> i64 {
    (REF_TYPE << REF_KIND_SHIFT) | ((kind as i64) << 32) | id as i64
}

/// A cell reference, in cells.
const fn cell_ref(x: u16, y: u16) -> i64 {
    (REF_CELL << REF_KIND_SHIFT) | ((x as i64) << 16) | y as i64
}

/// What a nonzero reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ref {
    Object(u64),
    House(usize),
    Type {
        kind: i32,
        id: u32,
    },
    Cell {
        x: u16,
        y: u16,
    },
    /// A waypoint or the global rules reference: valid, not read yet.
    Other,
}

/// Decodes a reference; `None` for 0. Client kinds (5, 6) and unknown kinds
/// are invalid in the simulation.
fn decode_ref(reference: i64) -> Result<Option<Ref>, Invalid> {
    if reference == 0 {
        return Ok(None);
    }
    if !(0..REF_LIMIT).contains(&reference) {
        return Err(Invalid(format!("{reference:#x} is not a reference")));
    }
    let payload = reference & ((1 << REF_KIND_SHIFT) - 1);
    Ok(Some(match reference >> REF_KIND_SHIFT {
        0 if reference & HOUSE_REF_BIT != 0 => Ref::House((payload & !HOUSE_REF_BIT) as usize),
        0 => Ref::Object(payload as u64),
        REF_TYPE => Ref::Type {
            kind: (payload >> 32) as i32,
            id: payload as u32,
        },
        REF_CELL if payload >> 32 == 0 => Ref::Cell {
            x: (payload >> 16) as u16,
            y: payload as u16,
        },
        3 | 4 => Ref::Other,
        _ => {
            return Err(Invalid(format!(
                "{reference:#x} is not a simulation reference"
            )));
        }
    }))
}

/// Object categories (appendix 8.3 `Category`).
const fn category_value(category: EntityCategory) -> i64 {
    match category {
        EntityCategory::Infantry => 0,
        EntityCategory::Unit => 1,
        EntityCategory::Aircraft => 2,
        EntityCategory::Structure => 3,
    }
}

/// The host a module sees during one call: the simulation, whose module
/// slots are set aside for the call, and the calling module's own state.
struct Host<'a> {
    sim: &'a mut Simulation,
    rules: &'a RuleSet,
    rng: &'a mut SimRng,
    source: ModuleSource,
}

impl SimHost for Host<'_> {
    fn frame(&mut self) -> i32 {
        self.sim.session.binary_frame() as i32
    }

    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        self.rng.next_range_i32_inclusive(lo, hi)
    }

    fn log(&mut self, line: &str) {
        log::info!(target: "ext_module", "{:?}: {line}", self.source);
        self.sim.ext_modules.log.push(line.to_owned());
    }

    fn has_kind(&mut self, table: i32, id: i32) -> bool {
        match table {
            TABLE_ACTION => id == ACTION_ADD_MONEY,
            TABLE_PROPERTY => property_name(id).is_some_and(|name| READABLE.contains(&name)),
            TABLE_NAME_KIND => matches!(id, NAME_TECHNO_TYPE | NAME_COUNTRY | NAME_PROPERTY),
            TABLE_SCOPE => matches!(id, SCOPE_OBJECTS | SCOPE_HOUSES),
            _ => false,
        }
    }

    fn lookup(&mut self, kind: i32, name: &str) -> Result<(i32, Status), Invalid> {
        let id = match kind {
            NAME_TECHNO_TYPE => self.rules.type_handle(name).map(|h| h.0 as i32),
            NAME_COUNTRY => self.rules.country_index(name).map(|c| i32::from(c.0)),
            _ => return Ok((-1, Status::Unsupported)),
        };
        Ok(id.map_or((-1, Status::NotFound), |id| (id, Status::Ok)))
    }

    fn name_of(&mut self, kind: i32, id: i32) -> Result<(String, Status), Invalid> {
        let name = match kind {
            NAME_TECHNO_TYPE => self.techno_type(id).map(|object| object.id.as_str()),
            NAME_COUNTRY => u16::try_from(id)
                .ok()
                .and_then(|id| self.rules.country_name(CountryIdx(id))),
            _ => return Ok((String::new(), Status::Unsupported)),
        };
        name.map(|name| (name.to_owned(), Status::Ok))
            .ok_or_else(|| Invalid(format!("name kind {kind} has no ID {id}")))
    }

    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        self.read(reference, prop)
    }

    fn set(&mut self, reference: i64, prop: i32, _value: i64) -> Result<Status, Invalid> {
        let name = property_name(prop).unwrap_or_default();
        let (_, status) = self.read(reference, prop)?;
        Ok(match status {
            // Health and Owner are writable in the RFC; their owner paths
            // are not wired yet (module residuals).
            Status::Ok if matches!(name, "Health" | "Owner") => Status::Unsupported,
            Status::Ok => Status::Refused,
            other => other,
        })
    }

    fn find(&mut self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        match query.scope {
            SCOPE_OBJECTS => self.find_objects(query, cap),
            SCOPE_HOUSES => self.find_houses(query, cap),
            _ => Ok((FindPage::default(), Status::Unsupported)),
        }
    }

    fn var_get(&mut self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        self.read_var(scope, var)
    }

    fn var_set(&mut self, scope: i64, var: i32, value: i64) -> Result<Status, Invalid> {
        let (global, index) = stock_variable(scope, var)?;
        // The trigger runtime's writer also resets the timers of triggers
        // waiting on the variable (events 27, 28, 36, 37), as map actions do.
        let sim = &mut *self.sim;
        Simulation::set_trigger_variable(
            &sim.session,
            &mut sim.scenario_rng,
            &mut sim.trigger_runtime,
            global,
            index as i32,
            value != 0,
        );
        Ok(Status::Ok)
    }

    fn action(
        &mut self,
        kind: i32,
        target: i64,
        params: &ActionParams,
    ) -> Result<(i64, Status), Invalid> {
        match kind {
            ACTION_ADD_MONEY => {
                let Some(owner) = self.house(target)? else {
                    return Ok((0, Status::Gone));
                };
                let amount = i32::try_from(params.value).map_err(|_| {
                    Invalid(format!("AddMoney amount {} is out of range", params.value))
                })?;
                crate::sim::credit_income::add_credits(&mut self.sim.houses, owner, amount);
                Ok((0, Status::Ok))
            }
            _ => Ok((0, Status::Unsupported)),
        }
    }
}

/// The properties [`Host::get`] reads (others answer `Unsupported`).
const READABLE: &[&str] = &[
    "Type",
    "Owner",
    "Category",
    "Health",
    "Strength",
    "HealthPercent",
    "X",
    "Y",
    "Z",
    "Cell",
    "Index",
    "Money",
    "PowerOutput",
    "PowerDrain",
    "LowPower",
    "IsDefeated",
    "IsHuman",
];

impl Host<'_> {
    /// A house reference's house; `None` for a house that does not exist.
    fn house(&self, reference: i64) -> Result<Option<crate::sim::intern::InternedId>, Invalid> {
        if reference & HOUSE_REF_BIT == 0 || reference >> 48 != 0 {
            return Err(Invalid(format!("{reference:#x} is not a house reference")));
        }
        let index = (reference & !HOUSE_REF_BIT) as usize;
        Ok(self.sim.houses.order().get(index).copied())
    }

    /// A property, as `get` reads it.
    fn read(&self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        let name = property_name(prop).unwrap_or_default();
        let Some(reference) = decode_ref(reference)? else {
            return Ok((0, Status::Gone));
        };
        Ok(match reference {
            Ref::Object(id) => match self.sim.entities().get(id) {
                Some(entity) => self.object_property(entity, name),
                None => (0, Status::Gone),
            },
            Ref::House(index) => self.house_property(index, name),
            Ref::Type { kind, id } => self.type_property(kind, id, name)?,
            Ref::Cell { .. } | Ref::Other => (0, Status::Unsupported),
        })
    }

    /// A variable, as `var_get` reads it.
    fn read_var(&self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        let (global, index) = stock_variable(scope, var)?;
        let runtime = &self.sim.trigger_runtime;
        let set = if global {
            runtime.global_variable_is_set(index)
        } else {
            runtime.local_variable_is_set(index)
        };
        Ok((i64::from(set), Status::Ok))
    }

    /// TechnoType `id`: an index into the rules' type registry.
    fn techno_type(&self, id: i32) -> Option<&crate::rules::object_type::ObjectType> {
        let id = usize::try_from(id).ok()?;
        self.rules.all_objects().nth(id)
    }

    /// An object's property (appendix 7.1).
    fn object_property(&self, entity: &GameEntity, name: &str) -> (i64, Status) {
        let object = || self.sim.entity_object_type(entity, self.rules);
        let value = match name {
            "Type" => match object().and_then(|o| self.rules.type_handle(&o.id)) {
                Some(handle) => type_ref(NAME_TECHNO_TYPE, handle.0),
                None => return (0, Status::NotFound),
            },
            "Owner" => match self.sim.houses.index_of(entity.owner()) {
                Some(index) => house_ref(index),
                None => return (0, Status::Gone),
            },
            "Category" => category_value(entity.category()),
            "Health" => i64::from(entity.health.current()),
            "Strength" | "HealthPercent" => {
                let Some(strength) = object().map(|o| o.strength) else {
                    return (0, Status::NotFound);
                };
                if name == "Strength" {
                    i64::from(strength)
                } else if strength > 0 {
                    i64::from(entity.health.current()) * 100 / i64::from(strength)
                } else {
                    0
                }
            }
            "X" | "Y" => {
                let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
                i64::from(if name == "X" { x } else { y })
            }
            "Z" => i64::from(crate::sim::movement::ground_pose::object_world_z_leptons(
                entity,
                self.sim.resolved_terrain(),
            )),
            "Cell" => cell_ref(entity.position.rx(), entity.position.ry()),
            _ => return (0, Status::Unsupported),
        };
        (value, Status::Ok)
    }

    /// A house's property (appendix 7.5).
    fn house_property(&self, index: usize, name: &str) -> (i64, Status) {
        let Some(id) = self.sim.houses.order().get(index).copied() else {
            return (0, Status::Gone);
        };
        let Some(house) = self.sim.houses.get(&id) else {
            return (0, Status::Gone);
        };
        let power = self.sim.power_states.get(&id);
        let (output, drain) = power.map_or((0, 0), |p| p.effective_totals());
        let value = match name {
            "Index" => index as i64,
            "Money" => i64::from(house.economy().credits()),
            "PowerOutput" => i64::from(output),
            "PowerDrain" => i64::from(drain),
            "LowPower" => i64::from(power.is_some_and(|p| p.low_power())),
            "IsDefeated" => i64::from(house.is_defeated()),
            "IsHuman" => i64::from(house.is_human()),
            _ => return (0, Status::Unsupported),
        };
        (value, Status::Ok)
    }

    /// A type's property (appendix 7.7). A wildcard or unknown ID is invalid.
    fn type_property(&self, kind: i32, id: u32, name: &str) -> Result<(i64, Status), Invalid> {
        if kind != NAME_TECHNO_TYPE {
            return Ok((0, Status::Unsupported));
        }
        let object = i32::try_from(id)
            .ok()
            .and_then(|id| self.techno_type(id))
            .ok_or_else(|| Invalid(format!("no TechnoType {id}")))?;
        Ok(match name {
            "Strength" => (i64::from(object.strength), Status::Ok),
            _ => (0, Status::Unsupported),
        })
    }

    /// Whether `candidate` passes every clause; `None` when a clause needs
    /// what is not kept yet. Clauses read like `get`; a reading that is not
    /// `Ok` fails its clause.
    fn passes(&self, candidate: i64, query: &Query) -> Result<Option<bool>, Invalid> {
        for clause in &query.clauses {
            let (reading, status) = match clause.subject {
                ClauseSubject::Property(prop) => self.read(candidate, prop)?,
                ClauseSubject::Var {
                    var,
                    subject: false,
                } => self.read_var(0, var)?,
                // Object variables (`vera.vars` scope 2) are not kept yet.
                ClauseSubject::Var { subject: true, .. } => return Ok(None),
            };
            if status != Status::Ok || !clause.op.holds(reading, clause.value) {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    }

    /// `find` over objects, in creation (stable ID) order.
    fn find_objects(&self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        let type_filter = match decode_ref(query.type_ref)? {
            None => None,
            Some(Ref::Type {
                kind: NAME_TECHNO_TYPE,
                id,
            }) if i32::try_from(id).is_ok_and(|id| self.techno_type(id).is_some()) => {
                Some(type_ref(NAME_TECHNO_TYPE, id))
            }
            Some(_) => {
                return Err(Invalid(format!(
                    "{:#x} is not a TechnoType for an Objects query",
                    query.type_ref
                )));
            }
        };
        let owner = match query.owner {
            0 => None,
            owner => match self.house(owner)? {
                Some(owner) => Some(owner),
                None => return Ok((FindPage::default(), Status::Gone)),
            },
        };
        let centre = match query.area {
            Area::Map => None,
            Area::Circle { cell, .. } | Area::Rect { cell, .. } => match decode_ref(cell)? {
                Some(Ref::Cell { x, y }) => Some((x, y)),
                _ => return Err(Invalid(format!("{cell:#x} is not a cell reference"))),
            },
        };
        let after = u64::try_from(query.after).unwrap_or(0);
        let mut page = FindPage::default();
        for (id, entity) in self.sim.entities().iter_sorted() {
            if id < after {
                continue;
            }
            if page.found.len() == cap {
                break;
            }
            page.scanned += 1;
            if owner.is_some_and(|owner| entity.owner() != owner)
                || centre.is_some_and(|(cx, cy)| !in_area(entity, query.area, cx, cy))
                || type_filter.is_some_and(|t| self.object_property(entity, "Type").0 != t)
            {
                continue;
            }
            let reference = id as i64;
            match self.passes(reference, query)? {
                Some(true) => page.found.push((reference, reference)),
                Some(false) => {}
                None => return Ok((FindPage::default(), Status::Unsupported)),
            }
        }
        Ok((page, Status::Ok))
    }

    /// `find` over houses, in house array order. Houses have no place or
    /// type: a query with an area, type or owner answers `Unsupported`.
    fn find_houses(&self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        if query.area != Area::Map || query.type_ref != 0 || query.owner != 0 {
            return Ok((FindPage::default(), Status::Unsupported));
        }
        let mut page = FindPage::default();
        let start = usize::try_from(query.after).unwrap_or(usize::MAX);
        for index in start..self.sim.houses.order().len() {
            if page.found.len() == cap {
                break;
            }
            page.scanned += 1;
            let reference = house_ref(index);
            match self.passes(reference, query)? {
                Some(true) => page.found.push((index as i64, reference)),
                Some(false) => {}
                None => return Ok((FindPage::default(), Status::Unsupported)),
            }
        }
        Ok((page, Status::Ok))
    }
}

/// Whether an object stands in a query area around cell (`cx`, `cy`):
/// within `radius` leptons of the cell's center, or in the rectangle of
/// cells from it.
fn in_area(entity: &GameEntity, area: Area, cx: u16, cy: u16) -> bool {
    match area {
        Area::Map => true,
        Area::Circle { radius, .. } => {
            let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
            let dx = i64::from(x) - (i64::from(cx) * 256 + 128);
            let dy = i64::from(y) - (i64::from(cy) * 256 + 128);
            dx * dx + dy * dy <= i64::from(radius) * i64::from(radius)
        }
        Area::Rect { width, height, .. } => {
            let (rx, ry) = (
                i64::from(entity.position.rx()),
                i64::from(entity.position.ry()),
            );
            let (cx, cy) = (i64::from(cx), i64::from(cy));
            (cx..cx + i64::from(width)).contains(&rx) && (cy..cy + i64::from(height)).contains(&ry)
        }
    }
}

/// A Red Alert 2 variable ID: global scope only, IDs 0..99 local, 100..149 global.
fn stock_variable(scope: i64, var: i32) -> Result<(bool, u32), Invalid> {
    if scope != 0 {
        return Err(Invalid(format!(
            "variable {var} has global scope, not {scope:#x}"
        )));
    }
    if LOCAL_VARS.contains(&var) {
        Ok((false, var as u32))
    } else if GLOBAL_VARS.contains(&var) {
        Ok((true, (var - GLOBAL_VARS.start) as u32))
    } else {
        Err(Invalid(format!("unknown variable {var}")))
    }
}

impl Simulation {
    /// Loads a module and runs its `vera_init`. An error means the module
    /// cannot run and the match should not start (RFC section 3).
    pub fn install_ext_module(
        &mut self,
        rules: &RuleSet,
        source: ModuleSource,
        bytes: &[u8],
    ) -> Result<(), String> {
        let module = SimModule::load(bytes, source.limits()).map_err(|e| e.to_string())?;
        // The module's own stream (RFC section 4): from the scenario seed and
        // the slot, so module draws never move a native stream.
        let rng = SimRng::new(self.session.seed() ^ (0x5645_5241_0000_0000 | source as u64));
        let position = self
            .ext_modules
            .slots
            .partition_point(|s| s.source <= source);
        self.ext_modules.slots.insert(
            position,
            Slot {
                source,
                module,
                rng,
                started: false,
                disabled: None,
                last_frame_ops: 0,
            },
        );
        match self.call_ext_module(rules, position, Export::Init, ONCE_BUDGET) {
            Ok(_) => Ok(()),
            Err(fault) => {
                self.ext_modules.slots.remove(position);
                Err(format!("vera_init {fault}"))
            }
        }
    }

    /// Loads a scenario's modules (RFC section 2): the selected mod's
    /// `rules.wasm`, read by the caller, then the one the map embeds. An
    /// error names the module; the match must not start.
    pub fn install_scenario_ext_modules(
        &mut self,
        rules: &RuleSet,
        mod_module: Option<&[u8]>,
        map: &crate::rules::ini_parser::IniFile,
    ) -> Result<(), String> {
        if let Some(bytes) = mod_module {
            self.install_ext_module(rules, ModuleSource::Mod, bytes)
                .map_err(|e| format!("the mod's rules.wasm: {e}"))?;
        }
        let embedded = crate::map::ext_module_pack::rules_module(map)
            .map_err(|e| format!("the map's rules module: {e}"))?;
        if let Some(bytes) = embedded {
            self.install_ext_module(rules, ModuleSource::Map, &bytes)
                .map_err(|e| format!("the map's rules module: {e}"))?;
        }
        Ok(())
    }

    /// Instantiates the modules a save carried and puts back their state.
    /// An error means this game cannot continue the save.
    pub(crate) fn restore_ext_modules(&mut self) -> Result<(), String> {
        for saved in std::mem::take(&mut self.ext_modules.pending) {
            let module = SimModule::restore(&saved.bytes, saved.source.limits(), &saved.state)
                .map_err(|e| format!("{:?} module: {e}", saved.source))?;
            self.ext_modules.slots.push(Slot {
                source: saved.source,
                module,
                rng: saved.rng,
                started: saved.started,
                disabled: saved.disabled,
                last_frame_ops: saved.last_frame_ops,
            });
        }
        Ok(())
    }

    pub fn ext_modules(&self) -> &ExtModules {
        &self.ext_modules
    }

    pub fn ext_modules_mut(&mut self) -> &mut ExtModules {
        &mut self.ext_modules
    }

    /// The module phase: each enabled module in order gets its
    /// `on_scenario_start` once, then `on_frame`.
    pub(crate) fn run_ext_module_phase(&mut self, rules: &RuleSet) {
        for index in 0..self.ext_modules.slots.len() {
            if self.ext_modules.slots[index].disabled.is_some() {
                continue;
            }
            if !self.ext_modules.slots[index].started {
                self.ext_modules.slots[index].started = true;
                if let Err(fault) =
                    self.call_ext_module(rules, index, Export::ScenarioStart, ONCE_BUDGET)
                {
                    self.disable_ext_module(index, Export::ScenarioStart, fault);
                    continue;
                }
            }
            match self.call_ext_module(rules, index, Export::Frame, FRAME_BUDGET) {
                Ok(ops) => self.ext_modules.slots[index].last_frame_ops = ops,
                Err(fault) => self.disable_ext_module(index, Export::Frame, fault),
            }
        }
    }

    /// Calls one module. Its slot list is set aside for the call, so the
    /// host reaches the whole simulation; nothing in a call reads modules.
    fn call_ext_module(
        &mut self,
        rules: &RuleSet,
        index: usize,
        export: Export,
        budget: u64,
    ) -> Result<u64, Fault> {
        let mut slots = std::mem::take(&mut self.ext_modules.slots);
        let slot = &mut slots[index];
        let mut host = Host {
            sim: self,
            rules,
            rng: &mut slot.rng,
            source: slot.source,
        };
        let result = slot.module.call(export, budget, &mut host);
        self.ext_modules.slots = slots;
        result
    }

    /// Stops a module for good. Its completed effects stay (RFC section 3).
    fn disable_ext_module(&mut self, index: usize, export: Export, fault: Fault) {
        let slot = &mut self.ext_modules.slots[index];
        let reason = format!(
            "{:?} module stopped in {}: {fault}",
            slot.source,
            export.name()
        );
        log::warn!(target: "ext_module", "{reason}");
        self.ext_modules.log.push(reason.clone());
        slot.disabled = Some(reason.clone());
        self.publish_trigger_effects([
            crate::sim::trigger_runtime::TriggerEffect::ExtModuleStopped { reason },
        ]);
    }
}

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_tests.rs"]
mod tests;
