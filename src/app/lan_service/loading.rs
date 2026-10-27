//! LoadingCallback69AE90's borrowed service while the synchronous loader holds
//! its LoadingSession outside Frontend. Session, UDP and progress rows retain
//! their original owners; this view owns no transport, inbox or progress copy.

use super::*;
use crate::app::loading::progress_row::LoadingProgressRowSnapshot;
use crate::app::match_runtime::session::MatchSession;
use crate::net::global::GlobalMessageRef;
use crate::net::lan_messages::ingame::{LoadingProgressMessage, SCOPED_LOADING_MAGIC};

pub(crate) struct LoadingNetworkService<'a> {
    session: &'a mut MatchSession,
    service: Option<&'a mut LanService>,
}

impl<'a> LoadingNetworkService<'a> {
    pub(crate) fn borrow(
        session: &'a mut MatchSession,
        service: Option<&'a mut LanService>,
        controlled_capture: bool,
    ) -> anyhow::Result<Option<Self>> {
        let count = session.network_connected_source_indices().len();
        //684505 passes SessionA8B238 as69AE90's this. +284C isA8DA84,
        //the same mutable node count used by69AFD3..69B013. One node still
        //draws local progress but skips network reporting/service at69AF0F.
        if count == 0 || count == 1 {
            return Ok(None);
        }
        let Some(service) = service else {
            //The explicit hidden-window capture boundary skips opening real
            //sockets. Keep its report intents for the existing controlled
            //service; a normal/live attempt cannot use this exception.
            anyhow::ensure!(controlled_capture, "loading has no network service");
            return Ok(Some(Self {
                session,
                service: None,
            }));
        };
        anyhow::ensure!(
            service.transport.is_some(),
            "loading transport is not available"
        );
        Ok(Some(Self {
            session,
            service: Some(service),
        }))
    }

    pub(crate) fn milestone(
        &mut self,
        percent: i32,
        rows: &mut LoadingProgressRowSnapshot,
    ) -> anyhow::Result<()> {
        //69AF64 reads the displayed percentage, so RMG cadence is already
        //applied exactly once by the loading sink, never by this transport.
        self.session.report_network_loading_progress(percent);
        let Some(service) = self.service.as_deref_mut() else {
            return Ok(()); //The controlled service consumes those SAME intents.
        };
        service.drain_loading_reports(self.session)?;
        let now = loading_bucket();
        #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
        {
            self.pump(rows, now)?;
            self.service
                .as_deref_mut()
                .unwrap()
                .defer_loading_drain(now);
            return Ok(());
        }
        #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
        {
            let drain = crate::sim::timer::CdTimer::started(now as i32, 240);
            loop {
                self.pump(rows, loading_bucket())?;
                if self.service.as_deref().unwrap().queued_setup_count() <= 5
                    || drain.expired(loading_bucket() as i32)
                {
                    break;
                }
                //69B015..69B079 services ACK/retry traffic while the existing
                //global queue exceeds5, bounded by240 wall buckets with20ms waits.
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(())
        }
    }

    /// Resume the same bounded ACK wait after the browser has delivered events.
    #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
    pub(crate) fn resume(&mut self, rows: &mut LoadingProgressRowSnapshot) -> anyhow::Result<bool> {
        if self
            .service
            .as_deref()
            .is_none_or(|service| service.loading_drain.is_none())
        {
            return Ok(true);
        }
        let now = loading_bucket();
        self.pump(rows, now)?;
        Ok(self
            .service
            .as_deref_mut()
            .unwrap()
            .finish_loading_drain(now))
    }

    fn pump(&mut self, rows: &mut LoadingProgressRowSnapshot, now: u32) -> anyhow::Result<()> {
        let service = self
            .service
            .as_deref_mut()
            .ok_or_else(|| anyhow::anyhow!("controlled loading has no physical service"))?;
        service.flush_setup();
        let scope = service.setup_scope;
        let transport = service
            .transport
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("loading transport was retired"))?;
        transport.service(now);
        let mut global = transport.global_mut();
        //Global53FFA0/540030 already support indexed inspection/consumption.
        //Progress may pass an unrelated queued control, but that control keeps
        //its original FIFO slot for the ordinary app reducer after this phase.
        //Do not pop it into another inbox or run UI reducers on a missing
        //frontend.loading_session while the loader holds that same session.
        for index in 0..global.queued_receive_count() {
            let Some(incoming) = global.peek_received(index) else {
                continue;
            };
            if !is_progress(incoming.payload) {
                continue;
            }
            match decode_progress(self.session, scope, incoming) {
                Ok(Some((source, percent))) => {
                    let sequence = incoming.sequence;
                    let applied = super::super::loading::pump::accept_network_loading_progress_row(
                        self.session,
                        rows,
                        source,
                        percent,
                    );
                    if let Some(displayed) = rows
                        .participants
                        .get(source)
                        .and_then(|row| row.remote_progress.as_ref())
                    {
                        global.observe_loading_application(
                            source,
                            sequence,
                            percent,
                            displayed.current_percent(),
                            applied,
                        );
                    }
                }
                Ok(None) => {}
                Err(error) => log::debug!("Ignored loading progress: {error:#}"),
            }
            global.consume_received(index);
        }
        drop(global);
        //The UDP owner retires consumed entries and physically sends ACKs;
        //private startup data, unrelated globals and dedup history stay intact.
        transport.service(now);
        service.drain_loading_reports(self.session)?;
        service.flush_setup();
        service
            .transport
            .as_mut()
            .expect("loading transport retained")
            .service(now);
        Ok(())
    }
}

