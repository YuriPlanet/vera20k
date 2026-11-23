//! The setup room's liveness and requests: setup leases, presence announcements,
//! queued setup requests, host timing and the player-row projection.

use crate::net::global::GlobalDestination;
use crate::net::lan_messages::LanMessage;
use crate::net::lan_messages::LanNamedMessage;
use crate::net::lan_messages::LanPayload;
use std::net::SocketAddrV4;

use super::{
    AppState, DueBroadcasts, SessionMode, active_game_name, chat_packet, direct, endpoint_id,
    local_control_proof, local_name, options, send, verify_setup_agreement,
};

pub(super) fn setup_liveness_active(state: &AppState) -> bool {
    state.match_state.session_mode() == SessionMode::Lan
        && state.match_state.live_network.is_none()
        && state.frontend.shell_exit.is_none()
        && state.frontend.network_setup_protocol.is_some()
        && state
            .match_state
            .lan_service
            .as_ref()
            .is_some_and(|service| service.departure.is_none())
}

pub(super) fn poll_setup_leases(state: &mut AppState, now: u32) {
    if !setup_liveness_active(state)
        || !state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| protocol.editable())
    {
        return;
    }
    // GO/loading keep their own timeout. Continue sending probes while GO's
    // final-options ACK barrier is draining, so still-editable guests do not
    // mistake a busy host for a crash before GO itself can be admitted.
    loop {
        // A HostLeft or ParticipantLeft event below can close the visit.
        let Some(service) = state.match_state.lan_service.as_mut() else {
            break;
        };
        let Some((index, endpoint)) = service.expired_setup_peer(&state.match_state.session, now)
        else {
            break;
        };
        let host_left = service.retire_setup_peer(endpoint);
        let event = if host_left {
            super::shell_network_setup::lock_lan_departure(state);
            super::shell_network_setup::NetworkSetupEvent::HostLeft
        } else {
            super::shell_network_setup::NetworkSetupEvent::ParticipantLeft { index }
        };
        // Native5E10F2/5E1114 and HostLeft share their ordinary teardown/UI
        // effects. Only the authenticated elapsed-lease trigger is VERA-only.
        if let Err(error) = super::shell_network_setup::service_event(state, event) {
            log::error!("LAN setup timeout cleanup failed: {error:#}");
            break;
        }
        if host_left {
            break;
        }
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
            protocol.republish_setup();
        }
    }
}

pub(super) fn announce(state: &mut AppState, now: u32) {
    if state.match_state.live_network.is_some() {
        return;
    }
    let host_setup =
        state.match_state.session.host() && state.frontend.network_setup_protocol.is_some();
    let in_setup = state.frontend.network_setup_protocol.is_some();
    let lease_active = setup_liveness_active(state);
    let Some(service) = state.match_state.lan_service.as_mut() else {
        return;
    };
    let DueBroadcasts {
        discovery,
        query,
        presence,
    } = service.due_broadcasts(now, host_setup);
    let name = local_name(state);
    let broadcast = GlobalDestination::Broadcast { port_override: 0 };
    if discovery && let Ok(message) = LanMessage::named(LanNamedMessage::Discovery, &name) {
        send(state, &message, broadcast, false);
    }
    if query {
        let game_name = active_game_name(state).or_else(|| {
            let service = state.match_state.lan_service.as_ref()?;
            service
                .selected_game
                .and_then(|id| service.game(id))
                .map(|game| game.name.clone())
        });
        if let Some(game_name) = game_name
            && let Ok(message) = LanMessage::named(LanNamedMessage::GameQuery, &game_name)
        {
            send(state, &message, broadcast, false);
        }
    }
    if presence && lease_active {
        if let Some(service) = state.match_state.lan_service.as_mut()
            && let Err(error) = service.send_setup_keepalives(&state.match_state.session)
        {
            log::debug!("LAN setup keepalive unavailable: {error:#}");
        }
    } else if presence && !in_setup {
        let human = state
            .match_state
            .session
            .network_setup()
            .map(|launch| launch.local_player().clone())
            .unwrap_or_else(|| state.frontend.multiplayer_shell_runtime.local_participant());
        if let Ok(message) = LanMessage::presence(
            &name,
            human.slot.country_shell_value(human.is_observer()),
            human.slot.color_shell_value(),
        ) {
            send(state, &message, broadcast, false);
        }
    }
}

