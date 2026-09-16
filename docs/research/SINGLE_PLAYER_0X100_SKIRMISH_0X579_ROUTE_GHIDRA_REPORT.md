# Single Player 0x100 Skirmish 0x579 Route - Ghidra Report

**Date:** 2026-05-27  
**Investigation mode:** exhaustive-slice for dialog `0x100` Skirmish control `0x579`; coverage-map only for unrelated generic shell-transition caller `0x00612690`.  
**Target question:** In active Yuri's Revenge, what exact command/control route does the Single Player shell Skirmish button use, what result does it write, what click/pressed-art ordering is visible around that route, and does this route directly or indirectly call `FUN_00608260` / `FUN_006071E0`?  
**Non-goals:** full Campaign `0x94`, Load Game picker, per-pixel final layout beyond `0x100` dialog template, full taxonomy of `FUN_00608260` callers, and Rust code changes.  
**Evidence needed to mark COMPLETE:** decompile or disassembly for `Main_Game -> FUN_0060D380`, `FUN_0060D380` result-pointer loop, dialog-proc `0x0052D640` `WM_COMMAND`, dialog resource `0x100` control ids/styles, owner-draw click-sound/pressed-art callback evidence, and xref/caller evidence for `FUN_00608260`.  
**Stop conditions:** write only this report and the swarm claim row; leave Rust, INI, assets, and other docs untouched; keep Ghidra read-only; put unrelated transition-owner gaps into Remaining Uncertainty.

## Summary

Active in YR: Yes. The standard Single Player submenu is dialog resource `0x100`, created by `Main_Game` case `1` through `FUN_0060D380(1)`. Its Skirmish button is visible control `0x579` with title `GUI:Skirmish`, style `0x5000000B`, and owner-draw Button class. When the default Button control emits parent `WM_COMMAND`, dialog proc `0x0052D640` masks `LOWORD(wParam)`, matches `0x579`, and writes route result `0x0B` directly through the result pointer stored at `GetWindowLongA(hwnd, 8)`.

The route result write itself does not call `FUN_00608260` or `FUN_006071E0`. The click sound and pressed art belong to the child owner-draw button subclass before the parent command handler runs: mouse down/double-click can play `GUIMainButtonSound`, paint chooses released/pressed PCX/SHP art and can play `GenericClick` on a released-to-pressed paint transition, then default Button processing later produces the parent `WM_COMMAND`.

The separate `0x00612690 -> FUN_00608260 -> FUN_006071E0(DL=1)` transition caller remains real and active conditionally, but this slice found no binary evidence tying it to the `0x0052D640` Skirmish result write. Treat that as a sibling transition-owner question, not a prerequisite for implementing `0x579 -> 0x0B`.

## Verified Route

