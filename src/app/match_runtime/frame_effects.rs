//! Frame-scoped adaptation of existing client input and audio owners.
//! Native House4FC1EE calls the planner before the outcome sound boundary;
//! both use the same queue, and no client borrow survives advance_frame.

use super::sound_dispatch::dispatch_sim_sound_events;
use crate::app::input::messages::HudMessage;
use crate::app::input::planning_mode::PlanningInputState;
use crate::app::presentation::building_anim::SoundPlaybackInputs;
use crate::app::sidebar_projection::SidebarProjectionState;
use crate::audio::events::{GameSoundEvent, SoundEventQueue};
use crate::audio::sfx::SfxPlayer;
use crate::rules::ruleset::RuleSet;
use crate::sim::selection::SelectionState;
use crate::sim::world::{
    FrameClient, LocalPlanningBoundary, NetworkCommandBoundary, NetworkCommandError,
    OutcomeWaitBoundary, SimSoundEvent, Simulation, SoundBoundary, TacticalRadarBoundary,
};

#[cfg(all(test, tests_app_match))]
#[path = "planning_defeat_tests.rs"]
mod planning_defeat_tests;

#[cfg(all(test, tests_app_match))]
#[path = "outcome_wait_tests.rs"]
mod outcome_wait_tests;

/// A frame-local borrow of the existing app/audio owners. Native Gattling
/// Stop405D40 must finish before Report's Main65C780 draw; Update70E000 and
/// Limbo6F6C6B similarly release at their reached boundaries. No callback or
/// simulation reference survives this scope.
/// Source: `tools/input_oracle/gattling_loop.json` and its native harness.
type RadarAdmission<'a> =
    dyn FnMut(crate::sim::radar::RadarEventRequest, u64, &RuleSet) -> bool + 'a;
/// [`crate::app::presentation::building_anim::set_tactical_radar`] on the
/// lent presentation owners: the availability, the frame's SFX player and
/// the rules in, the cue to play out.
type TacticalRadarSetter<'a> =
    dyn FnMut(bool, Option<&mut SfxPlayer>, &RuleSet) -> Option<GameSoundEvent> + 'a;
/// MainTick55DBCD's campaign save, given the strip and the elapsed clock this
/// boundary also lends to Strip6A8F42's request clear and to scenario pauses.
type CampaignSave<'a> = dyn FnMut(&Simulation, &mut SidebarProjectionState, &mut super::frame_pacer::ScenarioElapsedClock)
    + 'a;

struct ScenarioPauseOwners<'a> {
    clock: &'a mut super::scenario_pause::ScenarioPauseClock,
    elapsed: &'a mut super::frame_pacer::ScenarioElapsedClock,
    mode: super::session::SessionMode,
    other_local_pause: bool,
}

