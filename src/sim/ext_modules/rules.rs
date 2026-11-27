//! Modules during rules processing (RFC 0001 sections 2, 3 and 8): the mod's
//! `[VeraWasm]` policy, the map's `VeraMinVersion`, and `vera_init` plus
//! `on_rules` at each module's rules layer.
//!
//! The scenario loader builds a [`PreparedExtModules`] before it processes
//! the rules and hands it to the native rules owner as the
//! [`ExtRulesHook`]. At the mod layer (after `RULESMD.INI`, `LANGRULE.INI`
//! and the mode) a mod module that exports `on_rules` runs `vera_init` and
//! `on_rules`; at the map layer such a map module does. `rules_set` there
//! writes INI text, which the rules owner processes as one more pass before
//! later layers, so the native readers, clamps and postprocessing apply.
//! Once the simulation exists,
//! [`Simulation::install_scenario_ext_modules`](crate::sim::world::Simulation::install_scenario_ext_modules)
//! takes the modules over without running their initialization again; a
//! module without `on_rules` runs `vera_init` there, as before.
//!
//! Initialization here answers what the rules answer: names of rules types,
//! the modules' declared variables (IDs as the match numbers them, from
//! every loaded module's `vera.vars`) and `has_kind`. TeamType names need
//! the map's teams and answer `Unsupported` here.
//!
//! Residuals:
//! - In `vera_init` and `on_rules`, `random` writes 0 with `Ok`: the ABI's
//!   `random` has no status the host can set; the RFC wants `Unsupported`.
//! - `set` on a type reference in `on_rules` answers `Unsupported` instead
//!   of writing the value's INI text.
//! - Lobby and save checks do not compare `on_rules` results; every machine
//!   reproduces them from the same module bytes, and the processed rules a
//!   save carries include them.
//! - The mod's `Api=` and `MapUI=` are read and kept; nothing checks `Api=`
//!   against a module's import version yet, and `MapUI` waits for phase 2.

use vera20k_wasm_ext::{
    ActionParams, Export, FindPage, Invalid, NAME_MODULE_EVENT, NAME_STRING, NAME_VARIABLE, Query,
    SimHost, SimModule, Status, property_name,
};

use super::vars::VarStore;
use super::{FRAME_BUDGET, MAP_FRAME_BUDGET, MIN_BUDGET, ModuleSource, ONCE_BUDGET, SEAT_BUDGET};

/// A budget key's value: at most `ceiling`; below [`MIN_BUDGET`] it is an
/// error naming the key.
fn checked_budget(key: &str, value: i64, ceiling: u64) -> Result<u64, String> {
    match u64::try_from(value) {
        Ok(value) if value >= MIN_BUDGET => Ok(value.min(ceiling)),
        _ => Err(format!(
            "[VeraWasm] {key}={value} is below {MIN_BUDGET} operations per frame"
        )),
    }
}
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::native_processing::{ExtRulesHook, ExtRulesLayer};
use crate::rules::ruleset::RuleSet;

/// How a mod lets map modules show interface (`[VeraWasm] MapUI=`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MapUi {
    #[default]
    Overlay,
    Full,
    No,
}

impl MapUi {
    /// `[VeraWasm] MapUI=` of the mod's own `rulesmd.ini` (the stock one
    /// without a mod). The client reads it when it installs its modules
    /// (`app::ui_modules::install`), for a new match and a restored save
    /// alike: the simulation does not hold it, so a save does not carry it.
    pub fn read(mod_rules: &IniFile) -> Self {
        match mod_rules
            .section_or_empty("VeraWasm")
            .read_name("MapUI", 16)
        {
            Some(name) if name.eq_ignore_ascii_case("full") => Self::Full,
            Some(name) if name.eq_ignore_ascii_case("no") => Self::No,
            _ => Self::Overlay,
        }
    }
}

