# Gameplay relay

The relay forwards authenticated datagrams and sequences modern online gameplay.
It owns the certified match clock and immutable command stream; clients own the
simulation. Legacy v2 datagram routing remains supported.
The shared codec in `../relay-protocol` binds each ticket to a match and player,
authenticates packet contents, rejects replayed game traffic, and challenges a new source address
before admitting it. Protocol v2 also encrypts gameplay payloads end to end
with XChaCha20-Poly1305. The relay forwards ciphertext and never receives the
match payload key. A new match requires new tickets and a new payload key;
there is no v1 or plaintext fallback.
The HTTPS lobby is trusted to distribute those keys. Encryption protects the
network path and relay; a compromised lobby remains outside that guarantee.

The signed 98-byte ticket contains version 2, match UUID, source index, expiry,
nonce, player UUID and the player's monotonic admission generation. A later
return-confirmed generation retires that player's previous hop membership; an empty
legacy room is removed. A sequenced match and its journal survive expired peers.
Generation tombstones last for the maximum 300-second ticket
lifetime, so old still-valid grants cannot recreate retired rooms. Renewals keep
the same player, match, generation and payload key. The client gets the payload
key only through authenticated HTTPS, derives a distinct source key, and uses a
fresh OS-random 128-bit nonce prefix plus a checked packet counter. Renewal or
reconstruction generates a new prefix; NAT rebinding retains the counter.

The relay bounds aggregate input at 16,384 datagrams/second and handshakes at
512/second; admitted principals get eight handshakes/second, enough for paired
old/new routes during renewal. The existing 2,048 packet/second principal cap
is joined by actual ciphertext output budgets: 2 MiB/second per sender and
64 MiB/second per room and relay. Whole broadcasts reserve all recipient bytes
before forwarding, preventing partial delivery when a budget is exhausted.
Fixed one-second windows allow ordinary short bursts, including simultaneous
NAT rebinding. Limits count ciphertext, framing and MAC bytes, not just input
packet count. Provider-level protection is still needed for link saturation.

Build from the repository root:

```sh
python -m tools.cargo_run -- build --release -p vera20k-relay
```

Install the executable as `/usr/local/bin/vera20k-relay`, create the unprivileged
`vera20k-relay` user, and install `vera20k-relay.service`. Allow UDP port 5000 in
the cloud firewall. Limit administrative SSH access to the management network.

Generate a 32-byte cryptographic random key outside the repository and encode it
as 64 hexadecimal digits. Store it in root-owned mode-0600
`/etc/vera20k/relay-ticket-key`; the unit supplies it through systemd credentials.
Set the identical key as the Cloudflare Worker's `RELAY_TICKET_KEY` secret. Never
place it in a command argument, source, test result, or client configuration.
Rotate the key during maintenance after existing matches have ended.

Set the Worker's public `RELAY_ADDRESS` to the server's IPv4 address and port.
Clients obtain individual signed tickets and packet keys over authenticated HTTPS;
they never receive the server signing key. A reconnecting player renews the same
match/player/source/generation, then replays its original cursor. Ticket expiry
and endpoint loss never write a departure or reset the clock or batch counter.

The service journal reports aggregate traffic and rejection counts once a minute.
These counts contain no player, endpoint, room identity, ticket or packet contents.
Use them to distinguish rate limits, expired grants and authentication failures
from packet loss and client timeouts.

Modern ordering uses authenticated control kind4 and a lobby-signed `VO1C`
certificate (see `../lobby/protocol.md`). All sources certify the same initial
world; fixed FPS deadlines then commit encrypted batches sorted by source and
batch ID, followed by an explicit frame end. An absent input contributes nothing.
Input joins the open frame after the sealed head. New clients opt in
to server-pushed updates through ordering request4 and its authenticated
`PushState` acknowledgement. Older clients continue pulling the same history.
The subscription is bound to the confirmed hop and expires after3seconds without
a receive watermark; renewing or rebinding a hop requires a fresh subscription.
Control-envelope ordering prevents a delayed enable from undoing a newer disable.
Retries are idempotent; modified batch retries and source impersonation fail
authentication.

