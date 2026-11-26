//! The admitted playing sources release each authored presentation boundary.
//! Media readiness is local. Simulation receives only ordered scenario
//! services at the completed tick, so playback needs neither peers nor media.

use std::collections::{BTreeMap, VecDeque};

use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::fullscreen_movie::{FullscreenMovie, MovieReturn};
use crate::app::frontend::state::FrontendState;
use crate::app::lan_service::CampaignControlSend;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::{App, AppState, GameScreen, PlatformState};
use crate::net::global::GlobalBatch;
use crate::net::lan_messages::extended_setup::campaign::Signal;
use crate::net::live_commands::NarrativeHold;
use crate::sim::replay::ScenarioPauseInput;

use super::network_match;
use super::scenario_pause;

/// A disconnected or stuck clip cannot keep a match in this service forever.
const HOLD_TIMEOUT_MS: u64 = 180_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Point {
    boundary: u64,
    tick: u64,
}

struct Barrier {
    point: Point,
    playing_sources: Vec<usize>,
    deadline_ms: u64,
    movie_pause: bool,
    timed: bool,
    sent_ready: bool,
    release_batch: Option<GlobalBatch>,
}

enum HostRelease {
    Wait,
    Send,
    Complete,
}

#[derive(Clone, Copy)]
pub(super) enum Clip {
    Fullscreen(i32),
    Radar(i32),
}

#[derive(Default)]
pub(crate) struct NarrativeSync {
    clips: VecDeque<Clip>,
    radar_playing: bool,
    barrier: Option<Barrier>,
    completed: u64,
    ready: BTreeMap<usize, Point>,
    release: Option<Point>,
    aborted: Option<Point>,
    last_chrono_step: Option<u32>,
    opening_queued: bool,
}

impl NarrativeSync {
    fn owns_fullscreen(&self, movie: Option<&MovieReturn>) -> bool {
        self.barrier.is_some() && matches!(movie, Some(MovieReturn::RunningScenario))
    }

    fn record_ready(&mut self, source: usize, point: Point, playing_sources: &[usize]) {
        if point.boundary == self.completed.saturating_add(1) && playing_sources.contains(&source) {
            self.ready.insert(source, point);
        }
    }

    fn all_ready(&self, barrier: &Barrier) -> bool {
        barrier
            .playing_sources
            .iter()
            .all(|source| self.ready.get(source) == Some(&barrier.point))
    }

    fn host_release(&self, delivered: bool) -> HostRelease {
        let Some(barrier) = self.barrier.as_ref() else {
            return HostRelease::Wait;
        };
        if barrier.release_batch.is_some() {
            if delivered {
                HostRelease::Complete
            } else {
                HostRelease::Wait
            }
        } else if self.all_ready(barrier) {
            HostRelease::Send
        } else {
            HostRelease::Wait
        }
    }

    pub(super) fn pending(&self) -> bool {
        self.barrier.is_some() || !self.clips.is_empty()
    }
}

pub(crate) fn owns_fullscreen(
    frontend: &crate::app::frontend::state::FrontendState,
    presentation: &crate::app::presentation::state::MatchPresentationState,
) -> bool {
    presentation.narrative_sync.owns_fullscreen(
        frontend
            .fullscreen_movie
            .as_ref()
            .map(FullscreenMovie::return_to),
    )
}

fn enabled(match_state: &crate::app::match_runtime::state::MatchState) -> bool {
    match_state.session.network_authored_start().is_some()
        && match_state.live_network.is_some()
        && !match_state.match_diagnostics.is_replay_playback()
}

pub(super) fn queue(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    clip: Clip,
) -> bool {
    if !enabled(match_state) {
        return false;
    }
    match_state
        .match_presentation
        .narrative_sync
        .clips
        .push_back(clip);
    true
}

pub(crate) fn receive(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    source: usize,
    signal: Signal,
) {
    if !enabled(match_state) {
        return;
    }
    let from_host = crate::app::lan_service::campaign_host_source(match_state) == Some(source);
    let session = &match_state.session;
    let playing = session.network_connected_source_indices();
    let sync = &mut match_state.match_presentation.narrative_sync;
    match signal {
        Signal::NarrativeReady { boundary, tick } if session.host() => {
            sync.record_ready(source, Point { boundary, tick }, playing);
        }
        Signal::NarrativeRelease { boundary, tick }
            if !session.host() && from_host && boundary == sync.completed.saturating_add(1) =>
        {
            sync.release = Some(Point { boundary, tick });
        }
        Signal::NarrativeAbort { boundary, tick }
            if !session.host() && from_host && boundary == sync.completed.saturating_add(1) =>
        {
            sync.aborted = Some(Point { boundary, tick });
        }
        _ => {}
    }
}

