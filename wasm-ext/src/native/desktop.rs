use std::sync::OnceLock;

pub(super) use wasmtime::{
    Caller, Engine, Error, Extern, Instance, Linker, Memory, Module, Store, Val, ValType,
};
use wasmtime::{Config, Trap};

use super::{Ctx, TrapKind};

pub(super) fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        // Both supported Macs are 64-bit little-endian. Interpret guest code so
        // a notarized game can call it under the hardened runtime.
        #[cfg(target_os = "macos")]
        config.target("pulley64").expect("valid Pulley target");
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

pub(super) fn trap_kind(error: &wasmtime::Error) -> TrapKind {
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

pub(super) fn instantiate(
    linker: &Linker<Ctx>,
    store: &mut Store<Ctx>,
    module: &Module,
) -> Result<Instance, Error> {
    linker.instantiate(store, module)
}

pub(super) fn host_stop(_: crate::host::Stop) -> Error {
    Error::msg("stopped by a host call")
}
