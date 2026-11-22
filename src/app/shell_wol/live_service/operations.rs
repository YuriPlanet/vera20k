//! `LiveService`'s own operations: opening the lobby worker, sending and
//! retrying commands, and the finish, leave, logout, relay-renewal and report
//! bookkeeping the service keeps for its visit.

#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
use super::DEFAULT_LOBBY_URL;
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
use super::lobby_base_url;
use super::{
    ENDED_AGO_LIMIT, LiveService, MatchReport, RELAY_GRANT_LIFETIME, RELAY_RENEWAL_MARGIN,
    RELAY_RENEWAL_RETRY_MS, RELAY_TIMEOUT, RetryingCommand, RoomAgreement, Translations,
    WatchStart, Watching, decode, http, maps, relay_failure_line, relay_uuid, update_required_text,
    wire,
};
use crate::util::time::Instant;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, Sender};

impl LiveService {
    pub(super) fn new() -> Result<Self> {
        Self::open(None)
    }

    #[cfg(all(
        feature = "wasm",
        not(feature = "wasm-net"),
        target_arch = "wasm32",
        target_os = "unknown"
    ))]
    pub(super) fn open(_service_url: Option<&str>) -> Result<Self> {
        anyhow::bail!("Online play is unavailable in the single-player browser build")
    }

    /// `service_url` replaces the configured lobby with an external launch's
    /// already validated match API origin (HTTPS, or loopback HTTP).
    #[cfg(any(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        feature = "wasm-net"
    ))]
    pub(super) fn open(service_url: Option<&str>) -> Result<Self> {
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let lobby = match service_url {
            Some(url) => url.to_owned(),
            None => lobby_base_url()?,
        }
        .trim_end_matches('/')
        .to_owned();
        #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
        let lobby = service_url
            .unwrap_or(DEFAULT_LOBBY_URL)
            .trim_end_matches('/')
            .to_owned();
        let (requests, input) = mpsc::channel();
        let (output, responses) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = Arc::clone(&stopping);
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let base = lobby.clone();
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        std::thread::Builder::new()
            .name("vera-lobby".into())
            .spawn(move || http::run(base, input, output, worker_stopping))
            .context("could not start online lobby worker")?;
        #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
        let browser_requests = http::start_browser(input, output, worker_stopping, service_url)?;
        Ok(Self::with_worker(
            lobby,
            stopping,
            requests,
            responses,
            #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
            browser_requests,
        ))
    }

    /// The service over its lobby worker's channels, before any request.
    #[cfg(any(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        feature = "wasm-net"
    ))]
    pub(super) fn with_worker(
        lobby: String,
        stopping: Arc<AtomicBool>,
        requests: Sender<http::Request>,
        responses: Receiver<http::Response>,
        #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
        browser_requests: http::RequestOwner,
    ) -> Self {
        Self {
            #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
            _browser_requests: browser_requests,
            stopping,
            requests,
            responses,
            next_request: 1,
            auth_attempt: 0,
            revision: 0,
            room: RoomAgreement::default(),
            start_preparation: None,
            locally_refused_start: false,
            player_id: String::new(),
            member_ids: BTreeMap::new(),
            pending: None,
            match_id: None,
            switching_room: None,
            finishing: None,
            leaving: None,
            logging_out: None,
            next_renewal_attempt: None,
            relay_renewal_due: None,
            relay_lapses_at: None,
            maps: Default::default(),
            translations: Translations::default(),
            spawned: false,
            back_to_room: false,
            finish_result: None,
            finish_winners: None,
            finish_decided: None,
            finish_opponents_left: false,
            network_quality: None,
            relay_timeout: RELAY_TIMEOUT,
            outdated_notice: None,
            left_for_match: false,
            ladder_lookup: None,
            lobby,
            signing_in_remembered: false,
            watching: None,
            spawned_watch: None,
        }
    }

    /// The first lobby-upgrade refusal of this sign-in, as one notice; the
    /// sign-in's later refused requests add none.
    pub(super) fn outdated_notice(&mut self, message: Option<&str>) -> Option<String> {
        (self.outdated_notice != Some(self.auth_attempt)).then(|| {
            self.outdated_notice = Some(self.auth_attempt);
            update_required_text(message)
        })
    }

    pub(super) fn publish_room_options(&mut self, options: wire::Options) {
        let options = json!(options);
        let id = self.command_with_id("options", json!({"options": &options}));
        self.room.published_options = Some((id, options));
    }

    pub(super) fn is_room_publication(&self, id: u64) -> bool {
        self.room
            .published_options
            .as_ref()
            .is_some_and(|(sent, _)| *sent == id)
    }

    pub(super) fn take_legacy_refusal(
        &mut self,
        id: u64,
        watches: bool,
        current: impl FnOnce() -> Option<Value>,
    ) -> bool {
        if !self.is_room_publication(id) {
            return false;
        }
        self.room
            .published_options
            .take()
            .is_some_and(|(_, sent)| !watches && current().as_ref() == Some(&sent))
    }

    pub(super) fn command(&mut self, operation: &'static str, payload: Value) {
        self.command_with_id(operation, payload);
    }

    pub(super) fn publish_options_agreement(
        &mut self,
        mut agreement: Value,
        status: u8,
        host: bool,
    ) {
        // Protocol v4 shares the agreement fields, but ready rejects status.
        let mut map_status = agreement.clone();
        map_status["status"] = json!(status);
        self.command("map_status", map_status);
        if host {
            agreement["ready"] = json!(true);
            self.command("ready", agreement);
        }
    }

    pub(super) fn command_with_id(&mut self, operation: &'static str, payload: Value) -> u64 {
        let id = self.next_request;
        self.next_request = self.next_request.wrapping_add(1);
        self.send_command(id, operation, payload);
        id
    }

    pub(super) fn send_command(&self, id: u64, operation: &'static str, payload: Value) {
        // A launcher's watcher's bearer acts as no player (docs/spawn.md
        // "Watching"); the worker refuses its commands too.
        if self.spawned_watch.is_some() {
            return;
        }
        let _ = self.requests.send(http::Request::Command {
            id,
            operation,
            payload,
        });
    }

    /// Send a command its owner retries until it is acknowledged.
    pub(super) fn retrying_command(
        &mut self,
        operation: &'static str,
        payload: Value,
        now: Instant,
    ) -> RetryingCommand {
        RetryingCommand {
            request_id: self.command_with_id(operation, payload.clone()),
            operation,
            payload,
            next_attempt: now + RELAY_TIMEOUT,
            failures: 0,
        }
    }

    /// The `ladder` command for the ladder (`0x007A3100`) or high-score
    /// (`0x007A3B30`) table. Its one event answers both, so the shell's
    /// high-score request for the names it just asked the ladder about
    /// shares that command.
    pub(super) fn look_up_ladder(&mut self, names: Vec<String>, high_scores: bool) {
        if high_scores && self.ladder_lookup.take().as_ref() == Some(&names) {
            return;
        }
        self.command("ladder", json!({"names":names}));
        self.ladder_lookup = (!high_scores).then_some(names);
    }

    /// The match returned to the shell: finish it, a lobby match with its
    /// result and winners. A spawned match reports when the process exits,
    /// and a watched one is only left.
    pub(super) fn match_returned(&mut self, (result, winners, decided): MatchReport, now: Instant) {
        if self.stop_watching() {
            return;
        }
        if !self.spawned && self.finishing.is_none() {
            self.finish_result = result;
            self.finish_winners = winners;
            self.finish_decided = decided;
        }
        self.begin_finish(now);
    }

    /// Send the match's `finish`, its payload built once from what the match
    /// reported: every retry sends it as it was.
    pub(super) fn begin_finish(&mut self, now: Instant) {
        let Some(match_id) = self.match_id.clone() else {
            return;
        };
        if self.finishing.is_some() {
            return;
        }
        self.next_renewal_attempt = None;
        self.relay_renewal_due = None;
        self.relay_lapses_at = None;
        let payload = self.finish_payload(&match_id, now);
        self.back_to_room = false;
        self.network_quality = None;
        self.finish_result = None;
        self.finish_winners = None;
        self.finish_decided = None;
        self.finishing = Some(self.retrying_command("finish", payload, now));
    }

    /// A lobby match this game plays, loads or shows the score of, whose
    /// `finish` has not begun. A launcher's match reports in its own result
    /// (`spawn_result.json`).
    pub(super) fn match_unfinished(&self) -> bool {
        !self.spawned && self.match_id.is_some() && self.finishing.is_none()
    }

    /// A lobby `game_started` is already committed for every member. If this
    /// game cannot adopt it, the existing authenticated `finish` cancels that
    /// exact pending match for everyone. Leave targets a room, not a match.
    pub(super) fn receive_lobby_start(
        &mut self,
        payload: Value,
        admit: impl FnOnce(&mut Self, wire::Start) -> Result<()>,
    ) -> Result<()> {
        if self.pending.is_some()
            || self.match_id.is_some()
            || self.watching.is_some()
            || self.spawned
            || self.admission_blocked()
        {
            return Ok(());
        }
        self.locally_refused_start = false;
        let match_id = payload["match_id"]
            .as_str()
            .context("missing start match")?;
        relay_uuid(match_id)?;
        let match_id = match_id.to_owned();
        let result = decode(payload).and_then(|start: wire::Start| {
            ensure!(start.revision == self.revision, "stale options revision");
            admit(self, start)
        });
        self.start_preparation = None;
        if result.is_err() {
            self.locally_refused_start = true;
            self.hold_match(Some(match_id));
            self.begin_finish(Instant::now());
        }
        result
    }

    /// Keep the network report of the lobby match whose transport just
    /// returned, for its `finish`; a finish already sent stays as it was.
    pub(super) fn keep_network_report(&mut self, report: Value) {
        if self.match_unfinished() {
            self.network_quality = Some(report);
        }
    }

    /// The lobby's `finish` is `{match_id}`, with `back_to_room` when the
    /// player returns to the room it kept and `network_quality` when the match
    /// played; a spawned match adds its local result for the launcher
    /// (docs/spawn.md), and a lobby match that ended its local result,
    /// `winners` and `ended_ago`, the whole seconds since its result was
    /// decided at `now`, for the ranked ladder (docs/lobby-protocol.md).
    pub(super) fn finish_payload(&self, match_id: &str, now: Instant) -> Value {
        let mut payload = json!({"match_id":match_id});
        if self.back_to_room {
            payload["back_to_room"] = json!(true);
        }
        if let Some(report) = &self.network_quality {
            payload["network_quality"] = report.clone();
        }
        if self.spawned {
            payload["result"] = json!(self.finish_result.unwrap_or("quit"));
            payload["opponents_left"] = json!(self.finish_opponents_left);
        } else if let Some(result) = self.finish_result {
            payload["result"] = json!(result);
            if let Some(winners) = &self.finish_winners {
                payload["winners"] = json!(winners);
            }
            if let Some(decided) = self.finish_decided {
                let ago = now.saturating_duration_since(decided).as_secs();
                payload["ended_ago"] = json!(ago.min(ENDED_AGO_LIMIT));
            }
        }
        payload
    }

    pub(super) fn finish_completed(&mut self, id: u64) {
        if self
            .finishing
            .as_ref()
            .is_some_and(|finish| finish.request_id == id)
        {
            self.finishing = None;
            self.hold_match(None);
            self.left_for_match = false;
        }
    }

    /// True when the lobby refused `back_to_room` (a lobby from before rooms
    /// came back): the finish retries without it and the room is not kept.
    pub(super) fn finish_failed(
        &mut self,
        id: u64,
        failure: http::CommandFailure,
        now: Instant,
    ) -> bool {
        if failure == http::CommandFailure::AlreadyFinished {
            self.finish_completed(id);
            return false;
        }
        let Some(finish) = self
            .finishing
            .as_mut()
            .filter(|finish| finish.request_id == id)
        else {
            return false;
        };
        finish.failed(now);
        if !matches!(failure, http::CommandFailure::Rejected(_)) {
            return false;
        }
        // A lobby from before leaving was timed by the game refuses
        // `ended_ago`, one from before team ladders `winners`, one from
        // before match reports `network_quality` and one from before rooms
        // came back `back_to_room`: the finish goes again without the newest,
        // and then as before. A refused finish committed nothing, so its
        // request ID can carry the smaller payload.
        let Some(payload) = finish.payload.as_object_mut() else {
            return false;
        };
        for field in ["ended_ago", "winners", "network_quality"] {
            if payload.remove(field).is_some() {
                return false;
            }
        }
        payload.remove("back_to_room").is_some()
    }

    /// A room leave the shell asked for; one during a started match is noted
    /// for its `channel_left` ([`Self::room_closed`]).
    pub(super) fn leave_room(&mut self, now: Instant) {
        self.left_for_match |= self.match_id.is_some();
        self.begin_leave(now);
    }

    /// A `channel_left` closed the room under this client, rather than
    /// answering its own leave for its started match.
    pub(super) fn room_closed(&mut self) -> bool {
        !std::mem::take(&mut self.left_for_match)
    }

    pub(super) fn begin_leave(&mut self, now: Instant) {
        if self.leaving.is_none() {
            self.leaving = Some(self.retrying_command("leave", json!({}), now));
        }
    }

    pub(super) fn leave_completed(&mut self, id: u64) {
        if self
            .leaving
            .as_ref()
            .is_some_and(|leave| leave.request_id == id)
        {
            self.leaving = None;
        }
    }

    pub(super) fn leave_failed(&mut self, id: u64, now: Instant) {
        if let Some(leave) = self.leaving.as_mut().filter(|leave| leave.request_id == id) {
            leave.failed(now);
        }
    }

    pub(super) fn begin_logout(&mut self, now: Instant) {
        if self.logging_out.is_none() {
            self.logging_out = Some(self.retrying_command("logout", json!({}), now));
        }
    }

    pub(super) fn logout_completed(&mut self, id: u64) {
        if self
            .logging_out
            .as_ref()
            .is_some_and(|logout| logout.request_id == id)
        {
            self.clear_cleanup();
        }
    }

    pub(super) fn logout_failed(&mut self, id: u64, now: Instant) {
        if let Some(logout) = self
            .logging_out
            .as_mut()
            .filter(|logout| logout.request_id == id)
        {
            logout.failed(now);
        }
    }

    pub(super) fn clear_cleanup(&mut self) {
        self.room.published_options = None;
        self.locally_refused_start = false;
        self.finishing = None;
        self.leaving = None;
        self.left_for_match = false;
        self.logging_out = None;
        self.hold_match(None);
        self.watching = None;
        self.next_renewal_attempt = None;
        self.relay_renewal_due = None;
    }

    /// The one writer of the match this player is in. The worker holds the
    /// same match: it resumes it when the lobby has lost this session's
    /// event history, and finishes it before logging out a bearer it still
    /// holds. A room left for the match, as Quick Match leaves it, keeps it.
    pub(super) fn hold_match(&mut self, match_id: Option<String>) {
        if self.match_id != match_id {
            let _ = self
                .requests
                .send(http::Request::HoldMatch(match_id.clone()));
            self.match_id = match_id;
        }
    }

    /// A new sign-in under `auth_attempt`. The worker first logs out a bearer
    /// it still holds, after one attempt to finish this service's match with
    /// it; then that match, and any cleanup still owed for the old session,
    /// is dropped, whether or not the lobby answered.
    pub(super) fn sign_in(
        &mut self,
        nickname: String,
        credential: http::Credential,
        changes: String,
    ) {
        let _ = self.requests.send(http::Request::OpenSession {
            attempt: self.auth_attempt,
            nickname,
            credential,
            changes,
        });
        self.clear_cleanup();
    }

    /// A gameplay grant reached its UDP owner at `now_ms`.
    pub(super) fn relay_granted(&mut self, now_ms: u32) {
        let renew_after = (RELAY_GRANT_LIFETIME - RELAY_RENEWAL_MARGIN).as_millis() as u32;
        self.relay_renewal_due = Some(now_ms.wrapping_add(renew_after));
        self.relay_lapses_at = Some(now_ms.wrapping_add(RELAY_GRANT_LIFETIME.as_millis() as u32));
        self.next_renewal_attempt = None;
    }

    /// The newest gameplay grant lapsed by `now_ms`; false with none.
    pub(super) fn relay_pass_lapsed(&self, now_ms: u32) -> bool {
        self.relay_lapses_at
            .is_some_and(|lapses_at| deadline_reached(now_ms, lapses_at))
    }

    /// The lobby already reserved this match's place. Release it if its start
    /// cannot be decoded or installed, including before a relay route exists.
    pub(super) fn receive_watch_start(
        &mut self,
        match_id: &str,
        awaited: bool,
        prepare: impl FnOnce() -> Result<WatchStart>,
    ) -> Result<()> {
        if self
            .watching
            .as_ref()
            .is_some_and(|watch| watch.match_id == match_id)
        {
            return Ok(());
        }
        if !awaited || self.watching.is_some() || self.pending.is_some() || self.match_id.is_some()
        {
            self.command("unwatch", json!({"match_id": match_id}));
            return Ok(());
        }
        let start = prepare().inspect_err(|_| {
            self.command("unwatch", json!({"match_id": match_id}));
        })?;
        self.watching = Some(Watching {
            match_id: match_id.to_owned(),
            start: Some(start),
        });
        maps::watch(self, match_id.to_owned());
        Ok(())
    }

    /// This game stops watching: `unwatch` frees its place at once, which
    /// the lobby frees anyway once its grant runs out. False when it
    /// watches nothing.
    pub(super) fn stop_watching(&mut self) -> bool {
        let Some(watching) = self.watching.take() else {
            return false;
        };
        self.next_renewal_attempt = None;
        self.relay_renewal_due = None;
        self.relay_lapses_at = None;
        self.command("unwatch", json!({"match_id":watching.match_id}));
        true
    }

    pub(super) fn poll_renewal(&mut self, now_ms: u32) {
        if self.finishing.is_some() {
            return;
        }
        // A watcher renews its watch, a player its place in the match. A
        // launcher's watcher polls its events, which answer `watch_renewed`.
        let (operation, match_id) = match (&self.watching, &self.match_id, &self.spawned_watch) {
            (Some(watching), _, _) => ("renew_watch", &watching.match_id),
            (None, Some(match_id), _) => ("renew_relay", match_id),
            (None, None, Some(match_id)) => ("", match_id),
            (None, None, None) => return,
        };
        let Some(due) = self.relay_renewal_due else {
            return;
        };
        // Short-lived grants renew before expiry. Retry a missing reply without
        // replacing the live route or resetting the event cursor.
        if !deadline_reached(now_ms, due)
            || self
                .next_renewal_attempt
                .is_some_and(|next| !deadline_reached(now_ms, next))
        {
            return;
        }
        let match_id = match_id.clone();
        self.next_renewal_attempt = Some(now_ms.wrapping_add(RELAY_RENEWAL_RETRY_MS));
        if self.spawned_watch.is_some() {
            let _ = self.requests.send(http::Request::PollEvents);
        } else {
            self.command(operation, json!({"match_id":match_id}));
        }
    }

    pub(super) fn send_report(&mut self, operation: &'static str, payload: Value, now: Instant) {
        let report = self.retrying_command(operation, payload, now);
        if let Some(pending) = self.pending.as_mut() {
            pending.report = Some(report);
        }
    }

    pub(super) fn report_completed(&mut self, id: u64) {
        if let Some(pending) = self.pending.as_mut()
            && let Some(report) = pending.report.take_if(|report| report.request_id == id)
        {
            pending.relay_reported |= report.operation == "relay_ready";
        }
    }

    /// Whether the refused report will be resent: only a busy or unreachable
    /// lobby is asked again, any other refusal stands.
    pub(super) fn report_failed(
        &mut self,
        id: u64,
        failure: http::CommandFailure,
        now: Instant,
    ) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        let Some(report) = pending
            .report
            .as_mut()
            .filter(|report| report.request_id == id)
        else {
            return false;
        };
        if failure == http::CommandFailure::Retryable {
            report.failed(now);
            return true;
        }
        pending.report = None;
        false
    }

    pub(super) fn poll_report(&mut self, now: Instant) {
        if let Some((id, operation, payload)) = self
            .pending
            .as_mut()
            .and_then(|pending| pending.report.as_mut())
            .and_then(|report| report.take_due(now))
        {
            self.send_command(id, operation, payload);
        }
    }

    pub(super) fn admission_blocked(&self) -> bool {
        self.finishing.is_some() || self.leaving.is_some() || self.logging_out.is_some()
    }

    /// The line for a pending start whose barrier ran out by `now`. Until
    /// `start_confirmed` the barrier is relay admission; after it, the peer
    /// latency check. A game whose own relay answered while the lobby never
    /// took its `relay_ready` waited on the lobby.
    pub(super) fn expired_admission(&self, now: Instant) -> Option<&'static str> {
        let pending = self
            .pending
            .as_ref()
            .filter(|pending| now >= pending.deadline)?;
        Some(if pending.probes_started {
            "VERA:StartLatencyCheckFailed"
        } else if pending.relay_acknowledged && !pending.relay_reported {
            "VERA:StartLobbyBusy"
        } else {
            relay_failure_line(pending)
        })
    }

    /// The line for a `start_rejected` reason. The lobby sends its reasons in
    /// English only (vera20k-services `lobby/service.js`: `leave`, `finish`
    /// and the `reap` sweep, through `abortPending`); one this game does not
    /// know, or a relay failure with no start of this game's pending, keeps
    /// the general line.
    pub(super) fn start_rejected_line(&self, reason: &str) -> &'static str {
        match reason {
            "A player left before the game started." => "VERA:StartPlayerLeft",
            "A player cancelled the game start." => "VERA:StartPlayerCancelled",
            "A player could not complete the peer latency check." => "VERA:StartLatencyCheckFailed",
            "A player could not connect to the gameplay relay." => self
                .pending
                .as_ref()
                .map_or("VERA:StartRefusedConnection", relay_failure_line),
            _ => "VERA:StartRefusedConnection",
        }
    }

    pub(super) fn poll_cleanup(&mut self, now: Instant) {
        let finish = self
            .finishing
            .as_mut()
            .and_then(|finish| finish.take_due(now));
        let leave = self.leaving.as_mut().and_then(|leave| leave.take_due(now));
        // The logout waits for the finish and the leave.
        let logout = if self.finishing.is_none() && self.leaving.is_none() {
            self.logging_out
                .as_mut()
                .and_then(|logout| logout.take_due(now))
        } else {
            None
        };
        for (id, operation, payload) in [finish, leave, logout].into_iter().flatten() {
            self.send_command(id, operation, payload);
        }
    }
}

/// `deadline` on the low 32 bits of frame_pacer::wall_clock_ms has come by
/// `now_ms`. Windows' timeGetTime wraps this word every 49.7 days.
fn deadline_reached(now_ms: u32, deadline: u32) -> bool {
    now_ms.wrapping_sub(deadline) as i32 >= 0
}
