//! The LAN visit's physical service. Discovery records are projections; accepted
//! participant addresses belong to MatchSession. One socket pair moves from
//! this service to LiveCommandSession at the native697B70 connection boundary.
//!
//! Retail LAN opens IPX at52E3D2..52E41F. This owner's IPv4 broadcast is an
//! explicit VERA transport adaptation; application messages remain the native
//! 455-byte protocol from5DF9E0/5DFE00/5E00B0.

use crate::app::match_runtime::sim_host::SimWrite;
use std::net::{Ipv4Addr, SocketAddrV4};

use super::AppState;
use super::match_runtime::session::SessionMode;
use crate::net::global::{GlobalDestination, GlobalMessage};
use crate::net::lan_messages::{
    LanCompatibility, LanJoinRequest, LanMessage, LanMessageKind, LanNamedMessage, LanPayload,
    LanSessionProof, SessionScope,
};
use crate::net::reliable::{
    Delivery, LAN_PRODUCT_ID, LanConnectionConfig, LanConnections, RetryTiming,
};
use crate::net::udp::{DEFAULT_LAN_PORT, LanUdpTransport, UdpBindConfig};
use crate::ui::lan_shell::LanSelectionChange;

// VERA IPv4 setup lease: 1800 native16-ms wall buckets (28.8s), allowing
// more than21 missed83-bucket announcements. Native5DF9E0 owns the same
// presence cadence;5E1410's nativeType5 probe is not an admitted-peer lease.
const SETUP_PEER_LEASE: u32 = 1800;

/// What one setup keepalive did.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SetupKeepalive {
    /// Not a renewal: another scope or sender, or a replayed sequence.
    Ignored,
    /// Renewed its sender's setup lease.
    Renewed,
    /// VERA: a direct spawn's participant, named here, runs another build.
    OtherBuild(String),
    /// VERA: a direct spawn's participant, named here, plays other rules.
    OtherRules(String),
}

