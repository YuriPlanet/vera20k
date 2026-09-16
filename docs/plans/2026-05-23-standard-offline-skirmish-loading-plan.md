# Standard Offline Skirmish Loading Screen Implementation Plan

> Execute only after user approval. This is an implementation plan, not the implementation.

**Goal:** Replace the current one-frame egui Skirmish loading placeholder with an
app-level pumpable loading path that renders the verified native gamemd.exe standard
offline Skirmish loading background and progress surface while map loading advances.

**Design Doc:** `docs/plans/2026-05-23-standard-offline-skirmish-loading-design.md`

**Implementation Contract:** `docs/contracts/2026-05-23-skirmish-loading-screen-implementation-contract.md`

**Progress Reswarm Sources:**

- `docs/research/LOADING_FUN_0069AE90_SKIRMISH_CALLERS_AFTER_FIRST_RENDERER_GHIDRA_REPORT.md`
- `docs/research/LOADING_FULL_INIT_PROGRESS_SEQUENCE_AFTER_00552D60_GHIDRA_REPORT.md`
- [docs/research/LOADING_READ_SCENARIO_PRE_FULL_INIT_PROGRESS_SETUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOADING_READ_SCENARIO_PRE_FULL_INIT_PROGRESS_SETUP_GHIDRA_REPORT.md)
- [docs/research/LOADING_PROGRESSCLASS_VALUE_MAX_MAPPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOADING_PROGRESSCLASS_VALUE_MAX_MAPPING_GHIDRA_REPORT.md)
- [docs/research/LOADING_DIRECT_DRAW_REPAINT_PRESENT_PATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOADING_DIRECT_DRAW_REPAINT_PRESENT_PATH_GHIDRA_REPORT.md)

---

## Grounding Summary

- Current Rust enters `GameScreen::Loading { map_name }`, draws egui text/panels in
  `ui::main_menu::draw_loading_screen`, presents once, then synchronously calls
  `app_transitions::transition_to_in_game`.
- Current `transition_to_in_game` calls `app_init::load_map`, which performs config
  load, `AssetManager` creation, map parse, theater/rules/art load, terrain build,
  simulation spawn, render atlas creation, sidebar/cursor setup, and final
  `MapLoadResult` hydration in one blocking path.
- Verified standard offline Skirmish loading does not use the first-swarm
  `PUDLGBG*` mode-2 branch. The first verified renderer is `0x00552D60`.
- The verified first renderer draws `ls640/ls800<country>.shp` with `MPLS*.PAL` /
  `MPYLS.PAL`, before milestone `3`.
- Standard Skirmish progress uses `PROGBARM.SHP` frame `0` as a clipped fill, not a
  scaled bar and not a campaign `SPLDBR.SHP` surface.
- Standard selected-map Skirmish initializes one `ProgressClass` lane with max
  `100.0`, null HWND, and direct draw repaint/present behavior.
- Progress repaint cadence is monotonic and discrete: `FUN_0069AE90` admits only
  strict advancing milestone values; duplicate or lower milestone values do not
  repaint or present.
- The verified effective selected-map standard Skirmish milestone skeleton after the
  first renderer is:
  `3,8,12,<changed dynamic theater values in 13..25>,25,30,31,35,45,50,55,58,60,63,65,67,68,69,70,72,74,76,78,82,86,90,93,96,98,100`.
  The `13..25` theater ramp values are conditional changed values, not a guaranteed
  fixed list; the final `25` may also be duplicate-suppressed if the dynamic ramp
  already reached `25`.
  Raw native calls such as `6` after `8`, outer `58` after inner `60`, duplicate
  outer `60`, and final `25` after a ramp that already reached `25` are real call
  sites but visible no-ops when they do not strictly advance the lane.
- `Read_INI_Basic` owns milestones `55`, `58`, and `60`; the following outer
  `Full_Init` checkpoints `58` and `60` are non-advancing on normal selected-map
  Skirmish.