/// One frame's lent client owners. The simulation reaches every role through
/// `&mut` ([`FrameClient`]), so each method borrows these owners only for its
/// own call, and a role that asks another (radar availability flushing the
/// sound prefix) does so through `self`.
pub(crate) struct FrameClientBoundary<'a> {
    delivered: usize,
    playback: Option<(&'a mut SfxPlayer, SoundPlaybackInputs<'a>)>,
    local_owner: Option<&'a str>,
    output: &'a mut SoundEventQueue,
    admit_radar: &'a mut RadarAdmission<'a>,
    tactical_radar: Option<&'a mut TacticalRadarSetter<'a>>,
    planning: Option<(&'a mut PlanningInputState, &'a mut SelectionState)>,
    /// [`super::session::MatchSession::latency_fudge`] as the frame began;
    /// only the frame's lifecycle outputs change it, after the frame.
    latency_fudge: i32,
    /// The host's process-lifetime checksum history (`SimHost::tick_parts`).
    frame_checksums: Option<&'a mut crate::sim::command_queue::FrameChecksumState>,
    live_network: Option<&'a mut crate::net::live_commands::LiveCommandSession>,
    outcome_wait: Option<&'a mut Option<super::scenario_exit::ScenarioOutcomeVoiceWait>>,
    /// Whether the simulation sees [`OutcomeWaitBoundary`] at all. Replay
    /// playback's recorded completion owns outcome timing, so a local audio
    /// wait cannot add a frame or change its hash.
    lends_outcome_wait: bool,
    scenario_pause: Option<ScenarioPauseOwners<'a>>,
    /// The local production strip, for Strip6A8F42's synchronous request
    /// clear and the campaign save's sidebar order.
    sidebar: Option<&'a mut SidebarProjectionState>,
    campaign_save: Option<&'a mut CampaignSave<'a>>,
    /// Whether the result's own EVA line is withheld for a commander clip
    /// (`Custom End Game Movie`,
    /// [`crate::app::commander_feedback::withholds_verdict_line`]).
    ///
    /// The clip's entrance clears the EVA queue
    /// (`RadarMovieCommand::Announce`), so a line admitted on the kill frame
    /// would be cut. The clip layer says it once no clip is in its way, or the
    /// exit router after the clip. Only sound delivery and the result's Vox
    /// wait read this; the simulation never sees it.
    defer_verdict_to_clip: bool,
    /// Accessibility rows from the frame's synchronously delivered sound
    /// prefix. Audio takes only the suffix, but HUD consumers inspect the
    /// complete facts, so this sink is filled in the same pass that plays the
    /// prefix and drained by the frame tail alongside the suffix rows.
    eva_text: Option<&'a mut Vec<HudMessage>>,
    /// Sounds of the player's own automatic orders this game skips
    /// ([`crate::app::match_audio::QuietSounds`]).
    quiet: Option<&'a mut crate::app::match_audio::QuietSounds>,
}

impl<'a> FrameClientBoundary<'a> {
    pub(crate) fn new(
        playback: Option<(&'a mut SfxPlayer, SoundPlaybackInputs<'a>)>,
        local_owner: Option<&'a str>,
        output: &'a mut SoundEventQueue,
        admit_radar: &'a mut RadarAdmission<'a>,
    ) -> Self {
        Self {
            delivered: 0,
            playback,
            local_owner,
            output,
            admit_radar,
            tactical_radar: None,
            planning: None,
            latency_fudge: 0,
            frame_checksums: None,
            live_network: None,
            outcome_wait: None,
            lends_outcome_wait: true,
            scenario_pause: None,
            sidebar: None,
            campaign_save: None,
            defer_verdict_to_clip: false,
            eva_text: None,
            quiet: None,
        }
    }

    /// Skip the sounds the player's own automatic orders cause. `None`, as
    /// in every headless and test frame, plays them all.
    pub(crate) fn with_quiet_sounds(
        mut self,
        quiet: &'a mut crate::app::match_audio::QuietSounds,
    ) -> Self {
        self.quiet = Some(quiet);
        self
    }

    /// Collect the accessibility rows from the synchronously delivered sound
    /// prefix. `None` keeps the boundary's native audio-only behaviour, which
    /// is what every headless and test frame uses.
    pub(crate) fn with_eva_text(mut self, sink: Option<&'a mut Vec<HudMessage>>) -> Self {
        self.eva_text = sink;
        self
    }

    /// Hold the result's EVA line for the losing commander's clip. See
    /// [`FrameClientBoundary::defer_verdict_to_clip`].
    pub(crate) fn with_deferred_verdict(mut self, deferred: bool) -> Self {
        self.defer_verdict_to_clip = deferred;
        self
    }

    pub(crate) fn with_tactical_radar(mut self, setter: &'a mut TacticalRadarSetter<'a>) -> Self {
        self.tactical_radar = Some(setter);
        self
    }

    fn consume_queued_audio(&mut self, sim: &Simulation, rules: &RuleSet) {
        let events = self.output.drain();
        if let Some((sfx, inputs)) = self.playback.as_mut() {
            inputs.consume(
                sfx,
                &events,
                Some(sim),
                Some(rules.sound_names()),
                self.local_owner,
            );
        }
    }

    pub(crate) fn with_planning(
        mut self,
        planning: &'a mut PlanningInputState,
        selection: &'a mut SelectionState,
    ) -> Self {
        self.planning = Some((planning, selection));
        self
    }

