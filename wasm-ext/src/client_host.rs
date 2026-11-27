//! The `vera:client@1.0` imports (appendix 2.1), linked into every runtime
//! by [`for_each_client_import`] as [`crate::host`]'s are by
//! `for_each_import`. Each is a thin adapter onto [`ClientHost`] over the
//! same [`Guest`] helpers: argument checks, the budget charged through the
//! rewrite's meter, and copies in and out of module memory.
//!
//! The reads the appendix defines "as" a simulation import (`client_frame`,
//! `client_log`, `client_has_kind`, `client_lookup`, `client_name_of`,
//! `client_get`, `client_get_many`, `client_find`, `client_var_get`) are
//! [`crate::host`]'s own functions, which reach either world's host. In
//! `client_init` only `client_lookup`, `client_name_of`, `client_has_kind`
//! and `client_log` are available; the others answer `Unsupported` with
//! their empty value, as unavailable imports do in `vera_init` (RFC 0001
//! section 3, "Initialization"), and `client_frame` answers 0.

use crate::client::{NodeInit, client_action, node_kind};
use crate::host::{
    self, Guest, R, charge, check_buffer, copy_input, count_arg, fail, input_buffer, read,
    read_str, write,
};
use crate::{
    ActionParams, ClientHost, ENTRY_COST, HOST_CALL_COST, Invalid, Order, Status, property_name,
};

/// The imports the appendix defines "as" their simulation twins.
pub(crate) use crate::host::{has_kind, log, lookup, name_of};

/// Every client import: this module, its name, the function implementing it
/// and its core Wasm signature. `$wrap!` links one import into a runtime.
macro_rules! for_each_client_import {
    ($wrap:ident) => {
        $wrap!(client_host, "client_frame", frame() -> i32);
        $wrap!(client_host, "client_random", random(lo: i32, hi: i32, out: i32) -> i32);
        $wrap!(client_host, "client_log", log(ptr: i32, len: i32) -> ());
        $wrap!(client_host, "client_has_kind", has_kind(table: i32, id: i32) -> i32);
        $wrap!(client_host, "client_lookup", lookup(kind: i32, ptr: i32, len: i32, out: i32) -> i32);
        $wrap!(client_host, "client_name_of", name_of(kind: i32, id: i32, out: i32, cap: i32, len_out: i32) -> i32);
        $wrap!(client_host, "client_get", get(reference: i64, prop: i32, out: i32) -> i32);
        $wrap!(client_host, "client_get_many", get_many(refs: i32, n: i32, props: i32, m: i32, out: i32, count_out: i32) -> i32);
        $wrap!(client_host, "client_find", find(query: i32, len: i32, out: i32, cap: i32, res: i32) -> i32);
        $wrap!(client_host, "client_var_get", var_get(scope: i64, var: i32, out: i32) -> i32);
        $wrap!(client_host, "client_set", set(reference: i64, prop: i32, value: i64) -> i32);
        $wrap!(client_host, "client_set_bytes", set_bytes(reference: i64, prop: i32, ptr: i32, len: i32) -> i32);
        $wrap!(client_host, "client_node_create", node_create(kind: i32, parent: i64, ptr: i32, len: i32, out: i32) -> i32);
        $wrap!(client_host, "client_node_remove", node_remove(node: i64) -> i32);
        $wrap!(client_host, "client_node_find", node_find(ptr: i32, len: i32, out: i32) -> i32);
        $wrap!(client_host, "client_action", action(kind: i32, target: i64, ptr: i32, len: i32) -> i32);
        $wrap!(client_host, "client_send_order", send_order(ptr: i32, len: i32) -> i32);
        $wrap!(client_host, "client_send_message", send_message(ptr: i32, len: i32) -> i32);
        $wrap!(client_host, "client_storage_read", storage_read(key: i32, key_len: i32, out: i32, cap: i32, len_out: i32) -> i32);
        $wrap!(client_host, "client_storage_write", storage_write(key: i32, key_len: i32, data: i32, data_len: i32) -> i32);
        $wrap!(client_host, "client_status", status_template(ptr: i32, len: i32) -> i32);
    };
}
pub(crate) use for_each_client_import;

/// The client host, for the imports only the client world has.
fn client<'a>(g: &'a mut dyn Guest) -> R<&'a mut dyn ClientHost> {
    match g.ctx().client {
        // SAFETY: see `Ctx::client`.
        Some(mut host) => Ok(unsafe { host.as_mut() }),
        None => Err(fail(
            g,
            Invalid("client call outside a client module call".into()),
        )),
    }
}