Each pushed reader has64entries of independent delivery credit. Receive
watermarks acknowledge contiguous retained data, never simulation progress or
another reader's input. Each socket-loop drain rotates recipients and emits at
most128packets/96KiB total,32packets per room and8per reader, using the same
authenticated output budgets as replies. A250ms status heartbeat exposes a lost
tail. Clients use the existing Pull operation to repair gaps, reading at most32
entries at a time; healthy pushed clients do not continually request frames.

Synchronized pause is opt-in for every active match source. Ordering request5
uses request ID0 to advertise support and query status; positive IDs carry an
explicit desired state and expected epoch. Old clients and relays keep their
existing behavior, and a mixed room cannot pause. Accepted transitions append
an authenticated canonical pause marker after the current head's frame end.
That frame remains the barrier until an explicit resume appends its marker and
displaces the paused wall time; buffered input retains its assigned frames.
Pause and resume share the existing journal/cache writer, with both marker
reservations made before accepting pause. Retries and stale epochs never toggle
state. Lost markers replay through Pull. Ticket loss does not resume a game;
the last explicit Leave appends a resume marker so the departure tail can finish.

A federated match spreads its sources over up to six relays, each serving the
players nearest it. The lobby-signed `VO1F` roster (see `../lobby/protocol.md`)
rides in every CONFIRM after the cookie; a relay admits only the sources its
own roster entry homes and recovers the match link key with its ticket key. Each
relay seals its own players' entries at the shared frame deadlines and sends
them, authenticated with the link key, to every other relay as `VF1L` link
packets at each new seal and otherwise every 50 ms. Entries are retained and
resent until acknowledged, and a packet carrying entries goes out twice. A
frame is emitted once every relay sealed it, in the same canonical order, so
every relay delivers the same stream and sequence numbers. Player DATA to a remote source crosses one link to its relay. The
first roster relay picks the start time from measured link round trips; the
others follow its clock offset. Only it cancels a startup: another relay's Leave
before the start asks it to, and becomes a frame-1 departure if the start was
already on its way. Federated rooms cannot pause. Link packets go to the roster
address on the relay's existing UDP port and are authenticated by their MAC,
whatever address they arrive from.

The RAM hot cache retains4,096 entries/2 MiB per room under a64 MiB global
budget. Eviction from this cache does not evict match history: every complete
canonical frame first appends to the encrypted full-match disk journal. The data
file uses a length prefix per entry and the separate offset index is also on
disk; no full-match index grows in RAM. Push and Pull share one reader for the
cache and journal, always from the recipient's independent sequence. Active
journals therefore advertise oldest sequence1 throughout the match.

Poor or disconnected internet never removes a player. Healthy players continue;
the returning player can replay from its old cursor even after every short hop
ticket expired. Only explicit Leave or the certified24-hour match lifetime ends
participation. Explicit-all-left retains a300-second terminal replay grace before
exact-file cleanup. No reader acknowledgement gates advancement or retention.
The service checks deadlines every6ms even when no packets arrive.

Explicit Leave is retried on the original socket until a terminal acknowledgement
echoes that exact signed request. This idempotent operation alone remains valid
after ticket expiry: both MACs still verify, and an existing room checks its
certified source and retained principal/generation. An already expired, absent
room can acknowledge without recreating membership or storage. Other expired
operations remain rejected. Menu cleanup retains this transport until the
acknowledgement; network silence never creates a Leave request.

The unit owns `/var/lib/vera20k-relay` through `StateDirectory=vera20k-relay` and
`StateDirectoryMode=0700`. The executable requires `STATE_DIRECTORY`, or an
explicit absolute `VERA_RELAY_STATE_DIR` for local development. Journal files
are mode0600 in a mode0700 directory named only from the16-byte signed room ID.
They contain encrypted batches and public ordering metadata, never payload keys.
Logical quotas reserve1 GiB per room under8 GiB globally, admitting at most8
simultaneous archived matches; this does not preallocate disk space. Each room
reserves enough space for24hours of empty frame ends and all departures. Reaching
the optional-command quota backpressures new batches while the clock and already
accepted entries remain writable.

Unexpected journal I/O failure reports `StorageUnavailable` and preserves the
files, rather than silently deleting history or blaming a disconnected player.
Restart recovery is outside this implementation: a new process refuses an
existing room's files instead of overwriting them or inventing a new timeline.
Preserve these files for diagnosis; retire them only after the corresponding
match ended. The in-memory `RelayServer::new` constructor remains useful for
short protocol fixtures; deployed ordering uses `RelayServer::with_journal`.
