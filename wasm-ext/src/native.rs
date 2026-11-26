//! Desktop and WASI bindings for the rewritten module. Both runtimes use the
//! same host imports, meter, memory accesses and saved globals.

#[cfg_attr(target_os = "wasi", path = "native/wasi.rs")]
#[cfg_attr(not(target_os = "wasi"), path = "native/desktop.rs")]
mod backend;
use backend::*;

use crate::client_host::{self, for_each_client_import};
use crate::host::{self, Ctx, Guest, for_each_import};
use crate::instrument::METER;
use crate::module::{Arg, Runtime, TrapKind, World};

pub struct Embedded {
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

/// A linker holding `world`'s imports.
fn linker(engine: &Engine, world: World) -> Linker<Ctx> {
    let mut linker = Linker::new(engine);
    let import_module = world.import_module();
    // The simulation list names [`host`] functions, the client list its
    // module.
    macro_rules! wrap {
        ($name:literal, $f:ident($($a:ident: $t:ty),*) -> $r:ty) => {
            wrap!(host, $name, $f($($a: $t),*) -> $r);
        };
        ($m:ident, $name:literal, $f:ident($($a:ident: $t:ty),*) -> $r:ty) => {
            linker
                .func_wrap(
                    import_module,
                    $name,
                    |mut caller: Caller<'_, Ctx>, $($a: $t),*| -> Result<$r, Error> {
                        $m::$f(&mut Cx(&mut caller), $($a),*)
                            .map_err(host_stop)
                    },
                )
                .expect("each import is defined once with a valid signature");
        };
    }
    match world {
        World::Sim => {
            for_each_import!(wrap);
        }
        World::Client => {
            for_each_client_import!(wrap);
        }
    }
    linker
}

impl Runtime for Embedded {
    fn instantiate(bytes: &[u8], ctx: Ctx, world: World) -> Result<Self, String> {
        let engine = engine();
        let module = Module::new(engine, bytes).map_err(|e| format!("{e:#}"))?;
        let mut store = Store::new(engine, ctx);
        let instance = instantiate(&linker(engine, world), &mut store, &module)
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

    fn call(&mut self, name: &str, args: &[Arg]) -> Result<(), TrapKind> {
        let result = match *args {
            [] => self
                .instance
                .get_typed_func::<(), ()>(&mut self.store, name)
                .and_then(|f| f.call(&mut self.store, ())),
            [Arg::I32(a), Arg::I32(b)] => self
                .instance
                .get_typed_func::<(i32, i32), ()>(&mut self.store, name)
                .and_then(|f| f.call(&mut self.store, (a, b))),
            [Arg::I64(a), Arg::I32(b), Arg::I32(c)] => self
                .instance
                .get_typed_func::<(i64, i32, i32), ()>(&mut self.store, name)
                .and_then(|f| f.call(&mut self.store, (a, b, c))),
            [Arg::F32(a), Arg::F32(b)] => self
                .instance
                .get_typed_func::<(f32, f32), ()>(&mut self.store, name)
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
            #[cfg(not(target_os = "wasi"))]
            Val::F32(bits) => u64::from(bits),
            #[cfg(target_os = "wasi")]
            Val::F32(value) => u64::from(value.to_bits()),
            #[cfg(not(target_os = "wasi"))]
            Val::F64(bits) => bits,
            #[cfg(target_os = "wasi")]
            Val::F64(value) => value.to_bits(),
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
            ValType::F32 => Val::from(f32::from_bits(bits as u32)),
            ValType::F64 => Val::from(f64::from_bits(bits)),
            other => return Err(format!("global of type {other:?}")),
        };
        global
            .set(&mut self.store, value)
            .map_err(|e| e.to_string())
    }
}