    pub(crate) fn with_latency_fudge(mut self, latency_fudge: i32) -> Self {
        self.latency_fudge = latency_fudge;
        self
    }

    pub(crate) fn with_frame_checksums(
        mut self,
        checksums: &'a mut crate::sim::command_queue::FrameChecksumState,
    ) -> Self {
        self.frame_checksums = Some(checksums);
        self
    }

    pub(crate) fn with_live_network(
        mut self,
        live: Option<&'a mut crate::net::live_commands::LiveCommandSession>,
    ) -> Self {
        self.live_network = live;
        self
    }

    pub(crate) fn with_outcome_wait(
        mut self,
        pending: &'a mut Option<super::scenario_exit::ScenarioOutcomeVoiceWait>,
    ) -> Self {
        self.outcome_wait = Some(pending);
        self
    }

    /// Keep [`OutcomeWaitBoundary`] from the simulation during replay
    /// playback. See [`FrameClientBoundary::lends_outcome_wait`].
    pub(crate) fn with_replay_playback(mut self, replaying: bool) -> Self {
        self.lends_outcome_wait = !replaying;
        self
    }

    pub(crate) fn with_scenario_pause(
        mut self,
        clock: &'a mut super::scenario_pause::ScenarioPauseClock,
        elapsed: &'a mut super::frame_pacer::ScenarioElapsedClock,
        mode: super::session::SessionMode,
        other_local_pause: bool,
    ) -> Self {
        self.scenario_pause = Some(ScenarioPauseOwners {
            clock,
            elapsed,
            mode,
            other_local_pause,
        });
        self
    }

    /// Lend the local production strip: Strip6A8F42 clears its requests, and
    /// `campaign_save`, when given, runs at MainTick55DBCD with it and the
    /// scenario pause's elapsed clock.
    pub(crate) fn with_sidebar(
        mut self,
        sidebar: &'a mut SidebarProjectionState,
        campaign_save: Option<&'a mut CampaignSave<'a>>,
    ) -> Self {
        self.sidebar = Some(sidebar);
        self.campaign_save = campaign_save;
        self
    }

    fn apply_prefix_effect_at(
        &mut self,
        effect: &crate::sim::trigger_runtime::TriggerEffect,
        wall_ms: u64,
    ) -> bool {
        use crate::sim::trigger_runtime::TriggerEffect;
        if !matches!(
            effect,
            TriggerEffect::ScenarioPause { .. } | TriggerEffect::ScenarioResume
        ) {
            return false;
        }
        let Some(ScenarioPauseOwners {
            clock,
            elapsed,
            mode,
            other_local_pause,
        }) = self.scenario_pause.as_mut()
        else {
            return false;
        };
        let sfx = self.playback.as_mut().map(|(sfx, _)| &mut **sfx);
        match *effect {
            TriggerEffect::ScenarioPause {
                duration,
                outermost,
            } => {
                super::scenario_pause::enter_with_owners(
                    clock, elapsed, *mode, sfx, duration, outermost, wall_ms,
                );
            }
            TriggerEffect::ScenarioResume => super::scenario_pause::resume_with_owners(
                elapsed,
                sfx,
                *other_local_pause,
                // The admitted frame completes before a relay pause takes
                // effect, so its native/audio pause reasons still coincide.
                *other_local_pause,
                wall_ms,
            ),
            _ => unreachable!(),
        }
        true
    }
}

