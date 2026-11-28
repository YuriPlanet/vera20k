//! UDP gameplay relay. No retail assets, simulation engine, client identity
//! database or lobby bearer credentials are present in this process.

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use vera20k_relay_protocol::{
    ForwardDatagram, MAX_DATAGRAM, OrderingCounts, RelayError, RelayServer, SecretKey,
};

/// Aggregate operational counts only. No request, endpoint, room identity,
/// authentication material or error body enters the relay's journal.
#[derive(Default)]
struct TrafficCounts {
    received: u64,
    forwarded: u64,
    send_errors: u64,
    malformed: u64,
    authentication: u64,
    expired: u64,
    replay: u64,
    unregistered: u64,
    full: u64,
    rate_limited: u64,
    counter_exhausted: u64,
    entropy_unavailable: u64,
    // Ordering push datagrams and their bytes; each also counts in `forwarded`
    // or `send_errors`.
    pushed: u64,
    pushed_bytes: u64,
}

impl TrafficCounts {
    fn reject(&mut self, error: RelayError) {
        let count = match error {
            RelayError::Malformed => &mut self.malformed,
            RelayError::Authentication => &mut self.authentication,
            RelayError::Expired => &mut self.expired,
            RelayError::Replay => &mut self.replay,
            RelayError::Unregistered => &mut self.unregistered,
            RelayError::Full => &mut self.full,
            RelayError::RateLimited => &mut self.rate_limited,
            RelayError::CounterExhausted => &mut self.counter_exhausted,
            RelayError::EntropyUnavailable => &mut self.entropy_unavailable,
        };
        *count += 1;
    }

