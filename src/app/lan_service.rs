//! The LAN visit's physical service. Discovery records are projections; accepted
//! participant addresses belong to MatchSession. One socket pair moves from
//! this service to LiveCommandSession at the native697B70 connection boundary.
//!
//! Retail LAN opens IPX at52E3D2..52E41F. This owner's IPv4 broadcast is an
//! explicit VERA transport adaptation; application messages remain the native
//! 455-byte protocol from5DF9E0/5DFE00/5E00B0.

use std::net::{Ipv4Addr, SocketAddrV4};

use super::AppState;
use super::match_runtime::session::SessionMode;
use crate::net::global::{GlobalDestination, GlobalMessage};
use crate::net::lan_messages::{LanCompatibility, LanMessage, LanMessageKind, LanNamedMessage};
use crate::net::reliable::{Delivery, LanConnectionConfig, LanConnections, RetryTiming};
use crate::net::udp::{DEFAULT_LAN_PORT, LanUdpTransport, UdpBindConfig};

const LOBBY_RETRY_TIMING: RetryTiming = RetryTiming {
    retry_delta: 30,
    max_retries: u32::MAX,
    timeout: 600,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum KickedExit {
    Draining,
    Leaving,
}

#[derive(Debug, Clone)]
struct DiscoveryRecord {
    endpoint: SocketAddrV4,
    name: String,
    open: bool,
    last_seen: u32,
}

#[derive(Debug, Clone)]
struct ObservedParticipant {
    endpoint: SocketAddrV4,
    message: LanMessage,
    last_seen: u32,
}

pub(crate) struct LanService {
    transport: Option<LanUdpTransport>,
    games: Vec<DiscoveryRecord>,
    /// Radio observations for BB only; never admitted as gameplay endpoints.
    observed_participants: Vec<ObservedParticipant>,
    selected_game: Option<u64>,
    pending_host: Option<SocketAddrV4>,
    admitted_host: Option<(String, SocketAddrV4)>,
    kicked_exit: Option<KickedExit>,
    deferred_match:
        std::collections::VecDeque<super::match_runtime::session::NetworkLoadingRequest>,
    /// Native5DF9E0's independent discovery/query/presence timers.
    discovery_timer: super::match_runtime::session::WallBucketTimer,
    query_timer: super::match_runtime::session::WallBucketTimer,
    presence_timer: super::match_runtime::session::WallBucketTimer,
    force_presence: bool,
    force_query: bool,
    deferred_setup: std::collections::VecDeque<super::shell_network_setup::NetworkSetupRequest>,
}

impl LanService {
    pub(crate) fn open(_now: u32) -> std::io::Result<Self> {
        // IPv4 adaptation: limited broadcast on the OS-selected interface.
        // Explicit interface/target selection remains a separate transport
        // option; never reinterpret retail IPX DestNet/NetCard as IPv4 values.
        Self::open_on(
            UdpBindConfig::default(),
            &[SocketAddrV4::new(Ipv4Addr::BROADCAST, DEFAULT_LAN_PORT)],
        )
    }

    /// The same physical service with explicit interfaces for two independent
    /// diagnostic clients. This changes socket routing only, never admission,
    /// protocol timing, queues or gameplay state.
    pub(crate) fn open_on(
        bind: UdpBindConfig,
        discovery_targets: &[SocketAddrV4],
    ) -> std::io::Result<Self> {
        let mut transport = LanUdpTransport::bind(
            bind,
            LanConnections::new(LanConnectionConfig::default(), LOBBY_RETRY_TIMING),
        )?;
        transport.set_broadcast_targets(discovery_targets)?;
        Ok(Self {
            transport: Some(transport),
            games: Vec::new(),
            observed_participants: Vec::new(),
            selected_game: None,
            pending_host: None,
            admitted_host: None,
            kicked_exit: None,
            deferred_match: Default::default(),
            discovery_timer: Default::default(),
            query_timer: Default::default(),
            presence_timer: Default::default(),
            force_presence: true,
            force_query: false,
            deferred_setup: Default::default(),
        })
    }

    fn send(&mut self, payload: &[u8], destination: GlobalDestination, reliable: bool) -> bool {
        self.transport.as_mut().is_some_and(|transport| {
            transport.global_mut().send(
                payload,
                destination,
                if reliable {
                    Delivery::Reliable
                } else {
                    Delivery::Unreliable
                },
                0,
            )
        })
    }

    fn game(&self, id: u64) -> Option<&DiscoveryRecord> {
        self.games
            .iter()
            .find(|record| endpoint_id(record.endpoint) == id)
    }

    fn pop_lobby_received(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
    ) -> Option<GlobalMessage> {
        // Native5E00B0 changes BB to BD synchronously on Type7. Our slide is
        // asynchronous: retain every subsequent packet in the existing global
        // queue until MatchSession has accepted BD, including options/GO.
        // Frozen loading still owns that setup and must keep receiving progress.
        if self.kicked_exit.is_some()
            || (self.admitted_host.is_some() && session.network_setup().is_none())
        {
            return None;
        }
        self.transport.as_mut()?.pop_global_received()
    }

    fn kick_peer(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
        name: &str,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(session.host(), "only the LAN host may kick a participant");
        let launch = session
            .network_setup()
            .ok_or_else(|| anyhow::anyhow!("kick requires an admitted setup"))?;
        let source = launch
            .humans
            .iter()
            .position(|human| human.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow::anyhow!("kick target is not admitted"))?;
        anyhow::ensure!(source != launch.local_player_index, "cannot kick the host");
        let endpoint = session
            .network_endpoint(source)
            .ok_or_else(|| anyhow::anyhow!("kick target has no admitted endpoint"))?;
        //5EC9F0 ->5DB600: reliable Type8/reason4 to the old node. Membership
        // remains until its Type10 reply, even after its display row is unbound.
        self.send(
            LanMessage::join_denied(4).as_bytes(),
            direct(endpoint),
            true,
        );
        Ok(())
    }

    fn queue_departure(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
        now: u32,
    ) -> anyhow::Result<()> {
        let launch = session
            .network_setup()
            .ok_or_else(|| anyhow::anyhow!("departure requires an admitted setup"))?;
        let message = LanMessage::named(LanNamedMessage::Departure, &launch.local_player().name)?;
        //5E09CD..5E0A97: service each direct reliable Type10, then broadcast
        //unreliably. The absent CDEF/router destination is a separate residual.
        for source in 0..launch.humans.len() {
            if source != launch.local_player_index
                && let Some(endpoint) = session.network_endpoint(source)
            {
                self.send(message.as_bytes(), direct(endpoint), true);
                if let Some(transport) = self.transport.as_mut() {
                    transport.service(now);
                }
            }
        }
        self.send(message.as_bytes(), GlobalDestination::BROADCAST, false);
        Ok(())
    }

    fn begin_kicked_exit(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
        source: SocketAddrV4,
        now: u32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !session.host()
                && self
                    .admitted_host
                    .as_ref()
                    .is_some_and(|(_, host)| *host == source),
            "denial is not from the admitted LAN host"
        );
        self.queue_departure(session, now)?;
        self.kicked_exit = Some(KickedExit::Draining);
        Ok(())
    }

    fn kicked_exit_ready(&mut self, service_ok: bool) -> bool {
        //5E0A97..5E0AC6 drains while queued and service succeeds. Freeze app
        //receive/announcements until the existing shell owner completes exit.
        if self.kicked_exit != Some(KickedExit::Draining)
            || (service_ok
                && self
                    .transport
                    .as_ref()
                    .is_some_and(|t| t.global().queued_send_count() > 0))
        {
            return false;
        }
        self.kicked_exit = Some(KickedExit::Leaving);
        true
    }

    fn apply_start_timing(&mut self, host: bool, completed: bool) {
        if let Some(transport) = self.transport.as_mut() {
            let timing =
                RetryTiming::lan_start(host, completed, || transport.global().mean_response_time());
            transport.set_retry_timing(timing, true);
            if !host && completed {
                //5DD9FD ->540D10 overrides only the primary-global channel.
                transport
                    .global_mut()
                    .set_timing(RetryTiming::LAN_GUEST_COMPLETED_GLOBAL);
            }
        }
    }

    /// BC/BD Back stays inside5DC350's lobby visit (5DC93E..5DC985).
    /// Admission and pending app work end; global transport history survives.
    fn clear_setup(&mut self) {
        self.selected_game = None;
        self.pending_host = None;
        self.admitted_host = None;
        self.kicked_exit = None;
        self.deferred_setup.clear();
        self.deferred_match.clear();
        self.force_query = false;
        self.force_presence = true;
    }

    fn restart_visit(&mut self) {
        self.clear_setup();
        self.games.clear();
        self.observed_participants.clear();
        self.discovery_timer = Default::default();
        self.query_timer = Default::default();
        self.presence_timer = Default::default();
        if let Some(transport) = self.transport.as_mut() {
            transport.reset_session();
            transport.set_retry_timing(LOBBY_RETRY_TIMING, true);
        }
    }
}