| Fact | Active in YR | Evidence |
|---|---|---|
| `Main_Game` case `1` creates Single Player dialog `0x100` with proc `0x0052D640` and stack arg `1`. | Yes | Ghidra `disassemble_function 0052d9a0`: `0x0052DD39 PUSH 1`, `0x0052DD3B MOV EDX,0x52D640`, `0x0052DD40 MOV ECX,0x100`, `0x0052DD4B CALL 0x0060D380`. |
| `FUN_0060D380` stores `&local_4` at window long offset `8`, pumps until `local_4 != 0`, destroys the dialog, and returns `local_4`. | Yes | Ghidra `decompile_function 0060d380`: `SetWindowLongA(hWnd,8,&local_4)`, loop on `local_4`, `FUN_00622720()`, `iVar2 = local_4`. |
| `FUN_00622650` uses caller `ECX`/`EDX` as dialog resource id/proc, creates via `CreateDialogIndirectParamA`, then records current HWND/resource. | Yes | Ghidra `decompile_function 00622650`: `local_8[0] = param_1`, `CreateDialogIndirectParamA(..., param_2, local_8)`, stores `DAT_00B72F44/48`. |
| Dialog proc `0x0052D640` delegates to common shell proc first; only if it returns zero does local message handling continue. | Yes | Raw PE disassembly: `0x0052D656..0x0052D663` pushes args, calls `0x00622B50`, `TEST EAX,EAX`, `JNE 0x52D77F`. |
| `WM_COMMAND` dispatch uses `LOWORD(wParam)`. | Yes | Raw PE disassembly: `0x0052D6DF MOV ECX, EBX`; `0x0052D6E1 AND ECX,0xFFFF`. |
| Skirmish control `0x579` writes result `0x0B`. | Yes | Raw PE disassembly: `0x0052D6F1 CMP ECX,0x579`; `0x0052D6F7 JE 0x52D712`; `0x0052D713 MOV [EAX],0x0B`; `RET 0x10` at `0x0052D720`. |
| Main Menu/back control `0x686` writes result `0x12`. | Yes | Raw PE disassembly: `0x0052D6F9 CMP ECX,0x686`; fall-through `0x0052D702 MOV [EAX],0x12`. |
| New Campaign `0x688` writes `8`; Load Saved Game `0x689` writes `9`; proc-only `0x68A` writes `0x0A`. | Yes for visible `0x688/0x689`; Conditional/unknown visible source for `0x68A` | Raw PE disassembly `0x0052D723..0x0052D75E`; resource `0x100` has no child `0x68A`. |

## Dialog 0x100 Resource

Active in YR: Yes. Parsed from retail `gamemd.exe` `RT_DIALOG` id `0x100`, language `1033`, `DIALOGEX` size `500`.

| Index | ID | Class | Title | Style | DLU rect | Active in YR |
|---:|---:|---|---|---:|---|---|
| 0 | `0x694` | Static `#130` | `GUI:SinglePlayerMenu` | `0x50020001` | `(425,1,108,10)` | Yes |
| 1 | `0x688` | Button `#128` | `GUI:NewCampaign` | `0x5000000B` | `(425,122,108,23)` | Yes |
| 2 | `0x689` | Button `#128` | `GUI:LoadSavedGame` | `0x5000000B` | `(425,149,108,23)` | Yes, enabled conditionally |
| 3 | `0x579` | Button `#128` | `GUI:Skirmish` | `0x5000000B` | `(425,176,108,23)` | Yes |
| 4 | `0x686` | Button `#128` | `GUI:MainMenu` | `0x5000000B` | `(425,346,108,23)` | Yes |
| 5 | `0x695` | Static `#130` | `GUI:Blank` | `0x50000200` | `(2,355,303,12)` | Yes |
| 6 | `0x71C` | Static `#130` | none | `0x50000007`, ex `0x20` | `(446,29,61,33)` | Yes |
| 7 | `0x71A` | Static `#130` | none | `0x50000000` | `(0,0,304,266)` | Yes |

Tiny details: the template is `DIALOGEX`, style `0x40000040`, rect `(0,0,533,369)`, font `MS Sans Serif`, point size `8`, charset `1`; all four visible command buttons share low style bits `(style & 0x0B) == 0x0B`, which routes through the shared shell owner-draw Button subclass.

## Load Saved Game Enable Gate

Active in YR: Yes. Message `0x497` in `0x0052D640` gets child `0x689`, constructs/uses the load-options scanner, and calls `EnableWindow(load_button, scan_result)`.

Evidence:

- `0x0052D683 CMP EDI,0x497`, then `0x0052D68F PUSH 0x689`, `GetDlgItem`, `0x0052D6B0 CALL 0x00559C20`, and `0x0052D6CE CALL [0x007E14A0]` (`EnableWindow` import).
- `FUN_00559C20` decompile scans `param_1[2]` with `FindFirstFileA`, skips `dwFileAttributes & 0x116`, excludes `SAVEGAME.NET`, calls a validation vfunc at `+0x10`, and returns `1` on the first valid non-network save.

## Click Sound, Pressed Art, And Command Ordering

