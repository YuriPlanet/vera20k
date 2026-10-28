# VERA20k lobby protocol v4

HTTPS JSON only. Authentication is `Authorization: Bearer <token>`; tokens,
relay tickets and packet keys must never be logged. Names are ASCII letters,
digits, `_` or `-`, 1–24 bytes, unique without case among active sessions.
Request bodies are at most 64 KiB and 64 nested containers; every JSON string and
property name must be well-formed Unicode. Escaped unpaired surrogates are rejected,
not replaced, before any event or persistent state is created.

`POST /v1/sessions` accepts exactly `{nickname,protocol:4}` and returns
`{player_id,nickname,token,expires_at}`. Each entry receives a fresh random player
UUID and bearer token. `expires_at` is a Unix timestamp in seconds. An active name
returns 409 `name_unavailable`; entering its name never replaces that session.
Guest locale, buddies and relay admission counters belong to this player identity
and are removed with its session on logout or expiry. Idle lobby visits expire
after 120 seconds without polling; admitted v4 players retain their session during
a network outage until explicit finish/leave or the match/session lifetime.
A later entry gets a new identity and empty preferences. No password account is
created or consulted. Existing stored accounts and their current v3 sessions remain
intact until their ordinary cleanup; active legacy sessions reserve their names too.
`POST /v1/accounts` and password-bearing sign-in requests return 426 `upgrade_required`.
Failed requests return `{error:{code,message}}`; messages must not echo credentials.
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
requests a minute per address, and one answer serves every reader for 10 seconds.

`POST /v1/commands` accepts `{request_id,after,operation,payload}`. A request ID is unique
within a session and retries are idempotent. Responses and
`GET /v1/events?after=<cursor>` return `{events:[{sequence,kind,payload}],cursor}`.
Sequences increase within a session. An event appears once logically even when
responses are repeated. `GET /v1/channels?kind=0|41` returns `{kind,channels}`.
Clients poll at most once per second. Responses contain at most 256 events and
256 KiB of UTF-8 JSON. Critical history is bounded to 1,024 events/2 MiB per session;
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
ask for before joining, otherwise 0. The other fields are presentation metadata,
not authority. `name` identifies a channel uniquely for compatibility with the
existing shell. Member: `{player_id,name,flags,squad,clan_name,clan_tag}`. Flags bit 0
is the channel owner, bit 1 voice; the client adds its own local bit 32768. Servers
set identity and flags from the authenticated membership, never from user payloads.
The only public chat channel is German Bunker (`name:"Lob_41_2"`, `id:"public"`).
The service updates that stored room's name without replacing its membership or
accounts. Other chat channels are neither listed nor joinable; game rooms remain
available through `kind:41`.

Commands and their payloads:

| operation | payload |
| --- | --- |
| `join` | `{channel:<channel name>,key}` |
| `create` | `{channel:<Channel>,key}` |
| `leave` | `{}` |
| `logout` | `{}` |
| `options` | `{options:<HostOptions>}` |
| `choices` | `{row,slot:<SkirmishLocalSlot>,observer}` |
| `ready` | `{ready,revision,version,rules_digest,map_digest,effective_map_digest,staging_digest}` |
| `map_status` | `{status,revision,version,rules_digest,map_digest,effective_map_digest,staging_digest}` |
| `advertise` | `{channel:<Channel>}` |
| `start` | `{users:[name],revision,version,rules_digest,effective_map_digest,staging_digest}` |
| `finish` | `{match_id}` |
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
| `kick` | `{name}` |

HostOptions is `{launch,map_metadata,vacancies,observer_row,cooperative_difficulty,seed}`.
Generated maps add `random_map:{start_staging,preview:{source_sha256,byte_length}}`.
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
| `member_joined` | `{member:<Member>}` |
| `member_left` | `{name}` |
| `options` | `{revision,options:<HostOptions>}` |
| `choices` | `{from,row,slot,observer}` |
| `ready` | `{from,ready,revision}` |
| `map_status` | `{from,status,revision}` |
| `chat` | `{from:<Member>,text,private,action}` |
| `game_started` | `{match_id,revision,version,rules_digest,seed,users:[{player_id,name,flags,source_index}],relay:<RelayGrant>}` |
| `start_confirmed` | `{match_id}` |
| `timing_confirmed` | `{match_id,pings:[integer],relay:<RelayGrant>}` |
| `relay_renewed` | `{match_id,relay:<RelayGrant>}` |
| `map_available` | `{digest,byte_length,resource?}` |
| `start_rejected` | `{message}` |
| `locales` | `{locales:[{name,locale}]}` |
| `buddies` | `{names:[name]}` |
| `located` | `{channel:<Channel or null>}` |
| `kicked` | `{channel:<Channel>,user:<Member>,by:<Member>}` |
| `error` | `{code,message}` |

