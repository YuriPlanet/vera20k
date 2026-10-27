//! Opt-in wall-time measurements beside the existing live capture. Commands,
//! simulation time, and network policy remain owned by their production paths.
use super::*;
use crate::app::match_diagnostics::{
    ActorPresentation, CommandExecutionObservation, ObservationTime,
};
use crate::sim::command::{MegaMissionOrder, MegaMissionRecord, StopRecord};

const TARGET_PAIRS: usize = 30;
const WARMUP_MS: u64 = 5_000;
const WARMUP_LIMIT_MS: u64 = 30_000;
const STABLE_WINDOW_MS: u64 = 2_000;
const MAX_RENDER_GAP_US: u64 = 100_000;
const ACTION_TIMEOUT_MS: u64 = 5_000;
const STOP_QUIET_MS: u64 = 100;
const STOP_PREPARATION_MIN_MS: u64 = 350;
const STOP_PREPARATION_LIMIT_MS: u64 = 5_000;
const STOP_STEADY_MS: u64 = 100;
// Probe eligibility, not a native movement rule: measure braking from a
// meaningfully moving, stable-speed state rather than during acceleration.
const STOP_MIN_FRACTION: f64 = 0.5;

fn recent_draw(draw: &ActorPresentation, at: ObservationTime) -> bool {
    at.instant >= draw.at.instant
        && at.instant.saturating_duration_since(draw.at.instant)
            <= std::time::Duration::from_millis(STOP_STEADY_MS)
}

fn recent_translation(previous: &ActorPresentation, latest: &ActorPresentation) -> bool {
    previous.frame < latest.frame
        && previous.at.instant < latest.at.instant
        && recent_draw(previous, latest.at)
        && previous.screen != latest.screen
}

struct StopPreparation {
    move_action_id: usize,
    started: ObservationTime,
    started_frame: u32,
    checked_at: ObservationTime,
    latest: Option<ActorPresentation>,
    steady_since: Option<ActorPresentation>,
    steady_draws: usize,
    ready: Option<ObservationTime>,
    failed: bool,
}
impl StopPreparation {
    fn new(move_action_id: usize, started: ObservationTime, started_frame: u32) -> Self {
        Self {
            move_action_id,
            started,
            started_frame,
            checked_at: started,
            latest: None,
            steady_since: None,
            steady_draws: 0,
            ready: None,
            failed: false,
        }
    }

    fn observe(&mut self, draw: &ActorPresentation) {
        if self.ready.is_some()
            || self.failed
            || draw.at.instant < self.started.instant
            || self
                .latest
                .as_ref()
                .is_some_and(|last| draw.frame <= last.frame)
        {
            return;
        }
        let moving = draw.motion.locomotor.as_deref() == Some("Drive")
            && draw.motion.moving == Some(true)
            && draw.motion.applied_fraction.is_finite()
            && draw.motion.applied_fraction >= STOP_MIN_FRACTION;
        let continues = moving
            && self.steady_since.is_some()
            && self.latest.as_ref().is_some_and(|last| {
                last.motion.applied_fraction_bits == draw.motion.applied_fraction_bits
                    && recent_translation(last, draw)
            });
        if continues {
            self.steady_draws += 1;
        } else {
            self.steady_since = moving.then(|| draw.clone());
            self.steady_draws = usize::from(moving);
        }
        self.latest = Some(draw.clone());
    }

    fn poll(&mut self, now: ObservationTime) -> bool {
        if self.ready.is_some() {
            return true;
        }
        self.checked_at = now;
        let elapsed = now
            .instant
            .saturating_duration_since(self.started.instant)
            .as_millis();
        if elapsed > u128::from(STOP_PREPARATION_LIMIT_MS) {
            self.failed = true;
            return false;
        }
        let steady = self
            .steady_since
            .as_ref()
            .zip(self.latest.as_ref())
            .is_some_and(|(first, last)| {
                self.steady_draws >= 3
                    && last
                        .at
                        .instant
                        .saturating_duration_since(first.at.instant)
                        .as_millis()
                        >= u128::from(STOP_STEADY_MS)
                    && recent_draw(last, now)
            });
        if !self.failed && elapsed >= u128::from(STOP_PREPARATION_MIN_MS) && steady {
            self.ready = Some(now);
            return true;
        }
        self.failed |= elapsed >= u128::from(STOP_PREPARATION_LIMIT_MS);
        false
    }
}

struct Action {
    kind: &'static str,
    actor: u64,
    target: Option<(u16, u16)>,
    issue: ObservationTime,
    issue_frame: u32,
    admission: Value,
    baseline: ([f32; 2], u16),
    execution: Option<usize>,
    response: Option<ActorPresentation>,
    quiet: Option<ActorPresentation>,
    error: Option<&'static str>,
    completion_logged: bool,
    stop_reaction: StopReaction,
}

#[derive(Default)]
struct StopReaction {
    prepared_draw: Option<ActorPresentation>,
    baseline: Option<ActorPresentation>,
    no_op: bool,
    rendered_travel_before: bool,
    deceleration_frame: Option<u32>,
    first: Option<ActorPresentation>,
    criterion: Option<&'static str>,
}
impl StopReaction {
    fn from_prepared_move(preparation: Option<&StopPreparation>, at: ObservationTime) -> Self {
        let prepared_draw = preparation
            .filter(|preparation| {
                !preparation.failed
                    && preparation
                        .ready
                        .is_some_and(|ready| ready.instant <= at.instant)
            })
            .and_then(|preparation| preparation.latest.as_ref())
            .filter(|draw| recent_draw(draw, at))
            .cloned();
        Self {
            rendered_travel_before: prepared_draw.is_some(),
            prepared_draw,
            ..Default::default()
        }
    }
}

struct Execution {
    observed: CommandExecutionObservation,
    kind: &'static str,
    target: Option<(u16, u16)>,
}

#[derive(Clone, Copy)]
struct ProgressPoint {
    at: crate::util::time::Instant,
    frame: u32,
    fps: Option<u16>,
    backlog: u32,
    recovering: bool,
    maximum_render_interval_us: u64,
}

struct PreparedView {
    at: ObservationTime,
    frame: u32,
    camera: (f32, f32),
    zoom: f32,
    viewport: (u32, u32, u32, u32),
    first_draw: Option<ActorPresentation>,
    maintained: bool,
}
impl PreparedView {
    fn observe_draw(&mut self, draw: &ActorPresentation) {
        if self.first_draw.is_none() && draw.at.instant >= self.at.instant {
            self.first_draw = Some(draw.clone());
        }
    }

    fn ready(&self) -> bool {
        self.maintained
            && self
                .first_draw
                .as_ref()
                .is_some_and(|draw| draw.gpu_completed_at().is_some())
    }
}