- Progress fill width is `Math__ftol(frame0_width * lane / max)` through gamemd's
  x87 `Math__ftol/FISTP` helper. Do not describe the native standard path as a
  generic floor/truncate/clamp helper or add a native zero-max fallback claim.
- Standard Skirmish ignores campaign `LSLoadMessage`, `LSLoadBriefing`, `Briefing`,
  and `UIName` loading metadata.
- Current `Palette::from_bytes` normalizes PAL components to 255 and bakes alpha.
  Verified gamemd UI/loading conversion shifts components left by two, so `63`
  becomes `252`, and PAL conversion itself does not assign alpha.
- `mmpb.shp` loading marker gates/frames and post-marker localized text are known
  visible follow-ups but remain blocked. Do not guess them in this plan.

## Key Technical Decisions

- Add a first-class app-level `LoadingSession`; do not use egui as the standard
  Skirmish loading renderer.
- Keep final `MapLoadResult` as the app hydration boundary. Extract hydration from
  `transition_to_in_game` before changing load cadence.
- Preserve a synchronous `load_map` compatibility wrapper while introducing a
  pumpable loader. The standard native Skirmish path must use the pumpable path.
- Keep GPU resource creation on the main thread. Do not introduce a worker-thread
  loader in this pass.
- Add a separate gamemd-compatible UI PAL path instead of changing all existing
  palette parsing behavior.
- Put progress monotonicity in a small testable app/render state type before
  wiring the full loading flow.
- Derive loading side from the first/local launch session node country. Do not use
  map theater, sidebar theme, display label, or fallback text as the side source
  when a `SkirmishLaunchSession` exists.
- The first visible `GameScreen::Loading` frame must already contain the native LS
  background. Do not switch to `Loading` and show a black/fallback frame while the
  loading atlas is built after present.
- The selected-map standard Skirmish milestone ledger is now verified enough to
  wire the full app-loop loading replacement for that scope. Keep random-map,
  campaign, multiplayer wait/resend, `mmpb.shp`, and post-marker text paths
  explicitly out of scope until their own evidence closes.
- The loading atlas must be built from the same `LoadingJob`-owned `AssetManager`
  that continues through the map load and lands in `MapLoadResult.asset_manager`.
  Do not create a separate UI-only asset manager for the first frame.
- Treat blocked marker/text layers as explicit extension slots that render nothing
  until the research contracts are closed.

## File Map

| Action | Path | Responsibility |
|---|---|---|
| Modify | `src/app.rs` | Store `LoadingSession`, start native loading from Skirmish launch, render native loading branch, pump after present. |
| Modify | `src/app_transitions.rs` | Extract `apply_map_load_result`; keep synchronous compatibility wrapper. |
| Modify | `src/app_init.rs` | Extract `load_map` internals into phase helpers / `LoadingJob` while preserving `MapLoadResult`. |
| Create | `src/app_loading.rs` | App-level loading request/session/job/progress state and pump orchestration. |
| Modify | `src/main.rs` or module root | Add module declaration for `app_loading` if needed by crate layout. |
| Create | `src/render/loading_screen_chrome.rs` | Native loading atlas for LS background and `PROGBARM.SHP` frame `0`. |
| Modify | `src/render/mod.rs` | Export loading screen renderer module. |
| Modify | `src/assets/pal_file.rs` | Add gamemd UI/loading PAL conversion path and tests. |
| Modify as needed | `src/assets/shp_file.rs` | Add native UI SHP-to-RGBA helper if alpha must be separated from PAL conversion. |
| Modify | `src/ui/main_menu.rs` | Remove/bypass egui loading text for standard Skirmish parity path; retain only fallback/debug use if needed. |
| Modify as needed | `src/skirmish_launch.rs` | Expose first-node/local country mapping helpers if not already available. |
| Modify as needed | tests or app test modules | Add progress, palette, launch filename, side derivation, and flow tests. |

## Parity-Critical Items

