//! Production VERA lobby adapter. HTTPS results enter the existing WOL reducers;
//! MatchSession remains the sole authority for accepted participants and options.
//! The custom protocol is documented in docs/lobby-protocol.md.

mod http;
mod maps;
mod wire;

use crate::util::time::{Duration, Instant};
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
use anyhow::Context;
use anyhow::{Result, anyhow, ensure};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    WolApiStatus, WolLoginAccepted, WolProvider, WolServer, WolServerList, WolServiceEvent,
    WolServiceRequest, service_event,
};
use crate::app::AppState;
use crate::app::shell_network_setup::{NetworkSetupEvent, NetworkSetupRequest};
use crate::net::reliable::{LanConnectionConfig, LanConnections, RetryTiming};
use crate::net::udp::{LanUdpTransport, UdpBindConfig};
use crate::ui::wol_chat::WolChatLine;

// The deployment sets this at build time; a runtime override supports staging.
const DEFAULT_LOBBY_URL: &str = match option_env!("VERA20K_LOBBY_URL") {
    Some(value) => value,
    None => "https://lobby.vera20k.org",
};
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const RELAY_TIMEOUT: Duration = Duration::from_secs(30);
const EXIT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(90);
/// Gameplay grants (timing confirmation, `relay_renewed`, match resume) are
/// issued for 300 seconds (docs/lobby-protocol.md). Their
/// `expires` is lobby wall time; a local clock running slow would renew after
/// the relay already rejects the ticket, so renewal is timed from receipt.
/// It uses frame_pacer::wall_clock_ms, which keeps counting through sleep;
/// an `Instant` deadline would wait out the slept time with a dead ticket.
const RELAY_GRANT_LIFETIME: Duration = Duration::from_secs(300);
/// docs/lobby-protocol.md: renew at half-life. The relay refuses an expired ticket, so a
/// lobby outage longer than this margin freezes the match. Retrying every 10 s
/// spends 6 of renew_relay's 12 a minute (docs/lobby-protocol.md).
const RELAY_RENEWAL_MARGIN: Duration = Duration::from_secs(150);
const RELAY_RENEWAL_RETRY_MS: u32 = 10_000;

/// The departing visit's same HTTP/cleanup owner survives the menu teardown.
/// Native WOL_Main77B532 releases WOLAPI786080 after logout785EA0. VERA's
/// HTTPS retries are asynchronous: no retired callback may enter a new visit,
/// and a new worker waits for this single bounded owner to finish or expire.
#[derive(Default)]
pub(crate) struct OnlineCleanup {
    retired: Option<RetiredService>,
    // Derived receipt only: reset per retirement, set by that owner's exact
    // logout acknowledgement. It never controls retries or worker shutdown.
    logout_receipt: Option<LogoutReceipt>,
}

#[derive(Clone, Copy)]
struct LogoutReceipt {
    request_id: u64,
    failures: u8,
    acknowledged_uptime_ms: u64,
}

/// Nonsecret read-only facts from the existing retry/retirement owner. No
/// diagnostic may change its command identity, retry timing or stop boundary.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub(crate) struct LogoutObservation {
    pub pending: bool,
    pub request_id: Option<u64>,
    pub failures: u8,
    pub acknowledged: bool,
    pub acknowledged_uptime_ms: Option<u64>,
    pub worker_closed: bool,
}

struct RetiredService {
    service: LiveService,
    deadline: Instant,
}

impl OnlineCleanup {
    pub(crate) fn pending(&self) -> bool {
        self.retired.is_some()
    }

    pub(crate) fn logout_acknowledged(&self) -> bool {
        self.logout_receipt.is_some()
    }

    pub(crate) fn logout_observation(&self) -> LogoutObservation {
        let command = self
            .retired
            .as_ref()
            .and_then(|retired| retired.service.logging_out.as_ref());
        LogoutObservation {
            pending: self.pending(),
            request_id: command
                .map(|command| command.request_id)
                .or(self.logout_receipt.map(|receipt| receipt.request_id)),
            failures: command.map_or_else(
                || self.logout_receipt.map_or(0, |receipt| receipt.failures),
                |command| command.failures,
            ),
            acknowledged: self.logout_acknowledged(),
            acknowledged_uptime_ms: self
                .logout_receipt
                .map(|receipt| receipt.acknowledged_uptime_ms),
            worker_closed: self.logout_acknowledged() && !self.pending(),
        }
    }

    fn leave(&mut self, visit: &mut super::WolVisit, now: Instant) {
        if let Some(service) = visit.live_service.take() {
            self.retire(service, now);
        }
    }

    fn retire(&mut self, mut service: LiveService, now: Instant) {
        // New online workers are deferred while one retiring visit exists.
        assert!(self.retired.is_none(), "one online exit owner at a time");
        self.logout_receipt = None;
        service.auth_attempt = service.auth_attempt.wrapping_add(1);
        service.start_preparation = None;
        service.pending_choices = None;
        if let Some(pending) = service.pending.take() {
            service.match_id.get_or_insert(pending.match_id);
        }
        self.retired = Some(RetiredService {
            service,
            deadline: now + EXIT_CLEANUP_TIMEOUT,
        });
        let _ = self.poll(now);
    }

    fn poll(&mut self, now: Instant) -> Option<Instant> {
        let retired = self.retired.as_mut()?;
        // The worker already decoded/bounded its FIFO. Drain old responses only
        // into that owner's critical command state; never call a UI reducer.
        for _ in 0..256 {
            let response = match retired.service.responses.try_recv() {
                Ok(response) => response,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.retired = None;
                    return None;
                }
            };
            if retired.service.stopping.load(Ordering::Acquire) {
                continue;
            }
            match response {
                http::Response::CommandCompleted { id, operation } => match operation {
                    "finish" => retired.service.finish_completed(id),
                    "leave" => retired.service.leave_completed(id),
                    "logout" => {
                        let receipt = retired
                            .service
                            .logging_out
                            .as_ref()
                            .filter(|logout| logout.request_id == id)
                            .map(|logout| LogoutReceipt {
                                request_id: id,
                                failures: logout.failures,
                                acknowledged_uptime_ms:
                                    crate::app::match_runtime::frame_pacer::wall_clock_ms(),
                            });
                        retired.service.logout_completed(id);
                        if let Some(receipt) = receipt {
                            self.logout_receipt = Some(receipt);
                            retired.service.stopping.store(true, Ordering::Release);
                        }
                    }
                    _ => {}
                },
                http::Response::CommandFailed {
                    id,
                    operation,
                    failure,
                } => match operation {
                    "finish" => retired.service.finish_failed(id, failure, now),
                    "leave" => retired.service.leave_failed(id, now),
                    "logout" => retired.service.logout_failed(id, now),
                    _ => {}
                },
                http::Response::Event(event) if event.kind == "game_started" => {
                    // A start can commit just before Back, while its callback is
                    // still in this FIFO. Finish that actual identity first.
                    if retired.service.match_id.is_none()
                        && let Some(id) = event.payload["match_id"].as_str()
                        && !id.is_empty()
                        && id.len() <= 128
                    {
                        retired.service.match_id = Some(id.to_owned());
                    }
                }
                http::Response::Offline => {
                    retired.service.clear_cleanup();
                    retired.service.stopping.store(true, Ordering::Release);
                }
                _ => {}
            }
        }
        if retired.service.stopping.load(Ordering::Acquire) {
            return Some(now + POLL_INTERVAL);
        }
        if now >= retired.deadline {
            // Stop before another HTTP request. Retain this owner until its
            // response channel closes, so a subsequent visit cannot race an
            // old worker's in-flight cleanup. Each HTTP call is bounded to 12 s.
            retired.service.stopping.store(true, Ordering::Release);
            return Some(now + POLL_INTERVAL);
        }
        if retired.service.logging_out.is_none()
            && retired.service.match_id.is_none()
            && retired.service.finishing.is_none()
            && retired.service.leaving.is_none()
        {
            // A completed logout returns above; initial retirement still
            // needs to issue it after any finish/leave acknowledgement.
            retired.service.begin_logout(now);
        }
        retired.service.begin_finish(now);
        retired.service.poll_cleanup(now);
        Some(now + POLL_INTERVAL)
    }
}

pub(super) struct LiveService {
    #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
    _browser_requests: http::RequestOwner,
    stopping: Arc<AtomicBool>,
    requests: Sender<http::Request>,
    responses: Receiver<http::Response>,
    next_request: u64,
    auth_attempt: u64,
    revision: u64,
    // Canonical received options only; MatchSession remains the editable owner.
    accepted_options: Option<Value>,
    start_preparation: Option<StartPreparation>,
    last_ready_request: Option<(u64, u64)>,
    pending_choices: Option<PendingChoices>,
    player_id: String,
    member_ids: BTreeMap<String, String>,
    pending: Option<PendingStart>,
    match_id: Option<String>,
    finishing: Option<RetryingCommand>,
    leaving: Option<RetryingCommand>,
    logging_out: Option<RetryingCommand>,
    /// Renewal deadlines on the low 32 bits of frame_pacer::wall_clock_ms.
    next_renewal_attempt: Option<u32>,
    /// Renewal deadline of the newest gameplay grant received.
    relay_renewal_due: Option<u32>,
    maps: maps::Transfers,
    translations: Translations,
    /// A match an external launcher formed (docs/spawn.md): adopted bearer,
    /// no lobby pages, and `finish` carries the local result.
    spawned: bool,
    /// The spawned match's `finish` result (`ExternalMatchResult`).
    finish_result: Option<&'static str>,
    /// docs/spawn.md: every opponent left before the local player did.
    finish_opponents_left: bool,
    /// Each admission barrier's client deadline; `[Relay] AdmissionTimeout`
    /// for a spawned match.
    relay_timeout: Duration,
    /// The sign-in (`auth_attempt`) that already showed its lobby-upgrade notice.
    outdated_notice: Option<u64>,
}

/// Where players download a newer build (README "Download and play").
pub(crate) const RELEASES_URL: &str = "https://github.com/YuriPlanet/vera20k/releases";

/// The lobby's own 426 reason when it gave a plain one, then where to get a
/// newer build.
fn update_required_text(message: Option<&str>) -> String {
    let reason = message.unwrap_or("Update VERA20k to use secure online multiplayer.");
    format!("{reason} {RELEASES_URL}")
}

/// The external launch's online visit: once its match ends or admission
/// fails, the result/leave cleanup runs and then the process quits.
#[derive(Debug, Default)]
pub(super) struct SpawnedVisit {
    exit_deadline: Option<Instant>,
    cleanup_begun: bool,
    quit: bool,
}

impl Drop for LiveService {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
    }
}

impl std::fmt::Debug for LiveService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveService")
            .field("revision", &self.revision)
            .field("pending_start", &self.pending.is_some())
            .finish_non_exhaustive()
    }
}

/// The match identity remains authoritative until this idempotent command is
/// acknowledged. Only retry timing/request correlation is stored here.
struct RetryingCommand {
    request_id: u64,
    next_attempt: Instant,
    failures: u8,
}

impl RetryingCommand {
    fn new(request_id: u64, now: Instant) -> Self {
        Self {
            request_id,
            next_attempt: now + RELAY_TIMEOUT,
            failures: 0,
        }
    }

    fn failed(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1);
        self.next_attempt = now + Duration::from_secs((1u64 << self.failures.min(5)).min(30));
    }

    fn take_due(&mut self, now: Instant) -> Option<u64> {
        if now < self.next_attempt {
            return None;
        }
        self.next_attempt = now + RELAY_TIMEOUT;
        Some(self.request_id)
    }
}

/// The expected echo of one outgoing local choice, retained across earlier
/// full-options updates until the host accepts it or its request fails.
struct PendingChoices {
    request_id: u64,
    expected_human: Value,
    deadline: Instant,
}

/// One outgoing Start transaction. Its frozen request is not editable setup
/// state: a changed publication cancels it, and every acknowledgement is tied
/// to its committed server revision before the request can leave this owner.
struct StartPreparation {
    users: Vec<String>,
    options: Value,
    revision: Option<u64>,
    publication_request: Option<u64>,
    submitted_request: Option<u64>,
    deadline: Instant,
}

impl StartPreparation {
    fn accept_options(&mut self, revision: u64, options: &Value) -> bool {
        if &self.options != options {
            return false;
        }
        self.revision = Some(revision);
        true
    }

    fn ready(
        &self,
        session: &crate::app::match_runtime::session::MatchSession,
        revision: u64,
    ) -> bool {
        self.revision == Some(revision)
            && session.network_setup().is_some_and(|launch| {
                launch
                    .humans
                    .iter()
                    .map(|human| &human.name)
                    .eq(self.users.iter())
                    && (0..launch.humans.len()).all(|index| {
                        session.network_player_ready(index)
                            && session.network_map_status(index) == Some(1)
                    })
            })
    }
}

// A busy room cannot queue minutes of model calls: a line that finds this
// many waiting stays untranslated. An unavailable translator pauses them.
const MAX_PENDING_TRANSLATIONS: usize = 4;
const TRANSLATION_PAUSE: Duration = Duration::from_secs(60);

/// Received chat lines waiting for `POST /v1/translate`, by request id.
#[derive(Default)]
struct Translations {
    next: u64,
    pending: BTreeMap<u64, super::translation::TranslatedLine>,
    paused_until: Option<Instant>,
}

impl Translations {
    fn request(
        &mut self,
        requests: &Sender<http::Request>,
        text: &[u16],
        target: crate::ui::chat_translation::ChatLanguage,
        line: super::translation::TranslatedLine,
        now: Instant,
    ) {
        let text = String::from_utf16_lossy(text);
        if self.pending.len() >= MAX_PENDING_TRANSLATIONS
            || self.paused_until.is_some_and(|until| now < until)
            || text.len() > 2048
        {
            return;
        }
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        let target = target.tag();
        if requests
            .send(http::Request::Translate { id, text, target })
            .is_ok()
        {
            self.pending.insert(id, line);
        }
    }

    fn reply(
        &mut self,
        id: u64,
        outcome: Result<String, http::TranslateFailure>,
        now: Instant,
    ) -> Option<(super::translation::TranslatedLine, String)> {
        let line = self.pending.remove(&id)?;
        match outcome {
            Ok(text) => Some((line, text)),
            Err(failure) => {
                if failure == http::TranslateFailure::Unavailable {
                    self.paused_until = Some(now + TRANSLATION_PAUSE);
                }
                None
            }
        }
    }
}