#[derive(Default)]
pub(super) struct LatencyCapture {
    started: Option<ObservationTime>,
    render_deferral_start: u64,
    render_deferred_passes: u64,
    start_frame: u32,
    warmup: Option<Value>,
    prepared: Option<PreparedView>,
    warmup_started: Option<ObservationTime>,
    actor: Option<u64>,
    origin: Option<(u16, u16)>,
    movement_observed: bool,
    navigation_clear_at_end: bool,
    actions: Vec<Action>,
    first_move_trace: Vec<ActorPresentation>,
    stop_preparations: Vec<StopPreparation>,
    executions: Vec<Execution>,
    samples: Vec<Value>,
    startup_samples: Vec<Value>,
    recent: std::collections::VecDeque<ProgressPoint>,
    last_sample: Option<ObservationTime>,
    last_present: Option<ObservationTime>,
    latest_actor_draw: Option<ActorPresentation>,
    previous_actor_draw: Option<ActorPresentation>,
    maximum_render_interval_us: u64,
    next_issue: Option<crate::util::time::Instant>,
    errors: Vec<String>,
    peer_rtt: Option<Vec<(u8, u32)>>,
    finished: bool,
}

impl LatencyCapture {
    pub(super) fn observe_setup(&mut self, state: &AppState) {
        if let Some(rtt) = crate::app::lan_service::online_peer_round_trips(state) {
            self.peer_rtt = Some(rtt);
        }
    }

    fn timestamp(&self, at: ObservationTime, frame: u32) -> Value {
        json!({"mono_us":at.instant.saturating_duration_since(self.started.unwrap().instant).as_micros() as u64,
            "unix_us":at.unix_us,"frame":frame})
    }

    fn observe_execution(&mut self, observed: CommandExecutionObservation) {
        let (kind, target) = if let Some(command) = MegaMissionRecord::decode(&observed.record) {
            let MegaMissionOrder::Move { target_x, target_y } = command.order else {
                return;
            };
            ("Move", Some((target_x as u16, target_y as u16)))
        } else if StopRecord::decode(&observed.record).is_some() {
            ("Stop", None)
        } else {
            return;
        };
        if self.executions.len() >= 4096 {
            if self.errors.is_empty() {
                self.errors
                    .push("command observation capacity exceeded".into());
            }
            return;
        }
        let index = self.executions.len();
        let rendered_before = self
            .latest_actor_draw
            .as_ref()
            .filter(|draw| {
                draw.at.instant < observed.observed_at.instant
                    && recent_draw(draw, observed.observed_at)
            })
            .cloned();
        if let Some(action) = self.actions.iter_mut().find(|action| {
            action.execution.is_none()
                && action.kind == kind
                && Some(action.actor) == observed.actor
                && action.target == target
                && observed.observed_at.instant >= action.issue.instant
        }) {
            if !observed.applied {
                action.error = Some("receiver rejected command");
            }
            action.execution = Some(index);
            if kind == "Stop" {
                action.stop_reaction.no_op = rendered_before
                    .as_ref()
                    .is_some_and(|draw| draw.motion.moving == Some(false))
                    && observed
                        .motion
                        .as_ref()
                        .is_some_and(|motion| motion.moving == Some(false));
                // Keep the validated steady moving proof from the click even
                // if one later admitted draw repeats the same pixel position.
                action.stop_reaction.rendered_travel_before |= rendered_before
                    .as_ref()
                    .zip(self.previous_actor_draw.as_ref())
                    .is_some_and(|(latest, previous)| recent_translation(previous, latest));
                action.stop_reaction.baseline = rendered_before;
            }
        }
        self.executions.push(Execution {
            observed,
            kind,
            target,
        });
    }

    fn observe_presented(&mut self, presented: ActorPresentation) {
        if self.actions.len() == 1
            && self.actions[0].kind == "Move"
            && self.actions[0].response.is_none()
            && presented.at.instant >= self.actions[0].issue.instant
            && self.first_move_trace.len() < 64
            && self
                .first_move_trace
                .last()
                .is_none_or(|last| last.frame < presented.frame)
        {
            self.first_move_trace.push(presented.clone());
        }
        let Some(action) = self.actions.last_mut() else {
            return;
        };
        let Some(index) = action.execution else {
            return;
        };
        let execution = &self.executions[index].observed;
        observe_stop_reaction(&mut action.stop_reaction, execution, &presented);
        if action.response.is_some() || presented.at.instant < execution.observed_at.instant {
            return;
        }
        if action.kind == "Move" {
            if (presented.screen, presented.body) != action.baseline {
                action.response = Some(presented);
            }
        } else if execution.destination.is_none() {
            match action.quiet.as_ref() {
                Some(first) if first.screen == presented.screen && first.body == presented.body => {
                    if presented.frame.saturating_sub(first.frame) >= 10
                        && presented
                            .at
                            .instant
                            .saturating_duration_since(first.at.instant)
                            .as_millis()
                            >= u128::from(STOP_QUIET_MS)
                    {
                        action.response = Some(first.clone());
                    }
                }
                _ => action.quiet = Some(presented),
            }
        }
    }

