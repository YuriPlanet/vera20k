# Browser network gateway

[English](README.md) | [简体中文](README.zh-CN.md)

This optional service forwards `/game/primary` and `/game/alternate` binary
WebSocket messages to the UDP relay each browser's grant names, when that relay
is on its list. One message is one unchanged datagram, and each WebSocket owns a
separate connected UDP socket. It does not interpret game packets, issue
credentials, acknowledge commands or implement retries. The original lobby, relay
and desktop clients require no changes.

Use Node.js 22 or newer. Dependencies are private to this directory:

```sh
cd tools/wasm/gateway
npm ci
VERA_RELAYS=203.0.113.1:5000,198.51.100.2:5000 \
  VERA_WEB_ORIGIN=https://game.example.com npm start
```

`VERA_RELAYS` is required: every relay address the lobby grants, as
comma-separated IPv4:port. Each game WebSocket names its grant's relay in its
path (`/game/primary/203.0.113.1:5000`), and the gateway forwards it there only
when that relay is listed; any other path is refused with 403, so browsers cannot
reach other destinations. List a relay the lobby adds in the same deploy, or its
browser players never pass relay admission.
A browser bundle from before relay paths opens `/game/primary` and
`/game/alternate` alone. To keep pages loaded before an upgrade playing, set
`VERA_LEGACY_RELAY` to the one listed relay those bundles were served by; the
gateway then forwards the bare paths there. Unset, they get 403. Remove it once
no such page can still be open.
`VERA_LOBBY_URL` defaults to `https://lobby.vera20k.org`. It must be an HTTPS
origin. `/v1/*` forwards the existing method, path, Authorization, the headers a
game signs in with (`Vera-Version`, `Vera-Build`, `Vera-Quick-Match`,
`Vera-Host-Handover`, `Vera-Watch-AI`, `Vera-Language` and `Vera-Changes`) and body without JSON
conversion, following redirects or replaying requests. It preserves the upstream
status and body. `VERA_WEB_ORIGIN` is required: the origin of every site that serves
the game, comma-separated (`https://game.example.com,https://www.example.org`), each
exactly as the browser sends it, so `www.` and a port count. CORS permits only those
origins, each answered under its own name, GET/POST and those headers with
Content-Type. Ambient cookies are not forwarded. Deploy a gateway at least as new as
the browser package it serves: an older one refuses a newer package's sign-in headers
at its CORS check, and that sign-in fails.

The lobby keys its per-network limits on the requesting address, which for
proxied requests is the gateway's. To give each browser player its own limits,
set `VERA_GATEWAY_KEY` to the lobby Worker's `GATEWAY_KEY` secret, a random string
of at least 32 characters (`openssl rand -hex 32`). The gateway then names each
player's address (the socket peer, or `VERA_CLIENT_IP_HEADER`) with the headers
[the lobby protocol](../../../docs/lobby-protocol.md) defines for it. Unset, every
browser player shares the gateway's address and limits.

The browser's `WS_PROXY_URL` is compiled into its `wasm-net` build, defaulting to
`wss://wss.vera20k.com`. An external launch overrides it for its match with
`[Relay] GatewayUrl` in `spawn.ini`. Either way the relay comes from the match's
grant, so that gateway must list it. Deploy a TLS reverse proxy at that origin and
forward `/game/*` with WebSocket Upgrade and `/v1/*` to this process. Set
`VERA_CLIENT_IP_HEADER` to the header that proxy writes with the browser's address
(for example `cf-connecting-ip`, or `x-forwarded-for`, whose last entry is used);
never set it when clients can reach the gateway directly. It is required behind
any proxy that does not connect over loopback, such as one on another host or in
a container: without it every player counts as the proxy's network for the
limits below. Preserve the browser Origin header; allow at least
70 seconds before an idle WebSocket timeout. The Node server binds
`127.0.0.1:8788` by default (`HOST` and `PORT` can override it). It serves no static
game files. The static site and gateway may be hosted separately.

LAN signaling is off by default. `VERA_LAN_SIGNAL=1` enables `/lan/signal` for
`wasm-lan` builds: it groups browsers by public address, assigns virtual IPv4s
and forwards WebRTC descriptions and candidates within the group; game datagrams
never pass through it. Behind a reverse proxy, route `/lan/*` WebSocket Upgrades
here; groups use the address from `VERA_CLIENT_IP_HEADER`. `VERA_LAN_STUN` optionally lists
comma-separated `stun:` URLs for networks that hide host candidates; TURN is
rejected. Limits are in `signal.mjs`: 256 connections, 16 members per group,
16 KiB messages and 64 incoming messages/Pings per second per connection.
JSON, Ping and Pong writes check queued output against 256 KiB per recipient
and 1 MiB across signaling connections, including a frame-header allowance.
A write that exceeds either budget retires its recipient through ordinary
member cleanup. A refused welcome never announces a join. A departed browser's
virtual address is held for two minutes, until its peers have dropped it; only
a signaling reconnect that asks for it back gets it.

Local development can use `VERA_WEB_ORIGIN=http://localhost:8000`. An HTTP lobby
requires both `VERA_ALLOW_LOCAL_HTTP=1` and a loopback lobby origin, for example
`VERA_LOBBY_URL=http://127.0.0.1:8790`. Remote HTTP is rejected. The browser must be
compiled with a matching local `ws://localhost:8788` proxy origin for this setup.

Game datagrams are authenticated per hop but not encrypted: the gateway carries
players' commands in plain text, and its HTTP proxy handles the lobby bearer and
responses holding tickets and packet keys. Treat the gateway as trusted lobby
infrastructure; do not log headers, bodies, tickets, keys or relayed datagrams.
It needs no relay signing key and no retail resources. WS Origin checks supplement
existing relay authentication; they do not authenticate arbitrary non-browser
clients.

Transport limits are defined together in `server.mjs`: 256 WebSockets, 128 pending
HTTP exchanges, 64 KiB request and 1 MiB response bodies, 2048-byte messages,
256 queued UDP packets / 256 KiB, and 1 MiB pending WS output. One network (an
IPv4 address, or the IPv6 /48 around one, as the lobby counts its shares) holds
about a tenth of each: 24 WebSockets, which is 12 players since each opens two,
and 12 HTTP exchanges; past them an upgrade gets 403 and a request 503. With
`VERA_CLIENT_IP_HEADER` set, a request without that header is refused (403);
without it, a loopback peer (a proxy connecting over loopback, or local tests) is
not counted. Each connection may pass up to 2048 combined
incoming/outgoing datagrams per second from a 16384/second aggregate shared
max-min fairly: each one-second window reserves for
every connection what it attempted in the previous window, up to an equal share,
and traffic past a reservation uses only unreserved budget. A datagram over budget
or behind a full queue is dropped, as UDP would lose it; the connection stays open
and only the original game protocol decides recovery. Text, empty or oversized
messages close the transport. Messages also have bounded fragmentation and
buffering. HTTP responses use stream backpressure and a 12-second absolute request
deadline.
Socket shutdown releases queues and the UDP port.
Pong replies to a client's Pings have a separate fair allowance of 64 per second
per connection and 1024 per second shared across game connections; a Ping past it
closes that transport through normal cleanup. Pongs and the gateway's own heartbeat
Pings must also fit the 1 MiB WS output queue, including a frame-header allowance,
or the slow transport closes. The heartbeat is not charged to the shared allowance,
so other connections cannot spend a quiet player's heartbeat, and control frames
never spend game-datagram credit. The 30-second WS Ping/Pong
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
