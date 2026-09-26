//! Production-route owner for the hidden tactical radar checkpoint.
//!
//! The session enters through accepted fixed-Battle startup, schedules only
//! ordinary commands, advances exactly one production step per pump, observes
//! the real renderer, and publishes one transactional final readback. It does
//! not focus the window, inject desktop input, or grant simulation state.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::app::AppState;
use crate::app::diagnostics::tactical_capture::evidence::{
    ArtifactEvidence, FinalFingerprint, GraphicsEvidence, SidebarRenderEvidence,
    SidebarSourceEvidence, build_evidence,
};
use crate::app::diagnostics::tactical_capture::manifest::{
    FrameArtifact, TacticalCaptureManifest, publish_complete, publish_failure,
};
use crate::app::diagnostics::tactical_capture::placement::first_valid_placement;
use crate::app::diagnostics::tactical_capture::profile::{
    CONTRACT_SCHEMA, EMBEDDED_CONTRACT, TacticalCaptureProfile, sha256_file, sha256_hex,
};
use crate::app::diagnostics::tactical_capture::script::{
    BuildOptionObservation, DeploymentContract, ProductionQueueObservation,
    ProductionTargetContract, StageBudget, StructureRole, TacticalAction,
    TacticalEntityObservation, TacticalExpectedLedger, TacticalObservation, TacticalScript,
    TacticalScriptConfig, TacticalScriptStage, TacticalStageBudgets,
};
use crate::app::frontend::launch::TacticalCaptureRequest;
use crate::app::presentation::render::GameRenderOutput;
use crate::app::types::{CursorId, SIM_TICK_MS};
use crate::match_bootstrap::{MatchSeedClock, MatchSeedSource, StartupSessionClassification};
use crate::render::radar_anim::RadarAnimPhase;
use crate::render::sidebar_chrome::SidebarTheme;
use crate::sim::command::{Command, QueueMode};
use crate::sim::house_state::HouseDifficulty;
use crate::sim::production;
use crate::ui::game_screen::GameScreen;

const PRODUCTION_PROGRESS_INTERVALS: u64 = 53;
const CAPTURE_PUMP_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug, Clone)]
struct RuntimeInputs {
    config: ArtifactEvidence,
    executable: ArtifactEvidence,
    archive: ArtifactEvidence,
    font: ArtifactEvidence,
}

#[derive(Debug, Clone, Copy)]
struct ControlledSeedClock(u32);

impl MatchSeedClock for ControlledSeedClock {
    fn low_u32(&mut self) -> u32 {
        self.0
    }

    fn source(&self) -> MatchSeedSource {
        MatchSeedSource::Controlled
    }

    fn seed_authority_certifying(&self) -> bool {
        true
    }
}

/// Hidden tactical capture state retained across winit pumps.
pub(crate) struct TacticalCaptureSession {
    request: TacticalCaptureRequest,
    started_at: Instant,
    post_l0_started_at: Option<Instant>,
    inputs: Option<RuntimeInputs>,
    map_source_evidence: Option<Value>,
    startup_evidence: Option<Value>,
    script: Option<TacticalScript>,
    exact_step_receipts: Vec<crate::app::match_runtime::sim_tick::ExactStepReceipt>,
    last_render_ready: bool,
    last_render_evidence: Option<Value>,
    capture_requested: bool,
    readback_started: bool,
    fingerprint_before_readback: Option<Value>,
    render_frames: u64,
    focus_violations: u32,
    input_violations: u32,
    failure_stage: String,
    outcome: Option<std::result::Result<(), String>>,
}

impl TacticalCaptureSession {
    pub(crate) fn new(request: TacticalCaptureRequest) -> Self {
        Self {
            request,
            started_at: Instant::now(),
            post_l0_started_at: None,
            inputs: None,
            map_source_evidence: None,
            startup_evidence: None,
            script: None,
            exact_step_receipts: Vec::new(),
            last_render_ready: false,
            last_render_evidence: None,
            capture_requested: false,
            readback_started: false,
            fingerprint_before_readback: None,
            render_frames: 0,
            focus_violations: 0,
            input_violations: 0,
            failure_stage: "initialization".to_owned(),
            outcome: None,
        }
    }

    pub(crate) fn request(&self) -> &TacticalCaptureRequest {
        &self.request
    }

    /// Seal process inputs, verify hidden/no-input launch facts, and enter the
    /// ordinary accepted fixed-Battle loading path with a controlled seed.
    pub(crate) fn prepare_state(&mut self, state: &mut AppState) -> Result<()> {
        self.failure_stage = "preflight".to_owned();
        self.request.validate_runtime_environment()?;
        ensure!(
            state.platform.window.is_visible() == Some(false),
            "tactical capture window is not observably hidden"
        );
        ensure!(
            !state.platform.window.has_focus(),
            "tactical capture window unexpectedly owns focus"
        );

        let profile = self.request.profile();
        let capture = &profile.capture;
        ensure!(
            state.renderer.gpu.config.width == capture.output_width
                && state.renderer.gpu.config.height == capture.output_height,
            "tactical surface is {}x{}, expected {}x{}",
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
            capture.output_width,
            capture.output_height
        );
        ensure!(
            capture
                .surface_formats
                .contains(&format!("{:?}", state.renderer.gpu.config.format)),
            "tactical surface format {:?} is outside the sealed profile",
            state.renderer.gpu.config.format
        );
        ensure!(
            state
                .renderer.gpu
                .config
                .usage
                .contains(wgpu::TextureUsages::COPY_SRC),
            "tactical swapchain lacks COPY_SRC readback usage"
        );
        ensure!(
            (state.match_state.match_presentation.ui_scale - capture.app_ui_scale as f32).abs() <= f32::EPSILON,
            "app UI scale {} differs from sealed {}",
            state.match_state.match_presentation.ui_scale,
            capture.app_ui_scale
        );
        ensure!(
            state.match_state.configured_input_delay_ticks == u64::from(profile.launch.input_delay_ticks),
            "configured input delay differs from sealed launch"
        );

        let config = state
            .platform.game_config
            .as_ref()
            .context("tactical capture requires a loaded config.toml")?;
        ensure!(
            config.graphics.vsync == capture.vsync
                && config.graphics.upscale == capture.upscale
                && config.graphics.extra_animations == capture.extra_animations,
            "live graphics toggles differ from the sealed tactical profile"
        );
        ensure!(
            state.renderer.upscale_pass.is_none(),
            "tactical capture forbids the upscale pass"
        );
        ensure!(
            SIM_TICK_MS == capture.sim_tick_ms,
            "compiled simulation step {SIM_TICK_MS}ms differs from sealed {}ms",
            capture.sim_tick_ms
        );
        let cwd = std::env::current_dir().context("read tactical child working directory")?;
        ensure!(
            cwd.is_absolute(),
            "tactical child working directory is not absolute"
        );
        let config_path = cwd.join("config.toml");
        let executable_path =
            std::env::current_exe().context("resolve tactical child executable")?;
        let archive_path = config.paths.ra2_dir.join(&profile.fixture.archive_name);
        let font_path = profile.pixel_inputs.font.path.clone();

        let config_identity = artifact(&config_path, "config.toml")?;
        let executable_identity = artifact(&executable_path, "tactical executable")?;
        let archive_identity = artifact(&archive_path, "retail tactical archive")?;
        let font_identity = artifact(&font_path, "tactical system font")?;
        require_identity(
            &archive_identity,
            profile.fixture.archive_byte_length,
            &profile.fixture.archive_sha256,
            "retail archive",
        )?;
        require_identity(
            &font_identity,
            profile.pixel_inputs.font.byte_length,
            &profile.pixel_inputs.font.sha256,
            "system font",
        )?;
        reject_loose_shadow(&cwd.join(&profile.fixture.logical_map_name))?;
        reject_loose_shadow(&config.paths.ra2_dir.join(&profile.fixture.logical_map_name))?;

        self.inputs = Some(RuntimeInputs {
            config: config_identity,
            executable: executable_identity,
            archive: archive_identity,
            font: font_identity,
        });
        state.match_state.input.cursor_x = capture.post_load_cursor.x as f32;
        state.match_state.input.cursor_y = capture.post_load_cursor.y as f32;
        let now_ms = crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(
            state,
            std::time::Instant::now(),
        );
        state.platform.frame_pacer.reanchor(now_ms);
        self.begin_accepted_loading(state)?;
        self.failure_stage = "loading".to_owned();
        Ok(())
    }

