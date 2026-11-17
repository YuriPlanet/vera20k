//! Progress and outcome of an asset import, from the copying thread to the UI.
//!
//! The copy runs on the platform's own thread (only it can read a Storage Access
//! Framework tree), while the dialog lives in the event loop. This is the one
//! hand-off between them: the platform posts events, the card drains them on the
//! frame it draws.
//!
//! ## Dependency rules
//! - Part of util/: no dependencies on game modules.

use std::collections::VecDeque;
use std::sync::Mutex;

/// One thing the import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportEvent {
    /// The copy started; its total size is not known up front for a SAF tree.
    Started,
    /// Bytes copied so far.
    Progress(u64),
    /// Every file was copied and the config was written; the path is the copy
    /// the next launch will read.
    Done(String),
    /// The copy stopped, with what to tell the player.
    Failed(String),
    /// Cancellation finished and the temporary copy was removed.
    Cancelled,
}

static EVENTS: Mutex<VecDeque<ImportEvent>> = Mutex::new(VecDeque::new());

/// Post an event from the copying thread.
pub fn note(event: ImportEvent) {
    if let Ok(mut queue) = EVENTS.lock() {
        queue.push_back(event);
    }
}

/// Take every event posted since the last call.
pub fn drain() -> Vec<ImportEvent> {
    EVENTS
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default()
}
