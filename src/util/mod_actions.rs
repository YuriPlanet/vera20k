//! The Mods page's folder actions where the platform has to do them.
//!
//! Android keeps the player's folders inside the app's private storage, so the
//! page cannot hand a path to a file manager: the activity exports a folder
//! through the system picker, copies the ZIPs and folders Add Mod picks into
//! its own `tmp/`, and pins a shortcut for a mod on the home screen. Deleting
//! a folder needs no help: the app owns it. Hooks stay uninstalled elsewhere,
//! and every call answers false or nothing there.

use std::sync::OnceLock;

/// What the Android crate reports for this module. Texts are the toasts the
/// activity shows, in the player's language.
pub struct Hooks {
    /// Hand the named `Mods` folder to the system file manager to copy out;
    /// the toast says `done` or `failed`.
    pub export: fn(folder: &str, done: &str, failed: &str) -> bool,
    /// Pack the named `Mods` folder into a ZIP the player picks; `progress`
    /// labels the wait, and the toast says `done` or `failed`.
    pub export_zip: fn(folder: &str, done: &str, failed: &str, progress: &str) -> bool,
    /// Add `folder` as a home-screen shortcut for `id`.
    pub pin: fn(folder: &str, id: &str) -> bool,
    /// Let the player pick a mod ZIP in the system file manager; the toast
    /// says `failed` when the copy fails.
    pub pick_zip: fn(failed: &str) -> bool,
    /// Let the player pick a mod folder in the system file manager.
    pub pick_folder: fn(failed: &str) -> bool,
    /// Forget `folder`'s shortcuts, keyed by `id` and by its old name.
    pub forget: fn(folder: &str, id: &str) -> bool,
    /// The part the picker copied in, once; the caller takes it.
    pub take_picked: fn() -> Option<String>,
    /// Delete every copy the pickers made.
    pub clear_picked: fn(),
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

/// Install the activity hooks; the Android crate calls this before the loop.
pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

/// Whether the platform's actions can run at all (an installed hook).
pub fn available() -> bool {
    HOOKS.get().is_some()
}

/// Ask the platform to export `folder`; false when it cannot.
pub fn export(folder: &str, done: &str, failed: &str) -> bool {
    HOOKS
        .get()
        .is_some_and(|hooks| (hooks.export)(folder, done, failed))
}

/// Ask the platform to pack `folder` into a ZIP; false when it cannot.
pub fn export_zip(folder: &str, done: &str, failed: &str, progress: &str) -> bool {
    HOOKS
        .get()
        .is_some_and(|hooks| (hooks.export_zip)(folder, done, failed, progress))
}

/// Ask for a mod ZIP; false when the platform cannot show a picker.
pub fn pick_zip(failed: &str) -> bool {
    HOOKS.get().is_some_and(|hooks| (hooks.pick_zip)(failed))
}

/// Ask for a mod folder; false when the platform cannot show a picker.
pub fn pick_folder(failed: &str) -> bool {
    HOOKS.get().is_some_and(|hooks| (hooks.pick_folder)(failed))
}

/// Take the part a picker copied in, if one arrived.
pub fn take_picked() -> Option<std::path::PathBuf> {
    HOOKS
        .get()
        .and_then(|hooks| (hooks.take_picked)())
        .map(std::path::PathBuf::from)
}

/// Delete the copies the pickers made, once Add Mod is done with them.
pub fn clear_picked() {
    if let Some(hooks) = HOOKS.get() {
        (hooks.clear_picked)();
    }
}

/// Ask the platform to drop `folder`'s shortcut; false when it cannot.
pub fn forget(folder: &str, id: &str) -> bool {
    HOOKS.get().is_some_and(|hooks| (hooks.forget)(folder, id))
}

/// Ask the platform to pin `folder`; false when it cannot.
pub fn pin(folder: &str, id: &str) -> bool {
    HOOKS.get().is_some_and(|hooks| (hooks.pin)(folder, id))
}
