# Skirmish Crates Off Initial Crates Trace

**Scenario:** Standard offline Skirmish Battle, one human and one AI, Crates checkbox off, default other options.  
**Scope:** Start Game option packing through scenario post-map initialization and first in-game frame.  
**Report date:** 2026-05-23.  
**Write constraint:** This is the only file written for the trace.

## Summary Verdict

YR packs the Crates checkbox into `DAT_00A8B261`. `ScenarioClass__Post_Map_Init @ 0x00686890` checks that byte after selected-mode startup/unit generation. When it is `0`, YR skips the whole initial random crate placement block, producing exactly `0` initial random crate placement calls for this scenario.

Current Rust carries `state.crates == false` into `SkirmishLaunchSession.options.crates == false`, then into `sim.game_options.crates == false`. Rust has no implemented initial random crate placement consumer, crate slot array, crate regen, pickup, or random crate placement system. For this exact Crates-off first-frame scenario, the observable random initial crate count is still `0`, but that is a vacuous match: the equivalent enabled-path/gated initial-crate system is not implemented.

## Pipeline

`Start Game click` -> `SkirmishShellState.crates=false` -> `SkirmishLaunchOptions.crates=false` -> `SkirmishLaunchSession` -> `apply_skirmish_launch_session` -> `sim.game_options.crates=false` -> post-map setup -> first frame contains no initial random crates.

YR equivalent:

`Start Game command 0x617` -> `DAT_00A8B261=0` -> `ScenarioClass__Post_Map_Init @ 0x00686890` -> `if DAT_00A8B261 != 0` fails -> `MapClass__PlaceCrateAtRandomCell` called `0` times -> first frame contains no initial random crates from this path.

## Stage Trace

### Stage 1 - UI Control State

- Rust input: Crates checkbox off.
- Rust output: `SkirmishShellState.crates == false`; UI control maps `CratesAppear0x696` to `state.crates`.
- Rust evidence: `src/ui/skirmish_shell/state/trackbars.rs:70`, `src/app_skirmish_shell_render/controls.rs:49`.
- YR output: checkbox `0x696` off means `DAT_00A8B261 = 0`.
- YR evidence: [SKIRMISH_CHECKBOX_CONTROL_LABEL_MAPPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHECKBOX_CONTROL_LABEL_MAPPING_GHIDRA_REPORT.md) maps `0x696` to `CratesAppear`, `Rules+0x14B1`, and `DAT_00A8B261`.
- Verdict: **PASS** for this stage (`false == 0`).

### Stage 2 - Start Game Packing

- Rust input: `state.crates == false`.
- Rust output: `options.crates = state.crates`, so `SkirmishLaunchSession.options.crates == false`.
- Rust evidence: `src/ui/skirmish_shell/state/launch.rs:178`, `src/skirmish_launch.rs:144`.
- YR output: Start branch writes `DAT_00A8B261 = (BM_GETCHECK(0x696) == 1)`, so off writes `0`.
- YR evidence: [SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md) section 2, Crates Appear row; Start write `0x006AD806..0x006AD81B`.
- Verdict: **PASS** (`false == 0`).

### Stage 3 - Session To Match Options

- Rust input: `session.options.crates == false`.
- Rust output: `sim.game_options.crates == false` through `SkirmishLaunchOptions::to_game_options`.
- Rust evidence: `src/app_skirmish.rs:177`, `src/skirmish_launch.rs:180`, `src/skirmish_launch.rs:187`, `src/sim/game_options.rs:24`.
- YR output: `DAT_00A8B261` remains the runtime Crates flag consumed by post-map init.
- YR evidence: `ScenarioClass__Post_Map_Init @ 0x00686890` decompile reads `DAT_00a8b261` directly.
- Verdict: **PASS** (`false == 0`).

### Stage 4 - Standard Post-Map Ordering

- Rust order: `app_init.rs` loads/spawns map entities, then calls `apply_skirmish_launch_session`; that creates houses, alliances, start assignments, and opening MCVs.
- Rust evidence: `src/app_init.rs:519`, `src/app_skirmish.rs:162`, `src/app_skirmish.rs:177`.
- YR order: `ScenarioClass__Post_Map_Init @ 0x00686890` runs selected-mode startup via vtable `+0x84`, calls `FUN_005D6D80`, then checks `DAT_00A8B261` for initial crates.
- Ghidra evidence: read-only decompile of `0x00686890` shows `(**(code **)(*DAT_00a8b23c + 0x84))(param_1); FUN_005d6d80();` before `if (DAT_00a8b261 != '\0')`.
- Verdict: **UNCHECKED** for exact timing equality. The broad order is similar enough to locate the stage, but Rust has no frame-level post-map crate stage to compare numerically.