    /// Called after each real render submission; a failed capacity gate remains
    /// evidence, rather than suppressing the commands needed to diagnose it.
    pub(super) fn step(&mut self, state: &mut AppState, limit: u32) -> Result<bool> {
        // Nonblocking dispatch of completion callbacks; delayed callbacks only
        // overestimate latency, unlike CPU submission timestamps.
        let _ = state.renderer.gpu.device.poll(wgpu::PollType::Poll);
        let now = ObservationTime::now();
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("latency simulation")?
            .simulation;
        let frame = sim.session().binary_frame();
        if self.started.is_none() {
            self.started = Some(now);
            self.render_deferral_start = state.platform.frame_pacer.total_render_deferrals();
            self.start_frame = frame;
            state
                .match_state
                .match_diagnostics
                .begin_command_observation();
            match battle::prepare_latency_view(state) {
                Ok((actor, origin)) => {
                    self.actor = Some(actor);
                    self.origin = Some(origin);
                    state
                        .match_state
                        .match_diagnostics
                        .observe_actor_presentation(actor);
                    let input = &state.match_state.input;
                    self.prepared = Some(PreparedView {
                        at: ObservationTime::now(),
                        frame,
                        camera: (input.camera_x, input.camera_y),
                        zoom: input.zoom_level,
                        viewport: crate::app::input::camera::tactical_viewport_px(state),
                        first_draw: None,
                        maintained: true,
                    });
                }
                Err(error) => self.errors.push(format!("prepare latency view: {error:#}")),
            }
        }
        let elapsed = now
            .instant
            .saturating_duration_since(self.started.unwrap().instant)
            .as_millis() as u64;
        self.render_deferred_passes = state
            .platform
            .frame_pacer
            .total_render_deferrals()
            .saturating_sub(self.render_deferral_start);
        for observation in state
            .match_state
            .match_diagnostics
            .take_command_observations()?
        {
            self.observe_execution(observation);
        }
        if let Some(presented) = state.match_state.match_diagnostics.actor_presentation() {
            if let Some(preparation) = self.stop_preparations.last_mut() {
                preparation.observe(&presented);
            }
            if let Some(prepared) = self.prepared.as_mut() {
                prepared.observe_draw(&presented);
            }
            self.observe_presented(presented.clone());
            if self
                .latest_actor_draw
                .as_ref()
                .is_some_and(|last| last.frame < presented.frame)
            {
                self.previous_actor_draw = self.latest_actor_draw.clone();
            }
            self.latest_actor_draw = Some(presented);
        }
        if let Some(prepared) = self.prepared.as_mut() {
            let input = &state.match_state.input;
            if prepared.maintained
                && (prepared.camera != (input.camera_x, input.camera_y)
                    || prepared.zoom != input.zoom_level
                    || prepared.viewport != crate::app::input::camera::tactical_viewport_px(state))
            {
                prepared.maintained = false;
                self.errors
                    .push("prepared latency camera/zoom/viewport changed".into());
            }
            if self.warmup_started.is_none() && prepared.ready() {
                // The asynchronous completion may arrive after this step's
                // initial timestamp. Read the clock after observing it.
                self.warmup_started = Some(ObservationTime::now());
                self.start_frame = frame;
                self.recent.clear();
                self.last_sample = None;
                self.maximum_render_interval_us = 0;
                self.last_present = Some(now);
            }
        }
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("latency simulation")?
            .simulation;
        if let Some(entity) = self.actor.and_then(|actor| sim.entities().get(actor)) {
            self.movement_observed |= self
                .origin
                .is_some_and(|origin| origin != (entity.position.rx(), entity.position.ry()));
            self.navigation_clear_at_end =
                entity.navigation.nav_com().is_none() && entity.navigation.nav_queue().is_empty();
        }
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("latency transport")?;
        let fps = live.fixed_frames_per_second();
        let frontier = live.committed_frontier();
        let recovery = live.recovery_status(
            crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32,
            frame,
        );
        let interval = self.last_present.map(|last| {
            now.instant
                .saturating_duration_since(last.instant)
                .as_micros() as u64
        });
        self.maximum_render_interval_us =
            self.maximum_render_interval_us.max(interval.unwrap_or(0));
        self.last_present = Some(now);
        let sample_updated = self.last_sample.is_none_or(|last| {
            now.instant
                .saturating_duration_since(last.instant)
                .as_millis()
                >= 250
        });
        if sample_updated {
            let mut sample = self.timestamp(now, frame);
            sample["fps"] = json!(fps);
            sample["committed_frontier"] = json!(frontier);
            sample["frontier_gap"] = json!(frontier.saturating_sub(frame));
            sample["recovery"] = json!(format!("{recovery:?}"));
            sample["render_interval_us"] = json!(interval);
            sample["maximum_render_interval_us"] = json!(self.maximum_render_interval_us);
            sample["render_deferred_passes"] = json!(self.render_deferred_passes);
            sample["server_push"] = json!(live.sequenced_timing().is_some());
            if let Some(pacing) = state.platform.frame_pacer.online_pacing_status() {
                sample["pacing_reserve_us"] = json!(pacing.reserve_micros);
                sample["pacing_underruns"] = json!(pacing.underruns);
                sample["pacing_catching_up"] = json!(pacing.catching_up);
            }
            sample["pending_local_commands"] = json!(sim.pending_local_command_count());
            sample["pending_synchronized_commands"] =
                json!(sim.synchronized_network_command_count());
            let samples = if self.warmup.as_ref().is_some_and(|w| w["passed"] == true) {
                &mut self.samples
            } else {
                &mut self.startup_samples
            };
            if samples.len() < 4096 {
                samples.push(sample);
            }
            if self.warmup_started.is_some() {
                self.recent.push_back(ProgressPoint {
                    at: now.instant,
                    frame,
                    fps,
                    backlog: frontier.saturating_sub(frame),
                    recovering: recovery.is_some(),
                    maximum_render_interval_us: self.maximum_render_interval_us,
                });
            }
            while self.recent.get(1).is_some_and(|point| {
                now.instant.saturating_duration_since(point.at).as_millis()
                    >= u128::from(STABLE_WINDOW_MS)
            }) {
                self.recent.pop_front();
            }
            self.last_sample = Some(now);
            self.maximum_render_interval_us = 0;
        }
        let warmup_elapsed = self.warmup_started.map(|start| {
            now.instant
                .saturating_duration_since(start.instant)
                .as_millis() as u64
        });
        if self.warmup.is_none()
            && let Some((keeping_up, recent_fps, window_ms)) = warmup_elapsed
                .and_then(|elapsed| warmup_decision(&self.recent, elapsed, sample_updated))
        {
            let elapsed = warmup_elapsed.unwrap();
            let keeping_up = keeping_up && self.errors.is_empty();
            self.warmup = Some(json!({"minimum_ms":WARMUP_MS,"maximum_ms":WARMUP_LIMIT_MS,
                    "elapsed_ms":elapsed,"passed":keeping_up,"frames_advanced":frame.saturating_sub(self.start_frame),
                    "maximum_render_gap_us":MAX_RENDER_GAP_US,
                    "recent_maximum_render_gap_us":self.recent.iter().map(|point|point.maximum_render_interval_us).max(),
                    "recent_window_ms":window_ms,"recent_fps":recent_fps,"fps":fps,
                    "frontier_gap":frontier.saturating_sub(frame),"recovery":format!("{recovery:?}")}));
            if !keeping_up {
                self.errors.push("prepared view did not sustain negotiated cadence, low backlog, and render gaps <=100ms for two seconds within 30000ms".into());
            }
        } else if self.warmup.is_none()
            && self.warmup_started.is_none()
            && elapsed >= WARMUP_LIMIT_MS
        {
            self.warmup = Some(json!({"passed":false,"elapsed_ms":elapsed,
                "maximum_ms":WARMUP_LIMIT_MS,"reason":"prepared actor was not rendered and GPU-completed"}));
            self.errors
                .push("prepared actor was not rendered and GPU-completed within 30000ms".into());
        }
        if let Some(action) = self.actions.last_mut() {
            if !action_gpu_complete(action)
                && action.error.is_none()
                && now
                    .instant
                    .saturating_duration_since(action.issue.instant)
                    .as_millis()
                    >= u128::from(ACTION_TIMEOUT_MS)
            {
                action.error = Some("no submitted response within 5000ms");
                log::info!(
                    "Latency action timeout: kind={} actor={} issue_unix_us={} dispatch_unix_us={:?}",
                    action.kind,
                    action.actor,
                    action.issue.unix_us,
                    action
                        .execution
                        .map(|index| self.executions[index].observed.observed_at.unix_us)
                );
            }
        }
        if let Some(action) = self.actions.last_mut()
            && !action.completion_logged
            && action_gpu_complete(action)
        {
            let response = action.response.as_ref().unwrap();
            let gpu = response.gpu_completed_at().unwrap();
            let dispatch = self.executions[action.execution.unwrap()]
                .observed
                .observed_at;
            log::info!(
                "Latency action complete: kind={} actor={} issue_unix_us={} dispatch_unix_us={} submitted_unix_us={} gpu_unix_us={} dispatch_ms={:.3} submitted_ms={:.3} gpu_ms={:.3}",
                action.kind,
                action.actor,
                action.issue.unix_us,
                dispatch.unix_us,
                response.at.unix_us,
                gpu.unix_us,
                dispatch
                    .instant
                    .saturating_duration_since(action.issue.instant)
                    .as_secs_f64()
                    * 1000.0,
                response
                    .at
                    .instant
                    .saturating_duration_since(action.issue.instant)
                    .as_secs_f64()
                    * 1000.0,
                gpu.instant
                    .saturating_duration_since(action.issue.instant)
                    .as_secs_f64()
                    * 1000.0
            );
            action.completion_logged = true;
        }
        // The requested prefix is a bound, even when capacity or rendering is
        // insufficient. Persist all partial observations and use normal exit.
        if frame >= limit {
            self.finished = true;
            state
                .match_state
                .match_diagnostics
                .end_command_observation();
            state
                .match_state
                .match_diagnostics
                .end_presentation_observation();
            return Ok(true);
        }
        if !self.warmup.as_ref().is_some_and(|w| w["passed"] == true) || !self.errors.is_empty() {
            return Ok(false);
        }
        if let Some(action) = self.actions.last() {
            if !action_gpu_complete(action) && action.error.is_none() {
                return Ok(false);
            }
            if action.kind == "Move" {
                let move_id = self.actions.len() - 1;
                if self
                    .stop_preparations
                    .last()
                    .is_none_or(|preparation| preparation.move_action_id != move_id)
                {
                    self.stop_preparations
                        .push(StopPreparation::new(move_id, now, frame));
                }
                let preparation = self.stop_preparations.last_mut().unwrap();
                if !preparation.poll(now) {
                    if preparation.failed {
                        self.errors.push(format!("Move action {move_id} did not reach fresh rendered stable motion at fraction >=0.5 within 5000ms; Stop was not issued"));
                    }
                    return Ok(false);
                }
            } else if self.next_issue.is_none() {
                self.next_issue = Some(now.instant + std::time::Duration::from_millis(250));
            }
        }
        if self.actions.len() >= TARGET_PAIRS * 2
            || self
                .next_issue
                .is_some_and(|deadline| now.instant < deadline)
        {
            return Ok(false);
        }
        let kind = if self.actions.len() % 2 == 0 {
            "Move"
        } else {
            "Stop"
        };
        let result = self.issue(state, kind, frame);
        if let Err(error) = result {
            self.errors.push(format!("{error:#}"));
        }
        self.next_issue = None;
        Ok(false)
    }