impl FrameClient for FrameClientBoundary<'_> {
    fn sound(&mut self) -> Option<&mut dyn SoundBoundary> {
        Some(self)
    }

    fn tactical_radar(&mut self) -> Option<&mut dyn TacticalRadarBoundary> {
        Some(self)
    }

    fn outcome_wait(&mut self) -> Option<&mut dyn OutcomeWaitBoundary> {
        if self.lends_outcome_wait {
            Some(self)
        } else {
            None
        }
    }

    fn local_planning(&mut self) -> Option<&mut dyn LocalPlanningBoundary> {
        Some(self)
    }

    fn network_commands(&mut self) -> Option<&mut dyn NetworkCommandBoundary> {
        Some(self)
    }

    fn before_logic(&mut self, sim: &Simulation) {
        // The save reads the scenario pause's elapsed clock, so a boundary
        // lent a save without the pause would skip it without a sign.
        debug_assert!(
            self.campaign_save.is_none() || self.scenario_pause.is_some(),
            "a campaign save is lent together with the scenario pause"
        );
        if let (Some(save), Some(sidebar), Some(pause)) = (
            self.campaign_save.as_deref_mut(),
            self.sidebar.as_deref_mut(),
            self.scenario_pause.as_mut(),
        ) {
            save(sim, sidebar, pause.elapsed);
        }
    }

    fn production_request_cleared(&mut self, type_id: crate::sim::intern::InternedId) {
        if let Some(sidebar) = self.sidebar.as_deref_mut() {
            sidebar.clear_production_request(type_id);
        }
    }
}

impl OutcomeWaitBoundary for FrameClientBoundary<'_> {
    fn apply_prefix_effect(&mut self, effect: &crate::sim::trigger_runtime::TriggerEffect) -> bool {
        self.apply_prefix_effect_at(effect, super::frame_pacer::wall_clock_ms())
    }

    fn wait_complete(
        &mut self,
        _owner: crate::sim::intern::InternedId,
        kind: crate::sim::house_state::HouseOutcomeKind,
    ) -> bool {
        let Some(pending) = self.outcome_wait.as_deref_mut() else {
            return true;
        };
        // A line still withheld here waits for a clip that is playing, and
        // the clip's entrance suspends EVA, so this wait could only run into
        // its ceiling with the clip frozen. Ending it now changes wall time
        // only: the frame, its result and its hash are the same, as in replay
        // playback.
        if self.defer_verdict_to_clip {
            *pending = None;
            return true;
        }
        let wall_ms = super::frame_pacer::wall_clock_ms();
        // The outer event loop services the existing AudioServiceClock and
        // network owners between visits. This is only native7529E0's Vox poll.
        let voices_active = self.playback.as_mut().is_some_and(|(sfx, inputs)| {
            sfx.pump_and_check_voices(inputs.catalog.eva(), inputs.assets, inputs.catalog.index())
        });
        super::scenario_exit::ScenarioOutcomeVoiceWait::poll_pending(
            pending,
            kind,
            wall_ms,
            voices_active,
        )
    }
}

impl NetworkCommandBoundary for FrameClientBoundary<'_> {
    fn exchange_commands(
        &mut self,
        context: crate::sim::world::NetworkQueueContext,
        queues: &mut crate::sim::world::NetworkCommandQueues<'_>,
    ) -> Result<Option<crate::sim::world::NetworkQueuePass>, NetworkCommandError> {
        use crate::net::command_exchange::CommandExchangeError;
        use crate::net::event_packet::DecodeFailureKind;
        use crate::net::live_commands::LiveCommandError;
        use crate::sim::world::NetworkInterruption;
        let Some(live) = self.live_network.as_deref_mut() else {
            return Ok(None);
        };
        let max_ahead = queues.max_ahead_frames().ok_or_else(|| {
            NetworkCommandError::Unavailable("live network has no admitted MaxAhead".to_owned())
        })?;
        let now_ms = super::frame_pacer::wall_clock_ms();
        let now = super::frame_pacer::frame_bucket(now_ms);
        live.finish_processing(now_ms as u32);
        live.poll(
            queues,
            context,
            max_ahead,
            now,
            now_ms as u32,
            self.latency_fudge,
        )
        .map(Some)
        .map_err(|error| {
            log::error!("live network command exchange: {error}");
            match error {
                LiveCommandError::Sequenced(error) => error.interruption().into(),
                LiveCommandError::Startup(crate::net::live_commands::StartupError::Aborted) => {
                    NetworkInterruption::ConnectionFailed.into()
                }
                LiveCommandError::Startup(_)
                | LiveCommandError::Exchange(CommandExchangeError::InitialChecksum { .. }) => {
                    NetworkInterruption::ScenariosDiffer.into()
                }
                LiveCommandError::Exchange(CommandExchangeError::Decode(error))
                    if error.kind == DecodeFailureKind::AdmissionFull =>
                {
                    NetworkInterruption::QueueFull.into()
                }
                error => NetworkCommandError::Unavailable(error.to_string()),
            }
        })
    }

    fn record_frame_checksum(&mut self, frame: u32, checksum: u32) {
        if let Some(checksums) = self.frame_checksums.as_deref_mut() {
            checksums.record_frame_checksum(frame, checksum);
        }
    }

    fn compare_frame_info(
        &self,
        record: &crate::sim::command::CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        let checksums = self.frame_checksums.as_deref().ok_or_else(|| {
            NetworkCommandError::Unavailable(
                "FRAMEINFO requires the host's checksum owner".to_owned(),
            )
        })?;
        let info = crate::sim::command_queue::FrameInfo::decode(record).ok_or_else(|| {
            NetworkCommandError::Unavailable(
                "network comparison received a non-FRAMEINFO record".to_owned(),
            )
        })?;
        checksums
            .compare_frame_info(info, current_frame)
            .map_err(NetworkCommandError::Mismatch)
    }
}

