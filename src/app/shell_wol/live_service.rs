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

use anyhow::{Context, Result, anyhow, ensure};
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
use crate::ui::chat_translation::{ChatLanguage, ChatReply, only_repeats, without_note};
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
/// The lobby's chat limit in UTF-8 bytes (docs/lobby-protocol.md): the
/// longest text `/v1/translate` takes, and the longest a lobby phrase fills
/// to before its line's own text stands ([`super::lobby_phrases`]).
pub(super) const CHAT_LIMIT_BYTES: usize = 2048;

/// The configured lobby's HTTPS origin: the build's, or `VERA20K_LOBBY_URL`
/// for staging.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn lobby_base_url() -> Result<String> {
    let base = std::env::var("VERA20K_LOBBY_URL").unwrap_or_else(|_| DEFAULT_LOBBY_URL.to_owned());
    ensure!(
        base.starts_with("https://") && !base[8..].chars().any(|ch| matches!(ch, '@' | '?' | '#')),
        "the online lobby needs its configured HTTPS service"
    );
    Ok(base)
}

/// The configured lobby's answer to a public read ([`lobby_get`]).
pub(crate) struct LobbyAnswer {
    pub(crate) status: u16,
    pub(crate) etag: Option<String>,
    pub(crate) body: Box<dyn std::io::Read>,
}

/// `GET <path>` from the configured lobby, for the reads that need no
/// session (`/v1/stats`, the map library), with `If-None-Match` when
/// `if_none_match` is given. Blocking: worker threads only. Its timeouts are
/// per read, not the session agent's 12 s in all, so a library map still
/// arrives on a slow line.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn lobby_get(path: &str, if_none_match: Option<&str>) -> Result<LobbyAnswer> {
    let url = format!("{}{path}", lobby_base_url()?.trim_end_matches('/'));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(std::time::Duration::from_secs(15)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(30)))
        .timeout_per_read(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let mut request = agent.get(&url).header("User-Agent", http::USER_AGENT);
    if let Some(tag) = if_none_match {
        request = request.header("If-None-Match", tag);
    }
    let response = request
        .call()
        .with_context(|| format!("cannot reach {url}"))?;
    Ok(LobbyAnswer {
        status: response.status().as_u16(),
        etag: response
            .headers()
            .get("ETag")
            .and_then(|tag| tag.to_str().ok())
            .map(str::to_owned),
        body: Box::new(response.into_body().into_reader()),
    })
}

/// The browser build starts no worker thread for these reads
/// ([`crate::app::worker::THREADS`]).
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub(crate) fn lobby_get(_path: &str, _if_none_match: Option<&str>) -> Result<LobbyAnswer> {
    anyhow::bail!("the browser build reads the lobby only through its session")
}

