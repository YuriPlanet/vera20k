# Local multiplayer spawn runs

Two desktop clients on loopback, launched with `vera20k -SPAWN` from per-player
directories (docs/spawn.md). Nothing here is used by ordinary play.

* `spawn-e2e.mjs direct` writes `OUT/p0`, `OUT/p1`: `spawn.ini` (ports 1300,
  1301 on 127.0.0.1), the map as `spawnmap.ini` (default `tests/maps/match.map`)
  and a `config.toml` whose `ra2_dir` is the retail directory you pass and
  whose settings stay in that player directory. Machine paths never enter the
  repository.
* `spawn-e2e.mjs relay` writes the same with a `[Relay]` section and then
  serves the match API subset of docs/spawn.md on `127.0.0.1:8787` (events
  cursor, `relay_ready`/`latency_ready`/`renew_relay`/`finish`/`leave`,
  `match/resume`; `start_confirmed`, `timing_confirmed`, `relay_renewed`).
  Grants and ordering certificates are signed by `relay_grant.mjs` in the
  lobby's format (docs/lobby-protocol.md): `VO1C`, or `VO2C` with
  `--pacing on|off|continuous` (`run.sh`: `PACING=on|off|continuous`). Results go to `OUT/results.json`.
* `SLOW=K` makes source K a slow PC. The spawn-only switches
  `VERA20K_SPAWN_SLOW_DRAW_MS` and `VERA20K_SPAWN_SLOW_TICK_MS` (`run.sh`:
  `SLOW_DRAW_MS`, `SLOW_TICK_MS`) add that many milliseconds to each drawn
  picture and to each simulation tick throughout, the tick's inside its
  reported cost. The switch `VERA20K_SPAWN_SLOW_PLAN` (`run.sh`:
  `SLOW_PLAN`), which may be set as well (tick delays add up), is
  `second:draw_ms:tick_ms[:unreported_ms]` points, comma separated, from the
  match's start; that many milliseconds go on each frame pass (drawn or not,
  so hidden windows pay it too) and each simulation tick (inside its
  reported cost), and the optional last on each tick outside that cost (a
  PC whose ticks look quick but run slowly, as 1e9fac21's did), linear
  between points and held after the last. `0:0:20` is a PC whose
  simulation is slow throughout; `0:0:0,300:0:3:12` one that slows as the
  match grows. Catching up runs the same ticks, so it slows too.
  `SPEED=0` plays at 60 frames a second. Each client's `External launch result`
  line carries its `network_quality` (catch-up seconds, largest backlog,
  slowdowns).
  `run.sh` takes `RELAY_PORT` and `SERVICE_PORT` when 5000 or 8787 is in use.
* `run.sh relay` with `WATCHERS=N` also writes `OUT/w0..` (docs/spawn.md "Watching"): source 0's
  file with a watch ticket and a watch token, which the mock answers on `/v1/events` with
  `watch_renewed` and refuses everywhere else. The watchers start `WATCH_AFTER` seconds (default
  0) after p0's match is on screen, and `run.sh` waits for them and prints their results too.
  A watcher's log says `Relay spawn: … watch=true`, then `External launch: frame N` lines; the
  mock logs each `watch renewal`.
* `AI=N` adds N computer houses after the humans; in relay mode `PLAYERS=1 AI=N` is one person
  against AI, and `ALONE=on` signs its certificate as a `VO2C` the relay orders alone, so the
  match runs through the relay and can be watched (`WATCHERS=1`).
* `run.sh` builds nothing; it starts the relay (relay mode) and the mock, then
  both clients, waits for both processes to exit, and prints each client's
  exit status and `spawn_result.json` (docs/spawn.md "Results"; exit status 0
  `quit`, 1 `failed`, 10 `won`, 11 `lost`, 12 `draw`, 13 `disconnected`).
  The mock records `finish.result` and `finish.opponents_left`.

* `run.sh browser` is relay mode with browser players: it also starts the
  WebSocket gateway (`tools/wasm/gateway`, forwarding to the local relay) and
  writes `[Relay] GatewayUrl`, then `browser.mjs` serves the online package
  (`target/wasm-dist`, or `VERA20K_WASM_DIST`), the retail files and a test
  host page (`browser-host.js`) on `http://localhost:8000` and opens one
  headless Chrome per player (`CHROME` overrides the macOS Chrome path). Each
  page imports the retail folder, starts the spawn and calls `spawn_leave()`
  ACT seconds into the match (source 1; the others ACT+BOUND). Logs:
  `OUT/pN/browser.log` (console), `OUT/pN/events.jsonl` (lifecycle events,
  whose `match-ended` carries `network_quality`), `OUT/gateway.log`. Exit
  status 0 means every page ended its match and exited. A `--threads`
  package is served cross-origin isolated and starts its worker pool;
  `SIM_THREAD=1|always`, or one value per source (`always,0`), turns on its
  pages' simulation worker (`set_simulation_thread`, docs/sim-thread.md
  "The browser"); it is off by default.
