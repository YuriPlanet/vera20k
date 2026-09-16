---
title: Shell Substrate (ui::shell) — Design Spec
date: 2026-05-31
status: design (live-src-verified this session; Slice 0 only is approved-for-implementation scope)
source doc: [docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md)
scope: consolidate the three front-end shells' duplicated geometry into one ui/shell/geom.rs;
       full substrate (descriptor/layout/paint-trait/controller/modal/slide) is roadmap-only.
verification: every current-code claim below is quoted file:line from a live read this run.
              Ghidra read-only; cargo NOT run (separate build pass).
---

# Shell Substrate — Design Spec

## 0. TL;DR

The study doc's headline finding — **3-way triplication of RectPx / DLU helpers / right-panel
geometry / owner-draw-button snap across the three shells** — is **CONFIRMED against live src**,
with two important corrections to the doc's line numbers and one **critical subtle difference**
the doc undersells: the three copies are **NOT byte-identical**, and a naive "shared" extraction
*would* move pixels. The differences are:

1. **`RectPx` itself differs**: skirmish's has an extra `const fn translate` (layout.rs:55-62)
   that main_menu's lacks; single_player imports main_menu's (no translate). Same fields, same
   `new`/`contains`.
2. **Two different button-snap algorithms**: main_menu uses **round-half-up to nearest 42-px row**
   (`sdbtnanm_button_rect`, layout.rs:289-302); single_player and skirmish use **`+tile_h/2`
   truncating tile-index** (`owner_draw_button_snap_rect`, SP layout.rs:169-185 / skirmish
   layout.rs:500-516). These can disagree by one row at certain inputs — they are not the
   same function.
3. **`SDBTNANM` cell width differs**: main_menu `SDBTNANM_CELL_W = 156` (layout.rs:97),
   skirmish `SDBTNANM_W = 156` (layout.rs:6), but **single_player `SDBTNANM_W = 168`**
   (layout.rs:15). SP buttons are 168 wide flush at x=632; the other two are 156 wide flush
   at x=644. A single shared snap-rect with one hardcoded width silently shifts SP pixels.

**Slice 0** (the only implement-now item) is therefore a *parameterized* geom module — the
shared functions take width/algorithm as inputs so each caller keeps its exact current output —
not a single canonical formula. Everything past Slice 0 is planned-not-applied roadmap (§5).

---

## 1. Triplication — confirmed/corrected with file:line, and the byte-diff verdict

Live read this session. Verdict column: **IDENTICAL** = byte-for-byte same logic across copies;
**PARAM-DIFF** = same shape but a differing constant/branch that changes output and MUST be
preserved per-caller; **ALGO-DIFF** = genuinely different algorithm.

