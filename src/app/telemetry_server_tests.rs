use super::*;
use crate::map::source::test_support::TestDirectory;

struct Fixture {
    server: TelemetryServer,
    root: TestDirectory,
}

impl Fixture {
    fn new() -> Self {
        let root = TestDirectory::new("telemetry-server");
        root.write("index.html", b"<html>dashboard</html>");
        root.write("dashboard.js", b"export {};\n");
        let server =
            TelemetryServer::start("127.0.0.1:0".parse().unwrap(), root.path().to_path_buf())
                .unwrap();
        Self { server, root }
    }
}

fn request(server: &TelemetryServer) -> String {
    let url = server.bootstrap_url();
    let token = url.split_once("#token=").unwrap().1;
    format!(
        "GET /ws HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: {PROTOCOL}, {AUTH_PREFIX}{token}\r\n\r\n",
        server.addr(),
        server.addr()
    )
}

fn connect(server: &TelemetryServer) -> TcpStream {
    let stream = TcpStream::connect(server.addr()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
}

fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
        assert!(head.len() <= MAX_HTTP_HEAD);
    }
    String::from_utf8(head).unwrap()
}

fn websocket(server: &TelemetryServer) -> TcpStream {
    let mut stream = connect(server);
    stream.write_all(request(server).as_bytes()).unwrap();
    let head = read_head(&mut stream);
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    assert!(head.contains("Sec-WebSocket-Protocol: vera-telemetry.v1\r\n"));
    assert!(!head.contains(AUTH_PREFIX));
    stream
}

fn masked(opcode: u8, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 125);
    let mut bytes = vec![0x80 | opcode, 0x80 | payload.len() as u8, 1, 2, 3, 4];
    bytes.extend(
        payload
            .iter()
            .enumerate()
            .map(|(i, value)| value ^ [1, 2, 3, 4][i % 4]),
    );
    bytes
}

fn frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut head = [0; 2];
    stream.read_exact(&mut head).unwrap();
    assert_eq!(head[0] & 0x80, 0x80);
    assert_eq!(head[1] & 0x80, 0);
    let len = match head[1] {
        126 => {
            let mut len = [0; 2];
            stream.read_exact(&mut len).unwrap();
            u16::from_be_bytes(len) as usize
        }
        127 => {
            let mut len = [0; 8];
            stream.read_exact(&mut len).unwrap();
            usize::try_from(u64::from_be_bytes(len)).unwrap()
        }
        len => usize::from(len),
    };
    assert!(len <= MAX_SNAPSHOT);
    let mut payload = vec![0; len];
    stream.read_exact(&mut payload).unwrap();
    (head[0] & 15, payload)
}

#[test]
fn rfc_handshake_and_frame_encoding_are_preserved() {
    assert_eq!(
        websocket_accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
    assert_eq!(encode_text_frame("hi"), [0x81, 2, b'h', b'i']);
    for len in [126, 65535, 65536] {
        let encoded = encode_text_frame(&"x".repeat(len));
        let header = if len > 65535 { 10 } else { 4 };
        assert_eq!(encoded.len(), len + header);
        assert_eq!(encoded[1], if len > 65535 { 127 } else { 126 });
    }
}

#[test]
fn credentials_are_per_server_and_bind_policy_is_loopback_only() {
    let first = Fixture::new();
    let second = Fixture::new();
    let first_url = first.server.bootstrap_url();
    let first_token = first_url.split_once("#token=").unwrap().1;
    assert_eq!(first_token.len(), 64);
    assert!(
        first_token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    assert_ne!(
        first_token,
        second
            .server
            .bootstrap_url()
            .split_once("#token=")
            .unwrap()
            .1
    );
    for addr in ["0.0.0.0:0", "192.0.2.1:0", "[::]:0", "[::ffff:127.0.0.1]:0"] {
        let result = TelemetryServer::start(addr.parse().unwrap(), first.root.path().to_path_buf());
        assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::PermissionDenied));
    }
    assert_eq!(
        resolve_bind("::1").unwrap(),
        format!("[::1]:{DEFAULT_PORT}").parse().unwrap()
    );
    assert!(resolve_bind("localhost").is_err());
    // Pure admission covers IPv6 even on hosts where IPv6 sockets are disabled.
    let admission = Admission {
        authority: "[::1]:9741".into(),
        localhost: "localhost:9741".into(),
        secret: [0; 32],
    };
    assert!(admission.host_allowed("[::1]:9741"));
    assert!(!admission.host_allowed("[::1]:9742"));
}