struct PendingStart {
    match_id: String,
    host_name: String,
    transport: Option<LanUdpTransport>,
    deadline: Instant,
    relay_acknowledged: bool,
    probes_started: bool,
    latencies_reported: bool,
    /// The unanswered `relay_ready` or `latency_ready`.
    report: Option<PendingReport>,
}

/// One admission report, resent under its request ID within the start window
/// when the lobby answers 429, 5xx or nothing; the lobby replays a committed
/// request ID (docs/lobby-protocol.md `POST /v1/commands`).
struct PendingReport {
    command: RetryingCommand,
    operation: &'static str,
    payload: Value,
}

impl LiveService {
    fn new() -> Result<Self> {
        Self::open(None)
    }

    #[cfg(all(
        feature = "wasm",
        not(feature = "wasm-net"),
        target_arch = "wasm32",
        target_os = "unknown"
    ))]
    fn open(_service_url: Option<&str>) -> Result<Self> {
        anyhow::bail!("Online play is unavailable in the single-player browser build")
    }

    /// `service_url` replaces the configured lobby with an external launch's
    /// already validated match API origin (HTTPS, or loopback HTTP).
    #[cfg(any(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        feature = "wasm-net"
    ))]
    fn open(service_url: Option<&str>) -> Result<Self> {
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let base = match service_url {
            Some(url) => url.to_owned(),
            None => {
                let base = std::env::var("VERA20K_LOBBY_URL")
                    .unwrap_or_else(|_| DEFAULT_LOBBY_URL.to_owned());
                ensure!(
                    base.starts_with("https://")
                        && !base[8..].chars().any(|ch| matches!(ch, '@' | '?' | '#')),
                    "the online lobby needs its configured HTTPS service"
                );
                base
            }
        };
        let (requests, input) = mpsc::channel();
        let (output, responses) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = Arc::clone(&stopping);
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        std::thread::Builder::new()
            .name("vera-lobby".into())
            .spawn(move || {
                http::run(
                    base.trim_end_matches('/').to_owned(),
                    input,
                    output,
                    worker_stopping,
                )
            })
            .context("could not start online lobby worker")?;
        #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
        let browser_requests = http::start_browser(input, output, worker_stopping, service_url)?;
        Ok(Self {
            #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
            _browser_requests: browser_requests,
            stopping,
            requests,
            responses,
            next_request: 1,
            auth_attempt: 0,
            revision: 0,
            accepted_options: None,
            start_preparation: None,
            last_ready_request: None,
            pending_choices: None,
            player_id: String::new(),
            member_ids: BTreeMap::new(),
            pending: None,
            match_id: None,
            finishing: None,
            leaving: None,
            logging_out: None,
            next_renewal_attempt: None,
            relay_renewal_due: None,
            maps: Default::default(),
            translations: Translations::default(),
            spawned: false,
            finish_result: None,
            finish_opponents_left: false,
            relay_timeout: RELAY_TIMEOUT,
            outdated_notice: None,
        })
    }

    /// The first lobby-upgrade refusal of this sign-in, as one notice; the
    /// sign-in's later refused requests add none.
    fn outdated_notice(&mut self, message: Option<&str>) -> Option<String> {
        (self.outdated_notice != Some(self.auth_attempt)).then(|| {
            self.outdated_notice = Some(self.auth_attempt);
            update_required_text(message)
        })
    }

    fn command(&mut self, operation: &'static str, payload: Value) {
        self.command_with_id(operation, payload);
    }

    fn publish_options_agreement(&mut self, mut agreement: Value, status: u8, host: bool) {
        // Protocol v4 shares the agreement fields, but ready rejects status.
        let mut map_status = agreement.clone();
        map_status["status"] = json!(status);
        self.command("map_status", map_status);
        if host {
            agreement["ready"] = json!(true);
            self.command("ready", agreement);
        }
    }

    fn command_with_id(&mut self, operation: &'static str, payload: Value) -> u64 {
        let id = self.next_request;
        self.next_request = self.next_request.wrapping_add(1);
        self.send_command(id, operation, payload);
        id
    }

    fn send_command(&self, id: u64, operation: &'static str, payload: Value) {
        let _ = self.requests.send(http::Request::Command {
            id,
            operation,
            payload,
        });
    }

    fn begin_finish(&mut self, now: Instant) {
        let Some(match_id) = self.match_id.clone() else {
            return;
        };
        if self.finishing.is_some() {
            return;
        }
        self.next_renewal_attempt = None;
        self.relay_renewal_due = None;
        self.finishing = Some(RetryingCommand::new(self.next_request, now));
        let payload = self.finish_payload(&match_id);
        self.command("finish", payload);
    }

    /// The lobby's `finish` is exactly `{match_id}`; a spawned match adds
    /// its local result for the launcher (docs/spawn.md).
    fn finish_payload(&self, match_id: &str) -> Value {
        let mut payload = json!({"match_id":match_id});
        if self.spawned {
            payload["result"] = json!(self.finish_result.unwrap_or("quit"));
            payload["opponents_left"] = json!(self.finish_opponents_left);
        }
        payload
    }

    fn finish_completed(&mut self, id: u64) {
        if self
            .finishing
            .as_ref()
            .is_some_and(|finish| finish.request_id == id)
        {
            self.finishing = None;
            self.match_id = None;
        }
    }

    fn finish_failed(&mut self, id: u64, failure: http::CommandFailure, now: Instant) {
        if failure == http::CommandFailure::AlreadyFinished {
            self.finish_completed(id);
            return;
        }
        if let Some(finish) = self
            .finishing
            .as_mut()
            .filter(|finish| finish.request_id == id)
        {
            finish.failed(now);
        }
    }

    fn begin_leave(&mut self, now: Instant) {
        if self.leaving.is_none() {
            self.leaving = Some(RetryingCommand::new(self.next_request, now));
            self.command("leave", json!({}));
        }
    }

    fn leave_completed(&mut self, id: u64) {
        if self
            .leaving
            .as_ref()
            .is_some_and(|leave| leave.request_id == id)
        {
            self.leaving = None;
        }
    }

    fn leave_failed(&mut self, id: u64, now: Instant) {
        if let Some(leave) = self.leaving.as_mut().filter(|leave| leave.request_id == id) {
            leave.failed(now);
        }
    }

    fn begin_logout(&mut self, now: Instant) {
        if self.logging_out.is_none() {
            self.logging_out = Some(RetryingCommand::new(self.next_request, now));
            self.command("logout", json!({}));
        }
    }

    fn logout_completed(&mut self, id: u64) {
        if self
            .logging_out
            .as_ref()
            .is_some_and(|logout| logout.request_id == id)
        {
            self.clear_cleanup();
        }
    }

    fn logout_failed(&mut self, id: u64, now: Instant) {
        if let Some(logout) = self
            .logging_out
            .as_mut()
            .filter(|logout| logout.request_id == id)
        {
            logout.failed(now);
        }
    }

    fn clear_cleanup(&mut self) {
        self.finishing = None;
        self.leaving = None;
        self.logging_out = None;
        self.match_id = None;
        self.next_renewal_attempt = None;
        self.relay_renewal_due = None;
    }

    /// A gameplay grant reached its UDP owner at `now_ms`.
    fn relay_granted(&mut self, now_ms: u32) {
        let renew_after = (RELAY_GRANT_LIFETIME - RELAY_RENEWAL_MARGIN).as_millis() as u32;
        self.relay_renewal_due = Some(now_ms.wrapping_add(renew_after));
        self.next_renewal_attempt = None;
    }

    fn poll_renewal(&mut self, now_ms: u32) {
        if self.finishing.is_some() {
            return;
        }
        let (Some(match_id), Some(due)) = (self.match_id.as_ref(), self.relay_renewal_due) else {
            return;
        };
        // Short-lived grants renew before expiry. Retry a missing reply without
        // replacing the live route or resetting the event cursor.
        // Windows' timeGetTime wraps this 32-bit word every 49.7 days.
        let reached = |deadline: u32| now_ms.wrapping_sub(deadline) as i32 >= 0;
        if !reached(due) || self.next_renewal_attempt.is_some_and(|next| !reached(next)) {
            return;
        }
        let match_id = match_id.clone();
        self.next_renewal_attempt = Some(now_ms.wrapping_add(RELAY_RENEWAL_RETRY_MS));
        self.command("renew_relay", json!({"match_id":match_id}));
    }

    fn send_report(&mut self, operation: &'static str, payload: Value, now: Instant) {
        let id = self.command_with_id(operation, payload.clone());
        if let Some(pending) = self.pending.as_mut() {
            pending.report = Some(PendingReport {
                command: RetryingCommand::new(id, now),
                operation,
                payload,
            });
        }
    }

    fn report_completed(&mut self, id: u64) {
        if let Some(pending) = self.pending.as_mut()
            && pending
                .report
                .as_ref()
                .is_some_and(|report| report.command.request_id == id)
        {
            pending.report = None;
        }
    }

    /// Whether the refused report will be resent: only a busy or unreachable
    /// lobby is asked again, any other refusal stands.
    fn report_failed(&mut self, id: u64, failure: http::CommandFailure, now: Instant) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        let Some(report) = pending
            .report
            .as_mut()
            .filter(|report| report.command.request_id == id)
        else {
            return false;
        };
        if failure == http::CommandFailure::Retryable {
            report.command.failed(now);
            return true;
        }
        pending.report = None;
        false
    }

    fn poll_report(&mut self, now: Instant) {
        if let Some((id, operation, payload)) = self
            .pending
            .as_mut()
            .and_then(|pending| pending.report.as_mut())
            .and_then(|report| {
                Some((
                    report.command.take_due(now)?,
                    report.operation,
                    report.payload.clone(),
                ))
            })
        {
            self.send_command(id, operation, payload);
        }
    }

    fn admission_blocked(&self) -> bool {
        self.finishing.is_some() || self.leaving.is_some() || self.logging_out.is_some()
    }

    fn poll_cleanup(&mut self, now: Instant) {
        if let Some(id) = self
            .finishing
            .as_mut()
            .and_then(|finish| finish.take_due(now))
            && let Some(match_id) = self.match_id.as_ref()
        {
            self.send_command(id, "finish", self.finish_payload(match_id));
        }
        if let Some(id) = self.leaving.as_mut().and_then(|leave| leave.take_due(now)) {
            self.send_command(id, "leave", json!({}));
        }
        if self.finishing.is_none()
            && self.leaving.is_none()
            && let Some(id) = self
                .logging_out
                .as_mut()
                .and_then(|logout| logout.take_due(now))
        {
            self.send_command(id, "logout", json!({}));
        }
    }
}

fn take_service(state: &mut AppState) -> Option<LiveService> {
    state.frontend.wol.as_mut()?.live_service.take()
}

fn put_service(state: &mut AppState, service: LiveService) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.live_service = Some(service);
    } else {
        // A service callback can end its visit while this same owner is
        // temporarily borrowed by the reducer. Preserve its cleanup too.
        state
            .frontend
            .online_cleanup
            .retire(service, Instant::now());
    }
}

/// The authenticated match identity retained by the existing live service.
pub(crate) fn online_match_id(state: &AppState) -> Option<&str> {
    state
        .frontend
        .wol
        .as_ref()?
        .live_service
        .as_ref()?
        .match_id
        .as_deref()
}

/// Both existing cleanup owners must finish before another online admission.
/// HTTP logout may complete while the same socket still retries its explicit
/// relay departure; this observation owns no retry or membership state.
pub(crate) fn online_cleanup_pending(state: &AppState) -> bool {
    state.frontend.online_cleanup.pending()
        || crate::app::lan_service::online_retirement_pending(state)
}

/// Admission facts only; used by the opt-in exit fault observer before it
/// clicks the ordinary Back button. All cleanup state remains in LiveService.
pub(crate) fn online_exit_ready(state: &AppState) -> bool {
    state.frontend.wol.as_ref().is_some_and(|visit| {
        visit.provider == WolProvider::Online
            && visit.pending_service_request_count() == 0
            && visit.live_service.as_ref().is_some_and(|service| {
                !service.player_id.is_empty()
                    && service.match_id.is_none()
                    && service.finishing.is_none()
                    && service.leaving.is_none()
                    && service.logging_out.is_none()
                    && service.pending.is_none()
                    && service.start_preparation.is_none()
            })
    }) && !online_cleanup_pending(state)
}

pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    crate::app::lan_service::poll_online_retirement(state);
    let relay_pending = crate::app::lan_service::online_retirement_pending(state);
    let cleanup_deadline = state.frontend.online_cleanup.poll(Instant::now());
    if state
        .frontend
        .wol
        .as_ref()
        .is_none_or(|visit| visit.provider != WolProvider::Online)
    {
        return cleanup_deadline.or_else(|| relay_pending.then(|| Instant::now() + POLL_INTERVAL));
    }
    if cleanup_deadline.is_some() {
        // Welcome/menu navigation remains responsive. Keep this visit's
        // existing service requests until the old bearer cleanup is retired.
        return cleanup_deadline;
    }
    if state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.live_service.is_none())
    {
        if relay_pending {
            return Some(Instant::now() + POLL_INTERVAL);
        }
        match LiveService::new() {
            Ok(service) => {
                state
                    .frontend
                    .wol
                    .as_mut()
                    .expect("checked visit")
                    .live_service = Some(service)
            }
            Err(_) => {
                // Only answer once the user enters the online flow.
                let requests = state
                    .frontend
                    .wol
                    .as_mut()
                    .expect("checked visit")
                    .take_service_requests();
                if requests
                    .iter()
                    .any(|request| matches!(request, WolServiceRequest::EnsureApi))
                {
                    service_event(state, WolServiceEvent::Api(WolApiStatus::Missing));
                }
                return Some(Instant::now() + Duration::from_secs(1));
            }
        }
    }
    let mut service = take_service(state)?;
    if state.frontend.shell_exit.is_none() {
        while state.frontend.shell_exit.is_none() && state.frontend.wol.is_some() {
            let Ok(response) = service.responses.try_recv() else {
                break;
            };
            apply_response(state, &mut service, response);
        }
        if state.frontend.shell_exit.is_some() || state.frontend.wol.is_none() {
            // Created/Joined may begin a slide during the drain. Native slides
            // pump no callbacks: leave the following FIFO messages untouched
            // until the transition has admitted the next setup owner.
            put_service(state, service);
            return Some(Instant::now() + POLL_INTERVAL);
        }
        let requests = state
            .frontend
            .wol
            .as_mut()
            .map(|visit| visit.take_service_requests())
            .unwrap_or_default();
        for request in requests {
            if (service.admission_blocked() || relay_pending)
                && matches!(
                    request,
                    WolServiceRequest::JoinChannel { .. }
                        | WolServiceRequest::CreateChannel { .. }
                        | WolServiceRequest::Connect { .. }
                )
            {
                // Keep the existing UI request queued until acknowledged
                // cleanup has drained old room events. Never bind a new attempt
                // while an old success could still arrive.
                if let Some(visit) = state.frontend.wol.as_mut() {
                    visit.requests.push(request);
                }
            } else {
                request_service(state, &mut service, request);
            }
        }
        service.poll_cleanup(Instant::now());
        if state.match_state.session_mode() == crate::app::match_runtime::session::SessionMode::Wol
        {
            let requests = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .map(|protocol| protocol.take_service_requests())
                .unwrap_or_default();
            let prepares_start = requests
                .iter()
                .any(|request| matches!(request, NetworkSetupRequest::StartGame { .. }));
            for request in requests {
                // Start publishes one immutable final snapshot. Sending both
                // native roster/options observations separately would create
                // an agreement revision between publication and StartGame.
                if prepares_start
                    && matches!(
                        request,
                        NetworkSetupRequest::PublishOptions | NetworkSetupRequest::PublishRoster
                    )
                {
                    continue;
                }
                let starts = matches!(request, NetworkSetupRequest::StartGame { .. });
                if request_setup(state, &mut service, request).is_err() {
                    if starts {
                        service.start_preparation = None;
                        fail_start(
                            state,
                            "The lobby request does not match the current game setup.",
                        );
                    } else {
                        notice(
                            state,
                            "The lobby request does not match the current game setup.",
                        );
                    }
                }
            }
        }
        if service
            .pending_choices
            .as_ref()
            .is_some_and(|choices| Instant::now() >= choices.deadline)
        {
            service.pending_choices = None;
            restore_accepted_options(state, &service);
            notice(
                state,
                "The host did not acknowledge the player choices. Review the settings and try again.",
            );
        }
        if poll_start_preparation(state, &mut service).is_err() {
            service.start_preparation = None;
            fail_start(
                state,
                "The final game settings could not be agreed. Try Start again.",
            );
        }
        if poll_relay(state, &mut service).is_err() {
            cancel_pending(state, &mut service);
            fail_start(state, "Could not establish the gameplay relay connection.");
        }
        if !service.spawned && service.pending.is_none() && service.match_id.is_none() {
            maps::poll(state, &mut service);
        }
        if crate::app::lan_service::has_online_relay_route(state) {
            service.poll_renewal(crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32);
        }
        if service.spawned {
            poll_spawned_exit(state, &mut service);
        }
    }
    put_service(state, service);
    Some(Instant::now() + POLL_INTERVAL)
}

fn notice(state: &mut AppState, text: &str) {
    crate::app::shell_lan::append_chat(
        state,
        crate::ui::lan_shell::LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
    state.platform.window.request_redraw();
}

fn fail_start(state: &mut AppState, text: &str) {
    if crate::app::loading::pump::cancel_network_loading(state, text) {
        return;
    }
    if super::spawned_visit(state) {
        // No room or setup to return to: the launch itself failed.
        crate::app::App::fail_external_launch(state, text);
        return;
    }
    notice(state, text);
    if crate::app::shell_network_setup::verify(state).is_some() {
        let _ = crate::app::shell_network_setup::service_event(
            state,
            NetworkSetupEvent::Negotiated { result: 1 },
        );
    } else if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        // A rejected HTTP Start has not handed the setup to the lobby runner.
        // Keep the ordinary host/join screen and release its pending input.
        protocol.set_online_start_phase(crate::app::shell_network_setup::OnlineStartPhase::Idle);
        state.platform.window.request_redraw();
    } else {
        crate::app::shell_wol::start_aborted(state);
    }
}

fn cancel_pending(state: &mut AppState, service: &mut LiveService) {
    service.start_preparation = None;
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.set_online_start_phase(crate::app::shell_network_setup::OnlineStartPhase::Idle);
    }
    if service.pending.take().is_some() {
        // A Ready relay may already have moved into the shared transport owner.
        // Retire either ownership location when this start is canceled.
        crate::app::lan_service::close(state);
    }
}

pub(super) fn match_returned(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.match_returned(Instant::now());
    }
}

impl super::WolVisit {
    fn match_returned(&mut self, now: Instant) {
        if self.provider != WolProvider::Online {
            return;
        }
        if let Some(service) = self.live_service.as_mut() {
            service.begin_finish(now);
        } else if !self.requests.contains(&WolServiceRequest::MatchReturned) {
            //poll holds the SAME service across HTTP callbacks. Loading abort
            //can reenter the lobby there; retain its semantic intent before
            //new join/create requests, without copying the match or retry ID.
            self.requests.insert(0, WolServiceRequest::MatchReturned);
        }
    }
}

pub(super) fn leave_visit(state: &mut AppState) {
    if let Some(mut service) = take_service(state) {
        maps::cancel(state, &mut service);
        put_service(state, service);
    }
    let frontend = &mut state.frontend;
    if let Some(visit) = frontend.wol.as_mut() {
        frontend.online_cleanup.leave(visit, Instant::now());
    }
    crate::app::lan_service::close(state);
    state.frontend.wol_account.password.clear();
}

fn request_service(state: &mut AppState, service: &mut LiveService, request: WolServiceRequest) {
    match request {
        WolServiceRequest::EnsureApi => {
            service_event(state, WolServiceEvent::Api(WolApiStatus::Available))
        }
        WolServiceRequest::ListCredentials => {
            service_event(state, WolServiceEvent::Credentials(Vec::new()))
        }
        WolServiceRequest::FetchServers { .. } => {
            service_event(
                state,
                WolServiceEvent::ServerList(Ok(WolServerList {
                    servers: vec![
                        WolServer {
                            name: "VERA:".into(),
                        },
                        WolServer {
                            name: "VERA:Online".into(),
                        },
                    ],
                    selected: 1,
                })),
            );
            service_event(state, WolServiceEvent::NetStatus(0x0004_012F));
        }
        WolServiceRequest::Connect { nickname, .. } => {
            service.clear_cleanup();
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            let _ = service.requests.send(http::Request::OpenSession {
                attempt: service.auth_attempt,
                nickname,
            });
        }
        WolServiceRequest::ListChannels { kind, .. } => {
            let _ = service.requests.send(http::Request::Channels(kind));
        }
        WolServiceRequest::JoinChannel { channel, key } => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command("join", json!({"channel":channel.name,"key":key}));
            }
        }
        WolServiceRequest::CreateChannel { channel, key } => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command(
                    "create",
                    json!({"channel":wire::Channel::from(channel),"key":key}),
                );
            }
        }
        WolServiceRequest::LeaveChannel => {
            maps::cancel(state, service);
            cancel_pending(state, service);
            service.begin_leave(Instant::now());
        }
        WolServiceRequest::MatchReturned => service.begin_finish(Instant::now()),
        WolServiceRequest::Logout => {
            maps::cancel(state, service);
            cancel_pending(state, service);
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            service.begin_logout(Instant::now());
            crate::app::lan_service::close(state);
        }
        WolServiceRequest::Chat { line, to } => send_chat(service, line, to),
        WolServiceRequest::Emote { text, to } => service.command(
            "chat",
            json!({"text":String::from_utf16_lossy(&text),"to":to,"action":true}),
        ),
        WolServiceRequest::RequestLocales { names } => {
            service.command("locales", json!({"names":names}))
        }
        WolServiceRequest::SetLocale(locale) => {
            service.command("set_locale", json!({"locale":locale}))
        }
        WolServiceRequest::LocaleCount => service_event(state, WolServiceEvent::LocaleCount(1)),
        WolServiceRequest::ListBuddies => service.command("buddies", json!({})),
        WolServiceRequest::AddBuddy(name) => service.command("add_buddy", json!({"name":name})),
        WolServiceRequest::RemoveBuddy(name) => {
            service.command("remove_buddy", json!({"name":name}))
        }
        WolServiceRequest::Locate(name) => service.command("locate", json!({"name":name})),
        WolServiceRequest::SquadInfo { id } => service_event(
            state,
            WolServiceEvent::Squad {
                id,
                squad: Err(0x8000_4005),
            },
        ),
        WolServiceRequest::LadderRecords(_) => {
            service_event(state, WolServiceEvent::Ladder(Vec::new()))
        }
        WolServiceRequest::HighScores(_) => {
            service_event(state, WolServiceEvent::HighScores(Ok(Vec::new())))
        }
        WolServiceRequest::CancelWait => {
            maps::cancel(state, service);
            cancel_pending(state, service);
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            if let Some(visit) = state.frontend.wol.as_mut() {
                visit.authenticated = false;
                visit.connected = false;
                visit.requests.retain(|request| {
                    !matches!(
                        request,
                        WolServiceRequest::JoinChannel { .. }
                            | WolServiceRequest::CreateChannel { .. }
                    )
                });
            }
            service.begin_logout(Instant::now());
        }
        WolServiceRequest::StoreCredential { .. }
        | WolServiceRequest::ForgetCredential { .. }
        | WolServiceRequest::SetLangFilter(_)
        | WolServiceRequest::SetFindPage { .. } => {}
        WolServiceRequest::RequestMatch { .. } => {
            notice(state, "Choose Custom Match to host or join an online game.")
        }
        WolServiceRequest::Translate { text, line } => {
            if let Some(target) = state.persistence.options_profile.translate_chat {
                service.translations.request(
                    &service.requests,
                    &text,
                    target,
                    line,
                    Instant::now(),
                );
            }
        }
    }
}

fn send_chat(service: &mut LiveService, line: WolChatLine, mut to: Vec<String>) {
    let text = match line {
        WolChatLine::Say(text) | WolChatLine::Reply { text } => text,
        WolChatLine::Page { name, text } => {
            to = vec![String::from_utf16_lossy(&name)];
            text
        }
        WolChatLine::Locate { name } => {
            service.command("locate", json!({"name":String::from_utf16_lossy(&name)}));
            return;
        }
        WolChatLine::Notice(_) => return,
    };
    service.command(
        "chat",
        json!({"text":String::from_utf16_lossy(&text),"to":to,"action":false}),
    );
}

fn local_agreement(state: &mut AppState, revision: u64) -> Result<Value> {
    let setup = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow!("no setup"))?;
    let file = setup
        .selected_map_file
        .as_deref()
        .ok_or_else(|| anyhow!("no selected map"))?;
    let map_digest = state
        .match_state
        .session
        .network_local_map_source()
        .and_then(|source| {
            source
                .physical_identity()
                .map(|(_, digest)| digest.to_owned())
        })
        .or_else(|| {
            crate::app::shell_network_setup::local_map_metadata(state, file)
                .ok()
                .map(|map| map.source_sha256)
        })
        .unwrap_or_default();
    let effective_map_digest = state
        .match_state
        .session
        .network_local_map_effective_digest()
        .map(str::to_owned)
        .or_else(|| {
            crate::app::shell_network_setup::local_map_metadata(state, file)
                .ok()
                .map(|m| m.effective_digest)
        })
        // Status0 carries no proof; a fixed impossible digest keeps the wire
        // shape valid without claiming the host's unverified expanded content.
        .unwrap_or_else(|| "0".repeat(64));
    let staging_digest = crate::app::shell_network_setup::maps::staging_digest(state);
    let agreement = crate::app::shell_network_setup::agreement::local(state)?;
    Ok(
        json!({"revision":revision,"version":agreement.version,"rules_digest":agreement.rules_digest,"map_digest":map_digest,"effective_map_digest":effective_map_digest,"staging_digest":staging_digest}),
    )
}

fn local_map_status(state: &AppState, service: &LiveService) -> u8 {
    if !service.maps.current_verified(state) {
        return 0;
    }
    let Some(expected) = state.match_state.session.network_map_metadata() else {
        return 0;
    };
    let Some(file) = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.as_deref())
    else {
        return 0;
    };
    if file.eq_ignore_ascii_case("RandMap.Sed")
        && state
            .match_state
            .session
            .network_random_map_preview()
            .is_none()
    {
        return 0;
    }
    u8::from(
        crate::map::source::valid_source_sha256(&expected.source_sha256)
            && (state
                .match_state
                .session
                .network_local_map_source()
                .and_then(|source| source.physical_identity())
                == Some((
                    expected.byte_length as usize,
                    expected.source_sha256.as_str(),
                ))
                || crate::app::shell_network_setup::local_map_metadata(state, file).is_ok_and(
                    |actual| {
                        actual.source_sha256 == expected.source_sha256
                            && actual.byte_length == expected.byte_length
                            && actual.effective_digest == expected.effective_digest
                    },
                )),
    )
}

fn options(state: &AppState) -> Result<wire::Options> {
    let session = &state.match_state.session;
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no online setup"))?
        .clone();
    let file = launch
        .selected_map_file
        .as_deref()
        .ok_or_else(|| anyhow!("no selected map"))?;
    let metadata =
        crate::app::shell_network_setup::local_map_metadata(state, file).inspect_err(|_| {
            log::warn!("Online host map metadata could not be prepared");
        })?;
    Ok(wire::Options {
        launch,
        map_metadata: metadata.into(),
        random_map: crate::app::shell_network_setup::maps::host_generated_manifest(state)?,
        vacancies: std::array::from_fn(|row| {
            session
                .network_vacancy(row as u8)
                .unwrap_or(crate::app::match_runtime::session::NetworkVacancy::Closed)
                .into()
        }),
        observer_row: session.network_observer_row(),
        cooperative_difficulty: state
            .frontend
            .multiplayer_shell_runtime
            .cooperative_difficulty(),
        seed: Some(crate::app::shell_network_setup::resident_seed(state)),
    })
}