/// The mod's `rulesmd.ini` `[VeraWasm]` keys (appendix "INI keys"), read
/// from that file alone, without layering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmPolicy {
    /// `Api=`: the API `major.minor` the mod's modules need.
    pub api: Option<String>,
    /// `Budget=`: the mod module's operations per frame.
    pub budget: u64,
    /// `MapBudget=`: the map module's operations per frame.
    pub map_budget: u64,
    /// `SeatBudget=`: what each claimed seat adds to the mod module's
    /// budget (RFC 0001 6i).
    pub seat_budget: u64,
    /// `MapWasm=`: whether maps with modules may start under this mod.
    pub map_wasm: bool,
    /// `Controller=`: the lobby offers "mod AI" for computer seats, and
    /// such a seat's house starts with every domain claimed by the mod
    /// module (RFC 6i, `claims`).
    pub controller: bool,
    /// The room switch "Allow automation" (global `AutomationAllowed`,
    /// appendix 7.8): a room setting, not a `[VeraWasm]` key; the loader
    /// sets it from the room's settings
    /// ([`PreparedExtModules::set_automation_allowed`]).
    pub automation_allowed: bool,
}

impl Default for WasmPolicy {
    fn default() -> Self {
        Self {
            api: None,
            budget: FRAME_BUDGET,
            map_budget: MAP_FRAME_BUDGET,
            seat_budget: SEAT_BUDGET,
            map_wasm: true,
            controller: false,
            automation_allowed: false,
        }
    }
}

impl WasmPolicy {
    /// Reads `[VeraWasm]` from the mod's own `rulesmd.ini`; `player_budget`
    /// (the room's `[VERA] WasmBudget=`, 0 for none) replaces `Budget=`.
    /// Budgets above the engine ceiling are lowered to it; one below
    /// [`MIN_BUDGET`] refuses the match (RFC 15.4).
    pub fn read(mod_rules: &IniFile, player_budget: u32) -> Result<Self, String> {
        let section = mod_rules.section_or_empty("VeraWasm");
        let budget = |key: &str, default: u64, ceiling: u64| -> Result<u64, String> {
            let value = section.read_int(key, i32::try_from(default).unwrap_or(i32::MAX));
            checked_budget(key, i64::from(value), ceiling)
        };
        let mod_budget = match player_budget {
            0 => budget("Budget", FRAME_BUDGET, FRAME_BUDGET)?,
            value => checked_budget("WasmBudget", i64::from(value), FRAME_BUDGET)?,
        };
        Ok(Self {
            api: section.read_name("Api", 16).map(str::to_owned),
            budget: mod_budget,
            map_budget: budget("MapBudget", MAP_FRAME_BUDGET, FRAME_BUDGET)?,
            seat_budget: budget("SeatBudget", SEAT_BUDGET, SEAT_BUDGET)?,
            map_wasm: section.read_bool("MapWasm", true),
            controller: section.read_bool("Controller", false),
            automation_allowed: false,
        })
    }

    pub const fn budget_of(&self, source: ModuleSource) -> u64 {
        match source {
            ModuleSource::Mod => self.budget,
            ModuleSource::Map => self.map_budget,
        }
    }
}

/// A module loaded for a scenario, initialized during rules processing.
pub(super) struct PreparedModule {
    pub(super) source: ModuleSource,
    pub(super) module: SimModule,
    pub(super) initialized: bool,
    /// The "mod AI" seats its memory limit counts ([`vera20k_wasm_ext::Limits::with_seats`]).
    pub(super) memory_seats: usize,
}

/// A scenario's modules before the simulation exists: the rules hook.
#[derive(Default)]
pub struct PreparedExtModules {
    pub(super) policy: WasmPolicy,
    pub(super) modules: Vec<PreparedModule>,
    /// Lines the modules logged before the match.
    pub(super) log: Vec<String>,
    /// What the players are told when the match starts
    /// ([`map_wasm_tags_notice`]).
    pub(super) notices: Vec<String>,
    /// The mod's INI triggers (`Wasm.Tags`), built at install.
    pub(super) ini_triggers: super::ini_triggers::IniTriggers,
    /// What the modules' `vera_init` registered at the rules stage; the
    /// simulation takes it over at install.
    pub(super) ini: super::ini_names::IniNames,
}

impl PreparedExtModules {
    /// Read the selected mod's loose rules module through the asset catalogue.
    pub fn read_mod_file(
        assets: &crate::assets::asset_manager::AssetManager,
    ) -> Result<Option<std::sync::Arc<[u8]>>, String> {
        assets
            .read_mod_file_bounded("rules.wasm", crate::map::ext_module_pack::MAX_MODULE_BYTES)
            .map_err(|error| format!("the mod's rules.wasm: {error}"))
    }

