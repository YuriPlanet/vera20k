//! Process-wide record that this launch asked for original Red Alert 2 mode.
//!
//! `-ra2` is parsed once by the retail switch table, which is pure argv code
//! and must not own filesystem behavior. The parsed choice is stored here so
//! every [`crate::util::config::GameConfig`] load — the app's own and the many
//! headless tools' — resolves the same non-md INI roots without threading a
//! parameter through each call site.
//!
//! The default is off: a build with no `-ra2` keeps its YR roots. A build
//! without the `ra2` feature never sets this (see `entry::run_with_args`), so
//! the flag is inert there even if a stale `-ra2` reaches the switch table.

use std::sync::atomic::{AtomicBool, Ordering};

static RA2_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Record the launch's `-ra2` choice. Later calls replace the earlier value so
/// headless callers that parse several argument sets in one process stay
/// independent.
pub fn set(requested: bool) {
    RA2_REQUESTED.store(requested, Ordering::Relaxed);
}

/// Whether this process launched with `-ra2`.
///
/// Under `cfg(test)`, a thread-local override takes precedence when set. This
/// lets a unit test exercise the RA2 branch without flipping the process-wide
/// flag (which races with unrelated tests running in parallel on other
/// threads). Production code never sets the override.
pub fn enabled() -> bool {
    #[cfg(test)]
    if let Some(value) = TEST_RA2_OVERRIDE.with(std::cell::Cell::get) {
        return value;
    }
    RA2_REQUESTED.load(Ordering::Relaxed)
}

/// Test-only: make [`enabled`] report `value` on the current thread until
/// [`clear_override`]. Restores nothing on drop — callers clear explicitly.
#[cfg(test)]
pub(crate) fn set_override(value: bool) {
    TEST_RA2_OVERRIDE.set(Some(value));
}

#[cfg(test)]
pub(crate) fn clear_override() {
    TEST_RA2_OVERRIDE.set(None);
}

#[cfg(test)]
thread_local! {
    static TEST_RA2_OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Parse the process arguments for `-ra2` and record the choice. Called once,
/// before any config load. The switch is only honored by an `ra2`-featured
/// build; without it the argument is consumed as a no-op so an older binary
/// still starts instead of failing on an unknown token.
pub fn configure(args: &[&std::ffi::OsStr]) {
    let requested = args
        .iter()
        .any(|arg| arg.to_string_lossy().eq_ignore_ascii_case("-ra2"));
    set(requested && crate::rules::RA2_ENABLED);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ra2_launch_flag_defaults_off_and_tracks_the_last_set() {
        // Other tests in this process may have set it; make the contract about
        // the value we write, not about a pristine process.
        set(true);
        assert!(enabled());
        set(false);
        assert!(!enabled());
    }
}