fn canonical_options(options: &wire::Options) -> Result<Value> {
    let mut value = serde_json::to_value(options)?;
    value["launch"]["local_player_index"] = json!(0);
    Ok(value)
}

fn local_choices_match(
    session: &crate::app::match_runtime::session::MatchSession,
    accepted: &Value,
) -> bool {
    let Some(local) = session.network_setup().map(|launch| launch.local_player()) else {
        return false;
    };
    let Ok(local) = serde_json::to_value(local) else {
        return false;
    };
    accepted_human_matches(accepted, &local)
}

fn accepted_human_matches(accepted: &Value, human: &Value) -> bool {
    accepted["launch"]["humans"]
        .as_array()
        .is_some_and(|humans| humans.iter().any(|candidate| candidate == human))
}

fn restore_accepted_options(state: &mut AppState, service: &LiveService) {
    let Some(accepted) = service.accepted_options.clone() else {
        return;
    };
    let result = decode::<wire::Options>(accepted)
        .and_then(|options| options.into_options(&state.frontend.wol_account.nickname))
        .and_then(|options| {
            crate::app::shell_network_setup::service_event(
                state,
                NetworkSetupEvent::HostSetup(options),
            )
        });
    if result.is_err() {
        notice(
            state,
            "The latest host settings are unavailable. Leave and rejoin the room.",
        );
    }
}

/// Optimistic country/color controls cannot accept until the host's complete
/// server options echo the same local row. Controlled/native providers keep
/// their existing admission behavior.
pub(crate) fn local_choices_agreed(state: &AppState) -> bool {
    let Some(visit) = state
        .frontend
        .wol
        .as_ref()
        .filter(|visit| visit.provider == WolProvider::Online)
    else {
        return true;
    };
    visit.live_service.as_ref().is_some_and(|service| {
        service.pending_choices.is_none()
            && service
                .accepted_options
                .as_ref()
                .is_some_and(|accepted| local_choices_match(&state.match_state.session, accepted))
    })
}

fn poll_start_preparation(state: &mut AppState, service: &mut LiveService) -> Result<()> {
    let Some(mut start) = service.start_preparation.take() else {
        return Ok(());
    };
    if Instant::now() >= start.deadline {
        fail_start(
            state,
            "Not every player's game was ready in time. Try Start again.",
        );
        return Ok(());
    }
    if start.submitted_request.is_some()
        || !start.ready(&state.match_state.session, service.revision)
        || local_map_status(state, service) != 1
    {
        service.start_preparation = Some(start);
        return Ok(());
    }
    ensure!(
        canonical_options(&options(state)?)? == start.options,
        "prepared options changed"
    );
    let mut payload = local_agreement(state, service.revision)?;
    payload["users"] = json!(&start.users);
    start.submitted_request = Some(service.command_with_id("start", payload));
    start.deadline = Instant::now() + RELAY_TIMEOUT;
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol
            .set_online_start_phase(crate::app::shell_network_setup::OnlineStartPhase::Submitted);
    }
    service.start_preparation = Some(start);
    Ok(())
}

fn request_setup(
    state: &mut AppState,
    service: &mut LiveService,
    request: NetworkSetupRequest,
) -> Result<()> {
    match request {
        NetworkSetupRequest::PublishOptions | NetworkSetupRequest::PublishRoster => {
            service.command("options", json!({"options":options(state)?}));
        }
        NetworkSetupRequest::PlayerChoices {
            row,
            slot,
            observer,
        } => {
            let mut human = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow!("no setup for player choices"))?
                .local_player()
                .clone();
            // Earlier full options can arrive before this queue drains. The
            // outgoing request, not that intervening UI echo, defines its ack.
            // A guest who starts or stops watching changed nothing here: the
            // host picks its row, or keeps it.
            let switch = human.is_observer() != observer;
            human.raw_slot_index = Some(row);
            human.slot = slot.clone();
            human.set_observer(observer);
            let expected_human = serde_json::to_value(human)?;
            let request_id = service.command_with_id(
                "choices",
                json!({"row":row,"slot":slot,"observer":observer}),
            );
            service.pending_choices = (!switch
                && !service
                    .accepted_options
                    .as_ref()
                    .is_some_and(|accepted| accepted_human_matches(accepted, &expected_human)))
            .then_some(PendingChoices {
                request_id,
                expected_human,
                deadline: Instant::now() + RELAY_TIMEOUT,
            });
        }
        NetworkSetupRequest::Ready(requested) => {
            let ready = requested && local_map_status(state, service) == 1;
            if requested && !ready {
                // The guest's automatic Accept waits for the verified map
                // instead of repeating this request every frame.
                crate::app::shell_network_setup::refuse_local_acceptance(state)?;
            } else if !ready {
                let index = state
                    .match_state
                    .session
                    .network_setup()
                    .ok_or_else(|| anyhow!("no setup"))?
                    .local_player_index;
                state
                    .match_state
                    .session
                    .set_network_player_ready(index, false)?;
            }
            let mut payload = local_agreement(state, service.revision)?;
            payload["ready"] = json!(ready);
            let id = service.command_with_id("ready", payload);
            service.last_ready_request = Some((id, service.revision));
        }
        NetworkSetupRequest::MapStatus(status) => {
            let mut payload = local_agreement(state, service.revision)?;
            // Native RandMap admission assumes the pending seed/image
            // receive. Online status1 requires both verified resources.
            payload["status"] = json!(status.min(local_map_status(state, service)));
            service.command("map_status", payload);
        }
        NetworkSetupRequest::StartGame { users } => {
            if service.start_preparation.is_none() {
                let snapshot = canonical_options(&options(state)?)?;
                let revision = (service.accepted_options.as_ref() == Some(&snapshot))
                    .then_some(service.revision);
                let publication_request = if revision.is_none() {
                    Some(service.command_with_id("options", json!({"options":snapshot})))
                } else {
                    None
                };
                service.start_preparation = Some(StartPreparation {
                    users,
                    options: snapshot,
                    revision,
                    publication_request,
                    submitted_request: None,
                    deadline: Instant::now() + Duration::from_secs(90),
                });
                if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
                    protocol.set_online_start_phase(
                        crate::app::shell_network_setup::OnlineStartPhase::Preparing,
                    );
                }
                notice(
                    state,
                    "Waiting for every player's game to confirm the final settings.",
                );
            }
        }
        NetworkSetupRequest::Leave => {
            maps::cancel(state, service);
            cancel_pending(state, service);
            service.begin_leave(Instant::now());
        }
        NetworkSetupRequest::Kick { name } => service.command("kick", json!({"name":name})),
        NetworkSetupRequest::Advertise => {
            if let Some(channel) = super::lobby::republish_channel(state) {
                service.command("advertise", json!({"channel":wire::Channel::from(channel)}));
            }
        }
        NetworkSetupRequest::RequestPings => service.command("ping", json!({})),
        NetworkSetupRequest::WolChat(line) => send_chat(service, line, Vec::new()),
        NetworkSetupRequest::WolEmote(text) => service.command(
            "chat",
            json!({"text":String::from_utf16_lossy(&text),"to":[],"action":true}),
        ),
        NetworkSetupRequest::Chat(text) => send_chat(service, WolChatLine::Say(text), Vec::new()),
        NetworkSetupRequest::PublishStartStaging(_) => {
            service.command("options", json!({"options":options(state)?}));
        }
        NetworkSetupRequest::RequestPreview
        | NetworkSetupRequest::OfferPreview
        | NetworkSetupRequest::AcknowledgePreview
        | NetworkSetupRequest::RequestPreviewFile
        | NetworkSetupRequest::SendPreviewFile
        | NetworkSetupRequest::RequestRandomMapImage => {}
        NetworkSetupRequest::RequestAcceptance
        | NetworkSetupRequest::BroadcastPings
        | NetworkSetupRequest::ObserverSlot { .. } => {}
        _ => notice(
            state,
            "This online setup operation is not available for the selected map.",
        ),
    }
    Ok(())
}

fn apply_response(state: &mut AppState, service: &mut LiveService, response: http::Response) {
    match response {
        response @ (http::Response::MapChunk { .. }
        | http::Response::MapUploadPrepared { .. }
        | http::Response::MapUploaded { .. }
        | http::Response::CachedMap { .. }
        | http::Response::MapInstalled { .. }
        | http::Response::MapFailure { .. }
        | http::Response::MapPending { .. }) => {
            maps::response(state, service, response);
        }
        http::Response::StartRejected { id, retryable } => {
            if !service
                .start_preparation
                .as_ref()
                .is_some_and(|start| start.submitted_request == Some(id))
            {
                return;
            }
            cancel_pending(state, service);
            fail_start(
                state,
                if retryable {
                    "The online lobby is busy or not responding. Try Start again."
                } else {
                    "The match could not start. Check every player's map, rules, version and readiness."
                },
            );
        }
        http::Response::Authenticated {
            attempt,
            player_id,
            nickname,
            motd,
        } if attempt == service.auth_attempt => {
            if validate_online_identity(&player_id, &nickname).is_err() {
                state.frontend.wol_account.password.clear();
                service_event(state, WolServiceEvent::Login(Err(0x8000_4005)));
                return;
            }
            service.player_id = player_id;
            state.frontend.wol_account.nickname = nickname;
            state.frontend.wol_account.password.clear();
            let motd = motd
                .filter(|motd| !motd.trim().is_empty())
                .unwrap_or_else(|| super::service::vera_motd(state));
            service_event(state, WolServiceEvent::Login(Ok(WolLoginAccepted { motd })));
        }
        http::Response::LoginFailed { attempt, code } if attempt == service.auth_attempt => {
            state.frontend.wol_account.password.clear();
            service_event(state, WolServiceEvent::Login(Err(code)));
        }
        http::Response::UpgradeRequired { attempt, message } if attempt == service.auth_attempt => {
            state.frontend.wol_account.password.clear();
            service_event(state, WolServiceEvent::Login(Err(0x8000_4005)));
            // The login has no lobby yet, so a chat line would be lost: the
            // failure's own box says why.
            let text = update_required_text(message.as_deref());
            if !super::service::replace_open_box_body(state, &text) {
                notice(state, &text);
            }
        }
        http::Response::Outdated { message } => {
            if let Some(text) = service.outdated_notice(message.as_deref()) {
                notice(state, &text);
            }
        }
        http::Response::Channels(list) => service_event(
            state,
            WolServiceEvent::Channels {
                kind: list.kind,
                channels: list.channels.into_iter().map(Into::into).collect(),
            },
        ),
        http::Response::Event(event) => {
            if apply_event(state, service, &event.kind, event.payload).is_err() {
                log::warn!(
                    "Online lobby update rejected: host_options={}",
                    event.kind == "options"
                );
                notice(state, "The server sent an invalid lobby update.");
            }
        }
        http::Response::CommandCompleted { id, operation } => match operation {
            "finish" => service.finish_completed(id),
            "leave" => service.leave_completed(id),
            "logout" => service.logout_completed(id),
            "relay_ready" | "latency_ready" => service.report_completed(id),
            _ => {}
        },
        http::Response::CommandFailed {
            id,
            operation,
            failure,
        } => match operation {
            "join" | "create" => {
                if super::lobby::online_request_pending(state, id) {
                    if failure == http::CommandFailure::Retryable {
                        // Both responses can be lost after a server-side commit.
                        // Acknowledge cleanup before binding another attempt.
                        service.begin_leave(Instant::now());
                    }
                    super::lobby::online_request_failed(state, id, failure.wol_code());
                }
            }
            "leave" => service.leave_failed(id, Instant::now()),
            "logout" => service.logout_failed(id, Instant::now()),
            "options"
                if service
                    .start_preparation
                    .as_ref()
                    .is_some_and(|start| start.publication_request == Some(id)) =>
            {
                service.start_preparation = None;
                fail_start(
                    state,
                    "The final settings could not be published. Try Start again.",
                );
            }
            "choices"
                if service
                    .pending_choices
                    .as_ref()
                    .is_some_and(|choices| choices.request_id == id) =>
            {
                service.pending_choices = None;
                restore_accepted_options(state, service);
                notice(
                    state,
                    "The host could not accept those player choices. Review the settings and try again.",
                );
            }
            "ready" if service.last_ready_request == Some((id, service.revision)) => {
                if failure != http::CommandFailure::Retryable {
                    // The lobby refused these settings, usually because the
                    // room has already moved on: the guest's automatic
                    // Accept waits for the next ones.
                    let _ = crate::app::shell_network_setup::refuse_local_acceptance(state);
                } else if let Some(index) = state
                    .match_state
                    .session
                    .network_setup()
                    .map(|launch| launch.local_player_index)
                {
                    // A lost request: the automatic Accept sends it again.
                    let _ = crate::app::shell_network_setup::service_event(
                        state,
                        NetworkSetupEvent::PlayerReady {
                            index,
                            accepted: false,
                        },
                    );
                }
            }
            "finish" => {
                let first_failure = service
                    .finishing
                    .as_ref()
                    .is_some_and(|finish| finish.request_id == id && finish.failures == 0);
                service.finish_failed(id, failure, Instant::now());
                if first_failure && failure != http::CommandFailure::AlreadyFinished {
                    notice(
                        state,
                        "Reconnecting to finish the previous match before rematch.",
                    );
                }
            }
            // The host's 5 s ping; an outage is reported once by the poll.
            "ping" => {}
            "relay_ready" | "latency_ready" => {
                if !service.report_failed(id, failure, Instant::now()) {
                    notice(state, "The online request was not accepted.");
                }
            }
            _ => notice(state, "The online request was not accepted."),
        },
        http::Response::Translated { id, outcome } => {
            if let Some((line, text)) = service.translations.reply(id, outcome, Instant::now()) {
                super::translation::show(state, line, &text);
            }
        }
        http::Response::Failure(message) => notice(state, message),
        http::Response::LobbyUnreachable => {
            notice(state, "The online lobby is not responding. Retrying.")
        }
        http::Response::LobbyReachable => notice(state, "The online lobby is responding again."),
        http::Response::Offline => {
            crate::app::loading::pump::cancel_network_loading(
                state,
                "The online session ended while the map was loading.",
            );
            maps::cancel(state, service);
            cancel_pending(state, service);
            // The bearer session is definitively gone; a new login has no
            // authority to retry cleanup for the old server session.
            service.clear_cleanup();
            if let Some(spawned) = spawned_visit_mut(state) {
                // Nothing is left to finish or leave with this bearer. A
                // running match continues on its relay; an admission ends.
                spawned.cleanup_begun = true;
                if !super::in_match(state) {
                    crate::app::App::fail_external_launch(
                        state,
                        "the match service ended this session",
                    );
                }
                return;
            }
            service_event(state, WolServiceEvent::NetStatus(super::NET_STATUS_LOST))
        }
        http::Response::Authenticated { .. }
        | http::Response::LoginFailed { .. }
        | http::Response::UpgradeRequired { .. } => {}
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| anyhow!("invalid lobby message"))
}