| Item | main_menu_shell/layout.rs | single_player_shell/layout.rs | skirmish_shell/layout.rs | Verdict |
|---|---|---|---|---|
| `RectPx` struct + `new` + `contains` | 14-30 | imports main_menu's (mod line 4) | 42-67 | **IDENTICAL** (fields/new/contains) |
| `RectPx::translate` | absent | absent (uses main_menu's) | **present** 55-62 | **PARAM-DIFF** (skirmish-only method) |
| `mul_div_round` | 107-114 | 50-57 | 229-236 | **IDENTICAL** (verified char-for-char) |
| `dlu_rect` | 116-123 | 59-66 | 238-245 | **IDENTICAL** |
| `center_offset` | inline (no named fn) | 68-74 | 427-429 | **IDENTICAL math** (`(s-b)/2` clamped ≥0) |
| `right_panel_rects` | 147-181 | 90-124 | 447-476 | **IDENTICAL output**; bottom_h `.max(0)` placement differs (end vs construct) but algebraically equal |
| `lower_strip_rect` | 183-208 | 126-149 | **absent** (no lower strip in 0x102) | **IDENTICAL** (the two that have it) |
| owner-draw button snap | `sdbtnanm_button_rect` 289-302 | `owner_draw_button_snap_rect` 169-185 | `owner_draw_button_snap_rect` 500-516 | **ALGO-DIFF** main_menu vs other two; **PARAM-DIFF** (width 156 vs 168) SP vs skirmish |
| `back_rect` | `exit_button_rect` 308-312 (raw DLU, NOT last-tile) | 187-195 (last-tile) | 490-498 (last-tile) | **ALGO-DIFF** main_menu vs other two; SP/skirmish identical bar width const |
| `right_anchor` | `right_anchor_rect` 259-272 (top.y anchored) | 151-161 (offset_y anchored) | 435-445 (offset_y anchored) | **PARAM-DIFF** main_menu uses `right_panel.top.y`, others use `center_offset(h)` |

### Const-table verdict (the doc's "RIGHT_PANEL_WIDTH/TILE_H/TILE_COUNT" claim)

| const | main_menu | single_player | skirmish | note |
|---|---|---|---|---|
| `RIGHT_PANEL_WIDTH` | 86 = 168 | 10 = 168 | 5 = 168 | identical |
| panel top H | `RIGHT_PANEL_TOP_H` 87 = 199 | 11 = 199 | inline `199` (line 459) | identical value, skirmish unnamed |
| tile H | `RIGHT_PANEL_TILE_H` 88 = 42 | 12 = 42 | `SDBTNBKGD_H` 8 = 42 | identical value, **different name** |
| tile-count cap | `RIGHT_PANEL_TILE_COUNT_BASE` 89 = 9 | 13 = 9 | inline `.min(9)` (line 467) | identical value, skirmish unnamed |
| SDBTNANM cell W | `SDBTNANM_CELL_W` 97 = **156** | `SDBTNANM_W` 15 = **168** | `SDBTNANM_W` 6 = **156** | **DIVERGENT — load-bearing** |
| SDBTNANM cell H | `SDBTNANM_CELL_H` 98 = 42 | `SDBTNANM_H` 16 = 42 | `SDBTNANM_H` 7 = 42 | identical |
| `LOWER_STRIP_H` | 91 = 32 | 14 = 32 | absent | identical (the two with it) |

**So:** the doc is right that the *value set* is duplicated, but the names diverge (skirmish
inlines several, renames `RIGHT_PANEL_TILE_H`→`SDBTNBKGD_H`) and **one value genuinely
disagrees (SDBTNANM width 156 vs 168)**. The shared module must expose the width as a parameter,
not a single constant.

### The two snap algorithms (must NOT be merged into one formula)

main_menu `sdbtnanm_button_rect` (layout.rs:289-302) — round-half-up to nearest row:
```
delta = (dlu_top - panel_y).max(0); q = delta / row_h; rem = delta % row_h;
q = if row_h - rem <= rem { q + 1 } else { q };  y = q*row_h + panel_y
```
single_player / skirmish `owner_draw_button_snap_rect` (SP 169-185 / sk 500-516) — biased-truncate:
```
source_y = source.y + center_offset(h); tile_h = panel.tile.h.max(1);
tile_index = ((source_y - panel.tile.y + tile_h/2) / tile_h).max(0);  y = panel.tile.y + tile_index*tile_h
```
For non-negative `delta` and `tile_h=42` these usually coincide, but the main_menu tie-break
(`row_h - rem <= rem`, i.e. rounds the exact-midpoint *down*-distance) and the other's
(`+tile_h/2` then floor, i.e. rounds midpoint *up*) differ at the half-row boundary and main_menu
adds `dlu_top` from `right_panel.top.y` while the other adds `center_offset(h)` to `source.y`.
**Default-to-DRIFT applies: do not assume equivalence.** Slice 0 keeps both as distinct functions
(or one function taking a `SnapRounding` enum) and the golden test asserts both reproduce every
existing per-shell test rect exactly.

### Hit/press-match triplication (doc §7 item 4) — CONFIRMED

- main_menu `hit_test_owner_draw_button` state.rs:90-100 + `mouse_up` press-match 114-129.
- single_player `hit_test_owner_draw_button` state.rs:60-70 + `mouse_up` press-match 98-116
  (adds a `LoadSavedGame` disabled-guard, lines 108-110 — a PARAM-DIFF, not pure dup).
- skirmish `hit_test_owner_draw_button` state/hit_test.rs:292-307 (manual if-chain over 3
  named buttons, not an array `.iter().find`) + press-match in **app.rs:1426-1437** (the doc's
  cited cluster — CONFIRMED at those exact lines this run).

The press-match *logic* ("pressed.is_some() && pressed == released → fire, else None") is
identical across all three; the hit-test *shape* differs (array-find vs if-chain) and SP carries
a disabled-button guard. This is Slice 2 territory (planned-not-applied), not Slice 0.

### Render-emitter & atlas-pack triplication (doc §7 items 5-6)

CONFIRMED the files exist: `src/app_main_menu_shell_render.rs`, `src/app_single_player_shell_render.rs`,
`src/app_skirmish_shell_render.rs` (+ dir), and chrome builders `render/main_menu_shell_chrome.rs`,
`render/skirmish_shell_chrome.rs`, `render/loading_screen_chrome.rs`, plus the shared seams
`render/shell_text.rs` and `render/shell_transition_pass.rs`. Line-range spot-audit of the emitter
internals was out of Slice-0 scope and is marked **UNCHECKED** here — Slice 3/6 must re-verify the
doc's `:47-292` / `:36-300` ranges before touching them (they are render-coupled and a parallel
session may be mid-edit).

