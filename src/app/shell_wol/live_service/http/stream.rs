//! The lobby's event stream (docs/lobby-protocol.md): one WebSocket at a time,
//! read on its own thread, which hands each frame to the serial worker. The
//! worker keeps the cursor and polls whenever no stream is up. The bearer goes
//! only into the upgrade request's header.

use crate::util::time::{Duration, Instant};
use serde_json::{Value, json};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

/// The lobby answers each ack with a frame, so a stream that stays silent
/// for `SILENCE` is gone. Idle sessions expire after 120 s without one.
const ACK_INTERVAL: Duration = Duration::from_secs(15);
const SILENCE: Duration = Duration::from_secs(25);
/// How long the thread waits when its non-blocking socket has nothing.
const IDLE: Duration = Duration::from_millis(20);
const RETRY: Duration = Duration::from_secs(5);
const MAX_RETRY: Duration = Duration::from_secs(300);
const TIMEOUT: Duration = Duration::from_secs(10);

enum Event {
    Up,
    Frame(Value),
}

struct Open {
    token: String,
    live: bool,
    events: Receiver<Event>,
    stop: Arc<AtomicBool>,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// The worker's stream, which serves only the bearer it was opened with.
pub(super) struct Streams {
    /// `ws://` or `wss://` and the lobby's base without its scheme.
    origin: Option<(bool, String)>,
    open: Option<Open>,
    retry: Instant,
    failures: u32,
}

impl Streams {
    pub(super) fn new(base: &str) -> Self {
        let origin = match base.split_once("://") {
            Some(("https", rest)) => Some((true, rest.to_owned())),
            Some(("http", rest)) => Some((false, rest.to_owned())),
            _ => None,
        };
        Self {
            origin,
            open: None,
            retry: Instant::now(),
            failures: 0,
        }
    }

    /// True while frames take the place of event polls.
    pub(super) fn live(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.live)
    }

    /// The next frame of a stream serving `token`. A stream that serves
    /// another bearer is dropped, and one that ended is retried later.
    pub(super) fn next(&mut self, token: Option<&str>) -> Option<Value> {
        let open = self.open.as_mut()?;
        if token != Some(open.token.as_str()) {
            self.open = None;
            return None;
        }
        loop {
            match open.events.try_recv() {
                Ok(Event::Up) => {
                    open.live = true;
                    self.failures = 0;
                }
                Ok(Event::Frame(frame)) => return Some(frame),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    self.close();
                    return None;
                }
            }
        }
    }

    /// Ends the stream and waits longer before each failed one's successor.
    pub(super) fn close(&mut self) {
        self.open = None;
        self.retry = Instant::now() + (RETRY * 2u32.pow(self.failures.min(6))).min(MAX_RETRY);
        self.failures = self.failures.saturating_add(1);
    }

    /// Opens a stream from `cursor` for `token` once none is open and the
    /// retry is due.
    pub(super) fn open(&mut self, token: Option<&str>, cursor: u64, stopping: &Arc<AtomicBool>) {
        let (Some((secure, base)), Some(token), None) = (&self.origin, token, &self.open) else {
            return;
        };
        if Instant::now() < self.retry {
            return;
        }
        let url = format!(
            "{}://{base}/v1/events/stream?after={cursor}",
            if *secure { "wss" } else { "ws" }
        );
        let (sender, events) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (stopped, stopping, bearer) =
            (Arc::clone(&stop), Arc::clone(stopping), token.to_owned());
        let secure = *secure;
        let started = std::thread::Builder::new()
            .name("vera-lobby-stream".into())
            .spawn(move || {
                let stopped =
                    || stopped.load(Ordering::Acquire) || stopping.load(Ordering::Acquire);
                let _ = run(&url, secure, &bearer, cursor, &sender, &stopped);
            });
        if started.is_ok() {
            self.open = Some(Open {
                token: token.to_owned(),
                live: false,
                events,
                stop,
            });
        } else {
            self.close();
        }
    }
}

trait Transport: Read + Write + Send {
    fn tcp(&self) -> &TcpStream;
}

impl Transport for TcpStream {
    fn tcp(&self) -> &TcpStream {
        self
    }
}

impl Transport for rustls::StreamOwned<rustls::ClientConnection, TcpStream> {
    fn tcp(&self) -> &TcpStream {
        &self.sock
    }
}

/// A non-blocking socket's "not yet": a read found no data, or a write stays
/// buffered for the next one.
fn would_block(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(error) if error.kind() == ErrorKind::WouldBlock)
}