// Deliberately no Debug: the ticket and keys are relay credentials.
pub(crate) struct DecodedRelayGrant {
    pub(crate) ticket: Vec<u8>,
    pub(crate) key: [u8; 32],
    pub(crate) payload_key: [u8; 32],
    pub(crate) ordering_certificate: Vec<u8>,
    pub(crate) federation: Option<Vec<u8>>,
}

fn relay_uuid(value: &str) -> Result<[u8; 16]> {
    ensure!(
        value.len() == 36
            && [8, 13, 18, 23]
                .into_iter()
                .all(|i| value.as_bytes()[i] == b'-'),
        "invalid relay identity"
    );
    let compact = value.replace('-', "");
    ensure!(
        compact.len() == 32 && compact.is_ascii(),
        "invalid relay identity"
    );
    let mut bytes = [0; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&compact[index * 2..index * 2 + 2], 16)
            .map_err(|_| anyhow!("invalid relay identity"))?;
    }
    Ok(bytes)
}

fn validate_online_identity(player_id: &str, nickname: &str) -> Result<()> {
    crate::app::frontend::skirmish_session::validate_online_handle(nickname)?;
    relay_uuid(player_id)?;
    Ok(())
}

fn decode_relay_grant(
    grant: &wire::RelayGrant,
    match_id: &str,
    player_id: &str,
) -> Result<DecodedRelayGrant> {
    ensure!(
        grant.ticket.len() <= 160
            && grant.packet_key.len() == 43
            && grant.payload_key.len() == 43
            && grant.ordering_certificate.len() == 88,
        "invalid relay grant"
    );
    let ticket = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&grant.ticket)
        .map_err(|_| anyhow!("invalid relay grant"))?;
    let key = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&grant.packet_key)
        .map_err(|_| anyhow!("invalid relay grant"))?
        .try_into()
        .map_err(|_| anyhow!("invalid relay grant"))?;
    let payload_key = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&grant.payload_key)
        .map_err(|_| anyhow!("invalid relay grant"))?
        .try_into()
        .map_err(|_| anyhow!("invalid relay grant"))?;
    let parsed =
        crate::net::udp::RelayTicket::parse(&ticket).map_err(|_| anyhow!("invalid relay grant"))?;
    let ordering_certificate = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&grant.ordering_certificate)
        .map_err(|_| anyhow!("invalid gameplay clock"))?;
    let certificate = crate::net::udp::OrderingCertificate::parse(&ordering_certificate)
        .map_err(|_| anyhow!("invalid gameplay clock"))?;
    ensure!(
        parsed.source == grant.source_index
            && parsed.room == relay_uuid(match_id)?
            && parsed.player == relay_uuid(player_id)?
            && certificate.room == parsed.room
            && certificate.sources_mask & (1 << parsed.source) != 0,
        "relay grant identity differs"
    );
    let federation = grant
        .federation
        .as_ref()
        .map(|roster| {
            ensure!(roster.len() <= 1024, "invalid relay roster");
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(roster)
                .map_err(|_| anyhow!("invalid relay roster"))
        })
        .transpose()?;
    Ok(DecodedRelayGrant {
        ticket,
        key,
        payload_key,
        ordering_certificate,
        federation,
    })
}

/// A spawn.ini `[Relay]` section carries the same `game_started` grant fields
/// (docs/spawn.md); admit it through the lobby's own decoder.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_external_relay_grant(
    address: SocketAddrV4,
    ticket: String,
    packet_key: String,
    payload_key: String,
    ordering_certificate: String,
    federation: Option<String>,
    source_index: u8,
    match_id: &str,
    player_id: &str,
) -> Result<DecodedRelayGrant> {
    decode_relay_grant(
        &wire::RelayGrant {
            address,
            ticket,
            packet_key,
            payload_key,
            source_index,
            ordering_certificate,
            federation,
        },
        match_id,
        player_id,
    )
}

fn apply_event(
    state: &mut AppState,
    service: &mut LiveService,
    kind: &str,
    payload: Value,
) -> Result<()> {
    let local = state.frontend.wol_account.nickname.clone();
    match kind {
        "relay_renewed" => {
            #[derive(Deserialize)]
            struct Renewed {
                match_id: String,
                relay: wire::RelayGrant,
            }
            let renewed: Renewed = decode(payload)?;
            // A late reply from the previous match cannot replace a rematch.
            if service.match_id.as_ref() != Some(&renewed.match_id) {
                return Ok(());
            }
            let grant = decode_relay_grant(&renewed.relay, &renewed.match_id, &service.player_id)?;
            crate::app::lan_service::renew_online_relay(
                state,
                renewed.relay.address,
                &grant.ticket,
                grant.key,
                grant.payload_key,
            )?;
            service.relay_granted(crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32);
        }
        "joined" | "created" => {
            let joined: wire::Joined = decode(payload)?;
            service.revision = 0;
            service.accepted_options = None;
            service.last_ready_request = None;
            service.pending_choices = None;
            service.start_preparation = None;
            service.member_ids = joined
                .members
                .iter()
                .map(|m| (m.name.to_ascii_lowercase(), m.player_id.clone()))
                .collect();
            let members = joined
                .members
                .into_iter()
                .map(|member| member.into_member(&local))
                .collect();
            let event = if kind == "created" {
                WolServiceEvent::Created(Ok(members))
            } else {
                WolServiceEvent::Joined(Ok(super::lobby::WolJoined {
                    channel: joined.channel.into(),
                    members,
                }))
            };
            service_event(state, event);
        }
        "channel_left" => {
            let active = crate::app::loading::pump::network_loading_active(state);
            let taken = state
                .frontend
                .wol
                .as_ref()
                .is_some_and(|visit| visit.live_service.is_none());
            state.match_state.match_diagnostics.observe_loading_cancel(
                "deployed_channel_left",
                service.match_id.clone(),
                None,
                taken,
                active,
            );
            crate::app::loading::pump::cancel_network_loading(
                state,
                "The online room closed while the map was loading.",
            );
            service.accepted_options = None;
            service.last_ready_request = None;
            service.pending_choices = None;
            maps::cancel(state, service);
            cancel_pending(state, service);
            service_event(state, WolServiceEvent::ChannelLeft);
        }
        "member_joined" => {
            #[derive(Deserialize)]
            struct Payload {
                member: wire::Member,
            }
            let p: Payload = decode(payload)?;
            service.member_ids.insert(
                p.member.name.to_ascii_lowercase(),
                p.member.player_id.clone(),
            );
            service_event(
                state,
                WolServiceEvent::UserJoined(p.member.into_member(&local)),
            );
        }
        "member_left" => {
            let name = payload["name"]
                .as_str()
                .ok_or_else(|| anyhow!("missing member"))?
                .to_owned();
            service.member_ids.remove(&name.to_ascii_lowercase());
            let required = state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| {
                    launch.humans.iter().enumerate().any(|(source, human)| {
                        human.name.eq_ignore_ascii_case(&name)
                            && state
                                .match_state
                                .session
                                .network_connected_source_indices()
                                .contains(&source)
                    })
                });
            if required {
                crate::app::loading::pump::cancel_network_loading(
                    state,
                    "A player left the online room while the map was loading.",
                );
            }
            service_event(state, WolServiceEvent::UserLeft(name));
        }
        "options" => {
            let p: wire::OptionsEvent = decode(payload).inspect_err(|_| {
                log::warn!("Online host options could not be decoded");
            })?;
            if p.revision < service.revision {
                return Ok(());
            }
            let changed = p.revision != service.revision || service.accepted_options.is_none();
            let canonical = canonical_options(&p.options)?;
            if let Some(start) = service.start_preparation.as_mut()
                && !start.accept_options(p.revision, &canonical)
                && start.revision.is_some()
            {
                service.start_preparation = None;
                fail_start(
                    state,
                    "The game settings changed while starting. Try Start again.",
                );
            }
            service.revision = p.revision;
            if service
                .pending_choices
                .as_ref()
                .is_some_and(|choices| accepted_human_matches(&canonical, &choices.expected_human))
            {
                service.pending_choices = None;
            }
            service.accepted_options = Some(canonical);
            if changed {
                service.last_ready_request = None;
                crate::app::shell_network_setup::invalidate_network_agreement(state)?;
            }
            let generated = p.options.random_map.clone();
            if !state.match_state.session.host() {
                let names = p
                    .options
                    .launch
                    .humans
                    .iter()
                    .map(|human| human.name.clone())
                    .collect::<Vec<_>>();
                state
                    .match_state
                    .session
                    .canonicalize_online_roster(&names)
                    .inspect_err(|_| {
                        log::warn!("Online host options roster was not admitted");
                    })?;
                let options = p.options.into_options(&local).inspect_err(|_| {
                    log::warn!("Online host options had no admitted local player");
                })?;
                crate::app::shell_network_setup::service_event(
                    state,
                    NetworkSetupEvent::HostSetup(options),
                )
                .inspect_err(|_| {
                    log::warn!("Online host setup was not admitted");
                })?;
            }
            maps::received_options(state, service, generated)?;
            // The owner is ready without a guest Accept button, but still proves its
            // own local map and rules to the same server admission gate.
            let agreement = local_agreement(state, service.revision)?;
            let status = local_map_status(state, service);
            service.publish_options_agreement(agreement, status, state.match_state.session.host());
        }
        "choices" => {
            #[derive(Deserialize)]
            struct Payload {
                from: String,
                slot: crate::skirmish_launch::SkirmishLocalSlot,
                observer: bool,
            }
            let p: Payload = decode(payload)?;
            service_event(
                state,
                WolServiceEvent::PlayerChoices {
                    from: p.from,
                    slot: p.slot,
                    observer: p.observer,
                },
            );
        }
        "ready" => {
            if payload["revision"].as_u64() != Some(service.revision) {
                return Ok(());
            }
            let name = payload["from"]
                .as_str()
                .ok_or_else(|| anyhow!("missing player"))?
                .to_owned();
            let ready = payload["ready"]
                .as_bool()
                .ok_or_else(|| anyhow!("missing ready state"))?;
            service_event(
                state,
                WolServiceEvent::PlayerReady {
                    from: name,
                    value: i32::from(ready),
                },
            );
        }
        "map_status" => {
            if payload["revision"].as_u64() != Some(service.revision) {
                return Ok(());
            }
            let name = payload["from"]
                .as_str()
                .ok_or_else(|| anyhow!("missing player"))?
                .to_owned();
            let status = payload["status"]
                .as_u64()
                .filter(|value| *value <= 2)
                .ok_or_else(|| anyhow!("invalid map status"))? as u8;
            service_event(state, WolServiceEvent::MapStatus { from: name, status });
        }
        "map_available" => {
            let digest = payload["digest"]
                .as_str()
                .ok_or_else(|| anyhow!("missing map digest"))?;
            let total = payload["byte_length"]
                .as_u64()
                .ok_or_else(|| anyhow!("missing map length"))?;
            maps::available(state, service, digest, usize::try_from(total)?);
        }
        "chat" => {
            #[derive(Deserialize)]
            struct Payload {
                from: wire::Member,
                text: String,
                #[serde(default)]
                private: bool,
                #[serde(default)]
                action: bool,
                #[serde(default)]
                sent_at: Option<u64>,
            }
            let p: Payload = decode(payload)?;
            service_event(
                state,
                WolServiceEvent::Message {
                    from: p.from.into_member(&local),
                    text: p.text.encode_utf16().collect(),
                    private: p.private,
                    action: p.action,
                    ansi: false,
                    options: None,
                    sent_at: p.sent_at,
                },
            );
        }
        "locales" => {
            #[derive(Deserialize)]
            struct Locale {
                name: String,
                locale: i32,
            }
            #[derive(Deserialize)]
            struct Payload {
                locales: Vec<Locale>,
            }
            let p: Payload = decode(payload)?;
            service_event(
                state,
                WolServiceEvent::UserLocales(Ok(p
                    .locales
                    .into_iter()
                    .map(|entry| (entry.name, entry.locale))
                    .collect())),
            );
        }
        "buddies" => {
            #[derive(Deserialize)]
            struct Payload {
                names: Vec<String>,
            }
            let p: Payload = decode(payload)?;
            service_event(state, WolServiceEvent::Buddies(Ok(p.names)));
        }
        "located" => {
            #[derive(Deserialize)]
            struct Payload {
                channel: Option<wire::Channel>,
            }
            let p: Payload = decode(payload)?;
            service_event(
                state,
                WolServiceEvent::Located(p.channel.map(Into::into).ok_or(0x8000_4005)),
            );
        }
        "kicked" => {
            #[derive(Deserialize)]
            struct Payload {
                channel: wire::Channel,
                user: wire::Member,
                by: wire::Member,
            }
            let p: Payload = decode(payload)?;
            service_event(
                state,
                WolServiceEvent::Kicked {
                    channel: p.channel.into(),
                    user: p.user.into_member(&local),
                    by: p.by.into_member(&local),
                },
            );
        }
        "game_started" => {
            begin_start(state, service, decode(payload)?)?;
            service.start_preparation = None;
        }
        "start_confirmed" => {
            let id = payload["match_id"]
                .as_str()
                .ok_or_else(|| anyhow!("missing match"))?;
            let pending = service
                .pending
                .as_mut()
                .ok_or_else(|| anyhow!("no pending match"))?;
            ensure!(
                pending.match_id == id && pending.relay_acknowledged,
                "unadmitted start"
            );
            if !pending.probes_started {
                crate::app::lan_service::begin_online_latency_probe(state)?;
                pending.probes_started = true;
                pending.deadline = Instant::now() + service.relay_timeout;
            }
        }
        "timing_confirmed" => {
            #[derive(Deserialize)]
            struct TimingConfirmed {
                match_id: String,
                pings: Vec<i32>,
                relay: wire::RelayGrant,
            }
            let timing: TimingConfirmed = decode(payload)?;
            let pending = service
                .pending
                .as_ref()
                .ok_or_else(|| anyhow!("no pending match"))?;
            ensure!(
                pending.match_id == timing.match_id && pending.latencies_reported,
                "timing was not measured for this start"
            );
            let grant = decode_relay_grant(&timing.relay, &timing.match_id, &service.player_id)?;
            crate::app::lan_service::renew_online_relay(
                state,
                timing.relay.address,
                &grant.ticket,
                grant.key,
                grant.payload_key,
            )?;
            service.relay_granted(crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32);
            crate::app::lan_service::apply_online_timing(state, &timing.pings)?;
            let count = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow!("missing setup"))?
                .humans
                .len();
            for node in 1..count {
                crate::app::shell_network_setup::service_event(
                    state,
                    NetworkSetupEvent::PeerConnected { node },
                )?;
            }
            crate::app::shell_network_setup::service_event(
                state,
                NetworkSetupEvent::Negotiated { result: 0 },
            )?;
            service.match_id = Some(timing.match_id);
            service.pending = None;
        }
        "start_rejected" => {
            cancel_pending(state, service);
            fail_start(
                state,
                "The match could not start. Check every player's connection, map and version.",
            );
        }
        "error" => notice(state, "The online service rejected the request."),
        _ => {}
    }
    Ok(())
}

