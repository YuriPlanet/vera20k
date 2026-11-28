//! Wasm extension modules in the simulation ([RFC 0001](../../docs/rfcs/0001-wasm-extension-modules.md)).
//!
//! Minimal first step. A loaded `rules.wasm` runs in the module phase, which
//! sits at the end of the Triggers step (open question 8), mod first, then
//! map:
//! - `on_scenario_start` once on its first phase;
//! - `on_frame` every frame.
//!
//! Each call gets a whole operation budget. What the frame has used only
//! decides whether another call starts, so a busy frame defers work instead
//! of stopping the module. A fault disables only that module, on every
//! machine in the same frame. The host offers:
//! - Red Alert 2 local and global variables, through the trigger runtime's
//!   own writer;
//! - the module's own random stream;
//! - the log;
//! - names of TechnoTypes, countries and properties (`lookup`, `name_of`);
//! - reads of the object, house and type properties [`Host::get`] lists
//!   ([`props`]), viewpoint properties in filter clauses and queries, and
//!   `find` over objects and houses;
//! - modifiers on objects ([`modifiers`]) and the variables modules declare
//!   in `vera.vars` ([`vars`]);
//! - writes of `Health`, `Owner`, `Rank`, `Mission`, `Ammo` and `Online`
//!   through their owners ([`writes`]);
//! - runtime triggers and `on_events` delivery ([`triggers`]), and
//!   decisions through `on_decide` ([`decide`]);
//! - names of weapons, warheads, TeamTypes and action kinds;
//! - the `action` import ([`actions`]): atomic actions and stock trigger
//!   actions through their owners, for direct calls and trigger actions;
//! - cell and global properties and `find` over cells ([`cells`]);
//! - superweapon properties and `find` over superweapons ([`supers`]);
//! - house orders for computer houses ([`orders`]);
//! - client messages for `on_message` ([`messages`]), notifications
//!   (`send_notify`) and forwarded simulation events for client modules
//!   ([`client_outbox`]);
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
//! presentation copy carries no modules (RFC 15.1, item 8). The state hash
//! and the multiplayer frame checksum read a module's memory through its
//! page digests, refreshed a fifteenth at a time ([`PageDigests`]).
//!
//! Residuals before this leaves preview:
//! - The in-game notices of a stopped module and of a map's ignored
//!   `Wasm.Tags` are English only.
//! - House and superweapon references use provisional encodings
//!   ([`house_ref`], [`super_ref`]); the RFC puts both on the objects'
//!   creation counter (see there).
//! - TechnoType IDs are the rules' registry handles (`TypeHandle`), found by
//!   name; a type whose ID two categories share reads as the first.
//! - `set` writes `Health` (1 up to the type's Strength; a write of 0 or
//!   less, which would kill without credit, answers `Refused`), `Owner`
//!   (through ChangeOwner, as the stock change-house action does), `Rank`,
//!   `Mission` (ordered as a player's order), `Ammo` and a building's
//!   `Online` ([`writes`]). Other readable properties refuse.
//! - The modifiable properties [`modifiers`] wires take modifiers; other
//!   modifiable properties answer `Unsupported`, and houses take none.

use std::hash::{Hash, Hasher};

use vera20k_wasm_ext::decide::{EVENT_DEFEAT, EVENT_PRODUCTION_ALLOWED};
use vera20k_wasm_ext::{
    ActionParams, Area, ClauseSubject, Export, Fault, FindPage, Invalid, Limits,
    MAX_NOTIFIES_PER_FRAME, ModuleState, NAME_ACTION_KIND, NAME_ANIM, NAME_COUNTRY,
    NAME_MODULE_EVENT, NAME_NOTIFY, NAME_PARTICLE_SYSTEM, NAME_PROJECTILE, NAME_PROPERTY,
    NAME_STRING, NAME_SUPER_WEAPON, NAME_TEAM_TYPE, NAME_TECHNO_TYPE, NAME_VARIABLE,
    NAME_VOXEL_ANIM, NAME_WARHEAD, NAME_WEAPON, Order, Query, SCOPE_CELLS, SCOPE_HOUSES,
    SCOPE_MODIFIERS, SCOPE_OBJECTS, SCOPE_SUPER_WEAPONS, SCOPE_TYPES, SimHost, SimModule, Status,
    TABLE_ACTION, TABLE_NAME_KIND, TABLE_PROPERTY, TABLE_SCOPE, property_name,
};

mod actions;
mod cells;
mod chains;
pub mod client_reads;
mod claims;
mod client_outbox;
mod control;
mod decide;
mod events;
mod held;
mod ini_names;
mod ini_start;
mod ini_triggers;
mod messages;
pub(crate) mod modifiers;
mod motion;
mod orders;
mod paths;
pub(crate) mod projectiles;
mod props;
mod rules;
mod supers;
mod unit_ai;
pub mod vars;
mod writes;

pub use actions::{ActionContext, run_action};
pub(crate) use client_outbox::CLIENT_FORWARDED_EVENTS;
pub use client_outbox::{ClientNotify, ClientOutbox, ClientSimEvent};
pub(crate) use decide::CommandAdmission;
use modifiers::{Modifier, ModifierOp, ModifierStore, Stat};
pub use orders::{CheckedOrder, check_client_order};
pub(crate) use orders::take_due_orders;
pub(crate) use props::View;
use props::type_property;
use rules::check_notify_collision;
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

/// The frame budget B at most and the mod module's default (RFC 15.4,
/// 0.0.21; open question 7): the limit of one call, and the frame's use
/// past which no further batch starts. A frame stays below 3B plus the
/// decision pools (RFC section 5). Claimed seats add [`SEAT_BUDGET`] each.
pub const FRAME_BUDGET: u64 = 500_000;
/// A map module's default budget (RFC 15.4).
pub const MAP_FRAME_BUDGET: u64 = 100_000;
/// What each claimed seat adds by default, and at most (RFC 15.4, 6i).
pub const SEAT_BUDGET: u64 = 500_000;
/// A configured budget below this refuses the match (RFC 15.4).
pub const MIN_BUDGET: u64 = 20_000;
/// Operations for each one-time call: `vera_init` and `on_scenario_start`.
pub const ONCE_BUDGET: u64 = 5_000_000;

/// Module log lines kept for `take_log`; older ones are dropped, so a
/// chatty module cannot grow memory over a match.
const LOG_KEPT: usize = 256;
/// Module log lines shown to the players per frame; the rest go only to the
/// log file.
const MAX_SHOWN_LOG_LINES: u32 = 8;
/// Characters of a shown log line.
const SHOWN_LOG_CHARS: usize = 160;

/// Variable IDs (RFC appendix "IDs"): Red Alert 2 locals 0..99, globals 100..149.
const LOCAL_VARS: std::ops::Range<i32> = 0..100;
const GLOBAL_VARS: std::ops::Range<i32> = 100..150;

/// Atomic action `AddMoney`: atomic actions are numbered from 1000 in
/// appendix 5.3 table order, where it is entry 15 (numbers are open question 5).
pub const ACTION_ADD_MONEY: i32 = actions::ADD_MONEY;

/// Provisional house reference: kind 0 with bit 47 set over the house array
/// index. Houses here have no creation serial (the RFC's appendix
/// "References" puts them on the objects' counter). It stays so: the houses
/// exist before the first object, so serials for them would renumber every
/// object of every game, modules or not, and change the state hash of stock
/// play and the meaning of serials in saves and replays already made; and the
/// Rust SDK's `House::from_index`, which builds the reference without a host
/// call, would need one. What the RFC asks of a house handle holds here as
/// well: it is unique against every serial (a match allocates far fewer than
/// 2^47), it never expires, and the house array is fixed for a match.
const HOUSE_REF_BIT: i64 = 1 << 47;

pub const fn house_ref(index: usize) -> i64 {
    HOUSE_REF_BIT | index as i64
}

/// Superweapon references: bit 46 over (house index << 16) | the
/// `[SuperWeaponTypes]` index ([`supers`]). A House's Supers are fixed for
/// the match, so for the reasons given at [`HOUSE_REF_BIT`] they stay off
/// the creation counter as well; unique against every serial (far below
/// 2^46) and every house reference (bit 47 clear).
const SUPER_REF_BIT: i64 = 1 << 46;

