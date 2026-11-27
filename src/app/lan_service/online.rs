//! Admission without discovery: the online control plane's transport handoff,
//! direct spawn launches and their derived admissions, and the online relay,
//! latency, restore and start-timing queries on the same service.

use crate::net::lan_messages::SessionScope;
use crate::net::reliable::LanConnectionConfig;
use crate::net::reliable::LanConnections;
use crate::net::reliable::RetryTiming;
use crate::net::udp::LanUdpTransport;
use crate::net::udp::UdpBindConfig;
use std::net::Ipv4Addr;
use std::net::SocketAddrV4;

use super::{
    AppState, ClientStep, LOBBY_RETRY_TIMING, LanService, PeerAdmission, PeerAdmissionRecord,
    online_retirement_pending,
};

/// The online control plane admits identity and the immutable roster. Once
/// its UDP probe has completed, transfer that same socket/session owner to
/// the shared loading/gameplay service. No synthetic connection succeeds here.
pub(crate) fn install_online_transport(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    transport: LanUdpTransport,
    host_name: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !online_retirement_pending(match_state),
        "Waiting to confirm departure from the previous online match."
    );
    anyhow::ensure!(
        match_state.live_network.is_none(),
        "a match already owns its transport"
    );
    anyhow::ensure!(
        match_state.session.network_return_context()
            == Some(super::match_runtime::session::NetworkReturnContext::Wol),
        "online transport requires an online visit"
    );
    let launch = match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("online transport requires an admitted roster"))?;
    anyhow::ensure!(
        transport.relay_ready(),
        "relay has not completed its UDP probe"
    );
    // A watcher has no source of its own: it stands in the seat of the
    // player it follows (relay README "Watchers").
    let watching = transport.relay_watch_delay().is_some();
    anyhow::ensure!(
        watching || transport.relay_source().map(usize::from) == Some(launch.local_player_index),
        "relay identity differs from the admitted local participant"
    );
    for source in 0..launch.humans.len() {
        let endpoint = crate::net::udp::relay_peer_endpoint(u8::try_from(source)?)?;
        anyhow::ensure!(
            match_state.session.network_endpoint(source) == Some(endpoint),
            "online participant {source} has no matching relay identity"
        );
    }
    let host_source = launch
        .humans
        .iter()
        .position(|human| human.name == host_name)
        .ok_or_else(|| anyhow::anyhow!("online host is outside the admitted roster"))?;
    let host_endpoint = match_state
        .session
        .network_endpoint(host_source)
        .ok_or_else(|| anyhow::anyhow!("online host has no admitted endpoint"))?;
    if watching {
        match_state
            .session
            .admit_local_network_watcher(transport.primary_addr()?)?;
    }
    let mut service = LanService::with_transport(transport);
    // The setup dialog is discarded before loading. Retain the same admitted
    // host binding as LAN so timing authority survives that handoff.
    service.admitted_host = Some((host_name.to_owned(), host_endpoint));
    match_state.lan_service = Some(service);
    Ok(())
}

/// The admission every participant of a direct spawn derives from the same
/// launch (docs/spawn.md), in place of discovery and join/confirm: a shared
/// setup scope, one join/admission nonce pair per source, and the bound
/// remote sources. Only the local index differs between participants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectAdmission {
    pub(super) scope: SessionScope,
    /// The local source's pair, as carried by its control proofs.
    pub(super) local: PeerAdmission,
    /// Source 0's name and endpoint; `None` at the host itself.
    pub(super) host: Option<(String, SocketAddrV4)>,
    /// Every remote source: endpoint, name and that source's nonce pair.
    pub(super) peers: Vec<(SocketAddrV4, String, PeerAdmission)>,
}

