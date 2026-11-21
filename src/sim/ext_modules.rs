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
//! - modifiers on objects ([`modifiers`]) and the variables modules declare
//!   in `vera.vars` ([`vars`]);
//! - writes of `Health`, `Owner`, `Rank`, `Mission`, `Ammo` and `Online`
//!   through their owners ([`writes`]);
//! - runtime triggers and `on_events` delivery ([`triggers`]);
//! - names of weapons, warheads, TeamTypes and action kinds;
//! - the `action` import ([`actions`]): atomic actions and stock trigger
//!   actions through their owners, for direct calls and trigger actions;
//! - cell and global properties and `find` over cells ([`cells`]);
//! - house orders for computer houses ([`orders`]);
//! - `rules_get`, and `rules_set` and `on_rules` while the rules are
//!   processed ([`rules`]).
//!
//! Modules come from the selected mod's folder and the map's
//! `[VeraWasmRulesPack]`, under the mod's `[VeraWasm]` policy
//! ([`PreparedExtModules`]). A module that exports `on_rules` runs
//! `vera_init` and `on_rules` while the rules are processed; the others run
//! `vera_init` when they join the simulation
//! ([`Simulation::install_scenario_ext_modules`]).
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
//! - `set` writes `Health` (1 up to the type's Strength; a write of 0 or
//!   less, which would kill without credit, answers `Refused`), `Owner`
//!   (through ChangeOwner, as the stock change-house action does), `Rank`,
//!   `Mission` (ordered as a player's order), `Ammo` and a building's
//!   `Online` ([`writes`]). Other readable properties refuse.
//! - Only `FirepowerMultiplier`, `ArmorMultiplier` and `SpeedMultiplier`
//!   take modifiers; other modifiable properties answer `Unsupported`, and
//!   houses take none.

use std::hash::{Hash, Hasher};

use vera20k_wasm_ext::{
    ActionParams, Area, ClauseSubject, Export, Fault, FindPage, Invalid, Limits, ModuleState,
    NAME_ACTION_KIND, NAME_COUNTRY, NAME_PROPERTY, NAME_SUPER_WEAPON, NAME_TEAM_TYPE,
    NAME_TECHNO_TYPE, NAME_VARIABLE, NAME_WARHEAD, NAME_WEAPON, Order, Query, SCOPE_CELLS,
    SCOPE_HOUSES, SCOPE_MODIFIERS, SCOPE_OBJECTS, SimHost, SimModule, Status, TABLE_ACTION,
    TABLE_NAME_KIND, TABLE_PROPERTY, TABLE_SCOPE, property_name,
};

mod actions;
mod cells;
pub(crate) mod modifiers;
mod orders;
mod rules;
pub(crate) mod vars;
mod writes;

pub use actions::{ActionContext, run_action};
use modifiers::{Modifier, ModifierOp, ModifierStore, Stat};
pub(crate) use orders::take_due_orders;
pub use rules::{MapUi, PreparedExtModules, WasmPolicy};
use vars::{FIRST_DECLARED_VAR, VarStore, VarValues};

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::{CountryIdx, RuleSet};
use crate::sim::game_entity::GameEntity;
use crate::sim::rng::SimRng;
use crate::sim::world::{FrameEffects, Simulation};

mod triggers;
pub(crate) use triggers::ModuleEvent;

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
pub const ACTION_ADD_MONEY: i32 = actions::ADD_MONEY;

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
    /// Operations per `on_frame` call: [`FRAME_BUDGET`], or the mod's
    /// `[VeraWasm]` budget for this source.
    frame_budget: u64,
    /// Orders waiting for the next command step ([`orders`]).
    orders: Vec<orders::PendingOrder>,
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
    #[serde(default = "default_frame_budget")]
    frame_budget: u64,
    #[serde(default)]
    orders: Vec<orders::PendingOrder>,
}

const fn default_frame_budget() -> u64 {
    FRAME_BUDGET
}

/// The loaded simulation modules, in run order, and the modifiers and
/// variables they own.
#[derive(Default)]
pub struct ExtModules {
    slots: Vec<Slot>,
    modifiers: ModifierStore,
    vars: VarStore,
    /// Modules read from a save, instantiated by
    /// [`Simulation::restore_ext_modules`]. Deserializing never compiles a
    /// module, so describing the save layout stays data-only.
    pending: Vec<SavedSlot>,
    /// Log lines from modules, drained by the caller.
    log: Vec<String>,
    /// Set while a module trigger runs its immediate actions, so the events
    /// they raise reach no module trigger (no chain triggers yet). False
    /// between frames; not saved.
    firing: bool,
    /// The merged rules text `rules_get` reads during the match; `None`
    /// without modules.
    rules_ini: Option<IniFile>,
    /// The mod's `[VeraWasm] MapUI=`, kept for client modules (phase 2).
    /// Not saved: a loaded match reads the default.
    map_ui: MapUi,
}