const LOBBY_RETRY_TIMING: RetryTiming = RetryTiming {
    retry_delta: 30,
    max_retries: u32::MAX,
    timeout: 600,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepartureReason {
    Kicked,
    Back,
    CancelJoin { game_id: u64 },
}

struct DepartureWait {
    reason: DepartureReason,
    deadline: super::match_runtime::session::WallBucketTimer,
    receipt: bool,
    completed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct PeerAdmission {
    join_nonce: [u8; 16],
    admission_nonce: [u8; 16],
}

/// Derived from the host's admitted Session snapshot. This names which
/// challenge may assemble a peer's Type3 into the local Session; it is not a
/// second participant/address writer.
struct PeerAdmissionRecord {
    name: String,
    identity: PeerAdmission,
    lease: Option<super::match_runtime::session::WallBucketTimer>,
    heartbeat_sequence: Option<u32>,
}

impl PeerAdmissionRecord {
    fn project(previous: Option<Self>, name: String, identity: PeerAdmission) -> Self {
        if let Some(previous) = previous
            && previous.name.eq_ignore_ascii_case(&name)
            && previous.identity == identity
        {
            return previous;
        }
        Self {
            name,
            identity,
            lease: None,
            heartbeat_sequence: None,
        }
    }
}

#[derive(Clone)]
struct PendingAdmission {
    endpoint: SocketAddrV4,
    request: LanMessage,
    /// `request`'s join fields, read when it arrived.
    fields: LanJoinRequest,
    identity: PeerAdmission,
    deadline: super::match_runtime::session::WallBucketTimer,
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

/// VERA: an outside-data clamp. gamemd's receive adds each new name to its
/// growable game list (`0x005E038F`); here a new sender is ignored while the
/// list is full, listed rows keep refreshing, and the `0x005DB720` expiry frees
/// rows. It bounds what each advertisement re-projects and compares.
const MAX_LAN_GAMES: usize = 64;

/// VERA: the same clamp for lobby player observations. A row is kept per sender
/// and message kind, so a player sending both Presence and ParticipantDetails
/// takes two.
const MAX_LAN_PARTICIPANTS: usize = 256;

/// VERA: rows one IPv4 address may hold in each table, so one machine sending
/// from many ports cannot fill a table. A game binds the one LAN port, so an
/// honest address holds one game and two observations; the rest is room for
/// machines behind one address.
const MAX_LAN_GAMES_PER_ADDRESS: usize = 4;
const MAX_LAN_PARTICIPANTS_PER_ADDRESS: usize = 8;

/// How long a player the host kicks from VERA's room is kept out of it.
const KICK_KEEP_OUT_BUCKETS: u32 = super::match_runtime::frame_pacer::frame_bucket(5 * 60 * 1000);

/// A player the host kicked from VERA's room: joins from that address or
/// name are refused until the wall bucket `until`.
struct KeptOut {
    address: Ipv4Addr,
    name: String,
    until: u32,
}

pub(crate) struct LanService {
    setup_generation: u64,
    setup_scope: Option<SessionScope>,
    setup_agreement: Option<super::shell_network_setup::agreement::MultiplayerAgreement>,
    received_options: Option<options::ReceivedOptions>,
    join_nonce: Option<[u8; 16]>,
    admission_nonce: Option<[u8; 16]>,
    // Correlation only. MatchSession remains the sole admitted roster/address owner.
    pending_admissions: Vec<PendingAdmission>,
    peer_admissions: std::collections::BTreeMap<SocketAddrV4, PeerAdmissionRecord>,
    setup_reassembly: crate::net::lan_messages::extended_setup::Reassembly,
    maps: maps::Transfers,
    pending_setup: std::collections::VecDeque<(Vec<u8>, GlobalDestination)>,
    // The loading ACK drain's deadline (loading.rs). The browser keeps it
    // across event-loop turns; the desktop loader waits it out in one milestone.
    loading_drain: Option<crate::sim::timer::CdTimer>,
    transport: Option<LanUdpTransport>,
    // Explicit visit exit, never inferred from silence or the current screen.
    // The same socket owner survives until its terminal relay ACK arrives.
    online_closing: bool,
    online_timing: Option<crate::net::lockstep::WolStartTiming>,
    // This process replaces one that left the match: the others are already
    // playing, and its loading is nobody's business but its own.
    online_restore: bool,
    games: Vec<DiscoveryRecord>,
    /// Radio observations for BB only; never admitted as gameplay endpoints.
    observed_participants: Vec<ObservedParticipant>,
    selected_game: Option<u64>,
    pending_host: Option<SocketAddrV4>,
    admitted_host: Option<(String, SocketAddrV4)>,
    departure: Option<DepartureWait>,
    /// Native5DF9E0's independent discovery/query/presence timers.
    discovery_timer: super::match_runtime::session::WallBucketTimer,
    query_timer: super::match_runtime::session::WallBucketTimer,
    presence_timer: super::match_runtime::session::WallBucketTimer,
    force_presence: bool,
    force_query: bool,
    deferred_setup: std::collections::VecDeque<super::shell_network_setup::NetworkSetupRequest>,
    /// VERA-only: who the host kicked and may not rejoin yet.
    kept_out: Vec<KeptOut>,
    /// A direct spawn launch (docs/spawn.md): the launcher admitted the
    /// roster and the map, so discovery, join/confirm and map transfer never
    /// run; scope, nonces and endpoints come from [`DirectAdmission`].
    preadmitted: bool,
}

impl LanService {
    pub(crate) fn open() -> std::io::Result<Self> {
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
        Ok(Self::with_transport(transport))
    }

    /// Normal App activation; explicit/manual-clock fixture constructors stay synchronous.
    pub(crate) fn start_io_pump(&mut self) -> std::io::Result<()> {
        self.transport
            .as_mut()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "LAN transport is not installed",
                )
            })?
            .start_io_pump(super::match_runtime::frame_pacer::wall_clock_bucket)
    }

    fn with_transport(transport: LanUdpTransport) -> Self {
        Self {
            transport: Some(transport),
            online_closing: false,
            online_timing: None,
            online_restore: false,
            games: Vec::new(),
            observed_participants: Vec::new(),
            selected_game: None,
            pending_host: None,
            admitted_host: None,
            departure: None,
            discovery_timer: Default::default(),
            query_timer: Default::default(),
            presence_timer: Default::default(),
            force_presence: true,
            force_query: false,
            deferred_setup: Default::default(),
            kept_out: Vec::new(),
            preadmitted: false,
            setup_generation: 0,
            setup_scope: None,
            setup_agreement: None,
            received_options: None,
            join_nonce: None,
            admission_nonce: None,
            pending_admissions: Vec::new(),
            peer_admissions: Default::default(),
            setup_reassembly: Default::default(),
            maps: Default::default(),
            pending_setup: Default::default(),
            loading_drain: None,
        }
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

    /// All setup control packets share the fragment FIFO. A full native
    /// global channel is backpressure, never successful application admission.
    fn queue_setup(
        &mut self,
        packets: impl ExactSizeIterator<Item = (Vec<u8>, GlobalDestination)>,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(packets.len() <= 4096, "LAN setup batch is too large");
        if self.pending_setup.len().saturating_add(packets.len()) > 4096 {
            return Ok(false);
        }
        self.pending_setup.extend(packets);
        self.flush_setup();
        Ok(true)
    }

    pub(super) fn setup_scope(&self) -> Option<SessionScope> {
        self.setup_scope
    }

    /// Physical source, effective map and generated preview/staging are one
    /// admission through the retained transfer owner, for this exact options.
    fn setup_maps_verified(&self, campaign_verified: Option<bool>) -> bool {
        // A direct spawn's map is the launcher's file beside spawn.ini; the
        // selected-mode agreement still compares the rules digest at GO.
        self.setup_scope.is_some_and(|scope| {
            self.preadmitted
                || campaign_verified.unwrap_or_else(|| self.maps.current_verified(scope))
        })
    }

    pub(super) fn setup_roster_complete(
        &self,
        session: &super::match_runtime::session::MatchSession,
    ) -> bool {
        let Some(launch) = session.network_setup() else {
            return false;
        };
        if self.preadmitted {
            // Every remote source was bound from the launch file at install.
            return (0..launch.humans.len()).all(|source| {
                source == launch.local_player_index || session.network_endpoint(source).is_some()
            });
        }
        if !session.host()
            && (self.received_options.is_none()
                || self.peer_admissions.len() != launch.humans.len())
        {
            return false;
        }
        launch.humans.iter().enumerate().all(|(source, human)| {
            if source == launch.local_player_index {
                return session.host()
                    || self.peer_admissions.values().any(|record| {
                        record.name.eq_ignore_ascii_case(&human.name)
                            && Some(record.identity.join_nonce) == self.join_nonce
                            && Some(record.identity.admission_nonce) == self.admission_nonce
                    });
            }
            session
                .network_endpoint(source)
                .and_then(|endpoint| self.peer_admissions.get(&endpoint))
                .is_some_and(|record| record.name.eq_ignore_ascii_case(&human.name))
        })
    }

    fn ensure_host_scope(&mut self) -> anyhow::Result<SessionScope> {
        if let Some(scope) = self.setup_scope {
            return Ok(scope);
        }
        let scope = SessionScope {
            epoch: fresh_nonce()?,
            revision: 0,
        };
        self.setup_scope = Some(scope);
        Ok(scope)
    }

    fn control_proof(&self) -> anyhow::Result<LanSessionProof> {
        Ok(LanSessionProof {
            scope: self
                .setup_scope
                .ok_or_else(|| anyhow::anyhow!("LAN setup has no admitted scope"))?,
            join_nonce: self.join_nonce.unwrap_or([0; 16]),
            admission_nonce: self.admission_nonce.unwrap_or([0; 16]),
            version: super::shell_network_setup::agreement::version(),
            rules_digest: String::new(),
        })
    }

    /// A control proof with this game's rules digest, for the messages whose
    /// receiver compares it: GO, its acknowledgements and setup keepalives.
    fn agreed_proof(&self) -> anyhow::Result<LanSessionProof> {
        let mut proof = self.control_proof()?;
        if let Some(agreement) = self.setup_agreement.as_ref() {
            proof.rules_digest.clone_from(&agreement.rules_digest);
        }
        Ok(proof)
    }

    fn validate_scope(&self, proof: &LanSessionProof) -> bool {
        self.setup_scope == Some(proof.scope) && self.in_room(proof)
    }

    fn accepts_join_reply(&self, source: SocketAddrV4, proof: &LanSessionProof) -> bool {
        self.departure.is_none() && self.correlates_join_reply(source, proof)
    }

    fn correlates_join_reply(&self, source: SocketAddrV4, proof: &LanSessionProof) -> bool {
        self.pending_host == Some(source)
            && self.join_nonce == Some(proof.join_nonce)
            && proof.scope.epoch != [0; 16]
    }

    fn validate_peer_agreement(&self, proof: &LanSessionProof) -> bool {
        self.validate_scope(proof)
            && self
                .setup_agreement
                .as_ref()
                .is_some_and(|expected| expected.rules_digest == proof.rules_digest)
    }

    /// The proof names this room at any options revision and uses its build.
    fn in_room(&self, proof: &LanSessionProof) -> bool {
        self.setup_scope
            .is_some_and(|scope| scope.epoch == proof.scope.epoch)
            && proof.version == super::shell_network_setup::agreement::version()
    }

    pub(super) fn queued_setup_count(&self) -> usize {
        self.pending_setup.len()
            + self
                .transport
                .as_ref()
                .map_or(0, |transport| transport.global().queued_send_count())
    }

    fn go_transport_observation(&self, service_ok: bool) -> Option<SetupTransportObservation> {
        // Guest GO5DD950..97D observes service success and its queue together.
        // A worker may have timed out the last Type13 since the earlier poll;
        // read that pending failure with the frontier before any App callback.
        let observed = self.transport.as_ref()?.global_observation();
        Some(SetupTransportObservation {
            queued: self.pending_setup.len() + observed.queued_sends,
            response_time: observed.response_time,
            service_ok: service_ok && !observed.pending_delivery_failure,
        })
    }

    fn can_queue_go(&self, recipients: usize) -> bool {
        self.queued_setup_count() == 0
            && self
                .transport
                .as_ref()
                .is_some_and(|transport| transport.global().available_send_slots() >= recipients)
    }

    /// Global53FCB0 delivers by arrival, so FIFO admission alone cannot order
    /// a large VERA setup before GO. ACK every earlier packet first; its guest
    /// receive entry then precedes GO even when datagrams reorder in transit.
    /// Keep an unadmitted request in deferred_setup, using this same FIFO only
    /// after capacity for its whole recipient set is available.
    fn queue_go(
        &mut self,
        message: &LanMessage,
        destinations: &[GlobalDestination],
    ) -> anyhow::Result<bool> {
        if !self.can_queue_go(destinations.len()) {
            return Ok(false);
        }
        let proof = self.agreed_proof()?;
        let message = message.clone().with_session_proof(&proof)?;
        let transport = self
            .transport
            .as_mut()
            .expect("GO preflight retains transport");
        // Setup Back retains its departure until ACK. Only this drained
        // boundary may retire a former prepared route before a new match.
        if transport.lan_scope().is_some_and(|old| old != proof.scope) {
            transport.clear_lan_scope();
        }
        transport.prepare_lan_scope(proof.scope)?;
        let admitted = self.queue_setup(
            destinations
                .iter()
                .map(|destination| (message.as_bytes().to_vec(), *destination)),
        )?;
        assert!(admitted, "reserved GO batch must fit the setup FIFO");
        assert!(
            self.pending_setup.is_empty(),
            "reserved GO slots must admit every recipient"
        );
        Ok(true)
    }

    fn flush_setup(&mut self) {
        if let Some(transport) = self.transport.as_mut() {
            Self::flush_setup_queue(&mut self.pending_setup, &mut *transport.global_mut());
        }
    }

    /// The same FIFO remains here when gameplay takes the physical transport.
    /// A bounded LoadingCallback drain may still leave terminal100 outside
    /// global160; both service phases admit it through this one loop.
    fn flush_setup_queue(
        pending: &mut std::collections::VecDeque<(Vec<u8>, GlobalDestination)>,
        global: &mut crate::net::global::GlobalChannel,
    ) {
        while let Some((bytes, destination)) = pending.front() {
            if !global.send(bytes, *destination, Delivery::Reliable, 0) {
                break;
            }
            pending.pop_front();
        }
    }

    fn game(&self, id: u64) -> Option<&DiscoveryRecord> {
        self.games
            .iter()
            .find(|record| endpoint_id(record.endpoint) == id)
    }

    /// The lobby's game ID is the sender endpoint. VERA: gamemd's receive
    /// (`0x005E018D..0x005E0233`) finds the row by name and moves its address
    /// only when the game turns open, so same-named hosts share one row; here
    /// each sender keeps its own row, and a rematch guest rejoins its original
    /// host. The usual `0x005DB720` expiry retires stale entries, and a new
    /// sender is ignored while [`MAX_LAN_GAMES`] are listed, or
    /// [`MAX_LAN_GAMES_PER_ADDRESS`] from its address.
    fn record_advertised_game(
        &mut self,
        endpoint: SocketAddrV4,
        name: String,
        open: bool,
        now: u32,
    ) {
        if let Some(record) = self
            .games
            .iter_mut()
            .find(|record| record.endpoint == endpoint)
        {
            record.name = name;
            record.open = open;
            record.last_seen = now;
        } else if self.games.len() < MAX_LAN_GAMES
            && self
                .games
                .iter()
                .filter(|record| record.endpoint.ip() == endpoint.ip())
                .count()
                < MAX_LAN_GAMES_PER_ADDRESS
        {
            self.games.push(DiscoveryRecord {
                endpoint,
                name,
                open,
                last_seen: now,
            });
        }
    }

    /// Keeps the latest message of its kind from each sender for BB's player
    /// list; a new row is ignored while [`MAX_LAN_PARTICIPANTS`] are held, or
    /// [`MAX_LAN_PARTICIPANTS_PER_ADDRESS`] from its address.
    fn record_observed_participant(
        &mut self,
        endpoint: SocketAddrV4,
        message: LanMessage,
        now: u32,
    ) {
        if let Some(peer) = self
            .observed_participants
            .iter_mut()
            .find(|peer| peer.endpoint == endpoint && peer.message.kind() == message.kind())
        {
            peer.message = message;
            peer.last_seen = now;
        } else if self.observed_participants.len() < MAX_LAN_PARTICIPANTS
            && self
                .observed_participants
                .iter()
                .filter(|peer| peer.endpoint.ip() == endpoint.ip())
                .count()
                < MAX_LAN_PARTICIPANTS_PER_ADDRESS
        {
            self.observed_participants.push(ObservedParticipant {
                endpoint,
                message,
                last_seen: now,
            });
        }
    }

    fn pop_lobby_received(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
    ) -> Option<GlobalMessage> {
        // Native5E00B0 changes BB to BD synchronously on Type7. Our slide is
        // asynchronous: retain every subsequent packet in the existing global
        // queue until MatchSession has accepted BD, including options/GO.
        // Frozen loading still owns that setup and must keep receiving progress.
        if self
            .departure
            .as_ref()
            .is_some_and(|wait| !matches!(wait.reason, DepartureReason::CancelJoin { .. }))
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
        keep_out: bool,
        now: u32,
    ) -> anyhow::Result<bool> {
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
        if keep_out {
            // Several players can share an address, so each keeps its entry.
            self.kept_out.retain(|entry| {
                (entry.until.wrapping_sub(now) as i32) > 0
                    && !(entry.address == *endpoint.ip() && entry.name.eq_ignore_ascii_case(name))
            });
            self.kept_out.push(KeptOut {
                address: *endpoint.ip(),
                name: name.to_owned(),
                until: now.wrapping_add(KICK_KEEP_OUT_BUCKETS),
            });
        }
        //5EC9F0 ->5DB600: reliable Type8/reason4 to the old node. Membership
        // remains until its Type10 reply, even after its display row is unbound.
        let mut proof = self.control_proof()?;
        if let Some(identity) = self
            .peer_admissions
            .get(&endpoint)
            .map(|record| record.identity)
        {
            proof.join_nonce = identity.join_nonce;
            proof.admission_nonce = identity.admission_nonce;
        }
        let denied = LanMessage::join_denied(4).with_session_proof(&proof)?;
        self.queue_setup(std::iter::once((
            denied.as_bytes().to_vec(),
            direct(endpoint),
        )))
    }

    /// Whether a join from `endpoint` as `name` comes from a player the host
    /// kicked less than five minutes ago.
    fn keeps_out(&self, endpoint: SocketAddrV4, name: &str, now: u32) -> bool {
        self.kept_out.iter().any(|entry| {
            (entry.until.wrapping_sub(now) as i32) > 0
                && (entry.address == *endpoint.ip() || entry.name.eq_ignore_ascii_case(name))
        })
    }

    fn queue_departure(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
        _now: u32,
    ) -> anyhow::Result<bool> {
        // Native5E09CD..5E0AC6 services/drains reliable direct Type10 before
        // returning. VERA retains the entire bounded batch through the same
        // setup FIFO when the larger roster/map stream fills global160.
        let packets = departure_packets(session, Some(&self.control_proof()?))?;
        self.queue_setup(packets.into_iter())
    }

    fn begin_departure(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
        reason: DepartureReason,
        now: u32,
    ) -> anyhow::Result<()> {
        if self.departure.is_some() {
            return Ok(());
        }
        self.maps.cancel();
        self.deferred_setup.clear();
        self.pending_setup.clear();
        let receipt = !matches!(reason, DepartureReason::CancelJoin { .. });
        if let DepartureReason::CancelJoin { .. } = reason {
            let endpoint = self
                .pending_host
                .ok_or_else(|| anyhow::anyhow!("canceled join has no host"))?;
            let proof = LanSessionProof {
                scope: self.setup_scope.unwrap_or(SessionScope {
                    epoch: [0; 16],
                    revision: 0,
                }),
                join_nonce: self
                    .join_nonce
                    .ok_or_else(|| anyhow::anyhow!("canceled join has no attempt"))?,
                admission_nonce: self.admission_nonce.unwrap_or([0; 16]),
                version: super::shell_network_setup::agreement::version(),
                rules_digest: String::new(),
            };
            let message =
                LanMessage::named(LanNamedMessage::CancelJoin, "")?.with_session_proof(&proof)?;
            anyhow::ensure!(
                self.queue_setup(std::iter::once((
                    message.as_bytes().to_vec(),
                    direct(endpoint)
                )))?,
                "canceled join could not enter the cleared FIFO"
            );
        } else {
            anyhow::ensure!(
                self.queue_departure(session, now)?,
                "departure could not enter the cleared FIFO"
            );
        }
        self.departure = Some(DepartureWait {
            reason,
            deadline: super::match_runtime::session::WallBucketTimer::start(now, 600),
            receipt,
            completed: false,
        });
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
        self.begin_departure(session, DepartureReason::Kicked, now)
    }

    fn departure_ready(&mut self, now: u32, service_ok: bool) -> Option<DepartureReason> {
        let wait = self.departure.as_ref()?;
        if wait.completed {
            return None;
        }
        let failed = !service_ok || wait.deadline.remaining(now) == 0;
        if !failed && (!wait.receipt || self.queued_setup_count() != 0) {
            return None;
        }
        if failed {
            self.pending_setup.clear();
            // Keep the same global sequence owner through a BB retry. Resetting
            // it here would replay sequence0 against the host's retained dedup
            // ring. Its bounded reliable attempts retain their old admission
            // proof, so they cannot mutate a replacement; the ordinary channel
            // timeout retires them. Unsent application work is canceled above.
        }
        let wait = self.departure.as_mut().expect("retained exit");
        wait.completed = true;
        Some(wait.reason)
    }

    fn join_offer(
        &mut self,
        endpoint: SocketAddrV4,
        request: &LanMessage,
        now: u32,
    ) -> anyhow::Result<PeerAdmission> {
        let LanPayload::JoinRequest(fields) = request.payload() else {
            anyhow::bail!("not a join request");
        };
        let proof = request
            .session_proof()?
            .ok_or_else(|| anyhow::anyhow!("join proof missing"))?;
        self.pending_admissions
            .retain(|entry| entry.deadline.remaining(now) != 0);
        if let Some(entry) = self.pending_admissions.iter().find(|entry| {
            entry.endpoint == endpoint && entry.identity.join_nonce == proof.join_nonce
        }) {
            anyhow::ensure!(
                entry.request == *request,
                "join attempt changed its request"
            );
            return Ok(entry.identity);
        }
        anyhow::ensure!(
            self.pending_admissions.len() < crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT,
            "too many pending LAN admissions"
        );
        let identity = PeerAdmission {
            join_nonce: proof.join_nonce,
            admission_nonce: fresh_nonce()?,
        };
        self.pending_admissions.push(PendingAdmission {
            endpoint,
            request: request.clone(),
            fields,
            identity,
            deadline: super::match_runtime::session::WallBucketTimer::start(now, 600),
        });
        Ok(identity)
    }

    /// The offer this Confirm answers: its sender's name and join fields. Its
    /// caller, `confirm_join`, has already checked the proof is
    /// [`Self::in_room`].
    fn confirmed_request(
        &mut self,
        endpoint: SocketAddrV4,
        proof: &LanSessionProof,
        now: u32,
    ) -> Option<(String, LanJoinRequest)> {
        self.pending_admissions
            .retain(|entry| entry.deadline.remaining(now) != 0);
        self.pending_admissions
            .iter()
            .find(|entry| {
                entry.endpoint == endpoint
                    && entry.identity.join_nonce == proof.join_nonce
                    && entry.identity.admission_nonce == proof.admission_nonce
            })
            .map(|entry| (entry.request.name(), entry.fields))
    }

    fn peer_admission_matches(&self, endpoint: SocketAddrV4, proof: &LanSessionProof) -> bool {
        self.peer_admissions.get(&endpoint).is_some_and(|record| {
            let identity = record.identity;
            identity.join_nonce == proof.join_nonce
                && identity.admission_nonce == proof.admission_nonce
        })
    }

    fn commit_peer_admission(
        &mut self,
        endpoint: SocketAddrV4,
        name: &str,
        proof: &LanSessionProof,
    ) {
        let record = PeerAdmissionRecord::project(
            self.peer_admissions.remove(&endpoint),
            name.to_owned(),
            PeerAdmission {
                join_nonce: proof.join_nonce,
                admission_nonce: proof.admission_nonce,
            },
        );
        self.peer_admissions.insert(endpoint, record);
        self.pending_admissions
            .retain(|entry| entry.endpoint != endpoint);
    }

    fn replace_peer_admissions(
        &mut self,
        entries: impl IntoIterator<Item = (SocketAddrV4, String, PeerAdmission)>,
    ) {
        let mut previous = std::mem::take(&mut self.peer_admissions);
        self.peer_admissions = entries
            .into_iter()
            .map(|(endpoint, name, identity)| {
                (
                    endpoint,
                    PeerAdmissionRecord::project(previous.remove(&endpoint), name, identity),
                )
            })
            .collect();
    }

    fn send_setup_keepalives(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
    ) -> anyhow::Result<()> {
        let Some(launch) = session.network_setup() else {
            return Ok(());
        };
        let endpoints = if session.host() {
            (0..launch.humans.len())
                .filter(|source| *source != launch.local_player_index)
                .filter_map(|source| session.network_endpoint(source))
                .collect::<Vec<_>>()
        } else {
            self.admitted_host
                .as_ref()
                .map(|(_, endpoint)| *endpoint)
                .into_iter()
                .collect()
        };
        for endpoint in endpoints {
            let mut proof = self.agreed_proof()?;
            if session.host() {
                let Some(peer) = self.peer_admissions.get(&endpoint) else {
                    continue;
                };
                // A host's probe echoes this recipient's confirmed pair. A
                // former same-endpoint join cannot renew its replacement.
                proof.join_nonce = peer.identity.join_nonce;
                proof.admission_nonce = peer.identity.admission_nonce;
            }
            let message =
                LanMessage::named(LanNamedMessage::Keepalive, &launch.local_player().name)?
                    .with_session_proof(&proof)?;
            // Probes are intentionally lossy: the existing global queue owns
            // capacity, and the next presence tick retries without a new FIFO.
            self.send(message.as_bytes(), direct(endpoint), false);
        }
        Ok(())
    }

    /// Direct launcher participants use this lease without a room join.
    /// Keep their epoch and build checks distinct at the lease boundary.
    fn accept_setup_keepalive(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
        incoming: &GlobalMessage,
        message: &LanMessage,
        now: u32,
    ) -> anyhow::Result<SetupKeepalive> {
        let proof = message
            .session_proof()?
            .ok_or_else(|| anyhow::anyhow!("LAN keepalive has no confirmed proof"))?;
        if message.kind() != LanMessageKind::Keepalive
            || self.departure.is_some()
            || !self
                .setup_scope
                .is_some_and(|scope| scope.epoch == proof.scope.epoch)
            || if session.host() {
                !self.peer_admission_matches(incoming.source, &proof)
            } else {
                !self
                    .admitted_host
                    .as_ref()
                    .is_some_and(|(_, endpoint)| *endpoint == incoming.source)
                    || self.join_nonce != Some(proof.join_nonce)
                    || self.admission_nonce != Some(proof.admission_nonce)
            }
        {
            return Ok(SetupKeepalive::Ignored);
        }
        let Some(source) = session.network_endpoint_source(incoming.source) else {
            return Ok(SetupKeepalive::Ignored);
        };
        let Some(launch) = session.network_setup() else {
            return Ok(SetupKeepalive::Ignored);
        };
        let Some(record) = self.peer_admissions.get_mut(&incoming.source) else {
            return Ok(SetupKeepalive::Ignored);
        };
        if source == launch.local_player_index
            || !record.name.eq_ignore_ascii_case(&message.name())
            || !launch.humans[source]
                .name
                .eq_ignore_ascii_case(&record.name)
        {
            return Ok(SetupKeepalive::Ignored);
        }
        // VERA: a direct spawn's participants derive scope and nonces from
        // the launch alone, so one of another build or other rules files
        // gets here. It could never start; name it rather than let its
        // lease run out. A LAN room refused another build at its join.
        if proof.version != super::shell_network_setup::agreement::version() {
            return Ok(if self.preadmitted {
                SetupKeepalive::OtherBuild(record.name.clone())
            } else {
                SetupKeepalive::Ignored
            });
        }
        if self.preadmitted
            && self
                .setup_agreement
                .as_ref()
                .is_some_and(|agreement| agreement.rules_digest != proof.rules_digest)
        {
            return Ok(SetupKeepalive::OtherRules(record.name.clone()));
        }
        if record
            .heartbeat_sequence
            .is_some_and(|last| incoming.sequence.wrapping_sub(last) as i32 <= 0)
        {
            return Ok(SetupKeepalive::Ignored);
        }
        // Global already owns each sender's sequence. Reordered/duplicated
        // probes cannot renew a lease; a new confirmed nonce resets this view.
        record.heartbeat_sequence = Some(incoming.sequence);
        record.lease = Some(super::match_runtime::session::WallBucketTimer::start(
            now,
            SETUP_PEER_LEASE,
        ));
        Ok(SetupKeepalive::Renewed)
    }

    fn expired_setup_peer(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
        now: u32,
    ) -> Option<(usize, SocketAddrV4)> {
        let launch = session.network_setup()?;
        for source in 0..launch.humans.len() {
            if source == launch.local_player_index {
                continue;
            }
            let Some(endpoint) = session.network_endpoint(source) else {
                continue;
            };
            if !session.host()
                && !self
                    .admitted_host
                    .as_ref()
                    .is_some_and(|(_, host)| *host == endpoint)
            {
                continue;
            }
            let Some(record) = self.peer_admissions.get_mut(&endpoint) else {
                continue;
            };
            let lease = record.lease.get_or_insert_with(|| {
                super::match_runtime::session::WallBucketTimer::start(now, SETUP_PEER_LEASE)
            });
            if lease.remaining(now) == 0 {
                return Some((source, endpoint));
            }
        }
        None
    }

    fn retire_setup_peer(&mut self, endpoint: SocketAddrV4) -> bool {
        let host_left = self
            .admitted_host
            .as_ref()
            .is_some_and(|(_, host)| *host == endpoint);
        if host_left {
            // Drop all old setup/map intent before the normal HostLeft slide.
            // MatchSession's existing return owner still performs membership
            // teardown; no subsequent old callback can restart a transfer.
            if let Some(transport) = self.transport.as_mut() {
                for endpoint in self.peer_admissions.keys() {
                    transport.global_mut().cancel_sends_to_endpoint(*endpoint);
                }
            }
            self.clear_setup();
        } else {
            self.peer_admissions.remove(&endpoint);
            self.pending_setup
                .retain(|(_, destination)| *destination != direct(endpoint));
            if let Some(transport) = self.transport.as_mut() {
                transport.global_mut().cancel_sends_to_endpoint(endpoint);
            }
        }
        host_left
    }

    fn accepts_participant(
        &self,
        host: bool,
        endpoint: SocketAddrV4,
        name: &str,
        proof: &LanSessionProof,
    ) -> bool {
        // VERA's host admits only Confirm. Guests assemble the host-approved
        // snapshot through native Type3, never an arbitrary same-epoch sender.
        !host
            && self.in_room(proof)
            && self
                .peer_admissions
                .get(&endpoint)
                .is_some_and(|record| record.name.eq_ignore_ascii_case(name))
            && self.peer_admission_matches(endpoint, proof)
    }

    /// Cancel only this request/challenge. In particular an old zero-challenge
    /// request can retire its proposal, but can never revoke an admitted peer.
    fn cancel_peer_admission(&mut self, endpoint: SocketAddrV4, proof: &LanSessionProof) -> bool {
        self.pending_admissions.retain(|entry| {
            !(entry.endpoint == endpoint
                && entry.identity.join_nonce == proof.join_nonce
                && (proof.admission_nonce == [0; 16]
                    || proof.admission_nonce == entry.identity.admission_nonce))
        });
        let admitted = self.in_room(proof) && self.peer_admission_matches(endpoint, proof);
        if admitted {
            self.peer_admissions.remove(&endpoint);
        }
        admitted
    }

    fn accept_cancel_receipt(&mut self, endpoint: SocketAddrV4, proof: &LanSessionProof) -> bool {
        if !self.correlates_join_reply(endpoint, proof)
            || proof.version != super::shell_network_setup::agreement::version()
            || proof.admission_nonce != self.admission_nonce.unwrap_or([0; 16])
        {
            return false;
        }
        if let Some(wait) = self.departure.as_mut()
            && matches!(wait.reason, DepartureReason::CancelJoin { .. })
        {
            wait.receipt = true;
            return true;
        }
        false
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
        self.loading_drain = None;
        self.maps.cancel();
        self.online_timing = None;
        self.online_restore = false;
        self.setup_scope = None;
        self.setup_agreement = None;
        self.received_options = None;
        self.join_nonce = None;
        self.admission_nonce = None;
        self.pending_admissions.clear();
        self.peer_admissions.clear();
        self.selected_game = None;
        self.pending_host = None;
        self.admitted_host = None;
        self.departure = None;
        self.deferred_setup.clear();
        self.pending_setup.clear();
        // A kick keeps a player out of that room only, as online.
        self.kept_out.clear();
        self.setup_reassembly = Default::default();
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

    /// The selection handlers' calls of `0x005DF9E0`. A click on another row
    /// forces discovery, the game query and presence (the `0x549` handler at
    /// `0x005DDD12`); its change test at `0x005DDCFF` skips the call for the
    /// selected row. `0x005DB9B0` clamping a selection past the shortened list
    /// forces only the query (`0x005DB9FF`).
    fn select_game(&mut self, game_id: Option<u64>, change: LanSelectionChange) {
        self.selected_game = game_id;
        match change {
            LanSelectionChange::Unchanged => {}
            LanSelectionChange::Changed => {
                self.discovery_timer = Default::default();
                self.force_query = true;
                self.force_presence = true;
            }
            LanSelectionChange::Clamped => self.force_query = true,
        }
    }

    /// `0x005DF9E0`'s timers: the broadcasts due at `now`, each restarted.
    fn due_broadcasts(&mut self, now: u32, host_setup: bool) -> DueBroadcasts {
        use super::match_runtime::session::WallBucketTimer;
        // A direct spawn has no lobby to discover or query; it only renews the
        // admitted peers' setup leases.
        let lobby = !self.preadmitted;
        let due = DueBroadcasts {
            discovery: lobby && self.discovery_timer.remaining(now) == 0 && !host_setup,
            query: lobby && (self.force_query || self.query_timer.remaining(now) == 0),
            presence: self.force_presence || self.presence_timer.remaining(now) == 0,
        };
        if due.discovery {
            self.discovery_timer = WallBucketTimer::start(now, 120);
        }
        if due.query {
            self.query_timer = WallBucketTimer::start(now, 35);
            self.force_query = false;
        }
        if due.presence {
            self.presence_timer = WallBucketTimer::start(now, 83);
            self.force_presence = false;
        }
        due
    }
}

#[derive(Debug, PartialEq, Eq)]
struct DueBroadcasts {
    discovery: bool,
    query: bool,
    presence: bool,
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

fn reject_setup_compatibility(state: &mut AppState, message: &str) {
    super::shell_lan::append_chat(
        state,
        crate::ui::lan_shell::LanChatLine {
            text: message.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
    super::shell_network_setup::fail_go_handoff(state);
}

/// Recheck after a received map/seed closure is installed, before status1,
/// Ready or GO. The selected host mode is already admitted by HostSetup.
pub(super) fn verify_setup_agreement(state: &mut AppState) -> anyhow::Result<()> {
    let expected = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.setup_agreement.clone())
        .ok_or_else(|| anyhow::anyhow!("LAN host agreement is not admitted"))?;
    let actual = match super::shell_network_setup::agreement::local(state) {
        Ok(actual) => actual,
        Err(error) => {
            reject_setup_compatibility(state, &crate::i18n::tr("VERA:LanRulesUnavailable"));
            return Err(error);
        }
    };
    if expected != actual {
        // VERA: say what this game changes. The build is the same: the join
        // refuses another, and so does `options::admit_host_room`.
        let message = super::shell_network_setup::agreement::rules_differ_text(state);
        reject_setup_compatibility(state, &message);
        anyhow::bail!("LAN selected-mode agreement mismatch");
    }
    Ok(())
}

fn local_control_proof(state: &mut AppState) -> anyhow::Result<LanSessionProof> {
    verify_setup_agreement(state)?;
    state
        .match_state
        .lan_service
        .as_ref()
        .ok_or_else(closed_visit)?
        .agreed_proof()
}

/// The error for a step that finds no LAN service. Each packet, request and
/// poll step runs while the visit holds one, but a step can close the visit
/// part way (a failed launch, a departure or a kick reaches `close`), so a
/// later step of the same pass ends with this error instead of a panic.
pub(super) fn closed_visit() -> anyhow::Error {
    anyhow::anyhow!("the LAN visit has closed")
}

fn fresh_nonce() -> anyhow::Result<[u8; 16]> {
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce)
        .map_err(|_| anyhow::anyhow!("OS multiplayer nonce generation failed"))?;
    anyhow::ensure!(nonce != [0; 16], "OS multiplayer nonce generation failed");
    Ok(nonce)
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
    send_bytes(state, message.as_bytes(), destination, reliable)
}

fn direct(endpoint: SocketAddrV4) -> GlobalDestination {
    GlobalDestination::Direct {
        endpoint,
        port_override: 0,
    }
}

/// Android: close the lobby's sockets and open new ones on the network the
/// process is bound to now, then show the emptied game and player lists. A
/// failed open reports the ports as unavailable, as entering the lobby does.
#[cfg(target_os = "android")]
fn reopen_lobby_on_new_network(state: &mut AppState) {
    close(state);
    open(state);
    if state.match_state.lan_service.is_some() {
        project_games(state);
        project_players(state);
    }
    log::info!("LAN network changed; announcing on the new Wi-Fi network");
}

pub(crate) fn open(state: &mut AppState) {
    // Every capture owns its service: scripted ones run without sockets, and
    // live ones install theirs before clicking Network (movies/lan/live.rs).
    if state.platform.capture_client_size.is_some() {
        return;
    }
    if online_retirement_pending(state) {
        super::shell_lan::report_error(state, "VERA:WaitingForOnlineDeparture");
        return;
    }
    if state.match_state.lan_service.is_some() {
        return;
    }
    match LanService::open().and_then(|mut service| {
        service.start_io_pump()?;
        Ok(service)
    }) {
        Ok(service) => state.match_state.lan_service = Some(service),
        Err(error) => {
            log::error!("LAN socket initialization failed: {error}");
            report_ports_unavailable(state);
        }
    }
}

/// The LAN visit could not open its sockets, so nothing it shows works.
/// Scripted captures run without sockets on purpose.
pub(crate) fn unavailable(state: &AppState) -> bool {
    state.match_state.lan_service.is_none() && state.platform.capture_client_size.is_none()
}

/// Why the LAN lobby cannot host or join: its ports are taken.
pub(crate) fn report_ports_unavailable(state: &mut AppState) {
    super::shell_lan::report_error(
        state,
        if state.frontend.native_shell_ui {
            "TXT_UNABLE_TO_CONNECT"
        } else {
            "VERA:LanPortsUnavailable"
        },
    );
}

/// Network_ServiceLoop48D080 runs across shell, loading, modal and gameplay
/// waits. Its private receive queue is consumed only by the frame boundary.
pub(crate) fn poll(state: &mut AppState, now_ms: u64) {
    let mode = state
        .match_state
        .session
        .network_return_context()
        .map(|context| context.session_mode());
    if !matches!(mode, Some(SessionMode::Lan | SessionMode::Wol)) {
        return;
    }
    // Android: a Wi-Fi network that appeared, left or changed leaves the
    // visit's sockets on a network that may be gone. Only the lobby list acts
    // on it: a room, loading or a match keeps its transport, and the change
    // waits until the player is back in the lobby.
    #[cfg(target_os = "android")]
    if mode == Some(SessionMode::Lan)
        && state.match_state.live_network.is_none()
        && state.frontend.shell_route == crate::app::shell_route::ShellRoute::LanLobby
        && crate::util::lan_network::take_changed()
    {
        reopen_lobby_on_new_network(state);
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
    if mode == Some(SessionMode::Lan) && cooperative::poll(state, now) {
        return;
    }
    if mode == Some(SessionMode::Wol) {
        // WOL room/configuration events stay with its HTTPS control plane.
        // Loading progress and in-match side channels share the same native
        // service/command owners as LAN, over authenticated relay datagrams.
        loop {
            let incoming = if let Some(live) = state.match_state.live_network.as_mut() {
                live.pop_global_received()
            } else {
                state
                    .match_state
                    .lan_service
                    .as_mut()
                    .and_then(|service| service.transport.as_mut()?.pop_global_received())
            };
            let Some(incoming) = incoming else {
                break;
            };
            if accepts_online_application_packet(
                &incoming.payload,
                state.match_state.live_network.is_some(),
                super::loading::pump::network_loading_active(state),
            ) {
                if let Err(error) = receive(state, incoming, now) {
                    log::debug!("Ignored online match packet: {error:#}");
                }
            }
            service_transport(state, now);
        }
        if state.match_state.lan_service.is_some() {
            drain_match_requests(state);
        }
        return;
    }
    // BB timeout may have queued CancelJoin before this service pass. Apply
    // that existing request FIFO before ingress can complete the same attempt;
    // the endpoint/challenge must survive until its cleanup receipt/drain.
    drain_requests(state, now);
    if advance_departure(state, now, service_ok) {
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
        // gamemd's host reply wait (0x005DD6F8) also records a 0x10 map-file
        // request. VERA guests transfer the map before GO and reply only 0x13.
        let accepted_kind = mode != super::shell_network_setup::LanGoReceiveMode::RepliesOnly
            || incoming
                .payload
                .get(..4)
                .is_some_and(|kind| kind == LanMessageKind::ReadyToGo.raw().to_le_bytes());
        if accepted_kind && let Err(error) = receive(state, incoming, now) {
            log::debug!("Ignored LAN application packet: {error:#}");
        }
        //53FF10 marks only the head consumed;541820/5400D0 retires it.
        let service_ok = service_transport(state, now);
        if advance_departure(state, now, service_ok) {
            return;
        }
        advance_go(state, now, service_ok);
    }
    poll_setup_leases(state, now);
    // Lossy presence probes use the first available global slots before the
    // bulk map window refills them; they add no reliable admission/retry queue.
    announce(state, now);
    maps::poll(state);
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.flush_setup();
    }
    poll_direct_start(state);
    drain_setup_requests(state, now);
    drain_match_requests(state);
    advance_go(state, now, true);
}

fn service_transport(state: &mut AppState, now: u32) -> bool {
    if let Some(live) = state.match_state.live_network.as_mut() {
        live.set_presentation_hidden(state.platform.window_hidden);
        if let Some(service) = state.match_state.lan_service.as_mut() {
            LanService::flush_setup_queue(&mut service.pending_setup, &mut *live.global_mut());
        }
        if let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Network) {
            live.service_pending_commands(&mut runtime.simulation.network_command_queues(), now);
        } else {
            live.service_transport(now);
        }
        true
    } else if let Some(transport) = state
        .match_state
        .lan_service
        .as_mut()
        .and_then(|s| s.transport.as_mut())
    {
        let report = transport.service(now);
        !report.delivery_failed()
    } else {
        true
    }
}

fn advance_go(state: &mut AppState, now: u32, service_ok: bool) {
    let Some(observation) = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.go_transport_observation(service_ok))
    else {
        return;
    };
    if !observation.service_ok
        && super::shell_network_setup::lan_go_receive_mode(state)
            != super::shell_network_setup::LanGoReceiveMode::Normal
    {
        // A retired failed send is not proof that its final fragment or GO
        // arrived. Fail through the existing setup owner instead of loading.
        super::shell_network_setup::fail_go_delivery(state);
        return;
    }
    let queued = observation.queued;
    if queued != 0 {
        // VERA's larger setup can remain outside the native160-slot owner.
        // Do not enter or complete the native GO drain until both are empty.
        return;
    }
    let response = observation.response_time;
    if !super::shell_network_setup::observe_lan_go_transport(
        state,
        now,
        queued,
        observation.service_ok,
    ) {
        return;
    }
    let host = state.match_state.session.host();
    // `observe_lan_go_transport` can fail the setup, which closes the visit.
    let Some(service) = state.match_state.lan_service.as_mut() else {
        return;
    };
    service.apply_start_timing(host, true);
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

/// One derived observation for the existing GO admission owner. No retained state.
struct SetupTransportObservation {
    queued: usize,
    response_time: u32,
    service_ok: bool,
}

fn advance_departure(state: &mut AppState, now: u32, service_ok: bool) -> bool {
    let Some(service) = state.match_state.lan_service.as_mut() else {
        return false;
    };
    let Some(wait) = service.departure.as_ref() else {
        return false;
    };
    let canceling = matches!(wait.reason, DepartureReason::CancelJoin { .. });
    service.flush_setup();
    let Some(reason) = service.departure_ready(now, service_ok) else {
        return !canceling;
    };
    match reason {
        DepartureReason::CancelJoin { game_id } => {
            service.clear_setup();
            super::shell_lan::service_event(
                state,
                super::shell_lan::LanServiceEvent::JoinCanceled { game_id },
            );
        }
        DepartureReason::Back => {
            let dialog = super::shell_network_setup::dialog(state);
            super::App::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellSlideKind::NetworkSetup(dialog),
                crate::app::shell_network_setup::commit_back,
            );
        }
        DepartureReason::Kicked => {
            if let Err(error) = super::shell_network_setup::service_event(
                state,
                super::shell_network_setup::NetworkSetupEvent::Kicked,
            ) {
                log::error!("LAN kick cleanup failed: {error:#}");
            }
        }
    }
    true
}

pub(crate) fn request_setup_back(state: &mut AppState) -> anyhow::Result<()> {
    let now = super::match_runtime::frame_pacer::frame_bucket(
        super::match_runtime::frame_pacer::wall_clock_ms(),
    );
    state
        .match_state
        .lan_service
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("no LAN service"))?
        .begin_departure(&state.match_state.session, DepartureReason::Back, now)?;
    // LAN host Back (BC WndProc005DE520, original5DE733..5DE742) calls
    // Snapshot5ED400 before publishing result2. Preserve that same profile
    // before VERA's asynchronous departure lock makes teardown read-only.
    if state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.editable())
    {
        super::shell_network_setup::remember_setup(state);
    }
    super::shell_network_setup::lock_lan_departure(state);
    Ok(())
}