/// The departing visit's same HTTP/cleanup owner survives the menu teardown.
/// Native WOL_Main77B532 releases WOLAPI786080 after logout785EA0. VERA's
/// HTTPS retries are asynchronous: no retired callback may enter a new visit,
/// and a new worker waits for this single bounded owner to finish or expire.
#[derive(Default)]
pub(crate) struct OnlineCleanup {
    retired: Option<RetiredService>,
    // Reset per retirement and set by that owner's exact logout
    // acknowledgement, unlike a deadline stop. It never controls retries.
    logout_acknowledged: bool,
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
        self.logout_acknowledged
    }

    fn leave(&mut self, visit: &mut super::WolVisit, now: Instant) {
        if let Some(service) = visit.live_service.take() {
            self.retire(service, now);
        }
    }

    fn retire(&mut self, mut service: LiveService, now: Instant) {
        // New online workers are deferred while one retiring visit exists.
        assert!(self.retired.is_none(), "one online exit owner at a time");
        self.logout_acknowledged = false;
        service.auth_attempt = service.auth_attempt.wrapping_add(1);
        if let Some(pending) = service.pending.take()
            && service.match_id.is_none()
        {
            service.hold_match(Some(pending.match_id));
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
                        let matched = retired
                            .service
                            .logging_out
                            .as_ref()
                            .is_some_and(|logout| logout.request_id == id);
                        retired.service.logout_completed(id);
                        if matched {
                            self.logout_acknowledged = true;
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
                    // still in this FIFO. Finish that actual identity first; the
                    // worker also resumes it by URL, so only a match UUID is held.
                    if retired.service.match_id.is_none()
                        && let Some(id) = event.payload["match_id"].as_str()
                        && relay_uuid(id).is_ok()
                    {
                        retired.service.hold_match(Some(id.to_owned()));
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
    room: RoomAgreement,
    start_preparation: Option<StartPreparation>,
    /// The local reason was already shown. The ordered lobby stream will echo
    /// the cancellation before announcing another match.
    locally_refused_start: bool,
    player_id: String,
    member_ids: BTreeMap<String, String>,
    pending: Option<PendingStart>,
    /// The started match this player is in, until its `finish` is
    /// acknowledged or the session ends; only [`Self::hold_match`] writes it.
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
    /// (`back_to_room`), projected from MatchSession when building finish.
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
    /// VERA: the match this game watches, from its `watch_started` until it
    /// leaves with `unwatch`.
    watching: Option<Watching>,
}

/// VERA: a match this game watches (docs/lobby-protocol.md "Watching"). It
/// plays no part in it: it renews its grant with `renew_watch`, sends no
/// `finish`, and leaves with `unwatch`.
struct Watching {
    match_id: String,
    /// The start `watch_started` described, until the match's map is here
    /// and the relay is asked to let this game in.
    start: Option<WatchStart>,
}

struct WatchStart {
    version: String,
    rules_digest: String,
    host_name: String,
    users: Vec<(String, u32)>,
    seed: u32,
    address: SocketAddrV4,
    grant: DecodedRelayGrant,
    /// The timing every player's game agreed (`timing_confirmed`).
    pings: Vec<i32>,
    terms: crate::app::match_runtime::session::WolStartTerms,
}

/// The label of the login box's text for a lobby that did not answer,
/// instead of retail TXT_CANT_CONNECT's "Westwood Online".
const CANT_CONNECT_TEXT: &str = "VERA:OnlineCantConnect";

/// The label of the box's text when the lobby stops answering while the
/// player is in it.
const LOBBY_LOST_TEXT: &str = "VERA:OnlineConnectionLost";

/// The lobby's own 426 reason when it gave a plain one, then where to get a
/// newer build; without one, VERA's own sentence with the link.
fn update_required_text(message: Option<&str>) -> String {
    use crate::app::web_link::DOWNLOAD_PAGE;
    match message {
        Some(reason) => format!("{reason} {DOWNLOAD_PAGE}"),
        None => crate::i18n::tr_fill("VERA:OnlineUpdateRequired", &[("url", &DOWNLOAD_PAGE)]),
    }
}

/// The lobby says this version is behind: the desktop updater checks again
/// ([`crate::app::updater::lobby_outdated`]).
fn lobby_says_outdated(state: &mut AppState) {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    crate::app::updater::lobby_outdated(state);
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let _ = state;
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

/// One idempotent command, sent again under its request ID until it is
/// acknowledged. Every retry carries the payload first sent: the lobby
/// replays a committed request ID and refuses one reused with another payload
/// (docs/lobby-protocol.md `POST /v1/commands`).
struct RetryingCommand {
    request_id: u64,
    operation: &'static str,
    payload: Value,
    next_attempt: Instant,
    failures: u8,
}

impl RetryingCommand {
    fn failed(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1);
        self.next_attempt = now + Duration::from_secs((1u64 << self.failures.min(5)).min(30));
    }

    /// The command to send again, once its retry is due.
    fn take_due(&mut self, now: Instant) -> Option<(u64, &'static str, Value)> {
        if now < self.next_attempt {
            return None;
        }
        self.next_attempt = now + RELAY_TIMEOUT;
        Some((self.request_id, self.operation, self.payload.clone()))
    }
}

/// What this game agreed with its room's host, and its requests the host has
/// yet to answer. A new room, the room's `channel_left` and a host handover
/// each start it afresh.
#[derive(Default)]
struct RoomAgreement {
    /// Canonical received options only; MatchSession remains the editable owner.
    accepted_options: Option<Value>,
    /// The latest ordinary host publication. A legacy refusal may only change
    /// the editable room while this exact snapshot is still current.
    published_options: Option<(u64, Value)>,
    /// The latest `ready` request and the options revision it answered.
    last_ready_request: Option<(u64, u64)>,
    pending_choices: Option<PendingChoices>,
}

impl RoomAgreement {
    /// This game is done with the room before its `channel_left`: its
    /// publication and choices wait for no answer. The accepted options
    /// stay, so a late options echo still compares with them.
    fn stop_waiting(&mut self) {
        self.published_options = None;
        self.pending_choices = None;
    }

    /// The choices request the host did not answer in time, which then waits
    /// no longer.
    fn choices_unanswered(&mut self, now: Instant) -> bool {
        self.pending_choices
            .take_if(|choices| now >= choices.deadline)
            .is_some()
    }
}

/// The expected echo of one outgoing local choice, retained across earlier
/// full-options updates until the host accepts it, its request fails or this
/// game is done with the room.
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
/// each one shows, its text and the language it was asked for.
#[derive(Default)]
struct Translations {
    next: u64,
    pending: BTreeMap<u64, (super::translation::TranslatedLine, String, ChatLanguage)>,
    paused_until: Option<Instant>,
}

impl Translations {
    fn request(
        &mut self,
        requests: &Sender<http::Request>,
        text: &[u16],
        target: ChatLanguage,
        line: super::translation::TranslatedLine,
        now: Instant,
    ) {
        let text = String::from_utf16_lossy(text);
        if self.pending.len() >= MAX_PENDING_TRANSLATIONS
            || self.paused_until.is_some_and(|until| now < until)
            || text.len() > CHAT_LIMIT_BYTES
        {
            return;
        }
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        let request = http::Request::Translate {
            id,
            text: text.clone(),
            target: target.tag(),
        };
        if requests.send(request).is_ok() {
            self.pending.insert(id, (line, text, target));
        }
    }

    fn reply(
        &mut self,
        id: u64,
        outcome: Result<ChatReply, http::TranslateFailure>,
        now: Instant,
    ) -> Option<(super::translation::TranslatedLine, ChatLanguage, ChatReply)> {
        let (line, original, target) = self.pending.remove(&id)?;
        match outcome {
            Ok(reply) if only_repeats(&original, without_note(&reply.text)) => None,
            Ok(reply) => Some((line, target, reply)),
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
    /// The lobby took this game's `relay_ready`: from then on the start
    /// waits on the other players' games.
    relay_reported: bool,
    probes_started: bool,
    latencies_reported: bool,
    /// A returning player: the match started without this process, so its
    /// barriers are over and these are the timing it agreed.
    resume_pings: Option<Vec<i32>>,
    /// The unanswered `relay_ready` or `latency_ready`, resent within the
    /// start window when the lobby answers 429, 5xx or nothing.
    report: Option<RetryingCommand>,
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
    fn with_worker(
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
            watching: None,
        }
    }

    /// The first lobby-upgrade refusal of this sign-in, as one notice; the
    /// sign-in's later refused requests add none.
    fn outdated_notice(&mut self, message: Option<&str>) -> Option<String> {
        (self.outdated_notice != Some(self.auth_attempt)).then(|| {
            self.outdated_notice = Some(self.auth_attempt);
            update_required_text(message)
        })
    }

    fn publish_room_options(&mut self, options: wire::Options) {
        let options = json!(options);
        let id = self.command_with_id("options", json!({"options": &options}));
        self.room.published_options = Some((id, options));
    }

    fn is_room_publication(&self, id: u64) -> bool {
        self.room
            .published_options
            .as_ref()
            .is_some_and(|(sent, _)| *sent == id)
    }

    fn take_legacy_refusal(
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

    /// Send a command its owner retries until it is acknowledged.
    fn retrying_command(
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
    fn look_up_ladder(&mut self, names: Vec<String>, high_scores: bool) {
        if high_scores && self.ladder_lookup.take().as_ref() == Some(&names) {
            return;
        }
        self.command("ladder", json!({"names":names}));
        self.ladder_lookup = (!high_scores).then_some(names);
    }

    /// The match returned to the shell: finish it, a lobby match with its
    /// result and winners. A spawned match reports when the process exits,
    /// and a watched one is only left.
    fn match_returned(&mut self, (result, winners, decided): MatchReport, now: Instant) {
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
    fn begin_finish(&mut self, now: Instant) {
        let Some(match_id) = self.match_id.clone() else {
            return;
        };
        if self.finishing.is_some() {
            return;
        }
        self.next_renewal_attempt = None;
        self.relay_renewal_due = None;
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
    fn match_unfinished(&self) -> bool {
        !self.spawned && self.match_id.is_some() && self.finishing.is_none()
    }

    /// A lobby `game_started` is already committed for every member. If this
    /// game cannot adopt it, the existing authenticated `finish` cancels that
    /// exact pending match for everyone. Leave targets a room, not a match.
    fn receive_lobby_start(
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
    fn keep_network_report(&mut self, report: Value) {
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
            self.hold_match(None);
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
            self.leaving = Some(self.retrying_command("leave", json!({}), now));
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
            self.logging_out = Some(self.retrying_command("logout", json!({}), now));
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
    fn hold_match(&mut self, match_id: Option<String>) {
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
    fn sign_in(&mut self, nickname: String, credential: http::Credential) {
        let _ = self.requests.send(http::Request::OpenSession {
            attempt: self.auth_attempt,
            nickname,
            credential,
        });
        self.clear_cleanup();
    }

    /// A gameplay grant reached its UDP owner at `now_ms`.
    fn relay_granted(&mut self, now_ms: u32) {
        let renew_after = (RELAY_GRANT_LIFETIME - RELAY_RENEWAL_MARGIN).as_millis() as u32;
        self.relay_renewal_due = Some(now_ms.wrapping_add(renew_after));
        self.next_renewal_attempt = None;
    }

    /// The lobby already reserved this match's place. Release it if its start
    /// cannot be decoded or installed, including before a relay route exists.
    fn receive_watch_start(
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
    fn stop_watching(&mut self) -> bool {
        let Some(watching) = self.watching.take() else {
            return false;
        };
        self.next_renewal_attempt = None;
        self.relay_renewal_due = None;
        self.command("unwatch", json!({"match_id":watching.match_id}));
        true
    }

    fn poll_renewal(&mut self, now_ms: u32) {
        if self.finishing.is_some() {
            return;
        }
        // A watcher renews its watch, a player its place in the match.
        let (operation, match_id) = match (&self.watching, &self.match_id) {
            (Some(watching), _) => ("renew_watch", &watching.match_id),
            (None, Some(match_id)) => ("renew_relay", match_id),
            (None, None) => return,
        };
        let Some(due) = self.relay_renewal_due else {
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
        self.command(operation, json!({"match_id":match_id}));
    }

    fn send_report(&mut self, operation: &'static str, payload: Value, now: Instant) {
        let report = self.retrying_command(operation, payload, now);
        if let Some(pending) = self.pending.as_mut() {
            pending.report = Some(report);
        }
    }

    fn report_completed(&mut self, id: u64) {
        if let Some(pending) = self.pending.as_mut()
            && let Some(report) = pending.report.take_if(|report| report.request_id == id)
        {
            pending.relay_reported |= report.operation == "relay_ready";
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

    fn poll_report(&mut self, now: Instant) {
        if let Some((id, operation, payload)) = self
            .pending
            .as_mut()
            .and_then(|pending| pending.report.as_mut())
            .and_then(|report| report.take_due(now))
        {
            self.send_command(id, operation, payload);
        }
    }

    fn admission_blocked(&self) -> bool {
        self.finishing.is_some() || self.leaving.is_some() || self.logging_out.is_some()
    }

    /// The line for a pending start whose barrier ran out by `now`. Until
    /// `start_confirmed` the barrier is relay admission; after it, the peer
    /// latency check. A game whose own relay answered while the lobby never
    /// took its `relay_ready` waited on the lobby.
    fn expired_admission(&self, now: Instant) -> Option<&'static str> {
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
    fn start_rejected_line(&self, reason: &str) -> &'static str {
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

    fn poll_cleanup(&mut self, now: Instant) {
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
            Err(error) => {
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
                    log::warn!("Online play could not start its lobby service: {error:#}");
                    service_event(state, WolServiceEvent::Api(WolApiStatus::Missing));
                }
                return Some(Instant::now() + Duration::from_secs(1));
            }
        }
    }
    let mut service = take_service(state)?;
    if state.frontend.shell_exit.is_none() {
        // A timed-out watch may still be downloading or negotiating. Retire
        // only that admission before handling a later grant.
        if service.watching.as_ref().is_some_and(|watch| {
            (watch.start.is_some() || service.pending.is_some())
                && !super::lobby::awaits_watch(state, &watch.match_id)
        }) {
            cancel_watch_start(state, &mut service);
        }
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
                        | WolServiceRequest::Watch { .. }
                        | WolServiceRequest::Connect { .. }
                        | WolServiceRequest::VisitLobby
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
        if service.watching.is_some() {
            // A watcher's setup is no room's: it tells the lobby nothing.
            if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
                protocol.take_service_requests();
            }
        } else if state.match_state.session_mode()
            == crate::app::match_runtime::session::SessionMode::Wol
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
                if let Err(error) = request_setup(state, &mut service, request) {
                    log::warn!("Online setup request failed: {error:#}");
                    let text = if let Some(MapTooSmall(capacity)) = error.downcast_ref() {
                        crate::assets::csf_file::format_csf(
                            &crate::app::App::csf_label(
                                state,
                                "TXT_SCENARIO_TOO_SMALL",
                                "TXT_SCENARIO_TOO_SMALL",
                            ),
                            &[crate::assets::csf_file::CsfArg::Int(i64::from(*capacity))],
                        )
                    } else {
                        tr("VERA:OnlineRequestMismatch")
                    };
                    if starts {
                        service.start_preparation = None;
                        fail_start(state, &text);
                    } else {
                        notice(state, &text);
                    }
                }
            }
        }
        if service.room.choices_unanswered(Instant::now()) {
            restore_accepted_options(state, &service);
            notice(state, &tr("VERA:ChoicesNotAcknowledged"));
        }
        if let Err(error) = poll_start_preparation(state, &mut service) {
            log::warn!("Online start preparation failed: {error:#}");
            service.start_preparation = None;
            fail_start(state, &tr("VERA:SettingsNotAgreed"));
        }
        let admission = match service.expired_admission(Instant::now()) {
            Some(line) => Err((line, anyhow!("the start's admission barrier timed out"))),
            None => poll_relay(state, &mut service)
                .map_err(|error| ("VERA:RelayConnectionFailed", error)),
        };
        if let Err((line, error)) = admission {
            if service.watching.is_some() {
                log::warn!("Could not watch the match: {error:#}");
                fail_watch(state, &mut service, "VERA:RelayConnectionFailed");
            } else {
                log::warn!("Online relay admission failed ({line}): {error:#}");
                cancel_pending(state, &mut service);
                fail_start(state, &tr(line));
            }
        }
        if !service.spawned && service.pending.is_none() && service.match_id.is_none() {
            maps::poll(state, &mut service);
        }
        if let Err(error) = poll_watch(state, &mut service) {
            log::warn!("Could not watch the match: {error:#}");
            fail_watch(state, &mut service, "VERA:WatchCouldNotStart");
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
    let verifying = crate::app::shell_network_setup::verify(state).is_some();
    if !verifying && let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        // A rejected HTTP Start has not handed the setup to the lobby runner.
        // Keep the ordinary host/join screen and release its pending input.
        protocol.set_start_phase(crate::app::shell_network_setup::StartPhase::Idle);
        notice(state, text);
        return;
    }
    // VERA: the abort leaves the room, and the setup's chat with it
    // (`start_aborted`), so the reason waits for the chat the player lands in.
    crate::app::shell_wol::hold_problem_line(state, text.to_owned());
    if verifying {
        let _ = crate::app::shell_network_setup::service_event(
            state,
            NetworkSetupEvent::Negotiated { result: 1 },
        );
    } else {
        crate::app::shell_wol::start_aborted(state);
    }
}

/// The lobby's `start_rejected` `phrases` in the language in use: who held
/// the start up. Phrases this game cannot read are left out.
fn lobby_names(payload: &Value) -> Option<String> {
    let phrases: Vec<super::lobby_phrases::Phrase> =
        serde_json::from_value(payload.get("phrases")?.clone()).ok()?;
    super::lobby_phrases::localized(&phrases).filter(|names| !names.is_empty())
}

/// Whose game failed relay admission: once the lobby took this game's
/// `relay_ready`, another player's game is the one the start waited on.
fn relay_failure_line(pending: &PendingStart) -> &'static str {
    if pending.relay_reported {
        "VERA:StartRelayNotReachedByOther"
    } else {
        "VERA:StartRelayNotReached"
    }
}

fn cancel_pending(state: &mut AppState, service: &mut LiveService) {
    service.start_preparation = None;
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.set_start_phase(crate::app::shell_network_setup::StartPhase::Idle);
    }
    if service.pending.take().is_some() {
        // A Ready relay may already have moved into the shared transport owner.
        // Retire either ownership location when this start is canceled.
        crate::app::lan_service::close(state);
    }
}

/// This game is done with its room (it left, signed out or lost the lobby,
/// or the room closed) or with a watch's start: the map transfer and any
/// start stop, and nothing waits for the host's answer.
fn end_room(state: &mut AppState, service: &mut LiveService) {
    maps::cancel(state, service);
    cancel_pending(state, service);
    service.room.stop_waiting();
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
    let back_to_room = state.match_state.session.awaiting_wol_room_return();
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.match_returned(report, Instant::now(), back_to_room);
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

    fn match_returned(&mut self, report: MatchReport, now: Instant, back_to_room: bool) {
        if self.provider != WolProvider::Online {
            return;
        }
        if let Some(service) = self.live_service.as_mut() {
            service.back_to_room = back_to_room;
            service.match_returned(report, now);
        } else if !self.requests.contains(&WolServiceRequest::MatchReturned) {
            //poll holds the SAME service across HTTP callbacks. Loading abort
            //can reenter the lobby there; retain its semantic intent before
            //new join/create requests, without copying the match or retry ID.
            self.requests.insert(0, WolServiceRequest::MatchReturned);
        }
    }

    /// The game closes before the shell return finished this visit's lobby
    /// match: the match finishes with `report`, the result the game recorded
    /// for the player who left, and asks for no room back.
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    fn match_left_for_exit(&mut self, report: MatchReport, now: Instant) {
        self.match_returned(report, now, false);
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
/// the visit so nothing but that cleanup runs until exit. A lobby match the
/// shell return has not finished first finishes with the player's result:
/// `quit`, as Abort Mission records it, unless the game already decided one.
/// One still loading has no result and finishes without one, as a match
/// whose loading failed does.
pub(crate) fn retire_visit_for_exit(state: &mut AppState) {
    let Some(visit) = state
        .frontend
        .wol
        .as_ref()
        .filter(|visit| visit.provider == WolProvider::Online)
    else {
        return;
    };
    if visit
        .live_service
        .as_ref()
        .is_some_and(LiveService::match_unfinished)
    {
        crate::app::App::record_match_result(state, crate::app::cncnet::ExternalMatchResult::Quit);
        let report = match_report(state);
        if let Some(visit) = state.frontend.wol.as_mut() {
            visit.match_left_for_exit(report, Instant::now());
        }
    }
    leave_visit(state);
    state.frontend.wol = None;
}

/// Answer a request that needs no lobby; false for any other.
fn answer_local_request(state: &mut AppState, request: &WolServiceRequest) -> bool {
    match request {
        WolServiceRequest::EnsureApi => {
            service_event(state, WolServiceEvent::Api(WolApiStatus::Available))
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
            service_event(
                state,
                WolServiceEvent::NetStatus(super::SERVER_STATUS_READY),
            );
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
    match request {
        request @ (WolServiceRequest::EnsureApi | WolServiceRequest::FetchServers { .. }) => {
            answer_local_request(state, &request);
        }
        // The login reducer asks only native providers for stored credentials.
        WolServiceRequest::ListCredentials => {}
        WolServiceRequest::Connect {
            nickname,
            password,
            remember,
            ..
        } => {
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            let credential =
                sign_in_credential(settings_dir(state), service, &nickname, password, remember);
            service.signing_in_remembered = matches!(credential, http::Credential::Remembered(_));
            service.sign_in(nickname, credential);
        }
        WolServiceRequest::Account(request) => send_account_request(service, request),
        WolServiceRequest::ForgetSignIn => forget_sign_in(settings_dir(state), service),
        WolServiceRequest::ListChannels { kind, .. } => {
            let _ = service.requests.send(http::Request::Channels(kind));
        }
        WolServiceRequest::ListMatches => {
            let _ = service.requests.send(http::Request::Matches {
                refused_options: None,
            });
        }
        WolServiceRequest::Watch { match_id } => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command("watch", json!({"match_id":match_id}));
            }
        }
        WolServiceRequest::JoinChannel { channel, key } => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command("join", json!({"channel":channel.name,"key":key}));
            }
        }
        WolServiceRequest::VisitLobby => {
            if super::lobby::bind_online_request(state, service.next_request) {
                service.command("visit_lobby", json!({}));
            }
        }
        WolServiceRequest::EndVisit => service.command("end_visit", json!({})),
        WolServiceRequest::VisitChat { line, to } => chat(state, service, line, to, true),
        WolServiceRequest::VisitEmote { text, to } => service.command(
            "chat",
            json!({"text":String::from_utf16_lossy(&text),"to":to,"action":true,"visit":true}),
        ),
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
            end_room(state, service);
            service.leave_room(Instant::now());
        }
        WolServiceRequest::MatchReturned => {
            service.back_to_room = state.match_state.session.awaiting_wol_room_return();
            service.match_returned(match_report(state), Instant::now());
        }
        WolServiceRequest::Logout => {
            end_room(state, service);
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            service.begin_logout(Instant::now());
            crate::app::lan_service::close(state);
        }
        WolServiceRequest::Chat { line, to } => chat(state, service, line, to, false),
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
            end_room(state, service);
            service.auth_attempt = service.auth_attempt.wrapping_add(1);
            if let Some(visit) = state.frontend.wol.as_mut() {
                visit.authenticated = false;
                visit.connected = false;
                visit.requests.retain(|request| {
                    !matches!(
                        request,
                        WolServiceRequest::JoinChannel { .. }
                            | WolServiceRequest::CreateChannel { .. }
                            | WolServiceRequest::Watch { .. }
                            | WolServiceRequest::VisitLobby
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
        WolServiceRequest::Translate { text, target, line } => {
            service
                .translations
                .request(&service.requests, &text, target, line, Instant::now());
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
        None,
        WolChatLine::Say(text.encode_utf16().collect()),
        vec![matchbot],
        false,
    );
}

/// [`send_chat`], answering a page's result to its sink `0x007A9800`.
fn chat(
    state: &mut AppState,
    service: &mut LiveService,
    line: WolChatLine,
    to: Vec<String>,
    visit: bool,
) {
    if let Some(result) = send_chat(service, state.frontend.wol.as_ref(), line, to, visit) {
        service_event(state, WolServiceEvent::PageResult(result));
    }
}

/// `visit` sends to the public lobby this player visits from their room.
/// A page (API+0xF8, or `/reply` to `[0xB77E30]`) returns its result: the
/// lobby delivers private chat only within one room and outside a match
/// (docs/lobby-protocol.md, `chat`), so a page goes, under the name the
/// room lists, to this player's channel or the lobby they visit, and fails
/// unsent for anyone neither lists.
/// RESIDUAL: success is answered on sending; a later refusal shows the
/// generic refused line after `TXT_PAGESENT`.
fn send_chat(
    service: &mut LiveService,
    wol: Option<&super::WolVisit>,
    line: WolChatLine,
    mut to: Vec<String>,
    mut visit: bool,
) -> Option<Result<(), u32>> {
    let (text, page) = match line {
        WolChatLine::Say(text) => (text, None),
        WolChatLine::Page { name, text } => (text, Some(String::from_utf16_lossy(&name))),
        WolChatLine::Reply { text } => {
            let pager = wol
                .and_then(|wol| wol.last_pager.clone())
                .unwrap_or_default();
            (text, Some(pager))
        }
        WolChatLine::Locate { name } => {
            service.command("locate", json!({"name":String::from_utf16_lossy(&name)}));
            return None;
        }
        WolChatLine::Notice(_) => return None,
    };
    if let Some(name) = &page {
        let Some((name, visited)) = wol.and_then(|wol| super::lobby::page_target(wol, name)) else {
            return Some(Err(0x8000_4005));
        };
        to = vec![name.to_owned()];
        visit = visited;
    }
    let mut chat = json!({"text":String::from_utf16_lossy(&text),"to":to,"action":false});
    if visit {
        chat["visit"] = Value::Bool(true);
    }
    service.command("chat", chat);
    page.map(|_| Ok(()))
}

/// The map digests are the verified local source's. Without one the map
/// status is 0, which proves nothing: the lobby checks only the shape, and a
/// fixed impossible effective digest keeps it valid without claiming
/// unverified expanded content.
fn local_agreement(state: &mut AppState, revision: u64) -> Result<Value> {
    let session = &state.match_state.session;
    let map_digest = session
        .network_local_map_source()
        .and_then(|source| source.physical_identity())
        .map_or_else(String::new, |(_, digest)| digest.to_owned());
    let effective_map_digest = session
        .network_local_map_effective_digest()
        .map_or_else(|| "0".repeat(64), str::to_owned);
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
    if crate::map::rmg::is_seed_selection(file)
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
            && state
                .match_state
                .session
                .network_local_map_source()
                .and_then(|source| source.physical_identity())
                == Some((
                    expected.byte_length as usize,
                    expected.source_sha256.as_str(),
                )),
    )
}

#[derive(Debug, thiserror::Error)]
#[error("room exceeds the map's {0} starts")]
struct MapTooSmall(i32);

fn check_map_capacity(options: &wire::Options) -> Result<(), MapTooSmall> {
    let launch = &options.launch;
    let needed = crate::app::shell_network_setup::starts_needed(
        launch,
        launch.options.teams.is_some(),
        crate::ui::multiplayer_setup::SetupFlavor::Wol,
    );
    if needed > options.map_metadata.player_capacity {
        return Err(MapTooSmall(options.map_metadata.player_capacity));
    }
    Ok(())
}

/// The host's options for the lobby. The map is the one its session
/// admitted when the host chose it, as LAN publishes it.
fn options(state: &AppState) -> Result<wire::Options> {
    let session = &state.match_state.session;
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no online setup"))?
        .clone();
    let map_metadata = session
        .network_map_metadata()
        .cloned()
        .ok_or_else(|| anyhow!("no admitted map metadata"))?;
    let options = wire::Options {
        launch,
        map_metadata,
        random_map: crate::app::shell_network_setup::maps::host_generated_manifest(state)?,
        vacancies: std::array::from_fn(|row| {
            session
                .network_vacancy(row as u8)
                .unwrap_or(crate::app::match_runtime::session::NetworkVacancy::Closed)
        }),
        observer_row: session.network_observer_row(),
        cooperative_difficulty: state
            .frontend
            .multiplayer_shell_runtime
            .cooperative_difficulty(),
        cooperative_campaign: cooperative_campaign(state),
        seed: Some(crate::app::shell_network_setup::resident_seed(state)),
        // Only a host's VERA room has one; a guest publishes no options.
        watching: state
            .frontend
            .network_setup_protocol
            .as_ref()
            .and_then(|protocol| protocol.published_watching(super::lobby_watches(state)))
            .map(str::to_owned),
    };
    check_map_capacity(&options)?;
    Ok(options)
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
    let Some(accepted) = service.room.accepted_options.clone() else {
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
        service.room.pending_choices.is_none()
            && service
                .room
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
        protocol.set_start_phase(crate::app::shell_network_setup::StartPhase::Submitted);
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
        // Cooperative's C0 also travels in the host's options.
        NetworkSetupRequest::PublishOptions
        | NetworkSetupRequest::PublishRoster
        | NetworkSetupRequest::PublishCooperativeCampaign(_) => {
            service.publish_room_options(options(state)?);
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
            service.room.pending_choices = (answered
                && !switch
                && !service
                    .room
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
            service.room.last_ready_request = Some((id, service.revision));
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
                service.room.published_options = None;
                let snapshot = canonical_options(&options(state)?)?;
                let revision = (service.room.accepted_options.as_ref() == Some(&snapshot))
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
                    protocol
                        .set_start_phase(crate::app::shell_network_setup::StartPhase::Preparing);
                }
                notice(state, &tr("VERA:ConfirmingFinalSettings"));
            }
        }
        NetworkSetupRequest::Leave => {
            end_room(state, service);
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
        // VERA's room: the host's Balance is `/balance` as typed.
        NetworkSetupRequest::Balance => {
            service.command("chat", json!({"text":"/balance","to":[],"action":false}))
        }
        NetworkSetupRequest::WolChat(line) => chat(state, service, line, Vec::new(), false),
        NetworkSetupRequest::WolEmote(text) => service.command(
            "chat",
            json!({"text":String::from_utf16_lossy(&text),"to":[],"action":true}),
        ),
        NetworkSetupRequest::RequestAcceptance | NetworkSetupRequest::ObserverSlot { .. } => {}
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
            let mut text = tr(if retryable {
                "VERA:StartLobbyBusy"
            } else {
                "VERA:StartRefusedSetup"
            });
            // VERA: the lobby does not say which player differs; this game
            // can say what it changes.
            if let Some(own) = (!retryable)
                .then(|| crate::app::shell_network_setup::agreement::own_changes_text(state))
                .flatten()
            {
                text.push(' ');
                text.push_str(&own);
            }
            fail_start(state, &text);
        }
        http::Response::Authenticated {
            attempt,
            player_id,
            nickname,
            motd,
            sign_in_token,
            lobby_visits,
        } if attempt == service.auth_attempt => {
            // `http::decode_session` admitted this identity.
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
            service_event(
                state,
                WolServiceEvent::Login(Ok(WolLoginAccepted { motd, lobby_visits })),
            );
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
            lobby_says_outdated(state);
        }
        http::Response::Outdated { message } => {
            if let Some(text) = service.outdated_notice(message.as_deref()) {
                notice(state, &text);
                lobby_says_outdated(state);
            }
        }
        http::Response::Channels(list) => service_event(
            state,
            WolServiceEvent::Channels {
                kind: list.kind,
                channels: list.channels.into_iter().map(Into::into).collect(),
            },
        ),
        http::Response::Matches {
            list: matches,
            refused_options,
        } => {
            if refused_options.is_some_and(|id| !service.is_room_publication(id)) {
                return;
            }
            let watches = matches.is_some();
            // Compare before replacing the capability: it affects the Watching
            // field in the options we originally sent.
            let legacy_refusal = refused_options.is_some_and(|id| {
                service.take_legacy_refusal(id, watches, || {
                    options(state).ok().map(|room| json!(room))
                })
            });
            let before = state
                .frontend
                .wol
                .as_mut()
                .and_then(|visit| visit.lobby_watches.replace(watches));
            // A lobby that lists no matches is from before watching, or went
            // back to it: a room whose options carried its Watching goes on
            // without it.
            if !watches
                && state.match_state.session.host()
                && (legacy_refusal || refused_options.is_none() && before != Some(false))
            {
                if state
                    .frontend
                    .network_setup_protocol
                    .as_mut()
                    .is_some_and(|room| room.stop_legacy_watching())
                {
                    notice(state, &tr("VERA:WatchingNotTaken"));
                } else if legacy_refusal
                    && let Some(key) = crate::app::shell_network_setup::lobby_refused_option(state)
                {
                    notice(state, &tr(key));
                }
            }
            service_event(
                state,
                WolServiceEvent::Matches(
                    matches
                        .unwrap_or_default()
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                ),
            )
        }
        http::Response::Event(event) => {
            // The reason goes in the log: a guest who sees too little of the
            // host's room can only tell from it which check refused it.
            if let Err(error) = apply_event(state, service, &event.kind, event.payload) {
                log::warn!(
                    "Online lobby update rejected: kind={}: {error:#}",
                    event.kind
                );
                notice(state, &tr("VERA:InvalidLobbyUpdate"));
            }
        }
        http::Response::CommandCompleted { id, operation } => match operation {
            "options" if service.is_room_publication(id) => service.room.published_options = None,
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
            // VERA: a refused visit leaves the player in their room
            // (docs/lobby-protocol.md "Visiting the lobby").
            "visit_lobby" => {
                super::lobby::online_request_failed(state, id, failure.wol_code());
            }
            // A watch joins no room, so nothing needs leaving; a place it
            // took anyway frees itself once its grant runs out.
            "watch" => {
                super::lobby::online_request_failed(state, id, failure.wol_code());
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
                if service.is_room_publication(id)
                    && let Some(key) =
                        crate::app::shell_network_setup::lobby_refused_building_queue(state)
                {
                    // The newest room rule first: a lobby from before it lists
                    // matches, so the capability check below cannot tell.
                    notice(state, &tr(key));
                } else if service.is_room_publication(id) {
                    notice(state, &tr("VERA:OnlineRequestRefused"));
                    // Only a fresh 404 for this publication can confirm rollback.
                    let _ = service.requests.send(http::Request::Matches {
                        refused_options: Some(id),
                    });
                }
            }
            "choices"
                if service
                    .room
                    .pending_choices
                    .as_ref()
                    .is_some_and(|choices| choices.request_id == id) =>
            {
                service.room.pending_choices = None;
                restore_accepted_options(state, service);
                notice(state, &tr("VERA:ChoicesRefused"));
            }
            "ready" if service.room.last_ready_request == Some((id, service.revision)) => {
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
            // The room moved on before these reached the lobby: the host
            // changed its settings again, as a dragged slider does at each
            // step, or a player came or went. Its next options send the map
            // status again, with the host's ready; a guest's ready follows
            // its Accept, the automatic one in VERA's room.
            "map_status" | "ready" if failure == http::CommandFailure::StaleRevision => {}
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
            // A refused ladder lookup leaves the tables as they were; the
            // high-score sink only logs a failure (`0x007A3B30`).
            "ladder" => {}
            // The match ended for its players: the watcher plays on to the
            // end of the stream with the grant it holds, which outlasts the
            // delay (docs/lobby-protocol.md "Watching").
            "renew_watch" if failure != http::CommandFailure::Retryable => {
                service.relay_renewal_due = None;
            }
            "renew_watch" | "unwatch" => {}
            "relay_ready" | "latency_ready" => {
                if !service.report_failed(id, failure, Instant::now()) {
                    notice(state, &tr("VERA:OnlineRequestRefused"));
                }
            }
            _ => notice(state, &tr("VERA:OnlineRequestRefused")),
        },
        http::Response::Translated { id, outcome } => {
            if let Some((line, target, reply)) =
                service.translations.reply(id, outcome, Instant::now())
            {
                super::translation::show(state, line, target, &reply);
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
            end_room(state, service);
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
            end_room(state, service);
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
    /// VERA: said in the public lobby this player visits from their room.
    #[serde(default)]
    visit: bool,
    /// The lobby's own lines as phrases ([`super::lobby_phrases`]), read
    /// only once the event decodes, so that a shape this game does not know
    /// leaves the line in `text`.
    #[serde(default)]
    phrases: Value,
}

impl Chat {
    /// The line in the language in use when the lobby said it as phrases
    /// that VERA's language files hold, else the lobby's `text`, and whether
    /// it is the former. A matchbot reply's phrases say what follows its
    /// first word, the reply's kind, which its Quick Match runner reads
    /// (`Info`).
    fn shown_text(&self) -> (Vec<u16>, bool) {
        let Some(said) =
            serde_json::from_value::<Vec<super::lobby_phrases::Phrase>>(self.phrases.clone())
                .ok()
                .and_then(|phrases| super::lobby_phrases::localized(&phrases))
        else {
            return (self.text.encode_utf16().collect(), false);
        };
        let said = match self.text.split_once(' ') {
            Some((kind, _)) if self.ansi => format!("{kind} {said}"),
            _ => said,
        };
        (said.encode_utf16().collect(), true)
    }

    /// The lobby's line that a newer release is out (`NewRelease`), which a
    /// game older than the newest release hears once per release. Only the
    /// lobby's own lines carry `phrases`. Only their keys are read, so values
    /// a newer lobby adds that this game cannot show still count.
    fn announces_release(&self) -> bool {
        #[derive(Deserialize)]
        struct Said {
            key: String,
        }
        Vec::<Said>::deserialize(&self.phrases)
            .is_ok_and(|phrases| phrases.iter().any(|phrase| phrase.key == "NewRelease"))
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

/// The ticket, keys and gameplay clock every relay grant carries, decoded
/// and size-checked; whose they are is the caller's to check.
fn decode_grant_fields(
    ticket: &str,
    packet_key: &str,
    payload_key: &str,
    ordering_certificate: &str,
) -> Result<(DecodedRelayGrant, crate::net::udp::OrderingCertificate)> {
    ensure!(
        ticket.len() <= 160
            && packet_key.len() == 43
            && payload_key.len() == 43
            // Base64 of a 66-byte VO1C or a 67-byte VO2C certificate.
            && matches!(ordering_certificate.len(), 88 | 90),
        "invalid relay grant"
    );
    let decode = |text: &str| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| anyhow!("invalid relay grant"))
    };
    let key = |text: &str| -> Result<[u8; 32]> {
        decode(text)?
            .try_into()
            .map_err(|_| anyhow!("invalid relay grant"))
    };
    let (ticket, key, payload_key) = (decode(ticket)?, key(packet_key)?, key(payload_key)?);
    let ordering_certificate = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(ordering_certificate)
        .map_err(|_| anyhow!("invalid gameplay clock"))?;
    let certificate = crate::net::udp::OrderingCertificate::parse(&ordering_certificate)
        .map_err(|_| anyhow!("invalid gameplay clock"))?;
    let grant = DecodedRelayGrant {
        ticket,
        key,
        payload_key,
        ordering_certificate,
        federation: None,
    };
    Ok((grant, certificate))
}

fn decode_relay_grant(
    grant: &wire::RelayGrant,
    match_id: &str,
    player_id: &str,
) -> Result<DecodedRelayGrant> {
    let (mut decoded, certificate) = decode_grant_fields(
        &grant.ticket,
        &grant.packet_key,
        &grant.payload_key,
        &grant.ordering_certificate,
    )?;
    let parsed = crate::net::udp::RelayTicket::parse(&decoded.ticket)
        .map_err(|_| anyhow!("invalid relay grant"))?;
    ensure!(
        parsed.source == grant.source_index
            && parsed.room == relay_uuid(match_id)?
            && parsed.player == relay_uuid(player_id)?
            && certificate.room == parsed.room
            && certificate.sources_mask & (1 << parsed.source) != 0,
        "relay grant identity differs"
    );
    decoded.federation = grant
        .federation
        .as_ref()
        .map(|roster| {
            ensure!(roster.len() <= 1024, "invalid relay roster");
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(roster)
                .map_err(|_| anyhow!("invalid relay roster"))
        })
        .transpose()?;
    Ok(decoded)
}

/// A watcher's grant (`watch_started`, `watch_renewed`): a watch ticket
/// for `match_id` with that match's keys and gameplay clock, no source of
/// its own and one relay even in a federated match.
fn decode_watch_grant(grant: &wire::WatchGrant, match_id: &str) -> Result<DecodedRelayGrant> {
    let (decoded, certificate) = decode_grant_fields(
        &grant.ticket,
        &grant.packet_key,
        &grant.payload_key,
        &grant.ordering_certificate,
    )?;
    let parsed = crate::net::udp::RelayWatchTicket::parse(&decoded.ticket)
        .map_err(|_| anyhow!("invalid watch grant"))?;
    ensure!(
        parsed.room == relay_uuid(match_id)? && certificate.room == parsed.room,
        "watch grant identity differs"
    );
    Ok(decoded)
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
        "watch_started" => {
            let id = payload["match_id"]
                .as_str()
                .context("missing watch match")?
                .to_owned();
            relay_uuid(&id)?;
            let awaited = super::lobby::awaits_watch(state, &id);
            if let Err(error) = service.receive_watch_start(&id, awaited, || {
                let start = prepare_watch(state, decode(payload)?)?;
                if let Some(lobby) = state
                    .frontend
                    .wol
                    .as_mut()
                    .and_then(|visit| visit.lobby_mut())
                {
                    lobby.watch_granted(&id);
                }
                Ok(start)
            }) {
                log::warn!("Could not watch the match: {error:#}");
                fail_watch(state, service, "VERA:WatchCouldNotStart");
            }
        }
        "watch_renewed" => {
            #[derive(Deserialize)]
            struct Renewed {
                match_id: String,
                relay: wire::WatchGrant,
            }
            let renewed: Renewed = decode(payload)?;
            if service.watching.as_ref().map(|watching| &watching.match_id)
                != Some(&renewed.match_id)
            {
                return Ok(());
            }
            let grant = decode_watch_grant(&renewed.relay, &renewed.match_id)?;
            crate::app::lan_service::renew_online_relay(
                state,
                renewed.relay.address,
                &grant.ticket,
                grant.key,
                grant.payload_key,
            )?;
            service.relay_granted(crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32);
        }
        // The public lobby this player visits from a game room, whose state
        // stays as it is (docs/lobby-protocol.md "Visiting the lobby").
        "joined" if payload["visit"].as_bool() == Some(true) => {
            let joined: wire::Joined = decode(payload)?;
            let members = joined
                .members
                .into_iter()
                .map(|member| member.into_member(&local))
                .collect();
            service_event(
                state,
                WolServiceEvent::VisitJoined(super::lobby::WolJoined {
                    channel: joined.channel.into(),
                    members,
                }),
            );
        }
        "joined" | "created" | "room_reopened" => {
            let joined: wire::Joined = decode(payload)?;
            service.revision = 0;
            service.room = RoomAgreement::default();
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
                crate::app::loading::pump::cancel_network_loading(
                    state,
                    &tr("VERA:OnlineRoomClosedWhileLoading"),
                );
            }
            end_room(state, service);
            service.room = RoomAgreement::default();
            if state
                .frontend
                .wol
                .as_ref()
                .is_some_and(|visit| visit.in_game)
            {
                // The kept room closed during the match: nothing to return to.
                state.match_state.session.take_network_room_return();
            }
            service_event(state, WolServiceEvent::ChannelLeft);
        }
        "member_joined" => {
            #[derive(Deserialize)]
            struct Payload {
                member: wire::Member,
                #[serde(default)]
                visit: bool,
            }
            let p: Payload = decode(payload)?;
            if p.visit {
                service_event(
                    state,
                    WolServiceEvent::VisitUserJoined(p.member.into_member(&local)),
                );
                return Ok(());
            }
            service.member_ids.insert(
                p.member.name.to_ascii_lowercase(),
                p.member.player_id.clone(),
            );
            service_event(
                state,
                WolServiceEvent::UserJoined(p.member.into_member(&local)),
            );
        }
        // The room's host left and the lobby made another player its host
        // (docs/lobby-protocol.md "Host handover"): what this game agreed
        // with the old host is gone, and the room opens again.
        "host_changed" => {
            #[derive(Deserialize)]
            struct Payload {
                member: wire::Member,
                channel: Option<wire::Channel>,
            }
            let p: Payload = decode(payload)?;
            service.room = RoomAgreement::default();
            service.start_preparation = None;
            maps::cancel(state, service);
            service_event(
                state,
                WolServiceEvent::HostChanged {
                    name: p.member.name,
                    channel: p.channel.map(Into::into),
                },
            );
        }
        "member_left" => {
            let name = payload["name"]
                .as_str()
                .ok_or_else(|| anyhow!("missing member"))?
                .to_owned();
            if payload["visit"].as_bool() == Some(true) {
                service_event(state, WolServiceEvent::VisitUserLeft(name));
                return Ok(());
            }
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
            let mut p: wire::OptionsEvent = decode(payload).inspect_err(|_| {
                log::warn!("Online host options could not be decoded");
            })?;
            if p.revision < service.revision {
                return Ok(());
            }
            let changed = p.revision != service.revision || service.room.accepted_options.is_none();
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
                .room
                .pending_choices
                .as_ref()
                .is_some_and(|choices| choices.answered_by(&canonical))
            {
                service.room.pending_choices = None;
            }
            service.room.accepted_options = Some(canonical);
            if changed {
                service.room.last_ready_request = None;
                crate::app::shell_network_setup::invalidate_network_agreement(state)?;
            }
            let generated = p.options.random_map.clone();
            if !state.match_state.session.host() {
                // VERA: Quick Match plays the original game, whatever rules
                // its host sends.
                if super::lobby::quick_match_runner(state) {
                    p.options.launch.options.host_rules = Default::default();
                }
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
            let (text, localized) = p.shown_text();
            if p.visit {
                service_event(
                    state,
                    WolServiceEvent::VisitMessage {
                        text,
                        localized,
                        from: p.from.into_member(&local),
                        private: p.private,
                        action: p.action,
                        sent_at: p.sent_at,
                    },
                );
                return Ok(());
            }
            let options = p
                .hosted_pairing(&local)
                .and_then(|pairing| super::quick_match::online_start_options(state, pairing));
            if p.announces_release() {
                lobby_says_outdated(state);
            }
            service_event(
                state,
                WolServiceEvent::Message {
                    from: p.from.into_member(&local),
                    text,
                    localized,
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
            let admitted = service.receive_lobby_start(payload, |service, start| {
                // The lobby ended a visit to the public lobby for the match.
                super::lobby::visit_ends(state);
                begin_start(state, service, start)
            });
            if let Err(error) = admitted {
                if !service.locally_refused_start {
                    return Err(error);
                }
                log::warn!("Online match start refused locally: {error:#}");
                let line = if error.is::<StartDataMismatch>() {
                    "VERA:MatchStartMismatch"
                } else if error.is::<std::io::Error>() {
                    "VERA:RelayConnectionFailed"
                } else {
                    "VERA:StartRefusedSetup"
                };
                if state.frontend.network_setup_protocol.is_some() {
                    fail_start(state, &tr(line));
                } else {
                    // A delayed callback after leaving its setup may finish
                    // only its match. Never leave the player's current room.
                    notice(state, &tr(line));
                }
            }
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
            if std::mem::take(&mut service.locally_refused_start) {
                // Our finish (or a simultaneous peer cancellation) released
                // this start. Keep the reason this game already showed.
                return Ok(());
            }
            let reason = payload["message"].as_str().unwrap_or_default();
            let names = lobby_names(&payload);
            log::warn!(
                "The lobby refused the online start: {reason} {}",
                names.as_deref().unwrap_or_default()
            );
            if service.pending.is_none()
                && crate::app::shell_network_setup::verify(state).is_none()
                && state.frontend.network_setup_protocol.is_none()
            {
                // This game already failed the start and left the room: only
                // the lobby's names are new.
                if let Some(names) = names {
                    crate::app::shell_wol::add_problem_line(state, names);
                }
                return Ok(());
            }
            let line = tr(service.start_rejected_line(reason));
            let text = names.map_or_else(|| line.clone(), |names| format!("{line} {names}"));
            cancel_pending(state, service);
            fail_start(state, &text);
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
    // A watched match is no match of this player's: nothing finishes it.
    if service.watching.is_none() {
        service.hold_match(Some(match_id));
    }
    service.pending = None;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct StartDataMismatch(&'static str);

fn begin_start(state: &mut AppState, service: &mut LiveService, start: wire::Start) -> Result<()> {
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
        StartDataMismatch("client version differs")
    );
    ensure!(
        agreement["rules_digest"].as_str() == Some(&start.rules_digest),
        StartDataMismatch("rules differ")
    );
    ensure!(
        local_map_status(state, service) == 1,
        StartDataMismatch("local map differs")
    );
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

/// The lobby's `watch_started` (docs/lobby-protocol.md "Watching"): this
/// game sets up the start every player's game set up, standing in the seat
/// [`followed_user`] picks. The match's map then arrives as a guest's would,
/// and [`poll_watch`] asks the relay to let this game in.
fn prepare_watch(state: &mut AppState, started: wire::WatchStarted) -> Result<WatchStart> {
    let grant = decode_watch_grant(&started.relay, &started.match_id)?;
    for (index, user) in started.users.iter().enumerate() {
        ensure!(
            usize::from(user.source_index) == index,
            "noncanonical source index"
        );
    }
    let hosts: Vec<_> = started
        .users
        .iter()
        .filter(|user| user.flags & 1 != 0)
        .collect();
    ensure!(hosts.len() == 1, "start requires one admitted host");
    let host_name = hosts[0].name.clone();
    let followed = followed_user(&started.users, &started.options.launch.humans)
        .ok_or_else(|| anyhow!("the match has no player to follow"))?
        .to_owned();
    let names: Vec<_> = started.users.iter().map(|user| user.name.clone()).collect();
    let options = started.options.into_options(&followed)?;
    crate::app::shell_network_setup::install_watch_setup(
        state,
        options,
        crate::app::match_runtime::session::NetworkRoom {
            tournament: started.tournament != 0,
            clan: false,
        },
    )?;
    state
        .match_state
        .session
        .canonicalize_online_roster(&names)?;
    Ok(WatchStart {
        version: started.version,
        rules_digest: started.rules_digest,
        host_name,
        users: started
            .users
            .into_iter()
            .map(|user| (user.name, user.flags))
            .collect(),
        seed: started.seed,
        address: started.relay.address,
        grant,
        pings: started.pings,
        // The lobby lists no clan room, whose start splits its
        // players by squad.
        terms: crate::app::match_runtime::session::WolStartTerms {
            tournament: started.tournament,
            squad_lists: Default::default(),
            quick_coop: started.quick_coop,
        },
    })
}

/// The user whose seat a watcher takes: the first in source order who plays
/// rather than observes, or, where only AI play and everyone observes, the
/// first one, whose seat shows the observer's sidebar and the whole map.
fn followed_user<'a>(
    users: &'a [wire::WatchedUser],
    humans: &[crate::skirmish_launch::SkirmishHumanSlot],
) -> Option<&'a str> {
    let plays = |user: &&wire::WatchedUser| {
        humans
            .iter()
            .any(|human| human.name.eq_ignore_ascii_case(&user.name) && !human.is_observer())
    };
    users
        .iter()
        .find(plays)
        .or_else(|| {
            users
                .first()
                .filter(|_| humans.iter().all(|human| human.is_observer()))
        })
        .map(|user| user.name.as_str())
}

/// A watch whose map is here asks its relay to let this game in, as a
/// returning player's game does, with the timing every player's game agreed.
fn poll_watch(state: &mut AppState, service: &mut LiveService) -> Result<()> {
    if service.pending.is_some()
        || service
            .watching
            .as_ref()
            .is_none_or(|watching| watching.start.is_none())
    {
        return Ok(());
    }
    ensure!(!service.maps.failed(), "the match's map did not arrive");
    if local_map_status(state, service) != 1 {
        return Ok(());
    }
    let watching = service.watching.as_mut().expect("checked watch");
    let start = watching.start.take().expect("checked start");
    let match_id = watching.match_id.clone();
    let agreement = crate::app::shell_network_setup::agreement::local(state)?;
    ensure!(
        agreement.version == start.version && agreement.rules_digest == start.rules_digest,
        "this game's version or rules differ from the match's"
    );
    admit_relay_start(
        state,
        service,
        RelayStart {
            match_id,
            host_name: start.host_name,
            users: start.users,
            seed: start.seed,
            address: start.address,
            gateway: None,
            grant: start.grant,
            resume_pings: Some(start.pings),
        },
    )?;
    // The match's own terms, not those of the lobby this game is in.
    state.match_state.session.set_wol_start(start.terms);
    Ok(())
}

/// A watch that could not begin: the lobby frees its place, the setup and
/// any relay route close, and the lobby says why.
fn fail_watch(state: &mut AppState, service: &mut LiveService, key: &str) {
    cancel_watch_start(state, service);
    super::lobby::watch_failed(state, key);
}

fn cancel_watch_start(state: &mut AppState, service: &mut LiveService) {
    service.stop_watching();
    end_room(state, service);
    if state.match_state.session.network_setup().is_some()
        || state.frontend.network_setup_protocol.is_some()
    {
        crate::app::shell_network_setup::abandon_for_wol_runner(state);
    }
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
            max_peers: crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT - 1,
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
        relay_reported: false,
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

/// A returning player and a spectator have the match's timing already. The
/// spectator stays in the watch wait until its local start really succeeded.
fn enter_restored_start(
    state: &mut AppState,
    service: &mut LiveService,
    match_id: String,
    pings: &[i32],
) -> Result<()> {
    crate::app::lan_service::mark_online_restore(state)?;
    let start = enter_timed_start(state, service, match_id, pings);
    if service.watching.is_none() {
        return start;
    }
    match start {
        Ok(()) => {
            super::lobby::watch_admitted(state);
        }
        Err(error) => {
            log::warn!("Could not start the watched match: {error:#}");
            fail_watch(state, service, "VERA:WatchCouldNotStart");
        }
    }
    Ok(())
}

fn poll_relay(state: &mut AppState, service: &mut LiveService) -> Result<()> {
    let mut commands = Vec::new();
    // The caller fails the start first once its deadline passes
    // (`LiveService::expired_admission`).
    let Some(pending) = service.pending.as_mut() else {
        return Ok(());
    };
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
            // A returning player, or a watcher: the match is under way.
            if let Some(pings) = pending.resume_pings.take() {
                return enter_restored_start(state, service, match_id, &pings);
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

#[cfg(all(test, tests_app))]
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
                    ordered_alone: false,
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
            ordered_alone: false,
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

    /// A watcher stands in the first player's seat, and where only AI play and
    /// everyone watches, in the first observer's.
    #[test]
    fn a_watcher_follows_the_first_player_or_where_only_ai_play_an_observer() {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(fixture["launch"].clone()).unwrap();
        let human = |name: &str, observer| {
            let mut human = launch.humans[0].clone();
            human.name = name.into();
            human.set_observer(observer);
            human
        };
        let users = ["Host", "Guest"].map(|name| wire::WatchedUser {
            name: name.into(),
            flags: 0,
            source_index: 0,
        });
        let followed = |host_plays: bool, guest_plays: bool| {
            followed_user(
                &users,
                &[human("Host", !host_plays), human("Guest", !guest_plays)],
            )
        };
        assert_eq!(followed(false, true), Some("Guest"));
        assert_eq!(followed(false, false), Some("Host"));
    }

    /// A watch grant admits to the watched match's room only.
    #[test]
    fn a_watch_grant_is_for_its_match_only() {
        use crate::net::udp::{
            RelayWatchTicket,
            test_support::{SecretKey, derive_packet_key},
        };
        let signing_key = SecretKey::new([9; 32]);
        let ticket = RelayWatchTicket {
            room: [0x11; 16],
            expires: 300,
            nonce: [3; 16],
            watcher: [4; 16],
            delay_seconds: 120,
        }
        .sign(&signing_key)
        .unwrap();
        let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let grant = wire::WatchGrant {
            address: "127.0.0.1:5000".parse().unwrap(),
            ticket: encode(&ticket),
            packet_key: encode(derive_packet_key(&signing_key, &ticket).expose_for_https()),
            payload_key: encode(&[7; 32]),
            ordering_certificate: encode(
                &crate::net::udp::OrderingCertificate {
                    room: [0x11; 16],
                    sources_mask: 3,
                    tick_hz: 45,
                    expires: 86400,
                    pacing: Default::default(),
                    ordered_alone: false,
                }
                .sign(&signing_key)
                .unwrap(),
            ),
        };
        let watched = "11111111-1111-1111-1111-111111111111";
        assert_eq!(
            decode_watch_grant(&grant, watched).unwrap().payload_key,
            [7; 32]
        );
        assert!(decode_watch_grant(&grant, "33333333-3333-3333-3333-333333333333").is_err());
    }

    fn control_service(
        match_id: Option<&str>,
    ) -> (LiveService, Receiver<http::Request>, Sender<http::Response>) {
        let (requests, input) = mpsc::channel();
        let (output, responses) = mpsc::channel();
        let mut service = LiveService::with_worker(
            "https://lobby.test".into(),
            Arc::new(AtomicBool::new(false)),
            requests,
            responses,
        );
        service.next_request = 5;
        service.match_id = match_id.map(str::to_owned);
        (service, input, output)
    }

    #[test]
    fn a_refused_watch_releases_its_grant_without_finishing_the_players_match() {
        let (mut service, input, _output) = control_service(None);
        let id = "11111111-1111-1111-1111-111111111111";
        // A grant with no start that this game can read.
        let error = service
            .receive_watch_start(id, true, || {
                let _: wire::WatchStarted = decode(json!({"match_id": id}))?;
                unreachable!()
            })
            .unwrap_err();
        assert_eq!(error.to_string(), "invalid lobby message");
        assert!(
            matches!(input.try_recv(), Ok(http::Request::Command { operation: "unwatch", payload, .. })
            if payload == json!({"match_id": id}))
        );
        assert!(service.watching.is_none());
        assert!(!service.match_unfinished());
        assert!(input.try_recv().is_err());
    }

    #[test]
    fn a_stale_or_repeated_watch_grant_preserves_the_current_watch() {
        let (mut service, input, _output) = control_service(None);
        let current = "11111111-1111-1111-1111-111111111111";
        let old = "22222222-2222-2222-2222-222222222222";
        service.watching = Some(Watching {
            match_id: current.into(),
            start: None,
        });
        service
            .receive_watch_start(old, false, || panic!("stale grant"))
            .unwrap();
        assert!(
            matches!(input.try_recv(), Ok(http::Request::Command { operation: "unwatch", payload, .. })
            if payload == json!({"match_id": old}))
        );
        service
            .receive_watch_start(current, false, || panic!("duplicate"))
            .unwrap();
        assert!(input.try_recv().is_err());
        assert_eq!(service.watching.as_ref().unwrap().match_id, current);
    }

    #[test]
    fn a_locally_refused_lobby_start_finishes_only_the_announced_match() {
        let (mut service, input, _output) = control_service(None);
        let id = "11111111-1111-1111-1111-111111111111";
        // The lobby has committed a room revision that this guest cannot
        // adopt. It never opens the relay or reports relay_ready.
        let start = json!({"match_id": id, "revision": 9, "version": "test", "rules_digest": "",
            "seed": 1, "users": [], "relay": {"address":"127.0.0.1:4000", "source_index":0,
            "packet_key":"", "payload_key":"", "ticket":"", "ordering_certificate":""}});
        let error = service
            .receive_lobby_start(start, |_, _| panic!("stale revision"))
            .unwrap_err();
        assert_eq!(error.to_string(), "stale options revision");
        assert!(matches!(input.try_recv(), Ok(http::Request::HoldMatch(Some(held))) if held == id));
        assert!(
            matches!(input.try_recv(), Ok(http::Request::Command { operation: "finish", payload, .. }) if payload == json!({"match_id": id}))
        );
        assert!(service.locally_refused_start);
        // A later callback cannot replace the identity under its finish.
        service
            .receive_lobby_start(
                json!({"match_id": "22222222-2222-2222-2222-222222222222"}),
                |_, _| panic!("already held"),
            )
            .unwrap();
        assert!(input.try_recv().is_err());
        assert_eq!(service.match_id.as_deref(), Some(id));
    }

    #[test]
    fn a_game_started_without_a_match_identity_finishes_nothing() {
        let (mut service, input, _output) = control_service(None);
        assert!(
            service
                .receive_lobby_start(json!({"match_id": "other"}), |_, _| panic!("bad identity"))
                .is_err()
        );
        assert!(service.match_id.is_none());
        assert!(!service.locally_refused_start);
        assert!(input.try_recv().is_err());
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

    /// The lobby drops private chat to anyone outside the sender's room: a
    /// page with no listed target fails unsent, and a reply never goes to
    /// the whole room.
    #[test]
    fn a_page_or_reply_without_a_listed_target_fails_unsent() {
        let (mut service, input, _output) = control_service(None);
        let visit = super::super::WolVisit::open(false);
        let units = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        let page = WolChatLine::Page {
            name: units("Bob"),
            text: units("hi"),
        };
        let reply = WolChatLine::Reply { text: units("hi") };
        for line in [page, reply] {
            let result = send_chat(&mut service, Some(&visit), line, Vec::new(), false);
            let sent = input.try_recv().ok().map(|request| match request {
                http::Request::Command { payload, .. } => payload,
                _ => Value::Null,
            });
            assert_eq!((result, sent), (Some(Err(0x8000_4005)), None));
        }
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

    /// The lobby's new-release line asks the updater, even with a value this
    /// game cannot show; a player's line with the same words, or another
    /// lobby line, does not.
    #[test]
    fn the_lobbys_new_release_line_announces_a_release() {
        let line = |name: &str, phrases: Value| -> Chat {
            decode(json!({
                "from": {"player_id": "00000000-0000-0000-0000-000000000000", "name": name, "flags": 1},
                "text": "VERA20k 0.3.7 is out, and you have 0.3.6.",
                "private": false,
                "action": false,
                "sent_at": 1,
                "phrases": phrases,
            }))
            .unwrap()
        };
        let release = json!([{"key": "NewRelease", "values": {"newest": "0.3.7", "later": true}}]);
        assert!(line("VERA20k", release).announces_release());
        assert!(!line("Tanya", Value::Null).announces_release());
        assert!(!line("VERA20k", json!([{"key": "OnlyPlayerOnline"}])).announces_release());
    }

    /// Only a line shown from the lobby's phrases counts as localized, which
    /// is what keeps it from translation into the interface language; a
    /// player's line, or phrases this game cannot show, keep the text.
    #[test]
    fn only_a_line_shown_from_the_lobbys_phrases_is_localized() {
        let line = |phrases: Value| -> (String, bool) {
            let chat: Chat = decode(json!({
                "from": {"player_id": "p", "name": "VERA20k", "flags": 1},
                "text": "the lobby's text",
                "phrases": phrases,
            }))
            .unwrap();
            let (text, localized) = chat.shown_text();
            (String::from_utf16(&text).unwrap(), localized)
        };
        let hidden = "1 game is hidden because it runs another VERA20k version.";
        assert_eq!(
            line(json!([{"key": "GamesHidden", "values": {"count": 1}}])),
            (hidden.to_owned(), true)
        );
        let text = ("the lobby's text".to_owned(), false);
        assert_eq!(line(Value::Null), text);
        assert_eq!(line(json!([{"key": "NotYetKnown"}])), text);
    }

    #[test]
    fn lobby_upgrade_notice_names_the_download_once_per_sign_in() {
        let (mut service, _input, _output) = control_service(None);
        let reason = "Online play needs VERA20k 0.3.0 or newer.";
        assert_eq!(
            service.outdated_notice(Some(reason)).as_deref(),
            Some(format!("{reason} https://vera20k.org/").as_str())
        );
        assert_eq!(service.outdated_notice(Some(reason)), None);
        service.auth_attempt += 1;
        assert_eq!(
            service.outdated_notice(None).as_deref(),
            Some("Update VERA20k to use secure online multiplayer. https://vera20k.org/")
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
        let first = finish_payload_sent(&mut lobby, &input);
        assert_eq!(
            first,
            json!({"match_id":"lobby-match","result":"won","winners":["Tanya","Boris"],"ended_ago":90})
        );
        let mut retried = |failure, at: u64| {
            assert!(!lobby.finish_failed(5, failure, Instant::now()));
            lobby.poll_cleanup(Instant::now() + Duration::from_secs(at));
            let http::Request::Command { payload, .. } = input.try_recv().unwrap() else {
                panic!("finish retry");
            };
            payload
        };
        // The lobby replays a committed request ID only with the payload it
        // committed: a finish whose answer was lost goes again as first sent.
        assert_eq!(retried(http::CommandFailure::Retryable, 3), first);
        // A lobby from before `ended_ago` refuses it, and one from before team
        // ladders `winners`: the finish goes again without the newest.
        let rejected = http::CommandFailure::Rejected(0x8000_4005);
        assert_eq!(
            retried(rejected, 5),
            json!({"match_id":"lobby-match","result":"won","winners":["Tanya","Boris"]})
        );
        assert_eq!(
            retried(rejected, 10),
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
        service.hold_match(Some("cancelled-match".into()));
        assert!(matches!(
            input.try_recv(),
            Ok(http::Request::HoldMatch(None))
        ));
        assert!(matches!(
            input.try_recv(),
            Ok(http::Request::HoldMatch(Some(_)))
        ));
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
        service.pending = Some(start_waiting_until(now + RELAY_TIMEOUT, true));
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

    fn start_waiting_until(deadline: Instant, relay_acknowledged: bool) -> PendingStart {
        PendingStart {
            match_id: "m".into(),
            host_name: "Host".into(),
            transport: None,
            deadline,
            relay_acknowledged,
            relay_reported: false,
            probes_started: false,
            latencies_reported: false,
            resume_pings: None,
            report: None,
        }
    }

    /// A start that runs out says whose game it waited on: one whose own
    /// relay answered used to read that it could not connect.
    #[test]
    fn an_expired_start_says_whose_game_could_not_reach_the_relay() {
        let (mut service, input, _output) = control_service(None);
        let now = Instant::now();
        let expired = now + RELAY_TIMEOUT;
        service.pending = Some(start_waiting_until(expired, false));
        assert_eq!(service.expired_admission(now), None);
        assert_eq!(
            service.expired_admission(expired),
            Some("VERA:StartRelayNotReached")
        );
        service.pending = Some(start_waiting_until(expired, true));
        service.send_report("relay_ready", json!({"match_id":"m"}), now);
        assert_eq!(
            service.expired_admission(expired),
            Some("VERA:StartLobbyBusy"),
            "the lobby never took its relay_ready"
        );
        let Ok(http::Request::Command { id, .. }) = input.try_recv() else {
            panic!("relay_ready");
        };
        service.report_completed(id);
        assert_eq!(
            service.expired_admission(expired),
            Some("VERA:StartRelayNotReachedByOther")
        );
        service.pending.as_mut().unwrap().probes_started = true;
        assert_eq!(
            service.expired_admission(expired),
            Some("VERA:StartLatencyCheckFailed")
        );
    }

    /// The lobby's reasons for refusing a start show as their own lines; one
    /// this game does not know keeps the general line.
    #[test]
    fn a_refused_start_shows_the_lobbys_reason() {
        let (mut service, _input, _output) = control_service(None);
        let relay = "A player could not connect to the gameplay relay.";
        assert_eq!(
            service.start_rejected_line("A player left before the game started."),
            "VERA:StartPlayerLeft"
        );
        assert_eq!(
            service.start_rejected_line(relay),
            "VERA:StartRefusedConnection",
            "no start pending here"
        );
        service.pending = Some(start_waiting_until(Instant::now(), true));
        assert_eq!(
            service.start_rejected_line(relay),
            "VERA:StartRelayNotReached",
            "the lobby never took its relay_ready"
        );
        service.pending.as_mut().unwrap().relay_reported = true;
        assert_eq!(
            service.start_rejected_line(relay),
            "VERA:StartRelayNotReachedByOther"
        );
        assert_eq!(
            service.start_rejected_line("A newer reason."),
            "VERA:StartRefusedConnection"
        );
        // The names a timed-out start's `start_rejected` carries, as
        // `service.js` sends them; a released lobby sends none.
        let names = serde_json::json!({"message": relay, "phrases": [{"key": "StartNotConnected",
            "values": {"names": {"and": ["Alex", "Sam"]}, "count": 2}}]});
        assert_eq!(
            lobby_names(&names).as_deref(),
            Some(
                "Alex and Sam did not connect to the game server in time, so the game did not start."
            )
        );
        assert_eq!(lobby_names(&serde_json::json!({"message": relay})), None);
        for key in [
            "VERA:StartRelayNotReached",
            "VERA:StartRelayNotReachedByOther",
            "VERA:StartLatencyCheckFailed",
            "VERA:StartPlayerLeft",
            "VERA:StartPlayerCancelled",
        ] {
            assert!(crate::i18n::find(key).is_some(), "{key}");
        }
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
        assert!(shown.is_some_and(|(shown, target, reply)| {
            shown == line("A") && target == ChatLanguage::SimplifiedChinese && reply.text == "冲"
        }));
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
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch, true, None, false)
            .unwrap();
        session
    }

    #[test]
    fn a_legacy_reply_can_only_retry_its_unchanged_publication_once() {
        let (mut service, input, _) = control_service(None);
        let options: wire::Options = serde_json::from_value(
            crate::test_fixture::json("tests/fixtures/lobby/options.json")["rooms"]["cooperative"]
                .clone(),
        )
        .unwrap();
        let publish = |service: &mut LiveService| {
            service.publish_room_options(options.clone());
            let http::Request::Command { id, payload, .. } = input.try_recv().unwrap() else {
                panic!("expected the room options");
            };
            (id, payload["options"].clone())
        };
        let (old, _) = publish(&mut service);
        let (latest, sent) = publish(&mut service);
        assert!(!service.take_legacy_refusal(old, false, || Some(sent.clone())));
        assert!(!service.take_legacy_refusal(latest, true, || Some(sent.clone())));
        let (edited, mut other) = publish(&mut service);
        other["launch"]["options"]["short_game"] = json!(!options.launch.options.short_game);
        assert!(!service.take_legacy_refusal(edited, false, || Some(other)));
        let (unchanged, sent) = publish(&mut service);
        assert!(service.take_legacy_refusal(unchanged, false, || Some(sent.clone())));
        assert!(!service.take_legacy_refusal(unchanged, false, || Some(sent)));
    }

    #[test]
    fn host_options_need_one_map_start_per_playing_team() {
        let cases = crate::test_fixture::json("tests/fixtures/lobby/options.json");
        let mut room: wire::Options =
            serde_json::from_value(cases["rooms"]["fifteen_teams_of_thirty"].clone()).unwrap();
        assert!(check_map_capacity(&room).is_ok());
        room.map_metadata.player_capacity = 14;
        assert_eq!(check_map_capacity(&room).unwrap_err().0, 14);
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

    /// What `end_room` stops for a guest who left the room, signed out or
    /// lost the lobby before the host answered: the answer's time running out
    /// restores no settings and shows no notice. Its call sites need an
    /// `AppState`, which unit tests cannot build.
    #[test]
    fn a_room_this_game_is_done_with_waits_for_no_answer() {
        let (mut service, _input, _output) = control_service(None);
        let deadline = Instant::now() + RELAY_TIMEOUT;
        let waiting = |service: &mut LiveService| {
            service.room.published_options = Some((7, json!({})));
            service.room.pending_choices = Some(PendingChoices {
                request_id: 8,
                expected_human: json!({}),
                expected_random: json!({}),
                deadline,
            });
        };
        service.room.accepted_options = Some(json!({}));
        waiting(&mut service);
        assert!(
            service.room.choices_unanswered(deadline),
            "still in the room"
        );
        waiting(&mut service);
        service.room.stop_waiting();
        assert!(!service.room.choices_unanswered(deadline));
        assert!(
            !service.is_room_publication(7),
            "a late refusal changes nothing"
        );
        assert!(
            service.room.accepted_options.is_some(),
            "a late options echo still compares"
        );
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
        visit.match_returned((None, None, None), now, false);
        visit.match_returned((None, None, None), now, false); //Repeated abort callbacks keep one intent.
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
        visit.match_returned((None, None, None), now, false);
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
        assert!(matches!(
            input.try_recv(),
            Ok(http::Request::HoldMatch(None))
        ));
        service.clear_cleanup(); //Definitive Offline has retired the authority.
        visit.match_returned((None, None, None), now, false);
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
        visit.live_service = Some(service);
        let now = Instant::now();
        visit.match_returned((None, None, None), now, true);
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
    fn closing_the_game_finishes_its_match_with_the_recorded_result() {
        // The game closes during a match from a kept room, before the shell
        // return finished it: the finish carries the result the game
        // recorded for the player who left, and asks for no room.
        let (service, input, _output) = control_service(Some("closed-match"));
        let mut visit = super::super::WolVisit::open(false);
        visit.provider = WolProvider::Online;
        visit.live_service = Some(service);
        let now = Instant::now();
        visit.match_left_for_exit((Some("quit"), None, Some(now)), now);
        let Ok(http::Request::Command {
            operation: "finish",
            payload,
            ..
        }) = input.try_recv()
        else {
            panic!("finish command");
        };
        assert_eq!(
            payload,
            json!({"match_id":"closed-match","result":"quit","ended_ago":0})
        );
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

    /// The worker holds the match through Quick Match's leave, and a new
    /// sign-in lets it finish that match with the bearer it logs out before
    /// the service drops it.
    #[test]
    fn the_worker_holds_the_match_through_a_leave_until_the_next_sign_in() {
        let (mut service, input, _output) = control_service(None);
        service.hold_match(Some("quick-match".into()));
        service.leave_room(Instant::now());
        service.leave_completed(5);
        service.sign_in("Tanya".into(), http::Credential::Guest);
        let sent: Vec<_> = input
            .try_iter()
            .map(|request| match request {
                http::Request::HoldMatch(id) => format!("hold {id:?}"),
                http::Request::Command { operation, .. } => operation.to_owned(),
                http::Request::OpenSession { .. } => "sign in".to_owned(),
                _ => "other".to_owned(),
            })
            .collect();
        assert_eq!(
            sent,
            [
                "hold Some(\"quick-match\")",
                "leave",
                "sign in",
                "hold None"
            ]
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
        assert!(matches!(
            input.try_recv(),
            Ok(http::Request::HoldMatch(None))
        ));
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
        assert!(cleanup.pending() && !cleanup.logout_acknowledged());
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
                    lobby_visits: false,
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
        drop(output);
        assert!(cleanup.poll(now + Duration::from_secs(3)).is_none());
        assert!(!cleanup.pending() && cleanup.logout_acknowledged());
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
        assert!(matches!(
            input.try_recv(),
            Ok(http::Request::HoldMatch(None))
        ));
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
        const LATE: &str = "22222222-2222-2222-2222-222222222222";
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
                    payload: json!({"match_id":LATE})
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
        assert!(matches!(
            input.try_recv(),
            Ok(http::Request::HoldMatch(Some(id))) if id == LATE
        ));
        let http::Request::Command {
            operation: "finish",
            payload,
            ..
        } = input.try_recv().unwrap()
        else {
            panic!("late match finish");
        };
        assert_eq!(payload, json!({"match_id":LATE}));
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
