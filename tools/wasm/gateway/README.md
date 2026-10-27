# Browser network gateway

[English](README.md) | [简体中文](README.zh-CN.md)

This optional service forwards `/game/primary` and `/game/alternate` binary
WebSocket messages to a fixed UDP relay. One message is one unchanged datagram,
and each WebSocket owns a separate connected UDP socket. It does not interpret
game packets, issue credentials, acknowledge commands or implement retries.
The original lobby, relay and desktop clients require no changes.

Use Node.js 22 or newer. Dependencies are private to this directory:

```sh
cd tools/wasm/gateway
npm ci
VERA_RELAY_HOST=relay.example.com VERA_RELAY_PORT=5000 \
  VERA_WEB_ORIGIN=https://game.example.com npm start
```

`VERA_RELAY_HOST` is required. `VERA_RELAY_PORT` defaults to 5000. Configure the
relay corresponding to the lobby's `RelayGrant.address`; the gateway cannot
substitute a different relay's credentials. Browsers cannot select the upstream.
`VERA_LOBBY_URL` defaults to `https://lobby.vera20k.org`. It must be an HTTPS
origin. `/v1/*` forwards the existing method, path, Authorization and body without
JSON conversion, following redirects or replaying requests. It preserves the
upstream status and body. CORS permits only `VERA_WEB_ORIGIN`, GET/POST and the
Authorization/Content-Type headers. Ambient cookies are not forwarded.

The browser's `WS_PROXY_URL` is compiled into its `wasm-net` build, defaulting to
`wss://wss.vera20k.com`. This is a client setting, separate from the server's UDP
target. Deploy a TLS reverse proxy at that origin and forward `/game/*` with
WebSocket Upgrade and `/v1/*` to this process. Preserve the browser Origin header;
allow at least 70 seconds before an idle WebSocket timeout. The Node server binds
`127.0.0.1:8788` by default (`HOST` and `PORT` can override it). It serves no static
game files. The static site and gateway may be hosted separately.

LAN signaling is off by default. `VERA_LAN_SIGNAL=1` enables `/lan/signal` for
`wasm-lan` builds: it groups browsers by public address, assigns virtual IPv4s
and forwards WebRTC descriptions and candidates within the group; game datagrams
never pass through it. Behind a reverse proxy, set `VERA_CLIENT_IP_HEADER` to the
header that proxy writes (for example `cf-connecting-ip`, or `x-forwarded-for`,
whose last entry is used) and route `/lan/*` WebSocket Upgrades here; never set it
when clients can reach the gateway directly. `VERA_LAN_STUN` optionally lists
comma-separated `stun:` URLs for networks that hide host candidates; TURN is
rejected. Limits are in `signal.mjs`: 256 connections, 16 members per group,
16 KiB messages and 64 messages per second per connection.

Local development can use `VERA_WEB_ORIGIN=http://localhost:8000`. An HTTP lobby
requires both `VERA_ALLOW_LOCAL_HTTP=1` and a loopback lobby origin, for example
`VERA_LOBBY_URL=http://127.0.0.1:8790`. Remote HTTP is rejected. The browser must be
compiled with a matching local `ws://localhost:8788` proxy origin for this setup.

All game payloads remain encrypted, but the HTTP proxy handles the lobby bearer
and key-bearing responses. Treat the gateway as trusted lobby infrastructure;
do not log headers, bodies, tickets or payload keys. It needs no relay signing key
and no retail resources. WS Origin checks supplement existing relay authentication;
they do not authenticate arbitrary non-browser clients.

Transport limits are defined together in `server.mjs`: 256 WebSockets, 128 pending
HTTP exchanges, 64 KiB request and 1 MiB response bodies, 2048-byte messages,
256 queued UDP packets / 256 KiB, and 1 MiB pending WS output. Each connection
permits 2048 combined incoming/outgoing datagrams per second, under a 16384/second
aggregate limit. Messages also have bounded fragmentation and buffering. Overflow
closes that transport; only the original game protocol decides recovery. HTTP
responses use stream backpressure and a 12-second absolute request deadline.
Socket shutdown releases queues and the UDP port. The 30-second WS Ping/Pong
heartbeat never changes game readiness, tickets or Leave semantics.

The pinned `ws` version includes the upstream fixes for
[fragment memory exhaustion](https://github.com/websockets/ws/security/advisories/GHSA-96hv-2xvq-fx4p)
and [close reason memory disclosure](https://github.com/websockets/ws/security/advisories/GHSA-58qx-3vcg-4xpx).
The implementation uses the documented
[ws 8.22.0 limits](https://github.com/websockets/ws/blob/8.22.0/doc/ws.md)
and [connected UDP semantics](https://nodejs.org/api/dgram.html#socketconnectport-address-callback).

Run `npm test` for real HTTP and WS/UDP integration tests. They cover byte and
packet preservation, separate stable source ports and release, proxy limits and
admission failures. They do not establish authenticated gameplay, browser/native
simulation convergence or the 30-player / 20,000-unit performance target.
