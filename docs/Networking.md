# Multiplayer behavior and ownership

Windows, macOS and Linux builds provide LAN and online multiplayer through
the normal game menus. Both paths use the existing
owners for session setup, synchronized commands, reliable UDP delivery, frame
pacing, checksums, disconnects, results and rematches.

Legacy IPX sockets and the IPX protocol are excluded. Shared behavior in
IPX-named classes remains relevant. Retail's Network menu constructs the IPX
interface (`52E3D2`); VERA uses IPv4 UDP for this LAN entry point. That replacement,
including limited broadcast on port 1234, is an explicit transport adaptation.
Online play uses the Cloudflare HTTPS lobby and authenticated UDP gameplay relays;
the lobby gives each match a relay near its players, or spreads it over several
relays when they are far apart. Relay admission handles client source-address changes;
the relay orders modern online commands while clients retain simulation authority. Matches are between
VERA20k clients; legacy IPX and gamemd wire compatibility are not supported.

## Current production path

The Network menu enters the LAN lobby. The physical service handles discovery,
join, options, roster, readiness and GO, then transfers the same socket pair into
the running match. For online play, choose Internet, enter a username and
press OK. Names use 1–24 ASCII letters, digits, underscores or hyphens;
each active player needs a different name. No password or account creation is
required. Players then create or join a game. A host can
select Easy, Normal or Hard AI from an open player slot in Battle or Free For All.
The HTTPS service supplies match-specific relay credentials before the same loading
and command handoff. Both paths transfer selected authored maps or generated
seed/preview data and verify build, rules and effective-map agreement before start.
Games with two or more human players number their objects from one agreed start, so
a player whose game already ran the random-map dialog or an earlier match numbers
them as one just opened does; nothing new is exchanged, and older builds are
refused by the build check.
Whether Ares plays in a match is decided from that same data: the rules, mode
and map, whether the selected mod loads `ares.mix`, and the player's
[Ares setting](ares.md#the-ares-setting), which the rules agreement covers. With
**No mod** selected and the setting on Auto, the installation's `ares.mix` is ignored and
the game keeps the original archive order and INI readers. A selected mod
can load its own `ares.mix` or use the installation's copy; that game's INIs and
rules are read the Ares way. Without it Ares plays only
from Ares keys in the rules, mode or map, read the stock way ([Ares](ares.md)).
A game loading `ares.mix` adds one byte to the rules digest, so its digest differs
from the same files read without it; a game without it adds nothing.
Players on one build reach the same answer. Older builds, including those that
load an installed `ares.mix` with No mod selected, are refused by the existing
build check. A rollback restores that build's earlier selection and INI reading.
Ares needs no build feature: the build check's version keeps a 0 where the retired `ares`
feature's flag was, and the rules digest still hashes `false` in its place, as a released
game's do; nothing exchanged changes.
Whether Phobos plays is decided from the same rules, mode and map
([the Phobos switch](phobos.md#the-phobos-switch)) and adds nothing to the
digest, so players on one build reach the same answer; older builds are refused
by the build check.

The rules agreement is one digest of the process's rules sources
(`ProcessAssets::multiplayer_rules_digest`). With **No mod** selected, the
optional `langrule.ini` is neither loaded nor included in agreement or changed-file
checks, even when it comes from a language archive. Selected mods retain that
layer and compare it. Translated `.csf` text still loads. Mixed builds are refused
by the existing build check; a rollback restores that build's LANGRULE selection.
The agreement also covers each theater's INI as the theater
loader reads it (`map::theater::theater_ini_names`: the file
[`theater_suffix`](ini-roots.md) selects, else the stock name such as `snowmd.ini`),
whose tilesets decide each cell's land and height. It also covers the theater tiles the
game folder changes (`map::theater::changed_tiles`, found once per process): by name and
content, each tile file a theater reads whose bytes differ from the ones a retail install
reads, variants included, since map loading picks each cell's variant by how many its tile
has. A theater MIX that takes a retail one's place counts even if it does not open. With a
mod in use ([modding guide](modding.md#the-mods-folder)) it also covers every file in the mod's
folder but `mod.ini`, `veraui.ini` and `mod-files.txt`, by path and content, so players whose copies of a mod differ are
refused as on any rules difference. A mod that adds INI files to every map
([modding guide](modding.md#add-to-every-map), `app::mod_additions`) also adds those files
as the game read them for its matches, once per start, so a player whose file changed
after that is refused rather than playing different maps; older builds are refused by
the build check, and a rollback plays them as an ordinary mod's files. The files are read once, starting when the player
opens Network or Internet, and nothing more is exchanged. A game from before the mod
files, the theater INIs or their tiles were compared is another version and is
refused by the build check first; a rollback returns to the earlier digest for everyone
on that build. Quick Match plays the original game, and the matchbot pairs without the
rules hash, so a client ends its search before it queues with a message saying what to
turn off: a mod in use; `[ini]` roots or a `theater_suffix` in config.toml other than
YR's; an [Ares setting](ares.md#the-ares-setting) other than Auto; game-folder files that change a compared INI (Rules, Art, AI, MPModesMD and the mode INIs, CoopCampMD,
the theater INIs) or a theater's tiles from the retail copy; a copy from a retail
archive (`langmd.mix`, `language.mix`, `expandmd01.mix`, `ra2md.mix`, `ra2.mix`) counts as
the original, another INI is compared with the retail archives' own entry, else
`ra2md.mix`'s `localmd.mix` (`retail_sources::files_changing_shipped_inis`), and another
tile with the one a retail install reads (`map::theater::changed_tiles`), both found at
startup; the message names three and counts the rest, which the log lists. Personal
rules, Terrain Deformation and the Superweapon Limit need nothing: Quick Match's room has no
[room rules](#room-rules), so its match plays without them. A difference none of
these catches still meets the guest's admission: after `WOL:MatchErrorJoiningGame` its
status page says the games differ (`VERA:MatchStartMismatch`), the log records both
hashes, and the search goes on after the stage timeout; the host only times out. This
adds nothing to what game, lobby and relay exchange.

The online lobby's Options page has a Translate Chat Into list: Off, or one of 14
languages, saved as `[VERA] TranslateChat=`, and under it a Translate in Matches box, on
by default and saved as `[VERA] TranslateMatches=`. With a language chosen, other
players' chat in the lobby, game rooms, private messages and, with the box ticked,
matches is sent to the lobby's translation service when it needs it: for a Latin-script
language a line with any letter outside it, for any other language a line with none of
its letters. A reader whose script another listed language shares, English readers
excepted, also sends the lines in that script, since they may be another language. The
original stays; its translation follows as a line from the same sender marked `»`,
without the service's bracketed note naming the source language, unless it only repeats
the original or the service says it was already in the chosen language.

The app services reliable messages and intermediate progress while loading, and
continues servicing transport during frame waits. Gameplay uses the same
OutList/DoList owners as offline play; the transport does not own a second
simulation command queue.

Online games admitted with exactly one human use local command scheduling: commands
run at the next dispatch opportunity without a multiplayer lead-time buffer. The
first simulation frame still primes the existing network queue. Game-speed and CPU
timing continue through the same timing owner. Games originally admitted with
multiple human sources, including observers, retain synchronized scheduling even
after other players leave. AI players never acquire network identities.

Online matches with two or more human sources use a lobby-certified relay clock
and one immutable relay command stream. Every originally admitted client that
finishes loading must agree to the initial world before it starts. A player who
never finishes loading leaves in the first frame once silent for 60 seconds, or 3
minutes after the first client was ready; the native loading timeout leaves that
departure to the relay. A match spread over several relays is cancelled instead.
At each deadline, missing input is empty; a command joins the next unsealed
frame. The sender also waits for that commit.
Frames use the existing Event codec and simulation queues, with a fresh compression
cache per self-contained batch. Batches travel in plain text: the relay can read
them, and only the per-hop MAC authenticates them. A batch's Event packet header
is not sent; each client rebuilds it from the batch's source and frame. A batch
that does not decode, or whose House is not its source's, makes its sender leave
at that frame on every client, as if by Leave; the rest of its stream is ignored.

Redundancy follows each player's measured link
(`services/relay/adaptive-transport/spec.md`). One merged UPLINK request carries
unacknowledged batches, the receive watermark, the per-hop receipt feedback,
checksums and pace. The relay pushes each turn packed, with the reader's
unacknowledged records piggybacked into the space the turn leaves. A good link
sends each batch and turn once. A lossy or jittery link also gets 16 ms uplink
copies and 8 ms downlink repeats. A congested link gets neither, and a silent
one only probes. Losses that the receipt feedback shows are retransmitted by the
relay; a lasting gap falls back to the existing replay reader. Clients and relay
ship together: older clients are not supported.

Every datagram uses the compact envelope
(`services/relay/bandwidth-compaction/spec.md`): a 32-bit session the relay
assigns at admission, a 32-bit hop sequence and a 4-byte truncated HMAC, 13 bytes
in all. History packs encode each record relative to the previous one and decode
alone. The receiving client rebuilds the same canonical records, so journal,
replay and federation are unchanged. Measured traffic is in
`services/relay/adaptive-transport/results.md`.

When every active player and the relay support synchronized pause, any player
can use the Pause key or Options' **Pause All Players** / **Resume All Players**
button, or type `/pause` or `/resume` as a whole line in match chat. Such a line
goes out as chat to its recipients like any other, so they read who asked; in a
match without synchronized pause it answers that pausing is not available.
A pause marker follows the relay's last committed frame; clients finish that
same frame and stop. Resume uses a separate authenticated control, so it works
while simulation ticks are stopped. Both markers use the normal ordered
history and replay recovery, and each names the player who asked: once a
client's world reaches one, the other players and watchers read "<name> paused
the game." or "<name> resumed the game." A rejoining game shows only those its
world reaches after catching up. Connections, chat and ticket renewal keep
running; accepted gameplay inputs wait for their assigned frames after resume.
Opening Options alone remains local. After a resume nobody can pause again for
30 seconds. Confirming Leave resumes as needed until the queued EXIT executes.
Shared paused time is excluded from the score-screen duration.
This feature requires updated clients and relay;
it is unavailable in a mixed-version room or a direct LAN match.

Pushed clients use a local adaptive playout schedule based on socket receipt
timestamps. It starts with no added buffer and can reserve at most 20 ms or one
frame, whichever is smaller, to absorb jitter. An early receipt moves the
schedule earlier, so no hold exceeds that reserve. Stable delivery gradually
reduces the reserve. The relay still owns the clock; local pacing never changes
commands, simulation order or another player's progress. A stalled or slow client
catches up through at most eight ordinary simulation transactions or eight
milliseconds per service wake, then yields to input. It draws once no frame is
due, so a client that draws slower than the match rate shows a steady number of
frames per picture instead of falling behind and catching up in bursts. A
picture stands at most four recent draw times (at least 50 ms): the simulation
may take the four fifths of the time the relay expects a player's tick cost to
hold. It never skips simulation ticks, and pacing never interrupts an
already-started frame transaction.
An agreed resume resets the local playout clock so paused time creates no catch-up debt.

A player whose PC cannot keep up slows the match, within limits. Each client
reports about once a second whether it is catching up (frames have waited on it
for two seconds) or behind (also, two seconds in a row, it ran some but fewer
than the relay sealed). A hidden window, a paused or rejoining match and a world
that ran no frames never count as behind. The relay then seals at the fastest
Game Speed rate at most nine tenths of what that player ran, so it also drains
the frames waiting on it: at least one row slower but never more than two rows
below the match's own speed (60 frames a second goes no lower than 30), with at
least five seconds between changes. After 30 seconds with nobody behind or
catching up, not counting paused time, it tries one row faster; a slowdown
within 30 seconds of that doubles the wait, up to five minutes. Every client
shows "<player> is slowing the game down." and, once the match is back at its
own speed, "Game speed is back to normal." A rate change is a marker in the
stream, so only playout pacing changes: the simulation, its saves and replays
keep the match's own frame rate, and a replay plays at that speed. It needs
every active client to report; a room with an older client or one spread over
several relays keeps its speed. Native lockstep instead slows every match to its
slowest player (ProcessNetwork `0x00647BF4..0x00647E6D`).

The relay keeps a bounded RAM cache and the full match journal on disk. The
journal is not encrypted: it holds plain commands and relies on file permissions. Temporary connection loss keeps the player's identity, units, input cursor
and world in the match; the client renews its ticket and reads missed frames when
connectivity returns. The UI shows Reconnecting or Catching up while healthy
clients continue. Once the relay has heard nothing from a player of a started
match for 10 seconds, the other players read "<name> is not responding.", and
"<name> is back." when it hears them again; watchers read neither. The relay
names silent players in each player's status (relay README "Silence"). A
released game ignores the names. An older relay names nobody, so a game on it
shows no such line; in a match spread over several relays, players homed on an
older relay are never named.
After ten minutes of silence the relay removes the player
through the same stream. A game that has had no answer from the relay for that
long, and 15 seconds more, releases its relay route and sends it nothing more:
its place is gone, and a game whose ticket lapsed gets no answer that says so.
A match of two or more players then ends with the game-server connection
message; a one-human match plays on without the relay. Like a native lost connection, a removal for silence
destroys the player's House and shows "connection lost"; a player who quits still
leaves through EXIT. While Reconnecting, the relay cannot sequence that EXIT, so
a confirmed Abort Mission returns to the shell once the relay has been silent for
three seconds; the client's relay Leave, or else that silence, removes the player. Silence that long, explicit Leave, joining another online
match on the same relay, or the match's final frame after 24 hours of play ends
membership. The relay
remains required for progress.

A process that left a match without Leave (a closed window or browser tab, a crash)
can be replaced within that silence limit. The launcher receives a fresh `[Relay]`
section with `ResumePings`, the timing vector every client started with, and
`ResumeCursor`, the end of the lobby events that process already consumed. The new
process skips the start barriers, loads the same world without reporting progress to
the others, and reads the whole stream from sequence 1 as ordinary catch-up. It
learns its batch counter from that stream, so it submits nothing until the world has
caught up; local orders given until then are dropped, and effects and voices stay
silent. The relay refuses a different world as an agreement mismatch. Abort Mission
during this replay leaves as it does while Reconnecting.

Authored matches, Cooperative aside, record reconstructible JSON replays in `replays/`,
offline skirmishes against computer players included.
A replay is written when its match ends or the game closes, and also when an
internal error (a panic) closes the game; the folder keeps the newest 20.
`vera20k --replay <replay.json> --ra2-dir <retail-directory>` opens read-only
playback with camera controls and Escape to close.
`vera20k --replay-verify <replay.json> --ra2-dir <retail-directory>` reconstructs the
accepted launch and replays the production frame transaction without connecting
to the lobby. Startup version, rules, map bytes, seed and initial state must match;
recorded state hashes are checked during playback. Generated maps, campaigns and
restored mid-match segments currently lack standalone replay startup; a skirmish or
network match that cannot be recorded says why in the log. `--replay-verify` loads no mod,
so it refuses a match played with one as a rules mismatch.
Replay version 4 regenerates shared planning events through the simulation at
their original frame phases. Earlier JSON logs remain readable for inspection;
playback requires the current version to avoid executing old internal events twice.

| Responsibility | Authoritative owner |
|---|---|
| Pending and scheduled simulation commands, native event dispatch | `sim::command_queue`, `sim::world::network_commands` |
| Event compression/decompression and persistent caches | `net::event_packet` |
| Packet construction, cumulative counts and peer frame progress | `net::command_exchange` |
| Shared reliable send queue, retransmission and response-time history | `net::reliable` |
| Global application message admission, acknowledgements and retirement | `net::global` |
| UDP sockets, datagram checksum and retained raw send attempts | `net::udp` |
| Startup agreement, native send cadence and command wait | `net::live_commands` |
| Certified online clock, immutable command history and bounded push/replay delivery | `vera20k-relay-protocol` |
| Local adaptive playout and bounded catch-up presentation | `app::match_runtime::frame_pacer` |
| LAN packet layouts and options parsing | `net::lan_messages` |
| Participant membership, admitted endpoints, launch state and checksum history | `app::match_runtime::session::MatchSession` |
| Discovery projection and physical LAN lobby/setup service | `app::lan_service` |
| Online session, lobby and relay-admission adapter | `app::shell_wol::live_service` |
| Shared map source, generated staging and verified transfer admission | `map::source`, `app::shell_network_setup::maps` |
| Setup UI, map admission and pending handoff | `app::shell_network_setup` |
| Production frame boundary and post-Logic continuation | `app::match_runtime::sim_tick` and `sim::world` |
| Connection dialogs, votes and participant cleanup | `app::match_runtime::network_match` |

Simulation never reads a socket. The app lends the transport-backed command
boundary to the simulation at the established multiplayer queue phase. A pending
network wait saves the post-Logic continuation; resuming it does not repeat Logic
or input admission. Presentation and local input state do not select the shared
simulation update order.

The service retains admitted setup traffic in the global receive queue during
the asynchronous lobby-to-setup transition. Loading retains the frozen setup,
so that transition guard does not block loading progress. Row bindings and
participant membership remain distinct: replacing a human row sends the native
kick, while its departure removes the admitted participant and endpoint.
In VERA's rooms the host's own kick also keeps that address and name out for five
minutes: their joins get the VERA-only denial reason 9, shown as the join ban.

A VERA room's Teams count, which only its host sets, travels with the host's options:
online as `launch.options.teams` ([lobby protocol](lobby-protocol.md)), on LAN as
`teams` beside VERA's room options in the extended options. With it set, the host
keeps every seated player on one of the room's teams, so guests only show what the host
sends. A LAN guest from before Teams
ignores the field and is refused at the build check. Online, an older guest cannot
read the host's options while Teams is set and says the update was invalid, in a room
that cannot start, since its players' versions differ. A lobby from before Teams
refuses the field; the host's game then turns Teams Off, says so and sends its options
again without it. A room has 2 to 15 teams, named A to O.

In a VERA room whose mode has teams and Teams set, people play only on a team. A newcomer
comes in watching while a spectator seat is open and takes a seat with a team header's
Join; a guest who picks no team watches instead, and with no spectator seat open the
host seats a newcomer on the team with the fewest players. With Teams Off the room is
one box without a caption, free-for-all, and seats newcomers as gamemd does: turning
Teams Off takes every player and AI off their team, the Team lists close and the host
keeps a guest's team pick off a team. Nothing new travels: guests send their picks as before, and a released guest, whose
team headers have Join too, takes a seat the same way. An older host still seats a
newcomer on no team; a newer guest lists that player under a No team header without
Join. A rollback seats newcomers as before.

With Teams set the map does not limit who plays, only the room's Max players does, so no
team is ever full: a team's people share its bases where the map runs out of starts
(`MatchSession::network_shares_bases`). People and AI past the map's seats sit in the
room's spare rows. A VERA room's LAN options carry all thirty AI rows on any map, where
gamemd's text carries eight for a map of up to eight players; a guest of another build is
refused at the build check. At Start Game, Team Starts gives
each team a start and the spare starts to the teams with the most players for each; a
team's people on one start share its House, as players who pick the same start do, and
each AI there starts in its own House beside them, its units on the cells nearby
(`SkirmishLaunchSession::shares_bases`). The map then needs a start for each team.
Turning Teams Off, or picking a map or mode with fewer seats, makes the people past the
seats watch rather than leave the room, and the AI there leave. Nothing new travels: the
host's options carry each player's row and start as before, and a room of mixed versions
cannot start. Online the lobby checks the map's starts against the bases the room needs
([lobby protocol](lobby-protocol.md)). A lobby from before that refuses the host's options
once more people play than the map has seats; the host's game then turns Teams Off, says
so and sends its options again, and the people past the seats watch. A rollback seats no
more players than the map has starts.

AI play in a VERA room while people watch, unlike in gamemd's room (`0x5EC9F0`'s
TXT_NO_AI_WITH_OBSERVER, and WOL's TXT_NO_AI_OBSERVATION at Start Game): the host keeps
the spectator seat open with AI in the room, and a guest may watch beside them. Nothing new
travels. A lobby from before this refuses to start such a game, and the host's game says
the match could not start. An older host keeps gamemd's rule, and a rollback keeps it
again.

A VERA room's Max players counts everyone in it, players and spectators, at most its
thirty rows less its AI. Its host sets it at Create Game and changes it in the room with
the team bar's Room size, which the host's game advertises again to the lobby; a lower
size sends no one away, it only keeps newcomers out. The host may watch too, with the
Spectators' Watch or by dragging its own row there, and plays again with a team's Join,
as a guest would; someone else, or two or more AI, must play for the match to start.
Nothing new travels. An older host keeps its Create Game Max players and always plays,
and a rollback does again.

Everyone in a VERA room may watch while two or more AI play, a match gamemd's room never
starts. It runs until one AI, or one team of them, is left: no spectator's defeat
decides it, and the AI left do not band together when one falls (`[AI] Paranoid`). Its
spectators look on from one start, which the first AI then takes, so the map needs a
start for each AI. The lobby places its relay by the people watching and does not list
it to watch, as a watcher's game follows a player. Nothing new travels. A lobby from
before this refuses to start it, and the host's game says the match could not start; an
older game keeps its rule that someone plays, and a rollback does again.

Games 0.3.3 and 0.3.4 had a host setting, Balance automatically, that travels online as
`launch.options.auto_balance` and had the lobby balance the room 5 seconds after who
plays or the Teams count changed. Current games neither offer it nor send it. In a room
whose older host has it on, a current guest still closes its seated players' Team lists
and team headers' Join (a spectator's Join still asks to play, and that host's balance
places it), as that host's own game does. A released guest in a current host's room
sees the setting off; the host's Balance button still evens the teams.

A VERA room queues its spectators for seats. A spectator who asks to play while no seat
is free, with a team header's Join Queue or, in a room without teams, as a newcomer who
prefers to play, joins the end of the host's join queue; Leave Queue, or picking to watch again, leaves it. The host's
game owns the queue as it owns seating: when a seat frees (a player leaves or starts
watching, or the host adds a slot or removes an AI), it seats the first spectator in the
queue as its own Seat would and publishes its options. The queue's order travels online
as `launch.options.queue` and on LAN as `queue` beside VERA's room options. Every game in
the room lists the waiting spectators first, each with its place before its name, and a
waiting player's own game says its place and when a seat came free. As with Teams, an
older guest is refused at LAN's build check and online cannot read the options while
anyone waits. A lobby from before the queue refuses the field; the host's game stops
queueing in that room, says so and sends its options again, and its spectators keep
watching. A rollback past Teams then turns Teams Off too.

A room's Create Random Map is Skirmish's whole generator: up to 30 players, the sizes
past Very Large, the added landform and theaters, Advanced, Fair Starts, Extras, Presets
and Export Map. A map for more than eight players stages one `W` slot for each start
(`ScenarioStartStaging::to_w_text`), so its record runs past the native 37 numbers and
512 bytes, up to 2048 (`maps::checked_staging`); it travels as any random map's does, on
LAN in the extended options and online as `random_map` ([lobby protocol](lobby-protocol.md)).
The generated rules digest covers VERA's town catalogue and `[Extras]`, which each game
resolves from its own files, so players whose games would build different towns are
refused at Start as on any rules difference. In a game read the Ares way (Ares.dll
loaded: the mod ships `ares.mix`, or the Ares setting is On) the seed's effective digest
also covers its `UrbanAreas=` key and the generated rules digest Ares' `[Urban]` lists;
the rules agreement already covers which way each game
reads, so nothing new is exchanged. A guest reading the other way accepts the host's
seed bytes and is refused by that agreement, whose text names the Ares setting. A
preview larger than the 512 KiB preview
transfer, as Enormous and Colossal maps make, is written at half size until it fits.
A LAN guest of another build is refused at the build check. Online, an older guest
cannot read options carrying such a map and says the update was invalid, in a room that
cannot start, since its players' versions differ. A lobby from before these maps refuses
options carrying one with 400 `invalid_options`; the host's game says so and keeps its
other options until the host picks a smaller map. A rollback of the game
returns rooms to the eight-player dialog; one of the lobby refuses these maps again.

The host of a VERA room can also set a seated guest's side, colour, start and team from
the guest's row. The host's game checks the change as it checks the guest's own pick and
sends it in its options, as `/balance`'s moves travel, so nothing new goes over the wire:
a released guest shows it as it shows any row the host sends, and an older host just
doesn't offer it. A guest's own pick sent at the same moment carries its whole row and
wins on the host. Online, a game sends its room commands to the lobby one at a time; a
newer options, map-status or ready command replaces an older one of the same kind that
is still waiting, and goes out after whatever was queued before it.

### Room rules

A network room plays its host's rules (`skirmish_launch::HostRules`): the host's
Terrain Deformation box and [Superweapon Limit](superweapon-limit.md). They open as the
host's skirmish Game Options have them and the host can change them in the room's Game Options. They travel with the host's options, online as
`launch.options.host_rules` ([lobby protocol](lobby-protocol.md)) and on LAN as
`host_rules` beside VERA's room options. Every player's game, the host's too, plays
them in place of its own from the next scenario (`ProcessAssets::use_personal_rules`).
The switches compose over the root each scenario reads, never over the
cold start or the shell's Country reads, so every player's one rules registry is the
same whichever rules a room plays. Offline games play the player's own, a network
replay the room's it recorded. A Quick Match room has none: the guest's game drops
any its host sends, and the lobby refuses them there. A launcher's network game
(`tools/spawn`) has no host to share them, so each player plays their own, which its
`ExpectedRulesDigest` compares. So the room's door, the native rules hash (LAN's join
check, online the room's `ex_info`), leaves the switches out, while the rules
digest compares the ones each game plays. When the door
still refuses a join because the rules files differ, or the digest a start, each game
says what it changes from the original game, its mod, `config.toml`'s INI roots and
changed game files, or that it plays the original (`agreement::rules_differ_text`);
retail's text for it sends the player to Westwood. Nothing more is exchanged for
that. A game of another build is refused at the build check first, so no released
game reads `host_rules`; a lobby from before it refuses the field with 400
`invalid_options`, and the host's game drops the room's rules, says so, and its room
plays without them. A rollback past it plays every room without them. The Superweapon Limit came later, as
`host_rules.superweapon_limit` (2 to 20): an older guest cannot read options that carry
it, in a room that cannot start as its players' versions differ, and an older LAN guest
is refused at the build check. A lobby from before it refuses the field; the host's game
turns the limit off, says so and sends its options again with its other rules. A
rollback past it plays every room without it, as offline games then do.

The player's [Ares setting](ares.md#the-ares-setting) (`[VERA] Ares=`, or a launch
file's `[Settings] Ares=`) is not a room rule: like the mod, each game reads it at start
and plays its own. Auto adds nothing to the rules digest, so a game set to Auto agrees
with a released game as before. On makes every game an Ares game, which adds the byte a
mounted `ares.mix` adds, and Off adds its own words, so a player whose setting differs
from the others' is refused at the start, and that game's text names the setting
(`agreement::own_changes_text`). Nothing new is exchanged. A rollback reads no setting
and plays Auto.

Until VERARULES.INI was retired for [New Mod](modding.md#start-a-new-mod), `host_rules`
also carried the host's file as `personal_rules`. This version never sends it, and
refuses a room or a replay whose `host_rules` still has it: an older host's room is
refused at the build check before that, and an older replay plays without it and fails
its rules check. A guest of an older version is refused at the build check. The lobby
still accepts the field, so a rollback needs no lobby change; a rolled-back game reads
`VERARULES.INI` again.

### Watching

A player in VERA's online lobby can watch a match being played (VERA's; gamemd has
no such thing). The game list shows running matches after the rooms, tagged Watch,
with how many watch each out of the 10 it takes; Join on one asks the lobby to `watch`
it ([lobby protocol](lobby-protocol.md#watching)). The lobby lists only matches of the
watcher's own version between two or more players, on a relay that lets watchers in,
and never one from a room with a password, a clan room, a generated map or a co-op
mission: a watcher's game cannot yet rebuild their starts. A host chooses in Create
Game who may watch the room's games: everyone 2 minutes behind (the default), nobody,
or everyone live. Quick Match and ranked games are always watched 2 minutes behind.

The lobby's grant carries the match's start, its options and a watch ticket. The
watcher's game downloads the map as a guest does, checks that its version and rules
digest are the players', and joins the relay as a watcher (`live_service::begin_watch`,
`poll_watch`): a read-only session that pulls the match's stream from its first frame,
held back by the delay, and sends nothing into it, not even a checksum
(relay README, "Watchers"). It loads the world as a returning player does, with no
progress reports and nobody waiting for it, and then plays the stream's frames. Until
it reaches the match's delayed present it runs them at speed, in passes of up to 50 ms
a picture where a returning player's stop at 8 frames or 8 ms, says Catching up, and
keeps its sounds off. It keeps every frame it has read rather than a returning player's
64, so a long match is read to its end before the relay forgets it, 300 seconds after
the end. It checks its world against the checksums all players reported
alike and stops with the usual message when they differ.

A watcher stands in one player's seat: that player's sidebar, credits and sounds, with
the whole map shown. Only the picture changes: every House keeps its own fog in the
shared world. It sends nothing in that seat either (`Simulation::install_watcher`): the
followed player's records are another game's to it, so their Options opens nothing and
their leaving ends the watch no more than it ends the other players' match. Tab follows
the next player still in the match, in House order, also while a frame waits on the
stream (`match_runtime::watching`): online the current House only chooses this game's
view and result, so following rebuilds just the objects' owner and discovery bytes from
the history every player's game keeps (`Simulation::follow_house`). After the last
player, Tab shows the observer's sidebar as an observer in the room sees it: each
player's name, kills, units and money, and the time played on the credits counter
(`Simulation::observe`). Only the sidebar changes: the world keeps the last player's
House, so the sounds, radar and end of the match stay that player's. The next Tab
follows the first player again. A watcher gives no
orders: its input is the camera, zoom, Tab
and Escape, and it has no chat. Escape leaves at once; the returned transport's relay
Leave ends the watcher's session. A match that ends with a result ends for the watcher
as for the player it follows, with the score page, then the online lobby. One that every
player leaves without a result returns it to the lobby with a line saying the match
ended. The lobby renews a watcher's relay pass as it does a player's (`renew_watch`).

Older versions and rollbacks: a game from before watching never asks for the list, its
rooms send no `watching`, and their matches are never watchable. A watcher sees only
matches of its own version, and its game refuses a start whose version or rules digest
differs (Could not start watching this match). A lobby from before watching, or rolled
back to it, answers the list with 404, which the game takes as no matches and as a lobby
that takes no `watching`: a room's options then leave it out. Before the list's first
answer a room sends it, and an old lobby refuses those options: the host's game stops
sending it, says so, and the room plays unwatched. A lobby that lists matches refused
something else, so the room keeps its Watching and the game asks for the list again; a
404 then, after a rollback, stops the room's Watching the same way. A grant that comes
after the player gave up waiting for it is handed back (`unwatch`). A relay rolled back
to before watchers refuses watch tickets: a new watch
ends in the lobby with the relay connection message, and a watcher already on it loses
its connection and returns to the lobby with the usual message.

## Native behavior and deliberate bounds

The following native scheduling/reliability details apply to LAN and legacy routing;
modern online ordering above is an intentional adaptation.

- Event packets use the native layouts and persistent compression caches from
  `64B4D0`, `64B660` and `64BDD0`. The packet boundary rejects a semantic event
  without a supported raw command producer rather than inventing a wire form.
- `649CA0` mirrors the local OutList before broadcasting it. Send failures do not
  undo that mirror; the reached native wait and recovery paths decide what
  happens next. Transport queues retain bytes, not gameplay decisions.
- Private and global connections share the `48C3E0` send/retry owner. Global
  admission and FIFO retirement preserve `53FCB0`, `53FF10` and `5400D0` behavior.
- Ordinary acknowledgements use the primary socket even after an alternate-socket
  receive. The native exception is the separate CDEF router connection, which is
  not implemented here.
- Raw UDP send attempts follow `7B23B0`/`7B3AA0`: hard errors retain the exact FIFO
  head; WouldBlock retires that raw attempt. The reliable owner still controls its
  retry. Portable polling supplies write opportunities in place of Win32 events.
- LAN pre/post-GO timing preserves the distinct host and guest paths, including
  conditional primary-global response-time reads and signed versus unsigned
  arithmetic. Guest GO completion gives the primary global channel its separate
  timing. After gameplay peers are installed, `52EA18`/`52EA4F` select independent
  private and primary-global retry intervals.
- The initial successful player barrier resets response history through the
  shared `48BAA0` owner. This preserves pending traffic, original send timestamps,
  sequence counters and receive history. WOL's periodic reset scheduling and its
  additional channels remain outside the current LAN path.
- Ordinary waits use the private response maximum from `542450`; primary-global
  acknowledgement history is excluded. The native optional extra channel at
  manager `+50` remains an unimplemented dependency.
- The LAN application message's name field holds 19 bytes and a terminator. An
  online name may be 24 characters, so VERA writes the first 19 bytes of a longer
  name and receivers match a sender by that same prefix (together with its
  endpoint, as before). LAN names are at most 19 and travel unchanged.
  A LAN name is the saved player name's bytes: gamemd copies them into each packet
  (`5DC6A6`) and shows them one byte to a character (`735120`), so every player,
  the sender too, sees the same name, and a name beyond ASCII shows as its bytes.
  The packet is unchanged. Older builds on Windows sent other bytes for such a
  name than they used themselves, so a game they hosted could not be joined;
  different builds meet only in the lobby, where such a name now looks different.
- A player's colour travels as the number the lobby lists it under: 0 to 7 for
  gamemd's eight, 8 for an observer and 9 to 30 for VERA's
  [extra lobby colours](skirmish-colors.md). The LAN fields and the online slot
  already carried that range, so no packet changed, and a room keeps each colour to
  one player while it lists at least as many colours as it has players.
- LAN chat in the lobby and setup rooms carries gamemd's fixed chat colour,
  `0x0000F0F0` (`5DE360`), not the player's colour number; match chat carries the
  number, and co-op's score screen the fixed colour. Different builds meet only in
  the lobby, where older ones already sent the fixed colour; they show others'
  lines as `name: text` and their own lobby lines twice. The packet is unchanged.
- Setup Back retains transport history within a lobby visit. A fresh post-match
  visit resets queues, sequence history, observations and lobby timing while
  retaining the portable socket bindings.

## Reproducible validation tools

The `tools/storage_oracle/lan_*` harnesses execute original instruction slices
for the packet, checksum, queue, timing and message controls their Rust tests
consume. Metadata states the input boundaries and substitutions. These small
corpora establish only their covered cases.

The `lan-live-host` and `lan-live-guest` shell-capture targets drive independent
App processes through the ordinary LAN service and gameplay frame boundary.
They require isolated mutable copies of the retail game directory, each with a
`.vera20k-lan-capture-root` containing `VERA20k isolated shell capture fixture`
followed by a newline. Set each process's `RA2_DIR` to its own copy and keep its
own `config.toml`.

After building the normal app with `--profile fast`, run the host with
`--shell-capture lan-live-host --width 800 --height 600 --cursor-x 400
--cursor-y 300 --output <new-host-directory> --lan-bind 127.0.0.1:32100
--lan-peer 127.0.0.1:32110`. Run the guest with `lan-live-guest`, a separate output
directory, and the bind/peer endpoints exchanged. The output manifest's
`lan.rounds[n].runtime.native_checksums` records per-frame checksums for each
round. The live targets drive real Move/Stop input, results, a rematch in the
host's reopened room and Main Menu return across two games.
The `lan-live-timeout-host` and `lan-live-quit-host` targets pair with
`lan-live-failure-guest` on the same endpoints. That guest plays the shared 300-frame
prefix and then exits without leaving, so the host meets a real peer disappearance
and either lets the connection time out or Quits from the wait dialog.
This loopback example alone does not establish separate-machine or scale coverage.
Normal players do not need these diagnostic flags. Validation results belong in
the task's validation records, not the tree.

The `lan-battle-host` and `lan-battle-guest` targets use the same isolated setup
and endpoints. Both select Americans through setup, deploy their AMCV (host D
key, guest self-click), and produce a GAPOWR. The input driver holds, resumes,
cancels with a refund check, then builds and places a fresh product while
retaining its paid identity. Both clients must observe both completed power
plants. The manifest retains 7,200 frame checksums and the observed production
actions. This is a bounded opening check with a 300-second default diagnostic
budget; simulation timing and network timeouts still belong to production.

The `lan-live-coop-host` and `lan-live-coop-guest` targets use the same setup.
The host uses the Soviet co-op campaign through Choose Map; both players take
B6 Observe at native frame 150, BC6 opens on both after the lost mission, both
press Continue, and the retried mission must run 90 frames over the same LAN
admission. The manifest's `lan.cooperative` records the BC6 result and the
retried startup. `lan-live-coop-saved-host` replaces the host's Soviet row with Use Saved
Campaign; seed its settings folder with a `coopsave.ini` section for the pair
(`LiveHostLiveGuest`), and both players load the saved stage's map.
`lan-live-coop-final-host` and `lan-live-coop-final-guest` start from a section whose
`CurrentMap` is the campaign's last stage: at native frame 150 both processes flag every
human House to win, BC6 offers only Quit, both press it and the campaign-end dialog
`0xBC7` opens. The host stays on it; the guest presses Continue back to the lobby.
The `online-live-coop-host` and `online-live-coop-guest` targets play the same campaign
in a VERA room on the deployed lobby and relay, with the offline test service off. Each
takes `--online-account <name>`; the guest also takes `--online-host <host name>`. The
host uses the Soviet row through Choose Map and the guest joins. At native frame 150
both flag every human House to win, both press BC6's Continue, and the kept room comes
back on the next stage with no `TXT_SAVE_CAMP_AVAILABLE` line in either setup's chat;
the host starts it and it must run 90 frames. The manifest's
`lan.cooperative` records the BC6 result, the room's next map and its startup.

The `online-live-watch` target checks [watching](#watching) from a third account. It
takes `--online-account <watcher>` and `--online-host <host name>`, both required, and
refuses every other online option: the host's match sets the rest. From the settled
online lobby it waits, with only the lobby's own 30-second list poll, until the watch
list shows a running match whose players include the host, then selects that row and
presses Join. The grant, the map and the relay's stream must bring it into the match.
It records whether the game was catching up and the frame where it reached the delayed
present, then requires 300 more frames without a stop: a desync's "Scenarios do not
match." or a lost relay's message fails it. It presses Tab until it is back on the
player it started with, which must pass another player and the observer's sidebar,
then Escape, which must return it to the online lobby, and leaves for the Main Menu.
Run it beside `online-live-host` and `online-live-guest` for two other
accounts. Their `--online-frames` must keep their first match going well past the
watcher's start: the list poll takes up to 30 seconds, the watcher sees the match two
minutes behind and needs 300 frames after catching up, and it catches up only when it
joins more than two minutes into the match. The host's room must let everyone watch,
as the Create Game Watching choice's default does; Quick Match games
(`--online-quick-match` on both) can always be watched. The manifest's `lan.watch`
records the match, the grant's delay, whether catch-up was seen and the frame it ended,
the frames played after it, the views Tab went through, and the exit route.
