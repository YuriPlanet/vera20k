//! The part of the screen the game must keep clear on Android: the rounded
//! corners and the display cutout that cover the tactical view's top-left.
//!
//! The battle view fills the window, so its messages start a few units from
//! the physical edge; a phone's punch-hole (120 px on the left in landscape) and
//! the corner arc would hide them. The activity measures both from the window
//! insets and reports them in screen pixels; `app::input::messages` moves the
//! message list in by that much, scaled to the render frame.
//!
//! The hooks stay uninstalled everywhere else, so [`get`] is zero and the
//! desktop and browser keep their native layout.

use std::sync::OnceLock;

/// How far the screen's top-left corner hides the game, in screen pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insets {
    pub left: i32,
    pub top: i32,
}

/// What the Android crate reports for this module.
pub struct Hooks {
    /// The cutout and corner radius at the window's top-left, in screen pixels.
    pub insets: fn() -> Insets,
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

/// Install the activity hook. The Android crate calls this once, before the
/// event loop.
pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

/// The screen's top-left insets, zero when no hook is installed.
pub fn get() -> Insets {
    HOOKS.get().map(|hooks| (hooks.insets)()).unwrap_or_default()
}
