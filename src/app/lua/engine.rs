//! Desktop Lua VM owner behind the opt-in `lua` cargo feature.
//!
//! One VM per process, created with the app window and kept for the whole run,
//! matching WolfCTOS: scenario changes reuse the state and re-fire the
//! scenario-start hook instead of rebuilding the host.
//!
//! Dispatch mirrors WolfCTOS's `scripts/init.lua`: each loaded mod's
//! `Update(frame)` runs once per committed logical frame, inside `pcall`
//! semantics (the Rust equivalent: an error is logged and contained, never
//! propagated into the simulation). A failing mod is disabled after a run of
//! consecutive errors so a broken script cannot cost every frame forever.
//!
//! Startup must never be fatal. A VM that fails to come up leaves the host
//! offline (logged once); the game keeps running and no frame or scenario hook
//! reaches the simulation.

use std::cell::RefCell;
use std::rc::Rc;

use super::LuaPaths;
use super::loader::{self, LoadedMod, MAX_CONSECUTIVE_ERRORS};
use super::api;
use super::userdata::ScriptContext;

/// `Engine.PrintMessage` lines buffered between drains. A mod that prints every
/// frame cannot grow the queue without bound; the HUD shows the first lines.
const MAX_QUEUED_MESSAGES: usize = 32;

/// Lua state owned by [`AppState`](crate::app::AppState).
pub(crate) struct LuaHost {
    /// `None` when the VM could not start. Every hook checks this first.
    lua: Option<mlua::Lua>,
    mods: Vec<LoadedMod>,
    /// `Engine.PrintMessage` lines waiting for the HUD.
    messages: Rc<RefCell<Vec<String>>>,
    /// This frame's read-only world snapshot, refreshed by the app after each
    /// committed frame. Shared with the API closures through an `Rc`, so a
    /// frame dispatch never clones the whole world.
    snapshot: Rc<RefCell<crate::app::lua::snapshot::FrameSnapshot>>,
    /// Commands queued by mods this frame, to be flushed after `on_frame`.
    queued_commands: Rc<RefCell<Vec<super::userdata::PendingCommand>>>,
}

impl LuaHost {
    /// Start the VM, its compatibility shims, and the active mods.
    pub(crate) fn new(paths: LuaPaths) -> Self {
        match Self::start(&paths) {
            Ok((lua, mods, messages)) => Self {
                lua: Some(lua),
                mods,
                messages,
                snapshot: Rc::new(RefCell::new(
                    crate::app::lua::snapshot::FrameSnapshot::default(),
                )),
                queued_commands: Rc::new(RefCell::new(Vec::new())),
            },
            Err(error) => {
                log::error!("LuaAPI offline: {error}");
                Self {
                    lua: None,
                    mods: Vec::new(),
                    messages: Rc::new(RefCell::new(Vec::new())),
                    snapshot: Rc::new(RefCell::new(
                        crate::app::lua::snapshot::FrameSnapshot::default(),
                    )),
                    queued_commands: Rc::new(RefCell::new(Vec::new())),
                }
            }
        }
    }

    /// Run every active mod's `Update(frame)`, isolating each mod's errors.
    /// The World/House closures read the snapshot the app refreshed for this
    /// committed frame; orders a mod queues land in the shared command buffer,
    /// which the app drains after the dispatch.
    pub(crate) fn on_frame(&mut self, frame: u64) {
        if self.lua.is_none() {
            return;
        }
        // Publish the frame context: the shared snapshot and the command
        // queue the userdata handles fill.
        api::set_current(ScriptContext {
            snapshot: Rc::clone(&self.snapshot),
            commands: Rc::clone(&self.queued_commands),
        });
        for loaded in &mut self.mods {
            if loaded.disabled {
                continue;
            }
            let Some(update) = loaded.update.clone() else {
                continue;
            };
            match update.call::<()>(frame) {
                Ok(()) => {
                    loaded.consecutive_errors = 0;
                    loaded.last_error = None;
                }
                Err(error) => record_mod_error(loaded, &error),
            }
        }
        // Unpublish: outside the dispatch every query returns an empty table.
        api::clear_current();
    }

