# Shell Substrate Slice 5b - In-Game Options Modal Implementation Plan

Plan only. No Rust code is written by this planning pass.
Date: 2026-06-02
Design: `docs/plans/2026-06-02-shell-substrate-slice5b-options-design.md`

## Objective

Implement the active-YR in-game Options dialog path using the dedicated Options shell surface from the approved design:

- active `0xBBB` full-shell Options chrome,
- shell/non-active `0xF5` control set support,
- verified owner-draw assets and frames,
- native result/apply/persist convention,
- app-layer modal pump contract.

## Non-Goals

- Do not implement a full generic Win32 dialog engine.
- Do not port raw Win32 subclass/vtable architecture.
- Do not move session or modal state into `sim/`.
- Do not implement unrelated keyboard/sound subdialogs without either follow-up research or an explicit scoped handoff. The `0x52C` / `0x52D` state-transition request must still be preserved.
- Do not patch unrelated dirty sim/radio worktree files.

## Prerequisites

Resolved and authoritative:

- [OPTIONS_0XBBB_0XF5_CHROME_OWNERDRAW_ASSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OPTIONS_0XBBB_0XF5_CHROME_OWNERDRAW_ASSETS_GHIDRA_REPORT.md)
- [OPTIONS_PROC_004E1FE0_INIT_PERSIST_PATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OPTIONS_PROC_004E1FE0_INIT_PERSIST_PATH_GHIDRA_REPORT.md)
- [MODAL_PUMP_00623120_SERVICE_TICK_CONTRACT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MODAL_PUMP_00623120_SERVICE_TICK_CONTRACT_GHIDRA_REPORT.md)
- [DLU_TO_PIXEL_FOR_SHELL_DIALOGS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DLU_TO_PIXEL_FOR_SHELL_DIALOGS_GHIDRA_REPORT.md)
- [SKIRMISH_CHECKBOX_TRACKBAR_OWNERDRAW_PAINT_GEOMETRY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHECKBOX_TRACKBAR_OWNERDRAW_PAINT_GEOMETRY_GHIDRA_REPORT.md)

Blocking decision before final clickable-button parity:

- `0x52C` and `0x52D` set `g_GameState` 4 and 6 after result `1`. If the implementation does not include the downstream Keyboard/Sound dialogs, preserve these as explicit pending transitions and document the remaining parity blocker. Do not silently make them behave like Back.
- `0x52D` enabled state comes from `FUN_00407000()`. If no Rust equivalent is identified during implementation, keep active Sound as an explicit gate input and mark the exact source of that input as a blocker before exposing it as fully functional.

Documentation corrections to carry forward:

- Active Options buttons use `SIDEBTTN.SHP`, not `SIDE2B.SHP`, `SDBTNANM`, `MNBTTN`, or PCX pieces.
- `DAT_00b0f9ec` is `SIDEBTTN.SHP`; `SIDE2B.SHP` is a separate global.
- The modal pump keeps messages responsive but does not keep offline world/frame advancement live.

## Implementation Order

Each sub-step should build and test independently. Run Cargo serially only, after checking for active `cargo` / `rustc` processes.

### 1. Options State And Native IDs

Files likely touched:

- `src/ui/shell/mod.rs`
- new `src/ui/shell/options.rs`
- possibly `src/ui/shell/modal.rs`

Work:

- Add `OptionsTemplate::{Active0xBBB, Shell0xF5}`.
- Add native control ID constants for `0x686`, `0x52C`, `0x52D`, `0x529`, `0x52A`, `0x52B`, `0x50F`, `0x601`, `0x602`, `0x604`, `0x51A`, `0x71C`, label/statics.
- Add `OptionsValues` mirroring the verified dialog-facing fields.
- Add `OptionsRuntimeGates` or equivalent app-supplied init inputs:
  - active byte equality result;
  - `g_GameMode`;
  - `0x00A8EDDC` and `0x00A8B538` equivalents for GameSpeed visibility;
  - active Sound enable result from `FUN_00407000()` or an explicit unresolved adapter.
