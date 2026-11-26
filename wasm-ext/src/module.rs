//! One loaded simulation module, over any [`Runtime`]: the load checks, the
//! shared rewrite ([`crate::instrument`]), budgets, delivery and saves are
//! the same on every platform; only executing the rewritten bytes differs.

use std::ptr::NonNull;
use std::sync::Arc;

use wasmparser::{FuncType, Payload, TypeRef, ValType, Validator, WasmFeatures};

use crate::client::{CLIENT_IMPORT_MODULE, CLIENT_IMPORTS};
use crate::decide::{DecideRecord, ResultArea};
use crate::host::Ctx;
use crate::instrument::{self, DEPTH, METER, STOP, STOP_BUDGET, STOP_DEPTH};
use crate::sections::{self, Delivery, Sections};
use crate::{
    EventRecord, Export, Fault, IMPORTS, Limits, LoadError, MAX_MESSAGE_BYTES, ModuleState,
    PAGE_BYTES, SIM_IMPORT_MODULE, SimHost, VarDecl,
};

/// Which world a module belongs to (RFC section 1): it decides the import
/// module a module may import from and the imports a runtime links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum World {
    /// `rules.wasm`, `vera:sim@1.0` ([`crate::host`]).
    Sim,
    /// `ui.wasm`, `vera:client@1.0` ([`crate::client_host`]).
    Client,
}

impl World {
    pub(crate) const fn import_module(self) -> &'static str {
        match self {
            Self::Sim => SIM_IMPORT_MODULE,
            Self::Client => CLIENT_IMPORT_MODULE,
        }
    }

    const fn imports(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Sim => IMPORTS,
            Self::Client => CLIENT_IMPORTS,
        }
    }

    /// The SDK-generated initialization export that replaces `start`.
    const fn init_export(self) -> &'static str {
        match self {
            Self::Sim => "vera_init",
            Self::Client => "client_init",
        }
    }
}

/// One argument of an export call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Arg {
    I32(i32),
    I64(i64),
    /// `client_on_render`'s `dt` and `alpha`.
    F32(f32),
}

/// Most records one `on_events` call receives (RFC 15.4), within what the
/// delivery area holds, so that a burst is many calls that can wait instead
/// of one that overruns.
pub const MAX_BATCH: usize = 16;

/// Load-time shape limits (RFC section 3, open question 7), so compiling a
/// downloaded map's module cannot stall the lobby.
const MAX_FUNCTIONS: u32 = 20_000;
const MAX_BODY_BYTES: usize = 1 << 20;
const MAX_MODULE_BYTES: usize = 8 << 20;

/// Why a call trapped, named the same on every runtime: the text becomes
/// part of the module's stop reason, which every machine must agree on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrapKind {
    Unreachable,
    MemoryOutOfBounds,
    TableOutOfBounds,
    IndirectCallToNull,
    BadSignature,
    IntegerOverflow,
    IntegerDivisionByZero,
    BadConversionToInteger,
    /// The runtime's own stack ran out before the rewrite's depth limit.
    StackOverflow,
    /// A host call stopped the module (its fault says why), or another error.
    Other,
}

impl TrapKind {
    fn text(self) -> &'static str {
        match self {
            Self::Unreachable => "wasm `unreachable` instruction executed",
            Self::MemoryOutOfBounds => "out of bounds memory access",
            Self::TableOutOfBounds => "undefined element: out of bounds table access",
            Self::IndirectCallToNull => "uninitialized element",
            Self::BadSignature => "indirect call type mismatch",
            Self::IntegerOverflow => "integer overflow",
            Self::IntegerDivisionByZero => "integer divide by zero",
            Self::BadConversionToInteger => "invalid conversion to integer",
            Self::StackOverflow => "call stack exhausted",
            Self::Other => "module call failed",
        }
    }
}

