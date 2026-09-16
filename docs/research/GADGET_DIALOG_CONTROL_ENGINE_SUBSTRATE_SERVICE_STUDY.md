# GadgetClass/Dialog Control — Engine Substrate Service Study & Replacement-Boundary Design

**Status:** STUDY + DESIGN (not an approved implementation plan). Read-only research; no Rust written.
**Date:** 2026-06-10
**Rule:** Rust-native structure, gamemd-native semantics.
**Scope:** gamemd.exe's two UI substrates studied as ONE engine substrate service pair:
**Framework A** — the GadgetClass/ControlClass/LinkClass retained-mode gadget tree (in-game UI; new ground, fully decoded this study) and
**Framework B** — the Win32 RT_DIALOG + owner-draw control shell substrate (already studied in [SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) 2026-05-31 and partially shipped as `ui::shell` Slices 0–3/5; integrated here **by reference plus this study's verified delta corrections** — its body is NOT duplicated).
**Provenance:** assembled from six parallel lane worknotes (`docs/research/substrate/worknotes/gadget-dialog-20260610/`: gadget-core, gadget-family, gscreen-chain, dialog-delta, rust-current, globals-registries) plus an **adversarial verification pass** whose verdicts are authoritative over lane claims — the verdict-by-verdict evidence ledger is saved as the seventh lane file [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) (same dir): every named verdict cited in this doc resolves there to its MCP calls (lane-logged, study-session-only-flagged, or re-verified live 2026-06-10). Every WRONG verdict's correction is folded into the body; every claim is tagged **VERIFIED-LIVE** (binary read this study, MCP call cited) or **DOC-INHERITED** (named doc, not re-read). **Default verdict for any unproven equivalence is DRIFT** — no internal-only escape hatch for active UI/input/render behavior. Unverifiable claims live ONLY in §9 (YELLOW).
**Bar:** indistinguishable-from-gamemd on observable output in a standard YR skirmish (offline lens; WOL/LAN paths enumerated, flagged, never silently dropped).

---

## Executive Summary

**Verdict: the current Rust UI is two half-substrates — a shipped-but-incomplete Framework B (`ui::shell`, Slices 0–3/5) and NO Framework A substrate at all (the in-game sidebar/radar/tab surface is ad-hoc per-surface code with no retained list, no capture, no fire-on-release, no tooltips).** The binary side is now fully pinned: Framework A is a 20-class single-spine gadget family of which only 9 classes are live in YR (the entire TS shell-control wing — ListClass, EditClass, SliderClass, GaugeClass, Dial8Class, etc. — is linker-retained dead code, proven by an exhaustive direct-transfer + imm32 scan), driven once per gameplay tick by `GadgetClass::Input 0x004E1640` through a three-tier dispatch (sticky capture → keyboard focus → broadcast walk) with smallest-area/half-open hit-testing and an `ID|0x8000` result protocol consumed layer-locally by the GScreen AI cascade. The five top player-visible DRIFTs vs Rust: (1) every sidebar action fires on mouse-DOWN instead of gamemd's silent-press/fire-on-RELEASE with drag-off cancel; (2) the in-game tooltip surface (1000 ms wall-clock delay, inclusive-edge rects) does not exist; (3) hit-testing is first-match-in-feed-order with mixed edge conventions instead of smallest-area-wins half-open; (4) there is no sticky-capture/hold-repeat substrate (gamemd repeats per-tick via event-mask held bits); (5) the chat/system message TextLabel surface is unimplemented. Three lane claims were overturned by the adversarial pass and are corrected herein: ShapeButtonClass does NOT repurpose Set_Peer (Set_Shape is a NEW +0x88 slot); the live gadget population is NOT built purely by CRT static initializers (message-list labels are heap-built at runtime); and player commands do NOT enter the sim inside LogicClass::Update (the "Process_QueuedEvents" label is drift for a lightning-storm screen flash — real command entry is Main_Tick → Queue_AI → DoList → EventClass::Execute, after the object-sim pass). The proposed replacement is a new `ui::gadget` service (retained list + FocusState + event-flag dispatch, gamemd-native semantics) beside the existing `ui::shell`, migrated shadow-first in 7 slices (A0–A6) while the B-track continues its own plan (Slice 4 skirmish controls, 5b options).

---

## Table of Contents

- §0. Two frameworks — scope, and relationship to the shipped shell substrate
- §1. Verified active-YR responsibilities (both frameworks)
- §2. Full inventory (vtables, methods, globals, registries, static tables, legacy paths)
- §3. Active-YR vs inactive/legacy census
- §4. Comparison against current Rust architecture (gap table)
- §5. gamemd-native BEHAVIOR CONTRACT (G* / O* / D* / S* clauses)
- §6. Rust-native replacement boundary (service design)
- §7. Old ad-hoc Rust logic to retire
- §8. Migration slices + acceptance tests
- §9. UNVERIFIED (YELLOW)
- §10. Sources

---

## 0. Two frameworks — scope, and relationship to the shipped shell substrate

gamemd.exe runs two parallel, never-sharing-dispatch UI frameworks:

| | **Framework A — GadgetClass/LinkClass gadget tree** | **Framework B — Win32 RT_DIALOG shell** |
|---|---|---|
| Used by | In-game sidebar, cameo strips, tabs, command bar, radar buttons, minimap click region, full-tactical click catcher, chat/system message labels | Main menu, single-player, skirmish setup, options, load/save, quit/validation modals, movies, WOL/LAN lobbies |
| Model | Retained-mode intrusive doubly-linked sibling list; smallest-area-wins hit-test; sticky capture; Input-driven drawing | Native modeless HWND dialogs from RT_DIALOG templates, owner-draw subclassed behind one shared wndproc, hand-pumped |
| Dispatch | `GadgetClass::Input 0x004E1640` per gameplay tick, from `GScreenClass::Input 0x004F4320` | `Process_NetworkMessages 0x005D4D50` Win32 message pump + per-class subclass WNDPROCs |
| Event cadence | engine frame (Input call rate) | OS messages + wall-clock `SetTimer` |
| Authority doc | **this study** (supersedes [GADGET_UI_FRAMEWORK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GADGET_UI_FRAMEWORK_GHIDRA_REPORT.md) where contradicted — see corrections in §2/§3) | [SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) (2026-05-31) + **this study's §5-D delta corrections** |
| Rust today | NO substrate (ad-hoc per-surface, §4) | `ui::shell` Slices 0–3/5 SHIPPED; 4/5b not (§4.3) |

**Relationship to the shipped shell substrate.** Slices 0 (geom), 1 (descriptor+layout), 2 (DialogController), 3 (paint pass) and most of 5 (modal substrate) are committed on `dev` (commit list in rust-current lane §4, verified via `git log --grep="substrate Slice"`). This study does not reopen that design. It contributes to Framework B exactly three things: (a) the dialog-delta lane's **verified corrections** to the 2026-05-31 study (record-map offset convention, 55-id include-set, accelerator registry, DWL_USER naming, owner-draw button asset truth — §5-D); (b) the **coexistence matrix** between a pumping Framework-B dialog and the Framework-A world (§5-O12); (c) shared-service boundaries (tooltips, keyboard, surfaces — §6). The shell doc's §2.4 record map and §C7 include-set should be patched from §5-D in a follow-up `/audit` pass.

The two frameworks meet in exactly three places (all VERIFIED-LIVE, §2.6/§2.8): the modal pump `0x00623120` (which decides whether the Framework-A world ticks behind a dialog), the shared ToolTipManager singleton `[0x00887368]`, and the open-shell-dialog counter `0x00A8ED8C` read by in-game code.

## 1. Verified active-YR responsibilities

What the substrate pair **owns** in a normal YR skirmish — the player-observable contract a Rust replacement must reproduce. Each row cites its verification.

### 1.1 Framework A — the gadget tree service

| # | Responsibility | Evidence |
|---|---|---|
| A1 | **Single input+draw authority for all in-game chrome.** Once per gameplay tick, `GadgetClass::Input 0x004E1640` is called on the Buttons list head `[0x00A8EF54]`; it reads the keyboard queue, computes event flags, runs hover transitions, dispatches through three exclusive tiers (sticky → keyboard focus → broadcast walk), and **draws gadgets inside the same walk** — there is no separate per-gadget draw pump apart from the per-frame dirty sweep (A10). | VERIFIED-LIVE: decompile+disassemble 0x004E1640 (gadget-core lane; verdicts `input-heldbits-idle-only`, `clickedon-sticky-kbd-bypass`) |
| A2 | **Sticky mouse capture.** Press bits acquire `g_StickyFocus [0x008B3E88]` iff the gadget's IsSticky byte is set; release bits release it (holder-only, or same-call acquirer). While captured, the holder is re-dispatched every Input tick even with masked-0 flags — this powers press-hold drag-off/drag-back visuals. | VERIFIED-LIVE: read_memory hand-decode 0x004E1970 + decompile 0x004E13F0 (verdicts `sticky-process-decode`, `clickedon-sticky-kbd-bypass`) |
| A3 | **Keyboard focus protocol.** `Set_Focus 0x004E19A0` steals focus (old holder redrawn + cleared, Flags bit 0x100 moved); keyboard-flag events (0x100) route to the focus holder tier and bypass the bounds test entirely. | VERIFIED-LIVE: decompile 0x004E19A0/0x004E19D0 (gadget-core §4); 0x100 bounds bypass per verdict `clickedon-sticky-kbd-bypass` |
| A4 | **Hover enter/leave.** `Hit_Test` runs every tick BEFORE dispatch; on change the old gadget's Mouse_Leave then the new gadget's Mouse_Enter fire. `g_HoveredGadget [0x008B3E94]` has exactly one writer (Input) and one reader (Input) program-wide; destructors do NOT clear it (stale-pointer hazard recorded in §5-G7). | VERIFIED-LIVE: get_xrefs_to 0x008B3E94 (verdict `hover-global-sole-writer`) |
| A5 | **Hit-test rule.** Half-open rects (left/top in, right/bottom out); disabled gadgets invisible; winner = smallest area with signed `<=` tie-break on a head→tail walk (equal area → LATER gadget wins); seed best-area = the .rdata constants 1024×768 = 786,432 px², not live resolution. | VERIFIED-LIVE: decompile+disassemble 0x004E15A0, read_memory 0x007F5BE8/0x007F5BF4, list_segments (verdict `hittest-seed-tiebreak`) |
| A6 | **The button machine.** Every live in-game button (sidebar tabs, repair/sell, strip scroll, 25-button command bar, radar pair) runs `ToggleClass::Action 0x00723EC0`: press = silent consume + capture; release-inside = toggle per Kind (1 = flip, 2 = latch-ON only) and fire `ID|0x8000` (`|0x4000` on right-release when masked-in); drag-off cancels via the captured flags-0 hover tracking. Result IDs are consumed layer-locally by the AI cascade (A11). | VERIFIED-LIVE: read_memory hand-decode 0x00723EC0 ×352 (verdict `toggle-action-decode`); ControlClass::Action 0x0048E5A0 decompile (gadget-core §6.2) |
| A7 | **Cameo strip click surface.** 240 SelectClass gadgets (4 tabs × 60 slots, 0x38 bytes each @ 0x00B07E80) are the clickable cameos; SelectClass is the only family member overriding Mouse_Enter/Mouse_Leave (0x006AB990/0x006AB9E0) — the gadget-level tooltip hook. Visible-subset registration swaps on tab switch via Add_A_Button/Remove_A_Button. | VERIFIED-LIVE: gadget-family §3.6 (read_memory 0x007F2FCC, decompile 0x006A4DC0); gscreen-chain §3.2 (decompile 0x006A6820) |
| A8 | **Invisible click-region gadgets.** The full-tactical-screen catcher (global 0x008A06F8, flags 0x7F, sticky) and the minimap/radar region (0x00B04A10, flags 0x9F, sticky) are Action-only gadgets with base no-op Draw_Me — ALL tactical and minimap clicks enter through the same gadget walk. | VERIFIED-LIVE: read_memory hand-decode 0x004A86E0 / 0x00652870 (verdict `live-gadget-census` evidence e/f) |
| A9 | **Chat/system message labels.** TextLabelClass gadgets (0x4C bytes) are heap-built at RUNTIME by MessageListClass::Add_Message (`FUN_005D3BA0` / `FUN_005D4210`, `operator_new(0x4C)` + ctor 0x0072A440) — fires in normal play on every chat/system message. This is the live runtime-construction path of Framework A (census correction, §3.1). | VERIFIED-LIVE: verdict `live-gadget-census` correction; decompile 0x005D3BA0 (gadget-family §3.10) |
| A10 | **Dual draw pump.** Input draws every visited gadget per tick (forced=1 on a fresh list); additionally `RenderFrame_main 0x004F4480` calls the list-walk Draw_All slot (+0x2C → 0x004E1570) with forced=0 every frame — draw-if-dirty over all registered gadgets. Draw order = list order = registration order. No third pump exists (exhaustive xref census of the head). | VERIFIED-LIVE: verdict `dual-draw-pump` (get_xrefs_to 0x00A8EF54, decompile 0x004E1570/0x004F4480) |
| A11 | **Layer-local ID consumption + the UI→sim seam.** GScreenClass::Input ends in the chain AI cascade (Mouse→Scroll→Tab→Sidebar→Power→Radar→Display→GScreen); each layer consumes its own button IDs and mutates **UI state only** (modes, scroll, flash, camera, selection). Sim-affecting clicks become queued network events; commands enter the sim via Main_Tick → `FUN_00647260` (Queue_AI-shape, name inferred) → `FUN_0064C380` (DoList-shape, name inferred) → `EventClass::Execute 0x004C6CB0`, AFTER the object-sim pass — never inside the UI layer. | VERIFIED-LIVE: cascade per verdict `ai-cascade-order` (gscreen-chain lane §4: decompile 0x005BDDC0/0x006922E0/0x006D0680/0x006A7780 + callee lists). Command entry re-verified live 2026-06-10: `get_function_callers 0x00647260` → Main_Tick 0x0055D360 sole caller; `get_function_callees 0x00647260` → includes FUN_0064C380; `get_function_callers 0x004C6CB0` → FUN_0064C380 sole caller; `decompile_function 0x0055D360` pins the FUN_00647260() call AFTER LogicClassPerTickUpdateLiveVector (LogicClass::Update 0x0055AFB0) and RenderFrame_main, before Network_ServiceLoop and the frame-counter increment; `decompile_function 0x0053B560` confirms the `Process_QueuedEvents` label there is drift (3-state screen-flash machine, no EventClass) — full log: [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §3 (verdict `queued-events-in-logic-update`). Note: gscreen-chain §5's Main_Tick step list omits the 0x00647260 call (subsumed in its step 9); reconciled in [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §3.5. |
| A12 | **Lifecycle/registration.** The Buttons head has exactly 4 writers (two zero-stubs One_Time/base-Init_IO + Add_A_Button 0x004F4410 tail-append with double-insert reject + Remove_A_Button 0x004F4450); registration happens at activate/tab-switch/toggle time; rebuild events are video-mode change (0x00560BF0), load-game (0x0067E440) and scenario/session start (TabClass::Activate 0x006D04F0). | VERIFIED-LIVE: verdict `buttons-head-writers`; gscreen-chain §3 (get_function_callers 0x006D03A0/0x006D04F0) |

### 1.2 Framework B — the shell dialog service (by reference + delta)

The ten responsibilities in [SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) §1 (create from RT_DIALOG / dual registries / subclass at INITDIALOG / re-anchor / compose / slide / pump / result channel / INI seed / teardown-with-focus-restore) all **re-verified VERIFIED this study** (dialog-delta lane §1, fifteen-row delta report — every load-bearing prior claim re-decompiled). Net-new or corrected responsibilities:

| # | Responsibility (new/corrected) | Evidence |
|---|---|---|
| B1 | **Keyboard routing is a three-stage pump pass:** registration-order `IsDialogMessageA` over the HWND vector → `TranslateAcceleratorA` over an `{HACCEL,HWND}` pair registry → optional message-filter hook that can swallow Translate/Dispatch. | VERIFIED-LIVE: verdict `kbd-routing-accel-registries` (disassemble 0x005D4D50, decompile 0x005D4E70/0x005D4ED0) |
| B2 | **Tooltip timing is owned by the Win32 pump, not the frame loop:** ToolTipManager::ProcessMessage 0x00724200 has exactly one caller (Process_NetworkMessages); 'TTIP' SetTimer with ms delay fields; region point test is **inclusive on both edges** — deliberately different from the gadget half-open rule. | VERIFIED-LIVE: verdict `tooltip-inclusive-edges` — globals-registries lane §2.4 (decompile 0x00724200), gscreen-chain lane §7 (get_function_callers 0x00724200, get_xrefs_to 0x00887368) |
| B3 | **Owner-draw button asset truth:** PCX names are always `b{u,d}e_{li,mi,ri}{24,30}.pcx` (second char hardcoded 'e'); disabled = 50% black AlphaBlend, never `bud_*` art; asset-1 = SDBTNANM.SHP frames 2 idle / 3 flash (1 Hz hover timer) / 4 pressed. | VERIFIED-LIVE: verdict `ownerdraw-button-assets` (decompile 0x00612B70, 0x00621B80; read_memory 0x0083587C..) |
| B4 | **Reposition/slide/paint gating runs off a 55-dialog-id include-set** (not the 4 ids the prior doc listed) + a 19-id mode-2 modal set, disjoint. | VERIFIED-LIVE: verdicts `include-set-55-ids`; dialog-delta §5.1/§5.2 (decompile 0x0060C540, 0x00622820) |
| B5 | **One record-offset convention:** the 0x208 per-control record's data root = bucket+4; the prior study's mixed-convention map is replaced by §2.7's single-convention table; `+0xB0` is one dual-role field (parent paint-mode AND button paint-asset); there is **no** separate `+0xB4` field. | VERIFIED-LIVE: verdict `record-data-root-convention` — dialog-delta lane headline + §3 (decompile 0x00624760; record-map decompiles). The "binary-wide byte-pattern scan: zero +0xB4 writers" sub-claim is study-session-only ([S], [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §1 #16); the lane-grade form of the same fact is dialog-delta §3's closing note ("that write IS paint-mode=1") |
| B6 | **Modal-over-game coexistence:** the pump body 0x00623120 always services Win32 messages first; offline (g_GameMode ∈ {0,5}) or blocked → network service only (sim+render+frame counter FROZEN); in-game network modes → guarded Main_Tick (sim advances, gadget input + render suppressed by the g_GameState gate). | VERIFIED-LIVE: verdict `modal-pump-matrix` — gscreen-chain lane §6.1/§6.2 (decompile 0x00623120, 0x004E1D00), dialog-delta lane #8 (decompile 0x00623120 + 3 owner loops), globals-registries lane §2.5 (decompile + disassemble 0x00623120) |


## 2. Full inventory (vtables, methods, globals, registries, static tables, legacy paths)

Condensed from the lane worknotes; every table names its lane + the MCP calls that produced it. Where this section corrects a prior doc or a lane, the correction is stated inline.

### 2.1 Framework A — GadgetClass base vtable @ 0x007E92BC (33 slots, +0x00..+0x80)

VERIFIED-LIVE: gadget-core lane §1.1 (read_memory 0x007E92BC ×144; per-slot decompiles in gadget-core §4); independently re-dumped by globals-registries lane §3.1.

| Slot | +off | Address | Role |
|---|---|---|---|
| 0 | +0x00 | 0x004E1A60 | scalar-deleting dtor |
| 1/2 | +0x04/+0x08 | 0x004E14A0 / 0x004E14B0 | Get_Next / Get_Prev (JMP thunks → 0x00556620/0x00556630) |
| 3–8 | +0x0C..+0x20 | 0x005566A0, 0x00556700, 0x005566D0, 0x00556640, 0x00556670, 0x005565F0 | LinkClass: Add(after), Add_Tail, Add_Head, Head_Of_List, Tail_Of_List, Zap |
| 9 | +0x24 | 0x004E1480 | Remove (clears focus, then LinkClass::Remove 0x00556730) |
| 10 | +0x28 | 0x004E1640 | **Input** |
| 11 | +0x2C | 0x004E1570 | Draw_All(forced) — list walk of Draw_Me (Ghidra label `LocomotionClass__ForEach_SetSlopeIndex` = drift) |
| 12 | +0x30 | 0x004E14C0 | Delete_List |
| 13 | +0x34 | 0x004E1920 | Extract_Gadget(id) |
| 14 | +0x38 | 0x00488690 | Clear_Attached_List (g_CurrentGadgetList := 0) |
| 15/16 | +0x3C/+0x40 | 0x004E1460 / 0x004E1450 | Disable / Enable (both Flag_To_Redraw + Clear_Focus) |
| 17 | +0x44 | 0x004AEBA0 | Get_ID (base: 0) |
| 18 | +0x48 | 0x004E1960 | Flag_To_Redraw |
| 19 | +0x4C | 0x0048E650 | Peer_Callback (base: no-op RET 0xC) |
| 20/21/22 | +0x50/+0x54/+0x58 | 0x004E19A0 / 0x004E19D0 / 0x004E19F0 | Set_Focus / Clear_Focus / Has_Focus |
| 23 | +0x5C | 0x004E1A00 | Any_Redraw_Pending (tail-ward walk) |
| 24 | +0x60 | 0x004886A0 | Get_IsToRedraw |
| 25/26 | +0x64/+0x68 | 0x004E1A20 / 0x004E1A40 | **Set_Position / Set_Size** (prior GADGET doc called +0x64 "Get_Rect" — WRONG, gadget-core §4) |
| 27 | +0x6C | 0x004E1550 | Draw_Me(forced) (base: dirty-flag gate) |
| 28/29 | +0x70/+0x74 | 0x004E1510 / 0x004E1520 | Mouse_Enter / Mouse_Leave (base: RET stubs) |
| 30 | +0x78 | 0x004E1970 | Sticky_Process (acquire/release g_StickyFocus) |
| 31 | +0x7C | 0x004E1530 | Action (base) |
| 32 | +0x80 | 0x004E13F0 | **Clicked_On** (per-gadget input filter + dispatch) |

**Correction vs GADGET_UI_FRAMEWORK_GHIDRA_REPORT §8:** the vtable ends at +0x80. The prior doc's "slot 33 = 0x00800AE0 terminator / slot 34 = 0x004E1AD0 LinkClass helper" misreads neighbor bytes: 0x00800AE0 is the RTTI COL of the NEXT vtable, and 0x007E9344 is the **LinkClass vtable base** (slot 0 = 0x004E1AD0) — gadget-core §1.1 (read_memory 0x00800AE0, 0x007E92B8), globals-registries §3.1 (get_xrefs_to 0x007E9344).

Field layout (gadget-core §3.3, decompile 0x004E12F0): +0x00 vtbl, +0x04 Next, +0x08 Prev, +0x0C X, +0x10 Y, +0x14 W, +0x18 H, +0x1C IsToRedraw u8, +0x1D IsSticky u8, +0x1E IsDisabled u8, +0x20 Flags u32; sizeof 0x24. ControlClass adds +0x24 ID, +0x28 Peer (0x2C). ToggleClass adds +0x2C IsPressed, +0x2D IsOn, +0x30 Kind. Object sizes from the static-init census (gadget-family §4.2): ShapeButtonClass 0x60, SBGadgetClass 0x28, SelectClass 0x38, TextLabelClass 0x4C.

### 2.2 Framework A — family vtable override matrix (20 vtables)

VERIFIED-LIVE: gadget-family lane §1/§2/§5 (get_xrefs_to 0x004E1640 and 0x004E13F0 both → the same 20 vtables; read_memory of each vtable). No family member overrides Input (+0x28), Clicked_On (+0x80), Get_Next (+0x04), or the non-virtual Hit_Test 0x004E15A0 — the dispatch spine is shared by all 20 classes.

| Class (vtable) | Slots | Overrides vs parent (slot offset: addr) |
|---|---|---|
| GadgetClass (0x007E92BC) | 33 | — base |
| ControlClass (0x007E528C) | 34 | dtor 0x0048E660; +0x44 Get_ID 0x0048E610; +0x6C Draw_Me 0x0048E620 (peer first); +0x7C Action 0x0048E5A0 (posts ID\|0x8000); **+0x84 Set_Peer 0x0048E600 (new virtual)** |
| ToggleClass (0x007E8118) | 34 | vs Control: dtor 0x004B5810; +0x7C Action 0x00723EC0 only |
| **ShapeButtonClass (0x007E8088)** | **35** | vs Toggle: dtor 0x004B57F0; +0x6C Draw_Me 0x0069DEB0; **+0x84 Set_Peer 0x0048E600 INHERITED; +0x88 Set_Shape 0x0069DE00 (NEW slot 34)**. ← **Adversarial correction C1** over gadget-core lane §9.1, which misread +0x84 as a repurposed Set_Peer. Re-verified live 2026-06-10 via read_memory 0x007E8088 ×152: +0x84=0x0048E600, +0x88=0x0069DE00, then 0x00800010 (next vtable's RTTI COL) and 0x004B5810 (= ToggleClass dtor, vtable 0x007E8118 slot 0). A Rust port may treat +0x84 as uniformly Set_Peer across the hierarchy; Set_Shape is an appended virtual. ([verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §2 C1) |
| TextButtonClass (0x007F55DC) | 37 | vs Toggle: dtor 0x00720210; +0x6C Draw_Me 0x0071FFE0; +0x84 0x00720020 (overrides the Set_Peer slot); +34..36 new 0x00720200/0x00720070/0x00720140 |
| SelectClass cameo (0x007F2FCC) | 34 | vs Control: dtor 0x006AC780; **+0x70 Mouse_Enter 0x006AB990 / +0x74 Mouse_Leave 0x006AB9E0 (only family member — tooltip hooks)**; +0x7C Action 0x006AAD00; Draw_Me NOT overridden (strip painter draws cameos) |
| SBGadgetClass (0x007F2F44) | 33 | vs Gadget: dtor 0x006AC7A0; +0x7C Action 0x006ABA40 only (invisible) |
| TextLabelClass (0x007F5B44) | 34 | vs Gadget: dtor 0x0072A670; +0x6C Draw_Me 0x0072A4A0; +33 new (Set_Text-shape) 0x0072A660 |
| Tactical-screen gadget (0x007E608C) | 33 | vs Gadget: dtor 0x004AEBB0; +0x7C Action 0x004AAC10 only (invisible) |
| RTacticalClass radar gadget (0x007F02BC) | 33 | vs Gadget: dtor 0x00658780; +0x7C Action 0x006539D0 only (invisible) |
| ListClass (0x007ED10C) | 50 | vs Control: 10 overrides (incl. +0x48 Flag_To_Redraw 0x00557FD0, +0x6C Draw_Me 0x00557920, +0x7C Action 0x00557830) + 16 new list-API slots — full list gadget-family §5 |
| CheckListClass (0x007E4F84) | 50 | vs List: dtor 0x004886E0, Action 0x004884A0, +8 others |
| ColorListClass (0x007E5054) | 52 | vs List: 5 overrides + 2 new |
| EditClass (0x007E81A4) | 39 | vs Control: +0x50 Set_Focus 0x004C3570, Draw_Me 0x004C3110, Action 0x004C3190, +5 new |
| DropListClass (0x007E7FCC) | 46 | vs Edit: 9 overrides (+0x54 ClearFocus 0x004B50A0 auto-collapse, +0x4C PeerCb 0x004B50C0 expand/collapse) + 7 new. Draw_Me/Action slots 0x004C3110/0x004C3190 are INHERITED EditClass methods (prior doc's "DropListClass::Draw_Me/Action" naming = identity drift) |
| GaugeClass (0x007E9384) | ≥42 | vs Control: Draw_Me 0x004E2690, Action 0x004E2830 (sticky-held thumb drag), +6+ new gauge API — **EXISTS; prior GADGET doc §1.1 "no GaugeClass in binary" REFUTED** (RTTI `.?AVGaugeClass@@` @ 0x00822868; gadget-core §9, gadget-family §3.1) |
| TriColorGaugeClass (0x007E9430) | as Gauge | Draw_Me 0x004E2B50 only — RTTI-confirmed (gadget-core §9) |
| SliderClass (0x007ED21C) | 44 | vs Gauge: +0x4C PeerCb 0x006B2160, Draw_Me 0x006B20F0, Action 0x006B1F50, 6 more — proves SliderClass : GaugeClass : ControlClass (Gauge-ctor call 0x006B1B44) |
| StaticButtonClass (0x007F3EA0) | 36 | vs Gadget: Draw_Me 0x006C6640 + 3 new (owns a pixel buffer) |
| Dial8Class (0x007E5E3C) | 34 | vs Control: Draw_Me 0x004A57B0, Action 0x004A5660 — **EXISTS; prior doc "fully stripped from YR" REFUTED** (gadget-family §3.3); DORMANT (§3) |

### 2.3 Framework A — method inventory (non-virtual core + helpers)

All VERIFIED-LIVE in gadget-core lane (§2–§7) unless noted.

| Address | Method | One-line contract |
|---|---|---|
| 0x004E1640 | GadgetClass::Input(list_head) | the per-tick dispatch authority — full 9-step contract in §5 G5–G13 (decompile + disassemble) |
| 0x004E15A0 | Hit_Test(head, x, y) | half-open rects, skip disabled, smallest-area `<=` tie-break, seed = 1024×768 consts (§5 G14) |
| 0x004E13F0 | Clicked_On | mask-first filter; sticky/keyboard bypasses (§5 G15) |
| 0x004E12F0 | GadgetClass ctor | (x,y,w,h,flags,sticky); sticky → Flags \|= 5 |
| 0x004E1390 / 0x004E1A60 | ~GadgetClass / scalar dtor | clear sticky/keyboard/current-list globals if self; NOT hover (§5 G24). Both Ghidra-labeled `GadgetClass__Constructor` = drift |
| 0x004E1970 | Sticky_Process(flags) | press 0x11 acquires iff IsSticky; release 0x44 releases holder-only (byte-decoded; no DB function) |
| 0x004E19A0 / 0x004E19D0 / 0x004E19F0 | Set_Focus / Clear_Focus / Has_Focus | focus steal protocol (§5 G18) |
| 0x004E1530 / 0x004E1550 / 0x004E1960 | base Action / Draw_Me / Flag_To_Redraw | consume-any-flags + Sticky_Process; dirty-gate; set dirty |
| 0x004E1570 / 0x004E1920 / 0x004E14C0 | Draw_All / Extract_Gadget / Delete_List | list-walk draw; find-by-id; rewind-to-head destroy-forward |
| 0x005566A0 / 0x00556700 / 0x005566D0 / 0x005565F0 / 0x00556730 | LinkClass Add / Add_Tail / Add_Head / Zap / Remove | every insert self-Removes first; Zap = no neighbor repair; ~LinkClass = 0x005565A0 (label `LinkClass__Constructor` = drift) |
| 0x0048E520 / 0x0048E5A0 / 0x0048E620 / 0x0048E600 | ControlClass ctor / Action / Draw_Me / Set_Peer | Action posts ID\|0x8000 (+\|0x4000 right-release iff mask has 0x10); does NOT hardcode sticky (correction vs prior doc §6.1); 0x0048E550 is NOT a function entry (prior "Route_Event helper" claim wrong) |
| 0x00723E60 / 0x00723EC0 / 0x00723EA0 / 0x00723EB0 | ToggleClass ctor / Action / Turn_On / Turn_Off | ctor hardcodes flags=5, sticky=1; Action = the §5 G22 button machine (byte-decoded ×352) |
| 0x0069DCF0 / 0x0069DD30 / 0x0069DEB0 / 0x0069DE00 | ShapeButtonClass ctorA / ctorB / Draw_Me / Set_Shape | Draw_Me also devirtualized-called from Radar/Sidebar/MainGame draw code (gadget-core §9.1) |
| 0x004F4320 / 0x004F4410 / 0x004F4450 / 0x004F43F0 / 0x004F42F0 | GScreenClass::Input / Add_A_Button / Remove_A_Button / Is_A_Button / Flag_To_Redraw | chain slots 9/12/13/11/14; Add = tail-append with double-insert reject (gscreen-chain §1/§2; `Hide_Cameo_Slots` label on 0x004F4450 = drift) |
| 0x004E2830 | GaugeClass::Action | canonical sticky-held drag consumer: gate `(flags&1) \|\| ((flags&2) && this==g_StickyFocus)` (gadget-core §9) |
| 0x005D3BA0 / 0x005D4210 | MessageListClass Add_Message paths | heap-build TextLabelClass per message (§3.1) |

### 2.4 Framework A — singleton state (writer censuses exhaustive per Ghidra xref DB)

From gadget-core §8 + globals-registries §1 (get_bulk_xrefs).

| Global | Addr | Role | Writers (complete) |
|---|---|---|---|
| g_StickyFocus | 0x008B3E88 | mouse-capture gadget | Sticky_Process acquire/release; Input fresh-list reset; both dtors (null-if-this). Extra reader: GaugeClass::Action 0x004E285C |
| g_CurrentGadgetList | 0x008B3E8C | reset-detector head (no other consumer) | Input; Clear_Attached_List 0x00488690; dtors |
| g_KeyboardFocus | 0x008B3E90 | keyboard-event routing | Set_Focus/Clear_Focus; Input fresh-list reset; dtors |
| g_HoveredGadget | 0x008B3E94 | hover enter/leave driver | **Input is the sole writer AND sole reader; dtors do NOT clear (G7 hazard)** |
| Buttons head | 0x00A8EF54 | THE in-game gadget list | exactly 4 writers: 0x004F42A0, 0x004F42E0 (zero-stubs), Add_A_Button 0x004F4410, Remove_A_Button 0x004F4450 |
| Hit_Test seed dims | 0x007F5BE8=1024 / 0x007F5BF4=768 | constant best-area seed 786,432 px² | zero writers — NOT live resolution (prior GADGET doc §3 "screen width/height" = WRONG) |
| WWKeyboard ptr | 0x0087F770 | event queue; Check 0x0054F000 / Get 0x0054F050 / Down 0x0054F5C0; event coords at +0/+4 | init-time writers 0x006BC2AE/B6, 0x006BEA78 |
| WWMouse ptr | 0x00887640 | live mouse X/Y (vtbl +0x2C/+0x30), cursor draw (+0x3C/+0x40) | WinMain 0x006BDF25, Set_Video_Mode. NOT the display chain (g_DisplayChain label = drift; chain instance = static @ 0x0087F7E8, gscreen-chain §1.1) |
| Modifier VK pairs | 0x00A8EBF8..0x00A8EC0C | SHIFT=1/CTRL=2/ALT=4 modifier word pairs (OptionsClass +0x98..+0xAC) | sole writer OptionsClass::SetDefaults 0x005FA350 (defaults VK 0x10/0x11/0x12) |

### 2.5 Framework A — live gadget population (static objects + IDs)

From gadget-family §4.2 (hand-decoded CRT initializers) + gscreen-chain §3.1 (Init_IO ID assignment):

| Global(s) | Count×size | Class | Identity / IDs |
|---|---|---|---|
| 0x00B07C48 | 4×0x60 | ShapeButtonClass | 4 sidebar tab buttons (IDs 0xCB..0xCE) |
| 0x00B07DF8, 0x00B0B328, 0x00B0B3A0, 0x00B0B408 | 4×0x60 | ShapeButtonClass | sidebar singles: repair/sell pair IDs 0x65/0x66; strip scroll IDs 0xC9/0xC8, Flags=0x55 (per-global binding partially YELLOW, §9) |
| 0x00B07E58 | 1×0x28 | SBGadgetClass | sidebar body click zone (invisible) |
| 0x00B07E80 | 240×0x38 | SelectClass | cameo click gadgets (4 tabs × 60 slots; CRT init 0x006A4DC0) |
| 0x00B04978, 0x00B04910 | 2×0x60 | ShapeButtonClass | radar-frame mode buttons |
| 0x00B04A10 | 1 | RTacticalClass | minimap click region, flags 0x9F, sticky |
| 0x00B0C1C0 | 25×0x60 | ShapeButtonClass | command bar array (IDs 0x80D6..0x80EE dispatch range) |
| 0x00B0CCB0, 0x00B0CC40 | 2×0x60 | ShapeButtonClass | sidebar collapse/expand toggles, IDs 0xF0/0xF1 |
| 0x008A06F8 | 1 | Tactical-screen gadget | full-tactical click catcher, flags 0x7F, sticky (init 0x004A86E0) |
| heap | n×0x4C | TextLabelClass | chat/system message labels (§3.1) |

### 2.6 Framework B — registries, singleton state, and the modal pump

From dialog-delta §6 + globals-registries §2 (decompiles + censuses):

| State | Addr(s) | Role |
|---|---|---|
| dialog LIFO stack | 0x00B72D28 {HWND,id} stride 8; depth 0x00B72F50; top mirrors 0x00B72F44/48 | display/focus stack; factory 0x00622650 push, teardown 0x00622720 memmove-compact + focus restore; WOL-side factory 0x00775700 shares it |
| keyboard-routing vector | object 0x00ABFC90 (Items 0x00ABFC94, count 0x00ABFCA0) | IsDialogMessageA scan in registration order; append 0x005D4E70 / prune 0x005D4ED0 |
| accelerator registry | 0x00ABFCBC {HACCEL,HWND}[], count 0x00ABFCC8; hook ptr 0x00ABFD34 | second + third stages of the pump's keyboard pass (B1) |
| owner-draw hashtables | 0x00AC18C0 (HWND→subclass WNDPROC), 0x00AC1B48 (HWND→orig wndproc), 0x00AC1B00 (HWND→0x208 record; chain at bucket+0x204) | populated by subclass pass 0x0060F9A0; consumed by shared wndproc 0x00610CA0 |
| topmost/modal-exclusion array | 0x00AC1DE8 (count 0x00AC1DE0) | input exclusion for stacked dialogs (msg 0x4A9 push) |
| reentrancy guard table | 0x00AC1858 | (msg,hwnd)-keyed re-entrant message drop |
| paint bookkeeping | depth 0x00AC48DC; union dirty rect 0x0083367C/0x00833680/0x00AC48E0/0x00AC48E4 | end-of-paint front blit Alternate 0x00887310 → front 0x00887308 |
| theme colors | 0x00AC18A4=0xFFFF yellow, 0x00AC1CB4 disabled red, etc. | single-writer-at-init (inside 0x0060F9A0) |
| **open-shell-dialog counter** | **0x00A8ED8C** | ++ at DLGPROC WM_INITDIALOG / WOL factory, −− at WM_DESTROY; **read by in-game code: GScreen flip region 0x004F4B5A and sidebar 0x006A6A00** — framework meeting point #3 |
| **modal pump body** | **0x00623120** | Process_NetworkMessages 0x005D4D50 ALWAYS first; offline {0,5} or blockers 0x00A8D60E/0x00A8DAB4 → Network_ServiceLoop 0x0048D080 only; else guarded Main_Tick via reentrancy byte DAT_00ABCD58 — framework meeting point #1; full coexistence matrix in §5 O12 |
| pump activity counter | 0x00AA0430 | ++ per pump entry in every pump variant |

Framework B method inventory (all decompiled in dialog-delta §1/§2): factory 0x00622650 (CreateDialogIndirectParamA, never DialogBoxParam), common DLGPROC 0x00622B50, init bridge 0x00622820, subclass classifier 0x0060F9A0 (Win32-class strcmp cascade → 16 owner-draw WNDPROCs, dialog-delta §4), shared wndproc 0x00610CA0, WM_PAINT composer 0x00621E90 (mode 1 shell / mode 2 modal SHP / mode 0 dbak6440.pcx fallback), teardown 0x00622720, slide engine 0x006071E0 + triggers 0x00608260 (in) / 0x00608070 (out) / re-arm 0x00608380, reposition pass 0x0060C4A0 + include test 0x0060C540, record initializer 0x00623340, record-lookup helper 0x00624760 (returns bucket+4 = data root), Main_Game state machine 0x0052D9A0, dialog runners 0x00531CC0 (main menu) / 0x00558DD0 (load/save/delete; `CDFileClass__Constructor` label = drift).

### 2.7 Framework B — the 0x208 record map (single convention, data root = bucket+4)

Adversarial verdict `record-data-root-convention` (B5): bucket+0x00 = HWND key, bucket+0x04..0x203 = data, bucket+0x204 = hash chain. ALL offsets below data-root-relative; this supersedes the mixed-convention map in [SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) §2.4 (patch obligation §5-D1). Key rows (full 26-row table: dialog-delta lane §3):

- +0x10 lazy offscreen BSurface; +0x14/+0x18 custom image handles; +0x28/+0x2C owned wide text + dirty flag; +0x38 focus flag; +0x64 font; +0x68 control-kind 0..0xB; **+0x6C dialog resource id** (resolves the prior doc's +0x70-vs-+0x6C hedge: ONE field);
- **+0xB0 dual-role: parent paint-mode (1 shell / 2 modal SHP / other → dbak6440.pcx) AND button paint-asset (0 PCX / 1 SDBTNANM / 2 / 3 modal-OK)** — writers 0x0060C540 (=1), 0x00622820 (=2), FUN_0060A330 (=1/2/3); there is no separate "+0xB4 slide-eligible" field — that write IS paint-mode=1;
- +0xBC paint-suppress; +0xBD slide-IN gate; +0xBE deferred-slide pending; +0xC4 hover-active (arms 1000 ms timer); +0xC5 1 Hz flash phase; +0xD5..+0xD8 chrome-overlay markers by dialog-id set; +0xE0/+0xE4 background SHPs; +0xE8 bit0 pressed/checked; +0x1FC first-paint slide state machine 1→2→3.

### 2.8 The shared ToolTipManager — where the two frameworks meet

The frameworks meet in exactly three places (all VERIFIED-LIVE): the **modal pump 0x00623120** (§2.6; decides whether the Framework-A world ticks behind a dialog), the **open-shell-dialog counter 0x00A8ED8C** (§2.6; in-game code reads it), and the **ToolTipManager singleton ptr [0x00887368]** described here.

- Identity + behavior: globals-registries §2.4 (decompile 0x00724200) — 'TTIP' timer id 0x54544950; WM_MOUSEMOVE arms SetTimer with delay this[+0x228] unless suppression byte 0x00A8F7D8; WM_TIMER walks rect array this[+0x238] (count this[+0x244]) with an **inclusive-both-edges** point test; button messages kill+hide; enable gate this[+0x0C].
- Single pump ownership: ProcessMessage 0x00724200's ONLY caller is Process_NetworkMessages 0x005D4D50 (gscreen-chain §7, get_function_callers) — tooltip cadence is wall-clock OS-message time, never the frame loop; the engine frame loop only DRAWS it (RenderFrame_main slot-3 hook at 0x004F4562).
- Registrants from BOTH frameworks (~70 readers, get_xrefs_to 0x00887368): SidebarClass__InitSurface (13 sites), PowerClass, radar 0x00654320, TabClass::Activate (register/unregister by id via FUN_00724730 on collapse/expand), shell dialogs, Main_Game, scenario init.

### 2.9 Static id tables (Framework B)

Dumped live in dialog-delta §5: the 55-dialog-id include-set of 0x0060C540 (§5.1, full list) + the disjoint 19-id mode-2 modal set of 0x00622820 (§5.2) — verdict `include-set-55-ids`, supersedes the prior doc's 4-id list (patch obligation §5-D2); the background table 0x0060CF00 (id → convert/small/large SHP; default = MNSCRN family); the re-anchor predicate 0x00608CD0 ((dialogId, ctlId) → bool; title ctl 0x694 ~47 ids); the modal-OK predicate 0x00609E20 (paint-asset 3); the tooltip map 0x006040B0 (50 dialog ids / 381 `STT:*` CSF keys); chrome-overlay marker id sets (+0xD5/+0xD6/+0xD7/+0xD8, dialog-delta #15).

### 2.10 Legacy-path inventory + prior-doc supersession ledger

**Dormant Framework-A wing** (full census evidence in §3): ListClass, DropListClass, EditClass, CheckListClass, ColorListClass, SliderClass, GaugeClass, TriColorGaugeClass, TextButtonClass, StaticButtonClass, Dial8Class — fully linked, reachable only from each other; plus the **Dropship Loadout screen** (TS legacy, §3.3). **WOL/LAN Framework-B wing**: live code, dormant service (servers offline) — dialog-delta §7 census; enumerate, never implement as default.

Supersessions of [GADGET_UI_FRAMEWORK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GADGET_UI_FRAMEWORK_GHIDRA_REPORT.md) established by this study: (1) GaugeClass and Dial8Class DO exist (§2.2); (2) base vtable is 33 slots, slot-33/34 claims were neighbor-data misreads (§2.1); (3) +0x64 is Set_Position, not Get_Rect (§2.1); (4) held/up event bits are idle-tick-only (§5 G8); (5) §12's "0x8065/0x8066 → strip scroll" routing is wrong — those are repair/sell; scroll is 0x80C9/0x80C8 (gadget-core §10); (6) DAT_007F5BE8/F4 are constants, not screen dims (§2.4); (7) ControlClass ctor does not hardcode sticky/flags (§2.3). Supersessions of [SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) are itemized as §5-D patch obligations.

## 3. Active-YR vs inactive/legacy census

### 3.1 Runtime-construction correction (adversarial correction C2)

The live gadget population is NOT built purely by CRT static initializers. **TextLabelClass gadgets (0x4C bytes) are heap-built at runtime** by MessageListClass::Add_Message — `FUN_005D3BA0` (and the second add path `FUN_005D4210`): walks a linked list of up to 14 (0xE) label slots, `operator_new(0x4C)` + ctor 0x0072A440 per message, VocClass__PlayAtPos on insert, recursive wrap for long text. Fires on every chat/system message in normal play — the one live runtime-construction path of Framework A. VERIFIED-LIVE: gadget-family §3.10 (decompile_function 0x005D3BA0; get_function_callers TextLabelClass ctor → 0x005D3D5B, 0x005D430B); [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §2 C2.

### 3.2 Census method + completeness argument

gadget-family §1/§4: (a) three independent xref sweeps (Input 0x004E1640, Clicked_On 0x004E13F0, ctor chains) agree on the same 20 vtables; (b) because UI translation units are heavily un-disassembled in the Ghidra DB (xrefs under-report), the on-disk retail gamemd.exe (5,286,504 bytes; sanity-anchored byte-for-byte to Ghidra at the GadgetClass vtable) was scanned **exhaustively for every E8 call / E9 jump targeting each family ctor plus every absolute imm32 reference** (vector-ctor/CRT-table patterns). Complete over direct transfers; residual risk = register-indirect ctor calls only (implausible for VC++6 ctors; recorded in §9).

### 3.3 Per-class ACTIVE/DORMANT table (instantiation evidence per class)

From gadget-family §4.1/§6:

| Class | ctor | Instantiation evidence | Verdict |
|---|---|---|---|
| GadgetClass / ControlClass / ToggleClass | 0x004E12F0 / 0x0048E520 / 0x00723E60 | base subobjects of the live classes only | **ACTIVE (substrate)** |
| ShapeButtonClass | A:0x0069DCF0, B:0x0069DD30 | ctorA: 10 direct callers — radar init 0x006528D5/0x00652925, sidebar init 0x006A4C65/CA5/CEE/D45/D85, commandbar init 0x006CFBAE/C05/C45 → 37 live globals (4 tabs + 4 sidebar singles + 2 radar + 25 cmdbar + 2 cmdbar singles). ctorB callers are all dormant (dropship ×4, DropList, ListClass ×2, Slider ×2) | **ACTIVE** |
| SelectClass (cameo) | CRT init 0x006A4DC0 | referenced from the CRT static-init table entry @ 0x00814B00; builds 240×0x38 @ 0x00B07E80; default-ctor variant 0x006AACB0 has zero callers | **ACTIVE** |
| SBGadgetClass | inline @ ~0x006A4C10 | GadgetClass-ctor call 0x006A4C21 + vtable write 0x006A4C31 → global 0x00B07E58; default ctor 0x006A4E40 zero callers | **ACTIVE** |
| TextLabelClass | 0x0072A440 | 2 callers: 0x005D3D5B / 0x005D430B (MessageList heap path, §3.1) | **ACTIVE** |
| Tactical-screen gadget | static init 0x004A86E0 | global 0x008A06F8, flags 0x7F, sticky; re-added to Buttons at scenario init when !g_IsMapEditor | **ACTIVE** |
| RTacticalClass | static init 0x00652870 | global 0x00B04A10, flags 0x9F, sticky | **ACTIVE** |
| ListClass | ctorA ~0x0055725x: 0 callers; ctorB 0x00557380: 1 caller = DropList ctorB (dead) | — | **DORMANT** |
| DropListClass | ctorA: 0; ctorB 0x004B53E0: 0 | — | **DORMANT** |
| EditClass | 0x004C2FC0: 1 caller = DropList ctorA (dead) | — | **DORMANT** |
| CheckListClass | 0x00488280: 0 | — | **DORMANT** |
| ColorListClass | ~0x004887xx: 0 | — | **DORMANT** |
| SliderClass | 0x006B1B20: 1 caller = ListClass ctorA (dead); inlined copy lives in ListClass ctorB (itself dead) | — | **DORMANT** |
| GaugeClass | 0x004E2500: 1 caller = Slider ctor (dead) | exists (RTTI-proven, §2.2) | **DORMANT** |
| TriColorGaugeClass | 0x004E2A50: 0 (vtable has zero data refs) | — | **DORMANT** |
| TextButtonClass | 0x0071FF20: 0 (vtable imm32 hits only its own ctor/dtor) | — | **DORMANT-leaning** (unless a non-disassembled caller exists — §9) |
| StaticButtonClass | 0x006C6540 / 0x006C65D0: 0 | — | **DORMANT** |
| Dial8Class | 0x004A53B0: 0 (single vtable imm32 hit = own ctor) | exists (refutes prior doc) | **DORMANT** |

**Headline:** the entire TS shell-control wing of Framework A is linker-retained dead code — RA2/YR replaced the TS gadget shell with Win32 RT_DIALOG screens (Framework B). The LIVE Framework-A surface is exactly 9 classes: the three bases + ShapeButtonClass, SelectClass, SBGadgetClass, TextLabelClass, the tactical-screen gadget, and RTacticalClass.

### 3.4 Dropship Loadout screen — TS legacy, gated DORMANT

`FUN_004B6C30` (Dropship.cpp strings; Ghidra label `CDFileClass__Constructor` = drift) builds 4 ShapeButtons; single caller = Start_Scenario 0x00683D97 gated on `ScenarioClass+0x34D0 > 0` (StartingDropships — INFERRED binding, §9). Dormant in YR unless a map sets the key; do NOT implement as default (gadget-family §3.12).

### 3.5 Framework B dialog census (offline YR lens)

dialog-delta §7: **ACTIVE offline** — 0xE2 main menu, 0x100 SP, 0x102 skirmish + 0x6B choose-map, 0x94 campaign select, 0xB7/0x2B4/0x2B5 load/save/delete, options family (0xB5, 0xBBA, 0xBBB, 0xFF, 0xF5, 0xD5), 0x101 movies/credits, mode-2 modal family (quit-confirm, 0xCE/0x120/0x121), in-game shell variants (LeftPanel + paint-asset 2 when in-game). **LIVE CODE, ONLINE-DEAD** — the ~39-id WOL/ladder family (servers offline; enumerate, never silently drop, do not implement as default). **LAN/IPX** — 0xBC/0xBD/0xC2/0xC9 live with IPX only. **REFUTED/DEAD** — `bud_*`/`bdd_*` disabled art (disabled = AlphaBlend); 0x4DC hover message has no shell sender for main-menu buttons.

## 4. Comparison against current Rust architecture (gap table)

All Rust claims VERIFIED against the working tree at commit 7b79a186 (`dev`) by the rust-current lane (file:line cited per row). Binary-side contract references point at §5 clauses.

### 4.1 Architecture map (classification summary)

- **SUBSTRATE (ui::shell, shipped):** `src/ui/shell/{geom,descriptor,layout,controller,modal}.rs`, `src/render/shell_paint.rs`; descriptor-driven layout only for 0xE2 (`main_menu_shell`); 0x100 uses shared geom helpers (no descriptor); validation modal 0xCE routes through `DialogController`.
- **AD-HOC (Framework B):** the whole skirmish 0x102 board (`src/ui/skirmish_shell/` ≈ 2.2k ln + bespoke gesture handling app.rs:1394-1522), `app_skirmish_shell_render/*`, `app_shell_transition.rs`, host glue `app.rs`.
- **AD-HOC (Framework A):** `src/sidebar/mod.rs` (hit-test + actions), `sidebar_view.rs`, `app_input.rs` (mouse-DOWN dispatch), `app_sidebar_render`/`render/sidebar_chrome` — **no retained gadget list, no capture, no tooltips anywhere in-game**.
- **Faithful primitives (keep):** `sidebar/gadget_flash.rs`, `app_sidebar_gadgets.rs`, `sidebar/power_bar_anim.rs` (one admitted placeholder constant), `skirmish_shell/static_reveal.rs`.
- **EGUI placeholders:** `ui/pause_menu.rs`, `ui/mission_status.rs`, `ui/main_menu_dialogs.rs` (options/movies/campaign), `ui/main_menu.rs` fallback.
- **DEAD:** `ui/in_game_hud.rs` (`draw_in_game_hud` has zero callers).
- Layering invariant holds: `src/sim/` has zero imports of ui/render/sidebar/audio/net (rust-current §1.4 grep).

### 4.2 DRIFT gap table

Severity = player-visibility × trigger frequency (CLAUDE.md rule); every row names its frequency.

| # | Gap (Rust today) | gamemd contract | Trigger frequency | Verdict |
|---|---|---|---|---|
| D-A1 | **Sidebar actions fire on mouse-DOWN** (app_input.rs:39-43 → handle_sidebar_mouse_input app_input.rs:227-238) | silent press + capture, fire `ID\|0x8000` on RELEASE-inside, drag-off cancels (§5 G22) | **every sidebar click of every match** — tabs, repair, sell, cameos, scroll | DRIFT (top fix) |
| D-A2 | **No in-game tooltip surface at all** (rust-current §2.4: no cameo name/cost tooltip, no delay timer) | shared ToolTipManager: 1000 ms wall-clock delay, inclusive-edge rects, kill-on-press (§2.8, §5 S1) | every hover over a cameo/button — continuous in normal play | DRIFT (GAP) |
| D-A3 | **Hit-testing is first-match-in-feed-order with mixed edge conventions** — shell `RectPx::contains` right/bottom-exclusive (geom.rs:34-36) vs sidebar `Rect::contains` right/bottom-INCLUSIVE (sidebar/mod.rs:61-63); no area tie-break anywhere (controller.rs:210-212; sidebar/mod.rs:379-425) | half-open rects, smallest-area-wins, `<=` tie-break (later wins), 1024×768 seed (§5 G14) | edge-pixel clicks on the sidebar fire on every boundary hit; ordering divergence latent until any overlap ships — structural risk | DRIFT |
| D-A4 | **No sticky-capture / hold-repeat substrate** — only bespoke booleans (trackbar thumb, minimap drag); dropdown arrows scroll once per press (combos.rs:645-656); no per-tick re-dispatch of a held gadget | sticky capture (G17), masked-0 re-dispatch hover tracking (G22), hold-repeat as a mask property (G23) | every press-hold interaction: drag-off cancel attempts, gauge drags, held scroll arrows (e.g. the 9-item color combo) | DRIFT |
| D-A5 | **Chat/system message TextLabel surface unimplemented** | heap TextLabelClass per message, 14-slot list, Voc on insert (§3.1) | every system notification; every chat line in MP | DRIFT (GAP) |
| D-B1 | Skirmish 0x102 controls entirely off-substrate (bespoke state machines, combos.rs/trackbars.rs) | one control substrate per §5-D + owner-draw census §2.6 | whole skirmish screen, every session | known gap = B-track Slice 4 |
| D-B2 | Tooltip delay absent in shells too: 0xE2 tooltip emitted immediately (app_main_menu_shell_render.rs:155-162); `hover_started_at` armed but consumed only by the 0x100 hover flash | 1000 ms 'TTIP' delay before show (§5 S1) | every menu hover | DRIFT |
| D-B3 | `DialogController::on_key` placeholder (Enter/Esc-as-dismiss only, controller.rs:192-194); exit-confirm Esc bypasses controller and never pops the stack (app.rs:2106-2112, 1950-1955) | three-stage keyboard routing + LIFO focus restore (§5 D3, prior doc C3/C5) | every keyboard interaction with dialogs | DRIFT + internal inconsistency |
| D-B4 | Slice-3 mirror retirement not landed: render still reads per-shell `pressed/hovered` mirrors (app.rs:1556-1578; app_single_player_shell_render.rs:219-231) with a stale "retired in Slice 3" comment | single input authority | plumbing-only today; divergence risk on every future edit | cleanup debt |
| D-B5 | `ui::shell::controller` uses wall-clock `std::time::Instant` (controller.rs:19,184) | acceptable for menus (non-sim), but makes shell behavior non-replayable in tests | testing-only | note |

### 4.3 Shell substrate slice status (B-track)

From rust-current §4 (`git log --grep="substrate Slice"`): Slice 0 geom e1b50ec4 SHIPPED; Slice 1 descriptor+layout 21d3341a SHIPPED; Slice 2 DialogController 71b9a3de SHIPPED; Slice 3 paint pass 32f066f0 SHIPPED (mirror retirement pending, D-B4); **Slice 4 skirmish controls NOT SHIPPED** (kickoff doc exists; 87-test safety net must stay green); Slice 5 modal substrate LARGELY SHIPPED (d355d495, b3d39232, 31cbacbf, 1be4e2ff, 87a7e598, 76826114, 635423cd, 54de2fd3); **Slice 5b options NOT SHIPPED** (`ModalKind::InGameOptions` has zero consumers).

### 4.4 What the binary census buys the Rust plan

The §3 census shrinks the Framework-A replacement surface to 9 classes and 6 behaviors (button machine, cameo strip, invisible catchers, text labels, focus/capture core, tooltip draw hook) — the TS shell-control wing (ListClass etc.) needs NO Rust counterpart; its Framework-B equivalents are already the ui::shell roadmap (Slice 4/5b).

## 5. gamemd-native BEHAVIOR CONTRACT

Stable, citable clause IDs. G* = gadget core (Framework A base machinery; source: gadget-core lane §12, adopted with its numbering). O* = orchestration (GScreen chain / frame cycle; source: gscreen-chain lane §9, with O9 corrected per verdict `queued-events-in-logic-update`). D* = Framework-B delta corrections that must ALSO be patched into [SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) (follow-up /audit). S* = shared cross-framework services. "Tick" = one GadgetClass::Input call on a list head.

### 5.1 G-series — gadget service core [evidence: gadget-core lane §§2–10]

- **G1 — Storage.** A gadget list is an intrusive doubly-linked sibling list (Next +0x04 / Prev +0x08); no parent/child tree, no z-index field.
- **G2 — Insertion.** `Add(after)` inserts immediately after a node; `Add_Tail` appends; `Add_Head` prepends; every insert implicitly Removes the node from any previous list first.
- **G3 — Removal.** `Remove` repairs both neighbors and returns the recomputed head; `Zap` zeroes own links WITHOUT neighbor repair; GadgetClass::Remove additionally Clear_Focus()es itself first.
- **G4 — Construction defaults.** GadgetClass(x,y,w,h,flags,sticky): geometry+mask+IsSticky set, rest zeroed; `sticky → Flags |= 0x05`. ControlClass adds ID/Peer. ToggleClass hardcodes flags=5, sticky=1.
- **G5 — Fresh-list reset.** A different head than the previous Input call nulls g_StickyFocus and g_KeyboardFocus, stores the new head, and force-draws every gadget this tick. g_CurrentGadgetList has no other consumer.
- **G6 — Coordinate source.** Mouse-button events (low byte 1/2: codes 0x001/0x002/0x801/0x802) use the event-queue coords at WWKeyboard+0/+4; keyboard events and idle ticks use live mouse X/Y from WWMouse vtbl +0x2C/+0x30.
- **G7 — Hover transitions.** Hit_Test runs every tick BEFORE dispatch; on change, old gadget's Mouse_Leave fires, the global updates, then new gadget's Mouse_Enter. Base impls are RET stubs. **g_HoveredGadget is written nowhere else — including destructors: destroying a hovered gadget leaves a stale pointer that the next hover-change tick will call Mouse_Leave on** (gamemd only avoids the crash by destroying lists between modal loops; a Rust port must clear hover on removal). [cited by §1.A4]
- **G8 — Event flags.** 0x1 LEFTPRESS, 0x4 LEFTRELEASE, 0x10 RIGHTPRESS, 0x40 RIGHTRELEASE from the queued event; 0x2/0x8 LEFTHELD/LEFTUP and 0x20/0x80 RIGHTHELD/RIGHTUP polled ONLY on no-event ticks; a queued non-mouse event yields exactly 0x100 KEYBOARD. Never both event-bits and held-bits in one tick.
- **G9 — Modifier word.** SHIFT=1, CTRL=2, ALT=4 (configurable 2-key pairs, defaults VK 0x10/0x11/0x12 from OptionsClass::SetDefaults 0x005FA350), polled fresh each tick; passed as 5th Handle_Input arg ONLY in the broadcast walk — hardwired 0 for sticky- and focus-tier dispatch.
- **G10 — Dispatch precedence.** sticky > keyboard-focus > broadcast walk; tiers exclusive per tick. Keyboard-focus tier requires `flags & 0x100`; non-keyboard events skip it entirely.
- **G11 — Sticky/focus tier draw cadence.** The dispatched gadget gets Draw_Me(0) immediately before AND after its Handle_Input (post-draw re-reads the global; a gadget that released capture still gets its post-draw).
- **G12 — Broadcast walk.** Head→tail; every visited gadget gets Draw_Me(list_changed) BEFORE dispatch; disabled gadgets are drawn but not dispatched; the first Handle_Input returning non-zero stops the walk and gets one extra Draw_Me(0); gadgets after the consumer get neither call this tick.
- **G13 — Return value.** Input returns the 16-bit key code, possibly rewritten via the &key out-param: ControlClass posts `ID|0x8000`, plus `|0x4000` iff RIGHTRELEASE fired AND the gadget's mask contains RIGHTPRESS. ID==0 posts 0.
- **G14 — Hit-test rule.** Half-open rects (left/top in, right/bottom out); disabled gadgets invisible; winner = smallest area with signed `<=` tie-break on a head→tail walk (equal area → LATER gadget wins); seed best-area = the constants 1024×768 = 786,432 px² (not live resolution) — a gadget with area > seed can never win.
- **G15 — Per-gadget event filtering.** Clicked_On masks flags by the gadget's Flags first; early-out (0) unless: gadget is the sticky holder (always dispatches, even masked-0), or masked flags contain 0x100 (keyboard bypasses bounds), or masked flags ≠ 0 AND the point is inside the half-open rect.
- **G16 — Base Action.** Consumes ANY non-zero masked flags: sets IsToRedraw, runs Sticky_Process, returns 1. Returns 0 only for masked-0.
- **G17 — Sticky capture protocol.** Press bits (0x11) acquire g_StickyFocus iff IsSticky; release bits (0x44) release iff this holds capture (or same call that acquired). Runs on every Action via the base chain.
- **G18 — Keyboard focus protocol.** Set_Focus steals: old holder gets Flag_To_Redraw + Clear_Focus (its Flags bit 0x100 cleared), new holder gets Flags|=0x100. Clear_Focus is self-conditional; Has_Focus is pointer equality. Enable, Disable, Remove and destruction all force Clear_Focus.
- **G19 — Redraw flags.** Flag_To_Redraw sets the local IsToRedraw byte only; Draw_Me(0) no-ops unless dirty then clears the bit; Enable/Disable set dirty unconditionally; Any_Redraw_Pending scans tail-ward only.
- **G20 — Draw order/driver.** Drawing is driven by the same Input walk (head→tail) — tail-ward gadgets render on top, consistent with G14's later-wins tie-break. Draw_All (+0x2C) exists for the per-frame dirty sweep; engine chrome additionally force-draws specific buttons via devirtualized Draw_Me calls outside Input. No full-frame clear.
- **G21 — ControlClass layering.** Draw_Me draws the Peer (unforced) before itself; Action posts the ID per G13, notifies the Peer via Peer_Callback(flags, &key, this), then chains to base Action (so every Control click runs G17).
- **G22 — Toggle/button machine.** Press: IsPressed=1, capture, consume silently (return 1, key forced 0). Hold: per-tick sticky re-dispatch with masked-0 flags tracks the live cursor in/out of the rect, popping IsPressed. Release: not-pressed → release bits stripped (no fire); pressed+inside → Kind 1 flips IsOn, Kind 2 latches IsOn=1 (never off), fire `ID|0x8000`; pressed+outside → no toggle but release bits NOT stripped (fires only in the no-intervening-idle-tick boundary case).
- **G23 — Hold-repeat is a mask property.** A held button repeats its ID every tick iff its Flags mask includes held bits (0x2/0x20) — no timer, no initial delay, no acceleration. GaugeClass thumb-drag is the live consumer; the sidebar strip scroll buttons (mask 0x55, no held bit) do NOT repeat — they fire once per click on release and scroll a page (gadget-core §10; contradicts SIDEBAR_TIMING_AND_TOOLTIPS §5.3's mechanism claim — flagged DRIFT there).
- **G24 — Destruction.** ~GadgetClass clears g_KeyboardFocus (incl. the gadget's 0x100 bit), g_StickyFocus and g_CurrentGadgetList if they point at the dying gadget, but NOT g_HoveredGadget; then ~LinkClass unlinks with neighbor repair. Delete_List rewinds to head and destroys forward, capturing Next before each delete.
- **G25 — Modal/list-swap hygiene.** Clear_Attached_List (+0x38) zeroes only g_CurrentGadgetList, guaranteeing the next Input call takes the G5 reset path — the documented way the engine swaps gadget pages.

### 5.2 O-series — orchestration spine [evidence: gscreen-chain lane §§1–7, 9]

- **O1 — Single chain object.** One static instance @ 0x0087F7E8 (GScreen→Map→Display→Radar→Power→Sidebar→Tab→Scroll→Mouse), final vtable 0x007E1964, built by CRT static init 0x0040D190 before WinMain; never reconstructed (only re-Init'd).
- **O2 — Input stage.** Main_Tick calls GScreenClass::Input (chain slot 9) once per gameplay tick @ 0x0055D8AB, gated by `(SpecialFlags&2)==0 && g_GameState==0 && g_GameRunning`. Mouse x/y from WWMouse; key from the gadget list if Buttons≠0 (full 32-bit result, 0x8000|ID protocol), else keyboard Check/Get masked to 16 bits.
- **O3 — Gadget surface swap.** Gadget Input runs with the draw-target global [0x00887314] temporarily set to HiddenSurface [0x0088730C]; restored immediately after.
- **O4 — Pre-input dirty propagation.** If any gadget IsToRedraw, Input calls chain Flag_To_Redraw(0) → sets g_Tactical+0xD7D only (no full chrome repaint).
- **O5 — AI cascade order is fixed:** Mouse(cursor anim) → Scroll(edge scroll) → Tab(button-ID hub + credits tick) → Sidebar(strips/tabs/scroll/repair-sell) → Power(bar anim) → Radar → Display(tactical) → GScreen(decay). Key pointer shared down the chain; x/y a copy.
- **O6 — Button-ID consumption is layer-local:** 0xF0/0xF1 and command bar 0xD6..0xEE in TabClass; 0x65/0x66, 0xC8/0xC9, 0xCB..0xCE in SidebarClass; all effects UI-state only. Sim-affecting clicks become queued events only.
- **O7 — Buttons-list lifecycle:** objects/IDs/positions initialized by the Init_IO chain; REGISTERED dynamically at tail via Add_A_Button on activate/tab-switch/toggle; head cleared by One_Time/base Init_IO. Hit-test priority and draw order both = insertion order.
- **O8 — Rebuild events:** video-mode change (0x00560BF0) and load-game (0x0067E440) re-run Init_IO + TabClass::Activate(1) + InitSurface; scenario/session start runs Activate via 0x00684C30 / Main_Game.
- **O9 — Frame order (gameplay tick), CORRECTED:** Input → Process_Command (hotkeys) → Map__Logic → RenderFrame_main → record/playback service → **LogicClass::Update 0x0055AFB0 (the object-sim pass)** → sound/scroll service → **queued-command execution: FUN_00647260 → FUN_0064C380 → EventClass::Execute 0x004C6CB0** → Network_ServiceLoop → pause gate → frame-counter++ → frame-limiter 0x0055E160. **Render precedes the object-sim tick (screen shows tick N−1); player commands execute AFTER the object-sim pass, NOT inside LogicClass::Update** — the gscreen-chain lane's "(incl. queued-event execution)" phrasing is superseded (verdict `queued-events-in-logic-update`; live calls in [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §3, incl. decompile 0x0055D360 and the 0x0053B560 label-drift confirmation).
- **O10 — Render composition:** cursor-prep → tactical pass0/pass1 → chain Draw_It (TabClass::Draw_It 0x006D0A20 → sidebar chrome, sandwiched) → tactical pass2 → conditional sidebar blit → gadget Draw_Me(0) walk → chat overlay → tooltip draw ([0x00887368] slot 3) → cursor restore → present.
- **O11 — Network bonus UI passes:** in modes 3/4 the frame limiter 0x0055E160 may run extra {Input, Process_Command, TacticalClass::Update, RenderFrame_main} sequences while waiting out the lockstep budget — UI responsiveness can exceed sim rate.
- **O12 — Framework-B coexistence (modal-over-game matrix).** While a shell dialog pumps via 0x00623120 — **offline (g_GameMode 0/5):** gadget input, AI cascade, render, object sim, frame counter ALL frozen (pump never calls Main_Tick); Win32 messages (dialog keys/paint/tooltips) stay live. **LAN/WOL (3/4):** object sim + frame counter ADVANCE (Main_Tick unconditional once entered), gadget input + render stay suppressed via the g_GameState gate; reentrancy byte DAT_00ABCD58 forbids nested Main_Tick. Dialogs repaint via WM_PAINT through the OS — the tactical surface beneath stays whatever the last RenderFrame_main left. [cited by §0]
- **O13 — Tooltip ownership:** hover timing + show/hide = ToolTipManager driven ONLY by the Win32 message pump; the engine frame loop only draws it. Tooltip cadence is wall-clock ms, not frames.
- **O14 — Sim isolation seam:** nothing below the queued-event boundary may read gadget/chain state; the AI cascade mutates UI globals only; the sim consumes commands via the O9 queue-execution point. Rust `sim/` keeps the same seam.

### 5.3 D-series — Framework B delta corrections (patch obligations for [SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md)) [evidence: dialog-delta lane §§1–5]

- **D1 — Record map convention.** The 0x208 record's data root = bucket+4; replace the prior doc's §2.4 mixed-convention map with §2.7's single-convention table; +0xB0 is one dual-role field; there is no +0xB4 "slide-eligible" field. (verdict `record-data-root-convention`)
- **D2 — Include-set truth.** Reposition/slide/paint-mode-1 gating = the 55-dialog-id set of 0x0060C540 (§2.9), NOT {0xE2,0x6B,0x100,0x102}; plus the disjoint 19-id mode-2 modal set of 0x00622820. Mode-2 modals can never slide. (verdict `include-set-55-ids`)
- **D3 — Keyboard routing is three-stage.** Registration-order IsDialogMessageA over the HWND vector → TranslateAcceleratorA over the {HACCEL,HWND} registry 0x00ABFCBC → optional message-filter hook 0x00ABFD34. The prior doc described stage 1 only. (verdict `kbd-routing-accel-registries`)
- **D4 — Result-channel naming.** The window long index used for the result pointer is **8 = DWL_USER**, not GWLP_USERDATA(−21); GWL_USERDATA carries the per-control override block (freed at WM_NCDESTROY). Read all prior "GWLP_USER" wording as DWL_USER(8).
- **D5 — Owner-draw button asset truth.** PCX names always `b{u,d}e_{li,mi,ri}{24,30}.pcx` (second char hardcoded 'e'); disabled = 50% black AlphaBlend, never `bud_*` art; asset-1 = SDBTNANM.SHP frames 2 idle / 3 flash (1 Hz hover timer) / 4 pressed; Voc on the u→d edge. (verdict `ownerdraw-button-assets`)
- **D6 — Two WM_INITDIALOG paths.** lParam≠0 (factory) runs the full subclass cascade but does NOT write the dialog id — the id is written by the init bridge 0x00622820 called from per-dialog procs; a proc that never calls the bridge keeps id=0 and falls to the centered/dbak6440 default path.
- **D7 — Pump precision.** 0x00623120 is the loop BODY (the loop lives in each owner); sim advances behind modals only in-game (O12); the front-end pump services network/UI only.

### 5.4 S-series — shared services (both frameworks)

- **S1 — Tooltip service.** One ToolTipManager singleton [0x00887368] serves both frameworks: 'TTIP' SetTimer, delay field this[+0x228] (1000 ms per SIDEBAR_TIMING doc — DOC-INHERITED value), auto-hide re-arm this[+0x230], **inclusive-both-edges** rect test (deliberately ≠ the gadget half-open rule), kill-on-button-press, suppression byte 0x00A8F7D8. (§2.8)
- **S2 — Open-dialog counter.** 0x00A8ED8C (++ INITDIALOG / −− DESTROY) is READ by in-game code (GScreen flip 0x004F4B5A, sidebar 0x006A6A00) — the Rust shell service must expose an "any dialog open" query to the in-game layer.
- **S3 — Keyboard singleton.** WWKeyboard [0x0087F770] (queue + Down-polling + cached event coords) is the shared input source for both frameworks; the modifier word (G9) is derived from it.
- **S4 — Surface discipline.** Gadget drawing targets the hidden surface via the [0x00887314] swap (O3); shell composition targets Alternate 0x00887310 with a front blit to 0x00887308 when paint-depth returns to 0.
- **S5 — No RNG in UI.** No RNG entry-point calls appear in any UI body decompiled across the lanes (Input, Hit_Test, Sticky_Process, factory, teardown, pump, wndproc, tooltip hook) — scope-limited negative (§9); UI must never consume sim RNG streams in the Rust port.

## 6. Rust-native replacement boundary (service design)

Design (not yet an approved plan). Rust-native structure, gamemd-native semantics: no vtables, no raw-pointer globals — but every §5 clause reproduced observably.

### 6.1 New service: `ui::gadget` (Framework A substrate)

Sits beside the existing `ui::shell`; owns the in-game chrome input+draw authority.

- **`GadgetList`** — retained, ordered list (Vec-backed, stable handles; insertion order = hit-priority order = draw order, per O7/G20). API mirrors the verified mutation set: `add_after`, `add_tail`, `add_head` (each self-removes first, G2), `remove` (neighbor repair + focus clear, G3/G18), `extract_by_id`, `clear_list`.
- **`FocusState`** — ONE struct replacing the four gamemd globals: `sticky: Option<GadgetHandle>`, `keyboard: Option<GadgetHandle>`, `hovered: Option<GadgetHandle>`, `current_list: ListId`. Fresh-list reset per G5. **Removal/destruction clears hover too** — deliberately closing the G7 stale-pointer hazard (observable behavior unchanged: gamemd never visibly exercises the hazard because lists die only between modal loops).
- **Event-flag dispatch** — one `tick(list, &mut FocusState, input: &InputSnapshot) -> Option<FiredId>` reproducing the Input contract exactly: G6 coordinate source, G8 flag assembly (held bits idle-only), G9 modifier word, G10 tier precedence, G11/G12 draw cadence, G13 `ID|0x8000`/`|0x4000` result protocol, G14 hit-test (half-open, smallest-area, `<=` tie-break, 786,432 px² seed), G15 filtering, G17 capture, G22 button machine, G23 mask-property repeat.
- **Gadget behaviors as plain Rust** — a `GadgetBehavior` enum/trait family covering only the 9 live classes (§3.3): `ShapeButton` (ToggleClass machine + shape draw + flash via the existing `gadget_flash.rs` primitive), `Cameo` (SelectClass: Mouse_Enter/Leave tooltip hooks; draw stays with the strip painter), `ClickRegion` (SBGadget/tactical/minimap: invisible, Action-only, sticky), `TextLabel` (chat/system lines). The dormant TS wing is NOT ported.
- **Draw integration** — the service emits draw requests in walk order into the existing sidebar/chrome renderer; per-frame dirty sweep mirrors the dual pump (A10): tick-walk draw + frame draw-if-dirty.

### 6.2 Ownership boundaries (state → owner; from globals-registries §8)

| gamemd state | Rust owner |
|---|---|
| 4 focus globals + Buttons head | `ui::gadget::{FocusState, GadgetList}` |
| dialog LIFO + routing vector + accel registry + open-dialog counter | `ui::shell::DialogController` (counter exposed as an `any_dialog_open()` query for the in-game layer — S2) |
| owner-draw hashtables + 0x208 records | per-`DialogInstance` control map (process-global tables are an HWND-keying artifact) |
| surfaces / swap pattern | `render` target parameter, not a mutable global (S4) |
| **ToolTipManager + suppression byte** | **one `app::Tooltips` service SHARED by `ui::gadget` and `ui::shell`** (S1): wall-clock delay/auto-hide, inclusive-edge rects, kill-on-press; both UIs register rects; the renderer draws last (O10) |
| WWKeyboard + modifier pairs | existing `input` layer (S3/G9) |
| click sounds | rules-driven `audio` lookups (RulesClass fields), invoked from UI handlers, never sim |

### 6.3 The sim seam (binding rule, from gscreen-chain §4.2 / O14)

The gadget layer (and the whole AI-cascade equivalent) mutates **UI state only**: tab/scroll/flash/camera/selection/targeting modes. Sim-affecting clicks produce queued `Command`s consumed by the sim tick — matching gamemd's queue-execution point (O9) and the project's #1 invariant. `sim/` never sees gadget state; the existing `app_commands::*` path already has this shape (rust-current §2.5) and is kept.

### 6.4 Explicit non-goals / policy carve-outs

- The RON-tunable sidebar geometry (`sidebar/layout_spec.rs`) deliberately diverges for the 20k-unit/30-player scale target; pinning retail strip geometry is a separate POLICY DECISION (R11) — the gadget service must not silently impose retail layout.
- WOL/LAN dialog wing: enumerated (§3.5), not implemented by default.
- TS shell-control wing + dropship loadout: not ported (§3.3/§3.4).

## 7. Old ad-hoc Rust logic to retire

From rust-current lane §3 (file:line verified at commit 7b79a186), ordered by blast radius. "Superseded by" names the §5 clause(s) the replacement must satisfy and the §8 slice that lands it.

| # | File:lines | What it does today | Superseded by | Risk |
|---|---|---|---|---|
| R1 | `src/ui/in_game_hud.rs` (whole file, 210 ln) | egui build palette; `draw_in_game_hud` has ZERO callers | delete outright (no clause needed) — verify with a build | none (dead code) |
| R2 | `src/app.rs:1556-1578` mirrors + per-shell pressed/hovered fields (main_menu_shell/state.rs:62-70, single_player_shell/state.rs:52-57) | duplicate press/hover state for render | render reads `DialogController` directly (finishes B-track Slice 3; §4.2 D-B4) | low |
| R3 | `src/app.rs:1394-1478` skirmish bespoke press-release gesture + `hit_test_owner_draw_button` (skirmish_shell/state/hit_test.rs:292-331) | hand-rolled press-must-match-release for 3 owner-draw buttons | `DialogController` feed / B-track Slice 4 | low-medium (keep down-sound + modal pre-empts) |
| R4 | `src/ui/single_player_shell/layout.rs:55-146` + `src/ui/skirmish_shell/layout.rs:380-412` | per-shell layout passes off the descriptor table | new `AnchorRule` variants + 0x100/0x102 descriptor tables (B-track) | medium (golden rect tests) |
| R5 | `skirmish_shell/state/combos.rs:635-727`, `state/trackbars.rs:227-334`, `state/player_name.rs` | the 0x102 control set as bespoke state machines | B-track Slice 4 (`ControlKind` behaviors; D-series + owner-draw census §2.6) | HIGH (87-test net must stay green) |
| R6 | combo dropdown scrollbar (combos.rs:643-712, 200-260) vs choose-map listbox scrollbar (skirmish_shell/layout.rs:593-732) | duplicated thumb/track/arrow math | one substrate ScrollBar control (B-track Slice 4); hold-repeat per G23-analogue (Win32 semantics, not gadget) | medium |
| R7 | `src/sidebar/mod.rs:379-425` `hit_test` + the separately-ordered draw stack in `app_sidebar_render`/`render/sidebar_chrome` | hit order and draw order as two hardcoded lists | `ui::gadget` retained list — ONE order for hit + draw (G14/G20/O7) — Slices A1–A3, finished in A6 | medium-high (preserve click outcomes) |
| R8 | `src/app_input.rs:39-43, 227-238` | sidebar actions fire on mouse-DOWN | G22 fire-on-release + drag-off cancel — Slice A1 | **player-visible DRIFT-correcting change** (§4.2 D-A1) |
| R9 | `src/ui/pause_menu.rs` + options egui in `main_menu_dialogs.rs` | egui stand-ins for Options | `ModalKind::InGameOptions` 0xBBB/0xF5 (B-track Slice 5b; modal.rs already models results) | high visibility |
| R10 | `main_menu_dialogs.rs` movies/credits + campaign egui panels | egui stand-ins | Framework-B dialogs (ids in §3.5; decode before migrating) | keep egui until decoded |
| R11 | `src/sidebar/layout_spec.rs` RON spec + `sidebar/mod.rs:273-333` adaptive rows | modern adaptive sidebar geometry | retail strip geometry ONLY IF the user signs off (§6.4 policy) | POLICY — do not retire unilaterally |
| R12 | `main_menu_shell/layout.rs:282-325` `compute_responsive_layout` | stretch-to-window drift mode | drop or keep as explicit non-parity config; verify no input path consumes it | low |
| R13 | `src/app.rs:1950-1955` `close_main_menu_dialogs` (0x120 instance never `pop()`ed; clobbered later by `reset_to`) + Esc bypass app.rs:2106-2112 | controller-stack bypass on exit-confirm Esc | proper LIFO pop + focus restore (D-series / prior doc C5) — B-track | low (internal consistency) |

**Keep (faithful primitives):** `sidebar/gadget_flash.rs`, `app_sidebar_gadgets.rs`, `sidebar/power_bar_anim.rs` (fix its `SLIDE_TICKS_PER_STEP` placeholder instead), `skirmish_shell/static_reveal.rs`, all of `ui/shell/*`.

## 8. Migration slices + acceptance tests (A-track)

Shadow-first: each slice lands the substrate piece running in parallel (asserting agreement) before flipping authority — same pattern as the shipped B-track slices. Acceptance tests cite §5 clause IDs; all are headless `ui::gadget` unit/integration tests except the marked manual checks.

| Slice | Scope | Retires | Acceptance tests |
|---|---|---|---|
| **A0** | `ui::gadget` core: `GadgetList` + `FocusState` + event-flag tick. No surface wired — pure substrate + tests | — | G1–G6, G8–G21, G24–G25 each get a direct unit test (list mutation set; fresh-list reset; flag assembly incl. held-bits-idle-only; tier precedence; tie-break incl. equal-area-later-wins and the 786,432 px² seed; capture acquire/release; focus steal; result protocol incl. `\|0x4000`); G7-closure test: removing the hovered gadget clears hover, no Leave on a dead handle |
| **A1** | Sidebar buttons (4 tabs, repair, sell, 2 strip scroll) as ShapeButton gadgets; fire-on-release authority flip | R8; part of R7 | G22 (silent press; fire on release-inside; drag-off cancel; Kind 1 vs 2), G23 (mask 0x55 → NO per-tick repeat, one page per click), G17 capture during press-hold; manual: every sidebar click outcome identical to pre-flip build |
| **A2** | Cameo strip: 60-slot-per-tab SelectClass-equivalents; registration swap on tab switch | part of R7 | O7 (registration order = hit+draw order), G12 walk semantics over the strip, tab-switch remove/add swap (gscreen-chain §3.2 visible-count formula), Mouse_Enter/Leave hooks fire on hover change (G7) |
| **A3** | Invisible click regions: full-tactical catcher (flags 0x7F analogue) + minimap region (0x9F) routed through the same walk | part of R7; app_input tactical/minimap special-casing | A8-parity: tactical + minimap clicks resolve through the gadget walk with sticky capture; G14 ordering between overlapping catcher/sidebar rects; G15 sticky bypass on masked-0 ticks |
| **A4** | `app::Tooltips` shared service (S1): wall-clock delay, inclusive-edge rects, kill-on-press; sidebar + command-bar rect registration; renderer draw hook last (O10) | — (new GAP fill, §4.2 D-A2) | S1 (delay before show; auto-hide; inclusive edges — boundary-pixel test vs the gadget half-open rule; press kills tip), O13 (cadence decoupled from sim tick), S2 query consumed by the in-game layer |
| **A5** | Chat/system TextLabel surface: runtime label gadgets, 14-slot list, wrap, Voc on insert | — (GAP fill, §4.2 D-A5) | §3.1 contract (heap per message; 14-slot cap; insert sound); draw order in the walk (G20); labels never consume clicks (mask) |
| **A6** | Command bar (25 buttons + 2 toggles) + dual-draw-pump alignment (tick walk + per-frame dirty sweep); delete remaining parallel hit/draw lists | rest of R7; R1 cleanup build | O6 layer-local ID consumption (0xF0/0xF1, 0xD6..0xEE); A10/G20 dual pump (dirty-only frame sweep test); end-to-end golden input-replay: a scripted click session produces identical UI state vs the A5 build |
| **B-track (coexistence)** | `ui::shell` continues its own plan in parallel: Slice 4 (skirmish 0x102 controls; R3–R6), Slice 5b (options 0xBBB/0xF5; R9), mirror retirement (R2), keyboard routing D3 (R13). Shared touchpoints with the A-track: the A4 tooltip service (B consumes it for shell tooltips per the 0x006040B0 map) and the S2 open-dialog query (gadget layer suppresses input per O12 when a dialog is open) | — | O12 matrix test: with a (Rust) modal open offline, gadget tick + render + sim frozen, tooltips live; D-series tests live in the B-track plan docs |

Ordering rationale: A0 is pure substrate (no player-visible change, maximal test surface); A1 flips the single most player-visible DRIFT (fire-on-DOWN, §4.2 D-A1) first; A4/A5 are pure additions (GAPs) that don't destabilize existing behavior; A6 ends with the one-retained-list invariant that prevents the hit-vs-draw divergence class permanently.

## 9. UNVERIFIED (YELLOW)

Everything below is uncertain, identity-MEDIUM, or session-unlogged. None of it appears as fact in §§1–8; if a §5 clause depends on one of these, the clause says so. Harvested from all seven lane files.

### 9.1 Identity / binding open items (Framework A + spine)

- **FUN_00647260 / FUN_0064C380 names.** The edges and Main_Tick position are VERIFIED ([verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §3); the names "Queue_AI"/"DoList" are INFERRED from RA-source shape. Bodies not decompiled.
- **"LightningStorm__Process" 0x0053A6C0** — the in-LogicClass::Update wrapper that calls 0x0053B560: call edge verified (gscreen-chain §4.2); the WRAPPER's body/label remain unverified. (0x0053B560 itself IS now decompiled — screen-flash machine, label drift confirmed, [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §3.6.)
- **Process_Command 0x0055DEE0 = "keyboard hotkey processor"** — identity MEDIUM (callee shape only: Keyboard Check/Get + handlers; gscreen-chain §5/§8).
- **TextButtonClass DORMANT-leaning** — zero direct/imm32 callers, but a non-disassembled caller cannot be excluded (gadget-family §3.5/§8).
- **Dormancy scan residual risk:** the §3.2 byte scan covers E8/E9 transfers + imm32 only; a register-indirect ctor call with a computed address would evade it (implausible for VC++6, not excluded). Family completeness rests on no class overriding Input+Clicked_On+Get_Next simultaneously (gadget-family §8).
- FUN_00648350 / FUN_00648710 — two non-Main_Tick callers of GScreenClass::Input (radar region); roles unknown (gscreen-chain §8).
- WWMouse vtable slot roles (+0x2C/+0x30 Get_X/Y; +0x3C/+0x40 cursor draw/restore) — inferred from consumption shape; bodies not decompiled. 0x00887640 identity = MEDIUM (WinMain write site not decompiled).
- FUN_00684C30 = scenario-start initializer (region + call pattern only). Map__Logic 0x004D2370's actual role (tiny; 2 cell lookups). FUN_0072F430 gate in TabClass::Init_IO. DAT_00A8B538 gate on 0x80F1.
- Sidebar single-button global↔role bindings (which of 0x00B07DF8/0x00B0B328/0x00B0B3A0/0x00B0B408 is sell/repair/scroll-up/scroll-down) — partially DOC-INHERITED; commandbar singles 0x00B0CCB0 vs 0x00B0CC40 ↔ IDs 0x80F0/0x80F1 — INFERRED from ID adjacency (gadget-family §8).
- ScenarioClass+0x34D0 = StartingDropships count — INFERRED from string adjacency; parse site not traced.
- GaugeClass true slot count (≥42 implied); its extended pixel↔value virtuals inferred from call shape. Radar ShapeButton pair's player-visible roles. MessageList Add-path caller sets beyond chat. FUN_0069DFF0 (SidebarClass::Init post-button helper). 0x0069DE00 Set_Shape tail (first 48 bytes decoded only).
- keyboard(md).ini runtime remap of the OptionsClass modifier pairs — only SetDefaults writes found; indexed INI-driven writes would be invisible to the census (gadget-core YELLOW).
- StripClass::AI scroll-animation cadence ("one row per tick" player claim) — not re-traced; only SIDEBAR_TIMING §5.3's *mechanism* claim is refuted (G23).
- FUN_00565800 / FUN_004A8930 owner classes in the Init_Clear chain; the 0x006007xx–0x00600Cxx unbounded duplicate hashtable insert/remove family; ToolTipManager construction sites 0x007777B8/C3/0x00777803 (unbounded region); 0x00A8D60E / 0x00A8DAB4 precise meanings; QI FUN_004F4240's host vtable + its two GUIDs; companion-field layout of hashtables 0x00AC18C0/0x00AC1B48 (by analogy only) (globals-registries §7).
- **S5 RNG negative is scope-limited:** "no RNG in UI" proven only for the bodies decompiled across the lanes, not corpus-wide (globals-registries §6.3).

### 9.2 Framework B open items (dialog-delta §9)

- DLU→pixel constants MulDiv(6,4)/(13,8) + the 1-px finalizer FUN_0060B950 rows — DOC-INHERITED, not re-read.
- Pressed-text offset deltas (+2y/+1x) in 0x00612B70 — present but exact values not recovered (needs disassembly-level read).
- FUN_0060C7D0 presumed "center non-include dialogs" — not decompiled. Per-dialog WM_COMMAND→result maps beyond 0xE2/0x100/0x102. C13 modal template-id selection (0xCE/0x120/0x121 per caller) — register-passed, UNCHECKED.
- FUN_0069BBE0 receiver binding (in-game gate byte +0x30D8) — content verified, binding MEDIUM. Dialog-id selection 0xBBB/0xF5 in the in-game Options path — DOC-INHERITED assembly cite, not seen in this study's decompile.
- Voc indices for click/slide sounds (register-passed); DAT_00B0FACC = MNBTTN.SHP and DAT_00B0F9EC identities — DOC-INHERITED. Accelerator-registry writers (0x005D4Cxx region) not traced. Ids 0x10B/0xD4/0xFB/0x73/0xA3/0xD6/0xD7/0xD8 naming unconfirmed. Sidebar side-switch tooltip re-registration detail — confidence MEDIUM (xref list only).
- S1's 1000 ms delay VALUE is DOC-INHERITED (SIDEBAR_TIMING_AND_TOOLTIPS); the delay FIELD (+0x228) and mechanism are verified.

### 9.3 Study-session-only citations (original tool log not saved)

Flagged [S] in [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §1; the facts stand on lane-grade evidence, the listed *extra* calls do not: `list_segments`/`disassemble 0x004E15A0` (hit-test seed .rdata residency — #3); `decompile 0x00621B80` + `read_memory 0x0083587C..` (owner-draw PCX strings — #14); the binary-wide +0xB4 zero-writer byte scan (#16).

### 9.4 Rust-lane unverified (rust-current §5)

skirmish hit_test.rs:97-99's "statics never overlap widgets" claim (asserted in a comment, not checked across all resolutions); `compute_responsive_layout` render-path reachability; the 87-test safety net's semantic coverage (count verified, content not re-read); `power_bar_anim.rs` `SLIDE_TICKS_PER_STEP = 9` placeholder; the validation-modal 450×325 size candidate; whether gamemd's 0x102 status-help line is immediate or delayed (current Rust = immediate).

## 10. Sources

### 10.1 Lane worknotes (this study, `docs/research/substrate/worknotes/gadget-dialog-20260610/`)

1. [gadget-core.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/gadget-core.md) — GadgetClass/LinkClass base machinery; Input/Hit_Test/Clicked_On/Toggle decodes; G-contract source. (One claim overturned: §9.1 ShapeButton +0x84 — corrected in §2.2 herein, [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) C1.)
2. [gadget-family.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/gadget-family.md) — 20-class census, vtable matrix, instantiation byte-scan, live population.
3. [gscreen-chain.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/gscreen-chain.md) — chain identity, Buttons lifecycle, AI cascade, Main_Tick frame order, coexistence, tooltip ownership; O-contract source. (One claim overturned: §4.2 command-entry point — corrected in §1.A11/§5-O9, [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) C3; its §5 step list omits the 0x00647260 call, reconciled in [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) §3.5.)
4. [dialog-delta.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/dialog-delta.md) — Framework B fifteen-row delta vs the 2026-05-31 shell study; record map; id tables; owner-draw census; D-contract source.
5. [rust-current.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/rust-current.md) — Rust architecture map at commit 7b79a186; gap/retire/slice-status source (§4/§7).
6. [globals-registries.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/globals-registries.md) — cross-cutting singleton/state ledger, writer censuses, ToolTipManager decode, service-boundary proposal.
7. [verification-pass.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/substrate/worknotes/gadget-dialog-20260610/verification-pass.md) — the adversarial-pass verdict ledger (17 verdicts + 3 overturns → MCP calls), incl. this study's 2026-06-10 live re-verification log (command-entry chain, ShapeButton vtable, 0x0053B560 label drift).

### 10.2 Prior authority docs

- **[SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) (2026-05-31)** — remains the Framework-B authority for everything not contradicted here; its ten §1 responsibilities re-verified this study (§1.2). **Open patch obligations (apply §5-D in a follow-up /audit): D1 (§2.4 record map → single convention), D2 (§C7 include-set → 55+19 ids), D3 (keyboard routing → three-stage), D4 (GWLP_USER wording → DWL_USER(8)), D5 (owner-draw asset truth).**
- **[GADGET_UI_FRAMEWORK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GADGET_UI_FRAMEWORK_GHIDRA_REPORT.md) (2026-04-22)** — SUPERSEDED by this study where contradicted; enumerated supersessions in §2.10: GaugeClass/Dial8Class existence, 33-slot vtable end, Set_Position naming, held-bits idle-only, 0x8065/0x8066 routing, seed-constant identity, ControlClass ctor defaults. Still valid as navigation for the uncontradicted remainder.

### 10.3 Secondary docs (DOC-INHERITED inputs, not re-verified here)

`GSCREEN_RTACTICAL_GHIDRA_REPORT.md` (drift ledger in gscreen-chain §10 — needs its own /verify-doc pass), [MODAL_PUMP_00623120_SERVICE_TICK_CONTRACT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MODAL_PUMP_00623120_SERVICE_TICK_CONTRACT_GHIDRA_REPORT.md) (matched live), [SIDEBAR_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/SIDEBAR_SYSTEM_GHIDRA_REPORT.md) (global map; §15 event posting), [SIDEBAR_TIMING_AND_TOOLTIPS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_TIMING_AND_TOOLTIPS_GHIDRA_REPORT.md) (1000 ms tooltip value; its §5.3 scroll-repeat mechanism claim is DRIFT per G23), [MINIMAP_GADGETCLASS_CLICK_PROVENANCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MINIMAP_GADGETCLASS_CLICK_PROVENANCE_GHIDRA_REPORT.md), [BUTTON_FADE_EFFECT_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUTTON_FADE_EFFECT_TRIGGER_GHIDRA_REPORT.md), [SHELL_UI_SOUND_PLAYBACK_PLUMBING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_UI_SOUND_PLAYBACK_PLUMBING_GHIDRA_REPORT.md), skirmish-ui owner-draw family docs, `docs/plans/2026-06-01-shell-substrate-slice4-kickoff.md`, `docs/plans/2026-06-02-shell-substrate-slice5b-options-plan.md`.
