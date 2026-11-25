//! The Mods page's folder actions where the platform has to do them.
//!
//! Android keeps the player's folders inside the app's private storage, so the
//! page cannot hand a path to a file manager: the activity exports a folder
//! through the system picker, imports one the same way, and pins a short-cut
//! for one on the home screen. Deleting a folder needs no help — the app owns
//! it. Hooks stay uninstalled elsewhere, and every call answers false there.

use std::sync::OnceLock;

/// What the Android crate reports for this module.
pub struct Hooks {
    /// Hand the named `Mods` folder to the system file manager to copy out.
    pub export: fn(folder: &str) -> bool,
    /// Let the player pick a folder in the system file manager to copy in.
    pub import: fn() -> bool,
    /// Add the named folder as a home-screen shortcut.
    pub pin: fn(folder: &str) -> bool,
    /// Let the player pick a mod ZIP in the system file manager.
    pub pick_zip: fn() -> bool,
    /// Let the player pick a mod folder in the system file manager.
    pub pick_folder: fn() -> bool,
    /// The part the picker copied in, once; the caller takes it.
    pub take_picked: fn() -> Option<String>,
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

/// Install the activity hooks; the Android crate calls this before the loop.
pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

/// Whether a platform action can run at all (an installed hook).
pub fn available() -> bool {
    HOOKS.get().is_some()
}

/// Ask the platform to export `folder`; false when it cannot.
pub fn export(folder: &str) -> bool {
    HOOKS.get().is_some_and(|hooks| (hooks.export)(folder))
}

/// Ask the platform for a folder to import; false when it cannot.
pub fn import() -> bool {
    HOOKS.get().is_some_and(|hooks| (hooks.import)())
}

/// Whether the platform can pick a mod ZIP or folder at all.
pub fn picker_available() -> bool {
    HOOKS.get().is_some()
}

/// Ask for a mod ZIP; false when the platform cannot show a picker.
pub fn pick_zip() -> bool {
    HOOKS.get().is_some_and(|hooks| (hooks.pick_zip)())
}

/// Ask for a mod folder; false when the platform cannot show a picker.
pub fn pick_folder() -> bool {
    HOOKS.get().is_some_and(|hooks| (hooks.pick_folder)())
}

/// Take the part a picker copied in, if one arrived.
pub fn take_picked() -> Option<std::path::PathBuf> {
    HOOKS
        .get()
        .and_then(|hooks| (hooks.take_picked)())
        .map(std::path::PathBuf::from)
}

/// Ask the platform to pin `folder`; false when it cannot.
pub fn pin(folder: &str) -> bool {
    HOOKS.get().is_some_and(|hooks| (hooks.pin)(folder))
}
