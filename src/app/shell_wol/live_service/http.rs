//! Blocking HTTPS is confined to this worker. Session bearer tokens never
//! enter Debug output, file storage, error strings, or the gameplay client state.

use crate::util::time::{Duration, Instant};
use base64::Engine;
use std::collections::VecDeque;
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
use std::io::Read;
use std::path::PathBuf;
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::wire::EventBatch;
use super::wire::{ChannelList, Envelope, Session};
use serde_json::Value;
use serde_json::json;

pub(super) enum Request {
    ReadMapUpload {
        transfer: u64,
        source: crate::map::source::TransferMapSource,
        digest: String,
        total: usize,
    },
    MapChunk {
        transfer: u64,
        resource: crate::map::source::TransferResource,
        seed_digest: String,
        digest: String,
        offset: usize,
    },
    UploadMapChunk {
        transfer: u64,
        resource: crate::map::source::TransferResource,
        id: u64,
        digest: String,
        total: usize,
        offset: usize,
        bytes: Vec<u8>,
    },
    CachedMap {
        transfer: u64,
        resource: crate::map::source::TransferResource,
        directory: PathBuf,
        digest: String,
        total: usize,
    },
    InstallMap {
        transfer: u64,
        resource: crate::map::source::TransferResource,
        directory: PathBuf,
        digest: String,
        total: usize,
        bytes: Vec<u8>,
    },
    OpenSession {
        attempt: u64,
        nickname: String,
    },
    /// The shell gave up on an unanswering lobby outside a match: stop
    /// polling and log this bearer out at the next sign-in.
    Abandon,
    /// An external launch's bearer for its match API (docs/spawn.md), in
    /// place of `/v1/sessions`. Polling starts with the first event.
    AdoptSession {
        token: String,
        /// A returning player's match and the end of its event history; the
        /// `timing_confirmed` before it belongs to the process that left.
        resume: Option<(String, u64)>,
    },
    Channels(u32),
    Command {
        id: u64,
        operation: &'static str,
        payload: Value,
    },
    /// `POST /v1/translate`, answered by [`Response::Translated`].
    Translate {
        id: u64,
        text: String,
        target: &'static str,
    },
}

impl Request {
    /// The periodic requests a waiting copy already covers: the host's 5 s
    /// `ping` and the 10 s relay renewal retry.
    fn repeats(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Command {
                    operation, payload, ..
                },
                Self::Command {
                    operation: other_operation,
                    payload: other_payload,
                    ..
                },
            ) => {
                matches!(*operation, "ping" | "renew_relay")
                    && operation == other_operation
                    && payload == other_payload
            }
            _ => false,
        }
    }
}

/// Queues what arrived during the last request behind `queued`, without a
/// periodic request that `current` or a queued one repeats. A failing command
/// takes about 24 s, so copies would otherwise pile up ahead of Ready, Start
/// and chat for as long as the lobby is unreachable.
fn queue_arrivals(
    input: &Receiver<Request>,
    current: Option<&Request>,
    queued: &mut VecDeque<Request>,
) {
    while let Ok(request) = input.try_recv() {
        if !current
            .into_iter()
            .chain(queued.iter())
            .any(|waiting| waiting.repeats(&request))
        {
            queued.push_back(request);
        }
    }
}