| Item | Implementation Home | Verification |
|---|---|---|
| Loading screen remains alive across multiple load steps | `app_loading`, `app.rs`, `app_init.rs` | Flow test: not one-frame transition. |
| First Skirmish renderer uses `ls640/ls800<country>.shp` | `render/loading_screen_chrome.rs` | Atlas tests and visual capture. |
| No `PUDLGBG*` in first standard Skirmish renderer | `render/loading_screen_chrome.rs` | Asset-name assertions / review. |
| Loading side comes from first launch session node country | `app_loading`, `skirmish_launch.rs` | Unit test with varied first node. |
| First renderer exists before milestone `3` | `app_loading::LoadingJob` | State-machine test. |
| Progress uses `PROGBARM.SHP` frame `0` | `render/loading_screen_chrome.rs` | Atlas/frame test. |
| Fill width uses gamemd `Math__ftol(frame_width * lane / max)` | progress model / renderer | Unit test exact/integer-domain values first; keep fractional odd-width expectations pending until exact live FPU rounding is confirmed. |
| Duplicate/lower milestones do not repaint | `LoadingProgressState` | Unit test with `3, 3, 2, 8`. |
| First visible loading frame is native LS art, not black/fallback | `app_loading`, `app.rs` | App-flow test before first pump-after-present. |
| Selected-map visible progress milestones are evidence-backed | `app_loading::LoadingJob` | Milestone ledger test using the verified selected-map sequence. |
| Loading atlas and map load share one asset-manager lifetime | `app_loading::LoadingJob` | Ownership/lifetime test or review invariant. |
| LS background destination geometry is native, not scaled/centered | `render/loading_screen_chrome.rs` | Pixel/screenshot checks at supported widths. |
| No egui/map/status text on standard Skirmish loading | `app.rs`, `ui/main_menu.rs` | Render-path test / screenshot check. |
| Campaign LS metadata ignored in Skirmish | `app_loading` / app init boundary | Fixture map test with LS keys. |
| PAL UI path maps `63 -> 252` | `assets/pal_file.rs` | Unit test. |
| Selected map uses filename token | launch path | Existing/new launch test. |
| `mmpb.shp` and post-marker text are not guessed | renderer extension slots | Review plus absence tests if practical. |

---

## Tasks

### Task 1: Extract App Hydration From Synchronous Transition

**Why:** The pumpable loader still needs to end with the exact current
`MapLoadResult -> AppState` behavior. Extracting that first reduces risk before
changing load cadence.

**Files:**

- `src/app_transitions.rs`
- `src/app.rs` if call sites need small adjustments

**Steps:**

1. Move the large field-assignment block from `transition_to_in_game` into
   `apply_map_load_result(state: &mut AppState, result: MapLoadResult)`.
2. Keep `transition_to_in_game` as a compatibility wrapper that still calls
   `app_init::load_map` and then `apply_map_load_result`.
3. Preserve existing error fallback behavior in the wrapper for now.
4. Ensure no field assignment is dropped during the extraction: radar animation,
   sidebar chrome, cursor visibility, minimap, selection overlay, shroud buffer,
   animation sequences, sound registries, music, spawn-pick transition, and title
   update must still happen.

**Tests:**

- Existing load/start tests should continue passing.
- Add a narrow test if there is an existing `MapLoadResult` fixture path; otherwise
  rely on compile plus smoke launch until later app-flow tests exist.

**Checks:**

- `cargo check`
- Focused existing tests around app init/skirmish launch if available.

### Task 2: Add Gamemd UI Palette Conversion

**Why:** Native loading assets must use gamemd's `component << 2` conversion, not
the current normalized `63 -> 255` parser path.

**Files:**

- `src/assets/pal_file.rs`
- `src/assets/shp_file.rs` only if the SHP conversion needs a separate alpha policy

**Steps:**

