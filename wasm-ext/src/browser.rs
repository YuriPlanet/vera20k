//! Browser runtime: the browser's own WebAssembly engine runs the rewritten
//! module ([`crate::instrument`]), RFC 0001 open question 1.
//!
//! Measured in Chrome (2026-11-24, 200,000-operation calls): 36 to 54 times
//! faster than the wasmi interpreter on computation and calls, twice as fast
//! on host calls. The rewrite meters operations, limits call depth,
//! canonicalizes NaNs, caps memory and stores float globals as integer bits,
//! so results match the desktop's Wasmtime bit for bit.
//!
//! The instance, its memory and the import closures are JavaScript objects
//! of the thread that loaded the module, the page's main thread: a match
//! with modules never lends ticks to the simulation worker
//! (`app::match_runtime::sim_host`). Imports reach the engine directly
//! through wasm-bindgen closures; `i64` arguments arrive as BigInts.

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Array, BigInt, Function, Object, Reflect, Uint8Array, WebAssembly};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::client_host::{self, for_each_client_import};
use crate::host::{self, Ctx, Guest, for_each_import};
use crate::instrument::METER;
use crate::module::{Arg, Runtime, TrapKind, World};

/// What the module and its imports share.
struct Inner {
    ctx: Ctx,
    memory: Option<WebAssembly::Memory>,
    meter: Option<WebAssembly::Global>,
}

pub struct Browser {
    inner: Rc<RefCell<Inner>>,
    exports: Object,
}

// SAFETY: the module is created, called and dropped on the thread that
// loaded it: a browser match with modules ticks only on the main thread
// (`app::match_runtime::sim_host::SimHost::thread_enabled`).
unsafe impl Send for Browser {}
// SAFETY: as above; shared references never reach the JavaScript objects
// from another thread.
unsafe impl Sync for Browser {}

fn bytes_view(memory: &WebAssembly::Memory) -> Uint8Array {
    Uint8Array::new(&memory.buffer())
}

fn read(memory: Option<&WebAssembly::Memory>, offset: usize, buf: &mut [u8]) -> bool {
    let Some(memory) = memory else {
        return buf.is_empty();
    };
    let view = bytes_view(memory);
    let Some(end) = offset.checked_add(buf.len()) else {
        return false;
    };
    if end > view.length() as usize {
        return false;
    }
    view.subarray(offset as u32, end as u32).copy_to(buf);
    true
}

fn write(memory: Option<&WebAssembly::Memory>, offset: usize, bytes: &[u8]) -> bool {
    let Some(memory) = memory else {
        return bytes.is_empty();
    };
    let view = bytes_view(memory);
    let Some(end) = offset.checked_add(bytes.len()) else {
        return false;
    };
    if end > view.length() as usize {
        return false;
    }
    view.subarray(offset as u32, end as u32).copy_from(bytes);
    true
}

fn global_bits(global: &WebAssembly::Global) -> u64 {
    let value = global.value();
    match value.dyn_into::<BigInt>() {
        Ok(big) => i64::try_from(big).map_or(0, |v| v as u64),
        // The rewrite leaves only integer mutable globals: an i32 is a Number.
        Err(number) => number.as_f64().map_or(0, |v| u64::from(v as i32 as u32)),
    }
}

/// An import's view of the calling module.
struct Cx<'a>(&'a mut Inner);

impl Guest for Cx<'_> {
    fn ctx(&self) -> &Ctx {
        &self.0.ctx
    }

    fn ctx_mut(&mut self) -> &mut Ctx {
        &mut self.0.ctx
    }

    fn memory_len(&mut self) -> Option<usize> {
        self.0
            .memory
            .as_ref()
            .map(|m| bytes_view(m).length() as usize)
    }

    fn read_memory(&mut self, offset: usize, buf: &mut [u8]) -> bool {
        read(self.0.memory.as_ref(), offset, buf)
    }

    fn write_memory(&mut self, offset: usize, bytes: &[u8]) -> bool {
        write(self.0.memory.as_ref(), offset, bytes)
    }

    fn meter(&mut self) -> u64 {
        self.0.meter.as_ref().map_or(0, global_bits)
    }

    fn set_meter(&mut self, ops: u64) {
        if let Some(meter) = &self.0.meter {
            meter.set_value(&BigInt::from(ops as i64).into());
        }
    }
}

