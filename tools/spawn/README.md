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
  Grants and ordering certificates are signed by `relayGrant` imported from
  `services/lobby/service.js`. Results go to `OUT/results.json`.
* `run.sh` builds nothing; it starts the relay (relay mode) and the mock, then
  both clients, waits for both processes to exit, and prints each client's
  exit status and `spawn_result.json` (docs/spawn.md "Results"; exit status 0
  `quit`, 1 `failed`, 10 `won`, 11 `lost`, 12 `draw`, 13 `disconnected`).
  The mock records `finish.result` and `finish.opponents_left`.

Clients never show a window: `run.sh` sets `VERA20K_SPAWN_HIDDEN_WINDOW=1`,
which (for `-SPAWN` launches only) creates the window invisible, keeps the
event loop servicing it as if shown (macOS otherwise parks an occluded
shell/loading screen), and on macOS gives the process the `Prohibited`
activation policy (no Dock icon or menu bar). Never run these clients without
it on a shared screen. `python3 -I tools/spawn/winwatch.py SECONDS`, run
beside `run.sh`, samples the CoreGraphics on-screen window list for the client
PIDs and prints any visible window.

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
field derives another session scope and is treated as not responding.

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
