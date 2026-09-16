# Skirmish Start Validation Modal Activation Recheck - Ghidra Report

**Date:** 2026-05-24  
**Address(es):** `0x006ACEE0`, `0x005D3490`, `0x00622650`, `0x00622B50`, `0x00621E90`  
**Investigation Mode:** targeted verification slice  
**Claimed Scope:** whether ordinary offline Skirmish Start failure actually reaches the generic message dialog helper, which dialog template it selects, and which `PUDLGBG*` theme branch is active for that shell/no-game modal.  
**Non-Scope:** exhaustive catalog of all dialogs using `PUDLGBG*`, native screenshot capture, exact RGB pixels after DirectDraw conversion, or further Rust implementation.  
**Confidence:** High for activation/template/theme branch; Medium for final perceived pixels because no runtime screenshot was captured.  
**Active in YR:** Yes for ordinary offline Skirmish Start validation failures.

## 1. Bottom Line

The use case is real: pressing Start in offline Skirmish can fail validation before launch, and those failure branches call `FUN_005D3490`, the generic shell message dialog helper.

The ordinary failure calls pass:

- `param_1`: body text
- `param_2`: OK text
- `param_3`: zero / absent
- `param_4`: zero / absent

That makes `FUN_005D3490` select RT_DIALOG `0xCE`, the one-body-text plus one-OK-button template. The dialog is then painted through the common mode-2 dialog background path.

For native parity, standard shell/no-game state selects `PUDLGBGN.SHP + DIALOGN.PAL`, not the Soviet variant. The Soviet `PUDLGBGS.SHP + DIALOG.PAL` branch exists, but it is conditional on the in-game side-1 branch in `WM_PAINT_Handler @ 0x00621E90`.

## 2. Start Handler Activation Evidence

`FUN_006ACEE0` handles Skirmish shell command traffic. In the Start/Back command block:

- It handles `param_2 == 0x617` as the Start Game owner-draw button.
- On Start it disables control `0x617` before validation.
- It counts active AI row-state combo selections across controls `0x50B`, `0x50E`, `0x516`, `0x51A`, `0x51B`, `0x51C`, and `0x51D`.
- It writes the active-count result to `DAT_00A8B274`.
- If a validation branch fails, it calls `FUN_005D3490(...)`.
- After the modal call returns, it re-enables control `0x617`.

Verified failure branches in `FUN_006ACEE0`:

| Failure branch | Native call behavior | Evidence | Active in target |
|---|---|---|---|
| Map capacity smaller than requested player count | loads text, formats with capacity, calls `FUN_005D3490(body, ok, 0, 0, 0)`, re-enables Start | decompile `0x006ACEE0`, call in first Start validation branch | Yes |
| Fewer than two total players | loads text, calls `FUN_005D3490(body, ok, 0, 0, 0)`, re-enables Start | decompile `0x006ACEE0`, second Start validation branch | Yes |
| Same explicit team rejection | loads text, calls `FUN_005D3490(body, ok, 0, 0, 0)`, re-enables Start | decompile `0x006ACEE0`, team check loop branch | Yes |
| Selected-mode/session rejection returning output id `0x617` | calls `FUN_005D3490(...)` and re-enables Start | decompile `0x006ACEE0`, lower failure branch after session validation | Conditional |

This confirms the dialog is not an invented Rust-only concept. It is the native result of invalid Start Game conditions.

## 3. Dialog Template Selection Evidence

`FUN_005D3490(short *body, short *ok, short *third, short *fourth)` checks whether each incoming text pointer is non-null and non-empty. It then creates the dialog with `FUN_00622650`.

Existing assembly/resource report resolves the hidden register argument to `FUN_00622650`:

- Default dialog id is `0xCE`.
- If `param_3` is non-empty, it selects `0x120`.
- If `param_4` is non-empty, it selects `0x121`.

For the ordinary Start validation calls, `param_3` and `param_4` are zero, so the selected template is `0xCE`.

`FUN_00622650` then:

- Loads RT_DIALOG resource type `5` via `FUN_004A3B40`.
- Calls `CreateDialogIndirectParamA`.
- Stores the dialog id in the dialog tracking table.
- Dispatches setup/subclassing.

Resource `0xCE` is verified as:

| Item | Value | Evidence |
|---|---:|---|
| Dialog unit rect | `x=0 y=0 cx=300 cy=200` | resource parse in [VALIDATION_MODAL_0X005D3490_DIALOG_TEMPLATE_CONTROL_RECTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/VALIDATION_MODAL_0X005D3490_DIALOG_TEMPLATE_CONTROL_RECTS_GHIDRA_REPORT.md) |
| Font | `8pt MS Sans Serif` | resource parse |
| Child count | `2` | resource parse |
| Body static | control `0x5B0`, `x=40 y=40 cx=220 cy=50` | resource parse + `FUN_005D3490` text write |
| OK button | control `0x5AE`, `x=207 y=175 cx=83 cy=15` | resource parse + `FUN_005D3490` text write |

## 4. Paint / Theme Branch Evidence

`FUN_00622B50` handles dialog messages. On `WM_PAINT (0x0F)`, it calls `WM_PAINT_Handler @ 0x00621E90`.

`WM_PAINT_Handler @ 0x00621E90` checks the owner-draw dialog record mode:

- Mode `1`: right/left panel shell drawing.
- Mode `2`: `PUDLGBG*` dialog background drawing.
- Other modes: fallback `dbak6440.pcx` path.

The `0xCE` dialog is in the mode-2 allow-list per the prior paint-composition report. In mode `2`, `0x00621E90` selects the background asset and palette like this:

| Condition | SHP pointer | Palette function | Visible role |
|---|---|---|---|
| `FUN_0069BBE0() == 0` no-game / shell state | `DAT_00B0FC80` = `PUDLGBGN.SHP` | `FUN_0072B030()` = `DIALOGN.PAL` | Neutral shell modal |
| in-game and scenario side `0` | `DAT_00B0FC84` = `PUDLGBGA.SHP` | `FUN_0072AFF0()` = `DIALOG.PAL` | Allied-themed modal |
| in-game and scenario side `1` | `DAT_00B0FC88` = `PUDLGBGS.SHP` | `FUN_0072AFF0()` = `DIALOG.PAL` | Soviet-themed modal |
| in-game and other side | `DAT_00B0FC8C` = `PUDLGBGY.SHP` | `FUN_0072B010()` = `DIALOGY.PAL` | Yuri/third-side modal |

For offline Skirmish setup, the target is shell/no-game, so the native branch is the neutral `PUDLGBGN.SHP + DIALOGN.PAL` path.

## 5. What This Means For The Current Requested Soviet Theme

The user-requested Soviet background on this popup is a deliberate visual override.

It is not native parity for ordinary offline Skirmish Start validation. Native parity would use the neutral no-game background.

The Soviet asset itself is real and live in `gamemd.exe`, but under a different condition: the in-game side-1 mode-2 dialog branch. It is appropriate if the design goal is "make the Skirmish validation popup Soviet-themed," but it should not be documented as matching the native Skirmish validation popup.

## 6. Current Rust Implementation Status

Current Rust now has:

- Functional Start validation modal mapping in `src/app.rs`.
- `MNBTTN.SHP + MAINBTTN.PAL` OK button art in `src/render/skirmish_shell_chrome.rs` and `src/app_skirmish_shell_render`.
- A user-requested Soviet `PUDLGBGS.SHP + DIALOG.PAL` validation background in `src/render/skirmish_shell_chrome.rs` and `src/app_skirmish_shell_render/modals.rs`.

Current Rust delta vs native:

- Background theme is intentionally Soviet, while native ordinary shell validation is neutral.
- Final native screenshot pixels remain uncaptured.
- `0xCE` dialog-unit rects are modeled approximately through current Rust layout, but Win32 runtime DLU conversion has not been screenshot-verified.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Start command reaches validation branch | verified | `FUN_006ACEE0` decompile | none for ordinary failures |
| Start failure calls `FUN_005D3490` | verified | `FUN_006ACEE0` decompile | selected-mode lower branch remains conditional |
| `FUN_005D3490` generic helper behavior | verified | `FUN_005D3490` decompile; prior assembly report for hidden dialog id | none for ordinary two-text calls |
| `0xCE` template shape | verified | resource report [VALIDATION_MODAL_0X005D3490_DIALOG_TEMPLATE_CONTROL_RECTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/VALIDATION_MODAL_0X005D3490_DIALOG_TEMPLATE_CONTROL_RECTS_GHIDRA_REPORT.md) | final runtime pixels need capture |
| mode-2 parent background paint | verified | `FUN_00622B50`, `WM_PAINT_Handler @ 0x00621E90` | none for branch logic |
| neutral vs side-themed asset selection | verified | `WM_PAINT_Handler @ 0x00621E90`; prior pointer extraction | none for branch logic |
| exact DirectDraw RGB output | deferred | no runtime capture | capture native 800x600 screenshot |
| all dialogs using `PUDLGBG*` | not-touched | out of scope | separate global dialog inventory |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-01 - Is "Skirmish Start validation modal" a real native use case? -> Yes; Start command `0x617` failure branches call `FUN_005D3490`.` (evidence: `FUN_006ACEE0`)
- `[RESOLVED] OQ-02 - Which dialog resource is used for ordinary failures? -> `0xCE`, because optional third/fourth texts are absent.` (evidence: `FUN_005D3490`; prior assembly/resource report)
- `[RESOLVED] OQ-03 - Does the native ordinary popup have only one OK button? -> Yes; resource `0xCE` has controls `0x5B0` and `0x5AE` only.` (evidence: resource report)
- `[RESOLVED] OQ-04 - Is the mode-2 background path active for this dialog? -> Yes; `0xCE` is mode-2 and `WM_PAINT_Handler` draws `PUDLGBG*` for mode `2`.` (evidence: `FUN_00622B50`, `0x00621E90`, paint-composition report)
- `[RESOLVED] OQ-05 - Does native ordinary Skirmish validation use the Soviet background? -> No; it uses neutral no-game `PUDLGBGN.SHP + DIALOGN.PAL`.` (evidence: `0x00621E90` no-game branch)
- `[RESOLVED] OQ-06 - Is the Soviet background a real live asset? -> Yes, but conditional on in-game side `1` in mode-2 paint.` (evidence: `0x00621E90`, pointer `DAT_00B0FC88`)
- `[DEFERRED] OQ-07 - Exact native screenshot pixels for the modal.` (category: needs-runtime-debugger; reason: no retail screenshot/pixel capture in this recheck; next-step-if-pursued: capture native invalid-start popup at 800x600 and compare)
- `[DEFERRED] OQ-08 - Every non-Skirmish dialog using `PUDLGBG*`.` (category: out-of-scope; reason: this recheck only validates the Start failure use case; next-step-if-pursued: global dialog id/mode-2 inventory)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Invalid offline Skirmish Start shows a native message dialog, not just a log or silent failure | `FUN_006ACEE0 -> FUN_005D3490` | Rust has functional validation modal | `src/app.rs`, `src/skirmish_launch.rs` | keep modal on invalid Start and block parent input until dismissal | no-opponent Start click stays in shell and shows one modal | do not launch or silently ignore |
| Ordinary failures use template `0xCE`, body `0x5B0`, OK `0x5AE` | resource report, `FUN_005D3490` | Rust models one body and one OK, approximate pixel layout | `src/ui/skirmish_shell/layout.rs`, text renderer | keep single OK button; do not add Cancel/third button | capacity/no-opponent/same-team produces one actionable OK | do not add controls `2` or `0x5AF` for ordinary Start |
| Native parity background is neutral no-game `PUDLGBGN.SHP + DIALOGN.PAL` | `0x00621E90` no-game branch | Rust currently uses user-requested Soviet `PUDLGBGS.SHP + DIALOG.PAL` | `src/render/skirmish_shell_chrome.rs`, `src/app_skirmish_shell_render/modals.rs` | if parity is prioritized, switch back to neutral background | invalid Start in Skirmish setup shows neutral lightning background | do not document Soviet as native for this popup |
| Soviet `PUDLGBGS.SHP + DIALOG.PAL` is real but belongs to in-game side-1 mode-2 branch | `0x00621E90` side branch | Rust intentionally applies it to shell validation by request | same as above | acceptable only as intentional visual style override | popup uses Soviet emblem background | do not call this exact behavior parity-correct |

## Sources

- Ghidra read-only decompile: `FUN_006ACEE0`, `FUN_005D3490`, `FUN_00622650`, `FUN_00622B50`, `WM_PAINT_Handler @ 0x00621E90`.
- [docs/research/skirmish-ui/VALIDATION_MODAL_0X005D3490_DIALOG_TEMPLATE_CONTROL_RECTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/VALIDATION_MODAL_0X005D3490_DIALOG_TEMPLATE_CONTROL_RECTS_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/VALIDATION_MODAL_0X005D3490_PAINT_COMPOSITION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/VALIDATION_MODAL_0X005D3490_PAINT_COMPOSITION_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/SKIRMISH_START_VALIDATION_MODAL_CURRENT_CONTRACT_RECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_VALIDATION_MODAL_CURRENT_CONTRACT_RECHECK_GHIDRA_REPORT.md)
