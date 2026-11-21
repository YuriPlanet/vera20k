//! Desktop runtime: Wasmtime with fuel metering and NaN canonicalization.
//!
//! Determinism: fuel counts Wasm operators the same way on every platform for
//! one Wasmtime version, and Cranelift's NaN canonicalization makes every
//! float result bit-identical across CPUs. Residuals for later steps: the
//! call-depth limit is Wasmtime's stack-size limit, which depends on the
//! platform's frame sizes (the RFC wants an instrumented depth counter), and
//! module memory is not saved yet.

use std::ops::Range;
use std::ptr::NonNull;
use std::sync::OnceLock;

use wasmparser::{Payload, TypeRef, Validator, WasmFeatures};
use wasmtime::{
    Caller, Config, Engine, Extern, Global, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, Trap, TypedFunc, Val, ValType,
};

use crate::decide::DecideRecord;
use crate::sections::{Delivery, Sections};
use crate::triggers::{ActionDef, NAME_MODULE_ACTION, TriggerDef};
use crate::{
    ActionParams, ENTRY_COST, EventRecord, Export, Fault, HOST_CALL_COST, IMPORTS, Invalid, Limits,
    LoadError, ModuleState, NAME_ACTION_KIND, NAME_KINDS, NAME_PROPERTY, Order, PAGE_BYTES, Query,
    SCAN_COST, SIM_IMPORT_MODULE, SimHost, Status, VarDecl, action_kind_id, action_kind_name,
    property_id, property_name,
};

/// Most records one `on_events` call receives (open question 7), within
/// what the delivery area holds.
pub const MAX_BATCH: usize = 256;

/// Load-time shape limits (RFC section 3, open question 7), so compiling a
/// downloaded map's module cannot stall the lobby.
const MAX_FUNCTIONS: u32 = 20_000;
const MAX_BODY_BYTES: usize = 1 << 20;
const MAX_MODULE_BYTES: usize = 8 << 20;

/// The Wasm features modules may use (RFC section 3): MVP plus mutable
/// globals, sign extension, non-trapping float-to-int, bulk memory and
/// multi-value. `GC_TYPES` only admits the MVP `funcref` table type.
fn allowed_features() -> WasmFeatures {
    WasmFeatures::MUTABLE_GLOBAL
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::BULK_MEMORY
        | WasmFeatures::FLOATS
        | WasmFeatures::GC_TYPES
}

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.cranelift_nan_canonicalization(true);
        config.wasm_simd(false);
        config.wasm_relaxed_simd(false);
        config.wasm_multi_memory(false);
        config.wasm_memory64(false);
        config.wasm_tail_call(false);
        // Threads and reference types are off in this feature set; `check`
        // enforces the full allowlist with wasmparser before compiling.
        // Only reachable on a broken host configuration; the engine is the
        // same for every module, so this is not module data.
        Engine::new(&config).expect("the module engine configuration is valid")
    })
}

/// The store's data: the host while a call runs, and the memory limit.
struct Ctx {
    /// Set only for the duration of [`SimModule::call`].
    host: Option<NonNull<dyn SimHost>>,
    limits: StoreLimits,
    /// The first invalid argument or budget fault a host call raised.
    fault: Option<Fault>,
    /// The module's custom sections; `lookup(ModuleAction)` reads them.
    sections: Sections,
    /// The export running, for imports unavailable in `vera_init`.
    export: Option<Export>,
}

// SAFETY: `host` is non-null only inside `SimModule::call`, which holds the
// `&mut dyn SimHost` it came from for its whole duration on this thread.
unsafe impl Send for Ctx {}
// SAFETY: `host` is only dereferenced through `&mut Store` inside a call;
// shared access to the store (`SimModule::memory`) never reads it.
unsafe impl Sync for Ctx {}

/// One loaded simulation module.
pub struct SimModule {
    /// The module as supplied, before load-time rewriting; saves carry it.
    bytes: Vec<u8>,
    store: Store<Ctx>,
    memory: Option<Memory>,
    exports: [Option<TypedFunc<(), ()>>; 4],
    on_events: Option<TypedFunc<(i32, i32), ()>>,
    on_decide: Option<TypedFunc<(i32, i32), ()>>,
    /// `vera.delivery`, checked against memory at load.
    delivery: Option<Delivery>,
    /// Every mutable global, in global index order (exported at load time).
    globals: Vec<Global>,
    /// Their raw bits after the latest call, so a save or hash reads them
    /// without a mutable store.
    global_bits: Vec<u64>,
}