pub(crate) const fn super_ref(house: usize, index: usize) -> i64 {
    SUPER_REF_BIT | ((house as i64) << 16) | index as i64
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
    /// Operations per frame this frame: `base_budget` plus `seat_budget`
    /// for each house this module claims (RFC 15.4, 6i), set at the start
    /// of each module phase.
    frame_budget: u64,
    /// [`FRAME_BUDGET`], or the `[VeraWasm]` budget for this source.
    base_budget: u64,
    /// `[VeraWasm] SeatBudget=`.
    seat_budget: u64,
    /// The "mod AI" seats its memory limit counts.
    memory_seats: usize,
    /// The frame the three counts below belong to. Saves resume at frame
    /// boundaries, so they carry only the configured budget.
    budget_frame: u32,
    /// `ProductionAllowed`, event delivery and `on_frame`. Decides whether
    /// another call starts and never shortens one.
    frame_ops_used: u64,
    /// The other decisions, apart from `Defeat`.
    decide_ops_used: u64,
    defeat_ops_used: u64,
    /// Orders waiting for the next command step ([`orders`]).
    orders: Vec<orders::PendingOrder>,
    /// The memory digests the state hash and the checksum read
    /// ([`PageDigests`]).
    digests: PageDigests,
}

impl Slot {
    fn roll_to(&mut self, frame: u32) {
        if self.budget_frame != frame {
            self.budget_frame = frame;
            self.frame_ops_used = 0;
            self.decide_ops_used = 0;
            self.defeat_ops_used = 0;
        }
    }

    /// Whether the module phase may start another call this frame. The call
    /// then has the whole frame budget (RFC section 5).
    fn may_start_call(&mut self, frame: u32) -> bool {
        self.roll_to(frame);
        self.frame_ops_used < self.frame_budget
    }

    fn charge_frame_ops(&mut self, frame: u32, used: u64) {
        self.roll_to(frame);
        self.frame_ops_used += used;
    }

    /// What decisions of `kind` count against, and its size (RFC section 5).
    fn decision_pool(&mut self, frame: u32, kind: i32) -> (&mut u64, u64) {
        self.roll_to(frame);
        let budget = self.frame_budget;
        match kind {
            EVENT_PRODUCTION_ALLOWED => (&mut self.frame_ops_used, budget),
            EVENT_DEFEAT => (&mut self.defeat_ops_used, (budget / 4).max(1)),
            _ => (&mut self.decide_ops_used, (budget / 2).max(1)),
        }
    }
}

/// The ids of four ascending lists, ascending, without allocating.
fn merge_ascending(mut lists: [&[u64]; 4]) -> impl Iterator<Item = u64> + '_ {
    std::iter::from_fn(move || {
        let list = lists
            .iter_mut()
            .filter(|list| !list.is_empty())
            .min_by_key(|list| list[0])?;
        let (&id, rest) = list.split_first()?;
        *list = rest;
        Some(id)
    })
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
    /// The base budget (`Slot::base_budget`).
    #[serde(default = "default_frame_budget")]
    frame_budget: u64,
    #[serde(default = "default_seat_budget")]
    seat_budget: u64,
    #[serde(default)]
    memory_seats: usize,
    #[serde(default)]
    orders: Vec<orders::PendingOrder>,
    /// Saved so a loaded game hashes what the running one does. Without
    /// any, each page's is 0 until its turn, as for a new page.
    #[serde(default)]
    digests: PageDigests,
}

const fn default_frame_budget() -> u64 {
    FRAME_BUDGET
}

const fn default_seat_budget() -> u64 {
    SEAT_BUDGET
}

/// Frames for the digest rotation to visit every page (RFC 15.1, item 7).
const DIGEST_ROTATION: usize = 15;

/// One XXH32 digest per 64 KiB page of a module's memory, as far as
/// it has grown (`memory.size`), for the state hash and the multiplayer
/// checksum (RFC 15.1, item 7). [`PageDigests::refresh`] runs once a frame
/// at the end of the module phase: it digests the pages whose index is the
/// frame modulo 15; the others keep their digest, and a new page's is 0
/// until its turn. The page count is in the digests' length, so growth shows
/// at once. A divergence that lasts until its page's turn shows within 15
/// frames, and no frame digests more than a fifteenth of the memory, also
/// when it grows. Every machine refreshes the same pages on the same frames,
/// so the digests agree wherever the memory does.
/// A derived cache of the memory, saved with it (`SavedSlot::digests`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct PageDigests(Vec<u32>);

impl PageDigests {
    /// Every page of `memory` digested now.
    #[cfg(test)]
    fn of(memory: &[u8]) -> Self {
        Self(
            memory
                .chunks(vera20k_wasm_ext::PAGE_BYTES)
                .map(page_digest)
                .collect(),
        )
    }

    /// Digests this frame's share of a memory of `len` bytes (see the type),
    /// copying only the pages due with `read(offset, page)`: a browser
    /// module's memory lives outside the engine's.
    fn refresh(&mut self, len: usize, frame: u32, mut read: impl FnMut(usize, &mut [u8]) -> bool) {
        const PAGE: usize = vera20k_wasm_ext::PAGE_BYTES;
        self.0.resize(len / PAGE, 0);
        let phase = frame as usize % DIGEST_ROTATION;
        let mut page = vec![0u8; PAGE];
        for (index, digest) in self.0.iter_mut().enumerate().skip(phase).step_by(DIGEST_ROTATION) {
            if read(index * PAGE, &mut page) {
                *digest = page_digest(&page);
            }
        }
    }

    /// The digests as bytes, page order.
    fn bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.iter().flat_map(|digest| digest.to_le_bytes())
    }
}

/// XXH32 with seed 0, which reads the page by little-endian words (RFC
/// 15.1, item 7; <https://github.com/Cyan4973/xxHash/blob/dev/doc/xxhash_spec.md>).
fn page_digest(page: &[u8]) -> u32 {
    const P: [u32; 5] = [
        0x9E37_79B1,
        0x85EB_CA77,
        0xC2B2_AE3D,
        0x27D4_EB2F,
        0x1656_67B1,
    ];
    let word = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let round = |lane: u32, bytes: &[u8]| {
        lane.wrapping_add(word(bytes).wrapping_mul(P[1]))
            .rotate_left(13)
            .wrapping_mul(P[0])
    };
    let mut stripes = page.chunks_exact(16);
    let mut h = if page.len() < 16 {
        P[4]
    } else {
        let mut lanes = [P[0].wrapping_add(P[1]), P[1], 0, 0u32.wrapping_sub(P[0])];
        for stripe in &mut stripes {
            lanes = [
                round(lanes[0], &stripe[..4]),
                round(lanes[1], &stripe[4..8]),
                round(lanes[2], &stripe[8..12]),
                round(lanes[3], &stripe[12..]),
            ];
        }
        lanes[0]
            .rotate_left(1)
            .wrapping_add(lanes[1].rotate_left(7))
            .wrapping_add(lanes[2].rotate_left(12))
            .wrapping_add(lanes[3].rotate_left(18))
    };
    h = h.wrapping_add(page.len() as u32);
    let mut words = stripes.remainder().chunks_exact(4);
    for bytes in &mut words {
        h = h
            .wrapping_add(word(bytes).wrapping_mul(P[2]))
            .rotate_left(17)
            .wrapping_mul(P[3]);
    }
    for &byte in words.remainder() {
        h = h
            .wrapping_add(u32::from(byte).wrapping_mul(P[4]))
            .rotate_left(11)
            .wrapping_mul(P[0]);
    }
    h = (h ^ (h >> 15)).wrapping_mul(P[1]);
    h = (h ^ (h >> 13)).wrapping_mul(P[2]);
    h ^ (h >> 16)
}

