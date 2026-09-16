# Skirmish Shell Input/Focus/Message Broad Recheck - Ghidra Research Report

**Address(es):** `0x006AE2C0`, `0x006AE3F0`, `0x006AE6E0`, `0x006ACEE0`, `0x00614190`, `0x00622B50`, `0x006040B0`, `0x00612B70`, `0x00617250`, `0x0060D540`, `0x0061D950`, `0x0061C690`  
**Investigation Mode:** coverage-map  
**Claimed Scope:** standard offline Yuri's Revenge Skirmish dialog `0x102` input/focus/message behavior beyond mouse-only owner-draw painting: player-name edit `0x6A0`, focus messages `0x4B0/0x4AF`, verified keyboard behavior in the shell path, modal blocking/dismissal, status strip `0x695`, button/check/dropdown/trackbar sound boundaries, and current Rust backlog.  
**Non-Scope:** full visual painting, full `0x6B` Choose Map visual layout, online lobby/host/guest shells, full common-shell invalidation timing, runtime audio capture, and unverified keyboard shortcuts.  
**Confidence:** High for active `0x102` reachability, player-name edit setup/readback/focus, status strip source, scoped sound call sites, and current Rust gaps; Medium for global modal/keyboard taxonomy because this pass classifies broad behavior and defers unverified shortcuts.  
**Active in YR:** Yes for the verified standard offline `0x102` paths below. Conditional or No is stated per finding.

## 0. Investigation Gate

- Target question: What active standard offline Skirmish shell input/focus/message behavior remains implementation-relevant beyond owner-draw mouse widgets, and what should Rust implement or avoid?
- Non-goals: Do not re-cover full paint composition, pixel geometry except caret/status needs, full Choose Map modal visuals, online dialogs, or broad shell keyboard accelerators without proof.
- Evidence needed to mark COMPLETE: prior-report reconciliation, Ghidra spot-checks for the active dialog, edit/focus/status/sound paths, assembly context for handoff-critical claims, current Rust scan, and a ledger with every scoped area resolved or explicitly deferred.
- Stop conditions: read-only Ghidra only; write only this report and [.swarm-claims.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/.swarm-claims.md); mark unverified keyboard/modal behavior deferred instead of inventing parity rules.

## 1. Overview

The standard offline Skirmish setup screen is a live Win32 shell dialog (`0x102`) created by `FUN_006AE2C0` with proc `0x006AE3F0`. Input behavior is not limited to custom-rendered mouse controls: the local player name is a real editable child (`0x6A0`) with focus restoration messages, the bottom-left status/help strip (`0x695`) is updated from hover hit testing, and several owner-draw controls play specific UI sounds at specific input boundaries.

The most important current Rust backlog is still input/state, not raw rendering: Rust has a `player_name` field for launch packing, but renders the literal `"Player"` and has no focused text-edit/caret route or `0x695` hover-help state. Rust currently also closes the native Skirmish shell on Escape; this recheck found no scoped binary proof for Escape-to-Back in the `0x102` proc, so that behavior should be treated as unverified.

## 2. Key Controls, Messages, And State

