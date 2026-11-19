//! Blocking authored presentation reaches this gate after the existing frame
//! transaction. MatchSession owns the connected roster and ScenarioSession
//! owns its timed pause and input lock. Only media readiness is local here.

use std::collections::{BTreeMap, VecDeque};

use crate::app::frontend::fullscreen_movie::MovieReturn;
use crate::app::{App, AppState, GameScreen};
use crate::net::global::{GlobalDestination, GlobalMessage};
use crate::net::lan_messages::SessionScope;
use crate::sim::replay::ScenarioServiceInput;

use super::frame_pacer::frame_bucket;
use super::session::SessionMode;

const MAGIC: &[u8; 4] = b"VMP1";
const PROBE_BUCKETS: u32 = 30;

pub(super) enum Clip {
    Fullscreen(i32),
    Radar { index: i32, blocking: bool },
}

impl Clip {
    fn blocking(&self) -> bool {
        matches!(
            self,
            Self::Fullscreen(_) | Self::Radar { blocking: true, .. }
        )
    }
}

#[derive(Clone, Copy)]
enum Playing {
    Fullscreen,
    Radar,
}

#[derive(Default)]
pub(crate) struct AuthoredNarrative {
    queue: VecDeque<Clip>,
    playing: Option<Playing>,
    round: Option<Round>,
    local_ready: u64,
    last_probe: Option<u32>,
    last_chrono_step: Option<u32>,
    peers: BTreeMap<usize, Receipt>,
}

#[derive(Clone, Copy)]
struct Round {
    id: u64,
    opened: u32,
    timed: bool,
}

#[derive(Default)]
struct Receipt {
    ready: u64,
    heard_at: Option<u32>,
}

impl AuthoredNarrative {
    pub(crate) const fn pending(&self) -> bool {
        self.round.is_some()
    }

    fn observe(&mut self, source: usize, ready: u64, now: u32) {
        let reached = self.round.map_or(self.local_ready, |round| round.id);
        let receipt = self.peers.entry(source).or_default();
        receipt.heard_at = Some(now);
        // A peer can finish the next round before this application visits the
        // committed frame that opens it. Older/reordered probes cannot undo it.
        if ready <= reached.saturating_add(1) {
            receipt.ready = receipt.ready.max(ready);
        }
    }

    fn all_ready(&self, round: Round, local: usize, connected: &[usize]) -> bool {
        self.local_ready >= round.id
            && connected.iter().all(|source| {
                *source == local
                    || self
                        .peers
                        .get(source)
                        .is_some_and(|peer| peer.ready >= round.id)
            })
    }

    fn expired(&self, round: Round, source: usize, now: u32) -> bool {
        let timeout = crate::net::live_commands::LiveCommandMode::Lan
            .wait_timing()
            .timeout as u32;
        bucket_delta(now, round.opened) >= timeout
            && self
                .peers
                .get(&source)
                .and_then(|peer| peer.heard_at)
                .is_none_or(|heard| bucket_delta(now, heard) >= timeout)
    }
}

fn bucket_delta(now: u32, earlier: u32) -> u32 {
    now.wrapping_sub(earlier) & frame_bucket(u64::from(u32::MAX))
}

pub(crate) fn enabled(state: &AppState) -> bool {
    state.match_state.session_mode() == SessionMode::Lan
        && state.match_state.live_network.is_some()
        && state
            .match_state
            .lan_service
            .as_ref()
            .is_some_and(|lan| lan.setup_scope().is_some())
        && state
            .match_state
            .sim_runtime
            .runtime()
            .is_some_and(|runtime| runtime.simulation.session().is_authored_campaign())
        && !state.match_state.match_diagnostics.is_replay_playback()
}

pub(crate) fn pending(state: &AppState) -> bool {
    state.match_state.scenario_exit.is_none()
        && state
            .match_state
            .match_presentation
            .authored_narrative
            .pending()
}

pub(super) fn queue(state: &mut AppState, clip: Clip) {
    state
        .match_state
        .match_presentation
        .authored_narrative
        .queue
        .push_back(clip);
}

pub(crate) fn reset(state: &mut AppState) {
    state.match_state.match_presentation.authored_narrative = AuthoredNarrative::default();
}

