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
//!
//! ## Lifetime
//!
//! The loader reads the modules' bytes with the scenario
//! ([`UiModuleSources::read`], beside `rules.wasm`); [`install`] builds the
//! match's [`UiModules`] once the world and its presentation are in place,
//! loading the mod's module, then the map's (unless the mod says
//! `[VeraWasm] MapUI=no`), and runs `client_init`. They live in
//! `MatchState::ui_modules`, an `Option` that stays `None` for a match
//! without client modules or map `[VeraUI]` parts, so such a match pays
//! nothing. A new scenario or a restored save drops them
//! (`reset_scenario_exit_runtime`).
//!
//! ## Frame order
//!
//! Per simulation frame, at the tick's tail (`sim_tick::after_frame`):
//! [`collect_sim_outbox`] takes the simulation's client outbox and keeps it
//! for the next render frame; during catch-up it is dropped.
//!
//! Per render frame (`App::render_frame`, after the match service and before
//! the picture is judged), [`render_frame`]:
//!
//! 1. Catch-up (a rejoin's restore or a watcher catching up): nothing runs;
//!    the outbox is dropped and `client_on_start` is owed again.
//! 2. `client_on_start` for each module, once since load or catch-up.
//! 3. `local.deliver_outbox` (the kept `Notify`/`SimEvent`s), then
//!    `local.observe_selection`.
//! 4. The tree's pointer events (for every module), then per module, mod
//!    first: its local events, `begin_frame`, `client_on_events`,
//!    `client_on_render(dt, alpha)`.
//! 5. Log lines to the in-game console; queued commands into the local
//!    player's command path; `local::apply` for the other effects.
//!
//! A fault disables that module alone: its nodes go
//! ([`tree::UiTree::remove_source`]), its player is told once, and the match
//! plays on (RFC section 3, "Error model").
//!
//! Input reaches the tree before the game ([`pointer_moved`],
//! [`pointer_button`], [`key_escape`]); a key the game did not use goes to
//! the modules' hotkeys ([`key_unused`]), which know the game's bound keys
//! ([`rebind_game_keys`]).

pub(crate) mod host;
pub(crate) mod local;
pub(crate) mod reads;
pub(crate) mod storage;
pub(crate) mod tree;

use std::path::PathBuf;
use std::sync::Arc;

use vera20k_gui::Gui;
use vera20k_wasm_ext::{ClientEvent, ClientModule, Fault, Limits};

pub(crate) use vera20k_wasm_ext::client::Source;

use crate::app::AppState;
use crate::rules::ini_parser::IniFile;
use crate::sim::ext_modules::{ClientOutbox, MapUi};
use crate::util::time::Instant;
use host::{ClientRng, FrameHost};
use local::{LocalEffects, LocalInputs, LocalState};
use reads::{ClientView, LocalFacts};
use storage::Storage;
use tree::UiTree;

/// Whose view a client module reads with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Viewer {
    /// This player's house handle (reference kind 0); 0 for a spectator.
    pub house: i64,
    pub spectator: bool,
    /// A spectator in a room that shows the whole map.
    pub sees_whole_map: bool,
}

/// Nodes the client overlay holds at most (engine, INI and module nodes).
const MAX_CLIENT_NODES: usize = 4096;
/// Module log lines shown in the console per module and render frame; the
/// rest go to the log file only (as the simulation modules' `log`).
const SHOWN_LOG_LINES: usize = 8;
/// Section prefix of a map's own interface parts (`[VeraUI]`,
/// `[VeraUI.<part>]`, RFC 10 "Static layouts").
const MAP_LAYOUT_PREFIX: &str = "VeraUI";

/// What the loader read for the match's client modules, before the world is
/// installed: the bytes, the map's `[VeraUI]` sections and its storage key.
#[derive(Default)]
pub(crate) struct UiModuleSources {
    pub mod_module: Option<Arc<[u8]>>,
    pub map_module: Option<Vec<u8>>,
    /// The map's `[VeraUI]` and `[VeraUI.<part>]` sections, if any.
    pub map_layout: Option<IniFile>,
    /// `[VeraWasm] StorageId=`, else the map's file name.
    pub map_storage_id: String,
    /// Why a module could not be read; told to the player, the match plays
    /// without it.
    pub problems: Vec<String>,
}