| Item | Verified behavior | Active in YR | Evidence |
|---|---|---|---|
| Dialog `0x102` | Standard offline Skirmish shell proc is `0x006AE3F0`; launcher loops until result `0x617` or `0x5C0` | Yes | `0x006AE31C..0x006AE328`; decompile `0x006AE2C0` |
| Edit `0x6A0` | Real ordinary `Edit`, initialized from `DAT_00A8B380`, max text `0x13`, read back with `0x4B3` into a 20-wide-char buffer | Yes | `0x006AE6F2..0x006AE735`; `0x006AD375..0x006AD39F`; prior [SKIRMISH_PLAYER_NAME_EDIT_CONTROL_0X6A0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PLAYER_NAME_EDIT_CONTROL_0X6A0_GHIDRA_REPORT.md) |
| Focus messages `0x4B0/0x4AF` | `WM_SETFOCUS` selects all and posts `0x4B0`; later `0x4AF` restores focus/style without recursive repost | Yes | `OwnerDraw_Edit_00614190`; assembly `0x006143BE..0x006145AD`; prior focus report |
| Edit Tab | `WM_CHAR` Tab (`0x09`) calls `GetNextDlgTabItem` and `SetFocus`; key down/up Tab is consumed before native handling | Yes, when focus is in old Edit | `OwnerDraw_Edit_00614190` decompile around `WM_CHAR 0x102`; prior edit report |
| Edit Enter | `WM_CHAR` Enter (`0x0D`) only has special handling when style bit `0x4` is set; otherwise falls through to native edit proc | Conditional | `OwnerDraw_Edit_00614190` decompile `WM_CHAR` branch |
| Escape | No scoped `0x102` dialog-proc or old-Edit evidence found for Escape closing the shell | No verified standard path in this slice | `0x006AE3F0` decompile handles `0x497`, `WM_PAINT`, `WM_COMMAND`, `0x4E9`; app Rust handles Escape independently |
| Status child `0x695` | Visible bottom-left help/status static; text is updated on common parent `WM_NCHITTEST` via `0x4B2` | Yes | `FUN_00622B50`; assembly `0x00622CCB..0x00622E83`; prior status report |
| Status fallback mapping | `FUN_006040B0` maps dialog `0x102` child IDs to `STT:Skirmish*` keys; no `0x695` self-mapping | Yes | decompile `0x006040B0` |
| Choose Map modal blocking | Parent hides and opens modal; current Rust blocks mouse while modal state is open, but native modal transaction/visual shell remains broader work | Yes for native modal; partial/current in Rust | [SKIRMISH_CHOOSE_MAP_ACCEPT_CANCEL_SIDE_EFFECTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_ACCEPT_CANCEL_SIDE_EFFECTS_GHIDRA_REPORT.md); `src/app.rs:758..856` |

## 3. Core Logic

### 3.1 Active Dialog Path

Active in YR: Yes. `FUN_006AE2C0` loads proc address `0x006AE3F0`, dialog id `0x102`, and calls `0x00622650`. It then pumps until the dialog result dword becomes `0x617` (Start) or `0x5C0` (Back). This is the standard offline Skirmish path, not TS legacy.

Evidence:

- `0x006AE31C`: `MOV EDX,0x6ae3f0`
- `0x006AE321`: `MOV ECX,0x102`
- `0x006AE328`: `CALL 0x00622650`
- decompile `0x006AE2C0`: loop condition checks `local_4 != 0x617` and `local_4 != 0x5c0`

### 3.2 Player Name Edit `0x6A0`

Active in YR: Yes. `FUN_006AE6E0` gets child `0x6A0`, sends `EM_SETLIMITTEXT (0xC5)` with `wParam=0x13`, converts `DAT_00A8B380` to wide text, sends custom text message `0x4B2`, then sends `0x4D1`.

Start reads the same child before packing the local session: `FUN_006ACEE0` gets `0x6A0`, sends `0x4B3` with `wParam=0x14` and a stack wide buffer, and copies the result back to the player-name global. The old edit callback's `0x4B3` branch force-terminates the final wide code unit at index `wParam - 1`.

Handoff consequence: the visible field must be an editable 19-character-capped name with focus/caret/selection, not a static label.

Current Rust:

- `src/ui/skirmish_shell/state.rs:544` has `player_name: String`; default is `"Player"` at line `578`; launch packing uses it at line `1429`.
- `src/app_skirmish_shell_render.rs:1762..1768` still renders literal `"Player"` into `layout.player_name`.
- No `rg` hit showed a Skirmish text-input, caret, selection, or focused-edit route.

### 3.3 Focus Messages `0x4B0` And `0x4AF`

Active in YR: Yes for old Edit `0x6A0`. `OwnerDraw_Edit_00614190` has an entry guard before message-specific handling: if the focused HWND is this edit and the restore-in-progress flag is zero, it marks restore-needed and temporarily moves focus to `g_hWnd`.

On `WM_SETFOCUS (7)`, old Edit sends native `EM_SETSEL(-1,-1)`, then posts `0x4B0` to itself unless restore-in-progress is already set. `0x4B0` has no dedicated old-Edit switch body; delivery triggers the entry guard. On `0x4AF`, old Edit sets restore-in-progress, calls `SetFocus(edit)` if restore-needed was set, clears restore-needed, and restores `WS_VISIBLE` if the style latch was set.

