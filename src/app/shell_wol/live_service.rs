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
use crate::app::persistence::account;
use crate::app::shell_network_setup::{NetworkSetupEvent, NetworkSetupRequest};
use crate::i18n::tr;
use crate::net::reliable::{LanConnectionConfig, LanConnections, RetryTiming};
use crate::net::udp::{LanUdpTransport, UdpBindConfig};
use crate::ui::chat_translation::{ChatReply, only_repeats, without_note};
use crate::ui::wol_account::AccountRequest;
use crate::ui::wol_chat::WolChatLine;

// The deployment sets this at build time; a runtime override supports staging.
const DEFAULT_LOBBY_URL: &str = match option_env!("VERA20K_LOBBY_URL") {
    Some(value) => value,
    None => "https://lobby.vera20k.org",
};
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const RELAY_TIMEOUT: Duration = Duration::from_secs(30);
const EXIT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(90);
/// The most `ended_ago` VERA20K Online's lobby accepts in a `finish`, a day.
const ENDED_AGO_LIMIT: u64 = 24 * 60 * 60;
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

/// The configured lobby's HTTPS origin: the build's, or `VERA20K_LOBBY_URL`
/// for staging.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn lobby_base_url() -> Result<String> {
    let base = std::env::var("VERA20K_LOBBY_URL").unwrap_or_else(|_| DEFAULT_LOBBY_URL.to_owned());
    ensure!(
        base.starts_with("https://") && !base[8..].chars().any(|ch| matches!(ch, '@' | '?' | '#')),
        "the online lobby needs its configured HTTPS service"
    );
    Ok(base)
}

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
                    "finish" => {
                        retired.service.finish_failed(id, failure, now);
                    }
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
    /// VERA: the finish asks the lobby to give this player's game room back
    /// (`back_to_room`), set from the visit when the match returns.
    back_to_room: bool,
    /// The match's `finish` result (`ExternalMatchResult`): a spawned
    /// match's for its launcher, a lobby match's for the ranked ladder.
    finish_result: Option<&'static str>,
    /// A lobby match's `winners`: the players still in it whose Houses won.
    finish_winners: Option<Vec<String>>,
    /// VERA: when a lobby match's result was decided, which `finish` sends
    /// as `ended_ago`.
    finish_decided: Option<Instant>,
    /// docs/spawn.md: every opponent left before the local player did.
    finish_opponents_left: bool,
    /// The lobby match's network report (`network_quality`), kept as its
    /// transport returned, for its `finish`.
    network_quality: Option<Value>,
    /// Each admission barrier's client deadline; `[Relay] AdmissionTimeout`
    /// for a spawned match.
    relay_timeout: Duration,
    /// The sign-in (`auth_attempt`) that already showed its lobby-upgrade notice.
    outdated_notice: Option<u64>,
    /// This client left its room for its started match: Quick Match leaves
    /// it as the match starts (API+0x24 at `0x007955F1`). The `channel_left`
    /// that answers is not the room closing under the loading map.
    left_for_match: bool,
    /// The names of the last ladder lookup, which the high-score request
    /// for the same names shares (one `ladder` event answers both tables).
    ladder_lookup: Option<Vec<String>>,
    /// The lobby's origin, whose remembered sign-in this service sends and
    /// keeps.
    lobby: String,
    /// The current sign-in sends this computer's remembered sign-in.
    signing_in_remembered: bool,
    /// The latest account request: an answer to an earlier one, whose popup
    /// was cancelled, is dropped.
    account_attempt: u64,
    /// The pending Delete Account request's attempt and name. Its answer
    /// forgets this computer's sign-in of the name, popup or not.
    deleting: Option<(u64, String)>,
}

/// Where players download a newer build (README "Download and play").
pub(crate) const RELEASES_URL: &str = "https://github.com/YuriPlanet/vera20k/releases";

/// The label of the login box's text for a lobby that did not answer,
/// instead of retail TXT_CANT_CONNECT's "Westwood Online".
const CANT_CONNECT_TEXT: &str = "VERA:OnlineCantConnect";

/// The label of the box's text when the lobby stops answering while the
/// player is in it.
const LOBBY_LOST_TEXT: &str = "VERA:OnlineConnectionLost";

/// The lobby's own 426 reason when it gave a plain one, then where to get a
/// newer build; without one, VERA's own sentence with the link.
fn update_required_text(message: Option<&str>) -> String {
    match message {
        Some(reason) => format!("{reason} {RELEASES_URL}"),
        None => crate::i18n::tr_fill("VERA:OnlineUpdateRequired", &[("url", &RELEASES_URL)]),
    }
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
    /// The same row with a Random country: the host's answer when one of
    /// VERA's room bans takes the asked country.
    expected_random: Value,
    deadline: Instant,
}

impl PendingChoices {
    fn answered_by(&self, accepted: &Value) -> bool {
        accepted_choices_match(accepted, &self.expected_human)
            || accepted_choices_match(accepted, &self.expected_random)
    }
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
    /// VERA: the lobby's `start` takes the room's humans in the options'
    /// order, while native API+0x48 sends its caller's order: Quick Match's
    /// is reverse channel order (`0x0077EA53..0x0077EB72`). The same names
    /// in another order go in the humans' order; any other list stays as it
    /// is and never becomes ready.
    fn users_in_human_order(
        session: &crate::app::match_runtime::session::MatchSession,
        users: Vec<String>,
    ) -> Vec<String> {
        let Some(launch) = session.network_setup() else {
            return users;
        };
        let humans: Vec<String> = launch
            .humans
            .iter()
            .map(|human| human.name.clone())
            .collect();
        let mut requested = users.clone();
        let mut expected = humans.clone();
        requested.sort();
        expected.sort();
        if requested == expected { humans } else { users }
    }

    fn accept_options(&mut self, revision: u64, options: &Value) -> bool {
        if &self.options != options {
            return false;
        }
        self.revision = Some(revision);
        true
    }

    /// Every guest accepted this revision and every player holds its map.
    /// The host's own acceptance is its `ready` with each options revision
    /// (`publish_options_agreement`), not its local flag: a Quick Match
    /// host's flag falls with the matchbot's options (the material change
    /// `0x005E3AB0` consumes), and its start (`0x0077EA53`) never reads it.
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
                        (index == launch.local_player_index || session.network_player_ready(index))
                            && session.network_map_status(index) == Some(1)
                    })
            })
    }
}

// A busy room cannot queue minutes of model calls: a line that finds this
// many waiting stays untranslated. An unavailable translator pauses them.
const MAX_PENDING_TRANSLATIONS: usize = 4;
const TRANSLATION_PAUSE: Duration = Duration::from_secs(60);

