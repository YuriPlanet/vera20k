//! Desktop Lua VM owner behind the opt-in `lua` cargo feature.
//!
//! One VM per process, created with the app window and kept for the whole run,
//! matching WolfCTOS: scenario changes reuse the state and re-fire the
//! scenario-start hook instead of rebuilding the host.
//!
//! Startup must never be fatal. A VM that fails to come up leaves the host
//! offline (logged once); the game keeps running and no frame or scenario hook
//! reaches the simulation.

/// Lua state owned by [`AppState`](crate::app::AppState).
pub(crate) struct LuaHost {
    /// `None` when the VM could not start. Every hook checks this first.
    lua: Option<mlua::Lua>,
}

impl LuaHost {
    /// Start the VM and its compatibility shims, or log why it stayed offline.
    pub(crate) fn new() -> Self {
        match Self::start() {
            Ok(lua) => Self { lua: Some(lua) },
            Err(error) => {
                log::error!("LuaAPI offline: {error}");
                Self { lua: None }
            }
        }
    }

    /// The live VM, or `None` while the host is offline. The scenario and
    /// frame hooks consume this once they land.
    #[allow(dead_code)]
    pub(crate) fn lua(&self) -> Option<&mlua::Lua> {
        self.lua.as_ref()
    }

    fn start() -> mlua::Result<mlua::Lua> {
        let lua = mlua::Lua::new();
        install_compat_shims(&lua)?;
        let version: String = lua.load("return _VERSION").eval()?;
        log::info!("LuaAPI online: {version} (WolfCTOS API 0.2.0 subset; mod loading follows)");
        Ok(lua)
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
