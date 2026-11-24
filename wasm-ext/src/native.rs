//! Desktop runtime: Wasmtime runs the rewritten module ([`crate::instrument`]).
//!
//! Its own fuel and NaN canonicalization are off: the rewrite meters
//! operations, limits call depth and canonicalizes NaNs identically for every
//! runtime, so this one and the browser's WebAssembly engine agree bit for
//! bit. The rewrite also caps the memory's maximum at the module's limit.

use std::sync::OnceLock;

use wasmtime::{
    Caller, Config, Engine, Extern, Instance, Linker, Memory, Module, Store, Trap, Val, ValType,
};

use crate::SIM_IMPORT_MODULE;
use crate::host::{self, Ctx, Guest, for_each_import};
use crate::instrument::METER;
use crate::module::{Runtime, TrapKind};

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        config.consume_fuel(false);
        config.cranelift_nan_canonicalization(false);
        config.wasm_simd(false);
        config.wasm_relaxed_simd(false);
        config.wasm_multi_memory(false);
        config.wasm_memory64(false);
        config.wasm_tail_call(false);
        // Threads and reference types are off in this feature set; the load
        // checks enforce the full allowlist with wasmparser before compiling.
        // Only reachable on a broken host configuration; the engine is the
        // same for every module, so this is not module data.
        Engine::new(&config).expect("the module engine configuration is valid")
    })
}

pub struct Wasmtime {
    store: Store<Ctx>,
    instance: Instance,
    memory: Option<Memory>,
}

/// An import's view of the calling module.
struct Cx<'a, 'b>(&'a mut Caller<'b, Ctx>);

impl Guest for Cx<'_, '_> {
    fn ctx(&self) -> &Ctx {
        self.0.data()
    }

    fn ctx_mut(&mut self) -> &mut Ctx {
        self.0.data_mut()
    }

    fn memory_len(&mut self) -> Option<usize> {
        match self.0.get_export("memory") {
            Some(Extern::Memory(memory)) => Some(memory.data_size(&*self.0)),
            _ => None,
        }
    }

    fn read_memory(&mut self, offset: usize, buf: &mut [u8]) -> bool {
        match self.0.get_export("memory") {
            Some(Extern::Memory(memory)) => memory.read(&*self.0, offset, buf).is_ok(),
            _ => false,
        }
    }

    fn write_memory(&mut self, offset: usize, bytes: &[u8]) -> bool {
        match self.0.get_export("memory") {
            Some(Extern::Memory(memory)) => memory.write(&mut *self.0, offset, bytes).is_ok(),
            _ => false,
        }
    }

    fn meter(&mut self) -> u64 {
        match self.0.get_export(METER) {
            Some(Extern::Global(global)) => global.get(&mut *self.0).i64().unwrap_or(0) as u64,
            _ => 0,
        }
    }

    fn set_meter(&mut self, ops: u64) {
        if let Some(Extern::Global(global)) = self.0.get_export(METER) {
            // The rewrite declares the meter a mutable i64.
            let _ = global.set(&mut *self.0, Val::I64(ops as i64));
        }
    }
}

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
    wire(linker.func_wrap(
        m,
        "ini_register",
        |mut caller: Caller<'_, Ctx>,
         kind: i32,
         section: i32,
         ptr: i32,
         len: i32,
         key: i32,
         out: i32|
         -> wasmtime::Result<i32> {
            // Only the initialization function registers (INI registration
            // RFC 0001 section 6h).
            if !in_init(&caller) {
                return Err(fail(
                    &mut caller,
                    Invalid("ini_register outside vera_init".into()),
                ));
            }
            check_buffer(&mut caller, out, 1, 4).map_err(|e| fail(&mut caller, e))?;
            charge(&mut caller, HOST_CALL_COST)?;
            let name = read_str(&mut caller, ptr, len)?;
            let sections = match kind {
                INI_KEY => INI_SECTIONS.len() as i32,
                INI_EVENT => 2,
                INI_REGISTRY => 1,
                _ => {
                    return Err(fail(
                        &mut caller,
                        Invalid(format!("unknown IniKind {kind}")),
                    ));
                }
            };
            if !(0..sections).contains(&section) {
                return Err(fail(
                    &mut caller,
                    Invalid(format!("IniKind {kind} takes no section {section}")),
                ));
            }
            if !ini_name_valid(&name) {
                return Err(fail(
                    &mut caller,
                    Invalid(format!("{name:?} is not a registrable INI name")),
                ));
            }
            let result = host(&mut caller)?.ini_register(kind, section, &name, key);
            let (id, status) = result.map_err(|e| fail(&mut caller, e))?;
            let id = if status == Status::Ok { id } else { 0 };
            write_bytes(&mut caller, out, &id.to_le_bytes()).map_err(|e| fail(&mut caller, e))?;
            Ok(status as i32)
        },
    ));
    linker
}

