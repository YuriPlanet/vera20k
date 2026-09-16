# Bitfont / Shell Text Rendering — Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass. Execute it by running
> `/re-investigate bitfont shell text rendering` with this plan loaded as context.
> Phase 1 must produce a usable findings checkpoint before Phase 2 begins.

**Topic:** The BitFont class family and the shell owner-draw text pipeline used by
Skirmish dialog `0x102` and all sibling owner-draw shells. Specifically: how labels
on every Win32 owner-draw control (Button, ComboBox, Static, Checkbox, Trackbar,
ListBox, Edit) get from a `wchar_t*` to pixels on the 16-bit display surface, using
`GAME.FNT` glyphs via `BitFont` and shell wrappers `FUN_00621040` / `FUN_006211D0`.

**Scope Size:** Medium — ~19 functions, 0 INI keys, 1 retail asset (`GAME.FNT`).

**Est. Effort:** ~6–8 hours of `/re-investigate` work
(anchored: 13 FULL functions × ~20 min + 6 LIGHT/MEDIUM × ~5 min + synthesis).

**Prior Research:**
- `SIDEBAR_READY_TEXT_RENDERING.md` — FNT header partially extracted, font-object
  offsets `+0x18`/`+0x1C` known, sidebar "Ready" call site documented. Confidence:
  HIGH for sidebar Ready path, NONE for shell owner-draw path.
- [TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md) — mentions `BitFont__MeasureText` in
  passing for viewport corner text. Out of scope here.
- [SELECTION_BRACKETS_PIPS_DRAW_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/building-selection-brackets/SELECTION_BRACKETS_PIPS_DRAW_ORDER_GHIDRA_REPORT.md) — single mention. Out of
  scope.
- [SKIRMISH_OWNERDRAW_CALLBACKS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_OWNERDRAW_CALLBACKS_GHIDRA_REPORT.md) §1.2 — names `FUN_00621040` /
  `FUN_006211D0` / `FUN_00623880` and their roles, but does not decompile their
  internals. Flags font identity as open question.
- [SKIRMISH_SHELL_ACTIVE_RENDER_PATH_LIVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_SHELL_ACTIVE_RENDER_PATH_LIVE_GHIDRA_REPORT.md) §`0x00621040` —
  documents flag `0x04` = vertical center; horizontal align bits `0x01` center,
  `0x02` right; color conversion via `g_DD_*Loss/Shift`. **Open question 3** in
  that doc: font identity not named beyond `g_GAME_FNT`/bitfont state.
- [CREDITS_COUNTER_SYSTEM.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/CREDITS_COUNTER_SYSTEM.md), [ADDRESS_MAP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADDRESS_MAP.md) — incidental.

**Conflicts to resolve:**
- `SIDEBAR_READY_TEXT_RENDERING.md` claims FNT header `[0x08] = 3 = inter-char
  spacing`, but `src/assets/fnt_file.rs` text-width math uses `+1` per glyph pair.
  Either the field meaning is wrong, or Rust is wrong, or one applies to FNT
  spacing and the other to draw advance. Phase 1 must reconcile this.

**Expected Output:** Research document at
[docs/research/BITFONT_SHELL_TEXT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BITFONT_SHELL_TEXT_GHIDRA_REPORT.md).

**Next Pipeline Step:** `/brainstorm` how to render Rust shell text matching the
binary's BitFont path (likely: extend `sidebar_text.rs` or sibling renderer to
cover shell owner-draw text with binary-faithful spacing, alignment, color
conversion, and disabled-fade). Implementation plan after brainstorm.

---

## 1. Goal

When this investigation finishes, the report must answer:

1. **What pixel-exact algorithm** does the BitFont draw routine
   (`FUN_00434CD0`) use? Including: glyph blit, inter-character advance, line
   advance, tab stops, word-wrap, clipping, fade/disabled overlay.
2. **What is FUN_00621040** — the shell text wrapper — doing on top of the raw
   BitFont draw? Exact behavior of flags `0x01`/`0x02`/`0x04`, color conversion,
   clip-rect application, all unlabeled flag bits.
3. **What is FUN_006211D0** — and when do callers reach it directly vs through
   FUN_00621040? Two text pipelines exist; we need their semantic distinction.
4. **Is `GAME.FNT` the only shell font?** Or do specific shells (Choose Map,
   Score) use different font objects (MSFont/ScoreFont) via the same wrappers?
5. **Why does Rust's text-width use `+1` while the FNT header field 0x08 = 3?**
   Reconcile this. If both apply, when does each fire?