    fn begin_accepted_loading(&mut self, state: &mut AppState) -> Result<()> {
        let launch = self.request.launch_session();
        let accepted = match crate::match_bootstrap::classify_startup_session(&launch) {
            StartupSessionClassification::AcceptedExplicitFixedBattle(accepted) => accepted,
            StartupSessionClassification::UnverifiedLegacy(reason) => {
                bail!("sealed tactical launch was not accepted: {reason:?}")
            }
        };
        let correlation = crate::match_bootstrap::allocate_match_correlation(
            &mut state.frontend.next_match_correlation,
        )
        .context("allocate tactical match correlation")?;
        let mut clock = ControlledSeedClock(self.request.profile().launch.seed);
        let startup =
            crate::match_bootstrap::prepare_match_startup(correlation, accepted, &mut clock);
        ensure!(
            startup.seed.value == self.request.profile().launch.seed
                && startup.seed.source == MatchSeedSource::Controlled
                && startup.seed.seed_authority_certifying,
            "controlled tactical seed did not survive startup preparation"
        );
        self.startup_evidence = Some(json!({
            "correlation": correlation.get(),
            "seed": startup.seed.value,
            "seed_source": "Controlled",
            "seed_authority_certifying": startup.seed.seed_authority_certifying,
            "classification": "AcceptedExplicitFixedBattle",
        }));

        state.frontend.skirmish_shell_state.pressed_owner_draw_button = None;
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
        state.frontend.shell_first_paint_slide = None;
        state.frontend.skirmish_preview_texture = None;
        let request = crate::app::loading::pump::LoadingRequest::accepted_skirmish(
            startup,
            state.frontend.skirmish_settings.clone(),
        );
        crate::app::loading::pump::begin_loading(state, request);
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
        Ok(())
    }

    /// Record and reject any attempt by the hidden capture window to gain focus.
    pub(crate) fn record_focus_violation(&mut self) {
        self.focus_violations = self.focus_violations.saturating_add(1);
        self.fail("hidden tactical capture window gained focus");
    }

    /// Record and reject all keyboard, pointer, gesture, IME, and touch input.
    pub(crate) fn record_input_violation(&mut self, kind: &str) {
        self.input_violations = self.input_violations.saturating_add(1);
        self.fail(format!("hidden tactical capture received {kind} input"));
    }

    /// Drive one condition-led script observation and, unless the script has
    /// frozen on capture, exactly one production simulation step.
    pub(crate) fn drive_before_render(&mut self, state: &mut AppState) -> Result<()> {
        ensure!(
            self.outcome.is_none(),
            "tactical session drove after terminal outcome"
        );
        self.check_process_timeout()?;
        if state.platform.window.has_focus() {
            self.record_focus_violation();
            bail!("hidden tactical capture window gained focus");
        }

        match state.frontend.screen {
            GameScreen::Loading => return Ok(()),
            GameScreen::InGame => {}
            _ if self.script.is_none() => {
                bail!("tactical startup left Loading before accepted InGame installation")
            }
            _ => bail!("tactical match left InGame before capture completion"),
        }

        if self.script.is_none() {
            self.initialize_script_at_rust_l0(state)?;
        }
        let observation = self.build_observation(state, false)?;
        let action = self
            .script
            .as_mut()
            .context("tactical script missing after initialization")?
            .next_action(&observation);

        match action {
            Some(TacticalAction::DeployMcv {
                action_id,
                owner,
                entity_id,
                ..
            }) => {
                self.schedule_action(
                    state,
                    action_id,
                    observation.tick,
                    &owner,
                    Command::DeployMcv { entity_id },
                )?;
                self.advance_exact_step(state)?;
            }
            Some(TacticalAction::QueueExactType {
                action_id,
                owner,
                type_id,
                ..
            }) => {
                let (owner_id, type_ref) = {
                    let sim = state
                        .match_state.sim_runtime
                        .as_mut()
                        .map(|rt| &mut rt.simulation)
                        .context("queue action requires live simulation")?;
                    (sim.interner.intern(&owner), sim.interner.intern(&type_id))
                };
                self.schedule_action(
                    state,
                    action_id,
                    observation.tick,
                    &owner,
                    Command::QueueProduction {
                        owner: owner_id,
                        type_id: type_ref,
                        mode: QueueMode::Append,
                    },
                )?;
                self.advance_exact_step(state)?;
            }
            Some(TacticalAction::PlaceExactType {
                action_id,
                owner,
                choice,
                ..
            }) => {
                let (owner_id, type_ref) = {
                    let sim = state
                        .match_state.sim_runtime
                        .as_mut()
                        .map(|rt| &mut rt.simulation)
                        .context("placement action requires live simulation")?;
                    (
                        sim.interner.intern(&owner),
                        sim.interner.intern(&choice.type_id),
                    )
                };
                self.schedule_action(
                    state,
                    action_id,
                    observation.tick,
                    &owner,
                    Command::PlaceReadyBuilding {
                        owner: owner_id,
                        type_id: type_ref,
                        rx: choice.cell.0,
                        ry: choice.cell.1,
                    },
                )?;
                self.advance_exact_step(state)?;
            }
            Some(TacticalAction::Capture) => {
                ensure!(
                    !self.capture_requested && !self.readback_started,
                    "tactical script requested more than one capture"
                );
                self.capture_requested = true;
                self.failure_stage = "final-render".to_owned();
            }
            Some(TacticalAction::Complete) => {
                bail!("tactical script completed before GPU readback")
            }
            Some(TacticalAction::Fail { failure }) => {
                bail!(
                    "tactical script failed at {:?} tick {}: {}",
                    failure.stage,
                    failure.tick,
                    failure.message
                )
            }
            None => {
                let stage = self.script.as_ref().expect("script exists").stage();
                if stage != TacticalScriptStage::CaptureRequested {
                    self.advance_exact_step(state)?;
                }
            }
        }
        Ok(())
    }

