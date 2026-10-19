//! Match diagnostics owner (F10): the Rust-only diagnostic replay log and its
//! retry-safe file lifecycle.
//!
//! Diagnostic state lives beside the runtime, never inside `Simulation` —
//! recording is app-owned so it cannot perturb deterministic state, and the
//! segment's lifetime is owned by explicit app transitions:
//! - lazily opened on the first recorded frame or scenario service input of
//!   a runtime (fresh or restored timelines alike);
//! - a failed load retains the active segment untouched (the log no longer
//!   rides the simulation slot, so replacing or restoring a sim cannot drop
//!   it);
//! - a successful in-scenario load flushes (closes) the segment before the
//!   restored simulation commits, and the restored timeline lazily opens a
//!   new one;
//! - new match install, scenario teardown, and app exit flush before drop;
//! - a failed flush restores the log for retry — a segment is never lost
//!   silently — and a flush never overwrites a published segment: two
//!   segments closing at the same `(tick, unix second)` publish side by
//!   file, the later one under an incrementing name suffix.
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
    /// Open before the first frame or scenario service input of this timeline.
    pub(crate) fn get_or_begin_replay(
        &mut self,
        runtime: &crate::sim::runtime::SimRuntime,
    ) -> &mut ReplayLog {
        self.replay_log.get_or_insert_with(|| {
            let sim = &runtime.simulation;
            ReplayLog::new(crate::sim::replay::ReplayHeader {
                pixel_conversion_bounds: sim.session.pixel_conversion_bounds,
                version: 2,
                tick_hz: crate::app::types::SIM_TICK_HZ,
                seed: sim.session.seed,
                map_name: sim.session.map_name.clone(),
                rules_hash: runtime.resources.rules.simulation_config_hash(),
            })
        })
    }

    /// Persist and consume the active segment. `Ok(None)` when there is no
    /// segment or it recorded nothing. A successful write consumes the log so
    /// repeated teardown hooks cannot duplicate it; any failure restores it
    /// for a later retry. Writes `replay_tick{session_tick}_{unix_secs}.json`
    /// under `replays_dir`; when that name is already taken by an earlier
    /// segment of the same `(tick, second)`, the new segment publishes as
    /// `replay_tick{session_tick}_{unix_secs}_{n}.json` instead of replacing
    /// it (see [`reserve_segment_path`]).
    pub(crate) fn flush_to(
        &mut self,
        session_tick: u64,
        replays_dir: &Path,
        unix_secs: u64,
    ) -> anyhow::Result<Option<ReplayLogFlush>> {
        let Some(log) = self.replay_log.take() else {
            return Ok(None);
        };
        if log.ticks.is_empty() && log.initial_scenario_services.is_empty() {
            return Ok(None);
        }

        let result = (|| {
            std::fs::create_dir_all(replays_dir)?;
            let path = reserve_segment_path(replays_dir, session_tick, unix_secs)?;
            // `save` truncates the reservation into the published artifact.
            // Release the reserved name on failure so a retry can take the
            // same candidate instead of walking to a suffix behind a stray
            // empty file.
            if let Err(error) = log.save(&path) {
                let _ = std::fs::remove_file(&path);
                return Err(error);
            }
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

/// Reserve an unused publication name for one diagnostic segment.
///
/// Two distinct segments can legitimately close at the same simulation tick
/// inside the same Unix second — e.g. an in-scenario load closes the outgoing
/// timeline and the restored timeline replays back to the same close tick —
/// and for a desync investigation both command/hash traces are exactly what
/// must survive, so a later segment never replaces an earlier one. The first
/// segment at a `(tick, second)` keeps the canonical
/// `replay_tick{tick}_{secs}.json` name that existing consumers expect; a
/// colliding segment takes `replay_tick{tick}_{secs}_{n}.json` with an
/// incrementing `n`. Each candidate is claimed with an exclusive `create_new`
/// open, so concurrent or repeated flushes can never settle on the same path
/// even when they race past an existence check.
fn reserve_segment_path(
    replays_dir: &Path,
    session_tick: u64,
    unix_secs: u64,
) -> anyhow::Result<PathBuf> {
    let stem = format!("replay_tick{session_tick}_{unix_secs}");
    let mut suffix: u32 = 0;
    loop {
        let path = if suffix == 0 {
            replays_dir.join(format!("{stem}.json"))
        } else {
            replays_dir.join(format!("{stem}_{suffix}.json"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_reservation) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                suffix += 1;
            }
            Err(error) => {
                return Err(anyhow::Error::new(error).context(format!(
                    "Failed to reserve diagnostic-log name: {}",
                    path.display()
                )));
            }
        }
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

    fn sample_log_with(seed: u64, map_name: &str, state_hash: u64) -> ReplayLog {
        let mut log = ReplayLog::new(ReplayHeader {
            pixel_conversion_bounds: Default::default(),
            version: 71,
            tick_hz: 15,
            seed,
            map_name: map_name.to_owned(),
            rules_hash: 0x5678,
        });
        log.record_tick(41, Vec::new(), Some(state_hash));
        log
    }

    fn sample_log() -> ReplayLog {
        sample_log_with(0x1234, "gsi_17_08.map", 0x9abc)
    }

    fn diagnostics_with_segment() -> MatchDiagnosticsState {
        MatchDiagnosticsState {
            replay_log: Some(sample_log()),
        }
    }

    #[test]
    fn service_before_first_frame_is_flushed_without_inventing_a_tick() {
        use crate::sim::replay::ScenarioServiceInput;
        let root = test_path("initial-service");
        let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(
            crate::sim::world::Simulation::with_seed(17),
        );
        let mut diagnostics = MatchDiagnosticsState::default();
        diagnostics
            .get_or_begin_replay(&runtime)
            .apply_scenario_service(&mut runtime.simulation, ScenarioServiceInput::Pause);
        let flush = diagnostics.flush_to(0, &root, 1234).unwrap().unwrap();
        let log = ReplayLog::load(&flush.path).unwrap();
        assert_eq!(log.header.seed, 17);
        assert!(log.ticks.is_empty());
        assert_eq!(log.initial_scenario_services, [ScenarioServiceInput::Pause]);
        assert!(diagnostics.replay_log.is_none());
        std::fs::remove_dir_all(root).unwrap();
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
        assert_eq!(decoded.ticks[0].state_hash, Some(0x9abc));

        assert!(
            diagnostics
                .flush_to(41, &root, 1_235)
                .expect("repeat is a no-op")
                .is_none()
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// #1338 core regression: two different segments closed at the same
    /// simulation tick inside the same Unix second must both survive as their
    /// own decodable artifacts. Before the fix the second `std::fs::write`
    /// silently replaced the first segment's file.
    #[test]
    fn gsi_17_08_colliding_segments_publish_side_by_side_without_overwrite() {
        let root = test_path("collision");
        let mut first = diagnostics_with_segment();
        let mut second = MatchDiagnosticsState {
            replay_log: Some(sample_log_with(0xBEEF, "restored.map", 0x5555)),
        };

        let first_flush = first
            .flush_to(41, &root, 1_234)
            .expect("first close succeeds")
            .expect("first segment flushes");
        let second_flush = second
            .flush_to(41, &root, 1_234)
            .expect("second close succeeds")
            .expect("second segment flushes");

        // The first segment keeps the canonical name for existing consumers;
        // the colliding segment takes an incrementing suffix.
        assert_eq!(first_flush.path, root.join("replay_tick41_1234.json"));
        assert_eq!(second_flush.path, root.join("replay_tick41_1234_1.json"));
        assert!(first.replay_log.is_none());
        assert!(second.replay_log.is_none());

        let first_log = ReplayLog::load(&first_flush.path).expect("first artifact decodes");
        let second_log = ReplayLog::load(&second_flush.path).expect("second artifact decodes");
        assert_eq!(first_log.header.seed, 0x1234);
        assert_eq!(first_log.ticks[0].state_hash, Some(0x9abc));
        assert_eq!(second_log.header.seed, 0xBEEF);
        assert_eq!(second_log.ticks[0].state_hash, Some(0x5555));

        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A publication walks the suffix sequence past every occupied candidate
    /// and never rewrites an existing artifact. (The old test forced a write
    /// failure with a directory at the target name; under collision-safe
    /// naming an occupied name is no longer a failure on every platform, so
    /// this half now pins the walk instead.)
    #[test]
    fn gsi_17_08_suffix_walks_past_every_occupied_candidate() {
        let root = test_path("suffix-walk");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("replay_tick41_2001.json"), b"earlier segment").unwrap();
        std::fs::write(root.join("replay_tick41_2001_1.json"), b"earlier segment").unwrap();

        let mut diagnostics = diagnostics_with_segment();
        let flush = diagnostics
            .flush_to(41, &root, 2_001)
            .expect("flush succeeds")
            .expect("segment flushes");
        assert_eq!(flush.path, root.join("replay_tick41_2001_2.json"));
        assert_eq!(
            std::fs::read(root.join("replay_tick41_2001.json")).unwrap(),
            b"earlier segment"
        );
        assert_eq!(
            std::fs::read(root.join("replay_tick41_2001_1.json")).unwrap(),
            b"earlier segment"
        );
        assert!(diagnostics.replay_log.is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The realistic in-scenario-load shape from #1338: the outgoing timeline
    /// closes at (tick, second) via the new-timeline close path, the restored
    /// timeline replays back to the same close tick, and its teardown/exit
    /// flush lands on the identical facts. Both timelines' traces survive.
    #[test]
    fn replay_load_and_teardown_closes_at_same_tick_keep_both_segments() {
        let root = test_path("load-close-collision");
        let mut outgoing = diagnostics_with_segment();
        let first = outgoing
            .close_segment_for_new_timeline(41, &root, 1_234)
            .expect("load close succeeds")
            .expect("outgoing segment closes");
        assert_eq!(first.path, root.join("replay_tick41_1234.json"));

        // The restored timeline lazily opens a fresh segment (different
        // header), replays to the same tick, and leaves via a teardown flush.
        let mut restored = MatchDiagnosticsState {
            replay_log: Some(sample_log_with(0xAAAA, "restored.map", 0x1111)),
        };
        let second = restored
            .flush_to(41, &root, 1_234)
            .expect("teardown flush succeeds")
            .expect("restored segment flushes");
        assert_eq!(second.path, root.join("replay_tick41_1234_1.json"));

        assert_eq!(ReplayLog::load(&first.path).unwrap().header.seed, 0x1234);
        assert_eq!(ReplayLog::load(&second.path).unwrap().header.seed, 0xAAAA);
        assert!(outgoing.replay_log.is_none());
        assert!(restored.replay_log.is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn gsi_17_08_create_failure_restores_log_for_retry() {
        let create_blocker = test_path("create-failure");
        std::fs::write(&create_blocker, b"not a directory").unwrap();
        let mut diagnostics = diagnostics_with_segment();
        assert!(diagnostics.flush_to(41, &create_blocker, 2_000).is_err());
        assert_eq!(
            diagnostics.replay_log.as_ref().unwrap().ticks[0].state_hash,
            Some(0x9abc)
        );
        std::fs::remove_file(&create_blocker).unwrap();
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
        assert_eq!(retained.ticks[0].state_hash, Some(0x9abc));
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