pub(crate) fn queue_opening(state: &mut AppState, wall_ms: u64) {
    if !enabled(state) {
        return;
    }
    let basic = &state.match_state.map_basic;
    if [&basic.intro, &basic.briefing, &basic.action]
        .into_iter()
        .all(|name| name.as_ref().is_none_or(|name| name.trim().is_empty()))
    {
        return;
    }
    // StartScenario683AB0 uses Brief only for an unresolved Intro. Basic.Action
    // follows the read (683D9C). Both clips precede the first authored tick.
    let indexes = state.process_assets.native_rules().map(|rules| {
        let movies = rules.movies();
        let index = |name: Option<&str>| name.map_or(-1, |name| movies.find_index(name));
        let intro = index(basic.intro.as_deref());
        let intro_or_brief = if intro == -1 {
            index(basic.briefing.as_deref())
        } else {
            intro
        };
        [intro_or_brief, index(basic.action.as_deref())]
    });
    for index in indexes.unwrap_or([-1, -1]) {
        // Unavailable media still enters the same scoped readiness handoff.
        queue(state, Clip::Fullscreen(index));
    }
    arm_after_frame(state, wall_ms);
}

pub(super) fn arm_after_frame(state: &mut AppState, wall_ms: u64) {
    if !enabled(state) {
        return;
    }
    let Some(runtime) = state.match_state.sim_runtime.runtime() else {
        return;
    };
    let scenario = runtime.simulation.session();
    let narrative = &mut state.match_state.match_presentation.authored_narrative;
    if narrative.round.is_none()
        && (scenario.scenario_paused() || narrative.queue.iter().any(Clip::blocking))
    {
        narrative.round = Some(Round {
            id: narrative.local_ready + 1,
            opened: frame_bucket(wall_ms),
            timed: scenario.scenario_pause_timed(),
        });
        narrative.last_probe = None;
        narrative.last_chrono_step = None;
        state.match_state.scenario_elapsed_clock.pause(wall_ms);
    }
}

pub(crate) fn receive(state: &mut AppState, message: &GlobalMessage, now: u32) -> Option<()> {
    let remaining = message.payload.strip_prefix(MAGIC)?;
    if !enabled(state) {
        return Some(());
    }
    if let Some((scope, ready)) = remaining.split_at_checked(SessionScope::LEN)
        && SessionScope::decode(scope).is_some_and(|scope| {
            state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.setup_scope())
                == Some(scope)
        })
        && let Ok(ready) = <[u8; 8]>::try_from(ready)
        && let Some(source) = state
            .match_state
            .session
            .network_endpoint_source(message.source)
        && state
            .match_state
            .session
            .network_connected_source_indices()
            .contains(&source)
    {
        state
            .match_state
            .match_presentation
            .authored_narrative
            .observe(source, u64::from_le_bytes(ready), now);
    }
    Some(())
}

/// This service runs while the fullscreen player, local menu, or a radar clip
/// owns presentation and no new frame is admitted. Matching peers still
/// exchange transport records and keep their physical connections alive.
pub(crate) fn service(state: &mut AppState, wall_ms: u64) {
    if !enabled(state) || state.match_state.scenario_exit.is_some() {
        return;
    }
    advance_movies(state);
    let now = frame_bucket(wall_ms);
    if pending(state)
        && state
            .match_state
            .sim_runtime
            .runtime()
            .is_some_and(|runtime| runtime.simulation.session().chrono_screen_active())
        && state
            .match_state
            .match_presentation
            .authored_narrative
            .last_chrono_step
            .replace(now)
            != Some(now)
    {
        // The animation can release a native untimed pause. It owns its
        // service steps while the next gameplay frame is held.
        super::scenario_pause::apply_service(state, ScenarioServiceInput::StepChronoScreen);
    }
    let Some(local) = super::network_match::local_source(&state.match_state.session) else {
        return;
    };
    let connected = state.match_state.session.network_connected_source_indices();
    let narrative = &mut state.match_state.match_presentation.authored_narrative;
    if let Some(round) = narrative.round {
        let expired: Vec<_> = connected
            .iter()
            .copied()
            .filter(|&source| source != local && narrative.expired(round, source, now))
            .collect();
        for source in expired {
            super::network_match::kick(state, source, super::network_match::ConnectionError::Lost);
        }
    }
    let connected = state.match_state.session.network_connected_source_indices();
    let scenario_paused = state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.simulation.session().scenario_paused());
    let presentation = &mut state.match_state.match_presentation;
    let narrative = &mut presentation.authored_narrative;
    if let Some(round) = narrative.round {
        let pause_ready = if round.timed {
            presentation.scenario_pause.elapsed(wall_ms)
        } else {
            !scenario_paused
        };
        if narrative.playing.is_none()
            && !narrative.queue.iter().any(Clip::blocking)
            && pause_ready
            && narrative.local_ready != round.id
        {
            narrative.local_ready = round.id;
            narrative.last_probe = None;
        }
        if narrative.all_ready(round, local, connected) {
            narrative.round = None;
            if round.timed {
                super::scenario_pause::apply_service(state, ScenarioServiceInput::ExpireTimedPause);
            }
            arm_after_frame(state, wall_ms);
            super::scenario_pause::resume(state, wall_ms);
            state.platform.window.request_redraw();
        }
    }
    probe(state, local, now);
}