#[test]
fn handshake_requires_exact_host_origin_and_session_credential() {
    let fixture = Fixture::new();
    let original = request(&fixture.server);
    let url = fixture.server.bootstrap_url();
    let token = url.split_once("#token=").unwrap().1;
    let origin_line = format!("Origin: http://{}\r\n", fixture.server.addr());
    let host_line = format!("Host: {}\r\n", fixture.server.addr());
    let wrong_token = if token.starts_with('0') {
        format!("1{}", &token[1..])
    } else {
        format!("0{}", &token[1..])
    };
    let cases = [
        original.replace(&host_line, "Host: attacker.invalid:9741\r\n"),
        original.replace(&host_line, ""),
        original.replace(&host_line, &format!("{host_line}{host_line}")),
        original.replace(&origin_line, "Origin: null\r\n"),
        original.replace(&origin_line, "Origin: https://attacker.invalid\r\n"),
        original.replace(&origin_line, ""),
        original.replace(&origin_line, &format!("{origin_line}{origin_line}")),
        original.replace(token, &wrong_token),
        original.replace(&format!(", {AUTH_PREFIX}{token}"), ""),
        original.replace("Sec-WebSocket-Version: 13", "Sec-WebSocket-Version: 12"),
        original.replace("dGhlIHNhbXBsZSBub25jZQ==", "bm90LXNpeHRlZW4="),
        original.replace("Connection: Upgrade", "Connection: close"),
        original.replace("Upgrade: websocket", "Upgrade: other"),
    ];
    for bad in cases {
        let mut stream = connect(&fixture.server);
        stream.write_all(bad.as_bytes()).unwrap();
        let head = read_head(&mut stream);
        assert!(!head.starts_with("HTTP/1.1 101"), "unauthorized upgrade");
        assert!(!head.contains(token));
    }
    let _accepted = websocket(&fixture.server);
}

#[test]
fn http_parser_rejects_ambiguous_headers_and_bodies() {
    for head in [
        "GET / HTTP/1.1\r\nHost : example",
        "GET / HTTP/1.1\r\nHost: one\r\nhost: two",
        "GET / HTTP/1.1\r\nHost: one\nOrigin: two",
        "GET / HTTP/1.1\r\nHost: one\r\n folded: no",
        "GET / HTTP/1.1\r\nContent-Length: 1",
        "GET / HTTP/1.1\r\nTransfer-Encoding: chunked",
        "GET http://example/ HTTP/1.1\r\nHost: example",
        "GET / HTTP/1.1 extra\r\nHost: example",
        "POST / HTTP/1.1\r\nHost: example",
    ] {
        assert!(Request::parse(head).is_none());
    }
    let oversized = format!(
        "GET /{} HTTP/1.1\r\nHost: example",
        "x".repeat(MAX_HTTP_LINE)
    );
    assert!(Request::parse(&oversized).is_none());
    let too_many = format!(
        "GET / HTTP/1.1\r\n{}",
        (0..=MAX_HEADERS)
            .map(|i| format!("X-{i}: value\r\n"))
            .collect::<String>()
    );
    assert!(Request::parse(&too_many).is_none());
}

#[test]
fn all_data_including_old_commands_is_closed_by_policy() {
    let fixture = Fixture::new();
    for payload in [
        br#"{"cmd":"frame_step"}"#.as_slice(),
        br#"{"cmd":"set_cheats","flags":{}}"#,
        br#"{"cmd":"toggle","switch":"heightmap"}"#,
    ] {
        for opcode in [1, 2] {
            let mut stream = websocket(&fixture.server);
            stream.write_all(&masked(opcode, payload)).unwrap();
            assert_eq!(frame(&mut stream), (8, 1008_u16.to_be_bytes().to_vec()));
        }
    }
    // The rejection does not wait for a giant payload to arrive.
    assert_eq!(parse_client_frame(&[0x81, 0xff]), Err(1008));
}

#[cfg(feature = "debug-tools")]
#[test]
fn debug_commands_require_enabled_epoch_and_keep_control_protocol() {
    use crate::app::debug_tools::DebugCommand;
    let fixture = Fixture::new();
    fixture.server.set_debug_session(Some(7));
    let mut stream = websocket(&fixture.server);
    let command = br#"{"session":7,"command":{"cmd":"frame_step"}}"#;
    stream.write_all(&masked(1, command)).unwrap();
    stream.write_all(&masked(9, b"barrier")).unwrap();
    assert_eq!(frame(&mut stream), (10, b"barrier".to_vec()));
    assert_eq!(
        fixture.server.drain_debug_commands(7),
        vec![DebugCommand::FrameStep {}]
    );

    fixture.server.set_debug_session(Some(8));
    stream.write_all(&masked(1, command)).unwrap();
    assert_eq!(frame(&mut stream), (8, 1008_u16.to_be_bytes().to_vec()));
    assert!(fixture.server.drain_debug_commands(8).is_empty());
}

