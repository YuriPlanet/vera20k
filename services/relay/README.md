# Gameplay relay

The relay forwards authenticated datagrams and sequences modern online gameplay.
It owns the certified match clock and immutable command stream; clients own the
simulation. Older clients, which speak envelope v2, are not supported.
The shared codec in `../relay-protocol` binds each ticket to a match and player,
authenticates every datagram per hop, rejects replayed game traffic, and
challenges a new client's address before admitting its hop; an admitted hop
then moves to a new address without a challenge (see below;
`bandwidth-compaction/spec.md` holds the full rules). Every datagram is
envelope v3, 13 bytes around its payload of at most 720:

```
[hdr: version 3 in bits 5..7, kind in bits 0..4][session u32][seq u32][payload][tag 4]
```

The tag is the first 4 bytes of HMAC-SHA256 under the hop's packet key over
`"VERA20k relay client v3\0"` or `"VERA20k relay server v3\0"`, then the header
and payload, compared in constant time. HELLO and CONFIRM carry the ticket in
their payload and session 0, as does the CHALLENGE answer; the relay assigns a
random non-zero session on CONFIRM and hands it over in the envelope of its first
READY (sequence 1), which the client claims by its tag. Every later datagram of
the hop names it by that session. Gameplay travels plaintext: DATA is
`[target]` or `[source]` then the message, and ordered batches are the plain
command records. The relay and its journal can read every command; the
transport no longer protects payloads from the relay. The lobby still grants a
match payload key, which clients accept and do not use.
The HTTPS lobby is trusted to distribute tickets and packet keys.

The signed 98-byte ticket contains version 2, match UUID, source index, expiry,
nonce, player UUID and the player's monotonic admission generation. A later
return-confirmed generation retires that player's previous hop membership; an empty
legacy room is removed. A sequenced match and its journal survive expired peers.
Generation tombstones last for the maximum 300-second ticket
lifetime, so old still-valid grants cannot recreate retired rooms. Renewals keep
the same player, match and generation, and are a new hop with a new session;
the old session then takes only Leave, so a player quitting before the new
READY still leaves, and is released 300 seconds after its old ticket expires,
so renewals every few minutes do not fill the session table. A released session
is never assigned again for 10 minutes. Each hop counts its sequences from 1
to `u32::MAX` (a 300-second ticket never comes near it), and a relay that ran
out would stop sending on the hop. An authenticated datagram whose sequence
the replay window accepts moves its hop to the address it came from (NAT
rebinding), push subscription included; a replay or forgery cannot. This
skips the path check the handshake makes, a deliberate trade-off: an on-path
attacker who forwards a copy first can divert the reader's push stream until
the client's next datagram moves it back, which such an attacker could disrupt
anyway; source address spoofing is out of scope. A datagram with an unknown
session is dropped without an HMAC and counts in `unregistered`, one with a
wrong tag only by itself and counts in `authentication`; a session past 4,096
wrong tags in a fixed minute adds one `session_alerts`. No count ever drops a
valid datagram.

Each player gets eight handshakes/second, admitted or not, enough for paired
old/new routes during renewal; Leave acknowledgements, which need no admission,
share that budget. Leave for a room the relay no longer holds also has a
relay-wide 512/second. Only permitted handshakes count toward the relay's
8,192/second: 1,024 players at their own rate, more than the lobby's sessions and
an hour's new guests together (`../../docs/lobby-protocol.md`). A Pull that
reads the journal spends its sender's own 256/second before it reads.
Relay-wide input of 16,384 datagrams/second, and ordering output of as many
packets, count only datagrams that authenticate and pass their sender's own
limits, including the replay window and the 2,048 packet/second principal cap,
so forged, junk or replayed traffic cannot spend capacity other players need.
Rejecting one before any budget costs at most three HMACs; a CONFIRM within the
handshake budget then checks its address cookie and, in a federated match, its
roster. These packet caps are joined by actual output byte budgets: 2 MiB/second
per sender and 64 MiB/second per room and relay. Rooms share each relay-wide total max-min fairly: every second,
each room reserves what it asked for the second before, the largest asks cut to
one level so the reservations fit, and spends past its reservation only what no
room reserved. A flooding match therefore cannot take what others steadily use.
Handshakes, and Leave for a room no longer held, take part as one more room.
Whole broadcasts reserve all recipient bytes
before forwarding, preventing partial delivery when a budget is exhausted.
Fixed one-second windows allow ordinary short bursts, including simultaneous
NAT rebinding. Limits count payload, framing and tag bytes, not just input
packet count. Provider-level protection is still needed for link saturation.