fn loading_bucket() -> u32 {
    super::super::match_runtime::frame_pacer::frame_bucket(
        super::super::match_runtime::frame_pacer::wall_clock_ms(),
    )
}

pub(super) fn is_progress(payload: &[u8]) -> bool {
    payload.first() == Some(&0x0f) || payload.starts_with(SCOPED_LOADING_MAGIC)
}

/// The single scope/name/source decoder for both ordinary polling and a held
/// synchronous load.48DA6E/53EF30 admit by node name and source host; LAN's
/// accepted VERA epoch/revision remains mandatory before the native payload.
pub(super) fn decode_progress(
    session: &MatchSession,
    scope: Option<SessionScope>,
    incoming: GlobalMessageRef<'_>,
) -> anyhow::Result<Option<(usize, i32)>> {
    anyhow::ensure!(incoming.product_id == 0xaa04, "different loading product");
    let progress = if session
        .network_return_context()
        .map(|context| context.session_mode())
        == Some(SessionMode::Lan)
    {
        LoadingProgressMessage::decode_scoped(
            incoming.payload,
            scope.ok_or_else(|| anyhow::anyhow!("loading has no accepted LAN scope"))?,
        )?
    } else {
        LoadingProgressMessage::decode(incoming.payload)?
    };
    let source = session.network_setup().and_then(|launch| {
        launch
            .humans
            .iter()
            .enumerate()
            .find_map(|(source, human)| {
                (human.name.eq_ignore_ascii_case(&progress.name())
                    && session
                        .network_endpoint(source)
                        .is_some_and(|endpoint| endpoint.ip() == incoming.source.ip()))
                .then_some(source)
            })
    });
    Ok(source.map(|source| (source, progress.percent())))
}

