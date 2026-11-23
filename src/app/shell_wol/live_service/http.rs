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
use super::wire::{ChannelList, Envelope, Match, MatchList, Session};
use crate::ui::chat_translation::{ChatLanguage, ChatReply};
use serde_json::Value;
use serde_json::json;

pub(super) enum MapIo {
    ReadMapUpload {
        source: crate::map::source::TransferMapSource,
        digest: String,
        total: usize,
    },
    MapChunk {
        resource: crate::map::source::TransferResource,
        seed_digest: String,
        digest: String,
        offset: usize,
        /// A watcher's download names the match it watches
        /// (docs/lobby-protocol.md "Watching").
        match_id: Option<String>,
    },
    UploadMapChunk {
        resource: crate::map::source::TransferResource,
        id: u64,
        digest: String,
        total: usize,
        offset: usize,
        bytes: Vec<u8>,
    },
    CachedMap {
        resource: crate::map::source::TransferResource,
        directory: PathBuf,
        digest: String,
        total: usize,
    },
    InstallMap {
        resource: crate::map::source::TransferResource,
        directory: PathBuf,
        digest: String,
        total: usize,
        bytes: Vec<u8>,
    },
}

pub(super) enum Request {
    Map {
        transfer: u64,
        active: std::sync::Weak<()>,
        io: MapIo,
        completed: Sender<Request>,
    },
    MapFinished(map_io::Finished),
    OpenSession {
        attempt: u64,
        nickname: String,
        credential: Credential,
        /// What this game changes from the original game, the `Vera-Changes`
        /// sign-in header (`agreement::changes_header`).
        changes: String,
    },
    /// An `/v1/accounts/` request, answered by [`Response::AccountAnswered`]
    /// with the same `attempt`.
    Account {
        attempt: u64,
        request: crate::ui::wol_account::AccountRequest,
    },
    /// `/v1/accounts/forget`: the lobby drops this computer's remembered
    /// sign-in. Nothing answers.
    ForgetSignIn {
        name: String,
        sign_in_token: String,
    },
    /// The shell gave up on an unanswering lobby outside a match: stop
    /// polling and log this bearer out at the next sign-in.
    Abandon,
    /// The match the live service now holds (`LiveService::hold_match`), or
    /// none.
    HoldMatch(Option<String>),
    /// An external launch's bearer for its match API (docs/spawn.md), in
    /// place of `/v1/sessions`. Polling starts with the first event.
    AdoptSession {
        token: String,
        /// A returning player's match and the end of its event history; the
        /// `timing_confirmed` before it belongs to the process that left.
        resume: Option<(String, u64)>,
        /// A watcher's bearer (docs/spawn.md "Watching"): it polls only on
        /// `Request::PollEvents` and sends no command.
        watch: bool,
    },
    /// One `/v1/events` poll under a watcher's adopted bearer, which the
    /// game service answers with `watch_renewed`.
    PollEvents,
    Channels(u32),
    /// `GET /v1/matches`, answered by [`Response::Matches`].
    Matches {
        refused_options: Option<u64>,
    },
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

/// How a sign-in proves its name (docs/lobby-protocol.md "Accounts"). It
/// holds a password or a remembered sign-in, so it has no Debug.
pub(super) enum Credential {
    /// No password: a guest, or `name_claimed` for an account's name.
    Guest,
    /// A name assigned by Play as Guest, which may be replaced on a collision.
    GeneratedGuest,
    /// The typed password; `remember` asks for a `sign_in_token`.
    Password { password: String, remember: bool },
    /// The `sign_in_token` this computer remembers for the name.
    Remembered(String),
}

/// The `/v1/sessions` body: a password or a remembered sign-in, never both.
fn session_body(nickname: &str, credential: &Credential) -> Value {
    let mut body = json!({"nickname": nickname, "protocol": 4});
    match credential {
        Credential::Guest | Credential::GeneratedGuest => {}
        Credential::Password { password, remember } => {
            body["password"] = json!(password);
            body["remember"] = json!(remember);
        }
        Credential::Remembered(token) => body["sign_in_token"] = json!(token),
    }
    body
}

/// An account request's path and body.
fn account_call(request: &crate::ui::wol_account::AccountRequest) -> (&'static str, Value) {
    use crate::ui::wol_account::AccountRequest;
    match request {
        AccountRequest::Register {
            name,
            email,
            password,
        } => (
            "/v1/accounts/register",
            json!({"name": name, "email": email, "password": password}),
        ),
        AccountRequest::Verify { name, code } => {
            ("/v1/accounts/verify", json!({"name": name, "code": code}))
        }
        AccountRequest::Forgot { name } => ("/v1/accounts/forgot", json!({"name": name})),
        AccountRequest::Reset {
            name,
            code,
            password,
        } => (
            "/v1/accounts/reset",
            json!({"name": name, "code": code, "password": password}),
        ),
        AccountRequest::Delete {
            identifier,
            password,
        } => {
            let key = if identifier.contains('@') {
                "email"
            } else {
                "name"
            };
            (
                "/v1/accounts/delete",
                json!({(key): identifier, "password": password}),
            )
        }
    }
}

fn account_result(
    request: &super::AccountRequest,
    response: Result<Value, RequestFailure>,
) -> Result<super::AccountSuccess, String> {
    use crate::ui::wol_account::valid_name;
    let super::AccountRequest::Delete { identifier, .. } = request else {
        return response
            .map(|_| super::AccountSuccess::Accepted)
            .map_err(sign_in_refusal);
    };
    let by_name = valid_name(identifier);
    let reply = response.map_err(|failure| match failure {
        RequestFailure::Account {
            status: 400,
            invalid_request: true,
            ..
        } if !by_name => crate::i18n::tr("VERA:DeleteUseUsername"),
        other => sign_in_refusal(other),
    })?;
    // Older lobbies accept only usernames and return {}. Never infer a username
    // from an email, or clear another sign-in on a malformed reply.
    let canonical = reply["name"]
        .as_str()
        .filter(|name| valid_name(name) && (!by_name || name.eq_ignore_ascii_case(identifier)))
        .or_else(|| {
            (by_name && reply.as_object().is_some_and(|o| o.is_empty())).then_some(identifier)
        });
    canonical
        .map(|name| super::AccountSuccess::Deleted(name.to_owned()))
        .ok_or_else(|| crate::i18n::tr("VERA:DeleteNameUnavailable"))
}

impl Request {
    /// The periodic requests a waiting copy already covers: the 10 s retry
    /// of a player's `renew_relay` and a watcher's `renew_watch`, and the
    /// lobby's lists, asked for every few seconds.
    fn repeats(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Channels(kind), Self::Channels(other)) => kind == other,
            (
                Self::Matches {
                    refused_options: None,
                },
                Self::Matches {
                    refused_options: None,
                },
            ) => true,
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
                matches!(*operation, "renew_relay" | "renew_watch")
                    && operation == other_operation
                    && payload == other_payload
            }
            _ => false,
        }
    }

    /// The room's settings and the answers to them, each whole: a newer one
    /// makes a copy still waiting stale.
    fn supersedes(&self, waiting: &Self) -> bool {
        match (self, waiting) {
            (
                Self::Command { operation, .. },
                Self::Command {
                    operation: waiting_operation,
                    ..
                },
            ) => {
                matches!(*operation, "options" | "map_status" | "ready")
                    && operation == waiting_operation
            }
            _ => false,
        }
    }
}