pub(crate) fn leave_setup(state: &mut AppState) {
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.clear_setup();
    }
}

/// A new PrepareSession visit reaches5DB680 ->540A80 before5DC350.
/// Reuse the portable socket pair, with freshly initialized channel state.
pub(crate) fn restart_lobby_visit(state: &mut AppState) {
    return_transport(state);
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.restart_visit();
    }
}

fn endpoint_id(endpoint: SocketAddrV4) -> u64 {
    (u64::from(u32::from(*endpoint.ip())) << 16) | u64::from(endpoint.port())
}

fn compatibility(state: &AppState) -> Option<LanCompatibility> {
    let hashes = state.process_assets.native_ini_hashes()?;
    Some(LanCompatibility {
        field_0x43: hashes.rules,
        field_0x47: hashes.ai,
        field_0x4b: hashes.art,
        field_0x4f: crate::ui::wol_lobby::EX_INFO_VERSION as u32,
    })
}

fn local_name(state: &AppState) -> String {
    state
        .match_state
        .session
        .network_setup()
        .map(|launch| launch.local_player().name.clone())
        .unwrap_or_else(|| {
            state
                .frontend
                .multiplayer_shell_runtime
                .local_participant()
                .name
        })
}

fn send(
    state: &mut AppState,
    message: &LanMessage,
    destination: GlobalDestination,
    reliable: bool,
) -> bool {
    if let Some(live) = state.match_state.live_network.as_mut() {
        live.send_global(message.as_bytes(), reliable, destination)
    } else {
        state
            .match_state
            .lan_service
            .as_mut()
            .is_some_and(|service| service.send(message.as_bytes(), destination, reliable))
    }
}

fn direct(endpoint: SocketAddrV4) -> GlobalDestination {
    GlobalDestination::Direct {
        endpoint,
        port_override: 0,
    }
}

pub(crate) fn open(state: &mut AppState) {
    // Deterministic hidden-window capture drivers own explicit service events.
    // Their existing request/response transcript must not race real sockets.
    if state.platform.capture_client_size.is_some() {
        return;
    }
    if state.match_state.lan_service.is_some() {
        return;
    }
    let now = super::match_runtime::frame_pacer::frame_bucket(
        super::match_runtime::frame_pacer::wall_clock_ms(),
    );
    match LanService::open(now) {
        Ok(service) => state.match_state.lan_service = Some(service),
        Err(error) => {
            log::error!("LAN socket initialization failed: {error}");
            super::shell_lan::report_error(state, "TXT_UNABLE_TO_CONNECT");
        }
    }
}

/// Network_ServiceLoop48D080 runs across shell, loading, modal and gameplay
/// waits. Its private receive queue is consumed only by the frame boundary.
pub(crate) fn poll(state: &mut AppState, now_ms: u64) {
    if state
        .match_state
        .session
        .network_return_context()
        .map(|context| context.session_mode())
        != Some(SessionMode::Lan)
    {
        return;
    }
    let now = super::match_runtime::frame_pacer::frame_bucket(now_ms);
    if state.match_state.live_network.is_none()
        && state
            .match_state
            .lan_service
            .as_ref()
            .and_then(|service| service.transport.as_ref())
            .is_none()
    {
        return;
    }
    let service_ok = service_transport(state, now);
    if advance_kicked_exit(state, service_ok) {
        return;
    }
    advance_go(state, now, service_ok);
    loop {
        if super::shell_network_setup::lan_go_receive_mode(state)
            == super::shell_network_setup::LanGoReceiveMode::Defer
        {
            break;
        }
        let incoming = if let Some(live) = state.match_state.live_network.as_mut() {
            live.pop_global_received()
        } else {
            state
                .match_state
                .lan_service
                .as_mut()
                .and_then(|service| service.pop_lobby_received(&state.match_state.session))
        };
        let Some(incoming) = incoming else {
            break;
        };
        let mode = super::shell_network_setup::lan_go_receive_mode(state);
        let accepted_kind = mode != super::shell_network_setup::LanGoReceiveMode::RepliesOnly
            || matches!(incoming.payload.first(), Some(0x13 | 0x10));
        if accepted_kind && let Err(error) = receive(state, incoming, now) {
            log::debug!("Ignored LAN application packet: {error:#}");
        }
        //53FF10 marks only the head consumed;541820/5400D0 retires it.
        let service_ok = service_transport(state, now);
        if advance_kicked_exit(state, service_ok) {
            return;
        }
        advance_go(state, now, service_ok);
    }
    drain_requests(state, now);
    announce(state, now);
    drain_setup_requests(state);
    drain_match_requests(state);
    advance_go(state, now, true);
}

