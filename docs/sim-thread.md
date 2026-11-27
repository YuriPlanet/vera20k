# Simulation thread

Goal: run the simulation on its own thread so that drawing keeps its frame
rate when a tick costs more than the frame budget. In a 1v29 skirmish a tick
at about 2,000 objects costs about 25 ms against a 22 ms step. When the tick
and the draw shared the main thread, the frame rate fell to about 30.
With its own thread, a slow simulation slows only game time: the camera,
the cursor and the interface keep the picture rate, while objects move once
a tick, as they always draw their current state (`RenderPose`).

This changes no gameplay. Every phase must keep `sim-bench --frame-hashes`
identical frame by frame.

## Where the main thread coupled

The starting point, before phase 0; the phases below take each apart.

1. **Frame loop** (`app/frame.rs` `render_frame`). Each frame it prepares
   interpolation, then runs `service_match_runtime`, then draws. Catch-up runs
   up to 8 ticks or 8 ms per frame (`frame_pacer.rs` `permits_catch_up`). A
   hidden window keeps ticking without drawing (`handler.rs` `about_to_wait`).
2. **Mid-tick client effects** (`sim/world/frame_effects.rs`, implemented by
   `app/match_runtime/frame_effects.rs` `FrameClientBoundary`). These run in
   native order *inside* a tick, and they cannot become post-tick events:
   - `SoundBoundary` plays the reached sound prefix at once. Gattling Stop must
     finish before Report's Main draw, as in gamemd, where sound and
     simulation share one Main RNG.
   - `OutcomeWaitBoundary` lets `HouseClass::Update` (`4F860F`/`4F8736`)
     wait for the EVA voice. The frame suspends until Vox is idle.
   - `LocalPlanningBoundary::leave_planning` returns a command into the
     running frame (`House4FC1EE`).
   - `NetworkCommandBoundary` is the native `Queue_AI` exchange and the
     checksum record and compare, both inside the frame.
   - The remaining effects are the radar availability setter, scenario pause
     and resume, the production strip and the `before_logic` save.
3. **Drawing reads live state.** About 86 non-test sites in `app/presentation`
   read `SimRuntime` directly: entities, display layers, fog, terrain,
   overlays, anims, particles, projectiles and houses. Interpolation keeps
   only previous and current poses. `Simulation` is not `Clone`.
4. **The app writes the simulation outside the tick** at about 54 sites in
   19 files:
   - commands go into the simulation's own queue, encoded from live reads;
   - selection flashes and voices;
   - debug and cheat tools;
   - pause, save, load and restore;
   - network departures.

## Not obstacles

- `SimRuntime` is `Send`. A compile-time assertion in `sim/runtime.rs` keeps
  it so. `src/sim` holds no `Rc`; its `thread_local!`s are test and trace
  hooks.
- The owners the mid-tick effects borrow are `Send` too: `SfxPlayer`,
  `SoundEventQueue`, `MatchSession`, `LiveCommandSession`,
  `PlanningInputState`, `SelectionState`, `ClientRadarEvents` and
  `ScenarioPauseClock`. A compile-time probe checked this when this plan was
  written; phase 1 should assert it where it moves them.
- The process Main RNG is outside saves and the state hash. The app's cell
  (`sim/rng.rs` `MainRng`) is `Arc<Mutex<_>>`, drawn on the main thread and
  the audio output thread; the simulation owns its Main as a plain `SimRng`.
  Only the order of presentation draws (voices, laser jitter) can differ
  from a single-threaded run.
- Commands are already scheduled by frame, so replays and lockstep do not
  depend on when the main thread submits them.

## Plan

### Phase 0: drawing never writes the simulation

- [x] Main draws take a shared reference (`app::state::process_main_draws`).
- [x] Radar-terrain marks leave the simulation once per frame
  (`SimRuntime::take_radar_terrain_dirty`, `App::drain_radar_terrain_marks`).
  The minimap's own `RadarTerrainBacklog` owns retry and acknowledgement.