/// What a save holds: the modules, the modifiers and the variable values
/// (the declarations come back from the module bytes).
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedExtModules {
    slots: Vec<SavedSlot>,
    modifiers: ModifierStore,
    vars: VarValues,
    #[serde(default)]
    rules_ini: Option<IniFile>,
}

impl serde::Serialize for ExtModules {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let slots = self
            .slots
            .iter()
            .map(|slot| SavedSlot {
                source: slot.source,
                bytes: slot.module.bytes().to_vec(),
                state: slot.module.state(),
                rng: slot.rng.clone(),
                started: slot.started,
                disabled: slot.disabled.clone(),
                last_frame_ops: slot.last_frame_ops,
                frame_budget: slot.frame_budget,
                orders: slot.orders.clone(),
            })
            .collect();
        SavedExtModules {
            slots,
            modifiers: self.modifiers.clone(),
            vars: self.vars.values.clone(),
            rules_ini: self.rules_ini.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for ExtModules {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let saved = SavedExtModules::deserialize(deserializer)?;
        let mut modules = Self {
            pending: saved.slots,
            modifiers: saved.modifiers,
            rules_ini: saved.rules_ini,
            ..Self::default()
        };
        modules.vars.values = saved.vars;
        Ok(modules)
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

    /// How map modules may show interface under the selected mod.
    pub fn map_ui(&self) -> MapUi {
        self.map_ui
    }

    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    /// Takes the variable declarations from the loaded modules, in order.
    fn declare_vars(&mut self) {
        self.vars.declare(
            self.slots
                .iter()
                .map(|slot| (slot.source, slot.module.vars())),
        );
    }

    /// The modifiers modules hold.
    pub fn modifiers(&self) -> &ModifierStore {
        &self.modifiers
    }

    /// A declared variable's value for `holder` (`0` global, else a house
    /// or object reference); `None` for an unknown variable, a wrong scope
    /// or a gone holder.
    pub fn declared_var(&self, sim: &Simulation, scope: i64, var: i32) -> Option<i64> {
        let holder = self
            .vars
            .holder(
                scope,
                var,
                |index| index < sim.houses.order().len(),
                |id| sim.entities().get(id).is_some(),
            )
            .ok()??;
        Some(self.vars.get(var, holder))
    }

    /// One 32-bit FNV-1a digest per module over its memory and globals, for
    /// the multiplayer frame checksum, then one over the modifiers and the
    /// declared variables' values when any are held. Byte-wise, in handle and
    /// key order, so 32- and 64-bit platforms agree. Residual: it reads all
    /// memory each frame instead of the RFC's 1/15 page rotation.
    pub(crate) fn checksum_digests(&self) -> impl Iterator<Item = u32> + '_ {
        let modules = self.slots.iter().map(|slot| {
            let globals = slot.module.globals().iter().flat_map(|g| g.to_le_bytes());
            fnv1a(globals.chain(slot.module.memory().iter().copied()))
        });
        let values = &self.vars.values;
        let held = !self.modifiers.is_empty()
            || !values.global.is_empty()
            || !values.house.is_empty()
            || !values.object.is_empty();
        modules.chain(held.then(|| fnv1a(self.held_state_bytes())))
    }

    /// The modifiers and declared-variable values as checksum bytes.
    fn held_state_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        let modifiers = self.modifiers.iter().flat_map(|(handle, m)| {
            let expires = m.expires.map_or(-1, i64::from);
            [handle, m.target]
                .into_iter()
                .flat_map(u64::to_le_bytes)
                .chain([m.stat as u8, m.op as u8, m.origin as u8])
                .chain(
                    [m.value, expires, m.source]
                        .into_iter()
                        .flat_map(i64::to_le_bytes),
                )
                .chain(m.group.to_le_bytes())
        });
        let values = &self.vars.values;
        let global = values
            .global
            .iter()
            .flat_map(|(var, value)| var.to_le_bytes().into_iter().chain(value.to_le_bytes()));
        let house = values.house.iter().flat_map(|((var, house), value)| {
            var.to_le_bytes()
                .into_iter()
                .chain(house.to_le_bytes())
                .chain(value.to_le_bytes())
        });
        let object = values.object.iter().flat_map(|((id, var), value)| {
            id.to_le_bytes()
                .into_iter()
                .chain(var.to_le_bytes())
                .chain(value.to_le_bytes())
        });
        // Section tags keep an entry from reading as one of another kind.
        modifiers
            .chain([1])
            .chain(global)
            .chain([2])
            .chain(house)
            .chain([3])
            .chain(object)
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
            slot.orders.hash(hasher);
        }
        self.modifiers.hash(hasher);
        self.vars.values.hash(hasher);
    }
}

