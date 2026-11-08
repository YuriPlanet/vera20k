//! Read-only, loopback dashboard transport. The app publishes bounded JSON
//! snapshots; browsers can only receive them in dashboard-only builds. The
//! separate `debug-tools` feature admits bounded commands only while the app
//! explicitly enables an offline world, and binds them to that world epoch.
//!
//! Admission follows RFC 6455 with a per-server OS-random credential and exact
//! Host/Origin checks. Only fixed public dashboard assets are served over HTTP.
//! The accept thread owns every worker/socket, caps concurrency, reaps finished
//! workers, and shuts down and joins the remainder when the server is dropped.
//! References: https://www.rfc-editor.org/rfc/rfc6455.html and
//! https://docs.rs/getrandom/0.3.4/getrandom/fn.fill.html.

use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

const WS_MAGIC: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const PROTOCOL: &str = "vera-telemetry.v1";
const AUTH_PREFIX: &str = "vera-auth.";
const MAX_HTTP_HEAD: usize = 8 * 1024;
const MAX_HTTP_LINE: usize = 2048;
const MAX_HEADERS: usize = 32;
const MAX_WORKERS: usize = 8;
const CLIENT_QUEUE_DEPTH: usize = 4;
const MAX_SNAPSHOT: usize = 8 * 1024 * 1024;
const MAX_STATIC_FILE: usize = 8 * 1024 * 1024;
const MAX_INBOUND_BUFFER: usize = 2048;
const MAX_CONTROL_BATCH: usize = 8;
const MAX_CONTROLS_PER_SECOND: usize = 128;
const POLL: Duration = Duration::from_millis(50);
const IO_DEADLINE: Duration = Duration::from_secs(2);
/// Largest binary frame the app may publish. The picture stream shares the
/// snapshot's ceiling; its own header + RGBA tile is bounded by the producer.
const MAX_SCREEN_FRAME: usize = 8 * 1024 * 1024;
/// A picture stream keeps at most the newest tiles per client. Dropping a
/// tile never drops the JSON snapshots, which have their own queue.
/// Whole-map frames are complete pictures (~5 MB each), not small scan tiles;
/// keep the per-client backlog short so a slow viewer cannot retain dozens of
/// megabytes of stale frames.
const SCREEN_QUEUE_DEPTH: usize = 3;
/// Audio blocks are small (40 ms of stereo PCM16, ~7.7 KB), so a deeper queue
/// costs little and rides out a socket write that waits on a slow client
/// without the listener hearing a gap.
const AUDIO_QUEUE_DEPTH: usize = 12;
/// Largest audio block the app may publish.
const MAX_AUDIO_FRAME: usize = 64 * 1024;
/// Subscription requests only change which frames this socket receives. They
/// never reach the match state, so both dashboard-only and debug builds admit
/// them; any other client data frame is still rejected at the header.
const SCREEN_ON: &[u8] = b"vera-screen.on";
const SCREEN_OFF: &[u8] = b"vera-screen.off";
const AUDIO_ON: &[u8] = b"vera-audio.on";
const AUDIO_OFF: &[u8] = b"vera-audio.off";
/// Release the pre-match hold a `RA2_QUICKPLAY_HOLD=1` launch takes. It only
/// ever clears a hold this process set, so a match that already runs is
/// untouched.
const MATCH_START: &[u8] = b"vera-match.start";
/// A subscribed socket must drain its picture queue faster than the HTTP
/// accept poll, or a 60 Hz producer would stall behind a 50 ms read.
const WRITER_POLL: Duration = Duration::from_millis(2);

pub const ENV_ADDR: &str = "VERA_TELEMETRY_ADDR";
pub const ENV_WEB_ROOT: &str = "VERA_TELEMETRY_WEB";
pub const DEFAULT_WEB_ROOT: &str = "tools/telemetry-dashboard";
pub const DEFAULT_PORT: u16 = 9741;

/// RFC 6455 accept digest; SHA-1 here is protocol framing, not authentication.
pub fn websocket_accept_key(client_key: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(client_key.as_bytes());
    hasher.update(WS_MAGIC.as_bytes());
    base64_encode(&hasher.finalize())
}

pub fn encode_text_frame(payload: &str) -> Vec<u8> {
    encode_ws_frame(0x1, payload.as_bytes())
}

/// RFC 6455 binary frame. The dashboard picture stream uses these for live
/// map tiles; JSON snapshots stay on the text opcode.
pub fn encode_binary_frame(payload: &[u8]) -> Vec<u8> {
    encode_ws_frame(0x2, payload)
}

