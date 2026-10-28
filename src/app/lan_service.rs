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
use crate::net::lan_messages::{
    LanCompatibility, LanMessage, LanMessageKind, LanNamedMessage, LanSessionProof, SessionScope,
};
use crate::net::reliable::{Delivery, LanConnectionConfig, LanConnections, RetryTiming};
use crate::net::udp::{DEFAULT_LAN_PORT, LanUdpTransport, UdpBindConfig};

// VERA IPv4 setup lease: 1800 native16-ms wall buckets (28.8s), allowing
// more than21 missed83-bucket announcements. Native5DF9E0 owns the same
// presence cadence;5E1410's nativeType5 probe is not an admitted-peer lease.
const SETUP_PEER_LEASE: u32 = 1800;

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
    // Browser loading suspends the native ACK drain between event-loop turns.
    // The same service retains the queue and deadline; no progress is duplicated.
    #[cfg(any(
        test,
        all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")
    ))]
    loading_drain: Option<crate::sim::timer::CdTimer>,
    transport: Option<LanUdpTransport>,
    // Explicit visit exit, never inferred from silence or the current screen.
    // The same socket owner survives until its terminal relay ACK arrives.
    online_closing: bool,
    online_timing: Option<crate::net::lockstep::WolStartTiming>,
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
    /// A direct spawn launch (docs/spawn.md): the launcher admitted the
    /// roster and the map, so discovery, join/confirm and map transfer never
    /// run; scope, nonces and endpoints come from [`DirectAdmission`].
    preadmitted: bool,
}

impl LanService {
    /// Diagnostic opt-in follows the SAME global owner through loading/live
    /// handoff; callers never borrow the private socket or another queue.
    pub(crate) fn observe_loading(&mut self) {
        if let Some(transport) = self.transport.as_mut() {
            transport.global_mut().observe_loading();
        }
    }

    /// Bounded owned snapshot for receipt generation only. Release the I/O
    /// guard before the capture performs JSON construction or file work.
    pub(crate) fn loading_observation(
        &self,
    ) -> Option<(crate::net::global::LoadingObservationLog, bool)> {
        let global = self.transport.as_ref()?.global();
        Some((
            global.loading_observation()?.clone(),
            global.authenticated_acknowledgments(),
        ))
    }

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
            #[cfg(any(
                test,
                all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")
            ))]
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

    /// Read-only observations for the real process-loss capture. Admission,
    /// countdowns, map work and cancellation remain in their existing owners.
    pub(super) fn setup_peer_lease(&self, endpoint: SocketAddrV4, now: u32) -> Option<(u32, u32)> {
        let record = self.peer_admissions.get(&endpoint)?;
        record.heartbeat_sequence?;
        Some((record.lease?.remaining(now), SETUP_PEER_LEASE))
    }

    pub(super) fn setup_peer_cleanup_pending(&self, endpoint: SocketAddrV4) -> bool {
        self.peer_admissions.contains_key(&endpoint)
            || self
                .pending_admissions
                .iter()
                .any(|entry| entry.endpoint == endpoint)
            || self
                .pending_setup
                .iter()
                .any(|(_, destination)| *destination == direct(endpoint))
            || self.maps.has_peer(endpoint)
    }

    pub(super) fn setup_map_work_active(&self) -> bool {
        self.maps.active()
    }

    /// Current authored-map bytes that have actually arrived but have not yet
    /// reached installation. No progress or admission state is changed.
    pub(super) fn setup_map_download(&self) -> Option<(&str, usize, usize)> {
        self.maps.incomplete_map_download(self.setup_scope?)
    }

    /// Physical source, effective map and generated preview/staging are one
    /// admission through the retained transfer owner, for this exact options.
    fn setup_maps_verified(&self) -> bool {
        // A direct spawn's map is the launcher's file beside spawn.ini; the
        // selected-mode agreement still compares the rules digest at GO.
        self.setup_scope
            .is_some_and(|scope| self.preadmitted || self.maps.current_verified(scope))
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

    fn validate_scope(&self, proof: &LanSessionProof) -> bool {
        self.setup_scope == Some(proof.scope)
            && proof.version == super::shell_network_setup::agreement::version()
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
        let mut proof = self.control_proof()?;
        if let Some(agreement) = self.setup_agreement.as_ref() {
            proof.rules_digest = agreement.rules_digest.clone();
        }
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

    fn queue_departure(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
        _now: u32,
    ) -> anyhow::Result<bool> {
        let launch = session
            .network_setup()
            .ok_or_else(|| anyhow::anyhow!("departure requires an admitted setup"))?;
        let message = LanMessage::named(LanNamedMessage::Departure, &launch.local_player().name)?
            .with_session_proof(&self.control_proof()?)?;
        // Native5E09CD..5E0AC6 services/drains reliable direct Type10 before
        // returning. VERA retains the entire bounded batch through the same
        // setup FIFO when the larger roster/map stream fills global160.
        let mut packets = (0..launch.humans.len())
            .filter(|source| *source != launch.local_player_index)
            .filter_map(|source| session.network_endpoint(source))
            .map(|endpoint| (message.as_bytes().to_vec(), direct(endpoint)))
            .collect::<Vec<_>>();
        packets.push((message.as_bytes().to_vec(), GlobalDestination::BROADCAST));
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
            identity,
            deadline: super::match_runtime::session::WallBucketTimer::start(now, 600),
        });
        Ok(identity)
    }

    fn confirmed_request(
        &mut self,
        endpoint: SocketAddrV4,
        proof: &LanSessionProof,
        now: u32,
    ) -> Option<LanMessage> {
        self.pending_admissions
            .retain(|entry| entry.deadline.remaining(now) != 0);
        if !self
            .setup_scope
            .is_some_and(|scope| scope.epoch == proof.scope.epoch)
        {
            return None;
        }
        self.pending_admissions
            .iter()
            .find(|entry| {
                entry.endpoint == endpoint
                    && entry.identity.join_nonce == proof.join_nonce
                    && entry.identity.admission_nonce == proof.admission_nonce
            })
            .map(|entry| entry.request.clone())
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
            let mut proof = self.control_proof()?;
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

    fn accept_setup_keepalive(
        &mut self,
        session: &super::match_runtime::session::MatchSession,
        incoming: &GlobalMessage,
        message: &LanMessage,
        now: u32,
    ) -> anyhow::Result<bool> {
        let proof = message
            .session_proof()?
            .ok_or_else(|| anyhow::anyhow!("LAN keepalive has no confirmed proof"))?;
        if message.kind() != LanMessageKind::Keepalive
            || self.departure.is_some()
            || !self
                .setup_scope
                .is_some_and(|scope| scope.epoch == proof.scope.epoch)
            || proof.version != super::shell_network_setup::agreement::version()
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
            return Ok(false);
        }
        let Some(source) = session.network_endpoint_source(incoming.source) else {
            return Ok(false);
        };
        let Some(launch) = session.network_setup() else {
            return Ok(false);
        };
        let Some(record) = self.peer_admissions.get_mut(&incoming.source) else {
            return Ok(false);
        };
        if source == launch.local_player_index
            || !record.name.eq_ignore_ascii_case(&message.name())
            || !launch.humans[source]
                .name
                .eq_ignore_ascii_case(&record.name)
            || record
                .heartbeat_sequence
                .is_some_and(|last| incoming.sequence.wrapping_sub(last) as i32 <= 0)
        {
            return Ok(false);
        }
        // Global already owns each sender's sequence. Reordered/duplicated
        // probes cannot renew a lease; a new confirmed nonce resets this view.
        record.heartbeat_sequence = Some(incoming.sequence);
        record.lease = Some(super::match_runtime::session::WallBucketTimer::start(
            now,
            SETUP_PEER_LEASE,
        ));
        Ok(true)
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
            && self
                .setup_scope
                .is_some_and(|scope| scope.epoch == proof.scope.epoch)
            && proof.version == super::shell_network_setup::agreement::version()
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
        let admitted = self
            .setup_scope
            .is_some_and(|scope| scope.epoch == proof.scope.epoch)
            && self.peer_admission_matches(endpoint, proof);
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
        #[cfg(any(
            test,
            all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")
        ))]
        {
            self.loading_drain = None;
        }
        self.maps.cancel();
        self.online_timing = None;
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
            reject_setup_compatibility(
                state,
                "The selected multiplayer retail rules or generated-map inputs are unavailable.",
            );
            return Err(error);
        }
    };
    if expected != actual {
        reject_setup_compatibility(
            state,
            "VERA20k build, gameplay features or selected-mode rules differ between LAN clients.",
        );
        anyhow::bail!("LAN selected-mode agreement mismatch");
    }
    Ok(())
}

fn local_control_proof(state: &mut AppState) -> anyhow::Result<LanSessionProof> {
    verify_setup_agreement(state)?;
    let service = state.match_state.lan_service.as_ref().expect("visit");
    let mut proof = service.control_proof()?;
    proof.rules_digest = service
        .setup_agreement
        .as_ref()
        .expect("verified agreement")
        .rules_digest
        .clone();
    Ok(proof)
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
    if online_retirement_pending(state) {
        super::shell_lan::report_error(
            state,
            "Waiting to confirm departure from the previous online match.",
        );
        return;
    }
    if state.match_state.lan_service.is_some() {
        return;
    }
    let now = super::match_runtime::frame_pacer::frame_bucket(
        super::match_runtime::frame_pacer::wall_clock_ms(),
    );
    match LanService::open(now).and_then(|mut service| {
        service.start_io_pump()?;
        Ok(service)
    }) {
        Ok(service) => state.match_state.lan_service = Some(service),
        Err(error) => {
            log::error!("LAN socket initialization failed: {error}");
            super::shell_lan::report_error(state, "TXT_UNABLE_TO_CONNECT");
        }
    }
}

