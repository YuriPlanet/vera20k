//! The LAN lobby's join flow: chat packets, discovered-game projection, lobby
//! requests, and the host/guest join offer, confirm, denial and cancel exchange.

use crate::net::global::GlobalDestination;
use crate::net::lan_messages::LanJoinRequest;
use crate::net::lan_messages::LanMessage;
use crate::net::lan_messages::LanNamedMessage;
use crate::net::lan_messages::LanParticipantDetails;
use crate::net::lan_messages::LanSessionProof;
use crate::net::lan_messages::SessionScope;
use std::net::SocketAddrV4;

use super::{
    AppState, DepartureReason, PeerAdmission, compatibility, direct, endpoint_id, fresh_nonce,
    local_name, options, project_players, send,
};

/// The chat packet `0x005DE360` sends to every other node. The lobby
/// (`0x005DDEB9`) and both setup rooms (host `0x005DE800`, guest `0x005E1FD5`)
/// pass it `[0x7EEF2C]`, the fixed `0x0000F0F0`, never the player's colour.
/// The caller shows the sender's own echo.
pub(super) fn chat_packet(
    name: &str,
    game_name: &str,
    text: &[u16],
) -> Result<LanMessage, crate::net::lan_messages::ingame::InGameMessageError> {
    LanMessage::in_game_chat(
        name,
        crate::ui::wol_chat::PUBLIC_COLOR,
        game_name,
        &String::from_utf16_lossy(text),
    )
}

/// A received chat packet as `0x005E00B0` shows it (`0x005E14BB..0x005E1564`):
/// `0x005E2700` draws `"[%s] %s"` (`0x0081B460`) in the packet's colour.
/// `0x005DE360` never sends to its own node, but VERA broadcasts the lobby's
/// chat and its own copy comes back; like the other reflections, it shows
/// nothing. Residual: gamemd tells lobby entries apart by address, so two
/// lobby players with the same name there still see each other's lines; here
/// they do not. A setup room refuses a second player with the same name.
pub(super) fn chat_line(
    local_name: &str,
    message: &LanMessage,
    chat: &crate::net::lan_messages::ingame::InGameChat<'_>,
) -> Option<crate::ui::lan_shell::LanChatLine> {
    if crate::net::lan_messages::wire_name_matches(local_name, message.name_bytes()) {
        return None;
    }
    let mut text: Vec<u16> = format!("[{}] ", message.name()).encode_utf16().collect();
    text.extend(chat.text_units());
    Some(crate::ui::lan_shell::LanChatLine {
        text,
        color: chat.color,
    })
}

pub(super) fn project_games(state: &mut AppState) {
    let Some(service) = state.match_state.lan_service.as_ref() else {
        return;
    };
    let games = service
        .games
        .iter()
        .map(|record| crate::ui::lan_shell::LanGameRow {
            id: endpoint_id(record.endpoint),
            name: record.name.clone(),
            open: record.open,
        })
        .collect();
    super::shell_lan::service_event(state, super::shell_lan::LanServiceEvent::Games(games));
}

pub(super) fn drain_requests(state: &mut AppState, now: u32) {
    let requests = state
        .frontend
        .lan
        .as_mut()
        .map(|lobby| lobby.take_service_requests())
        .unwrap_or_default();
    for request in requests {
        if let Err(error) = lobby_request(state, request) {
            log::warn!("LAN request failed: {error:#}");
        }
    }
    // Native5DB720 expires discovery advertisements after300 wall buckets.
    let Some(service) = state.match_state.lan_service.as_mut() else {
        return;
    };
    let old_len = service.games.len();
    service
        .games
        .retain(|record| (now.wrapping_sub(record.last_seen) as i32) <= 300);
    let changed = old_len != service.games.len();
    let old_participants = service.observed_participants.len();
    service
        .observed_participants
        .retain(|peer| (now.wrapping_sub(peer.last_seen) as i32) <= 360);
    let players_changed = old_participants != service.observed_participants.len();
    if changed {
        project_games(state);
    }
    if players_changed {
        project_players(state);
    }
}