fn accepts_online_application_packet(payload: &[u8], live: bool, loading: bool) -> bool {
    live || loading::is_progress(payload)
        || (loading
            && payload
                .get(..4)
                .is_some_and(|kind| kind == LanMessageKind::Departure.raw().to_le_bytes()))
}

fn receive(state: &mut AppState, incoming: GlobalMessage, now: u32) -> anyhow::Result<()> {
    anyhow::ensure!(
        incoming.product_id == LAN_PRODUCT_ID,
        "different LAN product"
    );
    if crate::net::lan_messages::extended_setup::campaign::is_control(&incoming.payload) {
        return receive_campaign_control(state, incoming);
    }
    if state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.departure.as_ref())
        .is_some_and(|wait| matches!(wait.reason, DepartureReason::CancelJoin { .. }))
        && incoming
            .payload
            .get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            != Some(LanMessageKind::JoinCanceled.raw())
    {
        return Ok(());
    }
    if incoming.payload.starts_with(maps::MAGIC) {
        anyhow::ensure!(
            state.match_state.session_mode() == SessionMode::Lan,
            "LAN map packet outside a LAN visit"
        );
        return maps::receive(state, incoming.source, &incoming.payload);
    }
    if incoming
        .payload
        .starts_with(crate::net::lan_messages::extended_setup::MAGIC)
    {
        return options::receive_extended(state, incoming.source, &incoming.payload, now);
    }
    if loading::is_progress(&incoming.payload) {
        let scope = state
            .match_state
            .lan_service
            .as_ref()
            .and_then(LanService::setup_scope);
        if let Some((source, percent)) = loading::decode_progress(
            &state.match_state.session,
            scope,
            crate::net::global::GlobalMessageRef {
                source: incoming.source,
                product_id: incoming.product_id,
                sequence: incoming.sequence,
                payload: &incoming.payload,
            },
        )? {
            super::loading::pump::accept_network_loading_progress(state, source, percent);
        }
        return Ok(());
    }
    if incoming.payload.len() == crate::net::lan_messages::ingame::BEACON_LEN
        && matches!(incoming.payload.first(), Some(0x20 | 0x21))
    {
        //48D7AA..48D7EF: no source or session check precedes the manager.
        let message = crate::net::lan_messages::ingame::BeaconMessage::decode(&incoming.payload)?;
        if state.match_state.live_network.is_some() && state.match_state.sim_runtime.is_installed()
        {
            super::match_runtime::beacons::receive(state, message.record());
        }
        return Ok(());
    }
    let message = LanMessage::decode(&incoming.payload)?;
    if super::loading::pump::network_loading_active(state)
        && let Some(departed) = state.match_state.lan_service.as_ref().and_then(|service| {
            service
                .loading_departure_source(&state.match_state.session, incoming.source, &message)
                .ok()
                .flatten()
        })
        && state
            .match_state
            .session
            .retire_network_start_on_departure(departed)
    {
        //Native48D81A->5DA750 has already-bound private Houses. VERA builds
        //those after synchronous loading: cancel this attempt at its safe
        //boundary through the existing loading-failure owner, never install
        //a stale result or invent a partial House/roster mutation.
        super::loading::pump::cancel_network_loading(
            state,
            &crate::i18n::tr("VERA:PlayerLeftWhileLoading"),
        );
        return Ok(());
    }
    if state.match_state.live_network.is_some() {
        if message.kind() == LanMessageKind::Departure {
            let session = &mut state.match_state.session;
            if session.campaign_result().is_some() {
                if let Some(source) = session.network_endpoint_source(incoming.source) {
                    session.disconnect_network_participant(source);
                } else {
                    session.disconnect_network_watcher(incoming.source);
                }
                return Ok(());
            }
            //ProcessGlobalPackets48D81A uses the active private endpoint;
            //the packet name is irrelevant. This is not a synchronized EXIT.
            let matched = &mut state.match_state;
            let destroyed = if let (Some(live), Some(runtime)) = (
                matched.live_network.as_mut(),
                matched.sim_runtime.write_sim(SimWrite::Network),
            ) {
                super::match_runtime::network_match::receive_departure(
                    &mut matched.session,
                    &mut runtime.simulation,
                    live,
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
            let (name, color) = super::match_runtime::network_match::chat_sender(
                &state.match_state.session,
                incoming.source,
                message.name(),
                chat.color as i32,
            );
            super::input::chat::receive_packet(state, &name, color, &chat.text());
            return Ok(());
        }
        if let Some(taunt) = message.in_game_taunt_fields() {
            super::input::taunt::receive(state, taunt.byte);
            return Ok(());
        }
        if let Some(text) = message.beacon_text_fields() {
            //48D7F4..48D810: like 0x20/0x21, no source or session check.
            if state.match_state.sim_runtime.is_installed() {
                super::match_runtime::beacons::receive_text(
                    state,
                    text.house,
                    text.slot,
                    &text.text_units(),
                );
            }
            return Ok(());
        }
        if let Some(runtime) = state.match_state.sim_runtime.runtime()
            && let Some((voter, target)) = super::match_runtime::network_match::decode_kick_proposal(
                &state.match_state.session,
                &runtime.simulation,
                incoming.source,
                &incoming.payload,
            )
        {
            state.match_state.network_wait.queue_vote(voter, target);
        }
        return Ok(());
    }
    if matches!(
        message.kind(),
        LanMessageKind::Options
            | LanMessageKind::PlayerChoices
            | LanMessageKind::ReadyToGo
            | LanMessageKind::StartPrompt
            | LanMessageKind::Go
            | LanMessageKind::Departure
    ) {
        let proof = message
            .session_proof()?
            .ok_or_else(|| anyhow::anyhow!("LAN control has no confirmed peer proof"))?;
        if !state
            .match_state
            .lan_service
            .as_ref()
            .is_some_and(|service| service.peer_admission_matches(incoming.source, &proof))
        {
            return Ok(());
        }
    }
    if let Some(chat) = message.in_game_chat_fields() {
        if let Some(game) = active_game_name(state) {
            anyhow::ensure!(
                chat.game_name_hash == crate::net::lan_messages::game_name_hash(&game),
                "chat belongs to a different game"
            );
        }
        if let Some(line) = chat_line(&local_name(state), &message, &chat) {
            super::shell_lan::append_chat(state, line);
            let sound = state
                .rules()
                .and_then(|rules| rules.general.incoming_message_sound())
                .map(str::to_owned);
            super::App::play_shell_ui_sound_by_id(state, sound.as_deref());
        }
        return Ok(());
    }
    match message.payload() {
        LanPayload::Keepalive => {
            if setup_liveness_active(state) {
                let keepalive = state
                    .match_state
                    .lan_service
                    .as_mut()
                    .ok_or_else(closed_visit)?
                    .accept_setup_keepalive(&state.match_state.session, &incoming, &message, now)?;
                let differs = match keepalive {
                    SetupKeepalive::OtherBuild(name) => Some(format!(
                        "direct participant {name} runs another VERA20k build"
                    )),
                    SetupKeepalive::OtherRules(name) => Some(format!(
                        "direct participant {name}: {}",
                        super::shell_network_setup::agreement::rules_differ_text(state)
                    )),
                    SetupKeepalive::Ignored | SetupKeepalive::Renewed => None,
                };
                if let Some(reason) = differs {
                    // As when its lease runs out, with the reason.
                    state
                        .match_state
                        .lan_service
                        .as_mut()
                        .ok_or_else(closed_visit)?
                        .retire_setup_peer(incoming.source);
                    super::App::fail_external_launch(state, &reason);
                }
            }
        }
        LanPayload::Options(options) => {
            if state.frontend.network_setup_protocol.is_some() {
                let text = options.text;
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
                    let proof = message
                        .session_proof()?
                        .ok_or_else(|| anyhow::anyhow!("LAN acknowledgement has no scope"))?;
                    if value != 0
                        && !state
                            .match_state
                            .lan_service
                            .as_ref()
                            .is_some_and(|service| service.validate_peer_agreement(&proof))
                    {
                        return Ok(());
                    }
                    if value == 0
                        && !state
                            .match_state
                            .lan_service
                            .as_ref()
                            .is_some_and(|service| service.validate_scope(&proof))
                    {
                        return Ok(());
                    }
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
                } else if text.starts_with(b"C0") {
                    // 0x005E43CE: the host's campaign text. Like player
                    // choices it names the session, not one options revision.
                    ensure_host_source(state, incoming.source)?;
                    let proof = message
                        .session_proof()?
                        .ok_or_else(|| anyhow::anyhow!("co-op campaign has no LAN scope"))?;
                    if !state
                        .match_state
                        .lan_service
                        .as_ref()
                        .is_some_and(|service| service.in_room(&proof))
                    {
                        return Ok(());
                    }
                    let text = crate::util::native_string::widen_bytes(text);
                    super::shell_network_setup::service_event(
                        state,
                        super::shell_network_setup::NetworkSetupEvent::CooperativeCampaign(text),
                    )?;
                } else {
                    // VERA sends the full options and roster as VS31 setup.
                    anyhow::bail!("unknown LAN options text");
                }
            }
        }
        LanPayload::PlayerChoices { text } => {
            if state.frontend.network_setup_protocol.is_some() {
                let proof = message
                    .session_proof()?
                    .ok_or_else(|| anyhow::anyhow!("player choices have no LAN scope"))?;
                if !state
                    .match_state
                    .lan_service
                    .as_ref()
                    .is_some_and(|service| service.in_room(&proof))
                {
                    return Ok(());
                }
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
                // VERA binds the actor to the confirmed physical sender. The
                // packet's display name cannot select another guest's row.
                let source = state
                    .match_state
                    .session
                    .network_endpoint_source(incoming.source)
                    .filter(|&source| launch.humans[source].name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| anyhow::anyhow!("player choices sender is not admitted"))?;
                let mut choices =
                    received_choices(values[0], values[1], &launch.humans[source].slot)?;
                choices.start_position = if values[2] < 0 {
                    crate::skirmish_launch::LaunchStartPosition::Auto
                } else {
                    crate::skirmish_launch::LaunchStartPosition::Position(u8::try_from(values[2])?)
                };
                choices.team = crate::skirmish_launch::LaunchTeam::from_shell_value(values[3]);
                super::shell_network_setup::service_event(
                    state,
                    super::shell_network_setup::NetworkSetupEvent::PlayerChoices {
                        index: source,
                        choices,
                        observer: values[0] == -3,
                    },
                )?;
            }
        }
        LanPayload::ReadyToGo => {
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("GO reply has no LAN scope"))?;
            if !state
                .match_state
                .lan_service
                .as_ref()
                .is_some_and(|service| {
                    service.validate_peer_agreement(&proof)
                        && service.setup_maps_verified(
                            super::shell_network_setup::campaign_room::local_map_verified(
                                &state.match_state.session,
                            ),
                        )
                        && service.setup_roster_complete(&state.match_state.session)
                })
            {
                return Ok(());
            }
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
        LanPayload::StartPrompt => {
            if state.frontend.network_setup_protocol.is_some() {
                ensure_host_source(state, incoming.source)?;
                let proof = message
                    .session_proof()?
                    .ok_or_else(|| anyhow::anyhow!("start prompt has no LAN scope"))?;
                if !state
                    .match_state
                    .lan_service
                    .as_ref()
                    .is_some_and(|service| service.in_room(&proof))
                {
                    return Ok(());
                }
                super::shell_network_setup::service_event(
                    state,
                    super::shell_network_setup::NetworkSetupEvent::StartPrompt,
                )?;
            }
        }
        LanPayload::Go { max_ahead } => {
            if state.frontend.network_setup_protocol.is_some() {
                ensure_host_source(state, incoming.source)?;
                let proof = message
                    .session_proof()?
                    .ok_or_else(|| anyhow::anyhow!("GO has no LAN scope"))?;
                if !state
                    .match_state
                    .lan_service
                    .as_ref()
                    .is_some_and(|service| {
                        service.validate_peer_agreement(&proof)
                            && service.setup_maps_verified(
                                super::shell_network_setup::campaign_room::local_map_verified(
                                    &state.match_state.session,
                                ),
                            )
                            && service.setup_roster_complete(&state.match_state.session)
                    })
                {
                    return Ok(());
                }
                verify_setup_agreement(state)?;
                let seed = state
                    .match_state
                    .session
                    .network_start_seed()
                    .ok_or_else(|| anyhow::anyhow!("GO before host options"))?;
                super::shell_network_setup::service_event(
                    state,
                    super::shell_network_setup::NetworkSetupEvent::HostGo {
                        agreed_seed: seed,
                        max_ahead_frames: max_ahead,
                    },
                )?;
            }
        }
        LanPayload::Advertisement { flags } => {
            let local = local_name(state);
            let name = message.name();
            if name.eq_ignore_ascii_case(&local) {
                return Ok(());
            }
            let service = state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(closed_visit)?;
            let open = flags & 1 != 0;
            service.record_advertised_game(incoming.source, name, open, now);
            project_games(state);
        }
        LanPayload::Discovery => {
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
        LanPayload::JoinRequest(request) => {
            offer_join(state, incoming.source, &message, request, now)?
        }
        LanPayload::JoinConfirm => confirm_join(state, incoming.source, &message, now)?,
        LanPayload::CancelJoin => cancel_join(state, incoming.source, &message)?,
        LanPayload::JoinCanceled => {
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("cancel receipt has no proof"))?;
            let service = state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(closed_visit)?;
            service.accept_cancel_receipt(incoming.source, &proof);
        }
        LanPayload::JoinAccepted(_) => {
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("challenge has no proof"))?;
            let name = local_name(state);
            let service = state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(closed_visit)?;
            if !service.accepts_join_reply(incoming.source, &proof) {
                return Ok(());
            }
            anyhow::ensure!(
                proof.version == super::shell_network_setup::agreement::version()
                    && proof.admission_nonce != [0; 16],
                "invalid LAN admission challenge"
            );
            let confirmation = LanMessage::named(LanNamedMessage::JoinConfirm, &name)?
                .with_session_proof(&proof)?;
            anyhow::ensure!(
                service.queue_setup(std::iter::once((
                    confirmation.as_bytes().to_vec(),
                    direct(incoming.source)
                )))?,
                "LAN confirmation queue is full"
            );
            service.setup_scope = Some(proof.scope);
            service.admission_nonce = Some(proof.admission_nonce);
        }
        LanPayload::JoinConfirmed(fields) => {
            // A VERA room's observer may sit on any row.
            let several_observers = super::shell_network_setup::setup_dialog(
                state,
                false,
                crate::ui::multiplayer_setup::SetupFlavor::Lan,
            )
            .modern;
            let service = state
                .match_state
                .lan_service
                .as_mut()
                .ok_or_else(closed_visit)?;
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("LAN acceptance has no session proof"))?;
            if !service.accepts_join_reply(incoming.source, &proof) {
                return Ok(());
            }
            anyhow::ensure!(
                proof.version == super::shell_network_setup::agreement::version(),
                "LAN host build mismatch"
            );
            anyhow::ensure!(
                service.admission_nonce == Some(proof.admission_nonce),
                "unconfirmed LAN admission"
            );
            service.setup_scope = Some(proof.scope);
            let record = PeerAdmissionRecord::project(
                service.peer_admissions.remove(&incoming.source),
                message.name(),
                PeerAdmission {
                    join_nonce: [0; 16],
                    admission_nonce: [0; 16],
                },
            );
            service.peer_admissions.insert(incoming.source, record);
            let mut local = state.frontend.multiplayer_shell_runtime.local_participant();
            local.raw_slot_index = Some(u8::try_from(fields.assigned_row)?);
            local.slot = received_choices(fields.country, fields.color, &local.slot)?;
            local.role = crate::skirmish_launch::LaunchHumanRole::from_observer(
                (several_observers || fields.assigned_row == fields.observer_row)
                    && fields.country == -3,
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
                        observer_row: u8::try_from(fields.observer_row).ok().filter(|row| {
                            usize::from(*row) < crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT
                        }),
                    },
                ),
            );
        }
        LanPayload::ParticipantDetails(fields) => {
            if state.frontend.network_setup_protocol.is_some() {
                accept_participant(state, incoming.source, &message, fields)?;
            } else {
                observe_participant(state, incoming.source, message, now);
            }
        }
        LanPayload::Presence { .. } => observe_participant(state, incoming.source, message, now),
        LanPayload::GameQuery => {
            if state.match_state.session.network_setup().is_some() {
                let game_name = active_game_name(state);
                if let Some(game_name) = game_name
                    .as_deref()
                    .filter(|&game_name| game_name == message.name())
                {
                    let local = state
                        .match_state
                        .session
                        .network_setup()
                        .expect("setup")
                        .local_player();
                    let fields = crate::net::lan_messages::LanParticipantDetails {
                        country: local.slot.country_shell_value(local.is_observer()),
                        color: local.slot.color_shell_value(),
                        game_name_hash: crate::net::lan_messages::game_name_hash(game_name),
                        field_0x53: u32::from(local.raw_slot_index.unwrap_or(0)),
                        field_0x57: u32::from(local.is_observer()),
                    };
                    //5DC170 leaves an empty serial when its registry read fails.
                    let name = local.name.clone();
                    let proof = state
                        .match_state
                        .lan_service
                        .as_ref()
                        .ok_or_else(closed_visit)?
                        .control_proof()?;
                    let response =
                        LanMessage::participant(&name, &[], fields)?.with_session_proof(&proof)?;
                    send(state, &response, direct(incoming.source), true);
                }
            }
        }
        LanPayload::Departure => {
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("departure has no LAN scope"))?;
            if !state
                .match_state
                .lan_service
                .as_ref()
                .is_some_and(|service| service.in_room(&proof))
            {
                return Ok(());
            }
            if let Some(source) = state
                .match_state
                .session
                .network_endpoint_source(incoming.source)
                && state.frontend.network_setup_protocol.is_some()
            {
                state
                    .match_state
                    .lan_service
                    .as_mut()
                    .ok_or_else(closed_visit)?
                    .peer_admissions
                    .remove(&incoming.source);
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
                    if state.match_state.session.host()
                        && let Some(protocol) = state.frontend.network_setup_protocol.as_mut()
                    {
                        protocol.republish_setup();
                    }
                }
            }
        }
        LanPayload::JoinDenied { reason } => {
            if let Some(service) = state.match_state.lan_service.as_ref()
                && service.pending_host == Some(incoming.source)
            {
                let proof = message
                    .session_proof()?
                    .ok_or_else(|| anyhow::anyhow!("LAN denial has no request proof"))?;
                if service.join_nonce != Some(proof.join_nonce) {
                    return Ok(());
                }
                let game_id = endpoint_id(incoming.source);
                super::shell_lan::service_event(
                    state,
                    super::shell_lan::LanServiceEvent::JoinDenied { game_id, reason },
                );
                state
                    .match_state
                    .lan_service
                    .as_mut()
                    .ok_or_else(closed_visit)?
                    .pending_host = None;
            } else if state.frontend.network_setup_protocol.is_some() {
                let proof = message
                    .session_proof()?
                    .ok_or_else(|| anyhow::anyhow!("kick has no LAN scope"))?;
                if !state
                    .match_state
                    .lan_service
                    .as_ref()
                    .is_some_and(|service| service.in_room(&proof))
                {
                    return Ok(());
                }
                let service = state
                    .match_state
                    .lan_service
                    .as_ref()
                    .ok_or_else(closed_visit)?;
                if proof.join_nonce != service.join_nonce.unwrap_or([0; 16])
                    || proof.admission_nonce != service.admission_nonce.unwrap_or([0; 16])
                {
                    return Ok(());
                }
                state
                    .match_state
                    .lan_service
                    .as_mut()
                    .ok_or_else(closed_visit)?
                    .begin_kicked_exit(&state.match_state.session, incoming.source, now)?;
                super::shell_network_setup::lock_lan_departure(state);
            }
        }
        _ => {}
    }
    Ok(())
}