impl DirectAdmission {
    /// `map` is the launcher's map file: participants keep it in their own
    /// directories, so its bytes, not its resolved path, are shared.
    pub(crate) fn derive(
        launch: &crate::skirmish_launch::SkirmishLaunchSession,
        seed: u32,
        peers: &[Option<SocketAddrV4>],
        map: &[u8],
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            peers.len() == launch.humans.len()
                && launch.humans.len() >= 2
                && peers
                    .iter()
                    .enumerate()
                    .all(|(source, peer)| peer.is_none() == (source == launch.local_player_index)),
            "direct endpoints do not match the launch roster"
        );
        // Hash the launch every participant shares, so a different map,
        // option or roster can never enter another participant's scope. It
        // also holds each player's own Terrain Deformation and Superweapon
        // Limit (`start_spawn_session`, which leaves the Building Production
        // Queue off), so players whose Options rules differ never share one
        // either.
        let mut shared = launch.clone();
        shared.local_player_index = 0;
        shared.selected_map_file = None;
        let mut digest = crate::util::sha256::Sha256::new();
        digest.update(b"VERA20k direct spawn\0");
        digest.update(&seed.to_le_bytes());
        let mut map_digest = crate::util::sha256::Sha256::new();
        map_digest.update(map);
        digest.update(&map_digest.finalize());
        digest.update(&serde_json::to_vec(&shared)?);
        let root = digest.finalize();
        let nonce = |label: &[u8], source: usize| {
            let mut digest = crate::util::sha256::Sha256::new();
            digest.update(&root);
            digest.update(label);
            digest.update(&(source as u32).to_le_bytes());
            let mut nonce = [0; 16];
            nonce.copy_from_slice(&digest.finalize()[..16]);
            // A zero nonce means "none" to the join owners.
            nonce[0] |= 1;
            nonce
        };
        let identity = |source| PeerAdmission {
            join_nonce: nonce(b"join", source),
            admission_nonce: nonce(b"admission", source),
        };
        let mut epoch = [0; 16];
        epoch.copy_from_slice(&root[..16]);
        epoch[0] |= 1;
        let remote = peers
            .iter()
            .enumerate()
            .filter_map(|(source, peer)| {
                Some((
                    *peer.as_ref()?,
                    launch.humans[source].name.clone(),
                    identity(source),
                ))
            })
            .collect();
        Ok(Self {
            // The first published revision; maps::poll never serves a
            // preadmitted service's scope.
            scope: SessionScope { epoch, revision: 1 },
            local: identity(launch.local_player_index),
            host: (launch.local_player_index != 0)
                .then(|| peers[0].map(|endpoint| (launch.humans[0].name.clone(), endpoint)))
                .flatten(),
            peers: remote,
        })
    }
}

impl LanService {
    /// A direct spawn's service: the LAN socket on the launcher's port with
    /// every participant already admitted. The alternate receive socket
    /// (retail 1236) carries no VERA traffic, so it takes an ephemeral port
    /// and never collides with another local participant.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn open_direct(local_port: u16, admission: &DirectAdmission) -> std::io::Result<Self> {
        let transport = LanUdpTransport::bind(
            UdpBindConfig {
                primary: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, local_port),
                alternate: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
            },
            LanConnections::new(LanConnectionConfig::default(), LOBBY_RETRY_TIMING),
        )?;
        let mut service = Self::with_transport(transport);
        service.adopt_direct_admission(admission);
        Ok(service)
    }

    /// The setup state a completed discovery/join/confirm would have left.
    pub(super) fn adopt_direct_admission(&mut self, admission: &DirectAdmission) {
        self.preadmitted = true;
        self.force_presence = true;
        self.setup_scope = Some(admission.scope);
        self.join_nonce = Some(admission.local.join_nonce);
        self.admission_nonce = Some(admission.local.admission_nonce);
        self.admitted_host = admission.host.clone();
        self.peer_admissions = admission
            .peers
            .iter()
            .map(|(endpoint, name, identity)| {
                (
                    *endpoint,
                    PeerAdmissionRecord::project(None, name.clone(), *identity),
                )
            })
            .collect();
    }

    /// The host may send GO once every guest's keepalive has arrived: each
    /// one is running and has admitted this host.
    pub(super) fn direct_peers_heard(&self) -> bool {
        self.preadmitted
            && self
                .peer_admissions
                .values()
                .all(|record| record.heartbeat_sequence.is_some())
    }
}

/// Enter a direct multiplayer spawn (docs/spawn.md): the launcher's session
/// as a LAN setup without a dialog, every endpoint bound from the launch
/// file, then the ordinary LAN GO handshake (host source 0), loading and
/// gameplay. Desktop only.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn begin_direct_spawn(
    game_config: Option<&crate::util::config::GameConfig>,
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    launch: crate::skirmish_launch::SkirmishLaunchSession,
    direct: crate::app::frontend::cncnet_spawn::DirectSpawn,
    seed: u32,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        match_state.lan_service.is_none() && match_state.live_network.is_none(),
        "a network service is already open"
    );
    let map_file = launch
        .selected_map_file
        .clone()
        .ok_or_else(|| anyhow::anyhow!("direct launch has no map"))?;
    let map = crate::util::file_system::read(std::path::Path::new(&map_file))
        .map_err(|error| anyhow::anyhow!("could not read {map_file}: {error}"))?;
    let admission = DirectAdmission::derive(&launch, seed, &direct.peers, &map)?;
    let names: Vec<_> = launch
        .humans
        .iter()
        .map(|human| human.name.clone())
        .collect();
    super::shell_network_setup::install_external_setup(
        game_config,
        frontend,
        match_state,
        process_assets,
        launch,
        super::match_runtime::session::NetworkReturnContext::LanLobby,
    )?;
    for (name, peer) in names.iter().zip(&direct.peers) {
        if let Some(endpoint) = peer {
            match_state.session.bind_network_endpoint(name, *endpoint)?;
        }
    }
    match_state.session.set_network_start_seed(seed)?;
    let mut service = LanService::open_direct(direct.local_port, &admission).map_err(|error| {
        anyhow::anyhow!("could not open UDP port {}: {error}", direct.local_port)
    })?;
    service.start_io_pump()?;
    // The agreement reads the installed setup, not the service, so it is
    // made first and installed with the service.
    service.setup_agreement = Some(super::shell_network_setup::agreement::local(
        game_config,
        &match_state.session,
        process_assets,
    )?);
    match_state.lan_service = Some(service);
    Ok(())
}

