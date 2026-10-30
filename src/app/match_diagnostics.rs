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
    command_executions: Option<CommandExecutionLog>,
    actor_presentation: std::cell::RefCell<Option<ActorPresentationProbe>>,
    loading_cancel: Option<Vec<LoadingCancelObservation>>,
    replay_playback: Option<ReplayPlaybackState>,
    replay_capture_output: Option<PathBuf>,
    replay_artifact_sha256: Option<String>,
}

enum ReplayPlaybackState {
    Loading(std::sync::Arc<ReplayLog>),
    Playing(crate::sim::replay::ReplayPlayback),
    Failed(String),
}

/// One actual post-formation native command dispatch and its immediate
/// receiver state. This opt-in app observation is never saved or hashed.
pub(crate) struct CommandExecutionObservation {
    pub observed_at: ObservationTime,
    pub frame: u32,
    pub record: crate::sim::command::CommandRecord,
    pub applied: bool,
    pub actor: Option<u64>,
    pub motion: Option<ActorMotionObservation>,
    pub timing: Option<CommandTimingObservation>,
    pub destination: Option<crate::sim::components::NavTargetRef>,
    pub cell: Option<(u16, u16)>,
    pub mission: Option<(i32, i32)>,
}

/// Existing movement owners projected for app diagnostics; no mutable access.
#[derive(Clone, serde::Serialize)]
pub(crate) struct ActorMotionObservation {
    pub actor_type: String,
    pub locomotor: Option<String>,
    pub applied_fraction: f64,
    pub applied_fraction_bits: u64,
    pub moving: Option<bool>,
    pub drive_destination: Option<crate::sim::components::DriveCoord>,
    pub drive_head: Option<crate::sim::components::DriveCoord>,
    pub mission: (i32, i32),
}
impl ActorMotionObservation {
    pub(crate) fn read(
        sim: &crate::sim::world::Simulation,
        entity: &crate::sim::game_entity::GameEntity,
    ) -> Self {
        use crate::sim::movement::track_process::TrackFamily;
        let drive = entity.locomotor.as_ref().filter(|loco| {
            loco.active_kind() == crate::rules::locomotor_type::LocomotorKind::Drive
        });
        let bits = entity.foot_speed.native_applied_fraction().bits();
        Self {
            actor_type: sim.interner().resolve(entity.type_ref()).to_owned(),
            locomotor: entity
                .locomotor
                .as_ref()
                .map(|loco| format!("{:?}", loco.active_kind())),
            applied_fraction: f64::from_bits(bits),
            applied_fraction_bits: bits,
            moving: crate::sim::movement::motion_query::is_moving(entity),
            drive_destination: drive.and_then(|loco| loco.track_destination(TrackFamily::Drive)),
            drive_head: drive.and_then(|loco| loco.track_head(TrackFamily::Drive)),
            mission: (
                entity.mission.current().raw(),
                entity.mission.queued().raw(),
            ),
        }
    }
}