### Stage 5 - Initial Random Crate Gate

- Rust input: `sim.game_options.crates == false`.
- Rust output: `0` initial random crate placement calls, because no initial random crate placement function exists.
- Rust evidence: targeted scan found `crates` only in option/state/hash surfaces and overlay type parsing/render offset; no sim crate placement consumer. Relevant hits: `src/sim/game_options.rs:24`, `src/sim/world/world_hash.rs:94`, `src/map/overlay_types.rs:78`, `src/map/overlay_types.rs:232`.
- YR input: `DAT_00A8B261 == 0`.
- YR output: `0` calls to `MapClass__PlaceCrateAtRandomCell`; the whole block is skipped.
- Ghidra evidence: `ScenarioClass__Post_Map_Init @ 0x00686890` has `if (DAT_00a8b261 != '\0') { ... MapClass__PlaceCrateAtRandomCell(); ... }`.
- Verdict: **PASS** for the Crates-off first-frame observable (`0 == 0` random initial crate calls).

### Stage 6 - Equivalent Initial Crate System

- Rust output: not implemented. There is no crate slot array, random crate placement, regen timer, pickup dispatch, or option-gated initial placement system.
- Rust evidence: no placement consumer in `src/sim`; `CRATE_SYSTEM_GHIDRA_REPORT.md` also records crate gameplay as not implemented in Rust.
- YR output if the same checkbox were on: initial count is `min(max(Rules.CrateMinimum, DAT_00A8B54C), Rules.CrateMaximum)` calls to `MapClass__PlaceCrateAtRandomCell`. In stock `rulesmd.ini`, `CrateMinimum=1`, `CrateMaximum=255`, and default `Crates=yes`.
- YR evidence: Ghidra `0x00686890`; `CRATE_SYSTEM_GHIDRA_REPORT.md` section 3.2; `ini/rulesmd.ini:783..784`, `ini/rulesmd.ini:3034`.
- Verdict: **NOT-IMPLEMENTED**. This does not change the exact off-scenario first frame, but it means Rust does not implement the equivalent initial-crate behavior that the off flag is supposed to gate.

### Stage 7 - First In-Game Frame Visual

- Rust visible result: no initial random crate overlays are added by skirmish post-map setup.
- YR visible result: no initial random crate overlays are added by the random crate path when Crates is off.
- Caveat: pre-authored map overlays are out of scope; this trace is only about initial random crates from the Skirmish Crates option.
- Verdict: **PASS** for this concrete scenario.

## Player-Visible Findings

1. **NOT-IMPLEMENTED - Stage 6:** Rust has no equivalent initial random crate system; Crates-off first frame matches only because the placement path is absent. Evidence: Rust scan hits only option/hash/render metadata; YR `0x00686890` implements the gated placement loop.
2. **NOT-IMPLEMENTED - Stage 6:** Rust has no crate slot/regen/pickup gameplay behind `GameOptions.crates`, so changing Crates on cannot produce YR's initial/ongoing crate behavior. Evidence: `CRATE_SYSTEM_GHIDRA_REPORT.md` primary addresses `0x0056BD40`, `0x0056BBE0`, `0x00481A00`; no Rust consumer found.

No direct FAIL was found for the exact Crates-off initial-random-crate first frame: both engines produce `0` random initial crate placements from this path.

## Sources

- Read-only Ghidra decompile: `ScenarioClass__Post_Map_Init @ 0x00686890`.
- [docs/research/skirmish-ui/SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md).
- [docs/research/skirmish-ui/SKIRMISH_CHECKBOX_CONTROL_LABEL_MAPPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHECKBOX_CONTROL_LABEL_MAPPING_GHIDRA_REPORT.md).
- `docs/research/CRATE_SYSTEM_GHIDRA_REPORT.md`.
- Rust surfaces: `src/ui/skirmish_shell/state/launch.rs`, `src/skirmish_launch.rs`, `src/app_skirmish.rs`, `src/sim/game_options.rs`, `src/map/overlay_types.rs`.

## Verdict Tally

PASS: 5 | FAIL: 0 | UNCHECKED: 1 | NOT-IMPLEMENTED: 1

## Status

COMPLETE