fn advance_movies(state: &mut AppState) {
    let playing = state
        .match_state
        .match_presentation
        .authored_narrative
        .playing;
    let pending_frame = state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.simulation.pending_network_wait());
    if pending_frame || state.frontend.screen != GameScreen::InGame {
        return;
    }
    if matches!(playing, Some(Playing::Radar)) && crate::app::radar_movie::has_pending_pause(state)
    {
        return;
    }
    state
        .match_state
        .match_presentation
        .authored_narrative
        .playing = None;
    while let Some(clip) = state
        .match_state
        .match_presentation
        .authored_narrative
        .queue
        .pop_front()
    {
        let playing = match clip {
            Clip::Fullscreen(index) => {
                let name = state
                    .process_assets
                    .native_rules()
                    .and_then(|rules| rules.movies().name(index))
                    .map(str::to_owned);
                if let Some(name) = name {
                    App::start_fullscreen_movie(state, &name, MovieReturn::RunningScenario);
                }
                state
                    .frontend
                    .fullscreen_movie
                    .is_some()
                    .then_some(Playing::Fullscreen)
            }
            Clip::Radar { index, blocking } => {
                let admitted = state.match_state.match_presentation.radar_anim.is_some()
                    && crate::app::radar_movie::enqueue(state, index, blocking);
                (admitted && blocking).then_some(Playing::Radar)
            }
        };
        if let Some(playing) = playing {
            state
                .match_state
                .match_presentation
                .authored_narrative
                .playing = Some(playing);
            break;
        }
    }
}

fn probe(state: &mut AppState, local: usize, now: u32) {
    let narrative = &mut state.match_state.match_presentation.authored_narrative;
    if (narrative.round.is_none() && narrative.local_ready == 0)
        || narrative
            .last_probe
            .is_some_and(|last| bucket_delta(now, last) < PROBE_BUCKETS)
    {
        return;
    }
    let Some(scope) = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.setup_scope())
    else {
        return;
    };
    let mut bytes = [0; 4 + SessionScope::LEN + 8];
    bytes[..4].copy_from_slice(MAGIC);
    bytes[4..4 + SessionScope::LEN].copy_from_slice(&scope.encode());
    bytes[4 + SessionScope::LEN..].copy_from_slice(&narrative.local_ready.to_le_bytes());
    narrative.last_probe = Some(now);
    if let Some(live) = state.match_state.live_network.as_mut() {
        for &source in state.match_state.session.network_connected_source_indices() {
            if source != local
                && let Some(endpoint) = state.match_state.session.network_endpoint(source)
            {
                live.send_global(&bytes, false, GlobalDestination::direct(endpoint));
            }
        }
    }
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;

    #[test]
    fn observers_hold_the_gate_until_ready_or_disconnected() {
        let mut narrative = AuthoredNarrative {
            local_ready: 1,
            ..Default::default()
        };
        let round = Round {
            id: 1,
            opened: 0,
            timed: false,
        };
        narrative.observe(1, 1, 0);
        assert!(!narrative.all_ready(round, 0, &[0, 1, 2]));
        assert!(narrative.all_ready(round, 0, &[0, 1]));
        narrative.observe(2, 1, 1);
        assert!(narrative.all_ready(round, 0, &[0, 1, 2]));
    }

    #[test]
    fn early_and_reordered_receipts_preserve_only_reachable_readiness() {
        let mut narrative = AuthoredNarrative::default();
        narrative.observe(1, 2, 0);
        narrative.observe(1, 1, 1);
        narrative.observe(1, 0, 2);
        assert_eq!(narrative.peers[&1].ready, 1);
        assert_eq!(narrative.peers[&1].heard_at, Some(2));
    }

    #[test]
    fn a_connected_slow_viewer_does_not_time_out() {
        let mut narrative = AuthoredNarrative::default();
        let round = Round {
            id: 1,
            opened: 0,
            timed: true,
        };
        let timeout = crate::net::live_commands::LiveCommandMode::Lan
            .wait_timing()
            .timeout as u32;
        narrative.observe(1, 0, timeout - 1);
        assert!(!narrative.expired(round, 1, timeout));
        assert!(narrative.expired(round, 2, timeout));
        assert!(narrative.expired(round, 1, 2 * timeout));
        let before_wrap = frame_bucket(u64::from(u32::MAX)) - 5;
        let round = Round {
            opened: before_wrap,
            ..round
        };
        narrative.observe(1, 0, before_wrap);
        assert!(!narrative.expired(round, 1, 1));
    }
}
