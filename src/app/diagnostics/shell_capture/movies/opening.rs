//! The existing campaign capture route continues through the actual app
//! runtime. Timer captures explicitly replay a later mission's parsed timer
//! actions after opening unlock; they do not claim those objectives were played.

use super::*;
use crate::app::input::{camera, hotkeys, keyboard};
use crate::app::match_runtime::sim_tick::ExactStepReceipt;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key, KeyCode, KeyLocation, NamedKey};

mod aircraft;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningTarget {
    Locked,
    CameraPan,
    Flash,
    Arrows,
    Unlocked,
    /// Let the normal wall-clock pacer and audio device run through SOV01.
    RealtimeUnlocked,
    RestoreUnlocked,
    /// ALL01, whose first authored action locks input: an unlocked save
    /// loaded from the Esc menu while a replay of the opening holds the lock.
    LoadUnlockedWhileLocked,
    /// A fixed committed production tick, for opening aircraft raster diagnosis.
    AlliedTick(u64),
    /// Observe the ordinary SOV01 simulation after its opening unlock.
    SovietTick(u64),
    /// Issue ordinary naval attack orders, then observe the committed tick.
    SovietNavy(u64),
    /// SOV01 timer actions replayed after opening unlock, then ordinary expiry.
    TimerRunning,
    TimerExpired,
    /// SOV02's ordinary briefing Action55 creates a type-5 alert.
    RadarAlert,
    /// Save/load that naturally created alert, then present the restored radar.
    RadarRestored,
}

impl OpeningTarget {
    pub(super) fn phase(self) -> &'static str {
        match self {
            Self::Locked => "opening-locked",
            Self::CameraPan => "opening-pan",
            Self::Flash => "opening-flash",
            Self::Arrows => "opening-arrows",
            Self::Unlocked => "opening-unlocked",
            Self::RealtimeUnlocked => "opening-realtime-unlocked",
            Self::RestoreUnlocked => "opening-restore-unlocked",
            Self::LoadUnlockedWhileLocked => "opening-load-unlocked-while-locked",
            Self::AlliedTick(_) => "opening-allied-tick",
            Self::SovietTick(_) => "opening-soviet-tick",
            Self::SovietNavy(_) => "soviet-naval-encounter",
            Self::TimerRunning => "opening-timer-running",
            Self::TimerExpired => "opening-timer-expired",
            Self::RadarAlert => "radar-alert",
            Self::RadarRestored => "radar-restored",
        }
    }

    pub(crate) fn side(self) -> CampaignSide {
        match self {
            Self::LoadUnlockedWhileLocked | Self::AlliedTick(_) => CampaignSide::Allied,
            _ => CampaignSide::Soviet,
        }
    }

    fn step_limit(self) -> usize {
        match self {
            Self::LoadUnlockedWhileLocked => 6_000,
            Self::SovietTick(tick) | Self::SovietNavy(tick) => tick as usize + 1,
            Self::TimerRunning | Self::TimerExpired => 8_000,
            _ => 2_500,
        }
    }
}

/// Production steps run after the in-lock load before the readback.
const STEPS_AFTER_LOAD: u32 = 45;

pub(super) struct OpeningCapture {
    target: OpeningTarget,
    save_directory: PathBuf,
    initial_runtime: Option<Value>,
    steps: Vec<ExactStepReceipt>,
    observations: Vec<Value>,
    previous_projection: Option<Value>,
    saw_lock: bool,
    saw_unlock: bool,
    saw_countdown: bool,
    last_running_countdown: Option<Value>,
    timer_action_replay: Option<Value>,
    lock_probe: Option<Value>,
    unlock_probe: Option<Value>,
    locked_mouse_probe: Option<Value>,
    unlocked_mouse_probe: Option<Value>,
    restored: Option<Value>,
    /// LoadUnlockedWhileLocked: the save made before the first lock, the save
    /// made after its unlock, and the Esc-menu load made in the replayed lock.
    pre_lock_save: Option<PathBuf>,
    unlocked_save: Option<PathBuf>,
    load_while_locked: Option<Value>,
    steps_after_load: u32,
    /// The (tick, binary frame) a load installed, until the next step.
    loaded_frame: Option<(u64, u32)>,
    saved_radar: Option<crate::sim::snapshot::saved_local_view::SavedRadarEvents>,
}

impl OpeningCapture {
    pub(super) fn new(target: OpeningTarget, save_directory: PathBuf) -> Self {
        Self {
            target,
            save_directory,
            initial_runtime: None,
            steps: Vec::new(),
            observations: Vec::new(),
            previous_projection: None,
            saw_lock: false,
            saw_unlock: false,
            saw_countdown: false,
            last_running_countdown: None,
            timer_action_replay: None,
            lock_probe: None,
            unlock_probe: None,
            locked_mouse_probe: None,
            unlocked_mouse_probe: None,
            restored: None,
            pre_lock_save: None,
            unlocked_save: None,
            load_while_locked: None,
            steps_after_load: 0,
            loaded_frame: None,
            saved_radar: None,
        }
    }