/// Forwards frames until the stream fails or closes, or `stopped()`.
fn run(
    url: &str,
    secure: bool,
    token: &str,
    cursor: u64,
    events: &Sender<Event>,
    stopped: &dyn Fn() -> bool,
) -> Result<(), ()> {
    let mut socket = connect(url, secure, token)?;
    events.send(Event::Up).map_err(drop)?;
    let (mut ack, mut heard, mut acked) = (cursor, Instant::now(), Instant::now());
    while !stopped() && heard.elapsed() < SILENCE {
        match socket.read() {
            Ok(Message::Text(text)) => {
                let frame: Value = serde_json::from_str(text.as_str()).map_err(drop)?;
                ack = frame["cursor"].as_u64().ok_or(())?;
                heard = Instant::now();
                events.send(Event::Frame(frame)).map_err(drop)?;
            }
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Ok(Message::Binary(_) | Message::Close(_)) => break,
            Err(error) if would_block(&error) => std::thread::sleep(IDLE),
            Err(_) => return Err(()),
        }
        if acked.elapsed() >= ACK_INTERVAL {
            let sent = socket.send(Message::text(json!({ "ack": ack }).to_string()));
            if let Err(error) = sent
                && !would_block(&error)
            {
                return Err(());
            }
            acked = Instant::now();
        }
    }
    let _ = socket.close(None);
    let _ = socket.flush();
    Ok(())
}

fn connect(
    url: &str,
    secure: bool,
    token: &str,
) -> Result<tungstenite::WebSocket<Box<dyn Transport>>, ()> {
    let mut request = url.into_client_request().map_err(drop)?;
    // Uri keeps an IPv6 literal's brackets.
    let host = request.uri().host().ok_or(())?;
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = request
        .uri()
        .port_u16()
        .unwrap_or(if secure { 443 } else { 80 });
    let headers = request.headers_mut();
    headers.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(drop)?,
    );
    headers.insert("User-Agent", HeaderValue::from_static(super::USER_AGENT));
    // Unlike ureq, the stream ignores HTTP(S)_PROXY: behind a proxy the game
    // polls.
    let tcp = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(drop)?
        .find_map(|address| TcpStream::connect_timeout(&address, TIMEOUT).ok())
        .ok_or(())?;
    tcp.set_read_timeout(Some(TIMEOUT)).map_err(drop)?;
    tcp.set_write_timeout(Some(TIMEOUT)).map_err(drop)?;
    tcp.set_nodelay(true).map_err(drop)?;
    let transport: Box<dyn Transport> = if secure {
        // ureq's own TLS: rustls with the ring provider and webpki roots.
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(drop)?
        .with_root_certificates(rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        })
        .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(host).map_err(drop)?;
        let connection = rustls::ClientConnection::new(Arc::new(config), name).map_err(drop)?;
        Box::new(rustls::StreamOwned::new(connection, tcp))
    } else {
        Box::new(tcp)
    };
    let limit = super::MAX_BODY_BYTES as usize;
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(limit))
        .max_frame_size(Some(limit));
    let (socket, _) =
        tungstenite::client::client_with_config(request, transport, Some(config)).map_err(drop)?;
    // Windows leaves a socket whose read timed out in an undefined state, so
    // the thread reads without blocking instead of with a short timeout.
    socket.get_ref().tcp().set_nonblocking(true).map_err(drop)?;
    Ok(socket)
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_stream_forwards_its_frames_and_its_end_returns_the_worker_to_polls() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let lobby = std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut upgrade = None;
            let mut socket = tungstenite::accept_hdr(
                tcp,
                |request: &tungstenite::handshake::server::Request, response| {
                    upgrade = Some((
                        request.uri().to_string(),
                        request.headers()["authorization"]
                            .to_str()
                            .unwrap()
                            .to_owned(),
                    ));
                    Ok(response)
                },
            )
            .unwrap();
            socket
                .send(Message::text(
                    r#"{"events":[{"sequence":8,"kind":"chat","payload":{}}],"cursor":8}"#,
                ))
                .unwrap();
            socket.close(None).unwrap();
            while socket.read().is_ok() {}
            upgrade.unwrap()
        });
        let mut streams = Streams::new(&format!("http://{address}"));
        streams.open(Some("bearer"), 7, &Arc::new(AtomicBool::new(false)));
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut frames = Vec::new();
        while streams.open.is_some() && Instant::now() < deadline {
            frames.extend(streams.next(Some("bearer")));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            lobby.join().unwrap(),
            (
                "/v1/events/stream?after=7".to_owned(),
                "Bearer bearer".to_owned()
            )
        );
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["cursor"], 8);
        assert!(!streams.live());
        assert!(streams.retry > Instant::now(), "the next stream waits");
        streams.open(Some("bearer"), 8, &Arc::new(AtomicBool::new(false)));
        assert!(streams.open.is_none());
    }
}