/// What executes rewritten module bytes: Wasmtime on the desktop, the
/// browser's own WebAssembly engine in the browser. Every import goes through
/// [`crate::host`] or, for a client module, [`crate::client_host`]. Module
/// memory is reached by copying, since a browser's lives outside the
/// engine's memory.
pub(crate) trait Runtime: Sized {
    /// Compiles and instantiates the rewritten module with `world`'s
    /// imports. No module code runs.
    fn instantiate(bytes: &[u8], ctx: Ctx, world: World) -> Result<Self, String>;
    /// Runs `f` on the context the imports share.
    fn with_ctx<T>(&mut self, f: impl FnOnce(&mut Ctx) -> T) -> T;
    /// Calls export `name` (a function taking `args`' types and returning
    /// nothing, checked at load) with `args`. A runtime supports the
    /// argument lists the engine's exports take: none, `(i32, i32)`,
    /// `(i64, i32, i32)` and `(f32, f32)`.
    fn call(&mut self, name: &str, args: &[Arg]) -> Result<(), TrapKind>;
    /// The module's memory size in bytes, 0 without memory.
    fn memory_len(&self) -> usize;
    /// Copies module memory at `offset` into `buf`; false out of bounds.
    fn read_memory(&self, offset: usize, buf: &mut [u8]) -> bool;
    /// Copies `bytes` into module memory at `offset`; false out of bounds.
    fn write_memory(&mut self, offset: usize, bytes: &[u8]) -> bool;
    fn grow(&mut self, pages: u64) -> Result<(), String>;
    /// An exported integer global's raw bits, 0 when there is none (the
    /// rewrite stores every mutable global as an integer).
    fn global(&mut self, name: &str) -> u64;
    fn set_global(&mut self, name: &str, bits: u64) -> Result<(), String>;
}

/// One loaded simulation module.
pub struct SimModuleOn<R> {
    /// The module as supplied, before load-time rewriting; saves carry it.
    bytes: Vec<u8>,
    rt: R,
    /// The module's custom sections, shared with its imports.
    sections: Arc<Sections>,
    /// Which of [`Export::ALL`] the module has.
    exports: [bool; 4],
    on_events: bool,
    on_decide: bool,
    /// `on_message(sender: i64, ptr, len)` (appendix 1.2, phase 2).
    on_message: bool,
    /// `vera.delivery`, checked against memory at load.
    delivery: Option<Delivery>,
    /// Every mutable global the module declares, in global index order.
    globals: Vec<u32>,
    /// Their raw bits after the latest call, so a save or hash reads them
    /// without a mutable store.
    global_bits: Vec<u64>,
}

#[allow(private_bounds)]
impl<R: Runtime> SimModuleOn<R> {
    /// Checks, rewrites and instantiates the module. No module code runs:
    /// modules with a `start` section are rejected.
    pub fn load(bytes: &[u8], limits: Limits) -> Result<Self, LoadError> {
        check(bytes, World::Sim)?;
        let sections = Arc::new(Sections::read(bytes).map_err(LoadError::Invalid)?);
        let delivery = sections.delivery;
        let max_pages = (limits.memory_bytes / PAGE_BYTES) as u64;
        let rewritten = instrument::rewrite(bytes, max_pages).map_err(LoadError::Invalid)?;
        // An export the engine calls takes `params` i32s and returns nothing.
        let export = |export: Export, params: usize| {
            export_fits(
                &rewritten.exports,
                export.name(),
                &vec![ValType::I32; params],
            )
        };
        let mut exports = [false; 4];
        for (slot, name) in exports.iter_mut().zip(Export::ALL) {
            *slot = export(name, 0)?;
        }
        let on_events = export(Export::Events, 2)?;
        let on_decide = export(Export::Decide, 2)?;
        let on_message = export_fits(
            &rewritten.exports,
            Export::Message.name(),
            &[ValType::I64, ValType::I32, ValType::I32],
        )?;
        let rt = R::instantiate(
            &rewritten.bytes,
            Ctx::new(Arc::clone(&sections)),
            World::Sim,
        )
        .map_err(LoadError::Invalid)?;
        // A message is written into the delivery area; any valid area holds
        // the largest one (appendix "Custom sections").
        check_delivery(
            delivery,
            if on_events {
                Some(Export::Events.name())
            } else if on_decide {
                Some(Export::Decide.name())
            } else if on_message {
                Some(Export::Message.name())
            } else {
                None
            },
            EventRecord::SIZE,
            rt.memory_len(),
        )
        .map_err(LoadError::Invalid)?;
        // `on_decide` reads one `DecideRecord` in place.
        if on_decide && delivery.is_some_and(|area| (area.bytes as usize) < DecideRecord::SIZE) {
            return Err(LoadError::Invalid(format!(
                "exports on_decide with a vera.delivery area smaller than one {}-byte DecideRecord",
                DecideRecord::SIZE
            )));
        }
        let mut module = Self {
            bytes: bytes.to_vec(),
            rt,
            sections,
            exports,
            on_events,
            on_decide,
            on_message,
            delivery,
            globals: rewritten.mutable_globals,
            global_bits: Vec::new(),
        };
        module.read_globals();
        Ok(module)
    }