    pub(super) fn timeout(&self) -> Duration {
        Duration::from_secs(match self.target {
            OpeningTarget::LoadUnlockedWhileLocked
            | OpeningTarget::SovietTick(_)
            | OpeningTarget::SovietNavy(_) => 600,
            OpeningTarget::TimerRunning | OpeningTarget::TimerExpired => 600,
            _ => 180,
        })
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        if matches!(
            self.target,
            OpeningTarget::RadarAlert | OpeningTarget::RadarRestored
        ) {
            let Some(expected) = self.saved_radar.as_ref() else {
                return Ok(false);
            };
            let radar = state
                .match_state
                .match_presentation
                .minimap
                .as_ref()
                .context("restored radar")?;
            let actual = radar.saved_radar_events();
            // after_present may have just resumed the first gameplay frame
            // after load. Wait for its minimap draw before comparing it.
            if actual.last_advanced_frame < expected.last_advanced_frame {
                return Ok(false);
            }
            ensure!(
                actual == *expected,
                "first radar redraw changed the retained alert state: actual={actual:?}, expected={expected:?}"
            );
            let visible = state
                .match_state
                .match_presentation
                .radar_anim
                .as_ref()
                .is_none_or(|radar| radar.is_minimap_visible());
            return Ok(
                visible && (self.target == OpeningTarget::RadarAlert || self.restored.is_some())
            );
        }
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("campaign opening has no installed runtime")?
            .simulation;
        let locked = sim.session().input_locked();
        let view = camera::saved_tactical_view(state);
        Ok(self.saw_lock
            && match self.target {
                OpeningTarget::Locked => locked,
                OpeningTarget::CameraPan => mid_pan(&view),
                OpeningTarget::Flash => sim
                    .entities()
                    .iter_sorted()
                    .any(|(_, entity)| entity.is_active() && entity.flashing.flashed()),
                OpeningTarget::Arrows => sim.anims().any(|(_, anim)| anim.no_effects()),
                OpeningTarget::Unlocked | OpeningTarget::RealtimeUnlocked => {
                    self.saw_unlock && !locked
                }
                OpeningTarget::RestoreUnlocked => {
                    self.saw_unlock && !locked && self.restored.is_some()
                }
                OpeningTarget::LoadUnlockedWhileLocked => {
                    self.load_while_locked.is_some() && self.steps_after_load >= STEPS_AFTER_LOAD
                }
                OpeningTarget::AlliedTick(tick)
                | OpeningTarget::SovietTick(tick)
                | OpeningTarget::SovietNavy(tick) => sim.session().tick() == tick,
                OpeningTarget::TimerRunning => sim.session().scenario_timer_view().is_some(),
                OpeningTarget::TimerExpired => {
                    self.saw_countdown && sim.session().scenario_timer_view().is_none()
                }
                OpeningTarget::RadarAlert | OpeningTarget::RadarRestored => unreachable!(),
            })
    }