/// App-only clocks for measurement. Neither clock enters commands or hashes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ObservationTime {
    pub instant: crate::util::time::Instant,
    pub unix_us: u64,
}
impl ObservationTime {
    pub(crate) fn now() -> Self {
        Self {
            instant: crate::util::time::Instant::now(),
            unix_us: crate::util::time::SystemTime::now()
                .duration_since(crate::util::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
        }
    }
}

pub(crate) type GpuCompletion = std::sync::Arc<std::sync::Mutex<Option<ObservationTime>>>;

#[derive(Clone)]
pub(crate) struct ActorPresentation {
    pub at: ObservationTime,
    pub frame: u32,
    pub screen: [f32; 2],
    pub body: u16,
    pub motion: ActorMotionObservation,
    pub sampled_from_frame: u32,
    pub gpu_done: GpuCompletion,
}
impl ActorPresentation {
    pub(crate) fn gpu_completed_at(&self) -> Option<ObservationTime> {
        *self
            .gpu_done
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}
struct ActorPresentationProbe {
    actor: u64,
    pending: Option<(u32, [f32; 2], u16, ActorMotionObservation, u32)>,
    submitted: Option<ActorPresentation>,
}
pub(crate) struct CommandTimingObservation {
    pub publisher: Option<i8>,
    pub fps: Option<u16>,
    pub frame_send_rate: Option<u8>,
    pub max_ahead: Option<i32>,
}

#[derive(Default)]
pub(crate) struct CommandExecutionLog {
    records: Vec<CommandExecutionObservation>,
    overflowed: bool,
}
impl CommandExecutionLog {
    pub(crate) fn record(
        &mut self,
        sim: &crate::sim::world::Simulation,
        record: &crate::sim::command::CommandRecord,
        applied: bool,
    ) {
        let actor = crate::sim::command::MegaMissionRecord::decode(record)
            .map(|r| r.source_id)
            .or_else(|| crate::sim::command::StopRecord::decode(record).map(|r| r.entity_id));
        let actor = actor.and_then(|id| u64::try_from(id).ok());
        let is_timing = matches!(
            record.opcode(),
            crate::sim::command::TIMING_OPCODE | crate::sim::command::PROCESS_TIME_OPCODE
        );
        if actor.is_none() && !is_timing {
            return;
        }
        if self.records.len() >= 131072 {
            self.overflowed = true;
            return;
        }
        let entity = actor.and_then(|actor| sim.entities().get(actor));
        self.records.push(CommandExecutionObservation {
            observed_at: ObservationTime::now(),
            frame: sim.session().binary_frame(),
            record: record.clone(),
            applied,
            actor,
            motion: entity.map(|entity| ActorMotionObservation::read(sim, entity)),
            timing: sim
                .session()
                .network_command_timing()
                .map(|timing| CommandTimingObservation {
                    publisher: sim.network_timing_host_house(),
                    fps: Some(timing.frames_per_second()),
                    frame_send_rate: Some(timing.frame_send_rate().get()),
                    max_ahead: sim.session().network_max_ahead_frames(),
                }),
            destination: entity.and_then(|e| e.navigation.nav_com()),
            cell: entity.map(|e| (e.position.rx(), e.position.ry())),
            mission: entity.map(|e| (e.mission.current().raw(), e.mission.queued().raw())),
        });
    }
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct LoadingCancelObservation {
    pub event: &'static str,
    pub match_id: Option<String>,
    pub source: Option<usize>,
    pub worker_owner_taken: bool,
    pub loading_active: bool,
}

pub(crate) struct ReplayLogFlush {
    pub(crate) path: PathBuf,
    pub(crate) tick_count: usize,
}

impl MatchDiagnosticsState {
    pub(crate) fn actor_presentation_observer_active(&self) -> bool {
        self.actor_presentation.borrow().is_some()
    }

    pub(crate) fn observe_actor_presentation(&self, actor: u64) {
        *self.actor_presentation.borrow_mut() = Some(ActorPresentationProbe {
            actor,
            pending: None,
            submitted: None,
        });
    }
    pub(crate) fn begin_presentation_observation(&self) {
        if let Some(probe) = self.actor_presentation.borrow_mut().as_mut() {
            probe.pending = None;
            probe.submitted = None;
        }
    }
    pub(crate) fn record_actor_draw(
        &self,
        sim: &crate::sim::world::Simulation,
        entity: &crate::sim::game_entity::GameEntity,
        frame: u32,
        screen: [f32; 2],
        body: u16,
        interpolation: &crate::app::presentation::interpolation::RenderInterpolation,
    ) {
        if let Some(probe) = self.actor_presentation.borrow_mut().as_mut()
            && probe.actor == entity.stable_id()
        {
            probe.pending = Some((
                frame,
                screen,
                body,
                ActorMotionObservation::read(sim, entity),
                interpolation.sampled_from_frame(entity, frame),
            ));
        }
    }
    pub(crate) fn commit_presentation_observation(&self) -> Option<GpuCompletion> {
        if let Some(probe) = self.actor_presentation.borrow_mut().as_mut() {
            probe.submitted =
                probe
                    .pending
                    .take()
                    .map(
                        |(frame, screen, body, motion, sampled_from_frame)| ActorPresentation {
                            at: ObservationTime::now(),
                            frame,
                            screen,
                            body,
                            motion,
                            sampled_from_frame,
                            gpu_done: Default::default(),
                        },
                    );
            return probe.submitted.as_ref().map(|frame| frame.gpu_done.clone());
        }
        None
    }
    pub(crate) fn actor_presentation(&self) -> Option<ActorPresentation> {
        self.actor_presentation.borrow().as_ref()?.submitted.clone()
    }
    pub(crate) fn end_presentation_observation(&self) {
        *self.actor_presentation.borrow_mut() = None;
    }
    pub(crate) fn is_replay_playback(&self) -> bool {
        self.replay_playback.is_some()
    }