impl LocalPlanningBoundary for FrameClientBoundary<'_> {
    fn leave_planning(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
    ) -> Option<crate::sim::command::CommandEnvelope> {
        let (planning, selection) = self.planning.as_mut()?;
        planning.set_active(false, Some(sim), Some(rules), selection, self.output)
    }

    fn planning_active(&self) -> bool {
        self.planning
            .as_ref()
            .is_some_and(|(planning, _)| planning.active())
    }
}

impl SoundBoundary for FrameClientBoundary<'_> {
    fn consume(&mut self, sim: &Simulation, rules: &RuleSet, events: &[SimSoundEvent]) {
        let delivered = self.delivered;
        let pending = events
            .get(delivered..)
            .expect("frame sound prefix shrank after synchronous delivery");
        if pending.is_empty() {
            return;
        }
        let frame = sim.session().tick();
        let defer_verdict_to_clip = self.defer_verdict_to_clip;
        let admit_radar = &mut *self.admit_radar;
        let mut quiet = self.quiet.as_deref_mut();
        dispatch_sim_sound_events(
            pending.iter().cloned().filter(|event| {
                // The router says this line after the clip: see
                // [`FrameClientBoundary::defer_verdict_to_clip`].
                !(defer_verdict_to_clip && matches!(event, SimSoundEvent::MatchOutcome { .. }))
                    && quiet.as_mut().is_none_or(|quiet| quiet.admit(event, frame))
            }),
            sim,
            rules,
            self.local_owner,
            &mut |request| admit_radar(request, sim.session().tick(), rules),
            self.output,
            self.eva_text.as_deref_mut(),
        );
        self.consume_queued_audio(sim, rules);
        self.delivered = delivered + pending.len();
    }

    fn delivered(&self) -> usize {
        self.delivered
    }

    fn update_looping_sound(
        &mut self,
        sim: &Simulation,
        owner: u64,
        world: crate::sim::anim_class::AnimWorldCoord,
    ) {
        if let Some((sfx, inputs)) = self.playback.as_mut() {
            inputs.update_looping_sound(sfx, sim, self.local_owner, owner, world);
        }
    }
}

impl TacticalRadarBoundary for FrameClientBoundary<'_> {
    fn set_available(&mut self, sim: &Simulation, rules: &RuleSet, available: bool) {
        // House508F4F/4FC21B -> Radar656DF0 ->656BE0 is synchronous.
        // Earlier death sounds and637A10's queued EndPlanningModeSound
        // must reach the existing SFX owner before the availability cue.
        self.consume(sim, rules, sim.sound_events());
        self.consume_queued_audio(sim, rules);
        let Some(setter) = self.tactical_radar.as_deref_mut() else {
            return;
        };
        let sfx = self.playback.as_mut().map(|(sfx, _)| &mut **sfx);
        let Some(sound) = setter(available, sfx, rules) else {
            return;
        };
        if let Some((sfx, inputs)) = self.playback.as_mut() {
            inputs.consume(
                sfx,
                &[sound],
                Some(sim),
                Some(rules.sound_names()),
                self.local_owner,
            );
        }
    }
}