    fn issue(&mut self, state: &mut AppState, kind: &'static str, frame: u32) -> Result<()> {
        let actor = self.actor.context("prepared latency actor")?;
        let (actor, target, input) = if kind == "Move" {
            let (target, at) = battle::latency_move_with_mouse(
                state,
                actor,
                self.origin.context("prepared latency origin")?,
                self.actions.len() / 2,
            )?;
            (actor, Some(target), at)
        } else {
            (
                actor,
                None,
                battle::latency_stop_with_keyboard(state, actor)?,
            )
        };
        let at = input.at;
        let stop_reaction = if kind == "Stop" {
            StopReaction::from_prepared_move(
                self.stop_preparations
                    .last()
                    .filter(|preparation| preparation.move_action_id + 1 == self.actions.len()),
                at,
            )
        } else {
            StopReaction::default()
        };
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("latency simulation")?
            .simulation;
        let entity = sim.entities().get(actor).context("latency actor")?;
        let pose = crate::app::presentation::interpolation::pose(state, entity, frame);
        log::info!(
            "Latency action issued: kind={kind} actor={actor} issue_unix_us={} frame={frame}",
            at.unix_us
        );
        self.actions.push(Action {
            kind,
            actor,
            target,
            issue: at,
            issue_frame: frame,
            admission: input.admission,
            baseline: (pose.screen, pose.body),
            execution: None,
            response: None,
            quiet: None,
            error: None,
            completion_logged: false,
            stop_reaction,
        });
        Ok(())
    }

    fn execution_json(&self, execution: &Execution) -> Value {
        let observed = &execution.observed;
        let mut canonical = observed.record.clone();
        canonical.clear_processed();
        let record_hex: String = canonical
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut dispatch = self.timestamp(observed.observed_at, observed.frame);
        dispatch["applied"] = json!(observed.applied);
        json!({"execution_key":{"house":observed.record.house_id(),"opcode":observed.record.opcode(),
            "scheduled_frame":observed.record.frame_stamp(),"actor":observed.actor,"record_hex":record_hex},
            "kind":execution.kind,"target":execution.target,"dispatch":dispatch,
            "receiver":{"destination":observed.destination,"cell":observed.cell,"mission":observed.mission,"motion":observed.motion}})
    }

