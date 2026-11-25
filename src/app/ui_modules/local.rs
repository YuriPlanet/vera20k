//! Local client state: actions (appendix 5.4), the camera and local
//! settings references, hotkeys, selection changes, `Notify` and `SimEvent`
//! delivery, and the two ways into the command stream
//! (`client_send_order`, `client_send_message`).
//!
//! Effects that need the app (sound, camera, selection, messages, commands)
//! are queued in [`LocalEffects`]; the runtime applies them after the call.

use vera20k_wasm_ext::{ActionParams, ClientEvent, Invalid, Order, Source, Status};

use super::Viewer;
use crate::sim::command::Command;

/// What the app applies after a module call.
#[derive(Default)]
pub(crate) struct LocalEffects {
    /// Commands for the local player's command stream, in order.
    pub commands: Vec<Command>,
}

/// Local facts the camera and settings references read.
pub(crate) struct LocalInputs {
    /// The cell at the camera's centre.
    pub camera_cell: Option<(u16, u16)>,
}

/// Per-match local client state.
pub(crate) struct LocalState {}

impl LocalState {
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// `client_action`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn action(
        &mut self,
        _effects: &mut LocalEffects,
        _viewer: Viewer,
        _source: Source,
        _kind: i32,
        _target: i64,
        _params: &ActionParams,
        _text: &str,
        _extra: &[u8],
    ) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }

    /// `client_get` on the camera or local settings (kind 6, 1 and 2).
    pub(crate) fn global_get(
        &self,
        _inputs: &LocalInputs,
        _viewer: Viewer,
        _reference: i64,
        _prop: i32,
    ) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }

    /// `client_set` on the camera or local settings.
    pub(crate) fn global_set(
        &mut self,
        _effects: &mut LocalEffects,
        _reference: i64,
        _prop: i32,
        _value: i64,
    ) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }

    /// `client_lookup(Hotkey, name)`.
    pub(crate) fn hotkey_id(&self, _source: Source, _name: &str) -> (i32, Status) {
        (-1, Status::NotFound)
    }

    /// `client_send_order`.
    pub(crate) fn send_order(
        &mut self,
        _effects: &mut LocalEffects,
        _viewer: Viewer,
        _automation_allowed: bool,
        _order: &Order,
    ) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }

    /// `client_send_message`.
    pub(crate) fn send_message(
        &mut self,
        _effects: &mut LocalEffects,
        _viewer: Viewer,
        _source: Source,
        _data: &[u8],
    ) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }

    /// The frame's `Hotkey`, `SelectionChanged`, `Notify` and `SimEvent`
    /// events for `source`'s module.
    pub(crate) fn take_events(&mut self, _source: Source, _out: &mut Vec<ClientEvent>) {}
}
