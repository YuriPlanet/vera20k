//! The `vera:sim@1.0` imports, written once over [`Guest`] and linked into
//! every runtime by [`for_each_import`]. Each is a thin adapter onto
//! [`SimHost`]: it checks arguments, charges the call's budget through the
//! rewrite's meter, and copies data in and out of module memory.
//!
//! The reads both worlds share (`frame`, `log`, `has_kind`, `lookup`,
//! `name_of`, `get`, `get_many`, `find`, `var_get`) go through [`Reads`],
//! so the `vera:client@1.0` imports ([`crate::client_host`]) use the same
//! functions (appendix 2.1, "As `lookup`", "as for `get_many`").

use std::ops::Range;
use std::ptr::NonNull;
use std::sync::Arc;

use crate::sections::Sections;
use crate::triggers::{ActionDef, NAME_MODULE_ACTION, TriggerDef};
use crate::{
    ActionParams, ClientHost, ENTRY_COST, Export, Fault, FindPage, HOST_CALL_COST, INI_EVENT,
    INI_KEY, INI_REGISTRY, INI_SECTIONS, Invalid, MAX_NOTIFY_BYTES, MAX_SCAN, NAME_ACTION_KIND,
    NAME_KINDS, NAME_NOTIFY,
    NAME_PROPERTY, Order, Query, SKIPS_PER_OP, SimHost, Status, action_kind_id, action_kind_name,
    ini_name_valid, property_id, property_name,
};

/// What the imports keep per module, beside the runtime's own store data.
pub(crate) struct Ctx {
    /// Set only for the duration of a simulation module call.
    pub host: Option<NonNull<dyn SimHost>>,
    /// Set only for the duration of a client module call
    /// ([`crate::ClientModuleOn`]).
    pub client: Option<NonNull<dyn ClientHost>>,
    /// The first invalid argument or budget fault a host call raised.
    pub fault: Option<Fault>,
    /// The module's custom sections; `lookup(ModuleAction)` reads them.
    pub sections: Arc<Sections>,
    /// The export running, for imports unavailable in `vera_init`.
    pub export: Option<Export>,
    /// The budget the running call started with; `find` sizes its pages by it.
    pub budget: u64,
    /// Client modules: the running call is `client_init`, where only
    /// `client_lookup`, `client_name_of`, `client_has_kind` and `client_log`
    /// are available (appendix 2.1).
    pub client_init: bool,
    /// Client modules: `client_random`'s xorshift64 state, never 0.
    pub rng: u64,
}

// SAFETY: `host` and `client` are non-null only inside a module call, which
// holds the `&mut dyn SimHost` or `&mut dyn ClientHost` it came from for its
// whole duration on this thread.
unsafe impl Send for Ctx {}
// SAFETY: `host` and `client` are only dereferenced through a runtime's
// `&mut` store inside a call; shared access to the store never reads them.
unsafe impl Sync for Ctx {}

impl Ctx {
    pub fn new(sections: Arc<Sections>) -> Self {
        Self {
            host: None,
            client: None,
            fault: None,
            sections,
            export: None,
            budget: 0,
            client_init: false,
            rng: 1,
        }
    }
}

/// A runtime's view of the calling module during an import.
pub(crate) trait Guest {
    fn ctx(&self) -> &Ctx;
    fn ctx_mut(&mut self) -> &mut Ctx;
    /// The size of the module's exported `memory`, `None` without one.
    fn memory_len(&mut self) -> Option<usize>;
    /// Copies module memory at `offset` into `buf`; false out of bounds.
    fn read_memory(&mut self, offset: usize, buf: &mut [u8]) -> bool;
    /// Copies `bytes` into module memory at `offset`; false out of bounds.
    fn write_memory(&mut self, offset: usize, bytes: &[u8]) -> bool;
    /// Operations left in the call (the rewrite's meter).
    fn meter(&mut self) -> u64;
    fn set_meter(&mut self, ops: u64);
}

/// An import stopped the module; the reason is in [`Ctx::fault`].
#[derive(Debug)]
pub(crate) struct Stop;

