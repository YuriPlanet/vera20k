//! The [`LuaHost`] of builds that carry no Lua VM.
//!
//! Builds without the `lua` cargo feature, and every wasm build, keep this
//! zero-sized placeholder so the app owns a host unconditionally. It has no
//! state and never touches the simulation, so those builds behave exactly as
//! they did before the Lua layer existed.

/// Inert stand-in for the desktop Lua host.
pub(crate) struct LuaHost;

impl LuaHost {
    pub(crate) const fn new() -> Self {
        Self
    }
}
