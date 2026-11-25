//! Client extension modules (`ui.wasm`, RFC 0001 section 10, phase 2).
//!
//! The mod's and the map's `ui.wasm` run on the main thread once per
//! render frame: [`UiModules::render_frame`] gathers the frame's client
//! events, calls `client_on_events`, then `client_on_render`. Never during
//! catch-up; a rejoining player's modules get `client_on_start` once it
//! ends. Imports reach the game through [`host::FrameHost`], which hands
//! each call to its owner:
//!
//! - [`tree`]: nodes, the cursor, pointer routing (`Click`, `Hover`,
//!   `Dismiss`, `MapClick`).
//! - [`reads`]: `client_get`/`find`/`var_get`/`lookup` under this player's
//!   view.
//! - [`local`]: client actions, camera and local settings, hotkeys,
//!   selection changes, `Notify` and `SimEvent`, orders and messages into
//!   the command stream.
//! - [`storage`]: each source's local key-value store.

pub(crate) mod host;
pub(crate) mod local;
pub(crate) mod reads;
pub(crate) mod storage;
pub(crate) mod tree;

pub(crate) use vera20k_wasm_ext::client::Source;

/// Whose view a client module reads with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Viewer {
    /// This player's house handle (reference kind 0); 0 for a spectator.
    pub house: i64,
    pub spectator: bool,
    /// A spectator in a room that shows the whole map.
    pub sees_whole_map: bool,
}

/// The match's loaded client modules and their per-match state.
pub(crate) struct UiModules {
    // Owned by the runtime/glue work: loaded modules per source, the
    // `MapUi` policy, `tree::UiTree`, `local::LocalState`, storages.
}
