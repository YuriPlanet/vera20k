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

Each player gets eight handshakes/second, admitted or not, enough for paired
old/new routes during renewal; Leave acknowledgements, which need no admission,
share that budget. Leave for a room the relay no longer holds also has a
relay-wide 512/second. Only permitted handshakes count toward the relay's
512/second. Relay-wide input of 16,384 datagrams/second, and ordering output of
as many packets, count only datagrams that authenticate and pass their sender's
own limits, including the replay window and the 2,048 packet/second principal
cap, so forged, junk or replayed traffic cannot spend capacity other players
need. Rejecting one costs at most three HMACs. These packet caps are joined by
actual ciphertext output budgets: 2 MiB/second per sender and 64 MiB/second per
room and relay. Rooms share each relay-wide total max-min fairly: every second,
each room reserves what it asked for the second before, the largest asks cut to
one level so the reservations fit, and spends past its reservation only what no
room reserved. A flooding match therefore cannot take what others steadily use.
Handshakes, and Leave for a room no longer held, take part as one more room.
Whole broadcasts reserve all recipient bytes
before forwarding, preventing partial delivery when a budget is exhausted.
Fixed one-second windows allow ordinary short bursts, including simultaneous
NAT rebinding. Limits count ciphertext, framing and MAC bytes, not just input
packet count. Provider-level protection is still needed for link saturation.

Build from the repository root:

```sh
python -m tools.cargo_run -- build --release -p vera20k-relay
```

A deploy reaches players who have not updated. On a PR that touches the relay,
the Relay compatibility workflow (`tools/relay_compat.py`) plays the latest
stable release's and the newest dev build's client code against it, including a
player the relay removes for silence.

Clients from v0.2.4 read a Status, PushState, PauseStatus, Ready, Ack or Error
longer than they know by its known prefix, and an unknown error code as an
unknown reply. A canonical entry must keep its exact layout, because members
that read it differently would desync. A client that cannot read the record at
its cursor, or before the start any Status, for about 5 seconds ends the match
and asks for a newer VERA20k; v0.2.3 and older stall instead. Extend replies
only by appending fields, and add a record kind, departure reason or entry field
only after clients that read it have shipped. A new error code counts as
unreadable too, so one the relay repeats while a client's cursor waits ends that
client's match the same way.

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
and endpoint loss never reset the clock or batch counter, and write a departure
only once the silence limits below pass.

The service journal reports aggregate traffic and rejection counts once a minute.
These counts contain no player, endpoint, room identity, ticket or packet contents.
Use them to distinguish rate limits, expired grants and authentication failures
from packet loss and client timeouts.

Modern ordering uses authenticated control kind4 and a lobby-signed `VO1C`
certificate (see `../../docs/lobby-protocol.md`). Every source that becomes ready
certifies the same initial world; fixed FPS deadlines then commit encrypted
batches sorted by source and
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
state. After a resume nobody can pause for 30 seconds; a refused pause returns
the unchanged status. Lost markers replay through Pull. Ticket loss does not
resume a game;
the last explicit Leave appends a resume marker so the departure tail can finish.

Pace: clients send ordering request6 about once a second with no reply,
`[pace][frames run per second, LE u16]`, pace 0 keeping up, 1 behind, 2 catching
up; the first one advertises reading rate markers. Once every active source has
reported, a fresh behind report (at most 3 seconds old) slows the clock to the
fastest Game Speed rate at most nine tenths of what that source ran, at least
one row slower and at most two rows below the certificate's rate. Each change
appends an authenticated rate marker after the current head's frame end,
`[10][seq][frame][rate LE u16][source]`, source 255 for a raise; later frames are
due at the new rate from that moment. Changes are at least 5 seconds apart.
After 30 seconds with no fresh behind or catching-up report the clock tries one
row faster; a slowdown within 30 seconds of a raise doubles that wait, up to 5
minutes. Pauses displace these times like the start. Status keeps the
certificate's rate. Federated rooms keep the certificate's rate.

A federated match spreads its sources over up to six relays, each serving the
players nearest it. The lobby-signed `VO1F` roster (see `../../docs/lobby-protocol.md`)
rides in every CONFIRM after the cookie; a relay admits only the sources its
own roster entry homes and recovers the match link key with its ticket key. Each
relay seals its own players' entries at the shared frame deadlines and sends
them, authenticated with the link key, to every other relay as `VF1L` link
packets at each new seal and otherwise every 50 ms. Each entry is sent once, in a
packet that goes out twice, and is retained until acknowledged; it is sent
again only after going unacknowledged for the measured round trip plus 100 ms.
A relay that misses a packet ignores later ones until then. A
frame is emitted once every relay sealed it, in the same canonical order, so
every relay delivers the same stream and sequence numbers. Player DATA to a remote source crosses one link to its relay. The
first roster relay picks the start time from measured link round trips; the
others follow its clock offset. Only it cancels a startup: another relay's Leave
before the start asks it to, and becomes a frame-1 departure if the start was
already on its way. Federated rooms cannot pause. Link packets go to the roster
address on the relay's existing UDP port and are authenticated by their MAC,
whatever address they arrive from. Deploy every relay of a federation at
once: a relay rejects link entries newer than its own code, and one rejected
packet stalls the whole merged stream.

