//! Match diagnostics owner (F10): the Rust-only diagnostic replay log and its
//! retry-safe file lifecycle.
//!
//! Diagnostic state lives beside the runtime, never inside `Simulation` —
//! recording is app-owned so it cannot perturb deterministic state, and the
//! segment's lifetime is owned by explicit app transitions:
//! - lazily opened on the first recorded frame of a runtime (fresh or
//!   restored timelines alike);
//! - a failed load retains the active segment untouched (the log no longer
//!   rides the simulation slot, so replacing or restoring a sim cannot drop
//!   it);
//! - a successful in-scenario load flushes (closes) the segment before the
//!   restored simulation commits, and the restored timeline lazily opens a
//!   new one;
//! - new match install, scenario teardown, and app exit flush before drop;
//! - a failed flush restores the log for retry — a segment is never lost
//!   silently.
//!
//! This JSON artifact is separate from the fixed native recording stream in
//! `sim::replay`.

use std::path::{Path, PathBuf};

use crate::sim::replay::ReplayLog;

/// App-owned diagnostic recording state for the running match.
#[derive(Default)]
pub(crate) struct MatchDiagnosticsState {
    /// The active diagnostic segment; `None` between segments. Opened lazily
    /// by the frame loop, closed by the lifecycle flush points above.
    pub(crate) replay_log: Option<ReplayLog>,
}

pub(crate) struct ReplayLogFlush {
    pub(crate) path: PathBuf,
    pub(crate) tick_count: usize,
}

impl MatchDiagnosticsState {
    /// Persist and consume the active segment. `Ok(None)` when there is no
    /// segment or it recorded nothing. A successful write consumes the log so
    /// repeated teardown hooks cannot duplicate it; any failure restores it
    /// for a later retry. Writes `replay_tick{session_tick}_{unix_secs}.json`
    /// under `replays_dir`.
    pub(crate) fn flush_to(
        &mut self,
        session_tick: u64,
        replays_dir: &Path,
        unix_secs: u64,
    ) -> anyhow::Result<Option<ReplayLogFlush>> {
        let Some(log) = self.replay_log.take() else {
            return Ok(None);
        };
        if log.ticks.is_empty() {
            return Ok(None);
        }

        let result = (|| {
            std::fs::create_dir_all(replays_dir)?;
            let path = replays_dir.join(format!("replay_tick{session_tick}_{unix_secs}.json"));
            log.save(&path)?;
            Ok(Some(ReplayLogFlush {
                path,
                tick_count: log.ticks.len(),
            }))
        })();

        if result.is_err() {
            self.replay_log = Some(log);
        }
        result
    }