/// A LAN stage continues on the same socket and admitted global scope.
/// Clear private command traffic before loading its successor.
pub(crate) fn hold_transport_for_continuation(state: &mut AppState) {
    if state.match_state.session_mode() != SessionMode::Lan {
        return;
    }
    if let (Some(service), Some(live)) = (
        state.match_state.lan_service.as_mut(),
        state.match_state.live_network.take(),
    ) {
        let mut transport = live.into_transport();
        transport.connections_mut().reset();
        service.transport = Some(transport);
    }
}

pub(crate) fn advance_campaign_transport(
    state: &mut AppState,
    receipts: Vec<GlobalMessage>,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let service = state
        .match_state
        .lan_service
        .as_mut()
        .context("no LAN campaign service")?;
    let scope = service.setup_scope.context("no LAN campaign scope")?;
    let live = state
        .match_state
        .live_network
        .take()
        .context("no LAN campaign transport")?;
    let mut transport = live.into_transport();
    let next = transport.advance_lan_stage_scope(scope, receipts);
    service.transport = Some(transport);
    service.setup_scope = Some(next?);
    service.pending_setup.clear();
    Ok(())
}

pub(crate) mod cooperative;
mod loading;
mod maps;
mod options;
pub(crate) use loading::LoadingNetworkService;