impl UiModuleSources {
    /// The mod's loose `ui.wasm` (as `rules.wasm`,
    /// `PreparedExtModules::read_mod_file`) and the map's embedded one
    /// (`[VeraWasmUIPack]`). A client module is presentation only, so one
    /// that cannot be read is left out with a notice instead of stopping
    /// the match.
    pub(crate) fn read(
        assets: &crate::assets::asset_manager::AssetManager,
        map: &IniFile,
        map_source: &crate::map::source::LoadedMapSource,
    ) -> Self {
        let mut sources = Self::default();
        match assets.read_mod_file_bounded("ui.wasm", crate::map::ext_module_pack::MAX_MODULE_BYTES)
        {
            Ok(bytes) => sources.mod_module = bytes,
            Err(error) => sources.problems.push(format!("the mod's ui.wasm: {error}")),
        }
        match crate::map::ext_module_pack::ui_module(map) {
            Ok(bytes) => sources.map_module = bytes,
            Err(error) => sources.problems.push(format!("the map's ui.wasm: {error}")),
        }
        let layout: Vec<_> = map
            .section_names()
            .into_iter()
            .filter(|name| is_map_layout_section(name))
            .filter_map(|name| map.section(name).cloned())
            .collect();
        if !layout.is_empty() {
            sources.map_layout = Some(IniFile::from_sections(layout));
        }
        sources.map_storage_id = map
            .section_or_empty(crate::map::ext_module_pack::SECTION)
            .read_name("StorageId", 64)
            .map(str::to_owned)
            .unwrap_or_else(|| map_file_name(map_source));
        sources
    }

    fn is_empty(&self) -> bool {
        self.mod_module.is_none() && self.map_module.is_none() && self.map_layout.is_none()
    }
}

/// `VeraUI` or `VeraUI.<part>`, case as the INI reader keeps it.
fn is_map_layout_section(name: &str) -> bool {
    name.len() >= MAP_LAYOUT_PREFIX.len()
        && name[..MAP_LAYOUT_PREFIX.len()].eq_ignore_ascii_case(MAP_LAYOUT_PREFIX)
        && (name.len() == MAP_LAYOUT_PREFIX.len()
            || name.as_bytes()[MAP_LAYOUT_PREFIX.len()] == b'.')
}

/// The map's file name, the default map storage key (RFC 10, "Storage").
fn map_file_name(source: &crate::map::source::LoadedMapSource) -> String {
    use crate::map::source::LoadedMapSource as S;
    match source {
        S::Loose { path, .. } => path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        S::Mix { logical_name, .. } | S::Bundled { logical_name, .. } => logical_name.clone(),
        S::Generated { seed_name, .. } => seed_name.clone(),
        S::LegacyFallback { label } => label.clone(),
    }
}

/// One loaded client module.
struct Slot {
    source: Source,
    module: ClientModule,
    storage: Storage,
}

/// The match's loaded client modules and their per-match state.
pub(crate) struct UiModules {
    /// Mod first, then map (RFC section 5's source order).
    slots: Vec<Slot>,
    /// The mod's `[VeraWasm] MapUI=`.
    map_ui: MapUi,
    /// The client overlay the tree's nodes live on.
    gui: Gui,
    tree: UiTree,
    local: LocalState,
    rng: ClientRng,
    /// `client_on_start` given since load or the last catch-up.
    started: bool,
    /// What the simulation's frames since the last render frame sent the
    /// clients ([`collect_sim_outbox`]).
    outbox: ClientOutbox,
    /// The last render frame, for `dt`.
    last_render: Option<Instant>,
    /// Buttons (bit 0 left, bit 1 right) whose press the tree consumed: their
    /// release is the tree's too.
    consumed_buttons: u8,
    /// Lines for the console that came before it could show them (load).
    pending_lines: Vec<(String, bool)>,
}

/// What one round of calls left for the app.
#[derive(Default)]
struct CallOutput {
    effects: LocalEffects,
    /// (line, is a fault) for the in-game console.
    lines: Vec<(String, bool)>,
}

impl UiModules {
    fn new(map_ui: MapUi) -> Self {
        let seed = crate::util::time::SystemTime::now()
            .duration_since(crate::util::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9_7F4A_7C15, |since| since.as_nanos() as u64);
        Self {
            slots: Vec::new(),
            map_ui,
            gui: Gui::new(MAX_CLIENT_NODES),
            tree: UiTree::new(),
            local: LocalState::new(),
            rng: ClientRng::new(seed),
            started: false,
            outbox: ClientOutbox::default(),
            last_render: None,
            consumed_buttons: 0,
            pending_lines: Vec::new(),
        }
    }