    fn schedule_action(
        &mut self,
        state: &mut AppState,
        action_id: u64,
        scheduled_tick: u64,
        owner: &str,
        command: Command,
    ) -> Result<()> {
        let execute_tick = crate::app::input::commands::try_schedule_command(state, owner, command);
        let script = self.script.as_mut().context("tactical script missing")?;
        match execute_tick {
            Some(execute_tick) => script
                .record_scheduled(action_id, scheduled_tick, execute_tick)
                .map_err(|failure| anyhow::anyhow!(failure.message)),
            None => {
                let failure = script.record_schedule_rejected(action_id, scheduled_tick);
                bail!("{}", failure.message)
            }
        }
    }

    fn advance_exact_step(&mut self, state: &mut AppState) -> Result<()> {
        let receipt = crate::app::match_runtime::sim_tick::advance_in_game_runtime_exact_step(state)
            .context("advance hidden tactical production step")?;
        self.exact_step_receipts.push(receipt);
        Ok(())
    }

    fn initialize_script_at_rust_l0(&mut self, state: &AppState) -> Result<()> {
        self.failure_stage = "rust-l0".to_owned();
        self.validate_rust_l0(state)?;
        let profile = self.request.profile();
        let sim = state
            .match_state.sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .context("Rust L0 requires live simulation")?;
        let rules = state.rules()
            .context("Rust L0 requires live rules")?;
        let owner = &profile.launch.player_name;

        let deployers: Vec<_> = sim
            .entities()
            .iter_sorted()
            .filter(|(_, entity)| {
                entity.is_active()
                    && sim.interner.resolve(entity.owner()) == owner
                    && rules
                        .object(sim.interner.resolve(entity.type_ref()))
                        .is_some_and(|object| object.deploys_into.is_some())
            })
            .collect();
        ensure!(
            deployers.len() == 1,
            "Rust L0 expected one local MCV deployer, observed {}",
            deployers.len()
        );
        let mcv = deployers[0].1;
        let mcv_type_id = sim.interner.resolve(mcv.type_ref()).to_owned();
        let mcv_rule = rules
            .object(&mcv_type_id)
            .context("local MCV has no merged rule")?;
        let yard_type_id = mcv_rule
            .deploys_into
            .clone()
            .context("local MCV lacks DeploysInto")?;
        let yard_rule = rules
            .object(&yard_type_id)
            .context("MCV DeploysInto target has no merged rule")?;

        let ledger = &profile.budgets.expected_ledger;
        // Offline commands are observed one frame after their raw issue stamp.
        let result_delay = 1;
        let power_rate = derive_rate(
            ledger.power_ready,
            ledger.yard_active,
            result_delay,
            "power",
        )?;
        let refinery_rate = derive_rate(
            ledger.refinery_ready,
            ledger.power_active,
            result_delay,
            "refinery",
        )?;
        let radar_rate = derive_rate(
            ledger.radar_ready,
            ledger.refinery_active,
            result_delay,
            "radar",
        )?;
        // The place command runs one frame after the ready observation; the
        // building then builds up for its type's Buildup control.
        let construction_ticks = |type_id: &str| -> Result<u64> {
            let frames = crate::sim::components::BuildingUp::placement_frames_to_complete(
                rules.buildup_control(type_id),
                &sim.session.game_options,
            )
            .with_context(|| format!("{type_id} build-up never completes"))?;
            Ok(result_delay + u64::try_from(frames)?)
        };
        let stage = |index: usize| StageBudget {
            max_ticks: u64::from(profile.budgets.stages[index].tick_cap),
            max_wall_ms: u64::from(profile.budgets.stages[index].wall_seconds) * 1000,
        };
        let config = TacticalScriptConfig {
            owner: owner.clone(),
            deployment: DeploymentContract {
                mcv_type_id,
                yard_type_id,
                deploy_facing: yard_rule.deploy_facing,
            },
            power: ProductionTargetContract {
                role: StructureRole::Power,
                type_id: profile.capture.build_targets.power.clone(),
                expected_rate_frames: power_rate,
                expected_ready_tick: ledger.power_ready,
                expected_active_tick: ledger.power_active,
                construction_ticks: construction_ticks(&profile.capture.build_targets.power)?,
            },
            refinery: ProductionTargetContract {
                role: StructureRole::Refinery,
                type_id: profile.capture.build_targets.refinery.clone(),
                expected_rate_frames: refinery_rate,
                expected_ready_tick: ledger.refinery_ready,
                expected_active_tick: ledger.refinery_active,
                construction_ticks: construction_ticks(&profile.capture.build_targets.refinery)?,
            },
            radar: ProductionTargetContract {
                role: StructureRole::Radar,
                type_id: profile.capture.build_targets.radar.clone(),
                expected_rate_frames: radar_rate,
                expected_ready_tick: ledger.radar_ready,
                expected_active_tick: ledger.radar_active,
                construction_ticks: construction_ticks(&profile.capture.build_targets.radar)?,
            },
            refinery_harvester_type_id: profile
                .capture
                .build_targets
                .refinery_spawned_harvester
                .clone(),
            placement_radius: u16::try_from(profile.capture.placement_radius)
                .context("placement radius exceeds u16")?,
            warm_frames: u16::try_from(profile.capture.warm_frames)
                .context("warm-frame count exceeds u16")?,
            overall_tick_cap: u64::from(profile.budgets.overall_tick_cap),
            budgets: TacticalStageBudgets {
                deploy_yard: stage(0),
                power_production: stage(1),
                power_placement: stage(2),
                refinery_production: stage(3),
                refinery_placement: stage(4),
                radar_production: stage(5),
                radar_placement: stage(6),
                radar_opening: stage(7),
                stable_frames: stage(8),
            },
            expected: TacticalExpectedLedger {
                yard_active_tick: ledger.yard_active,
            },
        };
        self.script = Some(TacticalScript::new(config).context("build tactical script")?);
        self.post_l0_started_at = Some(Instant::now());
        self.failure_stage = "production-script".to_owned();
        Ok(())
    }