use super::{App, match_runtime, shell_lan, shell_network_setup};

mod gameplay;
mod lobby;
mod online;
mod setup_room;
pub(crate) use gameplay::{install_gameplay, return_transport};
use gameplay::{send_bytes, send_sources};
use lobby::{
    accept_participant, active_game_name, cancel_join, chat_line, chat_packet, confirm_join,
    drain_requests, offer_join, project_games, received_choices,
};
#[cfg(test)]
pub(crate) use online::DirectAdmission;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use online::begin_direct_spawn;
use online::poll_direct_start;
pub(crate) use online::{
    apply_online_timing, begin_online_latency_probe, has_online_relay_route,
    install_online_transport, mark_online_restore, online_peer_round_trips, online_restore,
    renew_online_relay,
};
pub(crate) use setup_room::{admitted_host_room, force_presence, prepare_host_timing};
use setup_room::{
    announce, drain_setup_requests, ensure_host_source, observe_participant, poll_setup_leases,
    project_players, remote_sources, setup_liveness_active,
};

fn campaign_control_scope(
    state: &AppState,
) -> Option<(Option<crate::net::lan_messages::SessionScope>, u32)> {
    let session = &state.match_state.session;
    let attempt = session.campaign_room()?.attempt;
    let scope = match session.network_return_context()?.session_mode() {
        SessionMode::Lan => Some(state.match_state.lan_service.as_ref()?.setup_scope()?),
        SessionMode::Wol if state.match_state.live_network.is_some() => None,
        _ => return None,
    };
    Some((scope, attempt))
}