1. Add a separate `Palette::from_bytes_gamemd_ui` or equivalently named function.
2. Validate the input size exactly like `from_bytes`.
3. Convert each RGB component with `value << 2`.
4. Do not assign transparency in this PAL conversion path.
5. If current `ShpFile::frame_to_rgba` cannot support this without alpha in the
   palette, add a native UI frame conversion helper that applies the transparent
   index/chroma handling while converting the SHP frame.
6. Leave existing `Palette::from_bytes` behavior unchanged.

**Tests:**

- `pal_file_gamemd_shift_left_two_maps_63_to_252`
- `pal_file_gamemd_ui_does_not_assign_alpha_in_palette_conversion`
- Existing palette tests still assert normalized path maps `63 -> 255`.

**Checks:**

- `cargo test pal_file --lib`

### Task 3: Add Loading Progress And Layout Model

**Why:** Progress monotonicity and clipped-fill math are small, parity-critical
units that should be correct before the renderer or loader uses them.

**Files:**

- `src/app_loading.rs`
- possibly `src/render/loading_screen_chrome.rs` if clip helpers belong there
- module declaration file

**Steps:**

1. Create `LoadingProgressState` with `max_value`, `current_value`, and repaint
   tracking. Standard offline Skirmish starts as one lane with max `100.0` and
   current `0.0`.
2. Add `advance_progress(value) -> bool` with native `FUN_0069AE90` monotonic
   suppression: duplicate or lower values return `false`; only strict advances
   reach the setter/redraw path.
3. Model the setter separately from the callback gate:
   - store `max * 0.01 * percent`;
   - clamp only above max;
   - return without redraw/present when the stored double is unchanged;
   - do not claim a native lower clamp, generic draw-helper `0..=frame_width`
     clamp, or zero-max fallback for the standard path.
4. Add a fill-width helper named after the native behavior, not a generic clip
   helper. It should compute `Math__ftol(frame0_width * lane_value / max_value)`.
   For the initial implementation, document and test the positive selected-map
   domain; keep exact runtime x87 control-word rounding as a narrow follow-up if
   one-pixel fractional cases become disputed.
5. Add a standard Skirmish progress layout helper for the verified base origins and
   width override:
   - default-width base `+0x0C,+0x100`;
   - wider case base `+0x10,+0x141`;
   - width override `0x146/0x196`;
   - keep row/helper insets separate so later Ghidra refinements do not require
     rewriting the base layout function.
6. Add the selected-map standard Skirmish milestone ledger as data or test fixture:
   `3,8,12,<changed dynamic theater values in 13..25>,25,30,31,35,45,50,55,58,60,63,65,67,68,69,70,72,74,76,78,82,86,90,93,96,98,100`.
   Do not hardcode every integer from `13` through `25` as unconditional visible
   output; model the native changed-value ramp and duplicate suppression.
   Keep raw non-advancing calls in a separate test fixture so `6` after `8`, outer
   `58`, duplicate outer `60`, and optional duplicate `25` are asserted as
   no-redraw/no-present cases.
7. Do not add interpolation or smooth progress.

**Tests:**

- `loading_progress_duplicate_milestones_do_not_redraw`
- `loading_progress_lower_milestone_does_not_redraw`
- `loading_progress_advancing_milestone_requests_redraw`
- `loading_progress_clipped_width_matches_native_formula_for_exact_values`
- `loading_progress_fill_width_uses_gamemd_ftol_positive_domain`
- `loading_progress_suppresses_nonadvancing_raw_native_calls`
- `loading_progress_standard_skirmish_selected_map_emits_verified_milestone_ledger`
- `loading_progress_theater_ramp_emits_only_changed_dynamic_values`
- `loading_progress_read_ini_basic_milestones_precede_map_pack_milestones`
- `loading_progress_standard_skirmish_presents_on_advancing_milestones`
- `loading_progress_duplicate_or_lower_milestones_do_not_present`
- `skirmish_load_progress_origin_and_width_match_native`

**Checks:**

- `cargo test loading_progress --lib`

### Task 4: Build Native Loading Screen Atlas