    /// The client overlay, for drawing.
    pub(crate) fn gui(&self) -> &Gui {
        &self.gui
    }

    /// Runs `call` on every loaded module, mod first. A fault disables that
    /// module: its nodes go and its player is told.
    fn call_each(
        &mut self,
        view: &ClientView<'_>,
        inputs: &LocalInputs,
        out: &mut CallOutput,
        mut call: impl FnMut(&mut ClientModule, &mut FrameHost<'_, '_>) -> Result<(), Fault>,
    ) {
        let Self {
            slots,
            map_ui,
            gui,
            tree,
            local,
            rng,
            ..
        } = self;
        let mut failed = Vec::new();
        for (index, slot) in slots.iter_mut().enumerate() {
            let mut log = Vec::new();
            let result = {
                let mut host = FrameHost {
                    source: slot.source,
                    map_ui: *map_ui,
                    view,
                    inputs,
                    gui: &mut *gui,
                    tree: &mut *tree,
                    local: &mut *local,
                    storage: &mut slot.storage,
                    effects: &mut out.effects,
                    rng: &mut *rng,
                    log: &mut log,
                };
                call(&mut slot.module, &mut host)
            };
            out.lines.extend(
                log.into_iter()
                    .take(SHOWN_LOG_LINES)
                    .map(|line| (line, false)),
            );
            if let Err(fault) = result {
                failed.push((index, fault));
            }
        }
        for (index, fault) in failed.into_iter().rev() {
            let slot = self.slots.remove(index);
            self.tree.remove_source(&mut self.gui, slot.source);
            let reason = format!("{} ui.wasm {fault}", source_label(slot.source));
            log::warn!(target: "ext_module", "client module stopped: {reason}");
            out.lines.push((reason, true));
        }
    }

    /// One render frame of every module (see the module docs, "Frame
    /// order"), steps 2..4.
    fn run_frame(
        &mut self,
        view: &ClientView<'_>,
        inputs: &LocalInputs,
        selected: &[u64],
        dt: f32,
        out: &mut CallOutput,
    ) {
        if !self.started {
            self.started = true;
            self.call_each(view, inputs, out, |module, host| module.start(host));
        }
        let outbox = std::mem::take(&mut self.outbox);
        self.local.deliver_outbox(&outbox, view);
        self.local.observe_selection(selected);
        let mut shared = Vec::new();
        self.tree.take_events(&mut shared);
        let mut per_source: Vec<(Source, Vec<ClientEvent>)> = self
            .slots
            .iter()
            .map(|slot| {
                let mut events = shared.clone();
                self.local.take_events(slot.source, &mut events);
                (slot.source, events)
            })
            .collect();
        // Render-frame interpolation: the picture shows the latest finished
        // simulation frame as it is, so no fraction of the next one.
        let alpha = 0.0;
        self.call_each(view, inputs, out, |module, host| {
            let events = per_source
                .iter_mut()
                .find(|(source, _)| *source == host.source)
                .map(|(_, events)| std::mem::take(events))
                .unwrap_or_default();
            module.begin_frame();
            if !events.is_empty() {
                module.events(host, &events)?;
            }
            module.render(host, dt, alpha)
        });
    }
}

fn source_label(source: Source) -> &'static str {
    match source {
        Source::Mod => "the mod's",
        Source::Map => "the map's",
    }
}

/// Whether the match is catching up: a rejoin restoring the match or a
/// watcher playing its way to the present (RFC 11: client modules do not
/// run, notifications are suspended).
pub(crate) fn catching_up(state: &AppState) -> bool {
    state
        .match_state
        .live_network
        .as_ref()
        .is_some_and(|live| live.restoring_match())
        || crate::app::match_runtime::watching::catching_up(
            state,
            crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32,
        )
}

