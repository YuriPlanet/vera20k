# Shell First-Paint Slide: Generic Trigger (menu / single-player / skirmish / campaign) — Ghidra Report

**Date:** 2026-05-29
**Question:** Do main-menu→Skirmish AND main-menu→Campaign play a shell transition (slide) in gamemd YR? A prior session concluded "no slide on skirmish entry" but only inspected the final launcher `FUN_006AE2C0`.
**Verdict:** **YES — both slide.** The slide is **not** button-specific and **not** a menu→skirmish whole-screen transition. It is a **generic first-paint behavior of every shell dialog**: each shell dialog animates its own controls sliding into place the first time it paints. The prior conclusion was wrong because the slide is never called from `FUN_006AE2C0` (the launcher) — it is fired by the shared owner-draw subclass window-proc on the dialog's first `WM_PAINT`.
**Authority:** binary → Ghidra (live decompile, this session) → docs.
**Confidence:** High for the trigger chain, the allow-list membership, and the campaign dialog ID. Medium-low only for exact per-frame pixels (no runtime capture).

## 1. Resolving the unresolved caller `0x00612690`

`0x00612690` had no Ghidra function boundary, which blocked the prior session. Recovered this session: it is inside the **owner-draw shell subclass window procedure** that Ghidra had not carved.

- `create_function 0x00610CA0` → body `0x00610CA0..0x006128E1` (7265 bytes); `0x00612690` is inside. (Now `FUN_00610ca0`.)
- `get_xrefs_to 0x00610CA0` → `[DATA]` ref from `0x0060FF05` in `FUN_0060f9a0`: i.e. `SetWindowLongA(hwnd, GWL_WNDPROC=-4, 0x00610ca0)` installs `0x00610ca0` as the subclass proc.

This matches and resolves the [SHELL_MENU_TRANSITION_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_MENU_TRANSITION_SYSTEM_MODEL_SYNTHESIS.md) "Needs Re-Investigation #1/#2" and OQ-09 in [SKIRMISH_MAIN_MENU_TO_SHELL_TRANSITION_CALLER_FRAME_COMPOSITION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_MAIN_MENU_TO_SHELL_TRANSITION_CALLER_FRAME_COMPOSITION_GHIDRA_REPORT.md).

## 2. The slide trigger — first-paint of any shell dialog

Inside `FUN_00610ca0`, the slide is fired during `WM_PAINT` unwind (`decompile_function 0x00610ca0`):

```c
if (piVar24[0x7f] == 1) {                 // owner-draw record +0x1FC == 1
    piVar24[0x7f] = 2;
    LVar10 = GetWindowLongA(param_1,4);   // index 4 = DWLP_DLGPROC (non-zero => this IS a dialog)
    if ((LVar10 != 0) && (cVar6 = FUN_00608260(), cVar6 != '\0')) {  // <- call at 0x00612690
        piVar24[0x7f] = 3;                // slide done; never repeats
    }
    ...
}
```

`FUN_00608260` (verified `decompile_function 0x00608260`) plays the slide: gates on `FUN_0069bbe0()==0`, shell-hash active `DAT_00ac1b04!=0`, record byte `+0xC1!=0`, record `+0xB4==1`, `IsWindowVisible`; then plays `ShellButtonSlideSound`, disables children, and calls `FUN_006071e0` with **`DL=1`** (assembly `0x0060833F MOV DL,1`), the 30 ms-per-frame slide that ends by broadcasting `0x4EC` → child statics receive `0x4EE` (text reveal).

### How `+0x1FC` reaches 1 (the linchpin)

Same function, the `WM_PAINT` body. When painting the enclosing **dialog** (`pHVar8 == param_1`, where `pHVar8` is found by walking parents until `GetWindowLongA(_,4)!=0`):

```c
iStack_330 = piVar24[0x7f];     // current state of the dialog's record
if (0 < iStack_330) goto ...;   // already 1/2/3 -> no change
iStack_330 = 1;                 // FIRST paint: stage a slide
... CallWindowProcA(original_proc, ...) ...
piVar24[0x7f] = iStack_330;     // writes +0x1FC = 1
```

So on a dialog's **first** `WM_PAINT`, `+0x1FC` goes `0→1`; the same paint's unwind then sees `==1`, fires `FUN_00608260` (the slide), and sets `+0x1FC=3` so it never repeats. There is no dedicated "request slide" writer — the staging is intrinsic to first paint.

`GetWindowLongA(hwnd, 4)` is **`DWLP_DLGPROC`** (Win32: `DWLP_MSGRESULT=0`, `DWLP_DLGPROC=4`, `DWLP_USER=8`). It is non-zero for a dialog and zero for plain child windows (buttons/statics). So the parent-walk simply locates "the enclosing shell dialog," and the slide fires on that dialog. No game-set marker required — being a dialog is the marker.

## 3. Which dialogs are eligible — the allow-list in `FUN_0060c540`

`FUN_00608260`'s gate needs record `+0xB4==1` and `+0xC1!=0`. The only writer is `FUN_0060c540` (`get_function_callers 0x0060C540` → only `FUN_00622820` and `FUN_00622b50`, the shared shell init). It sets those two markers **only if the dialog's resource ID (stored at record `node+0x70`, written by `FUN_0060d2c0`) is in a hardcoded allow-list** (`decompile_function 0x0060C540`):

```c
piVar3[0x2d] = 1;                      // +0xB4 = 1
*(undefined1 *)((int)piVar3 + 0xc1) = 1;  // +0xC1 = 1
```