    pub(crate) fn begin_playback(
        &mut self,
        replay: std::sync::Arc<ReplayLog>,
        capture_output: Option<PathBuf>,
        replay_sha256: String,
    ) {
        self.replay_playback = Some(ReplayPlaybackState::Loading(replay));
        self.replay_capture_output = capture_output;
        self.replay_artifact_sha256 = Some(replay_sha256);
    }

    pub(crate) fn install_playback(
        &mut self,
        runtime: &mut crate::sim::runtime::SimRuntime,
        source: &crate::map::source::LoadedMapSource,
        map_hash: u64,
    ) -> anyhow::Result<()> {
        let replay = match self.replay_playback.take() {
            Some(ReplayPlaybackState::Loading(replay)) => replay,
            other => {
                self.replay_playback = other;
                anyhow::bail!("replay loader lost its immutable startup");
            }
        };
        match crate::sim::replay::ReplayPlayback::prepare(runtime, replay, source, map_hash) {
            Ok(playback) => {
                self.replay_playback = Some(ReplayPlaybackState::Playing(playback));
                Ok(())
            }
            Err(error) => {
                self.replay_playback = Some(ReplayPlaybackState::Failed(format!("{error:#}")));
                Err(error)
            }
        }
    }

    pub(crate) fn playback_can_advance(&self) -> bool {
        matches!(&self.replay_playback, Some(ReplayPlaybackState::Playing(playback)) if playback.can_advance())
    }

    pub(crate) fn playback_status(&self) -> Option<String> {
        Some(match self.replay_playback.as_ref()? {
            ReplayPlaybackState::Loading(_) => "Loading replay".to_owned(),
            ReplayPlaybackState::Failed(error) => format!("Replay failed: {error} — Esc to exit"),
            ReplayPlaybackState::Playing(playback) => match playback.failure() {
                Some(error) => format!("Replay failed: {error} — Esc to exit"),
                None if playback.is_finished() => {
                    "Replay ended — camera controls available; Esc to exit".to_owned()
                }
                None => "Replay — camera controls available; Esc to exit".to_owned(),
            },
        })
    }

    /// Disjoint borrows keep the recorder/observation owner beside playback;
    /// no caller moves the cursor out or temporarily disables the input gate.
    pub(crate) fn playback_and_observations_mut(
        &mut self,
    ) -> (
        Option<&mut crate::sim::replay::ReplayPlayback>,
        Option<&mut CommandExecutionLog>,
    ) {
        let playback = match &mut self.replay_playback {
            Some(ReplayPlaybackState::Playing(playback)) => Some(playback),
            _ => None,
        };
        (playback, self.command_executions.as_mut())
    }

    pub(crate) fn playback_capture_ready(&self) -> bool {
        self.replay_capture_output.is_some()
            && match &self.replay_playback {
                Some(ReplayPlaybackState::Failed(_)) => true,
                Some(ReplayPlaybackState::Playing(playback)) => !playback.can_advance(),
                _ => false,
            }
    }

    pub(crate) fn playback_capture_requested(&self) -> bool {
        self.replay_capture_output.is_some()
    }