Build from the repository root:

```sh
python -m tools.cargo_run -- build --release -p vera20k-relay
```

A deploy reaches every version the lobby admits. On a PR that touches the relay,
the Relay compatibility workflow (`tools/relay_compat.py`) builds a compat client
against the protocol crate of each stable release from `OLDEST_ADMITTED` (the
lobby's lowest admitted version) on and of the newest dev build, and plays each
against it, including a player the relay removes for silence.

Clients read a Status, PauseStatus, Ready, Ack or Error longer than they know
by its known prefix, and an unknown error code as an unknown reply. A canonical
entry must keep its exact layout, because members that read it differently
would desync. A client that cannot read the record at its cursor, or before the
start any Status, for about 5 seconds ends the match and asks for a newer
VERA20k. Extend replies only by appending fields, and add a record kind,
departure reason or entry field only after every admitted release reads it. A
new error code counts as unreadable too, so one the relay repeats while a
client's cursor waits ends that client's match the same way. A change that
breaks admitted releases on purpose raises `OLDEST_ADMITTED` past them, the
lobby raises its floor to match before the relay deploys, and the PR carries
the `relay-compat-break` label.

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

The service journal reports aggregate traffic and rejection counts once a minute,
with the ordering push datagrams and their bytes as `pushed` and `pushed_bytes`
the Pull requests answered as `pull_replies`, the matches it ended with a
desync record as `desyncs`, and the startups it refused for a different initial
world as `start_mismatches`. The reader downlink adds `push_packets` and
`push_bytes` by kind (status, new pack, repeat, retransmission, tail probe),
`piggybacked_records`, `down_lost` datagrams and the `retransmitted_records`
for them, `readers_avg` (subscribed readers per state: good, lossy, congested,
offline) and `offline_unsent`, about one new pack per head an offline reader
missed.
These counts contain no player, endpoint, room identity, ticket or packet contents.
Use them to distinguish rate limits, expired grants and authentication failures
from packet loss and client timeouts.

Modern ordering uses authenticated control kind 4 and a lobby-signed `VO1C` or
`VO2C` certificate (see `../../docs/lobby-protocol.md`). The relay orders a match of
two or more sources, or of one whose `VO2C` flags bit 1 the lobby set: one person
who watches AI play, ordered so that others can watch it
(`OrderingCertificate::relay_orders`). It refuses the Ready of any other one-source
certificate (`Authentication`), whose match runs locally. Every source that becomes ready
certifies the same initial world; fixed FPS deadlines then commit batches
sorted by source and
batch ID, followed by an explicit frame end. An absent input contributes nothing.
The clock seals in turns of `turn_frames(rate)` = ⌈rate / 15⌉ frames (4 at
60 Hz, 3 at 45, 2 at 30 and 20, 1 at 15 and slower), counted in whole turns
from the last rate change, or from the start; the final frame and the last
departures still seal at once. Input joins the open frame after the sealed head,
the first frame of the next turn. A turn's frames reach the players together
when its last frame is due, so clients play them a turn's span behind receipt,
evenly, and count a backlog only beyond a whole turn.

Clients send everything but Ready (request 0), Pull (2), Leave (3) and Pause
(5) in one control request, UPLINK (8), scheduled by the client session
(`adaptive-transport/spec.md` holds the fields and scheduling rules,
`bandwidth-compaction/spec.md` §7 this encoding):

```
[8][flags][recv_next varint][down_high varint][down_bits u64][srtt_ms varint][base_ms varint][rttvar_ms varint][link]
[checksum_count 0..2]{[frame varint][checksum u32]} [pace u8][fps varint][tick µs varint if flags bit1]
[batch_count 0..4][first_batch_id varint if any]{[len varint 1..512][batch]}
```

A varint is unsigned LEB128 in its shortest form, at most 10 bytes, within the
field's former width (frames u32, milliseconds u16); anything else is
malformed. Pull is `[2][next varint]`, Pause `[5][request id varint][expected
epoch varint][paused]`, Error `[6][code][expected varint]` and PauseStatus
`[8][supported u32][active u32][request id varint][epoch varint][paused][frame
u32]`.

Flags: bit0 PUSH subscribes or renews push, bit1 PACE, bit2 PROBE asks for a
Status, bit3 RESTORING keeps the subscription without new records; `link` is
the client's uplink state (0 good, 1 lossy, 2 congested, 3 offline) with bit2
DELAYED and bit3 JITTERY; srtt, base and rttvar are whole milliseconds, 0
before the first sample. Any other length, count or reserved bit is malformed and dropped. The
relay merges `recv_next` into the reader's acknowledged watermark (it never
moves back), judges its downlink loss from `down_high`/`down_bits` against the
hop's send log, then takes the batches in ID order: an ID below the last
accepted is skipped, the last one again must be identical (else ModifiedRetry),
the next is accepted under the usual frame and journal limits (else
Backpressure), a later one is a BatchGap; before the start NotStarted, after
leaving Inactive. An UPLINK with batches and no error gets one ACK, duplicates
included; an error gets only the error; a probe without batches gets a Status;
anything else no reply. The subscription is bound to the confirmed hop and
expires after 3 seconds without an UPLINK; renewing or rebinding a hop starts
unsubscribed, and the client's first UPLINK there subscribes again. Retries are
idempotent; modified batch retries and source impersonation fail
authentication.