- Add `OptionsDialogState` with pressed button, checkbox states, trackbar values, drag state, live label state, runtime visible/enabled state, result state, and optional pending next-dialog request.
- Apply init gates:
  - hide `0x529`, `0x714`, and `0x671` when `g_GameMode == 0 && 0x00A8EDDC == 0`;
  - hide the same trio when `0x00A8B538 != 0`;
  - enable/disable active `0x52D` from the verified sound gate.
- Keep the module render-agnostic and app-agnostic.

Tests:

- `test_ingame_options_template_selection_and_control_sets`
- `test_options_state_initializes_inverted_speed_and_scrollrate`
- `test_options_0f5_has_shell_only_controls_and_no_keyboard_sound_buttons`
- `test_options_init_hides_gamespeed_triplet_for_native_gates`
- `test_options_init_disables_sound_button_from_active_sound_gate`

Acceptance:

- `0xBBB` and `0xF5` control inventories are separate and exact.
- `0xF5` is not derived from `0xBBB`.
- Hidden controls are not rendered or hit-tested.
- Disabled controls render through the native disabled state where verified and never produce commands.

### 2. Options Layout And DLU Projection

Files likely touched:

- new `src/ui/shell/options_layout.rs`
- `src/ui/shell/geom.rs`

Work:

- Encode parsed `0xBBB` and `0xF5` DLU rect tables from the reports.
- Convert through existing DLU helpers: baseX `6`, baseY `13`.
- Keep resource rects for hidden/disabled controls in the layout table; runtime visibility/enabled filtering belongs in Options state/render/input, not in DLU projection.
- Apply Options-specific resize/anchoring helper behavior from the chrome report:
  - active `0xBBB` owner-draw buttons follow active type-2 helper dimensions/offsets;
  - Back uses the verified helper path;
  - ordinary controls use Options helper behavior and not skirmish global proportional scaling.
- Include title/static finalizer behavior where verified.

Tests:

- `test_options_0bbb_dlu_rects_match_parsed_resource`
- `test_options_0f5_slider_rects_are_148_dlu_wide`
- `test_options_chrome_rect_anchoring_matches_native_helpers`

Acceptance:

- Layout tests fail if `0xF5` reuses `0xBBB` slider rects.
- No proportional scaling path is introduced.

### 3. Render Assets And Options Emitter

Files likely touched:

- `src/render/skirmish_shell_chrome.rs` or a shared shell chrome atlas module
- `src/app_skirmish_shell_render/controls.rs` if small generic helpers are extracted
- new app/render module for Options instances
- possibly `src/render/shell_paint.rs` for shared primitive helpers only

Work:

- Load `SIDEBTTN.SHP` with `SIDEBAR.PAL`, frames `0..=2`.
- Preserve existing `SDBTNANM` entries for `0xF5` Back.
- Add Options button paint role:
  - active `0xBBB`: `SIDEBTTN` frame `0` released, `1` pressed, `2` timer/highlight.
  - shell `0xF5`: `SDBTNANM` frame `2` released, `4` pressed, `3` timer/highlight.
- Reuse or extract trackbar/checkbox paint helpers for `trakgrip`, `trof*`, `cue_i`, `cce_i`.
- Keep `MNBTTN` exclusively for mode-2 message-box modals.
- Render all visible Options controls over the frozen battlefield frame for active offline mode.
- Do not emit hidden GameSpeed controls. Disabled controls must use the verified owner-draw disabled handling when that handling is available; otherwise the enabled-state mismatch stays a named visual blocker.

Tests:

- `ingame_options_0bbb_buttons_use_sidebttn_type2_frames`
- `shell_options_0f5_back_uses_sdbtnanm_type1_frames`
- `options_buttons_do_not_use_mnbttn_or_pcx_pieces`
- `options_trackbars_and_checkboxes_use_verified_callback_assets`
- `options_hidden_gamespeed_triplet_is_not_emitted`

Acceptance:

- Asset selection is testable without a full screenshot.
- Manual screenshot stop gate remains required before merge.

### 4. Input And Result Routing