pub(super) enum Response {
    MapUploadPrepared {
        transfer: u64,
        bytes: Vec<u8>,
    },
    MapChunk {
        transfer: u64,
        digest: String,
        total: usize,
        offset: usize,
        bytes: Vec<u8>,
        complete: bool,
    },
    MapUploaded {
        transfer: u64,
        completed: usize,
    },
    CachedMap {
        transfer: u64,
        loaded: Option<crate::map::source::LoadedTransferAsset>,
    },
    MapInstalled {
        transfer: u64,
        loaded: crate::map::source::LoadedTransferAsset,
    },
    MapFailure {
        transfer: u64,
        retryable: bool,
    },
    MapPending {
        transfer: u64,
    },
    Authenticated {
        attempt: u64,
        player_id: String,
        nickname: String,
        motd: Option<String>,
    },
    LoginFailed {
        attempt: u64,
        code: u32,
    },
    /// HTTP 426 to a sign-in: the lobby no longer admits this version.
    /// `message` is the lobby's own reason when it gave a plain one.
    UpgradeRequired {
        attempt: u64,
        message: Option<String>,
    },
    /// HTTP 426 to a signed-in request: the lobby was upgraded during this
    /// session. Only the refused request failed.
    Outdated {
        message: Option<String>,
    },
    Channels(ChannelList),
    Event(Envelope),
    CommandCompleted {
        id: u64,
        operation: &'static str,
    },
    CommandFailed {
        id: u64,
        operation: &'static str,
        failure: CommandFailure,
    },
    Failure(&'static str),
    Offline,
    /// The first event poll of an outage failed; polling keeps retrying.
    LobbyUnreachable,
    /// An event poll succeeded again after `LobbyUnreachable`.
    LobbyReachable,
    /// About a minute of failed polls outside a match: polling stopped.
    LobbyLost,
    /// The lobby refused a sign-in for a reason of its own (maintenance,
    /// full or busy): the text to show.
    SignInRefused {
        attempt: u64,
        message: String,
    },
    StartRejected {
        id: u64,
        /// 429, 5xx or no answer: the lobby, not the setup, refused it.
        retryable: bool,
    },
    Translated {
        id: u64,
        outcome: Result<String, TranslateFailure>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranslateFailure {
    /// No translator, model failure, budget or limit: try again later.
    Unavailable,
    /// The lobby refused this line or language (400).
    Unsupported,
}

const MAX_BODY_BYTES: u64 = 1024 * 1024;
const USER_AGENT: &str = concat!("VERA20k/", env!("CARGO_PKG_VERSION"), " multiplayer-client");
// The server retains1024 events and returns at most256 at a time. Permit
// concurrent arrivals too, but never release cleanup on a partial history.
const MAX_CLEANUP_DRAIN_BATCHES: usize = 8;
// After `timing_confirmed` a spawned match's only event is `relay_renewed`, in
// the reply to its own `renew_relay`. This poll recovers a reply that was lost.
const SPAWNED_MATCH_POLL: Duration = Duration::from_secs(60);
// About three lines of the 0xD0 box body, which also holds the download link.
const MAX_UPGRADE_MESSAGE_CHARS: usize = 160;
// The failed polls, 2+4+8+16+30 s apart, after which a player outside a
// match is told the lobby connection was lost: about a minute.
const LOBBY_LOST_POLLS: u8 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RequestFailure {
    Status(u16),
    LostCursor,
    ChannelRejected {
        status: u16,
        code: u32,
    },
    AlreadyFinished,
    /// 426 `upgrade_required` (docs/lobby-protocol.md), with the lobby's
    /// message when it is one short line of plain text.
    UpgradeRequired(Option<String>),
    /// 503 `maintenance`: the operator closed sign-ins, with the same kind
    /// of message.
    Maintenance(Option<String>),
    /// 503 `service_full` or `network_full`: the lobby has no room for this
    /// sign-in, with the same kind of message.
    Full(Option<String>),
}

/// What a lobby refusal of a sign-in tells the player: its own plain
/// message when it gave one.
fn sign_in_refusal(failure: RequestFailure) -> String {
    match failure {
        RequestFailure::Maintenance(message) => message.unwrap_or_else(|| {
            "VERA20k Online is down for maintenance. Please try again later.".to_owned()
        }),
        RequestFailure::Full(message) => message.unwrap_or_else(|| {
            "VERA20k Online is full right now. Please try again later.".to_owned()
        }),
        _ => "VERA20k Online is busy right now. Please try again in a minute.".to_owned(),
    }
}

/// The lobby's `error.message` when it is one short line of plain text.
fn plain_message(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|value| value["error"]["message"].as_str())
        .map(str::trim)
        .filter(|message| {
            !message.is_empty()
                && message.chars().count() <= MAX_UPGRADE_MESSAGE_CHARS
                && !message.chars().any(char::is_control)
        })
        .map(str::to_owned)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandFailure {
    Retryable,
    Rejected(u32),
    AlreadyFinished,
}

impl CommandFailure {
    pub(super) fn wol_code(self) -> u32 {
        match self {
            Self::Rejected(code) => code,
            _ => 0x8000_4005,
        }
    }
}

impl RequestFailure {
    fn response(status: u16, bytes: &[u8]) -> Self {
        let value = serde_json::from_slice::<Value>(bytes).ok();
        let code = value
            .as_ref()
            .and_then(|value| value["error"]["code"].as_str());
        match code {
            Some("invalid_cursor" | "history_expired") if status == 409 => Self::LostCursor,
            Some("room_password") if status == 403 => Self::ChannelRejected {
                status,
                code: 0x8004_0071,
            },
            // The host kicked this player from the room within five minutes.
            Some("room_kept_out") if status == 403 => Self::ChannelRejected {
                status,
                code: crate::app::shell_wol::lobby::KEPT_OUT_JOIN_ERROR,
            },
            Some("room_full") if status == 409 => Self::ChannelRejected {
                status,
                code: 0x8004_006E,
            },
            Some("room_unavailable") if status == 409 => Self::ChannelRejected {
                status,
                code: 0x8004_0070,
            },
            Some("unknown_match") if status == 409 => Self::AlreadyFinished,
            Some("maintenance") if status == 503 => {
                Self::Maintenance(plain_message(value.as_ref()))
            }
            Some("service_full" | "network_full") if status == 503 => {
                Self::Full(plain_message(value.as_ref()))
            }
            _ if status == 426 => Self::UpgradeRequired(plain_message(value.as_ref())),
            _ => Self::Status(status),
        }
    }

    fn command_failure(&self, operation: &str) -> CommandFailure {
        match self {
            Self::AlreadyFinished if operation == "finish" => CommandFailure::AlreadyFinished,
            Self::ChannelRejected { code, .. } => CommandFailure::Rejected(*code),
            _ if matches!(self.status(), 0 | 429 | 500..=599) => CommandFailure::Retryable,
            _ => CommandFailure::Rejected(0x8000_4005),
        }
    }

    fn status(&self) -> u16 {
        match self {
            Self::Status(status) | Self::ChannelRejected { status, .. } => *status,
            Self::LostCursor | Self::AlreadyFinished => 409,
            Self::UpgradeRequired(_) => 426,
            Self::Maintenance(_) | Self::Full(_) => 503,
        }
    }
}

struct Client {
    stopping: Arc<AtomicBool>,
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    agent: ureq::Agent,
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    base: String,
    #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
    browser: u32,
    token: Option<String>,
    cursor: u64,
    active_match: Option<String>,
    /// The bearer came from an external launcher, not `/v1/sessions`.
    spawned: bool,
    /// The lobby stopped answering and the player was told the connection
    /// was lost: no more polls; the next sign-in logs this bearer out first.
    abandoned: bool,
}

fn decode_session(value: Value, requested_nickname: &str) -> Result<Session, RequestFailure> {
    let session: Session = serde_json::from_value(value).map_err(|_| RequestFailure::Status(0))?;
    super::validate_online_identity(&session.player_id, &session.nickname)
        .map_err(|_| RequestFailure::Status(0))?;
    // A successful response may preserve the requested name's casing, but
    // cannot replace the name the player chose. Session credentials
    // remain private; validation errors never contain response field values.
    let now = crate::util::time::SystemTime::now()
        .duration_since(crate::util::time::UNIX_EPOCH)
        .map_err(|_| RequestFailure::Status(0))?
        .as_secs();
    if !session.nickname.eq_ignore_ascii_case(requested_nickname)
        || session.expires_at <= now
        || session.token.len() != 43
        || !base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&session.token)
            .is_ok_and(|token| token.len() == 32)
    {
        return Err(RequestFailure::Status(0));
    }
    Ok(session)
}

impl Client {
    fn adopt_session(&mut self, token: String, resume: Option<(String, u64)>) {
        self.token = Some(token);
        let (active_match, cursor) = resume.map_or((None, 0), |(id, cursor)| (Some(id), cursor));
        self.cursor = cursor;
        self.active_match = active_match;
        self.spawned = true;
    }

    fn accept_match_resume(
        &mut self,
        value: Value,
        output: &Sender<Response>,
    ) -> Result<(), RequestFailure> {
        let match_id = value["match_id"]
            .as_str()
            .ok_or(RequestFailure::Status(0))?;
        let cursor = value["cursor"].as_u64().ok_or(RequestFailure::Status(0))?;
        if self.active_match.as_deref() != Some(match_id) || cursor < self.cursor {
            return Err(RequestFailure::Status(0));
        }
        // The app validates the same grant's exact match/player/crypto binding
        // before it renews its UDP lease; never replace its gameplay session.
        serde_json::from_value::<super::wire::RelayGrant>(value["relay"].clone())
            .map_err(|_| RequestFailure::Status(0))?;
        output
            .send(Response::Event(Envelope {
                sequence: cursor,
                kind: "relay_renewed".to_owned(),
                payload: json!({"match_id":match_id,"relay":value["relay"]}),
            }))
            .map_err(|_| RequestFailure::Status(0))?;
        self.cursor = cursor;
        Ok(())
    }

    fn batch(&mut self, value: Value, output: &Sender<Response>) -> Result<(), RequestFailure> {
        let mut batch: EventBatch =
            serde_json::from_value(value).map_err(|_| RequestFailure::Status(0))?;
        batch.events.retain(|event| event.sequence > self.cursor);
        if batch.cursor < self.cursor
            || batch
                .events
                .iter()
                .any(|event| event.sequence > batch.cursor)
        {
            return Err(RequestFailure::Status(0));
        }
        self.cursor = batch.cursor;
        // A callback can begin a shell slide halfway through a server batch.
        // Each event stays on the same FIFO until the UI may pump messages;
        // emitting one batch would force the consumer to duplicate its tail.
        for event in batch.events {
            if event.kind == "timing_confirmed"
                && let Some(id) = event.payload["match_id"].as_str()
                && super::relay_uuid(id).is_ok()
            {
                self.active_match = Some(id.to_owned());
            }
            output
                .send(Response::Event(event))
                .map_err(|_| RequestFailure::Status(0))?;
        }
        Ok(())
    }
}

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
impl Client {
    /// The bearer a new stream opens with. A spawned match only needs events
    /// during its start barrier, which polls cover, so it opens none.
    fn stream_bearer(&self) -> Option<&str> {
        self.token
            .as_deref()
            .filter(|_| !self.spawned && !self.abandoned)
    }

    /// A stream frame starts at the stream's own cursor, which command
    /// replies and polls may have passed: a stale frame is dropped, and
    /// `batch` drops the events this worker already delivered.
    fn streamed(&mut self, frame: Value, output: &Sender<Response>) -> Result<(), RequestFailure> {
        if frame["cursor"]
            .as_u64()
            .is_some_and(|cursor| cursor < self.cursor)
        {
            return Ok(());
        }
        self.batch(frame, output)
    }

    fn request(&self, path: &str, body: Option<&Value>) -> Result<Value, RequestFailure> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(RequestFailure::Status(0));
        }
        let url = format!("{}{path}", self.base);
        let response = if let Some(body) = body {
            let mut request = self.agent.post(&url).header("User-Agent", USER_AGENT);
            if path == "/v1/sessions" {
                // Its relay CONFIRM carries a federation roster; see docs/lobby-protocol.md.
                request = request.header("Vera-Relay-Federation", "1");
            }
            if let Some(token) = &self.token {
                request = request.header("Authorization", format!("Bearer {token}"));
            }
            request.send_json(body)
        } else {
            let mut request = self.agent.get(&url).header("User-Agent", USER_AGENT);
            if let Some(token) = &self.token {
                request = request.header("Authorization", format!("Bearer {token}"));
            }
            request.call()
        };
        let mut response = response.map_err(|_| RequestFailure::Status(0))?;
        let status = response.status().as_u16();
        if status == 401 {
            return Err(RequestFailure::Status(status));
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_BODY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| RequestFailure::Status(0))?;
        if bytes.len() as u64 > MAX_BODY_BYTES {
            return Err(RequestFailure::Status(0));
        }
        if !(200..300).contains(&status) {
            return Err(RequestFailure::response(status, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(|_| RequestFailure::Status(0))
    }
}

fn translation_result(result: Result<Value, RequestFailure>) -> Result<String, TranslateFailure> {
    match result {
        Ok(value) => value["text"]
            .as_str()
            .map(str::to_owned)
            .ok_or(TranslateFailure::Unavailable),
        Err(RequestFailure::Status(400)) => Err(TranslateFailure::Unsupported),
        Err(_) => Err(TranslateFailure::Unavailable),
    }
}

/// The model takes about a second, so translation has its own thread and
/// polling and commands never wait for it. A job carries the current bearer.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn spawn_translator(
    client: &Client,
    output: &Sender<Response>,
) -> Option<Sender<(u64, String, String, &'static str)>> {
    let (jobs, input) = std::sync::mpsc::channel::<(u64, String, String, &'static str)>();
    let mut translator = Client {
        stopping: Arc::clone(&client.stopping),
        agent: client.agent.clone(),
        base: client.base.clone(),
        token: None,
        cursor: 0,
        active_match: None,
        spawned: false,
        abandoned: false,
    };
    let output = output.clone();
    std::thread::Builder::new()
        .name("vera-translate".into())
        .spawn(move || {
            for (id, token, text, target) in input {
                translator.token = Some(token);
                let body = json!({"text":text,"target":target});
                let outcome = translation_result(translator.request("/v1/translate", Some(&body)));
                if output.send(Response::Translated { id, outcome }).is_err() {
                    break;
                }
            }
        })
        .ok()
        .map(|_| jobs)
}

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(super) fn run(
    base: String,
    input: Receiver<Request>,
    output: Sender<Response>,
    stopping: Arc<AtomicBool>,
) {
    // Only an external launch's validated loopback origin is plain HTTP.
    let config = ureq::Agent::config_builder()
        .https_only(base.starts_with("https://"))
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(12)))
        .build();
    let client = Client {
        stopping: Arc::clone(&stopping),
        agent: config.into(),
        base,
        token: None,
        cursor: 0,
        active_match: None,
        spawned: false,
        abandoned: false,
    };
    let translator = spawn_translator(&client, &output);
    run_loop(client, input, output, translator);
}

// One source for the serial HTTP decisions. Native expands to ordinary blocking
// functions; only the explicitly enabled browser target emits async functions.
// No native executor or second copy of the protocol state is introduced.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
macro_rules! platform_await {
    ($operation:expr) => {
        $operation
    };
}
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
macro_rules! platform_await {
    ($operation:expr) => {
        $operation.await
    };
}

macro_rules! serial_driver {
    ($($asynchronous:tt)?) => {
        impl Client {
    /// Log out a bearer this worker still holds (given up in an outage, or
    /// whose logout the shell stopped retrying), which still holds its name.
    /// The lobby refuses to log out a session in a match, so a match the
    /// shell already left is finished first. False while the lobby is away.
    $($asynchronous)? fn release_held_session(&mut self) -> bool {
        if self.token.is_none() {
            return true;
        }
        if let Some(match_id) = self.active_match.clone() {
            let body = json!({"request_id":"held-finish","after":self.cursor,"operation":"finish","payload":{"match_id":match_id}});
            let _ = platform_await!(self.request("/v1/commands", Some(&body)));
            self.active_match = None;
        }
        let body = json!({"request_id":"held-logout","after":self.cursor,"operation":"logout","payload":{}});
        match platform_await!(self.request("/v1/commands", Some(&body))) {
            Ok(_) | Err(RequestFailure::Status(401)) => {
                self.token = None;
                self.abandoned = false;
                true
            }
            Err(_) => false,
        }
    }

    $($asynchronous)? fn session_failed(&mut self, error: RequestFailure, output: &Sender<Response>) -> bool {
        if let RequestFailure::UpgradeRequired(message) = error {
            // The lobby keeps serving an outdated session's events, finish,
            // leave and logout (docs/lobby-protocol.md), so it stays.
            let _ = output.send(Response::Outdated { message });
            return false;
        }
        if error == RequestFailure::LostCursor
            && let Some(match_id) = self.active_match.clone()
        {
            let recovered = platform_await!(self
                .request(&format!("/v1/match/resume?match_id={match_id}"), None))
                .and_then(|value| self.accept_match_resume(value, output));
            // Shell event history is separate from the committed gameplay
            // journal. Network failures keep the same bearer/identity/cursor
            // until a matching active-match recovery succeeds.
            if !matches!(
                recovered,
                Err(RequestFailure::Status(401) | RequestFailure::AlreadyFinished)
            ) {
                return true;
            }
            self.active_match = None;
        }
        if matches!(
            error,
            RequestFailure::Status(401) | RequestFailure::LostCursor
        ) {
            if error == RequestFailure::LostCursor && self.token.is_some() {
                // The bearer still owns this guest name, even though its
                // event history cannot be replayed. Ordinary logout accepts
                // an expired cursor and retires only this session. If the
                // request fails, normal stale-session reap remains the fallback.
                let _ = platform_await!(self.request(
                    "/v1/commands",
                    Some(&json!({
                        "request_id":"lost-cursor-logout","operation":"logout","payload":{}
                    })),
                ));
            }
            // A missing history cannot be replayed safely. Retire this login so
            // it stops refreshing the broken server session and use the WOL
            // disconnect/re-login path. Repeated queued failures emit once.
            if self.token.take().is_some() {
                let _ = output.send(Response::Offline);
            }
            true
        } else {
            false
        }
    }

    $($asynchronous)? fn poll_finished(
        &mut self,
        result: Result<(), RequestFailure>,
        failed_polls: &mut u8,
        output: &Sender<Response>,
    ) -> Duration {
        match result {
            Ok(()) => {
                if *failed_polls > 0 {
                    let _ = output.send(Response::LobbyReachable);
                }
                *failed_polls = 0;
                if self.spawned && self.active_match.is_some() {
                    SPAWNED_MATCH_POLL
                } else {
                    Duration::from_secs(1)
                }
            }
            Err(error) if platform_await!(self.session_failed(error.clone(), output)) => Duration::from_secs(30),
            Err(_) => {
                if *failed_polls == 0 {
                    let _ = output.send(Response::LobbyUnreachable);
                }
                *failed_polls = failed_polls.saturating_add(1);
                // From about a minute on, each failed poll asks the shell,
                // which gives up (`Request::Abandon`) unless a match is on.
                if *failed_polls >= LOBBY_LOST_POLLS && !self.spawned && !self.abandoned {
                    let _ = output.send(Response::LobbyLost);
                }
                Duration::from_secs((1u64 << (*failed_polls).min(5)).min(30))
            }
        }
    }

    $($asynchronous)? fn drain_events(&mut self, output: &Sender<Response>) -> Result<(), RequestFailure> {
        for _ in 0..MAX_CLEANUP_DRAIN_BATCHES {
            let value = platform_await!(self.request(&format!("/v1/events?after={}", self.cursor), None))?;
            let empty = value
                .get("events")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty);
            self.batch(value, output)?;
            if empty {
                return Ok(());
            }
        }
        // Keep the same cleanup request ID and retry; a busy event stream is
        // not proof that all old room callbacks have reached the UI owner.
        Err(RequestFailure::Status(0))
    }
        }
        $($asynchronous)? fn run_loop(
            mut client: Client,
            input: Receiver<Request>,
            output: Sender<Response>,
            translator: Option<Sender<(u64, String, String, &'static str)>>,
        ) {
    let stopping = Arc::clone(&client.stopping);
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    let mut streams = stream::Streams::new(&client.base);
    let mut failed_polls = 0_u8;
    let mut next_poll = Instant::now();
    let mut queued = VecDeque::new();
    loop {
        if stopping.load(Ordering::Acquire) {
            break;
        }
        let request = match queued.pop_front() {
            Some(request) => Some(request),
            None => match platform_await!(next_request(&input)) {
                Ok(request) => request,
                Err(()) => break,
            },
        };
        queue_arrivals(&input, request.as_ref(), &mut queued);
        if stopping.load(Ordering::Acquire) {
            break;
        }
        match request {
            Some(Request::ReadMapUpload {
                transfer,
                source,
                digest,
                total,
            }) => {
                let response = match source.read(&digest, total) {
                    Ok(bytes) => Response::MapUploadPrepared { transfer, bytes },
                    Err(_) => Response::MapFailure {
                        transfer,
                        retryable: false,
                    },
                };
                let _ = output.send(response);
            }
            Some(Request::CachedMap {
                transfer,
                resource,
                directory,
                digest,
                total,
            }) => {
                let loaded =
                    crate::map::source::load_received_asset(&directory, &digest, total, resource)
                        .ok();
                let _ = output.send(Response::CachedMap { transfer, loaded });
            }
            Some(Request::InstallMap {
                transfer,
                resource,
                directory,
                digest,
                total,
                bytes,
            }) => {
                let result = crate::map::source::install_received_asset(
                    &directory, bytes, &digest, total, resource,
                );
                let response = match result {
                    Ok(loaded) => Response::MapInstalled { transfer, loaded },
                    Err(_) => Response::MapFailure {
                        transfer,
                        retryable: false,
                    },
                };
                let _ = output.send(response);
            }
            Some(Request::MapChunk {
                transfer,
                resource,
                seed_digest,
                digest,
                offset,
            }) => {
                let result = if crate::map::source::valid_source_sha256(&digest) {
                    platform_await!(client.request(
                        &format!(
                            "/v1/maps/{seed_digest}?offset={offset}&resource={}",
                            if resource == crate::map::source::TransferResource::Preview {
                                "preview"
                            } else {
                                "map"
                            }
                        ),
                        None,
                    ))
                } else {
                    Err(RequestFailure::Status(400))
                };
                if let Err(error) = result.as_ref() {
                    platform_await!(client.session_failed(error.clone(), &output));
                }
                let response = match result {
                    Ok(value) => {
                        #[derive(serde::Deserialize)]
                        struct Chunk {
                            digest: String,
                            byte_length: usize,
                            offset: usize,
                            data: String,
                            complete: bool,
                        }
                        serde_json::from_value::<Chunk>(value)
                            .ok()
                            .and_then(|chunk| {
                                if chunk.data.len() > 43700 {
                                    return None;
                                }
                                let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                                    .decode(&chunk.data)
                                    .ok()?;
                                (bytes.len() <= 32768).then_some(Response::MapChunk {
                                    transfer,
                                    digest: chunk.digest,
                                    total: chunk.byte_length,
                                    offset: chunk.offset,
                                    bytes,
                                    complete: chunk.complete,
                                })
                            })
                            .unwrap_or(Response::MapFailure {
                                transfer,
                                retryable: false,
                            })
                    }
                    Err(RequestFailure::Status(409 | 429)) => Response::MapPending { transfer },
                    Err(error) => Response::MapFailure {
                        transfer,
                        retryable: matches!(error.status(), 0 | 500..=599),
                    },
                };
                let _ = output.send(response);
            }
            Some(Request::UploadMapChunk {
                transfer,
                resource,
                id,
                digest,
                total,
                offset,
                bytes,
            }) => {
                let completed = offset + bytes.len();
                let body = json!({"request_id":id.to_string(),"after":client.cursor,"operation":"map_upload",
                    "payload":{"digest":digest,"byte_length":total,"offset":offset,"resource":if resource == crate::map::source::TransferResource::Preview { "preview" } else { "map" },
                    "data":base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)}});
                let result = platform_await!(client
                    .request("/v1/commands", Some(&body)))
                    .and_then(|value| client.batch(value, &output));
                if let Err(error) = result.as_ref() {
                    platform_await!(client.session_failed(error.clone(), &output));
                }
                let response = match result {
                    Ok(()) => Response::MapUploaded {
                        transfer,
                        completed,
                    },
                    Err(error) => Response::MapFailure {
                        transfer,
                        retryable: matches!(
                            error,
                            RequestFailure::Status(0 | 409 | 429 | 500..=599)
                        ),
                    },
                };
                let _ = output.send(response);
            }
            Some(Request::OpenSession { attempt, nickname }) => {
                // A held bearer still holds its name: log it out first, and
                // while the lobby is away, report that instead of the name.
                if !platform_await!(client.release_held_session()) {
                    let _ = output.send(Response::LoginFailed { attempt, code: 0 });
                    continue;
                }
                client.cursor = 0;
                client.active_match = None;
                let result = platform_await!(client
                    .request(
                        "/v1/sessions",
                        Some(&json!({"nickname": nickname, "protocol": 4})),
                    ))
                    .and_then(|value| decode_session(value, &nickname));
                match result {
                    Ok(session) => {
                        client.token = Some(session.token);
                        failed_polls = 0;
                        if output
                            .send(Response::Authenticated {
                                attempt,
                                player_id: session.player_id,
                                nickname: session.nickname,
                                motd: session.motd,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(RequestFailure::Status(0)) => {
                        let _ = output.send(Response::LoginFailed { attempt, code: 0 });
                    }
                    Err(RequestFailure::Status(401 | 403)) => {
                        let _ = output.send(Response::LoginFailed {
                            attempt,
                            code: 0x8004_0073,
                        });
                    }
                    Err(RequestFailure::Status(409)) => {
                        let _ = output.send(Response::LoginFailed {
                            attempt,
                            code: 0x8004_0064,
                        });
                    }
                    Err(RequestFailure::UpgradeRequired(message)) => {
                        let _ = output.send(Response::UpgradeRequired { attempt, message });
                    }
                    Err(error @ (RequestFailure::Maintenance(_)
                    | RequestFailure::Full(_)
                    | RequestFailure::Status(429))) => {
                        let message = sign_in_refusal(error);
                        let _ = output.send(Response::SignInRefused { attempt, message });
                    }
                    Err(_) => {
                        let _ = output.send(Response::LoginFailed { attempt, code: 0 });
                    }
                }
            }
            Some(Request::Abandon) => client.abandoned = client.token.is_some(),
            Some(Request::AdoptSession { token, resume }) => {
                if client.token.is_none() {
                    client.adopt_session(token, resume);
                    failed_polls = 0;
                }
            }
            Some(Request::Channels(kind)) => {
                match platform_await!(client
                    .request(&format!("/v1/channels?kind={kind}"), None))
                    .and_then(|value| {
                        serde_json::from_value(value).map_err(|_| RequestFailure::Status(0))
                    }) {
                    Ok(channels) => {
                        let _ = output.send(Response::Channels(channels));
                    }
                    Err(error) => {
                        if !platform_await!(client.session_failed(error, &output)) {
                            let _ = output
                                .send(Response::Failure("Could not refresh the online lobby."));
                        }
                    }
                }
            }
            Some(Request::Command {
                id,
                operation,
                payload,
            }) => {
                if operation == "logout" && client.token.is_none() {
                    // This serial worker has either already acknowledged
                    // logout or never obtained a bearer. No old authority is
                    // left to clean up, including Back from the sign-in page.
                    let _ = output.send(Response::CommandCompleted { id, operation });
                    continue;
                }
                let body = json!({"request_id":id.to_string(),"after":client.cursor,"operation":operation,"payload":payload});
                let mut response = platform_await!(client.request("/v1/commands", Some(&body)));
                if response == Err(RequestFailure::Status(0)) {
                    response = platform_await!(client.request("/v1/commands", Some(&body)));
                }
                let result = response.and_then(|value| {
                        // Logout removes its event stream and returns cursor0.
                        // Its successful HTTP response is the completion receipt.
                        if operation == "logout" {
                            Ok(())
                        } else {
                            client.batch(value, &output)
                        }
                    });
                let result = if result.is_ok() && matches!(operation, "finish" | "leave") {
                    // The server may return events pending before the mutation.
                    // Drain its committed tail before releasing the next room.
                    platform_await!(client.drain_events(&output))
                } else {
                    result
                };
                let completed = result.is_ok();
                if let Err(error) = &result {
                    // Static operation names and numeric status only: request
                    // bodies, headers and transport errors can carry secrets.
                    log::warn!(
                        "Online command failed: operation={operation}, status={}",
                        error.status()
                    );
                }
                match result {
                    Ok(()) => {
                        if matches!(operation, "finish" | "leave" | "logout") {
                            client.active_match = None;
                        }
                        let _ = output.send(Response::CommandCompleted { id, operation });
                    }
                    Err(error)
                        if !platform_await!(client.session_failed(error.clone(), &output)) && operation != "renew_relay" =>
                    {
                        let _ = output.send(if operation == "start" {
                            Response::StartRejected {
                                id,
                                retryable: error.command_failure(operation)
                                    == CommandFailure::Retryable,
                            }
                        } else {
                            Response::CommandFailed {
                                id,
                                operation,
                                failure: error.command_failure(operation),
                            }
                        });
                    }
                    Err(_) => {}
                }
                if operation == "logout" && completed {
                    client.token = None;
                    client.abandoned = false;
                }
            }
            Some(Request::Translate { id, text, target }) => {
                let job = client.token.clone().map(|token| (id, token, text, target));
                if job
                    .and_then(|job| translator.as_ref()?.send(job).ok())
                    .is_none()
                {
                    let _ = output.send(Response::Translated {
                        id,
                        outcome: Err(TranslateFailure::Unavailable),
                    });
                }
            }
            None => {}
        }
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let streaming = {
            while let Some(frame) = streams.next(client.stream_bearer()) {
                // A bad frame ends the stream; the next poll asks again.
                if client.streamed(frame, &output).is_err() {
                    streams.close();
                }
            }
            streams.open(client.stream_bearer(), client.cursor, &stopping);
            // A stream that is up is a lobby that answers.
            if streams.live() && failed_polls > 0 {
                client.poll_finished(Ok(()), &mut failed_polls, &output);
            }
            streams.live()
        };
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let streaming = false;
        if client.token.is_some() && !client.abandoned && !streaming && Instant::now() >= next_poll {
            let result = platform_await!(client
                .request(&format!("/v1/events?after={}", client.cursor), None))
                .and_then(|value| client.batch(value, &output));
            next_poll = Instant::now() + platform_await!(client.poll_finished(result, &mut failed_polls, &output));
        }
    }
        }
    };
}
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
serial_driver!();
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
serial_driver!(async);

// Stream frames arrive on their own channel, so the worker looks for them
// every 50 ms, as the browser worker does for requests.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn next_request(input: &Receiver<Request>) -> Result<Option<Request>, ()> {
    match input.recv_timeout(Duration::from_millis(50)) {
        Ok(request) => Ok(Some(request)),
        Err(RecvTimeoutError::Timeout) => Ok(None),
        Err(RecvTimeoutError::Disconnected) => Err(()),
    }
}
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
async fn next_request(input: &Receiver<Request>) -> Result<Option<Request>, ()> {
    match input.try_recv() {
        Ok(request) => Ok(Some(request)),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(()),
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            browser::delay(50).await;
            Ok(None)
        }
    }
}

#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
mod browser;
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
mod stream;
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
pub(super) use browser::{RequestOwner, start as start_browser};

#[cfg(test)]
mod tests {
    use super::*;