| Step | Behavior | Active in YR | Evidence |
|---:|---|---|---|
| 1 | Common shell init routes Button class controls with `(style & 0x0B) == 0x0B` to `OwnerDraw_Button_00612B70`. | Yes for dialog `0x100` buttons because their styles are `0x5000000B`. | Resource parse plus `FUN_0060F9A0` decompile: Button branch checks `(bVar2 & 0xB) == 0xB`, sets callback `OwnerDraw_Button_00612B70`. |
| 2 | Mouse down/double-click on an unsuppressed owner-draw button can play `GUIMainButtonSound`. | Yes, conditional on owner record `+0xBC == 0`. | `OwnerDraw_Button_00612B70` decompile: message `0x201`/`0x203`, if suppress byte is zero calls `VocClass__PlayAtPos(1.0,0)` using Rules audio slot for `GUIMainButtonSound`; INI stock `GUIMainButtonSound=MenuClick`. |
| 3 | Pressed/released visual state is drawn during button `WM_PAINT`, before/around parent command delivery. | Yes. | `OwnerDraw_Button_00612B70` decompile: default PCX path derives state char `'u'`/`'d'`, loads `b%c%c_li%d.pcx`, `b%c%c_mi%d.pcx`, `b%c%c_ri%d.pcx`, or SHP frame branch for transition-style buttons. |
| 4 | Paint transition from last global `'u'` to current `'d'` can play `GenericClick`, not `ShellButtonSlideSound`. | Yes, conditional on enabled style and paint timing. | `OwnerDraw_Button_00612B70` decompile and sibling sound report: GenericClick uses Rules `+0x70C`; stock `GenericClick=MenuClick` in rules docs, while `ShellButtonSlideSound=` is empty in shipped INI. |
| 5 | Parent `0x0052D640` writes `0x0B` only after the child Button has emitted `WM_COMMAND`. | Yes. | Win32 ordering plus binary split: click/paint sites are in child subclass; parent `WM_COMMAND` handler is the direct result write at `0x0052D6DF..0x0052D720`. |

## Transition Helper Relationship

Active in YR: Conditional. The generic slide-in helper exists and is live in shell UI paths, but this slot does not prove it is triggered by Single Player `0x579`.

Verified negatives for this route:

- Dialog proc `0x0052D640..0x0052D785` contains no `CALL 0x00608260` and no `CALL 0x006071E0`; the Skirmish branch is a direct `MOV [EAX],0x0B` and return.
- `FUN_0060D380` does not call `FUN_00608260`; it creates/shows/pumps/destroys the dialog.
- `FUN_006AE2C0` entry to Skirmish setup was checked in sibling docs and has no direct `FUN_00608260`/`FUN_006071E0` call on entry.
- Ghidra xref context for `FUN_00608260` shows the known direct callers `0x005E6B49` and `0x00612690`; neither is inside `0x0052D640`.

Verified positive transition facts, but not tied to this command result:

- `FUN_00608260` gates on shell record liveness, byte `+0xC1`, paint mode `+0xB4 == 1`, and `IsWindowVisible(hwnd)`, then enumerates children, calls `FUN_006071E0` with `DL=1`, restores children/enabled state, invalidates, and returns `1`. Evidence: Ghidra `disassemble_function 00608260`, especially `0x0060833F MOV DL,0x1`, `0x00608343 CALL 0x006071E0`.
- `0x00612690` is a real caller context: it calls a helper with arg `4`, then `CALL 0x00608260`, then on success writes `[EDI+0x1FC]=3`. Evidence: Ghidra assembly context for xref source `0x00612690`. Its owning high-level function is not recovered in this slot.
- Common shell paint can call `FUN_006071E0` in zero mode, but that is a deferred redraw path, not the direct Skirmish command result. Evidence: `FUN_00622B50` decompile plus sibling report for `0x00622CA6 XOR DL,DL; CALL 0x006071E0`.

## Current Rust Touchpoints

Active in YR: not applicable; this is current Rust implementation status for handoff only.