/// The loaded simulation modules, in run order, and the modifiers and
/// variables they own.
#[derive(Default)]
pub struct ExtModules {
    slots: Vec<Slot>,
    modifiers: ModifierStore,
    vars: VarStore,
    /// Timed conversions and forbidden types actions left ([`held`]).
    held: held::ActionHeld,
    /// Modules read from a save, instantiated by
    /// [`Simulation::restore_ext_modules`]. Deserializing never compiles a
    /// module, so describing the save layout stays data-only.
    pending: Vec<SavedSlot>,
    /// The latest [`LOG_KEPT`] log lines from modules (and why a module
    /// stopped), oldest first; `take_log` drains them.
    log: Vec<String>,
    /// (frame, lines shown) for [`MAX_SHOWN_LOG_LINES`]; not saved, it
    /// only limits what the players see.
    shown_log: (u32, u32),
    /// How deep the module trigger chain on the stack is and how many
    /// chained firings this frame ran ([`chains`]). Zero between frames;
    /// not saved.
    chain: chains::ChainState,
    /// The merged rules text `rules_get` reads during the match; `None`
    /// without modules.
    rules_ini: Option<IniFile>,
    /// Decision `random` substreams of the current frame ([`decide`]); not saved.
    decide: decide::DecideState,
    /// Registered INI names and the string table ([`ini_names`]).
    ini: ini_names::IniNames,
    /// House domain claims (RFC 6i, [`claims`]).
    claims: claims::DomainClaims,
    /// Per-projectile module data and `Launch` paths ([`projectiles`]).
    projectiles: projectiles::ModuleProjectiles,
    /// The `Push`es in progress ([`motion`]).
    motions: motion::ModuleMotions,
    /// The mod's `[VeraWasm] Controller=`, for "mod AI" seats
    /// ([`Simulation::seat_mod_ai_houses`]). Not saved: seats are given
    /// when the match starts, and a save carries the claims.
    controller: bool,
    /// The `Commanded` serials of the current frame ([`orders`]); not
    /// saved, as a save resumes at a frame boundary.
    command_serial: orders::CommandSerial,
    /// Client messages waiting for `on_message` and the rate counts
    /// ([`messages`]).
    messages: messages::ClientMessages,
    /// (frame, notifications sent) per source, for
    /// [`MAX_NOTIFIES_PER_FRAME`]; not saved, as a save resumes at a frame
    /// boundary.
    notify_count: [(u32, u32); 2],
    /// What client modules receive ([`client_outbox`]); not saved, not
    /// hashed.
    client_outbox: ClientOutbox,
    /// The room switch "Allow automation", the global `AutomationAllowed`
    /// (appendix 7.8, RFC 6i): from the room's settings
    /// (`PersonalRules::automation_allowed`), saved with the match.
    automation_allowed: bool,
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
    #[serde(default)]
    held: held::ActionHeld,
    #[serde(default)]
    ini: ini_names::IniNames,
    #[serde(default)]
    claims: claims::DomainClaims,
    #[serde(default)]
    projectiles: projectiles::ModuleProjectiles,
    #[serde(default)]
    motions: motion::ModuleMotions,
    messages: messages::ClientMessages,
    #[serde(default)]
    automation_allowed: bool,
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
                frame_budget: slot.base_budget,
                seat_budget: slot.seat_budget,
                memory_seats: slot.memory_seats,
                orders: slot.orders.clone(),
                digests: slot.digests.clone(),
            })
            .collect();
        SavedExtModules {
            slots,
            modifiers: self.modifiers.clone(),
            vars: self.vars.values.clone(),
            rules_ini: self.rules_ini.clone(),
            held: self.held.clone(),
            ini: self.ini.clone(),
            claims: self.claims.clone(),
            projectiles: self.projectiles.clone(),
            motions: self.motions.clone(),
            messages: self.messages.clone(),
            automation_allowed: self.automation_allowed,
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
            held: saved.held,
            ini: saved.ini,
            claims: saved.claims,
            projectiles: saved.projectiles,
            motions: saved.motions,
            messages: saved.messages,
            automation_allowed: saved.automation_allowed,
            ..Self::default()
        };
        modules.vars.values = saved.vars;
        Ok(modules)
    }
}

/// The presentation copy excludes module state (RFC 15.1, item 8).
/// A copy holds no module, only the variable declarations: the world copy
/// drawing reads while the simulation ticks elsewhere
/// (`Simulation::presentation_copy`) names variables for the status
/// templates and client reads. The values stay the simulation's.
impl Clone for ExtModules {
    fn clone(&self) -> Self {
        Self {
            vars: self.vars.declarations_only(),
            ..Self::default()
        }
    }
}