---

## 2. Doc-vs-code drift found (corrections to [SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) §4/§7)

1. **§4 table & §7.1 say single_player DLU copy is `layout.rs:50-74`** — CONFIRMED exact
   (mul_div_round 50-57, dlu_rect 59-66, center_offset 68-74).
2. **§7.1 says skirmish copy is `layout.rs:42-67,229-245,427-429`** — CONFIRMED (RectPx 42-67,
   mul_div_round/dlu_rect 229-245, center_offset 427-429). Good.
3. **DRIFT: §4/§7 treat the three snap-rect copies as one item to "fold."** They are **two
   different algorithms** (main_menu round-half-up vs other-two biased-truncate) **and** carry a
   **divergent width const (156 vs 168)**. The doc's "delete and share one" framing would change
   single_player's button pixels. Corrected scope: parameterize, don't unify-to-one.
4. **DRIFT: §7.2 cites `chrome.rs:585-612` render-recompute and §7.5 cites
   `app_main_menu_shell_render.rs:47-292` / `app_single_player_shell_render.rs:36-300`.** These
   render-side ranges were NOT line-verified this run (Slice 0 is ui-only). Treat as UNCHECKED;
   re-verify at Slice 3/6.
5. **Confirmed-accurate:** §4's "the clean seam already exists, ui/mod.rs:10-12 layering honored"
   — verified: `ui/mod.rs:10-12` states ui depends on sim/ only, NOT assets/render/sidebar/audio/net;
   the three layout/state modules import nothing from those layers. The shared geom module fits
   under this rule unchanged.
6. **Note (not drift):** §4 says first-paint slide is already shared in `app_shell_transition.rs` —
   that file exists; its sharing was not re-audited this run (render-side, out of Slice-0 scope).

---

## 3. Target module — `src/ui/shell/` (Slice 0: geom only)

New submodule under `ui/`, added to `ui/mod.rs` as `pub mod shell;`. **Honors the ui/-no-render
rule**: depends only on plain integers (no sim/ needed for geom; the wider substrate in §5 may read
sim/rules, still never render/assets). The three shells re-export from here instead of defining
their own copies.

### 3.1 `ui/shell/geom.rs` — public API (the ONE copy)

