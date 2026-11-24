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
    macro_rules! wrap {
        ($name:literal, $f:ident($($a:ident: $t:ty),*) -> $r:ty) => {
            linker
                .func_wrap(
                    SIM_IMPORT_MODULE,
                    $name,
                    |mut caller: Caller<'_, Ctx>, $($a: $t),*| -> wasmtime::Result<$r> {
                        host::$f(&mut Cx(&mut caller), $($a),*)
                            .map_err(|host::Stop| wasmtime::Error::msg("stopped by a host call"))
                    },
                )
                .expect("each import is defined once with a valid signature");
        };
    }
    for_each_import!(wrap);
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