fn encode_ws_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 10);
    frame.push(0x80 | opcode);
    match payload.len() {
        len @ 0..=125 => frame.push(len as u8),
        len @ 126..=65535 => {
            frame.push(126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        }
        len => {
            frame.push(127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(payload);
    frame
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        out.push(ALPHABET[(value >> 18) as usize & 63] as char);
        out.push(ALPHABET[(value >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(value >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[value as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

// Deliberately no Debug implementation: the secret never enters logs/snapshots.
struct Admission {
    authority: String,
    localhost: String,
    secret: [u8; 32],
}

impl Admission {
    fn host_allowed(&self, host: &str) -> bool {
        host == self.authority || host == self.localhost
    }

    fn bootstrap_url(&self) -> String {
        let mut url = format!("http://{}/#token=", self.authority);
        for byte in self.secret {
            write!(url, "{byte:02x}").expect("write to String");
        }
        url
    }

    fn authenticates(&self, protocols: &str) -> bool {
        let mut protocol = false;
        let mut credential = None;
        for item in protocols.split(',').map(str::trim) {
            if item == PROTOCOL && !protocol {
                protocol = true;
            } else if let Some(token) = item.strip_prefix(AUTH_PREFIX)
                && credential.is_none()
            {
                credential = Some(token);
            } else {
                return false;
            }
        }
        let Some(token) = credential.filter(|token| token.len() == 64) else {
            return false;
        };
        let mut decoded = [0_u8; 32];
        for (byte, pair) in decoded.iter_mut().zip(token.as_bytes().chunks_exact(2)) {
            let hex = |value: u8| match value {
                b'0'..=b'9' => Some(value - b'0'),
                b'a'..=b'f' => Some(value - b'a' + 10),
                _ => None,
            };
            let (Some(high), Some(low)) = (hex(pair[0]), hex(pair[1])) else {
                return false;
            };
            *byte = high * 16 + low;
        }
        // Compare every decoded byte, without a secret-dependent early return.
        let difference = decoded
            .iter()
            .zip(self.secret)
            .fold(0, |difference, (a, b)| difference | (a ^ b));
        protocol && difference == 0
    }
}

struct Client {
    id: u64,
    sender: SyncSender<Arc<[u8]>>,
    /// Toggled by this socket's own writer on a subscription request. The
    /// publisher reads it to decide whether to spend a frame on the picture.
    screen: Arc<AtomicBool>,
    /// Tiles have their own queue: a burst of picture frames can never push a
    /// JSON snapshot out of `sender`.
    screen_sender: SyncSender<Arc<[u8]>>,
    /// Toggled by this socket's own writer on a subscription request. Audio is
    /// much smaller than a picture but must never delay one, so it has its own
    /// queue as well.
    audio: Arc<AtomicBool>,
    audio_sender: SyncSender<Arc<[u8]>>,
}

struct Shared {
    clients: Mutex<Vec<Client>>,
    stop: AtomicBool,
    /// Connections accepted since the server started. A publisher compares it
    /// with the value it last saw to know a client attached meanwhile.
    attached: AtomicU64,
    /// Set by a dashboard that pressed "start"; the app swaps it back to false
    /// when it releases the pre-match hold.
    match_start: AtomicBool,
    admission: Admission,
    web_root: PathBuf,
    #[cfg(feature = "debug-tools")]
    commands: Mutex<crate::app::debug_tools::CommandInbox>,
}

impl Shared {
    #[cfg(feature = "debug-tools")]
    fn commands(&self) -> MutexGuard<'_, crate::app::debug_tools::CommandInbox> {
        self.commands
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn debug_allowed(&self) -> bool {
        #[cfg(feature = "debug-tools")]
        {
            self.commands().enabled()
        }
        #[cfg(not(feature = "debug-tools"))]
        {
            false
        }
    }

    fn clients(&self) -> MutexGuard<'_, Vec<Client>> {
        self.clients
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

/// The picture fan-out, detached from the server handle so a producer thread
/// can keep publishing while the app keeps its frame. Same back-pressure as
/// [`TelemetryServer::publish_binary`]: a full queue drops a picture.
#[derive(Clone)]
pub struct PictureSink {
    shared: Arc<Shared>,
}

impl PictureSink {
    pub fn publish_binary(&self, payload: &[u8]) {
        publish_binary_to(&self.shared, payload);
    }
}

fn publish_binary_to(shared: &Shared, payload: &[u8]) {
    if payload.len() > MAX_SCREEN_FRAME || shared.stopped() {
        return;
    }
    let frame: Arc<[u8]> = encode_binary_frame(payload).into();
    shared.clients().retain(|client| {
        if !client.screen.load(Ordering::Acquire) {
            return true;
        }
        !matches!(
            client.screen_sender.try_send(Arc::clone(&frame)),
            Err(TrySendError::Disconnected(_))
        )
    });
}

/// The audio fan-out. Small frames and a shallower cadence than pictures, but
/// the same rule: a full queue drops the block rather than delaying the mix.
#[derive(Clone)]
pub struct AudioSink {
    shared: Arc<Shared>,
}

impl AudioSink {
    pub fn publish(&self, payload: &[u8]) {
        publish_audio_to(&self.shared, payload);
    }

    /// Whether any attached dashboard wants the live audio stream.
    pub fn wanted(&self) -> bool {
        has_audio_clients(&self.shared)
    }
}

fn publish_audio_to(shared: &Shared, payload: &[u8]) {
    if payload.len() > MAX_AUDIO_FRAME || shared.stopped() {
        return;
    }
    let frame: Arc<[u8]> = encode_binary_frame(payload).into();
    shared.clients().retain(|client| {
        if !client.audio.load(Ordering::Acquire) {
            return true;
        }
        !matches!(
            client.audio_sender.try_send(Arc::clone(&frame)),
            Err(TrySendError::Disconnected(_))
        )
    });
}

fn has_audio_clients(shared: &Shared) -> bool {
    shared
        .clients()
        .iter()
        .any(|client| client.audio.load(Ordering::Acquire))
}

pub struct TelemetryServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
    accept_thread: Option<thread::JoinHandle<()>>,
}

impl TelemetryServer {
    pub fn start(addr: SocketAddr, web_root: PathBuf) -> io::Result<Self> {
        if !addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "dashboard requires a loopback address",
            ));
        }
        let web_root = web_root.canonicalize()?;
        if !web_root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "dashboard root is not a directory",
            ));
        }
        let mut secret = [0_u8; 32];
        getrandom::fill(&mut secret)
            .map_err(|_| io::Error::other("dashboard credential generation failed"))?;
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            clients: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
            attached: AtomicU64::new(0),
            match_start: AtomicBool::new(false),
            admission: Admission {
                authority: addr.to_string(),
                localhost: format!("localhost:{}", addr.port()),
                secret,
            },
            web_root,
            #[cfg(feature = "debug-tools")]
            commands: Default::default(),
        });
        let accept_shared = Arc::clone(&shared);
        let accept_thread = thread::Builder::new()
            .name("telemetry-accept".into())
            .spawn(move || accept_loop(listener, accept_shared))?;
        Ok(Self {
            addr,
            shared,
            accept_thread: Some(accept_thread),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Display locally once; never send this URL through the telemetry log tee.
    pub fn bootstrap_url(&self) -> String {
        self.shared.admission.bootstrap_url()
    }

    /// Bounded, nonblocking fanout. Full queues drop a snapshot, not the client.
    pub fn publish(&self, json: String) {
        if json.len() > MAX_SNAPSHOT || self.shared.stopped() {
            return;
        }
        let frame: Arc<[u8]> = encode_text_frame(&json).into();
        self.shared
            .clients()
            .retain(|client| match client.sender.try_send(Arc::clone(&frame)) {
                Ok(()) | Err(TrySendError::Full(_)) => true,
                Err(TrySendError::Disconnected(_)) => false,
            });
    }

    /// Publish one live-map picture tile to subscribed sockets only. A full
    /// queue drops the tile, never the client and never a JSON snapshot.
    pub fn publish_binary(&self, payload: &[u8]) {
        publish_binary_to(&self.shared, payload);
    }

    /// A clone of the publish path usable from another thread. The whole-map
    /// stream encodes and pushes its pictures on a worker so a 1080p frame's
    /// swizzle and 8 MB copy never sit in the presentation thread's budget.
    pub fn picture_sink(&self) -> PictureSink {
        PictureSink {
            shared: Arc::clone(&self.shared),
        }
    }

    /// A clone of the audio publish path, for the pump thread that drains the
    /// shared mixer's PCM ring.
    pub fn audio_sink(&self) -> AudioSink {
        AudioSink {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Whether any attached dashboard currently wants live audio. The pump
    /// stops draining the mixer's ring when nothing listens, so an idle
    /// dashboard costs nothing.
    pub fn has_audio_clients(&self) -> bool {
        has_audio_clients(&self.shared)
    }

    /// Take a pending "start the match" request. The app calls this once per
    /// frame; `true` means a dashboard pressed the button since the last call.
    pub fn take_match_start(&self) -> bool {
        self.shared.match_start.swap(false, Ordering::AcqRel)
    }

    /// Whether any attached dashboard currently wants live map pictures. The
    /// app gates the extra render pass on this, so an idle dashboard costs
    /// nothing beyond the JSON snapshots.
    pub fn has_screen_clients(&self) -> bool {
        self.shared
            .clients()
            .iter()
            .any(|client| client.screen.load(Ordering::Acquire))
    }

    pub fn has_clients(&self) -> bool {
        !self.shared.clients().is_empty()
    }

    #[cfg(feature = "debug-tools")]
    pub(crate) fn set_debug_session(&self, session: Option<u64>) {
        self.shared.commands().set_session(session);
    }

    #[cfg(feature = "debug-tools")]
    pub(crate) fn drain_debug_commands(
        &self,
        session: u64,
    ) -> Vec<crate::app::debug_tools::DebugCommand> {
        self.shared.commands().drain(session)
    }

    /// Connections accepted since the server started; a publisher that sees a
    /// higher value than last time knows a client needs the match history.
    pub fn attach_generation(&self) -> u64 {
        self.shared.attached.load(Ordering::Acquire)
    }
}

impl Drop for TelemetryServer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
    }
}

struct Worker {
    socket: TcpStream,
    thread: thread::JoinHandle<()>,
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let mut workers: Vec<Worker> = Vec::new();
    let mut next_id = 0_u64;
    while !shared.stopped() {
        let mut index = 0;
        while index < workers.len() {
            if workers[index].thread.is_finished() {
                let worker = workers.swap_remove(index);
                let _ = worker.thread.join();
            } else {
                index += 1;
            }
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if shared.stopped() || workers.len() == MAX_WORKERS {
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let Ok(socket) = stream.try_clone() else {
                    continue;
                };
                let state = Arc::clone(&shared);
                let id = next_id;
                next_id = next_id.wrapping_add(1);
                let worker = thread::Builder::new()
                    .name("telemetry-client".into())
                    .spawn(move || {
                        let mut stream = stream;
                        let _ = serve_connection(&mut stream, &state, id);
                        let _ = stream.shutdown(Shutdown::Both);
                        state.clients().retain(|client| client.id != id);
                    });
                if let Ok(thread) = worker {
                    workers.push(Worker { socket, thread });
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => thread::sleep(POLL),
        }
    }
    for worker in &workers {
        let _ = worker.socket.shutdown(Shutdown::Both);
    }
    for worker in workers {
        let _ = worker.thread.join();
    }
    shared.clients().clear();
}

fn check_progress(stop: &AtomicBool, deadline: Instant) -> io::Result<()> {
    if stop.load(Ordering::Acquire) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "dashboard stopped",
        ))
    } else if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "dashboard I/O deadline",
        ))
    } else {
        Ok(())
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

fn write_until(
    writer: &mut impl Write,
    mut bytes: &[u8],
    stop: &AtomicBool,
    deadline: Instant,
) -> io::Result<()> {
    while !bytes.is_empty() {
        check_progress(stop, deadline)?;
        match writer.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "dashboard peer stopped reading",
                ));
            }
            Ok(written) => bytes = &bytes[written..],
            Err(error) if retryable(&error) => (),
            Err(error) => return Err(error),
        }
    }
    check_progress(stop, deadline)
}