Assembly spot-check:

- `0x006143C5..0x006143CF`: pushes `-1,-1,0xB1,hwnd` for `EM_SETSEL`
- `0x006143D5..0x006143DD`: checks restore-in-progress before the post
- prior report pins `PostMessageA(hwnd,0x4B0,0,0)` in this branch
- `0x00614558`: compare message `0x4AF`
- `0x00614566..0x0061457C`: set restore-in-progress and call `SetFocus`

Handoff consequence: Rust does not need literal custom Win32 messages, but does need stable edit focus and input routing across redraws and modal/status updates.

### 3.4 Keyboard Boundaries

Active in YR: Yes, but only for the scoped old-Edit behavior.

- Tab in the player-name edit: verified. `WM_CHAR (0x102)` with `wParam=9` calls `GetParent`, `GetNextDlgTabItem`, then `SetFocus`, returning handled. Key down/up Tab (`WM_KEYDOWN/WM_KEYUP` with `wParam=9`) is consumed before native handling.
- Enter in the player-name edit: conditional. `WM_CHAR` Enter (`0x0D`) sends a native text-change-style parent command only when style bit `0x4` is set; otherwise it falls through to native edit behavior. This recheck did not prove a global Start-on-Enter shell accelerator.
- Escape: not verified for standard `0x102` Skirmish. `0x006AE3F0` does not handle `WM_KEYDOWN`, `WM_CHAR`, or `VK_ESCAPE` directly. Do not convert Rust's current Escape-close behavior into a parity claim without a focused dialog-manager/runtime trace.

### 3.5 Status Strip `0x695`

Active in YR: Yes. The common parent handler `FUN_00622B50` handles `WM_NCHITTEST (0x84)` by getting child `0x695`, finding the child under the cursor, trying child `0x4E8`, parent `0x4E9`, and then `FUN_006040B0` fallback key mapping. It converts the resulting holder to wide text and sends `SendMessageA(status_hwnd,0x4B2,0,wide_text)`.

Assembly spot-check:

- `0x00622CCB`: `PUSH 0x695`
- `0x00622E71`: call wide text conversion `0x007B7140`
- `0x00622E7D`: `PUSH 0x4B2`
- `0x00622E83`: call `SendMessageA`

`FUN_006040B0` confirms standard `0x102` mappings including `0x6A0 -> STT:SkirmishEditPlayer`, `0x617 -> STT:SkirmishButtonStartGame`, `0x5C0 -> STT:SkirmishButtonBack`, `0x529/0x511/0x50C` trackbar help, and the checkbox/combo groups. It has no `0x695` case, so hovering the strip itself falls through to blank.

Current Rust has no named `0x695` layout field, status/help state, or render path in the scanned Skirmish shell files.

### 3.6 Modal Blocking And Dismissal

Active in YR: Yes for native modal transactions; current Rust is partial. Prior Choose Map reports prove the parent hides and blocks through the `0x6B` modal transaction, with result `2` meaning cancel and accept committing via helper flow. Current Rust now has `choose_map_modal` state and mouse handlers return early while the modal is open:

- `src/app.rs:758..760`: modal mouse-down handling consumes before parent hit tests.
- `src/app.rs:797..800`, `829..832`, `845..848`: parent mouse-up/move/wheel are blocked while the modal is open.

However, this pass did not verify keyboard dismissal for the Rust modal. It also did not make a new claim that Escape closes `0x102` or `0x6B`.

### 3.7 Sound Boundaries

