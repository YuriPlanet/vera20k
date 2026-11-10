# Simulation thread

Goal: run the simulation on its own thread so that drawing keeps its frame
rate when a tick costs more than the frame budget. In a 1v29 skirmish a tick
at about 2,000 objects costs about 25 ms against a 22 ms step. Today the tick
and the draw share the main thread, so the frame rate falls to about 30.
With its own thread, a slow simulation slows only game time, and drawing keeps
interpolating between the last two ticks (`app::presentation::interpolation`).

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

### Phase 1: a `SimHost` owns everything a tick touches

Move the mid-tick effect *owners* next to the simulation instead of turning
their calls into events. A `SimHost` holds:

- `SimRuntime`;
- the live network session and the checksum history (`MatchSession`,
  `LiveCommandSession`);
- the sound producer: `SfxPlayer` and its `SoundEventQueue`;
- the outcome-voice wait;
- the scenario pause clock;
- the planning input that a defeat leaves.

The main thread talks to it through:

- `submit(commands)`;
- `outputs()`: the per-tick `SimFrameOutput` remainder, screen shake, camera,
  HUD and radar events;
- `request(query)`, for input queries that need live state.

`SimHost` still runs on the main thread in this phase. Done when every
`sim_runtime.as_mut()` outside `SimHost` is gone.

Progress:

- [x] `MatchState::sim_runtime` holds a `SimHost`
  (`app/match_runtime/sim_host.rs`). Reads go through `Deref`. Every write
  names its route through `write_sim(SimWrite::…)`, and nothing else
  compiles.
- [ ] Replace each route with operations of its own.
- [ ] Move the mid-tick effect owners into the host.

The routes as first assigned (`git grep "write_sim(SimWrite::"`):

| Route | Sites | What they do |
|---|---|---|
| `Command` | 21 | Issue local commands from input, encoded from live reads: stop, guard, scatter, production, superweapons, placement, planning, diplomacy, cheer, options. Also the order and selection voices and Phobos selection flash. |
| `Debug` | 21 | Debug tools, cheat flags, the trainer, frame profiling, and the capture harnesses under `app/diagnostics`. |
| `Lifecycle` | 19 | Install, take or restore the runtime; retire the scenario and its rules; campaign carry-over and stage; replay startup; input delay. |
| `Network` | 8 | Departures and kicks (`destroy_connection`), received events, EventClass queue service, timing install, initial-wait interrupt. |
| `Tick` | 2 | `advance_one_simulation_frame` and scenario pause service. |
| `Presentation` | 2 | Radar-terrain marks and lighting events: handoff state, neither saved nor hashed. |
| `Rules` | 1 | Multiplayer country projection into the bound `RuleSet`. |

When the simulation runs elsewhere, these writes change gameplay state
outside the command stream and must keep their frame:

- `destroy_connection` calls `flag_house_to_die` directly. This happens on
  a frame-0 loss or a WOL tournament leave.
- `deploy_voice_request` clears a unit's deploy-voice suppression
  (Techno+0x4F8) at input time.
- Phobos selection flash writes each selected unit's flash countdown.
- `ScenarioServiceInput::apply` pauses, resumes or expires the scenario,
  or applies a chrono screen effect, between ticks. It is recorded in the
  replay.
- The trainer applies debug tools.

Order and selection voices draw the process Main RNG at input time. That
does not touch lockstep, but a read-only view cannot serve them. They need
the host or their own draw handle.

### Phase 2: a presentation frame for the hot draw path

At the end of each tick, `SimHost` publishes an immutable
`Arc<PresentationFrame>`. It holds per-object draw records (pose, type,
owner, health, flags), the display order, anim, particle and projectile
points, and cell deltas for fog, terrain and overlays. Object, anim and
particle drawing reads only the last two frames. That is the work that scales
with unit count.

### Phase 3: cold paths and input

The sidebar, tooltips and cursor legality read the frame when the data is
small. Heavy queries such as reachability and fire error either go to
`SimHost` as a request answered after the next tick (one tick, 22 ms), or
take a short lock between ticks.

### Phase 4: the thread

`SimHost` runs on a dedicated thread at the game speed. The browser build
keeps the inline mode behind a cfg until a Worker thread with shared memory
exists.

## Validation

- `sim-bench --frame-hashes` stays identical before and after every phase:
  1v29 on the 30-start map, and retail `xmp34u4.map` for bridges.
- Replay determinism tests pass.
- Before phase 2, measure client draw CPU at 2,000 units. If drawing alone
  exceeds the frame budget, it needs its own work.