fn lobby_request(
    state: &mut AppState,
    request: super::shell_lan::LanServiceRequest,
) -> anyhow::Result<()> {
    use super::shell_lan::LanServiceRequest;
    let name = local_name(state);
    match request {
        LanServiceRequest::Refresh => {
            state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?
                .discovery_timer = Default::default();
        }
        LanServiceRequest::SelectGame { game_id, change } => {
            let service = state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?;
            service.select_game(game_id, change);
            project_players(state);
        }
        LanServiceRequest::Join { game_id, observe } => {
            let peer = state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.game(game_id))
                .ok_or_else(|| anyhow::anyhow!("LAN game is no longer advertised"))?
                .endpoint;
            let local = state.frontend.multiplayer_shell_runtime.local_participant();
            let request = crate::net::lan_messages::LanJoinRequest {
                country: local.slot.country_shell_value(observe),
                color: local.slot.color_shell_value(),
                version: crate::net::lan_messages::LanVersionInterval::RETAIL,
                compatibility: compatibility(state)
                    .ok_or_else(|| anyhow::anyhow!("retail compatibility inputs unavailable"))?,
                observer_request: u32::from(observe),
            };
            let nonce = fresh_nonce()?;
            let proof = LanSessionProof {
                scope: SessionScope {
                    epoch: [0; 16],
                    revision: 0,
                },
                join_nonce: nonce,
                admission_nonce: [0; 16],
                version: super::shell_network_setup::agreement::version(),
                rules_digest: String::new(),
            };
            let join = LanMessage::join_request(&name, &[], request)?.with_session_proof(&proof)?;
            let service = state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?;
            if service.queue_setup(std::iter::once((join.as_bytes().to_vec(), direct(peer))))? {
                service.pending_host = Some(peer);
                service.join_nonce = Some(nonce);
                service.admission_nonce = None;
            }
        }
        LanServiceRequest::CancelJoin { game_id } => {
            let now = super::match_runtime::frame_pacer::frame_bucket(
                super::match_runtime::frame_pacer::wall_clock_ms(),
            );
            let service = state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?;
            if service.pending_host.is_some() {
                service.begin_departure(
                    &state.match_state.session,
                    DepartureReason::CancelJoin { game_id },
                    now,
                )?;
            } else {
                super::shell_lan::service_event(
                    state,
                    super::shell_lan::LanServiceEvent::JoinCanceled { game_id },
                );
            }
        }
        LanServiceRequest::NameChanged => {
            if let Some(service) = state.match_state.lan_service.as_mut() {
                service.force_presence = true;
            }
        }
        LanServiceRequest::Chat { text } => {
            let game = active_game_name(state).unwrap_or_default();
            let message = chat_packet(&name, &game, &text)?;
            send(
                state,
                &message,
                GlobalDestination::Broadcast { port_override: 0 },
                false,
            );
        }
    }
    Ok(())
}

pub(super) fn active_game_name(state: &AppState) -> Option<String> {
    if state.match_state.session.host() {
        Some(
            state
                .match_state
                .session
                .network_setup()?
                .local_player()
                .name
                .clone(),
        )
    } else {
        state
            .match_state
            .lan_service
            .as_ref()?
            .admitted_host
            .as_ref()
            .map(|(name, _)| name.clone())
    }
}

pub(super) fn received_choices(
    country: i32,
    color: i32,
    initial: &crate::skirmish_launch::SkirmishLocalSlot,
) -> anyhow::Result<crate::skirmish_launch::SkirmishLocalSlot> {
    let mut slot = initial.clone();
    // Type3 initializes start/team to Random. Subsequent authoritative host
    // options own their selected values (696FF0/697020 at5E060D..5E061A).
    slot.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
    slot.team = crate::skirmish_launch::LaunchTeam::None;
    slot.country_random = country < 0;
    if country >= 0 {
        anyhow::ensure!(
            country <= i32::from(u16::MAX),
            "country outside registry domain"
        );
        slot.country = crate::skirmish_launch::LaunchCountry::from_country_index(country as u32);
    }
    slot.color_random = color < 0;
    if color >= 0 {
        slot.color_index = u8::try_from(color)?;
    }
    Ok(slot)
}