    pub(super) fn after_present(&mut self, state: &mut AppState) -> Result<()> {
        if self.initial_runtime.is_none() {
            if self.target == OpeningTarget::RealtimeUnlocked {
                ensure!(
                    state.audio.sfx_player.is_some(),
                    "realtime opening requires audio output"
                );
            }
            self.initial_runtime = Some(if self.target == OpeningTarget::RealtimeUnlocked {
                installed_campaign_runtime_receipt(state)?
            } else {
                campaign_runtime_receipt(state)?
            });
            // A real pointer press before the first authored lock. Action46
            // must cancel its capture while preserving physical button state.
            if !matches!(
                self.target,
                OpeningTarget::RadarAlert | OpeningTarget::RadarRestored
            ) {
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Pressed,
                );
            }
        }
        self.observe(state)?;
        if matches!(
            self.target,
            OpeningTarget::RadarAlert | OpeningTarget::RadarRestored
        ) {
            self.capture_radar_load(state)?;
            if self.saved_radar.is_some() {
                return Ok(());
            }
        }
        let locked = crate::app::input::campaign_input::is_locked(state);
        if !matches!(
            self.target,
            OpeningTarget::RadarAlert | OpeningTarget::RadarRestored
        ) && locked
            && self.lock_probe.is_none()
        {
            self.lock_probe = Some(probe_arrow_edge(state, true)?);
            self.locked_mouse_probe = Some(probe_locked_mouse_edges(state)?);
        } else if !matches!(
            self.target,
            OpeningTarget::RadarAlert | OpeningTarget::RadarRestored
        ) && self.saw_unlock
            && !locked
            && self.unlock_probe.is_none()
        {
            self.unlock_probe = Some(probe_arrow_edge(state, false)?);
            self.unlocked_mouse_probe = Some(probe_unlocked_mouse_edges(state)?);
        }
        // Readback is submitted before this hook. Keep its receipt on the
        // exact state that was drawn, including the first unlocked frame.
        if self.ready(state)? {
            return Ok(());
        }
        if self.target == OpeningTarget::RealtimeUnlocked {
            return Ok(());
        }
        if matches!(self.target, OpeningTarget::SovietNavy(_)) && self.saw_unlock && !locked {
            self.issue_naval_orders(state)?;
        }
        if matches!(
            self.target,
            OpeningTarget::TimerRunning | OpeningTarget::TimerExpired
        ) && self.saw_unlock
            && !locked
            && self.timer_action_replay.is_none()
        {
            self.replay_timer_actions(state)?;
            self.observe(state)?;
        }
        if self.target == OpeningTarget::RestoreUnlocked
            && self.restored.is_none()
            && mid_pan(&camera::saved_tactical_view(state))
        {
            state
                .persistence
                .isolate_capture_saves(&self.save_directory)
                .context("create isolated campaign capture save directory")?;
            let before = camera::saved_tactical_view(state);
            let depth = state.match_state.input.cursor_visibility.level();
            let saved_runtime = installed_campaign_runtime_receipt(state)?;
            let path =
                crate::app::persistence::commands::save_game(state, "SOV01 opening pan", None)
                    .context("save through ordinary production command")?;
            let saved_bytes = std::fs::read(&path).context("read saved capture bytes")?;
            crate::app::persistence::commands::try_load_save_file(state, &path)
                .context("load through ordinary prepared-load/commit owners")?;
            let after = camera::saved_tactical_view(state);
            ensure!(
                before == after,
                "ordinary load changed the retained pan trajectory"
            );
            ensure!(
                state.match_state.input.cursor_visibility.level() == depth,
                "ordinary load changed outgoing cursor display depth"
            );
            let loaded_runtime = installed_campaign_runtime_receipt(state)?;
            ensure!(
                saved_runtime["tick"] == loaded_runtime["tick"]
                    && saved_runtime["binary_frame"] == loaded_runtime["binary_frame"],
                "ordinary load changed the saved simulation frame"
            );
            self.restored = Some(json!({
                "path": path, "byte_length": saved_bytes.len(),
                "sha256": crate::util::sha256::sha256_hex(&saved_bytes),
                "view_before": before, "view_after": after,
                "cursor_depth_before": depth,
                "cursor_depth_after": state.match_state.input.cursor_visibility.level(),
                "saved_runtime": saved_runtime, "loaded_runtime": loaded_runtime,
                "rng_limit": "LoadScenario resets Scenario RNG; Main and MapGen are retained by their existing owners",
                "format": "VERA snapshot; native SAV wire compatibility is not claimed",
            }));
            self.observe(state)?;
        }
        if self.target == OpeningTarget::LoadUnlockedWhileLocked {
            self.drive_load_while_locked(state)?;
        }
        let limit = self.target.step_limit();
        ensure!(
            self.steps.len() < limit,
            "campaign opening did not reach {:?} within {limit} production steps",
            self.target
        );
        let receipt =
            crate::app::match_runtime::sim_tick::advance_in_game_runtime_exact_step(state)
                .context("advance exact ordinary campaign production frame")?;
        // A load installs another frame; the next step starts exactly there.
        let expected = self.loaded_frame.take().or_else(|| {
            self.steps
                .last()
                .map(|previous| (previous.tick_after, previous.binary_frame_after))
        });
        if let Some((tick, binary_frame)) = expected {
            ensure!(
                tick == receipt.tick_before && binary_frame == receipt.binary_frame_before,
                "campaign capture lost its contiguous runtime frame sequence"
            );
        }
        self.steps.push(receipt);
        self.observe(state)?;
        Ok(())
    }

    fn capture_radar_load(&mut self, state: &mut AppState) -> Result<()> {
        if self.saved_radar.is_some() {
            return Ok(());
        }
        let Some(minimap) = state.match_state.match_presentation.minimap.as_ref() else {
            return Ok(());
        };
        let before = minimap.saved_radar_events();
        // SOV02's ordinary briefing: Action55 type5 at waypoint1=(80,58).
        // No radar request or gameplay order is injected by this capture.
        if !before.events.iter().any(|event| {
            event.event_type == crate::sim::radar::RadarEventType::EnemyObjectSensed
                && event.cell == (80, 58)
                && event.needs_draw
        }) {
            return Ok(());
        }
        self.saved_radar = Some(before.clone());
        if self.target == OpeningTarget::RadarAlert {
            return Ok(());
        }
        state
            .persistence
            .isolate_capture_saves(&self.save_directory)?;
        let ring_before = state
            .match_state
            .match_presentation
            .minimap
            .as_mut()
            .unwrap()
            .cycle_radar_event(Instant::now());
        ensure!(
            ring_before.is_some(),
            "authored alert did not enter the review ring"
        );
        let path =
            crate::app::persistence::commands::save_game(state, "SOV02 live radar alert", None)?;
        crate::app::persistence::commands::try_load_save_file(state, &path)?;
        let minimap = state
            .match_state
            .match_presentation
            .minimap
            .as_mut()
            .context("incoming saved radar")?;
        let after = minimap.saved_radar_events();
        ensure!(
            before == after,
            "ordinary load discarded saved radar alerts"
        );
        ensure!(
            minimap.cycle_radar_event(Instant::now()).is_none(),
            "load retained outgoing Spacebar history"
        );
        // The ordinary post-load frame refreshes House-derived radar
        // availability. Freezing before it leaves the newly installed radar
        // housing offline, even though its live alert array loaded correctly.
        let receipt =
            crate::app::match_runtime::sim_tick::advance_in_game_runtime_exact_step(state)?;
        let mut expected = crate::render::radar::ClientRadarEvents::from_saved(before.clone())?;
        expected.advance_to_frame(
            receipt.tick_after,
            &state
                .rules()
                .context("restored radar rules")?
                .radar_event_config,
        );
        self.saved_radar = Some(expected.saved());
        self.steps.push(receipt);
        self.restored = Some(
            json!({"path":path,"radar_before":before,"radar_after_load":after,
            "ring_before":ring_before,"ring_after":null,
            "first_resumed_frame":receipt,
            "scope":"ordinary SOV02 briefing Action55 type5 at waypoint1; explicit campaign admission, no preceding mission completion; normal VERA save/load; first radar redraw checked separately"}),
        );
        Ok(())
    }

    #[cfg(feature = "dev-overlays")]
    fn replay_timer_actions(&mut self, state: &mut AppState) -> Result<()> {
        let runtime = state
            .match_state
            .sim_runtime
            .as_mut()
            .context("timer replay has no runtime")?;
        let sim = &mut runtime.simulation;
        let count = sim
            .dev_replay_scenario_timer_actions(&runtime.resources.rules, "0952F87C")
            .context("SOV01 has no parsed timer actions")?;
        ensure!(
            count == 3 && sim.session().scenario_timer_view() == Some((1800, "MISSION:TmeTimer")),
            "SOV01 timer replay differs from its retail actions"
        );
        self.timer_action_replay = Some(json!({
            "mode": "retail-timer-actions-after-opening-unlock", "trigger": "0952F87C",
            "action_count": count, "tick": sim.session().tick(),
            "binary_frame": sim.session().binary_frame(),
            "scope": "later time-machine/power/Jurassic objectives were not played; only timer actions27/23/103 were replayed",
        }));
        Ok(())
    }

    #[cfg(not(feature = "dev-overlays"))]
    fn replay_timer_actions(&mut self, _state: &mut AppState) -> Result<()> {
        anyhow::bail!("timer action replay requires dev-overlays")
    }

    /// Save before ALL01's first authored lock and again after its unlock,
    /// replay the opening from the first save, then load the unlocked save the
    /// way the Esc menu's Load does (`apply_saved_game_outcome`: open B5, load,
    /// close the menu) while the replayed lock holds.
    fn drive_load_while_locked(&mut self, state: &mut AppState) -> Result<()> {
        use crate::app::persistence::commands::{save_game, try_load_save_file};
        use crate::ui::pause_menu::InGameMenuState;
        if self.load_while_locked.is_some() {
            self.steps_after_load += 1;
            return Ok(());
        }
        let locked = crate::app::input::campaign_input::is_locked(state);
        let Some(pre_lock) = self.pre_lock_save.clone() else {
            ensure!(
                !locked,
                "ALL01 locked input before its first presented frame"
            );
            state
                .persistence
                .isolate_capture_saves(&self.save_directory)
                .context("create isolated campaign capture save directory")?;
            let path = save_game(state, "ALL01 before opening lock", None)
                .context("save before the opening lock")?;
            self.pre_lock_save = Some(path);
            return Ok(());
        };
        let Some(unlocked) = self.unlocked_save.clone() else {
            if self.saw_unlock && !locked {
                let path = save_game(state, "ALL01 after opening unlock", None)
                    .context("save after the opening unlock")?;
                self.unlocked_save = Some(path);
                try_load_save_file(state, &pre_lock)
                    .context("replay the opening from the pre-lock save")?;
                self.loaded_frame = Some(installed_frame(state)?);
                self.observe(state)?;
            }
            return Ok(());
        };
        let depth_before = state.match_state.input.cursor_visibility.level();
        if !locked || depth_before >= 0 {
            return Ok(());
        }
        App::enter_in_game_menu_state(state, InGameMenuState::Menu);
        try_load_save_file(state, &unlocked).context("load the unlocked save in the lock")?;
        App::enter_in_game_menu_state(state, InGameMenuState::Closed);
        self.loaded_frame = Some(installed_frame(state)?);
        let locked_after = crate::app::input::campaign_input::is_locked(state);
        let depth_after = state.match_state.input.cursor_visibility.level();
        let cursor_instances_after =
            crate::app::presentation::ui_overlays::build_software_cursor_instances(state).len();
        ensure!(
            !locked_after && depth_after == 0 && cursor_instances_after == 1,
            "the unlocked save loaded in the lock left the pointer hidden \
             (locked {locked_after}, depth {depth_after}, {cursor_instances_after} cursor sprites)"
        );
        self.load_while_locked = Some(json!({
            "depth_before": depth_before, "locked_after": locked_after,
            "depth_after": depth_after, "cursor_instances_after": cursor_instances_after,
            "right_drag_pans_after": probe_right_drag_pans(state),
        }));
        self.observe(state)?;
        Ok(())
    }

    fn observe(&mut self, state: &AppState) -> Result<()> {
        let facts = opening_observation(state)?;
        if self.target == OpeningTarget::RealtimeUnlocked {
            ensure!(
                facts["game_speed"] == 2,
                "ordinary campaign lost its native speed"
            );
            ensure!(
                facts["eva_event"].as_str().is_none_or(|event| !event.eq_ignore_ascii_case("EVA_PsychicDominatorDetected")),
                "unpowered SOV01 Dominator warning interrupted the opening"
            );
        }
        let locked = facts["input_locked"]
            .as_bool()
            .context("lock observation missing")?;
        self.saw_unlock |= self.saw_lock && !locked;
        self.saw_lock |= locked;
        self.saw_countdown |= !facts["scenario_timer"].is_null();
        if !facts["scenario_timer"].is_null() {
            self.last_running_countdown = Some(json!({
                "tick": facts["tick"], "binary_frame": facts["binary_frame"],
                "timer": facts["scenario_timer"],
            }));
        }
        if matches!(
            self.target,
            OpeningTarget::SovietTick(_) | OpeningTarget::SovietNavy(_)
        ) && self.saw_unlock
            && facts["tick"].as_u64().is_some_and(|tick| tick % 120 != 0)
            && self
                .observations
                .last()
                .is_some_and(|last| last["input_locked"] == facts["input_locked"])
        {
            return Ok(());
        }
        // Simulation time is attached to changes; it does not make every
        // moving pixel or every countdown visit into a new observation row.
        let mut projection = facts.clone();
        projection
            .as_object_mut()
            .expect("observation object")
            .remove("tick");
        projection
            .as_object_mut()
            .expect("observation object")
            .remove("binary_frame");
        projection
            .as_object_mut()
            .expect("observation object")
            .remove("wall_ms");
        if let Some(timer) = projection["scenario_timer"].as_object_mut() {
            timer.remove("remaining");
        }
        projection["camera"]
            .as_object_mut()
            .expect("camera object")
            .remove("focus_progress_bits");
        projection["camera"]
            .as_object_mut()
            .expect("camera object")
            .remove("view_center_world_pixels");
        projection["camera"]
            .as_object_mut()
            .expect("camera object")
            .remove("last_ai_frame");
        for building in projection["building_receivers"]
            .as_array_mut()
            .expect("building observations")
        {
            for slot in building["anim_slots"]
                .as_array_mut()
                .expect("animation slot observations")
            {
                slot.as_object_mut().expect("slot object").remove("frame");
            }
        }
        for timer in projection["super_timers"]
            .as_array_mut()
            .expect("super timer observations")
        {
            timer
                .as_object_mut()
                .expect("timer object")
                .remove("remaining");
        }
        if self.previous_projection.as_ref() != Some(&projection) {
            self.previous_projection = Some(projection);
            self.observations.push(facts);
        }
        Ok(())
    }

    /// A bounded play input for the first retail objective. It uses the client
    /// command queue; damage, navigation, reinforcements and triggers remain
    /// entirely in the ordinary simulation.
    fn issue_naval_orders(&mut self, state: &mut AppState) -> Result<()> {
        use crate::sim::command::Command;
        let sim = &mut state
            .match_state
            .sim_runtime
            .as_mut()
            .context("naval runtime")?
            .simulation;
        let tick = sim.session().tick();
        if tick < 1100 || tick % 30 != 0 {
            return Ok(());
        }
        let viewer = sim.session().current_house().context("campaign player")?;
        let cells = sim
            .resolved_terrain()
            .map(crate::map::resolved_terrain::NativeCellQuery::isolated)
            .context("campaign terrain")?;
        let mut orders = Vec::new();
        for (id, actor) in sim.entities().iter_sorted() {
            if !actor.is_active()
                || actor.lifecycle.in_limbo()
                || sim.interner().resolve(actor.owner()) != "Russians"
                || actor.attack_target.target().is_some()
            {
                continue;
            }
            let kind = sim.interner().resolve(actor.type_ref());
            if !matches!(kind, "SUB" | "SQD" | "DRED") {
                continue;
            }
            let target = sim
                .entities()
                .iter_sorted()
                .filter(|(_, enemy)| enemy.is_active() && !enemy.lifecycle.in_limbo())
                .filter(|(_, enemy)| {
                    !crate::sim::vision::cell_is_shrouded(
                        sim.fog(),
                        &cells,
                        viewer,
                        (enemy.position.rx(), enemy.position.ry()),
                    )
                })
                .filter(|(_, enemy)| {
                    let owner = sim.interner().resolve(enemy.owner());
                    let ty = sim.interner().resolve(enemy.type_ref());
                    if kind == "DRED" {
                        owner == "Alliance" && ty == "GTGCAN"
                    } else {
                        owner == "Americans" && matches!(ty, "CARRIER" | "DEST")
                    }
                })
                .min_by_key(|(_, enemy)| {
                    let dx = i64::from(actor.position.rx()) - i64::from(enemy.position.rx());
                    let dy = i64::from(actor.position.ry()) - i64::from(enemy.position.ry());
                    dx * dx + dy * dy
                })
                .map(|(id, _)| id);
            if let Some(target_id) = target {
                orders.push(Command::Attack {
                    attacker_id: id,
                    target_id,
                });
            }
        }
        for order in orders {
            log::info!("SOV01 capture ordinary player order at {tick}: {order:?}");
            ensure!(
                crate::app::input::commands::schedule_command_in_sim(sim, "Russians", order)
                    .is_some(),
                "naval order rejected"
            );
        }
        Ok(())
    }

    pub(super) fn receipt(&self, state: &AppState) -> Result<Value> {
        ensure!(
            self.ready(state)?,
            "opening readback missed its selected checkpoint"
        );
        Ok(json!({
            "target": self.target.phase(), "initial_runtime": self.initial_runtime,
            "exact_steps": self.steps, "observations": self.observations,
            "saw_lock": self.saw_lock, "saw_unlock": self.saw_unlock,
            "saw_countdown": self.saw_countdown,
            "last_running_countdown": self.last_running_countdown,
            "timer_action_replay": self.timer_action_replay,
            "locked_arrow_probe": self.lock_probe, "unlocked_arrow_probe": self.unlock_probe,
            "locked_mouse_probe": self.locked_mouse_probe,
            "unlocked_mouse_probe": self.unlocked_mouse_probe,
            "save_restore": self.restored, "current": opening_observation(state)?,
            "retained_radar": self.saved_radar,
            "radar_display": state.match_state.match_presentation.radar_anim.as_ref()
                .map(|radar| json!({"phase": format!("{:?}", radar.phase()),
                    "minimap_visible": radar.is_minimap_visible()})),
            "pre_lock_save": self.pre_lock_save, "unlocked_save": self.unlocked_save,
            "load_while_locked": self.load_while_locked,
            "requested_tick": match self.target { OpeningTarget::AlliedTick(tick) | OpeningTarget::SovietTick(tick) | OpeningTarget::SovietNavy(tick) => Some(tick), _ => None },
            "encounter_entities": if matches!(self.target, OpeningTarget::SovietTick(_) | OpeningTarget::SovietNavy(_)) {
                let sim = &state.match_state.sim_runtime.as_ref().context("encounter runtime")?.simulation;
                Some(sim.entities().iter_sorted().map(|(_, entity)| json!({
                    "id": entity.stable_id(), "type": sim.interner().resolve(entity.type_ref()),
                    "owner": sim.interner().resolve(entity.owner()), "active": entity.is_active(),
                    "limbo": entity.lifecycle.in_limbo(),
                    "cell": [entity.position.rx(), entity.position.ry()],
                    "health": entity.health.current(), "mission": format!("{:?}", entity.mission),
                    "target": format!("{:?}", entity.attack_target),
                    "spawn_manager": entity.spawn_manager,
                })).collect::<Vec<_>>())
            } else { None },
            "aircraft_raster": if matches!(self.target, OpeningTarget::AlliedTick(_)) {
                Some(aircraft::observe(state, &self.save_directory)?)
            } else { None },
            "clock_domain": if self.target == OpeningTarget::RealtimeUnlocked {
                "ordinary wall-clock frame pacing and audio service"
            } else {
                "one exact production sim/app frame per presented game frame; audio/message service uses its ordinary wall clock"
            },
        }))
    }
}

