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
dashboard asset directory when running outside the checkout root. Live snapshot
publishing and the render/ledger meters start with the first authenticated
client; the whole-match summary ring (1 Hz) and the report-grade ring (5 s) are
recorded whether or not one is attached, and arrive with the first publish after
a client connects. The newest report sample rides every snapshot.
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

## Whole-map spectator view

The fifth header tab, **whole map**, opens a full-screen live picture of the
entire battlefield. It is a spectator mode, not a second camera: while a viewer
is subscribed, the tactical camera is fixed at the map's corner, shroud and fog
are lifted, and the sidebar is not drawn. The game window shows only a one-line
notice — it does not draw the player's view. The simulation keeps ticking at
its normal rate, so this suits watching an AI match; a human cannot play while
it is on. Closing the tab restores the camera, zoom and visibility the player
had before.

The whole map is rendered **in one frame** into a fixed **1920x1080** target,
then cropped to the map's visible area. A 4560x2295 map is about 2:1, so it is
fitted with a uniform zoom; the frame is cut to the `LocalSize` rectangle, which
is what retail lets the camera reach, so the picture has no black band top or
bottom (1920x797 for that map). The production wheel-zoom path renders the world
at native scale into an enlarged source and resamples it into that picture;
keeping this standard path matters, because voxel turrets, surface lines and the
Z-depth passes all assume one world pixel per source texel. Every streamed frame
is one complete, coherent picture as a single binary WebSocket frame — there is
no tile scan and no progressive refresh, so moving units are continuous rather
than a slideshow.

The picture is captured asynchronously (two readbacks in flight), swizzled and
published on a worker thread, so a 6 MB frame never sits in the render loop's
budget. Measured at 1920x797 with two Hard AI: **59.7 pictures/s, 366 MB/s, 0
malformed frames, render loop 59.4 fps**. `VERA_SPECTATOR_FPS` (1..=240, default
90) caps the rate without a rebuild, but the simulation tick (60/s) is the real
ceiling — a higher cap only repeats the same tick. Screen-fixed UI (message rows,
outcome banners, tooltips, timer lines, the software cursor and the sidebar blit)
is suppressed for these frames: it is anchored to the window, not the world.

### Live audio

The engine's music and SFX players share one mixer, and its samples are both
played and copied into a lock-free PCM ring. A pump thread publishes **40 ms
blocks of 48 kHz stereo PCM16** (~188 KiB/s) to subscribed dashboards, so the
viewer hears exactly what the machine outputs: no second soundtrack, no
re-encoding. The stream is separate from the picture stream — audio blocks are
tens of kilobytes and take priority over a megabyte-sized frame — and blocks are
dropped rather than queued when a viewer falls behind, which keeps the sound
current instead of late. Audio follows the tab: opening **whole map** subscribes,
leaving unsubscribes, and the toolbar has a mute checkbox and a volume slider.
The context is created from the click that opens the view, which is the gesture
browsers require before audio may start.

The tap is an addition, not a requirement: if the device refuses the shared
output, both players fall back to their own rodio sinks and the picture stream is
unaffected. `tap_output` keeps the same 512-frame period the SFX path used, so
short `CreditTicks` cues still reach the device before the arbiter stops them.

### Recording a match

The **record match** button captures the visible canvas (30 fps) plus the same
mix the speakers get, and saves one `.webm` when stopped — VP9/Opus, falling back
to VP8. It is a whole-match recording in the sense that matters: start it when
the match starts, stop it when the match ends, and the file needs no
post-processing. MediaRecorder runs in the page, so no video encoder ships with
the engine; the cost is that the tab must stay open for the whole capture.

The engine does none of this work when no viewer is attached. The mode starts
only during an in-game match without an Options zoom pass. It changes
presentation state only: it does not pause, step, or mutate the world.

Collecting world
instances for the entire map in a single frame raises `render.world_ms` from
~1 ms to ~4.7 ms, which the asynchronous readback and the picture worker keep off
the loop's critical path.

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

## Startup parameters and usage

### Command line