pub(super) fn accept_participant(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: &LanMessage,
    fields: LanParticipantDetails,
) -> anyhow::Result<()> {
    let proof = message
        .session_proof()?
        .ok_or_else(|| anyhow::anyhow!("participant has no VERA admission"))?;
    let name = message.name();
    anyhow::ensure!(
        state
            .match_state
            .lan_service
            .as_ref()
            .ok_or_else(super::closed_visit)?
            .accepts_participant(state.match_state.session.host(), endpoint, &name, &proof),
        "participant is outside the host's confirmed LAN admission"
    );
    anyhow::ensure!(
        !name.eq_ignore_ascii_case(&local_name(state)),
        "local participant reflection"
    );
    //5E050A rejects an existing endpoint before allocating/appending a node.
    if state
        .match_state
        .session
        .network_endpoint_source(endpoint)
        .is_some()
    {
        return Ok(());
    }
    if let Some(game) = active_game_name(state) {
        anyhow::ensure!(
            fields.game_name_hash == crate::net::lan_messages::game_name_hash(&game),
            "different LAN game"
        );
    }
    if let Some(launch) = state.match_state.session.network_setup()
        && launch.humans.iter().any(|human| human.name == name)
    {
        return Ok(());
    }
    let initial = state
        .frontend
        .multiplayer_shell_runtime
        .local_participant()
        .slot;
    let human = crate::skirmish_launch::SkirmishHumanSlot {
        name: name.clone(),
        slot: received_choices(fields.country, fields.color, &initial)?,
        //5E0685 passes this slot to5E8D90 after appending the node.
        raw_slot_index: Some(
            u8::try_from(fields.field_0x53)
                .ok()
                .filter(|row| usize::from(*row) < crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT)
                .ok_or_else(|| anyhow::anyhow!("participant row is outside native slots"))?,
        ),
        role: crate::skirmish_launch::LaunchHumanRole::from_observer(fields.field_0x57 != 0),
    };
    super::shell_network_setup::service_event(
        state,
        super::shell_network_setup::NetworkSetupEvent::ParticipantJoined(human),
    )?;
    state
        .match_state
        .session
        .bind_network_endpoint(&name, endpoint)?;
    state
        .match_state
        .lan_service
        .as_mut()
        .ok_or_else(super::closed_visit)?
        .commit_peer_admission(endpoint, &name, &proof);
    // The host snapshot can precede Type3. Apply it through HostSetup again
    // after this binding so country/start/team cannot remain Type3 defaults.
    options::reapply_received_options(state)?;
    Ok(())
}

fn join_denial(
    state: &AppState,
    endpoint: SocketAddrV4,
    name: &str,
    request: &LanJoinRequest,
    proof: &LanSessionProof,
    now: u32,
) -> Option<u32> {
    let same_peer = state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| {
            launch.humans.iter().enumerate().any(|(index, human)| {
                index != launch.local_player_index
                    && human.name.eq_ignore_ascii_case(name)
                    && state.match_state.session.network_endpoint(index) == Some(endpoint)
            })
        });
    if proof.version != super::shell_network_setup::agreement::version() {
        Some(8)
    } else if !state.match_state.session.host()
        || state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_none_or(|protocol| !protocol.editable())
    {
        Some(5)
    } else if !same_peer
        && state
            .match_state
            .session
            .network_endpoint_source(endpoint)
            .is_some()
    {
        Some(0)
    } else if !same_peer
        && state
            .match_state
            .session
            .network_setup()
            .is_some_and(|launch| {
                launch
                    .humans
                    .iter()
                    .any(|human| human.name.eq_ignore_ascii_case(name))
            })
    {
        Some(0)
    } else if !same_peer
        && state
            .match_state
            .lan_service
            .as_ref()
            .is_some_and(|service| service.keeps_out(endpoint, name, now))
    {
        // VERA-only: the host kicked this player within five minutes.
        Some(9)
    } else if !same_peer
        && state
            .match_state
            .session
            .network_join_row(request.observer_request == 1)
            .is_none()
        // VERA's room gives a newcomer the last AI's seat.
        && state.match_state.session.network_newcomer_ai_seat().is_none()
    {
        Some(1)
    } else if Some(request.compatibility) != compatibility(state) {
        Some(6)
    } else if request.version.maximum < crate::net::lan_messages::LanVersionInterval::RETAIL.minimum
    {
        Some(2)
    } else if request.version.minimum > crate::net::lan_messages::LanVersionInterval::RETAIL.maximum
    {
        Some(3)
    } else {
        None
    }
}

fn send_join_denial(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    reason: u32,
    proof: &LanSessionProof,
) -> anyhow::Result<()> {
    let service = state
        .match_state
        .lan_service
        .as_mut()
        .ok_or_else(super::closed_visit)?;
    let denied_proof = LanSessionProof {
        scope: service.setup_scope().unwrap_or(SessionScope {
            epoch: [0; 16],
            revision: 0,
        }),
        join_nonce: proof.join_nonce,
        admission_nonce: proof.admission_nonce,
        version: super::shell_network_setup::agreement::version(),
        rules_digest: String::new(),
    };
    let denied = LanMessage::join_denied(reason).with_session_proof(&denied_proof)?;
    anyhow::ensure!(
        service.queue_setup(std::iter::once((
            denied.as_bytes().to_vec(),
            direct(endpoint)
        )))?,
        "LAN denial queue is full"
    );
    Ok(())
}