| Control/action | Sound behavior | Active in YR | Evidence |
|---|---|---|---|
| Owner-draw buttons `0x617/0x5AA/0x5C0` mouse down/double-click | Plays `[AudioVisual] GUIMainButtonSound` from Rules `+0x188` before command handling | Yes | `0x0061374B..0x00613771`; `rulesmd.ini:643` |
| Owner-draw button first paint `u -> d` | Plays `[AudioVisual] GenericClick` from Rules `+0x70C` | Conditional on visual transition | prior button report; `rulesmd.ini:703` |
| Checkbox icon click | Plays `[AudioVisual] GUICheckboxSound` from Rules `+0x1AC` after state toggle/invalidate, before parent `WM_COMMAND`; label/outside click is silent | Yes | prior checkbox sound report; `rulesmd.ini:652` |
| Combo collapsed mouse down/double-click | Plays `[AudioVisual] GUIComboOpenSound` from Rules `+0x1A4` before the rightmost-20px arrow gate | Yes | `0x006184A2..0x006184BA`; `rulesmd.ini:650` |
| Combo popup row select/close/cancel | Plays `[AudioVisual] GUIComboCloseSound` from Rules `+0x1A8` | Yes | `0x0060E4E9..0x0060E500`; `rulesmd.ini:651` |
| Dropdown scrollbar arrow/page/thumb | Silent; sends scroll state/notification only | Conditional when dropdown has a scrollbar | decompile `OwnerDraw_ScrollBar_0061C690`; no `0x00750920` call |
| Trackbar changed quantized value | Sends parent `WM_HSCROLL` first, then plays `[AudioVisual] GenericClick` unless suppressed; unchanged drag/release/setup are silent | Yes | `0x0061E609..0x0061E6DD`; `rulesmd.ini:703` |

Current Rust has implemented most scoped sound boundaries: `SkirmishShellUiSound::{GuiCheckboxSound, GenericClick, GuiComboOpenSound, GuiComboCloseSound}` at `src/ui/skirmish_shell/state.rs:53..59`, app mapping at `src/app.rs:920..936`, and app playback/drain at `src/app.rs:903..949`. The remaining broad input backlog is not sound identity; it is edit/status/keyboard parity.

## 4. INI Keys

| Key | YR value | Binary role | Active in YR |
|---|---|---|---|
| `[AudioVisual] GUIMainButtonSound` | `MenuClick` | Button mouse-down/double-click sound, Rules `+0x188` | Yes |
| `[AudioVisual] GUIComboOpenSound` | `MenuACBOpen` | Collapsed combo open sound, Rules `+0x1A4` | Yes |
| `[AudioVisual] GUIComboCloseSound` | `MenuACBClose` | Dropdown close/select/cancel sound, Rules `+0x1A8` | Yes |
| `[AudioVisual] GUICheckboxSound` | `MenuClick` | Checkbox icon-click sound, Rules `+0x1AC` | Yes |
| `[AudioVisual] GenericClick` | `MenuClick` | Button paint transition and changed trackbar sound, Rules `+0x70C` | Yes |
| Player-name edit/status behavior | none | Shell HWND/message/resource/CSF behavior, not INI-driven | Yes |

Evidence: `ini/rulesmd.ini:643`, `650`, `651`, `652`, `703`; base fallback in `ini/rules.ini:489`, `496`, `497`, `498`, `577`.

## 5. Integration Points

| Point | Behavior | Active in YR | Evidence |
|---|---|---|---|
| `FUN_006AE2C0` | Creates offline Skirmish dialog `0x102`, pumps until Start/Back result | Yes | decompile; `0x006AE31C..0x006AE328` |
| `FUN_006AE3F0` | Dialog proc delegates common handler first, then handles init, paint, command, and `0x4E9` status text | Yes | decompile `0x006AE3F0` |
| `FUN_006AE6E0` | Initializes edit, combos, trackbars, checkboxes, selected map/mode state | Yes | decompile `0x006AE6E0` |
| `FUN_006ACEE0` | Handles parent `WM_COMMAND`; reads `0x6A0` on Start and runs Choose Map/Back/validation paths | Yes | assembly `0x006AD375..0x006AD39F`; prior reports |
| `OwnerDraw_Edit_00614190` | Old Edit paint/text/focus/key behavior | Yes for `0x6A0` | decompile `0x00614190` |
| `FUN_00622B50` | Common shell parent handler; status strip update on hover hit test | Yes | decompile `0x00622B50` |
| `FUN_006040B0` | Dialog/control id to status key mapper | Yes | decompile `0x006040B0` |
| Owner-draw button/check/combo/trackbar callbacks | UI sound and input state boundaries | Yes/Conditional per control | reports and assembly ranges above |

## 6. Current Rust Implementation Status