fn read_request_head(
    reader: &mut impl Read,
    stop: &AtomicBool,
    deadline: Instant,
) -> io::Result<(String, Vec<u8>)> {
    let mut bytes = Vec::with_capacity(MAX_HTTP_HEAD);
    let mut chunk = [0_u8; 1024];
    loop {
        check_progress(stop, deadline)?;
        let capacity = (MAX_HTTP_HEAD - bytes.len()).min(chunk.len());
        if capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP head too large",
            ));
        }
        match reader.read(&mut chunk[..capacity]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete HTTP head",
                ));
            }
            Ok(read) => {
                check_progress(stop, deadline)?;
                bytes.extend_from_slice(&chunk[..read]);
                if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    let tail = bytes.split_off(end + 4);
                    bytes.truncate(end);
                    let head = String::from_utf8(bytes).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "non-ASCII HTTP head")
                    })?;
                    return Ok((head, tail));
                }
            }
            Err(error) if retryable(&error) => (),
            Err(error) => return Err(error),
        }
    }
}

struct Request<'a> {
    path: &'a str,
    headers: Vec<(&'a str, &'a str)>,
}

impl<'a> Request<'a> {
    fn parse(head: &'a str) -> Option<Self> {
        let mut lines = head.split("\r\n");
        let first = lines.next()?;
        if first.len() > MAX_HTTP_LINE {
            return None;
        }
        let mut words = first.split(' ');
        if words.next()? != "GET" {
            return None;
        }
        let path = words.next()?;
        if words.next()? != "HTTP/1.1"
            || words.next().is_some()
            || !path.starts_with('/')
            || !path.bytes().all(|byte| (33..=126).contains(&byte))
        {
            return None;
        }
        let mut headers: Vec<(&str, &str)> = Vec::new();
        for line in lines {
            if headers.len() == MAX_HEADERS || line.len() > MAX_HTTP_LINE {
                return None;
            }
            let (name, value) = line.split_once(':')?;
            if name.is_empty()
                || !name.bytes().all(http_token)
                || !value
                    .bytes()
                    .all(|byte| byte == b'\t' || (32..=126).contains(&byte))
                || headers
                    .iter()
                    .any(|(prior, _)| prior.eq_ignore_ascii_case(name))
            {
                return None;
            }
            headers.push((name, value.trim_matches([' ', '\t'])));
        }
        let request = Self { path, headers };
        if request.header("Transfer-Encoding").is_some()
            || request
                .header("Content-Length")
                .is_some_and(|value| value != "0")
        {
            return None;
        }
        Some(request)
    }

