# Shell Substrate Slice 4 — Fold Skirmish Controls onto the Substrate — FINAL Implementation Plan

> **Re-verification preamble (load-bearing).** Every skirmish/substrate line range below was re-read by CONTENT this session against current source. Parallel sessions are editing `src/app_skirmish_shell_render.rs`, `src/app_single_player_shell_render.rs`, and `src/app_main_menu_shell_render.rs` (git status shows them dirty), and `src/render/shell_paint.rs` is UNTRACKED (`?? src/render/shell_paint.rs` — Slice 3's paint pass is NOT yet committed). **Anchor on function/symbol names, never line numbers, and re-verify by CONTENT immediately before each edit.** If a cited block shows another session's in-progress edits, WAIT — do not rewire, fix, revert, or stash (per `feedback`/CLAUDE.md parallel-sessions rule).

> **Scope this run:** Slice 4 only. It folds the SKIRMISH dialog (0x102) heterogeneous controls — checkbox, trackbar, combo/dropdown, choose-map listbox — onto the shared `ui::shell` substrate as first-class `ControlKind`s; lands the deferred per-`ControlKind` paint+input dispatch (the "`OwnerDrawControl`" seam Slice 3 punted); unifies the two skirmish scroll models into one; and makes the C14 `[MultiplayerDialogSettings]` defaults seed byte-exact for the modeled keys. **Biggest blast radius of the six slices; delivered as a 4-PRE design gate + SIX independently buildable sub-cycles, each with its own build+tests-green checkpoint and a hard STOP-and-revert rule.**

---

## 0. Grounding summary (live-src re-verified THIS run)

Two corrections to the orchestration brief, both confirmed this session:

- **CORRECTION 1 — the control state machine lives under `src/ui/skirmish_shell/`, NOT the flat `src/skirmish_*.rs` files.** The flat `src/skirmish_*.rs` + `src/app_skirmish*.rs` files are the LAUNCH / SCENARIO-DATA layer (session building, MCV seeding, mode/scenario lists). The interactive controls live in `src/ui/skirmish_shell/state/{combos.rs, trackbars.rs, choose_map.rs, hit_test.rs}` (state machine) + `src/ui/skirmish_shell/layout.rs` (all rects/constants), with paint in `src/app_skirmish_shell_render/{controls.rs, modals.rs}`. Slice 4 targets the `ui/skirmish_shell/` layer. (Open question O7 confirms this layering target.)
- **CORRECTION 2 — there is NO `OwnerDrawControl` trait in code.** `OwnerDrawControl` appears only in docs (zero source matches). Slice 3 shipped free functions + the render-side `ButtonPolicy`/`ArtFit` data enum, NOT a trait. `render/shell_paint.rs` dispatch today branches ONLY on `ButtonPolicy.art_fit` — there is NO `match` on `ControlKind` anywhere. Slice 4 introduces the per-`ControlKind` dispatch.

**CONFIRMED substrate facts (verified by Read this session):**
- `ControlKind` (`ui/shell/descriptor.rs:21-32`) declares 9 fieldless variants; only `Button`/`Static` are exercised; the other 7 are placeholders "the skirmish controls (Slice 4) will fill in."
- `LaidOutControl` (`ui/shell/layout.rs:16-20`) carries **only `id: u16` + `rect: RectPx`** — NO kind field. `layout_pass` (`:24-42`) maps over `desc.controls`, never reading `ControlKind`.
- `DialogController` (`ui/shell/controller.rs`) is **button-only**: `on_pointer_down` (:149) latches a pressed `u16`; `on_pointer_up` (:158-175) fires a `u16` ONLY when release hits the SAME pressed id and it is not disabled; `on_pointer_move` (:179) updates hover; `on_key` (:192) always returns `false`. `hit_any` (:210-215) is a flat first-match `find(|c| c.rect.contains)` returning `Option<u16>`. There is NO down-path, move-drag-path, or wheel-path that returns into per-control state. There is a LIFO `stack` + registration-order `kbd_route`.

**CONFIRMED skirmish facts (verified by Read this session):**
- **Checkbox** (`state/trackbars.rs:66-74,173-180`; `layout.rs:15-17,276-287`): five bools; toggle ONLY on the 18×18 `checkbox_icon_rect`, NOT the full rect; hover/status-help uses the FULL `checkbox.rect`; `CHECKBOX_ICON_W/H=18`, `CHECKBOX_TEXT_LEFT_OFFSET=26`; toggle plays `GuiCheckboxSound`.
- **Trackbar** (`state/trackbars.rs:15-25,35-52,160-245`): `GAME_SPEED 0..6/1`, `CREDITS 5000..10000/100`, `UNIT_COUNT 0..10/1`; `TRACKBAR_MOUSE_X_BIAS=6`, `TRACKBAR_MIN_CLAMP_X=1`, active width `= w-50-13`, `TRACKBAR_THUMB_W=12`; Y-gate accepts only bottom 18px; thumb-hit → value-tracking drag (`dragging_thumb=true`, follows cursor); rail click jumps ONCE and does NOT follow (`handle_option_mouse_move` early-returns on `!dragging_thumb`); `game_speed` stored INVERTED vs visual (`visual = GAME_SPEED_MAX - stored`); value change pushes HSCROLL + `GenericClick`.
- **Combo — scroll Model A** (`state/combos.rs:142-257,635-728`; `layout.rs`): cursor = `OpenComboDropdown.top_index` FUSED with the open-state `Option`; `COMBO_DROPDOWN_ROW_H=23`; per-combo max-visible Side=7 / Color/Start=9 / AiType/Team=0(unbounded); `thumb_h = (track_h*visible_rows/item_count).clamp(MIN_THUMB_H=14, track_h)`, `track_h = scrollbar.h - 2*BUTTON_H(22)`; **`combo_dropdown_thumb_height` has an `item_count==0` early-return → `track_h.max(MIN_THUMB_H)`** (`:144-145`); thumb-DRAG supported (`top_index_from_thumb_y:214-235`, uses `mouse_y - grab_offset_y`); track-click (`top_index_from_scrollbar_track_click:237-257`, uses `mouse_y - thumb.h/2`); wheel INERT; captured popup; reverse hit order; chrome-click consumes-without-close; off-click closes; two-clicks-to-switch.
- **Choose-map listbox — scroll Model B** (`state/choose_map.rs`; `layout.rs:644-693`): cursors = TWO bare `usize` fields `mode_top_index`/`map_top_index`; `CHOOSE_MAP_LISTBOX_ROW_H=19`, `visible_rows = rect.h/19` (geometric, no per-control cap); same thumb formula reimplemented separately reusing `BUTTON_H(22)`/`MIN_THUMB_H(14)`; **`choose_map_listbox_scroll_thumb_rect` returns `None` on `row_count==0 || visible_rows==0`** (`:655`) — a STRUCTURALLY DIFFERENT empty-list path from Model A's `track_h.max(MIN_THUMB_H)`; track-click `mouse_y - thumb.h/2` (`:687`, identical rounding to Model A but fed by a different `max_top` via the different visible-row source); thumb DRAG NOT implemented; `max_top = row_count.saturating_sub(visible_rows)`. **Input currently lives in the APP layer**, `app.rs:979/1080/1130`, NOT the ui module.
- **Choose-map WHEEL** (`app.rs:1130-1162`) has FOUR behaviors, ALL of which must survive the migration: (a) `lines==0.0` → `return true` (consume, NO scroll); (b) `lines>0.0` → `rows = -(ceil(|lines|).max(1))`; (c) `lines<0.0` → `rows = +(ceil(|lines|).max(1))`; (d) cursor over `map_list` scrolls map (checked FIRST), else over `mode_list` scrolls mode, else `return true` (consume, NO scroll). BOTH lists are wheel-scrollable.
- **C14 defaults** (`ini/rulesmd.ini:3017-3042`; `sim/game_options.rs:58-78`): 17 keys modeled in `GameOptions::default()` are byte-exact. The seed chain is `GameOptions::default → SkirmishLaunchOptions::default → SkirmishShellState::default` (verify the chain preserves byte-exactness, not just `game_options.rs`). `GameOptions::default()` is a hardcoded literal — NO runtime INI parse (only per-mode `AlliesAllowed`/`MustAlly` is parsed in `skirmish_modes.rs`). **[STALE as of 2026-06-12: commit `1f54995f` added the runtime `GameOptions::from_multiplayer_dialog_settings` parse + `launch_options_base` carry chain; `default()` stays a hardcoded fallback but the live path now parses the section. See the 4F status banner.]**

**Test counts (the unchanged safety net):** `state/tests.rs`=87, `layout.rs`=30, `state/player_name.rs`=1, `app_skirmish_shell_render.rs`=53, `controls.rs`=3, `text.rs`=11, `app_skirmish.rs`=20, `skirmish_launch.rs`=7, `skirmish_scenarios.rs`=12, `skirmish_modes.rs`=9.

**Explicitly UNCHECKED / out of this slice (stated so the C-contract boundary is unambiguous):**
- 0x102's keyboard routing (player-name Tab/Esc in `state/player_name.rs`) and the Start(0x617)/Back(0x5C0)/ChooseMap result-code routing through the controller's press-must-match path — **DEFERRED; NOT in Slice 4** (see §5.7). This slice migrates control *interaction* (checkbox/trackbar/combo/listbox), not the dialog's keyboard/button result-routing contract (C3/C4).
- Whether gamemd's 0x102 surfaces the ~11 GameOptions-only keys as actual child widgets the Rust shell is MISSING — **UNCHECKED, raised as a Ghidra pre-req in 4F (O5), not assumed away.**
- Replacing `GameOptions::default()` with a runtime INI parse — larger than Slice 4.

---

## 1. Architecture — the dispatch seam (concrete data path) and thin callers

### 1.1 The per-`ControlKind` dispatch seam — RESOLVED CONCRETELY (was a hole; reviewers 1+2+3 all flagged)

The draft described the seam at policy level ("controller dispatches into existing skirmish mutators") but never said HOW `ControlKind` reaches the dispatch, given `LaidOutControl` carries no kind. **Decision (least-invasive, matches current architecture — Option (c) of Review 1):**

- **Input dispatch STAYS in the skirmish layer**, keyed off the existing per-kind id enums (`SkirmishCheckboxId`, `SkirmishTrackbarId`, `SkirmishComboId`, `ChooseMapListboxId`). `handle_option_mouse_down/move/wheel` and the choose-map handlers remain the behavior owners and the precedence owners. The generic `DialogController` does NOT gain a `&mut SkirmishShellState` parameter (that would drag a skirmish type into the render-agnostic substrate and violate ui-layering).
- **The substrate's role for 0x102 this slice is narrow:** the descriptor table provides the dialog template + `layout_pass` rects; the controller continues to own only press-must-match-release for the THREE owner-draw buttons (Start/Back/ChooseMap) if/when those are wired (deferred — see §5.7). The heterogeneous controls dispatch through a thin per-kind seam co-located with the skirmish state, NOT through `DialogController::on_pointer_*`.
- **`LaidOutControl` does NOT gain a `kind` field, and `ControlDescriptor`/`ControlKind` stay fieldless.** The `ControlKind` enrichment of the descriptor table is used ONLY by the PAINT seam (§1.2), where a render-side `match ControlKind` selects the emitter; it is NOT used to route input through the generic controller.
- **Consequence:** 4A–4D are call-site re-expressions inside `ui/skirmish_shell/` + `app.rs`, NOT a `DialogController` API redesign. This keeps the substrate button-only and render-agnostic. (O1/O2 record the trait-vs-enum and ownership defaults that this resolves; they are RESOLVED here, not deferred into 4A.)

### 1.2 Paint seam — render-side `match ControlKind`

`render/shell_paint.rs` (currently branching only on `ButtonPolicy.art_fit`) gains a render-side `match ControlKind` that selects the correct emitter (`paint_checkbox`/`paint_trackbar`/`paint_combo`/`paint_listbox`), fed a render-side `ControlPaint` input carrying the resolved per-control state (checked / visual-value+thumb-rect / open-dropdown rows+top_index / listbox rows+top_index) read-only from the skirmish state. Per-control paint policy (frame index, PCX names, swatch colors, depth) stays render-side; NO render type leaks into `ui::shell`. **The actual current emitters live in `app_skirmish_shell_render/{controls.rs,modals.rs}` (`push_checkbox_instances`, `push_trackbar_instances`, `push_combo_instances`, `push_dropdown_*_instances`), NOT in `paint_buttons`** — the seam re-homes the dispatch SITE; it must preserve the existing emission ORDER and z-ordering (see §1.4).

### 1.3 Why this reproduces skirmish bit-for-bit

Every behavior stays in its current skirmish function; the math (clamp, track-click, thumb-height, value-quantize, inversion) is untouched. The 87+30 tests pin output and act as the regression net. The unification (4E) is the only sub-step that *touches* the math; it is gated by an explicit equivalence PROOF.

### 1.4 Paint draw-ORDER invariant (Review 3 gap — pinned)

The per-`ControlKind` dispatch changes the emission SITE, so the inter-control draw order and the **open-dropdown-on-top z-ordering (an open dropdown's instances MUST be emitted AFTER / over all sibling combo faces and other in-band controls)** and the depth constants must be preserved byte-for-byte. **Action:** capture the current `app_skirmish_shell_render` draw sequence (the ordered list of emitter calls + their depths) as a reference, and add a draw-list assertion that the new `match ControlKind` path reproduces it exactly (instance count / uv / depth / position per control state). A 1-frame/1-pixel z-order regression (dropdown painted before a later combo) is the concrete risk this guards.

---

## 2. C14 + per-control contracts reproduced EXACTLY

### C-Checkbox / C-Trackbar / C-Combo(A) / C-Listbox(B) constants — carried verbatim
(`CHECKBOX_ICON_W/H=18`, `CHECKBOX_TEXT_LEFT_OFFSET=26`; `TRACKBAR_MOUSE_X_BIAS=6`, `TRACKBAR_MIN_CLAMP_X=1`, plaque 50, active-subtract 13, thumb 12; `COMBO_DROPDOWN_ROW_H=23`, `CHOOSE_MAP_LISTBOX_ROW_H=19`, `SCROLLBAR_W=20`, `SCROLLBAR_BUTTON_H=22`, `MIN_THUMB_H=14`, `COMBO_ARROW_RESERVE_W=20`; per-combo caps Side=7/Color,Start=9/AiType,Team=0.) Re-verify each by CONTENT before edit.

### C14 — `[MultiplayerDialogSettings]` seed (byte-exact, `ini/rulesmd.ini:3017-3042`)

The seed contract is measured on **widget surfacing**, NOT struct storage. The table below splits the two axes (Review 1 fix): a key can be Stored-in-`GameOptions` yet NOT surfaced as a 0x102 child widget. **"Surfaced as widget" for the >5-checkbox/3-trackbar set is UNCHECKED until the 4F Ghidra pre-req (O5) resolves it.**

| INI key | Value | Stored in GameOptions | Surfaced as 0x102 widget (verified) |
| --- | --- | --- | --- |
| MinMoney | 5000 | no | trackbar bound? **UNCHECKED (O4)** |
| Money | 10000 | yes (`starting_credits`) | YES — Credits trackbar |
| MaxMoney | 10000 | no | trackbar bound? **UNCHECKED (O4)** |
| MoneyIncrement | 100 | no | trackbar step? **UNCHECKED (O4)** |
| MinUnitCount | 0 | no | trackbar bound? **UNCHECKED (O4)** |
| UnitCount | 10 | yes (`unit_count`) | YES — UnitCount trackbar |
| MaxUnitCount | 10 | no | trackbar bound? **UNCHECKED (O4)** |
| TechLevel | 10 | yes (`tech_level`) | **UNCHECKED — widget? (O5)** |
| GameSpeed | 1 | yes (`game_speed`) | YES — GameSpeed trackbar (inverted) |
| AIDifficulty | 0 | yes | **UNCHECKED (O5)** |
| AIPlayers | 0 | yes | **UNCHECKED (O5)** |
| BridgeDestruction | yes | yes | **UNCHECKED (O5)** |
| ShadowGrow | no | no | **UNCHECKED (O5)** |
| Shroud | yes | yes | **UNCHECKED (O5)** |
| Bases | yes | yes | **UNCHECKED (O5)** |
| TiberiumGrows | yes | yes | **UNCHECKED (O5)** |
| Crates | yes | yes | YES — Crates checkbox |
| CaptureTheFlag | no | no | **UNCHECKED (O5)** |
| HarvesterTruce | no | yes | **UNCHECKED (O5)** |
| MultiEngineer | no | yes | **UNCHECKED (O5)** |
| AlliesAllowed | no | mode-derived | mode-derived (not a value seed) |
| ShortGame | yes | yes | YES — ShortGame checkbox |
| FogOfWar | no | yes | **UNCHECKED (O5)** |
| MCVRedeploys | yes | yes | YES — MCVRedeploys checkbox |
| AllyChangeAllowed | yes | yes | **UNCHECKED (O5)** |

Surfaced controls confirmed THIS session = **5 checkboxes** (ShortGame, MCVRedeploys, Crates, SuperWeapons, BuildOffAlly) + **3 trackbars** (GameSpeed, Credits, UnitCount). SuperWeapons/BuildOffAlly default `true` with no INI key. Whether the ~11 GameOptions-only keys SHOULD surface as widgets is the O5 Ghidra pre-req — surfaced explicitly as a DRIFT/UNCHECKED list, NOT folded into "modeled." **[O5 RESOLVED 2026-06-12: they do NOT surface as widgets — confirmed stored-only against the complete 72-child 0x102 RT_DIALOG inventory. The table's "UNCHECKED (O4)/(O5)" cells are superseded; O4 trackbar-bound keys are seeded (`bc3ae055`). Canonical mapping: [docs/research/skirmish-ui/SKIRMISH_0X102_MPDIALOGSETTINGS_KEY_TO_WIDGET_DRIFT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_0X102_MPDIALOGSETTINGS_KEY_TO_WIDGET_DRIFT_GHIDRA_REPORT.md).]**

### Differences that MUST stay parameterized after 4E (DRIFT to preserve)
Model A row 23 vs Model B row 19; A per-control max-visible cap vs B geometric `rect.h/19`; A thumb-drag supported vs B not; A wheel inert vs B wheel active; A cursor fused `Option<OpenComboDropdown>` vs B two bare `usize`; **and the empty-list thumb path: A `combo_dropdown_thumb_height(item_count==0) → track_h.max(MIN_THUMB_H)` vs B `choose_map_listbox_scroll_thumb_rect(row_count==0||visible_rows==0) → None`** (a SIXTH structural divergence the draft missed — added per Review 3).

---

## 3. Invariant — `state/tests.rs` (87) + `layout.rs` (30) GREEN and UNCHANGED; ONE scroll model after 4E

No assertion in any skirmish test module may be edited. **Count-stability is necessary but NOT sufficient** (an edited assertion keeps the count): each checkpoint additionally asserts `git diff HEAD -- src/ui/skirmish_shell/state/tests.rs src/ui/skirmish_shell/layout.rs` is EMPTY (Review 2 fix). After 4E there is exactly one scroll primitive whose six observable parameters reproduce both prior behaviors byte-for-byte (proven, not assumed). No packed-pixel/atlas changes.

---

## 4-PRE. Design gate (BLOCKING — resolve before 4A; was deferred, now required by Review 2)

Before any edit, write down (and get O1/O2/O3 sign-off):
1. **Dispatch shape:** enum/data per `ControlKind` for the PAINT seam (matching `ButtonPolicy`), NOT a Rust trait, NOT a `&mut SkirmishShellState` into `DialogController`. Input dispatch stays skirmish-layer per §1.1.
2. **State ownership:** skirmish state structs remain owners; controller is NOT extended to hold open-dropdown/drag/top_index/checked.
3. **Id mapping:** per-kind enums map onto `ControlDescriptor.id:u16` resource ids.
This gate exists because, IF the alternative (threading `&mut SkirmishShellState` through the generic controller) were chosen, 4A would be a controller-API redesign, not a call-site move. The chosen design keeps 4A small. **Do not begin 4A until this is signed off** — it determines whether `DialogController` changes at all (it should not).

---

## SUB-STEP BREAKDOWN (each: scope → files → acceptance → checkpoint+STOP rule)

### Sub-step 4A — Checkbox family (lands the paint dispatch seam)
**Scope:** Add `Checkbox` as an exercised `ControlKind` in the descriptor table. Land the render-side per-`ControlKind` paint seam (§1.2) on the simplest control. Route the five skirmish checkboxes through it for PAINT; keep input in `handle_option_mouse_down`'s checkbox branch (toggle on 18×18 `checkbox_icon_rect`, hover on FULL rect — two different rects, preserve both), `checkbox_value_mut` as the mutator, `GuiCheckboxSound` on icon-rect down only.
**Files (real flat paths):** `src/ui/shell/descriptor.rs`, `src/render/shell_paint.rs`, `src/render/mod.rs`, `src/ui/skirmish_shell/state/trackbars.rs`, `src/ui/skirmish_shell/state/hit_test.rs`, `src/app_skirmish_shell_render/controls.rs`.
**Acceptance:**
- `checkbox_icon_click_toggles_but_label_click_does_not` GREEN unchanged; icon right-edge boundary `x==icon.x+18` tie-break matches `contains()` (NEW boundary test).
- Hover/status-help keys off the FULL `checkbox.rect`; toggle off the 18×18 icon rect; `GuiCheckboxSound` exactly once on icon-rect down; label-rect down inert.
- Paint draw-order/depth for checkboxes byte-identical to the pre-seam `push_checkbox_instances` sequence (draw-list assertion, §1.4).
- **Pixel/draw-list diff == 0** vs HEAD for the checkbox region in checked + unchecked states (via the §6.4 mechanism, NOT a source text diff).
**Checkpoint + STOP:** `cargo build -p vera20k` && `cargo test -p vera20k` (separate bounded pass); read the literal `test result:` line; confirm `state/tests.rs`=87, `layout.rs`=30, `app_skirmish_shell_render.rs`=53 unchanged AND `git diff HEAD -- state/tests.rs layout.rs` empty. If ANY check fails, hard-revert THIS commit and STOP before 4B.

### Sub-step 4B — Trackbar family
**Scope:** Add exercised `Trackbar` kind + paint seam arm (rail+plaque+trakgrip). Keep input in the trackbar branch: Y-gate (bottom 18px), thumb-hit→value-tracking drag (`dragging_thumb=true`, follows cursor via `handle_option_mouse_move`), rail-click jumps ONCE and does NOT follow (early-return on `!dragging_thumb`), x-clamp (`BIAS=6`/`MIN_CLAMP_X=1`/active `w-50-13`). Keep the `game_speed` stored↔visual inversion ENTIRELY in the skirmish set/get mutator (substrate/paint sees only visual values — no double-invert). HSCROLL + `GenericClick` on change only. Bounds stay the current hardcoded consts (do NOT seed from MinMoney/MaxMoney yet — O4).
**Files:** `src/ui/shell/descriptor.rs`, `src/render/shell_paint.rs`, `src/ui/skirmish_shell/state/trackbars.rs`, `src/ui/skirmish_shell/layout.rs`, `src/app_skirmish_shell_render/controls.rs`.
**Acceptance:**
- `trackbar_mouse_y_gate_rejects_top_four_pixels`, `trackbar_thumb_hit_uses_exclusive_twelve_pixel_interval`, `trackbar_mouse_x_clamps_below_and_above_range`, `trackbar_outside_thumb_click_remaps_value_and_keeps_capture`, `trackbar_thumb_hit_starts_drag`, `trackbar_rail_click_jumps_once_and_does_not_track_cursor` GREEN unchanged.
- Y-gate lower/upper edges (`rect.y+rect.h-18`, `rect.y+rect.h`) tie-break matches legacy (NEW boundary test); `game_speed` inversion intact (no double-invert through the paint path); HSCROLL+`GenericClick` fire on change only.
- Pixel/draw-list diff == 0 across min/mid/max thumb positions.
**Checkpoint + STOP:** as 4A.

### Sub-step 4C — Combo family (introduces scroll Model A onto the substrate)
**Scope:** Add exercised `Combo`+`ScrollBar` kinds + paint seam arms (collapsed faces; open popup drawn LAST so it overlays — z-order per §1.4). Keep the WHOLE `state/combos.rs` captured-popup state machine in place (`handle_combo_mouse_down`): arrow-zone face click opens + `GuiComboOpenSound`; up/down arrows ±1; thumb-drag via `DropdownScrollDragState`+`top_index_from_thumb_y`; track-click via `top_index_from_scrollbar_track_click`; content row = `top_index + (y-content.y)/23` + select + close + `GuiComboCloseSound`; chrome click consumes WITHOUT closing; off-click closes; two-clicks-to-switch; `combo_hit_order` reverse-row (lower rows win); per-combo caps 7/9/unbounded; **wheel INERT (`handle_option_mouse_wheel` returns false — test-pin the inertness post-move).**
**Gesture-model reconciliation (Review 3 gap):** the open dropdown is a single-click-down captured popup with multiple overlapping sub-rects and a REVERSE hit order — the opposite of the controller's forward first-match press-must-match-release. **Design: the open dropdown keeps its OWN capture path (consumes mouse-DOWN directly via `handle_combo_mouse_down`), bypassing the controller's press/release model entirely. Only the collapsed combo FACE may later route through a descriptor hit-test.** Document reverse-hit-order, two-clicks-to-switch, and chrome-consume as state-machine invariants with tests.
**Files:** `src/ui/shell/descriptor.rs`, `src/render/shell_paint.rs`, `src/ui/skirmish_shell/state/combos.rs`, `src/ui/skirmish_shell/layout.rs`, `src/app_skirmish_shell_render/controls.rs`, `src/app_skirmish_shell_render.rs`.
**Acceptance:**
- `dropdown_wheel_is_inert_and_content_click_uses_top_index`, `dropdown_scrollbar_arrows_step_and_drag_clamp_top_index`, `skirmish_side_dropdown_scrollbar_track_click_jumps_to_native_top_index` GREEN unchanged.
- Open/select/chrome/off-click sounds + semantics preserved; per-combo caps + reverse order preserved; wheel inertness test-pinned; arrow-zone left edge `face.x+face.w-20` tie-break matches legacy (NEW boundary test).
- Open-dropdown z-order: instances emit AFTER sibling combo faces (draw-list assertion).
- Pixel/draw-list diff == 0 for collapsed combos and open dropdown (swatches, highlight, scrollbar thumb) across scroll positions.
**Checkpoint + STOP:** as 4A (also confirm `app_skirmish_shell_render.rs` test count, currently 53, unchanged).

### Sub-step 4D — Listbox / choose-map family (introduces scroll Model B + migrates app.rs input)
**Scope:** Add exercised `Listbox` kind + paint seam (paint stays in `modals.rs`). Two listboxes (`Mode0x6eb`, `Map0x553`), row 19, `visible_rows=rect.h/19`; arrows ±1 via `scroll_listbox_by_rows`→`set_top_index_clamped` (clamps to `row_count.saturating_sub(visible_rows.max(1))`); track-click via `choose_map_listbox_top_index_from_track_click`; thumb-hit returns true but NO drag-follow (preserve the divergence from combo). Mode row→`select_mode` (rebuilds filter, resets `map_top_index`); map row→`select_map_filtered_row`.
**WHEEL — migrate ALL FOUR behaviors verbatim (Review 3 gap):** (a) `lines==0.0`→consume, no scroll; (b) `lines>0`→`-(ceil(|lines|).max(1))`; (c) `lines<0`→`+(ceil(|lines|).max(1))`; (d) cursor over `map_list` (checked FIRST) scrolls map, else over `mode_list` scrolls mode, else consume-without-scroll. Add a NEW test pinning EACH of the four branches.
**Input migration:** input currently lives in `app.rs` (`handle_choose_map_modal_mouse_down:979`, `handle_choose_map_listbox_scrollbar_mouse_down:1080`, `handle_choose_map_modal_mouse_wheel:1130`); move the LISTBOX SCROLL + wheel input into the `ui/skirmish_shell/` layer, keeping `ChooseMapModalState` the owner. **`app.rs` DELEGATES, never duplicates** (no double-handling). **Modal boundary (Review 1 fix):** the choose-map modal's OK/Cancel/Random buttons (`ChooseMapModalButton`, `app.rs:979-1075`) and its mouse-up commit **stay app-side this slice** — only the listbox scroll/wheel migrates; the modal chrome/buttons and modal stacking (C1/C5) are DEFERRED (state this in §5.7).
**Files:** `src/ui/shell/descriptor.rs`, `src/render/shell_paint.rs`, `src/ui/skirmish_shell/state/choose_map.rs`, `src/ui/skirmish_shell/layout.rs`, `src/app.rs`, `src/app_skirmish_shell_render/modals.rs`.
**Acceptance:**
- `choose_map_modal_scrolls_mode_and_map_listboxes_independently`, `choose_map_modal_listbox_hit_testing_reserves_scrollbar_width`, `choose_map_modal_scrollbar_thumb_and_track_map_to_top_index` GREEN unchanged.
- All four wheel branches test-pinned (incl. zero-line consume, outside-lists consume, mode-list path, negative branch); listbox thumb NOT draggable; mode-select resets `map_top_index` + rebuilds filter; last-partial-row click tie-break (clipped via `.min`) matches legacy (NEW boundary test).
- No double-handling: `app.rs` delegates; assert one handler per click.
- Pixel/draw-list diff == 0 for the modal (both listboxes, scrollbar, highlight) across scroll/selection states.
**Checkpoint + STOP:** as 4A. **Extra parallel-safety:** `app.rs` is a contended file (see §7) — re-verify the dispatch block by CONTENT immediately before edit; if another session's edits are present, WAIT.

### Sub-step 4E — Unify the two scroll models into ONE (only after 4C AND 4D landed-green)
**Scope:** Collapse Model A and Model B into a single parameterized primitive taking `visible_rows` AS A PARAMETER. Shared core: `BUTTON_H=22`, `MIN_THUMB_H=14`, `max_top = row_count - visible_rows`, `thumb_h = (track_h*visible_rows/item_count).clamp(MIN_THUMB_H, track_h)`, track-click `mouse_y - thumb.h/2`. **The SIX observable divergences MUST stay PARAMETERS, not collapse:** (a) row_h 23 vs 19; (b) visible-row source `PerControlCap(i32)` vs `GeometricFromRect`; (c) thumb-drag enabled (combo yes / listbox no); (d) wheel active (combo no / listbox yes); (e) cursor storage (fused `Option` vs separate fields — behind a `top_index` getter/setter); (f) empty-list path (combo `track_h.max(MIN_THUMB_H)` vs listbox `None`).
**Parameter type (Review 1 fix — pinned):** a struct `{ row_h: i32, visible_row_source: enum { PerControlCap(i32) | GeometricFromRect }, thumb_drag_enabled: bool, wheel_active: bool, empty_path: enum { MaxThumb | NoThumb }, top_index_accessor }`. Combo passes `PerControlCap + drag + wheel-inert + MaxThumb`; listbox passes `Geometric + no-drag + wheel-active + NoThumb`.
**Equivalence PROOF step (BEFORE any DELETE — Review 2+3 fix):**
1. Enumerate the reachable input domain under the `needs_scrollbar` gate; PROVE which of `row_count==0`/`visible_rows==0` are actually reachable when a scrollbar exists (so the A `MaxThumb` vs B `NoThumb` empty-paths never diverge in practice — or, if reachable, prove they produce identical observable output).
2. Keep BOTH legacy functions compiled; assert the unified primitive == both legacy impls bit-for-bit over `row_count ∈ 0..=N`, `visible_rows ∈ 0..=N` supplied by EACH model's own source, `top_index ∈ 0..=max_top`, `mouse_y ∈ [scrollbar.y .. scrollbar.y+h]`, at boundaries: empty / single-row / exactly-visible / overflow-by-one / max-top / `track_span.max(1)` clamp.
3. Confirm `max_top` via `saturating_sub` matches BOTH callers.
4. DELETE the duplicated thumb-height/track-click copies (`combos.rs:142-257`, `layout.rs:648-693`) ONLY after the boundary tests are green.
**Files:** `src/ui/skirmish_shell/state/combos.rs`, `src/ui/skirmish_shell/state/choose_map.rs`, `src/ui/skirmish_shell/layout.rs`.
**Acceptance:** ALL scroll tests from 4C+4D GREEN unchanged against the unified primitive; the boundary-equivalence test set green for BOTH visible-row sources; thumb-drag still combo-only, wheel still listbox-only, empty-path preserved per model; pixel/draw-list diff == 0 for both scrollbars across all scroll positions.
**Checkpoint + STOP:** as 4A. Hard-gated behind 4C AND 4D both green.

### Sub-step 4F — C14 defaults seed (byte-exact for MODELED keys only)

> **STATUS UPDATE (2026-06-12) — 4F is effectively CLOSED; this section is largely STALE.** Two commits
> that postdate this plan already shipped the seed work: `1f54995f` added the runtime
> `GameOptions::from_multiplayer_dialog_settings` parse + the `launch_options_base` carry chain, and
> `bc3ae055` resolved **O4** (trackbar bounds ARE now seeded from MinMoney/MaxMoney/MoneyIncrement/
> MinUnitCount/MaxUnitCount via `SkirmishTrackbarBounds::from_multiplayer_dialog_settings`). **O5 is RESOLVED:
> gamemd's 0x102 surfaces exactly 8 option widgets (5 checkboxes + 3 trackbars), all present in the Rust
> shell — the missing-widget DRIFT list is EMPTY.** The ~11 GameOptions-only keys + ShadowGrow/CaptureTheFlag
> are confirmed stored-only/unmodeled, not missing widgets. Evidence + full 25-key DRIFT table:
> [docs/research/skirmish-ui/SKIRMISH_0X102_MPDIALOGSETTINGS_KEY_TO_WIDGET_DRIFT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_0X102_MPDIALOGSETTINGS_KEY_TO_WIDGET_DRIFT_GHIDRA_REPORT.md). The "stays
> HARDCODED / DRIFT-UNCHECKED" framing below is superseded; no further 4F implementation is needed.
**Ghidra pre-req (read-only, BEFORE any edit — Review 1 fix):** trace the 0x102 dialog-init read path and enumerate EXACTLY which `[MultiplayerDialogSettings]` keys map to an actual 0x102 child widget vs which are stored-only (consumed at launch). Resolve whether the ~11 GameOptions-only keys (TechLevel/Shroud/Bases/TiberiumGrows/MultiEngineer/HarvesterTruce/BridgeDestruction/FogOfWar/AllyChangeAllowed/AIDifficulty/AIPlayers) and ShadowGrow/CaptureTheFlag are surfaced widgets the Rust shell is MISSING, or are correctly stored-only. Output an explicit DRIFT list (missing widgets), NOT a folded "modeled."
**Scope:** Re-confirm the 17 modeled keys byte-exact across the FULL seed chain `GameOptions::default → SkirmishLaunchOptions::default → SkirmishShellState::default` (not just `game_options.rs`); enumerate WHICH surfaced control seeds from WHICH key. Trackbar bound seeding from MinMoney/MaxMoney/MoneyIncrement/MinUnitCount/MaxUnitCount stays HARDCODED and explicitly marked **DRIFT/UNCHECKED** (O4) — do NOT seed without Ghidra proof of the runtime read path. ShadowGrow/CaptureTheFlag stay **DRIFT/UNCHECKED** (O5), NOT "inert seeds." Do NOT change any `GameOptions` field value without explicit INI evidence.
**Files:** `src/sim/game_options.rs`, `src/ui/skirmish_shell/state/trackbars.rs`, `src/skirmish_launch.rs`.
**Acceptance (claim NARROWED — Review 3 fix):**
- "**The 17 modeled keys seed byte-exact across all three chain hops**" — NOT "every surfaced control."
- NEW seed test per surfaced control: default == the INI literal, asserted at the END of the chain.
- The 5 trackbar bounds + ShadowGrow/CaptureTheFlag remain explicitly DRIFT/UNCHECKED in the shipped slice (documented, not silently equated).
- Existing GameOptions + `skirmish_launch.rs` (7) tests GREEN unchanged.
**Checkpoint + STOP:** as 4A.

---

## 5. Retire/replace edits + do-not-touch

### 5.1–5.6 (per sub-step above) — KEEP all current skirmish math local; REPLACE only the call SITE / paint emission site; DELETE duplicated scroll copies only in 4E after the proof.

### 5.7 Do NOT touch / DEFERRED-out-of-scope (boundary made explicit)
- The flat `src/skirmish_*.rs` launch/data layer (except `game_options.rs`/`skirmish_launch.rs` seed in 4F).
- The migrated button shells (`app_main_menu_shell_render.rs`, `app_single_player_shell_render.rs`) and `paint_buttons`' button arm.
- Any `*tests.rs` assertion (87 + 30 + others).
- **0x102 keyboard routing** (player-name Tab/Esc) and **Start(0x617)/Back(0x5C0)/ChooseMap result-code routing** through `DialogController` — DEFERRED to a later slice; NOT in Slice 4.
- **Choose-map modal OK/Cancel/Random buttons + mouse-up commit + modal stacking (C1/C5)** — stay app-side; only listbox scroll/wheel migrates in 4D.
- `app.rs` is touched ONLY for the 4D listbox-scroll/wheel migration; it is a CONTENDED file (see §7).

---

## 6. Acceptance (consolidated)

### 6.1 Per-interaction identical — checkbox/trackbar/combo/listbox behavior bullets per sub-step above; defaults seed = 17 modeled keys byte-exact across the chain.

### 6.2 Per-sub-step BLOCKING checkpoint (replaces "one bounded pass after all edits")
After EACH of 4A–4F: run `cargo build -p vera20k` && `cargo test -p vera20k` (separate bounded pass, per `feedback_cargo_separate_verify_pass`, run PER COMMIT not once at the end); read the literal `test result:` line; confirm (a) `state/tests.rs`=87, `layout.rs`=30, `app_skirmish_shell_render.rs`=53 unchanged, AND (b) `git diff HEAD -- src/ui/skirmish_shell/state/tests.rs src/ui/skirmish_shell/layout.rs` is EMPTY. **If ANY check fails: hard-revert THAT sub-step's commit and STOP — do not proceed.** This keeps the six commits genuine isolated checkpoints (a 4A regression cannot hide until after 4F).

### 6.3 NEW tests to add (never edit existing)
4A icon-edge; 4B Y-gate edges; 4C arrow-zone edge + wheel-inert pin; 4D four wheel branches + last-partial-row + no-double-handle; 4E boundary equivalence (both visible-row sources, all boundaries); 4F per-control seed == INI; §1.4 draw-list/z-order assertions per sub-step.

### 6.4 Pixel-diff mechanism (Review 3 gap — the bar must be EXECUTABLE)
"`git show HEAD:`" on source is a TEXT diff, not a pixel comparison, and no render-snapshot harness exists. **Resolve before claiming any pixel-diff acceptance (O6):** either (a) build a minimal render-snapshot/golden harness for the migrated control regions, OR (b) DROP the "pixel diff == 0" line and substitute concrete executable gates: (1) 87+30 state tests green-unchanged, (2) **draw-list assertions** comparing `SpriteInstance` count/uv/depth/position per control state against the pre-migration emitter output captured as fixtures (this is the real pixel-equivalence proxy and is buildable now), (3) a manual in-app A/B screenshot checklist per control if no harness is built. Do NOT ship a pixel-diff acceptance criterion with no backing mechanism.

---

## 7. Parallel-session safety
- Land 4A–4F as separate commits to `dev`; minimize each write window.
- **CONTENDED files (git status + this session):** `app_skirmish_shell_render.rs`, `app_single_player_shell_render.rs`, `app_main_menu_shell_render.rs` (all dirty from another session), `render/shell_paint.rs` (UNTRACKED — Slice 3 not yet committed), and **`app.rs`** (4D rewires `app.rs:979/1080/1130` mouse dispatch). Before editing any of these, re-verify by CONTENT; if another session's in-progress edits are present, WAIT — do not fix/revert/stash. **Confirm `render/shell_paint.rs` is committed/owned before 4A edits it.**
- If `cargo` fails in files you did NOT touch, assume another session's WIP — continue or wait.
- Each sub-step is contained-rollback (revert one commit) and gated by the §6.2 STOP rule.

---

## 8. Open questions for the human (decide before implementation)
O1 Dispatch shape — default: render-side enum/`match ControlKind` for paint (like `ButtonPolicy`), NOT a Rust trait, NOT `&mut SkirmishShellState` into the generic controller. Confirm at 4-PRE.
O2 Runtime state ownership — default: skirmish state structs own; controller is NOT extended. Confirm at 4-PRE.
O3 Id mapping — default: per-kind enums → `ControlDescriptor.id:u16` resource ids.
O4 **RESOLVED (commit `bc3ae055`)** — trackbar bounds ARE seeded from MinMoney/MaxMoney/MoneyIncrement/MinUnitCount/MaxUnitCount via `SkirmishTrackbarBounds::from_multiplayer_dialog_settings` (wired `app.rs:2406`). The "stay hardcoded" guidance is superseded.
O5 **RESOLVED (2026-06-12)** — gamemd's 0x102 surfaces exactly 8 option widgets (5 checkboxes + 3 trackbars), ALL present in the Rust shell; the ~11 GameOptions-only keys + ShadowGrow/CaptureTheFlag are stored-only/unmodeled, NOT missing widgets. Missing-widget DRIFT list is EMPTY. Evidence: [docs/research/skirmish-ui/SKIRMISH_0X102_MPDIALOGSETTINGS_KEY_TO_WIDGET_DRIFT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_0X102_MPDIALOGSETTINGS_KEY_TO_WIDGET_DRIFT_GHIDRA_REPORT.md).
O6 Pixel-diff mechanism — build a snapshot harness, or substitute draw-list-assertion + manual A/B per §6.4? Pick before any "pixel diff == 0" acceptance is claimed.
O7 Layer target — confirm Slice 4 folds the `ui/skirmish_shell/` control state machine, NOT the flat launch/data files.
O8 Sub-step granularity — confirm the 4-PRE + six cycles (4A→4F), or request finer/coarser.

### Plan correctness note (load-bearing)
The two highest-risk items: (1) **4E must not change observable scroll behavior** — SIX divergence axes (now including the empty-list `track_h.max(MIN_THUMB_H)` vs `None` path), default verdict DRIFT, each preserved as a parameter and PROVEN with boundary tests over BOTH visible-row sources before either copy is deleted; (2) **the dispatch seam is paint-only / skirmish-input-stays-local** — the generic `DialogController` (button-only, press-must-match-release, single-rect, forward first-match) is NOT redesigned to carry combo's reverse-hit-order captured-popup single-click model; the open dropdown keeps its own capture path. Get these two right and the rest is call-site re-expression with the 87+30 suite as the net.