| Rust surface | Current status | Binary delta |
|---|---|---|
| `src/ui/skirmish_shell/state.rs` | Has `player_name: String` and launch packing uses it; has no edit focus/caret/selection/text input fields found by scan | Missing `0x6A0` user editing, 19-char cap enforcement on input, select-all-on-focus, caret/selection, and focus-stable redraw behavior |
| `src/app_skirmish_shell_render.rs` | Still draws literal `"Player"` in the player-name rect | Mismatch: should render editable state text through edit-specific frame/inset/caret path |
| `src/app.rs` keyboard handling | Escape closes native Skirmish shell at `1068..1073`; no Skirmish text input route found | Escape close is unverified against scoped binary evidence; text entry for `0x6A0` is missing |
| `src/ui/skirmish_shell/layout.rs` | `layout.player_name` rect matches `(58,59,151,23)` | Geometry basis is present; edit frame/caret/status pieces are missing |
| `src/ui/skirmish_shell/layout.rs` and renderer | No named status/help strip `0x695` field or render path found | Missing bottom-left help/status strip rect, blank default, hover text resolver, and CSF/STT source |
| `src/ui/skirmish_shell/state.rs` / `src/app.rs` | Button/check/combo/trackbar sound events mostly implemented | Keep scoped sound boundaries; do not add wheel/scrollbar sounds or unverified keyboard-triggered sounds |
| `src/app.rs` modal guards | Choose Map modal consumes parent mouse down/up/move/wheel while open | Partial: modal blocking exists, native modal transaction/visuals and keyboard dismissal remain broader work |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Standard `0x102` Skirmish entry | verified | `0x006AE31C..0x006AE328`, decompile `0x006AE2C0` | none |
| Player-name edit setup/readback | verified | `0x006AE6F2..0x006AE735`; `0x006AD375..0x006AD39F`; prior report | none |
| Old Edit focus `0x4B0/0x4AF` | verified | decompile `0x00614190`; assembly `0x006143BE..0x006145AD`; prior report | full common-shell frame scheduling deferred |
| Edit Tab behavior | verified | decompile `0x00614190` `WM_CHAR` branch; prior report | none for old Edit |
| Edit Enter behavior | touched-not-exhausted | decompile `0x00614190` style-bit branch | standard resource style and global dialog Enter accelerator remain follow-up if needed |
| Escape-to-Back | not-touched/negative-for-this-slice | no branch in `0x006AE3F0` decompile; Rust has app-level Escape close | needs focused dialog-manager/runtime trace before claiming parity |
| Status child `0x695` update/source | verified | `0x00622CCB..0x00622E83`; decompile `0x006040B0`; prior report | pixel screenshot optional |
| Button sound boundaries | verified-by-prior-plus-asm | `0x0061374B..0x00613771`; button report | runtime double-audibility optional |
| Checkbox sound boundaries | verified-by-prior | checkbox sound report; `rulesmd.ini:652` | disabled-control OS delivery deferred |
| Combo/dropdown sound boundaries | verified-by-prior-plus-asm | `0x006184A2..0x006184BA`; `0x0060E4E9..0x0060E500` | mouse-wheel runtime delivery deferred |
| Dropdown scrollbar silence | verified-by-prior | decompile `0x0061C690`; no sound call | none for sound slice |
| Trackbar changed-value sound | verified-by-prior-plus-asm | `0x0061E609..0x0061E6DD` | runtime audibility optional |
| Modal blocking/dismissal | touched-not-exhausted | Choose Map accept/cancel reports; Rust scan | keyboard dismissal and full `0x6B` visual shell out of scope |
| Current Rust backlog | verified by scan | `rg` and focused reads listed in Sources | implementation pending |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-01 - Is this the live offline YR Skirmish shell path? -> Yes; `FUN_006AE2C0` creates dialog `0x102` with proc `0x006AE3F0` and pumps until `0x617/0x5C0`.` (evidence: `0x006AE31C..0x006AE328`)
- `[RESOLVED] OQ-02 - Is `0x6A0` a real editable control? -> Yes; setup initializes it with `0xC5`, `0x4B2`, `0x4D1`, and Start reads it with `0x4B3`.` (evidence: `0x006AE6E0`, `0x006ACEE0`)
- `[RESOLVED] OQ-03 - Does current Rust render/edit the actual player-name state? -> No; state exists and launch uses it, but renderer draws literal `"Player"` and no edit input route was found.` (evidence: `src/ui/skirmish_shell/state.rs:544`, `1429`; `src/app_skirmish_shell_render.rs:1762..1768`; `rg` scan)
- `[RESOLVED] OQ-04 - What do `0x4B0/0x4AF` do for `0x6A0`? -> They implement deferred focus restore around old Edit focus/redraw; Rust should model stable edit focus, not literal messages.` (evidence: `0x00614190`; prior focus report)
- `[RESOLVED] OQ-05 - Is Tab verified? -> Yes inside the old Edit path; it moves focus to the next dialog tab item and returns handled.` (evidence: `0x00614190` `WM_CHAR 0x102` branch)
- `[RESOLVED] OQ-06 - Is Enter globally verified as Start? -> No; only conditional old-Edit Enter behavior was observed in scope.` (evidence: `0x00614190`; `0x006AE3F0` lacks key handling)
- `[RESOLVED] OQ-07 - Is Escape-to-Back verified? -> No scoped evidence; current Rust's Escape close remains an unverified parity risk.` (evidence: `0x006AE3F0`; `src/app.rs:1068..1073`)
- `[RESOLVED] OQ-08 - Is `0x695` a real status/help strip? -> Yes; common parent hover hit-test sends `0x4B2` dynamic text to child `0x695`.` (evidence: `0x00622B50`; prior status report)
- `[RESOLVED] OQ-09 - Does current Rust implement `0x695`? -> No named status strip rect/state/render path was found.` (evidence: `rg` scan of `layout.rs`, `state.rs`, `app_skirmish_shell_render.rs`)
- `[RESOLVED] OQ-10 - Are button/check/combo/trackbar sound identities settled? -> Yes for scoped mouse/control paths; see Section 3.7.` (evidence: prior sound reports and assembly spot-checks)
- `[RESOLVED] OQ-11 - Are dropdown scrollbar actions sounded? -> No; scrollbar callback is silent at the scoped callback level.` (evidence: decompile `0x0061C690`)
- `[RESOLVED] OQ-12 - Does current Rust mostly implement scoped UI sound identities? -> Yes for checkbox, combo, trackbar, and button mouse-down; maintain boundaries.` (evidence: `src/ui/skirmish_shell/state.rs:53..59`; `src/app.rs:903..949`)
- `[DEFERRED] OQ-13 - Exact native previous-proc return for posted `0x4B0`.` (category: `bounded-cost-too-high`; reason: visible focus-restore side effect is proven before fallback; native unknown-message return is not Rust-facing; next-step-if-pursued: runtime trace native edit WndProc return)
- `[DEFERRED] OQ-14 - Full common-shell schedule for every `0x4AF` broadcast.` (category: `requires-different-system-context`; reason: producer/consumer behavior is known, but all parent paint/invalidation scheduling conditions are common-shell-wide; next-step-if-pursued: focused common-shell timing report)
- `[DEFERRED] OQ-15 - Global Enter/Escape dialog-manager accelerator behavior for `0x102` and `0x6B`.` (category: `needs-runtime-debugger`; reason: not proven in the scoped dialog proc/old-Edit reads; next-step-if-pursued: runtime breakpoint on `FUN_006ACEE0` while pressing Enter/Escape with and without edit/modal focus)
- `[DEFERRED] OQ-16 - Retail runtime mouse-wheel delivery for combo popup.` (category: `needs-runtime-debugger`; reason: no explicit `0x20A` handler found in scoped callbacks; next-step-if-pursued: live wheel trace on `ComboDropWin`)
- `[DEFERRED] OQ-17 - Full Choose Map modal keyboard dismissal and visual shell.` (category: `out-of-scope`; reason: modal visual composition assigned to separate slots; next-step-if-pursued: focused `0x6B` input/modal report)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| `0x6A0` is an editable 19-character player-name field seeded/read back through shell messages | `0x006AE6F2..0x006AE735`; `0x006AD375..0x006AD39F`; prior edit report | missing/mismatch: state exists, render still literal, no input route | `src/ui/skirmish_shell/state.rs`, `src/app.rs`, `src/app_skirmish_shell_render.rs` | Add focused edit state, text input/backspace/delete/caret/selection, 19-char cap, and render state text instead of literal label | Focus name field, type 20 printable chars: displayed/stored name is capped to 19; Start session uses edited name | Do not leave the hardcoded `"Player"` draw; do not route typed chars to global hotkeys while edit focused |
| Focus restore makes old Edit input stable across redraws via `0x4B0/0x4AF` | `0x00614190`; `0x006143BE..0x006145AD`; focus report | missing | `src/ui/skirmish_shell/state.rs`, `src/app.rs` | Keep edit focus/caret/input active after shell redraws, dropdown changes, status updates, and preview invalidations | Focus name, type `A`, change map/combo causing redraw, type `B`; field contains `AB` and caret remains in edit | Do not build a generic Win32 custom-message bus; model the observable state |
| First focus selects all old edit text before deferred focus guard | `0x006143C5..0x006143CF`; edit report | missing | `src/ui/skirmish_shell/state.rs`, renderer | Track selection so first printable character can replace the default selected name | Click default name, type `E`; field becomes `E`, not `PlayerE` | Do not implement append-only click focus |
| Edit Tab is verified; global Enter/Escape are not | `0x00614190`; no key branch in `0x006AE3F0` | mismatch/risk: Rust closes shell on Escape | `src/app.rs` keyboard handling and tests | Implement only verified edit Tab behavior now; gate or remove Escape-close parity claim until traced | Press Tab while name edit focused: focus leaves edit to next tab control; pressing Escape is not claimed as retail parity unless later trace proves it | Do not invent Start-on-Enter or Escape-to-Back behavior from modern UI expectations |
| `0x695` status strip is bottom-left, blank by default, and populated from hover `STT:*` mapping | `0x00622CCB..0x00622E83`; `0x006040B0`; status report | missing | `src/ui/skirmish_shell/layout.rs`, `src/ui/skirmish_shell/state.rs`, `src/app_skirmish_shell_render.rs` | Add status/help strip rect/state, blank default, hover resolver using verified control IDs and CSF/STT keys | Fresh shell shows no status text; hover Start shows localized `STT:SkirmishButtonStartGame`; hover strip itself stays blank | Do not show hardcoded "Status", map name, or visible control labels as help text |
| Button/check/combo/trackbar sounds have specific boundaries | assembly/reports in Section 3.7; `rulesmd.ini` keys | mostly implemented/current | `src/ui/skirmish_shell/state.rs`, `src/app.rs`, rules sound parsing | Preserve scoped sound events and silence boundaries | Button down plays main button sound; checkbox label click silent; combo open/close sound once; scrollbar-only click silent; changed trackbar step plays GenericClick | Do not add sounds to dropdown scrollbar, mouse wheel, programmatic setters, or every boolean/value mutation |
| Native Choose Map is modal and blocks parent interactions while open | Choose Map reports; Rust `src/app.rs:758..856` | partial: blocking exists, native transaction/visuals broader | `src/app.rs`, `src/ui/skirmish_shell/state.rs`, renderer | Keep parent input blocked while modal is open; do not let parent controls change behind modal | Open Choose Map modal, click where parent Start button sits; parent Start is not triggered | Do not use modal-open state as proof of Escape/Enter dismissal without binary evidence |

