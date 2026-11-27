//! [`vera20k_wasm_ext::ClientHost`] for one module call: routes each import to
//! [`super::tree`], [`super::reads`], [`super::local`] or
//! [`super::storage`] (RFC 0001 section 10; appendix section 2).
//!
//! A [`FrameHost`] lives for one call into one module (`client_init`,
//! `client_on_start`, `client_on_events`, `client_on_render`). It borrows the
//! parts the call may reach and the source it speaks for, and gathers what
//! the call leaves for the app: [`LocalEffects`] and log lines.
//!
//! Routing (appendix "References", 7.10):
//!
//! | Call | Reference / name kind | Owner |
//! | --- | --- | --- |
//! | `get`, `set`, `set_bytes` | node (kind 5), cursor (kind 6, 3) | [`UiTree`] |
//! | `get`, `set` | camera, local settings (kind 6, 1 and 2) | [`LocalState::global_get`] / [`LocalState::global_set`] |
//! | `get` | any other reference (kinds 0..4) | [`super::reads::get`] |
//! | `set`, `set_bytes` | simulation references | `Refused`: clients write only client-owned references |
//! | `lookup` | `Notify` | [`crate::sim::ext_modules::notify_id`] |
//! | `lookup` | `Hotkey` | [`LocalState::hotkey_id`] |
//! | `lookup` | any other kind | [`super::reads::lookup`] |
//! | `storage_*` | — | the source's [`Storage`] |

use vera20k_gui::Gui;
use vera20k_wasm_ext::client::{
    CLIENT_CAMERA, CLIENT_CURSOR, CLIENT_SETTINGS, REF_KIND_CLIENT_GLOBAL, REF_KIND_NODE,
};
use vera20k_wasm_ext::{
    ActionParams, ClientHost, FindPage, Invalid, NAME_HOTKEY, NAME_NOTIFY, NodeInit, Order, Query,
    Source, Status,
};

use super::local::{LocalEffects, LocalInputs, LocalState};
use super::reads::{self, ClientView};
use super::storage::Storage;
use super::tree::UiTree;
use crate::sim::ext_modules::MapUi;


/// Log lines one module keeps per render frame, and bytes per line
/// (RFC 15.4 "Log"); the excess is dropped.
pub(crate) const LOG_LINES_PER_FRAME: usize = 64;
pub(crate) const LOG_LINE_BYTES: usize = 256;

/// Bits 48.. of a reference: its kind (appendix "References").
const REF_KIND_SHIFT: u32 = 48;
/// The global rules reference (kind 4, payload 0), whose `AutomationAllowed`
/// says whether `client_send_order` may play for this player (7.8).
const GLOBAL_RULES: i64 = 4 << REF_KIND_SHIFT;

/// Which part owns a reference for `get`/`set`/`set_bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefRoute {
    /// A node (kind 5) or the cursor (kind 6, 3).
    Tree,
    /// The camera or the local settings (kind 6, 1 and 2).
    LocalGlobal,
    /// Another client global (kind 6): none exist in API 1.0.
    UnknownClientGlobal,
    /// A simulation reference (kinds 0..4), read under this player's view.
    Reads,
}

pub(crate) fn ref_route(reference: i64) -> RefRoute {
    let kind = reference >> REF_KIND_SHIFT;
    let payload = reference & ((1 << REF_KIND_SHIFT) - 1);
    match kind {
        REF_KIND_NODE => RefRoute::Tree,
        REF_KIND_CLIENT_GLOBAL => match payload {
            CLIENT_CURSOR => RefRoute::Tree,
            CLIENT_CAMERA | CLIENT_SETTINGS => RefRoute::LocalGlobal,
            _ => RefRoute::UnknownClientGlobal,
        },
        _ => RefRoute::Reads,
    }
}

/// Which part answers `client_lookup` for a name kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LookupRoute {
    Notify,
    Hotkey,
    Reads,
}

pub(crate) fn lookup_route(kind: i32) -> LookupRoute {
    match kind {
        NAME_NOTIFY => LookupRoute::Notify,
        NAME_HOTKEY => LookupRoute::Hotkey,
        _ => LookupRoute::Reads,
    }
}

/// Keeps `line` in `log` within the per-frame limits (RFC 15.4 "Log"): at
/// most [`LOG_LINES_PER_FRAME`] lines, each cut to [`LOG_LINE_BYTES`] bytes
/// on a character boundary.
pub(crate) fn keep_log_line(log: &mut Vec<String>, line: &str) {
    if log.len() >= LOG_LINES_PER_FRAME {
        return;
    }
    let mut end = line.len().min(LOG_LINE_BYTES);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    log.push(line[..end].to_owned());
}