/// The admitted transport peer, not the role source sorted by the manifest.
pub(crate) fn campaign_host_source(state: &AppState) -> Option<usize> {
    let session = &state.match_state.session;
    if session.host() {
        return super::match_runtime::network_match::local_source(session);
    }
    let (_, endpoint) = state
        .match_state
        .lan_service
        .as_ref()?
        .admitted_host
        .as_ref()?;
    session.network_endpoint_source(*endpoint)
}

pub(crate) enum CampaignControlSend {
    Immediate,
    Queued(crate::net::global::GlobalBatch),
}

pub(crate) fn send_campaign_control(
    state: &mut AppState,
    recipients: &[usize],
    signal: crate::net::lan_messages::extended_setup::campaign::Signal,
) -> Option<CampaignControlSend> {
    use crate::net::lan_messages::extended_setup::campaign::{Record, Signal};
    let (scope, attempt) = campaign_control_scope(state)?;
    let bytes = Record {
        scope,
        attempt,
        signal,
    }
    .encode();
    let endpoints = recipients
        .iter()
        .map(|&source| state.match_state.session.network_endpoint(source))
        .collect::<Option<Vec<_>>>()?;
    if matches!(signal, Signal::StageAbort | Signal::NarrativeAbort { .. }) {
        // Both callers reset the match transport before the next I/O poll.
        // Write these terminal records on the still-admitted route now.
        let sent = if let Some(live) = state.match_state.live_network.as_mut() {
            live.send_terminal_global(&bytes, &endpoints)
        } else if let Some(transport) = state
            .match_state
            .lan_service
            .as_mut()
            .and_then(|service| service.transport.as_mut())
        {
            transport.send_terminal_global(&bytes, &endpoints)
        } else {
            return None;
        };
        if let Err(error) = sent {
            log::warn!("Cannot send campaign abort before leaving the match: {error}");
            return None;
        }
        return Some(CampaignControlSend::Immediate);
    }
    let batch = if let Some(live) = state.match_state.live_network.as_mut() {
        live.global_mut().send_reliable_batch(&bytes, &endpoints)
    } else {
        state
            .match_state
            .lan_service
            .as_mut()?
            .transport
            .as_mut()?
            .global_mut()
            .send_reliable_batch(&bytes, &endpoints)
    }?;
    Some(CampaignControlSend::Queued(batch))
}

