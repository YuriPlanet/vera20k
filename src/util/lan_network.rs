//! The Wi-Fi network a LAN visit runs on.
//!
//! Android delivers a datagram on the interface of the process's default
//! network. With Wi-Fi and cellular both up — and especially when the Wi-Fi
//! network has no internet — a limited broadcast can leave by the mobile
//! interface, so a LAN visit finds nothing although every player is on the same
//! Wi-Fi. The activity binds this process to the Wi-Fi network while the lobby
//! is open (`ConnectivityManager.bindProcessToNetwork`) and reports every
//! network change; the visit then forgets what it learned and announces again
//! on the network that is current.
//!
//! Only the Android crate can reach the activity, so it installs the hooks
//! before the event loop. Everywhere else the module is not compiled.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// What the activity does for [`acquire`] and [`release`].
pub struct Hooks {
    /// Bind this process to the Wi-Fi network and start watching it.
    pub acquire: fn(),
    /// Stop watching and let the system pick the default network again.
    pub release: fn(),
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();
static CHANGED: AtomicBool = AtomicBool::new(false);

/// Install the activity hooks. The Android crate calls this once, before the
/// event loop.
pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

/// The LAN lobby opened: run on the Wi-Fi network and watch it.
pub fn acquire() {
    if let Some(hooks) = HOOKS.get() {
        (hooks.acquire)();
    }
}

/// The LAN lobby closed: stop watching and drop the binding.
pub fn release() {
    if let Some(hooks) = HOOKS.get() {
        (hooks.release)();
    }
}

/// The activity saw a Wi-Fi network appear, leave or change.
pub fn note_changed() {
    CHANGED.store(true, Ordering::Release);
}

/// Whether a network change is waiting to be handled, clearing the request.
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::AcqRel)
}