    /// Loads `bytes` and puts back a state taken with [`SimModuleOn::state`].
    /// No module code runs: initialization ran before the save.
    pub fn restore(bytes: &[u8], limits: Limits, state: &ModuleState) -> Result<Self, LoadError> {
        let invalid = |why: String| LoadError::Invalid(format!("saved module state: {why}"));
        let mut module = Self::load(bytes, limits)?;
        if state.globals.len() != module.globals.len() {
            return Err(invalid(format!(
                "{} globals saved, module has {}",
                state.globals.len(),
                module.globals.len()
            )));
        }
        if module.rt.memory_len() > 0 || state.pages > 0 {
            let have = (module.rt.memory_len() / PAGE_BYTES) as u64;
            let want = u64::from(state.pages);
            if want < have {
                return Err(invalid(format!(
                    "{want} pages saved, module starts with {have}"
                )));
            }
            module
                .rt
                .grow(want - have)
                .map_err(|e| invalid(format!("memory cannot grow to {want} pages: {e}")))?;
            let zero = vec![0u8; PAGE_BYTES];
            for page in 0..want as usize {
                module.rt.write_memory(page * PAGE_BYTES, &zero);
            }
            for (page, bytes) in &state.nonzero_pages {
                if bytes.len() != PAGE_BYTES {
                    return Err(invalid(format!("page {page} holds {} bytes", bytes.len())));
                }
                if !module.rt.write_memory(*page as usize * PAGE_BYTES, bytes) {
                    return Err(invalid(format!("page {page} is outside {want} pages")));
                }
            }
        } else if !state.nonzero_pages.is_empty() {
            return Err(invalid("memory saved for a module without memory".into()));
        }
        for (index, bits) in module.globals.clone().iter().zip(&state.globals) {
            module
                .rt
                .set_global(&instrument::global_export_name(*index), *bits)
                .map_err(|e| invalid(format!("global cannot be set: {e}")))?;
        }
        module.read_globals();
        Ok(module)
    }

    /// The module as supplied to [`SimModuleOn::load`].
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The variables the module's `vera.vars` section declares.
    pub fn vars(&self) -> &[VarDecl] {
        &self.sections.vars
    }

    /// The dispatch key the module's `vera.actions` gives `name` (INI
    /// `Wasm:name` actions).
    pub fn action_key(&self, name: &str) -> Option<i32> {
        self.sections.action_key(name)
    }

    /// The notification names the module's `vera.notify` declares, with
    /// their IDs ([`crate::notify_id`]).
    pub fn notify_names(&self) -> &[(String, i32)] {
        &self.sections.notify
    }

    /// The raw bits of every mutable global after the latest call.
    pub fn globals(&self) -> &[u64] {
        &self.global_bits
    }

    /// The state a save needs: memory pages that are not all zero, and the
    /// mutable globals.
    pub fn state(&self) -> ModuleState {
        let memory = self.memory();
        ModuleState {
            pages: (self.memory_len() / PAGE_BYTES) as u32,
            nonzero_pages: memory
                .chunks(PAGE_BYTES)
                .enumerate()
                .filter(|(_, page)| page.iter().any(|b| *b != 0))
                .map(|(index, page)| (index as u32, page.to_vec()))
                .collect(),
            globals: self.global_bits.clone(),
        }
    }

    fn read_globals(&mut self) {
        let rt = &mut self.rt;
        self.global_bits = self
            .globals
            .iter()
            .map(|index| rt.global(&instrument::global_export_name(*index)))
            .collect();
    }

    pub fn has_export(&self, export: Export) -> bool {
        match export {
            Export::Events => self.on_events,
            Export::Decide => self.on_decide,
            Export::Message => self.on_message,
            _ => self.exports.get(export as usize).copied().unwrap_or(false),
        }
    }

    /// How many records one `on_events` call can take: 0 without the export,
    /// else what `vera.delivery` holds, at most [`MAX_BATCH`].
    pub fn batch_capacity(&self) -> usize {
        match (self.on_events, self.delivery) {
            (true, Some(area)) => (area.bytes as usize / EventRecord::SIZE).min(MAX_BATCH),
            _ => 0,
        }
    }