Allow-list members relevant here (verified present in the decompile's comparison chain): **`0xE2`** (main menu), **`0x6B`**, **`0x94`** (campaign — see §4), **`0x100`** (single-player shell), **`0x101`**, **`0x102`** (skirmish), **`0x129`**, plus ~45 other menu/setup dialog IDs. Dialogs not in the list (e.g. transient message boxes) do not get the markers and do not slide.

## 4. The two routes (full chains, not just the launcher)

Shared factory: every shell dialog is created by `FUN_00622650` (CreateDialogIndirectParamA) and inited by the common shell proc `FUN_00622b50`, which calls `FUN_00622820` → `FUN_0060f9a0` (installs the `0x00610ca0` subclass on the dialog + children, allocates the 0x208-byte record) and `FUN_0060c540` (sets slide markers if allow-listed). This is identical for all dialogs.

**Skirmish:** `Main_Game` (`0x0052D9A0`) case 1 (main-menu Single Player → `1`) → `FUN_0060d380(1)` opens dialog **`0x100`** (proc `0x0052d640`, asm `0x0052DD39`). `0x100` Skirmish button returns **`0x0B`** → case `0x0b` sets `g_GameMode=5` → falls into `case 0x10/0x11` → `switch(g_GameMode) case 5: FUN_006ae2c0()` opens dialog **`0x102`**. `FUN_006AE2C0` itself never calls the slide — it only `FUN_00622650(0)` + `FUN_00622800()` (ShowWindow/SetForeground). The slide fires generically on `0x102`'s first paint via §2.

**Campaign:** `0x100` "New Campaign" (control `0x688`) returns **`8`** → `Main_Game` case 8 (asm `0x0052DF05`). It opens its own shell dialog: `0x0052DF4D MOV ECX,0x94` (dialog ID) / `0x0052DF48 MOV EDX,0x52ec00` (DLGPROC) / `0x0052DF65 CALL 0x00622650`. So campaign shows dialog **`0x94`** (then reads difficulty `DAT_00a8eb64` and sets `g_GameMode=0` for the scenario). `0x94` is in the allow-list → it slides on first paint.

Other legs use the same pattern: case-1's `0x100`, `0x0052DD93`'s `0x101`, `0x0052DE72`'s `0x129` — all allow-listed, all slide.

## 5. Answer to the question

- **Skirmish (`0x102`) slides on entry.** Prior "no slide" was wrong: it inspected only `FUN_006AE2C0`, but the slide is fired by the shared subclass proc on first paint, and `0x102` is allow-listed.
- **Campaign (`0x94`) slides on entry**, same mechanism.
- **Main menu (`0xE2`) and the intermediate single-player shell (`0x100`) also slide** — it is generic to every allow-listed shell dialog, not a property of the menu→skirmish edge.

### Participating controls

`FUN_006071e0` enumerates the dialog's visible, enabled, owner-draw children (`FUN_00608CD0` / `FUN_00609730` filters per [SKIRMISH_FUN_006071E0_SHELL_TRANSITION_REDRAW_PATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_FUN_006071E0_SHELL_TRANSITION_REDRAW_PATH_GHIDRA_REPORT.md)) and slides them in over `max(schedule)+6` ticks at 30 ms each, plus optional right-panel/radar groups gated by record `+0xD5/+0xD6/+0xD7/+0xD8` (set per-dialog-ID in `FUN_00622820`, asm `0x006228E1..0x00622A12`). For `0x102` these include the right-panel groups; for source dialogs like `0x100` they are cleared.

## 6. Implication for current Rust (no code changed)

The committed slide wave is wired to fire on **skirmish-shell entry**. That leg is not "wrong" (skirmish does slide), but the model is off in two ways that are DRIFT until reconciled:

1. **Character:** native is a per-dialog "controls slide into their final positions on first paint," not a whole-screen menu→skirmish crossfade/slide. A whole-screen compositor between two screens is a different observable effect.
2. **Coverage:** native slides every allow-listed shell dialog. Current Rust collapses the intermediate `0x100` single-player shell (per [SKIRMISH_MAIN_MENU_TO_SHELL_TRANSITION_CALLER_FRAME_COMPOSITION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_MAIN_MENU_TO_SHELL_TRANSITION_CALLER_FRAME_COMPOSITION_GHIDRA_REPORT.md) §7) and has no campaign `0x94` dialog, so the main-menu `0xE2`, single-player `0x100`, and campaign `0x94` first-paint slides are missing.

Stock `ShellButtonSlideSound=` is empty in `rules.ini`/`rulesmd.ini`, so the slide is silent in stock YR; the animation itself is still played.

## 7. Evidence log

- Recovered/decompiled this session: `FUN_00610ca0` (created at `0x00610CA0`), `FUN_00608260`, `FUN_0069bbe0`, `FUN_00774070`, `FUN_006AE2C0`, `FUN_0060d380`, `FUN_00622800`, `FUN_00622650`, `FUN_0060f9a0`, `FUN_0060c540`, `FUN_0060d2c0`, `FUN_0060d450`, `Main_Game @ 0x0052D9A0` (decompile + full disassemble), `FUN_00622820` (disasm), `FUN_00622b50` (disasm), `FUN_004a3b40`.
- Xrefs: `get_xrefs_to 0x00610CA0`; `get_function_callers 0x0060F9A0` / `0x0060C540`; `get_xrefs_to 0x00622650` (Main_Game call site `0x0052DF65`).
- Key asm: slide call `0x00612690`; `DWLP_DLGPROC` read `GetWindowLongA(_,4)`; campaign dialog id `0x0052DF4D MOV ECX,0x94`; `0x100` route `0x0052DD39`.
- Win32: `DWLP_DLGPROC == 4`.
