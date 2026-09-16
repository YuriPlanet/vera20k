# Skirmish Status Color Item STT Table - Ghidra Research Report

**Address(es):** `0x006AE3F0`, `0x004E4230`, `0x004E4E20`, `0x004E42A0`  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** Exact item-data values consumed by the standard YR Skirmish color-dropdown status path and the `STT:PlayerColor*` keys returned for Random, normal colors, and Observer.  
**Non-Scope:** side/country item status, generic `0x695` source precedence beyond the color branch, color combo paint/swatch rendering, online/lobby-only dialog behavior, and Rust implementation.  
**Confidence:** High for the mapping helper and active `0x102` branch; Medium for Observer reachability from standard Skirmish dropdown rows because the mapping branch exists but normal `0x102` color population omits item-data `8`.  
**Active in YR:** Yes for standard Skirmish color controls `0x6A2` and `0x522..0x528`; Observer mapping is Conditional as described below.

## 0. Working Notes Gate

- Target question: What exact color dropdown item-data values map to each `STT:PlayerColor*` key in the Skirmish status/help path?
- Non-goals: Do not rediscover the generic `0x695` static map, do not cover side/country rows, do not implement Rust, and do not mutate Ghidra.
- Evidence needed to mark COMPLETE: active `0x102` caller into color item status, color-control recognizer, item-data reader semantics including `-1`, exact `FUN_004E42A0` item-data table with key spellings, current Rust delta, negative facts, and implementation handoff.
- Stop conditions: Stop after `FUN_006AE3F0`, `FUN_004E4230`, `FUN_004E4E20`, and `FUN_004E42A0` have a zero-add re-read and every scoped open question is resolved or explicitly deferred.

## 1. Overview

When the parent Skirmish status path receives an open color-combo row hover, `FUN_006AE3F0` recognizes color controls through `FUN_004E4230`, reads that row's combo item data through `FUN_004E4E20`, maps the item data through `FUN_004E42A0`, and stores the localized string holder through `FUN_007B6880`. The Rust-facing result is a compact native mapping: `-2` is Random, `0..7` are the eight playable colors, and `8` is Observer.

Active in YR: Yes. Evidence: the prior full mapping report proved standard dialog `0x102` uses the common status path; this pass re-read `FUN_006AE3F0` and assembly context `0x006AE531..0x006AE598`, where the color branch calls `FUN_004E4230`, `FUN_004E4E20`, then `FUN_004E42A0`.

## 2. Key Offsets / Values

| Value | Meaning | Evidence | Active in YR |
|---:|---|---|---|
| `0x6A2` | local player color combo, slot `0` | `FUN_004E4230` | Yes |
| `0x522..0x528` | AI row color combos, slots `1..7` | `FUN_004E4230` | Yes |
| `-1` | non-color control return from recognizer; selected-row sentinel to item-data getter when helper is called directly | `FUN_004E4230`; `FUN_004E4E20` assembly `0x004E4E2D..0x004E4E4D` | Yes |
| `-2` | Random/no-fixed-color color item data | `FUN_004E42A0`; prior population report `FUN_004E45A0` | Yes |
| `0..7` | normal playable color item data | `FUN_004E42A0`; prior population report `FUN_004E45A0` | Yes |
| `8` | Observer color item data | `FUN_004E42A0`; prior population report says normal Skirmish population omits row `8` | Conditional |

## 3. Core Logic

### 3.1 Active Color Status Branch

Active in YR: Yes. In `FUN_006AE3F0`, after AI row checks fail, the parent `0x4E9` handler passes the hovered control id in `ESI` to `FUN_004E3830`; only if that returns `-1` does it pass the same id to `FUN_004E4230`. Assembly context:

- `0x006AE531..0x006AE544`: `ESI` is moved to `ECX`, `FUN_004E4230` is called, and `EAX` is compared with `-1`.
- `0x006AE581..0x006AE598`: the item index from `[EDI+4]` is pushed, `ECX=parent hwnd`, `EDX=control id`, `FUN_004E4E20` returns item data in `EAX`; that `EAX` is moved to `ECX` and passed to `FUN_004E42A0`; the returned string pointer is passed to `FUN_007B6880`.