    /// The explicit automation flag captures the same final visible frame.
    /// Files are exclusively created; ordinary replay never writes a capture
    /// or exits automatically.
    pub(crate) fn complete_playback_capture(
        &mut self,
        runtime: &crate::sim::runtime::SimRuntime,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        use image::ImageEncoder;
        use std::io::Write;
        anyhow::ensure!(
            self.playback_capture_ready(),
            "replay capture is not complete"
        );
        anyhow::ensure!(
            pixels.len() as u64 == u64::from(width) * u64::from(height) * 4,
            "replay BGRA readback dimensions differ"
        );
        let path = self
            .replay_capture_output
            .as_ref()
            .context("replay capture path")?;
        let mut rgba = pixels.to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(
            &rgba,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )?;
        let verification = match &self.replay_playback {
            Some(ReplayPlaybackState::Playing(playback)) if playback.failure().is_none() => {
                Some(playback.verification(runtime)?)
            }
            _ => None,
        };
        let receipt = serde_json::json!({
            "replay_sha256": self.replay_artifact_sha256,
            "png_sha256": crate::util::sha256::sha256_hex(&png),
            "width": width, "height": height, "status": self.playback_status(),
            "verification": verification,
            "recorded_final_state_hash": match &self.replay_playback {
                Some(ReplayPlaybackState::Playing(playback)) => playback.recorded_final_hash(), _ => None,
            },
        });
        for (output, bytes) in [
            (path.clone(), png),
            (
                path.with_extension("json"),
                serde_json::to_vec_pretty(&receipt)?,
            ),
        ] {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        self.replay_capture_output = None;
        Ok(())
    }

    pub(crate) fn begin_loading_cancel_observation(&mut self) {
        self.loading_cancel.get_or_insert_with(Vec::new);
    }
    pub(crate) fn observe_loading_cancel(
        &mut self,
        event: &'static str,
        match_id: Option<String>,
        source: Option<usize>,
        worker_owner_taken: bool,
        loading_active: bool,
    ) {
        if let Some(log) = self.loading_cancel.as_mut() {
            if log.len() < 16 {
                log.push(LoadingCancelObservation {
                    event,
                    match_id,
                    source,
                    worker_owner_taken,
                    loading_active,
                });
            }
        }
    }
    pub(crate) fn loading_cancel_observations(&self) -> Option<&[LoadingCancelObservation]> {
        self.loading_cancel.as_deref()
    }
    pub(crate) fn end_loading_cancel_observation(&mut self) {
        self.loading_cancel = None;
    }

    pub(crate) fn begin_command_observation(&mut self) {
        self.command_executions = Some(CommandExecutionLog::default());
    }
    pub(crate) fn command_observations_mut(&mut self) -> Option<&mut CommandExecutionLog> {
        self.command_executions.as_mut()
    }
    pub(crate) fn take_command_observations(
        &mut self,
    ) -> anyhow::Result<Vec<CommandExecutionObservation>> {
        let Some(log) = self.command_executions.as_mut() else {
            return Ok(Vec::new());
        };
        anyhow::ensure!(!log.overflowed, "command execution observation overflowed");
        Ok(std::mem::take(&mut log.records))
    }
    pub(crate) fn end_command_observation(&mut self) {
        self.command_executions = None;
    }

    /// Open before the first frame or scenario service input of this timeline.
    pub(crate) fn get_or_begin_replay(
        &mut self,
        runtime: &crate::sim::runtime::SimRuntime,
    ) -> Option<&mut ReplayLog> {
        if self.is_replay_playback() {
            return None;
        }
        // Trainer changes are deliberately outside the ordinary command log.
        // The caller closes the valid prefix before the first such mutation;
        // never emit an apparently replayable continuation afterward.
        #[cfg(feature = "debug-tools")]
        if runtime.simulation.trainer_modified() {
            self.replay_log = None;
            return None;
        }
        Some(self.replay_log.get_or_insert_with(|| {
            let sim = &runtime.simulation;
            ReplayLog::new(crate::sim::replay::ReplayHeader {
                pixel_conversion_bounds: sim.session().pixel_conversion_bounds(),
                version: 2,
                tick_hz: crate::app::types::SIM_TICK_HZ,
                seed: sim.session().seed(),
                map_name: sim.session().map_name().to_owned(),
                rules_hash: runtime.resources.rules.simulation_config_hash(),
            })
        }))
    }

    /// Attach startup only before any commands or scenario service has been
    /// recorded. The Simulation and accepted launch remain the sole owners.
    pub(crate) fn retain_replay_startup(
        &mut self,
        runtime: &crate::sim::runtime::SimRuntime,
        launch: Option<&crate::skirmish_launch::SkirmishLaunchSession>,
        source: Option<&crate::map::source::LoadedMapSource>,
        map_hash: Option<u64>,
        every_frame_network: bool,
    ) {
        let Some(log) = self.replay_log.as_mut().filter(|log| {
            log.startup.is_none()
                && log.ticks.is_empty()
                && log.initial_scenario_services.is_empty()
        }) else {
            return;
        };
        let (Some(launch), Some(source), Some(map_hash)) = (launch, source, map_hash) else {
            return;
        };
        match crate::sim::replay::ReplayStartup::capture(
            runtime,
            launch,
            source,
            map_hash,
            crate::app::types::SIM_TICK_MS,
            every_frame_network,
        ) {
            Ok(startup) => {
                log.header.version = crate::sim::replay::REPLAY_VERSION;
                log.startup = Some(Box::new(startup));
            }
            Err(error) => log::debug!("Standalone replay startup unavailable: {error}"),
        }
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
        // The browser has no std::fs and nothing reads a segment back: it goes
        // straight to the host, or nowhere when the host takes no replays.
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        {
            let Some(log) = self.replay_log.take() else {
                return Ok(None);
            };
            if (log.ticks.is_empty() && log.initial_scenario_services.is_empty())
                || !crate::web::has_replay_callback()
            {
                return Ok(None);
            }
            let name = format!("replay_tick{session_tick}_{unix_secs}.json");
            let mut bytes = Vec::new();
            let written = log
                .write_json(&mut bytes)
                .and_then(|()| crate::web::hand_over_replay(&name, &bytes));
            if let Err(error) = written {
                self.replay_log = Some(log);
                return Err(error);
            }
            return Ok(Some(ReplayLogFlush {
                path: replays_dir.join(name),
                tick_count: log.ticks.len(),
            }));
        }
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        self.flush_to_with_writer(session_tick, replays_dir, unix_secs, |log, file| {
            log.write_json(file)
        })
    }

    fn flush_to_with_writer(
        &mut self,
        session_tick: u64,
        replays_dir: &Path,
        unix_secs: u64,
        write: impl FnOnce(&ReplayLog, &mut std::fs::File) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<ReplayLogFlush>> {
        let Some(log) = self.replay_log.take() else {
            return Ok(None);
        };
        if log.ticks.is_empty() && log.initial_scenario_services.is_empty() {
            return Ok(None);
        }

        let result = (|| {
            crate::util::file_system::create_dir_all(replays_dir)?;
            let (path, mut file) = reserve_segment_path(replays_dir, session_tick, unix_secs)?;
            // Keep the exclusively created handle through the write. Reopening
            // its path could truncate a replacement supplied after reservation.
            let written = write(&log, &mut file);
            drop(file);
            if let Err(error) = written {
                // Close before deleting so retry cleanup also works on Windows.
                let _ = crate::util::file_system::remove_file(&path);
                return Err(error.context(format!("Diagnostic log: {}", path.display())));
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
) -> anyhow::Result<(PathBuf, std::fs::File)> {
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
            Ok(file) => return Ok((path, file)),
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

    #[test]
    fn actor_observer_is_active_before_a_draw_and_until_explicit_end() {
        let diagnostics = MatchDiagnosticsState::default();
        assert!(!diagnostics.actor_presentation_observer_active());
        diagnostics.observe_actor_presentation(7);
        assert!(diagnostics.actor_presentation_observer_active());
        assert!(diagnostics.actor_presentation().is_none());
        diagnostics.begin_presentation_observation();
        assert!(diagnostics.actor_presentation_observer_active());
        diagnostics.end_presentation_observation();
        assert!(!diagnostics.actor_presentation_observer_active());
    }

    #[cfg(feature = "debug-tools")]
    #[test]
    fn trainer_modified_world_cannot_resume_normal_replay_recording() {
        use crate::sim::cheats::{CheatFlags, DebugSessionContext};
        use crate::sim::house_state::HouseState;
        let mut sim = crate::sim::world::Simulation::with_seed(17);
        let owner = sim.intern("Allies");
        sim.register_house(HouseState::new(owner, 0, None, true, 500, 10));
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
        let mut diagnostics = MatchDiagnosticsState::default();
        assert!(diagnostics.get_or_begin_replay(&runtime).is_some());
        runtime
            .simulation
            .set_debug_session_context(DebugSessionContext::Offline);
        runtime
            .simulation
            .set_cheat_flags(CheatFlags {
                infinite_money: true,
                money_target: 1000,
                ..Default::default()
            })
            .unwrap();
        runtime
            .simulation
            .apply_debug_tools(crate::rules::ruleset::RuleSet::empty_for_test());
        assert!(diagnostics.get_or_begin_replay(&runtime).is_none());
        assert!(diagnostics.replay_log.is_none());
        runtime.simulation.clear_debug_tools();
        assert!(
            diagnostics.get_or_begin_replay(&runtime).is_none(),
            "switching off cannot make a mutated world replayable"
        );
    }

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

    #[test]
    fn playback_owns_input_during_loading_and_failure_without_recording_or_changing_world() {
        let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(
            crate::sim::world::Simulation::with_seed(7),
        );
        let hash = runtime.simulation.state_hash();
        let mut diagnostics = MatchDiagnosticsState::default();
        diagnostics.begin_playback(std::sync::Arc::new(sample_log()), None, "0".repeat(64));
        assert!(diagnostics.is_replay_playback());
        assert!(!diagnostics.playback_can_advance());
        assert!(diagnostics.get_or_begin_replay(&runtime).is_none());
        assert!(
            diagnostics
                .install_playback(
                    &mut runtime,
                    &crate::map::source::LoadedMapSource::LegacyFallback {
                        label: "unused".into()
                    },
                    0
                )
                .is_err()
        );
        assert!(diagnostics.is_replay_playback());
        assert!(!diagnostics.playback_can_advance());
        assert!(
            diagnostics
                .playback_status()
                .unwrap()
                .starts_with("Replay failed")
        );
        assert!(diagnostics.get_or_begin_replay(&runtime).is_none());
        assert_eq!(runtime.simulation.state_hash(), hash);
    }

    fn diagnostics_with_segment() -> MatchDiagnosticsState {
        MatchDiagnosticsState {
            replay_log: Some(sample_log()),
            ..Default::default()
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
            .expect("unmodified offline match records")
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
            ..Default::default()
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
            ..Default::default()
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
    fn partial_write_failure_restores_the_segment_and_reuses_the_reserved_name() {
        use std::io::Write;
        struct PartialWriter<'a> {
            file: &'a mut std::fs::File,
            written: usize,
        }
        impl Write for PartialWriter<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.written != 0 {
                    return Err(std::io::Error::other("injected partial-write failure"));
                }
                let length = bytes.len().min(17);
                self.file.write_all(&bytes[..length])?;
                self.written = length;
                Ok(length)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.file.flush()
            }
        }
        let root = test_path("partial-write");
        let first = diagnostics_with_segment()
            .flush_to(41, &root, 1234)
            .unwrap()
            .unwrap();
        let original_bytes = std::fs::read(&first.path).unwrap();
        let mut second = MatchDiagnosticsState {
            replay_log: Some(sample_log_with(0xBEEF, "restored.map", 0x5555)),
            ..Default::default()
        };
        let error = second.flush_to_with_writer(41, &root, 1234, |log, file| {
            let mut writer = PartialWriter { file, written: 0 };
            let result = log.write_json(&mut writer);
            assert_eq!(
                writer.written, 17,
                "error follows a real nonempty file write"
            );
            result
        });
        assert!(error.is_err());
        assert_eq!(second.replay_log.as_ref().unwrap().header.seed, 0xBEEF);
        assert_eq!(
            second.replay_log.as_ref().unwrap().ticks[0].state_hash,
            Some(0x5555)
        );
        let candidate = root.join("replay_tick41_1234_1.json");
        assert!(!candidate.exists(), "partial reservation must be removed");
        assert_eq!(std::fs::read(&first.path).unwrap(), original_bytes);
        let retry = second.flush_to(41, &root, 1234).unwrap().unwrap();
        assert_eq!(retry.path, candidate);
        assert_eq!(ReplayLog::load(&retry.path).unwrap().header.seed, 0xBEEF);
        assert!(second.replay_log.is_none());
        assert!(second.flush_to(41, &root, 1234).unwrap().is_none());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn reserved_handle_does_not_reopen_a_replaced_path() {
        let root = test_path("reserved-handle");
        std::fs::create_dir_all(&root).unwrap();
        let (path, mut file) = super::reserve_segment_path(&root, 41, 1234).unwrap();
        let moved = root.join("original-reservation.json");
        std::fs::rename(&path, &moved).unwrap();
        std::fs::write(&path, b"replacement sentinel").unwrap();
        sample_log().write_json(&mut file).unwrap();
        drop(file);
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement sentinel");
        assert_eq!(ReplayLog::load(&moved).unwrap().header.seed, 0x1234);
        std::fs::remove_dir_all(root).unwrap();
    }
}