The RAM hot cache retains4,096 entries/2 MiB per room under a64 MiB global
budget. Eviction from this cache does not evict match history: every complete
canonical frame first appends to the encrypted full-match disk journal. The data
file uses a length prefix per entry and the separate offset index is also on
disk; no full-match index grows in RAM. Push and Pull share one reader for the
cache and journal, always from the recipient's independent sequence. Active
journals therefore advertise oldest sequence1 throughout the match.

A short outage never removes a player. Healthy players continue; the returning
player can replay from its old cursor even after every short hop ticket expired.
Any authenticated packet, keepalives included, keeps a source's place. A started
source silent for 10 minutes departs through the stream. Before the start, a
source that never sent Ready departs in frame 1 after 60 seconds of silence,
counted from its last packet even before the room existed, or once the room is 3
minutes old, and the others start without it; a Ready source waits. A federated
startup cannot drop a source alone: those limits, or any relay's room reaching 3
minutes unstarted, ask the coordinator to cancel it, and a relay whose
coordinator has then been silent for 10 seconds cancels on its own. A relay
reports only its own players' departures, and a restarted relay never does. Once
a started two-relay room has heard nothing from the other relay for 10 minutes,
that relay's players depart as lost after the last frame it sealed there, and
the room merges on without it: the match continues among the remaining players
or ends. With three or more relays the survivors could disagree on that frame,
so such a room ends instead once its own players have left and every other
relay has been silent for 10 minutes. A
departure's last byte is
DestroyConnection's error code: 1, connection lost, when silence ended the place;
otherwise the byte is absent. Otherwise only explicit Leave, the player's
admission on this relay with a newer lobby generation (recorded like Leave,
started or not), or the match's final frame ends participation. The certificate
bounds only a startup; a started room ends at its final frame, which pauses
postpone, and is retired regardless 48 hours and 3 minutes after it was created.
Once every source has departed, a
300-second terminal replay grace precedes exact-file cleanup. For 24 hours after
that cleanup, longer than any of its certificates lasts, a cleaned-up match
answers Ready with `Inactive`, so a player returning later cannot start a new
timeline under its ID; the marker lives in memory and a relay restart drops it. No reader
acknowledgement gates advancement or retention.
The service checks deadlines every6ms even when no packets arrive. Its pass over
every room starts a millisecond after the previous one ended, plus once after
each datagram it accepts, so rejected traffic, junk included, adds no pass.

Explicit Leave is retried on the original socket until a terminal acknowledgement
echoes that exact signed request. This idempotent operation alone remains valid
after ticket expiry, for as long past it as a room can live plus 300 seconds:
both MACs still verify, and an existing room checks its
certified source and retained principal/generation. An already expired, absent
room can acknowledge without recreating membership or storage. Other expired
operations remain rejected. Menu cleanup retains this transport until the
acknowledgement, or until 235 unanswered attempts 256 ms apart (about a minute);
the client then releases it, leaving the place to be treated as a lost
connection. Network silence never creates a Leave request.

The unit owns `/var/lib/vera20k-relay` through `StateDirectory=vera20k-relay` and
`StateDirectoryMode=0700`. The executable requires `STATE_DIRECTORY`, or an
explicit absolute `VERA_RELAY_STATE_DIR` for local development. Journal files
are mode0600 in a mode0700 directory named only from the16-byte signed room ID.
They contain encrypted batches and public ordering metadata, never payload keys.
Journal reservations share 8 GiB and never preallocate disk space. Each room
reserves all departures, an hour of empty frame ends at its certified FPS
(0.9–5.2 MiB) and 32 MiB of commands per certified source, and keeps what it
wrote. A new room is admitted only while the next hour of every live room, its
own included, still fits beside the reservations: about 114 two-player or 31
eight-player new matches at once at 45 FPS, 110 two-player at 60 FPS. A full
relay therefore refuses new matches rather than running ones: once less than
half an hour of frame ends is left, a live room reserves its next hour without
waiting on the quota, up to the 24 hours its certificate allows, as does a burst
of frames that outgrows the reserved hour; running rooms' later hours can take
the total past 8 GiB, to about 22 GB if 110 two-player 60 FPS matches all ran
for 24 hours. Reaching
the command allowance backpressures new batches while the clock and already
accepted entries remain writable. A finished room keeps only the bytes it wrote
through its replay grace. A room whose files could not all be created, or whose
exact files could not all be removed, keeps its reservation while the relay
retries the removal once a second.

Unexpected journal I/O failure, or a federated entry that no longer fits the
journal, reports `StorageUnavailable` and preserves the files, rather than
silently deleting history or blaming a disconnected player. The match stays
frozen; its room is released once its players' tickets have all expired, or 10
minutes after the failure. Its files stay, keep its match ID from starting a new
timeline, and count only their size on disk.
Leave still records the departure in memory and is acknowledged, so the client
may join another match.
Restart recovery is outside this implementation: a new process refuses an
existing room's files instead of overwriting them or inventing a new timeline.
Each leftover counts its size on disk, not a room's reservation. At startup, a
leftover last written longer ago than a room can live plus the 300-second grace
moves into the state directory's `archive` subfolder, where it counts against no
quota and blocks no match. The relay never deletes these files; preserve them
for diagnosis and remove them by hand. The
in-memory `RelayServer::new` constructor remains useful for short protocol
fixtures; deployed ordering uses `RelayServer::with_journal`.