    fn validate_rust_l0(&mut self, state: &AppState) -> Result<()> {
        let profile = self.request.profile();
        ensure!(
            state.frontend.screen == GameScreen::InGame,
            "Rust L0 is not InGame"
        );
        ensure!(
            state.match_state.local_player_owner.as_deref()
                == Some(profile.launch.player_name.as_str()),
            "local owner differs from sealed tactical launch"
        );
        let startup = state
            .match_state
            .startup
            .startup()
            .context("accepted loaded startup is absent")?;
        let receipt = state
            .match_state
            .startup
            .receipt()
            .context("Rust L0 receipt is absent")?;
        ensure!(
            crate::match_bootstrap::accepted_tick_is_admitted(Some(startup), Some(receipt)),
            "loaded startup and Rust L0 receipt do not admit ticks"
        );
        ensure!(
            receipt.seed == profile.launch.seed
                && receipt.seed_source == MatchSeedSource::Controlled
                && receipt.seed_authority_certifying
                && receipt.tick == 0
                && receipt.total_sim_ms == 0
                && receipt.binary_frame == 0,
            "Rust L0 receipt differs from the controlled tick-zero contract"
        );
        ensure!(
            receipt.session.launch_session() == &self.request.launch_session(),
            "Rust L0 launch session differs from the sealed profile"
        );

        let sim = state
            .match_state.sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .context("Rust L0 simulation is absent")?;
        ensure!(
            sim.session.tick == 0
                && sim.session.total_sim_ms == 0
                && sim.session.binary_frame == 0
                && sim.session.seed == u64::from(profile.launch.seed)
                && sim.input_delay_ticks == u64::from(profile.launch.input_delay_ticks),
            "live simulation is not the sealed tick-zero session"
        );
        ensure!(
            sim.session
                .map_name
                .eq_ignore_ascii_case(&profile.fixture.logical_map_name)
                && sim
                    .session
                    .theater
                    .eq_ignore_ascii_case(&profile.fixture.theater)
                && sim.session.local_width == profile.fixture.local_size.width as u16
                && sim.session.local_height == profile.fixture.local_size.height as u16
                && sim.session.mp_start_waypoints.len()
                    == profile.fixture.start_waypoint_count as usize,
            "live map/session identity differs from the sealed fixture"
        );
        let bounds = sim
            .playfield_bounds
            .context("Rust L0 playfield bounds are absent")?;
        ensure!(
            bounds.base == profile.fixture.map_size.width as i32
                && bounds.off_104 == profile.fixture.local_size.width as i32
                && bounds.off_108 == profile.fixture.local_size.height as i32,
            "live raw map/playfield bounds differ from the sealed fixture"
        );
        validate_game_options(sim, profile)?;
        validate_houses_and_slots(sim, profile)?;

        ensure!(
            state.rules().is_some()
                && state.match_state.match_presentation.tile_atlas.is_some()
                && state.match_state.match_presentation.terrain_grid.is_some()
                && state.terrain_template().is_some()
                && state.match_state.match_presentation.unit_atlas.is_some()
                && state.match_state.match_presentation.palette_set.is_some()
                && state.match_state.match_presentation.sprite_atlas.is_some()
                && state.match_state.match_presentation.overlay_atlas.is_some()
                && state.match_state.match_presentation.minimap.is_some()
                && state.match_state.match_presentation.sidebar_chrome.is_some()
                && state.match_state.match_presentation.software_cursor.is_some()
                && sim.path_grid().is_some()
                && state.process_assets.is_available(),
            "Rust L0 is missing one or more production render/simulation resources"
        );
        ensure!(
            state
                .match_state.match_presentation.software_cursor
                .as_ref()
                .and_then(|cursor| cursor.get(CursorId::Default))
                .is_some(),
            "Rust L0 lacks the Default software cursor sequence"
        );
        ensure!(
            state.match_state.input.cursor_x == profile.capture.post_load_cursor.x as f32
                && state.match_state.input.cursor_y == profile.capture.post_load_cursor.y as f32,
            "post-load cursor differs from the sealed neutral point: ({}, {})",
            state.match_state.input.cursor_x, state.match_state.input.cursor_y
        );

        let loaded = state
            .match_state.loaded_map_source
            .as_ref()
            .context("loaded map source evidence is absent")?;
        let (logical_name, source_archive, entry_id, payload_len) = match loaded {
            crate::app::frontend::list_maps::LoadedMapSource::Mix {
                logical_name,
                source_archive,
                entry_id,
                payload_len,
            } => (logical_name, source_archive, *entry_id as u32, *payload_len),
            other => bail!("tactical fixture was not loaded from MIX: {other:?}"),
        };
        ensure!(
            logical_name.eq_ignore_ascii_case(&profile.fixture.logical_map_name)
                && source_archive.eq_ignore_ascii_case(&profile.fixture.archive_name)
                && entry_id == profile.fixture.mix_entry_id
                && payload_len as u64 == profile.fixture.entry_payload_byte_length,
            "loaded map source differs from sealed MIX provenance"
        );
        let resolved = state
            .process_assets
            .manager()
            .and_then(|assets| assets.resolve_ref(&profile.fixture.logical_map_name))
            .context("post-load production asset lookup cannot resolve fixture map")?;
        let payload_sha256 = sha256_hex(resolved.bytes);
        ensure!(
            resolved
                .source_archive
                .eq_ignore_ascii_case(&profile.fixture.archive_name)
                && resolved.entry_id as u32 == profile.fixture.mix_entry_id
                && resolved.bytes.len() as u64 == profile.fixture.entry_payload_byte_length
                && payload_sha256 == profile.fixture.entry_payload_sha256,
            "post-load production asset lookup differs from sealed fixture bytes"
        );
        self.map_source_evidence = Some(json!({
            "archive_name": profile.fixture.archive_name,
            "logical_map_name": profile.fixture.logical_map_name,
            "mix_entry_id": profile.fixture.mix_entry_id,
            "payload_byte_length": profile.fixture.entry_payload_byte_length,
            "payload_sha256": payload_sha256,
            "entry_digest_authority": profile.fixture.entry_digest_authority,
            "loose_shadow_rejected": true,
            "loaded_source": loaded,
            "post_load_resolve_source_archive": resolved.source_archive,
            "post_load_resolve_entry_id": resolved.entry_id as u32,
        }));
        Ok(())
    }

