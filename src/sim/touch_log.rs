//! Touch logs: which stored objects were handed out mutably since a reader
//! last looked.
//!
//! A store that routes every `&mut` to its objects through itself notes each
//! hand-out, so a product derived from those objects (the movement pass's
//! blocker plane, the kept Ground display sort keys, the pointer-expiry
//! listener index) re-derives only what was noted instead of walking the
//! world. A log records the hand-out, not a
//! change, so it over-reports, which is harmless. Transient: never saved,
//! never hashed, and no simulation result reads it.

/// What a reader takes from a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Touched {
    /// Anything may have changed: rebuild from the store.
    All,
    /// Only these ids (unsorted, repeats possible).
    Ids(Vec<u64>),
}

/// One reader's log.
#[derive(Debug)]
struct TouchLog {
    ids: Vec<u64>,
    /// Set when ids are not enough: a fresh, cloned or restored store, an
    /// all-object mutable walk, or an overflowed log.
    all: bool,
}

impl TouchLog {
    /// A log that reports everything, for a store nothing has been derived
    /// from yet.
    fn everything() -> Self {
        Self {
            ids: Vec::new(),
            all: true,
        }
    }

    /// Note that `id` was handed out mutably; `stored` is the store's size.
    fn note(&mut self, id: u64, stored: usize) {
        if self.all || self.ids.last() == Some(&id) {
            return;
        }
        // Nobody is taking the log (nothing moving for a long stretch): stop
        // growing and ask the next reader to rebuild instead.
        if self.ids.len() > stored.saturating_mul(4) + 4096 {
            self.note_all();
            return;
        }
        self.ids.push(id);
    }

    /// Note that every object may have changed.
    fn note_all(&mut self) {
        self.ids = Vec::new();
        self.all = true;
    }

    /// Take the log, leaving it empty.
    fn take(&mut self) -> Touched {
        let log = std::mem::replace(
            self,
            Self {
                ids: Vec::new(),
                all: false,
            },
        );
        if log.all {
            Touched::All
        } else {
            Touched::Ids(log.ids)
        }
    }
}

/// A store's touch logs, one per reader, noted together. The store holds it
/// as a field it can derive through: a default, cloned or deserialized copy
/// reports everything (whatever was derived from the original says nothing
/// certain about the copy's future), and two copies compare equal (a log is
/// no simulation state).
#[derive(Debug)]
pub(crate) struct TouchLogs<const N: usize> {
    logs: [TouchLog; N],
    /// Every hand-out so far, for test checks that a pass handed nothing out.
    #[cfg(any(test, feature = "test-support"))]
    hand_outs: u64,
}

impl<const N: usize> TouchLogs<N> {
    /// Logs that report everything, for a store nothing has been derived
    /// from yet.
    pub(crate) fn everything() -> Self {
        Self {
            logs: std::array::from_fn(|_| TouchLog::everything()),
            #[cfg(any(test, feature = "test-support"))]
            hand_outs: 0,
        }
    }

    /// Note that `id` was handed out mutably; `stored` is the store's size.
    pub(crate) fn note(&mut self, id: u64, stored: usize) {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.hand_outs += 1;
        }
        for log in &mut self.logs {
            log.note(id, stored);
        }
    }

    /// Note that every object may have changed.
    pub(crate) fn note_all(&mut self) {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.hand_outs += 1;
        }
        for log in &mut self.logs {
            log.note_all();
        }
    }

    /// Take reader `reader`'s log, leaving it empty.
    pub(crate) fn take(&mut self, reader: usize) -> Touched {
        self.logs[reader].take()
    }

    /// How many mutable hand-outs the store has made (test builds only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn hand_outs(&self) -> u64 {
        self.hand_outs
    }
}

impl<const N: usize> Default for TouchLogs<N> {
    fn default() -> Self {
        Self::everything()
    }
}

impl<const N: usize> Clone for TouchLogs<N> {
    fn clone(&self) -> Self {
        Self::everything()
    }
}

impl<const N: usize> PartialEq for TouchLogs<N> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<const N: usize> Eq for TouchLogs<N> {}

pub use crate::util::debug_checks::live_read_check_enabled;