fn receive(state: &mut AppState, incoming: GlobalMessage, now: u32) -> anyhow::Result<()> {
    anyhow::ensure!(incoming.product_id == 0xaa04, "different LAN product");
    if incoming.payload.first() == Some(&0x0f) {
        let progress =
            crate::net::lan_messages::ingame::LoadingProgressMessage::decode(&incoming.payload)?;
        //48DA6E/53EF30 compare only the source host; the payload name
        //disambiguates native nodes. The UDP port is not part of this test.
        let source = state
            .match_state
            .session
            .network_setup()
            .and_then(|launch| {
                launch
                    .humans
                    .iter()
                    .enumerate()
                    .find_map(|(source, human)| {
                        (human.name.eq_ignore_ascii_case(&progress.name())
                            && state
                                .match_state
                                .session
                                .network_endpoint(source)
                                .is_some_and(|endpoint| endpoint.ip() == incoming.source.ip()))
                        .then_some(source)
                    })
            });
        if let Some(source) = source {
            super::loading::pump::accept_network_loading_progress(
                state,
                source,
                progress.percent(),
            );
        }
        return Ok(());
    }
    let message = LanMessage::decode(&incoming.payload)?;
    if state.match_state.live_network.is_some() {
        if message.kind() == LanMessageKind::Departure {
            //ProcessGlobalPackets48D81A uses the active private endpoint;
            //the packet name is irrelevant. This is not a synchronized EXIT.
            let matched = &mut state.match_state;
            let destroyed = if let (Some(live), Some(runtime)) =
                (matched.live_network.as_mut(), matched.sim_runtime.as_mut())
            {
                super::match_runtime::network_match::receive_departure(
                    &mut matched.session,
                    &mut runtime.simulation,
                    live,
                    &mut matched.network_wait,
                    incoming.source,
                )
            } else {
                None
            };
            if let Some(destroyed) = destroyed {
                super::match_runtime::network_match::post_connection_line(state, destroyed);
            }
            return Ok(());
        }
        if let Some(chat) = message.in_game_chat_fields() {
            super::input::chat::receive_packet(
                state,
                &message.name(),
                chat.color as i32,
                &chat.text(),
            );
            return Ok(());
        }
        if let Some(taunt) = message.in_game_taunt_fields() {
            super::input::taunt::receive(state, taunt.byte);
            return Ok(());
        }
        if let Some(runtime) = &state.match_state.sim_runtime
            && let Some(event) = super::match_runtime::network_match::decode_kick_proposal(
                &state.match_state.session,
                &runtime.simulation,
                &incoming.payload,
            )
        {
            super::match_runtime::network_match::service_event(state, event)?;
        }
        return Ok(());
    }
    if let Some(chat) = message.in_game_chat_fields() {
        if let Some(game) = active_game_name(state) {
            anyhow::ensure!(
                chat.game_name_hash == crate::net::lan_messages::game_name_hash(&game),
                "chat belongs to a different game"
            );
        }
        let mut text: Vec<u16> = format!("{}: ", message.name()).encode_utf16().collect();
        text.extend(chat.text_units());
        super::shell_lan::append_chat(
            state,
            crate::ui::lan_shell::LanChatLine {
                text,
                color: chat.color,
            },
        );
        let sound = state
            .rules()
            .and_then(|rules| rules.general.incoming_message_sound())
            .map(str::to_owned);
        super::App::play_shell_ui_sound_by_id(state, sound.as_deref());
        return Ok(());
    }
    match message.kind() {
        LanMessageKind::Options => {
            if state.frontend.network_setup_protocol.is_some() {
                let text = message.options_fields().expect("options").text;
                if matches!(text.first(), Some(b'A' | b'K')) {
                    let source = state
                        .match_state
                        .session
                        .network_endpoint_source(incoming.source)
                        .ok_or_else(|| {
                            anyhow::anyhow!("options sender has no admitted endpoint")
                        })?;
                    let value = crate::rules::ini_value::crt_atoi(
                        &crate::util::native_string::widen_bytes(&text[1..]),
                    );
                    let event = if text[0] == b'A' {
                        super::shell_network_setup::NetworkSetupEvent::PlayerReady {
                            index: source,
                            accepted: value != 0,
                        }
                    } else {
                        super::shell_network_setup::NetworkSetupEvent::MapStatus {
                            index: source,
                            status: u8::try_from(value)?,
                        }
                    };
                    super::shell_network_setup::service_event(state, event)?;
                } else {
                    options::receive_options(state, incoming.source, &message)?;
                }
            }
        }
        LanMessageKind::Roster => {
            if state.frontend.network_setup_protocol.is_some() {
                options::receive_roster(state, incoming.source, &message)?;
            }
        }
        LanMessageKind::PlayerChoices => {
            if state.frontend.network_setup_protocol.is_some() {
                let text = message.player_choices_text().expect("player choices");
                anyhow::ensure!(text.first() == Some(&b'R'), "unknown player choices text");
                let text = crate::util::native_string::widen_bytes(&text[1..]);
                let values: Vec<_> = crate::rules::ini_value::strtok(&text, &[','])
                    .map(crate::rules::ini_value::crt_atoi)
                    .collect();
                anyhow::ensure!(values.len() >= 4, "incomplete player choices");
                let name = message.name();
                let launch = state
                    .match_state
                    .session
                    .network_setup()
                    .ok_or_else(|| anyhow::anyhow!("no setup"))?;
                let source = launch
                    .humans
                    .iter()
                    .position(|human| human.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| anyhow::anyhow!("player choices sender is not admitted"))?;
                let mut choices =
                    received_choices(values[0], values[1], &launch.humans[source].slot)?;
                choices.start_position = if values[2] < 0 {
                    crate::skirmish_launch::LaunchStartPosition::Auto
                } else {
                    crate::skirmish_launch::LaunchStartPosition::Position(u8::try_from(values[2])?)
                };
                choices.team = crate::skirmish_launch::LaunchTeam::from_shell_value(values[3]);
                let change = state
                    .match_state
                    .session
                    .accept_lan_peer_choices(source, choices)?;
                if change.options {
                    let message = options::encode_options(state)?;
                    send_sources(state, message.as_bytes(), remote_sources(state));
                }
            }
        }
        LanMessageKind::Unknown(0x10) => {
            if let Some(source_index) = state
                .match_state
                .session
                .network_endpoint_source(incoming.source)
                && state.frontend.network_setup_protocol.is_some()
            {
                super::shell_network_setup::service_event(
                    state,
                    super::shell_network_setup::NetworkSetupEvent::GoMapRequested { source_index },
                )?;
            }
        }
        LanMessageKind::ReadyToGo => {
            if let Some(source_index) = state
                .match_state
                .session
                .network_endpoint_source(incoming.source)
                && state.frontend.network_setup_protocol.is_some()
            {
                super::shell_network_setup::service_event(
                    state,
                    super::shell_network_setup::NetworkSetupEvent::GoAcknowledged { source_index },
                )?;
            }
        }
        LanMessageKind::StartPrompt => {
            if state.frontend.network_setup_protocol.is_some() {
                ensure_host_source(state, incoming.source)?;
                super::shell_network_setup::service_event(
                    state,
                    super::shell_network_setup::NetworkSetupEvent::StartPrompt,
                )?;
            }
        }
        LanMessageKind::Go => {
            if state.frontend.network_setup_protocol.is_some() {
                ensure_host_source(state, incoming.source)?;
                let seed = state
                    .match_state
                    .session
                    .network_start_seed()
                    .ok_or_else(|| anyhow::anyhow!("GO before host options"))?;
                super::shell_network_setup::service_event(
                    state,
                    super::shell_network_setup::NetworkSetupEvent::HostGo {
                        agreed_seed: seed,
                        max_ahead_frames: message.go_max_ahead().expect("GO"),
                    },
                )?;
            }
        }
        LanMessageKind::Advertisement => {
            let local = local_name(state);
            if message.name().eq_ignore_ascii_case(&local) {
                return Ok(());
            }
            let service = state
                .match_state
                .lan_service
                .as_mut()
                .expect("live LAN visit");
            let open = message.advertisement_flags().expect("advertisement") & 1 != 0;
            if let Some(record) = service
                .games
                .iter_mut()
                .find(|record| record.name.eq_ignore_ascii_case(&message.name()))
            {
                record.endpoint = incoming.source;
                record.open = open;
                record.last_seen = now;
            } else {
                service.games.push(DiscoveryRecord {
                    endpoint: incoming.source,
                    name: message.name(),
                    open,
                    last_seen: now,
                });
            }
            project_games(state);
        }
        LanMessageKind::Discovery => {
            if state.match_state.session.host()
                && let Some(launch) = state.match_state.session.network_setup()
            {
                let name = launch
                    .humans
                    .first()
                    .map(|human| human.name.clone())
                    .unwrap_or_default();
                let advertisement = LanMessage::advertisement(&name, true)?;
                send(state, &advertisement, direct(incoming.source), true);
            }
        }
        LanMessageKind::JoinRequest => accept_join(state, incoming.source, &message)?,
        LanMessageKind::JoinAccepted => {
            let service = state.match_state.lan_service.as_mut().expect("visit");
            if service.pending_host != Some(incoming.source) {
                return Ok(());
            }
            let fields = message.acceptance_fields().expect("acceptance");
            let mut local = state.frontend.multiplayer_shell_runtime.local_participant();
            local.raw_slot_index = Some(u8::try_from(fields.assigned_row)?);
            local.slot = received_choices(fields.country, fields.color, &local.slot)?;
            local.role = crate::skirmish_launch::LaunchHumanRole::from_observer(
                fields.assigned_row == fields.observer_row && fields.country == -3,
            );
            let mut host = local.clone();
            host.name = message.name();
            // Host identity is held as envelope metadata until its own type3
            // arrives; Packet7 contains the guest's choices, not the host's.
            service.admitted_host = Some((host.name.clone(), incoming.source));
            service.pending_host = None;
            service.force_query = true;
            service.force_presence = true;
            super::shell_lan::service_event(
                state,
                super::shell_lan::LanServiceEvent::JoinAccepted(
                    super::shell_lan::LanJoinAcceptance {
                        game_id: endpoint_id(incoming.source),
                        local,
                        host,
                        observer_row: u8::try_from(fields.observer_row)
                            .ok()
                            .filter(|row| *row < 8),
                    },
                ),
            );
        }
        LanMessageKind::ParticipantDetails => {
            if state.frontend.network_setup_protocol.is_some() {
                accept_participant(state, incoming.source, &message)?;
            } else {
                observe_participant(state, incoming.source, message, now);
            }
        }
        LanMessageKind::Presence => observe_participant(state, incoming.source, message, now),
        LanMessageKind::GameQuery => {
            if state.match_state.session.network_setup().is_some() {
                let game_name = active_game_name(state);
                if game_name.as_deref() == Some(message.name().as_str()) {
                    let local = state
                        .match_state
                        .session
                        .network_setup()
                        .expect("setup")
                        .local_player();
                    let fields = crate::net::lan_messages::LanParticipantDetails {
                        country: country_value(local),
                        color: color_value(local),
                        game_name_hash: crate::net::lan_messages::game_name_hash(
                            game_name.as_deref().unwrap(),
                        ),
                        field_0x53: u32::from(local.raw_slot_index.unwrap_or(0)),
                        field_0x57: u32::from(local.is_observer()),
                    };
                    //5DC170 leaves an empty serial when its registry read fails.
                    let response = LanMessage::participant(&local.name, &[], fields)?;
                    send(state, &response, direct(incoming.source), true);
                }
            }
        }
        LanMessageKind::Departure => {
            if let Some(source) = state
                .match_state
                .session
                .network_endpoint_source(incoming.source)
                && state.frontend.network_setup_protocol.is_some()
            {
                if state
                    .match_state
                    .lan_service
                    .as_ref()
                    .and_then(|s| s.admitted_host.as_ref())
                    .is_some_and(|(_, endpoint)| *endpoint == incoming.source)
                {
                    super::shell_network_setup::service_event(
                        state,
                        super::shell_network_setup::NetworkSetupEvent::HostLeft,
                    )?;
                } else {
                    super::shell_network_setup::service_event(
                        state,
                        super::shell_network_setup::NetworkSetupEvent::ParticipantLeft {
                            index: source,
                        },
                    )?;
                }
            }
        }
        LanMessageKind::JoinDenied => {
            if let Some(service) = state.match_state.lan_service.as_ref()
                && service.pending_host == Some(incoming.source)
            {
                let game_id = endpoint_id(incoming.source);
                super::shell_lan::service_event(
                    state,
                    super::shell_lan::LanServiceEvent::JoinDenied {
                        game_id,
                        reason: message.denial_reason().expect("denial"),
                    },
                );
                state
                    .match_state
                    .lan_service
                    .as_mut()
                    .expect("visit")
                    .pending_host = None;
            } else if state.frontend.network_setup_protocol.is_some() {
                state
                    .match_state
                    .lan_service
                    .as_mut()
                    .expect("visit")
                    .begin_kicked_exit(&state.match_state.session, incoming.source, now)?;
                super::shell_network_setup::lock_lan_departure(state);
            }
        }
        _ => {}
    }
    Ok(())
}