/// One encoder/recipient policy shared with the normal MatchSession request
/// drain.51-byte native progress is lossy below100 and reliable per remote at
/// terminal100 (69AF45..69B013); LAN adds its existing accepted scope envelope.
pub(super) fn progress_packets(
    session: &MatchSession,
    scope: Option<SessionScope>,
    percent: i32,
) -> anyhow::Result<Vec<(Vec<u8>, GlobalDestination)>> {
    let connected = session.network_connected_source_indices();
    if connected.len() == 1 {
        return Ok(Vec::new());
    }
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("loading report has no admitted roster"))?;
    let message = LoadingProgressMessage::new(&launch.local_player().name, percent)?;
    let bytes = if session
        .network_return_context()
        .map(|context| context.session_mode())
        == Some(SessionMode::Lan)
    {
        message
            .scoped_bytes(scope.ok_or_else(|| anyhow::anyhow!("loading report has no LAN scope"))?)
            .to_vec()
    } else {
        message.as_bytes().to_vec()
    };
    Ok(if percent < 100 {
        vec![(bytes, GlobalDestination::BROADCAST)]
    } else {
        connected
            .iter()
            .copied()
            .filter(|&source| source != launch.local_player_index)
            .map(|source| {
                let endpoint = session.network_endpoint(source).ok_or_else(|| {
                    anyhow::anyhow!("loading participant has no admitted endpoint")
                })?;
                Ok((bytes.clone(), direct(endpoint)))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    })
}

impl LanService {
    #[cfg(any(
        test,
        all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")
    ))]
    fn defer_loading_drain(&mut self, now: u32) {
        if self.queued_setup_count() > 5 && self.loading_drain.is_none() {
            // LoadingCallback69AE90's 240-bucket bound; only the wait scheduling
            // changes on the browser, not the reliable queue or its retry timers.
            self.loading_drain = Some(crate::sim::timer::CdTimer::started(now as i32, 240));
        }
    }

    #[cfg(any(
        test,
        all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")
    ))]
    fn finish_loading_drain(&mut self, now: u32) -> bool {
        if self
            .loading_drain
            .is_some_and(|deadline| self.queued_setup_count() > 5 && !deadline.expired(now as i32))
        {
            return false;
        }
        self.loading_drain = None;
        true
    }

    /// A pre-install Departure has no private connection table yet in VERA.
    /// Preserve the admitted exact endpoint and current LAN proof instead of
    /// the native48D81A private-table lookup. No display name selects a source.
    pub(super) fn loading_departure_source(
        &self,
        session: &MatchSession,
        endpoint: SocketAddrV4,
        message: &LanMessage,
    ) -> anyhow::Result<Option<usize>> {
        if message.kind() != LanMessageKind::Departure {
            return Ok(None);
        }
        let Some(source) = session.network_endpoint_source(endpoint) else {
            return Ok(None);
        };
        if !session.network_connected_source_indices().contains(&source)
            || session
                .network_setup()
                .is_none_or(|launch| source == launch.local_player_index)
        {
            return Ok(None);
        }
        if session
            .network_return_context()
            .map(|context| context.session_mode())
            == Some(SessionMode::Lan)
        {
            let proof = message
                .session_proof()?
                .ok_or_else(|| anyhow::anyhow!("loading departure has no LAN proof"))?;
            if !self.validate_scope(&proof) || !self.peer_admission_matches(endpoint, &proof) {
                return Ok(None);
            }
        }
        Ok(Some(source))
    }

    pub(super) fn queue_loading_progress(
        &mut self,
        session: &MatchSession,
        percent: i32,
    ) -> anyhow::Result<bool> {
        let packets = progress_packets(session, self.setup_scope, percent)?;
        if percent < 100 {
            for (bytes, destination) in packets {
                //Native5410F0 loss/backpressure is intentionally lossy. Only
                //the terminal reliable publication enters the existing FIFO.
                self.send(&bytes, destination, false);
            }
            Ok(true)
        } else {
            self.queue_setup(packets.into_iter())
        }
    }

    fn drain_loading_reports(&mut self, session: &mut MatchSession) -> anyhow::Result<()> {
        let mut reports = session.take_network_loading_progress_reports().into_iter();
        while let Some(percent) = reports.next() {
            match self.queue_loading_progress(session, percent) {
                Ok(true) => {}
                Ok(false) => session.report_network_loading_progress(percent),
                Err(error) => {
                    session.report_network_loading_progress(percent);
                    for pending in reports {
                        session.report_network_loading_progress(pending);
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_loading_drain_keeps_its_deadline_and_reliable_queue() {
        let mut service = super::super::tests::service();
        let peer = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 12345);
        for value in 0..6 {
            assert!(service.send(&[value], direct(peer), true));
        }
        service.defer_loading_drain(100);
        assert!(!service.finish_loading_drain(339));
        service.defer_loading_drain(339);
        assert!(
            service.finish_loading_drain(340),
            "a later callback must not extend the drain"
        );
        assert_eq!(
            service.queued_setup_count(),
            6,
            "only the reliable owner retires messages"
        );
        service.defer_loading_drain(400);
        service.clear_setup();
        assert!(
            service.loading_drain.is_none(),
            "a cancelled setup cannot retain its wait"
        );
    }

    #[test]
    fn controlled_loading_retains_report_intents_without_masking_a_broken_live_service() {
        use crate::app::match_runtime::session::NetworkLoadingRequest;

        let (mut service, mut session, _, _, _, _) = super::super::loading_test_pair();
        let mut rows = LoadingProgressRowSnapshot::from_launch_session(
            session.network_setup().unwrap(),
            crate::rules::skirmish_colors::SkirmishColorMode::VANILLA,
        );
        assert!(LoadingNetworkService::borrow(&mut session, None, false).is_err());
        {
            let mut controlled = LoadingNetworkService::borrow(&mut session, None, true)
                .unwrap()
                .unwrap();
            controlled.milestone(30, &mut rows).unwrap();
            controlled.milestone(100, &mut rows).unwrap();
        }
        assert_eq!(
            session.take_network_loading_requests(),
            [
                NetworkLoadingRequest::ReportProgress(30),
                NetworkLoadingRequest::ReportProgress(100)
            ],
            "controlled LAN/WOL drivers must still see the actual terminal report"
        );
        let transport = service.transport.take().unwrap();
        assert!(LoadingNetworkService::borrow(&mut session, Some(&mut service), true).is_err());
        service.transport = Some(transport);
        assert!(
            LoadingNetworkService::borrow(&mut session, Some(&mut service), true)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn authenticated_online_departure_reaches_loading_with_its_admitted_source() {
        use crate::app::match_runtime::session::NetworkReturnContext;
        use crate::net::udp::test_support::{
            MAX_DATAGRAM, RelayServer, SecretKey, derive_packet_key,
        };
        use crate::net::udp::{RelayTicket, relay_peer_endpoint};
        use std::net::{SocketAddr, UdpSocket};

        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket.set_nonblocking(true).unwrap();
        let SocketAddr::V4(relay_address) = socket.local_addr().unwrap() else {
            unreachable!()
        };
        let (mut host, _, _, mut guest, original_guest, _) = super::super::loading_test_pair();
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::Wol);
        session
            .accept_network_setup(original_guest.network_setup().unwrap().clone(), false, None)
            .unwrap();
        session
            .bind_network_endpoint("Host", relay_peer_endpoint(0).unwrap())
            .unwrap();
        session.prepare_wol_timing([0; 2], false).unwrap();
        session.freeze_network_start(77).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let key = SecretKey::new([42; 32]); //Local fixture key, never a deployment credential.
        let mut relay = RelayServer::new(key.clone(), 1);
        for (service, source) in [(&mut host, 0), (&mut guest, 1)] {
            let transport = service.transport.as_mut().unwrap();
            transport.clear_lan_scope();
            let ticket = RelayTicket {
                room: [11; 16],
                source,
                expires: now + 300,
                nonce: [source + 1; 16],
                player: [source + 1; 16],
                generation: 1,
            }
            .sign(&key)
            .unwrap();
            let packet_key = derive_packet_key(&key, &ticket);
            transport
                .enable_relay(
                    relay_address,
                    &ticket,
                    *packet_key.expose_for_https(),
                    [55; 32],
                )
                .unwrap();
        }
        let departure =
            LanMessage::named(crate::net::lan_messages::LanNamedMessage::Departure, "Host")
                .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut tick = 0;
        let mut sent = false;
        let incoming = loop {
            host.transport.as_mut().unwrap().service(tick);
            guest.transport.as_mut().unwrap().service(tick);
            let mut bytes = [0; MAX_DATAGRAM + 1];
            loop {
                let (len, source) = match socket.recv_from(&mut bytes) {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("fixture relay receive: {error}"),
                };
                let SocketAddr::V4(source) = source else {
                    unreachable!()
                };
                for forwarded in relay.process(source, &bytes[..len], now).unwrap() {
                    socket
                        .send_to(&forwarded.bytes, forwarded.endpoint)
                        .unwrap();
                }
            }
            if !sent
                && host.transport.as_ref().unwrap().relay_ready()
                && guest.transport.as_ref().unwrap().relay_ready()
            {
                assert!(host.send(
                    departure.as_bytes(),
                    direct(relay_peer_endpoint(1).unwrap()),
                    true
                ));
                sent = true;
            }
            if let Some(incoming) = guest.transport.as_mut().unwrap().pop_global_received() {
                break incoming;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "authenticated departure did not arrive"
            );
            tick += 1;
            std::thread::yield_now();
        };
        assert_eq!(incoming.source, relay_peer_endpoint(0).unwrap());
        assert_eq!(incoming.payload, departure.as_bytes());
        assert!(super::super::accepts_online_application_packet(
            &incoming.payload,
            false,
            true
        ));
        assert!(!super::super::accepts_online_application_packet(
            &incoming.payload,
            false,
            false
        ));
        assert!(!super::super::accepts_online_application_packet(
            b"other control",
            false,
            true
        ));
        let decoded = LanMessage::decode(&incoming.payload).unwrap();
        assert_eq!(
            guest
                .loading_departure_source(&session, incoming.source, &decoded)
                .unwrap(),
            Some(0)
        );
        assert_eq!(
            guest
                .loading_departure_source(&session, relay_peer_endpoint(1).unwrap(), &decoded)
                .unwrap(),
            None
        );
        session.return_to_network_lobby();
        assert_eq!(
            guest
                .loading_departure_source(&session, incoming.source, &decoded)
                .unwrap(),
            None
        );
    }

    #[test]
    fn held_loading_retries_terminal_progress_without_consuming_other_global_controls() {
        use crate::net::live_commands::{LiveCommandMode, LiveCommandSession, StartupAgreement};
        use std::num::NonZeroU32;

        let (mut host, host_session, _, mut guest, mut guest_session, _) =
            super::super::loading_test_pair();
        let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
        host.transport
            .as_mut()
            .unwrap()
            .global_mut()
            .observe_loading();
        guest
            .transport
            .as_mut()
            .unwrap()
            .global_mut()
            .observe_loading();
        let mut guest_rows = LoadingProgressRowSnapshot::from_launch_session(
            guest_session.network_setup().unwrap(),
            crate::rules::skirmish_colors::SkirmishColorMode::VANILLA,
        );
        let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
        let proof = host.control_proof().unwrap();
        guest.commit_peer_admission(host_address, "Host", &proof);
        let departure =
            LanMessage::named(crate::net::lan_messages::LanNamedMessage::Departure, "Host")
                .unwrap()
                .with_session_proof(&proof)
                .unwrap();
        assert_eq!(
            guest
                .loading_departure_source(&guest_session, host_address, &departure)
                .unwrap(),
            Some(0)
        );
        let mut stale = proof.clone();
        stale.scope.revision -= 1;
        let old = departure.clone().with_session_proof(&stale).unwrap();
        assert_eq!(
            guest
                .loading_departure_source(&guest_session, host_address, &old)
                .unwrap(),
            None
        );
        let foreign = SocketAddrV4::new(*host_address.ip(), host_address.port().wrapping_add(1));
        assert_eq!(
            guest
                .loading_departure_source(&guest_session, foreign, &departure)
                .unwrap(),
            None
        );
        let slots = host
            .transport
            .as_ref()
            .unwrap()
            .global()
            .available_send_slots();
        assert!(host.send(departure.as_bytes(), direct(guest_address), true));
        for _ in 1..slots {
            assert!(host.send(b"held control", direct(guest_address), false));
        }
        assert!(host.queue_loading_progress(&host_session, 100).unwrap());
        assert_eq!(
            host.pending_setup.len(),
            1,
            "full global is retained backpressure"
        );
        //The synchronous callback's bounded drain may end at this point.
        //Gameplay takes the SAME transport while its terminal report remains
        //in LanService's FIFO; the live service route must keep admitting it.
        let mut transport = host.transport.take().unwrap();
        transport.set_lan_scope(host.setup_scope.unwrap()).unwrap();
        transport
            .connections_mut()
            .add_peer(1, guest_address)
            .unwrap();
        let rate = NonZeroU32::new(5).unwrap();
        let agreement =
            StartupAgreement::new([(0, 0), (1, 1)], [0, 1], 77, 10, 30, rate, 0).unwrap();
        let mut live =
            LiveCommandSession::new(agreement, transport, 0, 77, rate, LiveCommandMode::Lan)
                .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut now = loading_bucket();
        let mut controls = 0;
        loop {
            LanService::flush_setup_queue(&mut host.pending_setup, &mut *live.global_mut());
            live.service_transport(now);
            LoadingNetworkService::borrow(&mut guest_session, Some(&mut guest), false)
                .unwrap()
                .unwrap()
                .pump(&mut guest_rows, now)
                .unwrap();
            //The ordinary global FIFO consumer still sees every other control.
            loop {
                if guest
                    .transport
                    .as_ref()
                    .unwrap()
                    .global()
                    .peek_received(0)
                    .is_some_and(|head| is_progress(head.payload))
                {
                    break;
                }
                let Some(message) = guest.transport.as_mut().unwrap().pop_global_received() else {
                    break;
                };
                if controls == 0 {
                    assert_eq!(message.payload, departure.as_bytes());
                    let decoded = LanMessage::decode(&message.payload).unwrap();
                    assert_eq!(
                        guest
                            .loading_departure_source(&guest_session, message.source, &decoded)
                            .unwrap(),
                        Some(0)
                    );
                } else {
                    assert_eq!(message.payload, b"held control");
                }
                controls += 1;
                guest.transport.as_mut().unwrap().service(now);
            }
            if host.pending_setup.is_empty()
                && live.global().queued_send_count() == 0
                && guest_rows.participants[0]
                    .remote_progress
                    .as_ref()
                    .unwrap()
                    .current_percent()
                    == 100.0
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "terminal progress/ACK did not retire"
            );
            now = now.wrapping_add(1);
            std::thread::yield_now();
        }
        assert_eq!(controls, slots);
        assert!(host.pending_setup.is_empty());
        assert_eq!(live.global().queued_send_count(), 0);
        let log = live.global().loading_observation().unwrap().clone();
        assert!(!log.overflowed());
        let (sequence, destination) = log
            .records()
            .iter()
            .find_map(|r| match r {
                crate::net::global::LoadingObservation::Admitted {
                    sequence,
                    destination,
                    percent: 100,
                    reliable: true,
                } => Some((*sequence, *destination)),
                _ => None,
            })
            .expect("actual terminal admission");
        assert!(log.records().iter().any(|r|matches!(r,crate::net::global::LoadingObservation::Retired{sequence:s,destination:d} if *s==sequence && *d==destination)),
            "a zero queue alone is not ACK proof");
        let records = guest
            .transport
            .as_ref()
            .unwrap()
            .global()
            .loading_observation()
            .unwrap()
            .records()
            .to_vec();
        assert!(records.iter().any(|r| matches!(
            r,
            crate::net::global::LoadingObservation::Applied {
                source: 0,
                percent: 100,
                displayed,
                applied: true,
                ..
            } if *displayed == 100.0
        )));

        //A delayed native lossy report can arrive AFTER100 was ACKed. It
        //must reach the existing row admission owner without reopening wait.
        let delayed = progress_packets(&host_session, host.setup_scope, 98).unwrap();
        for (bytes, destination) in delayed {
            assert!(live.send_global(&bytes, false, destination));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            live.service_transport(now);
            guest.transport.as_mut().unwrap().service(now);
            if let Some(incoming) = guest.transport.as_ref().unwrap().global().peek_received(0) {
                assert_eq!(
                    decode_progress(&guest_session, guest.setup_scope, incoming).unwrap(),
                    Some((0, 98)),
                    "the delayed real datagram must actually arrive"
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "delayed98 did not arrive"
            );
            now = now.wrapping_add(1);
            std::thread::yield_now();
        }
        LoadingNetworkService::borrow(&mut guest_session, Some(&mut guest), false)
            .unwrap()
            .unwrap()
            .pump(&mut guest_rows, now)
            .unwrap();
        assert_eq!(
            guest_rows.participants[0]
                .remote_progress
                .as_ref()
                .unwrap()
                .current_percent(),
            100.0
        );
        assert_eq!(
            guest
                .transport
                .as_ref()
                .unwrap()
                .global()
                .queued_receive_count(),
            0
        );
        assert!(
            guest
                .transport
                .as_ref()
                .unwrap()
                .global()
                .loading_observation()
                .unwrap()
                .records()
                .iter()
                .any(|r| matches!(
                    r,
                    crate::net::global::LoadingObservation::Applied {
                        source: 0,
                        percent: 98,
                        displayed,
                        applied: false,
                        ..
                    } if *displayed == 100.0
                )),
            "actual delayed98 was observed after the same row owner retained terminal100"
        );
        let mut fresh = LoadingProgressRowSnapshot::from_launch_session(
            guest_session.network_setup().unwrap(),
            crate::rules::skirmish_colors::SkirmishColorMode::VANILLA,
        );
        assert!(
            fresh.update_remote_progress(0, 98),
            "new attempt resets terminal state"
        );

        guest_session.return_to_network_lobby();
        let progress = LoadingProgressMessage::new("Host", 50)
            .unwrap()
            .scoped_bytes(guest.setup_scope.unwrap());
        let source = live.primary_addr().unwrap();
        let decoded = decode_progress(
            &guest_session,
            guest.setup_scope,
            GlobalMessageRef {
                source,
                product_id: 0xaa04,
                sequence: 1,
                payload: &progress,
            },
        )
        .unwrap();
        assert!(
            decoded.is_none(),
            "retired frozen admission cannot bind another progress row"
        );
        assert!(
            LoadingNetworkService::borrow(&mut guest_session, Some(&mut guest), false)
                .unwrap()
                .is_none()
        );
    }
}