/// Every import: its name, the [`host`](self) function and its core Wasm
/// signature. `$wrap!` links one import into a runtime; the client world's
/// list is [`crate::client_host::for_each_client_import`].
macro_rules! for_each_import {
    ($wrap:ident) => {
        $wrap!("frame", frame() -> i32);
        $wrap!("random", random(lo: i32, hi: i32, out: i32) -> i32);
        $wrap!("log", log(ptr: i32, len: i32) -> ());
        $wrap!("has_kind", has_kind(table: i32, id: i32) -> i32);
        $wrap!("lookup", lookup(kind: i32, ptr: i32, len: i32, out: i32) -> i32);
        $wrap!("name_of", name_of(kind: i32, id: i32, out: i32, cap: i32, len_out: i32) -> i32);
        $wrap!("get", get(reference: i64, prop: i32, out: i32) -> i32);
        $wrap!("get_many", get_many(refs: i32, n: i32, props: i32, m: i32, out: i32, count_out: i32) -> i32);
        $wrap!("set", set(reference: i64, prop: i32, value: i64) -> i32);
        $wrap!("modify", modify(reference: i64, prop: i32, op: i32, value: i64, frames: i32, source: i64, group: i32, out: i32) -> i32);
        $wrap!("unmodify", unmodify(modifier: i64) -> i32);
        $wrap!("find", find(query: i32, len: i32, out: i32, cap: i32, res: i32) -> i32);
        $wrap!("var_get", var_get(scope: i64, var: i32, out: i32) -> i32);
        $wrap!("var_set", var_set(scope: i64, var: i32, value: i64) -> i32);
        $wrap!("action", action(kind: i32, target: i64, ptr: i32, len: i32, out: i32) -> i32);
        $wrap!("trigger_create", trigger_create(ptr: i32, len: i32, out: i32) -> i32);
        $wrap!("trigger_remove", trigger_remove(trigger: i64) -> i32);
        $wrap!("trigger_bind", trigger_bind(trigger: i64, reference: i64) -> i32);
        $wrap!("trigger_unbind", trigger_unbind(trigger: i64, reference: i64) -> i32);
        $wrap!("rules_get", rules_get(sec: i32, sec_len: i32, key: i32, key_len: i32, out: i32, cap: i32, len_out: i32) -> i32);
        $wrap!("rules_set", rules_set(sec: i32, sec_len: i32, key: i32, key_len: i32, val: i32, val_len: i32) -> i32);
        $wrap!("send_notify", send_notify(to: i64, id: i32, ptr: i32, len: i32) -> i32);
        $wrap!("order", order(house: i64, ptr: i32, len: i32) -> i32);
        $wrap!("ini_register", ini_register(kind: i32, section: i32, ptr: i32, len: i32, key: i32, out: i32) -> i32);
    };
}
pub(crate) use for_each_import;

pub(crate) type R<T> = Result<T, Stop>;

/// Charges one host call and hands back the host. A host call that runs past
/// the budget stops the module as a budget fault.
fn enter<'a>(g: &'a mut dyn Guest, extra: u64) -> R<&'a mut dyn SimHost> {
    charge(g, HOST_CALL_COST.saturating_add(extra))?;
    host(g)
}

fn host<'a>(g: &'a mut dyn Guest) -> R<&'a mut dyn SimHost> {
    match g.ctx().host {
        // SAFETY: see `Ctx::host`.
        Some(mut host) => Ok(unsafe { host.as_mut() }),
        None => Err(fail(g, Invalid("host call outside a module call".into()))),
    }
}

/// The host of the calling module's world, for the reads both worlds share.
pub(crate) enum Reads<'a> {
    Sim(&'a mut dyn SimHost),
    Client(&'a mut dyn ClientHost),
}

impl Reads<'_> {
    fn frame(&mut self) -> i32 {
        match self {
            Self::Sim(h) => h.frame(),
            Self::Client(h) => h.frame(),
        }
    }

    fn log(&mut self, line: &str) {
        match self {
            Self::Sim(h) => h.log(line),
            Self::Client(h) => h.log(line),
        }
    }

    fn has_kind(&mut self, table: i32, id: i32) -> bool {
        match self {
            Self::Sim(h) => h.has_kind(table, id),
            Self::Client(h) => h.has_kind(table, id),
        }
    }

    fn lookup(&mut self, kind: i32, name: &str) -> Result<(i32, Status), Invalid> {
        match self {
            Self::Sim(h) => h.lookup(kind, name),
            Self::Client(h) => h.lookup(kind, name),
        }
    }

    fn name_of(&mut self, kind: i32, id: i32) -> Result<(String, Status), Invalid> {
        match self {
            Self::Sim(h) => h.name_of(kind, id),
            Self::Client(h) => h.name_of(kind, id),
        }
    }

    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        match self {
            Self::Sim(h) => h.get(reference, prop),
            Self::Client(h) => h.get(reference, prop),
        }
    }

    fn find(&mut self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        match self {
            Self::Sim(h) => h.find(query, cap),
            Self::Client(h) => h.find(query, cap),
        }
    }

    fn var_get(&mut self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        match self {
            Self::Sim(h) => h.var_get(scope, var),
            Self::Client(h) => h.var_get(scope, var),
        }
    }
}

