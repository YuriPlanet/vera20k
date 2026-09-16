# Shell-Substrate Slice 5 — Modal substrate (`ModalKind`) + lifecycle/pump contract (FINAL — resolution pass)

Implementation plan. **PLAN ONLY — no code is written in this slice's planning pass.** Build/test verification runs as a separate bounded foreground pass; STOP for in-game OK before commit; commit the slice separately on `dev`.

This slice has the largest blast radius of the substrate series. Every sub-step below is independently buildable, and the skirmish safety net (`src/ui/skirmish_shell/state/tests.rs`, **2147 lines, 87 `#[test]` functions** — verified by `wc -l`/`grep -c` this pass) must stay GREEN and UNCHANGED throughout. No edit to that file is part of this slice. (The render-side unit test in `src/app_skirmish_shell_render.rs` is NOT part of that safety net and WILL change in sub-step 6 — see §2.1 frame-index correction.)

**Resolution-pass status:** six read-only investigators closed the prior blockers. The two user decisions for this pass are baked in: **(a)** the in-game Options 0xBBB/0xF5 chrome is now **IN SCOPE** — pixel-faithful from the parsed resource layouts, not mechanism-only; **(b)** the quit-confirm settings filename and the 0x120 ESC default-button behavior are now **PRE-REQS, RESOLVED**, not follow-ups.

---

## 0. RESOLVED pre-requisite: C13 template-id mapping (was the blocker)

The Slice-5 row in `docs/plans/2026-05-31-shell-substrate-design.md:296` carries **"C13 template-id mapping is UNCHECKED — trace a caller of the modal helper before wiring ModalKind."** That pre-req is now **fully RESOLVED**. Both the count-rule **selection** (finding *C13 modal template-id mapping*) and the in-game-Options own-path **selection + own dialog proc + result convention + chrome layout** are PROOFED from static disassembly/decompile — no emulation required.

**The count-rule selection is a populated-optional-slot COUNT, not a string lookup** (finding *C13*, claims 1–3). The generic CSF message-box helper takes four pre-resolved UTF-16 CSF string pointers (body, ok, third, fourth); it tests each for non-null-and-non-empty-first-char and selects the RT_DIALOG template id purely by which optional slots are populated:

| Slots populated | Template id | Controls |
|---|---|---|
| body + ok only (slots 3 & 4 empty) | **0xCE** | body static `0x5B0` + OK button `0x5AE` |
| third slot present, fourth empty | **0x120** | static `0x5B0` + buttons `2`/`0x5AE`/`0x5AF` |
| fourth slot present | **0x121** | adds the 4th owner-draw button (TS-era branch, reachable but unused by the three target modals) |

Param→control→result mapping for the **count-rule (message-box helper) modals** (PROOFED this pass via `decompile_function 0x005D36A0`): body→`0x5B0` (Static, no click result); OK→`0x5AE` → **result 0** (`*puVar2 = 0`); →control `2` (IDCANCEL, matched by `param_3 != 0 && param_3 < 3`) → **result 1** (`*puVar2 = 1`); →`0x5AF` → **result 2** (`*puVar2 = 2`). **This corrects an INVERTED mapping in the earlier draft** — see §1.2 and §4. The id is passed by ECX into the dialog factory, which does `FindResourceA(RT_DIALOG=5, id)` + `CreateDialogIndirectParamA`.

**Verified `ModalKind` → template-id table:**
- `QuitConfirm` → **0x120** — body `0x5B0` + OK `0x5AE` + Cancel control `2`; reached from main-menu Quit (0xE2 ctrl `0x3EE` → `FUN_00531CC0` returns 6 → `Main_Game @0x0052D9A0` case 6). Template raw bytes at VA `0x00C00B24`: 4 controls ids `2`/`0x5B0`/`0x5AE`/`0x5AF`, all `BS_OWNERDRAW`, **zero BS_DEFPUSHBUTTON**.
- `SkirmishStartValidation` → **0xCE** — body `0x5B0` + single OK `0x5AE`; never add control `2`/`0x5AF`.
- `InGameOptions` → **0xBBB** active-game / **0xF5** shell — SEPARATE mechanism (`OptionsClass__ShowInGameDialog @0x004E1D00`, own proc `FUN_004E1FE0`), NOT the count-rule helper.