fn receive_campaign_control(state: &mut AppState, incoming: GlobalMessage) -> anyhow::Result<()> {
    use crate::net::lan_messages::extended_setup::campaign::{Record, Signal};
    let Some(record) = Record::decode(&incoming.payload) else {
        anyhow::bail!("invalid campaign control record");
    };
    if campaign_control_scope(state) != Some((record.scope, record.attempt)) {
        return Ok(());
    }
    let session = &state.match_state.session;
    let Some(source) = session
        .network_endpoint_source(incoming.source)
        .filter(|source| session.network_connected_source_indices().contains(source))
    else {
        return Ok(());
    };
    match record.signal {
        Signal::NarrativeReady { .. }
        | Signal::NarrativeRelease { .. }
        | Signal::NarrativeAbort { .. } => {
            super::match_runtime::narrative_sync::receive(state, source, record.signal);
        }
        Signal::StageVote(_)
        | Signal::StagePrepare { .. }
        | Signal::StageAccepted(_)
        | Signal::StageBegin(_)
        | Signal::StageBegun(_)
        | Signal::StageAbort => {
            let receipt = (record.scope.is_some()
                && matches!(record.signal, Signal::StageBegun(_)))
            .then_some(incoming);
            super::match_runtime::stage_result::receive(state, source, record.signal, receipt);
        }
    }
    Ok(())
}