/// The online control plane admits identity and the immutable roster. Once
/// its UDP probe has completed, transfer that same socket/session owner to
/// the shared loading/gameplay service. No synthetic connection succeeds here.
pub(crate) fn install_online_transport(
    state: &mut AppState,
    transport: LanUdpTransport,
    host_name: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !online_retirement_pending(state),
        "Waiting to confirm departure from the previous online match."
    );
    anyhow::ensure!(
        state.match_state.live_network.is_none(),
        "a match already owns its transport"
    );
    anyhow::ensure!(
        state.match_state.session.network_return_context()
            == Some(super::match_runtime::session::NetworkReturnContext::Wol),
        "online transport requires an online visit"
    );
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("online transport requires an admitted roster"))?;
    anyhow::ensure!(
        transport.relay_ready(),
        "relay has not completed its UDP probe"
    );
    anyhow::ensure!(
        transport.relay_source().map(usize::from) == Some(launch.local_player_index),
        "relay identity differs from the admitted local participant"
    );
    for source in 0..launch.humans.len() {
        let endpoint = crate::net::udp::relay_peer_endpoint(u8::try_from(source)?)?;
        anyhow::ensure!(
            state.match_state.session.network_endpoint(source) == Some(endpoint),
            "online participant {source} has no matching relay identity"
        );
    }
    let host_source = launch
        .humans
        .iter()
        .position(|human| human.name == host_name)
        .ok_or_else(|| anyhow::anyhow!("online host is outside the admitted roster"))?;
    let host_endpoint = state
        .match_state
        .session
        .network_endpoint(host_source)
        .ok_or_else(|| anyhow::anyhow!("online host has no admitted endpoint"))?;
    let mut service = LanService::with_transport(transport);
    // The setup dialog is discarded before loading. Retain the same admitted
    // host binding as LAN so timing authority survives that handoff.
    service.admitted_host = Some((host_name.to_owned(), host_endpoint));
    state.match_state.lan_service = Some(service);
    Ok(())
}

/// The admission every participant of a direct spawn derives from the same
/// launch (docs/spawn.md), in place of discovery and join/confirm: a shared
/// setup scope, one join/admission nonce pair per source, and the bound
/// remote sources. Only the local index differs between participants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectAdmission {
    scope: SessionScope,
    /// The local source's pair, as carried by its control proofs.
    local: PeerAdmission,
    /// Source 0's name and endpoint; `None` at the host itself.
    host: Option<(String, SocketAddrV4)>,
    /// Every remote source: endpoint, name and that source's nonce pair.
    peers: Vec<(SocketAddrV4, String, PeerAdmission)>,
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
        // option or roster can never enter another participant's scope.
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
    fn adopt_direct_admission(&mut self, admission: &DirectAdmission) {
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
    fn direct_peers_heard(&self) -> bool {
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
    state: &mut AppState,
    launch: crate::skirmish_launch::SkirmishLaunchSession,
    direct: crate::app::frontend::cncnet_spawn::DirectSpawn,
    seed: u32,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.match_state.lan_service.is_none() && state.match_state.live_network.is_none(),
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
        state,
        launch,
        super::match_runtime::session::NetworkReturnContext::LanLobby,
    )?;
    for (name, peer) in names.iter().zip(&direct.peers) {
        if let Some(endpoint) = peer {
            state
                .match_state
                .session
                .bind_network_endpoint(name, *endpoint)?;
        }
    }
    state.match_state.session.set_network_start_seed(seed)?;
    let mut service = LanService::open_direct(direct.local_port, &admission).map_err(|error| {
        anyhow::anyhow!("could not open UDP port {}: {error}", direct.local_port)
    })?;
    service.start_io_pump()?;
    state.match_state.lan_service = Some(service);
    let agreement = super::shell_network_setup::agreement::local(state)?;
    state
        .match_state
        .lan_service
        .as_mut()
        .expect("installed")
        .setup_agreement = Some(agreement);
    Ok(())
}

/// The direct spawn host starts the ordinary LAN GO once every guest is up.
fn poll_direct_start(state: &mut AppState) {
    if !state.match_state.session.host()
        || !super::shell_network_setup::external(state)
        || !state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| protocol.editable())
        || !state
            .match_state
            .lan_service
            .as_ref()
            .is_some_and(LanService::direct_peers_heard)
    {
        return;
    }
    if let Err(error) = super::shell_network_setup::begin_external_lan_go(state) {
        super::App::fail_external_launch(state, &format!("direct start: {error:#}"));
    }
}

pub(crate) fn begin_online_latency_probe(state: &mut AppState) -> anyhow::Result<()> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("latency probe requires an admitted roster"))?;
    let sources = (0..launch.humans.len())
        .filter(|source| *source != launch.local_player_index)
        .map(u8::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    state
        .match_state
        .lan_service
        .as_mut()
        .and_then(|service| service.transport.as_mut())
        .ok_or_else(|| anyhow::anyhow!("latency probe has no transport"))?
        .begin_relay_peer_probes(&sources)?;
    Ok(())
}

pub(crate) fn online_peer_round_trips(state: &AppState) -> Option<Vec<(u8, u32)>> {
    state
        .match_state
        .lan_service
        .as_ref()?
        .transport
        .as_ref()?
        .relay_peer_round_trips()
}

pub(crate) fn online_relay_expires(state: &AppState) -> Option<u64> {
    if let Some(live) = state.match_state.live_network.as_ref() {
        return live.relay_expires();
    }
    state
        .match_state
        .lan_service
        .as_ref()?
        .transport
        .as_ref()?
        .relay_expires()
}

pub(crate) fn renew_online_relay(
    state: &mut AppState,
    address: SocketAddrV4,
    ticket: &[u8],
    key: [u8; 32],
    payload_key: [u8; 32],
) -> anyhow::Result<()> {
    if let Some(live) = state.match_state.live_network.as_mut() {
        return Ok(live.renew_relay(address, ticket, key, payload_key)?);
    }
    let transport = state
        .match_state
        .lan_service
        .as_mut()
        .and_then(|service| service.transport.as_mut())
        .ok_or_else(|| anyhow::anyhow!("online renewal has no admitted route"))?;
    Ok(transport.renew_relay(address, ticket, key, payload_key)?)
}

/// Apply the same agreed timing vector on every client before freezing the
/// start. Native794F6F's outputs stay with their existing session/transport
/// owners; construction of private channels later inherits this retry policy.
pub(crate) fn apply_online_timing(state: &mut AppState, pings: &[i32]) -> anyhow::Result<()> {
    let count = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("online timing has no roster"))?
        .humans
        .len();
    anyhow::ensure!(
        pings.len() == count && pings.iter().all(|ping| (0..=30000).contains(ping)),
        "online timing vector does not match the admitted roster"
    );
    let host = state.match_state.session.host();
    let timing = state
        .match_state
        .session
        .prepare_wol_timing(pings.iter().copied(), host)?;
    let service = state
        .match_state
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
        let accepted_kind = mode != super::shell_network_setup::LanGoReceiveMode::RepliesOnly
            || matches!(incoming.payload.first(), Some(0x13 | 0x10));
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
    drain_setup_requests(state);
    drain_match_requests(state);
    advance_go(state, now, true);
}

fn accepts_online_application_packet(payload: &[u8], live: bool, loading: bool) -> bool {
    live || loading::is_progress(payload)
        || (loading
            && payload
                .get(..4)
                .is_some_and(|kind| kind == LanMessageKind::Departure.raw().to_le_bytes()))
}