The branch is guarded by `param_4[1] != -1` in the decompile. Therefore the item-specific color status branch is for concrete hovered row indexes. The generic "selected row if item is `-1`" behavior exists inside `FUN_004E4E20`, but the standard `0x102` parent status branch does not use it for item-specific color text.

### 3.2 Color Control Recognizer

Active in YR: Yes. `FUN_004E4230` maps only the eight standard Skirmish color combo control IDs:

| Control id | Slot return |
|---:|---:|
| `0x6A2` | `0` |
| `0x522` | `1` |
| `0x523` | `2` |
| `0x524` | `3` |
| `0x525` | `4` |
| `0x526` | `5` |
| `0x527` | `6` |
| `0x528` | `7` |
| any other id | `-1` |

Evidence: `FUN_004E4230` decompile and assembly context `0x004E4230..0x004E4248`; final expression returns `7` only for `0x528`, otherwise signed `-1`.

### 3.3 Item-Data Reader

Active in YR: Yes. `FUN_004E4E20(hwnd, control_id, item_index)` reads `CB_GETITEMDATA (0x150)`. If `item_index == -1`, it first reads `CB_GETCURSEL (0x147)` and then reads item data for the selected index. Although the decompiler prints `void`, assembly returns the `SendDlgItemMessageA(...,0x150,...)` result in `EAX`.

Evidence: assembly context `0x004E4E2D..0x004E4E4D` shows compare against `-1`, optional `0x147`, then `0x150` and function return; `FUN_006AE3F0` assembly `0x006AE589..0x006AE590` immediately moves `EAX` to `ECX` for `FUN_004E42A0`.

## 4. Exact Color Item Data -> STT Table

Active in YR: Yes for `-2` and `0..7` in standard Skirmish dropdown rows. Active in YR: Conditional for `8` because `FUN_004E42A0` maps it, but prior verified normal color population `FUN_004E45A0` adds only rows `0..7` after the `-2` sentinel and stops before initialized row `8`.

| Native item data | Status key | String id arg | Key pointer | Evidence |
|---:|---|---:|---:|---|
| `-2` | `STT:PlayerColorRandom` | `0x1C1` | `0x00822AC4` | `FUN_004E42A0`, `0x004E42A0..0x004E42B6`; PE string read |
| `0` | `STT:PlayerColorGold` | `0x1C3` | `0x00822AB0` | `FUN_004E42A0`, `0x004E42BC..0x004E42D1`; PE string read |
| `1` | `STT:PlayerColorRed` | `0x1C5` | `0x00822A9C` | `FUN_004E42A0` decompile; PE string read |
| `2` | `STT:PlayerColorBlue` | `0x1C7` | `0x00822A88` | `FUN_004E42A0` decompile; PE string read |
| `3` | `STT:PlayerColorGreen` | `0x1C9` | `0x00822A70` | `FUN_004E42A0`, `0x004E430F..0x004E4325`; PE string read |
| `4` | `STT:PlayerColorOrange` | `0x1CB` | `0x00822A58` | `FUN_004E42A0` decompile; PE string read |
| `5` | `STT:PlayerColorSkyBlue` | `0x1CD` | `0x00822A40` | `FUN_004E42A0` decompile; PE string read |
| `6` | `STT:PlayerColorPurple` | `0x1CF` | `0x00822A28` | `FUN_004E42A0` decompile; PE string read |
| `7` | `STT:PlayerColorPink` | `0x1D1` | `0x00822A14` | `FUN_004E42A0`, `0x004E437F..0x004E4395`; PE string read |
| `8` | `STT:PlayerColorObserver` | `0x1D3` | `0x008229FC` | `FUN_004E42A0`, `0x004E439B..0x004E43B1`; PE string read |
| any other value, including `-1` if directly passed | null / no item-specific string | n/a | n/a | `FUN_004E42A0`, `0x004E43B7..0x004E43B9` |

The key spelling evidence is a direct PE read of `gamemd.exe` at the key pointers loaded into `ECX` before `StringTable__LoadString`. `0x008229C0` is the adjacent source-file string `D:\ra2mdpost\GDlgSupp.cpp`.