fn drain_match_requests(state: &mut AppState) {
    use super::match_runtime::session::NetworkSendRequest;
    if let Some(service) = state.match_state.lan_service.as_mut()
        && let Err(error) = service.drain_loading_reports(&mut state.match_state.session)
    {
        // The held loader fails its load on the same error.
        log::warn!("Loading report failed: {error:#}");
        super::loading::pump::cancel_network_loading(state, &crate::i18n::tr("VERA:LoadingFailed"));
    }
    let requests = state.match_state.session.take_network_send_requests();
    for request in requests {
        match request {
            NetworkSendRequest::ProposeKick { target_source } => {
                if let Some(runtime) = state.match_state.sim_runtime.runtime()
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
            NetworkSendRequest::Chat { recipients, text } => {
                // BC6's chat writes [0x7EEF2C] (0x0046E126).
                let session = &state.match_state.session;
                let color = if session.cooperative_round().is_some() {
                    crate::ui::wol_chat::PUBLIC_COLOR as i32
                } else {
                    super::match_runtime::network_match::local_source(session).map_or(-2, |local| {
                        super::match_runtime::network_match::participant_color(session, local)
                    })
                };
                let game_name = active_game_name(state).unwrap_or_default();
                if let Ok(message) =
                    LanMessage::in_game_chat(&local_name(state), color as u32, &game_name, &text)
                {
                    send_sources(state, message.as_bytes(), recipients);
                }
            }
            NetworkSendRequest::Taunt { recipients, byte } => {
                let session = &state.match_state.session;
                let color = super::match_runtime::network_match::local_source(session)
                    .map_or(-2, |local| {
                        super::match_runtime::network_match::participant_color(session, local)
                    });
                let game_name = active_game_name(state).unwrap_or_default();
                if let Ok(message) =
                    LanMessage::in_game_taunt(&local_name(state), color as u32, &game_name, byte)
                {
                    send_sources(state, message.as_bytes(), recipients);
                }
            }
            NetworkSendRequest::Beacon(record) => {
                if let Ok(message) =
                    crate::net::lan_messages::ingame::BeaconMessage::new(&local_name(state), record)
                {
                    let peers: Vec<_> =
                        super::match_runtime::network_match::nodes(&state.match_state.session)
                            .into_iter()
                            .skip(1)
                            .collect();
                    send_sources(state, message.as_bytes(), peers);
                }
            }
            NetworkSendRequest::BeaconText { house, slot, text } => {
                if let Ok(message) = LanMessage::beacon_text(&local_name(state), house, slot, &text)
                {
                    let peers: Vec<_> =
                        super::match_runtime::network_match::nodes(&state.match_state.session)
                            .into_iter()
                            .skip(1)
                            .collect();
                    send_sources(state, message.as_bytes(), peers);
                }
            }
            // BC6's records; an ordinary Battle match never emits them.
            other => cooperative::send(state, other),
        }
    }
}

pub(crate) fn close(state: &mut AppState) {
    if let Some(service) = state.match_state.lan_service.as_mut() {
        if let Some(transport) = service.transport.as_mut()
            && transport.relay_source().is_some()
        {
            service.online_closing = true;
            transport.retire_relay();
        }
        if service.online_closing {
            // Keep the worker running. A one-shot final service followed by
            // Drop would lose either Leave or its acknowledgement under loss.
            if service
                .transport
                .as_ref()
                .is_some_and(LanUdpTransport::relay_retirement_pending)
            {
                return;
            }
            state.match_state.lan_service = None;
            return;
        }
    }
    // A room, loading screen or match drops an unproven departure, so one
    // sent from there carries this visit's proof and goes to each player too.
    let proof = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.control_proof().ok());
    let packets = departure_packets(&state.match_state.session, proof.as_ref()).or_else(|_| {
        LanMessage::named(LanNamedMessage::Departure, &local_name(state)).map(|message| {
            vec![(
                message.as_bytes().to_vec(),
                GlobalDestination::Broadcast { port_override: 0 },
            )]
        })
    });
    if let Ok(packets) = packets {
        for (bytes, destination) in packets {
            send_bytes(state, &bytes, destination, false);
        }
        if let Some(transport) = state
            .match_state
            .lan_service
            .as_mut()
            .and_then(|service| service.transport.as_mut())
        {
            transport.service_before_close(super::match_runtime::frame_pacer::wall_clock_bucket);
        }
    }
    state.match_state.lan_service = None;
}

/// The local player's departure to each other admitted player and the
/// broadcast address, carrying `proof` when the visit has one.
fn departure_packets(
    session: &super::match_runtime::session::MatchSession,
    proof: Option<&LanSessionProof>,
) -> anyhow::Result<Vec<(Vec<u8>, GlobalDestination)>> {
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("departure requires an admitted setup"))?;
    let mut message = LanMessage::named(LanNamedMessage::Departure, &launch.local_player().name)?;
    if let Some(proof) = proof {
        message = message.with_session_proof(proof)?;
    }
    let mut packets = (0..launch.humans.len())
        .filter(|source| *source != launch.local_player_index)
        .filter_map(|source| session.network_endpoint(source))
        .map(|endpoint| (message.as_bytes().to_vec(), direct(endpoint)))
        .collect::<Vec<_>>();
    packets.push((message.as_bytes().to_vec(), GlobalDestination::BROADCAST));
    Ok(packets)
}

pub(crate) fn online_retirement_pending(state: &AppState) -> bool {
    state
        .match_state
        .lan_service
        .as_ref()
        .is_some_and(|service| {
            service.online_closing
                || service
                    .transport
                    .as_ref()
                    .is_some_and(LanUdpTransport::relay_retirement_pending)
        })
}

/// Resume only a previously requested online close. This remains reachable
/// from Main Menu after the HTTP visit and MatchSession context are retired.
pub(crate) fn poll_online_retirement(state: &mut AppState) {
    let Some(service) = state
        .match_state
        .lan_service
        .as_mut()
        .filter(|service| service.online_closing)
    else {
        return;
    };
    if let Some(transport) = service.transport.as_mut() {
        transport.service(super::match_runtime::frame_pacer::wall_clock_bucket());
        if transport.relay_retirement_pending() {
            return;
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
    let connected = session.network_connected_source_indices();
    anyhow::ensure!(
        connected.contains(&local),
        "gameplay requires the frozen local participant"
    );
    // DestroyConnection (0x005DA750) retires loading sources at 0x005DAA8A.
    // Its historical endpoint remains part of the original admission, but
    // must not recreate a private channel when surviving gameplay installs.
    let peers: Vec<_> = connected
        .iter()
        .copied()
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
    let config = LanConnectionConfig {
        max_peers: crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT - 1,
        ..Default::default()
    };
    let mut connections = LanConnections::new(config, timing);
    for (house, endpoint) in peers {
        connections.add_peer(house, endpoint)?;
    }
    Ok((i8::try_from(local_house)?, seed, connections))
}

#[cfg(test)]
pub(crate) fn loading_test_pair() -> (
    LanService,
    super::match_runtime::session::MatchSession,
    super::match_bootstrap::LoadingStartup,
    LanService,
    super::match_runtime::session::MatchSession,
    super::match_bootstrap::LoadingStartup,
) {
    let mut host = tests::service();
    let mut guest = tests::service();
    let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
    let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
    let mut host_session = tests::session(true, guest_address);
    let mut guest_session = tests::session(false, host_address);
    for (service, peer) in [(&mut host, guest_address), (&mut guest, host_address)] {
        service
            .transport
            .as_mut()
            .unwrap()
            .set_broadcast_targets(&[peer])
            .unwrap();
        service
            .transport
            .as_mut()
            .unwrap()
            .prepare_lan_scope(service.setup_scope.unwrap())
            .unwrap();
    }
    host_session.prepare_network_timing(2, || 0).unwrap();
    guest_session.prepare_network_timing(2, || 0).unwrap();
    let host_startup = host_session.freeze_network_start(77, None).unwrap();
    let guest_startup = guest_session.freeze_network_start(77, None).unwrap();
    (
        host,
        host_session,
        host_startup,
        guest,
        guest_session,
        guest_startup,
    )
}

#[cfg(test)] // shared by test groups
mod tests;