impl SimModule {
    /// Checks, compiles and instantiates the module. No module code runs:
    /// modules with a `start` section are rejected.
    pub fn load(bytes: &[u8], limits: Limits) -> Result<Self, LoadError> {
        check(bytes)?;
        let sections = Sections::read(bytes).map_err(LoadError::Invalid)?;
        let delivery = sections.delivery;
        let (rewritten, mutable_globals) =
            export_mutable_globals(bytes).map_err(LoadError::Invalid)?;
        let engine = engine();
        let module =
            Module::new(engine, &rewritten).map_err(|e| LoadError::Invalid(format!("{e:#}")))?;
        let ctx = Ctx {
            host: None,
            limits: StoreLimitsBuilder::new()
                .memory_size(limits.memory_bytes)
                .memories(1)
                .tables(1)
                .instances(1)
                .build(),
            fault: None,
            sections,
            export: None,
        };
        let mut store = Store::new(engine, ctx);
        store.limiter(|ctx| &mut ctx.limits);
        // Instantiation runs no module code, but Wasmtime charges fuel for
        // initializing data segments. Each call sets its own budget.
        store
            .set_fuel(u64::MAX)
            .map_err(|e| LoadError::Invalid(format!("{e:#}")))?;
        let instance = linker(engine)
            .instantiate(&mut store, &module)
            .map_err(|e| LoadError::Invalid(format!("{e:#}")))?;
        let memory = instance.get_memory(&mut store, "memory");
        let mut exports = [None, None, None, None];
        for (slot, export) in exports.iter_mut().zip(Export::ALL) {
            *slot = typed_export(&instance, &mut store, export)?;
        }
        let on_events = records_export(&instance, &mut store, Export::Events)?;
        let on_decide = records_export(&instance, &mut store, Export::Decide)?;
        check_delivery(
            delivery,
            on_events.is_some() || on_decide.is_some(),
            memory.map_or(0, |m| m.data_size(&store)),
        )
        .map_err(LoadError::Invalid)?;
        // `on_decide` reads one `DecideRecord` in place.
        if on_decide.is_some()
            && delivery.is_some_and(|area| (area.bytes as usize) < DecideRecord::SIZE)
        {
            return Err(LoadError::Invalid(format!(
                "exports on_decide with a vera.delivery area smaller than one {}-byte DecideRecord",
                DecideRecord::SIZE
            )));
        }
        let globals = mutable_globals
            .iter()
            .map(|index| {
                instance
                    .get_global(&mut store, &global_export_name(*index))
                    .ok_or_else(|| LoadError::Invalid(format!("global {index} was not exported")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut module = Self {
            bytes: bytes.to_vec(),
            store,
            memory,
            exports,
            on_events,
            on_decide,
            delivery,
            globals,
            global_bits: Vec::new(),
        };
        module.read_globals();
        Ok(module)
    }

    /// Loads `bytes` and puts back a state taken with [`SimModule::state`].
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
        match module.memory {
            Some(memory) => {
                let have = memory.size(&module.store);
                let want = u64::from(state.pages);
                if want < have {
                    return Err(invalid(format!(
                        "{want} pages saved, module starts with {have}"
                    )));
                }
                memory
                    .grow(&mut module.store, want - have)
                    .map_err(|e| invalid(format!("memory cannot grow to {want} pages: {e}")))?;
                let data = memory.data_mut(&mut module.store);
                data.fill(0);
                for (page, bytes) in &state.nonzero_pages {
                    let start = *page as usize * PAGE_BYTES;
                    let Some(target) = data.get_mut(start..start + PAGE_BYTES) else {
                        return Err(invalid(format!("page {page} is outside {want} pages")));
                    };
                    if bytes.len() != PAGE_BYTES {
                        return Err(invalid(format!("page {page} holds {} bytes", bytes.len())));
                    }
                    target.copy_from_slice(bytes);
                }
            }
            None if state.pages == 0 && state.nonzero_pages.is_empty() => {}
            None => return Err(invalid("memory saved for a module without memory".into())),
        }
        for (global, bits) in module.globals.clone().iter().zip(&state.globals) {
            let value = match global.ty(&module.store).content() {
                ValType::I32 => Val::I32(*bits as u32 as i32),
                ValType::I64 => Val::I64(*bits as i64),
                ValType::F32 => Val::F32(*bits as u32),
                ValType::F64 => Val::F64(*bits),
                other => return Err(invalid(format!("global of type {other}"))),
            };
            global
                .set(&mut module.store, value)
                .map_err(|e| invalid(format!("global cannot be set: {e}")))?;
        }
        module.read_globals();
        Ok(module)
    }

    /// The module as supplied to [`SimModule::load`].
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The variables the module's `vera.vars` section declares.
    pub fn vars(&self) -> &[VarDecl] {
        &self.store.data().sections.vars
    }

    /// The dispatch key the module's `vera.actions` gives `name` (INI
    /// `Wasm:name` actions).
    pub fn action_key(&self, name: &str) -> Option<i32> {
        self.store.data().sections.action_key(name)
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
            pages: (memory.len() / PAGE_BYTES) as u32,
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
        let globals = self.globals.clone();
        self.global_bits = globals
            .iter()
            .map(|global| match global.get(&mut self.store) {
                Val::I32(v) => u64::from(v as u32),
                Val::I64(v) => v as u64,
                Val::F32(bits) => u64::from(bits),
                Val::F64(bits) => bits,
                // MVP globals hold only numbers; `check` admits no other.
                _ => 0,
            })
            .collect();
    }

    pub fn has_export(&self, export: Export) -> bool {
        match export {
            Export::Events => self.on_events.is_some(),
            Export::Decide => self.on_decide.is_some(),
            _ => self
                .exports
                .get(export as usize)
                .is_some_and(Option::is_some),
        }
    }

    /// How many records one `on_events` call can take: 0 without the export,
    /// else what `vera.delivery` holds, at most [`MAX_BATCH`].
    pub fn batch_capacity(&self) -> usize {
        match (self.on_events.is_some(), self.delivery) {
            (true, Some(area)) => (area.bytes as usize / EventRecord::SIZE).min(MAX_BATCH),
            _ => 0,
        }
    }

    /// Calls `export` with `budget` operations. Returns the operations used.
    /// A missing export costs nothing; [`Export::Events`] goes through
    /// [`SimModule::deliver`].
    pub fn call(
        &mut self,
        export: Export,
        budget: u64,
        host: &mut dyn SimHost,
    ) -> Result<u64, Fault> {
        let Some(func) = self.exports.get(export as usize).cloned().flatten() else {
            return Ok(0);
        };
        self.run(export, budget, host, |store| func.call(store, ()))
    }

    /// Writes `records` into the delivery area and calls `on_events` on
    /// them with `budget` operations. At most [`SimModule::batch_capacity`]
    /// records; an empty batch makes no call.
    pub fn deliver(
        &mut self,
        records: &[EventRecord],
        budget: u64,
        host: &mut dyn SimHost,
    ) -> Result<u64, Fault> {
        let (Some(func), Some(area), Some(memory)) =
            (self.on_events.clone(), self.delivery, self.memory)
        else {
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
        // `check_delivery` placed the area inside memory at load, and memory
        // never shrinks.
        memory
            .write(&mut self.store, area.offset as usize, &bytes)
            .map_err(|e| Fault::Trap(format!("delivery area: {e}")))?;
        let (ptr, count) = (area.offset as i32, records.len() as i32);
        self.run(Export::Events, budget, host, |store| {
            func.call(store, (ptr, count))
        })
    }

    /// Writes `record` into the delivery area and calls `on_decide(ptr, 1)`
    /// with `budget` operations (RFC 6f). Returns the handler's answer
    /// ([`DecideRecord::answer`]) and the operations used. Without the
    /// export no call is made and the answer is `None`. During the call a
    /// write import stops the module.
    pub fn decide(
        &mut self,
        record: &DecideRecord,
        budget: u64,
        host: &mut dyn SimHost,
    ) -> Result<(Option<i64>, u64), Fault> {
        let (Some(func), Some(area), Some(memory)) =
            (self.on_decide.clone(), self.delivery, self.memory)
        else {
            return Ok((None, 0));
        };
        let offset = area.offset as usize;
        // Load checked the area holds a record inside memory, which never
        // shrinks.
        memory
            .write(&mut self.store, offset, &record.to_bytes())
            .map_err(|e| Fault::Trap(format!("delivery area: {e}")))?;
        let ptr = area.offset as i32;
        let used = self.run(Export::Decide, budget, host, |store| {
            func.call(store, (ptr, 1))
        })?;
        let mut bytes = [0u8; DecideRecord::SIZE];
        memory
            .read(&self.store, offset, &mut bytes)
            .map_err(|e| Fault::Trap(format!("delivery area: {e}")))?;
        Ok((DecideRecord::answer(&bytes), used))
    }

    /// Runs one export call with `host` and `budget`.
    fn run(
        &mut self,
        export: Export,
        budget: u64,
        host: &mut dyn SimHost,
        call: impl FnOnce(&mut Store<Ctx>) -> wasmtime::Result<()>,
    ) -> Result<u64, Fault> {
        // The store outlives this call but the host does not: erase the
        // borrow's lifetime and clear the pointer before returning.
        let host: NonNull<dyn SimHost + '_> = NonNull::from(host);
        // SAFETY: only the lifetime changes; `host` is cleared below.
        let host: NonNull<dyn SimHost> = unsafe { std::mem::transmute(host) };
        let data = self.store.data_mut();
        data.host = Some(host);
        data.fault = None;
        data.export = Some(export);
        let fueled = self.store.set_fuel(budget);
        let result = match fueled {
            Ok(()) => call(&mut self.store),
            Err(e) => Err(e),
        };
        let data = self.store.data_mut();
        data.host = None;
        data.export = None;
        let fault = data.fault.take();
        let used = budget.saturating_sub(self.store.get_fuel().unwrap_or(0));
        self.read_globals();
        match result {
            Ok(()) => Ok(used),
            Err(e) => Err(fault.unwrap_or_else(|| match e.downcast_ref::<Trap>() {
                Some(Trap::OutOfFuel) => Fault::Budget,
                Some(trap) => Fault::Trap(trap.to_string()),
                None => Fault::Trap(format!("{e:#}")),
            })),
        }
    }

    /// The module's linear memory, empty when it has none.
    pub fn memory(&self) -> &[u8] {
        self.memory.map_or(&[], |m| m.data(&self.store))
    }
}

/// The RFC section 3 checks Wasmtime does not make itself: the feature
/// allowlist, no `start` section, imports only from `vera:sim@1.0` with known
/// names, and shape limits. [`Sections::read`] reads the custom sections.
fn check(bytes: &[u8]) -> Result<(), LoadError> {
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
                return Err(invalid(
                    "has a start section; export vera_init instead".into(),
                ));
            }
            Payload::ImportSection(imports) => {
                for import in imports.into_imports() {
                    let import = import.map_err(|e| invalid(e.to_string()))?;
                    if import.module != SIM_IMPORT_MODULE {
                        return Err(invalid(format!(
                            "imports {}::{}; only {SIM_IMPORT_MODULE} is available",
                            import.module, import.name
                        )));
                    }
                    if !matches!(import.ty, TypeRef::Func(_)) {
                        return Err(invalid(format!("imports non-function {}", import.name)));
                    }
                    if !IMPORTS.iter().any(|(name, _)| *name == import.name) {
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

/// `vera.delivery` must hold at least one `EventRecord`, start 8-byte
/// aligned (records are read in place) and lie inside the initial memory. A
/// module exporting `on_events` must declare it.
fn check_delivery(
    delivery: Option<Delivery>,
    on_events: bool,
    memory_bytes: usize,
) -> Result<(), String> {
    let Some(area) = delivery else {
        return match on_events {
            true => Err("exports on_events without a vera.delivery section".into()),
            false => Ok(()),
        };
    };
    let end = u64::from(area.offset) + u64::from(area.bytes);
    if area.offset % 8 != 0 || (area.bytes as usize) < EventRecord::SIZE {
        return Err(format!(
            "vera.delivery at {} of {} bytes is unaligned or smaller than one record",
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

/// An `(ptr, count)` export (`on_events`, `on_decide`), if the module has
/// it.
fn records_export(
    instance: &Instance,
    store: &mut Store<Ctx>,
    export: Export,
) -> Result<Option<TypedFunc<(i32, i32), ()>>, LoadError> {
    let Some(func) = instance.get_func(&mut *store, export.name()) else {
        return Ok(None);
    };
    func.typed::<(i32, i32), ()>(&*store)
        .map(Some)
        .map_err(|_| {
            LoadError::Invalid(format!(
                "export {} must have signature (i32, i32) -> ()",
                export.name()
            ))
        })
}

fn typed_export(
    instance: &Instance,
    store: &mut Store<Ctx>,
    export: Export,
) -> Result<Option<TypedFunc<(), ()>>, LoadError> {
    let Some(func) = instance.get_func(&mut *store, export.name()) else {
        return Ok(None);
    };
    func.typed::<(), ()>(&*store).map(Some).map_err(|_| {
        LoadError::Invalid(format!(
            "export {} must have signature () -> ()",
            export.name()
        ))
    })
}

/// Charges one host call and hands back the host. A host call that runs past
/// the budget stops the module as a budget fault.
fn enter<'a>(caller: &'a mut Caller<'_, Ctx>, extra: u64) -> wasmtime::Result<&'a mut dyn SimHost> {
    charge(caller, HOST_CALL_COST.saturating_add(extra))?;
    host(caller)
}

fn host<'a>(caller: &'a mut Caller<'_, Ctx>) -> wasmtime::Result<&'a mut dyn SimHost> {
    match caller.data().host {
        // SAFETY: see `Ctx::host`.
        Some(mut host) => Ok(unsafe { host.as_mut() }),
        None => Err(wasmtime::Error::msg("host call outside a module call")),
    }
}

/// Takes `cost` operations from the call's budget; running out is a budget
/// fault.
fn charge(caller: &mut Caller<'_, Ctx>, cost: u64) -> wasmtime::Result<()> {
    let fuel = caller.get_fuel()?;
    if fuel < cost {
        caller.data_mut().fault = Some(Fault::Budget);
        return Err(wasmtime::Error::msg(
            "operation budget exceeded in a host call",
        ));
    }
    caller.set_fuel(fuel - cost)?;
    Ok(())
}

/// Checks that `count` entries of `size` bytes at `ptr` lie in module memory.
fn check_buffer(
    caller: &mut Caller<'_, Ctx>,
    ptr: i32,
    count: usize,
    size: usize,
) -> Result<(Memory, Range<usize>), Invalid> {
    let memory = memory_of(caller)?;
    let range = usize::try_from(ptr).ok().and_then(|start| {
        let end = start.checked_add(count.checked_mul(size)?)?;
        (end <= memory.data_size(&*caller)).then_some(start..end)
    });
    range.map(|range| (memory, range)).ok_or_else(|| {
        Invalid(format!(
            "buffer {ptr} of {count} x {size} bytes is outside module memory"
        ))
    })
}

/// A count or capacity argument, which may not be negative.
fn count_arg(value: i32, what: &str) -> Result<usize, Invalid> {
    usize::try_from(value).map_err(|_| Invalid(format!("negative {what} {value}")))
}

fn fail(caller: &mut Caller<'_, Ctx>, why: Invalid) -> wasmtime::Error {
    let message = why.0.clone();
    caller.data_mut().fault.get_or_insert(Fault::Invalid(why.0));
    wasmtime::Error::msg(message)
}

fn memory_of(caller: &mut Caller<'_, Ctx>) -> Result<Memory, Invalid> {
    match caller.get_export("memory") {
        Some(Extern::Memory(m)) => Ok(m),
        _ => Err(Invalid("the module exports no memory".into())),
    }
}

fn input_buffer(
    caller: &mut Caller<'_, Ctx>,
    ptr: i32,
    len: i32,
) -> wasmtime::Result<(Memory, Range<usize>)> {
    let len = count_arg(len, "buffer length").map_err(|e| fail(caller, e))?;
    check_buffer(caller, ptr, len, 1).map_err(|e| fail(caller, e))
}

/// Variable-size copies use both the guest memory bounds and the fuel budget.
fn copy_input(caller: &mut Caller<'_, Ctx>, ptr: i32, len: i32) -> wasmtime::Result<Vec<u8>> {
    let (memory, range) = input_buffer(caller, ptr, len)?;
    charge(caller, range.len() as u64)?;
    Ok(memory.data(&*caller)[range].to_vec())
}

/// `trigger_create` copies each action's `text` out of module memory into
/// the definition (appendix "Fixed layouts"). Text
/// that is not UTF-8 or longer than [`ActionDef::MAX_TEXT`] is invalid.
fn read_action_texts(caller: &mut Caller<'_, Ctx>, def: &mut TriggerDef) -> wasmtime::Result<()> {
    for action in &mut def.actions {
        let len = action.params.text_len;
        if len == 0 {
            continue;
        }
        if usize::try_from(len).is_ok_and(|len| len > ActionDef::MAX_TEXT) {
            return Err(fail(
                caller,
                Invalid(format!(
                    "trigger action text of {len} bytes, at most {}",
                    ActionDef::MAX_TEXT
                )),
            ));
        }
        action.text = read_str(caller, action.params.text, len)?;
    }
    Ok(())
}

fn write_bytes(caller: &mut Caller<'_, Ctx>, ptr: i32, bytes: &[u8]) -> Result<(), Invalid> {
    let memory = memory_of(caller)?;
    let Ok(offset) = usize::try_from(ptr) else {
        return Err(Invalid(format!("negative out pointer {ptr}")));
    };
    memory
        .write(&mut *caller, offset, bytes)
        .map_err(|_| Invalid(format!("out pointer {ptr} is outside module memory")))
}

/// The `vera:sim@1.0` imports, each a thin adapter onto [`SimHost`].
fn linker(engine: &Engine) -> Linker<Ctx> {
    let mut linker = Linker::new(engine);
    let m = SIM_IMPORT_MODULE;
    let wire = |r: wasmtime::Result<&mut Linker<Ctx>>| {
        r.expect("each import is defined once with a valid signature");
    };
    wire(linker.func_wrap(
        m,
        "frame",
        |mut caller: Caller<'_, Ctx>| -> wasmtime::Result<i32> {
            Ok(enter(&mut caller, 0)?.frame())
        },
    ));
    wire(linker.func_wrap(
        m,
        "random",
        |mut caller: Caller<'_, Ctx>, lo: i32, hi: i32, out: i32| -> wasmtime::Result<i32> {
            if lo > hi {
                return Err(fail(
                    &mut caller,
                    Invalid(format!("random({lo}, {hi}) has lo > hi")),
                ));
            }
            let value = enter(&mut caller, 0)?.random(lo, hi);
            write_bytes(&mut caller, out, &value.to_le_bytes())
                .map_err(|e| fail(&mut caller, e))?;
            Ok(Status::Ok as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "log",
        |mut caller: Caller<'_, Ctx>, ptr: i32, len: i32| -> wasmtime::Result<()> {
            charge(&mut caller, HOST_CALL_COST)?;
            let bytes = copy_input(&mut caller, ptr, len)?;
            let line = String::from_utf8_lossy(&bytes);
            host(&mut caller)?.log(&line);
            Ok(())
        },
    ));
    wire(linker.func_wrap(
        m,
        "has_kind",
        |mut caller: Caller<'_, Ctx>, table: i32, id: i32| -> wasmtime::Result<i32> {
            Ok(i32::from(enter(&mut caller, 0)?.has_kind(table, id)))
        },
    ));
    wire(linker.func_wrap(
        m,
        "lookup",
        |mut caller: Caller<'_, Ctx>,
         kind: i32,
         ptr: i32,
         len: i32,
         out: i32|
         -> wasmtime::Result<i32> {
            if !(0..NAME_KINDS).contains(&kind) {
                return Err(fail(
                    &mut caller,
                    Invalid(format!("unknown name kind {kind}")),
                ));
            }
            charge(&mut caller, HOST_CALL_COST)?;
            let bytes = copy_input(&mut caller, ptr, len)?;
            // This module's own `vera.actions` answers ModuleAction.
            let module_action = std::str::from_utf8(&bytes)
                .ok()
                .filter(|_| kind == NAME_MODULE_ACTION)
                .map(|name| caller.data().sections.action_key(name));
            let host = host(&mut caller)?;
            // A name that is not UTF-8 names nothing.
            let result = match std::str::from_utf8(&bytes) {
                Err(_) => Ok((-1, Status::NotFound)),
                Ok(name) if kind == NAME_PROPERTY => {
                    Ok(property_id(name).map_or((-1, Status::NotFound), |id| (id, Status::Ok)))
                }
                Ok(_) if kind == NAME_MODULE_ACTION => Ok(module_action
                    .flatten()
                    .map_or((-1, Status::NotFound), |key| (key, Status::Ok))),
                Ok(name) if kind == NAME_ACTION_KIND => {
                    Ok(action_kind_id(name).map_or((-1, Status::NotFound), |id| (id, Status::Ok)))
                }
                Ok(name) => host.lookup(kind, name),
            };
            let (id, status) = result.map_err(|e| fail(&mut caller, e))?;
            let id = if status == Status::Ok { id } else { -1 };
            write_bytes(&mut caller, out, &id.to_le_bytes()).map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "name_of",
        |mut caller: Caller<'_, Ctx>,
         kind: i32,
         id: i32,
         out: i32,
         cap: i32,
         len_out: i32|
         -> wasmtime::Result<i32> {
            let cap = count_arg(cap, "capacity").map_err(|e| fail(&mut caller, e))?;
            if !(0..NAME_KINDS).contains(&kind) {
                return Err(fail(
                    &mut caller,
                    Invalid(format!("unknown name kind {kind}")),
                ));
            }
            let module_action = (kind == NAME_MODULE_ACTION)
                .then(|| caller.data().sections.action_name(id).map(str::to_owned));
            let host = enter(&mut caller, 0)?;
            let result = if let Some(name) = module_action {
                // Any i32 is a dispatch key; one `vera.actions` lacks has no name.
                Ok(name.map_or((String::new(), Status::NotFound), |n| (n, Status::Ok)))
            } else if id == -1 {
                Ok((String::new(), Status::NotFound))
            } else if kind == NAME_PROPERTY {
                property_name(id)
                    .map(|name| (name.to_owned(), Status::Ok))
                    .ok_or_else(|| Invalid(format!("unknown property {id}")))
            } else if kind == NAME_ACTION_KIND {
                // A stock number without a name (0, 39, 49, 50) is NotFound.
                action_kind_name(id)
                    .map(|name| {
                        name.map_or((String::new(), Status::NotFound), |n| {
                            (n.to_owned(), Status::Ok)
                        })
                    })
                    .ok_or_else(|| Invalid(format!("unknown action kind {id}")))
            } else {
                host.name_of(kind, id)
            };
            let (name, status) = result.map_err(|e| fail(&mut caller, e))?;
            let name = if status == Status::Ok {
                name
            } else {
                String::new()
            };
            charge(&mut caller, name.len() as u64)?;
            // A name longer than `cap` reports its length and writes nothing.
            if name.len() <= cap {
                write_bytes(&mut caller, out, name.as_bytes()).map_err(|e| fail(&mut caller, e))?;
            }
            let len = name.len() as i32;
            write_bytes(&mut caller, len_out, &len.to_le_bytes())
                .map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "get",
        |mut caller: Caller<'_, Ctx>,
         reference: i64,
         prop: i32,
         out: i32|
         -> wasmtime::Result<i32> {
            if property_name(prop).is_none() {
                return Err(fail(
                    &mut caller,
                    Invalid(format!("unknown property {prop}")),
                ));
            }
            let result = enter(&mut caller, 0)?.get(reference, prop);
            let (value, status) = result.map_err(|e| fail(&mut caller, e))?;
            let value = if status == Status::Ok { value } else { 0 };
            write_bytes(&mut caller, out, &value.to_le_bytes())
                .map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "get_many",
        |mut caller: Caller<'_, Ctx>,
         refs: i32,
         n: i32,
         props: i32,
         m: i32,
         out: i32,
         count_out: i32|
         -> wasmtime::Result<i32> {
            get_many(&mut caller, refs, n, props, m, out, count_out)
        },
    ));
    wire(linker.func_wrap(
        m,
        "set",
        |mut caller: Caller<'_, Ctx>,
         reference: i64,
         prop: i32,
         value: i64|
         -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "set")?;
            if property_name(prop).is_none() {
                return Err(fail(
                    &mut caller,
                    Invalid(format!("unknown property {prop}")),
                ));
            }
            let result = enter(&mut caller, 0)?.set(reference, prop, value);
            Ok(result.map_err(|e| fail(&mut caller, e))? as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "modify",
        |mut caller: Caller<'_, Ctx>,
         reference: i64,
         prop: i32,
         op: i32,
         value: i64,
         frames: i32,
         source: i64,
         group: i32,
         out: i32|
         -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "modify")?;
            if property_name(prop).is_none() || !(0..=2).contains(&op) {
                return Err(fail(
                    &mut caller,
                    Invalid(format!("modify: property {prop}, op {op}")),
                ));
            }
            let result =
                enter(&mut caller, 0)?.modify(reference, prop, op, value, frames, source, group);
            let (handle, status) = result.map_err(|e| fail(&mut caller, e))?;
            let handle = if status == Status::Ok { handle } else { 0 };
            write_bytes(&mut caller, out, &handle.to_le_bytes())
                .map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "unmodify",
        |mut caller: Caller<'_, Ctx>, modifier: i64| -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "unmodify")?;
            let result = enter(&mut caller, 0)?.unmodify(modifier);
            Ok(result.map_err(|e| fail(&mut caller, e))? as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "find",
        |mut caller: Caller<'_, Ctx>,
         query: i32,
         len: i32,
         out: i32,
         cap: i32,
         res: i32|
         -> wasmtime::Result<i32> { find(&mut caller, query, len, out, cap, res) },
    ));
    wire(linker.func_wrap(
        m,
        "var_get",
        |mut caller: Caller<'_, Ctx>, scope: i64, var: i32, out: i32| -> wasmtime::Result<i32> {
            let result = enter(&mut caller, 0)?.var_get(scope, var);
            let (value, status) = result.map_err(|e| fail(&mut caller, e))?;
            write_bytes(&mut caller, out, &value.to_le_bytes())
                .map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "var_set",
        |mut caller: Caller<'_, Ctx>, scope: i64, var: i32, value: i64| -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "var_set")?;
            let result = enter(&mut caller, 0)?.var_set(scope, var, value);
            Ok(result.map_err(|e| fail(&mut caller, e))? as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "action",
        |mut caller: Caller<'_, Ctx>,
         kind: i32,
         target: i64,
         ptr: i32,
         len: i32,
         out: i32|
         -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "action")?;
            let (memory, range) = input_buffer(&mut caller, ptr, len)?;
            charge(&mut caller, HOST_CALL_COST)?;
            let params = ActionParams::parse(&memory.data(&caller)[range])
                .map_err(|e| fail(&mut caller, e))?;
            let text = match params.text_len {
                0 => String::new(),
                len => read_str(&mut caller, params.text, len)?,
            };
            let result = host(&mut caller)?.action(kind, target, &params, &text);
            let (handle, status) = result.map_err(|e| fail(&mut caller, e))?;
            write_bytes(&mut caller, out, &handle.to_le_bytes())
                .map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "trigger_create",
        |mut caller: Caller<'_, Ctx>, ptr: i32, len: i32, out: i32| -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "trigger_create")?;
            let (memory, range) = input_buffer(&mut caller, ptr, len)?;
            check_buffer(&mut caller, out, 1, 8).map_err(|e| fail(&mut caller, e))?;
            let (handle, status) = if in_init(&caller) {
                (0, Status::Unsupported)
            } else {
                charge(&mut caller, HOST_CALL_COST + range.len() as u64)?;
                let mut def = TriggerDef::parse(&memory.data(&caller)[range])
                    .map_err(|e| fail(&mut caller, e))?;
                read_action_texts(&mut caller, &mut def)?;
                let result = host(&mut caller)?.trigger_create(&def);
                result.map_err(|e| fail(&mut caller, e))?
            };
            let handle = if status == Status::Ok { handle } else { 0 };
            write_bytes(&mut caller, out, &handle.to_le_bytes())
                .map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "trigger_remove",
        |mut caller: Caller<'_, Ctx>, trigger: i64| -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "trigger_remove")?;
            if in_init(&caller) {
                return Ok(Status::Unsupported as i32);
            }
            let result = enter(&mut caller, 0)?.trigger_remove(trigger);
            Ok(result.map_err(|e| fail(&mut caller, e))? as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "trigger_bind",
        |mut caller: Caller<'_, Ctx>, trigger: i64, reference: i64| -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "trigger_bind")?;
            if in_init(&caller) {
                return Ok(Status::Unsupported as i32);
            }
            let result = enter(&mut caller, 0)?.trigger_bind(trigger, reference);
            Ok(result.map_err(|e| fail(&mut caller, e))? as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "trigger_unbind",
        |mut caller: Caller<'_, Ctx>, trigger: i64, reference: i64| -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "trigger_unbind")?;
            if in_init(&caller) {
                return Ok(Status::Unsupported as i32);
            }
            let result = enter(&mut caller, 0)?.trigger_unbind(trigger, reference);
            Ok(result.map_err(|e| fail(&mut caller, e))? as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "rules_get",
        |mut caller: Caller<'_, Ctx>,
         sec: i32,
         sec_len: i32,
         key: i32,
         key_len: i32,
         out: i32,
         cap: i32,
         len_out: i32|
         -> wasmtime::Result<i32> {
            let cap = count_arg(cap, "capacity").map_err(|e| fail(&mut caller, e))?;
            charge(&mut caller, HOST_CALL_COST)?;
            let section = read_str(&mut caller, sec, sec_len)?;
            let key = read_str(&mut caller, key, key_len)?;
            let result = host(&mut caller)?.rules_get(&section, &key);
            let (value, status) = result.map_err(|e| fail(&mut caller, e))?;
            let (value, status) = match (value, status) {
                (Some(value), Status::Ok) => (value, Status::Ok),
                (None, Status::Ok) => (String::new(), Status::NotFound),
                (_, status) => (String::new(), status),
            };
            charge(&mut caller, value.len() as u64)?;
            // A value longer than `cap` reports its length and writes nothing;
            // a missing key reports -1.
            if value.len() <= cap {
                write_bytes(&mut caller, out, value.as_bytes())
                    .map_err(|e| fail(&mut caller, e))?;
            }
            let len = if status == Status::NotFound {
                -1
            } else {
                value.len() as i32
            };
            write_bytes(&mut caller, len_out, &len.to_le_bytes())
                .map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "rules_set",
        |mut caller: Caller<'_, Ctx>,
         sec: i32,
         sec_len: i32,
         key: i32,
         key_len: i32,
         val: i32,
         val_len: i32|
         -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "rules_set")?;
            charge(&mut caller, HOST_CALL_COST)?;
            let section = read_str(&mut caller, sec, sec_len)?;
            let key = read_str(&mut caller, key, key_len)?;
            let value = read_str(&mut caller, val, val_len)?;
            let result = host(&mut caller)?.rules_set(&section, &key, &value);
            Ok(result.map_err(|e| fail(&mut caller, e))? as i32)
        },
    ));
    wire(linker.func_wrap(
        m,
        "order",
        |mut caller: Caller<'_, Ctx>, house: i64, ptr: i32, len: i32| -> wasmtime::Result<i32> {
            deny_in_decide(&mut caller, "order")?;
            let (memory, range) = input_buffer(&mut caller, ptr, len)?;
            let unit_bytes = range.len().saturating_sub(Order::HEADER);
            // Charged per order and per unit entry (RFC 6g).
            let cost = (unit_bytes / size_of::<i64>()).min(Order::MAX_UNITS) as u64 * ENTRY_COST;
            charge(&mut caller, HOST_CALL_COST + cost)?;
            let order =
                Order::parse(&memory.data(&caller)[range]).map_err(|e| fail(&mut caller, e))?;
            let result = host(&mut caller)?.order(house, &order);
            Ok(result.map_err(|e| fail(&mut caller, e))? as i32)
        },
    ));
    linker
}

/// A section, key or value name: UTF-8 text in module memory. Text that is
/// not UTF-8 is an invalid argument.
fn read_str(caller: &mut Caller<'_, Ctx>, ptr: i32, len: i32) -> wasmtime::Result<String> {
    String::from_utf8(copy_input(caller, ptr, len)?)
        .map_err(|_| fail(caller, Invalid(format!("text at {ptr}+{len} is not UTF-8"))))
}

/// Write imports are unavailable in `on_decide` (appendix 1.1, "decide"):
/// calling one is an invalid call, which stops the module.
fn deny_in_decide(caller: &mut Caller<'_, Ctx>, import: &str) -> wasmtime::Result<()> {
    match caller.data().export {
        Some(Export::Decide) => Err(fail(
            caller,
            Invalid(format!("{import} is a write, unavailable in on_decide")),
        )),
        _ => Ok(()),
    }
}

/// Triggers are unavailable in `vera_init` (appendix 1.1, "all").
fn in_init(caller: &Caller<'_, Ctx>) -> bool {
    caller.data().export == Some(Export::Init)
}

/// `get_many` (appendix "Other layouts"): `n x m` scalar reads, row-major,
/// each with its own empty value; returns the first non-`Ok` status.
fn get_many(
    caller: &mut Caller<'_, Ctx>,
    refs: i32,
    n: i32,
    props: i32,
    m: i32,
    out: i32,
    count_out: i32,
) -> wasmtime::Result<i32> {
    let n = count_arg(n, "reference count").map_err(|e| fail(caller, e))?;
    let m = count_arg(m, "property count").map_err(|e| fail(caller, e))?;
    let total = n
        .checked_mul(m)
        .ok_or_else(|| fail(caller, Invalid(format!("{n} x {m} values"))))?;
    check_buffer(caller, out, total, 8).map_err(|e| fail(caller, e))?;
    let (memory, refs) = check_buffer(caller, refs, n, 8).map_err(|e| fail(caller, e))?;
    let (_, props) = check_buffer(caller, props, m, 4).map_err(|e| fail(caller, e))?;
    // A query with no references still validates each property. Without
    // properties there is no need to read or allocate the reference list.
    let cost = (total.max(m) as u64).saturating_mul(ENTRY_COST);
    charge(caller, HOST_CALL_COST.saturating_add(cost))?;
    let data = memory.data(&*caller);
    let refs: Vec<i64> = if m == 0 {
        Vec::new()
    } else {
        data[refs]
            .chunks_exact(8)
            .map(|c| i64::from_le_bytes(c.try_into().unwrap_or_default()))
            .collect()
    };
    let props: Vec<i32> = data[props]
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap_or_default()))
        .collect();
    if let Some(bad) = props.iter().find(|p| property_name(**p).is_none()) {
        return Err(fail(caller, Invalid(format!("unknown property {bad}"))));
    }
    let host = host(caller)?;
    let mut first = Status::Ok;
    let mut values = Vec::with_capacity(total * 8);
    let mut result = Ok(());
    'rows: for reference in &refs {
        for prop in &props {
            match host.get(*reference, *prop) {
                Ok((value, status)) => {
                    if first == Status::Ok {
                        first = status;
                    }
                    let value = if status == Status::Ok { value } else { 0 };
                    values.extend_from_slice(&value.to_le_bytes());
                }
                Err(e) => {
                    result = Err(e);
                    break 'rows;
                }
            }
        }
    }
    result.map_err(|e| fail(caller, e))?;
    write_bytes(caller, out, &values).map_err(|e| fail(caller, e))?;
    write_bytes(caller, count_out, &(total as i32).to_le_bytes()).map_err(|e| fail(caller, e))?;
    Ok(first as i32)
}

/// `find`: runs a `Query`, writes references to `out` and a `FindResult`
/// (count, reserved, next) to `res` ("Strings and buffers").
fn find(
    caller: &mut Caller<'_, Ctx>,
    query: i32,
    len: i32,
    out: i32,
    cap: i32,
    res: i32,
) -> wasmtime::Result<i32> {
    let cap = count_arg(cap, "capacity").map_err(|e| fail(caller, e))?;
    let (memory, range) = input_buffer(caller, query, len)?;
    check_buffer(caller, out, cap, 8).map_err(|e| fail(caller, e))?;
    check_buffer(caller, res, 1, 16).map_err(|e| fail(caller, e))?;
    charge(caller, HOST_CALL_COST)?;
    let query = Query::parse(&memory.data(&*caller)[range]).map_err(|e| fail(caller, e))?;
    let write_result = |caller: &mut Caller<'_, Ctx>, count: usize, next: i64| {
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(&(count as i32).to_le_bytes());
        bytes[8..].copy_from_slice(&next.to_le_bytes());
        write_bytes(caller, res, &bytes).map_err(|e| fail(caller, e))
    };
    // A valid zero-capacity query scans nothing.
    if cap == 0 {
        write_result(caller, 0, 0)?;
        return Ok(Status::Ok as i32);
    }
    let result = host(caller)?.find(&query, cap);
    let (page, status) = result.map_err(|e| fail(caller, e))?;
    if page.found.len() > cap {
        return Err(fail(
            caller,
            Invalid("host returned more than a page".into()),
        ));
    }
    let cost = page
        .scanned
        .saturating_mul(SCAN_COST)
        .saturating_add((page.found.len() as u64).saturating_mul(ENTRY_COST));
    charge(caller, cost)?;
    if status != Status::Ok {
        write_result(caller, 0, 0)?;
        return Ok(status as i32);
    }
    let refs: Vec<u8> = page
        .found
        .iter()
        .flat_map(|(_, r)| r.to_le_bytes())
        .collect();
    write_bytes(caller, out, &refs).map_err(|e| fail(caller, e))?;
    // A full page continues after its last key; the next page may be empty.
    let next = match page.found.last() {
        Some((key, _)) if page.found.len() == cap => key.saturating_add(1),
        _ => 0,
    };
    write_result(caller, page.found.len(), next)?;
    Ok(Status::Ok as i32)
}

fn global_export_name(index: u32) -> String {
    format!("__vera_g{index}")
}

/// RFC section 3 step 4: export every mutable global (a compiler's stack
/// pointer, for instance) as `__vera_g<index>`, so saves and the state hash
/// can read and write it. Returns the rewritten module and the mutable
/// globals' indices. Sections are copied unchanged apart from the export
/// section.
fn export_mutable_globals(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u32>), String> {
    use wasm_encoder::{ExportKind, ExportSection, RawSection};
    use wasmparser::ExternalKind;

    let mut mutable = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let Payload::GlobalSection(reader) = payload.map_err(|e| e.to_string())? {
            for (index, global) in reader.into_iter().enumerate() {
                if global.map_err(|e| e.to_string())?.ty.mutable {
                    mutable.push(index as u32);
                }
            }
        }
    }
    let ours = |section: &mut ExportSection| {
        for index in &mutable {
            section.export(&global_export_name(*index), ExportKind::Global, *index);
        }
    };
    let mut out = wasm_encoder::Module::new();
    let mut exported = false;
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(|e| e.to_string())?;
        if let Payload::ExportSection(reader) = &payload {
            let mut section = ExportSection::new();
            for export in reader.clone() {
                let export = export.map_err(|e| e.to_string())?;
                if export.name.starts_with("__vera_g") {
                    return Err(format!("exports reserved name {}", export.name));
                }
                let kind = match export.kind {
                    ExternalKind::Func => ExportKind::Func,
                    ExternalKind::Table => ExportKind::Table,
                    ExternalKind::Memory => ExportKind::Memory,
                    ExternalKind::Global => ExportKind::Global,
                    other => return Err(format!("export {} of kind {other:?}", export.name)),
                };
                section.export(export.name, kind, export.index);
            }
            ours(&mut section);
            out.section(&section);
            exported = true;
            continue;
        }
        let Some((id, range)) = payload.as_section() else {
            continue;
        };
        // The export section (7) precedes start, element, data count, code
        // and data.
        if !exported && matches!(id, 8 | 9 | 10 | 11 | 12) {
            let mut section = ExportSection::new();
            ours(&mut section);
            out.section(&section);
            exported = true;
        }
        out.section(&RawSection {
            id,
            data: &bytes[range],
        });
    }
    if !exported {
        let mut section = ExportSection::new();
        ours(&mut section);
        out.section(&section);
    }
    Ok((out.finish(), mutable))
}