/// Charges one host call and hands back the client host.
fn enter<'a>(g: &'a mut dyn Guest, extra: u64) -> R<&'a mut dyn ClientHost> {
    charge(g, HOST_CALL_COST.saturating_add(extra))?;
    client(g)
}

fn in_init(g: &dyn Guest) -> bool {
    g.ctx().client_init
}

/// An import unavailable in `client_init`: writes `empty` (the import's
/// empty value) through `out` when there is one and answers `Unsupported`.
fn unsupported(g: &mut dyn Guest, out: Option<(i32, &[u8])>) -> R<i32> {
    charge(g, HOST_CALL_COST)?;
    if let Some((out, empty)) = out {
        write(g, out, empty)?;
    }
    Ok(Status::Unsupported as i32)
}

/// A status-only call's result.
fn status(g: &mut dyn Guest, result: Result<Status, Invalid>) -> R<i32> {
    Ok(result.map_err(|e| fail(g, e))? as i32)
}

/// A handle-creating call's result: the handle through `out`, 0 unless `Ok`.
fn handle(g: &mut dyn Guest, out: i32, result: Result<(i64, Status), Invalid>) -> R<i32> {
    let (handle, status) = result.map_err(|e| fail(g, e))?;
    let handle = if status == Status::Ok { handle } else { 0 };
    write(g, out, &handle.to_le_bytes())?;
    Ok(status as i32)
}

fn check_property(g: &mut dyn Guest, prop: i32) -> R<()> {
    match property_name(prop) {
        Some(_) => Ok(()),
        None => Err(fail(g, Invalid(format!("unknown property {prop}")))),
    }
}

/// `client_frame`: the latest finished simulation frame; 0 in `client_init`.
pub(crate) fn frame(g: &mut dyn Guest) -> R<i32> {
    if in_init(g) {
        charge(g, HOST_CALL_COST)?;
        return Ok(0);
    }
    host::frame(g)
}

/// `client_random`: a draw from the module's own local xorshift64 stream
/// (display only, never part of the simulation).
pub(crate) fn random(g: &mut dyn Guest, lo: i32, hi: i32, out: i32) -> R<i32> {
    if lo > hi {
        return Err(fail(
            g,
            Invalid(format!("client_random({lo}, {hi}) has lo > hi")),
        ));
    }
    if in_init(g) {
        return unsupported(g, Some((out, &0i32.to_le_bytes())));
    }
    charge(g, HOST_CALL_COST)?;
    let mut x = g.ctx().rng;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    g.ctx_mut().rng = x;
    let span = (i64::from(hi) - i64::from(lo) + 1) as u64;
    let value = (i64::from(lo) + (x % span) as i64) as i32;
    write(g, out, &value.to_le_bytes())?;
    Ok(Status::Ok as i32)
}

pub(crate) fn get(g: &mut dyn Guest, reference: i64, prop: i32, out: i32) -> R<i32> {
    if in_init(g) {
        check_property(g, prop)?;
        return unsupported(g, Some((out, &0i64.to_le_bytes())));
    }
    host::get(g, reference, prop, out)
}

/// `client_get_many`: [`host::get_many`]'s layout over `client_get`. In
/// `client_init` every slot is empty: `n x m` zeros, `count_out` 0.
pub(crate) fn get_many(
    g: &mut dyn Guest,
    refs: i32,
    n: i32,
    props: i32,
    m: i32,
    out: i32,
    count_out: i32,
) -> R<i32> {
    if in_init(g) {
        check_buffer(g, count_out, 1, 4).map_err(|e| fail(g, e))?;
        return unsupported(g, Some((count_out, &0i32.to_le_bytes())));
    }
    host::get_many(g, refs, n, props, m, out, count_out)
}

pub(crate) fn find(
    g: &mut dyn Guest,
    query: i32,
    len: i32,
    out: i32,
    cap: i32,
    res: i32,
) -> R<i32> {
    if in_init(g) {
        return unsupported(g, Some((res, &[0u8; 16])));
    }
    host::find(g, query, len, out, cap, res)
}

pub(crate) fn var_get(g: &mut dyn Guest, scope: i64, var: i32, out: i32) -> R<i32> {
    if in_init(g) {
        return unsupported(g, Some((out, &0i64.to_le_bytes())));
    }
    host::var_get(g, scope, var, out)
}

