//! Asking the system for a folder, when the platform has a picker.
//!
//! The asset-import dialog needs a folder the player chooses; only the platform
//! can show that UI. A build with a picker installs a hook at start-up (Android
//! reaches `ACTION_OPEN_DOCUMENT_TREE` through JNI), and a build without one
//! leaves it unset, where the request reports `false` and the dialog says so.
//!
//! The result is asynchronous: the hook reports whether the picker opened, and
//! the platform delivers the choice separately to
//! [`crate::util::folder_picker::deliver`]'s consumer. Keeping the request and
//! its answer apart is what lets the engine stay single-threaded and the UI stay
//! in the event loop.
//!
//! ## Dependency rules
//! - Part of util/: no dependencies on game modules.

use std::sync::{Mutex, OnceLock};

struct Hooks {
    request: fn() -> bool,
    cancel: fn() -> bool,
}

static HOOK: OnceLock<Hooks> = OnceLock::new();
/// Outer `Some` means an answer has arrived; the inner one is the choice
/// (`None` = the player cancelled).
static PICKED: Mutex<Option<Option<String>>> = Mutex::new(None);

/// Install the platform hook. The first install wins.
pub fn install(request: fn() -> bool, cancel: fn() -> bool) -> bool {
    HOOK.set(Hooks { request, cancel }).is_ok()
}

/// Ask the platform to show its folder picker.
///
/// `true` means the picker opened and its answer will arrive later; `false`
/// means this build has no picker (or the platform refused to start one).
pub fn request() -> bool {
    HOOK.get().is_some_and(|hooks| (hooks.request)())
}

/// Ask the active copy to stop. A true result still needs its completion event:
/// the worker retains ownership until its partial files have been removed.
pub fn cancel_import() -> bool {
    HOOK.get().is_some_and(|hooks| (hooks.cancel)())
}

/// Record the platform's answer: a tree identifier, or `None` when the player
/// cancelled.
pub fn deliver(picked: Option<String>) {
    if let Ok(mut slot) = PICKED.lock() {
        *slot = Some(picked);
    }
}

/// Take the platform's answer if one has arrived since the last call.
///
/// The outer `Option` says whether an answer arrived; the inner one is the
/// player's choice (`None` = cancelled).
pub fn take() -> Option<Option<String>> {
    PICKED.lock().ok().and_then(|mut slot| slot.take())
}