/// Native Type6/7 admission is extended for VERA: an OS challenge does not
/// reserve a player slot. Only the correlated Confirm below reaches the
/// existing ParticipantJoined owner. Delayed old requests can only create
/// bounded proposals; no replay history or second roster is retained.
pub(super) fn offer_join(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: &LanMessage,
    request: LanJoinRequest,
    now: u32,
) -> anyhow::Result<()> {
    let proof = message
        .session_proof()?
        .ok_or_else(|| anyhow::anyhow!("LAN join has no VERA build identity"))?;
    anyhow::ensure!(proof.join_nonce != [0; 16], "LAN join has no request nonce");
    if let Some(reason) = join_denial(state, endpoint, &message.name(), &request, &proof, now) {
        return send_join_denial(state, endpoint, reason, &proof);
    }
    let current = state
        .match_state
        .lan_service
        .as_ref()
        .ok_or_else(super::closed_visit)?
        .peer_admissions
        .get(&endpoint)
        .map(|record| record.identity)
        .filter(|identity| identity.join_nonce == proof.join_nonce);
    if let Some(identity) = current {
        let confirmed = confirmed_join_message(state, endpoint, identity)?;
        anyhow::ensure!(
            state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?
                .queue_setup(std::iter::once((
                    confirmed.as_bytes().to_vec(),
                    direct(endpoint)
                )))?,
            "LAN final acceptance queue is full"
        );
        return Ok(());
    }
    let identity = {
        let service = state
            .match_state
            .lan_service
            .as_mut()
            .ok_or_else(super::closed_visit)?;
        service.ensure_host_scope()?;
        match service.join_offer(endpoint, message, now) {
            Ok(identity) => identity,
            Err(_) => return send_join_denial(state, endpoint, 5, &proof),
        }
    };
    let host_name = local_name(state);
    let acceptance = crate::net::lan_messages::LanAcceptance {
        country: request.country,
        color: request.color,
        assigned_row: 0,
        observer_row: -1,
    };
    let mut offer_proof = state
        .match_state
        .lan_service
        .as_ref()
        .ok_or_else(super::closed_visit)?
        .control_proof()?;
    offer_proof.join_nonce = identity.join_nonce;
    offer_proof.admission_nonce = identity.admission_nonce;
    let offer =
        LanMessage::join_accepted(&host_name, acceptance)?.with_session_proof(&offer_proof)?;
    anyhow::ensure!(
        state
            .match_state
            .lan_service
            .as_mut()
            .ok_or_else(super::closed_visit)?
            .queue_setup(std::iter::once((
                offer.as_bytes().to_vec(),
                direct(endpoint)
            )))?,
        "LAN challenge queue is full"
    );
    Ok(())
}

fn confirmed_join_message(
    state: &AppState,
    endpoint: SocketAddrV4,
    identity: PeerAdmission,
) -> anyhow::Result<LanMessage> {
    let source = state
        .match_state
        .session
        .network_endpoint_source(endpoint)
        .ok_or_else(|| anyhow::anyhow!("confirmed admission has no Session source"))?;
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("admitted setup");
    let human = &launch.humans[source];
    let acceptance = crate::net::lan_messages::LanAcceptance {
        country: human.slot.country_shell_value(human.is_observer()),
        color: human.slot.color_shell_value(),
        assigned_row: i32::from(
            human
                .raw_slot_index
                .ok_or_else(|| anyhow::anyhow!("confirmed source has no row"))?,
        ),
        observer_row: state
            .match_state
            .session
            .network_observer_row()
            .map_or(-1, i32::from),
    };
    let mut proof = state
        .match_state
        .lan_service
        .as_ref()
        .ok_or_else(super::closed_visit)?
        .control_proof()?;
    proof.join_nonce = identity.join_nonce;
    proof.admission_nonce = identity.admission_nonce;
    Ok(
        LanMessage::join_confirmed(&launch.local_player().name, acceptance)?
            .with_session_proof(&proof)?,
    )
}