    fn report(&self, seconds: u64, rooms: usize, ordering: OrderingCounts, session_alerts: u64) {
        eprintln!(
            "VERA20k relay interval_s={seconds} rooms={rooms} slowdowns={} raises={} catching_up_s={} received={} forwarded={} send_errors={} malformed={} authentication={} expired={} replay={} unregistered={} full={} rate_limited={} counter_exhausted={} entropy_unavailable={} pushed={} pushed_bytes={} pull_replies={} desyncs={} start_mismatches={} outcome_handoff_drops={} push_packets={:?} push_bytes={:?} piggybacked_records={} down_lost={} retransmitted_records={} readers_avg={:?} offline_unsent={} session_alerts={session_alerts}",
            ordering.slowdowns,
            ordering.raises,
            ordering.catching_up_s,
            self.received,
            self.forwarded,
            self.send_errors,
            self.malformed,
            self.authentication,
            self.expired,
            self.replay,
            self.unregistered,
            self.full,
            self.rate_limited,
            self.counter_exhausted,
            self.entropy_unavailable,
            self.pushed,
            self.pushed_bytes,
            ordering.pull_replies,
            ordering.desyncs,
            ordering.start_mismatches,
            ordering.outcome_handoff_drops,
            ordering.push_packets,
            ordering.push_bytes,
            ordering.piggybacked_records,
            ordering.down_lost_packets,
            ordering.retransmitted_records,
            // Average subscribed readers per state: good, lossy, congested,
            // offline (spec §10.2).
            ordering
                .reader_states
                .map(|seen| seen as f64 / ordering.reader_passes.max(1) as f64),
            ordering.offline_unsent_packets,
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = load_key()?;
    let bind = std::env::var("VERA_RELAY_BIND").unwrap_or_else(|_| "0.0.0.0:5000".to_string());
    let socket = UdpSocket::bind(&bind)?;
    socket.set_read_timeout(Some(Duration::from_millis(6)))?;
    let state_dir = std::env::var_os("STATE_DIRECTORY")
        .or_else(|| std::env::var_os("VERA_RELAY_STATE_DIR"))
        .map(PathBuf::from)
        .ok_or("relay journal requires STATE_DIRECTORY or VERA_RELAY_STATE_DIR")?;
    let mut server = RelayServer::with_journal(SecretKey::new(key), 1024, &state_dir)?;
    let mut reporter = vera20k_relay::Reporter::configured(&state_dir)?;
    if reporter.is_some() {
        server.enable_federation_outcomes();
    }
    let mut buffer = [0u8; MAX_DATAGRAM + 1];
    let mut counts = TrafficCounts::default();
    let mut reported_at = Instant::now();
    let clock_origin = Instant::now();
    // The millisecond the last pass over every room ended.
    let mut passed_ms = None;
    eprintln!("VERA20k relay listening on {}", socket.local_addr()?);
    loop {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let monotonic_ms = clock_origin.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        // A pass costs time in every room, so it starts a millisecond after
        // the last one ended, even when a full relay's pass takes longer. No
        // datagram adds one: the pushes or link packets an accepted one may
        // owe go out with the next pass, as a sealed frame's do.
        if passed_ms.is_none_or(|passed| monotonic_ms > passed) {
            // Clock advancement is independent of datagram arrivals and readers.
            server.advance_clock(monotonic_ms, now);
            // Federated relays exchange what they just sealed before players read it.
            send_datagrams(
                &socket,
                &mut counts,
                server.drain_federation(monotonic_ms, now),
            );
            if let Some(reporter) = &mut reporter {
                reporter.poll(&mut server);
            }
            let pushed = server.drain_ordering_push(monotonic_ms, now);
            counts.pushed += pushed.len() as u64;
            counts.pushed_bytes += pushed
                .iter()
                .map(|datagram| datagram.bytes.len() as u64)
                .sum::<u64>();
            send_datagrams(&socket, &mut counts, pushed);
            passed_ms = Some(clock_origin.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
        }
        let elapsed = reported_at.elapsed().as_secs();
        if elapsed >= 60 {
            counts.report(
                elapsed,
                server.room_count(),
                server.take_ordering_counts(),
                server.take_session_alerts(),
            );
            counts = TrafficCounts::default();
            reported_at = Instant::now();
        }
        let (length, source) = match socket.recv_from(&mut buffer) {
            Ok(packet) => packet,
            Err(error) => match receive_error(&error, cfg!(windows)) {
                ReceiveError::Continue => continue,
                ReceiveError::MalformedDatagram => {
                    counts.received += 1;
                    counts.malformed += 1;
                    continue;
                }
                ReceiveError::Fatal => return Err(error.into()),
            },
        };
        counts.received += 1;
        let SocketAddr::V4(source) = source else {
            counts.malformed += 1;
            continue;
        };
        if length > MAX_DATAGRAM {
            counts.malformed += 1;
            continue;
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let monotonic_ms = clock_origin.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        // Rejections expose no packet, key, ticket, address or player data.
        match server.process_at(source, &buffer[..length], now, monotonic_ms) {
            Ok(forward) => send_datagrams(&socket, &mut counts, forward),
            Err(error) => counts.reject(error),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ReceiveError {
    Continue,
    MalformedDatagram,
    Fatal,
}

fn receive_error(error: &io::Error, windows: bool) -> ReceiveError {
    if matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    ) {
        return ReceiveError::Continue;
    }
    if windows {
        // These Winsock recvfrom results have datagram-specific meanings.
        // https://learn.microsoft.com/windows/win32/api/winsock2/nf-winsock2-recvfrom
        const WSAEMSGSIZE: i32 = 10040;
        const WSAECONNRESET: i32 = 10054;
        match error.raw_os_error() {
            // A truncated packet is discarded without parsing its contents.
            Some(WSAEMSGSIZE) => return ReceiveError::MalformedDatagram,
            // Datagram delivery notices, not a received packet or a closed socket.
            Some(WSAECONNRESET) => return ReceiveError::Continue,
            _ => {}
        }
    }
    ReceiveError::Fatal
}

fn send_datagrams(socket: &UdpSocket, counts: &mut TrafficCounts, datagrams: Vec<ForwardDatagram>) {
    for datagram in datagrams {
        match socket.send_to(&datagram.bytes, datagram.endpoint) {
            Ok(_) => counts.forwarded += 1,
            Err(_) => counts.send_errors += 1,
        }
    }
}

fn load_key() -> Result<[u8; 32], Box<dyn std::error::Error>> {
    // systemd LoadCredential keeps signing material out of environment values
    // and process arguments. A selected credential file must be readable;
    // failure never falls back to a different secret source.
    let path = std::env::var_os("VERA_RELAY_TICKET_KEY_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("CREDENTIALS_DIRECTORY")
                .map(|directory| PathBuf::from(directory).join("relay-ticket-key"))
        });
    let text = if let Some(path) = path {
        std::fs::read_to_string(path).map_err(|_| "unable to read relay signing credential file")?
    } else {
        // Local development only; deployed units use LoadCredential above.
        std::env::var("VERA_RELAY_TICKET_KEY")
            .map_err(|_| "relay signing credential file is not configured")?
    };
    Ok(decode_key(text.trim())?)
}

fn decode_key(text: &str) -> Result<[u8; 32], &'static str> {
    if text.len() != 64 {
        return Err("relay signing key must be exactly 64 hexadecimal digits");
    }
    let mut key = [0u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        fn digit(byte: u8) -> Result<u8, &'static str> {
            match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                b'A'..=b'F' => Ok(byte - b'A' + 10),
                _ => Err("relay signing key must contain only hexadecimal digits"),
            }
        }
        key[index] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_udp_per_datagram_errors_are_not_fatal() {
        for (code, expected) in [
            (10040, ReceiveError::MalformedDatagram),
            (10054, ReceiveError::Continue),
        ] {
            let error = io::Error::from_raw_os_error(code);
            assert_eq!(receive_error(&error, true), expected);
            assert_eq!(receive_error(&error, false), ReceiveError::Fatal);
        }
    }

    #[test]
    fn retries_and_fatal_errors_stay_narrow() {
        for windows in [false, true] {
            for kind in [
                io::ErrorKind::WouldBlock,
                io::ErrorKind::TimedOut,
                io::ErrorKind::Interrupted,
            ] {
                assert_eq!(
                    receive_error(&io::Error::from(kind), windows),
                    ReceiveError::Continue
                );
            }
            for error in [
                io::Error::from(io::ErrorKind::PermissionDenied),
                io::Error::from(io::ErrorKind::ConnectionReset),
                io::Error::from_raw_os_error(10038), // WSAENOTSOCK
                io::Error::from_raw_os_error(10052), // WSAENETRESET stays fatal
                io::Error::from_raw_os_error(10058), // WSAESHUTDOWN
            ] {
                assert_eq!(receive_error(&error, windows), ReceiveError::Fatal);
            }
        }
    }
}