**Why:** The visual surface must come from retail SHP/PAL assets and follow the
existing native shell/sidebar renderer pattern.

**Files:**

- `src/render/loading_screen_chrome.rs`
- `src/render/mod.rs`
- `src/assets/shp_file.rs` if a native UI frame conversion helper was deferred

**Steps:**

1. Define `LoadingScreenAtlas` and `LoadingScreenEntry` similarly to shell chrome
   atlas entries: UV origin, UV size, pixel size.
2. Define a `LoadingSide` / `LoadingArtVariant` mapping for the verified standard
   Skirmish variants. Keep the mapping narrow and evidence-backed.
3. Before implementing asset loading, add a small manifest table in code comments
   or tests that records every supported loading variant:
   - exact `ls640...shp` asset name;
   - exact `ls800...shp` asset name;
   - exact palette name (`MPLS*.PAL` / `MPYLS.PAL`);
   - screen-width rule for selecting the 640 vs 800 asset;
   - source report or asset-existence proof.
   Do not collapse this into a string-format guess unless tests prove all generated
   names exist in retail assets.
4. Load the side/country-specific `ls640/ls800<country>.shp` and palette
   (`MPLS*.PAL` / `MPYLS.PAL`) through `AssetManager`.
5. Load `PROGBARM.SHP` frame `0`.
6. Convert frames with the gamemd UI PAL path.
7. Pack entries into one `BatchTexture`, following the padding/packing style used
   by `skirmish_shell_chrome.rs`.
8. Add draw helpers or data accessors for:
   - full loading background drawn at the verified native destination geometry:
     anchor at screen origin `(0,0)`, no centering, no aspect scaling, and no
     invented letterboxing. The selected 640/800 asset is drawn at native pixel
     size; larger render targets may expose the same surrounding clear/background
     behavior only after that behavior is verified or explicitly accepted.
   - clipped progress bar source rect;
   - future `mmpb.shp` marker slot that is explicitly not drawn yet.
9. Log missing required assets clearly and return `None` or an error state; do not
   substitute unrelated art.

**Tests:**

- `loading_screen_atlas_loads_progbarm_frame0`
- `loading_screen_atlas_uses_gamemd_ui_palette`
- `loading_screen_atlas_does_not_register_pudlgbg_for_standard_skirmish`
- `loading_screen_manifest_asset_names_exist_for_supported_variants`
- `loading_screen_manifest_selects_640_or_800_by_verified_width_rule`
- `loading_screen_background_draws_at_origin_without_scaling`
- `loading_screen_background_larger_target_behavior_is_verified_or_blocked`
- Retail-asset-dependent tests should be gated/skipped consistently with existing
  asset tests if the repo has that pattern.

**Checks:**

- `cargo test loading_screen_chrome --lib`

### Task 5: Add Loading Request, Session, And Side Derivation

**Why:** The loading path needs a real app-level state object that carries map
request data, launch session data, native side, progress state, and loader job.

**Files:**

- `src/app_loading.rs`
- `src/app.rs`
- `src/skirmish_launch.rs` if helper accessors are needed
- `src/ui/game_screen.rs` if the loading enum payload is adjusted

**Steps:**

1. Add `LoadingRequest` with selected map token, optional `SkirmishLaunchSession`,
   and the legacy `SkirmishSettings` fallback needed by current `load_map`.
2. Add `LoadingSession` with request, native screen state, and job state.
3. Add `NativeLoadingScreenState` with side, progress, optional atlas, and first
   renderer readiness flag.
4. Derive loading side from the first/local launch session node country.
5. Define explicit fallback behavior for non-session loading:
   - legacy egui/default paths may use the old wrapper until migrated;
   - standard native Skirmish with a session must not silently guess a side.
6. Add `begin_skirmish_loading(state, selected_map_file, session)` or equivalent
   app helper to initialize the session and set `GameScreen::Loading`.
7. Keep `GameScreen::Loading { map_name }` temporarily if needed for legacy callers,
   but stop using it as the source of native loading truth.