    /// Loads the selected mod's `rules.wasm` and the map's embedded module,
    /// under the mod's `[VeraWasm]` policy (read from `mod_rules`, the mod's
    /// own `rulesmd.ini`, or the stock one without a mod). Checks the map's
    /// `[Basic] VeraMinVersion=`. An error is the reason the match cannot
    /// start; it names the module.
    pub fn load(
        mod_rules: &IniFile,
        mod_module: Option<&[u8]>,
        map: &IniFile,
        player_budget: u32,
        mod_ai_seats: usize,
    ) -> Result<Self, String> {
        let current = crate::util::version::GAME_VERSION;
        if let Some(needs) = map
            .section_or_empty("Basic")
            .read_name("VeraMinVersion", 32)
            && !crate::util::version::version_serves(current, needs)
        {
            return Err(format!(
                "this map needs VERA20k {needs} or newer; this is {current}"
            ));
        }
        let policy = WasmPolicy::read(mod_rules, player_budget)?;
        let ini_triggers = super::ini_triggers::IniTriggers::read(mod_rules)
            .map_err(|e| format!("the mod's rulesmd.ini: {e}"))?;
        let mut modules = Vec::new();
        if let Some(bytes) = mod_module {
            // A controller mod's module gets memory for each "mod AI" seat
            // of the room (RFC 0001 15.2, 6i).
            let seats = if policy.controller { mod_ai_seats } else { 0 };
            modules.push(PreparedModule {
                source: ModuleSource::Mod,
                module: SimModule::load(bytes, ModuleSource::Mod.limits().with_seats(seats))
                    .map_err(|e| format!("the mod's rules.wasm: {e}"))?,
                initialized: false,
                memory_seats: seats,
            });
        }
        let embedded = crate::map::ext_module_pack::rules_module(map)
            .map_err(|e| format!("the map's rules module: {e}"))?;
        if let Some(bytes) = embedded {
            if !policy.map_wasm {
                return Err("the selected mod does not allow maps with Wasm modules \
                     ([VeraWasm] MapWasm=no); choose another map or play without the mod"
                    .to_owned());
            }
            modules.push(PreparedModule {
                source: ModuleSource::Map,
                module: SimModule::load(&bytes, ModuleSource::Map.limits())
                    .map_err(|e| format!("the map's rules module: {e}"))?,
                initialized: false,
                memory_seats: 0,
            });
        }
        // A map notification whose ID a different mod name has rejects the
        // map module (RFC 0001 section 3, step 7).
        if let [first, second] = &modules[..] {
            check_notify_collision(&first.module, &second.module)?;
        }
        Ok(Self {
            policy,
            modules,
            log: Vec::new(),
            notices: map_wasm_tags_notice(map).into_iter().collect(),
            ini_triggers,
            ini: Default::default(),
        })
    }

    pub fn policy(&self) -> &WasmPolicy {
        &self.policy
    }

    /// The room switch "Allow automation" (`PersonalRules::automation_allowed`;
    /// a network room plays its host's), read as the global
    /// `AutomationAllowed`. Off unless set.
    pub fn set_automation_allowed(&mut self, allowed: bool) {
        self.policy.automation_allowed = allowed;
    }

    /// The variables the modules declare, numbered as the match numbers
    /// them (mod module first).
    fn declared_vars(&self) -> VarStore {
        let mut vars = VarStore::default();
        vars.declare(self.modules.iter().map(|m| (m.source, m.module.vars())));
        vars
    }

    /// A module that exports `on_rules` but missed its rules stage: these
    /// modules were not handed to the rules load (or its layer never came).
    pub(super) fn missed_rules_stage(&self) -> Option<ModuleSource> {
        self.modules
            .iter()
            .find(|m| !m.initialized && m.module.has_export(Export::Rules))
            .map(|m| m.source)
    }
}

/// RFC 0001 section 3, step 7: notification names of the mod module
/// (`first`) and the map module (`second`) that differ but share an ID
/// reject the map module, naming both. The same name in both is one
/// notification. Each module's own names were checked at its load.
pub(super) fn check_notify_collision(first: &SimModule, second: &SimModule) -> Result<(), String> {
    match vera20k_wasm_ext::sections::notify_collision(second.notify_names(), first.notify_names())
    {
        Some((map, mods)) => Err(format!(
            "the map's rules module: its notification {map} has the same ID as the mod's {mods}"
        )),
        None => Ok(()),
    }
}