/// Received chat lines waiting for `POST /v1/translate`, by request id: where
/// each one shows, and its text.
#[derive(Default)]
struct Translations {
    next: u64,
    pending: BTreeMap<u64, (super::translation::TranslatedLine, String)>,
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
        let request = http::Request::Translate {
            id,
            text: text.clone(),
            target,
        };
        if requests.send(request).is_ok() {
            self.pending.insert(id, (line, text));
        }
    }

    fn reply(
        &mut self,
        id: u64,
        outcome: Result<ChatReply, http::TranslateFailure>,
        now: Instant,
    ) -> Option<(super::translation::TranslatedLine, ChatReply)> {
        let (line, original) = self.pending.remove(&id)?;
        match outcome {
            Ok(reply) if only_repeats(&original, without_note(&reply.text)) => None,
            Ok(reply) => Some((line, reply)),
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
    /// A returning player: the match started without this process, so its
    /// barriers are over and these are the timing it agreed.
    resume_pings: Option<Vec<i32>>,
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
            account_attempt: 0,
            deleting: None,
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

    /// The `ladder` command for the ladder (`0x007A3100`) or high-score
    /// (`0x007A3B30`) table. Its one event answers both, so the shell's
    /// high-score request for the names it just asked the ladder about
    /// shares that command.
    fn look_up_ladder(&mut self, names: Vec<String>, high_scores: bool) {
        if high_scores && self.ladder_lookup.take().as_ref() == Some(&names) {
            return;
        }
        self.command("ladder", json!({"names":names}));
        self.ladder_lookup = (!high_scores).then_some(names);
    }

    /// The match returned to the shell: finish it, a lobby match with its
    /// result and winners. A spawned match reports when the process exits.
    fn match_returned(&mut self, (result, winners, decided): MatchReport, now: Instant) {
        if !self.spawned && self.finishing.is_none() {
            self.finish_result = result;
            self.finish_winners = winners;
            self.finish_decided = decided;
        }
        self.begin_finish(now);
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
        let payload = self.finish_payload(&match_id, now);
        self.command("finish", payload);
    }

    /// Keep the network report of the lobby match whose transport just
    /// returned, for its `finish`. A launcher's match reports in its own
    /// result (`spawn_result.json`), and a finish already sent stays as it was.
    fn keep_network_report(&mut self, report: Value) {
        if !self.spawned && self.match_id.is_some() && self.finishing.is_none() {
            self.network_quality = Some(report);
        }
    }

    /// The lobby's `finish` is `{match_id}`, with `back_to_room` when the
    /// player returns to the room it kept and `network_quality` when the match
    /// played; a spawned match adds its local result for the launcher
    /// (docs/spawn.md), and a lobby match that ended its local result,
    /// `winners` and `ended_ago`, the whole seconds since its result was
    /// decided at `now`, for the ranked ladder (docs/lobby-protocol.md).
    fn finish_payload(&self, match_id: &str, now: Instant) -> Value {
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

    fn finish_completed(&mut self, id: u64) {
        if self
            .finishing
            .as_ref()
            .is_some_and(|finish| finish.request_id == id)
        {
            self.finishing = None;
            self.match_id = None;
            self.network_quality = None;
            self.finish_result = None;
            self.finish_winners = None;
            self.finish_decided = None;
            self.left_for_match = false;
        }
    }

    /// True when the lobby refused `back_to_room` (a lobby from before rooms
    /// came back): the finish retries without it and the room is not kept.
    fn finish_failed(&mut self, id: u64, failure: http::CommandFailure, now: Instant) -> bool {
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
        let rejected = matches!(failure, http::CommandFailure::Rejected(_));
        // A lobby from before leaving was timed by the game refuses
        // `ended_ago`, one from before team ladders `winners`, and one from
        // before match reports `network_quality`: the finish goes again
        // without the newest, and then as before.
        if rejected && self.finish_decided.take().is_some() {
            return false;
        }
        if rejected && self.finish_winners.take().is_some() {
            return false;
        }
        if rejected && self.network_quality.take().is_some() {
            return false;
        }
        let refused = self.back_to_room && rejected;
        if refused {
            self.back_to_room = false;
        }
        refused
    }

    /// A room leave the shell asked for; one during a started match is noted
    /// for its `channel_left` ([`Self::room_closed`]).
    fn leave_room(&mut self, now: Instant) {
        self.left_for_match |= self.match_id.is_some();
        self.begin_leave(now);
    }

    /// A `channel_left` closed the room under this client, rather than
    /// answering its own leave for its started match.
    fn room_closed(&mut self) -> bool {
        !std::mem::take(&mut self.left_for_match)
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
        self.left_for_match = false;
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
            self.send_command(id, "finish", self.finish_payload(match_id, now));
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
        // The old bearer's logout may retry for a while in an outage. Answer
        // the requests that need no lobby, so the player reaches the login
        // page and its connect wait rather than a blank screen; keep the rest
        // queued until that cleanup is retired.
        let requests = state
            .frontend
            .wol
            .as_mut()
            .map(|visit| visit.take_service_requests())
            .unwrap_or_default();
        let mut kept = Vec::new();
        for request in requests {
            if !answer_local_request(state, &request) {
                kept.push(request);
            }
        }
        if let Some(visit) = state.frontend.wol.as_mut() {
            visit.requests.splice(0..0, kept);
        }
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
            let publishes_options = requests.iter().any(|request| {
                matches!(
                    request,
                    NetworkSetupRequest::PublishOptions | NetworkSetupRequest::PublishRoster
                )
            });
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
                // The options this batch publishes already carry the C0.
                if (prepares_start || publishes_options)
                    && matches!(request, NetworkSetupRequest::PublishCooperativeCampaign(_))
                {
                    continue;
                }
                let starts = matches!(request, NetworkSetupRequest::StartGame { .. });
                if request_setup(state, &mut service, request).is_err() {
                    let text = tr("VERA:OnlineRequestMismatch");
                    if starts {
                        service.start_preparation = None;
                        fail_start(state, &text);
                    } else {
                        notice(state, &text);
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
            notice(state, &tr("VERA:ChoicesNotAcknowledged"));
        }
        if poll_start_preparation(state, &mut service).is_err() {
            service.start_preparation = None;
            fail_start(state, &tr("VERA:SettingsNotAgreed"));
        }
        if poll_relay(state, &mut service).is_err() {
            cancel_pending(state, &mut service);
            fail_start(state, &tr("VERA:RelayConnectionFailed"));
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
        show_problem_line(state);
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

/// The chat a returning player lands in: the kept room once the lobby gives
/// it back, the chat lobby or the Quick Match page. Anywhere else, such as a
/// sign-in after a lost connection, the line keeps waiting.
fn problem_chat_open(
    route: crate::app::shell_route::ShellRoute,
    setup_open: bool,
    quick_match_open: bool,
    lobby_open: bool,
    awaiting_room: bool,
) -> bool {
    match route {
        crate::app::shell_route::ShellRoute::NetworkSetup => setup_open,
        route if route.wol() => quick_match_open,
        route => route.wol_lobby() && lobby_open && !awaiting_room,
    }
}

fn show_problem_line(state: &mut AppState) {
    let open = problem_chat_open(
        state.frontend.shell_route,
        state.frontend.multiplayer_setup.is_some(),
        super::quick_match::page_open(state),
        state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| visit.lobby().is_some()),
        super::lobby::awaiting_room(state),
    );
    if !open {
        return;
    }
    if let Some(text) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.take_problem_line())
    {
        notice(state, &text);
    }
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

/// The report of the online match whose transport just returned, for that
/// match's `finish` (docs/lobby-protocol.md).
pub(crate) fn keep_network_report(
    state: &mut AppState,
    report: &crate::app::match_runtime::network_quality::NetworkQualityReport,
) {
    if let Some(service) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.live_service.as_mut())
        && let Ok(report) = serde_json::to_value(report)
    {
        service.keep_network_report(report);
    }
}

pub(super) fn match_returned(state: &mut AppState) {
    let report = match_report(state);
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.match_returned(report, Instant::now());
    }
}

/// A lobby match's `finish` result and `winners`, as
/// `App::record_match_result` decided them, and when it did.
type MatchReport = (Option<&'static str>, Option<Vec<String>>, Option<Instant>);

fn match_report(state: &AppState) -> MatchReport {
    (
        crate::app::App::match_result(state).map(crate::app::cncnet::ExternalMatchResult::as_str),
        crate::app::App::match_winners(state).map(<[String]>::to_vec),
        crate::app::App::match_result_at(state),
    )
}

impl super::WolVisit {
    fn take_problem_line(&mut self) -> Option<String> {
        self.problem_line.take()
    }

    fn match_returned(&mut self, report: MatchReport, now: Instant) {
        if self.provider != WolProvider::Online {
            return;
        }
        if let Some(service) = self.live_service.as_mut() {
            service.back_to_room = self.back_to_room;
            service.match_returned(report, now);
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

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
/// The game is closing: hand the online visit to the ordinary cleanup owner
/// (finish, leave, logout and the relay departure), as Back does, and end
/// the visit so nothing but that cleanup runs until exit.
pub(crate) fn retire_visit_for_exit(state: &mut AppState) {
    if state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.provider == WolProvider::Online)
    {
        leave_visit(state);
        state.frontend.wol = None;
    }
}

/// Answer a request that needs no lobby; false for any other.
fn answer_local_request(state: &mut AppState, request: &WolServiceRequest) -> bool {
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
        _ => return false,
    }
    true
}

/// The configured lobby's origin, as [`LiveService::open`] takes it without an
/// external launch.
fn configured_lobby() -> Option<String> {
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    let lobby = lobby_base_url().ok()?;
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    let lobby = DEFAULT_LOBBY_URL.to_owned();
    Some(lobby.trim_end_matches('/').to_owned())
}

/// The name this computer signs in to without its password on this visit's
/// lobby (docs/lobby-protocol.md "Accounts").
pub(super) fn remembered_name(state: &AppState) -> Option<String> {
    let dir = state.platform.game_config.as_ref()?.paths.settings_dir();
    let lobby = match state.frontend.wol.as_ref()?.live_service.as_ref() {
        Some(service) => service.lobby.clone(),
        None => configured_lobby()?,
    };
    account::load(dir, &lobby).map(|remembered| remembered.name)
}

/// The folder of VERA.INI, which holds `Account.json`.
fn settings_dir(state: &AppState) -> Option<&std::path::Path> {
    state
        .platform
        .game_config
        .as_ref()
        .map(|config| config.paths.settings_dir())
}

/// How this sign-in proves its name: the typed password, else, while
/// Remember password stays ticked, the sign-in this computer remembers for
/// the name, else nothing (a guest). The remembered name's password sign-in
/// forgets the old sign-in here and at the lobby; another name's sign-in
/// leaves it.
fn sign_in_credential(
    settings: Option<&std::path::Path>,
    service: &LiveService,
    nickname: &str,
    password: String,
    remember: bool,
) -> http::Credential {
    let remembered = settings
        .and_then(|dir| account::load(dir, &service.lobby))
        .filter(|remembered| remembered.name.eq_ignore_ascii_case(nickname));
    if let Some(remembered) = remembered {
        if password.is_empty() && remember {
            return http::Credential::Remembered(remembered.sign_in_token);
        }
        if !password.is_empty() {
            forget_sign_in(settings, service);
        }
    }
    if password.is_empty() {
        http::Credential::Guest
    } else {
        http::Credential::Password { password, remember }
    }
}

/// Drop this computer's remembered sign-in, here and at the lobby.
fn forget_sign_in(settings: Option<&std::path::Path>, service: &LiveService) {
    let Some(dir) = settings else {
        return;
    };
    match account::forget(dir, &service.lobby) {
        Ok(Some(remembered)) => {
            let _ = service.requests.send(http::Request::ForgetSignIn {
                name: remembered.name,
                sign_in_token: remembered.sign_in_token,
            });
        }
        Ok(None) => {}
        Err(error) => log::warn!("Could not forget the remembered sign-in: {error}"),
    }
}

fn send_account_request(service: &mut LiveService, request: AccountRequest) {
    service.account_attempt = service.account_attempt.wrapping_add(1);
    if let AccountRequest::Delete { name, .. } = &request {
        service.deleting = Some((service.account_attempt, name.clone()));
    }
    let _ = service.requests.send(http::Request::Account {
        attempt: service.account_attempt,
        request,
    });
}

/// The lobby answered account request `attempt`. A deletion it accepted
/// took the account's remembered sign-ins with it, so this computer's
/// sign-in of that name goes too, even when its popup closed meanwhile;
/// another account's stays.
fn account_answered(
    settings: Option<&std::path::Path>,
    service: &mut LiveService,
    attempt: u64,
    answer: &Result<(), String>,
) {
    let Some((_, name)) = service.deleting.take_if(|(sent, _)| *sent == attempt) else {
        return;
    };
    let Some(dir) = settings.filter(|_| answer.is_ok()) else {
        return;
    };
    if account::load(dir, &service.lobby)
        .is_some_and(|remembered| remembered.name.eq_ignore_ascii_case(&name))
        && let Err(error) = account::forget(dir, &service.lobby)
    {
        log::warn!("Could not forget the deleted account's sign-in: {error}");
    }
}

/// VERA: keep the name the lobby accepted as `[VERA] OnlineName=`, so the
/// next visit's login starts with it
/// ([`crate::ui::wol_login::WolLoginState::open_online`]).
fn remember_online_name(state: &mut AppState, nickname: &str) {
    let profile = &mut state.persistence.options_profile;
    if profile.online_name == nickname {
        return;
    }
    profile.online_name = nickname.to_owned();
    if let Some(config) = state.platform.game_config.as_ref()
        && let Err(error) = profile.commit_online_name(config.paths.settings_dir())
    {
        log::warn!("Could not save the online name: {error}");
    }
}

/// Keep the sign-in the lobby gave a password sign-in with Remember password,
/// in place of another account's, which the lobby then forgets.
fn remember_sign_in(
    settings: Option<&std::path::Path>,
    service: &LiveService,
    name: &str,
    token: String,
) {
    let Some(dir) = settings else {
        return;
    };
    let remembered = account::Remembered {
        name: name.to_owned(),
        sign_in_token: token,
    };
    if !account::valid_token(&remembered.sign_in_token) {
        log::warn!("The lobby's remembered sign-in for {name} is malformed");
        return;
    }
    forget_sign_in(settings, service);
    if let Err(error) = account::save(dir, &service.lobby, remembered) {
        log::warn!("Could not remember the sign-in of {name}: {error}");
    }
}

fn request_service(state: &mut AppState, service: &mut LiveService, request: WolServiceRequest) {
    if answer_local_request(state, &request) {
        return;
    }
    match request {
        WolServiceRequest::EnsureApi
        | WolServiceRequest::ListCredentials
        | WolServiceRequest::FetchServers { .. } => {}
        WolServiceRequest::Connect {
            nickname,
            password,
            remember,
            ..
        } => {
            service.clear_cleanup();
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            let credential =
                sign_in_credential(settings_dir(state), service, &nickname, password, remember);
            service.signing_in_remembered = matches!(credential, http::Credential::Remembered(_));
            let _ = service.requests.send(http::Request::OpenSession {
                attempt: service.auth_attempt,
                nickname,
                credential,
            });
        }
        WolServiceRequest::Account(request) => send_account_request(service, request),
        WolServiceRequest::ForgetSignIn => forget_sign_in(settings_dir(state), service),
        WolServiceRequest::ListChannels { kind, .. } => {
            let _ = service.requests.send(http::Request::Channels(kind));
        }
        WolServiceRequest::JoinChannel { channel, key } => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command("join", json!({"channel":channel.name,"key":key}));
            }
        }
        WolServiceRequest::CreateChannel {
            channel,
            key,
            ranked,
        } => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command("create", create_payload(channel, key, ranked));
            }
        }
        WolServiceRequest::LeaveChannel => {
            maps::cancel(state, service);
            cancel_pending(state, service);
            service.leave_room(Instant::now());
        }
        WolServiceRequest::MatchReturned => {
            service.back_to_room = state
                .frontend
                .wol
                .as_ref()
                .is_some_and(|visit| visit.back_to_room);
            service.match_returned(match_report(state), Instant::now());
        }
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
        WolServiceRequest::LocaleCount => service_event(
            state,
            WolServiceEvent::LocaleCount(crate::ui::wol_persona::LOCALE_COUNT),
        ),
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
        WolServiceRequest::LadderRecords(names) => service.look_up_ladder(names, false),
        WolServiceRequest::HighScores(names) => service.look_up_ladder(names, true),
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
        WolServiceRequest::RequestMatch { matchbot, request } => {
            request_match(service, matchbot, &request)
        }
        WolServiceRequest::Translate { text, line } => {
            if let Some(target) = state.persistence.options_profile.chat_translation.language {
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

/// `create`'s payload. Only an unranked room says so, as a lobby from before
/// the choice refuses the field.
fn create_payload(channel: crate::ui::wol_lobby::WolChannel, key: String, ranked: bool) -> Value {
    let mut payload = json!({"channel":wire::Channel::from(channel),"key":key});
    if !ranked {
        payload["ranked"] = json!(false);
    }
    payload
}

/// API+0x30's request: ordinary private chat to the matchbot. This game
/// has no transport fields to report, so RAT, ADR, NAT and PRT are 0.
fn request_match(
    service: &mut LiveService,
    matchbot: String,
    request: &super::quick_match::MatchRequest,
) {
    let text = request.text(super::quick_match::MatchTransport::default());
    send_chat(
        service,
        WolChatLine::Say(text.encode_utf16().collect()),
        vec![matchbot],
    );
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
        cooperative_campaign: cooperative_campaign(state),
        seed: Some(crate::app::shell_network_setup::resident_seed(state)),
    })
}

/// The host's current `C0` text for its options, only in Cooperative: the
/// record `+0x60` would send each guest at that moment.
fn cooperative_campaign(state: &AppState) -> Option<String> {
    let cooperative = state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| launch.mode.class == crate::skirmish_modes::MpModeClass::Cooperative);
    cooperative
        .then(|| {
            state
                .frontend
                .multiplayer_shell_runtime
                .cooperative_campaign_text()
        })
        .flatten()
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

/// Whether the accepted options hold a player's requested choices in
/// whatever row the host seated it: the host's `choices` takes no row, and a
/// guest's first choices leave before its seat is known. With Teams set, the
/// host's game puts every seated player on a team itself
/// (`MatchSession::admit_network_team`), so it answers with whatever team
/// it gave.
fn accepted_choices_match(accepted: &Value, human: &Value) -> bool {
    let any_team = !accepted["launch"]["options"]["teams"].is_null();
    let seatless = |human: &Value| {
        let mut human = human.clone();
        if let Some(fields) = human.as_object_mut() {
            fields.remove("raw_slot_index");
            if any_team && let Some(slot) = fields.get_mut("slot").and_then(Value::as_object_mut) {
                slot.remove("team");
            }
        }
        human
    };
    let expected = seatless(human);
    accepted["launch"]["humans"]
        .as_array()
        .is_some_and(|humans| {
            humans
                .iter()
                .any(|candidate| seatless(candidate) == expected)
        })
}

fn restore_accepted_options(state: &mut AppState, service: &LiveService) {
    let Some(accepted) = service.accepted_options.clone() else {
        return;
    };
    let result = decode::<wire::Options>(accepted)
        .and_then(|options| options.into_setup_events(&state.frontend.wol_account.nickname))
        .and_then(|events| {
            events
                .into_iter()
                .try_for_each(|event| crate::app::shell_network_setup::service_event(state, event))
        });
    if result.is_err() {
        notice(state, &tr("VERA:HostSettingsUnavailable"));
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
        fail_start(state, &tr("VERA:PlayersNotReadyInTime"));
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
            human.slot = slot.clone();
            human.set_observer(observer);
            let expected_human = serde_json::to_value(&human)?;
            human.slot.country_random = true;
            let expected_random = serde_json::to_value(human)?;
            let request_id = service.command_with_id(
                "choices",
                json!({"row":row,"slot":slot,"observer":observer}),
            );
            // A Quick Match host drops `R` (`0x007A8260`), so no options
            // answer it.
            let answered = !crate::app::shell_network_setup::quick_match(state);
            service.pending_choices = (answered
                && !switch
                && !service
                    .accepted_options
                    .as_ref()
                    .is_some_and(|accepted| accepted_choices_match(accepted, &expected_human)))
            .then_some(PendingChoices {
                request_id,
                expected_human,
                expected_random,
                deadline: Instant::now() + RELAY_TIMEOUT,
            });
        }
        NetworkSetupRequest::Ready(requested) => {
            let ready = requested && local_map_status(state, service) == 1;
            let index = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow!("no setup"))?
                .local_player_index;
            let withdrawn = state.match_state.session.network_player_ready(index);
            if requested && !ready {
                // The guest's automatic Accept waits for the verified map
                // instead of repeating this request every frame.
                crate::app::shell_network_setup::refuse_local_acceptance(state)?;
                if !withdrawn {
                    // Only the Quick Match guest's accept (`0x007A7F72`)
                    // leaves its node unaccepted. It withdraws nothing, so
                    // nothing is sent: the host starts on the guest's first
                    // `ready` (`0x0077EA53`) and drops the later ones.
                    return Ok(());
                }
            } else if !ready {
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
                    users: StartPreparation::users_in_human_order(
                        &state.match_state.session,
                        users,
                    ),
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
                notice(state, &tr("VERA:ConfirmingFinalSettings"));
            }
        }
        NetworkSetupRequest::Leave => {
            maps::cancel(state, service);
            cancel_pending(state, service);
            service.leave_room(Instant::now());
        }
        // Older lobbies take no keep_out, so only a host's own kick sends it.
        NetworkSetupRequest::Kick { name, keep_out } if keep_out => {
            service.command("kick", json!({"name":name,"keep_out":true}))
        }
        NetworkSetupRequest::Kick { name, .. } => service.command("kick", json!({"name":name})),
        NetworkSetupRequest::Advertise => {
            if let Some(channel) = super::lobby::republish_channel(state) {
                service.command("advertise", json!({"channel":wire::Channel::from(channel)}));
            }
        }
        NetworkSetupRequest::RequestPings => service.command("ping", json!({})),
        // VERA's room: the host's Balance is `/balance` as typed, Balance
        // automatically the lobby's quiet `balance` command
        // (docs/lobby-protocol.md).
        NetworkSetupRequest::Balance { automatic: true } => service.command("balance", json!({})),
        NetworkSetupRequest::Balance { automatic: false } => {
            service.command("chat", json!({"text":"/balance","to":[],"action":false}))
        }
        NetworkSetupRequest::WolChat(line) => send_chat(service, line, Vec::new()),
        NetworkSetupRequest::WolEmote(text) => service.command(
            "chat",
            json!({"text":String::from_utf16_lossy(&text),"to":[],"action":true}),
        ),
        NetworkSetupRequest::Chat(text) => send_chat(service, WolChatLine::Say(text), Vec::new()),
        NetworkSetupRequest::PublishStartStaging(_) => {
            service.command("options", json!({"options":options(state)?}));
        }
        // +0x60's C0 rides in the options (`cooperative_campaign`), so its
        // publish is one of them; the loop drops it when its batch already
        // publishes options or starts.
        NetworkSetupRequest::PublishCooperativeCampaign(_) => {
            service.command("options", json!({"options":options(state)?}));
        }
        // RESIDUAL: mode+64's C1 (0x005C2710) is not carried, as on LAN:
        // VERA reads no round assignment after the publish, so guests keep
        // their own.
        NetworkSetupRequest::PublishCooperativeAssignments(_) => {}
        NetworkSetupRequest::RequestPreview
        | NetworkSetupRequest::OfferPreview
        | NetworkSetupRequest::AcknowledgePreview
        | NetworkSetupRequest::RequestPreviewFile
        | NetworkSetupRequest::SendPreviewFile
        | NetworkSetupRequest::RequestRandomMapImage => {}
        NetworkSetupRequest::RequestAcceptance
        | NetworkSetupRequest::BroadcastPings
        | NetworkSetupRequest::ObserverSlot { .. } => {}
        _ => notice(state, &tr("VERA:SetupOperationUnavailable")),
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
                &tr(if retryable {
                    "VERA:StartLobbyBusy"
                } else {
                    "VERA:StartRefusedSetup"
                }),
            );
        }
        http::Response::Authenticated {
            attempt,
            player_id,
            nickname,
            motd,
            sign_in_token,
        } if attempt == service.auth_attempt => {
            if validate_online_identity(&player_id, &nickname).is_err() {
                state.frontend.wol_account.password.clear();
                service_event(state, WolServiceEvent::Login(Err(0x8000_4005)));
                return;
            }
            if let Some(token) = sign_in_token {
                remember_sign_in(settings_dir(state), service, &nickname, token);
            }
            remember_online_name(state, &nickname);
            service.player_id = player_id;
            state.frontend.wol_account.nickname = nickname;
            state.frontend.wol_account.password.clear();
            // The operator's welcome is the only one: the game has none of its own.
            let motd = motd
                .filter(|motd| !motd.trim().is_empty())
                .unwrap_or_default();
            service_event(state, WolServiceEvent::Login(Ok(WolLoginAccepted { motd })));
        }
        http::Response::LoginFailed { attempt, code } if attempt == service.auth_attempt => {
            state.frontend.wol_account.password.clear();
            service_event(state, WolServiceEvent::Login(Err(code)));
            // Code 0 is no answer or an unexpected one: TXT_CANT_CONNECT.
            if code == 0 {
                super::service::replace_open_box_body(state, &tr(CANT_CONNECT_TEXT));
            }
        }
        http::Response::SignInRefused {
            attempt,
            message,
            bad_credential,
        } if attempt == service.auth_attempt => {
            if bad_credential && service.signing_in_remembered {
                forget_sign_in(settings_dir(state), service);
            }
            state.frontend.wol_account.password.clear();
            service_event(state, WolServiceEvent::Login(Err(0)));
            if !super::service::replace_open_box_body(state, &message) {
                notice(state, &message);
            }
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
                notice(state, &tr("VERA:InvalidLobbyUpdate"));
            }
        }
        http::Response::CommandCompleted { id, operation } => match operation {
            "finish" => service.finish_completed(id),
            "leave" => service.leave_completed(id),
            "logout" => service.logout_completed(id),
            "relay_ready" | "latency_ready" => service.report_completed(id),
            "balance" => crate::app::shell_network_setup::balance_answered(state),
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
                fail_start(state, &tr("VERA:SettingsNotPublished"));
            }
            "options" if failure == http::CommandFailure::InvalidOptions => {
                // A lobby from before Teams, Balance automatically or the
                // join queue refuses the field: the room goes on without it
                // and publishes its options again.
                let text = crate::app::shell_network_setup::lobby_refused_option(state)
                    .unwrap_or("VERA:OnlineRequestRefused");
                notice(state, &tr(text));
            }
            // The room no longer balances itself, or the lobby refused it:
            // the next change of who plays asks again.
            "balance" => crate::app::shell_network_setup::balance_answered(state),
            "choices"
                if service
                    .pending_choices
                    .as_ref()
                    .is_some_and(|choices| choices.request_id == id) =>
            {
                service.pending_choices = None;
                restore_accepted_options(state, service);
                notice(state, &tr("VERA:ChoicesRefused"));
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
                if service.finish_failed(id, failure, Instant::now()) {
                    // No room comes back: the waiting runner goes to the lobby.
                    super::lobby::room_refused(state);
                } else if first_failure && failure != http::CommandFailure::AlreadyFinished {
                    notice(state, &tr("VERA:FinishingPreviousMatch"));
                }
            }
            // The host's 5 s ping; an outage is reported once by the poll.
            "ping" => {}
            // A refused ladder lookup leaves the tables as they were; the
            // high-score sink only logs a failure (`0x007A3B30`).
            "ladder" => {}
            "relay_ready" | "latency_ready" => {
                if !service.report_failed(id, failure, Instant::now()) {
                    notice(state, &tr("VERA:OnlineRequestRefused"));
                }
            }
            _ => notice(state, &tr("VERA:OnlineRequestRefused")),
        },
        http::Response::Translated { id, outcome } => {
            if let Some((line, reply)) = service.translations.reply(id, outcome, Instant::now()) {
                super::translation::show(state, line, &reply);
            }
        }
        http::Response::AccountAnswered { attempt, answer } => {
            account_answered(settings_dir(state), service, attempt, &answer);
            if attempt == service.account_attempt {
                service_event(state, WolServiceEvent::Account(answer));
            }
        }
        http::Response::Failure(label) => notice(state, &tr(label)),
        http::Response::LobbyUnreachable => notice(state, &tr("VERA:OnlineNotResponding")),
        http::Response::LobbyReachable => notice(state, &tr("VERA:OnlineRespondingAgain")),
        http::Response::LobbyLost => {
            // A match keeps its relay through a lobby outage; this asks
            // again after it.
            if super::in_match(state) || crate::app::loading::pump::network_loading_active(state) {
                return;
            }
            // The worker keeps the bearer and logs it out at the next
            // sign-in, so the name is free again.
            let _ = service.requests.send(http::Request::Abandon);
            if !state
                .frontend
                .wol
                .as_ref()
                .is_some_and(|visit| visit.connected)
            {
                return;
            }
            maps::cancel(state, service);
            cancel_pending(state, service);
            // The native disconnect: a box over the lobby, or none where the
            // runner has no lobby (Quick Match), which then gets one at the
            // welcome.
            service_event(state, WolServiceEvent::NetStatus(super::NET_STATUS_LOST));
            let text = tr(LOBBY_LOST_TEXT);
            if !super::lobby::replace_box_body(state, &text)
                && !super::service::show_text_box(state, &text)
            {
                notice(state, &text);
            }
        }
        http::Response::Offline => {
            crate::app::loading::pump::cancel_network_loading(
                state,
                &tr("VERA:OnlineSessionEndedWhileLoading"),
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
        | http::Response::UpgradeRequired { .. }
        | http::Response::SignInRefused { .. } => {}
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| anyhow!("invalid lobby message"))
}