/// The calling module's host, of either world.
fn reads<'a>(g: &'a mut dyn Guest) -> R<Reads<'a>> {
    let (sim, client) = (g.ctx().host, g.ctx().client);
    match (sim, client) {
        // SAFETY: see `Ctx::host`.
        (Some(mut host), _) => Ok(Reads::Sim(unsafe { host.as_mut() })),
        // SAFETY: see `Ctx::client`.
        (None, Some(mut host)) => Ok(Reads::Client(unsafe { host.as_mut() })),
        (None, None) => Err(fail(g, Invalid("host call outside a module call".into()))),
    }
}

/// [`enter`] for the reads both worlds share.
fn enter_reads<'a>(g: &'a mut dyn Guest, extra: u64) -> R<Reads<'a>> {
    charge(g, HOST_CALL_COST.saturating_add(extra))?;
    reads(g)
}

/// Takes `cost` operations from the call's budget; running out is a budget
/// fault.
pub(crate) fn charge(g: &mut dyn Guest, cost: u64) -> R<()> {
    let left = g.meter();
    if left < cost {
        return Err(out_of_budget(g));
    }
    g.set_meter(left - cost);
    Ok(())
}

pub(crate) fn out_of_budget(g: &mut dyn Guest) -> Stop {
    g.ctx_mut().fault = Some(Fault::Budget);
    Stop
}

pub(crate) fn fail(g: &mut dyn Guest, why: Invalid) -> Stop {
    g.ctx_mut().fault.get_or_insert(Fault::Invalid(why.0));
    Stop
}

fn memory_len(g: &mut dyn Guest) -> Result<usize, Invalid> {
    g.memory_len()
        .ok_or_else(|| Invalid("the module exports no memory".into()))
}

/// Checks that `count` entries of `size` bytes at `ptr` lie in module memory.
pub(crate) fn check_buffer(
    g: &mut dyn Guest,
    ptr: i32,
    count: usize,
    size: usize,
) -> Result<Range<usize>, Invalid> {
    let len = memory_len(g)?;
    let range = usize::try_from(ptr).ok().and_then(|start| {
        let end = start.checked_add(count.checked_mul(size)?)?;
        (end <= len).then_some(start..end)
    });
    range.ok_or_else(|| {
        Invalid(format!(
            "buffer {ptr} of {count} x {size} bytes is outside module memory"
        ))
    })
}

/// A count or capacity argument, which may not be negative.
pub(crate) fn count_arg(value: i32, what: &str) -> Result<usize, Invalid> {
    usize::try_from(value).map_err(|_| Invalid(format!("negative {what} {value}")))
}

pub(crate) fn input_buffer(g: &mut dyn Guest, ptr: i32, len: i32) -> R<Range<usize>> {
    let len = count_arg(len, "buffer length").map_err(|e| fail(g, e))?;
    check_buffer(g, ptr, len, 1).map_err(|e| fail(g, e))
}

/// The bytes at `range`, which a check placed inside module memory.
pub(crate) fn read(g: &mut dyn Guest, range: Range<usize>) -> Vec<u8> {
    let mut bytes = vec![0u8; range.len()];
    g.read_memory(range.start, &mut bytes);
    bytes
}

/// Variable-size copies use both the guest memory bounds and the budget.
pub(crate) fn copy_input(g: &mut dyn Guest, ptr: i32, len: i32) -> R<Vec<u8>> {
    let range = input_buffer(g, ptr, len)?;
    charge(g, range.len() as u64)?;
    Ok(read(g, range))
}