/// `world`'s import object (`vera:sim@1.0` or `vera:client@1.0`), each
/// import a closure over `inner`.
fn imports(inner: &Rc<RefCell<Inner>>, world: World) -> Result<Object, JsValue> {
    let functions = Object::new();
    // The simulation list names [`host`] functions, the client list its
    // module.
    macro_rules! wrap {
        ($name:literal, $f:ident($($a:ident: $t:ty),*) -> $r:ty) => {
            wrap!(host, $name, $f($($a: $t),*) -> $r);
        };
        ($m:ident, $name:literal, $f:ident($($a:ident: $t:ty),*) -> $r:ty) => {
            let shared = Rc::clone(inner);
            let closure = Closure::<dyn FnMut($($t),*) -> Result<$r, JsValue>>::new(
                move |$($a: $t),*| -> Result<$r, JsValue> {
                    let mut inner = shared.borrow_mut();
                    $m::$f(&mut Cx(&mut inner), $($a),*)
                        .map_err(|host::Stop| JsValue::from_str("stopped by a host call"))
                },
            );
            Reflect::set(&functions, &$name.into(), &closure.into_js_value())?;
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
    let imports = Object::new();
    Reflect::set(&imports, &world.import_module().into(), &functions)?;
    Ok(imports)
}

fn describe(error: &JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| format!("{error:?}"))
}

/// Trap messages differ between browsers; these cover Chrome, Firefox and
/// Safari. The text only names the stop reason; every machine agrees that
/// the module stopped, which is what the state hash holds.
fn trap_kind(error: &JsValue) -> TrapKind {
    if error.is_instance_of::<js_sys::RangeError>() {
        return TrapKind::StackOverflow;
    }
    if !error.is_instance_of::<WebAssembly::RuntimeError>() {
        return TrapKind::Other;
    }
    let message = describe(error).to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| message.contains(w));
    if has(&["unreachable"]) {
        TrapKind::Unreachable
    } else if has(&["memory access", "out of bounds memory", "index out of bounds"]) {
        TrapKind::MemoryOutOfBounds
    } else if has(&["divide by zero", "division by zero", "remainder by zero"]) {
        TrapKind::IntegerDivisionByZero
    } else if has(&["unrepresentable", "conversion to integer", "float to int"]) {
        TrapKind::BadConversionToInteger
    } else if has(&["integer overflow"]) {
        TrapKind::IntegerOverflow
    } else if has(&["signature"]) {
        TrapKind::BadSignature
    } else if has(&["null"]) {
        TrapKind::IndirectCallToNull
    } else if has(&["table"]) {
        TrapKind::TableOutOfBounds
    } else {
        TrapKind::Other
    }
}

impl Browser {
    fn export(&self, name: &str) -> Option<JsValue> {
        Reflect::get(&self.exports, &name.into())
            .ok()
            .filter(|value| !value.is_undefined())
    }

    fn global_of(&self, name: &str) -> Option<WebAssembly::Global> {
        self.export(name)?.dyn_into().ok()
    }

    fn memory(&self) -> Option<WebAssembly::Memory> {
        self.inner.borrow().memory.clone()
    }
}

impl Runtime for Browser {
    fn instantiate(bytes: &[u8], ctx: Ctx, world: World) -> Result<Self, String> {
        let module = WebAssembly::Module::new(&Uint8Array::from(bytes).into())
            .map_err(|e| describe(&e))?;
        let inner = Rc::new(RefCell::new(Inner {
            ctx,
            memory: None,
            meter: None,
        }));
        let imports = imports(&inner, world).map_err(|e| describe(&e))?;
        // The load checks rejected `start` sections, so no module code runs.
        let instance =
            WebAssembly::Instance::new(&module, &imports).map_err(|e| describe(&e))?;
        let exports = instance.exports();
        let get = |name: &str| Reflect::get(&exports, &name.into()).ok();
        {
            let mut shared = inner.borrow_mut();
            shared.memory = get("memory").and_then(|m| m.dyn_into().ok());
            shared.meter = get(METER).and_then(|m| m.dyn_into().ok());
        }
        Ok(Self { inner, exports })
    }

    fn with_ctx<T>(&mut self, f: impl FnOnce(&mut Ctx) -> T) -> T {
        f(&mut self.inner.borrow_mut().ctx)
    }

    fn call(&mut self, name: &str, args: &[Arg]) -> Result<(), TrapKind> {
        let function: Function = self
            .export(name)
            .and_then(|f| f.dyn_into().ok())
            .ok_or(TrapKind::Other)?;
        // `i64` parameters take BigInts, `i32` and `f32` Numbers.
        let values: Array = args
            .iter()
            .map(|arg| match *arg {
                Arg::I32(v) => JsValue::from(v),
                Arg::I64(v) => BigInt::from(v).into(),
                Arg::F32(v) => JsValue::from(f64::from(v)),
            })
            .collect();
        function
            .apply(&JsValue::NULL, &values)
            .map(|_| ())
            .map_err(|e| trap_kind(&e))
    }

    fn memory_len(&self) -> usize {
        self.memory()
            .map_or(0, |m| bytes_view(&m).length() as usize)
    }

    fn read_memory(&self, offset: usize, buf: &mut [u8]) -> bool {
        read(self.memory().as_ref(), offset, buf)
    }

    fn write_memory(&mut self, offset: usize, bytes: &[u8]) -> bool {
        write(self.memory().as_ref(), offset, bytes)
    }

    fn grow(&mut self, pages: u64) -> Result<(), String> {
        let memory = self.memory().ok_or("no memory")?;
        let grow: Function = Reflect::get(&memory, &"grow".into())
            .and_then(|f| f.dyn_into().map_err(JsValue::from))
            .map_err(|e| describe(&e))?;
        grow.call1(&memory, &JsValue::from(pages as f64))
            .map(|_| ())
            .map_err(|e| describe(&e))
    }

    fn global(&mut self, name: &str) -> u64 {
        self.global_of(name).map_or(0, |g| global_bits(&g))
    }

    fn set_global(&mut self, name: &str, bits: u64) -> Result<(), String> {
        let global = self
            .global_of(name)
            .ok_or_else(|| format!("no global {name}"))?;
        // Set it with the kind of value it holds: BigInt for i64, Number for i32.
        let value = if global.value().is_bigint() {
            BigInt::from(bits as i64).into()
        } else {
            JsValue::from(bits as u32 as i32)
        };
        global.set_value(&value);
        Ok(())
    }
}