- [x] `SimRuntime: Send` is asserted at compile time.
- [x] No `sim_runtime.as_mut()` remains under `app/presentation` or
  `render`.
- [x] Drawing cannot reach the live world's objects: it reads the
  presentation frame and the world copy (phase 2), and debug builds draw
  every frame from a copy whose entity store panics on a read.
  `render_game` itself still takes `&mut AppState`.

### Phase 1: a tick that borrows no app state

The host lives as long as the process, like the thread it becomes. Several
owners a tick reaches are process globals in gamemd, so they survive launch
and load: the checksum ring `B04474` is one. A host that lived and died with
each scenario could not hold them.

Moving every client owner into the host does not work: the sound player,
the network transport and the input state are shared with menus, the lobby
and the UI. Instead, a tick that runs on the simulation thread takes what it
reads from those owners before it starts. It records what it writes, and the
main thread applies the record in order afterwards
(`app/match_runtime/deferred_effects.rs`):

- **Sound** is not delivered mid-tick. The frame tail already plays every
  sound fact the tick left undelivered, and refreshes looping sound
  positions.
- **The radar availability cue, the production strip's request clear and
  Ares bounty text** are recorded, then applied in order after the tick.
- **Planning Mode**: the flag is taken before the tick. Leaving it issues
  the same PLANCOMMIT (`planning_mode::plan_commit_command`), and the main
  thread leaves its own planner after the tick.
- **The outcome-voice wait** polls a copy of the wait with the voice state
  taken before the tick. It moves only wall time.
- **Scenario pause and resume effects** go to the frame tail's
  `apply_trigger_effects`, as on the headless path.

Only presentation order moves: sounds and the radar cue follow the tick
instead of interleaving with it. Replay playback, a campaign's initial save
and command observation still need the lent owners. Those frames stay on the
borrowing path (`DeferredClientEffects::applies`). A network frame's
`Queue_AI` exchange needs the live session; phase 5 hands it back to the
main thread.

Done:

- [x] `MatchState::sim_runtime` is the process-lifetime `SimHost`
  (`app/match_runtime/sim_host.rs`), holding `Option<SimRuntime>`.
  - Reads take `runtime()`.
  - Every write names its route through `write_sim(SimWrite::…)`.
  - The tick takes its parts through `tick_parts()`.
- [x] The checksum history moved from `MatchSession` into the host.
- [x] Deferred client effects: every frame the simulation thread could take
  uses them. In a hidden 1v29 client run, the state
  hashes at every common tick from 1 to 9000 are identical with and without
  it.
- [x] The radar terrain updates per changed cell instead of redrawing its
  whole surface.
  - In a hidden 1v29 client run at 600–800 objects, frame time fell from
    22.9 ms to 14.5 ms and render time from 10.5 ms to 3.7 ms.
  - The tick is now most of each frame.

### Phase 2: drawing reads a snapshot, never the live world

Drawing reads two things, both owned by the main thread, and shares no
storage with the world the simulation thread ticks:

- **The presentation frame** (`app/presentation/frame.rs`,
  `PresentationFrame`): one `ObjectRecord` per stored object. A record keeps
  the object's state under `GameEntity`'s own names, so a drawer reads a
  record as it read the object, plus what reading it needs more of the world
  for, answered at capture by the same functions a draw used to call: the
  local player's view of it (`Seen`), its placement (`Place`), its status
  strip (`ui_overlays::HudFacts`), its radar facts (`frame_scene`), and the
  frame-wide parts (anim coordinates, the sidebar's and the sounds' inputs,
  spotlights, the planning graph). Values that need only the record and the
  world copy (the Foot draw depth) are computed when an object is drawn.
  `SimHost::frame` captures it from the home world on its first read after a
  write, and `SimHost::lend` before a tick leaves.
  The default display option **Smooth unit movement** (`[VERA] RenderInterpolation`)
  keeps the last two poses of visible walking or flying infantry, Drive, Ship
  and Hover vehicles, voxel Jumpjet vehicles, and voxel Fly aircraft. The renderer
  samples movement between consecutive committed ticks when the picture budget
  allows. SHP bodies use the current sprite facing, action and animation frame.
  A Chrono Miner uses its active Drive locomotor while travelling to ore and
  snaps to its destination when its Teleport locomotor returns it to a refinery.
  Airborne and hovercraft shadows follow the ground under the displayed body.
  Movement appears about one tick later. Picking uses the last presented
  position. A newly visible unit, a warp, a display-layer change or a shot
  starts at the committed pose. A late tick holds its last known position.