### 0.1 In-game Options: RESOLVED facts — read before wiring InGameOptions

- **Discriminator VALUE is `== 1` (PROOFED):** `ShowInGameDialog @0x004E1D00` reads byte `0x00A8E9A0`, `CMP AL,0x1 @0x004E1D32`, `JZ→ECX=0xBBB`, else `MOV ECX,0xF5`. So byte==1 → 0xBBB (active), else → 0xF5 (shell). Equality, not `!=0`. (`search_byte_patterns 'B9 BB 0B 00 00'` single hit.)
- **Own dialog proc = `FUN_004E1FE0` (PROOFED):** factory `FUN_00622650` → `CreateDialogIndirectParamA(..., lpDialogFunc=0x004E1FE0)`. Result in `local_4` (init -1) via `SetWindowLongA(hWnd, 8, &local_4)`; pump `FUN_00623120`.
- **Result convention (own-proc, PROOFED this pass via `decompile_function 0x004E1FE0`):** `result == 1 ⇒ PERSIST`. EVERY close button sets `*puVar3 = 1` directly in `FUN_004E1FE0`'s WM_COMMAND (0x111) handler: OK `0x52C` (when `g_GameActive == 1` → also sets `g_GameState = 4`), `0x52D` (when `g_GameActive == 1` → sets `g_GameState = 6`), Back `0x686` (unconditional). On `result == 1` the caller chain runs `ApplyFromInGameDialog (0x004E1DE0)` then `WriteToINI (0x005FAD10)`. **NO cancel-without-save** — there is no close button that yields a non-persist result. Only the game ending while the modal is open → result 2 → no persist.
  - **Supersession note (recorded explicitly):** resolution-pass Finding #2 flagged this same proc's WM_COMMAND/result mapping as UNTRACED and made sub-step 5a's INI-write-on-OK depend on tracing it. The dedicated ShowInGameDialog/own-proc investigator (Finding #1) traced it directly and PROOFED `result == 1 ⇒ persist` for `0x52C`/`0x52D`/`0x686` (`decompile_function 0x004E1FE0`, each branch `*puVar3 = 1`; `0x004E1D9A CMP EAX,0x1` then `CALL Apply` / `CALL WriteToINI`). The plan adopts Finding #1; Finding #2's UNTRACED tag is superseded, not silently overridden.

> **In-game Options chrome is a FULL shell dialog, NOT a message-box modal** — 533×369 DLU (same as `0x102`), standard Win32 BUTTON/STATIC/`msctls_trackbar32`, NOT `MNBTTN+PUDLGBGN`. CANNOT use `paint_modal_shp`; uses the skirmish `0x102` owner-draw control family.

**Parsed 0xBBB layout** (plain DLGTEMPLATE @VA `0x00C01B18`, size `0x3E8`; 17 controls, DLU (0,0,533,369), 8pt MS Sans Serif): Back `0x686` BUTTON `0x5000000B` (425,346,108,23); title `0x694` STATIC `0x50020001` (425,1,108,10); GameSpeed slider `0x529` (144,100,128,13); GUI:GameSpeed `0x714` (61,99,78,15); ScrollRate slider `0x52A` (144,131,128,13); GUI:ScrollRate `0x715` (61,130,78,15); VisualDetails slider `0x52B` `0x48000018` (144,162,128,13); GUI:VisualDetails `0x716` (61,161,78,15); GUI:HigherDetail `0x673` (278,161,92,15); GUI:Faster `0x672` (278,130,92,15); GUI:Faster `0x671` (278,99,92,15); TargetLines checkbox `0x601` `0x50008003` (89,206,119,10); Tooltips checkbox `0x602` (214,206,127,10); Keyboard btn `0x52C` `0x5000000B` (425,149,108,23); Sound btn `0x52D` (425,122,108,23); ShowHidden checkbox `0x604` (89,224,119,10); GUI:Blank `0x695` (2,355,303,12). (`read_memory 0x00C01B18 len 1000`.)

