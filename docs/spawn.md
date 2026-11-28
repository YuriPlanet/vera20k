# External launches

A launcher outside the game starts a match with `-SPAWN` and a `spawn.ini`. Before it does,
headless commands answer what that launch needs: the version says which build this is, the probe
says what a launch on this installation is checked against, and the random-map command makes the
file of a random-map room. The probe and the random-map command run in a folder whose
`config.toml` names the game folder; the version needs none. Each opens no window, prints one
JSON line on stdout and exits 0; on failure it exits non-zero with the reason on stderr. The
browser package exports them as functions ([Wasm guide](wasm.md)).

## Version

```sh
vera20k --version
vera20k --probe --version
```

Both print the probe's `version` and `build_identity` (below) and nothing else, without a game
folder, retail files or rules:

```json
{"version":"0.3.9:<network build>","build_identity":{"package_version":"0.3.9","network_build":"<sha-256>","phobos":true,"npatch":true,"verax":true,"target":"macos","commit":"00f524cc8…"}}
```

`package_version` is `Cargo.toml`'s, which a release raises right after it is made, so a
development build names the release it leads to. `commit` is the commit the builder named in
`VERA20K_COMMIT` when building (the Release workflow does), or `null`. Neither `commit` nor
`target` decides who plays together; `version` does.

A build from before this command treats `--version` as an unknown launch argument and starts the
game, while it refuses `--probe --version` with a usage error. A launcher that may meet such a
build asks with `--probe --version` and treats a failure as "this build cannot say". In the
browser, `version()` returns the same JSON text; it needs no imported files, and a package without
it lacks the export.

## Probe

```sh
vera20k --probe [--ra2-dir <dir>] [--map <map-file>] [--mod <folder> | --no-mod] [--ares Auto|On|Off]
```