Every datagram's envelope sequence feeds a 64-bit receive window at the other
end. ACK, `[2][accepted through varint][frame varint]`, and Status,
`[1][head frame varint][oldest seq varint][next seq varint][active sources
u32][tick Hz u16][start delay ms varint][started]`, both end in the relay's
window for the hop and the delay since its newest datagram, `[up_high
varint][up_bits u64][echo delay ms varint]`: the client
measures its RTT and uplink loss from them. A player's Status then names the
silent players, if any (Silence, below). Each side judges its own sends
three behind the newest feedback, once per feedback: a missing send counts as
lost only if it went srtt + 4 rttvar before the feedback arrived (the relay
uses the reader's reported srtt and rttvar, 100 / 50 ms before a report), and
the loss folds every 32 judged packets. A direction is offline after 1000 ms
(client) or 1500 ms (relay) of silence; congested while DELAYED (srtt - 2
rttvar at least base + max(base, 100)) or (uplink) 2 s after a Backpressure,
and for 10 s after that last held; lossy from 5‰ until 2‰ with no loss for
20 s, severe from 150‰ until 100‰. Each direction starts lossy (the uplink
at the start, a reader's downlink at its subscription) until its first fold
decides, and a renewed hop keeps the measured loss, the state and both
holds. The client also reports JITTERY when the arrival jitter of new-turn
packs, RFC 3550 smoothed, reaches max(10, turn_ms / 4) ms over 16 samples,
until half that.

History entries travel packed (PACK): consecutive entries fill one datagram
up to a 681-byte payload as `[12][base seq varint][base frame varint][count]`,
then per entry a tag byte (kind in bits 0..3; bits 4..5 the frame: same as the
previous, the next, or a varint delta of at least 2 after the tag; bit 6 a
BATCH's ID one more than the pack's previous BATCH of its source) and the
entry's fields: BATCH `[source][batch id varint unless bit 6][len varint][batch]`,
DEPARTURE `[source][lost 0 or 1]`, PAUSE `[epoch varint][paused][source]`, RATE
and DESYNC their canonical bytes after the frame. The relay writes the one
shortest encoding and the client refuses any other. Each pack decodes alone:
the client rebuilds each canonical entry, byte for byte, with sequence base plus
its index and reads it as if it had come alone. Status, Ack, Error and the other
replies stay datagrams of their own. Each pushed reader has 64 entries of
independent delivery credit, counted in new entries only. Receive watermarks
acknowledge contiguous retained data, never simulation progress or another
reader's input. Each socket-loop drain rotates recipients and emits at most 128
datagrams/96 KiB total, 32 datagrams per room and 8 per reader, using the same
authenticated output budgets as replies. Per reader, in order: a status
heartbeat every 250 ms or when the start or active sources change; the
retransmission of what lost packs still owe; new packs, whose free space first
carries the unacknowledged records of the reader's K newest packs (K 1 good, 2
good and JITTERY, 2 lossy, 3 severe, at most 2 congested) without ever adding a
pack; then, from a relay-wide 8192 a second, the entries of a lossy, or good
and JITTERY, reader once more 8 ms after they were first sent (never while
congested), and a tail probe of its newest pack after a PTO without
acknowledgement progress. An offline reader gets nothing and restarts from its
watermark when it is heard again. Clients repair gaps first by an UPLINK, then
by the existing Pull after a grace of about an RTT plus a turn: a reply is a
Status, then at most 32 entries packed.

Synchronized pause is opt-in for every active match source. Ordering request 5
uses request ID 0 to advertise support and query status; positive IDs carry an
explicit desired state and expected epoch. Each source advertises once its
match has started, and the room can pause after every active source has. The
relay counts the asking source before it answers, so a PauseStatus whose
supported mask leaves the asker out means the room never pauses (a federated
room, below), and the game stops asking. Accepted transitions append
an authenticated canonical pause marker after the current head's frame end.
That frame remains the barrier until an explicit resume appends its marker and
displaces the paused wall time; buffered input retains its assigned frames.
Pause and resume share the existing journal/cache writer, with both marker
reservations made before accepting pause. Retries and stale epochs never toggle
state. After a resume nobody can pause for 30 seconds; a refused pause returns
the unchanged status. Lost markers replay through Pull. Ticket loss does not
resume a game;
the last explicit Leave appends a resume marker so the departure tail can finish.

Pace: clients report about once a second in the next UPLINK's pace segment,
once, with no reply: pace 0 keeping up, 1 behind, 2 catching up, the frames run
in the last second and their recent 90th-percentile simulation tick cost in µs,
0 for none; the first one advertises reading rate markers. A
reporter holds the fastest rate its tick cost fits into four fifths of a second
(legacy reports: nine tenths of what it ran). A fresh behind report (at most 3
seconds old) from a player who cannot hold the current rate slows the clock to
the fastest Game Speed rate that player holds, at least one row slower and at
most two rows below the certificate's rate. When fresh behind reports span two
seconds of running fewer frames than the clock, the relay also limits the rate
to nine tenths of the measured frames per second. This measured capacity can
hold back a raise for five minutes, unless the player's ticks become at least
10% quicker. Under `VO1C` every active
source must have reported, so a member that may not read markers never meets one;
under `VO2C` silent sources are ignored, and a certificate whose pacing flag is
off keeps its rate. Each change appends an authenticated rate marker after the
current head's frame end, `[10][seq][frame][rate LE u16][source]`, source 255
for a raise; later frames are due at the new rate from that moment. Changes are
at least 5 seconds apart. After 30 seconds with no fresh behind or catching-up
report, and while every tick-cost reporter holds the next faster rate, the clock
tries one row faster; a slowdown within 30 seconds of a raise doubles that wait,
up to 5 minutes. Pauses displace these times like the start. Status keeps the
certificate's rate. A `VO2C` with flags bit 2 (continuous pacing) instead lets
the clock seal at any whole rate from the certificate's down to 10: a behind
player slows it to its holdable rate, at least one frame and at most 25%
slower per change; a player catching up for 10 seconds straight slows it 5%
once the last change is 10 seconds old. After a third of the raise wait it goes
half the way to the slowest raise ceiling, at least one frame and at most a
tenth of the rate per step. The ceiling is a reporter's tick cost and 95% of
what it ran in a 10-second backlog or in a second sustained shortfall within
five minutes, unless its ticks got 10% quicker since.
Its rate markers carry any rate from 10 to 90; each game checks the rate
against its certificate. In a federated room the coordinator decides from every
relay's reports (see below). The relay's
per-minute journal line counts the slowdowns as `slowdowns`, the raises as
`raises`, and the reports of a player catching up or behind, about a second of
catching up each, as `catching_up_s`. A paused, rejoining or hidden game
reports keeping up.

Checksums: batches carry only command records, not checksums.
Every 15th frame each client sends its post-Logic checksum in the next UPLINK's
checksum segment, with no reply: the newest sample and the one before, so one
lost datagram loses nothing, and alone within 100 ms if no other UPLINK goes. The relay
keeps each active source's first value per sampled frame for the newest 256
sampled frames. It takes none for a frame past the one after its sealed frames
(a client reports frame F once it has run F - 1), and a source's values stop
counting once it leaves. Once one frame's values differ it names the outlier: the first
reporter off a strict majority of the active and reporting sources, or, once
every active source reported without one (two players), the later of the first
two that differ, against the earlier. It then appends the match's one desync
record after the current head's frame end, `[11][seq][frame][LE sample frame
u32][outlier source][LE outlier checksum u32][other source][LE other checksum
u32]`, and compares nothing more. Each client ends the match with a desync
error, naming the outlier's House (or the other side's, if it is the outlier),
as it reaches the frame after that record.

A federated match spreads its sources over up to six relays, each serving the
players nearest it. The lobby-signed `VO1F` roster (see `../../docs/lobby-protocol.md`)
rides in every CONFIRM after the cookie; a relay admits only the sources its
own roster entry homes and recovers the match link key with its ticket key. It
refuses Ready as an agreement mismatch unless the roster homes exactly the
certificate's sources. Each
relay seals its own players' entries at the shared frame deadlines and sends
them, authenticated with the link key, to every other relay as `VF1L` or `VF2L` link
packets at each new seal and otherwise every 50 ms. Each entry is sent once, in a
packet that goes out twice, and is retained until acknowledged; it is sent
again only after going unacknowledged for the measured round trip plus 100 ms.
A relay that misses a packet ignores later ones until then. A
frame is emitted once every relay sealed it, in the same canonical order, so
every relay delivers the same stream and sequence numbers. Player DATA to a remote source
crosses one link to its relay. Both paths forward to addressed peers other than the
sender, through 30 seconds since each peer was last heard from. The first roster relay picks the start time from measured link round trips; the
others follow its clock offset. It alone cancels a startup for an ordinary
reason: another relay's Leave before the start asks it to, and becomes a
frame-1 departure if the start was already on its way. A relay that proves an
agreement mismatch (below) refuses the start itself and asks it to cancel the
rest; one that hears another link version (below) cancels its own startup.
Federated rooms cannot pause. The other relays forward
their players' latest pace reports to the coordinator every 500 ms as link kind
2, `[count]` then per source `[source][pace][LE frames u16][LE tick µs u32, 0
none][LE age ms u32]`. The coordinator's rate change travels as a contribution
of source 255 (batch ID `u64::MAX`, `[LE rate u16][source]`) for the frame after
its seal; every relay emits the same rate marker after that frame's end and
seals later frames at the new rate from that frame's due time on its shared
start timeline. The others forward their players' checksum reports to the
coordinator as they arrive, as link kind 3, `[count]` then per report
`[source][LE sample frame u32][LE checksum u32]`; only the coordinator compares
them, and its desync travels as a contribution of source 255 (batch ID
`u64::MAX - 1`, the record's 14 bytes after its frame) for the frame after its
seal, which every relay emits after that frame's end. Every relay also tells
every other when it last heard its players (link kind 4, Silence below). Link packets go to the roster
address on the relay's existing UDP port and are authenticated by their MAC,
whatever address they arrive from. Deploy and roll back every relay at once,
so all run one version: relays of different versions can end the same startup
differently. The digit in the link magic carries the link version and the
marking together: this version sends `VF1L` unmarked and `VF2L` marked. The
version is bumped whenever the layout or meaning of an existing link kind
changes (the FRAME header and its entries, a coordinator control batch, DATA,
pace, checksum or silence reports), and a bump moves both magics to digits no
build has used (`VF3L` and `VF4L` next), so no two versions share a digit. A
relay refuses a packet it cannot read: one refused FRAME stalls the
merged stream, and refused reports leave desyncs and slow players unnoticed.
Only a new kind whose loss changes no result needs none, as kind 4 did: an
older relay only leaves that silence untold. The 32-byte link header and the
MAC domain stay the same in every version, so a relay that authenticates a link
packet of another version cancels that startup at once. A relay too old to
know this drops the packet and cancels at its 3-minute loading limit. Relays of
different link versions therefore never start a match together.

The RAM hot cache retains 4,096 entries/2 MiB per room under a 64 MiB global
budget. Across rooms, the relay evicts the oldest cached entry by its estimated
frame due time. After a speed change, that estimate follows the same start clock
as sealing, including a follower's clock corrections. Eviction from this cache
does not evict match history: every complete
canonical frame first appends to the full-match disk journal. The data
file uses a length prefix per entry and the separate offset index is also on
disk; no full-match index grows in RAM. Push and Pull share one reader for the
cache and journal, always from the recipient's independent sequence. Active
journals therefore advertise oldest sequence 1 throughout the match.

A short outage never removes a player. Healthy players continue; the returning
player can replay from its old cursor even after every short hop ticket expired.
Any authenticated packet, keepalives included, keeps a source's place. A game's
keepalive is a PING every two seconds, answered with READY; an established hop
that traded other datagrams both ways within those two seconds skips it, so a
match's UPLINKs and pushes are its keepalive (a released game pings throughout).
A started
source silent for 10 minutes departs through the stream (`ABSENT_MS`; a game
that hears nothing from its relay for that long releases its route, which ends
the match). Before the start, a
source that never sent Ready departs in frame 1 after 60 seconds of silence,
counted from its last packet even before the room existed, or once the room is 3
minutes old, and the others start without it; a Ready source waits. A Ready
whose agreement or signed match settings (including pacing) differ from the
room's first gets
`AgreementMismatch`. From a source still loading, it proves the others cannot
start with it: the relay cancels the startup for every source at once, as that
source's Leave could, and answers every later Ready, and the ready sources'
Pull and UPLINK, with the same error. A federated relay also refuses so once another relay's ready players
report a different agreement. A federated startup cannot drop a source alone:
those limits, a refused start, or any relay's room reaching 3 minutes unstarted
ask the coordinator to cancel it, and a relay whose coordinator has then been
silent for 10 seconds cancels on its own. Players on a relay that proved no
mismatch see an ordinary cancellation. A relay
reports only its own players' departures, and a restarted relay never does. Once
a started two-relay room has heard nothing from the other relay for 10 minutes,
that relay's players depart as lost after the last frame it sealed there, and
the room merges on without it: the match continues among the remaining players
or ends. A lobby can select adjudication for a match on three or more relays.
Its VO1F wrap tags then use the domain `VERA20k relay adjudicated federation roster v1\0`;
the admission and link key stay opaque to the game. The relay accepts such a grant
only with its durable outcome reporter enabled. Each admitted member uses VF2L,
where FRAME byte 4 keeps the pre-start cancellation bit 0, adds bit 1 for a
durably recorded federation failure, and bit 2 for a sender that has started this
match. A startup-only heartbeat after a process restart cannot renew the old
match. If any member is missing for the full ten-minute in-match grace, the
detecting relay freezes its ordered stream and signs a failure receipt. Once
durably saved, it repeats the failure bit to every other relay. If its disk
cannot take the receipt, it stops reporting a healthy started match to peers.
Players get ordinary server unavailability after five seconds of backpressure.
An unsaved verdict cannot award or void a ladder result. Ordinary terminal room
and journal retirement still runs. If the outbox cannot take an ended room's
receipt, the relay retains only those 94 bytes for retry, up to its room count
in detached receipts. At capacity the relay increments `outcome_handoff_drops`
and frees the game's resources; the lobby leaves incomplete evidence unrated. Any member that
hears it also ends the match; this needs no living coordinator. Players and
watchers receive `Inactive` with expected cursor 1. Released games show a game
server failure; updated games say the match ended without a winner. A normal
game signs its own ordinary close only after the ordered terminal frame.
The lobby settles a marked game from the signed results, as described in the
[federation outcome protocol](../../docs/lobby-protocol.md). Unmarked grants
and two-relay matches continue to use VF1L. Enable the lobby's gate after all
selected relays support the marked grants; disable it and drain marked matches
before rolling relays back.

In an unmarked match with three or more relays the survivors could disagree on that frame,
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
that cleanup, or after a startup was cancelled before any Ready reached this
relay, longer than any certificate signed by then lasts, the relay answers
Ready for that match with `Inactive`, so a player returning
later cannot start a new timeline under its ID. While the room remains, a
federated relay also tells the others the startup is cancelled. The marker
outlives the room and lives in memory; a relay restart drops it. No reader
acknowledgement gates advancement or retention.
The service checks deadlines every 6 ms even when no packets arrive. Its pass
over every room starts a millisecond after the previous one ended, so no
datagram, accepted or junk, adds a pass.

Explicit Leave is retried on the original socket until a terminal acknowledgement
echoes that exact signed request. After a Leave, or once its ticket expired, a
player's session takes only Leave, each answered again, for as long as a room can
live plus 300 seconds; then it is released (a watcher's: [Watchers](#watchers)).
Its tag still verifies, and an existing room checks its
certified source and retained principal/generation. An already expired, absent
room can acknowledge without recreating membership or storage. Other expired
operations remain rejected. Menu cleanup retains this transport until the
acknowledgement, or until 235 unanswered attempts 256 ms apart (about a minute);
the client then releases it, leaving the place to be treated as a lost
connection. Network silence never creates a Leave request.

The unit owns `/var/lib/vera20k-relay` through `StateDirectory=vera20k-relay` and
`StateDirectoryMode=0700`. The executable requires `STATE_DIRECTORY`, or an
explicit absolute `VERA_RELAY_STATE_DIR` for local development. Journal files
are mode 0600 in a mode 0700 directory named only from the 16-byte signed room ID.
They contain the plaintext batches and ordering metadata, never keys.
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

## Silence

The relay plays on without a silent player where gamemd would hold the match in
its wait dialog, so it tells the others who is silent and the game names them.
Once the match has started, an active source the relay has heard nothing from
for 10 seconds (`SILENT_NOTICE_MS`; any authenticated packet counts, as for
`ABSENT_MS`) is silent until it is heard again. While any source is silent, a
player's Status ends, after the hop feedback, in `[silent sources u32]`; while
none is, it ends at the feedback, so a match whose players all keep in touch
sends no extra byte. Clients ignore it, as any appended field. A
field added after it later must come with `[silent sources u32]` always
written, or a reader cannot tell the two apart.

A relay hears only the players it homes. In a federated match every relay tells
every other when it last heard each of its active players, every 500 ms, as link
kind 4: `[count]` then per source `[source][LE age ms u32]`. A relay names
another relay's players only once that relay has sent such a report. An older
relay rejects the kind as malformed, which touches nothing else, and never sends
it, so its players are never named. Once a relay stops reporting, its link
down, its players count as silent from the time of its last report.

## Watchers

A watcher follows a started match from its first record, some time behind its
players, and is no part of it: it has no source, counts for no start, pace,
checksum or departure, and leaves nothing in the stream, so the players' games
never learn it is there. Its code is in `../relay-protocol/src/sequenced/watch.rs`.

Its hop holds a version 3 watch ticket of a player ticket's length:
`[3][match UUID16][0][expiry LEu64][nonce16][watcher UUID16][delay seconds LEu64]`
then HMAC-SHA256 under the relay ticket key over those 66 bytes with domain
`VERA20k relay watch ticket v1\0`. Its packet key is derived from the whole
ticket as a player's is. The delay is at most 24 hours, as long as a match runs;
a finished match's records stay 300 seconds, so a watcher held back longer misses
the end.
A relay from before watchers refuses any ticket version but 2.

HELLO and CONFIRM go as a player's, but a CONFIRM carries only the cookie (no
federation roster) and spends the players' handshake budgets. The relay admits
a watcher only to a match that started and whose whole stream is in its journal,
and at most 256 watchers a match and 1,024 in all (`Full`). A CONFIRM with the
same watcher and a later expiry renews the hop: a new session that keeps the old
hop's agreement and budgets. A watcher sends only PING, which READY answers, and
CONTROL requests: Ready `[0][agreement32]`, the startup agreement, which must equal
the players' (`AgreementMismatch` otherwise) and is answered with STATUS; Pull
`[2][next varint]`, refused before an agreed Ready (`NotConfigured`), answered
with STATUS and at most 4 PACKs (1,020 records) from `next`; and Leave `[3]`,
which ends the hop and writes nothing.
Anything else, UPLINK, batches, DATA and Pause included, is refused. After a
Leave, or once a renewal replaced it, a watcher's session takes only Leave for 62
seconds, the client's minute of resends, then it is released. Each Leave, resends
included, is answered with LEAVE_ACK echoing its sequence, as a player's is, and
spends the watcher's own handshake budget. A watcher whose ticket expires, who
is silent for 30 seconds, or whose match the relay no longer holds loses its hop
without being told, and its session takes only Leave for 677 seconds: its game
keeps a route that hears nothing for up to 615 seconds, and may start its minute
of Leave resends until then.

The delay holds in every reply. Once a second the room notes its stream's end
(frame, next record, active sources), keeping 86,402 such marks (24 bytes each,
about 2 MiB for a day-long match, counted in the room's ordering memory); a watcher reads
the newest mark at least its delay old, and before the first one nothing. A
watcher's STATUS is a player's STATUS of that delayed end with the hop feedback
and no silent players, then, when there is one, `[frame varint][checksum u32]`: the newest frame at or
before that end, one in every 240 (16 checksum intervals), whose checksum every
active player reported alike. The relay keeps the last 128 such samples. A
watcher compares its own world's checksum at that frame and stops if they differ.

Watchers spend budgets of their own, never the players': each watcher 64 packets,
32 journal pulls and 256 KiB out a second, all watchers together 4,096 packets,
512 journal pulls and 8 MiB a second. A Pull that reads the journal pays before
it reads.

## Federation outcome delivery

To enable signed outcome reporting, set `VERA_RELAY_OUTCOME_URL` to the lobby’s
HTTPS origin followed by `/v1/relay/outcome`. The default is off. The relay
checks the server certificate and sends the exact 94 signed bytes as
`application/octet-stream`. It follows no redirects. The URL accepts no
credentials, query string or fragment. The lobby authenticates the body itself.

Before the relay publishes a marked federation’s terminal failure, its disk
worker commits the signed receipt to `federation-outcomes.sqlite3` in the
relay state directory. The database is mode 0600 on Unix. SQLite is bundled
into the relay, so the host needs no separately installed database library or
service. Its rollback journal and EXTRA-synchronous commits protect both the
receipt and its retry deadline across process or machine restarts. Repeated
handoffs of identical bytes for the same room are safe. Preserve this file
alongside the relay state when restarting or moving a relay. It contains
signed receipts and no signing key.

HTTPS runs on a separate worker from disk writes and game traffic. A 204
retires the receipt. Network failures, 408, 429 and 5xx responses retry with
backoff from 10 seconds up to one hour, for seven days from the receipt’s
issue time. A crash during a POST can repeat the delivery. All other HTTP
responses keep the refused receipt until expiry without retrying it. The
relay logs aggregate failure notices without receipt bytes or room IDs.

The outbox holds up to 4,096 receipts, with an 8 MiB database limit plus
SQLite’s transient rollback journal. An unavailable or full store acknowledges
only the receipts it committed. Other terminal rooms wait for storage and
cannot publish a failure on the strength of a volatile handoff. A restarted
relay drains committed receipts when reporting is enabled again.