fn project_games(state: &mut AppState) {
    let games = state
        .match_state
        .lan_service
        .as_ref()
        .expect("visit")
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

fn drain_requests(state: &mut AppState, now: u32) {
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
                .expect("visit")
                .discovery_timer = Default::default();
        }
        LanServiceRequest::SelectGame { game_id, .. } => {
            let service = state.match_state.lan_service.as_mut().expect("visit");
            service.selected_game = game_id;
            service.force_query = true;
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
                country: if observe {
                    -3
                } else if local.slot.country_random {
                    -2
                } else {
                    i32::from(local.slot.country.raw_index())
                },
                color: if local.slot.color_random {
                    -2
                } else {
                    i32::from(local.slot.color_index)
                },
                version: crate::net::lan_messages::LanVersionInterval::RETAIL,
                compatibility: compatibility(state)
                    .ok_or_else(|| anyhow::anyhow!("retail compatibility inputs unavailable"))?,
                observer_request: u32::from(observe),
            };
            let join = LanMessage::join_request(&name, &[], request)?;
            if send(state, &join, direct(peer), true) {
                state
                    .match_state
                    .lan_service
                    .as_mut()
                    .expect("visit")
                    .pending_host = Some(peer);
            }
        }
        LanServiceRequest::CancelJoin { .. } => {
            state
                .match_state
                .lan_service
                .as_mut()
                .expect("visit")
                .pending_host = None;
        }
        LanServiceRequest::NameChanged(_) => {
            if let Some(service) = state.match_state.lan_service.as_mut() {
                service.force_presence = true;
            }
        }
        LanServiceRequest::Chat { text, color } => {
            let game = active_game_name(state).unwrap_or_default();
            let message =
                LanMessage::in_game_chat(&name, color, &game, &String::from_utf16_lossy(&text))?;
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

fn active_game_name(state: &AppState) -> Option<String> {
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

fn country_value(human: &crate::skirmish_launch::SkirmishHumanSlot) -> i32 {
    if human.is_observer() {
        -3
    } else if human.slot.country_random {
        -2
    } else {
        i32::from(human.slot.country.raw_index())
    }
}
fn color_value(human: &crate::skirmish_launch::SkirmishHumanSlot) -> i32 {
    if human.slot.color_random {
        -2
    } else {
        i32::from(human.slot.color_index)
    }
}

fn received_choices(
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

fn accept_participant(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: &LanMessage,
) -> anyhow::Result<()> {
    let fields = message.participant_fields().expect("participant details");
    let name = message.name();
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
                .filter(|row| *row < 8)
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
    Ok(())
}

fn accept_join(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: &LanMessage,
) -> anyhow::Result<()> {
    let request = message.join_fields().expect("join request");
    let name = message.name();
    if let Some(launch) = state.match_state.session.network_setup()
        && let Some((index, _)) = launch
            .humans
            .iter()
            .enumerate()
            .find(|(_, human)| human.name.eq_ignore_ascii_case(&name))
        && index != launch.local_player_index
        && state.match_state.session.network_endpoint(index) == Some(endpoint)
    {
        return Ok(());
    }
    let reason = if !state.match_state.session.host()
        || state.frontend.network_setup_protocol.is_none()
    {
        Some(5)
    } else if state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| {
            launch
                .humans
                .iter()
                .any(|human| human.name.eq_ignore_ascii_case(&name))
        })
    {
        Some(0)
    } else if state
        .match_state
        .session
        .network_free_row(request.observer_request == 1)
        .is_none()
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
    };
    if let Some(reason) = reason {
        send(
            state,
            &LanMessage::join_denied(reason),
            direct(endpoint),
            true,
        );
        return Ok(());
    }
    let observer = request.observer_request == 1;
    let row = state
        .match_state
        .session
        .network_free_row(observer)
        .expect("checked free row");
    let initial = state
        .frontend
        .multiplayer_shell_runtime
        .local_participant()
        .slot;
    let mut human = crate::skirmish_launch::SkirmishHumanSlot {
        name: name.clone(),
        raw_slot_index: Some(row),
        slot: received_choices(request.country, request.color, &initial)?,
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
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("admitted setup");
    let human = launch.humans.last().expect("admitted participant");
    let acceptance = crate::net::lan_messages::LanAcceptance {
        country: country_value(human),
        color: color_value(human),
        assigned_row: i32::from(row),
        observer_row: state
            .match_state
            .session
            .network_observer_row()
            .map_or(-1, i32::from),
    };
    let accepted = LanMessage::join_accepted(&launch.local_player().name, acceptance)?;
    send(state, &accepted, direct(endpoint), true);
    Ok(())
}

/// Session697B70 binds source nodes to the actual constructed House table.
/// The same physical transport used for discovery/loading enters the command
/// owner; no private peer is inferred from a colour or UI row.
pub(crate) fn install_gameplay(state: &mut AppState) -> anyhow::Result<()> {
    if state.match_state.session_mode() != SessionMode::Lan
        || (state.platform.capture_client_size.is_some() && state.match_state.lan_service.is_none())
    {
        return Ok(());
    }
    anyhow::ensure!(
        state.match_state.lan_service.is_some(),
        "LAN transport was not opened"
    );
    let sim = &state
        .match_state
        .sim_runtime
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("LAN startup has no constructed world"))?
        .simulation;
    let timing = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.transport.as_ref())
        .ok_or_else(|| anyhow::anyhow!("LAN startup transport is already owned by a match"))?
        .retry_timing();
    let (local_house, seed, connections) =
        gameplay_connections(&state.match_state.session, sim, timing)?;
    let mut transport = state
        .match_state
        .lan_service
        .as_mut()
        .expect("visit")
        .transport
        .take()
        .ok_or_else(|| anyhow::anyhow!("LAN startup transport is already owned by a match"))?;
    *transport.connections_mut() = connections;
    // MainPrepareSession52EA18/52EA4F selects these after CreateConnections.
    // Keep validation above transactional: a failed binding retains the socket.
    let (private, global) = RetryTiming::lan_gameplay(|| transport.global().mean_response_time());
    transport.set_retry_timing(private, true);
    transport.global_mut().set_timing(global);
    // LAN5DD2DF/5DD49F, independent of the game-speed options field.
    let live = crate::net::live_commands::LiveCommandSession::new(
        transport,
        local_house,
        seed,
        std::num::NonZeroU32::new(5).expect("native LAN FrameSendRate"),
    )?;
    state.match_state.live_network = Some(live);
    Ok(())
}