/// 32-bit FNV-1a over `bytes`.
fn fnv1a(bytes: impl Iterator<Item = u8>) -> u32 {
    bytes.fold(0x811C_9DC5u32, |digest, byte| {
        (digest ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// Reference kinds (appendix "References"): bits 48..52 of a reference.
const REF_KIND_SHIFT: u32 = 48;
const REF_TYPE: i64 = 1;
const REF_CELL: i64 = 2;
const REF_WAYPOINT: i64 = 3;
const REF_GLOBAL: i64 = 4;
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
    /// A waypoint number.
    Waypoint(i32),
    /// The global rules reference (kind 4, payload 0).
    Global,
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
        REF_WAYPOINT => match i32::try_from(payload) {
            Ok(number) => Ref::Waypoint(number),
            Err(_) => return Err(Invalid(format!("{reference:#x} is not a waypoint"))),
        },
        REF_GLOBAL if payload == 0 => Ref::Global,
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
struct Host<'a, 'f> {
    sim: &'a mut Simulation,
    rules: &'a RuleSet,
    /// For the owners a write or action reaches (ChangeOwner, ...).
    overlay_registry: Option<&'a OverlayTypeRegistry>,
    frame_effects: &'a mut FrameEffects<'f>,
    rng: &'a mut SimRng,
    source: ModuleSource,
    /// The calling module's queued orders.
    orders: &'a mut Vec<orders::PendingOrder>,
    /// The merged rules text (`rules_get`).
    rules_ini: Option<&'a IniFile>,
}

/// `has_kind`, the same in every call (the rules stage included).
fn has_kind(rules: &RuleSet, table: i32, id: i32) -> bool {
    match table {
        vera20k_wasm_ext::triggers::TABLE_EVENT => triggers::RAISED_EVENTS.contains(&id),
        TABLE_ACTION => actions::has_action(id, rules),
        TABLE_PROPERTY => property_name(id)
            .is_some_and(|name| READABLE.contains(&name) || Stat::from_property(name).is_some()),
        TABLE_NAME_KIND => matches!(
            id,
            NAME_TECHNO_TYPE
                | NAME_WEAPON
                | NAME_WARHEAD
                | NAME_SUPER_WEAPON
                | NAME_COUNTRY
                | NAME_TEAM_TYPE
                | NAME_VARIABLE
                | NAME_PROPERTY
                | NAME_ACTION_KIND
        ),
        TABLE_SCOPE => matches!(
            id,
            SCOPE_OBJECTS | SCOPE_HOUSES | SCOPE_CELLS | SCOPE_MODIFIERS
        ),
        _ => false,
    }
}

/// `lookup` of the name kinds the rules answer; `None` for other kinds.
fn rules_name_id(rules: &RuleSet, kind: i32, name: &str) -> Option<Option<i32>> {
    Some(match kind {
        NAME_TECHNO_TYPE => rules.type_handle(name).map(|h| h.0 as i32),
        NAME_COUNTRY => rules.country_index(name).map(|c| i32::from(c.0)),
        NAME_WEAPON => rules.weapon_index(name).map(|i| i as i32),
        NAME_WARHEAD => rules.warhead_index(name).map(|i| i as i32),
        NAME_SUPER_WEAPON => rules
            .super_weapon_index(name)
            .and_then(|i| i32::try_from(i).ok()),
        _ => return None,
    })
}

/// `name_of` of the name kinds the rules answer; `None` for other kinds.
fn rules_id_name(rules: &RuleSet, kind: i32, id: i32) -> Option<Option<&str>> {
    Some(match kind {
        NAME_TECHNO_TYPE => techno_type(rules, id).map(|object| object.id.as_str()),
        NAME_COUNTRY => u16::try_from(id)
            .ok()
            .and_then(|id| rules.country_name(CountryIdx(id))),
        NAME_WEAPON => u32::try_from(id)
            .ok()
            .and_then(|id| rules.weapon_at(id))
            .map(|weapon| weapon.id.as_str()),
        NAME_WARHEAD => u32::try_from(id)
            .ok()
            .and_then(|id| rules.warhead_at(id))
            .map(|warhead| warhead.id.as_str()),
        NAME_SUPER_WEAPON => usize::try_from(id)
            .ok()
            .and_then(|i| rules.super_weapon_order.get(i))
            .map(String::as_str),
        _ => return None,
    })
}

/// The `name_of` answer for a name: an ID without one is invalid.
fn name_answer(name: Option<&str>, kind: i32, id: i32) -> Result<(String, Status), Invalid> {
    name.map(|name| (name.to_owned(), Status::Ok))
        .ok_or_else(|| Invalid(format!("name kind {kind} has no ID {id}")))
}

/// `name_of(Variable, id)`. The Red Alert 2 variables have numbers, not
/// names.
fn variable_name(vars: &VarStore, id: i32) -> Result<(String, Status), Invalid> {
    if id < FIRST_DECLARED_VAR {
        return Ok((String::new(), Status::NotFound));
    }
    name_answer(vars.name(id), NAME_VARIABLE, id)
}

/// TechnoType `id`: an index into the rules' type registry.
fn techno_type(rules: &RuleSet, id: i32) -> Option<&crate::rules::object_type::ObjectType> {
    let id = usize::try_from(id).ok()?;
    rules.all_objects().nth(id)
}

/// A type's property (appendix 7.7). A wildcard or unknown ID is invalid.
fn type_property(
    rules: &RuleSet,
    kind: i32,
    id: u32,
    name: &str,
) -> Result<(i64, Status), Invalid> {
    if kind != NAME_TECHNO_TYPE {
        return Ok((0, Status::Unsupported));
    }
    let object = i32::try_from(id)
        .ok()
        .and_then(|id| techno_type(rules, id))
        .ok_or_else(|| Invalid(format!("no TechnoType {id}")))?;
    Ok(match name {
        "Strength" => (i64::from(object.strength), Status::Ok),
        "Cost" => (i64::from(object.cost), Status::Ok),
        _ => (0, Status::Unsupported),
    })
}

impl SimHost for Host<'_, '_> {
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
        has_kind(self.rules, table, id)
    }

    fn lookup(&mut self, kind: i32, name: &str) -> Result<(i32, Status), Invalid> {
        let id = match kind {
            NAME_TEAM_TYPE => self
                .sim
                .stock_definition(crate::sim::trigger_runtime::StockReferenceKind::Team, name)
                .map(|index| index as i32),
            NAME_VARIABLE => self.sim.ext_modules.vars.lookup(self.source, name),
            _ => match rules_name_id(self.rules, kind, name) {
                Some(id) => id,
                None => return Ok((-1, Status::Unsupported)),
            },
        };
        Ok(id.map_or((-1, Status::NotFound), |id| (id, Status::Ok)))
    }

    fn name_of(&mut self, kind: i32, id: i32) -> Result<(String, Status), Invalid> {
        let name = match kind {
            NAME_VARIABLE => return variable_name(&self.sim.ext_modules.vars, id),
            NAME_TEAM_TYPE => usize::try_from(id)
                .ok()
                .and_then(|id| self.sim.stock_team_type_name(id)),
            _ => match rules_id_name(self.rules, kind, id) {
                Some(name) => name,
                None => return Ok((String::new(), Status::Unsupported)),
            },
        };
        name_answer(name, kind, id)
    }

    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        if let Some(answer) = self.trigger_property(reference, prop, None) {
            return Ok(answer);
        }
        self.read(reference, prop)
    }

    fn set(&mut self, reference: i64, prop: i32, value: i64) -> Result<Status, Invalid> {
        if let Some((_, status)) = self.trigger_property(reference, prop, Some(value)) {
            return Ok(status);
        }
        let name = property_name(prop).unwrap_or_default();
        if let Some(Ref::Object(id)) = decode_ref(reference)?
            && self.sim.entities().get(id).is_some()
        {
            match name {
                "Health" => return Ok(self.write_health(id, value)),
                "Owner" => return self.write_owner(id, value),
                "Rank" => return self.write_rank(id, value),
                "Mission" => return self.order_mission(id, value, None),
                "Ammo" => return Ok(self.write_ammo(id, value)),
                "Online" => return Ok(self.write_online(id, value)),
                _ => {}
            }
        }
        let (_, status) = self.read(reference, prop)?;
        Ok(match status {
            Status::Ok => Status::Refused,
            other => other,
        })
    }

    fn modify(
        &mut self,
        reference: i64,
        prop: i32,
        op: i32,
        value: i64,
        frames: i32,
        source: i64,
        group: i32,
    ) -> Result<(i64, Status), Invalid> {
        let name = property_name(prop).unwrap_or_default();
        if !matches!(decode_ref(source)?, None | Some(Ref::Object(_))) {
            return Err(Invalid(format!(
                "modifier source {source:#x} is not an object"
            )));
        }
        let target = match decode_ref(reference)? {
            None => return Ok((0, Status::Gone)),
            Some(Ref::Object(id)) => id,
            Some(Ref::House(_)) => return Ok((0, Status::Unsupported)),
            Some(_) => {
                return Err(Invalid(format!("{reference:#x} is not an object or house")));
            }
        };
        if self.sim.entities().get(target).is_none() {
            return Ok((0, Status::Gone));
        }
        let Some(stat) = Stat::from_property(name) else {
            // Appendix 7.2's other properties are not wired yet; the rest
            // are not modifiable.
            let modifiable = property_id_range(MODIFIABLE).contains(&prop);
            return Ok((
                0,
                if modifiable {
                    Status::Unsupported
                } else {
                    Status::Refused
                },
            ));
        };
        let frame = self.sim.session.binary_frame();
        let modifier = Modifier {
            target,
            stat,
            op: ModifierOp::from_abi(op),
            value,
            expires: u32::try_from(frames)
                .ok()
                .map(|frames| frame.saturating_add(frames)),
            source,
            group,
            origin: self.source,
        };
        let sim = &mut *self.sim;
        let handle = sim.substrate.allocate_stable_id();
        if !sim
            .ext_modules
            .modifiers
            .insert(&mut sim.substrate.entities, handle, modifier)
        {
            return Ok((0, Status::Refused));
        }
        Ok((handle as i64, Status::Ok))
    }

    fn unmodify(&mut self, modifier: i64) -> Result<Status, Invalid> {
        let handle = match decode_ref(modifier)? {
            None => return Ok(Status::Gone),
            Some(Ref::Object(handle)) => handle,
            Some(_) => return Err(Invalid(format!("{modifier:#x} is not a modifier"))),
        };
        let sim = &mut *self.sim;
        let removed = sim
            .ext_modules
            .modifiers
            .remove(&mut sim.substrate.entities, handle);
        Ok(if removed { Status::Ok } else { Status::Gone })
    }

    fn find(&mut self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        match query.scope {
            SCOPE_OBJECTS => self.find_objects(query, cap),
            SCOPE_HOUSES => self.find_houses(query, cap),
            SCOPE_CELLS => self.find_cells(query, cap),
            SCOPE_MODIFIERS => self.find_modifiers(query, cap),
            _ => Ok((FindPage::default(), Status::Unsupported)),
        }
    }

    fn var_get(&mut self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        self.read_var(scope, var)
    }

    fn var_set(&mut self, scope: i64, var: i32, value: i64) -> Result<Status, Invalid> {
        self.sim.write_module_var(scope, var, value)
    }

    fn action(
        &mut self,
        kind: i32,
        target: i64,
        params: &ActionParams,
        text: &str,
    ) -> Result<(i64, Status), Invalid> {
        let context = ActionContext {
            rules: self.rules,
            overlay_registry: self.overlay_registry,
            frame_effects: &mut *self.frame_effects,
            text,
            trigger_house: None,
            source: self.source,
        };
        run_action(self.sim, kind, target, params, context)
    }

    fn trigger_create(
        &mut self,
        def: &vera20k_wasm_ext::TriggerDef,
    ) -> Result<(i64, Status), Invalid> {
        self.create_trigger(def)
    }

    fn trigger_remove(&mut self, trigger: i64) -> Result<Status, Invalid> {
        self.remove_trigger(trigger)
    }

    fn trigger_bind(&mut self, trigger: i64, reference: i64) -> Result<Status, Invalid> {
        self.bind_trigger(trigger, reference, true)
    }

    fn trigger_unbind(&mut self, trigger: i64, reference: i64) -> Result<Status, Invalid> {
        self.bind_trigger(trigger, reference, false)
    }

    fn rules_get(&mut self, section: &str, key: &str) -> Result<(Option<String>, Status), Invalid> {
        Ok(match self.rules_ini {
            Some(ini) => (rules::raw_rules_value(ini, section, key), Status::Ok),
            None => (None, Status::Unsupported),
        })
    }

    /// During a match only allowlisted keys are writable (RFC 8, open
    /// question 11); the allowlist is empty.
    fn rules_set(&mut self, _section: &str, _key: &str, _value: &str) -> Result<Status, Invalid> {
        Ok(Status::Refused)
    }

    fn order(&mut self, house: i64, order: &Order) -> Result<Status, Invalid> {
        self.queue_order(house, order)
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
    "Rank",
    "Mission",
    "Ammo",
    "Online",
    "Cost",
    "LandType",
    "Height",
    "Overlay",
    "Ore",
    "Waypoint",
    "CellTag",
    "Frame",
    "PlayerCount",
    "GameSpeed",
];

/// Appendix 7.2's modifiable properties: `PROPERTIES` from the first to the
/// last name.
const MODIFIABLE: (&str, &str) = ("ArmorMultiplier", "PowerOutage");

fn property_id_range((first, last): (&str, &str)) -> std::ops::RangeInclusive<i32> {
    let id = |name| vera20k_wasm_ext::property_id(name).unwrap_or(-1);
    id(first)..=id(last)
}

impl Host<'_, '_> {
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
                None => match self.sim.ext_modules.modifiers.get(id) {
                    Some(modifier) => modifier
                        .property(name, self.sim.session.binary_frame())
                        .map_or((0, Status::Unsupported), |value| (value, Status::Ok)),
                    None => (0, Status::Gone),
                },
            },
            Ref::House(index) => self.house_property(index, name),
            Ref::Type { kind, id } => type_property(self.rules, kind, id, name)?,
            Ref::Cell { x, y } => self.cell_property(x, y, name),
            Ref::Waypoint(number) => match self.waypoint_cell(number) {
                Some((x, y)) => self.cell_property(x, y, name),
                None => (0, Status::NotFound),
            },
            Ref::Global => self.global_property(name),
        })
    }

    /// A variable, as `var_get` reads it.
    fn read_var(&self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        if var >= FIRST_DECLARED_VAR {
            let holder = self.sim.module_var_holder(scope, var)?;
            return Ok(self.sim.ext_modules.vars.read(var, holder));
        }
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
        techno_type(self.rules, id)
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
            "Rank" => match entity.veterancy.rank_kind() {
                crate::sim::combat::veterancy::VeterancyRank::Rookie => 0,
                crate::sim::combat::veterancy::VeterancyRank::Veteran => 1,
                crate::sim::combat::veterancy::VeterancyRank::Elite => 2,
            },
            // `None` (255) for no mission (appendix 8.3).
            "Mission" => match entity.mission.current().raw() {
                -1 => 255,
                raw => i64::from(raw),
            },
            "Ammo" => match entity.ammo() {
                Some(ammo) => i64::from(ammo.current()),
                None => return (0, Status::Unsupported),
            },
            "Online" if entity.category() == EntityCategory::Structure => {
                i64::from(entity.building_online())
            }
            "Online" => return (0, Status::Unsupported),
            _ => match Stat::from_property(name) {
                Some(stat) => entity.effective_stat(stat),
                None => return (0, Status::Unsupported),
            },
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

    /// Whether `candidate` passes every clause; `None` when a clause needs
    /// what is not kept yet. Clauses read like `get`; a reading that is not
    /// `Ok` fails its clause.
    fn passes(&self, candidate: i64, query: &Query) -> Result<Option<bool>, Invalid> {
        for clause in &query.clauses {
            let (reading, status) = match clause.subject {
                ClauseSubject::Property(prop) => self.read(candidate, prop)?,
                ClauseSubject::Var { var, subject } => {
                    self.read_var(if subject { candidate } else { 0 }, var)?
                }
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

    /// `find` over modifiers, in insertion (handle) order. Modifiers have no
    /// place, type or owner: a query with one answers `Unsupported`. Select
    /// an object's with a `Target = ref` clause.
    fn find_modifiers(&self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        if query.area != Area::Map || query.type_ref != 0 || query.owner != 0 {
            return Ok((FindPage::default(), Status::Unsupported));
        }
        let mut page = FindPage::default();
        let after = u64::try_from(query.after).unwrap_or(0);
        for handle in self.sim.ext_modules.modifiers.handles_from(after) {
            if page.found.len() == cap {
                break;
            }
            page.scanned += 1;
            let reference = handle as i64;
            match self.passes(reference, query)? {
                Some(true) => page.found.push((reference, reference)),
                Some(false) => {}
                None => return Ok((FindPage::default(), Status::Unsupported)),
            }
        }
        Ok((page, Status::Ok))
    }

    /// `Health`: sets health without kill credit or damage events, clamped
    /// to the type's Strength. Writing 0 or less would kill without credit,
    /// which needs the death owner's credit-free path: it answers `Refused`.
    fn write_health(&mut self, id: u64, value: i64) -> Status {
        let sim = &mut *self.sim;
        let Some(entity) = sim.substrate.entities.get(id) else {
            return Status::Gone;
        };
        let Some(strength) = sim
            .entity_object_type(entity, self.rules)
            .map(|o| o.strength)
        else {
            return Status::Refused;
        };
        if !entity.lifecycle.object_alive() {
            return Status::Gone;
        }
        if value <= 0 || entity.death.is_dying() || entity.health.current() <= 0 {
            return Status::Refused;
        }
        let health = i32::try_from(value)
            .unwrap_or(i32::MAX)
            .min(strength.max(1));
        let owner = entity.owner();
        let structure = entity.category() == EntityCategory::Structure;
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.health.set(health);
        }
        // As the cheat heal does: a building's power follows its health.
        if structure {
            Simulation::invalidate_house_power(&sim.houses, &mut sim.power_states, owner, true);
        }
        Status::Ok
    }

    /// `Owner`: ChangeOwner, as the stock change-house trigger action runs it
    /// on a live, placed object. A gone house answers `Gone`; an object in
    /// limbo answers `Refused`.
    fn write_owner(&mut self, id: u64, value: i64) -> Result<Status, Invalid> {
        let Some(Ref::House(_)) = decode_ref(value)? else {
            return Err(Invalid(format!("{value:#x} is not a house reference")));
        };
        let Some(house) = self.house(value)? else {
            return Ok(Status::Gone);
        };
        let Some(entity) = self.sim.entities().get(id) else {
            return Ok(Status::Gone);
        };
        if !entity.lifecycle.object_alive() {
            return Ok(Status::Gone);
        }
        if !entity.lifecycle.cell_marked() || entity.lifecycle.in_limbo() {
            return Ok(Status::Refused);
        }
        self.sim.change_owner_with_rules(
            id,
            house,
            self.rules,
            self.overlay_registry,
            self.frame_effects,
        );
        Ok(Status::Ok)
    }

    /// `Rank`: VeterancyStruct's SetRookie/SetVeteran/SetElite; the next
    /// AI_Update sample announces a promotion as for a crate.
    fn write_rank(&mut self, id: u64, value: i64) -> Result<Status, Invalid> {
        let Some(entity) = self.sim.substrate.entities.get_mut(id) else {
            return Ok(Status::Gone);
        };
        match value {
            0 => entity.veterancy.set_rookie(),
            1 => entity.veterancy.set_veteran(),
            2 => entity.veterancy.set_elite(),
            _ => return Err(Invalid(format!("rank {value} is not a Rank"))),
        }
        Ok(Status::Ok)
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
    /// The holder of declared variable `var` that `scope` names (`None`
    /// when gone); a wrong scope or unknown variable is invalid.
    fn module_var_holder(&self, scope: i64, var: i32) -> Result<Option<vars::Holder>, Invalid> {
        self.ext_modules.vars.holder(
            scope,
            var,
            |index| index < self.houses.order().len(),
            |id| self.entities().get(id).is_some(),
        )
    }

    /// The one variable writer for modules: `var_set` and the `SetVar`
    /// action. Declared variables go to [`VarStore`]; Red Alert 2 variables
    /// go through the trigger runtime's writer, which also resets the timers
    /// of triggers waiting on the variable (events 27, 28, 36, 37), as map
    /// actions do.
    fn write_module_var(&mut self, scope: i64, var: i32, value: i64) -> Result<Status, Invalid> {
        if var >= FIRST_DECLARED_VAR {
            let Some(holder) = self.module_var_holder(scope, var)? else {
                return Ok(Status::Gone);
            };
            self.ext_modules.vars.set(var, holder, value);
            return Ok(Status::Ok);
        }
        let (global, index) = stock_variable(scope, var)?;
        Simulation::set_trigger_variable(
            &self.session,
            &mut self.scenario_rng,
            &mut self.trigger_runtime,
            global,
            index as i32,
            value != 0,
        );
        Ok(Status::Ok)
    }

    /// Loads a module and runs its `vera_init`. An error means the module
    /// cannot run and the match should not start (RFC section 3).
    pub fn install_ext_module(
        &mut self,
        rules: &RuleSet,
        source: ModuleSource,
        bytes: &[u8],
    ) -> Result<(), String> {
        let module = SimModule::load(bytes, source.limits()).map_err(|e| e.to_string())?;
        let position = self.insert_ext_slot(source, module, FRAME_BUDGET);
        self.init_ext_slot(rules, position)
    }

    /// Puts a loaded module in its run-order slot and takes its variable
    /// declarations; returns the slot index.
    fn insert_ext_slot(
        &mut self,
        source: ModuleSource,
        module: SimModule,
        frame_budget: u64,
    ) -> usize {
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
                frame_budget,
                orders: Vec::new(),
            },
        );
        self.ext_modules.declare_vars();
        position
    }

    /// Runs a new slot's `vera_init`; a fault removes the slot.
    fn init_ext_slot(&mut self, rules: &RuleSet, position: usize) -> Result<(), String> {
        let result = self.call_ext_module(
            rules,
            Some(rules.overlay_registry()),
            &mut FrameEffects::default(),
            position,
            Export::Init,
            ONCE_BUDGET,
        );
        match result {
            Ok(_) => Ok(()),
            Err(fault) => {
                self.ext_modules.slots.remove(position);
                self.ext_modules.declare_vars();
                Err(format!("vera_init {fault}"))
            }
        }
    }

    /// Takes over a scenario's modules (RFC section 2), the selected mod's
    /// `rules.wasm` then the one the map embeds ([`PreparedExtModules`]).
    /// Those that ran `vera_init` and `on_rules` while `rules` were
    /// processed keep their state; the others run `vera_init` now, once
    /// every module's variables are declared. `rules_ini` is the merged
    /// rules text `rules_get` reads. An error names the module; the match
    /// must not start.
    pub fn install_scenario_ext_modules(
        &mut self,
        rules: &RuleSet,
        prepared: PreparedExtModules,
        rules_ini: IniFile,
    ) -> Result<(), String> {
        if let Some(source) = prepared.missed_rules_stage() {
            return Err(format!(
                "the {source:?} module exports on_rules, which this way of loading cannot run"
            ));
        }
        let PreparedExtModules {
            policy,
            modules,
            log,
        } = prepared;
        self.ext_modules.log.extend(log);
        self.ext_modules.map_ui = policy.map_ui;
        if modules.is_empty() {
            return Ok(());
        }
        self.ext_modules.rules_ini = Some(rules_ini);
        let mut uninitialized = Vec::new();
        for prepared in modules {
            let source = prepared.source;
            self.insert_ext_slot(source, prepared.module, policy.budget_of(source));
            if !prepared.initialized {
                uninitialized.push(source);
            }
        }
        for source in uninitialized {
            let position = self
                .ext_modules
                .slots
                .iter()
                .position(|slot| slot.source == source)
                .ok_or_else(|| format!("the {source:?} module was not installed"))?;
            self.init_ext_slot(rules, position)
                .map_err(|e| format!("the {source:?} module: {e}"))?;
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
                frame_budget: saved.frame_budget,
                orders: saved.orders,
            });
        }
        self.ext_modules.declare_vars();
        self.ext_modules
            .modifiers
            .refresh_all(&mut self.substrate.entities);
        Ok(())
    }

    /// Drops what modules hold on an object that leaves the game: its
    /// modifiers and object-scope variables.
    pub(crate) fn forget_ext_module_object(&mut self, stable_id: u64) {
        if !self.ext_modules.modifiers.is_empty() {
            self.ext_modules.modifiers.forget_object(stable_id);
        }
        self.ext_modules.vars.forget_object(stable_id);
    }

    pub fn ext_modules(&self) -> &ExtModules {
        &self.ext_modules
    }

    pub fn ext_modules_mut(&mut self) -> &mut ExtModules {
        &mut self.ext_modules
    }

    /// The module phase: modifiers whose time is up expire and `Timer`
    /// triggers whose period ends fire, then each enabled module in order
    /// gets its `on_scenario_start` once, its queued event records
    /// (`on_events`), then `on_frame`.
    pub(crate) fn run_ext_module_phase(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        if !self.ext_modules.modifiers.is_empty() {
            let frame = self.session.binary_frame();
            self.ext_modules
                .modifiers
                .expire(&mut self.substrate.entities, frame);
        }
        self.raise_module_timers(rules, frame_effects);
        for index in 0..self.ext_modules.slots.len() {
            if self.ext_modules.slots[index].disabled.is_some() {
                continue;
            }
            if !self.ext_modules.slots[index].started {
                self.ext_modules.slots[index].started = true;
                if let Err(fault) = self.call_ext_module(
                    rules,
                    overlay_registry,
                    frame_effects,
                    index,
                    Export::ScenarioStart,
                    ONCE_BUDGET,
                ) {
                    self.disable_ext_module(index, Export::ScenarioStart, fault);
                    continue;
                }
            }
            if let Err(fault) =
                self.deliver_ext_module_events(rules, overlay_registry, frame_effects, index)
            {
                self.disable_ext_module(index, Export::Events, fault);
                continue;
            }
            let budget = self.ext_modules.slots[index].frame_budget;
            match self.call_ext_module(
                rules,
                overlay_registry,
                frame_effects,
                index,
                Export::Frame,
                budget,
            ) {
                Ok(ops) => self.ext_modules.slots[index].last_frame_ops = ops,
                Err(fault) => self.disable_ext_module(index, Export::Frame, fault),
            }
        }
    }

    /// Calls one module.
    fn call_ext_module(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
        index: usize,
        export: Export,
        budget: u64,
    ) -> Result<u64, Fault> {
        self.with_ext_module(
            rules,
            overlay_registry,
            frame_effects,
            index,
            |module, host| module.call(export, budget, host),
        )
    }

    /// Runs `f` on module `index` and its host. The slot list is set aside
    /// meanwhile, so the host reaches the whole simulation; what a call
    /// reaches that needs a module's host finds none
    /// (`triggers::with_module_host`).
    fn with_ext_module<R>(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
        index: usize,
        f: impl FnOnce(&mut SimModule, &mut Host<'_, '_>) -> R,
    ) -> R {
        let mut slots = std::mem::take(&mut self.ext_modules.slots);
        let rules_ini = self.ext_modules.rules_ini.take();
        let slot = &mut slots[index];
        let mut host = Host {
            sim: self,
            rules,
            overlay_registry,
            frame_effects,
            rng: &mut slot.rng,
            source: slot.source,
            orders: &mut slot.orders,
            rules_ini: rules_ini.as_ref(),
        };
        let result = f(&mut slot.module, &mut host);
        self.ext_modules.slots = slots;
        self.ext_modules.rules_ini = rules_ini;
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
        // Its queued orders, triggers and queued records go with it (RFC
        // section 3).
        slot.orders.clear();
        let source = slot.source;
        self.trigger_runtime
            .module_triggers_mut()
            .remove_source(source);
        self.publish_trigger_effects([
            crate::sim::trigger_runtime::TriggerEffect::ExtModuleStopped { reason },
        ]);
    }
}

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_tests.rs"]
mod tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_trigger_tests.rs"]
mod trigger_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_action_tests.rs"]
mod action_tests;