pub(crate) fn set(g: &mut dyn Guest, reference: i64, prop: i32, value: i64) -> R<i32> {
    check_property(g, prop)?;
    if in_init(g) {
        return unsupported(g, None);
    }
    let result = enter(g, 0)?.set(reference, prop, value);
    status(g, result)
}

/// `client_set_bytes`: a string or bytes property, charged per byte copied.
pub(crate) fn set_bytes(
    g: &mut dyn Guest,
    reference: i64,
    prop: i32,
    ptr: i32,
    len: i32,
) -> R<i32> {
    check_property(g, prop)?;
    if in_init(g) {
        return unsupported(g, None);
    }
    charge(g, HOST_CALL_COST)?;
    let bytes = copy_input(g, ptr, len)?;
    let result = client(g)?.set_bytes(reference, prop, &bytes);
    status(g, result)
}

/// `client_node_create`: `len` bytes of `PropValue` entries (appendix "Other
/// layouts"), charged per entry. An unknown `NodeKind` or property, or a
/// nonzero `reserved`, is invalid.
pub(crate) fn node_create(
    g: &mut dyn Guest,
    kind: i32,
    parent: i64,
    ptr: i32,
    len: i32,
    out: i32,
) -> R<i32> {
    if !(node_kind::TEXT..=node_kind::MAP_MARKER).contains(&kind) {
        return Err(fail(g, Invalid(format!("unknown node kind {kind}"))));
    }
    let range = input_buffer(g, ptr, len)?;
    if range.len() % NodeInit::SIZE != 0 {
        return Err(fail(
            g,
            Invalid(format!(
                "{} bytes of PropValue, not a multiple of {}",
                range.len(),
                NodeInit::SIZE
            )),
        ));
    }
    check_buffer(g, out, 1, 8).map_err(|e| fail(g, e))?;
    if in_init(g) {
        return unsupported(g, Some((out, &0i64.to_le_bytes())));
    }
    let count = (range.len() / NodeInit::SIZE) as u64;
    charge(
        g,
        HOST_CALL_COST.saturating_add(count.saturating_mul(ENTRY_COST)),
    )?;
    let bytes = read(g, range);
    let mut init = Vec::with_capacity(count as usize);
    for entry in bytes.chunks_exact(NodeInit::SIZE) {
        let prop = i32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
        let reserved = i32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
        let mut value = [0u8; 8];
        value.copy_from_slice(&entry[8..16]);
        if property_name(prop).is_none() || reserved != 0 {
            return Err(fail(
                g,
                Invalid(format!("PropValue: property {prop}, reserved {reserved}")),
            ));
        }
        init.push(NodeInit {
            prop,
            value: i64::from_le_bytes(value),
        });
    }
    let result = client(g)?.node_create(kind, parent, &init);
    handle(g, out, result)
}

pub(crate) fn node_remove(g: &mut dyn Guest, node: i64) -> R<i32> {
    if in_init(g) {
        return unsupported(g, None);
    }
    let result = enter(g, 0)?.node_remove(node);
    status(g, result)
}

/// `client_node_find`: a node by `Name`; (0, `NotFound`) without one. A name
/// that is not UTF-8 names nothing.
pub(crate) fn node_find(g: &mut dyn Guest, ptr: i32, len: i32, out: i32) -> R<i32> {
    check_buffer(g, out, 1, 8).map_err(|e| fail(g, e))?;
    if in_init(g) {
        return unsupported(g, Some((out, &0i64.to_le_bytes())));
    }
    charge(g, HOST_CALL_COST)?;
    let bytes = copy_input(g, ptr, len)?;
    let result = match std::str::from_utf8(&bytes) {
        Ok(name) => client(g)?.node_find(name),
        Err(_) => Ok((0, Status::NotFound)),
    };
    handle(g, out, result)
}

/// `client_action` (appendix 5.4): `ActionParams` with its `text` (UTF-8)
/// and `extra` bytes copied out of module memory, both charged per byte. An
/// unknown client action kind is invalid.
pub(crate) fn action(g: &mut dyn Guest, kind: i32, target: i64, ptr: i32, len: i32) -> R<i32> {
    if !(client_action::PLAY_SOUND..=client_action::REGISTER_HOTKEY).contains(&kind) {
        return Err(fail(
            g,
            Invalid(format!("unknown client action kind {kind}")),
        ));
    }
    let range = input_buffer(g, ptr, len)?;
    if in_init(g) {
        return unsupported(g, None);
    }
    charge(g, HOST_CALL_COST)?;
    let bytes = read(g, range);
    let params = ActionParams::parse(&bytes).map_err(|e| fail(g, e))?;
    let text = match params.text_len {
        0 => String::new(),
        len => read_str(g, params.text, len)?,
    };
    let extra = match params.extra_len {
        0 => Vec::new(),
        len => copy_input(g, params.extra, len)?,
    };
    let result = client(g)?.action(kind, target, &params, &text, &extra);
    status(g, result)
}