    /// Calls `export` with `budget` operations. Returns the operations used.
    /// A missing export costs nothing; [`Export::Events`] goes through
    /// [`SimModuleOn::deliver`].
    pub fn call(
        &mut self,
        export: Export,
        budget: u64,
        host: &mut dyn SimHost,
    ) -> Result<u64, Fault> {
        if !matches!(export, Export::Events | Export::Decide | Export::Message)
            && self.has_export(export)
        {
            return self.run(export, budget, host, &[]);
        }
        Ok(0)
    }

    /// Writes `records` into the delivery area and calls `on_events` on
    /// them with `budget` operations. At most [`SimModuleOn::batch_capacity`]
    /// records; an empty batch makes no call.
    pub fn deliver(
        &mut self,
        records: &[EventRecord],
        budget: u64,
        host: &mut dyn SimHost,
    ) -> Result<u64, Fault> {
        let (true, Some(area)) = (self.on_events, self.delivery) else {
            return Ok(0);
        };
        if records.is_empty() {
            return Ok(0);
        }
        if records.len() > self.batch_capacity() {
            return Err(Fault::Invalid(format!(
                "{} records exceed the delivery area",
                records.len()
            )));
        }
        let bytes: Vec<u8> = records.iter().flat_map(EventRecord::to_bytes).collect();
        self.write_delivery(area, &bytes)?;
        let args = [Arg::I32(area.offset as i32), Arg::I32(records.len() as i32)];
        self.run(Export::Events, budget, host, &args)
    }

    /// Writes one client message's `data` at the start of the delivery area
    /// and calls `on_message(sender, ptr, len)` with `budget` operations
    /// (RFC 0001 section 10, appendix 1.2). `sender` is the sending player's
    /// house reference, which the engine fills. Without the export no call
    /// is made and nothing is used. A message over [`MAX_MESSAGE_BYTES`] is
    /// the engine's error, refused before the call.
    pub fn deliver_message(
        &mut self,
        sender: i64,
        data: &[u8],
        budget: u64,
        host: &mut dyn SimHost,
    ) -> Result<u64, Fault> {
        let (true, Some(area)) = (self.on_message, self.delivery) else {
            return Ok(0);
        };
        if data.len() > MAX_MESSAGE_BYTES || data.len() > area.bytes as usize {
            return Err(Fault::Invalid(format!(
                "a {}-byte message exceeds the delivery area",
                data.len()
            )));
        }
        self.write_delivery(area, data)?;
        let args = [
            Arg::I64(sender),
            Arg::I32(area.offset as i32),
            Arg::I32(data.len() as i32),
        ];
        self.run(Export::Message, budget, host, &args)
    }

    /// Writes `record` into the delivery area and calls `on_decide(ptr, 1)`
    /// with `budget` operations (RFC 6f). Returns the handler's answer
    /// ([`DecideRecord::answer`]) and the operations used. Without the
    /// export no call is made and the answer is `None`. During the call a
    /// write import stops the module.
    ///
    /// `result` is the request's result area (RFC 6f, appendix "Fixed
    /// layouts"): it follows the record in the delivery area, its `cap`
    /// lowered to what the area holds there (0 when nothing fits), and holds
    /// the previous link's bytes, the rest zeroed. After a handled call
    /// `result.bytes` is the whole area as the handler left it.
    pub fn decide(
        &mut self,
        record: &DecideRecord,
        result: &mut ResultArea,
        budget: u64,
        host: &mut dyn SimHost,
    ) -> Result<(Option<i64>, u64), Fault> {
        let (true, Some(area)) = (self.on_decide, self.delivery) else {
            return Ok((None, 0));
        };
        let room = (area.bytes as usize).saturating_sub(DecideRecord::SIZE);
        result.cap = result.cap.min(room);
        let previous_len = result.bytes.len().min(result.cap);
        let at = area.offset as usize + DecideRecord::SIZE;
        let (area_at, cap) = match result.cap {
            0 => (0, 0),
            cap => (at as i32, cap as i32),
        };
        self.write_delivery(area, &record.to_bytes(area_at, cap, previous_len as i32))?;
        if result.cap > 0 {
            let mut bytes = vec![0u8; result.cap];
            bytes[..previous_len].copy_from_slice(&result.bytes[..previous_len]);
            if !self.rt.write_memory(at, &bytes) {
                return Err(Fault::Trap("delivery area: out of bounds".into()));
            }
        }
        let args = [Arg::I32(area.offset as i32), Arg::I32(1)];
        let used = self.run(Export::Decide, budget, host, &args)?;
        let mut bytes = [0u8; DecideRecord::SIZE];
        if !self.rt.read_memory(area.offset as usize, &mut bytes) {
            return Err(Fault::Trap("delivery area: out of bounds".into()));
        }
        let answer = DecideRecord::answer(&bytes);
        if answer.is_some() && result.cap > 0 {
            let mut bytes = vec![0u8; result.cap];
            if !self.rt.read_memory(at, &mut bytes) {
                return Err(Fault::Trap("delivery area: out of bounds".into()));
            }
            result.bytes = bytes;
        }
        Ok((answer, used))
    }