fn receive(state: &mut AppState, incoming: GlobalMessage, now: u32) -> anyhow::Result<()> {
    anyhow::ensure!(incoming.product_id == 0xaa04, "different LAN product");
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
            let applied =
                super::loading::pump::accept_network_loading_progress(state, source, percent);
            if let Some(displayed) =
                super::loading::pump::network_loading_displayed_percent(state, source)
            {
                let global = if let Some(live) = state.match_state.live_network.as_mut() {
                    Some(live.global_mut())
                } else {
                    state
                        .match_state
                        .lan_service
                        .as_mut()
                        .and_then(|service| service.transport.as_mut())
                        .map(|transport| transport.global_mut())
                };
                if let Some(mut global) = global {
                    global.observe_loading_application(
                        source,
                        incoming.sequence,
                        percent,
                        displayed,
                        applied,
                    );
                }
            }
        }
        return Ok(());
    }
    if incoming.payload.len() == crate::net::lan_messages::ingame::BEACON_LEN
        && matches!(incoming.payload.first(), Some(0x20 | 0x21))
    {
        //48D7AA..48D7EF: no source or session check precedes the manager.
        let message = crate::net::lan_messages::ingame::BeaconMessage::decode(&incoming.payload)?;
        if state.match_state.live_network.is_some() && state.match_state.sim_runtime.is_some() {
            super::match_runtime::beacons::receive(state, message.record());
        }
        return Ok(());
    }
    let message = LanMessage::decode(&incoming.payload)?;
    if super::loading::pump::network_loading_active(state)
        && state
            .match_state
            .lan_service
            .as_ref()
            .is_some_and(|service| {
                service
                    .loading_departure_source(&state.match_state.session, incoming.source, &message)
                    .is_ok_and(|source| source.is_some())
            })
    {
        //Native48D81A->5DA750 has already-bound private Houses. VERA builds
        //those after synchronous loading: cancel this attempt at its safe
        //boundary through the existing loading-failure owner, never install
        //a stale result or invent a partial House/roster mutation.
        let source = state
            .match_state
            .session
            .network_endpoint_source(incoming.source);
        let match_id = super::shell_wol::online_match_id(state).map(str::to_owned);
        state.match_state.match_diagnostics.observe_loading_cancel(
            "authenticated_departure",
            match_id,
            source,
            false,
            true,
        );
        super::loading::pump::cancel_network_loading(
            state,
            "A player left while the map was loading.",
        );
        return Ok(());
    }
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
        LanMessageKind::Keepalive => {
            if setup_liveness_active(state) {
                state
                    .match_state
                    .lan_service
                    .as_mut()
                    .expect("visit")
                    .accept_setup_keepalive(&state.match_state.session, &incoming, &message, now)?;
            }
        }
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
                        .and_then(|service| service.setup_scope())
                        .is_some_and(|scope| scope.epoch == proof.scope.epoch)
                        || proof.version != super::shell_network_setup::agreement::version()
                    {
                        return Ok(());
                    }
                    let text = crate::util::native_string::widen_bytes(text);
                    super::shell_network_setup::service_event(
                        state,
                        super::shell_network_setup::NetworkSetupEvent::CooperativeCampaign(text),
                    )?;
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
                let proof = message
                    .session_proof()?
                    .ok_or_else(|| anyhow::anyhow!("player choices have no LAN scope"))?;
                if !state
                    .match_state
                    .lan_service
                    .as_ref()
                    .and_then(|service| service.setup_scope())
                    .is_some_and(|scope| scope.epoch == proof.scope.epoch)
                    || proof.version != super::shell_network_setup::agreement::version()
                {
                    return Ok(());
                }
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
                let change = state
                    .match_state
                    .session
                    .accept_lan_peer_choices(source, choices)?;
                if change.options && !options::publish_options(state)? {
                    state
                        .match_state
                        .lan_service
                        .as_mut()
                        .expect("visit")
                        .deferred_setup
                        .push_back(super::shell_network_setup::NetworkSetupRequest::PublishOptions);
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
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("GO reply has no LAN scope"))?;
            if !state
                .match_state
                .lan_service
                .as_ref()
                .is_some_and(|service| {
                    service.validate_peer_agreement(&proof)
                        && service.setup_maps_verified()
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
        LanMessageKind::StartPrompt => {
            if state.frontend.network_setup_protocol.is_some() {
                ensure_host_source(state, incoming.source)?;
                let proof = message
                    .session_proof()?
                    .ok_or_else(|| anyhow::anyhow!("start prompt has no LAN scope"))?;
                if !state
                    .match_state
                    .lan_service
                    .as_ref()
                    .and_then(|service| service.setup_scope())
                    .is_some_and(|scope| scope.epoch == proof.scope.epoch)
                {
                    return Ok(());
                }
                super::shell_network_setup::service_event(
                    state,
                    super::shell_network_setup::NetworkSetupEvent::StartPrompt,
                )?;
            }
        }
        LanMessageKind::Go => {
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
                            && service.setup_maps_verified()
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
        LanMessageKind::JoinRequest => offer_join(state, incoming.source, &message, now)?,
        LanMessageKind::JoinConfirm => confirm_join(state, incoming.source, &message, now)?,
        LanMessageKind::CancelJoin => cancel_join(state, incoming.source, &message)?,
        LanMessageKind::JoinCanceled => {
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("cancel receipt has no proof"))?;
            let service = state.match_state.lan_service.as_mut().expect("visit");
            service.accept_cancel_receipt(incoming.source, &proof);
        }
        LanMessageKind::JoinAccepted => {
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("challenge has no proof"))?;
            let name = local_name(state);
            let service = state.match_state.lan_service.as_mut().expect("visit");
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
        LanMessageKind::JoinConfirmed => {
            let service = state.match_state.lan_service.as_mut().expect("visit");
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
                        observer_row: u8::try_from(fields.observer_row).ok().filter(|row| {
                            usize::from(*row) < crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT
                        }),
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
                    let name = local.name.clone();
                    let proof = state
                        .match_state
                        .lan_service
                        .as_ref()
                        .expect("visit")
                        .control_proof()?;
                    let response =
                        LanMessage::participant(&name, &[], fields)?.with_session_proof(&proof)?;
                    send(state, &response, direct(incoming.source), true);
                }
            }
        }
        LanMessageKind::Departure => {
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("departure has no LAN scope"))?;
            if !state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.setup_scope())
                .is_some_and(|scope| scope.epoch == proof.scope.epoch)
            {
                return Ok(());
            }
            if !state
                .match_state
                .lan_service
                .as_ref()
                .is_some_and(|service| service.peer_admission_matches(incoming.source, &proof))
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
                    .expect("visit")
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
        LanMessageKind::JoinDenied => {
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
                let proof = message
                    .session_proof()?
                    .ok_or_else(|| anyhow::anyhow!("kick has no LAN scope"))?;
                if !state
                    .match_state
                    .lan_service
                    .as_ref()
                    .and_then(|service| service.setup_scope())
                    .is_some_and(|scope| scope.epoch == proof.scope.epoch)
                {
                    return Ok(());
                }
                let service = state.match_state.lan_service.as_ref().expect("visit");
                if proof.join_nonce != service.join_nonce.unwrap_or([0; 16])
                    || proof.admission_nonce != service.admission_nonce.unwrap_or([0; 16])
                {
                    return Ok(());
                }
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
            let service = state.match_state.lan_service.as_mut().expect("visit");
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
            let service = state.match_state.lan_service.as_mut().expect("visit");
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
    let proof = message
        .session_proof()?
        .ok_or_else(|| anyhow::anyhow!("participant has no VERA admission"))?;
    let name = message.name();
    anyhow::ensure!(
        state
            .match_state
            .lan_service
            .as_ref()
            .expect("visit")
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
        .expect("visit")
        .commit_peer_admission(endpoint, &name, &proof);
    // The host snapshot can precede Type3. Apply its existing parser/HostSetup
    // after this binding so country/start/team cannot remain Type3 defaults.
    options::reapply_received_options(state)?;
    Ok(())
}

fn join_denial(
    state: &AppState,
    endpoint: SocketAddrV4,
    message: &LanMessage,
    proof: &LanSessionProof,
) -> Option<u32> {
    let request = message.join_fields().expect("join request");
    let same_peer = state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| {
            launch.humans.iter().enumerate().any(|(index, human)| {
                index != launch.local_player_index
                    && human.name.eq_ignore_ascii_case(&message.name())
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
                    .any(|human| human.name.eq_ignore_ascii_case(&message.name()))
            })
    {
        Some(0)
    } else if !same_peer
        && state
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
    }
}

fn send_join_denial(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    reason: u32,
    proof: &LanSessionProof,
) -> anyhow::Result<()> {
    let service = state.match_state.lan_service.as_mut().expect("visit");
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
fn offer_join(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: &LanMessage,
    now: u32,
) -> anyhow::Result<()> {
    let proof = message
        .session_proof()?
        .ok_or_else(|| anyhow::anyhow!("LAN join has no VERA build identity"))?;
    anyhow::ensure!(proof.join_nonce != [0; 16], "LAN join has no request nonce");
    if let Some(reason) = join_denial(state, endpoint, message, &proof) {
        return send_join_denial(state, endpoint, reason, &proof);
    }
    let current = state
        .match_state
        .lan_service
        .as_ref()
        .expect("visit")
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
                .expect("visit")
                .queue_setup(std::iter::once((
                    confirmed.as_bytes().to_vec(),
                    direct(endpoint)
                )))?,
            "LAN final acceptance queue is full"
        );
        return Ok(());
    }
    let identity = {
        let service = state.match_state.lan_service.as_mut().expect("visit");
        service.ensure_host_scope()?;
        match service.join_offer(endpoint, message, now) {
            Ok(identity) => identity,
            Err(_) => return send_join_denial(state, endpoint, 5, &proof),
        }
    };
    let request = message.join_fields().expect("join request");
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
        .expect("visit")
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
            .expect("visit")
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
        country: country_value(human),
        color: color_value(human),
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
        .expect("visit")
        .control_proof()?;
    proof.join_nonce = identity.join_nonce;
    proof.admission_nonce = identity.admission_nonce;
    Ok(
        LanMessage::join_confirmed(&launch.local_player().name, acceptance)?
            .with_session_proof(&proof)?,
    )
}