#[cfg(feature = "debug-tools")]
#[test]
fn debug_data_frames_reject_fragmentation_binary_and_oversize() {
    let text = masked(1, b"{}");
    assert!(matches!(
        parse_client_frame_with_debug(&text, true),
        Ok(Some((ControlFrame::Command(_), _)))
    ));
    let mut fragment = text.clone();
    fragment[0] &= 0x7f;
    assert_eq!(parse_client_frame_with_debug(&fragment, true), Err(1002));
    assert_eq!(
        parse_client_frame_with_debug(&masked(2, b"{}"), true),
        Err(1008)
    );
    assert_eq!(
        parse_client_frame_with_debug(&[0x81, 0xff], true),
        Err(1009)
    );
    assert_eq!(
        parse_client_frame_with_debug(&[0x81, 0xfe, 4, 1], true),
        Err(1009)
    );
    assert_eq!(
        parse_client_frame_with_debug(&masked(1, &[0xff]), true),
        Err(1007)
    );
}

#[test]
fn controls_validate_mask_bits_lengths_codes_and_utf8() {
    let ping = masked(9, b"hello");
    assert_eq!(
        parse_client_frame(&ping),
        Ok(Some((ControlFrame::Ping(b"hello".to_vec()), ping.len())))
    );
    assert_eq!(parse_client_frame(&ping[..5]), Ok(None));
    for bad in [
        vec![0x89, 0],
        vec![0xc9, 0x80],
        vec![9, 0x80],
        vec![0x80, 0x80],
        vec![0x8b, 0x80],
        vec![0x89, 0xfe],
        masked(8, &[1]),
        masked(8, &1005_u16.to_be_bytes()),
    ] {
        assert_eq!(parse_client_frame(&bad), Err(1002));
    }
    assert_eq!(parse_client_frame(&masked(8, &[3, 232, 0xff])), Err(1007));
}

#[test]
fn coalesced_handshake_tail_and_ping_close_follow_protocol() {
    let fixture = Fixture::new();
    let mut stream = connect(&fixture.server);
    let mut bytes = request(&fixture.server).into_bytes();
    bytes.extend(masked(9, b"coalesced"));
    stream.write_all(&bytes).unwrap();
    assert!(read_head(&mut stream).starts_with("HTTP/1.1 101"));
    assert_eq!(frame(&mut stream), (10, b"coalesced".to_vec()));
    fixture.server.publish("{\"tick\":42}".to_owned());
    assert_eq!(frame(&mut stream), (1, b"{\"tick\":42}".to_vec()));
    stream
        .write_all(&masked(8, &1000_u16.to_be_bytes()))
        .unwrap();
    assert_eq!(frame(&mut stream), (8, 1000_u16.to_be_bytes().to_vec()));
}

#[test]
fn full_snapshot_queue_is_retained_and_disconnected_queue_is_removed() {
    let fixture = Fixture::new();
    let (sender, receiver) = mpsc::sync_channel(CLIENT_QUEUE_DEPTH);
    fixture.server.shared.clients().push(Client {
        id: u64::MAX,
        sender,
    });
    for _ in 0..CLIENT_QUEUE_DEPTH + 2 {
        fixture.server.publish("old".into());
    }
    assert!(fixture.server.has_clients());
    for _ in 0..CLIENT_QUEUE_DEPTH {
        assert_eq!(&*receiver.try_recv().unwrap(), encode_text_frame("old"));
    }
    assert!(receiver.try_recv().is_err());
    fixture.server.publish("latest".into());
    assert_eq!(&*receiver.try_recv().unwrap(), encode_text_frame("latest"));
    fixture.server.publish("x".repeat(MAX_SNAPSHOT + 1));
    assert!(receiver.try_recv().is_err());
    drop(receiver);
    fixture.server.publish("gone".into());
    assert!(!fixture.server.has_clients());
}

#[test]
fn control_flood_is_bounded_and_connection_capacity_is_finite() {
    let fixture = Fixture::new();
    let mut stream = websocket(&fixture.server);
    let flood = masked(10, b"pong").repeat(MAX_CONTROLS_PER_SECOND + 1);
    stream.write_all(&flood).unwrap();
    assert_eq!(frame(&mut stream), (8, 1008_u16.to_be_bytes().to_vec()));
    drop(stream);

    // A separate listener avoids racing the just-closed worker's reaping.
    let capped = Fixture::new();
    let clients: Vec<_> = (0..MAX_WORKERS)
        .map(|_| websocket(&capped.server))
        .collect();
    let mut overflow = connect(&capped.server);
    let _ = overflow.write_all(request(&capped.server).as_bytes());
    let mut byte = [0];
    match overflow.read(&mut byte) {
        Ok(0) => (),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe
            ) =>
        {
            ()
        }
        other => panic!("excess connection must close promptly: {other:?}"),
    }
    drop(clients);
}