- **The world copy** (`SimRuntime::presentation_copy`): the world without
  its objects. It leaves out the objects (its entity store is
  `EntityStore::detached`, which panics on a read in debug builds, so a
  drawer that still reaches for an object shows up in any draw), the zone
  and path grids, the team script VM, the Houses' spatial threat grids and
  the other state only gameplay reads. Its fog is only the local player's
  (`FogState::presentation_copy`), whose shroud knowledge, like its terrain,
  overlays, smudges, terrain objects, interner and occupation grids, is the
  previous copy brought up to date. The occupation grids track changes one
  dense cell at a time and reuse the destination's occupant lists.
  The House array, the substrate stores, the
  clone of the rest and the fog copy run at once beside those syncs. Its
  Dummy cell, ally graph and Main RNG are its own. Release builds take it when
  a tick is lent; debug builds with every frame, so every test draw reads a
  lent tick's view.

The app's Main RNG is its own cell from a scenario's install on
(`Simulation::install_fresh_process_main`); the simulation draws its own
generator through `&mut`.
Main is outside saves and the hash: only which random voice plays can
differ from gamemd.

Per-frame questions the cursor asks of the live world (its action, hover,
the ferry preview, the planning hover) are answered with the world home and
kept for the draws beside a lent tick (`sim_host::home_answer`). When every
frame is lent no draw finds the world home, so each is also asked just
before a tick leaves (`cursor::answer_kept_questions`), as is the building
placement preview, which the passes beside the tick keep: all are one tick
late at most. An atlas refresh asked beside a tick (a growth published
then) waits for the tick's tail.

The live world and its presentation copy share `SimResources` while the
match runs. It holds the rules and the terrain template. The rules own the
overlay types used by simulation and drawing. After an offline Skirmish, the
setup dialog writes the current countries into the retained world's rules.
The host releases the copy's reference before it returns that world for a
write. The captured frame holds none: it sorts its Display order at capture.

What the snapshot costs the main thread:

- The records, the Display order, the anims' coordinates, the parachutes
  and the control groups are captured on the worker pool while the scene
  frame is captured on the calling thread. A record asks only what
  a drawer of that object reads: an action line only for a selected or
  psychically detected object, the airstrike tint only for a Structure, a
  Carryall's cargo only for an Aircraft, the mind-control count only for a
  capture manager.
- Before a tick leaves, the pool files the frame's Display members
  (`PresentationFrame::display_members`), a run at a time, so no draw beside
  the tick files them.
- A retired frame's object records and draw caches are freed on the pool
  (`sim::parallel::drop_on_pool`), as is the old world copy's unsynced part.
- The world copy's cell-at-a-time syncs, the local player's fog, the Anim
  store, the House array and the substrate stores it carries run on the pool
  beside the clone of its small rest.
- The first world copy is a whole one; desktop takes it when a scenario is
  installed (`SimHost::install`), so the first lent tick only syncs it.

Current cost: see Status.

### Phase 3: lending the runtime

`SimHost::lend` hands the runtime to a persistent simulation thread for
one job. While it is away, `runtime()` serves the published presentation
frame, so the sidebar, tooltips and the other cold readers see the last
tick without waiting; a reader that needs the objects keeps an answer taken
with the world home (Phase 2). Every write route (`write_sim`,
`tick_parts`, `install`, `take_runtime`) first takes the runtime back,
waiting for the job; its result then waits in the host. A panic in the job
resumes on the main thread.

A player command therefore waits for the tick in flight, at most one tick.
Heavy queries can still move to requests answered after the next tick if
that wait shows in play.

### Phase 4: the thread

