# Simulation thread

Goal: run the simulation on its own thread so that drawing keeps its frame
rate when a tick costs more than the frame budget. In a 1v29 skirmish a tick
at about 2,000 objects costs about 25 ms against a 22 ms step. Today the tick
and the draw share the main thread, so the frame rate falls to about 30.
With its own thread, a slow simulation slows only game time: the camera,
the cursor and the interface keep the picture rate, while objects move once
a tick, as they always draw their current state (`RenderPose`).

This changes no gameplay. Every phase must keep `sim-bench --frame-hashes`
identical frame by frame.

## Where the main thread couples today

1. **Frame loop** (`app/frame.rs` `render_frame`). Each frame it prepares
   interpolation, then runs `service_match_runtime`, then draws. Catch-up runs
   up to 8 ticks or 8 ms per frame (`frame_pacer.rs` `permits_catch_up`). A
   hidden window keeps ticking without drawing (`handler.rs` `about_to_wait`).
2. **Mid-tick client effects** (`sim/world/frame_effects.rs`, implemented by
   `app/match_runtime/frame_effects.rs` `FrameClientBoundary`). These run in
   native order *inside* a tick, and they cannot become post-tick events:
   - `SoundBoundary` plays the reached sound prefix at once. Gattling Stop must
     finish before Report's Main draw, and sound and simulation share the
     process Main RNG.
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
- The process Main RNG (`sim/rng.rs` `MainRng`) is outside saves and the
  state hash, and it is already `Arc<Mutex<_>>`. Draws from two threads stay
  deterministic for gameplay. Only the order of presentation draws (voices,
  laser jitter) can differ from a single-threaded run.
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
- [ ] Nothing enforces that yet: `render_game` still takes `&mut AppState`.
  Phase 1's `SimHost` closes this. Presentation then gets only a shared
  view and cannot reach the runtime mutably.

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
`Queue_AI` exchange needs the live session; phase 4 hands it back to the
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
- **The world copy** (`SimRuntime::presentation_copy`): the world without
  its objects. It leaves out the objects (its entity store is
  `EntityStore::detached`, which panics on a read in debug builds, so a
  drawer that still reaches for an object shows up in any draw), the zone
  and path grids, the team script VM and the other state only gameplay
  reads. Its fog is only the local player's (`FogState::presentation_copy`);
  its terrain, overlays, smudges, terrain objects, interner and occupation
  grids are the previous copy brought up to date by copying only the chunks
  a write changed (`util::synced`); its Dummy cell, ally graph and Main RNG
  are its own. Release builds take it when a tick is lent; debug builds with
  every frame, so every test draw reads a lent tick's view.

The app's Main RNG is its own cell from a scenario's install on
(`Simulation::install_fresh_process_main`); the simulation draws its own.
Main is outside saves and the hash: only which random voice plays can
differ from gamemd.

Per-frame questions the cursor asks of the live world (its action, hover,
the ferry preview) are answered with the world home and kept for the draws
beside a lent tick (`sim_host::home_answer`), one tick late at most.

Still shared, read-only: `SimResources` (the rules, the terrain template, the
overlay registry), which nothing writes once a match is loaded.

Measured (release test build, sim-bench's 20,000-unit march on a loaded
12-core machine whose tick ran 3.7–5.8x slower than quiet): capturing the
frame 19–30 ms, bringing the world copy up to date 7–9 ms, the first whole
copy 46 ms. Not yet within the 2 ms goal at 20,000 objects; see Status.

### Phase 3: lending the runtime

`SimHost::lend` hands the runtime to a persistent simulation thread for
one job. While it is away, `runtime()` serves the published presentation
frame, so the sidebar, tooltips, cursor legality and the other cold readers
see the last tick without waiting. Every write route (`write_sim`,
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
it did before the thread, and publishes nothing. The frame nothing follows
is lent, and the service ends there: another pass would only find it
running.
Debug speed's extra frames follow the same rule.

A lent tick frees the main thread from the tick and costs it the frame's
copy when the tick leaves, any wait for it, and the frame's drop
(`SimHost::lend_cost`). A frame whose
tick is expected to cost the main thread less than the last lent one did
ticks inline as well. In a 1v29 skirmish on the 30-start map the frame costs
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
  frame it can take, or `0`. Lending then follows desktop's rules.
- **Nothing wakes the loop.** The next animation frame asks whether the
  tick has ended. Every window event waits while a tick is out, pointer
  motion too, and a write no route covers (a debug tool, a teardown) spins
  until the tick ends and counts as an early return. The other routes leave
  a lent tick alone: departures, the EXIT check and the radar drain, and
  network frames hand Queue_AI back to the main thread, so the WebSocket
  session never leaves it.

Nothing the draw and a tick can both take waits on the main thread
(`util::sync`). The Main RNG and the zone grid's precheck memo lock with
`wasm_sync`'s mutex, which spins there. Once-cells in copy-on-write state
(`DerivedCache`, the zone grid's cached hash, the terrain's allocation
caches) and the static tables both read never wait: two threads that fill
one at once both compute it, and one value is kept. (`wasm_sync`'s own
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
separate Main RNG cells.

Not done:

- **Snapshot cost.** The goal was 2 ms per tick at 20,000 objects. Measured
  in a release test build on the sim-bench march (12 cores, a loaded
  machine): frame capture 14.9 ms mean (records 11.2 ms in parallel from
  46 ms of thread time: placement 13.4, component copies 8.8, pose and
  entity answers 8.0, local player's view 6.8, radar 6.3, status 1.8,
  tether 1.0; the scene frame 3.7 ms serial), world copy sync 6.3 ms, frame
  drop 1.8 ms, first whole copy 40 ms. A record is 1,200 bytes, about 24 MB
  at 20,000 objects. Next: compute placement and radar values only for
  drawn objects (as the Foot depth already is), slim the records (the
  locomotor is cloned whole), parallelize the scene frame, and build the
  first world copy while a match loads. The estimated floor at 20,000
  objects is 1.5–2.5 ms.
- **Tests.** The client lib suite passes but for 195 cases that need retail
  assets this machine lacks (`Hills.mmx`, `MOVIES01.MIX`, language files)
  and 7 others: the per-client-read pin (fails on main too), a retail cameo
  label check, and five fixtures not yet adapted to the frame
  (`draw_state` fading building: its sensed-observer row needs a frame
  captured for a sensing player; `tooltips` IFV passenger; radar `minimap`
  Mirage and two `visibility` cases whose fixtures have no terrain).
- The core lib suite passes but for 495 cases that need retail assets this
  machine lacks.
- **Phase 4** (the simulation thread ticking on its own) is the next phase.

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