/// What one module call reaches: the parts, the view it reads with, and
/// where its effects and log lines go.
pub(crate) struct FrameHost<'a, 'v> {
    pub source: Source,
    pub map_ui: MapUi,
    pub view: &'a ClientView<'v>,
    pub inputs: &'a LocalInputs,
    pub gui: &'a mut Gui,
    pub tree: &'a mut UiTree,
    pub status: &'a mut super::status::ModuleStatus,
    pub local: &'a mut LocalState,
    pub storage: &'a mut Storage,
    pub effects: &'a mut LocalEffects,
    /// This call's log lines, for the in-game console.
    pub log: &'a mut Vec<String>,
}

impl FrameHost<'_, '_> {
    /// `AutomationAllowed` on the global rules reference, as this client
    /// reads it (7.8).
    fn automation_allowed(&self) -> bool {
        vera20k_wasm_ext::property_id("AutomationAllowed").is_some_and(|prop| {
            matches!(
                reads::get(self.view, GLOBAL_RULES, prop),
                Ok((value, Status::Ok)) if value != 0
            )
        })
    }
}

impl ClientHost for FrameHost<'_, '_> {
    /// The latest finished simulation frame. The session's binary frame is
    /// the one about to run, so the last finished one is just before it.
    fn frame(&mut self) -> i32 {
        let frame = self.view.runtime.simulation.session().binary_frame();
        frame.saturating_sub(1) as i32
    }

    fn log(&mut self, line: &str) {
        log::info!(target: "ext_module", "{:?} ui.wasm: {line}", self.source);
        keep_log_line(self.log, line);
    }

    fn has_kind(&mut self, table: i32, id: i32) -> bool {
        reads::has_kind(self.view, table, id)
    }

    fn lookup(&mut self, kind: i32, name: &str) -> Result<(i32, Status), Invalid> {
        match lookup_route(kind) {
            LookupRoute::Notify => Ok(
                match crate::sim::ext_modules::notify_id(&self.view.runtime.simulation, name) {
                    Some(id) => (id, Status::Ok),
                    None => (-1, Status::NotFound),
                },
            ),
            LookupRoute::Hotkey => Ok(self.local.hotkey_id(self.source, name)),
            LookupRoute::Reads => reads::lookup(self.view, kind, name),
        }
    }