## 5. INI Keys

No INI key controls this status mapping. Active in YR: Yes. The path is binary control-id/item-data and string-table driven; the scoped functions do not call INI readers.

## 6. Integration Points

| Point | Role | Evidence | Active in YR |
|---|---|---|---|
| `FUN_006AE3F0` | Standard Skirmish parent proc `0x4E9` item-status dispatcher | decompile; assembly `0x006AE531..0x006AE598`; prior full mapping report proves `0x102` liveness | Yes |
| `FUN_004E4230` | Color control-id recognizer | decompile; assembly `0x004E4230..` | Yes |
| `FUN_004E4E20` | combo item-data getter with selected fallback if called with `-1` | decompile; assembly `0x004E4E2D..0x004E4E4D` | Yes |
| `FUN_004E42A0` | item-data to `STT:PlayerColor*` key loader | decompile; assembly contexts through `0x004E43B9`; PE string reads | Yes / Conditional for `8` reachability |
| `FUN_004E45A0` | normal color combo population, from prior report | [SKIRMISH_COLOR_COMBO_POPULATION_AND_SWATCH_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_COLOR_COMBO_POPULATION_AND_SWATCH_ORDER_GHIDRA_REPORT.md) | Yes |

## 7. Current Rust Implementation Status

Current Rust already models the color item values needed for the standard normal dropdown rows: `ColorSentinel(-2)` followed by `Color(0..HOUSE_COLOR_COUNT)`, with tests proving `Color(8)` is not included. It does not yet use item-specific color status keys; any non-AI `ComboItem` falls back to `status_help_key_for_combo`, so color dropdown rows show `STT:SkirmishComboColor` instead of `STT:PlayerColor*`.

Evidence: `src/ui/skirmish_shell/state/combos.rs:283..284`, `src/ui/skirmish_shell/state/hit_test.rs:138..142`, `src/ui/skirmish_shell/state/hit_test.rs:196..200`, `src/ui/skirmish_shell/state/tests.rs:1465..1482`. Active in YR: Rust comparison only; binary behavior above is the spec.

## 8. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Standard `0x102` status branch reaches color item helper | verified | `FUN_006AE3F0`, `0x006AE531..0x006AE598`; prior full mapping report | none |
| Color control-id recognizer values | verified | `FUN_004E4230` | none |
| Item-data getter return semantics and `-1` selected fallback | verified | `FUN_004E4E20`, assembly `0x004E4E2D..0x004E4E4D` | none |
| Exact `-2,0..8` color status table | verified | `FUN_004E42A0`; PE string reads at `0x008229FC..0x00822AC4` | none |
| Standard normal Skirmish population of color rows | verified-by-prior | [SKIRMISH_COLOR_COMBO_POPULATION_AND_SWATCH_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_COLOR_COMBO_POPULATION_AND_SWATCH_ORDER_GHIDRA_REPORT.md) | none for this slice |
| Observer row in normal standard Skirmish dropdown | verified-negative-by-prior | prior report: `FUN_004E45A0` inserts `0..7`, not `8` | none |
| Online/lobby observer row reachability | deferred | out of scope | separate lobby/observer-mode investigation if needed |
| Current Rust status mapping delta | verified | source scan paths listed above | implementation later |

## 9. Open Questions - Final State