### Negative Facts / Do Not Do

- Do not treat `0x6A0` as static text. Active in YR: No. Evidence: `GetDlgItem(0x6A0)`, `0xC5`, `0x4B2`, `0x4B3` in setup/start.
- Do not keep rendering literal `"Player"` once edit state exists. Active in YR: No. Evidence: setup/readback use `DAT_00A8B380`; Rust currently draws literal at `src/app_skirmish_shell_render.rs:1765`.
- Do not implement `0x4B0` as a visible text command. Active in YR: No. Evidence: old Edit has no dedicated `0x4B0` body; visible effect is focus guard.
- Do not claim Escape closes the Skirmish shell yet. Active in YR: Not verified in this slice. Evidence: no scoped key branch in `0x006AE3F0`; needs runtime/dialog-manager trace.
- Do not put permanent text in `0x695`. Active in YR: No. Evidence: status fallback sends empty text when no hover source exists and `FUN_006040B0` has no `0x695` self-tooltip case.
- Do not use visible labels as status strip help. Active in YR: No. Evidence: status fallback maps to `STT:*` keys through `FUN_006040B0`.
- Do not play combo sounds for dropdown scrollbar-only actions or invent mouse-wheel sounds. Active in YR: No scoped evidence. Evidence: `OwnerDraw_ScrollBar_0061C690` has no sound call; combo mouse wheel remains deferred.
- Do not route shell UI input/sounds through `sim/`. Active in YR: No. Evidence: all verified behavior is Win32 shell/UI callback behavior before match launch.

