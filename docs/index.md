---
layout: default
title: VERA20k Engine
---

# VERA20k

Red Alert 2: Yuri's Revenge — rebuilt from scratch in Rust.

This page is a short tour of the code. If it disagrees with the code, the code is right.

The engine is built mostly from plain structs and functions, organized by concern in files and folders.

The top-level layout under `src/`, roughly bottom-up:

- `assets/` — format parsers for `.mix`, `.shp`, `.vxl`, `.hva`, `.pal`, `.tmp`, `.csf`, `.aud`, `.pcx` and Bink video. Written from scratch in this repository, with no third-party C&C format libraries.
- `util/` — low-level helpers: fixed-point math, the original game's trig table and x87 arithmetic, config loading, compression.
- `rules/` — parses `rulesmd.ini` / `artmd.ini` and exposes the resolved game rules.
- `map/` — `.mmx` / `.map` parsing, theaters, resolved terrain, map triggers and the random map generator (`map/rmg/`).
- `sim/` — game state and deterministic behavior. Owns `Simulation`. Never depends on `render/`, `ui/`, `sidebar/`, `audio/` or `net/`. Major subsystems sit in their own subdirs: `combat/`, `movement/`, `pathfinding/`, `aircraft/`, `production/`, `miner/`, `superweapon/`, `vision/`, `docking/`, `world/`.
- `render/` — the wgpu renderer: atlases, terrain, sprites, voxels, radar, shroud and sidebar chrome.
- `sidebar/` — the in-game sidebar's layout and state (cameos, tabs, power bar). The drawing happens in `render/` and `app/presentation/`.
- `ui/` — menus, dialogs and in-game screens: main menu, skirmish setup, pause menu, score screen, messages.
- `audio/` — sound effects and music through `rodio`; plays the sound events the simulation produces.
- `net/` — deterministic lockstep. There is no network transport yet.
- `bin/` — extra programs: MIX browser, Bink video player, INI extractor and asset inspection tools.
- `app/` — the app layer that wires sim, render, ui, audio and net together.

A few files at the root of `src/` handle skirmish and match startup (`skirmish_*.rs`, `match_bootstrap.rs`) and headless retail-map loading for tools (`headless_scenario.rs`).

When adding a new file, ask which of those concerns it belongs to. If it's gameplay logic, it goes in `sim/`. If it touches the GPU, it goes in `render/`. If it connects the two, it's app layer.

To learn what a specific file does, read its `//!` header. Most modules start with a short comment stating their purpose and what they may depend on.

All mutable game state lives in one struct: `Simulation` (`src/sim/world/mod.rs`). Match data that never changes during a match (rules, map heights, trigger definitions) is bound beside it in `SimRuntime` (`src/sim/runtime.rs`).

`Simulation` contains, among other fields:

- `substrate: ObjectSubstrate` — the object stores and their order. Units, buildings and aircraft live in its `EntityStore`; animations, voxel debris and particle systems have their own stores. It also holds the active-object order and the cell occupancy grids. (Projectiles and waves are separate `Simulation` fields.)
- `session: ScenarioSession` — scenario identity, seed, match options and the frame counters (`tick`, `binary_frame`)
- `houses` — per-player state such as credits and defeat status (the original's `HouseClass`)
- `house_alliances` — the alliance graph
- `production: ProductionState` — production bookkeeping (finished items, active factories), plus ore growth and terrain objects
- `fog: FogState` — shroud and visibility per player
- `power_states` — per-player power (output, drain, low power, spy blackout timer)
- `super_weapons` — per-player superweapon instances and their charge timers
- `projectiles` — shots in flight
- `terrain_costs` — pathfinding cost grids
- `zone_grid` — zone connectivity for unreachability checks
- `overlay_grid` — per-cell overlay state on the map (ore density, wall damage, bridge frames)
- `bridge_state` — bridge runtime state (damage and destruction, used by combat and pathfinding)
- `scenario_rng`, `main_rng`, `mapgen_rng` — the three deterministic random number streams (see Determinism below)

Each `GameEntity` is one struct with optional fields.

Every infantry unit, vehicle, ship, building and aircraft is the same struct. Always-present fields include `stable_id`, `position`, `health`, `owner`, `facing`, `type_ref` and `category`. Optional parts are `Option<T>`: a moving unit has a `locomotor`, a harvester has a `miner`.

Behavior lives in functions and in methods on `Simulation`, grouped by mechanism.

Some examples:
- `advance_live_object_pass()` — every active object takes its turn (AI, movement, lifecycle effects), in the original's active-object order
- `refresh_fog()` — updates shroud and visibility
- `tick_power_states()` — recalculates each player's power
- `tick_superweapon_instances()` — advances superweapon charge timers and handles power suspend and resume
- `tick_repairs()` — heals repairing buildings and charges their owners
- `tick_ore_growth_rungs()` — grows and spreads ore in the overlay grid

The frame.

One production entry point runs one frame: `SimRuntime::advance_frame()`, which calls `Simulation::advance_master_frame()` in `src/sim/world/mod.rs`. It follows the original game's frame order: commands due this frame → triggers → ore growth and active superweapon effects → team scripts → the live object pass → vision → power → superweapon timers → combat → crates → production, repairs and docks → houses, defeat checks and AI → frame commit → removal of dead objects → state hash. `Simulation::advance_tick()` is only a test adapter around the same frame.

Output for the rest of the program leaves through per-frame batches (`SimFrameOutput`: sound events, weapon-fire events, lifecycle outputs, overlay changes, lighting events) that the app drains each frame. Inside `sim/`, some mechanisms keep their own queues and message links, such as the radio contact bus between objects (`sim/radio/`), which uses the original's radio message codes.

Planning history. A May 2026 plan for moving the frame onto the original's scheduler is at `docs/plans/2026-05-28-foundational-scheduler-roadmap-todo.md`. Parts of it have landed since then (the active-object pass above), so read the code first.

Rendering.

A 2D renderer using wgpu. At map load, terrain tiles and sprites (buildings, infantry, overlays) are packed into atlas textures — big images containing many images side by side. Voxel models (vehicles, aircraft) are rasterized to 2D images as well; see `unit_atlas.rs` and the `vxl_*.rs` files in `render/`.

Each frame, the app's presentation code (`src/app/presentation/`) reads entity state from the simulation — position, facing, health, animation frame — and builds sprite instances, and `render/` draws them. Isometric depth is handled by draw order and depth values.

Rendering reads simulation state and never writes back. You can change rendering without touching game logic, and vice versa.

App layer.

The app layer wires everything together: the window and event loop, input, menus, loading, saving, and the hand-off between simulation, renderer and audio. Gameplay rules belong in `sim/`, not here.

When you click on a unit, the app layer handles that. It figures out which entity you clicked, translates it into a command, and passes it to the simulation. The app layer is the translator between "what the player did" and "what the simulation understands."

Timing. `src/util/fixed_math.rs` defines two rates:

- `RA2_LOGIC_FRAMES_PER_SECOND = 15` — the original game's logic-frame rate at normal speed. Every INI time value (rate of fire, reload, C4 delay, ore growth, trigger timers) is converted to frames through it.
- `SIM_TICK_HZ = 45` — VERA's own fixed step constant. The game passes its step length, `SIM_TICK_MS` = 22 ms, to every simulation frame. It is not the original's logic rate. Headless tools step at 66 ms instead; `src/headless_scenario.rs` records that difference.

The simulation never reads wall-clock time. The app's frame pacer (`src/app/match_runtime/frame_pacer.rs`) decides when the next gameplay frame may run, using the original's GameSpeed timing, and the app advances the simulation one frame at a time.

After each frame, the app layer hands the updated simulation state to the renderer, which draws the frame. It also drains the sound events that the simulation produced (weapon fired, unit died, construction complete) and plays them through the audio system.

Like everything else, it's split into folders by concern — `app/input/`, `app/presentation/`, `app/match_runtime/`, `app/frontend/`, `app/loading/`, `app/persistence/` — around one shared `AppState` struct.

Determinism.

Everything in `Simulation` must be deterministic: the same state, inputs and random seed give the same result on every supported platform and CPU.

- Math. Simulation math prefers `SimFixed`, a 16.16 fixed-point type (`src/util/fixed_math.rs`), because floats can differ across CPUs and compilers. Where the original game's floating-point behavior matters, some code reproduces it exactly with integer arithmetic (`src/util/native_x87.rs`). Some `sim/` code still uses `f32` / `f64` directly; the project rule is that each such use is documented and validated.
- Randomness. There are three `SimRng` streams, matching the original's three generators: `scenario_rng` (in-game draws such as scatter, debris and ore growth), `main_rng` (the original's global generator; death sounds, for example) and `mapgen_rng` (map generation). Which stream a draw uses matters: a draw on the wrong stream changes every later draw.
- Order. Entities in `EntityStore` are kept in a `BTreeMap` keyed by stable id, so storage iteration is sorted. The order in which objects take their turn is separate: the active-object vector in `ObjectSubstrate`, which follows the original.

At the end of every frame the simulation produces a state hash. Two clients with the same inputs must agree on it. Lockstep multiplayer depends on this (the network transport does not exist yet), and the diagnostic command log records the hashes to help find desyncs. It's also why nothing in `sim/` is allowed to call into `render/`, `ui/`, `sidebar/`, `audio/` or `net/`.

---