```rust
//! Shared front-end shell geometry: DLU→pixel, right-panel chrome, button snap.
//! Render-agnostic; depends on nothing above sim/. The three shells (0xE2/0x100/0x102)
//! import these instead of each keeping a private copy.

/// Pixel rect in window space. Identical fields/semantics to the three prior copies;
/// `translate` is hoisted from skirmish so all shells share it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectPx { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }
impl RectPx {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self;
    pub const fn translate(self, dx: i32, dy: i32) -> Self;   // from skirmish layout.rs:55-62
    pub fn contains(self, x: i32, y: i32) -> bool;            // identical to all three
}

// --- DLU base metrics (MS Sans Serif 8pt; C7) ---
pub const DLU_BASE_X: i32 = 6;   // was BASE_X in all three
pub const DLU_BASE_Y: i32 = 13;  // was BASE_Y in all three

/// Round-half-up MulDiv (sign-correct). Byte-identical to the three private copies.
pub fn mul_div_round(n: i32, numer: i32, denom: i32) -> i32;

/// DLU rect → pixel rect using DLU_BASE_X/Y. Byte-identical to the three private copies.
pub fn dlu_rect(x: i32, y: i32, w: i32, h: i32) -> RectPx;

/// `(screen - base)/2` clamped to ≥0. Folds main_menu's inline form + SP/skirmish center_offset.
pub fn center_offset(screen: i32, base: i32) -> i32;

// --- Right-panel chrome (SDTP top / SDBTNBKGD tile / SDBTM bottom) ---
pub const RIGHT_PANEL_WIDTH: i32 = 168;
pub const RIGHT_PANEL_TOP_H: i32 = 199;
pub const RIGHT_PANEL_TILE_H: i32 = 42;   // == skirmish SDBTNBKGD_H
pub const RIGHT_PANEL_TILE_COUNT_CAP: i32 = 9;
pub const SDBTNANM_CELL_H: i32 = 42;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RightPanelRects { pub top: RectPx, pub tile: RectPx, pub tile_count: i32, pub bottom: RectPx }

/// Right-panel layout. Reproduces all three copies' output exactly (incl. the
/// `bottom_h.max(0)` clamp). Same for 0xE2/0x100/0x102.
pub fn right_panel_rects(screen_w: i32, screen_h: i32) -> RightPanelRects;

/// Lower strip (LWSCRN cap). Used by 0xE2 and 0x100 only; 0x102 has none.
pub const LOWER_STRIP_H: i32 = 32;
pub fn lower_strip_rect(screen_w: i32, screen_h: i32) -> RectPx;

// --- Button snap: TWO algorithms preserved, selected by enum (NOT merged) ---
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapRounding {
    /// main_menu `sdbtnanm_button_rect`: round-half-up to nearest 42-px row,
    /// anchored at panel.top.y + DLU(dlu_y). (layout.rs:289-302)
    RoundHalfUpFromPanelTop,
    /// single_player/skirmish `owner_draw_button_snap_rect`: `+tile_h/2` truncate
    /// tile-index from panel.tile.y, source.y + center_offset(h). (SP 169-185 / sk 500-516)
    BiasedTruncateFromTile,
}

/// Snap an owner-draw button to its SDBTNANM cell. `cell_w` is per-shell
/// (156 for 0xE2/0x102, 168 for 0x100 — do NOT hardcode). `source` is the DLU-derived rect
/// for BiasedTruncate, or carries `dlu_y` for RoundHalfUp via the caller.
pub fn snap_button_rect(
    screen_w: i32, screen_h: i32,
    source: RectPx, panel: RightPanelRects,
    cell_w: i32, rounding: SnapRounding,
) -> RectPx;

/// Last-tile "Back/Exit"-row rect for SP/skirmish (panel.tile_count-1 row).
/// main_menu's Exit is a DIFFERENT rule (raw DLU top) — keep that one in the shell, not here.
pub fn last_tile_button_rect(screen_w: i32, panel: RightPanelRects, cell_w: i32) -> RectPx;
```

**Per-caller width constants stay in each shell** (or as named consts here:
`SDBTNANM_CELL_W_NARROW = 156`, `SDBTNANM_CELL_W_WIDE = 168`) and are passed to
`snap_button_rect`. This is the guardrail against item 3 above.

### 3.2 Shell-specific pieces that DO NOT move in Slice 0

Stay private to each shell because they encode dialog-specific rules:
- main_menu `right_anchor_rect` (anchors to `right_panel.top.y`), `exit_button_rect`,
  `title_rect` (+7/+1 nudge), version/tooltip/website rects.