/// A section, key or value name: UTF-8 text in module memory. Text that is
/// not UTF-8 is an invalid argument.
pub(crate) fn read_str(g: &mut dyn Guest, ptr: i32, len: i32) -> R<String> {
    let bytes = copy_input(g, ptr, len)?;
    String::from_utf8(bytes)
        .map_err(|_| fail(g, Invalid(format!("text at {ptr}+{len} is not UTF-8"))))
}

fn write_bytes(g: &mut dyn Guest, ptr: i32, bytes: &[u8]) -> Result<(), Invalid> {
    let Ok(offset) = usize::try_from(ptr) else {
        return Err(Invalid(format!("negative out pointer {ptr}")));
    };
    memory_len(g)?;
    match g.write_memory(offset, bytes) {
        true => Ok(()),
        false => Err(Invalid(format!("out pointer {ptr} is outside module memory"))),
    }
}

pub(crate) fn write(g: &mut dyn Guest, ptr: i32, bytes: &[u8]) -> R<()> {
    write_bytes(g, ptr, bytes).map_err(|e| fail(g, e))
}

/// `trigger_create` copies each action's `text` out of module memory into
/// the definition (appendix "Fixed layouts"). Text that is not UTF-8 or
/// longer than [`ActionDef::MAX_TEXT`] is invalid.
fn read_action_texts(g: &mut dyn Guest, def: &mut TriggerDef) -> R<()> {
    for action in &mut def.actions {
        let len = action.params.text_len;
        if len == 0 {
            continue;
        }
        if usize::try_from(len).is_ok_and(|len| len > ActionDef::MAX_TEXT) {
            return Err(fail(
                g,
                Invalid(format!(
                    "trigger action text of {len} bytes, at most {}",
                    ActionDef::MAX_TEXT
                )),
            ));
        }
        action.text = read_str(g, action.params.text, len)?;
    }
    Ok(())
}

/// Write imports are unavailable in `on_decide` (appendix 1.1, "decide"):
/// calling one is an invalid call, which stops the module.
fn deny_in_decide(g: &mut dyn Guest, import: &str) -> R<()> {
    match g.ctx().export {
        Some(Export::Decide) => Err(fail(
            g,
            Invalid(format!("{import} is a write, unavailable in on_decide")),
        )),
        _ => Ok(()),
    }
}

/// Triggers are unavailable in `vera_init` (appendix 1.1, "all").
fn in_init(g: &dyn Guest) -> bool {
    g.ctx().export == Some(Export::Init)
}

pub(crate) fn frame(g: &mut dyn Guest) -> R<i32> {
    Ok(enter_reads(g, 0)?.frame())
}

pub(crate) fn random(g: &mut dyn Guest, lo: i32, hi: i32, out: i32) -> R<i32> {
    if lo > hi {
        return Err(fail(g, Invalid(format!("random({lo}, {hi}) has lo > hi"))));
    }
    let value = enter(g, 0)?.random(lo, hi);
    write(g, out, &value.to_le_bytes())?;
    Ok(Status::Ok as i32)
}

pub(crate) fn log(g: &mut dyn Guest, ptr: i32, len: i32) -> R<()> {
    charge(g, HOST_CALL_COST)?;
    let bytes = copy_input(g, ptr, len)?;
    let line = String::from_utf8_lossy(&bytes);
    reads(g)?.log(&line);
    Ok(())
}

pub(crate) fn has_kind(g: &mut dyn Guest, table: i32, id: i32) -> R<i32> {
    Ok(i32::from(enter_reads(g, 0)?.has_kind(table, id)))
}