/// RFC section 2: a map's `Wasm.Tags` is ignored, and the players are told.
/// The notice names the map's sections that carry the key; `None` when none
/// does.
fn map_wasm_tags_notice(map: &IniFile) -> Option<String> {
    let sections: Vec<&str> = map
        .section_names()
        .into_iter()
        .filter(|name| {
            map.section(name)
                .is_some_and(|section| section.raw_value("Wasm.Tags").is_some())
        })
        .collect();
    (!sections.is_empty()).then(|| {
        format!(
            "This map sets Wasm.Tags in [{}]. Only a mod's rulesmd.ini binds triggers to \
             types, so the game ignores it; the map's own module can bind in code.",
            sections.join("], [")
        )
    })
}

const fn source_of(layer: ExtRulesLayer) -> ModuleSource {
    match layer {
        ExtRulesLayer::Mod => ModuleSource::Mod,
        ExtRulesLayer::Map => ModuleSource::Map,
    }
}

impl ExtRulesHook for PreparedExtModules {
    fn runs_at(&self, layer: ExtRulesLayer) -> bool {
        let source = source_of(layer);
        self.modules
            .iter()
            .any(|m| m.source == source && m.module.has_export(Export::Rules))
    }

    fn on_rules(
        &mut self,
        layer: ExtRulesLayer,
        merged: &IniFile,
        rules: &RuleSet,
    ) -> Result<Option<IniFile>, String> {
        let source = source_of(layer);
        let vars = self.declared_vars();
        let mut writes = Vec::new();
        let at_layer = self
            .modules
            .iter_mut()
            .filter(|m| m.source == source && m.module.has_export(Export::Rules));
        for prepared in at_layer {
            let mut host = RulesHost {
                merged,
                rules,
                vars: &vars,
                source,
                writes: &mut writes,
                log: &mut self.log,
                in_rules: false,
                ini: &mut self.ini,
            };
            prepared
                .module
                .call(Export::Init, ONCE_BUDGET, &mut host)
                .map_err(|fault| format!("{source:?} module stopped in vera_init: {fault}"))?;
            host.in_rules = true;
            prepared
                .module
                .call(Export::Rules, ONCE_BUDGET, &mut host)
                .map_err(|fault| {
                    format!("{source:?} module stopped in on_rules; its rules writes are incomplete: {fault}")
                })?;
            prepared.initialized = true;
        }
        Ok((!writes.is_empty()).then(|| writes_to_ini(writes)))
    }
}

/// The writes of one layer, in write order, as one INI pass.
fn writes_to_ini(writes: Vec<(String, String, String)>) -> IniFile {
    let mut sections: Vec<IniSection> = Vec::new();
    for (section, key, value) in writes {
        let index = match sections.iter().position(|s| s.name == section) {
            Some(index) => index,
            None => {
                sections.push(IniSection::new(section));
                sections.len() - 1
            }
        };
        sections[index].set(&key, &value);
    }
    IniFile::from_sections(sections)
}

/// Whether a section, key or value can be written as INI text: no line
/// breaks, and no `]` in a section name or `=` in a key.
fn writable(section: &str, key: &str, value: &str) -> bool {
    let line = |text: &str| !text.contains(['\r', '\n', ';']);
    !section.trim().is_empty()
        && !key.trim().is_empty()
        && line(section)
        && line(key)
        && line(value)
        && !section.contains(']')
        && !key.contains('=')
}

/// Reads `[section] key` from merged rules text.
pub(super) fn raw_rules_value(ini: &IniFile, section: &str, key: &str) -> Option<String> {
    ini.section(section)?.raw_value(key).map(str::to_owned)
}

/// The host during `vera_init` and `on_rules`: names, probes, the log and
/// rules text. Everything else answers `Unsupported` (appendix 1.1).
struct RulesHost<'a> {
    merged: &'a IniFile,
    rules: &'a RuleSet,
    /// Every loaded module's declared variables (names only).
    vars: &'a VarStore,
    source: ModuleSource,
    /// This layer's writes so far: (section, key, value).
    writes: &'a mut Vec<(String, String, String)>,
    log: &'a mut Vec<String>,
    /// Inside `on_rules` rather than `vera_init`.
    in_rules: bool,
    /// INI names registered so far.
    ini: &'a mut super::ini_names::IniNames,
}