- `src/ui/single_player_shell/state.rs` already has `NewCampaign0x688`, `LoadSavedGame0x689`, `Skirmish0x579`, `MainMenu0x686`, and return codes `8`, `9`, `0x0B`, `0x12`.
- `src/ui/single_player_shell/layout.rs` already models dialog `0x100` control geometry and right-panel shell placement.
- `src/app.rs` now routes `MainMenuShellAction::SinglePlayer` to `open_single_player_shell`, not directly to Skirmish. This makes older reports saying there is no Single Player shell module stale.
- `src/app.rs` handles `SinglePlayerShellAction::Skirmish` by entering native Skirmish from the Single Player shell and setting `skirmish_shell_return_to_single_player_shell = true`.
- Current remaining parity risk is not the numeric `0x579 -> 0x0B` result identity; it is exact surrounding transition/reveal behavior and exact owner of `0x00612690`.

## Implementation Handoff

| Verified behavior | Rust delta | Affected surface | Acceptance scenario | Proposed test | Risk |
|---|---|---|---|---|---|
| `0x579` in dialog `0x100` writes result `0x0B` directly through the result pointer. | Preserve existing `Skirmish0x579 -> Some(0x0B)` and keep Skirmish launch behind the Single Player shell action. | `src/ui/single_player_shell/state.rs`, `src/app.rs` shell route dispatcher. | Main menu Single Player opens Single Player shell; clicking Skirmish from that shell enters Skirmish setup. | `single_player_shell_skirmish_0x579_emits_route_0x0b_before_skirmish_setup` | Low; mostly implemented, but route tests should protect against shortcut regression. |
| Load Saved Game `0x689` is enabled only if the save scanner finds a valid non-network save. | Keep `load_saved_game_enabled` tied to save-list scan; do not emit result `9` when disabled. | `src/ui/single_player_shell/state.rs`, app save-list cache. | Empty save directory disables Load Saved Game; valid save enables it and action opens load panel. | `single_player_shell_load_saved_game_disabled_without_valid_saves` | Medium; native scanner filters attributes and `SAVEGAME.NET`, Rust cache semantics may differ. |
| Owner-draw button sound/pressed art happens before parent `WM_COMMAND`; `ShellButtonSlideSound` is not the click sound. | If adding transition polish, keep press sound using `GUIMainButtonSound`/`GenericClick` semantics separate from slide helper audio. | app input/render sound paths, single-player shell render. | Holding Skirmish shows pressed art and plays click before route transition/completion. | `single_player_shell_button_press_sound_precedes_route_action` | Medium; paint-time `GenericClick` timing is Windows-message sensitive. |
| `FUN_00608260` is not required to produce route `0x0B`; its `0x00612690` caller is separate unresolved transition-owner evidence. | Do not block or rewire the result route on `FUN_00608260`; only attach native slide-in after sibling slot proves the exact owner/flags for this action. | `src/app_shell_transition.rs`, shell transition/reveal code. | Route result works without slide helper; any visual bridge remains explicitly DRIFT until caller owner is proven. | `single_player_shell_skirmish_route_does_not_require_transition_helper` | High if a non-native bridge is presented as parity. |

## Negative Facts / Do Not Do

- Do not map main-menu `Single Player` button `0x683` directly to Skirmish setup as a parity claim. Active in YR: No. Evidence: `0x0052DD39..0x0052DD4B` enters dialog `0x100` first.
- Do not invent a visible `0x68A` button on dialog `0x100`. Active in YR: No visible resource evidence. Evidence: resource `0x100` has eight children and none is `0x68A`; the proc-only branch writes `0x0A`.
- Do not use `ShellButtonSlideSound` for ordinary `0x579` click feedback. Active in YR for this click route: No evidence. Evidence: owner-draw click uses button sound paths; shipped `ShellButtonSlideSound=` is empty in `rules.ini`/`rulesmd.ini`.
- Do not claim `0x0052D640` calls `FUN_00608260` or `FUN_006071E0`. Active in YR: No for this proc. Evidence: raw PE disassembly `0x0052D640..0x0052D785`.
- Do not treat `LoadSavedGame` as always enabled. Active in YR: No. Evidence: message `0x497` scans saves and calls `EnableWindow(0x689, result)`.