fn queue_opening_clips(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &crate::app::process_assets::ProcessAssets,
) {
    let sync = &mut match_state.match_presentation.narrative_sync;
    if std::mem::replace(&mut sync.opening_queued, true) {
        return;
    }
    // StartScenario683AB0 falls back to Brief if Intro is unresolved.
    // Basic.Action follows the scenario read (683D9C).
    let basic = &match_state.map_basic;
    if let Some(rules) = process_assets.native_rules() {
        let index = |name: Option<&str>| name.map_or(-1, |name| rules.movies().find_index(name));
        let intro = index(basic.intro.as_deref());
        let first = if intro < 0 {
            index(basic.briefing.as_deref())
        } else {
            intro
        };
        for index in [first, index(basic.action.as_deref())] {
            if index >= 0 {
                sync.clips.push_back(Clip::Fullscreen(index));
            }
        }
    }
}

/// Run while the game, a fullscreen clip, or a local modal owns the window.
/// No world frame is admitted between acquiring and releasing this hold.
pub(crate) fn service(state: &mut AppState, wall_ms: u64) {
    if !enabled(&state.match_state)
        || state.match_state.scenario_exit.is_some()
        || (state.frontend.screen != GameScreen::InGame
            && !owns_fullscreen(&state.frontend, &state.match_state.match_presentation))
    {
        return;
    }
    let Some(runtime) = state.match_state.sim_runtime.runtime() else {
        return;
    };
    let sim = &runtime.simulation;
    if sim.pending_network_wait() || sim.pending_outcome_wait().is_some() {
        return;
    }
    let tick = sim.session().tick();
    let timed = sim.session().scenario_pause_timed();
    let chrono = sim.session().scenario_paused() && sim.session().chrono_screen_active();
    if state.frontend.screen == GameScreen::InGame {
        queue_opening_clips(&mut state.match_state, &state.process_assets);
    }
    let sync = &mut state.match_state.match_presentation.narrative_sync;
    if sync.barrier.is_none() && (timed || chrono || !sync.clips.is_empty()) {
        let movie_pause = !sync.clips.is_empty();
        let session = &state.match_state.session;
        sync.barrier = Some(Barrier {
            point: Point {
                boundary: sync.completed.saturating_add(1),
                tick,
            },
            playing_sources: session.network_connected_source_indices().to_vec(),
            deadline_ms: wall_ms.saturating_add(HOLD_TIMEOUT_MS),
            movie_pause,
            timed,
            sent_ready: false,
            release_batch: None,
        });
        if !session.local_connection_watches()
            && let Some(live) = state.match_state.live_network.as_mut()
        {
            live.set_narrative_hold(Some(if session.host() {
                NarrativeHold::Host
            } else {
                NarrativeHold::Player
            }));
        }
        state.match_state.scenario_elapsed_clock.pause(wall_ms);
        super::sim_tick::prepare_replay_recording(&mut state.match_state, &state.process_assets);
        if movie_pause {
            scenario_pause::apply_pause_service(
                &state.platform,
                &state.renderer,
                &state.diag,
                &state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &state.persistence,
                ScenarioPauseInput::Pause,
            );
        }
    }
    if !state
        .match_state
        .match_presentation
        .narrative_sync
        .pending()
    {
        return;
    }
    if chrono {
        let bucket = super::frame_pacer::frame_bucket(wall_ms);
        let sync = &mut state.match_state.match_presentation.narrative_sync;
        if sync.last_chrono_step.replace(bucket) != Some(bucket) {
            scenario_pause::apply_chrono_screen_step(state);
        }
    }
    if state.match_state.session.local_connection_watches() {
        // A viewer projects the same services at this boundary, while the
        // ordered stream supplies the live relay hold and resume markers.
        if !chrono {
            release(
                &mut state.platform,
                &state.renderer,
                &state.diag,
                &state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &state.persistence,
                wall_ms,
            );
        }
        return;
    }
    play_next_clip(state);
    finish_or_wait(state, chrono, wall_ms);
}

