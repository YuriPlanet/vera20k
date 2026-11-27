//! `MapObservation`, the run's accumulator: its clock, load segments, audio
//! state, draw times and tooltip reads, and the sample budget it enforces.

use super::*;

#[derive(Default)]
pub(in crate::app::diagnostics::tactical_capture::session) struct MapObservation {
    pub(in crate::app::diagnostics::tactical_capture::session) initial: Option<Value>,
    pub(super) inputs: Option<Value>,
    pub(super) loaded_session: Option<Value>,
    pub(super) rule_types: Vec<Value>,
    pub(super) draws: Vec<MapDrawTime>,
    pub(super) electric_bolt_draws: Vec<MapElectricBoltDrawObservation>,
    pub(super) radiation_beam_draws: Vec<MapRadiationBeamDrawObservation>,
    pub(super) commands: Vec<MapCommandReceipt>,
    pub(super) gestures: Vec<MapGestureReceipt>,
    pub(super) keyboard_bindings: Vec<MapKeyboardBinding>,
    pub(super) sidebar_frames: Vec<MapSidebarFrame>,
    pub(super) frames: Vec<MapFrameObservation>,
    pub(super) observed_ids: BTreeSet<u64>,
    pub(super) sample_count: usize,
    pub(super) allow_load_segments: bool,
    pub(super) load_segments: Vec<MapLoadSegment>,
    pub(super) audio_tail_started: Option<Instant>,
    pub(super) audio_tail_draws: u32,
    pub(super) audio: Option<Value>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct MapClock {
    pub(super) simulation_tick: u64,
    pub(super) binary_frame: u32,
    pub(super) total_simulation_ms: u64,
}

impl MapClock {
    pub(super) fn capture(sim_runtime: &SimHost) -> Result<Self> {
        let session = &sim_runtime
            .runtime()
            .context("clock simulation absent")?
            .simulation
            .session();
        Ok(Self {
            simulation_tick: session.tick(),
            binary_frame: session.binary_frame(),
            total_simulation_ms: session.total_sim_ms(),
        })
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MapLoadSegment {
    pub(super) after_step: u64,
    pub(super) gesture_ordinal: usize,
    pub(super) before: MapClock,
    pub(super) after: MapClock,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) restored_audio_state: Option<Value>,
}

pub(super) fn sound_state(
    sim_runtime: &SimHost,
    sfx_player: Option<&crate::audio::sfx::SfxPlayer>,
    profile: &MapCaptureProfile,
) -> Result<Value> {
    let sim = &sim_runtime
        .runtime()
        .context("sound observation simulation absent")?
        .simulation;
    let rng = sim.rng_views();
    let actors = sim.entities().iter_sorted().filter_map(|(id, entity)| {
        let owner = sim.interner().resolve(entity.owner());
        let type_id = sim.interner().resolve(entity.type_ref());
        (profile.observe_owners().iter().any(|watch| watch == owner)
            && profile.observe_types.as_ref().is_none_or(|types| types.iter().any(|watch| watch == type_id)))
            .then(|| json!({"stable_id": id, "body_counter": entity.body_frame_counter,
                "active": entity.move_sound.is_active(), "countdown": entity.move_sound.countdown(),
                "gattling_stage": entity.gattling.stage(), "gattling_value": entity.gattling.value()}))
    }).collect::<Vec<_>>();
    let mut coordinate_requests = sfx_player.map(|sfx| sfx.coordinate_sound_observation());
    if let (Some(audio), Some(rows)) = (&profile.observe_audio, &mut coordinate_requests) {
        audio.retain_coordinate_requests(rows);
    }
    let snapshot = json!({"main_rng_cursor": [rng.main.index_a, rng.main.index_b],
        "scenario_rng_cursor": [rng.scenario.index_a, rng.scenario.index_b], "actors": actors,
        "coordinate_requests": coordinate_requests});
    ensure!(
        sound_state_sample_count(&snapshot) <= MAX_OBSERVATION_SAMPLES,
        "sound state exceeds sample budget"
    );
    Ok(snapshot)
}

pub(super) fn sound_state_sample_count(state: &Value) -> usize {
    2 + state["actors"].as_array().map_or(0, Vec::len)
        + state["coordinate_requests"].as_array().map_or(0, Vec::len)
}

pub(super) fn pcm_report(report: PcmObservationReport, tail_ms: u64, settled: bool) -> Value {
    let mut result = report.into_json();
    result["policy"] = json!(AUDIO_POLICY);
    result["point"] = json!("post_player_pre_device_mixer");
    result["completion_tail_ms"] = json!(tail_ms);
    result["settled"] = json!(settled);
    result
}

#[derive(Debug, Serialize)]
pub(super) struct MapDrawTime {
    pub(super) completed_steps: u64,
    pub(super) radar_ms: u64,
    pub(super) tooltip_ms: u64,
    pub(super) message_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) simulation_tick: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tooltip: Option<Value>,
}

/// Read the actual post-render service and its existing coordinate/fog owners.
/// No hit-test, tooltip update, canonical Dummy stamp or presentation mutation.
pub(super) fn capture_tooltip(match_state: &MatchState) -> Result<Value> {
    let view = match_state
        .sim_view()
        .context("tooltip observation lost simulation")?;
    let terrain = view
        .resolved_terrain()
        .context("tooltip observation lost terrain")?;
    let viewer = view
        .session()
        .current_house()
        .context("tooltip observation lost current House")?;
    let input = &match_state.input;
    let world = crate::app::match_runtime::sim_tick::screen_point_to_world(
        &match_state.input,
        input.cursor_x,
        input.cursor_y,
    );
    let point = crate::map::terrain::screen_to_plane_leptons(world.0, world.1);
    let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
    let cell = cells.lookup_world(point.x, point.y);
    let active = match_state.match_presentation.tooltips.active().map(|tip| {
        json!({"id": tip.id, "text": tip.text, "cursor_px": [tip.x, tip.y],
               "shown_at_ms": tip.shown_at_ms})
    });
    Ok(json!({
        "cursor_px": input.cursor_px(),
        "camera_top_left": [input.camera_x, input.camera_y],
        "camera_zoom": input.zoom_level,
        "cursor_coordinates": input.cursor_coordinates,
        "captured": input.tactical_mouse.captured,
        "input_locked": view.session().input_locked(),
        "camera_blocks_tooltip": input.pending_camera_scroll.blocks_tactical_tooltip(),
        "plane_xyz": point.to_array(), "plane_cell": cells.coord(cell),
        "ground_unshrouded": crate::sim::vision::cell_ground_mapped(view.fog(), terrain, viewer, cell),
        "active": active,
    }))
}

impl MapObservation {
    pub(super) fn observe_load_segment(&mut self, segment: MapLoadSegment) -> Result<()> {
        let count = self
            .sample_count
            .checked_add(
                segment
                    .restored_audio_state
                    .as_ref()
                    .map_or(0, sound_state_sample_count),
            )
            .context("restored observation sample count overflow")?;
        ensure!(
            count <= MAX_OBSERVATION_SAMPLES,
            "restored sound observation exceeds sample budget"
        );
        self.sample_count = count;
        self.load_segments.push(segment);
        Ok(())
    }