6. **Full BitFont class struct layout** — offsets beyond `+0x18`/`+0x1C`.

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps |
|---|---|---|---|
| `SIDEBAR_READY_TEXT_RENDERING.md` | Sidebar "Ready" call site, FNT header partial, BitFont +0x18/+0x1C | HIGH for narrow scope | No glyph-data layout details, no draw algorithm, no width algorithm, font object full struct, lookup-table semantics |
| [SKIRMISH_OWNERDRAW_CALLBACKS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_OWNERDRAW_CALLBACKS_GHIDRA_REPORT.md) §1.2 | FUN_00621040/006211D0/00623880 named & roles | MEDIUM | All internals open |
| [SKIRMISH_SHELL_ACTIVE_RENDER_PATH_LIVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_SHELL_ACTIVE_RENDER_PATH_LIVE_GHIDRA_REPORT.md) §0x00621040 | Vertical center flag 0x04, color conversion via g_DD_* | HIGH for that one function summary | Tabs, wrap, newline, fade not documented; font ID open |
| [TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md) | Single mention | Out of scope | n/a |

**Conflicts between reports:** The FNT spacing field `[0x08]=3` (SIDEBAR_READY_TEXT_RENDERING.md) vs. Rust's `text_width` adding `+1` per pair (`src/assets/fnt_file.rs:171`) — both internally documented but the relationship is unclear.

## 3. Function Inventory

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Legacy Risk |
|---|---|---|---|---|---|---|
| 1 | 1 | `0x00433880` | `BitFont__Constructor` | Init BitFont object; sets font-object struct fields from the loaded data | FULL | LOW |
| 2 | 1 | `0x00433990` | `BitFont__LoadFontData` | Parses GAME.FNT bytes into the BitFont object; reads all 6 header fields | FULL | LOW |
| 3 | 1 | `0x00433CF0` | `BitFont__MeasureText` | Measures string pixel width — answers the "+1 vs 3" question | FULL | LOW |
| 4 | 1 | `0x00433ED0` | `BitFont__GetTextWidth` | Thin wrapper over MeasureText (body 28 bytes — confirm and dispose) | FULL | LOW |
| 5 | 1 | `0x00434CD0` | `FUN_00434cd0` (unnamed) | **THE bitfont draw routine** — 1602 bytes; glyph blit, advance, line, tabs, wrap, clip | FULL | LOW |
| 6 | 1 | `0x00621040` | `FUN_00621040` | Shell text wrapper: color conv + flags + clip; called by all 6 owner-draw paint paths | FULL | LOW |
| 7 | 1 | `0x006211D0` | `FUN_006211D0` | Lower-level text drawer with H/V align; called by ListBox + ButtonVariant + Edit | FULL | LOW |
| 8 | 2 | `0x00434AE7` area | `BitText__Constructor` | Adjacent class; owns the `GAME.FNT` filename string xref. Resolve BitFont vs BitText relationship | MEDIUM | LOW |
| 9 | 2 | `0x00623880` | `FUN_00623880` | Edit-control text drawer — cursor/selection/password masking on top of FUN_006211D0 | MEDIUM | LOW |
| 10 | 2 | `0x00621B80` | `AlphaBlendRect` | Used for disabled-text fade (button alpha 0x80 path) | MEDIUM | LOW |
| 11 | 2 | `0x004A59E0` | `ComputeTextRect` | Sidebar text-rect helper (already documented); compare to shell path | LIGHT | LOW |
| 12 | 2 | `0x004A60E0` | `DrawText` (sidebar) | Sidebar drawer (already documented); contrast with FUN_00621040 — same engine or different? | MEDIUM | LOW |
| 13 | 2 | `0x008A0DD0`-`0x008A0DE4` | `g_DD_RLoss/RShift/GLoss/GShift/BLoss/BShift` | RGB → 16-bit color conversion globals; FULL extraction of exact arithmetic | FULL | LOW |
| 14 | 3 | `0x00615AE8` in `OwnerDraw_Static_006153E0` | (caller) | Confirms Static reaches FUN_00621040; verify path | LIGHT | LOW |
| 15 | 3 | `0x00616674` in `OwnerDraw_Checkbox` | (caller) | Verify checkbox label routing | LIGHT | LOW |
| 16 | 3 | `0x00617C04` in `OwnerDraw_ComboBox` | (caller) | Verify combo selected-text routing | LIGHT | LOW |
| 17 | 3 | `0x0061E30A` in `OwnerDraw_Trackbar` | (caller) | Verify slider value-text routing | LIGHT | LOW |
| 18 | 3 | `0x006135EE` in `OwnerDraw_Button` | (caller) | Verify button label routing — confirm color `0x00000C05` constant flagged in LIVE doc | LIGHT | LOW |
| 19 | 3 | `DAT_0089C4D0` | `g_GAME_FNT` | Verify global pointer; spot-check whether any shell wrapper uses a different font object | LIGHT | LOW |

