//! Driving a map observation from the capture session: audio context, preparation,
//! the per-step loop and issuing scheduled commands and gestures.

use super::*;

impl TacticalCaptureSession {
    fn set_map_audio_context(&self, state: &mut AppState) -> Result<()> {
        if self
            .request
            .map_profile()
            .is_some_and(|profile| profile.value.observe_audio.is_some())
        {
            let clock = MapClock::capture(state)?;
            state
                .audio
                .sfx_player
                .as_mut()
                .context("PCM capture lost its SFX device")?
                .set_pcm_observation_context(PcmObservationContext {
                    completed_steps: self.exact_step_receipts.len() as u64,
                    simulation_tick: clock.simulation_tick,
                    binary_frame: clock.binary_frame,
                });
        }
        Ok(())
    }

    pub(super) fn map_state(&self) -> Result<&MapObservation> {
        match &self.controller {
            CaptureController::Map(map) => Ok(map.as_ref()),
            _ => bail!("map controller is absent"),
        }
    }

    pub(super) fn map_state_mut(&mut self) -> Result<&mut MapObservation> {
        match &mut self.controller {
            CaptureController::Map(map) => Ok(map.as_mut()),
            _ => bail!("map controller is absent"),
        }
    }

    pub(in crate::app::diagnostics::tactical_capture::session) fn prepare_map_observation(
        &mut self,
        state: &mut AppState,
    ) -> Result<()> {
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
        state
            .platform
            .game_config
            .as_ref()
            .context("map observation requires config.toml")?;
        ensure!(
            state.renderer.upscale_pass.is_none(),
            "map observation requires native-resolution rendering (no zoom)"
        );
        if let Some(presentation) = profile.presentation {
            presentation.apply(state);
        }
        let cwd = std::env::current_dir()?;
        let inputs = json!({
            "config": artifact(&cwd.join("config.toml"), "config.toml")?,
            "executable": artifact(&std::env::current_exe()?, "capture executable")?,
        });
        self.map_state_mut()?.inputs = Some(inputs);
        state.diag.use_map_presentation_clock()?;
        state.match_state.input.cursor_x = 0.0;
        state.match_state.input.cursor_y = 0.0;
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        state.platform.frame_pacer.reanchor(now_ms);
        Ok(())
    }