pub(super) fn confirm_join(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: &LanMessage,
    now: u32,
) -> anyhow::Result<()> {
    let proof = message
        .session_proof()?
        .ok_or_else(|| anyhow::anyhow!("LAN confirmation has no proof"))?;
    if !state
        .match_state
        .lan_service
        .as_ref()
        .is_some_and(|service| service.in_room(&proof))
    {
        return Ok(());
    }
    let duplicate = state
        .match_state
        .lan_service
        .as_ref()
        .ok_or_else(super::closed_visit)?
        .peer_admission_matches(endpoint, &proof);
    let request = state
        .match_state
        .lan_service
        .as_mut()
        .ok_or_else(super::closed_visit)?
        .confirmed_request(endpoint, &proof, now);
    if let Some((name, request)) = request {
        if let Some(reason) = join_denial(state, endpoint, &name, &request, &proof, now) {
            return send_join_denial(state, endpoint, reason, &proof);
        }
        // Reserve the final reply before changing Session membership. UI-side
        // join effects append semantic requests, not transport packets.
        anyhow::ensure!(
            state
                .match_state
                .lan_service
                .as_ref()
                .ok_or_else(super::closed_visit)?
                .pending_setup
                .len()
                < 4096,
            "LAN final admission is backpressured"
        );
        let same_peer = state
            .match_state
            .session
            .network_endpoint_source(endpoint)
            .is_some();
        if !same_peer {
            let initial = state
                .frontend
                .multiplayer_shell_runtime
                .local_participant()
                .slot;
            let slot = received_choices(request.country, request.color, &initial)?;
            // With every seat taken, VERA's room seats the newcomer on the
            // last AI's, which it frees.
            let freed = state.match_state.session.free_network_seat_for_newcomer()?;
            let (row, observer) = match freed {
                Some(row) => (row, false),
                None => state
                    .match_state
                    .session
                    .network_join_row(request.observer_request == 1)
                    .expect("validated row"),
            };
            let mut human = crate::skirmish_launch::SkirmishHumanSlot {
                name: name.clone(),
                raw_slot_index: Some(row),
                slot,
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(observer),
            };
            if !human.slot.color_random
                && state
                    .match_state
                    .session
                    .network_color_in_use(human.slot.color_index)
            {
                human.slot.color_random = true;
            }
            // Host's production admission owns mode-specific country validation.
            if observer {
                human.slot.country_random = true;
                human.slot.color_random = true;
            }
            super::shell_network_setup::service_event(
                state,
                super::shell_network_setup::NetworkSetupEvent::ParticipantJoined(human),
            )?;
            state
                .match_state
                .session
                .bind_network_endpoint(&name, endpoint)?;
        } else {
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("editable admission");
            protocol.republish_setup();
        }
        state
            .match_state
            .lan_service
            .as_mut()
            .ok_or_else(super::closed_visit)?
            .commit_peer_admission(endpoint, &name, &proof);
    } else if !duplicate {
        return Ok(());
    }
    let identity = PeerAdmission {
        join_nonce: proof.join_nonce,
        admission_nonce: proof.admission_nonce,
    };
    let confirmed = confirmed_join_message(state, endpoint, identity)?;
    anyhow::ensure!(
        state
            .match_state
            .lan_service
            .as_mut()
            .ok_or_else(super::closed_visit)?
            .queue_setup(std::iter::once((
                confirmed.as_bytes().to_vec(),
                direct(endpoint)
            )))?,
        "reserved LAN acceptance must fit"
    );
    Ok(())
}

pub(super) fn cancel_join(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: &LanMessage,
) -> anyhow::Result<()> {
    let proof = message
        .session_proof()?
        .ok_or_else(|| anyhow::anyhow!("cancel has no proof"))?;
    if !state.match_state.session.host()
        || state.frontend.network_setup_protocol.is_none()
        || proof.version != super::shell_network_setup::agreement::version()
    {
        return Ok(());
    }
    let editable = state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.editable());
    let service = state
        .match_state
        .lan_service
        .as_mut()
        .ok_or_else(super::closed_visit)?;
    service.ensure_host_scope()?;
    let mut reply_proof = service.control_proof()?;
    reply_proof.join_nonce = proof.join_nonce;
    reply_proof.admission_nonce = proof.admission_nonce;
    let reply =
        LanMessage::named(LanNamedMessage::JoinCanceled, "")?.with_session_proof(&reply_proof)?;
    if !service.queue_setup(std::iter::once((
        reply.as_bytes().to_vec(),
        direct(endpoint),
    )))? {
        return Ok(());
    }
    let remove = editable && service.cancel_peer_admission(endpoint, &proof);
    if remove {
        if let Some(index) = state.match_state.session.network_endpoint_source(endpoint) {
            super::shell_network_setup::service_event(
                state,
                super::shell_network_setup::NetworkSetupEvent::ParticipantLeft { index },
            )?;
        }
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
            protocol.republish_setup();
        }
    }
    Ok(())
}
