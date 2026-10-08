//! Chosen-map controller within the existing hidden tactical capture lifecycle.
//! The launch is the production session DTO; this diagnostic owns no gameplay.

use super::super::integrity::{SealedJsonFile, parse_strict_json, read_stable_regular_bytes};
use super::super::manifest::{PublishFault, encode_manifest, publish_transaction};
use super::*;
use crate::app::diagnostics::state::{MAP_PRESENTATION_CLOCK_POLICY, MAP_PRESENTATION_INTERVAL_MS};
use crate::app::presentation::render::GameRenderTimes;
use crate::skirmish_launch::{LaunchStartPosition, PreFillHouseRoster, SkirmishLaunchSession};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const PROFILE_V1: &str = "vera20k.map-observation-profile.v1";
const PROFILE_V2: &str = "vera20k.map-observation-profile.v2";
const CHILD_SCHEMA: &str = "vera20k.map-observation.v7";
const OBSERVATION_POLICY: &str = "map-ordinary-command-observation-v4";
const MAX_COMMANDS: usize = 1024;
const MAX_GESTURES: usize = 1024;
const GESTURE_POLICY: &str = "map-tactical-left-gesture-v1";
const MAX_OBSERVED_OWNERS: usize = 30;
const MAX_OBSERVED_TYPES: usize = 256;
const MAX_TERRAIN_CELLS: usize = 256;
const MAX_OBSERVATION_SAMPLES: usize = 100_000;
const MAX_RECEIPT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MapScheduledCommand {
    issue_after_step: u32,
    owner: String,
    #[serde(deserialize_with = "deserialize_command")]
    payload: Command,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MapScheduledGesture {
    issue_after_step: u32,
    gesture: MapGesture,
}

/// Render-target pixels, fed to ordinary local left-button input. A complete
/// gesture occurs between exact steps; no render sees a synthetic held button.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum MapGesture {
    Click { position: [u32; 2] },
    Drag { from: [u32; 2], to: [u32; 2] },
}

impl MapGesture {
    fn points(&self) -> ([u32; 2], [u32; 2]) {
        match *self {
            Self::Click { position } => (position, position),
            Self::Drag { from, to } => (from, to),
        }
    }
}

// Reuse Command's one serde schema, but reject fields that its permissive
// enum deserializer would otherwise discard. No second command parser here.
fn deserialize_command<'de, D>(deserializer: D) -> std::result::Result<Command, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let command: Command =
        serde_json::from_value(value.clone()).map_err(serde::de::Error::custom)?;
    let canonical = serde_json::to_value(&command).map_err(serde::de::Error::custom)?;
    if value != canonical {
        return Err(serde::de::Error::custom(
            "command payload has unrecognized or noncanonical fields",
        ));
    }
    Ok(command)
}

/// The observed House's Supers in interned-id order: the id a profile's
/// `LaunchSuperWeapon` names and the state the sidebar reads.
fn super_weapon_rows(sim: &crate::sim::world::Simulation, owner: &str) -> Value {
    let frame = sim.session.binary_frame as i32;
    sim.interner
        .get(owner)
        .and_then(|id| sim.super_weapons.get(&id))
        .into_iter()
        .flatten()
        .map(|(&type_id, inst)| {
            let (fade_countdown, fade_coords) = inst.fade();
            json!({"type": sim.interner.resolve(type_id), "interned_id": type_id.index(),
                "granted": inst.is_active, "ready": inst.is_ready, "on_hold": inst.is_suspended,
                "charge_start": inst.charge_start_tick, "charge_duration": inst.charge_duration,
                "remaining": inst.charge_remaining(frame), "fade_countdown": fade_countdown,
                "fade_coords": fade_coords})
        })
        .collect()
}

fn deserialize_present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapCaptureProfile {
    pub(crate) schema_version: String,
    pub(crate) launch: SkirmishLaunchSession,
    pub(crate) seed: u32,
    pub(crate) input_delay_ticks: u32,
    pub(crate) ticks: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) timeout_seconds: u32,
    // Option retains field presence in the sealed profile receipt: old v1
    // profiles serialize byte-equivalent JSON values without new defaults.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    commands: Option<Vec<MapScheduledCommand>>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    gestures: Option<Vec<MapScheduledGesture>>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    observe_owners: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    observe_types: Option<Vec<String>>,
    // Opt-in immutable inputs for native Attack-coordinate comparisons.
    // Historical profiles and their exact observation rows stay unchanged.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    observe_action_line_inputs: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    camera_cell: Option<[u16; 2]>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    cursor_position: Option<[u32; 2]>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    terrain_cells: Option<Vec<[u16; 2]>>,
    // Opt-in Super rows on each observed House: the interned type id a
    // `LaunchSuperWeapon` command names, grant, readiness, hold and the
    // charge timer. Historical profiles and their rows stay unchanged.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    observe_super_weapons: Option<bool>,
}