    pub(super) fn expected_clock(&self, step: u64, after_inputs: bool) -> (u64, u32) {
        self.load_segments
            .iter()
            .rev()
            .find(|segment| {
                segment.after_step < step || (after_inputs && segment.after_step == step)
            })
            .map_or((step, step as u32), |segment| {
                let delta = step - segment.after_step;
                (
                    segment.after.simulation_tick + delta,
                    segment.after.binary_frame.wrapping_add(delta as u32),
                )
            })
    }
    pub(super) fn observes_actor(
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

    pub(super) fn transcript(&self, profile: &MapCaptureProfile) -> Value {
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
                crate::ui::sidebar::SidebarFrame::native(profile.width, profile.height)
                    .tactical_size();
            observations["gesture_input"] = json!({
                "policy": profile.gesture_policy(),
                "equal_step_order": "commands_then_gestures",
                "tactical_extent": [width, height],
                "receipts": self.gestures,
            });
            if profile.observes_local_input() {
                observations["gesture_input"]["keyboard_bindings"] = json!(self.keyboard_bindings);
            }
        }
        if profile.observe_sidebar_steps.is_some() {
            observations["sidebar"] =
                json!({"policy": SIDEBAR_POLICY, "frames": self.sidebar_frames});
        }
        if profile.observe_electric_bolts == Some(true) {
            let policy = if profile.observe_electric_bolt_steps.is_some() {
                "electric-bolt-after-requested-tactical-composite-v2"
            } else {
                "electric-bolt-after-each-tactical-composite-v1"
            };
            observations["electric_bolt_draws"] =
                json!({"policy": policy, "frames": self.electric_bolt_draws});
        }
        if profile.observe_radiation_beams == Some(true) {
            observations["radiation_beam_draws"] = json!({
                "policy": "radiation-beam-after-each-tactical-composite-v1",
                "frames": self.radiation_beam_draws});
        }
        if profile.observe_audio.is_some() {
            observations["audio"] = json!(self.audio);
        }
        if profile.allow_load_segments == Some(true) {
            observations["load_segments"] =
                json!({"policy": LOAD_SEGMENT_POLICY, "transitions": self.load_segments});
        }
        observations
    }