pub(super) fn remote_sources(state: &AppState) -> Vec<usize> {
    state
        .match_state
        .session
        .network_setup()
        .map_or_else(Vec::new, |launch| {
            (0..launch.humans.len())
                .filter(|source| *source != launch.local_player_index)
                .collect()
        })
}

pub(super) fn drain_setup_requests(state: &mut AppState, now: u32) {
    let incoming = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .map(|protocol| protocol.take_service_requests())
        .unwrap_or_default();
    let Some(service) = state.match_state.lan_service.as_mut() else {
        return;
    };
    service.deferred_setup.extend(incoming);
    let mut requests = std::mem::take(&mut service.deferred_setup);
    let mut deferred = std::collections::VecDeque::new();
    while let Some(request) = requests.pop_front() {
        if state
            .match_state
            .lan_service
            .as_ref()
            .is_some_and(|service| service.departure.is_some())
        {
            break;
        }
        // A guest reads C0 only under the scope the options before it bring.
        if !deferred.is_empty()
            && matches!(
                request,
                super::shell_network_setup::NetworkSetupRequest::PublishCooperativeCampaign(_)
            )
        {
            deferred.push_back(request);
            continue;
        }
        match setup_request(state, &request, now) {
            Ok(true) => {}
            Ok(false) => deferred.push_back(request),
            Err(error) => {
                log::warn!("LAN setup request failed: {error:#}");
                // A direct spawn has no dialog to retry from.
                if super::shell_network_setup::external(state) {
                    super::App::fail_external_launch(
                        state,
                        &crate::i18n::tr("VERA:StartRefusedSetup"),
                    );
                    return;
                }
            }
        }
    }
    if let Some(service) = state.match_state.lan_service.as_mut() {
        if service.departure.is_none() {
            service.deferred_setup.extend(deferred);
        }
    }
}