fn play_next_clip(state: &mut AppState) {
    if state.frontend.fullscreen_movie.is_some() {
        return;
    }
    let radar_pending = crate::app::radar_movie::has_pending(&state.match_state.match_presentation);
    let sync = &mut state.match_state.match_presentation.narrative_sync;
    if sync.radar_playing && radar_pending {
        return;
    }
    sync.radar_playing = false;
    match sync.clips.pop_front() {
        Some(Clip::Fullscreen(index)) => {
            let name = state
                .process_assets
                .native_rules()
                .and_then(|rules| rules.movies().name(index))
                .map(str::to_owned);
            if let Some(name) = name {
                App::start_fullscreen_movie(
                    state,
                    &name,
                    crate::app::frontend::fullscreen_movie::MovieReturn::RunningScenario,
                );
            }
        }
        Some(Clip::Radar(index)) => {
            // The FIFO presents the clip; its usual local pause credit is
            // supplied by the synchronized scenario services instead.
            crate::app::radar_movie::enqueue(
                &mut state.match_state,
                &mut state.persistence.options_profile,
                &state.process_assets,
                &state.renderer,
                index,
                false,
            );
            state
                .match_state
                .match_presentation
                .narrative_sync
                .radar_playing =
                crate::app::radar_movie::has_pending(&state.match_state.match_presentation);
        }
        None => {}
    }
}

fn finish_or_wait(state: &mut AppState, chrono: bool, wall_ms: u64) {
    let host_source = crate::app::lan_service::campaign_host_source(&state.match_state);
    let session = &state.match_state.session;
    let Some(local) = network_match::local_source(session) else {
        return;
    };
    let playing = session.network_connected_source_indices();
    let presentation = &mut state.match_state.match_presentation;
    let sync = &mut presentation.narrative_sync;
    let Some(barrier) = sync.barrier.as_ref() else {
        return;
    };
    let point = barrier.point;
    let host = session.host();
    let peers: Vec<_> = playing
        .iter()
        .copied()
        .filter(|&source| source != local)
        .collect();
    if sync.aborted == Some(point)
        || wall_ms >= barrier.deadline_ms
        || playing != barrier.playing_sources
    {
        if host {
            let _ = crate::app::lan_service::send_campaign_control(
                &mut state.match_state,
                &peers,
                Signal::NarrativeAbort {
                    boundary: point.boundary,
                    tick: point.tick,
                },
            );
        }
        let step =
            App::finish_fullscreen_movie(&mut state.frontend.fullscreen_movie, &mut state.audio);
        if let Some(step) = step {
            App::apply_step(state, step);
        }
        App::return_disconnected_to_main_menu(state);
        App::open_shell_notice(
            &state.platform.window,
            &state.renderer,
            &mut state.frontend,
            &state.process_assets,
            &state.persistence.options_profile,
            crate::i18n::tr("VERA:CampaignNarrativeTimeout"),
        );
        return;
    }
    let ready = state.frontend.fullscreen_movie.is_none()
        && sync.clips.is_empty()
        && !sync.radar_playing
        && !chrono
        && (!barrier.timed || presentation.scenario_pause.elapsed(wall_ms));
    if ready && !barrier.sent_ready {
        let sent = if host {
            sync.record_ready(local, point, playing);
            true
        } else if let Some(host_source) = host_source {
            crate::app::lan_service::send_campaign_control(
                &mut state.match_state,
                &[host_source],
                Signal::NarrativeReady {
                    boundary: point.boundary,
                    tick: point.tick,
                },
            )
            .is_some()
        } else {
            false
        };
        if let Some(barrier) = state
            .match_state
            .match_presentation
            .narrative_sync
            .barrier
            .as_mut()
        {
            barrier.sent_ready = sent;
        }
    }
    let sync = &state.match_state.match_presentation.narrative_sync;
    if host {
        let live = state.match_state.live_network.as_ref();
        let ordered = live.is_some_and(|live| live.uses_sequenced_lockstep());
        let delivered = sync
            .barrier
            .as_ref()
            .and_then(|barrier| barrier.release_batch)
            .is_some_and(|batch| live.is_some_and(|live| live.global().batch_delivered(batch)));
        match sync.host_release(delivered) {
            HostRelease::Wait => {}
            HostRelease::Complete => release(
                &mut state.platform,
                &state.renderer,
                &state.diag,
                &state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &state.persistence,
                wall_ms,
            ),
            HostRelease::Send => {
                if let Some(CampaignControlSend::Queued(batch)) =
                    crate::app::lan_service::send_campaign_control(
                        &mut state.match_state,
                        &peers,
                        Signal::NarrativeRelease {
                            boundary: point.boundary,
                            tick: point.tick,
                        },
                    )
                {
                    // The reliable Release must reach every playing peer
                    // before the ordered relay starts sealing again.
                    if ordered && !peers.is_empty() {
                        if let Some(barrier) = state
                            .match_state
                            .match_presentation
                            .narrative_sync
                            .barrier
                            .as_mut()
                        {
                            barrier.release_batch = Some(batch);
                        }
                    } else {
                        release(
                            &mut state.platform,
                            &state.renderer,
                            &state.diag,
                            &state.frontend,
                            &mut state.match_state,
                            &state.process_assets,
                            &mut state.audio,
                            &state.persistence,
                            wall_ms,
                        );
                    }
                }
            }
        }
    } else if ready && sync.release == Some(point) {
        release(
            &mut state.platform,
            &state.renderer,
            &state.diag,
            &state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio,
            &state.persistence,
            wall_ms,
        );
    }
}

