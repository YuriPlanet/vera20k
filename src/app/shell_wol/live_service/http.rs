//! Blocking HTTPS is confined to this worker. Session bearer tokens never
//! enter Debug output, file storage, error strings, or the gameplay client state.

use base64::Engine;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use super::wire::{ChannelList, Envelope, EventBatch, Session};
use serde_json::{Value, json};

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
    Channels(u32),
    /// `GET /v1/stats`; a failure is dropped and the next poll asks again.
    Stats,
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
    },
    LoginFailed {
        attempt: u64,
        code: u32,
    },
    UpgradeRequired {
        attempt: u64,
    },
    Channels(ChannelList),
    Stats(crate::ui::wol_shell::WolLobbyStats),
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
    StartRejected {
        id: u64,
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
// The server retains1024 events and returns at most256 at a time. Permit
// concurrent arrivals too, but never release cleanup on a partial history.
const MAX_CLEANUP_DRAIN_BATCHES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestFailure {
    Status(u16),
    LostCursor,
    ChannelRejected { status: u16, code: u32 },
    AlreadyFinished,
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
            Some("room_full") if status == 409 => Self::ChannelRejected {
                status,
                code: 0x8004_006E,
            },
            Some("room_unavailable") if status == 409 => Self::ChannelRejected {
                status,
                code: 0x8004_0070,
            },
            Some("unknown_match") if status == 409 => Self::AlreadyFinished,
            _ => Self::Status(status),
        }
    }

    fn command_failure(self, operation: &str) -> CommandFailure {
        match self {
            Self::AlreadyFinished if operation == "finish" => CommandFailure::AlreadyFinished,
            Self::ChannelRejected { code, .. } => CommandFailure::Rejected(code),
            _ if matches!(self.status(), 0 | 429 | 500..=599) => CommandFailure::Retryable,
            _ => CommandFailure::Rejected(0x8000_4005),
        }
    }

    fn status(self) -> u16 {
        match self {
            Self::Status(status) | Self::ChannelRejected { status, .. } => status,
            Self::LostCursor | Self::AlreadyFinished => 409,
        }
    }
}

struct Client {
    stopping: Arc<AtomicBool>,
    agent: ureq::Agent,
    base: String,
    token: Option<String>,
    cursor: u64,
    active_match: Option<String>,
}