pub(crate) fn lookup(g: &mut dyn Guest, kind: i32, ptr: i32, len: i32, out: i32) -> R<i32> {
    if !(0..NAME_KINDS).contains(&kind) {
        return Err(fail(g, Invalid(format!("unknown name kind {kind}"))));
    }
    charge(g, HOST_CALL_COST)?;
    let bytes = copy_input(g, ptr, len)?;
    // This module's own `vera.actions` answers ModuleAction, its own
    // `vera.notify` Notify (RFC 0001 section 10: a name it did not declare
    // is not found).
    let module_action = std::str::from_utf8(&bytes)
        .ok()
        .filter(|_| kind == NAME_MODULE_ACTION)
        .map(|name| g.ctx().sections.action_key(name));
    // Only a simulation module declares notifications; a client module's
    // `lookup(Notify)` asks its host for the match's declared names.
    let notify = std::str::from_utf8(&bytes)
        .ok()
        .filter(|_| kind == NAME_NOTIFY && g.ctx().client.is_none())
        .map(|name| g.ctx().sections.notify_lookup(name));
    let mut host = reads(g)?;
    // A name that is not UTF-8 names nothing.
    let result = match std::str::from_utf8(&bytes) {
        Err(_) => Ok((-1, Status::NotFound)),
        Ok(name) if kind == NAME_PROPERTY => {
            Ok(property_id(name).map_or((-1, Status::NotFound), |id| (id, Status::Ok)))
        }
        Ok(_) if kind == NAME_MODULE_ACTION => Ok(module_action
            .flatten()
            .map_or((-1, Status::NotFound), |key| (key, Status::Ok))),
        Ok(_) if notify.is_some() => Ok(notify
            .flatten()
            .map_or((-1, Status::NotFound), |id| (id, Status::Ok))),
        Ok(name) if kind == NAME_ACTION_KIND => {
            Ok(action_kind_id(name).map_or((-1, Status::NotFound), |id| (id, Status::Ok)))
        }
        Ok(name) => host.lookup(kind, name),
    };
    let (id, status) = result.map_err(|e| fail(g, e))?;
    let id = if status == Status::Ok { id } else { -1 };
    write(g, out, &id.to_le_bytes())?;
    Ok(status as i32)
}

pub(crate) fn name_of(
    g: &mut dyn Guest,
    kind: i32,
    id: i32,
    out: i32,
    cap: i32,
    len_out: i32,
) -> R<i32> {
    let cap = count_arg(cap, "capacity").map_err(|e| fail(g, e))?;
    if !(0..NAME_KINDS).contains(&kind) {
        return Err(fail(g, Invalid(format!("unknown name kind {kind}"))));
    }
    let module_action = match kind {
        NAME_MODULE_ACTION => Some(g.ctx().sections.action_name(id).map(str::to_owned)),
        NAME_NOTIFY if g.ctx().client.is_none() => {
            Some(g.ctx().sections.notify_name(id).map(str::to_owned))
        }
        _ => None,
    };
    let mut host = enter_reads(g, 0)?;
    let result = if let Some(name) = module_action {
        // Any i32 is a dispatch key or a notification ID; one the module's
        // section lacks has no name.
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
    let (name, status) = result.map_err(|e| fail(g, e))?;
    let name = if status == Status::Ok {
        name
    } else {
        String::new()
    };
    charge(g, name.len() as u64)?;
    // A name longer than `cap` reports its length and writes nothing.
    if name.len() <= cap {
        write(g, out, name.as_bytes())?;
    }
    write(g, len_out, &(name.len() as i32).to_le_bytes())?;
    Ok(status as i32)
}

pub(crate) fn get(g: &mut dyn Guest, reference: i64, prop: i32, out: i32) -> R<i32> {
    if property_name(prop).is_none() {
        return Err(fail(g, Invalid(format!("unknown property {prop}"))));
    }
    let result = enter_reads(g, 0)?.get(reference, prop);
    let (value, status) = result.map_err(|e| fail(g, e))?;
    let value = if status == Status::Ok { value } else { 0 };
    write(g, out, &value.to_le_bytes())?;
    Ok(status as i32)
}

/// `get_many` (appendix "Other layouts"): `n x m` scalar reads, row-major,
/// each with its own empty value; returns the first non-`Ok` status.
pub(crate) fn get_many(
    g: &mut dyn Guest,
    refs: i32,
    n: i32,
    props: i32,
    m: i32,
    out: i32,
    count_out: i32,
) -> R<i32> {
    let n = count_arg(n, "reference count").map_err(|e| fail(g, e))?;
    let m = count_arg(m, "property count").map_err(|e| fail(g, e))?;
    let total = n
        .checked_mul(m)
        .ok_or_else(|| fail(g, Invalid(format!("{n} x {m} values"))))?;
    check_buffer(g, out, total, 8).map_err(|e| fail(g, e))?;
    let refs = check_buffer(g, refs, n, 8).map_err(|e| fail(g, e))?;
    let props = check_buffer(g, props, m, 4).map_err(|e| fail(g, e))?;
    // A query with no references still validates each property. Without
    // properties there is no need to read or allocate the reference list.
    let cost = (total.max(m) as u64).saturating_mul(ENTRY_COST);
    charge(g, HOST_CALL_COST.saturating_add(cost))?;
    let refs: Vec<i64> = if m == 0 {
        Vec::new()
    } else {
        read(g, refs)
            .chunks_exact(8)
            .map(|c| i64::from_le_bytes(c.try_into().unwrap_or_default()))
            .collect()
    };
    let props: Vec<i32> = read(g, props)
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap_or_default()))
        .collect();
    if let Some(bad) = props.iter().find(|p| property_name(**p).is_none()) {
        return Err(fail(g, Invalid(format!("unknown property {bad}"))));
    }
    let mut host = reads(g)?;
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
    result.map_err(|e| fail(g, e))?;
    write(g, out, &values)?;
    write(g, count_out, &(total as i32).to_le_bytes())?;
    Ok(first as i32)
}