    fn header(&self, key: &str) -> Option<&'a str> {
        self.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| *value)
    }

    fn admitted_host(&self, admission: &Admission) -> bool {
        self.header("Host").is_some_and(|host| {
            admission.host_allowed(host)
                && self
                    .header("Origin")
                    .is_none_or(|origin| origin == format!("http://{host}"))
        })
    }

    fn websocket_key(&self, admission: &Admission) -> Option<&'a str> {
        if !self.admitted_host(admission)
            || self.header("Origin").is_none()
            || self.header("Sec-WebSocket-Version")? != "13"
            || !self.header("Upgrade")?.eq_ignore_ascii_case("websocket")
            || !self
                .header("Connection")?
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
            || !admission.authenticates(self.header("Sec-WebSocket-Protocol")?)
        {
            return None;
        }
        let key = self.header("Sec-WebSocket-Key")?;
        if key.len() != 24 {
            return None;
        }
        let decoded = crate::util::base64::base64_decode(key).ok()?;
        (decoded.len() == 16 && base64_encode(&decoded) == key).then_some(key)
    }
}

fn http_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn serve_connection(stream: &mut TcpStream, shared: &Shared, id: u64) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(POLL))?;
    stream.set_write_timeout(Some(POLL))?;
    let (head, tail) = match read_request_head(stream, &shared.stop, Instant::now() + IO_DEADLINE) {
        Ok(request) => request,
        Err(_) => return serve_status(stream, shared, 400, "bad request"),
    };
    let Some(request) = Request::parse(&head) else {
        return serve_status(stream, shared, 400, "bad request");
    };
    if !request.admitted_host(&shared.admission) {
        return serve_status(stream, shared, 403, "forbidden");
    }
    if request.path != "/ws" {
        return serve_static(stream, shared, request.path);
    }
    let Some(key) = request.websocket_key(&shared.admission) else {
        return serve_status(stream, shared, 403, "forbidden");
    };
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\nSec-WebSocket-Protocol: {PROTOCOL}\r\n\r\n",
        websocket_accept_key(key)
    );
    let (sender, receiver) = mpsc::sync_channel(CLIENT_QUEUE_DEPTH);
    let (screen_sender, screen_receiver) = mpsc::sync_channel(SCREEN_QUEUE_DEPTH);
    let (audio_sender, audio_receiver) = mpsc::sync_channel(AUDIO_QUEUE_DEPTH);
    let screen = Arc::new(AtomicBool::new(false));
    let audio = Arc::new(AtomicBool::new(false));
    shared.clients().push(Client {
        id,
        sender,
        screen: Arc::clone(&screen),
        screen_sender,
        audio: Arc::clone(&audio),
        audio_sender,
    });
    // Only an admitted dashboard counts: static asset requests also reach the
    // accept loop, and they do not read snapshots.
    shared.attached.fetch_add(1, Ordering::AcqRel);
    write_until(
        stream,
        response.as_bytes(),
        &shared.stop,
        Instant::now() + IO_DEADLINE,
    )?;
    writer_loop(
        stream,
        receiver,
        screen_receiver,
        screen,
        audio_receiver,
        audio,
        shared,
        tail,
    )
}

