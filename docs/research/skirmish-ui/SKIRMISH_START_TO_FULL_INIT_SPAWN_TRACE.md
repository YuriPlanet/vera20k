# Skirmish Start To Full Init Spawn Trace

**Trace target:** `/trace-action start skirmish from shell to ScenarioClass Full_Init house creation spawn placement`  
**Slot:** 3 of 3  
**Mode:** read-only reverse-engineering trace plus Rust output-shape comparison  
**Status:** COMPLETE  
**Primary verdict:** current Rust launches a playable shortcut, not the native Skirmish start pipeline. The largest player-visible gap is startup composition: native YR creates houses from packed session/node state, assigns start cells through scenario tables and mode callbacks, then grants starting MCV/units through post-map start generators. Rust currently selects a map, loads it, and seeds at most two MCVs with simplified waypoint pairing.

## Failures And Missing Stages First

| Stage | Verdict | gamemd output | Current Rust output shape | Player-visible consequence |
|---|---|---|---|---|
| Start validation/session packing | FAIL | Start exits only after row count, capacity, same-team, mode acceptance, node/AI arrays, random assignment, options, and preview teardown | `launch_settings` returns selected map, player country, first enabled AI country, credits, player start, short game, zoom | invalid or underspecified starts can enter load; AI rows/colors/team/start/options are lost |
| House creation source/order | PARTIAL | `Create_Houses` consumes `DAT_00A8DA78/84`, `DAT_00A8B274`, AI arrays, credits, start/team fields before terrain and Techno sections | explicit skirmish creates participant `HouseState`s from the launch session, followed by Neutral/Special, before the shared terrain/object spawn funnel; generic scenarios create their map-roster houses at the same pre-object boundary | the foundational house-before-object order now matches; remaining mode/session-field differences are tracked separately |
| Explicit start preassignment | FAIL | Battle-style `+0x80` reads `House+0x16058` into `ScenarioClass+0x1180`; `AssignStartingPoints` consumes that table | selected player start swaps one waypoint to index 0; no start table, no per-AI explicit start | chosen and AI start behavior only matches the simplest cases by accident |
| Start-unit / MCV generation | FAIL | Post-map path uses mode/unit-generation helpers; `DAT_00A8B270` drives a value budget; side/base-unit and spawnability gates apply | `seed_skirmish_opening_if_needed` zips starts with houses and `take(2)`, spawning only two MCVs | games with 3+ players, UnitCount, extra starting units, and side-specific generator behavior mismatch |
| Placement fallback / queued deploy | NOT IMPLEMENTED | starting MCV placement uses native Place plus fallback helpers; MCVDeploy queues normal Deploy mission after placement | `spawn_object` places directly at waypoint; no startup auto-deploy flag or mission queue | blocked starts and MCVDeploy maps/options behave differently |
| Mode-specific callbacks | UNCHECKED / PARTIAL | `Full_Init` calls selected mode `+0x80`; later paths call `+0x84`, `+0x88`, `+0x8C` as appropriate | no selected `MPModesMD.ini` mode object model | Battle-like path partially understood; non-Battle mode starts are not modeled |

## Pipeline Diagram

```text
WM_COMMAND 0x617
  -> FUN_006ACEE0 Start handler
  -> FUN_006AE2C0 modal returns true
  -> selected map/session load path
  -> ScenarioClass::Read_Scenario_INI
  -> ScenarioClass::Full_Init
  -> clear Scenario+0x1180 start table
  -> read/gather map waypoints
  -> ScenarioClass::Create_Houses
  -> selected mode vtable +0x80 preassignment
  -> AssignStartingPoints or mode +0x84
  -> map/object INI load
  -> ScenarioClass::Post_Map_Init
  -> mode/start-unit helper and FUN_005D6D80
  -> crates/final house init/mode post callbacks
```

Current Rust:

```text
OwnerDrawButton::StartGame0x617
  -> launch_settings
  -> start_selected_skirmish
  -> GameScreen::Loading
  -> app_init::load_map
  -> create launch-session houses, then Neutral/Special
  -> spawn terrain, then Units/Aircraft/Infantry/Structures
  -> assign starts and generate the starting force
  -> place scenario-start crates
  -> commit launch alliances
  -> transition to InGame
```

## Trace Stages

### Stage 1 - Start Button Trigger