| Switch | Effect |
| --- | --- |
| `-CD` | Use the wildcard media-archive branch. |
| `-NOAUDIO` | Disable music and sound output. |
| `-?`, `-h`, `/?`, `/h` | Print usage and exit. |
| `-WIN` | Accepted; no effect. Full screen is a choice on the Options page. |
| `-<W>X<H>`, `-480`, `-16`, `-RECORD`, `-PLAY`, `-SOCKET`, `-DESTNET`, other net/debug flags | Accepted so a launch never fails, not applied yet. |

### Environment

| Variable | Value | Effect |
| --- | --- | --- |
| `RA2_DIR` | retail install path | Used by the tool binaries, headless benchmarks and tests. The game itself reads `config.toml` `[paths] ra2_dir` from its working directory (or beside the executable), not this variable. |
| `RA2_QUICKPLAY` | map name or path (`.map`/`.mpr`/`.mmx`), or a `.sed` seed | Load straight into that map through the authored-map sandbox: one local house, no generated opponents and no starting forces. |
| `RA2_DEBUG_SPAWN_UNITS` | `TYPE[:COUNT],...`, or `1` | Spawn units at load on the nearest free cells. A bare type is one unit; `=1` keeps the default list `HTNK,MTNK,E1`. |
| `RA2_DEBUG_SPAWN_MARCH` | `1` or `rx,ry` | Order every spawned object to attack-move across the map (`1`) or to one cell. |
| `VERA_TELEMETRY_ADDR` | `127.0.0.1:9741` | Start the read-only dashboard server on that loopback address. |
| `VERA_TELEMETRY_WEB` | directory | Dashboard asset root when running outside the checkout. |
| `RA2_DEBUG_DEPTH_VIEW`, `RA2_DEBUG_BRIDGE_RENDER_BUCKETS`, `RA2_DEBUG_CAMEO_PALETTES`, `RA2_DEBUG_MOUSE_CURSOR_SHEET`, `RA2_FORCE_TIB3_TO_TIB01`, `RA2_TIB_ID_OFFSET`, `RA2_FORCE_TIB_IMAGE`, `RA2_NORMALS`, `RA2_NORMAL_COUNT`, `RA2_QUEUE_FRAME_MS`, `RA2_DUMP_SHROUD`, `RA2_ENABLE_LAT`, `RA2_DISABLE_LAT` | — | Render and diagnostic toggles; not needed for ordinary runs. |

A scale run on a base-less map needs one `[General] BaseUnit=` type in the spawn
list (stock YR: `AMCV`, `SMCV`, `PCV`), or the defeat gate ends the match about
half a minute in.

### Examples

20,000 units marching on the flat 500x500 scale map:

```sh
python -m tools.scale_map target/scale/flat500.map --starts 8
python -m tools.cargo_run -- build --locked --profile fast -p vera20k   --features telemetry-dashboard --bin vera20k
game=$(python -m tools.cargo_run --resolve vera20k --profile fast)
RA2_QUICKPLAY="target/scale/flat500.map" RA2_DEBUG_SPAWN_UNITS=AMCV:1,HTNK:10000,MTNK:10000 RA2_DEBUG_SPAWN_MARCH=1 VERA_TELEMETRY_ADDR=127.0.0.1:9741 "$game"
```

Headless simulation benchmark (no window, no dashboard):

```sh
cargo build --release -p vera20k-core --bin sim-bench
target/release/sim-bench --scenario march --units 20000 --houses 30
```

### Recording semantics

The timeline on the match report page is tick-based, not wall-clock based.
Whole-match summaries close every 15 ticks (one second at the retail 15 Hz
logic rate); report-grade samples close every 75 ticks (five seconds) and each
one renders the same report as the live view. A report sample closes the
ledger window, so the live phase means cover at most the last five seconds and
briefly fall back to the window the sample just closed. Both rings are
app-owned, never read by the simulation, and are not persisted in saves.

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
controls or gameplay mutations. The whole-map picture is a presentation-only
spectator stream: it shows the whole visible map without shroud or sidebar,
not the player's current viewport, and it advances one tile per rendered
frame.

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