#[derive(Debug, PartialEq, Eq)]
enum ControlFrame {
    Ping(Vec<u8>),
    Pong,
    Close(Vec<u8>),
    /// Subscribe or unsubscribe this socket from the live map picture stream.
    Screen(bool),
    /// Subscribe or unsubscribe this socket from the live game audio stream.
    Audio(bool),
    /// Ask the app to release the pre-match hold, so a recording starts on
    /// frame zero instead of wherever the match happened to be.
    MatchStart,
    #[cfg(feature = "debug-tools")]
    Command(Vec<u8>),
}

#[cfg(test)]
fn parse_client_frame(bytes: &[u8]) -> Result<Option<(ControlFrame, usize)>, u16> {
    parse_client_frame_with_debug(bytes, false)
}

/// Reject data at the header unless this build and current session explicitly
/// admit debug commands. Even then, only small, final, masked text frames pass.
fn parse_client_frame_with_debug(
    bytes: &[u8],
    _allow_debug: bool,
) -> Result<Option<(ControlFrame, usize)>, u16> {
    if bytes.len() < 2 {
        return Ok(None);
    }
    if bytes[0] & 0x70 != 0 || bytes[1] & 0x80 == 0 {
        return Err(1002);
    }
    let opcode = bytes[0] & 15;
    if matches!(opcode, 1 | 2) {
        #[cfg(feature = "debug-tools")]
        if opcode == 1 && _allow_debug {
            return parse_debug_frame(bytes);
        }
        // The picture subscription is transport-only: it changes what this
        // socket receives, not the match. Admit it in every dashboard build,
        // but only as one of two exact, short, final, masked text frames.
        if opcode == 1 {
            return parse_screen_frame(bytes);
        }
        return Err(1008);
    }
    if bytes[0] & 0x80 == 0 || !matches!(opcode, 8..=10) {
        return Err(1002);
    }
    let len = usize::from(bytes[1] & 127);
    if len > 125 {
        return Err(1002);
    }
    let end = 6 + len;
    if bytes.len() < end {
        return Ok(None);
    }
    let payload: Vec<u8> = bytes[6..end]
        .iter()
        .enumerate()
        .map(|(i, byte)| byte ^ bytes[2 + i % 4])
        .collect();
    let frame = match opcode {
        9 => ControlFrame::Ping(payload),
        10 => ControlFrame::Pong,
        _ => {
            if payload.len() == 1 {
                return Err(1002);
            }
            if payload.len() >= 2 {
                let code = u16::from_be_bytes([payload[0], payload[1]]);
                if !((1000..=1014).contains(&code) && !matches!(code, 1004..=1006)
                    || (3000..=4999).contains(&code))
                {
                    return Err(1002);
                }
                if std::str::from_utf8(&payload[2..]).is_err() {
                    return Err(1007);
                }
            }
            ControlFrame::Close(payload)
        }
    };
    Ok(Some((frame, end)))
}