Files likely touched:

- `src/app.rs`
- `src/app_input.rs`
- new `src/app_ingame_options.rs`
- `src/ui/shell/options.rs`

Work:

- Open Options from the in-game ESC/options path instead of the egui pause-options surface for the parity route.
- Route pointer down/up/move through Options state:
  - button press must match release;
  - hidden controls do not hit;
  - disabled controls do not press or fire;
  - checkbox toggles only on icon rect;
  - trackbar drag/click follows verified bounds and quantization.
- Route key input through dialog handling before global ESC/pause/egui routes.
- Add an explicit key policy for active Options:
  - prevent global ESC from toggling the existing pause menu while Options is active;
  - do not invent an Enter/Escape close result unless active Options dialog-manager translation is verified;
  - if exact Enter/Escape result mapping remains unverified, mark it `UNCHECKED` and keep keys consumed/no-op rather than creating a discard path.
- Produce native results:
  - `0x686` -> result `1`;
  - active `0x52C` -> result `1` plus pending state `4`;
  - active `0x52D` -> result `1` plus pending state `6`;
  - pump game-end -> result `2`.

Tests:

- `options_back_button_result_one_persists`
- `options_keyboard_sound_buttons_require_active_template`
- `options_checkbox_icon_click_toggles_label_click_does_not`
- `options_trackbar_drag_updates_visual_value_and_label`
- `options_disabled_sound_button_does_not_fire`
- `options_trackbar_click_rejects_native_top_y_gate`
- `options_trackbar_thumb_hit_starts_drag_without_remap`
- `options_trackbar_outside_thumb_click_remaps_with_native_quantization`
- `options_trackbar_zero_step_normalizes_to_one`
- `options_escape_is_routed_before_global_pause_and_does_not_discard`

Acceptance:

- No cancel-without-save path exists.
- `0x52C` / `0x52D` do not disappear into Back semantics.
- Existing global ESC pause handling cannot close, pause, or unpause through the old egui route while Options is active.

### 5. Apply And Persistence

Files likely touched:

- new app-level options persistence module
- `src/app.rs`
- `src/util/ini_writer.rs` only if reusable writer support needs extension
- avoid making `src/audio/music.rs` the owner of general Options persistence

Work:

- Apply dialog values on result `1`:
  - `0x529`: internal GameSpeed = `6 - pos`;
  - `0x52A`: ScrollRate = `6 - pos`;
  - `0x52B`: DetailLevel direct;
  - `0x50F`: Difficulty direct, shell/inactive only;
  - `0x601`: UnitActionLines from `BM_GETCHECK == 1`;
  - `0x604`: ShowHidden from `BM_GETCHECK == 1`;
  - `0x602`: ToolTips from `BM_GETCHECK == 1`, with active tooltip-manager update where supported.
- Handle active network GameSpeed by queuing the verified command request rather than immediate store. If the project lacks the command surface, keep this as an explicit blocked acceptance item for network modes; offline can direct-store.
- Write full Options object to `RA2MD.INI`, including pass-through keys that native `WriteToINI` writes from the object.
- Result `2` performs no apply/write.

Tests:

- `test_ingame_options_result_one_applies_then_writes_all_options`
- `test_ingame_options_result_two_skips_apply_and_write`
- `test_ingame_options_gamespeed_network_queues_without_immediate_store`
- `test_ingame_options_checkboxes_map_to_options_bytes`

Acceptance:

- File write order is apply then `RA2MD.INI`.
- Persistence is not limited to changed controls.

### 6. Modal Pump Contract

Files likely touched:

- `src/app_sim_tick.rs`
- `src/app.rs`
- possible app session-mode owner

Work:

- Add `SessionMode` for the app layer with at least campaign/SP, LAN/IPX, WOL/Internet, offline skirmish, and legacy/unknown as explicit cases.
- Add a pure pump decision returning an action rather than a boolean, for example:
  - `MessagesThenNetworkServiceOnly` for modes `0` and `5`, or when blocker globals are set;
  - `MessagesThenAdvanceFixedSim` for modes `3` and `4` only when blockers are clear and reentrant is false;
  - `MessagesOnlyReentrant` when the reentrancy byte is set after the offline/blocker check;
  - `DeferredLegacyMode` or equivalent for mode `1`, mode `2`, and unknown modes until those paths are researched.
