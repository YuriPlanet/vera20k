//! The writable per-app directory a platform entry hands the engine before
//! start-up.
//!
//! Android has no `HOME` and starts every process in `/`, which is read-only.
//! Its cdylib therefore passes `AndroidApp::internal_data_path` here before
//! anything opens a file. Desktop launches leave it unset and keep resolving
//! the settings folder and the log directory the way they always have, so an
//! unset value is not an error: it means "ask the platform the old way".
//!
//! The working directory is a separate matter. [`crate::app::app_bundle`] moves
//! into the data folder on macOS bundles and Linux AppImages and does the same
//! on Android, so relative paths in the engine keep meaning the same thing on
//! every platform.
//!
//! ## Dependency rules
//! - Part of util/: no dependencies on game modules.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Record the platform's data directory.
///
/// The first call wins: an Android activity that is recreated calls this again
/// with the same path, and resetting it would invalidate paths already handed
/// out.
pub fn set(path: PathBuf) -> bool {
    DATA_DIR.set(path).is_ok()
}

/// The platform-provided data directory, when this launch has one.
pub fn get() -> Option<&'static Path> {
    DATA_DIR.get().map(PathBuf::as_path)
}
