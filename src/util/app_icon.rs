//! The icon the VERA Icon box selects, on platforms whose icon is not a window
//! attribute.
//!
//! The desktop changes its window, taskbar and Dock icon live
//! (`crate::app::window_icon`). Android's launcher icon is a resource compiled
//! into the package, so the choice there is carried by two `activity-alias`
//! entries and flipped through `PackageManager`. Only the Android crate can
//! reach the activity, so it installs [`install`]'s hook before the event loop;
//! everywhere else [`apply`] does nothing.

use std::sync::OnceLock;

static HOOK: OnceLock<fn(bool)> = OnceLock::new();

/// Install the platform hook. The Android crate calls this once, before the
/// event loop, with a function that calls `VeraActivity.applyAppIcon`.
pub fn install(hook: fn(bool)) {
    let _ = HOOK.set(hook);
}

/// Show the icon the VERA Icon box selected.
///
/// Safe to call on every start-up and on every toggle: without an installed
/// hook it does nothing.
pub fn apply(vera_icon: bool) {
    if let Some(hook) = HOOK.get() {
        hook(vera_icon);
    }
}