**gamemd verified finding:** `FUN_006AE3F0` routes `WM_COMMAND` to `FUN_006ACEE0`; command `0x617` is Start only when the notification word is `0`. Successful Start writes `0x617` through the dialog result pointer after packing. Active in YR: Yes. Evidence: existing [SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md); parent-settled facts.

**Rust comparison:** `src/ui/skirmish_shell/state.rs:97` maps `OwnerDrawButton::StartGame0x617` to `SkirmishShellAction::StartGame`, and `src/app.rs:547` immediately builds `SkirmishSettings` and calls `start_selected_skirmish`.

**Verdict:** FAIL. The trigger exists, but the native validation/disable/re-enable/packing lifecycle is absent.

### Stage 2 - Shell Handoff Data

**gamemd verified finding:** `FUN_006ACEE0` commits map token/index mirrors, local node record, seven AI row arrays, active AI count, compact launch table, random country/color resolution, trackbars, checkboxes, and forced launch flags before modal exit. Active in YR: Yes. Evidence: [SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md) plus Ghidra report source list for `0x006ACEE0`.

**Rust comparison:** `src/ui/skirmish_shell/state.rs:70` emits only `selected_map_idx`, `player_country`, first enabled `ai_country`, `starting_credits`, local `start_position`, `short_game`, and `zoom_enabled`.

**Verdict:** FAIL. The Rust output contract is narrower than the native launch contract.

### Stage 3 - Map Selection / Load Entry

**gamemd verified finding:** selected map token/index are mirrored at Start and consumed by the scenario/map load path before or around `Full_Init`; exact loader entry was not expanded in this slot. Active in YR: Yes for non-campaign Skirmish. Evidence: [SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md); `ScenarioClass__Full_Init @ 0x00686B20` spot-check confirms non-campaign init path.

**Rust comparison:** `src/app.rs:411` resolves `available_maps[selected_map_idx].file_name`, stores `GameScreen::Loading { map_name }`, and `src/app_transitions.rs:38` passes it to `app_init::load_map`.

**Verdict:** UNCHECKED. Both select a map, but exact token/index fallback and selected scenario mirrors are not represented in Rust.

### Stage 4 - Full Init Order

**gamemd verified finding:** `ScenarioClass__Full_Init @ 0x00686B20` clears `ScenarioClass+0x1180..0x11C0` to `-1`; in `g_GameMode != 0`, it calls `FUN_0068BDC0`, rules reads, `ScenarioClass__Create_Houses`, selected mode vtable `+0x80`, then either `ScenarioClass__AssignStartingPoints` when `DAT_00A8B244 == 2` or selected mode vtable `+0x84`. Active in YR: Yes; offline Skirmish is mode `5` and follows this non-campaign branch. Evidence: live Ghidra decompile `0x00686B20`.

**Rust comparison (corrected 2026-08-13):** `load_map_from_initial` passes a one-shot house initializer into `spawn_entities`. Explicit skirmish constructs participant houses followed by Neutral/Special; generic scenarios construct the map-roster houses. `spawn_entities` invokes that initializer before terrain objects and map entities. The post-map path no longer clears/recounts houses or rebuilds base reservations. Starting forces and scenario crates remain later, and diplomacy is committed after crates.

**Verdict:** MATCH for the verified house-before-terrain/object order and post-crate alliance timing. Other start-table and mode-callback differences remain scoped to their own stages below.

### Stage 5 - House Creation

**gamemd verified finding:** `ScenarioClass__Create_Houses @ 0x00687F10` creates human houses from `DAT_00A8DA78/84`, then AI houses from `DAT_00A8B274` and `DAT_00A8B29C/B2BC/B2DC/B2FC/B27C`. It writes credits/color, `House+0x16058` start, `House+0x1605C` team/adjunct, difficulty, and local `g_PlayerPtr`. Active in YR: Yes. Evidence: live Ghidra decompile `0x00687F10`.

**Rust comparison (corrected 2026-08-13):** explicit offline skirmish creates one runtime house per normalized launch slot, then Neutral and Special, before any map object. Generic scenarios preserve the map roster at that same boundary. Object Reveal therefore writes owned counts and house-indexed base reservations once against the final house order.

**Verdict:** MATCH for runtime house source/order used by the current explicit offline launch session. Unmodeled native fields or mode-specific callbacks remain separate gaps, not a reason to restore object-before-house repair passes.