impl MapCaptureProfile {
    pub(crate) fn load(path: &Path) -> Result<SealedJsonFile<Self>> {
        let (bytes, digest) = read_stable_regular_bytes(path, "map observation profile")?;
        let value: Self = parse_strict_json(&bytes, "map observation profile")?;
        value.validate()?;
        Ok(SealedJsonFile {
            path: path.to_path_buf(),
            byte_length: digest.byte_length,
            sha256: digest.sha256,
            bytes,
            value,
        })
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.schema_version.as_str(), PROFILE_V1 | PROFILE_V2),
            "unsupported map observation schema"
        );
        if self.schema_version == PROFILE_V1 {
            ensure!(
                self.commands.is_none()
                    && self.gestures.is_none()
                    && self.observe_owners.is_none()
                    && self.observe_types.is_none()
                    && self.observe_action_line_inputs.is_none()
                    && self.camera_cell.is_none()
                    && self.cursor_position.is_none()
                    && self.terrain_cells.is_none()
                    && self.observe_super_weapons.is_none(),
                "map observation profile v1 cannot declare v2 extension fields"
            );
        }
        ensure!(
            (640..=4096).contains(&self.width) && (480..=4096).contains(&self.height),
            "capture extent must be 640..4096 by 480..4096"
        );
        if let Some([x, y]) = self.cursor_position {
            ensure!(
                x > 0 && x < self.width - 1 && y > 0 && y < self.height - 1,
                "cursor_position must be inside the outermost capture pixels"
            );
        }
        ensure!(self.ticks <= 100_000, "capture tick budget exceeds 100000");
        ensure!(
            (1..=900).contains(&self.timeout_seconds),
            "timeout must be 1..900 seconds"
        );
        ensure!(
            self.commands().len() <= MAX_COMMANDS,
            "too many scheduled commands"
        );
        let mut previous = 0;
        for command in self.commands() {
            ensure!(
                command.issue_after_step >= previous && command.issue_after_step < self.ticks,
                "commands must be ordered by issue_after_step before the final step"
            );
            ensure!(!command.owner.is_empty(), "command owner is empty");
            ensure!(
                matches!(
                    command.payload,
                    Command::Select { .. }
                        | Command::Move { .. }
                        | Command::Stop { .. }
                        | Command::Attack { .. }
                        | Command::ForceAttack { .. }
                        | Command::Guard { .. }
                        | Command::DeployMcv { .. }
                        | Command::SetRally { .. }
                        | Command::ForceAttackCell { .. }
                        | Command::QueueProduction { .. }
                        | Command::PlaceReadyBuilding { .. }
                        | Command::CaptureBuilding { .. }
                        | Command::ToggleRepair { .. }
                        | Command::EnterTransport { .. }
                        | Command::UnloadPassengers { .. }
                        | Command::RepairAtDepot { .. }
                        | Command::SellBuilding { .. }
                        | Command::LaunchSuperWeapon { .. }
                ),
                "command is outside the map observation's ordinary order coverage"
            );
            previous = command.issue_after_step;
        }
        ensure!(
            self.gestures().len() <= MAX_GESTURES,
            "too many scheduled gestures"
        );
        if self.gestures.is_some() {
            ensure!(
                self.cursor_position.is_some(),
                "gestures require a sealed cursor_position"
            );
        }
        let (tactical_width, tactical_height) =
            crate::app::input::camera::tactical_viewport_size_px(self.width, self.height);
        let mut previous = 0;
        for gesture in self.gestures() {
            ensure!(
                gesture.issue_after_step >= previous && gesture.issue_after_step < self.ticks,
                "gestures must be ordered by issue_after_step before the final step"
            );
            let (from, to) = gesture.gesture.points();
            for [x, y] in [from, to] {
                ensure!(
                    x > 0 && x < tactical_width - 1 && y > 0 && y < tactical_height - 1,
                    "gesture points must be inside the tactical viewport's outermost pixels"
                );
            }
            ensure!(
                !matches!(gesture.gesture, MapGesture::Drag { .. }) || from != to,
                "drag endpoints must differ"
            );
            previous = gesture.issue_after_step;
        }
        let owners = self.observe_owners();
        ensure!(
            owners.len() <= MAX_OBSERVED_OWNERS,
            "too many observed owners"
        );
        let mut unique_owners = BTreeSet::new();
        for owner in owners {
            ensure!(
                !owner.is_empty() && unique_owners.insert(owner),
                "empty or duplicate observed owner"
            );
        }
        if let Some(types) = &self.observe_types {
            ensure!(
                !types.is_empty() && types.len() <= MAX_OBSERVED_TYPES,
                "observed types must contain 1..256 names"
            );
            let mut unique_types = BTreeSet::new();
            for name in types {
                ensure!(
                    !name.is_empty() && unique_types.insert(name),
                    "empty or duplicate observed type"
                );
            }
        }
        let cells = self.terrain_cells();
        ensure!(
            cells.len() <= MAX_TERRAIN_CELLS,
            "too many observed terrain cells"
        );
        ensure!(
            cells.iter().collect::<BTreeSet<_>>().len() == cells.len(),
            "duplicate observed terrain cell"
        );
        ensure!(
            matches!(
                crate::match_bootstrap::classify_startup_session(&self.launch),
                StartupSessionClassification::AcceptedExplicitFixedBattle(_)
            ),
            "map observation requires an explicit fixed Battle launch"
        );
        ensure!(
            self.launch.pre_fill_house_roster
                == PreFillHouseRoster::from_compact_skirmish(self.launch.opponents.len()),
            "map observation requires the ordinary one-human compact skirmish roster"
        );
        let mut starts = std::collections::BTreeSet::new();
        let mut colors = std::collections::BTreeSet::new();
        for (start, color) in std::iter::once((
            self.launch.local.start_position,
            self.launch.local.color_index,
        ))
        .chain(
            self.launch
                .opponents
                .iter()
                .map(|slot| (slot.start_position, slot.color_index)),
        ) {
            let LaunchStartPosition::Position(start) = start else {
                bail!("automatic start is unsupported")
            };
            ensure!(
                start < 8 && starts.insert(start),
                "invalid or duplicate explicit start"
            );
            ensure!(
                color < 8 && colors.insert(color),
                "invalid or duplicate house color"
            );
        }
        Ok(())
    }

    fn commands(&self) -> &[MapScheduledCommand] {
        self.commands.as_deref().unwrap_or_default()
    }

    fn gestures(&self) -> &[MapScheduledGesture] {
        self.gestures.as_deref().unwrap_or_default()
    }

    fn observe_owners(&self) -> &[String] {
        self.observe_owners.as_deref().unwrap_or_default()
    }

    fn terrain_cells(&self) -> &[[u16; 2]] {
        self.terrain_cells.as_deref().unwrap_or_default()
    }

    fn validate_observed_rule_types(&self, rule_types: &[Value]) -> Result<()> {
        if let Some(types) = &self.observe_types {
            for name in types {
                ensure!(
                    rule_types
                        .iter()
                        .any(|row| row["type_id"].as_str() == Some(name.as_str())),
                    "observed type {name:?} is absent from the loaded rule types"
                );
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct MapObservation {
    pub(super) initial: Option<Value>,
    inputs: Option<Value>,
    loaded_session: Option<Value>,
    rule_types: Vec<Value>,
    draws: Vec<MapDrawTime>,
    commands: Vec<MapCommandReceipt>,
    gestures: Vec<MapGestureReceipt>,
    frames: Vec<MapFrameObservation>,
    observed_ids: BTreeSet<u64>,
    sample_count: usize,
}

#[derive(Debug, Serialize)]
struct MapCommandReceipt {
    ordinal: usize,
    issue_after_step: u32,
    issued_simulation_tick: u64,
    envelope_execute_tick: u64,
    owner: String,
    payload: Command,
}

#[derive(Debug, Serialize)]
struct MapGestureCommandReceipt {
    owner: String,
    execute_tick: u64,
    payload: Command,
}

#[derive(Debug, Serialize)]
struct MapInputObservation {
    /// The existing ordered input ledger, including an optimistic selection
    /// that the next ordinary command drain has not committed yet.
    selected_ids: Vec<u64>,
    selection_pending: bool,
    target_line_remaining: i32,
    target_line_active: bool,
}

impl MapInputObservation {
    fn capture(state: &AppState) -> Result<Self> {
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("gesture observation requires a simulation")?
            .simulation;
        let lines = &state.match_state.match_presentation.target_lines;
        Ok(Self {
            selected_ids: crate::app::input::dispatch::selected_stable_ids_in_order(
                Some(sim),
                state.rules(),
                &state.match_state.input.selection_order,
                state.match_state.input.selection_order_pending,
            ),
            selection_pending: state.match_state.input.selection_order_pending,
            target_line_remaining: lines.remaining_frames(sim.session.binary_frame),
            target_line_active: lines.is_selected_action_active(sim.session.binary_frame),
        })
    }
}

#[derive(Debug, Serialize)]
struct MapGestureReceipt {
    ordinal: usize,
    issue_after_step: u32,
    issued_simulation_tick: u64,
    issued_binary_frame: u32,
    gesture: MapGesture,
    before: MapInputObservation,
    after: MapInputObservation,
    left_press_captured: bool,
    band_box_before_release: bool,
    neutral_input_restored: bool,
    queued_commands: Vec<MapGestureCommandReceipt>,
}

#[derive(Debug, Serialize)]
struct MapFrameObservation {
    completed_steps: u64,
    simulation_tick: u64,
    binary_frame: u32,
    total_simulation_ms: u64,
    actors: Vec<Value>,
    houses: Vec<Value>,
    missing_actor_ids: Vec<u64>,
    terrain: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<MapInputObservation>,
}

#[derive(Debug, Serialize)]
struct MapDrawTime {
    completed_steps: u64,
    radar_ms: u64,
    tooltip_ms: u64,
    message_ms: u64,
}

impl MapObservation {
    fn observes_actor(
        &self,
        profile: &MapCaptureProfile,
        id: u64,
        owner: &str,
        type_name: &str,
    ) -> bool {
        self.observed_ids.contains(&id)
            || (profile.observe_owners().iter().any(|watch| watch == owner)
                && profile
                    .observe_types
                    .as_ref()
                    .is_none_or(|types| types.iter().any(|watch| watch == type_name)))
    }

    fn transcript(&self, profile: &MapCaptureProfile) -> Value {
        let mut observations = json!({
            "policy": OBSERVATION_POLICY,
            "owners": profile.observe_owners(),
            "rule_types": self.rule_types,
            "commands": self.commands,
            "frames": self.frames,
        });
        if let Some(types) = &profile.observe_types {
            observations["type_filter"] = json!(types);
        }
        if profile.gestures.is_some() {
            let (width, height) =
                crate::app::input::camera::tactical_viewport_size_px(profile.width, profile.height);
            observations["gesture_input"] = json!({
                "policy": GESTURE_POLICY,
                "equal_step_order": "commands_then_gestures",
                "tactical_extent": [width, height],
                "receipts": self.gestures,
            });
        }
        observations
    }

    fn pending_command<'a>(
        &self,
        profile: &'a MapCaptureProfile,
        completed_steps: u64,
    ) -> Result<Option<&'a MapScheduledCommand>> {
        let Some(command) = profile.commands().get(self.commands.len()) else {
            return Ok(None);
        };
        ensure!(
            u64::from(command.issue_after_step) >= completed_steps,
            "scheduled command missed its issue step"
        );
        Ok((u64::from(command.issue_after_step) == completed_steps).then_some(command))
    }

    fn pending_gesture<'a>(
        &self,
        profile: &'a MapCaptureProfile,
        completed_steps: u64,
    ) -> Result<Option<&'a MapScheduledGesture>> {
        let Some(gesture) = profile.gestures().get(self.gestures.len()) else {
            return Ok(None);
        };
        ensure!(
            u64::from(gesture.issue_after_step) >= completed_steps,
            "scheduled gesture missed its issue step"
        );
        Ok((u64::from(gesture.issue_after_step) == completed_steps).then_some(gesture))
    }

    fn observe_frame(&mut self, frame: MapFrameObservation, ids: BTreeSet<u64>) -> Result<()> {
        ensure!(
            frame.completed_steps == self.frames.len() as u64
                && frame.simulation_tick == frame.completed_steps
                && u64::from(frame.binary_frame) == frame.completed_steps,
            "actor observation skipped or repeated a committed frame"
        );
        let count = self
            .sample_count
            .checked_add(frame.actors.len())
            .and_then(|count| {
                frame.actors.iter().try_fold(count, |count, actor| {
                    count.checked_add(
                        actor["building"]["animation_slots"]
                            .as_array()
                            .map_or(0, Vec::len),
                    )
                })
            })
            .and_then(|count| count.checked_add(frame.houses.len()))
            .and_then(|count| {
                frame.houses.iter().try_fold(count, |count, house| {
                    count.checked_add(house["super_weapons"].as_array().map_or(0, Vec::len))
                })
            })
            .and_then(|count| count.checked_add(frame.missing_actor_ids.len()))
            .and_then(|count| count.checked_add(frame.terrain.len()))
            .and_then(|count| {
                count.checked_add(
                    frame
                        .input
                        .as_ref()
                        .map_or(0, |input| input.selected_ids.len()),
                )
            })
            .context("actor observation sample count overflow")?;
        ensure!(
            count <= MAX_OBSERVATION_SAMPLES,
            "observation exceeds sample budget"
        );
        self.sample_count = count;
        self.observed_ids = ids;
        self.frames.push(frame);
        Ok(())
    }

    fn observe_draw(
        &mut self,
        requested: u32,
        completed_steps: u64,
        times: GameRenderTimes,
    ) -> Result<()> {
        ensure!(self.initial.is_some(), "map draw precedes accepted L0");
        ensure!(
            self.draws.len() < requested.max(1) as usize,
            "extra map draw"
        );
        let expected_step = if requested == 0 {
            0
        } else {
            self.draws.len() as u64 + 1
        };
        ensure!(
            completed_steps == expected_step,
            "map draw skipped or repeated a committed step"
        );
        let expected_ms = expected_step
            .checked_mul(MAP_PRESENTATION_INTERVAL_MS)
            .context("map presentation time overflow")?;
        let message_ms = times
            .message_ms
            .context("map draw omitted message expiry update")?;
        ensure!(
            times.radar_ms == expected_ms
                && times.tooltip_ms == expected_ms
                && message_ms == expected_ms,
            "map draw consumed inconsistent presentation clocks"
        );
        self.draws.push(MapDrawTime {
            completed_steps,
            radar_ms: times.radar_ms,
            tooltip_ms: times.tooltip_ms,
            message_ms,
        });
        Ok(())
    }
}

impl TacticalCaptureSession {
    fn map_state(&self) -> Result<&MapObservation> {
        match &self.controller {
            CaptureController::Map(map) => Ok(map),
            _ => bail!("map controller is absent"),
        }
    }

    fn map_state_mut(&mut self) -> Result<&mut MapObservation> {
        match &mut self.controller {
            CaptureController::Map(map) => Ok(map),
            _ => bail!("map controller is absent"),
        }
    }

    pub(super) fn prepare_map_observation(&mut self, state: &mut AppState) -> Result<()> {
        let profile = &self
            .request
            .map_profile()
            .context("map profile missing")?
            .value;
        ensure!(
            state.renderer.gpu.config.width == profile.width
                && state.renderer.gpu.config.height == profile.height,
            "map observation surface extent differs from request"
        );
        ensure!(
            matches!(
                state.renderer.gpu.config.format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            ) && state
                .renderer
                .gpu
                .config
                .usage
                .contains(wgpu::TextureUsages::COPY_SRC),
            "map observation requires BGRA8 COPY_SRC surface"
        );
        ensure!(
            state.match_state.configured_input_delay_ticks == u64::from(profile.input_delay_ticks),
            "configured input delay differs from observation profile"
        );
        let config = state
            .platform
            .game_config
            .as_ref()
            .context("map observation requires config.toml")?;
        ensure!(
            !config.graphics.upscale && state.renderer.upscale_pass.is_none(),
            "map observation requires native-resolution rendering (upscale=false)"
        );
        let cwd = std::env::current_dir()?;
        let inputs = json!({
            "config": artifact(&cwd.join("config.toml"), "config.toml")?,
            "executable": artifact(&std::env::current_exe()?, "capture executable")?,
        });
        self.map_state_mut()?.inputs = Some(inputs);
        state.diag.use_map_presentation_clock()?;
        state.match_state.input.cursor_x = 0.0;
        state.match_state.input.cursor_y = 0.0;
        let now_ms =
            crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(state, Instant::now());
        state.platform.frame_pacer.reanchor(now_ms);
        Ok(())
    }