    /// Close the active segment at a timeline boundary (new match install or
    /// in-scenario load). Unlike the retry-safe teardown flush, a write
    /// failure here DISCARDS the segment: retaining it would let the next
    /// timeline's ticks append under the previous timeline's header, and a
    /// mixed-timeline artifact misleads exactly the desync investigation this
    /// log exists for. The loss is logged.
    pub(crate) fn close_segment_for_new_timeline(
        &mut self,
        session_tick: u64,
        replays_dir: &Path,
        unix_secs: u64,
    ) -> anyhow::Result<Option<ReplayLogFlush>> {
        let result = self.flush_to(session_tick, replays_dir, unix_secs);
        if result.is_err() {
            self.replay_log = None;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::MatchDiagnosticsState;
    use crate::sim::replay::{ReplayHeader, ReplayLog};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_PATH: AtomicU64 = AtomicU64::new(0);

    fn test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "vera20k-gsi-17-08-{}-{label}-{}",
            std::process::id(),
            NEXT_TEST_PATH.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn sample_log() -> ReplayLog {
        let mut log = ReplayLog::new(ReplayHeader {
            pixel_conversion_bounds: Default::default(),
            version: 71,
            tick_hz: 15,
            seed: 0x1234,
            map_name: "gsi_17_08.map".to_owned(),
            rules_hash: 0x5678,
        });
        log.record_tick(41, Vec::new(), 0x9abc);
        log
    }

    fn diagnostics_with_segment() -> MatchDiagnosticsState {
        MatchDiagnosticsState {
            replay_log: Some(sample_log()),
        }
    }

    #[test]
    fn gsi_17_08_success_writes_decodable_json_once_and_consumes_log() {
        let root = test_path("success");
        let mut diagnostics = diagnostics_with_segment();

        let flush = diagnostics
            .flush_to(41, &root, 1_234)
            .expect("flush succeeds")
            .expect("nonempty log flushes");
        assert!(diagnostics.replay_log.is_none());
        assert_eq!(flush.tick_count, 1);
        assert_eq!(flush.path, root.join("replay_tick41_1234.json"));

        let decoded = ReplayLog::load(&flush.path).expect("written JSON decodes");
        assert_eq!(decoded.header.seed, 0x1234);
        assert_eq!(decoded.header.map_name, "gsi_17_08.map");
        assert_eq!(decoded.ticks.len(), 1);
        assert_eq!(decoded.ticks[0].tick, 41);
        assert_eq!(decoded.ticks[0].state_hash, 0x9abc);

        assert!(
            diagnostics
                .flush_to(41, &root, 1_235)
                .expect("repeat is a no-op")
                .is_none()
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn gsi_17_08_create_and_write_failures_restore_log_for_retry() {
        let create_blocker = test_path("create-failure");
        std::fs::write(&create_blocker, b"not a directory").unwrap();
        let mut diagnostics = diagnostics_with_segment();
        assert!(diagnostics.flush_to(41, &create_blocker, 2_000).is_err());
        assert_eq!(
            diagnostics.replay_log.as_ref().unwrap().ticks[0].state_hash,
            0x9abc
        );
        std::fs::remove_file(&create_blocker).unwrap();

        let root = test_path("write-failure");
        let output = root.join("replay_tick41_2001.json");
        std::fs::create_dir_all(&output).unwrap();
        let mut diagnostics = diagnostics_with_segment();
        assert!(diagnostics.flush_to(41, &root, 2_001).is_err());
        assert_eq!(diagnostics.replay_log.as_ref().unwrap().header.seed, 0x1234);
        assert_eq!(diagnostics.replay_log.as_ref().unwrap().ticks.len(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// F10 lifecycle contract: the segment is app-owned, so nothing that
    /// happens to the simulation slot can drop it — a failed close retains it
    /// for retry, and every successful lifecycle flush (in-scenario load
    /// closing before commit, new match install, scenario teardown, app exit)
    /// rotates the slot so the next timeline lazily opens a fresh segment.
    #[test]
    fn replay_segment_survives_failed_load_and_rotates_on_success_new_match_and_teardown() {
        // Failed close (the failed-load shape: nothing consumed the segment):
        // the segment survives, contents intact.
        let blocked = test_path("lifecycle-blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        let mut diagnostics = diagnostics_with_segment();
        assert!(diagnostics.flush_to(41, &blocked, 3_000).is_err());
        let retained = diagnostics.replay_log.as_ref().expect("segment retained");
        assert_eq!(retained.ticks.len(), 1);
        assert_eq!(retained.ticks[0].state_hash, 0x9abc);
        std::fs::remove_file(&blocked).unwrap();

        // Successful lifecycle flush: the segment closes to disk and the slot
        // rotates to None...
        let root = test_path("lifecycle-rotate");
        let flush = diagnostics
            .flush_to(41, &root, 3_001)
            .expect("close succeeds")
            .expect("segment closes");
        assert!(diagnostics.replay_log.is_none(), "slot rotated");
        assert_eq!(flush.tick_count, 1);

        // ...and the next timeline opens a FRESH segment lazily, exactly as
        // the frame loop does, without inheriting closed ticks.
        diagnostics.replay_log = Some(ReplayLog::new(ReplayHeader {
            pixel_conversion_bounds: Default::default(),
            version: 71,
            tick_hz: 15,
            seed: 0xAAAA,
            map_name: "restored.map".to_owned(),
            rules_hash: 0x5678,
        }));
        let fresh = diagnostics.replay_log.as_ref().unwrap();
        assert_eq!(fresh.header.seed, 0xAAAA);
        assert!(fresh.ticks.is_empty(), "no ticks inherited across rotation");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
