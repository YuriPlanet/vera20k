//! Mod discovery and loading, in WolfCTOS's on-disk layout.
//!
//! A mod is a folder with a `main.lua` that returns a table (`mods/<id>/main.lua`);
//! the loader reads enabled ids from `scripts/active_mods.txt` (one per line,
//! `#` comments) and requires `mods.<id>.main` in that order, exactly as
//! WolfCTOS's `scripts/init.lua` does. Libraries under `scripts/` resolve
//! through the same `package.path`, so `require("framework.util")` works.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::LuaPaths;

/// A mod whose `main.lua` loaded. `Update` is optional: library-style mods may
/// only export helpers for other mods.
pub(super) struct LoadedMod {
    pub(super) id: String,
    pub(super) update: Option<mlua::Function>,
    /// Consecutive `Update` failures; one success clears the count.
    pub(super) consecutive_errors: u32,
    /// The last error text, so the log gets one line per distinct failure
    /// instead of one per frame.
    pub(super) last_error: Option<String>,
    pub(super) disabled: bool,
}

/// Consecutive failures after which a mod is switched off. WolfCTOS itself only
/// reports errors; VERA also stops paying for a mod that fails every frame.
pub(super) const MAX_CONSECUTIVE_ERRORS: u32 = 10;

/// Search roots for `require`, most specific first: the retail install (its
/// root and `scripts/`), then VERA's profile for user-installed overlays.
pub(super) fn search_roots(paths: &LuaPaths) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(game_root) = &paths.game_root {
        roots.push(game_root.clone());
        roots.push(game_root.join("scripts"));
    }
    if let Some(settings_root) = &paths.settings_root {
        roots.push(settings_root.clone());
        roots.push(settings_root.join("scripts"));
    }
    roots
}

/// Prepend the WolfCTOS search patterns to `package.path`.
pub(super) fn install_search_paths(lua: &mlua::Lua, roots: &[PathBuf]) -> mlua::Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    let package: mlua::Table = lua.globals().get("package")?;
    let current: String = package.get("path")?;
    let mut patterns = Vec::with_capacity(roots.len() * 2 + 1);
    for root in roots {
        let root = root.to_string_lossy().replace('\\', "/");
        patterns.push(format!("{root}/?.lua"));
        patterns.push(format!("{root}/?/init.lua"));
    }
    patterns.push(current);
    package.set("path", patterns.join(";"))
}

/// Enabled mod ids, in load order.
pub(super) fn active_mod_ids(paths: &LuaPaths) -> Vec<String> {
    let Some(path) = active_mods_file(paths) else {
        return Vec::new();
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            log::warn!("LuaAPI: cannot read {}: {error}", path.display());
            return Vec::new();
        }
    };
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for line in text.lines() {
        let id = line.trim();
        if id.is_empty() || id.starts_with('#') {
            continue;
        }
        // A repeated id must not dispatch the same module twice per frame.
        if seen.insert(id.to_string()) {
            ids.push(id.to_string());
        }
    }
    ids
}

/// The locations WolfCTOS's `scripts/init.lua` probes, in its order, plus
/// VERA's profile slots: the retail `mods/active_mods.txt` first (the deployed
/// loader reads it first), then `scripts/`, then the VERA profile, then the
/// retail root.
fn active_mods_file(paths: &LuaPaths) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(game_root) = &paths.game_root {
        candidates.push(game_root.join("mods/active_mods.txt"));
        candidates.push(game_root.join("scripts/active_mods.txt"));
    }
    if let Some(settings_root) = &paths.settings_root {
        candidates.push(settings_root.join("active_mods.txt"));
        candidates.push(settings_root.join("mods/active_mods.txt"));
    }
    if let Some(game_root) = &paths.game_root {
        candidates.push(game_root.join("active_mods.txt"));
    }
    candidates
        .into_iter()
        .find(|path| Path::new(path).is_file())
}

/// Require every active mod in order. A mod that fails to load is logged and
/// skipped; it cannot stop the mods after it.
pub(super) fn load_mods(lua: &mlua::Lua, ids: &[String]) -> Vec<LoadedMod> {
    let Ok(require) = lua.globals().get::<mlua::Function>("require") else {
        log::error!("LuaAPI: the runtime has no `require`; mod loading is unavailable");
        return Vec::new();
    };
    let mut loaded = Vec::new();
    for id in ids {
        match require.call::<mlua::Value>(format!("mods.{id}.main")) {
            Ok(mlua::Value::Table(table)) => {
                let update = table
                    .get::<Option<mlua::Function>>("Update")
                    .unwrap_or(None);
                if update.is_some() {
                    log::info!("Lua mod active: '{id}'");
                } else {
                    log::info!("Lua mod loaded without Update: '{id}'");
                }
                loaded.push(LoadedMod {
                    id: id.clone(),
                    update,
                    consecutive_errors: 0,
                    last_error: None,
                    disabled: false,
                });
            }
            Ok(other) => log::warn!(
                "Lua mod '{id}' returned {} instead of a table; skipped",
                other.type_name()
            ),
            Err(error) => log::error!("Lua mod '{id}' failed to load: {error}"),
        }
    }
    loaded
}
