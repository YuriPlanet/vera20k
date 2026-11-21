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
//! - the atomic `AddMoney` action.
//!
//! Residuals before this leaves preview:
//! - Module memory is not saved. Saves and the presentation copy carry no
//!   modules.
//! - The state hash folds all of a module's memory every hash (not a 1/15
//!   page rotation), and the multiplayer frame checksum does not include
//!   modules.
//! - A fault is logged, not shown to players.
//! - House references use a provisional encoding ([`house_ref`]).
//! - Modules come only from [`Simulation::install_ext_module`]. Map packs and
//!   mod folders are not read yet.

use std::hash::{Hash, Hasher};

use vera20k_wasm_ext::{ActionParams, Export, Fault, Invalid, Limits, SimHost, SimModule, Status};

use crate::sim::houses::Houses;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::trigger_runtime::TriggerRuntime;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// The loaded simulation modules, in run order.
#[derive(Default)]
pub struct ExtModules {
    slots: Vec<Slot>,
    /// Log lines from modules, drained by the caller.
    log: Vec<String>,
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

    pub fn hash_state(&self, hasher: &mut impl Hasher) {
        b"ext-modules-v1".hash(hasher);
        for slot in &self.slots {
            slot.source.hash(hasher);
            slot.started.hash(hasher);
            slot.disabled.is_some().hash(hasher);
            slot.rng.hash_state(hasher);
            slot.module.memory().hash(hasher);
        }
    }
}

/// The host a module sees during one call: borrows of the owners it may use.
struct Host<'a> {
    session: &'a ScenarioSession,
    scenario_rng: &'a mut SimRng,
    trigger_runtime: &'a mut TriggerRuntime,
    houses: &'a mut Houses,
    rng: &'a mut SimRng,
    log: &'a mut Vec<String>,
    source: ModuleSource,
}

impl SimHost for Host<'_> {
    fn frame(&mut self) -> i32 {
        self.session.binary_frame() as i32
    }

    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        self.rng.next_range_i32_inclusive(lo, hi)
    }

    fn log(&mut self, line: &str) {
        log::info!(target: "ext_module", "{:?}: {line}", self.source);
        self.log.push(line.to_owned());
    }

    fn var_get(&mut self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        let (global, index) = stock_variable(scope, var)?;
        let set = if global {
            self.trigger_runtime.global_variable_is_set(index)
        } else {
            self.trigger_runtime.local_variable_is_set(index)
        };
        Ok((i64::from(set), Status::Ok))
    }

    fn var_set(&mut self, scope: i64, var: i32, value: i64) -> Result<Status, Invalid> {
        let (global, index) = stock_variable(scope, var)?;
        // The trigger runtime's writer also resets the timers of triggers
        // waiting on the variable (events 27, 28, 36, 37), as map actions do.
        Simulation::set_trigger_variable(
            self.session,
            self.scenario_rng,
            self.trigger_runtime,
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
                crate::sim::credit_income::add_credits(self.houses, owner, amount);
                Ok((0, Status::Ok))
            }
            _ => Ok((0, Status::Unsupported)),
        }
    }
}

impl Host<'_> {
    /// A house reference's house; `None` for a house that does not exist.
    fn house(&self, reference: i64) -> Result<Option<crate::sim::intern::InternedId>, Invalid> {
        if reference & HOUSE_REF_BIT == 0 || reference >> 48 != 0 {
            return Err(Invalid(format!("{reference:#x} is not a house reference")));
        }
        let index = (reference & !HOUSE_REF_BIT) as usize;
        Ok(self.houses.order().get(index).copied())
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
    pub fn install_ext_module(&mut self, source: ModuleSource, bytes: &[u8]) -> Result<(), String> {
        let limits = match source {
            ModuleSource::Mod => Limits::MOD,
            ModuleSource::Map => Limits::MAP,
        };
        let module = SimModule::load(bytes, limits).map_err(|e| e.to_string())?;
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
        match self.call_ext_module(position, Export::Init, ONCE_BUDGET) {
            Ok(_) => Ok(()),
            Err(fault) => {
                self.ext_modules.slots.remove(position);
                Err(format!("vera_init {fault}"))
            }
        }
    }

    pub fn ext_modules(&self) -> &ExtModules {
        &self.ext_modules
    }

    pub fn ext_modules_mut(&mut self) -> &mut ExtModules {
        &mut self.ext_modules
    }

    /// The module phase: each enabled module in order gets its
    /// `on_scenario_start` once, then `on_frame`.
    pub(crate) fn run_ext_module_phase(&mut self) {
        for index in 0..self.ext_modules.slots.len() {
            if self.ext_modules.slots[index].disabled.is_some() {
                continue;
            }
            if !self.ext_modules.slots[index].started {
                self.ext_modules.slots[index].started = true;
                if let Err(fault) = self.call_ext_module(index, Export::ScenarioStart, ONCE_BUDGET)
                {
                    self.disable_ext_module(index, Export::ScenarioStart, fault);
                    continue;
                }
            }
            match self.call_ext_module(index, Export::Frame, FRAME_BUDGET) {
                Ok(ops) => self.ext_modules.slots[index].last_frame_ops = ops,
                Err(fault) => self.disable_ext_module(index, Export::Frame, fault),
            }
        }
    }

    fn call_ext_module(&mut self, index: usize, export: Export, budget: u64) -> Result<u64, Fault> {
        let ExtModules { slots, log } = &mut self.ext_modules;
        let slot = &mut slots[index];
        let mut host = Host {
            session: &self.session,
            scenario_rng: &mut self.scenario_rng,
            trigger_runtime: &mut self.trigger_runtime,
            houses: &mut self.houses,
            rng: &mut slot.rng,
            log,
            source: slot.source,
        };
        slot.module.call(export, budget, &mut host)
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
        slot.disabled = Some(reason);
    }
}

#[cfg(all(test, tests_sim_c_g))]
#[path = "ext_modules_tests.rs"]
mod tests;
