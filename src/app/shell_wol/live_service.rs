//! Production VERA lobby adapter. HTTPS results enter the existing WOL reducers;
//! MatchSession remains the sole authority for accepted participants and options.
//! The custom protocol is documented in docs/lobby-protocol.md.

mod events;
mod http;
mod maps;
mod match_start;
mod operations;
mod requests;
mod responses;
mod sign_in;
mod wire;

use crate::app::shell_route::ShellRoute;
use crate::util::time::{Duration, Instant};
use std::collections::BTreeMap;
use std::net::SocketAddrV4;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, anyhow, ensure};
use serde_json::Value;

use super::{
    NET_STATUS_LOST, WolPage, WolVisit, in_match, lobby, lobby_phrases, lobby_watches, quick_match,
    service, translation,
};
use super::{
    WolApiStatus, WolLoginAccepted, WolProvider, WolServer, WolServerList, WolServiceEvent,
    WolServiceRequest, service_event,
};
use crate::app::AppState;
use crate::app::shell_network_setup::{NetworkSetupEvent, NetworkSetupRequest};
use crate::i18n::tr;
use crate::net::udp::LanUdpTransport;
use crate::ui::chat_translation::{ChatLanguage, ChatReply, only_repeats, without_note};
use crate::ui::wol_account::{AccountRequest, AccountSuccess};
use events::apply_event;
pub(crate) use events::{decode_external_watch_grant, relay_ticket_is_watch};
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
pub(super) use http::browser_stats;
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use match_start::discard_spawned_visit;
use match_start::{
    StartDataMismatch, begin_start, cancel_watch_start, enter_timed_start, fail_watch, poll_relay,
    poll_spawned_exit, poll_watch, prepare_watch, spawned_visit_mut,
};
pub(crate) use match_start::{begin_spawned_match, end_spawned_match};
pub(crate) use requests::local_choices_agreed;
use requests::{
    MapTooSmall, accepted_choices_match, canonical_options, local_agreement, local_map_status,
    options, poll_start_preparation, request_service, request_setup, restore_accepted_options,
};
use responses::{
    Chat, apply_response, decode, decode_relay_grant, decode_watch_grant, relay_uuid,
    validate_online_identity,
};
pub(crate) use responses::{DecodedRelayGrant, decode_external_relay_grant};
pub(super) use sign_in::remembered_name;
use sign_in::{
    forget_deleted_account, forget_sign_in, remember_online_name, remember_sign_in, settings_dir,
    sign_in_credential,
};

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
/// the relay already rejects the ticket, so renewal and lapse are timed from
/// receipt.
/// It uses frame_pacer::wall_clock_ms, which keeps counting through sleep;
/// an `Instant` deadline would wait out the slept time with a dead ticket.
const RELAY_GRANT_LIFETIME: Duration = Duration::from_secs(300);
/// docs/lobby-protocol.md: renew at half-life. The relay refuses an expired ticket, so a
/// lobby outage longer than this margin freezes the match. Retrying every 10 s
/// spends 6 of renew_relay's 12 a minute (docs/lobby-protocol.md).
const RELAY_RENEWAL_MARGIN: Duration = Duration::from_secs(150);
const RELAY_RENEWAL_RETRY_MS: u32 = 10_000;
/// How long a launcher's watcher waits for its relay to let it in: the
/// relay's loading limit (`LOADING_LIMIT_MS`) and some more.
const WATCH_ADMISSION_TIMEOUT: Duration = Duration::from_secs(240);
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
    /// Deletions completed while a visit was closing. The main thread still
    /// clears local sign-ins, without forwarding an old reply to a new form.
    deleted_accounts: Vec<(String, String)>,
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
        if service.spawned_watch.is_some() {
            // A launcher's watcher has nothing to finish, leave or log out
            // (docs/spawn.md "Watching"): its worker just stops.
            service.stopping.store(true, Ordering::Release);
        } else if let Some(pending) = service.pending.take()
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
            if let http::Response::AccountAnswered {
                answer: Ok(AccountSuccess::Deleted(name)),
                ..
            } = response
            {
                self.deleted_accounts
                    .push((retired.service.lobby.clone(), name));
                continue;
            }
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
    // A visitor's join. The server validates the destination before leaving
    // the original room. Resend the same ID when the outcome is uncertain.
    switching_room: Option<RetryingCommand>,
    finishing: Option<RetryingCommand>,
    leaving: Option<RetryingCommand>,
    logging_out: Option<RetryingCommand>,
    /// Renewal deadlines on the low 32 bits of frame_pacer::wall_clock_ms.
    next_renewal_attempt: Option<u32>,
    /// Renewal deadline of the newest gameplay grant received.
    relay_renewal_due: Option<u32>,
    /// When the newest gameplay grant lapses: its receipt plus
    /// [`RELAY_GRANT_LIFETIME`]. A lobby session that ends mid-match
    /// (`clear_cleanup`) keeps it: the match plays on until the pass lapses.
    relay_lapses_at: Option<u32>,
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
    /// VERA: the match this game watches, from its `watch_started` until it
    /// leaves with `unwatch`.
    watching: Option<Watching>,
    /// The match a launcher's watcher follows (docs/spawn.md "Watching"):
    /// its bearer only polls for `watch_renewed`, and this service sends no
    /// command, report, finish or leave.
    spawned_watch: Option<String>,
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

/// This game's relay pass lapsed by `now_ms` (frame_pacer::wall_clock_ms).
pub(crate) fn relay_pass_lapsed(state: &AppState, now_ms: u32) -> bool {
    state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.live_service.as_ref())
        .is_some_and(|service| service.relay_pass_lapsed(now_ms))
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
    for (lobby, name) in std::mem::take(&mut state.frontend.online_cleanup.deleted_accounts) {
        forget_deleted_account(state, &lobby, &name);
    }
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
        if let Some(switch) = &mut service.switching_room {
            if super::lobby::online_request_pending(state, switch.request_id) {
                if let Some((id, operation, payload)) = switch.take_due(Instant::now()) {
                    service.send_command(id, operation, payload);
                }
                // Keep A's queued setup and map traffic in its owner until
                // we know which room owns the session. On refusal it resumes;
                // on admission the old setup is discarded with channel_left.
                put_service(state, service);
                return Some(Instant::now() + POLL_INTERVAL);
            }
            service.switching_room = None;
        }
        if service.watching.is_some() || service.spawned_watch.is_some() {
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
                            &state.csf_label("TXT_SCENARIO_TOO_SMALL", "TXT_SCENARIO_TOO_SMALL"),
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
        poll_maps(state, &mut service);
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

/// The room's map transfer runs only between matches.
fn poll_maps(state: &mut AppState, service: &mut LiveService) {
    if !service.spawned && service.pending.is_none() && service.match_id.is_none() {
        maps::poll(state, service);
    }
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
        ShellRoute::NetworkSetup => setup_open,
        ShellRoute::Wol => quick_match_open,
        ShellRoute::WolLobby => lobby_open && !awaiting_room,
        _ => false,
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
    state.match_state.session.take_network_room_return();
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

#[cfg(all(test, tests_app))]
mod completion_tests;