fn begin_start(state: &mut AppState, service: &mut LiveService, start: wire::Start) -> Result<()> {
    ensure!(service.pending.is_none(), "a start is already pending");
    ensure!(start.revision == service.revision, "stale options revision");
    ensure!(
        !state.match_state.session.host()
            || service
                .start_preparation
                .as_ref()
                .is_some_and(|request| request.submitted_request.is_some()),
        "host start was not submitted for this preparation"
    );
    let agreement = local_agreement(state, service.revision)?;
    ensure!(
        agreement["version"].as_str() == Some(&start.version),
        "client version differs"
    );
    ensure!(
        agreement["rules_digest"].as_str() == Some(&start.rules_digest),
        "rules differ"
    );
    ensure!(local_map_status(state, service) == 1, "local map differs");
    let names = start
        .users
        .iter()
        .map(|user| user.name.clone())
        .collect::<Vec<_>>();
    state
        .match_state
        .session
        .canonicalize_online_roster(&names)?;
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow!("no setup"))?;
    ensure!(
        start.users.len() == launch.humans.len(),
        "membership differs"
    );
    for (index, user) in start.users.iter().enumerate() {
        ensure!(
            usize::from(user.source_index) == index,
            "noncanonical source index"
        );
        ensure!(
            service.member_ids.get(&user.name.to_ascii_lowercase()) == Some(&user.player_id),
            "unknown player identity"
        );
    }
    let hosts: Vec<_> = start
        .users
        .iter()
        .filter(|user| user.flags & 1 != 0)
        .collect();
    ensure!(hosts.len() == 1, "start requires one admitted host");
    let host_name = hosts[0].name.clone();
    let local = start
        .users
        .iter()
        .find(|user| user.player_id == service.player_id)
        .ok_or_else(|| anyhow!("local user absent"))?;
    ensure!(
        local.source_index == start.relay.source_index,
        "relay grant belongs to another player"
    );
    ensure!(
        launch.local_player_index == usize::from(local.source_index),
        "setup source order differs"
    );
    ensure!(
        (local.flags & 1 != 0) == state.match_state.session.host(),
        "host identity differs"
    );
    for (index, user) in start.users.iter().enumerate() {
        ensure!(
            launch.humans[index].name.eq_ignore_ascii_case(&user.name),
            "setup source order differs"
        );
    }
    let grant = decode_relay_grant(&start.relay, &start.match_id, &service.player_id)?;
    admit_relay_start(
        state,
        service,
        RelayStart {
            match_id: start.match_id,
            host_name,
            users: start
                .users
                .into_iter()
                .map(|user| (user.name, user.flags))
                .collect(),
            seed: start.seed,
            address: start.relay.address,
            gateway: None,
            grant,
        },
    )
}

/// One admitted relay start, from the lobby's `game_started` or an external
/// launch's `[Relay]` section: identity checks are the caller's.
struct RelayStart {
    match_id: String,
    host_name: String,
    /// Canonical source order: name and lobby flags (bit 0 room owner).
    users: Vec<(String, u32)>,
    seed: u32,
    address: SocketAddrV4,
    /// A spawn launch's `[Relay] GatewayUrl`; the built-in lobby uses the compiled one.
    gateway: Option<String>,
    grant: DecodedRelayGrant,
}

/// The state right after `game_started`: the authenticated relay socket,
/// the bound roster and seed, and the start handshake's verify. Relay
/// admission, the latency barrier and `timing_confirmed` follow in
/// [`poll_relay`] and [`apply_event`].
fn admit_relay_start(
    state: &mut AppState,
    service: &mut LiveService,
    start: RelayStart,
) -> Result<()> {
    let grant = start.grant;
    let connections = LanConnections::new(
        LanConnectionConfig {
            max_peers: 29,
            ..Default::default()
        },
        RetryTiming {
            retry_delta: 15,
            max_retries: u32::MAX,
            timeout: 600,
        },
    );
    let any = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    let mut transport = LanUdpTransport::bind(
        UdpBindConfig {
            primary: any,
            alternate: any,
        },
        connections,
    )?;
    transport.set_relay_gateway(start.gateway);
    transport.enable_relay(start.address, &grant.ticket, grant.key, grant.payload_key)?;
    if let Some(roster) = &grant.federation {
        transport.set_federation_roster(roster)?;
    }
    let certificate = crate::net::udp::OrderingCertificate::parse(&grant.ordering_certificate)?;
    ensure!(
        certificate.sources_mask == (1u32 << start.users.len()) - 1,
        "gameplay clock roster differs"
    );
    transport.set_ordering_certificate(grant.ordering_certificate)?;
    transport.start_io_pump(crate::app::match_runtime::frame_pacer::wall_clock_bucket)?;
    let names: Vec<_> = start.users.iter().map(|(name, _)| name.clone()).collect();
    bind_relay_roster(&mut state.match_state.session, &names, start.seed)?;
    // Unknown startup latency uses the native sentinel while the verify dialog
    // is open. Real peer ACK measurements replace it before Negotiated.
    let pings = vec![-1; start.users.len()];
    crate::app::shell_network_setup::service_event(
        state,
        NetworkSetupEvent::GameStarted {
            users: start
                .users
                .into_iter()
                .map(|(name, flags)| crate::app::shell_network_setup::WolStartUser { name, flags })
                .collect(),
            pings,
        },
    )?;
    crate::app::shell_network_setup::service_event(
        state,
        NetworkSetupEvent::VerifyStarted { pairs: true },
    )?;
    service.pending = Some(PendingStart {
        match_id: start.match_id,
        host_name: start.host_name,
        transport: Some(transport),
        deadline: Instant::now() + service.relay_timeout,
        relay_acknowledged: false,
        probes_started: false,
        latencies_reported: false,
        report: None,
    });
    Ok(())
}

/// Every canonical source's relay peer identity and the agreed seed.
fn bind_relay_roster(
    session: &mut crate::app::match_runtime::session::MatchSession,
    names: &[String],
    seed: u32,
) -> Result<()> {
    for (source, name) in names.iter().enumerate() {
        session.bind_network_endpoint(
            name,
            crate::net::udp::relay_peer_endpoint(u8::try_from(source)?)?,
        )?;
    }
    session.set_network_start_seed(seed)
}

/// Enter the match an external launcher formed (docs/spawn.md): the online
/// visit and setup the lobby path holds right after `game_started`, with the
/// launcher's session, grant and seed. Everything after it is the lobby's.
pub(crate) fn begin_spawned_match(
    state: &mut AppState,
    launch: crate::skirmish_launch::SkirmishLaunchSession,
    relay: crate::app::frontend::cncnet_spawn::RelaySpawn,
    seed: u32,
) -> Result<()> {
    let mut service = LiveService::open(Some(&relay.service_url))?;
    service.spawned = true;
    service.relay_timeout = relay.admission_timeout;
    service.player_id = relay.player_id;
    // The launcher's bearer replaces `/v1/sessions`; nothing else is opened.
    service
        .requests
        .send(http::Request::AdoptSession { token: relay.token })
        .map_err(|_| anyhow!("the match service worker stopped"))?;
    let users: Vec<_> = launch
        .humans
        .iter()
        .enumerate()
        .map(|(source, human)| (human.name.clone(), u32::from(source == 0)))
        .collect();
    let host_name = users[0].0.clone();
    crate::app::shell_network_setup::install_external_setup(
        state,
        launch,
        crate::app::match_runtime::session::NetworkReturnContext::Wol,
    )?;
    let mut visit = super::WolVisit::open(false);
    visit.provider = WolProvider::Online;
    visit.page = super::WolPage::Backdrop { wait: None };
    visit.spawned = Some(SpawnedVisit::default());
    state.frontend.wol = Some(visit);
    let admitted = admit_relay_start(
        state,
        &mut service,
        RelayStart {
            match_id: relay.match_id,
            host_name,
            users,
            seed,
            address: relay.address,
            gateway: relay.gateway_url,
            grant: relay.grant,
        },
    );
    // Keep this visit's own worker even on failure: no lobby worker may
    // take its place while the process exits.
    put_service(state, service);
    admitted
}

/// Browser engine exit: retire the external launch's online visit through
/// the ordinary cleanup owner, so an unacknowledged `finish` still settles
/// before notifying the host and stopping the event loop.
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub(crate) fn discard_spawned_visit(state: &mut AppState) {
    if !super::spawned_visit(state) {
        return;
    }
    let Some(mut visit) = state.frontend.wol.take() else {
        return;
    };
    if let Some(service) = visit.live_service.take() {
        state
            .frontend
            .online_cleanup
            .retire(service, Instant::now());
    }
}

fn spawned_visit_mut(state: &mut AppState) -> Option<&mut SpawnedVisit> {
    state.frontend.wol.as_mut()?.spawned.as_mut()
}

/// The external launch's match returned or failed: report it, then quit.
/// False when this launch has no spawned online visit.
pub(crate) fn end_spawned_match(state: &mut AppState) -> bool {
    let Some(spawned) = spawned_visit_mut(state) else {
        return false;
    };
    spawned
        .exit_deadline
        .get_or_insert_with(|| Instant::now() + EXIT_CLEANUP_TIMEOUT);
    true
}

/// `finish` with the result after a match, else `leave` for a failed
/// admission; the process quits on its acknowledgement or the deadline.
fn poll_spawned_exit(state: &mut AppState, service: &mut LiveService) {
    let result = crate::app::App::external_match_result(state)
        .unwrap_or(crate::app::cncnet::ExternalMatchResult::Quit);
    let opponents_left = state.frontend.external_progress.opponents_left;
    let Some(spawned) = spawned_visit_mut(state) else {
        return;
    };
    let Some(deadline) = spawned.exit_deadline.filter(|_| !spawned.quit) else {
        return;
    };
    let now = Instant::now();
    if !std::mem::replace(&mut spawned.cleanup_begun, true) {
        if service.match_id.is_some() {
            service.finish_result = Some(result.as_str());
            service.finish_opponents_left = opponents_left;
            service.begin_finish(now);
        } else {
            service.begin_leave(now);
        }
    }
    if (service.finishing.is_none() && service.leaving.is_none()) || now >= deadline {
        spawned.quit = true;
        crate::app::App::start_quit_cascade(state);
    }
}