- Add service wrapper in native order:
  - process app/dialog/repaint input first;
  - for `MessagesThenNetworkServiceOnly`, call the app/network service hook if one exists; if no equivalent exists in current Rust, keep this as an explicit no-op/deferred side effect and test the selected action;
  - for `MessagesThenAdvanceFixedSim`, call the existing `advance_fixed_simulation`;
  - for `MessagesOnlyReentrant`, do not call fixed sim and do not explicitly call network service.
- Do not change `World::advance_tick`.

Tests:

- `modal_pump_action_matches_gamemd_modes_blockers_and_reentrancy`
- `offline_options_modal_pump_freezes_world_tick_and_keeps_ui_responsive`
- `network_options_modal_pump_advances_fixed_sim_without_sim_layer_dependency`
- `reentrant_modal_pump_processes_messages_without_network_service_or_sim`
- `legacy_modal_pump_modes_are_explicitly_deferred`

Acceptance:

- Offline Options leaves `World.tick` unchanged over N pumped frames.
- Offline/blocker branches select network-service-only, not generic no-op.
- Reentrant branch selects message-only and does not run the network-service side effect.
- Network branch is unit-tested even if dead in current build.

### 7. Integration And Manual Stop Gate

Files likely touched:

- `src/app.rs`
- in-game render/input modules
- options render module

Work:

- Integrate active Options rendering on top of the last battlefield frame.
- Ensure software cursor/OS cursor behavior follows the current shell-modal pattern.
- Ensure closing result resets timing accumulators enough to avoid catch-up burst, while preserving native pump semantics.
- Run focused tests, then one final `cargo check -q`.
- Manual visual stop gate:
  - open active in-game Options at 800x600;
  - verify `SIDEBTTN` button art for Keyboard/Sound/Back;
  - press Back and verify frame `1`;
  - verify trackbars, checkboxes, labels, and `0xF5` wider sliders where shell/non-active path is reachable.

Acceptance:

- No merge/commit until manual in-game Options OK passes.

## Test Run Cadence

Before Cargo:

```powershell
Get-Process cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU
```

Preferred order:

1. Focused unit tests for `ui::shell::options`.
2. Focused render/asset selection tests.
3. Focused app/persistence tests.
4. Focused pump tests.
5. `cargo check -q`.

Do not run multiple Cargo commands in parallel.

## Do Not Do

- Do not use `paint_modal_shp` for Options.
- Do not use `MNBTTN` for Options.
- Do not use `SIDE2B.SHP` for active Options buttons.
- Do not add `0x52C` / `0x52D` to `0xF5`.
- Do not wire `0x51A` ScrollCoasting as if the verified proc reads/writes it.
- Do not add cancel-without-save.
- Do not let the egui Options placeholder become the parity path.
- Do not push modal/session state into `sim/`.
- Do not render or hit-test hidden controls.
- Do not let disabled controls produce result writes.
- Do not call the network-service side effect on the reentrancy-skip pump branch.

## Open Follow-Ups

- Downstream `g_GameState` 4 and 6 behavior for Keyboard/Sound dialogs needs a follow-up trace or a separate design if the Slice 5b implementation exposes those buttons as fully functional.
- Exact source/semantics of `FUN_00407000()` for active Sound enable should be resolved before treating Sound as fully functional.
- Exact active Options Enter/Escape translation through `IsDialogMessageA` remains unverified by the current Options reports; key routing must prevent global ESC bypass meanwhile.
- If the current Rust app has no network-service equivalent, the offline/blocker pump side effect remains a named deferred network hook while preserving no-sim behavior.
- Exact visible behavior of `0xF5` `0x71C` remains deferred.
- Runtime framebuffer RGB diff was not captured for Options; manual visual gate is required before commit.
