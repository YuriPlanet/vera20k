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
`{player_id,nickname,token,expires_at,features?}`, plus `motd` while an operator welcome is set:
text the game prints in the chat lobby, one line per `\n`. Without it the game prints no welcome.
`features` lists what the lobby offers beyond protocol 4, and a game offers only what it
lists: `visit_lobby` ([Visiting the lobby](#visiting-the-lobby)) and `campaign_rooms`
(HostOptions `campaign` below), which VERA's lobby does not offer.
A game says what it is and what it can do in [sign-in headers](#sign-in-headers).
Each entry receives a fresh random player
UUID and bearer token. `expires_at` is a Unix timestamp in seconds by the lobby's clock,
which alone decides expiry; games before 0.3.6 need the field and refuse a session their
own clock calls expired. An active name
returns 409 `name_unavailable`; entering its name never replaces that session.
An account's name signs in only with its password or a remembered sign-in, and a name
without an account enters as a guest ([Accounts](#accounts)).
Guest locale, buddies and relay admission counters belong to this player identity
and are removed with its session on logout or expiry. Idle lobby visits expire
after 120 seconds without a request or stream ack; admitted players retain their session during
a network outage while another unfinished player of their match is online, and
otherwise through 10 minutes of silence, until explicit finish/leave or the
match/session lifetime.
A later entry gets a new identity and empty preferences. The old
`POST /v1/accounts` returns 426 `upgrade_required`.
Failed requests return `{error:{code,message,phrases?,retry_after?}}`; messages must not echo
credentials. `retry_after` comes with a spent rate budget's 429 (the admission
limits below). A refusal a game shows its player (sign-in, accounts, versions, a full
or busy lobby) also carries its message as `phrases`, as a lobby line does (`chat`).
Clients show a 426 `message` (plain text, at most 160 characters) to the player with
the download page's link (vera20k.org; older releases link GitHub's), so it should name
the version players need; older clients show a fixed line. A 426 also makes a released
game check for updates again and offer a newer release at its next idle main menu,
even one the player skipped.
While the operator has closed sign-ins for maintenance, `POST /v1/sessions` returns 503
`maintenance` with the operator's one-line `message` (at most 160 characters), which
clients show in place of their "could not connect" text; other 503s mean the lobby is
unavailable. Players already signed in keep their sessions.
A client in the lobby, outside a match, whose event polls fail for about a minute tells
its player the connection was lost and stops polling; its next sign-in logs the old
session out first.
The HTTP paths keep `/v1`. Sign-in requires `protocol:4`; any other value gets 426
`upgrade_required`. Relay tickets are wire version 2 and relay datagrams envelope
version 3, plain text under a per-hop 4-byte MAC (`services/relay/README.md`); lobby
capability and relay wire versions are independent.
`GET /health` reports `protocol:4`.
`GET /v1/stats` needs no bearer and returns `{online,in_game,open_games,games_24h,games_total}`:
active sessions, those in a playing match, joinable game rooms, and games whose play
began in the last 24 hours and ever. It names no player or room, is limited to 30
requests a minute per address (an IPv4 address or IPv6 /64), and one answer serves
every reader for 10 seconds.

`POST /v1/commands` accepts `{request_id,after,operation,payload}`. A request ID is unique
within a session and retries are idempotent: a retry sends the same payload, and a request ID
the lobby committed, reused with another payload, is refused with 409 `request_conflict`; a
refused request records nothing. Responses and
`GET /v1/events?after=<cursor>` return `{events:[{sequence,kind,payload}],cursor}`.
Sequences increase within a session. An event appears once logically even when
responses are repeated. `GET /v1/channels?kind=0|41` returns `{kind,channels}`. The
`kind=41` list leaves out game rooms whose host's game is on another version than the
caller's ([sign-in headers](#sign-in-headers)). A game in the lobby asks for the
`kind=41` list, and `GET /v1/matches` with it, every 5 seconds (30 before), and for
`kind=0` every 53.
Clients poll at most once per second. Responses contain at most 256 events and
256 KiB of UTF-8 JSON.
`GET /v1/events/stream?after=<cursor>` with a WebSocket upgrade and the bearer (400
without an upgrade) pushes the same replies as text frames, from `after` on,
as soon as a request, ack or expiry adds events. Frames continue from the stream's last
frame, so one can repeat events a command reply already delivered or arrive behind it;
the game keeps the newer cursor and drops events it has. Its only message is
`{"ack":<cursor>}`, every 15 seconds (at most 60 a minute): it counts as a request for
idle expiry and is answered with a frame, empty when nothing is new. A newer stream of
the session closes the older with 4000 `replaced`; a stream whose request would now fail
closes with 4000 plus that status (4401, 4409, ...) and the error code as its reason.
Native games stream, poll whenever no stream is up and reconnect after 5 seconds,
doubling to 5 minutes; the browser build polls.
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
`id` is the lobby's own; it ignores one in `create` and `advertise`, where games from
0.3.6 leave it out and older games send it empty, so it stays optional there.
`kind` is 0 for chat and 41 for a game. The service sets `flags` from the room, never
from the creator: 256 when a game has a password, which clients show as a lock and
ask for before joining, otherwise 0. `max_users` (1–30) from `create` and the host's
`advertise` is the room's join limit, never below its current members; a join past
it fails with `room_full`. Games advertise the most people their room
takes, players and spectators: the host's Max players, which current games' hosts also
change in the room as Room size and advertise again, at most the room's thirty rows less
its AI, and with spectators off no more than its seats. Their host gives a newcomer a free seat, then the last AI's
seat, then a spectator's place, and their lobby lists each room as `users` over
`max_users`. A host `kick` with `keep_out:true` keeps that player out of
the room for five minutes, by account and by name without case: their `join` fails
with 403 `room_kept_out`, which clients show with the join ban's text while staying in the lobby. The other fields are presentation metadata, not authority. `name` identifies a channel uniquely for compatibility with the
existing shell. Member: `{player_id,name,flags,squad,clan_name,clan_tag}`. Flags bit 0
is the channel owner, bit 1 voice; the client adds its own local bit 32768. Servers
set identity and flags from the authenticated membership, never from user payloads.
The only public chat channel is `name:"Lob_41_2"` (`id:"public"`), which the modern UI
shows as VERA20K Online and the legacy UI by its retail name, German Bunker.
Other chat channels, apart from Quick Match's below, are neither listed nor
joinable; game rooms remain available through `kind:41`.

Commands and their payloads:

| operation | payload |
| --- | --- |
| `join` | `{channel:<channel name>,key}` |
| `create` | `{channel:<Channel>,key,ranked?}` |
| `leave` | `{}` |
| `visit_lobby` / `end_visit` | `{}` ([Visiting the lobby](#visiting-the-lobby)) |
| `logout` | `{}` |
| `options` | `{options:<HostOptions>}` |
| `choices` | `{row,slot:<SkirmishLocalSlot>,observer}` |
| `ready` | `{ready,revision,version,rules_digest,map_digest,effective_map_digest,staging_digest}` |
| `map_status` | `{status,revision,version,rules_digest,map_digest,effective_map_digest,staging_digest}` |
| `advertise` | `{channel:<Channel>}` |
| `start` | `{users:[name],revision,version,rules_digest,effective_map_digest,staging_digest}` |
| `finish` | `{match_id,back_to_room?,result?,winners?,ended_ago?,network_quality?}` |
| `relay_ready` | `{match_id}` |
| `latency_ready` | `{match_id,pings:[{source_index,round_trip_ms}]}` |
| `renew_relay` | `{match_id}` |
| `watch` / `renew_watch` / `unwatch` | `{match_id}` ([Watching](#watching)) |
| `map_upload` | `{digest,byte_length,offset,data,resource?}` |
| `chat` | `{text,to:[name],action,visit?}` |
| `ping` | `{}` (accepted and ignored; only older games send it) |
| `locales` | `{names:[name]}` |
| `set_locale` | `{locale}` |
| `buddies` | `{}` |
| `add_buddy` / `remove_buddy` / `locate` | `{name}` |
| `kick` | `{name,keep_out?}` |
| `ladder` | `{names:[name]}` |
| `balance` | `{}` |

`chat` reaches the other members of the sender's room, or with `visit:true` of the lobby
they visit, and with `to` only those it names, by exact name; it succeeds when `to` names
nobody there. Outside a room it fails with 409 `not_in_room`, and in a room past its lobby
phase with 409 `match_active`; [Quick Match](#quick-match) chat goes only to the matchbot.
Games page a player (`/page`, `/reply`, the Page dialog) as `chat` with `to:[name]`, under
the name their room or visited lobby lists; for a player neither lists, or during a match,
they show the page as failed and send nothing.

HostOptions is `{launch,map_metadata,vacancies,observer_row,cooperative_difficulty,seed}`.
A slot's `color_index` is 0 to 7 for the original colours, 8 for an observer and 9 to 30
for the game's [extra lobby colours](skirmish-colors.md); the lobby passes 0 to 255 on
unchanged.
Generated maps add `random_map:{start_staging,preview:{source_sha256,byte_length}}`.
`start_staging` is the canonical native `W` record: 37 numbers for up to eight starts,
and for a random map of 9 to 30 players four more for each start past the eighth,
bounded to 2048 bytes. A lobby from before those maps refuses a record past 37 numbers
or 512 bytes with 400 `invalid_options`;
`staging_digest` is its lowercase SHA-256, or the empty string for authored maps.
Cooperative rooms add `cooperative_campaign`, the host's campaign record as the game's
`C0` text (at most 512 bytes, starting `C0,`); a guest applies it before the options
that carry it. Other modes omit the field.
Multiplayer Campaign rooms add `campaign`, the authored room record
([Multiplayer Campaign rooms](Networking.md#multiplayer-campaign-rooms)), only on a lobby
that offers `campaign_rooms`; that lobby keeps it through room revisions, the start and
`watch_started`. Without the feature a game refuses campaign rooms online, and a 400
`invalid_options` for a campaign room's options turns them off until the next sign-in.
`launch` is the existing serde representation of `SkirmishLaunchSession` (including
its human rows, AI rows and launch options). The game's tests keep `tests/fixtures/lobby/`
current: `options.json` holds rooms' options as the game sends them at the limits it
reaches, `start_staging.json` holds `W` records, each with whether the game takes it
for its seed's capacity, and `game_contract.json` holds the selectable setup speeds
with each one's FPS and the map chunk size.
The lobby requires `launch.pre_fill_house_roster` because
guests decode it before rebuilding it from the rows. The lobby accepts setup speeds
0 to 6 and VERA's 8 in `launch.options.game_speed`. An older lobby can forward another
speed, which current guests refuse with the invalid-update line. `launch.options.teams`, a VERA room's
Teams count, is an optional integer from 2 to 15 that the game leaves out while Teams
is Off. A lobby from before it refuses options that carry
it with 400 `invalid_options`; the host's game then turns Teams Off, says so in the
room and sends its options again without it. Options that name a player no longer in
the room return 409 `player_not_in_room` instead, which leaves Teams as it is. A game
from before Teams cannot read a host's options while Teams is set and shows its
invalid-update line, in a room that cannot start anyway, since its players' versions
differ. `launch.options.auto_balance`, the Balance automatically setting of hosts on
0.3.2 to 0.3.4, is `true` or left out while it is off; those games send it only with a Teams count,
and current games never send it. `launch.options.queue`, a VERA room's join
queue, lists by name the room's spectators (humans with the `Observer` role) who wait
for a seat, first first, each once and at most 30; the game leaves it out while nobody
waits, and the host's game seats the first when a seat frees. A lobby from before it
refuses it in the same way; the host's game then stops queueing in that room, says so
and sends its options again, before it turns Teams Off if the lobby still refuses. `launch.options.host_rules`, the rules a VERA room plays, is
`{terrain_deformation?:true, superweapon_limit?:2..20, building_production_queue?:true, personal_rules?:<base64>}`: the host's
Terrain Deformation, its Superweapon Limit and its Building Production Queue. `personal_rules`, the host's `VERARULES.INI`
in standard base64 of at most 8,192 bytes (10,924 characters), came from games before the
file was retired; current games never send it and refuse a room that has it. The game leaves each part out while it is off, and
the object while all are; every player's match plays them in place of the player's own. A
lobby from before the Superweapon Limit refuses options that carry `superweapon_limit`; the host's
game then turns it off, says so and sends its options again, keeping the other rules. A
lobby from before the Building Production Queue refuses options that carry
`building_production_queue`; since such a lobby may list matches, the host's game turns
the queue off on any refusal of options that carry it, says so and sends its options
again, before anything else. A
lobby from before the room's rules refuses them in the same way; the host's game then
drops the room's rules, says so and sends its options again, before it stops queueing and
then turns Teams Off if the lobby still refuses. `watching`, the room's Watching, is `off`,
`delayed` or `live` ([Watching](#watching)). A lobby from before it refuses it in the same
way; the host's game then turns Watching off, says so and sends its options again, before
any of the others. Only a fresh 404 on the watch list for the latest, still unchanged room
publication can retire an unsupported setting. A lobby that lists matches already understands those fields, so its refusal
keeps the room settings. `map_metadata` is
`{display_name,player_capacity,official,byte_length,digest,source_sha256,effective_digest}`. The legacy
INI `digest` remains distinct from lowercase SHA-256 over the physical map bytes;
online `map_digest` agreements compare `source_sha256`. `effective_digest` and
`effective_map_digest` compare the parsed gameplay map after its applicable include
expansion, so identical root bytes cannot conceal different local dependencies. Until a
game holds a verified copy of the map, its agreements carry map status 0, an empty
`map_digest` and 64 zeros as `effective_map_digest` (0.3.5 and earlier sent the digests
of an unverified copy); the lobby checks only their shape, since `start` compares the
map digests of members at status 1 alone. `vacancies` is an array
of `open`, `closed`, `observer` or `waiting`. A network map's display name keeps legacy
Windows punctuation and fits the lobby's 256 UTF-8 bytes without control characters. A receiver resolves its own
`launch.local_player_index` from the authenticated player; it never uses the host's.
`slot` uses `SkirmishLocalSlot` serde. Options are authored only by the room owner;
the service increments `revision` and clears guest readiness on a material change.
Starting requires at least two playing participants, counting the actual
`launch.opponents` AI rows. A host can select an AI opponent and start alone;
`ai_player_count` without an AI row is insufficient. AI players may play while observers
watch, and with two or more AI everyone in the room may watch. The map's
`player_capacity` must cover every playing human and AI row, or with Teams set
(`launch.options.teams`), where a team's players share its bases, one start for each
team that has players or AI and one for each playing participant on no team. The host's
game checks this limit before publishing; past it, it tells the host the map is too small
and publishes nothing. A lobby from before this refuses options past the map's capacity
with `invalid_options`, and a start with both observers and AI with 409 `not_ready`; one
from before AI-only games refuses a start where every human watches with 409 `not_ready`.
`start.users` contains exactly the canonical human names, including observers, and never
includes AI rows.

Event kinds and payloads:

| kind | payload |
| --- | --- |
| `joined` / `created` | `{channel:<Channel>,members:[Member],visit?}` |
| `channel_left` | `{}` |
| `room_reopened` | `{channel:<Channel>,members:[Member]}` |
| `member_joined` | `{member:<Member>,visit?}` |
| `member_left` | `{name,visit?}` |
| `host_changed` | `{member:<Member>, channel:<Channel>}` |
| `options` | `{revision,options:<HostOptions>}` |
| `choices` | `{from,row,slot,observer}` |
| `ready` | `{from,ready,revision}` |
| `map_status` | `{from,status,revision}` |
| `chat` | `{from:<Member>,text,private,action,sent_at,ansi?,quick_match?,phrases?,visit?}` |
| `game_started` | `{match_id,revision,version,rules_digest,seed,users:[{player_id,name,flags,source_index}],relay:<RelayGrant>}` |
| `start_confirmed` | `{match_id}` |
| `timing_confirmed` | `{match_id,pings:[integer],relay:<RelayGrant>,ranked?}` |
| `relay_renewed` | `{match_id,relay:<RelayGrant>}` |
| `watch_started` | `{match_id,revision,version,rules_digest,staging_digest,seed,tournament,quick_coop,users:[{name,flags,source_index}],options:<HostOptions>,pings:[integer],delay_seconds,playing_since,ranked?,relay:<WatchGrant>}` |
| `watch_renewed` | `{match_id,relay:<WatchGrant>}` |
| `map_available` | `{digest,byte_length,resource?}` |
| `start_rejected` | `{message,phrases?}` |
| `locales` | `{locales:[{name,locale}]}` |
| `buddies` | `{names:[name]}` |
| `located` | `{channel:<Channel or null>}` |
| `kicked` | `{channel:<Channel>,user:<Member>,by:<Member>}` |
| `ladder` | `{season,records:[{name,points,os,rank,wins,losses,disconnects}],high_scores:[{name,points,rank}]}` |
| `balance` | `{teams:[{name,team}]}` |
| `error` | `{code,message}` |

A `chat` event's `sent_at` is the Unix time in seconds the service sent it. Games
show it, in the player's local time, before each line of the chat lobby.

Lines the lobby writes itself, from the notice sender (`VERA20k`) and the matchbot's
`Info` lines, also carry `phrases`: the line as a list of sentences `{key,values?}`, of
which `text` is the English joined by spaces (for a matchbot line, the text after its
first word, `Info`). A client shows each sentence as its
language file's `VERA:Lobby<key>` with `{name}` filled from `values`, and shows `text`
when it lacks one of the keys, a sentence names a value its phrase lacks, or a phrase or
join fills past 2048 UTF-8 bytes, the chat limit. A value is a string, a number, a phrase, an
`{and:[value]}` list read "a, b and c", or a `{unit:[value]}` list read "a, b, c"; the
language file gives both joins. A phrase whose values hold `count` takes the plural form
its language gives that number. The keys and their English are the `VERA:Lobby` entries of
`src/i18n/vera20k.en.ini`, and a key the lobby adds needs its English line there.
A key keeps its values and meaning once a release has shipped it, and a change takes a
new key: games before 0.3.6 show a value their sentence names and the phrase lacks as
its `{name}`, while every game shows `text` for a key it lacks.
Operator notices carry no `phrases`: they come in the session's language when the
operator translated them.

`start_rejected`'s `message` is English. Games from 0.3.6 on match
the four messages the lobby sends: `A player left before the game started.`,
`A player cancelled the game start.`, `A player could not connect to the gameplay relay.`
and `A player could not complete the peer latency check.`, and show each in the player's
language; for the relay message a game whose `relay_ready` the service took says another
player's game could not reach the server, and any other says its own could not. Any
other message, including a reworded one, shows the general line that the match could
not start, as every message does in older games. The lobby therefore keeps these four
messages unchanged while it admits games from 0.3.6 on;
`tests/fixtures/lobby/start_rejected.json` lists them.

A start that times out connecting also says who held it up: every game's
`start_rejected` then carries `phrases`, one `StartNotConnected` holding
`{names:{and:[name]},count}`, the players whose games never sent `relay_ready`, so every
game names the same players after its own line. A game that already gave up on the start
adds the names to the line it keeps for its next chat. One that times out in the latency
check names no one: a game sends `latency_ready` only once it timed every peer, so one
broken link stops every report. Released games read only `message`.

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
first 34 bytes with domain `VERA20k relay ordering certificate v1\0`. A lobby
whose every member build reads relay rate markers may instead issue a 67-byte
`VO2C` certificate: the same fields, then a flags byte (bit 0: the host turned
relay pacing off; bit 1: the relay orders this one-source match, below; bit 2:
continuous pacing, never with bit 0, issued only when the relay and every admitted
build read it; other bits zero), then HMAC-SHA256 over the first 35 bytes with
domain `VERA20k relay ordering certificate v2\0`. Every grant of one match carries
the same version and flags; clients reject a renewal whose flags differ. The signed
mask includes all human sources (including observers); AI has no source. FPS
follows launch speed `[60,45,30,20,15,12,10]` for stored settings 0–6;
VERA's setting 8 selects 90 FPS. Setting 7 is not a selectable setup speed.
`tests/fixtures/lobby/game_contract.json` lists these rows from the game's own table,
and the relay accepts no other FPS (`FRAME_RATES` in `services/relay-protocol`).
The certificate lasts 24 hours;
every UDP control still needs a live authenticated short-lived ticket. The relay
binds the first matching startup agreement to this roster/clock and starts once
every remaining signed source is ready. A source that never becomes ready leaves
in the first frame after 60 seconds of silence, or 3 minutes after the first
Ready; a match federated over several relays is cancelled instead. Renewals
preserve the timeline and batch IDs.
`federation`, present only in a federated match, is the same base64url roster
in every grant: `VO1F`, room UUID16, relay count (2–6), then per relay IPv4,
LE port and LE source mask, then per relay nonce16, wrapped32 and tag32. The
masks partition the signed sources; a relay refuses Ready as an agreement
mismatch when they do not. `wrapped` is the match link key XORed with
HMAC-SHA256(relay ticket key, `VERA20k relay federation wrap v1\0` ‖ room ‖
nonce); `tag` is HMAC-SHA256(relay ticket key, `VERA20k relay federation roster
v1\0` ‖ the table through the masks ‖ index byte ‖ nonce ‖ wrapped). The client
appends it unchanged to every CONFIRM; the first relay coordinates the start.
The lobby federates a match only when every human's sign-in request carried the
header `Vera-Relay-Federation: 1`, which desktop games send since they forward the
roster. Browser games do not ([sign-in headers](#sign-in-headers)), so a match with a
browser player stays on one relay.

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
options echo. While the request is staged, the host's client holds its own
edits, kicks a newcomer and ignores `balance`; a guest's changed choices, the
host's kick of a guest and any departure change the options, which cancels the
staged request, as the host leaving does. Rejected or timed-out starts
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
require, but the relay refuses a one-source certificate. The one exception is one
person who watches AI play: when its room has watching on, its game signed in with
`Vera-Watch-AI: 1` on `POST /v1/sessions` and the selected relay is configured
with both `watch: true` and `ordered_alone: true`, the certificate carries VO2C flags
bit 1. The relay orders that one source so others can watch ([Watching](#watching)).
The optional `ordered_alone` flag defaults to false. Enable it after upgrading the
relay. Host and watcher renewals keep the decision made at match start.
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
host. The public channel takes 300 joins and 300 chat lines a minute; a game room takes
60 joins a minute. Re-joining the current room is free.
Map upload/download bytes each have a 32 MiB/minute player budget, and so do the
earlier chunks the service re-reads to check a finished upload.
Canonical options are limited to 24 KiB, choice objects use the exact canonical
slot schema, and unknown request/option fields are rejected. The actor refuses a
request with 429 `busy` while 320 requests are pending, a POST while 160 are, and a
body that would take the parsed bodies pending past 4 MiB; an event is at most 64 KiB.
Active sessions, guest profiles and public-room membership are capped at 320, of which
guests hold at most three quarters, 240, so that accounts always fit. When guests hold
them all, a new guest from a smaller range replaces the longest-silent guest of the
largest range, unless that guest is in a match, and is otherwise refused (503
`service_full`). Per-address limits key on the client's network: an IPv4 address,
or an IPv6 /64, with the enclosing /48 as the range. New guest entries are limited
to 40 an hour per address and 640 an hour in all, with the sign-in and ingress
limits also applied. One range may use at most a tenth of these lobby-wide budgets: 32
of the sessions (503 `network_full`), 64 new guest entries an hour, 30 public-channel
joins and 30 chat lines a minute, and 1,000 of the 10,000 daily translations (below).
A request past a rate budget gets 429 `rate_limited`, or `lobby_busy` past a lobby-wide
total, with `retry_after`, the whole seconds until that budget refills, also sent as a
`Retry-After` header; a spent daily translation budget answers 503 instead (below).
One name can be taken 30 times in 5 minutes; refused entries do not count. These are
service admission limits, not gameplay timers.
Clients verify and install into a
local cache, preserve existing installed maps, and still require user readiness.
Generated maps transfer the `.SED` bytes as the map and the separate native PCX
preview with `resource:"preview"` (512 KiB maximum). Preview downloads use the
selected seed's URL plus `&resource=preview`; response `digest` names the preview.
Both resources use the same ownership, ordered chunk, hash and cleanup rules.
Generated capacity equals the staged start count (2–30); observers consume only
transport slots. Start admission also compares the canonical staging digest, so clients cannot
agree on seed bytes while using different generated start cells.

<a id="map-library"></a>

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
the map's `[Basic] Name=`, `file_name` its catalog filename, and `header`
the base64 of the map's own `[Basic]`, `[Preview]`, `[PreviewPack]`, `[Waypoints]`,
`[Header]` and `[RandomMap]` lines, byte for byte. No two library maps share a
`file_name` without case: one whose name would, or whose name has nothing left to save
under, ends in `-<first 8 hex of its SHA-256>.yrm`. The service supplies lowercase
`.yrm` catalog filenames for older games. Current games accept `.map`, `.yrm` and
`.mpr` in any case. They skip entries with an unsupported extension, a path, a colon,
a control character, a leading dot, a trailing dot or space, another character Windows
forbids, or a name Windows reserves. The catalog carries
`ETag: "lib-<revision>-<16 hex>"`, the hex from its content (catalogs written before:
`"lib-<revision>"`), and answers 304 to a matching `If-None-Match`, weak or not, or `*`.
`GET /v1/library/maps/<sha256>` downloads one map, and
`GET /v1/library/maps?search=<text>&players=<n>&page=<n>` searches them (newest first,
50 a page). The catalog and downloads also answer HEAD, and 503 `service_unavailable`
when the library's storage fails. All three are public, and the search is limited per
network; the service keeps no player, session or address with a map. The game reads the catalog
when the player goes online and keeps it for the run once it arrives; a failed read is
tried once more in that visit, and again on the next. In an online room (not one that
takes only official maps), Choose Map lists its library members under Online Maps, starred maps first. An existing custom map in the selected mod,
game folder or VERA's `Maps` folder is identified by its length and SHA-256,
including files the player renamed. Use Map on a missing map asks
to download it, checks its length and SHA-256, and saves it in VERA's `Maps` folder,
where it is an ordinary map in the applicable game types. The game saves a new
file as `<stem>.map`, or `<stem>-<first 8 hex of its SHA-256>.map` when the name
is taken. Identical files keep their existing name and extension.

For the official lobby, desktop games also fetch
`https://vera20k.org/maps/categories.json` in a separate background request on
the online visit. The website supplies `{"categories":{"<sha256>":["survival", …]}}`;
a hash can have several category IDs. Unknown IDs are ignored. Category entries
are joined by SHA-256 to the lobby's map records and their byte-matched local
copies. Category data never selects a game mode or changes room rules. On feed
failure, the full Online Maps list remains usable. Custom lobbies skip the
website request. Browser builds use their existing room transfer path and do
not open the desktop library picker.

The match admission is retained independently of channel membership until
finish/timeout. The original-style shell leaves its game room at the start and
postmatch rejoins chat, where it may create or join another game. VERA's own room
stays joined through the match, and its `finish` sets `back_to_room: true`. When
the last player finishes, the players still in the room who asked for it get it
back: `room_reopened` (owner first) and then `options` with the host's settings
and every readiness cleared. Players who finished without asking are dropped from
it silently. If the owner is absent, the next eligible host (below) gets the
room. If there is no eligible host or another open room took its name, the lobby
sends each remaining player `channel_left`. An expired match closes its room with
the same event. Each rematch is a fresh match with fresh relay credentials.

Host handover: when a game room's owner leaves before start (Leave, sign-out or an
expired session), the player who has been in the room longest becomes its owner,
provided every game still in it signed in with the header `Vera-Host-Handover: 1`, which
games send on `POST /v1/sessions` once they take over a room. The lobby sends
`host_changed` with the new owner's member and the room's channel, then `member_left`
for the old one, and clears every readiness; a room named as games name rooms
(`<name>'s_game`) becomes `<new>'s_game` unless another open room has that name. From
then on the room is listed to the new owner's version, and it stays ranked or unranked
as it was created. The room's options, map upload and advertisement stay until the new
owner's game publishes its own. A Quick Match room, or one with a game that did not send
the header, closes with `channel_left`. Earlier lobbies also sent `error` with
code `host_left` before the close, which released games displayed as a refused
request. A lobby from before handover closes every room whose host leaves.
Membership, authorization, readiness, sequence history and idle expiration are server-owned.
Idle lobby visits expire after 120 seconds. An unfinished playing membership
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
`pacing:{slowdowns,slowed_by,slowed_seconds},outcome:{desync_frame,reconnects,started,failure,start_hash}}`.
Times are seconds, or milliseconds where the name says so; `command_round_trip_ms`
is `{mean,max,samples}` or null, `tick_ms_p50`, `tick_ms_p99`, `fps_p50`, `fps_p5` and
`desync_frame` may be null, and `slowed_by` lists the House each relay slowdown named.
`started` says the relay's clock for the match started. `failure` is null, or names the
error that ended the match on this game: `agreement_mismatch`, `start_cancelled`, `modified_retry`,
`invalid_cursor`, `refused`, `history_expired`, `invalid_timeline`, `desync`, `unavailable`,
`federation_lost` (a marked game ended after its relays lost contact),
`absent` (no answer from the relay for ten minutes), `queue_full`, `newer_client_required` or
`local_error`. `start_hash` is null once the
relay's clock started; before that it maps each part of the start this game offered the
relay, at most 64, to 16 hex digits, so two games' reports of one match differ in the part
that differs: the `setup_*` parts, `world` with the world's own parts, `build`, `rules`
and, from games that certify it, `asset_files` (what the simulation reads from picture
and model files).
Games before these fields send none of them.
`fps_p50` and `fps_p5` read the presents of a moving picture: one that stands between
frames is not drawn again, and the present after it starts a new interval.
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
gets the same finish again without it. `tests/fixtures/lobby/network_quality.json` is a
report as the game sends it, every field filled, which the game's tests keep current.

`POST /v1/translate` accepts exactly `{text,target}` with the bearer and returns
`{text}`: `text` translated into `target`, a language tag such as `en` or `zh-Hans`.
A reply can start with a short bracketed note, in that language, naming the language
it was translated from. It can also carry `source`, that language's tag, when it is one
of the translation targets; clients hide a translation whose `source` is its `target`.
`text` is non-blank and at most 2048 UTF-8 bytes, the chat limit. Clients translate
received chat only after the player opts in, and always keep the original visible.
A deployment without a translator, a failed model call or one taking over 10 seconds,
a new message while 8 others wait on the model, or a spent daily translation budget,
the service's or one range's share of it (above), returns 503
`translation_unavailable`; an unsupported target
returns 400 `invalid_request`. Each player may ask 60 times a minute. Successful
replies are cached for 24 hours by `target` and `text`, and readers asking for the
same message while it is translated share one model call. Translation never waits
for, or delays, room commands.

`GET /v1/match/resume?match_id=<UUID>` requires the bearer and its own
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
2048 UTF-8 bytes, kept for 1–10080 minutes, and optional `translations`: `{<tag>:text}`
for at most 40 BCP 47 tags, each text within the same limit, of which each session gets
the one for its `Vera-Language`. `{minutes:0}` alone clears it. Both
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
2048 UTF-8 bytes, with optional `translations` as for a notice. `{text:null}` clears it.
Both return `{welcome}`, the text or `null`. `POST /v1/admin/maintenance` takes the same
bearer: `{text}`, one line of at most 160 characters with optional `translations` of the
same kind, closes sign-ins, and `{text:null}` opens them.

A browser gateway (`tools/wasm/gateway`) sends its players' requests from its own
address. It may name the player's address in `Vera-Client-Address` together with
`Vera-Gateway-Key`, the deployment's `GATEWAY_KEY`; the lobby then keys that request's
per-address limits on the named address. Without a `GATEWAY_KEY` of at least 32
characters, or with another key, the named address is ignored and the request's own
address counts. A gateway refuses to start with a key shorter than 32 characters.

A client that names a version ([sign-in headers](#sign-in-headers)) older than 0.2.7 is
refused at sign-in with 426
`upgrade_required`, since clients before it cannot play through the relays'
adaptive transport (UPLINK, `services/relay/README.md`); one that names no
version is not. Relay compatibility plays every release from this floor, which
`OLDEST_ADMITTED` in `tools/relay_compat.py` must equal.
A session whose version is older than `version` in
`https://downloads.vera20k.org/latest.json`, which the lobby reads at most every
10 minutes, receives one such chat line per newer release, naming both versions,
since a match starts only between equal versions (`ready`, `start`). It arrives
as a notice does: at once in the public channel or a game room that has not started,
otherwise on the next public-channel join. Its phrase key `NewRelease` makes a game
check for updates again, as a 426 does.
latest.json also names the release's `build`, its multiplayer build
([build check](Networking.md#build-check)), from the releases that log it
(`.github/release/downloads.md`). A session on the release's version whose sign-in
names another build receives one line the same way, `NotRelease` with `version`: its
game is not the release, so it plays only with players on its own build.
A session that still joins a game room from another version, through a buddy's game or
its name, makes every member hear a `chat` line from the notice sender naming both
versions (`VersionMismatch`). When the versions are the same, the line names no build:
where latest.json names that version's `build` and only one of the two games has it,
it names the other one's player as not on the release (`ReleaseMismatch` for the
joining player, `HostReleaseMismatch` for the host), and otherwise says the joining
player has another build than the room (`BuildMismatch`); each has `name` and `version`.
Where the join could not tell, since one of the games names no build, `VersionMismatch`
with both agreement `version` strings goes to the room once when a member's `ready` or
`map_status` names another one than the host's, since such a room cannot start.
A member on the host's build whose `rules_digest` differs from the host's makes the room
hear one line naming them (`LobbyRulesMismatch`, which has only `name`). As the CnCNet
client compares a joining player's files with the host's at once, such a member is named
as soon as they and the host have sent `ready` or `map_status` for the room's settings,
which every game does when the settings reach it. With a generated map (`random_map`)
both must also have reported `map_status` 1: before it, its game names the rules without
the map's. Until every member has answered, the lobby only adds such lines, and none
while the host is named: one who has not answered may agree with either side. Once all
have, as `start` compares them: when every other player on the host's build shares one
`rules_digest`, two or more of them, and the host's differs from it, the line names the
host instead (`LobbyHostRulesMismatch`, also only `name`); otherwise each player whose
rules differ is named, so with one other player, that player. So the first player to
differ is named until the rest show that the host's rules are the odd ones. A player is
named again only after they stopped being named (their rules matched, or the line moved
between the host and the others) or after they left the room and joined it again, not
when a change of settings has every game send its agreement again.

Where both games named what they change from the original game at sign-in
(`Vera-Changes`, below), the line instead says what differs. It opens with
`LobbyRulesDifferFromHost` or `LobbyHostRulesDifferFromOthers` (only `name`), compared
with the host or, when the host is named, with the first other player, whose rules every
other player shares. Then `LobbyOnlyHas` (`name`, `changes`) names what only the named
player has, and `LobbyOnlyHostHas` or `LobbyOnlyOthersHave` (`changes`) what only the
other side has. When neither side has anything the other lacks, `LobbyBothHave` or
`LobbyAllHave` (`name`, `changes`) names what both have in copies that differ, or
`LobbyNoChangesNamed` says VERA names no change in either, so their installed game files
differ. `LobbySameRulesToStart` closes the line. `changes` is an `and` list of
`LobbyChangeMod` (`name`, the mod's title), `LobbyChangeIniRoots`, `LobbyChangeAresOn`,
`LobbyChangeAresOff` and `LobbyChangeFiles` (`count` and `files`, a `unit` list of up to
six file names, then `LobbyMoreFiles` with the `count` left). A mod or Ares setting
differs when the other side has another or none; file names compare ignoring case, and
when the other side changes no files, every file of the named side's `count` differs.
Games without a key show its English `text`. VERA shows these lines, and the version and
build lines above, in red, as the CnCNet client shows its modified-files warning; a game
before it shows them as the lobby's other lines.

Each `join` of the public channel gives the joining session one `chat` line from the
notice sender after its `joined`. The line says how many players are online: in the
public channel, in game rooms, playing, for sessions that play Quick Match searching on
their version, and elsewhere. It also counts the game rooms the list leaves out for another
version and, when the list holds none for the session, says how to start a game.

## Sign-in headers

A game says what it is and what it can do in headers on `POST /v1/sessions`, each
optional: the lobby ignores a missing or malformed one, and an older lobby one it does
not know.

- `Vera-Version: <MAJOR.MINOR.PATCH>`: its version. Released games name it in
  `User-Agent: VERA20k/<version> multiplayer-client`, which the lobby reads when this
  header is absent; a browser cannot set its User-Agent.
- `Vera-Build: <build>`: the agreement `version` its `ready` and `map_status` carry
  ([build check](Networking.md#build-check)), 1–128 characters of `0-9A-Za-z.:_+-`,
  which two builds of one version (two nightlies, or a nightly and the release) do not
  share.
- `Vera-Language: <tag>`: its player's language, a BCP 47 tag of two or three letters
  and at most three subtags such as `zh-CN`. The operator's welcome, notice and
  maintenance message come in that language when the operator wrote it: the translation
  for that tag, else for its language alone (`zh`), else for another region of that
  language, else the operator's `text`, which a missing or malformed tag gets.
- `Vera-Changes: <percent-encoded JSON>`: what the game changes from the original game,
  which a room's rules line names: an object with `mod` (the mod's title), optional `kratos: true` when the startup rules enable Kratos, `ini_roots:
  true` (config.toml names other Rules, Art, AI, mode roster or map catalog files, or
  another theater suffix, or [RA2 Mode](ra2-mode.md) is on), `ares` (`on` or `off`, the player's Ares setting where it
  decides which game opens: On unless a selected mod mounts `ares.mix` anyway, Off only
  with such a mod or over rules that hold an Ares key) and `files` with `count` (up to 32 names of the game-folder files
  that change the rules the network compares, of `count`), each left out when unchanged:
  `{}` for the original game. Texts are 1–64 bytes of well-formed UTF-16 without control
  or format characters (such as bidi overrides), `count` at most 65535 and the header at
  most 8192 characters; the lobby ignores a malformed one whole.
- `Vera-Quick-Match: 1` ([Quick Match](#quick-match)), `Vera-Host-Handover: 1` (host
  handover), `Vera-Watch-AI: 1` ([Watching](#watching)) and `Vera-Relay-Federation: 1`
  (a RelayGrant's `federation`): what it can do. A browser game sends every header but
  this last one, which its gateway would not forward either (`FORWARDED_HEADERS` in
  `tools/wasm/gateway/server.mjs`).

Two games compare their builds when both name one, else their versions. They are on
another version when both name what they compare and it differs: they could never start
a match together, so neither sees the other's game room or match to watch. Quick Match
pairs only games whose compared values are equal, where two that name nothing are equal.

## Room list summaries

The public game-room list (`kind=41`) and each listed watchable match can
also carry `room_summary: {map?, teams?, forming?, observers?, computers?}`.
The lobby derives a room's figures from the host's published options and
current membership. A running match uses its frozen options. The fields are
display-only. A client never sends them on create or advertise.

`map` names the map. `teams` lists occupied seats per team in team-number
order, counting playing humans and AI. For example, `[0,4,0,4]` means four
on team 2 and four on team 4. An empty array means a known free-for-all.
`forming:true` means an open room is still being set up. If it also carries
partial team counts, the list says “Teams forming” and the details below the list show
the occupied seats. Missing team data leaves the layout unknown, including
setups where opposing team assignments share a starting House. `observers`
counts seated human spectators and
`computers` counts AI. Remote watchers remain in the existing `watchers`
field. Older clients ignore the extra summary; on an older lobby or rollback
a new client uses the original map and occupancy information it has.
A watch list without a summary can still supply seated spectator and AI
counts on the match itself. Those counts establish no team assignments.

## Visiting the lobby

A member of a VERA game room before its start may `visit_lobby`: they also join the
public lobby and stay in the room, which goes on as before (chat, choices, readiness,
host changes and the start all reach them). They get the public lobby's `joined` with
`visit:true`, and the lobby's others get `member_joined` as for any join. While the
visit lasts, the public lobby's `chat`, `member_joined` and `member_left` reach the
visitor marked `visit:true`, and their `chat` with `visit:true` goes to the lobby
instead of the room. `end_visit` leaves the lobby (`member_left` there, nothing to the
visitor); so do leaving the room, joining or creating another, signing out, an expired
session and the room's `start`, before its `game_started`. A kick (`kicked`) or the room
closing (`channel_left`) leaves the visitor in the public lobby as their channel.
The close event belongs to the room. A visitor the lobby no longer lists gets
`channel_left` after `kicked` and is in no channel. A repeated
`visit_lobby` answers `joined` again; a Quick Match room, also after its first start, or
one past its lobby phase refuses it (409 `room_busy`). A lobby from before it lists no
`visit_lobby` in the sign-in's `features` and refuses the command as unknown. `locate`
names the visitor's room, not the lobby.

## Watching

A player in the lobby can watch a match being played. Their game runs it from its first
frame from the relay's command stream, then follows it some time behind its players. A
watcher is no part of the match: it has no source, sends nothing into it, and the
players' games never learn it is there (relay README, "Watchers").

`GET /v1/matches` returns
`{matches:[{match_id,name,map,mode,players,observers,computers,since,names:[name],watchers,max_watchers,delay_seconds,room_summary?}]}`:
the matches the caller may watch. These are matches in play between two or more humans,
at least one of them playing (a watcher's game follows a player), or, to a caller that
signed in with `Vera-Watch-AI: 1`, where only AI play while every human watches (a
watcher's game then takes the first observer's seat), one human only when its
certificate carries VO2C flags bit 1; with watching on, in a
room without a password, on a relay that lets watchers in and on the caller's version
(as `kind=41` lists rooms), that the caller plays no part in. Clan rooms, generated maps
and stock co-op missions are never listed: a watcher's game cannot yet rebuild a start
that splits players by squad, comes from the host's map seed or from a stock co-op
campaign. A lobby that offers `campaign_rooms` may list an authored campaign match: its
`watch_started` options keep the `campaign` record, which the watcher's game checks
against its own installed series. `name`
is the room's; `map`, `mode`, `players`, `observers`, `computers` and `since` (when play
began, Unix seconds) are as the dashboard shows a match; `names` are the playing humans
(none where only AI play);
`watchers` counts who watches now, of at most `max_watchers` (10); and `delay_seconds` is
how far behind its players a watcher sees it: 120, or 0 when the host chose `live`. Quick
Match and ranked games are always 120 seconds behind; other rooms follow the host's
`watching`, and options without it leave watching off. A lobby from before watching
answers 404 `not_found`, and the game lists no running matches.

`watch` replies `watch_started`: what every player's game got at the start (`revision`,
`version`, `rules_digest`, `seed`, `users` without `player_id`, and that revision's
`options`), the room's `staging_digest` (empty for an authored map) and `tournament`
value, `quick_coop:true` for a Quick Co-op room (a paired room with tournament 0), the
`pings` of `timing_confirmed`, the delay, `playing_since`, `ranked:true` for a ranked
game, and a WatchGrant. It fails with 409 `unknown_match` when the caller may not watch that match,
409 `match_active` while the caller plays one, and 409 `watchers_full` when the match has
its 10 watchers. `match_active` also refuses `create`, `join` and `logout` while the caller
plays, and a room's commands once its game started. Of the room and match commands, games say
why only for `watch`; account deletion refuses with it too and shows its message ([Accounts](#accounts)).
Watching the same match again, as after the game restarted, keeps the watcher's place. An account is always the same watcher of a match on a relay, its
identity there an HMAC of the match and the player under the relay's ticket key, so a
newer watch ticket replaces the relay session it had rather than taking another place.
While the match plays, its watchers download its map as a
room member does, naming it: `GET /v1/maps/<source_sha256>?offset=<n>&match_id=<id>`.

WatchGrant is `{address,ticket,packet_key,payload_key,ordering_certificate}`. The relay is
the nearest of the match's relays that serves the watcher, one relay even in a federated
match. `payload_key` and `ordering_certificate` are the match's. The ticket is a version
3 watch ticket (relay README, "Watchers"): match, expiry, nonce, the watcher's identity
and the delay, signed with the relay's ticket key; `packet_key` is derived from it as a
player's is. A grant lasts 300 seconds: `renew_watch` replies `watch_renewed` with a new
one on the same relay for the same watcher, and the game renews at half its life. Once
the match ends for its players `renew_watch` fails with 409 `unknown_match`, and the
watcher plays on to the end of the stream with the grant it holds, which outlasts the
delay. `unwatch` frees the watcher's place at once; a watcher that stops renewing frees it
when its grant runs out. Watchers never join the match: `finish`, the players' grants and
the end of the match are as without them.

A game from before watching never asks for the list. Matches are listed only to their
own version, and the relay compares a watcher's startup agreement with the players', so
a watcher always runs the players' build. A relay from before watchers refuses a watch
ticket, so the lobby lists only matches on relays it knows let watchers in.

## Quick Match

1v1, ranked while a season runs ([Ranked games](#ranked-games)); Quick Co-op (`Lob_39_`) has no lobby. Games that play it send
`Vera-Quick-Match: 1` on `POST /v1/sessions` ([sign-in headers](#sign-in-headers)). Only
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
a fresh random u32 seed and each player's COU and COL as sent. Pairing joins two players
on the same version ([sign-in headers](#sign-in-headers)): a searcher pairs
with the waiting player who makes the most even game, weighing each by their OS
([Ranked games](#ranked-games)), one a season would rank them with before one it would not,
as long as the favorite's chance to win is at most 60%, a limit that widens with the longer
wait of the two until, after 60 seconds, anyone pairs. While a season runs, a pairing it
will not rank (a guest, or two players who played three rated games against each other in
the last 24 hours) first sends both `Info This match will not be ranked: <why>.`
Every request the lobby serves pairs again those whose waits now allow it; the longer waiter
hosts. Leaving `Lob_40_0` (leave, logout, expiry or another join) leaves the queue.
A request that puts a player in the queue without pairing them gets private `Info <text>`
lines after `Working`, which the game prints as they come. These say:
- that the match starts when a player near their OS presses Quick Match, or any player
  once they have waited a minute;
- when no season runs, or they play as a guest, that the match will not be ranked;
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
  the `x@y.z` shape, one account each. `tests/fixtures/lobby/accounts.json` holds names,
  passwords and emails at the edges of these rules, each with whether the game takes it;
  the lobby gives each the same answer.
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
- `/v1/accounts/delete` accepts either `{name,password}` or `{email,password}`. Both
  require the current password. An email identifies the registered account
  case-insensitively. Successful deletion by email returns `{name}` with the account's
  username so the game can clear that account's remembered sign-in. Deletion by
  username keeps its empty JSON success response. Deletion removes the email, remembered
  sign-ins, ratings and standings, signs the player out and mails the owner. Anyone
  may then take the name. Ranked game records stay 90 days for disputes, and no
  profile names the deleted account. Wrong passwords count toward the sign-in limits.
  An unknown email and an incorrect password both return 403 `wrong_password`.
  A name without an account returns 404 `no_account`. Deletion returns 409
  `match_active` while the player is in a match or a ranked game of theirs is still
  waiting for results.
- `/v1/accounts/rename` `{name,password,new_name}` returns 200 `{name}` with the new
  name; no game sends it. The new name follows the session rule, and the password stays
  the same. Ratings, standings, past seasons and ranked games move to the new name; the
  rename drops every remembered sign-in, signs out whoever plays under the old name and
  mails the owner. An account renames at most once in 30 days: sooner returns 429
  `rename_too_soon` with `retry_after`. Wrong passwords count toward the sign-in limits,
  which refuse a rename with 429 `account_locked` or `rate_limited` as they refuse a
  sign-in. It also refuses with 400 `invalid_request` or `invalid_name`, 403
  `wrong_password`, 404 `no_account`, 409 `match_active` as deletion does, 409
  `same_name` for the account's own name (a change of case only is allowed), and 409
  `name_unavailable` for a name that is reserved, held, an account's or being
  registered, or has a player online, a match or ranked games under it. For 30 days the
  old name stays held for the account: registering it returns 409 `name_unavailable`,
  `GET /v1/players/<old name>` answers the account's profile under its new name, and
  deleting the account frees it. A change of case only holds nothing.

At `/v1/sessions` an account's name needs its `password` or a `sign_in_token`, never
both: with neither it returns 403 `name_claimed`, with a wrong one 403 `wrong_password`,
and 10 wrong passwords in an hour return 429 `account_locked` for 15 minutes and mail
the owner. 30 wrong passwords from one network in an hour return 429 `rate_limited`.
An account's sign-in has `account:true` in the response, and `remember:true` with the
right password adds a `sign_in_token` (32 bytes, base64url without padding). A password
or `sign_in_token` for a name without an account returns 404 `no_account`; otherwise
that name enters as a guest. A renamed account's held old name returns the same 404 to a
password or `sign_in_token`, with `phrases` `AccountRenamed` holding `{name}`, the new
name. Every game the lobby admits shows it and, as for any `no_account`, forgets the
remembered sign-in. A guest under that name gets 409 `name_unavailable`. A lobby without
accounts set up refuses a password with 503 `accounts_unavailable`.

Opening VERA Online offers **Play as Guest** and **Sign In and Play**. Guest entry
immediately requests a session under a temporary name such as `Guest04219`, with no
credentials. If the lobby says the name is in use or registered, the game tries up to
three more names. It keeps the last account username for the next sign-in. The account
path takes a username and password or the remembered sign-in for that username.
The welcome dialog shows the current online player count from `GET /v1/stats` on
entry and refreshes it once a minute while the chooser is open. It shows zero for an
empty lobby and a dash while the count is unavailable. The account form also shows the last 24 hours'
games, omits zero counts, and offers registration, recovery and deletion.
Create Account asks for the name, email and password,
then the mailed code, and signs in. Forgot Password asks for the name and mails its
code, then asks for it and a new password, and signs in. Delete Account accepts the
username or registered email and asks for the current password. After success it
forgets that account's remembered sign-in. Older games send the username as before.
A lobby rollback accepts deletion by username.

A remembered sign-in lives in `Account.json` beside `VERA.INI` as `{name,
sign_in_token}` per lobby, readable only by its owner. The account form shows a masked
password for the remembered username and can submit it without typing the password.
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
ranks. Unless every side is one player, every side has at least a 20% chance against every
other, head to head, weighing each player by their OS. Ranked matches have no AI, every
player is signed in to an account, and the same rosters have not played three rated games
against each other in the last 24 hours.

A host who unticks Ranked game at Create Game sends `ranked:false` on `create`, and that
room's games never rank; its odds line says why. A ticked box sends nothing, as a lobby
from before the choice refuses the field.

A guest's match plays unranked. `ranked:true` in `timing_confirmed` marks a ranked match.
The players of a Quick Match a running season does not rank hear why from `VERA20k` after
it, such as `This Quick Match was not ranked: Guest plays as a guest.`

A `finish` after play adds the game's own `result`: `won`, `lost`, `quit` (the player
left) or `disconnected` (the game lost its connection). A player who leaves after the game
decided their House reports that result: `lost` once it was defeated, `won` once it won. When the game saw the match
decided, `winners` names the players still in it whose Houses won. A lobby from before
team ladders refuses `winners`; the game then finishes again without it. `ended_ago` is how
many whole seconds before it first sends this `finish` the game decided its `result`, which the
score screen holds back, so the lobby times a player's leaving by their game, not by when they
closed the score screen; a retry repeats it. A lobby from before it refuses the field, and the
game then finishes again without it. A desktop game closed during its match or on the score
screen finishes the match as it closes, with `quit` unless it had already decided the player's
result; one closed while the match loads finishes it without a result.

`ladder` with up to 64 names answers with the running season's records of those that
played a rated game in it (`season` is `null` and `records` empty when no season runs),
and the `high_scores` of those that ever played one. Every ranked game, Quick Match, team
game or free-for-all, moves one rating per player, shown as OS: μ − σ, never below 0, as
Beyond All Reason shows skill, 16.67 for a new player. A record's `os` is its player's OS
to the hundredth, `points` it rounded, and `rank` their place among the season's players
by OS, then wins. A high score's `points` pick its player's badge from their OS, as the
game's own thresholds (gamemd `0x00783B80`) read them: Private below OS 18, then one badge
every 4 OS, from Corporal (32 points) at 18 to Five-Star General (1501) at 46, and
Commander in Chief (2001) for the running season's number 1 from OS 46. Its `rank` is
their place this season, 0 when unranked. One lookup fills both of the game's tables: its
ladder table from `records`, and its high-score table, whose points pick each player's
badge, from `high_scores`. The game draws those badges in the lobby's player list and
beside each name in VERA's game rooms, and the help under the pointer on a name adds
the rank and OS. Until the operator starts the first season no one has a rating,
and `high_scores` gives every name asked for the Colonel's 251 points with `rank` 0; a
lobby from before that sends none then. A lobby from before badges sends no `high_scores`, and the game
then shows none; one from before `os` leaves it out, and the game shows the rounded OS.

A game-room join sends the existing `ladder` event immediately before the
membership event in the same transaction. The guest receives the roster's records
before `joined`. Existing members receive the newcomer's records before
`member_joined`. Creation and reopening send the roster's records before
`created` and `room_reopened`. These ladder events have the same retention as the
membership events. The transport delivers them in sequence, so the game already
has the badges when it displays the names.

After a ranked game each of its players hears from `VERA20k` how it counted: their new OS,
their place in the season and the badge the game brought them, such as `Ranked game won:
your OS is now 19.57 (+2.90), rank 1 of 2 in season 1. Promoted to Corporal.`, with why a
loss counted as it did, or why the game rated nothing, such as `This ranked game was not
rated: the players' games disagree on who won.` Every session not in a match then gets the
rated players' records as an unasked `ladder` event, and when a season starts or ends one
with no records and the new `season` (`null` when ranked play stopped). A game that sees
`season` change forgets the records and badges it holds and asks for them again.

The operator starts a season, with a length of 7 to 366 days or none; one with a length
ends on time and the next starts then, as long, unless the operator stops ranked play
after it. An ended season keeps its final standings.

`GET /v1/players/<name>` answers the player profile vera20k.org shows:
`{name,os,badge,season,rank,ranked,wins,losses,disconnects,games,trend,seasons}`. `badge`
indexes the ten badges, lowest first, and is `null` before the player's first ranked game;
`rank` is `null` when they are unranked in the running season, and `ranked` is how many
players it ranks. `games` holds their 20 newest rated games, each
`{ended,season,sides,size,map,mode,teams,won,os,change}`, their own team first in `teams`,
where `null` stands for a player whose account has since been deleted; `trend`
their OS after each of their 200 newest rated games, oldest first, each `{ended,season,os}`;
and `seasons` their final standing in each ended season they played, newest first, each
`{season,rank,total,os,badge,wins,losses,disconnects}`. A name without an account answers
404, unless it is a renamed account's held old name ([Accounts](#accounts)).
`GET /v1/leaderboard` answers the leaderboard vera20k.org shows, renewed once a
minute: `{season,started_at,ends_at,updated_at,total,players,seasons,running}`, `season`
being `null` when no season runs and `ends_at` `null` without an end, `total` the players
ranked in it and `players` the best 1,000 of them by OS, then wins, each
`{name,os,rank,wins,losses,badge}`. `seasons` lists the ended seasons, newest first, each
`{number,started_at,ended_at}`, and `running` names the running season. With
`?season=<n>` it answers an ended season's final standings, with `final` true and its
`ended_at`; an unknown season answers 404.

While a season runs, the host's options for a game room whose players are all on teams,
or in a mode without alliances, give an odds line, which the room hears from the `VERA20k`
notice sender each time it changes: each side's chance to win, such as
`Team A 54%, Team B 46%. Ranked.` (a free-for-all names its players), or `Unranked:` and
why. Quick Match and Cooperative rooms get none. In a game room the host's chat line
`/balance` is a command, not chat.
The room hears its playing humans split into its Teams count (`launch.options.teams`),
or into two teams without one, weighing each player by their OS, and the host's game gets
a `balance` event with every playing human's `team` (0 for A, 1 for B and so on). Two
teams get the most even odds, keeping as many players on their teams as it can. Three or
more take loser picks: from the strongest player down, each joins the
team with the least OS so far among those below the players over the teams, rounded up;
each team then keeps the letter most of its players have, and teams as even as the picks
stay as they are. The host's game moves each named player to that team, as their own team
choice would, and publishes the options. A game that ignores the event leaves the teams
to the host. The host alone hears why nothing moves: a Quick Match room, a mode without
teams, AI players, fewer than two players, or teams already as even as they can be.
Anyone else typing `/balance` hears that only the host can. The host's Balance teams, in
Quick Team Setup, sends `/balance` in the room's chat. A host on 0.3.2 to 0.3.4 with Balance automatically on sends
the `balance` command 5 seconds after who plays or the Teams count last changed; the
lobby balances as for `/balance` but says nothing when nothing moves, and refuses the
command from anyone but the host with 403 `forbidden`. Current games never send it.
The host can also move a player by hand: to another team, into a free seat or to the
spectators. The host's game publishes the options as it does after `/balance`, with no new
field, and the moved player's game tells them in chat.

### Confirming a federated match result

For a match on three or more relays, the lobby can ask the relays to confirm whether
they maintained one command history. Each selected relay must be configured to report
outcomes and listed with `settlement: true` in the lobby's relay fleet. If a ranked
room would use three or more homes and any selected relay lacks that capability, the
lobby selects the ordinary central relay and keeps the room ranked. Matches already
created keep the grants and scoring policy they started with.

The marked `VO1F` roster uses the same table and wrapped link key as an ordinary roster.
Only the per-relay roster authentication domain changes to
`VERA20k relay adjudicated federation roster v1\0` (the final byte is zero).
Released games can carry the signed roster unchanged. The marked relays speak `VF2L`.
The lobby and each home retain the identity of the roster the lobby issued.

Each home sends its immutable terminal receipt to the lobby over HTTPS with
`POST /v1/relay/outcome` and `Content-Type: application/octet-stream`. The 94 bytes are:

| Bytes | Meaning |
|---|---|
| 0–3 | ASCII `VFO1` |
| 4–19 | Match UUID in roster byte order |
| 20–51 | SHA-256 of the entire signed roster, including its wraps |
| 52 | Home's index in that roster |
| 53 | 0 for normal ordering closure, 1 for confirmed federation failure |
| 54–61 | Issue time in Unix seconds, little-endian |
| 62–93 | HMAC-SHA256 under that home's ticket key, over `VERA20k relay federation outcome v1\0` followed by bytes 0–61 |

The lobby returns 204 for an accepted receipt, including an identical retry. A home
can report only one outcome for the match. A conflicting report receives 409.
A relay retries a network failure, 408, 429 or server error within seven days.
The endpoint needs no player credentials. A player's disconnect, concession, quality
report or missing finish cannot establish an infrastructure failure.

One authenticated failure voids the match without changing any player's points, wins
or losses. Normal closure from **every** home releases the ordinary player-reported
result after the match closes. Players can finish and rejoin their room while delivery
is pending. If evidence is still incomplete seven days after the match ends, the
lobby records an unconfirmed result without changing ratings. A delayed finish cannot
apply points to a different season.

Roll out the receiving lobby and reporting relays before marking the fleet capable.
Before a rollback, stop issuing marked rosters and drain marked matches and pending
results. Remove the optional fleet field before restoring a lobby whose config reader
predates it. Earlier lobbies do not rate a protected match from player reports alone.