    /// Load checked the area lies inside memory, which never shrinks.
    fn write_delivery(&mut self, area: Delivery, bytes: &[u8]) -> Result<(), Fault> {
        match self.rt.write_memory(area.offset as usize, bytes) {
            true => Ok(()),
            false => Err(Fault::Trap("delivery area: out of bounds".into())),
        }
    }

    /// Runs one export call with `host` and `budget`.
    fn run(
        &mut self,
        export: Export,
        budget: u64,
        host: &mut dyn SimHost,
        args: &[Arg],
    ) -> Result<u64, Fault> {
        // The store outlives this call but the host does not: erase the
        // borrow's lifetime and clear the pointer before returning.
        let host: NonNull<dyn SimHost + '_> = NonNull::from(host);
        // SAFETY: only the lifetime changes; `host` is cleared below.
        let host: NonNull<dyn SimHost> = unsafe { std::mem::transmute(host) };
        self.rt.with_ctx(|ctx| {
            ctx.host = Some(host);
            ctx.export = Some(export);
        });
        let result = invoke(&mut self.rt, export.name(), args, budget);
        self.rt.with_ctx(|ctx| {
            ctx.host = None;
            ctx.export = None;
        });
        self.read_globals();
        result
    }

    /// A copy of the module's linear memory, empty when it has none.
    pub fn memory(&self) -> Vec<u8> {
        let mut memory = vec![0u8; self.memory_len()];
        self.rt.read_memory(0, &mut memory);
        memory
    }

    /// The module's memory size in bytes.
    pub fn memory_len(&self) -> usize {
        self.rt.memory_len()
    }

    /// Copies module memory at `offset` into `buf`; false when that runs
    /// past the memory.
    pub fn read_memory(&self, offset: usize, buf: &mut [u8]) -> bool {
        self.rt.read_memory(offset, buf)
    }
}

/// Calls export `name` with `args` and `budget` operations, once the caller
/// has put its host into the context (and takes it out after). Returns the
/// operations used, or the fault: the first a host call raised, else what
/// stopped the module.
pub(crate) fn invoke<R: Runtime>(
    rt: &mut R,
    name: &str,
    args: &[Arg],
    budget: u64,
) -> Result<u64, Fault> {
    rt.with_ctx(|ctx| {
        ctx.fault = None;
        ctx.budget = budget;
    });
    let started = [(METER, budget), (DEPTH, 0), (STOP, 0)]
        .into_iter()
        .try_for_each(|(name, bits)| rt.set_global(name, bits));
    let result = match started {
        Ok(()) => rt.call(name, args),
        Err(_) => Err(TrapKind::Other),
    };
    let fault = rt.with_ctx(|ctx| ctx.fault.take());
    let used = budget.saturating_sub(rt.global(METER));
    let stop = rt.global(STOP) as u32 as i32;
    match result {
        Ok(()) => Ok(used),
        Err(trap) => Err(fault.unwrap_or_else(|| match stop {
            STOP_BUDGET => Fault::Budget,
            STOP_DEPTH => Fault::Trap(TrapKind::StackOverflow.text().into()),
            _ => Fault::Trap(trap.text().into()),
        })),
    }
}