**Sizing:** 19 entries, 13 FULL or MEDIUM, 6 LIGHT. Normal scope.

**Phase 1 checkpoint:** After functions 1–7, report:
- FNT header field semantics (all 6 fields)
- Inter-character advance arithmetic (resolve `+1` vs `3` conflict)
- Draw algorithm shape (glyph blit + advance + wrap rules)
- FUN_00621040's complete flag table
- FUN_006211D0's complete flag table
If any of those are still ambiguous, revise scope before Phase 2.

## 4. Detail Checklist

### Magic numbers / constants to decode
- **FNT header field 0x08 = 3** — claimed "inter-char spacing"; verify usage site in BitFont__MeasureText
- **FNT header field 0x14 = 29655** — claimed "row stride"; verify
- **FNT header field 0x18 = 49** — claimed "total rows"; verify (also glyph_stride in Rust)
- **Font object `+0x18`** — set to 3 (spacing); confirm
- **Font object `+0x1C`** — set to 17 (height); confirm
- **Disabled alpha `0x80`** — Button path; confirm exact AlphaBlendRect call signature
- **Button text color `0x00000C05`** — recovered from button call site; convert to expected display RGB and verify against retail screenshots later

### Bit flags to decode in FUN_00621040
- `0x01` — horizontal center (claimed)
- `0x02` — horizontal right (claimed)
- `0x04` — vertical center (claimed)
- All other bits — unknown; enumerate from the function body

### Bit flags in FUN_006211D0
- Horizontal align bits same as 00621040 or different?
- Vertical align flag — same?
- Any wrap/truncate/multiline flags?

### State machine / algorithm
- Glyph lookup: how does the draw routine resolve `wchar_t` codepoint → glyph slot index?
- Empty/missing glyph fallback: does it skip, draw a placeholder, or use index 0?
- Newline handling: line-height = `+0x1C` (17)? Or font-internal?
- Tab handling: tab width source (from FNT? from caller?)
- Word-wrap: at last-recorded space, or hard cut?
- Clip rect: clip glyph pixel-by-pixel or skip whole glyphs outside?

### Color conversion (FULL extraction)
- `g_DD_RLoss/GLoss/BLoss` semantics — bits to drop per channel
- `g_DD_RShift/GShift/BShift` — bit positions for each channel
- Exact arithmetic: `(R >> RLoss) << RShift | (G >> GLoss) << GShift | (B >> BLoss) << BShift`?
- 16-bit format: 565? 555? Verify from globals at runtime

### BitFont struct offsets to extract
- Beyond `+0x18` (spacing) and `+0x1C` (height), enumerate every field BitFont__LoadFontData writes
- Find the glyph-data pointer field
- Find the lookup-table pointer field
- Document `param_1` type (`int` vs `int *`) per CLAUDE.md pitfall

### Clamps, rounding, off-by-ones
- Width measurement final `+1` or `-1` adjustments (the conflict)
- Vertical center: `(rect_h - text_h) / 2` — is it floor, round, or ceil?
- Clipping at glyph boundaries: inclusive or exclusive?

### Edge cases
- Empty string `""`
- Null `wchar_t *`
- String exceeds clip rect (truncate vs ellipsis?)
- String with only spaces
- Codepoint 0 in middle (early terminate?)
- Codepoints beyond 0xFFFF (lookup table is `u16` indexed)

### TS-legacy: low risk
- BitFont/GAME.FNT exists in YR and is hot-path (every owner-draw label). No TS gating expected.
- BUT: confirm the FNT format is the same between TS and YR. The "fonT" magic suggests "Tiberian sun" era; YR might have new fields appended that weren't seen by older parsers.

### Vtable dispatches
- Does BitFont have a vtable? RTTI string `.?AVBitFont@@` at 0x00818b70 suggests yes. Resolve the vtable.
- BitText (separate class at `.?AVBitFont@@` adjacent area) — also has vtable?

## 5. INI Keys in Scope

| Key | Section | Default | Suspected Purpose | Currently Parsed in Rust? |
|---|---|---|---|---|
| (none) | — | — | The bitfont system has no INI surface; `GAME.FNT` is hardcoded | n/a |