### Stage 6 - Start Table / AssignStartingPoints

**gamemd verified finding:** Battle-style mode `+0x80` writes explicit starts from `House+0x16058` into `ScenarioClass+0x1180`; `ScenarioClass__AssignStartingPoints @ 0x005EE9D0` calls `Gather_Start_Positions`, builds a 16-byte occupied table, assigns human houses first, then AI houses. Active in YR: Yes for standard Battle/ManBattle-style Skirmish; conditional for other mode objects. Evidence: [SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md); live Ghidra decompile `0x005EE9D0`; Ghidra xrefs show data vtable references to `0x005D6BE0`.

**Rust comparison:** `src/app_skirmish.rs:47` swaps the chosen local waypoint into vector index `0`; AI explicit starts are ignored. There is no human-first/AI-second assignment pass.

**Verdict:** FAIL. Explicit start assignment semantics are only approximated for the local player.

### Stage 7 - Gather Start Positions

**gamemd verified finding:** `ScenarioClass__Gather_Start_Positions @ 0x00688380` scans waypoints `0..7` until sentinel, counts required non-observer human plus AI starts, and generates fallback random passable starts with an 8x8 clearance when waypoints are deficient. Active in YR: Yes. Evidence: live Ghidra decompile `0x00688380`.

**Rust comparison:** `src/app_skirmish.rs:36` uses `waypoints::multiplayer_start_waypoints`; if fewer than two starts exist, seeding returns `None`. No fallback random start generation exists.

**Verdict:** FAIL for deficient maps; UNCHECKED for exact normal waypoint ordering.

### Stage 8 - Post Map Start Units / MCVs

**gamemd verified finding:** `ScenarioClass__Post_Map_Init @ 0x00686890` runs after map/object load. With a selected mode object it calls vtable `+0x84` then `FUN_005D6D80`; `FUN_005D6D80` exits early if `DAT_00A8B270 <= 0`, computes an eligible unit value budget from spawnable unit/infantry costs and house side masks, then iterates non-special houses through mode callbacks. Active in YR: Yes for Battle-style Skirmish; exact mode callback internals are partially unresolved. Evidence: live Ghidra decompile `0x00686890` and `0x005D6D80`; [SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md).

**gamemd supporting finding:** fallback/null-mode `ScenarioClass__Generate_Random_Units @ 0x006886B0` shows the concrete native pattern: `DAT_00A8B270` budget, start-position gathering, first start random then farthest spacing, `DAT_00A8B258` Bases-gated BaseUnit MCV creation, centered lepton placement, fallback placement, MCVDeploy check, then extra units. Active in YR: Yes/Conditional; standard selected Battle path routes through mode callbacks and `0x005D6D80`, not necessarily the null-mode body directly. Evidence: live Ghidra decompile `0x006886B0`; [MCV_CREATION_STARTING_UNITS_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MCV_CREATION_STARTING_UNITS_DEEP_DIVE.md).

**Rust comparison:** `src/app_skirmish.rs:58` sets credits, then `src/app_skirmish.rs:62` uses `pairings.take(2)` and `spawn_object` for exactly two MCVs. No `UnitCount` budget, all-house loop, side-mask generator, native fallback, or startup deploy queue exists.

**Verdict:** FAIL.

### Stage 9 - Visible Startup State

**gamemd expected output:** enabled players/AI receive houses and starts according to session rows, map waypoints, mode callbacks, UnitCount, Bases/BaseUnit, and placement fallback. Initial crates may also be placed when `DAT_00A8B261` is enabled. Active in YR: Yes/Conditional by options. Evidence: live decompiles above plus [SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md).

**Rust observed shape from source:** at most two MCV entities are added to `Simulation.entities`; credits are set only for spawned pairings; `base_center` and `waypoint_edge` are set for those houses; AI setup is based on playable map houses except local owner.

**Verdict:** FAIL for parity; useful shortcut for smoke testing only.

## Implementation Handoff