#[test]
fn only_bounded_public_assets_are_served() {
    let fixture = Fixture::new();
    for (path, status) in [
        ("/", 200),
        ("/dashboard.js", 200),
        ("/../Cargo.toml", 404),
        ("/C:/secret", 404),
        ("/\\\\server\\share", 404),
        ("/%2e%2e/Cargo.toml", 404),
        ("/dashboard%2ejs", 404),
        ("//dashboard.js", 404),
        ("/dashboard.js?x=1", 404),
        ("/reports/latest.html", 404),
        ("/types.ts", 404),
        ("/package.json", 404),
    ] {
        let mut stream = connect(&fixture.server);
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {}\r\n\r\n",
            fixture.server.addr()
        )
        .unwrap();
        let head = read_head(&mut stream);
        assert!(
            head.starts_with(&format!("HTTP/1.1 {status}")),
            "{path}: {head}"
        );
    }
    let file = std::fs::File::create(fixture.root.path().join("types.js")).unwrap();
    file.set_len(MAX_STATIC_FILE as u64 + 1).unwrap();
    assert!(asset_bytes(&fixture.server.shared.web_root, "types.js").is_err());
    std::fs::create_dir(fixture.root.path().join("i18n.js")).unwrap();
    assert!(asset_bytes(&fixture.server.shared.web_root, "i18n.js").is_err());
}

#[cfg(unix)]
#[test]
fn static_symlink_cannot_escape_canonical_root() {
    let fixture = Fixture::new();
    let outside = TestDirectory::new("telemetry-outside");
    outside.write("secret", b"not a dashboard asset");
    std::os::unix::fs::symlink(
        outside.path().join("secret"),
        fixture.root.path().join("types.js"),
    )
    .unwrap();
    assert!(asset_bytes(&fixture.server.shared.web_root, "types.js").is_err());
}

#[test]
fn absolute_deadlines_do_not_restart_after_progress() {
    struct SlowBytes;
    impl Read for SlowBytes {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            thread::sleep(Duration::from_millis(5));
            bytes[0] = b'x';
            Ok(1)
        }
    }
    impl Write for SlowBytes {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            thread::sleep(Duration::from_millis(5));
            Ok(1)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let stop = AtomicBool::new(false);
    let read = read_request_head(
        &mut SlowBytes,
        &stop,
        Instant::now() + Duration::from_millis(15),
    );
    assert!(matches!(read, Err(error) if error.kind() == io::ErrorKind::TimedOut));
    let write = write_until(
        &mut SlowBytes,
        &[0; 100],
        &stop,
        Instant::now() + Duration::from_millis(15),
    );
    assert!(matches!(write, Err(error) if error.kind() == io::ErrorKind::TimedOut));
    let full_head = vec![b'x'; MAX_HTTP_HEAD];
    assert!(
        read_request_head(
            &mut full_head.as_slice(),
            &stop,
            Instant::now() + IO_DEADLINE
        )
        .is_err()
    );
}

#[test]
fn stalled_header_does_not_block_another_client_or_shutdown() {
    let fixture = Fixture::new();
    let mut stalled = connect(&fixture.server);
    stalled.write_all(b"GET / HTTP/1.1\r\nHost:").unwrap();
    let mut active = websocket(&fixture.server);
    active.write_all(&masked(9, b"ready")).unwrap();
    assert_eq!(frame(&mut active).0, 10);
    let (done, ended) = mpsc::channel();
    let worker = thread::spawn(move || {
        drop(fixture);
        done.send(()).unwrap();
    });
    ended
        .recv_timeout(Duration::from_secs(1))
        .expect("Drop joins stalled and active workers");
    worker.join().unwrap();
}

#[test]
fn slow_reader_and_continuous_traffic_do_not_hold_shutdown() {
    let fixture = Fixture::new();
    let mut traffic = websocket(&fixture.server);
    let _slow_reader = websocket(&fixture.server);
    fixture.server.publish("x".repeat(MAX_SNAPSHOT));
    let (started, ready) = mpsc::channel();
    let traffic_worker = thread::spawn(move || {
        traffic.write_all(&masked(9, b"traffic")).unwrap();
        started.send(()).unwrap();
        while traffic.write_all(&masked(10, b"traffic")).is_ok() {}
    });
    ready.recv_timeout(Duration::from_secs(1)).unwrap();
    let (done, ended) = mpsc::channel();
    let worker = thread::spawn(move || {
        drop(fixture);
        done.send(()).unwrap();
    });
    ended
        .recv_timeout(Duration::from_secs(1))
        .expect("Drop interrupts writes and busy reads");
    worker.join().unwrap();
    traffic_worker.join().unwrap();
}