fn decode_session(value: Value, requested_nickname: &str) -> Result<Session, RequestFailure> {
    let session: Session = serde_json::from_value(value).map_err(|_| RequestFailure::Status(0))?;
    super::validate_online_identity(&session.player_id, &session.nickname)
        .map_err(|_| RequestFailure::Status(0))?;
    // A successful response may preserve the requested name's casing, but
    // cannot replace the name the player chose. Session credentials
    // remain private; validation errors never contain response field values.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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
    fn session_failed(&mut self, error: RequestFailure, output: &Sender<Response>) -> bool {
        if error == RequestFailure::LostCursor
            && let Some(match_id) = self.active_match.clone()
        {
            let recovered = self
                .request(&format!("/v1/match/resume?match_id={match_id}"), None)
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
                let _ = self.request(
                    "/v1/commands",
                    Some(&json!({
                        "request_id":"lost-cursor-logout","operation":"logout","payload":{}
                    })),
                );
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

    fn poll_finished(
        &mut self,
        result: Result<(), RequestFailure>,
        failed_polls: &mut u8,
        output: &Sender<Response>,
    ) -> Duration {
        match result {
            Ok(()) => {
                *failed_polls = 0;
                Duration::from_secs(1)
            }
            Err(error) if self.session_failed(error, output) => Duration::from_secs(30),
            Err(_) => {
                *failed_polls = failed_polls.saturating_add(1);
                Duration::from_secs((1u64 << (*failed_polls).min(5)).min(30))
            }
        }
    }

    fn request(&self, path: &str, body: Option<&Value>) -> Result<Value, RequestFailure> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(RequestFailure::Status(0));
        }
        let url = format!("{}{path}", self.base);
        let response = if let Some(body) = body {
            let mut request = self.agent.post(&url).header(
                "User-Agent",
                concat!("VERA20k/", env!("CARGO_PKG_VERSION"), " multiplayer-client"),
            );
            if let Some(token) = &self.token {
                request = request.header("Authorization", format!("Bearer {token}"));
            }
            request.send_json(body)
        } else {
            let mut request = self.agent.get(&url).header(
                "User-Agent",
                concat!("VERA20k/", env!("CARGO_PKG_VERSION"), " multiplayer-client"),
            );
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

    fn drain_events(&mut self, output: &Sender<Response>) -> Result<(), RequestFailure> {
        for _ in 0..MAX_CLEANUP_DRAIN_BATCHES {
            let value = self.request(&format!("/v1/events?after={}", self.cursor), None)?;
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

/// The model takes about a second, so translation has its own thread and
/// polling and commands never wait for it. A job carries the current bearer.
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
    };
    let output = output.clone();
    std::thread::Builder::new()
        .name("vera-translate".into())
        .spawn(move || {
            for (id, token, text, target) in input {
                translator.token = Some(token);
                let body = json!({"text":text,"target":target});
                let outcome = match translator.request("/v1/translate", Some(&body)) {
                    Ok(value) => value["text"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or(TranslateFailure::Unavailable),
                    Err(RequestFailure::Status(400)) => Err(TranslateFailure::Unsupported),
                    Err(_) => Err(TranslateFailure::Unavailable),
                };
                if output.send(Response::Translated { id, outcome }).is_err() {
                    break;
                }
            }
        })
        .ok()
        .map(|_| jobs)
}

pub(super) fn run(
    base: String,
    input: Receiver<Request>,
    output: Sender<Response>,
    stopping: Arc<AtomicBool>,
) {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(12)))
        .build();
    let mut client = Client {
        stopping: Arc::clone(&stopping),
        agent: config.into(),
        base,
        token: None,
        cursor: 0,
        active_match: None,
    };
    let translator = spawn_translator(&client, &output);
    let mut failed_polls = 0_u8;
    let mut next_poll = Instant::now();
    loop {
        if stopping.load(Ordering::Acquire) {
            break;
        }
        let request = match input.recv_timeout(Duration::from_secs(1)) {
            Ok(request) => Some(request),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
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
                    client.request(
                        &format!(
                            "/v1/maps/{seed_digest}?offset={offset}&resource={}",
                            if resource == crate::map::source::TransferResource::Preview {
                                "preview"
                            } else {
                                "map"
                            }
                        ),
                        None,
                    )
                } else {
                    Err(RequestFailure::Status(400))
                };
                if let Err(error) = result.as_ref() {
                    client.session_failed(*error, &output);
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
                let result = client
                    .request("/v1/commands", Some(&body))
                    .and_then(|value| client.batch(value, &output));
                if let Err(error) = result.as_ref() {
                    client.session_failed(*error, &output);
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
                // A new username cannot prove ownership of an old bearer.
                // The service's existing cleanup gate must retire it first.
                if client.token.is_some() {
                    let _ = output.send(Response::LoginFailed {
                        attempt,
                        code: 0x8004_0064,
                    });
                    continue;
                }
                client.cursor = 0;
                client.active_match = None;
                let result = client
                    .request(
                        "/v1/sessions",
                        Some(&json!({"nickname": nickname, "protocol": 4})),
                    )
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
                    Err(RequestFailure::Status(426)) => {
                        let _ = output.send(Response::UpgradeRequired { attempt });
                    }
                    Err(_) => {
                        let _ = output.send(Response::LoginFailed { attempt, code: 0 });
                    }
                }
            }
            Some(Request::Channels(kind)) => {
                match client
                    .request(&format!("/v1/channels?kind={kind}"), None)
                    .and_then(|value| {
                        serde_json::from_value(value).map_err(|_| RequestFailure::Status(0))
                    }) {
                    Ok(channels) => {
                        let _ = output.send(Response::Channels(channels));
                    }
                    Err(error) => {
                        if !client.session_failed(error, &output) {
                            let _ = output
                                .send(Response::Failure("Could not refresh the online lobby."));
                        }
                    }
                }
            }
            Some(Request::Stats) => {
                if let Ok(stats) = client.request("/v1/stats", None).and_then(|value| {
                    serde_json::from_value(value).map_err(|_| RequestFailure::Status(0))
                }) {
                    let _ = output.send(Response::Stats(stats));
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
                    // left to clean up, including Back from the welcome page.
                    let _ = output.send(Response::CommandCompleted { id, operation });
                    continue;
                }
                let body = json!({"request_id":id.to_string(),"after":client.cursor,"operation":operation,"payload":payload});
                let result = client
                    .request("/v1/commands", Some(&body))
                    .or_else(|error| {
                        if error == RequestFailure::Status(0) {
                            client.request("/v1/commands", Some(&body))
                        } else {
                            Err(error)
                        }
                    })
                    .and_then(|value| {
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
                    client.drain_events(&output)
                } else {
                    result
                };
                let completed = result.is_ok();
                if let Err(error) = result {
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
                        if !client.session_failed(error, &output) && operation != "renew_relay" =>
                    {
                        let _ = output.send(if operation == "start" {
                            Response::StartRejected { id }
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
        if client.token.is_some() && Instant::now() >= next_poll {
            let result = client
                .request(&format!("/v1/events?after={}", client.cursor), None)
                .and_then(|value| client.batch(value, &output));
            next_poll = Instant::now() + client.poll_finished(result, &mut failed_polls, &output);
        }
    }
}

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
        };
        let mut failures = 0;
        assert_eq!(
            client.poll_finished(Err(RequestFailure::Status(0)), &mut failures, &output),
            Duration::from_secs(2)
        );
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
        assert!(input.try_recv().is_err());
        assert_eq!(
            client.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(1)
        );
        assert_eq!(failures, 0);
        client.poll_finished(Err(RequestFailure::Status(401)), &mut failures, &output);
        assert!(client.token.is_none());
        assert!(matches!(input.try_recv(), Ok(Response::Offline)));
        assert!(client.session_failed(RequestFailure::Status(401), &output));
        assert!(input.try_recv().is_err());
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
        client.poll_finished(Err(expired), &mut 0, &output);
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