**Tests:**

- `loading_side_comes_from_first_launch_node_country`
- `loading_session_preserves_selected_map_filename`
- `loading_session_rejects_or_falls_back_without_native_session_only_outside_parity_path`

**Checks:**

- `cargo test loading_session --lib`

### Task 6: Extract `load_map` Into Pumpable Phases

**Why:** This is the central architecture change. The player needs real presents
between load milestones, but the final result must remain identical to the current
load path.

**Files:**

- `src/app_init.rs`
- `src/app_loading.rs`
- `src/app_transitions.rs`

**Steps:**

1. Identify the existing `load_map` data dependency boundaries and extract helpers
   without changing behavior:
   - config and `AssetManager` creation;
   - map selection and parse;
   - theater/rules/art/CSF/sequence load;
   - resolved terrain/grid/lighting/tile atlas;
   - house roster/color map/height/bridge data;
   - simulation spawn and launch-session application;
   - entity atlas rebuild;
   - overlay/bridge/sidebar/cursor/font/path grid/final runtime setup;
   - final `MapLoadResult` construction.
2. Introduce `LoadingJob` state that owns the intermediate data required between
   phases. Use owned values, not long-lived borrows into `AppState`.
3. Initialize the `LoadingJob` and its `AssetManager` before the first visible
   loading frame. Build the native loading atlas from that same job-owned
   `AssetManager`, then keep that manager alive through all later phases and move
   it into `MapLoadResult.asset_manager` at completion.
4. Add a `pump(&mut self, state_resources...) -> LoadingPump` API. Pass GPU, batch,
   skirmish settings, and VXL compute only for the duration of the pump call.
5. Emit verified native milestone `3` only after the first loading renderer is
   built/ready.
6. Wire the verified selected-map standard Skirmish milestone ledger to load
   phases. The implementation ledger must preserve each value's owner/source:
   - `3`: `Full_Init` immediately after first renderer;
   - `8`, conditional/suppressed `6`, `12`, changed dynamic ramp values in
     `13..25`, and final `25`: `Init_Theater`;
   - `30`, `31`, `35`, `45`, `50`: outer `Full_Init`;
   - `55`, `58`, `60`: `Read_INI_Basic`;
   - outer `58` and outer `60`: raw calls that should be suppressed after inner
     `Read_INI_Basic` reaches `60`;
   - `63`, `65`, `67`, `68`, `69`: `Read_Map_Section_And_IsoMapPacks`;
   - `70`, `72`, `74`, `76`, `78`, `82`, `86`, `90`: outer `Full_Init`;
   - `93`: `Post_Map_Init`;
   - `96`, `98`: outer `Full_Init`;
   - `100`: final `Read_Scenario` completion.
   Keep random-map generator values and multiplayer wait/resend progress out of
   this selected-map implementation.
7. Keep synchronous `load_map` as a wrapper that runs the same job to completion,
   or keep it as a separate compatibility implementation only briefly while tests
   prove equivalence.
8. Ensure asset manager ownership ends in `MapLoadResult.asset_manager` as today.
9. Preserve error context with `anyhow` and do not swallow phase errors inside the
   pumpable path.

**Tests:**

- If feasible, add a test that pump-to-completion and synchronous wrapper produce
  equivalent high-level `MapLoadResult` fields for a small fixture.
- Add phase-order tests around first renderer readiness and milestone `3`.
- Add a milestone-ledger test for the selected-map sequence and owner/source
  metadata.
- Add tests that raw non-advancing calls do not redraw or present.
- Add an ownership invariant test/review check proving the first-frame loading
  atlas and final map result use one `LoadingJob` asset-manager lifetime.
- Existing map load tests must continue passing.

**Checks:**

- `cargo check`
- `cargo test app_init --lib`
- Any existing skirmish launch/app init tests.

### Task 7: Wire Native Loading Render And Pump Into The App Loop