    /// Fire the global `OnScenarioStart` hook, if a mod defined one.
    ///
    /// WolfCTOS looks the event up as a global rather than as a mod-table
    /// method, and the last definition wins; this host keeps that contract.
    pub(crate) fn on_scenario_start(&mut self) {
        let Some(lua) = self.lua.as_ref() else {
            return;
        };
        let hook: Option<mlua::Function> = match lua.globals().get("OnScenarioStart") {
            Ok(hook) => hook,
            Err(error) => {
                log::error!("LuaAPI: cannot read the OnScenarioStart hook: {error}");
                return;
            }
        };
        let Some(hook) = hook else {
            return;
        };
        if let Err(error) = hook.call::<()>(()) {
            log::error!("Lua OnScenarioStart failed: {error}");
        }
    }

    /// Take the HUD lines mods queued since the last call.
    pub(crate) fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut *self.messages.borrow_mut())
    }

    /// Refresh the World/House API snapshot from the current frame state.
    /// Called by the app after the simulation tick has committed.
    pub(crate) fn refresh_snapshot(&mut self, snapshot: crate::app::lua::snapshot::FrameSnapshot) {
        *self.snapshot.borrow_mut() = snapshot;
    }

    fn start(
        paths: &LuaPaths,
    ) -> mlua::Result<(mlua::Lua, Vec<LoadedMod>, Rc<RefCell<Vec<String>>>)> {
        let lua = mlua::Lua::new();
        install_compat_shims(&lua)?;
        install_print(&lua)?;
        let messages = Rc::new(RefCell::new(Vec::new()));
        install_engine(&lua, Rc::clone(&messages))?;
        let roots = loader::search_roots(paths);
        loader::install_search_paths(&lua, &roots)?;
        let ids = loader::active_mod_ids(paths);
        let mods = loader::load_mods(&lua, &ids);
        let version: String = lua.load("return _VERSION").eval()?;
        log::info!(
            "LuaAPI online: {version}; {} active mod(s)",
            mods.len()
        );
        // Install the World / House Lua namespaces. The per-call closures in
        // these tables read the current frame's snapshot, which is managed by
        // set_current / clear_current around on_frame().
        api::install(&lua)?;
        Ok((lua, mods, messages))
    }

    /// Test view of the loaded mods: id and whether the host disabled it.
    #[cfg(test)]
    pub(super) fn mods(&self) -> impl Iterator<Item = (&str, bool)> {
        self.mods
            .iter()
            .map(|loaded| (loaded.id.as_str(), loaded.disabled))
    }
}

fn record_mod_error(loaded: &mut LoadedMod, error: &mlua::Error) {
    let text = error.to_string();
    loaded.consecutive_errors += 1;
    if loaded.last_error.as_deref() != Some(text.as_str()) {
        log::error!("Lua mod '{}' Update error: {text}", loaded.id);
        loaded.last_error = Some(text);
    }
    if loaded.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
        loaded.disabled = true;
        log::error!(
            "Lua mod '{}' disabled after {} consecutive Update errors",
            loaded.id,
            loaded.consecutive_errors
        );
    }
}

/// Lua 5.1 names that shipped WolfCTOS mods call. Lua 5.4 removed or moved
/// them; the shims keep those mods loading unmodified.
fn install_compat_shims(lua: &mlua::Lua) -> mlua::Result<()> {
    lua.load(
        r#"
        -- `loadstring` and `unpack` were removed into `load` and `table.unpack`.
        loadstring = loadstring or load
        unpack = unpack or table.unpack
        "#,
    )
    .exec()
}

/// Route script `print` to the log, as the native DLL's console output does.
fn install_print(lua: &mlua::Lua) -> mlua::Result<()> {
    let tostring: mlua::Function = lua.globals().get("tostring")?;
    let print = lua.create_function(move |_, args: mlua::Variadic<mlua::Value>| {
        let mut line = String::new();
        for (index, value) in args.into_iter().enumerate() {
            if index > 0 {
                line.push('\t');
            }
            let text: String = tostring.call(value)?;
            line.push_str(&text);
        }
        log::info!(target: "lua", "{line}");
        Ok(())
    })?;
    lua.globals().set("print", print)
}

/// `Engine.PrintMessage`: queue a HUD line for the next committed frame.
fn install_engine(lua: &mlua::Lua, messages: Rc<RefCell<Vec<String>>>) -> mlua::Result<()> {
    let queue = Rc::clone(&messages);
    let print_message = lua.create_function(move |_, text: String| {
        let mut queue = queue.borrow_mut();
        if queue.len() < MAX_QUEUED_MESSAGES {
            queue.push(text);
        }
        Ok(())
    })?;
    let engine = lua.create_table()?;
    engine.set("PrintMessage", print_message)?;
    lua.globals().set("Engine", engine)
}