It reports the multiplayer `version` (`[Settings] ExpectedVersion`) and its parts
(`build_identity`, as [Version](#version) prints it), the default mode's `rules_digest` (`ExpectedRulesDigest`) and every loaded
mode's digest in `game_modes`, the `map` given (`file`, `byte_length`, `source_sha256` for
`ExpectedMapDigest`, `effective_digest`) and its `max_start_waypoints`, the `Side=` values
(`countries`), the `Color=` values (`colors`), the `campaigns` a single-player launch may start,
and `"generate_map": true` when the build has the random-map command below. A build without it
lacks the field and would start the game for `--generate-map`, so check it first.
`max_offline_houses` is the most houses a launch with one human and no `[Relay]` section seats,
120, as offline Skirmish does: `AIPlayers` up to 119, AI rows `Multi2` to `Multi120`, starts
(`[SpawnLocations]`) and `[HouseTeams]` numbers 0 to 119. Every other launch seats at most 30
houses, with starts and teams 0 to 29. A build that lacks the field seats 30 in every launch.
`"shared_starts": true` says the build reads [shared starts](#shared-starts); a build without it
lacks the field and refuses a shared start.
The game folder comes from `config.toml` unless `--ra2-dir` names one, the mod from `config.toml`
unless `--mod` or `--no-mod` chooses, the Ares setting from VERA.INI unless `--ares` names the
one `spawn.ini`'s `[Settings] Ares=` will, and [RA2 Mode](ra2-mode.md) from VERA.INI or `-ra2`, as
the game takes it; `--generate-map` takes RA2 Mode the same way. In the browser, `probe(map_name)` returns the same JSON
text.

The mode catalog is the selected mod's `[Files] Modes`, or `MPModesMD.ini` by default. The
probe's default is loaded mode 1 when its class supports Skirmish, otherwise the first loaded
Skirmish mode. For a launch selecting another `GameMode`, use that row's digest from
`game_modes`. Custom ids are reported too; if the catalog has no mode 1, the launcher must name
its chosen id explicitly in `spawn.ini`.

### Rules parts

`rules_parts`, beside `rules_digest` and in each `game_modes` row, is that digest in parts, so a
launcher can say what differs when two players' digests do. A build that lacks the field reports
only the digest. The parts never decide a match: games play together when their `rules_digest`s
agree.

```json
{"v":1,"digest":"<16 hex>",
 "files":[{"id":"rules","name":"rulesmd.ini","digest":"<16 hex>","source":"retail","changed":false},
          {"id":"art","name":"artmd.ini","digest":"<16 hex>","source":"loose","changed":true,
           "sections":["General","NUKE"],"sections_more":0}],
 "settings":{"terrain_deformation":false,"superweapon_limit":0,"ares":"auto","ares_mix":false,
             "ini_roots_stock":true,"mod":null,
             "ini":{"VERA":{"Ares":"Auto","Kratos":"Auto","RA2Mode":"no","SuperweaponLimit":"0",
                            "TerrainDeformation":"no","WasmBudget":"0"}}},
 "mod_check":null}
```

- `digest`: the first 16 hex digits of the SHA-256 of the object as compact JSON without `digest`,
  keys in the order shown. The same parts give the same digest on every platform.
- `files`: one part per component of `rules_digest`, in its order: `rules` (the Rules root
  without the Options switches, which `settings` names), `langrule` (only when read), `art`, `ai`,
  `mode` (the mode's INI; none for a mode without one), `theater:<ini name>` for each theater's INI,
  `tiles` (the theater tiles the game folder changes), and with a mod `mod_files` (its folder) and
  `map_additions` (when it has any). Without a mod the theater parts are the copies a retail
  install ships (`source` `retail`, unchanged) and `tiles` is `absent`: the original game leaves
  its folder's terrain files to each match ([Networking](Networking.md)), so they are in no part. `name` is the file name, never a path; `digest` the first 16
  hex digits of the part's digest, or `absent`; `source` `retail` (the original archives), `loose`
  (a file in the game folder), `mix:<archive>` (another archive by its lowercased name, such as `expandmd99.mix`), `mod`
  or `unknown`; `changed` whether it differs from the retail copy. A changed INI names in
  `sections` the sections that differ from the retail copy (added, removed or changed), sorted, at
  most 20, and counts the rest in `sections_more`.
  The `ai` part and `rules_digest` name the game folder's AI INI. A network match that seats no
  computer reads the retail copy instead ([Networking](Networking.md)), so two games that differ
  only in `ai` would play such a match together, though their digests differ: a launcher that
  compares `rules_digest` keeps them apart as before.
- `settings`: the Options switches the game plays (`terrain_deformation`, `superweapon_limit`, and
  `building_production_queue`, which is left out while off and is only ever on in a single-player
  game: a network launch plays it off, as the probe reports it), the Ares setting as the digest
  counts it (`on` or `off` where the setting decides which game opens; `auto` for Auto and for a
  setting that opens the game Auto opens, such as Off without a mod), whether
  `ares.mix` is mounted, `kratos: true` when the startup rules enable Kratos (omitted while off), whether the INI roots listed under `resources` and the theater suffix are
  all YR's (`ini_roots_stock`), `ra2: true` when [RA2 Mode](ra2-mode.md) is on (left out while
  off), and the mod (`title` and `version`, from its `mod.ini`, else its folder's
  name; `version` is empty without one). When a mod requires Ares or the game reads one of these INI roots
  or archives under another name than YR's, `resources` names them as the digest compares them, lowercased:
  `requires_ares`, `roots` (Rules, Art, AI, mode and map catalog) and `archives` (expansion and
  map prefixes, multiplayer and theme archives, movie prefix); otherwise it is left out. The
  sound, EVA, theme, campaign, mission and map-selection roots are in no part: the digest does
  not compare their names.
  When the host enables automation, the rules parts include
  `settings.automation_allowed: true`. The player-facing label is “Allow automation”.
  While the switch is off, the field is omitted.
  `settings.ini` lists the effective launch settings in VERA.INI form, including defaults.
  The `[VERA]` keys are `Ares`, `Kratos`, `RA2Mode`, `TerrainDeformation`, `SuperweaponLimit`
  and `WasmBudget`. A launcher can show a difference by section and key.
  To apply the host's settings, use the player's own probe to determine which section and key
  pairs may be written. Ignore unknown pairs. Accept only single-line scalar strings in the
  format the game writes for that key, and assign each value through an INI writer. Probe
  again and compare `rules_digest` before starting. On leaving the room, restore the player's
  original file entries and remove keys that were originally absent.
  A new build can expose another key through its own probe. Settings from the game folder
  (`ares_mix`) or config.toml (`ini_roots_stock` and `resources`) are reported separately.
- `mod_check`: with a mod that carries a `mod-files.txt`, where its files part from that list
  (`changed`, `missing`, `extra`, at most 20 paths each); otherwise null. It can name a file the
  `mod_files` part leaves out: one that only shows, plays or tells something, such as a `.csf`
  ([Networking](Networking.md)).

The object is at most 8192 bytes: longer lists are cut first, then free text (file names,
sources, the mod's title and version, the `resources` names) to 64 characters, and last the files from the end; `digest`
is always that of the object as sent.

A `-SPAWN` launch whose `ExpectedRulesDigest` differs fails as before, with the reason, and its
`spawn_result.json` (in the browser, the `spawn:finished:` report) adds
`"failure": "rules_mismatch"` and this game's `rules_parts` for the launched mode. A network match
logs its `Rules parts:` when it starts. When a launcher's match is refused at the start, the
game's box says what this game changes from the original game and that the launcher shows the
details.

## Shared starts

In a mode whose players may share a start (Battle, Free for All, ManBattle, Siege, Unholy; not
Cooperative), two or more playing humans may write the same explicit `[SpawnLocations] MultiN`
start. They then control one House together, as players who pick the same start in VERA's own
rooms do ([Networking](Networking.md)). The human with the lowest `SourceIndex` on that start
leads it: the House plays the leader's `Side`, `Color` and team, and the other players' are
ignored. The House counts once wherever houses are counted: the map's starts, the distinct
colours of up to eight houses, the two playing participants a multiplayer launch needs, and the
teams the mode admits. Each player keeps their own `SourceIndex`, relay grant and `ResumePings`
entry, so `PlayerCount` and the 30-participant limit still count every human. An AI never
shares a start, with a human or another AI; a spectator still has none; in Cooperative a
duplicate start is refused as before. Check the probe's `shared_starts` first.

## Random maps

A random-map room carries no map file. It carries the random map's share code (`VR1-…`, the
Generate Map dialog's [Share Code](random-map-extras.md#tools)) and the SHA-256 of the file the
host made from it. Every player makes the file from the code, checks the digest, and plays the file
as an ordinary map, with that digest as `ExpectedMapDigest`.

```sh
vera20k --generate-map (--code <VR1-…> | --sed <file>) --output <map-file> [--mod <folder> | --no-mod]
```

The input is a share code, in either case, or a `.SED` file whose `[RandomMap]` section holds the
options:

| Key | Values |
| --- | --- |
| `Width`, `Height` | the same value: 0 to 3 for Small to Very Large, 4 to 7 for 160, 250, 350 and 500 cells |
| `NumPlayers` | 2 to 120; a map for more than 30 plays offline |
| `Seed` | 0 to 65535; required |
| `MapType` | 0 archipelago, 1 continent, 2 team continent, 3 inland, 4 mountainous |
| `Theater` | 0 temperate, 1 snow |
| `VeraFair` | `yes` for Fair Starts |
| `Ruggedness`, `WaterAmount`, `Tiberium`, `TiberiumLayout`, `Vegetation`, `Resources`, `Time`, `Accessibility`, `RegionSize` | optional, as the dialog's sliders |

Out-of-range numbers are clamped as the dialog clamps them. Another theater, Urban Presence
(`VeraUrban`) and the [Extras](random-map-extras.md) are refused for now, and so is a map that
cannot place every player. Generation uses the same default mode and its rules as the probe.
If that mode disallows random maps, generation reports the mode id and refuses the request.

It writes the map to `--output`, replacing that file only once the new one is complete: a
multiplayer map named `Random Map <seed>` with its preview and start header. Then it prints

```json
{"code":"VR1-…","sha256":"<64 hex digits>","byte_length":123456,"seed":4242,"players":2}
```

`code` is the options' code as this build writes it, `sha256` the file's digest in lowercase hex,
`seed` the options' seed (Fair Starts may have built the map from a later one), and `players` the
starts the map holds, which is the options' player count.

The file depends only on the options, this build's generator and the game's files, so the same
input makes the same bytes on Windows, macOS, Linux and in the browser. A mod's files would change
it: rooms pass `--no-mod`. VERA.INI's Ares setting and Options switches are not read. Another VERA
version may make another map from the same code, which the digest check then reports.

In the browser, `generate_map(input)` takes the code or the `.SED` text and returns a promise of
`{ report, bytes }`: `report` is the JSON line above and `bytes` the file. It reads the retail files
imported with `import_file`, never a mod, and rejects with an `Error` saying what is wrong. Call it
before a launch or between launches, as `probe`. On the threaded package, once the host has awaited
`initThreadPool`, it runs on a pool worker and the page's thread goes on; otherwise it runs on the
page's thread, which it holds until the map is made.

## Watching

A launcher can start a game that watches a match being played, from its first frame, without
taking part in it. Its `spawn.ini` is the one the human with the lowest `SourceIndex` (source 0)
gets, with the same `[Settings]`, `[OtherK]`, AI rows, `[IsSpectator]` and teams, and
`PlayerCount` counting every human, spectators included. Only `[Relay]` differs:

```ini
[Relay]
Address=203.0.113.5:5000
MatchId=<the match's UUID>
PlayerId=<the watcher's UUID>
Ticket=<a version 3 watch ticket, base64url>
PacketKey=<base64url>
PayloadKey=<the match's payload key>
OrderingCertificate=<the match's certificate>
ServiceUrl=https://match.example.com
Token=<the watcher's watch token>
GatewayUrl=wss://gateway.example.com   ; optional
AdmissionTimeout=60                     ; optional
```

The ticket alone makes the game a watcher (relay README "Watchers"): a player's ticket starts a
player, byte for byte as before. `ResumePings`, `ResumeCursor` and `Federation` are refused with a
watch ticket: a watcher joins one relay and is nobody returning. The ticket and certificate must be
`MatchId`'s, and the certificate must name all `PlayerCount` humans.

A match of one human against AI is watchable only when the relay orders it: its certificate must be
a `VO2C` marked ordered alone (`OrderingCertificate::ordered_alone`, flags bit 1, see
[Watching](Networking.md#watching)). Without the mark the player's game runs its commands locally,
the relay never starts the match, and a watcher's launch fails once its admission wait runs out.
With it the player plays through the relay as in any relay match, and its watcher, whose
connections hold no other source, takes source 0's seat as always.

The watcher's world stands in source 0's seat, because a world needs a current House, but the
watcher is none of the players: it starts on the observer's sidebar, which lists every player and
sees the whole map, and Tab moves it to each player's view and back, as the
[online lobby's watcher](Networking.md#watching) does. It controls nothing: its orders are dropped
and it sends nothing into the match (`SequencedSession`'s watch). On the observer's sidebar it
hears no House's verdict.
Its relay lets it in once the match has started and is journaled; until then its handshake is
left unanswered and the game tries again for up to 240 seconds (or `AdmissionTimeout`, if
longer), then fails the launch. It then reads the match from its first frame at speed until it
has caught up, and plays on with the players.

`Token` is only for renewing the watch ticket, which, like a player's, lives at most 300 seconds:
once the relay has let the watcher in, then 150 seconds after each grant, and every 10 seconds
until one comes, the game asks
`GET {ServiceUrl}/v1/events?after=<cursor>` with it, which answers with a `watch_renewed` event,
`{"match_id": "…", "relay": {"address", "ticket", "packet_key", "payload_key",
"ordering_certificate"}}`. A watcher sends nothing else to the match service: no command, report,
`finish`, `leave` or `logout`, and it ignores any other event. A refusal (401, 403, 409
`unknown_match` once the match ended, any 4xx but 408 and 429) stops its polls for good; a network
failure, 408, 429 or 5xx is tried again.

The game ends by itself, with no score page, once the match is decided or every player has left
it, or when the watcher closes it (Escape). Its `spawn_result.json` says `"watch": true`, with
`result` always `quit` and no `source_index`. Its log names the launch `Relay spawn: … watch=true`,
says `External launch: match on screen at frame N` once the match is on screen and then
`External launch: frame N` every 900 frames, which a watcher passes quickly while it catches up.

`tools/spawn/run.sh relay` with `WATCHERS=N` starts N such watchers beside its players once p0's
match is on screen ([tools/spawn/README.md](../tools/spawn/README.md)).
