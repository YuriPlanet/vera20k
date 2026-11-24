//! The load-time rewrite every runtime runs (RFC 0001 sections 3 and 4): one
//! pure function of the module bytes, so Wasmtime on the desktop and the
//! browser's own WebAssembly engine count the same operations, stop at the
//! same point, refuse the same recursion and produce the same float bits.
//!
//! - **Operations.** Each basic block is charged in full before it runs
//!   (one operation per original instruction); `memory.copy`, `memory.fill`,
//!   `memory.init`, `table.copy` and `table.init` also charge one operation per
//!   [`BULK_BYTES_PER_OP`] of their length first. The remaining budget is the
//!   exported global [`METER`]; a block that does not fit sets [`STOP`] to
//!   [`STOP_BUDGET`] and traps.
//! - **Call depth.** Each function adds its weight ([`FRAME_WEIGHT`] +
//!   parameters + declared locals) to [`DEPTH`] on entry and traps with
//!   [`STOP_DEPTH`] past [`MAX_STACK_SLOTS`]; each call site puts the
//!   caller's depth back after the call returns. The limit sits well below
//!   every runtime's own stack limit, so recursion stops at the same call
//!   everywhere.
//! - **NaN.** Every float instruction whose result a CPU may produce with
//!   any NaN bits (arithmetic, rounding, `min`/`max`, promote and demote) is
//!   followed by a select that replaces a NaN with the canonical one.
//! - **State.** Every mutable global the module declares is exported as
//!   `__vera_g<index>`, so saves and the state hash can read and write it.
//!   A mutable `f32` or `f64` global is stored as an `i32` or `i64` holding
//!   its bits (reinterpreted at each `global.get` and `global.set`), so a
//!   host that reads globals as JavaScript numbers still sees every bit.
//! - **Memory.** The memory's maximum becomes at most the module's limit,
//!   so `memory.grow` fails at the same size on every engine.
//!
//! Instructions the rewrite inserts are not charged. Nothing else changes:
//! function, type and global indices of the module stay valid because the
//! new globals and locals are appended.

use wasm_encoder::reencode::{Reencode, RoundtripReencoder};
use wasm_encoder::{
    BlockType, ConstExpr, ExportKind, ExportSection, Function, GlobalSection, GlobalType, Ieee32,
    Ieee64, Instruction, RawSection, ValType,
};
use wasmparser::{ExternalKind, FuncType, Operator, Payload};

/// The operations left in the running call (i64, unsigned).
pub(crate) const METER: &str = "__vera_meter";
/// The weighted call depth of the running call (i32).
pub(crate) const DEPTH: &str = "__vera_depth";
/// Why the module trapped on its own: 0, [`STOP_BUDGET`] or [`STOP_DEPTH`].
pub(crate) const STOP: &str = "__vera_stop";
pub(crate) const STOP_BUDGET: i32 = 1;
pub(crate) const STOP_DEPTH: i32 = 2;
/// Export names starting with this are the rewrite's.
pub(crate) const RESERVED_PREFIX: &str = "__vera_";

/// Weighted stack slots one call may hold (RFC section 4; open question 7).
/// At [`FRAME_WEIGHT`] per frame this is at most 2,048 frames: Wasmtime's
/// default 512 KiB stack holds them, and so does a browser Web Worker, whose
/// stack held about 10,500 empty frames in Chrome (2026-11-24).
pub(crate) const MAX_STACK_SLOTS: i32 = 8_192;
/// Slots every frame weighs on top of its parameters and locals.
pub(crate) const FRAME_WEIGHT: i64 = 4;
/// Bytes a bulk memory or table instruction moves per operation charged.
pub(crate) const BULK_BYTES_PER_OP: i64 = 8;

const CANONICAL_NAN_F32: u32 = 0x7FC0_0000;
const CANONICAL_NAN_F64: u64 = 0x7FF8_0000_0000_0000;

/// A rewritten module.
pub(crate) struct Rewritten {
    pub bytes: Vec<u8>,
    /// The module's own mutable globals, in global index order.
    pub mutable_globals: Vec<u32>,
    /// Every exported function with its type.
    pub exports: Vec<(String, FuncType)>,
}

