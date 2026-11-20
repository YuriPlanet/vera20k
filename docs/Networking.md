# Multiplayer behavior and ownership

Windows, macOS and Linux builds provide LAN and online multiplayer through
the normal game menus; Android builds play LAN ([Android](android.md#lan-play-on-android)),
and the browser's network package plays online and, built with `--lan`, LAN with other
browsers only ([browser networking](wasm-net.en.md#lan-wasm-lan)).
Both paths use the existing
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
A room's map name is a logical name (`map::source::is_logical_name`): a game refuses one
with a drive, a root, a `.` or `..` part, or a part Windows reads as a device, such as
`CON` or `COM1`. Refusing the device part changes no packets: an older build still admits
such a name, and a rollback admits it again.
Games with two or more human players number their objects from one agreed start, so
a player whose game already ran the random-map dialog or an earlier match numbers
them as one just opened does; nothing new is exchanged, and older builds are
refused by the [build check](#build-check).
Fresh LAN and online matches also initialize local trigger variables from the
incoming map only. Unassigned values cannot carry over from a player's previous
match, including on generated maps with no local assignments. This includes a
co-op campaign's next mission and a retried mission, where the original game keeps
the previous mission's unassigned locals. Network replay and rejoin are unchanged:
each runs in a newly started game, so it never had an earlier match to carry from.
Watching loads in the running game, so a watcher who played an earlier match no
longer carries its locals either. Loading a save keeps its saved variables. Offline skirmish and campaign loads retain
the original game's local variable carryover. This changes no packets: mixed builds
are refused by the existing build check, and a rollback restores the earlier
carryover behavior.

Each new LAN or online match also rebuilds mutable Rules from the frozen startup
sources and the room's current options. This includes the country/color choices
shown before loading and the Rules used for random-map agreement and generation.
A previous map's scalars or extra Types cannot become defaults for the next room.
Offline loads keep the original retaining reset. No packet changes are required;
mixed builds use the existing refusal, and a rollback restores the earlier retention.

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
Ares needs no build feature: the rules digest still hashes `false` where the retired
`ares` feature's flag was, as a released game's does.
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
turn off, for the first of what it changes from the original game
(`agreement::original_game_changes`): a mod in use; `[ini]` roots or a
`theater_suffix` in config.toml other than YR's; an [Ares setting](ares.md#the-ares-setting) other than Auto; game-folder files that change a compared INI (Rules, Art, AI, MPModesMD and the mode INIs, CoopCampMD,
the theater INIs) or a theater's tiles from the retail copy; a copy from a retail
archive (`langmd.mix`, `language.mix`, `expandmd01.mix`, `ra2md.mix`, `ra2.mix`) counts as
the original, another INI is compared with the retail archives' own entry, else
`ra2md.mix`'s `localmd.mix` (`retail_sources::files_changing_shipped_inis`), and another
tile with the one a retail install reads (`map::theater::changed_tiles`), both found at
startup; the message names three and counts the rest, which the log lists. Personal
rules, Terrain Deformation, the Superweapon Limit and the Building Production Queue
need nothing: Quick Match's room has no
[room rules](#room-rules), so its match plays without them. A difference none of
these catches still meets the guest's admission: after `WOL:MatchErrorJoiningGame` its
status page says the games differ (`VERA:MatchStartMismatch`), the log records both
hashes, and the search goes on after the stage timeout; the host only times out. This
adds nothing to what game, lobby and relay exchange.

Every game names its version and build when it signs in (`Vera-Version` and `Vera-Build`,
docs/lobby-protocol.md "Sign-in headers"); the browser's sends every sign-in header the
desktop's does but federation (`live_service::http::sign_in_headers`). The lobby compares
two games' builds when both name one, else their versions. Two games that both name a
build therefore see no room or match to watch from another build, and Quick Match does
not pair them, where the start would be refused. Where one names no build, such as a
released game, which names only its version in its User-Agent, their versions decide as
before, and two builds of one version can still meet and be refused at the start. A
lobby that does not read the headers ignores them, and a rollback of game or lobby
compares versions again. The browser gateway must forward them: an older one
refuses them at its CORS check, failing a newer browser package's sign-in, so the
gateway ships first.

The lobby lists only rooms of the player's own version, and the matchbot pairs only
equal versions, so a game the lobby calls old asks its player to update. A 426 at
sign-in, the first 426 to a signed-in request (`Outdated`) and the lobby's
new-release line (phrase `NewRelease`) make the desktop updater check again, and a
newer release is offered at the next idle main menu, even a version the player
skipped (`app::updater::lobby_outdated`). Every text that points to a download
names vera20k.org (`web_link::DOWNLOAD_PAGE`), but a release signed by a new key
still points to the GitHub releases. Nothing exchanged changes: a released game
shows only the text with its link, and a rollback does the same.

A game on the newest release's version can still be another build, such as a dev build
or one built from source. latest.json names the release's own `build`, read from the
`Multiplayer build:` line its program logs at start-up (`src/entry.rs`,
`.github/release/downloads.md`). A game of that version whose sign-in names another
build hears that it is not the release (phrase `NotRelease`), and when a player joins a
room on another build of the room's version, the room's line names whose game is not
the release (`ReleaseMismatch`, `HostReleaseMismatch`) or says that the builds differ
(`BuildMismatch`), without the build hashes ([lobby protocol](lobby-protocol.md)). They
are chat lines: a released game without the keys shows the lobby's English text, a
lobby rollback names the builds again, and a latest.json without `build`, from a
release before it or a rollback to one, leaves `BuildMismatch` alone.

A relay deploy reaches every version the lobby admits, from `OLDEST_ADMITTED` in
`tools/relay_compat.py` on. Before a relay change merges, the Relay compatibility
workflow plays a client built on the relay protocol crate of each of those releases,
and of the newest dev build, against it. A change that breaks one of them raises that
floor, so the lobby refuses the release at sign-in with 426 instead of its matches
ending under it.

When an online start fails, each player's game says whose side failed and logs why.
If the lobby announces a match whose start this game cannot accept (for example,
its map or rules differ), the game finishes that specific pending match. Its
player sees the local reason, and the lobby tells the others the start was
cancelled. An older game that refuses locally still leaves everyone waiting for
the admission timeout. No packets or lobby changes are needed.
A start whose 30-second relay admission runs out tells a player whose `relay_ready` the
lobby took that another player's game could not reach the game server, a player whose
relay answered but whose `relay_ready` the lobby never took that the lobby is busy or not
responding, and a player whose relay never answered that their own game could not reach
it and that a firewall may block UDP; one that runs out in the latency check after it says
a player's game could not finish checking its connection. The lobby's `start_rejected`
reasons (a player left, cancelled, could not reach the relay or could not finish the
latency check) show as their own lines ([lobby protocol](lobby-protocol.md)); any other
reason keeps the general line. A start that times out connecting also names, in
`start_rejected`'s optional `phrases`, the players whose games never sent `relay_ready`,
so every player reads the same names after their line. A failed start leaves the room and
its chat, so the game keeps its line for the chat the player lands in next. Released games
read only `message` and show their earlier line, and a lobby rolled back to before this
sends no names, so current games show their own line alone. Nothing else new travels.

In network games, the trigger actions that only play audio (19, 20, 21, 98 and 99)
keep the map's sound, EVA or theme name, or its numeric sound slot, until the local
audio owner resolves it; offline games still resolve it when the map loads. A player
whose sound, EVA or music catalog lacks that entry therefore starts with the same
world as the others, and an object's action-99 custom sound keeps the same schedule.
That cue is silent for that player. On an action-99 object it also stops the object's
previous custom sound. The original game does the same for a missing slot, but for a
name that player's catalog lacks it lets the previous sound play out.
Other actions that use an audio-catalog ordinal as a number (such as a timer with a
sound-name parameter) still retain and hash that number, so real gameplay
differences are refused. This changes no messages: older builds are
refused by the existing build check. Saved custom sound references use a new automatic
snapshot layout; older layouts are refused. A rollback uses its earlier audio resolution
and save layout.

The online lobby asks for its game list, and the matches to watch with it, every
5 seconds while it is the front window, so a game someone creates shows up within
that; gamemd's lobby asked every 30 (`SetTimer(0, 30000)`). Refresh still asks at
once, and the lobbies list keeps its 53 seconds. A lobby that stops answering the list
says so once until a list arrives again. No request or reply changes: an older
game keeps asking every 30 seconds, and a rollback restores that.

The online lobby's Options page has a Translate Chat Into list: Off, or one of 14
languages, saved as `[VERA] TranslateChat=`, and under it a Translate in Matches box, on
by default and saved as `[VERA] TranslateMatches=`. With a language chosen, other
players' chat in the lobby, game rooms, private messages and, with the box ticked,
matches is sent to the lobby's translation service when it needs it: for a Latin-script
language a line with any letter outside it, for any other language a line with none of
its letters. A reader whose script another listed language shares, English readers
excepted, also sends the lines in that script, since they may be another language. The
lobby's own lines that VERA shows from their phrases in the chosen language are not sent.
The original stays; its translation follows as a line from the same sender marked `»`,
without the service's bracketed note naming the source language, unless it only repeats
the original, the service says it was already in the chosen language, or the player chose
another language while it was asked.

The app services reliable messages and intermediate progress while loading, and
continues servicing transport during frame waits. Gameplay uses the same
OutList/DoList owners as offline play; the transport does not own a second
simulation command queue.

Online games admitted with exactly one human use local command scheduling: commands
run at the next dispatch opportunity without a multiplayer lead-time buffer. The
first simulation frame still primes the existing network queue. Game-speed and CPU
timing continue through the same timing owner. Games originally admitted with
multiple human sources, including observers, retain synchronized scheduling even
after other players leave. AI players never acquire network identities. One
exception: one person who watches AI play in a room others may watch plays through
the relay, so that its match can be watched. The lobby marks that match's
certificate (`OrderingCertificate::ordered_alone`, [Watching](#watching)), and the
relay orders a one-source match only with that mark.

Online matches with two or more human sources, or one the lobby marked, use a
lobby-certified relay clock
and one immutable relay command stream. Every originally admitted client that
finishes loading must agree to the initial world, its build and its rules
(`agreement::local`) before it starts. A player who
never finishes loading leaves in the first frame once silent for 60 seconds, or 3
minutes after the first client was ready; the native loading timeout leaves that
departure to the relay. A match spread over several relays is cancelled instead.
At each relay, a loading player's Ready must agree with the first player's
initial world and signed match settings, including pacing. Only certificate
expiry may change on renewal. If these checks fail, or ready players on
different relays report different worlds, the relay refuses the start for
everyone at once with the existing agreement-mismatch error.
The other relays of such a match cancel through the coordinator, so their
players may see an ordinary cancellation. A relay whose players have not sent
Ready is still loading, not a mismatch. The refusal remains available to retries
after the empty match journal is removed. No client or relay packet changes.
Games from 0.2.9 name the start mismatch; 0.2.7 and 0.2.8, the oldest the lobby
admits, show a connection failure. Every relay is deployed and rolled back at
once (`services/relay/README.md`). A rolled-back relay again refuses only the
later player's Ready: the others wait until that player leaves, which cancels
the start, or for up to 3 minutes, then start without it (a match on several
relays is cancelled instead).
Deploying the relay change is separate from publishing a new game build.
A relay keeps a startup closed for 24 hours after it was cancelled before any Ready
reached that relay, or after the relay cleaned up its match, even once its room is
gone: a player who arrives later gets the cancelled-start message (0.2.7 and 0.2.8,
a game server failure) instead of starting alone. While that room remains, each
relay of a match on several relays tells the others the startup is cancelled; once
it is gone, another relay whose players arrive later cancels at its 3-minute
loading limit.
Relays of one match must also speak one relay-to-relay link version: a relay
that hears another version cancels the startup at once, and one too old to know
this cancels at its loading limit, so players get the cancelled-start message
instead of a frozen match. Game and lobby traffic do not change.
The build and rules ride in the same 32-byte start identity the relay already
compares (`StartupAgreement`), and a match that never started reports them as its
`build` and `rules` start parts. A lobby room refused another build or other rules
before its start, so only a launcher's relay match (`tools/spawn`) meets the new
refusal: one whose players run other builds or rules, which used to start and could
desync later, is now refused at its start like a different world, a released game
facing this one included. Its box (`VERA:MatchStartMismatch`) names the version and
files and, for a launcher's match, Terrain Deformation and the Superweapon Limit.
Players of one build agree as before, also after a rollback, and nothing new is
exchanged.
At each deadline, missing input is empty; a command joins the next unsealed
frame. The sender also waits for that commit.
Each relay seals at most 32 batches of its own players into a frame
(`MAX_FRAME_BATCHES`), and a federated match's frame merges those of up to six
relays (`MAX_FEDERATED_RELAYS`); a game takes every record such a frame can hold.
Earlier games end the match on a frame of more than 4,096 records (`QueueFull`),
what 32 full batches hold. Raising either limit in the relays ends busy matches
that way for the games built before it, so it raises `OLDEST_ADMITTED` too.
Frames use the existing Event codec and simulation queues, with a fresh compression
cache per self-contained batch. Batches travel in plain text: the relay can read
them, and only the per-hop MAC authenticates them. A batch's Event packet header
is not sent; each client rebuilds it from the batch's source and frame. A batch
that does not decode, or whose House is not its source's, makes its sender leave
at that frame on every client, as if by Leave; the rest of its stream is ignored.
Online and on LAN, every client also drops a received order for an object, wall or
bomb that another House owns, and a plan's dispatch (MEGAMISSION planning flag 2),
which each client's planner adds itself and never sends; gamemd trusts both.
No packet changes: older builds are refused by the build check, and a rollback
admits these orders again.

Redundancy follows each player's measured link
(`services/relay/adaptive-transport/spec.md`). One merged UPLINK request carries
unacknowledged batches, the receive watermark, the per-hop receipt feedback,
checksums and pace. The relay pushes each turn packed, with the reader's
unacknowledged records piggybacked into the space the turn leaves. A good link
sends each batch and turn once. A lossy link also gets 16 ms uplink copies and
8 ms downlink repeats; a jittery link gets only the downlink repeats. A congested
link gets neither, and a silent one only probes. Losses that the receipt feedback
shows are retransmitted by the relay; a lasting gap falls back to the existing
replay reader. Clients and relay ship together: older clients are not supported.

Every datagram uses the compact envelope
(`services/relay/bandwidth-compaction/spec.md`): a 32-bit session the relay
assigns at admission, a 32-bit hop sequence and a 4-byte truncated HMAC, 13 bytes
in all. History packs encode each record relative to the previous one and decode
alone. The receiving client rebuilds the same canonical records, so journal,
replay and federation are unchanged.

In a match played through one relay, any player
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
world reaches after catching up. Each game also keeps a message row while its
own request waits for the relay and while the match is paused for all players.
Connections, chat and ticket renewal keep
running; accepted gameplay inputs wait for their assigned frames after resume.
Opening Options alone remains local. After a resume nobody can pause again for
30 seconds. Confirming Leave resumes as needed until the queued EXIT executes.
Shared paused time is excluded from the score-screen duration.
A direct LAN match and a match spread over several relays cannot pause.

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
keep the match's own frame rate, and a replay plays at that speed. A match
spread over several relays slows the same way; its first relay decides for all.
Native lockstep instead slows every match to its slowest player (ProcessNetwork
`0x00647BF4..0x00647E6D`).

When the lobby admits a match on three or more relays with federation
adjudication, a relay missing for ten minutes ends that match without a winner.
A short outage keeps its place. Losing a player's own connection still follows
the ordinary reconnect rule below. Each surviving relay propagates a confirmed
failure even when the missing relay coordinated the start; it creates no player
departure or simulation frames to bridge the gap. Current games and watchers
show what happened, and released games show a connection failure.
The relay and lobby compatibility gates are in [the relay guide](../services/relay/README.md).

The relay keeps a bounded RAM cache and the full match journal on disk. The
journal is not encrypted: it holds plain commands and relies on file permissions. Temporary connection loss keeps the player's identity, units, input cursor
and world in the match; the client renews its ticket and reads missed frames when
connectivity returns. The game shows Reconnecting or Catching up as a message
row while healthy clients continue. Such a row, and the shared pause's, stays
in the message list while its state lasts; it comes and goes only once the
state has held for a second, so a short dip adds no row. Once the relay has
heard nothing from a player of a started match for 10 seconds, the other
players read "<name> is not responding.", and "<name> is back." when it hears
them again; watchers read neither. The relay
names silent players in each player's status (relay README "Silence"). A
released game ignores the names. An older relay names nobody, so a game on it
shows no such line; in a match spread over several relays, players homed on an
older relay are never named.
A game's network thread keeps the relay hearing from it through a long frame or
loading step (its keepalive, and the UPLINK heartbeat of spec §6.6), but not
for a game that is gone: it stops once the game crashes, from the moment the
crash box shows, or once the game thread has not serviced the connection for
60 seconds. The others then read "<name> is not responding." 10 seconds later
and the ten-minute limit below starts, as for a closed game. A frozen game that
recovers is heard again; no Leave goes out, so the launcher can still resume
the match. A crash also silences a LAN match's sockets. A healthy game
whose window is minimised or covered after relay admission continues its
connection checks, loads its map and enters the match without a redraw.
The game thread services that work and the crash/stall limits stay in effect.
An older game can still fall silent in that window and be dropped before it
starts.
The relay and the packets are unchanged. A released game keeps sending for a
crashed or frozen game, so it is never named.
After ten minutes of silence the relay removes the player
through the same stream. A game that has had no answer from the relay for that
long, and 15 seconds more, releases its relay route and sends it nothing more:
its place is gone, and a game whose ticket lapsed gets no answer that says so.
A match the relay orders then ends with the game-server connection message, also
one person's who watches AI play; any other one-human match plays on without the
relay. Like a native lost connection, a removal for silence
destroys the player's House and shows "connection lost"; a player who quits still
leaves through EXIT. While Reconnecting, the relay cannot sequence that EXIT, so
a confirmed Abort Mission returns to the shell once the relay has been silent for
three seconds; the client's relay Leave, or else that silence, removes the player. Silence that long, explicit Leave, joining another online
match on the same relay, or the match's final frame after 24 hours of play ends
membership. The relay
remains required for progress.

A closed desktop game first sends the relay its Leave and the lobby its `finish`,
waiting up to three seconds for them. A process that left a match without Leave (a
crash, a closed browser tab, or a closed game whose Leave did not arrive in time) can
be replaced within that silence limit. The launcher receives a fresh `[Relay]`
section with `ResumePings`, the timing vector every client started with, and
`ResumeCursor`, the end of the lobby events that process already consumed. The new
process skips the start barriers, loads the same world without reporting progress to
the others, and reads the whole stream from sequence 1 as ordinary catch-up. It
learns its batch counter from that stream, so it submits nothing until the world has
caught up; local orders given until then are dropped, and effects and voices stay
silent. The relay refuses a different world, build or rules as an agreement mismatch.
Abort Mission during this replay leaves as it does while Reconnecting.

Authored matches record reconstructible JSON replays in `replays/`,
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

Fresh LAN and online games also seed the bridge-repair MapGen RNG from the
agreed match seed, instead of retaining each player's earlier random-map or
bridge-repair history; on a generated map this replaces the cursor the map's
generation left. The cursor is part of the world hash the start agreement compares,
so a difference stops the start; the per-frame sync checksum does not include it.
Rejoining and watching rebuild this same start before replaying the relay's
history. Offline games carry the cursor over from the previous game as the original
game does, but right after a network match that is the match's reseeded cursor,
not the one the original game would have kept. This adds no
packet fields; older builds are refused by the existing build check, and a
rollback restores that build's earlier retention and checksums.

| Responsibility | Authoritative owner |
|---|---|
| Pending and scheduled simulation commands, native event dispatch | `sim::command_queue`, `sim::world::network_commands` |
| Event compression/decompression and persistent caches | `net::event_packet` |
| Packet construction, cumulative counts and peer frame progress | `net::command_exchange` |
| Shared reliable send queue, retransmission and response-time history | `net::reliable` |
| Global application message admission, acknowledgements and retirement | `net::global` |
| UDP sockets, datagram checksum, retained raw send attempts and the client relay route (session, renewal, Leave, absence release, ordering inbox) | `net::udp` |
| Startup agreement, native send cadence and command wait; online, the relay timeline's assembly, its departure records and the checks that end the match | `net::live_commands` |
| Certified online clock, immutable command history and bounded push/replay delivery | `vera20k-relay-protocol` |
| Local adaptive playout and bounded catch-up presentation | `app::match_runtime::frame_pacer` |
| LAN packet layouts and setup payload framing | `net::lan_messages` |
| Participant membership, admitted endpoints and launch state | `app::match_runtime::session::MatchSession` |
| Multiplayer frame-checksum history (`B04474`) and its comparison gate | `sim::command_queue::FrameChecksumState`, held by `app::match_runtime::sim_host::SimHost` |
| Discovery projection, physical LAN lobby/setup service and the host's options | `app::lan_service` |
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

A LAN host sends its full options and its roster as VS31 setup payloads, split over
the reliable global channel (`net::lan_messages::extended_setup`), in place of gamemd's
options text (`0x005DBB60`) and roster packet. The options are a JSON envelope with
`agreement`, `current_speed`, `game_speed`, `admissions`, and `body`. The body
uses `kind: "stock"` and carries `random_map` and `room`. The stock room holds the
seed, setup options, mode, map and metadata, each player's side, colour, start and team
by name, and every row's AI words. A guest applies the envelope to its admitted
participants and reapplies it for a late Type3 only in the same admission epoch and
revision (`app::lan_service::options`).

The build agreement identifies the envelope revision through the existing
source-derived build identity. Mixed builds are refused at join with reason 8.
The guest also reads the agreement before decoding the options body, so it can show
`VERA:LanHostBuildDiffers` even for the earlier layout. An invalid room update uses
`VERA:LanOptionsInvalid` and ends that setup. After a rollback, two games running
the earlier build exchange its earlier layout. The lobby, relay, and VS31
fragment framing keep their existing formats.

A VERA room's Teams count, which only its host sets, travels with the host's options:
online as `launch.options.teams` ([lobby protocol](lobby-protocol.md)), on LAN as
`body.room.options.teams`. With it set, the host keeps every seated player on one of the
room's teams, so guests only show what the host sends. A LAN guest from before Teams is
refused at the build check. Online, an older guest cannot
read the host's options while Teams is set and says the update was invalid, in a room
that cannot start, since its players' versions differ. A lobby from before Teams
refuses the field; the host's game then turns Teams Off, says so and sends its options
again without it. A lobby that lists matches to watch already supports Teams, the queue
and the room rules. A refusal from it leaves the settings in place. The host checks the
map has enough starts for the playing teams, people and AI before publishing, so they
can change the map or the seats while keeping their room settings. A room has 2 to 15
teams, named A to O.

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
room's spare rows. The host's LAN options carry all thirty AI rows on any map, where
gamemd's text carries eight for a map of up to eight players; a guest of another build is
refused at the build check. At Start Game, Team Starts gives
each team a start and the spare starts to the teams with the most players for each; a
team's people on one start share its House, as players who pick the same start do, and
each AI there starts in its own House beside them, its units on the cells nearby
(`SkirmishLaunchSession::shares_bases`). The map then needs a start for each team.
Turning Teams Off, or picking a map or mode with fewer seats, makes the people past the
seats watch rather than leave the room, and the AI there leave. Nothing new travels: the
host's options carry each player's row and start as before, and a room of mixed versions
cannot start. Online the lobby checks the map's starts against the bases the room needs,
and the host's game checks the same count before it publishes and says the map is too
small ([lobby protocol](lobby-protocol.md)). A lobby rolled back past this is from before
Watching as well, and refuses the host's options once more people and AI play than the
map has seats; the host's game then drops the Superweapon Limit, the host rules and the
queue, then Teams, one at a time and saying so each time, after which the people past the
seats watch. A rollback of the game seats no more players than the map has starts.

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
start for each AI. The lobby places its relay by the people watching and lists it to
watch like any match ([Watching](#watching)), also when one person alone watches, to
games that can watch it (`Vera-Watch-AI: 1`). Nothing new travels. A lobby from
before this refuses to start it, and the host's game says the match could not start; an
older game keeps its rule that someone plays, and a rollback does again.

A VERA room whose host leaves stays open when every game left in it said at sign-in that
it can take over (`Vera-Host-Handover: 1`, sent by every game built with this): the
lobby makes the player who has been in it longest its host and sends `host_changed`
before the old host's `member_left` (docs/lobby-protocol.md "Host handover"). The new
host's game opens the room again from its own room settings, as Create Game does, and
seats everyone still there, raising its Room size and opening the Spectators where its
own settings would leave someone out; the others' games enter it again as after a join.
A room named as games name rooms (`<name>'s_game`) takes the new host's name unless
another open room has it; the room is listed to the new host's version and stays ranked
or unranked as it was created. The chat stays, and it says who left and who hosts now. A
room that comes back after its match without its host goes to its next player the same
way, through `room_reopened`. In a room with an older game in it, or a Quick Match
room, the guests receive `channel_left` when the host leaves. Released games already
close the room on that event. Rolling the lobby back can also show the old generic
refusal before the close. A lobby from before host handover closes every such room.

In a VERA room before its start, the host and every guest can press Lobby to look round
the public lobby while staying in the room, and Back to Room shows the room again
(docs/lobby-protocol.md "Visiting the lobby"). The room goes on hidden: its chat, picks,
readiness and newcomers keep reaching the visitor's game, and the host's game keeps
seating newcomers. The room's start brings everyone back to it. The new commands are
`visit_lobby` and `end_visit`, and the lobby marks the visited lobby's `joined`, `chat`,
`member_joined` and `member_left` with `visit`. Released games never send the commands,
and nobody else sees a difference: a visitor is a member of both channels, listed in the
public lobby too. A lobby from before this lists no `visit_lobby` among the sign-in
answer's `features`, so the game greys Lobby out. One rolled back after the sign-in
refuses `visit_lobby`; the game then says it could not go to the lobby, and the player
stays in the room. A rollback during a visit leaves the visitor in both channels: the
older lobby sends the public lobby's events to their game unmarked, which takes them as
the room's, and refuses Back to Room's `end_visit` and their lobby chat with a notice. A
host who visited then seats the public lobby's newcomers in their room; the older lobby
refuses options naming them, so the room cannot start until those players leave the
public lobby or the host leaves the room, and a visitor who joins the public lobby again
is listed twice. Roll the lobby back past this only when no room has a visitor. The
public lobby lists a stranded visitor until this lobby is back and they leave the room
or sign out; it then also drops an entry whose session is gone.

Games 0.3.3 and 0.3.4 had a host setting, Balance automatically, that travels online as
`launch.options.auto_balance` and had the lobby balance the room 5 seconds after who
plays or the Teams count changed. Current games neither offer it nor send it. A current
guest reads and ignores it in such a host's room, which cannot start anyway, since the
versions differ: the guest's Team list and team headers' Join stay open, that host's
game keeps the player's team, and the guest is told the host did not acknowledge the
choice. That host's own balance still moves the players. A released guest in a current
host's room sees the setting off; the host's Balance button still evens the teams.

A VERA room queues its spectators for seats. A spectator who asks to play while no seat
is free, with a team header's Join Queue or, in a room without teams, as a newcomer who
prefers to play, joins the end of the host's join queue; Leave Queue, or picking to watch again, leaves it. The host's
game owns the queue as it owns seating: when a seat frees (a player leaves or starts
watching, or the host adds a slot or removes an AI), it seats the first spectator in the
queue as its own Seat would and publishes its options. The queue's order travels online
as `launch.options.queue` and on LAN as `body.room.options.queue`. Every game in
the room lists the waiting spectators first, each with its place before its name, and a
waiting player's own game says its place and when a seat came free. As with Teams, an
older guest is refused at LAN's build check and online cannot read the options while
anyone waits. A lobby from before the queue refuses the field; the host's game stops
queueing in that room, says so and sends its options again, and its spectators keep
watching. A rollback past Teams then turns Teams Off too.

Network rooms seat up to 30 players on an authored map, including maps that seat up to
120 in offline Skirmish. For maps with more than 30 starts, the host advertises 30 and
guests verify the same capacity. Create Random Map and imported random-map seeds support up to 30
starts in a room. The room offers the sizes past Very Large, the added landform and
theaters, Advanced, Fair Starts, Extras, Presets and Export Map. A map for more than eight players stages one `W` slot for each start
(`ScenarioStartStaging::to_w_text`), so its record runs past the native 37 numbers and
512 bytes, up to 2048 (`maps::checked_staging`); it travels as any random map's does, on
LAN in the host's options and online as `random_map` ([lobby protocol](lobby-protocol.md)).
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

### Build check

Two games play together only on one build: their `shell_network_setup::agreement::version()`,
the package version and `VERA20K_NETWORK_BUILD` joined by `:`, must be equal. build.rs hashes
into `VERA20K_NETWORK_BUILD` the files under `src/` and `services/relay-protocol/src`, the
manifests, `Cargo.lock` and `build.rs`, by path and content with CRLF line endings counted as
LF, and every enabled Cargo feature but the test groups and the browser's thread pool
(`wasm-threads`), which do not change how the game plays (`network_build::add_features`). One
commit built with the same features therefore matches on every platform, and any other
difference keeps games apart: a default build (`dev-overlays`) never meets a
`--no-default-features` release, APK or browser package, and a `--lan` browser package does
not meet a `--network` one. A browser package built with `--threads` matches the same package
built without it, and the desktop build with the package's other features
(`--no-default-features --features wasm-net`), which is how a launcher seats desktop and
browser players in one room. After a rollback to a build that counts `wasm-threads` and
compares the build at a start, such a room is refused at its start. Earlier builds add `:0` and their Phobos, NPatch and NPBUG flags (`:0000`
in a release), which the hash already covers; they are refused as any other build is.

- LAN: the host denies a join from another build (reason 8), and the guest sees Request
  denied and "VERA20k build or enabled gameplay features differ." (`VERA:LanBuildDiffers`).
  A guest that receives the host's options from another build says so in the room's chat
  (`VERA:LanHostBuildDiffers`) and does not start.
- Online: the sign-in names the version (`Vera-Build`), and every `ready` and `map_status`
  carries it. The lobby hides rooms and matches of another build, or of another version
  where a game names no build, and starts a match only when every player's is equal
  ([lobby protocol](lobby-protocol.md#sign-in-headers)); a watcher's game compares its own
  with the match's before it loads. A game of the newest release's version on another
  build is told it is not the release ([current production path](#current-production-path)).
- Start: every network start folds it into the start identity the relay and LAN's startup
  compare (`StartupAgreement`), so a launcher's match of two builds, which no lobby
  compares, is refused at its start.
- External launches: `[Settings] ExpectedVersion` is compared with it, as `--probe`
  reports it ([external launches](spawn.md#probe)).

### Room rules

The LAN envelope carries two independent speed values. `current_speed` applies to
the guest's live OptionsClass (`0x005E4DE1`). `game_speed` supplies the setup trackbar
(`0x005E4E01`) and the speed copied into the match at launch (`0x005E74AA`). The
stock body omits the speed, so neither value acquires a second wire owner.

A network room plays its host's rules (`skirmish_launch::HostRules`): the host's
Terrain Deformation box, [Superweapon Limit](superweapon-limit.md) and
[Building Production Queue](phobos-building-queue.md). They open as the
host's skirmish Game Options and Options page have them, and the host can change them in
the room's Game Options. They travel with the host's options, online as
`launch.options.host_rules` ([lobby protocol](lobby-protocol.md)) and on LAN as
`body.room.options.host_rules`. Every player's game, the host's too, plays
them in place of its own from the next scenario (`ProcessAssets::use_personal_rules`).
The switches compose over the root each scenario reads, never over the
cold start or the shell's Country reads, so every player's one rules registry is the
same whichever rules a room plays. Offline games play the player's own, a network
replay the room's it recorded. A Quick Match room has none: the guest's game drops
any its host sends, and the lobby refuses them there. A launcher's network game
(`tools/spawn`) has no host to share them, so each player plays their own, which its
`ExpectedRulesDigest` compares, and the start barrier compares again; it plays
without the Building Production Queue, which its probe leaves out too, so a player's
Options page cannot keep a launch from starting. So the room's
door, the native rules hash (LAN's join check, online the room's `ex_info`), leaves
the switches out, while the rules
digest compares the ones each game plays.
The two checks also read different files, so they can disagree either way. The door
(`rules::native_ini_hash`, `0x00679D90`/`0x00679ED0`/`0x00679EC0`, compared on LAN at
`0x005E1711..0x005E174D` and online at `0x0079BB16..0x0079BBAE`) hashes the rules root,
every mode's INI, CoopCampMD, AIMD and ARTMD whatever mode the room plays, but no
LANGRULE, theater INI, theater tile or other file of a mod. The digest
(`ProcessAssets::multiplayer_rules_digest`) covers only the mode played, and adds
LANGRULE, the theater INIs, the changed tiles, the switches and a mod's other files.
So a player whose only change is another mode's INI is refused at the join though the
match would agree, and one whose theater INI or tiles differ is let in and refused at
the start. This is kept on purpose: the door is gamemd's admission check, which
released games rely on. When the door
still refuses a join because the rules files differ, or the digest a start, each game
says what it changes from the original game, its mod, `config.toml`'s INI roots, its
Ares setting other than Auto and changed game files (`agreement::original_game_changes`),
or that it plays the original (`agreement::rules_differ_text`);
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
The Building Production Queue came after that, as
`host_rules.building_production_queue` (`true`, left out while off). An older guest
cannot read options that carry it, in a room that cannot start as its players'
versions differ, and an older LAN guest is refused at the build check. A lobby from
before it lists matches, so the capability check that retires the older rules cannot
tell it apart: the host's game turns the queue off whenever the lobby refuses options
that carry it, says so and sends its options again with its other rules
(`shell_network_setup::lobby_refused_building_queue`). A rollback of the game plays
every room without it; a rollback of the lobby refuses it again, which the host's
game handles the same way.

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
watcher's own version with two or more people in them, one of them playing, or, to a
game that signed in with `Vera-Watch-AI: 1`, where only AI play while everyone in the
room watches; on a relay that lets watchers in,
and never one from a room with a password, a clan room, a generated map or a co-op
mission: a watcher's game cannot yet rebuild their starts. Create Game starts Watching
on Live. The host can also choose a two-minute delay or turn Watching off. The next
room uses the host's last confirmed choice in the same session. Quick Match and ranked
games always have a two-minute viewing delay.

A lone observer normally plays an AI match locally, so there is no stream for
others to watch ([above](#current-production-path)). To make it watchable, the
room must allow watching, the observer's game must advertise `Vera-Watch-AI: 1`
at sign-in, and the selected relay must have both `watch: true` and
`ordered_alone: true` in the lobby's fleet configuration.

When all three apply, the lobby sets VO2C flags bit 1
(`OrderingCertificate::ordered_alone`). The relay then orders that one source,
and the game plays through it with the usual command lead time
(`live_commands::relay_clock`). Even without a watcher, the match holds a relay
ordering room and journal, and losing the relay ends it. The desktop client
advertises support. Other clients keep local play for a lone observer until
they advertise it too.

The lobby's grant carries the match's start, its options and a watch ticket. The
watcher's game downloads the map as a guest does, checks that its version and rules
digest are the players', and joins the relay as a watcher (`live_service::prepare_watch`,
`poll_watch`): a read-only session that pulls the match's stream from its first frame,
held back by the delay. Receiving the grant ends the lobby request's wait. The map
download and relay connection each keep their own deadline. If the setup cannot
start, the game releases the watch grant with
`unwatch` and shows the failure in the lobby. A watcher only reads the stream
(relay README, "Watchers"). It loads the world as a returning player does, with no
progress reports and nobody waiting for it, and then plays the stream's frames. Until
it reaches the match's delayed present it runs them at speed, in passes of up to 50 ms
a picture where a returning player's stop at 8 frames or 8 ms, says Catching up, and
keeps its sounds off. It keeps every frame it has read rather than a returning player's
64, so a long match is read to its end before the relay forgets it, 300 seconds after
the end. It checks its world against the checksums all players reported
alike and stops with the usual message when they differ.

A relay watcher has a separate connection entry in the session roster. The roster
of playing sources still follows the admitted native nodes. Direct LAN observes
through the classic Observer House, as do classic WOL and Skirmish. A connection
watcher has no node, command source, or House and
contributes no progress, readiness, or retry vote. It borrows a player's sight,
sidebar, credits and sounds. Every House keeps its own fog in the shared world, so the watcher's map and
radar show that House's, as the player sees them. Where only AI play, it stands in the
first observer's seat (`live_service::followed_user`) and sees what that observer sees:
the observer's sidebar and the whole map; Tab then has no player to follow and changes
nothing. It sends nothing in that seat either
(`Simulation::install_watcher`): the
followed player's records are another game's to it, so their Options opens nothing and
their leaving ends the watch no more than it ends the other players' match. Tab follows
the next player still in the match, in House order, also while a frame waits on the
stream (`match_runtime::watching`): online the current House only chooses this game's
view and result, so following rebuilds just the objects' owner and discovery bytes from
the history every player's game keeps (`Simulation::follow_house`). After the last
player, Tab shows the observer's sidebar as an observer in the room sees it: each
player's name, kills, units and money, and the time played on the credits counter
(`Simulation::observe`), with the whole map on the battlefield and the radar. The world
keeps the last player's House for sounds and radar power. The next Tab follows the first player again, with their
sight. A watcher gives no orders: its input is the camera (the arrow keys, the screen's
edges, a right drag over the battlefield and a press on the radar), zoom, Tab and
Escape, and it has no chat. Escape leaves at once; the returned transport's relay
Leave ends the watcher's session. Defeat or departure of the followed player
leaves the watch active. The shared elimination result leads to the score page
and the online lobby. When every player's departure reaches the relay stream
without that result, the watcher returns to the lobby with the match-ended line.
The lobby renews a watcher's relay pass as it does a player's (`renew_watch`).

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

A game that does not send `Vera-Watch-AI: 1`, such as any released before this, gets
no marked certificate, plays one person's match without the relay as before, and never
sees a match where only AI play listed. A lobby from before this marks no certificate,
so one person watching AI play plays without the relay, unwatched, and it lists no such
match. A lobby rolled back to before this will not sign a marked match's certificate
again, so that match's relay ticket eventually expires. Enable `ordered_alone`
only after upgrading the selected relay. Before a rollback, remove the field from
the fleet configuration to keep new solo matches local. Wait for existing marked
matches to end before rolling back the lobby or relay.

### Returning after a match

The accepted match owns its return to a kept LAN/WOL room, including the
guest's LAN rejoin deadline and the stock Cooperative stage chosen by
Continue. The lobby displays the current disposition and sends the emitted
join or finish requests. A stock LAN Cooperative continuation or retry
uses BC6's existing Begin, Options, Ack and Go packets. Only playing
connections vote or acknowledge. The host sends Options after resolving
assignments, then starts after one acknowledgment
from each required guest. A handoff that times out sends the existing scoped
Departure so a peer already loading the same admission releases its loader.
If the departure is lost, the next mission's loading deadline still retires
the entire continuation when a required playing connection is missing.
A rollback restores that build's return handling. Mixed builds are refused
by the room agreement before either side can enter a continuation.

## Native behavior and deliberate bounds

The following native scheduling/reliability details apply to LAN and to the native
reliable messages online play sends through the relay as DATA; the relay-ordered
command stream above is an intentional adaptation.

- Event packets use the native layouts and persistent compression caches from
  `64B4D0`, `64B660` and `64BDD0`. The packet boundary rejects a semantic event
  without a supported raw command producer rather than inventing a wire form.
  On LAN every packet, FRAMESYNC probes included, starts with VERA's `VCM\x01`
  and a little-endian u64 command frontier within the native 558 bytes, and a
  receiver refuses one without it (`net::event_packet`). VERA's own records,
  which LAN packets and online batches carry alike, are laid out in
  `sim::command`: 0x30 paid superweapon, 0x31 bunker eject (still decoded so older
  recordings play; no input sends it), 0x32 producer cycle, and 0x33 explicit
  target-House economic order. The 0x33 body is a native House slot byte, an
  inner economic opcode byte, and 21 bytes of inner payload with zero padding.
  The event header still authenticates the issuing source. The simulation
  requires FullControl over the target. Ordinary production, placement and
  superweapon records continue to use the source's primary House. The build
  check refuses peers with older codecs before play.
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
  retaining the portable socket bindings. A returning guest waits for the
  admitted host's discovery ID. Same-named games keep their own advertisements.

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

The single-process `lan-*` targets (listed in
`src/app/diagnostics/shell_capture/movies/lan.rs`) enter Main Menu Network and the
real BB/BC/BD handlers; declared service events stand in for the remote side:
discovery, admission, host options, readiness, map reports and completion. They
write LAN and MultiPlayer preferences, so give them an isolated `RA2_DIR` and a
`[paths] settings_dir` outside it, never a player's profile. The WOL persona
targets (`wolinfoMD.ini`) and `lan-guest-random-map` (the received `RandMap.Sed`
and `RandMap.img`) write into that settings directory only when it holds the same
marker. `lan-transfer-loading-first` writes the original `XMP29U2.MAP` as
`CONTROLLED.MAP` into the marked game copy and checks the production parser's
name, length and SHA-256; it refuses an existing file and removes only the
unchanged file it created. The `lan-*-loading-first` targets stop at the network
loading boundary, while `lan-host-abort-return` and `lan-guest-abort-return` go on
through pause, Abort and the queued EXIT back to the LAN lobby.
`lan-0xbb-join-denied` and `lan-0xbb-join-denied-return` answer a real Join with
reason 1; `lan-0xbb-join-timeout-return` sends no answer and waits out the normal
600 × 16 ms join timer.

After building the normal app with `--profile fast`, run the host with
`--shell-capture lan-live-host --width 800 --height 600 --cursor-x 400
--cursor-y 300 --output <new-host-directory> --lan-bind 127.0.0.1:32100
--lan-peer 127.0.0.1:32110`. Run the guest with `lan-live-guest`, a separate output
directory, and the bind/peer endpoints exchanged. The output manifest's
`lan.rounds[n].runtime.native_checksums` records per-frame checksums for each
round. The live targets drive real Move/Stop input, results, a rematch in the
host's reopened room and Main Menu return across two games.
For a process-state comparison, give both peers `--lan-isolation-seed 73`
and the host `--lan-map XMP03T4.MAP`. This fixes the seed, countries, colors and
start positions across both rounds and skips the separate observer-seat exercise. Compare
the accepted launch, startup receipt and committed frame checksums; any input
timing differences still need to be reconciled before attributing a difference
to retained state.
The host can add `--lan-prior-map <history-map>` to play a different first map.
The second round still uses `--lan-map`. Each round records `setup_catalog`
before loading and `gameplay_rules` afterward, so the comparison can confirm
that the history changed the first world and that its choices were then cleared.

`sim-bench --state-isolation-report <new.json> --map XMP03T4.MAP --seed 73
--client-timing --isolation-mode wol --isolation-humans 1 --computers 1`
provides the complementary headless comparison. Run it in separate processes,
first without a history, then with one or more `--prior-map <map>` arguments.
Equal `target` values cover the Rules owner, world construction and every
recorded simulation frame for that fixture. This does not cover app input or
the network transport.

The `lan-live-timeout-host` and `lan-live-quit-host` targets pair with
`lan-live-failure-guest` on the same endpoints. That guest plays the shared 300-frame
prefix and then exits without leaving, so the host meets a real peer disappearance
and either lets the connection time out or Quits from the wait dialog.
This loopback example alone does not establish separate-machine or scale coverage.
Normal players do not need these diagnostic flags. Validation results belong in
the task's validation records, not the tree.

The `online-live-host` and `online-live-guest` targets play a match in a VERA room on
the deployed lobby and relay (`VERA20K_LOBBY_URL` names another HTTPS lobby, such as a
staging one). They take the LAN pair's window,
cursor and output flags and isolated copies, but no `--lan-*` option. The host takes
`--online-account <host>`, a name that leaves room for `'s_game` in a 24-character
room name; the guest takes `--online-account <guest> --online-host <host>`. They play
two players for 7,200 frames, and `--online-frames N` changes the count. More than two
players (`--online-players N` on every process, one guest process for each further
account) also needs `--online-simple-input` or `--online-mass-input`.
`online-live-timeout-host` and `online-live-quit-host` pair with
`online-live-failure-guest` as their LAN namesakes do, except that this guest writes
`<output>.prefix.json` after the shared 300-frame prefix and then stays in the match:
kill its process by hand to make the peer disappear. `lan-live-desync-host` with
`lan-live-desync-guest` expects the desync box once the `lan-fault-proxy` tool between
them damages a FRAMEINFO checksum past the shared 300-frame prefix. The parser's
errors give each target's option rules. A launcher's
match (`vera20k -SPAWN`) on loopback, played directly, through a local relay or with
browser players, runs from `tools/spawn/run.sh direct|relay|browser`
([its README](../tools/spawn/README.md)).

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
in a VERA room on the deployed lobby and relay. Each
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
presses Join. The grant, the map and the relay's stream must bring it into the match
as a watcher with its followed player's sight, not the whole map. It records whether
the game was catching up and the frame where it reached the delayed present, then
requires 300 more frames without a stop: a desync's "Scenarios do not match." or a lost
relay's message fails it. It presses Tab until it is back on the player it started
with, which must pass another player and the observer's sidebar; only the observer's
sidebar may show the whole map. A right drag from the battlefield's middle must then
move the view within 10 seconds (it turns left halfway, for a view already at the
map's right edge), then Escape must return it to the online lobby, and it leaves for
the Main Menu.
Run it beside `online-live-host` and `online-live-guest` for two other
accounts. Set their `--online-frames` so the match lasts through the list poll (up to
5 seconds), the granted viewing delay and 300 frames after the watcher catches up.
To see catch-up on a two-minute stream, join more than two minutes into the match.
The host's room must allow watching. Quick Match games
(`--online-quick-match` on both) can always be watched. The manifest's `lan.watch`
records the match, the grant's delay, whether catch-up was seen and the frame it ended,
the frames played after it, the views Tab went through, how far the right drag moved
the view, and the exit route.

The `lan-live-desync-host` and `lan-live-desync-guest` targets use the isolated setup
of `lan-live-host` and `lan-live-guest` and check that a checksum mismatch ends a LAN
match as in gamemd. After the shared 300-frame prefix each writes
`<output>.prefix.json` and waits for its peer's damaged FRAMEINFO checksum; each must
then get the out-of-sync dialog, press Stop and come back to the settled LAN lobby. The
`lan-fault-proxy` tool does the damage and is absent from game builds; build it with
`python -m tools.cargo_run -- build --profile fast -p vera20k-core --features
test-support --bin lan-fault-proxy`. Start it first with `--host-proxy
127.0.0.1:32120 --guest-proxy 127.0.0.1:32130 --host 127.0.0.1:32100 --guest
127.0.0.1:32110 --timeout 1800`, then the host with `--lan-bind 127.0.0.1:32100
--lan-peer 127.0.0.1:32130` and the guest with `--lan-bind 127.0.0.1:32110 --lan-peer
127.0.0.1:32120`, so each player reaches the other only through the proxy. Once both
prefix files exist, write `ARM` and a newline to the proxy's standard input: it then
flips one bit of the checksum in the next command-free FRAMEINFO each way and passes
every other datagram unchanged. Write `STOP` after both captures finish. The proxy
prints one JSON line per step on standard output, and exits with an error if armed
before it has seen both players past frame 300.