#[cfg(all(test, tests_app_match))]
mod tactical_radar_tests {
    use super::*;
    use crate::sim::world::FrameEffects;
    use std::cell::RefCell;

    #[test]
    fn tactical_radar_delivers_sound_prefix_and_queued_planning_exit() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let rules = retail.rules;
        let mut sim = Simulation::new();
        let map = crate::map::map_file::MapFile::from_bytes(
            b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
              [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
              [Basic]\nNewINIFormat=4\n[Waypoints]\n0=1001\n\
              [Triggers]\nT=Americans,<none>,Radar,0,1,1,1,0\n\
              [Events]\nT=1,13,0,0\n\
              [Actions]\nT=1,55,0,1,0,0,0,0,A\n\
              [Tags]\nTAG=0,Radar,T\n",
        )
        .unwrap();
        sim.install_waypoints(&map.waypoints);
        sim.initialize_trigger_definitions(&map, &Default::default(), None, None, None);
        sim.classify_map_triggers(&rules);
        sim.advance_triggers(&rules, None, &mut FrameEffects::empty());
        assert!(matches!(
            sim.sound_events(),
            [SimSoundEvent::RadarEvent { .. }]
        ));
        let observed = RefCell::new(Vec::new());
        let mut admit = |_, _, _: &RuleSet| {
            observed.borrow_mut().push("prefix");
            true
        };
        let mut setter = |_: bool, _: Option<&mut SfxPlayer>, _: &RuleSet| {
            observed.borrow_mut().push("setter");
            None
        };
        let mut planning = PlanningInputState::default();
        let mut selection = SelectionState::new();
        let mut sounds = SoundEventQueue::new();
        planning.set_active(true, Some(&sim), Some(&rules), &mut selection, &mut sounds);
        let _ = sounds.drain();
        let mut boundary = FrameClientBoundary::new(None, None, &mut sounds, &mut admit)
            .with_tactical_radar(&mut setter)
            .with_planning(&mut planning, &mut selection);
        boundary.set_available(&sim, &rules, false);
        assert_eq!(*observed.borrow(), ["prefix", "setter"]);
        assert_eq!(boundary.delivered(), 1);

        //637A10 queues UI audio without a new SimSound fact. A second
        //consume of the same prefix alone would return without draining it.
        boundary.leave_planning(&sim, &rules);
        let expected = rules.general.end_planning_mode_sound.as_ref().unwrap();
        assert!(matches!(
            boundary.output.iter().collect::<Vec<_>>().as_slice(),
            [crate::audio::events::GameSoundEvent::UiSound { sound_id }] if sound_id.name_in(&rules) == expected
        ));
        boundary.set_available(&sim, &rules, true);
        assert!(boundary.output.is_empty());
        assert_eq!(boundary.delivered(), 1);
        assert_eq!(*observed.borrow(), ["prefix", "setter", "setter"]);
        // Real dispatch/queue consumption, without SFX/device playback.
    }
}

#[cfg(all(test, tests_app_match))]
mod radar_persistence_tests {
    use super::*;
    use crate::render::radar::{ClientRadarEvents, saved_radar_fixture};
    use crate::sim::radar::{RadarEventRequest, RadarEventType};