- single_player `right_anchor`/`back_rect`/`status_help_rect` and the disabled-LoadSavedGame guard.
- skirmish `right_anchor`/`back_rect`/all the combo/trackbar/checkbox/listbox helpers +
  `translate_layout`/`compute_fixed_800_layout` + the two scroll models + validation modal.

These migrate (or not) in later slices; Slice 0 only lifts the three shared primitives.

---

## 4. Slice 0 — the only implement-now scope

**Goal:** zero behavior change. Create `ui/shell/geom.rs`, re-point the three shells to it,
delete the three private copies of the shared primitives, keep every dialog-specific helper local.

Concrete edits (ui-only; no render/app files touched):
1. Add `ui/shell/mod.rs` (`pub mod geom;`) and `geom.rs` per §3.1; add `pub mod shell;` to
   `ui/mod.rs`.
2. main_menu_shell/layout.rs: delete RectPx (14-30), mul_div_round (107-114), dlu_rect (116-123),
   right_panel_rects (147-181), lower_strip_rect (183-208), the right-panel/lower consts (86-91);
   re-export/import from `ui::shell::geom`. Keep `sdbtnanm_button_rect` calling
   `snap_button_rect(..., 156, RoundHalfUpFromPanelTop)`; keep `exit_button_rect`,
   right_anchor/title/version/tooltip/website local.
3. single_player_shell/layout.rs: delete its RectPx import re-jig, mul_div_round (50-57),
   dlu_rect (59-66), center_offset (68-74), right_panel_rects (90-124), lower_strip_rect (126-149),
   consts (10-16); call `snap_button_rect(..., 168, BiasedTruncateFromTile)` and
   `last_tile_button_rect(..., 168)`. Keep the LoadSavedGame guard in state.rs untouched.
4. skirmish_shell/layout.rs: delete RectPx (42-67) and re-export `geom::RectPx`
   (keeping `translate` now shared), mul_div_round (229-236), dlu_rect (238-245), center_offset
   (427-429), right_panel_rects (447-476); call `snap_button_rect(..., 156,
   BiasedTruncateFromTile)` + `last_tile_button_rect(..., 156)`. Keep ALL combo/trackbar/checkbox/
   listbox/modal helpers and `translate_layout` local. **Watch:** skirmish re-exports `RectPx` from
   its `mod.rs:16` and is heavily used across `state/*` + render — the re-export path must keep the
   public name stable so no downstream import breaks.

**Acceptance (Slice 0):**
- Every existing per-shell layout test stays green **unchanged** — main_menu_shell/layout.rs tests
  (key_rects_match_800x600, buttons_grid_snap…, title/tooltip/version, 640/1024, responsive),
  single_player tests (key_rects_match_dialog_0x100_rows_at_800x600 incl. the `168`-wide buttons at
  x=632/535, status_help), skirmish state/tests.rs (the big suite) + layout.rs tests
  (key_rects 800/1024/640, choose_map modal, validation modal, trackbar/checkbox/combo geometry).
- New golden test in `ui/shell/geom.rs`: `mul_div_round` == round-half-up MulDiv for all odd DLU
  in 0..=1024 (both signs); `right_panel_rects` byte-equal at 640×480 / 800×600 / 1024×768 to the
  pre-refactor literals already asserted in the three suites.
- New golden test: `snap_button_rect` with `RoundHalfUpFromPanelTop`+156 reproduces the five
  stacked 0xE2 cells (199/241/283/325/367 at 800×600); with `BiasedTruncateFromTile`+168
  reproduces SP rows (199/241/283 + back 535); with +156 reproduces skirmish (241/283 + back 535).
- **No render/app file changes in this slice** — the parallel sessions editing `src/app_*` and
  render shells are untouched.

---

## 5. Full substrate roadmap (planned, NOT applied now)

From study doc §6/§7. Each is a later slice; none is in Slice 0. Marked here so the geom API in
§3.1 is forward-compatible (e.g. RectPx/translate/center_offset are the primitives every later
piece needs).