    fn build_observation(
        &self,
        state: &AppState,
        capture_complete: bool,
    ) -> Result<TacticalObservation> {
        let profile = self.request.profile();
        let owner = profile.launch.player_name.clone();
        let sim = state
            .match_state.sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .context("tactical observation requires live simulation")?;
        let rules = state.rules()
            .context("tactical observation requires live rules")?;
        let entities = sim
            .entities()
            .iter_sorted()
            .map(|(_, entity)| TacticalEntityObservation {
                stable_id: entity.stable_id(),
                owner: sim.interner.resolve(entity.owner()).to_owned(),
                type_id: sim.interner.resolve(entity.type_ref()).to_owned(),
                cell: (entity.position.rx, entity.position.ry),
                facing: entity.facing,
                active: entity.is_active(),
                dying: entity.dying,
                building_up: entity.building_up.is_some(),
            })
            .collect();
        let build_options = production::build_options_for_owner(sim, rules, &owner)
            .into_iter()
            .map(|option| BuildOptionObservation {
                type_id: sim.interner.resolve(option.type_id).to_owned(),
                enabled: option.enabled,
            })
            .collect();
        let owner_id = sim.interner.get(&owner).unwrap_or_default();
        let mut queued_production = Vec::new();
        for factory in sim.production.factory_shadow.iter_insertion_ordered() {
            if factory.owner != owner_id {
                continue;
            }
            // A completed building remains attached to its factory until
            // placement. It belongs to ready_buildings, not the in-flight list.
            if let Some(object) = factory.object.as_ref()
                && factory.progress < crate::sim::production::PRODUCTION_STEPS
            {
                queued_production.push(ProductionQueueObservation {
                    type_id: sim.interner.resolve(object.type_id).to_owned(),
                    resolved_rate_frames: factory.step_rate_frames,
                });
            }
            for queued in &factory.queue {
                queued_production.push(ProductionQueueObservation {
                    type_id: sim.interner.resolve(queued.type_id).to_owned(),
                    resolved_rate_frames: 0,
                });
            }
        }
        let ready_buildings: Vec<String> =
            production::ready_buildings_for_owner(sim, rules, &owner)
                .iter()
                .map(|ready| sim.interner.resolve(ready.type_id).to_owned())
                .collect();
        let placement_choice = self
            .script
            .as_ref()
            .and_then(|script| script.structure_bindings().yard.as_ref())
            .and_then(|yard| {
                ready_buildings.first().map(|ready| {
                    let path_grid = sim
                        .path_grid()
                        .context("ready building lacks production PathGrid")?;
                    first_valid_placement(
                        sim,
                        rules,
                        &owner,
                        ready,
                        yard.stable_id,
                        yard.cell,
                        u16::try_from(profile.capture.placement_radius)
                            .context("placement radius exceeds u16")?,
                        path_grid,
                        &state.height_map(),
                        state.overlay_registry(),
                    )
                    .map_err(anyhow::Error::from)
                })
            })
            .transpose()?;
        let (power_output, power_drain) = production::power_balance_for_owner(sim, rules, &owner);
        let power_state = sim.power_states.get(&owner_id);
        let power_authority_sufficient = power_output >= power_drain
            && power_state
                .is_some_and(|power| !power.is_low_power && power.power_blackout_remaining == 0);
        let radar_authority_active = crate::sim::radar::has_radar_for_owner(sim, rules, &owner);
        let radar_online = state
            .match_state.match_presentation.radar_anim
            .as_ref()
            .is_some_and(|radar| radar.phase() == RadarAnimPhase::Online);
        let match_ended = sim
            .houses
            .get(&owner_id)
            .is_some_and(|house| house.is_defeated || house.has_won || house.has_lost);
        let wall_elapsed_ms = self
            .post_l0_started_at
            .unwrap_or(self.started_at)
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);