    #[test]
    fn same_frame_admission_does_not_tick_restored_alerts_twice() {
        let saved = saved_radar_fixture();
        let retained_count = saved.events.len();
        let mut restored = ClientRadarEvents::from_saved(saved.clone()).unwrap();
        let mut uninterrupted = ClientRadarEvents::from_saved(saved).unwrap();
        let mut sim = Simulation::with_seed(73);
        sim.session_mut_for_test().set_tick_for_test(100);
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let mut sounds = SoundEventQueue::new();
        {
            let mut admit = |request, frame, rules: &RuleSet| {
                restored.admit(
                    request,
                    frame,
                    Some(((50, 50), (140, 108))),
                    &rules.radar_event_config,
                )
            };
            let mut boundary = FrameClientBoundary::new(None, None, &mut sounds, &mut admit);
            // Action55 flushes this boundary before complete_tick increments
            // the simulation frame, which may already have been presented.
            boundary.consume(
                &sim,
                &rules,
                &[SimSoundEvent::RadarEvent {
                    radar: RadarEventRequest::new(RadarEventType::Noncombat, 147, 99),
                }],
            );
            assert_eq!(boundary.delivered(), 1);
        }
        restored.advance_to_frame(101, &rules.radar_event_config);
        uninterrupted.advance_to_frame(101, &rules.radar_event_config);
        let after = restored.saved();
        assert_eq!(after.events.len(), retained_count + 1);
        assert_eq!(
            after.events[..retained_count],
            uninterrupted.saved().events,
            "a later alert must not replay a retained alert's previous animation frame"
        );
        assert_eq!(after.last_advanced_frame, Some(101));
    }
}

#[cfg(all(test, tests_app_match))]
pub(super) mod network_command_tests {
    use super::*;
    use crate::sim::house_state::HouseState;
    use crate::sim::runtime::SimRuntime;
    use crate::sim::world::{FrameEffects, TickLane};

    /// A WOL world at frame 256, the first frame its House gate admits.
    pub(in crate::app::match_runtime) fn frame_256_runtime() -> SimRuntime {
        let mut sim = Simulation::with_seed(73);
        let owner = sim.intern("Human");
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Wol);
        // WOL admission includes MaxAhead before the frame-256 House gate.
        sim.install_network_timing(10);
        sim.session_mut_for_test().set_tick_for_test(256);
        sim.session_mut_for_test().set_binary_frame_for_test(256);
        SimRuntime::from_simulation(sim)
    }

    #[test]
    fn production_boundary_publishes_current_checksum_before_comparison_and_resumes_once() {
        let control = frame_256_runtime()
            .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
            .unwrap();
        let expected = control
            .multiplayer_checksum
            .unwrap()
            .checksum
            .unwrap()
            .value;
        let mut runtime = frame_256_runtime();
        let mut checksums = crate::sim::command_queue::FrameChecksumState::default();
        checksums.record_frame_checksum(0, 0);
        checksums.record_frame_checksum(255, expected ^ 2);
        let info = crate::sim::command_queue::FrameInfo {
            house_id: 0,
            event_frame: 256,
            checksum: expected ^ 1,
            timing_word: 0,
            delay: 0,
        };
        runtime
            .simulation
            .admit_native_command_record(info.encode());
        let mut sounds = SoundEventQueue::new();
        let mut radar = |_, _, _: &RuleSet| false;
        let prefix = {
            let mut boundary =
                FrameClientBoundary::new(None, Some("Human"), &mut sounds, &mut radar)
                    .with_frame_checksums(&mut checksums);
            runtime
                .advance_frame(
                    &[],
                    33,
                    TickLane::Ordinary,
                    &mut FrameEffects::for_client(&mut boundary),
                )
                .unwrap()
        };
        let failure = prefix
            .network_failure
            .expect("real session comparator rejects the peer CRC");
        assert_eq!(failure.local_checksum, expected);
        assert_eq!(failure.remote_checksum, expected ^ 1);
        assert_eq!(checksums.checksum_history().get(256), expected);
        assert_eq!(runtime.simulation.session().binary_frame(), 256);
        assert!(!prefix.tick.frame_committed);
        let suffix = runtime
            .resume_after_network_failure(&mut FrameEffects::empty())
            .unwrap();
        assert!(suffix.tick.frame_committed);
        assert_eq!(runtime.simulation.session().binary_frame(), 257);
        assert!(
            !suffix
                .lifecycle_outputs
                .iter()
                .any(|event| matches!(event, crate::sim::world::LifecycleOutput::LogicVisit))
        );
        assert!(
            runtime
                .resume_after_network_failure(&mut FrameEffects::empty())
                .is_err()
        );
    }
}