pub(crate) fn set(g: &mut dyn Guest, reference: i64, prop: i32, value: i64) -> R<i32> {
    deny_in_decide(g, "set")?;
    if property_name(prop).is_none() {
        return Err(fail(g, Invalid(format!("unknown property {prop}"))));
    }
    let result = enter(g, 0)?.set(reference, prop, value);
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn modify(
    g: &mut dyn Guest,
    reference: i64,
    prop: i32,
    op: i32,
    value: i64,
    frames: i32,
    source: i64,
    group: i32,
    out: i32,
) -> R<i32> {
    deny_in_decide(g, "modify")?;
    if property_name(prop).is_none() || !(0..=2).contains(&op) {
        return Err(fail(
            g,
            Invalid(format!("modify: property {prop}, op {op}")),
        ));
    }
    let result = enter(g, 0)?.modify(reference, prop, op, value, frames, source, group);
    let (handle, status) = result.map_err(|e| fail(g, e))?;
    let handle = if status == Status::Ok { handle } else { 0 };
    write(g, out, &handle.to_le_bytes())?;
    Ok(status as i32)
}

pub(crate) fn unmodify(g: &mut dyn Guest, modifier: i64) -> R<i32> {
    deny_in_decide(g, "unmodify")?;
    let result = enter(g, 0)?.unmodify(modifier);
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

/// `find`: runs a `Query`, writes references to `out` and a `FindResult`
/// (count, reserved, next) to `res` ("Strings and buffers").
pub(crate) fn find(g: &mut dyn Guest, query: i32, len: i32, out: i32, cap: i32, res: i32) -> R<i32> {
    let cap = count_arg(cap, "capacity").map_err(|e| fail(g, e))?;
    let range = input_buffer(g, query, len)?;
    check_buffer(g, out, cap, 8).map_err(|e| fail(g, e))?;
    check_buffer(g, res, 1, 16).map_err(|e| fail(g, e))?;
    charge(g, HOST_CALL_COST)?;
    let bytes = read(g, range);
    let mut query = Query::parse(&bytes).map_err(|e| fail(g, e))?;
    let write_result = |g: &mut dyn Guest, count: usize, next: i64| {
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(&(count as i32).to_le_bytes());
        bytes[8..].copy_from_slice(&next.to_le_bytes());
        write(g, res, &bytes)
    };
    // A valid zero-capacity query scans nothing.
    if cap == 0 {
        write_result(g, 0, 0)?;
        return Ok(Status::Ok as i32);
    }
    // A page costs at most half of a whole call, so that a search too long
    // for one call can be paged across calls, and the host examines no more
    // than what is left pays for.
    let scan_cost = query.scan_cost();
    let page_limit = MAX_SCAN.min(g.ctx().budget / 2 / scan_cost).max(1);
    query.max_scan = page_limit.min(g.meter() / scan_cost);
    let result = reads(g)?.find(&query, cap);
    let (page, status) = result.map_err(|e| fail(g, e))?;
    if page.found.len() > cap {
        return Err(fail(g, Invalid("host returned more than a page".into())));
    }
    if page.resume.is_some() && page.scanned >= query.max_scan && query.max_scan < page_limit {
        // The budget ended the scan, not the page limit.
        return Err(out_of_budget(g));
    }
    let cost = page
        .scanned
        .saturating_mul(scan_cost)
        .saturating_add(page.skipped / SKIPS_PER_OP)
        .saturating_add((page.found.len() as u64).saturating_mul(ENTRY_COST));
    charge(g, cost)?;
    if status != Status::Ok {
        write_result(g, 0, 0)?;
        return Ok(status as i32);
    }
    let refs: Vec<u8> = page
        .found
        .iter()
        .flat_map(|(_, r)| r.to_le_bytes())
        .collect();
    write(g, out, &refs)?;
    // A scan cut short resumes where it stopped, found or not. A full page
    // continues after its last key; the next page may be empty.
    let next = match (page.resume, page.found.last()) {
        (Some(key), _) => key,
        (None, Some((key, _))) if page.found.len() == cap => key.saturating_add(1),
        _ => 0,
    };
    write_result(g, page.found.len(), next)?;
    Ok(Status::Ok as i32)
}

pub(crate) fn var_get(g: &mut dyn Guest, scope: i64, var: i32, out: i32) -> R<i32> {
    let result = enter_reads(g, 0)?.var_get(scope, var);
    let (value, status) = result.map_err(|e| fail(g, e))?;
    write(g, out, &value.to_le_bytes())?;
    Ok(status as i32)
}

pub(crate) fn var_set(g: &mut dyn Guest, scope: i64, var: i32, value: i64) -> R<i32> {
    deny_in_decide(g, "var_set")?;
    let result = enter(g, 0)?.var_set(scope, var, value);
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

pub(crate) fn action(g: &mut dyn Guest, kind: i32, target: i64, ptr: i32, len: i32, out: i32) -> R<i32> {
    deny_in_decide(g, "action")?;
    let range = input_buffer(g, ptr, len)?;
    charge(g, HOST_CALL_COST)?;
    let bytes = read(g, range);
    let params = ActionParams::parse(&bytes).map_err(|e| fail(g, e))?;
    let text = match params.text_len {
        0 => String::new(),
        len => read_str(g, params.text, len)?,
    };
    let result = host(g)?.action(kind, target, &params, &text);
    let (handle, status) = result.map_err(|e| fail(g, e))?;
    write(g, out, &handle.to_le_bytes())?;
    Ok(status as i32)
}

pub(crate) fn trigger_create(g: &mut dyn Guest, ptr: i32, len: i32, out: i32) -> R<i32> {
    deny_in_decide(g, "trigger_create")?;
    let range = input_buffer(g, ptr, len)?;
    check_buffer(g, out, 1, 8).map_err(|e| fail(g, e))?;
    let (handle, status) = if in_init(g) {
        (0, Status::Unsupported)
    } else {
        charge(g, HOST_CALL_COST + range.len() as u64)?;
        let bytes = read(g, range);
        let mut def = TriggerDef::parse(&bytes).map_err(|e| fail(g, e))?;
        read_action_texts(g, &mut def)?;
        let result = host(g)?.trigger_create(&def);
        result.map_err(|e| fail(g, e))?
    };
    let handle = if status == Status::Ok { handle } else { 0 };
    write(g, out, &handle.to_le_bytes())?;
    Ok(status as i32)
}

pub(crate) fn trigger_remove(g: &mut dyn Guest, trigger: i64) -> R<i32> {
    deny_in_decide(g, "trigger_remove")?;
    if in_init(g) {
        return Ok(Status::Unsupported as i32);
    }
    let result = enter(g, 0)?.trigger_remove(trigger);
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

pub(crate) fn trigger_bind(g: &mut dyn Guest, trigger: i64, reference: i64) -> R<i32> {
    deny_in_decide(g, "trigger_bind")?;
    if in_init(g) {
        return Ok(Status::Unsupported as i32);
    }
    let result = enter(g, 0)?.trigger_bind(trigger, reference);
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

pub(crate) fn trigger_unbind(g: &mut dyn Guest, trigger: i64, reference: i64) -> R<i32> {
    deny_in_decide(g, "trigger_unbind")?;
    if in_init(g) {
        return Ok(Status::Unsupported as i32);
    }
    let result = enter(g, 0)?.trigger_unbind(trigger, reference);
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rules_get(
    g: &mut dyn Guest,
    sec: i32,
    sec_len: i32,
    key: i32,
    key_len: i32,
    out: i32,
    cap: i32,
    len_out: i32,
) -> R<i32> {
    let cap = count_arg(cap, "capacity").map_err(|e| fail(g, e))?;
    charge(g, HOST_CALL_COST)?;
    let section = read_str(g, sec, sec_len)?;
    let key = read_str(g, key, key_len)?;
    let result = host(g)?.rules_get(&section, &key);
    let (value, status) = result.map_err(|e| fail(g, e))?;
    let (value, status) = match (value, status) {
        (Some(value), Status::Ok) => (value, Status::Ok),
        (None, Status::Ok) => (String::new(), Status::NotFound),
        (_, status) => (String::new(), status),
    };
    charge(g, value.len() as u64)?;
    // A value longer than `cap` reports its length and writes nothing; a
    // missing key reports -1.
    if value.len() <= cap {
        write(g, out, value.as_bytes())?;
    }
    let len = if status == Status::NotFound {
        -1
    } else {
        value.len() as i32
    };
    write(g, len_out, &len.to_le_bytes())?;
    Ok(status as i32)
}

pub(crate) fn rules_set(
    g: &mut dyn Guest,
    sec: i32,
    sec_len: i32,
    key: i32,
    key_len: i32,
    val: i32,
    val_len: i32,
) -> R<i32> {
    deny_in_decide(g, "rules_set")?;
    charge(g, HOST_CALL_COST)?;
    let section = read_str(g, sec, sec_len)?;
    let key = read_str(g, key, key_len)?;
    let value = read_str(g, val, val_len)?;
    let result = host(g)?.rules_set(&section, &key, &value);
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

/// `send_notify` (RFC 0001 section 10, appendix 1.1 #22): an ID the
/// module's `vera.notify` does not declare is `NotFound`, data over
/// [`MAX_NOTIFY_BYTES`] `Refused`, both before the host is asked; the host
/// counts the frame's notifications. Unavailable in `vera_init`.
pub(crate) fn send_notify(g: &mut dyn Guest, to: i64, id: i32, ptr: i32, len: i32) -> R<i32> {
    deny_in_decide(g, "send_notify")?;
    let range = input_buffer(g, ptr, len)?;
    if in_init(g) {
        return Ok(Status::Unsupported as i32);
    }
    charge(g, HOST_CALL_COST)?;
    if g.ctx().sections.notify_name(id).is_none() {
        return Ok(Status::NotFound as i32);
    }
    if range.len() > MAX_NOTIFY_BYTES {
        return Ok(Status::Refused as i32);
    }
    charge(g, range.len() as u64)?;
    let data = read(g, range);
    let result = host(g)?.send_notify(to, id, &data);
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

pub(crate) fn order(g: &mut dyn Guest, house: i64, ptr: i32, len: i32) -> R<i32> {
    deny_in_decide(g, "order")?;
    let range = input_buffer(g, ptr, len)?;
    let unit_bytes = range.len().saturating_sub(Order::HEADER);
    // Charged per order and per unit entry (RFC 6g).
    let cost = (unit_bytes / size_of::<i64>()).min(Order::MAX_UNITS) as u64 * ENTRY_COST;
    charge(g, HOST_CALL_COST + cost)?;
    let bytes = read(g, range);
    let order = Order::parse(&bytes).map_err(|e| fail(g, e))?;
    let result = host(g)?.order(house, &order);
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

pub(crate) fn ini_register(
    g: &mut dyn Guest,
    kind: i32,
    section: i32,
    ptr: i32,
    len: i32,
    key: i32,
    out: i32,
) -> R<i32> {
    // Only the initialization function registers (RFC 0001 section 6h).
    if !in_init(g) {
        return Err(fail(g, Invalid("ini_register outside vera_init".into())));
    }
    check_buffer(g, out, 1, 4).map_err(|e| fail(g, e))?;
    charge(g, HOST_CALL_COST)?;
    let name = read_str(g, ptr, len)?;
    let sections = match kind {
        INI_KEY => INI_SECTIONS.len() as i32,
        INI_EVENT => 2,
        INI_REGISTRY => 1,
        _ => return Err(fail(g, Invalid(format!("unknown IniKind {kind}")))),
    };
    if !(0..sections).contains(&section) {
        return Err(fail(
            g,
            Invalid(format!("IniKind {kind} takes no section {section}")),
        ));
    }
    if !ini_name_valid(&name) {
        return Err(fail(
            g,
            Invalid(format!("{name:?} is not a registrable INI name")),
        ));
    }
    let result = host(g)?.ini_register(kind, section, &name, key);
    let (id, status) = result.map_err(|e| fail(g, e))?;
    let id = if status == Status::Ok { id } else { 0 };
    write(g, out, &id.to_le_bytes())?;
    Ok(status as i32)
}