/// A `chat` event's payload.
#[derive(Deserialize)]
struct Chat {
    from: wire::Member,
    text: String,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    action: bool,
    // The matchbot's replies; its `Start` carries the pairing.
    #[serde(default)]
    ansi: bool,
    #[serde(default)]
    quick_match: Option<super::quick_match::MatchStart>,
    #[serde(default)]
    sent_at: Option<u64>,
    /// The lobby's own lines as phrases ([`super::lobby_phrases`]), read
    /// only once the event decodes, so that a shape this game does not know
    /// leaves the line in `text`.
    #[serde(default)]
    phrases: Value,
}

impl Chat {
    /// The line in the language in use when the lobby said it as phrases
    /// that VERA's language files hold, else the lobby's `text`. A matchbot
    /// reply's phrases say what follows its first word, the reply's kind,
    /// which its Quick Match runner reads (`Info`).
    fn shown_text(&self) -> String {
        let Some(said) =
            serde_json::from_value::<Vec<super::lobby_phrases::Phrase>>(self.phrases.clone())
                .ok()
                .and_then(|phrases| super::lobby_phrases::localized(&phrases))
        else {
            return self.text.clone();
        };
        match self.text.split_once(' ') {
            Some((kind, _)) if self.ansi => format!("{kind} {said}"),
            _ => said,
        }
    }

