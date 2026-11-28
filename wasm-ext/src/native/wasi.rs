use std::sync::OnceLock;

pub(super) use wasmi::{
    Caller, Engine, Error, Extern, Instance, Linker, Memory, Module, Store, Val, ValType,
};
use wasmi::{Config, TrapCode};

use super::{Ctx, TrapKind};

pub(super) fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::default();
        // Let the shared weighted depth counter stop recursion first.
        config.set_max_recursion_depth(crate::instrument::MAX_STACK_SLOTS as usize);
        Engine::new(&config)
    })
}

pub(super) fn trap_kind(error: &Error) -> TrapKind {
    match error.as_trap_code() {
        Some(TrapCode::UnreachableCodeReached) => TrapKind::Unreachable,
        Some(TrapCode::MemoryOutOfBounds) => TrapKind::MemoryOutOfBounds,
        Some(TrapCode::TableOutOfBounds) => TrapKind::TableOutOfBounds,
        Some(TrapCode::IndirectCallToNull) => TrapKind::IndirectCallToNull,
        Some(TrapCode::BadSignature) => TrapKind::BadSignature,
        Some(TrapCode::IntegerOverflow) => TrapKind::IntegerOverflow,
        Some(TrapCode::IntegerDivisionByZero) => TrapKind::IntegerDivisionByZero,
        Some(TrapCode::BadConversionToInteger) => TrapKind::BadConversionToInteger,
        Some(TrapCode::StackOverflow) => TrapKind::StackOverflow,
        _ => TrapKind::Other,
    }
}

pub(super) fn instantiate(
    linker: &Linker<Ctx>,
    store: &mut Store<Ctx>,
    module: &Module,
) -> Result<Instance, Error> {
    linker.instantiate_and_start(store, module)
}

pub(super) fn host_stop(_: crate::host::Stop) -> Error {
    Error::new("stopped by a host call")
}