/// This player's view (RFC 10, "Client reads"). A watcher is a spectator
/// that sees the whole map on the observer's view; an observer seat sees the
/// whole map; a replay views as the player it follows (the session's current
/// house). Read every frame, as a watcher's Tab and a replay's view change.
fn viewer(state: &AppState) -> Viewer {
    let spectator = |sees_whole_map| Viewer {
        house: 0,
        spectator: true,
        sees_whole_map,
    };
    if crate::app::match_runtime::watching::active(state) {
        return spectator(crate::app::match_runtime::watching::whole_map(state));
    }
    let Some(runtime) = state.match_state.sim_runtime.runtime() else {
        return spectator(false);
    };
    let sim = &runtime.simulation;
    match sim.session().current_house() {
        Some(house) if sim.session().is_observer_house(house) => spectator(true),
        Some(house) => match sim.houses.index_of(house) {
            Some(index) => Viewer {
                house: crate::sim::ext_modules::house_ref(index),
                spectator: false,
                sees_whole_map: false,
            },
            None => spectator(false),
        },
        None => spectator(false),
    }
}

/// The cell under a screen point when the point is over the battlefield.
fn battlefield_cell(state: &AppState, x: f32, y: f32) -> Option<(u16, u16)> {
    let (left, top, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    let inside = x >= left as f32
        && y >= top as f32
        && x < (left + width) as f32
        && y < (top + height) as f32;
    inside.then(|| crate::app::match_runtime::sim_tick::screen_point_to_world_cell(state, x, y))
}

/// What the camera and settings references read this frame.
fn local_inputs(state: &AppState) -> LocalInputs {
    let (left, top, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    let centre = (
        left as f32 + width as f32 / 2.0,
        top as f32 + height as f32 / 2.0,
    );
    LocalInputs {
        camera_cell: battlefield_cell(state, centre.0, centre.1),
    }
}

/// Builds the match's client modules once the world and its presentation
/// are installed (`loading::transitions::apply_map_load_result`): the mod's
/// `ui.wasm`, then the map's under `[VeraWasm] MapUI=`, the map's `[VeraUI]`
/// parts, then `client_init` for each module.
pub(crate) fn install(state: &mut AppState, sources: UiModuleSources) {
    state.match_state.ui_modules = None;
    let mut lines: Vec<(String, bool)> = sources
        .problems
        .iter()
        .map(|problem| (problem.clone(), true))
        .collect();
    let Some(map_ui) = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|runtime| runtime.simulation.ext_modules().map_ui())
    else {
        return;
    };
    if sources.is_empty() && lines.is_empty() {
        return;
    }
    let storage_root = storage_root(state);
    let mod_key = state
        .frontend
        .mods
        .active
        .as_ref()
        .map(|active| crate::app::mods::name_and_version(active.dir.path(), &active.folder).0)
        .unwrap_or_else(|| "RA2".to_owned());
    let mut ui = UiModules::new(map_ui);
    let mut load =
        |source: Source, bytes: &[u8], limits: Limits, key: &str| match ClientModule::load(
            bytes, limits,
        ) {
            Ok(module) => ui.slots.push(Slot {
                source,
                module,
                storage: Storage::open(&storage_root, key),
            }),
            Err(error) => {
                lines.push((format!("{} ui.wasm: {error}", source_label(source)), true));
            }
        };
    if let Some(bytes) = sources.mod_module.as_deref() {
        load(Source::Mod, bytes, Limits::MOD, &mod_key);
    }
    let map_allowed = map_ui != MapUi::No;
    if let Some(bytes) = sources.map_module.as_deref() {
        if map_allowed {
            load(Source::Map, bytes, Limits::MAP, &sources.map_storage_id);
        } else {
            log::info!(
                target: "ext_module",
                "the mod's [VeraWasm] MapUI=no: the map's ui.wasm is not loaded"
            );
        }
    }
    let map_layout = sources.map_layout.filter(|_| map_allowed);
    if let Some(layout) = map_layout.as_ref() {
        ui.tree.add_map_parts(&mut ui.gui, layout);
    }
    if !ui.slots.is_empty() {
        let viewer = viewer(state);
        let inputs = local_inputs(state);
        let runtime = state
            .match_state
            .sim_runtime
            .runtime()
            .expect("checked above");
        let view = ClientView {
            runtime,
            viewer,
            local: LocalFacts::default(),
        };
        let mut out = CallOutput::default();
        ui.call_each(&view, &inputs, &mut out, |module, host| module.init(host));
        lines.extend(out.lines);
        // `client_init` may only look names up and log (appendix 2.2):
        // nothing of it reaches the app.
        drop(out.effects);
    }
    ui.pending_lines = lines;
    if !ui.slots.is_empty() || map_layout.is_some() || !ui.pending_lines.is_empty() {
        state.match_state.ui_modules = Some(ui);
        rebind_game_keys(state);
    }
}

/// `wasm-storage/` in the user's VERA profile folder (RFC 10, "Storage").
fn storage_root(state: &AppState) -> PathBuf {
    state
        .platform
        .game_config
        .as_ref()
        .map(|config| config.paths.settings_dir().join("wasm-storage"))
        .unwrap_or_else(|| PathBuf::from("wasm-storage"))
}

/// Takes the simulation's client outbox at a frame's tail and keeps it for
/// the next render frame; during catch-up, or without client modules, it is
/// dropped (RFC 11: notifications are suspended while catching up).
pub(crate) fn collect_sim_outbox(state: &mut AppState) {
    let has_sim_modules = state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| !runtime.simulation.ext_modules().is_empty());
    if !has_sim_modules {
        return;
    }
    let catching_up = catching_up(state);
    let Some(outbox) = state.match_state.sim_runtime.take_client_outbox() else {
        return;
    };
    let Some(ui) = state.match_state.ui_modules.as_mut() else {
        return;
    };
    if catching_up {
        return;
    }
    ui.outbox.notifies.extend(outbox.notifies);
    ui.outbox.sim_events.extend(outbox.sim_events);
}