### Stale Docs / Follow-up Docs

- [SKIRMISH_PLAYER_NAME_EDIT_CONTROL_0X6A0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PLAYER_NAME_EDIT_CONTROL_0X6A0_GHIDRA_REPORT.md) current-Rust wording is now partly stale: Rust has a `player_name` field and launch packing, but still lacks real edit rendering/input/focus/caret. Replacement wording: "Current Rust has `SkirmishShellState::player_name` and launch packing uses it, but the native shell renderer still draws the literal `Player`, and no Skirmish text-input/focus/caret/selection route was found."
- Sound reports that say checkbox/combo/trackbar Rust deltas were missing are now stale in implementation-status sections; their binary sound boundaries remain valid.

## Sources

- Ghidra read-only decompile: `FUN_006AE2C0 @ 0x006AE2C0`, `FUN_006AE3F0 @ 0x006AE3F0`, `FUN_006AE6E0 @ 0x006AE6E0`, `FUN_006ACEE0 @ 0x006ACEE0`, `OwnerDraw_Edit_00614190 @ 0x00614190`, `FUN_00622B50 @ 0x00622B50`, `FUN_006040B0 @ 0x006040B0`.
- Ghidra read-only assembly contexts: `0x006AE31C..0x006AE328`, `0x006AE6F2..0x006AE735`, `0x006AD375..0x006AD39F`, `0x006143BE..0x006145AD`, `0x00622CCB..0x00622E83`, `0x0061374B..0x00613771`, `0x006184A2..0x006184BA`, `0x0060E4E9..0x0060E500`, `0x0061E609..0x0061E6DD`.
- Prior reports: [SKIRMISH_PLAYER_NAME_EDIT_CONTROL_0X6A0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PLAYER_NAME_EDIT_CONTROL_0X6A0_GHIDRA_REPORT.md), [SKIRMISH_PLAYER_NAME_EDIT_FOCUS_MESSAGES_0X4B0_0X4AF_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PLAYER_NAME_EDIT_FOCUS_MESSAGES_0X4B0_0X4AF_GHIDRA_REPORT.md), [SKIRMISH_STATUS_CHILD_0X695_TEXT_SOURCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_STATUS_CHILD_0X695_TEXT_SOURCE_GHIDRA_REPORT.md), [SKIRMISH_BUTTON_CLICK_SOUND_PARITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_BUTTON_CLICK_SOUND_PARITY_GHIDRA_REPORT.md), [SKIRMISH_CHECKBOX_CLICK_SOUND_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHECKBOX_CLICK_SOUND_GHIDRA_REPORT.md), [SKIRMISH_COMBO_DROPDOWN_SCROLLBAR_SOUNDS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_COMBO_DROPDOWN_SCROLLBAR_SOUNDS_GHIDRA_REPORT.md), [SKIRMISH_TRACKBAR_CHANGED_VALUE_SOUND_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_TRACKBAR_CHANGED_VALUE_SOUND_GHIDRA_REPORT.md), [SKIRMISH_CHOOSE_MAP_ACCEPT_CANCEL_SIDE_EFFECTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_ACCEPT_CANCEL_SIDE_EFFECTS_GHIDRA_REPORT.md), [SKIRMISH_START_VALIDATION_MODAL_TEXT_AND_MODE_REJECTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_VALIDATION_MODAL_TEXT_AND_MODE_REJECTION_GHIDRA_REPORT.md).
- INI checked: `ini/rulesmd.ini`, `ini/rules.ini`.
- Rust scanned: `src/ui/skirmish_shell/state.rs`, `src/ui/skirmish_shell/layout.rs`, `src/app_skirmish_shell_render.rs`, `src/app.rs`.
