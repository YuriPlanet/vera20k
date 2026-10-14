# Networking: CnCNet-style UDP multiplayer

Roadmap for multiplayer in VERA20k. It describes the CnCNet model, the pieces the
repository already has, the target ownership inside `src/`, and the implementation
chains in dependency order so they can be built and merged one at a time.

Status: design. Nothing below is implemented unless the "Current state" section says so.

## Goal and non-goals

Goals:

- Several VERA20k clients play one deterministic lockstep match over UDP.
- On a local network, clients find each other from inside the app and play without
  any external program, internet service or configuration file. This is what gamemd's
  Network button does over IPX, and it is the native experience VERA20k reproduces
  over UDP.
- Over the internet, play first through the CnCNet client and its tunnels, later
  from VERA20k's own lobby.

Non-goals:

- Playing against gamemd. Lockstep needs bit-identical simulation. VERA20k uses
  `SimFixed` where native uses x87, so a mixed session desyncs. CnCNet provides
  matchmaking and NAT traversal, not compatibility.
- IPX. gamemd's Network button opens an `AF_IPX` socket (`0x007B10C0`). Modern
  systems have no IPX, so VERA20k replaces it with UDP behind the same role. This is
  a recorded, necessary divergence from native.
- Reproducing gamemd's wire format. Only VERA20k clients talk to each other, so the
  packet layout is VERA20k's own. The native event semantics (opcodes, send policies,
  frame stamps, checksum history) are kept, because they decide gameplay behavior.

## How CnCNet works

CnCNet is three separate GPL-3 programs:

| Piece | What it does | Relevance to VERA20k |
|---|---|---|
| Client (`xna-cncnet-client`, C#) | IRC lobby, game negotiation, writes `spawn.ini`, launches the game, relays game UDP through tunnels | Launcher for the initial port |
| Tunnel server (`cncnet-server`, C#) | UDP relay for NAT traversal, announces itself to an HTTP master list | Transport VERA20k can talk to directly later |
| YR spawner (`yrpp-spawner`, injected DLL) | Makes gamemd start a match from `spawn.ini` instead of its menus | Not needed: VERA20k reads `spawn.ini` itself |

### Tunnel protocol, version 3

From `CnCNet/Net/Tunnel/TunnelV3.cs` in the tunnel server:

- UDP, default port 50001. Datagrams shorter than 8 bytes are dropped; receive buffer 1024 bytes.
- Header: `u32 sender_id` at offset 0, `u32 receiver_id` at offset 4, native byte
  order on the server (little-endian in practice). Payload follows unchanged.
- Ping: sender 0, receiver 0, exactly 50 bytes. The server echoes the first 12 bytes.
- Sender 0 with receiver `u32::MAX` is a maintenance command; any other sender-0 packet is dropped.
- A sender id is bound to the first endpoint that uses it. Packets with equal
  sender and receiver ids are dropped. Default limit 8 ids per IP, 200 clients.
- Master list: the server sends `GET <master>?version=3&name=..&port=..&clients=..&maxclients=..&masterpw=..&maintenance=..`
  every 60 seconds. Clients read `http://cncnet.org/master-list`, one tunnel per
  line, semicolon-separated: `address:port;country;countrycode;name;password;clients;maxclients;status;lat;lon;version;distance`.

### How the client launches a game (v3 mode)

From `GameLobbyBase.cs` and `CnCNetGameLobby.cs` in the client:

- The client reserves a local UDP port and writes `[Tunnel] Ip=127.0.0.1 Port=<reserved>`
  into `spawn.ini`. The game sends everything to loopback; the client's bridge
  forwards per player through the tunnels. The game never needs NAT traversal.
- `[Settings]`: `Name`, `Scenario`, `UIGameMode`, `UIMapName`, `MapID` (official maps only),
  `PlayerCount`, `Side`, `IsSpectator`, `Color`, `CustomLoadScreen`, `AIPlayers`, `Seed`,
  `MapSHA1`, `GameID`, `Host`, `Port`, plus `CoachMode`, `AutoSurrender` and any
  `[ForcedSpawnIniOptions]` keys from the client's `GameOptions.ini`.
- `[Other1]`..`[OtherN]`: one per remote human. `Name`, `Side`, `IsSpectator`, `Color`, `Ip`, `Port`.
- `[HouseHandicaps]`, `[HouseCountries]`, `[HouseColors]`, `[SpawnLocations]`, `[IsSpectator]`:
  keyed `MultiN`. AI players appear only here and in `AIPlayers`, never in `[OtherN]`.
- The host broadcasts `STARTV3 <GameID>;<id>;<name>;<tunnelIp>:<tunnelPort>;...` to the
  lobby; each client's bridge resolves the per-player route from it.
- The client then starts the configured executable with the spawner flags
  (`-SPAWN` for gamemd) and waits for it to exit.

Facts to confirm from client source before implementing stage 5 (not verified here):
how the loopback bridge distinguishes remote players (one local port per player, or a
single port with the game's own addressing), what `GetIPAddressForPlayer` returns in the
CnCNet lobby, and which statistics or result packets the client expects after the match.
The authoritative files are `V3GameTunnelHandler.cs`, `TunnelHandler.cs` and
`GameProcessLogic.cs`.

## Current state in the repository

| Area | State |
|---|---|
| `src/net/lockstep.rs` | Ported native command staging and dispatch: send policies, FrameInfo (opcode `0x1c`), checksum history, single-player and network transfer. Test-only; no production caller. |
| `src/net/lockstep_sim_convergence_tests.rs` | Two-instance convergence tests over the queue. |
| `src/sim/multiplayer_checksum.rs` | Retail per-frame sync checksum. Not fed into the lockstep history. |
| `MatchSession` (`src/app/match_runtime/session.rs`) | Stage 1 done: the live session owner (mode, participant list, local participant, host flag). The launch path writes Campaign or Skirmish and the saved-game restore follows the persisted classification; the modal pump, elapsed clock and command bar read it. No writer produces `Lan` or `Wol` yet. |
| `src/app/frontend/skirmish_session.rs` | Reads the local multiplayer preferences (handle, game mode) that native refreshes before the Network dialog. |
| `src/skirmish_launch.rs` | Pre-fill plan already models several human nodes plus AI slots. |
| Main menu Network button | `bounce_network_to_main_menu` in `src/app/shell_main_menu.rs` reproduces native's silent return when the IPX socket fails. |
| Sockets, protocol, lobby | None. `src/net/mod.rs` reserves `protocol`. |

## Target ownership

`sim/` stays transport-agnostic. `net/` depends on `sim/` only. App code orchestrates.

| Responsibility | Owner |
|---|---|
| Command staging, frame stamps, dispatch order, checksum history | `net::lockstep` (exists) |
| Wire framing, peer table, send and receive, tunnel header | `net::protocol` (new) |
| LAN discovery: announce, listen, game list | `net::discovery` (new, stage 7) |
| Tunnel discovery: master list, ping, selection | `net::tunnel` (new, stage 8) |
| Session mode, player list, local player id, host flag | `app::match_runtime::session::MatchSession` (exists) |
| `spawn.ini` parsing into a launch descriptor | `app/frontend` launch path, reusing `skirmish_launch` types |
| Per-frame network service in the game loop | `app/match_runtime/sim_tick` (stage 6) |
| Lobby UI | `ui/` on top of the skirmish shell (stages 7 and 9) |

Rules that apply throughout: one owner per state, no competing command paths, every
RNG draw and timer in native frame order, presentation never reads the socket.

## Implementation chains, in dependency order

Each stage is one PR. A stage is done when its "Done when" holds and the usual lib
suite and Clippy pass. Stages 1 to 6 produce the initial game port: a VERA20k that the
CnCNet client can launch and play. Stage 7 adds native LAN play from the app. Stages 8
and 9 move internet play into the app.

Stages 3, 4 and 6 are shared by LAN and internet play and must not assume loopback:
the transport binds a LAN-reachable address, and the peer table holds real endpoints.

### Stage 0: determinism groundwork

Scope: hook `compute_retail_multiplayer_checksum` into `MultiplayerChecksumHistory`
per frame, and extend the convergence tests to cover the command types the live game
issues. Add a headless two-instance test that runs a scenario for N frames and compares
checksums every frame.

Done when: the two-instance test passes on macOS, Linux and Windows CI with identical
checksums per frame.

Why first: every later stage debugs against this. A desync found after the transport
exists is far harder to attribute.

### Stage 1: live session mode (done)

Scope: replace the hard-coded `current_session_mode` with a value the launch path
writes. Introduce the session owner holding mode, player list, local player index and
host flag. Skirmish and campaign keep writing their modes.

Done when: the modal pump, elapsed clock and any other `is_network` consumer read the
live value, and offline play is unchanged.

### Stage 2: lockstep queue in production, offline path

Scope: route live command issue through `SynchronizedCommandQueue`, using
`transfer_single_player` and `dispatch_due_offline`. Delete the direct command path it
replaces. Keep RNG and same-frame ordering identical; the convergence tests and the
stage 0 checksum test guard this.

Done when: skirmish and campaign play through the queue with no behavior change, and
the old path is gone.

Why before transport: the queue's frame-stamp and ahead-window semantics are native
behavior the game must already obey before a second client exists.

### Stage 3: UDP transport and network lockstep

Scope: `net::protocol` with a `Transport` abstraction over a UDP socket bound to a
configurable address, loopback for the CnCNet bridge and the LAN interface for native
play. Wire format: a small versioned little-endian header (protocol version, game id,
player id, packet kind) followed by lockstep command records. Peer table keyed by
player id with an address per peer. Enable `transfer_network` and `dispatch_due_network`, exchange
FrameInfo, compare checksums, raise a desync event.

Done when: two VERA20k instances on one machine, started from a test harness or a
command-line flag, play a scenario in lockstep and detect an injected desync.

Evidence: a headless two-process test plus side-by-side play.

### Stage 4: multi-human match bootstrap

Scope: the loading path builds houses from a negotiated player list instead of one
local human plus AI slots. Each human gets its house index, side, color, spawn
location and spectator flag from the launch descriptor. Seed comes from the host.

Done when: the two instances from stage 3 load the same map with different local
players and identical world state (stage 0 checksum).

### Stage 5: `spawn.ini` launch and CnCNet client bridge

Scope: a `-SPAWN` style command-line flag that reads `spawn.ini` from the game
directory, builds the launch descriptor from `[Settings]`, `[OtherN]`, `[House*]`,
`[SpawnLocations]` and `[IsSpectator]`, points the transport at `[Tunnel]` on loopback,
and starts the match directly. Parse with the native readers in `src/rules/ini_value.rs`.
Confirm the bridge addressing facts listed above first.

Done when: a VERA20k-configured CnCNet client hosts a game, launches VERA20k on two
machines through a tunnel, and both play to completion.

Deliverable outside the repo: a client configuration package (executable name, game
id, `GameOptions.ini` with `[ForcedSpawnIniOptions]`) kept in its own repository.

### Stage 6: in-game network service

Scope: per-frame send and receive inside `advance_in_game_runtime`, the ahead-window
pacing and frame-send-rate policy, timeout and player-drop handling, and opcode `0x13`
EXIT routing for local versus remote players, which `net::lockstep` leaves to the
session owner. Reconnect is not native and is out of scope.

Done when: a player quitting or losing connection is removed as in native, the others
continue, and the match reports its result.

This stage completes the initial game port.

### Stage 7: LAN discovery and native LAN play

Scope: `net::discovery` announces a hosted game by UDP broadcast on a fixed port
(game id, protocol version, host name, map, mode, player count, game port) and lists
announcements heard on that port, with expiry. Host and join screens built on the
skirmish shell: game list, player list, sides, colors, ready state, chat, and map and
options broadcast from the host. Join negotiates the player list and seed, then
stage 4 bootstraps and stage 3 carries the match over the LAN endpoints. The
main-menu Network route replaces `bounce_network_to_main_menu` here, taking the role
of gamemd's IPX game list and lobby (state 0x10 onward from `0x0052DD75`). Session
mode is `Lan`.

Done when: two machines on one LAN host, see, join and play a full match from the
app with no CnCNet client, tunnel, internet access or configuration file.

Evidence: side-by-side play on two machines, plus a two-process discovery test on one
host.

### Stage 8: built-in tunnel client

Scope: `net::tunnel` fetches the master list, parses the twelve-field lines, pings
tunnels with the 50-byte probe and sends game traffic with the 8-byte v3 header. Peer
addresses in the stage 3 peer table become tunnel ids. The stage 7 lobby gains a
direct-IP join for internet play without a lobby server.

Done when: two VERA20k instances play through a public or self-hosted tunnel with no
CnCNet client running.

Before using public tunnels under a new game id, ask the CnCNet team. The tunnel
server is open source and self-hosting is the fallback.

### Stage 9: in-app internet lobby

Scope: a CnCNet IRC lobby speaking the client's dialect under a VERA20k game
identifier, so VERA20k games never appear joinable to gamemd players. It reuses the
stage 7 lobby screens with the game list fed from IRC instead of broadcast.

Done when: a full internet match is set up and played without any external program.

## Validation per stage

- Determinism: stage 0 checksum test runs in every later stage's suite.
- Transport and discovery: headless two-process tests in `src/net/`, never against retail assets in the lib suite.
- LAN: side-by-side play on two machines before stage 7 merges.
- Gameplay: side-by-side play against retail for anything the lockstep semantics change.
- Each PR states which evidence level each claim reached, per the project contract.

## Sources

- Client: <https://github.com/CnCNet/xna-cncnet-client> (`GameLobbyBase.cs`, `CnCNetGameLobby.cs`, `CnCNetTunnel.cs`)
- Tunnel server: <https://github.com/CnCNet/cncnet-server> (`CnCNet/Net/Tunnel/TunnelV3.cs`)
- YR spawner: <https://github.com/CnCNet/yrpp-spawner>
- Master list: <http://cncnet.org/master-list>
- v3 tunnel design notes: <https://opencollective.com/xna-cncnet-client-v3-tunnels>