impl ExtModules {
    /// What a fork of the world holds (`world::fork`): no module. A live
    /// instance cannot be copied, so a turn on a fork must not call one; the
    /// forked object pass runs only while no module is loaded.
    pub(crate) fn fork_empty(&self) -> Self {
        debug_assert!(self.slots.is_empty(), "a world with Wasm modules was forked");
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The next command starts a new `Commanded` serial: one module order's
    /// commands share one ([`orders`]).
    pub(crate) fn begin_command_group(&mut self) {
        self.command_serial.begin_group();
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

    /// Each loaded module's memory size in bytes (`memory.size`).
    #[cfg(test)]
    pub(crate) fn memory_bytes(&self) -> Vec<usize> {
        self.slots.iter().map(|s| s.module.memory_len()).collect()
    }

    /// Keeps `line`, dropping the oldest past [`LOG_KEPT`].
    fn keep_log(&mut self, line: String) {
        if self.log.len() >= LOG_KEPT {
            self.log.remove(0);
        }
        self.log.push(line);
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

    /// The declared variables (RFC 6e), for client reads.
    pub fn vars(&self) -> &VarStore {
        &self.vars
    }

    /// Declares variables as loaded modules would, for client read tests.
    #[cfg(test)]
    pub(crate) fn declare_vars_for_test(
        &mut self,
        decls: &[(ModuleSource, vera20k_wasm_ext::VarDecl)],
    ) {
        self.vars.declare(
            decls
                .iter()
                .map(|(source, decl)| (*source, std::slice::from_ref(decl))),
        );
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

    /// One 32-bit FNV-1a digest per module over its globals and its memory
    /// page digests ([`PageDigests`], the 1/15 rotation), for the
    /// multiplayer frame checksum, then one over the modifiers and the
    /// declared variables' values when any are held. Byte-wise, in handle and
    /// key order, so 32- and 64-bit platforms agree.
    pub(crate) fn checksum_digests(&self) -> impl Iterator<Item = u32> + '_ {
        let modules = self.slots.iter().map(|slot| {
            let globals = slot.module.globals().iter().flat_map(|g| g.to_le_bytes());
            fnv1a(globals.chain(slot.digests.bytes()))
        });
        let values = &self.vars.values;
        let held = !self.modifiers.is_empty()
            || !values.global.is_empty()
            || !values.house.is_empty()
            || !values.object.is_empty();
        let ini = (!self.ini.is_empty()).then(|| fnv1a(self.ini.state_bytes()));
        let claims = (!self.claims.is_empty()).then(|| fnv1a(self.claims.state_bytes()));
        let projectiles =
            (!self.projectiles.is_empty()).then(|| fnv1a(self.projectiles.state_bytes()));
        let motions = (!self.motions.is_empty()).then(|| fnv1a(self.motions.state_bytes()));
        let messages = (!self.messages.is_empty()).then(|| fnv1a(self.messages.state_bytes()));
        modules
            .chain(held.then(|| fnv1a(self.held_state_bytes())))
            .chain(ini)
            .chain(claims)
            .chain(projectiles)
            .chain(motions)
            .chain(messages)
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
            // Memory through its page digests, the digest the checksum
            // shares (RFC 15.1, item 7).
            slot.digests.hash(hasher);
            slot.orders.hash(hasher);
        }
        self.modifiers.hash(hasher);
        self.vars.values.hash(hasher);
        if !self.held.is_empty() {
            self.held.hash(hasher);
        }
        if !self.ini.is_empty() {
            for byte in self.ini.state_bytes() {
                byte.hash(hasher);
            }
        }
        if !self.claims.is_empty() {
            self.claims.hash(hasher);
        }
        if !self.projectiles.is_empty() {
            self.projectiles.hash(hasher);
        }
        if !self.motions.is_empty() {
            self.motions.hash(hasher);
        }
        if !self.messages.is_empty() {
            self.messages.hash(hasher);
        }
        if self.automation_allowed {
            b"automation-allowed".hash(hasher);
        }
    }

    /// The global `AutomationAllowed` (appendix 7.8): whether the room
    /// allows client modules to play for their player (RFC 6i,
    /// `client_send_order`).
    pub fn automation_allowed(&self) -> bool {
        self.automation_allowed
    }
}

/// `lookup(Notify, name)` for the client (`client_lookup`, RFC 0001 section
/// 10): the ID of a notification name some simulation module of this match
/// declares in its `vera.notify` (ASCII case-insensitive), `None` for a name
/// none declares, so a client cannot match an undeclared name by its hash.
pub fn notify_id(sim: &Simulation, name: &str) -> Option<i32> {
    sim.ext_modules.slots.iter().find_map(|slot| {
        slot.module
            .notify_names()
            .iter()
            .find(|(declared, _)| declared.eq_ignore_ascii_case(name))
            .map(|(_, id)| *id)
    })
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
pub(crate) const fn type_ref(kind: i32, id: u32) -> i64 {
    (REF_TYPE << REF_KIND_SHIFT) | ((kind as i64) << 32) | id as i64
}

/// A cell reference, in cells.
pub(crate) const fn cell_ref(x: u16, y: u16) -> i64 {
    (REF_CELL << REF_KIND_SHIFT) | ((x as i64) << 16) | y as i64
}

/// What a nonzero reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ref {
    Object(u64),
    House(usize),
    /// House `house`'s Super of `[SuperWeaponTypes]` entry `index`.
    SuperWeapon {
        house: usize,
        index: usize,
    },
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
pub(crate) fn decode_ref(reference: i64) -> Result<Option<Ref>, Invalid> {
    if reference == 0 {
        return Ok(None);
    }
    if !(0..REF_LIMIT).contains(&reference) {
        return Err(Invalid(format!("{reference:#x} is not a reference")));
    }
    let payload = reference & ((1 << REF_KIND_SHIFT) - 1);
    Ok(Some(match reference >> REF_KIND_SHIFT {
        0 if reference & HOUSE_REF_BIT != 0 => Ref::House((payload & !HOUSE_REF_BIT) as usize),
        0 if reference & SUPER_REF_BIT != 0 => Ref::SuperWeapon {
            house: ((payload & !SUPER_REF_BIT) >> 16) as usize,
            index: (payload & 0xFFFF) as usize,
        },
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

/// A client module's cell argument (RFC 10; appendix "References"): a
/// cell, or a waypoint's cell (`NotFound` for a waypoint the map does not
/// set). Any other reference is invalid.
pub fn client_cell(
    sim: &Simulation,
    reference: i64,
) -> Result<Result<(u16, u16), Status>, Invalid> {
    match decode_ref(reference)? {
        Some(Ref::Cell { x, y }) => Ok(Ok((x, y))),
        Some(Ref::Waypoint(number)) => {
            Ok(orders::waypoint_cell(sim, number).ok_or(Status::NotFound))
        }
        _ => Err(Invalid(format!("{reference:#x} is not a cell"))),
    }
}

/// A client module's object argument: the object ID. Any other reference
/// (0 included) is invalid.
pub fn client_object(reference: i64) -> Result<u64, Invalid> {
    match decode_ref(reference)? {
        Some(Ref::Object(id)) => Ok(id),
        _ => Err(Invalid(format!("{reference:#x} is not an object"))),
    }
}

/// A cell's reference, for a client read (the camera's `Cell`).
pub const fn client_cell_ref(x: u16, y: u16) -> i64 {
    cell_ref(x, y)
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

/// The host a module sees during one call: the simulation, whose slot list
/// lacks the calling module's slot for the call, and that module's own state.
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

impl Host<'_, '_> {
    /// What this call's reads (`get`, `find`, `var_get`, filter clauses)
    /// go through: the simulation, read-only, with no client gate.
    fn reader(&self) -> Reader<'_> {
        Reader {
            sim: &*self.sim,
            rules: self.rules,
            overlay_registry: self.overlay_registry,
            gate: None,
        }
    }
}

/// The one reader of module-visible state (appendix 7, 8.2): property
/// reads, variable reads and `find`, over a simulation it never writes. The
/// module host reads through it ([`Host::reader`]); client modules read
/// through it too (`app::ui_modules::reads`), behind their visibility gate,
/// on whatever runtime the app reads (possibly the presentation copy).
#[derive(Clone, Copy)]
pub(crate) struct Reader<'a> {
    pub(crate) sim: &'a Simulation,
    pub(crate) rules: &'a RuleSet,
    /// For `Placeable` (the placement owner's overlay test).
    pub(crate) overlay_registry: Option<&'a OverlayTypeRegistry>,
    /// A client's view (RFC 10, "Client reads"); `None` in the simulation.
    /// With one, `find` lists only the candidates the gate admits and reads
    /// every clause through it.
    pub(crate) gate: Option<&'a dyn ReadGate>,
}

/// What a client may see of a `find` (RFC 10, "Client reads"; appendix 8.2
/// "client" column): which candidates exist for it, and how its clauses
/// read. Candidates it does not list are passed over like keys without a
/// candidate ([`FindPage::skip`]), so a page says nothing about them.
pub(crate) trait ReadGate {
    /// Whether `find` may list `reference`: an object, a cell or a
    /// superweapon reference.
    fn lists(&self, reference: i64) -> bool;
    /// A clause's reading of `prop` on `reference`, for the query's `view`.
    fn clause_read(&self, reference: i64, prop: i32, view: View) -> Result<(i64, Status), Invalid>;
    /// A clause's reading of variable `var` in `scope`.
    fn clause_var(&self, scope: i64, var: i32) -> Result<(i64, Status), Invalid>;
}

/// `has_kind`, the same in every call (the rules stage included); client
/// modules answer the simulation's tables with it too.
pub(crate) fn has_kind(rules: &RuleSet, table: i32, id: i32) -> bool {
    match table {
        vera20k_wasm_ext::triggers::TABLE_EVENT => {
            events::raises(rules, id)
                || vera20k_wasm_ext::decide::DECISION_ONLY_EVENTS.contains(&id)
                || matches!(
                    id,
                    vera20k_wasm_ext::triggers::EVENT_INI_ENTRY
                        | vera20k_wasm_ext::triggers::EVENT_INI_KEY
                )
        }
        TABLE_ACTION => actions::has_action(id, rules),
        TABLE_PROPERTY => property_name(id)
            .is_some_and(|name| READABLE.contains(&name) || Stat::wired_property(name).is_some()),
        TABLE_NAME_KIND => matches!(
            id,
            NAME_TECHNO_TYPE
                | NAME_WEAPON
                | NAME_WARHEAD
                | NAME_PROJECTILE
                | NAME_ANIM
                | NAME_VOXEL_ANIM
                | NAME_PARTICLE_SYSTEM
                | NAME_SUPER_WEAPON
                | NAME_COUNTRY
                | NAME_TEAM_TYPE
                | NAME_VARIABLE
                | NAME_PROPERTY
                | NAME_ACTION_KIND
                | NAME_STRING
                | NAME_MODULE_EVENT
                | NAME_NOTIFY
        ),
        TABLE_SCOPE => matches!(
            id,
            SCOPE_OBJECTS
                | SCOPE_HOUSES
                | SCOPE_CELLS
                | SCOPE_TYPES
                | SCOPE_MODIFIERS
                | SCOPE_SUPER_WEAPONS
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
        NAME_PROJECTILE => rules.projectile_index(name).map(|i| i as i32),
        NAME_SUPER_WEAPON => rules
            .super_weapon_index(name)
            .and_then(|i| i32::try_from(i).ok()),
        // The AnimType array position stock action 41 and `SpawnAnim` take;
        // the first of a repeated name, as the native name search finds it.
        NAME_ANIM => (0..)
            .map_while(|i| rules.anim_type_name_by_index(i).map(|anim| (i, anim)))
            .find(|(_, anim)| anim.eq_ignore_ascii_case(name))
            .map(|(i, _)| i),
        NAME_VOXEL_ANIM => rules
            .voxel_anim_type_id_by_name(name)
            .and_then(|id| i32::try_from(id.0).ok()),
        NAME_PARTICLE_SYSTEM => rules
            .ps_type_id_by_name(name)
            .and_then(|id| i32::try_from(id.0).ok()),
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
        NAME_PROJECTILE => u32::try_from(id)
            .ok()
            .and_then(|id| rules.projectile_at(id))
            .map(|projectile| projectile.id.as_str()),
        NAME_SUPER_WEAPON => usize::try_from(id)
            .ok()
            .and_then(|i| rules.super_weapon_order.get(i))
            .map(String::as_str),
        NAME_ANIM => rules.anim_type_name_by_index(id),
        NAME_VOXEL_ANIM => usize::try_from(id)
            .ok()
            .and_then(|i| rules.voxel_anim_types_iter().nth(i))
            .map(|anim| anim.name.as_str()),
        NAME_PARTICLE_SYSTEM => rules
            .particle_system_type_id_by_index(id)
            .map(|id| rules.particle_system_type(id).name.as_str()),
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
/// `name_of` for name kinds `String` and `ModuleEvent` ([`ini_names`]).
fn ini_name_of(ini: &ini_names::IniNames, kind: i32, id: i32) -> (String, Status) {
    let name = match kind {
        NAME_STRING => ini.string(id),
        _ => ini.event(id).map(|r| r.name.as_str()),
    };
    name.map_or((String::new(), Status::NotFound), |name| {
        (name.to_owned(), Status::Ok)
    })
}

fn variable_name(vars: &VarStore, id: i32) -> Result<(String, Status), Invalid> {
    if id < FIRST_DECLARED_VAR {
        return Ok((String::new(), Status::NotFound));
    }
    name_answer(vars.name(id), NAME_VARIABLE, id)
}

/// TechnoType `id`: an index into the rules' type registry.
pub(crate) fn techno_type(
    rules: &RuleSet,
    id: i32,
) -> Option<&crate::rules::object_type::ObjectType> {
    let id = usize::try_from(id).ok()?;
    rules.all_objects().nth(id)
}

/// `lookup` of the name kinds the simulation answers, for a module of
/// `source` (whose variables a name without a `mod.`/`map.` prefix finds).
/// The module host and client modules (`client_lookup`) both answer with it;
/// `Property`, `ActionKind` and `ModuleAction` are the sandbox's.
pub(crate) fn lookup_name(
    sim: &Simulation,
    rules: &RuleSet,
    source: ModuleSource,
    kind: i32,
    name: &str,
) -> (i32, Status) {
    let id = match kind {
        NAME_TEAM_TYPE => sim
            .stock_definition(crate::sim::trigger_runtime::StockReferenceKind::Team, name)
            .map(|index| index as i32),
        NAME_VARIABLE => sim.ext_modules.vars.lookup(source, name),
        NAME_STRING => sim.ext_modules.ini.string_id(name),
        NAME_MODULE_EVENT => sim.ext_modules.ini.event_id(name),
        _ => match rules_name_id(rules, kind, name) {
            Some(id) => id,
            None => return (-1, Status::Unsupported),
        },
    };
    id.map_or((-1, Status::NotFound), |id| (id, Status::Ok))
}

/// `name_of` of the name kinds the simulation answers ([`lookup_name`]'s).
pub(crate) fn name_of_id(
    sim: &Simulation,
    rules: &RuleSet,
    kind: i32,
    id: i32,
) -> Result<(String, Status), Invalid> {
    let name = match kind {
        NAME_VARIABLE => return variable_name(&sim.ext_modules.vars, id),
        NAME_STRING | NAME_MODULE_EVENT => {
            return Ok(ini_name_of(&sim.ext_modules.ini, kind, id));
        }
        NAME_TEAM_TYPE => usize::try_from(id)
            .ok()
            .and_then(|id| sim.stock_team_type_name(id)),
        _ => match rules_id_name(rules, kind, id) {
            Some(name) => name,
            None => return Ok((String::new(), Status::Unsupported)),
        },
    };
    name_answer(name, kind, id)
}

impl SimHost for Host<'_, '_> {
    fn frame(&mut self) -> i32 {
        self.sim.session.binary_frame() as i32
    }

    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        self.rng.next_range_i32_inclusive(lo, hi)
    }

    /// Appendix 1.1 `log`: a line in the in-game log. It goes to the game's
    /// log file and, [`MAX_SHOWN_LOG_LINES`] per frame and cut to
    /// [`SHOWN_LOG_CHARS`], to the players' message list.
    fn log(&mut self, line: &str) {
        log::info!(target: "ext_module", "{:?}: {line}", self.source);
        self.sim.ext_modules.keep_log(line.to_owned());
        let frame = self.sim.session.binary_frame();
        let shown = &mut self.sim.ext_modules.shown_log;
        if shown.0 != frame {
            *shown = (frame, 0);
        }
        if shown.1 < MAX_SHOWN_LOG_LINES {
            shown.1 += 1;
            let text = line.chars().take(SHOWN_LOG_CHARS).collect();
            self.sim.publish_trigger_effects([
                crate::sim::trigger_runtime::TriggerEffect::ExtModuleNotice { text },
            ]);
        }
    }

    fn has_kind(&mut self, table: i32, id: i32) -> bool {
        has_kind(self.rules, table, id)
    }

    fn lookup(&mut self, kind: i32, name: &str) -> Result<(i32, Status), Invalid> {
        Ok(lookup_name(self.sim, self.rules, self.source, kind, name))
    }

    fn name_of(&mut self, kind: i32, id: i32) -> Result<(String, Status), Invalid> {
        name_of_id(self.sim, self.rules, kind, id)
    }

    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        if let Some(answer) = self.trigger_property(reference, prop, None) {
            return Ok(answer);
        }
        // A string property's text gets its string ID here, added when new.
        let name = property_name(prop).unwrap_or_default();
        if let Some(decoded) = decode_ref(reference)?
            && let Some(text) = self.reader().property_text(decoded, name)?
        {
            return Ok(match text {
                Ok(text) => (i64::from(self.sim.ext_modules.ini.intern(&text)), Status::Ok),
                Err(status) => (0, status),
            });
        }
        self.reader().read(reference, prop)
    }

    fn set(&mut self, reference: i64, prop: i32, value: i64) -> Result<Status, Invalid> {
        if let Some((_, status)) = self.trigger_property(reference, prop, Some(value)) {
            return Ok(status);
        }
        let name = property_name(prop).unwrap_or_default();
        if name == "ControlDomains"
            && let Some(Ref::House(index)) = decode_ref(reference)?
        {
            return self.write_control_domains(index, value);
        }
        if let Some(Ref::SuperWeapon { house, index }) = decode_ref(reference)? {
            return self.write_super_weapon(house, index, name, value);
        }
        if let Some(Ref::Object(id)) = decode_ref(reference)?
            && self.sim.entities().get(id).is_none()
            && self.sim.projectiles.get(id).is_some()
        {
            return Ok(self.write_projectile_property(id, name, value));
        }
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
                "DoorOpen" => return self.write_door(id, value),
                "ImageOverride" => return self.write_image_override(id, value),
                _ => {}
            }
        }
        let (_, status) = self.reader().read(reference, prop)?;
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
        let Some(stat) = Stat::wired_property(name) else {
            // Appendix 7.2's other properties are not wired (in this
            // build); the rest are not modifiable.
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
        let op = ModifierOp::from_abi(op);
        // A mask (`ImmunityFlags`) takes only an override of its bits.
        if !stat.admits(op, value) {
            return Ok((0, Status::Refused));
        }
        let frame = self.sim.session.binary_frame();
        let modifier = Modifier {
            target,
            stat,
            op,
            value,
            expires: u32::try_from(frames)
                .ok()
                .map(|frames| frame.saturating_add(frames)),
            source,
            group,
            origin: self.source,
        };
        let sim = &mut *self.sim;
        let before = sim.ext_modules.modifiers.group_count(target, group);
        let handle = sim.substrate.allocate_stable_id();
        let Some(replaced) =
            sim.ext_modules
                .modifiers
                .insert(&mut sim.substrate.entities, handle, modifier)
        else {
            return Ok((0, Status::Refused));
        };
        sim.raise_modifiers_expired(self.rules, self.frame_effects, &replaced);
        sim.raise_modifier_started(self.rules, self.frame_effects, handle, before);
        Ok((handle as i64, Status::Ok))
    }

    fn unmodify(&mut self, modifier: i64) -> Result<Status, Invalid> {
        let handle = match decode_ref(modifier)? {
            None => return Ok(Status::Gone),
            Some(Ref::Object(handle)) => handle,
            Some(_) => return Err(Invalid(format!("{modifier:#x} is not a modifier"))),
        };
        let sim = &mut *self.sim;
        let Some(removed) = sim
            .ext_modules
            .modifiers
            .remove(&mut sim.substrate.entities, handle)
        else {
            return Ok(Status::Gone);
        };
        sim.raise_modifiers_expired(self.rules, self.frame_effects, &[(handle, removed)]);
        Ok(Status::Ok)
    }

    fn find(&mut self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        self.reader().find(query, cap)
    }

    fn var_get(&mut self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        self.reader().read_var(scope, var)
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

    fn ini_register(
        &mut self,
        kind: i32,
        section: i32,
        name: &str,
        key: i32,
    ) -> Result<(i32, Status), Invalid> {
        self.sim
            .ext_modules
            .ini
            .register(self.source, kind, section, name, key)
    }

    fn order(&mut self, house: i64, order: &Order) -> Result<Status, Invalid> {
        self.queue_order(house, order)
    }

    /// `send_notify` (RFC 0001 section 10): `to` is a house reference or 0
    /// for the observers; a house that does not exist is `Gone`. Past
    /// [`MAX_NOTIFIES_PER_FRAME`] this frame `Refused`. The notification
    /// goes to the client outbox, never into simulation state.
    fn send_notify(&mut self, to: i64, id: i32, data: &[u8]) -> Result<Status, Invalid> {
        if to != 0 && self.reader().house(to)?.is_none() {
            return Ok(Status::Gone);
        }
        let frame = self.sim.session.binary_frame();
        let source = self.source as usize;
        let count = &mut self.sim.ext_modules.notify_count[source];
        if count.0 != frame {
            *count = (frame, 0);
        }
        if count.1 >= MAX_NOTIFIES_PER_FRAME {
            return Ok(Status::Refused);
        }
        count.1 += 1;
        self.sim
            .ext_modules
            .client_outbox
            .push_notify(ClientNotify {
                to,
                source: self.source.into(),
                id,
                data: data.to_vec(),
            });
        Ok(Status::Ok)
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
    // Objects (props.rs).
    "Layer",
    "Target",
    "Experience",
    "Storage",
    "Transport",
    "PassengerCount",
    "TransportSpaceLeft",
    "Team",
    "Tag",
    "DisguiseType",
    "DisguiseOwner",
    "Moving",
    "Facing",
    "TurretFacing",
    "Armor",
    // Objects for driving a house (props.rs, RFC 0001 6i).
    "Destination",
    "WeaponReady",
    "Deployed",
    "UnderConstruction",
    "Parasite",
    // Objects past the first set (props.rs).
    "Cloaked",
    "NearbyEnemyCount",
    "TargetInRange",
    "CanFireAtTarget",
    "DoorOpen",
    "Animating",
    "ImageOverride",
    // Projectiles (projectiles.rs, appendix 7.4).
    "VX",
    "VY",
    "VZ",
    "Source",
    "Data0",
    "Data1",
    // Viewpoint properties: in filter clauses and queries only.
    "Relation",
    "Revealed",
    "Placeable",
    "Visible",
    "Engage",
    "Buildable",
    // Strings (props.rs): a type's name, the multiplayer mode.
    "Name",
    "GameMode",
    // Houses.
    "TechLevel",
    "Country",
    "ControlDomains",
    "Factory.Building.Type",
    "Factory.Building.Progress",
    "Factory.Building.Ready",
    "Factory.Building.Queued",
    "Factory.Defense.Type",
    "Factory.Defense.Progress",
    "Factory.Defense.Ready",
    "Factory.Defense.Queued",
    "Factory.Infantry.Type",
    "Factory.Infantry.Progress",
    "Factory.Infantry.Ready",
    "Factory.Infantry.Queued",
    "Factory.Vehicle.Type",
    "Factory.Vehicle.Progress",
    "Factory.Vehicle.Ready",
    "Factory.Vehicle.Queued",
    "Factory.Naval.Type",
    "Factory.Naval.Progress",
    "Factory.Naval.Ready",
    "Factory.Naval.Queued",
    "Factory.Aircraft.Type",
    "Factory.Aircraft.Progress",
    "Factory.Aircraft.Ready",
    "Factory.Aircraft.Queued",
    // Superweapons (supers.rs).
    "Ready",
    "ChargePercent",
    // Types and rules (props.rs).
    "Soylent",
    "Speed",
    "ROT",
    "Crushable",
    "Selfhealing",
    "Explodes",
    "Power",
    "Sight",
    "InitialAmmo",
    "Passengers",
    "SizeLimit",
    "Points",
    "Damage",
    "ROF",
    "Burst",
    "MinimumRange",
    "Range",
    "CellSpread",
    "PercentAtMax",
    "Wall",
    "Wood",
    "Sparky",
    "Fire",
    "Conventional",
    "Rocker",
    "InfDeath",
    // Globals (cells.rs).
    "Difficulty",
    "MissionTimer",
    "MissionTimerRunning",
    "AmbientLight",
    "IonStormActive",
    "AutomationAllowed",
    // Cells (cells.rs): Zone.<MovementZone>, Obstructed, Passable.<SpeedType>.
    "Zone.Normal",
    "Zone.Crusher",
    "Zone.Destroyer",
    "Zone.AmphibiousDestroyer",
    "Zone.AmphibiousCrusher",
    "Zone.Amphibious",
    "Zone.Subterrannean",
    "Zone.Infantry",
    "Zone.InfantryDestroyer",
    "Zone.Fly",
    "Zone.Water",
    "Zone.WaterBeach",
    "Zone.CrusherAll",
    "Obstructed",
    "Passable.Foot",
    "Passable.Track",
    "Passable.Wheel",
    "Passable.Hover",
    "Passable.Winged",
    "Passable.Float",
    "Passable.Amphibious",
    "Passable.FloatBeach",
];

/// Appendix 7.2's modifiable properties: `PROPERTIES` from the first to the
/// last name.
const MODIFIABLE: (&str, &str) = ("ArmorMultiplier", "PowerOutage");

fn property_id_range((first, last): (&str, &str)) -> std::ops::RangeInclusive<i32> {
    let id = |name| vera20k_wasm_ext::property_id(name).unwrap_or(-1);
    id(first)..=id(last)
}

impl Reader<'_> {
    /// `find`, by scope (appendix 8.2).
    pub(crate) fn find(&self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        match query.scope {
            SCOPE_OBJECTS => self.find_objects(query, cap),
            SCOPE_HOUSES => self.find_houses(query, cap),
            SCOPE_CELLS => self.find_cells(query, cap),
            SCOPE_TYPES => self.find_types(query, cap),
            SCOPE_MODIFIERS => self.find_modifiers(query, cap),
            SCOPE_SUPER_WEAPONS => self.find_super_weapons(query, cap),
            _ => Ok((FindPage::default(), Status::Unsupported)),
        }
    }

    /// A house reference's house; `None` for a house that does not exist.
    fn house(&self, reference: i64) -> Result<Option<crate::sim::intern::InternedId>, Invalid> {
        if reference & HOUSE_REF_BIT == 0 || reference >> 48 != 0 {
            return Err(Invalid(format!("{reference:#x} is not a house reference")));
        }
        let index = (reference & !HOUSE_REF_BIT) as usize;
        Ok(self.sim.houses.order().get(index).copied())
    }

    /// A property, as `get` reads it: without a viewpoint.
    pub(crate) fn read(&self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        self.read_in(reference, prop, View::default())
    }

    /// A property read for `view` (appendix 7, "Viewpoint"): a filter
    /// clause reads for the trigger house, a query for its `viewpoint` (a
    /// house, or an object: `Engage` reads for it, the house properties for
    /// its owner). A direct `get` has none, and the viewpoint properties
    /// answer `Unsupported` there.
    pub(crate) fn read_in(
        &self,
        reference: i64,
        prop: i32,
        view: View,
    ) -> Result<(i64, Status), Invalid> {
        let name = property_name(prop).unwrap_or_default();
        let Some(reference) = decode_ref(reference)? else {
            return Ok((0, Status::Gone));
        };
        // A string property outside `get` adds no string: a text not yet
        // held has no ID to match.
        if let Some(text) = self.property_text(reference, name)? {
            return Ok(match text {
                Ok(text) if text.is_empty() => (0, Status::Ok),
                Ok(text) => self
                    .sim
                    .ext_modules
                    .ini
                    .string_id(&text)
                    .map_or((0, Status::NotFound), |id| (i64::from(id), Status::Ok)),
                Err(status) => (0, status),
            });
        }
        Ok(match reference {
            Ref::Object(id) => match self.sim.entities().get(id) {
                Some(entity) => match name {
                    "Relation" => self.relation(entity.owner(), view),
                    "Visible" => self.visible(entity, view),
                    "Engage" => self.engage(id, view),
                    _ => self.object_property(entity, name),
                },
                None if self.sim.projectiles.get(id).is_some() => {
                    self.projectile_property(id, name)
                }
                None => match self.sim.ext_modules.modifiers.get(id) {
                    Some(modifier) => modifier
                        .property(name, self.sim.session.binary_frame())
                        .map_or((0, Status::Unsupported), |value| (value, Status::Ok)),
                    None => (0, Status::Gone),
                },
            },
            Ref::House(index) if name == "Relation" => {
                match self.sim.houses.order().get(index).copied() {
                    Some(house) => self.relation(house, view),
                    None => (0, Status::Gone),
                }
            }
            Ref::House(index) => self.house_property(index, name),
            Ref::SuperWeapon { house, index } => self.super_weapon_property(house, index, name),
            Ref::Type {
                kind: NAME_TECHNO_TYPE,
                id,
            } if name == "Buildable" => self.buildable(id, view)?,
            Ref::Type { kind, id } => type_property(self.rules, kind, id, name)?,
            Ref::Cell { x, y } => self.cell_property(x, y, name, view),
            Ref::Waypoint(number) => match self.waypoint_cell(number) {
                Some((x, y)) => self.cell_property(x, y, name, view),
                None => (0, Status::NotFound),
            },
            Ref::Global => self.global_property(name),
        })
    }

    /// A variable, as `var_get` reads it.
    pub(crate) fn read_var(&self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
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
            "ImmunityFlags" => match object() {
                Some(object) => entity.immunity_flags(object, self.rules),
                None => return (0, Status::Unsupported),
            },
            _ => match Stat::wired_property(name) {
                Some(stat) => entity.effective_stat(stat),
                None => return self.more_object_property(entity, name),
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
            "TechLevel" => i64::from(house.tech_level()),
            "ControlDomains" => self.sim.ext_modules.claims.mask(index),
            "Country" => match self
                .sim
                .interner
                .try_resolve(house.house_type_id())
                .and_then(|name| self.rules.country_index(name))
            {
                Some(country) => type_ref(NAME_COUNTRY, u32::from(country.0)),
                None => return (0, Status::NotFound),
            },
            _ => return self.factory_property(id, name),
        };
        (value, Status::Ok)
    }

    /// Whether `candidate` passes every clause; `None` when a clause needs
    /// what is not kept yet. Clauses read like `get`, for the query's
    /// viewpoint; a reading that is not `Ok` fails its clause. A client
    /// reads them through its gate.
    fn passes(&self, candidate: i64, query: &Query) -> Result<Option<bool>, Invalid> {
        let view = if query.clauses.is_empty() {
            View::default()
        } else {
            self.query_view(query)?
        };
        for clause in &query.clauses {
            let (reading, status) = match (clause.subject, self.gate) {
                (ClauseSubject::Property(prop), None) => self.read_in(candidate, prop, view)?,
                (ClauseSubject::Property(prop), Some(gate)) => {
                    gate.clause_read(candidate, prop, view)?
                }
                (ClauseSubject::Var { var, subject }, gate) => {
                    let scope = if subject { candidate } else { 0 };
                    match gate {
                        None => self.read_var(scope, var)?,
                        Some(gate) => gate.clause_var(scope, var)?,
                    }
                }
            };
            if status != Status::Ok || !clause.op.holds(reading, clause.value) {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    }

    /// `find` over objects, in creation (stable ID) order. An owner's come
    /// from the store's owner and category index; with a type, only from the
    /// categories that have a type of that name.
    fn find_objects(&self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        let wanted = match decode_ref(query.type_ref)? {
            None => None,
            Some(reference) => {
                let found = match reference {
                    Ref::Type {
                        kind: NAME_TECHNO_TYPE,
                        id,
                    } => i32::try_from(id)
                        .ok()
                        .and_then(|signed| self.techno_type(signed))
                        .map(|object| (id, object)),
                    _ => None,
                };
                Some(found.ok_or_else(|| {
                    Invalid(format!(
                        "{:#x} is not a TechnoType for an Objects query",
                        query.type_ref
                    ))
                })?)
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
        let store = self.sim.entities();
        let mut owned: [&[u64]; 4] = [&[]; 4];
        if let Some(owner) = owner {
            let categories = [
                EntityCategory::Unit,
                EntityCategory::Infantry,
                EntityCategory::Structure,
                EntityCategory::Aircraft,
            ];
            for (list, category) in owned.iter_mut().zip(categories) {
                if wanted.is_none_or(|(_, object)| {
                    self.rules
                        .type_handle_in_category(category.into(), &object.id)
                        .is_some()
                }) {
                    let ids = store.owned_ids(owner, category);
                    *list = &ids[ids.partition_point(|&id| id < after)..];
                }
            }
        }
        let mut owned = merge_ascending(owned).filter_map(|id| Some((id, store.get(id)?)));
        let mut all = store.iter_sorted_from(after);
        let candidates: &mut dyn Iterator<Item = (u64, &GameEntity)> = match owner {
            Some(_) => &mut owned,
            None => &mut all,
        };
        let mut page = FindPage::default();
        for (id, entity) in candidates {
            if page.found.len() == cap {
                break;
            }
            // What a client cannot see is no candidate for it.
            if self.gate.is_some_and(|gate| !gate.lists(id as i64)) {
                if !page.skip(id as i64) {
                    break;
                }
                continue;
            }
            if !page.examine(id as i64, query) {
                break;
            }
            if centre.is_some_and(|(cx, cy)| !in_area(entity, query.area, cx, cy))
                || wanted.is_some_and(|(wanted, _)| {
                    self.sim
                        .interner
                        .type_handle(entity.type_ref(), self.rules)
                        .is_none_or(|handle| handle.0 != wanted)
                })
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
            if page.found.len() == cap || !page.examine(handle as i64, query) {
                break;
            }
            let reference = handle as i64;
            match self.passes(reference, query)? {
                Some(true) => page.found.push((reference, reference)),
                Some(false) => {}
                None => return Ok((FindPage::default(), Status::Unsupported)),
            }
        }
        Ok((page, Status::Ok))
    }
}

impl Host<'_, '_> {
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
        let Some(house) = self.reader().house(value)? else {
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

    /// `DoorOpen` (appendix 7.1): 1 draws a war factory's or gate's door
    /// open and 0 shut until -1 lets it show its own state; the door and
    /// what it lets through run on (`held::ActionHeld`). Other objects
    /// answer `Unsupported`.
    fn write_door(&mut self, id: u64, value: i64) -> Result<Status, Invalid> {
        let Some(entity) = self.sim.entities().get(id) else {
            return Ok(Status::Gone);
        };
        let drawn = entity.category() == crate::map::entities::EntityCategory::Structure
            && self
                .sim
                .entity_object_type(entity, self.rules)
                .is_some_and(|object| object.weapons_factory || object.gate);
        if !drawn {
            return Ok(Status::Unsupported);
        }
        let open = match value {
            -1 => None,
            0 => Some(false),
            1 => Some(true),
            _ => return Err(Invalid(format!("DoorOpen {value}"))),
        };
        self.sim.ext_modules.held.hold_door(id, open);
        Ok(Status::Ok)
    }

    /// `ImageOverride` (appendix 7.1): 0 draws the object as itself; a
    /// TechnoType of its own category draws it as that type. A building,
    /// another category or a non-TechnoType answers `Refused`.
    fn write_image_override(&mut self, id: u64, value: i64) -> Result<Status, Invalid> {
        let image = match decode_ref(value)? {
            None => None,
            Some(Ref::Type { kind, id: type_id }) if kind == NAME_TECHNO_TYPE => {
                let Some(image) = i32::try_from(type_id)
                    .ok()
                    .and_then(|type_id| techno_type(self.rules, type_id))
                else {
                    return Ok(Status::NotFound);
                };
                Some(image)
            }
            Some(_) => return Ok(Status::Refused),
        };
        let Some(entity) = self.sim.entities().get(id) else {
            return Ok(Status::Gone);
        };
        let own = self.sim.entity_object_type(entity, self.rules);
        let fits = image.is_none_or(|image| {
            own.is_some_and(|own| {
                own.category == image.category
                    && own.category != crate::rules::object_type::ObjectCategory::Building
            })
        });
        if !fits {
            return Ok(Status::Refused);
        }
        let image = image.map(|image| image.id.clone());
        // The draw finds the type by its interned name.
        if let Some(image) = image.as_deref() {
            self.sim.interner.intern(image);
        }
        self.sim.ext_modules.held.set_image(id, image);
        // The atlas checks the objects whose look changed.
        self.sim.substrate.entities.note_look(id);
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
}

impl Reader<'_> {
    /// `find` over houses, in house array order. Houses have no place or
    /// type: a query with an area, type or owner answers `Unsupported`.
    fn find_houses(&self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        if query.area != Area::Map || query.type_ref != 0 || query.owner != 0 {
            return Ok((FindPage::default(), Status::Unsupported));
        }
        let mut page = FindPage::default();
        let start = usize::try_from(query.after).unwrap_or(usize::MAX);
        for index in start..self.sim.houses.order().len() {
            if page.found.len() == cap || !page.examine(index as i64, query) {
                break;
            }
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
            let changed = self.ext_modules.vars.get(var, holder) != value;
            self.ext_modules.vars.set(var, holder, value);
            // A change resets the `VariablePoll` timers waiting on it, as a
            // stock variable's change resets Tag timers (appendix 4.4).
            if changed
                && self
                    .trigger_runtime
                    .module_listens(vera20k_wasm_ext::triggers::EVENT_VARIABLE_POLL)
            {
                let frame = self.session.binary_frame();
                self.trigger_runtime
                    .module_triggers_mut()
                    .reset_variable_polls(var, frame);
            }
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
        for slot in &self.ext_modules.slots {
            let (first, second) = match slot.source < source {
                true => (&slot.module, &module),
                false => (&module, &slot.module),
            };
            check_notify_collision(first, second)?;
        }
        let position = self.insert_ext_slot(source, module, FRAME_BUDGET, SEAT_BUDGET, 0);
        self.trigger_runtime.set_client_forwarding(true);
        self.init_ext_slot(rules, position)
    }

    /// Puts a loaded module in its run-order slot and takes its variable
    /// declarations; returns the slot index.
    fn insert_ext_slot(
        &mut self,
        source: ModuleSource,
        module: SimModule,
        frame_budget: u64,
        seat_budget: u64,
        memory_seats: usize,
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
                base_budget: frame_budget,
                seat_budget,
                memory_seats,
                budget_frame: 0,
                frame_ops_used: 0,
                decide_ops_used: 0,
                defeat_ops_used: 0,
                orders: Vec::new(),
                // Each page is digested when its turn comes.
                digests: PageDigests::default(),
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
    /// rules text `rules_get` reads. Then the mod's INI triggers are built
    /// ([`ini_triggers`]). An error names the module or the INI section; the
    /// match must not start.
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
            notices,
            ini_triggers,
            ini,
        } = prepared;
        // Names the rules stage's `vera_init` calls registered; the other
        // modules register below.
        self.ext_modules.ini = ini;
        self.ext_modules.log.extend(log);
        for text in notices {
            log::warn!(target: "ext_module", "{text}");
            self.ext_modules.keep_log(text.clone());
            self.publish_trigger_effects([
                crate::sim::trigger_runtime::TriggerEffect::ExtModuleNotice { text },
            ]);
        }
        self.ext_modules.controller = policy.controller;
        self.ext_modules.automation_allowed = policy.automation_allowed;
        if !modules.is_empty() {
            self.ext_modules.rules_ini = Some(rules_ini);
            // Client modules hear the forwarded events ([`client_outbox`]).
            self.trigger_runtime.set_client_forwarding(true);
        }
        let mut uninitialized = Vec::new();
        for prepared in modules {
            let source = prepared.source;
            self.insert_ext_slot(
                source,
                prepared.module,
                policy.budget_of(source),
                policy.seat_budget,
                prepared.memory_seats,
            );
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
        // The mod's INI triggers, once their modules can answer names.
        self.install_ini_triggers(rules, &ini_triggers)
            .map_err(|e| format!("the mod's rulesmd.ini: {e}"))
    }

    /// Instantiates the modules a save carried and puts back their state.
    /// An error means this game cannot continue the save.
    pub(crate) fn restore_ext_modules(&mut self) -> Result<(), String> {
        for saved in std::mem::take(&mut self.ext_modules.pending) {
            let limits = saved.source.limits().with_seats(saved.memory_seats);
            let module = SimModule::restore(&saved.bytes, limits, &saved.state)
                .map_err(|e| format!("{:?} module: {e}", saved.source))?;
            self.ext_modules.slots.push(Slot {
                source: saved.source,
                module,
                rng: saved.rng,
                started: saved.started,
                disabled: saved.disabled,
                last_frame_ops: saved.last_frame_ops,
                frame_budget: saved.frame_budget.clamp(1, FRAME_BUDGET),
                base_budget: saved.frame_budget.clamp(1, FRAME_BUDGET),
                seat_budget: saved.seat_budget.min(SEAT_BUDGET),
                memory_seats: saved.memory_seats,
                budget_frame: 0,
                frame_ops_used: 0,
                decide_ops_used: 0,
                defeat_ops_used: 0,
                orders: saved.orders,
                // As saved, also none before the first module phase: what
                // the running game has.
                digests: saved.digests,
            });
        }
        self.ext_modules.declare_vars();
        self.ext_modules
            .modifiers
            .refresh_all(&mut self.substrate.entities);
        if !self.ext_modules.slots.is_empty() {
            self.trigger_runtime.set_client_forwarding(true);
        }
        Ok(())
    }

    /// Drops what modules hold on an object that leaves the game: its
    /// modifiers and object-scope variables.
    pub(crate) fn forget_ext_module_object(&mut self, stable_id: u64) {
        if !self.ext_modules.modifiers.is_empty() {
            self.ext_modules.modifiers.forget_object(stable_id);
        }
        self.ext_modules.vars.forget_object(stable_id);
        self.ext_modules.held.forget_object(stable_id);
        self.ext_modules.projectiles.forget(stable_id);
        self.ext_modules.motions.end_push(stable_id);
    }

    pub fn ext_modules(&self) -> &ExtModules {
        &self.ext_modules
    }

    pub fn ext_modules_mut(&mut self) -> &mut ExtModules {
        &mut self.ext_modules
    }

    /// The module phase: modifiers whose time is up expire, timed
    /// conversions whose time is up revert, kept animations whose time is
    /// up or whose object went are destroyed, and `Timer` triggers whose
    /// period ends fire, then each enabled module in order
    /// gets its `on_scenario_start` once, its client messages
    /// (`on_message`, [`messages`]), its queued event records
    /// (`on_events`), then `on_frame` (RFC section 5).
    pub(crate) fn run_ext_module_phase(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame();
        // Each claimed seat adds its budget for this frame (RFC 15.4, 6i).
        for index in 0..self.ext_modules.slots.len() {
            let source = self.ext_modules.slots[index].source;
            let seats = self.ext_modules.claimed_seats(source) as u64;
            let slot = &mut self.ext_modules.slots[index];
            slot.frame_budget = slot.base_budget + seats * slot.seat_budget;
        }
        if !self.ext_modules.modifiers.is_empty() {
            let expired = self
                .ext_modules
                .modifiers
                .expire(&mut self.substrate.entities, frame);
            self.raise_modifiers_expired(rules, frame_effects, &expired);
        }
        // The `ProductionAllowed` step: once per frame, before any module
        // runs (`decide`).
        self.decide_production_allowed(rules);
        self.revert_ext_module_conversions(rules, frame_effects);
        self.maintain_ext_module_anims(rules);
        self.fire_deferred_module_triggers(rules, frame_effects);
        self.poll_module_events(rules, frame_effects);
        self.raise_module_timers(rules, frame_effects);
        self.ext_modules.messages.prune(frame);
        for index in 0..self.ext_modules.slots.len() {
            if self.ext_modules.slots[index].disabled.is_some() {
                continue;
            }
            if !self.ext_modules.slots[index].started {
                self.ext_modules.slots[index].started = true;
                // The INI start records come first and share the one-time
                // budget (RFC 0001 section 6h).
                let used = match self.deliver_ini_start_records(
                    rules,
                    overlay_registry,
                    frame_effects,
                    index,
                    ONCE_BUDGET,
                ) {
                    Ok(used) => used,
                    Err(fault) => {
                        self.disable_ext_module(index, Export::Events, fault);
                        continue;
                    }
                };
                let Some(left) = ONCE_BUDGET.checked_sub(used).filter(|&left| left > 0) else {
                    self.disable_ext_module(index, Export::ScenarioStart, Fault::Budget);
                    continue;
                };
                if let Err(fault) = self.call_ext_module(
                    rules,
                    overlay_registry,
                    frame_effects,
                    index,
                    Export::ScenarioStart,
                    left,
                ) {
                    self.disable_ext_module(index, Export::ScenarioStart, fault);
                    continue;
                }
            }
            let messaged = match self.deliver_ext_module_messages(
                rules,
                overlay_registry,
                frame_effects,
                index,
            ) {
                Ok(called) => called,
                Err(fault) => {
                    self.disable_ext_module(index, Export::Message, fault);
                    continue;
                }
            };
            if let Err(fault) = self.deliver_ext_module_events(
                rules,
                overlay_registry,
                frame_effects,
                index,
                messaged,
            ) {
                self.disable_ext_module(index, Export::Events, fault);
                continue;
            }
            // Every frame, whatever the calls before it used.
            let budget = self.ext_modules.slots[index].frame_budget;
            match self.call_ext_module(
                rules,
                overlay_registry,
                frame_effects,
                index,
                Export::Frame,
                budget,
            ) {
                Ok(ops) => {
                    let slot = &mut self.ext_modules.slots[index];
                    slot.charge_frame_ops(frame, ops);
                    slot.last_frame_ops = ops;
                }
                Err(fault) => self.disable_ext_module(index, Export::Frame, fault),
            }
        }
        // This frame's share of every module's memory digests, a stopped
        // module's frozen memory included (RFC 15.1, item 7).
        for slot in &mut self.ext_modules.slots {
            let module = &slot.module;
            slot.digests
                .refresh(module.memory_len(), frame, |at, page| module.read_memory(at, page));
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

    /// Runs `f` on module `index` and its host. The module's slot leaves the
    /// slot list meanwhile and goes back to `index` after, so the host
    /// reaches the whole simulation while the module is out of reach (RFC
    /// section 5, "No reentry"): what the call reaches finds no slot for it
    /// (its decision links are skipped, `triggers::with_module_host` serves
    /// a placeholder), but the other module's slot stays, so a decision the
    /// call causes still asks that module's links. Such a nested call is
    /// read-only (`on_decide`), so it raises nothing that could call back.
    /// The merged rules text is lent to the outermost call only.
    fn with_ext_module<R>(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
        index: usize,
        f: impl FnOnce(&mut SimModule, &mut Host<'_, '_>) -> R,
    ) -> R {
        let mut slot = self.ext_modules.slots.remove(index);
        let rules_ini = self.ext_modules.rules_ini.take();
        let source = slot.source;
        let mut host = Host {
            sim: self,
            rules,
            overlay_registry,
            frame_effects,
            rng: &mut slot.rng,
            source,
            orders: &mut slot.orders,
            rules_ini: rules_ini.as_ref(),
        };
        let result = f(&mut slot.module, &mut host);
        // Nested calls put their slots back before returning, and no call
        // removes a slot for good, so `index` is where this one was.
        let at = index.min(self.ext_modules.slots.len());
        self.ext_modules.slots.insert(at, slot);
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
        slot.disabled = Some(reason.clone());
        // Its queued orders, triggers and queued records go with it (RFC
        // section 3).
        slot.orders.clear();
        let source = slot.source;
        // Its house domain claims; the native AI takes them back (RFC 6i).
        self.ext_modules.claims.release(source);
        // Its waiting client messages.
        self.ext_modules.messages.clear(source);
        self.ext_modules.keep_log(reason.clone());
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

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_ini_trigger_tests.rs"]
mod ini_trigger_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_decide_tests.rs"]
mod decide_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_flight_tests.rs"]
mod flight_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_death_tests.rs"]
mod death_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_unit_ai_tests.rs"]
mod unit_ai_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_event_tests.rs"]
mod event_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_ini_register_tests.rs"]
mod ini_register_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_claim_tests.rs"]
mod claim_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_drive_tests.rs"]
mod drive_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_order_tests.rs"]
mod order_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_control_tests.rs"]
mod control_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_movement_tests.rs"]
mod movement_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_message_tests.rs"]
mod message_tests;