| Slice | Module | What | Contract (study §5) | Pre-req / risk |
|---|---|---|---|---|
| 1 | `descriptor.rs` + `layout.rs` (pass) | `DialogDescriptor{id,controls,bg_kind,slide_eligible,reposition_policy}`, `ControlDescriptor{id,kind,dlu_rect,csf_key,group,enabled}`, `ControlKind`; `layout_pass()` does DLU→px once + include-set re-anchor + 1px finalizers | C6, C7 | convert 0xE2 first; include-set gating (0x120/0xCE excluded) |
| 2 | `input.rs` controller + `result.rs` | `DialogController{stack, kbd_route}`, descriptor-driven hit/press-must-match, `on_event→Option<ShellAction>`, result-code map + nav table | C1, C3, C4, C5, C12 | retire the 3 hit/press copies + app.rs:1426-1437; SP stays intermediate (0x579→skirmish) |
| 3 | `render/shell_paint/` `OwnerDrawControl` trait | one paint pass over descriptors; SDBTNANM 2/3/4 + ~1Hz flash, pressed sink +2y/+1x, parent_compose order | C8, C9, C10 | render-coupled; re-verify emitter line ranges first |
| 4 | fold skirmish controls | combo/trackbar/checkbox/listbox → `ControlKind`s; unify the two skirmish scroll models | (per-control) | biggest blast radius; the 2147-line tests.rs is the safety net |
| 5 | `modal.rs` + pump | `ModalKind::{Body,BodyOk(0xCE),Confirm(0x120),ThreeButton(0x121)}`; service_tick advances sim+net behind modals; mode-2 SHP compose | C2, C13 | **C13 template-id mapping is UNCHECKED** — trace a caller of the modal helper before wiring ModalKind |
| 6 | `slide.rs` data-driven | drive `app_shell_transition` off the id allow-list; add missing 0x100 first-paint slide; loop bound = max stagger + 6 (reuse WAVE_TAIL_TICKS) | C11 | keep app_shell_transition (generalize, don't rewrite) |
| 7 | `defaults.rs` | `[MultiplayerDialogSettings]` → control seeds (TechLevel 10, GameSpeed 1, FogOfWar off, …) | C14 | reads rules INI, still no render dep |

**Out of scope entirely:** the egui menus (`main_menu.rs`, `main_menu_dialogs.rs`, `pause_menu.rs`)
and the in-game GadgetClass/sidebar (study §0 Framework A) — different service, do not fold.

---

## 6. Risks

1. **Pixel drift from "shared" snap (highest).** The three snap copies are NOT one function:
   two algorithms + a 156-vs-168 width. Mitigation: `SnapRounding` enum + `cell_w` parameter +
   golden tests that reproduce each shell's existing asserted rects. **Do not collapse to one
   formula.** (Default-to-DRIFT: equivalence of the two roundings is unproven across the half-row
   boundary; the enum sidesteps having to prove it.)
2. **`RectPx` re-export path.** skirmish re-exports `RectPx` widely (`mod.rs:16`, used across
   `state/*`, render, app). Slice 0 must keep `ui::skirmish_shell::RectPx` resolving (re-export the
   shared type) so no downstream `use` breaks. Same for main_menu's `pub use` (mod.rs:8).
3. **Parallel human edits to src/ui/* and src/app_*.** This run found app.rs:1426-1437 and the
   render files as the doc describes, but they're moving. Slice 0 deliberately touches **only the
   three `ui/<shell>/layout.rs`** (+ new `ui/shell/`), avoiding the contested app/render files.
4. **`bottom_h.max(0)` placement variance** (skirmish clamps at construction, the other two at
   assignment). Verified algebraically identical, but the golden test pins it so a future edit
   can't silently change one.
5. **Render-side line ranges UNCHECKED.** §7.5/§7.6 ranges in the study doc were not re-verified;
   later slices must re-audit before deleting emitter code.

---

## 7. Verdict

The triplication is real and the consolidation is sound, but the study doc **understates** the
subtle differences — they are exactly the "shared extraction changes a caller's pixels" trap the
task warned about. Slice 0 as scoped here (parameterized geom, two preserved snap algorithms,
golden tests pinning every existing rect, ui-only edits) is safe to implement now; the rest is
roadmap pending the per-slice pre-reqs (notably the C13 modal-template UNCHECKED trace before
Slice 5).