/// End private channels on leaving gameplay, retaining the LAN visit's same
/// socket pair for BB. It is dropped only when that visit itself ends.
pub(crate) fn return_transport(state: &mut AppState) {
    let Some(live) = state.match_state.live_network.take() else {
        return;
    };
    if let Some(service) = state.match_state.lan_service.as_mut() {
        let mut transport = live.into_transport();
        transport.connections_mut().reset();
        service.transport = Some(transport);
        service.clear_setup();
    }
}

mod options;

fn send_bytes(
    state: &mut AppState,
    bytes: &[u8],
    destination: GlobalDestination,
    reliable: bool,
) -> bool {
    if let Some(live) = state.match_state.live_network.as_mut() {
        live.send_global(bytes, reliable, destination)
    } else {
        state
            .match_state
            .lan_service
            .as_mut()
            .is_some_and(|service| service.send(bytes, destination, reliable))
    }
}

fn send_sources(state: &mut AppState, bytes: &[u8], sources: impl IntoIterator<Item = usize>) {
    for source in sources {
        if let Some(endpoint) = state.match_state.session.network_endpoint(source) {
            send_bytes(state, bytes, direct(endpoint), true);
        }
    }
}

fn drain_match_requests(state: &mut AppState) {
    use super::match_runtime::session::NetworkLoadingRequest;
    let requests = state.match_state.session.take_network_loading_requests();
    for request in requests {
        match request {
            NetworkLoadingRequest::ReportProgress(percent) => {
                if let Ok(message) = crate::net::lan_messages::ingame::LoadingProgressMessage::new(
                    &local_name(state),
                    percent,
                ) {
                    if percent < 100 {
                        send_bytes(
                            state,
                            message.as_bytes(),
                            GlobalDestination::Broadcast { port_override: 0 },
                            false,
                        );
                    } else {
                        let local = super::match_runtime::network_match::local_source(
                            &state.match_state.session,
                        );
                        let sources: Vec<_> = state
                            .match_state
                            .session
                            .network_connected_source_indices()
                            .iter()
                            .copied()
                            .filter(|source| Some(*source) != local)
                            .collect();
                        send_sources(state, message.as_bytes(), sources);
                    }
                }
            }
            NetworkLoadingRequest::ProposeKick { target_source } => {
                if let Some(runtime) = &state.match_state.sim_runtime
                    && let Some(message) = super::match_runtime::network_match::encode_kick_proposal(
                        &state.match_state.session,
                        &runtime.simulation,
                        target_source,
                    )
                {
                    let local = super::match_runtime::network_match::local_source(
                        &state.match_state.session,
                    );
                    let sources: Vec<_> = state
                        .match_state
                        .session
                        .network_connected_source_indices()
                        .iter()
                        .copied()
                        .filter(|source| Some(*source) != local)
                        .collect();
                    send_sources(state, &message, sources);
                }
            }
            NetworkLoadingRequest::Disconnect { source_index, .. } => {
                if let Some(runtime) = &state.match_state.sim_runtime
                    && let Some(house) = super::match_runtime::network_match::source_house(
                        &state.match_state.session,
                        &runtime.simulation,
                        source_index,
                    )
                    && let Some(live) = state.match_state.live_network.as_mut()
                {
                    live.remove_peer(house as i8);
                }
            }
            NetworkLoadingRequest::Chat { recipients, text } => {
                let color = state
                    .match_state
                    .session
                    .network_setup()
                    .map_or(-2, |launch| color_value(launch.local_player()));
                let game_name = active_game_name(state).unwrap_or_default();
                if let Ok(message) =
                    LanMessage::in_game_chat(&local_name(state), color as u32, &game_name, &text)
                {
                    send_sources(state, message.as_bytes(), recipients);
                }
            }
            NetworkLoadingRequest::Taunt { recipients, byte } => {
                let color = state
                    .match_state
                    .session
                    .network_setup()
                    .map_or(-2, |launch| color_value(launch.local_player()));
                let game_name = active_game_name(state).unwrap_or_default();
                if let Ok(message) =
                    LanMessage::in_game_taunt(&local_name(state), color as u32, &game_name, byte)
                {
                    send_sources(state, message.as_bytes(), recipients);
                }
            }
            // The co-op round service retains these until its packet family
            // is connected; an ordinary Battle match never emits them.
            other => state
                .match_state
                .lan_service
                .as_mut()
                .expect("LAN visit")
                .deferred_match
                .push_back(other),
        }
    }
}