/// The only client data a read-only dashboard may send: the transport
/// subscriptions. Anything else is a protocol error, so this doubles as the
/// admission test that no game command can reach a `telemetry-dashboard` build.
fn parse_screen_frame(bytes: &[u8]) -> Result<Option<(ControlFrame, usize)>, u16> {
    // Any client data that is not an exact subscription frame keeps the old
    // policy result: rejected at the header, without waiting for a long body.
    if bytes[0] & 0x80 == 0 || bytes[1] & 0x80 == 0 {
        return Err(1008);
    }
    let len = usize::from(bytes[1] & 127);
    if len > 125 {
        return Err(1008);
    }
    let end = 6 + len;
    if bytes.len() < end {
        return Ok(None);
    }
    let payload: Vec<u8> = bytes[6..end]
        .iter()
        .enumerate()
        .map(|(i, byte)| byte ^ bytes[2 + i % 4])
        .collect();
    let frame = match payload.as_slice() {
        SCREEN_ON => ControlFrame::Screen(true),
        SCREEN_OFF => ControlFrame::Screen(false),
        AUDIO_ON => ControlFrame::Audio(true),
        AUDIO_OFF => ControlFrame::Audio(false),
        MATCH_START => ControlFrame::MatchStart,
        _ => return Err(1008),
    };
    Ok(Some((frame, end)))
}

#[cfg(feature = "debug-tools")]
fn parse_debug_frame(bytes: &[u8]) -> Result<Option<(ControlFrame, usize)>, u16> {
    if bytes[0] & 0x80 == 0 {
        return Err(1002);
    }
    let length = usize::from(bytes[1] & 127);
    let (length, header) = match length {
        127 => return Err(1009),
        126 => {
            if bytes.len() < 4 {
                return Ok(None);
            }
            let length = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
            if length < 126 {
                return Err(1002);
            }
            (length, 4)
        }
        length => (length, 2),
    };
    if length > crate::app::debug_tools::MAX_REQUEST_BYTES {
        return Err(1009);
    }
    let end = header + 4 + length;
    if bytes.len() < end {
        return Ok(None);
    }
    let payload: Vec<u8> = bytes[header + 4..end]
        .iter()
        .enumerate()
        .map(|(i, byte)| byte ^ bytes[header + i % 4])
        .collect();
    if std::str::from_utf8(&payload).is_err() {
        return Err(1007);
    }
    // The transport subscription is not a trainer command: keep it out of the
    // command inbox so a debug build behaves like a read-only one here.
    if payload.as_slice() == SCREEN_ON {
        return Ok(Some((ControlFrame::Screen(true), end)));
    }
    if payload.as_slice() == SCREEN_OFF {
        return Ok(Some((ControlFrame::Screen(false), end)));
    }
    if payload.as_slice() == AUDIO_ON {
        return Ok(Some((ControlFrame::Audio(true), end)));
    }
    if payload.as_slice() == AUDIO_OFF {
        return Ok(Some((ControlFrame::Audio(false), end)));
    }
    // Same rule for the start latch: a debug build must not turn the
    // dashboard's start button into a trainer request and close the socket.
    if payload.as_slice() == MATCH_START {
        return Ok(Some((ControlFrame::MatchStart, end)));
    }
    Ok(Some((ControlFrame::Command(payload), end)))
}