The font system is fully asset-driven and code-internal; no INI flags affect it.

## 6. Caller & Integration Map

### FUN_00621040 (shell text wrapper) — 12 call sites:
| Caller Address | Caller Function | Decompile? |
|---|---|---|
| `0x006135EE` | `OwnerDraw_Button_00612B70` (button label) | LIGHT — confirm color/flag args |
| `0x00615AE8` | `OwnerDraw_Static_006153E0` (static text kind 0) | LIGHT |
| `0x00616674` | `OwnerDraw_Checkbox_006163A0` (checkbox label) | LIGHT |
| `0x00616FF0` | `OwnerDraw_RadioVariant_00616980` (radio label) | LIGHT |
| `0x00617C04` | `OwnerDraw_ComboBox_00617250` (selected text) | LIGHT |
| `0x0061E30A` | `OwnerDraw_Trackbar_0061D950` (numeric value text) | LIGHT |
| `0x005532F4`/`0x005539DF`/`0x00553D01` | `CCFileClass__Constructor` (?) | NO — likely unrelated function-overload labeling |
| `0x0055433C` | `FUN_00554280` | NO |
| `0x0060DFC8` | unlabeled | NO |
| `0x00614131` | (probably Edit) | LIGHT — verify it's in Edit callback |

### FUN_006211D0 (lower text) — 9 call sites:
| Caller Address | Caller | Decompile? |
|---|---|---|
| `0x0061E8A6` | `OwnerDraw_ButtonVariant_0061E700` (frame text) | LIGHT |
| `0x006199DD`, `0x00619B42` | `OwnerDraw_ListBox_00618D40` (list items) | LIGHT — list item text is critical for combo dropdowns |
| `0x00623AD9`, `0x00623B52`, `0x00623B9E`, `0x00623BEB` | `FUN_00623880` (Edit/NewEdit text) | LIGHT — confirm 4 distinct call sites = 4 modes (normal, cursor, selection, password?) |
| `0x004C3DBA` | `FUN_004C3D00` | NO |
| `0x00610AFB` | `BSurface__Constructor` | NO |

### Rust integration points
- **Today:** `src/render/sidebar_text.rs` consumes `FntFile` from `src/assets/fnt_file.rs`
- **Today:** `src/app_skirmish_shell_render.rs:242` reuses `state.sidebar_text` for shell labels — already using the BitFont path through the same renderer
- **Will need:** verification that the sidebar renderer's algorithm (inter-char `+1`, no vertical center, no clip-rect, no fade) matches what FUN_00621040 does for the same input. If not, either extend `sidebar_text` or fork a `shell_text` renderer.

## 7. TS-Legacy Risk Register