| Verified behavior | Rust delta | Affected surface | Acceptance scenario | Proposed test name | Risk |
|---|---|---|---|---|---|
| Start commits full session/node/AI row contract before scenario init | replace narrow `SkirmishSettings` handoff with launch session data including per-row country/color/start/team/difficulty/options | `src/ui/skirmish_shell/state.rs`, `src/ui/main_menu.rs`, launch setup | 1 human + 3 AI rows launch as 4 houses with selected colors/teams/starts | `skirmish_start_packs_all_enabled_rows_into_launch_session` | high; affects all Skirmish starts |
| `Create_Houses` creates runtime houses from launch/session slots, not map roster ordering | build skirmish houses from launch slots and reserve map roster for neutral/special/map-authored data | `src/app_init.rs`, `src/app_skirmish.rs`, `src/sim/house_state.rs` | Dustbowl with 4 enabled players creates exactly those player houses before seeding | `skirmish_create_houses_uses_launch_slots_not_map_roster` | high; house ownership is foundational |
| Native start assignment uses `House+0x16058 -> Scenario+0x1180 -> AssignStartingPoints`, then start-unit generation for all non-special houses | add a start-assignment table and all-house start generator before MCV/unit spawning | `src/app_skirmish.rs`, future scenario-init module | explicit local start and explicit AI starts place each house at requested waypoint; auto starts use native ordering/fallback | `skirmish_explicit_start_table_assigns_human_then_ai` | high; random order and spawn positions are player-visible |

## Negative Facts / Do Not Do

- Do not treat Start Game as a direct spawn command. Active in YR: Yes. Evidence: `0x006ACEE0` only packs and exits; spawn consumers begin in `Full_Init`/post-map reports.
- Do not use `House+0x1605C` as the Battle explicit-start field. Active in YR: Yes for Battle-style mode. Evidence: [SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md) verifies `House+0x16058`; `House+0x1605C` is team/adjunct.
- Do not cap startup to two players. Active in YR: Yes. Evidence: `Create_Houses` loops `DAT_00A8DA84` humans and `DAT_00A8B274` AIs; Rust `take(2)` is a shortcut.
- Do not ignore `UnitCount` just because MCVs spawn. Active in YR: Yes. Evidence: `FUN_005D6D80` reads `DAT_00A8B270` and computes start-unit budget; `Generate_Random_Units` shows the same option family.
- Do not implement deficient waypoint fallback as "no spawn." Active in YR: Yes. Evidence: `Gather_Start_Positions @ 0x00688380` generates random passable fallback starts with 8x8 clearance.

## Remaining Uncertainty

- Exact selected map filename/token loader between `FUN_006AE2C0` returning true and `Read_Scenario_INI` was not expanded.
- Exact standard Battle `+0x84` body at `0x005D6C70` remains partly opaque because it is not a decompiler-defined function in this Ghidra DB; `Post_Map_Init -> +0x84 -> FUN_005D6D80` is verified.
- Exact MCV placement formulas inside the mode-specific callbacks are not fully decoded here; fallback/null-mode `Generate_Random_Units` provides strong pattern evidence but should not be blindly substituted for every mode.

## Sources

- Live Ghidra read-only decompile: `ScenarioClass__Full_Init @ 0x00686B20`, `ScenarioClass__Create_Houses @ 0x00687F10`, `ScenarioClass__AssignStartingPoints @ 0x005EE9D0`, `ScenarioClass__Gather_Start_Positions @ 0x00688380`, `ScenarioClass__Post_Map_Init @ 0x00686890`, `ScenarioClass__Generate_Random_Units @ 0x006886B0`, `FUN_005D6D80 @ 0x005D6D80`.
- Live Ghidra read-only xrefs: vtable/data references to `0x005D6BE0`.
- Existing reports: [SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md), [SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md), [SKIRMISH_START_SESSION_VTABLE_0X14_ACCEPTANCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_SESSION_VTABLE_0X14_ACCEPTANCE_GHIDRA_REPORT.md), [SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md), [SCENARIO_INIT_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SCENARIO_INIT_DEEP_DIVE.md), [MCV_CREATION_STARTING_UNITS_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MCV_CREATION_STARTING_UNITS_DEEP_DIVE.md), [MCVDEPLOY_START_FLAG_AUTO_DEPLOY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MCVDEPLOY_START_FLAG_AUTO_DEPLOY_GHIDRA_REPORT.md).
- Rust scan: `src/ui/skirmish_shell/state.rs`, `src/app.rs`, `src/app_transitions.rs`, `src/app_init.rs`, `src/app_skirmish.rs`, `src/sim/world/world_spawn.rs`, `src/sim/house_state.rs`, `src/sim/game_options.rs`.