`advance_one_simulation_frame` is split into three parts:

- the prepare, on the main thread: client reads, due commands and the
  deferred effects;
- `run_deferred_tick`, which borrows nothing from the app;
- `finish_simulation_frame`, the frame tail on the main thread.

An ordinary offline frame on the deferred path can be lent to the simulation
thread instead of run inline. The thread has no switch: every desktop build
uses it (`sim_host::sim_thread_available`). No pass waits for a lent tick
(`SimHost::pass_may_run`). A pass that finds it running admits no frame and
takes nothing from the world, as a pass the pacer does not admit
(`sim_tick::service_beside_tick`): the camera takes the scroll steps its own
clock owes, the per-pass presentation services run, and it draws the frame
published for the tick. When the tick ends, the simulation thread wakes the
event loop (`SimHost::set_wake`, an `EventLoopProxy`), and `about_to_wait`
services the match at once, without a draw, so the next tick leaves without
waiting for a redraw. A pass that goes on:

1. `finish_threaded_frame`, first thing in the pass: run the ended tick's
   tail and hand its radar-terrain marks to the minimap. The frame
   published for that tick is dropped, and reads see the live world again;
2. the pass's own service: pause and chrono service, exit and outcome
   routing, menus, saves. They read a world whose last tick the client has
   fully taken, and nothing is in flight;
3. if a frame is admitted, tick it inline or lend it. Lending publishes the
   world as it is now, and the draw reads that frame while the tick runs.

Only one tick can run beside a draw, and each lent tick costs a published
frame, so only the last frame of a service is lent. `service_match_runtime`
runs every frame that is due within its catch-up budget (eight passes,
eight milliseconds): at GameSpeed 0, which never waits, that is up to eight
frames a picture. A frame that another pass will follow
(`LocalFramePacer::another_pass_follows`: another frame is due, and the
budget is still open after this one's expected tick time) ticks inline, as
it did before the thread, and publishes nothing, unless lending it is
measured cheaper (below). The frame nothing follows is lent, and the
service ends there: another pass would only find it running; a lent
catch-up frame ends it the same way, and the tick's end wakes the pass that
runs the next frame due.
Debug speed's extra frames follow the same rule.

A lent tick frees the main thread from the tick and costs it the frame's
copy when the tick leaves, any wait for it, and the frame's drop
(`SimHost::lend_cost`). A frame whose
tick is expected to cost the main thread less than the last lent one did
ticks inline as well (`sim_tick::frame_ticks_inline`). A frame that hashes
its state for the replay log goes whatever the estimate says: it costs far
more than the frames around it, which the estimate follows, and on a large
map the hash alone can take longer than a frame's budget. In a 1v29 skirmish on the 30-start map the frame costs
more than a tick until the battle is large, so lending every last frame made
GameSpeed 0 run 8 to 22% slower there, and far slower when other programs
kept the simulation thread waiting for a core.

Between the dispatch and the next pass nothing on the main thread may take
the live world: a write waits for the tick, and the tick then did not run
beside the draw. The per-pass EXIT check therefore looks at the published
frame before it takes the world, and the draw-time radar drain leaves the
marks to the tail while a tick is out. `SimHost` counts jobs taken back
early and logs them every 1,024 ticks and when a scenario leaves. Window
events wait, in order, while a tick is out, since an event may write the
world (`App::release_held_events`). The pass that finds the tick back runs
its tail, replays them, then services, so input acts at most one tick later
than inline. Pointer motion with nothing held ahead of it goes on, so the
cursor keeps following the hand; a motion that writes the world waits for
the tick. Composed IME text waits with the held events, so it cannot
overtake a key pressed before it. Captures and hidden runs keep the old
order: their pass waits for the tick, so each picture follows a finished
one.

While a tick is lent, drawing shows the world one tick behind the
simulation.

A network frame (LAN or online) can be lent the same way. On the thread it
runs Logic and its checksum and stops at `Queue_AI`: `DeferredClientEffects`
answers the exchange with a wait and records the checksum. Finishing the tick
applies that record to the history, then resumes the frame inline as a
network wait is resumed (`FrameResume::QueueHandoff`): the live session runs
the exchange, the dispatch, the FRAMEINFO comparison and the commit. The
session never leaves the main thread, and the frame keeps gamemd's order:
Logic, checksum, exchange, wait, dispatch. A tick on the thread therefore
never executes departures or an EXIT. Two measurements follow the frame
across the hand-off: `MainTick55D3DA/55DE0D` times the Logic visit up to
`Queue_AI` on the thread, and the frame's cost is the thread's part alone.
Online, whether another pass follows a frame is asked of the relay's
frontier and the online pacer, as the catch-up loop asks it, not of the
local timer.

These frames always stay inline:

- replay, a campaign's initial save and command observation (the deferred
  path does not apply to them);
