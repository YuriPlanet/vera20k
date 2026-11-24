//! Lobby HTTP responses entering the WOL reducers, and decoding the relay and
//! watch grants they carry.

use super::{
    CANT_CONNECT_TEXT, LOBBY_LOST_TEXT, LiveService, WolLoginAccepted, WolServiceEvent,
    apply_event, cancel_pending, end_room, fail_start, forget_deleted_account, forget_sign_in,
    http, lobby_says_outdated, maps, notice, options, remember_online_name, remember_sign_in,
    restore_accepted_options, service_event, settings_dir, spawned_visit_mut, update_required_text,
    wire,
};
use crate::app::AppState;
use crate::app::shell_network_setup::NetworkSetupEvent;
use crate::i18n::tr;
use crate::ui::wol_account::AccountSuccess;
use crate::util::time::Instant;
use anyhow::{Result, anyhow, ensure};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::SocketAddrV4;

pub(super) fn apply_response(
    state: &mut AppState,
    service: &mut LiveService,
    response: http::Response,
) {
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
            generated_guest,
            player_id,
            nickname,
            motd,
            sign_in_token,
            lobby_visits,
            campaign_rooms,
        } if attempt == service.auth_attempt
            && state.frontend.wol.as_ref().is_some_and(|visit| {
                matches!(
                    visit.awaiting,
                    crate::app::shell_wol::service::Awaiting::Connect { .. }
                )
            }) =>
        {
            // The wait may have ended before its queued CancelWait reaches the worker.
            // Only a still-awaited session can set the current name or saved credentials.
            if !generated_guest {
                if let Some(token) = sign_in_token {
                    remember_sign_in(settings_dir(state), service, &nickname, token);
                }
                remember_online_name(state, &nickname);
            }
            service.player_id = player_id;
            state.frontend.wol_account.nickname = nickname;
            state.frontend.wol_account.password.clear();
            // The operator's welcome is the only one: the game has none of its own.
            let motd = motd
                .filter(|motd| !motd.trim().is_empty())
                .unwrap_or_default();
            service_event(
                state,
                WolServiceEvent::Login(Ok(WolLoginAccepted {
                    motd,
                    lobby_visits,
                    campaign_rooms,
                })),
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
        http::Response::WatchListFailed => {
            super::lobby::watch_list_refresh(state, super::lobby::WatchListRefresh::Failed);
        }
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
            );
            // Only the public list's own refresh establishes freshness. A
            // room's option probe may belong to an older publication.
            if refused_options.is_none() {
                let refresh = if watches {
                    super::lobby::WatchListRefresh::Listed
                } else {
                    super::lobby::WatchListRefresh::Unsupported
                };
                super::lobby::watch_list_refresh(state, refresh);
            }
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
                    if let Some(switch) = service.switching_room.as_mut()
                        && switch.request_id == id
                        && failure == http::CommandFailure::Retryable
                    {
                        switch.failed(Instant::now());
                        return;
                    }
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
            "options"
                if failure == http::CommandFailure::InvalidOptions
                    && service.is_room_publication(id)
                    && state.match_state.session.campaign_room().is_some() =>
            {
                if let Some(visit) = state.frontend.wol.as_mut() {
                    visit.lobby_campaign_rooms = false;
                }
                notice(state, &tr("VERA:CampaignRoomOnlineUnsupported"));
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
            if let Ok(AccountSuccess::Deleted(name)) = &answer {
                forget_deleted_account(state, &service.lobby, name);
            }
            if state
                .frontend
                .wol
                .as_ref()
                .is_some_and(|visit| visit.account_attempt == attempt)
            {
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

pub(super) fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| anyhow!("invalid lobby message"))
}

/// A `chat` event's payload.
#[derive(Deserialize)]
pub(super) struct Chat {
    pub(super) from: wire::Member,
    pub(super) text: String,
    #[serde(default)]
    pub(super) private: bool,
    #[serde(default)]
    pub(super) action: bool,
    // The matchbot's replies; its `Start` carries the pairing.
    #[serde(default)]
    pub(super) ansi: bool,
    #[serde(default)]
    pub(super) quick_match: Option<super::quick_match::MatchStart>,
    #[serde(default)]
    pub(super) sent_at: Option<u64>,
    /// VERA: said in the public lobby this player visits from their room.
    #[serde(default)]
    pub(super) visit: bool,
    /// The lobby's own lines as phrases ([`super::lobby_phrases`]), read
    /// only once the event decodes, so that a shape this game does not know
    /// leaves the line in `text`.
    #[serde(default)]
    pub(super) phrases: Value,
}

impl Chat {
    /// The line in the language in use when the lobby said it as phrases
    /// that VERA's language files hold, else the lobby's `text`, and whether
    /// it is the former. A matchbot reply's phrases say what follows its
    /// first word, the reply's kind, which its Quick Match runner reads
    /// (`Info`).
    pub(super) fn shown_text(&self) -> (Vec<u16>, bool) {
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

    /// Whether the line is the lobby's own and says one of `keys`. Only the
    /// lobby's own lines carry `phrases`. Only their keys are read, so values
    /// a newer lobby adds that this game cannot show still count.
    fn says_any(&self, keys: &[&str]) -> bool {
        #[derive(Deserialize)]
        struct Said {
            key: String,
        }
        Vec::<Said>::deserialize(&self.phrases).is_ok_and(|phrases| {
            phrases
                .iter()
                .any(|phrase| keys.contains(&phrase.key.as_str()))
        })
    }

    /// The lobby's line that a newer release is out (`NewRelease`), which a
    /// game older than the newest release hears once per release.
    pub(super) fn announces_release(&self) -> bool {
        self.says_any(&["NewRelease"])
    }

    /// VERA: the lobby's line that a player's game differs from the host's,
    /// which no match between them can start with: another version, build or
    /// rules (docs/lobby-protocol.md). The room shows it red
    /// (`wol_chat::WARNING_COLOR`).
    pub(super) fn warns_of_other_game(&self) -> bool {
        self.says_any(&[
            "VersionMismatch",
            "ReleaseMismatch",
            "HostReleaseMismatch",
            "BuildMismatch",
            "RulesMismatch",
            "HostRulesMismatch",
            "RulesDifferFromHost",
            "HostRulesDifferFromOthers",
        ])
    }

    /// The pairing of the matchbot's `Start` that `local` hosts; only its
    /// ANSI text carries one.
    pub(super) fn hosted_pairing(&self, local: &str) -> Option<super::quick_match::MatchStart> {
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

pub(super) fn relay_uuid(value: &str) -> Result<[u8; 16]> {
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

pub(super) fn validate_online_identity(player_id: &str, nickname: &str) -> Result<()> {
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

pub(super) fn decode_relay_grant(
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
pub(super) fn decode_watch_grant(
    grant: &wire::WatchGrant,
    match_id: &str,
) -> Result<DecodedRelayGrant> {
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