fn announce(state: &mut AppState, now: u32) {
    use super::match_runtime::session::WallBucketTimer;
    if state.match_state.live_network.is_some() {
        return;
    }
    let host_setup =
        state.match_state.session.host() && state.frontend.network_setup_protocol.is_some();
    let in_setup = state.frontend.network_setup_protocol.is_some();
    let Some(service) = state.match_state.lan_service.as_mut() else {
        return;
    };
    let discovery = service.discovery_timer.remaining(now) == 0 && !host_setup;
    let query = service.force_query || service.query_timer.remaining(now) == 0;
    let presence =
        service.force_presence || (!in_setup && service.presence_timer.remaining(now) == 0);
    if discovery {
        service.discovery_timer = WallBucketTimer::start(now, 120);
    }
    if query {
        service.query_timer = WallBucketTimer::start(now, 35);
        service.force_query = false;
    }
    if presence {
        service.presence_timer = WallBucketTimer::start(now, 83);
        service.force_presence = false;
    }
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
    if presence {
        let human = state
            .match_state
            .session
            .network_setup()
            .map(|launch| launch.local_player().clone())
            .unwrap_or_else(|| state.frontend.multiplayer_shell_runtime.local_participant());
        if let Ok(message) = LanMessage::presence(&name, country_value(&human), color_value(&human))
        {
            send(state, &message, broadcast, false);
        }
    }
}

fn remote_sources(state: &AppState) -> Vec<usize> {
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

fn drain_setup_requests(state: &mut AppState) {
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
        match setup_request(state, &request) {
            Ok(true) => {}
            Ok(false) => deferred.push_back(request),
            Err(error) => log::warn!("LAN setup request failed: {error:#}"),
        }
    }
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.deferred_setup.extend(deferred);
    }
}

fn setup_request(
    state: &mut AppState,
    request: &super::shell_network_setup::NetworkSetupRequest,
) -> anyhow::Result<bool> {
    use super::shell_network_setup::NetworkSetupRequest;
    let message = match request {
        NetworkSetupRequest::Kick { name } => {
            state
                .match_state
                .lan_service
                .as_mut()
                .expect("visit")
                .kick_peer(&state.match_state.session, name)?;
            return Ok(true);
        }
        NetworkSetupRequest::PublishOptions => options::encode_options(state)?,
        NetworkSetupRequest::PublishRoster => options::encode_roster(state)?,
        NetworkSetupRequest::Go {
            agreed_seed,
            max_ahead_frames,
        } => {
            anyhow::ensure!(
                state.match_state.session.network_start_seed() == Some(*agreed_seed),
                "GO seed differs from admitted options"
            );
            LanMessage::go(*max_ahead_frames)
        }
        NetworkSetupRequest::RequestAcceptance => LanMessage::start_prompt(&local_name(state))?,
        NetworkSetupRequest::Chat(text) => {
            let color = state
                .match_state
                .session
                .network_setup()
                .map_or(-2, |launch| color_value(launch.local_player()));
            LanMessage::in_game_chat(
                &local_name(state),
                color as u32,
                &active_game_name(state).unwrap_or_default(),
                &String::from_utf16_lossy(text),
            )?
        }
        NetworkSetupRequest::Leave => {
            let now = super::match_runtime::frame_pacer::frame_bucket(
                super::match_runtime::frame_pacer::wall_clock_ms(),
            );
            state
                .match_state
                .lan_service
                .as_mut()
                .expect("visit")
                .queue_departure(&state.match_state.session, now)?;
            return Ok(true);
        }
        NetworkSetupRequest::PlayerChoices { slot, observer, .. } => {
            let country = if *observer {
                -3
            } else if slot.country_random {
                -2
            } else {
                i32::from(slot.country.raw_index())
            };
            let color = if slot.color_random {
                -2
            } else {
                i32::from(slot.color_index)
            };
            let start = match slot.start_position {
                crate::skirmish_launch::LaunchStartPosition::Auto => -2,
                crate::skirmish_launch::LaunchStartPosition::Position(position) => {
                    i32::from(position)
                }
            };
            let team = match slot.team {
                crate::skirmish_launch::LaunchTeam::None => -2,
                crate::skirmish_launch::LaunchTeam::Team(team) => i32::from(team),
            };
            let message = LanMessage::player_choices(
                &local_name(state),
                format!("R{country},{color},{start},{team}").as_bytes(),
            )?;
            let endpoint = state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.admitted_host.as_ref())
                .map(|(_, endpoint)| *endpoint)
                .ok_or_else(|| anyhow::anyhow!("player choices have no admitted host"))?;
            send(state, &message, direct(endpoint), true);
            return Ok(true);
        }
        NetworkSetupRequest::Ready(ready) => {
            short_options(state, if *ready { "A1" } else { "A0" })?
        }
        NetworkSetupRequest::MapStatus(status) => short_options(state, &format!("K{status}"))?,
        NetworkSetupRequest::PrepareMapForHandoff { .. } => {
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
                .expect("visit")
                .apply_start_timing(false, false);
            super::shell_network_setup::prepare_lan_guest_handoff(state)?;
            send(state, &LanMessage::ready_to_go(), direct(endpoint), true);
            let now = super::match_runtime::frame_pacer::frame_bucket(
                super::match_runtime::frame_pacer::wall_clock_ms(),
            );
            super::shell_network_setup::mark_lan_go_packets_queued(state, now)?;
            return Ok(true);
        }
        // The existing transfer owner must explicitly complete these; never
        // invent a successful file handoff from a UI request.
        _ => return Ok(false),
    };
    send_sources(state, message.as_bytes(), remote_sources(state));
    if matches!(request, NetworkSetupRequest::Go { .. }) {
        let now = super::match_runtime::frame_pacer::frame_bucket(
            super::match_runtime::frame_pacer::wall_clock_ms(),
        );
        super::shell_network_setup::mark_lan_go_packets_queued(state, now)?;
    }
    Ok(true)
}

pub(crate) fn force_presence(state: &mut AppState) {
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.force_presence = true;
    }
}

