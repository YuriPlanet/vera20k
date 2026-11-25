//! Client nodes (appendix 7.10) on the in-match [`vera20k_gui::Gui`]: who
//! may write which node (RFC 10, "Node ownership"), lookup by part name,
//! the cursor reference, and pointer routing into client events.

use vera20k_gui::Gui;
use vera20k_wasm_ext::{ClientEvent, Invalid, NodeInit, Source, Status};

use crate::sim::ext_modules::MapUi;

/// Client node handles and their sources, over the in-match `Gui`.
pub(crate) struct UiTree {}

impl UiTree {
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// `client_node_create`.
    pub(crate) fn create(
        &mut self,
        _gui: &mut Gui,
        _source: Source,
        _map_ui: MapUi,
        _kind: i32,
        _parent: i64,
        _init: &[NodeInit],
    ) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }

    /// `client_node_remove`.
    pub(crate) fn remove(
        &mut self,
        _gui: &mut Gui,
        _source: Source,
        _map_ui: MapUi,
        _node: i64,
    ) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }

    /// `client_node_find`.
    pub(crate) fn find(&self, _gui: &Gui, _source: Source, _name: &str) -> (i64, Status) {
        (0, Status::NotFound)
    }

    /// `client_get` on a node (kind 5) or the cursor (kind 6, 3).
    pub(crate) fn get(
        &self,
        _gui: &Gui,
        _reference: i64,
        _prop: i32,
    ) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }

    /// `client_set` on a node or the cursor.
    pub(crate) fn set(
        &mut self,
        _gui: &mut Gui,
        _source: Source,
        _map_ui: MapUi,
        _reference: i64,
        _prop: i32,
        _value: i64,
    ) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }

    /// `client_set_bytes` on a node.
    pub(crate) fn set_bytes(
        &mut self,
        _gui: &mut Gui,
        _source: Source,
        _map_ui: MapUi,
        _reference: i64,
        _prop: i32,
        _bytes: &[u8],
    ) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }

    /// Takes away every node a source's module made (module disabled or
    /// unloaded).
    pub(crate) fn remove_source(&mut self, _gui: &mut Gui, _source: Source) {}

    /// The frame's `Click`, `Hover`, `Dismiss` and `MapClick` events, for
    /// every client module.
    pub(crate) fn take_events(&mut self, _out: &mut Vec<ClientEvent>) {}
}
