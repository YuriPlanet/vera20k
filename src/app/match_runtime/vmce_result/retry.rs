//! VMCE's defeat-page agreement. The frozen start and admitted endpoints
//! remain in MatchSession. The LAN service carries these records on its
//! existing reliable global route.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::app::frontend::score_overlay::room_result_active;
use crate::app::lan_service;
use crate::app::match_bootstrap::LoadingStartup;
use crate::app::match_runtime::frame_pacer::{frame_bucket, wall_clock_ms};
use crate::app::match_runtime::session::{SessionMode, WallBucketTimer};
use crate::app::{App, AppState, GameScreen};
use crate::net::global::{GlobalDestination, GlobalMessage};
use crate::net::lan_messages::SessionScope;

const MAGIC: &[u8; 4] = b"VRR1";
const HANDOFF_BUCKETS: u32 = 600;

#[derive(Clone, Copy)]
#[repr(u8)]
enum Record {
    Retry = 1,
    Cancel = 2,
    Prepare = 3,
    Confirm = 4,
    Start = 5,
}

impl Record {
    fn decode(bytes: &[u8]) -> Result<(Self, SessionScope)> {
        let (scope, [kind]) = bytes
            .strip_prefix(MAGIC)
            .and_then(|payload| payload.split_at_checked(SessionScope::LEN))
            .context("invalid retry record length")?
        else {
            anyhow::bail!("invalid retry record length");
        };
        let record = match kind {
            1 => Self::Retry,
            2 => Self::Cancel,
            3 => Self::Prepare,
            4 => Self::Confirm,
            5 => Self::Start,
            _ => anyhow::bail!("unknown retry record"),
        };
        Ok((
            record,
            SessionScope::decode(scope).context("invalid retry scope")?,
        ))
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Waiting,
    CheckingFiles(WallBucketTimer),
    Preparing(WallBucketTimer),
    Prepared(WallBucketTimer),
    Starting(WallBucketTimer),
    Returning(WallBucketTimer, ResultExit),
    Canceled,
}

impl Phase {
    fn can_leave(self) -> bool {
        matches!(self, Self::Waiting | Self::Canceled)
    }

    fn deadline(self) -> Option<WallBucketTimer> {
        match self {
            Self::CheckingFiles(timer)
            | Self::Preparing(timer)
            | Self::Prepared(timer)
            | Self::Starting(timer)
            | Self::Returning(timer, _) => Some(timer),
            Self::Waiting | Self::Canceled => None,
        }
    }

    fn failure_step(self) -> Step {
        match self {
            // A prepared guest has given the host permission to send Start.
            // A starting host has queued it and a guest may already be loading.
            Self::Prepared(_) | Self::Starting(_) => Step::RecoverStart,
            _ => Step::Cancel,
        }
    }
}

pub(crate) struct RetryRound {
    scope: SessionScope,
    phase: Phase,
    votes: BTreeSet<usize>,
    confirmed: BTreeSet<usize>,
}

impl RetryRound {
    fn new(scope: SessionScope) -> Self {
        Self {
            scope,
            phase: Phase::Waiting,
            votes: BTreeSet::new(),
            confirmed: BTreeSet::new(),
        }
    }

    fn receive_prepare(&mut self, local: usize, observer: bool, visible: bool, now: u32) {
        // The result pump confirms a watching client's queued preparation
        // after its defeat page is active.
        if (observer || (visible && self.votes.contains(&local)))
            && matches!(self.phase, Phase::Waiting)
        {
            self.phase = Phase::CheckingFiles(WallBucketTimer::start(now, HANDOFF_BUCKETS));
        }
    }

    fn receive_cancel(&mut self, from_host: bool) -> Step {
        match self.phase {
            Phase::Starting(_) => Step::RecoverStart,
            Phase::Returning(..) | Phase::Canceled => Step::Wait,
            // Only the host can resolve the prepared guest's uncertainty about
            // whether the Start batch was queued. It sends Cancel before Start.
            Phase::Prepared(_) if !from_host => Step::Wait,
            _ => {
                self.phase = Phase::Canceled;
                Step::Cancel
            }
        }
    }