* `BROWSERS=1` (browser mode) makes only those sources pages and starts the
  others as desktop clients: a mixed room. The desktop build must match the
  package's features (`--no-default-features --features wasm-net`, set with
  `VERA20K_BIN`), or the relay refuses the start (`AgreementMismatch`).
  `MOD=FOLDER` (any mode) copies a mod folder into every player's `Mods` and
  plays it; pages import it (`import_mod_file`, `set_mod`). The pages'
  summary goes to `OUT/browsers.log`.

Clients never show a window: `run.sh` sets `VERA20K_SPAWN_HIDDEN_WINDOW=1`,
which (for `-SPAWN` launches only) creates the window invisible, keeps the
event loop servicing it as if shown (macOS otherwise parks an occluded
shell/loading screen), and on macOS gives the process the `Prohibited`
activation policy (no Dock icon or menu bar). Never run these clients without
it on a shared screen. `python3 -I tools/spawn/winwatch.py SECONDS`, run
beside `run.sh`, samples the CoreGraphics on-screen window list for the client
PIDs and prints any visible window.

Two more test hooks time a match instead of testing its network, and are read
by every `-SPAWN` launch, one human against computer houses included
(`python -m tools.scale_bench client` uses both,
[the scale benchmark](../../docs/scale-benchmark.md)):

* `VERA20K_SPAWN_LEAVE_AT_FRAME=N` presses Leave once the match has reached
  simulation frame N, then Continue on the score page, so the game ends by
  itself after a set stretch of the match.
* `VERA20K_SPAWN_FRAME_COST=N` counts, from simulation frame N on, what the
  match costs the game's main thread, in that thread's own CPU time, so a wait
  for the display or for a tick on the simulation thread costs nothing.
  `spawn_result.json` then carries `frame_cost`: `draw_ms` (the mean drawn
  picture, with `draw_p50_ms` and `draw_p99_ms`), `service_ms` (the match
  service of one simulation frame: the finished tick's tail and the frame and
  world copy the next tick leaves for drawing, without any tick it ran
  itself), `frame_ms` (their sum: one picture of one simulation frame),
  `tick_ms` and `tick_p99_ms` (a simulation frame's own wall time, on
  whichever thread ran it), the counts `pictures`, `ticks` and `lent_ticks`
  (ticks run on the simulation thread), and the frames it spans
  (`first_frame`, `last_frame`).
  It also reports `present_p99_ms` between battlefield hand-offs
  and `intermediate_pictures` showing interpolated unit movement. The latter includes
  `jumpjet_pictures` and `fly_pictures` for voxel aircraft or vehicles, and
  `infantry_pictures` for hand-offs with a submitted infantry sprite origin strictly
  between its endpoints. The same positional test counts `ship_pictures`,
  `hover_pictures` and `shp_drive_pictures` for vessels, hover units and
  SHP Drive vehicles. At native zoom those origins are whole pixels. `fewest_mobile` gives the lowest live mobile population
  sampled every 100 logic frames. `buffered_voxels` counts unkeyed stable body draws
  that use the native shared voxel depth. `atlas` reuses an existing colour
  image, `gpu` paints the accepted stores, and `cpu` uses the software raster.

The test hook `VERA20K_SPAWN_LEAVE_AFTER_SECONDS=N` presses the abort box's
Leave N seconds after the match appears. It is read only by external
multiplayer launches (`-SPAWN` with `PlayerCount>1`). `run.sh` gives source 1
the requested delay and source 0 the delay plus 60 s as a bound.

## Commands

```sh
export PATH=$HOME/.cargo/bin:$PATH
python3 -m tools.cargo_run -- build --profile fast -p vera20k --bin vera20k
python3 -m tools.cargo_run -- build -p vera20k-relay
RA2=/path/to/Yuri_s_Revenge_1.001
tools/spawn/run.sh direct /tmp/spawn-direct "$RA2" 40 300
tools/spawn/run.sh relay  /tmp/spawn-relay  "$RA2" 40 300
# Browser players: the online package and the gateway's dependencies first.
python3 tools/wasm/build.py --network && npm --prefix tools/wasm/gateway ci
tools/spawn/run.sh browser /tmp/spawn-browser "$RA2" 40 400
```

Logs: `OUT/pN/logs/ra2.log` (client), `OUT/pN/stdout.log` (exit status),
`OUT/pN/spawn_result.json` (launch result), `OUT/service.log` (mock),
`OUT/relay.log` (relay).