fn installed_frame(state: &AppState) -> Result<(u64, u32)> {
    let session = &state
        .match_state
        .sim_runtime
        .as_ref()
        .context("load left no installed runtime")?
        .simulation
        .session();
    Ok((session.tick(), session.binary_frame()))
}

fn mid_pan(view: &crate::sim::snapshot::SavedTacticalView) -> bool {
    let progress = f32::from_bits(view.focus_progress_bits);
    view.focus_rate_bits != 0 && (0.25..1.0).contains(&progress)
}

fn opening_observation(state: &AppState) -> Result<Value> {
    let sim = &state
        .match_state
        .sim_runtime
        .as_ref()
        .context("opening observation has no runtime")?
        .simulation;
    let viewer = crate::app::input::commands::preferred_local_owner_name(state)
        .and_then(|name| sim.interner().get(&name));
    // Read the same CellClass shroud owner as world drawing. Diagnostic misses
    // retain a separate Dummy, so observing a capture never changes the sim.
    let cells = sim
        .resolved_terrain()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    let cell_shrouded = |cell| {
        viewer.zip(cells.as_ref()).map(|(viewer, cells)| {
            crate::sim::vision::cell_is_shrouded(sim.fog(), cells, viewer, cell)
        })
    };
    let mut active_types = std::collections::BTreeMap::<String, usize>::new();
    let mut flashing = Vec::new();
    let mut building_receivers = Vec::new();
    let mut aircraft = Vec::new();
    for (_, entity) in sim
        .entities()
        .iter_sorted()
        .filter(|(_, entity)| entity.is_active())
    {
        let name = sim.interner().resolve(entity.type_ref()).to_owned();
        *active_types.entry(name.clone()).or_default() += 1;
        if entity.category() == crate::map::entities::EntityCategory::Aircraft {
            aircraft.push(json!({
                "id": entity.stable_id(), "type": name,
                "cell": [entity.position.rx(), entity.position.ry()],
                "world_z": entity.position.exact_z_leptons(),
                "anchor": crate::render::locomotor_visual::screen_position(entity),
                "voxel": entity.is_voxel(),
                "display_layer": sim.display_layers().layer_of(entity.stable_id()).map(|layer| layer.index()),
                "crashing": entity.crash.is_crashing(),
                "health": entity.health.current(),
            }));
        }
        if entity.category() == crate::map::entities::EntityCategory::Structure {
            // Observe existing damage/power owners after exact production
            // frames; this must never substitute a receiver or recalculate power.
            building_receivers.push(json!({
                "id": entity.stable_id(), "type": name,
                "owner": sim.interner().resolve(entity.owner()),
                "cell": [entity.position.rx(), entity.position.ry()],
                "health": entity.health.current(),
                "estimated_health": entity.estimated_health.get(),
                "sampled_power_health": entity.building_power_health_sample(),
                "damaged_mode": entity.building_art.damage_state_active(),
                "damage_smoke_system": entity.damage_smoke_system_id,
                "anim_slots": entity.building_art.anim_slots().iter().enumerate()
                    .filter_map(|(slot, id)| {
                        let id = (*id)?;
                        let anim = sim.anim(id)?;
                        Some(json!({"slot": slot, "id": id,
                            "type": sim.interner().resolve(anim.type_id()),
                            "frame": anim.runtime().current_frame,
                            "paused": anim.runtime().paused}))
                    }).collect::<Vec<_>>(),
                "house_power": sim.power_states().get(&entity.owner()).map(|power| json!({
                    "output": power.total_output(), "drain": power.total_drain(),
                    "low_power": power.is_low_power(),
                })),
            }));
        }
        if entity.flashing.flashed() {
            let cell = (entity.position.rx(), entity.position.ry());
            flashing.push(json!({"id": entity.stable_id(), "type": name,
                "cell": cell, "cell_shrouded": cell_shrouded(cell)}));
        }
    }
    let markers = sim
        .anims()
        .filter(|(_, anim)| anim.no_effects())
        .map(|(id, anim)| {
            let cell = (
                crate::util::lepton::lepton_to_cell_packed(anim.world_coord().x) as u16,
                crate::util::lepton::lepton_to_cell_packed(anim.world_coord().y) as u16,
            );
            json!({"id": id, "type": sim.interner().resolve(anim.type_id()),
            "coord": [anim.world_coord().x, anim.world_coord().y, anim.world_coord().z],
            "z_adjust": anim.z_adjust(), "cell": cell,
            "cell_shrouded": cell_shrouded(cell)})
        })
        .collect::<Vec<_>>();
    let teams = sim
        .team_script_vm()
        .teams_in_order()
        .map(|team| {
            json!({
                "id": team.id(), "type": team.team_type_id().map(|id| sim.interner().resolve(id)),
                "script": sim.interner().resolve(team.script_id()), "cursor": team.cursor(),
                "members": team.members().collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let messages = state.match_state.match_presentation.message_list.messages().iter()
        .map(|message| json!({"text": message.text, "rgb": message.rgb, "deadline_ms": message.deadline_ms}))
        .collect::<Vec<_>>();
    Ok(json!({
        "tick": sim.session().tick(), "binary_frame": sim.session().binary_frame(),
        "input_locked": sim.session().input_locked(),
        "scenario_timer": sim.session().scenario_timer_view().map(|(remaining, caption)|
            json!({"remaining": remaining, "caption": caption})),
        "cursor_depth": state.match_state.input.cursor_visibility.level(),
        "camera": camera::saved_tactical_view(state),
        "camera_origin": [state.match_state.input.camera_x, state.match_state.input.camera_y],
        "fog_viewer": viewer.map(|viewer| sim.interner().resolve(viewer)),
        "active_types": active_types, "flashing": flashing, "no_effects_anims": markers,
        "building_receivers": building_receivers, "aircraft": aircraft,
        "super_timers": state.rules().map(|rules|
            crate::sim::superweapon::super_timer_views(sim, rules).into_iter().map(|view|
                json!({"owner": sim.interner().resolve(view.owner()),
                    "type": sim.interner().resolve(view.sw_type()),
                    "remaining": view.frames_left(), "held": view.on_hold(),
                    "recharge": view.recharge_frames(), "place": view.place()}))
                .collect::<Vec<_>>()).unwrap_or_default(),
        "teams": teams, "messages": messages,
        "eva_active": state.audio.sfx_player.as_ref().is_some_and(|sfx| sfx.voices_active()),
        "eva_event": state.audio.sfx_player.as_ref().and_then(|sfx| sfx.current_eva_event()),
        "game_speed": sim.session().game_options().game_speed,
        "wall_ms": crate::app::match_runtime::frame_pacer::wall_clock_ms(),
    }))
}

/// Use the same literal-key entry point as a window event. A changed user
/// binding is reported rather than invoking its unrelated gameplay command.
fn probe_arrow_edge(state: &mut AppState, locked: bool) -> Result<Value> {
    let key = Key::Named(NamedKey::ArrowRight);
    let resolution = state.match_state.input.hotkey_bindings.resolve_event(
        &key,
        KeyLocation::Standard,
        state.match_state.input.hotkey_modifiers,
    );
    let Some(scroll_key) = hotkeys::fallback_scroll_key(resolution) else {
        return Ok(json!({"locked": locked, "custom_binding": true}));
    };
    for edge_state in [ElementState::Pressed, ElementState::Released] {
        keyboard::in_game_key_edge(
            state,
            keyboard::InGameKeyEdge {
                physical: KeyCode::ArrowRight,
                logical: &key,
                unmodified: &key,
                location: KeyLocation::Standard,
                state: edge_state,
                repeat: false,
                text: None,
            },
            false,
        );
        if edge_state.is_pressed() {
            ensure!(
                state.match_state.input.keys_held.contains(&scroll_key) == !locked,
                "ordinary arrow press admission differs from the Scenario lock"
            );
        }
    }
    ensure!(
        !state.match_state.input.keys_held.contains(&scroll_key),
        "ordinary arrow release retained its scroll latch"
    );
    Ok(json!({"locked": locked, "press_admitted": !locked,
        "release_cleared": true, "custom_binding": false}))
}

/// Ordinary pointer edges remain physical input even when6930A0 refuses the
/// tactical message. The native per-frame consumer692F30 reads live buttons
/// through54F5C0->53EC90; fast_scroll.json update_cases pins its priority.
fn probe_locked_mouse_edges(state: &mut AppState) -> Result<Value> {
    use crate::app::input::dispatch::handle_mouse_input;
    ensure!(
        state.match_state.input.tactical_mouse.left_held(),
        "authored lock discarded the pre-lock physical left press"
    );
    ensure!(
        !state.match_state.input.tactical_mouse.captured
            && !state.match_state.input.selection_state.is_band_box_active(),
        "authored lock retained the pre-lock tactical gesture"
    );
    handle_mouse_input(state, MouseButton::Left, ElementState::Released);
    ensure!(
        !state.match_state.input.tactical_mouse.left_held(),
        "physical left release during the input lock was discarded"
    );
    handle_mouse_input(state, MouseButton::Left, ElementState::Pressed);
    ensure!(
        state.match_state.input.tactical_mouse.left_held()
            && !state.match_state.input.tactical_mouse.captured,
        "locked left press must update physical state without taking capture"
    );
    handle_mouse_input(state, MouseButton::Right, ElementState::Pressed);
    ensure!(
        state.match_state.input.tactical_mouse.right_held()
            && !state.match_state.input.tactical_mouse.captured,
        "locked right press must update physical state without taking capture"
    );
    handle_mouse_input(state, MouseButton::Right, ElementState::Released);
    ensure!(
        !state.match_state.input.tactical_mouse.right_held(),
        "physical right release during the input lock was discarded"
    );
    Ok(
        json!({"pre_lock_capture_cancelled": true, "locked_release_cleared": true,
        "locked_press_recorded": true, "left_held_until_unlock": true,
        "locked_press_captured": false}),
    )
}

/// Whether an ordinary right drag over the battlefield requests a pan, that
/// is, whether the mouse reaches the tactical view.
fn probe_right_drag_pans(state: &mut AppState) -> bool {
    use crate::app::input::dispatch::{handle_cursor_moved_in_game, handle_mouse_input};
    let cursor = (
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    state.match_state.input.cursor_x = 200.0;
    state.match_state.input.cursor_y = 200.0;
    handle_cursor_moved_in_game(state);
    handle_mouse_input(state, MouseButton::Right, ElementState::Pressed);
    state.match_state.input.cursor_x = 280.0;
    handle_cursor_moved_in_game(state);
    let pan = camera::poll_mouse_scroll(state);
    handle_mouse_input(state, MouseButton::Right, ElementState::Released);
    (
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    ) = cursor;
    handle_cursor_moved_in_game(state);
    pan != (0.0, 0.0)
}

fn probe_unlocked_mouse_edges(state: &mut AppState) -> Result<Value> {
    use crate::app::input::dispatch::{handle_cursor_moved_in_game, handle_mouse_input};
    ensure!(
        state.match_state.input.tactical_mouse.left_held(),
        "physical left press did not survive until authored unlock"
    );
    let cursor = (
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    state.match_state.input.cursor_x = 200.0;
    state.match_state.input.cursor_y = 200.0;
    handle_cursor_moved_in_game(state);
    handle_mouse_input(state, MouseButton::Right, ElementState::Pressed);
    state.match_state.input.cursor_x = 240.0;
    handle_cursor_moved_in_game(state);
    ensure!(
        camera::poll_mouse_scroll(state) == (0.0, 0.0),
        "right drag panned despite the left button held across unlock"
    );
    handle_mouse_input(state, MouseButton::Right, ElementState::Released);
    handle_mouse_input(state, MouseButton::Left, ElementState::Released);
    handle_mouse_input(state, MouseButton::Right, ElementState::Pressed);
    state.match_state.input.cursor_x = 280.0;
    handle_cursor_moved_in_game(state);
    let pan = camera::poll_mouse_scroll(state);
    ensure!(
        pan.0 != 0.0 || pan.1 != 0.0,
        "ordinary right drag remained blocked after physical releases"
    );
    handle_mouse_input(state, MouseButton::Right, ElementState::Released);
    ensure!(
        !state.match_state.input.tactical_mouse.captured
            && !state.match_state.input.tactical_mouse.left_held()
            && !state.match_state.input.tactical_mouse.right_held(),
        "pointer regression probe retained a physical button or capture"
    );
    (
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    ) = cursor;
    handle_cursor_moved_in_game(state);
    Ok(json!({"held_left_blocks_right_pan": true,
        "released_buttons_admit_right_pan": true, "capture_released": true,
        "camera_request_committed": false}))
}