- `[RESOLVED] OQ-001 - Is this color item-status branch active in standard YR Skirmish? -> Yes, standard `0x102` status `0x4E9` reaches `FUN_004E4230 -> FUN_004E4E20 -> FUN_004E42A0` for color controls with concrete row indexes.` (evidence: `FUN_006AE3F0`, `0x006AE531..0x006AE598`; prior full mapping report)
- `[RESOLVED] OQ-002 - Which control ids are color combos? -> `0x6A2` and `0x522..0x528`; any other id returns `-1`.` (evidence: `FUN_004E4230`)
- `[RESOLVED] OQ-003 - Is Random `-2`? -> Yes, item data `-2` maps to `STT:PlayerColorRandom`.` (evidence: `FUN_004E42A0`, key pointer `0x00822AC4`)
- `[RESOLVED] OQ-004 - Are playable colors `0..7`? -> Yes, `0..7` map Gold, Red, Blue, Green, Orange, SkyBlue, Purple, Pink.` (evidence: `FUN_004E42A0`, key pointers `0x00822AB0..0x00822A14`)
- `[RESOLVED] OQ-005 - Is Observer item data `8` or a sentinel? -> Observer is item data `8`, not a separate negative sentinel.` (evidence: `FUN_004E42A0`, key pointer `0x008229FC`)
- `[RESOLVED] OQ-006 - What happens for other item data? -> `FUN_004E42A0` returns null for values outside `-2,0..8`.` (evidence: `0x004E43B7..0x004E43B9`)
- `[RESOLVED] OQ-007 - Does status item `-1` select the current row? -> Not in the standard parent item-status branch because `FUN_006AE3F0` guards `param_4[1] != -1`; if `FUN_004E4E20` is called directly with `-1`, it reads `CB_GETCURSEL` first.` (evidence: `FUN_006AE3F0`; `FUN_004E4E20` assembly)
- `[RESOLVED] OQ-008 - Does normal Skirmish population insert Observer? -> No, prior verified `FUN_004E45A0` inserts `-2` and `0..7`; initialized row `8` is not inserted by that normal path.` (evidence: [SKIRMISH_COLOR_COMBO_POPULATION_AND_SWATCH_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_COLOR_COMBO_POPULATION_AND_SWATCH_ORDER_GHIDRA_REPORT.md))
- `[RESOLVED] OQ-009 - Does current Rust use item-specific color status? -> No, color `ComboItem` falls back to generic `STT:SkirmishComboColor`.` (evidence: `state/hit_test.rs:138..142`, `:196..200`)
- `[DEFERRED] OQ-010 - Which non-standard online/lobby paths can feed item data `8` to this helper?` (category: out-of-scope; reason: target is standard Skirmish status handoff and the helper mapping is already exact; next-step-if-pursued: investigate observer-mode/online color combo population)

## 10. Visual/UI Composition Ledger

This is a status text source slice, not a paint/composition slice. No SHP, palette, rect, or z-order claim is made here. The visible effect is that the already-verified `0x695` status strip receives the item-specific `STT:PlayerColor*` text instead of the generic color-combo help text.

## 11. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Color row item data maps exactly as `-2 -> STT:PlayerColorRandom`, `0..7 -> Gold/Red/Blue/Green/Orange/SkyBlue/Purple/Pink`, `8 -> Observer`, otherwise null | `FUN_004E42A0`; PE key reads `0x008229FC..0x00822AC4` | missing | `src/ui/skirmish_shell/state/hit_test.rs` status resolver | Add a color item-status resolver for `SkirmishComboItem::ColorSentinel(-2)` and `Color(0..7)`, with `8` supported only if the model ever exposes it | Open color dropdown and hover Random/Gold/Pink rows; status uses `STT:PlayerColorRandom/Gold/Pink`, not `STT:SkirmishComboColor`; proposed test `test_skirmish_status_color_item_keys_match_native` | Do not derive status from visible row labels or swatch colors |
| Normal standard Skirmish color dropdown rows are Random plus playable colors `0..7`; Observer item data `8` is a supported mapping branch but not normally inserted | prior population report `FUN_004E45A0`; current Rust tests `state/tests.rs:1465..1482` | mostly aligned for item model | `src/ui/skirmish_shell/state/combos.rs`, tests | Keep standard dropdown generation at `-2,0..7` unless a separate observer-mode path is implemented; table resolver may still include `8` defensively | Standard combo item test continues to reject `Color(8)` while status-mapper unit test still documents `8 -> STT:PlayerColorObserver`; proposed test `test_skirmish_color_combo_items_exclude_observer_but_mapper_knows_observer` | Do not add Observer to ordinary offline Skirmish color dropdown just because the status helper can map it |
| The standard parent status branch does not use `-1` to select current color item; it only runs item-specific status for concrete row indexes | `FUN_006AE3F0` guard plus `FUN_004E4E20` assembly | current Rust hover model already carries concrete dropdown item | `hover_open_combo_item`, `status_help_key_for_hover` | Resolve color item status from the hovered row item, not the selected face, for open dropdown row hovers; closed combo face remains generic `STT:SkirmishComboColor` | Hover closed color combo face -> generic combo help; open dropdown and hover row -> item-specific color help; proposed test `test_skirmish_status_color_face_generic_row_item_specific` | Do not apply selected-item color help to the closed combo face |