    pub(super) fn pending_command<'a>(
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

    pub(super) fn pending_gesture<'a>(
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

    pub(super) fn observe_frame(
        &mut self,
        frame: MapFrameObservation,
        ids: BTreeSet<u64>,
    ) -> Result<()> {
        let (tick, binary_frame) = self.expected_clock(frame.completed_steps, false);
        ensure!(
            frame.completed_steps == self.frames.len() as u64
                && frame.simulation_tick == tick
                && frame.binary_frame == binary_frame,
            "actor observation skipped or repeated a committed frame"
        );
        let count = self
            .sample_count
            .checked_add(frame.actors.len())
            .and_then(|count| {
                frame.actors.iter().try_fold(count, |count, actor| {
                    count
                        .checked_add(
                            actor["building"]["animation_slots"]
                                .as_array()
                                .map_or(0, Vec::len),
                        )
                        .and_then(|count| count.checked_add(spawn_sample_count(&actor["spawns"])))
                        .and_then(|count| {
                            count.checked_add(temporal_sample_count(&actor["temporal"]))
                        })
                        .and_then(|count| {
                            count.checked_add(usize::from(actor["cloak"].get("runtime").is_some()))
                        })
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
                frame.terrain.iter().try_fold(count, |count, cell| {
                    count.checked_add(cell["terrain_instances"].as_array().map_or(0, Vec::len))
                })
            })
            .and_then(|count| {
                count.checked_add(
                    frame
                        .audio_state
                        .as_ref()
                        .map_or(0, sound_state_sample_count),
                )
            })
            .and_then(|count| {
                count.checked_add(
                    frame
                        .effects
                        .as_ref()
                        .map_or(0, MapEffectObservation::sample_count),
                )
            })
            .and_then(|count| {
                count.checked_add(
                    frame
                        .lasers
                        .as_ref()
                        .map_or(0, MapLaserObservation::sample_count),
                )
            })
            .and_then(|count| count.checked_add(frame.bombs.as_ref().map_or(0, bomb_sample_count)))
            .and_then(|count| count.checked_add(frame.crates.as_ref().map_or(0, Vec::len)))
            .and_then(|count| {
                count.checked_add(
                    frame
                        .input
                        .as_ref()
                        .map_or(0, MapInputObservation::sample_count),
                )
            })
            .and_then(|count| {
                count.checked_add(
                    frame
                        .electric_bolts
                        .as_ref()
                        .map_or(0, MapElectricBoltObservation::sample_count),
                )
            })
            .and_then(|count| {
                count.checked_add(
                    frame
                        .radiation_beams
                        .as_ref()
                        .map_or(0, MapRadiationBeamObservation::sample_count),
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

    pub(super) fn observe_draw(
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
        let clock_tick = self.expected_clock(expected_step, false).0;
        let expected_ms = clock_tick
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
            simulation_tick: self.allow_load_segments.then_some(clock_tick),
            tooltip: None,
        });
        Ok(())
    }
}