fn trap_kind(error: &wasmtime::Error) -> TrapKind {
    match error.downcast_ref::<Trap>() {
        Some(Trap::UnreachableCodeReached) => TrapKind::Unreachable,
        Some(Trap::MemoryOutOfBounds | Trap::HeapMisaligned) => TrapKind::MemoryOutOfBounds,
        Some(Trap::TableOutOfBounds) => TrapKind::TableOutOfBounds,
        Some(Trap::IndirectCallToNull) => TrapKind::IndirectCallToNull,
        Some(Trap::BadSignature) => TrapKind::BadSignature,
        Some(Trap::IntegerOverflow) => TrapKind::IntegerOverflow,
        Some(Trap::IntegerDivisionByZero) => TrapKind::IntegerDivisionByZero,
        Some(Trap::BadConversionToInteger) => TrapKind::BadConversionToInteger,
        Some(Trap::StackOverflow) => TrapKind::StackOverflow,
        _ => TrapKind::Other,
    }
}

impl Runtime for Wasmtime {
    fn instantiate(bytes: &[u8], ctx: Ctx) -> Result<Self, String> {
        let engine = engine();
        let module = Module::new(engine, bytes).map_err(|e| format!("{e:#}"))?;
        let mut store = Store::new(engine, ctx);
        let instance = linker(engine)
            .instantiate(&mut store, &module)
            .map_err(|e| format!("{e:#}"))?;
        let memory = instance.get_memory(&mut store, "memory");
        Ok(Self {
            store,
            instance,
            memory,
        })
    }

    fn with_ctx<T>(&mut self, f: impl FnOnce(&mut Ctx) -> T) -> T {
        f(self.store.data_mut())
    }

    fn call(&mut self, name: &str, args: &[i32]) -> Result<(), TrapKind> {
        let result = match *args {
            [] => self
                .instance
                .get_typed_func::<(), ()>(&mut self.store, name)
                .and_then(|f| f.call(&mut self.store, ())),
            [a, b] => self
                .instance
                .get_typed_func::<(i32, i32), ()>(&mut self.store, name)
                .and_then(|f| f.call(&mut self.store, (a, b))),
            _ => return Err(TrapKind::Other),
        };
        result.map_err(|e| trap_kind(&e))
    }

    fn memory_len(&self) -> usize {
        self.memory.map_or(0, |m| m.data_size(&self.store))
    }

    fn read_memory(&self, offset: usize, buf: &mut [u8]) -> bool {
        self.memory
            .is_some_and(|m| m.read(&self.store, offset, buf).is_ok())
    }

    fn write_memory(&mut self, offset: usize, bytes: &[u8]) -> bool {
        match self.memory {
            Some(m) => m.write(&mut self.store, offset, bytes).is_ok(),
            None => false,
        }
    }

    fn grow(&mut self, pages: u64) -> Result<(), String> {
        let memory = self.memory.ok_or("no memory")?;
        memory
            .grow(&mut self.store, pages)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn global(&mut self, name: &str) -> u64 {
        let Some(global) = self.instance.get_global(&mut self.store, name) else {
            return 0;
        };
        match global.get(&mut self.store) {
            Val::I32(v) => u64::from(v as u32),
            Val::I64(v) => v as u64,
            Val::F32(bits) => u64::from(bits),
            Val::F64(bits) => bits,
            // MVP globals hold only numbers; the load checks admit no other.
            _ => 0,
        }
    }

    fn set_global(&mut self, name: &str, bits: u64) -> Result<(), String> {
        let global = self
            .instance
            .get_global(&mut self.store, name)
            .ok_or_else(|| format!("no global {name}"))?;
        let value = match global.ty(&self.store).content() {
            ValType::I32 => Val::I32(bits as u32 as i32),
            ValType::I64 => Val::I64(bits as i64),
            ValType::F32 => Val::F32(bits as u32),
            ValType::F64 => Val::F64(bits),
            other => return Err(format!("global of type {other}")),
        };
        global
            .set(&mut self.store, value)
            .map_err(|e| e.to_string())
    }
}