**Why:** The native Skirmish path must stop drawing egui and stop consuming the
whole load immediately after one presented frame.

**Files:**

- `src/app.rs`
- `src/app_loading.rs`
- `src/ui/main_menu.rs`
- `src/app_transitions.rs`

**Steps:**

1. Add `loading_session: Option<LoadingSession>` to `AppState`.
2. Update native Skirmish start paths to create `LoadingSession`.
3. Before switching to `GameScreen::Loading`, create the `LoadingJob`, create its
   `AssetManager`, and build the required loading atlas from that same manager, so
   the first visible loading frame is native LS art. If required assets are missing,
   fail into the explicit loading-error path rather than showing a black/fallback
   parity frame.
4. Wire this full replacement only for selected-map standard offline Skirmish. It
   may use the verified selected-map milestone ledger above; it must not silently
   claim random-map, campaign, or multiplayer loading parity.
5. In the `GameScreen::Loading` render branch, call `app_loading::render_loading`
   instead of `main_menu::draw_loading_screen` for the standard Skirmish path.
6. Keep a clearly named legacy/fallback egui loading branch only if still required
   by non-native paths.
7. Replace post-present `transition_to_in_game(state)` with
   `app_loading::pump_loading_after_present(state)`.
8. On `LoadingPump::Finished(result)`, call
   `app_transitions::apply_map_load_result(state, result)`.
9. On `LoadingPump::Failed(err)`, log the error and transition to a controlled
   failure/menu state. Do not display Rust-invented loading text as a parity path.
10. Ensure the window mode, zoom reset, cursor visibility, and pending launch session
   cleanup still happen in the correct ownership location.

**Tests:**

- `loading_screen_does_not_transition_after_single_static_frame`
- `loading_first_visible_frame_is_native_ls_background`
- `loading_first_frame_and_map_result_share_job_asset_manager`
- `loading_selected_map_skirmish_uses_verified_milestone_ledger`
- `loading_random_map_or_campaign_does_not_use_selected_map_ledger`
- `skirmish_loading_does_not_render_map_name_or_egui_status_text`
- `skirmish_loading_ignores_lsloadmessage_metadata`

**Checks:**

- `cargo check`
- Focused app/skirmish tests.
- Manual launch smoke test.

### Task 8: Add Visual And Asset Verification Coverage

**Why:** Compilation cannot prove this feature. The target is player-visible native
loading composition.

**Files:**

- Existing visual-check or fidelity-check harnesses under `docs/visual-checks` /
  `docs/fidelity-checks` if present
- Tests near `render/loading_screen_chrome.rs`
- Optional screenshot script/test harness

**Steps:**

1. Add an asset verification test that asserts the selected side variants load the
   expected LS background asset and `PROGBARM.SHP`.
2. Add a render capture or screenshot check for first loading frame:
   - no egui panel/text;
   - background exists;
   - background is anchored at `(0,0)` and drawn at native pixel size;
   - progress bar uses the native origin model;
   - `PUDLGBG*` is absent from this render path.
3. Add progress screenshots or pixel checks at known values to verify clipped fill.
4. Run manual stock Skirmish starts for Allied, Soviet, and Yuri first-player side
   variants.
5. Record any visual mismatch as a follow-up doc/check, not as a hidden acceptance.

**Tests:**

- `loading_first_renderer_draws_ls_country_background_before_progress_3`
- `loading_progress_skirmish_draws_progbarm_frame0_clipped_to_native_percent`
- Visual screenshot comparison/check if the local harness supports it.

**Checks:**

- `cargo test loading --lib`
- Manual run of the app through native Skirmish Start Game.

### Task 9: Document Remaining Blockers And Do Not Guess Them

**Why:** The verified core can land without claiming full loading-screen completion.
The remaining visible layers need targeted research before implementation.

**Files:**

- `docs/contracts/2026-05-23-skirmish-loading-screen-implementation-contract.md`
  if docs are updated later
- Future research docs in `docs/research/`
- `render/loading_screen_chrome.rs` comments only where useful