    /// The pairing of the matchbot's `Start` that `local` hosts; only its
    /// ANSI text carries one.
    fn hosted_pairing(&self, local: &str) -> Option<super::quick_match::MatchStart> {
        let pairing = self.quick_match.clone().filter(|_| self.ansi)?;
        super::quick_match::hosted_pairing(local, &self.text, self.private, pairing)
    }
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
            // Base64 of a 66-byte VO1C or a 67-byte VO2C certificate.
            && matches!(grant.ordering_certificate.len(), 88 | 90),
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
        "joined" | "created" | "room_reopened" => {
            let joined: wire::Joined = decode(payload)?;
            service.revision = 0;
            service.accepted_options = None;
            service.last_ready_request = None;
            service.pending_choices = None;
            service.start_preparation = None;
            service.left_for_match = false;
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
            let joined = super::lobby::WolJoined {
                channel: joined.channel.into(),
                members,
            };
            let event = match kind {
                "created" => WolServiceEvent::Created(Ok(joined.members)),
                "room_reopened" => WolServiceEvent::RoomReopened(joined),
                _ => WolServiceEvent::Joined(Ok(joined)),
            };
            service_event(state, event);
        }
        "channel_left" => {
            if service.room_closed() {
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
                    &tr("VERA:OnlineRoomClosedWhileLoading"),
                );
            }
            service.accepted_options = None;
            service.last_ready_request = None;
            service.pending_choices = None;
            maps::cancel(state, service);
            cancel_pending(state, service);
            if let Some(visit) = state.frontend.wol.as_mut()
                && visit.in_game
            {
                // The kept room closed during the match: nothing to return to.
                visit.back_to_room = false;
            }
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
                    &tr("VERA:OnlinePlayerLeftWhileLoading"),
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
                fail_start(state, &tr("VERA:SettingsChangedWhileStarting"));
            }
            service.revision = p.revision;
            if service
                .pending_choices
                .as_ref()
                .is_some_and(|choices| choices.answered_by(&canonical))
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
                let events = p.options.into_setup_events(&local).inspect_err(|_| {
                    log::warn!("Online host options had no admitted local player");
                })?;
                for event in events {
                    crate::app::shell_network_setup::service_event(state, event).inspect_err(
                        |_| {
                            log::warn!("Online host setup was not admitted");
                        },
                    )?;
                }
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
        // The lobby's answer to the host's `/balance` (docs/lobby-protocol.md).
        "balance" => {
            #[derive(Deserialize)]
            struct Team {
                name: String,
                team: u8,
            }
            #[derive(Deserialize)]
            struct Payload {
                teams: Vec<Team>,
            }
            let p: Payload = decode(payload)?;
            let teams: Vec<_> = p
                .teams
                .into_iter()
                .map(|team| (team.name, team.team))
                .collect();
            if let Err(error) = crate::app::shell_network_setup::balance_teams(state, &teams) {
                log::warn!("lobby balance: {error:#}");
            }
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
            let p: Chat = decode(payload)?;
            let options = p
                .hosted_pairing(&local)
                .and_then(|pairing| super::quick_match::online_start_options(state, pairing));
            let text = p.shown_text().encode_utf16().collect();
            service_event(
                state,
                WolServiceEvent::Message {
                    from: p.from.into_member(&local),
                    text,
                    private: p.private,
                    action: p.action,
                    ansi: p.ansi,
                    options,
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
        "ladder" => {
            let ladder = decode::<wire::Ladder>(payload)?;
            let (records, high_scores) = ladder.tables();
            service_event(state, WolServiceEvent::LadderSeason(ladder.season));
            service_event(state, WolServiceEvent::Ladder(records));
            service_event(state, WolServiceEvent::HighScores(Ok(high_scores)));
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
                #[serde(default)]
                ranked: bool,
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
            if timing.ranked {
                log::info!("Online match {} is ranked", timing.match_id);
            }
            enter_timed_start(state, service, timing.match_id, &timing.pings)?;
        }
        "start_rejected" => {
            cancel_pending(state, service);
            fail_start(state, &tr("VERA:StartRefusedConnection"));
        }
        "error" => notice(state, &tr("VERA:OnlineServiceRefused")),
        _ => {}
    }
    Ok(())
}

/// The agreed timing is known and the current grant is fresh: leave the online
/// barriers for the loading that follows `Negotiated`.
fn enter_timed_start(
    state: &mut AppState,
    service: &mut LiveService,
    match_id: String,
    pings: &[i32],
) -> Result<()> {
    service.relay_granted(crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32);
    // A new match: no earlier one's result may stand for it.
    crate::app::App::clear_match_result(state);
    crate::app::lan_service::apply_online_timing(state, pings)?;
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
    service.match_id = Some(match_id);
    service.pending = None;
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
            resume_pings: None,
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
    /// A spawn launch returning to a match under way: its agreed timing.
    resume_pings: Option<Vec<i32>>,
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
        resume_pings: start.resume_pings,
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
    let (resume_pings, resume_cursor) = relay.resume.map_or((None, None), |resume| {
        (Some(resume.pings), Some(resume.cursor))
    });
    // The launcher's bearer replaces `/v1/sessions`; nothing else is opened.
    service
        .requests
        .send(http::Request::AdoptSession {
            token: relay.token,
            resume: resume_cursor.map(|cursor| (relay.match_id.clone(), cursor)),
        })
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
            resume_pings,
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
    let result = crate::app::App::match_result(state)
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
            if let Some(pings) = pending.resume_pings.take() {
                crate::app::lan_service::mark_online_restore(state)?;
                return enter_timed_start(state, service, match_id, &pings);
            }
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
    use crate::app::shell_wol::{WolHighScoreRecord, WolLadderRecord};

    /// The reason must not land in a transcript the player is about to lose.
    #[test]
    fn a_failed_matchs_reason_waits_for_the_chat_the_player_lands_in() {
        use crate::app::shell_route::ShellRoute;
        assert!(problem_chat_open(
            ShellRoute::WolLobby,
            false,
            false,
            true,
            false
        ));
        assert!(!problem_chat_open(
            ShellRoute::WolLobby,
            false,
            false,
            true,
            true
        ));
        assert!(problem_chat_open(
            ShellRoute::NetworkSetup,
            true,
            false,
            true,
            true
        ));
        assert!(problem_chat_open(
            ShellRoute::Wol,
            false,
            true,
            false,
            false
        ));
        assert!(!problem_chat_open(
            ShellRoute::Wol,
            false,
            false,
            false,
            false
        ));
    }

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
                    pacing: Default::default(),
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
        // A VO2C certificate carries the host's pacing switch.
        let vo2c = crate::net::udp::OrderingCertificate {
            room: [0x11; 16],
            sources_mask: 3,
            tick_hz: 45,
            expires: 86400,
            pacing: crate::net::udp::CertificatePacing::Off,
        }
        .sign(&signing_key)
        .unwrap();
        grant.ordering_certificate = encode(&vo2c);
        assert_eq!(
            decode_relay_grant(&grant, match_id, player_id)
                .unwrap()
                .ordering_certificate,
            vo2c
        );
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
            lobby: "https://lobby.test".into(),
            signing_in_remembered: false,
            account_attempt: 0,
            deleting: None,
        };
        (service, input, output)
    }

    /// The remembered sign-in signs its name in, survives another name's
    /// sign-in, and is forgotten here and at the lobby by its name's password
    /// sign-in or by another account's remembered sign-in.
    #[test]
    fn a_remembered_sign_in_lasts_until_its_name_or_another_account_replaces_it() {
        let directory = crate::map::source::test_support::TestDirectory::new("live-account");
        let settings = Some(directory.path());
        let (service, input, _output) = control_service(None);
        let token = |c: char| c.to_string().repeat(43);
        let forgotten = |input: &Receiver<http::Request>| match input.try_recv() {
            Ok(http::Request::ForgetSignIn { sign_in_token, .. }) => Some(sign_in_token),
            _ => None,
        };
        remember_sign_in(settings, &service, "Tanya", token('A'));
        assert!(matches!(
            sign_in_credential(settings, &service, "tanya", String::new(), true),
            http::Credential::Remembered(sent) if sent == token('A')
        ));
        assert!(matches!(
            sign_in_credential(settings, &service, "Boris", String::new(), false),
            http::Credential::Guest
        ));
        remember_sign_in(settings, &service, "Boris", token('B'));
        assert_eq!(forgotten(&input), Some(token('A')));
        assert!(matches!(
            sign_in_credential(settings, &service, "Boris", "s3cret-pw".into(), true),
            http::Credential::Password { remember: true, .. }
        ));
        assert_eq!(forgotten(&input), Some(token('B')));
        assert_eq!(account::load(directory.path(), &service.lobby), None);
    }

    #[test]
    fn a_deleted_account_forgets_its_sign_in_even_after_its_popup_closed() {
        let directory = crate::map::source::test_support::TestDirectory::new("live-delete");
        let settings = Some(directory.path());
        let (mut service, _input, _output) = control_service(None);
        remember_sign_in(settings, &service, "Tanya", "A".repeat(43));
        let delete = |name: &str| AccountRequest::Delete {
            name: name.into(),
            password: "s3cret-pw".into(),
        };
        send_account_request(&mut service, delete("Boris"));
        let boris = service.account_attempt;
        account_answered(settings, &mut service, boris, &Ok(()));
        assert!(
            account::load(directory.path(), &service.lobby).is_some(),
            "another account's sign-in stays"
        );
        send_account_request(&mut service, delete("tanya"));
        let tanya = service.account_attempt;
        // Cancelled, then Forgot Password sent: the deletion's answer still counts.
        send_account_request(
            &mut service,
            AccountRequest::Forgot {
                name: "Tanya".into(),
            },
        );
        account_answered(settings, &mut service, tanya, &Ok(()));
        assert_eq!(account::load(directory.path(), &service.lobby), None);
    }

    /// A lobby from before the Ranked game choice refuses the field, so a
    /// ticked box sends none.
    #[test]
    fn only_an_unranked_room_sends_ranked() {
        let channel = crate::ui::wol_lobby::WolChannel::default();
        assert_eq!(
            create_payload(channel.clone(), "k".into(), true).get("ranked"),
            None
        );
        assert_eq!(
            create_payload(channel, "k".into(), false)["ranked"],
            json!(false)
        );
    }

    #[test]
    fn a_match_request_is_private_chat_to_the_matchbot() {
        let (mut service, input, _output) = control_service(None);
        let request = super::super::quick_match::MatchRequest {
            country: -2,
            color: 3,
            match_by_resolution: false,
            locale: None,
            width: 1024,
        };
        request_match(&mut service, "matchbot".into(), &request);
        let http::Request::Command {
            operation, payload, ..
        } = input.try_recv().unwrap()
        else {
            panic!("chat command");
        };
        assert_eq!(
            (operation, payload),
            (
                "chat",
                json!({
                    "text": "Match COU=-2, COL=3, MBR=0, RAT=0, RES=1024, ADR=0, NAT=0, PRT=0",
                    "to": ["matchbot"],
                    "action": false,
                })
            )
        );
    }

    /// Only the matchbot's ANSI `Start` gives its host the pairing.
    #[test]
    fn a_chat_start_gives_its_host_the_pairing() {
        let chat = |ansi: bool| -> Chat {
            decode(json!({
                "from": {"player_id": "m", "name": "matchbot", "flags": 1},
                "text": "Start 7:Alpha,0,0,0,0,0,0,0,Beta,0,0,0,0,0,0,0",
                "private": true,
                "ansi": ansi,
                "quick_match": {
                    "seed": 7,
                    "host": {"name": "Alpha", "country": -2, "color": 3},
                    "guest": {"name": "Beta", "country": 5, "color": -2},
                },
            }))
            .unwrap()
        };
        let pairing = chat(true).hosted_pairing("Alpha").unwrap();
        assert_eq!((pairing.seed, pairing.guest.country), (7, 5));
        assert_eq!(chat(true).hosted_pairing("Beta"), None);
        assert_eq!(chat(false).hosted_pairing("Alpha"), None);
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
    fn a_match_finishes_with_its_result_and_a_lobby_match_with_its_winners() {
        // A lobby match reports for the ranked ladder. A lobby from before
        // team ladders refuses `winners`: the finish goes again without them.
        // Its score screen held the finish back for a minute and a half: the
        // lobby times its player's leaving by `ended_ago`.
        let (mut lobby, input, _output) = control_service(Some("lobby-match"));
        lobby.finish_result = Some("won");
        lobby.finish_winners = Some(vec!["Tanya".into(), "Boris".into()]);
        lobby.finish_decided = Instant::now().checked_sub(Duration::from_secs(90));
        assert_eq!(
            finish_payload_sent(&mut lobby, &input),
            json!({"match_id":"lobby-match","result":"won","winners":["Tanya","Boris"],"ended_ago":90})
        );
        // A lobby from before `ended_ago` refuses it, and one from before team
        // ladders `winners`: the finish goes again without the newest.
        let mut retried = |at: u64| {
            assert!(!lobby.finish_failed(
                5,
                http::CommandFailure::Rejected(0x8000_4005),
                Instant::now()
            ));
            lobby.poll_cleanup(Instant::now() + Duration::from_secs(at));
            let http::Request::Command { payload, .. } = input.try_recv().unwrap() else {
                panic!("finish retry");
            };
            payload
        };
        assert_eq!(
            retried(3),
            json!({"match_id":"lobby-match","result":"won","winners":["Tanya","Boris"]})
        );
        assert_eq!(
            retried(10),
            json!({"match_id":"lobby-match","result":"won"})
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
        assert_eq!(
            spawned.finish_payload("m", Instant::now())["opponents_left"],
            json!(true)
        );
    }

    #[test]
    fn one_ladder_command_answers_the_ladder_and_high_score_tables() {
        let (mut service, input, _output) = control_service(None);
        let names = |input: &Receiver<http::Request>| {
            let mut sent = Vec::new();
            while let Ok(http::Request::Command {
                operation, payload, ..
            }) = input.try_recv()
            {
                assert_eq!(operation, "ladder");
                sent.push(payload["names"].clone());
            }
            sent
        };
        service.look_up_ladder(vec!["tanya".into()], false);
        service.look_up_ladder(vec!["tanya".into()], true);
        service.look_up_ladder(vec!["boris".into()], true);
        assert_eq!(names(&input), [json!(["tanya"]), json!(["boris"])]);

        // Boris played in an earlier season only: no record, but a badge.
        let ladder: wire::Ladder = serde_json::from_value(json!({"season":2,"records":[
            {"name":"Tanya","points":49,"os":48.51,"rank":7,"wins":40,"losses":12,"disconnects":1}],
            "high_scores":[{"name":"Tanya","points":1501,"rank":7},{"name":"Boris","points":81,"rank":0}]}))
        .unwrap();
        let high_score = |name: &str, points, rank| WolHighScoreRecord {
            name: name.into(),
            points,
            rank,
            coop: 0,
        };
        let tanya = WolLadderRecord {
            name: "Tanya".into(),
            wins: 40,
            losses: 12,
            points: 49,
            os_hundredths: 4851,
            rank: 7,
            disconnects: 1,
        };
        assert_eq!(
            ladder.tables(),
            (
                vec![tanya.clone()],
                vec![high_score("Tanya", 1501, 7), high_score("Boris", 81, 0)],
            )
        );
        assert_eq!(ladder.tables().0[0].os_text(), "48.51");
        // A lobby from before `os` gives the rounded OS.
        let older: wire::Ladder = serde_json::from_value(json!({"season":2,"records":[
            {"name":"Tanya","points":49,"rank":7,"wins":40,"losses":12,"disconnects":1}]}))
        .unwrap();
        let rounded = WolLadderRecord {
            os_hundredths: 4900,
            ..tanya
        };
        assert_eq!(older.tables(), (vec![rounded], vec![]));
        let unseasoned: wire::Ladder =
            serde_json::from_value(json!({"season":null,"records":[]})).unwrap();
        assert_eq!(unseasoned.tables(), (vec![], vec![]));
    }

    #[test]
    fn a_played_lobby_match_sends_its_network_report_until_a_lobby_refuses_it() {
        let report = json!({"played_seconds": 600.0});
        let (mut lobby, input, _output) = control_service(Some("played-match"));
        lobby.keep_network_report(report.clone());
        assert_eq!(
            finish_payload_sent(&mut lobby, &input),
            json!({"match_id":"played-match","network_quality":report})
        );
        // A lobby from before match reports refuses the field: the finish
        // goes again without it.
        assert!(!lobby.finish_failed(
            5,
            http::CommandFailure::Rejected(0x8000_4005),
            Instant::now()
        ));
        lobby.poll_cleanup(Instant::now() + Duration::from_secs(3));
        let http::Request::Command { payload: retry, .. } = input.try_recv().unwrap() else {
            panic!("finish retry");
        };
        assert_eq!(retry, json!({"match_id":"played-match"}));

        // A launcher's match reports in spawn_result.json, not to its service.
        let (mut spawned, input, _output) = control_service(Some("spawned-match"));
        spawned.spawned = true;
        spawned.keep_network_report(report);
        assert!(
            finish_payload_sent(&mut spawned, &input)
                .get("network_quality")
                .is_none()
        );
    }

    #[test]
    fn a_finished_matchs_report_never_reaches_a_later_matchs_finish() {
        let (mut service, input, _output) = control_service(Some("first-match"));
        service.keep_network_report(json!({"played_seconds": 600.0}));
        assert!(
            finish_payload_sent(&mut service, &input)
                .get("network_quality")
                .is_some()
        );
        service.finish_completed(5);
        // The next start is cancelled before it plays.
        service.match_id = Some("cancelled-match".into());
        assert_eq!(
            finish_payload_sent(&mut service, &input),
            json!({"match_id":"cancelled-match"})
        );
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
            resume_pings: None,
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
        let shown = translations.reply(
            0,
            Ok(ChatReply {
                text: "冲".into(),
                source: None,
            }),
            now,
        );
        assert!(shown.is_some_and(|(shown, reply)| shown == line("A") && reply.text == "冲"));
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

    #[test]
    fn a_reply_that_only_repeats_its_line_shows_nothing() {
        use super::super::translation::TranslatedLine;
        use crate::ui::chat_translation::ChatLanguage;
        let (requests, _input) = mpsc::channel();
        let mut translations = Translations::default();
        let now = Instant::now();
        for (id, (original, reply, shown)) in [
            ("gg", "gg", false),
            ("GG!", "gg", false),
            ("2v2 go", "2V2, go.", false),
            ("gg wp", "gg 打得好", true),
            ("gg", "[从英语翻译] gg", false),
        ]
        .into_iter()
        .enumerate()
        {
            let text: Vec<u16> = original.encode_utf16().collect();
            let line = TranslatedLine::Match {
                name: "A".into(),
                color: 0,
            };
            translations.request(&requests, &text, ChatLanguage::SimplifiedChinese, line, now);
            let reply = ChatReply {
                text: reply.into(),
                source: None,
            };
            let reply = translations.reply(id as u64, Ok(reply), now);
            assert_eq!(reply.is_some(), shown, "{original}");
        }
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
    fn a_cooperative_campaign_reaches_the_guest_ahead_of_the_options_it_rides_in() {
        let launch = setup_session().network_setup().unwrap().clone();
        let mut value = json!({
            "launch": launch,
            "map_metadata": {"display_name": "Map", "player_capacity": 2, "official": true,
                "byte_length": 1, "digest": "d", "source_sha256": "11".repeat(32),
                "effective_digest": "aa".repeat(32)},
            "vacancies": vec!["open"; 30],
            "observer_row": null,
            "cooperative_difficulty": 1,
            "seed": null,
        });
        let plain: wire::Options = serde_json::from_value(value.clone()).unwrap();
        // Lobbies from before the field refuse it, so other modes leave it out.
        let sent = serde_json::to_value(&plain).unwrap();
        assert!(sent.get("cooperative_campaign").is_none());
        let events = plain.into_setup_events("Guest").unwrap();
        assert!(matches!(
            events.as_slice(),
            [NetworkSetupEvent::HostSetup(_)]
        ));
        // The native host sends its C0 ahead of the options (+0x94 at the
        // join accept 0x007A6E5A), so the guest applies it first.
        let text = "C0,0,0,0,0,0,0,0,0,0,0,0,1,2,-1,Host,Guest";
        value["cooperative_campaign"] = json!(text);
        let cooperative: wire::Options = serde_json::from_value(value).unwrap();
        let events = cooperative.into_setup_events("Guest").unwrap();
        assert!(matches!(
            events.as_slice(),
            [
                NetworkSetupEvent::CooperativeCampaign(campaign),
                NetworkSetupEvent::HostSetup(options),
            ] if campaign == text && options.launch.local_player_index == 1
        ));
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
    fn a_quick_match_host_starts_its_reverse_channel_order_unaccepted() {
        let mut session = setup_session();
        for index in 0..2 {
            session.set_network_map_status(index, 1).unwrap();
        }
        // The matchbot's options withdrew the host's own acceptance.
        session.set_network_player_ready(0, false).unwrap();
        session.set_network_player_ready(1, true).unwrap();
        let start = |users: [&str; 2]| StartPreparation {
            users: StartPreparation::users_in_human_order(
                &session,
                users.map(String::from).to_vec(),
            ),
            options: Value::Null,
            revision: Some(5),
            publication_request: None,
            submitted_request: None,
            deadline: Instant::now() + RELAY_TIMEOUT,
        };
        let reversed = start(["Guest", "Host"]);
        assert_eq!(reversed.users, ["Host", "Guest"]);
        assert!(reversed.ready(&session, 5));
        assert!(!start(["Guest", "Other"]).ready(&session, 5));
        session.set_network_player_ready(1, false).unwrap();
        assert!(!reversed.ready(&session, 5), "the guest still accepts");
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
    fn a_host_answering_with_random_settles_a_banned_country_choice() {
        let mut session = setup_session();
        let mut human = session.network_setup().unwrap().local_player().clone();
        human.slot.country_random = false;
        let expected_human = serde_json::to_value(&human).unwrap();
        human.slot.country_random = true;
        let pending = PendingChoices {
            request_id: 1,
            expected_human,
            expected_random: serde_json::to_value(&human).unwrap(),
            deadline: Instant::now() + RELAY_TIMEOUT,
        };
        let mut launch = session.network_setup().unwrap().clone();
        launch.local_player_mut().slot = human.slot.clone();
        launch.rebuild_house_roster();
        session
            .accept_network_setup(launch, true, None, false)
            .unwrap();
        assert!(pending.answered_by(&json!({"launch":session.network_setup()})));
        let mut launch = session.network_setup().unwrap().clone();
        launch.local_slot_mut().color_random = !launch.local_slot().color_random;
        assert!(!pending.answered_by(&json!({"launch":launch})));
    }

    #[test]
    fn a_guests_first_choices_are_answered_in_the_row_the_host_gave_it() {
        // The first choices leave before the host's options seat the guest,
        // so they name row 0; the host keeps the guest in its own row.
        let session = setup_session();
        let launch = session.network_setup().unwrap();
        let mut human = launch.humans[1].clone();
        human.raw_slot_index = Some(0);
        let expected_human = serde_json::to_value(&human).unwrap();
        human.slot.country_random = true;
        let pending = PendingChoices {
            request_id: 1,
            expected_human,
            expected_random: serde_json::to_value(&human).unwrap(),
            deadline: Instant::now() + RELAY_TIMEOUT,
        };
        assert!(pending.answered_by(&json!({"launch":launch})));
        let mut other = launch.clone();
        other.humans[1].slot.color_random = !other.humans[1].slot.color_random;
        assert!(
            !pending.answered_by(&json!({"launch":other})),
            "other choices still wait"
        );
    }

    #[test]
    fn a_room_with_teams_answers_choices_with_the_team_it_gave() {
        // A guest's first choices ask for no team; with Teams set the host
        // keeps the guest on the team it gave.
        use crate::skirmish_launch::LaunchTeam;
        let session = setup_session();
        let mut launch = session.network_setup().unwrap().clone();
        let mut human = launch.humans[1].clone();
        human.slot.team = LaunchTeam::None;
        let expected_human = serde_json::to_value(&human).unwrap();
        human.slot.country_random = true;
        let pending = PendingChoices {
            request_id: 1,
            expected_human,
            expected_random: serde_json::to_value(&human).unwrap(),
            deadline: Instant::now() + RELAY_TIMEOUT,
        };
        launch.humans[1].slot.team = LaunchTeam::Team(1);
        assert!(
            !pending.answered_by(&json!({"launch":launch})),
            "without Teams another team still waits"
        );
        launch.options.teams = Some(2);
        assert!(pending.answered_by(&json!({"launch":launch})));
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
        visit.match_returned((None, None, None), now);
        visit.match_returned((None, None, None), now); //Repeated abort callbacks keep one intent.
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
        visit.match_returned((None, None, None), now);
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
        visit.match_returned((None, None, None), now);
        assert!(
            input.try_recv().is_err(),
            "expired authority cannot issue finish"
        );
    }

    #[test]
    fn a_match_from_a_kept_room_asks_for_the_room_back_on_finish() {
        let (service, input, _output) = control_service(Some("room-match"));
        let mut visit = super::super::WolVisit::open(false);
        visit.provider = WolProvider::Online;
        visit.back_to_room = true;
        visit.live_service = Some(service);
        let now = Instant::now();
        visit.match_returned((None, None, None), now);
        let sent = |input: &Receiver<http::Request>| {
            let http::Request::Command {
                id,
                operation,
                payload,
            } = input.try_recv().unwrap()
            else {
                panic!("finish command");
            };
            assert_eq!(operation, "finish");
            (id, payload)
        };
        let (id, payload) = sent(&input);
        assert_eq!(
            payload,
            json!({"match_id":"room-match","back_to_room":true})
        );
        // A lobby from before rooms came back refuses the field: the room is
        // not kept and the finish goes again without it.
        let service = visit.live_service.as_mut().unwrap();
        assert!(service.finish_failed(id, http::CommandFailure::Rejected(0x8000_4005), now));
        service.poll_cleanup(now + Duration::from_secs(2));
        assert_eq!(sent(&input).1, json!({"match_id":"room-match"}));
    }

    #[test]
    fn a_leave_for_the_started_match_keeps_its_loading_map() {
        let (mut service, input, _output) = control_service(Some("test-match"));
        service.leave_room(Instant::now());
        let http::Request::Command { operation, .. } = input.try_recv().unwrap() else {
            panic!("leave command");
        };
        assert_eq!(operation, "leave");
        assert!(!service.room_closed(), "its own leave's channel_left");
        assert!(service.room_closed(), "a later one closes the room");
        let (mut service, _input, _output) = control_service(None);
        service.leave_room(Instant::now());
        assert!(service.room_closed(), "a leave before any start");
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
                    motd: None,
                    sign_in_token: None,
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
