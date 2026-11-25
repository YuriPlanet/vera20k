//! Client reads (`client_get`, `client_get_many`, `client_find`,
//! `client_var_get`, `client_lookup`, `client_name_of`,
//! `client_has_kind`): only what this player's own interface can show
//! (RFC 10, "Client reads"; appendix 7 "client" column).

use vera20k_wasm_ext::{FindPage, Invalid, Query, Status};

use super::Viewer;
use crate::sim::runtime::SimRuntime;

/// Local facts a read needs beside the world.
#[derive(Default)]
pub(crate) struct LocalFacts<'a> {
    /// This player's selection (`Selected`).
    pub selected: &'a [u64],
    /// (object, control group) pairs (`ControlGroup`).
    pub control_groups: &'a [(u64, i32)],
}

/// One client's view of the world for the duration of a call.
pub(crate) struct ClientView<'a> {
    pub runtime: &'a SimRuntime,
    pub viewer: Viewer,
    pub local: LocalFacts<'a>,
}

/// `client_get` for simulation references (kinds 0..4).
pub(crate) fn get(
    _view: &ClientView<'_>,
    _reference: i64,
    _prop: i32,
) -> Result<(i64, Status), Invalid> {
    Ok((0, Status::Unsupported))
}

/// `client_find`.
pub(crate) fn find(
    _view: &ClientView<'_>,
    _query: &Query,
    _cap: usize,
) -> Result<(FindPage, Status), Invalid> {
    Ok((FindPage::default(), Status::Unsupported))
}

/// `client_var_get`.
pub(crate) fn var_get(
    _view: &ClientView<'_>,
    _scope: i64,
    _var: i32,
) -> Result<(i64, Status), Invalid> {
    Ok((0, Status::Unsupported))
}

/// `client_lookup` for every name kind but `Hotkey` (that one is
/// [`super::local`]'s).
pub(crate) fn lookup(
    _view: &ClientView<'_>,
    _kind: i32,
    _name: &str,
) -> Result<(i32, Status), Invalid> {
    Ok((-1, Status::Unsupported))
}

/// `client_name_of`.
pub(crate) fn name_of(
    _view: &ClientView<'_>,
    _kind: i32,
    _id: i32,
) -> Result<(String, Status), Invalid> {
    Ok((String::new(), Status::Unsupported))
}

/// `client_has_kind`.
pub(crate) fn has_kind(_table: i32, _id: i32) -> bool {
    false
}