fn release(
    platform: &mut PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    wall_ms: u64,
) {
    let sync = &mut match_state.match_presentation.narrative_sync;
    let Some(barrier) = sync.barrier.take() else {
        return;
    };
    sync.completed = barrier.point.boundary;
    sync.ready.clear();
    sync.release = None;
    sync.aborted = None;
    sync.clips.clear();
    sync.last_chrono_step = None;
    if barrier.movie_pause {
        scenario_pause::apply_pause_service(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            ScenarioPauseInput::Resume,
        );
    }
    if barrier.timed {
        scenario_pause::apply_pause_service(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            ScenarioPauseInput::ExpireTimedPause,
        );
    }
    if let Some(live) = match_state.live_network.as_mut() {
        live.set_narrative_hold(None);
    }
    scenario_pause::resume(&frontend.screen, match_state, audio, wall_ms);
    platform.frame_pacer.reanchor(wall_ms);
    platform.window.request_redraw();
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;

    #[test]
    fn a_running_scenario_movie_preserves_only_its_active_narrative_service() {
        let mut sync = NarrativeSync::default();
        sync.clips.push_back(Clip::Fullscreen(0));
        assert!(
            sync.pending(),
            "routing the stage result must wait for a queued clip"
        );
        assert!(!sync.owns_fullscreen(Some(&MovieReturn::RunningScenario)));
        sync.barrier = Some(Barrier {
            point: Point {
                boundary: 1,
                tick: 2,
            },
            playing_sources: vec![0],
            deadline_ms: HOLD_TIMEOUT_MS,
            movie_pause: true,
            timed: false,
            sent_ready: false,
            release_batch: None,
        });
        assert!(sync.owns_fullscreen(Some(&MovieReturn::RunningScenario)));
        assert!(!sync.owns_fullscreen(Some(&MovieReturn::StartupIntro)));
    }

    #[test]
    fn a_watcher_and_a_receipt_from_the_wrong_tick_cannot_release_the_players() {
        let point = Point {
            boundary: 1,
            tick: 31,
        };
        let barrier = Barrier {
            point,
            playing_sources: vec![0, 1],
            deadline_ms: HOLD_TIMEOUT_MS,
            movie_pause: true,
            timed: false,
            sent_ready: false,
            release_batch: None,
        };
        let mut sync = NarrativeSync::default();
        sync.record_ready(0, point, &barrier.playing_sources);
        sync.record_ready(1, Point { tick: 32, ..point }, &barrier.playing_sources);
        sync.record_ready(2, point, &barrier.playing_sources);
        assert!(!sync.ready.contains_key(&2));
        assert!(!sync.all_ready(&barrier));
        sync.record_ready(1, point, &barrier.playing_sources);
        assert!(sync.all_ready(&barrier));
        sync.barrier = Some(barrier);
        assert!(matches!(sync.host_release(true), HostRelease::Send));
        let release = crate::net::global::GlobalChannel::new(
            Default::default(),
            crate::net::reliable::RetryTiming::LAN_GUEST_COMPLETED_GLOBAL,
        )
        .send_reliable_batch(b"", &[])
        .unwrap();
        sync.barrier.as_mut().unwrap().release_batch = Some(release);
        assert!(matches!(sync.host_release(false), HostRelease::Wait));
        assert!(matches!(sync.host_release(true), HostRelease::Complete));
    }
}