/// `client_send_order`: an `Order`, charged per order and per unit entry as
/// `order` is (RFC 6g); the host checks it like player input.
pub(crate) fn send_order(g: &mut dyn Guest, ptr: i32, len: i32) -> R<i32> {
    let range = input_buffer(g, ptr, len)?;
    if in_init(g) {
        return unsupported(g, None);
    }
    let unit_bytes = range.len().saturating_sub(Order::HEADER);
    let cost = (unit_bytes / size_of::<i64>()).min(Order::MAX_UNITS) as u64 * ENTRY_COST;
    charge(g, HOST_CALL_COST + cost)?;
    let bytes = read(g, range);
    let order = Order::parse(&bytes).map_err(|e| fail(g, e))?;
    let result = client(g)?.send_order(&order);
    status(g, result)
}

/// `client_send_message`: the bytes as they are; the host applies the size
/// and rate limits (RFC 15.4) and refuses spectators.
pub(crate) fn send_message(g: &mut dyn Guest, ptr: i32, len: i32) -> R<i32> {
    input_buffer(g, ptr, len)?;
    if in_init(g) {
        return unsupported(g, None);
    }
    charge(g, HOST_CALL_COST)?;
    let data = copy_input(g, ptr, len)?;
    let result = client(g)?.send_message(&data);
    status(g, result)
}

/// `client_storage_read`: as `rules_get`, the full length through `len_out`
/// and nothing through `out` when it exceeds `cap`; −1 and `NotFound` for
/// no entry (RFC 10, "Storage").
pub(crate) fn storage_read(
    g: &mut dyn Guest,
    key: i32,
    key_len: i32,
    out: i32,
    cap: i32,
    len_out: i32,
) -> R<i32> {
    let cap = count_arg(cap, "capacity").map_err(|e| fail(g, e))?;
    check_buffer(g, len_out, 1, 4).map_err(|e| fail(g, e))?;
    if in_init(g) {
        return unsupported(g, Some((len_out, &(-1i32).to_le_bytes())));
    }
    charge(g, HOST_CALL_COST)?;
    let key = read_str(g, key, key_len)?;
    let result = client(g)?.storage_read(&key);
    let (value, status) = result.map_err(|e| fail(g, e))?;
    let (value, status) = match (value, status) {
        (Some(value), Status::Ok) => (value, Status::Ok),
        (None, Status::Ok) => (Vec::new(), Status::NotFound),
        (_, status) => (Vec::new(), status),
    };
    charge(g, value.len() as u64)?;
    if value.len() <= cap {
        write(g, out, &value)?;
    }
    let len = if status == Status::NotFound {
        -1
    } else {
        value.len() as i32
    };
    write(g, len_out, &len.to_le_bytes())?;
    Ok(status as i32)
}

/// `client_status`: registers, replaces or (with no elements) removes one
/// of this source's status templates, given as the text of its INI section
/// (`Name=` first). Text that is not UTF-8 is invalid. Charged per byte.
pub(crate) fn status_template(g: &mut dyn Guest, ptr: i32, len: i32) -> R<i32> {
    input_buffer(g, ptr, len)?;
    if in_init(g) {
        return unsupported(g, None);
    }
    charge(g, HOST_CALL_COST)?;
    let bytes = copy_input(g, ptr, len)?;
    let Ok(text) = String::from_utf8(bytes) else {
        return Err(fail(g, Invalid("client_status text is not UTF-8".into())));
    };
    let result = client(g)?.status_template(&text);
    status(g, result)
}

/// `client_storage_write`: `data_len` 0 deletes the key. The host checks the
/// key's form and the quotas and answers `Refused` over them.
pub(crate) fn storage_write(
    g: &mut dyn Guest,
    key: i32,
    key_len: i32,
    data: i32,
    data_len: i32,
) -> R<i32> {
    input_buffer(g, key, key_len)?;
    input_buffer(g, data, data_len)?;
    if in_init(g) {
        return unsupported(g, None);
    }
    charge(g, HOST_CALL_COST)?;
    let key = read_str(g, key, key_len)?;
    let data = copy_input(g, data, data_len)?;
    let result = client(g)?.storage_write(&key, &data);
    status(g, result)
}
