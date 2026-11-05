# VERA20k lobby protocol v4

HTTPS JSON only. Authentication is `Authorization: Bearer <token>`; tokens,
relay tickets and packet keys must never be logged. Names are ASCII letters,
digits, `_` or `-`, 1–24 bytes, unique without case among active sessions.
Request bodies are at most 64 KiB and 64 nested containers and arrive within 5
seconds; one player may have 4 and one network 8 bodies arriving at once, and a
further POST returns 429 `rate_limited`. Requests without a body never wait for
arriving bodies. Every JSON string and property name must be well-formed Unicode.
Escaped unpaired surrogates are rejected, not replaced, before any event or
persistent state is created.

`POST /v1/sessions` accepts exactly
`{nickname,protocol:4,password?,remember?,sign_in_token?}` and returns
`{player_id,nickname,token,expires_at}`, plus `motd` while an operator welcome is set:
text the game prints in the chat lobby in place of its own welcome, one line per `\n`.
Each entry receives a fresh random player
UUID and bearer token. `expires_at` is a Unix timestamp in seconds. An active name
returns 409 `name_unavailable`; entering its name never replaces that session.
An account's name signs in only with its password or a remembered sign-in, and a name
without an account enters as a guest ([Accounts](#accounts)).
Guest locale, buddies and relay admission counters belong to this player identity
and are removed with its session on logout or expiry. Idle lobby visits expire
after 120 seconds without a request or stream ack; admitted v4 players retain their session during
a network outage while another unfinished player of their match is online, and
otherwise through 10 minutes of silence, until explicit finish/leave or the
match/session lifetime.
A later entry gets a new identity and empty preferences. The old
`POST /v1/accounts` returns 426 `upgrade_required`.
Failed requests return `{error:{code,message}}`; messages must not echo credentials.
Clients show a 426 `message` (plain text, at most 160 characters) to the player with
the releases link, so it should name the version players need; older clients show a
fixed line.
While the operator has closed sign-ins for maintenance, `POST /v1/sessions` returns 503
`maintenance` with the operator's one-line `message` (at most 160 characters), which
clients show in place of their "could not connect" text; other 503s mean the lobby is
unavailable. Players already signed in keep their sessions.
A client in the lobby, outside a match, whose event polls fail for about a minute tells
its player the connection was lost and stops polling; its next sign-in logs the old
session out first.
The HTTP paths retain `/v1`; `protocol:4` is the required client capability.
Older/missing capabilities cannot enter new games. Existing v3 sessions may poll, renew an active lease and finish/leave/logout; their active match is preserved.
Lobby capability4 adds the signed fixed-clock ordering capability.
Relay tickets and encrypted datagrams retain wire version2; lobby capability and
relay wire versions are independent. Unsupported sessions retire through ordinary
room/match cleanup and cannot renew grants.
`GET /health` reports `protocol:4`.
`GET /v1/stats` needs no bearer and returns `{online,in_game,open_games,games_24h,games_total}`:
active sessions, those in a playing match, joinable game rooms, and games whose play
began in the last 24 hours and ever. It names no player or room, is limited to 30
requests a minute per address (an IPv4 address or IPv6 /64), and one answer serves
every reader for 10 seconds.

`POST /v1/commands` accepts `{request_id,after,operation,payload}`. A request ID is unique
within a session and retries are idempotent. Responses and
`GET /v1/events?after=<cursor>` return `{events:[{sequence,kind,payload}],cursor}`.
Sequences increase within a session. An event appears once logically even when
responses are repeated. `GET /v1/channels?kind=0|41` returns `{kind,channels}`. The
`kind=41` list leaves out game rooms whose host's `User-Agent` named another version than
the caller's, since those could never start together (two sessions that do not both name a
version see each other's rooms).
Clients poll at most once per second. Responses contain at most 256 events and
256 KiB of UTF-8 JSON.
`GET /v1/events/stream?after=<cursor>` with a WebSocket upgrade and the bearer (protocol 4
only; 400 without an upgrade) pushes the same replies as text frames, from `after` on,
as soon as a request, ack or expiry adds events. Frames continue from the stream's last
frame, so one can repeat events a command reply already delivered or arrive behind it;
the game keeps the newer cursor and drops events it has. Its only message is
`{"ack":<cursor>}`, every 15 seconds (at most 60 a minute): it counts as a request for
idle expiry and is answered with a frame, empty when nothing is new. A newer stream of
the session closes the older with 4000 `replaced`; a stream whose request would now fail
closes with 4000 plus that status (4401, 4409, ...) and the error code as its reason.
Games from 0.2.4 stream on native builds, poll whenever no stream is up and reconnect
after 5 seconds, doubling to 5 minutes; older games and the browser build poll.
A game an external launcher started never streams and polls once a minute after
`timing_confirmed`, so its service answers `renew_relay` with `relay_renewed` in the reply.
Critical history is bounded to 1,024 events/2 MiB per session;
expired cursors fail explicitly rather than silently dropping state.
Chat, locale and buddy observations have a separate best-effort 64-event/128 KiB
retention budget and cannot evict critical events. Options and per-player
choice/readiness/map observations coalesce to the newest state within the same
room. Sequence gaps are valid; an empty fully drained reply advances the cursor
past discarded soft events. Only actual critical-history eviction expires a cursor.
`logout` alone can release its authenticated idle session with a lost or stale
cursor and returns `{events:[],cursor:0}` without draining history. Its command
schema, bearer checks, rate limit and active-match refusal still apply; other
operations retain the cursor barrier. Clients attempt logout with their existing
bearer before discarding a session whose history expired.

Channel: `{id,kind,max_users,users,tournament,flags,game_flags,ping,name,ex_info}`.
`kind` is 0 for chat and 41 for a game. The service sets `flags` from the room, never
from the creator: 256 when a game has a password, which clients show as a lock and
ask for before joining, otherwise 0. `max_users` (1–30) from `create` and the host's
`advertise` is the room's join limit, never below its current members; a join past
it fails with `room_full`. Games from 0.2.6 advertise the most people their room
takes, players and spectators: the host's Max players, and with spectators off no
more than its seats. Their host gives a newcomer a free seat, then the last AI's
seat, then a spectator's place, and their lobby lists each room as `users` over
`max_users`. A host `kick` with `keep_out:true` keeps that player out of
the room for five minutes, by account and by name without case: their `join` fails
with 403 `room_kept_out`, which clients show with the join ban's text while staying in the lobby. The other fields are presentation metadata, not authority. `name` identifies a channel uniquely for compatibility with the
existing shell. Member: `{player_id,name,flags,squad,clan_name,clan_tag}`. Flags bit 0
is the channel owner, bit 1 voice; the client adds its own local bit 32768. Servers
set identity and flags from the authenticated membership, never from user payloads.
The only public chat channel is `name:"Lob_41_2"` (`id:"public"`), which games from
0.2.4 show as VERA20K Online and older games by its retail name, German Bunker.
The service updates that stored room's name without replacing its membership or
accounts. Other chat channels, apart from Quick Match's below, are neither listed nor
joinable; game rooms remain available through `kind:41`.

Commands and their payloads:

| operation | payload |
| --- | --- |
| `join` | `{channel:<channel name>,key}` |
| `create` | `{channel:<Channel>,key,ranked?}` |
| `leave` | `{}` |
| `logout` | `{}` |
| `options` | `{options:<HostOptions>}` |
| `choices` | `{row,slot:<SkirmishLocalSlot>,observer}` |
| `ready` | `{ready,revision,version,rules_digest,map_digest,effective_map_digest,staging_digest}` |
| `map_status` | `{status,revision,version,rules_digest,map_digest,effective_map_digest,staging_digest}` |
| `advertise` | `{channel:<Channel>}` |
| `start` | `{users:[name],revision,version,rules_digest,effective_map_digest,staging_digest}` |
| `finish` | `{match_id,back_to_room?,result?,winners?,network_quality?}` |
| `relay_ready` | `{match_id}` |
| `latency_ready` | `{match_id,pings:[{source_index,round_trip_ms}]}` |
| `renew_relay` | `{match_id}` |
| `map_upload` | `{digest,byte_length,offset,data,resource?}` |
| `chat` | `{text,to:[name],action}` |
| `ping` | `{}` |
| `locales` | `{names:[name]}` |
| `set_locale` | `{locale}` |
| `buddies` | `{}` |
| `add_buddy` / `remove_buddy` / `locate` | `{name}` |
| `kick` | `{name,keep_out?}` |
| `ladder` | `{names:[name]}` |

HostOptions is `{launch,map_metadata,vacancies,observer_row,cooperative_difficulty,seed}`.
Generated maps add `random_map:{start_staging,preview:{source_sha256,byte_length}}`.
Cooperative rooms add `cooperative_campaign`, the host's campaign record as the game's
`C0` text (at most 512 bytes, starting `C0,`); a guest applies it before the options
that carry it. Other modes omit the field.
`start_staging` is the canonical 37-number native `W` record, bounded to512bytes;
`staging_digest` is its lowercase SHA-256, or the empty string for authored maps.
`launch` is the existing serde representation of `SkirmishLaunchSession` (including
its human rows, AI rows and launch options). `map_metadata` is
`{display_name,player_capacity,official,byte_length,digest,source_sha256,effective_digest}`. The legacy
INI `digest` remains distinct from lowercase SHA-256 over the physical map bytes;
online `map_digest` agreements compare `source_sha256`. `effective_digest` and
`effective_map_digest` compare the parsed gameplay map after its applicable include
expansion, so identical root bytes cannot conceal different local dependencies. `vacancies` is an array
of `open`, `closed`, `observer` or `waiting`. A receiver resolves its own
`launch.local_player_index` from the authenticated player; it never uses the host's.
`slot` uses `SkirmishLocalSlot` serde. Options are authored only by the room owner;
the service increments `revision` and clears guest readiness on a material change.
Starting requires at least one non-observer human and at least two playing
participants, counting the actual `launch.opponents` AI rows. A host can select
an AI opponent and start alone; `ai_player_count` without an AI row is insufficient.
Games with observers cannot include AI players. `start.users` contains exactly
the canonical human names, including observers, and never includes AI rows.

Event kinds and payloads:

| kind | payload |
| --- | --- |
| `joined` / `created` | `{channel:<Channel>,members:[Member]}` |
| `channel_left` | `{}` |
| `room_reopened` | `{channel:<Channel>,members:[Member]}` |
| `member_joined` | `{member:<Member>}` |
| `member_left` | `{name}` |
| `options` | `{revision,options:<HostOptions>}` |
| `choices` | `{from,row,slot,observer}` |
| `ready` | `{from,ready,revision}` |
| `map_status` | `{from,status,revision}` |
| `chat` | `{from:<Member>,text,private,action,sent_at,ansi?,quick_match?}` |
| `game_started` | `{match_id,revision,version,rules_digest,seed,users:[{player_id,name,flags,source_index}],relay:<RelayGrant>}` |
| `start_confirmed` | `{match_id}` |
| `timing_confirmed` | `{match_id,pings:[integer],relay:<RelayGrant>,ranked?}` |
| `relay_renewed` | `{match_id,relay:<RelayGrant>}` |
| `map_available` | `{digest,byte_length,resource?}` |
| `start_rejected` | `{message}` |
| `locales` | `{locales:[{name,locale}]}` |
| `buddies` | `{names:[name]}` |
| `located` | `{channel:<Channel or null>}` |
| `kicked` | `{channel:<Channel>,user:<Member>,by:<Member>}` |
| `ladder` | `{season,records:[{name,points,rank,wins,losses,disconnects}],high_scores:[{name,points,rank}]}` |
| `balance` | `{teams:[{name,team}]}` |
| `error` | `{code,message}` |

A `chat` event's `sent_at` is the Unix time in seconds the service sent it. Games from
0.2.4 show it, in the player's local time, before each line of the chat lobby.

RelayGrant is `{address,ticket,packet_key,payload_key,source_index,ordering_certificate,federation?}`. Address is `IPv4:port`
of the user's relay in the match. The service chooses it at start and every later grant for that match names it;
ticket is the signed relay protocol ticket encoded base64url without padding and
packet_key is 32 bytes encoded the same way. Credentials are unique to each user
and each new match. `payload_key` is a random 32-byte match key encoded the same way.
It remains unchanged through bootstrap, timing confirmation and renewal. Current
clients accept it and do not use it: gameplay batches and DATA travel in plain
text under the per-hop MAC, and the relay can read them
(`services/relay/bandwidth-compaction/spec.md` §3). The field stays in the grant
so the lobby needs no change; removing it is a follow-up. `source_index` is the immutable canonical user index, 0–29.
`ordering_certificate` is 66 base64url-encoded bytes: `VO1C`, room UUID16,
source mask LEu32, fixed FPS LEu16, expiry LEu64, then HMAC-SHA256 over the
first34 bytes with domain `VERA20k relay ordering certificate v1\0`. A lobby
whose every member build reads relay rate markers may instead issue a 67-byte
`VO2C` certificate: the same fields, then a flags byte (bit 0: the host turned
relay pacing off; other bits zero), then HMAC-SHA256 over the first35 bytes with
domain `VERA20k relay ordering certificate v2\0`. Every grant of one match carries
the same version and flags; clients reject a renewal whose pacing differs. The signed
mask includes all human sources (including observers); AI has no source. FPS
follows launch speed `[60,45,30,20,15,12,10]`. The certificate lasts24 hours;
every UDP control still needs a live authenticated short-lived ticket. The relay
binds the first matching startup agreement to this roster/clock and starts once
every remaining signed source is ready. A source that never becomes ready leaves
in the first frame after 60 seconds of silence, or 3 minutes after the first
Ready; a match federated over several relays is cancelled instead. Renewals
preserve the timeline and batch IDs.
`federation`, present only in a federated match, is the same base64url roster
in every grant: `VO1F`, room UUID16, relay count (2–6), then per relay IPv4,
LE port and LE source mask, then per relay nonce16, wrapped32 and tag32. The
masks partition the signed sources. `wrapped` is the match link key XORed with
HMAC-SHA256(relay ticket key, `VERA20k relay federation wrap v1\0` ‖ room ‖
nonce); `tag` is HMAC-SHA256(relay ticket key, `VERA20k relay federation roster
v1\0` ‖ the table through the masks ‖ index byte ‖ nonce ‖ wrapped). The client
appends it unchanged to every CONFIRM; the first relay coordinates the start.
The lobby federates a match only when every human's sign-in request carried the
header `Vera-Relay-Federation: 1`, which clients send once they forward the roster.

The relay room uses the fresh `match_id`, including for rematches. The server sends
`game_started` only after all current participants agree to revision, version,
rules digest and map metadata and are ready. The client still performs authenticated
UDP admission and validates its local map/rules/version before it admits the start.
The client stages a Start click while it publishes the final assignments from
its existing assignment owner. It does not send `start` with an earlier revision.
A changed publication clears every prior ready/map acknowledgement, and each
guest's client accepts the final displayed settings by itself once it has the
verified map; players never ready up by hand. Only after the matching options
revision and every fresh acknowledgement arrive does the retained Start request
reach the server. A local optimistic choice cannot be accepted before its host
options echo. Later edits cancel the staged request; rejected or timed-out starts
release the normal setup screen for retry without rerunning assignment while a
request is pending. The service continues enforcing the same full agreement.

An HTTP reply alone never establishes a UDP peer connection. Each client sends
`relay_ready` only after the authenticated UDP admission completes. The server emits
`start_confirmed` after every admitted member has acknowledged that same match.
Clients then measure actual peer round trips through their reliable UDP owner.
Every `latency_ready` report contains each other canonical source exactly once;
the service sends the same per-source maxima in `timing_confirmed` to everyone.
For one human playing against AI, there are no remote peers: the latency report
is `pings:[]` and the confirmed per-source timings are `pings:[0]`. AI players do
not receive source indices or relay credentials. Such a match runs its commands
locally: its grant still carries an ordering certificate, which released clients
require, but the relay refuses a one-source certificate.
Only then does game loading start. Each barrier has a 30-second timeout. Within it,
clients resend `relay_ready` and `latency_ready` under the same request ID after a
429, a 5xx or no answer, and a UDP I/O error alone does not end admission.
Initial tickets last 120 seconds; timing confirmation supplies a fresh 300-second
ticket which the client must authenticate before loading. During play, clients
renew once half of that lifetime has passed, so a lobby outage of up to 150 seconds
cannot expire a running match's ticket. Renewal is timed from grant receipt on an
uptime clock that keeps counting through sleep, rather than the ticket's expiry
against the local wall clock, so a changed grant lifetime needs a client update. The server
selects the source from authenticated frozen membership, issues a fresh 300-second
grant and extends the private match lease. Clients authenticate the replacement
on the existing socket before promoting it; game/reliable queues remain intact.

V2 relay tickets are 98 bytes: version 2 at 0; match UUID 1..17; source 17;
little-endian expiry 18..26; random nonce 26..42; stable opaque player UUID 42..58;
little-endian nonzero admission generation 58..66; HMAC-SHA256 66..98. The ticket
domain is `VERA20k relay ticket v2\0`; packet-key derivation uses
`VERA20k relay packet key v2\0`. The server increments a durable generation for
each player identity transactionally only on a fresh match, never on renewal. The relay
permits only that principal's newest generation/match, evicts its older membership,
and retains the generation floor for the maximum 300-second ticket lifetime.
Cancelling a start cannot accumulate long-lived rooms using two player identities.

Every live VERA event packet, including FRAMESYNC probes, starts with bytes
`56 43 4d 01` (`VCM`, version 1), then an immutable little-endian `u64` command
frontier for the source's sending frame, followed by the existing 14-byte event
header and body. The payload remains at most 558 bytes. Native `timing_word` is
only the low 16-bit projection; readiness compares the full frontier with
reliably decoded commands. Live packets have no raw-header fallback.

Reliable broadcast must accept every peer copy before the sender removes or
mirrors commands or commits compression state. A rejected send retains the same
frame target and retries while the existing receive/ACK service proceeds. Local
outstanding input is bounded to 32,768 source commands, reserving 128 slots for
control records. Receiver DoList allows 65,536 records per source to include one
FRAMEINFO per packet, with a 30-source total bound and allocation on demand.

The VCM1 event body also carries these fixed VERA records. Each retains the
ordinary seven-byte event header and authenticated source House. All numeric
payload fields are little-endian; unknown action/category values are rejected.

| Opcode | Payload |
| --- | --- |
| `0x30` | 21 bytes: action u8 (begin 0, complete 1, cancel 2), source and destination SuperWeaponTypes indices i32, one-use sequence u64, cell x/y i16 |
| `0x31` | 5 bytes: the existing TargetClass object token for bunker eject, stable object identity i32 and kind `0x34` |
| `0x32` | 1 byte: production category Building 0, Defense 1, Infantry 2, Vehicle 3, Aircraft 4, Ship 5 |

Paid selections require distinct nonnegative type indices and a nonzero
sequence. Target cells are within 0–511; cancel has canonical zero coordinates.
The existing payment, bunker and producer owners validate and execute these
records. They add no independent ticket, entity or factory state. Native SELL
`0x16`, PRIMARY `0x11` and SPECIAL_PLACE `0x12` retain their existing layouts.

Maps are limited to 8 MiB. Only the room owner can upload the currently selected
physical SHA-256, in contiguous 32,768-byte chunks (except the final chunk).
`data` is unpadded base64url; identical chunk retries are accepted. A completed
upload is hash-checked before `map_available`. Authenticated room members use
`GET /v1/maps/<source_sha256>?offset=<multiple of 32768>` to receive
`{digest,byte_length,offset,data,complete}`. An unfinished upload returns
`map_pending` (409); a changed selection returns `map_changed` (409). Changing
only roster/settings revisions preserves the blob. A map change or room cleanup
removes it. Uploads/downloads have a separate 600-request/minute/player budget;
ordinary commands and events share 600/minute/player. Action limits additionally
bound starts to 12 per 10 minutes, relay renewals to 12/minute, chat to 30/minute and
24 KiB/minute, and room chat to 120/minute. Control payloads are bounded to
4 MiB/minute/player and room fanout to 128 MiB/minute. Room chat, control and fanout limits bind guests, never the room's
host. The public channel takes 120 joins/minute and each network 30 of them and 30 of its
chats/minute; a game room takes 60 joins/minute. Re-joining the current room is free.
Map upload/download bytes each have a 32 MiB/minute player budget, and so do the
earlier chunks the service re-reads to check a finished upload.
Canonical options are limited to 24 KiB, choice objects use the exact canonical
slot schema, and unknown request/option fields are rejected. The actor also caps
pending requests/parsed bodies at 128/4 MiB and individual events at 64 KiB. Active
sessions, guest profiles and public-room membership are capped at 256. Per-address
limits key on the client's network: an IPv4 address, or an IPv6 /64, with the
enclosing /48 as the range. New guest entries are limited to 40/hour per address,
64/hour per range and 256/hour globally, and one range holds at most 64 active
sessions (503 `network_full`), with the existing sign-in and ingress limits also
applied. One name can be taken 30 times in 5 minutes; refused entries do not count. These are service admission limits, not gameplay timers.
Clients verify and install into a
local cache, preserve existing installed maps, and still require user readiness.
Generated maps transfer the `.SED` bytes as the map and the separate native PCX
preview with `resource:"preview"` (512KiB maximum). Preview downloads use the
selected seed's URL plus `&resource=preview`; response `digest` names the preview.
Both resources use the same ownership, ordered chunk, hash and cleanup rules.
Generated capacity equals the staged start count (2–8); observers consume only
transport slots. Start admission also compares the canonical staging digest, so clients cannot
agree on seed bytes while using different generated start cells.

The service may keep a map library: custom maps its operator adds, each kept once under
its SHA-256. Players never add maps; room uploads stay out of it. When the host selects
a map the library holds (same `source_sha256` and length, not `map_metadata.official`,
not a generated map), the service sends `map_available` with the settings, before any
upload, and members download it through the same `GET /v1/maps/…` chunks. When the
service does not copy it (it could not read a correct copy within about 2 seconds, or is
busy copying others), it sends no `map_available` and the host uploads as usual. A host that hears `map_available` for its current map stops uploading; an upload
sent anyway is accepted. Other custom maps go from host to members as before. A member
whose game lacks the room's map is asked first: Download fetches it and saves it in
VERA's own `Maps` folder (beside its settings, never the game folder); Leave leaves the
room.

`GET /v1/library/catalog` lists every library map:
`{revision,maps:[{sha256,name,file_name,players,byte_length,header}]}`, where `name` is
the map's `[Basic] Name=`, `file_name` the `.yrm` name to save it under, and `header`
the base64 of the map's own `[Basic]`, `[Preview]`, `[PreviewPack]`, `[Waypoints]`,
`[Header]` and `[RandomMap]` lines, byte for byte. No two library maps share a
`file_name` without case: one whose name would, or whose name has nothing left to save
under, ends in `-<first 8 hex of its SHA-256>.yrm`. The catalog carries
`ETag: "lib-<revision>-<16 hex>"`, the hex from its content (catalogs written before:
`"lib-<revision>"`), and answers 304 to a matching `If-None-Match`, weak or not, or `*`.
`GET /v1/library/maps/<sha256>` downloads one map, and
`GET /v1/library/maps?search=<text>&players=<n>&page=<n>` searches them (newest first,
50 a page). The catalog and downloads also answer HEAD, and 503 `service_unavailable`
when the library's storage fails. All three are public, and the search is limited per
network; the service keeps no player, session or address with a map. The game reads the catalog
when the player goes online and keeps it for the run once it arrives; a failed read is
tried once more in that visit, and again on the next. In an online room (not one that
takes only official maps), Choose Map lists under its Online Maps row each map the
player does not have, starred maps first: a file of its name in the game folder or
VERA's `Maps` folder that holds its bytes means the player has it. Use Map on one asks
to download it, checks its length and SHA-256, and saves it in VERA's `Maps` folder,
where it is an ordinary map in every game type's list. A map whose name another map
already holds is saved as `<stem>-<first 8 hex of its SHA-256>.yrm`; no file is ever
replaced.

The match admission is retained independently of channel membership until
finish/timeout. The original-style shell leaves its game room at the start and
postmatch rejoins chat, where it may create or join another game. VERA's own room
stays joined through the match, and its `finish` sets `back_to_room: true`. When
the last player finishes, the players still in the room who asked for it get it
back: `room_reopened` (owner first) and then `options` with the host's settings
and every readiness cleared. Players who finished without asking are dropped from
it silently. If the owner is not among them, or a newer room took its name, the
room closes for them with `error` `host_left` and `channel_left`; an expired match
closes it with `channel_left`. Each rematch is a fresh match with fresh relay
credentials. A channel owner leaving before
start closes the room and emits a failure/leave event to the guests. Membership,
authorization, readiness, sequence history and idle expiration are server-owned.
Idle lobby visits expire after 120 seconds. An unfinished v4 playing membership
survives that idle interval while another unfinished player of the match is online
(a request or stream ack in the last 120 seconds), and otherwise through 10 minutes of silence, until
explicit finish/leave or the match/session lifetime; lost internet does not remove
its gameplay source. Names held by crashed clients therefore free once nobody else
in their match is online. Sooner, a sign-in from the session's own network takes its
name back once the session has made no request for 30 seconds and has no open
stream: a running game reaches the lobby at least every 15 seconds, so that session
belongs to a game that closed or crashed. Its match is finished for it and its bearer
answers 401. A lobby outage or overload is not player silence: these
intervals count from the later of the player's last request and the lobby's first
request after 90 seconds in which it neither served a request nor ran its
once-a-minute alarm, or its latest 429 `busy` refusal.

A game whose online match reached play adds `network_quality` to that match's
`finish`: its own record of how the match ran, which it also logs as
`Network quality:`. It is
`{played_seconds,catch_up:{episodes,seconds,max_backlog_frames,skipped_draws},`
`network:{gaps,repair_pulls,command_round_trip_ms,max_jitter_reserve_ms,underruns,`
`underrun_wait_ms,max_underrun_wait_ms,link},`
`performance:{tick_ms_p50,tick_ms_p99,fps_p50,fps_p5,unfocused_seconds},`
`pacing:{slowdowns,slowed_by,slowed_seconds},outcome:{desync_frame,reconnects}}`.
Times are seconds, or milliseconds where the name says so; `command_round_trip_ms`
is `{mean,max,samples}` or null, `tick_ms_p50`, `tick_ms_p99`, `fps_p50`, `fps_p5` and
`desync_frame` may be null, and `slowed_by` lists the House each relay slowdown named.
`underruns` counts the waits for a frame due but not yet received; `underrun_wait_ms`
sums how late each waited-for frame arrived after its deadline and
`max_underrun_wait_ms` is the longest, leaving out waits that end in a reconnect or
catch-up. `link` is the relay hop as the game's relay client measured it,
`{rtt_ms_p50,max_up_loss_permille,up_lossy_seconds,up_congested_seconds,rto_timeouts,`
`gaps_repaired_by_push,max_arrival_jitter_ms}` (`rtt_ms_p50` may be null), or null
without one (`services/relay/adaptive-transport/spec.md` §10.1). Older games send none
of these three, so a service reads the report with or without them.
A start cancelled before play and a match an external launcher formed send none. The
service may keep reports for its operator without the player's name or address; one it
cannot read is dropped and the match still finishes. A service that refuses the field
gets the same finish again without it.

`POST /v1/translate` accepts exactly `{text,target}` from a v4 bearer and returns
`{text}`: `text` translated into `target`, a language tag such as `en` or `zh-Hans`.
`text` is non-blank and at most 2048 UTF-8 bytes, the chat limit. Clients translate
received chat only after the player opts in, and always keep the original visible.
A deployment without a translator, a failed model call or one taking over 10 seconds,
a new message while 8 others wait on the model, the service's daily budget
of 10,000 model calls or one network's daily 2,500 of them returns 503
`translation_unavailable`; an unsupported target
returns 400 `invalid_request`. Each player may ask 60 times a minute. Successful
replies are cached for 24 hours by `target` and `text`, and readers asking for the
same message while it is translated share one model call. Translation never waits
for, or delays, room commands.

`GET /v1/match/resume?match_id=<UUID>` requires the same v4 bearer and its own
unfinished playing match. It returns `{match_id,cursor,relay}` with a fresh
short-lived ticket for the existing source/generation and the current shell cursor.
This recovers an expired lobby event cursor without skipping gameplay: missed
commands still come from the relay's complete journal. It shares the existing
renewal action budget; it cannot create a membership or change match identity.

`POST /v1/admin/notice` is for operators, not players: its bearer is the
deployment's `ADMIN_KEY`. Without a key of at least 32 characters it returns 404
`not_found`; a missing or different bearer returns 401 `unauthorized`. It shares
the sign-in ingress limit of 120 requests a minute per address. `{text,minutes}`
sets the one current notice, replacing any earlier one: non-blank text of at most
2048 UTF-8 bytes, kept for 1–10080 minutes. `{minutes:0}` alone clears it. Both
return `{notice,delivered}`: the notice `{id,text,sent_at,expires_at}` or `null`, and how
many sessions received it at once. A notice is an ordinary public `chat` event from
`{player_id:"00000000-0000-0000-0000-000000000000",name:"VERA20k",flags:1,squad:0,clan_name:"",clan_tag:""}`
whose `sent_at` is the time it was set.
Sessions in the public channel or a game room that has not started receive it when it is
set; others receive it on their next public-channel join until it expires. Each
session receives a notice once. Sign-in refuses `VERA20k` without case with 409
`name_unavailable`.

`POST /v1/admin/welcome` takes the same bearer, answers and ingress limit. `{text}`
sets the welcome that sign-in responses carry as `motd`: non-blank text of at most
2048 UTF-8 bytes. `{text:null}` clears it. Both return `{welcome}`, the text or `null`.

Released clients sign in with `User-Agent: VERA20k/<MAJOR.MINOR.PATCH> multiplayer-client`.
A client that names a version older than 0.2.7 is refused at sign-in with 426
`upgrade_required`, since clients before it cannot play through the relays'
adaptive transport (UPLINK, `services/relay/README.md`); one that names no
version is not.
A session whose version is older than `version` in
`https://downloads.vera20k.org/latest.json`, which the lobby reads at most every
10 minutes, receives one such chat line per newer release, naming both versions,
since a match starts only between equal versions (`ready`, `start`). It arrives
as a notice does: at once in the public channel or a game room that has not started,
otherwise on the next public-channel join.
A session that still joins a game room from another version, through a buddy's game or
its name, makes every member hear a `chat` line from the notice sender naming both
versions.

Each `join` of the public channel gives the joining session one `chat` line from the
notice sender after its `joined`. The line says how many players are online: in the
public channel, in game rooms, playing, for sessions that play Quick Match searching on
their version, and elsewhere. It also counts the game rooms the list leaves out for another
version and, when the list holds none for the session, says how to start a game.

## Quick Match

1v1, ranked while a season runs ([Ranked games](#ranked-games)); Quick Co-op (`Lob_39_`) has no lobby. Games that play it send
`Vera-Quick-Match: 1` on `POST /v1/sessions`, beside `Vera-Relay-Federation: 1`. Only
those sessions see `GET /v1/channels?kind=0` list, after `Lob_41_2`, the chat channel
`{id:"quick-match",kind:0,name:"Lob_40_0",max_users:<lobby cap>,users,tournament:0,flags:0,game_flags:0,ping:-1,ex_info:""}`
or may `join` it (any key); to others it is an unknown channel. Its members are its
players, then always last the matchbot
`{player_id:"00000000-0000-0000-0000-000000000001",name:"matchbot",flags:1,squad:0,clan_name:"",clan_tag:""}`.
Sign-in refuses `matchbot` without case with 409 `name_unavailable`. There a player's only
chat is private to `matchbot`: `{text:"Match COU=<int>, COL=<int>, MBR=<0|1>[, LOC=<int>],
RAT=<int>, RES=<int>, ADR=<int>, NAT=<int>, PRT=<int>",to:["matchbot"],action:false}`, the
retail request with the transport fields 0; COU and COL are the side and colour the player
saved for multiplayer. The lobby reads COU (country) and COL (colour, each -2 for Random); a text not starting `Match ` gets a private `nack`. The matchbot's
replies are `chat` events with `ansi:true`: a private `Working` at once, also to a repeated
request, which keeps the player's place; a public `Pool <n>` whenever the number waiting
changes; and on a pairing a private `Start <seed>:<host>,0,0,0,0,0,0,0,<guest>,0,0,0,0,0,0,0`
to both players with `quick_match:{seed,host:{name,country,color},guest:{name,country,color}}`,
a fresh random u32 seed and each player's COU and COL as sent. Pairing is first come, first
served between two players whose `User-Agent` names the same version (two unknown versions
may pair); the earlier waiter hosts. Leaving `Lob_40_0` (leave, logout, expiry or another
join) leaves the queue.
A request that puts a player in the queue without pairing them makes them first in line for
their version. They then get private `Info <text>` lines after `Working`, which the game
prints as they come. These say:
- that the match starts when another player presses Quick Match;
- how many others wait on another version;
- how many public-channel players heard of the search;
- that their game sent the side and colour last picked for a multiplayer game.

Those players are the public channel's sessions that play Quick Match on the same version.
Each gets a `chat` line from the notice sender, `<name> is looking for a Quick Match. Press
Quick Match to play them.`, at most once every 5 minutes from each network.

The host's game then leaves and `create`s the two-player game `<host>'s_game` with
`tournament:1` and the `ex_info` of a Create; the guest's stays in `Lob_40_0`, lists `kind=41`
up to 30 times, 100 ms apart, for that name (without case) and joins it, which leaves
`Lob_40_0`. For 2 minutes a room the paired host creates is that pairing's: only the paired
guest sees it listed or may join it (anyone else `room_full`), until the guest has joined.
From there the v4 room flow runs. When the guest joins, the host's game publishes options
built from `quick_match`: host row 0, guest row 1, automatic starts, the first mode (Battle),
`seed`, and the official two-player map of index `seed % count` among those of its catalog
the mode admits. A guest colour the host holds becomes Random; each Random is then drawn from
`seed`, a country from the game's list and a colour the other player does not hold. The host
ignores the guest's `choices`; the guest's game accepts the options by itself, and the
host's starts once it has.

## Accounts

A player enters as a guest by name alone, or signs in to an account with its name and
password. Every request below is a `POST` with a JSON body; a refusal's
`{error:{code,message}}` message is plain text the game shows as it is.

- `/v1/accounts/register` `{name,email,password}` returns 202 and mails a 6-digit code.
  It refuses with 400 `invalid_name`, `invalid_email` or `invalid_password`, 409
  `name_unavailable` or `email_in_use`, or 429. Names follow the session rule;
  passwords are 8–64 printable ASCII characters; emails are at most 254 characters of
  the `x@y.z` shape, one account each.
- `/v1/accounts/verify` `{name,code}` returns 200 and creates the account: 400
  `invalid_code` (5 tries), 410 `code_expired` (30 minutes, or the tries used up).
- `/v1/accounts/forgot` `{name}` returns 202 whether or not the name has an account,
  and mails a code to the account's email when it has one. It refuses a malformed
  name (400) and too many requests (429).
- `/v1/accounts/reset` `{name,code,password}` returns 200, sets the password, signs out
  every remembered computer and mails the owner. It refuses as verify does, and with
  `invalid_password`.
- `/v1/accounts/forget` `{name,sign_in_token}` returns 200 and drops that remembered
  sign-in.
- `/v1/accounts/delete` `{name,password}` returns 200, deletes the account with its
  email, remembered sign-ins and ranked games, signs out its player and mails the owner;
  anyone may then take the name. A wrong password counts as at `/v1/sessions` (403
  `wrong_password`, the lock), and a name without an account returns 404 `no_account`.

At `/v1/sessions` an account's name needs its `password` or a `sign_in_token`, never
both: with neither it returns 403 `name_claimed`, with a wrong one 403 `wrong_password`,
and 10 wrong passwords in an hour return 429 `account_locked` for 15 minutes and mail
the owner. 30 wrong passwords from one network in an hour return 429 `rate_limited`.
An account's sign-in has `account:true` in the response, and `remember:true` with the
right password adds a `sign_in_token` (32 bytes, base64url without padding). A password
or `sign_in_token` for a name without an account returns 404 `no_account`; otherwise
that name enters as a guest. A lobby without accounts set up refuses a password with
503 `accounts_unavailable`.

VERA20k's online login page has its buttons beside an info pane, and each button opens
a popup. Login asks for the name and password, with Remember password, and Play as Guest
for the name alone, with no password or remembered sign-in; Enter on the page opens Login.
Create Account asks for the name, email and password, then the mailed code, and signs in.
Forgot Password asks for the name and mails its code, then asks for it and a new
password, and signs in. Delete Account asks for the name and its password, then says the
account is deleted and forgets a remembered sign-in of it. The pane shows the players
online, or while nobody is, the last 24 hours' games (`GET /v1/stats`), vera20k.org's five
newest news posts in the game's language (`GET https://vera20k.org/news.json?lang=en` or
`zh-CN`: `{posts:[{date,title,summary}]}`) and the online rules; it reads both each time
the page opens. A remembered
sign-in lives in `Account.json` beside `VERA.INI` as `{name, sign_in_token}` per lobby,
readable only by its owner; Login's popup then opens on the name with a masked password.
Unticking Remember password forgets it here and at the lobby, and so does a password
sign-in of that name or a remembered sign-in of another account. A `wrong_password` or
`no_account` answer to it forgets it too. Signing in under another name leaves it.

## Ranked games

While the operator runs a season, the lobby ranks a 1v1 Quick Match pairing played by its
two players alone, and a game room's match of at least three players on sides of the same
size: teams, in a mode that lets players ally, or each player alone, in a mode that does
not. Three or more teams also need in-game allying off. The game must keep the sides
apart: players on two sides may not pick the same start, which makes them share a House,
nor, in a mode without alliances, the same team. A Cooperative mode (`mode.class`) never
ranks. Every side's chance against every other is within 80/20. Ranked matches have no AI,
and every player is signed in to an account.

A host who unticks Ranked game at Create Game sends `ranked:false` on `create`, and that
room's games never rank; its odds line says why. A ticked box sends nothing, as a lobby
from before the choice refuses the field.

A guest's match plays unranked. `ranked:true` in `timing_confirmed` marks a ranked match.

A `finish` after play adds the game's own `result`: `won`, `lost`, `quit` (the player
left) or `disconnected` (the game lost its connection). A player who leaves after the game
decided their House reports that result: `lost` once it was defeated, `won` once it won. When the game saw the match
decided, `winners` names the players still in it whose Houses won. A lobby from before
team ladders refuses `winners`; the game then finishes again without it.

`ladder` with up to 64 names answers with the running season's records of those that
played a rated game in it (`season` is `null` and `records` empty when no season runs),
and the `high_scores` of those that ever played one. Every ranked game, Quick Match, team
game or free-for-all, moves one rating per player, shown as OS: μ − σ, as Beyond All Reason
shows skill, about 16.7 for a new player. A record's `points` is its player's OS rounded,
and `rank` their place among the season's players by OS. A high score's `points` are its
player's badge points, the ranked games of every season times the OS, over ten, rounded
and at most 2000, and `rank` their place by them. One lookup fills both of the game's
tables: its ladder table from `records`, and its high-score table, whose points pick each
player's badge (Private below 32 up to Five-Star General from 1501; the game's Commander in
Chief badge, from 2001, never shows), from `high_scores`. Badges therefore grow with play
and skill and outlast seasons. A lobby from before them sends no
`high_scores`, and the game then shows no badges. After a rated game each of its players
hears their new OS from `VERA20k`, such as `Ranked game won: your OS is now 19.57 (+2.90).`

`GET /v1/players/<name>` answers the player profile vera20k.org shows:
`{name,os,season,rank,wins,losses,disconnects,games}`, `rank` being `null` when the player
is unranked in the running season, and `games` their 20 newest rated games, each
`{ended,sides,size,won,os,change}`. A name without an account answers 404.
`GET /v1/leaderboard` answers the leaderboard vera20k.org shows, renewed once a minute:
`{season,updated_at,total,players}`, `season` being `null` when no season runs, `total` the
players ranked in it and `players` the best 1,000 of them by OS, then wins, each
`{name,os,rank,wins,losses,badge_points}`, its badge points picking the badge shown by the
name.

While a season runs, the host's options for a game room whose players are all on teams,
or in a mode without alliances, give an odds line, which the room hears from the `VERA20k`
notice sender each time it changes: each side's chance to win, such as
`Team A 54%, Team B 46%. Ranked.` (a free-for-all names its players), or `Unranked:` and
why. Quick Match and Cooperative rooms get none. In a game room the host's chat line
`/balance` is a command, not chat.
The room hears the two teams with the most even odds on the team ratings, keeping as
many players on their teams as it can, and the host's game gets a `balance` event with
every playing human's `team` (0 to 3 for A to D). The host's game moves each named player
to that team, as their own team choice would, and publishes the options. A game that
ignores the event leaves the teams to the host. The host alone hears why nothing moves:
a Quick Match room, a mode without teams, AI players, fewer than two players, or teams
already as even as they can be. Anyone else typing `/balance` hears that only the host can.