    pub(super) fn receipt(&self) -> Value {
        let actions: Vec<_> = self.actions.iter().enumerate().map(|(id, action)| {
            let execution = action.execution.map(|index| self.execution_json(&self.executions[index]));
            let latency = |at: ObservationTime| at.instant.saturating_duration_since(action.issue.instant).as_secs_f64() * 1000.0;
            let dispatch_ms = action.execution.map(|index| latency(self.executions[index].observed.observed_at));
            let response_ms = action.response.as_ref().map(|response| latency(response.at));
            let submitted = action.response.as_ref().map(|response| {
                let mut value = self.timestamp(response.at, response.frame);
                value["screen"] = json!(response.screen); value["body"] = json!(response.body); value
            });
            let gpu = action.response.as_ref().and_then(|response| response.gpu_completed_at().map(|at| (at,response.frame)));
            let gpu_ms = gpu.map(|(at,_)|latency(at));
            let first_stop_response = action.stop_reaction.first.as_ref().map(|first| {
                let mut submitted = self.timestamp(first.at, first.frame);
                submitted["motion"] = json!(first.motion);
                submitted["sampled_from_frame"] = json!(first.sampled_from_frame);
                json!({"criterion":action.stop_reaction.criterion,"submitted":submitted,
                    "gpu":first.gpu_completed_at().map(|at|self.timestamp(at,first.frame)),
                    "submitted_ms":latency(first.at),"gpu_ms":first.gpu_completed_at().map(latency)})
            });
            let stop_baseline = action.stop_reaction.baseline.as_ref().map(|draw|json!({
                "at":self.timestamp(draw.at,draw.frame),"motion":draw.motion}));
            json!({"id":id,"kind":action.kind,"actor":action.actor,"target":action.target,
                "issue":self.timestamp(action.issue,action.issue_frame),"admission":action.admission,
                "input_pose":{"screen":action.baseline.0,"body":action.baseline.1},
                "execution_key":execution.as_ref().map(|value|&value["execution_key"]),
                "dispatch":execution.as_ref().map(|value|&value["dispatch"]),
                "submitted_response":submitted,"gpu_response":gpu.map(|(at,frame)|self.timestamp(at,frame)),"error":action.error,
                "first_stop_response":first_stop_response,"stop_baseline":stop_baseline,
                "stop_prepared_movement":action.stop_reaction.prepared_draw.as_ref().map(|draw|json!({
                    "at":self.timestamp(draw.at,draw.frame),"screen":draw.screen,"motion":draw.motion})),
                "stop_rendered_travel_before":action.stop_reaction.rendered_travel_before,
                "dispatch_motion":action.execution.and_then(|index|self.executions[index].observed.motion.as_ref()),
                "stop_response_status":(action.kind=="Stop").then_some(if action.stop_reaction.no_op {"no_op"} else if action.stop_reaction.first.is_some() {"observed"} else {"unproven"}),
                "latency_ms":{"dispatch":dispatch_ms,"submitted_response":response_ms,"gpu_response":gpu_ms},
                "exceeds_500ms":{"dispatch":dispatch_ms.map(|ms|ms>500.0),"submitted_response":response_ms.map(|ms|ms>500.0),"gpu_response":gpu_ms.map(|ms|ms>500.0)}})
        }).collect();
        let completed_pairs = self
            .actions
            .chunks_exact(2)
            .filter(|pair| {
                pair.iter()
                    .all(|a| action_gpu_complete(a) && a.error.is_none())
            })
            .count();
        let complete = completed_pairs == TARGET_PAIRS && self.errors.is_empty();
        let first_move_trace: Vec<_> = self
            .first_move_trace
            .iter()
            .map(|draw| {
                json!({
                    "submitted":self.timestamp(draw.at,draw.frame),
                    "gpu":draw.gpu_completed_at().map(|at|self.timestamp(at,draw.frame)),
                    "screen":draw.screen,"body":draw.body,"motion":draw.motion,
                    "sampled_from_frame":draw.sampled_from_frame
                })
            })
            .collect();
        let preparation = self.prepared.as_ref().map(|prepared| json!({
            "mode":"fixed_view_selected_actor", "actor":self.actor,"origin":self.origin,
            "waypoint_policy":"six-cell offsets from the initial actor origin; only visible legal goals",
            "selected_at":self.timestamp(prepared.at,prepared.frame),
            "camera":prepared.camera,"zoom":prepared.zoom,"viewport":prepared.viewport,
            "fixed_view_maintained":prepared.maintained,
            "first_actor_submitted":prepared.first_draw.as_ref().map(|draw|self.timestamp(draw.at,draw.frame)),
            "first_actor_gpu":prepared.first_draw.as_ref().and_then(|draw|draw.gpu_completed_at().map(|at|self.timestamp(at,draw.frame))),
            "warmup_started":self.warmup_started.map(|at|self.timestamp(at,self.start_frame))
        }));
        let stop_preparations: Vec<_> = self.stop_preparations.iter().map(|preparation|json!({
            "move_action_id":preparation.move_action_id,"next_stop_action_id":preparation.move_action_id+1,
            "status":if preparation.ready.is_some() {"ready"} else if preparation.failed {"failed"} else {"waiting"},
            "started":self.timestamp(preparation.started,preparation.started_frame),
            "ready":preparation.ready.map(|at|self.timestamp(at,preparation.latest.as_ref().unwrap().frame)),
            "wait_ms":preparation.checked_at.instant.saturating_duration_since(preparation.started.instant).as_secs_f64()*1000.0,
            "minimum_delay_ms":STOP_PREPARATION_MIN_MS,"maximum_wait_ms":STOP_PREPARATION_LIMIT_MS,
            "probe_minimum_fraction":STOP_MIN_FRACTION,"minimum_stable_ms":STOP_STEADY_MS,
            "distinct_steady_draws":preparation.steady_draws,
            "steady_since":preparation.steady_since.as_ref().map(|draw|self.timestamp(draw.at,draw.frame)),
            "latest_draw":preparation.latest.as_ref().map(|draw|json!({"at":self.timestamp(draw.at,draw.frame),
                "screen":draw.screen,"motion":draw.motion})),
            "criterion":"at least three translating admitted Drive draws on distinct advancing logical frames; unchanged applied fraction >=0.5 for 100ms; latest draw <=100ms old; consecutive draw gaps <=100ms"
        })).collect();
        json!({"schema":"vera20k.online-input-latency.v1","target_pairs":TARGET_PAIRS,
            "render_deferred_passes":self.render_deferred_passes,
            "first_move_trace":first_move_trace,
            "completed_pairs":completed_pairs,"status":if complete {"complete"} else if self.finished {"incomplete"} else {"sampling"},
            "limitation":"submitted admitted Unit draw-plan geometry; GPU callback is a completion upper bound, not monitor scanout; remote observations are dispatch only",
            "clock":{"unix_unit":"microseconds","mono_unit":"microseconds since this round observer began","remote_requires_calibration":true},
            "preparation":preparation,"stop_preparations":stop_preparations,"warmup":self.warmup,"actions":actions,"executions":self.executions.iter().map(|e|self.execution_json(e)).collect::<Vec<_>>(),
            "keep_up":self.samples,"startup_keep_up":self.startup_samples,"peer_round_trip_ms":self.peer_rtt,"peer_round_trip_scope":"one-shot startup peer RTT; 16ms buckets; separate from command latency",
            "validation":{"movement_observed":self.movement_observed,"navigation_clear_at_end":self.navigation_clear_at_end,
                "last_stop_submitted":self.actions.last().is_some_and(|a|a.kind=="Stop" && a.response.is_some())},
            "drive_target_fraction":"unavailable through public core getters; drive_destination is the coordinate target",
            "errors":self.errors})
    }
}

/// The last actual pre-dispatch draw and the immediate post-dispatch speed
/// are independent baselines. A lowered logical speed alone is insufficient:
/// every endpoint contributing to the admitted draw must include that change.
fn observe_stop_reaction(
    reaction: &mut StopReaction,
    execution: &CommandExecutionObservation,
    draw: &ActorPresentation,
) {
    if reaction.no_op
        || !reaction.rendered_travel_before
        || reaction.first.is_some()
        || !execution.applied
        || draw.at.instant < execution.observed_at.instant
    {
        return;
    }
    let (Some(before), Some(after)) = (reaction.baseline.as_ref(), execution.motion.as_ref())
    else {
        return;
    };
    if before.motion.locomotor.as_deref() != Some("Drive")
        || after.locomotor.as_deref() != Some("Drive")
        || draw.motion.locomotor.as_deref() != Some("Drive")
        || after.drive_destination.is_some()
        || draw.motion.drive_destination.is_some()
        || before.motion.moving != Some(true)
    {
        return;
    }
    let speed = draw.motion.applied_fraction;
    if !speed.is_finite()
        || speed < 0.0
        || speed >= before.motion.applied_fraction
        || speed >= after.applied_fraction
        || draw.frame <= execution.frame
    {
        return;
    }
    let first_lowered = *reaction.deceleration_frame.get_or_insert(draw.frame);
    if draw.sampled_from_frame >= first_lowered {
        reaction.first = Some(draw.clone());
        reaction.criterion = Some(if draw.motion.moving == Some(false) {
            "settled_stop_on_post_deceleration_draw"
        } else {
            "lower_applied_speed_with_post_deceleration_endpoints"
        });
    }
}