- a network restore, which replays many frames per pass, each waiting for
  the last;
- resumed frames: outcome, network and checksum waits;
- `ExactOneStep` diagnostics and debug frame steps, which must end inside
  the call. A tick already in flight finishes before an exact step is
  counted.

A pass whose finished tick left an outcome wait, a stopped frame, an open
menu, a pause or a result to route dispatches nothing: step 2 sees it
before step 3.

The browser lends ticks only in its multi-threaded package (below).

Residuals:

- The simulation thread still runs one tick per hand-off: the main thread
  runs each tick's tail and dispatches the next. The wake keeps that gap to
  the tail's own cost, not a picture.
- Held window events include `Resized` and `ScaleFactorChanged`: a window
  resized while a tick is out reconfigures its surface when the tick is
  back (`follow_window_size` covers the draws between). While events are
  held, the arrow keys queue no scroll, since a held key release has not
  reached them yet.
- At GameSpeed 0 the tick rate is bounded by the catch-up budget, eight
  frames or eight milliseconds a picture, as before the thread. Running
  those frames back to back on the simulation thread needs each frame's
  client inputs and its tail to leave the main thread's pass.
- At 20,000 units and 30 houses (`sim-bench --scenario march
  --presentation-frame`), keeping a frame each tick takes the mean tick from
  54.0 ms to 64.6 ms; the final hash is unchanged. A lent tick therefore
  gains only where the draw it runs beside costs more than its frame.
- A tick whose tail has not run when a debug tool or a load takes the
  runtime back keeps its result until the next pass, or drops it when a
  scenario is installed.

### The browser