    fn name_of(&mut self, kind: i32, id: i32) -> Result<(String, Status), Invalid> {
        reads::name_of(self.view, kind, id)
    }

    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        match ref_route(reference) {
            RefRoute::Tree => self.tree.get(self.gui, reference, prop),
            RefRoute::LocalGlobal => {
                self.local
                    .global_get(self.inputs, self.view.viewer, reference, prop)
            }
            RefRoute::UnknownClientGlobal => Ok((0, Status::NotFound)),
            RefRoute::Reads => reads::get(self.view, reference, prop),
        }
    }

    fn find(&mut self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        reads::find(self.view, query, cap)
    }

    fn var_get(&mut self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        reads::var_get(self.view, scope, var)
    }

    fn set(&mut self, reference: i64, prop: i32, value: i64) -> Result<Status, Invalid> {
        match ref_route(reference) {
            RefRoute::Tree => {
                self.tree
                    .set(self.gui, self.source, self.map_ui, reference, prop, value)
            }
            RefRoute::LocalGlobal => self.local.global_set(self.effects, reference, prop, value),
            RefRoute::UnknownClientGlobal => Ok(Status::NotFound),
            // Clients never write simulation state (RFC 10, "Client reads").
            RefRoute::Reads => Ok(Status::Refused),
        }
    }

    fn set_bytes(&mut self, reference: i64, prop: i32, bytes: &[u8]) -> Result<Status, Invalid> {
        match ref_route(reference) {
            RefRoute::Tree => {
                self.tree
                    .set_bytes(self.gui, self.source, self.map_ui, reference, prop, bytes)
            }
            // The camera and local settings have no string or bytes
            // property in API 1.0.
            RefRoute::LocalGlobal => Ok(Status::Refused),
            RefRoute::UnknownClientGlobal => Ok(Status::NotFound),
            RefRoute::Reads => Ok(Status::Refused),
        }
    }

    fn node_create(
        &mut self,
        kind: i32,
        parent: i64,
        init: &[NodeInit],
    ) -> Result<(i64, Status), Invalid> {
        self.tree
            .create(self.gui, self.source, self.map_ui, kind, parent, init)
    }

    fn node_remove(&mut self, node: i64) -> Result<Status, Invalid> {
        self.tree.remove(self.gui, self.source, self.map_ui, node)
    }

    fn node_find(&mut self, name: &str) -> Result<(i64, Status), Invalid> {
        Ok(self.tree.find(self.gui, self.source, name))
    }

    fn action(
        &mut self,
        kind: i32,
        target: i64,
        params: &ActionParams,
        text: &str,
        extra: &[u8],
    ) -> Result<Status, Invalid> {
        self.local.action(
            self.effects,
            self.view,
            self.source,
            kind,
            target,
            params,
            text,
            extra,
        )
    }

    fn send_order(&mut self, order: &Order) -> Result<Status, Invalid> {
        let automation_allowed = self.automation_allowed();
        self.local
            .send_order(self.effects, self.view, automation_allowed, order)
    }

    fn send_message(&mut self, data: &[u8]) -> Result<Status, Invalid> {
        self.local
            .send_message(self.effects, self.view, self.source, data)
    }

    fn storage_read(&mut self, key: &str) -> Result<(Option<Vec<u8>>, Status), Invalid> {
        self.storage.read(key)
    }

    fn storage_write(&mut self, key: &str, data: &[u8]) -> Result<Status, Invalid> {
        self.storage.write(key, data)
    }

    fn status_template(&mut self, text: &str) -> Result<Status, Invalid> {
        let sim = &self.view.runtime.simulation;
        let type_id = |name: &str| sim.interner().get(name).map(|id| id.index());
        let names = super::status::Names {
            type_id: &type_id,
            vars: sim.ext_modules().vars(),
        };
        let origin = super::status::StatusOrigin::of_module(self.source);
        Ok(match self.status.add_text(origin, text, self.map_ui, &names) {
            Ok(()) => Status::Ok,
            Err(problem) => {
                ClientHost::log(self, &format!("client_status: {problem}"));
                Status::Refused
            }
        })
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    fn client_global(payload: i64) -> i64 {
        (REF_KIND_CLIENT_GLOBAL << REF_KIND_SHIFT) | payload
    }

    #[test]
    fn references_go_to_their_owners() {
        assert_eq!(ref_route((REF_KIND_NODE << 48) | 7), RefRoute::Tree);
        assert_eq!(ref_route(client_global(CLIENT_CURSOR)), RefRoute::Tree);
        assert_eq!(
            ref_route(client_global(CLIENT_CAMERA)),
            RefRoute::LocalGlobal
        );
        assert_eq!(
            ref_route(client_global(CLIENT_SETTINGS)),
            RefRoute::LocalGlobal
        );
        assert_eq!(ref_route(client_global(9)), RefRoute::UnknownClientGlobal);
        // An object serial, a house (bit 47), the global rules, a cell.
        assert_eq!(ref_route(12345), RefRoute::Reads);
        assert_eq!(
            ref_route(crate::sim::ext_modules::house_ref(2)),
            RefRoute::Reads
        );
        assert_eq!(ref_route(GLOBAL_RULES), RefRoute::Reads);
        assert_eq!(ref_route((2 << 48) | (5 << 16) | 9), RefRoute::Reads);
        assert_eq!(ref_route(0), RefRoute::Reads);
        assert_eq!(ref_route(-1), RefRoute::Reads);
    }

    #[test]
    fn notify_and_hotkey_names_leave_the_reads() {
        assert_eq!(lookup_route(NAME_NOTIFY), LookupRoute::Notify);
        assert_eq!(lookup_route(NAME_HOTKEY), LookupRoute::Hotkey);
        assert_eq!(
            lookup_route(vera20k_wasm_ext::NAME_VARIABLE),
            LookupRoute::Reads
        );
        assert_eq!(
            lookup_route(vera20k_wasm_ext::NAME_TECHNO_TYPE),
            LookupRoute::Reads
        );
    }

    #[test]
    fn log_lines_keep_the_frame_limits() {
        let mut log = Vec::new();
        let long = "é".repeat(200); // 400 bytes
        keep_log_line(&mut log, &long);
        assert!(log[0].len() <= LOG_LINE_BYTES);
        assert!(long.starts_with(&log[0]));
        for _ in 0..100 {
            keep_log_line(&mut log, "x");
        }
        assert_eq!(log.len(), LOG_LINES_PER_FRAME);
    }
}