/// A catch-up burst is not sustainable cadence. Require a full recent window,
/// negotiated rate in both directions, and no reported backlog/recovery sample.
fn warmup_decision(
    points: &std::collections::VecDeque<ProgressPoint>,
    elapsed_ms: u64,
    sample_updated: bool,
) -> Option<(bool, Option<f64>, u64)> {
    let timed_out = elapsed_ms >= WARMUP_LIMIT_MS;
    if elapsed_ms < WARMUP_MS || (!sample_updated && !timed_out) {
        return None;
    }
    let (steady, fps, window_ms) = stable_window(points);
    // Only a newly sampled window contains this draw's gap and current
    // frontier/recovery state. A deadline still terminates a stale window.
    let ready = sample_updated && steady;
    (ready || timed_out).then_some((ready, fps, window_ms))
}

fn stable_window(points: &std::collections::VecDeque<ProgressPoint>) -> (bool, Option<f64>, u64) {
    let (Some(first), Some(last)) = (points.front(), points.back()) else {
        return (false, None, 0);
    };
    let ms = last.at.saturating_duration_since(first.at).as_millis() as u64;
    if ms < STABLE_WINDOW_MS {
        return (false, None, ms);
    }
    let actual = f64::from(last.frame.saturating_sub(first.frame)) * 1000.0 / ms as f64;
    let ready = last.fps.is_some_and(|fps| {
        fps > 0
            && actual >= f64::from(fps) * 0.9
            && actual <= f64::from(fps) * 1.1
            && points.iter().all(|point| {
                point.fps == Some(fps)
                    && point.backlog <= 2
                    && !point.recovering
                    && point.maximum_render_interval_us <= MAX_RENDER_GAP_US
            })
    });
    (ready, Some(actual), ms)
}