    fn step(
        &self,
        host: bool,
        players: usize,
        participants: usize,
        now: u32,
        healthy: bool,
        drained: bool,
    ) -> Step {
        let expired = self
            .phase
            .deadline()
            .is_some_and(|timer| timer.remaining(now) == 0);
        match self.phase {
            Phase::Returning(_, exit) if drained || expired || !healthy => Step::Return(exit),
            Phase::CheckingFiles(_)
            | Phase::Preparing(_)
            | Phase::Prepared(_)
            | Phase::Starting(_)
                if expired || !healthy =>
            {
                self.phase.failure_step()
            }
            Phase::Waiting if !healthy && !self.votes.is_empty() => Step::Cancel,
            Phase::Waiting if host && self.votes.len() == players => Step::Prepare,
            Phase::CheckingFiles(_) => Step::Confirm,
            Phase::Preparing(_) if self.confirmed.len() == participants => Step::SendStart,
            Phase::Starting(_) if drained => Step::Start,
            _ => Step::Wait,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultExit {
    Room,
    Lan,
}

impl ResultExit {
    fn finish(self, state: &mut AppState) {
        match self {
            Self::Room => App::leave_mission_result_screen(state),
            Self::Lan => App::leave_lan_score(state),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Wait,
    Prepare,
    Confirm,
    SendStart,
    Start,
    Return(ResultExit),
    Cancel,
    RecoverStart,
}

fn current(state: &AppState) -> Option<&RetryRound> {
    state
        .match_state
        .lan_service
        .as_ref()?
        .result_round
        .as_ref()
}

fn active(state: &AppState) -> bool {
    room_result_active(state)
        && state.match_state.session_mode() == SessionMode::Lan
        && state.frontend.lan_rematch.is_some()
}

fn scope(state: &AppState) -> Option<SessionScope> {
    state.match_state.lan_service.as_ref()?.setup_scope()
}

fn round_mut(state: &mut AppState, scope: SessionScope) -> Result<&mut RetryRound> {
    let service = state
        .match_state
        .lan_service
        .as_mut()
        .context("retry has no LAN service")?;
    ensure!(
        service.setup_scope() == Some(scope),
        "retry belongs to another match"
    );
    let round = service
        .result_round
        .get_or_insert_with(|| RetryRound::new(scope));
    ensure!(round.scope == scope, "retry belongs to a retired result");
    Ok(round)
}

fn full_roster(state: &AppState) -> Result<&crate::skirmish_launch::SkirmishLaunchSession> {
    let session = &state.match_state.session;
    let start = session
        .frozen_authored_start()
        .context("no accepted authored start")?;
    let launch = start.launch_session();
    ensure!(
        session.network_connected_source_indices().len() == launch.humans.len(),
        "an admitted player has left the match"
    );
    Ok(launch)
}

fn send(state: &mut AppState, scope: SessionScope, record: Record) -> Result<()> {
    let session = &state.match_state.session;
    let local = session
        .network_setup()
        .context("retry has no admitted roster")?
        .local_player_index;
    let peers: Vec<_> = session
        .network_connected_source_indices()
        .iter()
        .copied()
        .filter(|&source| source != local)
        .map(|source| {
            session
                .network_endpoint(source)
                .context("a retry player has no endpoint")
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&endpoint| {
            !matches!(record, Record::Confirm)
                || lan_service::ensure_host_source(state, endpoint).is_ok()
        })
        .collect();
    if matches!(record, Record::Confirm) {
        ensure!(peers.len() == 1, "retry has no admitted host");
    }
    let bytes = [MAGIC.as_slice(), &scope.encode(), &[record as u8]].concat();
    let service = state
        .match_state
        .lan_service
        .as_mut()
        .context("retry has no LAN service")?;
    ensure!(
        service.queue_setup(
            peers
                .into_iter()
                .map(|endpoint| { (bytes.clone(), GlobalDestination::direct(endpoint)) })
        )?,
        "LAN result send queue is full"
    );
    Ok(())
}

pub(super) fn status(state: &AppState) -> Option<String> {
    use crate::i18n::tr;

    let round = current(state)?;
    match round.phase {
        Phase::Waiting if !round.votes.is_empty() => {
            let playing = full_roster(state)
                .ok()?
                .humans
                .iter()
                .filter(|player| !player.is_observer())
                .count();
            Some(format!(
                "{} ({}/{playing})",
                tr("VERA:VmceRetryWaiting"),
                round.votes.len(),
            ))
        }
        Phase::Waiting | Phase::Returning(..) => None,
        Phase::CheckingFiles(_) | Phase::Preparing(_) | Phase::Prepared(_) | Phase::Starting(_) => {
            Some(tr("VERA:VmceRetryStarting"))
        }
        Phase::Canceled => Some(tr("VERA:VmceRetryCanceled")),
    }
}

pub(super) fn can_vote(state: &AppState) -> bool {
    active(state)
        && full_roster(state).is_ok()
        && current(state).is_none_or(|round| {
            matches!(round.phase, Phase::Waiting)
                && state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| !round.votes.contains(&launch.local_player_index))
        })
}

pub(super) fn can_leave(state: &AppState) -> bool {
    current(state).is_none_or(|round| round.phase.can_leave())
}

pub(super) fn retry_pressed(state: &mut AppState) {
    let Some(scope) = scope(state).filter(|_| active(state)) else {
        return;
    };
    if state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| launch.local_player().is_observer())
    {
        return;
    }
    lan_service::hold_match_transport(state);
    let result = (|| -> Result<()> {
        let local = full_roster(state)?.local_player_index;
        let round = round_mut(state, scope)?;
        if matches!(round.phase, Phase::Waiting) && round.votes.insert(local) {
            send(state, scope, Record::Retry)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        fail(state, scope, &error);
    }
    state.platform.window.request_redraw();
}

pub(super) fn return_pressed(state: &mut AppState) {
    exit_result(state, ResultExit::Room);
}

pub(super) fn leave_pressed(state: &mut AppState) {
    exit_result(state, ResultExit::Lan);
}

fn exit_result(state: &mut AppState, exit: ResultExit) {
    let Some(scope) = scope(state).filter(|_| active(state)) else {
        exit.finish(state);
        return;
    };
    let Ok(round) = round_mut(state, scope) else {
        return;
    };
    if !round.phase.can_leave() {
        return;
    }
    let notify = matches!(round.phase, Phase::Waiting);
    round.phase = Phase::Returning(
        WallBucketTimer::start(frame_bucket(wall_clock_ms()), HANDOFF_BUCKETS),
        exit,
    );
    lan_service::hold_match_transport(state);
    if notify && let Err(error) = send(state, scope, Record::Cancel) {
        log::debug!("VMCE result return could not notify the room: {error:#}");
        exit.finish(state);
    }
    state.platform.window.request_redraw();
}

fn fail(state: &mut AppState, scope: SessionScope, error: &anyhow::Error) {
    let Ok(round) = round_mut(state, scope) else {
        log::debug!("VMCE retry round retired before failure: {error:#}");
        return;
    };
    if round.phase.failure_step() == Step::RecoverStart {
        recover_start(state, &format!("{error:#}"));
        return;
    }
    log::warn!("VMCE retry canceled: {error:#}");
    round.phase = Phase::Canceled;
    if let Err(error) = send(state, scope, Record::Cancel) {
        log::debug!("VMCE retry cancel could not notify the room: {error:#}");
    }
    state.platform.window.request_redraw();
}

fn recover_start(state: &mut AppState, reason: &str) {
    log::warn!("VMCE retry handoff interrupted: {reason}");
    let guest_stage = crate::app::shell_lan::prepare_initial_failure_return(state);
    App::close_finished_scenario(state);
    if state.match_state.session.host() {
        App::return_to_lan_room_after_score(state, true);
    } else {
        App::resume_shell_after_match(state);
        if let Some(stage) = guest_stage {
            crate::app::shell_lan::offer_ready_on_return(state, stage);
        }
    }
    App::open_shell_notice(state, crate::i18n::tr("VERA:VmceRetryInterrupted"));
}

/// The normal receive path can see a peer's button before this machine's
/// result finishes presenting. Those choices are inert until BC6 is open.
pub(crate) fn receive_result(
    state: &mut AppState,
    message: &GlobalMessage,
    now: u32,
) -> Option<Result<()>> {
    message
        .payload
        .starts_with(MAGIC)
        .then(|| receive(state, message, now))
}

fn receive(state: &mut AppState, message: &GlobalMessage, now: u32) -> Result<()> {
    ensure!(message.product_id == 0xaa04, "different LAN product");
    let (record, scope) = Record::decode(&message.payload)?;
    let visible = active(state);
    if !visible
        && state.frontend.screen != GameScreen::InGame
        && !state
            .frontend
            .fullscreen_movie
            .as_ref()
            .is_some_and(|movie| movie.precedes_lan_defeat_result())
    {
        return Ok(());
    }
    let session = &state.match_state.session;
    ensure!(
        session.frozen_authored_start().is_some() && state.frontend.lan_rematch.is_some(),
        "no authored result admission"
    );
    let launch = full_roster(state)?;
    let local = launch.local_player_index;
    let local_observer = launch.local_player().is_observer();
    let source = session
        .network_endpoint_source(message.source)
        .filter(|&source| {
            source != local && session.network_connected_source_indices().contains(&source)
        })
        .context("retry sender is not an admitted remote player")?;
    let host = session.host();
    let source_plays = launch
        .humans
        .get(source)
        .is_some_and(|human| !human.is_observer());
    if matches!(record, Record::Prepare | Record::Start) {
        lan_service::ensure_host_source(state, message.source)?;
    }
    let host_cancel = matches!(record, Record::Cancel)
        && lan_service::ensure_host_source(state, message.source).is_ok();
    let round = round_mut(state, scope)?;
    match record {
        // Everyone can display the confirmed count. Only the host takes
        // Prepare in step() after all playing sources have voted.
        Record::Retry if source_plays && matches!(round.phase, Phase::Waiting) => {
            round.votes.insert(source);
        }
        Record::Cancel => match round.receive_cancel(host_cancel) {
            Step::RecoverStart => recover_start(state, "a peer withdrew during the start handoff"),
            Step::Cancel if host => send(state, scope, Record::Cancel)?,
            _ => {}
        },
        Record::Prepare => round.receive_prepare(local, local_observer, visible, now),
        Record::Confirm if host && matches!(round.phase, Phase::Preparing(_)) => {
            round.confirmed.insert(source);
        }
        Record::Start if visible && matches!(round.phase, Phase::Prepared(_)) => {
            if let Err(error) = start(state, scope) {
                recover_start(state, &format!("{error:#}"));
            }
        }
        _ => {}
    }
    if visible {
        state.platform.window.request_redraw();
    }
    Ok(())
}

fn verify_source(state: &AppState) -> Result<()> {
    let start = state
        .match_state
        .session
        .frozen_authored_start()
        .context("the frozen start retired")?;
    crate::app::shell_network_setup::local_scenarios::verify_replay_source(state, start)
}

fn start(state: &mut AppState, scope: SessionScope) -> Result<()> {
    full_roster(state)?;
    let startup = state
        .match_state
        .session
        .frozen_authored_start()
        .context("retry lost the admitted start")?
        .clone();
    state
        .match_state
        .lan_service
        .as_mut()
        .context("retry lost the admitted transport")?
        .prepare_match_reload(scope)?;
    App::close_finished_scenario(state);
    // Keep the frozen start and scope, but retire the finished attempt's queues.
    let session = &mut state.match_state.session;
    drop(session.take_network_loading_progress_reports());
    drop(session.take_network_send_requests());
    crate::app::loading::pump::begin_loading(
        state,
        crate::app::loading::pump::LoadingRequest::network(LoadingStartup::Network(startup)),
    );
    Ok(())
}

fn poll_step(state: &mut AppState, scope: SessionScope, now: u32, service_ok: bool) -> Result<()> {
    let service = state
        .match_state
        .lan_service
        .as_ref()
        .context("retry lost its LAN service")?;
    let round = service
        .result_round
        .as_ref()
        .context("no retry agreement")?;
    let observation = service
        .transport_observation(service_ok)
        .context("no retained result transport")?;
    let roster = full_roster(state);
    let players = roster.as_ref().map_or(usize::MAX, |launch| {
        launch
            .humans
            .iter()
            .filter(|human| !human.is_observer())
            .count()
    });
    let participants = roster
        .as_ref()
        .map_or(usize::MAX, |launch| launch.humans.len());
    let step = round.step(
        state.match_state.session.host(),
        players,
        participants,
        now,
        observation.service_ok && roster.is_ok(),
        observation.queued == 0,
    );
    match step {
        Step::Wait => {}
        Step::Return(exit) => exit.finish(state),
        Step::Prepare => {
            let local = roster?.local_player_index;
            verify_source(state)?;
            send(state, scope, Record::Prepare)?;
            let round = round_mut(state, scope)?;
            round.confirmed.insert(local);
            round.phase = Phase::Preparing(WallBucketTimer::start(now, HANDOFF_BUCKETS));
        }
        Step::Confirm => {
            verify_source(state)?;
            send(state, scope, Record::Confirm)?;
            round_mut(state, scope)?.phase =
                Phase::Prepared(WallBucketTimer::start(now, HANDOFF_BUCKETS * 2));
        }
        Step::SendStart => {
            roster?;
            send(state, scope, Record::Start)?;
            round_mut(state, scope)?.phase =
                Phase::Starting(WallBucketTimer::start(now, HANDOFF_BUCKETS));
        }
        Step::Start => start(state, scope)?,
        Step::Cancel => anyhow::bail!("the LAN result handoff did not complete"),
        Step::RecoverStart => recover_start(state, "the LAN start handoff did not complete"),
    }
    Ok(())
}

pub(crate) fn poll_result(state: &mut AppState, now: u32, service_ok: bool) -> bool {
    if !active(state) {
        return false;
    }
    lan_service::hold_match_transport(state);
    let Some(scope) = scope(state) else {
        return true;
    };
    let mut service_ok = service_ok;
    while active(state) {
        let Some(message) = state
            .match_state
            .lan_service
            .as_mut()
            .and_then(|service| service.pop_lobby_received(&state.match_state.session))
        else {
            break;
        };
        let received = receive_result(state, &message, now)
            .unwrap_or_else(|| lan_service::receive_authored_result_departure(state, &message));
        if let Err(error) = received {
            log::debug!("Ignored VMCE result record: {error:#}");
        }
        service_ok &= lan_service::service_transport(state, now);
    }
    if active(state) && current(state).is_some() {
        if let Some(service) = state.match_state.lan_service.as_mut() {
            service.flush_setup();
        }
        if let Err(error) = poll_step(state, scope, now, service_ok) {
            fail(state, scope, &error);
        }
    }
    true
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;

    #[test]
    fn an_observer_retains_prepare_before_the_result_while_a_player_must_vote() {
        let scope = SessionScope {
            epoch: [1; 16],
            revision: 6,
        };
        let mut observer = RetryRound::new(scope);
        observer.receive_prepare(1, true, false, 10);
        assert_eq!(observer.step(false, 1, 2, 11, true, true), Step::Confirm);
        assert!(!observer.phase.can_leave());

        let mut player = RetryRound::new(scope);
        player.receive_prepare(1, false, true, 10);
        assert_eq!(player.step(false, 2, 2, 11, true, true), Step::Wait);
        assert!(player.phase.can_leave());
    }

    #[test]
    fn partial_start_delivery_cannot_return_to_the_finished_result() {
        let mut round = RetryRound::new(SessionScope {
            epoch: [1; 16],
            revision: 6,
        });
        let deadline = WallBucketTimer::start(10, HANDOFF_BUCKETS);

        round.phase = Phase::Preparing(deadline);
        assert_eq!(round.step(true, 3, 3, 11, false, false), Step::Cancel);

        round.phase = Phase::Starting(deadline);
        assert_eq!(round.step(true, 3, 3, 11, true, false), Step::Wait);
        assert_eq!(round.step(true, 3, 3, 11, true, true), Step::Start);
        assert_eq!(round.step(true, 3, 3, 11, false, false), Step::RecoverStart);
        assert_eq!(round.receive_cancel(false), Step::RecoverStart);

        round.phase = Phase::Prepared(deadline);
        assert_eq!(
            round.step(false, 3, 3, 10 + HANDOFF_BUCKETS, true, false),
            Step::RecoverStart
        );
        assert_eq!(round.receive_cancel(false), Step::Wait);
        assert_eq!(round.receive_cancel(true), Step::Cancel);
        assert!(round.phase.can_leave());
    }
}