/// One render frame of the match's client modules (see the module docs,
/// "Frame order"). Nothing without modules or outside the battlefield.
pub(crate) fn render_frame(state: &mut AppState) {
    if state.match_state.ui_modules.is_none()
        || state.frontend.screen != crate::ui::game_screen::GameScreen::InGame
    {
        return;
    }
    let pending = state
        .match_state
        .ui_modules
        .as_mut()
        .map(|ui| std::mem::take(&mut ui.pending_lines))
        .unwrap_or_default();
    post_lines(state, pending);
    if catching_up(state) {
        if let Some(ui) = state.match_state.ui_modules.as_mut() {
            ui.started = false;
            ui.outbox = ClientOutbox::default();
            ui.last_render = None;
        }
        return;
    }
    let viewer = viewer(state);
    let inputs = local_inputs(state);
    let selected = crate::app::input::dispatch::selected_in_order(state);
    let now = Instant::now();
    let mut out = CallOutput::default();
    {
        let match_state = &mut state.match_state;
        let (Some(ui), Some(runtime)) = (
            match_state.ui_modules.as_mut(),
            match_state.sim_runtime.runtime(),
        ) else {
            return;
        };
        if ui.slots.is_empty() {
            // Static map parts only: the tree's events have no reader.
            ui.tree.take_events(&mut Vec::new());
            return;
        }
        let dt = ui
            .last_render
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(1.0));
        ui.last_render = Some(now);
        let view = ClientView {
            runtime,
            viewer,
            local: LocalFacts {
                selected: &selected,
                control_groups: &[],
            },
        };
        ui.run_frame(&view, &inputs, &selected, dt, &mut out);
    }
    post_lines(state, out.lines);
    apply_effects(state, out.effects);
}

/// The modules' log lines and faults in the in-game console, as the
/// simulation modules' (`TriggerEffect::ExtModuleNotice`/`ExtModuleStopped`).
fn post_lines(state: &mut AppState, lines: Vec<(String, bool)>) {
    if lines.is_empty() {
        return;
    }
    let name = crate::i18n::tr("VERA:ModuleSender");
    for (line, fault) in lines {
        if fault {
            let text = crate::i18n::tr_fill("VERA:ModuleStopped", &[("reason", &line)]);
            crate::app::input::messages::post_chat_line(state, &name, &text, [1.0, 0.4, 0.4]);
        } else {
            crate::app::input::messages::post_chat_line(state, &name, &line, [1.0, 0.8, 0.4]);
        }
    }
}

/// Queued commands go through the local player's ordinary command path
/// (`input::commands::try_schedule_command`: replays and locks refuse them
/// there); the other effects are [`local::apply`]'s.
fn apply_effects(state: &mut AppState, mut effects: LocalEffects) {
    let commands = std::mem::take(&mut effects.commands);
    if !commands.is_empty()
        && let Some(owner) = crate::app::input::commands::preferred_local_owner(state)
    {
        for command in commands {
            let _ = crate::app::input::commands::try_schedule_command(state, &owner, command);
        }
    }
    local::apply(effects, state);
}