/// The direct spawn host starts the ordinary LAN GO once every guest is up.
/// A failed launch never does: the participant that ended it is retired, so
/// the rest may all have answered. A GO that cannot start returns the step
/// that fails the launch.
#[must_use]
pub(super) fn poll_direct_start(
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
) -> Option<ClientStep> {
    if !match_state.session.host()
        || frontend.external_launch_error.is_some()
        || !super::shell_network_setup::external(frontend.network_setup_protocol.as_ref())
        || !frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| protocol.editable())
        || !match_state
            .lan_service
            .as_ref()
            .is_some_and(LanService::direct_peers_heard)
    {
        return None;
    }
    if let Err(error) = super::shell_network_setup::begin_external_lan_go(frontend, match_state) {
        log::warn!("Direct LAN start failed: {error:#}");
        return Some(ClientStep::Now(Box::new(|state: &mut AppState| {
            super::App::fail_external_launch(state, &crate::i18n::tr("VERA:StartRefusedSetup"))
        })));
    }
    None
}

pub(crate) fn begin_online_latency_probe(
    match_state: &mut crate::app::match_runtime::state::MatchState,
) -> anyhow::Result<()> {
    let launch = match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("latency probe requires an admitted roster"))?;
    let sources = (0..launch.humans.len())
        .filter(|source| *source != launch.local_player_index)
        .map(u8::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    match_state
        .lan_service
        .as_mut()
        .and_then(|service| service.transport.as_mut())
        .ok_or_else(|| anyhow::anyhow!("latency probe has no transport"))?
        .begin_relay_peer_probes(&sources)?;
    Ok(())
}

pub(crate) fn online_peer_round_trips(
    match_state: &crate::app::match_runtime::state::MatchState,
) -> Option<Vec<(u8, u32)>> {
    match_state
        .lan_service
        .as_ref()?
        .transport
        .as_ref()?
        .relay_peer_round_trips()
}

pub(crate) fn has_online_relay_route(
    match_state: &crate::app::match_runtime::state::MatchState,
) -> bool {
    if let Some(live) = match_state.live_network.as_ref() {
        return live.relay_expires().is_some();
    }
    match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.transport.as_ref())
        .is_some_and(|transport| transport.relay_expires().is_some())
}

pub(crate) fn renew_online_relay(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    address: SocketAddrV4,
    ticket: &[u8],
    key: [u8; 32],
    payload_key: [u8; 32],
) -> anyhow::Result<()> {
    if let Some(live) = match_state.live_network.as_mut() {
        return Ok(live.renew_relay(address, ticket, key, payload_key)?);
    }
    let transport = match_state
        .lan_service
        .as_mut()
        .and_then(|service| service.transport.as_mut())
        .ok_or_else(|| anyhow::anyhow!("online renewal has no admitted route"))?;
    Ok(transport.renew_relay(address, ticket, key, payload_key)?)
}

/// The installed online transport returns to a match under way.
pub(crate) fn mark_online_restore(
    match_state: &mut crate::app::match_runtime::state::MatchState,
) -> anyhow::Result<()> {
    match_state
        .lan_service
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("online restore has no transport"))?
        .online_restore = true;
    Ok(())
}

/// Whether this process is returning to a match the others are already playing.
pub(crate) fn online_restore(match_state: &crate::app::match_runtime::state::MatchState) -> bool {
    match_state
        .lan_service
        .as_ref()
        .is_some_and(|service| service.online_restore)
}

/// Apply the same agreed timing vector on every client before freezing the
/// start. Native794F6F's outputs stay with their existing session/transport
/// owners; construction of private channels later inherits this retry policy.
pub(crate) fn apply_online_timing(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    pings: &[i32],
) -> anyhow::Result<()> {
    let count = match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("online timing has no roster"))?
        .humans
        .len();
    anyhow::ensure!(
        pings.len() == count && pings.iter().all(|ping| (0..=30000).contains(ping)),
        "online timing vector does not match the admitted roster"
    );
    let host = match_state.session.host();
    let timing = match_state
        .session
        .prepare_wol_timing(pings.iter().copied(), host)?;
    let service = match_state
        .lan_service
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("online timing has no transport"))?;
    let transport = service
        .transport
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("online transport already moved to gameplay"))?;
    if let Some(retry) = timing.set_timing {
        // IPXManager::SetTiming540C60 stores the defaults, updates every
        // private channel, and (true at795114) its primary global channel.
        transport.set_retry_timing(
            RetryTiming {
                retry_delta: retry.retry_delta as u32,
                max_retries: retry.max_retries as u32,
                timeout: retry.timeout as u32,
            },
            retry.apply_to_connections,
        );
    }
    service.online_timing = Some(timing);
    Ok(())
}