- **GAME.FNT** — hot-path in YR; no gating. LOW risk.
- **`.?AVBitFont@@`** vs `.?AVBitText@@` — two related RTTI types. Possible TS-era distinction; resolve in Phase 2 (#8).
- **MSFont (FULLFNT3.SHP)** — out of scope for this investigation (used by MapSelect / Score), but its existence means BitFont is NOT the only shell font. Phase 2 should explicitly check whether any FUN_00621040 caller swaps the font global. If yes, scope grows.
- **Win32 `CreateFontIndirectA`** — referenced in binary ("MapSelect: Unable to create font!"). Confirms MapSelect uses a Win32 GDI font, not BitFont. Out of scope here but flagged.

## 8. Current Rust Implementation Surface

| File | Lines | What it does | Gap vs binary |
|---|---|---|---|
| `src/assets/fnt_file.rs` | 273 | Parses GAME.FNT into `FntGlyph` map by `u16` codepoint; `text_width()` uses `+1` per glyph pair; decodes 1bpp glyph rows to RGBA white-on-transparent | Header field semantics not fully verified; spacing arithmetic conflict vs FNT field `[0x08]=3` |
| `src/render/sidebar_text.rs` | (read first 80 lines) | Packs glyph bitmaps into a GPU atlas; only packs codepoints 0x20–0x180 (printable ASCII + Latin-1 extended); has hardcoded 5×7 fallback; owns a 1×1 darken texture for "Ready" overlay | Codepoint range too narrow for full CSF (Korean/Chinese/Russian locales); no clip-rect; no fade; no shell-style vertical center |
| `src/app_sidebar_text.rs` | n/a | App-layer text builder | Same scope as sidebar_text |
| `src/app_skirmish_shell_render.rs:242` | 1 line | Uses `state.sidebar_text.text_width()` and `build_text()` for shell labels | Inherits all sidebar_text limitations; can't match shell's vertical-center / color / disabled fade |

## 9. Deferred Open Questions

1. **Does MapSelect's MSFont path also reach FUN_00621040?** Or is it entirely separate? Defer until investigation reaches Phase 3 #19 (g_GAME_FNT global verification).
2. **BitText vs BitFont** — separate classes or inheritance? Decompile in Phase 2 #8.
3. **Localization / Unicode** — does the shell ever pass a `wchar_t*` containing codepoints beyond 0xFFFF? FNT lookup table is u16-indexed, suggesting no, but verify.
4. **What's in the font object beyond `+0x18`/`+0x1C`?** Phase 1 #1/#2 should extract complete struct layout.
5. **MS Sans Serif from dialog template** — the resource specifies MS Sans Serif 8pt; is this *completely ignored* by owner-draw paths (which all use BitFont/GAME.FNT)? Spot-check by following the dialog template font field through `CreateDialogIndirectParamA`.

## 10. Execution Strategy

**Recommended: Single-session /re-investigate with Phase 1 checkpoint.**

Reason: 19 functions, ~6-8 hours total, all concentrated in two code regions
(`0x00433xxx`-`0x00434xxx` for BitFont, `0x00621xxx` for shell wrappers).
Cross-function context is high — splitting across sessions would force
re-context-loading.

Within the single session:
1. Execute Phase 1 (#1–#7) end-to-end, report checkpoint findings
2. User reviews Phase 1; revise scope if assumptions break
3. Execute Phase 2 (#8–#13) for depth
4. Execute Phase 3 (#14–#19) for caller-context confirmation
5. Synthesize → [BITFONT_SHELL_TEXT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BITFONT_SHELL_TEXT_GHIDRA_REPORT.md)

If Phase 1 reveals the draw algorithm is much larger than expected (e.g.,
function body 1602 bytes contains multiple sub-algorithms), split #5 into
`#5a draw skeleton`, `#5b advance/wrap`, `#5c clip/fade` and add a Phase 1.5.

## 11. Success Criteria

The executed research document must:
- Answer every question in Section 1
- Include every function from Section 3 (or explicitly justify omission)
- Resolve every conflict in Section 2 (especially the `+1` vs `3` spacing)
- Resolve or re-document every question in Section 9
- State "Active in YR: Yes" for BitFont path (gating is unrealistic, but the doc must say it)
- Provide a pixel-perfect spec usable by a Rust implementer:
  - Glyph blit algorithm
  - Inter-character advance formula
  - Line-height for newline
  - Tab-stop algorithm
  - Word-wrap rule
  - Clip-rect application
  - Color conversion math (16-bit, exact)
  - Disabled-fade algorithm
- Cite Ghidra addresses for every HIGH-confidence claim

## Sources

- Ghidra addresses sampled: `0x00433880`, `0x00433990`, `0x00433CF0`, `0x00433ED0`,
  `0x00434AE7`, `0x00434CD0`, `0x00621040`, `0x006211D0`, `0x00621B80`, `0x00623880`,
  `0x004A59E0`, `0x004A60E0`, `DAT_0089C4D0`, `DAT_008A0DD0`–`DAT_008A0DE4`
- Docs searched:
  - `docs/research/SIDEBAR_READY_TEXT_RENDERING.md`
  - `docs/research/SKIRMISH_OWNERDRAW_CALLBACKS_GHIDRA_REPORT.md` §1.2
  - `docs/research/SKIRMISH_SHELL_ACTIVE_RENDER_PATH_LIVE_GHIDRA_REPORT.md` §0x00621040
  - [docs/research/TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md)
  - `docs/research/SELECTION_BRACKETS_PIPS_DRAW_ORDER_GHIDRA_REPORT.md`
  - [docs/research/CREDITS_COUNTER_SYSTEM.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/CREDITS_COUNTER_SYSTEM.md)
  - [docs/research/ADDRESS_MAP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADDRESS_MAP.md)
- INI files checked: none — bitfont has no INI surface
- Rust files inspected: `src/assets/fnt_file.rs`, `src/render/sidebar_text.rs`
  (first 80 lines), `src/app_skirmish_shell_render.rs:242`
- Related plans: `docs/plans/2026-05-16-skirmish-shell-pixel-parity-design.md`
  (text rendering explicitly deferred), `docs/plans/2026-05-17-skirmish-shell-verified-assets-plan.md`