/// Queues what arrived during the last request behind `queued`, without a
/// periodic request that `current` or a queued one repeats. A failing command
/// takes about 24 s, so copies would otherwise pile up ahead of Ready, Start
/// and chat for as long as the lobby is unreachable. The room's settings and
/// the answers to them go out newest only: a host dragging a slider would
/// otherwise queue a send, and every player two answers, for each step.
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
            queued.retain(|waiting| !request.supersedes(waiting));
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
        generated_guest: bool,
        player_id: String,
        nickname: String,
        motd: Option<String>,
        /// A password sign-in with `remember`: what signs this computer in
        /// next time.
        sign_in_token: Option<String>,
        /// The lobby offers `visit_lobby`.
        lobby_visits: bool,
        /// The lobby offers `campaign_rooms`.
        campaign_rooms: bool,
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
    /// The matches the player can watch, or `None` from a lobby from before
    /// watching, which answers 404.
    Matches {
        list: Option<Vec<Match>>,
        refused_options: Option<u64>,
    },
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
    /// A notice for the lobby's chat, by its `VERA:` label.
    Failure(&'static str),
    /// An ordinary watch-list refresh failed; the shell retains its last rows.
    WatchListFailed,
    Offline,
    /// The first event poll of an outage failed; polling keeps retrying.
    LobbyUnreachable,
    /// An event poll succeeded again after `LobbyUnreachable`.
    LobbyReachable,
    /// About a minute of failed polls outside a match: polling stopped.
    LobbyLost,
    /// The lobby refused a sign-in for a reason of its own (maintenance,
    /// full, busy, or the name's account): the text to show.
    SignInRefused {
        attempt: u64,
        message: String,
        /// 403 `wrong_password` or 404 `no_account`: the password or
        /// remembered sign-in signs in to no account.
        bad_credential: bool,
    },
    /// The answer to a [`Request::Account`], or the message to show.
    AccountAnswered {
        attempt: u64,
        answer: Result<super::AccountSuccess, String>,
    },
    StartRejected {
        id: u64,
        /// 429, 5xx or no answer: the lobby, not the setup, refused it.
        retryable: bool,
    },
    Translated {
        id: u64,
        outcome: Result<ChatReply, TranslateFailure>,
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
pub(super) const USER_AGENT: &str =
    concat!("VERA20k/", env!("CARGO_PKG_VERSION"), " multiplayer-client");
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

/// What a game says about itself when it signs in, over either transport
/// (docs/lobby-protocol.md "Sign-in headers"). It names its version here
/// too, since a browser cannot set the User-Agent released games name it in.
/// The browser's gateway forwards only the headers it lists
/// (tools/wasm/gateway/server.mjs).
fn sign_in_headers(changes: &str) -> [(&'static str, String); 7] {
    [
        ("Vera-Version", env!("CARGO_PKG_VERSION").to_owned()),
        (
            "Vera-Build",
            crate::app::shell_network_setup::agreement::version(),
        ),
        ("Vera-Quick-Match", "1".to_owned()),
        ("Vera-Host-Handover", "1".to_owned()),
        ("Vera-Watch-AI", "1".to_owned()),
        ("Vera-Language", crate::i18n::selected_language()),
        ("Vera-Changes", changes.to_owned()),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RequestFailure {
    Status(u16),
    /// A session nickname is in use (409), or belongs to an account
    /// (403, with its localized message). Neither error created a session.
    SessionNameInUse,
    SessionNameClaimed(String),
    LostCursor,
    ChannelRejected {
        status: u16,
        code: u32,
    },
    AlreadyFinished,
    /// 409 `match_active`: the lobby refuses a command this player's own
    /// running match blocks (docs/lobby-protocol.md "Watching").
    MatchActive,
    /// 400 `invalid_options` ([`CommandFailure::InvalidOptions`]).
    InvalidOptions,
    /// 409 `stale_revision` ([`CommandFailure::StaleRevision`]).
    StaleRevision,
    /// 426 `upgrade_required` (docs/lobby-protocol.md), with the lobby's
    /// message when it is one short line of plain text.
    UpgradeRequired(Option<String>),
    /// 503 `maintenance`: the operator closed sign-ins, with the same kind
    /// of message.
    Maintenance(Option<String>),
    /// 503 `service_full` or `network_full`: the lobby has no room for this
    /// sign-in, with the same kind of message.
    Full(Option<String>),
    /// A refusal about an account (docs/lobby-protocol.md "Accounts"): a
    /// sign-in's `name_claimed`, `wrong_password`, `no_account` or
    /// `account_locked`, or any refusal of an `/v1/accounts/` request, with
    /// the message to show.
    Account {
        status: u16,
        /// `wrong_password` or `no_account`: the credential signs in to no
        /// account.
        bad_credential: bool,
        invalid_request: bool,
        message: String,
    },
}

/// What the shell hears when a sign-in gets no session. A 409 is the name
/// in use (0x8004_0064, TXT_LOGIN_USED); code 0 is no usable answer,
/// including a 401 or 403 without the lobby's code.
fn sign_in_failure(attempt: u64, failure: RequestFailure) -> Response {
    match failure {
        RequestFailure::SessionNameInUse | RequestFailure::Status(409) => Response::LoginFailed {
            attempt,
            code: 0x8004_0064,
        },
        RequestFailure::UpgradeRequired(message) => Response::UpgradeRequired { attempt, message },
        error @ (RequestFailure::SessionNameClaimed(_)
        | RequestFailure::Maintenance(_)
        | RequestFailure::Full(_)
        | RequestFailure::Account { .. }
        | RequestFailure::Status(429)) => {
            let bad_credential = matches!(
                error,
                RequestFailure::Account {
                    bad_credential: true,
                    ..
                }
            );
            Response::SignInRefused {
                attempt,
                message: sign_in_refusal(error),
                bad_credential,
            }
        }
        _ => Response::LoginFailed { attempt, code: 0 },
    }
}

/// What a lobby refusal of a sign-in tells the player: its own plain
/// message when it gave one.
fn sign_in_refusal(failure: RequestFailure) -> String {
    match failure {
        RequestFailure::Maintenance(message) => {
            message.unwrap_or_else(|| crate::i18n::tr("VERA:OnlineMaintenance"))
        }
        RequestFailure::Full(message) => {
            message.unwrap_or_else(|| crate::i18n::tr("VERA:OnlineFull"))
        }
        RequestFailure::SessionNameClaimed(message) | RequestFailure::Account { message, .. } => {
            message
        }
        _ => crate::i18n::tr("VERA:OnlineBusy"),
    }
}

/// The lobby's `error.message` when it is one short line of plain text: in
/// the language in use when the lobby said it as phrases that VERA's language
/// files hold.
fn plain_message(value: Option<&Value>) -> Option<String> {
    let error = &value?["error"];
    let said = serde_json::from_value::<Vec<crate::app::shell_wol::lobby_phrases::Phrase>>(
        error["phrases"].clone(),
    )
    .ok()
    .and_then(|phrases| crate::app::shell_wol::lobby_phrases::localized(&phrases));
    said.as_deref()
        .or_else(|| error["message"].as_str())
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
    /// 400 `invalid_options`: the lobby cannot take the room's options, as
    /// one from before a field they carry (`launch.options.teams`) does.
    InvalidOptions,
    /// 409 `stale_revision`: the room has moved on from the settings a
    /// `ready`, `map_status` or `start` was for (they changed, or a player
    /// came or went), or has none yet.
    StaleRevision,
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
            Some("name_unavailable") if status == 409 => Self::ChannelRejected {
                status,
                code: crate::app::shell_wol::lobby::ROOM_NAME_TAKEN,
            },
            Some("room_unavailable") if status == 409 => Self::ChannelRejected {
                status,
                code: 0x8004_0070,
            },
            Some("unknown_match") if status == 409 => Self::AlreadyFinished,
            Some("match_active") if status == 409 => Self::MatchActive,
            // A `watch` of a match with all its watchers
            // (docs/lobby-protocol.md "Watching").
            Some("watchers_full") if status == 409 => Self::ChannelRejected {
                status,
                code: crate::app::shell_wol::lobby::WATCHERS_FULL,
            },
            Some("invalid_options") if status == 400 => Self::InvalidOptions,
            Some("stale_revision") if status == 409 => Self::StaleRevision,
            Some("maintenance") if status == 503 => {
                Self::Maintenance(plain_message(value.as_ref()))
            }
            Some("service_full" | "network_full") if status == 503 => {
                Self::Full(plain_message(value.as_ref()))
            }
            Some(code @ ("name_claimed" | "wrong_password")) if status == 403 => {
                Self::account(status, Some(code), value.as_ref())
            }
            Some(code @ "no_account") if status == 404 => {
                Self::account(status, Some(code), value.as_ref())
            }
            Some(code @ "account_locked") if status == 429 => {
                Self::account(status, Some(code), value.as_ref())
            }
            _ if status == 426 => Self::UpgradeRequired(plain_message(value.as_ref())),
            _ => Self::Status(status),
        }
    }

    /// An answer to `path`: an `/v1/accounts/` request's refusals all
    /// carry their message, since `name_unavailable` there names an account,
    /// and so do a sign-in's wrong passwords from one network and its
    /// accounts being unavailable. A sign-in's `name_unavailable` is a name
    /// already online or reserved, while `name_claimed` is an account's name.
    /// Only these two replies let the game draw another guest name. A 401 on any path is the
    /// bearer the lobby no longer knows (`session_failed`).
    fn answer(path: &str, status: u16, bytes: &[u8]) -> Self {
        if status == 401 {
            return Self::Status(401);
        }
        let value = serde_json::from_slice::<Value>(bytes).ok();
        let code = value
            .as_ref()
            .and_then(|value| value["error"]["code"].as_str());
        if path == "/v1/sessions" {
            match (status, code) {
                (409, Some("name_unavailable")) => return Self::SessionNameInUse,
                (403, Some("name_claimed")) => {
                    let message = plain_message(value.as_ref())
                        .unwrap_or_else(|| crate::i18n::tr("VERA:NameHasAccount"));
                    return Self::SessionNameClaimed(message);
                }
                _ => {}
            }
        }
        let sign_in = path == "/v1/sessions"
            && matches!(
                (status, code),
                (429, Some("rate_limited")) | (503, Some("accounts_unavailable"))
            );
        if !path.starts_with("/v1/accounts/") && !sign_in {
            return Self::response(status, bytes);
        }
        Self::account(status, code, value.as_ref())
    }

    /// An account refusal with the lobby's plain message, or VERA's for its
    /// code.
    fn account(status: u16, code: Option<&str>, value: Option<&Value>) -> Self {
        let message = plain_message(value).unwrap_or_else(|| {
            crate::i18n::tr(match code {
                Some("name_claimed") => "VERA:NameHasAccount",
                Some("wrong_password") => "VERA:WrongPassword",
                Some("no_account") => "VERA:NoSuchAccount",
                Some("account_locked") => "VERA:AccountLocked",
                _ if status == 429 => "VERA:TooManyTries",
                _ if status == 0 || status >= 500 => "VERA:OnlineBusy",
                _ => "VERA:OnlineRefusedEntry",
            })
        });
        Self::Account {
            status,
            bad_credential: matches!(code, Some("wrong_password" | "no_account")),
            invalid_request: code == Some("invalid_request"),
            message,
        }
    }

    fn command_failure(&self, operation: &str) -> CommandFailure {
        match self {
            Self::AlreadyFinished if operation == "finish" => CommandFailure::AlreadyFinished,
            // `watch` of a match that ended or that this player may not watch.
            Self::AlreadyFinished if operation == "watch" => {
                CommandFailure::Rejected(crate::app::shell_wol::lobby::WATCH_UNKNOWN_MATCH)
            }
            // Only a refused `watch` says why; other commands get the plain
            // refusal below.
            Self::MatchActive if operation == "watch" => {
                CommandFailure::Rejected(crate::app::shell_wol::lobby::WATCH_MATCH_ACTIVE)
            }
            Self::ChannelRejected { code, .. } => CommandFailure::Rejected(*code),
            Self::InvalidOptions => CommandFailure::InvalidOptions,
            Self::StaleRevision => CommandFailure::StaleRevision,
            _ if matches!(self.status(), 0 | 429 | 500..=599) => CommandFailure::Retryable,
            _ => CommandFailure::Rejected(0x8000_4005),
        }
    }

    fn status(&self) -> u16 {
        match self {
            Self::Status(status) | Self::ChannelRejected { status, .. } => *status,
            Self::SessionNameClaimed(_) => 403,
            Self::SessionNameInUse
            | Self::LostCursor
            | Self::AlreadyFinished
            | Self::MatchActive
            | Self::StaleRevision => 409,
            Self::InvalidOptions => 400,
            Self::UpgradeRequired(_) => 426,
            Self::Maintenance(_) | Self::Full(_) => 503,
            Self::Account { status, .. } => *status,
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
    /// The `Vera-Changes` header its sign-ins send, from the last
    /// `Request::OpenSession`.
    changes: String,
    /// The match `/v1/match/resume` recovers after a lost event history and
    /// a held bearer's logout finishes first: the live service's, as
    /// `Request::HoldMatch` last named it, or a returning launcher's from
    /// `AdoptSession` until the service holds it.
    active_match: Option<String>,
    /// The bearer came from an external launcher, not `/v1/sessions`.
    spawned: bool,
    /// The adopted bearer is a watcher's: no polling of its own and no
    /// commands, only `Request::PollEvents`.
    watching: bool,
    /// The match service refused a watcher's renewal for good (its match
    /// ended, or the bearer is no longer valid): no more polls.
    watch_refused: bool,
    /// The lobby stopped answering and the player was told the connection
    /// was lost: no more polls; the next sign-in logs this bearer out first.
    abandoned: bool,
}

/// A watcher's renewal poll the match service refused for good: its match
/// ended (409 `unknown_match`), or its bearer acts no more (401, 403). A
/// network failure (status 0), 408, 429 and 5xx are worth another try.
fn watch_renewal_refused(error: &RequestFailure) -> bool {
    let status = error.status();
    (400..500).contains(&status) && !matches!(status, 408 | 429)
}

fn decode_session(value: Value, requested_nickname: &str) -> Result<Session, RequestFailure> {
    let session: Session = serde_json::from_value(value).map_err(|_| RequestFailure::Status(0))?;
    super::validate_online_identity(&session.player_id, &session.nickname)
        .map_err(|_| RequestFailure::Status(0))?;
    // A successful response may preserve the requested name's casing, but
    // cannot replace the name the player chose. Session credentials
    // remain private; validation errors never contain response field values.
    if !session.nickname.eq_ignore_ascii_case(requested_nickname)
        || session.token.len() != 43
        || !base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&session.token)
            .is_ok_and(|token| token.len() == 32)
    {
        return Err(RequestFailure::Status(0));
    }
    Ok(session)
}

/// A translation job: its id, the bearer it runs under, the text and the
/// target language.
type TranslateJob = (u64, String, String, &'static str);

impl Client {
    /// A client on its transport before any sign-in.
    fn new(
        stopping: Arc<AtomicBool>,
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        agent: ureq::Agent,
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        base: String,
        #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
        browser: u32,
    ) -> Self {
        Self {
            stopping,
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
            agent,
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
            base,
            #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
            browser,
            token: None,
            cursor: 0,
            changes: String::new(),
            active_match: None,
            spawned: false,
            watching: false,
            watch_refused: false,
            abandoned: false,
        }
    }

    /// A client on the same transport with no session of its own: a derived
    /// `Clone` would copy the live bearer.
    fn sibling(&self) -> Self {
        Self::new(
            Arc::clone(&self.stopping),
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
            self.agent.clone(),
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
            self.base.clone(),
            #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
            self.browser,
        )
    }

    fn adopt_session(&mut self, token: String, resume: Option<(String, u64)>, watch: bool) {
        self.token = Some(token);
        let (active_match, cursor) = resume.map_or((None, 0), |(id, cursor)| (Some(id), cursor));
        self.cursor = cursor;
        self.active_match = active_match;
        self.spawned = true;
        self.watching = watch;
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

    /// A stream frame or map upload reply can arrive after a newer batch.
    /// Discard an overtaken cursor and deliver overlapping events once.
    fn overlapping_batch(
        &mut self,
        frame: Value,
        output: &Sender<Response>,
    ) -> Result<(), RequestFailure> {
        if frame["cursor"]
            .as_u64()
            .is_some_and(|cursor| cursor < self.cursor)
        {
            return Ok(());
        }
        self.batch(frame, output)
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

    fn request(&self, path: &str, body: Option<&Value>) -> Result<Value, RequestFailure> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(RequestFailure::Status(0));
        }
        let url = format!("{}{path}", self.base);
        let response = if let Some(body) = body {
            let mut request = self.agent.post(&url).header("User-Agent", USER_AGENT);
            if path == "/v1/sessions" {
                // Its relay CONFIRM carries a federation roster. A browser
                // game does not say so, and its gateway would not forward it
                // (FORWARDED_HEADERS), so its matches stay on one relay.
                request = request.header("Vera-Relay-Federation", "1");
                for (name, value) in sign_in_headers(&self.changes) {
                    request = request.header(name, value);
                }
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
            return Err(RequestFailure::answer(path, status, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(|_| RequestFailure::Status(0))
    }
}

fn translation_result(
    result: Result<Value, RequestFailure>,
) -> Result<ChatReply, TranslateFailure> {
    match result {
        Ok(value) => value["text"]
            .as_str()
            .map(|text| ChatReply {
                text: text.to_owned(),
                source: value["source"].as_str().and_then(ChatLanguage::from_tag),
            })
            .ok_or(TranslateFailure::Unavailable),
        Err(RequestFailure::Status(400)) => Err(TranslateFailure::Unsupported),
        Err(_) => Err(TranslateFailure::Unavailable),
    }
}

/// The model takes about a second, so translation has its own thread and
/// polling and commands never wait for it.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn spawn_translator(client: &Client, output: &Sender<Response>) -> Option<Sender<TranslateJob>> {
    let (jobs, input) = std::sync::mpsc::channel::<TranslateJob>();
    let mut translator = client.sibling();
    let output = output.clone();
    std::thread::Builder::new()
        .name("vera-translate".into())
        .spawn(move || {
            for job in input {
                if output.send(translator.translate(job)).is_err() {
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
    let client = Client::new(stopping, config.into(), base);
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

mod map_io;

macro_rules! serial_driver {
    ($($asynchronous:tt)?) => {
        impl Client {
    /// Retry only explicit collisions for a name this game assigned. A network
    /// failure could already have created a session under the first name.
    $($asynchronous)? fn open_session(
        &self, attempt: u64, mut nickname: String, credential: &Credential,
        mut superseded: impl FnMut() -> bool,
    ) -> Result<Session, Response> {
        let mut retries_left = 3;
        loop {
            let body = session_body(&nickname, credential);
            let answer = platform_await!(self.request("/v1/sessions", Some(&body)));
            match answer {
                Err(RequestFailure::SessionNameInUse | RequestFailure::SessionNameClaimed(_))
                    if matches!(credential, Credential::GeneratedGuest) => {}
                other => return other.and_then(|value| decode_session(value, &nickname))
                    .map_err(|error| sign_in_failure(attempt, error)),
            }
            if retries_left == 0 || self.stopping.load(Ordering::Acquire) || superseded() {
                return Err(Response::SignInRefused {
                    attempt, message: crate::i18n::tr("VERA:GuestNameBusy"), bad_credential: false,
                });
            }
            retries_left -= 1;
            nickname = crate::ui::wol_account::guest_nickname();
        }
    }

    /// One translation job, under the bearer it carries.
    $($asynchronous)? fn translate(&mut self, (id, token, text, target): TranslateJob) -> Response {
        self.token = Some(token);
        let body = json!({"text":text,"target":target});
        let outcome = translation_result(platform_await!(self.request("/v1/translate", Some(&body))));
        Response::Translated { id, outcome }
    }

    /// Log out a bearer this worker still holds (given up in an outage, or
    /// whose logout the shell stopped retrying), which still holds its name.
    /// The lobby refuses to log out a session in a match, so the match the
    /// live service held with it is finished first. False while the lobby
    /// is away.
    $($asynchronous)? fn release_held_session(&mut self) -> bool {
        if self.token.is_none() {
            return true;
        }
        if let Some(match_id) = self.active_match.clone() {
            let body = json!({"request_id":"held-finish","after":self.cursor,"operation":"finish","payload":{"match_id":match_id}});
            let _ = platform_await!(self.request("/v1/commands", Some(&body)));
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
            // A 426 refuses this request alone: the session stays, and its
            // player hears once per sign-in that an update is needed.
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

    /// A lobby list's answer. A failure that keeps the session says so once
    /// until a list arrives again: the lobby asks every few seconds.
    $($asynchronous)? fn list_finished(
        &mut self,
        result: Result<ChannelList, RequestFailure>,
        refresh_failed: &mut bool,
        output: &Sender<Response>,
    ) {
        match result {
            Ok(channels) => {
                *refresh_failed = false;
                let _ = output.send(Response::Channels(channels));
            }
            Err(error) => {
                if !platform_await!(self.session_failed(error, output))
                    && !std::mem::replace(refresh_failed, true)
                {
                    let _ = output.send(Response::Failure("VERA:OnlineRefreshFailed"));
                }
            }
        }
    }

    $($asynchronous)? fn matches_finished(
        &mut self,
        result: Result<MatchList, RequestFailure>,
        refused_options: Option<u64>,
        output: &Sender<Response>,
    ) {
        let list = match result {
            Ok(list) => Some(list.matches),
            // A lobby from before watching has no watch list.
            Err(RequestFailure::Status(404)) => None,
            Err(error) => {
                let outdated = matches!(error, RequestFailure::UpgradeRequired(_));
                if !platform_await!(self.session_failed(error, output))
                    && refused_options.is_none() && !outdated && self.token.is_some()
                {
                    let _ = output.send(Response::WatchListFailed);
                }
                return;
            }
        };
        let _ = output.send(Response::Matches { list, refused_options });
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
            translator: Option<Sender<TranslateJob>>,
        ) {
    let stopping = Arc::clone(&client.stopping);
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    let mut streams = stream::Streams::new(&client.base);
    let mut failed_polls = 0_u8;
    let mut refresh_failed = false;
    let mut maps: Option<map_io::Worker> = None;
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
        if maps.as_ref().is_some_and(|worker| !worker.belongs_to(&client)) {
            maps = None;
        }
        if matches!(&request, Some(Request::Abandon | Request::OpenSession { .. } | Request::Command { operation: "logout", .. })) {
            maps = None;
        }
        match request {
            Some(Request::Map { transfer, active, io, completed }) => {
                if active.strong_count() != 0 {
                    if maps.is_none() && client.token.is_some() && !client.abandoned {
                        maps = map_io::Worker::start(&client);
                    }
                    let job = map_io::Job { transfer, active, after: client.cursor, io, completed };
                    if !maps.as_ref().is_some_and(|worker| worker.send(job)) {
                        let _ = output.send(Response::MapFailure { transfer, retryable: false });
                    }
                }
            }
            Some(Request::MapFinished(reply)) => {
                if maps.as_ref().is_some_and(|worker| worker.owns(&reply)) {
                    platform_await!(client.finish_map(reply, &output));
                }
            }
            Some(Request::OpenSession {
                attempt,
                nickname,
                credential,
                changes,
            }) => {
                client.changes = changes;
                // A held bearer still holds its name: log it out first, and
                // while the lobby is away, report that instead of the name.
                if !platform_await!(client.release_held_session()) {
                    let _ = output.send(Response::LoginFailed { attempt, code: 0 });
                    continue;
                }
                client.cursor = 0;
                let result = platform_await!(client.open_session(attempt, nickname, &credential, || {
                    queue_arrivals(&input, None, &mut queued);
                    queued.iter().any(|request| matches!(request,
                        Request::Abandon | Request::OpenSession { .. } | Request::Command { operation: "logout", .. }))
                }));
                match result {
                    Ok(session) => {
                        client.token = Some(session.token);
                        failed_polls = 0;
                        if output
                            .send(Response::Authenticated {
                                attempt,
                                generated_guest: matches!(credential, Credential::GeneratedGuest),
                                player_id: session.player_id,
                                nickname: session.nickname,
                                motd: session.motd,
                                lobby_visits: session.features.iter().any(|feature| feature == "visit_lobby"),
                                campaign_rooms: session.features.iter().any(|feature| feature == "campaign_rooms"),
                                sign_in_token: session
                                    .sign_in_token
                                    .filter(|_| matches!(credential, Credential::Password { remember: true, .. })),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(refusal) => {
                        let _ = output.send(refusal);
                    }
                }
            }
            Some(Request::Account { attempt, request }) => {
                let (path, body) = account_call(&request);
                let answer = account_result(&request, platform_await!(client.request(path, Some(&body))));
                let _ = output.send(Response::AccountAnswered { attempt, answer });
            }
            Some(Request::ForgetSignIn {
                name,
                sign_in_token,
            }) => {
                let body = json!({"name": name, "sign_in_token": sign_in_token});
                if let Err(error) = platform_await!(client.request("/v1/accounts/forget", Some(&body))) {
                    log::warn!("Could not forget the remembered sign-in: status={}", error.status());
                }
            }
            Some(Request::Abandon) => client.abandoned = client.token.is_some(),
            Some(Request::HoldMatch(match_id)) => client.active_match = match_id,
            Some(Request::AdoptSession { token, resume, watch }) => {
                if client.token.is_none() {
                    client.adopt_session(token, resume, watch);
                    failed_polls = 0;
                }
            }
            Some(Request::PollEvents) => {
                if client.watching && !client.abandoned && !client.watch_refused {
                    let result = platform_await!(client
                        .request(&format!("/v1/events?after={}", client.cursor), None))
                        .and_then(|value| client.batch(value, &output));
                    // The live service asks again at its renewal retry,
                    // unless the service said no for good.
                    if let Err(error) = result {
                        client.watch_refused = watch_renewal_refused(&error);
                        if client.watch_refused {
                            log::info!(
                                "Watch renewal refused: status={}; renewal stops",
                                error.status()
                            );
                        } else {
                            log::warn!("Watch renewal poll failed: status={}", error.status());
                        }
                    }
                }
            }
            Some(Request::Channels(kind)) => {
                let list = platform_await!(client
                    .request(&format!("/v1/channels?kind={kind}"), None))
                    .and_then(|value| {
                        serde_json::from_value(value).map_err(|_| RequestFailure::Status(0))
                    });
                platform_await!(client.list_finished(list, &mut refresh_failed, &output));
            }
            Some(Request::Matches { refused_options }) => {
                let list = platform_await!(client.request("/v1/matches", None)).and_then(|value| {
                    serde_json::from_value::<MatchList>(value)
                        .map_err(|_| RequestFailure::Status(0))
                });
                platform_await!(client.matches_finished(list, refused_options, &output));
            }
            Some(Request::Command {
                id,
                operation,
                payload,
            }) => {
                if client.watching {
                    // A watcher's bearer acts as no player (docs/spawn.md
                    // "Watching"): the live service sends it no command.
                    let _ = output.send(Response::CommandCompleted { id, operation });
                    continue;
                }
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
                if client.overlapping_batch(frame, &output).is_err() {
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
        if client.token.is_some()
            && !client.watching
            && !client.abandoned
            && !streaming
            && Instant::now() >= next_poll
        {
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
fn next_request<T>(input: &Receiver<T>) -> Result<Option<T>, ()> {
    match input.recv_timeout(Duration::from_millis(50)) {
        Ok(request) => Ok(Some(request)),
        Err(RecvTimeoutError::Timeout) => Ok(None),
        Err(RecvTimeoutError::Disconnected) => Err(()),
    }
}
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
async fn next_request<T>(input: &Receiver<T>) -> Result<Option<T>, ()> {
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

#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
pub(in crate::app::shell_wol) use browser::browser_stats;

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    /// docs/spawn.md "Watching": an ended match or a dead bearer stops a
    /// watcher's renewal; an outage or a busy service does not.
    #[test]
    fn a_watchers_renewal_stops_only_on_a_final_refusal() {
        for status in [401, 403, 409] {
            assert!(
                watch_renewal_refused(&RequestFailure::Status(status)),
                "{status}"
            );
        }
        assert!(watch_renewal_refused(&RequestFailure::LostCursor));
        for status in [0, 408, 429, 500, 503] {
            assert!(
                !watch_renewal_refused(&RequestFailure::Status(status)),
                "{status}"
            );
        }
    }

    fn test_client() -> Client {
        Client::new(
            Arc::new(AtomicBool::new(false)),
            ureq::Agent::new_with_defaults(),
            String::new(),
        )
    }

    /// A client holding a bearer, its events read up to `cursor`.
    fn signed_in_client(cursor: u64) -> Client {
        let mut client = test_client();
        client.token = Some("synthetic-test-session".to_owned());
        client.cursor = cursor;
        client
    }

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
    fn authentication_response_rejects_malformed_player_and_bearer() {
        let response = session_response();
        let nickname = response["nickname"].as_str().unwrap();
        let mut malformed_player = response.clone();
        malformed_player["player_id"] = json!("not-a-player-uuid");
        assert!(decode_session(malformed_player, nickname).is_err());
        let mut malformed_bearer = response.clone();
        malformed_bearer["token"] = json!("untrusted\r\nHeader: value");
        assert!(decode_session(malformed_bearer, nickname).is_err());
    }

    /// The lobby owns a session's expiry: a game whose clock runs ahead of
    /// the lobby's still signs in.
    #[test]
    fn a_session_signs_in_whatever_the_local_clock_says() {
        let mut response = session_response();
        response["expires_at"] = json!(1);
        let nickname = response["nickname"].as_str().unwrap().to_owned();
        assert!(decode_session(response, &nickname).is_ok());
    }

    /// A refused `watch` reaches the lobby's box with the lobby's reason; a
    /// lobby that cannot answer gets the usual refusal.
    #[test]
    fn a_refused_watch_says_why() {
        use crate::app::shell_wol::lobby::watch_refusal_key;
        let refusal = |code: &str| {
            let body = format!(r#"{{"error":{{"code":"{code}"}}}}"#);
            let failure = RequestFailure::response(409, body.as_bytes());
            watch_refusal_key(failure.command_failure("watch").wol_code())
        };
        assert_eq!(refusal("unknown_match"), "VERA:WatchUnknownMatch");
        assert_eq!(refusal("match_active"), "VERA:WatchMatchActive");
        assert_eq!(refusal("watchers_full"), "VERA:WatchersFull");
        let busy = RequestFailure::Status(503).command_failure("watch");
        assert_eq!(
            watch_refusal_key(busy.wol_code()),
            "VERA:OnlineRequestRefused"
        );
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
        let taken = RequestFailure::response(409, br#"{"error":{"code":"name_unavailable"}}"#);
        assert_eq!(
            taken.command_failure("create"),
            CommandFailure::Rejected(crate::app::shell_wol::lobby::ROOM_NAME_TAKEN)
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
        // A command the player's own running match blocks; only `watch`
        // says why.
        let playing = RequestFailure::response(409, br#"{"error":{"code":"match_active"}}"#);
        assert_eq!(
            playing.command_failure("join"),
            CommandFailure::Rejected(0x8000_4005)
        );
        // Options a lobby cannot take, apart from a player who has left.
        let options = RequestFailure::response(400, br#"{"error":{"code":"invalid_options"}}"#);
        assert_eq!(
            options.command_failure("options"),
            CommandFailure::InvalidOptions
        );
        let left = RequestFailure::response(409, br#"{"error":{"code":"player_not_in_room"}}"#);
        assert_eq!(
            left.command_failure("options"),
            CommandFailure::Rejected(0x8000_4005)
        );
        // Settings that moved on while a status for the older ones was on
        // its way: the room says nothing and answers the newer ones.
        let stale = RequestFailure::response(409, br#"{"error":{"code":"stale_revision"}}"#);
        assert_eq!(
            stale.command_failure("map_status"),
            CommandFailure::StaleRevision
        );
    }

    /// A server holds the map response open and sends a real WebSocket room
    /// event. Delivery must not depend on the map server answering first.
    #[test]
    fn a_room_event_reaches_the_game_while_its_map_request_is_stalled() {
        for upload in [false, true] {
            room_event_during_map_chunk(upload);
        }
    }

    fn room_event_during_map_chunk(upload: bool) {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::mpsc::channel;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted, sockets) = channel();
        let acceptor = std::thread::spawn(move || {
            for socket in listener.incoming() {
                if accepted.send(socket.unwrap()).is_err() {
                    break;
                }
            }
        });
        let mut client = signed_in_client(0);
        client.base = format!("http://{address}");
        let (requests, input) = channel();
        let (output, events) = channel();
        let stop = Arc::clone(&client.stopping);
        let worker = std::thread::spawn(move || run_loop(client, input, output, None));
        let timeout = Duration::from_secs(3);
        let active = Arc::new(());
        let mut stream = None;
        let digest = "ab".repeat(32);
        let mut held = loop {
            let mut socket = sockets.recv_timeout(timeout).unwrap();
            socket.set_read_timeout(Some(timeout)).unwrap();
            let mut header = [0; 128];
            let read = socket.peek(&mut header).unwrap();
            if header[..read].starts_with(b"GET /v1/events/stream?") {
                stream = Some(tungstenite::accept(socket).unwrap());
                let resource = crate::map::source::TransferResource::Map;
                let io = if upload {
                    MapIo::UploadMapChunk {
                        resource,
                        id: 9,
                        digest: digest.clone(),
                        total: 1,
                        offset: 0,
                        bytes: vec![1],
                    }
                } else {
                    MapIo::MapChunk {
                        resource,
                        seed_digest: digest.clone(),
                        digest: digest.clone(),
                        offset: 0,
                        match_id: None,
                    }
                };
                requests
                    .send(Request::Map {
                        transfer: 3,
                        active: Arc::downgrade(&active),
                        completed: requests.clone(),
                        io,
                    })
                    .unwrap();
            } else {
                let mut request = BufReader::new(&socket);
                let mut content_length = 0;
                for header in (&mut request).lines() {
                    let header = header.unwrap();
                    if header.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        content_length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; content_length];
                request.read_exact(&mut body).unwrap();
                if header[..read].starts_with(b"POST /v1/commands") {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&body).unwrap()["operation"],
                        "map_upload"
                    );
                    break socket;
                }
                if header[..read].starts_with(b"GET /v1/maps/") {
                    break socket;
                }
                assert!(header[..read].starts_with(b"GET /v1/events?"));
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 24\r\nConnection: close\r\n\r\n{\"cursor\":0,\"events\":[]}").unwrap();
            }
        };
        let mut stream = stream.unwrap();
        let event =
            json!({"cursor":1,"events":[{"sequence":1,"kind":"channel_left","payload":{}}]});
        stream
            .send(tungstenite::Message::text(event.to_string()))
            .unwrap();
        let before_map_reply = matches!(events.recv_timeout(timeout), Ok(Response::Event(envelope)) if envelope.kind == "channel_left");
        // The host's HTTP reply started at cursor zero; the room event above
        // has already advanced the session to one. Its upload still completed.
        let reply = if upload {
            json!({"cursor":0,"events":[]})
        } else {
            json!({"digest":digest,"byte_length":1,"offset":0,"data":"AQ","complete":true})
        }
        .to_string();
        write!(
            held,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        )
        .unwrap();
        drop(held);
        let completed = match events.recv_timeout(timeout) {
            Ok(Response::MapUploaded {
                transfer: 3,
                completed: 1,
            }) => upload,
            Ok(Response::MapChunk {
                transfer: 3, bytes, ..
            }) => !upload && bytes == [1],
            _ => false,
        };
        stop.store(true, Ordering::Release);
        drop(stream);
        drop(requests);
        worker.join().unwrap();
        drop(sockets);
        let _ = TcpStream::connect(address);
        acceptor.join().unwrap();
        assert!(
            before_map_reply,
            "the stalled map request held the room event (upload={upload})"
        );
        assert!(
            completed,
            "the transfer failed after the room advanced (upload={upload})"
        );
    }

    type LobbyRequests = Vec<(String, Value)>;

    /// A lobby on a local port. It reads one request per connection (the
    /// head, then a `Content-Length` JSON body), answers with the status and
    /// body `answer` gives for them, and closes. A connection that sends
    /// nothing ends it ([`close_lobby`]).
    fn local_lobby(
        mut answer: impl FnMut(&str, &Value) -> (u16, Value) + Send + 'static,
    ) -> (std::net::SocketAddr, std::thread::JoinHandle<LobbyRequests>) {
        use std::io::{BufRead, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let lobby = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for stream in listener.incoming() {
                let mut reader = std::io::BufReader::new(stream.unwrap());
                let (mut head, mut length, mut line) = (String::new(), 0, String::new());
                while reader.read_line(&mut line).unwrap_or(0) > 2 {
                    let lower = line.to_ascii_lowercase();
                    if let Some(value) = lower.strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                    head.push_str(&line);
                    line.clear();
                }
                if head.is_empty() {
                    break;
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap_or_default();
                let (status, reply) = answer(&head, &body);
                let reply = reply.to_string();
                let _ = write!(
                    reader.into_inner(),
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                seen.push((head, body));
            }
            seen
        });
        (address, lobby)
    }

    /// Ends a [`local_lobby`]; gives each request's head and body, in order.
    fn close_lobby(
        address: std::net::SocketAddr,
        lobby: std::thread::JoinHandle<LobbyRequests>,
    ) -> LobbyRequests {
        drop(std::net::TcpStream::connect(address).unwrap());
        lobby.join().unwrap()
    }

    /// The path a request's head asks for.
    fn request_path(head: &str) -> &str {
        head.split(' ').nth(1).unwrap_or_default()
    }

    /// docs/spawn.md "Watching": a watcher's bearer polls only when asked,
    /// sends no command over HTTP, and stops for good once its match ended.
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    #[test]
    fn a_watchers_worker_polls_on_request_and_stops_at_its_matchs_end() {
        let (address, lobby) = local_lobby(|head, _| {
            if request_path(head) == "/v1/events?after=0" {
                (
                    200,
                    json!({"cursor":1,"events":[{"sequence":1,"kind":"watch_renewed",
                        "payload":{"match_id":"m"}}]}),
                )
            } else {
                (409, json!({"error":{"code":"unknown_match"}}))
            }
        });
        let base = format!("http://{address}");
        let (requests, input) = std::sync::mpsc::channel();
        let (output, responses) = std::sync::mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker = {
            let stopping = Arc::clone(&stopping);
            std::thread::spawn(move || run(base, input, output, stopping))
        };
        let next = || responses.recv_timeout(Duration::from_secs(10)).unwrap();
        let command = |id| Request::Command {
            id,
            operation: "leave",
            payload: json!({}),
        };
        requests
            .send(Request::AdoptSession {
                token: "watch-token".into(),
                resume: None,
                watch: true,
            })
            .unwrap();
        requests.send(Request::PollEvents).unwrap();
        assert!(matches!(next(), Response::Event(event) if event.kind == "watch_renewed"));
        // The match ended: refused, and never asked again.
        for id in [1, 2] {
            requests.send(Request::PollEvents).unwrap();
            requests.send(command(id)).unwrap();
            assert!(matches!(
                next(),
                Response::CommandCompleted { id: done, operation: "leave" } if done == id
            ));
        }
        stopping.store(true, Ordering::Release);
        drop(requests);
        worker.join().unwrap();
        let seen = close_lobby(address, lobby);
        let paths: Vec<_> = seen.iter().map(|(head, _)| request_path(head)).collect();
        assert_eq!(paths, ["/v1/events?after=0", "/v1/events?after=1"]);
    }

    #[test]
    fn only_generated_names_retry_collisions_and_they_stop_after_four_attempts() {
        let mut asked = 0;
        let (address, lobby) = local_lobby(move |_, _| {
            asked += 1;
            let (status, code) = if asked % 2 == 0 {
                (403, "name_claimed")
            } else {
                (409, "name_unavailable")
            };
            (status, json!({"error":{"code":code}}))
        });
        let client = Client::new(
            Arc::new(AtomicBool::new(false)),
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
            format!("http://{address}"),
        );
        let named = client.open_session(1, "Guest00001".into(), &Credential::Guest, || false);
        assert!(matches!(
            named,
            Err(Response::LoginFailed {
                code: 0x8004_0064,
                ..
            })
        ));
        let automatic =
            client.open_session(2, "Guest00002".into(), &Credential::GeneratedGuest, || {
                false
            });
        assert!(
            matches!(automatic, Err(Response::SignInRefused { message, .. })
            if message == crate::i18n::tr("VERA:GuestNameBusy"))
        );
        let bodies = close_lobby(address, lobby);
        assert_eq!(bodies.len(), 5);
        for (_, body) in bodies {
            let name = body["nickname"].as_str().unwrap();
            let digits = name.strip_prefix("Guest").unwrap();
            assert_eq!(digits.len(), 5);
            assert!(digits.bytes().all(|b| b.is_ascii_digit()));
            assert_eq!(body, json!({"nickname":name,"protocol":4}));
        }
    }

    /// A bearer the shell stopped logging out (Back in an outage, or a
    /// sign-in that answered after its wait) is logged out before the next
    /// sign-in instead of reporting the name in use; while the lobby is
    /// away it stays held.
    #[test]
    fn a_held_bearer_is_logged_out_before_signing_in_again() {
        let (address, lobby) = local_lobby(|_, _| (200, json!({})));
        let mut client = signed_in_client(4);
        client.base = "http://127.0.0.1:1".to_owned();
        assert!(!client.release_held_session());
        assert!(client.token.is_some());
        client.base = format!("http://{address}");
        assert!(client.release_held_session());
        assert!(client.token.is_none());
        let seen = close_lobby(address, lobby);
        assert!(matches!(seen.as_slice(), [(_, body)] if body["operation"] == "logout"));
    }

    /// Quick Match leaves its room as its match starts. The match the live
    /// service holds still recovers a lost event history afterwards, and a
    /// held bearer's logout still finishes it first.
    #[test]
    fn a_quick_match_leave_keeps_the_match_to_resume_and_to_finish() {
        const MATCH: &str = "11111111-1111-1111-1111-111111111111";
        let (mut left, mut reads_after_leave) = (false, 0);
        let (address, lobby) = local_lobby(move |head, body| {
            let batch = json!({"cursor":1,"events":[]});
            match request_path(head) {
                "/v1/sessions" => (200, session_response()),
                "/v1/events?after=0" => (
                    200,
                    json!({"cursor":1,"events":[{"sequence":1,"kind":"timing_confirmed",
                        "payload":{"match_id":MATCH}}]}),
                ),
                "/v1/commands" => {
                    left |= body["operation"] == "leave";
                    (200, batch)
                }
                // The leave's own drain, then a poll after the lobby
                // lost this session's history.
                "/v1/events?after=1" if left => {
                    reads_after_leave += 1;
                    if reads_after_leave > 1 {
                        (409, json!({"error":{"code":"history_expired"}}))
                    } else {
                        (200, batch)
                    }
                }
                "/v1/events?after=1" => (200, batch),
                path if path == format!("/v1/match/resume?match_id={MATCH}") => (
                    200,
                    json!({"match_id":MATCH,"cursor":30,"relay":{
                        "address":"127.0.0.1:5000","ticket":"test","packet_key":"test",
                        "payload_key":"test","source_index":1,"ordering_certificate":"test"}}),
                ),
                _ => (404, json!({})),
            }
        });
        let base = format!("http://{address}");
        let (requests, input) = std::sync::mpsc::channel();
        let (output, responses) = std::sync::mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker = {
            let stopping = Arc::clone(&stopping);
            std::thread::spawn(move || run(base, input, output, stopping))
        };
        let next = || responses.recv_timeout(Duration::from_secs(10)).unwrap();
        let sign_in = |attempt| Request::OpenSession {
            attempt,
            nickname: "Player_01234567890123456".into(),
            credential: Credential::Guest,
            changes: String::new(),
        };
        requests.send(sign_in(1)).unwrap();
        assert!(matches!(next(), Response::Authenticated { attempt: 1, .. }));
        assert!(matches!(next(), Response::Event(event) if event.kind == "timing_confirmed"));
        // The service holds the match; Negotiated 0 leaves the room.
        requests
            .send(Request::HoldMatch(Some(MATCH.into())))
            .unwrap();
        let leave = Request::Command {
            id: 5,
            operation: "leave",
            payload: json!({}),
        };
        requests.send(leave).unwrap();
        assert!(matches!(
            next(),
            Response::CommandCompleted {
                id: 5,
                operation: "leave"
            }
        ));
        let Response::Event(renewed) = next() else {
            panic!("the lost history ended the session instead of resuming its match");
        };
        assert_eq!(renewed.kind, "relay_renewed");
        assert_eq!(renewed.payload["match_id"], MATCH);
        // Given up in an outage, the bearer is logged out at the next sign-in.
        requests.send(Request::Abandon).unwrap();
        requests.send(sign_in(2)).unwrap();
        assert!(matches!(next(), Response::Authenticated { attempt: 2, .. }));
        stopping.store(true, Ordering::Release);
        drop(requests);
        worker.join().unwrap();
        let seen = close_lobby(address, lobby);
        let held: Vec<_> = seen
            .iter()
            .filter_map(|(_, body)| body["request_id"].as_str().zip(Some(&body["payload"])))
            .filter(|(id, _)| id.starts_with("held-"))
            .collect();
        assert_eq!(
            held,
            [
                ("held-finish", &json!({"match_id":MATCH})),
                ("held-logout", &json!({}))
            ]
        );
    }

    /// From about a minute of failed polls on, each failed poll asks the
    /// shell to give up, until it does; then the bearer stays for the next
    /// sign-in and no stream reopens.
    #[test]
    fn a_lobby_lost_for_a_minute_asks_the_shell_until_it_gives_up() {
        let (output, input) = std::sync::mpsc::channel();
        let mut client = signed_in_client(19);
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
        let mut client = signed_in_client(19);
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
        let relay = |id, match_id| command(id, "renew_relay", json!({"match_id":match_id}));
        let watch = |id, match_id| command(id, "renew_watch", json!({"match_id":match_id}));
        for request in [
            command(2, "ready", json!({"ready":true})),
            relay(3, "a"),
            relay(4, "b"),
            relay(5, "b"),
            watch(6, "a"),
            watch(7, "a"),
            command(8, "chat", json!({"text":"gg"})),
            command(9, "chat", json!({"text":"gg"})),
        ] {
            requests.send(request).unwrap();
        }
        let mut queued = VecDeque::new();
        queue_arrivals(&input, Some(&relay(1, "a")), &mut queued);
        let ids: Vec<_> = queued
            .iter()
            .map(|request| match request {
                Request::Command { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, [Some(2), Some(4), Some(6), Some(8), Some(9)]);
    }

    #[test]
    fn a_waiting_lobby_list_covers_the_next_one() {
        let (requests, input) = std::sync::mpsc::channel();
        let matches = |refused_options| Request::Matches { refused_options };
        for request in [
            Request::Channels(41),
            Request::Channels(0),
            matches(None),
            matches(None),
            matches(Some(1)),
            matches(Some(1)),
        ] {
            requests.send(request).unwrap();
        }
        let mut queued = VecDeque::new();
        queue_arrivals(&input, Some(&Request::Channels(41)), &mut queued);
        let kept: Vec<_> = queued
            .iter()
            .map(|request| match request {
                Request::Channels(kind) => u64::from(*kind),
                Request::Matches { refused_options } => 100 + refused_options.unwrap_or(0),
                _ => u64::MAX,
            })
            .collect();
        assert_eq!(kept, [0, 100, 101, 101]);
    }

    #[test]
    fn a_lobby_that_stops_listing_says_so_once_until_it_lists_again() {
        let (output, input) = std::sync::mpsc::channel();
        let mut client = signed_in_client(1);
        let mut failed = false;
        let list = || ChannelList {
            kind: 41,
            channels: Vec::new(),
        };
        for _ in 0..2 {
            client.list_finished(Err(RequestFailure::Status(503)), &mut failed, &output);
        }
        assert!(matches!(input.try_recv(), Ok(Response::Failure(_))));
        assert!(input.try_recv().is_err());
        client.list_finished(Ok(list()), &mut failed, &output);
        assert!(matches!(input.try_recv(), Ok(Response::Channels(_))));
        client.list_finished(Err(RequestFailure::Status(503)), &mut failed, &output);
        assert!(matches!(input.try_recv(), Ok(Response::Failure(_))));
    }

    #[test]
    fn watch_list_failure_keeps_the_rows_and_legacy_probes_keep_their_revision() {
        let (output, input) = std::sync::mpsc::channel();
        let mut client = signed_in_client(1);
        client.matches_finished(Err(RequestFailure::Status(503)), None, &output);
        assert!(matches!(input.try_recv(), Ok(Response::WatchListFailed)));
        // A failed option probe has no watch rows to refresh. A 404 still
        // reaches the capability owner with the matching options revision.
        client.matches_finished(Err(RequestFailure::Status(503)), Some(9), &output);
        client.matches_finished(Err(RequestFailure::Status(404)), Some(9), &output);
        assert!(matches!(
            input.try_recv(),
            Ok(Response::Matches {
                list: None,
                refused_options: Some(9)
            })
        ));
        client.matches_finished(Err(RequestFailure::Status(401)), None, &output);
        assert!(matches!(input.try_recv(), Ok(Response::Offline)));
        assert!(input.try_recv().is_err());
    }

    #[test]
    fn newer_room_settings_replace_waiting_ones() {
        let (requests, input) = std::sync::mpsc::channel();
        let command = |id, operation, revision| Request::Command {
            id,
            operation,
            payload: json!({"revision":revision}),
        };
        for request in [
            command(2, "options", 2),
            command(3, "map_status", 1),
            command(4, "ready", 1),
            command(5, "chat", 0),
            command(6, "options", 3),
            command(7, "map_status", 2),
            command(8, "ready", 2),
        ] {
            requests.send(request).unwrap();
        }
        let mut queued = VecDeque::new();
        // The one being sent stays.
        queue_arrivals(&input, Some(&command(1, "options", 1)), &mut queued);
        let ids: Vec<_> = queued
            .iter()
            .map(|request| match request {
                Request::Command { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, [Some(5), Some(6), Some(7), Some(8)]);
    }

    #[test]
    fn expired_cursor_ends_the_session_once_but_a_room_conflict_does_not() {
        let (output, input) = std::sync::mpsc::channel();
        let mut client = signed_in_client(19);
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

    /// A sign-in under a name that is already online or reserved (the
    /// lobby's 409 `name_unavailable`) says the name is in use, not that the
    /// lobby could not be reached; a room's `name_unavailable` keeps its code.
    #[test]
    fn a_sign_in_name_in_use_is_not_a_failed_connection() {
        let taken = br#"{"error":{"code":"name_unavailable","message":"That player name is already in use. Choose another username."}}"#;
        let failure = RequestFailure::answer("/v1/sessions", 409, taken);
        assert!(matches!(
            sign_in_failure(7, failure),
            Response::LoginFailed {
                attempt: 7,
                code: 0x8004_0064
            }
        ));
        assert_eq!(
            RequestFailure::answer("/v1/commands", 409, taken).command_failure("create"),
            CommandFailure::Rejected(crate::app::shell_wol::lobby::ROOM_NAME_TAKEN)
        );
    }

    /// A sign-in refused without the lobby's code, as by something between
    /// the game and the lobby, is a failed connection.
    #[test]
    fn an_uncoded_sign_in_refusal_is_a_failed_connection() {
        for status in [401, 403] {
            assert!(matches!(
                sign_in_failure(7, RequestFailure::answer("/v1/sessions", status, b"")),
                Response::LoginFailed {
                    attempt: 7,
                    code: 0
                }
            ));
        }
    }

    /// An account's refusals show the lobby's words; only `wrong_password`
    /// and `no_account` say a remembered sign-in no longer works, and an
    /// account request's `name_unavailable` is a message, not a room's join
    /// code.
    #[test]
    fn delete_uses_one_identifier_and_only_forgets_a_confirmed_username() {
        use crate::ui::wol_account::{AccountRequest, AccountSuccess};
        let delete = |identifier: &str| AccountRequest::Delete {
            identifier: identifier.into(),
            password: "example-password".into(),
        };
        let username = delete("tanya");
        let email = delete("alias+yr@example.com");
        assert_eq!(
            account_call(&username).1,
            json!({"name":"tanya","password":"example-password"})
        );
        assert_eq!(
            account_call(&email).1,
            json!({"email":"alias+yr@example.com","password":"example-password"})
        );
        assert_eq!(
            account_result(&username, Ok(json!({}))),
            Ok(AccountSuccess::Deleted("tanya".into()))
        );
        assert_eq!(
            account_result(&email, Ok(json!({"name":"Tanya"}))),
            Ok(AccountSuccess::Deleted("Tanya".into()))
        );
        assert!(account_result(&email, Ok(json!({}))).is_err());
        assert!(account_result(&username, Ok(json!({"name":"SomeoneElse"}))).is_err());
        let legacy = RequestFailure::answer(
            "/v1/accounts/delete",
            400,
            br#"{"error":{"code":"invalid_request"}}"#,
        );
        assert_eq!(
            account_result(&email, Err(legacy)),
            Err(crate::i18n::tr("VERA:DeleteUseUsername"))
        );
    }

    #[test]
    fn account_refusals_carry_the_lobby_message() {
        let refusal = |code: &str, message: &str| {
            json!({"error":{"code":code,"message":message}}).to_string()
        };
        let wrong = RequestFailure::response(
            403,
            refusal("wrong_password", "Sign in again with your password.").as_bytes(),
        );
        assert_eq!(
            wrong,
            RequestFailure::Account {
                status: 403,
                bad_credential: true,
                invalid_request: false,
                message: "Sign in again with your password.".into(),
            }
        );
        let deleted = RequestFailure::response(404, br#"{"error":{"code":"no_account"}}"#);
        assert!(matches!(
            deleted,
            RequestFailure::Account {
                bad_credential: true,
                ..
            }
        ));
        let claimed = RequestFailure::response(403, br#"{"error":{"code":"name_claimed"}}"#);
        assert!(!matches!(
            claimed,
            RequestFailure::Account {
                bad_credential: true,
                ..
            }
        ));
        assert!(sign_in_refusal(claimed).contains("has an account"));
        let taken = RequestFailure::answer(
            "/v1/accounts/register",
            409,
            refusal("name_unavailable", "That name is taken.").as_bytes(),
        );
        assert_eq!(sign_in_refusal(taken), "That name is taken.");
        let network = refusal(
            "rate_limited",
            "Too many wrong passwords from your network.",
        );
        assert_eq!(
            sign_in_refusal(RequestFailure::answer(
                "/v1/sessions",
                429,
                network.as_bytes()
            )),
            "Too many wrong passwords from your network."
        );
        assert_eq!(
            RequestFailure::answer("/v1/maps", 429, network.as_bytes()),
            RequestFailure::Status(429)
        );
        // A bearer the lobby no longer knows, on an account path too.
        assert_eq!(
            RequestFailure::answer("/v1/accounts/register", 401, b""),
            RequestFailure::Status(401)
        );
    }

    /// A sign-in names the game's version, build and changes beside what it
    /// can do.
    #[test]
    fn a_sign_in_names_the_version_and_build() {
        let (address, lobby) = local_lobby(|_, _| (503, json!({})));
        let mut client = Client::new(
            Arc::new(AtomicBool::new(false)),
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
            format!("http://{address}"),
        );
        client.changes = "%7B%22ares%22%3A%22on%22%7D".into();
        let body = session_body("Tanya", &Credential::Guest);
        assert_eq!(
            client.request("/v1/sessions", Some(&body)),
            Err(RequestFailure::Status(503))
        );
        let (request, _) = close_lobby(address, lobby).remove(0);
        let header = |name: &str| {
            request.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_owned())
            })
        };
        assert_eq!(
            header("Vera-Version").as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(header("Vera-Relay-Federation").as_deref(), Some("1"));
        assert_eq!(
            header("Vera-Changes").as_deref(),
            Some("%7B%22ares%22%3A%22on%22%7D")
        );
        // The build `ready` and `map_status` carry, in the characters and
        // length the lobby takes (docs/lobby-protocol.md "Sign-in headers").
        let build = header("Vera-Build").unwrap();
        assert_eq!(build, crate::app::shell_network_setup::agreement::version());
        assert!(
            (1..=128).contains(&build.len())
                && build
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".:_+-".contains(&byte)),
            "{build}"
        );
    }

    /// The browser's gateway forwards every header a browser game's sign-in
    /// sends; it refuses the whole sign-in over any other.
    #[test]
    fn the_browser_gateway_forwards_every_sign_in_header() {
        let gateway = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/wasm/gateway/server.mjs"
        ))
        .unwrap();
        let (_, list) = gateway
            .split_once("const FORWARDED_HEADERS = Object.freeze([")
            .unwrap();
        let (forwarded, _) = list.split_once("])").unwrap();
        for (name, _) in sign_in_headers("") {
            let name = name.to_ascii_lowercase();
            assert!(forwarded.contains(&format!("'{name}'")), "{name}");
        }
    }

    /// A sign-in sends the typed password or the remembered sign-in, never
    /// both, and a guest sends neither.
    #[test]
    fn a_session_sends_a_password_or_a_remembered_sign_in() {
        let token = "A".repeat(43);
        let body = |credential| session_body("Tanya", &credential);
        let guest = body(Credential::Guest);
        assert_eq!(guest, json!({"nickname":"Tanya","protocol":4}));
        let password = body(Credential::Password {
            password: "s3cret-pw".into(),
            remember: true,
        });
        assert_eq!(password["password"], "s3cret-pw");
        assert_eq!(password["remember"], true);
        assert!(password.get("sign_in_token").is_none());
        let remembered = body(Credential::Remembered(token.clone()));
        assert_eq!(remembered["sign_in_token"], token);
        assert!(remembered.get("password").is_none() && remembered.get("remember").is_none());
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
        let mut client = signed_in_client(19);
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
        let mut client = test_client();
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
        let mut client = test_client();
        client.cursor = 5;
        let event = |sequence| json!({"sequence":sequence,"kind":"chat","payload":{}});
        client
            .overlapping_batch(json!({"cursor":4,"events":[event(4)]}), &output)
            .unwrap();
        assert_eq!(client.cursor, 5);
        client
            .overlapping_batch(
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
    fn a_spawned_match_polls_slowly_and_never_streams_once_its_match_is_held() {
        let (output, _input) = std::sync::mpsc::channel();
        // The live service holds the match `timing_confirmed` started.
        let held = Some("11111111-1111-1111-1111-111111111111".to_owned());
        let mut failures = 0;

        let mut spawned = test_client();
        // A returning engine's start is history: it polls at its match's pace at once.
        let mut returning = test_client();
        returning.adopt_session(
            "synthetic-test-session".to_owned(),
            Some(("11111111-1111-1111-1111-111111111111".to_owned(), 2)),
            false,
        );
        assert_eq!(returning.cursor, 2);
        assert_eq!(
            returning.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(60)
        );

        spawned.adopt_session("synthetic-test-session".to_owned(), None, false);
        assert_eq!(spawned.stream_bearer(), None);
        // The start barrier waits on the other players' events.
        assert_eq!(
            spawned.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(1)
        );
        spawned.active_match = held.clone();
        assert_eq!(
            spawned.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(60)
        );

        // A lobby session's match keeps its stream and its pace.
        let mut lobby = test_client();
        lobby.token = Some("synthetic-test-session".to_owned());
        assert_eq!(lobby.stream_bearer(), Some("synthetic-test-session"));
        lobby.active_match = held;
        assert_eq!(
            lobby.poll_finished(Ok(()), &mut failures, &output),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn active_match_resume_preserves_identity_and_rejects_another_match_or_old_cursor() {
        let (output, input) = std::sync::mpsc::channel();
        let id = "11111111-1111-1111-1111-111111111111";
        let mut client = signed_in_client(20);
        client.active_match = Some(id.to_owned());
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
