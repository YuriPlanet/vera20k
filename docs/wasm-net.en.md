# WASM networking adaptation

[简体中文](wasm-net.md) | [English](wasm-net.en.md)

`wasm-net` gives the browser build access to the existing online lobby and UDP relay. Default builds, desktop networking and single-player packages enabling only `wasm` retain their existing paths. The connection uses transparent WS/UDP conversion: the original lobby and relay servers, message formats, tickets, encryption, ACKs, reliable messaging and command-ordering protocols remain unchanged.

## Build and addresses

Follow the [WASM build requirements](wasm.md#build) to install the Rust target and the wasm-bindgen CLI version matching the lockfile, then run:

```sh
# Default single-player package
python3 tools/wasm/build.py

# Online package, default proxy wss://wss.vera20k.com
python3 tools/wasm/build.py --network

# A custom proxy address is embedded at compile time; changes require rebuilding
WS_PROXY_URL=wss://gateway.example.com python3 tools/wasm/build.py --network

# Local validation
WS_PROXY_URL=ws://localhost:8788 python3 tools/wasm/build.py --network
```

Both modes write to `target/wasm-dist/` and accept `--profile release`. Online packaging adds `network.js` and `lobby.js`; rebuilding the single-player package removes both files and their imports so that it cannot retain the previous online package's transport scripts.

`WS_PROXY_URL` is a compile-time proxy origin, with the full default value `wss://wss.vera20k.com`. The sole Rust configuration entry is [`net::udp::browser_proxy_url`](../src/net/udp.rs), which reads the build environment through `option_env!`; JS supplies no separate default. The build helper rejects empty values, credentials, paths, query strings, fragments and non-WebSocket schemes. `ws://` is allowed only for loopback addresses; normal deployments use `wss://`.

The default WS endpoints are `wss://wss.vera20k.com/game/primary` and `wss://wss.vera20k.com/game/alternate`. Lobby Fetch requests use HTTPS `/v1/*` at the same authority (local `ws:` maps to `http:`). There is no runtime address setting, localStorage override or fallback to the page's domain. The default domain is a build convention, not a claim that the service has been deployed. `WS_PROXY_URL` does not change the lobby's `RelayGrant.address`.

## Compile isolation

The root crate's `wasm-net` enables `wasm` and core's `wasm-net`; core adds an optional `wasm-bindgen` dependency for the browser target. Production browser networking branches require all three conditions:

```rust
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
```

| Build | Implementation |
| --- | --- |
| Desktop default, shipping, `wasm` or `wasm-net` | Native UDP, ureq and thread scheduling |
| WASM with only `wasm` | Existing single-player path; networking entry points return unsupported |
| WASM with `wasm-net` | WS datagram adapter, Fetch and browser loading waits |
| WASM without `wasm` | Existing compile error remains |

The separate manual WASM workflow packages single-player and online versions; desktop workflows gain no gateway or Node dependency. The gateway lives in [`tools/wasm/gateway`](../tools/wasm/gateway/README.md), is installed and run independently, and is outside the Rust workspace.

## Transport and services

```text
Existing online lobby logic → Fetch → gateway /v1/* → original HTTPS lobby
Existing network protocol → browser socket → WS /game/primary → separate UDP socket → original relay
                           browser alternate → WS /game/alternate → another UDP socket
Desktop client → original HTTPS lobby and UDP relay
```

One WS binary message carries one complete original UDP datagram. There is no added destination header, JSON, base64 or custom handshake, and datagrams are not combined. Authentication, renewal, ACKs, retries, ordering and match logs remain with their original owners. WS OPEN does not mean relay admission, and connection Ping/Pong is not a game-layer acknowledgement.

Each WS connection owns a connected UDP socket whose source port stays stable for that connection's lifetime. Players and channels do not share ports. An administrator configures the gateway's fixed UDP target, which must correspond to the lobby's relay; browsers cannot choose arbitrary forwarding destinations. See the [gateway guide](../tools/wasm/gateway/README.md) for deployment and limits.

The `/v1/*` proxy preserves the existing method, path, Authorization, request body, status code and response body. It neither parses nor rewrites game JSON, and it does not retry on the client's behalf. The gateway handles cross-origin CORS, so the original lobby needs no OPTIONS support. The gateway's HTTP component handles bearer tokens and responses containing payload keys, making it part of the lobby credential trust boundary. It needs no relay signing key, logs no credentials and reads no retail assets.

## Client boundaries

### Sockets and recovery

[`web_socket.rs`](../src/net/web_socket.rs) replaces only `UdpIo`'s private physical socket, preserving the `LanUdpTransport` interface and original protocol queues. `bind` creates only logical handles; WS connections open after the original owner accepts a relay grant. Opening the LAN menu cannot incidentally connect to a remote proxy. Send and receive endpoints are pinned to that grant, rather than inferring the source from the last send destination.

[`network.js`](../tools/wasm/network.js) buffers only raw bytes. Reception is limited to 256 packets and 256 KiB; sending uses `bufferedAmount` to enforce a 256 KiB limit. Rust uses the original codec's `MAX_DATAGRAM`. A pending connection or send backlog returns WouldBlock, leaving retries with the protocol queues.

Physical connection failures trigger reconnects with bounded backoff, clear the old connection's bytes and callbacks, and notify the original Rust owner to repeat the relay challenge. Original reliable queues, match identity, unacknowledged batches and the log cursor remain intact. Network loss never creates Leave; explicit exit still follows the original acknowledgement flow. Browsers have no native I/O pump thread, so incremental app service calls and the event loop advance networking together.

### Lobby executor

The `serial_driver!` macro in [`http.rs`](../src/app/shell_wol/live_service/http.rs) retains one serial control flow: native builds expand it into the original synchronous functions and blocking ureq calls, while the browser branch expands it into async functions. `platform_await!` selects only how the platform waits. No async executor is introduced on desktop, and there is no duplicate token, cursor, active_match, batch deduplication, resume or cleanup state machine.

Browser I/O lives in [`http/browser.rs`](../src/app/shell_wol/live_service/http/browser.rs) and [`lobby.js`](../tools/wasm/lobby.js). Main control requests remain serial, with translation scheduled independently; existing Request/Response types, map transfers and room reducers are reused. Each lobby visit owns separate Fetch request handles. Retirement cancels requests and releases callbacks so they cannot write into a later visit. HTTP retains timeouts, response size limits, omitted cookies, rejected redirects and existing error mappings.

Downloaded maps reuse `map::source` validation and cache quotas. Only the browser networking branch redirects disk operations to the existing memory filesystem, publishes complete validated bytes atomically and retains `/saves` persistence. Shared leases protect active maps until their last consumer releases them, after which they may be evicted. Desktop retains its existing file locks and staged-file publication.

### Yielding during loading

Online loading in the browser networking branch uses existing `FreshMapLoadJob` continuations. `LoadingNetworkService::milestone` services networking once, then defers the ACK drain to later loading pumps. The same `LanService` stores the deadline; it does not copy the transport or reliable queues.

The wait retains the original queue threshold of more than 5 entries and the 240 wall-bucket limit. Consecutive milestones do not reset an existing deadline, and timeout never fabricates an ACK. Later loading phases, presentation preparation and final installation resume the wait and service lobby and loading messages. After incremental loading, the browser still passes the original remote-progress and startup barriers; it cannot skip readiness and advance the first simulation frame. Desktop retains synchronous loading, 20 ms sleeps and its original thread path.

The relevant owners are [`lan_service/loading.rs`](../src/app/lan_service/loading.rs) and [`loading/pump.rs`](../src/app/loading/pump.rs). This adaptation changes browser scheduling, not the original rules reads, initialization order, simulation math or RNG.

## Validation and limitations

Focused checks:

```sh
node --test tools/wasm/tests/*.test.cjs
npm --prefix tools/wasm/gateway ci
npm --prefix tools/wasm/gateway test
python3 -m unittest tools.tests.test_wasm_build
```

Rust validation uses `python3 -m tools.cargo_run -- test -p <crate> --lib <filter>` for affected net, HTTP and loading modules and their direct consumers. Retail checks set `VERA20K_REQUIRE_RETAIL_INI=1`; loading changes also require a retail map load with `--profile fast`. Changes to Python tools require the project's `python -m tools.run_tests`. Compilation on one platform or JS mock tests do not establish correctness on other platforms or in real matches.

Before release, cover native default, native `wasm`, native `wasm-net`, WASM `wasm` and WASM `wasm-net` separately. Check artifacts after changing the proxy environment variable and cleanup when switching an online package back to single-player. Gateway integration tests cover real HTTP/WS/UDP bytes, datagram boundaries, source ports, port release and rejection paths.

Runtime acceptance still requires complete two-client matches, matching committed-frame digests between browser and unchanged desktop logic, catch-up after a temporary disconnection, explicit exit and starting another match. Only those runs can establish that online play works; compilation or UDP echo is insufficient. Report validation results with delivery, rather than retaining run logs in this document.

Interop tests require identical source and feature sets on both clients: the existing build identity includes every Cargo feature. This adaptation does not relax that check. Build the desktop test client corresponding to the browser online package with `--no-default-features --features wasm-net`; it still uses native networking. Do not assume an existing older desktop installation can join the new build.

This work connects existing online play only. It does not add LAN broadcast discovery, arbitrary direct-IP connections, CnCNet, a new protocol, random map generation or simulation restoration after page reload. Browser background throttling, long synchronous asset operations and WS head-of-line blocking still need runtime observation; the original relay's process-restart recovery limits remain. Support for 30 protocol sources does not establish browser performance at 30 players and 20,000 units.