The default browser packages have no threads and tick inline. The
multi-threaded package (`tools/wasm/build.py --threads`,
[Wasm guide](wasm.md#multi-threaded-build-experimental)) lends ticks as
desktop does, but the browser's main thread traps whenever it waits for
another thread, so two things differ:

- **The thread is a Web Worker** (`web::sim_worker`): a module worker that
  shares the engine's memory, started the way wasm-bindgen-rayon starts the
  pool's workers but with the main thread's 8 MiB stack. The pool's workers
  get wasm-bindgen's 2 MiB, and a Wasm stack that overflows writes over the
  heap. The first pass of a scenario starts it, and only when the page is
  cross-origin isolated and `initThreadPool` started the pool: without the
  pool, rayon runs its work on the main thread, which the worker's parallel
  work would wait for in vain. Until the worker runs, frames tick inline.
  It is experimental and off unless the page turns it on:
  `set_simulation_thread` takes `1` to turn it on, `always` to lend every
  frame it can take, or `0`. Lending then follows desktop's rules, except
  that a match with Wasm extension modules never lends: each module is a
  JavaScript `WebAssembly.Instance` of the main thread (wasm-ext `browser.rs`).
- **Nothing wakes the loop.** The next animation frame asks whether the
  tick has ended. Every window event waits while a tick is out, pointer
  motion too, and a write no route covers (a debug tool, a teardown) spins
  until the tick ends and counts as an early return. The other routes leave
  a lent tick alone: departures, the EXIT check and the radar drain, and
  network frames hand Queue_AI back to the main thread, so the WebSocket
  session never leaves it.

Nothing the draw and a tick can both take waits on the main thread
(`util::sync`). The zone grid's precheck memo, and the app's Main RNG cell,
lock with `wasm_sync`'s mutex, which spins there. Once-cells both can reach (the
static tables, such as `adjust_for_z_standard`'s) never wait: two threads
that fill one at once both compute it, and one value is kept. (`wasm_sync`'s own
once-cell reads the standard library's private state and spins forever on
the nightly the build uses.) The host and the worker hand each other the
runtime through a one-slot mailbox on the same mutex, since a standard
channel's sender can wait on a lock of its own. A panic aborts a Wasm
worker, so its tick never comes back: the panic hook keeps the message, and
the main thread panics with it at its next pass, which reports
`engine-error` to the host. A trap the hook never sees, such as an
allocation failure, reaches the same place through the worker's script.

Residuals:

- While the pool runs a tick's parallel work, the simulation worker waits
  for it, as the main thread did, instead of joining in. The pool keeps one
  worker per CPU thread but the main one.
- A pass that finds the tick running pumps no audio and services no network
  until the next one, at most one tick later. On desktop the tick's wake
  brings that pass at once.
- A pool worker's panic during an inline tick still leaves the main thread
  spinning in rayon, so the tab freezes instead of reporting it.

## Status (render snapshot, phases 2–3)

Done: drawing reads the presentation frame and the world copy only; the
world the simulation thread ticks shares no storage with them (the read-only
`SimResources` aside). Entities, the zone grid, fog planes and team script
definitions are no longer copy-on-write. The app and the simulation draw
separate Main RNG generators.

Not done:

- **Snapshot cost.** The goal is 2 ms per tick at 20,000 objects. The world
  copy now takes 1.6 ms per lent tick, under that for its part: measured in
  shipping `sim-bench` on the 20,000-unit, 30-house battle map (`--scenario
  battle --map battle30.map --ticks 600 --threads 16
  --presentation-frame`), AMD Ryzen 7 9800X3D (8 cores/16 threads),
  2026-11-26: `presentation_copy_mean_ms` 1.611 and 1.622 over 300 frames,
  against 3.462 and 3.189 for the copy before this change; all 600 per-frame
  hashes and the final `a76b4b338c2e955a` matched. The cells a write reached
  sync instead of their whole columns, and the House array and substrate
  stores clone on the pool. Capturing the frame (5.0–5.6 ms on the earlier
  12-core machine) and freeing are unchanged, so the whole snapshot is still
  above 2 ms. A record is 1,104 bytes, and capture is bound by reading 20,000
  objects scattered in memory, each touching several cache lines of its
  `GameEntity`. Left: answer placement, lighting and the status strip only for
  objects near the screen (radar facts stay for every object).
- **The simulation thread ticking on its own**, several ticks per hand-off
  (Phase 4's first residual), is the next phase.

## Validation

- `sim-bench --frame-hashes` stays identical before and after every phase:
  1v29 on the 30-start map, and retail `xmp34u4.map` for bridges.
- Replay determinism tests pass.
- Browser (multi-threaded package): an offline 1v29 skirmish with
  `set_simulation_thread("always")` takes no tick back early and accepts
  input, and `tools/spawn/run.sh browser` with `SIM_THREAD=always,0` and
  `always` ends without a desync.
- Before phase 2, measure client draw CPU at 2,000 units. If drawing alone
  exceeds the frame budget, it needs its own work.

## Where a 30-house frame goes

Measured on the 30-house match map (`python -m tools.scale_map
target/scale/match30.map --starts 30`, then `sim-bench --scenario match --map
target/scale/match30.map --houses 30 --ticks 300`): 22,142 live objects,
21,059 AI visits and 18,114 ground Processes a frame. The release build takes
52.2 ms a frame; the 45 fps step needs 22 ms.

The split below comes from per-object spans in a probe build that `main` does
not carry; `main`'s `--frame-profile` reports the same frame in the phase
slots of `sim::frame_profile`. The spans cost two clock reads per object and
took the frame to 54.5 ms, so read the rows as proportions.

| Part of one object's turn | ms |
|---|---|
| before the AI visit | 0.7 |
| the AI visit | 6.6 |
| between the visit and Process | 1.7 |
| the ground locomotor's Process | 28.8 |
| after the Process | 7.7 |
| other class branches | 7.5 |

Inside the Process: pending-work loop 16.2, track/turn gate 5.9, walk
admission 2.2, path request 1.0, tail 2.3; the 6.2 ms of inline path searches
are counted inside these parts, not beside them. Inside the post-Process tail:
firing 3.0, Foot body cadence 1.2, Foot sounds 0.7, idle
Scatter/piggyback/pending-entry 0.7, disguise/sinking/trailer 2.1.

No single site costs 5 ms. Halving the frame needs either the sharded pass
below or a general cut in per-object work; no one row reaches the 22 ms step
on its own.

### Inline path searches

95 searches a frame run inline: 9,147 expansions and 6.2 ms, of which 14
searches over 100 µs (the slowest 1.6 ms) own about 5 ms. By site:
Drive/Ship 61 searches and 4.6 ms, walk 14.8 and 0.6, walk admission 19.2 and
1.0, hover and linked cars none. They are immediate requests, a mission order
or a Drive/Ship Process path the mover needs this frame, so deferring them to
the batch would change behavior, as the batch already does for no-queue walk
requests. `defer_path_search` declines about 292 requests a frame, but only
because the Foot already has an answer to claim; a claimed answer's key
mismatches 0.4 times a frame.

Two attempts to remove them were measured and reverted:

- Deferring only the own-cell requests left the inline count at 95 and
  changed the state hash.
- Answering several requests per Foot per frame (`ready` holding a slot per
  registration, `pending` no longer one per Foot) took the count from 95.2 to
  94.1 and the frame from 53.8 to 54.2 ms, and changed the state hash. The
  one-answer-per-Foot rule is not what keeps these searches inline: of the 298
  slots a pass staged, 202 carried an answer and 96 were empty because
  `batched_search` refuses the request (a destination redirect, the Foot's own
  cell, a `Stopped` or `CellEntryUnavailable` search, or no destination). Each
  of those Feet then searches at its own turn, and an inline search costs about
  ten times the batch's per expansion, because it walks its own code-2 chains
  where the batch shares one cell's chain across every search that asks about
  it.

## Sharded object pass, costed

The pass (`Simulation::advance_live_object_pass`) walks live objects in
stable-id order, and each turn writes several owners besides its own entity:
the substrate's cell and occupancy records, terrain objects, production
queues, fog and shroud, sound events, another entity's health and lifecycle
through damage and kills, and the path batch's registrations. The vision
planes are the exception: the pass already defers them and applies them in
parallel.

Sharding cannot call today's turn as it stands. Every one of those writes
needs a shard-local overlay merged in shard order, and every read needs the
rule "my shard's writes, then the frame's start", which makes a write visible
to another shard one tick late. That is a deviation: a probe that walked the
same snapshot in reverse order diverged, with the path batch on or off.

The payoff: the pass is 48 of the release build's 52.2 ms (92%). By Amdahl,
5× on the pass leaves 13–14 ms and 8× leaves 10–11 ms, both inside 22 ms.

A bench-only probe counted the writes an overlay would stage on the same map,
with the state hash unchanged: 154,380 mutable hand-outs of the turn's own
entity a frame, 548 of another entity (0.35% of hand-outs, about one per 40
objects) and 15,691 writes of the shared cell grid (0.71 per object). That
staging is small, since a shard owns its own entities outright. The pass is
read-coupled rather than write-coupled: those cell writes matter because
other turns read them.

Status: set aside. The staging fits; splitting the `&mut Simulation` every
turn takes into per-shard views does not, and no counter prices that work. A
first step that keeps behavior identical would stage every write and apply it
in the same stable-id order on one thread, with a per-owner write counter and
`sim-bench`'s final hash unchanged against `main`; it would show which owners'
writes really cross objects before any thread runs. Running shards
concurrently is the step that carries the deviation.
