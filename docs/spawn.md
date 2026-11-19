# External launches

A launcher outside the game starts a match with `-SPAWN` and a `spawn.ini`. Before it does, two
headless commands answer what that launch needs: the probe says what a launch on this
installation is checked against, and the random-map command makes the file of a random-map
room. Each runs in a folder whose `config.toml` names the game folder, opens no window, prints one
JSON line on stdout and exits 0; on failure it exits non-zero with the reason on stderr. The
browser package exports both as functions ([Wasm guide](wasm.md)).

## Probe

```sh
vera20k --probe [--ra2-dir <dir>] [--map <map-file>] [--mod <folder> | --no-mod] [--ares Auto|On|Off]
```

It reports the multiplayer `version` (`[Settings] ExpectedVersion`) and its parts
(`build_identity`), the Battle mode's `rules_digest` (`ExpectedRulesDigest`) and each `GameMode`'s
in `game_modes`, the `map` given (`file`, `byte_length`, `source_sha256` for
`ExpectedMapDigest`, `effective_digest`) and its `max_start_waypoints`, the `Side=` values
(`countries`), the `Color=` values (`colors`), the `campaigns` a single-player launch may start,
and `"generate_map": true` when the build has the random-map command below. A build without it
lacks the field and would start the game for `--generate-map`, so check it first.
`max_offline_houses` is the most houses a launch with one human and no `[Relay]` section seats,
120, as offline Skirmish does: `AIPlayers` up to 119, AI rows `Multi2` to `Multi120`, starts
(`[SpawnLocations]`) and `[HouseTeams]` numbers 0 to 119. Every other launch seats at most 30
houses, with starts and teams 0 to 29. A build that lacks the field seats 30 in every launch.
The game folder comes from `config.toml` unless `--ra2-dir` names one, the mod from `config.toml`
unless `--mod` or `--no-mod` chooses, and the Ares setting from VERA.INI unless `--ares` names the
one `spawn.ini`'s `[Settings] Ares=` will. In the browser, `probe(map_name)` returns the same JSON
text.

## Random maps

A random-map room carries no map file. It carries the random map's share code (`VR1-…`, the
Generate Map dialog's [Share Code](random-map-extras.md#tools)) and the SHA-256 of the file the
host made from it. Every player makes the file from the code, checks the digest, and plays the file
as an ordinary map, with that digest as `ExpectedMapDigest`.

```sh
vera20k --generate-map (--code <VR1-…> | --sed <file>) --output <map-file> [--no-mod]
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
cannot place every player.

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
