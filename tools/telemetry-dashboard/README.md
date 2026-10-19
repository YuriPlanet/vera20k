# Telemetry dashboard and offline debug build

The dashboard observes a running VERA20k match: populations, production, AI
teams, map positions, selected paths, frame timing and process logs. The dashboard
design and initial implementation were contributed by
[1372704087](https://github.com/1372704087) in
[PR #2083](https://github.com/YuriPlanet/vera20k/pull/2083).

## Run

From the checkout root, with the ordinary retail data setup:

```sh
python -m tools.cargo_run -- build --locked --profile fast -p vera20k --features telemetry-dashboard --bin vera20k
VERA_TELEMETRY_ADDR=127.0.0.1:9741 "$(python -m tools.cargo_run --resolve vera20k --profile fast)"
```

Open the local URL printed to stderr. Its fragment contains a fresh session
credential. The page consumes and removes that fragment; reloading needs the
original printed URL again. Treat the URL as access to the current match and its
logs. The server requires the matching local Host, Origin and credential, serves
only fixed dashboard assets, and the dashboard-only build accepts no game commands. Non-loopback binding
is rejected. Close the game to revoke the session.

The feature is off by default and absent from ordinary release builds. Without
`VERA_TELEMETRY_ADDR`, it starts no server. `VERA_TELEMETRY_WEB` can select the
dashboard asset directory when running outside the checkout root. CPU timing and
snapshot capture run only while an authenticated dashboard client is attached.
The existing `dev-overlays` feature and build profile are independent choices;
an optimized build can include these diagnostics without a separate debug branch.
The runner supports building and resolving the executable separately; it does not
accept `cargo run`.

On PowerShell, after the same build command, launch with:

```powershell
$env:VERA_TELEMETRY_ADDR = "127.0.0.1:9741"
$game = python -m tools.cargo_run --resolve vera20k --profile fast
& $game
```

## Offline trainer and event hooks

Use the same current `main` checkout, with the explicit extra feature:

```sh
python -m tools.cargo_run -- build --locked --profile fast -p vera20k --features debug-tools --bin vera20k
```

Then launch as above. `debug-tools` includes `telemetry-dashboard` and the existing
developer overlays. It enables the **Offline debug tools** panel only for a live
offline campaign or skirmish with an authenticated dashboard viewer. LAN,
Internet, unknown session modes and replay playback do not admit trainer writes.
Closing the last dashboard disconnects its controls and clears active switches.
New worlds and loads also reset the switches and invalidate queued commands.

The trainer restores the original contributor's money, power, instant-build,
invincibility, damage, technology, placement, reveal, heal, veteran and superweapon
controls. The inspector records at most 32 selected entities, with 64 bounded
events per entity; logging is off by default. History observes changes around
each object turn, not every internal transition. Browser commands are limited in
size, rate and queue length, and refer to the current world session.

These controls intentionally change gameplay. Affected health, money, completed
production and other results can persist in saved games even though the control
switches do not. Before the first trainer mutation, the app closes its ordinary
diagnostic replay segment and stops recording that modified world. Such a run
cannot serve as a normal replay or parity/performance baseline. Turning the
switches off does not undo previous actions. Save files do not retain a trainer
provenance marker: loading one resets the controls and starts a new diagnostic
segment, but does not make the saved state a clean gameplay baseline.

| Build | Features | Trainer and event hooks |
| --- | --- | --- |
| Shipped game | `--no-default-features` | Absent |
| Local development | Default `dev-overlays` | Absent |
| Read-only dashboard | `telemetry-dashboard` | Absent |
| Offline debug tools | `debug-tools` | Explicitly included |

The existing `fast` profile keeps release optimizations for interactive use;
these are optional development features, not a permanent fork of the engine.

For a synthetic demonstration without retail data:

```sh
python -m tools.cargo_run -- build --locked --profile fast -p vera20k --features telemetry-dashboard --bin telemetry-serve
"$(python -m tools.cargo_run --resolve telemetry-serve --profile fast)" --ticks 300
```

This fixture animates artificial entities. Its timing is not a gameplay benchmark.

## Limits and interpretation

Snapshots update at most ten times per second, including while paused. Population
totals cover the complete world; displayed detail is capped at 32 houses, 20,000
map objects, 16 roster types per house, 8 production lines, 16 AI teams and 32 base
nodes per house. Selected-object and cursor details each show at most 32 objects.
Names, log records, connections and network buffers are bounded. A slow client
may skip snapshots. Logs are a shared incremental stream; new tabs receive future
batches, not a complete historical replay.

Timing describes the current instrumented run and includes instrumentation cost.
Some frame work remains grouped under its existing owner rather than split into
new hooks. CPU share is normalized to all logical cores. The GPU memory field
counts the engine's palette-index atlas pages, not total driver allocation.
PNG and HTML exports are generated locally in the browser. No external services
are used. The dashboard-only feature includes no trainer, remote pause/step
controls or gameplay mutations.

The full original contribution from
[PR #2083](https://github.com/YuriPlanet/vera20k/pull/2083) is preserved unchanged
in a local maintainer Git archive for future integration. The optional debug
build adapts that work to current state owners and the session, replay and
transport rules above.

## Frontend changes

```sh
cd tools/telemetry-dashboard
npm ci --ignore-scripts --no-audit --no-fund
npm run build
```

Commit the TypeScript and generated JavaScript together. The manual Dashboard
workflow checks they agree. Vendored Three.js r180 is unchanged upstream code;
its license is in `vendor/THREE-LICENSE.txt`.