/// A mutable float global, stored as an integer of the same width.
#[derive(Clone, Copy, PartialEq)]
enum Float {
    F32,
    F64,
}

pub(crate) fn global_export_name(index: u32) -> String {
    format!("__vera_g{index}")
}

/// Rewrites a module that passed the load checks, with memory limited to
/// `max_pages`.
pub(crate) fn rewrite(bytes: &[u8], max_pages: u64) -> Result<Rewritten, String> {
    let err = |e: wasmparser::BinaryReaderError| e.to_string();
    // First pass: types, function signatures, globals and exports.
    let mut types: Vec<FuncType> = Vec::new();
    // Every function's type index, imports first.
    let mut all_funcs: Vec<u32> = Vec::new();
    let mut imported_funcs = 0usize;
    let mut globals = 0u32;
    let mut mutable_globals = Vec::new();
    let mut floats: Vec<Option<Float>> = Vec::new();
    let mut func_exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.map_err(err)? {
            Payload::TypeSection(reader) => {
                for ty in reader.into_iter_err_on_gc_types() {
                    types.push(ty.map_err(err)?);
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    if let wasmparser::TypeRef::Func(ty) = import.map_err(err)?.ty {
                        all_funcs.push(ty);
                        imported_funcs += 1;
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for ty in reader {
                    all_funcs.push(ty.map_err(err)?);
                }
            }
            Payload::GlobalSection(reader) => {
                for global in reader {
                    let ty = global.map_err(err)?.ty;
                    if ty.mutable {
                        mutable_globals.push(globals);
                    }
                    floats.push(match (ty.mutable, ty.content_type) {
                        (true, wasmparser::ValType::F32) => Some(Float::F32),
                        (true, wasmparser::ValType::F64) => Some(Float::F64),
                        _ => None,
                    });
                    globals += 1;
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.map_err(err)?;
                    if export.kind == ExternalKind::Func {
                        func_exports.push((export.name.to_owned(), export.index));
                    }
                }
            }
            _ => {}
        }
    }
    let func_type = |index: u32| {
        all_funcs
            .get(index as usize)
            .and_then(|t| types.get(*t as usize))
            .cloned()
            .ok_or_else(|| format!("function {index} has no type"))
    };
    let exports = func_exports
        .into_iter()
        .map(|(name, index)| Ok((name, func_type(index)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let meter = globals;
    let depth = globals + 1;
    let stop = globals + 2;
    let ids = Ids { meter, depth, stop };

    let add_globals = |section: &mut GlobalSection| {
        for (ty, init) in [
            (ValType::I64, ConstExpr::i64_const(0)),
            (ValType::I32, ConstExpr::i32_const(0)),
            (ValType::I32, ConstExpr::i32_const(0)),
        ] {
            section.global(
                GlobalType {
                    val_type: ty,
                    mutable: true,
                    shared: false,
                },
                &init,
            );
        }
    };
    let add_exports = |section: &mut ExportSection| {
        for index in &mutable_globals {
            section.export(&global_export_name(*index), ExportKind::Global, *index);
        }
        section.export(METER, ExportKind::Global, meter);
        section.export(DEPTH, ExportKind::Global, depth);
        section.export(STOP, ExportKind::Global, stop);
    };

    let mut out = wasm_encoder::Module::new();
    let mut wrote_globals = false;
    let mut wrote_exports = false;
    let mut code: Option<(wasm_encoder::CodeSection, u32)> = None;
    let mut body_index = 0usize;
    let mut reencoder = RoundtripReencoder;
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(err)?;
        // Sections in binary order: global (6) precedes export (7), which
        // precedes start (8), element (9), data count (12), code (10) and
        // data (11). Insert missing ones before the first section after them.
        if let Some((id, _)) = payload.as_section() {
            let after_globals = matches!(id, 7 | 8 | 9 | 10 | 11 | 12);
            if !wrote_globals && after_globals {
                let mut section = GlobalSection::new();
                add_globals(&mut section);
                out.section(&section);
                wrote_globals = true;
            }
            if !wrote_exports && matches!(id, 8 | 9 | 10 | 11 | 12) {
                let mut section = ExportSection::new();
                add_exports(&mut section);
                out.section(&section);
                wrote_exports = true;
            }
        }
        match &payload {
            Payload::GlobalSection(reader) => {
                let mut section = GlobalSection::new();
                for (global, float) in reader.clone().into_iter().zip(&floats) {
                    let global = global.map_err(err)?;
                    let Some(float) = float else {
                        reencoder
                            .parse_global(&mut section, global)
                            .map_err(|e| e.to_string())?;
                        continue;
                    };
                    let init = match global.init_expr.get_operators_reader().read().map_err(err)? {
                        Operator::F32Const { value } => ConstExpr::i32_const(value.bits() as i32),
                        Operator::F64Const { value } => ConstExpr::i64_const(value.bits() as i64),
                        other => {
                            return Err(format!("a float global starts at {other:?}, not a constant"));
                        }
                    };
                    let val_type = match float {
                        Float::F32 => ValType::I32,
                        Float::F64 => ValType::I64,
                    };
                    section.global(
                        GlobalType {
                            val_type,
                            mutable: true,
                            shared: false,
                        },
                        &init,
                    );
                }
                add_globals(&mut section);
                out.section(&section);
                wrote_globals = true;
                continue;
            }
            Payload::ExportSection(reader) => {
                let mut section = ExportSection::new();
                for export in reader.clone() {
                    let export = export.map_err(err)?;
                    if export.name.starts_with(RESERVED_PREFIX) {
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
                add_exports(&mut section);
                out.section(&section);
                wrote_exports = true;
                continue;
            }
            Payload::MemorySection(reader) => {
                let mut section = wasm_encoder::MemorySection::new();
                for memory in reader.clone() {
                    let memory = memory.map_err(err)?;
                    if memory.initial > max_pages {
                        return Err(format!(
                            "memory starts at {} pages, limit {max_pages}",
                            memory.initial
                        ));
                    }
                    section.memory(wasm_encoder::MemoryType {
                        minimum: memory.initial,
                        maximum: Some(memory.maximum.map_or(max_pages, |m| m.min(max_pages))),
                        memory64: false,
                        shared: false,
                        page_size_log2: None,
                    });
                }
                out.section(&section);
                continue;
            }
            Payload::CodeSectionStart { count, .. } => {
                code = Some((wasm_encoder::CodeSection::new(), *count));
                continue;
            }
            Payload::CodeSectionEntry(body) => {
                let ty = func_type((imported_funcs + body_index) as u32)?;
                let function = instrument_body(body, &ty, ids, &floats, &mut reencoder)?;
                body_index += 1;
                let Some((section, count)) = code.as_mut() else {
                    return Err("function body outside the code section".into());
                };
                section.function(&function);
                if body_index == *count as usize {
                    out.section(&*section);
                }
                continue;
            }
            Payload::End(_) => {
                if !wrote_globals {
                    let mut section = GlobalSection::new();
                    add_globals(&mut section);
                    out.section(&section);
                }
                if !wrote_exports {
                    let mut section = ExportSection::new();
                    add_exports(&mut section);
                    out.section(&section);
                }
                continue;
            }
            _ => {}
        }
        if let Some((id, range)) = payload.as_section() {
            out.section(&RawSection {
                id,
                data: &bytes[range],
            });
        }
    }
    Ok(Rewritten {
        bytes: out.finish(),
        mutable_globals,
        exports,
    })
}

#[derive(Clone, Copy)]
struct Ids {
    meter: u32,
    depth: u32,
    stop: u32,
}

/// Scratch locals the rewrite appends to a function.
#[derive(Clone, Copy)]
struct Scratch {
    depth: u32,
    len: u32,
    len64: u32,
    f32: u32,
    f64: u32,
}

fn instrument_body(
    body: &wasmparser::FunctionBody<'_>,
    ty: &FuncType,
    ids: Ids,
    floats: &[Option<Float>],
    reencoder: &mut RoundtripReencoder,
) -> Result<Function, String> {
    let err = |e: wasmparser::BinaryReaderError| e.to_string();
    let mut locals = Vec::new();
    let mut declared = 0u32;
    for pair in body.get_locals_reader().map_err(err)? {
        let (count, ty) = pair.map_err(err)?;
        declared = declared.saturating_add(count);
        locals.push((
            count,
            reencoder.val_type(ty).map_err(|e| e.to_string())?,
        ));
    }
    let params = ty.params().len() as u32;
    let first = params
        .checked_add(declared)
        .ok_or("too many locals")?;
    let scratch = Scratch {
        depth: first,
        len: first + 1,
        len64: first + 2,
        f32: first + 3,
        f64: first + 4,
    };
    locals.extend([
        (2, ValType::I32),
        (1, ValType::I64),
        (1, ValType::F32),
        (1, ValType::F64),
    ]);
    let weight = FRAME_WEIGHT + i64::from(params) + i64::from(declared);
    let weight = i32::try_from(weight.min(i64::from(MAX_STACK_SLOTS) + 1)).unwrap_or(i32::MAX);

    let mut operators = Vec::new();
    let mut reader = body.get_operators_reader().map_err(err)?;
    while !reader.eof() {
        operators.push(reader.read().map_err(err)?);
    }

    let mut f = Function::new(locals);
    // Entry: count this frame's weight against the depth limit.
    f.instruction(&Instruction::GlobalGet(ids.depth));
    f.instruction(&Instruction::I32Const(weight));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::GlobalSet(ids.depth));
    f.instruction(&Instruction::GlobalGet(ids.depth));
    f.instruction(&Instruction::I32Const(MAX_STACK_SLOTS));
    f.instruction(&Instruction::I32GtS);
    stop_if(&mut f, ids, STOP_DEPTH);

    for (index, op) in operators.iter().enumerate() {
        // A block starts at the function's entry and after each instruction
        // that ends one.
        if index == 0 || ends_block(&operators[index - 1]) {
            charge_static(&mut f, ids, block_cost(&operators[index..]));
        }
        match op {
            Operator::MemoryCopy { .. }
            | Operator::MemoryFill { .. }
            | Operator::MemoryInit { .. }
            | Operator::TableCopy { .. }
            | Operator::TableInit { .. } => charge_length(&mut f, ids, scratch),
            Operator::Call { .. } | Operator::CallIndirect { .. } => {
                f.instruction(&Instruction::GlobalGet(ids.depth));
                f.instruction(&Instruction::LocalSet(scratch.depth));
            }
            Operator::GlobalSet { global_index } => match float_at(floats, *global_index) {
                Some(Float::F32) => {
                    f.instruction(&Instruction::I32ReinterpretF32);
                }
                Some(Float::F64) => {
                    f.instruction(&Instruction::I64ReinterpretF64);
                }
                None => {}
            },
            _ => {}
        }
        let instruction = reencoder
            .instruction(op.clone())
            .map_err(|e| e.to_string())?;
        f.instruction(&instruction);
        match op {
            Operator::Call { .. } | Operator::CallIndirect { .. } => {
                f.instruction(&Instruction::LocalGet(scratch.depth));
                f.instruction(&Instruction::GlobalSet(ids.depth));
            }
            Operator::GlobalGet { global_index } => match float_at(floats, *global_index) {
                Some(Float::F32) => {
                    f.instruction(&Instruction::F32ReinterpretI32);
                }
                Some(Float::F64) => {
                    f.instruction(&Instruction::F64ReinterpretI64);
                }
                None => {}
            },
            _ => {}
        }
        match nan_result(op) {
            Some(ValType::F32) => {
                f.instruction(&Instruction::LocalTee(scratch.f32));
                f.instruction(&Instruction::F32Const(Ieee32::new(CANONICAL_NAN_F32)));
                f.instruction(&Instruction::LocalGet(scratch.f32));
                f.instruction(&Instruction::LocalGet(scratch.f32));
                f.instruction(&Instruction::F32Eq);
                f.instruction(&Instruction::Select);
            }
            Some(_) => {
                f.instruction(&Instruction::LocalTee(scratch.f64));
                f.instruction(&Instruction::F64Const(Ieee64::new(CANONICAL_NAN_F64)));
                f.instruction(&Instruction::LocalGet(scratch.f64));
                f.instruction(&Instruction::LocalGet(scratch.f64));
                f.instruction(&Instruction::F64Eq);
                f.instruction(&Instruction::Select);
            }
            None => {}
        }
    }
    Ok(f)
}

fn float_at(floats: &[Option<Float>], index: u32) -> Option<Float> {
    floats.get(index as usize).copied().flatten()
}

/// Whether the instruction after `op` starts a new basic block: a branch
/// target, the inside of a block, or code after a branch.
fn ends_block(op: &Operator<'_>) -> bool {
    matches!(
        op,
        Operator::Block { .. }
            | Operator::Loop { .. }
            | Operator::If { .. }
            | Operator::Else
            | Operator::End
            | Operator::Br { .. }
            | Operator::BrIf { .. }
            | Operator::BrTable { .. }
            | Operator::Return
            | Operator::Unreachable
    )
}

/// Instructions from the start of `ops` up to and including the one that
/// ends the block.
fn block_cost(ops: &[Operator<'_>]) -> i64 {
    let len = ops
        .iter()
        .position(ends_block)
        .map_or(ops.len(), |end| end + 1);
    len as i64
}

/// The float type of `op`'s result when a CPU may give it any NaN bits.
fn nan_result(op: &Operator<'_>) -> Option<ValType> {
    use Operator as O;
    match op {
        O::F32Add
        | O::F32Sub
        | O::F32Mul
        | O::F32Div
        | O::F32Sqrt
        | O::F32Min
        | O::F32Max
        | O::F32Ceil
        | O::F32Floor
        | O::F32Trunc
        | O::F32Nearest
        | O::F32DemoteF64 => Some(ValType::F32),
        O::F64Add
        | O::F64Sub
        | O::F64Mul
        | O::F64Div
        | O::F64Sqrt
        | O::F64Min
        | O::F64Max
        | O::F64Ceil
        | O::F64Floor
        | O::F64Trunc
        | O::F64Nearest
        | O::F64PromoteF32 => Some(ValType::F64),
        _ => None,
    }
}

/// `if (i32 on the stack) { stop = why; unreachable }`.
fn stop_if(f: &mut Function, ids: Ids, why: i32) {
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(why));
    f.instruction(&Instruction::GlobalSet(ids.stop));
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);
}

/// Takes `cost` operations from the meter, or stops the module.
fn charge_static(f: &mut Function, ids: Ids, cost: i64) {
    f.instruction(&Instruction::GlobalGet(ids.meter));
    f.instruction(&Instruction::I64Const(cost));
    f.instruction(&Instruction::I64LtU);
    stop_if(f, ids, STOP_BUDGET);
    f.instruction(&Instruction::GlobalGet(ids.meter));
    f.instruction(&Instruction::I64Const(cost));
    f.instruction(&Instruction::I64Sub);
    f.instruction(&Instruction::GlobalSet(ids.meter));
}

/// Charges for the i32 length on top of the stack and leaves it there.
fn charge_length(f: &mut Function, ids: Ids, scratch: Scratch) {
    f.instruction(&Instruction::LocalTee(scratch.len));
    f.instruction(&Instruction::I64ExtendI32U);
    f.instruction(&Instruction::I64Const(BULK_BYTES_PER_OP.trailing_zeros().into()));
    f.instruction(&Instruction::I64ShrU);
    f.instruction(&Instruction::LocalTee(scratch.len64));
    f.instruction(&Instruction::GlobalGet(ids.meter));
    f.instruction(&Instruction::I64GtU);
    stop_if(f, ids, STOP_BUDGET);
    f.instruction(&Instruction::GlobalGet(ids.meter));
    f.instruction(&Instruction::LocalGet(scratch.len64));
    f.instruction(&Instruction::I64Sub);
    f.instruction(&Instruction::GlobalSet(ids.meter));
    f.instruction(&Instruction::LocalGet(scratch.len));
}