Probe (headless, never opens a window; compare both players' output):

```sh
BIN=$(ls -t target/owned-worktrees/*/fast/vera20k | head -n 1)
(cd /tmp/spawn-direct/p0 && "$BIN" --probe --map spawnmap.ini)
(cd /tmp/spawn-direct/p1 && "$BIN" --probe --map spawnmap.ini)
"$BIN" --probe --ra2-dir "$RA2" --map tests/maps/match.map
```

A launcher can copy `version`, `rules_digest` and `map.source_sha256` into
`[Settings] ExpectedVersion`, `ExpectedRulesDigest`, `ExpectedMapDigest`.
`countries` lists the `Side=` values with what the skirmish menu shows for each:
`name` (the `[Countries]` ID), `label` (its `UIName=` text), `side` (its `[Sides]`
index) and `selectable` (the menu offers it), so a launcher can offer a mod's
countries as the game does.
`rules_digest` is Battle's. A network launch plays each player's own Terrain Deformation
and Superweapon Limit, which the digest covers, and never the Building Production Queue. Without these keys, players whose
builds or rules differ still never play together: a relay launch ends at its start
with `agreement_mismatch`, a direct one as the launcher notes below say.
`[Settings] Ares=Auto|On|Off` replaces the player's [Ares setting](../../docs/ares.md#the-ares-setting)
for that launch (an unknown value plays Auto); the probe does not read spawn.ini, so
pass the same with `--ares <Auto|On|Off>`, or it reports VERA.INI's.
`[Settings] GameMode` selects an `MPModesMD.ini` row by id (default 1): a network
launch takes a stock row, and a one-human launch any row of the `MPModesMD.ini` the
game loaded, or row 1 when the id has none, so a
mod's own modes start too. The digest covers that row's
INI, so another mode takes its digest from the report's `game_modes`, which lists
the stock rows. A mod chosen in `config.toml`
(`[paths] mod`, [modding guide](../../docs/modding.md)) is read by the probe as by
the launch, so its rules and its other files are in the digest; `--mod <folder>` and
`--no-mod` choose for both. A launch whose mod cannot be used fails, with the reason in its result.
Cooperative plays one mission: the launcher
writes its enemy AI houses, and no next mission or progress follows.

Useful log lines: `Direct spawn: source=`, `Direct spawn: every guest answered`,
`Relay spawn:`, `Spawn test hook: pressing Leave`, `Match end ready for local
player`, `External launch: every opposing participant has left the match`,
`External launch result: {…}`, `External launch failed`, the mock's
`finish from source N: result=… opponents_left=…`.

## Launcher notes (direct mode)

Only `[Settings] Port` must be reachable (UDP) for every other participant;
VERA binds its alternate receive socket on an ephemeral port because no VERA
traffic uses it. Each participant must start within about 29 s of the others
(the LAN setup lease); a participant whose spawn.ini differs in any shared
field, or whose own Terrain Deformation or Superweapon Limit differs, derives
another session scope and is treated as not responding. One that shares the
scope but runs another VERA20k build or other rules files ends the launch at
once, naming that participant in `spawn_result.json`. `ExpectedVersion` and
`ExpectedRulesDigest` refuse before any of this, with the reason.

## Recorded runs (2026-10-28, macOS, hidden windows)

* direct: both clients loaded `match.map`, entered the match, source 1 left
  after 30 s (source 0 logged `Network connection destroyed: source=1 …
  native_frame=781`, i.e. 781 shared lockstep frames), source 0 left 60 s
  later, both exited 0. winwatch: 169 samples, 0 visible.
* relay: `relay_ready` ×2 → `start_confirmed`, `latency_ready` ×2 →
  `timing_confirmed`, both in game, source 1 left after 30 s (source 0:
  `native_frame=897`), `finish` `result=quit` from both, both exited 0; relay
  forwarded 4653 datagrams with no rejections. winwatch: 163 samples, 0
  visible.

## Recorded runs, Phase 4 (2026-10-28, macOS, hidden windows)

* direct: source 1 left after 40 s (`frames` 1010), source 0 logged
  `Network connection destroyed: source=1 … native_frame=1011` and
  `External launch: every opposing participant has left the match`, left at
  100 s (`frames` 2455). Both exited 0 with `spawn_result.json`
  `result=quit`; `opponents_left` true for source 0, false for source 1.
* relay: same sequence; the mock logged `finish from source 1: result=quit
  opponents_left=false` and `finish from source 0: result=quit
  opponents_left=true`; both exited 0. winwatch: 0 visible samples in both runs.
* `--probe --map spawnmap.ini` in all four player directories printed
  byte-identical JSON; a rejected spawn.ini and an `ExpectedMapDigest`
  mismatch each exited 1 with `result=failed` and the reason.