fn ensure_host_source(state: &AppState, endpoint: SocketAddrV4) -> anyhow::Result<()> {
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
        .map_or(-2, |launch| color_value(launch.local_player()));
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

fn observe_participant(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: LanMessage,
    now: u32,
) {
    if message.name().eq_ignore_ascii_case(&local_name(state)) {
        return;
    }
    let service = state.match_state.lan_service.as_mut().expect("visit");
    if let Some(peer) = service
        .observed_participants
        .iter_mut()
        .find(|peer| peer.endpoint == endpoint && peer.message.kind() == message.kind())
    {
        peer.message = message;
        peer.last_seen = now;
    } else {
        service.observed_participants.push(ObservedParticipant {
            endpoint,
            message,
            last_seen: now,
        });
    }
    project_players(state);
}

fn project_players(state: &mut AppState) {
    let service = state.match_state.lan_service.as_ref().expect("visit");
    let game = service.selected_game.and_then(|id| service.game(id));
    let selected = service.selected_game;
    let mut players = Vec::new();
    if game.is_none() {
        players.push(super::shell_lan::local_lobby_row(&local_name(state)));
    }
    for peer in &service.observed_participants {
        let color = match (game, peer.message.kind()) {
            (Some(game), LanMessageKind::ParticipantDetails) => {
                let fields = peer.message.participant_fields().expect("participant");
                if fields.game_name_hash != crate::net::lan_messages::game_name_hash(&game.name) {
                    continue;
                }
                fields.color
            }
            (None, LanMessageKind::Presence) => peer.message.presence_fields().expect("presence").1,
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

pub(crate) fn close(state: &mut AppState) {
    if let Ok(message) = LanMessage::named(LanNamedMessage::Departure, &local_name(state)) {
        send(
            state,
            &message,
            GlobalDestination::Broadcast { port_override: 0 },
            false,
        );
        if let Some(transport) = state
            .match_state
            .lan_service
            .as_mut()
            .and_then(|service| service.transport.as_mut())
        {
            let now = super::match_runtime::frame_pacer::frame_bucket(
                super::match_runtime::frame_pacer::wall_clock_ms(),
            );
            transport.service(now);
        }
    }
    state.match_state.lan_service = None;
}

fn gameplay_connections(
    session: &super::match_runtime::session::MatchSession,
    sim: &crate::sim::world::Simulation,
    timing: RetryTiming,
) -> anyhow::Result<(i8, u32, LanConnections)> {
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("LAN startup has no admitted roster"))?;
    let local = launch.local_player_index;
    let local_house = super::match_runtime::network_match::source_house(session, sim, local)
        .ok_or_else(|| anyhow::anyhow!("local LAN participant has no constructed House"))?;
    let peers: Vec<_> = (0..launch.humans.len())
        .filter(|source| *source != local)
        .map(|source| {
            let house = super::match_runtime::network_match::source_house(session, sim, source)
                .ok_or_else(|| anyhow::anyhow!("LAN source {source} has no constructed House"))?;
            let endpoint = session
                .network_endpoint(source)
                .ok_or_else(|| anyhow::anyhow!("LAN source {source} has no accepted endpoint"))?;
            Ok((house, endpoint))
        })
        .collect::<anyhow::Result<_>>()?;
    let seed = session
        .network_start_seed()
        .ok_or_else(|| anyhow::anyhow!("LAN startup has no agreed seed"))?;
    let mut connections = LanConnections::new(LanConnectionConfig::default(), timing);
    for (house, endpoint) in peers {
        connections.add_peer(house, endpoint)?;
    }
    Ok((i8::try_from(local_house)?, seed, connections))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext};
    use crate::sim::{house_state::HouseState, world::Simulation};

    fn service() -> LanService {
        LanService::open_on(
            UdpBindConfig {
                primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            },
            &[],
        )
        .unwrap()
    }

    fn launch() -> crate::skirmish_launch::SkirmishLaunchSession {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        launch.humans[0].name = "Host".into();
        let mut guest = launch.local_player().clone();
        guest.name = "Guest".into();
        guest.raw_slot_index = Some(1);
        guest.slot.color_index = 2;
        guest.slot.start_position = crate::skirmish_launch::LaunchStartPosition::Position(2);
        launch.humans.push(guest);
        launch
    }

    fn session(host: bool, remote: SocketAddrV4) -> MatchSession {
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        let mut launch = launch();
        launch.local_player_index = usize::from(!host);
        session.accept_network_setup(launch, host, None).unwrap();
        session
            .bind_network_endpoint(if host { "Guest" } else { "Host" }, remote)
            .unwrap();
        session
    }

    fn next_message(service: &mut LanService, session: &MatchSession) -> GlobalMessage {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            service.transport.as_mut().unwrap().service(32);
            if let Some(message) = service.pop_lobby_received(session) {
                return message;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "LAN packet did not arrive"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn admitted_join_keeps_all_early_setup_packets_in_the_transport_until_setup_exists() {
        let mut sender = service();
        let mut receiver = service();
        let host = sender.transport.as_ref().unwrap().primary_addr().unwrap();
        let guest = receiver.transport.as_ref().unwrap().primary_addr().unwrap();
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        receiver.admitted_host = Some(("Host".into(), host));
        let messages = [
            LanMessage::options("Host", b"seed,map,options", 0, 0).unwrap(),
            LanMessage::roster("Host", &["Host", "Guest", "", "", "", "", "", ""]).unwrap(),
            LanMessage::start_prompt("Host").unwrap(),
            LanMessage::go(10),
        ];
        for message in &messages {
            assert!(sender.send(message.as_bytes(), direct(guest), true));
        }
        sender.transport.as_mut().unwrap().service(31);
        receiver.transport.as_mut().unwrap().service(32);
        assert!(receiver.pop_lobby_received(&session).is_none());
        // ACKs can retire every sender entry while the guest's shell still
        // transitions. Only the existing receive queue retains the packets.
        sender.transport.as_mut().unwrap().service(33);
        assert_eq!(
            sender
                .transport
                .as_ref()
                .unwrap()
                .global()
                .queued_send_count(),
            0
        );
        let mut accepted = launch();
        accepted.local_player_index = 1;
        session.accept_network_setup(accepted, false, None).unwrap();
        session.bind_network_endpoint("Host", host).unwrap();
        for expected in messages {
            assert_eq!(
                next_message(&mut receiver, &session).payload,
                expected.as_bytes()
            );
        }

        session.prepare_network_timing(2, || 0).unwrap();
        session.freeze_network_start(19).unwrap();
        let progress =
            crate::net::lan_messages::ingame::LoadingProgressMessage::new("Host", 100).unwrap();
        assert!(sender.send(progress.as_bytes(), direct(guest), true));
        sender.transport.as_mut().unwrap().service(34);
        assert_eq!(
            next_message(&mut receiver, &session).payload,
            progress.as_bytes()
        );
    }

    #[test]
    fn row_replacement_sends_kick_and_departure_cleans_membership_after_ack() {
        let mut host = service();
        let mut guest = service();
        let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
        let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
        let mut host_session = session(true, guest_address);
        let mut guest_session = session(false, host_address);
        guest.admitted_host = Some(("Host".into(), host_address));
        let removed = host_session
            .set_network_row_kind(
                1,
                super::super::match_runtime::session::NetworkRowKind::Open,
            )
            .unwrap()
            .unwrap();
        assert!(
            host_session.network_setup().unwrap().humans[1]
                .raw_slot_index
                .is_none()
        );
        host.kick_peer(&host_session, &removed.name).unwrap();
        host.transport.as_mut().unwrap().service(31);
        let denied = next_message(&mut guest, &guest_session);
        assert_eq!(
            LanMessage::decode(&denied.payload).unwrap().denial_reason(),
            Some(4)
        );
        assert!(
            guest
                .begin_kicked_exit(&guest_session, guest_address, 32)
                .is_err()
        );
        guest
            .begin_kicked_exit(&guest_session, denied.source, 32)
            .unwrap();
        assert!(!guest.kicked_exit_ready(true));
        let departed = next_message(&mut host, &host_session);
        let departed = LanMessage::decode(&departed.payload).unwrap();
        assert_eq!(departed.kind(), LanMessageKind::Departure);
        assert_eq!(departed.name(), "Guest");
        let source = host_session.network_endpoint_source(guest_address).unwrap();
        host_session.remove_network_participant(source).unwrap();
        assert_eq!(host_session.network_setup().unwrap().humans.len(), 1);
        assert!(
            host_session
                .network_endpoint_source(guest_address)
                .is_none()
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !guest.kicked_exit_ready(true) {
            guest.transport.as_mut().unwrap().service(33);
            assert!(
                std::time::Instant::now() < deadline,
                "departure did not drain"
            );
            std::thread::yield_now();
        }
        assert!(!guest.kicked_exit_ready(true), "the shell exit occurs once");
        assert!(guest.pop_lobby_received(&guest_session).is_none());
        guest.clear_setup();
        guest_session.return_to_network_lobby();
        assert!(guest.admitted_host.is_none());
        assert!(guest_session.network_setup().is_none());
    }

    #[test]
    fn lan_start_timing_survives_setup_back_and_resets_on_fresh_visit() {
        let mut service = service();
        service.apply_start_timing(false, true);
        let timing = service.transport.as_ref().unwrap().retry_timing();
        assert_eq!((timing.retry_delta, timing.timeout), (20, 160));
        let destination = direct(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123));
        assert!(service.send(b"pending after GO", destination, true));
        // A fresh BB visit explicitly restores lobby settings for both
        // channels; returning through Setup Back does not reset them.
        service.clear_setup();
        assert_eq!(service.transport.as_ref().unwrap().retry_timing(), timing);
        let global = service.transport.as_mut().unwrap().global_mut();
        // Guest5DD9FD leaves primary-global at60/-1/600, separately from
        // the private20/-1/160 above. Back must preserve that timing too.
        global.service(21, &mut |_, _| panic!("private timing leaked into global"));
        let mut sends = 0;
        global.service(61, &mut |_, _| sends += 1);
        assert_eq!(sends, 1);
        service.restart_visit();
        assert_eq!(
            service.transport.as_ref().unwrap().retry_timing(),
            LOBBY_RETRY_TIMING
        );
        assert!(service.send(b"fresh visit", destination, true));
        let mut sends = 0;
        service
            .transport
            .as_mut()
            .unwrap()
            .global_mut()
            .service(LOBBY_RETRY_TIMING.retry_delta + 1, &mut |_, _| sends += 1);
        assert_eq!(sends, 1);
    }

    #[test]
    fn lan_start_timing_reads_primary_global_history_not_the_private_maximum() {
        use crate::net::reliable::Delivery;

        let mut service = service();
        let transport = service.transport.as_mut().unwrap();
        let local = transport.primary_addr().unwrap();
        let remote = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        let connections = transport.connections_mut();
        connections.add_peer(7, remote).unwrap();
        assert!(connections.send_to(7, b"prior private traffic", Delivery::Reliable));
        let mut sent = Vec::new();
        connections.service(31, &mut |_, bytes| sent.push(bytes.to_vec()));
        assert_eq!(sent.len(), 1);

        let mut peer = LanConnections::new(LanConnectionConfig::default(), LOBBY_RETRY_TIMING);
        peer.add_peer(0, local).unwrap();
        let mut acks = Vec::new();
        peer.receive(local, &sent[0], &mut |_, bytes| acks.push(bytes.to_vec()))
            .unwrap();
        connections
            .receive(remote, &acks[0], &mut |_, _| {})
            .unwrap();
        connections.service(131, &mut |_, _| {});
        assert_eq!(connections.mean_response_time(7), Some(100));
        assert_eq!(transport.global().mean_response_time(), 0);
        assert_eq!(transport.response_time(), 100);

        // The native GO callers use5424C0, not the aggregate542450 getter
        // used by Wait_For_Players. A slower private channel is irrelevant.
        service.apply_start_timing(true, false);
        assert_eq!(
            service.transport.as_ref().unwrap().retry_timing(),
            RetryTiming {
                retry_delta: 30,
                max_retries: u32::MAX,
                timeout: 600,
            }
        );
        service.apply_start_timing(false, true);
        assert_eq!(
            service.transport.as_ref().unwrap().retry_timing(),
            RetryTiming {
                retry_delta: 20,
                max_retries: u32::MAX,
                timeout: 160,
            }
        );
    }

    #[test]
    fn setup_back_ends_admission_but_only_a_fresh_visit_resets_transport_history() {
        let mut service = LanService::open_on(
            UdpBindConfig {
                primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            },
            &[],
        )
        .unwrap();
        let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32100);
        let address = service.transport.as_ref().unwrap().primary_addr().unwrap();
        service.admitted_host = Some(("old host".into(), host));
        service.pending_host = Some(host);
        service.selected_game = Some(endpoint_id(host));
        service
            .deferred_setup
            .push_back(super::super::shell_network_setup::NetworkSetupRequest::RequestAcceptance);
        assert!(service.send(b"old options", direct(host), true));

        service.clear_setup();
        assert!(service.admitted_host.is_none());
        assert!(service.pending_host.is_none());
        assert!(service.selected_game.is_none());
        assert!(service.deferred_setup.is_empty());
        assert_eq!(
            service
                .transport
                .as_ref()
                .unwrap()
                .global()
                .queued_send_count(),
            1
        );

        service.restart_visit();
        let transport = service.transport.as_ref().unwrap();
        assert_eq!(transport.global().queued_send_count(), 0);
        assert_eq!(transport.primary_addr().unwrap(), address);
        assert!(transport.connections().peers().next().is_none());
    }

    #[test]
    fn gameplay_binding_uses_source_order_and_constructed_house_ids() {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        let mut guest = launch.local_player().clone();
        guest.name = "Guest".to_owned();
        guest.raw_slot_index = Some(1);
        guest.slot.color_index = 2;
        launch.humans.push(guest);
        launch.local_player_index = 1;
        let host_name = launch.humans[0].name.clone();
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session.accept_network_setup(launch, false, None).unwrap();
        session.set_network_start_seed(19).unwrap();
        let mut sim = Simulation::new();
        for (house, source) in [1, 0].into_iter().enumerate() {
            let owner = sim.intern(&session.network_source_owner(source).unwrap());
            sim.houses_mut_for_test().register(HouseState::new(
                owner,
                house as u8,
                None,
                true,
                0,
                10,
            ));
        }
        // A display participant is not a network connection until its source
        // endpoint is admitted. Failure does not take or mutate any sockets.
        assert!(gameplay_connections(&session, &sim, LOBBY_RETRY_TIMING).is_err());
        let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        session.bind_network_endpoint(&host_name, host).unwrap();
        let (local_house, seed, connections) =
            gameplay_connections(&session, &sim, LOBBY_RETRY_TIMING).unwrap();
        assert_eq!((local_house, seed), (0, 19));
        assert_eq!(connections.peers().collect::<Vec<_>>(), vec![(1, host)]);
        assert_eq!(session.network_endpoint(0), Some(host));
    }
}

fn service_transport(state: &mut AppState, now: u32) -> bool {
    if let Some(live) = state.match_state.live_network.as_mut() {
        live.service_transport(now);
        true
    } else if let Some(transport) = state
        .match_state
        .lan_service
        .as_mut()
        .and_then(|s| s.transport.as_mut())
    {
        let report = transport.service(now);
        report.discarded_global_sends == 0 && report.failed_peer.is_none()
    } else {
        true
    }
}

fn advance_go(state: &mut AppState, now: u32, service_ok: bool) {
    let Some(transport) = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|s| s.transport.as_ref())
    else {
        return;
    };
    let queued = transport.global().queued_send_count();
    let response = transport.global().mean_response_time();
    if !super::shell_network_setup::observe_lan_go_transport(state, now, queued, service_ok) {
        return;
    }
    let host = state.match_state.session.host();
    state
        .match_state
        .lan_service
        .as_mut()
        .expect("visit")
        .apply_start_timing(host, true);
    let result = (|| -> anyhow::Result<()> {
        if !state.match_state.session.host() {
            super::shell_network_setup::service_event(
                state,
                super::shell_network_setup::NetworkSetupEvent::Timing {
                    protocol: 2,
                    response_times: [response, response],
                },
            )?;
        }
        super::shell_network_setup::service_event(
            state,
            super::shell_network_setup::NetworkSetupEvent::GoCompleted,
        )
    })();
    if let Err(error) = result {
        log::error!("LAN start handoff failed: {error:#}");
    }
}

fn advance_kicked_exit(state: &mut AppState, service_ok: bool) -> bool {
    let Some(service) = state.match_state.lan_service.as_mut() else {
        return false;
    };
    if service.kicked_exit.is_none() {
        return false;
    }
    if service.kicked_exit_ready(service_ok)
        && let Err(error) = super::shell_network_setup::service_event(
            state,
            super::shell_network_setup::NetworkSetupEvent::Kicked,
        )
    {
        log::error!("LAN kick cleanup failed: {error:#}");
    }
    true
}