/// Pointer motion over the match, before the game sees it.
pub(crate) fn pointer_moved(state: &mut AppState) {
    if state.match_state.ui_modules.is_none() {
        return;
    }
    let (x, y) = (
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let cell = battlefield_cell(state, x, y);
    if let Some(ui) = state.match_state.ui_modules.as_mut() {
        ui.tree.pointer_move(&ui.gui, x as i32, y as i32, cell);
    }
}

/// A left or right button edge over the match, before the game sees it.
/// True: the tree took it (a clickable node, a `Dismiss`, a captured map
/// click), and the game must not see it; a consumed press's release is
/// taken too.
pub(crate) fn pointer_button(
    state: &mut AppState,
    button: winit::event::MouseButton,
    pressed: bool,
) -> bool {
    let index: u8 = match button {
        winit::event::MouseButton::Left => 0,
        winit::event::MouseButton::Right => 1,
        _ => return false,
    };
    if state.match_state.ui_modules.is_none() {
        return false;
    }
    let (x, y) = (
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let cell = battlefield_cell(state, x, y);
    let Some(ui) = state.match_state.ui_modules.as_mut() else {
        return false;
    };
    let bit = 1 << index;
    if !pressed {
        let consumed = ui.consumed_buttons & bit != 0;
        ui.consumed_buttons &= !bit;
        return consumed;
    }
    let consumed = ui
        .tree
        .pointer_down(&mut ui.gui, x as i32, y as i32, index, cell);
    if consumed {
        ui.consumed_buttons |= bit;
    }
    consumed
}

/// Esc in the match, before the game's own: true when it dismissed a
/// module's node (`Dismiss`).
pub(crate) fn key_escape(state: &mut AppState) -> bool {
    state
        .match_state
        .ui_modules
        .as_mut()
        .is_some_and(|ui| ui.tree.key_escape(&ui.gui))
}

/// A key press the game did not use: the modules' hotkeys (game hotkeys
/// win, RFC 10). `key` is the key the bindings resolve (`hotkeys::
/// binding_logical_key`) and `modifiers` theirs, so a module hotkey reads the
/// key word a game binding would (`virtual key | Shift 0x100 | Ctrl 0x200 |
/// Alt 0x400`). True when a module hotkey fired.
pub(crate) fn key_unused(
    state: &mut AppState,
    key: &winit::keyboard::Key,
    location: winit::keyboard::KeyLocation,
    modifiers: winit::keyboard::ModifiersState,
) -> bool {
    let Some(ui) = state.match_state.ui_modules.as_mut() else {
        return false;
    };
    let Some(virtual_key) = crate::app::input::hotkeys::logical_virtual_key(key, location) else {
        return false;
    };
    let encoded = virtual_key | crate::app::input::hotkeys::modifier_bits(modifiers);
    local::key_text(encoded).is_some_and(|text| ui.local.key_down(&text))
}

/// Tells the modules' hotkeys which keys the game's bindings hold: at the
/// match's start and whenever the player rebinds (game hotkeys win, RFC 10).
pub(crate) fn rebind_game_keys(state: &mut AppState) {
    let crate::app::match_runtime::state::MatchState {
        ui_modules, input, ..
    } = &mut state.match_state;
    if let Some(ui) = ui_modules.as_mut() {
        ui.local.set_game_keys(input.hotkey_bindings.bound_keys());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_layout_sections_are_veraui_and_its_parts() {
        assert!(is_map_layout_section("VeraUI"));
        assert!(is_map_layout_section("VeraUI.Score"));
        assert!(is_map_layout_section("veraui.score"));
        assert!(!is_map_layout_section("VeraWasmUIPack"));
        assert!(!is_map_layout_section("VeraUIx"));
        assert!(!is_map_layout_section("Vera"));
    }

    #[test]
    fn a_map_layout_and_storage_key_are_read_from_the_map() {
        let map = IniFile::from_str(
            "[VeraUI]\nParts=Score\n[VeraUI.Score]\nType=Text\n[Basic]\nName=X\n[VeraWasm]\nStorageId=my-map\n",
        );
        let layout: Vec<_> = map
            .section_names()
            .into_iter()
            .filter(|name| is_map_layout_section(name))
            .collect();
        assert_eq!(layout.len(), 2);
        assert_eq!(
            map.section_or_empty("VeraWasm").read_name("StorageId", 64),
            Some("my-map")
        );
        let bundled = crate::map::source::LoadedMapSource::Bundled {
            logical_name: "Isle.map".to_owned(),
            payload_len: 8,
            source_sha256: String::new(),
        };
        assert_eq!(map_file_name(&bundled), "Isle.map");
    }
}