**Parsed 0xF5 layout** (DLGTEMPLATEEX @VA `0x00BF9F58`, size `0x514`; dlgVer 1 sig `0xFFFF`; 20 controls): SIMILAR control SET to 0xBBB (Back `0x686`, title `0x694`, three `msctls_trackbar32` sliders + GUI labels) **but with DIFFERENT rects — do NOT reuse 0xBBB slider rects for 0xF5.** The 0xF5 sliders are repositioned and WIDENED (cx 148, not 0xBBB's 128), verified inline this pass (`read_memory 0x00BF9F58 len 700`): GameSpeed slider `0x529` (138,82,148,13); ScrollRate slider `0x52A` (138,109,148,13); VisualDetails slider `0x52B` (138,163,148,13). PLUS 0xF5-only adds: Difficulty slider `0x50F` (138,136,148,13) + GUI:Difficulty + GUI:Harder `0x670`; ScrollCoasting checkbox `0x51A` (204,209,128,10); transparent bitmap placeholder `0x71C` (exStyle `0x20`). 3 unnamed labels = IDC_STATIC. NO COMBOBOX/LISTBOX/EDIT. Use the separately-parsed 0xF5 table for all 0xF5 rects — neither layout is derived from the other.

> **Remaining narrow chrome gaps (§9):** (1) DLU→pixel projection at 800×600/1024×768 (rects PROOFED, projection not computed); (2) which owner-draw SHP/PAL the controls use (analogous to `0x102`, not confirmed identical). Both bounded inside sub-step 5a; neither blocks scoping.

---

## 1. Where `ModalKind` lives and how it integrates

New module **`src/ui/shell/modal.rs`** (declared in `src/ui/shell/mod.rs`). Render-agnostic; no sim/render/assets deps. Substrate seams already present: `BgKind::ModalShp` (`descriptor.rs:65-66`), `RepositionPolicy::ModalCentered` (`descriptor.rs:74-77`), `DialogController` LIFO stack + `kbd_route` + press-must-match (`controller.rs`), `on_key` returns `false` today (`controller.rs:26-32,192-194`).

### 1.1 `ModalKind` data shape
```
pub enum ModalKind {
    BodyOk,        // 0xCE
    Confirm,       // 0x120 — body + OK 0x5AE + Cancel(control 2)
    ThreeButton,   // 0x121 — render-UNTESTED, excluded from in-game OK acceptance
    InGameOptions, // 0xBBB / 0xF5 — own proc 0x004E1FE0; selection PROOFED ==1 (byte @0x00A8E9A0)
}
impl ModalKind { pub fn template_id(self, in_active_game: bool) -> u16 }
```
Comment the `==1` discriminator PROOFED (no emulate-gate). `ThreeButton` dead surface; retain for count-rule test or drop. Message-box family (0xCE/0x120/0x121) uses the count-rule descriptor builder; `InGameOptions` builds a full-shell owner-draw descriptor from the parsed table.

### 1.2 Modal result mapping — TWO distinct conventions
- **Message-box helper (0xCE/0x120/0x121, proc `0x005D36A0`):** OK `0x5AE`→**0** (`*puVar2 = 0`); Cancel control `2`→**1** (`*puVar2 = 1`); `0x5AF`→**2** (`*puVar2 = 2`); body `0x5B0`→none. Quit caller consumes **0 = QUIT**, non-zero = CANCEL. **Corrects the earlier inverted "OK→1/Cancel→2".** Helper inits `local_8=-1`, returns it unchanged. (PROOFED this pass — see §0.)
- **InGameOptions own proc (0x004E1FE0):** `result==1 ⇒ persist`, fires for every close button (`0x52C`/`0x52D`/`0x686`, no cancel-without-save); game-ending → result 2 (no persist). Persist on result==1; no discard-on-cancel. (PROOFED this pass — see §0.1.)

Skirmish-validation callers ignore the result.

---

## 2. Verified modal render composition (mode-2 SHP) — message-box family only

InGameOptions is full-shell and uses the owner-draw path, NOT this emitter. Native composition: mode-2 SHP background = **PUDLGBGN.SHP frame 0 + DIALOGN.PAL** (shell theme); themed PUDLGBGA/S/Y dormant for shell; `dbak6440.pcx` never reached. OK button owner-draw type 3 = **MNBTTN.SHP + MAINBTTN.PAL, frame 0=up/1=disabled/2=pressed**. Text after art. Port already loads PUDLGBGN+DIALOGN+MNBTTN (`skirmish_shell_chrome.rs:200-209`); no Soviet skin exists. Migration is additive.

### 2.1 New render emitter
`paint_modal_shp` in `src/render/shell_paint.rs` (PUDLGBGN frame 0, MNBTTN type-3 0/1/2, body/OK labels via `paint_labels`). **LATENT BUG (verified this pass):** `modal_button_mnbttn_frame_index` (`chrome.rs:321-323`) maps `pressed → 1` (DISABLED); must be `pressed → 2`. The match consumer at `chrome.rs:341-344` already has a `2 => frame2` arm that is currently unreachable. Update render-side test `app_skirmish_shell_render.rs:969-970` (which presently asserts `(true)==1`; NOT the safety net). Rewrite `push_validation_modal_instances` (`modals.rs:171-208`) to delegate to `paint_modal_shp`.

---

## 3. Lifecycle / pump contract (C2) — RESOLVED

### 3.1 Verified pump body
`FUN_00623120`: always service Win32/net queue (`Process_NetworkMessages @0x005D4D50`, no blit). Advance sim ONLY in network: network-service-only branch when `g_GameMode==0 || ==5 || DAT_00A8D60E!=0 || DAT_00A8DAB4!=0`, else `Main_Tick (0x0055D360)`. OFFLINE={0,5} no sim; NETWORK={3,4} advances. Re-entrancy guard `FUN_0055CBF0` returns `DAT_00ABCD58` (Main_Tick-in-progress sentinel, not user-pause).

**RESOLVED — offline battlefield FROZEN-AS-LAST-BLIT:** recomposite `FUN_00532100` (BSurface+`FUN_004F4780`) gated `g_GameActive==0` — fires only in 0xF5 shell case, never active offline. Active offline (mode 5) runs only `Network_ServiceLoop`→`FUN_00406F70` sound tick, no draw. Last blit stays.

### 3.2 `service_tick(mode)` in the app layer (`src/app_sim_tick.rs`)
Network branch reuses unchanged `advance_fixed_simulation` (`app_sim_tick.rs:234`); `World::advance_tick` signature unchanged; `SessionMode` read only by app loop, never by `sim/`.

**Game-mode discriminator RESOLVED:** `g_GameMode @0x00A8B238` (int) — **0=campaign, 3=LAN, 4=WOL, 5=skirmish**. Enum values are WRITER-PROOFED: `search_byte_patterns 'C7 05 38 B2 A8 00'` (= `MOV dword ptr [0x00A8B238], imm32`) returns 5 write sites (`0x0052DD61`, `0x0052DD7F`, `0x0052E10F`, `0x0052E3C6`, `0x005EFDC3`) with imms 4/3/5/3/0. Predicate is SET membership: `{3,4}` advance, `{0,5}` frozen — NOT a single constant.

Pure seam: `fn modal_pump_should_advance_sim(session_mode, reentrancy_in_progress) -> bool`. Test drives it + headless `World` (`World.tick: u64`, `world/mod.rs:84`): delta==N network, ==0 offline; no `&mut AppState`. Add `SessionMode`/`is_network_game()` returning Skirmish/offline for current play. Network branch is DEAD CODE this build.

### 3.3 Input capture / focus
While modal open, route keyboard through `DialogController::on_key` (`controller.rs:131-194`), consume Enter/Escape before global Esc. Replaces bespoke handlers only for newly-substrate-hosted modals; validation field/tests untouched until sub-step 6.

---

## 4. Quit-confirm: settings-persist-before-cascade — RESOLVED filename + ESC

Quit: 0xE2 ctrl `0x3EE` → `FUN_00531CC0`=6 → `Main_Game @0x0052D9A0` case 6 → `FUN_005D3490` (template 0x120, body `GUI:ExitAreYouSure`/`TXT_OK`/`GUI:Cancel`/NULL). On confirm (result 0): `WriteToINI (0x005FAD10) @0x0052DE2F` then state=7; else state=0x12. INI-write strictly precedes teardown. No PostQuitMessage/ExitProcess.

`WriteToINI @0x005FAD10` writes `[Options]`/`[Video]`/`[Audio]`/`[Network]` (sections verified).

**RESOLVED facts:**
1. **Filename = `RA2MD.INI`** (uppercase, NUL-term): `WriteToINI` pushes ptr `0x826444` into CCFileClass ctor `0x004739F0`; `read_memory 0x00826444` (re-verified this pass) = `52 41 32 4D 44 2E 49 4E 49 00` = "RA2MD.INI\0". No "filename UNCHECKED" caveat.
2. **ESC on 0x120 = CANCEL** (state 0x12, return to main menu): control `2` (GUI:Cancel) present, no BS_DEFPUSHBUTTON; proc `FUN_005D36A0` has no raw WM_KEYDOWN; `IsDialogMessageA` translates VK_ESCAPE→IDCANCEL(2)→result 1→CANCEL. (Enter→IDOK(1)→also CANCEL; only OK-click `0x5AE`→result 0 quits.)
3. **Vox-pump bound = `0xBB8` (3000) GetRadarTimer ticks ≈ 48 s**, NOT ~3000 ms: `GetRadarTimer @0x006C8C40 = timeGetTime()>>4` (16 ms/tick); `0x0052E796 LEA EDI,[ESI+0xBB8]`; loop gated on `VoxClass__PumpAndCheckActive (0x007529E0)`.

Teardown order (case 7): WriteToINI → `FUN_00720EA0(1)` music stop → vox-pump wait ≤0xBB8 ticks → `FUN_00720EA0(0)` → `FUN_004A3C30(0)` fade → RET 0.

Rust delta: replace egui card with `ModalKind::Confirm` (0x120), wire OK `0x5AE`→0=quit / Cancel `2`→1=stay / ESC=Cancel; on result 0 write `RA2MD.INI` before teardown; reproduce graceful cascade with vox-pump cap encoded in ticks.

### 4.1 Skirmish validation modal
Keep `SkirmishStartValidation` (0xCE). Route through substrate (descriptor `0x5B0`/`0x5AE`, `paint_modal_shp`, `DialogController`) — LAST (sub-step 6), gated on safety-net green. Rects: message DLU (40,40,220,50), OK (207,175,83,15) (`layout.rs:758-759`). MNBTTN pressed→2 fix lands here.

### 4.2 In-game options modal — pixel-faithful chrome IN SCOPE
- **Own-path id selection:** `0xBBB`/`0xF5` via PROOFED `g_GameActive==1` (the `==1` compare is read directly in `FUN_004E1FE0` as `g_GameActive == '\x01'`); own proc `0x004E1FE0`.
- **Pixel-faithful chrome (NOW in scope):** render full-shell from the parsed §0.1 control tables (use the 0xBBB table for active-game, the SEPARATELY-PARSED 0xF5 table — different slider rects, width 148 — for shell) via the skirmish-shell owner-draw control family (trackbars/checkboxes/owner-draw buttons), NOT `paint_modal_shp`. Bounded follow-ups: DLU→pixel projection + owner-draw asset confirmation (§9).
- **INI-write-on-OK (verified own-proc convention):** persist on `result==1` (fires for every close button — `0x52C`/`0x52D`/`0x686`); run `ApplyFromInGameDialog` then `WriteToINI` to `RA2MD.INI`. Only game-ending = result 2 (no persist). No discard-on-cancel.
- **`service_tick` behind it:** offline {5,0} sim frozen + battlefield frozen-as-last-blit; network {3,4} advances (dead branch).

Acceptance boundary: 5a wires id-selection + parsed-layout chrome + INI-write-on-OK behind the existing paused freeze (resolve the two bounded chrome items); 5b swaps to `service_tick` + §7.A assertion.

---

## 5. Ordered SUB-STEPS (each independently buildable; safety net stays GREEN)

1. **`ModalKind` + template-id table (`modal.rs`).** Enum, `template_id(in_active_game)`, count-rule descriptor builder, `ModalResult` with BOTH conventions (§1.2). Unit tests for table + count rule. Mark `==1` PROOFED; `ThreeButton` render-untested (or drop). Declare `pub mod modal;`. Safety net untouched.
2. **Mode-2 SHP emitter (`shell_paint.rs`).** `paint_modal_shp` (PUDLGBGN+DIALOGN, MNBTTN 0/1/2, labels). Test pressed→2. No caller. Safety net untouched.
3. **Pure pump decision + `service_tick` + session-mode (`app_sim_tick.rs`, `app.rs`).** `modal_pump_should_advance_sim` (pure). `service_tick`: always net+input+repaint; advance via existing `advance_fixed_simulation` iff decision true. `SessionMode` with resolved mapping ({3,4} advance, {0,5} frozen), returns offline. Offline unchanged. Safety net untouched.
4. **Quit-confirm — 4a + 4b.** 4a: replace egui card with `Confirm` (0x120) via `paint_modal_shp`+`DialogController`; wire OK→0/Cancel→1/ESC=Cancel; exit still existing way. 4b: settings-persist writer to `RA2MD.INI` before teardown; graceful cascade (vox-pump ≤`0xBB8` radar ticks gated on `VoxClass__PumpAndCheckActive`). Tests C + C2-quit. Safety net untouched.
5. **In-game options FULL modal — 5a + 5b.** 5a: pixel-faithful chrome from parsed 0xBBB/0xF5 tables via owner-draw family + `g_GameActive==1` selection + INI-write-on-OK (result==1, Apply→WriteToINI to RA2MD.INI), behind existing paused freeze; resolve DLU→pixel + asset items (§9). Add §7.D own-proc-convention test. 5b: swap freeze for `service_tick` (offline frozen-as-last-blit; network advances) + §7.A assertion. Safety net untouched.
6. **Migrate skirmish validation modal (LAST).** Re-point `push_validation_modal_instances` to `paint_modal_shp`; route input through `DialogController` (deletion set §8.1); correct MNBTTN pressed→2 + update `app_skirmish_shell_render.rs:969-970`. `validation_modal` field + `state/tests.rs` (87 tests) UNCHANGED/GREEN — if a safety-net test would change, STOP.

---

## 7. ACCEPTANCE TESTS

**A. C2 tick-counter (pure seam + headless World).** Offline {5,0} PRIMARY: decision false, `World.tick` delta 0 over N frames; input+repaint each frame. Network {3,4} unit-only: decision true, delta N (no live caller). Battlefield-visual (NOW expressible offline): assert offline pump triggers NO battlefield recomposite (RESOLVED frozen-as-last-blit) alongside tick==0.

**B. Render parity = asset-binding + draw-order.** Message-box: assert PUDLGBGN frame 0 + DIALOGN.PAL + MNBTTN type-3 (pressed==2), control set `0x5B0`+populated buttons, NOT flat panel/PCX/egui/dbak6440. Exclude `ThreeButton`. InGameOptions: assert descriptor reproduces parsed 0xBBB/0xF5 control sets with verified DLU rects (0xF5 sliders at width 148, NOT 0xBBB's 128). Pixel-identical NOT automatable (DLU→pixel pending §9) → manual STOP gate §10.

**C. Quit-confirm persist-before-cascade.** Order: confirm (result 0) → persist to `RA2MD.INI` → teardown. Vox-pump cap encoded as `0xBB8` radar ticks gated on `VoxClass__PumpAndCheckActive`, not 3000 ms literal. Assert ESC→Cancel (result 1), OK click→quit (result 0).

**D. InGameOptions own-proc convention (the slice's highest-risk parity fact).** Assert: `result == 1` (yielded by EVERY close button — OK `0x52C`, `0x52D`, Back `0x686`) ⇒ `ApplyFromInGameDialog` + `WriteToINI(RA2MD.INI)` both fire; `result == 2` (game-ending) ⇒ NO persist; NO discard-on-cancel path exists (there is no close button producing a non-persist result). Mirrors §7.C for the message-box convention and guards against the two-distinct-conventions mix-up (§1.2).

---

## 8. §8 retire-list cross-reference (RESOLVED — dispositions + deletion set)

List is in **[docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) §7, lines 396-429 (9 items)** (design doc §7 is "Verdict"). Dispositions: items 1/2/3 LEAVE (Slice 0 geometry); item 4 PARTIAL (0xE2/0x100 RETIRED Slices 2/3; 0x102 non-modal DEFER Slice 2/4 — cluster drifted to `app.rs:1434-1442`); item 5 PARTIAL (0xE2/0x100 emitters DEFER Slice 3; validation-modal render rewrite IS Slice 5 sub-step 6); item 6 LEAVE (Slice 3/6); item 7 LEAVE (Slice 4); item 8 PARTIAL (0xE2/0x100 already on `shell_controller` `app.rs:1499-1567`; Slice 5 retires only validation-modal slice of cluster `app.rs:1306-1497`; Esc-consume at `app.rs:1989-1998`); item 9 LEAVE (Slice 6).

### 8.1 Validation-modal deletion set (sub-step 6) — PROOFED COMPLETE (line-exact, re-verified this pass)
In `src/app.rs`: delete 4 handler fns — `is_validation_modal_dismissal_key` `1306-1311`, `handle_validation_modal_key_input` `1313-1323`, `handle_validation_modal_mouse_down` `1325-1340`, `handle_validation_modal_mouse_up` `1342-1367`. Remove 2 in-fn guards — mouse_move `1463-1468`, mouse_wheel `1483-1486`. Re-route 2 call-sites — `1370-1372`, `1421-1423`. Retire validation-modal Esc branch — inner block `1990-1995` in arm `1989-1998` (`is_escape` `1962-1963`, dismissal route `1981-1987`); route Esc through `DialogController::on_key`. The `choose_map_modal` half of `1990` is Slice-4, NOT Slice 5.

### 8.2 ADDITIVE Slice-5 deletion (egui quit-confirm — not in §7 list)
Research-doc §7:427-429 says do NOT touch `main_menu_dialogs.rs` unless a later slice folds it — Slice 5 IS that slice. Additive deletion (§4 sub-steps 4a/4b), with field references corrected this pass:
- `exit_confirm_modal` FIELD DECLARATION at `app.rs:373` (struct field) and constructor-init at `app.rs:2489` (`exit_confirm_modal: None`) — both must be removed.
- `app.rs:429` is the `main_menu_dialog_open()` ACCESSOR (`self.exit_confirm_modal.is_some()`), not the field — update/keep as the predicate consumer, do NOT mistake it for the declaration.
- open/clear paths `app.rs:1812`/`1814`/`1827`; egui draw branch `1839-1850` (`draw_main_menu_dialogs` body, dialog drawn from `1836+`); egui Esc-consume `1973-1979`; menu-action call sites `771`/`1788`.

Out of scope: in-game pause Esc `app_input.rs:389-410`; `choose_map_modal` handlers/Esc half (Slice 4); items 1/2/3/6/7/9.

---

## 9. UNCHECKED (tightened — genuinely-unresolved only; tags: STILL_UNCHECKED / NEEDS_COMPUTE / NEEDS_TRACE / NEEDS_CONFIRM. No item NEEDS_EMULATE_PASS — every prior blocker was closed via static disasm/byte-read, which is the stronger outcome.)

- **InGameOptions DLU→pixel projection** (NEEDS_COMPUTE, not Ghidra): DLU rects PROOFED (0xBBB + separately-parsed 0xF5); pixel projection at 800×600/1024×768 via `MapDialogRect`/96-DPI not computed. Bounded inside 5a.
- **InGameOptions owner-draw paint assets** (NEEDS_TRACE): which SHP/PAL for the 0xBBB/0xF5 trackbars/checkboxes/buttons; analogous to `0x102`, not confirmed identical. Bounded inside 5a.
- **`0x52C` (g_GameState 4) vs `0x52D` (g_GameState 6) player-facing distinction** (STILL_UNCHECKED, LOW — both persist via result==1): downstream `g_GameState` 4/6 meaning only partially traced. Trace only if >1 close-button behavior needed.
- **`g_GameMode` symbol-NAME trust** (HIGH-hint; ENUM VALUES writer-proofed): the 0/3/4/5 enum is writer-proofed via 5 MOV-imm write sites (§3.2) — strong evidence; only the symbol *name* is a HIGH hint. (Re-verify the name before any rename, not before use.)
- **`g_GameActive` symbol-NAME trust** (HIGH-hint; ==1 COMPARE value PROOFED): the InGameOptions selection gate rests on the compare site (`g_GameActive == '\x01'`, read in `FUN_004E1FE0`), which is PROOFED; the writer chain was NOT traced and the name is a HIGH hint. Weaker evidence than `g_GameMode`; do not lump the two together.
- **Win32 `IsDialogMessageA` ESC→IDCANCEL synthesis** (PROOFED-for-gamemd-side): USER32 step not single-steppable; authoritative because proc has no raw WM_KEYDOWN and no DEFPUSHBUTTON with control 2 present.
- **`DAT_00A8D60E`/`DAT_00A8DAB4` set conditions in offline skirmish** (STILL_UNCHECKED, LOW): `g_GameMode==5` already forces the offline branch regardless.
- **`choose_map_modal` Slice-4-vs-5 ownership** (NEEDS_CONFIRM BEFORE DELETING): shares the `1989-1998` block + dispatch guards; confirm Slice-4 ownership so no orphaned choose-map branch is left.

---

## 10. Cadence
- **PLAN ONLY** — no implementation this pass.
- **Separate verification pass** — `cargo check -p vera20k` then `cargo test -p vera20k` as a bounded foreground pass; read the literal `test result:` line.
- **STOP for in-game OK** — confirm modals in-game (PUDLGBGN/DIALOGN/MNBTTN + pressed frame; quit-confirm order + ESC=Cancel; full-shell in-game Options from parsed 0xBBB/0xF5, including 0xF5 wider sliders). Manual pixel-parity gate (§7.B), pending screenshot-diff.
- **Commit separately** on `dev`.

---

## Adjudicated review notes
- All findings from both reviews were HIGH/MEDIUM/LOW; no BLOCK and both verdicts were READY, so no item is forced to the top of the open-questions list.
- Every MEDIUM/LOW finding was folded in (none rejected): §7.D added for the InGameOptions own-proc convention (MEDIUM); §8.2 field citations corrected to `app.rs:373` decl + `2489` init with `429` clarified as the accessor (LOW ×2 across reviewers); §0.1 supersession note added recording Finding #2's UNTRACED tag vs Finding #1's direct trace (LOW); §0.1/§4.2/§7.B reworded so 0xF5 slider rects (width 148) are transcribed inline and not described as derived from 0xBBB (LOW); §3.2/§9 split `g_GameMode` (enum writer-proofed) from `g_GameActive` (name HIGH-hint, ==1 compare PROOFED) (LOW); §9 tags normalized to STILL_UNCHECKED / NEEDS_COMPUTE / NEEDS_TRACE / NEEDS_CONFIRM with an explicit note that no item NEEDS_EMULATE_PASS (LOW, optional).
- No finding was adjudicated WRONG. The closest to a no-op was the first reviewer's two satisfied-decision findings (decision (a) chrome-in-scope, decision (b) filename+ESC resolved), which required no change; they are recorded here as confirmed-satisfied rather than as rebuttals.