fn setup_request(
    state: &mut AppState,
    request: &super::shell_network_setup::NetworkSetupRequest,
    now: u32,
) -> anyhow::Result<bool> {
    use super::shell_network_setup::NetworkSetupRequest;
    let message = match request {
        NetworkSetupRequest::Kick { name, keep_out } => {
            return state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?
                .kick_peer(&state.match_state.session, name, *keep_out, now);
        }
        NetworkSetupRequest::PublishOptions => return options::publish_options(state),
        NetworkSetupRequest::PublishRoster => return options::publish_roster(state),
        NetworkSetupRequest::Go {
            agreed_seed,
            max_ahead_frames,
        } => {
            if !state
                .frontend
                .network_setup_protocol
                .as_ref()
                .is_some_and(|protocol| protocol.lan_go_send_pending())
            {
                return Ok(true);
            }
            let recipients = remote_sources(state).len();
            if !state
                .match_state
                .lan_service
                .as_ref()
                .is_some_and(|service| {
                    service.setup_agreement.is_some()
                        && service.setup_maps_verified(
                            super::shell_network_setup::campaign_room::local_map_verified(
                                &state.match_state.session,
                            ),
                        )
                        && service.setup_roster_complete(&state.match_state.session)
                        && service.can_queue_go(recipients)
                })
            {
                return Ok(false);
            }
            verify_setup_agreement(state)?;
            anyhow::ensure!(
                state.match_state.session.network_start_seed() == Some(*agreed_seed),
                "GO seed differs from admitted options"
            );
            let destinations = remote_sources(state)
                .into_iter()
                .map(|source| {
                    state
                        .match_state
                        .session
                        .network_endpoint(source)
                        .map(direct)
                        .ok_or_else(|| anyhow::anyhow!("GO recipient has no admitted endpoint"))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            if !state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?
                .queue_go(&LanMessage::go(*max_ahead_frames), &destinations)?
            {
                return Ok(false);
            }
            let now = super::match_runtime::frame_pacer::frame_bucket(
                super::match_runtime::frame_pacer::wall_clock_ms(),
            );
            super::shell_network_setup::mark_lan_go_packets_queued(state, now)?;
            return Ok(true);
        }
        NetworkSetupRequest::RequestAcceptance => LanMessage::start_prompt(&local_name(state))?,
        NetworkSetupRequest::Chat(text) => chat_packet(
            &local_name(state),
            &active_game_name(state).unwrap_or_default(),
            text,
        )?,
        NetworkSetupRequest::PlayerChoices { slot, observer, .. } => {
            let message = LanMessage::player_choices(
                &local_name(state),
                format!(
                    "R{},{},{},{}",
                    slot.country_shell_value(*observer),
                    slot.color_shell_value(),
                    slot.start_position.shell_value(),
                    slot.team.shell_value()
                )
                .as_bytes(),
            )?
            .with_session_proof(
                &state
                    .match_state
                    .lan_service
                    .as_ref()
                    .ok_or_else(super::closed_visit)?
                    .control_proof()?,
            )?;
            let endpoint = state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.admitted_host.as_ref())
                .map(|(_, endpoint)| *endpoint)
                .ok_or_else(|| anyhow::anyhow!("player choices have no admitted host"))?;
            return state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?
                .queue_setup(std::iter::once((
                    message.as_bytes().to_vec(),
                    direct(endpoint),
                )));
        }
        NetworkSetupRequest::Ready(ready) => {
            let accepted = state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| {
                    state
                        .match_state
                        .session
                        .network_player_ready(launch.local_player_index)
                });
            if *ready && !accepted {
                // A late peer's retained full-options application can retract
                // local acceptance while an older A1 waits in this FIFO.
                return Ok(true);
            }
            short_options(state, if *ready { "A1" } else { "A0" })?
        }
        NetworkSetupRequest::MapStatus(status) => short_options(state, &format!("K{status}"))?,
        // +0x60's C0 (0x0049D680 through 0x005E3720) to every guest.
        NetworkSetupRequest::PublishCooperativeCampaign(text) => {
            if remote_sources(state).is_empty() {
                return Ok(true);
            }
            short_options(state, text)?
        }
        NetworkSetupRequest::PrepareMapForHandoff { .. } => {
            if !state
                .frontend
                .network_setup_protocol
                .as_ref()
                .is_some_and(|protocol| protocol.lan_go_send_pending())
            {
                return Ok(true);
            }
            if !state
                .match_state
                .lan_service
                .as_ref()
                .is_some_and(|service| {
                    service.setup_maps_verified(
                        super::shell_network_setup::campaign_room::local_map_verified(
                            &state.match_state.session,
                        ),
                    ) && service.setup_roster_complete(&state.match_state.session)
                        && service.can_queue_go(1)
                })
            {
                return Ok(false);
            }
            let endpoint = state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|s| s.admitted_host.as_ref())
                .map(|(_, endpoint)| *endpoint)
                .ok_or_else(|| anyhow::anyhow!("map preparation has no admitted host"))?;
            state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?
                .apply_start_timing(false, false);
            verify_setup_agreement(state)?;
            super::shell_network_setup::prepare_lan_guest_handoff(state)?;
            let admitted = state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(super::closed_visit)?
                .queue_go(&LanMessage::ready_to_go(), &[direct(endpoint)])?;
            assert!(admitted, "guest GO preflight must retain its reserved slot");
            let now = super::match_runtime::frame_pacer::frame_bucket(
                super::match_runtime::frame_pacer::wall_clock_ms(),
            );
            super::shell_network_setup::mark_lan_go_packets_queued(state, now)?;
            return Ok(true);
        }
        // None of these reaches a LAN service. The rest are WOL's requests, and
        // LAN Back departs through `lan_service::request_setup_back`.
        NetworkSetupRequest::Leave
        | NetworkSetupRequest::WolChat(_)
        | NetworkSetupRequest::WolEmote(_)
        | NetworkSetupRequest::StartGame { .. }
        | NetworkSetupRequest::Advertise
        | NetworkSetupRequest::Balance
        | NetworkSetupRequest::ObserverSlot { .. } => return Ok(false),
    };
    let require_rules = matches!(
        request,
        NetworkSetupRequest::Ready(true) | NetworkSetupRequest::MapStatus(1)
    );
    if require_rules
        && state
            .match_state
            .lan_service
            .as_ref()
            .is_none_or(|service| {
                service.setup_agreement.is_none()
                    || !service.setup_maps_verified(
                        super::shell_network_setup::campaign_room::local_map_verified(
                            &state.match_state.session,
                        ),
                    )
                    || !service.setup_roster_complete(&state.match_state.session)
            })
    {
        return Ok(false);
    }
    let proof = if require_rules {
        local_control_proof(state)?
    } else {
        state
            .match_state
            .lan_service
            .as_ref()
            .ok_or_else(super::closed_visit)?
            .control_proof()?
    };
    let message = if matches!(request, NetworkSetupRequest::Chat(_)) {
        message
    } else {
        message.with_session_proof(&proof)?
    };
    let destinations = remote_sources(state)
        .into_iter()
        .filter_map(|source| state.match_state.session.network_endpoint(source))
        .map(direct)
        .collect::<Vec<_>>();
    state
        .match_state
        .lan_service
        .as_mut()
        .ok_or_else(super::closed_visit)?
        .queue_setup(
            destinations
                .into_iter()
                .map(|destination| (message.as_bytes().to_vec(), destination)),
        )
}