## Remaining Uncertainty

- The exact high-level owner of `0x00612690 -> FUN_00608260` remains unresolved here; sibling slot `SHELL_TRANSITION_CALLER_00612690_OWNER` should own that.
- Whether a runtime click on Single Player `0x100` Skirmish reaches `0x00612690` indirectly through a state machine before/after `WM_COMMAND` is not proven by this slot. Static evidence only proves the dialog proc/result write and child owner-draw callback do not directly call `FUN_00608260`.
- Exact runtime audibility of both `GUIMainButtonSound` and paint-time `GenericClick` on one held click remains message-timing dependent.
- Exact post-`0x0B` first-frame transition/reveal flags for Skirmish setup are delegated to slot 3.

## Stale-Doc Replacement Wording

Replace older current-Rust wording that says "Rust has no implemented dialog `0x100` Single Player shell" with:

> Current Rust now implements a first-class `single_player_shell` UI surface with dialog `0x100` control identities and return codes (`0x688 -> 8`, `0x689 -> 9`, `0x579 -> 0x0B`, `0x686 -> 0x12`). Remaining parity work is focused on exact surrounding owner-draw sound/paint timing, native transition/reveal caller ownership, and full pixel/layout verification, not on the absence of the intermediate shell itself.

## Sources

- Ghidra read-only decompile/disassembly: `Main_Game @ 0x0052D9A0`, `FUN_0060D380 @ 0x0060D380`, `FUN_00622650 @ 0x00622650`, `FUN_00622B50 @ 0x00622B50`, `FUN_00608070 @ 0x00608070`, `FUN_00608260 @ 0x00608260`, `FUN_0060F9A0 @ 0x0060F9A0`, `OwnerDraw_Button_00612B70 @ 0x00612B70`, `FUN_00559C20 @ 0x00559C20`.
- Raw PE disassembly with Capstone over retail `gamemd.exe`: `0x0052D640..0x0052D785`.
- PE `RT_DIALOG` extraction from retail `gamemd.exe`: dialog resource `0x100`, language `1033`.
- Ghidra xref assembly context: `0x005E6B49` and `0x00612690` callers of `FUN_00608260`.
- Prior docs reconciled: [docs/research/skirmish-ui/SKIRMISH_FUN_0060D380_SINGLE_PLAYER_0X100_TO_0X0B_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_FUN_0060D380_SINGLE_PLAYER_0X100_TO_0X0B_GHIDRA_REPORT.md), [docs/research/SINGLE_PLAYER_SUBMENU_DIALOG_CASE1_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SINGLE_PLAYER_SUBMENU_DIALOG_CASE1_GHIDRA_REPORT.md), [docs/research/SHELL_TRANSITION_ON_MAIN_MENU_CLICK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_TRANSITION_ON_MAIN_MENU_CLICK_GHIDRA_REPORT.md), [docs/research/SHELL_BUTTON_SLIDE_SOUND_CALL_SITE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_BUTTON_SLIDE_SOUND_CALL_SITE_GHIDRA_REPORT.md), [docs/research/skirmish-ui/SKIRMISH_OWNERDRAW_PUSH_BUTTON_SOUNDS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_OWNERDRAW_PUSH_BUTTON_SOUNDS_GHIDRA_REPORT.md).
- INI checked: `ini/rules.ini`, `ini/rulesmd.ini`.
- Current Rust scan: `src/ui/single_player_shell/state.rs`, `src/ui/single_player_shell/layout.rs`, `src/app.rs`, `src/app_single_player_shell_render.rs`.

**Status:** COMPLETE for the `0x100`/`0x579` command route and direct transition-helper negative; PARTIAL only for the broader unrelated `0x00612690` transition-owner taxonomy, which is outside this slot.