### Negative Facts / Do Not Do

- Do not treat Observer as a separate negative sentinel. Active in YR: Conditional. Evidence: `FUN_004E42A0` maps item data `8` to `STT:PlayerColorObserver`.
- Do not include Observer in the ordinary standard Skirmish color dropdown solely because the status helper supports it. Active in YR: Yes for normal population omission. Evidence: prior `FUN_004E45A0` report processes rows `0..7` and stops before row `8`.
- Do not use generic `STT:SkirmishComboColor` for open color dropdown row hovers. Active in YR: Yes. Evidence: `FUN_006AE3F0`, `0x006AE581..0x006AE598`.
- Do not use visible color names, swatch RGBs, or `[Colors]` INI data as the status source. Active in YR: Yes. Evidence: `FUN_004E42A0` loads hardcoded `STT:PlayerColor*` keys; no INI reader is in the scoped chain.
- Do not rely on the decompiler's `void` return for `FUN_004E4E20`. Active in YR: Yes. Evidence: caller moves `EAX` from `FUN_004E4E20` into `ECX` for `FUN_004E42A0` at `0x006AE589..0x006AE590`.

## 12. Remaining Uncertainty

- Online/lobby/observer-mode population paths that may expose item data `8` were not investigated; this report only needs the helper mapping and standard Skirmish Rust handoff.

## 13. Stale Docs / Replacement Wording

- [docs/research/skirmish-ui/SKIRMISH_STATUS_CHILD_0X695_TEXT_SOURCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_STATUS_CHILD_0X695_TEXT_SOURCE_GHIDRA_REPORT.md): replace the broad sentence "For other scoped combo/list families, `FUN_006AE3F0` attempts item-specific text through combo/list helper functions (`FUN_004E3830`, `FUN_004E4230`, `FUN_004E4EC0`, and related getters). This report did not expand those helper families..." with "For color combo controls, the item-specific status table is now verified in `SKIRMISH_STATUS_COLOR_ITEM_STT_TABLE_GHIDRA_REPORT.md`: `-2 -> STT:PlayerColorRandom`, `0..7 -> Gold/Red/Blue/Green/Orange/SkyBlue/Purple/Pink`, and `8 -> STT:PlayerColorObserver`; standard offline Skirmish normal population inserts `-2` and `0..7`, not Observer."

## Sources

- Ghidra decompile: `FUN_006AE3F0`, `FUN_004E4230`, `FUN_004E4E20`, `FUN_004E42A0`.
- Ghidra assembly contexts: `0x006AE531..0x006AE598`, `0x004E4230..0x004E4248`, `0x004E4E2D..0x004E4E4D`, `0x004E42A0..0x004E43B9`.
- PE string reads from `<ra2-install>/gamemd.exe`: `0x00822AC4`, `0x00822AB0`, `0x00822A9C`, `0x00822A88`, `0x00822A70`, `0x00822A58`, `0x00822A40`, `0x00822A28`, `0x00822A14`, `0x008229FC`.
- Prior docs: [SKIRMISH_0X102_STATUS_HELP_FULL_MAPPING_CURRENT_RUST_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_0X102_STATUS_HELP_FULL_MAPPING_CURRENT_RUST_GHIDRA_REPORT.md), [SKIRMISH_COLOR_COMBO_POPULATION_AND_SWATCH_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_COLOR_COMBO_POPULATION_AND_SWATCH_ORDER_GHIDRA_REPORT.md), [SKIRMISH_STATUS_CHILD_0X695_TEXT_SOURCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_STATUS_CHILD_0X695_TEXT_SOURCE_GHIDRA_REPORT.md).
- Rust scan only: `src/ui/skirmish_shell/state/hit_test.rs`, `src/ui/skirmish_shell/state/combos.rs`, `src/ui/skirmish_shell/state/tests.rs`.
