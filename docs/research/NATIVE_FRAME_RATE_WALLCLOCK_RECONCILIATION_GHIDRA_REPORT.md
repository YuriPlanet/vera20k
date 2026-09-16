# Native Frame Rate / Wall-Clock Rate — Ghidra Report

**Date:** 2026-05-28
**Target:** Resolve the deferred "45 Hz vs 15 Hz rate" question from the Native
Frame / Tick Contract (roadmap item #2). What rate does `g_CurrentFrameCounter`
actually advance at, is all gameplay timing counted in frame-counter units, and
how does that reconcile with the Rust `SIM_TICK_HZ = 45` / `binary_frame = ms·15/1000`
model?
**Addresses:** `Main_Tick @ 0x0055D360`, `FUN_0055e160 @ 0x0055E160` (throttle/wait),
`GetRadarTimer @ 0x006C8C40`, `SessionClass__ReadSkirmishSettings @ 0x00697F10`,
`CDTimerClass::Start @ 0x0046B640`, `CDTimerClass::GetTimeRemaining @ 0x00426630`
**Active in YR:** Yes. `Main_Tick`, the local-skirmish throttle, the radar-timer
bucket source, and the skirmish-settings read are all on normal YR skirmish paths
(`g_GameMode == 5`).
**Status:** COMPLETE for the rate mechanism and the Rust reconciliation. The exact
achieved wall-clock fps under live render load was not measured (no attach); the
throttle **cap** and its game-speed dependence are verified from the binary.

## Bottom line (verified)

- `g_CurrentFrameCounter` advances **exactly once per `Main_Tick`** — one full
  input→logic→render→service pass. There is **no inner logic loop**; logic runs
  once per loop and the counter increments once, late.
  (`decompile_function 0x0055D360`)
- The per-frame **wall-clock rate is variable, set by the game-speed byte**, not a
  fixed 15 fps. Local skirmish throttles each `Main_Tick` to a minimum period of
  `speed_byte × 16 ms`. (`decompile_function 0x0055E160`, `0x006C8C40`)
- The verified **default YR skirmish speed byte is 1** → minimum frame period
  ~16 ms → **cap ≈ 62.5 fps**. Byte 0 = uncapped; byte N ≈ `1000/(16·N)` fps.
  (`decompile_function 0x00697F10`)
- **All gameplay timing is counted in `g_CurrentFrameCounter` frame units**
  (`CDTimerClass`, `RateTimer`, `AnimClass`, modulo gates). The game speeds up /
  slows down *uniformly* (movement, firing, animation, timers) with the speed
  setting. (`0x0046B640`, `0x00426630`; corroborated by
  `GLOBAL_TIMING_MODEL_GHIDRA_REPORT.md`)
- **"15 fps" is only the INI/art authoring convention** (`900 = 60 s × 15`), not
  the engine's wall-clock rate. It is literally one minute only at game-speed byte
  ~4 (~15.6 fps). At the default byte 1, 900 frames is ~14.4 s of wall clock.

## 1. `Main_Tick` advances the frame counter once per loop, late

`Main_Tick @ 0x0055D360` runs, in order, for an active local skirmish
(`g_GameMode == 5`, `g_GameState == 0`, `g_GameRunning != 0`):

```text
GScreenClass__Input → LogicClass__AI → [House_AI_Tick] → Map__Logic →
RenderFrame_main → FUN_00551a30 (side work) → LogicClassPerTickUpdateLiveVector →
… tactical/UI service … → Network_ServiceLoop →
if (DAT_00a83d49==0 && DAT_00a8ecd0==0 && DAT_008b41c0==0 && DAT_00a83d48==0)
    g_CurrentFrameCounter = g_CurrentFrameCounter + 1;   // late, guarded
FUN_0055e160();   // throttle / wait
```

Key facts for the rate question:
- The whole gameplay pass (`LogicClass__AI`, `Map__Logic`,
  `LogicClassPerTickUpdateLiveVector`) executes **once** per `Main_Tick`. There is
  no loop that runs logic multiple times before the increment, and no loop that
  runs the counter multiple times per render. **One `Main_Tick` = one logic frame =
  one counter increment.** (`decompile_function 0x0055D360`)
- The increment is the single late site behind the four pause/load/non-advancing
  flags — consistent with the Native Frame / Tick Contract.

## 2. The throttle sets the rate from the game-speed byte

For local/menu modes the budget is captured at frame start (`LAB_0055d79e`):

```text
DAT_00887348 = GetRadarTimer();   // start bucket
DAT_00887350 = DAT_00a8eb60;      // budget = live game-speed byte
```

`GetRadarTimer @ 0x006C8C40` is exactly `timeGetTime() >> 4` — a **16 ms bucket**
counter. (`decompile_function 0x006C8C40`, verified literally this session.)

`FUN_0055e160 @ 0x0055E160` is the wait. For local skirmish (`g_GameMode == 5`,
so the network do-while is skipped) it reaches the spin/sleep loop at
`LAB_0055e2e3` which blocks until:

```text
GetRadarTimer() - DAT_00887348 >= DAT_00887350      // elapsed_buckets >= budget_buckets
```

i.e. until `speed_byte` 16 ms buckets have elapsed since frame start. The inner
`Sleep(remaining_buckets)` is a coarse under-sleep; the bucket comparison is the
real gate. (`decompile_function 0x0055E160`.)

Therefore the minimum frame period is `speed_byte × 16 ms`:

| stored speed byte | UI slider (`6 − byte`) | min frame period | fps cap |
|---:|---:|---:|---:|
| 0 | 6 (fastest) | 0 ms | uncapped (render-bound) |
| **1** | **5** | **16 ms** | **~62.5** ← YR skirmish default |
| 2 | 4 | 32 ms | ~31.3 |
| 3 | 3 | 48 ms | ~20.8 |
| 4 | 2 | 64 ms | ~15.6 ← matches the "15 fps" authoring convention |
| 5 | 1 | 80 ms | ~12.5 |
| 6 | 0 (slowest) | 96 ms | ~10.4 |

Achieved fps = `min(cap, render+logic throughput)`. On modern hardware throughput
≫ cap, so the achieved rate ≈ the cap. The cap, and its game-speed dependence,
are verified; the exact achieved fps under live render load was not measured.

### Network mode (not skirmish) uses a millisecond budget, not buckets

For `g_GameMode != 0 && != 5`, `Main_Tick` instead computes
`DAT_00887330 = 1000 / DAT_00a8b558` (ms) and `0x3c / DAT_00a8b558` (≈ buckets),
where `DAT_00a8b558` is the network frame-rate value (written by `Main_Game` init
and `EventClass__Execute`; read by `Main_Tick`, `House_AI_Tick`,
`HouseClass::Begin_Production`). This is the "`1000/speed`" formula; it is the
**network** path and does **not** apply to local skirmish. (`get_xrefs_to 0x00a8b558`.)

## 3. Default skirmish speed byte = 1

`SessionClass__ReadSkirmishSettings @ 0x00697F10`:

```text
param_1[2] = CCINIClass__ReadInt(ini, "GameSpeed", *(RulesClass + 0x14A0));
```

`RulesClass + 0x14A0` is `[MultiplayerDialogSettings] GameSpeed`, which YR
`rulesmd.ini` sets to `1` (base `rules.ini` is `0`). The value is read **directly**
as the stored byte — no inversion at this layer. It propagates to the live speed
byte `DAT_00a8eb60` via the session/game-option packet apply
(`FUN_005B67F0`, per `GLOBAL_TIMING_MODEL_GHIDRA_REPORT.md`). The in-game options
slider uses the inverse mapping `stored = 6 − slider_position`
(`OptionsClass__ApplyFromInGameDialog`), so byte 1 = slider position 5
(second-fastest). (`decompile_function 0x00697F10`.)

## 4. All gameplay timing is in frame-counter units

Re-confirmed this session and in `GLOBAL_TIMING_MODEL_GHIDRA_REPORT.md`:
`CDTimerClass::Start` stores `g_CurrentFrameCounter` + a frame duration;
`GetTimeRemaining` derives `elapsed = g_CurrentFrameCounter − start` on read.
`RateTimer` (facing/interpolation) and `AnimClass` likewise key off the frame
counter, and several systems use `g_CurrentFrameCounter % N` modulo gates. None of
these is wall-clock-ms based. Consequently, raising or lowering the game speed
scales the wall-clock duration of **every** frame-counted effect uniformly —
movement, reload, build, ore growth, animation playback. (`0x0046B640`,
`0x00426630`.)

This resolves the apparent "62.5 fps vs 900 = 1 minute" contradiction: animations
and timers authored against the nominal 15 fps reference simply play faster at
faster game speeds, exactly as retail does. There is no fixed 15 fps in the engine.

## 5. Reconciliation with the current Rust model

> **2026-09-16 correction (Rust-side only — every binary finding above stands):**
> everything in this section below this block describes a Rust model that no
> longer exists. `e163a3aa` (2026-07-30, "phase0: implement retail deterministic
> authority spine") replaced it; the corpus citing it was first tracked wholesale
> by `5aaa9c22` (2026-08-18) without revalidation. Current state, read from
> source this date:
>
> - **One admitted frame is one native frame.** `advance_master_frame` commits
>   exactly once per call (`world/mod.rs:5836`, `binary_frame.wrapping_add(1)` in
>   `run_late_region`), and the exact-step receipt rejects any `frame_delta != 1`.
>   The table's `binary_frame = (total_sim_ms · 15)/1000` at "~21 / s
>   (= sim.tick / 3)" is dead, and with it the "one logic frame is modelled as
>   3 sim-ticks" bullet below. There is no sub-stepping.
> - **Admission is the native 16 ms bucket gate.** `LocalFramePacer::should_admit`
>   (`app/match_runtime/frame_pacer.rs`) compares `timeGetTime() >> 4` buckets
>   against `game_speed.clamp(1, 6)`, so stock `GameSpeed=1` admits one frame per
>   16 ms bucket — ~62.5/s, mirroring the `Main_Tick` throttle §3 establishes from
>   the binary. Byte 0 short-circuits to uncapped, as native does.
> - **Movement integrates per native frame.** Locomotors advance `speed × dt` with
>   `dt = native_movement_frame_fraction()` = 1/15 (`movement_tick.rs:3842,3997`),
>   and `movement_frame_budget_from_current_speed` is `speed / 15` — one native
>   frame's leptons per admitted frame. Nothing multiplies by the 22 ms tick, which
>   is what the "~3× slower" arithmetic below rests on.
> - **The combat clause is stale.** `GAME_FPS` no longer exists under
>   `src/sim/combat/`; `rof_to_cooldown_frames` (`combat/mod.rs:3367`) consumes ROF
>   frames directly. `advance_fixed_simulation` does not exist anywhere in `src/`,
>   and `app_sim_tick.rs` / `app_types.rs` are now `app/match_runtime/sim_tick.rs`
>   and `app/types.rs`.
>
> **Status of the pace claim: structurally matched, wall-clock unmeasured.** This
> is a code-and-binary read; no measurement was taken on either side, so it is not
> parity demonstrated. One residual is live and separate: `render/gpu.rs:184`
> hardcodes `wgpu::PresentMode::Fifo` and admission is reachable only from the
> render pass (`app/frame.rs:142`) with no catch-up loop, so the achieved logic
> rate is `min(display refresh, 62.5/s)` — about 4% under native on a 60 Hz panel,
> and never repaid.

Verified from source:

| Rust clock | Definition | Real rate at default (byte 1) |
|---|---|---|
| `sim.tick` | +1 per `advance_tick` (one full logic pass) | tps-scaled: ~63 / s |
| `binary_frame` | `(total_sim_ms · 15) / 1000`, `total_sim_ms += tick_ms` | ~21 / s (= sim.tick / 3) |
| wall clock | `scaled_elapsed = elapsed · sim_speed_tps / SIM_TICK_HZ` | real time |

- `SIM_TICK_HZ = 45` (`util/fixed_math.rs:51`), `SIM_TICK_MS = 22`
  (`app_types.rs:27`). The doc-comment on `SIM_TICK_HZ` still says "matches RA2's
  native 15 fps … every sim tick equals one RA2 game frame" — **stale**: the
  constant is 45, not 15.
- One logic *frame* is modelled as **3 sim-ticks** (45 / 15). This is internally
  consistent across systems:
  - Movement: `ra2_speed_to_leptons_per_second = leptons_per_tick · 15`
    (`fixed_math.rs:370`), stepped by `dt = tick_ms/1000`, so 3 ticks of 22 ms =
    one native frame's worth of travel. Comment: "baseline = gamemd Slowest."
  - Combat ROF: `rof_to_cooldown_ticks` uses `GAME_FPS = 15`
    (`combat/mod.rs:73,2318`): `N` ROF frames → `N·1000/15` ms → `≈ 3N` sim-ticks.
  - `binary_frame` advances once per 3 ticks → 15 frames per 1000 ms of sim time.
- Real-rate scaling: `advance_fixed_simulation`
  (`app_sim_tick.rs:237`) multiplies elapsed by `sim_speed_tps / SIM_TICK_HZ`.
  `tps_for_game_speed(1) = 63` (`app_types.rs:36-46`), so default real rate ≈
  `15 fps × (63/45)` = **~21 logic frames/s**.

> **2026-05-29 correction (Rust-side only):** `tps_for_game_speed` has since been
> refactored to the exact bucket formula §6 recommended below — it no longer uses
> the old hardcoded nominal-15 × tps approximation. It now returns 60 for stored
> byte 0 (uncapped → capped at 60) and otherwise `(1000 + bucket_ms/2)/bucket_ms`
> with `bucket_ms = stored × 16` (`GAME_SPEED_BUCKET_MS = 16`), i.e. the rounded
> form of gamemd's `1000/(speed_byte × 16 ms)` cap. `tps_for_game_speed(1)` still
> equals 63 (`(1000+8)/16 = 63`), so the ~21 logic-frames/s default above is
> unchanged. (Verified `app_types.rs:31-42` this session.) No binary claim changes.

### The gap

- **Architecturally faithful mapping:** one `advance_tick` = one full logic pass =
  the true analog of one `Main_Tick`/`g_CurrentFrameCounter` increment. So
  `sim.tick` — not `binary_frame` — is the structural twin of the native frame
  counter.
- **Rate mismatch:** native byte 1 runs the whole game at ~62.5 logic frames/s
  (cap); the current Rust model runs it at ~21 logic frames/s (15 fps nominal ×
  1.4 tps). At the *same* "default" game-speed setting, Rust plays the game ~3×
  slower in wall-clock than gamemd. This is a player-visible pace disparity (unit
  speed, reload cadence, build time, animation playback all ~3× slow) — not an
  internal-only difference.
- The mismatch is uniform (every frame-counted system shares the same nominal-15
  calibration), so the *relative* timing between Rust systems is self-consistent;
  it is the *absolute* wall-clock pace versus gamemd-at-the-same-speed that drifts.
- `binary_frame` advancing at 1/3 of `sim.tick` is an independent oddity: it is the
  logic-frame clock, yet the actual per-frame logic work (movement, combat, AI)
  happens every `sim.tick`. Whether logic should run once per native frame (and
  `sim.tick == binary_frame`) or be deliberately sub-stepped ×3 for render
  smoothness is the open architecture decision below.

## 6. Design options (no code; user decision required)

> **2026-09-16 correction:** this decision is closed in substance. `e163a3aa`
> (2026-07-30) shipped Option A's shape — one logic pass per admitted native
> frame, a single frame clock, durations used as frame counts, and the
> speed-byte → 16 ms bucket admission both options required. What follows is
> retained as the reasoning that led there, not as an open choice. What remains
> open is narrower and was not contemplated here: the render loop is the only
> caller of frame admission and vsync is pinned to `Fifo`, so the achieved rate
> is capped by display refresh rather than by the game-speed byte.

The faithful target is: the game-speed setting must produce the **same observable
pace** as gamemd at that setting (parity bar — outputs, especially pace). That
requires the default skirmish to run logic at ~62.5 frames/s, not ~21. Two
architectures can deliver it:

- **Option A — one `advance_tick` per native frame (collapse the ×3).**
  `sim.tick == binary_frame == g_CurrentFrameCounter`; a single frame clock.
  `advance_tick` is invoked at the game-speed rate (byte 1 → ~62.5/s). All
  durations are frame counts used directly (drop the `/15` and `·15` conversions;
  movement applies `Speed` per frame). Render interpolates entity positions
  between frames for smoothness. Maximally faithful, single clock, makes the
  late-commit contract trivially correct. Largest change; per-step movement is
  chunkier without render interpolation.

- **Option B — keep ×3 sub-stepping, raise the rate to native.**
  Keep 3 sim-ticks = 1 logic frame for smooth movement, but recalibrate so frames
  advance at native's game-speed fps (byte 1 → 62.5 frames/s → ~187.5 sim-ticks/s).
  Fix `tps_for_game_speed` to map the speed byte to native fps and make
  `binary_frame` track it. Keeps render-smooth sub-stepping, smaller conceptual
  change. ~187 sim-ticks/s is heavy; still two clocks at a fixed 3:1; not a literal
  mirror of native (native has no sub-steps).

Both also require the **game-speed → rate mapping** to reproduce gamemd's
`speed_byte × 16 ms` period (byte 0 uncapped … byte 6 ~10.4 fps).

> **2026-05-29 update (Rust-side only):** this specific recommendation —
> "replace the nominal-15 × tps approximation with the `speed_byte × 16 ms`
> bucket mapping" — is **already implemented**. `tps_for_game_speed`
> (`app_types.rs:36-42`) now returns 60 for byte 0 (uncapped → capped at 60) and
> otherwise `(1000 + bucket_ms/2)/bucket_ms` with `bucket_ms = stored × 16`,
> the rounded form of `1000/(speed_byte × 16 ms)`. So per-byte caps match the §2
> table (byte 1 → 63, byte 2 → 32, byte 4 → 16, byte 6 → 10). What remains for
> Option A/B is the *architecture* choice above (collapse the ×3 vs. raise the
> sub-step rate), not the speed-byte→cap mapping. (Verified `app_types.rs:31-42`
> this session.)

A prerequisite worth a live check before committing: confirm the achieved gamemd
default fps (attach + sample `g_CurrentFrameCounter` delta/sec over ~30 s at byte 1)
to validate the ~62.5 cap assumption against real render load.

## Sources

- `decompile_function 0x0055D360` (`Main_Tick`) — single late counter increment;
  no inner logic loop; local-skirmish budget capture.
- `decompile_function 0x0055E160` (`FUN_0055e160`) — local throttle blocks until
  `GetRadarTimer() - start >= speed_byte` buckets.
- `decompile_function 0x006C8C40` (`GetRadarTimer`) — `timeGetTime() >> 4` (16 ms).
- `decompile_function 0x00697F10` (`SessionClass__ReadSkirmishSettings`) — GameSpeed
  default = `RulesClass+0x14A0` (rulesmd `[MultiplayerDialogSettings] GameSpeed=1`).
- `get_xrefs_to 0x00a8b558` — network frame-rate value (`1000/x` ms budget path).
- `CDTimerClass::Start 0x0046B640`, `GetTimeRemaining 0x00426630` — frame-counter
  timers.
- `docs/research/GLOBAL_TIMING_MODEL_GHIDRA_REPORT.md`,
  `docs/research/FRAME_COUNTER_PREINCREMENT_VS_RUST_BINARY_FRAME_GHIDRA_REPORT.md`.
- Rust: `util/fixed_math.rs:51,107,370`, `app_types.rs:27,36-46`,
  `app_sim_tick.rs:237`, `combat/mod.rs:73,2318`, `world/mod.rs:1894-1896`.
```