fn action_gpu_complete(action: &Action) -> bool {
    action
        .response
        .as_ref()
        .is_some_and(|response| response.gpu_completed_at().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::command::{CommandRecord, IDLE_OPCODE};

    fn test_motion(
        fraction: f64,
        moving: bool,
    ) -> crate::app::match_diagnostics::ActorMotionObservation {
        let mut sim = crate::sim::world::Simulation::new();
        let owner = sim.intern("Human");
        let typ = sim.intern("MTNK");
        let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
            7,
            10,
            10,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(100),
            typ,
            crate::map::entities::EntityCategory::Unit,
            0,
            5,
            false,
        );
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        let loco = entity.locomotor.as_mut().unwrap();
        loco.ensure_installed_track_state();
        loco.store_track_destination(
            crate::sim::movement::track_process::TrackFamily::Drive,
            moving.then(|| crate::sim::components::DriveCoord::cell(14, 10, 0)),
        );
        entity
            .foot_speed
            .set_speed_fraction_native_bits(fraction.to_bits());
        crate::app::match_diagnostics::ActorMotionObservation::read(&sim, &entity)
    }

    #[test]
    fn prepared_stop_motion_survives_one_unchanged_predispatch_pose() {
        let start = ObservationTime::now();
        let draw = |ms, frame, x, speed, moving| ActorPresentation {
            at: at(start, ms),
            frame,
            screen: [x, 0.],
            body: 0,
            motion: test_motion(speed, moving),
            sampled_from_frame: frame,
            gpu_done: Default::default(),
        };
        let mut preparation = StopPreparation::new(0, start, 0);
        preparation.observe(&draw(350, 10, 1., 1., true));
        preparation.observe(&draw(400, 13, 2., 1., true));
        preparation.observe(&draw(450, 16, 3., 1., true));
        assert!(preparation.poll(at(start, 450)));
        let mut action = action(at(start, 460));
        action.stop_reaction = StopReaction::from_prepared_move(Some(&preparation), action.issue);
        assert!(action.stop_reaction.prepared_draw.is_some());
        assert!(
            StopReaction::from_prepared_move(Some(&preparation), at(start, 551))
                .prepared_draw
                .is_none()
        );
        let mut capture = LatencyCapture {
            started: Some(start),
            previous_actor_draw: Some(draw(500, 19, 42., 1., true)),
            latest_actor_draw: Some(draw(520, 20, 42., 1., true)),
            ..Default::default()
        };
        capture.actions.push(action);
        let mut execution = stop(start, 550, 7, 22);
        execution.motion = Some(test_motion(1., false));
        capture.observe_execution(execution);
        assert!(capture.actions[0].stop_reaction.rendered_travel_before);
        capture.observe_presented(draw(560, 23, 42., 1., false));
        assert!(
            capture.actions[0].stop_reaction.first.is_none(),
            "unchanged speed is insufficient"
        );
        let mut blended = draw(580, 24, 43., 0.8, false);
        blended.sampled_from_frame = 23;
        capture.observe_presented(blended);
        assert!(
            capture.actions[0].stop_reaction.first.is_none(),
            "prior endpoint predates deceleration"
        );
        let mut admitted = draw(610, 25, 44., 0.8, false);
        admitted.sampled_from_frame = 24;
        let completed = admitted.gpu_done.clone();
        capture.observe_presented(admitted);
        let first = capture.actions[0].stop_reaction.first.as_ref().unwrap();
        assert!(first.gpu_completed_at().is_none());
        *completed.lock().unwrap() = Some(at(start, 620));
        assert_eq!(
            first.gpu_completed_at().unwrap().unix_us,
            at(start, 620).unix_us
        );
    }

    #[test]
    fn first_move_trace_is_bounded_to_distinct_draws_of_the_first_action() {
        let start = ObservationTime::now();
        let mut capture = LatencyCapture {
            started: Some(start),
            ..Default::default()
        };
        let mut first = action(start);
        first.kind = "Move";
        capture.actions.push(first);
        for frame in 0..65 {
            let draw = ActorPresentation {
                at: at(start, u64::from(frame)),
                frame,
                screen: [0., 0.],
                body: 0,
                motion: test_motion(0.0, false),
                sampled_from_frame: frame,
                gpu_done: Default::default(),
            };
            capture.observe_presented(draw.clone());
            capture.observe_presented(draw);
        }
        assert_eq!(capture.first_move_trace.len(), 64);
        assert_eq!(
            capture.receipt()["first_move_trace"][63]["submitted"]["frame"],
            63
        );
        capture.first_move_trace.clear();
        capture.actions.push(action(start));
        capture.observe_presented(ActorPresentation {
            at: start,
            frame: 70,
            screen: [0., 0.],
            body: 0,
            motion: test_motion(0.0, false),
            sampled_from_frame: 70,
            gpu_done: Default::default(),
        });
        assert!(capture.first_move_trace.is_empty());
    }

    #[test]
    fn stop_evidence_uses_wall_time_when_rendering_skips_logical_frames() {
        let start = ObservationTime::now();
        let draw = |ms: u64, frame: u32| ActorPresentation {
            at: at(start, 350 + ms),
            frame,
            screen: [frame as f32, 0.],
            body: 0,
            motion: test_motion(0.7, true),
            sampled_from_frame: frame.saturating_sub(1),
            gpu_done: Default::default(),
        };
        let mut preparation = StopPreparation::new(0, start, 0);
        preparation.observe(&draw(0, 0));
        preparation.observe(&draw(67, 3));
        preparation.observe(&draw(134, 6));
        assert!(
            preparation.poll(at(start, 484)),
            "45Hz simulation shown at15Hz is valid steady motion"
        );

        let mut gap = StopPreparation::new(0, start, 0);
        gap.observe(&draw(0, 0));
        gap.observe(&draw(101, 5));
        gap.observe(&draw(168, 8));
        assert!(
            !gap.poll(at(start, 518)),
            "a render gap over100ms breaks the steady window"
        );
        let mut stale = StopPreparation::new(0, start, 0);
        stale.observe(&draw(0, 0));
        stale.observe(&draw(67, 3));
        stale.observe(&draw(134, 6));
        assert!(
            !stale.poll(at(start, 585)),
            "a stale latest draw is insufficient"
        );

        let mut capture = LatencyCapture {
            started: Some(start),
            previous_actor_draw: Some(draw(67, 3)),
            latest_actor_draw: Some(draw(134, 6)),
            ..Default::default()
        };
        capture.actions.push(action(start));
        capture.observe_execution(stop(start, 550, 7, 9));
        assert!(capture.actions[0].stop_reaction.rendered_travel_before);
        capture.actions.push(action(at(start, 600)));
        capture.observe_execution(stop(start, 650, 7, 12));
        assert!(!capture.actions[1].stop_reaction.rendered_travel_before);
        assert!(
            capture.actions[1].stop_reaction.baseline.is_none(),
            "pre-dispatch draw must be at most100ms old"
        );
    }

    #[test]
    fn stop_preparation_requires_fresh_stable_translation_and_has_a_deadline() {
        let start = ObservationTime::now();
        let draw = |ms, frame, x, fraction| ActorPresentation {
            at: at(start, ms),
            frame,
            screen: [x, 0.],
            body: 0,
            motion: test_motion(fraction, true),
            sampled_from_frame: frame - 1,
            gpu_done: Default::default(),
        };
        for (name, fractions, positions) in [
            ("too slow", [0.3, 0.3, 0.3], [1., 2., 3.]),
            ("accelerating", [0.5, 0.6, 0.7], [1., 2., 3.]),
            ("not translating", [0.7, 0.7, 0.7], [1., 1., 1.]),
        ] {
            let mut preparation = StopPreparation::new(0, start, 0);
            preparation.observe(&draw(350, 10, positions[0], fractions[0]));
            preparation.observe(&draw(400, 12, positions[1], fractions[1]));
            preparation.observe(&draw(450, 14, positions[2], fractions[2]));
            assert!(!preparation.poll(at(start, 450)), "{name}");
        }
        let mut stable = StopPreparation::new(0, start, 0);
        stable.observe(&draw(350, 10, 1., 0.7));
        stable.observe(&draw(400, 12, 2., 0.7));
        stable.observe(&draw(450, 14, 3., 0.7));
        assert!(
            !stable.poll(at(start, 551)),
            "old rendered motion cannot qualify"
        );
        stable.observe(&draw(500, 16, 4., 0.7));
        assert!(stable.poll(at(start, 551)));
        assert_eq!(stable.ready.unwrap().unix_us, at(start, 551).unix_us);
        let mut timeout = StopPreparation::new(2, start, 0);
        timeout.observe(&draw(4_900, 100, 1., 0.7));
        timeout.observe(&draw(4_950, 102, 2., 0.7));
        timeout.observe(&draw(5_000, 104, 3., 0.7));
        assert!(
            !timeout.poll(at(start, 5_001)),
            "late readiness cannot extend the bound"
        );
        assert!(timeout.failed);
        assert!(timeout.ready.is_none());
    }

    #[test]
    fn stop_braking_requires_fresh_speed_drop_and_post_deceleration_draw_endpoints() {
        let start = ObservationTime::now();
        let draw = |ms, frame, from, speed| ActorPresentation {
            at: at(start, ms),
            frame,
            screen: [frame as f32, 0.],
            body: 0,
            motion: test_motion(speed, false),
            sampled_from_frame: from,
            gpu_done: Default::default(),
        };
        let mut baseline = draw(100, 10, 9, 0.8);
        baseline.motion = test_motion(0.8, true);
        let mut reaction = StopReaction {
            baseline: Some(baseline),
            rendered_travel_before: true,
            ..Default::default()
        };
        let mut execution = stop(start, 200, 7, 12);
        execution.motion = Some(test_motion(0.9, false));
        observe_stop_reaction(&mut reaction, &execution, &draw(250, 13, 12, 0.9));
        observe_stop_reaction(&mut reaction, &execution, &draw(270, 14, 13, 0.85));
        assert!(
            reaction.first.is_none(),
            "unchanged speed or only lower than transit speed is insufficient"
        );
        observe_stop_reaction(&mut reaction, &execution, &draw(300, 15, 14, 0.7));
        assert!(
            reaction.first.is_none(),
            "the blended prior endpoint predates observed deceleration"
        );
        let first = draw(340, 16, 15, 0.7);
        let completed = first.gpu_done.clone();
        observe_stop_reaction(&mut reaction, &execution, &first);
        let observed = reaction.first.as_ref().unwrap();
        assert_eq!(observed.at.unix_us, at(start, 340).unix_us);
        assert!(observed.gpu_completed_at().is_none());
        *completed.lock().unwrap() = Some(at(start, 355));
        assert_eq!(
            observed.gpu_completed_at().unwrap().unix_us,
            at(start, 355).unix_us
        );
    }

    #[test]
    fn stop_at_rest_never_becomes_a_braking_response() {
        let start = ObservationTime::now();
        let rest = ActorPresentation {
            at: at(start, 150),
            frame: 10,
            screen: [0., 0.],
            body: 0,
            motion: test_motion(0.0, false),
            sampled_from_frame: 10,
            gpu_done: Default::default(),
        };
        let mut capture = LatencyCapture {
            started: Some(start),
            latest_actor_draw: Some(rest.clone()),
            ..Default::default()
        };
        capture.actions.push(action(start));
        let mut execution = stop(start, 200, 7, 12);
        execution.motion = Some(test_motion(0.0, false));
        capture.observe_execution(execution);
        assert!(capture.actions[0].stop_reaction.no_op);
        let mut later = rest;
        later.frame = 30;
        later.at = at(start, 500);
        capture.observe_presented(later);
        assert!(capture.actions[0].stop_reaction.first.is_none());
        assert_eq!(
            capture.receipt()["actions"][0]["stop_response_status"],
            "no_op"
        );
    }

    #[test]
    fn prepared_view_needs_a_postselection_gpu_completed_draw() {
        let start = ObservationTime::now();
        let mut view = PreparedView {
            at: at(start, 100),
            frame: 5,
            camera: (0., 0.),
            zoom: 1.,
            viewport: (0, 0, 472, 448),
            first_draw: None,
            maintained: true,
        };
        assert!(!view.ready());
        let mut draw = ActorPresentation {
            at: start,
            frame: 4,
            screen: [0., 0.],
            body: 0,
            motion: test_motion(0.0, false),
            sampled_from_frame: 4,
            gpu_done: Default::default(),
        };
        view.observe_draw(&draw);
        assert!(
            view.first_draw.is_none(),
            "old view does not satisfy preparation"
        );
        draw.at = at(start, 120);
        draw.frame = 6;
        view.observe_draw(&draw);
        assert!(!view.ready(), "CPU submission alone is insufficient");
        *draw.gpu_done.lock().unwrap() = Some(at(start, 140));
        assert!(view.ready());
        view.maintained = false;
        assert!(!view.ready(), "changed camera invalidates preparation");
    }

    #[test]
    fn warmup_does_not_pass_on_stale_samples_between_render_gaps() {
        let start = crate::util::time::Instant::now();
        let mut points = std::collections::VecDeque::from([
            ProgressPoint {
                at: start,
                frame: 100,
                fps: Some(45),
                backlog: 0,
                recovering: false,
                maximum_render_interval_us: 22_222,
            },
            ProgressPoint {
                at: start + std::time::Duration::from_secs(2),
                frame: 190,
                fps: Some(45),
                backlog: 0,
                recovering: false,
                maximum_render_interval_us: 22_222,
            },
        ]);
        assert!(
            warmup_decision(&points, 5_020, false).is_none(),
            "a new draw gap is not yet represented by the old good samples"
        );
        points[1].maximum_render_interval_us = 130_000;
        assert!(warmup_decision(&points, 5_050, true).is_none());
        points[1].maximum_render_interval_us = 22_222;
        assert!(warmup_decision(&points, 5_050, true).unwrap().0);
        assert!(
            !warmup_decision(&points, 30_000, false).unwrap().0,
            "the deadline ends an unsampled interval without calling it ready"
        );
    }

    #[test]
    fn warmup_requires_recent_sustainable_cadence_without_backlog_or_recovery() {
        let start = crate::util::time::Instant::now();
        let mut points = std::collections::VecDeque::from([
            ProgressPoint {
                at: start,
                frame: 100,
                fps: Some(45),
                backlog: 0,
                recovering: false,
                maximum_render_interval_us: 22_222,
            },
            ProgressPoint {
                at: start + std::time::Duration::from_secs(2),
                frame: 220,
                fps: Some(45),
                backlog: 0,
                recovering: false,
                maximum_render_interval_us: 22_222,
            },
        ]);
        assert!(
            !stable_window(&points).0,
            "catch-up faster than negotiated cadence is not ready"
        );
        points[1].frame = 190;
        points[1].backlog = 28;
        assert!(!stable_window(&points).0);
        points[1].backlog = 0;
        points[1].recovering = true;
        assert!(!stable_window(&points).0);
        points[1].recovering = false;
        assert!(stable_window(&points).0);
        points[1].maximum_render_interval_us = 100_001;
        assert!(
            !stable_window(&points).0,
            "a render stall invalidates readiness"
        );
        points[1].maximum_render_interval_us = 100_000;
        assert!(stable_window(&points).0);
        points[1].at = start + std::time::Duration::from_secs(1);
        assert!(
            !stable_window(&points).0,
            "the full recent window is required"
        );
    }

    fn at(base: ObservationTime, ms: u64) -> ObservationTime {
        ObservationTime {
            instant: base.instant + std::time::Duration::from_millis(ms),
            unix_us: base.unix_us + ms * 1000,
        }
    }
    fn stop(base: ObservationTime, ms: u64, actor: u64, frame: u32) -> CommandExecutionObservation {
        let mut record = CommandRecord::encode(IDLE_OPCODE, 0, frame as i32, &[]).unwrap();
        StopRecord {
            house_id: 0,
            frame: frame as i32,
            entity_id: actor as i32,
        }
        .write_into(&mut record);
        CommandExecutionObservation {
            observed_at: at(base, ms),
            frame,
            record,
            applied: true,
            actor: Some(actor),
            timing: None,
            motion: None,
            destination: None,
            cell: Some((10, 10)),
            mission: Some((0, 0)),
        }
    }
    fn action(issue: ObservationTime) -> Action {
        Action {
            kind: "Stop",
            actor: 7,
            target: None,
            issue,
            issue_frame: 10,
            admission: json!({}),
            baseline: ([0., 0.], 0),
            execution: None,
            response: None,
            quiet: None,
            error: None,
            completion_logged: false,
            stop_reaction: StopReaction::default(),
        }
    }

    #[test]
    fn repeated_stops_keep_independent_canonical_executions_and_remote_records() {
        let start = ObservationTime::now();
        let mut capture = LatencyCapture {
            started: Some(start),
            ..Default::default()
        };
        capture.actions.push(action(start));
        capture.observe_execution(stop(start, 100, 99, 12));
        capture.observe_execution(stop(start, 200, 7, 13));
        capture.actions.push(action(at(start, 300)));
        capture.observe_execution(stop(start, 450, 7, 19));
        let receipt = capture.receipt();
        assert_eq!(receipt["executions"].as_array().unwrap().len(), 3);
        assert_eq!(
            receipt["actions"][0]["execution_key"]["scheduled_frame"],
            13
        );
        assert_eq!(
            receipt["actions"][1]["execution_key"]["scheduled_frame"],
            19
        );
        assert_eq!(receipt["actions"][1]["latency_ms"]["dispatch"], 150.0);
        assert_ne!(
            receipt["actions"][0]["execution_key"]["record_hex"],
            receipt["actions"][1]["execution_key"]["record_hex"]
        );
    }

    #[test]
    fn stop_response_uses_first_stationary_submission_after_dispatch() {
        let start = ObservationTime::now();
        let mut capture = LatencyCapture {
            started: Some(start),
            ..Default::default()
        };
        capture.actions.push(action(start));
        capture.observe_execution(stop(start, 200, 7, 13));
        let presentation = |ms, frame, x| ActorPresentation {
            at: at(start, ms),
            frame,
            screen: [x, 0.],
            body: 0,
            motion: test_motion(0.0, false),
            sampled_from_frame: frame,
            gpu_done: std::sync::Arc::new(std::sync::Mutex::new(Some(at(start, ms + 10)))),
        };
        capture.observe_presented(presentation(150, 12, 1.));
        assert!(capture.actions[0].quiet.is_none());
        capture.observe_presented(presentation(250, 14, 1.));
        let first_stationary = presentation(300, 15, 2.);
        let completion = first_stationary.gpu_done.clone();
        *completion.lock().unwrap() = None;
        capture.observe_presented(first_stationary);
        capture.observe_presented(presentation(420, 18, 2.));
        assert!(
            capture.actions[0].response.is_none(),
            "a three-frame pause is insufficient"
        );
        capture.observe_presented(presentation(650, 25, 2.));
        assert!(!action_gpu_complete(&capture.actions[0]));
        *completion.lock().unwrap() = Some(at(start, 310));
        assert!(action_gpu_complete(&capture.actions[0]));
        assert_eq!(
            capture.receipt()["actions"][0]["gpu_response"]["mono_us"],
            310_000
        );
        assert_eq!(
            capture.receipt()["actions"][0]["submitted_response"]["mono_us"],
            300_000
        );
        assert_eq!(
            capture.receipt()["actions"][0]["latency_ms"]["submitted_response"],
            300.0
        );
    }
}