fn write_control(
    stream: &mut TcpStream,
    shared: &Shared,
    opcode: u8,
    payload: &[u8],
) -> io::Result<()> {
    let mut bytes = Vec::with_capacity(payload.len() + 2);
    bytes.extend_from_slice(&[0x80 | opcode, payload.len() as u8]);
    bytes.extend_from_slice(payload);
    write_until(stream, &bytes, &shared.stop, Instant::now() + IO_DEADLINE)
}

/// How long a socket may wait for client data between sends. Any live stream
/// has to drain far faster than the accept poll.
fn poll_timeout(screen: bool, audio: bool) -> Duration {
    if screen || audio { WRITER_POLL } else { POLL }
}

fn writer_loop(
    stream: &mut TcpStream,
    receiver: mpsc::Receiver<Arc<[u8]>>,
    screen_receiver: mpsc::Receiver<Arc<[u8]>>,
    screen: Arc<AtomicBool>,
    audio_receiver: mpsc::Receiver<Arc<[u8]>>,
    audio: Arc<AtomicBool>,
    shared: &Shared,
    mut inbound: Vec<u8>,
) -> io::Result<()> {
    let mut chunk = [0_u8; 1024];
    let mut window = Instant::now();
    let mut controls = 0;
    #[cfg(feature = "debug-tools")]
    let mut commands = 0;
    while !shared.stopped() {
        let mut processed = 0;
        while processed < MAX_CONTROL_BATCH && !shared.stopped() {
            let parsed = match parse_client_frame_with_debug(&inbound, shared.debug_allowed()) {
                Ok(parsed) => parsed,
                Err(code) => return write_control(stream, shared, 8, &code.to_be_bytes()),
            };
            let Some((frame, consumed)) = parsed else {
                break;
            };
            inbound.drain(..consumed);
            processed += 1;
            if window.elapsed() >= Duration::from_secs(1) {
                window = Instant::now();
                controls = 0;
                #[cfg(feature = "debug-tools")]
                {
                    commands = 0;
                }
            }
            controls += 1;
            if controls > MAX_CONTROLS_PER_SECOND {
                return write_control(stream, shared, 8, &1008_u16.to_be_bytes());
            }
            match frame {
                ControlFrame::Ping(payload) => write_control(stream, shared, 10, &payload)?,
                ControlFrame::Pong => (),
                ControlFrame::Close(payload) => return write_control(stream, shared, 8, &payload),
                ControlFrame::Screen(on) => {
                    screen.store(on, Ordering::Release);
                    // A subscribed socket cannot wait behind the 50 ms accept
                    // poll; a 60 Hz picture producer needs a shorter read.
                    stream.set_read_timeout(Some(poll_timeout(on, audio.load(Ordering::Acquire))))?;
                    if !on {
                        // Tiles already queued must not arrive after unsubscribe.
                        while screen_receiver.try_recv().is_ok() {}
                    }
                }
                ControlFrame::Audio(on) => {
                    audio.store(on, Ordering::Release);
                    // Audio arrives 25 times a second, far ahead of the accept
                    // poll; without this the listener hears stutter.
                    stream.set_read_timeout(Some(poll_timeout(
                        screen.load(Ordering::Acquire),
                        on,
                    )))?;
                    if !on {
                        // Blocks queued before unsubscribe must not play later.
                        while audio_receiver.try_recv().is_ok() {}
                    }
                }
                ControlFrame::MatchStart => {
                    // A latch, not a state: the app clears it on its next
                    // frame, so two dashboards clicking at once are one start.
                    shared.match_start.store(true, Ordering::Release);
                    log::info!("Dashboard asked for the held match to start");
                }
                #[cfg(feature = "debug-tools")]
                ControlFrame::Command(payload) => {
                    commands += 1;
                    if commands > 16 || shared.commands().push(&payload).is_err() {
                        return write_control(stream, shared, 8, &1008_u16.to_be_bytes());
                    }
                }
            }
        }
        match receiver.try_recv() {
            Ok(frame) => write_until(stream, &frame, &shared.stop, Instant::now() + IO_DEADLINE)?,
            Err(TryRecvError::Disconnected) => return Ok(()),
            Err(TryRecvError::Empty) => (),
        }
        // Audio first: a block is ~7 KB and goes stale in 40 ms, while a whole
        // picture is megabytes and can wait a few milliseconds behind it.
        if audio.load(Ordering::Acquire) {
            match audio_receiver.try_recv() {
                Ok(frame) => {
                    write_until(stream, &frame, &shared.stop, Instant::now() + IO_DEADLINE)?;
                }
                Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => (),
            }
        }
        // Snapshots and tiles have separate queues, so a burst of tiles never
        // starves the JSON stream that drives every other dashboard panel.
        if screen.load(Ordering::Acquire) {
            match screen_receiver.try_recv() {
                Ok(frame) => {
                    write_until(stream, &frame, &shared.stop, Instant::now() + IO_DEADLINE)?;
                }
                Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => (),
            }
        }
        if processed == MAX_CONTROL_BATCH {
            continue;
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if inbound.len() + count > MAX_INBOUND_BUFFER {
                    return write_control(stream, shared, 8, &1009_u16.to_be_bytes());
                }
                inbound.extend_from_slice(&chunk[..count]);
            }
            Err(error) if retryable(&error) => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn public_asset(path: &str) -> Option<(&'static str, &'static str)> {
    // An exact allowlist also rejects encoded traversal, Windows prefixes,
    // alternate separators, queries, source files and generated reports.
    match path {
        "/" | "/index.html" => Some(("index.html", "text/html; charset=utf-8")),
        "/dashboard.css" => Some(("dashboard.css", "text/css; charset=utf-8")),
        "/dashboard.js" => Some(("dashboard.js", "text/javascript; charset=utf-8")),
        "/starfield.js" => Some(("starfield.js", "text/javascript; charset=utf-8")),
        "/i18n.js" => Some(("i18n.js", "text/javascript; charset=utf-8")),
        "/map3d.js" => Some(("map3d.js", "text/javascript; charset=utf-8")),
        "/types.js" => Some(("types.js", "text/javascript; charset=utf-8")),
        "/debug-tools.js" => Some(("debug-tools.js", "text/javascript; charset=utf-8")),
        "/vendor/three.core.js" => Some(("vendor/three.core.js", "text/javascript; charset=utf-8")),
        "/vendor/three.module.js" => {
            Some(("vendor/three.module.js", "text/javascript; charset=utf-8"))
        }
        _ => None,
    }
}

fn asset_bytes(root: &Path, relative: &str) -> io::Result<Vec<u8>> {
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "asset outside dashboard",
        ));
    }
    let metadata = std::fs::metadata(&path)?;
    if !metadata.is_file() || metadata.len() > MAX_STATIC_FILE as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid dashboard asset",
        ));
    }
    let file = std::fs::File::open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > MAX_STATIC_FILE as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid dashboard asset",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_STATIC_FILE as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_STATIC_FILE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "dashboard asset too large",
        ));
    }
    Ok(bytes)
}