**Steps:**

1. Ensure `mmpb.shp` loading marker support is represented only as a blocked
   extension point.
2. Ensure post-marker localized text is not replaced with Rust text.
3. Add clear follow-up notes:
   - `/re-investigate 0x00640A40 mmpb loading marker frames and gates`
   - `/re-investigate 0x00552D60 loading text after marker pass`
4. Do not mark the whole loading screen as "full parity complete" until these
   blockers are closed or explicitly accepted as visible drift.

**Tests:**

- Optional absence test that standard Skirmish path does not draw setup-screen
  `mmpb` semantics as a substitute.

**Checks:**

- Review against implementation contract before finalizing the branch.

---

## Suggested Execution Order

1. Task 1: hydration extraction.
2. Task 2: gamemd UI palette path.
3. Task 3: progress/layout model.
4. Task 4: native loading atlas.
5. Task 5: loading session/state.
6. Task 6: pumpable loader phases.
7. Task 7: app loop wiring.
8. Task 8: visual verification.
9. Task 9: blocker documentation.

The order is deliberate: first isolate the current final state application, then
build the small parity primitives, then wire the app-level pump. Avoid starting
with the `load_map` split before the hydration extraction and progress primitives
are tested.

## Acceptance Criteria

- Starting a standard offline Skirmish no longer shows the egui loading panel,
  map name, or explanatory Rust status text.
- The first visible standard Skirmish loading frame is native LS art, not black,
  fallback UI, or a partially initialized frame.
- The LS background is drawn at the verified destination geometry: origin anchored,
  native pixel size, no centering, no scaling, and no unverified larger-target
  behavior.
- Loading does not transition to `InGame` immediately after a single presented
  loading frame.
- The first native loading render uses the side/country LS background and native
  loading palette, not `PUDLGBG*`.
- `PROGBARM.SHP` frame `0` is drawn as a clipped fill using runtime frame width and
  gamemd `Math__ftol(frame_width * lane / max)` semantics for the standard positive
  selected-map path.
- Duplicate and lower milestone values do not repaint or present.
- Selected-map standard offline Skirmish follows the verified effective milestone
  skeleton:
  `3,8,12,<changed dynamic theater values in 13..25>,25,30,31,35,45,50,55,58,60,63,65,67,68,69,70,72,74,76,78,82,86,90,93,96,98,100`.
- Raw non-advancing native calls such as `6` after `8`, outer `58`, duplicate outer
  `60`, and optional duplicate `25` are tested as no-redraw/no-present events.
- The selected map filename token remains the load input.
- The first-frame loading atlas and final map load share one job-owned
  `AssetManager` lifetime.
- The new palette path maps raw component `63` to `252`.
- `sim/` remains independent of render/UI/sidebar/audio/net.
- Remaining `mmpb.shp` and post-marker text gaps are explicitly blocked and not
  guessed.

## Do Not Do In This Plan

- Do not implement campaign loading UI.
- Do not implement mode-2 `PUDLGBG*` as the first standard Skirmish renderer.
- Do not smooth, interpolate, or animate progress between milestones.
- Do not hardcode SHP dimensions; read runtime frame dimensions.
- Do not generate LS asset names by unchecked string formatting; support only
  manifest entries proven against retail assets.
- Do not show a black/fallback first loading frame unless the user explicitly
  accepts that visible drift.
- Do not center, stretch, aspect-fit, or upscale the LS background without verified
  native evidence for that resolution path.
- Do not create a second loading-only `AssetManager` for the native loading atlas.
- Do not use the selected-map milestone ledger for random-map, campaign, or
  multiplayer loading paths.
- Do not add lower-clamp, generic draw-helper source-width clamp, or zero-max
  fallback behavior and label it native standard Skirmish behavior.
- Do not replace blocked native text with "Loading..." or map names.
- Do not move loading orchestration into `sim/`.
- Do not use a worker thread for GPU-resource-producing load phases in this pass.
