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
instead of interleaving with it. Network frames (the `Queue_AI` exchange),
replay playback, a campaign's initial save and command observation still
need the lent owners. Those frames stay on the borrowing path
(`DeferredClientEffects::applies`).

Done:

- [x] `MatchState::sim_runtime` is the process-lifetime `SimHost`
  (`app/match_runtime/sim_host.rs`), holding `Option<SimRuntime>`.
  - Reads take `runtime()`.
  - Every write names its route through `write_sim(SimWrite::…)`.
  - The tick takes its parts through `tick_parts()`.
- [x] The checksum history moved from `MatchSession` into the host.
- [x] Deferred client effects, opt-in through
  `VERA20K_DEFERRED_TICK_EFFECTS=1`. In a hidden 1v29 client run, the state
  hashes at every common tick from 1 to 9000 are identical with and without
  it.
- [x] The radar terrain updates per changed cell instead of redrawing its
  whole surface.
  - In a hidden 1v29 client run at 600–800 objects, frame time fell from
    22.9 ms to 14.5 ms and render time from 10.5 ms to 3.7 ms.
  - The tick is now most of each frame.

### Phase 2: a presentation frame for the hot draw path

At the end of each committed tick, `SimHost::publish_presentation` keeps a
copy of the runtime (`Arc<SimRuntime>`), and drawing reads it through
`SimHost::presentation`. A rules or lifecycle write drops it first. It is
opted into with `VERA20K_PRESENTATION_FRAME=1` until phase 4 turns it on.

A plain copy of the world is too slow: 40–60 ms in a 1v7 skirmish on the
30-start map, about half of it the terrain cells. The copy therefore shares
storage with the live world copy-on-write (`util::shared`):

- `ChunkedVec` (256-element chunks) for per-cell state written in small
  parts every tick: fog planes and their caches, overlays, smudges and the
  terrain cells. A write copies only its chunk.
- `CellMap` strips are one column each, shared the same way.
- `Shared<T>` for state that rarely changes as a whole: entities (each one
  separately), the zone grid, the interner, AI script definitions, terrain
  objects and the terrain's TMP headers and per-cell side arrays.
- `DerivedCache` for caches a copy can rebuild; a copy starts empty.

Writers that usually leave a value unchanged compare before they borrow it
mutably, so an unchanged write copies nothing: the terrain cell recalc
(`CellClass::RecalcAttributes @ 0x0047D2B0`) and the overlay projection
work on a one-cell shadow (`cow_cells`), and the zone grid checks a cell's
class and level before refreshing it.

Measured with `sim-bench`, 1v29 skirmish on the 30-start map, 3,000 ticks,
447–583 objects, keeping each tick's copy until the next one:

| | mean tick |
|---|---|
| before phase 2 | 3.5 ms |
| phase 2, no frame | 3.7 ms |
| phase 2, a frame each tick | 7.2 ms (copy 1.5 ms, drop of the previous 1.2 ms) |

`final_hash` is the same in all three. The copy still deep-copies the 30
houses (~0.3 ms) and the entity store's index (~0.2 ms); the remaining
tick cost is the chunks and entities a tick really writes.

Follow-ups: fog planes keep one byte per cell per house where gameplay
needs a few bits; packing them would shrink both the copy and the cache
footprint.

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