/// The name and discovery ID of the LAN host that admitted this guest.
pub(crate) fn admitted_host_room(state: &AppState) -> Option<(String, u64)> {
    let service = state.match_state.lan_service.as_ref()?;
    let (name, endpoint) = service.admitted_host.as_ref()?;
    Some((name.clone(), endpoint_id(*endpoint)))
}

pub(crate) fn force_presence(state: &mut AppState) {
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.force_presence = true;
    }
}

pub(super) fn ensure_host_source(state: &AppState, endpoint: SocketAddrV4) -> anyhow::Result<()> {
    anyhow::ensure!(
        !state.match_state.session.host(),
        "host cannot accept guest-authored host packet"
    );
    anyhow::ensure!(
        state
            .match_state
            .lan_service
            .as_ref()
            .and_then(|service| service.admitted_host.as_ref())
            .is_some_and(|(_, host)| *host == endpoint),
        "packet is not from admitted host"
    );
    Ok(())
}

fn short_options(state: &AppState, text: &str) -> anyhow::Result<LanMessage> {
    let name = local_name(state);
    let color = state
        .match_state
        .session
        .network_setup()
        .map_or(-2, |launch| launch.local_player().slot.color_shell_value());
    let game = active_game_name(state)
        .ok_or_else(|| anyhow::anyhow!("options without an admitted game"))?;
    Ok(LanMessage::options(
        &name,
        text.as_bytes(),
        color as u32,
        crate::net::lan_messages::game_name_hash(&game),
    )?)
}

pub(crate) fn prepare_host_timing(state: &mut AppState) -> anyhow::Result<()> {
    if let Some(transport) = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.transport.as_ref())
    {
        let response = transport.global().mean_response_time();
        state
            .match_state
            .session
            .prepare_network_timing(2, || response)?;
    }
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.apply_start_timing(true, false);
    }
    Ok(())
}

pub(super) fn observe_participant(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: LanMessage,
    now: u32,
) {
    if message.name().eq_ignore_ascii_case(&local_name(state)) {
        return;
    }
    let Some(service) = state.match_state.lan_service.as_mut() else {
        return;
    };
    service.record_observed_participant(endpoint, message, now);
    project_players(state);
}

pub(super) fn project_players(state: &mut AppState) {
    let Some(service) = state.match_state.lan_service.as_ref() else {
        return;
    };
    let game = service.selected_game.and_then(|id| service.game(id));
    let game_name_hash = game.map(|game| crate::net::lan_messages::game_name_hash(&game.name));
    let selected = service.selected_game;
    let mut players = Vec::new();
    if game.is_none() {
        players.push(super::shell_lan::local_lobby_row(&local_name(state)));
    }
    for peer in &service.observed_participants {
        let color = match (game_name_hash, peer.message.payload()) {
            (Some(hash), LanPayload::ParticipantDetails(fields)) => {
                if fields.game_name_hash != hash {
                    continue;
                }
                fields.color
            }
            (None, LanPayload::Presence { color, .. }) => color,
            _ => continue,
        };
        players.push(crate::ui::lan_shell::LanPlayerRow {
            name: peer.message.name(),
            color: u32::try_from(color).ok(),
            side_icon: None,
            host: game.is_some_and(|game| game.endpoint == peer.endpoint),
            accepted: false,
        });
    }
    super::shell_lan::service_event(
        state,
        super::shell_lan::LanServiceEvent::Players {
            game_id: selected,
            players,
        },
    );
}