    pub(super) fn drive_map_observation(&mut self, state: &mut AppState) -> Result<()> {
        if self.map_state()?.initial.is_none() {
            self.failure_stage = "rust-l0".to_owned();
            let profile = &self
                .request
                .map_profile()
                .context("map profile missing")?
                .value;
            let startup = state
                .match_state
                .startup
                .startup()
                .context("accepted startup absent")?;
            let receipt = state
                .match_state
                .startup
                .receipt()
                .context("Rust L0 receipt absent")?;
            ensure!(
                crate::match_bootstrap::accepted_tick_is_admitted(Some(startup), Some(receipt)),
                "loaded startup does not admit ticks"
            );
            ensure!(
                receipt.session.launch_session() == &profile.launch
                    && receipt.seed == profile.seed
                    && receipt.seed_source == MatchSeedSource::Controlled
                    && receipt.seed_authority_certifying
                    && receipt.tick == 0
                    && receipt.total_sim_ms == 0
                    && receipt.binary_frame == 0,
                "Rust L0 differs from requested fixed Battle launch"
            );
            ensure!(
                state.match_state.local_player_owner() == Some(profile.launch.player_name.as_str()),
                "loaded local owner differs from requested launch"
            );
            validate_loaded_resources(state)?;
            let sim = &state
                .match_state
                .sim_runtime
                .as_ref()
                .context("simulation absent")?
                .simulation;
            ensure!(
                sim.session.tick == 0
                    && sim.session.total_sim_ms == 0
                    && sim.session.binary_frame == 0
                    && sim.session.seed == u64::from(profile.seed)
                    && sim.input_delay_ticks == u64::from(profile.input_delay_ticks),
                "live simulation is not the requested tick-zero state"
            );
            validate_game_options(sim, &profile.launch)?;
            let slots: Vec<_> = sim
                .session
                .start_slot_houses
                .iter()
                .map(|(slot, house_id)| {
                    let house = sim.houses.get(house_id);
                    json!({"slot": slot, "house": sim.interner.resolve(*house_id),
                    "waypoint": sim.session.mp_start_waypoints.get(slot),
                    "country": house.and_then(|h| h.country).map(|c| sim.interner.resolve(c)),
                    "human": house.map(|h| h.is_human), "difficulty": house.map(|h| h.difficulty)})
                })
                .collect();
            for start in std::iter::once(profile.launch.local.start_position).chain(
                profile
                    .launch
                    .opponents
                    .iter()
                    .map(|slot| slot.start_position),
            ) {
                let LaunchStartPosition::Position(index) = start else {
                    bail!("unresolved start")
                };
                ensure!(
                    sim.session
                        .mp_start_waypoints
                        .contains_key(&u32::from(index))
                        && sim
                            .session
                            .start_slot_houses
                            .contains_key(&u32::from(index)),
                    "requested start {index} was not installed in the loaded map"
                );
            }
            let loaded_session = json!({"map_name": sim.session.map_name, "theater": sim.session.theater,
                "options": sim.session.game_options, "start_slots": slots, "map_waypoints": sim.session.mp_start_waypoints});
            // Simulation::intern_rule_type_ids owns these handles. Observation
            // reads the rules-owned names and installed handles without adding
            // an interner entry or translating a production command.
            let rules = state.rules().context("loaded rules absent")?;
            let mut rule_types = Vec::new();
            for (category, names) in [
                ("Infantry", &rules.infantry_ids),
                ("Unit", &rules.vehicle_ids),
                ("Aircraft", &rules.aircraft_ids),
                ("Structure", &rules.building_ids),
            ] {
                for name in names {
                    let handle = sim
                        .interner
                        .get(name)
                        .with_context(|| format!("rule type {name:?} was not preinterned"))?;
                    rule_types.push(json!({"type_id": name, "interned_id": handle.index(),
                        "category": category}));
                }
            }
            profile.validate_observed_rule_types(&rule_types)?;
            // Owner strings must name real loaded Houses before the ordinary
            // input owner is allowed to intern a command receiver.
            for owner in profile.observe_owners().iter().map(String::as_str).chain(
                profile
                    .commands()
                    .iter()
                    .map(|command| command.owner.as_str()),
            ) {
                ensure!(
                    sim.interner
                        .get(owner)
                        .is_some_and(|id| sim.houses.contains_key(&id)),
                    "observation owner {owner:?} is absent from the loaded Houses"
                );
            }
            let camera_cell = profile.camera_cell;
            let cursor_position = profile.cursor_position;
            let source = state
                .match_state
                .loaded_map_source
                .as_ref()
                .context("loaded source absent")?;
            ensure!(
                matches!(
                    source,
                    crate::map::source::LoadedMapSource::Loose { .. }
                        | crate::map::source::LoadedMapSource::Mix { .. }
                ),
                "map observation requires consumed loose or MIX map bytes"
            );
            self.map_source_evidence = Some(serde_json::to_value(source)?);
            // The production load transition has now installed its tactical
            // center. Apply an explicit diagnostic position once, before the
            // initial observation/commands; absent profiles keep that center.
            if let Some([x, y]) = cursor_position {
                state.match_state.input.cursor_x = x as f32;
                state.match_state.input.cursor_y = y as f32;
            }
            self.map_state_mut()?.initial = Some(self.map_fingerprint(state)?);
            self.map_state_mut()?.loaded_session = Some(loaded_session);
            self.map_state_mut()?.rule_types = rule_types;
            if let Some([rx, ry]) = camera_cell {
                crate::app::input::camera::center_camera_on_cell(state, rx, ry);
            }
            self.record_map_frame(state)?;
            self.post_l0_started_at = Some(Instant::now());
            self.failure_stage = "exact-steps".to_owned();
        }
        let requested = self
            .request
            .map_profile()
            .context("map profile missing")?
            .value
            .ticks as usize;
        ensure!(
            self.exact_step_receipts.len() <= requested,
            "map observation exceeded tick budget"
        );
        ensure!(
            self.map_state()?.draws.len() == self.exact_step_receipts.len(),
            "previous map step has no completed draw"
        );
        if self.exact_step_receipts.len() < requested {
            // A sealed profile has two distinct producers: already prepared
            // typed commands first, then ordinary local input gestures. Ties
            // retain order within each list; neither producer runs simulation.
            self.issue_map_commands(state)?;
            self.issue_map_gestures(state)?;
            self.advance_exact_step(state)?;
            self.record_map_frame(state)?;
        }
        if self.exact_step_receipts.len() == requested {
            self.capture_requested = true;
            self.failure_stage = "final-render".to_owned();
        }
        Ok(())
    }

    fn issue_map_commands(&mut self, state: &mut AppState) -> Result<()> {
        let completed_steps = self.exact_step_receipts.len() as u64;
        loop {
            let profile = &self
                .request
                .map_profile()
                .context("map profile missing")?
                .value;
            let Some(command) = self
                .map_state()?
                .pending_command(profile, completed_steps)?
                .cloned()
            else {
                return Ok(());
            };
            let issued_tick = state
                .match_state
                .sim_runtime
                .as_ref()
                .context("command simulation absent")?
                .simulation
                .session
                .tick;
            ensure!(
                issued_tick == completed_steps,
                "command issue is outside exact-step boundary"
            );
            // The sole ordinary input producer owns envelope encoding, stamping
            // and queuing. This diagnostic does not add the input-delay setting
            // or bypass ordinary House/actor admission in the next frame.
            let execute_tick = crate::app::input::commands::try_schedule_command(
                state,
                &command.owner,
                command.payload.clone(),
            )
            .context("ordinary command producer refused scheduled input")?;
            ensure!(
                execute_tick == issued_tick,
                "ordinary producer changed the issue stamp"
            );
            let map = self.map_state_mut()?;
            map.commands.push(MapCommandReceipt {
                ordinal: map.commands.len(),
                issue_after_step: command.issue_after_step,
                issued_simulation_tick: issued_tick,
                envelope_execute_tick: execute_tick,
                owner: command.owner,
                payload: command.payload,
            });
        }
    }

    fn issue_map_gestures(&mut self, state: &mut AppState) -> Result<()> {
        use winit::event::{ElementState, MouseButton};
        let completed_steps = self.exact_step_receipts.len() as u64;
        loop {
            let profile = &self
                .request
                .map_profile()
                .context("map profile missing")?
                .value;
            let Some(scheduled) = self
                .map_state()?
                .pending_gesture(profile, completed_steps)?
                .cloned()
            else {
                return Ok(());
            };
            let neutral = profile
                .cursor_position
                .context("gesture neutral cursor missing")?;
            ensure!(
                crate::app::input::camera::camera_input_idle(state)
                    && state.match_state.input.hotkey_modifiers.is_empty()
                    && !state.match_state.input.selection_state.is_band_box_active(),
                "gesture requires idle ordinary input without modifiers or an active band box"
            );
            ensure!(
                [
                    state.match_state.input.cursor_x,
                    state.match_state.input.cursor_y
                ] == neutral.map(|v| v as f32),
                "gesture did not start at the sealed neutral cursor"
            );
            let sim = &state
                .match_state
                .sim_runtime
                .as_ref()
                .context("gesture simulation absent")?
                .simulation;
            let issued_simulation_tick = sim.session.tick;
            let issued_binary_frame = sim.session.binary_frame;
            ensure!(
                issued_simulation_tick == completed_steps
                    && u64::from(issued_binary_frame) == completed_steps,
                "gesture issue is outside exact-step boundary"
            );
            let pending_before = sim.pending_command_snapshot();
            let before = MapInputObservation::capture(state)?;
            let move_cursor = |state: &mut AppState, [x, y]: [u32; 2]| {
                // Profile coordinates are render-target pixels already. The
                // OS window/upscale conversion is not a second input source.
                state.match_state.input.cursor_x = x as f32;
                state.match_state.input.cursor_y = y as f32;
                crate::app::input::tooltips::on_mouse_move(state);
                crate::app::input::dispatch::handle_cursor_moved_in_game(state);
            };
            let button = |state: &mut AppState, edge| {
                crate::app::input::tooltips::on_button_event(state);
                crate::app::input::dispatch::handle_mouse_input(state, MouseButton::Left, edge);
            };
            let (from, to) = scheduled.gesture.points();
            move_cursor(state, from);
            button(state, ElementState::Pressed);
            let mouse = &state.match_state.input.tactical_mouse;
            let gadgets = &state.match_state.match_presentation.in_game_gadgets;
            let left_press_captured = mouse.left_held
                && mouse.captured
                && !mouse.right_held
                && gadgets.left_held
                && !gadgets.right_held;
            if matches!(scheduled.gesture, MapGesture::Drag { .. }) {
                move_cursor(state, to);
            }
            let band_box_before_release =
                state.match_state.input.selection_state.is_band_box_active();
            button(state, ElementState::Released);
            move_cursor(state, neutral);
            let neutral_input_restored = crate::app::input::camera::camera_input_idle(state)
                && !state.match_state.input.selection_state.is_band_box_active()
                && !state
                    .match_state
                    .match_presentation
                    .in_game_gadgets
                    .left_held
                && !state
                    .match_state
                    .match_presentation
                    .in_game_gadgets
                    .right_held
                && [
                    state.match_state.input.cursor_x,
                    state.match_state.input.cursor_y,
                ] == neutral.map(|value| value as f32);
            ensure!(
                left_press_captured,
                "gesture left press did not reach the retained tactical capture"
            );
            ensure!(
                band_box_before_release == matches!(scheduled.gesture, MapGesture::Drag { .. }),
                "gesture did not produce its requested click/band-drag input state"
            );
            ensure!(
                neutral_input_restored,
                "gesture left retained input active after release"
            );
            let after = MapInputObservation::capture(state)?;
            let sim = &state
                .match_state
                .sim_runtime
                .as_ref()
                .context("gesture simulation disappeared")?
                .simulation;
            ensure!(
                sim.session.tick == issued_simulation_tick
                    && sim.session.binary_frame == issued_binary_frame,
                "gesture advanced simulation outside the exact-step owner"
            );
            let pending_after = sim.pending_command_snapshot();
            ensure!(
                pending_after.starts_with(&pending_before),
                "gesture changed previously queued commands"
            );
            let queued_commands: Vec<_> = pending_after
                .into_iter()
                .skip(pending_before.len())
                .map(|envelope| MapGestureCommandReceipt {
                    owner: sim.interner.resolve(envelope.owner).to_owned(),
                    execute_tick: envelope.execute_tick,
                    payload: envelope.payload,
                })
                .collect();
            let map = self.map_state_mut()?;
            let sample_count = map
                .sample_count
                .checked_add(before.selected_ids.len())
                .and_then(|count| count.checked_add(after.selected_ids.len()))
                .and_then(|count| count.checked_add(queued_commands.len()))
                .context("gesture observation sample count overflow")?;
            ensure!(
                sample_count <= MAX_OBSERVATION_SAMPLES,
                "gesture observation exceeds sample budget"
            );
            map.sample_count = sample_count;
            map.gestures.push(MapGestureReceipt {
                ordinal: map.gestures.len(),
                issue_after_step: scheduled.issue_after_step,
                issued_simulation_tick,
                issued_binary_frame,
                gesture: scheduled.gesture,
                before,
                after,
                left_press_captured,
                band_box_before_release,
                neutral_input_restored,
                queued_commands,
            });
        }
    }