fn poll_relay(state: &mut AppState, service: &mut LiveService) -> Result<()> {
    let mut commands = Vec::new();
    let Some(pending) = service.pending.as_mut() else {
        return Ok(());
    };
    ensure!(
        Instant::now() < pending.deadline,
        "relay admission timed out"
    );
    if let Some(transport) = pending.transport.as_mut() {
        let now = crate::app::match_runtime::frame_pacer::frame_bucket(
            crate::app::match_runtime::frame_pacer::wall_clock_ms(),
        );
        // Only the deadline fails admission. A receive error such as Windows'
        // 10054 (ICMP port unreachable while the relay restarts) or a passing
        // ENOBUFS/ENETUNREACH clears while the handshake keeps retrying.
        transport.service(now);
        if transport.relay_ready() {
            let transport = pending.transport.take().expect("ready pending relay");
            crate::app::lan_service::install_online_transport(
                state,
                transport,
                &pending.host_name,
            )?;
            pending.relay_acknowledged = true;
            let match_id = pending.match_id.clone();
            commands.push(("relay_ready", json!({"match_id":match_id})));
        }
    }
    if pending.probes_started && !pending.latencies_reported {
        if let Some(pings) = crate::app::lan_service::online_peer_round_trips(state) {
            let pings: Vec<_> = pings.into_iter().map(|(source_index, round_trip_ms)|
                json!({"source_index":source_index,"round_trip_ms":round_trip_ms})).collect();
            let match_id = pending.match_id.clone();
            pending.latencies_reported = true;
            commands.push(("latency_ready", json!({"match_id":match_id,"pings":pings})));
        }
    }
    for (operation, payload) in commands {
        service.send_report(operation, payload, Instant::now());
    }
    service.poll_report(Instant::now());
    Ok(())
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    #[test]
    fn relay_grant_requires_match_player_and_encryption_key_agreement() {
        use crate::net::udp::{
            RelayTicket as Ticket,
            test_support::{SecretKey, derive_packet_key},
        };
        let match_id = "11111111-1111-1111-1111-111111111111";
        let player_id = "22222222-2222-2222-2222-222222222222";
        let signing_key = SecretKey::new([9; 32]);
        let ticket = Ticket {
            room: [0x11; 16],
            player: [0x22; 16],
            source: 1,
            generation: 1,
            expires: 300,
            nonce: [3; 16],
        }
        .sign(&signing_key)
        .unwrap();
        let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let mut grant = wire::RelayGrant {
            address: "127.0.0.1:5000".parse().unwrap(),
            ticket: encode(&ticket),
            packet_key: encode(derive_packet_key(&signing_key, &ticket).expose_for_https()),
            payload_key: encode(&[7; 32]),
            source_index: 1,
            ordering_certificate: encode(
                &crate::net::udp::OrderingCertificate {
                    room: [0x11; 16],
                    sources_mask: 3,
                    tick_hz: 45,
                    expires: 86400,
                }
                .sign(&signing_key)
                .unwrap(),
            ),
            federation: None,
        };
        assert_eq!(
            decode_relay_grant(&grant, match_id, player_id)
                .unwrap()
                .payload_key,
            [7; 32]
        );
        assert!(decode_relay_grant(&grant, player_id, player_id).is_err());
        assert!(decode_relay_grant(&grant, match_id, match_id).is_err());
        grant.payload_key.clear();
        assert!(decode_relay_grant(&grant, match_id, player_id).is_err());
    }

    fn control_service(
        match_id: Option<&str>,
    ) -> (LiveService, Receiver<http::Request>, Sender<http::Response>) {
        let (requests, input) = mpsc::channel();
        let (output, responses) = mpsc::channel();
        let service = LiveService {
            stopping: Arc::new(AtomicBool::new(false)),
            requests,
            responses,
            next_request: 5,
            auth_attempt: 0,
            revision: 0,
            accepted_options: None,
            start_preparation: None,
            last_ready_request: None,
            pending_choices: None,
            player_id: String::new(),
            member_ids: BTreeMap::new(),
            pending: None,
            match_id: match_id.map(str::to_owned),
            finishing: None,
            leaving: None,
            logging_out: None,
            next_renewal_attempt: None,
            relay_renewal_due: None,
            maps: Default::default(),
            translations: Translations::default(),
            spawned: false,
            finish_result: None,
            finish_opponents_left: false,
            relay_timeout: RELAY_TIMEOUT,
            outdated_notice: None,
        };
        (service, input, output)
    }

    #[test]
    fn lobby_upgrade_notice_names_the_download_once_per_sign_in() {
        let (mut service, _input, _output) = control_service(None);
        let reason = "Online play needs VERA20k 0.3.0 or newer.";
        assert_eq!(
            service.outdated_notice(Some(reason)).as_deref(),
            Some(format!("{reason} {RELEASES_URL}").as_str())
        );
        assert_eq!(service.outdated_notice(Some(reason)), None);
        service.auth_attempt += 1;
        assert_eq!(
            service.outdated_notice(None).as_deref(),
            Some(
                format!("Update VERA20k to use secure online multiplayer. {RELEASES_URL}").as_str()
            )
        );
    }

    fn finish_payload_sent(service: &mut LiveService, input: &Receiver<http::Request>) -> Value {
        service.begin_finish(Instant::now());
        let http::Request::Command {
            operation, payload, ..
        } = input.try_recv().unwrap()
        else {
            panic!("finish command");
        };
        assert_eq!(operation, "finish");
        payload
    }

    #[test]
    fn only_a_spawned_match_adds_its_result_to_finish() {
        // docs/lobby-protocol.md: the lobby rejects unknown fields.
        let (mut lobby, input, _output) = control_service(Some("lobby-match"));
        lobby.finish_result = Some("won");
        assert_eq!(
            finish_payload_sent(&mut lobby, &input),
            json!({"match_id":"lobby-match"})
        );

        let (mut spawned, input, _output) = control_service(Some("spawned-match"));
        spawned.spawned = true;
        spawned.finish_result = Some("lost");
        let payload = finish_payload_sent(&mut spawned, &input);
        assert_eq!(
            payload,
            json!({"match_id":"spawned-match","result":"lost","opponents_left":false})
        );

        // Retries resend the same decided result.
        spawned.finish_failed(5, http::CommandFailure::Retryable, Instant::now());
        spawned.poll_cleanup(Instant::now() + Duration::from_secs(3));
        let http::Request::Command { payload: retry, .. } = input.try_recv().unwrap() else {
            panic!("finish retry");
        };
        assert_eq!(retry, payload);
        spawned.finish_opponents_left = true;
        assert_eq!(spawned.finish_payload("m")["opponents_left"], json!(true));
    }

    #[test]
    fn relay_renewal_is_timed_from_grant_receipt_at_half_life() {
        // A slow local clock made the ticket's lobby-time expiry look distant,
        // so renewal was requested only after the relay rejected the ticket.
        // Half-life leaves a lobby outage 150 s before the ticket expires; the
        // receipt clock's 32-bit word wraps in between.
        let (mut service, input, _output) = control_service(Some("renew-match"));
        let received = u32::MAX - 100_000;
        service.relay_granted(received);
        service.poll_renewal(received + 1_000);
        service.poll_renewal(received.wrapping_add(149_999));
        assert!(input.try_recv().is_err());
        let due = received.wrapping_add(150_000);
        service.poll_renewal(due);
        let http::Request::Command {
            operation, payload, ..
        } = input.try_recv().unwrap()
        else {
            panic!("renewal command");
        };
        assert_eq!(
            (operation, payload),
            ("renew_relay", json!({"match_id":"renew-match"}))
        );
        // The reply's fresh grant moves the deadline half a lifetime on.
        service.relay_granted(due + 1_000);
        service.poll_renewal(due + 11_000);
        assert!(input.try_recv().is_err());
    }

    #[test]
    fn admission_reports_resend_within_the_start_window_after_a_busy_lobby() {
        // One 429 or 503 to relay_ready used to fail the start at its deadline.
        let (mut service, input, _output) = control_service(None);
        let now = Instant::now();
        service.pending = Some(PendingStart {
            match_id: "m".into(),
            host_name: "Host".into(),
            transport: None,
            deadline: now + RELAY_TIMEOUT,
            relay_acknowledged: true,
            probes_started: false,
            latencies_reported: false,
            report: None,
        });
        let sent = || match input.try_recv() {
            Ok(http::Request::Command {
                id,
                operation,
                payload,
            }) => Some((id, operation, payload)),
            _ => None,
        };
        service.send_report("relay_ready", json!({"match_id":"m"}), now);
        let first = sent().unwrap();
        assert!(service.report_failed(first.0, http::CommandFailure::Retryable, now));
        service.poll_report(now + Duration::from_millis(1999));
        assert_eq!(sent(), None);
        service.poll_report(now + Duration::from_secs(2));
        assert_eq!(sent(), Some(first.clone()), "same request ID and payload");
        service.report_completed(first.0);
        service.poll_report(now + RELAY_TIMEOUT);
        assert_eq!(sent(), None);

        // A refusal for any other reason stands.
        service.send_report("latency_ready", json!({"match_id":"m","pings":[]}), now);
        let latency = sent().unwrap();
        let rejected = http::CommandFailure::Rejected(0x8000_4005);
        assert!(!service.report_failed(latency.0, rejected, now));
        service.poll_report(now + RELAY_TIMEOUT);
        assert_eq!(sent(), None);
    }

    #[test]
    fn spawned_session_binds_the_same_relay_roster_as_the_lobby_start() {
        // A guest at source 1: the lobby admits its own node first, then
        // canonicalizes the room roster before game_started binds it.
        let text = "[Settings]\nName=P1\nSide=1\nColor=1\nPlayerCount=2\nSourceIndex=1\n\
                    Seed=77\nSpecialHouseIsAlly=No\n[Other1]\nName=P0\nSide=0\nColor=0\n\
                    SourceIndex=0\nIp=10.0.0.1\n";
        let crate::app::frontend::cncnet_spawn::SpawnGame::Network(network) =
            crate::app::frontend::cncnet_spawn::SpawnRequest::parse(text)
                .unwrap()
                .game
        else {
            panic!("network spawn");
        };
        let canonical = network.session;
        let names = ["P0".to_owned(), "P1".to_owned()];

        let mut lobby = crate::app::match_runtime::session::MatchSession::default();
        lobby.enter_network(crate::app::match_runtime::session::NetworkReturnContext::Wol);
        let mut local_first = canonical.clone();
        local_first.humans.reverse();
        local_first.local_player_index = 0;
        lobby
            .accept_network_setup(local_first, false, None, false)
            .unwrap();
        lobby.canonicalize_online_roster(&names).unwrap();
        bind_relay_roster(&mut lobby, &names, 77).unwrap();

        let mut spawned = crate::app::match_runtime::session::MatchSession::default();
        crate::app::shell_network_setup::accept_external_session(
            &mut spawned,
            canonical,
            crate::app::match_runtime::session::NetworkReturnContext::Wol,
        )
        .unwrap();
        bind_relay_roster(&mut spawned, &names, 77).unwrap();

        assert_eq!(spawned.network_setup(), lobby.network_setup());
        assert_eq!(spawned.network_setup().unwrap().local_player_index, 1);
        assert_eq!(spawned.host(), lobby.host());
        assert_eq!(spawned.network_start_seed(), Some(77));
        assert_eq!(spawned.network_start_seed(), lobby.network_start_seed());
        for source in 0..2 {
            assert_eq!(
                spawned.network_endpoint(source),
                Some(crate::net::udp::relay_peer_endpoint(source as u8).unwrap())
            );
            assert_eq!(
                spawned.network_endpoint(source),
                lobby.network_endpoint(source)
            );
        }
    }

    #[test]
    fn options_agreement_keeps_map_status_out_of_host_ready() {
        let (mut service, input, _output) = control_service(None);
        let agreement = json!({
            "revision": 7, "version": "0.2.2", "rules_digest": "rules",
            "map_digest": "physical", "effective_map_digest": "effective",
            "staging_digest": "staging"
        });
        service.publish_options_agreement(agreement.clone(), 1, true);
        let next = || match input.try_recv().expect("queued agreement") {
            http::Request::Command {
                operation, payload, ..
            } => (operation, payload),
            _ => panic!("expected a protocol command"),
        };
        let mut expected_map = agreement.clone();
        expected_map["status"] = json!(1);
        let mut expected_ready = agreement.clone();
        expected_ready["ready"] = json!(true);
        assert_eq!(next(), ("map_status", expected_map));
        assert_eq!(next(), ("ready", expected_ready));
        assert!(input.try_recv().is_err());

        service.publish_options_agreement(agreement.clone(), 0, false);
        let mut expected_map = agreement;
        expected_map["status"] = json!(0);
        assert_eq!(next(), ("map_status", expected_map));
        assert!(
            input.try_recv().is_err(),
            "a guest's readiness comes from its setup's Accept"
        );
    }

    #[test]
    fn translations_stay_bounded_and_pause_after_an_unavailable_translator() {
        use super::super::translation::TranslatedLine;
        use crate::ui::chat_translation::ChatLanguage;
        let (requests, input) = mpsc::channel();
        let mut translations = Translations::default();
        let now = Instant::now();
        let line = |name: &str| TranslatedLine::Match {
            name: name.into(),
            color: 0,
        };
        let mut ask = |name, at| {
            let text: Vec<u16> = "rush".encode_utf16().collect();
            translations.request(
                &requests,
                &text,
                ChatLanguage::SimplifiedChinese,
                line(name),
                at,
            );
        };
        for name in ["A", "B", "C", "D", "E"] {
            ask(name, now);
        }
        let sent: Vec<_> = input
            .try_iter()
            .map(|request| match request {
                http::Request::Translate { id, target, .. } => (id, target),
                _ => panic!("only translations were asked"),
            })
            .collect();
        assert_eq!(sent, [0, 1, 2, 3].map(|id| (id, "zh-Hans")));
        let shown = translations.reply(0, Ok("冲".into()), now);
        assert!(shown.is_some_and(|(shown, text)| shown == line("A") && text == "冲"));
        let failed = translations.reply(1, Err(http::TranslateFailure::Unavailable), now);
        assert!(failed.is_none());
        let mut ask = |name, at| {
            let text: Vec<u16> = "rush".encode_utf16().collect();
            translations.request(
                &requests,
                &text,
                ChatLanguage::SimplifiedChinese,
                line(name),
                at,
            );
        };
        ask("F", now + TRANSLATION_PAUSE - Duration::from_secs(1));
        assert_eq!(input.try_iter().count(), 0);
        ask("G", now + TRANSLATION_PAUSE);
        assert_eq!(input.try_iter().count(), 1);
    }

    fn setup_session() -> crate::app::match_runtime::session::MatchSession {
        use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext};
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(fixture["launch"].clone()).unwrap();
        let mut guest = launch.humans[0].clone();
        launch.humans[0].name = "Host".into();
        launch.humans[0].raw_slot_index = Some(0);
        guest.name = "Guest".into();
        guest.raw_slot_index = Some(1);
        launch.humans = vec![launch.humans[0].clone(), guest];
        launch.local_player_index = 0;
        launch.opponents.clear();
        launch.ai_player_count = 0;
        launch.rebuild_house_roster();
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch, true, None, false)
            .unwrap();
        session
    }

    #[test]
    fn final_start_publication_requires_fresh_revision_readiness_and_map_acknowledgements() {
        let mut session = setup_session();
        for index in 0..2 {
            session.set_network_player_ready(index, true).unwrap();
            session.set_network_map_status(index, 1).unwrap();
        }
        let options = json!({"launch":session.network_setup()});
        let mut start = StartPreparation {
            users: vec!["Host".into(), "Guest".into()],
            options: options.clone(),
            revision: None,
            publication_request: Some(9),
            submitted_request: None,
            deadline: Instant::now() + RELAY_TIMEOUT,
        };
        assert!(!start.ready(&session, 4));
        assert!(start.accept_options(5, &options));
        session.invalidate_network_agreement().unwrap();
        assert!(!start.ready(&session, 4));
        assert!(!start.ready(&session, 5));
        session.set_network_player_ready(1, true).unwrap();
        assert!(
            !start.ready(&session, 5),
            "old map status cannot satisfy a new revision"
        );
        session.set_network_map_status(1, 1).unwrap();
        assert!(start.ready(&session, 5));
        assert!(!start.accept_options(6, &json!({"changed":true})));
    }

    #[test]
    fn optimistic_local_choice_waits_for_the_authoritative_options_echo() {
        let mut session = setup_session();
        let accepted = json!({"launch":session.network_setup()});
        assert!(local_choices_match(&session, &accepted));
        let mut launch = session.network_setup().unwrap().clone();
        let random = !launch.local_slot().color_random;
        launch.local_slot_mut().color_random = random;
        launch.rebuild_house_roster();
        session
            .accept_network_setup(launch, true, None, false)
            .unwrap();
        assert!(!local_choices_match(&session, &accepted));
        let expected =
            serde_json::to_value(session.network_setup().unwrap().local_player()).unwrap();
        assert!(
            !accepted_human_matches(&accepted, &expected),
            "an unrelated old echo does not acknowledge the outgoing request"
        );
        let echoed = json!({"launch":session.network_setup()});
        assert!(local_choices_match(&session, &echoed));
        assert!(accepted_human_matches(&echoed, &expected));
    }

    #[test]
    fn loading_return_while_service_is_taken_keeps_finish_before_lobby_admission() {
        let (service, input, _output) = control_service(Some("loading-match"));
        let mut visit = super::super::WolVisit::open(false);
        visit.provider = WolProvider::Online;
        visit.live_service = Some(service);
        let borrowed = visit.live_service.take().unwrap(); //The actual poll boundary.
        let now = Instant::now();
        visit.requests.push(WolServiceRequest::JoinChannel {
            channel: crate::ui::wol_lobby::WolChannel::default(),
            key: String::new(),
        });
        visit.match_returned(now);
        visit.match_returned(now); //Repeated abort callbacks keep one intent.
        assert!(matches!(
            visit.take_service_requests().as_slice(),
            [
                WolServiceRequest::MatchReturned,
                WolServiceRequest::JoinChannel { .. }
            ]
        ));
        assert!(
            input.try_recv().is_err(),
            "no second service was manufactured"
        );
        assert_eq!(borrowed.match_id.as_deref(), Some("loading-match"));
        visit.live_service = Some(borrowed);
        visit.match_returned(now);
        let http::Request::Command {
            id,
            operation,
            payload,
        } = input.try_recv().unwrap()
        else {
            panic!("same retained service must finish the same match");
        };
        assert_eq!(
            (id, operation, payload),
            (5, "finish", json!({"match_id":"loading-match"}))
        );
        let service = visit.live_service.as_mut().unwrap();
        assert!(service.admission_blocked());
        service.finish_completed(id);
        assert!(!service.admission_blocked());
        service.clear_cleanup(); //Definitive Offline has retired the authority.
        visit.match_returned(now);
        assert!(
            input.try_recv().is_err(),
            "expired authority cannot issue finish"
        );
    }

    #[test]
    fn finish_retries_the_same_request_and_retains_match_until_acknowledged() {
        let (mut service, input, _output) = control_service(Some("test-match"));
        let now = Instant::now();
        service.begin_finish(now);
        let http::Request::Command {
            id,
            operation,
            payload,
        } = input.try_recv().unwrap()
        else {
            panic!("finish command");
        };
        assert_eq!((id, operation), (5, "finish"));
        assert_eq!(payload["match_id"], "test-match");
        service.finish_failed(id, http::CommandFailure::Retryable, now);
        assert_eq!(service.match_id.as_deref(), Some("test-match"));
        service.poll_cleanup(now + Duration::from_secs(1));
        assert!(input.try_recv().is_err());
        service.poll_cleanup(now + Duration::from_secs(2));
        let http::Request::Command {
            id: retry_id,
            operation,
            payload: retry_payload,
        } = input.try_recv().unwrap()
        else {
            panic!("retry finish command");
        };
        assert_eq!(
            (retry_id, operation, retry_payload),
            (id, "finish", payload)
        );
        service.finish_completed(id + 1);
        assert!(service.finishing.is_some());
        service.finish_completed(id);
        assert!(service.finishing.is_none() && service.match_id.is_none());
        service.poll_cleanup(now + Duration::from_secs(60));
        assert!(input.try_recv().is_err());
        service.begin_leave(now);
        let http::Request::Command {
            id: leave_id,
            operation,
            ..
        } = input.try_recv().unwrap()
        else {
            panic!("leave command");
        };
        assert_eq!(operation, "leave");
        assert!(service.admission_blocked());
        service.leave_failed(leave_id, now);
        service.poll_cleanup(now + Duration::from_secs(2));
        let http::Request::Command { id, operation, .. } = input.try_recv().unwrap() else {
            panic!("retry leave command");
        };
        assert_eq!((id, operation), (leave_id, "leave"));
        service.leave_completed(leave_id + 1);
        assert!(service.admission_blocked());
        service.leave_completed(leave_id);
        assert!(!service.admission_blocked());
        service.begin_logout(now);
        let http::Request::Command {
            id: logout_id,
            operation,
            ..
        } = input.try_recv().unwrap()
        else {
            panic!("logout command");
        };
        assert_eq!(operation, "logout");
        service.logout_failed(logout_id, now);
        service.poll_cleanup(now + Duration::from_secs(2));
        let http::Request::Command { id, operation, .. } = input.try_recv().unwrap() else {
            panic!("retry logout command");
        };
        assert_eq!((id, operation), (logout_id, "logout"));
        service.logout_completed(logout_id);
        assert!(!service.admission_blocked());
    }
    #[test]
    fn online_exit_owner_retries_logout_after_the_visit_is_dropped_and_ignores_old_callbacks() {
        let (service, input, output) = control_service(None);
        let mut visit = super::super::WolVisit::open(false);
        visit.live_service = Some(service);
        let now = Instant::now();
        let mut cleanup = OnlineCleanup::default();
        cleanup.leave(&mut visit, now);
        assert!(visit.live_service.is_none());
        drop(visit);
        let http::Request::Command {
            id,
            operation,
            payload,
        } = input.try_recv().unwrap()
        else {
            panic!("logout");
        };
        assert_eq!((operation, payload), ("logout", json!({})));
        assert!(
            output
                .send(http::Response::CommandFailed {
                    id,
                    operation: "logout",
                    failure: http::CommandFailure::Retryable
                })
                .is_ok()
        );
        let _ = cleanup.poll(now);
        let _ = cleanup.poll(now + Duration::from_secs(1));
        assert!(input.try_recv().is_err());
        let observation = cleanup.logout_observation();
        assert_eq!(observation.request_id, Some(id));
        assert_eq!(observation.failures, 1);
        assert!(observation.pending && !observation.acknowledged && !observation.worker_closed);
        let _ = cleanup.poll(now + Duration::from_secs(2));
        let http::Request::Command {
            id: retry,
            operation,
            ..
        } = input.try_recv().unwrap()
        else {
            panic!("logout retry");
        };
        assert_eq!((retry, operation), (id, "logout"));
        assert!(
            output
                .send(http::Response::CommandCompleted {
                    id: id + 1,
                    operation: "logout"
                })
                .is_ok()
        );
        assert!(
            output
                .send(http::Response::Event(wire::Envelope {
                    sequence: 1,
                    kind: "channel_left".into(),
                    payload: json!({})
                }))
                .is_ok()
        );
        assert!(
            output
                .send(http::Response::Authenticated {
                    attempt: 0,
                    player_id: "old-player".into(),
                    nickname: "OldVisit".into(),
                    motd: None
                })
                .is_ok()
        );
        let _ = cleanup.poll(now + Duration::from_secs(2));
        let retired = cleanup.retired.as_ref().unwrap();
        assert!(!cleanup.logout_acknowledged());
        assert!(!retired.service.stopping.load(Ordering::Acquire));
        assert!(
            retired.service.player_id.is_empty(),
            "retired login callbacks never enter the active UI/service"
        );
        assert!(
            output
                .send(http::Response::CommandCompleted {
                    id,
                    operation: "logout"
                })
                .is_ok()
        );
        let _ = cleanup.poll(now + Duration::from_secs(3));
        assert!(
            cleanup
                .retired
                .as_ref()
                .unwrap()
                .service
                .stopping
                .load(Ordering::Acquire)
        );
        assert!(
            cleanup.retired.is_some(),
            "relogin waits for the old worker to stop"
        );
        assert!(cleanup.pending() && cleanup.logout_acknowledged());
        let observation = cleanup.logout_observation();
        assert_eq!(observation.request_id, Some(id));
        assert_eq!(observation.failures, 1);
        assert!(observation.acknowledged && !observation.worker_closed);
        drop(output);
        assert!(cleanup.poll(now + Duration::from_secs(3)).is_none());
        assert!(!cleanup.pending() && cleanup.logout_acknowledged());
        let observation = cleanup.logout_observation();
        assert_eq!(observation.request_id, Some(id));
        assert!(observation.worker_closed && observation.acknowledged_uptime_ms.is_some());
    }

    #[test]
    fn online_exit_owner_acknowledges_finish_and_leave_before_logout() {
        let (mut service, input, output) = control_service(Some("departing-match"));
        let now = Instant::now();
        service.begin_leave(now);
        let mut visit = super::super::WolVisit::open(false);
        visit.live_service = Some(service);
        let mut cleanup = OnlineCleanup::default();
        cleanup.leave(&mut visit, now);
        drop(visit);
        let http::Request::Command {
            id: leave,
            operation: "leave",
            ..
        } = input.try_recv().unwrap()
        else {
            panic!("leave");
        };
        let http::Request::Command {
            id: finish,
            operation: "finish",
            payload,
        } = input.try_recv().unwrap()
        else {
            panic!("finish");
        };
        assert_eq!(payload, json!({"match_id":"departing-match"}));
        for (id, operation) in [(leave, "leave"), (finish, "finish")] {
            assert!(
                output
                    .send(http::Response::CommandFailed {
                        id,
                        operation,
                        failure: http::CommandFailure::Retryable
                    })
                    .is_ok()
            );
        }
        let _ = cleanup.poll(now);
        let _ = cleanup.poll(now + Duration::from_secs(2));
        let http::Request::Command {
            id,
            operation,
            payload,
        } = input.try_recv().unwrap()
        else {
            panic!("finish retry");
        };
        assert_eq!(
            (id, operation, payload),
            (finish, "finish", json!({"match_id":"departing-match"}))
        );
        let http::Request::Command { id, operation, .. } = input.try_recv().unwrap() else {
            panic!("leave retry");
        };
        assert_eq!((id, operation), (leave, "leave"));
        assert!(
            output
                .send(http::Response::CommandCompleted {
                    id: finish,
                    operation: "finish"
                })
                .is_ok()
        );
        let _ = cleanup.poll(now + Duration::from_secs(3));
        assert!(
            input.try_recv().is_err(),
            "leave acknowledgement still owns the room drain"
        );
        assert!(
            output
                .send(http::Response::CommandCompleted {
                    id: leave,
                    operation: "leave"
                })
                .is_ok()
        );
        let _ = cleanup.poll(now + Duration::from_secs(3));
        let http::Request::Command {
            id: logout,
            operation: "logout",
            ..
        } = input.try_recv().unwrap()
        else {
            panic!("logout after cleanup");
        };
        assert!(
            output
                .send(http::Response::CommandCompleted {
                    id: logout,
                    operation: "logout"
                })
                .is_ok()
        );
        let _ = cleanup.poll(now + Duration::from_secs(4));
        drop(output);
        assert!(cleanup.poll(now + Duration::from_secs(4)).is_none());
    }

    #[test]
    fn online_exit_finishes_a_late_committed_start_and_bounds_worker_shutdown() {
        let (service, input, output) = control_service(None);
        let mut visit = super::super::WolVisit::open(false);
        visit.live_service = Some(service);
        let now = Instant::now();
        let mut cleanup = OnlineCleanup::default();
        cleanup.leave(&mut visit, now);
        drop(visit);
        let http::Request::Command {
            id: logout,
            operation: "logout",
            ..
        } = input.try_recv().unwrap()
        else {
            panic!("initial logout");
        };
        assert!(
            output
                .send(http::Response::Event(wire::Envelope {
                    sequence: 1,
                    kind: "game_started".into(),
                    payload: json!({"match_id":"late-match"})
                }))
                .is_ok()
        );
        assert!(
            output
                .send(http::Response::CommandFailed {
                    id: logout,
                    operation: "logout",
                    failure: http::CommandFailure::Rejected(0x8000_4005)
                })
                .is_ok()
        );
        let _ = cleanup.poll(now);
        let http::Request::Command {
            operation: "finish",
            payload,
            ..
        } = input.try_recv().unwrap()
        else {
            panic!("late match finish");
        };
        assert_eq!(payload, json!({"match_id":"late-match"}));
        let _ = cleanup.poll(now + EXIT_CLEANUP_TIMEOUT);
        let stopping = Arc::clone(&cleanup.retired.as_ref().unwrap().service.stopping);
        assert!(stopping.load(Ordering::Acquire));
        assert!(!cleanup.logout_acknowledged());
        assert!(
            input.try_recv().is_err(),
            "deadline cannot issue another cleanup attempt"
        );
        // The actual worker skips queued HTTP commands once the existing owner
        // stops it. No server or bearer is needed to exercise this boundary.
        let (send, commands) = mpsc::channel();
        assert!(
            send.send(http::Request::Command {
                id: 1,
                operation: "logout",
                payload: json!({})
            })
            .is_ok()
        );
        drop(send);
        let (events, received) = mpsc::channel();
        http::run("https://127.0.0.1:1".into(), commands, events, stopping);
        assert!(received.try_recv().is_err());
        // The welcome can already have completed logout before Main Menu.
        // The same worker knows it holds no bearer and completes idempotently
        // without touching an unauthenticated endpoint or waiting 90 seconds.
        let (send, commands) = mpsc::channel();
        send.send(http::Request::Command {
            id: 2,
            operation: "logout",
            payload: json!({}),
        })
        .unwrap();
        drop(send);
        let (events, received) = mpsc::channel();
        http::run(
            "https://127.0.0.1:1".into(),
            commands,
            events,
            Arc::new(AtomicBool::new(false)),
        );
        assert!(matches!(
            received.try_recv(),
            Ok(http::Response::CommandCompleted {
                id: 2,
                operation: "logout"
            })
        ));
        assert!(
            cleanup.retired.is_some(),
            "deadline retains the quiescence barrier"
        );
        drop(output);
        assert!(cleanup.poll(now + EXIT_CLEANUP_TIMEOUT).is_none());
    }
}