    fn session_response() -> Value {
        json!({
            "player_id": "11111111-1111-1111-1111-111111111111",
            "nickname": "Player_01234567890123456",
            "token": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0; 32]),
            "expires_at": u64::MAX,
        })
    }

    #[test]
    fn authentication_response_admits_only_the_requested_safe_nickname() {
        let response = session_response();
        let nickname = response["nickname"].as_str().unwrap().to_owned();
        let accepted = decode_session(response.clone(), &nickname.to_ascii_lowercase()).unwrap();
        assert_eq!(accepted.nickname, nickname);
        assert_eq!(accepted.player_id, response["player_id"].as_str().unwrap());

        let mut injected = response.clone();
        injected["nickname"] = json!("a]\r\n[Ask]\r\nAutoLogin=no\r\n[b");
        assert!(decode_session(injected.clone(), injected["nickname"].as_str().unwrap()).is_err());
        let mut substituted = response;
        substituted["nickname"] = json!("AnotherAccount");
        assert!(decode_session(substituted, &nickname).is_err());
    }

    #[test]
    fn authentication_response_carries_the_operators_welcome() {
        let response = session_response();
        let nickname = response["nickname"].as_str().unwrap().to_owned();
        assert_eq!(
            decode_session(response.clone(), &nickname).unwrap().motd,
            None
        );
        let mut welcomed = response;
        welcomed["motd"] = json!("Welcome to VERA20K Online!\nHave fun.");
        let accepted = decode_session(welcomed, &nickname).unwrap();
        assert_eq!(
            accepted.motd.as_deref(),
            Some("Welcome to VERA20K Online!\nHave fun.")
        );
    }

    #[test]
    fn authentication_response_rejects_malformed_player_bearer_and_expiry() {
        let response = session_response();
        let nickname = response["nickname"].as_str().unwrap();
        let mut malformed_player = response.clone();
        malformed_player["player_id"] = json!("not-a-player-uuid");
        assert!(decode_session(malformed_player, nickname).is_err());
        let mut malformed_bearer = response.clone();
        malformed_bearer["token"] = json!("untrusted\r\nHeader: value");
        assert!(decode_session(malformed_bearer, nickname).is_err());
        let mut expired = response.clone();
        expired["expires_at"] = json!(1);
        assert!(decode_session(expired, nickname).is_err());
    }

    #[test]
    fn command_failures_keep_safe_room_codes_and_retryable_finish_semantics() {
        let wrong_key = RequestFailure::response(
            403,
            br#"{"error":{"code":"room_password","message":"untrusted text"}}"#,
        );
        assert_eq!(
            wrong_key.command_failure("join"),
            CommandFailure::Rejected(0x8004_0071)
        );
        let kept_out = RequestFailure::response(403, br#"{"error":{"code":"room_kept_out"}}"#);
        assert_eq!(
            kept_out.command_failure("join"),
            CommandFailure::Rejected(crate::app::shell_wol::lobby::KEPT_OUT_JOIN_ERROR)
        );
        let full = RequestFailure::response(409, br#"{"error":{"code":"room_full"}}"#);
        assert_eq!(
            full.command_failure("join"),
            CommandFailure::Rejected(0x8004_006E)
        );
        let missing = RequestFailure::response(409, br#"{"error":{"code":"room_unavailable"}}"#);
        assert_eq!(
            missing.command_failure("join"),
            CommandFailure::Rejected(0x8004_0070)
        );
        assert_eq!(
            RequestFailure::Status(503).command_failure("finish"),
            CommandFailure::Retryable
        );
        let finished = RequestFailure::response(409, br#"{"error":{"code":"unknown_match"}}"#);
        assert_eq!(
            finished.command_failure("finish"),
            CommandFailure::AlreadyFinished
        );
        assert_eq!(
            finished.command_failure("create"),
            CommandFailure::Rejected(0x8000_4005)
        );
    }

    /// A bearer the shell stopped logging out (Back in an outage, or a
    /// sign-in that answered after its wait) is logged out before the next
    /// sign-in instead of reporting the name in use; while the lobby is
    /// away it stays held.
    #[test]
    fn a_held_bearer_is_logged_out_before_signing_in_again() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let lobby = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let (mut request, mut chunk) = (Vec::new(), [0; 1024]);
            while !String::from_utf8_lossy(&request).contains("logout") {
                let read = stream.read(&mut chunk).unwrap();
                assert_ne!(read, 0, "the request ended without a logout");
                request.extend_from_slice(&chunk[..read]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .unwrap();
        });
        let mut client = Client {
            stopping: Arc::new(AtomicBool::new(false)),
            agent: ureq::Agent::new_with_defaults(),
            base: "http://127.0.0.1:1".to_owned(),
            token: Some("synthetic-test-session".to_owned()),
            cursor: 4,
            active_match: None,
            spawned: false,
            abandoned: false,
        };
        assert!(!client.release_held_session());
        assert!(client.token.is_some());
        client.base = format!("http://{address}");
        assert!(client.release_held_session());
        assert!(client.token.is_none());
        lobby.join().unwrap();
    }

    /// From about a minute of failed polls on, each failed poll asks the
    /// shell to give up, until it does; then the bearer stays for the next
    /// sign-in and no stream reopens.
    #[test]
    fn a_lobby_lost_for_a_minute_asks_the_shell_until_it_gives_up() {
        let (output, input) = std::sync::mpsc::channel();
        let mut client = Client {
            stopping: Arc::new(AtomicBool::new(false)),
            agent: ureq::Agent::new_with_defaults(),
            base: String::new(),
            token: Some("synthetic-test-session".to_owned()),
            cursor: 19,
            active_match: None,
            spawned: false,
            abandoned: false,
        };
        let outage = |client: &mut Client, failures: &mut u8, polls| {
            for _ in 0..polls {
                client.poll_finished(Err(RequestFailure::Status(0)), failures, &output);
            }
            input
                .try_iter()
                .filter(|response| matches!(response, Response::LobbyLost))
                .count()
        };
        let mut failures = 0;
        assert_eq!(outage(&mut client, &mut failures, LOBBY_LOST_POLLS - 1), 0);
        // A shell in a match ignores these, so they keep coming after it.
        assert_eq!(outage(&mut client, &mut failures, 2), 2);
        client.abandoned = true;
        assert_eq!(outage(&mut client, &mut failures, 1), 0);
        assert!(client.token.is_some() && client.stream_bearer().is_none());
    }

    #[test]
    fn transient_poll_failure_preserves_session_and_cursor_until_definitive_unauthorized() {
        let (output, input) = std::sync::mpsc::channel();
        let mut client = Client {
            stopping: Arc::new(AtomicBool::new(false)),
            agent: ureq::Agent::new_with_defaults(),
            base: String::new(),
            token: Some("synthetic-test-session".to_owned()),
            cursor: 19,
            active_match: None,
            spawned: false,
            abandoned: false,
        };
        let mut failures = 0;
        assert_eq!(
            client.poll_finished(Err(RequestFailure::Status(0)), &mut failures, &output),
            Duration::from_secs(2)
        );
        // One status line when the outage begins and one when it ends.
        assert!(matches!(input.try_recv(), Ok(Response::LobbyUnreachable)));
        assert_eq!(
            client.poll_finished(Err(RequestFailure::Status(503)), &mut failures, &output),
            Duration::from_secs(4)
        );
        failures = 20;
        assert_eq!(
            client.poll_finished(Err(RequestFailure::Status(429)), &mut failures, &output),
            Duration::from_secs(30)
        );
        assert!(client.token.is_some());
        assert_eq!(client.cursor, 19);
        assert!(matches!(input.try_recv(), Ok(Response::LobbyLost)));
        assert!(input.try_recv().is_err());
        assert_eq!(
            client.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(1)
        );
        assert_eq!(failures, 0);
        assert!(matches!(input.try_recv(), Ok(Response::LobbyReachable)));
        client.poll_finished(Ok(()), &mut failures, &output);
        assert!(input.try_recv().is_err());
        client.poll_finished(Err(RequestFailure::Status(401)), &mut failures, &output);
        assert!(client.token.is_none());
        assert!(matches!(input.try_recv(), Ok(Response::Offline)));
        assert!(client.session_failed(RequestFailure::Status(401), &output));
        assert!(input.try_recv().is_err());
    }

    #[test]
    fn periodic_requests_do_not_pile_up_behind_a_timing_out_command() {
        let (requests, input) = std::sync::mpsc::channel();
        let command = |id, operation, payload| Request::Command {
            id,
            operation,
            payload,
        };
        let renew = |id, match_id| command(id, "renew_relay", json!({"match_id":match_id}));
        for request in [
            command(2, "ready", json!({"ready":true})),
            command(3, "ping", json!({})),
            renew(4, "a"),
            renew(5, "a"),
            renew(6, "b"),
            command(7, "chat", json!({"text":"gg"})),
            command(8, "chat", json!({"text":"gg"})),
        ] {
            requests.send(request).unwrap();
        }
        let mut queued = VecDeque::new();
        queue_arrivals(&input, Some(&command(1, "ping", json!({}))), &mut queued);
        let ids: Vec<_> = queued
            .iter()
            .map(|request| match request {
                Request::Command { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, [Some(2), Some(4), Some(6), Some(7), Some(8)]);
    }

    #[test]
    fn expired_cursor_ends_the_session_once_but_a_room_conflict_does_not() {
        let (output, input) = std::sync::mpsc::channel();
        let mut client = Client {
            stopping: Arc::new(AtomicBool::new(false)),
            agent: ureq::Agent::new_with_defaults(),
            base: String::new(),
            token: Some("synthetic-test-session".to_owned()),
            cursor: 19,
            active_match: None,
            spawned: false,
            abandoned: false,
        };
        let conflict = RequestFailure::response(
            409,
            br#"{"error":{"code":"map_pending","message":"Wait for the map."}}"#,
        );
        assert!(!client.session_failed(conflict, &output));
        assert!(client.token.is_some());
        let expired = RequestFailure::response(
            409,
            br#"{"error":{"code":"history_expired","message":"Sign in again."}}"#,
        );
        client.poll_finished(Err(expired.clone()), &mut 0, &output);
        assert!(client.token.is_none());
        assert_eq!(client.cursor, 19);
        assert!(matches!(input.try_recv(), Ok(Response::Offline)));
        assert!(client.session_failed(expired, &output));
        assert!(client.session_failed(RequestFailure::Status(401), &output));
        assert!(input.try_recv().is_err());
        assert_eq!(
            RequestFailure::response(409, br#"{"error":{"code":"invalid_cursor"}}"#),
            RequestFailure::LostCursor
        );
    }

    /// A full or busy lobby refuses a sign-in in its own words, not as a
    /// connection failure.
    #[test]
    fn a_full_or_busy_lobby_refuses_a_sign_in_in_its_own_words() {
        let reason = "Too many players from your network are online.";
        let full = RequestFailure::response(
            503,
            json!({"error":{"code":"network_full","message":reason}})
                .to_string()
                .as_bytes(),
        );
        assert_eq!(full, RequestFailure::Full(Some(reason.into())));
        assert_eq!(sign_in_refusal(full), reason);
        assert!(sign_in_refusal(RequestFailure::Full(None)).contains("full"));
        assert!(sign_in_refusal(RequestFailure::Status(429)).contains("busy"));
    }

    #[test]
    fn update_refusal_keeps_only_a_plain_lobby_reason_and_the_session() {
        let reason = "Online play needs VERA20k 0.3.0 or newer.";
        let refused = |message: &str| {
            let body = json!({"error":{"code":"upgrade_required","message":message}});
            RequestFailure::response(426, body.to_string().as_bytes())
        };
        let longest = "x".repeat(MAX_UPGRADE_MESSAGE_CHARS);
        assert_eq!(
            refused(&longest),
            RequestFailure::UpgradeRequired(Some(longest.clone()))
        );
        let too_long = format!("{longest}x");
        for unusable in [too_long.as_str(), "Update.\r\n[Ask]", "  "] {
            assert_eq!(refused(unusable), RequestFailure::UpgradeRequired(None));
        }
        assert_eq!(
            RequestFailure::response(426, b"not json"),
            RequestFailure::UpgradeRequired(None)
        );
        assert_eq!(
            RequestFailure::response(
                503,
                br#"{"error":{"code":"maintenance","message":"Back at 20:30 UTC."}}"#
            ),
            RequestFailure::Maintenance(Some("Back at 20:30 UTC.".into()))
        );
        assert_eq!(
            RequestFailure::response(503, br#"{"error":{"code":"service_unavailable"}}"#),
            RequestFailure::Status(503)
        );

        let (output, input) = std::sync::mpsc::channel();
        let mut client = Client {
            stopping: Arc::new(AtomicBool::new(false)),
            agent: ureq::Agent::new_with_defaults(),
            base: String::new(),
            token: Some("synthetic-test-session".to_owned()),
            cursor: 19,
            active_match: None,
            spawned: false,
            abandoned: false,
        };
        assert!(!client.session_failed(refused(reason), &output));
        assert!(client.token.is_some());
        assert!(matches!(
            input.try_recv(),
            Ok(Response::Outdated { message: Some(message) }) if message == reason
        ));
    }

    #[test]
    fn replayed_http_response_does_not_redeliver_lobby_events() {
        let (output, input) = std::sync::mpsc::channel();
        let mut client = Client {
            stopping: Arc::new(AtomicBool::new(false)),
            agent: ureq::Agent::new_with_defaults(),
            base: String::new(),
            token: None,
            cursor: 0,
            active_match: None,
            spawned: false,
            abandoned: false,
        };
        let response = json!({"cursor":2,"events":[
            {"sequence":1,"kind":"created","payload":{}},
            {"sequence":2,"kind":"member_joined","payload":{"name":"A"}}
        ]});
        client.batch(response.clone(), &output).unwrap();
        client.batch(response, &output).unwrap();
        let Response::Event(first) = input.recv().unwrap() else {
            panic!("missing first event")
        };
        assert_eq!((first.sequence, first.kind.as_str()), (1, "created"));
        // Created can start a slide. Its following member event remains queued
        // separately, with no replay required after setup opens.
        let Response::Event(second) = input.recv().unwrap() else {
            panic!("missing queued event tail")
        };
        assert_eq!(
            (second.sequence, second.kind.as_str()),
            (2, "member_joined")
        );
        assert!(
            input.try_recv().is_err(),
            "replayed batches must not enqueue duplicates"
        );
        assert_eq!(client.cursor, 2);
    }

    #[test]
    fn stream_frames_behind_a_command_reply_deliver_only_newer_events() {
        let (output, input) = std::sync::mpsc::channel();
        let mut client = Client {
            stopping: Arc::new(AtomicBool::new(false)),
            agent: ureq::Agent::new_with_defaults(),
            base: String::new(),
            token: None,
            cursor: 5,
            active_match: None,
            spawned: false,
            abandoned: false,
        };
        let event = |sequence| json!({"sequence":sequence,"kind":"chat","payload":{}});
        client
            .streamed(json!({"cursor":4,"events":[event(4)]}), &output)
            .unwrap();
        assert_eq!(client.cursor, 5);
        client
            .streamed(
                json!({"cursor":6,"events":[event(4), event(5), event(6)]}),
                &output,
            )
            .unwrap();
        let delivered: Vec<_> = input
            .try_iter()
            .map(|response| match response {
                Response::Event(event) => event.sequence,
                _ => panic!("only events"),
            })
            .collect();
        assert_eq!((delivered, client.cursor), (vec![6], 6));
    }

    #[test]
    fn a_spawned_match_polls_slowly_and_never_streams_once_timing_is_confirmed() {
        let (output, _input) = std::sync::mpsc::channel();
        let timing = || {
            json!({"cursor":1,"events":[{"sequence":1,"kind":"timing_confirmed",
                "payload":{"match_id":"11111111-1111-1111-1111-111111111111"}}]})
        };
        let client = || Client {
            stopping: Arc::new(AtomicBool::new(false)),
            agent: ureq::Agent::new_with_defaults(),
            base: String::new(),
            token: None,
            cursor: 0,
            active_match: None,
            spawned: false,
            abandoned: false,
        };
        let mut failures = 0;

        let mut spawned = client();
        // A returning engine's start is history: it polls at its match's pace at once.
        let mut returning = client();
        returning.adopt_session(
            "synthetic-test-session".to_owned(),
            Some(("11111111-1111-1111-1111-111111111111".to_owned(), 2)),
        );
        assert_eq!(returning.cursor, 2);
        assert_eq!(
            returning.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(60)
        );

        spawned.adopt_session("synthetic-test-session".to_owned(), None);
        assert_eq!(spawned.stream_bearer(), None);
        // The start barrier waits on the other players' events.
        assert_eq!(
            spawned.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(1)
        );
        spawned.batch(timing(), &output).unwrap();
        assert_eq!(
            spawned.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(60)
        );

        // A lobby session's match keeps its stream and its pace.
        let mut lobby = client();
        lobby.token = Some("synthetic-test-session".to_owned());
        assert_eq!(lobby.stream_bearer(), Some("synthetic-test-session"));
        lobby.batch(timing(), &output).unwrap();
        assert_eq!(
            lobby.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn active_match_resume_preserves_identity_and_rejects_another_match_or_old_cursor() {
        let (output, input) = std::sync::mpsc::channel();
        let id = "11111111-1111-1111-1111-111111111111";
        let mut client = Client {
            stopping: Arc::new(AtomicBool::new(false)),
            agent: ureq::Agent::new_with_defaults(),
            base: String::new(),
            token: Some("synthetic-test-session".to_owned()),
            cursor: 19,
            active_match: None,
            spawned: false,
            abandoned: false,
        };
        client
            .batch(
                json!({"cursor":20,"events":[{"sequence":20,"kind":"timing_confirmed",
            "payload":{"match_id":id}}]}),
                &output,
            )
            .unwrap();
        assert_eq!(client.active_match.as_deref(), Some(id));
        input.recv().unwrap();
        // Grant cryptographic binding is tested at the app's existing decoder;
        // this worker test bounds shell cursor recovery and identity lifetime.
        let response = json!({"match_id":id,"cursor":30,"relay":{
            "address":"127.0.0.1:5000","ticket":"test","packet_key":"test",
            "payload_key":"test","source_index":1,"ordering_certificate":"test"}});
        let mut other = response.clone();
        other["match_id"] = json!("22222222-2222-2222-2222-222222222222");
        assert!(client.accept_match_resume(other, &output).is_err());
        let mut old = response.clone();
        old["cursor"] = json!(18);
        assert!(client.accept_match_resume(old, &output).is_err());
        assert_eq!(client.cursor, 20);
        assert!(input.try_recv().is_err());
        client.accept_match_resume(response, &output).unwrap();
        assert_eq!(client.cursor, 30);
        assert!(client.token.is_some());
        let Response::Event(event) = input.recv().unwrap() else {
            panic!("missing renewal");
        };
        assert_eq!(event.kind, "relay_renewed");
        assert_eq!(event.payload["match_id"], id);
    }
}