/// Whether the module exports function `name`; one that does must take
/// `params` and return nothing.
pub(crate) fn export_fits(
    exports: &[(String, FuncType)],
    name: &str,
    params: &[ValType],
) -> Result<bool, LoadError> {
    let Some((_, ty)) = exports.iter().find(|(export, _)| export == name) else {
        return Ok(false);
    };
    match ty.params() == params && ty.results().is_empty() {
        true => Ok(true),
        false => Err(LoadError::Invalid(format!(
            "export {name} must have signature ({}) -> ()",
            params
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// The Wasm features modules may use (RFC section 3): MVP plus mutable
/// globals, sign extension, non-trapping float-to-int, bulk memory and
/// multi-value. `GC_TYPES` only admits the MVP `funcref` table type.
pub(crate) fn allowed_features() -> WasmFeatures {
    WasmFeatures::MUTABLE_GLOBAL
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::BULK_MEMORY
        | WasmFeatures::FLOATS
        | WasmFeatures::GC_TYPES
}

/// The RFC section 3 checks: the feature allowlist, no `start` section,
/// imports only from `world`'s import module with known names, and shape
/// limits. [`Sections::read`] reads the custom sections; a client module may
/// not declare `vera.vars` or `vera.notify`, which are the simulation's
/// (appendix "Custom sections", RFC 10 "Notifications").
pub(crate) fn check(bytes: &[u8], world: World) -> Result<(), LoadError> {
    let invalid = |why: String| LoadError::Invalid(why);
    if bytes.len() > MAX_MODULE_BYTES {
        return Err(invalid(format!(
            "module is {} bytes, limit {MAX_MODULE_BYTES}",
            bytes.len()
        )));
    }
    Validator::new_with_features(allowed_features())
        .validate_all(bytes)
        .map_err(|e| {
            invalid(format!(
                "uses a disallowed Wasm feature or is malformed: {e}"
            ))
        })?;
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.map_err(|e| invalid(e.to_string()))? {
            Payload::StartSection { .. } => {
                return Err(invalid(format!(
                    "has a start section; export {} instead",
                    world.init_export()
                )));
            }
            Payload::CustomSection(section)
                if world == World::Client
                    && matches!(section.name(), sections::VARS | sections::NOTIFY) =>
            {
                return Err(invalid(format!(
                    "a client module declares {}, which only simulation modules declare",
                    section.name()
                )));
            }
            Payload::ImportSection(imports) => {
                let module = world.import_module();
                for import in imports.into_imports() {
                    let import = import.map_err(|e| invalid(e.to_string()))?;
                    if import.module != module {
                        return Err(invalid(format!(
                            "imports {}::{}; only {module} is available",
                            import.module, import.name
                        )));
                    }
                    if !matches!(import.ty, TypeRef::Func(_)) {
                        return Err(invalid(format!("imports non-function {}", import.name)));
                    }
                    if !world.imports().iter().any(|(name, _)| *name == import.name) {
                        return Err(invalid(format!("imports unknown function {}", import.name)));
                    }
                }
            }
            Payload::FunctionSection(functions) if functions.count() > MAX_FUNCTIONS => {
                return Err(invalid(format!(
                    "has {} functions, limit {MAX_FUNCTIONS}",
                    functions.count()
                )));
            }
            Payload::CodeSectionEntry(body) if body.range().len() > MAX_BODY_BYTES => {
                return Err(invalid(format!(
                    "has a {}-byte function body",
                    body.range().len()
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// `vera.delivery` must hold at least `min_bytes` (one `EventRecord` for a
/// simulation module, [`crate::client::CLIENT_DELIVERY_MIN`] for a client
/// module), start 8-byte aligned (records are read in place) and lie inside
/// the initial memory. A module exporting `needed_by` must declare it.
pub(crate) fn check_delivery(
    delivery: Option<Delivery>,
    needed_by: Option<&str>,
    min_bytes: usize,
    memory_bytes: usize,
) -> Result<(), String> {
    let Some(area) = delivery else {
        return match needed_by {
            Some(export) => Err(format!("exports {export} without a vera.delivery section")),
            None => Ok(()),
        };
    };
    let end = u64::from(area.offset) + u64::from(area.bytes);
    if area.offset % 8 != 0 || (area.bytes as usize) < min_bytes {
        return Err(format!(
            "vera.delivery at {} of {} bytes is unaligned or smaller than {min_bytes} bytes",
            area.offset, area.bytes
        ));
    }
    if end > memory_bytes as u64 {
        return Err(format!(
            "vera.delivery ends at {end}, past the module's {memory_bytes}-byte memory"
        ));
    }
    Ok(())
}