impl SimHost for RulesHost<'_> {
    fn frame(&mut self) -> i32 {
        0
    }

    fn random(&mut self, _lo: i32, _hi: i32) -> i32 {
        0
    }

    fn log(&mut self, line: &str) {
        log::info!(target: "ext_module", "rules: {line}");
        self.log.push(line.to_owned());
    }

    fn has_kind(&mut self, table: i32, id: i32) -> bool {
        super::has_kind(self.rules, table, id)
    }

    fn lookup(&mut self, kind: i32, name: &str) -> Result<(i32, Status), Invalid> {
        let id = match kind {
            NAME_VARIABLE => self.vars.lookup(self.source, name),
            NAME_STRING => self.ini.string_id(name),
            NAME_MODULE_EVENT => self.ini.event_id(name),
            _ => match super::rules_name_id(self.rules, kind, name) {
                Some(id) => id,
                None => return Ok((-1, Status::Unsupported)),
            },
        };
        Ok(id.map_or((-1, Status::NotFound), |id| (id, Status::Ok)))
    }

    fn name_of(&mut self, kind: i32, id: i32) -> Result<(String, Status), Invalid> {
        let name = match kind {
            NAME_VARIABLE => return super::variable_name(self.vars, id),
            NAME_STRING | NAME_MODULE_EVENT => return Ok(super::ini_name_of(self.ini, kind, id)),
            _ => match super::rules_id_name(self.rules, kind, id) {
                Some(name) => name,
                None => return Ok((String::new(), Status::Unsupported)),
            },
        };
        super::name_answer(name, kind, id)
    }

    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        // Only type references are readable here (appendix 1.1); a type's
        // `Name` gets its string ID as in a match.
        let name = property_name(prop).unwrap_or_default();
        match super::decode_ref(reference)? {
            Some(super::Ref::Type { kind, id }) if self.in_rules && name == "Name" => {
                let rules = self.rules;
                Ok(match super::props::type_name(rules, kind, id)? {
                    Some(text) => (i64::from(self.ini.intern(text)), Status::Ok),
                    None => (0, Status::Unsupported),
                })
            }
            Some(super::Ref::Type { kind, id }) if self.in_rules => {
                super::type_property(self.rules, kind, id, name)
            }
            _ => Ok((0, Status::Unsupported)),
        }
    }

    fn set(&mut self, _reference: i64, _prop: i32, _value: i64) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }

    fn find(&mut self, _query: &Query, _cap: usize) -> Result<(FindPage, Status), Invalid> {
        Ok((FindPage::default(), Status::Unsupported))
    }

    fn var_get(&mut self, _scope: i64, _var: i32) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }

    fn var_set(&mut self, _scope: i64, _var: i32, _value: i64) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }

    fn action(
        &mut self,
        _kind: i32,
        _target: i64,
        _params: &ActionParams,
        _text: &str,
    ) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }

    fn rules_get(&mut self, section: &str, key: &str) -> Result<(Option<String>, Status), Invalid> {
        // `rules_get` is not available in `vera_init` (RFC 8).
        if !self.in_rules {
            return Ok((None, Status::Unsupported));
        }
        // This layer's own writes first, latest first, then the rules
        // processed before the layer.
        let written = self
            .writes
            .iter()
            .rev()
            .find(|(s, k, _)| s.eq_ignore_ascii_case(section) && k.eq_ignore_ascii_case(key));
        let value = match written {
            Some((_, _, value)) => Some(value.clone()),
            None => raw_rules_value(self.merged, section, key),
        };
        Ok((value, Status::Ok))
    }

    fn rules_set(&mut self, section: &str, key: &str, value: &str) -> Result<Status, Invalid> {
        if !self.in_rules {
            return Ok(Status::Unsupported);
        }
        if !writable(section, key, value) {
            return Ok(Status::Refused);
        }
        self.writes
            .push((section.to_owned(), key.to_owned(), value.to_owned()));
        Ok(Status::Ok)
    }

    fn ini_register(
        &mut self,
        kind: i32,
        section: i32,
        name: &str,
        key: i32,
    ) -> Result<(i32, Status), Invalid> {
        self.ini.register(self.source, kind, section, name, key)
    }

    fn order(&mut self, _house: i64, _order: &vera20k_wasm_ext::Order) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
}