fn serve_static(stream: &mut TcpStream, shared: &Shared, path: &str) -> io::Result<()> {
    let Some((relative, content_type)) = public_asset(path) else {
        return serve_status(stream, shared, 404, "not found");
    };
    let Ok(bytes) = asset_bytes(&shared.web_root, relative) else {
        return serve_status(stream, shared, 404, "not found");
    };
    serve_response(stream, shared, 200, "OK", content_type, &bytes)
}

fn serve_status(
    stream: &mut TcpStream,
    shared: &Shared,
    status: u16,
    reason: &str,
) -> io::Result<()> {
    serve_response(
        stream,
        shared,
        status,
        reason,
        "text/plain; charset=utf-8",
        reason.as_bytes(),
    )
}

fn serve_response(
    stream: &mut TcpStream,
    shared: &Shared,
    status: u16,
    reason: &str,
    content_type: &str,
    bytes: &[u8],
) -> io::Result<()> {
    let authority = &shared.admission.authority;
    let localhost = &shared.admission.localhost;
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self' ws://{authority} ws://{localhost}; frame-ancestors 'none'; object-src 'none'; base-uri 'none'\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    let deadline = Instant::now() + IO_DEADLINE;
    write_until(stream, head.as_bytes(), &shared.stop, deadline)?;
    write_until(stream, bytes, &shared.stop, deadline)
}

fn resolve_bind(raw: &str) -> Result<SocketAddr, String> {
    raw.parse::<SocketAddr>()
        .or_else(|_| {
            raw.parse::<IpAddr>()
                .map(|ip| SocketAddr::new(ip, DEFAULT_PORT))
        })
        .map_err(|_| format!("{ENV_ADDR} must be a numeric IP address or IP:port"))
}

pub fn start_from_env() -> Option<Result<TelemetryServer, String>> {
    let raw = std::env::var(ENV_ADDR).ok()?;
    let addr = match resolve_bind(&raw) {
        Ok(addr) => addr,
        Err(error) => return Some(Err(error)),
    };
    let root = std::env::var_os(ENV_WEB_ROOT)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WEB_ROOT));
    Some(TelemetryServer::start(addr, root).map_err(|error| format!("telemetry server: {error}")))
}

#[cfg(test)]
#[path = "telemetry_server_tests.rs"]
mod tests;