        Ok(TacticalObservation {
            tick: sim.session.tick,
            total_sim_ms: sim.session.total_sim_ms,
            binary_frame: sim.session.binary_frame,
            wall_elapsed_ms,
            accepted_rust_l0: crate::match_bootstrap::accepted_tick_is_admitted(
                state.match_state.startup.startup(),
                state.match_state.startup.receipt(),
            ),
            in_game: state.frontend.screen == GameScreen::InGame,
            local_owner: owner.clone(),
            match_ended,
            build_options_strict: production::has_strict_build_option_for_owner(sim, rules, &owner),
            entities,
            build_options,
            queued_production,
            ready_buildings,
            placement_choice,
            power_authority_sufficient,
            radar_authority_active,
            radar_online,
            readiness_complete: self.last_render_ready,
            capture_complete,
        })
    }

    /// Observe the completed production game/egui render and arm one readback
    /// only when the full player-visible tuple is true.
    pub(crate) fn observe_after_render(
        &mut self,
        state: &AppState,
        output: &GameRenderOutput,
    ) -> Result<bool> {
        self.render_frames = self.render_frames.saturating_add(1);
        if self.script.is_none() {
            return Ok(false);
        }
        ensure!(
            state.frontend.screen == GameScreen::InGame,
            "tactical render observation is not InGame"
        );
        let (ready, render_evidence) = self.render_readiness(state, output)?;
        self.last_render_ready = ready;
        self.last_render_evidence = Some(render_evidence);

        if self.capture_requested {
            ensure!(ready, "final tactical render readiness tuple is incomplete");
            ensure!(
                !self.readback_started,
                "tactical readback was already armed"
            );
            self.fingerprint_before_readback = Some(self.build_fingerprint(state)?);
            self.readback_started = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn render_readiness(
        &self,
        state: &AppState,
        output: &GameRenderOutput,
    ) -> Result<(bool, Value)> {
        let profile = self.request.profile();
        let sim = state
            .match_state.sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .context("render readiness requires live simulation")?;
        let owner = &profile.launch.player_name;
        let owner_id = sim.interner.get(owner).unwrap_or_default();
        let expected_theme = match profile.launch.local.country {
            crate::app::diagnostics::tactical_capture::profile::TacticalCountry::Russia => {
                SidebarTheme::Soviet
            }
            crate::app::diagnostics::tactical_capture::profile::TacticalCountry::Yuri => {
                SidebarTheme::Yuri
            }
        };
        let actual_theme = crate::app::presentation::sidebar_render::current_sidebar_theme(state);
        let radar_phase_online = state
            .match_state.match_presentation.radar_anim
            .as_ref()
            .is_some_and(|radar| radar.phase() == RadarAnimPhase::Online);
        let radar_source = state
            .match_state.match_presentation.radar_animation_source
            .as_ref()
            .context("radar animation lacks construction provenance")?;
        let source_evidence = SidebarSourceEvidence::from_identity(radar_source);
        let aperture = crate::app::presentation::sidebar_render::active_minimap_screen_rect(state);
        let insets = state
            .match_state.match_presentation.radar_content_insets
            .context("radar content insets are absent")?;
        let render_evidence = SidebarRenderEvidence::from_render_output(
            output,
            aperture,
            insets,
            [state.render_width(), state.render_height()],
        )?;
        let sidebar = output
            .sidebar_view
            .as_ref()
            .context("production render emitted no SidebarView")?;
        let panel_contains_aperture = aperture.x >= sidebar.panel_rect.x
            && aperture.y >= 0.0
            && aperture.x + aperture.w <= sidebar.panel_rect.x + sidebar.panel_rect.w
            && aperture.y + aperture.h <= state.render_height() as f32;
        let cursor_id = crate::app::input::cursor::current_cursor_feedback_kind(state)
            .and_then(crate::app::input::cursor::cursor_id_for_feedback)
            .unwrap_or(CursorId::Default);
        let cursor_ready = state.use_software_cursor()
            && cursor_id == CursorId::Default
            && state.match_state.input.cursor_x == profile.capture.post_load_cursor.x as f32
            && state.match_state.input.cursor_y == profile.capture.post_load_cursor.y as f32;
        let power = sim.power_states.get(&owner_id);
        let power_ready = power.is_some_and(|power| {
            power.total_output >= power.total_drain
                && !power.is_low_power
                && power.power_blackout_remaining == 0
        });
        let radar_authority = state.rules()
            .is_some_and(|rules| crate::sim::radar::has_radar_for_owner(sim, rules, owner));
        let bound_structures_ready = self.bound_structures_ready(state)?;
        let no_modal_or_debug = !state.match_state.paused
            && !state.match_state.match_presentation.show_save_load_panel
            && !state.main_menu_dialog_open()
            && !state.diag.debug_show_pathgrid
            && !state.diag.debug_unit_inspector
            && !state.match_state.match_presentation.show_hotkey_help
            && state.match_state.input.targeting_mode.is_none()
            && state.match_state.input.building_placement_preview.is_none()
            && state.match_state.input.keys_held.is_empty()
            && !state.match_state.input.minimap_dragging
            && !state.match_state.match_presentation.sidebar_gadget_state.repair_mode_on
            && !state.match_state.match_presentation.sidebar_gadget_state.sell_mode_on;
        let counts_ready = output.instance_counts.minimap > 0
            && output.instance_counts.viewport_rect > 0
            && output.instance_counts.radar_animation > 0;
        let sidebar_values_ready = sidebar.power_produced >= sidebar.power_drained
            && sidebar.credits
                == sim
                    .houses
                    .get(&owner_id)
                    .map(|house| house.economy.credits)
                    .unwrap_or(sidebar.credits);
        let egui = state.capture_egui_observation();
        let egui_ready = egui
            .pixels_per_point
            .is_some_and(|value| value.is_finite() && value > 0.0);
        let source_matches_owner = radar_source.requested_theme == expected_theme
            && radar_source.actual_theme == expected_theme
            && radar_source.atlas.atlas_theme == expected_theme;

        let ready = bound_structures_ready
            && power_ready
            && radar_authority
            && state.match_state.match_presentation.has_radar
            && radar_phase_online
            && actual_theme == expected_theme
            && source_matches_owner
            && output.sidebar_view.is_some()
            && panel_contains_aperture
            && counts_ready
            && sidebar_values_ready
            && cursor_ready
            && no_modal_or_debug
            && egui_ready
            && self.focus_violations == 0
            && self.input_violations == 0;
        Ok((
            ready,
            json!({
                "ready": ready,
                "theme": format!("{actual_theme:?}"),
                "expected_theme": format!("{expected_theme:?}"),
                "radar_phase": if radar_phase_online { "Online" } else { "NotOnline" },
                "radar_authority_active": radar_authority,
                "app_has_radar": state.match_state.match_presentation.has_radar,
                "power_ready": power_ready,
                "bound_structures_ready": bound_structures_ready,
                "cursor_id": format!("{cursor_id:?}"),
                "cursor": {"x": state.match_state.input.cursor_x, "y": state.match_state.input.cursor_y},
                "no_modal_or_debug": no_modal_or_debug,
                "panel_contains_aperture": panel_contains_aperture,
                "sidebar_values_ready": sidebar_values_ready,
                "egui_ready": egui_ready,
                "radar_animation_source": source_evidence,
                "production_render": render_evidence,
                "sidebar": {
                    "credits": sidebar.credits,
                    "power_produced": sidebar.power_produced,
                    "power_drained": sidebar.power_drained,
                    "low_power": sidebar.low_power,
                    "layout": {
                        "sidebar_x": sidebar.layout.sidebar_x,
                        "radar_y": sidebar.layout.radar_y,
                        "side1_y": sidebar.layout.side1_y,
                        "tabs_y": sidebar.layout.tabs_y,
                        "cameo_grid_top": sidebar.layout.cameo_grid_top,
                        "cameo_grid_bottom": sidebar.layout.cameo_grid_bottom,
                        "side3_y": sidebar.layout.side3_y,
                        "side2_tile_count": sidebar.layout.side2_tile_count,
                    }
                }
            }),
        ))
    }

    fn bound_structures_ready(&self, state: &AppState) -> Result<bool> {
        let script = self.script.as_ref().context("tactical script missing")?;
        let sim = state
            .match_state.sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .context("structure readiness requires live simulation")?;
        let bindings = script.structure_bindings();
        let required = [
            bindings.yard.as_ref(),
            bindings.power.as_ref(),
            bindings.refinery.as_ref(),
            bindings.radar.as_ref(),
        ];
        if required.iter().any(|binding| binding.is_none()) {
            return Ok(false);
        }
        for binding in required.into_iter().flatten() {
            let Some(entity) = sim.entities().get(binding.stable_id) else {
                return Ok(false);
            };
            if !entity.is_active()
                || entity.dying
                || entity.building_up.is_some()
                || sim.interner.resolve(entity.owner()) != self.request.profile().launch.player_name
                || !sim
                    .interner
                    .resolve(entity.type_ref())
                    .eq_ignore_ascii_case(&binding.type_id)
                || (entity.position.rx, entity.position.ry) != binding.cell
            {
                return Ok(false);
            }
        }
        if let Some(expected_harvester) = self
            .request
            .profile()
            .capture
            .build_targets
            .refinery_spawned_harvester
            .as_deref()
        {
            let Some(observed) = script.harvester_observation() else {
                return Ok(false);
            };
            let Some(entity) = sim.entities().get(observed.stable_id) else {
                return Ok(false);
            };
            if !entity.is_active()
                || !sim
                    .interner
                    .resolve(entity.type_ref())
                    .eq_ignore_ascii_case(expected_harvester)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn build_fingerprint(&self, state: &AppState) -> Result<Value> {
        let profile = self.request.profile();
        let owner = &profile.launch.player_name;
        let sim = state
            .match_state.sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .context("fingerprint requires live simulation")?;
        let owner_id = sim.interner.get(owner).unwrap_or_default();
        let house = sim
            .houses
            .get(&owner_id)
            .context("fingerprint local house is absent")?;
        let power = sim
            .power_states
            .get(&owner_id)
            .context("fingerprint local power state is absent")?;
        let script = self
            .script
            .as_ref()
            .context("fingerprint script is absent")?;
        let render = self
            .last_render_evidence
            .clone()
            .context("fingerprint render evidence is absent")?;
        let core = FinalFingerprint {
            simulation_tick: sim.session.tick,
            total_simulation_ms: sim.session.total_sim_ms,
            binary_frame: sim.session.binary_frame,
            deterministic_state_hash: sim.state_hash(),
        };
        Ok(json!({
            "core": core,
            "wallet": {
                "credits": house.economy.credits,
                "spent_credits": house.economy.spent_credits,
                "harvested_credits": house.economy.harvested_credits,
            },
            "power": {
                "output": power.total_output,
                "drain": power.total_drain,
                "is_low_power": power.is_low_power,
                "blackout_remaining": power.power_blackout_remaining,
            },
            "radar": {
                "authority_active": state.rules().is_some_and(|rules| {
                    crate::sim::radar::has_radar_for_owner(sim, rules, owner)
                }),
                "app_has_radar": state.match_state.match_presentation.has_radar,
                "phase": state.match_state.match_presentation.radar_anim.as_ref().map(|radar| format!("{:?}", radar.phase())),
            },
            "script": {
                "stage": script.stage(),
                "commands": script.command_ledger(),
                "placements": script.placement_ledger(),
                "bindings": script.structure_bindings(),
                "harvester": script.harvester_observation(),
                "observed_ledger": script.observed_ledger(),
            },
            "render": render,
            "cursor": {
                "x": state.match_state.input.cursor_x,
                "y": state.match_state.input.cursor_y,
                "id": format!("{:?}", crate::app::input::cursor::current_cursor_feedback_kind(state)
                    .and_then(crate::app::input::cursor::cursor_id_for_feedback)
                    .unwrap_or(CursorId::Default)),
            }
        }))
    }

    pub(crate) fn readback_timeout(&self) -> Result<Duration> {
        ensure!(
            self.readback_started,
            "tactical readback timeout requested before arming"
        );
        let timeout = Duration::from_secs(u64::from(
            self.request.profile().budgets.child_timeout_seconds,
        ));
        let remaining = timeout
            .checked_sub(self.started_at.elapsed())
            .context("tactical child timeout expired before GPU readback")?;
        ensure!(
            !remaining.is_zero(),
            "tactical child timeout expired before GPU readback"
        );
        Ok(remaining)
    }

    /// Revalidate the exact pre-encode fingerprint after present/readback, mark
    /// the script complete at the frozen capture tick, and publish atomically.
    pub(crate) fn complete_after_readback(
        &mut self,
        state: &AppState,
        surface_format: wgpu::TextureFormat,
        pixels: &[u8],
    ) -> Result<()> {
        self.failure_stage = "readback-publication".to_owned();
        ensure!(
            self.readback_started && self.capture_requested,
            "tactical readback completed before capture was armed"
        );
        ensure!(
            self.outcome.is_none(),
            "tactical outcome was already recorded"
        );
        let before = self
            .fingerprint_before_readback
            .as_ref()
            .context("pre-readback fingerprint is absent")?;
        let after = self.build_fingerprint(state)?;
        ensure!(
            before == &after,
            "tactical state changed between encode and readback completion"
        );

        let completed_observation = self.build_observation(state, true)?;
        let action = self
            .script
            .as_mut()
            .context("tactical script is absent at readback completion")?
            .next_action(&completed_observation);
        ensure!(
            matches!(action, Some(TacticalAction::Complete)),
            "tactical script did not complete at readback: {action:?}"
        );

        let evidence = self.build_manifest_evidence(state)?;
        let frame = FrameArtifact::from_bgra(
            self.request.width(),
            self.request.height(),
            format!("{surface_format:?}"),
            pixels,
        )?;
        let manifest = TacticalCaptureManifest::complete(
            self.request.sealed_profile(),
            self.request.sealed_contract(),
            frame,
            evidence,
        )?;
        publish_complete(self.request.output_dir(), &manifest, pixels)?;
        self.outcome = Some(Ok(()));
        Ok(())
    }

    fn build_manifest_evidence(&self, state: &AppState) -> Result<Value> {
        let inputs = self
            .inputs
            .as_ref()
            .context("manifest input identities are absent")?;
        let profile = self.request.profile();
        let egui = state.capture_egui_observation();
        let graphics = GraphicsEvidence::from_observations(
            state.renderer.gpu.capture_adapter_observation(),
            egui,
            format!("{:?}", state.renderer.gpu.config.format),
            [
                state.renderer.gpu.config.width,
                state.renderer.gpu.config.height,
            ],
            state.match_state.match_presentation.ui_scale,
            inputs.font.clone(),
        )?;
        let script = self.script.as_ref().context("manifest script is absent")?;
        ensure!(
            script.stage() == TacticalScriptStage::Complete,
            "manifest script is not complete"
        );
        let stable = json!({
            "inputs": {
                "config": inputs.config,
                "executable": inputs.executable,
                "archive": inputs.archive,
                "font": inputs.font,
            },
            "map_source": self.map_source_evidence,
            "lifecycle": {
                "window_hidden": state.platform.window.is_visible() == Some(false),
                "window_focused": state.platform.window.has_focus(),
                "focus_violations": self.focus_violations,
                "input_violations": self.input_violations,
            },
            "graphics": graphics,
            "contract": {
                "schema_version": CONTRACT_SCHEMA,
                "sha256": self.request.sealed_contract().sha256,
                "embedded_bytes_equal":
                    self.request.sealed_contract().bytes == EMBEDDED_CONTRACT.as_bytes(),
            },
            "profile": {
                "profile_id": profile.profile_id,
                "checkpoint": profile.checkpoint,
                "fixture_entry_sha256": profile.fixture.entry_payload_sha256,
            },
            "startup": self.startup_evidence,
            "production": {
                "exact_step_count": self.exact_step_receipts.len(),
                "first_exact_step": self.exact_step_receipts.first(),
                "last_exact_step": self.exact_step_receipts.last(),
                "command_ledger": script.command_ledger(),
                "placement_ledger": script.placement_ledger(),
                "structure_bindings": script.structure_bindings(),
                "harvester": script.harvester_observation(),
                "observed_ledger": script.observed_ledger(),
            },
            "render": self.last_render_evidence,
            "final_fingerprint": self.fingerprint_before_readback,
            "known_residuals": [
                "Native pixels and whole-game parity remain unverified."
            ],
        });
        let run = json!({
            "process_id": std::process::id(),
            "elapsed_ms": self.started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            "render_frames": self.render_frames,
            "exact_steps": self.exact_step_receipts.len(),
        });
        build_evidence(stable, run)
    }

    fn check_process_timeout(&self) -> Result<()> {
        let child_timeout = Duration::from_secs(u64::from(
            self.request.profile().budgets.child_timeout_seconds,
        ));
        ensure!(
            self.started_at.elapsed() <= child_timeout,
            "tactical child exceeded {} seconds",
            child_timeout.as_secs()
        );
        if let Some(post_l0) = self.post_l0_started_at {
            let post_l0_timeout = Duration::from_secs(u64::from(
                self.request.profile().budgets.post_l0_timeout_seconds,
            ));
            ensure!(
                post_l0.elapsed() <= post_l0_timeout,
                "tactical post-L0 loop exceeded {} seconds",
                post_l0_timeout.as_secs()
            );
        }
        Ok(())
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.outcome.is_some()
    }

    pub(crate) fn next_wake_deadline(&self) -> Instant {
        Instant::now() + CAPTURE_PUMP_INTERVAL
    }

    pub(crate) fn fail(&mut self, error: impl std::fmt::Display) {
        if self.outcome.is_none() {
            self.outcome = Some(Err(error.to_string()));
        }
    }

    /// Return the terminal child outcome. A failed run still publishes a typed
    /// immutable FAILED manifest when no prior complete directory exists.
    pub(crate) fn take_outcome(&mut self) -> Result<()> {
        match self.outcome.take() {
            Some(Ok(())) => Ok(()),
            Some(Err(error)) => {
                let failed = TacticalCaptureManifest::failed(
                    self.request.sealed_profile(),
                    self.request.sealed_contract(),
                    self.failure_stage.clone(),
                    error.clone(),
                )
                .and_then(|manifest| publish_failure(self.request.output_dir(), &manifest));
                match failed {
                    Ok(()) => bail!("{error}"),
                    Err(publish_error) => {
                        bail!("{error}; failed to publish FAILED manifest: {publish_error:#}")
                    }
                }
            }
            None => bail!("tactical capture event loop exited without a terminal outcome"),
        }
    }
}

fn artifact(path: &Path, label: &str) -> Result<ArtifactEvidence> {
    ensure!(path.is_absolute(), "{label} path must be absolute");
    let digest = sha256_file(path, label)?;
    Ok(ArtifactEvidence::from_path_digest(path, &digest))
}

fn require_identity(
    identity: &ArtifactEvidence,
    expected_length: u64,
    expected_sha256: &str,
    label: &str,
) -> Result<()> {
    ensure!(
        identity.byte_length == expected_length && identity.sha256 == expected_sha256,
        "{label} identity differs from sealed profile"
    );
    Ok(())
}

fn reject_loose_shadow(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => bail!(
            "loose tactical map shadow must be absent: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("inspect loose map shadow {}", path.display()))
        }
    }
}

fn derive_rate(
    ready_tick: u64,
    prior_active_tick: u64,
    result_delay_ticks: u64,
    label: &str,
) -> Result<u16> {
    let progress_ticks = ready_tick
        .checked_sub(prior_active_tick)
        .and_then(|value| value.checked_sub(result_delay_ticks))
        .and_then(|value| value.checked_sub(1))
        .with_context(|| format!("{label} ledger underflow"))?;
    ensure!(
        progress_ticks % PRODUCTION_PROGRESS_INTERVALS == 0,
        "{label} ledger is not divisible by {PRODUCTION_PROGRESS_INTERVALS}"
    );
    u16::try_from(progress_ticks / PRODUCTION_PROGRESS_INTERVALS)
        .with_context(|| format!("{label} rate exceeds u16"))
}

fn validate_game_options(
    sim: &crate::sim::world::Simulation,
    profile: &TacticalCaptureProfile,
) -> Result<()> {
    let actual = &sim.session.game_options;
    let expected = &profile.launch.options;
    ensure!(
        actual.starting_credits == expected.starting_credits
            && actual.unit_count == expected.unit_count
            && actual.tech_level == expected.tech_level
            && actual.game_speed == expected.game_speed
            && actual.ai_difficulty == expected.default_ai_difficulty
            && actual.ai_players == profile.launch.opponents.len() as i32
            && actual.short_game == expected.short_game
            && actual.bases == expected.bases
            && actual.bridges_destroyable == expected.bridges_destroyable
            && actual.super_weapons == expected.super_weapons
            && actual.build_off_ally == expected.build_off_ally
            && actual.crates == expected.crates
            && actual.mcv_redeploy == expected.mcv_redeploy
            && actual.fog_of_war == expected.fog_of_war
            && actual.shroud == expected.shroud
            && actual.tiberium_grows == expected.tiberium_grows
            && actual.multi_engineer == expected.multi_engineer
            && actual.harvester_truce == expected.harvester_truce
            && actual.ally_change_allowed == expected.ally_change_allowed,
        "live GameOptions differ from the sealed tactical launch"
    );
    Ok(())
}

fn validate_houses_and_slots(
    sim: &crate::sim::world::Simulation,
    profile: &TacticalCaptureProfile,
) -> Result<()> {
    let local_name = &profile.launch.player_name;
    let local_id = sim
        .interner
        .get(local_name)
        .context("sealed local house is absent")?;
    let local = sim
        .houses
        .get(&local_id)
        .context("local HouseState is absent")?;
    let local_country = local
        .country
        .map(|country| sim.interner.resolve(country))
        .context("local country is absent")?;
    let expected_local_country = profile.launch.local.country.launch_country();
    ensure!(
        local.is_human
            && local.side_index == expected_local_country.side_index()
            && local_country == expected_local_country.country_name()
            && local.difficulty == HouseDifficulty::Normal
            && local.economy.credits == profile.launch.options.starting_credits,
        "local HouseState differs from sealed slot"
    );

    ensure!(
        profile.launch.opponents.len() == 1,
        "tactical v2 requires one AI"
    );
    let ai_name = "Computer1";
    let ai_id = sim.interner.get(ai_name).context("Computer1 is absent")?;
    let ai = sim
        .houses
        .get(&ai_id)
        .context("Computer1 HouseState is absent")?;
    let ai_country = ai
        .country
        .map(|country| sim.interner.resolve(country))
        .context("Computer1 country is absent")?;
    let expected_ai_country = profile.launch.opponents[0].country.launch_country();
    // The sealed fixture uses Easy, zero starting units and stock RULESMD
    // MultiplayerAICM=400,0,0. Post_Map_Init @ 0x00686A52..0x00686A6B
    // therefore adds no opening grant (scenario_bootstrap's native evidence).
    ensure!(
        !ai.is_human
            && ai.side_index == expected_ai_country.side_index()
            && ai_country == expected_ai_country.country_name()
            && ai.difficulty == HouseDifficulty::Easy
            && ai.economy.credits == profile.launch.options.starting_credits,
        "Computer1 HouseState differs from sealed slot: credits={}, difficulty={:?}",
        ai.economy.credits, ai.difficulty
    );
    ensure!(
        sim.session.start_slot_houses.len() == 2
            && sim
                .session
                .start_slot_houses
                .get(&u32::from(profile.launch.local.start_position))
                == Some(&local_id)
            && sim
                .session
                .start_slot_houses
                .get(&u32::from(profile.launch.opponents[0].start_position))
                == Some(&ai_id),
        "start-slot house table differs from sealed positions"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::derive_rate;

    #[test]
    fn sealed_ledgers_derive_current_production_rates() {
        assert_eq!(derive_rate(617, 32, 1, "power").unwrap(), 11);
        assert_eq!(derive_rate(2611, 648, 1, "refinery").unwrap(), 37);
        assert_eq!(derive_rate(3598, 2642, 1, "radar").unwrap(), 18);
    }

    #[test]
    fn non_integral_rate_fails_closed() {
        assert!(derive_rate(620, 32, 1, "power").is_err());
    }
}