    fn record_map_frame(&mut self, state: &AppState) -> Result<()> {
        let profile = &self
            .request
            .map_profile()
            .context("map profile missing")?
            .value;
        let runtime = state
            .match_state
            .sim_runtime
            .as_ref()
            .context("observation simulation absent")?;
        let sim = &runtime.simulation;
        let grid = runtime.view().resolved_terrain();
        // This derived diagnostic index retains every observed stable handle.
        // Captured or retyped objects continue to report their actual state;
        // destroyed objects retain a missing row rather than disappearing.
        let map = self.map_state()?;
        let mut ids = map.observed_ids.clone();
        let mut actors = Vec::new();
        if !profile.observe_owners().is_empty() {
            for (id, entity) in sim.entities().iter_sorted() {
                let owner = sim.interner.resolve(entity.owner());
                let type_name = sim.interner.resolve(entity.type_ref());
                if !map.observes_actor(profile, id, owner, type_name) {
                    continue;
                }
                ids.insert(id);
                let coord =
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                let timer = entity.mission.dispatch_timer();
                // Only the installed class owns this runtime. These value
                // getters preserve retained integer/float bits without running
                // a movement, facing, height or owner callback.
                let jumpjet = entity
                    .locomotor
                    .as_ref()
                    .and_then(|locomotor| locomotor.jumpjet_runtime())
                    .map(|runtime| {
                        let destination = runtime.destination();
                        json!({"destination_leptons": [destination.x, destination.y, destination.z],
                            "moving": runtime.moving(), "phase": runtime.phase(),
                            "landing_latched": runtime.landing_latched(),
                            "params": runtime.params(), "flight": runtime.flight()})
                    });
                let foot = if entity.category != crate::map::entities::EntityCategory::Structure {
                    let (navigation_leptons, navigation_unavailable) =
                        match sim.foot_navigation_coordinate(id) {
                            Ok(coord) => (Some([coord.x, coord.y, coord.z]), None),
                            Err(cause) => (None, Some(cause)),
                        };
                    let tracker_cell = entity.air_tracker_cell();
                    let slot_cell = entity.air_slot_cell();
                    // Read the active Walk owner's retained coordinates and
                    // moving byte. Nav and fresh ground samples cannot stand
                    // in for its paid head or destination; other active classes
                    // and absent locomotors explicitly have no Walk observation.
                    let walk = entity.locomotor.as_ref().filter(|locomotor| {
                        locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Walk
                    });
                    let walk_head_leptons: Option<[i32; 3]> = walk
                        .and_then(|walk| walk.step_head())
                        .map(|coord| [coord.x, coord.y, coord.z]);
                    let walk_destination_leptons: Option<[i32; 3]> = walk
                        .and_then(|walk| walk.walk_destination())
                        .map(|coord| [coord.x, coord.y, coord.z]);
                    let walk_is_moving: Option<bool> = walk.and_then(|walk| walk.walk_is_moving());
                    // Read the installed Drive/Ship owner without allocating
                    // runtime or deriving a head from NavCom/path directions.
                    // Its selector/cursor distinguish a paid segment from a
                    // destination awaiting the first Process invocation.
                    let track = entity.locomotor.as_ref().and_then(|loco| {
                        use crate::sim::movement::track_process::TrackFamily;
                        let family = TrackFamily::from_kind(loco.active_kind())?;
                        let progress = loco.track_progress(family)?;
                        let valid = loco.track_valid(family)?;
                        Some(json!({
                            "family": match family {
                                TrackFamily::Drive => "Drive",
                                TrackFamily::Ship => "Ship",
                            },
                            "destination_leptons": loco.track_destination(family)
                                .map(|coord| [coord.x, coord.y, coord.z]),
                            "head_leptons": loco.track_head(family)
                                .map(|coord| [coord.x, coord.y, coord.z]),
                            "selector": progress.turn_index,
                            "cursor": progress.cursor,
                            "valid": valid,
                        }))
                    });
                    Some(json!({
                        "pending_entry_500": entity.pending_entry(),
                        "retarget_after_stop_688": entity.foot_retarget_after_stop(),
                        "firing_sequence_latch_68d": entity.mission_leaf.foot_firing_sequence_latch(),
                        "infantry_doing": entity.mission_leaf.as_infantry().map(|leaf| leaf.doing()),
                        "navigation_leptons": navigation_leptons,
                        "navigation_unavailable": navigation_unavailable,
                        // Foot +560 and +564 are independent native retained
                        // Cells. NativeNull remains [0,0]; slot holders are
                        // immutable raw Cell+E0 reads, never inferred from pose.
                        "air": {
                            "tracker_cell_560": [tracker_cell.0, tracker_cell.1],
                            "slot_cell_564": [slot_cell.0, slot_cell.1],
                            "spatial_bucket": entity.air_spatial_bucket(),
                            "spatial_enter_order": entity.air_spatial_enter_order(),
                            "current_cell_slot_holder": sim.substrate.air_slots.holder(
                                entity.position.rx, entity.position.ry),
                            "tracker_cell_slot_holder": sim.substrate.air_slots.holder(
                                tracker_cell.0 as u16, tracker_cell.1 as u16),
                            "slot_cell_slot_holder": sim.substrate.air_slots.holder(
                                slot_cell.0 as u16, slot_cell.1 as u16),
                        },
                        "walk_head_leptons": walk_head_leptons,
                        "walk_destination_leptons": walk_destination_leptons,
                        "walk_is_moving": walk_is_moving,
                        "track": track,
                    }))
                } else {
                    None
                };
                let unit = entity.mission_leaf.as_unit().map(|leaf| {
                    let animation = entity.deploy_anim().map(|anim_id| {
                        json!({"stable_id": anim_id, "live": sim.anim(anim_id).map(|anim| {
                            json!({"type_id": sim.interner.resolve(anim.type_id),
                                "frame": anim.runtime.current_frame, "owner_entity": anim.owner_entity})
                        })})
                    });
                    json!({"deployed_6e0": leaf.deployed(),
                        "deploying_6e1": leaf.deploy_begin_active(),
                        "undeploying_6e2": leaf.deploy_reverse_active(),
                        "landing_for_deploy_134": entity.landing_for_deploy(),
                        "deploy_anim_130": animation, "stage_f8": entity.native_stage().value(),
                        "body_counter_538": entity.body_frame_counter,
                        "current_weapon_138": entity.current_weapon_number(),
                        "current_turret_124": entity.current_turret_index()})
                });
                let building = if entity.category == crate::map::entities::EntityCategory::Structure
                {
                    let slots: Vec<_> = entity
                        .building_anim_slots
                        .iter()
                        .enumerate()
                        .filter_map(|(slot, anim_id)| {
                            anim_id.map(|anim_id| {
                                let animation = sim.anim(anim_id).map(|anim| {
                                    json!({"stable_id": anim.stable_id,
                                        "native_id": anim.native_unique_id,
                                        "type_id": sim.interner.resolve(anim.type_id),
                                        "interned_type_id": anim.type_id.index(),
                                        "physical_leptons": [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
                                        "in_logic_vector": anim.in_logic_vector,
                                        "owner_entity": anim.owner_entity,
                                        "building_slot": anim.building_slot,
                                        "runtime": anim.runtime})
                                });
                                json!({"slot": slot, "anim_id": anim_id, "animation": animation})
                            })
                        })
                        .collect();
                    Some(json!({"body_state": entity.building_body_state(),
                    "queued_body_state": entity.queued_building_body_state(),
                    "construction_control": entity.building_construction_control(),
                    "stage": entity.native_stage(), "ready_latch": entity.building_ready_latch(),
                    "actually_placed": entity.building_actually_placed,
                    "last_operational": entity.building_last_operational,
                    "animation_slots": slots,
                    "voxel_gun": {
                        "facing": entity.body_facing_current(sim.session.binary_frame),
                        "elevation": entity.barrel_elevation().current(sim.session.binary_frame),
                        "hva_counter": entity.turret_anim_frame,
                        "recoil": entity.voxel_recoil().0,
                        "recoil_active": entity.voxel_recoil().1
                    }}))
                } else {
                    None
                };
                let miner = entity.miner.as_ref().map(|miner| {
                    json!({"cargo_bales": miner.cargo.len(),
                        "capacity_bales": miner.capacity_bales,
                        "unload_active": miner.unload_active,
                        "harvesting": miner.harvesting})
                });
                // Preserve contact slot positions, including null holes. Reads
                // of the contact/tether owners never send a radio query.
                let contacts: Vec<_> = (0..entity.radio_contacts.capacity())
                    .map(|slot| entity.radio_contacts.slot(slot))
                    .collect();
                // Read the existing cloak/type owners, without advancing a
                // timer or inferring a phase from the rendered pixels. Signed
                // +224 progress is what the native visual query consumes.
                let cloak = if let Some(cloak) = entity.cloak.as_ref() {
                    let rules = state.rules().context("cloak observation rules absent")?;
                    let object = rules
                        .object(type_name)
                        .context("cloak observation type absent")?;
                    Some(json!({
                        "state_i32": cloak.state,
                        "progress_i32": cloak.depth as i32,
                        "cloaking_stages_i32": rules.general.cloaking_stages,
                        "voxel": entity.is_voxel,
                        "no_shadow": object.no_shadow,
                    }))
                } else {
                    None
                };
                let mut observation = json!({
                    "stable_id": id, "owner": owner,
                    "type_id": type_name, "category": entity.category,
                    "cell": [entity.position.rx, entity.position.ry],
                    "physical_leptons": [coord.x, coord.y, coord.z], "on_bridge": entity.on_bridge,
                    "health": entity.health.current, "active": entity.is_active(),
                    "in_limbo": entity.lifecycle.in_limbo, "dying": entity.dying,
                    "mission": {"current": entity.mission.current().raw(),
                        "queued": entity.mission.queued().raw(), "suspended": entity.mission.suspended().raw(),
                        "effective": entity.mission.effective().raw(), "handler_state": entity.mission.handler_state(),
                        "start_frame": entity.mission.mission_start_frame(), "ai_counter": entity.mission.ai_counter(),
                        "dispatch_timer": {"start_frame": timer.start_frame(), "delay": timer.delay()}},
                    "target": entity.attack_target.as_ref().map(|target| target.target),
                    "archive": entity.archive_target(), "nav": entity.navigation.nav_com, "foot": foot,
                    "jumpjet": jumpjet,
                    "cloak": cloak,
                    "building": building, "unit": unit,
                    "miner": miner,
                    "radio": {"contacts": contacts, "dock_entered_with": entity.dock_entered_with},
                });
                if profile.observe_action_line_inputs == Some(true) {
                    let inputs = if entity.category
                        == crate::map::entities::EntityCategory::Structure
                    {
                        Value::Null
                    } else {
                        let rules = state
                            .rules()
                            .context("action-line observation rules absent")?;
                        let object = rules
                            .object(type_name)
                            .context("action-line observation type absent")?;
                        // Read existing authority; never call a setter, advance
                        // Facing, change SharedCellDummy or recompute an order.
                        // Fixed-point raw bits are labelled as such, not passed
                        // off as native doubles. Native comparisons establish
                        // each getter before consuming these prepared inputs.
                        json!({
                            "body_facing": entity.body_facing_current(sim.session.binary_frame),
                            "turret_facing": entity.barrel_facing.as_ref()
                                .map(|facing| facing.current(sim.session.binary_frame)),
                            "locomotor": entity.locomotor.as_ref().map(|loco| loco.active_kind()),
                            "is_moving": crate::sim::movement::motion_query::is_moving(entity),
                            "applied_speed_fraction_fixed_bits": entity.foot_speed.applied_fraction().to_bits(),
                            "crate_speed_multiplier_f64_bits": entity.foot_speed.crate_multiplier().bits(),
                            "house_speed_bonus_f32_bits": crate::sim::movement::owner_speed_bonus(
                                &sim.houses, entity, Some(object)).bits(),
                            "current_speed": crate::sim::movement::owner_current_speed(
                                entity, Some(object), rules.general.veteran_speed, &sim.houses),
                            "veterancy": entity.veterancy(),
                            "current_weapon": crate::sim::combat::combat_weapon::current_weapon(entity, object),
                            "turret_offset": crate::sim::combat::fire_coord::firer_art(rules, object)
                                .map_or(0, |art| art.turret_offset),
                            "rocking_angles_fixed_bits": entity.rocking.as_ref().map(|rocking| [
                                rocking.angle_sideways.to_bits(), rocking.angle_forwards.to_bits()]),
                        })
                    };
                    observation["action_line_inputs"] = inputs;
                }
                actors.push(observation);
            }
        }
        let missing_actor_ids = ids
            .iter()
            .copied()
            .filter(|id| !sim.entities().contains(*id))
            .collect();
        let houses = profile
            .observe_owners()
            .iter()
            .map(|owner| {
                let economy = crate::sim::house_state::house_state_for_owner(
                    &sim.houses,
                    owner,
                    &sim.interner,
                )
                .map(|house| &house.economy);
                // Missing Houses stay explicit rather than inventing a zero
                // balance. Economy is the same immutable wallet used by play.
                let mut row = json!({"owner": owner, "economy": economy});
                if profile.observe_super_weapons == Some(true) {
                    row["super_weapons"] = super_weapon_rows(sim, owner);
                }
                row
            })
            .collect();
        let local_owner = crate::app::input::commands::preferred_local_owner_name(state);
        let local_visibility_owner = local_owner
            .as_deref()
            .and_then(|owner| sim.interner.get(owner).map(|id| (owner, id)));
        let terrain = profile.terrain_cells().iter().map(|&[rx, ry]| {
            // Immutable real-cell indexing only. A diagnostic lookup must not
            // stamp canonical Dummy or evaluate gameplay height/zone queries.
            let cell = grid.and_then(|grid| grid.cell(rx, ry));
            let at = (rx, ry);
            let overlay = sim.overlay_grid.as_ref().map(|grid| grid.cell(rx, ry));
            let terrain_object = sim.production.terrain_object_cells.get(&at)
                .and_then(|id| sim.production.terrain_objects.get(id));
            let animation = sim.production.terrain_animations.get(&at);
            // Observe the same viewer plane consumed by drawing and picking;
            // never reveal a cell or create missing visibility state for capture.
            let local_visibility = local_visibility_owner.map(|(owner, id)| json!({
                "owner": owner, "revealed": sim.fog.is_cell_revealed(id, rx, ry),
                "visible": sim.fog.is_cell_visible(id, rx, ry),
                "gap_covered": sim.fog.is_cell_gap_covered(id, rx, ry),
            }));
            json!({"cell": [rx, ry], "allocated": cell.is_some(),
                "local_visibility": local_visibility,
                "overlay": overlay.map(|overlay| json!({"id": overlay.overlay_id, "density": overlay.overlay_data})),
                "terrain_object": terrain_object.map(|object| json!({"name": sim.interner.resolve(object.type_ref),
                    "frame": animation.map(|animation| animation.current_frame()),
                    "active": animation.map(|animation| animation.is_active())})),
                "final_tile_index": cell.map(|cell| cell.final_tile_index),
                "final_sub_tile": cell.map(|cell| cell.final_sub_tile),
                "presentation_tile": cell.and_then(|cell| grid.map(|grid| grid.presentation_tile(cell))),
                "level": cell.map(|cell| cell.level), "slope": cell.map(|cell| cell.slope_type),
                "raw_bridge_flags": cell.map(|cell| cell.bridge_facts.raw_flags),
                "bridge_state": cell.map(|cell| cell.bridge_facts.state_byte),
                "has_deck": cell.map(|cell| cell.has_bridge_deck), "deck_level": cell.map(|cell| cell.bridge_deck_level),
                "walkable": cell.map(|cell| cell.bridge_walkable), "transition": cell.map(|cell| cell.bridge_transition)})
        }).collect();
        let frame = MapFrameObservation {
            completed_steps: self.exact_step_receipts.len() as u64,
            simulation_tick: sim.session.tick,
            binary_frame: sim.session.binary_frame,
            total_simulation_ms: sim.session.total_sim_ms,
            actors,
            houses,
            missing_actor_ids,
            terrain,
            input: profile
                .gestures
                .as_ref()
                .map(|_| MapInputObservation::capture(state))
                .transpose()?,
        };
        self.map_state_mut()?.observe_frame(frame, ids)
    }

    pub(super) fn observe_map_draw(
        &mut self,
        state: &AppState,
        output: &GameRenderOutput,
    ) -> Result<()> {
        let requested = self
            .request
            .map_profile()
            .context("map profile missing")?
            .value
            .ticks;
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("map simulation absent")?
            .simulation;
        ensure!(
            sim.session.tick == self.exact_step_receipts.len() as u64
                && u64::from(sim.session.binary_frame) == sim.session.tick,
            "map draw differs from committed exact-step receipts"
        );
        ensure!(
            state.diagnostic_presentation_ms() == Some(output.times.radar_ms),
            "map diagnostic presentation policy is not active"
        );
        self.map_state_mut()?
            .observe_draw(requested, sim.session.tick, output.times)
    }

    pub(super) fn map_fingerprint(&self, state: &AppState) -> Result<Value> {
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("simulation absent")?
            .simulation;
        Ok(json!(FinalFingerprint {
            simulation_tick: sim.session.tick,
            total_simulation_ms: sim.session.total_sim_ms,
            binary_frame: sim.session.binary_frame,
            deterministic_state_hash: sim.state_hash()
        }))
    }

    pub(super) fn map_render_readiness(
        &self,
        state: &AppState,
        output: &GameRenderOutput,
    ) -> Result<(bool, Value)> {
        validate_loaded_resources(state)?;
        let unit_atlas = state
            .match_state
            .match_presentation
            .unit_atlas
            .as_ref()
            .context("unit atlas absent at final map render")?
            .statistics()?;
        let ready = output.sidebar_view.is_some()
            && !state.match_state.paused()
            && !state.match_state.match_presentation.show_save_load_panel
            && !state.main_menu_dialog_open()
            && !state.diag.debug_show_pathgrid
            && !state.diag.debug_unit_inspector
            && !state.diag.debug_show_cell_grid
            && !state.diag.debug_show_heightmap
            && !state.match_state.match_presentation.show_hotkey_help
            && self.focus_violations == 0
            && self.input_violations == 0;
        let static_default_cursor =
            crate::app::presentation::ui_overlays::static_default_cursor(state);
        let camera_input_idle = crate::app::input::camera::camera_input_idle(state);
        let cursor_position = [
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        ];
        ensure!(
            ready && static_default_cursor && camera_input_idle,
            "map observation requires every draw ready with a static cursor and idle camera input: ready={ready}, static_cursor={static_default_cursor}, camera_idle={camera_input_idle}, cursor_feedback={:?}, cursor_position={cursor_position:?}",
            crate::app::input::cursor::current_cursor_feedback_kind(state)
        );
        let profile = &self
            .request
            .map_profile()
            .context("map profile missing")?
            .value;
        if let Some(requested) = profile.cursor_position {
            ensure!(
                cursor_position == requested.map(|value| value as f32),
                "map observation cursor moved: requested={requested:?}, actual={cursor_position:?}"
            );
        }
        // Existing frame-boundary timer: up to 60 wall intervals, including
        // simulation, diagnostic observation and presentation/vsync work.
        // This is capture cadence metadata, not GPU time or ordinary play FPS.
        let frame_wall_mean_ms = state.diag.frame_timer.frame_ms_mean();
        ensure!(
            frame_wall_mean_ms.is_finite() && frame_wall_mean_ms >= 0.0,
            "map observation frame wall mean is invalid"
        );
        let mut render = json!({"ready": ready, "sidebar_view_present": output.sidebar_view.is_some(),
            "instance_counts": super::super::evidence::RenderInstanceCountEvidence::from_counts(output.instance_counts)?,
            "internal_extent": [state.render_width(), state.render_height()],
            "surface_extent": [state.renderer.gpu.config.width, state.renderer.gpu.config.height],
            "ui_scale": state.match_state.match_presentation.ui_scale,
            "gpu": super::super::evidence::GpuAdapterEvidence::from_observation(state.renderer.gpu.capture_adapter_observation()),
            "unit_atlas": unit_atlas,
            "frame_wall_mean_ms": frame_wall_mean_ms,
            "neutral_input": {"static_default_cursor": static_default_cursor, "camera_input_idle": camera_input_idle},
            "camera": {"requested_cell": profile.camera_cell,
                "top_left": [state.match_state.input.camera_x, state.match_state.input.camera_y],
                "zoom": state.match_state.input.zoom_level},
        });
        if profile.cursor_position.is_some() {
            render["cursor_position"] = json!(cursor_position);
        }
        Ok((ready, render))
    }

    pub(super) fn publish_map_observation(
        &self,
        state: &AppState,
        format: wgpu::TextureFormat,
        pixels: &[u8],
    ) -> Result<()> {
        let profile = self.request.map_profile().context("map profile missing")?;
        ensure!(
            self.exact_step_receipts.len() == profile.value.ticks as usize,
            "incomplete exact-step sequence"
        );
        let frame = FrameArtifact::from_bgra(
            self.request.width(),
            self.request.height(),
            format!("{format:?}"),
            pixels,
        )?;
        let map = self.map_state()?;
        ensure!(
            map.draws.len() == profile.value.ticks.max(1) as usize,
            "incomplete map draw schedule"
        );
        ensure!(
            map.frames.len() == profile.value.ticks as usize + 1
                && map.commands.len() == profile.value.commands().len()
                && map.gestures.len() == profile.value.gestures().len(),
            "incomplete actor/command observation transcript"
        );
        let mut render = self
            .last_render_evidence
            .clone()
            .context("map render evidence absent")?;
        // Serialize the actual transcript only once, avoiding quadratic work
        // across the up-to-100000-step capture route.
        render["presentation_clock"] = json!({"policy": MAP_PRESENTATION_CLOCK_POLICY,
            "origin_ms": 0, "interval_ms": MAP_PRESENTATION_INTERVAL_MS, "draws": map.draws});
        let manifest = json!({
            "schema_version": CHILD_SCHEMA, "status": "COMPLETE",
            "profile": {"sha256": profile.sha256, "request": profile.value},
            "contract": {"sha256": self.request.sealed_contract().sha256},
            "inputs": self.map_state()?.inputs, "map_source": self.map_source_evidence,
            "startup": self.startup_evidence, "initial": self.map_state()?.initial,
            "loaded_session": self.map_state()?.loaded_session,
            "final": self.map_fingerprint(state)?, "exact_step_count": self.exact_step_receipts.len(),
            "first_exact_step": self.exact_step_receipts.first(), "last_exact_step": self.exact_step_receipts.last(),
            "frame": frame, "render": render,
            "observations": map.transcript(&profile.value),
            "lifecycle": {"window_hidden": state.platform.window.is_visible() == Some(false),
                "window_focused": state.platform.window.has_focus(), "focus_violations": self.focus_violations,
                "input_violations": self.input_violations},
            "native_comparator": "NONE", "parity_certification": "NONE",
            "evidence_limitations": ["Production loading, exact stepping and GPU readback only; no native pixel or gameplay equivalence is established.",
                "Radar and timed HUD presentation consume the recorded diagnostic exact-step clock; ordinary gameplay clocks are unchanged. Audio, menus, animated input and scenario exit are outside this comparison."],
        });
        ensure!(
            encode_manifest(&manifest)?.len() < MAX_RECEIPT_BYTES,
            "map observation manifest exceeds the 128 MiB receipt budget"
        );
        publish_transaction(
            self.request.output_dir(),
            &manifest,
            Some(pixels),
            PublishFault::None,
        )
    }

    pub(super) fn publish_map_failure(&self, error: &str) -> Result<()> {
        let profile = self.request.map_profile().context("map profile missing")?;
        let map = self.map_state()?;
        let manifest = json!({"schema_version": CHILD_SCHEMA, "status": "FAILED",
            "profile": {"sha256": profile.sha256, "request": profile.value},
            "contract": {"sha256": self.request.sealed_contract().sha256},
            "failure": {"stage": self.failure_stage, "message": error}, "frame": null,
            "exact_step_count": self.exact_step_receipts.len(), "map_source": self.map_source_evidence,
            "observations": map.transcript(&profile.value),
            "native_comparator": "NONE", "parity_certification": "NONE"});
        ensure!(
            encode_manifest(&manifest)?.len() < MAX_RECEIPT_BYTES,
            "map failure manifest exceeds the 128 MiB receipt budget"
        );
        publish_transaction(
            self.request.output_dir(),
            &manifest,
            None,
            PublishFault::None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initialized_map() -> MapObservation {
        MapObservation {
            initial: Some(json!({"tick": 0})),
            ..Default::default()
        }
    }

    fn times(ms: u64) -> GameRenderTimes {
        GameRenderTimes {
            radar_ms: ms,
            tooltip_ms: ms,
            message_ms: Some(ms),
        }
    }

    #[test]
    fn capture_retains_only_actual_draws_in_committed_order() {
        let mut zero = initialized_map();
        zero.observe_draw(0, 0, times(0)).unwrap();
        assert!(zero.observe_draw(0, 0, times(0)).is_err());
        let mut map = initialized_map();
        assert!(map.observe_draw(3, 0, times(0)).is_err());
        map.observe_draw(3, 1, times(22)).unwrap();
        assert!(map.observe_draw(3, 1, times(22)).is_err());
        assert!(map.observe_draw(3, 3, times(66)).is_err());
        map.observe_draw(3, 2, times(44)).unwrap();
        map.observe_draw(3, 3, times(66)).unwrap();
        let evidence = serde_json::to_value(&map.draws).unwrap();
        assert_eq!(evidence[0]["completed_steps"], 1);
        assert_eq!(evidence[2]["radar_ms"], 66);
        assert_eq!(map.draws.len(), 3);
        assert!(map.observe_draw(3, 4, times(88)).is_err());
    }

    #[test]
    fn capture_rejects_missing_hud_update_or_any_wall_clock_sample() {
        assert!(
            MapObservation::default()
                .observe_draw(1, 1, times(22))
                .is_err()
        );
        let mut map = initialized_map();
        for sample in [
            GameRenderTimes {
                radar_ms: 9_876,
                ..times(22)
            },
            GameRenderTimes {
                tooltip_ms: 9_876,
                ..times(22)
            },
            GameRenderTimes {
                message_ms: Some(0),
                ..times(22)
            },
            GameRenderTimes {
                message_ms: None,
                ..times(22)
            },
        ] {
            assert!(map.observe_draw(1, 1, sample).is_err());
            assert!(
                map.draws.is_empty(),
                "rejected observation must not advance schedule"
            );
        }
        map.observe_draw(1, 1, times(22)).unwrap();
    }

    fn example() -> MapCaptureProfile {
        serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.example.json",
        ))
        .unwrap()
    }

    #[test]
    fn example_preserves_existing_accepted_launch_and_allows_chosen_map() {
        let mut profile = example();
        profile.validate().unwrap();
        let radar: super::super::super::profile::TacticalCaptureProfile =
            serde_json::from_str(crate::test_fixture::text(
                "tools/tactical_certification/profiles/soviet-radar-online-v2.json",
            ))
            .unwrap();
        assert_eq!(profile.launch, radar.launch_session());
        profile.launch.selected_map_file = Some("mp01t4.map".to_owned());
        profile.validate().unwrap();
        profile.ticks = 0;
        profile.validate().unwrap();
    }

    #[test]
    fn unresolved_launch_duplicate_slots_and_inconsistent_roster_fail() {
        let mut profile = example();
        profile.launch.local.start_position = LaunchStartPosition::Auto;
        assert!(profile.validate().is_err());
        let mut profile = example();
        profile.launch.opponents[0].color_index = profile.launch.local.color_index;
        assert!(profile.validate().is_err());
        let mut profile = example();
        profile.launch.opponents.clear();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn invalid_budgets_and_unknown_request_fields_fail() {
        let mut profile = example();
        profile.timeout_seconds = 0;
        assert!(profile.validate().is_err());
        profile.timeout_seconds = 180;
        profile.ticks = 100_001;
        assert!(profile.validate().is_err());
        let mut json = serde_json::to_value(example()).unwrap();
        json["launch"]["ignored_option"] = json!(true);
        assert!(serde_json::from_value::<MapCaptureProfile>(json).is_err());
    }

    #[test]
    fn versioned_extension_fields_preserve_presence_and_reject_null_or_ignored_arguments() {
        let original: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.example.json",
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(example()).unwrap(), original);
        let mut legacy = example();
        legacy.commands = Some(Vec::new());
        assert!(legacy.validate().is_err());
        let mut legacy = example();
        legacy.observe_types = Some(vec!["CLEG".to_owned()]);
        assert!(legacy.validate().is_err());
        let mut legacy = example();
        legacy.observe_action_line_inputs = Some(false);
        assert!(legacy.validate().is_err());
        let mut legacy = example();
        legacy.cursor_position = Some([720, 556]);
        assert!(legacy.validate().is_err());
        let mut legacy = example();
        legacy.gestures = Some(Vec::new());
        assert!(legacy.validate().is_err());
        let mut legacy = example();
        legacy.observe_super_weapons = Some(false);
        assert!(legacy.validate().is_err());
        let mut modern = original;
        modern["schema_version"] = json!(PROFILE_V2);
        modern["commands"] = json!([{"issue_after_step": 0, "owner": "Computer1",
            "payload": {"DeployMcv": {"entity_id": 1}}},
            {"issue_after_step": 0, "owner": "Computer1", "payload": {"LaunchSuperWeapon":
                {"sw_type_id": 5, "target_rx": 3, "target_ry": 4}}}]);
        modern["observe_owners"] = json!(["Computer1"]);
        modern["observe_types"] = json!(["CLEG"]);
        modern["observe_action_line_inputs"] = json!(true);
        modern["cursor_position"] = json!([720, 556]);
        modern["gestures"] = json!([]);
        modern["observe_super_weapons"] = json!(true);
        let profile: MapCaptureProfile = serde_json::from_value(modern.clone()).unwrap();
        profile.validate().unwrap();
        assert_eq!(serde_json::to_value(profile).unwrap(), modern);
        for key in [
            "commands",
            "gestures",
            "observe_owners",
            "observe_types",
            "observe_action_line_inputs",
            "camera_cell",
            "cursor_position",
            "terrain_cells",
            "observe_super_weapons",
        ] {
            let mut invalid = modern.clone();
            invalid[key] = Value::Null;
            assert!(
                serde_json::from_value::<MapCaptureProfile>(invalid).is_err(),
                "{key}"
            );
        }
        modern["observe_action_line_inputs"] = json!(false);
        let disabled: MapCaptureProfile = serde_json::from_value(modern.clone()).unwrap();
        disabled.validate().unwrap();
        assert_eq!(serde_json::to_value(disabled).unwrap(), modern);
        for value in [json!(0), json!(1), json!("true"), json!([])] {
            let mut invalid = modern.clone();
            invalid["observe_action_line_inputs"] = value;
            assert!(serde_json::from_value::<MapCaptureProfile>(invalid).is_err());
        }
        for (key, value) in [("ignored", json!(true)), ("entity_id", json!(1.0))] {
            let mut invalid = modern.clone();
            invalid["commands"][0]["payload"]["DeployMcv"][key] = value;
            assert!(
                serde_json::from_value::<MapCaptureProfile>(invalid).is_err(),
                "{key}"
            );
        }
    }

    #[test]
    fn cursor_position_requires_integer_screen_coordinates_away_from_the_outermost_pixel() {
        let mut profile = example();
        profile.schema_version = PROFILE_V2.to_owned();
        for position in [[1, 1], [798, 598], [720, 556]] {
            profile.cursor_position = Some(position);
            profile.validate().unwrap();
        }
        for position in [
            [0, 1],
            [1, 0],
            [799, 1],
            [1, 599],
            [800, 1],
            [1, 600],
            [u32::MAX, 1],
        ] {
            profile.cursor_position = Some(position);
            assert!(profile.validate().is_err(), "{position:?}");
        }
        let original = serde_json::to_value(profile).unwrap();
        for position in [
            json!([]),
            json!([1]),
            json!([1, 1, 1]),
            json!([true, 1]),
            json!([1.0, 1]),
            json!([-1, 1]),
            json!([u64::from(u32::MAX) + 1, 1]),
        ] {
            let mut invalid = original.clone();
            invalid["cursor_position"] = position;
            assert!(serde_json::from_value::<MapCaptureProfile>(invalid).is_err());
        }
    }

    #[test]
    fn gestures_require_sealed_neutral_cursor_and_strict_tactical_pixel_arguments() {
        let mut profile = example();
        profile.schema_version = PROFILE_V2.to_owned();
        profile.gestures = Some(Vec::new());
        assert!(profile.validate().is_err());
        profile.cursor_position = Some([720, 556]);
        profile.validate().unwrap();
        let (width, height) =
            crate::app::input::camera::tactical_viewport_size_px(profile.width, profile.height);
        profile.gestures = Some(vec![MapScheduledGesture {
            issue_after_step: 0,
            gesture: MapGesture::Click {
                position: [width - 2, height - 2],
            },
        }]);
        profile.validate().unwrap();
        for position in [[0, 1], [1, 0], [width - 1, 1], [1, height - 1], [720, 556]] {
            profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Click { position };
            assert!(profile.validate().is_err(), "{position:?}");
        }
        profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Drag {
            from: [10, 10],
            to: [10, 10],
        };
        assert!(profile.validate().is_err());
        profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Drag {
            from: [10, 10],
            to: [100, 100],
        };
        profile.validate().unwrap();
        let valid = serde_json::to_value(profile).unwrap();
        for gesture in [
            json!({"kind": "click", "position": [true, 1]}),
            json!({"kind": "click", "position": [1.0, 1]}),
            json!({"kind": "click", "position": [1, 1], "button": "right"}),
            json!({"kind": "click", "position": [1, 1, 1]}),
            json!({"kind": "drag", "from": [1, 1]}),
            json!({"kind": "move", "position": [1, 1]}),
            Value::Null,
        ] {
            let mut invalid = valid.clone();
            invalid["gestures"][0]["gesture"] = gesture;
            assert!(serde_json::from_value::<MapCaptureProfile>(invalid).is_err());
        }
    }

    #[test]
    fn gesture_schedule_retains_equal_step_order_and_conditional_receipt_presence() {
        let mut profile = example();
        let mut map = initialized_map();
        assert!(map.transcript(&profile).get("gesture_input").is_none());
        profile.schema_version = PROFILE_V2.to_owned();
        profile.cursor_position = Some([720, 556]);
        profile.ticks = 3;
        profile.gestures = Some(vec![
            MapScheduledGesture {
                issue_after_step: 0,
                gesture: MapGesture::Click { position: [10, 10] },
            },
            MapScheduledGesture {
                issue_after_step: 0,
                gesture: MapGesture::Drag {
                    from: [20, 20],
                    to: [100, 100],
                },
            },
            MapScheduledGesture {
                issue_after_step: 2,
                gesture: MapGesture::Click { position: [30, 30] },
            },
        ]);
        profile.validate().unwrap();
        assert!(map.pending_gesture(&profile, 1).is_err());
        let observed = || MapInputObservation {
            selected_ids: Vec::new(),
            selection_pending: false,
            target_line_remaining: 0,
            target_line_active: false,
        };
        for ordinal in 0..2 {
            let scheduled = map.pending_gesture(&profile, 0).unwrap().unwrap().clone();
            assert_eq!(
                serde_json::to_value(&scheduled.gesture).unwrap(),
                serde_json::to_value(&profile.gestures()[ordinal].gesture).unwrap()
            );
            map.gestures.push(MapGestureReceipt {
                ordinal,
                issue_after_step: 0,
                issued_simulation_tick: 0,
                issued_binary_frame: 0,
                band_box_before_release: matches!(scheduled.gesture, MapGesture::Drag { .. }),
                gesture: scheduled.gesture,
                before: observed(),
                after: observed(),
                left_press_captured: true,
                neutral_input_restored: true,
                queued_commands: Vec::new(),
            });
        }
        assert!(map.pending_gesture(&profile, 1).unwrap().is_none());
        assert_eq!(
            map.pending_gesture(&profile, 2)
                .unwrap()
                .unwrap()
                .issue_after_step,
            2
        );
        assert!(map.pending_gesture(&profile, 3).is_err());
        let transcript = map.transcript(&profile);
        assert_eq!(transcript["gesture_input"]["policy"], GESTURE_POLICY);
        assert_eq!(
            transcript["gesture_input"]["equal_step_order"],
            "commands_then_gestures"
        );
        assert_eq!(
            transcript["gesture_input"]["receipts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        profile.gestures.as_mut().unwrap()[2].issue_after_step = 3;
        assert!(profile.validate().is_err());
        profile.gestures.as_mut().unwrap()[0].issue_after_step = 1;
        assert!(profile.validate().is_err());
        let row = profile.gestures()[1].clone();
        profile.gestures = Some(vec![row; MAX_GESTURES + 1]);
        assert!(profile.validate().is_err());
    }

    #[test]
    fn type_filters_require_bounded_unique_names_from_the_exact_loaded_registry() {
        let mut profile = example();
        profile.schema_version = PROFILE_V2.to_owned();
        for types in [
            Vec::new(),
            vec![String::new()],
            vec!["CLEG".to_owned(), "CLEG".to_owned()],
            (0..=MAX_OBSERVED_TYPES)
                .map(|n| format!("TYPE{n}"))
                .collect(),
        ] {
            profile.observe_types = Some(types);
            assert!(profile.validate().is_err());
        }
        profile.observe_types = Some(
            (0..MAX_OBSERVED_TYPES)
                .map(|n| format!("TYPE{n}"))
                .collect(),
        );
        profile.validate().unwrap();
        profile.observe_types = Some(vec!["CLEG".to_owned(), "MTNK".to_owned()]);
        profile.validate().unwrap();
        let registry = vec![
            json!({"type_id": "CLEG", "interned_id": 3, "category": "Infantry"}),
            json!({"type_id": "MTNK", "interned_id": 7, "category": "Unit"}),
        ];
        profile.validate_observed_rule_types(&registry).unwrap();
        assert!(
            profile
                .validate_observed_rule_types(&registry[..1])
                .is_err()
        );
        profile.observe_types = Some(vec!["cleg".to_owned()]);
        assert!(profile.validate_observed_rule_types(&registry).is_err());
        profile.observe_types = None;
        profile.validate_observed_rule_types(&[]).unwrap();
    }

    #[test]
    fn type_discovery_preserves_retained_identity_and_optional_transcript_presence() {
        let mut profile = example();
        profile.schema_version = PROFILE_V2.to_owned();
        profile.observe_owners = Some(vec!["Computer1".to_owned()]);
        profile.observe_types = Some(vec!["MTNK".to_owned(), "CLEG".to_owned()]);
        let mut map = initialized_map();
        assert!(map.observes_actor(&profile, 7, "Computer1", "CLEG"));
        assert!(!map.observes_actor(&profile, 8, "Computer2", "CLEG"));
        assert!(!map.observes_actor(&profile, 9, "Computer1", "GAPOWR"));
        assert!(!map.observes_actor(&profile, 10, "Computer1", "cleg"));
        map.observe_frame(
            MapFrameObservation {
                completed_steps: 0,
                simulation_tick: 0,
                binary_frame: 0,
                total_simulation_ms: 0,
                actors: vec![json!({"stable_id": 7, "owner": "Computer1", "type_id": "CLEG"})],
                houses: Vec::new(),
                missing_actor_ids: Vec::new(),
                terrain: Vec::new(),
                input: None,
            },
            BTreeSet::from([7]),
        )
        .unwrap();
        assert!(map.observes_actor(&profile, 7, "Computer2", "GAPOWR"));
        map.observe_frame(
            MapFrameObservation {
                completed_steps: 1,
                simulation_tick: 1,
                binary_frame: 1,
                total_simulation_ms: 22,
                actors: Vec::new(),
                houses: Vec::new(),
                missing_actor_ids: vec![7],
                terrain: Vec::new(),
                input: None,
            },
            BTreeSet::from([7]),
        )
        .unwrap();
        assert!(map.observes_actor(&profile, 7, "Computer2", "GAPOWR"));
        let transcript = map.transcript(&profile);
        assert_eq!(transcript["type_filter"], json!(["MTNK", "CLEG"]));
        assert_eq!(transcript["frames"][1]["missing_actor_ids"], json!([7]));
        profile.observe_types = None;
        assert!(map.observes_actor(&profile, 9, "Computer1", "GAPOWR"));
        assert!(!map.observes_actor(&profile, 8, "Computer2", "CLEG"));
        assert!(map.transcript(&profile).get("type_filter").is_none());
    }

    #[test]
    fn command_schedule_keeps_equal_step_input_order_and_refuses_missed_or_final_steps() {
        let mut profile = example();
        profile.schema_version = PROFILE_V2.to_owned();
        profile.ticks = 3;
        profile.commands = Some(vec![
            MapScheduledCommand {
                issue_after_step: 0,
                owner: "Computer1".to_owned(),
                payload: Command::Stop { entity_id: 1 },
            },
            MapScheduledCommand {
                issue_after_step: 0,
                owner: "Computer1".to_owned(),
                payload: Command::DeployMcv { entity_id: 2 },
            },
            MapScheduledCommand {
                issue_after_step: 2,
                owner: "Computer1".to_owned(),
                payload: Command::ForceAttackCell {
                    attacker_id: 3,
                    target_rx: 87,
                    target_ry: 53,
                },
            },
        ]);
        profile.validate().unwrap();
        let mut map = initialized_map();
        assert!(map.pending_command(&profile, 1).is_err());
        for ordinal in 0..2 {
            let command = map.pending_command(&profile, 0).unwrap().unwrap().clone();
            assert_eq!(command.payload, profile.commands()[ordinal].payload);
            map.commands.push(MapCommandReceipt {
                ordinal,
                issue_after_step: 0,
                issued_simulation_tick: 0,
                envelope_execute_tick: 0,
                owner: command.owner,
                payload: command.payload,
            });
        }
        assert!(map.pending_command(&profile, 1).unwrap().is_none());
        assert_eq!(
            map.pending_command(&profile, 2)
                .unwrap()
                .unwrap()
                .issue_after_step,
            2
        );
        assert!(map.pending_command(&profile, 3).is_err());
        profile.commands.as_mut().unwrap()[2].issue_after_step = 3;
        assert!(profile.validate().is_err());
        profile.commands.as_mut().unwrap()[0].issue_after_step = 1;
        assert!(profile.validate().is_err());
    }

    #[test]
    fn production_profile_reuses_typed_command_serde_without_name_translation() {
        let mut value = serde_json::to_value(example()).unwrap();
        value["schema_version"] = json!(PROFILE_V2);
        value["commands"] = json!([
            {"issue_after_step": 0, "owner": "Computer1",
                "payload": {"QueueProduction": {"type_id": 41}}},
            {"issue_after_step": 1, "owner": "Computer1",
                "payload": {"PlaceReadyBuilding": {"type_id": 41, "rx": 87, "ry": 53}}},
            {"issue_after_step": 2, "owner": "Computer1",
                "payload": {"CaptureBuilding": {"engineer_id": 7, "target_building_id": 9}}},
            {"issue_after_step": 2, "owner": "Computer1",
                "payload": {"ToggleRepair": {"entity_id": 9}}},
            {"issue_after_step": 2, "owner": "Computer1",
                "payload": {"EnterTransport": {"passenger_id": 7, "transport_id": 8}}},
            {"issue_after_step": 2, "owner": "Computer1",
                "payload": {"UnloadPassengers": {"transport_id": 8}}},
            {"issue_after_step": 2, "owner": "Computer1",
                "payload": {"RepairAtDepot": {"entity_id": 7, "depot_id": 9}}},
            {"issue_after_step": 2, "owner": "Computer1",
                "payload": {"SellBuilding": {"entity_id": 10}}},
        ]);
        let profile: MapCaptureProfile = serde_json::from_value(value.clone()).unwrap();
        profile.validate().unwrap();
        assert_eq!(serde_json::to_value(profile).unwrap(), value);
        for invalid_id in [json!("GAPOWR"), json!(true), json!(1.5), json!(u64::MAX)] {
            let mut invalid = value.clone();
            invalid["commands"][0]["payload"]["QueueProduction"]["type_id"] = invalid_id;
            assert!(serde_json::from_value::<MapCaptureProfile>(invalid).is_err());
        }
        for invalid_id in [json!("ENGINEER"), json!(true), json!(1.5), json!(-1)] {
            let mut invalid = value.clone();
            invalid["commands"][2]["payload"]["CaptureBuilding"]["engineer_id"] = invalid_id;
            assert!(serde_json::from_value::<MapCaptureProfile>(invalid).is_err());
        }
        for (index, variant, fields) in [
            (6, "RepairAtDepot", &["entity_id", "depot_id"][..]),
            (7, "SellBuilding", &["entity_id"][..]),
        ] {
            for field in fields {
                for invalid_id in [json!("HTNK"), json!(true), json!(1.5), json!(-1)] {
                    let mut invalid = value.clone();
                    invalid["commands"][index]["payload"][variant][*field] = invalid_id;
                    assert!(serde_json::from_value::<MapCaptureProfile>(invalid).is_err());
                }
                let mut invalid = value.clone();
                invalid["commands"][index]["payload"][variant]["ignored"] = json!(true);
                assert!(serde_json::from_value::<MapCaptureProfile>(invalid).is_err());
            }
        }
        value["commands"][1]["payload"]["PlaceReadyBuilding"]["ignored"] = json!(true);
        assert!(serde_json::from_value::<MapCaptureProfile>(value).is_err());
    }

    #[test]
    fn jumpjet_discovery_profile_preserves_verified_production_prefix_without_guessed_actor() {
        let profile: MapCaptureProfile = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.jumpjet-instance.example.json",
        ))
        .unwrap();
        profile.validate().unwrap();
        let cmin: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.cmin-instance.example.json",
        ))
        .unwrap();
        let factory: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.factory-tank-exit.example.json",
        ))
        .unwrap();
        let value = serde_json::to_value(&profile).unwrap();
        for key in ["launch", "seed", "input_delay_ticks", "ticks"] {
            assert_eq!(value[key], cmin[key], "{key}");
        }
        let commands = value["commands"].as_array().unwrap();
        let factory_commands = factory["commands"].as_array().unwrap();
        assert_eq!(
            &commands[..commands.len() - 1],
            &factory_commands[..factory_commands.len() - 1]
        );
        assert_eq!(
            commands.last().unwrap(),
            &json!({"issue_after_step": 4600, "owner": "VERA-OBSERVER",
                "payload": {"QueueProduction": {"type_id": 210}}})
        );
        assert!(profile.commands().iter().all(|command| {
            matches!(
                command.payload,
                Command::DeployMcv { .. }
                    | Command::QueueProduction { .. }
                    | Command::PlaceReadyBuilding { .. }
            )
        }));
        assert!(
            profile
                .observe_types
                .as_ref()
                .unwrap()
                .iter()
                .any(|name| name == "SHAD")
        );
    }

    #[test]
    fn observations_require_l0_then_every_committed_frame_and_bound_retained_samples() {
        let frame = |step, tick| MapFrameObservation {
            completed_steps: step,
            simulation_tick: tick,
            binary_frame: tick as u32,
            total_simulation_ms: tick * 22,
            actors: Vec::new(),
            houses: Vec::new(),
            missing_actor_ids: Vec::new(),
            terrain: Vec::new(),
            input: None,
        };
        let mut map = initialized_map();
        assert!(map.observe_frame(frame(1, 1), BTreeSet::new()).is_err());
        map.observe_frame(frame(0, 0), BTreeSet::new()).unwrap();
        assert!(map.observe_frame(frame(0, 0), BTreeSet::new()).is_err());
        assert!(map.observe_frame(frame(1, 2), BTreeSet::new()).is_err());
        map.observe_frame(frame(1, 1), BTreeSet::new()).unwrap();
        let mut large = frame(2, 2);
        large.terrain = vec![Value::Null; MAX_OBSERVATION_SAMPLES + 1];
        assert!(map.observe_frame(large, BTreeSet::new()).is_err());
        let mut large = frame(2, 2);
        large.input = Some(MapInputObservation {
            selected_ids: (1..=MAX_OBSERVATION_SAMPLES as u64 + 1).collect(),
            selection_pending: false,
            target_line_remaining: 0,
            target_line_active: false,
        });
        assert!(map.observe_frame(large, BTreeSet::new()).is_err());
        assert_eq!(map.frames.len(), 2);
        assert_eq!(map.sample_count, 0);
    }

    #[test]
    fn anytown_discovery_example_is_an_accepted_ordinary_allied_ai_launch() {
        let profile: MapCaptureProfile = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.bridge-response.example.json",
        ))
        .unwrap();
        profile.validate().unwrap();
        assert_eq!(
            profile.launch.selected_map_file.as_deref(),
            Some("XMP03T4.MAP")
        );
        assert_eq!(profile.launch.opponents.len(), 2);
        assert_eq!(
            profile.commands().len(),
            0,
            "discovery must not invent stable actor IDs"
        );
        assert_eq!(profile.ticks, 0);
        assert_eq!(profile.observe_owners(), ["Computer1", "Computer2"]);
    }

    #[test]
    fn barracks_output_example_preserves_opening_and_queues_two_gis_in_order() {
        let profile: MapCaptureProfile = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.barracks-output.example.json",
        ))
        .unwrap();
        let opening: MapCaptureProfile = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.building-opening.example.json",
        ))
        .unwrap();
        profile.validate().unwrap();
        assert_eq!(profile.launch, opening.launch);
        assert_eq!(profile.seed, opening.seed);
        assert_eq!(profile.input_delay_ticks, opening.input_delay_ticks);
        assert_eq!(profile.observe_owners(), opening.observe_owners());
        assert_eq!(profile.camera_cell, opening.camera_cell);
        assert_eq!(profile.terrain_cells(), opening.terrain_cells());
        assert!(profile.ticks > opening.ticks);
        let previous = opening.commands();
        let commands = profile.commands();
        assert_eq!(commands.len(), previous.len() + 1);
        for (observed, prior) in commands.iter().zip(previous) {
            assert_eq!(
                serde_json::to_value(observed).unwrap(),
                serde_json::to_value(prior).unwrap()
            );
        }
        let first_gi = previous.last().unwrap();
        assert!(matches!(first_gi.payload, Command::QueueProduction { .. }));
        assert_eq!(
            serde_json::to_value(commands.last().unwrap()).unwrap(),
            serde_json::to_value(first_gi).unwrap(),
            "two equal-step ordinary PRODUCE commands preserve queue insertion order"
        );
        assert!(
            commands
                .iter()
                .all(|command| !matches!(command.payload, Command::SetRally { .. })),
            "the default route exercises the constructor-empty Archive path"
        );
    }

    #[test]
    fn nuclear_missile_example_launches_the_charged_silo_at_the_neutral_tanks() {
        let profile: MapCaptureProfile = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.nuclear-missile.example.json",
        ))
        .unwrap();
        profile.validate().unwrap();
        assert_eq!(profile.observe_super_weapons, Some(true));
        // The silo's Super charges for 9000 frames from the first step; the
        // id is the one its `observe_super_weapons` row reports.
        let [command] = profile.commands() else {
            panic!("one launch");
        };
        assert_eq!(command.issue_after_step, 9010);
        assert!(matches!(
            command.payload,
            Command::LaunchSuperWeapon {
                target_rx: 41,
                target_ry: 63,
                ..
            }
        ));
    }

    #[test]
    fn computer_nuclear_missile_example_leaves_the_launch_to_the_computer() {
        let profile: MapCaptureProfile = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.ai-nuclear-missile.example.json",
        ))
        .unwrap();
        profile.validate().unwrap();
        assert_eq!(profile.observe_super_weapons, Some(true));
        // The computer's Strategy tick fires its charged silo.
        assert!(profile.commands().is_empty());
    }

    #[test]
    fn computer_psychic_dominator_example_leaves_the_launch_to_the_computer() {
        let profile: MapCaptureProfile = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.ai-psychic-dominator.example.json",
        ))
        .unwrap();
        profile.validate().unwrap();
        assert_eq!(profile.observe_super_weapons, Some(true));
        // The computer's Strategy tick aims its charged Dominator.
        assert!(profile.commands().is_empty());
    }

    #[test]
    fn computer_team_superweapon_examples_leave_the_launch_to_the_computer() {
        for path in [
            "tools/map_observation.ai-iron-curtain.example.json",
            "tools/map_observation.ai-chronosphere.example.json",
        ] {
            let profile: MapCaptureProfile =
                serde_json::from_str(crate::test_fixture::text(path)).unwrap();
            profile.validate().unwrap();
            assert_eq!(profile.observe_super_weapons, Some(true));
            // A computer team's script fires the charged Super.
            assert!(profile.commands().is_empty());
        }
    }

    #[test]
    fn computer_bombard_examples_leave_the_attack_to_the_computer() {
        for path in [
            "tools/map_observation.ai-v3-bombard.example.json",
            "tools/map_observation.ai-dred-bombard.example.json",
        ] {
            let profile: MapCaptureProfile =
                serde_json::from_str(crate::test_fixture::text(path)).unwrap();
            profile.validate().unwrap();
            // Soviet Bombard Medium is enabled for Normal only, Soviet Navy
            // Bombard for Normal and Hard.
            assert_eq!(
                profile.launch.opponents[0].difficulty,
                crate::skirmish_launch::AiDifficulty::Normal
            );
            // A computer team's script orders the spawners' attacks.
            assert!(profile.commands().is_empty());
        }
    }

    #[test]
    fn computer_force_shield_example_answers_the_observers_nuke() {
        let profile: MapCaptureProfile = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.ai-force-shield.example.json",
        ))
        .unwrap();
        profile.validate().unwrap();
        assert_eq!(profile.observe_super_weapons, Some(true));
        // The observer's nuke on the computer's yard alerts the computer,
        // whose Strategy tick fires its charged Force Shield there.
        let [command] = profile.commands() else {
            panic!("one launch");
        };
        assert_eq!(command.issue_after_step, 9010);
        assert!(matches!(
            command.payload,
            Command::LaunchSuperWeapon {
                target_rx: 56,
                target_ry: 56,
                ..
            }
        ));
    }

    #[test]
    fn chronosphere_example_warps_the_source_block_with_a_tactical_click() {
        let profile: MapCaptureProfile = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.chronosphere.example.json",
        ))
        .unwrap();
        profile.validate().unwrap();
        assert_eq!(profile.observe_super_weapons, Some(true));
        // The Chronosphere charges for 6300 frames from the first step; its
        // launch selects the Chrono Warp, and the click at the view's centre
        // (`camera_cell`) fires it.
        let [command] = profile.commands() else {
            panic!("one launch");
        };
        assert_eq!(command.issue_after_step, 6310);
        assert!(matches!(
            command.payload,
            Command::LaunchSuperWeapon {
                target_rx: 41,
                target_ry: 63,
                ..
            }
        ));
        let [gesture] = profile.gestures() else {
            panic!("one click");
        };
        assert_eq!(gesture.issue_after_step, 6320);
        assert!(matches!(
            gesture.gesture,
            MapGesture::Click {
                position: [316, 284]
            }
        ));
        assert_eq!(profile.camera_cell, Some([45, 55]));
    }

    #[test]
    fn rally_profile_reuses_literal_command_serde_and_rejects_ignored_fields() {
        let mut value: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.barracks-output.example.json",
        ))
        .unwrap();
        // Syntax-only supplied producer identity: the ordinary command owner
        // still validates actual ownership/presence at runtime. No diagnostic
        // Archive mutation or caller-specific rally setter is introduced.
        let rally = json!({"issue_after_step": 1159, "owner": "VERA-OBSERVER",
            "payload": {"SetRally": {"rx": 30, "ry": 86, "producer_ids": [17]}}});
        value["commands"].as_array_mut().unwrap().insert(5, rally);
        let profile: MapCaptureProfile = serde_json::from_value(value.clone()).unwrap();
        profile.validate().unwrap();
        assert_eq!(serde_json::to_value(profile).unwrap(), value);
        value["commands"][5]["payload"]["SetRally"]["ignored"] = json!(true);
        assert!(serde_json::from_value::<MapCaptureProfile>(value).is_err());
    }
}