    pub(in crate::app::diagnostics::tactical_capture::session) fn drive_map_observation(
        &mut self,
        state: &mut AppState,
    ) -> Result<()> {
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
                crate::app::match_bootstrap::accepted_tick_is_admitted(
                    Some(startup),
                    Some(receipt)
                ),
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
            // The receipt above seals the requested display handle. Runtime
            // ownership uses the same collision-safe slots as normal startup.
            let launch_slots =
                crate::sim::scenario_bootstrap::normalized_launch_slots(&profile.launch);
            ensure!(
                state.match_state.local_player_owner() == Some(launch_slots[0].owner_name.as_str()),
                "loaded local owner differs from requested launch"
            );
            validate_loaded_resources(state)?;
            let sim = &state
                .match_state
                .sim_runtime
                .runtime()
                .context("simulation absent")?
                .simulation;
            ensure!(
                sim.session().tick() == 0
                    && sim.session().total_sim_ms() == 0
                    && sim.session().binary_frame() == 0
                    && sim.session().seed() == u64::from(profile.seed)
                    && sim.input_delay_ticks() == u64::from(profile.input_delay_ticks),
                "live simulation is not the requested tick-zero state"
            );
            validate_game_options(sim, &profile.launch)?;
            let slots: Vec<_> = sim
                .session()
                .start_slot_houses()
                .iter()
                .map(|(slot, house_id)| {
                    let house = sim.houses().get(house_id);
                    json!({"slot": slot, "house": sim.interner().resolve(*house_id),
                    "waypoint": sim.session().mp_start_waypoints().get(slot),
                    "country": house.and_then(|h| h.country()).map(|c| sim.interner().resolve(c)),
                    "human": house.map(|h| h.is_human()), "difficulty": house.map(|h| h.difficulty())})
                })
                .collect();
            for start in std::iter::once(profile.launch.local_slot().start_position).chain(
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
                    sim.session()
                        .mp_start_waypoints()
                        .contains_key(&u32::from(index))
                        && sim
                            .session()
                            .start_slot_houses()
                            .contains_key(&u32::from(index)),
                    "requested start {index} was not installed in the loaded map"
                );
            }
            let loaded_session = json!({"map_name": sim.session().map_name(), "theater": sim.session().theater(),
                "options": sim.session().game_options(), "start_slots": slots, "map_waypoints": sim.session().mp_start_waypoints()});
            // Simulation::intern_rule_type_ids owns these handles. Observation
            // reads the rules-owned names and installed handles without adding
            // an interner entry or translating a production command.
            let rules = state.rules().context("loaded rules absent")?;
            let mut rule_types = Vec::new();
            for (category, names) in production_type_names(rules) {
                for name in names {
                    let handle = sim
                        .interner()
                        .get(name)
                        .with_context(|| format!("rule type {name:?} was not preinterned"))?;
                    rule_types.push(json!({"type_id": name, "interned_id": handle.index(),
                        "category": category}));
                }
            }
            profile.validate_observed_rule_types(&rule_types)?;
            if let Some(types) = &profile.observe_anim_types {
                for name in types {
                    ensure!(
                        rules.art().anim_runtime_config(name).is_some(),
                        "observed animation type {name:?} is absent from loaded ART"
                    );
                }
            }
            // Owner strings must name real loaded Houses before the ordinary
            // input owner is allowed to intern a command receiver.
            for owner in profile.observe_owners().iter().map(String::as_str).chain(
                profile
                    .commands()
                    .iter()
                    .map(|command| command.owner.as_str()),
            ) {
                ensure!(
                    sim.interner()
                        .get(owner)
                        .is_some_and(|id| sim.houses().contains_key(&id)),
                    "observation owner {owner:?} is absent from the loaded Houses"
                );
            }
            let camera_cell = profile.camera_cell;
            let cursor_position = profile.cursor_position;
            let audio_config = profile.observe_audio.as_ref().map(MapAudioProfile::config);
            let allow_load_segments = profile.allow_load_segments == Some(true);
            let keyboard_bindings: Vec<_> = if profile.observes_local_input() {
                crate::app::input::hotkeys::catalog::registered_commands()
                    .iter()
                    .map(|metadata| MapKeyboardBinding {
                        command: metadata.ini_name,
                        first_key: state
                            .match_state
                            .input
                            .hotkey_bindings
                            .first_key(metadata.command),
                    })
                    .collect()
            } else {
                Vec::new()
            };
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
                ) || matches!(
                    source,
                    crate::map::source::LoadedMapSource::Generated {
                        seed_source: Some(seed_source), ..
                    } if seed_source.payload_len > 0
                ),
                "map observation requires consumed map or nonempty seed-file bytes"
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
            self.map_state_mut()?.sample_count += keyboard_bindings.len();
            self.map_state_mut()?.keyboard_bindings = keyboard_bindings;
            self.map_state_mut()?.allow_load_segments = allow_load_segments;
            if let Some(config) = audio_config {
                state
                    .audio
                    .sfx_player
                    .as_mut()
                    .context("PCM observation requires the production SFX device")?
                    .observe_pcm(config)
                    .map_err(anyhow::Error::msg)?;
                self.set_map_audio_context(state)?;
            }
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
            self.set_map_audio_context(state)?;
            self.issue_map_commands(state)?;
            self.issue_map_gestures(state)?;
            self.set_map_audio_context(state)?;
            self.advance_exact_step(state)?;
            self.set_map_audio_context(state)?;
            self.record_map_frame(state)?;
        }
        if self.exact_step_receipts.len() == requested {
            let tail_limit = self
                .request
                .map_profile()
                .and_then(|profile| profile.value.observe_audio.as_ref())
                .map(|audio| audio.completion_tail_ms);
            if let Some(tail_limit) = tail_limit
                && self.map_state()?.audio.is_none()
            {
                let started = *self
                    .map_state_mut()?
                    .audio_tail_started
                    .get_or_insert_with(Instant::now);
                let elapsed = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                let sfx = state
                    .audio
                    .sfx_player
                    .as_mut()
                    .context("PCM completion lost its SFX device")?;
                let settled = sfx.observed_pcm_settled();
                if !settled && elapsed < tail_limit {
                    self.failure_stage = "audio-completion-tail".to_owned();
                    return Ok(());
                }
                let report = sfx
                    .finish_pcm_observation()
                    .context("PCM recorder absent at completion")?;
                let truncated =
                    report.truncated || report.outputs.iter().any(|output| output.truncated);
                self.map_state_mut()?.audio = Some(pcm_report(report, elapsed, settled));
                ensure!(
                    settled,
                    "observed SFX did not complete within its wall-clock audio tail"
                );
                ensure!(
                    !truncated,
                    "PCM observation exhausted a declared event/sample/action budget"
                );
            }
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
                .runtime()
                .context("command simulation absent")?
                .simulation
                .session()
                .tick();
            ensure!(
                issued_tick == self.map_state()?.expected_clock(completed_steps, true).0,
                "command issue is outside exact-step boundary"
            );
            // Resolve against the current owner state at this issue boundary,
            // including objects produced by earlier ordinary commands.
            let sim = &state
                .match_state
                .sim_runtime
                .runtime()
                .context("command simulation absent")?
                .simulation;
            let rules = state.rules().context("command rules absent")?;
            let (payload, bindings) = command.resolve(sim, rules)?;
            let sample_count = self
                .map_state()?
                .sample_count
                .checked_add(bindings.as_ref().map_or(0, std::collections::BTreeMap::len))
                .context("command binding sample count overflow")?;
            ensure!(
                sample_count <= MAX_OBSERVATION_SAMPLES,
                "command bindings exceed observation sample budget"
            );
            // The sole ordinary input producer owns envelope encoding, stamping
            // and queuing. This diagnostic does not add the input-delay setting
            // or bypass ordinary House/actor admission in the next frame.
            let execute_tick = crate::app::input::commands::try_schedule_command(
                state,
                &command.owner,
                payload.clone(),
            )
            .context("ordinary command producer refused scheduled input")?;
            ensure!(
                execute_tick == issued_tick,
                "ordinary producer changed the issue stamp"
            );
            let map = self.map_state_mut()?;
            map.sample_count = sample_count;
            map.commands.push(MapCommandReceipt {
                ordinal: map.commands.len(),
                issue_after_step: command.issue_after_step,
                issued_simulation_tick: issued_tick,
                envelope_execute_tick: execute_tick,
                owner: command.owner,
                payload,
                bindings,
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
                .runtime()
                .context("gesture simulation absent")?
                .simulation;
            let issued_simulation_tick = sim.session().tick();
            let issued_binary_frame = sim.session().binary_frame();
            let clock_before = MapClock::capture(state)?;
            let expected_clock = self.map_state()?.expected_clock(completed_steps, true);
            ensure!(
                (issued_simulation_tick, issued_binary_frame) == expected_clock,
                "gesture issue is outside exact-step boundary"
            );
            let pending_before = sim.pending_command_snapshot();
            let local_input = profile.observes_local_input();
            let before = MapInputObservation::capture(state, local_input)?;
            let right = scheduled.gesture.is_right_button();
            let mut minimap = None;
            let mut held_cursor = None;
            let (
                left_press_captured,
                right_press_captured,
                band_box_before_release,
                sidebar,
                keyboard,
                command_bar,
            ) = if let MapGesture::Key {
                key,
                modifiers,
                edge,
            } = &scheduled.gesture
            {
                (
                    false,
                    None,
                    false,
                    None,
                    Some(MapKeyboardGestureReceipt::dispatch(
                        state,
                        key,
                        modifiers.as_deref().unwrap_or_default(),
                        *edge,
                    )?),
                    None,
                )
            } else {
                let sidebar_before = match &scheduled.gesture {
                    MapGesture::Sidebar { .. } => Some(MapSidebarObservation::capture(state)?),
                    _ => None,
                };
                let mut command_bar = match &scheduled.gesture {
                    MapGesture::CommandBar { command } => {
                        Some(MapCommandBarGestureReceipt::capture(state, command)?)
                    }
                    _ => None,
                };
                let locked_thumb = command_bar.as_ref().is_some_and(|(receipt, _)| {
                    receipt
                        .thumb
                        .as_ref()
                        .is_some_and(|thumb| thumb.before.input_locked)
                });
                let (from, to, sidebar_slot) = match &scheduled.gesture {
                    MapGesture::MinimapObject { entity_id } => {
                        let position = crate::app::presentation::sidebar_render::minimap_object_click_position(state, *entity_id)
                            .context("requested object has no retained clickable radar pixel")?;
                        (position, position, None)
                    }
                    MapGesture::Sidebar { target, .. } => {
                        let (position, slot) =
                            sidebar_before
                                .as_ref()
                                .expect("sidebar captured")
                                .resolve(target, [state.render_width(), state.render_height()])?;
                        (position, position, Some(slot))
                    }
                    MapGesture::CommandBar { .. } => {
                        let position = command_bar
                            .as_ref()
                            .expect("command bar captured")
                            .0
                            .resolved_position;
                        (position, position, None)
                    }
                    _ => {
                        let (from, to) =
                            scheduled.gesture.points().expect("tactical gesture points");
                        (from, to, None)
                    }
                };
                let move_cursor = |state: &mut AppState, [x, y]: [u32; 2]| {
                    // Profile coordinates are render-target pixels already. The
                    // OS window/upscale conversion is not a second input source.
                    state.match_state.input.cursor_x = x as f32;
                    state.match_state.input.cursor_y = y as f32;
                    crate::app::input::tooltips::on_mouse_move(state);
                    crate::app::input::dispatch::handle_cursor_moved_in_game(state);
                };
                let mouse_button = if right {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                };
                let button = |state: &mut AppState, edge| {
                    crate::app::input::tooltips::on_button_event(state);
                    crate::app::input::dispatch::handle_mouse_input(state, mouse_button, edge);
                };
                let modifier_keys = literal_modifiers(scheduled.gesture.modifiers())?;
                literal_modifier_edges(state, &modifier_keys, true);
                move_cursor(state, from);
                if scheduled.gesture.is_minimap() {
                    let target =
                        crate::app::presentation::sidebar_render::minimap_camera_target(state)
                            .context("minimap point has no current native camera target")?;
                    if let MapGesture::MinimapObject { entity_id } = scheduled.gesture {
                        ensure!(
                            target.tracker_hit == Some(entity_id),
                            "resolved radar point no longer selects the requested object"
                        );
                    }
                    minimap = Some(MapMinimapGestureReceipt {
                        resolved_position: from,
                        tracker_hit: target.tracker_hit,
                        cell: [target.cell.0, target.cell.1],
                        world_leptons: [
                            target.world_leptons.0,
                            target.world_leptons.1,
                            target.world_leptons.2,
                        ],
                    });
                }
                button(state, ElementState::Pressed);
                if let Some((receipt, handle)) = &mut command_bar
                    && let Some(thumb) = &mut receipt.thumb
                {
                    thumb.press = Some(MapCommandBarThumbEdge::capture(state, *handle)?);
                }
                let mouse = &state.match_state.input.tactical_mouse;
                let gadgets = &state.match_state.match_presentation.in_game_gadgets;
                // A consumed gadget press still reaches the shared physical
                // button owner; only tactical capture depends on its route.
                let physical_press_observed =
                    mouse.left_held() == !right && mouse.right_held() == right;
                let press_captured = !locked_thumb
                    && physical_press_observed
                    && if matches!(
                        scheduled.gesture,
                        MapGesture::Sidebar { .. } | MapGesture::CommandBar { .. }
                    ) {
                        let expected = if let MapGesture::Sidebar { target, .. } =
                            &scheduled.gesture
                        {
                            let slot = sidebar_slot.expect("resolved sidebar slot");
                            match target {
                                MapSidebarTarget::Tab { .. } => {
                                    gadgets.handles.as_ref().map(|handles| handles.tabs[slot])
                                }
                                MapSidebarTarget::Cameo { .. } => gadgets.cameos.get(slot).copied(),
                                MapSidebarTarget::ScrollUp {} => {
                                    gadgets.handles.as_ref().map(|handles| handles.scroll_up)
                                }
                                MapSidebarTarget::ScrollDown {} => {
                                    gadgets.handles.as_ref().map(|handles| handles.scroll_down)
                                }
                                MapSidebarTarget::Repair {} => {
                                    gadgets.handles.as_ref().map(|handles| handles.repair)
                                }
                                MapSidebarTarget::Sell {} => {
                                    gadgets.handles.as_ref().map(|handles| handles.sell)
                                }
                            }
                        } else {
                            command_bar.as_ref().map(|(_, handle)| *handle)
                        };
                        let routed = gadgets.out.consumed_by.or(gadgets.focus.sticky);
                        expected.is_some() && routed == expected && !mouse.captured
                    } else if scheduled.gesture.is_minimap() {
                        state.match_state.input.minimap_press_active() && !mouse.captured
                    } else {
                        // A plain click lands on the battlefield, or on a
                        // button the side's layout added.
                        mouse.captured
                            || gadgets
                                .out
                                .consumed_by
                                .or(gadgets.focus.sticky)
                                .is_some_and(|handle| gadgets.is_layout_button(handle))
                    };
                if matches!(scheduled.gesture, MapGesture::Drag { .. }) {
                    let shown = |state: &AppState| {
                        crate::app::presentation::ui_overlays::shown_cursor_id(state)
                            .map(|id| format!("{id:?}"))
                    };
                    let press = shown(state);
                    move_cursor(state, to);
                    held_cursor = Some(MapHeldCursorReceipt {
                        press,
                        before_release: shown(state),
                    });
                }
                let band_box_before_release =
                    state.match_state.input.selection_state.is_band_box_active();
                button(state, ElementState::Released);
                literal_modifier_edges(state, &modifier_keys, false);
                if let Some((receipt, handle)) = &mut command_bar
                    && let Some(thumb) = &mut receipt.thumb
                {
                    thumb.release = Some(MapCommandBarThumbEdge::capture(state, *handle)?);
                }
                move_cursor(state, neutral);
                if let Some((receipt, handle)) = &mut command_bar
                    && let Some(thumb) = &mut receipt.thumb
                {
                    thumb.after = Some(MapCommandBarThumbState::capture(state, *handle)?);
                }
                ensure!(
                    press_captured || locked_thumb,
                    "gesture mouse press did not reach its retained input target"
                );
                let sidebar = sidebar_before
                    .map(|before| -> Result<_> {
                        let after = MapSidebarObservation::capture(state)?;
                        let toggle = match &scheduled.gesture {
                            MapGesture::Sidebar { target, .. } => {
                                before.toggle(target).map(|toggle| MapSidebarToggleReceipt {
                                    before: toggle.clone(),
                                    after: after.toggle(target).expect("same target").clone(),
                                })
                            }
                            _ => None,
                        };
                        Ok(MapSidebarGestureReceipt {
                            resolved_position: from,
                            before,
                            after,
                            toggle,
                        })
                    })
                    .transpose()?;
                (
                    !right && press_captured,
                    right.then_some(press_captured),
                    band_box_before_release,
                    sidebar,
                    None,
                    command_bar.map(|(receipt, _)| receipt),
                )
            };
            if scheduled.gesture.is_quickload() {
                // The key queues the ordinary two-phase loader. This sealed
                // gesture must finish its loading frame and restoration before
                // recording the rewound clock, without advancing simulation.
                ensure!(
                    crate::app::persistence::commands::pump_saved_game_load(state) == Some(true),
                    "literal quickload did not present its loading frame"
                );
                ensure!(
                    crate::app::persistence::commands::pump_saved_game_load(state) == Some(false),
                    "literal quickload did not finish its queued restoration"
                );
            }
            let neutral_input_restored = crate::app::input::camera::camera_input_idle(state)
                && state.match_state.input.hotkey_modifiers.is_empty()
                && !state.match_state.input.selection_state.is_band_box_active()
                && [
                    state.match_state.input.cursor_x,
                    state.match_state.input.cursor_y,
                ] == neutral.map(|value| value as f32);
            ensure!(
                band_box_before_release == matches!(scheduled.gesture, MapGesture::Drag { .. }),
                "gesture did not produce its requested click/band-drag input state"
            );
            ensure!(
                neutral_input_restored,
                "gesture left retained input active after release"
            );
            let after = MapInputObservation::capture(state, local_input)?;
            let clock_after = MapClock::capture(state)?;
            if scheduled.gesture.is_quickload() {
                ensure!(
                    self.map_state()?.allow_load_segments
                        && self.map_state()?.load_segments.len() < 16,
                    "quickload segment is not admitted or exceeds its 16-transition bound"
                );
                ensure!(
                    clock_after.simulation_tick < clock_before.simulation_tick
                        && clock_after.binary_frame < clock_before.binary_frame
                        && clock_after.total_simulation_ms < clock_before.total_simulation_ms,
                    "literal quickload did not restore an earlier simulation clock"
                );
                let restored_audio_state = self
                    .request
                    .map_profile()
                    .filter(|profile| profile.value.observe_audio.is_some())
                    .map(|profile| sound_state(state, &profile.value))
                    .transpose()?;
                let map = self.map_state_mut()?;
                map.observe_load_segment(MapLoadSegment {
                    after_step: completed_steps,
                    gesture_ordinal: map.gestures.len(),
                    before: clock_before,
                    after: clock_after,
                    restored_audio_state,
                })?;
            }
            let sim = &state
                .match_state
                .sim_runtime
                .runtime()
                .context("gesture simulation disappeared")?
                .simulation;
            ensure!(
                scheduled.gesture.is_quickload()
                    || (sim.session().tick() == issued_simulation_tick
                        && sim.session().binary_frame() == issued_binary_frame),
                "gesture advanced simulation outside the exact-step owner"
            );
            let pending_after = sim.pending_command_snapshot();
            ensure!(
                scheduled.gesture.is_quickload() || pending_after.starts_with(&pending_before),
                "gesture changed previously queued commands"
            );
            let queued_commands: Vec<_> = pending_after
                .into_iter()
                .skip(if scheduled.gesture.is_quickload() {
                    usize::MAX
                } else {
                    pending_before.len()
                })
                .map(|envelope| MapGestureCommandReceipt {
                    owner: sim.interner().resolve(envelope.owner).to_owned(),
                    execute_tick: envelope.execute_tick,
                    payload: envelope.payload,
                })
                .collect();
            let map = self.map_state_mut()?;
            let sample_count = map
                .sample_count
                .checked_add(before.sample_count())
                .and_then(|count| count.checked_add(after.sample_count()))
                .and_then(|count| count.checked_add(queued_commands.len()))
                .and_then(|count| {
                    count.checked_add(
                        command_bar
                            .as_ref()
                            .map_or(0, MapCommandBarGestureReceipt::sample_count),
                    )
                })
                .and_then(|count| count.checked_add(usize::from(minimap.is_some())))
                .and_then(|count| {
                    count.checked_add(sidebar.as_ref().map_or(0, |receipt| {
                        receipt.before.sample_count()
                            + receipt.after.sample_count()
                            + usize::from(receipt.toggle.is_some()) * 2
                    }))
                })
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
                right_press_captured,
                band_box_before_release,
                neutral_input_restored,
                queued_commands,
                sidebar,
                keyboard,
                command_bar,
                minimap,
                held_cursor,
            });
        }
    }
}