fn confirm_join(
    state: &mut AppState,
    endpoint: SocketAddrV4,
    message: &LanMessage,
    now: u32,
) -> anyhow::Result<()> {
    let proof = message
        .session_proof()?
        .ok_or_else(|| anyhow::anyhow!("LAN confirmation has no proof"))?;
    if proof.version != super::shell_network_setup::agreement::version()
        || !state
            .match_state
            .lan_service
            .as_ref()
            .and_then(|service| service.setup_scope())
            .is_some_and(|scope| scope.epoch == proof.scope.epoch)
    {
        return Ok(());
    }
    let duplicate = state
        .match_state
        .lan_service
        .as_ref()
        .expect("visit")
        .peer_admission_matches(endpoint, &proof);
    let request = state
        .match_state
        .lan_service
        .as_mut()
        .expect("visit")
        .confirmed_request(endpoint, &proof, now);
    if let Some(request) = request {
        if let Some(reason) = join_denial(state, endpoint, &request, &proof) {
            return send_join_denial(state, endpoint, reason, &proof);
        }
        // Reserve the final reply before changing Session membership. UI-side
        // join effects append semantic requests, not transport packets.
        anyhow::ensure!(
            state
                .match_state
                .lan_service
                .as_ref()
                .expect("visit")
                .pending_setup
                .len()
                < 4096,
            "LAN final admission is backpressured"
        );
        let name = request.name();
        let same_peer = state
            .match_state
            .session
            .network_endpoint_source(endpoint)
            .is_some();
        if !same_peer {
            let request = request.join_fields().expect("confirmed request");
            let row = state
                .match_state
                .session
                .network_free_row(request.observer_request == 1)
                .expect("validated row");
            let observer = request.observer_request == 1;
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
            .expect("visit")
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
            .expect("visit")
            .queue_setup(std::iter::once((
                confirmed.as_bytes().to_vec(),
                direct(endpoint)
            )))?,
        "reserved LAN acceptance must fit"
    );
    Ok(())
}

fn cancel_join(
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
    let service = state.match_state.lan_service.as_mut().expect("visit");
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

/// Session697B70 binds source nodes to the actual constructed House table.
/// The same physical transport used for discovery/loading enters the command
/// owner; no private peer is inferred from a colour or UI row.
pub(crate) fn install_gameplay(state: &mut AppState) -> anyhow::Result<()> {
    if !matches!(
        state.match_state.session_mode(),
        SessionMode::Lan | SessionMode::Wol
    ) || (state.platform.capture_client_size.is_some()
        && state.match_state.lan_service.is_none())
    {
        return Ok(());
    }
    if state.match_state.session_mode() == SessionMode::Wol {
        anyhow::ensure!(
            state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.transport.as_ref())
                .is_some_and(LanUdpTransport::relay_ready),
            "online startup requires a connected relay"
        );
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
    let host_house = if state.match_state.session.host() {
        local_house
    } else {
        let host_name = state
            .match_state
            .lan_service
            .as_ref()
            .and_then(|service| service.admitted_host.as_ref().map(|host| host.0.as_str()))
            .ok_or_else(|| anyhow::anyhow!("gameplay has no admitted host identity"))?;
        let source = state
            .match_state
            .session
            .network_setup()
            .and_then(|launch| {
                launch
                    .humans
                    .iter()
                    .position(|human| human.name.eq_ignore_ascii_case(host_name))
            })
            .ok_or_else(|| anyhow::anyhow!("host is absent from the admitted roster"))?;
        i8::try_from(
            super::match_runtime::network_match::source_house(
                &state.match_state.session,
                sim,
                source,
            )
            .ok_or_else(|| anyhow::anyhow!("host has no constructed House"))?,
        )
        .map_err(|_| anyhow::anyhow!("host House index cannot be encoded by the command channel"))?
    };
    // The loading wait may already have retired a source684596. Initialize
    // timing from MatchSession's accepted connected roster before the first
    // shared frame; private transport cleanup is drained by its existing owner.
    let source_houses = state
        .match_state
        .session
        .network_connected_source_indices()
        .iter()
        .map(|&source| {
            let house = super::match_runtime::network_match::source_house(
                &state.match_state.session,
                sim,
                source,
            )
            .ok_or_else(|| anyhow::anyhow!("connected source has no constructed House"))?;
            i8::try_from(house)
                .map_err(|_| anyhow::anyhow!("connected House index cannot be encoded"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let full_houses = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("gameplay has no frozen original admission"))?
        .humans
        .iter()
        .enumerate()
        .map(|(source, human)| {
            let row = human
                .raw_slot_index
                .ok_or_else(|| anyhow::anyhow!("admitted human has no shared setup row"))?;
            let house = super::match_runtime::network_match::source_house(
                &state.match_state.session,
                sim,
                source,
            )
            .ok_or_else(|| anyhow::anyhow!("admitted source has no constructed House"))?;
            Ok((
                row,
                i8::try_from(house)
                    .map_err(|_| anyhow::anyhow!("original House cannot be encoded"))?,
            ))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let active_rows = state
        .match_state
        .session
        .network_connected_source_indices()
        .iter()
        .map(|&source| {
            state
                .match_state
                .session
                .network_setup()
                .and_then(|launch| launch.humans.get(source))
                .and_then(|human| human.raw_slot_index)
                .ok_or_else(|| anyhow::anyhow!("active human has no shared setup row"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let online_timing = if state.match_state.session_mode() == SessionMode::Wol {
        Some(
            state
                .match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.online_timing)
                .ok_or_else(|| anyhow::anyhow!("online startup has no agreed measured timing"))?,
        )
    } else {
        None
    };
    // Freeze shared setup-row/House bindings and loading-survivor set exactly
    // once. The existing canonical world hash includes the installed shared
    // timing rows; no first probe, vote or local clock can replace this value.
    let ordering_clock = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.transport.as_ref())
        .and_then(|transport| transport.ordering_certificate())
        .map(|bytes| crate::net::udp::OrderingCertificate::parse(&bytes))
        .transpose()?
        .filter(|cert| cert.sources_mask.count_ones() > 1);
    let frame_send_rate = ordering_clock.map_or_else(
        || online_timing.map_or(5, |timing| timing.frame_send_rate as u32),
        |_| 1,
    );
    let rate =
        std::num::NonZeroU32::new(frame_send_rate).expect("native multiplayer FrameSendRate");
    let sim = &mut state
        .match_state
        .sim_runtime
        .as_mut()
        .expect("constructed world")
        .simulation;
    sim.network_command_queues().install_timing(
        host_house,
        std::num::NonZeroU8::new(frame_send_rate as u8).expect("native multiplayer send rate"),
        source_houses,
    );
    if let Some(clock) = ordering_clock {
        sim.network_command_queues()
            .configure_sequenced_clock(clock.tick_hz);
    }
    let publisher = sim.network_timing_host_house();
    let agreement = crate::net::live_commands::StartupAgreement::new(
        full_houses,
        active_rows,
        seed,
        sim.session()
            .network_max_ahead_frames()
            .expect("installed network MaxAhead"),
        sim.session()
            .network_command_timing()
            .expect("installed timing")
            .frames_per_second(),
        rate,
        sim.state_hash(),
    )?;
    if state.match_state.session_mode() == SessionMode::Lan {
        let service = state.match_state.lan_service.as_mut().expect("visit");
        let scope = service
            .setup_scope()
            .ok_or_else(|| anyhow::anyhow!("LAN gameplay has no final accepted scope"))?;
        service
            .transport
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("LAN gameplay lost its transport"))?
            .set_lan_scope(scope)?;
    }
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
    if online_timing.is_none() {
        let (private, global) =
            RetryTiming::lan_gameplay(|| transport.global().mean_response_time());
        transport.set_retry_timing(private, true);
        transport.global_mut().set_timing(global);
    }
    // LAN5DD2DF/5DD49F is independent of game speed. WOL794F6F uses
    // the same agreed timing owner; the manifest binds that initial cadence.
    let mut live = crate::net::live_commands::LiveCommandSession::new(
        agreement,
        transport,
        local_house,
        seed,
        rate,
        if state.match_state.session_mode() == SessionMode::Wol {
            crate::net::live_commands::LiveCommandMode::Wol
        } else {
            crate::net::live_commands::LiveCommandMode::Lan
        },
    )?;
    if publisher == Some(local_house)
        && let Some(timing) = online_timing
    {
        live.configure_precalculated_timing(timing.precalc_max_ahead, timing.precalc_frame_rate);
    }
    state.match_state.live_network = Some(live);
    Ok(())
}

/// End private channels on leaving gameplay, retaining the LAN visit's same
/// socket pair for BB. It is dropped only when that visit itself ends.
pub(crate) fn return_transport(state: &mut AppState) {
    let online = state.match_state.session_mode() == SessionMode::Wol;
    let Some(live) = state.match_state.live_network.take() else {
        return;
    };
    if let Some(service) = state.match_state.lan_service.as_mut() {
        let mut transport = live.into_transport();
        transport.connections_mut().reset();
        if online {
            transport.retire_relay();
        } else {
            transport.clear_lan_scope();
        }
        service.transport = Some(transport);
        service.clear_setup();
    }
}

pub(crate) mod cooperative;
mod loading;
mod maps;
mod options;
pub(crate) use loading::LoadingNetworkService;

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
                if state.match_state.live_network.is_none() || percent >= 100 {
                    if let Some(service) = state.match_state.lan_service.as_mut() {
                        match service.queue_loading_progress(&state.match_state.session, percent) {
                            Ok(true) => service.flush_setup(),
                            Ok(false) => state
                                .match_state
                                .session
                                .report_network_loading_progress(percent),
                            Err(error) => {
                                state
                                    .match_state
                                    .session
                                    .report_network_loading_progress(percent);
                                log::warn!("Loading report failed: {error:#}");
                            }
                        }
                    }
                } else {
                    let scope = state
                        .match_state
                        .lan_service
                        .as_ref()
                        .and_then(LanService::setup_scope);
                    match loading::progress_packets(&state.match_state.session, scope, percent) {
                        Ok(packets) => {
                            for (bytes, destination) in packets {
                                send_bytes(state, &bytes, destination, false);
                            }
                        }
                        Err(error) => {
                            state
                                .match_state
                                .session
                                .report_network_loading_progress(percent);
                            log::warn!("Loading report failed: {error:#}");
                        }
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
                // BC6's chat writes [0x7EEF2C] (0x0046E126).
                let color = if state.match_state.session.cooperative_round().is_some() {
                    crate::ui::wol_chat::PUBLIC_COLOR as i32
                } else {
                    state
                        .match_state
                        .session
                        .network_setup()
                        .map_or(-2, |launch| color_value(launch.local_player()))
                };
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
            NetworkLoadingRequest::Beacon(record) => {
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
            // BC6's records; an ordinary Battle match never emits them.
            other => cooperative::send(state, other),
        }
    }
}

fn setup_liveness_active(state: &AppState) -> bool {
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

fn poll_setup_leases(state: &mut AppState, now: u32) {
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
        let expired = state
            .match_state
            .lan_service
            .as_mut()
            .expect("visit")
            .expired_setup_peer(&state.match_state.session, now);
        let Some((index, endpoint)) = expired else {
            break;
        };
        let host_left = state
            .match_state
            .lan_service
            .as_mut()
            .expect("visit")
            .retire_setup_peer(endpoint);
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

fn announce(state: &mut AppState, now: u32) {
    use super::match_runtime::session::WallBucketTimer;
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
    // A direct spawn has no lobby to discover or query; it only renews the
    // admitted peers' setup leases below.
    let lobby = !service.preadmitted;
    let discovery = lobby && service.discovery_timer.remaining(now) == 0 && !host_setup;
    let query = lobby && (service.force_query || service.query_timer.remaining(now) == 0);
    let presence = service.force_presence || service.presence_timer.remaining(now) == 0;
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
    if presence && lease_active {
        let service = state.match_state.lan_service.as_mut().expect("visit");
        if let Err(error) = service.send_setup_keepalives(&state.match_state.session) {
            log::debug!("LAN setup keepalive unavailable: {error:#}");
        }
    } else if presence && !in_setup {
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
        match setup_request(state, &request) {
            Ok(true) => {}
            Ok(false) => deferred.push_back(request),
            Err(error) => {
                log::warn!("LAN setup request failed: {error:#}");
                // A direct spawn has no dialog to retry from.
                if super::shell_network_setup::external(state) {
                    super::App::fail_external_launch(
                        state,
                        &format!("direct setup request: {error:#}"),
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
) -> anyhow::Result<bool> {
    use super::shell_network_setup::NetworkSetupRequest;
    if let Some(admitted) = maps::setup_request(state, request)? {
        return Ok(admitted);
    }
    let message = match request {
        NetworkSetupRequest::Kick { name } => {
            return state
                .match_state
                .lan_service
                .as_mut()
                .expect("visit")
                .kick_peer(&state.match_state.session, name);
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
                        && service.setup_maps_verified()
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
                .expect("visit")
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
                .begin_departure(&state.match_state.session, DepartureReason::Back, now)?;
            super::shell_network_setup::lock_lan_departure(state);
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
            )?
            .with_session_proof(
                &state
                    .match_state
                    .lan_service
                    .as_ref()
                    .expect("visit")
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
                .expect("visit")
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
        // RESIDUAL: mode+64's C1 (0x005C2710) is not encoded. VERA reads no
        // round assignment after the publish, so guests keep their own.
        NetworkSetupRequest::PublishCooperativeAssignments(_) => return Ok(true),
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
                    service.setup_maps_verified()
                        && service.setup_roster_complete(&state.match_state.session)
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
                .expect("visit")
                .apply_start_timing(false, false);
            verify_setup_agreement(state)?;
            super::shell_network_setup::prepare_lan_guest_handoff(state)?;
            let admitted = state
                .match_state
                .lan_service
                .as_mut()
                .expect("visit")
                .queue_go(&LanMessage::ready_to_go(), &[direct(endpoint)])?;
            assert!(admitted, "guest GO preflight must retain its reserved slot");
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
                    || !service.setup_maps_verified()
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
            .expect("visit")
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
        .expect("visit")
        .queue_setup(
            destinations
                .into_iter()
                .map(|destination| (message.as_bytes().to_vec(), destination)),
        )
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
            transport.service_before_close(super::match_runtime::frame_pacer::wall_clock_bucket);
        }
    }
    state.match_state.lan_service = None;
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
    let mut config = LanConnectionConfig {
        max_peers: crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT - 1,
        ..Default::default()
    };
    if session.network_return_context()
        == Some(super::match_runtime::session::NetworkReturnContext::Wol)
    {
        // VERA-only relay extension; retail wire compatibility is not required.
        config.max_peers = crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT - 1;
    }
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
    let host_startup = host_session.freeze_network_start(77).unwrap();
    let guest_startup = guest_session.freeze_network_start(77).unwrap();
    (
        host,
        host_session,
        host_startup,
        guest,
        guest_session,
        guest_startup,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext};
    use crate::sim::{house_state::HouseState, world::Simulation};

    pub(super) fn service() -> LanService {
        let mut service = LanService::open_on(
            UdpBindConfig {
                primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            },
            &[],
        )
        .unwrap();
        service.setup_scope = Some(SessionScope {
            epoch: [7; 16],
            revision: 7,
        });
        service
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

    pub(super) fn session(host: bool, remote: SocketAddrV4) -> MatchSession {
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

    fn global_frames(service: &mut LanService, now: u32) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        service
            .transport
            .as_mut()
            .unwrap()
            .global_mut()
            .service(now, &mut |_, bytes| frames.push(bytes.to_vec()));
        frames
    }

    fn global_receive(
        service: &mut LanService,
        source: SocketAddrV4,
        frame: &[u8],
    ) -> Vec<Vec<u8>> {
        let mut replies = Vec::new();
        service
            .transport
            .as_mut()
            .unwrap()
            .global_mut()
            .receive(source, 0, frame, 32, &mut |_, bytes| {
                replies.push(bytes.to_vec())
            })
            .unwrap();
        replies
    }

    fn global_capacity(service: &mut LanService, slots: usize) {
        *service.transport.as_mut().unwrap().global_mut() = crate::net::global::GlobalChannel::new(
            crate::net::global::GlobalConfig {
                send_capacity: slots,
                ..Default::default()
            },
            LOBBY_RETRY_TIMING,
        );
    }

    #[test]
    fn go_drain_observes_worker_failure_with_the_empty_queue() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::{Duration, Instant};

        static BUCKET: AtomicU32 = AtomicU32::new(0);
        fn bucket() -> u32 {
            BUCKET.load(Ordering::Acquire)
        }
        let sink = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        sink.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let std::net::SocketAddr::V4(destination) = sink.local_addr().unwrap() else {
            unreachable!()
        };
        let mut service = service();
        let transport = service.transport.as_mut().unwrap();
        transport.global_mut().set_timing(RetryTiming {
            retry_delta: 0,
            max_retries: u32::MAX,
            timeout: 0,
        });
        transport.start_io_pump(bucket).unwrap();
        assert!(
            service
                .queue_go(&LanMessage::ready_to_go(), &[direct(destination)])
                .unwrap()
        );

        // The first real send succeeds, but this socket deliberately never
        // sends an ACK. Hold the fixture clock while the main poll reads OK.
        BUCKET.store(1, Ordering::Release);
        let mut bytes = [0; crate::net::udp::UDP_DATAGRAM_CAPACITY];
        let (length, _) = sink.recv_from(&mut bytes).unwrap();
        assert!(length > 20);
        assert_eq!(&bytes[20..24], &0x13_u32.to_le_bytes());
        let earlier = service.transport.as_mut().unwrap().service(1);
        let earlier_ok = earlier.discarded_global_sends == 0 && earlier.failed_peer.is_none();
        assert!(earlier_ok);
        assert_eq!(service.queued_setup_count(), 1);

        // Interleave the existing retry/timeout/discard body AFTER that poll
        // and BEFORE the GO admission observation. No injected queue/report.
        BUCKET.store(2, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(2);
        while service.queued_setup_count() != 0 {
            assert!(Instant::now() < deadline, "GO reply did not time out");
            std::thread::yield_now();
        }
        let observation = service.go_transport_observation(earlier_ok).unwrap();
        assert_eq!(observation.queued, 0);
        assert!(
            !observation.service_ok,
            "a timed-out GO reply must not become a successful empty drain"
        );
        // The admission snapshot must leave diagnostics for their ordinary
        // consumer instead of silently eating the worker's failure report.
        assert_eq!(
            service
                .transport
                .as_mut()
                .unwrap()
                .service(2)
                .discarded_global_sends,
            1
        );
    }

    #[test]
    fn admission_proofs_bind_current_attempt_scope_full_build_and_selected_rules() {
        let mut service = service();
        let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        service.pending_host = Some(host);
        service.join_nonce = Some([11; 16]);
        let agreement = super::super::shell_network_setup::agreement::MultiplayerAgreement {
            version: super::super::shell_network_setup::agreement::version(),
            rules_digest: "a".repeat(64),
        };
        service.setup_agreement = Some(agreement.clone());
        let proof = LanSessionProof {
            scope: service.setup_scope().unwrap(),
            join_nonce: [11; 16],
            admission_nonce: [0; 16],
            version: agreement.version,
            rules_digest: agreement.rules_digest,
        };
        let packet = LanMessage::join_accepted(
            "Host",
            crate::net::lan_messages::LanAcceptance {
                country: 0,
                color: 1,
                assigned_row: 1,
                observer_row: 7,
            },
        )
        .unwrap()
        .with_session_proof(&proof)
        .unwrap();
        let decoded = LanMessage::decode(packet.as_bytes()).unwrap();
        let mut received = decoded.session_proof().unwrap().unwrap();
        assert_eq!(
            received, proof,
            "full build/rules identities must survive the native packet tail"
        );
        assert!(service.accepts_join_reply(host, &received));
        assert!(service.validate_peer_agreement(&received));
        received.join_nonce = [12; 16];
        assert!(
            !service.accepts_join_reply(host, &received),
            "old attempt cannot complete the new join"
        );
        received = proof.clone();
        received.scope.epoch = [8; 16];
        assert!(
            !service.validate_peer_agreement(&received),
            "old room cannot ready/GO a new room"
        );
        received = proof.clone();
        received.scope.revision -= 1;
        assert!(
            !service.validate_scope(&received),
            "delayed A0/K0 cannot retract a newer revision"
        );
        received = proof.clone();
        received.version.push('x');
        assert!(
            !service.validate_peer_agreement(&received),
            "same package version with another build is incompatible"
        );
        received = proof;
        received.rules_digest = "b".repeat(64);
        assert!(
            !service.validate_peer_agreement(&received),
            "a different selected mode/rules closure is incompatible"
        );
    }

    /// Source `local`'s direct spawn.ini for `count` humans, all on loopback
    /// with port 1300 + source (docs/spawn.md).
    fn direct_spawn(
        count: usize,
        local: usize,
        seed: u32,
    ) -> crate::app::frontend::cncnet_spawn::NetworkSpawn {
        let mut text = format!(
            "[Settings]\nName=P{local}\nSide={}\nColor={local}\nPlayerCount={count}\n\
             SourceIndex={local}\nSeed={seed}\nSpecialHouseIsAlly=No\nPort={}\n",
            local % 2,
            1300 + local
        );
        for (other, source) in (0..count).filter(|&source| source != local).enumerate() {
            text.push_str(&format!(
                "[Other{}]\nName=P{source}\nSide={}\nColor={source}\nSourceIndex={source}\n\
                 Ip=127.0.0.1\nPort={}\n",
                other + 1,
                source % 2,
                1300 + source
            ));
        }
        match crate::app::frontend::cncnet_spawn::SpawnRequest::parse(&text)
            .unwrap()
            .game
        {
            crate::app::frontend::cncnet_spawn::SpawnGame::Network(network) => *network,
            other => panic!("expected a network spawn, got {other:?}"),
        }
    }

    fn direct_admission(count: usize, local: usize, seed: u32) -> (DirectAdmission, u16) {
        let network = direct_spawn(count, local, seed);
        let crate::app::frontend::cncnet_spawn::SpawnTransport::Direct(direct) = network.transport
        else {
            panic!("direct transport");
        };
        (
            DirectAdmission::derive(&network.session, seed, &direct.peers, b"map").unwrap(),
            direct.local_port,
        )
    }

    #[test]
    fn direct_spawn_participants_derive_one_admission() {
        let endpoint = |source: usize| SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1300 + source as u16);
        let all: Vec<_> = (0..3).map(|local| direct_admission(3, local, 77)).collect();
        let (host, host_port) = &all[0];
        assert_eq!(*host_port, 1300);
        assert_ne!(host.scope.epoch, [0; 16]);
        assert_eq!(host.scope.revision, 1, "a scope the match transport admits");
        assert_eq!(host.host, None, "source 0 is the host");
        for (local, (admission, port)) in all.iter().enumerate() {
            assert_eq!(*port, 1300 + local as u16);
            assert_eq!(admission.scope, host.scope, "one shared setup scope");
            assert_ne!(admission.local.join_nonce, [0; 16]);
            assert_ne!(admission.local.admission_nonce, [0; 16]);
            let remote: Vec<_> = admission.peers.iter().map(|peer| peer.0).collect();
            let expected: Vec<_> = (0..3).filter(|&s| s != local).map(endpoint).collect();
            assert_eq!(remote, expected);
            for (peer, name, identity) in &admission.peers {
                let source = usize::from(peer.port() - 1300);
                assert_eq!(*name, format!("P{source}"));
                // Each record names the pair that source's own proofs carry.
                assert_eq!(*identity, all[source].0.local);
            }
            if local != 0 {
                assert_eq!(admission.host, Some(("P0".to_owned(), endpoint(0))));
            }
        }
        assert_ne!(
            all[1].0.local, all[2].0.local,
            "sources have distinct pairs"
        );
        assert_ne!(
            direct_admission(3, 1, 78).0.scope,
            all[1].0.scope,
            "another launch never shares a scope"
        );
        let network = direct_spawn(2, 0, 77);
        assert!(
            DirectAdmission::derive(&network.session, 77, &[None, None], b"map").is_err(),
            "every remote source needs an endpoint"
        );
    }

    #[test]
    fn direct_spawn_admission_satisfies_the_lan_go_owners_over_udp() {
        let mut host = service();
        let mut guest = service();
        let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
        let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
        let network = |local: usize| {
            let mut launch = launch();
            launch.local_player_index = local;
            launch
        };
        let peers = [None, Some(guest_address)];
        let host_admission = DirectAdmission::derive(&network(0), 5, &peers, b"map").unwrap();
        let guest_admission =
            DirectAdmission::derive(&network(1), 5, &[Some(host_address), None], b"map").unwrap();
        let host_session = session(true, guest_address);
        let guest_session = session(false, host_address);
        let ordinary = service();
        assert!(!ordinary.setup_roster_complete(&host_session));
        assert!(!ordinary.setup_maps_verified());
        host.adopt_direct_admission(&host_admission);
        guest.adopt_direct_admission(&guest_admission);
        assert!(host.setup_roster_complete(&host_session));
        assert!(guest.setup_roster_complete(&guest_session));
        assert!(host.setup_maps_verified() && guest.setup_maps_verified());
        assert_eq!(guest.admitted_host, Some(("Host".to_owned(), host_address)));
        assert!(!host.direct_peers_heard(), "GO waits for every guest");

        guest.send_setup_keepalives(&guest_session).unwrap();
        guest.transport.as_mut().unwrap().service(31);
        let received = next_message(&mut host, &host_session);
        let message = LanMessage::decode(&received.payload).unwrap();
        assert!(
            host.accept_setup_keepalive(&host_session, &received, &message, 1)
                .unwrap()
        );
        assert!(host.direct_peers_heard());

        host.send_setup_keepalives(&host_session).unwrap();
        host.transport.as_mut().unwrap().service(32);
        let received = next_message(&mut guest, &guest_session);
        let message = LanMessage::decode(&received.payload).unwrap();
        assert!(
            guest
                .accept_setup_keepalive(&guest_session, &received, &message, 2)
                .unwrap(),
            "the host echoes the guest's own pair"
        );
        // GO and its Type13 reply carry proofs the other side admits.
        let go = host.control_proof().unwrap();
        assert!(guest.validate_scope(&go) && guest.peer_admission_matches(host_address, &go));
        let reply = guest.control_proof().unwrap();
        assert!(host.validate_scope(&reply) && host.peer_admission_matches(guest_address, &reply));
    }

    #[test]
    fn setup_keepalive_uses_the_confirmed_pair_and_ignores_reordered_replays() {
        let mut host = service();
        let mut guest = service();
        let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
        let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
        let host_session = session(true, guest_address);
        let guest_session = session(false, host_address);
        guest.join_nonce = Some([11; 16]);
        guest.admission_nonce = Some([12; 16]);
        guest.admitted_host = Some(("Host".into(), host_address));
        let proof = guest.control_proof().unwrap();
        host.commit_peer_admission(guest_address, "Guest", &proof);
        guest.replace_peer_admissions([(
            host_address,
            "Host".into(),
            PeerAdmission {
                join_nonce: [0; 16],
                admission_nonce: [0; 16],
            },
        )]);
        assert_eq!(host.expired_setup_peer(&host_session, 0), None);
        assert_eq!(guest.expired_setup_peer(&guest_session, 0), None);
        // One whole lease minus one bucket may pass without a probe. The real
        // UDP/global path supplies the sender sequence and physical endpoint.
        guest.send_setup_keepalives(&guest_session).unwrap();
        guest.transport.as_mut().unwrap().service(31);
        let received = next_message(&mut host, &host_session);
        let message = LanMessage::decode(&received.payload).unwrap();
        host.setup_scope.as_mut().unwrap().revision += 1;
        assert!(
            !host.validate_scope(&proof),
            "old readiness remains invalid"
        );
        assert!(
            host.accept_setup_keepalive(&host_session, &received, &message, SETUP_PEER_LEASE - 1)
                .unwrap()
        );
        assert!(
            !host
                .accept_setup_keepalive(
                    &host_session,
                    &received,
                    &message,
                    2 * SETUP_PEER_LEASE - 2
                )
                .unwrap()
        );
        assert_eq!(
            host.expired_setup_peer(&host_session, 2 * SETUP_PEER_LEASE - 2),
            None
        );
        assert_eq!(
            host.expired_setup_peer(&host_session, 2 * SETUP_PEER_LEASE - 1),
            Some((1, guest_address))
        );
        // The host echoes this recipient's pair, rather than a public zero
        // proof, so a fresh same-endpoint guest rejects the former heartbeat.
        host.send_setup_keepalives(&host_session).unwrap();
        host.transport.as_mut().unwrap().service(31);
        let received = next_message(&mut guest, &guest_session);
        let message = LanMessage::decode(&received.payload).unwrap();
        assert!(
            guest
                .accept_setup_keepalive(&guest_session, &received, &message, 10)
                .unwrap()
        );
        guest.admission_nonce = Some([13; 16]);
        let mut newer = received;
        newer.sequence = newer.sequence.wrapping_add(1);
        assert!(
            !guest
                .accept_setup_keepalive(&guest_session, &newer, &message, 11)
                .unwrap()
        );
    }

    #[test]
    fn setup_peer_expiry_cancels_only_that_endpoint_before_republishing_roster() {
        let mut host = service();
        global_capacity(&mut host, 2);
        let expired = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        let survivor = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30124);
        let mut session = session(true, expired);
        let mut other = session.network_setup().unwrap().humans[1].clone();
        other.name = "Other".into();
        other.raw_slot_index = Some(2);
        session.add_network_participant(other).unwrap();
        session.bind_network_endpoint("Other", survivor).unwrap();
        let mut proof = host.control_proof().unwrap();
        proof.join_nonce = [11; 16];
        proof.admission_nonce = [12; 16];
        host.commit_peer_admission(expired, "Guest", &proof);
        proof.admission_nonce = [13; 16];
        host.commit_peer_admission(survivor, "Other", &proof);
        assert_eq!(host.expired_setup_peer(&session, 0), None);
        let message = LanMessage::named(LanNamedMessage::Keepalive, "Other")
            .unwrap()
            .with_session_proof(&proof)
            .unwrap();
        let incoming = GlobalMessage {
            source: survivor,
            product_id: 0xaa04,
            sequence: 8,
            payload: message.as_bytes().to_vec(),
        };
        assert!(
            host.accept_setup_keepalive(&session, &incoming, &message, SETUP_PEER_LEASE - 1)
                .unwrap()
        );
        assert!(host.send(b"expired previous", direct(expired), true));
        assert!(host.send(b"survivor previous", direct(survivor), true));
        host.pending_setup.extend([
            (b"expired map".to_vec(), direct(expired)),
            (b"survivor map".to_vec(), direct(survivor)),
        ]);
        let (index, endpoint) = host.expired_setup_peer(&session, SETUP_PEER_LEASE).unwrap();
        assert_eq!((index, endpoint), (1, expired));
        assert!(!host.retire_setup_peer(endpoint));
        // The production app emits ParticipantLeft through the same existing
        // Session mutation before republishing the canonical setup snapshot.
        session.remove_network_participant(index).unwrap();
        host.flush_setup();
        assert_eq!(session.network_setup().unwrap().humans.len(), 2);
        assert_eq!(session.network_endpoint_source(survivor), Some(1));
        assert!(host.setup_roster_complete(&session));
        assert_eq!(host.queued_setup_count(), 2);
        let frames = global_frames(&mut host, 31);
        assert_eq!(frames.len(), 2);
        assert!(
            frames
                .iter()
                .all(|frame| !frame.ends_with(b"expired previous")
                    && !frame.ends_with(b"expired map"))
        );
        assert!(
            frames
                .iter()
                .any(|frame| frame.ends_with(b"survivor previous"))
        );
        assert!(frames.iter().any(|frame| frame.ends_with(b"survivor map")));
    }

    #[test]
    fn identical_host_snapshots_do_not_renew_the_lease_and_host_loss_retires_setup() {
        let mut guest = service();
        let host_address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        let remote_address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30124);
        let mut session = session(false, host_address);
        let mut other = session.network_setup().unwrap().humans[0].clone();
        other.name = "Other".into();
        other.raw_slot_index = Some(2);
        session.add_network_participant(other).unwrap();
        session
            .bind_network_endpoint("Other", remote_address)
            .unwrap();
        guest.admitted_host = Some(("Host".into(), host_address));
        guest.join_nonce = Some([11; 16]);
        guest.admission_nonce = Some([12; 16]);
        let identity = PeerAdmission {
            join_nonce: [0; 16],
            admission_nonce: [0; 16],
        };
        guest.replace_peer_admissions([
            (host_address, "Host".into(), identity),
            (remote_address, "Other".into(), identity),
        ]);
        let now = u32::MAX - 20;
        assert_eq!(guest.expired_setup_peer(&session, now), None);
        // A full current options snapshot may repeat during map download; it
        // cannot pretend the host sent a fresh authenticated presence probe.
        guest.replace_peer_admissions([
            (host_address, "Host".into(), identity),
            (remote_address, "Other".into(), identity),
        ]);
        assert_eq!(
            guest.expired_setup_peer(&session, now.wrapping_add(SETUP_PEER_LEASE - 1)),
            None
        );
        assert_eq!(
            guest.expired_setup_peer(&session, now.wrapping_add(SETUP_PEER_LEASE)),
            Some((0, host_address))
        );
        guest
            .pending_setup
            .push_back((b"old map request".to_vec(), direct(host_address)));
        assert!(guest.send(b"old options", direct(host_address), true));
        assert!(guest.retire_setup_peer(host_address));
        assert_eq!(guest.setup_scope(), None);
        assert!(guest.received_options.is_none());
        assert!(guest.admitted_host.is_none());
        assert!(guest.peer_admissions.is_empty());
        assert_eq!(guest.queued_setup_count(), 0);
        assert!(guest.queue_go(&LanMessage::go(2), &[]).is_err());
        assert_eq!(
            session.return_to_network_lobby(),
            Some(NetworkReturnContext::LanLobby)
        );
        assert!(session.network_setup().is_none());
        // A normal retry starts with no prior deadline/sequence ownership.
        guest.setup_scope = Some(SessionScope {
            epoch: [8; 16],
            revision: 1,
        });
        guest.admitted_host = Some(("Host".into(), host_address));
        guest.replace_peer_admissions([(host_address, "Host".into(), identity)]);
        let session = self::session(false, host_address);
        assert_eq!(
            guest.expired_setup_peer(&session, now.wrapping_add(SETUP_PEER_LEASE)),
            None
        );
    }

    #[test]
    fn newer_join_challenge_survives_old_request_confirm_and_cancel() {
        use crate::net::lan_messages::{LanJoinRequest, LanVersionInterval};
        let mut host = service();
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        let request = |nonce| {
            let mut proof = host.control_proof().unwrap();
            proof.scope = SessionScope {
                epoch: [0; 16],
                revision: 0,
            };
            proof.join_nonce = [nonce; 16];
            LanMessage::join_request(
                "Guest",
                &[],
                LanJoinRequest {
                    country: -2,
                    color: -2,
                    version: LanVersionInterval::RETAIL,
                    compatibility: LanCompatibility {
                        field_0x43: 1,
                        field_0x47: 2,
                        field_0x4b: 3,
                        field_0x4f: 4,
                    },
                    observer_request: 0,
                },
            )
            .unwrap()
            .with_session_proof(&proof)
            .unwrap()
        };
        let old = request(1);
        let current = request(2);
        let old_identity = host.join_offer(endpoint, &old, 31).unwrap();
        let identity = host.join_offer(endpoint, &current, 32).unwrap();
        assert_ne!(old_identity.admission_nonce, identity.admission_nonce);
        assert_eq!(host.join_offer(endpoint, &old, 33).unwrap(), old_identity);
        assert!(
            host.peer_admissions.is_empty(),
            "an offer cannot admit a Session peer"
        );
        let mut proof = host.control_proof().unwrap();
        proof.join_nonce = identity.join_nonce;
        proof.admission_nonce = identity.admission_nonce;
        // Concurrent peers can advance options while this challenge is in flight.
        host.setup_scope.as_mut().unwrap().revision += 1;
        assert_eq!(host.confirmed_request(endpoint, &proof, 34), Some(current));
        host.commit_peer_admission(endpoint, "Host", &proof);
        assert!(
            !host.accepts_participant(true, endpoint, "Host", &proof),
            "host Type3 must never bypass Confirm"
        );
        assert!(host.accepts_participant(false, endpoint, "Host", &proof));
        assert!(
            !host.accepts_participant(false, endpoint, "Other", &proof),
            "a guest assembles only the current host-authored binding"
        );
        let mut stale = proof.clone();
        stale.join_nonce = old_identity.join_nonce;
        stale.admission_nonce = old_identity.admission_nonce;
        assert!(host.confirmed_request(endpoint, &stale, 35).is_none());
        assert!(!host.cancel_peer_admission(endpoint, &stale));
        assert!(host.peer_admission_matches(endpoint, &proof));
        let replayed_offer = host.join_offer(endpoint, &old, 36).unwrap();
        assert_ne!(replayed_offer.admission_nonce, old_identity.admission_nonce);
        assert!(host.confirmed_request(endpoint, &stale, 37).is_none());
        stale.admission_nonce = [0; 16];
        assert!(!host.cancel_peer_admission(endpoint, &stale));
        assert!(host.peer_admission_matches(endpoint, &proof));
        assert!(host.pending_admissions.is_empty());
        assert!(host.cancel_peer_admission(endpoint, &proof));
        assert!(host.peer_admissions.is_empty());
        assert!(
            !host.accepts_participant(false, endpoint, "Host", &proof),
            "late Type3 cannot resurrect a canceled source"
        );
    }

    #[test]
    fn departure_fifo_survives_full_global_and_waits_for_every_ack() {
        let mut host = service();
        let mut guest = service();
        global_capacity(&mut host, 1);
        let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
        let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
        let session = session(true, guest_address);
        assert!(host.send(b"previous setup", direct(guest_address), true));
        host.pending_setup
            .push_back((b"superseded map chunk".to_vec(), direct(guest_address)));
        host.begin_departure(&session, DepartureReason::Back, 1)
            .unwrap();
        assert_eq!(host.queued_setup_count(), 3);
        assert!(host.departure_ready(31, true).is_none());
        let previous = global_frames(&mut host, 31).pop().unwrap();
        let ack = global_receive(&mut guest, host_address, &previous)
            .pop()
            .unwrap();
        host.flush_setup();
        assert_eq!(
            host.pending_setup.len(),
            2,
            "full global retains both departures"
        );
        global_receive(&mut host, guest_address, &ack);
        global_frames(&mut host, 33);
        host.flush_setup();
        let departure = global_frames(&mut host, 64).pop().unwrap();
        let ack = global_receive(&mut guest, host_address, &departure)
            .pop()
            .unwrap();
        let mut global = guest.transport.as_mut().unwrap().global_mut();
        assert_eq!(global.pop_received().unwrap().payload, b"previous setup");
        global.retire_consumed();
        assert_eq!(
            LanMessage::decode(&global.pop_received().unwrap().payload)
                .unwrap()
                .kind(),
            LanMessageKind::Departure
        );
        drop(global);
        assert!(
            host.departure_ready(64, true).is_none(),
            "direct receipt is still unacknowledged"
        );
        global_receive(&mut host, guest_address, &ack);
        global_frames(&mut host, 65);
        host.flush_setup();
        assert!(
            host.departure_ready(65, true).is_none(),
            "native broadcast still waits in the same queue"
        );
        global_frames(&mut host, 96);
        global_frames(&mut host, 97);
        assert_eq!(host.departure_ready(97, true), Some(DepartureReason::Back));
        assert!(host.departure_ready(97, true).is_none());
    }

    #[test]
    fn canceled_join_requires_its_receipt_and_timeout_preserves_global_sequence() {
        let mut guest = service();
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        let session = MatchSession::default();
        guest.pending_host = Some(endpoint);
        guest.join_nonce = Some([1; 16]);
        guest.admission_nonce = Some([2; 16]);
        guest
            .begin_departure(&session, DepartureReason::CancelJoin { game_id: 7 }, 100)
            .unwrap();
        let proof = guest.control_proof().unwrap();
        assert!(
            !guest.accepts_join_reply(endpoint, &proof),
            "a final reply already queued when BB times out cannot commit after cancellation intent"
        );
        assert_eq!(
            guest.pending_host,
            Some(endpoint),
            "cleanup retains its endpoint"
        );
        let mut stale = proof.clone();
        stale.admission_nonce = [3; 16];
        assert!(!guest.accept_cancel_receipt(endpoint, &stale));
        assert!(guest.departure_ready(132, true).is_none());
        assert!(guest.accept_cancel_receipt(endpoint, &proof));
        assert!(
            guest.departure_ready(132, true).is_none(),
            "receipt does not bypass queued ACK drain"
        );
        let sequence = guest
            .transport
            .as_ref()
            .unwrap()
            .global()
            .next_send_sequence();
        assert_eq!(
            guest.departure_ready(700, true),
            Some(DepartureReason::CancelJoin { game_id: 7 })
        );
        guest.clear_setup();
        assert!(guest.send(b"new request", direct(endpoint), true));
        assert_eq!(
            guest
                .transport
                .as_ref()
                .unwrap()
                .global()
                .next_send_sequence(),
            sequence + 1,
            "same endpoint retry must not collide with the host's retained sequence ring"
        );
    }

    #[test]
    fn setup_fifo_retains_backpressure_and_following_control_packets() {
        let mut host = service();
        host.setup_scope = Some(SessionScope {
            epoch: [7; 16],
            revision: 7,
        });
        let mut guest = service();
        global_capacity(&mut host, 2);
        let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
        let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
        host.queue_setup(
            [
                b"roster".to_vec(),
                b"options one".to_vec(),
                b"options two".to_vec(),
            ]
            .into_iter()
            .map(|bytes| (bytes, direct(guest_address))),
        )
        .unwrap();
        host.queue_setup(std::iter::once((b"ready".to_vec(), direct(guest_address))))
            .unwrap();
        assert_eq!(host.pending_setup.len(), 2);
        assert_eq!(host.queued_setup_count(), 4);
        let frames = global_frames(&mut host, 31);
        assert_eq!(frames.len(), 2);
        for frame in frames {
            for ack in global_receive(&mut guest, host_address, &frame) {
                global_receive(&mut host, guest_address, &ack);
            }
        }
        global_frames(&mut host, 33);
        host.flush_setup();
        assert!(host.pending_setup.is_empty());
        for frame in global_frames(&mut host, 64) {
            global_receive(&mut guest, host_address, &frame);
        }
        let mut global = guest.transport.as_mut().unwrap().global_mut();
        for expected in [
            b"roster".as_slice(),
            b"options one",
            b"options two",
            b"ready",
        ] {
            assert_eq!(global.pop_received().unwrap().payload, expected);
            global.retire_consumed();
        }
        assert!(global.pop_received().is_none());
    }

    #[test]
    fn go_waits_for_the_last_final_fragment_ack_and_its_own_drain() {
        use crate::net::lan_messages::extended_setup::{Reassembly, SetupKind, encode};
        let mut host = service();
        host.setup_scope = Some(SessionScope {
            epoch: [7; 16],
            revision: 7,
        });
        let mut guest = service();
        global_capacity(&mut host, 2);
        let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
        let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
        let payload = vec![73; 650];
        let parts = encode(
            SetupKind::Options,
            SessionScope {
                epoch: [7; 16],
                revision: 7,
            },
            &payload,
        )
        .unwrap();
        host.queue_setup(parts.into_iter().map(|part| (part, direct(guest_address))))
            .unwrap();
        assert!(host.pending_setup.is_empty());
        let go = LanMessage::go(10);
        assert!(!host.queue_go(&go, &[direct(guest_address)]).unwrap());
        let mut frames = global_frames(&mut host, 31);
        let last = frames.pop().unwrap();
        let delayed_ack = global_receive(&mut guest, host_address, &last)
            .pop()
            .unwrap();
        for frame in frames {
            for ack in global_receive(&mut guest, host_address, &frame) {
                global_receive(&mut host, guest_address, &ack);
            }
        }
        global_frames(&mut host, 33);
        assert_eq!(host.queued_setup_count(), 1);
        assert!(!host.queue_go(&go, &[direct(guest_address)]).unwrap());
        let mut global = guest.transport.as_mut().unwrap().global_mut();
        let mut reassembly = Reassembly::default();
        assert!(
            reassembly
                .receive(&global.pop_received().unwrap().payload, 32)
                .unwrap()
                .is_none()
        );
        global.retire_consumed();
        assert_eq!(
            reassembly
                .receive(&global.pop_received().unwrap().payload, 32)
                .unwrap(),
            Some((SetupKind::Options, payload))
        );
        global.retire_consumed();
        drop(global);
        global_receive(&mut host, guest_address, &delayed_ack);
        global_frames(&mut host, 34);
        assert!(host.queue_go(&go, &[direct(guest_address)]).unwrap());
        assert_eq!(
            host.queued_setup_count(),
            1,
            "GO handoff still waits for its ACK"
        );
        for frame in global_frames(&mut host, 65) {
            for ack in global_receive(&mut guest, host_address, &frame) {
                global_receive(&mut host, guest_address, &ack);
            }
        }
        let received_go = LanMessage::decode(
            &guest
                .transport
                .as_mut()
                .unwrap()
                .global_mut()
                .pop_received()
                .unwrap()
                .payload,
        )
        .unwrap();
        assert_eq!(received_go.kind(), LanMessageKind::Go);
        assert_eq!(received_go.go_max_ahead(), go.go_max_ahead());
        assert_eq!(
            received_go.session_proof().unwrap().unwrap().scope,
            host.setup_scope().unwrap()
        );
        global_frames(&mut host, 66);
        assert_eq!(host.queued_setup_count(), 0);
    }

    #[test]
    fn go_recipient_admission_is_atomic_when_global_capacity_is_insufficient() {
        let mut host = service();
        host.setup_scope = Some(SessionScope {
            epoch: [7; 16],
            revision: 7,
        });
        global_capacity(&mut host, 1);
        let destinations = [
            direct(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123)),
            direct(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30125)),
        ];
        assert!(!host.queue_go(&LanMessage::go(10), &destinations).unwrap());
        assert_eq!(host.queued_setup_count(), 0);
        assert_eq!(
            host.transport
                .as_ref()
                .unwrap()
                .global()
                .next_send_sequence(),
            0
        );
        global_capacity(&mut host, 2);
        assert!(host.queue_go(&LanMessage::go(10), &destinations).unwrap());
        assert!(host.pending_setup.is_empty());
        assert_eq!(host.queued_setup_count(), 2);
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
        // ACKs can retire every sender entry while the guest's shell still
        // transitions. Both sockets are nonblocking: wait for actual ACKs,
        // keeping protocol time fixed and every early packet in the queue.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            receiver.transport.as_mut().unwrap().service(32);
            assert!(receiver.pop_lobby_received(&session).is_none());
            sender.transport.as_mut().unwrap().service(33);
            if sender
                .transport
                .as_ref()
                .unwrap()
                .global()
                .queued_send_count()
                == 0
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "early setup packets were not acknowledged"
            );
            std::thread::yield_now();
        }
        assert_eq!(
            receiver
                .transport
                .as_ref()
                .unwrap()
                .global()
                .queued_receive_count(),
            messages.len()
        );
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
        assert!(guest.departure_ready(32, true).is_none());
        guest.transport.as_mut().unwrap().service(33);
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
        while guest.departure_ready(33, true).is_none() {
            guest.transport.as_mut().unwrap().service(33);
            assert!(
                std::time::Instant::now() < deadline,
                "departure did not drain"
            );
            std::thread::yield_now();
        }
        assert!(
            guest.departure_ready(33, true).is_none(),
            "the shell exit occurs once"
        );
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
        let mut global = service.transport.as_mut().unwrap().global_mut();
        // Guest5DD9FD leaves primary-global at60/-1/600, separately from
        // the private20/-1/160 above. Back must preserve that timing too.
        global.service(21, &mut |_, _| panic!("private timing leaked into global"));
        let mut sends = 0;
        global.service(61, &mut |_, _| sends += 1);
        assert_eq!(sends, 1);
        drop(global);
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
        let mut connections = transport.connections_mut();
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
        drop(connections);
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
        // Gameplay requires a frozen start and admitted endpoints. Rejection
        // does not take or mutate any sockets.
        assert!(gameplay_connections(&session, &sim, LOBBY_RETRY_TIMING).is_err());
        let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        session.bind_network_endpoint(&host_name, host).unwrap();
        session.prepare_network_timing(2, || 0).unwrap();
        session.freeze_network_start(19).unwrap();
        let (local_house, seed, connections) =
            gameplay_connections(&session, &sim, LOBBY_RETRY_TIMING).unwrap();
        assert_eq!((local_house, seed), (0, 19));
        assert_eq!(connections.peers().collect::<Vec<_>>(), vec![(1, host)]);
        assert_eq!(session.network_endpoint(0), Some(host));
    }

    #[test]
    fn loading_survivor_connections_handoff_the_original_majority_certificate() {
        use crate::net::live_commands::{LiveCommandMode, LiveCommandSession, StartupAgreement};
        use std::num::NonZeroU32;

        let mut launch = launch();
        launch.humans[0].raw_slot_index = Some(0);
        let mut third = launch.humans[1].clone();
        third.name = "Third".into();
        third.raw_slot_index = Some(2);
        third.slot.color_index = 3;
        third.slot.start_position = crate::skirmish_launch::LaunchStartPosition::Position(3);
        launch.humans.push(third);
        launch.local_player_index = 1;
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::Wol);
        session.accept_network_setup(launch, false, None).unwrap();
        let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
        let third = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30124);
        session.bind_network_endpoint("Host", host).unwrap();
        session.bind_network_endpoint("Third", third).unwrap();
        session.prepare_wol_timing([0; 3], false).unwrap();
        session.freeze_network_start(19).unwrap();
        let mut sim = Simulation::with_seed(19);
        for (house, source) in [1, 2, 0].into_iter().enumerate() {
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
        super::super::match_runtime::network_match::destroy_connection(
            &mut session,
            &mut sim,
            0,
            super::super::match_runtime::network_match::ConnectionError::Lost,
            None,
        )
        .unwrap();
        assert_eq!(session.network_connected_source_indices(), &[1, 2]);
        assert_eq!(session.network_endpoint(0), Some(host));
        assert_eq!(session.network_setup().unwrap().humans.len(), 3);
        let (local, seed, connections) =
            gameplay_connections(&session, &sim, LOBBY_RETRY_TIMING).unwrap();
        let rate = NonZeroU32::new(5).unwrap();
        let agreement = StartupAgreement::new(
            [(0, 2), (1, 0), (2, 1)],
            [1, 2],
            seed,
            10,
            30,
            rate,
            sim.state_hash(),
        )
        .unwrap();
        let mut service = service();
        let mut transport = service.transport.take().unwrap();
        let address = transport.primary_addr().unwrap();
        *transport.connections_mut() = connections;
        let live = LiveCommandSession::new(
            agreement,
            transport,
            local,
            seed,
            rate,
            LiveCommandMode::Wol,
        )
        .expect("the production connected roster must install its two-of-three certificate");
        assert_eq!(live.primary_addr().unwrap(), address);
        assert_eq!(live.connections().peers().collect::<Vec<_>>(), [(1, third)]);
        assert_eq!(live.into_transport().primary_addr().unwrap(), address);
    }
}

fn service_transport(state: &mut AppState, now: u32) -> bool {
    if let Some(live) = state.match_state.live_network.as_mut() {
        if let Some(service) = state.match_state.lan_service.as_mut() {
            LanService::flush_setup_queue(&mut service.pending_setup, &mut *live.global_mut());
        }
        if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
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
        super::shell_network_setup::fail_go_handoff(state);
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
                super::frontend::shell_transition::ShellExitThen::NetworkSetupBack(dialog),
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