RelayGrant is `{address,ticket,packet_key,payload_key,source_index,ordering_certificate,federation?}`. Address is `IPv4:port`
of the user's relay in the match. The service chooses it at start and every later grant for that match names it;
ticket is the signed relay protocol ticket encoded base64url without padding and
packet_key is 32 bytes encoded the same way. Credentials are unique to each user
and each new match. `payload_key` is a random 32-byte match payload-encryption key
shared by the participants and encoded the same way. It is delivered only over
authenticated HTTPS, stored only in private match state, and never placed in a
relay ticket or disclosed to the relay. It remains unchanged through bootstrap,
timing confirmation and renewal. The HTTPS lobby is a trusted key distributor:
a compromised lobby could disclose or replace payload keys; relay-blind payload
encryption does not protect against that compromise. `source_index` is the immutable canonical user index, 0–29.
`ordering_certificate` is 66 base64url-encoded bytes: `VO1C`, room UUID16,
source mask LEu32, fixed FPS LEu16, expiry LEu64, then HMAC-SHA256 over the
first34 bytes with domain `VERA20k relay ordering certificate v1\0`. The signed
mask includes all human sources (including observers); AI has no source. FPS
follows launch speed `[60,45,30,20,15,12,10]`. The certificate lasts24 hours;
every UDP control still needs a live authenticated short-lived ticket. The relay
binds the first matching startup agreement to this roster/clock and starts only
when every signed source is ready. Renewals preserve the timeline and batch IDs.
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
A changed publication clears every prior ready/map acknowledgement, and guests
explicitly accept the final displayed settings. Only after the matching options
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
not receive source indices or relay credentials.
Only then does game loading start. Each barrier has a 30-second timeout.
Initial tickets last 120 seconds; timing confirmation supplies a fresh 300-second
ticket which the client must authenticate before loading. During play, clients
renew before their remaining ticket lifetime falls below 30 seconds. The server
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
bound starts to 12 per 10 minutes, chat to 30/minute and 24 KiB/minute, and room chat
to 120/minute. Control payloads are bounded to 4 MiB/minute/player and room fanout
to 128 MiB/minute. Map upload/download bytes each have a 32 MiB/minute player budget.
Canonical options are limited to 24 KiB, choice objects use the exact canonical
slot schema, and unknown request/option fields are rejected. The actor also caps
pending requests/parsed bodies at 128/4 MiB and individual events at 64 KiB. Active
sessions, guest profiles and public-room membership are capped at 256. New guest
entries are limited to 40/hour/IP and 256/hour globally, with the existing sign-in
and ingress limits also applied. These are service admission limits, not gameplay timers.
Clients verify and install into a
local cache, preserve existing installed maps, and still require user readiness.
Generated maps transfer the `.SED` bytes as the map and the separate native PCX
preview with `resource:"preview"` (512KiB maximum). Preview downloads use the
selected seed's URL plus `&resource=preview`; response `digest` names the preview.
Both resources use the same ownership, ordered chunk, hash and cleanup rules.
Generated capacity equals the staged start count (2–8); observers consume only
transport slots. Start admission also compares the canonical staging digest, so clients cannot
agree on seed bytes while using different generated start cells.

The existing shell leaves its game channel during the successful start. The service
therefore retains the match admission independently of channel membership until
finish/timeout. Postmatch the shell rejoins chat and may create or join another game;
this is a fresh match with fresh relay credentials. A channel owner leaving before
start closes the room and emits a failure/leave event to the guests. Membership,
authorization, readiness, sequence history and idle expiration are server-owned.
Idle lobby visits expire after 120 seconds. An unfinished v4 playing membership
survives that idle interval until explicit finish/leave or the match/session
lifetime; lost internet does not remove its gameplay source.

`POST /v1/translate` accepts exactly `{text,target}` from a v4 bearer and returns
`{text}`: `text` translated into `target`, a language tag such as `en` or `zh-Hans`.
`text` is non-blank and at most 2048 UTF-8 bytes, the chat limit. Clients translate
received chat only after the player opts in, and always keep the original visible.
A deployment without a translator, a failed model call or the service's daily budget
of 10,000 model calls returns 503 `translation_unavailable`; an unsupported target
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
return `{notice,delivered}`: the notice `{id,text,expires_at}` or `null`, and how
many sessions received it at once. A notice is an ordinary public `chat` event from
`{player_id:"00000000-0000-0000-0000-000000000000",name:"VERA20k",flags:1,squad:0,clan_name:"",clan_tag:""}`.
Sessions in German Bunker or a game room that has not started receive it when it is
set; others receive it on their next German Bunker join until it expires. Each
session receives a notice once. Sign-in refuses `VERA20k` without case with 409
`name_unavailable`.
