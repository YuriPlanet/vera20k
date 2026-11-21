//! Desktop runtime: Wasmtime with fuel metering and NaN canonicalization.
//!
//! Determinism: fuel counts Wasm operators the same way on every platform for
//! one Wasmtime version, and Cranelift's NaN canonicalization makes every
//! float result bit-identical across CPUs. Residuals for later steps: the
//! call-depth limit is Wasmtime's stack-size limit, which depends on the
//! platform's frame sizes (the RFC wants an instrumented depth counter), and
//! module memory is not saved yet.

use std::ptr::NonNull;
use std::sync::OnceLock;

use wasmparser::{Payload, TypeRef, Validator, WasmFeatures};
use wasmtime::{
    Caller, Config, Engine, Extern, Global, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, Trap, TypedFunc, Val, ValType,
};

use crate::{
    ActionParams, Export, Fault, HOST_CALL_COST, IMPORTS, Invalid, Limits, LoadError, ModuleState,
    PAGE_BYTES, SIM_IMPORT_MODULE, SimHost, Status,
};

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
    exports: [Option<TypedFunc<(), ()>>; 3],
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
        let mut exports = [None, None, None];
        for (slot, export) in exports.iter_mut().zip(Export::ALL) {
            *slot = typed_export(&instance, &mut store, export)?;
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
        self.exports[export as usize].is_some()
    }

    /// Calls `export` with `budget` operations. Returns the operations used.
    /// A missing export costs nothing.
    pub fn call(
        &mut self,
        export: Export,
        budget: u64,
        host: &mut dyn SimHost,
    ) -> Result<u64, Fault> {
        let Some(func) = self.exports[export as usize].clone() else {
            return Ok(0);
        };
        // The store outlives this call but the host does not: erase the
        // borrow's lifetime and clear the pointer before returning.
        let host: NonNull<dyn SimHost + '_> = NonNull::from(host);
        // SAFETY: only the lifetime changes; `host` is cleared below.
        let host: NonNull<dyn SimHost> = unsafe { std::mem::transmute(host) };
        let data = self.store.data_mut();
        data.host = Some(host);
        data.fault = None;
        let fueled = self.store.set_fuel(budget);
        let result = match fueled {
            Ok(()) => func.call(&mut self.store, ()),
            Err(e) => Err(e),
        };
        let data = self.store.data_mut();
        data.host = None;
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
/// names, and shape limits.
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
    let cost = HOST_CALL_COST + extra;
    let fuel = caller.get_fuel()?;
    if fuel < cost {
        caller.data_mut().fault = Some(Fault::Budget);
        return Err(wasmtime::Error::msg(
            "operation budget exceeded in a host call",
        ));
    }
    caller.set_fuel(fuel - cost)?;
    let host = caller.data().host;
    match host {
        // SAFETY: see `Ctx::host`.
        Some(mut host) => Ok(unsafe { host.as_mut() }),
        None => Err(wasmtime::Error::msg("host call outside a module call")),
    }
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

fn read_bytes(caller: &mut Caller<'_, Ctx>, ptr: i32, len: i32) -> Result<Vec<u8>, Invalid> {
    let memory = memory_of(caller)?;
    let (Ok(ptr), Ok(len)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return Err(Invalid(format!(
            "negative pointer or length ({ptr}, {len})"
        )));
    };
    let mut out = vec![0; len];
    memory
        .read(&*caller, ptr, &mut out)
        .map_err(|_| Invalid(format!("buffer {ptr}+{len} is outside module memory")))?;
    Ok(out)
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
            let bytes = read_bytes(&mut caller, ptr, len).map_err(|e| fail(&mut caller, e))?;
            let line = String::from_utf8_lossy(&bytes).into_owned();
            enter(&mut caller, bytes.len() as u64)?.log(&line);
            Ok(())
        },
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
            let bytes = read_bytes(&mut caller, ptr, len).map_err(|e| fail(&mut caller, e))?;
            let params = ActionParams::parse(&bytes).map_err(|e| fail(&mut caller, e))?;
            let result = enter(&mut caller, 0)?.action(kind, target, &params);
            let (handle, status) = result.map_err(|e| fail(&mut caller, e))?;
            write_bytes(&mut caller, out, &handle.to_le_bytes())
                .map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    linker
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
